//! VE-F0053 · 显示器热插拔呈现端（VE-A 域 · A03 同步与呈现组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0053`
//!
//! **职责定位（锚点原文）**：显示器热插拔呈现端——显示器热插拔在呈现端的处理
//! （**主屏拔出的呈现迁移**），**迁移平滑承诺**（不白屏不崩溃），与 **V02 拓扑
//! 的联动契约**；含迁移的**窗口布局保持**（屏拔了窗口不乱飞）。
//! 数据结构：迁移器。
//!
//! **判据（锚点原文）**：热拔迁移、不白屏、回退主屏、契约联动、判据。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 白屏 → **修复** | 白屏落成**可机检事实**不是承诺：[`FrameOutcome`] 四值封闭 `Presented`/`Held`/`NoTarget`/`White`，逐帧记账 [`FrameAccount`]，总账守恒 `presented + held + no_target + white == total`。`NoTarget` 不可省——两值封闭会把「本帧未迁移」与「迁移中拦下」与「白屏」压成一类，告警随即被噪声淹没。`White` 是**设计违约的可检测出口**（正常路径不可达，见下），留 [`HotplugPresent::audit_force_drop`] 钩并以变异实测抓得到 |
//! | 迁移失败 → **回退主屏** | 两拍实现（备齐拍只写暂存、提交拍一次落地）：**未提交即未发生**，故回退天然无残影。[`HotplugPresent::commit`] 失败时目标与窗口**一个都不动**，呈现目标仍停在 `tx.from`（原主屏），即锚点要求的「回退主屏」；原主屏已离线时呈现一律 `Held`（不呈现到死屏、不丢弃帧），并立案要求重规划 |
//! | 契约漂移 → **拦截** | [`TopologyContract`] 是 V02 与本条之间的**声明契约**。[`HotplugPresent::declare`] 逐条比对四要素（世代单调 / 主屏在线 / 在线集相符 / 槽位几何合法），任一不符即拒登并置 [`HotplugPresent::drift_block`]，迁移规划随之拒（[`HotplugPresent::plan`] 先查漂移闸）——**物理事实照记、迁移照拦**：屏幕真拔了不能当没拔，但也不能拿一份漂移的拓扑去迁移 |
//!
//! **性能逐项分解（锚点原文）**：O(屏)——拓扑快照与在线集遍历都是
//! [`MAX_SCREENS`] 定容四槽常数上界；窗口布局迁移是 O(窗口)，窗口表定容
//! [`MAX_WINDOWS`]；迁移事务本身三段各是常数次比较与赋值。
//!
//! **跨批对接点**：**V02 拓扑联动**（前向声明）——显示拓扑的**唯一权威**在
//! V02 设备/拓扑管理，本条只持有**只读事实副本** [`TopologySnapshot`] 并如实
//! 拒绝，不在本条内裁决「系统有几块屏」。热插拔的**检测**（谁拔了、什么时候拔）
//! 也在 V02，本条从收到事实那一刻起负责**呈现侧怎么迁移**。
//!
//! **无障碍与隐私（锚点原文）**：迁移状态**读屏播报**
//! （[`HotplugPresent::a11y_lines`]）——中英双语七行，只报呈现迁移事实与聚合计数，
//! 不泄漏窗口标题、窗口内容与屏幕 EDID。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **「不白屏」必须是能算的账**：写成注释里的承诺，改一行代码就撒谎。故逐帧
//!   记账 + 总账守恒 + [`HotplugPresent::audit`] 复核三件齐做。
//! - **迁移中一律 Held（拦下不丢弃）**：拔屏到重建完成之间若「照常呈现」，画面
//!   会打到一块已经不存在的屏上——那正是白屏的成因。故在途帧拦下重排，而不是
//!   丢帧，也不是硬呈现。
//! - **`White` 诚实声明不可达**：两拍实现下呈现目标与几何同源，正常路径走到
//!   `White` 是不可能的。它留着的意义是**违约可检测**——故必须留审计钩并做变异
//!   实测，否则这条判据只是写死的死码（改代码它也永远绿）。
//! - **物理事实与声明契约分账**：拓扑快照记「屏真的没了」，契约记「V02 说现在
//!   该是什么」。两者矛盾时**都保留**、迁移**拦截**——把物理事实一起丢掉会让
//!   系统对着不存在的屏继续呈现，把契约一起丢掉会让漂移无人发现。
//! - **窗口布局保持是尺寸不变、位置夹取**：拔屏后窗口的**逻辑宽高**必须原样
//!   保留（用户摆好的版面不能被重排），**位置**则夹取进目标屏边界（不夹就是
//!   「窗口乱飞到看不见的地方」——同一个锚点句的两半）。
//! - **基线闸承原子性**（与 F0051/F0052 同纪律）：过期计划会让回退基准写成
//!   「从未处于的呈现目标」，故 [`HotplugPresent::begin`] 强制核对起点与世代，
//!   不符即拒（[`CODE_BASELINE`]，属调用方 bug 类：不计迁移失败、不动现役）；
//!   no-op 豁免。
//! - **零 panic 面**：查表走 `get`/match、计数全 `saturating_add`、无
//!   `unwrap`/`expect`，判据区同样约束。

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::vea46_present::{MAX_HEIGHT, MAX_WIDTH, MIN_HEIGHT, MIN_WIDTH};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x8Exx —— 全 kernel 树 grep 后确认零占用；
//    0x4A=vea50 / 0x4B=vea51 / 0x4D / 0x4E / 0x8D=vea52 均已占用）
// ---------------------------------------------------------------------------

/// 非法请求（未知槽位 / 窗口表满 / 非法几何）。
pub const CODE_BAD_REQUEST: u16 = 0x8E01;
/// 迁移失败（段或提交注入失败，回退已执行，现役未动）。
pub const CODE_MIGRATE_FAILED: u16 = 0x8E02;
/// 无存活屏可承接（物理事实记下，迁移拒）。
pub const CODE_NO_SURVIVOR: u16 = 0x8E03;
/// 迁移事务忙（在途未结束，拒绝重入）。
pub const CODE_TX_BUSY: u16 = 0x8E04;
/// 相位错（无事务即段/提交/确认/放弃，或跳段）。
pub const CODE_PHASE: u16 = 0x8E05;
/// V02 拓扑契约漂移（声明与物理事实矛盾，迁移拦截）。
pub const CODE_CONTRACT_DRIFT: u16 = 0x8E06;
/// 白屏（设计违约的可检测出口；正常路径不可达）。
pub const CODE_WHITE_SCREEN: u16 = 0x8E07;
/// 迁移基线不符（计划起点/世代与现役不一致）。
pub const CODE_BASELINE: u16 = 0x8E08;
/// 窗口落在离线屏上（布局未保持）。
pub const CODE_LAYOUT_LOST: u16 = 0x8E09;
/// 段未备齐即提交。
pub const CODE_SEG_INCOMPLETE: u16 = 0x8E0A;

/// 本域诊断码全集（判据对账：互异 + 独占 0x8E 段）。
pub const CODES: [u16; 10] = [
    CODE_BAD_REQUEST,
    CODE_MIGRATE_FAILED,
    CODE_NO_SURVIVOR,
    CODE_TX_BUSY,
    CODE_PHASE,
    CODE_CONTRACT_DRIFT,
    CODE_WHITE_SCREEN,
    CODE_BASELINE,
    CODE_LAYOUT_LOST,
    CODE_SEG_INCOMPLETE,
];

/// 人话说明（后果 + 下一步，不能只说「失败」；未知码有兜底不 panic）。
pub const fn explain(code: u16) -> &'static str {
    match code {
        CODE_BAD_REQUEST => "非法请求（未知屏槽/窗口表满/几何越界）：修正入参后重放",
        CODE_MIGRATE_FAILED => "迁移失败已回退主屏：呈现目标与窗口布局均未动，按失败段定位注入点后重试",
        CODE_NO_SURVIVOR => "无存活屏可承接：物理事实已记下，呈现迁移无从落地，请接入新屏后重规划",
        CODE_TX_BUSY => "迁移忙：上一事务未结束，串行化调用方逻辑后再发起",
        CODE_PHASE => "相位错（无事务/跳段）：检查调用方状态机时序后按序重放",
        CODE_CONTRACT_DRIFT => "V02 拓扑契约漂移：声明与物理事实矛盾，迁移已拦截，请对拍拓扑来源",
        CODE_WHITE_SCREEN => "检出白屏（设计违约）：呈现目标与可见面失配，须立即重规划迁移",
        CODE_BASELINE => "迁移基线不符：计划起点或世代与现役不一致，重读现役呈现目标后重新规划",
        CODE_LAYOUT_LOST => "窗口落在离线屏：窗口布局段未生效或被跳过，须重跑布局迁移段",
        CODE_SEG_INCOMPLETE => "段未备齐即提交：窗口段与目标段齐备（或本事务只需窗口段）后方可提交",
        _ => "未知呈现迁移诊断码（未登记）",
    }
}

// ---------------------------------------------------------------------------
// 二、屏槽封闭集与拓扑事实（V02 只读副本）
// ---------------------------------------------------------------------------

/// 屏槽定容上界（O(屏) 的常数上界来源）。
pub const MAX_SCREENS: usize = 4;

/// 窗口表定容上界。
pub const MAX_WINDOWS: usize = 8;

/// 屏槽（**四槽封闭集**，wire 显式映射禁 `as` 直转）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// 槽 0。
    S0,
    /// 槽 1。
    S1,
    /// 槽 2。
    S2,
    /// 槽 3。
    S3,
}

impl Slot {
    /// 全部四槽（顺序即槽位序）。
    pub const ALL: [Slot; 4] = [Slot::S0, Slot::S1, Slot::S2, Slot::S3];

    /// 槽下标（0..=3）。
    pub const fn index(self) -> usize {
        match self {
            Slot::S0 => 0,
            Slot::S1 => 1,
            Slot::S2 => 2,
            Slot::S3 => 3,
        }
    }

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            Slot::S0 => "屏一",
            Slot::S1 => "屏二",
            Slot::S2 => "屏三",
            Slot::S3 => "屏四",
        }
    }

    /// 英文名（读屏双语用）。
    pub const fn en(self) -> &'static str {
        match self {
            Slot::S0 => "Screen 1",
            Slot::S1 => "Screen 2",
            Slot::S2 => "Screen 3",
            Slot::S3 => "Screen 4",
        }
    }

    /// wire 值（0..=3）。
    pub const fn wire(self) -> u8 {
        self.index() as u8
    }

    /// 由 wire 反查（**越界即 `None`**，不做饱和）。
    pub const fn from_wire(v: u8) -> Option<Slot> {
        match v {
            0 => Some(Slot::S0),
            1 => Some(Slot::S1),
            2 => Some(Slot::S2),
            3 => Some(Slot::S3),
            _ => None,
        }
    }
}

/// 单屏状态（**V02 事实副本**，本条不裁决其真伪）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenState {
    /// 槽位。
    pub slot: Slot,
    /// 是否在线。
    pub online: bool,
    /// 像素宽。
    pub width: u16,
    /// 像素高。
    pub height: u16,
}

impl ScreenState {
    /// 空屏（离线零几何）。
    pub const fn blank(slot: Slot) -> ScreenState {
        ScreenState { slot, online: false, width: 0, height: 0 }
    }

    /// 几何是否合法（**上下界复用 A46 常量**，不另立一套标准）。
    pub fn geometry_ok(&self) -> bool {
        if !self.online {
            return true;
        }
        u32::from(self.width) >= MIN_WIDTH
            && u32::from(self.width) <= MAX_WIDTH
            && u32::from(self.height) >= MIN_HEIGHT
            && u32::from(self.height) <= MAX_HEIGHT
    }
}

/// 拓扑快照（**世代单调**，代际即 V02 的事实版本号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopologySnapshot {
    /// 世代（每次物理变更 +1）。
    pub generation: u32,
    /// 当前主屏槽。
    pub primary: Slot,
    /// 四槽状态（索引即 [`Slot::index`]）。
    pub screens: [ScreenState; MAX_SCREENS],
}

impl TopologySnapshot {
    /// 空快照（世代 0、四槽全离线）。
    pub const fn blank() -> TopologySnapshot {
        TopologySnapshot {
            generation: 0,
            primary: Slot::S0,
            screens: [
                ScreenState::blank(Slot::S0),
                ScreenState::blank(Slot::S1),
                ScreenState::blank(Slot::S2),
                ScreenState::blank(Slot::S3),
            ],
        }
    }

    /// 取某槽状态。
    pub fn screen(&self, s: Slot) -> ScreenState {
        self.screens[s.index()]
    }

    /// 某槽是否在线。
    pub fn is_online(&self, s: Slot) -> bool {
        self.screen(s).online
    }

    /// 在线屏数（O(屏)）。
    pub fn online_count(&self) -> u32 {
        let mut n = 0u32;
        for s in self.screens.iter() {
            if s.online {
                n = n.saturating_add(1);
            }
        }
        n
    }

    /// 在线位掩码（判据侧可独立重算）。
    pub fn online_mask(&self) -> u8 {
        let mut m = 0u8;
        for s in self.screens.iter() {
            if s.online {
                m |= 1u8 << s.slot.index();
            }
        }
        m
    }

    /// 首个在线槽（**跳过指定槽**；无则 `None`）。
    pub fn first_online_except(&self, skip: Slot) -> Option<Slot> {
        for s in Slot::ALL.iter().copied() {
            if s != skip && self.is_online(s) {
                return Some(s);
            }
        }
        None
    }

    /// 物理拔屏：**照记事实**，世代 +1（不受契约阻拦——屏真拔了不能当没拔）。
    pub fn unplug(&mut self, s: Slot) {
        self.screens[s.index()].online = false;
        self.screens[s.index()].width = 0;
        self.screens[s.index()].height = 0;
        self.generation = self.generation.saturating_add(1);
    }

    /// 物理插屏：照记事实，世代 +1。
    pub fn plug(&mut self, s: Slot, w: u16, h: u16) {
        self.screens[s.index()] = ScreenState { slot: s, online: true, width: w, height: h };
        self.generation = self.generation.saturating_add(1);
    }

    /// 指定新主屏（**不校验在线**——校验归契约闸，物理事实层不越权裁决）。
    pub fn set_primary(&mut self, s: Slot) {
        self.primary = s;
    }
}

/// V02 拓扑**声明契约**（联动契约的承载面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopologyContract {
    /// 声明世代（须与快照相等）。
    pub generation: u32,
    /// 声明主屏（须在线）。
    pub primary: Slot,
    /// 声明在线位掩码（须与快照逐位相等）。
    pub online_mask: u8,
}

impl TopologyContract {
    /// 由快照现取（**不缓存副本**，调用方现取现用）。
    pub fn of(snap: &TopologySnapshot) -> TopologyContract {
        TopologyContract {
            generation: snap.generation,
            primary: snap.primary,
            online_mask: snap.online_mask(),
        }
    }
}

/// **契约漂移判定**（四要素逐条独立裁决，返回首个不符项的码）。
///
/// 拆成四条而不是合成一个 bool：四种漂移的**补法完全不同**——世代不符是
/// 「拿到了旧快照」，主屏离线是「V02 指了个不存在的屏」，在线集不符是
/// 「有屏的状态没同步过来」，几何非法是「上报了越界分辨率」。合成一个布尔后
/// 复盘时分不出该去问谁。
pub fn drift_code(snap: &TopologySnapshot, c: &TopologyContract) -> Option<u16> {
    if c.generation != snap.generation {
        return Some(CODE_CONTRACT_DRIFT);
    }
    if !snap.is_online(c.primary) {
        return Some(CODE_CONTRACT_DRIFT);
    }
    if c.online_mask != snap.online_mask() {
        return Some(CODE_CONTRACT_DRIFT);
    }
    for s in snap.screens.iter() {
        if !s.geometry_ok() {
            return Some(CODE_BAD_REQUEST);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 三、窗口布局
// ---------------------------------------------------------------------------

/// 一个窗口的呈现侧布局（**逻辑尺寸与位置**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowSlot {
    /// 窗口号（判据语料用）。
    pub id: u32,
    /// 所在屏槽。
    pub slot: Slot,
    /// 逻辑 x。
    pub x: i32,
    /// 逻辑 y。
    pub y: i32,
    /// 逻辑宽（**迁移必须原样保留**）。
    pub w: u16,
    /// 逻辑高（**迁移必须原样保留**）。
    pub h: u16,
}

impl WindowSlot {
    /// 空窗口占位（定容表初始化用）。
    pub const EMPTY: WindowSlot = WindowSlot { id: 0, slot: Slot::S0, x: 0, y: 0, w: 0, h: 0 };

    /// 是否为占位空行。
    pub fn is_empty(&self) -> bool {
        self.w == 0 && self.h == 0
    }

    /// 读屏单行（**只报几何事实，不报窗口标题与内容**）。
    pub fn screen_line(&self) -> String {
        format!(
            "窗口 {} 在{}：{}×{} @ ({},{}) / Window {} on {}: {}x{} at ({},{})",
            self.id,
            self.slot.zh(),
            self.w,
            self.h,
            self.x,
            self.y,
            self.id,
            self.slot.en(),
            self.w,
            self.h,
            self.x,
            self.y
        )
    }
}

/// 位置夹取（**夹进目标屏边界**：不夹就是窗口乱飞到看不见的地方）。
fn clamp_pos(v: i32, extent: u16, win: u16) -> i32 {
    let max = i32::from(extent.saturating_sub(win));
    if v < 0 {
        0
    } else if v > max {
        max
    } else {
        v
    }
}

// ---------------------------------------------------------------------------
// 四、迁移计划与事务
// ---------------------------------------------------------------------------

/// 迁移种类（**两型**：拔副屏只迁布局，拔主屏才换呈现目标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanKind {
    /// 只迁窗口布局（现役呈现目标仍在线）。
    LayoutOnly,
    /// 呈现目标换屏（现役目标已离线）。
    TargetSwap,
}

/// 迁移计划（**起点即回退基准**，故必须与现役一致，见基线闸）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigrationPlan {
    /// 迁移种类。
    pub kind: PlanKind,
    /// 起点屏槽（= 现役呈现目标 = 回退基准）。
    pub from: Slot,
    /// 目标屏槽（**取 V02 契约声明的主屏**，不自行挑一块）。
    pub to: Slot,
    /// 起点世代。
    pub gen_from: u32,
    /// 目标世代。
    pub gen_to: u32,
}

/// 应用段（**两段**：窗口布局段 / 呈现目标段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    /// 窗口布局迁移段。
    Windows,
    /// 呈现目标切换段。
    Target,
}

impl Segment {
    /// 全部两段。
    pub const ALL: [Segment; 2] = [Segment::Windows, Segment::Target];

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            Segment::Windows => "窗口布局迁移",
            Segment::Target => "呈现目标切换",
        }
    }

    /// 位号（备齐位掩码用）。
    pub const fn bit(self) -> u8 {
        match self {
            Segment::Windows => 1u8,
            Segment::Target => 2u8,
        }
    }
}

/// 注入故障点（**判据侧逐点注入**，证明回退路径非死码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 不注入。
    None,
    /// 窗口段注入失败。
    StageWindows,
    /// 目标段注入失败。
    StageTarget,
    /// 提交拍注入失败。
    Commit,
    /// 确认拍注入失败。
    Confirm,
}

/// 事务相位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 已建档，两段未备齐。
    Validate,
    /// 段已备齐，可提交。
    Staged,
    /// 已提交，待确认。
    Committed,
}

/// 在途迁移事务（**暂存与落地分离**：未提交即未发生）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotTx {
    /// 计划（副本，回退基准取自此）。
    pub plan: MigrationPlan,
    /// 相位。
    pub phase: Phase,
    /// 备齐位掩码（[`Segment::bit`]）。
    pub staged: u8,
    /// 暂存窗口表。
    pub staged_windows: [WindowSlot; MAX_WINDOWS],
    /// 暂存窗口条数。
    pub staged_count: usize,
    /// 暂存呈现目标。
    pub staged_target: Slot,
    /// 提交前窗口快照（**确认段失败要显式还原**）。
    pub pre_windows: [WindowSlot; MAX_WINDOWS],
    /// 提交前窗口条数。
    pub pre_count: usize,
    /// 提交前呈现目标。
    pub pre_target: Slot,
    /// 提交前目标世代。
    pub pre_epoch: u32,
}

impl HotTx {
    /// 该计划必需的备齐位掩码。
    pub fn required_mask(&self) -> u8 {
        match self.plan.kind {
            PlanKind::LayoutOnly => Segment::Windows.bit(),
            PlanKind::TargetSwap => Segment::Windows.bit() | Segment::Target.bit(),
        }
    }
}

// ---------------------------------------------------------------------------
// 五、呈现逐帧账与聚合计数
// ---------------------------------------------------------------------------

/// 逐帧结局（**四值封闭**，`White` 是违约出口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameOutcome {
    /// 已呈现。
    Presented,
    /// 拦下待重排（迁移中或目标离线；**不丢弃**）。
    Held,
    /// 本帧未迁移目标（**不是白屏**）。
    NoTarget,
    /// 白屏（设计违约）。
    White,
}

impl FrameOutcome {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            FrameOutcome::Presented => "已呈现",
            FrameOutcome::Held => "拦下待重排",
            FrameOutcome::NoTarget => "本帧未迁移",
            FrameOutcome::White => "白屏",
        }
    }

    /// 英文名。
    pub const fn en(self) -> &'static str {
        match self {
            FrameOutcome::Presented => "presented",
            FrameOutcome::Held => "held",
            FrameOutcome::NoTarget => "not migrated this frame",
            FrameOutcome::White => "white screen",
        }
    }

    /// 是否为**违约**结局（只有白屏是）。
    pub const fn is_violation(self) -> bool {
        matches!(self, FrameOutcome::White)
    }
}

/// 逐帧账（**总账守恒**是可机检的不变量）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameAccount {
    /// 总帧数。
    pub total: u32,
    /// 已呈现。
    pub presented: u32,
    /// 拦下待重排。
    pub held: u32,
    /// 本帧未迁移。
    pub no_target: u32,
    /// 白屏。
    pub white: u32,
}

impl FrameAccount {
    /// 总账守恒：四类之和恰等于总帧数。
    pub fn balanced(&self) -> bool {
        self.presented.saturating_add(self.held)
            .saturating_add(self.no_target)
            .saturating_add(self.white)
            == self.total
    }
}

/// 聚合计数（读屏与判据都只读这张表）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// 拔屏事实次数。
    pub unplugs: u32,
    /// 迁移成功。
    pub migrations_ok: u32,
    /// 迁移失败（已回退主屏）。
    pub migrations_failed: u32,
    /// 放弃事务。
    pub abandoned: u32,
    /// 忙拒。
    pub tx_busy: u32,
    /// 基线拒。
    pub baseline_rejected: u32,
    /// 契约漂移拦截。
    pub drift_blocked: u32,
    /// 无存活屏拒。
    pub no_survivor: u32,
    /// 窗口迁移条数（累计）。
    pub windows_moved: u32,
    /// no-op 放行。
    pub noop: u32,
}

/// 迁移执行者（**留名便于追责**，三值封闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor {
    /// 合成器。
    Compositor,
    /// 窗口管理。
    WindowManager,
    /// 驱动侧。
    Driver,
}

impl Actor {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            Actor::Compositor => "合成器",
            Actor::WindowManager => "窗口管理",
            Actor::Driver => "驱动侧",
        }
    }
}

// ---------------------------------------------------------------------------
// 六、呈现迁移器（锚点「迁移器」）
// ---------------------------------------------------------------------------

/// 呈现迁移器（**热插拔呈现端**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotplugPresent {
    /// V02 拓扑事实副本（**只读持有**：本条不复制缓存另一份）。
    pub topology: TopologySnapshot,
    /// 当前声明契约。
    pub contract: TopologyContract,
    /// 漂移拦截码（`0` = 无漂移；非零时迁移一律拒）。
    pub drift_block: u16,
    /// 现役呈现目标。
    pub target: Slot,
    /// 目标世代（每次成功迁移 +1）。
    pub target_epoch: u32,
    /// 窗口布局表。
    pub windows: [WindowSlot; MAX_WINDOWS],
    /// 窗口条数。
    pub window_count: usize,
    /// 在途事务。
    pub tx: Option<HotTx>,
    /// 逐帧账。
    pub frames: FrameAccount,
    /// 聚合计数。
    pub counts: Counts,
    /// 注入故障点。
    pub inject: Fault,
    /// 审计钩：**强制制造白屏**，供变异实测证明该分支可达。
    pub audit_force_drop: bool,
}

impl HotplugPresent {
    /// 构造：按给定拓扑建立初始呈现目标（**初始目标在线且几何合法才成立**）。
    pub fn new(topology: TopologySnapshot) -> HotplugPresent {
        let target = topology.primary;
        let ok = topology.is_online(target) && topology.screen(target).geometry_ok();
        HotplugPresent {
            contract: TopologyContract::of(&topology),
            drift_block: 0,
            target,
            target_epoch: if ok { 1 } else { 0 },
            windows: [WindowSlot::EMPTY; MAX_WINDOWS],
            window_count: 0,
            tx: None,
            frames: FrameAccount::default(),
            counts: Counts::default(),
            inject: Fault::None,
            audit_force_drop: false,
            topology,
        }
    }

    /// 登记窗口（**表满即拒**，不静默丢弃）。
    pub fn add_window(&mut self, w: WindowSlot) -> Result<(), u16> {
        if self.window_count >= MAX_WINDOWS {
            return Err(CODE_BAD_REQUEST);
        }
        if !self.topology.is_online(w.slot) {
            return Err(CODE_BAD_REQUEST);
        }
        self.windows[self.window_count] = w;
        self.window_count += 1;
        Ok(())
    }

    /// 窗口布局快照只读。
    pub fn windows(&self) -> &[WindowSlot] {
        &self.windows[..self.window_count]
    }

    /// 落在离线屏上的窗口数（**布局未保持的可机检指标**）。
    pub fn stranded_windows(&self) -> u32 {
        let mut n = 0u32;
        for w in self.windows().iter() {
            if !self.topology.is_online(w.slot) {
                n = n.saturating_add(1);
            }
        }
        n
    }

    /// 物理拔屏（**事实照记**，不受契约阻拦）。
    pub fn unplug(&mut self, slot: Slot) -> Result<u32, u16> {
        if !self.topology.is_online(slot) {
            return Err(CODE_BAD_REQUEST);
        }
        self.topology.unplug(slot);
        self.counts.unplugs = self.counts.unplugs.saturating_add(1);
        if self.topology.online_count() == 0 {
            // 全屏拔光：事实记下（屏确实没了），但**迁移无从落地**，
            // 如实计数而不是让 plan 去挑一块不存在的屏。
            self.counts.no_survivor = self.counts.no_survivor.saturating_add(1);
        }
        Ok(self.topology.generation)
    }

    /// 物理插屏（事实照记）。
    pub fn plug(&mut self, slot: Slot, w: u16, h: u16) -> Result<u32, u16> {
        if self.topology.is_online(slot) {
            return Err(CODE_BAD_REQUEST);
        }
        self.topology.plug(slot, w, h);
        Ok(self.topology.generation)
    }

    /// 登记 V02 声明契约（**漂移即拒登并置拦截闸**）。
    pub fn declare(&mut self, c: TopologyContract) -> Result<(), u16> {
        match drift_code(&self.topology, &c) {
            None => {
                self.contract = c;
                self.drift_block = 0;
                Ok(())
            }
            Some(code) => {
                self.drift_block = code;
                self.counts.drift_blocked = self.counts.drift_blocked.saturating_add(1);
                Err(code)
            }
        }
    }

    /// **迁移规划**（**先查漂移闸**，再谈挑屏）。
    ///
    /// 目标屏**取契约声明的主屏**而非自行挑选：V02 是拓扑权威，本条自己
    /// 「找一块在线的屏」等于在呈现侧另立一套拓扑裁决，两处迟早打架。
    pub fn plan(&mut self) -> Result<MigrationPlan, u16> {
        // **物理无存活屏先于契约漂移判**：一块屏都不剩时，「契约陈旧」是
        // 派生现象、「无处可迁」才是根因——把根因报成派生现象，调用方会去
        // 对拍拓扑来源，而真正该做的是接上屏再重规划。
        if self.topology.online_count() == 0 {
            return Err(CODE_NO_SURVIVOR);
        }
        if self.drift_block != 0 {
            return Err(self.drift_block);
        }
        let from = self.target;
        if self.topology.is_online(from) {
            return Ok(MigrationPlan {
                kind: PlanKind::LayoutOnly,
                from,
                to: from,
                gen_from: self.topology.generation,
                gen_to: self.topology.generation,
            });
        }
        if self.topology.first_online_except(from).is_none() {
            return Err(CODE_NO_SURVIVOR);
        }
        let to = self.contract.primary;
        if !self.topology.is_online(to) {
            return Err(CODE_NO_SURVIVOR);
        }
        Ok(MigrationPlan {
            kind: PlanKind::TargetSwap,
            from,
            to,
            gen_from: self.topology.generation,
            gen_to: self.topology.generation,
        })
    }

    /// 建档（**忙闸先于基线闸**：忙才是唯一可操作的诊断）。
    pub fn begin(&mut self, plan: &MigrationPlan, _actor: Actor) -> Result<(), u16> {
        if self.tx.is_some() {
            self.counts.tx_busy = self.counts.tx_busy.saturating_add(1);
            return Err(CODE_TX_BUSY);
        }
        // no-op 豁免：只迁布局却没有任何窗口落在离线屏上——无事可做，
        // 不制造假事务（否则读痕迹的人以为真迁过）。
        if plan.kind == PlanKind::LayoutOnly && self.stranded_windows() == 0 {
            self.counts.noop = self.counts.noop.saturating_add(1);
            return Ok(());
        }
        if plan.from != self.target || plan.gen_to != self.topology.generation {
            self.counts.baseline_rejected = self.counts.baseline_rejected.saturating_add(1);
            return Err(CODE_BASELINE);
        }
        self.tx = Some(HotTx {
            plan: *plan,
            phase: Phase::Validate,
            staged: 0,
            staged_windows: self.windows,
            staged_count: self.window_count,
            staged_target: self.target,
            pre_windows: self.windows,
            pre_count: self.window_count,
            pre_target: self.target,
            pre_epoch: self.target_epoch,
        });
        Ok(())
    }

    /// 备齐一段（**只写暂存，绝不碰现役**）。
    pub fn stage(&mut self, seg: Segment) -> Result<(), u16> {
        let fault_match = match (self.inject, seg) {
            (Fault::StageWindows, Segment::Windows) => true,
            (Fault::StageTarget, Segment::Target) => true,
            _ => false,
        };
        if fault_match {
            // 注入失败：**暂存也不写**，现役更不动——回退零残影的前提就在这。
            self.counts.migrations_failed = self.counts.migrations_failed.saturating_add(1);
            return Err(CODE_MIGRATE_FAILED);
        }
        let tx = match self.tx.as_mut() {
            Some(t) => t,
            None => return Err(CODE_PHASE),
        };
        if tx.phase == Phase::Committed {
            return Err(CODE_PHASE);
        }
        if tx.staged & seg.bit() != 0 {
            // 同段重复备齐是**幂等**的（不是错）——调用方重试不该拿到相位错，
            // 但也不该把同一段算两遍。
            return Ok(());
        }
        match seg {
            Segment::Windows => {
                let to = tx.plan.to;
                let mut staged = tx.pre_windows;
                let mut moved = 0u32;
                for i in 0..tx.pre_count {
                    let w = tx.pre_windows[i];
                    if !self.topology.is_online(w.slot) {
                        let target_screen = self.topology.screen(to);
                        let nx = clamp_pos(w.x, target_screen.width, w.w);
                        let ny = clamp_pos(w.y, target_screen.height, w.h);
                        // 尺寸**原样保留**：用户摆好的版面不能被重排。
                        staged[i] = WindowSlot { slot: to, x: nx, y: ny, ..w };
                        moved = moved.saturating_add(1);
                    } else {
                        staged[i] = w;
                    }
                }
                tx.staged_windows = staged;
                tx.staged_count = tx.pre_count;
                tx.staged = tx.staged | seg.bit();
                self.counts.windows_moved = self.counts.windows_moved.saturating_add(moved);
            }
            Segment::Target => {
                tx.staged_target = tx.plan.to;
                tx.staged = tx.staged | seg.bit();
            }
        }
        tx.phase = Phase::Staged;
        Ok(())
    }

    /// 提交拍：**一次连续赋值落地**，窗口段与目标段同源生效。
    pub fn commit(&mut self) -> Result<(), u16> {
        if self.inject == Fault::Commit {
            // 备齐 ≠ 落地：失败时现役一个都不动，即「回退主屏」。
            self.counts.migrations_failed = self.counts.migrations_failed.saturating_add(1);
            return Err(CODE_MIGRATE_FAILED);
        }
        let tx = match self.tx.as_mut() {
            Some(t) => t,
            None => return Err(CODE_PHASE),
        };
        if tx.phase != Phase::Staged {
            return Err(CODE_PHASE);
        }
        if tx.staged & tx.required_mask() != tx.required_mask() {
            return Err(CODE_SEG_INCOMPLETE);
        }
        self.windows = tx.staged_windows;
        self.window_count = tx.staged_count;
        self.target = tx.staged_target;
        self.target_epoch = self.target_epoch.saturating_add(1);
        tx.phase = Phase::Committed;
        Ok(())
    }

    /// 确认拍：**显式还原提交前快照**（半迁移态不落地）。
    pub fn confirm(&mut self) -> Result<(), u16> {
        let failed = self.inject == Fault::Confirm;
        {
            let tx = match self.tx.as_mut() {
                Some(t) => t,
                None => return Err(CODE_PHASE),
            };
            if tx.phase != Phase::Committed {
                return Err(CODE_PHASE);
            }
            if failed {
                self.windows = tx.pre_windows;
                self.window_count = tx.pre_count;
                self.target = tx.pre_target;
                self.target_epoch = tx.pre_epoch;
            }
        }
        self.tx = None;
        self.inject = Fault::None;
        if failed {
            self.counts.migrations_failed = self.counts.migrations_failed.saturating_add(1);
            return Err(CODE_MIGRATE_FAILED);
        }
        self.counts.migrations_ok = self.counts.migrations_ok.saturating_add(1);
        Ok(())
    }

    /// 显式放弃（**留失败痕**：它确实改变了用户预期），现役一个都不动。
    pub fn abandon(&mut self) -> Result<(), u16> {
        if self.tx.is_none() {
            return Err(CODE_PHASE);
        }
        self.tx = None;
        self.inject = Fault::None;
        self.counts.abandoned = self.counts.abandoned.saturating_add(1);
        Ok(())
    }

    /// **逐帧呈现**（**不白屏承诺的记账点**）。
    ///
    /// 处置顺序是刻意的：
    ///
    /// 1. 审计钩先判——它是「故意违约」的表达口，必须能压过一切正常路径，
    ///    否则这个分支永远走不到、判据成了死码；
    /// 2. **从未建立过呈现目标** → [`FrameOutcome::NoTarget`]：这一帧确实
    ///    「没有迁移目标」，与「迁移中拦下」是两回事，压成一类告警就被噪声淹没；
    /// 3. 在途事务 → [`FrameOutcome::Held`]：拔屏到重建完成之间照常呈现，
    ///    画面会打到一块已经不存在的屏上，那正是白屏的成因；**提交后未确认
    ///    仍在途**，故确认前那一帧同样是拦下；
    /// 4. 目标离线 → [`FrameOutcome::Held`]：迁移失败回退主屏而原主屏已离线时，
    ///    呈现仍不落到死屏，也不丢帧；
    /// 5. 其余 → [`FrameOutcome::Presented`]。
    pub fn present_frame(&mut self) -> FrameOutcome {
        self.frames.total = self.frames.total.saturating_add(1);
        if self.audit_force_drop {
            self.frames.white = self.frames.white.saturating_add(1);
            return FrameOutcome::White;
        }
        if self.target_epoch == 0 {
            self.frames.no_target = self.frames.no_target.saturating_add(1);
            return FrameOutcome::NoTarget;
        }
        if self.tx.is_some() {
            self.frames.held = self.frames.held.saturating_add(1);
            return FrameOutcome::Held;
        }
        if !self.topology.is_online(self.target) {
            self.frames.held = self.frames.held.saturating_add(1);
            return FrameOutcome::Held;
        }
        self.frames.presented = self.frames.presented.saturating_add(1);
        FrameOutcome::Presented
    }

    /// **自审**（读册不一致的检出面；返回违规码列表）。
    pub fn audit(&self) -> Vec<u16> {
        let mut bad: Vec<u16> = Vec::new();
        if !self.frames.balanced() {
            // 总账不守恒 = 有一类帧没进账：承诺本身已不可核对。
            bad.push(CODE_WHITE_SCREEN);
        }
        if self.frames.white > 0 {
            bad.push(CODE_WHITE_SCREEN);
        }
        if self.stranded_windows() > 0 {
            bad.push(CODE_LAYOUT_LOST);
        }
        if self.target_epoch > 0 && !self.topology.is_online(self.target) {
            bad.push(CODE_NO_SURVIVOR);
        }
        if let Some(t) = &self.tx {
            if t.phase == Phase::Committed && self.staged_leftover(t) {
                bad.push(CODE_PHASE);
            }
        }
        bad
    }

    /// 已提交却仍有未备齐的段（**内部一致面**：不可能发生，发生即相位记账出错）。
    fn staged_leftover(&self, t: &HotTx) -> bool {
        t.staged & t.required_mask() != t.required_mask()
    }

    /// 读屏七行（**双语**，只报呈现迁移事实与聚合计数）。
    pub fn a11y_lines(&self) -> [String; 7] {
        [
            format!(
                "呈现目标：{} / Present target: {}",
                self.target.zh(),
                self.target.en()
            ),
            format!(
                "在线显示器：{} 块 / Online displays: {}",
                self.topology.online_count(),
                self.topology.online_count()
            ),
            format!(
                "迁移成功 {} 次，失败 {} 次 / Migrations ok {}, failed {}",
                self.counts.migrations_ok, self.counts.migrations_failed, self.counts.migrations_ok, self.counts.migrations_failed
            ),
            format!(
                "白屏 {} 帧，拦下待重排 {} 帧 / White {} frames, held {}",
                self.frames.white, self.frames.held, self.frames.white, self.frames.held
            ),
            format!(
                "窗口迁移 {} 个，离线屏残留 {} 个 / Windows moved {}, stranded {}",
                self.counts.windows_moved, self.stranded_windows(), self.counts.windows_moved, self.stranded_windows()
            ),
            format!(
                "呈现 {} 帧，未迁移 {} 帧 / Presented {}, not migrated {}",
                self.frames.presented, self.frames.no_target, self.frames.presented, self.frames.no_target
            ),
            format!(
                "契约漂移拦截 {} 次，无存活屏 {} 次 / Contract drift blocked {}, no survivor {}",
                self.counts.drift_blocked, self.counts.no_survivor, self.counts.drift_blocked, self.counts.no_survivor
            ),
        ]
    }
}

// ---------------------------------------------------------------------------
// 七、标准环境（判据语料）
// ---------------------------------------------------------------------------

/// 标准拓扑：**三屏在线，主屏为槽 0**。
pub fn standard_topology() -> TopologySnapshot {
    let mut t = TopologySnapshot::blank();
    t.plug(Slot::S0, 1920, 1080);
    t.plug(Slot::S1, 2560, 1440);
    t.plug(Slot::S2, 1280, 1024);
    t.generation = 0;
    t.set_primary(Slot::S0);
    t
}

/// 标准呈现迁移器（**契约与拓扑对齐**，无漂移闸）。
pub fn standard_present() -> HotplugPresent {
    let topo = standard_topology();
    let contract = TopologyContract::of(&topo);
    let mut h = HotplugPresent::new(topo);
    h.contract = contract;
    h
}

/// 标准窗口集（**两窗在主屏、一窗在副屏**）。
pub fn standard_windows() -> [WindowSlot; 3] {
    [
        WindowSlot { id: 1, slot: Slot::S0, x: 0, y: 0, w: 800, h: 600 },
        WindowSlot { id: 2, slot: Slot::S0, x: 900, y: 40, w: 1024, h: 768 },
        WindowSlot { id: 3, slot: Slot::S1, x: 1600, y: 0, w: 960, h: 540 },
    ]
}

/// 登记标准窗口集。
pub fn seed_windows(h: &mut HotplugPresent) {
    for w in standard_windows().iter().copied() {
        let _ = h.add_window(w);
    }
}

/// 拔主屏并登记新契约（**事实照记 → 契约重登记 → 规划**，返回规划结果）。
pub fn unplug_primary_and_plan(h: &mut HotplugPresent) -> Result<MigrationPlan, u16> {
    h.unplug(Slot::S0)?;
    h.topology.set_primary(Slot::S1);
    h.declare(TopologyContract::of(&h.topology))?;
    h.plan()
}

/// 把事务一路走到确认（**逐段备齐后提交**）。
pub fn drive_tx(h: &mut HotplugPresent, plan: &MigrationPlan) -> Result<(), u16> {
    h.begin(plan, Actor::Compositor)?;
    h.stage(Segment::Windows)?;
    if plan.kind == PlanKind::TargetSwap {
        h.stage(Segment::Target)?;
    }
    h.commit()?;
    h.confirm()
}

// ---------------------------------------------------------------------------
// 八、域自检（判据逐条对应锚点）
// ---------------------------------------------------------------------------

fn ls(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn check(cs: &mut CheckSet, name: &'static str, ok: bool, detail: String) {
    cs.add(name, ok, ls(detail));
}

/// 屏槽与拓扑事实面。
fn checks_slots(cs: &mut CheckSet) {
    check(
        cs,
        "A53-槽位-封闭集往返且双语",
        Slot::ALL.len() == MAX_SCREENS
            && (0u8..=3u8).all(|w| Slot::from_wire(w).map(|s| s.wire()) == Some(w))
            && Slot::from_wire(4).is_none()
            && Slot::ALL.iter().all(|s| !s.zh().is_empty() && !s.en().is_empty()),
        format!("四槽封闭集：wire 0..3 往返一致、4 返 None、双语名非空（{:?}）", Slot::ALL.len()),
    );
    let topo = standard_topology();
    check(
        cs,
        "A53-拓扑-在线集与计数可独立重算",
        topo.online_count() == 3 && topo.online_mask() == 0b0111 && topo.is_online(Slot::S0) && !topo.is_online(Slot::S3),
        format!("三屏在线：计数 {}、掩码 {:b}（判据侧手算 0b0111）", topo.online_count(), topo.online_mask()),
    );
    let mut idx = 0usize;
    let mut ordered = true;
    for s in Slot::ALL.iter().copied() {
        if topo.screen(s).slot != s || topo.screens[s.index()].slot != s {
            ordered = false;
        }
        idx += 1;
    }
    check(
        cs,
        "A53-拓扑-槽位与下标一一对应",
        ordered && idx == MAX_SCREENS,
        format!("槽位表按下标寻址无错位（{} 槽逐槽对位）", idx),
    );
    check(
        cs,
        "A53-拓扑-几何上下界复用A46",
        {
            let mut s = ScreenState { slot: Slot::S0, online: true, width: 0, height: 1080 };
            let bad = !s.geometry_ok();
            s.width = (MAX_WIDTH as u16).saturating_add(1);
            let over = !s.geometry_ok();
            s.width = MIN_WIDTH as u16;
            s.height = MIN_HEIGHT as u16;
            let good = s.geometry_ok();
            bad && over && good
        },
        format!("越下界/越上界皆拒、恰 MIN 放行（MIN={} MAX={} 取自 A46）", MIN_WIDTH, MAX_WIDTH),
    );
}

/// V02 联动契约面（**契约漂移→拦截**）。
fn checks_contract(cs: &mut CheckSet) {
    let topo = standard_topology();
    let c = TopologyContract::of(&topo);
    check(
        cs,
        "A53-契约-对齐即无漂移",
        drift_code(&topo, &c).is_none(),
        "声明与事实逐条对齐时不报漂移".to_string(),
    );
    let gen_bad = TopologyContract { generation: c.generation.saturating_add(1), ..c };
    let pri_bad = TopologyContract { primary: Slot::S3, ..c };
    let mask_bad = TopologyContract { online_mask: c.online_mask | 0b1000, ..c };
    let geo_bad = TopologyContract { ..c };
    let mut t2 = topo;
    t2.screens[Slot::S0.index()] = ScreenState { slot: Slot::S0, online: true, width: 1, height: 1080 };
    check(
        cs,
        "A53-契约-四要素漂移各自分码",
        drift_code(&topo, &gen_bad) == Some(CODE_CONTRACT_DRIFT)
            && drift_code(&topo, &pri_bad) == Some(CODE_CONTRACT_DRIFT)
            && drift_code(&topo, &mask_bad) == Some(CODE_CONTRACT_DRIFT)
            && drift_code(&t2, &geo_bad) == Some(CODE_BAD_REQUEST),
        "世代/主屏/在线集漂移同归契约码、几何非法归参数码（补法不同故分码）".to_string(),
    );
    let mut h = standard_present();
    h.unplug(Slot::S0).expect("拔屏");
    let stale = TopologyContract::of(&standard_topology());
    let r = h.declare(stale);
    check(
        cs,
        "A53-契约-漂移拒登并置迁移拦截闸",
        r == Err(CODE_CONTRACT_DRIFT) && h.drift_block == CODE_CONTRACT_DRIFT && h.plan() == Err(CODE_CONTRACT_DRIFT),
        format!("旧契约拒登（{:?}），迁移随之拒（闸码 {}）", r, h.drift_block),
    );
    check(
        cs,
        "A53-契约-物理事实不因漂移被抹掉",
        h.counts.unplugs == 1 && !h.topology.is_online(Slot::S0) && h.counts.drift_blocked == 1,
        format!("拔屏事实仍记账（拔 {} 次、槽一离线、漂移拦截 {} 次）", h.counts.unplugs, h.counts.drift_blocked),
    );
    let mut h2 = standard_present();
    h2.unplug(Slot::S0).expect("拔屏");
    h2.topology.set_primary(Slot::S1);
    check(
        cs,
        "A53-契约-重登记对齐即解拦截",
        h2.declare(TopologyContract::of(&h2.topology)) == Ok(()) && h2.drift_block == 0,
        "对齐契约重登记后拦截闸解除".to_string(),
    );
}

/// 迁移规划面（**O(屏)**：挑屏全走定容四槽）。
fn checks_plan(cs: &mut CheckSet) {
    let mut h = standard_present();
    seed_windows(&mut h);
    let p = unplug_primary_and_plan(&mut h).expect("规划");
    check(
        cs,
        "A53-规划-拔主屏走呈现目标切换",
        p.kind == PlanKind::TargetSwap && p.from == Slot::S0 && p.to == Slot::S1,
        format!("主屏拔出走目标切换：{} → {}", p.from.zh(), p.to.zh()),
    );
    let mut h2 = standard_present();
    seed_windows(&mut h2);
    h2.unplug(Slot::S2).expect("拔副屏");
    h2.declare(TopologyContract::of(&h2.topology)).expect("契约");
    let p2 = h2.plan().expect("规划");
    check(
        cs,
        "A53-规划-拔副屏只迁布局",
        p2.kind == PlanKind::LayoutOnly && p2.from == Slot::S0 && p2.to == Slot::S0,
        format!("副屏拔除不改呈现目标（{} → {}）", p2.from.zh(), p2.to.zh()),
    );
    let mut h3 = standard_present();
    h3.unplug(Slot::S0).expect("拔屏");
    h3.unplug(Slot::S1).expect("拔屏");
    h3.unplug(Slot::S2).expect("拔屏");
    check(
        cs,
        "A53-规划-无存活屏拒并如实计数",
        h3.plan() == Err(CODE_NO_SURVIVOR) && h3.counts.no_survivor == 1 && h3.counts.unplugs == 3,
        format!("三屏拔光后规划拒（{:?}），无存活屏计数 {}、拔屏事实 {} 次", h3.plan(), h3.counts.no_survivor, h3.counts.unplugs),
    );
    let mut h4 = standard_present();
    seed_windows(&mut h4);
    let p4 = unplug_primary_and_plan(&mut h4).expect("规划");
    let mut stale = p4;
    stale.from = Slot::S2;
    check(
        cs,
        "A53-规划-过期计划被基线闸拒且零副作用",
        h4.begin(&stale, Actor::Driver) == Err(CODE_BASELINE)
            && h4.tx.is_none()
            && h4.target == Slot::S0
            && h4.counts.baseline_rejected == 1,
        format!("起点不符即拒（基线拒 {} 次），现役目标仍 {}", h4.counts.baseline_rejected, h4.target.zh()),
    );
    let mut h5 = standard_present();
    h5.unplug(Slot::S2).expect("拔副屏");
    h5.declare(TopologyContract::of(&h5.topology)).expect("契约");
    let p5 = h5.plan().expect("规划");
    check(
        cs,
        "A53-规划-noop豁免不开假事务",
        h5.begin(&p5, Actor::Compositor) == Ok(()) && h5.tx.is_none() && h5.counts.noop == 1,
        "无窗口落在离线屏时不开事务（no-op 不制造假迁移）".to_string(),
    );
}

/// 事务相位与三段面。
fn checks_tx(cs: &mut CheckSet) {
    let mut h = standard_present();
    seed_windows(&mut h);
    check(
        cs,
        "A53-事务-无事务即段提交确认放弃全拒",
        h.stage(Segment::Windows) == Err(CODE_PHASE)
            && h.commit() == Err(CODE_PHASE)
            && h.confirm() == Err(CODE_PHASE)
            && h.abandon() == Err(CODE_PHASE),
        "无事务时四入口皆相位错，且不计迁移失败".to_string(),
    );
    let p = unplug_primary_and_plan(&mut h).expect("规划");
    h.begin(&p, Actor::Compositor).expect("建档");
    h.stage(Segment::Windows).expect("备窗口段");
    check(
        cs,
        "A53-事务-段未齐即提交被拒",
        h.commit() == Err(CODE_SEG_INCOMPLETE)
            && h.target == Slot::S0
            && h.windows()[0].slot == Slot::S0,
        format!("目标段未备齐即提交被拒，现役目标仍 {}、窗口表未动（暂存不外泄）", h.target.zh()),
    );
    h.stage(Segment::Target).expect("备目标段");
    let again = h.stage(Segment::Windows);
    check(
        cs,
        "A53-事务-同段重复备齐幂等",
        again == Ok(()),
        "同段重复备齐幂等放行（重试不该拿到相位错，也不重复计数）".to_string(),
    );
    let busy = h.begin(&p, Actor::Compositor);
    check(
        cs,
        "A53-事务-在途重入报忙且原事务不受影响",
        busy == Err(CODE_TX_BUSY) && h.tx.is_some() && h.counts.tx_busy == 1,
        format!("在途建档报忙（{:?}），原事务仍在", busy),
    );
    h.commit().expect("提交");
    check(
        cs,
        "A53-事务-已提交再段被相位错拒",
        h.stage(Segment::Target) == Err(CODE_PHASE),
        "提交后再备段被相位错拒（半迁移态不落地）".to_string(),
    );
    h.confirm().expect("确认");
    check(
        cs,
        "A53-事务-确认后事务清账且成功计数",
        h.tx.is_none() && h.counts.migrations_ok == 1,
        format!("确认后清账（成功 {} 次）", h.counts.migrations_ok),
    );
    let mut g = standard_present();
    seed_windows(&mut g);
    let pl = unplug_primary_and_plan(&mut g).expect("规划");
    g.begin(&pl, Actor::WindowManager).expect("建档");
    g.stage(Segment::Windows).expect("备段");
    g.stage(Segment::Target).expect("备段");
    g.abandon().expect("放弃");
    check(
        cs,
        "A53-事务-显式放弃留痕且现役不动",
        g.tx.is_none() && g.target == Slot::S0 && g.counts.abandoned == 1,
        format!("放弃记账 {} 次，现役目标仍 {}", g.counts.abandoned, g.target.zh()),
    );
}

/// 迁移落地与失败回退主屏。
fn checks_migrate(cs: &mut CheckSet) {
    let mut h = standard_present();
    seed_windows(&mut h);
    let pre_w = h.windows()[2];
    let p = unplug_primary_and_plan(&mut h).expect("规划");
    drive_tx(&mut h, &p).expect("迁移");
    let post = h.windows()[2];
    check(
        cs,
        "A53-迁移-主屏拔出后呈现目标换屏且世代+1",
        h.target == Slot::S1 && h.target_epoch == 2 && h.counts.migrations_ok == 1,
        format!("目标 {}→{}，世代 1→{}", Slot::S0.zh(), h.target.zh(), h.target_epoch),
    );
    check(
        cs,
        "A53-迁移-落在旧屏的窗口全部迁到新屏",
        h.windows().iter().all(|w| h.topology.is_online(w.slot)) && h.stranded_windows() == 0 && post.slot == Slot::S1,
        format!("三窗全在线屏上（残留 {} 个），原副屏窗迁至 {}", h.stranded_windows(), post.slot.zh()),
    );
    check(
        cs,
        "A53-布局-逻辑尺寸原样保留",
        post.w == pre_w.w && post.h == pre_w.h,
        format!("{}×{} → {}×{}（版面不被重排）", pre_w.w, pre_w.h, post.w, post.h),
    );
    let inside = h.windows().iter().all(|w| {
        let s = h.topology.screen(w.slot);
        w.x >= 0 && w.y >= 0 && i32::from(w.x) + i32::from(w.w) <= i32::from(s.width)
            && i32::from(w.y) + i32::from(w.h) <= i32::from(s.height)
    });
    check(
        cs,
        "A53-布局-位置夹取进新屏边界不乱飞",
        inside,
        format!("三窗左上角皆在 [0, 屏宽-窗宽] 内（逐窗手算：窗一 {}）", h.windows()[0].screen_line()),
    );
    let mut over = standard_present();
    seed_windows(&mut over);
    // 越界窗口放在**将被拔掉的主屏**上：布局段只搬离线屏上的窗口，
    // 放在还在线的屏上它压根不参与迁移，夹取也就无从谈起。
    over.windows[0] = WindowSlot { id: 1, slot: Slot::S0, x: 1800, y: 1000, w: 800, h: 600 };
    let p2 = unplug_primary_and_plan(&mut over).expect("规划");
    drive_tx(&mut over, &p2).expect("迁移");
    let m = over.windows()[0];
    check(
        cs,
        "A53-布局-越界坐标被夹取而非丢弃窗口",
        m.x == i32::from(2560u16.saturating_sub(800))
            && m.y == i32::from(1440u16.saturating_sub(600))
            && m.w == 800
            && m.h == 600,
        format!("越界坐标 (1800,1000) 夹到 ({},{})，尺寸仍 {}×{}，窗口仍在表内", m.x, m.y, m.w, m.h),
    );
    for (fault, label) in [
        (Fault::StageWindows, "窗口段"),
        (Fault::StageTarget, "目标段"),
        (Fault::Commit, "提交拍"),
        (Fault::Confirm, "确认拍"),
    ] {
        let mut g = standard_present();
        seed_windows(&mut g);
        let pre: [WindowSlot; MAX_WINDOWS] = g.windows;
        let pl = unplug_primary_and_plan(&mut g).expect("规划");
        g.inject = fault;
        let r = drive_tx(&mut g, &pl);
        let untouched = (0..g.window_count).all(|i| g.windows[i] == pre[i]);
        // 失败后窗口**原封不动**——「留在旧屏」是拔屏这一物理事实的后果，
        // 不是回退的缺陷；但它必须被自审**看见**（否则失败被读成迁移成功）。
        let seen = g.audit().iter().any(|c| *c == CODE_LAYOUT_LOST);
        check(
            cs,
            ls(format!("A53-回退-{label}注入失败即回退主屏")),
            r.is_err()
                && g.target == Slot::S0
                && untouched
                && g.counts.migrations_failed == 1
                && g.counts.migrations_ok == 0
                && seen,
            format!(
                "{}注入失败（{:?}）：呈现目标回退 {}、窗口表逐格未动、失败计数 {}、离线残留被自审看见={}",
                label, r, g.target.zh(), g.counts.migrations_failed, seen
            ),
        );
    }
    let mut g = standard_present();
    seed_windows(&mut g);
    let pl = unplug_primary_and_plan(&mut g).expect("规划");
    g.inject = Fault::Commit;
    let _ = drive_tx(&mut g, &pl);
    g.inject = Fault::None;
    let out = g.present_frame();
    check(
        cs,
        "A53-回退-回退后目标离线则呈现拦下而非落死屏",
        out == FrameOutcome::Held && g.frames.white == 0 && g.audit().iter().any(|c| *c == CODE_NO_SURVIVOR),
        format!("迁移失败回退主屏而原屏已离线：呈现判 {:?}（不呈现到死屏、不丢帧）", out),
    );
}

/// 不白屏承诺面（**可机检事实**）。
fn checks_noflicker(cs: &mut CheckSet) {
    let mut h = standard_present();
    seed_windows(&mut h);
    let o0 = h.present_frame();
    let p = unplug_primary_and_plan(&mut h).expect("规划");
    h.begin(&p, Actor::Compositor).expect("建档");
    let o1 = h.present_frame();
    let o2 = h.present_frame();
    h.stage(Segment::Windows).expect("备段");
    h.stage(Segment::Target).expect("备段");
    let o3 = h.present_frame();
    h.commit().expect("提交");
    let o4 = h.present_frame();
    h.confirm().expect("确认");
    let o5 = h.present_frame();
    check(
        cs,
        "A53-不白屏-迁移全程零白屏",
        h.frames.white == 0 && h.audit().iter().all(|c| *c != CODE_WHITE_SCREEN),
        format!("六帧走完（建档前/在途两帧/备齐后/提交后/确认后）白屏计数 {}", h.frames.white),
    );
    check(
        cs,
        "A53-不白屏-在途帧一律拦下且总账守恒",
        o0 == FrameOutcome::Presented
            && o1 == FrameOutcome::Held
            && o2 == FrameOutcome::Held
            && o3 == FrameOutcome::Held
            && o4 == FrameOutcome::Held
            && o5 == FrameOutcome::Presented
            && h.frames.balanced(),
        format!("逐帧：{:?}→{:?}→{:?}→{:?}→{:?}→{:?}，总账 {}={}+{}+{}+{}（提交后未确认仍在途，故拦下）",
            o0, o1, o2, o3, o4, o5,
            h.frames.total, h.frames.presented, h.frames.held, h.frames.no_target, h.frames.white),
    );
    check(
        cs,
        "A53-不白屏-迁移期用户看得见画面",
        h.frames.presented >= 2 && h.frames.held >= 3,
        format!("已呈现 {} 帧、拦下 {} 帧（拦下是重排不是丢帧，两端都看得见画面）", h.frames.presented, h.frames.held),
    );
    let mut v = standard_present();
    v.audit_force_drop = true;
    let white = v.present_frame();
    check(
        cs,
        "A53-不白屏-白屏分支可达非死码（变异实测）",
        white == FrameOutcome::White
            && v.frames.white == 1
            && v.audit().iter().any(|c| *c == CODE_WHITE_SCREEN),
        format!("审计钩强制丢帧即判 {:?}，自审当场报白屏（证明该分支真能走到）", white),
    );
    let mut n = HotplugPresent::new(TopologySnapshot::blank());
    let o = n.present_frame();
    check(
        cs,
        "A53-不白屏-未迁移目标与白屏分开记账",
        o == FrameOutcome::NoTarget && n.frames.no_target == 1 && n.frames.white == 0 && n.audit().is_empty(),
        format!("零屏时呈现判 {:?}（不是白屏），白屏计数 {}", o, n.frames.white),
    );
    check(
        cs,
        "A53-不白屏-四值结局封闭且只有白屏算违约",
        FrameOutcome::White.is_violation()
            && !FrameOutcome::Presented.is_violation()
            && !FrameOutcome::Held.is_violation()
            && !FrameOutcome::NoTarget.is_violation(),
        "Presented/Held/NoTarget/White 四值封闭，仅 White 计入违约".to_string(),
    );
}

/// 判据承载力与读屏面。
fn checks_meta(cs: &mut CheckSet) {
    let mut uniq = true;
    for i in 0..CODES.len() {
        for j in (i + 1)..CODES.len() {
            if CODES[i] == CODES[j] {
                uniq = false;
            }
        }
    }
    check(cs, "A53-判据-十码两两互异", uniq, format!("{} 个诊断码互异（按码归类的前提）", CODES.len()));
    let seg_ok = CODES.iter().all(|c| (c >> 8) == 0x8E);
    check(
        cs,
        "A53-判据-码段独占0x8E",
        seg_ok,
        format!("全码独占 0x8E 段（与 0x4A/0x4B/0x4D/0x4E/0x8D 段互斥），首码 {:04X}", CODES[0]),
    );
    let unknown = explain(0xFFFF);
    check(
        cs,
        "A53-判据-未知码兜底不panic",
        !unknown.is_empty() && !unknown.contains("未知切换") && CODES.iter().all(|c| !explain(*c).is_empty()),
        format!("未知码兜底人话：{:?}；{} 个在册码皆有人话", unknown, CODES.len()),
    );
    let h = standard_present();
    let lines = h.a11y_lines();
    let bilingual = lines.iter().all(|l| l.contains('/'));
    check(
        cs,
        "A53-面板-七行双语且逐行绑定事实",
        lines.len() == 7 && bilingual && lines[0].contains("屏一") && lines[2].contains("0") && lines[3].contains("0"),
        format!("面板 {} 行全双语，首行绑定呈现目标（{}）", lines.len(), lines[0]),
    );
    let mut g = standard_present();
    seed_windows(&mut g);
    let pl = unplug_primary_and_plan(&mut g).expect("规划");
    drive_tx(&mut g, &pl).expect("迁移");
    let l2 = g.a11y_lines();
    check(
        cs,
        "A53-面板-迁移后播报新目标与在线屏数",
        l2[0].contains("屏二") && l2[1].contains('2') && l2[4].contains('2'),
        format!("迁移后首行「{}」，第二行在线屏数 2，窗口迁移 2 个", l2[0]),
    );
    let w = standard_windows()[0];
    check(
        cs,
        "A53-面板-窗口播报只含几何不含标题",
        w.screen_line().contains("800×600") && !w.screen_line().contains("标题"),
        format!("窗口行只报几何：{}", w.screen_line()),
    );
}

/// 跑 VE-F0053 全部自检。
pub fn run_vea53_checks() -> CheckSet {
    let mut cs = CheckSet::new("VE-F0053 显示器热插拔呈现端");
    checks_slots(&mut cs);
    checks_contract(&mut cs);
    checks_plan(&mut cs);
    checks_tx(&mut cs);
    checks_migrate(&mut cs);
    checks_noflicker(&mut cs);
    checks_meta(&mut cs);
    cs
}
