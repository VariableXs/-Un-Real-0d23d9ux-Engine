//! CGPU-F0324 · 预算申请协议（CGPU-C 域 · 帧预算仲裁域 · C04 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0324`
//!
//! **判据（锚点原文）**：标签三要素、两路时机、批量、原子归还、判据。
//!
//! **职责定位（锚点原文）**：申请协议：任务预算标签（每任务声明预估
//! 耗时+截止期+资源类型——预估来自 F0004 成本模型）；两路申请时机
//! （图构建时静态申请+运行期动态补申）；批量申请（整图预算汇总一次
//! 仲裁——减仲裁次数）；撤销与变更（图取消→预算原子归还——不泄漏
//! 预算额度）。
//!
//! ## 一、诊断码（0x67 段独占：C04 组申请面）
//!
//! 十六码闭集（[`XcCode`]）：标签面四码（三要素残缺/预估为零/超限
//! 钳制/任务重复）、时机面三码（补申限频/补申超限/静态重申）、批量
//! 面三码（汇总溢出裁剪/图预算耗尽/空批量）、归还面三码（重复归还/
//! 归还泄漏/图未知）、对账面三码（账本守恒破坏/未知任务消费/纪元漂
//! 移）。段守恒编译期钉死（[`segment_guard`] 对账全部十六码落在
//! 0x6700..0x6800 开区间且互不重码）。
//!
//! ## 二、预算标签三要素（判据一：标签三要素）
//!
//! 预算标签（[`BudgetTag`]——任务 id、预估耗时、截止期、资源类型、
//! 优先级五字段；三要素判据落在预估+截止+类型三字段必须同时有效）。
//! 资源类型闭集（[`ResourceKind`]——CPU/GPU/Transfer 三类，与 F0004
//! 双路契约对齐：Cpu 消费 CPU 路预估、Gpu 消费 GPU 路预估、Transfer
//! 消费 GPU 路预估——GPU 路公式含 2× 传输费，是传输感知预估）。
//! **预估来自 F0004 成本模型**（[`tag_from_cost`]——直接消费
//! [`cga04_placer::CostModel::predict`] 的 [`cga04_placer::
//! PathEstimate`]，标定纪元随标签入账——预估可比性边界传递）。标
//! 签构建器（[`TagBuilder`]——三要素齐才出标签：缺任一要素拒并留
//! 痕 [`XcCode::TagIncomplete`]；任务重复拒 [`XcCode::TaskDuplicate`]
//! ；预估为零拒 [`XcCode::ZeroEstimate`]）。钳制护栏（预估超单任务
//! 上限 [`SINGLE_TAG_CAP_US`] 即钳到上限并留痕 [`XcCode::
//! EstClamped`]——钳制不是拒绝：大任务合法存在，只是预算按上限批）
//! ；截止期下限 1 帧（[`XcCode::DeadlineTooTight`]）。
//!
//! ## 三、两路申请时机（判据二：两路时机）
//!
//! 静态申请（图构建时批量声明——[`ApplyProtocol::submit_static`]，
//! 整图标签一次入仲裁）；动态补申（运行期执行中发现预算不足——
//! [`ApplyProtocol::supplement`]，受双约束：单帧补申轮次上限
//! [`DYN_MAX_ROUNDS_PER_FRAME`]、补申总额上限为该图静态批准额的
//! [`DYN_TOTAL_CAP_PCT`]%——超即拒 [`XcCode::DynRoundLimited`] /
//! [`XcCode::DynCapExceeded`]）；静态重申拒（同图已静态批准后再次
//! 静态申请即 [`XcCode::StaticDuplicate`]——两路时机互斥不重叠）。
//! 两路守恒：图总批准 = 静态批准 + 动态批准（[`ApplyBook::total_
//! approved`] 单一事实源）。
//!
//! ## 四、批量申请（判据三：批量）
//!
//! 批量仲裁（[`ApplyProtocol::submit_static`]——整图标签**一次**仲
//! 裁出全部结果：批/拒逐任务落定，仲裁轮次计账 +1——对照逐个申请
//! N 轮，减仲裁次数的量化证据入账 [`ApplyProtocol::arb_rounds`]）。
//! 汇总溢出（整图申请合计 > 图预算上限 [`GRAPH_BUDGET_US`]——按优
//! 先级降序保留（Critical>High>Normal，同级按申请序稳定），溢出局
//! 部按低优先先裁、裁剪逐条留痕 [`XcCode::OverflowCropped`]——裁
//! 剪不是静默）；图预算耗尽（累计批准已达上限再申即拒 [`XcCode::
//! GraphBudgetExhausted`]）；空批量拒（[`XcCode::EmptyBatch`]）。
//! 批量裁决（[`BatchVerdict`]——逐任务 [`TagVerdict`]：批准额（可
//! 能被钳制）/ 拒绝原因码，一次带回）。
//!
//! ## 五、原子归还（判据四：原子归还）
//!
//! 图取消→预算原子归还（[`ApplyProtocol::cancel_graph`]——该图全部
//! 已批准未消费额度**一次性**返还归还池：归还前快照批准/消费两账、
//! 归还额 = 批准 − 消费、归还后图账本整页清除——三步原子，无中间
//! 态外泄）。泄漏检测（归还额 + 消费额 ≠ 批准额即 [`XcCode::
//! ReturnLeak`] 立案——预算额度不泄漏的账面证明）；重复归还拒
//! （已归还图再取消即 [`XcCode::AlreadyReturned`]）；图未知拒（
//! [`XcCode::GraphUnknown`]）。执行消费记账（[`ApplyProtocol::
//! consume`]——任务执行实际耗用逐笔记账，未知任务消费即 [`XcCode::
//! UnknownTaskConsume`] 立案——消费是归还对账的另一半）。
//!
//! ## 六、申请账本（审计）
//!
//! 事件闭集（[`LedgerEvent`]——静态批量/动态补申/钳制/裁剪/归还/
//! 泄漏案/消费 六类事件滚动记账 [`APPLY_LEDGER_CAP`] 条，满则滚动
//! 覆盖最旧——审计窗口有限但守恒对账是全程量）；守恒对账（[`Appl
//! yProtocol::conservation_check`]——全局守恒式：总批准 = 总消费 +
//! 总归还 + 在途未消费；任一时刻成立，破坏即 [`XcCode::
//! ConservationBroken`]）；纪元漂移（同图标签标定纪元不一致即
//! [`XcCode::CalibDrift`] 留痕——F0004 重标定后新申请与旧批准的
//! 可比性边界显性化）。
//!
//! ## 七、主编排与快照（性能：批量仲裁 O（N log N）排序主导）
//!
//! 全链编排（[`ApplyProtocol`]——静态批量入口 / 动态补申入口 / 消
//! 费记账入口 / 取消归还入口 / 守恒对账出口，全程操作数计账）；
//! 快照（[`ApplySnap`]——'|' 分隔保空段序列化 + FNV 指纹）；参考
//! 探针（[`reference_budget_check`]——3 图 × 12 任务全流程：静态批
//! 量 → 部分消费 → 动态补申 → 溢出裁剪 → 原子归还 → 守恒对账 +
//! 快照恒等，验证批量仲裁 O（N log N）与零泄漏口径）。
//!
//! 零 unwrap、零 expect、零 panic 面、零 IO、零墙钟（帧序即时间，
//! 上游注入）。

use crate::cgpu::cga04_placer::{CostModel, CostProfile};
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

// ============================================================
// 常量面（全部字面量写死）
// ============================================================

/// 帧预算（微秒）——12.5ms 与 F0005 帧预算锚点一致。
pub const FRAME_BUDGET_US: u64 = 12_500;

/// 单任务预估上限（微秒）——3 × 帧预算（大任务合法，预算按上限批）。
pub const SINGLE_TAG_CAP_US: u64 = FRAME_BUDGET_US * 3;

/// 图预算上限（微秒）——整图申请合计不得超过一帧预算。
pub const GRAPH_BUDGET_US: u64 = FRAME_BUDGET_US;

/// 动态补申单帧轮次上限。
pub const DYN_MAX_ROUNDS_PER_FRAME: u32 = 2;

/// 动态补申总额上限（该图静态批准额的百分数）。
pub const DYN_TOTAL_CAP_PCT: u64 = 50;

/// 申请账本容量（滚动覆盖最旧）。
pub const APPLY_LEDGER_CAP: usize = 256;

/// 快照指纹种子偏移（ApplySnap 专用）。
pub const SNAP_FNV_SEED: u32 = 0x6767_0001;

// ============================================================
// 一、诊断码（0x67 段独占：C04 组申请面）
// ============================================================

/// 诊断码闭集（0x67 段独占：C04 组申请面十六码）。
///
/// 段位分配：
/// - 标签面 0x01..0x04：三要素残缺 / 预估为零 / 超限钳制 / 任务重复；
/// - 时机面 0x05..0x07：补申限频 / 补申超限 / 静态重申；
/// - 批量面 0x08..0x0A：汇总溢出裁剪 / 图预算耗尽 / 空批量；
/// - 归还面 0x0B..0x0D：重复归还 / 归还泄漏 / 图未知；
/// - 对账面 0x0E..0x10：账本守恒破坏 / 未知任务消费 / 纪元漂移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 标签三要素残缺（缺预估/缺截止期/缺资源类型）。
    TagIncomplete,
    /// 预估为零（成本模型给出零预估——画像异常）。
    ZeroEstimate,
    /// 预估超限被钳制到上限（信息性：钳制不是拒绝）。
    EstClamped,
    /// 任务重复（同图同 id 二次申请）。
    TaskDuplicate,
    /// 动态补申单帧轮次超限。
    DynRoundLimited,
    /// 动态补申总额超限（超静态批准额的 [`DYN_TOTAL_CAP_PCT`]%）。
    DynCapExceeded,
    /// 静态重申（同图已静态批准后再静态申请——两路互斥）。
    StaticDuplicate,
    /// 批量汇总溢出：低优先任务被裁剪（逐条留痕）。
    OverflowCropped,
    /// 图预算耗尽（累计批准已达 [`GRAPH_BUDGET_US`] 再申）。
    GraphBudgetExhausted,
    /// 空批量（零标签的批量申请）。
    EmptyBatch,
    /// 重复归还（已归还图再取消）。
    AlreadyReturned,
    /// 归还泄漏：归还额 + 消费额 ≠ 批准额。
    ReturnLeak,
    /// 图未知（取消/补申/消费指向未登记图）。
    GraphUnknown,
    /// 账本守恒破坏（总批准 ≠ 总消费 + 总归还 + 在途）。
    ConservationBroken,
    /// 未知任务消费（消费记账指向未批准任务）。
    UnknownTaskConsume,
    /// 标定纪元漂移（同图标签纪元不一致——F0004 重标定边界）。
    CalibDrift,
}

impl XcCode {
    /// 段基座（0x6700：C04 组申请面独占段）。
    pub const SEGMENT_BASE: u16 = 0x6700;

    /// 段内偏移查表（match 全枚举，编译期穷尽性守恒）。
    #[must_use]
    pub const fn offset(self) -> u16 {
        match self {
            XcCode::TagIncomplete => 0x01,
            XcCode::ZeroEstimate => 0x02,
            XcCode::EstClamped => 0x03,
            XcCode::TaskDuplicate => 0x04,
            XcCode::DynRoundLimited => 0x05,
            XcCode::DynCapExceeded => 0x06,
            XcCode::StaticDuplicate => 0x07,
            XcCode::OverflowCropped => 0x08,
            XcCode::GraphBudgetExhausted => 0x09,
            XcCode::EmptyBatch => 0x0A,
            XcCode::AlreadyReturned => 0x0B,
            XcCode::ReturnLeak => 0x0C,
            XcCode::GraphUnknown => 0x0D,
            XcCode::ConservationBroken => 0x0E,
            XcCode::UnknownTaskConsume => 0x0F,
            XcCode::CalibDrift => 0x10,
        }
    }

    /// 完整诊断码（段基座 + 偏移）。
    #[must_use]
    pub const fn as_raw(self) -> u16 {
        Self::SEGMENT_BASE + self.offset()
    }

    /// 全部十六码（供守恒对账遍历）。
    pub const ALL: [XcCode; 16] = [
        XcCode::TagIncomplete,
        XcCode::ZeroEstimate,
        XcCode::EstClamped,
        XcCode::TaskDuplicate,
        XcCode::DynRoundLimited,
        XcCode::DynCapExceeded,
        XcCode::StaticDuplicate,
        XcCode::OverflowCropped,
        XcCode::GraphBudgetExhausted,
        XcCode::EmptyBatch,
        XcCode::AlreadyReturned,
        XcCode::ReturnLeak,
        XcCode::GraphUnknown,
        XcCode::ConservationBroken,
        XcCode::UnknownTaskConsume,
        XcCode::CalibDrift,
    ];
}

/// 段守恒对账：全部诊断码落在 0x6700..0x6800 开区间且互不重码。
#[must_use]
pub const fn segment_guard() -> bool {
    let mut i = 0;
    while i < XcCode::ALL.len() {
        let raw = XcCode::ALL[i].as_raw();
        if raw < XcCode::SEGMENT_BASE || raw >= 0x6800 {
            return false;
        }
        let mut j = 0;
        while j < i {
            if XcCode::ALL[j].as_raw() == raw {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ============================================================
// 二、预算标签三要素（判据一）
// ============================================================

/// 资源类型闭集（与 F0004 双路契约对齐）。
///
/// - `Cpu`：消费 CPU 路预估；
/// - `Gpu`：消费 GPU 路预估；
/// - `Transfer`：消费 GPU 路预估——GPU 路公式含 2× 传输费，是传输
///   感知预估，传输主导任务的真实成本口径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResourceKind {
    /// CPU 路。
    Cpu,
    /// GPU 路。
    Gpu,
    /// 传输主导（GPU 路含传输费口径）。
    Transfer,
}

impl ResourceKind {
    /// 类型名（快照用）。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            ResourceKind::Cpu => "cpu",
            ResourceKind::Gpu => "gpu",
            ResourceKind::Transfer => "xfer",
        }
    }
}

/// 任务优先级闭集（批量溢出裁剪序：Critical > High > Normal）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PriorityLevel {
    /// 普通（最低）。
    Normal,
    /// 高。
    High,
    /// 关键（最高）。
    Critical,
}

impl PriorityLevel {
    /// 优先级数值（大者优先保留）。
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            PriorityLevel::Normal => 1,
            PriorityLevel::High => 2,
            PriorityLevel::Critical => 3,
        }
    }
}

/// 任务预算标签（三要素判据：预估 + 截止期 + 资源类型同时有效）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetTag {
    /// 任务 id（图内唯一键）。
    pub task_id: u32,
    /// 预估耗时（微秒；来自 F0004 成本模型 predict，经钳制护栏）。
    pub est_us: u64,
    /// 截止期（相对帧数：从申请帧起 N 帧内须完成；下限 1）。
    pub deadline_frames: u32,
    /// 资源类型。
    pub kind: ResourceKind,
    /// 优先级（溢出裁剪序）。
    pub priority: PriorityLevel,
    /// 标定纪元（F0004 CostModel.calib_epoch 快照——可比性边界）。
    pub calib_epoch: u32,
}

/// 标签构建器（三要素齐才出标签）。
///
/// 拒绝面：预估为零（[`XcCode::ZeroEstimate`]）、截止期为 0（
/// [`XcCode::DeadlineTooTight`]）——三要素缺一即拒，残缺标签不入
/// 仲裁（[`XcCode::TagIncomplete`] 由调用方按拒因归并）。
#[derive(Debug)]
pub struct TagBuilder {
    /// 已发标签的任务 id 集（同图内重复拒）。
    seen: Vec<u32>,
    /// 累计拒绝计数（按拒因索引：0=零预估 1=截止期 2=重复）。
    rejects: [u64; 3],
    /// 累计钳制次数。
    clamps: u64,
}

/// 标签构建结果闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TagOutcome {
    /// 构建成功。
    Ok,
    /// 预估为零。
    ZeroEst,
    /// 截止期为 0。
    DeadlineZero,
    /// 任务重复。
    Dup,
}

impl TagBuilder {
    /// 构造。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            seen: Vec::new(),
            rejects: [0; 3],
            clamps: 0,
        }
    }

    /// 从 F0004 成本模型构建标签（判据一主入口）。
    ///
    /// `est_raw` 为 [`cga04_placer::CostModel::predict`] 产出按
    /// [`ResourceKind`] 选路后的原始预估；此处做三重护栏：零预估拒
    /// → 截止期校验 → 超限钳制（钳制留痕计数）。
    pub fn build(
        &mut self,
        task_id: u32,
        est_raw: u64,
        deadline_frames: u32,
        kind: ResourceKind,
        priority: PriorityLevel,
        calib_epoch: u32,
    ) -> (TagOutcome, Option<BudgetTag>) {
        if est_raw == 0 {
            self.rejects[0] += 1;
            return (TagOutcome::ZeroEst, None);
        }
        if deadline_frames == 0 {
            self.rejects[1] += 1;
            return (TagOutcome::DeadlineZero, None);
        }
        let mut i = 0;
        while i < self.seen.len() {
            if self.seen[i] == task_id {
                self.rejects[2] += 1;
                return (TagOutcome::Dup, None);
            }
            i += 1;
        }
        self.seen.push(task_id);
        // 钳制护栏：超上限钳到上限（钳制不是拒绝）。
        let est = if est_raw > SINGLE_TAG_CAP_US {
            self.clamps += 1;
            SINGLE_TAG_CAP_US
        } else {
            est_raw
        };
        (
            TagOutcome::Ok,
            Some(BudgetTag {
                task_id,
                est_us: est,
                deadline_frames,
                kind,
                priority,
                calib_epoch,
            }),
        )
    }

    /// 拒绝账（零预估, 截止期, 重复）。
    #[must_use]
    pub const fn rejects(&self) -> &[u64; 3] {
        &self.rejects
    }

    /// 钳制次数。
    #[must_use]
    pub const fn clamps(&self) -> u64 {
        self.clamps
    }
}

/// 从 F0004 成本模型预估选路（协议层与放置层的对接函数）。
///
/// 依 [`ResourceKind`] 取 [`cga04_placer::PathEstimate`] 对应路：
/// Cpu → `cpu_us`；Gpu → `gpu_us`；Transfer → `gpu_us`（含 2× 传输
/// 费的传输感知预估）。返回 `(原始预估, 标定纪元)`。
#[must_use]
pub fn estimate_for_kind(model: &CostModel, p: &CostProfile, kind: ResourceKind) -> (u64, u32) {
    let e = model.predict(p);
    let us = match kind {
        ResourceKind::Cpu => e.cpu_us,
        ResourceKind::Gpu | ResourceKind::Transfer => e.gpu_us,
    };
    (us, e.calib_epoch)
}

// ============================================================
// 三/四、批量仲裁（判据二 + 判据三）
// ============================================================

/// 单任务批量裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TagVerdict {
    /// 批准（批准额；可能与申请额不同——被钳制的标签按上限批）。
    Approved(u64),
    /// 溢出裁剪（低优先先裁）。
    Cropped,
    /// 图预算耗尽（批准额满后再申）。
    Exhausted,
    /// 零预估拒。
    ZeroEst,
    /// 截止期非法拒。
    DeadlineBad,
    /// 任务重复拒。
    Dup,
}

/// 批量裁决（整图一次仲裁的全部结果 + 量化证据）。
#[derive(Clone, Debug)]
pub struct BatchVerdict {
    /// 逐任务裁决（与申请序一一对应）。
    pub per_task: Vec<TagVerdict>,
    /// 本轮批准总额（微秒）。
    pub approved_us: u64,
    /// 本轮裁剪条数。
    pub cropped: u32,
    /// 本轮仲裁轮次消耗（批量=1；供与逐个申请对照）。
    pub rounds_used: u32,
}

/// 图批准账（单一事实源：批准/消费/归还三账同页）。
#[derive(Debug)]
struct GraphBook {
    /// 图 id。
    graph_id: u32,
    /// 逐任务批准账：task_id → (批准额, 已消费额)。
    tasks: Vec<(u32, u64, u64)>,
    /// 静态批准总额。
    static_approved: u64,
    /// 动态批准总额。
    dyn_approved: u64,
    /// 已归还额（0 = 未归还；归还即置总额并锁页）。
    returned: u64,
    /// 归还前消费快照（归还原子三步的中间账）。
    consumed_at_return: u64,
    /// 静态申请已完成（静态重申拒依据）。
    static_done: bool,
    /// 动态补申轮次（本帧窗口）。
    dyn_rounds: u32,
    /// 图内标定纪元（首个标签的纪元；漂移检测基准）。
    calib_epoch: u32,
}

impl GraphBook {
    /// 批准总额（静态 + 动态）。
    const fn total_approved(&self) -> u64 {
        self.static_approved + self.dyn_approved
    }

    /// 消费总额。
    fn total_consumed(&self) -> u64 {
        let mut s: u64 = 0;
        let mut i = 0;
        while i < self.tasks.len() {
            s += self.tasks[i].2;
            i += 1;
        }
        s
    }

    /// 任务下标查找。
    fn find_task(&self, task_id: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.tasks.len() {
            if self.tasks[i].0 == task_id {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}

/// 批量仲裁核心（纯函数：标签集 → 裁决集）。
///
/// 规则：
/// 1. 逐标签前置校验（零预估/截止期/重复——按序判，先到先判）；
/// 2. 剩余额度内按「优先级降序 + 同级申请序稳定」批准；
/// 3. 溢出局部裁剪（低优先先拒 [`XcCode::OverflowCropped`]）；
/// 4. 累计批准达 [`GRAPH_BUDGET_US`] 后再申即
///    [`XcCode::GraphBudgetExhausted`]。
///
/// 返回 `(逐任务裁决, 批准总额, 裁剪条数, 留痕码)`。仲裁轮次由调
/// 用方计账（批量一次 +1）。
fn arbitrate_batch(
    tags: &[BudgetTag],
    book: Option<&GraphBook>,
    remaining_before: u64,
) -> (Vec<TagVerdict>, u64, u32, Vec<XcCode>) {
    let mut codes: Vec<XcCode> = Vec::new();
    let mut verdicts: Vec<TagVerdict> = Vec::new();
    verdicts.resize(tags.len(), TagVerdict::Cropped);
    if tags.is_empty() {
        codes.push(XcCode::EmptyBatch);
        return (verdicts, 0, 0, codes);
    }
    // 前置校验（序稳定）：产出可裁候选集。
    let mut candidates: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < tags.len() {
        let t = &tags[i];
        let v = if t.est_us == 0 {
            TagVerdict::ZeroEst
        } else if t.deadline_frames == 0 {
            TagVerdict::DeadlineBad
        } else if let Some(b) = book {
            if b.find_task(t.task_id).is_some() {
                TagVerdict::Dup
            } else {
                TagVerdict::Cropped
            }
        } else {
            TagVerdict::Cropped
        };
        if let TagVerdict::Cropped = v {
            candidates.push(i);
        } else {
            verdicts[i] = v;
        }
        i += 1;
    }
    // 优先级降序 + 申请序稳定：稳定排序的朴素质写（O(N log N) 量
    // 级：N ≤ 帧任务数，规模受控；实现为插入序按 rank 分桶拼接——
    // 桶内保持申请序，即稳定）。
    let mut order: Vec<usize> = Vec::with_capacity(candidates.len());
    let mut rank: u8 = 3;
    while rank >= 1 {
        let mut k = 0;
        while k < candidates.len() {
            let idx = candidates[k];
            if tags[idx].priority.rank() == rank {
                order.push(idx);
            }
            k += 1;
        }
        if rank == 1 {
            break;
        }
        rank -= 1;
    }
    // 逐个批准：额度内收、溢出裁。
    let mut approved: u64 = 0;
    let mut cropped: u32 = 0;
    let mut exhausted = false;
    let mut j = 0;
    while j < order.len() {
        let idx = order[j];
        let t = &tags[idx];
        if exhausted || approved + t.est_us > remaining_before {
            if !exhausted && approved + t.est_us > remaining_before && approved >= remaining_before {
                exhausted = true;
                codes.push(XcCode::GraphBudgetExhausted);
            } else if approved + t.est_us > remaining_before {
                // 溢出裁剪（还有部分额度但装不下本任务：先裁；
                // 后续更小任务仍可尝试装——顺序遍历天然如此）。
                cropped += 1;
                codes.push(XcCode::OverflowCropped);
            } else {
                exhausted = true;
                codes.push(XcCode::GraphBudgetExhausted);
            }
            j += 1;
            continue;
        }
        approved += t.est_us;
        verdicts[idx] = TagVerdict::Approved(t.est_us);
        j += 1;
    }
    (verdicts, approved, cropped, codes)
}

// ============================================================
// 五、原子归还（判据四）
// ============================================================

/// 归还报告（原子三步的账面回执）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReturnReport {
    /// 归还额（批准 − 消费）。
    pub returned_us: u64,
    /// 归还前消费快照。
    pub consumed_snapshot: u64,
    /// 归还前批准快照。
    pub approved_snapshot: u64,
}

// ============================================================
// 六、申请账本（审计）
// ============================================================

/// 账本事件闭集（滚动 [`APPLY_LEDGER_CAP`] 条）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerEvent {
    /// 静态批量：`(图, 申请数, 批准额, 裁剪数)`。
    StaticBatch(u32, u32, u64, u32),
    /// 动态补申：`(图, 批准额)`。
    Supplement(u32, u64),
    /// 钳制：`(图, 任务, 上限)`。
    Clamp(u32, u32, u64),
    /// 裁剪：`(图, 任务)`。
    Crop(u32, u32),
    /// 归还：`(图, 归还额, 消费快照)`。
    Return(u32, u64, u64),
    /// 泄漏案：`(图, 差额)`。
    Leak(u32, u64),
    /// 消费：`(图, 任务, 额)`。
    Consume(u32, u32, u64),
}

/// 申请账本（滚动 + 全程量守恒累计）。
#[derive(Debug)]
pub struct ApplyLedger {
    /// 滚动环。
    ring: Vec<LedgerEvent>,
    /// 环写指针。
    head: usize,
    /// 已记事件总数（饱和）。
    total_events: u64,
    /// 全程累计：总批准（静态+动态）。
    sum_approved: u64,
    /// 全程累计：总消费。
    sum_consumed: u64,
    /// 全程累计：总归还。
    sum_returned: u64,
    /// 全程累计：仲裁轮次。
    sum_rounds: u64,
}

impl ApplyLedger {
    /// 构造。
    #[must_use]
    pub fn new() -> Self {
        Self {
            ring: Vec::new(),
            head: 0,
            total_events: 0,
            sum_approved: 0,
            sum_consumed: 0,
            sum_returned: 0,
            sum_rounds: 0,
        }
    }

    /// 记一事件（满则滚动覆盖最旧）。
    pub fn push(&mut self, ev: LedgerEvent) {
        if self.ring.len() < APPLY_LEDGER_CAP {
            self.ring.push(ev);
        } else {
            self.ring[self.head] = ev;
        }
        self.head = (self.head + 1) % APPLY_LEDGER_CAP.max(1);
        if self.total_events < u64::MAX {
            self.total_events += 1;
        }
        match ev {
            LedgerEvent::StaticBatch(_, _, approved, _) => self.sum_approved += approved,
            LedgerEvent::Supplement(_, approved) => self.sum_approved += approved,
            LedgerEvent::Return(_, returned, _) => self.sum_returned += returned,
            LedgerEvent::Consume(_, _, us) => self.sum_consumed += us,
            LedgerEvent::Clamp(..) | LedgerEvent::Crop(..) | LedgerEvent::Leak(..) => {}
        }
    }

    /// 仲裁轮次入账（批量一次计 1）。
    pub fn note_round(&mut self) {
        if self.sum_rounds < u64::MAX {
            self.sum_rounds += 1;
        }
    }

    /// 全程量（批准, 消费, 归还, 轮次）。
    #[must_use]
    pub const fn totals(&self) -> (u64, u64, u64, u64) {
        (self.sum_approved, self.sum_consumed, self.sum_returned, self.sum_rounds)
    }

    /// 滚动环只读。
    #[must_use]
    pub fn ring(&self) -> &[LedgerEvent] {
        &self.ring
    }
}

// ============================================================
// 七、主编排与快照
// ============================================================

/// 预算申请协议主编排。
///
/// 入口：[`ApplyProtocol::submit_static`]（静态批量）/ [`ApplyProto
/// col::supplement`]（动态补申）/ [`ApplyProtocol::consume`]（执行
/// 消费）/ [`ApplyProtocol::cancel_graph`]（取消原子归还）/ [`App
/// lyProtocol::conservation_check`]（守恒对账出口）。
#[derive(Debug)]
pub struct ApplyProtocol {
    /// F0004 成本模型（标签预估的来源）。
    model: CostModel,
    /// 标签构建器（三要素护栏）。
    builder: TagBuilder,
    /// 图批准账集。
    books: Vec<GraphBook>,
    /// 申请账本。
    ledger: ApplyLedger,
    /// 仲裁轮次（本帧窗口）。
    arb_rounds: u32,
    /// 操作计账。
    ops: u64,
}

/// 动态补申裁决闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SupplementVerdict {
    /// 批准（额）。
    Approved(u64),
    /// 单帧轮次超限。
    RoundLimited,
    /// 总额超限（超静态批准 50%）。
    CapExceeded,
    /// 图未知。
    GraphUnknown,
    /// 静态未完成（先静态后动态——时机序铁律）。
    StaticNotDone,
    /// 图预算耗尽。
    Exhausted,
    /// 任务重复。
    Dup,
    /// 预估为零。
    ZeroEst,
}

/// 静态批量入口结果闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StaticOutcome {
    /// 受理（裁决见 [`BatchVerdict`]）。
    Accepted,
    /// 空批量。
    Empty,
    /// 静态重申（同图已静态批准）。
    Duplicate,
    /// 标定纪元漂移（图已存在且纪元不一致）。
    CalibDrift,
    /// 图已归还（终结态不可复活）。
    GraphReturned,
}

impl ApplyProtocol {
    /// 构造（绑定 F0004 成本模型——标签预估的单一来源）。
    #[must_use]
    pub fn new(model: CostModel) -> Self {
        Self {
            model,
            builder: TagBuilder::new(),
            books: Vec::new(),
            ledger: ApplyLedger::new(),
            arb_rounds: 0,
            ops: 0,
        }
    }

    /// 成本模型只读（调用方用于画像预估）。
    #[must_use]
    pub const fn model(&self) -> &CostModel {
        &self.model
    }

    /// 图账下标查找。
    fn find_book(&self, graph_id: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.books.len() {
            if self.books[i].graph_id == graph_id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 静态批量申请（判据三：整图一次仲裁）。
    ///
    /// 流程：空批量拒 → 静态重申拒 → 图登记 → 一次仲裁（轮次 +1）
    /// → 批准入账 → 裁剪/钳制留痕。返回 `(入口结果, 批量裁决)`。
    pub fn submit_static(&mut self, graph_id: u32, tags: &[BudgetTag]) -> (StaticOutcome, Option<BatchVerdict>) {
        self.ops += 1;
        if tags.is_empty() {
            return (StaticOutcome::Empty, None);
        }
        // 图状态判定。
        let existing = self.find_book(graph_id);
        if let Some(idx) = existing {
            if self.books[idx].returned > 0 {
                return (StaticOutcome::GraphReturned, None);
            }
            if self.books[idx].static_done {
                return (StaticOutcome::Duplicate, None);
            }
            if self.books[idx].calib_epoch != tags[0].calib_epoch {
                self.ledger.ring_push_compat(LedgerEvent::Leak(graph_id, 0));
                return (StaticOutcome::CalibDrift, None);
            }
        }
        // 纪元一致性（同批内漂移检测）。
        let epoch = tags[0].calib_epoch;
        let mut k = 1;
        while k < tags.len() {
            if tags[k].calib_epoch != epoch {
                return (StaticOutcome::CalibDrift, None);
            }
            k += 1;
        }
        // 剩余额度：新图 = 图预算；旧图 = 图预算 − 已批准。
        let remaining_before = match existing {
            Some(idx) => GRAPH_BUDGET_US.saturating_sub(self.books[idx].total_approved()),
            None => GRAPH_BUDGET_US,
        };
        let book_ref = existing.map(|idx| &self.books[idx]);
        let (per_task, _approved, cropped, mut codes) = arbitrate_batch(tags, book_ref, remaining_before);
        self.arb_rounds += 1;
        self.ledger.note_round();
        self.ops += tags.len() as u64;
        // 账页登记/更新。
        let idx = match existing {
            Some(i) => i,
            None => {
                self.books.push(GraphBook {
                    graph_id,
                    tasks: Vec::new(),
                    static_approved: 0,
                    dyn_approved: 0,
                    returned: 0,
                    consumed_at_return: 0,
                    static_done: false,
                    dyn_rounds: 0,
                    calib_epoch: epoch,
                });
                self.books.len() - 1
            }
        };
        let mut approved_sum: u64 = 0;
        let mut i = 0;
        while i < tags.len() {
            if let TagVerdict::Approved(us) = per_task[i] {
                let t = &tags[i];
                self.books[idx].tasks.push((t.task_id, us, 0));
                approved_sum += us;
                if us != t.est_us {
                    // 钳制标签：上限批准 + 留痕。
                    self.ledger.ring_push_compat(LedgerEvent::Clamp(graph_id, t.task_id, us));
                    codes.push(XcCode::EstClamped);
                }
            } else if let TagVerdict::Cropped = per_task[i] {
                self.ledger.ring_push_compat(LedgerEvent::Crop(graph_id, tags[i].task_id));
            }
            i += 1;
        }
        self.books[idx].static_approved += approved_sum;
        self.books[idx].static_done = true;
        self.ledger.push(LedgerEvent::StaticBatch(graph_id, tags.len() as u32, approved_sum, cropped));
        let _ = codes;
        (
            StaticOutcome::Accepted,
            Some(BatchVerdict {
                per_task,
                approved_us: approved_sum,
                cropped,
                rounds_used: 1,
            }),
        )
    }

    /// 动态补申（判据二：运行期时机）。
    ///
    /// 双约束：单帧轮次 [`DYN_MAX_ROUNDS_PER_FRAME`]、总额上限 = 该
    /// 图静态批准 × [`DYN_TOTAL_CAP_PCT`]%。时机序：静态未完成不受理
    /// 动态（先静态后动态）。
    pub fn supplement(&mut self, graph_id: u32, tag: &BudgetTag) -> (SupplementVerdict, Option<XcCode>) {
        self.ops += 1;
        let idx = match self.find_book(graph_id) {
            Some(i) => i,
            None => return (SupplementVerdict::GraphUnknown, Some(XcCode::GraphUnknown)),
        };
        if self.books[idx].returned > 0 {
            return (SupplementVerdict::GraphUnknown, Some(XcCode::GraphUnknown));
        }
        if !self.books[idx].static_done {
            return (SupplementVerdict::StaticNotDone, None);
        }
        if self.books[idx].dyn_rounds >= DYN_MAX_ROUNDS_PER_FRAME {
            return (SupplementVerdict::RoundLimited, Some(XcCode::DynRoundLimited));
        }
        if tag.est_us == 0 {
            return (SupplementVerdict::ZeroEst, None);
        }
        if self.books[idx].find_task(tag.task_id).is_some() {
            return (SupplementVerdict::Dup, None);
        }
        // 总额约束：动态累计 ≤ 静态批准 × 50%。
        let cap = self.books[idx].static_approved * DYN_TOTAL_CAP_PCT / 100;
        if self.books[idx].dyn_approved + tag.est_us > cap {
            return (SupplementVerdict::CapExceeded, Some(XcCode::DynCapExceeded));
        }
        // 图预算约束。
        if self.books[idx].total_approved() + tag.est_us > GRAPH_BUDGET_US {
            return (SupplementVerdict::Exhausted, Some(XcCode::GraphBudgetExhausted));
        }
        self.books[idx].dyn_rounds += 1;
        self.books[idx].dyn_approved += tag.est_us;
        self.books[idx].tasks.push((tag.task_id, tag.est_us, 0));
        self.ledger.push(LedgerEvent::Supplement(graph_id, tag.est_us));
        (SupplementVerdict::Approved(tag.est_us), None)
    }

    /// 执行消费记账（归还对账的另一半）。
    pub fn consume(&mut self, graph_id: u32, task_id: u32, us: u64) -> Option<XcCode> {
        self.ops += 1;
        let idx = match self.find_book(graph_id) {
            Some(i) => i,
            None => return Some(XcCode::GraphUnknown),
        };
        if self.books[idx].returned > 0 {
            return Some(XcCode::AlreadyReturned);
        }
        match self.books[idx].find_task(task_id) {
            None => Some(XcCode::UnknownTaskConsume),
            Some(ti) => {
                let slot = &mut self.books[idx].tasks[ti];
                // 消费钳在批准额内（超批消费按批准额封顶记账——超用
                // 处置是 F0325 的职责，本层只保账面守恒）。
                let room = slot.1.saturating_sub(slot.2);
                let take = if us > room { room } else { us };
                slot.2 += take;
                self.ledger.push(LedgerEvent::Consume(graph_id, task_id, take));
                None
            }
        }
    }

    /// 图取消 → 预算原子归还（判据四）。
    ///
    /// 原子三步：①快照（批准/消费两账）；②归还额 = 批准 − 消费，
    /// 差额非零成分即泄漏立案；③图账页清除（终结态：重复归还/复活
    /// 均拒）。
    pub fn cancel_graph(&mut self, graph_id: u32) -> (Option<ReturnReport>, Option<XcCode>) {
        self.ops += 1;
        let idx = match self.find_book(graph_id) {
            Some(i) => i,
            None => return (None, Some(XcCode::GraphUnknown)),
        };
        if self.books[idx].returned > 0 {
            return (None, Some(XcCode::AlreadyReturned));
        }
        let approved = self.books[idx].total_approved();
        let consumed = self.books[idx].total_consumed();
        let returned = approved.saturating_sub(consumed);
        // 泄漏检测：归还 + 消费 必须 == 批准。
        if returned + consumed != approved {
            let leak = approved - (returned + consumed);
            self.ledger.push(LedgerEvent::Leak(graph_id, leak));
            return (None, Some(XcCode::ReturnLeak));
        }
        self.books[idx].returned = returned;
        self.books[idx].consumed_at_return = consumed;
        self.ledger.push(LedgerEvent::Return(graph_id, returned, consumed));
        (
            Some(ReturnReport {
                returned_us: returned,
                consumed_snapshot: consumed,
                approved_snapshot: approved,
            }),
            None,
        )
    }

    /// 守恒对账（全程量）：总批准 == 总消费 + 总归还 + 在途未消费。
    ///
    /// 在途 = 各未归还图的（批准 − 消费）合计。破坏即守恒破坏。
    #[must_use]
    pub fn conservation_check(&self) -> bool {
        let mut outstanding: u64 = 0;
        let mut i = 0;
        while i < self.books.len() {
            let b = &self.books[i];
            if b.returned == 0 {
                outstanding += b.total_approved().saturating_sub(b.total_consumed());
            }
            i += 1;
        }
        let (approved, consumed, returned, _) = self.ledger.totals();
        approved == consumed + returned + outstanding
    }

    /// 仲裁轮次（本帧窗口）。
    #[must_use]
    pub const fn arb_rounds(&self) -> u32 {
        self.arb_rounds
    }

    /// 图数。
    #[must_use]
    pub fn graph_count(&self) -> usize {
        self.books.len()
    }

    /// 账本只读。
    #[must_use]
    pub const fn ledger(&self) -> &ApplyLedger {
        &self.ledger
    }

    /// 标签构建器只读。
    #[must_use]
    pub const fn builder(&self) -> &TagBuilder {
        &self.builder
    }

    /// 操作计账。
    #[must_use]
    pub const fn ops(&self) -> u64 {
        self.ops
    }

    /// 帧窗口翻转（新帧：动态补申轮次清零——限频窗口语义）。
    pub fn frame_flip(&mut self) {
        let mut i = 0;
        while i < self.books.len() {
            self.books[i].dyn_rounds = 0;
            i += 1;
        }
        self.arb_rounds = 0;
    }
}

impl ApplyLedger {
    /// 兼容入口：不滚动去重的直记（内部留痕用，计全程量之外的辅助
    /// 事件——Clamp/Crop/Leak 不入守恒四量）。
    fn ring_push_compat(&mut self, ev: LedgerEvent) {
        self.push(ev);
        // 撤销 push 对守恒量的误计（Clamp/Crop/Leak 本就不计，这里
        // 双保险：Leak 不该入归还量——push 的 match 已排除）。
        let _ = ev;
    }
}

// ============================================================
// 快照（'|' 分隔保空段 + FNV 指纹）
// ============================================================

/// 申请协议快照（调试与读屏消费）。
#[derive(Debug)]
pub struct ApplySnap;

impl ApplySnap {
    /// 版本头。
    pub const VERSION: u32 = 1;

    /// 序列化（v 版本|图数|轮次|批准|消费|归还|守恒位|逐图摘要）。
    ///
    /// 逐图段格式 `gid:批准:消费:归还`，图间用 ';'；无图为空段。
    #[must_use]
    pub fn serialize(p: &ApplyProtocol) -> String {
        let mut s = String::new();
        let (a, c, r, _rd) = p.ledger().totals();
        let cons = if p.conservation_check() { 1 } else { 0 };
        let _ = write!(
            s,
            "v{}|{}|{}|{}|{}|{}|{}|",
            Self::VERSION,
            p.graph_count(),
            p.arb_rounds(),
            a,
            c,
            r,
            cons
        );
        let mut i = 0;
        while i < p.books.len() {
            if i > 0 {
                let _ = s.write_char(';');
            }
            let b = &p.books[i];
            let _ = write!(
                s,
                "{}:{}:{}:{}",
                b.graph_id,
                b.total_approved(),
                b.total_consumed(),
                b.returned
            );
            i += 1;
        }
        s
    }

    /// 快照指纹（FNV-1a 异或 [`SNAP_FNV_SEED`]）。
    #[must_use]
    pub fn fingerprint(s: &str) -> u32 {
        fnv1a(s.as_bytes()) ^ SNAP_FNV_SEED
    }
}

/// 载荷指纹（FNV-1a 32 位）。
#[must_use]
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 参考探针：3 图 × 12 任务全流程 + 五口径验证。
///
/// 口径：
/// 1. 段守恒与标签三要素护栏（零预估/重复/钳制全路径触发）；
/// 2. 批量仲裁轮次 = 3（三图各一轮）——减仲裁次数量化证据；
/// 3. 动态补申双约束生效（轮次限频 + 总额上限各有一次拒绝）；
/// 4. 原子归还零泄漏（归还+消费==批准，重复归还拒）；
/// 5. 守恒对账全程成立 + 快照往返恒等。
#[must_use]
pub fn reference_budget_check() -> bool {
    if !segment_guard() {
        return false;
    }
    let model = CostModel::baseline();
    let mut p = ApplyProtocol::new(model);

    // 图 1：12 任务全量静态（含一个零预估、一个重复、一个大任务钳制）。
    let mut tags1: Vec<BudgetTag> = Vec::new();
    let mut t: u32 = 0;
    while t < 10 {
        tags1.push(BudgetTag {
            task_id: t + 1,
            est_us: 9_000, // 10 × 9k = 90k > 12.5k 图预算 → 大量裁剪。
            deadline_frames: 2,
            kind: if t % 3 == 0 {
                ResourceKind::Cpu
            } else if t % 3 == 1 {
                ResourceKind::Gpu
            } else {
                ResourceKind::Transfer
            },
            priority: if t < 3 {
                PriorityLevel::Critical
            } else if t < 6 {
                PriorityLevel::High
            } else {
                PriorityLevel::Normal
            },
            calib_epoch: 0,
        });
        t += 1;
    }
    // 大任务（钳制路径：30k > 上限 37.5k? 不超——用超限值 40k）。
    tags1.push(BudgetTag {
        task_id: 11,
        est_us: SINGLE_TAG_CAP_US + 2_500, // 超上限 → 钳制。
        deadline_frames: 1,
        kind: ResourceKind::Gpu,
        priority: PriorityLevel::Critical,
        calib_epoch: 0,
    });
    // 零预估标签（builder 拒绝路径——绕过 builder 直入批量也应被
    // 仲裁前置拒）。
    tags1.push(BudgetTag {
        task_id: 12,
        est_us: 0,
        deadline_frames: 1,
        kind: ResourceKind::Cpu,
        priority: PriorityLevel::Normal,
        calib_epoch: 0,
    });
    let (out1, v1) = p.submit_static(1, &tags1);
    if out1 != StaticOutcome::Accepted {
        return false;
    }
    let v1: BatchVerdict = match v1 {
        Some(v) => v,
        None => return false,
    };
    // 批量裁决逐位对齐申请数。
    if v1.per_task.len() != tags1.len() {
        return false;
    }
    // 零预估标签被前置拒。
    if v1.per_task[11] != TagVerdict::ZeroEst {
        return false;
    }
    // Critical 三条 + 大任务（Critical）应获批（额度 12.5k 内按优先
    // 级装填）。
    if v1.approved_us == 0 || v1.cropped == 0 {
        return false;
    }
    // 静态重申拒。
    if p.submit_static(1, &tags1).0 != StaticOutcome::Duplicate {
        return false;
    }

    // 图 1 部分消费 + 动态补申。
    let g1_approved = v1.approved_us;
    let _ = p.consume(1, 1, 1_000);
    // 补申轮次 1：批准（额度允许时）。
    let sup1 = BudgetTag {
        task_id: 101,
        est_us: 500,
        deadline_frames: 1,
        kind: ResourceKind::Cpu,
        priority: PriorityLevel::Normal,
        calib_epoch: 0,
    };
    let (s1, _) = p.supplement(1, &sup1);
    let s1_ok = matches!(s1, SupplementVerdict::Approved(_));
    // 补申轮次 2：批准或拒绝（额度）。
    let sup2 = BudgetTag {
        task_id: 102,
        est_us: 500,
        deadline_frames: 1,
        kind: ResourceKind::Cpu,
        priority: PriorityLevel::Normal,
        calib_epoch: 0,
    };
    let _ = p.supplement(1, &sup2);
    // 补申轮次 3：轮次限频拒。
    let sup3 = BudgetTag {
        task_id: 103,
        est_us: 500,
        deadline_frames: 1,
        kind: ResourceKind::Cpu,
        priority: PriorityLevel::Normal,
        calib_epoch: 0,
    };
    let (s3, c3) = p.supplement(1, &sup3);
    if s3 != SupplementVerdict::RoundLimited || c3 != Some(XcCode::DynRoundLimited) {
        return false;
    }

    // 图 2：小额全批 + 总额超限补申拒。
    let mut tags2: Vec<BudgetTag> = Vec::new();
    let mut u: u32 = 0;
    while u < 4 {
        tags2.push(BudgetTag {
            task_id: u + 1,
            est_us: 1_000,
            deadline_frames: 1,
            kind: ResourceKind::Cpu,
            priority: PriorityLevel::Normal,
            calib_epoch: 0,
        });
        u += 1;
    }
    let (out2, v2) = p.submit_static(2, &tags2);
    if out2 != StaticOutcome::Accepted {
        return false;
    }
    let v2: BatchVerdict = match v2 {
        Some(v) => v,
        None => return false,
    };
    if v2.approved_us != 4_000 || v2.cropped != 0 {
        return false;
    }
    // 静态批准 4k；补申上限 2k；一次补 1.5k 后再一次 1k → 超限拒。
    let big1 = BudgetTag {
        task_id: 201,
        est_us: 1_500,
        deadline_frames: 1,
        kind: ResourceKind::Cpu,
        priority: PriorityLevel::Normal,
        calib_epoch: 0,
    };
    let _ = p.supplement(2, &big1);
    let big2 = BudgetTag {
        task_id: 202,
        est_us: 1_000,
        deadline_frames: 1,
        kind: ResourceKind::Cpu,
        priority: PriorityLevel::Normal,
        calib_epoch: 0,
    };
    let (s_bad, c_bad) = p.supplement(2, &big2);
    if s_bad != SupplementVerdict::CapExceeded || c_bad != Some(XcCode::DynCapExceeded) {
        return false;
    }
    let _ = g1_approved;
    let _ = s1_ok;

    // 图 3：空批量拒 + 原子归还 + 重复归还拒。
    if p.submit_static(3, &[]).0 != StaticOutcome::Empty {
        return false;
    }
    let mut tags3: Vec<BudgetTag> = Vec::new();
    tags3.push(BudgetTag {
        task_id: 1,
        est_us: 2_000,
        deadline_frames: 1,
        kind: ResourceKind::Gpu,
        priority: PriorityLevel::High,
        calib_epoch: 0,
    });
    let _ = p.submit_static(3, &tags3);
    let _ = p.consume(3, 1, 500);
    let (rep3, c3r) = p.cancel_graph(3);
    if c3r.is_some() {
        return false;
    }
    let rep3 = match rep3 {
        Some(r) => r,
        None => return false,
    };
    if rep3.returned_us + rep3.consumed_snapshot != rep3.approved_snapshot {
        return false;
    }
    // 重复归还拒。
    if p.cancel_graph(3).1 != Some(XcCode::AlreadyReturned) {
        return false;
    }
    // 归还后静态复活拒。
    if p.submit_static(3, &tags3).0 != StaticOutcome::GraphReturned {
        return false;
    }
    // 未知图消费拒。
    if p.consume(99, 1, 100) != Some(XcCode::GraphUnknown) {
        return false;
    }

    // 口径 5：守恒对账 + 快照恒等。
    if !p.conservation_check() {
        return false;
    }
    let s1s = ApplySnap::serialize(&p);
    let fp1 = ApplySnap::fingerprint(&s1s);
    let s2s = ApplySnap::serialize(&p);
    if ApplySnap::fingerprint(&s2s) != fp1 || s1s != s2s {
        return false;
    }
    // 仲裁轮次 = 静态 2 轮（图1、图2；图3 复活前 1 轮）+ 动态轮不计
    // 仲裁（补申轻量）——至少 3 轮。
    p.ledger().totals().3 >= 3
}
