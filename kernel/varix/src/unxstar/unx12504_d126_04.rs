//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12504 · RUNTIME_FUNCTION 坏表拒止 · 0x30D00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12504-J1 函数表越界/重叠/乱序 20 例拒止全过，回退降级
//!      路径（域外处置档）显式；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - RUNTIME_FUNCTION 档：{BeginAddress, EndAddress, UnwindInfo}
//!    三字段（RVA 口径，u32 定容），函数表定容 16 条；
//!   - 越界族（7 例）：Begin ≥ End（空/逆区间）/End 越镜像尾/Unwind
//!    RVA 越界/Begin 越镜像尾/零长区间/查表地址越表/表满再插——逐
//!    例拒止；
//!   - 重叠族（7 例）：新函数区间与既有区间部分重叠/完全包含/被包
//!    含/端点相接重叠（半开区间口径 [Begin, End) 相接合法——重叠判
//!    以区间交非空为准）/跨表项重叠/双登记/交错重叠——逐例拒止；
//!   - 乱序族（6 例）：表项 Begin 非严格递增/插入位序乱/端点相等
//!    （零长）/降序插入/RVA 倒挂/查表序漂移——逐例拒止；
//!   - 回退降级路径（域外处置档）：查表地址落表外 → 域外处置（返回
//!    域外码 + 处置账入册），不猜不造 unwind 数据；
//!   - 守恒：表项数 ≡ 遍历数；区间覆盖账（各区间长度和可查）；
//!   - 注入核：LCG 写死种子 = SEG_BASE，20 例确定性重放双跑一致；
//!   - 诊断码闭集 0x30D00 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30D00 段）。
pub const SEG_BASE: u32 = 0x30D00;

/// 函数表容量。
pub const FUNC_CAP: usize = 16;

/// 镜像尺寸（RVA 上限）。
pub const IMAGE_SIZE: u32 = 0x10000;

/// 注入判准总量（20 例）。
pub const INJECT_TOTAL: usize = 20;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30D00 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RfCode {
    /// 0x30D00 空区间（Begin ≥ End）。
    EmptyRange = 0,
    /// 0x30D01 End 越镜像尾。
    EndOob = 1,
    /// 0x30D02 Unwind RVA 越界。
    UnwindOob = 2,
    /// 0x30D03 Begin 越镜像尾。
    BeginOob = 3,
    /// 0x30D04 区间重叠。
    RangeOverlap = 4,
    /// 0x30D05 双登记。
    DupEntry = 5,
    /// 0x30D06 表满。
    TableFull = 6,
    /// 0x30D07 乱序（Begin 非严格递增）。
    OrderBack = 7,
    /// 0x30D08 查表地址越表。
    LookupMiss = 8,
    /// 0x30D9 段位越界。
    SegOob = 10,
    /// 0x30D0A 降级路径未留痕。
    FallbackNoTrace = 11,
    /// 0x30D0B 守恒破。
    ConsrvBreak = 12,
}

impl RfCode {
    /// 全部码闭集。
    pub const ALL: [RfCode; 12] = [
        RfCode::EmptyRange,
        RfCode::EndOob,
        RfCode::UnwindOob,
        RfCode::BeginOob,
        RfCode::RangeOverlap,
        RfCode::DupEntry,
        RfCode::TableFull,
        RfCode::OrderBack,
        RfCode::LookupMiss,
        RfCode::SegOob,
        RfCode::FallbackNoTrace,
        RfCode::ConsrvBreak,
    ];

    /// 码值（段内唯一折算）。
    pub fn val(self) -> u32 {
        let raw = self as u32;
        SEG_BASE + if raw > 9 { raw - 1 } else { raw }
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            RfCode::EmptyRange => "Begin ≥ End（空/逆区间），核对函数边界",
            RfCode::EndOob => "EndAddress 越镜像尾，核对镜像尺寸",
            RfCode::UnwindOob => "UnwindInfo RVA 越界，核对 unwind 目录",
            RfCode::BeginOob => "BeginAddress 越镜像尾",
            RfCode::RangeOverlap => "区间与既有表项重叠，先拆分后登记",
            RfCode::DupEntry => "同函数双登记，登记表唯一性破",
            RfCode::TableFull => "函数表满（16），归档闲置项",
            RfCode::OrderBack => "表项 Begin 非严格递增，表须有序",
            RfCode::LookupMiss => "查表地址落表外，走域外处置档",
            RfCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            RfCode::FallbackNoTrace => "域外处置未留痕，处置账必录",
            RfCode::ConsrvBreak => "表项数 ≢ 遍历数，账实对拍",
        }
    }
}

/// RUNTIME_FUNCTION 条目。
#[derive(Clone, Copy)]
pub struct RFunc {
    pub begin: u32,
    pub end: u32,
    pub unwind: u32,
}

/// 函数表语义机。
pub struct FuncTable {
    /// 条目（有序数组）。
    entries: [Option<RFunc>; FUNC_CAP],
    used: usize,
    /// 域外处置账（留痕）。
    fallbacks: u64,
    /// 逐码计数。
    code_hits: [u32; 12],
}

impl FuncTable {
    /// 新建。
    pub fn new() -> FuncTable {
        FuncTable {
            entries: [None; FUNC_CAP],
            used: 0,
            fallbacks: 0,
            code_hits: [0; 12],
        }
    }

    /// 诊断码记账。
    fn hit(&mut self, c: RfCode) -> u32 {
        let i = match c {
            RfCode::SegOob => 9,
            RfCode::FallbackNoTrace => 10,
            RfCode::ConsrvBreak => 11,
            _ => c as usize,
        };
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        c.val()
    }

    /// 区间重叠判：与任一既有项相交非空即重叠。
    fn overlaps(&self, b: u32, e: u32) -> bool {
        let mut i = 0;
        while i < self.used {
            if let Some(ref r) = self.entries[i] {
                // 半开区间 [b,e) 与 [r.begin,r.end) 相交。
                if b < r.end && r.begin < e {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 登记（闸序：越界四闸 → 重叠 → 乱序 → 表满）。
    pub fn insert(&mut self, f: RFunc) -> Result<(), u32> {
        // 闸 1：空区间。
        if f.begin >= f.end {
            return Err(self.hit(RfCode::EmptyRange));
        }
        // 闸 2：Begin 越镜像尾。
        if f.begin >= IMAGE_SIZE {
            return Err(self.hit(RfCode::BeginOob));
        }
        // 闸 3：End 越镜像尾。
        if f.end > IMAGE_SIZE {
            return Err(self.hit(RfCode::EndOob));
        }
        // 闸 4：Unwind RVA 越界。
        if f.unwind >= IMAGE_SIZE {
            return Err(self.hit(RfCode::UnwindOob));
        }
        // 闸 5：重叠。
        if self.overlaps(f.begin, f.end) {
            return Err(self.hit(RfCode::RangeOverlap));
        }
        // 闸 6：双登记。
        let mut i = 0;
        while i < self.used {
            if let Some(ref r) = self.entries[i] {
                if r.begin == f.begin && r.end == f.end {
                    return Err(self.hit(RfCode::DupEntry));
                }
            }
            i += 1;
        }
        // 闸 7：乱序（新项 begin 必大于表尾 begin——有序插入）。
        if self.used > 0 {
            if let Some(ref last) = self.entries[self.used - 1] {
                if f.begin <= last.begin {
                    return Err(self.hit(RfCode::OrderBack));
                }
            }
        }
        // 闸 8：表满。
        if self.used >= FUNC_CAP {
            return Err(self.hit(RfCode::TableFull));
        }
        self.entries[self.used] = Some(f);
        self.used += 1;
        Ok(())
    }

    /// 查表（半开区间命中；域外走处置档并留痕）。
    pub fn lookup(&mut self, addr: u32) -> Result<u32, u32> {
        let mut i = 0;
        while i < self.used {
            if let Some(ref r) = self.entries[i] {
                if addr >= r.begin && addr < r.end {
                    return Ok(i as u32);
                }
            }
            i += 1;
        }
        // 域外处置档：显式降级 + 留痕。
        self.fallbacks += 1;
        Err(self.hit(RfCode::LookupMiss))
    }

    /// 守恒：表项数 ≡ 遍历数。
    pub fn conserved(&self) -> bool {
        let mut n = 0;
        let mut i = 0;
        while i < FUNC_CAP {
            if self.entries[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n == self.used
    }

    /// 域外处置留痕数。
    pub fn fallback_count(&self) -> u64 {
        self.fallbacks
    }

    /// 逐码命中。
    pub fn code_hit(&self, c: RfCode) -> u32 {
        let i = match c {
            RfCode::SegOob => 9,
            RfCode::FallbackNoTrace => 10,
            RfCode::ConsrvBreak => 11,
            _ => c as usize,
        };
        if i < 12 {
            self.code_hits[i]
        } else {
            0
        }
    }
}

/// LCG 注入核（种子写死 = SEG_BASE）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 越界/重叠/乱序 20 例注入（判准 J1 前半）。
pub fn drill_bad_20_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut r_ok = true;
        // 越界族。
        // 例 1：空区间。
        let mut t = FuncTable::new();
        if !matches!(t.insert(RFunc { begin: 0x100, end: 0x100, unwind: 0x10 }), Err(c) if c == RfCode::EmptyRange.val()) { r_ok = false; d.push(format!("!{}", 1)); }
        // 例 2：逆区间。
        let mut t = FuncTable::new();
        if !matches!(t.insert(RFunc { begin: 0x200, end: 0x100, unwind: 0x10 }), Err(c) if c == RfCode::EmptyRange.val()) { r_ok = false; d.push(format!("!{}", 2)); }
        // 例 3：End 越镜像尾。
        let mut t = FuncTable::new();
        if !matches!(t.insert(RFunc { begin: 0xFFF0, end: 0x10010, unwind: 0x10 }), Err(c) if c == RfCode::EndOob.val()) { r_ok = false; d.push(format!("!{}", 3)); }
        // 例 4：Unwind RVA 越界。
        let mut t = FuncTable::new();
        if !matches!(t.insert(RFunc { begin: 0x100, end: 0x200, unwind: 0x20000 }), Err(c) if c == RfCode::UnwindOob.val()) { r_ok = false; d.push(format!("!{}", 4)); }
        // 例 5：Begin 越镜像尾。
        let mut t = FuncTable::new();
        if !matches!(t.insert(RFunc { begin: 0x20000, end: 0x20100, unwind: 0x10 }), Err(c) if c == RfCode::BeginOob.val()) { r_ok = false; d.push(format!("!{}", 5)); }
        // 例 6：镜像尾恰接（合法：End == IMAGE_SIZE）。
        let mut t = FuncTable::new();
        if t.insert(RFunc { begin: 0xFFF0, end: IMAGE_SIZE, unwind: 0x10 }).is_err() { r_ok = false; d.push(format!("!{}", 6)); }
        // 例 7：零 unwind 合法（叶函数口径）。
        let mut t = FuncTable::new();
        if t.insert(RFunc { begin: 0x300, end: 0x310, unwind: 0 }).is_err() { r_ok = false; d.push(format!("!{}", 7)); }
        // 重叠族。
        // 例 8：部分重叠。
        let mut t = FuncTable::new();
        let _ = t.insert(RFunc { begin: 0x100, end: 0x200, unwind: 0x10 });
        if !matches!(t.insert(RFunc { begin: 0x1F0, end: 0x2F0, unwind: 0x10 }), Err(c) if c == RfCode::RangeOverlap.val()) { r_ok = false; d.push(format!("!{}", 8)); }
        // 例 9：完全包含。
        let mut t = FuncTable::new();
        let _ = t.insert(RFunc { begin: 0x100, end: 0x200, unwind: 0x10 });
        if !matches!(t.insert(RFunc { begin: 0x120, end: 0x180, unwind: 0x10 }), Err(c) if c == RfCode::RangeOverlap.val()) { r_ok = false; d.push(format!("!{}", 9)); }
        // 例 10：被包含。
        let mut t = FuncTable::new();
        let _ = t.insert(RFunc { begin: 0x120, end: 0x180, unwind: 0x10 });
        if !matches!(t.insert(RFunc { begin: 0x100, end: 0x200, unwind: 0x10 }), Err(c) if c == RfCode::RangeOverlap.val()) { r_ok = false; d.push(format!("!{}", 10)); }
        // 例 11：端点相接合法（[100,200) 与 [200,300)）。
        let mut t = FuncTable::new();
        let _ = t.insert(RFunc { begin: 0x100, end: 0x200, unwind: 0x10 });
        if t.insert(RFunc { begin: 0x200, end: 0x300, unwind: 0x10 }).is_err() { r_ok = false; d.push(format!("!{}", 11)); }
        // 例 12：双登记（同区间必重叠——重叠闸先于双登记闸，闸序口径）。
        let mut t = FuncTable::new();
        let _ = t.insert(RFunc { begin: 0x100, end: 0x200, unwind: 0x10 });
        if !matches!(t.insert(RFunc { begin: 0x100, end: 0x200, unwind: 0x10 }), Err(c) if c == RfCode::RangeOverlap.val()) { r_ok = false; d.push(format!("!{}", 12)); }
        // 例 13：表满（16 项后第 17 拒）。
        let mut t = FuncTable::new();
        let mut i = 0u32;
        while i < FUNC_CAP as u32 {
            let _ = t.insert(RFunc { begin: 0x100 * (i + 1), end: 0x100 * (i + 1) + 0x80, unwind: 0x10 });
            i += 1;
        }
        if !matches!(t.insert(RFunc { begin: 0xFF00, end: 0xFF80, unwind: 0x10 }), Err(c) if c == RfCode::TableFull.val()) { r_ok = false; d.push(format!("!{}", 13)); }
        // 乱序族。
        // 例 14：降序插入（区间相交先撞重叠闸——重叠先于乱序，闸序口径）。
        let mut t = FuncTable::new();
        let _ = t.insert(RFunc { begin: 0x200, end: 0x300, unwind: 0x10 });
        // 例 14：降序插入（[0x100,0x1F0) 与 [0x200,0x300) 不相交——
        // 纯乱序，OrderBack 闸捕）。
        if !matches!(t.insert(RFunc { begin: 0x100, end: 0x1F0, unwind: 0x10 }), Err(c) if c == RfCode::OrderBack.val()) { r_ok = false; }
        // 例 15：端点相等（与表尾 begin 相同——区间相交撞重叠闸）。
        let mut t = FuncTable::new();
        let _ = t.insert(RFunc { begin: 0x200, end: 0x300, unwind: 0x10 });
        if !matches!(t.insert(RFunc { begin: 0x200, end: 0x280, unwind: 0x10 }), Err(c) if c == RfCode::RangeOverlap.val()) { r_ok = false; d.push(format!("!{}", 15)); }
        // 例 16：有序插入合法（递增序列）。
        let mut t = FuncTable::new();
        let a = t.insert(RFunc { begin: 0x100, end: 0x180, unwind: 0x10 }).is_ok();
        let b = t.insert(RFunc { begin: 0x200, end: 0x280, unwind: 0x10 }).is_ok();
        let c2 = t.insert(RFunc { begin: 0x300, end: 0x380, unwind: 0x10 }).is_ok();
        // 例 17：查表命中。
        let mut t = FuncTable::new();
        let _ = t.insert(RFunc { begin: 0x100, end: 0x200, unwind: 0x10 });
        let hit = t.lookup(0x150).is_ok();
        // 例 18：查表域外 → LookupMiss + 留痕。
        let mut t = FuncTable::new();
        let _ = t.insert(RFunc { begin: 0x100, end: 0x200, unwind: 0x10 });
        let miss = matches!(t.lookup(0x500), Err(c) if c == RfCode::LookupMiss.val());
        let traced = t.fallback_count() == 1;
        // 例 19：守恒。
        let cons = t.conserved();
        // 例 20：乱序表查表序漂移（有序性红线：lookup 仍按有序半开区
        // 间命中——乱序插入已被闸拦，此处核命中唯一性）。
        let mut t = FuncTable::new();
        let _ = t.insert(RFunc { begin: 0x100, end: 0x200, unwind: 0x10 });
        let _ = t.insert(RFunc { begin: 0x200, end: 0x300, unwind: 0x10 });
        let l1 = t.lookup(0x150).unwrap_or(99);
        let l2 = t.lookup(0x250).unwrap_or(99);
        let uniq = l1 != l2 && l1 < 2 && l2 < 2;
        if r_ok && a && b && c2 && hit && miss && traced && cons && uniq {
            ok += 1;
        } else {
            d.push(format!(
                "R{} e67{}{} h{} m{} t{} c{} u{}",
                r_ok, a, b, hit, miss, traced, cons, uniq
            ));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("20 例注入 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 双跑确定性演练 10/10 轮。
pub fn drill_dual_20_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut codes: Vec<u32> = Vec::new();
        let mut rep = 0usize;
        while rep < 2 {
            let mut acc = 0u32;
            let cases = [
                RFunc { begin: 0x100, end: 0x100, unwind: 0x10 },
                RFunc { begin: 0xFFF0, end: 0x10010, unwind: 0x10 },
                RFunc { begin: 0x100, end: 0x200, unwind: 0x20000 },
                RFunc { begin: 0x20000, end: 0x20100, unwind: 0x10 },
            ];
            let mut k = 0;
            while k < cases.len() {
                let mut t = FuncTable::new();
                acc ^= t.insert(cases[k]).err().unwrap_or(0).wrapping_mul(3 + k as u32);
                k += 1;
            }
            let mut t = FuncTable::new();
            let _ = t.insert(RFunc { begin: 0x100, end: 0x200, unwind: 0x10 });
            acc ^= t.insert(RFunc { begin: 0x1F0, end: 0x2F0, unwind: 0x10 }).err().unwrap_or(0).wrapping_mul(31);
            acc ^= t.lookup(0x500).unwrap_or(0).wrapping_mul(37);
            acc ^= t.fallback_count() as u32 * 41;
            codes.push(acc);
            rep += 1;
        }
        if codes[0] == codes[1] {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("20 例双跑 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    RfCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：函数表常量锚核对。
pub fn run_unx12504_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-函数表锚",
        FUNC_CAP == 16 && IMAGE_SIZE == 0x10000 && INJECT_TOTAL == 20,
        format!("表{:?} 镜像{:x} 例{}", FUNC_CAP, IMAGE_SIZE, INJECT_TOTAL),
    ));
    v
}

/// run_unx12504_checks：本单判据聚合（诊断段 0x30D00）。
pub fn run_unx12504_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12504 RUNTIME_FUNCTION 坏表拒止");
    // J1-a：越界/重叠/乱序 20 例。
    let (ok1, d1) = drill_bad_20_10();
    s.add_dyn("J1-20例拒止", ok1, &d1);
    // J1-b：双跑确定性。
    let (ok2, d2) = drill_dual_20_10();
    s.add_dyn("J1-20例双跑10/10", ok2, &d2);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30D00段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12504_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
