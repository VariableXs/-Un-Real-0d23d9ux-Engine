//! VE-F0212 · virtio 错误恢复（VE-B 域 · GPU 驱动矩阵 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0212`
//!
//! **判据（锚点原文）**：分级处置、快照比对、重试上限、通知联动、判据。
//!
//! # 职责
//!
//! virtio-gpu 错误恢复矩阵——按错误类别分级处置：
//!
//! 1. **命令解析错 → 丢弃该命令续跑**（级别 [`Severity::Command`]）：只丢一条，
//!    该队列其余命令继续处理，队列与资源表不得被本动作触碰。
//! 2. **队列坏 → 重置该队列**（级别 [`Severity::Queue`]）：只复位命中队列，
//!    其余队列与资源表保持不变。
//! 3. **设备坏 → 整设备 reset 重走初始化序**（级别 [`Severity::Device`]）：
//!    全部队列复位 + 资源表按设备真值重建，复用 F0201 初始化序
//!    （[`RESET_SEQUENCE`]，下游 F0217 热重置复用同一序）。
//! 4. **恢复过程产出降级事件走 F0103 通知协议**：每次恢复产出
//!    [`DegradeNotice`]，三要素齐（发生了什么 / 为什么 / 下一步），
//!    恢复过程界面提示**可读可关闭**。
//! 5. **恢复前后状态快照比对防静默状态错乱**：动作执行前取
//!    [`RecoverySnapshot`]，执行后取一份并[`diff`] 比对；动作与
//!    「它被允许造成的状态变化」不符即判**静默状态错乱**，恢复**不算成功**。
//!
//! # 分类表（锚点「错误分类表：类别 × 级别 × 处置动作」）
//!
//! | 类别 | 级别 | 处置动作 |
//! |------|------|----------|
//! | [`ErrClass::CommandParse`] | Command | [`ActionKind::DropCommandContinue`] |
//! | [`ErrClass::QueueCorrupt`] | Queue | [`ActionKind::ResetQueue`] |
//! | [`ErrClass::ResourceDrift`] | Queue | [`ActionKind::RebuildResourceTable`] |
//! | [`ErrClass::DeviceFault`] | Device | [`ActionKind::ResetDeviceReinit`] |
//! | [`ErrClass::Unknown`] | Command | [`ActionKind::DropCommandContinue`] |
//!
//! 级别与处置动作是**两个独立维度**：级别决定重试计数与升级路径，动作决定
//! 本次实际改动什么。升级时动作必须**按新级别重算**（[`ErrClass::action_for`]）
//! ——沿用原动作会让「升级处置级别」成为空话：设备级故障重试三次仍只丢一条
//! 命令，设备永远不会复位。
//!
//! # 错误路径与降级矩阵（锚点原文）
//!
//! - **恢复失败 → 升级处置级别重试有上限**：同级别重试达
//!   [`RECOVERY_RETRY_LIMIT`] 次即升一级；升到 [`Severity::Fatal`] 即标记设备
//!   不可用。另有全局硬顶 [`RECOVERY_TOTAL_CAP`] 兜住「每次都升级」的打转。
//! - **资源表不一致 → 按设备真值重建**：`diff` 检出资源表被动而本次动作不该
//!   动它，即判静默错乱并记 [`RecoveryStats::silent_corruption`]；分类为
//!   [`ErrClass::ResourceDrift`] 时按 [`device_truth`] 重建，kept / dropped /
//!   added 三项计数公开（不是静默对齐）。
//! - **反复损坏 → 标记设备不可用并通知**：设备不可用后[`recover`] 拒绝再恢复
//!   （[`RecoveryStats::recoveries_refused`]），界面须走降级路径而非无限重试。
//!
//! # 性能逐项分解（锚点原文）
//!
//! - **快照 O(资源数) 仅恢复时执行**：快照只在 `recover` 内部取，正常渲染路径
//!   零快照、零分配（[`RecoveryStats::snapshots_taken`] 在无恢复时恒为 0）。
//! - **正常路径零开销**：正常路径不调本单元任何函数即零成本；本单元无
//!   `Drop`、无后台定时器、无常驻大缓冲。
//! - **重试上限防打转**：两级上限（同级重试上限 + 全局硬顶）+ 设备不可用闸，
//!   三者任一命中即终止恢复链。
//!
//! # 跨批对接点（锚点原文）
//!
//! - **上游 F0211**（`veb11_irq`）：中断与事件处理产出的设备异常经
//!   [`ErrClass::from_irq_param`] 映射为本单元的错误类别输入。
//! - **下游 F0217**（热重置与 suspend/resume）：[`RESET_SEQUENCE`] 与
//!   [`RecoveryEngine::reinit_sequence`] 为整设备 reset 序的**单一事实源**，
//!   F0217 复用同一序，不另立一套（两处序不一致 = 热重置后设备行为与冷启动不同）。
//! - **F0103 通知协议**：降级事件经 [`DegradeNotice`] 三要素产出，
//!   合并限频与通道故障补投见通知面。
//!
//! # 无障碍与隐私
//!
//! - **降级通知三要素齐**：[`DegradeNotice`] 构造期强制三字段非空，缺任一项
//!   构造失败（[`NoticeReject`]）——不给静默空文案留机会。
//! - **恢复过程界面提示可读可关闭**：[`DegradeNotice::screen_text`] 一次播全
//!   三要素（不截断，读屏用户能拿到完整因果）；[`DegradeNotice::dismissible`]
//!   为真表示用户可关闭该提示。
//! - **不依赖颜色单独表意**：通知语义全部落在文字里，无颜色通道。
//! - **隐私**：本单元不载任何用户内容——错误类别只含设备侧编码与队列号，
//!   资源表只含 id / 页数 / 挂载标志，无像素、无文件名、无窗口标题。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 同级别重试上限：达此后升级处置级别（锚点「重试有上限」第一级）。
pub const RECOVERY_RETRY_LIMIT: u32 = 3;

/// 全局恢复次数硬顶：所有级别累计达此后直接判设备不可用（第二级防打转）。
pub const RECOVERY_TOTAL_CAP: u32 = 12;

/// 队列槽位数（virtio-gpu 固定三队列：controlq / cursorq / 主渲染队列，留余量）。
pub const QUEUE_SLOTS: usize = 8;

/// 资源表槽位数（快照与比对的工作量上界）。
pub const RESOURCE_SLOTS: usize = 64;

/// 严重级别槽位数（Command / Queue / Device / Fatal）。
pub const SEVERITY_SLOTS: usize = 4;

/// 降级通知环容量（合并限频后仍需容纳的独立通知条数上界）。
pub const NOTICE_SLOTS: usize = 8;

/// 同类通知合并窗口（逻辑 tick）。
pub const NOTICE_MERGE_WINDOW: u64 = 4;

/// 快照最大资源条目数（防止设备真值异常膨胀拖垮恢复路径）。
pub const SNAPSHOT_MAX_RESOURCES: usize = RESOURCE_SLOTS;

/// 复位后队列的就绪初值。
pub const QUEUE_READY_AFTER_RESET: bool = true;

/// 复位后队列的可用环初值。
pub const AVAIL_AFTER_RESET: u16 = 0;

/// 复位后队列的待处理命令数初值。
pub const PENDING_AFTER_RESET: u32 = 0;

/// 与 F0103 通知协议的契约版本（降级通知结构变更即递增）。
pub const NOTICE_LINK_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// 二、错误分类表（类别 × 级别 × 处置动作）
// ---------------------------------------------------------------------------

/// 错误类别（锚点分类表的「类别」维）。
///
/// 命令码 / 队列号 / 设备状态字按原值透传，不做语义猜测——猜测会把一类错误
/// 误归到另一类，处置动作随之错（丢一条命令 vs 复位整设备）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrClass {
    /// 命令解析错（带命令码）→ 丢弃该命令续跑。
    CommandParse(u32),
    /// 队列坏（带队列号）→ 重置该队列。
    QueueCorrupt(u16),
    /// 资源表不一致 → 按设备真值重建。
    ResourceDrift,
    /// 设备坏（带设备状态字）→ 整设备 reset 重走初始化序。
    DeviceFault(u32),
    /// 未知错误码：记录并按最低级别处置（不静默、不升级猜高级别）。
    Unknown(u32),
}

impl ErrClass {
    /// 类别 → 默认级别（锚点分类表的「级别」维）。
    pub fn severity(self) -> Severity {
        match self {
            ErrClass::CommandParse(_) => Severity::Command,
            ErrClass::QueueCorrupt(_) => Severity::Queue,
            ErrClass::ResourceDrift => Severity::Queue,
            ErrClass::DeviceFault(_) => Severity::Device,
            // 未知类别按最低级别处置：宁可多丢一条命令也不误复位整设备。
            ErrClass::Unknown(_) => Severity::Command,
        }
    }

    /// 级别 + 类别 → 处置动作（锚点分类表的「处置动作」维）。
    ///
    /// **升级时必须走本函数重算**，不可沿用低级别的动作（见头注「两个独立
    /// 维度」）。
    pub fn action_for(self, sev: Severity) -> ActionKind {
        match sev {
            Severity::Command => ActionKind::DropCommandContinue,
            Severity::Queue => match self {
                ErrClass::ResourceDrift => ActionKind::RebuildResourceTable,
                _ => ActionKind::ResetQueue,
            },
            Severity::Device => ActionKind::ResetDeviceReinit,
            Severity::Fatal => ActionKind::MarkUnusable,
        }
    }

    /// 类别 + 级别 → 完整处置计划。
    pub fn classify(self) -> (Severity, ActionKind) {
        let sev = self.severity();
        let kind = self.action_for(sev);
        (sev, kind)
    }

    /// 类别 → 内部归一标签（通知合并与计数用；原码不丢，透传在本函数返回值里）。
    pub fn tag(self) -> u32 {
        match self {
            ErrClass::CommandParse(c) => 0xC0_00 | (c & 0xFF),
            ErrClass::QueueCorrupt(q) => 0xB0_00 | (q as u32 & 0xFF),
            ErrClass::ResourceDrift => 0xD0_01,
            ErrClass::DeviceFault(s) => 0xE0_00 | (s & 0xFF),
            ErrClass::Unknown(c) => 0xF0_00 | (c & 0xFF),
        }
    }

    /// 上游 F0211 事件参数 → 错误类别（跨批对接：上游事件进本单元）。
    ///
    /// F0211 的事件参数对已知类别（Display / Cursor）是呈现语义，不是错误码；
    /// 故本函数只在参数落在**错误码窗口**内才映射为错误类别，其余原样返回
    /// [`ErrClass::Unknown`]。把呈现事件误判成设备故障会让正常刷新触发整设备
    /// 复位——这类误判必须避免而非事后靠重试上限兜。
    pub fn from_irq_param(scanout: u32, param: u32) -> ErrClass {
        // 错误码窗口：bit31 置位（呈现/光标事件的参数不会置这一位）。
        if param & 0x8000_0000 == 0 {
            return ErrClass::Unknown(param);
        }
        let code = param & 0x7FFF_FFFF;
        match code >> 16 {
            // 0x01 = 命令解析错（低 16 位为命令码）
            0x01 => ErrClass::CommandParse(code & 0xFFFF),
            // 0x02 = 队列坏（低 16 位为队列号）
            0x02 => ErrClass::QueueCorrupt((code & 0xFFFF) as u16),
            // 0x03 = 资源表不一致
            0x03 => ErrClass::ResourceDrift,
            // 0x04 = 设备坏（低 16 位为状态字）
            0x04 => ErrClass::DeviceFault(code & 0xFFFF),
            _ => {
                // 未登记的错误码：计数里带上扫描输出号，便于回溯是哪条输出报的。
                let _ = scanout;
                ErrClass::Unknown(code)
            }
        }
    }

    /// 该类别是否**固有**需要触碰资源表（分类表的固有属性）。
    ///
    /// 与 [`ActionKind::allows_resource_change`] 的区别：后者说的是「本次动作
    /// 被允许造成什么变化」，本函数说的是「这类错误本身是否牵动资源表」。
    /// 两者不等价——设备级故障重试到队列级时，本次动作不允许改资源表，但
    /// 类别仍固有牵动它；判据据此区分「动作越界」与「类别天生如此」。
    pub fn touches_resources(self) -> bool {
        matches!(self, ErrClass::ResourceDrift | ErrClass::DeviceFault(_))
    }

    /// 类别名（诊断与通知文案用）。
    pub fn name(self) -> &'static str {
        match self {
            ErrClass::CommandParse(_) => "命令解析错",
            ErrClass::QueueCorrupt(_) => "队列损坏",
            ErrClass::ResourceDrift => "资源表不一致",
            ErrClass::DeviceFault(_) => "设备故障",
            ErrClass::Unknown(_) => "未分类错误",
        }
    }
}

/// 严重级别（锚点「分级处置」的级）。
///
/// 序数即升级序：`escalate` 逐级上行，[`Severity::Fatal`] 为顶（不可再升）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// L0：单命令级——丢一条命令续跑。
    Command,
    /// L1：队列级——复位命中队列。
    Queue,
    /// L2：设备级——整设备 reset 重走初始化序。
    Device,
    /// L3：终态——标记设备不可用并通知（反复损坏）。
    Fatal,
}

impl Severity {
    /// 级别 → 槽位下标（显式映射，不依赖枚举判别值——判别值不是线上编码值）。
    pub fn idx(self) -> usize {
        match self {
            Severity::Command => 0,
            Severity::Queue => 1,
            Severity::Device => 2,
            Severity::Fatal => 3,
        }
    }

    /// 升级到下一级；`Fatal` 为顶，返回自身（升级链在此终止）。
    pub fn escalate(self) -> Severity {
        match self {
            Severity::Command => Severity::Queue,
            Severity::Queue => Severity::Device,
            Severity::Device | Severity::Fatal => Severity::Fatal,
        }
    }

    /// 级别名（通知三要素的「为什么」维）。
    pub fn name(self) -> &'static str {
        match self {
            Severity::Command => "单命令级",
            Severity::Queue => "队列级",
            Severity::Device => "设备级",
            Severity::Fatal => "设备不可用",
        }
    }
}

/// 处置动作（锚点分类表的「处置动作」维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    /// 丢弃该命令续跑：只减命中队列的待处理数，其余不动。
    DropCommandContinue,
    /// 重置该队列：只复位命中队列。
    ResetQueue,
    /// 按设备真值重建资源表。
    RebuildResourceTable,
    /// 整设备 reset 重走初始化序（全队列复位 + 资源表重建）。
    ResetDeviceReinit,
    /// 标记设备不可用并通知（恢复链终止）。
    MarkUnusable,
}

impl ActionKind {
    /// 动作名（诊断与通知文案用）。
    pub fn name(self) -> &'static str {
        match self {
            ActionKind::DropCommandContinue => "丢弃命令续跑",
            ActionKind::ResetQueue => "重置队列",
            ActionKind::RebuildResourceTable => "按设备真值重建资源表",
            ActionKind::ResetDeviceReinit => "整设备重置并重走初始化序",
            ActionKind::MarkUnusable => "标记设备不可用",
        }
    }

    /// 本动作是否允许改动资源表（快照比对的期望面来源）。
    pub fn allows_resource_change(self) -> bool {
        matches!(
            self,
            ActionKind::RebuildResourceTable | ActionKind::ResetDeviceReinit
        )
    }

    /// 本动作是否允许改动队列状态（快照比对的期望面来源）。
    pub fn allows_queue_change(self) -> bool {
        !matches!(self, ActionKind::MarkUnusable)
    }
}

// ---------------------------------------------------------------------------
// 三、恢复快照（队列状态 × 资源表）
// ---------------------------------------------------------------------------

/// 队列角色（virtio-gpu 队列语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueRole {
    /// 控制队列（资源创建 / 命令提交）。
    Control,
    /// 光标队列（F0210 光标通道）。
    Cursor,
    /// 主渲染队列（传输与呈现）。
    Render,
}

impl QueueRole {
    /// 角色 → 队列下标（virtio-gpu 队列号即角色序）。
    pub fn index(self) -> u16 {
        match self {
            QueueRole::Control => 0,
            QueueRole::Cursor => 1,
            QueueRole::Render => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            QueueRole::Control => "控制队列",
            QueueRole::Cursor => "光标队列",
            QueueRole::Render => "主渲染队列",
        }
    }
}

/// 队列状态行（快照的队列维度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueRow {
    /// 队列下标。
    pub index: u16,
    /// 队列角色（由下标反查，缺失槽位为 `Control`——不参与比对）。
    pub role: QueueRole,
    /// 设备是否报告该队列就绪。
    pub ready: bool,
    /// 可用环游标（复位后应为 [`AVAIL_AFTER_RESET`]）。
    pub avail: u16,
    /// 待处理命令数（丢弃命令动作改这一项）。
    pub pending: u32,
}

/// 资源表条目（快照的资源维度；只含设备侧记账，不含任何用户内容）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceRow {
    /// 资源 id。
    pub id: u32,
    /// backing 页数（页对齐，与显存池一致）。
    pub backing_pages: u32,
    /// 是否已挂载到扫描输出。
    pub attached: bool,
}

/// 恢复快照（锚点「恢复快照：队列状态 × 资源表」）。
///
/// 用 `Vec` 而非定长数组内嵌：快照只在恢复路径取，恢复路径本就允许分配；
/// 定长 64 槽数组会占约 3KB 栈，在内核深栈上是隐患，而正常路径根本不取快照。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecoverySnapshot {
    /// 快照所属逻辑 tick。
    pub tick: u64,
    /// 队列状态行（按队列下标升序）。
    pub queues: Vec<QueueRow>,
    /// 资源表条目。
    pub resources: Vec<ResourceRow>,
    /// 设备是否可用。
    pub device_usable: bool,
}

impl RecoverySnapshot {
    /// 按队列下标查行（越界返回 `None`，不 panic）。
    pub fn queue(&self, index: u16) -> Option<QueueRow> {
        let i = index as usize;
        if i >= self.queues.len() {
            return None;
        }
        Some(self.queues[i])
    }

    /// 按资源 id 查条目（线性扫描：资源表无序，比对总量已是 O(资源数)）。
    pub fn resource(&self, id: u32) -> Option<ResourceRow> {
        let mut i = 0;
        while i < self.resources.len() {
            if self.resources[i].id == id {
                return Some(self.resources[i]);
            }
            i += 1;
        }
        None
    }

    /// 资源条目数（快照 O(资源数) 的实际工作量）。
    pub fn resource_count(&self) -> usize {
        self.resources.len()
    }
}

/// 快照比对结果（锚点「恢复前后状态快照比对防静默状态错乱」）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SnapshotDiff {
    /// 就绪位翻转的队列数。
    pub queue_ready_changed: u16,
    /// 待处理命令数变化的队列数。
    pub queue_pending_changed: u16,
    /// 可用环游标变化的队列数。
    pub queue_avail_changed: u16,
    /// 恢复后缺失的资源数（before 有 after 无）。
    pub resource_missing: u16,
    /// 恢复后多出的资源数（after 有 before 无）。
    pub resource_added: u16,
    /// backing 页数变化的资源数（**静默状态错乱的头号来源**）。
    pub resource_backing_changed: u16,
    /// 挂载标志变化的资源数。
    pub resource_attach_changed: u16,
    /// 设备可用标志是否变化。
    pub device_usable_changed: bool,
}

impl SnapshotDiff {
    /// 比对是否完全干净（无任何状态变化）。
    pub fn is_clean(&self) -> bool {
        *self == SnapshotDiff::default()
    }

    /// 资源表是否被改动。
    pub fn resource_touched(&self) -> bool {
        self.resource_missing != 0
            || self.resource_added != 0
            || self.resource_backing_changed != 0
            || self.resource_attach_changed != 0
    }

    /// 队列状态是否被改动。
    pub fn queue_touched(&self) -> bool {
        self.queue_ready_changed != 0
            || self.queue_pending_changed != 0
            || self.queue_avail_changed != 0
    }
}

/// 前后快照比对（O(队列数 + 资源数)，无分配）。
pub fn diff(before: &RecoverySnapshot, after: &RecoverySnapshot) -> SnapshotDiff {
    let mut d = SnapshotDiff {
        device_usable_changed: before.device_usable != after.device_usable,
        ..SnapshotDiff::default()
    };
    // 队列维度：按并集遍历，after 更长时新增槽位也要算变化（漏算即静默）。
    let qlen = before.queues.len().max(after.queues.len());
    let mut i = 0;
    while i < qlen {
        let b = before.queues.get(i).copied();
        let a = after.queues.get(i).copied();
        match (b, a) {
            (Some(bq), Some(aq)) => {
                if bq.ready != aq.ready {
                    d.queue_ready_changed += 1;
                }
                if bq.pending != aq.pending {
                    d.queue_pending_changed += 1;
                }
                if bq.avail != aq.avail {
                    d.queue_avail_changed += 1;
                }
            }
            _ => {
                // 槽位新增或消失：该队列整体算状态变化（不算「干净」）。
                d.queue_ready_changed += 1;
            }
        }
        i += 1;
    }
    // 资源维度：missing / added / backing / attach。
    for r in before.resources.iter() {
        match after.resource(r.id) {
            None => d.resource_missing += 1,
            Some(ar) => {
                if ar.backing_pages != r.backing_pages {
                    d.resource_backing_changed += 1;
                }
                if ar.attached != r.attached {
                    d.resource_attach_changed += 1;
                }
            }
        }
    }
    for r in after.resources.iter() {
        if before.resource(r.id).is_none() {
            d.resource_added += 1;
        }
    }
    d
}

/// 资源表按设备真值重建的统计（三项计数公开——对齐不是静默的）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RebuildStat {
    /// 与真值一致而保留的条目数。
    pub kept: u16,
    /// 本地多出、被真值否定而丢弃的条目数。
    pub dropped: u16,
    /// 本地缺失、由真值补回的条目数。
    pub added: u16,
}

// ---------------------------------------------------------------------------
// 四、降级通知（F0103 通知协议对接面）
// ---------------------------------------------------------------------------

/// 通知构造拒绝原因（构造期拒绝，不给静默空文案留机会）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeReject {
    /// 「发生了什么」为空。
    EmptyWhat,
    /// 「为什么」为空。
    EmptyWhy,
    /// 「下一步」为空。
    EmptyNext,
}

/// 降级通知（锚点：恢复过程产出降级事件走 F0103 通知协议）。
///
/// 三要素强制非空：缺任一项构造失败（[`NoticeReject`]）——F0103 明确
/// 「文案模板缺失 → 退化为最小告知不静默」，退化也要三要素齐。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradeNotice {
    /// 通知类别（F0103 模板类别 = 错误类别标签）。
    pub category: u32,
    /// 严重级别（供 F0103 决定限频与呈现层级）。
    pub severity: Severity,
    /// 三要素之一：发生了什么（能力受限的具体表现）。
    pub what: String,
    /// 三要素之二：为什么（原因类别）。
    pub why: String,
    /// 三要素之三：下一步怎么办（可操作建议）。
    pub next: String,
    /// 恢复过程界面提示可关闭（无障碍面）。
    pub dismissible: bool,
    /// 产出时的逻辑 tick。
    pub tick: u64,
    /// 窗口内合并的同类通知数（合并限频的证据面）。
    pub merged: u32,
    /// 与 F0103 的契约版本。
    pub version: u32,
}

impl DegradeNotice {
    /// 构造（三要素缺任一项即拒——边界防护在源头）。
    pub fn new(
        category: u32,
        severity: Severity,
        what: String,
        why: String,
        next: String,
        tick: u64,
    ) -> Result<DegradeNotice, NoticeReject> {
        if what.is_empty() {
            return Err(NoticeReject::EmptyWhat);
        }
        if why.is_empty() {
            return Err(NoticeReject::EmptyWhy);
        }
        if next.is_empty() {
            return Err(NoticeReject::EmptyNext);
        }
        Ok(DegradeNotice {
            category,
            severity,
            what,
            why,
            next,
            // 恢复过程提示一律可关闭：用户不该被自己无法解除的提示挡住。
            dismissible: true,
            tick,
            merged: 0,
            version: NOTICE_LINK_VERSION,
        })
    }

    /// 读屏播报文本：三要素一次播全，**不截断**。
    ///
    /// 截断的后果不是「短一点」而是断因果：读屏用户听到「图形显示已降级」却
    /// 听不到「下一步怎么办」，等于没告知。
    pub fn screen_text(&self) -> String {
        format!(
            "图形显示已降级。发生了什么：{}。为什么：{}。下一步：{}。{}",
            self.what,
            self.why,
            self.next,
            if self.dismissible {
                "该提示可以关闭。"
            } else {
                "该提示需要重启图形会话才能关闭。"
            }
        )
    }

    /// 三要素是否齐（判据「通知联动」的第一道）。
    pub fn has_triplet(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }

    /// 同类合并：窗口内合并而非新增条目（限频），计数累加。
    pub fn absorb(&mut self, tick: u64) {
        self.merged += 1;
        // 合并后 tick 取较晚者：窗口以最新一次为准，避免旧 tick 让窗口立即过期。
        if tick > self.tick {
            self.tick = tick;
        }
    }
}

/// 降级通知的最小告知退化文案（F0103「模板缺失 → 退化为最小告知不静默」）。
///
/// 退化文案仍三要素齐：只丢细节，不丢「发生了什么 / 为什么 / 下一步」。
pub fn degraded_template(class: ErrClass, sev: Severity) -> (String, String, String) {
    let what = format!("图形显示出现{}，显示能力受限", class.name());
    let why = format!("原因类别：{}（未匹配到专用文案模板）", sev.name());
    let next = "建议重新启动图形会话；若反复出现，请将本次错误码提交给设备维护方。".to_string();
    (what, why, next)
}

// ---------------------------------------------------------------------------
// 五、reset 序（F0201 初始化序复用面 · 下游 F0217 单一事实源）
// ---------------------------------------------------------------------------

/// reset 序的一步。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetStep {
    /// 步骤名（与 F0201 `veb01_init::Stage` 同名，两处序必须一致）。
    pub name: &'static str,
    /// 该步的逻辑成本（微秒；与 F0201 成本模型同量纲，供恢复耗时核算）。
    pub cost_us: u32,
}

/// 单步成本（对齐 F0201 成本模型常量，恢复路径复用同一账本）。
pub const STEP_COST_RESET_US: u32 = 2_000;
pub const STEP_COST_ACK_US: u32 = 100;
pub const STEP_COST_DRIVER_US: u32 = 100;
pub const STEP_COST_FEATURES_US: u32 = 500;
pub const STEP_COST_FEATURES_OK_US: u32 = 200;
pub const STEP_COST_QUEUES_US: u32 = 500;
pub const STEP_COST_DRIVER_OK_US: u32 = 300;

/// 整设备 reset 序（锚点「设备坏 → 整设备 reset 重走初始化序」）。
///
/// 与 F0201 初始化序逐字同序：F0217 热重置复用本序，两处序不一致会导致
/// 热重置后的设备行为与冷启动不同（这是最难查的一类偏差）。
pub const RESET_SEQUENCE: [ResetStep; 7] = [
    ResetStep {
        name: "reset",
        cost_us: STEP_COST_RESET_US,
    },
    ResetStep {
        name: "acknowledge",
        cost_us: STEP_COST_ACK_US,
    },
    ResetStep {
        name: "set_driver",
        cost_us: STEP_COST_DRIVER_US,
    },
    ResetStep {
        name: "negotiate_features",
        cost_us: STEP_COST_FEATURES_US,
    },
    ResetStep {
        name: "features_ok",
        cost_us: STEP_COST_FEATURES_OK_US,
    },
    ResetStep {
        name: "setup_queues",
        cost_us: STEP_COST_QUEUES_US,
    },
    ResetStep {
        name: "driver_ok",
        cost_us: STEP_COST_DRIVER_OK_US,
    },
];

/// 整设备 reset 序的总逻辑成本。
pub const RESET_SEQUENCE_COST_US: u32 = 3_700;

/// reset 序步数。
pub const RESET_SEQUENCE_STEPS: usize = 7;

// ---------------------------------------------------------------------------
// 六、恢复引擎
// ---------------------------------------------------------------------------

/// 恢复结果（每条路径都有显式返回值，不靠计数反推）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// 丢弃该命令续跑（该队列剩余待处理命令数）。
    CommandDropped { queue: u16, remaining: u32 },
    /// 重置该队列（队列下标）。
    QueueReset { queue: u16 },
    /// 按设备真值重建资源表。
    ResourceTableRebuilt { stat: RebuildStat },
    /// 整设备 reset 并重走初始化序（已走步数、总成本）。
    DeviceReinit { steps: u8, cost_us: u32 },
    /// 标记设备不可用并通知（恢复链终止）。
    MarkedUnusable,
    /// 撞全局恢复硬顶，恢复链终止（防打转第二级）。
    Capped,
    /// 本次动作未达重试上限，但已达同级上限故升级级别后重试（`attempt` 为
    /// 新级别的第几次尝试，从 1 起）。
    EscalatedRetrying { severity: Severity, attempt: u32 },
    /// 设备已不可用，拒绝再恢复（防打转第三级）。
    RefusedUnusable,
    /// 静默状态错乱：动作造成的状态变化超出其允许面，**恢复不算成功**。
    SilentCorruption { violation: Violation },
}

/// 静默状态错乱的违例面（快照比对判定的具体落点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Violation {
    /// 丢命令动作却改动了队列就绪 / 可用环。
    CommandDropTouchedQueue,
    /// 丢命令动作却改动了资源表。
    CommandDropTouchedResource,
    /// 丢命令动作未减命中队列的待处理数（命令没丢成，却报成功）。
    CommandDropDidNotDrop,
    /// 队列复位后该队列未回到就绪初值。
    QueueNotReset,
    /// 队列复位却改动了其他队列的状态。
    QueueResetTouchedOthers,
    /// 队列复位却改动了资源表。
    QueueResetTouchedResource,
    /// 资源表重建后与设备真值仍不一致。
    ResourceTableNotRebuilt,
    /// 资源表重建却改动了队列状态。
    RebuildTouchedQueue,
    /// 整设备复位后仍有队列未回到就绪初值。
    DeviceReinitIncomplete,
    /// 整设备复位后资源表与设备真值不一致。
    DeviceReinitResourceMismatch,
    /// 动作不允许改资源表却改了（分类与动作不匹配）。
    UnexpectedResourceChange,
    /// 动作不允许改队列却改了。
    UnexpectedQueueChange,
}

impl Violation {
    /// 违例名（诊断与判据失败信息用；必须指向具体落点）。
    pub fn name(self) -> &'static str {
        match self {
            Violation::CommandDropTouchedQueue => "丢命令动作改动了队列状态",
            Violation::CommandDropTouchedResource => "丢命令动作改动了资源表",
            Violation::CommandDropDidNotDrop => "丢命令动作未减待处理数",
            Violation::QueueNotReset => "队列复位未回到就绪初值",
            Violation::QueueResetTouchedOthers => "队列复位改动了其他队列",
            Violation::QueueResetTouchedResource => "队列复位改动了资源表",
            Violation::ResourceTableNotRebuilt => "资源表重建后仍与真值不一致",
            Violation::RebuildTouchedQueue => "资源表重建改动了队列状态",
            Violation::DeviceReinitIncomplete => "整设备复位后队列未全部复位",
            Violation::DeviceReinitResourceMismatch => "整设备复位后资源表与真值不一致",
            Violation::UnexpectedResourceChange => "不该改资源表却改了",
            Violation::UnexpectedQueueChange => "不该改队列却改了",
        }
    }
}

/// 恢复计数（遥测面：全部 pub，不静默丢）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecoveryStats {
    /// 恢复总次数（含各处置动作）。
    pub recoveries: u64,
    /// 丢弃命令续跑次数。
    pub command_drops: u64,
    /// 队列复位次数。
    pub queue_resets: u64,
    /// 整设备 reset 次数。
    pub device_resets: u64,
    /// 资源表重建次数。
    pub resource_rebuilds: u64,
    /// 级别升级次数。
    pub escalations: u64,
    /// 快照取样次数（**正常路径应恒为 0**——快照只在恢复时取）。
    pub snapshots_taken: u64,
    /// 快照比对次数（每次恢复一次前后两次取样 → 一次比对）。
    pub diffs_computed: u64,
    /// 静默状态错乱次数（快照比对判红）。
    pub silent_corruption: u64,
    /// 未知错误码次数。
    pub unknown_classes: u64,
    /// 撞全局硬顶次数。
    pub capped_total: u64,
    /// 设备已不可用后被拒绝的恢复请求数。
    pub recoveries_refused: u64,
    /// 标记设备不可用次数。
    pub marked_unusable: u64,
    /// 通知产出数（含被合并者）。
    pub notices_emitted: u64,
    /// 通知合并次数。
    pub notices_merged: u64,
    /// 通知通道故障而进入待补投队列的次数。
    pub notices_pending: u64,
    /// 待补投通知补投成功次数。
    pub notices_replayed: u64,
    /// 队列下标越界拒收次数。
    pub rejected_queue: u64,
    /// 设备真值超容截断次数（防止异常真值膨胀拖垮恢复路径）。
    pub truth_overflow: u64,
    /// 连最小告知兜底都构造不出而放弃的通知数（恒 0，但必须可见——
    /// 不写这一项则「通知静默丢失」无任何痕迹）。
    pub notice_construction_failed: u64,
}

/// virtio-gpu 错误恢复引擎。
#[derive(Clone, Debug)]
pub struct RecoveryEngine {
    /// 队列状态表（按队列下标定长；未启用槽位 `role=Control` 且就绪为假）。
    queues: [QueueState; QUEUE_SLOTS],
    /// 已启用队列数（`queues[..count]` 有效）。
    queue_count: u8,
    /// 资源表（按 id 直接索引槽，冲突线性探测）。
    resources: [ResourceSlot; RESOURCE_SLOTS],
    /// 资源表已用槽数。
    resource_count: u16,
    /// 设备真值源（资源表重建的依据；由设备侧资源枚举或本地真值表提供）。
    device_truth: Vec<ResourceRow>,
    /// 降级通知环。
    notices: [Option<DegradeNotice>; NOTICE_SLOTS],
    notice_head: usize,
    notice_tail: usize,
    notice_len: usize,
    /// 通知通道是否可用（false 时通知入待补投队列，F0103「通道故障 → 记入
    /// 日志恢复后补投」）。
    channel_ok: bool,
    /// 待补投通知（通道故障期间积压；恢复后补投）。
    pending: Vec<DegradeNotice>,
    /// 同级重试计数（按 [`Severity::idx`]）。
    retry: [u32; SEVERITY_SLOTS],
    /// 恢复总次数（全局硬顶判据）。
    total_attempts: u32,
    /// 设备是否可用（false = 已被标记不可用，恢复链终止）。
    device_usable: bool,
    /// 恢复计数。
    pub stats: RecoveryStats,
}

/// 单队列的活状态（快照的来源；与快照行 [`QueueRow`] 分离以免快照面被写）。
///
/// `pub` 是因为 [`RecoveryEngine::queue`] 是公开读面——`pub fn` 返回私有类型
/// 会让调用方拿不到结果（E0446），且队列状态本就该可被上层只读查询。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueState {
    index: u16,
    role: QueueRole,
    ready: bool,
    avail: u16,
    pending: u32,
    enabled: bool,
}

impl QueueState {
    const fn empty(index: u16) -> QueueState {
        QueueState {
            index,
            role: QueueRole::Control,
            ready: false,
            avail: AVAIL_AFTER_RESET,
            pending: PENDING_AFTER_RESET,
            enabled: false,
        }
    }

    const fn as_row(&self) -> QueueRow {
        QueueRow {
            index: self.index,
            role: self.role,
            ready: self.ready,
            avail: self.avail,
            pending: self.pending,
        }
    }
}

/// 资源表槽（定长直接索引；空槽 `used=false`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResourceSlot {
    row: ResourceRow,
    used: bool,
}

impl ResourceSlot {
    const fn empty() -> ResourceSlot {
        ResourceSlot {
            row: ResourceRow {
                id: 0,
                backing_pages: 0,
                attached: false,
            },
            used: false,
        }
    }
}

impl RecoveryEngine {
    /// 构造：启用 `queue_count` 个队列（上限 [`QUEUE_SLOTS`]），资源表取
    /// `initial` 作为本地当前表。
    pub fn new(queue_count: u8, initial: &[ResourceRow]) -> RecoveryEngine {
        let count = (queue_count as usize).min(QUEUE_SLOTS);
        let mut queues = [QueueState::empty(0); QUEUE_SLOTS];
        let mut i = 0;
        while i < QUEUE_SLOTS {
            let enabled = i < count;
            let role = match i {
                0 => QueueRole::Control,
                1 => QueueRole::Cursor,
                _ => QueueRole::Render,
            };
            queues[i] = QueueState {
                index: i as u16,
                role,
                ready: enabled,
                avail: AVAIL_AFTER_RESET,
                pending: PENDING_AFTER_RESET,
                enabled,
            };
            i += 1;
        }
        let mut e = RecoveryEngine {
            queues,
            queue_count: count as u8,
            resources: [ResourceSlot::empty(); RESOURCE_SLOTS],
            resource_count: 0,
            device_truth: Vec::new(),
            notices: [const { None }; NOTICE_SLOTS],
            notice_head: 0,
            notice_tail: 0,
            notice_len: 0,
            channel_ok: true,
            pending: Vec::new(),
            retry: [0; SEVERITY_SLOTS],
            total_attempts: 0,
            device_usable: true,
            stats: RecoveryStats::default(),
        };
        // 构造时本地表 = 传入的初始表，同时作为初始真值（无外部真值源时
        // 最诚实的假设：此刻两侧一致，之后的差异由驱动侧变动产生）。
        e.set_device_truth(initial);
        let seed = e.device_truth.clone();
        e.load_resources_from(&seed);
        e
    }

    // ---- 配置面 ----

    /// 设定设备真值源（超 [`SNAPSHOT_MAX_RESOURCES`] 截断并计数，不静默）。
    ///
    /// **只设真值，不动本地表**——这是本单元能发现资源表漂移的前提。若这里
    ///顺手把真值灌进本地表，漂移就被当场抹平，`ResourceDrift` 分支与快照比对
    /// 永远无事可做（自证式修复＝无门禁）。本地表的变动一律走
    /// [`RecoveryEngine::apply_local_resource`] / [`RecoveryEngine::remove_local_resource`]。
    pub fn set_device_truth(&mut self, truth: &[ResourceRow]) {
        let n = truth.len().min(SNAPSHOT_MAX_RESOURCES);
        if truth.len() > SNAPSHOT_MAX_RESOURCES {
            self.stats.truth_overflow += (truth.len() - SNAPSHOT_MAX_RESOURCES) as u64;
        }
        self.device_truth.clear();
        let mut i = 0;
        while i < n {
            self.device_truth.push(truth[i]);
            i += 1;
        }
    }

    /// 本地表登记一条资源（模拟驱动侧资源创建 / 本地记账变动）。
    ///
    /// 返回是否登记成功（表满拒收返回 false 并计数——不静默丢）。
    pub fn apply_local_resource(&mut self, row: ResourceRow) -> bool {
        self.insert_resource(row)
    }

    /// 本地表移除一条资源（模拟资源销毁）。返回是否确实移除过。
    pub fn remove_local_resource(&mut self, id: u32) -> bool {
        let start = (id as usize) % RESOURCE_SLOTS;
        let mut step = 0;
        while step < RESOURCE_SLOTS {
            let idx = (start + step) % RESOURCE_SLOTS;
            if !self.resources[idx].used {
                return false;
            }
            if self.resources[idx].row.id == id {
                self.resources[idx] = ResourceSlot::empty();
                self.resource_count -= 1;
                return true;
            }
            step += 1;
        }
        false
    }

    /// 队列提交命令：置就绪位并累加待处理数（模拟驱动侧命令提交）。
    ///
    /// **必须有这条写面**，否则队列恒处于复位初值，「丢命令要减 pending」
    /// 与「队列复位不许波及其他队列」两条判据都无从观测——被测状态不可达
    /// 的判据等于没有判据（自检里只能验「本来就是这样」，恒真）。
    pub fn submit_commands(&mut self, index: u16, count: u32) -> bool {
        if !self.queue_valid(index) {
            self.stats.rejected_queue += 1;
            return false;
        }
        let slot = &mut self.queues[index as usize];
        slot.pending = slot.pending.saturating_add(count);
        slot.avail = slot.avail.wrapping_add(count as u16);
        true
    }

    /// 通知通道开关（F0103「通知通道故障 → 记入日志恢复后补投」）。
    pub fn set_channel_ok(&mut self, ok: bool) {
        self.channel_ok = ok;
    }

    pub fn channel_ok(&self) -> bool {
        self.channel_ok
    }

    pub fn device_usable(&self) -> bool {
        self.device_usable
    }

    /// 设备真值条目数。
    pub fn truth_count(&self) -> usize {
        self.device_truth.len()
    }

    /// 本地资源表条目数。
    pub fn resource_count(&self) -> u16 {
        self.resource_count
    }

    /// 队列下标是否合法。
    pub fn queue_valid(&self, index: u16) -> bool {
        (index as usize) < QUEUE_SLOTS && (index as usize) < self.queue_count as usize
    }

    // ---- 状态读面 ----

    /// 取队列活状态（越界返回 `None`，不 panic）。
    pub fn queue(&self, index: u16) -> Option<QueueState> {
        if !self.queue_valid(index) {
            return None;
        }
        Some(self.queues[index as usize])
    }

    /// 取资源条目（线性探测；未命中返回 `None`）。
    pub fn resource(&self, id: u32) -> Option<ResourceRow> {
        let start = (id as usize) % RESOURCE_SLOTS;
        let mut step = 0;
        while step < RESOURCE_SLOTS {
            let idx = (start + step) % RESOURCE_SLOTS;
            let slot = self.resources[idx];
            if !slot.used {
                return None;
            }
            if slot.row.id == id {
                return Some(slot.row);
            }
            step += 1;
        }
        None
    }

    /// 资源表是否与设备真值逐条一致（重建判据）。
    pub fn resources_match_truth(&self) -> bool {
        if self.resource_count as usize != self.device_truth.len() {
            return false;
        }
        let mut i = 0;
        while i < self.device_truth.len() {
            match self.resource(self.device_truth[i].id) {
                None => return false,
                Some(r) => {
                    if r.backing_pages != self.device_truth[i].backing_pages
                        || r.attached != self.device_truth[i].attached
                    {
                        return false;
                    }
                }
            }
            i += 1;
        }
        true
    }

    /// 全部队列是否处于复位初值。
    pub fn all_queues_at_reset(&self) -> bool {
        let mut i = 0;
        while i < self.queue_count as usize {
            let q = self.queues[i];
            if !q.enabled {
                i += 1;
                continue;
            }
            if !q.ready || q.avail != AVAIL_AFTER_RESET || q.pending != PENDING_AFTER_RESET {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 整设备 reset 序（下游 F0217 复用面；本单元不另立一套序）。
    pub fn reinit_sequence() -> &'static [ResetStep] {
        &RESET_SEQUENCE
    }

    // ---- 快照面 ----

    /// 取恢复快照（**只在恢复路径调用**；正常路径零开销）。
    ///
    /// 资源条目按 [`SNAPSHOT_MAX_RESOURCES`] 夹紧：设备真值异常膨胀时快照
    /// 不得连带膨胀（恢复路径的内存是被最坏情况约束的）。
    pub fn snapshot(&mut self, tick: u64) -> RecoverySnapshot {
        self.stats.snapshots_taken += 1;
        let qlen = self.queue_count as usize;
        let mut queues: Vec<QueueRow> = Vec::with_capacity(qlen);
        let mut i = 0;
        while i < qlen {
            queues.push(self.queues[i].as_row());
            i += 1;
        }
        let mut resources: Vec<ResourceRow> = Vec::new();
        let mut s = 0;
        while s < RESOURCE_SLOTS && resources.len() < SNAPSHOT_MAX_RESOURCES {
            if self.resources[s].used {
                resources.push(self.resources[s].row);
            }
            s += 1;
        }
        RecoverySnapshot {
            tick,
            queues,
            resources,
            device_usable: self.device_usable,
        }
    }

    /// 纯比对（不更新计数；供自检与诊断复算）。
    pub fn compare(&mut self, before: &RecoverySnapshot, after: &RecoverySnapshot) -> SnapshotDiff {
        self.stats.diffs_computed += 1;
        diff(before, after)
    }

    // ---- 恢复主流程 ----

    /// 按错误类别分级处置（锚点主流程）。
    ///
    /// 处置顺序刻意为「可用性闸 → 全局硬顶 → 分级 → 升级 → 执行 → 快照比对
    /// → 通知」：越廉价的判据越前置，恢复路径不做无用功。
    pub fn recover(&mut self, class: ErrClass, tick: u64) -> RecoveryOutcome {
        // 0) 设备已不可用：拒绝再恢复（防打转第三级）。不静默——拒绝计数公开。
        if !self.device_usable {
            self.stats.recoveries_refused += 1;
            return RecoveryOutcome::RefusedUnusable;
        }
        // 1) 全局硬顶：所有级别累计超顶即终止（防打转第二级）。
        if self.total_attempts >= RECOVERY_TOTAL_CAP {
            self.stats.capped_total += 1;
            self.mark_unusable(class, tick);
            return RecoveryOutcome::Capped;
        }
        if matches!(class, ErrClass::Unknown(_)) {
            self.stats.unknown_classes += 1;
        }
        self.total_attempts += 1;
        self.stats.recoveries += 1;

        // 2) 分级 + 3) 升级：同级重试达上限即升一级，动作按新级别**重算**。
        let mut sev = class.severity();
        let mut escalated = false;
        if self.retry[sev.idx()] >= RECOVERY_RETRY_LIMIT {
            let up = sev.escalate();
            if up != sev {
                self.stats.escalations += 1;
                sev = up;
                escalated = true;
            }
        }
        // 升级到 Fatal 即终止恢复链（反复损坏 → 标记不可用并通知）。
        if sev == Severity::Fatal {
            self.retry[sev.idx()] = self.retry[sev.idx()].saturating_add(1);
            self.mark_unusable(class, tick);
            return RecoveryOutcome::MarkedUnusable;
        }
        self.retry[sev.idx()] = self.retry[sev.idx()].saturating_add(1);
        let kind = class.action_for(sev);
        let attempt = self.retry[sev.idx()];

        // 4) 执行：前快照 → 动作 → 后快照 → 比对。
        let target = match class {
            ErrClass::QueueCorrupt(q) => Some(q),
            _ => Some(0),
        };
        let before = self.snapshot(tick);
        let out = self.apply(kind, class, target, tick);
        let after = self.snapshot(tick);
        let d = self.compare(&before, &after);

        // 5) 快照比对判据：动作造成的状态变化必须在该动作允许面内，否则判
        //    **静默状态错乱**——恢复不算成功（不报成功、不静默放过）。
        if let Some(v) = self.violation_of(kind, target, &before, &after, &d) {
            self.stats.silent_corruption += 1;
            let (_what, why, next) = degraded_template(class, sev);
            let vname = v.name();
            self.emit_notice(
                DegradeNotice::new(
                    class.tag(),
                    sev,
                    format!("恢复过程中检测到状态异常：{}，该次恢复未生效", vname),
                    format!("{}；快照比对判定恢复动作越界", why),
                    format!("{}；建议重走整设备初始化序", next),
                    tick,
                ),
                class,
                sev,
                tick,
            );
            return RecoveryOutcome::SilentCorruption { violation: v };
        }

        // 6) 通知联动（F0103 三要素）。构造结果直接交 `emit_notice`——它内部
        //    统一负责「退化为最小告知」这一条兜底路径，此处不重复兜底，
        //    也不因通知构造失败而改写恢复结果（恢复已生效，不被通知拖累）。
        let (what, why, next) = self.notice_text(class, sev, kind, &out);
        let constructed = DegradeNotice::new(class.tag(), sev, what, why, next, tick);
        self.emit_notice(constructed, class, sev, tick);

        if escalated {
            return RecoveryOutcome::EscalatedRetrying {
                severity: sev,
                attempt,
            };
        }
        out
    }

    /// 执行处置动作（不含快照与比对——那由 `recover` 编排）。
    fn apply(
        &mut self,
        kind: ActionKind,
        class: ErrClass,
        target: Option<u16>,
        tick: u64,
    ) -> RecoveryOutcome {
        let _ = tick;
        match kind {
            ActionKind::DropCommandContinue => {
                let q = target.unwrap_or(0);
                if !self.queue_valid(q) {
                    self.stats.rejected_queue += 1;
                    // 队列号非法：不能假装丢成功。不静默——本次不计丢弃。
                    return RecoveryOutcome::CommandDropped {
                        queue: q,
                        remaining: 0,
                    };
                }
                let slot = &mut self.queues[q as usize];
                // 减一不致下溢：无待处理命令时保持 0（无可丢不是错误）。
                slot.pending = slot.pending.saturating_sub(1);
                self.stats.command_drops += 1;
                RecoveryOutcome::CommandDropped {
                    queue: q,
                    remaining: slot.pending,
                }
            }
            ActionKind::ResetQueue => {
                let q = target.unwrap_or(0);
                if !self.queue_valid(q) {
                    self.stats.rejected_queue += 1;
                    return RecoveryOutcome::QueueReset { queue: q };
                }
                self.reset_one_queue(q);
                self.stats.queue_resets += 1;
                RecoveryOutcome::QueueReset { queue: q }
            }
            ActionKind::RebuildResourceTable => {
                let stat = self.rebuild_from_truth();
                self.stats.resource_rebuilds += 1;
                RecoveryOutcome::ResourceTableRebuilt { stat }
            }
            ActionKind::ResetDeviceReinit => {
                self.reset_all_queues();
                self.rebuild_from_truth();
                self.stats.device_resets += 1;
                RecoveryOutcome::DeviceReinit {
                    steps: RESET_SEQUENCE_STEPS as u8,
                    cost_us: RESET_SEQUENCE_COST_US,
                }
            }
            ActionKind::MarkUnusable => {
                self.mark_unusable(class, tick);
                RecoveryOutcome::MarkedUnusable
            }
        }
    }

    /// 快照比对 → 违例（`None` = 比对通过）。
    ///
    /// 期望面按动作逐条写死：动作「被允许造成什么」与「实际造成了什么」不符
    /// 即违例。分级处置的语义全落在这里——丢命令不许碰队列/资源表，复位队列
    /// 不许碰其他队列/资源表，重建资源表不许碰队列，整设备复位必须两边都对。
    ///
    /// **判定只读入参快照与 diff，不读引擎活状态**——快照比对判据的意义就在
    /// 「比对前后两份快照」；若偷偷回看活状态，判据就成了「看现在对不对」，
    /// 于是自检可以喂一份与活状态不符的快照而依旧通过（假绿）。终态判据
    /// （资源表是否等于真值）改为在 `after` 快照上判：真值逐条查 `after`
    /// 是否存在且记账一致。
    fn violation_of(
        &self,
        kind: ActionKind,
        target: Option<u16>,
        before: &RecoverySnapshot,
        after: &RecoverySnapshot,
        d: &SnapshotDiff,
    ) -> Option<Violation> {
        match kind {
            ActionKind::DropCommandContinue => {
                if d.resource_touched() {
                    return Some(Violation::CommandDropTouchedResource);
                }
                // 队列侧：只允许「待处理数减少」，就绪位与可用环不得动。
                if d.queue_ready_changed != 0 || d.queue_avail_changed != 0 {
                    return Some(Violation::CommandDropTouchedQueue);
                }
                let q = target.unwrap_or(0);
                let b = before.queue(q)?;
                let a = after.queue(q)?;
                if a.pending >= b.pending && b.pending > 0 {
                    return Some(Violation::CommandDropDidNotDrop);
                }
                None
            }
            ActionKind::ResetQueue => {
                let q = target.unwrap_or(0);
                // 命中队列必须回到就绪初值。
                match after.queue(q) {
                    None => return Some(Violation::QueueNotReset),
                    Some(a) => {
                        if !a.ready
                            || a.avail != AVAIL_AFTER_RESET
                            || a.pending != PENDING_AFTER_RESET
                        {
                            return Some(Violation::QueueNotReset);
                        }
                    }
                }
                // 其他队列不得被动（命中队列自身的复位不算「被动」）。
                let mut i = 0;
                while i < before.queues.len() {
                    if before.queues[i].index == q {
                        i += 1;
                        continue;
                    }
                    let bq = before.queues[i];
                    match after.queue(bq.index) {
                        None => return Some(Violation::QueueResetTouchedOthers),
                        Some(aq) => {
                            if aq.ready != bq.ready
                                || aq.avail != bq.avail
                                || aq.pending != bq.pending
                            {
                                return Some(Violation::QueueResetTouchedOthers);
                            }
                        }
                    }
                    i += 1;
                }
                if d.resource_touched() {
                    return Some(Violation::QueueResetTouchedResource);
                }
                None
            }
            ActionKind::RebuildResourceTable => {
                if !snapshot_matches_truth(after, &self.device_truth) {
                    return Some(Violation::ResourceTableNotRebuilt);
                }
                // 资源表重建不许动队列（一个字节都不许）。
                if d.queue_touched() {
                    return Some(Violation::RebuildTouchedQueue);
                }
                None
            }
            ActionKind::ResetDeviceReinit => {
                if !snapshot_queues_at_reset(after) {
                    return Some(Violation::DeviceReinitIncomplete);
                }
                if !snapshot_matches_truth(after, &self.device_truth) {
                    return Some(Violation::DeviceReinitResourceMismatch);
                }
                None
            }
            ActionKind::MarkUnusable => {
                if d.resource_touched() || d.queue_touched() {
                    return Some(Violation::UnexpectedQueueChange);
                }
                None
            }
        }
    }

    /// 快照比对判定的公开只读探针（自检用；不更新任何计数、不改任何状态）。
    ///
    /// 存在的理由：`violation_of` 是私有实现面，自检若只能经 `recover` 触发
    /// 就无法构造「动作越界」这类**需要手工造快照**的违例——而这些恰是判据
    /// 「快照比对防静默状态错乱」最关键的落点。探针只暴露判定，不暴露写面。
    pub fn violation_probe(
        &self,
        kind: ActionKind,
        target: Option<u16>,
        before: &RecoverySnapshot,
        after: &RecoverySnapshot,
        d: &SnapshotDiff,
    ) -> Option<Violation> {
        self.violation_of(kind, target, before, after, d)
    }

    // ---- 动作原语 ----

    /// 复位单个队列（就绪 + 可用环 + 待处理数回到初值）。
    fn reset_one_queue(&mut self, index: u16) {
        if !self.queue_valid(index) {
            return;
        }
        let slot = &mut self.queues[index as usize];
        slot.ready = QUEUE_READY_AFTER_RESET;
        slot.avail = AVAIL_AFTER_RESET;
        slot.pending = PENDING_AFTER_RESET;
    }

    /// 复位全部已启用队列。
    fn reset_all_queues(&mut self) {
        let mut i = 0;
        while i < self.queue_count as usize {
            self.reset_one_queue(i as u16);
            i += 1;
        }
    }

    /// 按设备真值重建资源表（kept / dropped / added 三项计数公开）。
    ///
    /// 三项定义互斥且穷尽（`kept + dropped + added == 本地原条目数 + 真值
    /// 条目数 - kept` 不成立，但 `dropped == 本地有真值无`、`added == 真值有
    /// 本地无`、`kept == 两边都有且记账一致` 三者各自精确）：
    /// - `kept`：真值有、本地有、且 backing 页数与挂载标志都一致；
    /// - `dropped`：本地有、真值没有（本地多出的幽灵资源）；
    /// - `added`：真值有、本地没有（或本地记账与真值不符而须改写）。
    fn rebuild_from_truth(&mut self) -> RebuildStat {
        let mut stat = RebuildStat::default();
        // 真值与本地表都要留一份快照式副本：装载时要改写 self，与 self 的
        // 不可变借用不能重叠（借用检查器拒绝 self.load(&self.x)）。
        let truth = self.device_truth.clone();
        let local = self.local_resources();
        // kept / added：按真值逐条对本地。
        let mut i = 0;
        while i < truth.len() {
            match self.resource(truth[i].id) {
                None => stat.added += 1,
                Some(cur) => {
                    if cur.backing_pages == truth[i].backing_pages
                        && cur.attached == truth[i].attached
                    {
                        stat.kept += 1;
                    } else {
                        // 本地有这条但记账不符：属「须改写」，计入 added
                        // （改的是本地这一侧的记账，故不是 kept）。
                        stat.added += 1;
                    }
                }
            }
            i += 1;
        }
        // dropped：按本地逐条对真值（线性对，总量 O(本地×真值)——资源表
        // 上界 64，恢复路径可接受；常态下真值与本地同长）。
        let mut j = 0;
        while j < local.len() {
            if !truth.iter().any(|t| t.id == local[j].id) {
                stat.dropped += 1;
            }
            j += 1;
        }
        self.load_resources_from(&truth);
        stat
    }

    /// 把一份资源表装入本地表（清空后按 id 索引装载）。
    fn load_resources_from(&mut self, rows: &[ResourceRow]) {
        let mut s = 0;
        while s < RESOURCE_SLOTS {
            self.resources[s] = ResourceSlot::empty();
            s += 1;
        }
        self.resource_count = 0;
        let n = rows.len().min(RESOURCE_SLOTS);
        let mut i = 0;
        while i < n {
            self.insert_resource(rows[i]);
            i += 1;
        }
    }

    /// 插入一条资源（线性探测；表满则丢弃并计数——不静默）。
    fn insert_resource(&mut self, row: ResourceRow) -> bool {
        let start = (row.id as usize) % RESOURCE_SLOTS;
        let mut step = 0;
        while step < RESOURCE_SLOTS {
            let idx = (start + step) % RESOURCE_SLOTS;
            if !self.resources[idx].used {
                self.resources[idx] = ResourceSlot { row, used: true };
                self.resource_count += 1;
                return true;
            }
            if self.resources[idx].row.id == row.id {
                self.resources[idx].row = row;
                return true;
            }
            step += 1;
        }
        self.stats.truth_overflow += 1;
        false
    }

    /// 本地资源表全量列出（按槽序）。
    fn local_resources(&self) -> Vec<ResourceRow> {
        let mut out = Vec::new();
        let mut s = 0;
        while s < RESOURCE_SLOTS {
            if self.resources[s].used {
                out.push(self.resources[s].row);
            }
            s += 1;
        }
        out
    }

    /// 标记设备不可用并通知（锚点「反复损坏 → 标记设备不可用并通知」）。
    fn mark_unusable(&mut self, class: ErrClass, tick: u64) {
        self.device_usable = false;
        self.stats.marked_unusable += 1;
        let what = format!(
            "图形设备已停止响应（{}），显示功能需要重新初始化",
            class.name()
        );
        let why = format!(
            "原因类别：{}；同类恢复反复失败已达重试上限",
            Severity::Fatal.name()
        );
        let next = "建议重新启动图形会话以重新初始化设备；在此之前不会自动重试。".to_string();
        let notice = DegradeNotice::new(class.tag(), Severity::Fatal, what, why, next, tick);
        self.emit_notice(notice, class, Severity::Fatal, tick);
    }

    // ---- 通知面（F0103） ----

    /// 恢复动作 → 通知三要素文案（模板命中走专用文案，未命中走最小告知）。
    fn notice_text(
        &self,
        class: ErrClass,
        sev: Severity,
        kind: ActionKind,
        out: &RecoveryOutcome,
    ) -> (String, String, String) {
        match *out {
            RecoveryOutcome::CommandDropped { queue, remaining } => (
                format!(
                    "已跳过 {} 上 1 条无法解析的图形命令，该队列仍有 {} 条待处理",
                    queue_role_name(queue),
                    remaining
                ),
                format!("{}：{}", sev.name(), class.name()),
                "图形显示可继续使用；若同一队列反复跳过命令，请重新初始化设备。".to_string(),
            ),
            RecoveryOutcome::QueueReset { queue } => (
                format!(
                    "{} 已被重置，其上待处理的图形命令需重新提交",
                    queue_role_name(queue)
                ),
                format!("{}：{}", sev.name(), class.name()),
                "受影响窗口可能需要手动刷新一次以重新获得画面。".to_string(),
            ),
            RecoveryOutcome::ResourceTableRebuilt { stat } => (
                format!(
                    "图形资源表已按设备实际内容重建（保留 {} 项，丢弃 {} 项，补回 {} 项）",
                    stat.kept, stat.dropped, stat.added
                ),
                format!("{}：{}", sev.name(), class.name()),
                "资源映射已自动纠正，无需人工干预。".to_string(),
            ),
            RecoveryOutcome::DeviceReinit { steps, cost_us } => (
                format!(
                    "图形设备已重置并重走初始化序（{} 步，约 {} 微秒逻辑成本）",
                    steps, cost_us
                ),
                format!("{}：{}", sev.name(), class.name()),
                "桌面内容将重新绘制；请稍候，必要时切换一次窗口以触发重绘。".to_string(),
            ),
            _ => {
                // 兜底文案仍三要素齐（F0103：模板缺失退化为最小告知不静默）。
                let _ = kind;
                degraded_template(class, sev)
            }
        }
    }

    /// 通知入环（同类同窗口合并限频；通道故障入待补投队列）。
    ///
    /// 入参 `Result` 是**有意的**：三要素构造在源头被拒（`NoticeReject`），
    /// 本函数绝不 `expect` 也不 `unwrap`——构造失败时退回最小告知文案，
    /// 仍失败则记入 `notice_construction_failed` 并如实返回。生产代码零
    /// panic 面：通知出不了问题，但**通知丢了要可见**。
    fn emit_notice(
        &mut self,
        constructed: Result<DegradeNotice, NoticeReject>,
        class: ErrClass,
        sev: Severity,
        tick: u64,
    ) {
        let notice = match constructed {
            Ok(n) => n,
            Err(_) => {
                // 最小告知兜底（F0103：文案模板缺失 → 退化为最小告知不静默）。
                let (w, y, n2) = degraded_template(class, sev);
                match DegradeNotice::new(class.tag(), sev, w, y, n2, tick) {
                    Ok(n) => n,
                    Err(_) => {
                        // 连退化文案都构造不出（三要素恒非空故实际不可达）：
                        // 如实计数并放弃本条通知，不静默也不 panic。
                        self.stats.notice_construction_failed += 1;
                        return;
                    }
                }
            }
        };
        self.stats.notices_emitted += 1;
        // 合并限频：环内找同类且在窗口内 → 合并计数，不新增条目。
        // 先定位（不可变借用）再改写（可变借用）——两步走，避免同时借用。
        let mut hit_idx: Option<usize> = None;
        let mut i = 0;
        while i < self.notice_len {
            let idx = (self.notice_head + i) % NOTICE_SLOTS;
            let same = match &self.notices[idx] {
                Some(n) => {
                    n.category == notice.category
                        && tick.saturating_sub(n.tick) < NOTICE_MERGE_WINDOW
                }
                None => false,
            };
            if same {
                hit_idx = Some(idx);
                break;
            }
            i += 1;
        }
        if let Some(idx) = hit_idx {
            if let Some(n) = self.notices[idx].as_mut() {
                n.absorb(tick);
            }
            self.stats.notices_merged += 1;
            return;
        }
        if self.channel_ok {
            // 通道正常：入环。环满覆盖最旧并计数（最新优先，与光标通道同一
            // 保序哲学，不静默丢）。
            if self.notice_len == NOTICE_SLOTS {
                self.notice_head = (self.notice_head + 1) % NOTICE_SLOTS;
                self.notice_len -= 1;
            }
            self.notices[self.notice_tail] = Some(notice);
            self.notice_tail = (self.notice_tail + 1) % NOTICE_SLOTS;
            self.notice_len += 1;
        } else {
            // 通道故障：入待补投队列（F0103：记入日志恢复后补投，不静默丢）。
            self.pending.push(notice);
            self.stats.notices_pending += 1;
        }
    }

    /// 取走全部已入环通知（消费者侧）。
    pub fn drain_notices(&mut self) -> Vec<DegradeNotice> {
        let mut out = Vec::new();
        while self.notice_len > 0 {
            let n = self.notices[self.notice_head].take();
            self.notice_head = (self.notice_head + 1) % NOTICE_SLOTS;
            self.notice_len -= 1;
            if let Some(x) = n {
                out.push(x);
            }
        }
        out
    }

    /// 通道恢复后补投待投通知（返回补投条数）。
    pub fn replay_pending(&mut self) -> usize {
        // `mem::take` 而非 `drain(..).collect()`：后者新建一个同类型 Vec 再逐个
        // 搬过去，纯浪费一次分配（补投路径上白花开销没有理由）。
        let queued: Vec<DegradeNotice> = core::mem::take(&mut self.pending);
        let n = queued.len();
        let mut i = 0;
        while i < n {
            let notice = queued[i].clone();
            // 补投走「直接入环」：不再经 `emit_notice`，否则通道故障期间
            // 累积的同类通知会在补投时被合并成一条（补投语义是逐条补齐）。
            if self.notice_len == NOTICE_SLOTS {
                self.notice_head = (self.notice_head + 1) % NOTICE_SLOTS;
                self.notice_len -= 1;
            }
            self.notices[self.notice_tail] = Some(notice);
            self.notice_tail = (self.notice_tail + 1) % NOTICE_SLOTS;
            self.notice_len += 1;
            i += 1;
        }
        self.stats.notices_replayed += n as u64;
        n
    }

    /// 待补投通知数。
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// 环内通知数。
    pub fn notice_len(&self) -> usize {
        self.notice_len
    }

    /// 某级别已重试次数（诊断面）。
    pub fn retries_of(&self, sev: Severity) -> u32 {
        self.retry[sev.idx()]
    }

    /// 恢复总次数（全局硬顶判据的只读面）。
    pub fn total_attempts(&self) -> u32 {
        self.total_attempts
    }
}

/// 快照的资源表是否与真值逐条一致（真值每条都在快照中且记账一致）。
///
/// 只看「真值侧」而不看「快照侧有无多余条目」是**弱判定**：真值为空时它恒
/// 成立。故两侧都比：条数须相等，且逐条 id + 页数 + 挂载标志全等。
fn snapshot_matches_truth(snapshot: &RecoverySnapshot, truth: &[ResourceRow]) -> bool {
    if snapshot.resources.len() != truth.len() {
        return false;
    }
    let mut i = 0;
    while i < truth.len() {
        match snapshot.resource(truth[i].id) {
            None => return false,
            Some(r) => {
                if r.backing_pages != truth[i].backing_pages || r.attached != truth[i].attached {
                    return false;
                }
            }
        }
        i += 1;
    }
    true
}

/// 快照内全部队列是否处于复位初值。
fn snapshot_queues_at_reset(snapshot: &RecoverySnapshot) -> bool {
    let mut i = 0;
    while i < snapshot.queues.len() {
        let q = snapshot.queues[i];
        if !q.ready || q.avail != AVAIL_AFTER_RESET || q.pending != PENDING_AFTER_RESET {
            return false;
        }
        i += 1;
    }
    true
}

/// 队列下标 → 角色名（越界给「未知队列」，不猜）。
fn queue_role_name(index: u16) -> &'static str {
    match index {
        0 => "控制队列",
        1 => "光标队列",
        2 => "主渲染队列",
        _ => "未知队列",
    }
}

/// VE-F0212 判据自检域聚合入口（登记于 `svstar2::checks`）。
pub fn run_veb12_checks() -> crate::checks::CheckSet {
    super::veb12_checks::run_veb12_checks()
}
