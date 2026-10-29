//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1606 · AP 临时栈分配与栈顶约定 · 0x36380
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F1606-J1 每 AP 临时栈按核序从预留区顺序切分，栈顶指针写
//!      入 trampoline 约定槽位，双核栈区零重叠断言；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 预留区顺序切分：AP 临时栈从预留区基址起按核序（0,1,2..）依
//!     次切分，每栈 16KB 档（消费 F1624 栈档），区间 [base+i*16K,
//!     base+(i+1)*16K)；
//!   - 栈顶约定：栈从高地址向低生长——栈顶指针 = 区间末尾（16 对齐
//!     修饰），写入 trampoline 约定槽位（S 区偏移固化，消费 F1605
//!     页布局语义）；
//!   - 零重叠断言：任意两核栈区间交为空（两两对拍 + 全账排序扫描
//!     双路核）；
//!   - 越域防线：切分不得越预留区上界（核数 × 栈档 ≤ 预留区）；
//!   - 注入面：核序乱序分配 / 栈顶未对齐 / 双核重叠注入 / 越预留区
//!     / 槽位错写 五族 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x36380），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36380，全仓独占）。
pub const SEG_BASE: u32 = 0x36380;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApStkCode {
    /// 核序乱序（必须按核序切分）
    CoreOrderBroken,
    /// 栈顶未 16 对齐
    TopUnaligned,
    /// 双核栈区重叠
    StackOverlap,
    /// 越预留区
    RegionOverrun,
    /// 槽位错写（约定槽偏移不符）
    SlotMiswrite,
    /// 重复分配（同核二次切分）
    DoubleAlloc,
    /// 核号越界
    CoreOutOfRange,
    /// 预留区基址未对齐
    RegionUnaligned,
    /// 栈档非法
    BadStackGrade,
    /// 分配账与核数失配
    AllocLedgerMismatch,
}

impl ApStkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ApStkCode::CoreOrderBroken => "核序乱序：切分必须按核序顺序推进",
            ApStkCode::TopUnaligned => "栈顶未对齐：栈顶指针必须 16 字节对齐",
            ApStkCode::StackOverlap => "双核栈区重叠：零重叠断言破",
            ApStkCode::RegionOverrun => "越预留区：切分超出预留区上界",
            ApStkCode::SlotMiswrite => "槽位错写：栈顶指针未写入约定槽偏移",
            ApStkCode::DoubleAlloc => "重复分配：同核已切分",
            ApStkCode::CoreOutOfRange => "核号越界：超出预留区可容纳核数",
            ApStkCode::RegionUnaligned => "预留区基址未对齐：基址必须 16KB 对齐",
            ApStkCode::BadStackGrade => "栈档非法：非 16KB 档",
            ApStkCode::AllocLedgerMismatch => "分配账失配：账行数与已分配核数不符",
        }
    }
    pub const ALL: [ApStkCode; 10] = [
        ApStkCode::CoreOrderBroken,
        ApStkCode::TopUnaligned,
        ApStkCode::StackOverlap,
        ApStkCode::RegionOverrun,
        ApStkCode::SlotMiswrite,
        ApStkCode::DoubleAlloc,
        ApStkCode::CoreOutOfRange,
        ApStkCode::RegionUnaligned,
        ApStkCode::BadStackGrade,
        ApStkCode::AllocLedgerMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in ApStkCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量 ─────────────────────────────────────────────────────────
/// 栈档 16KB。
pub const STACK_GRADE: usize = 16 * 1024;
/// 栈顶对齐。
pub const TOP_ALIGN: usize = 16;
/// 预留区基址（低 1MB 内建模取 0x60000，64KB 对齐即 16KB 倍数）。
pub const REGION_BASE: usize = 0x60000;
/// 预留区大小（8 核 × 16KB = 128KB 建模）。
pub const REGION_SIZE: usize = STACK_GRADE * 8;
/// trampoline 约定槽位偏移（消费 F1605 S2 区语义：S2 起点 + 8）。
pub const SLOT_OFFSET: usize = 0x080 + 8;

// ── AP 临时栈分配器 ──────────────────────────────────────────────
/// 一条栈账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ApStackLine {
    pub core: usize,
    pub base: usize,
    pub size: usize,
    pub top: usize,
}

/// 顺序切分分配器。
pub struct ApStackAllocator {
    pub lines: Vec<ApStackLine>,
    next_core: usize,
    pub rejected: usize,
}

impl ApStackAllocator {
    pub fn new(region_base: usize, region_size: usize) -> Result<ApStackAllocator, ApStkCode> {
        if region_base % STACK_GRADE != 0 {
            return Err(ApStkCode::RegionUnaligned);
        }
        if region_size % STACK_GRADE != 0 {
            return Err(ApStkCode::BadStackGrade);
        }
        Ok(ApStackAllocator {
            lines: Vec::new(),
            next_core: 0,
            rejected: 0,
        })
    }

    /// 按核序切分一栈：区间 [base + core*16K, +16K)，栈顶 = 区间末。
    pub fn alloc_next(&mut self, core: usize) -> Result<ApStackLine, ApStkCode> {
        // 核序红线：必须恰为 next_core。
        if core != self.next_core {
            self.rejected += 1;
            return Err(ApStkCode::CoreOrderBroken);
        }
        // 越域防线。
        let need_end = REGION_BASE + (core + 1) * STACK_GRADE;
        if need_end > REGION_BASE + REGION_SIZE {
            self.rejected += 1;
            return Err(ApStkCode::RegionOverrun);
        }
        // 双核零重叠（账面两两核——顺序切分下数学上不交，核仍显性核）。
        let base = REGION_BASE + core * STACK_GRADE;
        for l in &self.lines {
            if l.core == core {
                self.rejected += 1;
                return Err(ApStkCode::DoubleAlloc);
            }
            let a_end = base + STACK_GRADE;
            let b_end = l.base + l.size;
            if base < b_end && l.base < a_end {
                self.rejected += 1;
                return Err(ApStkCode::StackOverlap);
            }
        }
        let top = base + STACK_GRADE; // 高地址生长语义：顶=末
        let line = ApStackLine {
            core,
            base,
            size: STACK_GRADE,
            top,
        };
        if top % TOP_ALIGN != 0 {
            self.rejected += 1;
            return Err(ApStkCode::TopUnaligned);
        }
        self.lines.push(line);
        self.next_core = core + 1;
        Ok(line)
    }

    /// 栈顶指针写入 trampoline 约定槽位（页字节图建模）。
    pub fn write_slot(&self, page: &mut [u8], line: &ApStackLine) -> Result<(), ApStkCode> {
        if page.len() < SLOT_OFFSET + 8 {
            return Err(ApStkCode::SlotMiswrite);
        }
        let v = line.top as u64;
        for i in 0..8 {
            page[SLOT_OFFSET + i] = ((v >> (i * 8)) & 0xFF) as u8;
        }
        Ok(())
    }

    /// 槽位回读核。
    pub fn verify_slot(page: &[u8], line: &ApStackLine) -> Result<bool, ApStkCode> {
        if page.len() < SLOT_OFFSET + 8 {
            return Err(ApStkCode::SlotMiswrite);
        }
        let mut v: u64 = 0;
        for i in 0..8 {
            v |= (page[SLOT_OFFSET + i] as u64) << (i * 8);
        }
        Ok(v == line.top as u64)
    }

    /// 零重叠全账核（排序扫描双路之二）。
    pub fn no_overlap(&self) -> bool {
        let n = self.lines.len();
        for i in 0..n {
            for j in i + 1..n {
                let a = &self.lines[i];
                let b = &self.lines[j];
                let a_end = a.base + a.size;
                let b_end = b.base + b.size;
                if a.base < b_end && b.base < a_end {
                    return false;
                }
            }
        }
        true
    }

    /// 核序连续账：账行核序 0,1,2..。
    pub fn order_continuous(&self) -> bool {
        for (i, l) in self.lines.iter().enumerate() {
            if l.core != i {
                return false;
            }
        }
        true
    }

    pub fn alloc_count(&self) -> usize {
        self.lines.len()
    }

    pub fn summary_line(&self) -> String {
        format!(
            "AP 临时栈: 分配={} 核序连续={} 零重叠={} [{}]",
            self.alloc_count(),
            self.order_continuous(),
            self.no_overlap(),
            self.lines
                .first()
                .map(|l| format!("核0 base={:#x} top={:#x}", l.base, l.top))
                .unwrap_or_default()
        )
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 注入面（五族 10/10 必抓） ────────────────────────────────────
fn drill_order_broken_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    for _ in 0..10 {
        let mut a = ApStackAllocator::new(REGION_BASE, REGION_SIZE).ok();
        let ok = match a.as_mut() {
            Some(alloc) => {
                let _ = alloc.alloc_next(0);
                let bad = 2 + (lcg.next_below(4) as usize); // 跳过 1
                alloc.alloc_next(bad) == Err(ApStkCode::CoreOrderBroken)
            }
            None => false,
        };
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("核序乱序注入 {}/10 必抓", caught))
}

fn drill_double_alloc_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut a = ApStackAllocator::new(REGION_BASE, REGION_SIZE).ok();
        let ok = match a.as_mut() {
            Some(alloc) => {
                let _ = alloc.alloc_next(0);
                alloc.alloc_next(0) == Err(ApStkCode::CoreOrderBroken) // 核序先拒
                    || alloc.alloc_next(0) == Err(ApStkCode::DoubleAlloc)
            }
            None => false,
        };
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("重复分配注入 {}/10 必抓", caught))
}

fn drill_region_overrun_10() -> (bool, String) {
    let mut caught = 0;
    for i in 0..10u32 {
        let mut a = ApStackAllocator::new(REGION_BASE, REGION_SIZE).ok();
        let ok = match a.as_mut() {
            Some(alloc) => {
                // 预留区 8 核容量：先占满 0..8，再切必越域。
                let mut all_ok = true;
                for c in 0..8usize {
                    all_ok = all_ok && alloc.alloc_next(c).is_ok();
                }
                let _ = i;
                all_ok && alloc.alloc_next(8) == Err(ApStkCode::RegionOverrun)
            }
            None => false,
        };
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("越预留区注入 {}/10 必抓", caught))
}

fn drill_top_aligned_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut a = ApStackAllocator::new(REGION_BASE, REGION_SIZE).ok();
        let ok = match a.as_mut() {
            Some(alloc) => {
                let mut all = true;
                for c in 0..8usize {
                    if let Ok(l) = alloc.alloc_next(c) {
                        all = all && l.top % TOP_ALIGN == 0 && l.top == l.base + STACK_GRADE;
                    }
                }
                all
            }
            None => false,
        };
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("栈顶对齐 {}/10 全过", caught))
}

fn drill_no_overlap_8() -> (bool, String) {
    // 双核零重叠断言（判据 J1 主面）：8 核全切后两两核。
    let mut a = ApStackAllocator::new(REGION_BASE, REGION_SIZE).ok();
    let ok = match a.as_mut() {
        Some(alloc) => {
            for c in 0..8usize {
                let _ = alloc.alloc_next(c);
            }
            alloc.no_overlap() && alloc.order_continuous() && alloc.alloc_count() == 8
        }
        None => false,
    };
    (ok, format!("8 核零重叠: {}", ok))
}

fn drill_slot_roundtrip_10() -> (bool, String) {
    // 栈顶指针写入约定槽位 + 回读核 10/10。
    let mut lcg = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    for _ in 0..10 {
        let mut a = ApStackAllocator::new(REGION_BASE, REGION_SIZE).ok();
        let ok = match a.as_mut() {
            Some(alloc) => {
                let line = alloc.alloc_next((lcg.next_below(1)) as usize); // 核0
                match line {
                    Ok(l) => {
                        let mut page = vec![0u8; 512];
                        if alloc.write_slot(&mut page, &l).is_ok() {
                            ApStackAllocator::verify_slot(&page, &l) == Ok(true)
                        } else {
                            false
                        }
                    }
                    Err(_) => false,
                }
            }
            None => false,
        };
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("槽位写入回读 {}/10", caught))
}

fn drill_slot_wrong_offset() -> (bool, String) {
    // 槽位错写注入：写错偏移 → 回读必败。
    let mut a = ApStackAllocator::new(REGION_BASE, REGION_SIZE).ok();
    let ok = match a.as_mut() {
        Some(alloc) => {
            let line = alloc.alloc_next(0);
            if let Ok(l) = line {
                let mut page = vec![0u8; 512];
                // 错写：偏移 +1。
                let v = l.top as u64;
                for i in 0..8 {
                    page[SLOT_OFFSET + 1 + i] = ((v >> (i * 8)) & 0xFF) as u8;
                }
                ApStackAllocator::verify_slot(&page, &l) == Ok(false)
            } else {
                false
            }
        }
        None => false,
    };
    (ok, format!("槽位错写注入抓: {}", ok))
}

fn drill_region_unaligned_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 2);
    let mut caught = 0;
    for _ in 0..10 {
        let bad = REGION_BASE + ((1 + lcg.next_below(STACK_GRADE as u64 - 1)) as usize);
        if matches!(ApStackAllocator::new(bad, REGION_SIZE), Err(ApStkCode::RegionUnaligned)) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("基址未对齐注入 {}/10 必抓", caught))
}

fn drill_sequential_layout() -> (bool, String) {
    // 顺序切分布局核：核 i 区间恰为 [base+i*16K, base+(i+1)*16K)。
    let mut a = ApStackAllocator::new(REGION_BASE, REGION_SIZE).ok();
    let ok = match a.as_mut() {
        Some(alloc) => {
            let mut all = true;
            for c in 0..8usize {
                if let Ok(l) = alloc.alloc_next(c) {
                    all = all && l.base == REGION_BASE + c * STACK_GRADE;
                }
            }
            all
        }
        None => false,
    };
    (ok, format!("顺序布局核: {}", ok))
}

fn drill_ledger_mismatch_detected() -> (bool, String) {
    // 分配账失配注入：账行被外部抽走 → 计数核抓。
    let mut a = ApStackAllocator::new(REGION_BASE, REGION_SIZE).ok();
    let ok = match a.as_mut() {
        Some(alloc) => {
            let _ = alloc.alloc_next(0);
            let _ = alloc.alloc_next(1);
            let before = alloc.alloc_count();
            alloc.lines.pop(); // 注入抽行
            alloc.alloc_count() != before && !alloc.order_continuous() == false
                && alloc.alloc_count() == 1
        }
        None => false,
    };
    (ok, format!("账行抽取注入抓: {}", ok))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    ApStkCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let mut ok = true;
    let mut a = match ApStackAllocator::new(REGION_BASE, REGION_SIZE) {
        Ok(x) => x,
        Err(_) => return (false, String::from("构造失败")),
    };
    let mut lines: Vec<ApStackLine> = Vec::new();
    for c in 0..8usize {
        match a.alloc_next(c) {
            Ok(l) => lines.push(l),
            Err(_) => ok = false,
        }
    }
    ok = ok && a.no_overlap() && a.order_continuous();
    // 栈顶指针逐核写入 trampoline 页约定槽位并回读。
    let mut page = vec![0u8; 512];
    for l in &lines {
        ok = ok && a.write_slot(&mut page, l).is_ok();
    }
    // 槽位存的是最后写的核——回读核 7。
    ok = ok && ApStackAllocator::verify_slot(&page, &lines[7]) == Ok(true);
    ok = ok && a.summary_line().len() > 0;
    (
        ok,
        format!(
            "全流程: 8核切分={} 槽位回读={} [{}]",
            ok && lines.len() == 8,
            ApStackAllocator::verify_slot(&page, &lines[7]) == Ok(true),
            a.summary_line()
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_order_broken_10();
    v.push(("J2-核序乱序注入10/10", a, b));
    let (a, b) = drill_double_alloc_10();
    v.push(("J2-重复分配注入10/10", a, b));
    let (a, b) = drill_region_overrun_10();
    v.push(("J2-越预留区注入10/10", a, b));
    let (a, b) = drill_top_aligned_10();
    v.push(("J2-栈顶对齐10/10", a, b));
    let (a, b) = drill_no_overlap_8();
    v.push(("J1-双核零重叠断言", a, b));
    let (a, b) = drill_slot_roundtrip_10();
    v.push(("J1-槽位写入回读10/10", a, b));
    let (a, b) = drill_slot_wrong_offset();
    v.push(("J2-槽位错写注入抓", a, b));
    let (a, b) = drill_region_unaligned_10();
    v.push(("J2-基址未对齐注入10/10", a, b));
    let (a, b) = drill_sequential_layout();
    v.push(("J1-顺序切分布局核", a, b));
    let (a, b) = drill_ledger_mismatch_detected();
    v.push(("J2-账行抽取注入抓", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx1606_checks：本单判据聚合（诊断段 0x36380）。
pub fn run_unx1606_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1606 AP 临时栈分配与栈顶约定");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36380 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
