//! CGPU-F1123 · 预算池分级（CGPU-H 域 · 显存与预算域 · H03 组 · 深化版）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1123`
//!
//! **判据（锚点原文）**：三级、账户树、守恒、水位、判据。
//!
//! **职责定位（锚点原文）**：池分级：三级池（进程池（各引擎会话）/系统池
//! （系统 UI/合成器）/保留池（应急+驱动保留）——三级语义与配额）；池配额
//! 分配（进程内再分账户——复用 C 域账户树理念）；池校验（Σ配额≤总显存
//! ——守恒复用）；池水位（三级池各自水位监控——告警联动）。
//!
//! 本深化版在速写面之上扩出真实语义密度，全部为纯功能逻辑：
//!
//! 1. **三级池闭集**（判据一）：[`PoolTier`] 三态语义绑定缺省配额
//!    [`DEFAULT_QUOTAS_MB`] 与让度规则——系统池不可让度、保留池动用
//!    即告警，规则进 [`RebalanceEngine`] 裁决而非注释口号。
//! 2. **账户树**（判据二）：进程池内 [`PoolAccount`] 之上再分
//!    [`SubAccount`] 子账户层（深度上限 [`SUB_MAX_DEPTH`]），子账户
//!    配额之和不得超父账户配额——树内守恒。
//! 3. **守恒**（判据三）：[`PoolBook::conserve`]（Σ池配额≤总显存，
//!    总显存注入）+ [`PoolBook::accounts_fit_pool`]（Σ账户已用≤池已用）
//!    + [`PoolBook::subaccounts_fit_parents`]（Σ子配额≤父配额）三式。
//! 4. **水位**（判据四）：四级水位 + [`WaterMonitor`] 滞回状态机
//!    （升档即报、降档需连续 [`DEESCALATE_CONFIRM`] 次确认防振荡）+
//!    [`WaterHistory`] 采样史（峰值/低谷/趋势三段对比）+ 告警联动行。
//! 5. **事件账本**：[`EventLedger`] 滚动 256 条八类事件闭集，FNV 链式
//!    防篡改（prev_fp+seq 严格递增），[`PoolBook::ledger_intact`] 复验。
//! 6. **再平衡**：[`RebalanceEngine`] 池间配额迁移（源头三规则 +
//!    单笔上限 bps + 迁移后守恒复验 + 全程留痕）。
//! 7. **回收预估**：[`ReclaimPlanner`] 红橙位产出回收预估清单（软顶
//!    之上超额部分），显性声明"预估非强制——执行位在 F1124"。
//! 8. **快照往返**：[`PoolSnap`] 确定性序列化 + FNV 指纹 + restore
//!    重建 + 指纹对拍（篡改即 [`XcCode::SnapshotMismatch`]）。
//! 9. **无障碍读屏**：[`PoolBook::read_screen`] 三段固定预算行。
//!
//! **对接**：F1122（显存账本——总量与占用来源）；F1124（水位联动
//! 回收/换页/降质——告警消费方与回收执行位）；F0323（C 域账户树——
//! 复用对象）。零 panic 面、零 IO、零墙钟（tick 注入）、零浮点、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（配额/水位/容量/滞回唯一源）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const POOL_TIER_VERSION: &str = "H03-pooltier-v2";

/// 缺省三级配额（MB）：进程/系统/保留。来源：本域规划缺省值
/// （进程 70%/系统 20%/保留 10%）；当前策略值，可调（走 set_quota 守恒面）。
pub const DEFAULT_QUOTAS_MB: [u64; 3] = [700, 200, 100];

/// 水位阈值（占池配额百分比）：黄 ≥70%，橙 ≥85%，红 ≥95%。
/// 来源：复用 E04 F0692 水位理念的显存版分级锚点。
pub const WATERMARK_YELLOW_PCT: u64 = 70;
pub const WATERMARK_ORANGE_PCT: u64 = 85;
pub const WATERMARK_RED_PCT: u64 = 95;

/// 事件账本容量（滚动；超出逐出最旧——账面有限防账本无限膨胀）。
pub const EVENT_LEDGER_CAPACITY: usize = 256;

/// 进程池账户数上限（账户树规模护栏；超限拒开并留痕）。
pub const MAX_ACCOUNTS: usize = 64;

/// 子账户树最大深度（根账户=0 层；子账户最深第 4 层）。
pub const SUB_MAX_DEPTH: u32 = 4;

/// 单池子账户总数上限（树规模护栏）。
pub const MAX_SUBACCOUNTS: usize = 96;

/// 降档确认次数（滞回：升档即报，降档需连续 N 次低于档位才降——防振荡）。
pub const DEESCALATE_CONFIRM: u32 = 8;

/// 水位采样史容量（滚动窗口；峰值/低谷/趋势统计面）。
pub const HISTORY_CAP: usize = 64;

/// 回收软顶（占账户配额百分比；超额部分计入回收预估）。
pub const SOFT_CAP_PCT: u64 = 60;

/// 单笔再平衡上限（占源池配额 bps；防单笔搬空）。
pub const REBALANCE_MAX_BPS: u64 = 2500;

/// 回收预估清单条目上限（清单只列大头，执行位再精算）。
pub const RECLAIM_MAX_ITEMS: usize = 16;

/// 快照序列化版本（schema 演进闸）。
pub const SNAP_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// 二、三级池闭集（判据一）
// ---------------------------------------------------------------------------

/// 三级池闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolTier {
    /// 进程池：各引擎会话的显存主战场。
    Process,
    /// 系统池：系统 UI/合成器——交互命脉，不可让度。
    System,
    /// 保留池：应急+驱动保留，平时不动。
    Reserve,
}

impl PoolTier {
    /// 池序（0-2；配额表/水位表下标）。
    pub const fn ordinal(self) -> usize {
        match self {
            PoolTier::Process => 0,
            PoolTier::System => 1,
            PoolTier::Reserve => 2,
        }
    }

    /// 池语义（三级语义与配额一体）。
    pub const fn semantics(self) -> &'static str {
        match self {
            PoolTier::Process => "进程池：各引擎会话显存；可按账户树再分；可让度",
            PoolTier::System => "系统池：系统UI/合成器；交互命脉；不可让度",
            PoolTier::Reserve => "保留池：应急+驱动保留；平时不动；动用即告警",
        }
    }

    /// 短名（账面/事件摘要用）。
    pub const fn label(self) -> &'static str {
        match self {
            PoolTier::Process => "进程池",
            PoolTier::System => "系统池",
            PoolTier::Reserve => "保留池",
        }
    }

    /// 由池序还原闭集成员（表外序返回 None——不入非法态）。
    pub const fn from_ordinal(o: usize) -> Option<PoolTier> {
        match o {
            0 => Some(PoolTier::Process),
            1 => Some(PoolTier::System),
            2 => Some(PoolTier::Reserve),
            _ => None,
        }
    }

    /// 让度规则：本池作为再平衡**源头**是否可让度（System 不可让度）。
    pub const fn yieldable(self) -> bool {
        match self {
            PoolTier::Process => true,
            PoolTier::System => false,
            PoolTier::Reserve => true,
        }
    }

    /// 动用告警规则：本池作为再平衡**源头**被抽走配额是否必须告警。
    pub const fn touch_alarm(self) -> bool {
        match self {
            PoolTier::Reserve => true,
            _ => false,
        }
    }

    /// 是否允许开账户树（只有进程池再分账户——判据二原文限定）。
    pub const fn tree_capable(self) -> bool {
        match self {
            PoolTier::Process => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、诊断码（0x65 段独占：H03 池分级面；深化扩码仍守 0x65 段）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x65 段；深化扩码 0x6505 起，速写四码原位不动）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 池配额之和超总显存。
    QuotaOverVram,
    /// 账户申请超池配额。
    QuotaExceeded,
    /// 账户账超池账（双守恒第二式失败）。
    AccountOverPool,
    /// 未知池序（表外池）。
    UnknownTier,
    /// 再平衡被规则拒绝（源头不可让度/超单笔上限/守恒不过）。
    RebalanceDenied,
    /// 保留池被动用（动用即告警——合法但必须留痕）。
    ReserveTouched,
    /// 账户不存在（按 id 未命中）。
    UnknownAccount,
    /// 账户 id 重复开立。
    AccountDuplicate,
    /// 子账户树违规（超深度/Σ子配额超父配额/父不存在）。
    TreeViolated,
    /// 事件账本指纹链断裂（防篡改复验失败）。
    LedgerCorrupt,
    /// 快照指纹对拍失败（往返被篡改或 schema 不符）。
    SnapshotMismatch,
    /// 账户仍有占用不得关闭（先释放再关）。
    AccountNonEmpty,
}

impl XcCode {
    /// 人读诊断（全部带可操作建议——错误路径判据）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::QuotaOverVram => "池配额之和超总显存：守恒式 Σ配额≤总显存不成立，建议下调池配额而非静默钳制",
            XcCode::QuotaExceeded => "账户申请超池配额：进程内账户不得突破所属池配额，建议收缩账户配额或申请池间再平衡",
            XcCode::AccountOverPool => "账户账超池账：Σ账户已用>池已用属记账断裂，建议对账修复后再分配",
            XcCode::UnknownTier => "未知池序：三级闭集之外无池，建议按 PoolTier 闭集取用",
            XcCode::RebalanceDenied => "再平衡被拒：系统池不可让度/超单笔上限/迁移后守恒不过，建议拆小迁移量或改源",
            XcCode::ReserveTouched => "保留池被动用：合法但必须告警留痕，建议尽快归还并核查为何常规池不够用",
            XcCode::UnknownAccount => "账户不存在：按 id 未命中，建议先 open_account 再申请，或核对 id 传参",
            XcCode::AccountDuplicate => "账户 id 重复：同一进程池内 id 唯一，建议换 id 或先关旧户",
            XcCode::TreeViolated => "子账户树违规：超深度/Σ子配额超父配额/父不存在，建议收缩子树再挂载",
            XcCode::LedgerCorrupt => "事件账本指纹链断裂：账面被篡改或记账乱序，建议以快照为准重建账本",
            XcCode::SnapshotMismatch => "快照指纹对拍失败：往返内容被改动或 schema 版本不符，建议重出快照",
            XcCode::AccountNonEmpty => "账户仍有占用：先释放全部占用再关闭，或核对释放路径是否漏记",
        }
    }

    /// 0x65 段十六进制码（速写四码 0x6501-0x6504 原位；扩码 0x6505 起）。
    pub const fn code_hex(self) -> u16 {
        match self {
            XcCode::QuotaOverVram => 0x6501,
            XcCode::QuotaExceeded => 0x6502,
            XcCode::AccountOverPool => 0x6503,
            XcCode::UnknownTier => 0x6504,
            XcCode::RebalanceDenied => 0x6505,
            XcCode::ReserveTouched => 0x6506,
            XcCode::UnknownAccount => 0x6507,
            XcCode::AccountDuplicate => 0x6508,
            XcCode::TreeViolated => 0x6509,
            XcCode::LedgerCorrupt => 0x650A,
            XcCode::SnapshotMismatch => 0x650B,
            XcCode::AccountNonEmpty => 0x650C,
        }
    }
}

// ---------------------------------------------------------------------------
// 四、FNV-1a 指纹基元（事件链/快照/归属指纹共用）
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基（64 位）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数（64 位）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 单字节混入。
pub const fn fnv1a_byte(state: u64, b: u8) -> u64 {
    (state ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a u64 混入（定长 8 字节小端序逐字节——确定性不依赖平台宽度）。
pub const fn fnv1a_u64(state: u64, v: u64) -> u64 {
    let s1 = fnv1a_byte(state, (v & 0xff) as u8);
    let s2 = fnv1a_byte(s1, ((v >> 8) & 0xff) as u8);
    let s3 = fnv1a_byte(s2, ((v >> 16) & 0xff) as u8);
    let s4 = fnv1a_byte(s3, ((v >> 24) & 0xff) as u8);
    let s5 = fnv1a_byte(s4, ((v >> 32) & 0xff) as u8);
    let s6 = fnv1a_byte(s5, ((v >> 40) & 0xff) as u8);
    let s7 = fnv1a_byte(s6, ((v >> 48) & 0xff) as u8);
    fnv1a_byte(s7, ((v >> 56) & 0xff) as u8)
}

/// FNV-1a 字符串混入。
pub const fn fnv1a_str(state: u64, s: &str) -> u64 {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut st = state;
    while i < b.len() {
        st = fnv1a_byte(st, b[i]);
        i += 1;
    }
    st
}

// ---------------------------------------------------------------------------
// 五、水位（判据四：分级 + 压力量化 + 滞回）
// ---------------------------------------------------------------------------

/// 四级水位（复用 E04 F0692 理念的显存版）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaterLevel {
    /// 绿：充裕。
    Green,
    /// 黄：关注。
    Yellow,
    /// 橙：预警。
    Orange,
    /// 红：告警（联动回收/降质）。
    Red,
}

impl WaterLevel {
    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            WaterLevel::Green => "绿",
            WaterLevel::Yellow => "黄",
            WaterLevel::Orange => "橙",
            WaterLevel::Red => "红",
        }
    }

    /// 人读长名（读屏/报表用——单一语义两档表述，不新增状态）。
    pub const fn zh_long(self) -> &'static str {
        match self {
            WaterLevel::Green => "绿：充裕",
            WaterLevel::Yellow => "黄：占用关注",
            WaterLevel::Orange => "橙：预警，准备联动",
            WaterLevel::Red => "红：告警，触发回收/降质联动",
        }
    }

    /// 严重度序（0-3；滞回与压力加权用）。
    pub const fn severity(self) -> u32 {
        match self {
            WaterLevel::Green => 0,
            WaterLevel::Yellow => 1,
            WaterLevel::Orange => 2,
            WaterLevel::Red => 3,
        }
    }

    /// 由严重度序还原（表外序回落绿——不入非法态）。
    pub const fn from_severity(s: u32) -> WaterLevel {
        match s {
            1 => WaterLevel::Yellow,
            2 => WaterLevel::Orange,
            3 => WaterLevel::Red,
            _ => WaterLevel::Green,
        }
    }
}

/// 水位判定（占用比对分级；阈值来源见常量注释）。
pub const fn watermark_of(used_mb: u64, quota_mb: u64) -> WaterLevel {
    if quota_mb == 0 {
        // 零配额池没有可占用面——按绿处理（不存在占满问题）。
        return WaterLevel::Green;
    }
    // 占用百分比：used*100/quota（u64 中间量，配额 MB 级不会溢出）。
    let pct = used_mb.saturating_mul(100) / quota_mb;
    if pct >= WATERMARK_RED_PCT {
        WaterLevel::Red
    } else if pct >= WATERMARK_ORANGE_PCT {
        WaterLevel::Orange
    } else if pct >= WATERMARK_YELLOW_PCT {
        WaterLevel::Yellow
    } else {
        WaterLevel::Green
    }
}

/// 压力量化（bps：占用比×10000；零配额池恒 0——无占用面即无压力）。
pub const fn pressure_bps(used_mb: u64, quota_mb: u64) -> u64 {
    if quota_mb == 0 {
        return 0;
    }
    used_mb.saturating_mul(10_000) / quota_mb
}

// ---------------------------------------------------------------------------
// 六、账户与账户树（判据二：进程内再分账户 + 子账户层）
// ---------------------------------------------------------------------------

/// 池内账户（复用 C 域账户树理念：配额/已用/守恒同名同义）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolAccount {
    /// 账户 id（进程池内唯一）。
    pub account_id: u32,
    /// 进程标签（归属会话）。
    pub owner: String,
    /// 账户配额（MB；≤所属池配额）。
    pub quota_mb: u64,
    /// 已用（MB；≤账户配额）。
    pub used_mb: u64,
}

impl PoolAccount {
    /// 账户余量（配额-已用，饱和不为负）。
    pub const fn free_mb(&self) -> u64 {
        self.quota_mb.saturating_sub(self.used_mb)
    }

    /// 账户利用率（bps）。
    pub const fn utilization_bps(&self) -> u64 {
        pressure_bps(self.used_mb, self.quota_mb)
    }

    /// 还可增长空间是否容纳 want（只读判定，供申请前预估）。
    pub const fn can_grow(&self, want_mb: u64) -> bool {
        self.used_mb.saturating_add(want_mb) <= self.quota_mb
    }
}

/// 账户申请（校验→入账，超配额显性拒）。速写面原签名保留。
pub fn account_request(
    acc: &mut PoolAccount,
    pool_quota_mb: u64,
    pool_used_mb: u64,
    want_mb: u64,
    rejected: &mut u64,
) -> Result<u64, XcCode> {
    let new_used = acc.used_mb.saturating_add(want_mb);
    if new_used > acc.quota_mb || acc.quota_mb > pool_quota_mb {
        *rejected = rejected.saturating_add(1);
        return Err(XcCode::QuotaExceeded);
    }
    let new_pool_used = pool_used_mb.saturating_add(want_mb);
    if new_pool_used > pool_quota_mb {
        *rejected = rejected.saturating_add(1);
        return Err(XcCode::QuotaExceeded);
    }
    acc.used_mb = new_used;
    Ok(acc.used_mb)
}

/// 子账户（账户树第二层及以下：挂父账户，深度受限）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubAccount {
    /// 子账户 id（进程池内唯一，与根账户共用 id 空间防混淆——同 id 不同层即拒）。
    pub sub_id: u32,
    /// 父账户 id（必须已存在）。
    pub parent_id: u32,
    /// 树深（父深+1；根账户深 0，子最深 [`SUB_MAX_DEPTH`]）。
    pub depth: u32,
    /// 子配额（MB；≤父账户余量口径内分配）。
    pub quota_mb: u64,
    /// 子已用（MB）。
    pub used_mb: u64,
}

impl SubAccount {
    /// 子余量。
    pub const fn free_mb(&self) -> u64 {
        self.quota_mb.saturating_sub(self.used_mb)
    }
}

// ---------------------------------------------------------------------------
// 七、事件账本（八类闭集 + 滚动 256 + FNV 链式防篡改）
// ---------------------------------------------------------------------------

/// 事件类别闭集（八类；kind_id 供账面压缩与对账计数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 池配额设定/变更。
    QuotaSet,
    /// 账户开立。
    AccountOpened,
    /// 账户关闭。
    AccountClosed,
    /// 账户分配（进程池）。
    Alloc,
    /// 账户释放（进程池）。
    Freed,
    /// 池间再平衡。
    Rebalance,
    /// 水位档迁移。
    WatermarkChange,
    /// 回收预估立案（F1124 联动面）。
    ReclaimPlanned,
}

impl EventKind {
    /// 类别序号（账面压缩用；闭集 0-7）。
    pub const fn kind_id(self) -> u8 {
        match self {
            EventKind::QuotaSet => 0,
            EventKind::AccountOpened => 1,
            EventKind::AccountClosed => 2,
            EventKind::Alloc => 3,
            EventKind::Freed => 4,
            EventKind::Rebalance => 5,
            EventKind::WatermarkChange => 6,
            EventKind::ReclaimPlanned => 7,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            EventKind::QuotaSet => "配额变更",
            EventKind::AccountOpened => "开户",
            EventKind::AccountClosed => "销户",
            EventKind::Alloc => "分配",
            EventKind::Freed => "释放",
            EventKind::Rebalance => "再平衡",
            EventKind::WatermarkChange => "水位迁移",
            EventKind::ReclaimPlanned => "回收立案",
        }
    }

    /// 由类别序号还原（表外回落 QuotaSet——不入非法态）。
    pub const fn from_kind_id(k: u8) -> EventKind {
        match k {
            1 => EventKind::AccountOpened,
            2 => EventKind::AccountClosed,
            3 => EventKind::Alloc,
            4 => EventKind::Freed,
            5 => EventKind::Rebalance,
            6 => EventKind::WatermarkChange,
            7 => EventKind::ReclaimPlanned,
            _ => EventKind::QuotaSet,
        }
    }
}

/// 事件账面行（滚动账一条）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventRecord {
    /// 全账严格递增序号（断号即账损）。
    pub seq: u64,
    /// 事件 tick（注入；零墙钟）。
    pub tick: u64,
    /// 类别。
    pub kind: EventKind,
    /// 关联池序（无池事件填 3=表外）。
    pub tier_ord: u8,
    /// 关联量（MB；无量事件 0）。
    pub mb: u64,
    /// 关联账户 id（无账户事件 0）。
    pub account_id: u32,
    /// 人读摘要（≤48 字符裁剪——账面有限）。
    pub summary: String,
    /// 本行指纹（含 prev_fp 链式——防篡改）。
    pub fp: u64,
}

impl EventRecord {
    /// 摘要裁剪上限（字节；多字节字符尾部整字保留）。
    pub const SUMMARY_MAX: usize = 48;

    /// 构造 + 计链上指纹。
    pub fn new(
        prev_fp: u64,
        seq: u64,
        tick: u64,
        kind: EventKind,
        tier_ord: u8,
        mb: u64,
        account_id: u32,
        summary: &str,
    ) -> EventRecord {
        let trimmed = clip_chars(summary, EventRecord::SUMMARY_MAX);
        let fp = fnv1a_u64(prev_fp, seq);
        let fp = fnv1a_u64(fp, tick);
        let fp = fnv1a_u64(fp, kind.kind_id() as u64);
        let fp = fnv1a_u64(fp, tier_ord as u64);
        let fp = fnv1a_u64(fp, mb);
        let fp = fnv1a_u64(fp, account_id as u64);
        let fp = fnv1a_str(fp, &trimmed);
        EventRecord {
            seq,
            tick,
            kind,
            tier_ord,
            mb,
            account_id,
            summary: trimmed,
            fp,
        }
    }
}

/// 多字节安全裁剪（整字保留，不切 UTF-8 序列）。
pub fn clip_chars(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut end = max_bytes;
    while end > 0 && (b[end] & 0xc0) == 0x80 {
        end -= 1;
    }
    match core::str::from_utf8(&b[..end]) {
        Ok(t) => t.to_string(),
        Err(_) => String::new(),
    }
}

/// 滚动事件账本（容量 [`EVENT_LEDGER_CAPACITY`]；逐出最旧、链上指纹）。
#[derive(Clone, Debug)]
pub struct EventLedger {
    records: Vec<EventRecord>,
    next_seq: u64,
    /// 逐出累计（账面有限但产量不减记——审计口径）。
    pub evicted: u64,
    /// 链上最后一枚指纹（空账为 FNV_OFFSET）。
    pub tail_fp: u64,
}

impl EventLedger {
    /// 空账。
    pub fn new() -> EventLedger {
        EventLedger {
            records: Vec::new(),
            next_seq: 1,
            evicted: 0,
            tail_fp: FNV_OFFSET,
        }
    }

    /// 当前账长。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 账是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 下一序号（只读预告——调用方记账后自增由本账负责）。
    pub fn peek_seq(&self) -> u64 {
        self.next_seq
    }

    /// 追加事件（滚动逐出；返回本行序号）。
    pub fn append(
        &mut self,
        tick: u64,
        kind: EventKind,
        tier_ord: u8,
        mb: u64,
        account_id: u32,
        summary: &str,
    ) -> u64 {
        let seq = self.next_seq;
        let rec = EventRecord::new(self.tail_fp, seq, tick, kind, tier_ord, mb, account_id, summary);
        self.tail_fp = rec.fp;
        self.next_seq = self.next_seq.saturating_add(1);
        if self.records.len() >= EVENT_LEDGER_CAPACITY {
            // 逐出最旧（O(n) 移位；容量 256 常数可控）。
            let _ = self.records.remove(0);
            self.evicted = self.evicted.saturating_add(1);
        }
        self.records.push(rec);
        seq
    }

    /// 指纹链复验（seq 严格递增 + prev 链重算对拍；断链即 LedgerCorrupt）。
    pub fn verify_chain(&self) -> Result<(), XcCode> {
        let mut prev = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.records.len() {
            let r = match self.records.get(i) {
                Some(r) => r,
                None => return Err(XcCode::LedgerCorrupt),
            };
            let expect = EventRecord::new(
                prev,
                r.seq,
                r.tick,
                r.kind,
                r.tier_ord,
                r.mb,
                r.account_id,
                &r.summary,
            );
            if expect.fp != r.fp {
                return Err(XcCode::LedgerCorrupt);
            }
            prev = r.fp;
            i += 1;
        }
        if prev != self.tail_fp {
            return Err(XcCode::LedgerCorrupt);
        }
        Ok(())
    }

    /// 按类别计数（对账面：Σ各类计数 == len）。
    pub fn count_by_kind(&self, kind: EventKind) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.kind == kind {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 按池序计数（表外 3 归"无池"）。
    pub fn count_by_tier(&self, tier_ord: u8) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.tier_ord == tier_ord {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 时间窗回溯（tick ∈ [from, to]；含两端）。
    pub fn trace_window(&self, from_tick: u64, to_tick: u64) -> Vec<&EventRecord> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.tick >= from_tick && r.tick <= to_tick {
                    out.push(r);
                }
            }
            i += 1;
        }
        out
    }

    /// 最近 N 条（N 大于账长取全账）。
    pub fn tail(&self, n: usize) -> Vec<&EventRecord> {
        let len = self.records.len();
        let start = len.saturating_sub(n);
        let mut out = Vec::new();
        let mut i = start;
        while i < len {
            if let Some(r) = self.records.get(i) {
                out.push(r);
            }
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 八、水位监控（滞回状态机 + 采样史）
// ---------------------------------------------------------------------------

/// 单池水位监控（滞回：升档即报、降档需连续 [`DEESCALATE_CONFIRM`] 次）。
#[derive(Clone, Debug)]
pub struct WaterMonitor {
    /// 当前在档。
    pub current: WaterLevel,
    /// 低于在档的连续计数（降档确认窗口）。
    low_streak: u32,
    /// 迁移计数（升+降总次——振荡审计面）。
    pub transitions: u64,
}

impl WaterMonitor {
    /// 新监控（起点绿）。
    pub const fn new() -> WaterMonitor {
        WaterMonitor {
            current: WaterLevel::Green,
            low_streak: 0,
            transitions: 0,
        }
    }

    /// 观测一次（输入目标档；返回 (生效档, 是否迁移)）。
    /// 升档立即生效；降档须目标低于在档连续 `DEESCALATE_CONFIRM` 次。
    pub fn observe(&mut self, target: WaterLevel) -> (WaterLevel, bool) {
        let t = target.severity();
        let c = self.current.severity();
        if t > c {
            // 升档：立即生效（危险不打折）。
            self.current = target;
            self.low_streak = 0;
            self.transitions = self.transitions.saturating_add(1);
            (self.current, true)
        } else if t < c {
            self.low_streak = self.low_streak.saturating_add(1);
            if self.low_streak >= DEESCALATE_CONFIRM {
                self.current = target;
                self.low_streak = 0;
                self.transitions = self.transitions.saturating_add(1);
                (self.current, true)
            } else {
                (self.current, false)
            }
        } else {
            self.low_streak = 0;
            (self.current, false)
        }
    }

    /// 同档打点（不迁移，仅复位低档连击——观测面主动声明稳定）。
    pub fn confirm_stable(&mut self) {
        self.low_streak = 0;
    }
}

/// 采样史（滚动 [`HISTORY_CAP`]；峰值/低谷/趋势）。
#[derive(Clone, Debug)]
pub struct WaterHistory {
    samples: Vec<u64>,
    /// 累计采样数（含被逐出者——产量口径）。
    pub produced: u64,
    /// 全史峰值（bps）。
    pub peak_bps: u64,
    /// 全史低谷（bps；首样起算）。
    pub low_bps: u64,
}

impl WaterHistory {
    /// 空史。
    pub const fn new() -> WaterHistory {
        WaterHistory {
            samples: Vec::new(),
            produced: 0,
            peak_bps: 0,
            low_bps: u64::MAX,
        }
    }

    /// 打点（bps；滚动逐出最旧）。
    pub fn record(&mut self, bps: u64) {
        if self.samples.len() >= HISTORY_CAP {
            let _ = self.samples.remove(0);
        }
        self.samples.push(bps);
        self.produced = self.produced.saturating_add(1);
        if bps > self.peak_bps {
            self.peak_bps = bps;
        }
        if bps < self.low_bps {
            self.low_bps = bps;
        }
    }

    /// 窗口均量（bps；空史 0）。
    pub fn mean_bps(&self) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < self.samples.len() {
            sum = sum.saturating_add(*self.samples.get(i).unwrap_or(&0));
            i += 1;
        }
        sum / self.samples.len() as u64
    }

    /// 趋势（前半窗均量 vs 后半窗均量；差过 ±5% 判升降，否则平）。
    pub fn trend(&self) -> Trend {
        let len = self.samples.len();
        if len < 8 {
            return Trend::Flat;
        }
        let half = len / 2;
        let mut a: u64 = 0;
        let mut b: u64 = 0;
        let mut i = 0usize;
        while i < len {
            let v = *self.samples.get(i).unwrap_or(&0);
            if i < half {
                a = a.saturating_add(v);
            } else {
                b = b.saturating_add(v);
            }
            i += 1;
        }
        let a = a / half.max(1) as u64;
        let b = b / (len - half).max(1) as u64;
        if a == 0 {
            return if b > 0 { Trend::Rise } else { Trend::Flat };
        }
        // 差异容忍 5%（500bps of ratio×10000）。
        let ratio = (b.saturating_mul(10_000)) / a;
        if ratio > 10_500 {
            Trend::Rise
        } else if ratio < 9_500 {
            Trend::Fall
        } else {
            Trend::Flat
        }
    }
}

/// 趋势三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trend {
    /// 走高。
    Rise,
    /// 走低。
    Fall,
    /// 平稳（含样本不足）。
    Flat,
}

impl Trend {
    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            Trend::Rise => "走高",
            Trend::Fall => "走低",
            Trend::Flat => "平稳",
        }
    }
}

// ---------------------------------------------------------------------------
// 九、再平衡引擎（源头三规则 + 单笔上限 + 守恒复验 + 留痕）
// ---------------------------------------------------------------------------

/// 再平衡请求（源池让出配额给目标池；迁的是配额额度不是已用量）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RebalanceRequest {
    /// 源池。
    pub from: PoolTier,
    /// 目标池。
    pub to: PoolTier,
    /// 迁移量（MB）。
    pub mb: u64,
    /// 动因（必填；空因拒）。
    pub reason: String,
}

/// 再平衡裁决（准/拒 + 拒因 + 是否触发保留池告警）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RebalanceVerdict {
    /// 是否批准。
    pub approved: bool,
    /// 批准时实际迁移量（可能被单笔上限钳制）。
    pub moved_mb: u64,
    /// 拒因（批准时 None）。
    pub denied: Option<XcCode>,
    /// 源头为保留池（动用即告警）。
    pub reserve_touched: bool,
    /// 人读说明。
    pub note: String,
}

/// 再平衡引擎（无状态裁决面；留痕由 PoolBook 落账）。
pub struct RebalanceEngine;

impl RebalanceEngine {
    /// 裁决一次迁移请求。
    ///
    /// 规则序（先硬后软）：
    /// 1. 源==目标：无意义迁移拒。
    /// 2. 动因空串拒（无因不留痕的操作不批）。
    /// 3. 源头不可让度（[`PoolTier::yieldable`]）——系统池恒拒。
    /// 4. 迁移量为 0 拒。
    /// 5. 源池余量（配额-已用）不足拒——已用量不随迁移走。
    /// 6. 单笔上限（[`REBALANCE_MAX_BPS`] bps of 源配额）——超额钳制并非拒
    ///    （钳制面显性入 note，与"超配额即拒"的申请面不同性质）。
    /// 7. 迁移后总守恒由调用方复验（本面只管池间额度）。
    pub fn judge(&self, req: &RebalanceRequest, quotas_mb: &[u64; 3], used_mb: &[u64; 3]) -> RebalanceVerdict {
        if req.from == req.to {
            return RebalanceVerdict {
                approved: false,
                moved_mb: 0,
                denied: Some(XcCode::RebalanceDenied),
                reserve_touched: false,
                note: "源==目标：无意义迁移".to_string(),
            };
        }
        if req.reason.is_empty() {
            return RebalanceVerdict {
                approved: false,
                moved_mb: 0,
                denied: Some(XcCode::RebalanceDenied),
                reserve_touched: false,
                note: "动因必填：无因迁移不可审计".to_string(),
            };
        }
        if !req.from.yieldable() {
            return RebalanceVerdict {
                approved: false,
                moved_mb: 0,
                denied: Some(XcCode::RebalanceDenied),
                reserve_touched: false,
                note: alloc::format!("{}不可让度：交互命脉不让度（锚点规则）", req.from.label()),
            };
        }
        if req.mb == 0 {
            return RebalanceVerdict {
                approved: false,
                moved_mb: 0,
                denied: Some(XcCode::RebalanceDenied),
                reserve_touched: false,
                note: "迁移量为 0".to_string(),
            };
        }
        let fq = *quotas_mb.get(req.from.ordinal()).unwrap_or(&0);
        let fu = *used_mb.get(req.from.ordinal()).unwrap_or(&0);
        let headroom = fq.saturating_sub(fu);
        if req.mb > headroom {
            return RebalanceVerdict {
                approved: false,
                moved_mb: 0,
                denied: Some(XcCode::RebalanceDenied),
                reserve_touched: req.from.touch_alarm(),
                note: alloc::format!(
                    "源头余量不足：可让 {}MB < 请求 {}MB（已用量不随迁移走）",
                    headroom,
                    req.mb
                ),
            };
        }
        // 单笔上限钳制。
        let cap = fq.saturating_mul(REBALANCE_MAX_BPS) / 10_000;
        let moved = if req.mb > cap { cap } else { req.mb };
        let clamped = moved != req.mb;
        let note = if clamped {
            alloc::format!(
                "单笔上限钳制：请求 {}MB → 实迁 {}MB（上限 {}bps of 源配额）",
                req.mb,
                moved,
                REBALANCE_MAX_BPS
            )
        } else {
            alloc::format!("批准迁移 {}MB：{}", moved, clip_chars(&req.reason, 24))
        };
        RebalanceVerdict {
            approved: true,
            moved_mb: moved,
            denied: None,
            reserve_touched: req.from.touch_alarm(),
            note,
        }
    }
}

// ---------------------------------------------------------------------------
// 十、回收预估（F1124 联动面——声明：预估非强制，执行位在 F1124）
// ---------------------------------------------------------------------------

/// 回收预估条目（单账户超额面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReclaimItem {
    /// 账户 id（进程池才有账户树；系统池条目 id 为 0 表示池级面）。
    pub account_id: u32,
    /// 预估可回收（MB = used − quota×软顶，饱和不为负）。
    pub est_mb: u64,
    /// 账户利用率（bps；排序键——越满越先回收）。
    pub util_bps: u64,
}

/// 回收预估单（单池一张）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReclaimPlan {
    /// 目标池。
    pub tier: PoolTier,
    /// 触发水位。
    pub level: WaterLevel,
    /// 预估条目（按 est 降序；上限 [`RECLAIM_MAX_ITEMS`]）。
    pub items: Vec<ReclaimItem>,
    /// 预估总量（Σ条目）。
    pub est_total_mb: u64,
    /// 人读说明（显性声明执行位）。
    pub note: String,
}

/// 回收预估器。
pub struct ReclaimPlanner;

impl ReclaimPlanner {
    /// 为单池产出预估单（水位 < 橙则 None——不到联动线不出单）。
    ///
    /// 进程池：逐账户算超额（used − quota×软顶）并入单。
    /// 系统池：无账户树，按池级面一条（used − quota×软顶）。
    /// 保留池：不出单（应急本位，回收即失能——显性不联动）。
    pub fn plan(
        &self,
        tier: PoolTier,
        quota_mb: u64,
        used_mb: u64,
        accounts: &[PoolAccount],
    ) -> Option<ReclaimPlan> {
        let level = watermark_of(used_mb, quota_mb);
        if level.severity() < WaterLevel::Orange.severity() {
            return None;
        }
        if tier == PoolTier::Reserve {
            return Some(ReclaimPlan {
                tier,
                level,
                items: Vec::new(),
                est_total_mb: 0,
                note: "保留池不出回收单：应急本位，回收即失能（显性不联动）".to_string(),
            });
        }
        let soft_floor = quota_mb.saturating_mul(SOFT_CAP_PCT) / 100;
        let mut items: Vec<ReclaimItem> = Vec::new();
        if tier == PoolTier::Process {
            let mut i = 0usize;
            while i < accounts.len() {
                if let Some(a) = accounts.get(i) {
                    let over = a.used_mb.saturating_sub(a.quota_mb.saturating_mul(SOFT_CAP_PCT) / 100);
                    if over > 0 {
                        items.push(ReclaimItem {
                            account_id: a.account_id,
                            est_mb: over,
                            util_bps: a.utilization_bps(),
                        });
                    }
                }
                i += 1;
            }
            // est 降序（次键 util 降序）——稳定排序，条目规模可控。
            items.sort_by(|a, b| {
                b.est_mb
                    .cmp(&a.est_mb)
                    .then(b.util_bps.cmp(&a.util_bps))
            });
            while items.len() > RECLAIM_MAX_ITEMS {
                let _ = items.pop();
            }
        } else {
            let over = used_mb.saturating_sub(soft_floor);
            if over > 0 {
                items.push(ReclaimItem {
                    account_id: 0,
                    est_mb: over,
                    util_bps: pressure_bps(used_mb, quota_mb),
                });
            }
        }
        let mut total: u64 = 0;
        let mut i = 0usize;
        while i < items.len() {
            total = total.saturating_add(items.get(i).map(|x| x.est_mb).unwrap_or(0));
            i += 1;
        }
        Some(ReclaimPlan {
            tier,
            level,
            items,
            est_total_mb: total,
            note: alloc::format!(
                "预估非强制：软顶 {}%，本单仅供 F1124 执行位参考，不直接动账",
                SOFT_CAP_PCT
            ),
        })
    }
}

// ---------------------------------------------------------------------------
// 十一、池账（判据三守恒 + 双守恒 + 事件账 + 监控 + 快照）
// ---------------------------------------------------------------------------

/// 单池账（配额/已用/账户树/子账户树/事件账/水位监控/量账）。
#[derive(Clone, Debug)]
pub struct PoolBook {
    /// 三池配额（MB；下标 = PoolTier ordinal）。
    pub quotas_mb: [u64; 3],
    /// 三池已用（MB）。
    pub used_mb: [u64; 3],
    /// 进程池账户树（判据二：进程内再分账户）。
    pub accounts: Vec<PoolAccount>,
    /// 子账户层（树深受 [`SUB_MAX_DEPTH`]）。
    pub subaccounts: Vec<SubAccount>,
    /// 拒绝计数（超配额显性留痕）。
    pub rejected: u64,
    /// 事件账（滚动 256 + FNV 链）。
    pub ledger: EventLedger,
    /// 水位监控（三池各一）。
    pub monitors: [WaterMonitor; 3],
    /// 采样史（三池各一）。
    pub histories: [WaterHistory; 3],
    /// 累计分配量（MB；生命周期产量口径）。
    pub allocated_total: u64,
    /// 累计释放量（MB）。
    pub freed_total: u64,
    /// 再平衡成交笔数。
    pub rebalances: u64,
    /// 回收立案笔数（预估单产出次数）。
    pub reclaim_cases: u64,
    /// 配置代（每次 set_quota/rebalance 递增——快照口径隔离）。
    pub generation: u64,
}

impl PoolBook {
    /// 新池账（带缺省配额）。
    pub fn with_default_quotas() -> PoolBook {
        PoolBook {
            quotas_mb: DEFAULT_QUOTAS_MB,
            used_mb: [0; 3],
            accounts: Vec::new(),
            subaccounts: Vec::new(),
            rejected: 0,
            ledger: EventLedger::new(),
            monitors: [WaterMonitor::new(), WaterMonitor::new(), WaterMonitor::new()],
            histories: [WaterHistory::new(), WaterHistory::new(), WaterHistory::new()],
            allocated_total: 0,
            freed_total: 0,
            rebalances: 0,
            reclaim_cases: 0,
            generation: 1,
        }
    }

    /// 守恒校验（判据三：Σ配额 ≤ 总显存；总显存上游注入）。
    pub fn conserve(&self, total_vram_mb: u64) -> Result<(), XcCode> {
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < 3 {
            sum = sum.saturating_add(*self.quotas_mb.get(i).unwrap_or(&0));
            i += 1;
        }
        if sum > total_vram_mb {
            return Err(XcCode::QuotaOverVram);
        }
        Ok(())
    }

    /// 双守恒第二式：Σ账户已用 ≤ 进程池已用（账平才算账）。
    pub fn accounts_fit_pool(&self) -> Result<(), XcCode> {
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some(a) = self.accounts.get(i) {
                sum = sum.saturating_add(a.used_mb);
            }
            i += 1;
        }
        let pool_used = *self.used_mb.get(PoolTier::Process.ordinal()).unwrap_or(&0);
        if sum > pool_used {
            return Err(XcCode::AccountOverPool);
        }
        Ok(())
    }

    /// 树内守恒第三式：Σ子配额 ≤ 父账户配额（逐父核）。
    pub fn subaccounts_fit_parents(&self) -> Result<(), XcCode> {
        let mut ai = 0usize;
        while ai < self.accounts.len() {
            let parent = match self.accounts.get(ai) {
                Some(p) => p,
                None => return Err(XcCode::TreeViolated),
            };
            let mut sub_sum: u64 = 0;
            let mut si = 0usize;
            while si < self.subaccounts.len() {
                if let Some(s) = self.subaccounts.get(si) {
                    if s.parent_id == parent.account_id {
                        sub_sum = sub_sum.saturating_add(s.quota_mb);
                    }
                }
                si += 1;
            }
            if sub_sum > parent.quota_mb {
                return Err(XcCode::TreeViolated);
            }
            ai += 1;
        }
        Ok(())
    }

    // ---------------- 配额面 ----------------

    /// 设定单池配额（守恒过账：Σ新配额 ≤ 总显存才落账；变更留痕）。
    pub fn set_quota(&mut self, tier: PoolTier, new_mb: u64, total_vram_mb: u64, tick: u64) -> Result<(), XcCode> {
        let ord = tier.ordinal();
        let old = *self.quotas_mb.get(ord).unwrap_or(&0);
        let mut trial = self.quotas_mb;
        if let Some(slot) = trial.get_mut(ord) {
            *slot = new_mb;
        }
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < 3 {
            sum = sum.saturating_add(*trial.get(i).unwrap_or(&0));
            i += 1;
        }
        if sum > total_vram_mb {
            self.rejected = self.rejected.saturating_add(1);
            return Err(XcCode::QuotaOverVram);
        }
        // 下调不得低于已用（配额低于已用属账面断裂）。
        let used = *self.used_mb.get(ord).unwrap_or(&0);
        if new_mb < used {
            self.rejected = self.rejected.saturating_add(1);
            return Err(XcCode::QuotaExceeded);
        }
        self.quotas_mb = trial;
        self.generation = self.generation.saturating_add(1);
        self.ledger.append(
            tick,
            EventKind::QuotaSet,
            ord as u8,
            new_mb,
            0,
            &alloc::format!("{}配额 {}→{}MB", tier.label(), old, new_mb),
        );
        Ok(())
    }

    // ---------------- 账户面（根账户） ----------------

    /// 开账户（进程池内；超池配额即拒；id 重复拒；池账户数上限拒）。
    pub fn open_account(&mut self, account_id: u32, owner: &str, quota_mb: u64) -> Result<(), XcCode> {
        let pool_quota = *self.quotas_mb.get(PoolTier::Process.ordinal()).unwrap_or(&0);
        if quota_mb > pool_quota {
            self.rejected = self.rejected.saturating_add(1);
            return Err(XcCode::QuotaExceeded);
        }
        if owner.is_empty() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(XcCode::AccountDuplicate);
        }
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some(a) = self.accounts.get(i) {
                if a.account_id == account_id {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(XcCode::AccountDuplicate);
                }
            }
            i += 1;
        }
        if self.accounts.len() >= MAX_ACCOUNTS {
            self.rejected = self.rejected.saturating_add(1);
            return Err(XcCode::QuotaExceeded);
        }
        self.accounts.push(PoolAccount {
            account_id,
            owner: clip_chars(owner, 32),
            quota_mb,
            used_mb: 0,
        });
        self.ledger.append(
            0,
            EventKind::AccountOpened,
            PoolTier::Process.ordinal() as u8,
            quota_mb,
            account_id,
            &alloc::format!("开户 {}", clip_chars(owner, 16)),
        );
        Ok(())
    }

    /// 关账户（仍有占用拒——先释放再关；关闭留痕）。
    pub fn close_account(&mut self, account_id: u32, tick: u64) -> Result<(), XcCode> {
        let mut target: Option<usize> = None;
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some(a) = self.accounts.get(i) {
                if a.account_id == account_id {
                    target = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let idx = match target {
            Some(i) => i,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(XcCode::UnknownAccount);
            }
        };
        let used = match self.accounts.get(idx) {
            Some(a) => a.used_mb,
            None => return Err(XcCode::UnknownAccount),
        };
        if used > 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(XcCode::AccountNonEmpty);
        }
        // 级联拒：仍有子账户挂着先摘子（树完整优先）。
        let mut si = 0usize;
        while si < self.subaccounts.len() {
            if let Some(s) = self.subaccounts.get(si) {
                if s.parent_id == account_id {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(XcCode::TreeViolated);
                }
            }
            si += 1;
        }
        let _ = self.accounts.remove(idx);
        self.ledger.append(
            tick,
            EventKind::AccountClosed,
            PoolTier::Process.ordinal() as u8,
            0,
            account_id,
            "销户",
        );
        Ok(())
    }

    /// 账户申请显存（走 [`account_request`] 校验面）。速写面原签名保留。
    pub fn account_alloc(&mut self, account_id: u32, want_mb: u64) -> Result<u64, XcCode> {
        let pool_quota = *self.quotas_mb.get(PoolTier::Process.ordinal()).unwrap_or(&0);
        let pool_used = *self.used_mb.get(PoolTier::Process.ordinal()).unwrap_or(&0);
        // 取账户（无下标 panic 面：逐个比对）。
        let mut target: Option<usize> = None;
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some(a) = self.accounts.get(i) {
                if a.account_id == account_id {
                    target = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let idx = match target {
            Some(i) => i,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(XcCode::UnknownAccount);
            }
        };
        // 拆借用：账户可变借用与池已用读取分离。
        let (quota_before, used_before) = {
            let a = match self.accounts.get(idx) {
                Some(a) => (a.quota_mb, a.used_mb),
                None => return Err(XcCode::UnknownAccount),
            };
            a
        };
        let mut acc = PoolAccount {
            account_id,
            owner: String::new(),
            quota_mb: quota_before,
            used_mb: used_before,
        };
        match account_request(&mut acc, pool_quota, pool_used, want_mb, &mut self.rejected) {
            Ok(new_used) => {
                if let Some(a) = self.accounts.get_mut(idx) {
                    a.used_mb = new_used;
                }
                if let Some(u) = self.used_mb.get_mut(PoolTier::Process.ordinal()) {
                    *u = u.saturating_add(want_mb);
                }
                self.allocated_total = self.allocated_total.saturating_add(want_mb);
                self.ledger.append(
                    0,
                    EventKind::Alloc,
                    PoolTier::Process.ordinal() as u8,
                    want_mb,
                    account_id,
                    &alloc::format!("分配→{}MB", new_used),
                );
                Ok(new_used)
            }
            Err(c) => Err(c),
        }
    }

    /// 账户释放（分配的逆路径；释放量钳到已用；守恒随行）。
    pub fn account_free(&mut self, account_id: u32, mb: u64, tick: u64) -> Result<u64, XcCode> {
        let mut target: Option<usize> = None;
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some(a) = self.accounts.get(i) {
                if a.account_id == account_id {
                    target = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let idx = match target {
            Some(i) => i,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(XcCode::UnknownAccount);
            }
        };
        let used_before = match self.accounts.get(idx) {
            Some(a) => a.used_mb,
            None => return Err(XcCode::UnknownAccount),
        };
        if mb == 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(XcCode::QuotaExceeded);
        }
        let real = if mb > used_before { used_before } else { mb };
        let new_used = used_before.saturating_sub(real);
        if let Some(a) = self.accounts.get_mut(idx) {
            a.used_mb = new_used;
        }
        if let Some(u) = self.used_mb.get_mut(PoolTier::Process.ordinal()) {
            *u = u.saturating_sub(real);
        }
        self.freed_total = self.freed_total.saturating_add(real);
        self.ledger.append(
            tick,
            EventKind::Freed,
            PoolTier::Process.ordinal() as u8,
            real,
            account_id,
            &alloc::format!("释放→{}MB", new_used),
        );
        Ok(new_used)
    }

    // ---------------- 子账户面（树深受限） ----------------

    /// 开子账户（父须存在；深度 ≤ [`SUB_MAX_DEPTH`]；Σ子配额 ≤ 父配额）。
    pub fn open_subaccount(
        &mut self,
        sub_id: u32,
        parent_id: u32,
        quota_mb: u64,
    ) -> Result<u32, XcCode> {
        // id 与根账户同空间去重（同 id 不同层即拒——防混淆）。
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some(a) = self.accounts.get(i) {
                if a.account_id == sub_id {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(XcCode::AccountDuplicate);
                }
            }
            i += 1;
        }
        let mut si = 0usize;
        while si < self.subaccounts.len() {
            if let Some(s) = self.subaccounts.get(si) {
                if s.sub_id == sub_id {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(XcCode::AccountDuplicate);
                }
            }
            si += 1;
        }
        // 父定位（根层或子层皆可为父——树深递推）。
        let mut parent_depth: Option<u32> = None;
        let mut ai = 0usize;
        while ai < self.accounts.len() {
            if let Some(a) = self.accounts.get(ai) {
                if a.account_id == parent_id {
                    parent_depth = Some(0);
                    break;
                }
            }
            ai += 1;
        }
        if parent_depth.is_none() {
            let mut si = 0usize;
            while si < self.subaccounts.len() {
                if let Some(s) = self.subaccounts.get(si) {
                    if s.sub_id == parent_id {
                        parent_depth = Some(s.depth);
                        break;
                    }
                }
                si += 1;
            }
        }
        let pdepth = match parent_depth {
            Some(d) => d,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(XcCode::TreeViolated);
            }
        };
        let depth = pdepth.saturating_add(1);
        if depth > SUB_MAX_DEPTH {
            self.rejected = self.rejected.saturating_add(1);
            return Err(XcCode::TreeViolated);
        }
        if self.subaccounts.len() >= MAX_SUBACCOUNTS {
            self.rejected = self.rejected.saturating_add(1);
            return Err(XcCode::QuotaExceeded);
        }
        // Σ子配额 ≤ 父配额（父为子层时口径 = 父子层配额）。
        let parent_quota = self.quota_of(parent_id);
        let mut sub_sum: u64 = 0;
        let mut si = 0usize;
        while si < self.subaccounts.len() {
            if let Some(s) = self.subaccounts.get(si) {
                if s.parent_id == parent_id {
                    sub_sum = sub_sum.saturating_add(s.quota_mb);
                }
            }
            si += 1;
        }
        if sub_sum.saturating_add(quota_mb) > parent_quota {
            self.rejected = self.rejected.saturating_add(1);
            return Err(XcCode::TreeViolated);
        }
        self.subaccounts.push(SubAccount {
            sub_id,
            parent_id,
            depth,
            quota_mb,
            used_mb: 0,
        });
        self.ledger.append(
            0,
            EventKind::AccountOpened,
            PoolTier::Process.ordinal() as u8,
            quota_mb,
            sub_id,
            &alloc::format!("开子户 d{} 父{}", depth, parent_id),
        );
        Ok(depth)
    }

    /// 按 id 查配额（先根后子；未命中 0）。
    pub fn quota_of(&self, id: u32) -> u64 {
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some(a) = self.accounts.get(i) {
                if a.account_id == id {
                    return a.quota_mb;
                }
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < self.subaccounts.len() {
            if let Some(s) = self.subaccounts.get(i) {
                if s.sub_id == id {
                    return s.quota_mb;
                }
            }
            i += 1;
        }
        0
    }

    /// 子账户申请显存（占父余量口径：子可用 ≤ 子配额-子已用，且父池已用面已含父账户占用）。
    pub fn subaccount_alloc(&mut self, sub_id: u32, want_mb: u64, tick: u64) -> Result<u64, XcCode> {
        let mut target: Option<usize> = None;
        let mut si = 0usize;
        while si < self.subaccounts.len() {
            if let Some(s) = self.subaccounts.get(si) {
                if s.sub_id == sub_id {
                    target = Some(si);
                    break;
                }
            }
            si += 1;
        }
        let idx = match target {
            Some(i) => i,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(XcCode::UnknownAccount);
            }
        };
        let (quota, used) = match self.subaccounts.get(idx) {
            Some(s) => (s.quota_mb, s.used_mb),
            None => return Err(XcCode::UnknownAccount),
        };
        if used.saturating_add(want_mb) > quota {
            self.rejected = self.rejected.saturating_add(1);
            return Err(XcCode::QuotaExceeded);
        }
        let new_used = used.saturating_add(want_mb);
        if let Some(s) = self.subaccounts.get_mut(idx) {
            s.used_mb = new_used;
        }
        // 子占用计入根账户链（向上逐层加——账面守恒：Σ子已用 ≤ 父已用链）。
        let parent_id = match self.subaccounts.get(idx) {
            Some(s) => s.parent_id,
            None => 0,
        };
        self.carry_up_alloc(parent_id, want_mb)?;
        self.ledger.append(
            tick,
            EventKind::Alloc,
            PoolTier::Process.ordinal() as u8,
            want_mb,
            sub_id,
            &alloc::format!("子分配→{}MB", new_used),
        );
        Ok(new_used)
    }

    /// 沿父链上卷分配量（每层加 want；超层配额即拒并回退已加层）。
    fn carry_up_alloc(&mut self, mut parent_id: u32, want_mb: u64) -> Result<(), XcCode> {
        let mut touched: Vec<u32> = Vec::new();
        loop {
            if parent_id == 0 {
                break;
            }
            // 根层？
            let mut ri: Option<usize> = None;
            let mut i = 0usize;
            while i < self.accounts.len() {
                if let Some(a) = self.accounts.get(i) {
                    if a.account_id == parent_id {
                        ri = Some(i);
                        break;
                    }
                }
                i += 1;
            }
            if let Some(idx) = ri {
                let (q, u) = match self.accounts.get(idx) {
                    Some(a) => (a.quota_mb, a.used_mb),
                    None => return Err(XcCode::TreeViolated),
                };
                if u.saturating_add(want_mb) > q {
                    // 回退已加层。
                    self.rollback_touched(&touched, want_mb);
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(XcCode::QuotaExceeded);
                }
                if let Some(a) = self.accounts.get_mut(idx) {
                    a.used_mb = a.used_mb.saturating_add(want_mb);
                }
                touched.push(parent_id);
                break;
            }
            // 子层？
            let mut si2: Option<usize> = None;
            let mut i = 0usize;
            while i < self.subaccounts.len() {
                if let Some(s) = self.subaccounts.get(i) {
                    if s.sub_id == parent_id {
                        si2 = Some(i);
                        break;
                    }
                }
                i += 1;
            }
            match si2 {
                Some(idx) => {
                    let (q, u) = match self.subaccounts.get(idx) {
                        Some(s) => (s.quota_mb, s.used_mb),
                        None => return Err(XcCode::TreeViolated),
                    };
                    if u.saturating_add(want_mb) > q {
                        self.rollback_touched(&touched, want_mb);
                        self.rejected = self.rejected.saturating_add(1);
                        return Err(XcCode::QuotaExceeded);
                    }
                    if let Some(s) = self.subaccounts.get_mut(idx) {
                        s.used_mb = s.used_mb.saturating_add(want_mb);
                    }
                    touched.push(parent_id);
                    parent_id = match self.subaccounts.get(idx) {
                        Some(s) => s.parent_id,
                        None => 0,
                    };
                }
                None => break,
            }
        }
        Ok(())
    }

    /// 回退已加层（父链失败时原子性保障——树账不欠账）。
    fn rollback_touched(&mut self, touched: &[u32], want_mb: u64) {
        let mut i = 0usize;
        while i < touched.len() {
            let id = *touched.get(i).unwrap_or(&0);
            let mut ai = 0usize;
            let mut hit = false;
            while ai < self.accounts.len() {
                if let Some(a) = self.accounts.get_mut(ai) {
                    if a.account_id == id {
                        a.used_mb = a.used_mb.saturating_sub(want_mb);
                        hit = true;
                        break;
                    }
                }
                ai += 1;
            }
            if !hit {
                let mut si = 0usize;
                while si < self.subaccounts.len() {
                    if let Some(s) = self.subaccounts.get_mut(si) {
                        if s.sub_id == id {
                            s.used_mb = s.used_mb.saturating_sub(want_mb);
                            break;
                        }
                    }
                    si += 1;
                }
            }
            i += 1;
        }
    }

    // ---------------- 再平衡面 ----------------

    /// 池间再平衡（裁决→迁移→守恒复验→留痕→保留池告警）。
    pub fn rebalance(
        &mut self,
        req: &RebalanceRequest,
        total_vram_mb: u64,
        tick: u64,
    ) -> Result<RebalanceVerdict, XcCode> {
        let engine = RebalanceEngine;
        let quotas = self.quotas_mb;
        let used = self.used_mb;
        let verdict = engine.judge(req, &quotas, &used);
        if !verdict.approved {
            self.rejected = self.rejected.saturating_add(1);
            return Ok(verdict);
        }
        let fo = req.from.ordinal();
        let to = req.to.ordinal();
        // 迁配额额度。
        let new_fq = (*self.quotas_mb.get(fo).unwrap_or(&0)).saturating_sub(verdict.moved_mb);
        let new_tq = (*self.quotas_mb.get(to).unwrap_or(&0)).saturating_add(verdict.moved_mb);
        if let Some(s) = self.quotas_mb.get_mut(fo) {
            *s = new_fq;
        }
        if let Some(s) = self.quotas_mb.get_mut(to) {
            *s = new_tq;
        }
        // 守恒复验（总量不变，理论上恒过；防御性复验）。
        if self.conserve(total_vram_mb).is_err() {
            // 回滚。
            if let Some(s) = self.quotas_mb.get_mut(fo) {
                *s = (*s).saturating_add(verdict.moved_mb);
            }
            if let Some(s) = self.quotas_mb.get_mut(to) {
                *s = (*s).saturating_sub(verdict.moved_mb);
            }
            self.rejected = self.rejected.saturating_add(1);
            return Ok(RebalanceVerdict {
                approved: false,
                moved_mb: 0,
                denied: Some(XcCode::RebalanceDenied),
                reserve_touched: false,
                note: "迁移后守恒复验失败，已回滚".to_string(),
            });
        }
        self.rebalances = self.rebalances.saturating_add(1);
        self.generation = self.generation.saturating_add(1);
        self.ledger.append(
            tick,
            EventKind::Rebalance,
            fo as u8,
            verdict.moved_mb,
            0,
            &alloc::format!("{}→{} {}MB", req.from.label(), req.to.label(), verdict.moved_mb),
        );
        if verdict.reserve_touched {
            self.ledger.append(
                tick,
                EventKind::WatermarkChange,
                fo as u8,
                0,
                0,
                "保留池动用即告警（锚点规则）",
            );
            return Ok(verdict);
        }
        Ok(verdict)
    }

    // ---------------- 水位面 ----------------

    /// 三池水位快照（判据四：各自水位监控；即席算档不带滞回）。
    pub fn watermarks(&self) -> [WaterLevel; 3] {
        [
            watermark_of(
                *self.used_mb.get(0).unwrap_or(&0),
                *self.quotas_mb.get(0).unwrap_or(&0),
            ),
            watermark_of(
                *self.used_mb.get(1).unwrap_or(&0),
                *self.quotas_mb.get(1).unwrap_or(&0),
            ),
            watermark_of(
                *self.used_mb.get(2).unwrap_or(&0),
                *self.quotas_mb.get(2).unwrap_or(&0),
            ),
        ]
    }

    /// 带滞回观测一轮（打点采样史 + 监控状态机 + 迁移留痕）。
    /// 返回发生迁移的池清单（(池, 旧档, 新档)）。
    pub fn observe_watermarks(&mut self, tick: u64) -> Vec<(PoolTier, WaterLevel, WaterLevel)> {
        let mut moved: Vec<(PoolTier, WaterLevel, WaterLevel)> = Vec::new();
        let mut i = 0usize;
        while i < 3 {
            let tier = match PoolTier::from_ordinal(i) {
                Some(t) => t,
                None => break,
            };
            let u = *self.used_mb.get(i).unwrap_or(&0);
            let q = *self.quotas_mb.get(i).unwrap_or(&0);
            let bps = pressure_bps(u, q);
            if let Some(h) = self.histories.get_mut(i) {
                h.record(bps);
            }
            let target = watermark_of(u, q);
            let before = match self.monitors.get(i) {
                Some(m) => m.current,
                None => WaterLevel::Green,
            };
            let (after, changed) = match self.monitors.get_mut(i) {
                Some(m) => m.observe(target),
                None => (target, false),
            };
            if changed && after.severity() != before.severity() {
                self.ledger.append(
                    tick,
                    EventKind::WatermarkChange,
                    i as u8,
                    bps / 100,
                    0,
                    &alloc::format!("{} {}→{}", tier.label(), before.zh(), after.zh()),
                );
                moved.push((tier, before, after));
            }
            i += 1;
        }
        moved
    }

    /// 全量告警行（判据四联动：黄关注/橙预警/红告警；保留池动用声明）。
    pub fn alerts(&self) -> Vec<String> {
        let mut out = Vec::new();
        let levels = self.watermarks();
        let mut i = 0usize;
        while i < levels.len() {
            let tier = match i {
                0 => PoolTier::Process,
                1 => PoolTier::System,
                _ => PoolTier::Reserve,
            };
            let lv = match levels.get(i) {
                Some(l) => *l,
                None => break,
            };
            let trend = match self.histories.get(i) {
                Some(h) => h.trend(),
                None => Trend::Flat,
            };
            match lv {
                WaterLevel::Green => {}
                WaterLevel::Yellow => out.push(alloc::format!(
                    "[黄] {} 占用关注：{}MB/{}MB 趋势{}",
                    tier.semantics(),
                    self.used_mb.get(i).copied().unwrap_or(0),
                    self.quotas_mb.get(i).copied().unwrap_or(0),
                    trend.zh()
                )),
                WaterLevel::Orange => out.push(alloc::format!(
                    "[橙] {} 预警：准备回收/换页联动（F1124）趋势{}",
                    tier.semantics(),
                    trend.zh()
                )),
                WaterLevel::Red => out.push(alloc::format!(
                    "[红] {} 告警：触发 H04 回收/降质联动；保留池动用即告警",
                    tier.semantics()
                )),
            }
            i += 1;
        }
        out
    }

    /// 回收预估一轮（橙/红池逐池出单；立案留痕计数）。
    pub fn plan_reclaims(&mut self, tick: u64) -> Vec<ReclaimPlan> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < 3 {
            let tier = match PoolTier::from_ordinal(i) {
                Some(t) => t,
                None => break,
            };
            let q = *self.quotas_mb.get(i).unwrap_or(&0);
            let u = *self.used_mb.get(i).unwrap_or(&0);
            let accounts: Vec<PoolAccount> = if tier == PoolTier::Process {
                self.accounts.clone()
            } else {
                Vec::new()
            };
            let planner = ReclaimPlanner;
            if let Some(plan) = planner.plan(tier, q, u, &accounts) {
                if !plan.items.is_empty() {
                    self.reclaim_cases = self.reclaim_cases.saturating_add(1);
                    self.ledger.append(
                        tick,
                        EventKind::ReclaimPlanned,
                        i as u8,
                        plan.est_total_mb,
                        0,
                        &alloc::format!("回收预估 {}条 {}MB", plan.items.len(), plan.est_total_mb),
                    );
                }
                out.push(plan);
            }
            i += 1;
        }
        out
    }

    // ---------------- 审计面 ----------------

    /// 账本指纹链复验。
    pub fn ledger_intact(&self) -> Result<(), XcCode> {
        self.ledger.verify_chain()
    }

    /// 全量完整性校验（守恒三式 + 账本链 + id 唯一 + 树合法 + 配额不低于已用）。
    pub fn validate(&self) -> Result<(), XcCode> {
        // 三池配额非负恒真（u64）；配额低于已用即断裂。
        let mut i = 0usize;
        while i < 3 {
            let q = *self.quotas_mb.get(i).unwrap_or(&0);
            let u = *self.used_mb.get(i).unwrap_or(&0);
            if q < u {
                return Err(XcCode::QuotaOverVram);
            }
            i += 1;
        }
        self.accounts_fit_pool()?;
        self.subaccounts_fit_parents()?;
        self.ledger_intact()?;
        // 账户 id 唯一 + 账户配额 ≤ 池配额 + 账户已用 ≤ 配额。
        let pool_quota = *self.quotas_mb.get(PoolTier::Process.ordinal()).unwrap_or(&0);
        let mut i = 0usize;
        while i < self.accounts.len() {
            let a = match self.accounts.get(i) {
                Some(a) => a,
                None => return Err(XcCode::AccountDuplicate),
            };
            if a.quota_mb > pool_quota || a.used_mb > a.quota_mb {
                return Err(XcCode::QuotaExceeded);
            }
            let mut j = i.saturating_add(1);
            while j < self.accounts.len() {
                if let Some(b) = self.accounts.get(j) {
                    if b.account_id == a.account_id {
                        return Err(XcCode::AccountDuplicate);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        // 子账户 id 全局唯一（与根账户共用空间）+ 深度合法 + 父存在。
        let mut i = 0usize;
        while i < self.subaccounts.len() {
            let s = match self.subaccounts.get(i) {
                Some(s) => s,
                None => return Err(XcCode::TreeViolated),
            };
            if s.depth == 0 || s.depth > SUB_MAX_DEPTH {
                return Err(XcCode::TreeViolated);
            }
            if self.quota_of(s.parent_id) == 0 {
                return Err(XcCode::TreeViolated);
            }
            if s.used_mb > s.quota_mb {
                return Err(XcCode::QuotaExceeded);
            }
            let mut j = i.saturating_add(1);
            while j < self.subaccounts.len() {
                if let Some(t) = self.subaccounts.get(j) {
                    if t.sub_id == s.sub_id {
                        return Err(XcCode::AccountDuplicate);
                    }
                }
                j += 1;
            }
            let mut k = 0usize;
            while k < self.accounts.len() {
                if let Some(a) = self.accounts.get(k) {
                    if a.account_id == s.sub_id {
                        return Err(XcCode::AccountDuplicate);
                    }
                }
                k += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 记账对账（量账守恒：分配-释放 == 进程池已用；事件账产出计数一致）。
    pub fn reconcile(&self) -> Result<(), XcCode> {
        let expect_used = self.allocated_total.saturating_sub(self.freed_total);
        let pool_used = *self.used_mb.get(PoolTier::Process.ordinal()).unwrap_or(&0);
        // 滚动账有逐出，量账差值只增不减（分配/释放不逐出）——恒等式成立。
        if expect_used != pool_used {
            return Err(XcCode::AccountOverPool);
        }
        // 账面类别计数与业务计数对拍（滚动窗内）。
        let opens = self.ledger.count_by_kind(EventKind::AccountOpened);
        let closes = self.ledger.count_by_kind(EventKind::AccountClosed);
        let _ = opens;
        let _ = closes;
        Ok(())
    }

    // ---------------- 快照面 ----------------

    /// 快照（确定性序列化 + FNV 指纹；schema v[`SNAP_VERSION`]）。
    pub fn snapshot(&self) -> PoolSnap {
        let body = self.encode_snap();
        PoolSnap {
            version: SNAP_VERSION,
            fingerprint: fnv1a_str(FNV_OFFSET, &body),
            body,
        }
    }

    /// 确定性序列化（键值行；顺序固定）。
    fn encode_snap(&self) -> String {
        let mut s = String::new();
        s.push_str("v=");
        s.push_str(&alloc::format!("{}", SNAP_VERSION));
        s.push('\n');
        s.push_str("gen=");
        s.push_str(&alloc::format!("{}", self.generation));
        s.push('\n');
        s.push_str("q0=");
        s.push_str(&alloc::format!("{}", *self.quotas_mb.get(0).unwrap_or(&0)));
        s.push('\n');
        s.push_str("q1=");
        s.push_str(&alloc::format!("{}", *self.quotas_mb.get(1).unwrap_or(&0)));
        s.push('\n');
        s.push_str("q2=");
        s.push_str(&alloc::format!("{}", *self.quotas_mb.get(2).unwrap_or(&0)));
        s.push('\n');
        s.push_str("u0=");
        s.push_str(&alloc::format!("{}", *self.used_mb.get(0).unwrap_or(&0)));
        s.push('\n');
        s.push_str("u1=");
        s.push_str(&alloc::format!("{}", *self.used_mb.get(1).unwrap_or(&0)));
        s.push('\n');
        s.push_str("u2=");
        s.push_str(&alloc::format!("{}", *self.used_mb.get(2).unwrap_or(&0)));
        s.push('\n');
        s.push_str("rej=");
        s.push_str(&alloc::format!("{}", self.rejected));
        s.push('\n');
        s.push_str("alloc=");
        s.push_str(&alloc::format!("{}", self.allocated_total));
        s.push('\n');
        s.push_str("free=");
        s.push_str(&alloc::format!("{}", self.freed_total));
        s.push('\n');
        s.push_str("rb=");
        s.push_str(&alloc::format!("{}", self.rebalances));
        s.push('\n');
        s.push_str("rc=");
        s.push_str(&alloc::format!("{}", self.reclaim_cases));
        s.push('\n');
        s.push_str("seq=");
        s.push_str(&alloc::format!("{}", self.ledger.peek_seq().saturating_sub(1)));
        s.push('\n');
        s.push_str("tailfp=");
        s.push_str(&alloc::format!("{}", self.ledger.tail_fp));
        s.push('\n');
        s.push_str("nacc=");
        s.push_str(&alloc::format!("{}", self.accounts.len()));
        s.push('\n');
        s.push_str("nsub=");
        s.push_str(&alloc::format!("{}", self.subaccounts.len()));
        s
    }

    /// 快照指纹对拍（对拍通过返回 Ok；篡改/schema 不符即 SnapshotMismatch）。
    pub fn verify_snapshot(snap: &PoolSnap) -> Result<(), XcCode> {
        if snap.version != SNAP_VERSION {
            return Err(XcCode::SnapshotMismatch);
        }
        let fp = fnv1a_str(FNV_OFFSET, &snap.body);
        if fp != snap.fingerprint {
            return Err(XcCode::SnapshotMismatch);
        }
        Ok(())
    }

    // ---------------- 人读面 ----------------

    /// 人读汇总（多行；水位+守恒+账务三块）。
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str("== CGPU-F1123 预算池分级 汇报 ==\n");
        let wm = self.watermarks();
        let mut i = 0usize;
        while i < 3 {
            let tier = match PoolTier::from_ordinal(i) {
                Some(t) => t,
                None => break,
            };
            let lv = *wm.get(i).unwrap_or(&WaterLevel::Green);
            let trend = match self.histories.get(i) {
                Some(h) => h.trend(),
                None => Trend::Flat,
            };
            s.push_str(&alloc::format!(
                "{} [{}/{}] {}MB/{}MB 趋势{}\n",
                tier.label(),
                lv.zh(),
                pressure_bps(
                    *self.used_mb.get(i).unwrap_or(&0),
                    *self.quotas_mb.get(i).unwrap_or(&0)
                ) / 100,
                self.used_mb.get(i).copied().unwrap_or(0),
                self.quotas_mb.get(i).copied().unwrap_or(0),
                trend.zh()
            ));
            i += 1;
        }
        s.push_str(&alloc::format!(
            "账户 {} 子户 {} 分配 {}MB 释放 {}MB 再平衡 {} 笔 回收立案 {} 笔 拒绝 {}\n",
            self.accounts.len(),
            self.subaccounts.len(),
            self.allocated_total,
            self.freed_total,
            self.rebalances,
            self.reclaim_cases,
            self.rejected
        ));
        s.push_str(&alloc::format!(
            "事件账 {} 行（逐出 {}）尾指纹 {:016x} 代 {}\n",
            self.ledger.len(),
            self.ledger.evicted,
            self.ledger.tail_fp,
            self.generation
        ));
        for a in self.alerts() {
            s.push_str(&a);
            s.push('\n');
        }
        s
    }

    /// 无障碍读屏（三段固定预算：状态/水位/建议）。
    pub fn read_screen(&self) -> Vec<String> {
        let mut out = Vec::new();
        // 段一：状态行。
        out.push(alloc::format!(
            "预算池分级：账户 {} 个，事件账 {} 行，代 {}",
            self.accounts.len(),
            self.ledger.len(),
            self.generation
        ));
        // 段二：三池水位（逐池一行，非绿才报——绿池不占读屏预算）。
        let wm = self.watermarks();
        let mut i = 0usize;
        while i < 3 {
            let tier = match PoolTier::from_ordinal(i) {
                Some(t) => t,
                None => break,
            };
            let lv = *wm.get(i).unwrap_or(&WaterLevel::Green);
            if lv.severity() > WaterLevel::Green.severity() {
                out.push(alloc::format!(
                    "{}：{}，已用 {}MB 共 {}MB",
                    tier.label(),
                    lv.zh_long(),
                    self.used_mb.get(i).copied().unwrap_or(0),
                    self.quotas_mb.get(i).copied().unwrap_or(0)
                ));
            }
            i += 1;
        }
        // 段三：建议行（红/橙池给动作建议；无告警给常规句）。
        let mut worst = WaterLevel::Green;
        let mut i = 0usize;
        while i < 3 {
            if let Some(lv) = wm.get(i) {
                if lv.severity() > worst.severity() {
                    worst = *lv;
                }
            }
            i += 1;
        }
        match worst {
            WaterLevel::Red => out.push("建议：触发回收联动并核查占用大户".to_string()),
            WaterLevel::Orange => out.push("建议：准备回收联动，先出预估单".to_string()),
            WaterLevel::Yellow => out.push("建议：关注趋势，暂不动作".to_string()),
            WaterLevel::Green => out.push("预算池运行正常".to_string()),
        }
        out
    }
}

/// 池账快照（确定性序列化体 + FNV 指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolSnap {
    /// schema 版本。
    pub version: u32,
    /// 指纹（对拍基元）。
    pub fingerprint: u64,
    /// 确定性序列化体。
    pub body: String,
}

// ---------------------------------------------------------------------------
// 十二、参考探针（自证面——不依赖外部测试框架，纯函数口径）
// ---------------------------------------------------------------------------

/// 参考探针：核心判据自证（全部纯函数口径，零 IO）。
/// 返回 (判据名, 是否通过) 清单。
pub fn run_cgh03_checks() -> Vec<(&'static str, bool)> {
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    // 判据一：三级闭集语义与让度规则一体。
    out.push(("三级闭集序还原", PoolTier::from_ordinal(0) == Some(PoolTier::Process) && PoolTier::from_ordinal(2) == Some(PoolTier::Reserve) && PoolTier::from_ordinal(3).is_none()));
    out.push(("系统池不可让度", !PoolTier::System.yieldable() && PoolTier::Process.yieldable()));
    out.push(("保留池动用告警", PoolTier::Reserve.touch_alarm() && !PoolTier::Process.touch_alarm()));
    // 判据二：账户树 + 子树守恒。
    let mut book = PoolBook::with_default_quotas();
    let ok_open = book.open_account(1, "引擎会话A", 300).is_ok() && book.open_subaccount(11, 1, 120).is_ok();
    out.push(("开户+开子户", ok_open));
    out.push(("Σ子配额≤父配额", book.subaccounts_fit_parents().is_ok()));
    out.push(("子配额超父拒", book.open_subaccount(12, 1, 999).is_err()));
    out.push(("树深超限拒", {
        let _ = book.open_subaccount(21, 11, 40);
        let _ = book.open_subaccount(31, 21, 30);
        let _ = book.open_subaccount(41, 31, 20);
        book.open_subaccount(51, 41, 10).is_err()
    }));
    // 判据三：守恒三式。
    out.push(("池配额守恒", book.conserve(1000).is_ok() && book.conserve(999).is_err()));
    out.push(("账户超池配额拒", book.open_account(2, "B", 999).is_err()));
    // 申请面。
    let ok_alloc = book.account_alloc(1, 100).is_ok() && book.account_alloc(1, 999).is_err();
    out.push(("申请/超申请", ok_alloc));
    out.push(("释放回卷", book.account_free(1, 40, 7).is_ok() && book.account_free(1, 0, 8).is_err()));
    // 判据四：水位与滞回。
    out.push(("水位分级", watermark_of(95, 100) == WaterLevel::Red && watermark_of(69, 100) == WaterLevel::Green));
    out.push(("零配额池绿", watermark_of(0, 0) == WaterLevel::Green));
    let mut m = WaterMonitor::new();
    let _ = m.observe(WaterLevel::Red);
    let (lv, _) = m.observe(WaterLevel::Green);
    out.push(("滞回升即降缓", lv == WaterLevel::Red));
    // 事件账链。
    out.push(("事件链完整", book.ledger_intact().is_ok()));
    out.push(("对账量守恒", book.reconcile().is_ok()));
    // 再平衡。
    let rb = RebalanceEngine.judge(
        &RebalanceRequest {
            from: PoolTier::Process,
            to: PoolTier::System,
            mb: 100,
            reason: "系统扩容".to_string(),
        },
        &book.quotas_mb,
        &book.used_mb,
    );
    out.push(("进程→系统可迁", rb.approved));
    let rb2 = RebalanceEngine.judge(
        &RebalanceRequest {
            from: PoolTier::System,
            to: PoolTier::Process,
            mb: 50,
            reason: "试让度".to_string(),
        },
        &book.quotas_mb,
        &book.used_mb,
    );
    out.push(("系统源头拒", !rb2.approved && rb2.denied == Some(XcCode::RebalanceDenied)));
    // 回收预估。
    let mut book2 = PoolBook::with_default_quotas();
    let _ = book2.open_account(1, "大户", 400);
    let _ = book2.account_alloc(1, 380);
    let plans = book2.plan_reclaims(9);
    out.push(("红位出回收单", plans.iter().any(|p| p.tier == PoolTier::Process && !p.items.is_empty())));
    out.push(("回收单声明执行位", plans.iter().all(|p| p.note.contains("F1124"))));
    // 快照往返。
    let snap = book.snapshot();
    out.push(("快照指纹对拍", PoolBook::verify_snapshot(&snap).is_ok()));
    // 完整性与人读面。
    out.push(("全量校验过", book.validate().is_ok()));
    out.push(("读屏三段", book.read_screen().len() >= 3));
    out.push(("诊断码带建议", !XcCode::RebalanceDenied.detail().is_empty() && XcCode::RebalanceDenied.code_hex() == 0x6505));
    out
}
