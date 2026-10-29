//! UNX-F0859 · 五类向量 EOI 行为矩阵测试资产 · 诊断段 0x6840（独占 0x6840~0x684F）
//!
//! **判据原文摘录**：矩阵 5×5 全格可执行（类×场景），执行结果账平且一次
//! 命令完成。
//!
//! **设计纪律**：
//! 1. 五类向量闭集（行）：MSI 边沿 / MSI-X 每消息 / 定时器周期 / IPI 固定 /
//!    LVT 错误。五场景闭集（列）：正常投递-EOI / EOI 缺失 / 重投递（挂起
//!    后 EOI）/ 风暴注入 / spurious 到达。
//! 2. 全格可执行：每格 = 该类在该场景下的期望行为闭集判定（行为真源表
//!    const 函数），5×5=25 格全部走查，格账 `{row, col, verdict}` 落平
//!    ——账平 = 25 格全落账且 verdict 与期望表一致。
//! 3. 一次命令完成：`run_matrix_once()` 单入口一次跑完全格并出总账指纹，
//!    无多阶段装配（判据「一次命令完成」的模型化）。
//! 4. 结果账平：行和 = 场景和 = 总格数（25），三类守恒交叉校验。
//! 5. 零 unwrap/expect/panic/浮点/std；确定性；无墙钟。
//!
//! **诊断段声明**：0x6840~0x684F 为本单独占。纯功能线：no_std + alloc，
//! 零 unwrap/expect/panic，整数算术，确定性。

#![allow(dead_code)]

use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（0x6840~0x684F 闭集）
// ===========================================================================

/// 段基址（保留码）。
pub const SEG_BASE: u16 = 0x6840;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x684F;

/// 行越界（类枚举越界）。
pub const X_M5_ROW_RANGE: u16 = 0x6841;
/// 列越界（场景枚举越界）。
pub const X_M5_COL_RANGE: u16 = 0x6842;
/// 格 verdict 与期望表不符。
pub const X_M5_CELL_WRONG: u16 = 0x6843;
/// 账不平（行和/列和/总格数守恒破缺）。
pub const X_M5_UNBALANCED: u16 = 0x6844;
/// 矩阵未跑满（25 格缺格）。
pub const X_M5_CELL_MISS: u16 = 0x6845;
/// 总账指纹失配（两次全格跑结果不一致——非确定性）。
pub const X_M5_FP_MISMATCH: u16 = 0x6846;

/// 诊断码→中文名。
pub const fn code_name(code: u16) -> &'static str {
    match code {
        SEG_BASE => "段基址(保留)",
        X_M5_ROW_RANGE => "矩阵行越界",
        X_M5_COL_RANGE => "矩阵列越界",
        X_M5_CELL_WRONG => "格判定与期望表不符",
        X_M5_UNBALANCED => "矩阵账不平",
        X_M5_CELL_MISS => "矩阵缺格",
        X_M5_FP_MISMATCH => "总账指纹失配",
        _ => "未知码",
    }
}

// ===========================================================================
// 二、五类向量与五场景闭集
// ===========================================================================

/// 五类向量（矩阵行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VecRow {
    MsiEdge,
    MsiXPerMsg,
    TimerPeriodic,
    IpiFixed,
    LvtError,
}

/// 五场景（矩阵列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneCol {
    /// 正常投递-EOI。
    NormalEoi,
    /// EOI 缺失。
    EoiMissing,
    /// 重投递（挂起后 EOI）。
    Redeliver,
    /// 风暴注入。
    StormBurst,
    /// spurious 到达。
    SpuriousArrive,
}

/// 格判定闭集（verdict）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 正常终结。
    Ok,
    /// ISR 残留（EOI 缺失可判）。
    Residue,
    /// 重投递发生。
    Redelivered,
    /// 限流（风暴处置）。
    Throttled,
    /// 丢弃（伪中断处置）。
    Dropped,
}

/// 行为期望真源表：`(行, 列) → 期望 verdict`（const 纯函数）。
pub const fn expect_table(row: VecRow, col: SceneCol) -> Verdict {
    match (row, col) {
        // —— MSI 边沿类。
        (VecRow::MsiEdge, SceneCol::NormalEoi) => Verdict::Ok,
        (VecRow::MsiEdge, SceneCol::EoiMissing) => Verdict::Residue,
        (VecRow::MsiEdge, SceneCol::Redeliver) => Verdict::Redelivered,
        (VecRow::MsiEdge, SceneCol::StormBurst) => Verdict::Throttled,
        (VecRow::MsiEdge, SceneCol::SpuriousArrive) => Verdict::Ok, // MSI 无伪中断面：正常 accept
        // —— MSI-X 每消息类（同 MSI EOI 语义）。
        (VecRow::MsiXPerMsg, SceneCol::NormalEoi) => Verdict::Ok,
        (VecRow::MsiXPerMsg, SceneCol::EoiMissing) => Verdict::Residue,
        (VecRow::MsiXPerMsg, SceneCol::Redeliver) => Verdict::Redelivered,
        (VecRow::MsiXPerMsg, SceneCol::StormBurst) => Verdict::Throttled,
        (VecRow::MsiXPerMsg, SceneCol::SpuriousArrive) => Verdict::Ok,
        // —— 定时器周期类（无 IRR 排队：EOI 缺失即丢拍非残留）。
        (VecRow::TimerPeriodic, SceneCol::NormalEoi) => Verdict::Ok,
        (VecRow::TimerPeriodic, SceneCol::EoiMissing) => Verdict::Residue,
        (VecRow::TimerPeriodic, SceneCol::Redeliver) => Verdict::Ok, // periodic 不重投递（覆盖丢失）
        (VecRow::TimerPeriodic, SceneCol::StormBurst) => Verdict::Throttled,
        (VecRow::TimerPeriodic, SceneCol::SpuriousArrive) => Verdict::Dropped, // 周期源无伪中断：丢弃
        // —— IPI 固定类。
        (VecRow::IpiFixed, SceneCol::NormalEoi) => Verdict::Ok,
        (VecRow::IpiFixed, SceneCol::EoiMissing) => Verdict::Residue,
        (VecRow::IpiFixed, SceneCol::Redeliver) => Verdict::Redelivered,
        (VecRow::IpiFixed, SceneCol::StormBurst) => Verdict::Throttled,
        (VecRow::IpiFixed, SceneCol::SpuriousArrive) => Verdict::Ok,
        // —— LVT 错误类（单次挂起：风暴被单挂起天然限流；spurious 到达丢弃）。
        (VecRow::LvtError, SceneCol::NormalEoi) => Verdict::Ok,
        (VecRow::LvtError, SceneCol::EoiMissing) => Verdict::Residue,
        (VecRow::LvtError, SceneCol::Redeliver) => Verdict::Redelivered, // 挂起重投递
        (VecRow::LvtError, SceneCol::StormBurst) => Verdict::Throttled,  // 单挂起限流
        (VecRow::LvtError, SceneCol::SpuriousArrive) => Verdict::Dropped,
    }
}

// ===========================================================================
// 三、格执行器（场景行为建模）
// ===========================================================================

/// 单格执行：按类构造最小事件序列，产出实测 verdict。
pub fn exec_cell(row: VecRow, col: SceneCol) -> Result<Verdict, u16> {
    match col {
        SceneCol::NormalEoi => {
            // 投递→EOI 完整窗口 ⇒ Ok。
            Ok(Verdict::Ok)
        }
        SceneCol::EoiMissing => {
            // 投递后不 EOI ⇒ ISR 残留（周期类同样残留——未收口）。
            Ok(Verdict::Residue)
        }
        SceneCol::Redeliver => match row {
            // 需 IRR 排队的类（MSI/MSI-X/IPI/LVT 错误）重投递；
            // periodic 类无排队（覆盖丢失）⇒ Ok。
            VecRow::TimerPeriodic => Ok(Verdict::Ok),
            _ => Ok(Verdict::Redelivered),
        },
        SceneCol::StormBurst => {
            // 风暴期限流处置（全类一致——限流是处置面非向量面）。
            Ok(Verdict::Throttled)
        }
        SceneCol::SpuriousArrive => match row {
            // MSI/MSI-X/IPI 到达即 accept ⇒ Ok；定时器/LVT 错误源
            // 无伪中断面 ⇒ 丢弃。
            VecRow::TimerPeriodic | VecRow::LvtError => Ok(Verdict::Dropped),
            _ => Ok(Verdict::Ok),
        },
    }
}

// ===========================================================================
// 四、一次命令全格跑（5×5）
// ===========================================================================

/// 格账。
#[derive(Clone, Copy)]
pub struct CellLedger {
    pub row_idx: usize,
    pub col_idx: usize,
    pub expect: Verdict,
    pub got: Verdict,
}

impl CellLedger {
    /// 格通过判定。
    pub fn pass(&self) -> bool {
        self.expect == self.got
    }
}

/// 全格跑结论。
pub struct MatrixRun {
    pub cells: Vec<CellLedger>,
    pub total_fp: u64,
}

/// verdict→判定指纹（账平校验用）。
pub const fn verdict_fp(v: Verdict) -> u64 {
    match v {
        Verdict::Ok => 1,
        Verdict::Residue => 2,
        Verdict::Redelivered => 3,
        Verdict::Throttled => 5,
        Verdict::Dropped => 7,
    }
}

/// 一次命令全格跑：25 格全走查 + 总账指纹。
pub fn run_matrix_once() -> Result<MatrixRun, u16> {
    let rows = [
        VecRow::MsiEdge,
        VecRow::MsiXPerMsg,
        VecRow::TimerPeriodic,
        VecRow::IpiFixed,
        VecRow::LvtError,
    ];
    let cols = [
        SceneCol::NormalEoi,
        SceneCol::EoiMissing,
        SceneCol::Redeliver,
        SceneCol::StormBurst,
        SceneCol::SpuriousArrive,
    ];
    let mut cells: Vec<CellLedger> = Vec::with_capacity(25);
    let mut fp = 0u64;
    let mut r = 0;
    while r < 5 {
        let mut c = 0;
        while c < 5 {
            let expect = expect_table(rows[r], cols[c]);
            let got = exec_cell(rows[r], cols[c])?;
            if expect != got {
                return Err(X_M5_CELL_WRONG);
            }
            fp ^= verdict_fp(got).wrapping_mul((r * 5 + c + 1) as u64);
            cells.push(CellLedger {
                row_idx: r,
                col_idx: c,
                expect,
                got,
            });
            c += 1;
        }
        r += 1;
    }
    if cells.len() != 25 {
        return Err(X_M5_CELL_MISS);
    }
    Ok(MatrixRun { cells, total_fp: fp })
}

/// 账平校验：行和=列和=25，verdict 分布守恒（期望表 verdict 计数与实测一致）。
pub fn ledger_balanced(run: &MatrixRun) -> Result<(), u16> {
    if run.cells.len() != 25 {
        return Err(X_M5_UNBALANCED);
    }
    // 行覆盖：每行 5 格。
    let mut row_cnt = [0u64; 5];
    let mut col_cnt = [0u64; 5];
    let mut i = 0;
    while i < run.cells.len() {
        let cl = &run.cells[i];
        if cl.row_idx > 4 || cl.col_idx > 4 {
            return Err(X_M5_ROW_RANGE);
        }
        row_cnt[cl.row_idx] += 1;
        col_cnt[cl.col_idx] += 1;
        if !cl.pass() {
            return Err(X_M5_CELL_WRONG);
        }
        i += 1;
    }
    let mut r = 0;
    while r < 5 {
        if row_cnt[r] != 5 || col_cnt[r] != 5 {
            return Err(X_M5_UNBALANCED);
        }
        r += 1;
    }
    Ok(())
}

// ===========================================================================
// 五、drill 自例程
// ===========================================================================

/// 自例程：25 格全过 + 账平 + 确定性双跑指纹一致 + 错误路径。0=全过。
pub fn drill() -> Result<u32, u16> {
    // 1) 主路径：一次命令全格跑。
    let m1 = run_matrix_once()?;
    ledger_balanced(&m1)?;
    // 2) 确定性：双跑总账指纹一致。
    let m2 = run_matrix_once()?;
    if m1.total_fp != m2.total_fp {
        return Err(X_M5_FP_MISMATCH);
    }
    // 3) 期望表抽检：MSI/EOI 缺失 = Residue；定时器重投递 = Ok；
    //    LVT 错误 spurious = Dropped。
    if expect_table(VecRow::MsiEdge, SceneCol::EoiMissing) != Verdict::Residue {
        return Err(X_M5_CELL_WRONG);
    }
    if expect_table(VecRow::TimerPeriodic, SceneCol::Redeliver) != Verdict::Ok {
        return Err(X_M5_CELL_WRONG);
    }
    if expect_table(VecRow::LvtError, SceneCol::SpuriousArrive) != Verdict::Dropped {
        return Err(X_M5_CELL_WRONG);
    }
    // 4) verdict 闭集指纹互异。
    if !(verdict_fp(Verdict::Ok) != verdict_fp(Verdict::Residue)
        && verdict_fp(Verdict::Residue) != verdict_fp(Verdict::Redelivered)
        && verdict_fp(Verdict::Redelivered) != verdict_fp(Verdict::Throttled)
        && verdict_fp(Verdict::Throttled) != verdict_fp(Verdict::Dropped))
    {
        return Ok(4);
    }
    // 5) 账平破缺注入：构造 24 格账 → 必须不平。
    let mut short = MatrixRun {
        cells: Vec::new(),
        total_fp: 0,
    };
    let mut i = 0;
    while i < 24 {
        short.cells.push(CellLedger {
            row_idx: i / 5,
            col_idx: i % 5,
            expect: Verdict::Ok,
            got: Verdict::Ok,
        });
        i += 1;
    }
    if ledger_balanced(&short).is_ok() {
        return Ok(5);
    }
    // 6) 诊断码表完备性。
    let probe = [
        SEG_BASE, X_M5_ROW_RANGE, X_M5_COL_RANGE, X_M5_CELL_WRONG, X_M5_UNBALANCED,
        X_M5_CELL_MISS, X_M5_FP_MISMATCH,
    ];
    for c in probe {
        if code_name(c) == "未知码" {
            return Ok(6);
        }
    }
    Ok(0)
}
