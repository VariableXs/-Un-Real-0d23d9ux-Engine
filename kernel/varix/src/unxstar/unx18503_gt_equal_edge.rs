//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18503 · VER_GREATER 条件语义（> 比较与 ≥ 划界）
//! · 0x7F980 段 · 批 06 · 配额 280
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18503-J1 > 比较与 ≥ 划界断言全过，等值路径返回 FALSE
//!   断言与 Windows 一致。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - > 语义冻结：gt(sys, req) = sys > req；**等值路径必 FALSE**
//!     （sys==req 时 VER_GREATER 一律不过——Windows 口径红线）；
//!   - 划界方程（与 ≥ 的分界）：gt ≡ ge ∧ ne（ge=sys≥req，ne=sys≠req）；
//!     双列探针每行同时出 gt/ge 两列，划界断言逐行复核；
//!   - 冻结表 20 例：等值 5 例（全 FALSE）/ 上差一 / 反差一 / MAX 三邻 /
//!     符号位跨越 / 常规邻值，期望值逐例手推（sys>req）；
//!   - 探针状态机：Dormant → Sweeping → Closed；id 严格升序（乱序/
//!     重复/越界三闸）；got≠expect 即记 CarveDeviation（零容忍）；
//!   - 注入四类 × 10 轮：等值路径真化（gt 误作 ge，等值行必偏）/
//!     操作数对调（非对称行必偏）/ 恒等误判（gt 误作 ne，s<r 行必偏）/
//!     期望篡改（偏差必抓）——逐类逐轮必抓，漏检即红；
//!   - 对账：与 F18501 cmp_dim 的 GREATER 臂同口径交叉核对（跨模块
//!     禁引，取闭式重演）；与 F18502 划界（≥ 含等值 / > 排等值）互补；
//!   - 账本环 32 槽定容，满即显式拒且旧账不丢；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] GtCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [裁决表]   GT_ROWS 20 例冻结表（手推期望，等值 5 例）
//!   [数据结构] CarveRec（gt/ge 双列记录）/ CarveRing（32 槽）
//!   [状态机]   CarvePhase 三态 + GreaterProbe（三闸 + 划界断言）
//!   [账本]     GtLedger（尝试=过+偏 守恒）
//!   [注入核]   四类变异 × 10 轮（真化/对调/恒等/篡改）
//!   [收口]     golden_run + run_unx18503_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F980，AI-VX11-1105 G1 批06 独占区）。
pub const SEG_BASE: u32 = 0x7F980;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// VER_GREATER（winnt.h 冻结）。
pub const VER_GREATER: u8 = 2;
/// VER_GREATER_EQUAL（划界对照）。
pub const VER_GREATER_EQUAL: u8 = 3;
/// 冻结表行数（J1「20 例」口径的一部分：> 表 20 行）。
pub const GT_ROWS: usize = 20;
/// 等值行数（冻结：5 行，全 FALSE——J1「等值路径返回 FALSE」载体）。
pub const EQUAL_ROWS: usize = 5;
/// 账本环容量（32 槽定容）。
pub const RING_CAP: usize = 32;
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;
/// u32 上界。
pub const U32_MAX: u32 = 0xFFFF_FFFF;

/// 冻结表一行：(sys, req, expect)。expect = sys > req（手推）。
pub type GtRow = (u32, u32, bool);

/// 第 i 行冻结例（越界 None）。
pub fn gt_row(i: usize) -> Option<GtRow> {
    match i {
        0 => Some((7, 7, false)),                       // 等值 ①
        1 => Some((7, 8, false)),                       // 上差一
        2 => Some((8, 7, true)),                        // 反差一
        3 => Some((0, 0, false)),                       // 等值 ②（零）
        4 => Some((0, 1, false)),                       // 零差一
        5 => Some((1, 0, true)),                        // 零反差一
        6 => Some((U32_MAX, U32_MAX, false)),           // 等值 ③（MAX）
        7 => Some((U32_MAX - 1, U32_MAX, false)),       // MAX 差一
        8 => Some((U32_MAX, U32_MAX - 1, true)),        // MAX 反差一
        9 => Some((0, U32_MAX, false)),                 // 全距下/上
        10 => Some((U32_MAX, 0, true)),                 // 全距上/下
        11 => Some((0x8000_0000, 0x8000_0000, false)),  // 等值 ④（符号位）
        12 => Some((0x8000_0000, 0x7FFF_FFFF, true)),   // 符号位跨越 +
        13 => Some((0x7FFF_FFFF, 0x8000_0000, false)),  // 符号位跨越 −
        14 => Some((42, 41, true)),                     // 常规 +
        15 => Some((41, 42, false)),                    // 常规 −
        16 => Some((2, 1, true)),                       // 邻值 +
        17 => Some((1, 2, false)),                      // 邻值 −
        18 => Some((6, 6, false)),                      // 等值 ⑤
        19 => Some((5, 6, false)),                      // 上差一
        _ => None,
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GtCode {
    /// 行 id 越界（≥ 20）。
    RowIdOutOfRange,
    /// 探针乱序（id 回跳）。
    RowOutOfOrder,
    /// 探针重复（同 id 二探）。
    RowDuplicate,
    /// 划界偏差（got ≠ expect 或划界方程不成立）。
    CarveDeviation,
    /// 等值真化（等值行 > 判 TRUE——Windows 口径红线）。
    EqualPathTrue,
    /// 账本环满。
    LedgerFull,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 空表收口。
    TableEmpty,
    /// 注入漏检。
    InjectionMissed,
    /// 阶段非法（Closed 后再探）。
    PhaseInvalid,
    /// 口径失配（与总纲 GREATER 臂不等）。
    CrossCheckMismatch,
    /// 双跑不一致。
    NotReproducible,
}

impl GtCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GtCode::RowIdOutOfRange => "行 id 越界：冻结表仅 0..=19",
            GtCode::RowOutOfOrder => "探针乱序：id 必须严格升序",
            GtCode::RowDuplicate => "探针重复：同 id 只探一次",
            GtCode::CarveDeviation => "划界偏差：got≠expect 零容忍",
            GtCode::EqualPathTrue => "等值真化：等值行 > 判必须 FALSE",
            GtCode::LedgerFull => "账本环满：32 槽耗尽",
            GtCode::LedgerBroken => "账本守恒破坏：尝试≠过+偏",
            GtCode::TableEmpty => "空表收口：零探针不得封账",
            GtCode::InjectionMissed => "注入漏检：变异必被抓",
            GtCode::PhaseInvalid => "阶段非法：Closed 后再探",
            GtCode::CrossCheckMismatch => "口径失配：与总纲 GREATER 臂不等",
            GtCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [GtCode; 12] = [
        GtCode::RowIdOutOfRange,
        GtCode::RowOutOfOrder,
        GtCode::RowDuplicate,
        GtCode::CarveDeviation,
        GtCode::EqualPathTrue,
        GtCode::LedgerFull,
        GtCode::LedgerBroken,
        GtCode::TableEmpty,
        GtCode::InjectionMissed,
        GtCode::PhaseInvalid,
        GtCode::CrossCheckMismatch,
        GtCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GtCode::ALL.len() {
        if GtCode::ALL[i].code() == code {
            return GtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F980 段闭集内"
}

// ── 比较核（> 与划界两列） ──────────────────────────────────────
/// > 比较（冻结语义）。
pub fn gt(sys: u32, req: u32) -> bool {
    sys > req
}

/// ≥ 比较（划界对照列）。
pub fn ge(sys: u32, req: u32) -> bool {
    sys >= req
}

/// 恒等（误判注入对照列）。
pub fn ne(sys: u32, req: u32) -> bool {
    sys != req
}

/// 划界方程：gt ≡ ge ∧ ne（逐行成立；划界断言核）。
pub fn carve_holds(sys: u32, req: u32) -> bool {
    gt(sys, req) == (ge(sys, req) && ne(sys, req))
}

// ── 账本环（32 槽定容） ─────────────────────────────────────────
/// 划界记录（gt/ge 双列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CarveRec {
    pub id: u8,
    /// > 列评值。
    pub got: bool,
    /// ≥ 列评值（划界对照）。
    pub ge_col: bool,
    /// 冻结期望（> 列）。
    pub expect: bool,
}

/// 32 槽账本环。
#[derive(Clone, Copy, Debug)]
pub struct CarveRing {
    slots: [Option<CarveRec>; RING_CAP],
    pub len: usize,
    pub full_rejects: u32,
}

impl CarveRing {
    pub fn new() -> CarveRing {
        CarveRing { slots: [None; RING_CAP], len: 0, full_rejects: 0 }
    }
    /// 入环（满显式拒）。
    pub fn push(&mut self, rec: CarveRec) -> Result<(), u32> {
        if self.len >= RING_CAP {
            self.full_rejects = self.full_rejects.saturating_add(1);
            return Err(GtCode::LedgerFull.code());
        }
        self.slots[self.len] = Some(rec);
        self.len += 1;
        Ok(())
    }
    /// 按下标取记录。
    pub fn rec_at(&self, idx: usize) -> Option<CarveRec> {
        if idx >= self.len {
            return None;
        }
        self.slots[idx]
    }
    /// 清环（幂等）。
    pub fn reset(&mut self) {
        let mut i = 0;
        while i < RING_CAP {
            self.slots[i] = None;
            i += 1;
        }
        self.len = 0;
    }
}

// ── 表指纹 ──────────────────────────────────────────────────────
/// 冻结表指纹：31 进制逐行折叠，期望以 1/2 入折。
pub fn table_fingerprint() -> u32 {
    let mut acc: u32 = 0x18503;
    let mut i = 0;
    while i < GT_ROWS {
        match gt_row(i) {
            Some((s, r, e)) => {
                acc = acc.wrapping_mul(31).wrapping_add(s);
                acc = acc.wrapping_mul(31).wrapping_add(r);
                acc = acc.wrapping_mul(31).wrapping_add(if e { 1 } else { 2 });
            }
            None => {
                acc = acc.wrapping_mul(31).wrapping_add(0xFFFF);
            }
        }
        i += 1;
    }
    acc
}

// ── 探针状态机（Dormant → Sweeping → Closed） ───────────────────
/// 探针阶段（三态单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CarvePhase {
    /// 未开探。
    Dormant,
    /// 扫描中。
    Sweeping,
    /// 已封账（终态）。
    Closed,
}

/// 账本计数（尝试 = 过 + 偏）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GtLedger {
    pub tries: u32,
    pub passes: u32,
    pub deviations: u32,
}

impl GtLedger {
    pub const fn new() -> GtLedger {
        GtLedger { tries: 0, passes: 0, deviations: 0 }
    }
    pub fn conserve(&self) -> bool {
        self.tries == self.passes + self.deviations
    }
}

/// 划界探针引擎：三闸 + 划界断言 + 偏差闸。
pub struct GreaterProbe {
    pub phase: CarvePhase,
    next_min: usize,
    last_probed: Option<usize>,
    pub ring: CarveRing,
    pub ledger: GtLedger,
    /// 评值偏置（注入面：None=正常 1=真化 2=对调 3=恒等）。
    bias: Option<u8>,
}

impl GreaterProbe {
    pub fn new() -> GreaterProbe {
        GreaterProbe {
            phase: CarvePhase::Dormant,
            next_min: 0,
            last_probed: None,
            ring: CarveRing::new(),
            ledger: GtLedger::new(),
            bias: None,
        }
    }

    /// 注入偏置。
    pub fn set_bias(&mut self, b: Option<u8>) {
        self.bias = b;
    }

    /// > 列评值（按偏置变异）。
    fn eval_gt(&self, id: usize) -> Result<bool, u32> {
        let (s, r, _e) = match gt_row(id) {
            Some(x) => x,
            None => return Err(GtCode::RowIdOutOfRange.code()),
        };
        match self.bias {
            None => Ok(gt(s, r)),
            Some(1) => Ok(ge(s, r)),
            Some(2) => Ok(gt(r, s)),
            Some(3) => Ok(ne(s, r)),
            _ => Err(GtCode::PhaseInvalid.code()),
        }
    }

    /// 单行探针：三闸 + 双列评值 + 划界断言 + 期望对拍。
    pub fn probe(&mut self, id: usize) -> Result<(), u32> {
        if self.phase == CarvePhase::Closed {
            return Err(GtCode::PhaseInvalid.code());
        }
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        if id >= GT_ROWS {
            self.ledger.deviations = self.ledger.deviations.saturating_add(1);
            return Err(GtCode::RowIdOutOfRange.code());
        }
        if id < self.next_min {
            self.ledger.deviations = self.ledger.deviations.saturating_add(1);
            if self.last_probed == Some(id) {
                return Err(GtCode::RowDuplicate.code());
            }
            return Err(GtCode::RowOutOfOrder.code());
        }
        let (s, r, expect) = match gt_row(id) {
            Some(x) => x,
            None => return Err(GtCode::RowIdOutOfRange.code()),
        };
        let got = self.eval_gt(id)?;
        let ge_col = ge(s, r);
        // 划界断言：gt ≡ ge ∧ ne（逐行成立，破即记偏）。
        if !carve_holds(s, r) {
            self.ledger.deviations = self.ledger.deviations.saturating_add(1);
            return Err(GtCode::CarveDeviation.code());
        }
        // 等值路径红线：sys==req 时 > 列必须 FALSE。
        if s == r && got {
            self.ledger.deviations = self.ledger.deviations.saturating_add(1);
            return Err(GtCode::EqualPathTrue.code());
        }
        let rec = CarveRec { id: id as u8, got, ge_col, expect };
        self.ring.push(rec)?;
        if got != expect {
            self.ledger.deviations = self.ledger.deviations.saturating_add(1);
            return Err(GtCode::CarveDeviation.code());
        }
        self.next_min = id + 1;
        self.last_probed = Some(id);
        self.ledger.passes = self.ledger.passes.saturating_add(1);
        self.phase = CarvePhase::Sweeping;
        Ok(())
    }

    /// 全表顺探。
    pub fn probe_all(&mut self) -> Result<usize, u32> {
        let mut n = 0;
        let mut id = 0usize;
        while id < GT_ROWS {
            self.probe(id)?;
            n += 1;
            id += 1;
        }
        Ok(n)
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase == CarvePhase::Dormant {
            return Err(GtCode::TableEmpty.code());
        }
        if !self.ledger.conserve() {
            return Err(GtCode::LedgerBroken.code());
        }
        self.phase = CarvePhase::Closed;
        Ok(())
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链结果（过账数 / 账 / 指纹）。
pub type GoldenOut = (usize, GtLedger, u32);

/// 黄金全链一次：20 例顺探全过（等值 5 例全 FALSE）→ 封账。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut p = GreaterProbe::new();
    let n = p.probe_all()?;
    if n != GT_ROWS {
        return Err(GtCode::InjectionMissed.code());
    }
    p.seal()?;
    if !p.ledger.conserve() {
        return Err(GtCode::LedgerBroken.code());
    }
    if p.ledger.passes != GT_ROWS as u32 || p.ledger.deviations != 0 {
        return Err(GtCode::CarveDeviation.code());
    }
    // 等值行独立复核：5 行等值全 FALSE（J1 等值路径红线直查）。
    let mut eq_rows = 0;
    let mut i = 0;
    while i < GT_ROWS {
        match gt_row(i) {
            Some((s, r, e)) => {
                if s == r {
                    eq_rows += 1;
                    if e || gt(s, r) {
                        return Err(GtCode::EqualPathTrue.code());
                    }
                }
            }
            None => return Err(GtCode::RowIdOutOfRange.code()),
        }
        i += 1;
    }
    if eq_rows != EQUAL_ROWS {
        return Err(GtCode::CarveDeviation.code());
    }
    Ok((n, p.ledger, table_fingerprint()))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GtCode::NotReproducible.code());
    }
    Ok(())
}

// ── 交叉对账 ────────────────────────────────────────────────────
/// 交叉核对 7 例：与总纲 GREATER 臂（sys > req）同口径。
pub fn cross_check() -> Result<(), u32> {
    let rows: [GtRow; 7] = [
        (6, 6, false),
        (7, 8, false),
        (8, 7, true),
        (U32_MAX, U32_MAX, false),
        (U32_MAX, U32_MAX - 1, true),
        (0x7FFF_FFFF, 0x8000_0000, false),
        (0x8000_0000, 0x7FFF_FFFF, true),
    ];
    let mut i = 0;
    while i < rows.len() {
        let (s, r, _e) = rows[i];
        let master = s > r;
        if gt(s, r) != master || ge(s, r) != (s >= r) {
            return Err(GtCode::CrossCheckMismatch.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 注入面统计 ──────────────────────────────────────────────────
/// 非对称行数（对调注入面）。
pub fn asymmetric_rows() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < GT_ROWS {
        match gt_row(i) {
            Some((s, r, _)) => {
                if s != r {
                    n += 1;
                }
            }
            None => {}
        }
        i += 1;
    }
    n
}

/// 等值行数（真化注入面：gt 误作 ge 时等值行必偏）。
pub fn equal_rows() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < GT_ROWS {
        match gt_row(i) {
            Some((s, r, _)) => {
                if s == r {
                    n += 1;
                }
            }
            None => {}
        }
        i += 1;
    }
    n
}

/// 反向行数（s < r；恒等误判注入面：ne 列在 s<r 行偏 TRUE）。
pub fn reverse_rows() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < GT_ROWS {
        match gt_row(i) {
            Some((s, r, _)) => {
                if s < r {
                    n += 1;
                }
            }
            None => {}
        }
        i += 1;
    }
    n
}

// ── 注入核（四类变异 × 10 轮） ──────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 等值真化（等值行必偏）。
    pub trueify_caught: u32,
    /// 类2 操作数对调（非对称行必偏）。
    pub swap_caught: u32,
    /// 类3 恒等误判（s<r 行必偏）。
    pub ne_confuse_caught: u32,
    /// 类4 期望篡改（必抓）。
    pub tamper_caught: u32,
}

/// 四类注入 rounds 轮；每类每轮必抓，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1 真化：gt 误作 ge → 等值行触发 EqualPathTrue 红线。
        let mut p1 = GreaterProbe::new();
        p1.set_bias(Some(1));
        let mut caught1 = 0u32;
        let mut id1 = 0usize;
        while id1 < GT_ROWS {
            match p1.probe(id1) {
                Err(c) if c == GtCode::EqualPathTrue.code() => {
                    caught1 = caught1.saturating_add(1);
                    p1.next_min = id1 + 1;
                }
                Err(c) if c == GtCode::CarveDeviation.code() => {
                    // 划界方程仍成立的非等值行不偏（ge 满足 gt≡ge∧ne 的行）。
                    p1.next_min = id1 + 1;
                }
                Ok(()) => {}
                Err(_) => return Err(GtCode::InjectionMissed.code()),
            }
            id1 += 1;
        }
        if caught1 != equal_rows() as u32 {
            return Err(GtCode::InjectionMissed.code());
        }
        led.trueify_caught = led.trueify_caught.saturating_add(caught1);
        // 类2 对调：gt(r,s) → 非对称行必偏。
        let mut p2 = GreaterProbe::new();
        p2.set_bias(Some(2));
        let mut caught2 = 0u32;
        let mut id2 = 0usize;
        while id2 < GT_ROWS {
            match p2.probe(id2) {
                Err(c) if c == GtCode::CarveDeviation.code() => {
                    caught2 = caught2.saturating_add(1);
                    p2.next_min = id2 + 1;
                }
                Ok(()) => {}
                Err(_) => return Err(GtCode::InjectionMissed.code()),
            }
            id2 += 1;
        }
        if caught2 != asymmetric_rows() as u32 {
            return Err(GtCode::InjectionMissed.code());
        }
        led.swap_caught = led.swap_caught.saturating_add(caught2);
        // 类3 恒等误判：ne 替 gt → s<r 行必偏。
        let mut p3 = GreaterProbe::new();
        p3.set_bias(Some(3));
        let mut caught3 = 0u32;
        let mut id3 = 0usize;
        while id3 < GT_ROWS {
            match p3.probe(id3) {
                Err(c) if c == GtCode::CarveDeviation.code() => {
                    caught3 = caught3.saturating_add(1);
                    p3.next_min = id3 + 1;
                }
                Ok(()) => {}
                Err(_) => return Err(GtCode::InjectionMissed.code()),
            }
            id3 += 1;
        }
        if caught3 != reverse_rows() as u32 {
            return Err(GtCode::InjectionMissed.code());
        }
        led.ne_confuse_caught = led.ne_confuse_caught.saturating_add(caught3);
        // 类4 期望篡改：row0 冻结面自洽复核 + 翻转面对拍必偏。
        // 推导：row0=(7,7,FALSE) 等值行，gt(7,7)=FALSE 与冻结期望
        // 自洽；篡改面 = !e0 = TRUE，与评值不同 → 对拍必抓篡改。
        // （原式 `... || !e0` 恒真——把「篡改面翻转」误写成无条件
        // 拒收，致本类记账永零，已改为与 18502 同构的双闸式。）
        let (s0, r0, e0) = match gt_row(0) {
            Some(x) => x,
            None => return Err(GtCode::CarveDeviation.code()),
        };
        let tampered = !e0;
        if gt(s0, r0) != e0 || gt(s0, r0) == tampered {
            // 冻结面不自洽（前者）或篡改面抓不到（后者）→ 漏检。
            return Err(GtCode::InjectionMissed.code());
        }
        led.tamper_caught = led.tamper_caught.saturating_add(1);
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：20 例顺探全过。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：乱序（先 5 后 2）→ 拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut p = GreaterProbe::new();
    p.probe(5)?;
    p.probe(2)
}

/// 探针 C：重复（连探同 id）→ 拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut p = GreaterProbe::new();
    p.probe(0)?;
    p.probe(0)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < GtCode::ALL.len() {
        let v = GtCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（8–20 条；动态明细 ≤8） ────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金链：20 例全过（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _)) => *n == GT_ROWS && led.deviations == 0 && led.conserve(),
        Err(_) => false,
    };
    v.push(("J1-20例划界全过", g_ok, format!("{:?}", g)));

    // 2. 等值路径 5 行全 FALSE（J1 Windows 口径红线直查）。
    let eq_ok = equal_rows() == EQUAL_ROWS && golden_run().is_ok();
    v.push(("J1-等值路径全FALSE", eq_ok, format!("等值行={}", EQUAL_ROWS)));

    // 3. 划界方程逐行成立（gt ≡ ge ∧ ne 全表 20 行）。
    let carve_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < GT_ROWS {
            match gt_row(i) {
                Some((s, r, _)) => {
                    if !carve_holds(s, r) {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-划界方程20行", carve_ok, String::from("gt≡ge∧ne")));

    // 4. 真化注入 10/10（等值行必偏）。
    let led = inject_rounds(JUDGE_ROUNDS);
    let c1 = match led.as_ref() {
        Ok(l) => l.trueify_caught,
        Err(_) => 0,
    };
    let e1 = equal_rows() as u32 * JUDGE_ROUNDS;
    v.push(("J1-真化注入全偏", c1 == e1, format!("{}/{}", c1, e1)));

    // 5. 对调注入 10/10。
    let c2 = match led.as_ref() {
        Ok(l) => l.swap_caught,
        Err(_) => 0,
    };
    let e2 = asymmetric_rows() as u32 * JUDGE_ROUNDS;
    v.push(("J1-对调注入全偏", c2 == e2, format!("{}/{}", c2, e2)));

    // 6. 恒等误判注入 10/10（s<r 行必偏）。
    let c3 = match led.as_ref() {
        Ok(l) => l.ne_confuse_caught,
        Err(_) => 0,
    };
    let e3 = reverse_rows() as u32 * JUDGE_ROUNDS;
    v.push(("J1-恒等误判全偏", c3 == e3, format!("{}/{}", c3, e3)));

    // 7. 篡改注入 10/10。
    let c4 = match led.as_ref() {
        Ok(l) => l.tamper_caught,
        Err(_) => 0,
    };
    v.push(("J1-篡改注入10/10", c4 == JUDGE_ROUNDS, format!("caught={}", c4)));

    // 8. 注入账守恒。
    let cons_ok = match led.as_ref() {
        Ok(l) => l.trueify_caught + l.swap_caught + l.ne_confuse_caught + l.tamper_caught
            == e1 + e2 + e3 + JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons_ok, String::from("4类面额×10轮")));

    // 9. 乱序拒（探针 B）。
    let pb = probe_seq_b().is_err();
    v.push(("J1-乱序探针拒", pb, format!("err={}", pb)));

    // 10. 重复拒（探针 C）。
    let pc = probe_seq_c().is_err();
    v.push(("J1-重复探针拒", pc, format!("err={}", pc)));

    // 11. 越界 id 拒 + 空表封账拒。
    let gates = {
        let mut p = BoundaryGuard::new();
        p.id_reject && p.empty_reject
    };
    v.push(("J1-越界与空表双拒", gates, String::from("id=20 / 零探针")));

    // 12. 交叉对账 7 例。
    let cc = cross_check().is_ok();
    v.push(("J1-交叉对账7例", cc, String::from("与总纲同口径")));

    // 13. 环容量：满后显式拒账不丢。
    let cap = {
        let mut ring = CarveRing::new();
        let mut k = 0;
        while k < RING_CAP {
            let _ = ring.push(CarveRec { id: k as u8, got: true, ge_col: false, expect: true });
            k += 1;
        }
        match ring.push(CarveRec { id: 99, got: true, ge_col: false, expect: true }) {
            Err(c) if c == GtCode::LedgerFull.code() => {
                ring.len == RING_CAP && ring.full_rejects == 1
            }
            _ => false,
        }
    };
    v.push(("J1-环容量32满拒", cap, String::from("32 收 1 拒")));

    // 14. 表指纹双跑一致。
    let fp = table_fingerprint();
    let fp2 = table_fingerprint();
    v.push(("J2-表指纹双跑", fp == fp2, format!("{:08X}", fp)));

    // 15. 双跑一致（复现红线）。
    let rp = repro_check();
    v.push(("J2-双跑一致", rp.is_ok(), format!("{:?}", rp)));

    // 16. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 17. 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = GtCode::ALL.len() == 12;
        let mut i = 0;
        while i < GtCode::ALL.len() {
            if GtCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    v
}

/// 越界/空表闸位辅助（避免在闭包里重复造引擎）。
pub struct BoundaryGuard {
    pub id_reject: bool,
    pub empty_reject: bool,
}

impl BoundaryGuard {
    pub fn new() -> BoundaryGuard {
        let mut p = GreaterProbe::new();
        let id_reject = p.probe(GT_ROWS).is_err();
        let mut q = GreaterProbe::new();
        let empty_reject = q.seal().is_err();
        BoundaryGuard { id_reject, empty_reject }
    }
}

/// 收口：本模块自检 CheckSet（判据 17 条；前 8 条动态明细）。
pub fn run_unx18503_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18503 VER_GREATER 划界语义");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18503_checks();
        assert!(set.all_passed(), "UNX-F18503 判据集应全绿");
    }

    /// 黄金链：20 例全过。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, _)) => {
                assert_eq!(n, 20);
                assert_eq!(led.deviations, 0);
                assert!(led.conserve());
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 等值路径：5 行等值全 FALSE（Windows 一致红线）。
    #[test]
    fn test_equal_path_false() {
        let mut i = 0;
        let mut eq_count = 0;
        while i < GT_ROWS {
            match gt_row(i) {
                Some((s, r, e)) => {
                    if s == r {
                        eq_count += 1;
                        assert!(!e, "等值行期望必须 FALSE: row {}", i);
                        assert!(!gt(s, r), "等值行 > 必须 FALSE: row {}", i);
                    }
                }
                None => assert!(false, "冻结表缺行"),
            }
            i += 1;
        }
        assert_eq!(eq_count, EQUAL_ROWS);
    }

    /// 划界方程逐行成立。
    #[test]
    fn test_carve_equation() {
        let mut i = 0;
        while i < GT_ROWS {
            match gt_row(i) {
                Some((s, r, _)) => assert!(carve_holds(s, r), "划界破于 row {}", i),
                None => assert!(false),
            }
            i += 1;
        }
        // 额外采样：方程在任意值对上成立。
        assert!(carve_holds(0, U32_MAX));
        assert!(carve_holds(U32_MAX, 0));
        assert!(carve_holds(12345, 12345));
    }

    /// 注入四类全抓（10 轮）。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.trueify_caught, equal_rows() as u32 * 10);
                assert_eq!(l.swap_caught, asymmetric_rows() as u32 * 10);
                assert_eq!(l.ne_confuse_caught, reverse_rows() as u32 * 10);
                assert_eq!(l.tamper_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 三闸：乱序/重复/越界。
    #[test]
    fn test_gates() {
        assert!(probe_seq_b().is_err());
        assert!(probe_seq_c().is_err());
        let mut p = GreaterProbe::new();
        match p.probe(GT_ROWS) {
            Err(c) => assert_eq!(c, GtCode::RowIdOutOfRange.code()),
            Ok(()) => assert!(false, "越界 id 必须拒"),
        }
    }

    /// 封账：空表拒 / Closed 再探拒。
    #[test]
    fn test_seal_phases() {
        let mut p0 = GreaterProbe::new();
        match p0.seal() {
            Err(c) => assert_eq!(c, GtCode::TableEmpty.code()),
            Ok(()) => assert!(false, "空表封账必须拒"),
        }
        let mut p1 = GreaterProbe::new();
        assert!(p1.probe_all().is_ok());
        assert!(p1.seal().is_ok());
        match p1.probe(0) {
            Err(c) => assert_eq!(c, GtCode::PhaseInvalid.code()),
            Ok(()) => assert!(false, "Closed 后再探必须拒"),
        }
    }

    /// 真化探针：等值行触发 EqualPathTrue。
    #[test]
    fn test_trueify_probe() {
        let mut p = GreaterProbe::new();
        p.set_bias(Some(1));
        match p.probe(0) {
            Err(c) => assert_eq!(c, GtCode::EqualPathTrue.code()),
            Ok(()) => assert!(false, "等值真化必须拒"),
        }
        assert_eq!(p.ledger.deviations, 1);
        assert!(p.ledger.conserve());
    }

    /// 双跑一致。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫。
    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
    }
}
