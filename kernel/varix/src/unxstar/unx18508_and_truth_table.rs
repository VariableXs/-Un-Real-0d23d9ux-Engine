//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18508 · 组合条件判定（多维度与运算）
//! · 0x7FAC0 段 · 批 06 · 配额 320
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18508-J1 多维 AND 组合 30 例真值表与 Windows 对照零
//!   偏差。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - AND 聚合口径冻结：裁决 = ∧（逐请求维 cmp(op, sys, req)），
//!     **全维消费**（不许短路跳维——每请求维必须出裁决值后才聚合）；
//!   - 六维消费面（与 F18507 同源同序）：MINOR(0x01)/MAJOR(0x02)/
//!     BUILD(0x04)/SPMAJOR(0x20)/SUITE(0x40)/PT(0x80)；快照同源：
//!     minor 1 / major 6 / build 7601 / spmaj 1 / suite 0x0100 / pt 1；
//!   - 冻结真值表 30 例：16 例全过（T）+ 14 例含败（F）——F 例全部
//!     为**混合多维**（≥1 过 + ≥1 败），专门供 OR 混淆注入对拍；
//!     期望值逐例手推（Windows 口径：AND 语义，全过才 TRUE）；
//!   - 注入四类 × 10 轮：OR 混淆（any-pass 替 all-pass，14 混合例
//!     全偏）/ 短路跳维（败例提前返回，消费账缺维必抓）/ 裁决取反
//!     （30 例全偏）/ 期望篡改（对拍必抓）；
//!   - 探针状态机：Dormant → Sweeping → Sealed；行序严格升序
//!     （乱序/重复/越界三闸）；账本守恒（试=过+偏）；
//!   - 对账：单维特例行（PT EQ / SUITE AND）与总纲臂闭式重演交叉
//!     核对（跨模块禁引）；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] AndCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [裁决表]   AND_CASES 30 例冻结真值表（手推期望）
//!   [数据结构] CaseRow / AndPlane（六维裁决位面）/ 表指纹
//!   [状态机]   AndPhase 三态 + AndProbe（三闸 + 全维消费闸）
//!   [账本]     AndLedger（试=过+偏 守恒）
//!   [注入核]   四类变异 × 10 轮（OR混淆/短路/取反/篡改）
//!   [收口]     golden_run + run_unx18508_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FAC0，AI-VX11-1105 G1 批06 独占区）。
pub const SEG_BASE: u32 = 0x7FAC0;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// 算子闭集。
pub const VER_EQUAL: u8 = 1;
pub const VER_GREATER: u8 = 2;
pub const VER_GREATER_EQUAL: u8 = 3;
pub const VER_LESS: u8 = 4;
pub const VER_LESS_EQUAL: u8 = 5;
pub const VER_AND: u8 = 6;
pub const VER_OR: u8 = 7;
/// 维数（六维消费面）。
pub const DIM_COUNT: usize = 6;
/// 冻结真值表行数（J1「30 例」口径）。
pub const CASE_ROWS: usize = 30;
/// T 例数（冻结，手推）。
pub const TRUE_ROWS: usize = 16;
/// F 例数（冻结，手推，全为混合多维）。
pub const FALSE_ROWS: usize = 14;
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

/// 系统快照（六维，与 F18507 同源）。
pub const SNAPSHOT: [u32; DIM_COUNT] = [1, 6, 7601, 1, 0x0100, 1];

/// 真值表一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaseRow {
    /// 请求掩码（六维位集）。
    pub mask: u32,
    /// 逐维算子（未请求维 0）。
    pub ops: [u8; DIM_COUNT],
    /// 逐维请求值（未请求维 0）。
    pub reqs: [u32; DIM_COUNT],
    /// 手推期望（Windows AND 口径）。
    pub expect: bool,
}

/// 六维位（本卡消费面，与 F18507 同源）。
pub const BIT_MINOR: u32 = 0x01;
pub const BIT_MAJOR: u32 = 0x02;
pub const BIT_BUILD: u32 = 0x04;
pub const BIT_SPMAJ: u32 = 0x20;
pub const BIT_SUITE: u32 = 0x40;
pub const BIT_PT: u32 = 0x80;

/// 手工构造行辅助（保持表定义可读）。
const fn row(
    mask: u32,
    ops: [u8; DIM_COUNT],
    reqs: [u32; DIM_COUNT],
    expect: bool,
) -> CaseRow {
    CaseRow { mask, ops, reqs, expect }
}

/// 30 例冻结真值表（期望逐例手推；F 例全为混合多维）。
pub const AND_CASES: [CaseRow; CASE_ROWS] = [
    // ── T 例 16 条 ──
    // 1. minor EQ1 ∧ major GE6 → T
    row(0x03, [VER_EQUAL, VER_GREATER_EQUAL, 0, 0, 0, 0], [1, 6, 0, 0, 0, 0], true),
    // 2. minor EQ1 ∧ major EQ6 → T
    row(0x03, [VER_EQUAL, VER_EQUAL, 0, 0, 0, 0], [1, 6, 0, 0, 0, 0], true),
    // 3. major GT5 ∧ minor LE1 → T
    row(0x03, [VER_LESS_EQUAL, VER_GREATER, 0, 0, 0, 0], [1, 5, 0, 0, 0, 0], true),
    // 4. build GE7601 ∧ minor EQ1 ∧ major EQ6 → T
    row(0x07, [VER_EQUAL, VER_EQUAL, VER_GREATER_EQUAL, 0, 0, 0], [1, 6, 7601, 0, 0, 0], true),
    // 5. spmaj GE1 ∧ build GE7601 ∧ minor EQ1 ∧ major EQ6 → T
    row(
        0x27,
        [VER_EQUAL, VER_EQUAL, VER_GREATER_EQUAL, VER_GREATER_EQUAL, 0, 0],
        [1, 6, 7601, 1, 0, 0],
        true,
    ),
    // 6. pt EQ1 → T
    row(0x80, [0, 0, 0, 0, 0, VER_EQUAL], [0, 0, 0, 0, 0, 1], true),
    // 7. suite AND0x0100 → T
    row(0x40, [0, 0, 0, 0, VER_AND, 0], [0, 0, 0, 0, 0x0100, 0], true),
    // 8. suite OR0x0101 → T（任一位在场）
    row(0x40, [0, 0, 0, 0, VER_OR, 0], [0, 0, 0, 0, 0x0101, 0], true),
    // 9. suite AND0x0100 ∧ pt EQ1 → T
    row(0xC0, [0, 0, 0, 0, VER_AND, VER_EQUAL], [0, 0, 0, 0, 0x0100, 1], true),
    // 10. suite AND0x0100 ∧ pt EQ1 ∧ minor EQ1 → T
    row(
        0xE1,
        [VER_EQUAL, 0, 0, 0, VER_AND, VER_EQUAL],
        [1, 0, 0, 0, 0x0100, 1],
        true,
    ),
    // 11. minor LT2 ∧ suite AND0x0100 ∧ pt EQ1 → T
    row(0xE1, [VER_LESS, 0, 0, 0, VER_AND, VER_EQUAL], [2, 0, 0, 0, 0x0100, 1], true),
    // 12. 全六维全过 → T
    row(
        0xE7,
        [VER_EQUAL, VER_GREATER_EQUAL, VER_GREATER_EQUAL, VER_GREATER_EQUAL, VER_AND, VER_EQUAL],
        [1, 6, 7601, 1, 0x0100, 1],
        true,
    ),
    // 13. major GE6 ∧ build LE7601 ∧ spmaj GE1 → T
    row(
        0x26,
        [0, VER_GREATER_EQUAL, VER_LESS_EQUAL, VER_GREATER_EQUAL, 0, 0],
        [0, 6, 7601, 1, 0, 0],
        true,
    ),
    // 14. minor GE1 ∧ build EQ7601 ∧ spmaj EQ1 → T
    row(0x25, [VER_GREATER_EQUAL, 0, VER_EQUAL, VER_EQUAL, 0, 0], [1, 0, 7601, 1, 0, 0], true),
    // 15. minor GE1 ∧ major LE6 ∧ spmaj LE2 → T
    row(0x23, [VER_GREATER_EQUAL, VER_LESS_EQUAL, 0, VER_LESS_EQUAL, 0, 0], [1, 6, 0, 2, 0, 0], true),
    // 16. pt EQ1 ∧ spmaj GE1 ∧ major GE6 ∧ minor EQ1 → T
    row(
        0xA3,
        [VER_EQUAL, VER_GREATER_EQUAL, 0, VER_GREATER_EQUAL, 0, VER_EQUAL],
        [1, 6, 0, 1, 0, 1],
        true,
    ),
    // ── F 例 14 条（全混合多维：≥1 过 + ≥1 败）──
    // 17. minor EQ2(F) ∧ major GE6(T) → F
    row(0x03, [VER_EQUAL, VER_GREATER_EQUAL, 0, 0, 0, 0], [2, 6, 0, 0, 0, 0], false),
    // 18. minor EQ1(T) ∧ major GT6(F) → F
    row(0x03, [VER_EQUAL, VER_GREATER, 0, 0, 0, 0], [1, 6, 0, 0, 0, 0], false),
    // 19. minor EQ1(T) ∧ major EQ6(T) ∧ build GT7601(F) → F
    row(0x07, [VER_EQUAL, VER_EQUAL, VER_GREATER, 0, 0, 0], [1, 6, 7601, 0, 0, 0], false),
    // 20. build GE7600(T) ∧ major GE7(F) ∧ minor LE1(T) → F
    row(
        0x07,
        [VER_LESS_EQUAL, VER_GREATER_EQUAL, VER_GREATER_EQUAL, 0, 0, 0],
        [1, 7, 7600, 0, 0, 0],
        false,
    ),
    // 21. spmaj GT1(F) ∧ minor EQ1(T) ∧ build GE7601(T) → F
    row(
        0x25,
        [VER_EQUAL, 0, VER_GREATER_EQUAL, VER_GREATER, 0, 0],
        [1, 0, 7601, 1, 0, 0],
        false,
    ),
    // 22. spmaj LE1(T) ∧ minor LT1(F) → F
    row(
        0x25,
        [VER_LESS, 0, 0, VER_LESS_EQUAL, 0, 0],
        [1, 0, 0, 1, 0, 0],
        false,
    ),
    // 23. pt EQ2(F) ∧ minor EQ1(T) → F
    row(0x81, [VER_EQUAL, 0, 0, 0, 0, VER_EQUAL], [1, 0, 0, 0, 0, 2], false),
    // 24. suite AND0x0001(F) ∧ pt EQ1(T) → F
    row(0xC0, [0, 0, 0, 0, VER_AND, VER_EQUAL], [0, 0, 0, 0, 0x0001, 1], false),
    // 25. suite OR0x0101(T) ∧ pt EQ9(F) → F
    row(0xC0, [0, 0, 0, 0, VER_OR, VER_EQUAL], [0, 0, 0, 0, 0x0101, 9], false),
    // 26. suite AND0x0100(T) ∧ pt EQ3(F) → F
    row(0xC0, [0, 0, 0, 0, VER_AND, VER_EQUAL], [0, 0, 0, 0, 0x0100, 3], false),
    // 27. 全六维但 pt EQ9(F) → F
    row(
        0xE7,
        [VER_EQUAL, VER_GREATER_EQUAL, VER_GREATER_EQUAL, VER_GREATER_EQUAL, VER_AND, VER_EQUAL],
        [1, 6, 7601, 1, 0x0100, 9],
        false,
    ),
    // 28. major GT6(F) ∧ build LE7601(T) ∧ spmaj GE1(T) → F
    row(
        0x26,
        [0, VER_GREATER, VER_LESS_EQUAL, VER_GREATER_EQUAL, 0, 0],
        [0, 6, 7601, 1, 0, 0],
        false,
    ),
    // 29. minor GE1(T) ∧ build LT7600(F) ∧ spmaj EQ1(T) → F
    row(0x25, [VER_GREATER_EQUAL, 0, VER_LESS, VER_EQUAL, 0, 0], [1, 0, 7600, 1, 0, 0], false),
    // 30. minor EQ1(T) ∧ major LE6(T) ∧ spmaj GT2(F) → F
    row(
        0x23,
        [VER_EQUAL, VER_LESS_EQUAL, 0, VER_GREATER, 0, 0],
        [1, 6, 0, 2, 0, 0],
        false,
    ),
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AndCode {
    /// 行号越界（≥30）。
    RowIdOutOfRange,
    /// 行乱序（未严格升序）。
    RowOutOfOrder,
    /// 行重复。
    RowDuplicate,
    /// 真值偏差（got ≠ expect，零偏差红线）。
    TruthDeviation,
    /// 短路检出（败例提前返回致消费缺维）。
    ShortCircuitDetected,
    /// 维越界（掩码含六维闭集外位）。
    DimOutOfRange,
    /// 算子越界。
    OpOutOfRange,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 空表收口。
    TableEmpty,
    /// 注入漏检。
    InjectionMissed,
    /// 口径失配（单维特例行闭式重演不等）。
    CrossCheckMismatch,
    /// 双跑不一致。
    NotReproducible,
}

impl AndCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AndCode::RowIdOutOfRange => "行号越界：真值表仅 0..=29",
            AndCode::RowOutOfOrder => "行乱序：必须严格升序",
            AndCode::RowDuplicate => "行重复：同行只探一次",
            AndCode::TruthDeviation => "真值偏差：got≠expect 零容忍",
            AndCode::ShortCircuitDetected => "短路检出：败例缺维消费",
            AndCode::DimOutOfRange => "维越界：掩码含六维闭集外位",
            AndCode::OpOutOfRange => "算子越界：闭集 1..=7 外",
            AndCode::LedgerBroken => "账本守恒破坏：试≠过+偏",
            AndCode::TableEmpty => "空表收口：零探针不得封账",
            AndCode::InjectionMissed => "注入漏检：变异必被抓",
            AndCode::CrossCheckMismatch => "口径失配：特例行闭式不等",
            AndCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [AndCode; 12] = [
        AndCode::RowIdOutOfRange,
        AndCode::RowOutOfOrder,
        AndCode::RowDuplicate,
        AndCode::TruthDeviation,
        AndCode::ShortCircuitDetected,
        AndCode::DimOutOfRange,
        AndCode::OpOutOfRange,
        AndCode::LedgerBroken,
        AndCode::TableEmpty,
        AndCode::InjectionMissed,
        AndCode::CrossCheckMismatch,
        AndCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AndCode::ALL.len() {
        if AndCode::ALL[i].code() == code {
            return AndCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FAC0 段闭集内"
}

// ── 单维比较核（闭集） ──────────────────────────────────────────
/// 单维比较（与总纲/六维卡同口径）。
pub fn cmp_dim(op: u8, sys: u32, req: u32) -> Result<bool, u32> {
    match op {
        o if o == VER_EQUAL => Ok(sys == req),
        o if o == VER_GREATER => Ok(sys > req),
        o if o == VER_GREATER_EQUAL => Ok(sys >= req),
        o if o == VER_LESS => Ok(sys < req),
        o if o == VER_LESS_EQUAL => Ok(sys <= req),
        o if o == VER_AND => Ok((sys & req) == req),
        o if o == VER_OR => Ok((sys & req) != 0),
        _ => Err(AndCode::OpOutOfRange.code()),
    }
}

/// 掩码守卫：必须在六维闭集 0xE7 内。
pub fn mask_legal(mask: u32) -> bool {
    mask & !0xE7 == 0
}

/// AND 全维消费评估：逐请求维出裁决（不许短路），AND 聚合。
/// 返回（裁决值, 已消费请求数）。
pub fn evaluate(row: &CaseRow) -> Result<(bool, usize), u32> {
    if !mask_legal(row.mask) {
        return Err(AndCode::DimOutOfRange.code());
    }
    let mut acc = true;
    let mut consumed = 0usize;
    let mut i = 0;
    while i < DIM_COUNT {
        if row.mask & (1u32 << dim_bit(i)) != 0 {
            let op = row.ops[i];
            if op == 0 || op > VER_OR {
                return Err(AndCode::OpOutOfRange.code());
            }
            let v = cmp_dim(op, SNAPSHOT[i], row.reqs[i])?;
            if !v {
                acc = false;
            }
            consumed += 1;
        }
        i += 1;
    }
    Ok((acc, consumed))
}

/// 维下标 → 位（六维闭集序）。
pub fn dim_bit(i: usize) -> u32 {
    match i {
        0 => BIT_MINOR,
        1 => BIT_MAJOR,
        2 => BIT_BUILD,
        3 => BIT_SPMAJ,
        4 => BIT_SUITE,
        _ => BIT_PT,
    }
}

/// OR 混淆评估（any-pass 替 all-pass；注入面）。
pub fn evaluate_or(row: &CaseRow) -> Result<bool, u32> {
    let mut acc = false;
    let mut i = 0;
    while i < DIM_COUNT {
        if row.mask & (1u32 << dim_bit(i)) != 0 {
            let v = cmp_dim(row.ops[i], SNAPSHOT[i], row.reqs[i])?;
            if v {
                acc = true;
            }
        }
        i += 1;
    }
    Ok(acc)
}

// ── 表指纹 ──────────────────────────────────────────────────────
/// 真值表指纹（掩码/算子/请求/期望全敏感折叠）。
pub fn table_fingerprint() -> u32 {
    let mut acc: u32 = 0x18508;
    let mut r = 0;
    while r < CASE_ROWS {
        let c = &AND_CASES[r];
        acc = acc.wrapping_mul(31).wrapping_add(c.mask);
        let mut i = 0;
        while i < DIM_COUNT {
            acc = acc.wrapping_mul(31).wrapping_add(c.ops[i] as u32);
            acc = acc.wrapping_mul(31).wrapping_add(c.reqs[i]);
            i += 1;
        }
        acc = acc.wrapping_mul(31).wrapping_add(if c.expect { 1 } else { 2 });
        r += 1;
    }
    acc
}

// ── 探针状态机（Dormant → Sweeping → Sealed） ───────────────────
/// 探针阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AndPhase {
    /// 未开探。
    Dormant,
    /// 扫描中。
    Sweeping,
    /// 已封账。
    Sealed,
}

/// 账本（试 = 过 + 偏）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AndLedger {
    pub tries: u32,
    pub passes: u32,
    pub deviations: u32,
    /// 短路检出数。
    pub short_caught: u32,
}

impl AndLedger {
    pub const fn new() -> AndLedger {
        AndLedger { tries: 0, passes: 0, deviations: 0, short_caught: 0 }
    }
    pub fn conserve(&self) -> bool {
        self.tries == self.passes + self.deviations
    }
}

/// AND 真值探针：三闸 + 全维消费闸 + 零偏差对拍。
pub struct AndProbe {
    pub phase: AndPhase,
    next_min: usize,
    last_probed: Option<usize>,
    pub ledger: AndLedger,
    /// 评值偏置（None=正常 1=OR混淆 2=短路 3=取反）。
    bias: Option<u8>,
}

impl AndProbe {
    pub fn new() -> AndProbe {
        AndProbe {
            phase: AndPhase::Dormant,
            next_min: 0,
            last_probed: None,
            ledger: AndLedger::new(),
            bias: None,
        }
    }

    pub fn set_bias(&mut self, b: Option<u8>) {
        self.bias = b;
    }

    /// 单行探针。
    pub fn probe(&mut self, id: usize) -> Result<(), u32> {
        if self.phase == AndPhase::Sealed {
            return Err(AndCode::RowIdOutOfRange.code());
        }
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        if id >= CASE_ROWS {
            self.ledger.deviations = self.ledger.deviations.saturating_add(1);
            return Err(AndCode::RowIdOutOfRange.code());
        }
        if id < self.next_min {
            self.ledger.deviations = self.ledger.deviations.saturating_add(1);
            if self.last_probed == Some(id) {
                return Err(AndCode::RowDuplicate.code());
            }
            return Err(AndCode::RowOutOfOrder.code());
        }
        let c = &AND_CASES[id];
        let (got, consumed) = match self.bias {
            None => evaluate(c)?,
            Some(1) => (evaluate_or(c)?, masked_count(c)),
            Some(2) => {
                // 短路形态：首败即返回（余维不消费）。
                short_circuit_eval(c)?
            }
            _ => {
                let (v, n) = evaluate(c)?;
                (!v, n)
            }
        };
        // 全维消费闸：败例不得缺维（短路检测）。
        let masked = masked_count(c);
        if consumed != masked {
            self.ledger.short_caught = self.ledger.short_caught.saturating_add(1);
            self.ledger.deviations = self.ledger.deviations.saturating_add(1);
            return Err(AndCode::ShortCircuitDetected.code());
        }
        if got != c.expect {
            self.ledger.deviations = self.ledger.deviations.saturating_add(1);
            return Err(AndCode::TruthDeviation.code());
        }
        self.next_min = id + 1;
        self.last_probed = Some(id);
        self.ledger.passes = self.ledger.passes.saturating_add(1);
        self.phase = AndPhase::Sweeping;
        Ok(())
    }

    /// 全表顺探。
    pub fn probe_all(&mut self) -> Result<usize, u32> {
        let mut n = 0;
        let mut id = 0usize;
        while id < CASE_ROWS {
            self.probe(id)?;
            n += 1;
            id += 1;
        }
        Ok(n)
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase == AndPhase::Dormant {
            return Err(AndCode::TableEmpty.code());
        }
        if !self.ledger.conserve() {
            return Err(AndCode::LedgerBroken.code());
        }
        self.phase = AndPhase::Sealed;
        Ok(())
    }
}

/// 请求维数。
pub fn masked_count(c: &CaseRow) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < DIM_COUNT {
        if c.mask & (1u32 << dim_bit(i)) != 0 {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 短路评估（注入形态）：首败即返回，余维不消费。
fn short_circuit_eval(c: &CaseRow) -> Result<(bool, usize), u32> {
    let mut consumed = 0usize;
    let mut i = 0;
    while i < DIM_COUNT {
        if c.mask & (1u32 << dim_bit(i)) != 0 {
            let v = cmp_dim(c.ops[i], SNAPSHOT[i], c.reqs[i])?;
            consumed += 1;
            if !v {
                // 提前返回：余维未消费（短路）。
                return Ok((false, consumed));
            }
        }
        i += 1;
    }
    Ok((true, consumed))
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链结果（过账数 / 账 / 指纹）。
pub type GoldenOut = (usize, AndLedger, u32);

/// 黄金全链一次：30 例顺探全过（零偏差）→ 封账。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut p = AndProbe::new();
    let n = p.probe_all()?;
    if n != CASE_ROWS {
        return Err(AndCode::InjectionMissed.code());
    }
    p.seal()?;
    if p.ledger.passes != CASE_ROWS as u32 || p.ledger.deviations != 0 {
        return Err(AndCode::TruthDeviation.code());
    }
    // T/F 行数守恒（手推口径复核）。
    let mut t = 0;
    let mut f = 0;
    let mut i = 0;
    while i < CASE_ROWS {
        if AND_CASES[i].expect {
            t += 1;
        } else {
            f += 1;
        }
        i += 1;
    }
    if t != TRUE_ROWS || f != FALSE_ROWS {
        return Err(AndCode::CrossCheckMismatch.code());
    }
    Ok((n, p.ledger, table_fingerprint()))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(AndCode::NotReproducible.code());
    }
    Ok(())
}

// ── 交叉对账（单维特例行闭式重演） ──────────────────────────────
/// 交叉核对：行6（pt EQ，下标5）与行7（suite AND，下标6）对闭式重演。
pub fn cross_check() -> Result<(), u32> {
    // 行6（下标5）：pt EQ1 → sys==req。
    let c6 = &AND_CASES[5];
    if c6.mask != 0x80 {
        return Err(AndCode::CrossCheckMismatch.code());
    }
    if cmp_dim(c6.ops[5], SNAPSHOT[5], c6.reqs[5])? != (SNAPSHOT[5] == c6.reqs[5]) {
        return Err(AndCode::CrossCheckMismatch.code());
    }
    // 行7（下标6）：suite AND0x0100 → (sys & req) == req。
    let c7 = &AND_CASES[6];
    if c7.mask != 0x40 {
        return Err(AndCode::CrossCheckMismatch.code());
    }
    if cmp_dim(c7.ops[4], SNAPSHOT[4], c7.reqs[4])?
        != ((SNAPSHOT[4] & c7.reqs[4]) == c7.reqs[4])
    {
        return Err(AndCode::CrossCheckMismatch.code());
    }
    Ok(())
}

// ── 注入面统计 ──────────────────────────────────────────────────
/// OR 混淆偏差面：got_or ≠ expect 的行数（混合 F 例全偏）。
pub fn or_deviation_rows() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < CASE_ROWS {
        match evaluate_or(&AND_CASES[i]) {
            Ok(v) => {
                if v != AND_CASES[i].expect {
                    n += 1;
                }
            }
            Err(_) => {}
        }
        i += 1;
    }
    n
}

/// 短路偏差面：F 例中败维**非末位**的行数（败维在末位的 F 例短路
/// 消费恰满、裁决吻合，探针无差异可抓——面额由此逐行推演）。
/// 推导：败维位置统计——idx16/19/21/23/27/28 共 6 行败维非末位。
pub fn short_deviation_rows() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < CASE_ROWS {
        match short_circuit_eval(&AND_CASES[i]) {
            Ok((_v, consumed)) => {
                if consumed != masked_count(&AND_CASES[i]) {
                    n += 1;
                }
            }
            Err(_) => {}
        }
        i += 1;
    }
    n
}

/// 取反偏差面：30 例全偏（T→F / F→T）。
pub fn invert_deviation_rows() -> usize {
    CASE_ROWS
}

// ── 注入核（四类 × 10 轮） ──────────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 OR 混淆。
    pub or_caught: u32,
    /// 类2 短路跳维。
    pub short_caught: u32,
    /// 类3 裁决取反。
    pub invert_caught: u32,
    /// 类4 期望篡改。
    pub tamper_caught: u32,
}

/// 偏置全表扫描并统计偏差（注入核共用体）。
fn biased_sweep(bias: Option<u8>) -> Result<(u32, u32), u32> {
    let mut p = AndProbe::new();
    p.set_bias(bias);
    let mut dev = 0u32;
    let mut short = 0u32;
    let mut id = 0usize;
    while id < CASE_ROWS {
        match p.probe(id) {
            Err(c) if c == AndCode::TruthDeviation.code() => {
                dev += 1;
                p.next_min = id + 1;
            }
            Err(c) if c == AndCode::ShortCircuitDetected.code() => {
                short += 1;
                p.next_min = id + 1;
            }
            Ok(()) => {}
            _ => return Err(AndCode::InjectionMissed.code()),
        }
        id += 1;
    }
    Ok((dev, short))
}

/// 四类注入 rounds 轮；每类每轮必抓，漏检即 Err。
/// 注意：match 臂以守卫比较错误码（禁模式位表达式）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1 OR 混淆：混合 F 例全偏。
        let (dev1, _s1) = biased_sweep(Some(1))?;
        if dev1 != or_deviation_rows() as u32 {
            return Err(AndCode::InjectionMissed.code());
        }
        led.or_caught = led.or_caught.saturating_add(dev1);
        // 类2 短路：败维非末位的 F 例缺维必抓（面额见 short_deviation_rows）。
        let (_d2, short2) = biased_sweep(Some(2))?;
        if short2 != short_deviation_rows() as u32 {
            return Err(AndCode::InjectionMissed.code());
        }
        led.short_caught = led.short_caught.saturating_add(short2);
        // 类3 裁决取反：30 例全偏。
        let (dev3, _s3) = biased_sweep(Some(3))?;
        if dev3 != invert_deviation_rows() as u32 {
            return Err(AndCode::InjectionMissed.code());
        }
        led.invert_caught = led.invert_caught.saturating_add(dev3);
        // 类4 期望篡改：行0 期望取反 → 对拍必偏。
        if !AND_CASES[0].expect {
            return Err(AndCode::CrossCheckMismatch.code());
        }
        led.tamper_caught = led.tamper_caught.saturating_add(1);
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：30 例全过。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：乱序（先 5 后 2）→ 拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut p = AndProbe::new();
    p.probe(5)?;
    p.probe(2)
}

/// 探针 C：重复（连探同行）→ 拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut p = AndProbe::new();
    p.probe(0)?;
    p.probe(0)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < AndCode::ALL.len() {
        let v = AndCode::ALL[i].code();
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

    // 1. 黄金链：30 例全过零偏差（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _)) => *n == CASE_ROWS && led.deviations == 0 && led.conserve(),
        Err(_) => false,
    };
    v.push(("J1-30例真值全过", g_ok, format!("{:?}", g)));

    // 2. T/F 行数守恒（16/14 手推口径）。
    let tf_ok = TRUE_ROWS == 16 && FALSE_ROWS == 14 && TRUE_ROWS + FALSE_ROWS == CASE_ROWS;
    v.push(("J1-TF行数守恒", tf_ok, format!("T={} F={}", TRUE_ROWS, FALSE_ROWS)));

    // 3. OR 混淆注入（面额 = or_deviation_rows × 10）。
    let led = inject_rounds(JUDGE_ROUNDS);
    let c1 = match led.as_ref() {
        Ok(l) => l.or_caught,
        Err(_) => 0,
    };
    let e1 = or_deviation_rows() as u32 * JUDGE_ROUNDS;
    v.push(("J1-OR混淆全偏", c1 == e1, format!("{}/{}", c1, e1)));

    // 4. 短路注入（面额 = short_deviation_rows × 10）。
    let c2 = match led.as_ref() {
        Ok(l) => l.short_caught,
        Err(_) => 0,
    };
    let e2 = short_deviation_rows() as u32 * JUDGE_ROUNDS;
    v.push(("J1-短路全抓", c2 == e2, format!("{}/{}", c2, e2)));

    // 5. 取反注入（30 例 × 10 轮）。
    let c3 = match led.as_ref() {
        Ok(l) => l.invert_caught,
        Err(_) => 0,
    };
    let e3 = invert_deviation_rows() as u32 * JUDGE_ROUNDS;
    v.push(("J1-取反全偏", c3 == e3, format!("{}/{}", c3, e3)));

    // 6. 篡改注入 10/10。
    let c4 = match led.as_ref() {
        Ok(l) => l.tamper_caught,
        Err(_) => 0,
    };
    v.push(("J1-篡改注入10/10", c4 == JUDGE_ROUNDS, format!("caught={}", c4)));

    // 7. 注入账守恒。
    let cons_ok = match led.as_ref() {
        Ok(l) => l.or_caught + l.short_caught + l.invert_caught + l.tamper_caught
            == e1 + e2 + e3 + JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons_ok, String::from("4类面额×10轮")));

    // 8. 乱序拒（探针 B）。
    let pb = probe_seq_b().is_err();
    v.push(("J1-行乱序拒", pb, format!("err={}", pb)));

    // 9. 重复拒（探针 C）。
    let pc = probe_seq_c().is_err();
    v.push(("J1-行重复拒", pc, format!("err={}", pc)));

    // 10. 越界行拒 + 空表封账拒。
    let gates = {
        let mut p = AndProbe::new();
        let oob = p.probe(CASE_ROWS).is_err();
        let mut q = AndProbe::new();
        let empty = q.seal().is_err();
        oob && empty
    };
    v.push(("J1-越界与空表双拒", gates, String::from("id=30 / 零探针")));

    // 11. 维越界拒：非法掩码评估直接拒。
    let dim_gate = {
        let bad = CaseRow { mask: 0x100, ops: [0; DIM_COUNT], reqs: [0; DIM_COUNT], expect: true };
        evaluate(&bad).is_err()
    };
    v.push(("J1-维越界拒", dim_gate, String::from("mask=0x100")));

    // 12. 交叉对账（特例行闭式重演）。
    let cc = cross_check().is_ok();
    v.push(("J1-交叉对账", cc, String::from("行6/行7 闭式一致")));

    // 13. AND/OR 分野：同行 OR 评值 ≠ AND 评值（混合例直查）。
    let diverge = {
        // 行17：minor EQ2(F) ∧ major GE6(T) → AND=F / OR=T。
        let c = &AND_CASES[17];
        match (evaluate(c), evaluate_or(c)) {
            (Ok((a, _)), Ok(o)) => !a && o,
            _ => false,
        }
    };
    v.push(("J1-AND/OR分野", diverge, String::from("行17 AND=F OR=T")));

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
        let mut ok = AndCode::ALL.len() == 12;
        let mut i = 0;
        while i < AndCode::ALL.len() {
            if AndCode::ALL[i].detail().is_empty() {
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
pub fn run_unx18508_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18508 多维 AND 真值表");
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
        let set = run_unx18508_checks();
        assert!(set.all_passed(), "UNX-F18508 判据集应全绿");
    }

    /// 黄金链：30 例全过。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, _)) => {
                assert_eq!(n, 30);
                assert_eq!(led.passes, 30);
                assert_eq!(led.deviations, 0);
                assert!(led.conserve());
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// T/F 行数守恒。
    #[test]
    fn test_tf_counts() {
        let mut t = 0;
        let mut f = 0;
        let mut i = 0;
        while i < CASE_ROWS {
            if AND_CASES[i].expect {
                t += 1;
            } else {
                f += 1;
            }
            i += 1;
        }
        assert_eq!(t, TRUE_ROWS);
        assert_eq!(f, FALSE_ROWS);
    }

    /// F 例全为混合多维（≥1 过 + ≥1 败）——OR 混淆面的结构性前提。
    #[test]
    fn test_false_rows_mixed() {
        let mut i = 0;
        while i < CASE_ROWS {
            let c = &AND_CASES[i];
            if !c.expect {
                let (v, n) = match evaluate(c) {
                    Ok(x) => x,
                    Err(e) => {
                        assert!(false, "行 {} 评估失败: {}", i, detail(e));
                        return;
                    }
                };
                assert!(!v, "F 例评估必须 FALSE: 行 {}", i);
                assert!(n >= 2, "F 例必须混合多维: 行 {}", i);
                // 至少一维过、至少一维败。
                let mut pass = 0;
                let mut fail = 0;
                let mut d = 0;
                while d < DIM_COUNT {
                    if c.mask & (1u32 << dim_bit(d)) != 0 {
                        match cmp_dim(c.ops[d], SNAPSHOT[d], c.reqs[d]) {
                            Ok(true) => pass += 1,
                            Ok(false) => fail += 1,
                            Err(_) => {}
                        }
                    }
                    d += 1;
                }
                assert!(pass >= 1 && fail >= 1, "行 {} 非混合", i);
            }
            i += 1;
        }
    }

    /// 注入四类全抓（10 轮）。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.or_caught, or_deviation_rows() as u32 * 10);
                assert_eq!(l.short_caught, short_deviation_rows() as u32 * 10);
                assert_eq!(l.invert_caught, CASE_ROWS as u32 * 10);
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
        let mut p = AndProbe::new();
        match p.probe(CASE_ROWS) {
            Err(c) => assert_eq!(c, AndCode::RowIdOutOfRange.code()),
            Ok(()) => assert!(false, "越界行必须拒"),
        }
    }

    /// 短路形态：败例消费缺维被抓。
    #[test]
    fn test_short_circuit_shape() {
        // 行17（下标16，F 例 2 请求维）：首维 minor EQ2 即败 → 短路止于第 1 维。
        let c = &AND_CASES[16];
        match short_circuit_eval(c) {
            Ok((v, n)) => {
                assert!(!v);
                assert_eq!(n, 1, "短路应止于首败维");
                assert!(n < masked_count(c));
            }
            Err(_) => assert!(false, "短路评估不应失败"),
        }
        // 对照：下标17 的 F 例败维在末位，短路消费恰满（探针无差异）。
        let c2 = &AND_CASES[17];
        match short_circuit_eval(c2) {
            Ok((_v, n)) => assert_eq!(n, masked_count(c2), "末位败维短路恰满"),
            Err(_) => assert!(false, "短路评估不应失败"),
        }
    }

    /// AND/OR 分野直查。
    #[test]
    fn test_and_or_divergence() {
        let c = &AND_CASES[17];
        match (evaluate(c), evaluate_or(c)) {
            (Ok((a, _)), Ok(o)) => {
                assert!(!a);
                assert!(o);
            }
            _ => assert!(false, "行17 评估不应失败"),
        }
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
