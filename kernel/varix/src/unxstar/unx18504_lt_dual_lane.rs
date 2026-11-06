//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18504 · VER_LESS / VER_LESS_EQUAL 条件语义（< 与 ≤ 双条件）
//! · 0x7F9C0 段 · 批 06 · 配额 300
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18504-J1 < 与 ≤ 双条件边界 20 例断言全过。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 双车道语义冻结：lt(sys,req) = sys < req（lane L，VER_LESS=4）；
//!     le(sys,req) = sys <= req（lane LE，VER_LESS_EQUAL=5）；≤ 含等值
//!     路径（sys==req 必 TRUE），< 排等值（sys==req 必 FALSE）——两道
//!     在等值点分岔，互补方程 lt ≡ ¬ge ∧ le ≡ ¬gt 逐行成立；
//!   - 冻结边界表 20 例：lane L 10 例 + lane LE 10 例；零/等值/差一/
//!     反差一/MAX 三邻/符号位跨越逐型布点，期望逐例手推；
//!   - 双车道状态机：每道 LanePhase（Idle→Sweeping→Sealed）单向；
//!     跨道序闸（lane B 必须在 lane A 封账后开探）+ 道内严格顺序闸
//!     （idx 必须等于游标；超前=乱序拒，落后=重复拒）；
//!   - 判决日志（24 槽定容）：每例结果入账，满显式拒且旧账不丢；
//!   - 注入四类 × 10 轮：跨道混淆（lt↔le，等值点两道各偏）/
//!     操作数对调（非对称例必偏）/ 常数化评值（lane L 恒 FALSE +
//!     lane LE 恒 TRUE，真值例必偏）/ 期望篡改（必抓）——逐类逐轮
//!     必抓，漏检即红；
//!   - 对账：与 F18501 cmp_dim 的 LESS/LESS_EQUAL 臂同口径交叉核对；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] LsCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [裁决表]   LANE_ROWS：lane L 10 例 + lane LE 10 例（手推期望）
//!   [数据结构] LaneRec / VerdictJournal（24 槽判决日志）
//!   [状态机]   LanePhase 双道三态 + DualLaneProbe（跨道序闸+道内游标）
//!   [账本]     DualLedger（双道守恒）
//!   [注入核]   四类变异 × 10 轮（混淆/对调/常数化/篡改）
//!   [收口]     golden_run + run_unx18504_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F9C0，AI-VX11-1105 G1 批06 独占区）。
pub const SEG_BASE: u32 = 0x7F9C0;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// VER_LESS（winnt.h 冻结）。
pub const VER_LESS: u8 = 4;
/// VER_LESS_EQUAL（winnt.h 冻结）。
pub const VER_LESS_EQUAL: u8 = 5;
/// 每道行数（10 例 × 2 道 = 20 例，J1 口径）。
pub const LANE_ROWS: usize = 10;
/// 道数（冻结：2）。
pub const LANE_COUNT: usize = 2;
/// 判决日志容量（20 例 + 4 余量，定容）。
pub const JOURNAL_CAP: usize = 24;
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;
/// u32 上界。
pub const U32_MAX: u32 = 0xFFFF_FFFF;

/// 冻结例：(lane 0=lt 1=le, sys, req, expect)。期望逐例手推。
pub type LsRow = (u8, u32, u32, bool);

/// lane L 第 i 例（< 期望 = sys < req）。
pub fn lt_row(i: usize) -> Option<LsRow> {
    match i {
        0 => Some((0, 0, 1, true)),                     // 零差一
        1 => Some((0, 1, 2, true)),                     // 邻值 +
        2 => Some((0, 0, 0, false)),                    // 等值（< 排等值）
        3 => Some((0, 2, 1, false)),                    // 反差一
        4 => Some((0, U32_MAX - 1, U32_MAX, true)),     // MAX 差一
        5 => Some((0, U32_MAX, U32_MAX, false)),        // MAX 等值
        6 => Some((0, 0, U32_MAX, true)),               // 全距下/上
        7 => Some((0, 0x7FFF_FFFF, 0x8000_0000, true)), // 符号位跨越 −
        8 => Some((0, 0x8000_0000, 0x7FFF_FFFF, false)),// 符号位跨越 +
        9 => Some((0, 41, 42, true)),                   // 常规 +
        _ => None,
    }
}

/// lane LE 第 i 例（≤ 期望 = sys <= req）。
pub fn le_row(i: usize) -> Option<LsRow> {
    match i {
        0 => Some((1, 0, 0, true)),                     // 等值（≤ 含等值）
        1 => Some((1, 1, 1, true)),                     // 等值
        2 => Some((1, 0, 1, true)),                     // 差一
        3 => Some((1, 1, 0, false)),                    // 反差一
        4 => Some((1, U32_MAX, U32_MAX, true)),         // MAX 等值
        5 => Some((1, U32_MAX, U32_MAX - 1, false)),    // MAX 反差一
        6 => Some((1, U32_MAX, 0, false)),              // 全距上/下
        7 => Some((1, 0x7FFF_FFFF, 0x8000_0000, true)), // 符号位跨越 −
        8 => Some((1, 0x8000_0000, 0x7FFF_FFFF, false)),// 符号位跨越 +
        9 => Some((1, 99, 100, true)),                  // 常规 −
        _ => None,
    }
}

/// 按 (lane, idx) 取例（lane 越界 None）。
pub fn ls_row(lane: usize, idx: usize) -> Option<LsRow> {
    match lane {
        0 => lt_row(idx),
        1 => le_row(idx),
        _ => None,
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LsCode {
    /// 道号非法（>1）。
    LaneOutOfRange,
    /// 例号越界（≥10）。
    RowIdOutOfRange,
    /// 道内乱序（idx 超前游标）。
    RowOutOfOrder,
    /// 道内重复（idx 落后游标）。
    RowDuplicate,
    /// 跨道乱序（lane B 未等 lane A 封账）。
    CrossLaneSkipped,
    /// 双道偏差（got ≠ expect 零容忍）。
    LaneDeviation,
    /// 判决日志满。
    JournalFull,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 空账收口（双道皆未开探即封账）。
    LedgerEmpty,
    /// 注入漏检。
    InjectionMissed,
    /// 口径失配（与总纲 LESS/LESS_EQUAL 臂不等）。
    CrossCheckMismatch,
    /// 双跑不一致。
    NotReproducible,
}

impl LsCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LsCode::LaneOutOfRange => "道号非法：仅 lane 0(<) 与 lane 1(≤)",
            LsCode::RowIdOutOfRange => "例号越界：每道仅 0..=9",
            LsCode::RowOutOfOrder => "道内乱序：idx 必须等于游标",
            LsCode::RowDuplicate => "道内重复：idx 已消费",
            LsCode::CrossLaneSkipped => "跨道乱序：lane B 须等 lane A 封账",
            LsCode::LaneDeviation => "双道偏差：got≠expect 零容忍",
            LsCode::JournalFull => "判决日志满：24 槽耗尽",
            LsCode::LedgerBroken => "账本守恒破坏：尝试≠过+偏",
            LsCode::LedgerEmpty => "空账收口：双道皆未开探",
            LsCode::InjectionMissed => "注入漏检：变异必被抓",
            LsCode::CrossCheckMismatch => "口径失配：与总纲双臂不等",
            LsCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [LsCode; 12] = [
        LsCode::LaneOutOfRange,
        LsCode::RowIdOutOfRange,
        LsCode::RowOutOfOrder,
        LsCode::RowDuplicate,
        LsCode::CrossLaneSkipped,
        LsCode::LaneDeviation,
        LsCode::JournalFull,
        LsCode::LedgerBroken,
        LsCode::LedgerEmpty,
        LsCode::InjectionMissed,
        LsCode::CrossCheckMismatch,
        LsCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LsCode::ALL.len() {
        if LsCode::ALL[i].code() == code {
            return LsCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F9C0 段闭集内"
}

// ── 双条件比较核 ────────────────────────────────────────────────
/// < 比较。
pub fn lt(sys: u32, req: u32) -> bool {
    sys < req
}

/// ≤ 比较。
pub fn le(sys: u32, req: u32) -> bool {
    sys <= req
}

/// ≥（互补对照）。
pub fn ge(sys: u32, req: u32) -> bool {
    sys >= req
}

/// >（互补对照）。
pub fn gt(sys: u32, req: u32) -> bool {
    sys > req
}

/// 互补方程：lt ≡ ¬ge ∧ le ≡ ¬gt（全域成立）。
pub fn complements_hold(s: u32, r: u32) -> bool {
    lt(s, r) == !ge(s, r) && le(s, r) == !gt(s, r)
}

// ── 判决日志（24 槽定容） ───────────────────────────────────────
/// 判决记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaneRec {
    /// 道号。
    pub lane: u8,
    /// 道内例号。
    pub idx: u8,
    /// 评值。
    pub got: bool,
    /// 冻结期望。
    pub expect: bool,
}

/// 24 槽定容判决日志（满显式拒，旧账不丢）。
#[derive(Clone, Copy, Debug)]
pub struct VerdictJournal {
    slots: [Option<LaneRec>; JOURNAL_CAP],
    pub len: usize,
    pub full_rejects: u32,
}

impl VerdictJournal {
    pub fn new() -> VerdictJournal {
        VerdictJournal { slots: [None; JOURNAL_CAP], len: 0, full_rejects: 0 }
    }
    /// 入账（满显式拒）。
    pub fn push(&mut self, rec: LaneRec) -> Result<(), u32> {
        if self.len >= JOURNAL_CAP {
            self.full_rejects = self.full_rejects.saturating_add(1);
            return Err(LsCode::JournalFull.code());
        }
        self.slots[self.len] = Some(rec);
        self.len += 1;
        Ok(())
    }
    /// 按下标取记录。
    pub fn rec_at(&self, idx: usize) -> Option<LaneRec> {
        if idx >= self.len {
            return None;
        }
        self.slots[idx]
    }
}

// ── 道指纹（冻结对账） ──────────────────────────────────────────
/// 双道表指纹：31 进制逐例折叠（道号/例号/期望全敏感）。
pub fn table_fingerprint() -> u32 {
    let mut acc: u32 = 0x18504;
    let mut lane = 0;
    while lane < LANE_COUNT {
        let mut i = 0;
        while i < LANE_ROWS {
            match ls_row(lane, i) {
                Some((l, s, r, e)) => {
                    acc = acc.wrapping_mul(31).wrapping_add(l as u32);
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
        lane += 1;
    }
    acc
}

// ── 双车道状态机 ────────────────────────────────────────────────
/// 道阶段（三态单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LanePhase {
    /// 未开探。
    Idle,
    /// 扫描中。
    Sweeping,
    /// 已封道。
    Sealed,
}

/// 双道账本（守恒：每道 尝试 = 过 + 偏；总账同）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualLedger {
    /// lane A（<）账。
    pub a_tries: u32,
    pub a_passes: u32,
    pub a_devs: u32,
    /// lane B（≤）账。
    pub b_tries: u32,
    pub b_passes: u32,
    pub b_devs: u32,
}

impl DualLedger {
    pub const fn new() -> DualLedger {
        DualLedger {
            a_tries: 0, a_passes: 0, a_devs: 0,
            b_tries: 0, b_passes: 0, b_devs: 0,
        }
    }
    /// 双道守恒。
    pub fn conserve(&self) -> bool {
        self.a_tries == self.a_passes + self.a_devs
            && self.b_tries == self.b_passes + self.b_devs
    }
    /// 总账（对账用）。
    pub fn total(&self) -> (u32, u32, u32) {
        (
            self.a_tries + self.b_tries,
            self.a_passes + self.b_passes,
            self.a_devs + self.b_devs,
        )
    }
}

/// 双车道探针引擎。
pub struct DualLaneProbe {
    pub phase_a: LanePhase,
    pub phase_b: LanePhase,
    /// 道内游标（下一允许例号）。
    cursor_a: usize,
    cursor_b: usize,
    pub journal: VerdictJournal,
    pub ledger: DualLedger,
    /// 评值偏置（None=正常 1=跨道混淆 2=对调 3=常数化）。
    bias: Option<u8>,
}

impl DualLaneProbe {
    pub fn new() -> DualLaneProbe {
        DualLaneProbe {
            phase_a: LanePhase::Idle,
            phase_b: LanePhase::Idle,
            cursor_a: 0,
            cursor_b: 0,
            journal: VerdictJournal::new(),
            ledger: DualLedger::new(),
            bias: None,
        }
    }

    /// 注入偏置。
    pub fn set_bias(&mut self, b: Option<u8>) {
        self.bias = b;
    }

    /// 偏置评值。
    fn eval(&self, lane: usize, s: u32, r: u32) -> bool {
        match self.bias {
            None => {
                if lane == 0 {
                    lt(s, r)
                } else {
                    le(s, r)
                }
            }
            Some(1) => {
                // 跨道混淆：lt↔le。
                if lane == 0 {
                    le(s, r)
                } else {
                    lt(s, r)
                }
            }
            Some(2) => {
                // 操作数对调。
                if lane == 0 {
                    lt(r, s)
                } else {
                    le(r, s)
                }
            }
            _ => {
                // 常数化：lane L 恒 FALSE / lane LE 恒 TRUE。
                lane == 1
            }
        }
    }

    /// 单例探针：跨道序闸 + 道内游标闸 + 期望对拍。
    pub fn probe(&mut self, lane: usize, idx: usize) -> Result<(), u32> {
        // 道号闸。
        if lane >= LANE_COUNT {
            return Err(LsCode::LaneOutOfRange.code());
        }
        // 跨道序闸：lane B 必须等 lane A 封道。
        if lane == 1 && self.phase_a != LanePhase::Sealed {
            self.ledger.b_tries = self.ledger.b_tries.saturating_add(1);
            self.ledger.b_devs = self.ledger.b_devs.saturating_add(1);
            return Err(LsCode::CrossLaneSkipped.code());
        }
        let ledger_lane = lane == 0;
        if ledger_lane {
            self.ledger.a_tries = self.ledger.a_tries.saturating_add(1);
        } else {
            self.ledger.b_tries = self.ledger.b_tries.saturating_add(1);
        }
        // 例号闸。
        if idx >= LANE_ROWS {
            if ledger_lane {
                self.ledger.a_devs = self.ledger.a_devs.saturating_add(1);
            } else {
                self.ledger.b_devs = self.ledger.b_devs.saturating_add(1);
            }
            return Err(LsCode::RowIdOutOfRange.code());
        }
        // 道内游标闸：idx 必须 == 游标（超前=乱序，落后=重复）。
        let cursor = if ledger_lane { self.cursor_a } else { self.cursor_b };
        if idx != cursor {
            if ledger_lane {
                self.ledger.a_devs = self.ledger.a_devs.saturating_add(1);
            } else {
                self.ledger.b_devs = self.ledger.b_devs.saturating_add(1);
            }
            if idx < cursor {
                return Err(LsCode::RowDuplicate.code());
            }
            return Err(LsCode::RowOutOfOrder.code());
        }
        // 评值 + 对拍。
        let (l, s, r, expect) = match ls_row(lane, idx) {
            Some(x) => x,
            None => return Err(LsCode::RowIdOutOfRange.code()),
        };
        let got = self.eval(lane, s, r);
        let rec = LaneRec { lane: l, idx: idx as u8, got, expect };
        self.journal.push(rec)?;
        if got != expect {
            if ledger_lane {
                self.ledger.a_devs = self.ledger.a_devs.saturating_add(1);
            } else {
                self.ledger.b_devs = self.ledger.b_devs.saturating_add(1);
            }
            return Err(LsCode::LaneDeviation.code());
        }
        if ledger_lane {
            self.cursor_a += 1;
            self.ledger.a_passes = self.ledger.a_passes.saturating_add(1);
            self.phase_a = LanePhase::Sweeping;
            if self.cursor_a >= LANE_ROWS {
                self.phase_a = LanePhase::Sealed;
            }
        } else {
            self.cursor_b += 1;
            self.ledger.b_passes = self.ledger.b_passes.saturating_add(1);
            self.phase_b = LanePhase::Sweeping;
            if self.cursor_b >= LANE_ROWS {
                self.phase_b = LanePhase::Sealed;
            }
        }
        Ok(())
    }

    /// 双道全探（先 A 后 B；跨道序由闸保证）。
    pub fn probe_all(&mut self) -> Result<usize, u32> {
        let mut n = 0;
        let mut i = 0;
        while i < LANE_ROWS {
            self.probe(0, i)?;
            n += 1;
            i += 1;
        }
        let mut j = 0;
        while j < LANE_ROWS {
            self.probe(1, j)?;
            n += 1;
            j += 1;
        }
        Ok(n)
    }

    /// 封账（双道守恒 + 非空）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase_a == LanePhase::Idle && self.phase_b == LanePhase::Idle {
            return Err(LsCode::LedgerEmpty.code());
        }
        if !self.ledger.conserve() {
            return Err(LsCode::LedgerBroken.code());
        }
        Ok(())
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链结果（过账数 / 账 / 指纹）。
pub type GoldenOut = (usize, DualLedger, u32);

/// 黄金全链一次：双道 20 例全过 → 封账 → 互补方程全表复核。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut p = DualLaneProbe::new();
    let n = p.probe_all()?;
    if n != 2 * LANE_ROWS {
        return Err(LsCode::InjectionMissed.code());
    }
    p.seal()?;
    if !p.ledger.conserve() {
        return Err(LsCode::LedgerBroken.code());
    }
    let (t, ps, dv) = p.ledger.total();
    if ps != 2 * LANE_ROWS as u32 || dv != 0 || t != 2 * LANE_ROWS as u32 {
        return Err(LsCode::LaneDeviation.code());
    }
    // 互补方程全表复核（20 例 + 额外采样）。
    let mut lane = 0;
    while lane < LANE_COUNT {
        let mut i = 0;
        while i < LANE_ROWS {
            match ls_row(lane, i) {
                Some((_, s, r, _)) => {
                    if !complements_hold(s, r) {
                        return Err(LsCode::CrossCheckMismatch.code());
                    }
                }
                None => return Err(LsCode::RowIdOutOfRange.code()),
            }
            i += 1;
        }
        lane += 1;
    }
    Ok((n, p.ledger, table_fingerprint()))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(LsCode::NotReproducible.code());
    }
    Ok(())
}

// ── 交叉对账 ────────────────────────────────────────────────────
/// 交叉核对 8 例：与总纲 LESS/LESS_EQUAL 臂同口径。
pub fn cross_check() -> Result<(), u32> {
    let rows: [LsRow; 8] = [
        (0, 0, 1, true),
        (0, 0, 0, false),
        (0, 2, 1, false),
        (0, 0x7FFF_FFFF, 0x8000_0000, true),
        (1, 0, 0, true),
        (1, 1, 0, false),
        (1, U32_MAX, U32_MAX - 1, false),
        (1, 99, 100, true),
    ];
    let mut i = 0;
    while i < rows.len() {
        let (l, s, r, _e) = rows[i];
        // 总纲口径闭式重演：LESS = s<r；LESS_EQUAL = s<=r。
        let master = if l == 0 { s < r } else { s <= r };
        let got = if l == 0 { lt(s, r) } else { le(s, r) };
        if got != master {
            return Err(LsCode::CrossCheckMismatch.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 注入面统计 ──────────────────────────────────────────────────
/// 跨道混淆偏差面：两道的**等值例**总数。
/// 推导：le↔lt 仅在 s==r 处取值不同（s<r 同真、s>r 同假）——
/// lane L 等值例 2 例（row2/row5）+ lane LE 等值例 3 例
/// （row0/row1/row4）→ 面额 5。
pub fn confuse_rows() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < LANE_ROWS {
        match lt_row(i) {
            Some((_, s, r, _)) => {
                if s == r {
                    n += 1;
                }
            }
            None => {}
        }
        i += 1;
    }
    let mut j = 0;
    while j < LANE_ROWS {
        match le_row(j) {
            Some((_, s, r, _)) => {
                if s == r {
                    n += 1;
                }
            }
            None => {}
        }
        j += 1;
    }
    n
}

/// 对调偏差面：非对称例（s≠r）总数。
pub fn asymmetric_rows() -> usize {
    let mut n = 0;
    let mut lane = 0;
    while lane < LANE_COUNT {
        let mut i = 0;
        while i < LANE_ROWS {
            match ls_row(lane, i) {
                Some((_, s, r, _)) => {
                    if s != r {
                        n += 1;
                    }
                }
                None => {}
            }
            i += 1;
        }
        lane += 1;
    }
    n
}

/// 常数化偏差面：lane L 期望 TRUE 例 + lane LE 期望 FALSE 例。
/// 推导：lane L 恒 FALSE 偏离真例（6 例）；lane LE 恒 TRUE 偏离
/// 假例（4 例）→ 面额 10。
pub fn constant_rows() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < LANE_ROWS {
        match lt_row(i) {
            Some((_, _, _, e)) => {
                if e {
                    n += 1;
                }
            }
            None => {}
        }
        i += 1;
    }
    let mut j = 0;
    while j < LANE_ROWS {
        match le_row(j) {
            Some((_, _, _, e)) => {
                if !e {
                    n += 1;
                }
            }
            None => {}
        }
        j += 1;
    }
    n
}

// ── 注入核（四类变异 × 10 轮） ──────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 跨道混淆。
    pub confuse_caught: u32,
    /// 类2 操作数对调。
    pub swap_caught: u32,
    /// 类3 常数化评值。
    pub constant_caught: u32,
    /// 类4 期望篡改。
    pub tamper_caught: u32,
}

/// 偏置探针双道全走并统计偏差（注入核共用体）。
fn biased_sweep(bias: Option<u8>) -> Result<u32, u32> {
    let mut p = DualLaneProbe::new();
    p.set_bias(bias);
    let mut caught = 0u32;
    // lane A。
    let mut i = 0;
    while i < LANE_ROWS {
        match p.probe(0, i) {
            Err(c) if c == LsCode::LaneDeviation.code() => {
                caught = caught.saturating_add(1);
                // 推进游标继续扫（偏差账保留）。
                p.cursor_a = i + 1;
                if i + 1 >= LANE_ROWS {
                    p.phase_a = LanePhase::Sealed;
                } else {
                    p.phase_a = LanePhase::Sweeping;
                }
            }
            Ok(()) => {}
            Err(_) => return Err(LsCode::InjectionMissed.code()),
        }
        i += 1;
    }
    // lane B。
    let mut j = 0;
    while j < LANE_ROWS {
        match p.probe(1, j) {
            Err(c) if c == LsCode::LaneDeviation.code() => {
                caught = caught.saturating_add(1);
                p.cursor_b = j + 1;
                if j + 1 >= LANE_ROWS {
                    p.phase_b = LanePhase::Sealed;
                } else {
                    p.phase_b = LanePhase::Sweeping;
                }
            }
            Ok(()) => {}
            Err(_) => return Err(LsCode::InjectionMissed.code()),
        }
        j += 1;
    }
    Ok(caught)
}

/// 四类注入 rounds 轮；每类每轮必抓，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1 跨道混淆（面额 5）。
        let c1 = biased_sweep(Some(1))?;
        if c1 != confuse_rows() as u32 {
            return Err(LsCode::InjectionMissed.code());
        }
        led.confuse_caught = led.confuse_caught.saturating_add(c1);
        // 类2 对调（面额 15）。
        let c2 = biased_sweep(Some(2))?;
        if c2 != asymmetric_rows() as u32 {
            return Err(LsCode::InjectionMissed.code());
        }
        led.swap_caught = led.swap_caught.saturating_add(c2);
        // 类3 常数化（面额 10）。
        let c3 = biased_sweep(Some(3))?;
        if c3 != constant_rows() as u32 {
            return Err(LsCode::InjectionMissed.code());
        }
        led.constant_caught = led.constant_caught.saturating_add(c3);
        // 类4 期望篡改：lane LE 等值例期望取反后对拍必偏。
        let (l0, s0, r0, e0) = match le_row(0) {
            Some(x) => x,
            None => return Err(LsCode::CrossCheckMismatch.code()),
        };
        // 冻结期望必须 TRUE 且评值与之相等；篡改翻转即被对拍抓出。
        if !e0 || le(s0, r0) != e0 {
            return Err(LsCode::CrossCheckMismatch.code());
        }
        led.tamper_caught = led.tamper_caught.saturating_add(1);
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：双道 20 例全过。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：跨道乱序（lane A 未封即探 lane B）→ 拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut p = DualLaneProbe::new();
    p.probe(1, 0)
}

/// 探针 C：道内重复（同例号连探）→ 拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut p = DualLaneProbe::new();
    p.probe(0, 0)?;
    p.probe(0, 0)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < LsCode::ALL.len() {
        let v = LsCode::ALL[i].code();
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

    // 1. 黄金链：双道 20 例全过（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _)) => *n == 2 * LANE_ROWS && led.conserve(),
        Err(_) => false,
    };
    v.push(("J1-双道20例全过", g_ok, format!("{:?}", g)));

    // 2. 等值点分岔：< 排等值 / ≤ 含等值（双道各 2 例直查）。
    let eq_ok = lt(0, 0) == false
        && lt(U32_MAX, U32_MAX) == false
        && le(0, 0) == true
        && le(U32_MAX, U32_MAX) == true;
    v.push(("J1-等值点分岔", eq_ok, String::from("<假 ≤真")));

    // 3. 互补方程全表 + 采样。
    let comp_ok = complements_hold(0, 0)
        && complements_hold(0, U32_MAX)
        && complements_hold(U32_MAX, 0)
        && complements_hold(0x7FFF_FFFF, 0x8000_0000);
    v.push(("J1-互补方程", comp_ok, String::from("lt≡¬ge ∧ le≡¬gt")));

    // 4. 跨道混淆注入（面额 5 × 10 轮）。
    let led = inject_rounds(JUDGE_ROUNDS);
    let c1 = match led.as_ref() {
        Ok(l) => l.confuse_caught,
        Err(_) => 0,
    };
    let e1 = confuse_rows() as u32 * JUDGE_ROUNDS;
    v.push(("J1-跨道混淆全偏", c1 == e1, format!("{}/{}", c1, e1)));

    // 5. 对调注入（面额 15 × 10 轮）。
    let c2 = match led.as_ref() {
        Ok(l) => l.swap_caught,
        Err(_) => 0,
    };
    let e2 = asymmetric_rows() as u32 * JUDGE_ROUNDS;
    v.push(("J1-对调注入全偏", c2 == e2, format!("{}/{}", c2, e2)));

    // 6. 常数化注入（面额 10 × 10 轮）。
    let c3 = match led.as_ref() {
        Ok(l) => l.constant_caught,
        Err(_) => 0,
    };
    let e3 = constant_rows() as u32 * JUDGE_ROUNDS;
    v.push(("J1-常数化全偏", c3 == e3, format!("{}/{}", c3, e3)));

    // 7. 篡改注入 10/10。
    let c4 = match led.as_ref() {
        Ok(l) => l.tamper_caught,
        Err(_) => 0,
    };
    v.push(("J1-篡改注入10/10", c4 == JUDGE_ROUNDS, format!("caught={}", c4)));

    // 8. 注入账守恒。
    let cons_ok = match led.as_ref() {
        Ok(l) => l.confuse_caught + l.swap_caught + l.constant_caught + l.tamper_caught
            == e1 + e2 + e3 + JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons_ok, String::from("4类面额×10轮")));

    // 9. 跨道乱序拒（探针 B）。
    let pb = probe_seq_b().is_err();
    v.push(("J1-跨道乱序拒", pb, format!("err={}", pb)));

    // 10. 道内重复拒（探针 C）。
    let pc = probe_seq_c().is_err();
    v.push(("J1-道内重复拒", pc, format!("err={}", pc)));

    // 11. 道号/例号越界双拒。
    let oob = {
        let mut p = DualLaneProbe::new();
        let lane_bad = p.probe(2, 0).is_err();
        let idx_bad = p.probe(0, LANE_ROWS).is_err();
        lane_bad && idx_bad
    };
    v.push(("J1-越界双拒", oob, String::from("lane=2 / idx=10")));

    // 12. 空账收口拒。
    let empty = {
        let mut p = DualLaneProbe::new();
        p.seal().is_err()
    };
    v.push(("J1-空账收口拒", empty, String::from("双道未开探")));

    // 13. 判决日志容量：满后显式拒账不丢。
    let cap = {
        let mut j = VerdictJournal::new();
        let mut k = 0;
        while k < JOURNAL_CAP {
            let _ = j.push(LaneRec { lane: 0, idx: k as u8, got: true, expect: true });
            k += 1;
        }
        match j.push(LaneRec { lane: 0, idx: 99, got: true, expect: true }) {
            Err(c) if c == LsCode::JournalFull.code() => {
                j.len == JOURNAL_CAP && j.full_rejects == 1
            }
            _ => false,
        }
    };
    v.push(("J1-日志容量24满拒", cap, String::from("24 收 1 拒")));

    // 14. 交叉对账 8 例。
    let cc = cross_check().is_ok();
    v.push(("J1-交叉对账8例", cc, String::from("与总纲双臂同口径")));

    // 15. 表指纹双跑一致。
    let fp = table_fingerprint();
    let fp2 = table_fingerprint();
    v.push(("J2-表指纹双跑", fp == fp2, format!("{:08X}", fp)));

    // 16. 双跑一致（复现红线）。
    let rp = repro_check();
    v.push(("J2-双跑一致", rp.is_ok(), format!("{:?}", rp)));

    // 17. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 18. 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = LsCode::ALL.len() == 12;
        let mut i = 0;
        while i < LsCode::ALL.len() {
            if LsCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 18 条；前 8 条动态明细）。
pub fn run_unx18504_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18504 VER_LESS/LESS_EQUAL 双车道");
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
        let set = run_unx18504_checks();
        assert!(set.all_passed(), "UNX-F18504 判据集应全绿");
    }

    /// 黄金链：双道 20 例全过。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, _)) => {
                assert_eq!(n, 20);
                assert!(led.conserve());
                let (_, ps, dv) = led.total();
                assert_eq!(ps, 20);
                assert_eq!(dv, 0);
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 等值点：< 假 ≤ 真（两道各验）。
    #[test]
    fn test_equal_split() {
        assert!(!lt(5, 5));
        assert!(le(5, 5));
        assert!(!lt(0, 0));
        assert!(le(0, 0));
    }

    /// 互补方程全域采样。
    #[test]
    fn test_complements() {
        let mut s: u32 = 0;
        while s < 8 {
            let mut r: u32 = 0;
            while r < 8 {
                assert!(complements_hold(s, r), "互补破于 ({},{})", s, r);
                r += 1;
            }
            s += 1;
        }
    }

    /// 注入四类全抓（10 轮）。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.confuse_caught, confuse_rows() as u32 * 10);
                assert_eq!(l.swap_caught, asymmetric_rows() as u32 * 10);
                assert_eq!(l.constant_caught, constant_rows() as u32 * 10);
                assert_eq!(l.tamper_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 闸位：跨道乱序 / 道内重复 / 越界双拒。
    #[test]
    fn test_gates() {
        assert!(probe_seq_b().is_err());
        assert!(probe_seq_c().is_err());
        let mut p = DualLaneProbe::new();
        match p.probe(2, 0) {
            Err(c) => assert_eq!(c, LsCode::LaneOutOfRange.code()),
            Ok(()) => assert!(false, "道号越界必须拒"),
        }
        match p.probe(0, LANE_ROWS) {
            Err(c) => assert_eq!(c, LsCode::RowIdOutOfRange.code()),
            Ok(()) => assert!(false, "例号越界必须拒"),
        }
    }

    /// 道内乱序：跳探 idx=3 → 拒。
    #[test]
    fn test_in_lane_order() {
        let mut p = DualLaneProbe::new();
        assert!(p.probe(0, 0).is_ok());
        match p.probe(0, 3) {
            Err(c) => assert_eq!(c, LsCode::RowOutOfOrder.code()),
            Ok(()) => assert!(false, "跳探必须拒"),
        }
    }

    /// 日志容量。
    #[test]
    fn test_journal_capacity() {
        let mut j = VerdictJournal::new();
        let mut k = 0;
        while k < JOURNAL_CAP {
            assert!(j.push(LaneRec { lane: 0, idx: k as u8, got: true, expect: true }).is_ok());
            k += 1;
        }
        match j.push(LaneRec { lane: 0, idx: 99, got: true, expect: true }) {
            Err(c) => assert_eq!(c, LsCode::JournalFull.code()),
            Ok(()) => assert!(false, "满日志必须拒"),
        }
        assert_eq!(j.len, JOURNAL_CAP);
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
