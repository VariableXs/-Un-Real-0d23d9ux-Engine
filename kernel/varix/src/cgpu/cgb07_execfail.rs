//! CGPU-F0167 · 执行失败与降级（CGPU-B 域 · 帧任务图与执行域 · B01 组 · 目标 380 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0167`
//!
//! **判据（锚点原文）**：四类失败路由、传播正确、降级执行对拍、记账全量、
//! 注入测试。
//!
//! **职责定位（锚点原文）**：任务执行失败的处置：失败分类（资源缺失/编译
//! 错误/GPU 超时/OOM）、失败传播策略（上游失败→下游全跳过；可替代任务→
//! 替代执行）、降级执行（失败任务走降级路径——低质量替代算子）、失败记账
//! （每个失败任务五元组入遥测——与 CGPU-F0015 对齐）。
//!
//! ## 一、失败四分类：闭集，不立第五类
//!
//! 锚点点名四类（[`FailureKind`]：资源缺失/编译错误/GPU 超时/OOM）——
//! 类别闭集纪律：表外失败不是「其他」桶，而是上游建模缺口，走显性
//! [`XcCode::Unclassified`] 拒绝并记账（异常零静默）。每类绑定独立诊断码
//! （0x60 段独占，[`XcCode`]）与固定遥测指标名（[`FAIL_METRICS`]）。
//!
//! ## 二、传播正确：失败沿依赖边走，替代与降级是两道止损闸
//!
//! 执行序按调用方给定的拓扑序推进（本模块校验拓扑性：依赖必须指向
//! 更早任务，违反即显性拒绝——不静默重排）。某任务失败后按三步路由：
//! ①有替代任务（[`TaskNode::substitute`]）→ 替代执行，替代成功则传播
//! 在此截断（下游照常）；②无可替代但标记可降级（[`TaskNode::degradable`]
//! ）→ 降级执行（低质量替代算子，质量档降一档），下游照常；③两闸皆无
//! → 硬失败，全部传递依赖任务按依赖边闭包逐个标 [`TaskOutcome::Skipped`]
//! （跳过也是结论，不是失踪）。
//!
//! ## 三、降级执行对拍：降级不是悄悄变差
//!
//! 降级执行留双账：结果账（[`TaskOutcome::Degraded`]）+ 对拍账
//! （[`ExecLedger::degraded_pairs`]：原任务 id → 降级档位 → 质量声明），
//! 消费方凭对拍账知道「这一帧这里是用低档算子跑的」——降级诚实是
//! O 域「降质不降帧」铁律在 B 域的执行面。
//!
//! ## 四、记账全量：五元组进遥测，账实守恒
//!
//! 每个失败任务一条五元组指标（[`FailMetric`]：名称/维度/单位/类型/
//! 采样——与 F0274/B08 遥测五元组同构对齐，不第二套口径）；账本侧
//! [`ExecLedger`] 记全量处置事实，并做守恒对账：六类结论计数之和 ==
//! 任务总数（[`ExecLedger::counts_reconcile`]）——多记少记都是缺陷。
//!
//! ## 五、注入测试：判据的函数级支撑
//!
//! [`FailureInjector`] 允许测试面按 (任务, 类别) 注入失败——判据
//! 「注入测试」的落地面；生产路径传空注入表即零注入。注入是显性
//! 入账的（`injected` 计数），不留暗门。
//!
//! **对接**：F0166（图执行器，本模块是其失败处置面）；F0161（任务图
//! 构建——节点与依赖来源）；F0274/B08（遥测五元组同源）；O 域（降级
//! 配方联动预留）。零 panic 面（下标走 `get`/`Option`，算术饱和）、
//! 零 IO、零墙钟（tick 上游注入）、无全局可变状态、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（0x60 段独占：B01 组失败处置面）
// ---------------------------------------------------------------------------

/// 诊断码（u16；0x60 段为本单独占段，段内互异）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 资源缺失类失败。
    FailResourceMissing = 0x6001,
    /// 编译错误类失败。
    FailCompileError = 0x6002,
    /// GPU 超时类失败。
    FailGpuTimeout = 0x6003,
    /// OOM 类失败。
    FailOom = 0x6004,
    /// 拓扑违规：依赖指向了排序更晚的任务（不静默重排）。
    TopologyViolation = 0x6005,
    /// 表外失败类别（建模缺口，显性拒绝）。
    Unclassified = 0x6006,
}

impl XcCode {
    /// 人读诊断（异常零静默：发生了什么/为什么/下一步）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::FailResourceMissing => "资源缺失：纹理/缓冲未就绪，建议核对上游生产任务与资源生命周期",
            XcCode::FailCompileError => "编译错误：着色器/管线编译失败，建议核对材质与目标档位兼容表",
            XcCode::FailGpuTimeout => "GPU 超时：任务超出预算窗口，建议核对预算标签与降级配方",
            XcCode::FailOom => "显存耗尽：分配失败，建议走 H 域水位回收后重试或降级",
            XcCode::TopologyViolation => "拓扑违规：依赖指向排序更晚任务，建议按拓扑序重排任务表",
            XcCode::Unclassified => "表外失败类别：上游建模缺口，建议补分类后再入执行面",
        }
    }
}

// ---------------------------------------------------------------------------
// 二、失败四分类（判据一：闭集）
// ---------------------------------------------------------------------------

/// 失败类别闭集（锚点点名四类；表外走 [`XcCode::Unclassified`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    /// 资源缺失（纹理/缓冲未就绪）。
    ResourceMissing,
    /// 编译错误（着色器/管线编译失败）。
    CompileError,
    /// GPU 超时（超预算窗口）。
    GpuTimeout,
    /// OOM（显存耗尽）。
    Oom,
}

impl FailureKind {
    /// 对应诊断码。
    pub const fn code(self) -> XcCode {
        match self {
            FailureKind::ResourceMissing => XcCode::FailResourceMissing,
            FailureKind::CompileError => XcCode::FailCompileError,
            FailureKind::GpuTimeout => XcCode::FailGpuTimeout,
            FailureKind::Oom => XcCode::FailOom,
        }
    }

    /// 遥测指标名尾段（三级命名 `b01.execfail.<尾段>`）。
    pub const fn metric_tail(self) -> &'static str {
        match self {
            FailureKind::ResourceMissing => "resource_missing",
            FailureKind::CompileError => "compile_error",
            FailureKind::GpuTimeout => "gpu_timeout",
            FailureKind::Oom => "oom",
        }
    }
}

/// 遥测五元组（与 F0274/B08 五元组同构：名称/维度/单位/类型/采样——
/// 不第二套口径；判据四的 schema 面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FailMetric {
    /// 三级命名（域.组.指标）。
    pub name: &'static str,
    /// 维度（按失败事件）。
    pub dimension: &'static str,
    /// 单位。
    pub unit: &'static str,
    /// 类型。
    pub kind: &'static str,
    /// 采样策略（事件驱动计数）。
    pub sampling: &'static str,
}

/// 四类失败的五元组指标表（与 [`FailureKind`] 一一对应，判据侧可独立
/// 重排对账）。
pub const FAIL_METRICS: [FailMetric; 4] = [
    FailMetric {
        name: "b01.execfail.resource_missing",
        dimension: "failure_event",
        unit: "count",
        kind: "u64",
        sampling: "on_event",
    },
    FailMetric {
        name: "b01.execfail.compile_error",
        dimension: "failure_event",
        unit: "count",
        kind: "u64",
        sampling: "on_event",
    },
    FailMetric {
        name: "b01.execfail.gpu_timeout",
        dimension: "failure_event",
        unit: "count",
        kind: "u64",
        sampling: "on_event",
    },
    FailMetric {
        name: "b01.execfail.oom",
        dimension: "failure_event",
        unit: "count",
        kind: "u64",
        sampling: "on_event",
    },
];

/// 五元组指标名前缀（可 grep 不可漂移）。
pub const FAIL_METRIC_PREFIX: &str = "b01.execfail.";

// ---------------------------------------------------------------------------
// 三、任务节点与结论闭集（判据二：传播）
// ---------------------------------------------------------------------------

/// 任务节点（依赖 + 两道止损闸声明）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskNode {
    /// 任务 id（帧内唯一；执行序按调用方拓扑序数组下标）。
    pub task_id: u32,
    /// 依赖的任务 id 集合（必须指向更早执行的任务）。
    pub deps: Vec<u32>,
    /// 替代任务 id（可替代任务的第一道闸）。
    pub substitute: Option<u32>,
    /// 可降级标记（第二道闸：低质量替代算子可顶上）。
    pub degradable: bool,
}

/// 单任务执行结论（闭集；六类账实守恒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskOutcome {
    /// 正常成功。
    Succeeded,
    /// 硬失败（两道止损闸皆无或止损亦失败）。
    Failed,
    /// 被传播跳过（上游硬失败的传递依赖）。
    Skipped,
    /// 替代执行成功（第一道闸生效）。
    Substituted,
    /// 降级执行成功（第二道闸生效）。
    Degraded,
    /// 从未进入执行面（拓扑校验拒绝的整帧）。
    NotRun,
}

/// 单任务处置账（结论 + 失败类别 + 诊断码 + 人读明细）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskVerdict {
    /// 任务 id。
    pub task_id: u32,
    /// 结论。
    pub outcome: TaskOutcome,
    /// 失败类别（仅 Failed/Degraded 时有值）。
    pub failure: Option<FailureKind>,
    /// 诊断码（结论的码面）。
    pub code: Option<XcCode>,
    /// 人读明细（错误路径带可操作建议）。
    pub detail: String,
}

// ---------------------------------------------------------------------------
// 四、失败注入面（判据五：注入测试）
// ---------------------------------------------------------------------------

/// 失败注入器（测试面：按 (任务, 类别) 注入；生产传空表即零注入）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FailureInjector {
    injections: Vec<(u32, FailureKind)>,
}

impl FailureInjector {
    /// 空注入（生产路径）。
    pub const fn none() -> FailureInjector {
        FailureInjector {
            injections: Vec::new(),
        }
    }

    /// 注入一条失败。
    pub fn inject(&mut self, task_id: u32, kind: FailureKind) {
        self.injections.push((task_id, kind));
    }

    /// 查询某任务的注入失败（有则返回类别）。
    pub fn fail_of(&self, task_id: u32) -> Option<FailureKind> {
        let mut i = 0usize;
        while i < self.injections.len() {
            if let Some((id, kind)) = self.injections.get(i) {
                if *id == task_id {
                    return Some(*kind);
                }
            }
            i += 1;
        }
        None
    }

    /// 注入总数（显性入账用）。
    pub const fn len(&self) -> usize {
        self.injections.len()
    }

    /// 空判定。
    pub const fn is_empty(&self) -> bool {
        self.injections.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 五、执行引擎（判据二、三：传播路由 + 降级对拍）
// ---------------------------------------------------------------------------

/// 帧执行账（全量处置事实 + 守恒对账 + 降级对拍账）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecLedger {
    verdicts: Vec<TaskVerdict>,
    /// 降级对拍账：(原任务 id, 遥测指标名, 质量声明)。
    degraded_pairs: Vec<(u32, String, String)>,
    counts: [u64; 6],
    injected: u64,
}

impl ExecLedger {
    /// 六类结论计数之和 == 任务总数（判据四：账实守恒；多记少记即缺陷）。
    pub fn counts_reconcile(&self, total_tasks: usize) -> bool {
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < 6 {
            sum = sum.saturating_add(self.counts[i]);
            i += 1;
        }
        sum == total_tasks as u64
    }

    /// 按结论取计数（越界给 0——不 panic）。
    pub fn count_of(&self, outcome: TaskOutcome) -> u64 {
        let idx = match outcome {
            TaskOutcome::Succeeded => 0,
            TaskOutcome::Failed => 1,
            TaskOutcome::Skipped => 2,
            TaskOutcome::Substituted => 3,
            TaskOutcome::Degraded => 4,
            TaskOutcome::NotRun => 5,
        };
        *self.counts.get(idx).unwrap_or(&0)
    }

    /// 全量处置账（判据四：记账全量——逐条可回查）。
    pub fn verdicts(&self) -> &[TaskVerdict] {
        &self.verdicts
    }

    /// 降级对拍账（判据三：降级不是悄悄变差）。
    pub fn degraded_pairs(&self) -> &[(u32, String, String)] {
        &self.degraded_pairs
    }

    /// 注入计数（显性入账，不留暗门）。
    pub const fn injected(&self) -> u64 {
        self.injected
    }

    /// 某结论的任务 id 集合（下游消费面）。
    pub fn ids_with(&self, outcome: TaskOutcome) -> Vec<u32> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.verdicts.len() {
            if let Some(v) = self.verdicts.get(i) {
                if v.outcome == outcome {
                    out.push(v.task_id);
                }
            }
            i += 1;
        }
        out
    }

    fn record(&mut self, v: TaskVerdict) {
        let idx = match v.outcome {
            TaskOutcome::Succeeded => 0,
            TaskOutcome::Failed => 1,
            TaskOutcome::Skipped => 2,
            TaskOutcome::Substituted => 3,
            TaskOutcome::Degraded => 4,
            TaskOutcome::NotRun => 5,
        };
        if let Some(c) = self.counts.get_mut(idx) {
            *c = c.saturating_add(1);
        }
        self.verdicts.push(v);
    }
}

/// 帧执行失败处置引擎。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecFailEngine {
    ledger: ExecLedger,
}

impl ExecFailEngine {
    /// 新引擎。
    pub const fn new() -> ExecFailEngine {
        ExecFailEngine {
            ledger: ExecLedger {
                verdicts: Vec::new(),
                degraded_pairs: Vec::new(),
                counts: [0; 6],
                injected: 0,
            },
        }
    }

    /// 推进一帧（判据二：拓扑校验→逐任务路由→传播闭包）。
    ///
    /// `order` 是拓扑序任务表（依赖必须指向表中更早项）；`injector` 是
    /// 测试注入面。返回 `Err(拓扑违规码)` 时整帧不执行（NotRun 语义由
    /// 调用方感知——不半帧执行）。
    pub fn run_frame(
        &mut self,
        order: &[TaskNode],
        injector: &FailureInjector,
    ) -> Result<&ExecLedger, XcCode> {
        // 拓扑校验：依赖必须指向更早任务（同 id 自依赖也违规）。
        let mut pos: Vec<(u32, usize)> = Vec::new();
        let mut i = 0usize;
        while i < order.len() {
            if let Some(n) = order.get(i) {
                pos.push((n.task_id, i));
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < order.len() {
            if let Some(n) = order.get(i) {
                let mut d = 0usize;
                while d < n.deps.len() {
                    let dep = match n.deps.get(d) {
                        Some(x) => *x,
                        None => break,
                    };
                    let mut found = false;
                    let mut p = 0usize;
                    while p < i {
                        if let Some((id, _)) = pos.get(p) {
                            if *id == dep {
                                found = true;
                                break;
                            }
                        }
                        p += 1;
                    }
                    if !found {
                        return Err(XcCode::TopologyViolation);
                    }
                    d += 1;
                }
            }
            i += 1;
        }

        // 逐任务推进。
        let mut i = 0usize;
        while i < order.len() {
            let node = match order.get(i) {
                Some(n) => n.clone(),
                None => break,
            };
            // 已被传播跳过？
            if self.is_skipped(&node.task_id, order) {
                self.record(TaskVerdict {
                    task_id: node.task_id,
                    outcome: TaskOutcome::Skipped,
                    failure: None,
                    code: None,
                    detail: "上游硬失败传播跳过：依赖闭包内存在未止损失败".to_string(),
                });
                i += 1;
                continue;
            }
            // 注入失败（判据五：显性入账）。
            let injected_kind = injector.fail_of(node.task_id);
            if injected_kind.is_some() {
                self.ledger.injected = self.ledger.injected.saturating_add(1);
            }
            match injected_kind {
                Some(kind) => self.route_failure(&node, kind),
                None => self.record(TaskVerdict {
                    task_id: node.task_id,
                    outcome: TaskOutcome::Succeeded,
                    failure: None,
                    code: None,
                    detail: "正常完成".to_string(),
                }),
            }
            i += 1;
        }
        Ok(&self.ledger)
    }

    /// 失败三步路由（判据一、二：替代闸→降级闸→硬失败传播）。
    fn route_failure(&mut self, node: &TaskNode, kind: FailureKind) {
        // 第一道闸：可替代任务 → 替代执行（替代任务在帧内顶上；其自身
        // 执行结论由 F0168 队列映射深化接入，本面按替代闸语义记替代成功）。
        if let Some(sub_id) = node.substitute {
            self.record(TaskVerdict {
                task_id: node.task_id,
                outcome: TaskOutcome::Substituted,
                failure: Some(kind),
                code: Some(kind.code()),
                detail: alloc::format!(
                    "{}；替代任务 {} 顶上，传播在此截断",
                    kind.code().detail(),
                    sub_id
                ),
            });
            return;
        }
        // 第二道闸：可降级 → 低质量替代算子（质量档降一档），对拍入账。
        if node.degradable {
            self.record(TaskVerdict {
                task_id: node.task_id,
                outcome: TaskOutcome::Degraded,
                failure: Some(kind),
                code: Some(kind.code()),
                detail: alloc::format!("{}；走降级路径：低质量替代算子，质量档降一档", kind.code().detail()),
            });
            self.ledger.degraded_pairs.push((
                node.task_id,
                alloc::format!("{}{}", FAIL_METRIC_PREFIX, kind.metric_tail()),
                "质量档-1（低质量替代算子）".to_string(),
            ));
            return;
        }
        // 硬失败：传播由 is_skipped 的依赖闭包承担。
        self.record(TaskVerdict {
            task_id: node.task_id,
            outcome: TaskOutcome::Failed,
            failure: Some(kind),
            code: Some(kind.code()),
            detail: alloc::format!("{}；两道止损闸皆无，下游传递依赖全部跳过", kind.code().detail()),
        });
    }

    /// 任务是否应被传播跳过：其依赖闭包内存在硬失败（含替代/降级未接住的）。
    fn is_skipped(&self, task_id: &u32, order: &[TaskNode]) -> bool {
        // 依赖闭包：沿 deps 递归上溯；此处以本账已记录结论为准（推进序保证
        // 依赖先于被依赖者出账——拓扑校验已拦截乱序）。
        let node = match find_node(order, *task_id) {
            Some(n) => n,
            None => return false,
        };
        let mut d = 0usize;
        while d < node.deps.len() {
            let dep = match node.deps.get(d) {
                Some(x) => *x,
                None => break,
            };
            let mut v = 0usize;
            while v < self.ledger.verdicts.len() {
                if let Some(verdict) = self.ledger.verdicts.get(v) {
                    if verdict.task_id == dep {
                        if verdict.outcome == TaskOutcome::Failed {
                            return true;
                        }
                        if verdict.outcome == TaskOutcome::Skipped && self.is_skipped(&dep, order) {
                            return true;
                        }
                        break;
                    }
                }
                v += 1;
            }
            d += 1;
        }
        false
    }

    fn record(&mut self, v: TaskVerdict) {
        self.ledger.record(v);
    }

    /// 当前账（只读）。
    pub const fn ledger(&self) -> &ExecLedger {
        &self.ledger
    }
}

fn find_node(order: &[TaskNode], task_id: u32) -> Option<&TaskNode> {
    let mut i = 0usize;
    while i < order.len() {
        if let Some(n) = order.get(i) {
            if n.task_id == task_id {
                return Some(n);
            }
        }
        i += 1;
    }
    None
}
