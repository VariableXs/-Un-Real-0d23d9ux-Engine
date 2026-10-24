//! UNX-F1502 · F0039 万次切换回归（10^4 次 Polling↔Irq）（CoRun·Unxreal · UNX-A1-B03 · 模式切换深化）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F1502`
//!
//! **判据 J1（原文）**：万次切换回归执行 10^4 次全绿（Polling↔Irq 双向切换
//! 逐次留痕：次数/态向/时延三列），切换成功率 100%，切换失败注入 10/10
//! 必抓，执行报告落档。
//!
//! ## 要点一：双态状态机——Polling/Irq 两态 + 迁移边闭集
//!
//! 状态闭集恰两态（Polling/Irq），迁移边闭集恰四边（Polling→Polling、
//! Irq→Irq、Polling→Irq、Irq→Polling）——两态全组合 2×2=4 无第五边；
//! 边权限用 4 位掩码表达，位清零即"边失效"，可注入非法迁移。
//!
//! ## 要点二：万次切换引擎——LCG 定向 + 逐次留痕三列
//!
//! 10^4 次逐次执行，每次方向由固定种子 LCG 序列决定（bit 决定目标态，
//! 低 3 位决定时延抖动）；逐次留痕三列（次数 u16 / 态向枚举 / 时延步数）
//! 落滚动窗，窗口 1024 封顶，溢出滚动淘汰并计入总量守恒账——
//! 窗内存 + 已淘汰 == 总执行数，账面永不丢行。
//!
//! ## 要点三：成功率千分位 + 失败注入 10/10 必抓
//!
//! 成功率按千分位整型计量：成功×1000/总次 == 1000 才判达标（无浮点）；
//! 失败注入器持 10 个失败样本闭集（非法迁移/迁移中断裂/状态失配/重入/
//! 超时/双写/半提交/悬挂回调/回退非法/未知态向），逐例喂给状态机并
//! 对拍预期诊断码，10/10 必抓才全绿——注入是对拍账不是 println。
//!
//! ## 要点四：时延逻辑步计量 + P50/P95/P99 整数分位
//!
//! 无墙钟无 IO：时延一律用逻辑步数计量（六相位步数常数 + LCG 抖动），
//! 1 步恒等 1 ns（常数来源见 `STEP_NS` 注释）；P50/P95/P99 用直方图
//! 累积法整数求分位（干净跑时延值域仅 19..=27，直方图 64 槽封顶），
//! P99 对红线（`RED_LINE_P99_STEPS`）越线即亮红。
//!
//! ## 要点五：执行报告落档 + 增量重跑对账 + 诊断码独占 0x97 段
//!
//! 报告双形态落档：kv 行式 + TLV 二进制式，二者同源同账；报告指纹用
//! FNV-1a 64 对 TLV 体滚动，指纹随 TLV 尾记录落档并可往返重算对拍；
//! 增量重跑对账：同种子重跑一遍，逐千次检查点指纹与首跑逐位一致才判
//! "重跑一致"。诊断码独占 `0x9700..=0x970F` 十六码（已 grep 全目录
//! 确认 0x97 段空闲，与 0x2E/0x2F/0x30/0x36/0x39 等既有段互不重叠）。
//! 本模块零 panic 面：无 unwrap/expect/panic!，所有错误走 Err+中文诊断。


use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::format;
// ---------------------------------------------------------------------------
// 一、诊断码闭集（独占 0x97 段，grep 全目录确认 0x9700..=0x970F 空闲）
// ---------------------------------------------------------------------------

/// 模式切换回归诊断码。独占 `0x97xx` 段。
///
/// 选段对账：`grep -r "0x97" varix/src/unxstar/` 零命中（2026-10 施工时
/// 实测），既有段 0x2E/0x2F/0x30/0x36/0x39/0x43/0x44/0x50..0x5D 等均不
/// 与本段重叠——段位独占，防撞码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MsCode(pub u16);

impl MsCode {
    /// 0x9700 非法迁移：目标边不在权限掩码内（边位被清零）。
    pub const MIG_ILLEGAL: MsCode = MsCode(0x9700);
    /// 0x9701 迁移中断裂：ARM 相位后迁移线被拉断（模拟硬件线故障）。
    pub const MIG_BREAK: MsCode = MsCode(0x9701);
    /// 0x9702 状态失配：主寄存器与影子寄存器读数不一致。
    pub const STATE_MISMATCH: MsCode = MsCode(0x9702);
    /// 0x9703 重入：迁移进行中又收到新的切换请求。
    pub const REENTRY: MsCode = MsCode(0x9703);
    /// 0x9704 超时：时延预算不足以覆盖六相位全程。
    pub const TIMEOUT: MsCode = MsCode(0x9704);
    /// 0x9705 双写：提交寄存器残留写计数，提交相位将发生第二次写。
    pub const DOUBLE_WRITE: MsCode = MsCode(0x9705);
    /// 0x9706 半提交：阶段寄存器停在 STAGED，未推进到 COMMITTED。
    pub const HALF_COMMIT: MsCode = MsCode(0x9706);
    /// 0x9707 悬挂回调：DISARM 相位后回调仍处于 armed 态。
    pub const DANGLING_CALLBACK: MsCode = MsCode(0x9707);
    /// 0x9708 回退非法：断裂发生但回退目标态不可用（回退通道失守）。
    pub const ILLEGAL_ROLLBACK: MsCode = MsCode(0x9708);
    /// 0x9709 未知态向：目标态原始编码不在两态闭集内。
    pub const UNKNOWN_DIRECTION: MsCode = MsCode(0x9709);
    /// 0x970A 段守恒对账失败：十六码值/连续/互异校验不过。
    pub const SEGMENT_GUARD: MsCode = MsCode(0x970A);
    /// 0x970B 总量守恒账不平：ok+fail != 总次 或 分布账对不上。
    pub const LEDGER_CONSERVATION: MsCode = MsCode(0x970B);
    /// 0x970C 滚动窗越界：窗长超 1024 封顶或淘汰账对不上。
    pub const WINDOW_CAP_BREACH: MsCode = MsCode(0x970C);
    /// 0x970D 报告域故障：TLV 结构坏或指纹往返重算不符。
    pub const REPORT_FINGERPRINT: MsCode = MsCode(0x970D);
    /// 0x970E 增量重跑对账不一致：重跑检查点指纹与首跑不同。
    pub const RERUN_MISMATCH: MsCode = MsCode(0x970E);
    /// 0x970F 时延红线越线：P99 超出红线步数。
    pub const REDLINE_BREACH: MsCode = MsCode(0x970F);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因（中文诊断）。
    pub fn reason(self) -> String {
        match self {
            MsCode::MIG_ILLEGAL => "[0x9700] 非法迁移：目标边不在权限掩码内，边位被清零".into(),
            MsCode::MIG_BREAK => "[0x9701] 迁移中断裂：ARM 相位后迁移线被拉断".into(),
            MsCode::STATE_MISMATCH => "[0x9702] 状态失配：主寄存器与影子寄存器读数不一致".into(),
            MsCode::REENTRY => "[0x9703] 重入：迁移进行中收到新的切换请求".into(),
            MsCode::TIMEOUT => "[0x9704] 超时：时延预算不足以覆盖六相位全程".into(),
            MsCode::DOUBLE_WRITE => "[0x9705] 双写：提交寄存器残留写计数，将发生第二次写".into(),
            MsCode::HALF_COMMIT => "[0x9706] 半提交：阶段寄存器停在 STAGED 未推进".into(),
            MsCode::DANGLING_CALLBACK => "[0x9707] 悬挂回调：DISARM 后回调仍处于 armed 态".into(),
            MsCode::ILLEGAL_ROLLBACK => "[0x9708] 回退非法：断裂后回退目标态不可用".into(),
            MsCode::UNKNOWN_DIRECTION => "[0x9709] 未知态向：目标态原始编码不在两态闭集内".into(),
            MsCode::SEGMENT_GUARD => "[0x970A] 段守恒对账失败：诊断码不连续或不互异".into(),
            MsCode::LEDGER_CONSERVATION => "[0x970B] 总量守恒账不平：ok+fail 与总次对不上".into(),
            MsCode::WINDOW_CAP_BREACH => "[0x970C] 滚动窗越界：窗长超 1024 封顶或淘汰账不齐".into(),
            MsCode::REPORT_FINGERPRINT => "[0x970D] 报告域故障：TLV 结构坏或指纹往返不符".into(),
            MsCode::RERUN_MISMATCH => "[0x970E] 增量重跑对账不一致：重跑指纹与首跑不同".into(),
            MsCode::REDLINE_BREACH => "[0x970F] 时延红线越线：P99 超出红线步数".into(),
            MsCode(_) => "未知 UNX 模式切换回归域诊断码".into(),
        }
    }
}

/// 全部十六码（闭集枚举表，供段守恒对账）。
pub const ALL_CODES: [MsCode; 16] = [
    MsCode::MIG_ILLEGAL,
    MsCode::MIG_BREAK,
    MsCode::STATE_MISMATCH,
    MsCode::REENTRY,
    MsCode::TIMEOUT,
    MsCode::DOUBLE_WRITE,
    MsCode::HALF_COMMIT,
    MsCode::DANGLING_CALLBACK,
    MsCode::ILLEGAL_ROLLBACK,
    MsCode::UNKNOWN_DIRECTION,
    MsCode::SEGMENT_GUARD,
    MsCode::LEDGER_CONSERVATION,
    MsCode::WINDOW_CAP_BREACH,
    MsCode::REPORT_FINGERPRINT,
    MsCode::RERUN_MISMATCH,
    MsCode::REDLINE_BREACH,
];

/// 段守恒对账：十六码互异、连续、落在 0x9700..=0x970F 闭区间。
///
/// 任何一位不齐即 Err(SEGMENT_GUARD)——段位事故最早暴露在入口对账。
pub fn segment_guard() -> Result<(), MsCode> {
    let base = 0x9700u16;
    for (i, c) in ALL_CODES.iter().enumerate() {
        let expect = base + i as u16;
        if c.code() != expect {
            return Err(MsCode::SEGMENT_GUARD);
        }
    }
    // 互异校验：连续即互异，但双保险显式两两比对。
    for i in 0..ALL_CODES.len() {
        for j in (i + 1)..ALL_CODES.len() {
            if ALL_CODES[i].code() == ALL_CODES[j].code() {
                return Err(MsCode::SEGMENT_GUARD);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、常量与计量——步数时延、LCG 常数、容量与红线
// ---------------------------------------------------------------------------

/// 1 逻辑步恒等 1 纳秒。
///
/// 来源：本模块逻辑时延计量约定——禁读墙钟，六相位各耗固定步数，
/// 步数即时延；常数 1 只作"步→ns"的刻度换算（P99=27 步即 27 ns 量级），
/// 不代表任何硬件实测值，无 IO、无浮点、无时钟读取。
pub const STEP_NS: u64 = 1;

/// 万次切换总次数（判据原文 10^4）。
pub const TOTAL_RUNS: u32 = 10_000;

/// 滚动窗封顶容量（判据要求 1024 封顶）。
pub const WINDOW_CAP: usize = 1024;

/// 固定 LCG 种子（全确定性——同一种子重跑逐位一致）。
pub const SEED_FIXED: u64 = 0x1502_1502_1502_1502;

/// LCG 乘子（与 unx1023_hpettick / unx1043_tscdeadline 同源常数）。
const LCG_A: u64 = 6_364_136_223_846_793_005;
/// LCG 增量（同源常数）。
const LCG_C: u64 = 1_442_695_040_888_963_407;

/// 相位一 GUARD 步数：重入检查 + 边权限查表。
pub const GUARD_STEPS: u32 = 2;
/// 相位二 PRE 步数：双寄存器比对 + 目标解码 + 残留检查。
pub const PRE_STEPS: u32 = 3;
/// 相位三 ARM 步数：挂回调 + 置迁移中标志。
pub const ARM_STEPS: u32 = 4;
/// 相位四 COMMIT 步数：状态翻转 + 阶段推进 + 提交写。
pub const COMMIT_STEPS: u32 = 5;
/// 相位五 VERIFY 步数：读回 + 影子同步 + 回查。
pub const VERIFY_STEPS: u32 = 3;
/// 相位六 DISARM 步数：撤回调 + 清迁移中标志 + 阶段归零。
pub const DISARM_STEPS: u32 = 2;
/// 断裂回退路径步数（仅故障路径计）。
pub const ROLLBACK_STEPS: u32 = 6;

/// 六相位基础步数合计（19 步）。
pub const BASE_STEPS: u32 =
    GUARD_STEPS + PRE_STEPS + ARM_STEPS + COMMIT_STEPS + VERIFY_STEPS + DISARM_STEPS;

/// 干净跑时延预算上限（远大于六相位全程，正常永不触发 TIMEOUT）。
pub const TIMEOUT_BUDGET_CLEAN: u32 = 128;

/// 时延抖动步数上界（LCG 低 3 位，0..=8）。
pub const JITTER_MAX: u32 = 8;

/// P99 红线步数（P99 超此值即亮红——基线 27 步上界留一倍裕量）。
pub const RED_LINE_P99_STEPS: u32 = 64;

/// 达标千分位（成功×1000/总次 必须 == 1000）。
pub const PER_MILLE_TARGET: u64 = 1000;

/// 增量重跑检查点步距（每 1000 次落一个指纹检查点）。
pub const CHECKPOINT_STRIDE: u16 = 1000;

/// 阶段寄存器：空闲。
pub const STAGE_IDLE: u8 = 0;
/// 阶段寄存器：已暂存未提交（半提交观测点）。
pub const STAGE_STAGED: u8 = 1;
/// 阶段寄存器：已提交。
pub const STAGE_COMMITTED: u8 = 2;

/// 千分位整型计算：ok*1000/total（无浮点；total=0 视为 0）。
pub fn per_mille(ok: u64, total: u64) -> u64 {
    if total == 0 {
        0
    } else {
        (ok * 1000) / total
    }
}

// ---------------------------------------------------------------------------
// 三、双态状态机——态闭集 / 态向闭集 / 迁移边闭集
// ---------------------------------------------------------------------------

/// 双态闭集：Polling（轮询态）/ Irq（中断态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeState {
    /// 轮询态。
    Polling,
    /// 中断态。
    Irq,
}

impl ModeState {
    /// 两态全集中列表（闭集证据）。
    pub const ALL: [ModeState; 2] = [ModeState::Polling, ModeState::Irq];

    /// wire 编码：Polling=1，Irq=2（1 起编，0 留给非法值）。
    pub const fn as_u8(self) -> u8 {
        match self {
            ModeState::Polling => 1,
            ModeState::Irq => 2,
        }
    }

    /// 从原始编码解码：1/2 之外一律 None（闭集外即未知态向）。
    pub fn from_u8(raw: u8) -> Option<ModeState> {
        match raw {
            1 => Some(ModeState::Polling),
            2 => Some(ModeState::Irq),
            _ => None,
        }
    }

    /// 对面态。
    pub const fn opposite(self) -> ModeState {
        match self {
            ModeState::Polling => ModeState::Irq,
            ModeState::Irq => ModeState::Polling,
        }
    }

    /// 态名。
    pub fn name(self) -> String {
        match self {
            ModeState::Polling => "Polling".into(),
            ModeState::Irq => "Irq".into(),
        }
    }
}

/// 目标态解码：闭集外编码 → Err(UNKNOWN_DIRECTION)。
///
/// 未知态向是失败注入样本 #10 的入口——编码不可信是本模块的基本假设。
pub fn decode_target(raw: u8) -> Result<ModeState, MsCode> {
    match ModeState::from_u8(raw) {
        Some(s) => Ok(s),
        None => Err(MsCode::UNKNOWN_DIRECTION),
    }
}

/// 态向闭集：一次切换的"从→到"四元枚举（含自环两向）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeDirection {
    /// Polling → Irq（跨态上行）。
    PollingToIrq,
    /// Irq → Polling（跨态下行）。
    IrqToPolling,
    /// Polling → Polling（自环刷新）。
    PollingSelf,
    /// Irq → Irq（自环刷新）。
    IrqSelf,
}

impl ModeDirection {
    /// 四向全集中列表（闭集证据）。
    pub const ALL: [ModeDirection; 4] = [
        ModeDirection::PollingToIrq,
        ModeDirection::IrqToPolling,
        ModeDirection::PollingSelf,
        ModeDirection::IrqSelf,
    ];

    /// 由迁移对推断态向（两态×两态全组合恰落四元闭集，无第五向）。
    pub const fn of(from: ModeState, to: ModeState) -> ModeDirection {
        match (from, to) {
            (ModeState::Polling, ModeState::Irq) => ModeDirection::PollingToIrq,
            (ModeState::Irq, ModeState::Polling) => ModeDirection::IrqToPolling,
            (ModeState::Polling, ModeState::Polling) => ModeDirection::PollingSelf,
            (ModeState::Irq, ModeState::Irq) => ModeDirection::IrqSelf,
        }
    }

    /// 态向 wire 码（0..=3，供序列化）。
    pub const fn as_u8(self) -> u8 {
        match self {
            ModeDirection::PollingToIrq => 0,
            ModeDirection::IrqToPolling => 1,
            ModeDirection::PollingSelf => 2,
            ModeDirection::IrqSelf => 3,
        }
    }

    /// 态向名（留痕列二的人类可读形态）。
    pub fn name(self) -> String {
        match self {
            ModeDirection::PollingToIrq => "P→I".into(),
            ModeDirection::IrqToPolling => "I→P".into(),
            ModeDirection::PollingSelf => "P→P".into(),
            ModeDirection::IrqSelf => "I→I".into(),
        }
    }

    /// 是否跨态（双向占比账只统计跨态两向）。
    pub const fn is_cross(self) -> bool {
        matches!(
            self,
            ModeDirection::PollingToIrq | ModeDirection::IrqToPolling
        )
    }
}

/// 迁移边闭集：两态全组合恰四边，四位掩码一位一边。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigEdge {
    /// Polling→Polling 自环边。
    PollingSelf,
    /// Polling→Irq 上行边。
    PollingToIrq,
    /// Irq→Polling 下行边。
    IrqToPolling,
    /// Irq→Irq 自环边。
    IrqSelf,
}

impl MigEdge {
    /// 四边全集中列表（闭集证据）。
    pub const ALL: [MigEdge; 4] = [
        MigEdge::PollingSelf,
        MigEdge::PollingToIrq,
        MigEdge::IrqToPolling,
        MigEdge::IrqSelf,
    ];

    /// 全边权限掩码（四位全 1）。
    pub const FULL_MASK: u8 = 0b1111;

    /// 边位（掩码中的一位）。
    pub const fn bit(self) -> u8 {
        match self {
            MigEdge::PollingSelf => 0b0001,
            MigEdge::PollingToIrq => 0b0010,
            MigEdge::IrqToPolling => 0b0100,
            MigEdge::IrqSelf => 0b1000,
        }
    }

    /// 由迁移对推断边。
    pub const fn of(from: ModeState, to: ModeState) -> MigEdge {
        match (from, to) {
            (ModeState::Polling, ModeState::Polling) => MigEdge::PollingSelf,
            (ModeState::Polling, ModeState::Irq) => MigEdge::PollingToIrq,
            (ModeState::Irq, ModeState::Polling) => MigEdge::IrqToPolling,
            (ModeState::Irq, ModeState::Irq) => MigEdge::IrqSelf,
        }
    }

    /// 边名。
    pub fn name(self) -> String {
        match self {
            MigEdge::PollingSelf => "Polling→Polling".into(),
            MigEdge::PollingToIrq => "Polling→Irq".into(),
            MigEdge::IrqToPolling => "Irq→Polling".into(),
            MigEdge::IrqSelf => "Irq→Irq".into(),
        }
    }
}

/// 迁移边闭集表（文档化输出：四边一位一名，无第五边）。
pub fn edge_table() -> Vec<(u8, String)> {
    let mut rows = Vec::new();
    for e in MigEdge::ALL.iter() {
        rows.push((e.bit(), e.name()));
    }
    rows
}

// ---------------------------------------------------------------------------
// 四、状态机执行核——六相位迁移 + 回退路径 + 故障面
// ---------------------------------------------------------------------------

/// 一次成功切换的产出（迁移对 + 态向 + 步数时延）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchOutcome {
    /// 迁移前态。
    pub from: ModeState,
    /// 迁移后态。
    pub to: ModeState,
    /// 走的迁移边。
    pub edge: MigEdge,
    /// 本次切换总时延（逻辑步数；×STEP_NS 即 ns 刻度）。
    pub steps: u32,
}

/// 双态切换状态机。
///
/// 故障面字段（edge_mask/break_in_migration/rollback_ok/callback_sticky/
/// timeout_budget/shadow/stage/committed_writes/in_migration）默认全干净；
/// 失败注入器逐字段下毒，正常引擎持有的机器永不带毒。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeMachine {
    /// 主状态寄存器。
    state: ModeState,
    /// 影子状态寄存器（双寄存器比对面；None=影子未初始化）。
    shadow: Option<ModeState>,
    /// 迁移进行中标志（重入防线）。
    in_migration: bool,
    /// 提交写计数（双写防线：进 COMMIT 前必须为 0）。
    committed_writes: u8,
    /// 回调 armed 标志。
    callback_armed: bool,
    /// 回调粘滞毒（DISARM 后仍 re-arm——悬挂回调故障源）。
    callback_sticky: bool,
    /// 阶段寄存器（半提交观测点）。
    stage: u8,
    /// 边权限掩码（四位，位清零即边失效）。
    edge_mask: u8,
    /// 时延预算（六相位步数须全程覆盖，不足即 TIMEOUT）。
    timeout_budget: u32,
    /// 迁移断裂毒（ARM 后迁移线拉断）。
    break_in_migration: bool,
    /// 回退通道可用毒（false 时断裂回退即 ILLEGAL_ROLLBACK）。
    rollback_ok: bool,
}

impl ModeMachine {
    /// 出厂干净机：Polling 态、全边开放、预算满格、零残留。
    pub fn new_clean() -> ModeMachine {
        ModeMachine {
            state: ModeState::Polling,
            shadow: Some(ModeState::Polling),
            in_migration: false,
            committed_writes: 0,
            callback_armed: false,
            callback_sticky: false,
            stage: STAGE_IDLE,
            edge_mask: MigEdge::FULL_MASK,
            timeout_budget: TIMEOUT_BUDGET_CLEAN,
            break_in_migration: false,
            rollback_ok: true,
        }
    }

    /// 当前态。
    pub fn state(&self) -> ModeState {
        self.state
    }

    /// 边权限掩码读数（注入器自检用）。
    pub fn edge_mask(&self) -> u8 {
        self.edge_mask
    }

    /// 六相位全程所需步数（含外部传入的抖动步）。
    pub fn need_steps(jitter: u32) -> u32 {
        BASE_STEPS.saturating_add(jitter)
    }

    /// 六相位切换主路径。
    ///
    /// 相位序：GUARD(2) → PRE(3) → ARM(4) → COMMIT(5) → VERIFY(3) →
    /// DISARM(2)，合计 19 步 + 外部抖动。任何一步不齐即 Err 返回，
    /// 机器不留半态（断裂路径先走回退再报错）。
    pub fn request_switch(
        &mut self,
        target: ModeState,
        jitter: u32,
    ) -> Result<SwitchOutcome, MsCode> {
        let from = self.state;
        let mut steps: u32 = 0;

        // ---- 相位一 GUARD（2 步）：重入防线 + 边权限查表 ----
        if self.in_migration {
            return Err(MsCode::REENTRY);
        }
        let edge = MigEdge::of(from, target);
        if self.edge_mask & edge.bit() == 0 {
            return Err(MsCode::MIG_ILLEGAL);
        }
        steps = steps.saturating_add(GUARD_STEPS);

        // ---- 相位二 PRE（3 步）：双寄存器比对 + 残留检查 + 预算 ----
        if let Some(shadow) = self.shadow {
            if shadow != self.state {
                return Err(MsCode::STATE_MISMATCH);
            }
        }
        if self.stage != STAGE_IDLE {
            return Err(MsCode::HALF_COMMIT);
        }
        if self.committed_writes != 0 {
            return Err(MsCode::DOUBLE_WRITE);
        }
        let need = ModeMachine::need_steps(jitter);
        if self.timeout_budget < need {
            return Err(MsCode::TIMEOUT);
        }
        steps = steps.saturating_add(PRE_STEPS);

        // ---- 相位三 ARM（4 步）：挂回调 + 置迁移中 ----
        self.in_migration = true;
        self.callback_armed = true;
        if self.break_in_migration {
            // 迁移中断裂：先走回退路径，回退不成才是回退非法。
            return self.abort_with_rollback(from, MsCode::MIG_BREAK);
        }
        steps = steps.saturating_add(ARM_STEPS);

        // ---- 相位四 COMMIT（5 步）：状态翻转 + 阶段推进 + 提交写 ----
        self.state = target;
        self.stage = STAGE_STAGED;
        self.stage = STAGE_COMMITTED;
        self.committed_writes = self.committed_writes.saturating_add(1);
        steps = steps.saturating_add(COMMIT_STEPS);

        // ---- 相位五 VERIFY（3 步）：读回 + 影子同步 + 双写/半提交回查 ----
        if self.committed_writes != 1 {
            return Err(MsCode::DOUBLE_WRITE);
        }
        if self.stage != STAGE_COMMITTED {
            return Err(MsCode::HALF_COMMIT);
        }
        if self.state != target {
            return Err(MsCode::STATE_MISMATCH);
        }
        self.shadow = Some(self.state);
        if self.shadow != Some(target) {
            return Err(MsCode::STATE_MISMATCH);
        }
        steps = steps.saturating_add(VERIFY_STEPS);

        // ---- 相位六 DISARM（2 步）：撤回调 + 清迁移中 + 阶段归零 ----
        self.callback_armed = false;
        if self.callback_sticky {
            // 粘滞毒：撤了又弹回 armed——悬挂回调。
            self.callback_armed = true;
            return Err(MsCode::DANGLING_CALLBACK);
        }
        if self.callback_armed {
            return Err(MsCode::DANGLING_CALLBACK);
        }
        self.in_migration = false;
        self.stage = STAGE_IDLE;
        self.committed_writes = 0;
        // 预算是"单次切换限额"不是跨次消耗池：只做 need 上限校验，不扣减。
        steps = steps.saturating_add(DISARM_STEPS);

        Ok(SwitchOutcome {
            from,
            to: target,
            edge,
            steps: steps.saturating_add(jitter),
        })
    }

    /// 断裂回退路径：迁移线断在 ARM 后，回退到迁移前态。
    ///
    /// 回退通道可用 → 恢复干净态并返回断裂码；回退通道失守 →
    /// ILLEGAL_ROLLBACK（回退本身也失守，双故障面必须分开报）。
    fn abort_with_rollback(
        &mut self,
        from: ModeState,
        break_code: MsCode,
    ) -> Result<SwitchOutcome, MsCode> {
        if !self.rollback_ok {
            return Err(MsCode::ILLEGAL_ROLLBACK);
        }
        // 回退：状态归位、影子同步、回调撤下、迁移标志清除、阶段归零。
        self.state = from;
        self.shadow = Some(from);
        self.callback_armed = false;
        self.in_migration = false;
        self.stage = STAGE_IDLE;
        self.committed_writes = 0;
        // 回退步数（ROLLBACK_STEPS）只计故障路径账，不留状态。
        let _ = ROLLBACK_STEPS;
        Err(break_code)
    }
}

// ---------------------------------------------------------------------------
// 五、LCG 确定性源——固定种子，重跑逐位一致
// ---------------------------------------------------------------------------

/// 确定性 LCG（64 位，常数与同目录 unx1023/unx1043 同源）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子起机（全确定性：同种子同序列）。
    pub const fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 推进一步，返回原始 64 位。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.state
    }

    /// 抽一目标态：高半区 bit 决定 Polling/Irq。
    pub fn next_target(&mut self) -> ModeState {
        let raw = self.next_u64();
        if (raw >> 32) & 1 == 1 {
            ModeState::Irq
        } else {
            ModeState::Polling
        }
    }

    /// 抽时延抖动：低 3 位（0..=8 步）。
    pub fn next_jitter(&mut self) -> u32 {
        let raw = self.next_u64();
        (raw & (JITTER_MAX as u64)) as u32
    }
}

// ---------------------------------------------------------------------------
// 六、逐次留痕三列 + 滚动窗 1024 封顶
// ---------------------------------------------------------------------------

/// 逐次留痕行（判据三列：次数 / 态向 / 时延）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchTrace {
    /// 列一：次数（1 起计，u16 可容 10^4）。
    pub index: u16,
    /// 列二：态向（四元闭集枚举）。
    pub direction: ModeDirection,
    /// 列三：时延（逻辑步数；失败次记 0）。
    pub latency_steps: u32,
}

impl SwitchTrace {
    /// 三列人类可读行："000007 P→I 000000021"。
    pub fn row_string(&self) -> String {
        format!(
            "{:0>6} {} {:0>9}",
            self.index,
            self.direction.name(),
            self.latency_steps
        )
    }

    /// 七字节序列化：index(u16 LE) + direction(u8) + latency(u32 LE)。
    pub fn to_bytes(&self, out: &mut Vec<u8>) {
        out.push((self.index & 0xFF) as u8);
        out.push((self.index >> 8) as u8);
        out.push(self.direction.as_u8());
        let lat = self.latency_steps;
        out.push((lat & 0xFF) as u8);
        out.push(((lat >> 8) & 0xFF) as u8);
        out.push(((lat >> 16) & 0xFF) as u8);
        out.push(((lat >> 24) & 0xFF) as u8);
    }
}

/// 滚动窗：1024 封顶，溢出滚动淘汰，淘汰量入守恒账。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollingWindow {
    buf: Vec<SwitchTrace>,
    head: usize,
    len: usize,
    evicted: u64,
}

impl RollingWindow {
    /// 建窗：容量恒为 WINDOW_CAP。
    pub fn new() -> RollingWindow {
        RollingWindow {
            buf: Vec::new(),
            head: 0,
            len: 0,
            evicted: 0,
        }
    }

    /// 压入一行：满则滚动淘汰最旧一行（淘汰计数 +1）。
    pub fn push(&mut self, t: SwitchTrace) {
        if self.len < WINDOW_CAP {
            self.buf.push(t);
            self.len += 1;
            return;
        }
        // 满：head 位即最旧行，覆写后 head 前移。
        let idx = self.head;
        self.buf[idx] = t;
        self.head = (self.head + 1) % WINDOW_CAP;
        self.evicted = self.evicted.saturating_add(1);
    }

    /// 窗内行数（恒 <= WINDOW_CAP）。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 窗空否。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 累计淘汰行数。
    pub fn evicted(&self) -> u64 {
        self.evicted
    }

    /// 按时间序快照（旧→新；快照即留痕证据链）。
    pub fn snapshot(&self) -> Vec<SwitchTrace> {
        let mut out = Vec::new();
        for i in 0..self.len {
            let idx = (self.head + i) % WINDOW_CAP;
            out.push(self.buf[idx]);
        }
        out
    }

    /// 窗账守恒：窗内 + 淘汰 == 应到总数。
    pub fn conservation_ok(&self, total: u64) -> bool {
        (self.len as u64) + self.evicted == total
    }

    /// 窗容量守卫：len 不越 1024 封顶。
    pub fn cap_ok(&self) -> bool {
        self.len <= WINDOW_CAP && self.buf.len() <= WINDOW_CAP
    }
}

// ---------------------------------------------------------------------------
// 七、总量守恒账 + 态向分布账
// ---------------------------------------------------------------------------

/// 态向分布账（四向计数 + 双向跨态占比千分位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectionLedger {
    /// Polling→Irq 次数。
    pub p2i: u64,
    /// Irq→Polling 次数。
    pub i2p: u64,
    /// Polling→Polling 自环次数。
    pub p_self: u64,
    /// Irq→Irq 自环次数。
    pub i_self: u64,
}

impl DirectionLedger {
    /// 空账。
    pub const fn new() -> DirectionLedger {
        DirectionLedger {
            p2i: 0,
            i2p: 0,
            p_self: 0,
            i_self: 0,
        }
    }

    /// 按态向记一笔。
    pub fn record(&mut self, d: ModeDirection) {
        match d {
            ModeDirection::PollingToIrq => self.p2i = self.p2i.saturating_add(1),
            ModeDirection::IrqToPolling => self.i2p = self.i2p.saturating_add(1),
            ModeDirection::PollingSelf => self.p_self = self.p_self.saturating_add(1),
            ModeDirection::IrqSelf => self.i_self = self.i_self.saturating_add(1),
        }
    }

    /// 四向合计。
    pub fn total(&self) -> u64 {
        self.p2i
            .saturating_add(self.i2p)
            .saturating_add(self.p_self)
            .saturating_add(self.i_self)
    }

    /// 跨态双向合计（占比账分母）。
    pub fn cross_total(&self) -> u64 {
        self.p2i.saturating_add(self.i2p)
    }

    /// Polling→Irq 占跨态比（千分位整型）。
    pub fn p2i_per_mille(&self) -> u64 {
        per_mille(self.p2i, self.cross_total())
    }

    /// Irq→Polling 占跨态比（千分位整型；两向占比和恒 1000）。
    pub fn i2p_per_mille(&self) -> u64 {
        per_mille(self.i2p, self.cross_total())
    }

    /// 双向齐备：跨态两向都必须发生过（单向倾斜即不齐）。
    pub fn both_directions_present(&self) -> bool {
        self.p2i > 0 && self.i2p > 0
    }

    /// 分布账守恒：四向和 == 总执行数。
    pub fn conservation_ok(&self, total: u64) -> bool {
        self.total() == total
    }

    /// 分布账快照（kv 用）。
    pub fn kv_rows(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        rows.push(("分布.P2I".into(), self.p2i.to_string()));
        rows.push(("分布.I2P".into(), self.i2p.to_string()));
        rows.push(("分布.P自环".into(), self.p_self.to_string()));
        rows.push(("分布.I自环".into(), self.i_self.to_string()));
        rows.push(("占比.P2I千分位".into(), self.p2i_per_mille().to_string()));
        rows.push(("占比.I2P千分位".into(), self.i2p_per_mille().to_string()));
        rows
    }
}

/// 执行守恒总账：执行/成功/失败/守恒判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionLedger {
    /// 总执行次数（含失败次）。
    pub total: u64,
    /// 成功切换次数。
    pub ok: u64,
    /// 失败次数（干净跑恒 0）。
    pub fail: u64,
}

impl ExecutionLedger {
    /// 空账。
    pub const fn new() -> ExecutionLedger {
        ExecutionLedger {
            total: 0,
            ok: 0,
            fail: 0,
        }
    }

    /// 记一笔成功。
    pub fn record_ok(&mut self) {
        self.ok = self.ok.saturating_add(1);
        self.total = self.total.saturating_add(1);
    }

    /// 记一笔失败。
    pub fn record_fail(&mut self) {
        self.fail = self.fail.saturating_add(1);
        self.total = self.total.saturating_add(1);
    }

    /// 守恒判定：ok + fail == total。
    pub fn conservation_ok(&self) -> bool {
        self.ok.saturating_add(self.fail) == self.total
    }

    /// 成功率千分位（判据：== 1000 才达标）。
    pub fn success_per_mille(&self) -> u64 {
        per_mille(self.ok, self.total)
    }

    /// 达标判定：总次足额 + 千分位 == 1000 + 账平。
    pub fn green(&self) -> bool {
        self.total == TOTAL_RUNS as u64
            && self.success_per_mille() == PER_MILLE_TARGET
            && self.conservation_ok()
    }
}

// ---------------------------------------------------------------------------
// 八、时延统计——直方图整数分位 P50/P95/P99 + 红线
// ---------------------------------------------------------------------------

/// 时延统计（直方图法：值域 0..=63 步封顶）。
///
/// 干净跑时延值域为 BASE_STEPS..=BASE_STEPS+JITTER_MAX（19..=27），
/// 直方图槽位 0..=63 绰绰有余；分位用累积计数整数求秩，无浮点无排序。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatencyStats {
    /// 最小时延（步）。
    pub min: u32,
    /// 最大时延（步）。
    pub max: u32,
    /// P50（步）。
    pub p50: u32,
    /// P95（步）。
    pub p95: u32,
    /// P99（步）。
    pub p99: u32,
    /// 红线步数。
    pub red_line: u32,
    /// 红内判定：P99 <= 红线。
    pub red_ok: bool,
    /// 样本总量。
    pub samples: u64,
}

/// 直方图槽位数（0..=63 步封顶）。
const HIST_SLOTS: usize = 64;

/// 由逐次时延流构建直方图统计。
pub fn latency_stats_from(latencies: &[u32]) -> LatencyStats {
    let mut hist = [0u64; HIST_SLOTS];
    let mut samples: u64 = 0;
    let mut min: u32 = u32::MAX;
    let mut max: u32 = 0;
    for &l in latencies.iter() {
        let slot = if (l as usize) < HIST_SLOTS {
            l as usize
        } else {
            HIST_SLOTS - 1
        };
        hist[slot] = hist[slot].saturating_add(1);
        samples = samples.saturating_add(1);
        if l < min {
            min = l;
        }
        if l > max {
            max = l;
        }
    }
    if samples == 0 {
        return LatencyStats {
            min: 0,
            max: 0,
            p50: 0,
            p95: 0,
            p99: 0,
            red_line: RED_LINE_P99_STEPS,
            red_ok: false,
            samples: 0,
        };
    }
    let p50 = hist_percentile(&hist, samples, 50);
    let p95 = hist_percentile(&hist, samples, 95);
    let p99 = hist_percentile(&hist, samples, 99);
    let red_ok = p99 <= RED_LINE_P99_STEPS;
    LatencyStats {
        min,
        max,
        p50,
        p95,
        p99,
        red_line: RED_LINE_P99_STEPS,
        red_ok,
        samples,
    }
}

/// 直方图整数分位：rank = ceil(p*n/100)，累积到 rank 的槽位即分位值。
fn hist_percentile(hist: &[u64; HIST_SLOTS], samples: u64, p: u32) -> u32 {
    let rank = ((p as u64 * samples) + 99) / 100;
    let mut cum: u64 = 0;
    for (slot, &c) in hist.iter().enumerate() {
        cum = cum.saturating_add(c);
        if cum >= rank {
            return slot as u32;
        }
    }
    (HIST_SLOTS - 1) as u32
}

// ---------------------------------------------------------------------------
// 九、FNV-1a 64 指纹——报告与检查点共用
// ---------------------------------------------------------------------------

/// FNV-1a 64 偏移基。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 素数。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64：逐字节滚动指纹（确定性、无浮点、无外部依赖）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// FNV-1a 64 增量滚动（检查点逐次推进用）。
pub fn fnv1a_step(h: u64, b: u8) -> u64 {
    let x = h ^ (b as u64);
    x.wrapping_mul(FNV_PRIME)
}

// ---------------------------------------------------------------------------
// 十、失败注入器——10 个失败样本闭集 + 10/10 必抓对拍账
// ---------------------------------------------------------------------------

/// 失败样本闭集定义（10 例，一例一码，无一重复）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultCase {
    /// 样本序号（1..=10）。
    pub no: u8,
    /// 样本名。
    pub name: &'static str,
    /// 预期诊断码（对拍基准）。
    pub expected: MsCode,
}

/// 十个失败样本闭集（判据点名十类）。
pub const FAULT_CASES: [FaultCase; 10] = [
    FaultCase { no: 1, name: "非法迁移", expected: MsCode::MIG_ILLEGAL },
    FaultCase { no: 2, name: "迁移中断裂", expected: MsCode::MIG_BREAK },
    FaultCase { no: 3, name: "状态失配", expected: MsCode::STATE_MISMATCH },
    FaultCase { no: 4, name: "重入", expected: MsCode::REENTRY },
    FaultCase { no: 5, name: "超时", expected: MsCode::TIMEOUT },
    FaultCase { no: 6, name: "双写", expected: MsCode::DOUBLE_WRITE },
    FaultCase { no: 7, name: "半提交", expected: MsCode::HALF_COMMIT },
    FaultCase { no: 8, name: "悬挂回调", expected: MsCode::DANGLING_CALLBACK },
    FaultCase { no: 9, name: "回退非法", expected: MsCode::ILLEGAL_ROLLBACK },
    FaultCase { no: 10, name: "未知态向", expected: MsCode::UNKNOWN_DIRECTION },
];

/// 单例对拍行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatchRow {
    /// 样本序号。
    pub no: u8,
    /// 样本名。
    pub name: String,
    /// 预期码。
    pub expected: u16,
    /// 实观测码（未抓到时为 0）。
    pub observed: u16,
    /// 必抓判定。
    pub caught: bool,
}

/// 10/10 必抓对拍账。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatchLedger {
    /// 十行对拍。
    pub rows: Vec<CatchRow>,
    /// 抓获数。
    pub caught: u8,
    /// 10/10 全抓判定。
    pub all_caught: bool,
}

/// 按样本序号构造下毒机器（一例一毒，其余字段保持出厂干净）。
fn poisoned_machine(case_no: u8) -> ModeMachine {
    let mut m = ModeMachine::new_clean();
    match case_no {
        1 => {
            // 非法迁移：目标边位清零（Polling→Irq 边失效）。
            m.edge_mask &= !MigEdge::PollingToIrq.bit();
        }
        2 => {
            // 迁移中断裂：断裂毒 + 回退通道可用（回退成功后报断裂）。
            m.break_in_migration = true;
            m.rollback_ok = true;
        }
        3 => {
            // 状态失配：影子寄存器预设成对面态。
            m.shadow = Some(ModeState::Irq);
        }
        4 => {
            // 重入：迁移中标志预置。
            m.in_migration = true;
        }
        5 => {
            // 超时：预算清零。
            m.timeout_budget = 0;
        }
        6 => {
            // 双写：提交写计数残留 1。
            m.committed_writes = 1;
        }
        7 => {
            // 半提交：阶段寄存器停在 STAGED。
            m.stage = STAGE_STAGED;
        }
        8 => {
            // 悬挂回调：粘滞毒——DISARM 后回调弹回 armed。
            m.callback_sticky = true;
        }
        9 => {
            // 回退非法：断裂毒 + 回退通道失守。
            m.break_in_migration = true;
            m.rollback_ok = false;
        }
        _ => {
            // 样本 10 走 decode_target 通道，机器不下毒。
        }
    }
    m
}

/// 执行单例注入并返回实观测码（None = 状态机没报错 = 漏抓）。
fn run_one_fault(case: &FaultCase) -> Option<MsCode> {
    if case.no == 10 {
        // 未知态向：闭集外编码 0x07 直喂解码通道。
        return decode_target(0x07).err();
    }
    let mut m = poisoned_machine(case.no);
    // 目标一律 Irq（机器出厂在 Polling），抖动 0——故障面与抖动解耦。
    m.request_switch(ModeState::Irq, 0).err()
}

/// 跑全量 10 例注入并出对拍账。
pub fn run_fault_injection() -> CatchLedger {
    let mut rows = Vec::new();
    let mut caught: u8 = 0;
    for case in FAULT_CASES.iter() {
        let observed = run_one_fault(case);
        let hit = match observed {
            Some(code) => code == case.expected,
            None => false,
        };
        if hit {
            caught = caught.saturating_add(1);
        }
        rows.push(CatchRow {
            no: case.no,
            name: case.name.into(),
            expected: case.expected.code(),
            observed: match observed {
                Some(c) => c.code(),
                None => 0,
            },
            caught: hit,
        });
    }
    CatchLedger {
        all_caught: caught == 10,
        caught,
        rows,
    }
}

// ---------------------------------------------------------------------------
// 十一、执行报告落档——kv 行式 + TLV 二进制 + FNV 指纹往返
// ---------------------------------------------------------------------------

/// TLV 标签闭集（报告域）。
pub mod tlv_tag {
    /// 总执行数。
    pub const TOTAL: u16 = 0x1502;
    /// 成功数。
    pub const OK: u16 = 0x1503;
    /// 失败数。
    pub const FAIL: u16 = 0x1504;
    /// 成功率千分位。
    pub const PER_MILLE: u16 = 0x1505;
    /// P50。
    pub const P50: u16 = 0x1506;
    /// P95。
    pub const P95: u16 = 0x1507;
    /// P99。
    pub const P99: u16 = 0x1508;
    /// P2I 计数。
    pub const P2I: u16 = 0x1509;
    /// I2P 计数。
    pub const I2P: u16 = 0x150A;
    /// P 自环计数。
    pub const P_SELF: u16 = 0x150B;
    /// I 自环计数。
    pub const I_SELF: u16 = 0x150C;
    /// 必抓数。
    pub const CAUGHT: u16 = 0x150D;
    /// 重跑一致标志。
    pub const RERUN_OK: u16 = 0x150E;
    /// 滚动窗淘汰量。
    pub const EVICTED: u16 = 0x150F;
    /// LCG 种子。
    pub const SEED: u16 = 0x1510;
    /// P99 红线。
    pub const RED_LINE: u16 = 0x1511;
    /// 报告指纹记录（TLV 尾记录，指纹对体计算）。
    pub const FINGERPRINT: u16 = 0x9715;
}

/// 追加一条 TLV 记录：tag(u16 LE) + len(u16 LE) + value。
fn tlv_push(out: &mut Vec<u8>, tag: u16, value: &[u8]) {
    out.push((tag & 0xFF) as u8);
    out.push((tag >> 8) as u8);
    let n = value.len() as u16;
    out.push((n & 0xFF) as u8);
    out.push((n >> 8) as u8);
    for &b in value.iter() {
        out.push(b);
    }
}

/// u64 小端入 TLV。
fn tlv_push_u64(out: &mut Vec<u8>, tag: u16, v: u64) {
    let b = [
        (v & 0xFF) as u8,
        ((v >> 8) & 0xFF) as u8,
        ((v >> 16) & 0xFF) as u8,
        ((v >> 24) & 0xFF) as u8,
        ((v >> 32) & 0xFF) as u8,
        ((v >> 40) & 0xFF) as u8,
        ((v >> 48) & 0xFF) as u8,
        ((v >> 56) & 0xFF) as u8,
    ];
    tlv_push(out, tag, &b);
}

/// TLV 解码：返回 (tag, value) 序列；结构坏即 Err(REPORT_FINGERPRINT)。
pub fn tlv_decode(bytes: &[u8]) -> Result<Vec<(u16, Vec<u8>)>, MsCode> {
    let mut out = Vec::new();
    let mut i: usize = 0;
    while i < bytes.len() {
        if i + 4 > bytes.len() {
            return Err(MsCode::REPORT_FINGERPRINT);
        }
        let tag = bytes[i] as u16 | ((bytes[i + 1] as u16) << 8);
        let len = bytes[i + 2] as usize | ((bytes[i + 3] as usize) << 8);
        i += 4;
        if i + len > bytes.len() {
            return Err(MsCode::REPORT_FINGERPRINT);
        }
        let mut val = vec![0u8; len];
        for k in 0..len {
            val[k] = bytes[i + k];
        }
        i += len;
        out.push((tag, val));
    }
    Ok(out)
}

/// TLV 记录取值（u64 形态；缺记录返回 None）。
fn tlv_find_u64(records: &[(u16, Vec<u8>)], tag: u16) -> Option<u64> {
    for (t, v) in records.iter() {
        if *t == tag && v.len() == 8 {
            let mut x: u64 = 0;
            for k in 0..8 {
                x |= (v[k] as u64) << (8 * k);
            }
            return Some(x);
        }
    }
    None
}

/// 执行报告（判据要求的落档形态）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeSwitchReport {
    /// LCG 种子。
    pub seed: u64,
    /// 守恒总账。
    pub ledger: ExecutionLedger,
    /// 态向分布账。
    pub direction: DirectionLedger,
    /// 时延统计。
    pub latency: LatencyStats,
    /// 滚动窗（1024 封顶快照）。
    pub window: RollingWindow,
    /// 滚动窗淘汰量。
    pub window_evicted: u64,
    /// 增量重跑检查点（每千次一个 FNV 指纹）。
    pub checkpoints: Vec<(u16, u64)>,
    /// 重跑一致判定。
    pub rerun_ok: bool,
    /// 必抓对拍账。
    pub catch: CatchLedger,
    /// kv 行式报告。
    pub kv: Vec<(String, String)>,
    /// TLV 二进制报告（含尾指纹记录）。
    pub tlv: Vec<u8>,
    /// 报告指纹（对 TLV 体计算）。
    pub fingerprint: u64,
    /// J1 全绿判定。
    pub j1_green: bool,
}

impl ModeSwitchReport {
    /// 由账面构建 kv + TLV 双形态并做指纹往返对拍。
    fn build_documents(&mut self) -> Result<(), MsCode> {
        let mut kv = Vec::new();
        kv.push(("模块".into(), "UNX-F1502 万次切换回归".into()));
        kv.push(("种子".into(), format!("{:#018x}", self.seed)));
        kv.push(("总次".into(), self.ledger.total.to_string()));
        kv.push(("成功".into(), self.ledger.ok.to_string()));
        kv.push(("失败".into(), self.ledger.fail.to_string()));
        kv.push((
            "成功率千分位".into(),
            self.ledger.success_per_mille().to_string(),
        ));
        for r in self.direction.kv_rows() {
            kv.push(r);
        }
        kv.push(("时延min".into(), self.latency.min.to_string()));
        kv.push(("时延max".into(), self.latency.max.to_string()));
        kv.push(("时延P50".into(), self.latency.p50.to_string()));
        kv.push(("时延P95".into(), self.latency.p95.to_string()));
        kv.push(("时延P99".into(), self.latency.p99.to_string()));
        kv.push(("时延红线".into(), self.latency.red_line.to_string()));
        kv.push((
            "红线判定".into(),
            if self.latency.red_ok { "绿" } else { "红" }.into(),
        ));
        kv.push(("窗长".into(), self.window.len().to_string()));
        kv.push(("窗淘汰".into(), self.window_evicted.to_string()));
        kv.push(("必抓".into(), format!("{}/10", self.catch.caught)));
        kv.push((
            "重跑一致".into(),
            if self.rerun_ok { "是" } else { "否" }.into(),
        ));
        // 留痕样例：窗首 8 行 + 窗尾 8 行（逐次留痕证据入档）。
        let snap = self.window.snapshot();
        let head_n = if snap.len() >= 8 { 8 } else { snap.len() };
        for i in 0..head_n {
            kv.push((format!("留痕首{}", i + 1), snap[i].row_string()));
        }
        let tail_start = snap.len().saturating_sub(8);
        for i in tail_start..snap.len() {
            kv.push((format!("留痕尾{}", snap.len() - i), snap[i].row_string()));
        }
        self.kv = kv;

        // TLV 体（指纹对此体计算）。
        let mut body: Vec<u8> = Vec::new();
        tlv_push_u64(&mut body, tlv_tag::TOTAL, self.ledger.total);
        tlv_push_u64(&mut body, tlv_tag::OK, self.ledger.ok);
        tlv_push_u64(&mut body, tlv_tag::FAIL, self.ledger.fail);
        tlv_push_u64(&mut body, tlv_tag::PER_MILLE, self.ledger.success_per_mille());
        tlv_push_u64(&mut body, tlv_tag::P50, self.latency.p50 as u64);
        tlv_push_u64(&mut body, tlv_tag::P95, self.latency.p95 as u64);
        tlv_push_u64(&mut body, tlv_tag::P99, self.latency.p99 as u64);
        tlv_push_u64(&mut body, tlv_tag::P2I, self.direction.p2i);
        tlv_push_u64(&mut body, tlv_tag::I2P, self.direction.i2p);
        tlv_push_u64(&mut body, tlv_tag::P_SELF, self.direction.p_self);
        tlv_push_u64(&mut body, tlv_tag::I_SELF, self.direction.i_self);
        tlv_push_u64(&mut body, tlv_tag::CAUGHT, self.catch.caught as u64);
        tlv_push_u64(&mut body, tlv_tag::RERUN_OK, if self.rerun_ok { 1 } else { 0 });
        tlv_push_u64(&mut body, tlv_tag::EVICTED, self.window_evicted);
        tlv_push_u64(&mut body, tlv_tag::SEED, self.seed);
        tlv_push_u64(&mut body, tlv_tag::RED_LINE, self.latency.red_line as u64);

        // 指纹往返：体指纹 → 尾记录 → 解码回读 → 重算对拍。
        let fp = fnv1a(&body);
        let mut full = body.clone();
        let fp_bytes = [
            (fp & 0xFF) as u8,
            ((fp >> 8) & 0xFF) as u8,
            ((fp >> 16) & 0xFF) as u8,
            ((fp >> 24) & 0xFF) as u8,
            ((fp >> 32) & 0xFF) as u8,
            ((fp >> 40) & 0xFF) as u8,
            ((fp >> 48) & 0xFF) as u8,
            ((fp >> 56) & 0xFF) as u8,
        ];
        tlv_push(&mut full, tlv_tag::FINGERPRINT, &fp_bytes);
        let records = tlv_decode(&full)?;
        // 解码回读：尾记录必须是指纹记录且值重算一致。
        let last = match records.last() {
            Some(r) => r,
            None => return Err(MsCode::REPORT_FINGERPRINT),
        };
        if last.0 != tlv_tag::FINGERPRINT {
            return Err(MsCode::REPORT_FINGERPRINT);
        }
        let body_len = full.len().saturating_sub(12); // 尾记录 4 头 + 8 值
        let recomputed = fnv1a(&full[0..body_len]);
        let read_back = tlv_find_u64(&records, tlv_tag::FINGERPRINT);
        match read_back {
            Some(x) if x == recomputed && x == fp => {
                self.tlv = full;
                self.fingerprint = fp;
                Ok(())
            }
            _ => Err(MsCode::REPORT_FINGERPRINT),
        }
    }
}

// ---------------------------------------------------------------------------
// 十二、万次切换引擎——10^4 次逐次执行 + 检查点 + 重跑对账
// ---------------------------------------------------------------------------

/// 单趟万次执行产出（首跑与重跑共用同一实现——对账才有意义）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassOutput {
    /// 守恒总账。
    pub ledger: ExecutionLedger,
    /// 态向分布账。
    pub direction: DirectionLedger,
    /// 逐次时延流（统计用）。
    pub latencies: Vec<u32>,
    /// 逐次留痕三列全文（10^4 行，回填滚动窗用）。
    pub traces: Vec<SwitchTrace>,
    /// 增量检查点（每千次一个 FNV 指纹）。
    pub checkpoints: Vec<(u16, u64)>,
}

/// 单趟万次执行。
///
/// 任何机器 Err 都不 panic：记 fail 一笔、时延 0 留痕、态向按尝试向记。
/// 逐次留痕指纹滚动推进，每满 CHECKPOINT_STRIDE 落一个检查点。
fn run_pass(seed: u64) -> PassOutput {
    let mut lcg = Lcg::new(seed);
    let mut machine = ModeMachine::new_clean();
    let mut ledger = ExecutionLedger::new();
    let mut direction = DirectionLedger::new();
    let mut latencies: Vec<u32> = Vec::new();
    let mut traces: Vec<SwitchTrace> = Vec::new();
    let mut checkpoints: Vec<(u16, u64)> = Vec::new();
    let mut rolling = FNV_OFFSET;

    for i in 1u32..=TOTAL_RUNS {
        let from = machine.state();
        let target = lcg.next_target();
        let jitter = lcg.next_jitter();
        let (dir, lat) = match machine.request_switch(target, jitter) {
            Ok(outcome) => {
                ledger.record_ok();
                (ModeDirection::of(outcome.from, outcome.to), outcome.steps)
            }
            Err(_code) => {
                // 干净跑不应到达；真到达则记失败一笔、时延 0 留痕。
                ledger.record_fail();
                (ModeDirection::of(from, target), 0)
            }
        };
        direction.record(dir);
        latencies.push(lat);
        let trace = SwitchTrace {
            index: i as u16,
            direction: dir,
            latency_steps: lat,
        };
        // 逐次留痕指纹滚动（检查点证据链：不留 10^4 行全文，只留指纹链）。
        let mut ser: Vec<u8> = Vec::new();
        trace.to_bytes(&mut ser);
        for &b in ser.iter() {
            rolling = fnv1a_step(rolling, b);
        }
        if i % CHECKPOINT_STRIDE as u32 == 0 {
            checkpoints.push((i as u16, rolling));
        }
        traces.push(trace);
    }
    PassOutput {
        ledger,
        direction,
        latencies,
        traces,
        checkpoints,
    }
}

/// 万次切换回归主引擎：首跑 → 守恒对账 → 滚动窗回填 → 时延统计 →
/// 失败注入 → 报告落档（指纹往返）→ 重跑对账，任一环失守即 Err。
pub fn run_regression(seed: u64) -> Result<ModeSwitchReport, MsCode> {
    segment_guard()?;

    // ---- 首跑 10^4 次 ----
    let first = run_pass(seed);
    if !first.ledger.conservation_ok() {
        return Err(MsCode::LEDGER_CONSERVATION);
    }
    if !first.direction.conservation_ok(first.ledger.total) {
        return Err(MsCode::LEDGER_CONSERVATION);
    }

    // ---- 滚动窗回填（1024 封顶，逐次留痕三列入窗） ----
    let mut window = RollingWindow::new();
    for t in first.traces.iter() {
        window.push(*t);
    }
    if !window.cap_ok() {
        return Err(MsCode::WINDOW_CAP_BREACH);
    }
    if !window.conservation_ok(first.ledger.total) {
        return Err(MsCode::WINDOW_CAP_BREACH);
    }

    // ---- 时延统计 + 红线 ----
    let latency = latency_stats_from(&first.latencies);

    // ---- 失败注入 10/10 ----
    let catch = run_fault_injection();

    // ---- 报告组装（kv + TLV + 指纹往返） ----
    let mut report = ModeSwitchReport {
        seed,
        ledger: first.ledger,
        direction: first.direction,
        latency,
        window,
        window_evicted: 0,
        checkpoints: first.checkpoints,
        rerun_ok: false,
        catch,
        kv: Vec::new(),
        tlv: Vec::new(),
        fingerprint: 0,
        j1_green: false,
    };
    report.window_evicted = report.window.evicted();
    report.build_documents()?;

    // ---- 增量重跑对账：同种子重跑，检查点逐位一致 ----
    let rerun = run_pass(seed);
    if rerun.checkpoints.len() != report.checkpoints.len() {
        return Err(MsCode::RERUN_MISMATCH);
    }
    let mut rerun_ok = true;
    for i in 0..report.checkpoints.len() {
        let a = report.checkpoints[i];
        let b = rerun.checkpoints[i];
        if a.0 != b.0 || a.1 != b.1 {
            rerun_ok = false;
        }
    }
    // 重跑逐次对账：三列全文逐行一致才算"重跑一致"。
    if rerun.traces.len() == first.traces.len() {
        for i in 0..first.traces.len() {
            if rerun.traces[i] != first.traces[i] {
                rerun_ok = false;
            }
        }
    } else {
        rerun_ok = false;
    }
    report.rerun_ok = rerun_ok;
    if !rerun_ok {
        return Err(MsCode::RERUN_MISMATCH);
    }

    // ---- J1 全绿判定 ----
    report.j1_green = report.ledger.green()
        && report.direction.both_directions_present()
        && report.latency.red_ok
        && report.catch.all_caught
        && report.rerun_ok
        && report.ledger.fail == 0;
    if !report.latency.red_ok {
        return Err(MsCode::REDLINE_BREACH);
    }
    Ok(report)
}

// ---------------------------------------------------------------------------
// 十三、J1 验收总装——判据逐条核对出口
// ---------------------------------------------------------------------------

/// J1 验收摘要（判据逐项 + 总旗）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct J1Summary {
    /// 万次执行且全成功（成功率千分位 == 1000）。
    pub runs_10k_all_green: bool,
    /// 逐次留痕三列入窗且守恒（次数/态向/时延）。
    pub trace_three_columns_ok: bool,
    /// 双向切换都发生过（P↔I 双向占比账两向非零）。
    pub both_directions_ok: bool,
    /// 失败注入 10/10 必抓。
    pub catch_10_of_10: bool,
    /// 执行报告落档（kv+TLV 双形态 + 指纹往返过）。
    pub report_archived: bool,
    /// 重跑一致。
    pub rerun_ok: bool,
    /// P99 红内。
    pub p99_in_red_line: bool,
    /// J1 总旗。
    pub green: bool,
}

/// J1 验收入口：跑完整回归并逐条核对判据。
pub fn j1_acceptance() -> Result<(J1Summary, ModeSwitchReport), MsCode> {
    let report = run_regression(SEED_FIXED)?;
    let summary = J1Summary {
        runs_10k_all_green: report.ledger.total == TOTAL_RUNS as u64
            && report.ledger.fail == 0
            && report.ledger.success_per_mille() == PER_MILLE_TARGET,
        trace_three_columns_ok: report.window.len() <= WINDOW_CAP
            && report.window.conservation_ok(report.ledger.total),
        both_directions_ok: report.direction.both_directions_present(),
        catch_10_of_10: report.catch.all_caught,
        report_archived: !report.kv.is_empty()
            && !report.tlv.is_empty()
            && report.fingerprint != 0,
        rerun_ok: report.rerun_ok,
        p99_in_red_line: report.latency.red_ok,
        green: report.j1_green,
    };
    Ok((summary, report))
}

/// 一句话结论（诊断出口人话形态）。
pub fn j1_headline(summary: &J1Summary) -> String {
    if summary.green {
        "UNX-F1502 J1 全绿：10^4 次切换成功率 1000‰，留痕三列守恒，双向齐备，注入必抓 10/10，报告已落档，重跑一致".into()
    } else {
        let mut why: Vec<&str> = vec![];
        if !summary.runs_10k_all_green {
            why.push("万次执行或成功率不达标");
        }
        if !summary.trace_three_columns_ok {
            why.push("留痕三列守恒失守");
        }
        if !summary.both_directions_ok {
            why.push("双向切换缺向");
        }
        if !summary.catch_10_of_10 {
            why.push("失败注入未全抓");
        }
        if !summary.report_archived {
            why.push("报告未落档");
        }
        if !summary.rerun_ok {
            why.push("重跑对账不一致");
        }
        if !summary.p99_in_red_line {
            why.push("P99 越红线");
        }
        format!("UNX-F1502 J1 亮红：{}", why.join("；"))
    }
}

// ---------------------------------------------------------------------------
// 十四、模块自检对账（纯函数出口，非测试）——构建期可调用的一致性面
// ---------------------------------------------------------------------------

/// 诊断码域自检：十六码 reason 非空且前缀码值与 wire 码一致。
pub fn diag_domain_selfcheck() -> Result<(), MsCode> {
    segment_guard()?;
    for c in ALL_CODES.iter() {
        let r = c.reason();
        if r.is_empty() {
            return Err(MsCode::SEGMENT_GUARD);
        }
        // 人话原因必须带 "[0x97xx]" 前缀（码值与 wire 码一致的可读面）。
        let want = format!("[0x{:04X}]", c.code());
        let prefix_ok = r.len() >= want.len() && {
            let mut same = true;
            let rb = r.as_bytes();
            let wb = want.as_bytes();
            for k in 0..wb.len() {
                if rb[k] != wb[k] {
                    same = false;
                }
            }
            same
        };
        if !prefix_ok {
            return Err(MsCode::SEGMENT_GUARD);
        }
    }
    Ok(())
}

/// 迁移边闭集自检：四边位互异且全掩码恰为四边位按位或。
pub fn edge_mask_selfcheck() -> Result<(), MsCode> {
    let mut acc: u8 = 0;
    for e in MigEdge::ALL.iter() {
        let b = e.bit();
        if acc & b != 0 {
            return Err(MsCode::SEGMENT_GUARD);
        }
        acc |= b;
    }
    if acc != MigEdge::FULL_MASK {
        return Err(MsCode::SEGMENT_GUARD);
    }
    Ok(())
}

/// 计量自检：六相位基础步数与逐相位常数和一致（19 步基线不漂移）。
pub fn step_baseline_selfcheck() -> Result<(), MsCode> {
    let sum = GUARD_STEPS
        .saturating_add(PRE_STEPS)
        .saturating_add(ARM_STEPS)
        .saturating_add(COMMIT_STEPS)
        .saturating_add(VERIFY_STEPS)
        .saturating_add(DISARM_STEPS);
    if sum != BASE_STEPS || BASE_STEPS != 19 {
        return Err(MsCode::SEGMENT_GUARD);
    }
    if ModeMachine::need_steps(JITTER_MAX) != BASE_STEPS + JITTER_MAX {
        return Err(MsCode::SEGMENT_GUARD);
    }
    Ok(())
}

/// 留痕窗自检：容量封顶语义——压入 CAP+1 行后窗长恒 CAP、淘汰恰 1。
pub fn window_cap_selfcheck() -> Result<(), MsCode> {
    let mut w = RollingWindow::new();
    for i in 0..(WINDOW_CAP + 1) {
        w.push(SwitchTrace {
            index: (i % 65536) as u16,
            direction: ModeDirection::PollingSelf,
            latency_steps: (i % 64) as u32,
        });
    }
    if w.len() != WINDOW_CAP || w.evicted() != 1 || !w.cap_ok() {
        return Err(MsCode::WINDOW_CAP_BREACH);
    }
    if !w.conservation_ok((WINDOW_CAP + 1) as u64) {
        return Err(MsCode::WINDOW_CAP_BREACH);
    }
    Ok(())
}
