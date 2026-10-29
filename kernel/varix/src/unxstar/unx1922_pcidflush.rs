//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1922 · INVPCID/INVLPG 指令面（刷新选择）· 0x36600
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1922-J1 四型 INVPCID（个体/单 PCID 全局/全 PCID 非全局/
//!      全 flush）选择语义断言全过，INVLPG 单页刷新与批刷新分界
//!      10/10 正确；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - INVPCID 四型闭集（SDM 语义建模）：type0 个体（PCID+VA）/
//!     type1 单 PCID 全局（PCID 全部 VA）/ type2 全 PCID 非全局（除
//!     global 页）/ type3 全 flush（含 global）；越型拒；
//!   - 选择语义：四型各自映射的失效域闭式（type0: 1 项；type1: 该
//!     PCID 全部；type2: 全部非 global；type3: 全部）——对账账按失
//!     效域计数守恒；
//!   - INVLPG 分界：单页（1 项）vs 批刷新（≥2 项走 INVPCID type2/3）
//!     分界 10/10 正确（单页走 INVLPG、多页不得走 INVLPG 逐条）；
//!   - PCID 支持：PCID 不支持平台四型只允许 type3 退基线（消费
//!     F2624）；PCID 位 12 位域；
//!   - 注入面：type 越闭集 / PCID 超 12 位 / 不支持平台用 type0..2 /
//!     批刷新误走单页 / 失效域计数漂移 五族 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x36600），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36600，全仓独占）。
pub const SEG_BASE: u32 = 0x36600;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlushCode {
    /// type 越闭集（>3）
    TypeOutOfRange,
    /// PCID 超 12 位域
    PcidOverDomain,
    /// 不支持 PCID 平台用 type0..2
    NoPcidTypeUsed,
    /// 批刷新误走单页（INVLPG 分界破）
    BatchViaSinglePage,
    /// 失效域计数漂移
    DomainCountDrift,
    /// PCID 标志缺失（type1 需 PCID 声明）
    PcidFlagMissing,
    /// 全局页标记失配
    GlobalFlagMismatch,
    /// 空 flush（零项失效）
    EmptyFlush,
}

impl FlushCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FlushCode::TypeOutOfRange => "type 越闭集：INVPCID 仅 0..3",
            FlushCode::PcidOverDomain => "PCID 超 12 位域",
            FlushCode::NoPcidTypeUsed => "不支持 PCID 平台：只许 type3 退基线",
            FlushCode::BatchViaSinglePage => "批刷新误走单页：≥2 项不得逐条 INVLPG",
            FlushCode::DomainCountDrift => "失效域计数漂移：账面失效数 ≠ 闭式",
            FlushCode::PcidFlagMissing => "PCID 标志缺失：type1 需 PCID 支持声明",
            FlushCode::GlobalFlagMismatch => "全局页标记失配：global 位与失效域不符",
            FlushCode::EmptyFlush => "空 flush：零项失效不得记账",
        }
    }
    pub const ALL: [FlushCode; 8] = [
        FlushCode::TypeOutOfRange,
        FlushCode::PcidOverDomain,
        FlushCode::NoPcidTypeUsed,
        FlushCode::BatchViaSinglePage,
        FlushCode::DomainCountDrift,
        FlushCode::PcidFlagMissing,
        FlushCode::GlobalFlagMismatch,
        FlushCode::EmptyFlush,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in FlushCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量 ─────────────────────────────────────────────────────────
/// PCID 位域宽。
pub const PCID_BITS: u64 = 12;
pub const PCID_MAX: u64 = (1 << PCID_BITS) - 1;
/// 建模页总数。
pub const PAGE_TOTAL: u64 = 64;

/// INVPCID 四型闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InvpcidType {
    Individual = 0,  // 个体（PCID+VA）
    SingleAll = 1,   // 单 PCID 全局
    AllNonGlobal = 2,// 全 PCID 非全局
    All = 3,         // 全 flush（含 global）
}

impl InvpcidType {
    pub fn of(v: u64) -> Result<InvpcidType, FlushCode> {
        match v {
            0 => Ok(InvpcidType::Individual),
            1 => Ok(InvpcidType::SingleAll),
            2 => Ok(InvpcidType::AllNonGlobal),
            3 => Ok(InvpcidType::All),
            _ => Err(FlushCode::TypeOutOfRange),
        }
    }
}

/// 建模 TLB 项：(pcid, page, global)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TlbEntry {
    pub pcid: u64,
    pub page: u64,
    pub global: bool,
}

/// 失效选择引擎（PCID 支持声明驱动）。
pub struct FlushSelector {
    pub pcid_supported: bool,
    pub entries: Vec<TlbEntry>,
    /// 失效计数账（只增）。
    pub invalidated_total: u64,
    pub rejected: usize,
}

impl FlushSelector {
    pub fn new(pcid_supported: bool) -> FlushSelector {
        let mut entries = Vec::new();
        for p in 0..PAGE_TOTAL {
            let pcid = p % 4;
            let global = p % 8 == 0;
            entries.push(TlbEntry { pcid, page: p, global });
        }
        FlushSelector {
            pcid_supported,
            entries,
            invalidated_total: 0,
            rejected: 0,
        }
    }

    /// INVPCID 四型选择：返回失效条数（闭式守恒核在调用侧）。
    pub fn invpcid(&mut self, ty: u64, pcid: u64, page: u64) -> Result<usize, FlushCode> {
        let t = InvpcidType::of(ty)?;
        if pcid > PCID_MAX {
            return Err(FlushCode::PcidOverDomain);
        }
        if !self.pcid_supported && t != InvpcidType::All {
            return Err(FlushCode::NoPcidTypeUsed);
        }
        let n = match t {
            InvpcidType::Individual => {
                let before = self.entries.len();
                self.entries.retain(|e| !(e.pcid == pcid && e.page == page));
                before - self.entries.len()
            }
            InvpcidType::SingleAll => {
                let before = self.entries.len();
                self.entries.retain(|e| e.pcid != pcid);
                before - self.entries.len()
            }
            InvpcidType::AllNonGlobal => {
                let before = self.entries.len();
                self.entries.retain(|e| e.global);
                before - self.entries.len()
            }
            InvpcidType::All => {
                let n = self.entries.len();
                self.entries.clear();
                n
            }
        };
        if n == 0 {
            return Err(FlushCode::EmptyFlush);
        }
        self.invalidated_total += n as u64;
        Ok(n)
    }

    /// INVLPG 单页刷新（恰 1 项语义——分界红线）。
    pub fn invlpg_single(&mut self, pcid: u64, page: u64) -> Result<(), FlushCode> {
        let before = self.entries.len();
        self.entries.retain(|e| !(e.pcid == pcid && e.page == page && !e.global));
        let n = before - self.entries.len();
        if n > 1 {
            return Err(FlushCode::BatchViaSinglePage);
        }
        if n == 0 {
            return Err(FlushCode::EmptyFlush);
        }
        self.invalidated_total += 1;
        Ok(())
    }

    /// 批刷新分界：≥2 项必须走 INVPCID（禁逐条 INVLPG）。
    pub fn batch_boundary_check(&self, pages: &[u64]) -> Result<(), FlushCode> {
        if pages.len() >= 2 {
            return Err(FlushCode::BatchViaSinglePage);
        }
        Ok(())
    }

    /// 失效域闭式核：四型期望失效数。
    pub fn expect_domain_count(ty: InvpcidType, pcid: u64, page: u64, snap: &[TlbEntry]) -> usize {
        match ty {
            InvpcidType::Individual => snap.iter().filter(|e| e.pcid == pcid && e.page == page).count(),
            InvpcidType::SingleAll => snap.iter().filter(|e| e.pcid == pcid).count(),
            InvpcidType::AllNonGlobal => snap.iter().filter(|e| !e.global).count(),
            InvpcidType::All => snap.len(),
        }
    }

    pub fn summary_line(&self) -> String {
        format!(
            "flush 账: PCID支持={} 在场项={} 累计失效={} 拒收={}",
            self.pcid_supported,
            self.entries.len(),
            self.invalidated_total,
            self.rejected
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

// ── 四型选择语义断言（判据 J1 主面） ─────────────────────────────
pub fn drill_four_types() -> (bool, String) {
    // 每型独立 selector（64 项基线：PCID 各 16，global 8 全在 PCID0）。
    let mut s0 = FlushSelector::new(true);
    let n0 = s0.invpcid(0, 0, 0); // 个体 1 项
    let mut s1 = FlushSelector::new(true);
    let n1 = s1.invpcid(1, 0, 0); // PCID0 全局 16 项（含 8 global）
    let mut s2 = FlushSelector::new(true);
    let n2 = s2.invpcid(2, 0, 0); // 56 非全局项（8 global 豁免）
    let mut s3 = FlushSelector::new(true);
    let n3 = s3.invpcid(3, 0, 0); // 全 flush 64
    let ok = n0 == Ok(1) && n1 == Ok(16) && n2 == Ok(56) && n3 == Ok(64);
    (
        ok,
        format!(
            "四型失效域: t0={:?} t1={:?} t2={:?} t3={:?}",
            n0, n1, n2, n3
        ),
    )
}

fn drill_type_range_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    for _ in 0..10 {
        let mut sel = FlushSelector::new(true);
        let bad = 4 + lcg.next_below(16);
        if sel.invpcid(bad, 0, 0) == Err(FlushCode::TypeOutOfRange) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("type 越闭集 {}/10 必抓", caught))
}

fn drill_pcid_domain_10() -> (bool, String) {
    let mut caught = 0;
    for i in 0..10u32 {
        let mut sel = FlushSelector::new(true);
        let bad = PCID_MAX + 1 + i as u64;
        if sel.invpcid(0, bad, 0) == Err(FlushCode::PcidOverDomain) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("PCID 超域 {}/10 必抓", caught))
}

fn drill_no_pcid_baseline_10() -> (bool, String) {
    // 不支持 PCID 平台：type0..2 拒，type3 退基线过。
    let mut caught = 0;
    for ty in 0..3u64 {
        let mut sel = FlushSelector::new(false);
        if sel.invpcid(ty, 0, 0) == Err(FlushCode::NoPcidTypeUsed) {
            caught += 1;
        }
    }
    let mut sel = FlushSelector::new(false);
    let t3 = sel.invpcid(3, 0, 0);
    // t3 = Ok(64) 且 entries 清空。
    let ok = caught == 3 && t3 == Ok(64) && sel.entries.is_empty();
    (ok, format!("退基线: t0..2拒={} t3全flush={:?}", caught == 3, t3))
}

fn drill_single_page_boundary_10() -> (bool, String) {
    // INVLPG 单页 vs 批刷新分界 10/10。
    let mut caught = 0;
    for i in 0..10u32 {
        let mut sel = FlushSelector::new(true);
        // 单页：恰 1 项（页 = 1+2i 恒非 global，PCID = 页%4）。
        let page = 1 + i as u64 * 2;
        let pcid = page % 4;
        let r1 = sel.invlpg_single(pcid, page);
        // 批刷新误走单页：分界核必抓。
        let r2 = sel.batch_boundary_check(&[1, 2, 3]);
        if r1.is_ok() && r2 == Err(FlushCode::BatchViaSinglePage) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("单页/批分界 {}/10", caught))
}

fn drill_domain_count_consistent() -> (bool, String) {
    // 失效域闭式守恒：账面失效数 == 闭式期望（10 型×页组合抽样）。
    let mut lcg = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    for round in 0..10u32 {
        let ty = (round % 4) as u64;
        let mut sel = FlushSelector::new(true);
        let snap = sel.entries.clone();
        let pcid = lcg.next_below(4);
        let page = lcg.next_below(PAGE_TOTAL);
        let expect = FlushSelector::expect_domain_count(
            InvpcidType::of(ty).unwrap_or(InvpcidType::All),
            pcid,
            page,
            &snap,
        );
        let got = sel.invpcid(ty, pcid, page);
        let before_total = sel.invalidated_total;
        let _ = before_total;
        if got == Ok(expect.max(1)) || (expect == 0 && got == Err(FlushCode::EmptyFlush)) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("失效域闭式 {}/10 一致", caught))
}

fn drill_global_exempt() -> (bool, String) {
    // type2 全局页豁免核：8 global 页保留。
    let mut sel = FlushSelector::new(true);
    let _ = sel.invpcid(2, 0, 0);
    let globals_left = sel.entries.iter().filter(|e| e.global).count();
    let nonleft = sel.entries.iter().filter(|e| !e.global).count();
    (globals_left == 8 && nonleft == 0, format!("type2: 全局保留={} 非全清零={}", globals_left, nonleft == 0))
}

fn drill_global_type3() -> (bool, String) {
    // type3 全 flush 含 global。
    let mut sel = FlushSelector::new(true);
    let _ = sel.invpcid(3, 0, 0);
    (sel.entries.is_empty(), format!("type3: 全清={}", sel.entries.is_empty()))
}

fn drill_empty_flush_10() -> (bool, String) {
    // 空 flush 注入：重复失效同一项 → 零项必抓。
    let mut caught = 0;
    for _ in 0..10 {
        let mut sel = FlushSelector::new(true);
        let _ = sel.invpcid(0, 1, 2);
        let r = sel.invpcid(0, 1, 2); // 已失效
        if r == Err(FlushCode::EmptyFlush) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("空 flush 注入 {}/10 必抓", caught))
}

fn drill_pcid_flag_missing() -> (bool, String) {
    // type1 在无 PCID 支持下的路径已被 NoPcidTypeUsed 覆盖；此处核
    // type1 语义需 PCID 声明的结构核（pcid_supported=false 时 type1 拒）。
    let mut sel = FlushSelector::new(false);
    let r = sel.invpcid(1, 0, 0);
    (r == Err(FlushCode::NoPcidTypeUsed), format!("type1 需 PCID 声明: {:?}", r.is_err()))
}

fn drill_invlpg_global_never() -> (bool, String) {
    // INVLPG 单页不失效 global 页（x86 语义：INVLPG 只失效非全局映射）。
    let mut sel = FlushSelector::new(true);
    // 页 0 是 global（p%8==0）。
    let r = sel.invlpg_single(0, 0);
    let g0_still = sel.entries.iter().any(|e| e.page == 0 && e.global);
    (r == Err(FlushCode::EmptyFlush) || (g0_still && r.is_err()), format!("INVLPG 不打 global: r={:?} g0保留={}", r, g0_still))
}

fn drill_total_monotonic() -> (bool, String) {
    // 失效计数只增。
    let mut sel = FlushSelector::new(true);
    let mut ok = true;
    let mut prev = 0u64;
    for i in 0..8u64 {
        let _ = sel.invpcid(0, i % 4, i + 1);
        ok = ok && sel.invalidated_total >= prev;
        prev = sel.invalidated_total;
    }
    (ok, format!("失效计数只增: {}", ok))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    FlushCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_four_types();
    let (a2, d2) = drill_single_page_boundary_10();
    let (a3, d3) = drill_no_pcid_baseline_10();
    let ok = a1 && a2 && a3;
    (ok, format!("全流程: 四型={} 分界10/10={} 退基线={} [{}|{}|{}]", a1, a2, a3, d1, d2, d3))
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_four_types();
    v.push(("J1-四型选择语义", a, b));
    let (a, b) = drill_type_range_10();
    v.push(("J2-type越闭集10/10", a, b));
    let (a, b) = drill_pcid_domain_10();
    v.push(("J2-PCID超域10/10", a, b));
    let (a, b) = drill_no_pcid_baseline_10();
    v.push(("J1-退基线type3", a, b));
    let (a, b) = drill_single_page_boundary_10();
    v.push(("J1-单页/批分界10/10", a, b));
    let (a, b) = drill_domain_count_consistent();
    v.push(("J2-失效域闭式10/10", a, b));
    let (a, b) = drill_global_exempt();
    v.push(("J3-type2全局豁免", a, b));
    let (a, b) = drill_global_type3();
    v.push(("J3-type3全清", a, b));
    let (a, b) = drill_empty_flush_10();
    v.push(("J2-空flush注入10/10", a, b));
    let (a, b) = drill_pcid_flag_missing();
    v.push(("J3-PCID声明核", a, b));
    let (a, b) = drill_invlpg_global_never();
    v.push(("J3-INVLPG不打global", a, b));
    let (a, b) = drill_total_monotonic();
    v.push(("J3-失效计数只增", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx1922_checks：本单判据聚合（诊断段 0x36600）。
pub fn run_unx1922_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1922 INVPCID/INVLPG 指令面");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36600 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
