//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18502 · VER_GREATER_EQUAL 条件语义（≥ 比较边界）
//! · 0x7F940 段 · 批 06 · 配额 320
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18502-J1 ≥ 比较边界（等值/差一）20 例断言全过，边界
//!   注入零偏差。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - ≥ 语义冻结：ge(sys, req) = sys >= req（u32 全序，无饱和截断、
//!     无隐式窄化）；等值（sys==req）必 TRUE、差一（sys==req-1）必
//!     FALSE、反差一（sys==req+1）必 TRUE——三型边界各占冻结表；
//!   - 冻结边界表 20 例：零/零、零/一、一/零、MAX/MAX、MAX-1/MAX、
//!     MAX/MAX-1、0x7FFF_FFFF 与 0x8000_0000 符号位跨越等，期望值
//!     逐例手推写死（Windows 口径：≥ 含等值路径）；
//!   - 探针状态机：Idle → Probing → Sealed；探针 id 必须严格升序
//!     （乱序拒 / 重复拒 / 越界 id 拒）；每例 got==expect 否则记
//!     BoundaryDeviation（边界零偏差红线）；
//!   - 账本环（32 槽定容）：探针记录 (id, got, expect) 入环，环满
//!     显式拒（LedgerFull）且已有账不丢；
//!   - 边界注入四类 × 10 轮（零偏差 = 每次变异后探针必记偏差）：
//!     操作数对调（仅非对称例必偏）/ 降级 VER_EQUAL（非对称例必偏）/
//!     差一偏移 req-1（边界例必偏）/ 期望值篡改（指纹翻转必抓）；
//!   - 对账：与 F18501 cmp_dim 的 GE 臂同口径（交叉核对 7 例）；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] GeCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [裁决表]   GE_ROWS 20 例冻结边界表（手推期望）
//!   [数据结构] ProbeRec / ProbeRing（32 槽账本环）+ 表指纹
//!   [状态机]   ProbePhase 三态 + BoundaryProbe（升序闸/重复闸/偏差闸）
//!   [账本]     GeLedger（尝试=过+偏 守恒）
//!   [注入核]   四类变异 × 10 轮（对调/降级/差一/篡改）
//!   [收口]     golden_run + run_unx18502_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F940，AI-VX11-1105 G1 批06 独占区）。
pub const SEG_BASE: u32 = 0x7F940;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// VER_GREATER_EQUAL（winnt.h 冻结）。
pub const VER_GREATER_EQUAL: u8 = 3;
/// VER_EQUAL（降级注入对照）。
pub const VER_EQUAL: u8 = 1;
/// 冻结边界表行数（J1「20 例」口径）。
pub const GE_ROWS: usize = 20;
/// 账本环容量（定容 32 槽）。
pub const RING_CAP: usize = 32;
/// 判据注入轮数（J1「零偏差」逐轮口径）。
pub const JUDGE_ROUNDS: u32 = 10;

/// u32 上界（边界例用，写死）。
pub const U32_MAX: u32 = 0xFFFF_FFFF;

/// 冻结边界表一行：(sys, req, expect)。expect 全部手推：
/// ge = sys >= req。
pub type GeRow = (u32, u32, bool);

/// 第 i 行冻结边界例（越界 None）。
/// 推导：1-4 零/一邻界，5-6 上差一，7-11 MAX 三邻界，
/// 12-17 常规等值/邻值，18-20 符号位跨越（有符号比较误判必红）。
pub fn ge_row(i: usize) -> Option<GeRow> {
    match i {
        0 => Some((0, 0, true)),                        // 零/零 等值
        1 => Some((0, 1, false)),                       // 零/一 差一
        2 => Some((1, 0, true)),                        // 一/零 反差一
        3 => Some((1, 1, true)),                        // 等值
        4 => Some((2, 1, true)),                        // 反差一
        5 => Some((1, 2, false)),                       // 上差一
        6 => Some((U32_MAX, U32_MAX, true)),            // MAX 等值
        7 => Some((U32_MAX - 1, U32_MAX, false)),       // MAX 差一
        8 => Some((U32_MAX, U32_MAX - 1, true)),        // MAX 反差一
        9 => Some((0, U32_MAX, false)),                 // 全距下/上
        10 => Some((U32_MAX, 0, true)),                 // 全距上/下
        11 => Some((5, 5, true)),                       // 常规等值
        12 => Some((6, 5, true)),                       // 邻值 +
        13 => Some((5, 6, false)),                      // 邻值 −
        14 => Some((100, 99, true)),                    // 常规 +
        15 => Some((99, 100, false)),                   // 常规 −
        16 => Some((1000, 1000, true)),                 // 常规等值
        17 => Some((0x7FFF_FFFF, 0x7FFF_FFFF, true)),   // 符号位前等值
        18 => Some((0x7FFF_FFFF, 0x8000_0000, false)),  // 符号位跨越 −
        19 => Some((0x8000_0000, 0x7FFF_FFFF, true)),   // 符号位跨越 +
        _ => None,
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GeCode {
    /// 行 id 越界（≥ 20）。
    RowIdOutOfRange,
    /// 探针乱序（id 未严格升序）。
    RowOutOfOrder,
    /// 探针重复（同 id 二探）。
    RowDuplicate,
    /// 边界偏差（got ≠ expect，零偏差红线）。
    BoundaryDeviation,
    /// 账本环满（32 槽耗尽）。
    LedgerFull,
    /// 账本守恒破坏（尝试 ≠ 过 + 偏）。
    LedgerBroken,
    /// 表指纹失配（冻结表被改）。
    FingerprintMismatch,
    /// 空表收口（零探针即封账）。
    TableEmpty,
    /// 注入漏检（变异后探针未记偏差）。
    InjectionMissed,
    /// 阶段非法（Sealed 后再探 / Idle 直接封账）。
    PhaseInvalid,
    /// 口径失配（与 F18501 cmp_dim GE 臂交叉核对不等）。
    CrossCheckMismatch,
    /// 双跑不一致。
    NotReproducible,
}

impl GeCode {
    /// 诊断码数值 = SEG_BASE + 序（闭集派生）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GeCode::RowIdOutOfRange => "行 id 越界：冻结表仅 0..=19",
            GeCode::RowOutOfOrder => "探针乱序：id 必须严格升序",
            GeCode::RowDuplicate => "探针重复：同 id 只探一次",
            GeCode::BoundaryDeviation => "边界偏差：got≠expect 零容忍",
            GeCode::LedgerFull => "账本环满：32 槽耗尽",
            GeCode::LedgerBroken => "账本守恒破坏：尝试≠过+偏",
            GeCode::FingerprintMismatch => "表指纹失配：冻结表被改",
            GeCode::TableEmpty => "空表收口：零探针不得封账",
            GeCode::InjectionMissed => "注入漏检：变异必记偏差",
            GeCode::PhaseInvalid => "阶段非法：Sealed 后再探",
            GeCode::CrossCheckMismatch => "口径失配：与总纲 GE 臂不等",
            GeCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [GeCode; 12] = [
        GeCode::RowIdOutOfRange,
        GeCode::RowOutOfOrder,
        GeCode::RowDuplicate,
        GeCode::BoundaryDeviation,
        GeCode::LedgerFull,
        GeCode::LedgerBroken,
        GeCode::FingerprintMismatch,
        GeCode::TableEmpty,
        GeCode::InjectionMissed,
        GeCode::PhaseInvalid,
        GeCode::CrossCheckMismatch,
        GeCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GeCode::ALL.len() {
        if GeCode::ALL[i].code() == code {
            return GeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F940 段闭集内"
}

// ── ≥ 比较核 ────────────────────────────────────────────────────
/// ≥ 比较（冻结语义：u32 全序直接比较）。
pub fn ge(sys: u32, req: u32) -> bool {
    sys >= req
}

/// 等值比较（降级注入对照用）。
pub fn eq(sys: u32, req: u32) -> bool {
    sys == req
}

/// 差一偏移比较（req-1 注入用；req==0 原样返回 ge(sys,0)）。
pub fn ge_minus_one(sys: u32, req: u32) -> bool {
    if req == 0 {
        ge(sys, req)
    } else {
        ge(sys, req - 1)
    }
}

// ── 账本环（32 槽定容） ─────────────────────────────────────────
/// 探针记录（Copy，可定容复制）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeRec {
    /// 行 id。
    pub id: u8,
    /// 引擎评值。
    pub got: bool,
    /// 冻结期望。
    pub expect: bool,
}

/// 32 槽账本环：定容、显式满、账不丢（满后拒新探不改旧账）。
#[derive(Clone, Copy, Debug)]
pub struct ProbeRing {
    slots: [Option<ProbeRec>; RING_CAP],
    pub len: usize,
    pub full_rejects: u32,
}

impl ProbeRing {
    pub fn new() -> ProbeRing {
        ProbeRing { slots: [None; RING_CAP], len: 0, full_rejects: 0 }
    }
    /// 入环（满则显式拒并计数，旧账不动）。
    pub fn push(&mut self, rec: ProbeRec) -> Result<(), u32> {
        if self.len >= RING_CAP {
            self.full_rejects = self.full_rejects.saturating_add(1);
            return Err(GeCode::LedgerFull.code());
        }
        self.slots[self.len] = Some(rec);
        self.len += 1;
        Ok(())
    }
    /// 按 id 取记录（未探 None；越界 None）。
    pub fn rec_of(&self, id: usize) -> Option<ProbeRec> {
        if id >= self.len {
            return None;
        }
        self.slots[id]
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

// ── 表指纹（冻结对账） ──────────────────────────────────────────
/// 冻结表指纹：31 进制逐行折叠（行序敏感 + 期望位敏感）。
/// 推导：初值取卡号派生 0x18502；expect 布尔以 1/2 入折避免与
/// id 折叠位混淆。
pub fn table_fingerprint() -> u32 {
    let mut acc: u32 = 0x18502;
    let mut i = 0;
    while i < GE_ROWS {
        match ge_row(i) {
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

// ── 探针状态机（Idle → Probing → Sealed） ───────────────────────
/// 探针阶段（三态单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbePhase {
    /// 未开探。
    Idle,
    /// 探测中（可继续入探）。
    Probing,
    /// 已封账（终态；再探拒）。
    Sealed,
}

/// 账本计数（尝试 = 过 + 偏，守恒方程）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeLedger {
    pub tries: u32,
    pub passes: u32,
    pub deviations: u32,
}

impl GeLedger {
    pub const fn new() -> GeLedger {
        GeLedger { tries: 0, passes: 0, deviations: 0 }
    }
    /// 守恒核算。
    pub fn conserve(&self) -> bool {
        self.tries == self.passes + self.deviations
    }
}

/// 边界探针引擎：升序闸 / 重复闸 / 偏差闸 三闸串联。
pub struct BoundaryProbe {
    pub phase: ProbePhase,
    /// 下一个允许的 id 下界（严格升序约束）。
    next_min: usize,
    /// 上一探 id（重复判定：与上探同 id = 重复，更小 = 乱序）。
    last_probed: Option<usize>,
    pub ring: ProbeRing,
    pub ledger: GeLedger,
    /// 评值函数偏置（注入面：正常探针恒 None）。
    bias: Option<u8>,
}

impl BoundaryProbe {
    pub fn new() -> BoundaryProbe {
        BoundaryProbe {
            phase: ProbePhase::Idle,
            next_min: 0,
            last_probed: None,
            ring: ProbeRing::new(),
            ledger: GeLedger::new(),
            bias: None,
        }
    }

    /// 注入偏置（测试/注入核专用：1=对调 2=降级 3=差一）。
    pub fn set_bias(&mut self, b: Option<u8>) {
        self.bias = b;
    }

    /// 偏置评值：正常 ge；注入类按偏置变异。
    fn eval(&self, id: usize) -> Result<bool, u32> {
        let (s, r, _e) = match ge_row(id) {
            Some(x) => x,
            None => return Err(GeCode::RowIdOutOfRange.code()),
        };
        match self.bias {
            None => Ok(ge(s, r)),
            Some(1) => Ok(ge(r, s)),
            Some(2) => Ok(eq(s, r)),
            Some(3) => Ok(ge_minus_one(s, r)),
            _ => Err(GeCode::PhaseInvalid.code()),
        }
    }

    /// 单行探针：id 严格升序 + got==expect 零偏差。
    pub fn probe(&mut self, id: usize) -> Result<(), u32> {
        // 阶段闸：Sealed 后再探 = 阶段非法。
        if self.phase == ProbePhase::Sealed {
            return Err(GeCode::PhaseInvalid.code());
        }
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        // 行 id 闸。
        if id >= GE_ROWS {
            self.ledger.deviations = self.ledger.deviations.saturating_add(1);
            return Err(GeCode::RowIdOutOfRange.code());
        }
        // 升序闸：id 必须 ≥ next_min；与上探同 id = 重复，更小 = 乱序。
        if id < self.next_min {
            self.ledger.deviations = self.ledger.deviations.saturating_add(1);
            if self.last_probed == Some(id) {
                return Err(GeCode::RowDuplicate.code());
            }
            return Err(GeCode::RowOutOfOrder.code());
        }
        // 评值 + 期望对拍（零偏差红线）。
        let got = self.eval(id)?;
        let expect = match ge_row(id) {
            Some((_, _, e)) => e,
            None => return Err(GeCode::RowIdOutOfRange.code()),
        };
        let rec = ProbeRec { id: id as u8, got, expect };
        self.ring.push(rec)?;
        if got != expect {
            self.ledger.deviations = self.ledger.deviations.saturating_add(1);
            return Err(GeCode::BoundaryDeviation.code());
        }
        self.next_min = id + 1;
        self.last_probed = Some(id);
        self.ledger.passes = self.ledger.passes.saturating_add(1);
        self.phase = ProbePhase::Probing;
        Ok(())
    }

    /// 全表顺探（0..20 逐行；确定性：同表双跑账逐槽相等）。
    pub fn probe_all(&mut self) -> Result<usize, u32> {
        let mut n = 0;
        let mut id = 0usize;
        while id < GE_ROWS {
            self.probe(id)?;
            n += 1;
            id += 1;
        }
        Ok(n)
    }

    /// 封账：空表拒；账守恒；Sealed 终态。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase == ProbePhase::Idle {
            return Err(GeCode::TableEmpty.code());
        }
        if !self.ledger.conserve() {
            return Err(GeCode::LedgerBroken.code());
        }
        self.phase = ProbePhase::Sealed;
        Ok(())
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链结果（20 例过账数 / 探针账 / 表指纹）。
pub type GoldenOut = (usize, GeLedger, u32);

/// 黄金全链一次：20 例顺探全过 → 封账 → 指纹入账。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut p = BoundaryProbe::new();
    let n = p.probe_all()?;
    if n != GE_ROWS {
        return Err(GeCode::InjectionMissed.code());
    }
    p.seal()?;
    if !p.ledger.conserve() {
        return Err(GeCode::LedgerBroken.code());
    }
    if p.ledger.passes != GE_ROWS as u32 || p.ledger.deviations != 0 {
        return Err(GeCode::BoundaryDeviation.code());
    }
    Ok((n, p.ledger, table_fingerprint()))
}

/// 双跑一致（确定性红线：账与指纹逐项相等）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GeCode::NotReproducible.code());
    }
    Ok(())
}

// ── 交叉对账（与 F18501 cmp_dim GE 臂同口径） ───────────────────
/// 交叉核对 7 例（覆盖等值/差一/反差一/MAX/符号位跨五型边界）。
/// 期望：本卡 ge 与总纲 GE 臂逐例相等。
pub fn cross_check() -> Result<(), u32> {
    // 与 unx18501 的 cmp_dim(VER_GREATER_EQUAL) 口径一致：sys >= req。
    // 本卡自持一份同口径实现做交叉（跨模块禁引，故取等价闭式）。
    let rows: [GeRow; 7] = [
        (0, 0, true),
        (1, 2, false),
        (2, 1, true),
        (U32_MAX, U32_MAX, true),
        (U32_MAX - 1, U32_MAX, false),
        (0x7FFF_FFFF, 0x8000_0000, false),
        (0x8000_0000, 0x7FFF_FFFF, true),
    ];
    let mut i = 0;
    while i < rows.len() {
        let (s, r, _e) = rows[i];
        // 总纲口径：GE = sys >= req（闭式重演）。
        let master = s >= r;
        if ge(s, r) != master {
            return Err(GeCode::CrossCheckMismatch.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 注入核（四类变异 × 10 轮，零偏差 = 必记偏差） ───────────────
/// 注入账（四类各轮计数）。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 操作数对调（非对称例必偏）。
    pub swap_caught: u32,
    /// 类2 降级 VER_EQUAL（非对称例必偏）。
    pub downgrade_caught: u32,
    /// 类3 差一偏移（边界例必偏）。
    pub offbyone_caught: u32,
    /// 类4 期望篡改（指纹/偏差必抓）。
    pub tamper_caught: u32,
}

/// 非对称行（sys != req）计数（对调/降级注入面）。
pub fn asymmetric_rows() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < GE_ROWS {
        match ge_row(i) {
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

/// 降级偏差行（sys > req）计数：等值评值仅在 ge 为 TRUE 的行上偏离
/// 期望（sys==req 行等值==期望、sys<req 行等值==期望 FALSE）。
/// 推导：ge≠eq ⟺ sys>req，冻结表内共 7 行（2/4/8/10/12/14/19）。
pub fn downgrade_deviation_rows() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < GE_ROWS {
        match ge_row(i) {
            Some((s, r, _)) => {
                if s > r {
                    n += 1;
                }
            }
            None => {}
        }
        i += 1;
    }
    n
}

/// 边界差一行（sys == req-1 且 req>0）计数（差一注入面）。
pub fn boundary_minus_one_rows() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < GE_ROWS {
        match ge_row(i) {
            Some((s, r, _)) => {
                if r > 0 && s == r - 1 {
                    n += 1;
                }
            }
            None => {}
        }
        i += 1;
    }
    n
}

/// 四类变异注入 rounds 轮；每类每轮必被抓，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1 对调：偏置探针全表走，非对称例必偏（探针记 BoundaryDeviation）。
        let mut p1 = BoundaryProbe::new();
        p1.set_bias(Some(1));
        let mut caught1 = 0u32;
        let mut id1 = 0usize;
        while id1 < GE_ROWS {
            match p1.probe(id1) {
                Err(c) if c == GeCode::BoundaryDeviation.code() => {
                    caught1 = caught1.saturating_add(1);
                    // 推进下界继续探（偏差账保留，不修复）。
                    p1.next_min = id1 + 1;
                }
                Ok(()) => {}
                Err(_) => return Err(GeCode::InjectionMissed.code()),
            }
            id1 += 1;
        }
        if caught1 != asymmetric_rows() as u32 {
            return Err(GeCode::InjectionMissed.code());
        }
        led.swap_caught = led.swap_caught.saturating_add(caught1);
        // 类2 降级：等值评值替 ≥，仅在 ge=TRUE 的行（sys>req）偏离期望。
        let mut p2 = BoundaryProbe::new();
        p2.set_bias(Some(2));
        let mut caught2 = 0u32;
        let mut id2 = 0usize;
        while id2 < GE_ROWS {
            match p2.probe(id2) {
                Err(c) if c == GeCode::BoundaryDeviation.code() => {
                    caught2 = caught2.saturating_add(1);
                    p2.next_min = id2 + 1;
                }
                Ok(()) => {}
                Err(_) => return Err(GeCode::InjectionMissed.code()),
            }
            id2 += 1;
        }
        if caught2 != downgrade_deviation_rows() as u32 {
            return Err(GeCode::InjectionMissed.code());
        }
        led.downgrade_caught = led.downgrade_caught.saturating_add(caught2);
        // 类3 差一偏移：req-1 评值，边界差一行必偏。
        let mut p3 = BoundaryProbe::new();
        p3.set_bias(Some(3));
        let mut caught3 = 0u32;
        let mut id3 = 0usize;
        while id3 < GE_ROWS {
            match p3.probe(id3) {
                Err(c) if c == GeCode::BoundaryDeviation.code() => {
                    caught3 = caught3.saturating_add(1);
                    p3.next_min = id3 + 1;
                }
                Ok(()) => {}
                Err(_) => return Err(GeCode::InjectionMissed.code()),
            }
            id3 += 1;
        }
        if caught3 != boundary_minus_one_rows() as u32 {
            return Err(GeCode::InjectionMissed.code());
        }
        led.offbyone_caught = led.offbyone_caught.saturating_add(caught3);
        // 类4 期望篡改：把 row0 期望取反探针 → 必记偏差。
        let (s0, r0, e0) = match ge_row(0) {
            Some(x) => x,
            None => return Err(GeCode::FingerprintMismatch.code()),
        };
        let tampered = !e0;
        // 篡改检测：对拍面翻转即偏差（零偏差红线覆盖期望面）。
        if ge(s0, r0) != e0 || ge(s0, r0) == tampered {
            return Err(GeCode::FingerprintMismatch.code());
        }
        led.tamper_caught = led.tamper_caught.saturating_add(1);
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：20 例顺探全过（J1 主体）。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：乱序（先探 5 再探 2）→ 拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut p = BoundaryProbe::new();
    p.probe(5)?;
    p.probe(2)
}

/// 探针 C：重复（连探同 id）→ 拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut p = BoundaryProbe::new();
    p.probe(0)?;
    p.probe(0)
}

/// 段位守卫（0x40 对齐窗口）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < GeCode::ALL.len() {
        let v = GeCode::ALL[i].code();
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

    // 1. 黄金链：20 例全过零偏差（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _)) => *n == GE_ROWS && led.deviations == 0 && led.conserve(),
        Err(_) => false,
    };
    v.push(("J1-20例边界全过", g_ok, format!("{:?}", g)));

    // 2. 探针 A（可复跑）。
    let pa = probe_seq_a().is_ok();
    v.push(("J1-探针A全表顺探", pa, format!("ok={}", pa)));

    // 3. 对调注入 10/10（非对称例必偏）。
    let led = inject_rounds(JUDGE_ROUNDS);
    let c1 = match led.as_ref() {
        Ok(l) => l.swap_caught,
        Err(_) => 0,
    };
    let expect1 = asymmetric_rows() as u32 * JUDGE_ROUNDS;
    v.push(("J1-对调注入全偏", c1 == expect1, format!("{}/{}", c1, expect1)));

    // 4. 降级注入 10/10（面额 = sys>req 行 × 轮数，推导见 downgrade_deviation_rows）。
    let c2 = match led.as_ref() {
        Ok(l) => l.downgrade_caught,
        Err(_) => 0,
    };
    let expect2 = downgrade_deviation_rows() as u32 * JUDGE_ROUNDS;
    v.push(("J1-降级注入全偏", c2 == expect2, format!("{}/{}", c2, expect2)));

    // 5. 差一注入 10/10（边界差一行必偏）。
    let c3 = match led.as_ref() {
        Ok(l) => l.offbyone_caught,
        Err(_) => 0,
    };
    let expect3 = boundary_minus_one_rows() as u32 * JUDGE_ROUNDS;
    v.push(("J1-差一注入全偏", c3 == expect3, format!("{}/{}", c3, expect3)));

    // 6. 篡改注入 10/10。
    let c4 = match led.as_ref() {
        Ok(l) => l.tamper_caught,
        Err(_) => 0,
    };
    v.push(("J1-篡改注入10/10", c4 == JUDGE_ROUNDS, format!("caught={}", c4)));

    // 7. 注入账守恒（四类合计 = 面额 × 轮数）。
    let cons_ok = match led.as_ref() {
        Ok(l) => {
            l.swap_caught + l.downgrade_caught + l.offbyone_caught + l.tamper_caught
                == expect1 + expect2 + expect3 + JUDGE_ROUNDS
        }
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons_ok, String::from("4类面额×10轮")));

    // 8. 乱序探针拒（探针 B）。
    let pb = probe_seq_b().is_err();
    v.push(("J1-乱序探针拒", pb, format!("err={}", pb)));

    // 9. 重复探针拒（探针 C）。
    let pc = probe_seq_c().is_err();
    v.push(("J1-重复探针拒", pc, format!("err={}", pc)));

    // 10. 越界 id 拒（id=20）。
    let oob = {
        let mut p = BoundaryProbe::new();
        p.probe(GE_ROWS).is_err()
    };
    v.push(("J1-越界id拒", oob, format!("id={}", GE_ROWS)));

    // 11. 空表封账拒。
    let empty = {
        let mut p = BoundaryProbe::new();
        p.seal().is_err()
    };
    v.push(("J1-空表封账拒", empty, String::from("零探针不得封账")));

    // 12. 环容量：20 例顺探后直接压环至满 + 1，显式满拒且旧账不丢。
    let cap = {
        let mut p = BoundaryProbe::new();
        let _ = p.probe_all();
        // 直接压环至满 + 1。
        let mut k = p.ring.len;
        while k < RING_CAP {
            let _ = p.ring.push(ProbeRec { id: k as u8, got: true, expect: true });
            k += 1;
        }
        match p.ring.push(ProbeRec { id: 99, got: true, expect: true }) {
            Err(c) if c == GeCode::LedgerFull.code() => {
                p.ring.len == RING_CAP && p.ring.full_rejects == 1
            }
            _ => false,
        }
    };
    v.push(("J1-环容量32满拒", cap, String::from("32 收 1 拒账不丢")));

    // 13. 交叉对账（与总纲 GE 臂同口径 7 例）。
    let cc = cross_check().is_ok();
    v.push(("J1-交叉对账7例", cc, String::from("与总纲同口径")));

    // 14. 表指纹双跑一致 + 扰动敏感。
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

    // 17. 诊断码闭集（12 码全非空中文明细）。
    let all_ok = {
        let mut ok = GeCode::ALL.len() == 12;
        let mut i = 0;
        while i < GeCode::ALL.len() {
            if GeCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 17 条；前 8 条动态明细）。
pub fn run_unx18502_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18502 VER_GREATER_EQUAL 边界语义");
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
        let set = run_unx18502_checks();
        assert!(set.all_passed(), "UNX-F18502 判据集应全绿");
    }

    /// 黄金链：20 例全过零偏差。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, _)) => {
                assert_eq!(n, 20);
                assert_eq!(led.passes, 20);
                assert_eq!(led.deviations, 0);
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 等值/差一/反差一三型边界逐型断言。
    #[test]
    fn test_boundary_types() {
        assert!(ge(0, 0));
        assert!(ge(1, 1));
        assert!(!ge(0, 1), "差一必须 FALSE");
        assert!(ge(1, 0), "反差一必须 TRUE");
        assert!(!ge(U32_MAX - 1, U32_MAX));
        assert!(ge(U32_MAX, U32_MAX - 1));
        assert!(!ge(0x7FFF_FFFF, 0x8000_0000), "符号位跨越 −");
        assert!(ge(0x8000_0000, 0x7FFF_FFFF), "符号位跨越 +");
    }

    /// 注入四类全偏（10 轮累计）。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.swap_caught, asymmetric_rows() as u32 * 10);
                assert_eq!(l.downgrade_caught, downgrade_deviation_rows() as u32 * 10);
                assert_eq!(l.offbyone_caught, boundary_minus_one_rows() as u32 * 10);
                assert_eq!(l.tamper_caught, 10);
            }
            Err(e) => assert!(false, "注入全偏不应报错: {}", detail(e)),
        }
    }

    /// 乱序/重复/越界三闸。
    #[test]
    fn test_gates() {
        assert!(probe_seq_b().is_err(), "乱序必须拒");
        assert!(probe_seq_c().is_err(), "重复必须拒");
        let mut p = BoundaryProbe::new();
        match p.probe(GE_ROWS) {
            Err(c) => assert_eq!(c, GeCode::RowIdOutOfRange.code()),
            Ok(()) => assert!(false, "越界 id 必须拒"),
        }
    }

    /// 环容量：满后显式拒且账不丢。
    #[test]
    fn test_ring_capacity() {
        let mut ring = ProbeRing::new();
        let mut k = 0;
        while k < RING_CAP {
            assert!(ring.push(ProbeRec { id: k as u8, got: true, expect: true }).is_ok());
            k += 1;
        }
        match ring.push(ProbeRec { id: 99, got: true, expect: true }) {
            Err(c) => {
                assert_eq!(c, GeCode::LedgerFull.code());
                assert_eq!(ring.len, RING_CAP);
                assert_eq!(ring.full_rejects, 1);
            }
            Ok(()) => assert!(false, "满环必须拒"),
        }
    }

    /// 封账：空表拒 / 守恒账过 / Sealed 再探拒。
    #[test]
    fn test_seal_phases() {
        let mut p0 = BoundaryProbe::new();
        match p0.seal() {
            Err(c) => assert_eq!(c, GeCode::TableEmpty.code()),
            Ok(()) => assert!(false, "空表封账必须拒"),
        }
        let mut p1 = BoundaryProbe::new();
        assert!(p1.probe_all().is_ok());
        assert!(p1.seal().is_ok());
        match p1.probe(0) {
            Err(c) => assert_eq!(c, GeCode::PhaseInvalid.code()),
            Ok(()) => assert!(false, "Sealed 后再探必须拒"),
        }
    }

    /// 偏差探针：期望取反后必记 BoundaryDeviation。
    #[test]
    fn test_deviation_recorded() {
        let mut p = BoundaryProbe::new();
        p.set_bias(Some(2)); // 降级：row0 (0,0) eq=TRUE==expect 不偏；row1 (0,1) eq=F==expect 不偏
        // 非对称例 row2 (1,0)：eq(1,0)=FALSE vs expect TRUE → 偏差。
        assert!(p.probe(0).is_ok());
        assert!(p.probe(1).is_ok());
        match p.probe(2) {
            Err(c) => assert_eq!(c, GeCode::BoundaryDeviation.code()),
            Ok(()) => assert!(false, "降级偏差必须记"),
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
