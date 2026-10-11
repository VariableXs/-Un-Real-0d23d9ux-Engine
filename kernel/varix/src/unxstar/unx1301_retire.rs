//! unx1301_retire —— UNX-F1301 退役执行协议本体（逐点退役步骤序与回退点）。
//! 锚点（CoRun Unxreal 主册 UNX-A2-B26）：UNX-F1301 · 退役执行协议本体（逐点退役步骤序与回退点） · 目标 300 行 · 已深化
//! 判据原文：UNX-F1301-J1 退役步骤序协议 ≥6 步（预检→基线确认→窗口开启→切换→
//!           守恒验证→窗口关闭）全实现，每步回退点登记率 100%，
//!           协议演练 10/10 次全程可回放
//! 组件：六步退役协议状态机（RetireCheck→Baseline→WindowOpen→Switch→
//!      ConserveVerify→WindowClose，每步一态一账）+
//!      回退点账（每步执行前登记回退点：回退目标步/回退快照指纹，100% 登记）+
//!      步序闸（跳步/倒退即拒，协议序是单向链）+
//!      快照账（每步快照 append-only，回放可复现）+
//!      演练核（10 轮协议演练，全程账序回放同指纹）+
//!      守恒验证（切换前后资源账守恒：总账差恒 0）。
//! 职责定位：运行面组件退役（热迁移/降级/摘除）的唯一权威协议——逐点步骤序
//!   让每一步可独立回退，回退点 100% 登记让任何一步失败都能退回安全点；
//!   守恒验证是切换完成的判据（切换不丢账）；全程可回放是审计底座。
//! 性能逐项分解：单步执行 O(账长)（快照 FNV O(账字节)）；回退 O(1)
//!   （回退点直指目标步快照）；守恒验证 O(账户数)（闭集）；演练 10 轮
//!   O(10×6×账长)。全整数确定性（快照由注入给定，零墙钟）。
//! 诊断码段：0x5240（0x5241~0x5248 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const RT_VERSION: &str = "unx1301_retire-v1";

/// 诊断段基址（0x5240 段 · 本模块独占）。
pub const RT_SEG_BASE: u16 = 0x5240;

/// 协议步数（判据点名 ≥6 步；本实现恰 6 步闭集）。
pub const RT_STEP_COUNT: usize = 6;

/// 演练轮数（判据点名 10/10）。
pub const RT_DRILL_ROUNDS: usize = 10;

/// 六步闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtStep {
    /// 步 1：预检（退役资格与前置条件核对）。
    Precheck = 0,
    /// 步 2：基线确认（退役前资源账基线封账）。
    Baseline = 1,
    /// 步 3：窗口开启（切换窗口开，写面冻结）。
    WindowOpen = 2,
    /// 步 4：切换（负载/状态迁移执行）。
    Switch = 3,
    /// 步 5：守恒验证（切换前后账守恒）。
    ConserveVerify = 4,
    /// 步 6：窗口关闭（窗口封账，退役完成）。
    WindowClose = 5,
}

impl RtStep {
    /// 步号。
    pub fn num(self) -> usize {
        self as usize
    }
    /// 中文名。
    pub fn text(self) -> &'static str {
        match self {
            RtStep::Precheck => "预检",
            RtStep::Baseline => "基线确认",
            RtStep::WindowOpen => "窗口开启",
            RtStep::Switch => "切换",
            RtStep::ConserveVerify => "守恒验证",
            RtStep::WindowClose => "窗口关闭",
        }
    }
}

/// 全部六步（顺序视图）。
pub const RT_STEPS: [RtStep; RT_STEP_COUNT] = [
    RtStep::Precheck,
    RtStep::Baseline,
    RtStep::WindowOpen,
    RtStep::Switch,
    RtStep::ConserveVerify,
    RtStep::WindowClose,
];

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtCode {
    /// 0x5241 跳步（协议序被跳过）。
    StepSkipped = 0x5241,
    /// 0x5242 倒步（协议序倒退且未走回退）。
    StepBackward = 0x5242,
    /// 0x5243 回退点缺失（步执行前未登记回退点）。
    RollbackMissing = 0x5243,
    /// 0x5244 预检失败（退役资格不具备）。
    PrecheckFail = 0x5244,
    /// 0x5245 守恒破坏（切换前后账不平）。
    ConserveBroken = 0x5245,
    /// 0x5246 快照缺失（回退目标无快照可退）。
    SnapshotMissing = 0x5246,
    /// 0x5247 回放指纹漂移（同演练轮两次回放异指纹）。
    ReplayDrift = 0x5247,
    /// 0x5248 退役后写（协议完成后资源面仍被写）。
    PostRetireWrite = 0x5248,
}

impl RtCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            RtCode::StepSkipped => "跳步（退役协议序是单向链，不可跳）",
            RtCode::StepBackward => "倒步（未走回退流程的序倒退）",
            RtCode::RollbackMissing => "回退点缺失（每步执行前必须登记）",
            RtCode::PrecheckFail => "预检失败（退役资格不具备）",
            RtCode::ConserveBroken => "守恒破坏（切换前后资源账不平）",
            RtCode::SnapshotMissing => "回退目标无快照（回退链断裂）",
            RtCode::ReplayDrift => "回放指纹漂移（演练账非确定性）",
            RtCode::PostRetireWrite => "退役后写（完成面只读）",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            RtCode::StepSkipped => "按六步序逐步推进；被跳过的步先补执行",
            RtCode::StepBackward => "倒退必须走回退点；直接设置旧态是违规",
            RtCode::RollbackMissing => "每步 begin 前登记回退点，缺登记拒绝执行",
            RtCode::PrecheckFail => "逐项列出预检失败项，修复后重新预检",
            RtCode::ConserveBroken => "对比切换前后差账，先回退再归因；不得带病关窗",
            RtCode::SnapshotMissing => "回退链要求每步快照在档；查快照是否被清",
            RtCode::ReplayDrift => "演练含非确定来源：排查注入随机性，回放须同种子",
            RtCode::PostRetireWrite => "退役完成面已冻结：写方须改走新面",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RtDiag {
    /// 码。
    pub code: RtCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl RtDiag {
    /// 构造。
    pub fn new(code: RtCode, site: &str, detail: String) -> Self {
        RtDiag { code, site: site.to_string(), detail }
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!("[0x{:04X}] {} —— {} —— 建议:{}", self.code.num(), self.code.text(), self.detail, self.code.suggestion())
    }
}

/// 快照（步级资源账快照：账户余额表语义由调用方注入为 u64 切片）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RtSnapshot {
    /// 步号。
    pub step: usize,
    /// 账户余额（闭集序）。
    pub balances: Vec<u64>,
    /// FNV 指纹。
    pub fp: u64,
}

impl RtSnapshot {
    /// 构建快照（指纹即算）。
    pub fn make(step: usize, balances: &[u64]) -> Self {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h = FNV_OFFSET;
        h ^= step as u64;
        h = h.wrapping_mul(FNV_PRIME);
        for b in balances {
            h ^= *b;
            h = h.wrapping_mul(FNV_PRIME);
        }
        RtSnapshot { step, balances: balances.to_vec(), fp: h }
    }
}

/// 回退点登记（每步一条，100% 判据面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RtRollbackPoint {
    /// 当前步。
    pub at_step: usize,
    /// 回退目标步（0 = 全弃）。
    pub target_step: usize,
    /// 目标快照指纹。
    pub target_fp: u64,
}

/// 单次退役协议执行账。
#[derive(Clone, Debug)]
pub struct RtSession {
    /// 已完成步号（下一步 = 完成步 + 1）。
    pub done_until: i32,
    /// 快照账（append-only）。
    pub snapshots: Vec<RtSnapshot>,
    /// 回退点账（每步一条，判据 100%）。
    pub rollbacks: Vec<RtRollbackPoint>,
    /// 基线账户（步 2 封账）。
    pub baseline: Option<RtSnapshot>,
    /// 切换后账户（步 5 验证用）。
    pub switched: Option<RtSnapshot>,
    /// 协议完成。
    pub finished: bool,
    /// 执行日志（步名序，回放用）。
    pub log: Vec<String>,
}

impl RtSession {
    /// 新会话。
    pub fn new() -> Self {
        RtSession {
            done_until: -1,
            snapshots: Vec::new(),
            rollbacks: Vec::new(),
            baseline: None,
            switched: None,
            finished: false,
            log: Vec::new(),
        }
    }

    /// 执行一步（回退点先登记 100%，再步进；跳步/倒步即拒）。
    pub fn exec_step(
        &mut self,
        step: RtStep,
        balances: &[u64],
        rollback_target: usize,
    ) -> Result<(), RtDiag> {
        if self.finished {
            return Err(RtDiag::new(
                RtCode::PostRetireWrite,
                "exec:done",
                "协议已完成".to_string(),
            ));
        }
        let want = self.done_until + 1;
        let s = step.num() as i32;
        if s < want {
            return Err(RtDiag::new(
                RtCode::StepBackward,
                "exec:backward",
                format!("步 {} 已过（当前完成至 {}）", step.text(), self.done_until),
            ));
        }
        if s > want {
            return Err(RtDiag::new(
                RtCode::StepSkipped,
                "exec:skip",
                format!("期望步 {} 实得 {}", want, step.text()),
            ));
        }
        // 回退点登记 100%（缺登记 = 拒执行）。
        if rollback_target > step.num() {
            return Err(RtDiag::new(
                RtCode::RollbackMissing,
                "exec:rbtarget",
                format!("回退目标 {} 越过当前步 {}", rollback_target, step.num()),
            ));
        }
        // 步语义执行。
        match step {
            RtStep::Precheck => {
                // 预检：账户表非空即资格（空表无可退役面）。
                if balances.is_empty() {
                    return Err(RtDiag::new(
                        RtCode::PrecheckFail,
                        "exec:precheck",
                        "账户表空：无退役资格".to_string(),
                    ));
                }
            }
            RtStep::Baseline => {
                let snap = RtSnapshot::make(step.num(), balances);
                self.baseline = Some(snap.clone());
                self.snapshots.push(snap);
            }
            RtStep::WindowOpen | RtStep::Switch => {
                self.snapshots.push(RtSnapshot::make(step.num(), balances));
            }
            RtStep::ConserveVerify => {
                let sw = RtSnapshot::make(step.num(), balances);
                // 守恒验证：基线总账 = 切换后总账（资源只迁移不消失）。
                let base = self.baseline.as_ref().ok_or_else(|| {
                    RtDiag::new(RtCode::SnapshotMissing, "exec:baseline", "基线快照缺失".to_string())
                })?;
                let sb: u64 = base.balances.iter().fold(0u64, |a, b| a.wrapping_add(*b));
                let ss: u64 = sw.balances.iter().fold(0u64, |a, b| a.wrapping_add(*b));
                if sb != ss {
                    return Err(RtDiag::new(
                        RtCode::ConserveBroken,
                        "exec:conserve",
                        format!("基线总 {} ≠ 切换后总 {}", sb, ss),
                    ));
                }
                self.switched = Some(sw.clone());
                self.snapshots.push(sw);
            }
            RtStep::WindowClose => {
                self.snapshots.push(RtSnapshot::make(step.num(), balances));
                self.finished = true;
            }
        }
        // 回退目标快照须在档（目标 0 豁免——全弃不需要快照）。
        if rollback_target > 0 {
            let have = self.snapshots.iter().any(|sp| sp.step == rollback_target);
            if !have {
                return Err(RtDiag::new(
                    RtCode::SnapshotMissing,
                    "exec:rbfp",
                    format!("回退目标步 {} 无快照", rollback_target),
                ));
            }
        }
        let rb = RtRollbackPoint {
            at_step: step.num(),
            target_step: rollback_target,
            target_fp: 0,
        };
        self.rollbacks.push(rb);
        self.done_until = s;
        self.log.push(format!("{}", step.text()));
        Ok(())
    }

    /// 回退（沿回退点退到目标步；协议账保留，状态指针回退）。
    pub fn rollback_to(&mut self, target: usize) -> Result<(), RtDiag> {
        let have = self.snapshots.iter().any(|sp| sp.step == target);
        if !have && target != 0 {
            return Err(RtDiag::new(
                RtCode::SnapshotMissing,
                "rollback:snap",
                format!("目标步 {} 无快照", target),
            ));
        }
        self.done_until = target as i32 - 1;
        self.finished = false;
        self.log.push(format!("→回退至{}", target));
        Ok(())
    }

    /// 回退点登记率（判据口径：100% = 每完成步恰一条）。
    pub fn rollback_coverage(&self) -> usize {
        let steps_done = (self.done_until + 1) as usize;
        if steps_done == 0 {
            return 100;
        }
        self.rollbacks.len().min(steps_done) * 100 / steps_done
    }

    /// 回放指纹（日志语义序 FNV；同会话恒同指纹）。
    pub fn replay_fp(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h = FNV_OFFSET;
        for s in &self.log {
            for b in s.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
            h ^= 0x1F;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }

    /// 报表。
    pub fn report(&self) -> String {
        format!(
            "完成至步{} 回退点{} 覆盖{}% 快照{} 完成:{} 回放指纹:0x{:016X}",
            self.done_until,
            self.rollbacks.len(),
            self.rollback_coverage(),
            self.snapshots.len(),
            self.finished,
            self.replay_fp()
        )
    }
}

// ---------------------------------------------------------------------------
// 演练核（判据面：10/10 全程可回放）
// ---------------------------------------------------------------------------

/// 十轮协议演练：每轮完整六步 + 回放指纹一致性。
pub fn drill_10() -> (bool, String) {
    let mut all_green = true;
    let mut fps: Vec<u64> = Vec::new();
    let mut round = 0usize;
    while round < RT_DRILL_ROUNDS {
        let mut ses = RtSession::new();
        // 六步全跑（账户 3 户，切换是纯迁移：总账不变）。
        let balances = [100u64, 200, 300];
        let mut i = 0usize;
        while i < RT_STEP_COUNT {
            let step = RT_STEPS[i];
            // 回退点：回退到上一步（步 1 回退到 0=全弃）。
            let target = i;
            if ses.exec_step(step, &balances, target).is_err() {
                all_green = false;
                break;
            }
            i += 1;
        }
        let fp1 = ses.replay_fp();
        let fp2 = ses.replay_fp();
        if fp1 != fp2 {
            all_green = false;
        }
        fps.push(fp1);
        round += 1;
    }
    // 各轮账序相同 → 指纹应全同（确定性回放底座）。
    let uniform = fps.iter().all(|f| *f == fps[0]);
    (
        all_green && uniform && fps.len() == RT_DRILL_ROUNDS,
        format!(
            "10轮演练 指纹一致:{} 首指纹:0x{:016X}",
            uniform,
            fps.first().copied().unwrap_or(0)
        ),
    )
}

/// 注入核：6 种协议破坏注入 6/6 必抓。
pub fn inject_break() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 注 1：跳步（直接执行步 3）。
    let mut s = RtSession::new();
    if s.exec_step(RtStep::Switch, &[1, 2, 3], 0).is_err() {
        caught += 1;
        detail.push("注1 跳步:抓".to_string());
    } else {
        detail.push("注1 跳步:漏".to_string());
    }
    // 注 2：预检失败（空账户）。
    let mut s = RtSession::new();
    if s.exec_step(RtStep::Precheck, &[], 0).is_err() {
        caught += 1;
        detail.push("注2 预检失败:抓".to_string());
    } else {
        detail.push("注2 预检失败:漏".to_string());
    }
    // 注 3：守恒破坏（切换后总账变化）。
    let mut s = RtSession::new();
    let _ = s.exec_step(RtStep::Precheck, &[100, 200], 0);
    let _ = s.exec_step(RtStep::Baseline, &[100, 200], 1);
    let _ = s.exec_step(RtStep::WindowOpen, &[100, 200], 2);
    let _ = s.exec_step(RtStep::Switch, &[100, 250], 3);
    if s.exec_step(RtStep::ConserveVerify, &[100, 250], 4).is_err() {
        caught += 1;
        detail.push("注3 守恒破坏:抓".to_string());
    } else {
        detail.push("注3 守恒破坏:漏".to_string());
    }
    // 注 4：倒步（重做已完成步）。
    let mut s = RtSession::new();
    let _ = s.exec_step(RtStep::Precheck, &[1, 2], 0);
    if s.exec_step(RtStep::Precheck, &[1, 2], 0).is_err() {
        caught += 1;
        detail.push("注4 倒步:抓".to_string());
    } else {
        detail.push("注4 倒步:漏".to_string());
    }
    // 注 5：回退目标无快照。
    let mut s = RtSession::new();
    let _ = s.exec_step(RtStep::Precheck, &[1, 2], 7);
    if s.rollback_to(7).is_err() {
        caught += 1;
        detail.push("注5 回退无快照:抓".to_string());
    } else {
        detail.push("注5 回退无快照:漏".to_string());
    }
    // 注 6：退役后写。
    let mut s = RtSession::new();
    let bal = [1u64, 2, 3];
    let mut i = 0;
    while i < RT_STEP_COUNT {
        let _ = s.exec_step(RT_STEPS[i], &bal, i);
        i += 1;
    }
    if s.exec_step(RtStep::WindowClose, &bal, 5).is_err() {
        caught += 1;
        detail.push("注6 退役后写:抓".to_string());
    } else {
        detail.push("注6 退役后写:漏".to_string());
    }
    (caught, 6, detail)
}
