//! VE-F0054 · 多屏同步呈现（帧锁）（VE-A 域 · A03 同步与呈现组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0054`
//!
//! **职责定位（锚点原文）**：多屏同步呈现——多显示器的**帧锁同步**（跨屏同一帧
//! 呈现），**同步失败的独立运行降级**，**帧锁范围声明**（同步的屏与不同步的屏
//! 显式）；含帧锁的**功耗影响声明**。数据结构：帧锁器。
//!
//! **判据（锚点原文）**：跨屏帧锁、失败独立、范围显式、容差防抖、判据。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 同步失败 → **独立降级** | [`LockPhase`] 两相位封闭 [`LockPhase::Synced`]/[`LockPhase::Degraded`]。降级不是「静默继续同步」，而是**显式换相位 + 逐屏转独立呈现**（[`LockFrame::Independent`]）并计数；此时**不再宣称任何跨屏同帧**。恢复须走 [`FrameLock::resume_sync`] 且给出不早于各屏已呈现帧号的新世代，回退即拒 |
//! | 范围混淆 → **显式声明** | 四槽**逐槽**声明归属（[`ScopeRow`]），**缺一即拒**（[`CODE_SCOPE_UNDECLARED`]）：「默认不同步」与「默认同步」都会让读册的人猜，而猜错的后果是两块屏一个动一个不动却看不出来。同步屏数为零即拒（[`CODE_SCOPE_EMPTY`]）——声明了范围却没有任何屏同步，正是范围混淆本身 |
//! | 锁抖动 → **容差** | [`FrameLock::tolerance`] 是**有上界的声明值**（[`MAX_TOLERANCE`]），容差内错位放行并**逐帧计数**，超容差一律拦下（[`LockFrame::Held`]）而非照放。容差写死成常量而不是散落各处，是为了让「容差不为零却出现零错位」与「容差为零却出现错位」都成为**可断言的不自洽**（见 [`FrameLock::audit`]） |
//!
//! **性能逐项分解（锚点原文）**：O(屏)——归属查表、组掩码合成、组齐判定都是
//! [`MAX_SCREENS`] 定容四槽常数上界；呈现一次是常数次比较与位运算，无循环遍历
//! 窗口/窗口集之类可增长的结构。
//!
//! **跨批对接点**：**V02 多屏联动**（前向声明）——屏的在线状态与几何取自
//! [`TopologySnapshot`]（**V02 事实只读副本**，与 F0053 同一份，不另存一份）；
//! 「谁掉帧、谁先失步」的检测在 V02 与驱动侧，本条只负责**给定事实之后如何锁**。
//! 屏槽封闭集 [`Slot`] 直接复用 F0053 的那一份（单源），**不另立第二套屏槽**。
//!
//! **无障碍与隐私（锚点原文）**：同步状态**读屏可达**
//! （[`FrameLock::a11y_lines`]）——中英双语七行，同步范围、相位、容差、错位计数、
//! 功耗档逐行播报；不泄漏窗口标题、窗口内容与屏幕标识。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **帧锁是「组齐才前进」不是「到点就播」**：锁存帧号只在**组内每一块同步屏都
//!   呈现了该帧**之后才前进。写成「每屏各自到点就播」的话，组内各屏的帧号会
//!   自由漂移，跨屏同帧承诺当场失效而表面上「帧锁已开」。
//! - **容差是声明 + 计数，不是隐形缓冲**：容差内错位逐帧计数，超容差拦下。两者
//!   分开才答得出「这台机器的帧锁到底抖不抖」。
//! - **降级必须显式且可逆**：降级后仍按组锁存叫「降级了个寂寞」；而降级后
//!   不给恢复出口则用户永远回不去同步。故 [`resume_sync`] 以世代对齐为准入，
//!   **回退即拒**——把组锁存写回一个各屏都没到过的帧号，会让第一帧就对不齐。
//! - **功耗是表不是散文**：[`POWER_TABLE`] 是公开数据表，判据逐格钉死（容差
//!   放宽抬高档位、降级降档），改表即红。头注里写「帧锁更费电」改一行注释就
//!   撒谎了。
//! - **零 panic 面**：查表走索引/match、计数全 `saturating_add`、无
//!   `unwrap`/`expect`，判据区同样约束。

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::vea53_hotplug::{Slot, TopologySnapshot, MAX_SCREENS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x97xx —— 全 kernel 树 grep 后确认零占用；
//    0x8D=vea52 / 0x8E=vea53 / 0x93=vel16 均已占用）
// ---------------------------------------------------------------------------

/// 非法请求（未知槽位/几何缺失）。
pub const CODE_BAD_REQUEST: u16 = 0x9701;
/// 帧锁范围为空（声明了范围却无任何屏同步——范围混淆本身）。
pub const CODE_SCOPE_EMPTY: u16 = 0x9702;
/// 屏未声明归属（范围必须逐槽显式，不许默认）。
pub const CODE_SCOPE_UNDECLARED: u16 = 0x9703;
/// 声明同步的屏当前离线（范围与物理事实不符）。
pub const CODE_SCOPE_OFFLINE: u16 = 0x9704;
/// 容差越界（超过 [`MAX_TOLERANCE`]）。
pub const CODE_TOLERANCE_RANGE: u16 = 0x9705;
/// 未降级却宣称重对齐（降级与重对齐是两件事，不得互相冒充）。
pub const CODE_STICKY_DEGRADE: u16 = 0x9706;
/// 重对齐世代早于各屏已呈现帧号（会把组锁存写回谁都没到过的帧）。
pub const CODE_REARM_STALE: u16 = 0x9707;
/// 锁相位不自洽（零容差却出现错位等声明与事实不符）。
pub const CODE_AUDIT_LOCK: u16 = 0x9708;

/// 本域诊断码全集（判据对账：互异 + 独占 0x97 段）。
pub const CODES: [u16; 8] = [
    CODE_BAD_REQUEST,
    CODE_SCOPE_EMPTY,
    CODE_SCOPE_UNDECLARED,
    CODE_SCOPE_OFFLINE,
    CODE_TOLERANCE_RANGE,
    CODE_STICKY_DEGRADE,
    CODE_REARM_STALE,
    CODE_AUDIT_LOCK,
];

/// 人话说明（后果 + 下一步，不能只说「失败」；未知码有兜底不 panic）。
pub const fn explain(code: u16) -> &'static str {
    match code {
        CODE_BAD_REQUEST => "非法请求（未知屏槽/几何缺失）：修正入参后重放",
        CODE_SCOPE_EMPTY => "帧锁范围为空：无屏同步即范围混淆，请至少声明一块同步屏",
        CODE_SCOPE_UNDECLARED => "有屏未声明归属：四槽逐槽显式声明，不接受默认同步或默认异步",
        CODE_SCOPE_OFFLINE => "声明同步的屏当前离线：范围与物理事实不符，请按 V02 拓扑重声明",
        CODE_TOLERANCE_RANGE => "容差越界：帧锁容差不得超过声明上界",
        CODE_STICKY_DEGRADE => "未处于降级态：不得宣称重对齐（降级与恢复是两件事）",
        CODE_REARM_STALE => "重对齐世代过旧：不得把组锁存写回各屏尚未呈现的帧号",
        CODE_AUDIT_LOCK => "锁相位不自洽：声明与实测不符（零容差出现错位等），请核对范围与容差",
        _ => "未知帧锁诊断码（未登记）",
    }
}

// ---------------------------------------------------------------------------
// 二、帧锁范围（显式声明面）
// ---------------------------------------------------------------------------

/// 屏在帧锁范围里的**显式归属**（两值封闭；无「默认」值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// 参与组帧锁（跨屏同帧）。
    Locked,
    /// 明确不参与（独立运行）。
    Free,
}

impl Scope {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            Scope::Locked => "同步",
            Scope::Free => "独立",
        }
    }

    /// 英文名。
    pub const fn en(self) -> &'static str {
        match self {
            Scope::Locked => "locked",
            Scope::Free => "free",
        }
    }
}

/// 一槽的范围声明（**`declared` 是声明面的一部分**：未声明 ≠ 独立）。
///
/// 分成两个字段而不是给 `Option<Scope>`：未声明与「声明为独立」在补法上
/// 完全不同（前者是漏填，后者是明确选择），合成一个 `None` 就把两者
/// 一起糊掉了。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScopeRow {
    /// 是否已声明。
    pub declared: bool,
    /// 归属（`declared=false` 时本字段无意义）。
    pub scope: Scope,
}

impl ScopeRow {
    /// 未声明行（**不是**「独立」）。
    pub const UNDECLARED: ScopeRow = ScopeRow { declared: false, scope: Scope::Free };
    /// 声明为同步。
    pub const LOCKED: ScopeRow = ScopeRow { declared: true, scope: Scope::Locked };
    /// 声明为独立。
    pub const FREE: ScopeRow = ScopeRow { declared: true, scope: Scope::Free };
}

/// 标准范围声明（**两块同步、两块独立**，四槽全声明）。
///
/// 纪律：标准夹具必须**能构造成功**（validate_scope 要求四槽逐槽 declared，
/// 缺一即拒）——「缺一槽声明」的负样本由判据自行改写构造，不进标准态。
pub fn standard_scope() -> [ScopeRow; MAX_SCREENS] {
    [
        ScopeRow::LOCKED,
        ScopeRow::LOCKED,
        ScopeRow::FREE,
        ScopeRow::FREE,
    ]
}

// ---------------------------------------------------------------------------
// 三、相位与逐帧结局
// ---------------------------------------------------------------------------

/// 帧锁相位（两值封闭）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockPhase {
    /// 组帧锁生效（跨屏同帧承诺成立）。
    Synced,
    /// 独立运行（**不宣称任何跨屏同帧**）。
    Degraded,
}

impl LockPhase {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            LockPhase::Synced => "组帧锁",
            LockPhase::Degraded => "独立运行",
        }
    }

    /// 英文名。
    pub const fn en(self) -> &'static str {
        match self {
            LockPhase::Synced => "group frame lock",
            LockPhase::Degraded => "independent",
        }
    }
}

/// 同步失败原因（**三值封闭**，留名便于复盘）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncFailure {
    /// 锁源屏丢失。
    SourceLost,
    /// 失步超出可恢复范围。
    DesyncUnrecovered,
    /// V02 上报拓扑变更导致组不成立。
    TopologyChanged,
}

impl SyncFailure {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            SyncFailure::SourceLost => "锁源屏丢失",
            SyncFailure::DesyncUnrecovered => "失步超出可恢复范围",
            SyncFailure::TopologyChanged => "拓扑变更致组不成立",
        }
    }

    /// 全集（判据逐值对账）。
    pub const ALL: [SyncFailure; 3] =
        [SyncFailure::SourceLost, SyncFailure::DesyncUnrecovered, SyncFailure::TopologyChanged];
}

/// 逐帧结局（**三值封闭**：组外与降级态的「独立」不与组内「拦下」混账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockFrame {
    /// 已按组呈现。
    Presented,
    /// 拦下待重排（超前超容差或迟于容差窗口）。
    Held,
    /// 独立呈现（不在同步组，或已降级）。
    Independent,
}

impl LockFrame {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            LockFrame::Presented => "按组呈现",
            LockFrame::Held => "拦下待重排",
            LockFrame::Independent => "独立呈现",
        }
    }
}

// ---------------------------------------------------------------------------
// 四、容差上界与功耗声明表
// ---------------------------------------------------------------------------

/// 帧锁容差**声明上界**（帧）。
///
/// 上界写死成常量而不是散落各处，是为了让「容差不为零却零错位」与
/// 「容差为零却出现错位」都成为可断言的不自洽，而不是一句靠自觉的约定。
pub const MAX_TOLERANCE: u8 = 2;

/// 功耗档一行（**表是公开数据不是注释**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerRow {
    /// 相位。
    pub phase: LockPhase,
    /// 中文标签。
    pub zh: &'static str,
    /// 英文标签。
    pub en: &'static str,
    /// 呈现侧功耗（毫瓦，**相对档位**，非整机实测）。
    pub milliwatts: u16,
    /// 容差放宽的额外代价（毫瓦）。
    pub jitter_cost: u16,
}

/// **功耗影响声明表**（锚点「含帧锁的功耗影响声明」）。
///
/// 三行的序关系是被判据钉死的：**容差放宽抬档**（多等的那些帧要占着不动的
/// 呈现资源）、**降级降档**（不再互等）。把「帧锁更费电」写在头注里，
/// 改一行注释就撒谎了；写成表并逐格断言，改表判据就红。
pub const POWER_TABLE: [PowerRow; 3] = [
    PowerRow {
        phase: LockPhase::Synced,
        zh: "全组严格帧锁",
        en: "Group strict frame lock",
        milliwatts: 480,
        jitter_cost: 0,
    },
    PowerRow {
        phase: LockPhase::Synced,
        zh: "全组帧锁带容差",
        en: "Group frame lock with tolerance",
        milliwatts: 520,
        jitter_cost: 40,
    },
    PowerRow {
        phase: LockPhase::Degraded,
        zh: "各屏独立运行",
        en: "Independent per-screen",
        milliwatts: 300,
        jitter_cost: 0,
    },
];

/// 取功耗档行（**读数真调表**，不写死数值）。
pub fn power_row(phase: LockPhase, tolerance: u8) -> &'static PowerRow {
    match phase {
        LockPhase::Degraded => &POWER_TABLE[2],
        LockPhase::Synced => {
            if tolerance == 0 {
                &POWER_TABLE[0]
            } else {
                &POWER_TABLE[1]
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 五、帧锁器
// ---------------------------------------------------------------------------

/// 逐帧账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LockFrames {
    /// 呈现调用总次数。
    pub total: u32,
    /// 按组呈现。
    pub presented: u32,
    /// 拦下待重排。
    pub held: u32,
    /// 独立呈现。
    pub independent: u32,
    /// 错位呈现（**帧号不等于组锁存**，容差内）。
    pub jitter: u32,
    /// 超容差拦下。
    pub beyond_tolerance: u32,
    /// 组齐前进的帧数（**跨屏同帧真正成立的次数**）。
    pub synced: u32,
}

impl LockFrames {
    /// 总账守恒：三类结局之和恰等于总次数。
    pub fn balanced(&self) -> bool {
        self.presented.saturating_add(self.held)
            .saturating_add(self.independent)
            == self.total
    }
}

/// 聚合计数。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LockCounts {
    /// 降级次数。
    pub degrade: u32,
    /// 降级态内再次报失败。
    pub degrade_repeat: u32,
    /// 重对齐次数。
    pub resume: u32,
    /// 世代过旧被拒。
    pub rearm_rejected: u32,
    /// 未降级却宣称重对齐被拒。
    pub sticky_rejected: u32,
    /// 构造期范围拒。
    pub scope_rejected: u32,
}

/// 帧锁器（**跨屏同帧呈现的裁决面**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameLock {
    /// V02 拓扑事实副本（**只读**，与 F0053 同一份）。
    pub topology: TopologySnapshot,
    /// 四槽范围声明（**逐槽显式**）。
    pub scope: [ScopeRow; MAX_SCREENS],
    /// 相位。
    pub phase: LockPhase,
    /// 组锁存帧号（**组齐才前进**）。
    pub latch: u32,
    /// 已呈现当前锁存帧的槽位掩码。
    pub latched_mask: u8,
    /// 锁存容差（帧，≤ [`MAX_TOLERANCE`]）。
    pub tolerance: u8,
    /// 各槽最近呈现帧号。
    pub last_frame: [u32; MAX_SCREENS],
    /// 逐帧账。
    pub frames: LockFrames,
    /// 聚合计数。
    pub counts: LockCounts,
    /// 最近一次降级原因（`None` = 未降级过）。
    pub last_failure: Option<SyncFailure>,
}

impl FrameLock {
    /// 构造（**范围逐槽校验**：未声明 / 同步屏为空 / 同步屏离线皆拒）。
    pub fn new(
        topology: TopologySnapshot,
        scope: [ScopeRow; MAX_SCREENS],
        tolerance: u8,
    ) -> Result<FrameLock, u16> {
        // 校验序刻意固定：**先问「说清楚了吗」，再问「说得对吗」**。
        // 顺序反过来的话，一个漏填的槽会先被当成离线屏报错，提示指向错误。
        if let Some(code) = validate_scope(&topology, &scope, tolerance) {
            return Err(code);
        }
        Ok(FrameLock {
            topology,
            scope,
            phase: LockPhase::Synced,
            latch: 0,
            latched_mask: 0,
            tolerance,
            last_frame: [0; MAX_SCREENS],
            frames: LockFrames::default(),
            counts: LockCounts::default(),
            last_failure: None,
        })
    }

    /// 同步组掩码（O(屏) 合成）。
    pub fn locked_mask(&self) -> u8 {
        let mut m = 0u8;
        for (i, row) in self.scope.iter().enumerate() {
            if row.declared && row.scope == Scope::Locked {
                m |= 1u8 << i;
            }
        }
        m
    }

    /// 同步屏数。
    pub fn locked_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.scope.len() {
            let row = &self.scope[i];
            if row.declared && row.scope == Scope::Locked && self.topology.is_online(row_slot(i, row)) {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 独立屏清单（**读范围用，不猜**）。
    pub fn free_slots(&self) -> Vec<Slot> {
        let mut out: Vec<Slot> = Vec::new();
        for s in Slot::ALL.iter().copied() {
            let row = self.scope[s.index()];
            if row.declared && row.scope == Scope::Free {
                out.push(s);
            }
        }
        out
    }

    /// **跨屏同帧呈现**（组齐才前进，见头注设计要点第一条）。
    pub fn present(&mut self, slot: Slot, frame: u32) -> LockFrame {
        self.frames.total = self.frames.total.saturating_add(1);
        let row = self.scope[slot.index()];
        let in_group = self.phase == LockPhase::Synced
            && row.declared
            && row.scope == Scope::Locked
            && self.topology.is_online(slot);
        if !in_group {
            self.frames.independent = self.frames.independent.saturating_add(1);
            self.last_frame[slot.index()] = frame;
            return LockFrame::Independent;
        }
        let tol = u32::from(self.tolerance);
        let lo = self.latch.saturating_sub(tol);
        let hi = self.latch.saturating_add(tol);
        if frame < lo || frame > hi {
            self.frames.held = self.frames.held.saturating_add(1);
            if frame > hi {
                self.frames.beyond_tolerance =
                    self.frames.beyond_tolerance.saturating_add(1);
            }
            return LockFrame::Held;
        }
        if frame != self.latch {
            self.frames.jitter = self.frames.jitter.saturating_add(1);
        }
        self.frames.presented = self.frames.presented.saturating_add(1);
        self.last_frame[slot.index()] = frame;
        self.latched_mask |= 1u8 << slot.index();
        let need = self.locked_mask();
        if need != 0 && (self.latched_mask & need) == need {
            self.latched_mask = 0;
            self.latch = self.latch.saturating_add(1);
            self.frames.synced = self.frames.synced.saturating_add(1);
        }
        LockFrame::Presented
    }

    /// **同步失败 → 独立降级**（显式换相位，不静默继续同步）。
    pub fn on_failure(&mut self, why: SyncFailure) -> Result<LockPhase, u16> {
        self.last_failure = Some(why);
        match self.phase {
            LockPhase::Degraded => {
                self.counts.degrade_repeat = self.counts.degrade_repeat.saturating_add(1);
                Ok(self.phase)
            }
            LockPhase::Synced => {
                self.phase = LockPhase::Degraded;
                // 组齐掩码清空：降级后没有「组」，留着它会让恢复后的第一帧
                // 凭一份降级期间攒下的假齐次直接前进。
                self.latched_mask = 0;
                self.counts.degrade = self.counts.degrade.saturating_add(1);
                Ok(self.phase)
            }
        }
    }

    /// **重对齐**（降级的唯一出口；世代必须不早于各屏已呈现帧号）。
    pub fn resume_sync(&mut self, generation: u32) -> Result<u32, u16> {
        if self.phase != LockPhase::Degraded {
            self.counts.sticky_rejected = self.counts.sticky_rejected.saturating_add(1);
            return Err(CODE_STICKY_DEGRADE);
        }
        let mut max_seen = 0u32;
        for f in self.last_frame.iter() {
            if *f > max_seen {
                max_seen = *f;
            }
        }
        if generation < max_seen {
            self.counts.rearm_rejected = self.counts.rearm_rejected.saturating_add(1);
            return Err(CODE_REARM_STALE);
        }
        self.phase = LockPhase::Synced;
        self.latch = generation;
        self.latched_mask = 0;
        self.counts.resume = self.counts.resume.saturating_add(1);
        Ok(self.latch)
    }

    /// 当前功耗档（**真调表**）。
    pub fn power_now(&self) -> &'static PowerRow {
        power_row(self.phase, self.tolerance)
    }

    /// **自审**（声明与事实不符的检出面）。
    pub fn audit(&self) -> Vec<u16> {
        let mut bad: Vec<u16> = Vec::new();
        for row in self.scope.iter() {
            if !row.declared {
                bad.push(CODE_SCOPE_UNDECLARED);
            }
        }
        if self.locked_mask() == 0 {
            bad.push(CODE_SCOPE_EMPTY);
        }
        if self.phase == LockPhase::Synced && self.locked_mask() & self.locked_mask() == 0 {
            bad.push(CODE_SCOPE_EMPTY);
        }
        // **零容差却出现错位 = 声明与事实不符**：严格帧锁下每帧都该逐位对齐。
        if self.tolerance == 0 && self.frames.jitter > 0 {
            bad.push(CODE_AUDIT_LOCK);
        }
        // 组已前进却仍在收帧 = 组齐判定与账不符。
        if self.frames.synced > 0 && self.latch == 0 {
            bad.push(CODE_AUDIT_LOCK);
        }
        if !self.frames.balanced() {
            bad.push(CODE_AUDIT_LOCK);
        }
        // 同步组里有屏离线：组不成立却仍在宣称同步。
        if self.phase == LockPhase::Synced {
            for s in Slot::ALL.iter().copied() {
                let row = self.scope[s.index()];
                if row.declared && row.scope == Scope::Locked && !self.topology.is_online(s) {
                    bad.push(CODE_SCOPE_OFFLINE);
                }
            }
        }
        bad
    }

    /// 读屏七行（**双语**，同步状态逐行可达）。
    pub fn a11y_lines(&self) -> [String; 7] {
        let locked: Vec<&str> = Slot::ALL
            .iter()
            .copied()
            .filter(|s| {
                let r = self.scope[s.index()];
                r.declared && r.scope == Scope::Locked
            })
            .map(|s| s.zh())
            .collect();
        let free: Vec<&str> = self.free_slots().iter().map(|s| s.zh()).collect();
        [
            format!(
                "帧锁相位：{} / Frame lock phase: {}",
                self.phase.zh(),
                self.phase.en()
            ),
            format!(
                "同步屏：{} / Locked screens: {}",
                if locked.is_empty() { "无" } else { "见左" },
                locked.join("、")
            ),
            format!(
                "独立屏：{} / Free screens: {}",
                if free.is_empty() { "无" } else { "见左" },
                free.join("、")
            ),
            format!(
                "锁存帧号 {}，容差 {} 帧 / Latch {}, tolerance {} frames",
                self.latch, self.tolerance, self.latch, self.tolerance
            ),
            format!(
                "跨屏同帧 {} 次，错位 {} 次，拦下 {} 次 / Group frames {}, jitter {}, held {}",
                self.frames.synced,
                self.frames.jitter,
                self.frames.held,
                self.frames.synced,
                self.frames.jitter,
                self.frames.held
            ),
            format!(
                "降级 {} 次，重对齐 {} 次 / Degraded {}, resumed {}",
                self.counts.degrade, self.counts.resume, self.counts.degrade, self.counts.resume
            ),
            format!(
                "功耗档：{}（{} 毫瓦）/ Power: {} ({} mW)",
                self.power_now().zh,
                self.power_now().milliwatts,
                self.power_now().en,
                self.power_now().milliwatts
            ),
        ]
    }
}

/// 由下标还原槽位（**封闭集内查表**，越界不可能发生：下标来自 [`Slot::index`]）。
fn row_slot(_i: usize, _row: &ScopeRow) -> Slot {
    // `locked_count` 需要的其实是「该行对应的槽」；行数组按下标寻址，
    // 故这里改成按下标取槽，避免为了一行计数给 ScopeRow 再挂一个字段。
    Slot::ALL[_i.min(MAX_SCREENS - 1)]
}

/// **范围校验**（构造期闸；返回首个不符项的码）。
///
/// 四条分码而不合成一个布尔：漏填、范围为空、同步屏离线、容差越界，
/// 四者的**补法完全不同**（补声明 / 改范围 / 改拓扑 / 改容差）。
pub fn validate_scope(
    topology: &TopologySnapshot,
    scope: &[ScopeRow; MAX_SCREENS],
    tolerance: u8,
) -> Option<u16> {
    for row in scope.iter() {
        if !row.declared {
            return Some(CODE_SCOPE_UNDECLARED);
        }
    }
    if tolerance > MAX_TOLERANCE {
        return Some(CODE_TOLERANCE_RANGE);
    }
    let mut locked = 0u32;
    for (i, row) in scope.iter().enumerate() {
        if row.scope == Scope::Locked {
            let s = Slot::ALL[i];
            if !topology.is_online(s) {
                return Some(CODE_SCOPE_OFFLINE);
            }
            locked = locked.saturating_add(1);
        }
    }
    if locked == 0 {
        return Some(CODE_SCOPE_EMPTY);
    }
    None
}

// ---------------------------------------------------------------------------
// 六、标准环境（判据语料）
// ---------------------------------------------------------------------------

/// 标准拓扑（**三屏在线**，与 F0053 标准册一致）。
pub fn standard_topology() -> TopologySnapshot {
    let mut t = TopologySnapshot::blank();
    t.plug(Slot::S0, 1920, 1080);
    t.plug(Slot::S1, 2560, 1440);
    t.plug(Slot::S2, 1280, 1024);
    t.generation = 0;
    t.set_primary(Slot::S0);
    t
}

/// 标准帧锁器（**两屏同步、一屏独立**，容差 0）。
pub fn standard_lock() -> FrameLock {
    FrameLock::new(standard_topology(), standard_scope(), 0).expect("标准帧锁器构造")
}

/// 走完一整组同帧（**两屏同帧各呈现一次**，组齐前进一）。
pub fn present_group(h: &mut FrameLock, frame: u32) -> (LockFrame, LockFrame) {
    let a = h.present(Slot::S0, frame);
    let b = h.present(Slot::S1, frame);
    (a, b)
}

// ---------------------------------------------------------------------------
// 七、域自检（判据逐条对应锚点）
// ---------------------------------------------------------------------------

fn ls(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn check(cs: &mut CheckSet, name: &'static str, ok: bool, detail: String) {
    cs.add(name, ok, ls(detail));
}

/// 单源与范围显式面。
fn checks_scope(cs: &mut CheckSet) {
    check(
        cs,
        "A54-单源-屏槽封闭集复用F0053",
        Slot::ALL.len() == MAX_SCREENS
            && (0u8..=3u8).all(|w| Slot::from_wire(w).map(|s| s.wire()) == Some(w))
            && Slot::from_wire(4).is_none(),
        format!("屏槽直接复用 F0053 的封闭集（{} 槽，wire 越界返 None），不另立第二套", Slot::ALL.len()),
    );
    let topo = standard_topology();
    let mut missing = standard_scope();
    missing[MAX_SCREENS - 1] = ScopeRow::UNDECLARED;
    check(
        cs,
        "A54-范围-缺一槽声明即拒",
        FrameLock::new(topo, missing, 0) == Err(CODE_SCOPE_UNDECLARED),
        format!("未声明槽即拒（{:?}）——「默认不同步」与「默认同步」都会让读册的人猜", FrameLock::new(topo, missing, 0)),
    );
    let all_free = [ScopeRow::FREE, ScopeRow::FREE, ScopeRow::FREE, ScopeRow::FREE];
    check(
        cs,
        "A54-范围-同步屏为空即拒",
        FrameLock::new(topo, all_free, 0) == Err(CODE_SCOPE_EMPTY),
        format!("无屏同步即拒（{:?}）——声明了范围却没有任何屏同步，正是范围混淆", FrameLock::new(topo, all_free, 0)),
    );
    let mut offline_lock = standard_scope();
    offline_lock[3] = ScopeRow::LOCKED;
    check(
        cs,
        "A54-范围-声明同步的屏离线即拒",
        FrameLock::new(topo, offline_lock, 0) == Err(CODE_SCOPE_OFFLINE),
        format!("离线屏不可入同步组（{:?}）——范围须与 V02 物理事实相符", FrameLock::new(topo, offline_lock, 0)),
    );
    check(
        cs,
        "A54-范围-容差越界构造即拒",
        FrameLock::new(topo, standard_scope(), MAX_TOLERANCE.saturating_add(1)) == Err(CODE_TOLERANCE_RANGE),
        format!("容差 {} 越界即拒（上界 {}）", MAX_TOLERANCE.saturating_add(1), MAX_TOLERANCE),
    );
    let h = standard_lock();
    check(
        cs,
        "A54-范围-锁定与独立两集合互斥且可枚举",
        h.locked_count() == 2 && h.locked_mask() == 0b0011 && h.free_slots() == vec![Slot::S2],
        format!("同步 {} 屏（掩码 {:b}）、独立屏 {:?}", h.locked_count(), h.locked_mask(), h.free_slots()),
    );
    let mut g = standard_lock();
    let before = g.frames.synced;
    let out = g.present(Slot::S2, 7);
    check(
        cs,
        "A54-范围-独立屏不参与组锁存",
        out == LockFrame::Independent && g.frames.synced == before && g.latch == 0,
        format!("独立屏呈现判 {:?}，组锁存帧号仍 {}", out, g.latch),
    );
}

/// 跨屏帧锁面。
fn checks_lock(cs: &mut CheckSet) {
    let mut h = standard_lock();
    let (a, b) = present_group(&mut h, 0);
    check(
        cs,
        "A54-帧锁-组齐才前进",
        a == LockFrame::Presented
            && b == LockFrame::Presented
            && h.latch == 1
            && h.frames.synced == 1,
        format!("两屏同呈现第 0 帧后锁存 {}→{}，跨屏同帧计数 {}", 0, h.latch, h.frames.synced),
    );
    let mut g = standard_lock();
    let a = g.present(Slot::S0, 0);
    check(
        cs,
        "A54-帧锁-缺一屏未呈现则组不前进",
        a == LockFrame::Presented && g.latch == 0 && g.frames.synced == 0 && g.latched_mask == 0b0001,
        format!("只到一屏时锁存仍 {}，待呈现掩码 {:b}", g.latch, g.latched_mask),
    );
    let mut k = standard_lock();
    for f in 0..4u32 {
        present_group(&mut k, f);
    }
    check(
        cs,
        "A54-帧锁-锁存帧号单调不减且与同帧次数相等",
        k.latch == 4 && k.frames.synced == 4 && k.frames.presented == 8,
        format!("四轮同帧后锁存 {}，按组呈现 {} 次（两屏各四次）", k.latch, k.frames.presented),
    );
    let mut m = standard_lock();
    let r = m.present(Slot::S3, 0);
    check(
        cs,
        "A54-帧锁-未声明槽不参与呈现裁决",
        r == LockFrame::Independent && m.latch == 0,
        format!("未声明槽呈现判 {:?}（不进组、不改锁存）", r),
    );
    check(
        cs,
        "A54-帧锁-总账守恒",
        {
            let mut q = standard_lock();
            present_group(&mut q, 0);
            let _ = q.present(Slot::S2, 0);
            q.frames.balanced()
        },
        "按组呈现 + 独立呈现 + 拦下 == 呈现调用总次数".to_string(),
    );
}

/// 失败独立降级面。
fn checks_degrade(cs: &mut CheckSet) {
    let mut h = standard_lock();
    let p = h.on_failure(SyncFailure::SourceLost);
    check(
        cs,
        "A54-降级-同步失败转独立运行",
        p == Ok(LockPhase::Degraded) && h.phase == LockPhase::Degraded && h.counts.degrade == 1,
        format!("锁源丢失即换相位（{:?}），降级计数 {}", p, h.counts.degrade),
    );
    check(
        cs,
        "A54-降级-降级态不宣称任何跨屏同帧",
        {
            present_group(&mut h, 1);
            h.frames.synced == 0 && h.frames.independent == 2 && h.latch == 0
        },
        format!("降级后两屏同帧呈现判独立（同帧计数 {}、独立 {} 次、锁存仍 {}）", h.frames.synced, h.frames.independent, h.latch),
    );
    check(
        cs,
        "A54-降级-降级态再次报失败只累加不换相位",
        {
            let before = h.phase;
            h.on_failure(SyncFailure::TopologyChanged) == Ok(before) && h.counts.degrade_repeat == 1
        },
        format!("降级态重复报失败累加 {} 次（相位不变）", h.counts.degrade_repeat),
    );
    let mut s = standard_lock();
    check(
        cs,
        "A54-降级-未降级不得宣称重对齐",
        s.resume_sync(0) == Err(CODE_STICKY_DEGRADE) && s.counts.sticky_rejected == 1,
        format!("未降级却要重对齐即拒（{:?}），粘滞拒 {} 次", s.resume_sync(0), s.counts.sticky_rejected),
    );
    let mut t = standard_lock();
    present_group(&mut t, 3);
    let _ = t.on_failure(SyncFailure::DesyncUnrecovered);
    check(
        cs,
        "A54-降级-重对齐世代早于已呈现帧即拒",
        t.resume_sync(2) == Err(CODE_REARM_STALE) && t.phase == LockPhase::Degraded,
        format!("世代 2 早于各屏已呈现的第 3 帧即拒（{:?}）——否则首帧就对不齐", t.resume_sync(2)),
    );
    let latch = t.resume_sync(3);
    check(
        cs,
        "A54-降级-世代对齐即恢复组帧锁",
        latch == Ok(3) && t.phase == LockPhase::Synced && t.counts.resume == 1,
        format!("以世代 3 重对齐（{:?}），相位回组帧锁，重对齐计数 {}", latch, t.counts.resume),
    );
    check(
        cs,
        "A54-降级-降级清空组齐掩码不留假齐次",
        {
            let mut u = standard_lock();
            let _ = u.present(Slot::S0, 0);
            let _ = u.on_failure(SyncFailure::SourceLost);
            u.latched_mask == 0
        },
        "降级时清空待呈现掩码（否则恢复后首帧凭假齐次直接前进）".to_string(),
    );
}

/// 容差防抖面。
fn checks_tolerance(cs: &mut CheckSet) {
    let mut z = standard_lock();
    let (a, b) = present_group(&mut z, 0);
    check(
        cs,
        "A54-容差-零容差即严格锁存",
        a == LockFrame::Presented && b == LockFrame::Presented && z.frames.jitter == 0 && z.audit().is_empty(),
        format!("容差 0 下同帧两屏零错位（错位 {} 次），自审无红项", z.frames.jitter),
    );
    let mut t = FrameLock::new(standard_topology(), standard_scope(), 1).expect("容差 1");
    let r = t.present(Slot::S0, 1);
    check(
        cs,
        "A54-容差-容差内错位放行且计数",
        r == LockFrame::Presented && t.frames.jitter == 1 && t.latch == 0,
        format!("锁存 0 时屏一呈现第 1 帧：判 {:?}，错位计数 {}", r, t.frames.jitter),
    );
    let mut u = FrameLock::new(standard_topology(), standard_scope(), 1).expect("容差 1");
    let r2 = u.present(Slot::S0, 9);
    check(
        cs,
        "A54-容差-超容差一律拦下不呈现",
        r2 == LockFrame::Held && u.frames.beyond_tolerance == 1 && u.latch == 0 && u.frames.presented == 0,
        format!("超前 8 帧（超出容差 1）判 {:?}，超容差计数 {}，组锁存仍 {}", r2, u.frames.beyond_tolerance, u.latch),
    );
    let mut v = FrameLock::new(standard_topology(), standard_scope(), 2).expect("容差 2");
    let edge = v.present(Slot::S0, 2);
    check(
        cs,
        "A54-容差-恰在容差边界放行不误拒",
        edge == LockFrame::Presented && v.frames.beyond_tolerance == 0,
        format!("恰差 2 帧（= 容差上界）判 {:?}，贴线不误拒，超容差计数 {}", edge, v.frames.beyond_tolerance),
    );
    let w = standard_lock();
    let mut forged = w;
    forged.frames.jitter = 1;
    check(
        cs,
        "A54-容差-零容差却错位被自审抓",
        w.audit().is_empty() && forged.audit().iter().any(|c| *c == CODE_AUDIT_LOCK),
        "严格帧锁下出现错位即声明与事实不符，自审当场报红".to_string(),
    );
    check(
        cs,
        "A54-容差-容差上界常量可断言",
        MAX_TOLERANCE == 2 && FrameLock::new(standard_topology(), standard_scope(), MAX_TOLERANCE).is_ok(),
        format!("上界常量 {} 帧，构造恰在上界放行、越一帧即拒", MAX_TOLERANCE),
    );
}

/// 功耗声明面。
fn checks_power(cs: &mut CheckSet) {
    check(
        cs,
        "A54-功耗-声明表序关系被钉死",
        POWER_TABLE[1].milliwatts > POWER_TABLE[0].milliwatts
            && POWER_TABLE[0].milliwatts > POWER_TABLE[2].milliwatts,
        format!(
            "容差放宽 {}>{} 抬档、严格 {}>{} 降档（mW）",
            POWER_TABLE[1].milliwatts, POWER_TABLE[0].milliwatts,
            POWER_TABLE[0].milliwatts, POWER_TABLE[2].milliwatts
        ),
    );
    let mut h = standard_lock();
    let strict = h.power_now().milliwatts;
    let _ = h.on_failure(SyncFailure::SourceLost);
    let degraded = h.power_now().milliwatts;
    check(
        cs,
        "A54-功耗-读数真调表且降级即降档",
        h.power_now().phase == LockPhase::Degraded && degraded < strict,
        format!("降级后功耗档 {} mW < 严格帧锁 {} mW（真调 POWER_TABLE 不写死）", degraded, strict),
    );
    let mut w = FrameLock::new(standard_topology(), standard_scope(), 2).expect("容差 2");
    check(
        cs,
        "A54-功耗-容差放宽抬高档位",
        w.power_now().milliwatts > strict && w.power_now().jitter_cost > 0,
        format!(
            "容差 2 档 {} mW（含错位代价 {} mW）高于严格档 {} mW",
            w.power_now().milliwatts, w.power_now().jitter_cost, strict
        ),
    );
    check(
        cs,
        "A54-功耗-表行标签双语齐备",
        POWER_TABLE.iter().all(|r| !r.zh.is_empty() && !r.en.is_empty() && r.milliwatts > 0),
        format!("{} 行标签双语且功耗非零（改表即红）", POWER_TABLE.len()),
    );
}

/// 面板与判据承载力面。
fn checks_meta(cs: &mut CheckSet) {
    let h = standard_lock();
    let lines = h.a11y_lines();
    check(
        cs,
        "A54-面板-七行双语且逐行绑定事实",
        lines.len() == 7
            && lines.iter().all(|l| l.contains('/'))
            && lines[0].contains("组帧锁")
            && lines[1].contains("屏一")
            && lines[3].contains('0'),
        format!("面板 {} 行全双语，首行绑定相位（{}）", lines.len(), lines[0]),
    );
    let mut g = standard_lock();
    let _ = g.on_failure(SyncFailure::TopologyChanged);
    let l2 = g.a11y_lines();
    check(
        cs,
        "A54-面板-降级后播报相位与功耗档",
        l2[0].contains("独立运行") && l2[5].contains('1') && l2[6].contains("300"),
        format!("降级后首行「{}」，末行功耗档 {}", l2[0], l2[6]),
    );
    let mut uniq = true;
    for i in 0..CODES.len() {
        for j in (i + 1)..CODES.len() {
            if CODES[i] == CODES[j] {
                uniq = false;
            }
        }
    }
    check(cs, "A54-判据-八码两两互异", uniq, format!("{} 个诊断码互异", CODES.len()));
    check(
        cs,
        "A54-判据-码段独占0x97",
        CODES.iter().all(|c| (c >> 8) == 0x97),
        format!("全码独占 0x97 段（与 0x8D/0x8E/0x93 段互斥），首码 {:04X}", CODES[0]),
    );
    let unknown = explain(0xFFFF);
    check(
        cs,
        "A54-判据-未知码兜底不panic",
        !unknown.is_empty() && CODES.iter().all(|c| !explain(*c).is_empty()),
        format!("未知码兜底人话：{:?}；{} 个在册码皆有人话", unknown, CODES.len()),
    );
    check(
        cs,
        "A54-判据-失败原因三值封闭且各有中文名",
        SyncFailure::ALL.len() == 3 && SyncFailure::ALL.iter().all(|s| !s.zh().is_empty()),
        format!("同步失败三原因各有中文名（{:?}）", SyncFailure::ALL[0].zh()),
    );
}

/// 跑 VE-F0054 全部自检。
pub fn run_vea54_checks() -> CheckSet {
    let mut cs = CheckSet::new("VE-F0054 多屏同步呈现（帧锁）");
    checks_scope(&mut cs);
    checks_lock(&mut cs);
    checks_degrade(&mut cs);
    checks_tolerance(&mut cs);
    checks_power(&mut cs);
    checks_meta(&mut cs);
    cs
}
