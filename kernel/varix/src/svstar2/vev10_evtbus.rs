//! VE-F4608 · 插件事件总线（VE-V 域 · 插件组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4608`
//!
//! **判据（锚点原文）**：发布订阅、按权裁剪、流量配额、受控桥接、判据。
//!
//! **职责定位（锚点原文摘录）**：插件事件总线（插件间与插件-宿主通信：发布订阅事件总线——事件分类（系统事件/用户交互事件/数据变更事件）+订阅按权限裁剪+事件流量配额（防事件风暴）；总线与 U05 集成总线同构不同域——域间桥接受控）。数据结构：事件总线；订阅裁剪表；流量配额器。错误路径与降级矩阵：事件风暴→配额限流；越权订阅→拒绝；桥接滥用→审计+熔断。性能逐项分解：投递 O(订阅数)；裁剪 O(1)；限流 O(1)。
//!
//! ---------------------------------------------------------------------------
//! # 施工分节（本文件为自包含子系统，零 `crate::` 依赖，no_std + alloc）
//!
//! * **一、诊断码段**：0x1C10..=0x1C1F 独占段（整仓 grep 在案：0x1C1 文本命中
//!   仅有 galaxy/inputexp.rs 的配色常量 0x1C1C24，非诊断码；本段零占用后选定），
//!   十六码互异、段内、每码配中文可操作 `detail`。
//! * **二、事件三分类**：[`EventClass`] 三态闭集（System/UserInteraction/
//!   DataChange）——闭集即分类学，第四类进不来。每类投递语义差异落到 const 表：
//!   [`CLASS_PRIORITY`]（调度档）、[`CLASS_RETRY_ALLOW`]（重投拍数，System
//!   at-most-once=0 / DataChange at-least-once=2）、[`CLASS_SUMMARY_OK`]
//!   （System 禁摘要降级——宁可丢弃不给残缺）、[`CLASS_QUOTA_WEIGHT`]（配额
//!   加权，数据变更事件加倍——变更风暴最伤下游）、[`CLASS_SUMMARY_MASK`]
//!   （摘要降级后保留字段掩码，逐类不同）。
//! * **三、事件封套**：[`Evt`] 带事件 id / 类 / 发布者 / 16 命名字段位掩码 +
//!   值槽。字段表 [`FIELD_NAMES`] 写死 16 项；值按字段位下标直存——裁剪只动
//!   掩码不动值，投影一步 AND（见第六节 O(1) 判据）。
//! * **四、按权裁剪**：[`PermClipper`] 订阅者权限档（Host/Trusted/Normal/
//!   Sandbox 四档）→ 事件字段裁剪。裁剪判据：`可见掩码 = 字段掩码 & 档位掩码`
//!   一次按位与 **O(1)**；裁剪前后字段可见性矩阵 [`VIS_MATRIX`] 为 const
//!   （4 档 × 16 字段，编译期展开）。越权订阅在准入即拒（[`CLASS_REQUIRED_TIER`]）。
//! * **五、流量配额器**：[`FlowQuota`] 每发布者滑动窗配额（8 槽环 × 每槽
//!   4 日拍 = 32 拍窗口）；双阈值：软限超→摘要降级投递，硬限超→整事件丢弃
//!   并显性计数（[`QuotaEntry::dropped_events`]，不静默）。限流判据：判定
//!   为窗口总数与阈值比较 **O(1)**；窗口推进每发布仅常数步（≤8 槽回收）。
//! * **六、发布订阅总线**：[`EvtBusCore`] 发布→按订阅者逐一投递 **O(订阅数)**；
//!   投递账 [`DeliveryRecord`] 逐事件（事件 id/类/发布者/投递数/被裁订阅者数/
//!   摘要态/丢弃态）；订阅表 [`Subscription`]（订阅者/类过滤/权限档/租约期，
//!   到期回收走 [`EvtBusCore::sweep`]）；账满显性拒收（[`EvtBusError::LedgerFull`]）。
//! * **七、受控桥接**：[`DomainBridge`] 与 U05 集成总线同构不同域（域号
//!   0x0505 vs 本域 0x5605；同构 = 同一套类白名单/配额/出箱队列形状；不同域 =
//!   域号隔离 + 白名单收窄至 DataChange 单类）。桥接白名单 [`BRIDGE_WHITELIST_BITS`]；
//!   桥接流量独立配额（独立 [`FlowQuota`] 实例，限额更紧）；滥用熔断：连续
//!   越权类 ≥ [`BRIDGE_FUSE_THRESHOLD`] 次即熔断 [`BRIDGE_COOLDOWN_DAYS`] 日，
//!   冷却后半开单探针恢复；桥接审计账 [`BridgeAuditRec`] 逐笔留痕。
//! * **八、健康与指纹**：[`EvtBusCore::health`] 六项体检（过期租约未回收/
//!   桥接熔断中/硬丢率超阈/订阅表满/事件账满/桥接连续违规临界）；FNV-1a
//!   全态指纹 [`EvtBusCore::fingerprint`] + [`EvtBusCore::seal`] 封印 +
//!   [`EvtBusCore::audit_seal`] 无账漂移检出。
//! * **九、注入面 + 双跑对拍**：风暴注入/越权订阅注入/桥接滥用注入三路各
//!   检出；固定对拍脚本 [`run_scenario`] 双实例 digest 全等（确定性对拍），
//!   不等 → [`EvtBusError::DualRunMismatch`]。
//! * **十、面板行**：订阅表/发布者/投递账/桥接审计 `*_board` 系列逐行
//!   中文输出，域本色一行——面板/日志共用。
//!
//! # 性能逐项分解（锚点口径落实）
//!
//! * 投递：[`EvtBusCore::publish`] 按订阅表逐一定投，**O(订阅数)**
//!   （订阅表容量 [`SUB_TABLE_CAP`]=32 为常量上界）。
//! * 裁剪：[`PermClipper::clip`] 掩码一次 AND + 计位，**O(1)**。
//! * 限流：[`FlowQuota`] 判定 = 窗口总数 vs 阈值一次比较 **O(1)**；窗口推进
//!   每发布仅回收已过槽位（≤[`QUOTA_SLOTS`]=8 常数步）；配额表按发布者线性
//!   定位，容量 64 常量上界（判定本身仍 O(1)）。
//! * 指纹/对拍：全态 FNV 一遍 **O(账面规模)**。
//!
//! # 纪律自查（铁律逐条）
//!
//! * 零 `crate::` 依赖；仅 `core` + `alloc::{vec::Vec, format, string::*}`。
//! * 内核路径零 unwrap/expect/panic!/运行期 assert!（可失败全走
//!   `Result<T, EvtBusError>`；编译期 `const _: () = assert!(…)` 仅限常量式）。
//! * 计数器全部 `saturating_add`/显性 wrapping；>64KB 结构体不落栈（本文件
//!   最大栈对象为 16×u32 事件封套，远低于阈）。
//! * const fn 禁坑：无 slice::get、无 Vec 索引、无 Ord::min/max、无 derive
//!   PartialEq 比较、无 for 循环（`vis_row` 用 while+下标）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码段（0x1C10..=0x1C1F 独占——整仓 grep 在案后选定）
// ---------------------------------------------------------------------------

// 段选择依据（逐项 grep 在案）：0x1C1 文本命中仅有
// varix/src/galaxy/inputexp.rs:368/543 的配色常量 0x1C1C24（暗色底色，非诊断码）；
// 其余 0x1C10..=0x1C1F 段内整段零命中——本层独占。

/// 诊断码：事件风暴硬限流（超硬配额整事件丢弃）。
pub const CODE_STORM_THROTTLED: u16 = 0x1C10;
/// 诊断码：摘要降级投递（超软配额，载荷降为摘要字段集）。
pub const CODE_SUMMARY_DEGRADED: u16 = 0x1C11;
/// 诊断码：越权订阅拒绝（权限档高于类准入上限）。
pub const CODE_SUB_DENIED: u16 = 0x1C12;
/// 诊断码：订阅表满（容量耗尽显性拒收）。
pub const CODE_SUB_TABLE_FULL: u16 = 0x1C13;
/// 诊断码：事件账满（投递账容量耗尽显性拒收，不静默挤占）。
pub const CODE_LEDGER_FULL: u16 = 0x1C14;
/// 诊断码：配额表满（发布者准入超登记上限）。
pub const CODE_QUOTA_TABLE_FULL: u16 = 0x1C15;
/// 诊断码：发布者未登记（未准入先发布）。
pub const CODE_PUBLISHER_UNKNOWN: u16 = 0x1C16;
/// 诊断码：租约到期（过期后原地续租被拒——须重新准入）。
pub const CODE_LEASE_EXPIRED: u16 = 0x1C17;
/// 诊断码：桥接类不在白名单（跨域类被拦）。
pub const CODE_BRIDGE_CLASS_BLOCKED: u16 = 0x1C18;
/// 诊断码：桥接配额耗尽（跨域流量独立限额）。
pub const CODE_BRIDGE_QUOTA: u16 = 0x1C19;
/// 诊断码：桥接熔断中（冷却未到，一切跨域转发被拒）。
pub const CODE_BRIDGE_FUSED: u16 = 0x1C1A;
/// 诊断码：桥接滥用（连续越权类触发熔断）。
pub const CODE_BRIDGE_ABUSE: u16 = 0x1C1B;
/// 诊断码：字段掩码越界（事件载荷位 ≥ 字段表宽度）。
pub const CODE_FIELD_MASK_OOB: u16 = 0x1C1C;
/// 诊断码：双跑对拍不一致（确定性破坏）。
pub const CODE_DUALRUN_MISMATCH: u16 = 0x1C1D;
/// 诊断码：封印指纹漂移（有变更无账）。
pub const CODE_FINGERPRINT_DRIFT: u16 = 0x1C1E;
/// 诊断码：健康降级（六项体检有失败项）。
pub const CODE_HEALTH_DEGRADED: u16 = 0x1C1F;

// ---------------------------------------------------------------------------
// 二、事件三分类（三态闭集 + 逐类 const 语义表）
// ---------------------------------------------------------------------------

/// 事件三分类（三态闭集——闭集即分类学，第四类进不来）。
///
/// 三类投递语义差异（锚点"事件分类"判据逐类落实，const 表见下）：
///
/// * `System`（系统事件）：抢占投递（优先档 0）、at-most-once（重投 0 拍）、
///   **禁摘要降级**——系统事件宁可整事件丢弃并显性计数，不给残缺载荷。
/// * `UserInteraction`（用户交互事件）：合并投递约定（同键连击可合并）、
///   允许摘要降级（风暴期降为坐标+键位摘要）。
/// * `DataChange`（数据变更事件）：必达投递（at-least-once，重投 ≤2 拍）、
///   允许摘要降级但摘要必须含 entity+checksum 变更指纹；配额权重加倍
///   （变更风暴最伤下游，见 [`CLASS_QUOTA_WEIGHT`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventClass {
    /// 系统事件（内核/宿主生命周期类）。
    System,
    /// 用户交互事件（输入/焦点/菜单类）。
    UserInteraction,
    /// 数据变更事件（存储/状态/同步类）。
    DataChange,
}

impl EventClass {
    /// 三类全集（下标即 [`EventClass::index`]）。
    pub const ALL: [EventClass; 3] = [
        EventClass::System,
        EventClass::UserInteraction,
        EventClass::DataChange,
    ];

    /// 类下标（const fn，match 展开——不走数组索引，避开 const 索引坑）。
    pub const fn index(self) -> usize {
        match self {
            EventClass::System => 0,
            EventClass::UserInteraction => 1,
            EventClass::DataChange => 2,
        }
    }

    /// 类位（类过滤位掩码中的独占位）。
    pub const fn bit(self) -> u8 {
        match self {
            EventClass::System => 0b001,
            EventClass::UserInteraction => 0b010,
            EventClass::DataChange => 0b100,
        }
    }

    /// 类下标反查（const fn）。
    pub const fn from_index(i: usize) -> EventClass {
        match i {
            0 => EventClass::System,
            1 => EventClass::UserInteraction,
            _ => EventClass::DataChange,
        }
    }

    /// 类名（与 [`CLASS_NAMES`] 一致——name 走 match，const 表供账面遍历）。
    pub fn name(self) -> &'static str {
        CLASS_NAMES[self.index()]
    }

    /// 逐类投递语义一句话（面板/账面共用）。
    pub fn semantics(self) -> &'static str {
        match self {
            EventClass::System => "抢占投递·at-most-once·禁摘要降级",
            EventClass::UserInteraction => "合并投递·允许摘要降级",
            EventClass::DataChange => "必达投递·at-least-once·重投≤2拍·摘要含指纹",
        }
    }
}

/// 类名表（下标 = [`EventClass::index`]）。
pub const CLASS_NAMES: [&str; 3] = ["System", "UserInteraction", "DataChange"];

/// 逐类调度优先档（0 最高；System 抢占、UI 居中、DataChange 让行）。
/// 来源：锚点"事件分类（系统事件/用户交互事件/数据变更事件）"逐类定档，写死口径。
pub const CLASS_PRIORITY: [u8; 3] = [0, 1, 2];

/// 逐类允许重投拍数（System=0 at-most-once；UI=0 合并即止；DataChange=2
/// at-least-once 宽限）。来源：锚点"投递语义差异"，写死口径。
pub const CLASS_RETRY_ALLOW: [u8; 3] = [0, 0, 2];

/// 逐类是否允许摘要降级（System=false——宁可丢弃不给残缺）。
pub const CLASS_SUMMARY_OK: [bool; 3] = [false, true, true];

/// 逐类配额权重（DataChange 加倍——变更风暴最伤下游，写死口径）。
pub const CLASS_QUOTA_WEIGHT: [u32; 3] = [1, 1, 2];

/// 逐类摘要降级后的字段保留掩码（逐类不同：UI 保交互位置、变更保实体指纹）。
pub const CLASS_SUMMARY_MASK: [u32; 3] = [
    F_KIND | F_SEQ,
    F_KIND | F_KEY | F_X | F_Y | F_BUTTON,
    F_KIND | F_SEQ | F_ENTITY | F_EXTENT | F_CHECKSUM,
];

/// 逐类订阅准入权限档上限（0=Host 最特权，3=Sandbox 最受限；订阅者档位
/// 必须 ≤ 上限才准入）。来源：锚点"订阅按权限裁剪"准入侧判据，写死口径：
/// 系统事件仅 Host/Trusted 可订；数据变更至 Normal；交互事件全档可订。
pub const CLASS_REQUIRED_TIER: [u8; 3] = [1, 3, 2];

// ---------------------------------------------------------------------------
// 三、事件封套（16 命名字段位掩码 + 值槽）
// ---------------------------------------------------------------------------

/// 事件命名字段总数（16 位掩码宽度）。
pub const EVT_FIELD_COUNT: usize = 16;

/// 事件值槽宽度（与字段数一致：值按字段位下标直存，裁剪不动值只动掩码）。
pub const EVT_VAL_SLOTS: usize = EVT_FIELD_COUNT;

/// 全字段掩码（16 位全 1）。
pub const EVT_FIELD_MASK_ALL: u32 = 0xFFFF;

/// 字段位：kind（事件子类型键）。
pub const F_KIND: u32 = 1 << 0;
/// 字段位：seq（发布者内序号）。
pub const F_SEQ: u32 = 1 << 1;
/// 字段位：source（来源句柄）。
pub const F_SOURCE: u32 = 1 << 2;
/// 字段位：target（目标句柄）。
pub const F_TARGET: u32 = 1 << 3;
/// 字段位：level（级别/严重度）。
pub const F_LEVEL: u32 = 1 << 4;
/// 字段位：key（键位/键名索引）。
pub const F_KEY: u32 = 1 << 5;
/// 字段位：delta（增量值）。
pub const F_DELTA: u32 = 1 << 6;
/// 字段位：old_val（旧值——敏感面：Normal 以下裁剪）。
pub const F_OLD_VAL: u32 = 1 << 7;
/// 字段位：new_val（新值——敏感面：Normal 以下裁剪）。
pub const F_NEW_VAL: u32 = 1 << 8;
/// 字段位：x（交互横坐标）。
pub const F_X: u32 = 1 << 9;
/// 字段位：y（交互纵坐标）。
pub const F_Y: u32 = 1 << 10;
/// 字段位：button（按键/按钮号）。
pub const F_BUTTON: u32 = 1 << 11;
/// 字段位：modifier（修饰键组合——敏感面：Sandbox 裁剪）。
pub const F_MODIFIER: u32 = 1 << 12;
/// 字段位：entity（实体句柄——摘要指纹保留项）。
pub const F_ENTITY: u32 = 1 << 13;
/// 字段位：extent（影响范围）。
pub const F_EXTENT: u32 = 1 << 14;
/// 字段位：checksum（载荷校验和——内部完整性项：Trusted 以下裁剪）。
pub const F_CHECKSUM: u32 = 1 << 15;

/// 命名字段表（位下标即 [`Evt::field`] 取值下标；面板按此渲染）。
pub const FIELD_NAMES: [&str; EVT_FIELD_COUNT] = [
    "kind", "seq", "source", "target", "level", "key", "delta", "old_val",
    "new_val", "x", "y", "button", "modifier", "entity", "extent", "checksum",
];

/// 事件封套：一次发布的最小完整单元。
///
/// 值槽按下标直存（`vals[i]` = 字段位 `i` 的值，字段未置位时该槽无意义）——
/// 裁剪只投影掩码、不动值槽，这是裁剪 O(1) 的结构前提（见 [`PermClipper`]）。
#[derive(Clone, Copy, Debug)]
pub struct Evt {
    /// 事件 id（账面唯一，总线自增分配）。
    pub id: u32,
    /// 事件类（三态闭集）。
    pub class: EventClass,
    /// 发布者 id（准入登记号）。
    pub publisher: u16,
    /// 发布日拍。
    pub day: u32,
    /// 字段置位掩码（仅低 16 位合法，越位 → [`EvtBusError::FieldMaskOob`]）。
    pub field_mask: u32,
    /// 值槽（下标 = 字段位）。
    pub vals: [u32; EVT_VAL_SLOTS],
}

impl Evt {
    /// 组装事件封套（掩码越位显性拒绝——不入账不留残件）。
    pub fn compose(
        id: u32,
        class: EventClass,
        publisher: u16,
        day: u32,
        field_mask: u32,
        vals: [u32; EVT_VAL_SLOTS],
    ) -> Result<Evt, EvtBusError> {
        if field_mask & !EVT_FIELD_MASK_ALL != 0 {
            return Err(EvtBusError::FieldMaskOob { mask: field_mask });
        }
        Ok(Evt { id, class, publisher, day, field_mask, vals })
    }

    /// 取字段值（未置位返回 None；运行期方法可用 get——const 禁坑仅限 const fn）。
    pub fn field(&self, idx: usize) -> Option<u32> {
        if idx >= EVT_FIELD_COUNT {
            return None;
        }
        if self.field_mask >> idx & 1 == 1 {
            Some(self.vals[idx])
        } else {
            None
        }
    }

    /// 摘要投影（掩码 ∩ 逐类摘要掩码；值槽不动——结构前提见文件头）。
    pub fn summarized(self) -> Evt {
        let keep = CLASS_SUMMARY_MASK[self.class.index()];
        Evt { field_mask: self.field_mask & keep, ..self }
    }

    /// 置位字段计数（账面用）。
    pub fn field_count(&self) -> u32 {
        self.field_mask.count_ones()
    }
}

// ---------------------------------------------------------------------------
// 四、错误枚举（十六码互异、段内、每码中文可操作 detail）
// ---------------------------------------------------------------------------

/// 事件总线错误全枚举——每变体一个诊断码（0x1C10..=0x1C1F 段内互异），
/// `detail()` 给中文可操作诊断。可失败路径全走本枚举，无静默路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvtBusError {
    /// 事件风暴硬限流（窗口总数超硬配额，整事件丢弃）。
    StormThrottled { publisher: u16, window_total: u32, hard_limit: u32 },
    /// 摘要降级（软配额超限；严格发布方显性拒绝降级时抛出）。
    SummaryDegraded { publisher: u16, event_id: u32 },
    /// 越权订阅拒绝（订阅者权限档高于类准入上限；`got_tier=0xFF` 表示类过滤位非法）。
    SubDenied { subscriber: u16, class: EventClass, required_tier: u8, got_tier: u8 },
    /// 订阅表满。
    SubTableFull { cap: u32 },
    /// 事件账满（显性拒收，不静默挤占历史账）。
    LedgerFull { cap: u32 },
    /// 配额表满（发布者准入超登记上限）。
    QuotaTableFull { cap: u32 },
    /// 发布者未登记。
    PublisherUnknown { publisher: u16 },
    /// 租约到期（过期后原地续租被拒——须退订重订走重新准入）。
    LeaseExpired { sub_id: u32, until_day: u32, day: u32 },
    /// 桥接类不在白名单。
    BridgeClassBlocked { class: EventClass },
    /// 桥接配额耗尽。
    BridgeQuota { window_total: u32, hard_limit: u32 },
    /// 桥接熔断中（冷却未到）。
    BridgeFused { cooldown_until: u32, day: u32 },
    /// 桥接滥用（连续越权类达熔断阈值）。
    BridgeAbuse { violations: u32, threshold: u32 },
    /// 字段掩码越界。
    FieldMaskOob { mask: u32 },
    /// 双跑对拍不一致。
    DualRunMismatch { lhs: u32, rhs: u32 },
    /// 封印指纹漂移。
    FingerprintDrift { sealed: u32, current: u32 },
    /// 健康降级（体检失败项数）。
    HealthDegraded { failed_checks: u32 },
}

impl EvtBusError {
    /// 诊断码（const fn，match 展开——段 0x1C10..=0x1C1F 互异）。
    pub const fn code(&self) -> u16 {
        match self {
            EvtBusError::StormThrottled { .. } => CODE_STORM_THROTTLED,
            EvtBusError::SummaryDegraded { .. } => CODE_SUMMARY_DEGRADED,
            EvtBusError::SubDenied { .. } => CODE_SUB_DENIED,
            EvtBusError::SubTableFull { .. } => CODE_SUB_TABLE_FULL,
            EvtBusError::LedgerFull { .. } => CODE_LEDGER_FULL,
            EvtBusError::QuotaTableFull { .. } => CODE_QUOTA_TABLE_FULL,
            EvtBusError::PublisherUnknown { .. } => CODE_PUBLISHER_UNKNOWN,
            EvtBusError::LeaseExpired { .. } => CODE_LEASE_EXPIRED,
            EvtBusError::BridgeClassBlocked { .. } => CODE_BRIDGE_CLASS_BLOCKED,
            EvtBusError::BridgeQuota { .. } => CODE_BRIDGE_QUOTA,
            EvtBusError::BridgeFused { .. } => CODE_BRIDGE_FUSED,
            EvtBusError::BridgeAbuse { .. } => CODE_BRIDGE_ABUSE,
            EvtBusError::FieldMaskOob { .. } => CODE_FIELD_MASK_OOB,
            EvtBusError::DualRunMismatch { .. } => CODE_DUALRUN_MISMATCH,
            EvtBusError::FingerprintDrift { .. } => CODE_FINGERPRINT_DRIFT,
            EvtBusError::HealthDegraded { .. } => CODE_HEALTH_DEGRADED,
        }
    }

    /// 中文可操作诊断（每变体一句，指出"哪错、怎么改"）。
    pub fn detail(&self) -> &'static str {
        match self {
            EvtBusError::StormThrottled { .. } => {
                "事件风暴硬限流：该发布者滑动窗总权重已超硬配额，事件被整件丢弃；\
                 请降低发布频率或改用数据变更摘要通道，丢弃数已显性入账（QuotaEntry.dropped_events）。"
            }
            EvtBusError::SummaryDegraded { .. } => {
                "摘要降级：软配额超限后总线仅投递摘要字段集；严格发布方（at-least-once \
                 契约）不应接受降级载荷，请退避至软限内再发全量。"
            }
            EvtBusError::SubDenied { .. } => {
                "越权订阅拒绝：订阅者权限档高于该事件类准入上限（系统事件仅 Host/Trusted，\
                 数据变更至 Normal）；请申请提档或改订低敏类，准入判定见 CLASS_REQUIRED_TIER。"
            }
            EvtBusError::SubTableFull { .. } => {
                "订阅表满：容量耗尽显性拒收；请先 sweep 回收到期租约，或缩减僵尸订阅。"
            }
            EvtBusError::LedgerFull { .. } => {
                "事件账满：投递账容量耗尽，本次发布整件拒收（不静默挤占历史账）；\
                 请消费方尽快拉取账面或调大 LEDGER_CAP。"
            }
            EvtBusError::QuotaTableFull { .. } => {
                "配额表满：发布者登记数超上限；请先注销不再活跃的发布者再准入。"
            }
            EvtBusError::PublisherUnknown { .. } => {
                "发布者未登记：请先 register_publisher 走准入（名字 + 配额建档）再发布。"
            }
            EvtBusError::LeaseExpired { .. } => {
                "租约到期：过期订阅不可原地续租（防僵尸提权）；请退订后重新 subscribe \
                 走完整准入授权。"
            }
            EvtBusError::BridgeClassBlocked { .. } => {
                "桥接类不在白名单：跨域仅放行数据变更类；系统/交互事件请走本域消费，\
                 不要绕道桥接。"
            }
            EvtBusError::BridgeQuota { .. } => {
                "桥接配额耗尽：跨域流量独立限额比域内更紧；请合批或延后转发，\
                 超限已计入桥接账。"
            }
            EvtBusError::BridgeFused { .. } => {
                "桥接熔断中：连续越权触发熔断，冷却期内一切跨域转发被拒；\
                 请等待冷却窗口结束（半开单探针恢复），勿重试试探。"
            }
            EvtBusError::BridgeAbuse { .. } => {
                "桥接滥用：连续越权类转发达熔断阈值，桥已熔断；\
                 请排查越权源，冷却后半开探针通过方可恢复。"
            }
            EvtBusError::FieldMaskOob { .. } => {
                "字段掩码越界：事件载荷位超出 16 位字段表宽度；请核对 FIELD_NAMES \
                 位定义，高位必须清零。"
            }
            EvtBusError::DualRunMismatch { .. } => {
                "双跑对拍不一致：同脚本两实例 digest 分叉，存在隐藏可变态；\
                 请排查时间源/随机源/未入账状态。"
            }
            EvtBusError::FingerprintDrift { .. } => {
                "封印指纹漂移：当前全态指纹与封印值不符而账面无对应变更记录；\
                 请先审计变更来源，再重新 seal 封印。"
            }
            EvtBusError::HealthDegraded { .. } => {
                "健康降级：六项体检存在失败项，请按 HealthReport.issues 逐项处置。"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 五、按权裁剪（PermClipper · 四档权限 × 16 字段 · 裁剪 O(1)）
// ---------------------------------------------------------------------------

/// 权限档总数（0=Host 最特权 … 3=Sandbox 最受限）。
pub const PERM_TIER_COUNT: usize = 4;

/// 权限档名表（下标 = 档位）。
pub const TIER_NAMES: [&str; PERM_TIER_COUNT] = ["Host", "Trusted", "Normal", "Sandbox"];

/// 逐档可见字段掩码（裁剪判据的数据面；来源：锚点"订阅按权限裁剪"——
/// 敏感面逐级收窄，写死口径）：
///
/// * Host：全 16 字段（宿主内核全量）。
/// * Trusted：全量除 `checksum`（内部完整性校验和不出信任圈）。
/// * Normal：交互/定位/实体面（kind/seq/source/target/x/y/entity/extent）；
///   键值增删改（key/delta/old_val/new_val）与修饰键、校验和全部裁剪。
/// * Sandbox：仅 kind/seq/entity（最小可观测面——只知道"发生了什么"，
///   不知道"值是什么"）。
pub const PERM_FIELD_MASKS: [u32; PERM_TIER_COUNT] = [
    EVT_FIELD_MASK_ALL,
    EVT_FIELD_MASK_ALL & !F_CHECKSUM,
    F_KIND | F_SEQ | F_SOURCE | F_TARGET | F_X | F_Y | F_ENTITY | F_EXTENT,
    F_KIND | F_SEQ | F_ENTITY,
];

/// 逐档字段可见性矩阵（const——4 档 × 16 字段，编译期展开；面板直读，
/// 与 [`PERM_FIELD_MASKS`] 恒等，见 `const` 断言与单元测试对账）。
pub const VIS_MATRIX: [[bool; EVT_FIELD_COUNT]; PERM_TIER_COUNT] = [
    vis_row(0),
    vis_row(1),
    vis_row(2),
    vis_row(3),
];

/// const 行展开器（while+下标——避开 const fn 的 for/迭代器禁坑）。
const fn vis_row(tier: usize) -> [bool; EVT_FIELD_COUNT] {
    let m = tier_mask(tier);
    let mut row = [false; EVT_FIELD_COUNT];
    let mut i = 0;
    while i < EVT_FIELD_COUNT {
        row[i] = (m >> i) & 1 == 1;
        i += 1;
    }
    row
}

/// 档位掩码 const 查询（match 展开——不走数组下标，避开 const 索引坑；
/// 越界档位返回 0——全裁即全盲，调用侧准入已保证档位合法）。
pub const fn tier_mask(tier: usize) -> u32 {
    match tier {
        0 => PERM_FIELD_MASKS[0],
        1 => PERM_FIELD_MASKS[1],
        2 => PERM_FIELD_MASKS[2],
        3 => PERM_FIELD_MASKS[3],
        _ => 0,
    }
}

// 裁剪矩阵与档位掩码恒等（编译期对账——两份口径必须永远一致）。
const _: () = {
    let mut t = 0;
    while t < PERM_TIER_COUNT {
        let mut f = 0;
        while f < EVT_FIELD_COUNT {
            assert!(((PERM_FIELD_MASKS[t] >> f) & 1 == 1) == VIS_MATRIX[t][f]);
            f += 1;
        }
        t += 1;
    }
};

/// 裁剪结论（一次按位与的产物；区分原样/被裁供账面计数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipOutcome {
    /// 无字段被裁，原样可见。
    Verbatim { visible_mask: u32 },
    /// 有字段被裁（`masked_bits` = 被裁位数，账面统计）。
    Clipped { visible_mask: u32, masked_bits: u32 },
}

impl ClipOutcome {
    /// 可见掩码（两态统一出口）。
    pub const fn visible_mask(&self) -> u32 {
        match self {
            ClipOutcome::Verbatim { visible_mask } => *visible_mask,
            ClipOutcome::Clipped { visible_mask, .. } => *visible_mask,
        }
    }

    /// 是否发生裁剪。
    pub const fn is_clipped(&self) -> bool {
        match self {
            ClipOutcome::Verbatim { .. } => false,
            ClipOutcome::Clipped { .. } => true,
        }
    }
}

/// 按权裁剪器：订阅者权限档 → 事件字段可见性投影。
///
/// **O(1) 判据（锚点口径）**：`可见掩码 = 字段掩码 & PERM_FIELD_MASKS[档]`
/// ——一次按位与；被裁位数由 `(!可见) & 字段掩码` 的 `count_ones` 计位，
/// 均为常数步。值槽按下标直存不动（见 [`Evt`] 结构前提），故裁剪全程无
/// 数据搬移。
#[derive(Debug)]
pub struct PermClipper {
    /// 发生裁剪的事件次数。
    pub clipped_events: u64,
    /// 累计被裁字段位数。
    pub masked_field_total: u64,
    /// 非法档位命中数（准入侧本应拦住——此处为零期待哨兵计数）。
    pub bad_tier_hits: u64,
}

impl PermClipper {
    /// 空裁剪器。
    pub fn new() -> PermClipper {
        PermClipper { clipped_events: 0, masked_field_total: 0, bad_tier_hits: 0 }
    }

    /// 裁剪一次：档位 × 字段掩码 → 投影结论（含账面计数；溢出显性 saturating）。
    pub fn clip(&mut self, tier: u8, field_mask: u32) -> ClipOutcome {
        let t = tier as usize;
        if t >= PERM_TIER_COUNT {
            // 非法档位：全裁（可见掩码 0）+ 哨兵计数——不 panic、不留残权。
            self.bad_tier_hits = self.bad_tier_hits.saturating_add(1);
            self.clipped_events = self.clipped_events.saturating_add(1);
            return ClipOutcome::Clipped { visible_mask: 0, masked_bits: field_mask.count_ones() };
        }
        let visible = field_mask & PERM_FIELD_MASKS[t];
        if visible == field_mask {
            ClipOutcome::Verbatim { visible_mask: visible }
        } else {
            let masked = (!visible) & field_mask;
            self.clipped_events = self.clipped_events.saturating_add(1);
            self.masked_field_total =
                self.masked_field_total.saturating_add(masked.count_ones() as u64);
            ClipOutcome::Clipped { visible_mask: visible, masked_bits: masked.count_ones() }
        }
    }

    /// 查询某档某字段是否可见（矩阵直读，O(1)）。
    pub fn visibility(&self, tier: u8, field: usize) -> bool {
        let t = tier as usize;
        if t >= PERM_TIER_COUNT || field >= EVT_FIELD_COUNT {
            return false;
        }
        VIS_MATRIX[t][field]
    }

    /// 渲染裁剪前后字段可见性矩阵（4 档 × 16 字段逐行——面板/审计共用）。
    pub fn render_matrix() -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!(
            "裁剪矩阵 {} 档 × {} 字段（可见=+ 裁剪=-）",
            PERM_TIER_COUNT, EVT_FIELD_COUNT
        ));
        let mut t = 0;
        while t < PERM_TIER_COUNT {
            let mut row = format!("档{} {:<8}", t, TIER_NAMES[t]);
            let mut f = 0;
            while f < EVT_FIELD_COUNT {
                row.push_str(if VIS_MATRIX[t][f] { "+" } else { "-" });
                f += 1;
            }
            lines.push(row);
            t += 1;
        }
        lines
    }
}

impl Default for PermClipper {
    fn default() -> Self {
        PermClipper::new()
    }
}

// ---------------------------------------------------------------------------
// 六、流量配额器（FlowQuota · 8 槽滑动窗 × 双阈值 · 限流 O(1)）
// ---------------------------------------------------------------------------

/// 滑动窗槽位数（8 槽环）。
pub const QUOTA_SLOTS: usize = 8;

/// 每槽覆盖日拍数（4 拍/槽 → 窗口 32 拍）。
pub const QUOTA_SLOT_TICKS: u32 = 4;

/// 滑动窗总宽（32 拍 = 8 槽 × 4 拍；写死口径：约一月的日拍粒度账期）。
pub const QUOTA_WINDOW_TICKS: u32 = QUOTA_SLOTS as u32 * QUOTA_SLOT_TICKS;

/// 软限（窗口总权重 > 软限 → 摘要降级投递）。来源：锚点"事件流量配额
/// （防事件风暴）"，写死口径。
pub const QUOTA_SOFT_LIMIT: u32 = 12;

/// 硬限（窗口总权重 > 硬限 → 整事件丢弃 + 显性计数）。
pub const QUOTA_HARD_LIMIT: u32 = 20;

/// 配额表容量（发布者登记上限；64 为常量上界）。
pub const QUOTA_TABLE_CAP: usize = 64;

// 编译期阈值自洽断言（const assert 允许）。
const _: () = assert!(QUOTA_HARD_LIMIT > QUOTA_SOFT_LIMIT);
const _: () = assert!(QUOTA_SLOTS > 0 && QUOTA_SLOT_TICKS > 0);

/// 单次扣费裁决（限流 O(1) 判据的产物：窗口总数 vs 阈值一次比较即出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaVerdict {
    /// 窗口内余量充足，全量放行。
    Pass,
    /// 超软限：放行但降级为摘要投递。
    Summary,
    /// 超硬限：整事件丢弃（显性计数，不静默）。
    Drop,
}

/// 单发布者配额账：8 槽滑动窗 + 双阈值计数。
///
/// 窗口推进：每发布仅回收"已越过槽界"的旧槽计数（≤ 8 槽常数步）；裁决 =
/// `窗口总数 + 权重` 与双阈值比较，**O(1)**（锚点"限流 O(1)"落实）。
#[derive(Debug)]
pub struct QuotaEntry {
    /// 发布者 id。
    pub publisher: u16,
    /// 本账软限（实例携带——总线宽限与桥接紧限同构不同参）。
    soft_limit: u32,
    /// 本账硬限。
    hard_limit: u32,
    /// 槽计数环（槽 i 覆盖日拍 `[slot_start + i*4, +4)` 窗内段）。
    slots: [u32; QUOTA_SLOTS],
    /// 当前槽起点日拍。
    slot_start: u32,
    /// 窗口总权重（= 各在窗槽计数之和，增量维护）。
    win_total: u32,
    /// 软限命中次数（降级为摘要）。
    pub soft_hits: u32,
    /// 硬限命中次数（整件丢弃）。
    pub hard_hits: u32,
    /// 累计丢弃事件数（显性计数——风暴不静默）。
    pub dropped_events: u32,
    /// 累计摘要降级事件数。
    pub summary_events: u32,
    /// 累计全量放行权重。
    pub charged_total: u32,
}

impl QuotaEntry {
    /// 新建配额账（窗口从 day 起算；软硬限随账携带）。
    pub fn new(publisher: u16, day: u32, soft_limit: u32, hard_limit: u32) -> QuotaEntry {
        QuotaEntry {
            publisher,
            soft_limit,
            hard_limit,
            slots: [0; QUOTA_SLOTS],
            slot_start: day,
            win_total: 0,
            soft_hits: 0,
            hard_hits: 0,
            dropped_events: 0,
            summary_events: 0,
            charged_total: 0,
        }
    }

    /// 窗口总权重快照。
    pub fn window_total(&self) -> u32 {
        self.win_total
    }

    /// 推进滑动窗到 `day`（仅回收越界槽——常数步 ≤ 8）。
    fn advance(&mut self, day: u32) {
        let elapsed = day.saturating_sub(self.slot_start);
        if elapsed >= QUOTA_WINDOW_TICKS {
            // 整窗过期：一次性清空（回卷口径，不逐槽省）。
            self.slots = [0; QUOTA_SLOTS];
            self.win_total = 0;
            self.slot_start = day;
        } else if elapsed >= QUOTA_SLOT_TICKS {
            let base = self.slot_start / QUOTA_SLOT_TICKS;
            let step = elapsed / QUOTA_SLOT_TICKS;
            let mut s: u32 = 0;
            while s < step && s < QUOTA_SLOTS as u32 {
                let idx = ((base + s) % QUOTA_SLOTS as u32) as usize;
                self.win_total = self.win_total.saturating_sub(self.slots[idx]);
                self.slots[idx] = 0;
                s += 1;
            }
            self.slot_start = self.slot_start.saturating_add(step.saturating_mul(QUOTA_SLOT_TICKS));
        }
    }

    /// 当前槽下标（日拍 → 槽环位）。
    fn cur_slot(day: u32) -> usize {
        ((day / QUOTA_SLOT_TICKS) % QUOTA_SLOTS as u32) as usize
    }

    /// 扣费一次：先推进窗口，再按双阈值裁决（限流 O(1) 判据落点）。
    pub fn charge(&mut self, weight: u32, day: u32) -> QuotaVerdict {
        self.advance(day);
        let cur = Self::cur_slot(day);
        let tentative = self.win_total.saturating_add(weight);
        if tentative > self.hard_limit {
            self.hard_hits = self.hard_hits.saturating_add(1);
            self.dropped_events = self.dropped_events.saturating_add(1);
            return QuotaVerdict::Drop;
        }
        if tentative > self.soft_limit {
            self.soft_hits = self.soft_hits.saturating_add(1);
            self.summary_events = self.summary_events.saturating_add(1);
        } else {
            self.charged_total = self.charged_total.saturating_add(weight);
        }
        // 软/硬限内的在窗计数都入账（软限放行的摘要载荷仍占窗）。
        self.win_total = tentative;
        self.slots[cur] = self.slots[cur].saturating_add(weight);
        if tentative > self.soft_limit {
            QuotaVerdict::Summary
        } else {
            QuotaVerdict::Pass
        }
    }

    /// 账行（面板用）。
    pub fn as_line(&self) -> String {
        format!(
            "配额 发布者0x{:04X} 窗总{} 软限{} 硬限{} 丢弃{} 降级{} 放行权重{}",
            self.publisher,
            self.win_total,
            self.soft_hits,
            self.hard_hits,
            self.dropped_events,
            self.summary_events,
            self.charged_total,
        )
    }
}

/// 流量配额器：发布者 → 配额账的登记与统一扣费入口。
///
/// 总线与桥接各持一个实例（同构不同参）：总线限额宽（12/20），桥接限额紧
/// （4/8，见第七节）——锚点"桥接流量单独配额"由此落实。
#[derive(Debug)]
pub struct FlowQuota {
    soft_limit: u32,
    hard_limit: u32,
    entries: Vec<QuotaEntry>,
    cap: usize,
}

impl FlowQuota {
    /// 新建配额器（soft < hard 由调用方保证；总线上有编译期断言兜底）。
    pub fn new(soft_limit: u32, hard_limit: u32, cap: usize) -> FlowQuota {
        FlowQuota { soft_limit, hard_limit, entries: Vec::new(), cap }
    }

    /// 总线默认参（12/20 × 64 发布者）。
    pub fn bus_default() -> FlowQuota {
        FlowQuota::new(QUOTA_SOFT_LIMIT, QUOTA_HARD_LIMIT, QUOTA_TABLE_CAP)
    }

    /// 桥接默认参（4/8 × 8 桥端——跨域面收紧）。
    pub fn bridge_default() -> FlowQuota {
        FlowQuota::new(BRIDGE_QUOTA_SOFT, BRIDGE_QUOTA_HARD, BRIDGE_QUOTA_TABLE_CAP)
    }

    /// 软限。
    pub fn soft_limit(&self) -> u32 {
        self.soft_limit
    }

    /// 硬限。
    pub fn hard_limit(&self) -> u32 {
        self.hard_limit
    }

    /// 已登记发布者数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 准入发布者（重复准入幂等刷新；表满显性拒绝）。
    pub fn admit(&mut self, publisher: u16, day: u32) -> Result<(), EvtBusError> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].publisher == publisher {
                return Ok(());
            }
            i += 1;
        }
        if self.entries.len() >= self.cap {
            return Err(EvtBusError::QuotaTableFull { cap: self.cap as u32 });
        }
        self.entries.push(QuotaEntry::new(publisher, day, self.soft_limit, self.hard_limit));
        Ok(())
    }

    /// 注销发布者（配额账随之销账）。
    pub fn retire(&mut self, publisher: u16) -> bool {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].publisher == publisher {
                let _ = self.entries.remove(i);
                return true;
            }
            i += 1;
        }
        false
    }

    /// 是否已登记。
    pub fn admitted(&self, publisher: u16) -> bool {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].publisher == publisher {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 统一扣费入口（未登记 → 显性错；裁决 O(1) 判据在 [`QuotaEntry::charge`]）。
    pub fn charge_for(
        &mut self,
        publisher: u16,
        weight: u32,
        day: u32,
    ) -> Result<QuotaVerdict, EvtBusError> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].publisher == publisher {
                return Ok(self.entries[i].charge(weight, day));
            }
            i += 1;
        }
        Err(EvtBusError::PublisherUnknown { publisher })
    }

    /// 某发布者窗口总权重（未登记 → 0）。
    pub fn window_of(&self, publisher: u16) -> u32 {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].publisher == publisher {
                return self.entries[i].window_total();
            }
            i += 1;
        }
        0
    }

    /// 某发布者累计丢弃数（未登记 → 0；风暴显性计数的读数口）。
    pub fn dropped_of(&self, publisher: u16) -> u32 {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].publisher == publisher {
                return self.entries[i].dropped_events;
            }
            i += 1;
        }
        0
    }

    /// 全表累计丢弃数（health 体检用）。
    pub fn dropped_total(&self) -> u32 {
        let mut acc: u32 = 0;
        let mut i = 0;
        while i < self.entries.len() {
            acc = acc.saturating_add(self.entries[i].dropped_events);
            i += 1;
        }
        acc
    }

    /// 全表面板行。
    pub fn board(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut i = 0;
        while i < self.entries.len() {
            lines.push(self.entries[i].as_line());
            i += 1;
        }
        lines
    }

    /// 指纹折入（发布者 id + 窗总 + 计数器——顺序稳定，遍历按下标）。
    pub fn fold_fingerprint(&self, acc: u32) -> u32 {
        let mut a = fnv_fold_u32(acc, self.soft_limit);
        a = fnv_fold_u32(a, self.hard_limit);
        a = fnv_fold_u32(a, self.entries.len() as u32);
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            a = fnv_fold_u32(a, e.publisher as u32);
            a = fnv_fold_u32(a, e.win_total);
            a = fnv_fold_u32(a, e.soft_hits);
            a = fnv_fold_u32(a, e.hard_hits);
            a = fnv_fold_u32(a, e.dropped_events);
            a = fnv_fold_u32(a, e.summary_events);
            a = fnv_fold_u32(a, e.charged_total);
            i += 1;
        }
        a
    }
}

// ---------------------------------------------------------------------------
// 七、受控桥接（DomainBridge · 与 U05 集成总线同构不同域）
// ---------------------------------------------------------------------------

/// 本域号（VE-V 插件域；与 U05 集成总线 0x0505 同构不同域——域号隔离写死口径）。
pub const LOCAL_DOMAIN_ID: u16 = 0x5605;

/// 对端域号（U05 集成总线；结构同构——同一套类白名单/配额/出箱队列形状，
/// 但域号不同、白名单收窄、限额更紧）。
pub const U05_DOMAIN_ID: u16 = 0x0505;

/// 桥接白名单位掩码：跨域仅放行数据变更类（系统事件不出域——内核生命周期
/// 面不外泄；交互事件不出域——本域交互本域消费）。来源：锚点"域间桥接受控"。
pub const BRIDGE_WHITELIST_BITS: u8 = EventClass::DataChange.bit();

/// 桥接配额软限（独立实例、独立参数——比总线 12 更紧）。
pub const BRIDGE_QUOTA_SOFT: u32 = 4;

/// 桥接配额硬限。
pub const BRIDGE_QUOTA_HARD: u32 = 8;

/// 桥接配额表容量（桥端登记上限）。
pub const BRIDGE_QUOTA_TABLE_CAP: usize = 8;

/// 连续越权类转发熔断阈值（连续 N 次 → 熔断；成功转发清零计数）。
pub const BRIDGE_FUSE_THRESHOLD: u32 = 3;

/// 熔断冷却日拍数（冷却到点 → 半开，单探针通过即恢复闭合）。
pub const BRIDGE_COOLDOWN_DAYS: u32 = 5;

/// 桥接审计账容量（滚动窗口——溢出淘汰最旧一笔，淘汰本身入账面口径）。
pub const BRIDGE_AUDIT_CAP: usize = 64;

/// 桥接出箱队列容量（跨域出口的落账队列，U05 侧按域号拉取）。
pub const BRIDGE_OUTBOX_CAP: usize = 32;

/// 桥端发布者 id（桥接配额账的记账主体——桥本身就是一个"发布者"）。
pub const BRIDGE_PUBLISHER_ID: u16 = 0xE100;

// 桥接参数自洽（编译期）。
const _: () = assert!(BRIDGE_QUOTA_HARD > BRIDGE_QUOTA_SOFT);
const _: () = assert!(BRIDGE_FUSE_THRESHOLD >= 2);

/// 桥接状态机：闭合 / 熔断开（含重开日拍）/ 半开（单探针）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeState {
    /// 闭合：正常转发（白名单 + 配额内）。
    Closed,
    /// 熔断开：冷却期内一切转发被拒。
    Open { /// 冷却到点日拍（≥ 此日拍转半开）。
    reopen_day: u32 },
    /// 半开：仅放行一枚白名单探针，通过即闭合、违例即回熔断。
    HalfOpen,
}

/// 桥接审计账一笔（逐桥接动作留痕——锚点"桥接滥用→审计+熔断"落实）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BridgeAuditRec {
    /// 日拍。
    pub day: u32,
    /// 事件 id。
    pub event_id: u32,
    /// 事件类。
    pub class: EventClass,
    /// 诊断码（0 = 无诊断，正常放行）。
    pub code: u16,
    /// 是否放行跨域。
    pub accepted: bool,
}

/// 桥接转发结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BridgeOutcome {
    /// 是否放行（进了出箱队列）。
    pub accepted: bool,
    /// 是否摘要降级放行。
    pub summary_mode: bool,
    /// 结论诊断码（0 = 全量放行无诊断）。
    pub code: u16,
}

/// 受控桥接：本域（0x5605）→ U05 集成总线（0x0505）的跨域出口。
///
/// 与 U05 集成总线**同构不同域**：形状上同样是"类白名单 + 流量配额 + 出箱
/// 队列 + 账面"；域上域号隔离（0x5605 ≠ 0x0505）、白名单收窄至数据变更单类、
/// 配额独立且更紧（4/8 vs 总线 12/20）。滥用熔断：连续越权类
/// [`BRIDGE_FUSE_THRESHOLD`] 次 → 熔断 [`BRIDGE_COOLDOWN_DAYS`] 日 → 半开
/// 单探针 → 通过闭合 / 违例回熔断。全程审计账 [`BridgeAuditRec`]。
#[derive(Debug)]
pub struct DomainBridge {
    state: BridgeState,
    /// 最近一次熔断的重开日拍（半开拒绝时的冷却参照）。
    last_reopen_day: u32,
    /// 当前连续越权计数（成功转发清零）。
    consecutive_violations: u32,
    /// 越权类转发总次数。
    violations_total: u32,
    /// 累计熔断次数。
    fused_count: u32,
    /// 全量放行笔数。
    forwarded: u64,
    /// 摘要降级放行笔数。
    forwarded_summary: u64,
    /// 配额丢弃笔数。
    quota_dropped: u32,
    /// 出箱队列（U05 侧按域号拉取）。
    outbox: Vec<Evt>,
    /// 审计账（滚动窗口）。
    audit: Vec<BridgeAuditRec>,
    /// 桥接独立配额（锚点"桥接流量单独配额"）。
    quota: FlowQuota,
    /// 半开探针是否已用。
    probe_used: bool,
}

impl DomainBridge {
    /// 新建桥（闭合态；桥端配额账建档）。
    pub fn new(day: u32) -> DomainBridge {
        let mut quota = FlowQuota::bridge_default();
        // 空表首准入，QuotaTableFull 不可能——仍显性消费以防参数改坏。
        let _ = quota.admit(BRIDGE_PUBLISHER_ID, day);
        DomainBridge {
            state: BridgeState::Closed,
            last_reopen_day: 0,
            consecutive_violations: 0,
            violations_total: 0,
            fused_count: 0,
            forwarded: 0,
            forwarded_summary: 0,
            quota_dropped: 0,
            outbox: Vec::new(),
            audit: Vec::new(),
            quota,
            probe_used: false,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> BridgeState {
        self.state
    }

    /// 当前连续越权计数。
    pub fn consecutive_violations(&self) -> u32 {
        self.consecutive_violations
    }

    /// 越权总次数。
    pub fn violations_total(&self) -> u32 {
        self.violations_total
    }

    /// 累计熔断次数。
    pub fn fused_count(&self) -> u32 {
        self.fused_count
    }

    /// 全量放行笔数。
    pub fn forwarded(&self) -> u64 {
        self.forwarded
    }

    /// 摘要降级放行笔数。
    pub fn forwarded_summary(&self) -> u64 {
        self.forwarded_summary
    }

    /// 配额丢弃笔数。
    pub fn quota_dropped(&self) -> u32 {
        self.quota_dropped
    }

    /// 出箱队列长度。
    pub fn outbox_len(&self) -> usize {
        self.outbox.len()
    }

    /// 审计账长度。
    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }

    /// 审计留痕（滚动窗口：满则淘汰最旧一笔——审计不阻断转发，淘汰口径入档）。
    fn audit_push(&mut self, day: u32, event_id: u32, class: EventClass, code: u16, accepted: bool) {
        if self.audit.len() >= BRIDGE_AUDIT_CAP {
            let _ = self.audit.remove(0);
        }
        self.audit.push(BridgeAuditRec { day, event_id, class, code, accepted });
    }

    /// 出箱入队（满则淘汰最旧——出口为滑动窗口，口径入档）。
    fn outbox_push(&mut self, evt: Evt) {
        if self.outbox.len() >= BRIDGE_OUTBOX_CAP {
            let _ = self.outbox.remove(0);
        }
        self.outbox.push(evt);
    }

    /// 跨域转发一事件（受控三闸：状态闸 → 白名单闸 → 配额闸）。
    pub fn forward(&mut self, evt: &Evt, day: u32) -> Result<BridgeOutcome, EvtBusError> {
        // 状态闸：熔断开 → 拒；冷却到点 → 半开。
        match self.state {
            BridgeState::Open { reopen_day } if day < reopen_day => {
                self.audit_push(day, evt.id, evt.class, CODE_BRIDGE_FUSED, false);
                return Err(EvtBusError::BridgeFused { cooldown_until: reopen_day, day });
            }
            BridgeState::Open { reopen_day } => {
                self.state = BridgeState::HalfOpen;
                self.last_reopen_day = reopen_day;
            }
            BridgeState::HalfOpen | BridgeState::Closed => {}
        }
        // 白名单闸：越权类 → 计违规 + 审计；连续达阈值 → 熔断。
        if evt.class.bit() & BRIDGE_WHITELIST_BITS == 0 {
            self.violations_total = self.violations_total.saturating_add(1);
            self.consecutive_violations = self.consecutive_violations.saturating_add(1);
            self.audit_push(day, evt.id, evt.class, CODE_BRIDGE_CLASS_BLOCKED, false);
            if self.consecutive_violations >= BRIDGE_FUSE_THRESHOLD {
                let reopen = day.saturating_add(BRIDGE_COOLDOWN_DAYS);
                self.state = BridgeState::Open { reopen_day: reopen };
                self.last_reopen_day = reopen;
                self.fused_count = self.fused_count.saturating_add(1);
                self.probe_used = false;
                self.audit_push(day, evt.id, evt.class, CODE_BRIDGE_ABUSE, false);
                return Err(EvtBusError::BridgeAbuse {
                    violations: self.consecutive_violations,
                    threshold: BRIDGE_FUSE_THRESHOLD,
                });
            }
            return Err(EvtBusError::BridgeClassBlocked { class: evt.class });
        }
        // 半开探针闸：半开只放一枚探针，探针已用则其余仍拒。
        if self.state == BridgeState::HalfOpen && self.probe_used {
            self.audit_push(day, evt.id, evt.class, CODE_BRIDGE_FUSED, false);
            return Err(EvtBusError::BridgeFused { cooldown_until: self.last_reopen_day, day });
        }
        // 配额闸：桥接独立配额（记账主体 = 桥端 id）。
        let weight = CLASS_QUOTA_WEIGHT[evt.class.index()];
        let verdict = match self.quota.charge_for(BRIDGE_PUBLISHER_ID, weight, day) {
            Ok(v) => v,
            // 桥端账在 new() 建档，此分支不可达——防御性视为丢弃（显性不静默）。
            Err(_) => QuotaVerdict::Drop,
        };
        match verdict {
            QuotaVerdict::Drop => {
                self.quota_dropped = self.quota_dropped.saturating_add(1);
                self.audit_push(day, evt.id, evt.class, CODE_BRIDGE_QUOTA, false);
                // 半开探针撞配额墙 = 探针失败：回熔断（重开日拍顺延一档冷却）。
                if self.state == BridgeState::HalfOpen {
                    let reopen = day.saturating_add(BRIDGE_COOLDOWN_DAYS);
                    self.state = BridgeState::Open { reopen_day: reopen };
                    self.last_reopen_day = reopen;
                    self.probe_used = false;
                }
                return Err(EvtBusError::BridgeQuota {
                    window_total: self.quota.window_of(BRIDGE_PUBLISHER_ID),
                    hard_limit: BRIDGE_QUOTA_HARD,
                });
            }
            QuotaVerdict::Summary => {
                let slim = evt.summarized();
                self.outbox_push(slim);
                self.forwarded_summary = self.forwarded_summary.saturating_add(1);
                self.accept_reset();
                self.audit_push(day, evt.id, evt.class, CODE_SUMMARY_DEGRADED, true);
                return Ok(BridgeOutcome { accepted: true, summary_mode: true, code: CODE_SUMMARY_DEGRADED });
            }
            QuotaVerdict::Pass => {}
        }
        // 全量放行：入出箱 + 成功清零 + 半开闭合。
        self.outbox_push(*evt);
        self.forwarded = self.forwarded.saturating_add(1);
        self.accept_reset();
        self.audit_push(day, evt.id, evt.class, 0, true);
        Ok(BridgeOutcome { accepted: true, summary_mode: false, code: 0 })
    }

    /// 放行善后：连续违规清零；半开探针通过 → 闭合。
    fn accept_reset(&mut self) {
        self.consecutive_violations = 0;
        if self.state == BridgeState::HalfOpen {
            self.state = BridgeState::Closed;
            self.probe_used = false;
        }
    }

    /// 拉取出箱队列（U05 侧按域号 0x0505 取件；取空即清）。
    pub fn drain_outbox(&mut self) -> Vec<Evt> {
        core::mem::take(&mut self.outbox)
    }

    /// 桥接审计账尾部 n 笔（面板行）。
    pub fn audit_tail(&self, n: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let total = self.audit.len();
        let start = if total > n { total - n } else { 0 };
        let mut i = start;
        while i < total {
            let r = &self.audit[i];
            lines.push(format!(
                "桥账 day{} evt0x{:08X} {} 码0x{:04X} {}",
                r.day,
                r.event_id,
                r.class.name(),
                r.code,
                if r.accepted { "放行" } else { "拦拒" },
            ));
            i += 1;
        }
        lines
    }

    /// 桥接状态行。
    pub fn state_line(&self) -> String {
        let s = match self.state {
            BridgeState::Closed => "闭合".to_string(),
            BridgeState::Open { reopen_day } => format!("熔断开·重开day{}", reopen_day),
            BridgeState::HalfOpen => "半开·单探针".to_string(),
        };
        format!(
            "桥 {:#06X}域→{:#06X}域 {} 连续违规{} 累计违规{} 熔断{}次 全量{} 摘要{} 配额丢{} 出箱{} 审计{}",
            LOCAL_DOMAIN_ID,
            U05_DOMAIN_ID,
            s,
            self.consecutive_violations,
            self.violations_total,
            self.fused_count,
            self.forwarded,
            self.forwarded_summary,
            self.quota_dropped,
            self.outbox.len(),
            self.audit.len(),
        )
    }

    /// 指纹折入（状态 + 计数 + 配额 + 账面——顺序稳定）。
    pub fn fold_fingerprint(&self, acc: u32) -> u32 {
        let mut a = acc;
        a = fnv_fold_u32(a, match self.state {
            BridgeState::Closed => 0,
            BridgeState::Open { .. } => 1,
            BridgeState::HalfOpen => 2,
        });
        a = fnv_fold_u32(a, self.last_reopen_day);
        a = fnv_fold_u32(a, self.consecutive_violations);
        a = fnv_fold_u32(a, self.violations_total);
        a = fnv_fold_u32(a, self.fused_count);
        a = fnv_fold_u32(a, self.forwarded as u32);
        a = fnv_fold_u32(a, self.forwarded_summary as u32);
        a = fnv_fold_u32(a, self.quota_dropped);
        a = self.quota.fold_fingerprint(a);
        a = fnv_fold_u32(a, self.audit.len() as u32);
        let mut i = 0;
        while i < self.audit.len() {
            let r = &self.audit[i];
            a = fnv_fold_u32(a, r.day);
            a = fnv_fold_u32(a, r.event_id);
            a = fnv_fold_u32(a, r.class.index() as u32);
            a = fnv_fold_u32(a, r.code as u32);
            a = fnv_fold_byte(a, if r.accepted { 1 } else { 0 });
            i += 1;
        }
        a = fnv_fold_u32(a, self.outbox.len() as u32);
        let mut j = 0;
        while j < self.outbox.len() {
            a = fnv_fold_u32(a, self.outbox[j].id);
            a = fnv_fold_u32(a, self.outbox[j].field_mask);
            j += 1;
        }
        a
    }
}

// ---------------------------------------------------------------------------
// 八、发布订阅总线（EvtBusCore · 投递 O(订阅数) · 租约期 · 投递账）
// ---------------------------------------------------------------------------

/// 订阅表容量（投递 O(订阅数) 的常量上界）。
pub const SUB_TABLE_CAP: usize = 32;

/// 事件账容量（逐事件投递账；满则显性拒收——不静默挤占历史账）。
pub const LEDGER_CAP: usize = 128;

/// 订阅租约期（日拍；到期回收走 [`EvtBusCore::sweep`]，写死口径 30 日）。
pub const LEASE_DAYS: u32 = 30;

/// 发布者登记上限。
pub const PUBLISHER_CAP: usize = 16;

/// 事件 id 基址（账面可读口径：0x4608xxxx = 任务号 F4608 域内自增）。
pub const EVT_ID_BASE: u32 = 0x4608_0000;

/// 宿主预登记发布者（name, id）——启动即入配额账（写死口径）。
pub const HOST_PUBLISHERS: [(&str, u16); 4] = [
    ("kernel.core", 0xE001),
    ("plugin.host", 0xE002),
    ("input.route", 0xE003),
    ("data.sync", 0xE004),
];

/// 发布者登记账一笔。
#[derive(Clone, Debug)]
pub struct PublisherReg {
    /// 发布者 id。
    pub publisher: u16,
    /// 登记名。
    pub name: String,
    /// 准入日拍。
    pub registered_day: u32,
    /// 累计发布笔数。
    pub published: u64,
    /// 累计被拒笔数（未登记/账满等显性拒绝）。
    pub rejected: u64,
}

/// 订阅账一笔（订阅者/类过滤/权限档/租约期——锚点"订阅裁剪表"数据面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subscription {
    /// 订阅账 id（自增）。
    pub sub_id: u32,
    /// 订阅者 id（插件/宿主句柄）。
    pub subscriber: u16,
    /// 类过滤位（bit0 System / bit1 UI / bit2 DataChange）。
    pub class_bits: u8,
    /// 权限档（裁剪依据，见 [`PERM_FIELD_MASKS`]）。
    pub tier: u8,
    /// 授权日拍。
    pub granted_day: u32,
    /// 租约到期日拍（含当日有效；`day > lease_until` 即过期）。
    pub lease_until: u32,
    /// 累计投递笔数。
    pub delivered: u64,
    /// 累计摘要降级投递笔数。
    pub degraded_summaries: u64,
}

impl Subscription {
    /// 类过滤描述（面板用：Sys/UI/DC 组合）。
    pub fn class_desc(&self) -> String {
        let mut s = String::new();
        if self.class_bits & EventClass::System.bit() != 0 {
            s.push_str("Sys");
        }
        if self.class_bits & EventClass::UserInteraction.bit() != 0 {
            if !s.is_empty() {
                s.push('|');
            }
            s.push_str("UI");
        }
        if self.class_bits & EventClass::DataChange.bit() != 0 {
            if !s.is_empty() {
                s.push('|');
            }
            s.push_str("DC");
        }
        if s.is_empty() {
            s.push_str("无");
        }
        s
    }
}

/// 投递账一笔（逐事件：事件 id/类/发布者/投递数——锚点"投递账"数据面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeliveryRecord {
    /// 事件 id。
    pub event_id: u32,
    /// 事件类。
    pub class: EventClass,
    /// 发布者 id。
    pub publisher: u16,
    /// 发布日拍。
    pub day: u32,
    /// 实际投递订阅者数。
    pub delivered: u32,
    /// 被裁剪（字段可见性收窄）的订阅者数。
    pub clipped_subs: u32,
    /// 是否摘要降级投递。
    pub summary_mode: bool,
    /// 是否整件丢弃（硬限流/系统禁降级）。
    pub dropped: bool,
    /// 投递时撞见的过期租约订阅者数。
    pub expired_subs_seen: u32,
    /// 本笔诊断码（0 = 全量正常投递）。
    pub code: u16,
    /// 实投载荷字段宽度（摘要降级后为投影掩码位数——账面可读）。
    pub payload_fields: u32,
}

impl DeliveryRecord {
    /// 账行（面板用）。
    pub fn as_line(&self) -> String {
        format!(
            "账 evt0x{:08X} {} 发布者0x{:04X} day{} 投递{} 被裁{} 载荷{} {} 码0x{:04X}",
            self.event_id,
            self.class.name(),
            self.publisher,
            self.day,
            self.delivered,
            self.clipped_subs,
            self.payload_fields,
            if self.dropped {
                "丢弃"
            } else if self.summary_mode {
                "摘要"
            } else {
                "全量"
            },
            self.code,
        )
    }
}

/// 单次发布结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublishOutcome {
    /// 事件 id（丢弃笔也有 id——账面可追）。
    pub event_id: u32,
    /// 事件类。
    pub class: EventClass,
    /// 实际投递订阅者数。
    pub delivered: u32,
    /// 被裁剪订阅者数。
    pub clipped_subs: u32,
    /// 是否摘要降级投递。
    pub summary_mode: bool,
    /// 是否整件丢弃。
    pub dropped: bool,
    /// 是否"系统禁降级而弃"（System 类超软限时的显性丢弃理由）。
    pub degrade_refused: bool,
    /// 投递时撞见的过期租约订阅者数。
    pub expired_seen: u32,
    /// 本笔诊断码（0 = 全量正常投递）。
    pub code: u16,
    /// 实投载荷字段宽度（摘要降级后为投影掩码位数）。
    pub payload_fields: u32,
}

impl PublishOutcome {
    /// 结论行（面板用）。
    pub fn as_line(&self) -> String {
        format!(
            "发布 evt0x{:08X} {} 投递{} 被裁{} 载荷{} {} 码0x{:04X}",
            self.event_id,
            self.class.name(),
            self.delivered,
            self.clipped_subs,
            self.payload_fields,
            if self.dropped {
                if self.degrade_refused { "丢弃·禁降级" } else { "丢弃·硬限流" }
            } else if self.summary_mode {
                "摘要降级"
            } else {
                "全量"
            },
            self.code,
        )
    }
}

/// 插件事件总线核心：发布者登记 × 订阅表 × 配额器 × 裁剪器 × 桥接 × 投递账。
///
/// 性能口径（锚点逐项）：投递 **O(订阅数)**（[`SUB_TABLE_CAP`]=32 上界）；
/// 裁剪 **O(1)**（掩码投影）；限流 **O(1)**（窗口总数 vs 阈值一次比较）。
#[derive(Debug)]
pub struct EvtBusCore {
    next_event_id: u32,
    next_sub_id: u32,
    /// 订阅表（锚点"订阅裁剪表"）。
    subs: Vec<Subscription>,
    /// 发布者登记账。
    publishers: Vec<PublisherReg>,
    /// 逐事件投递账（锚点"投递账"）。
    ledger: Vec<DeliveryRecord>,
    /// 总线流量配额器（每发布者滑动窗，宽参 12/20）。
    quota: FlowQuota,
    /// 按权裁剪器。
    clipper: PermClipper,
    /// 受控桥接（同构不同域）。
    bridge: DomainBridge,
    /// 累计发布笔数（含丢弃）。
    events_total: u64,
    /// 累计投递人次。
    delivered_total: u64,
    /// 累计整件丢弃笔数。
    dropped_total: u64,
    /// 累计摘要降级笔数。
    summary_total: u64,
    /// 距上次 sweep 撞见的过期租约订阅者数（health 体检项 1）。
    expired_seen: u32,
    /// 风暴注入轮次（每轮换新发布者 id——保证每轮窗口新鲜，检出口径恒定）。
    storm_round: u32,
    /// 封印指纹（audit_seal 对账用；指纹口径不含本字段）。
    sealed: Option<u32>,
    /// 启动日拍。
    boot_day: u32,
}

impl EvtBusCore {
    /// 启动总线：预登记宿主发布者并建档配额。
    pub fn boot(day: u32) -> Result<EvtBusCore, EvtBusError> {
        let mut bus = EvtBusCore {
            next_event_id: EVT_ID_BASE,
            next_sub_id: 1,
            subs: Vec::new(),
            publishers: Vec::new(),
            ledger: Vec::new(),
            quota: FlowQuota::bus_default(),
            clipper: PermClipper::new(),
            bridge: DomainBridge::new(day),
            events_total: 0,
            delivered_total: 0,
            dropped_total: 0,
            summary_total: 0,
            expired_seen: 0,
            storm_round: 0,
            sealed: None,
            boot_day: day,
        };
        let mut i = 0;
        while i < HOST_PUBLISHERS.len() {
            bus.register_publisher(HOST_PUBLISHERS[i].0, HOST_PUBLISHERS[i].1, day)?;
            i += 1;
        }
        Ok(bus)
    }

    // ---------------- 发布者准入 ----------------

    /// 登记发布者（幂等：重复 id 刷新名字；配额账随之建档）。
    pub fn register_publisher(
        &mut self,
        name: &str,
        publisher: u16,
        day: u32,
    ) -> Result<(), EvtBusError> {
        let mut i = 0;
        while i < self.publishers.len() {
            if self.publishers[i].publisher == publisher {
                self.publishers[i].name = name.to_string();
                return self.quota.admit(publisher, day);
            }
            i += 1;
        }
        if self.publishers.len() >= PUBLISHER_CAP {
            return Err(EvtBusError::QuotaTableFull { cap: PUBLISHER_CAP as u32 });
        }
        self.quota.admit(publisher, day)?;
        self.publishers.push(PublisherReg {
            publisher,
            name: name.to_string(),
            registered_day: day,
            published: 0,
            rejected: 0,
        });
        Ok(())
    }

    /// 注销发布者（配额账销账；留名账移除）。
    pub fn retire_publisher(&mut self, publisher: u16) -> bool {
        let mut i = 0;
        while i < self.publishers.len() {
            if self.publishers[i].publisher == publisher {
                let _ = self.publishers.remove(i);
                return self.quota.retire(publisher);
            }
            i += 1;
        }
        false
    }

    /// 发布者是否已登记。
    pub fn publisher_admitted(&self, publisher: u16) -> bool {
        self.quota.admitted(publisher)
    }

    // ---------------- 订阅（准入即裁剪授权） ----------------

    /// 订阅：逐类准入审（档位 ≤ [`CLASS_REQUIRED_TIER`] 上限）→ 建账。
    ///
    /// * 越权订阅 → [`EvtBusError::SubDenied`]（锚点"越权订阅→拒绝"）。
    /// * 类过滤位非法/空、档位越界 → `SubDenied`（`got_tier=0xFF` 哨兵口径，
    ///   detail 里说明）。
    /// * 同订阅者重复订阅 → 原位刷新（类过滤/档位/租约），账面计数保留。
    pub fn subscribe(
        &mut self,
        subscriber: u16,
        class_bits: u8,
        tier: u8,
        day: u32,
    ) -> Result<u32, EvtBusError> {
        if class_bits == 0
            || class_bits & !0b111 != 0
            || tier as usize >= PERM_TIER_COUNT
        {
            return Err(EvtBusError::SubDenied {
                subscriber,
                class: EventClass::System,
                required_tier: 0,
                got_tier: 0xFF,
            });
        }
        let mut c = 0;
        while c < 3 {
            if class_bits & EventClass::from_index(c).bit() != 0 {
                let required = CLASS_REQUIRED_TIER[c];
                if tier > required {
                    return Err(EvtBusError::SubDenied {
                        subscriber,
                        class: EventClass::from_index(c),
                        required_tier: required,
                        got_tier: tier,
                    });
                }
            }
            c += 1;
        }
        let mut i = 0;
        while i < self.subs.len() {
            if self.subs[i].subscriber == subscriber {
                self.subs[i].class_bits = class_bits;
                self.subs[i].tier = tier;
                self.subs[i].granted_day = day;
                self.subs[i].lease_until = day.saturating_add(LEASE_DAYS);
                return Ok(self.subs[i].sub_id);
            }
            i += 1;
        }
        if self.subs.len() >= SUB_TABLE_CAP {
            return Err(EvtBusError::SubTableFull { cap: SUB_TABLE_CAP as u32 });
        }
        let sub_id = self.next_sub_id;
        self.next_sub_id = self.next_sub_id.wrapping_add(1);
        self.subs.push(Subscription {
            sub_id,
            subscriber,
            class_bits,
            tier,
            granted_day: day,
            lease_until: day.saturating_add(LEASE_DAYS),
            delivered: 0,
            degraded_summaries: 0,
        });
        Ok(sub_id)
    }

    /// 原地续租（仅未过期订阅可续——过期续租被拒，防僵尸提权：
    /// 须退订后重新 subscribe 走完整准入）。
    pub fn renew_lease(&mut self, sub_id: u32, day: u32) -> Result<u32, EvtBusError> {
        let mut i = 0;
        while i < self.subs.len() {
            if self.subs[i].sub_id == sub_id {
                let until = self.subs[i].lease_until;
                if day > until {
                    return Err(EvtBusError::LeaseExpired { sub_id, until_day: until, day });
                }
                let new_until = day.saturating_add(LEASE_DAYS);
                self.subs[i].lease_until = new_until;
                return Ok(new_until);
            }
            i += 1;
        }
        Ok(0)
    }

    /// 退订（幂等：不存在返回 false，不占诊断码）。
    pub fn unsubscribe(&mut self, sub_id: u32) -> bool {
        let mut i = 0;
        while i < self.subs.len() {
            if self.subs[i].sub_id == sub_id {
                let _ = self.subs.remove(i);
                return true;
            }
            i += 1;
        }
        false
    }

    /// 到期回收：清出 `day > lease_until` 的订阅，返回回收笔数；
    /// 同时清零"过期租约未回收"体检计数。
    pub fn sweep(&mut self, day: u32) -> u32 {
        let mut reclaimed: u32 = 0;
        let mut i = 0;
        while i < self.subs.len() {
            if day > self.subs[i].lease_until {
                let _ = self.subs.remove(i);
                reclaimed = reclaimed.saturating_add(1);
            } else {
                i += 1;
            }
        }
        self.expired_seen = 0;
        reclaimed
    }

    /// 活跃订阅数。
    pub fn sub_count(&self) -> usize {
        self.subs.len()
    }

    // ---------------- 发布（配额 → 投递 → 账） ----------------

    /// 发布（宽松口径：摘要降级放行为正常路径）。
    pub fn publish(
        &mut self,
        publisher: u16,
        class: EventClass,
        field_mask: u32,
        vals: [u32; EVT_VAL_SLOTS],
        day: u32,
    ) -> Result<PublishOutcome, EvtBusError> {
        self.publish_ex(publisher, class, field_mask, vals, day, false)
    }

    /// 发布（严格口径：at-least-once 契约方不接受摘要降级——降级即显性退回）。
    pub fn publish_strict(
        &mut self,
        publisher: u16,
        class: EventClass,
        field_mask: u32,
        vals: [u32; EVT_VAL_SLOTS],
        day: u32,
    ) -> Result<PublishOutcome, EvtBusError> {
        self.publish_ex(publisher, class, field_mask, vals, day, true)
    }

    /// 发布全流程（锚点三闸复刻：准入闸 → 配额闸 → 投递面）：
    ///
    /// 1. 发布者准入审：未登记 → [`EvtBusError::PublisherUnknown`]。
    /// 2. 字段掩码审：越位 → [`EvtBusError::FieldMaskOob`]。
    /// 3. 账容量预检：投递账满 → [`EvtBusError::LedgerFull`]（事件未投递，
    ///    不产生半账——显性拒收）。
    /// 4. 配额闸（O(1) 判决）：硬限超 → 整件丢弃 + 显性计数；软限超 →
    ///    摘要降级（System 类禁降级 → 改为整件丢弃，理由 `degrade_refused`）；
    ///    严格发布方遇降级 → [`EvtBusError::SummaryDegraded`] 退回。
    /// 5. 投递面（**O(订阅数)**）：逐订阅者——类过滤位、租约期、按档裁剪。
    /// 6. 投递账逐事件落账。
    pub fn publish_ex(
        &mut self,
        publisher: u16,
        class: EventClass,
        field_mask: u32,
        vals: [u32; EVT_VAL_SLOTS],
        day: u32,
        strict: bool,
    ) -> Result<PublishOutcome, EvtBusError> {
        // 1) 发布者准入（未登记连计数都不留——先登记先准入）。
        let mut reg_idx: Option<usize> = None;
        let mut i = 0;
        while i < self.publishers.len() {
            if self.publishers[i].publisher == publisher {
                reg_idx = Some(i);
                i = self.publishers.len();
            } else {
                i += 1;
            }
        }
        let reg_idx = match reg_idx {
            Some(idx) => idx,
            None => return Err(EvtBusError::PublisherUnknown { publisher }),
        };
        // 2) 字段掩码审。
        if field_mask & !EVT_FIELD_MASK_ALL != 0 {
            self.bump_rejected(reg_idx);
            return Err(EvtBusError::FieldMaskOob { mask: field_mask });
        }
        // 3) 账容量预检（显性拒收——账满不接受半账）。
        if self.ledger.len() >= LEDGER_CAP {
            self.bump_rejected(reg_idx);
            return Err(EvtBusError::LedgerFull { cap: LEDGER_CAP as u32 });
        }
        // 4) 配额闸（限流 O(1) 判据落点）。
        let weight = CLASS_QUOTA_WEIGHT[class.index()];
        let verdict = self.quota.charge_for(publisher, weight, day)?;
        let mut dropped = false;
        let mut summary_mode = false;
        let mut degrade_refused = false;
        let mut code: u16 = 0;
        match verdict {
            QuotaVerdict::Drop => {
                dropped = true;
                code = CODE_STORM_THROTTLED;
                self.dropped_total = self.dropped_total.saturating_add(1);
            }
            QuotaVerdict::Summary => {
                if !CLASS_SUMMARY_OK[class.index()] {
                    // System 禁摘要降级：宁可整件丢弃并显性计数，不给残缺载荷。
                    dropped = true;
                    degrade_refused = true;
                    code = CODE_SUMMARY_DEGRADED;
                    self.dropped_total = self.dropped_total.saturating_add(1);
                } else if strict {
                    self.bump_rejected(reg_idx);
                    return Err(EvtBusError::SummaryDegraded {
                        publisher,
                        event_id: self.next_event_id,
                    });
                } else {
                    summary_mode = true;
                    code = CODE_SUMMARY_DEGRADED;
                    self.summary_total = self.summary_total.saturating_add(1);
                }
            }
            QuotaVerdict::Pass => {}
        }
        let event_id = self.next_event_id;
        self.next_event_id = self.next_event_id.wrapping_add(1);
        self.events_total = self.events_total.saturating_add(1);
        // 封套落形（掩码已审，直构无错路径；值槽随封套入账面口径）。
        let evt = Evt { id: event_id, class, publisher, day, field_mask, vals };
        // 丢弃笔：落账（delivered=0, dropped=true）即回——账面可追不静默。
        if dropped {
            self.ledger.push(DeliveryRecord {
                event_id,
                class,
                publisher,
                day,
                delivered: 0,
                clipped_subs: 0,
                summary_mode: false,
                dropped: true,
                expired_subs_seen: 0,
                code,
                payload_fields: evt.field_count(),
            });
            self.publishers[reg_idx].published = self.publishers[reg_idx].published.saturating_add(1);
            return Ok(PublishOutcome {
                event_id,
                class,
                delivered: 0,
                clipped_subs: 0,
                summary_mode: false,
                dropped: true,
                degrade_refused,
                expired_seen: 0,
                code,
                payload_fields: evt.field_count(),
            });
        }
        // 5) 投递面（O(订阅数)）：摘要降级先投影保留掩码，再逐订阅者按档裁剪。
        let eff_mask = if summary_mode {
            field_mask & CLASS_SUMMARY_MASK[class.index()]
        } else {
            field_mask
        };
        let mut delivered: u32 = 0;
        let mut clipped_subs: u32 = 0;
        let mut expired_now: u32 = 0;
        let mut j = 0;
        while j < self.subs.len() {
            let hit = self.subs[j].class_bits & class.bit() != 0;
            if hit {
                let expired = day > self.subs[j].lease_until;
                if expired {
                    // 租约过期：本拍不投（sweep 统一回收），撞见即计数。
                    expired_now = expired_now.saturating_add(1);
                } else {
                    let tier = self.subs[j].tier;
                    let clip = self.clipper.clip(tier, eff_mask);
                    let s = &mut self.subs[j];
                    s.delivered = s.delivered.saturating_add(1);
                    if summary_mode {
                        s.degraded_summaries = s.degraded_summaries.saturating_add(1);
                    }
                    delivered = delivered.saturating_add(1);
                    if clip.is_clipped() {
                        clipped_subs = clipped_subs.saturating_add(1);
                    }
                }
            }
            j += 1;
        }
        self.delivered_total = self.delivered_total.saturating_add(delivered as u64);
        self.expired_seen = self.expired_seen.saturating_add(expired_now);
        self.publishers[reg_idx].published = self.publishers[reg_idx].published.saturating_add(1);
        // 6) 投递账逐事件落账。
        self.ledger.push(DeliveryRecord {
            event_id,
            class,
            publisher,
            day,
            delivered,
            clipped_subs,
            summary_mode,
            dropped: false,
            expired_subs_seen: expired_now,
            code,
            payload_fields: eff_mask.count_ones(),
        });
        Ok(PublishOutcome {
            event_id,
            class,
            delivered,
            clipped_subs,
            summary_mode,
            dropped: false,
            degrade_refused: false,
            expired_seen: expired_now,
            code,
            payload_fields: eff_mask.count_ones(),
        })
    }

    /// 发布者被拒计数 +1（掩码越位/账满等显性拒绝路径）。
    fn bump_rejected(&mut self, reg_idx: usize) {
        if reg_idx < self.publishers.len() {
            self.publishers[reg_idx].rejected = self.publishers[reg_idx].rejected.saturating_add(1);
        }
    }

    /// 桥接转发透传（受控桥接闸门见 [`DomainBridge::forward`]）。
    pub fn bridge_forward(&mut self, evt: &Evt, day: u32) -> Result<BridgeOutcome, EvtBusError> {
        self.bridge.forward(evt, day)
    }

    /// 拉取桥接出箱（U05 侧按域号取件）。
    pub fn bridge_drain_outbox(&mut self) -> Vec<Evt> {
        self.bridge.drain_outbox()
    }

    /// 桥接状态行。
    pub fn bridge_state_line(&self) -> String {
        self.bridge.state_line()
    }

    /// 桥接状态读数（判据侧直接断言状态机）。
    pub fn bridge_state(&self) -> BridgeState {
        self.bridge.state()
    }

    // ---------------- 账面读数 ----------------

    /// 投递账笔数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 累计发布笔数。
    pub fn events_total(&self) -> u64 {
        self.events_total
    }

    /// 累计投递人次。
    pub fn delivered_total(&self) -> u64 {
        self.delivered_total
    }

    /// 累计丢弃笔数。
    pub fn dropped_total(&self) -> u64 {
        self.dropped_total
    }

    /// 累计摘要降级笔数。
    pub fn summary_total(&self) -> u64 {
        self.summary_total
    }

    /// 裁剪器读数（被裁事件数 / 被裁字段位累计）。
    pub fn clip_stats(&self) -> (u64, u64) {
        (self.clipper.clipped_events, self.clipper.masked_field_total)
    }

    /// 投递账尾部 n 笔（面板行）。
    pub fn ledger_tail(&self, n: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let total = self.ledger.len();
        let start = if total > n { total - n } else { 0 };
        let mut i = start;
        while i < total {
            lines.push(self.ledger[i].as_line());
            i += 1;
        }
        lines
    }

    /// 订阅表账行。
    pub fn subscription_board(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut i = 0;
        while i < self.subs.len() {
            let s = &self.subs[i];
            lines.push(format!(
                "订阅 #{:<3} 订阅者0x{:04X} 类[{}] 档{}{} 租约至day{} 已投{} 降级摘要{}",
                s.sub_id,
                s.subscriber,
                s.class_desc(),
                s.tier,
                TIER_NAMES[s.tier as usize],
                s.lease_until,
                s.delivered,
                s.degraded_summaries,
            ));
            i += 1;
        }
        lines
    }

    /// 发布者账行。
    pub fn publisher_board(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut i = 0;
        while i < self.publishers.len() {
            let p = &self.publishers[i];
            lines.push(format!(
                "发布者 0x{:04X} {:<12} day{} 发布{} 被拒{} 窗总{}",
                p.publisher,
                p.name,
                p.registered_day,
                p.published,
                p.rejected,
                self.quota.window_of(p.publisher),
            ));
            i += 1;
        }
        lines
    }
}

// ---------------------------------------------------------------------------
// 九、FNV 指纹 + 封印对账 + 健康体检
// ---------------------------------------------------------------------------

/// FNV-1a 32 位偏移基（标准参数 0x811C9DC5，写死口径）。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

/// FNV-1a 32 位素数（标准参数 0x01000193，写死口径）。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// FNV 折入单字节。
pub fn fnv_fold_byte(acc: u32, b: u8) -> u32 {
    (acc ^ (b as u32)).wrapping_mul(FNV_PRIME)
}

/// FNV 折入 u32（小端四字节，顺序稳定）。
pub fn fnv_fold_u32(acc: u32, v: u32) -> u32 {
    let a = fnv_fold_byte(acc, (v & 0xFF) as u8);
    let a = fnv_fold_byte(a, ((v >> 8) & 0xFF) as u8);
    let a = fnv_fold_byte(a, ((v >> 16) & 0xFF) as u8);
    fnv_fold_byte(a, ((v >> 24) & 0xFF) as u8)
}

/// FNV 折入字符串（逐字节，不含结尾零）。
pub fn fnv_fold_str(acc: u32, s: &str) -> u32 {
    let bytes = s.as_bytes();
    let mut a = acc;
    let mut i = 0;
    while i < bytes.len() {
        a = fnv_fold_byte(a, bytes[i]);
        i += 1;
    }
    a
}

/// 硬丢率健康阈（百分比；超过即降级，写死口径）。
pub const HEALTH_DROP_PCT_LIMIT: u32 = 30;

/// 健康体检报告（六项；`code` 0 = 全绿，否则 [`CODE_HEALTH_DEGRADED`]）。
#[derive(Clone, Debug)]
pub struct HealthReport {
    /// 是否降级。
    pub degraded: bool,
    /// 诊断码（0 / [`CODE_HEALTH_DEGRADED`]）。
    pub code: u16,
    /// 失败项数。
    pub failed_checks: u32,
    /// 失败项中文说明（≤8 行封顶——面板友好）。
    pub issues: Vec<String>,
    /// 活跃订阅数。
    pub subs_active: usize,
    /// 账面占用。
    pub ledger_used: usize,
    /// 累计发布。
    pub events_total: u64,
    /// 累计丢弃。
    pub dropped_total: u64,
    /// 硬丢率（百分比整值）。
    pub drop_pct: u32,
}

impl HealthReport {
    /// 单行摘要（域本色一行——面板/日志共用）。
    pub fn as_line(&self) -> String {
        format!(
            "健康 {} 失败{}项 订阅{} 账{}/{} 发布{} 丢{}({}%)",
            if self.degraded { "降级" } else { "全绿" },
            self.failed_checks,
            self.subs_active,
            self.ledger_used,
            LEDGER_CAP,
            self.events_total,
            self.dropped_total,
            self.drop_pct,
        )
    }
}

impl EvtBusCore {
    /// 全态 FNV-1a 指纹（订阅表 + 发布者账 + 配额 + 裁剪计数 + 桥接 + 投递账
    /// + 总计数；不含封印字段本身——否则 seal 后即自漂）。
    pub fn fingerprint(&self) -> u32 {
        let mut a = fnv_fold_u32(FNV_OFFSET, self.boot_day);
        a = fnv_fold_u32(a, self.next_event_id);
        a = fnv_fold_u32(a, self.next_sub_id);
        a = fnv_fold_u32(a, self.events_total as u32);
        a = fnv_fold_u32(a, self.delivered_total as u32);
        a = fnv_fold_u32(a, self.dropped_total as u32);
        a = fnv_fold_u32(a, self.summary_total as u32);
        a = fnv_fold_u32(a, self.expired_seen);
        a = fnv_fold_u32(a, self.subs.len() as u32);
        let mut i = 0;
        while i < self.subs.len() {
            let s = &self.subs[i];
            a = fnv_fold_u32(a, s.sub_id);
            a = fnv_fold_u32(a, s.subscriber as u32);
            a = fnv_fold_u32(a, s.class_bits as u32);
            a = fnv_fold_u32(a, s.tier as u32);
            a = fnv_fold_u32(a, s.granted_day);
            a = fnv_fold_u32(a, s.lease_until);
            a = fnv_fold_u32(a, s.delivered as u32);
            a = fnv_fold_u32(a, s.degraded_summaries as u32);
            i += 1;
        }
        a = fnv_fold_u32(a, self.publishers.len() as u32);
        let mut j = 0;
        while j < self.publishers.len() {
            let p = &self.publishers[j];
            a = fnv_fold_u32(a, p.publisher as u32);
            a = fnv_fold_str(a, &p.name);
            a = fnv_fold_u32(a, p.registered_day);
            a = fnv_fold_u32(a, p.published as u32);
            a = fnv_fold_u32(a, p.rejected as u32);
            j += 1;
        }
        a = self.quota.fold_fingerprint(a);
        a = fnv_fold_u32(a, self.clipper.clipped_events as u32);
        a = fnv_fold_u32(a, self.clipper.masked_field_total as u32);
        a = fnv_fold_u32(a, self.clipper.bad_tier_hits as u32);
        a = self.bridge.fold_fingerprint(a);
        a = fnv_fold_u32(a, self.ledger.len() as u32);
        let mut k = 0;
        while k < self.ledger.len() {
            let r = &self.ledger[k];
            a = fnv_fold_u32(a, r.event_id);
            a = fnv_fold_u32(a, r.class.index() as u32);
            a = fnv_fold_u32(a, r.publisher as u32);
            a = fnv_fold_u32(a, r.day);
            a = fnv_fold_u32(a, r.delivered);
            a = fnv_fold_u32(a, r.clipped_subs);
            a = fnv_fold_byte(a, if r.summary_mode { 1 } else { 0 });
            a = fnv_fold_byte(a, if r.dropped { 1 } else { 0 });
            a = fnv_fold_u32(a, r.expired_subs_seen);
            a = fnv_fold_u32(a, r.code as u32);
            k += 1;
        }
        a
    }

    /// 双跑对拍摘要（digest = 指纹 ⊕ 封印存在性——两口径分离：封印动作本身
    /// 不改指纹，但改 digest，封印时点不同即分叉）。
    pub fn digest(&self) -> u32 {
        let a = self.fingerprint();
        let a = fnv_fold_byte(a, if self.sealed.is_some() { 1 } else { 0 });
        match self.sealed {
            Some(v) => fnv_fold_u32(a, v),
            None => a,
        }
    }

    /// 封印当前指纹（审计基准；重复封印覆盖——以最近一次为准）。
    pub fn seal(&mut self) -> u32 {
        let fp = self.fingerprint();
        self.sealed = Some(fp);
        fp
    }

    /// 封印对账：当前指纹 vs 封印值，不符 → [`EvtBusError::FingerprintDrift`]
    /// （有变更无账的漂移检出）。未封印 → 先封后审（恒过）。
    pub fn audit_seal(&self) -> Result<u32, EvtBusError> {
        let cur = self.fingerprint();
        match self.sealed {
            Some(sealed) if sealed != cur => {
                Err(EvtBusError::FingerprintDrift { sealed, current: cur })
            }
            _ => Ok(cur),
        }
    }

    /// 六项健康体检：
    ///
    /// 1. 过期租约未回收（publish 撞见过期订阅者未 sweep）。
    /// 2. 桥接熔断中。
    /// 3. 硬丢率超 [`HEALTH_DROP_PCT_LIMIT`]（风暴常态化的预警）。
    /// 4. 订阅表满（新订阅将显性被拒）。
    /// 5. 投递账满（新发布将显性被拒）。
    /// 6. 桥接连续违规临界（再违一次即熔断——阈值-1 预警档）。
    pub fn health(&self) -> HealthReport {
        let mut issues: Vec<String> = Vec::new();
        let mut failed: u32 = 0;
        if self.expired_seen > 0 {
            failed += 1;
            issues.push(format!("体检1 过期租约未回收 {} 笔——请 sweep", self.expired_seen));
        }
        if let BridgeState::Open { reopen_day } = self.bridge.state() {
            failed += 1;
            issues.push(format!("体检2 桥接熔断中——重开day{}", reopen_day));
        }
        let drop_pct = if self.events_total == 0 {
            0
        } else {
            (self.dropped_total.saturating_mul(100) / self.events_total) as u32
        };
        if drop_pct > HEALTH_DROP_PCT_LIMIT {
            failed += 1;
            issues.push(format!(
                "体检3 硬丢率 {}% > {}%——发布方降频或走摘要通道",
                drop_pct, HEALTH_DROP_PCT_LIMIT
            ));
        }
        if self.subs.len() >= SUB_TABLE_CAP {
            failed += 1;
            issues.push(format!("体检4 订阅表满 {}/{}——新订阅将被拒", self.subs.len(), SUB_TABLE_CAP));
        }
        if self.ledger.len() >= LEDGER_CAP {
            failed += 1;
            issues.push(format!("体检5 投递账满 {}/{}——新发布将被拒", self.ledger.len(), LEDGER_CAP));
        }
        if self.bridge.consecutive_violations().saturating_add(1) >= BRIDGE_FUSE_THRESHOLD {
            failed += 1;
            issues.push(format!(
                "体检6 桥接连续违规 {} 临界（阈值 {}）——再违一次即熔断",
                self.bridge.consecutive_violations(),
                BRIDGE_FUSE_THRESHOLD
            ));
        }
        // issues 面板封顶 8 行。
        while issues.len() > 8 {
            let _ = issues.remove(0);
        }
        HealthReport {
            degraded: failed > 0,
            code: if failed > 0 { CODE_HEALTH_DEGRADED } else { 0 },
            failed_checks: failed,
            issues,
            subs_active: self.subs.len(),
            ledger_used: self.ledger.len(),
            events_total: self.events_total,
            dropped_total: self.dropped_total,
            drop_pct,
        }
    }

    /// 健康门：降级即 Err（判据侧直接用 Result 流转）。
    pub fn health_gate(&self) -> Result<HealthReport, EvtBusError> {
        let rep = self.health();
        if rep.degraded {
            Err(EvtBusError::HealthDegraded { failed_checks: rep.failed_checks })
        } else {
            Ok(rep)
        }
    }
}

// ---------------------------------------------------------------------------
// 十、注入面 + 自检 + 双跑确定性对拍
// ---------------------------------------------------------------------------

/// 自检报告（三路注入各检出 + 常规面抽检 + 封印 + 对拍摘要）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelfCheckReport {
    /// 风暴注入检出（软限降级 + 硬限丢弃双双现形）。
    pub storm_detected: bool,
    /// 越权订阅注入检出（Sandbox 订系统事件被拒）。
    pub unauth_detected: bool,
    /// 桥接滥用注入检出（连续越权熔断 + 冷却半开恢复）。
    pub abuse_detected: bool,
    /// 系统事件全量投递抽检（禁降级语义抽验）。
    pub system_verbatim: bool,
    /// 数据变更事件被裁抽检（Normal 档订阅者撞敏感字段）。
    pub clipped_seen: bool,
    /// 封印指纹。
    pub sealed: u32,
    /// 对拍 digest。
    pub digest: u32,
}

impl SelfCheckReport {
    /// 全部通过？
    pub fn all_pass(&self) -> bool {
        self.storm_detected
            && self.unauth_detected
            && self.abuse_detected
            && self.system_verbatim
            && self.clipped_seen
    }

    /// 单行摘要。
    pub fn as_line(&self) -> String {
        format!(
            "自检 风暴{} 越权订阅{} 桥接滥用{} 系统全量{} 裁剪现形{} 封印0x{:08X} 对拍0x{:08X}",
            if self.storm_detected { "✓" } else { "✗" },
            if self.unauth_detected { "✓" } else { "✗" },
            if self.abuse_detected { "✓" } else { "✗" },
            if self.system_verbatim { "✓" } else { "✗" },
            if self.clipped_seen { "✓" } else { "✗" },
            self.sealed,
            self.digest,
        )
    }
}

impl EvtBusCore {
    /// 注入一：事件风暴。注册风暴发布者，连发 `QUOTA_HARD_LIMIT + 8` 笔
    /// 数据变更事件（权重 2）——预期软限降级与硬限丢弃**双双现形**
    /// （软限不现形 = 降级通道失效；硬限不现形 = 限流失效）。
    pub fn inject_storm(&mut self, day: u32) -> Result<bool, EvtBusError> {
        // 每轮换新发布者 id（0xE0F0 起）——旧轮窗口不污染新轮的检出判定。
        let round = self.storm_round;
        self.storm_round = self.storm_round.saturating_add(1);
        let pid = 0xE0F0u16.saturating_add(round as u16);
        let name = format!("inject.storm{}", round);
        self.register_publisher(&name, pid, day)?;
        let mut vals = [0u32; EVT_VAL_SLOTS];
        vals[0] = 7;
        vals[1] = 1;
        let mask = F_KIND | F_SEQ | F_ENTITY | F_CHECKSUM;
        let mut dropped: u32 = 0;
        let mut summaries: u32 = 0;
        let mut n: u32 = 0;
        while n < QUOTA_HARD_LIMIT.saturating_add(8) {
            let out = self.publish(pid, EventClass::DataChange, mask, vals, day)?;
            if out.dropped {
                dropped = dropped.saturating_add(1);
            }
            if out.summary_mode {
                summaries = summaries.saturating_add(1);
            }
            n = n.saturating_add(1);
        }
        Ok(dropped > 0 && summaries > 0)
    }

    /// 注入二：越权订阅。Sandbox 档（3）订阅系统事件（准入上限 Trusted=1）
    /// ——预期 [`EvtBusError::SubDenied`] 现形。
    pub fn inject_unauth_sub(&mut self, day: u32) -> Result<bool, EvtBusError> {
        match self.subscribe(0xDEAD, EventClass::System.bit(), 3, day) {
            Err(e) => Ok(e.code() == CODE_SUB_DENIED),
            Ok(_) => Ok(false),
        }
    }

    /// 注入三：桥接滥用。连续 [`BRIDGE_FUSE_THRESHOLD`] 笔交互事件撞白名单
    /// ——预期第 3 笔触发 [`EvtBusError::BridgeAbuse`] 熔断；冷却到点后半开
    /// 单探针（白名单类）通过 → 闭合恢复。熔断与恢复双双现形才算检出。
    pub fn inject_bridge_abuse(&mut self, day: u32) -> Result<bool, EvtBusError> {
        let mut vals = [0u32; EVT_VAL_SLOTS];
        vals[0] = 9;
        let rogue = Evt::compose(
            0xF00D_0001,
            EventClass::UserInteraction,
            0xE002,
            day,
            F_KIND | F_KEY | F_BUTTON,
            vals,
        )?;
        let mut fused = false;
        let mut n: u32 = 0;
        while n < BRIDGE_FUSE_THRESHOLD {
            match self.bridge.forward(&rogue, day) {
                Err(EvtBusError::BridgeAbuse { .. }) => fused = true,
                Err(EvtBusError::BridgeClassBlocked { .. }) => {}
                _ => {}
            }
            n = n.saturating_add(1);
        }
        // 冷却到点 → 半开 → 白名单探针通过 → 闭合。
        let later = day.saturating_add(BRIDGE_COOLDOWN_DAYS);
        let probe = Evt::compose(
            0xF00D_0002,
            EventClass::DataChange,
            0xE002,
            later,
            F_KIND | F_SEQ | F_ENTITY,
            vals,
        )?;
        let recovered = match self.bridge.forward(&probe, later) {
            Ok(o) => o.accepted,
            Err(_) => false,
        };
        Ok(fused && recovered && self.bridge.state() == BridgeState::Closed)
    }

    /// 自检全流程（三路注入 + 常规面抽检 + 封印）。
    pub fn run_self_checks(&mut self, day: u32) -> Result<SelfCheckReport, EvtBusError> {
        let storm = self.inject_storm(day)?;
        let unauth = self.inject_unauth_sub(day)?;
        let abuse = self.inject_bridge_abuse(day)?;
        // 常规面：双档订阅 + 三类各一笔发布 + 到期回收空扫。
        let _host = self.subscribe(
            0x0A01,
            EventClass::System.bit() | EventClass::UserInteraction.bit(),
            0,
            day,
        )?;
        let _normal = self.subscribe(
            0x0A02,
            EventClass::UserInteraction.bit() | EventClass::DataChange.bit(),
            2,
            day,
        )?;
        let mut vals = [0u32; EVT_VAL_SLOTS];
        vals[0] = 1;
        vals[1] = 9;
        let o1 = self.publish(
            0xE001,
            EventClass::System,
            F_KIND | F_SEQ | F_CHECKSUM,
            vals,
            day,
        )?;
        let _o2 = self.publish(
            0xE003,
            EventClass::UserInteraction,
            F_KIND | F_KEY | F_X | F_Y | F_BUTTON | F_MODIFIER,
            vals,
            day,
        )?;
        let o3 = self.publish(
            0xE004,
            EventClass::DataChange,
            F_KIND | F_SEQ | F_OLD_VAL | F_NEW_VAL | F_ENTITY,
            vals,
            day,
        )?;
        let _ = self.sweep(day);
        let sealed = self.seal();
        Ok(SelfCheckReport {
            storm_detected: storm,
            unauth_detected: unauth,
            abuse_detected: abuse,
            system_verbatim: !o1.summary_mode && !o1.dropped && o1.delivered > 0,
            clipped_seen: o3.clipped_subs > 0,
            sealed,
            digest: self.digest(),
        })
    }
}

/// 固定对拍脚本（两个实例各跑一遍——任何隐藏可变态都会让 digest 分叉）。
pub fn run_scenario(bus: &mut EvtBusCore, day: u32) -> Result<u32, EvtBusError> {
    let rep = bus.run_self_checks(day)?;
    // 常规续演：一窗配额推进 + 一笔退订 + 一笔注销。
    let _ = bus.sweep(day.saturating_add(QUOTA_WINDOW_TICKS));
    let _ = bus.unsubscribe(1);
    let _ = bus.retire_publisher(0xE003);
    let mut acc = rep.digest;
    acc = fnv_fold_u32(acc, day);
    acc = fnv_fold_byte(acc, if rep.storm_detected { 1 } else { 0 });
    acc = fnv_fold_byte(acc, if rep.unauth_detected { 1 } else { 0 });
    acc = fnv_fold_byte(acc, if rep.abuse_detected { 1 } else { 0 });
    acc = fnv_fold_byte(acc, if rep.all_pass() { 1 } else { 0 });
    acc = fnv_fold_u32(acc, bus.sub_count() as u32);
    acc = fnv_fold_u32(acc, bus.ledger_len() as u32);
    Ok(acc)
}

/// 双跑确定性对拍报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualRunReport {
    /// 两实例一致的 digest。
    pub digest: u32,
    /// 实例 A 账面笔数。
    pub a_ledger: u32,
    /// 实例 B 账面笔数。
    pub b_ledger: u32,
}

/// 双跑确定性对拍：同日序建账两实例 → 同脚本 → digest 必须全等。
/// 不等 → [`EvtBusError::DualRunMismatch`]（确定性是账本的地基——账必须可重放）。
pub fn dual_run_check(day: u32) -> Result<DualRunReport, EvtBusError> {
    let mut a = EvtBusCore::boot(day)?;
    let mut b = EvtBusCore::boot(day)?;
    let da = run_scenario(&mut a, day)?;
    let db = run_scenario(&mut b, day)?;
    if da != db {
        return Err(EvtBusError::DualRunMismatch { lhs: da, rhs: db });
    }
    Ok(DualRunReport {
        digest: da,
        a_ledger: a.ledger_len() as u32,
        b_ledger: b.ledger_len() as u32,
    })
}

// ---------------------------------------------------------------------------
// 十一、单元测试（探针壳 #[cfg(test)] 下运行；unwrap/assert 仅测试路径）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 冒烟 1：发布订阅投递账——逐事件账面（id/类/发布者/投递数）落账正确，
    /// 订阅侧计数同步，投递 O(订阅数) 口径可读。
    #[test]
    fn pub_sub_delivery_ledger() {
        let mut bus = EvtBusCore::boot(10).unwrap();
        let s1 = bus.subscribe(0x0A01, EventClass::System.bit(), 0, 10).unwrap();
        let s2 = bus
            .subscribe(0x0A02, EventClass::System.bit() | EventClass::UserInteraction.bit(), 1, 10)
            .unwrap();
        assert!(s1 != s2);
        let mut vals = [0u32; EVT_VAL_SLOTS];
        vals[0] = 42;
        vals[1] = 7;
        let out = bus
            .publish(0xE001, EventClass::System, F_KIND | F_SEQ | F_LEVEL, vals, 11)
            .unwrap();
        assert!(!out.dropped && !out.summary_mode);
        assert_eq!(out.delivered, 2);
        assert_eq!(out.code, 0);
        assert_eq!(bus.ledger_len(), 1);
        let rec_lines = bus.ledger_tail(1);
        assert_eq!(rec_lines.len(), 1);
        assert!(rec_lines[0].contains("evt0x"));
        assert!(rec_lines[0].contains("发布者0xE001"));
        assert!(rec_lines[0].contains("投递2"));
        let board = bus.subscription_board();
        assert_eq!(board.len(), 2);
        assert!(board[0].contains("Sys"));
        assert!(bus.delivered_total() == 2);
    }

    /// 冒烟 2：裁剪矩阵——Sandbox 只见 kind/seq/entity，Normal 撞 old_val/new_val
    /// 必被裁；矩阵 const 与档位掩码一致；Host 全量原样。
    #[test]
    fn clip_matrix_and_projection() {
        let mut bus = EvtBusCore::boot(0).unwrap();
        let mut c = PermClipper::new();
        let mask = F_KIND | F_OLD_VAL | F_NEW_VAL | F_ENTITY | F_CHECKSUM;
        // Host：原样。
        match c.clip(0, mask) {
            ClipOutcome::Verbatim { visible_mask } => assert_eq!(visible_mask, mask),
            ClipOutcome::Clipped { .. } => panic!("Host 不应被裁"),
        }
        // Normal：old_val/new_val/checksum 被裁。
        match c.clip(2, mask) {
            ClipOutcome::Clipped { visible_mask, masked_bits } => {
                assert_eq!(visible_mask, F_KIND | F_ENTITY);
                assert_eq!(masked_bits, 3);
            }
            ClipOutcome::Verbatim { .. } => panic!("Normal 应被裁"),
        }
        // Sandbox：仅 kind/seq/entity——本掩码下剩 kind/entity。
        match c.clip(3, mask) {
            ClipOutcome::Clipped { visible_mask, .. } => {
                assert_eq!(visible_mask, F_KIND | F_ENTITY);
            }
            ClipOutcome::Verbatim { .. } => panic!("Sandbox 应被裁"),
        }
        assert!(c.clipped_events >= 2);
        // 可见性矩阵抽查：Sandbox 可见 kind 不可见 new_val；Trusted 可见 new_val
        // 不可见 checksum。
        assert!(PermClipper::default().visibility(3, 0));
        assert!(!PermClipper::default().visibility(3, 8));
        assert!(PermClipper::default().visibility(1, 8));
        assert!(!PermClipper::default().visibility(1, 15));
        // 矩阵渲染 4 档 + 表头。
        let m = PermClipper::render_matrix();
        assert_eq!(m.len(), PERM_TIER_COUNT + 1);
        // 端到端：Normal 档订阅数据变更，敏感字段投递即被裁。
        bus.subscribe(0x0B01, EventClass::DataChange.bit(), 2, 0).unwrap();
        let mut vals = [0u32; EVT_VAL_SLOTS];
        vals[0] = 1;
        let out = bus
            .publish(0xE004, EventClass::DataChange, F_KIND | F_OLD_VAL | F_NEW_VAL, vals, 1)
            .unwrap();
        assert_eq!(out.delivered, 1);
        assert_eq!(out.clipped_subs, 1);
        let (clipped, _masked) = bus.clip_stats();
        assert!(clipped >= 1);
    }

    /// 冒烟 3：越权订阅拒绝——Sandbox 订系统事件被拒（码 + detail）；
    /// 类过滤位非法/空同样被拒（哨兵口径）。
    #[test]
    fn unauth_subscription_denied() {
        let mut bus = EvtBusCore::boot(0).unwrap();
        let e = bus.subscribe(0xDEAD, EventClass::System.bit(), 3, 0).unwrap_err();
        assert_eq!(e.code(), CODE_SUB_DENIED);
        assert!(!e.detail().is_empty());
        match e {
            EvtBusError::SubDenied { required_tier, got_tier, .. } => {
                assert_eq!(required_tier, 1);
                assert_eq!(got_tier, 3);
            }
            _ => panic!("应 SubDenied"),
        }
        // Normal（2）订系统事件同样越界（上限 Trusted=1）。
        assert_eq!(
            bus.subscribe(0xDEAD, EventClass::System.bit(), 2, 0).unwrap_err().code(),
            CODE_SUB_DENIED
        );
        // Trusted（1）订系统事件放行。
        assert!(bus.subscribe(0x0A01, EventClass::System.bit(), 1, 0).is_ok());
        // 非法类过滤位 / 空位 / 档位越界。
        let bad = bus.subscribe(0xDEAD, 0b1000, 0, 0).unwrap_err();
        assert_eq!(bad.code(), CODE_SUB_DENIED);
        assert_eq!(bus.subscribe(0xDEAD, 0, 0, 0).unwrap_err().code(), CODE_SUB_DENIED);
        assert_eq!(bus.subscribe(0xDEAD, 0b001, 9, 0).unwrap_err().code(), CODE_SUB_DENIED);
        // DataChange 上限 Normal(2)：Sandbox(3) 被拒。
        assert_eq!(
            bus.subscribe(0xDEAD, EventClass::DataChange.bit(), 3, 0).unwrap_err().code(),
            CODE_SUB_DENIED
        );
    }

    /// 冒烟 4：配额限流——软限降级摘要、硬限丢弃显性计数；窗口随日拍推进
    /// 回收（O(1) 判据口径）；System 类禁降级改为丢弃。
    #[test]
    fn quota_throttle_and_system_no_degrade() {
        let mut q = FlowQuota::bus_default();
        q.admit(0x11, 0).unwrap();
        let mut verdicts: [u32; 3] = [0, 0, 0];
        let mut n = 0;
        while n < 14 {
            let v = q.charge_for(0x11, 1, 0).unwrap();
            match v {
                QuotaVerdict::Pass => verdicts[0] += 1,
                QuotaVerdict::Summary => verdicts[1] += 1,
                QuotaVerdict::Drop => verdicts[2] += 1,
            }
            n += 1;
        }
        // 窗总 1..14：>12 起摘要（第 13、14 笔），硬限 20 未到。
        assert_eq!(verdicts[0], 12);
        assert_eq!(verdicts[1], 2);
        assert_eq!(verdicts[2], 0);
        assert!(q.window_of(0x11) > q.soft_limit());
        // 窗口推进：32 拍后整窗过期回卷。
        assert_eq!(q.charge_for(0x11, 1, QUOTA_WINDOW_TICKS).unwrap(), QuotaVerdict::Pass);
        assert_eq!(q.window_of(0x11), 1);
        // 未登记发布者显性拒。
        assert_eq!(
            q.charge_for(0x99, 1, 0).unwrap_err().code(),
            CODE_PUBLISHER_UNKNOWN
        );
        // 端到端：System 类超软限 → 丢弃 + degrade_refused（禁降级）。
        let mut bus = EvtBusCore::boot(0).unwrap();
        bus.subscribe(0x0A01, EventClass::System.bit(), 0, 0).unwrap();
        let mut vals = [0u32; EVT_VAL_SLOTS];
        vals[0] = 3;
        let mut summaries = 0;
        let mut drops = 0;
        let mut degrade_refusals = 0;
        let mut n2 = 0;
        while n2 < 20 {
            let out = bus.publish(0xE001, EventClass::System, F_KIND | F_SEQ, vals, 0).unwrap();
            if out.summary_mode {
                summaries += 1;
            }
            if out.dropped {
                drops += 1;
                if out.degrade_refused {
                    degrade_refusals += 1;
                }
            }
            n2 += 1;
        }
        // System 权重 1：第 13..=20 笔超软限——全部改丢弃（禁降级），硬限未触。
        assert_eq!(summaries, 0);
        assert_eq!(drops, 8);
        assert_eq!(degrade_refusals, 8);
        // 丢弃笔落账可追。
        assert_eq!(bus.dropped_total(), 8);
        let tail = bus.ledger_tail(1);
        assert!(tail[0].contains("丢弃"));
    }

    /// 冒烟 5：摘要降级——UI/数据变更类软限超后摘要掩码投影正确；
    /// 严格发布方显性退回（SummaryDegraded）。
    #[test]
    fn summary_degrade_and_strict() {
        let mut bus = EvtBusCore::boot(0).unwrap();
        bus.subscribe(0x0A01, EventClass::UserInteraction.bit(), 0, 0).unwrap();
        let mut vals = [0u32; EVT_VAL_SLOTS];
        vals[0] = 5;
        let full = F_KIND | F_KEY | F_X | F_Y | F_BUTTON | F_MODIFIER;
        let mut degraded: Option<PublishOutcome> = None;
        let mut n = 0;
        while n < 14 {
            let out = bus.publish(0xE003, EventClass::UserInteraction, full, vals, 0).unwrap();
            if out.summary_mode {
                degraded = Some(out);
            }
            n += 1;
        }
        let d = degraded.unwrap();
        assert_eq!(d.code, CODE_SUMMARY_DEGRADED);
        // 摘要账落账标记。
        let tail = bus.ledger_tail(1);
        assert!(tail[0].contains("摘要"));
        // 严格发布方：降级即退回。
        let mut bus2 = EvtBusCore::boot(0).unwrap();
        bus2.register_publisher("strict", 0xE0F1, 0).unwrap();
        bus2.subscribe(0x0A01, EventClass::DataChange.bit(), 0, 0).unwrap();
        let mut vals2 = [0u32; EVT_VAL_SLOTS];
        vals2[0] = 1;
        let mask2 = F_KIND | F_SEQ | F_ENTITY;
        // 权重 2 × 6 笔 = 窗总 12：第 7 笔超软限 → 严格口径退回。
        let mut n2 = 0;
        while n2 < 6 {
            bus2.publish_strict(0xE0F1, EventClass::DataChange, mask2, vals2, 0).unwrap();
            n2 += 1;
        }
        let e = bus2
            .publish_strict(0xE0F1, EventClass::DataChange, mask2, vals2, 0)
            .unwrap_err();
        assert_eq!(e.code(), CODE_SUMMARY_DEGRADED);
        match e {
            EvtBusError::SummaryDegraded { publisher, .. } => assert_eq!(publisher, 0xE0F1),
            _ => panic!("应 SummaryDegraded"),
        }
    }

    /// 冒烟 6：租约期——过期订阅不投并显性计数；sweep 回收；过期原地续租
    /// 被拒（防僵尸提权）；退订幂等。
    #[test]
    fn lease_expiry_and_sweep() {
        let mut bus = EvtBusCore::boot(0).unwrap();
        let sid = bus.subscribe(0x0A01, EventClass::System.bit(), 0, 0).unwrap();
        assert_eq!(sid, 1);
        // 未过期正常投递。
        let mut vals = [0u32; EVT_VAL_SLOTS];
        vals[0] = 1;
        let out = bus.publish(0xE001, EventClass::System, F_KIND, vals, 5).unwrap();
        assert_eq!(out.delivered, 1);
        // 过期（租约 30 日，day=31 越界）。
        let out2 = bus.publish(0xE001, EventClass::System, F_KIND, vals, 31).unwrap();
        assert_eq!(out2.delivered, 0);
        assert_eq!(out2.expired_seen, 1);
        // 过期原地续租被拒。
        let e = bus.renew_lease(sid, 31).unwrap_err();
        assert_eq!(e.code(), CODE_LEASE_EXPIRED);
        // health 体检 1：过期未回收 → 降级。
        let rep = bus.health();
        assert!(rep.degraded);
        assert_eq!(rep.code, CODE_HEALTH_DEGRADED);
        assert!(rep.issues.iter().any(|s| s.contains("过期租约")));
        // sweep 回收 + 体检回绿。
        assert_eq!(bus.sweep(31), 1);
        assert_eq!(bus.sub_count(), 0);
        assert!(bus.health_gate().is_ok());
        // 退订幂等：二次退订 false。
        assert!(!bus.unsubscribe(sid));
        // 未到期续租放行。
        let sid2 = bus.subscribe(0x0A02, EventClass::DataChange.bit(), 2, 40).unwrap();
        assert_eq!(bus.renew_lease(sid2, 50).unwrap(), 80);
    }

    /// 冒烟 7：桥接受控——白名单拦交互类、独立配额、连续越权熔断、
    /// 冷却半开恢复、审计账逐笔留痕、出箱队列可拉取。
    #[test]
    fn bridge_whitelist_fuse_recover() {
        let mut bus = EvtBusCore::boot(0).unwrap();
        let mut vals = [0u32; EVT_VAL_SLOTS];
        vals[0] = 2;
        // 系统事件不出域。
        let sys = Evt::compose(1, EventClass::System, 0xE001, 0, F_KIND, vals).unwrap();
        assert_eq!(
            bus.bridge_forward(&sys, 0).unwrap_err().code(),
            CODE_BRIDGE_CLASS_BLOCKED
        );
        // 交互事件不出域。
        let ui = Evt::compose(2, EventClass::UserInteraction, 0xE001, 0, F_KIND, vals).unwrap();
        assert_eq!(
            bus.bridge_forward(&ui, 0).unwrap_err().code(),
            CODE_BRIDGE_CLASS_BLOCKED
        );
        // 数据变更放行入出箱。
        let dc = Evt::compose(3, EventClass::DataChange, 0xE004, 0, F_KIND | F_SEQ | F_ENTITY, vals)
            .unwrap();
        let o = bus.bridge_forward(&dc, 0).unwrap();
        assert!(o.accepted && !o.summary_mode && o.code == 0);
        assert_eq!(bus.bridge_drain_outbox().len(), 1);
        // 连续越权 3 次 → 熔断（BridgeAbuse）。
        let mut fused_code = 0;
        let mut n = 0;
        while n < 3 {
            match bus.bridge_forward(&ui, 0) {
                Err(e) => fused_code = e.code(),
                Ok(_) => {}
            }
            n += 1;
        }
        assert_eq!(fused_code, CODE_BRIDGE_ABUSE);
        assert!(matches!(bus.bridge_state(), BridgeState::Open { .. }));
        // 冷却期内一切转发被拒（含白名单类）。
        let dc2 = Evt::compose(4, EventClass::DataChange, 0xE004, 0, F_KIND, vals).unwrap();
        assert_eq!(bus.bridge_forward(&dc2, 2).unwrap_err().code(), CODE_BRIDGE_FUSED);
        // 冷却到点 → 半开探针（白名单类）通过 → 闭合。
        let later = 0 + BRIDGE_COOLDOWN_DAYS;
        let probe =
            Evt::compose(5, EventClass::DataChange, 0xE004, later, F_KIND | F_SEQ | F_ENTITY, vals)
                .unwrap();
        let o2 = bus.bridge_forward(&probe, later).unwrap();
        assert!(o2.accepted);
        assert_eq!(bus.bridge_state(), BridgeState::Closed);
        // 审计账逐笔留痕：拦拒 + 放行都在账上。
        let audit = bus.bridge.audit_tail(16);
        assert!(audit.len() >= 6);
        assert!(audit.iter().any(|s| s.contains("拦拒")));
        assert!(audit.iter().any(|s| s.contains("放行")));
        // 桥接配额独立：连发白名单类 5 笔（权重 2）→ 窗总 10 超硬限 8 → 丢弃。
        let mut drops = 0;
        let mut m = 0;
        while m < 5 {
            let e5 = Evt::compose(100 + m, EventClass::DataChange, 0xE004, later, F_KIND, vals)
                .unwrap();
            if bus.bridge_forward(&e5, later).is_err() {
                drops += 1;
            }
            m += 1;
        }
        assert!(drops >= 1);
        assert!(bus.bridge_state_line().contains("0x5605"));
    }

    /// 冒烟 8：三路注入各检出（风暴/越权订阅/桥接滥用）+ 常规面抽检。
    #[test]
    fn three_injections_detected() {
        let mut bus = EvtBusCore::boot(7).unwrap();
        assert!(bus.inject_storm(7).unwrap());
        assert!(bus.inject_unauth_sub(7).unwrap());
        assert!(bus.inject_bridge_abuse(7).unwrap());
        let rep = bus.run_self_checks(7).unwrap();
        assert!(rep.all_pass());
        assert!(rep.storm_detected && rep.unauth_detected && rep.abuse_detected);
        assert!(rep.system_verbatim);
        assert!(rep.clipped_seen);
        assert!(rep.as_line().contains("自检"));
    }

    /// 冒烟 9：双跑确定性对拍——同日序同脚本 digest 全等；异日序必分叉；
    /// 封印对账无账漂移检出。
    #[test]
    fn dual_run_and_seal() {
        let rep = dual_run_check(21).unwrap();
        assert_eq!(rep.a_ledger, rep.b_ledger);
        // 异日序必分叉（day 折入 digest）。
        let other = dual_run_check(22).unwrap();
        assert!(rep.digest != other.digest);
        // 封印 → 无账变更对账恒过；有账变更后对账即漂移检出。
        let mut bus = EvtBusCore::boot(0).unwrap();
        let sealed = bus.seal();
        assert_eq!(bus.audit_seal().unwrap(), sealed);
        let mut vals = [0u32; EVT_VAL_SLOTS];
        vals[0] = 1;
        bus.publish(0xE001, EventClass::System, F_KIND, vals, 0).unwrap();
        let e = bus.audit_seal().unwrap_err();
        assert_eq!(e.code(), CODE_FINGERPRINT_DRIFT);
        match e {
            EvtBusError::FingerprintDrift { sealed: s, .. } => assert_eq!(s, sealed),
            _ => panic!("应 FingerprintDrift"),
        }
    }

    /// 冒烟 10：错误枚举全覆盖——十六变体码段内互异、detail 非空。
    #[test]
    fn error_table_integrity() {
        let samples: [EvtBusError; 16] = [
            EvtBusError::StormThrottled { publisher: 1, window_total: 21, hard_limit: 20 },
            EvtBusError::SummaryDegraded { publisher: 1, event_id: 2 },
            EvtBusError::SubDenied {
                subscriber: 1,
                class: EventClass::System,
                required_tier: 1,
                got_tier: 3,
            },
            EvtBusError::SubTableFull { cap: 32 },
            EvtBusError::LedgerFull { cap: 128 },
            EvtBusError::QuotaTableFull { cap: 64 },
            EvtBusError::PublisherUnknown { publisher: 9 },
            EvtBusError::LeaseExpired { sub_id: 1, until_day: 30, day: 31 },
            EvtBusError::BridgeClassBlocked { class: EventClass::System },
            EvtBusError::BridgeQuota { window_total: 9, hard_limit: 8 },
            EvtBusError::BridgeFused { cooldown_until: 5, day: 2 },
            EvtBusError::BridgeAbuse { violations: 3, threshold: 3 },
            EvtBusError::FieldMaskOob { mask: 0x10000 },
            EvtBusError::DualRunMismatch { lhs: 1, rhs: 2 },
            EvtBusError::FingerprintDrift { sealed: 1, current: 2 },
            EvtBusError::HealthDegraded { failed_checks: 2 },
        ];
        let mut i = 0;
        while i < samples.len() {
            let c = samples[i].code();
            assert!((0x1C10..=0x1C1F).contains(&c), "码 0x{:04X} 出段", c);
            assert!(!samples[i].detail().is_empty());
            let mut j = i + 1;
            while j < samples.len() {
                assert!(samples[i].code() != samples[j].code(), "码撞车：{} vs {}", i, j);
                j += 1;
            }
            i += 1;
        }
    }

    /// 冒烟 11：类语义 const 表 + 事件封套守恒——掩码越位拒绝、摘要投影、
    /// 字段取值、健康全绿基线。
    #[test]
    fn class_semantics_and_evt_invariants() {
        // 三类优先档/重投/降级/权重口径。
        assert_eq!(CLASS_PRIORITY, [0, 1, 2]);
        assert_eq!(CLASS_RETRY_ALLOW, [0, 0, 2]);
        assert_eq!(CLASS_SUMMARY_OK, [false, true, true]);
        assert_eq!(CLASS_QUOTA_WEIGHT, [1, 1, 2]);
        assert_eq!(CLASS_REQUIRED_TIER, [1, 3, 2]);
        assert_eq!(EventClass::System.name(), "System");
        assert!(EventClass::DataChange.semantics().contains("at-least-once"));
        // 封套：越位拒绝。
        let mut vals = [0u32; EVT_VAL_SLOTS];
        vals[0] = 9;
        assert_eq!(
            Evt::compose(1, EventClass::System, 0xE001, 0, 0x1_0000, vals)
                .unwrap_err()
                .code(),
            CODE_FIELD_MASK_OOB
        );
        let evt = Evt::compose(1, EventClass::UserInteraction, 0xE001, 0, F_KIND | F_X, vals)
            .unwrap();
        assert_eq!(evt.field(0), Some(9));
        assert_eq!(evt.field(9), Some(0));
        assert_eq!(evt.field(1), None);
        assert_eq!(evt.field(16), None);
        // 摘要投影：UI 摘要保 kind/key/x/y/button——x 保留、modifier 不在掩码。
        let slim = evt.summarized();
        assert_eq!(slim.field_mask, F_KIND | F_X);
        assert_eq!(slim.field_count(), 2);
        // 发布者准入：未登记发布 → 显性拒；登记后放行；注销后复拒。
        let mut bus = EvtBusCore::boot(0).unwrap();
        assert_eq!(
            bus.publish(0xE077, EventClass::System, F_KIND, vals, 0).unwrap_err().code(),
            CODE_PUBLISHER_UNKNOWN
        );
        bus.register_publisher("p0x7700", 0xE077, 0).unwrap();
        assert!(bus.publish(0xE077, EventClass::System, F_KIND, vals, 0).is_ok());
        assert!(bus.retire_publisher(0xE077));
        assert_eq!(
            bus.publish(0xE077, EventClass::System, F_KIND, vals, 0).unwrap_err().code(),
            CODE_PUBLISHER_UNKNOWN
        );
        // 掩码越位发布：计入发布者被拒账。
        assert_eq!(
            bus.publish(0xE001, EventClass::System, 0x1_0000, vals, 0).unwrap_err().code(),
            CODE_FIELD_MASK_OOB
        );
        assert!(bus.publisher_board()[0].contains("被拒1"));
        // 基线健康全绿。
        assert!(bus.health_gate().is_ok());
        assert!(bus.health().as_line().contains("全绿"));
    }

    /// 冒烟 12：账满显性拒收——投递账满后发布整件被拒（LedgerFull），
    /// 半账不产生；配额表满/发布者上限同口径。
    #[test]
    fn ledger_full_explicit_reject() {
        let mut bus = EvtBusCore::boot(0).unwrap();
        bus.subscribe(0x0A01, EventClass::System.bit(), 0, 0).unwrap();
        let mut vals = [0u32; EVT_VAL_SLOTS];
        vals[0] = 1;
        // 账满前发布均入账。
        let mut n = 0;
        while n < LEDGER_CAP {
            bus.publish(0xE001, EventClass::System, F_KIND, vals, 0).unwrap();
            n += 1;
        }
        assert_eq!(bus.ledger_len(), LEDGER_CAP);
        // 账满：显性拒收，事件计数不增（无半账）。
        let before = bus.events_total();
        let e = bus.publish(0xE001, EventClass::System, F_KIND, vals, 0).unwrap_err();
        assert_eq!(e.code(), CODE_LEDGER_FULL);
        assert_eq!(bus.events_total(), before);
        // 事件封套/裁剪/配额常量自洽抽检。
        assert_eq!(EVT_VAL_SLOTS, EVT_FIELD_COUNT);
        assert_eq!(FIELD_NAMES.len(), EVT_FIELD_COUNT);
        assert!(QUOTA_HARD_LIMIT > QUOTA_SOFT_LIMIT);
        assert!(BRIDGE_QUOTA_HARD > BRIDGE_QUOTA_SOFT);
    }
}
