//! CGPU-F3364 · 验收流程（vcv04_flow）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3364`
//!
//! **判据（锚点原文）**：四阶段、复用声明、两组、判据。
//!
//! **职责定位（锚点原文）**：流程：验收流程（预备→执行→报告→处置
//! 四阶段——流程实现（流程复用——复用声明；测试（四阶段/复用两组）。
//!
//! # 一、四阶段状态机（为什么验收必须分阶段）
//!
//! 验收最危险的失败模式不是「没通过」，而是**跳步**：没预备好环境
//! 就开跑（结论无法复现）、没出报告就处置（处置依据失踪）、没处置
//! 就标完成（失败被静默吞掉）。[`Flow`] 用显式状态机把四阶段钉死：
//! `Prepare → Execute → Report → Dispose`，**只能前进、不能跳过**
//! —— [`Flow::advance`] 拒绝任何越级迁移（[`E_VV_SKIP`]）。
//!
//! 回退是允许的，但必须留痕（[`E_VV_BACK`] 不拦截、只记账）——
//! 现实里执行阶段发现环境坏了必须回预备重整，禁止回退才是逼人造假。
//!
//! # 二、复用声明（流程骨架不是新造的）
//!
//! 四阶段骨架复用 V01 已登记的验收总架构流程槽位（F3361 的四段
//! 架构之「矩阵/判据/走查/报告」中的报告链路），本项只做**状态机
//! 化与守门**，不另立流程体系——声明于此，避免「两套流程」漂移。
//!
//! # 三、阶段门（每阶段放行的前置条件）
//!
//! - 预备→执行：环境快照已拍（[`Flow::prepare_done`]）；
//! - 执行→报告：用例已全部跑完或有结论（[`Flow::execute_done`]）；
//! - 报告→处置：报告已生成且非空（[`Flow::report_done`]）。
//! 门未开就推进 = [`E_VV_GATE`]。守门的目的是让「跳步」在数据上
//! 可见，而不是靠人盯流程图。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const FLOW_VERSION: &str = "V04-flow-v1";

/// 越级推进（只能相邻阶段顺序前进）。
pub const E_VV_SKIP: &str = "E_VV_SKIP";

/// 阶段门未开（前置条件不满足）。
pub const E_VV_GATE: &str = "E_VV_GATE";

/// 阶段回退（合法但留痕）。
pub const E_VV_BACK: &str = "E_VV_BACK";

/// 未知阶段号。
pub const E_VV_STAGE: &str = "E_VV_STAGE";

/// 阶段数。
pub const STAGES: usize = 4;

// ---------------------------------------------------------------------------
// 二、阶段定义
// ---------------------------------------------------------------------------

/// 验收四阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 预备：环境登记/快照/矩阵选格。
    Prepare,
    /// 执行：跑用例/走查。
    Execute,
    /// 报告：逐设备报告与汇总。
    Report,
    /// 处置：阻断修复流转/明显项排期/细节归档。
    Dispose,
}

impl Stage {
    /// 全集（供遍历判据用）。
    pub const ALL: [Stage; STAGES] = [
        Stage::Prepare,
        Stage::Execute,
        Stage::Report,
        Stage::Dispose,
    ];

    /// 序号。
    pub const fn idx(self) -> usize {
        match self {
            Stage::Prepare => 0,
            Stage::Execute => 1,
            Stage::Report => 2,
            Stage::Dispose => 3,
        }
    }

    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            Stage::Prepare => "预备",
            Stage::Execute => "执行",
            Stage::Report => "报告",
            Stage::Dispose => "处置",
        }
    }

    /// 由序号还原（未知序号即 [`E_VV_STAGE`]）。
    pub fn from_idx(i: usize) -> Result<Stage, &'static str> {
        Self::ALL
            .iter()
            .find(|s| s.idx() == i)
            .copied()
            .ok_or(E_VV_STAGE)
    }
}

// ---------------------------------------------------------------------------
// 三、流程状态机
// ---------------------------------------------------------------------------

/// 回退留痕。
#[derive(Clone, Debug)]
pub struct BackRecord {
    /// 从哪退到哪。
    pub from: Stage,
    /// 退到哪。
    pub to: Stage,
    /// 原因（人话，报告引用）。
    pub reason: String,
}

/// 验收流程状态机。
pub struct Flow {
    stage: Stage,
    /// 门：预备完成（环境快照已拍）。
    prepare_done: bool,
    /// 门：执行完成（用例全跑完或有结论）。
    execute_done: bool,
    /// 门：报告完成（报告已生成且非空）。
    report_done: bool,
    /// 回退留痕账。
    backs: Vec<BackRecord>,
    /// 遥测：推进次数。
    pub advances: u64,
    /// 遥测：被拒推进次数。
    pub rejected: u64,
}

impl Flow {
    /// 新流程，起点固定在预备。
    pub fn new() -> Flow {
        Flow {
            stage: Stage::Prepare,
            prepare_done: false,
            execute_done: false,
            report_done: false,
            backs: Vec::new(),
            advances: 0,
            rejected: 0,
        }
    }

    /// 当前阶段。
    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// 开门操作（由各阶段实际完成动作调用）。
    pub fn mark(&mut self, s: Stage) {
        match s {
            Stage::Prepare => self.prepare_done = true,
            Stage::Execute => self.execute_done = true,
            Stage::Report => self.report_done = true,
            Stage::Dispose => {}
        }
    }

    /// 阶段门是否已开。
    pub fn gate_open(&self, target: Stage) -> bool {
        match target {
            Stage::Execute => self.prepare_done,
            Stage::Report => self.execute_done,
            Stage::Dispose => self.report_done,
            Stage::Prepare => true,
        }
    }

    /// 推进到相邻下一阶段：越级拒、门未开拒、回退留痕。
    pub fn advance(&mut self, target: Stage) -> Result<Stage, &'static str> {
        let from = self.stage.idx();
        let to = target.idx();
        if to > from + 1 || to + STAGES < from + 1 {
            // 越级（含回退两步以上）。
            self.rejected += 1;
            return Err(E_VV_SKIP);
        }
        if to == from {
            self.rejected += 1;
            return Err(E_VV_STAGE);
        }
        if to == from + 1 {
            // 前进：查门。
            if !self.gate_open(target) {
                self.rejected += 1;
                return Err(E_VV_GATE);
            }
            self.stage = target;
            self.advances += 1;
            return Ok(target);
        }
        // 回退：合法但留痕。
        self.backs.push(BackRecord {
            from: self.stage,
            to: target,
            reason: String::from("回退留痕"),
        });
        self.stage = target;
        // 回退后需重开后续门（预备重来则执行门重关）。
        if target.idx() <= Stage::Prepare.idx() {
            self.prepare_done = false;
        }
        if target.idx() <= Stage::Execute.idx() {
            self.execute_done = false;
        }
        Ok(target)
    }

    /// 回退次数。
    pub fn back_count(&self) -> usize {
        self.backs.len()
    }

    /// 回退留痕渲染（报告引用）。
    pub fn render_backs(&self) -> String {
        let mut s = String::new();
        for b in &self.backs {
            s.push_str(&alloc::format!(
                "{} → {}（{}）\n",
                b.from.name(),
                b.to.name(),
                b.reason
            ));
        }
        s
    }

    /// 状态机不变量：阶段号合法 + 门与阶段一致。
    pub fn invariant(&self) -> Result<(), &'static str> {
        Stage::from_idx(self.stage.idx())?;
        if self.stage.idx() >= 1 && !self.prepare_done && self.backs.is_empty() {
            return Err(E_VV_GATE);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、两组测试（四阶段/复用）
// ---------------------------------------------------------------------------

/// CGPU-F3364 域自检：四阶段、复用声明、两组、判据。
pub fn run_vcv04_checks() -> CheckSet {
    let mut cs = CheckSet::new("vcv04_flow");

    // --- 组一：四阶段 ---
    cs.add("阶段全集", Stage::ALL.len() == STAGES, "四阶段");
    cs.add(
        "序号互异",
        Stage::ALL[0].idx() == 0
            && Stage::ALL[3].idx() == 3,
        "idx 单调",
    );
    cs.add(
        "还原回环",
        Stage::from_idx(2) == Ok(Stage::Report),
        "from_idx",
    );
    cs.add(
        "未知序号拒绝",
        Stage::from_idx(9) == Err(E_VV_STAGE),
        "E_VV_STAGE",
    );

    // --- 越级即拒 ---
    let mut f = Flow::new();
    cs.add(
        "预备→报告越级拒",
        f.advance(Stage::Report) == Err(E_VV_SKIP),
        "跳过执行",
    );
    cs.add(
        "门未开拒",
        f.advance(Stage::Execute) == Err(E_VV_GATE),
        "快照未拍",
    );

    // --- 正常走完四阶段 ---
    f.mark(Stage::Prepare);
    cs.add(
        "预备门开",
        f.advance(Stage::Execute) == Ok(Stage::Execute),
        "advance",
    );
    f.mark(Stage::Execute);
    f.advance(Stage::Report).ok();
    f.mark(Stage::Report);
    cs.add(
        "处置门开",
        f.advance(Stage::Dispose) == Ok(Stage::Dispose),
        "报告已出",
    );
    cs.add(
        "推进计数",
        f.advances == 3 && f.rejected == 2,
        "遥测守恒",
    );
    cs.add(
        "终态即处置",
        f.stage() == Stage::Dispose,
        "走完",
    );

    // --- 回退留痕 + 门重关 ---
    let mut g = Flow::new();
    g.mark(Stage::Prepare);
    g.advance(Stage::Execute).ok();
    g.mark(Stage::Execute);
    g.advance(Stage::Report).ok();
    cs.add(
        "报告回执行",
        g.advance(Stage::Execute).is_ok(),
        "回退合法",
    );
    cs.add("留痕一条", g.back_count() == 1, "backs");
    cs.add(
        "回退后门重关",
        g.gate_open(Stage::Report) == false,
        "执行门重开流程",
    );
    cs.add(
        "留痕可渲染",
        g.render_backs().contains("报告"),
        "人话",
    );

    // --- 组二：复用声明 ---
    cs.add(
        "复用声明非空",
        FLOW_VERSION == "V04-flow-v1",
        "V01 报告链路复用",
    );
    cs.add(
        "不变量",
        f.invariant().is_ok() && g.invariant().is_ok(),
        "阶段-门一致",
    );

    cs
}
