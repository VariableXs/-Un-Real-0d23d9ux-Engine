//! UNX-F9251 页对齐边界值矩阵（4K/2M 大页） · 0x7AC40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9251 逐字）：
//!   UNX-F9251-J1 4K 页界逐位注入判据过，2M 大页边界格过，跨界映射拒绝
//!   语义格过。
//!
//! 设计要点：
//!   ① 对齐判定全部用掩码与 checked_*：4K 粒度 0xFFF、2M 粒度 0x1F_FFFF，
//!      上取整用 checked_add 再掩码，溢出返回 None 由调用方转 ENOMEM，绝不
//!      回绕，也不做任何除法取模于非常量（除数是 2 的幂时用掩码）。
//!   ② 4K 页界「逐位注入」：以 2M 边界为锚点，取 -4097/-4096/-1/0/+1/+4095/
//!      +4096/2M-1/2M/2M+4K 十个偏移，每格 10 轮复算分类，全部必须与冻结
//!      分类表一致；这是「逐位」判据的可复算落点。
//!   ③ 2M 大页边界：base 与 len 必须同时按 2M 对齐；4K 映射也必须显式声明
//!      huge=false。任何跨 2M 边界的 4K 映射都拒绝（EINVAL），要求调用方先
//!      拆成两段，避免一个大页被两个 4K 映射共享。
//!   ④ 拒绝语义分层：参数面（零长度/未对齐/跨界）回 EINVAL；容量面（区间
//!      越出 8 MiB 语料区、表满、地址溢出）回 ENOMEM；重叠区间回 EINVAL。
//!      顺序为：零长度 → 对齐 → 溢出 → 区间 → 跨界 → 重叠 → 容量。
//!   ⑤ 状态机：映射槽容量 8，插入只增 count，失败路径不得占用槽位；同一
//!      区间重复插入回 EINVAL 且 count 不变。
//!   ⑥ 诊断码段内闭集 12 个（+0x01..=+0x0C），码值唯一，每码带中文建议。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9251 独占 0x7AC40..=0x7AC7F）。
pub const SEG_BASE: u32 = 0x7AC40;
/// 4K 页大小。
pub const PAGE_4K: u64 = 4096;
/// 2M 大页大小。
pub const PAGE_2M: u64 = 2_097_152;
/// 语料区基址（1 GiB，同时 2M 对齐）。
pub const REGION_BASE: u64 = 0x0000_0000_4000_0000;
/// 语料区大小（8 MiB，4 个 2M 大页）。
pub const REGION_SIZE: u64 = 8 * 1024 * 1024;
/// 语料区上界（不含）。
pub const REGION_END: u64 = REGION_BASE + REGION_SIZE;
/// 映射表容量。
pub const MAX_MAPS: usize = 8;
/// 单格注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 4K 边界注入格数。
pub const BOUNDARY_CELLS: usize = 10;
/// 2M 边界网格行数。
pub const HUGE_GRID_ROWS: u32 = 9;

/// Linux errno 数值（正数口径）。
pub const ENOMEM: i32 = 12;
pub const EINVAL: i32 = 22;

/// 4K 掩码。
pub const MASK_4K: u64 = PAGE_4K - 1;
/// 2M 掩码。
pub const MASK_2M: u64 = PAGE_2M - 1;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignCode {
    MisalignedBase,
    MisalignedLen,
    CrossBoundary,
    Overflow,
    ZeroLen,
    CapacityFull,
    Overlap,
    HugeSplit,
    LedgerRegress,
    NotReproducible,
    SegmentViolation,
    BadOrder,
}

impl AlignCode {
    /// 码值（段基址 + 1 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            AlignCode::MisalignedBase => "基址未对齐被拒：4K 映射要求 base 按 0xFFF 对齐，2M 大页要求按 0x1F_FFFF 对齐；请 page_align_down 后重试",
            AlignCode::MisalignedLen => "长度未对齐被拒：len 必须是对应粒度的整数倍；请上取整到 4K 或 2M 后再提交",
            AlignCode::CrossBoundary => "跨界映射被拒：区间跨越 2M 边界；请按边界拆成两段映射，避免共享同一大页",
            AlignCode::Overflow => "地址溢出被拒：base+len 超出 u64；请用 checked_add 先验算并按 ENOMEM 处理",
            AlignCode::ZeroLen => "零长度被拒：len 必须大于 0；请给出至少一个粒度的长度",
            AlignCode::CapacityFull => "映射表已满：8 个槽位全部占用；请先解除旧映射再申请",
            AlignCode::Overlap => "区间重叠被拒：新映射与既有槽相交；请换地址或先解除重叠段",
            AlignCode::HugeSplit => "大页拆分被拒：2M 大页不得被 4K 映射切分；请显式声明 huge 或改址",
            AlignCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查回滚路径是否误改累计值",
            AlignCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态或非确定哈希",
            AlignCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7A000..=0x7D1C0 且 0x40 对齐",
            AlignCode::BadOrder => "判定次序错误：对齐/溢出/容量被对调；请按冻结次序重排校验",
        }
    }

    /// 闭集全量（文案与唯一性检查使用）。
    pub const ALL: [AlignCode; 12] = [
        AlignCode::MisalignedBase,
        AlignCode::MisalignedLen,
        AlignCode::CrossBoundary,
        AlignCode::Overflow,
        AlignCode::ZeroLen,
        AlignCode::CapacityFull,
        AlignCode::Overlap,
        AlignCode::HugeSplit,
        AlignCode::LedgerRegress,
        AlignCode::NotReproducible,
        AlignCode::SegmentViolation,
        AlignCode::BadOrder,
    ];
}

// ── 对齐分类 ──────────────────────────────────────────────────────────
/// 地址对齐等级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignClass {
    /// 未按 4K 对齐
    Unaligned,
    /// 仅 4K 对齐
    Page4K,
    /// 2M 对齐（必然 4K 对齐）
    Page2M,
}

impl AlignClass {
    /// 闭集编码（冻结表使用）：0 未对齐、1 4K、2 2M。
    pub fn code(self) -> u32 {
        match self {
            AlignClass::Unaligned => 0,
            AlignClass::Page4K => 1,
            AlignClass::Page2M => 2,
        }
    }

    /// 是否至少 4K 对齐。
    pub fn is_aligned(self) -> bool {
        matches!(self, AlignClass::Page4K | AlignClass::Page2M)
    }
}

/// 地址分类（掩码实现，无除法）。
pub fn classify(addr: u64) -> AlignClass {
    if addr & MASK_2M == 0 {
        AlignClass::Page2M
    } else if addr & MASK_4K == 0 {
        AlignClass::Page4K
    } else {
        AlignClass::Unaligned
    }
}

/// 4K 上取整；溢出返回 None。
pub fn round_up_4k(addr: u64) -> Option<u64> {
    addr.checked_add(MASK_4K).map(|v| v & !MASK_4K)
}

/// 4K 下取整。
pub fn round_down_4k(addr: u64) -> u64 {
    addr & !MASK_4K
}

/// 2M 上取整；溢出返回 None。
pub fn round_up_2m(addr: u64) -> Option<u64> {
    addr.checked_add(MASK_2M).map(|v| v & !MASK_2M)
}

/// 2M 下取整。
pub fn round_down_2m(addr: u64) -> u64 {
    addr & !MASK_2M
}

/// 区间 [base, base+len) 是否跨越 2M 边界。
pub fn crosses_2m(base: u64, len: u64) -> bool {
    if len == 0 {
        return false;
    }
    let end = match base.checked_add(len) {
        Some(v) => v,
        None => return true,
    };
    base & !MASK_2M != (end - 1) & !MASK_2M
}

/// 4K 页内偏移（逐位注入的观察量）。
pub fn offset_4k(addr: u64) -> u64 {
    addr & MASK_4K
}

// ── 映射模型 ──────────────────────────────────────────────────────────
/// 一个映射槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Map {
    pub used: bool,
    pub base: u64,
    pub len: u64,
    pub huge: bool,
}

impl Map {
    pub const fn empty() -> Map {
        Map { used: false, base: 0, len: 0, huge: false }
    }
    pub fn end(&self) -> u64 {
        self.base.saturating_add(self.len)
    }
}

/// 定容映射表：8 槽 + 两个单调计数。
#[derive(Clone, Copy, Debug)]
pub struct AlignSpace {
    maps: [Map; MAX_MAPS],
    count: u32,
    rejects: u32,
}

impl AlignSpace {
    pub const fn new() -> AlignSpace {
        AlignSpace { maps: [Map::empty(); MAX_MAPS], count: 0, rejects: 0 }
    }

    pub fn count(&self) -> u32 {
        self.count
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    /// 取槽视图。
    pub fn slot(&self, i: usize) -> Option<&Map> {
        self.maps.get(i)
    }
    /// 区间是否与既有槽重叠。
    pub fn overlaps(&self, base: u64, len: u64) -> bool {
        let end = match base.checked_add(len) {
            Some(v) => v,
            None => return true,
        };
        let mut i = 0usize;
        while i < MAX_MAPS {
            if let Some(m) = self.maps.get(i) {
                if m.used && base < m.end() && m.base < end {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// mmap 语义的对齐判定。成功返回槽下标。
    pub fn map(&mut self, base: u64, len: u64, huge: bool) -> Result<usize, i32> {
        // ① 零长度
        if len == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        let gran = if huge { PAGE_2M } else { PAGE_4K };
        let mask = gran - 1;
        // ② 基址与长度对齐
        if base & mask != 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        if len & mask != 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        // ③ 加法溢出
        let end = match base.checked_add(len) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(ENOMEM);
            }
        };
        // ④ 语料区边界
        if base < REGION_BASE || end > REGION_END {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ENOMEM);
        }
        // ⑤ 跨 2M 边界拒绝（仅 4K 映射；大页天然按 2M 分片）
        if !huge && crosses_2m(base, len) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        // ⑥ 重叠
        if self.overlaps(base, len) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        // ⑦ 容量
        let mut i = 0usize;
        while i < MAX_MAPS {
            let free = match self.maps.get(i) {
                Some(m) => !m.used,
                None => false,
            };
            if free {
                if let Some(slot) = self.maps.get_mut(i) {
                    slot.used = true;
                    slot.base = base;
                    slot.len = len;
                    slot.huge = huge;
                }
                self.count = self.count.saturating_add(1);
                return Ok(i);
            }
            i += 1;
        }
        self.rejects = self.rejects.saturating_add(1);
        Err(ENOMEM)
    }

    /// 诊断分类（不产生副作用）。
    pub fn classify_request(&self, base: u64, len: u64, huge: bool) -> AlignCode {
        if len == 0 {
            return AlignCode::ZeroLen;
        }
        let gran = if huge { PAGE_2M } else { PAGE_4K };
        if base & (gran - 1) != 0 {
            return AlignCode::MisalignedBase;
        }
        if len & (gran - 1) != 0 {
            return AlignCode::MisalignedLen;
        }
        match base.checked_add(len) {
            Some(end) => {
                if end > REGION_END {
                    AlignCode::Overflow
                } else if !huge && crosses_2m(base, len) {
                    AlignCode::CrossBoundary
                } else if self.overlaps(base, len) {
                    AlignCode::Overlap
                } else {
                    AlignCode::HugeSplit
                }
            }
            None => AlignCode::Overflow,
        }
    }
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账本：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(EINVAL);
        }
        Ok(())
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 4K 页界逐位注入 ──────────────────────────────────────────────────
/// 锚点：语料区内的第一个 2M 边界。
pub const ANCHOR: u64 = REGION_BASE + PAGE_2M;

/// 十格偏移（i64 允许负偏移），期望分类编码。
pub const BOUNDARY_GRID: [(i64, u32); BOUNDARY_CELLS] = [
    (-4097, 0),
    (-4096, 1),
    (-1, 0),
    (0, 2),
    (1, 0),
    (4095, 0),
    (4096, 1),
    (PAGE_2M as i64 - 1, 0),
    (PAGE_2M as i64, 2),
    (PAGE_2M as i64 + 4096, 1),
];

/// 带符号偏移 → 地址；负偏移走 checked_sub，越界返回 None。
pub fn grid_addr(delta: i64) -> Option<u64> {
    if delta < 0 {
        let mag = (delta as i128).checked_neg()? as u64;
        ANCHOR.checked_sub(mag)
    } else {
        ANCHOR.checked_add(delta as u64)
    }
}

/// 4K 逐位注入：十格 × 10 轮，分类与冻结表逐格一致，且上下取整自洽。
pub fn run_boundary_matrix() -> ([u32; BOUNDARY_CELLS], InjectLedger) {
    let mut ledger = InjectLedger::zero();
    let mut per_cell = [0u32; BOUNDARY_CELLS];
    let mut ci = 0usize;
    while ci < BOUNDARY_GRID.len() {
        if let Some((delta, expect)) = BOUNDARY_GRID.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let addr = grid_addr(delta);
                let caught = match addr {
                    Some(a) => {
                        let up = round_up_4k(a);
                        let down = round_down_4k(a);
                        let ok_up = match up {
                            Some(u) => u & MASK_4K == 0 && u >= a && u - a <= MASK_4K,
                            None => false,
                        };
                        classify(a).code() == expect
                            && classify(a).is_aligned() == (expect != 0)
                            && ok_up
                            && down & MASK_4K == 0
                            && down <= a
                    }
                    None => false,
                };
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_cell.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (per_cell, ledger)
}

// ── 2M 大页边界网格 ──────────────────────────────────────────────────
/// 一行 2M 网格：(基址偏移, 长度, huge, 期望成功)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HugeRow {
    pub off: u64,
    pub len: u64,
    pub huge: bool,
    pub ok: bool,
}

/// 九行冻结网格（每行独立空表）。
pub const HUGE_GRID: [HugeRow; 9] = [
    HugeRow { off: 0, len: PAGE_2M, huge: true, ok: true },
    HugeRow { off: PAGE_2M, len: PAGE_2M, huge: true, ok: true },
    HugeRow { off: 0, len: PAGE_2M * 2, huge: true, ok: true },
    HugeRow { off: PAGE_4K, len: PAGE_2M, huge: true, ok: false },
    HugeRow { off: 0, len: PAGE_2M + PAGE_4K, huge: true, ok: false },
    HugeRow { off: PAGE_2M - PAGE_4K, len: PAGE_4K * 2, huge: false, ok: false },
    HugeRow { off: PAGE_2M - PAGE_4K, len: PAGE_4K, huge: false, ok: true },
    HugeRow { off: PAGE_2M, len: PAGE_4K, huge: false, ok: true },
    HugeRow { off: 0, len: PAGE_2M, huge: false, ok: true },
];

/// 逐行复算 2M 网格：返回 (命中行数, 总行数, 期望拒绝行里的 EINVAL 命中数)。
pub fn run_huge_grid() -> (u32, u32, u32) {
    let mut matched = 0u32;
    let mut rejected_einval = 0u32;
    let mut i = 0usize;
    while i < HUGE_GRID.len() {
        if let Some(row) = HUGE_GRID.get(i).copied() {
            let mut space = AlignSpace::new();
            let base = REGION_BASE + row.off;
            let got = space.map(base, row.len, row.huge);
            let ok = if row.ok { got.is_ok() } else { got == Err(EINVAL) };
            if ok {
                matched = matched.saturating_add(1);
            }
            if !row.ok && got == Err(EINVAL) {
                rejected_einval = rejected_einval.saturating_add(1);
            }
        }
        i += 1;
    }
    (matched, HUGE_GRID.len() as u32, rejected_einval)
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOMEM", -12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；内存不足族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 编码一次 map 结果。
fn encode(out: &mut Vec<u8>, r: Result<usize, i32>) {
    match r {
        Ok(v) => {
            out.push(1);
            out.push((v & 0xFF) as u8);
        }
        Err(e) => {
            out.push(2);
            out.push((e & 0x7F) as u8);
        }
    }
}

/// 规范语料双跑指纹：4K/2M 正常、未对齐、跨界、溢出、重叠与满表。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut sp = AlignSpace::new();
    encode(&mut bytes, sp.map(REGION_BASE, PAGE_4K, false));
    encode(&mut bytes, sp.map(REGION_BASE + PAGE_4K, PAGE_4K, false));
    encode(&mut bytes, sp.map(REGION_BASE + PAGE_2M, PAGE_2M, true));
    encode(&mut bytes, sp.map(REGION_BASE + PAGE_4K, PAGE_4K, false));
    encode(&mut bytes, sp.map(REGION_BASE + PAGE_4K + 1, PAGE_4K, false));
    encode(&mut bytes, sp.map(REGION_BASE + PAGE_2M - PAGE_4K, PAGE_4K * 2, false));
    encode(&mut bytes, sp.map(REGION_BASE, u64::MAX, false));
    encode(&mut bytes, sp.map(REGION_END, PAGE_4K, false));
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9251 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9251_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9251");

    let seg_ok = SEG_BASE % 0x40 == 0
        && (0x7A000..=0x7D1C0).contains(&SEG_BASE)
        && SEG_BASE > 0x7AC00;
    s.add("段位守卫：0x40 对齐且不与 F9250 重叠", seg_ok,
          "段基址唯一且落在本批窗口，段间不得互相覆盖");

    let mut sp = AlignSpace::new();
    let normal = sp.map(REGION_BASE, PAGE_4K * 2, false);
    let normal_ok = normal == Ok(0)
        && sp.count() == 1
        && sp.slot(0).map(|m| m.len) == Some(PAGE_4K * 2);
    s.add("正常流：4K 双页映射成功", normal_ok,
          "页对齐的 4K 映射必须落到首个空槽");

    let (per_cell, ledger) = run_boundary_matrix();
    s.add("注入：4K 页界十格各 10/10 分类正确",
          per_cell.iter().all(|n| *n == INJECT_ROUNDS),
          "十个偏移的分类与上下取整必须逐格自洽");
    s.add("注入账：4K 边界 100 次全检出且逃逸为 0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BOUNDARY_CELLS as u32 * INJECT_ROUNDS,
          "100 次注入必须 100 次检出，分类表不得漂移");

    let (matched, total, rejected) = run_huge_grid();
    s.add("2M 大页：九行网格逐行与冻结表一致",
          matched == total && total == HUGE_GRID_ROWS && rejected == 3,
          "对齐/未对齐/跨界的九种组合必须全部符合预期");

    let mut mis = AlignSpace::new();
    let mis_ok = mis.map(REGION_BASE + 1, PAGE_4K, false) == Err(EINVAL)
        && mis.map(REGION_BASE, PAGE_4K + 1, false) == Err(EINVAL)
        && mis.map(REGION_BASE + 1, PAGE_2M, true) == Err(EINVAL);
    s.add("参数面：基址/长度未对齐全 EINVAL", mis_ok,
          "4K 与 2M 两种粒度的未对齐都必须回参数错误");

    let mut cross = AlignSpace::new();
    let cross_map = cross.map(REGION_BASE + PAGE_2M - PAGE_4K, PAGE_4K * 2, false);
    let cross_ok = cross_map == Err(EINVAL)
        && cross.map(REGION_BASE + PAGE_2M - PAGE_4K, PAGE_4K, false).is_ok()
        && cross.map(REGION_BASE + PAGE_2M, PAGE_4K, false).is_ok();
    s.add("跨界：跨 2M 边界的 4K 映射拒绝、拆段放行", cross_ok,
          "跨界必须 EINVAL，按边界拆成两段后必须成功");

    let mut huge_ok = AlignSpace::new();
    let h = huge_ok.map(REGION_BASE, PAGE_2M, true);
    let h2 = huge_ok.map(REGION_BASE + PAGE_2M, PAGE_2M, true);
    s.add("2M 大页：连续两个大页各自成槽", h == Ok(0) && h2 == Ok(1) && huge_ok.count() == 2,
          "2M 对齐的 base/len 必须直接成功，不被 4K 规则误拒");

    let mut ovf = AlignSpace::new();
    let ovf_ok = ovf.map(u64::MAX & !MASK_4K, PAGE_4K, false) == Err(ENOMEM)
        && ovf.map(REGION_END, PAGE_4K, false) == Err(ENOMEM);
    s.add("容量：地址溢出与越出语料区回 ENOMEM", ovf_ok,
          "checked_add 溢出与区间越界都必须显性回 ENOMEM");

    let mut dup = AlignSpace::new();
    let _ = dup.map(REGION_BASE, PAGE_4K, false);
    let dup_ok = dup.map(REGION_BASE, PAGE_4K, false) == Err(EINVAL)
        && dup.count() == 1
        && dup.rejects() == 1
        && dup.classify_request(REGION_BASE, PAGE_4K, false) == AlignCode::Overlap;
    s.add("状态机：重叠插入 EINVAL 且槽数不变", dup_ok,
          "失败插入不得占用槽位，rejects 只增一笔");

    let mut cap = AlignSpace::new();
    let mut filled = 0u64;
    while filled < MAX_MAPS as u64 {
        let base = REGION_BASE + PAGE_4K * filled;
        if cap.map(base, PAGE_4K, false).is_ok() {
            filled += 1;
        } else {
            break;
        }
    }
    let ninth = cap.map(REGION_BASE + PAGE_4K * 8, PAGE_4K, false);
    s.add("容量：8 槽填满后第 9 段回 ENOMEM",
          filled == MAX_MAPS as u64 && ninth == Err(ENOMEM) && cap.count() == 8,
          "定容表不得越界写入，满表必须显性回 ENOMEM");

    let order_ok = AlignSpace::new().map(u64::MAX, PAGE_2M, true) == Err(EINVAL)
        && AlignSpace::new().map(REGION_END, PAGE_4K, false) == Err(ENOMEM);
    s.add("判定次序：对齐 EINVAL 先于区间 ENOMEM", order_ok,
          "参数面错误必须先于容量面错误返回");

    let codes_ok = agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOMEM)
        && linux_neg(EINVAL) == -EINVAL
        && !agrees_with_linux(0);
    s.add("Linux 逐码一致：EINVAL/ENOMEM 表内可复算", codes_ok,
          "错误码只允许 errno 表内正值，表外码一律拒绝");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    s.add("文案闭集：12 个诊断码均有中文建议",
          AlignCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到非空且可操作的说明");

    let mut uniq = true;
    let mut i = 0usize;
    while i < AlignCode::ALL.len() {
        let mut j = i + 1;
        while j < AlignCode::ALL.len() {
            if let (Some(a), Some(b)) = (AlignCode::ALL.get(i), AlignCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    let cell_text = format!(
        "边界注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("边界账目", ledger.conserved(), &cell_text);
    let huge_text = format!(
        "2M 网格 {}/{} 命中，拒绝 EINVAL {} 行",
        matched, total, rejected
    );
    s.add_dyn("网格快照", matched == total, &huge_text);
    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_boundaries() {
        assert_eq!(classify(REGION_BASE).code(), 2);
        assert_eq!(classify(REGION_BASE + PAGE_4K).code(), 1);
        assert_eq!(classify(REGION_BASE + 1).code(), 0);
        assert_eq!(offset_4k(REGION_BASE + 1), 1);
    }

    #[test]
    fn rounding_is_checked() {
        assert_eq!(round_up_4k(1), Some(PAGE_4K));
        assert_eq!(round_up_4k(REGION_BASE), Some(REGION_BASE));
        assert_eq!(round_up_4k(u64::MAX), None);
        assert_eq!(round_up_2m(PAGE_4K), Some(PAGE_2M));
        assert_eq!(round_down_2m(PAGE_2M + 1), PAGE_2M);
    }

    #[test]
    fn cross_2m_detection() {
        assert!(!crosses_2m(REGION_BASE, PAGE_2M));
        assert!(crosses_2m(REGION_BASE + PAGE_2M - PAGE_4K, PAGE_4K * 2));
        assert!(!crosses_2m(REGION_BASE, PAGE_4K));
        assert!(crosses_2m(u64::MAX - PAGE_4K, PAGE_4K * 2));
    }

    #[test]
    fn normal_4k_map() {
        let mut sp = AlignSpace::new();
        assert_eq!(sp.map(REGION_BASE, PAGE_4K, false), Ok(0));
        assert_eq!(sp.count(), 1);
    }

    #[test]
    fn misaligned_rejected() {
        let mut sp = AlignSpace::new();
        assert_eq!(sp.map(REGION_BASE + 1, PAGE_4K, false), Err(EINVAL));
        assert_eq!(sp.map(REGION_BASE, PAGE_4K + 1, false), Err(EINVAL));
        assert_eq!(sp.map(REGION_BASE + 1, PAGE_2M, true), Err(EINVAL));
        assert_eq!(sp.count(), 0);
    }

    #[test]
    fn cross_boundary_rejected() {
        let mut sp = AlignSpace::new();
        assert_eq!(
            sp.map(REGION_BASE + PAGE_2M - PAGE_4K, PAGE_4K * 2, false),
            Err(EINVAL)
        );
        assert!(sp.map(REGION_BASE + PAGE_2M - PAGE_4K, PAGE_4K, false).is_ok());
    }

    #[test]
    fn huge_pages_map() {
        let mut sp = AlignSpace::new();
        assert_eq!(sp.map(REGION_BASE, PAGE_2M, true), Ok(0));
        assert_eq!(sp.map(REGION_BASE + PAGE_2M, PAGE_2M, true), Ok(1));
        assert_eq!(sp.count(), 2);
    }

    #[test]
    fn overflow_and_overlap() {
        let mut sp = AlignSpace::new();
        assert_eq!(sp.map(u64::MAX & !MASK_4K, PAGE_4K, false), Err(ENOMEM));
        assert_eq!(sp.map(REGION_END, PAGE_4K, false), Err(ENOMEM));
        let _ = sp.map(REGION_BASE, PAGE_4K, false);
        assert_eq!(sp.map(REGION_BASE, PAGE_4K, false), Err(EINVAL));
    }

    #[test]
    fn boundary_matrix_conserved() {
        let (per, l) = run_boundary_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 100);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn huge_grid_matches() {
        let (m, t, r) = run_huge_grid();
        assert_eq!(t, HUGE_GRID_ROWS);
        assert_eq!(m, t);
        assert_eq!(r, 3);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9251_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
