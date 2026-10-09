//! VE-F0052 · 分辨率热切换处理（VE-A 域 · A03 同步与呈现组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0052`
//!
//! **职责定位（锚点原文）**：分辨率热切换处理——游戏内分辨率变更的热处理
//! （**交换链重建 + 视口更新 + UI 重排三段原子**），**切换不闪屏承诺**，
//! **失败回退旧分辨率**；含**切换与 DPI 变更的联动处理**。数据结构：热切换器。
//!
//! **判据（锚点原文）**：三段原子、不闪屏、失败回退、视口联动、判据。
//!
//! ## 错误路径与降级矩阵（锚点原文，逐条落实）
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 闪屏 → **修复** | [`HotSwitch::present`] 的**几何对账**：拿活动链的**真描述符**（[`PresentHub::active_desc`]）与已提交视口比，宽高不等即判 [`PresentOutcome::Flickered`] 并计入 `flickers`。见下「不闪屏的可判定定义」——这里刻意不写成承诺 |
//! | 失败 → **回退** | [`HotSwitch::commit`] 是**唯一**落地点：三段任一未备齐、或链重建失败，`current`/`viewport`/`ui` **一个字段都不动**，`rollbacks` 留痕。回退不是「再切回去」（那要重建两次，两次重建之间用户看到的正是几何错配的链），回退是**从未离开** |
//! | 三段失配 → **事务回滚** | 三段各自独立备齐（[`Segment`]）+ 提交闸查全备齐；缺任一段即拒并清账，不存在「建了链没更视口」或「视口改了 UI 没重排」这种半落地态 |
//!
//! ## 三段原子：为什么是「先备齐、后一次落地」而不是「逐段生效、失败再撤」
//!
//! 逐段生效 + 补偿回滚听着更直接，实则更差：视口段失败后要把已经换掉的交换链
//! **重建回旧分辨率**，而这次重建本身要走一遍 A46 的三段事务——两次重建之间
//! 用户看到的就是一帧几何错配的链，那正是「闪屏」的成因。
//!
//! 故本条取**两拍**：备齐拍只写 `staged[3]`（三个 bool，不触任何现役状态），
//! 提交拍一次性落地 `current` + `viewport` + `ui` 并 `epoch += 1`。于是
//! 「三段失配 → 事务回滚」**不需要补偿逻辑**——没提交就等于没发生，回退天然
//! 廉价且无残影。锚点要的是「三段原子」，两拍是达成原子性的手段而非替代：
//! 落地那一拍三者是一次连续赋值，不存在中间可观测态。
//!
//! ## 不闪屏的可判定定义
//!
//! 呈现一次，结果四值封闭（[`PresentOutcome`]）：`Presented` / `Held` /
//! `NoChain` / `Flickered`。「无链」是**不可省的第三值**——只有两值封闭就
//! 没有「还不知道」这一说，统计上会把尚未建链算成闪烁，告警随即被噪声淹没。
//!
//! **诚实声明（勿夸大）**：在正常路径上 `Flickered` 是**不可达**的，因为
//! 「一次落地」已使链几何与视口同源。保留该分支不是声称它日常在抓 bug，而是
//! 它是**设计违约的可检测出口**：任何绕过 `commit` 直接改视口、或让 `commit`
//! 只重建不更新视口的改动，都会立刻让它转红。判据侧以变异实测这一点（删掉
//! `commit` 的视口更新即转红），否则「四值封闭」只是自说自话。
//!
//! 事务在途时呈现判 `Held`（拦下而非丢弃，且计数）——这是第二道保险：
//! 新链备齐前不放行呈现。
//!
//! ## DPI 联动（锚点：含切换与 DPI 变更的联动处理）
//!
//! - **空闲期 DPI 变更**：只走 UI 重排段，**不重建交换链**——像素几何没变，
//!   变的是逻辑尺寸。诚实的代价声明：省一次重建，代价是 UI 须按新比例重排。
//! - **在途期 DPI 变更**：**显式拒绝**（[`CODE_DPI_DURING_TX`]）而非悄悄并入
//!   暂存。并入会让「已暂存的 UI 布局」与「用户刚给的 DPI」互相矛盾，提交时
//!   按谁算都不对；拒绝则冲突显性，调用方提交后重驱一次即可。
//!
//! ## 基线闸（承原子性的前提，勿删）
//!
//! 与 VE-F0051 同一教训：对比基准是 `from`，若 `begin` 不核对
//! `plan.from == current`，过期计划会把基准写成从未处于的分辨率。见
//! [`HotSwitch::begin`]。
//!
//! ## 性能逐项分解（锚点原文）：O(1)
//!
//! 校验是常数次比较；UI 列数是**一次除法**；几何对账是常数次比较；
//! 无循环、无历史扫描（读屏行拼装定容 7 行）。
//!
//! ## 跨批对接点（锚点原文）：A46 重建复用
//!
//! 交换链重建**不重写**：提交拍直接调 [`PresentHub::rebuild`]（vea46_present），
//! 它自己的三段原子事务与失败账本一并生效，本条只负责「何时调、调成什么」。
//! 几何边界也复用 A46 的 `MIN/MAX_WIDTH/HEIGHT`——**不另立标准**：两处各写
//! 一套尺寸上限，迟早出现「A46 判合法、本条判非法」的互相扯皮。
//!
//! ## 无障碍与隐私（锚点原文）：切换状态读屏播报
//!
//! [`HotSwitch::a11y_lines`]——中英双语七行，只报切换事实与聚合计数，
//! 不泄漏窗口标题与画面内容。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **VE-F0046** 管交换链本身；本条只在提交拍调它，不碰链的内部。
//! - **VE-F0051** 管显示模式（FSE/BFS/窗口）；本条管模式内的分辨率几何。
//! - **VE-F0053** 管显示器热插拔；本条管用户在设置里改分辨率。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;

use super::vea46_present::{
    ChainDesc, PresentHub, BUFFERS_DOUBLE, MAX_HEIGHT, MAX_WIDTH, MIN_HEIGHT, MIN_WIDTH,
};

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x8Dxx —— 全 kernel 树 grep 后确认零占用；
//    0x4A=vea50 / 0x4B=vea51 / 0x4C=vef28 / 0x4D=vef31 / 0x4E=vef32 /
//    0x58=vcq02 均已占用）
// ---------------------------------------------------------------------------

/// 分辨率越界（宽高为零或超 A46 上界）。
pub const CODE_BAD_RESOLUTION: u16 = 0x8D01;
/// DPI 比例越界或为零。
pub const CODE_BAD_DPI: u16 = 0x8D02;
/// 切换事务在途（拒绝重入）。
pub const CODE_TX_BUSY: u16 = 0x8D03;
/// 相位/顺序违例（调用方 bug：跳段提交、未备齐即提交、非帧边界）。
pub const CODE_PHASE: u16 = 0x8D04;
/// 某段备齐失败或提交拍失败（注入或真实）。
pub const CODE_SEG_FAILED: u16 = 0x8D05;
/// DPI 在途期联动被拒（显性冲突，不悄悄并入暂存）。
pub const CODE_DPI_DURING_TX: u16 = 0x8D06;
/// 无活动交换链（尚未首建）。
pub const CODE_NO_ACTIVE_CHAIN: u16 = 0x8D07;
/// 呈现几何失配（**闪屏**——不闪屏承诺的反面事实）。
pub const CODE_FPS: u16 = 0x8D08;
/// 基线不符：计划起点与现役分辨率不一致（过期计划）。
pub const CODE_BASELINE: u16 = 0x8D09;

/// 本域诊断码全集（判据对账：两两互异 + 全占 0x8D 段）。
pub const CODES: [u16; 9] = [
    CODE_BAD_RESOLUTION,
    CODE_BAD_DPI,
    CODE_TX_BUSY,
    CODE_PHASE,
    CODE_SEG_FAILED,
    CODE_DPI_DURING_TX,
    CODE_NO_ACTIVE_CHAIN,
    CODE_FPS,
    CODE_BASELINE,
];

/// 人话说明（后果 + 下一步，不能只说「失败」；未知码有兜底不 panic）。
pub const fn explain(code: u16) -> &'static str {
    match code {
        CODE_BAD_RESOLUTION => "分辨率越界：宽高须落在交换链规格内，先修正取值再发起切换",
        CODE_BAD_DPI => "DPI 比例非法：须为 50..=400 的整数百分比，零与越界一律拒绝",
        CODE_TX_BUSY => "切换事务在途：等待本次提交或放弃后再发起，切勿并发发起",
        CODE_PHASE => "事务相位违例：须按「备齐三段→提交」顺序调用，跳段不改任何状态",
        CODE_SEG_FAILED => "某段失败：本次切换未落地，现役分辨率保持不变，可重试",
        CODE_DPI_DURING_TX => "DPI 变更与切换在途冲突：已显性拒绝，待本次提交后重驱一次",
        CODE_NO_ACTIVE_CHAIN => "尚无活动交换链：先建立初始链再谈呈现",
        CODE_FPS => "呈现几何失配（闪屏）：活动链与已提交视口宽高不等，提交后应归零",
        CODE_BASELINE => "基线不符：计划起点与现役分辨率不一致，重读现役后重新规划",
        _ => "未知热切换诊断码（未登记）",
    }
}

// ---------------------------------------------------------------------------
// 二、分辨率与 DPI 描述符（几何边界复用 A46 常量，不另立标准）
// ---------------------------------------------------------------------------

/// DPI 比例下界（百分比）。
pub const DPI_MIN_PCT: u16 = 50;
/// DPI 比例上界（百分比）。
pub const DPI_MAX_PCT: u16 = 400;

/// UI 逻辑单元宽（逻辑像素；UI 重排的列宽基准）。
pub const LOGICAL_CELL_W: u32 = 96;

/// 分辨率 + DPI（切换事务的唯一载荷）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    /// 像素宽。
    pub width: u32,
    /// 像素高。
    pub height: u32,
    /// DPI 比例（百分比，50..=400）。
    pub dpi_pct: u16,
}

impl Resolution {
    /// 构造。
    pub const fn new(width: u32, height: u32, dpi_pct: u16) -> Resolution {
        Resolution { width, height, dpi_pct }
    }

    /// 自检：像素几何复用 A46 边界，DPI 用自有边界。
    pub const fn validate(&self) -> Result<(), u16> {
        if self.width < MIN_WIDTH || self.width > MAX_WIDTH {
            return Err(CODE_BAD_RESOLUTION);
        }
        if self.height < MIN_HEIGHT || self.height > MAX_HEIGHT {
            return Err(CODE_BAD_RESOLUTION);
        }
        if self.dpi_pct < DPI_MIN_PCT || self.dpi_pct > DPI_MAX_PCT {
            return Err(CODE_BAD_DPI);
        }
        Ok(())
    }

    /// 像素面积（u64 口径：16384² 虽不溢出 u32，但乘法口径统一走 u64，
    /// 省得将来放宽上界时再回来补一次溢出防护）。
    pub const fn pixel_area(&self) -> u64 {
        (self.width as u64) * (self.height as u64)
    }

    /// 逻辑宽（像素宽按 DPI 折算；**向下取整是刻意的**——向上取整会让小窗
    /// 的列数凭空多一列，而「多出来的那列」在右边缘之外，用户看不见）。
    pub const fn logical_width(&self) -> u32 {
        ((self.width as u64) * 100u64 / (self.dpi_pct as u64)) as u32
    }

    /// 链描述符（缓冲数与撕裂策略由本模块沿用现状，不在此处发明）。
    pub const fn to_desc(&self, buffers: u8, allow_tearing: bool) -> ChainDesc {
        ChainDesc::new(self.width, self.height, buffers, allow_tearing)
    }

    /// 几何是否相等（**只比宽高**：DPI 不改变像素几何，比进去会让 DPI 变更
    /// 被误报成几何失配即闪屏）。
    pub const fn same_geometry(self, o: Resolution) -> bool {
        self.width == o.width && self.height == o.height
    }
}

// ---------------------------------------------------------------------------
// 三、视口与 UI 布局（两段的落地载体）
// ---------------------------------------------------------------------------

/// 视口（像素空间；本条只做整屏视口，偏移由上游定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    /// 由分辨率派生整屏视口（O(1)；唯一构造路径——视口若可被随手改写，
    /// 「视口与分辨率同源」这条不变式就不成立）。
    pub const fn fullscreen(r: &Resolution) -> Viewport {
        Viewport { x: 0, y: 0, width: r.width, height: r.height }
    }
}

/// UI 布局（重排后的可观察事实：缩放比例 + 列数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiLayout {
    /// 缩放比例（百分比，与分辨率一致）。
    pub scale_pct: u16,
    /// 逻辑列数（逻辑宽一次除法得出，下界 1）。
    pub columns: u32,
}

impl UiLayout {
    /// 由分辨率**确定性**派生。UI 重排不是自由变量：同一分辨率必得同一布局，
    /// 否则「重排完成」无法被核对，只能被声称。
    pub const fn derive(r: &Resolution) -> UiLayout {
        let lw = r.logical_width();
        let cols = lw / LOGICAL_CELL_W;
        UiLayout { scale_pct: r.dpi_pct, columns: if cols == 0 { 1 } else { cols } }
    }
}

// ---------------------------------------------------------------------------
// 四、三段事务（备齐拍 → 提交拍）
// ---------------------------------------------------------------------------

/// 事务三段（锚点：交换链重建 / 视口更新 / UI 重排）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    /// 交换链重建。
    Chain,
    /// 视口更新。
    Viewport,
    /// Ui 重排。
    Ui,
}

impl Segment {
    /// 在册三段（判据对账基准：恰三段）。
    pub const ALL: [Segment; 3] = [Segment::Chain, Segment::Viewport, Segment::Ui];

    /// 三段段数。
    pub const COUNT: usize = 3;

    pub const fn ordinal(self) -> usize {
        match self {
            Segment::Chain => 0,
            Segment::Viewport => 1,
            Segment::Ui => 2,
        }
    }

    pub const fn from_ordinal(i: usize) -> Option<Segment> {
        match i {
            0 => Some(Segment::Chain),
            1 => Some(Segment::Viewport),
            2 => Some(Segment::Ui),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Segment::Chain => "交换链重建 / swapchain rebuild",
            Segment::Viewport => "视口更新 / viewport update",
            Segment::Ui => "UI重排 / ui reflow",
        }
    }
}

/// 演练注入点（重建失败是常态不是异常：驱动升级/热插拔/模式切换都会触发，
/// 故三段各有常态化注入点，否则失败路径只能等真故障走一次）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultInjector {
    /// 不注入。
    None,
    /// 交换链段注入。
    AtChain,
    /// 视口段注入。
    AtViewport,
    /// Ui 段注入。
    AtUi,
    /// 提交拍注入（三段全备齐但落地失败）。
    AtCommit,
}

impl FaultInjector {
    pub const ALL: [FaultInjector; 5] = [
        FaultInjector::None,
        FaultInjector::AtChain,
        FaultInjector::AtViewport,
        FaultInjector::AtUi,
        FaultInjector::AtCommit,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            FaultInjector::None => 0,
            FaultInjector::AtChain => 1,
            FaultInjector::AtViewport => 2,
            FaultInjector::AtUi => 3,
            FaultInjector::AtCommit => 4,
        }
    }

    /// 是否命中某段（段号 0/1/2）。`None` 永不命中。
    pub const fn hits_segment(self, seg: usize) -> bool {
        let o = self.ordinal();
        o >= 1 && o <= 3 && o == seg + 1
    }

    /// 是否命中提交拍（与三个段注入点严格分离）。
    pub const fn hits_commit(self) -> bool {
        self.ordinal() == 4
    }

    pub const fn label(self) -> &'static str {
        match self {
            FaultInjector::None => "不注入 / none",
            FaultInjector::AtChain => "链段注入 / at chain",
            FaultInjector::AtViewport => "视口段注入 / at viewport",
            FaultInjector::AtUi => "UI段注入 / at ui",
            FaultInjector::AtCommit => "提交拍注入 / at commit",
        }
    }
}

/// 一次切换计划。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchPlan {
    pub from: Resolution,
    pub to: Resolution,
    /// 同分辨率同 DPI = no-op（无事可做，不制造假事务）。
    pub noop: bool,
}

/// 规划一次切换（拒绝优先级：no-op → 目标校验 → 非帧边界 → 在途）。
pub fn plan_switch(
    from: Resolution,
    to: Resolution,
    at_frame_boundary: bool,
    busy: bool,
) -> Result<SwitchPlan, u16> {
    if from == to {
        return Ok(SwitchPlan { from, to, noop: true });
    }
    to.validate()?;
    if !at_frame_boundary {
        return Err(CODE_PHASE);
    }
    if busy {
        return Err(CODE_TX_BUSY);
    }
    Ok(SwitchPlan { from, to, noop: false })
}

/// 在途事务。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotTx {
    pub from: Resolution,
    pub to: Resolution,
    /// 三段备齐标记（备齐只写这里，不触任何现役状态）。
    pub staged: [bool; Segment::COUNT],
}

impl HotTx {
    pub const fn all_staged(&self) -> bool {
        self.staged[0] && self.staged[1] && self.staged[2]
    }
}

/// 热切换器（锚点「数据结构：热切换器」）。
///
/// 不变式：`tx == None` 时 `current` / `viewport` / `ui` 三者**同源于
/// `current`**；`tx == Some` 时三者一律不动。只有 `commit` 会同时改三者
/// 并 `epoch += 1`。
pub struct HotSwitch {
    current: Resolution,
    viewport: Viewport,
    ui: UiLayout,
    /// 世代号（每次成功落地 +1）。
    epoch: u64,
    tx: Option<HotTx>,
    hub: PresentHub,
    buffers: u8,
    allow_tearing: bool,
    injector: FaultInjector,
    // 计数（读屏与遥测；全 saturating_add）
    switches_ok: u32,
    rollbacks: u32,
    stage_fails: u32,
    held_presents: u32,
    flickers: u32,
    no_chain_presents: u32,
    presented_total: u32,
    dpi_applied: u32,
    dpi_rejected: u32,
    last_code: u16,
}

/// 读屏面板行数。
pub const PANEL_LINES: usize = 7;

impl HotSwitch {
    /// 新建热切换器（`first` 为初始分辨率；**不建链**——首建也走同一条
    /// 提交路径，免得「首建」与「热切换」长出两套语义）。
    pub fn new(first: Resolution, buffers: u8, allow_tearing: bool) -> HotSwitch {
        HotSwitch {
            current: first,
            viewport: Viewport::fullscreen(&first),
            ui: UiLayout::derive(&first),
            epoch: 0,
            tx: None,
            hub: PresentHub::new(),
            buffers: if buffers == 0 { BUFFERS_DOUBLE } else { buffers },
            allow_tearing,
            injector: FaultInjector::None,
            switches_ok: 0,
            rollbacks: 0,
            stage_fails: 0,
            held_presents: 0,
            flickers: 0,
            no_chain_presents: 0,
            presented_total: 0,
            dpi_applied: 0,
            dpi_rejected: 0,
            last_code: 0,
        }
    }

    // --- 只读面 -------------------------------------------------------------

    pub const fn current(&self) -> Resolution {
        self.current
    }

    pub const fn viewport(&self) -> Viewport {
        self.viewport
    }

    pub const fn ui(&self) -> UiLayout {
        self.ui
    }

    pub const fn epoch(&self) -> u64 {
        self.epoch
    }

    pub const fn tx(&self) -> Option<HotTx> {
        self.tx
    }

    pub const fn buffers(&self) -> u8 {
        self.buffers
    }

    pub const fn flickers(&self) -> u32 {
        self.flickers
    }

    pub const fn held_presents(&self) -> u32 {
        self.held_presents
    }

    pub const fn rollbacks(&self) -> u32 {
        self.rollbacks
    }

    pub const fn stage_fails(&self) -> u32 {
        self.stage_fails
    }

    pub const fn switches_ok(&self) -> u32 {
        self.switches_ok
    }

    pub const fn no_chain_presents(&self) -> u32 {
        self.no_chain_presents
    }

    pub const fn presented_total(&self) -> u32 {
        self.presented_total
    }

    pub const fn dpi_applied(&self) -> u32 {
        self.dpi_applied
    }

    pub const fn dpi_rejected(&self) -> u32 {
        self.dpi_rejected
    }

    pub const fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 聚合计数（切换成功/回退/段失败/拦下/闪屏/无链/DPI应用/DPI拒绝）。
    pub const fn counters(&self) -> (u32, u32, u32, u32, u32, u32, u32, u32) {
        (
            self.switches_ok,
            self.rollbacks,
            self.stage_fails,
            self.held_presents,
            self.flickers,
            self.no_chain_presents,
            self.dpi_applied,
            self.dpi_rejected,
        )
    }

    pub fn set_injector(&mut self, f: FaultInjector) {
        self.injector = f;
    }

    pub const fn injector(&self) -> FaultInjector {
        self.injector
    }

    /// 活动链世代（跨批对接：A46 侧事实；未建链 ⇒ 0）。
    pub fn active_generation(&self) -> u32 {
        self.hub.active_generation()
    }

    /// 活动链描述符（A46 侧真值；未建链 ⇒ None）。
    pub fn active_desc(&self) -> Option<ChainDesc> {
        self.hub.active_desc()
    }

    // --- 事务面 -------------------------------------------------------------

    /// 发起事务（no-op 直通；在途即拒；**基线闸**核对起点）。
    pub fn begin(&mut self, plan: SwitchPlan) -> Result<(), u16> {
        if plan.noop {
            return Ok(());
        }
        if self.tx.is_some() {
            self.last_code = CODE_TX_BUSY;
            return Err(CODE_TX_BUSY);
        }
        if plan.from != self.current {
            self.last_code = CODE_BASELINE;
            return Err(CODE_BASELINE);
        }
        self.tx = Some(HotTx { from: plan.from, to: plan.to, staged: [false; Segment::COUNT] });
        Ok(())
    }

    /// 备齐某段（注入失败即清账回退——现役三件一个都不动）。
    pub fn stage(&mut self, seg: Segment) -> Result<(), u16> {
        match self.tx.as_mut() {
            Some(_) => {}
            None => {
                self.last_code = CODE_PHASE;
                return Err(CODE_PHASE);
            }
        }
        let idx = seg.ordinal();
        if self.injector.hits_segment(idx) {
            self.tx = None;
            self.stage_fails = self.stage_fails.saturating_add(1);
            self.rollbacks = self.rollbacks.saturating_add(1);
            self.last_code = CODE_SEG_FAILED;
            return Err(CODE_SEG_FAILED);
        }
        if let Some(t) = self.tx.as_mut() {
            t.staged[idx] = true;
        }
        Ok(())
    }

    /// 三段是否全备齐（无事务时恒 false——「全备齐」必须绑定一个在途事务）。
    pub fn all_staged(&self) -> bool {
        match self.tx.as_ref() {
            Some(t) => t.all_staged(),
            None => false,
        }
    }

    fn abort(&mut self, code: u16) -> u16 {
        self.tx = None;
        self.rollbacks = self.rollbacks.saturating_add(1);
        self.last_code = code;
        code
    }

    /// 提交拍：链重建（**真调 A46**）+ 视口 + UI 三者**同时**落地。
    ///
    /// 三段任一未备齐 → [`CODE_PHASE`] 且**不改任何状态**：这就是
    /// 「三段失配 →事务回滚」的实现——不提交即未发生，回退无需补偿。
    pub fn commit(&mut self) -> Result<u64, u16> {
        let tx = match self.tx.as_ref() {
            Some(t) => *t,
            None => {
                self.last_code = CODE_PHASE;
                return Err(CODE_PHASE);
            }
        };
        if !tx.all_staged() {
            return Err(self.abort(CODE_PHASE));
        }
        if self.injector.hits_commit() {
            return Err(self.abort(CODE_SEG_FAILED));
        }
        // 段 1 真调 A46：它自己也是三段原子事务，失败则旧链原封不动。
        let desc = tx.to.to_desc(self.buffers, self.allow_tearing);
        if self.hub.rebuild(desc).is_err() {
            return Err(self.abort(CODE_SEG_FAILED));
        }
        // 落地拍：三者同源于 tx.to，一次连续赋值完成，无中间可观测态。
        self.current = tx.to;
        self.viewport = Viewport::fullscreen(&tx.to);
        self.ui = UiLayout::derive(&tx.to);
        self.epoch = self.epoch.saturating_add(1);
        self.tx = None;
        self.switches_ok = self.switches_ok.saturating_add(1);
        self.last_code = 0;
        Ok(self.epoch)
    }

    /// 显式放弃事务（放弃不是「失败」，但计入回退留痕——它确实改了用户预期）。
    pub fn abort_tx(&mut self) -> Result<(), u16> {
        match self.tx {
            None => {
                self.last_code = CODE_PHASE;
                Err(CODE_PHASE)
            }
            Some(_) => {
                self.abort(CODE_PHASE);
                Ok(())
            }
        }
    }

    // --- DPI 联动 -----------------------------------------------------------

    /// DPI 变更（空闲期只重排 UI，不重建链；在途期显式拒绝不并入暂存）。
    pub fn apply_dpi(&mut self, dpi_pct: u16) -> Result<(), u16> {
        if dpi_pct < DPI_MIN_PCT || dpi_pct > DPI_MAX_PCT {
            self.last_code = CODE_BAD_DPI;
            return Err(CODE_BAD_DPI);
        }
        if self.tx.is_some() {
            self.dpi_rejected = self.dpi_rejected.saturating_add(1);
            self.last_code = CODE_DPI_DURING_TX;
            return Err(CODE_DPI_DURING_TX);
        }
        let mut next = self.current;
        next.dpi_pct = dpi_pct;
        self.current = next;
        self.viewport = Viewport::fullscreen(&next);
        self.ui = UiLayout::derive(&next);
        self.epoch = self.epoch.saturating_add(1);
        self.dpi_applied = self.dpi_applied.saturating_add(1);
        self.last_code = 0;
        Ok(())
    }

    // --- 呈现（不闪屏的可判定面） -------------------------------------------

    /// **审计钩**：故意让活动链几何与已提交视口**不一致**（链按一个偏移后的
    /// 假几何重建，视口仍停在已提交分辨率）。
    ///
    /// 为什么生产代码里要留这么一个钩：否则 `PresentOutcome::Flickered` 在正常
    /// 路径**不可达**（两拍设计已保证二者同源），判据就只能断言一个走不到的
    /// 分支——那正是「死判据」的由来。留这个钩是为了让「检测器抓得到违约」
    /// 成为可执行事实，从而证明不闪屏不是靠「反正走不到」蒙混过关。
    ///
    /// 它模拟的是**任何绕过 `commit` 改动链**的失误（漏更视口、或在别处
    /// 重建了链）。正常调用方永远不该调它；判据侧调用一次以验证对账闸灵敏度。
    pub fn audit_force_chain_desync(&mut self) {
        let bogus = Resolution::new(
            self.current.width.saturating_add(64),
            self.current.height.saturating_add(64),
            self.current.dpi_pct,
        );
        let desc = bogus.to_desc(self.buffers, self.allow_tearing);
        let _ = self.hub.rebuild(desc);
    }

    /// 呈现一次（四值封闭；事务在途即拦下）。
    pub fn present(&mut self) -> PresentOutcome {
        if self.tx.is_some() {
            self.held_presents = self.held_presents.saturating_add(1);
            return PresentOutcome::Held;
        }
        // 活动链描述符是 A46 的**真值**，不是本模块推导值——否则这条对账
        // 拿自己和自己比，恒真，等于没对账。
        let chain = match self.hub.active_desc() {
            Some(c) => c,
            None => {
                self.no_chain_presents = self.no_chain_presents.saturating_add(1);
                self.last_code = CODE_NO_ACTIVE_CHAIN;
                return PresentOutcome::NoChain;
            }
        };
        let vp = self.viewport;
        if chain.width != vp.width || chain.height != vp.height {
            self.flickers = self.flickers.saturating_add(1);
            self.last_code = CODE_FPS;
            return PresentOutcome::Flickered;
        }
        self.presented_total = self.presented_total.saturating_add(1);
        PresentOutcome::Presented
    }
}

/// 呈现结果（四值封闭：第三值「无链」不可省——只有 Presented/Flickered 两值
/// 就没有「还不知道」这一说，未建链会被统计成闪烁，告警随即被噪声淹没）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentOutcome {
    /// 已呈现且几何自洽。
    Presented,
    /// 事务在途被拦下（拦下而非丢弃，且计数）。
    Held,
    /// 无活动链。
    NoChain,
    /// 几何失配 = 闪屏。
    Flickered,
}

impl PresentOutcome {
    pub const ALL: [PresentOutcome; 4] = [
        PresentOutcome::Presented,
        PresentOutcome::Held,
        PresentOutcome::NoChain,
        PresentOutcome::Flickered,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            PresentOutcome::Presented => "已呈现 / presented",
            PresentOutcome::Held => "事务在途已拦下 / held",
            PresentOutcome::NoChain => "无活动链 / no chain",
            PresentOutcome::Flickered => "几何失配（闪屏）/ flickered",
        }
    }
}

impl HotSwitch {
    /// 切换状态读屏播报（中英双语七行；只报事实与计数）。
    pub fn a11y_lines(&self) -> [String; PANEL_LINES] {
        let (sw, rb, sf, held, flick, nchain, dpi, dpr) = self.counters();
        [
            format!(
                "当前分辨率：{}×{} @{}% / resolution: {}×{} @{}%",
                self.current.width,
                self.current.height,
                self.current.dpi_pct,
                self.current.width,
                self.current.height,
                self.current.dpi_pct
            ),
            format!(
                "视口：{}×{} / viewport: {}×{}",
                self.viewport.width, self.viewport.height, self.viewport.width, self.viewport.height
            ),
            format!(
                "UI布局：缩放 {}%，{} 列 / ui: {}% scale, {} columns",
                self.ui.scale_pct, self.ui.columns, self.ui.scale_pct, self.ui.columns
            ),
            format!(
                "切换成功 {}，回退 {} / switches: {}, rollbacks: {}",
                sw, rb, sw, rb
            ),
            format!("段失败 {} / stage failures: {}", sf, sf),
            format!(
                "呈现：成功 {}，拦下 {}，闪屏 {}，无链 {} / presented: {}, held: {}, flickers: {}, no-chain: {}",
                self.presented_total, held, flick, nchain, self.presented_total, held, flick, nchain
            ),
            format!(
                "DPI 应用 {}，在途拒绝 {} / dpi applied: {}, rejected: {}",
                dpi, dpr, dpi, dpr
            ),
        ]
    }
}

// ---------------------------------------------------------------------------
// 五、域自检（判据逐条映射锚点：三段原子 / 不闪屏 / 失败回退 / 视口联动 / 判据）
// ---------------------------------------------------------------------------

/// 测试用分辨率构造（正方形便于心算，但判据该心算的地方照样手算）。
fn sq(n: u32) -> Resolution {
    Resolution::new(n, n, 100)
}

/// 走完一整条切换（备齐三段 + 提交），任一段失败即返回其错误码。
fn drive(m: &mut HotSwitch, plan: SwitchPlan) -> Result<u64, u16> {
    m.begin(plan)?;
    for sg in Segment::ALL {
        m.stage(sg)?;
    }
    m.commit()
}

/// F0052 域自检入口（聚合器调用；零 panic 面，失败逐条红不炸域）。
pub fn run_vea52_checks() -> CheckSet {
    let mut s = CheckSet::new("vea52_hotres");

    // --- 判据 1：三段封闭集与标签 ---
    {
        let mut rt = true;
        for sg in Segment::ALL {
            rt = rt && Segment::from_ordinal(sg.ordinal()) == Some(sg);
            rt = rt && sg.label().contains(" / ");
        }
        rt = rt && Segment::from_ordinal(3).is_none() && Segment::COUNT == 3;
        s.add("A52-三段-封闭集往返且双语", rt, "三段（链/视口/UI）序号往返一致，越界 None，恰好三段");
        let lb = [Segment::ALL[0].label(), Segment::ALL[1].label(), Segment::ALL[2].label()];
        s.add(
            "A52-三段-标签互异",
            lb[0] != lb[1] && lb[1] != lb[2] && lb[0] != lb[2],
            "三段标签互异（锚点三段各有所指，不混段）",
        );
    }

    // --- 判据 2：边界防护（几何复用 A46 常量 + DPI 自有界） ---
    {
        let bounds_src =
            MIN_WIDTH == 2 && MAX_WIDTH == 16384 && MIN_HEIGHT == 2 && MAX_HEIGHT == 16384;
        let over_w = Resolution::new(MAX_WIDTH + 1, MAX_HEIGHT, 100);
        let zero_w = Resolution::new(0, MAX_HEIGHT, 100);
        let over_h = Resolution::new(MAX_WIDTH, MAX_HEIGHT + 1, 100);
        let zero_h = Resolution::new(MAX_WIDTH, 0, 100);
        let at_max = Resolution::new(MAX_WIDTH, MAX_HEIGHT, 100);
        s.add(
            "A52-边界-几何越界拒且界源复用A46",
            over_w.validate() == Err(CODE_BAD_RESOLUTION)
                && zero_w.validate() == Err(CODE_BAD_RESOLUTION)
                && over_h.validate() == Err(CODE_BAD_RESOLUTION)
                && zero_h.validate() == Err(CODE_BAD_RESOLUTION)
                && at_max.validate().is_ok()
                && bounds_src,
            "宽高四向越界（含 0）拒绝，恰上界放行；上下界直接取 A46 的 MIN/MAX 常量（不另立标准）",
        );
        s.add(
            "A52-边界-DPI零与越界拒",
            Resolution::new(1920, 1080, 0).validate() == Err(CODE_BAD_DPI)
                && Resolution::new(1920, 1080, DPI_MAX_PCT + 1).validate() == Err(CODE_BAD_DPI)
                && Resolution::new(1920, 1080, DPI_MIN_PCT).validate().is_ok()
                && Resolution::new(1920, 1080, DPI_MAX_PCT).validate().is_ok()
                && DPI_MIN_PCT == 50
                && DPI_MAX_PCT == 400,
            "DPI 零与超上界拒绝，恰 50 / 恰 400 放行（贴线不误拒）",
        );
        s.add(
            "A52-边界-规划期即拒而非事务期",
            plan_switch(sq(1920), over_w, true, false) == Err(CODE_BAD_RESOLUTION)
                && plan_switch(sq(1920), Resolution::new(1280, 720, 0), true, false)
                    == Err(CODE_BAD_DPI),
            "非法目标在建事务之前就被规划闸拒（不留半建事务）",
        );
    }

    // --- 判据 3：三段原子（缺段即拒且状态零变动 / 全备齐一次落地） ---
    {
        let before = sq(1920);
        let mut m = HotSwitch::new(before, BUFFERS_DOUBLE, false);
        let p1 = plan_switch(before, sq(1280), true, false);
        // 缺视口段即提交 → 拒，现役不动。
        let short = match p1 {
            Ok(pl) => {
                let _ = m.begin(pl);
                let _ = m.stage(Segment::Chain);
                let _ = m.stage(Segment::Ui);
                m.commit()
            }
            Err(_) => Ok(0),
        };
        s.add(
            "A52-三段原子-缺段提交拒且零变动",
            short == Err(CODE_PHASE)
                && m.current() == before
                && m.viewport() == Viewport::fullscreen(&before)
                && m.ui() == UiLayout::derive(&before)
                && m.active_generation() == 0
                && m.tx().is_none()
                && m.rollbacks() == 1
                && m.switches_ok() == 0,
            "缺视口段即提交被拒：分辨率/视口/UI 全部原封不动、活动链仍为 0（未提交即未发生）",
        );
        // 全备齐 → 一次落地，三者同源。
        let mut m2 = HotSwitch::new(before, BUFFERS_DOUBLE, false);
        let ok_all = match plan_switch(before, sq(1280), true, false) {
            Ok(pl) => drive(&mut m2, pl),
            Err(_) => Err(0),
        };
        s.add(
            "A52-三段原子-全备齐一次落地",
            ok_all == Ok(1)
                && m2.current() == sq(1280)
                && m2.viewport() == Viewport::fullscreen(&sq(1280))
                && m2.ui() == UiLayout::derive(&sq(1280))
                && m2.epoch() == 1
                && m2.switches_ok() == 1
                && m2.tx().is_none()
                && m2.rollbacks() == 0,
            "链重建+视口+UI 同源于目标分辨率，epoch+1，事务清账，零回退",
        );
        s.add(
            "A52-三段原子-链重建真调A46",
            m2.active_generation() == 1
                && matches!(m2.active_desc(), Some(d) if d.width == 1280 && d.height == 1280),
            "提交拍真调 A46 rebuild（活动链世代 1），几何即 A46 侧真值而非本模块推导值",
        );
    }

    // --- 判据 4：不闪屏（四值封闭 + 几何对账 + 在途拦下 + 全程零闪屏） ---
    {
        let mut m = HotSwitch::new(sq(1920), BUFFERS_DOUBLE, false);
        let no_chain = m.present();
        let built = match plan_switch(sq(1920), sq(1280), true, false) {
            Ok(pl) => drive(&mut m, pl),
            Err(_) => Err(0),
        };
        let after = m.present();
        s.add(
            "A52-不闪屏-无链不误判为闪屏",
            no_chain == PresentOutcome::NoChain
                && m.no_chain_presents() == 1
                && m.flickers() == 0
                && PresentOutcome::ALL.len() == 4
                && m.presented_total() == 1,
            "首建前呈现判 NoChain 而非闪屏（三值两值封闭会把「未建链」算成闪烁，告警被噪声淹没）",
        );
        s.add(
            "A52-不闪屏-提交后几何自洽",
            built == Ok(1)
                && after == PresentOutcome::Presented
                && m.flickers() == 0
                && m.presented_total() == 1,
            "提交后呈现几何自洽（A46 链真值 vs 已提交视口逐位相等），闪屏计数 0",
        );
        // 事务在途 → Held，不计闪屏。
        let mut m2 = HotSwitch::new(sq(1920), BUFFERS_DOUBLE, false);
        let held_ok = match plan_switch(sq(1920), sq(1280), true, false) {
            Ok(pl) => {
                let _ = m2.begin(pl);
                let _ = m2.stage(Segment::Chain);
                let h1 = m2.present();
                let h2 = m2.present();
                let _ = m2.stage(Segment::Viewport);
                let _ = m2.stage(Segment::Ui);
                let c = m2.commit();
                (h1, h2, c, m2.present())
            }
            Err(_) => (PresentOutcome::NoChain, PresentOutcome::NoChain, Err(0), PresentOutcome::NoChain),
        };
        s.add(
            "A52-不闪屏-在途呈现拦下且全程零闪屏",
            held_ok.0 == PresentOutcome::Held
                && held_ok.1 == PresentOutcome::Held
                && held_ok.2 == Ok(1)
                && held_ok.3 == PresentOutcome::Presented
                && m2.held_presents() == 2
                && m2.flickers() == 0,
            "事务在途呈现一律 Held（拦下不丢弃）：新链备齐前不放行呈现；全程闪屏计数恒 0",
        );
        // 检测器非死码：先正常提交，再故意违约让链几何与视口不等，
        // 几何对账必须抓得到（否则四值封闭里的 Flickered 只是摆设）。
        let mut m3 = HotSwitch::new(sq(1920), BUFFERS_DOUBLE, false);
        let desync = match plan_switch(sq(1920), sq(1280), true, false) {
            Ok(pl) => {
                drive(&mut m3, pl).ok();
                m3.audit_force_chain_desync();
                m3.present()
            }
            Err(_) => PresentOutcome::Presented,
        };
        s.add(
            "A52-不闪屏-检测器抓得到违约",
            desync == PresentOutcome::Flickered
                && m3.flickers() == 1
                && m3.last_code() == CODE_FPS,
            "提交后故意让链几何与视口不等，几何对账当场判 Flickered（证明该分支非死码）",
        );
    }

    // --- 判据 5：失败回退（三段注入 → 现役零变动） ---
    {
        let before = sq(1920);
        let mut clean_all = true;
        let mut iter = 0usize;
        while iter < Segment::COUNT {
            let inj = match iter {
                0 => FaultInjector::AtChain,
                1 => FaultInjector::AtViewport,
                _ => FaultInjector::AtUi,
            };
            let mut m = HotSwitch::new(before, BUFFERS_DOUBLE, false);
            m.set_injector(inj);
            let r = match plan_switch(before, sq(1280), true, false) {
                Ok(pl) => {
                    let _ = m.begin(pl);
                    let mut acc: Result<(), u16> = Ok(());
                    for sg in Segment::ALL {
                        if acc.is_ok() {
                            acc = m.stage(sg);
                        }
                    }
                    acc
                }
                Err(e) => Err(e),
            };
            let clean = r == Err(CODE_SEG_FAILED)
                && m.current() == before
                && m.viewport() == Viewport::fullscreen(&before)
                && m.ui() == UiLayout::derive(&before)
                && m.active_generation() == 0
                && m.tx().is_none()
                && m.stage_fails() == 1
                && m.rollbacks() == 1
                && m.switches_ok() == 0;
            if !clean {
                clean_all = false;
            }
            iter += 1;
        }
        s.add(
            "A52-失败回退-三段注入各自零变动",
            clean_all,
            "链/视口/UI 三段逐个注入失败：事务清账、现役三件原封不动、活动链仍 0（回退=从未离开）",
        );
    }

    // --- 判据 6：提交拍失败也回退（备齐≠落地，两拍设计的全部意义） ---
    {
        let before = sq(1920);
        let mut m = HotSwitch::new(before, BUFFERS_DOUBLE, false);
        m.set_injector(FaultInjector::AtCommit);
        let r = match plan_switch(before, sq(1280), true, false) {
            Ok(pl) => drive(&mut m, pl),
            Err(e) => Err(e),
        };
        s.add(
            "A52-失败回退-提交拍失败也回退",
            r == Err(CODE_SEG_FAILED)
                && m.current() == before
                && m.viewport() == Viewport::fullscreen(&before)
                && m.active_generation() == 0
                && m.rollbacks() == 1
                && m.switches_ok() == 0
                && m.stage_fails() == 0,
            "三段全备齐但提交拍失败：链未建、视口未改——备齐≠落地，这正是两拍设计的意义",
        );
    }

    // --- 判据 7：视口联动（UI 重排确定性 / 视口同源 / DPI 两条路径） ---
    {
        let r = Resolution::new(1920, 1080, 100);
        let l = UiLayout::derive(&r);
        let lw = (1920u32 as u64) * 100u64 / 100u64; // 逻辑宽 = 1920
        let expect_cols = lw as u32 / LOGICAL_CELL_W; // 1920 / 96 = 20
        s.add(
            "A52-视口联动-UI重排确定性可核对",
            l.columns == expect_cols
                && l.columns == 20
                && l.scale_pct == 100
                && UiLayout::derive(&r) == l
                && UiLayout::derive(&Resolution::new(1920, 1080, 200)).columns == 10,
            "UI 列数=逻辑宽/单元宽 一次除法（1920@100%→恰 20 列；@200%→恰 10 列），同分辨率必得同布局",
        );
        let mut m = HotSwitch::new(r, BUFFERS_DOUBLE, false);
        let vp_ok = m.viewport() == Viewport { x: 0, y: 0, width: 1920, height: 1080 };
        let target = Resolution::new(1280, 720, 100);
        let r2 = match plan_switch(r, target, true, false) {
            Ok(pl) => drive(&mut m, pl),
            Err(e) => Err(e),
        };
        s.add(
            "A52-视口联动-视口随分辨率同步",
            vp_ok
                && r2 == Ok(1)
                && m.viewport().width == 1280
                && m.viewport().height == 720
                && m.viewport().x == 0
                && m.viewport().y == 0
                && m.ui().scale_pct == 100,
            "视口与已提交分辨率恒同源（切后 1280×720，偏移恒 0，缩放透传）",
        );
        // DPI 空闲期：只重排 UI，不重建链。
        let mut m2 = HotSwitch::new(r, BUFFERS_DOUBLE, false);
        if let Ok(pl) = plan_switch(r, Resolution::new(1280, 720, 100), true, false) {
            let _ = drive(&mut m2, pl);
        }
        let gen_before = m2.active_generation();
        let d = m2.apply_dpi(200);
        s.add(
            "A52-视口联动-DPI空闲只重排不重建链",
            d == Ok(())
                && m2.current().dpi_pct == 200
                && m2.current().width == 1280
                && m2.ui().scale_pct == 200
                && m2.active_generation() == gen_before
                && m2.dpi_applied() == 1
                && m2.dpi_rejected() == 0,
            "DPI 变更只走 UI 重排（像素几何未变故不重建链）：缩放变 200% 而活动链世代不变（省一次重建）",
        );
        // DPI 在途期：显式拒绝，事务不受影响。
        let mut m3 = HotSwitch::new(r, BUFFERS_DOUBLE, false);
        let rej = match plan_switch(r, Resolution::new(1280, 720, 100), true, false) {
            Ok(pl) => {
                let _ = m3.begin(pl);
                let _ = m3.stage(Segment::Chain);
                let e = m3.apply_dpi(150);
                let still_open = m3.tx().is_some();
                let finish = drive_staged(&mut m3);
                (e, still_open, finish)
            }
            Err(e) => (Err(e), false, Err(e)),
        };
        s.add(
            "A52-视口联动-DPI在途显式拒绝",
            rej.0 == Err(CODE_DPI_DURING_TX)
                && rej.1
                && m3.current().dpi_pct == 100
                && m3.dpi_rejected() == 1
                && rej.2 == Ok(1)
                && m3.current().dpi_pct == 100,
            "在途 DPI 显式拒绝而非并入暂存（并入会让暂存 UI 与新 DPI 互相矛盾）；事务可继续走到提交",
        );
        // DPI 不改几何：故不被误判闪屏。
        let mut m4 = HotSwitch::new(r, BUFFERS_DOUBLE, false);
        if let Ok(pl) = plan_switch(r, Resolution::new(1280, 720, 100), true, false) {
            let _ = drive(&mut m4, pl);
        }
        let _ = m4.apply_dpi(300);
        let before_geom = Resolution::new(1280, 720, 100);
        s.add(
            "A52-视口联动-DPI变更不误判闪屏",
            m4.current().same_geometry(before_geom)
                && m4.current().dpi_pct == 300
                && m4.present() == PresentOutcome::Presented
                && m4.flickers() == 0,
            "DPI 只改逻辑尺寸不改像素几何，几何对账只比宽高（把 DPI 比进去会把 DPI 变更误报成闪屏）",
        );
    }

    // --- 判据 8：基线闸 / 在途重入 / 相位错 / no-op / 显式放弃 ---
    {
        let base = sq(1920);
        let mut m = HotSwitch::new(base, BUFFERS_DOUBLE, false);
        let stale = SwitchPlan { from: sq(1024), to: sq(1280), noop: false };
        let r = m.begin(stale);
        s.add(
            "A52-基线闸-过期计划被拒且零副作用",
            r == Err(CODE_BASELINE)
                && m.tx().is_none()
                && m.current() == base
                && m.rollbacks() == 0
                && m.switches_ok() == 0,
            "计划起点≠现役即拒：不开事务、现役不动、零计数（过期计划属调用方 bug，不是系统失败）",
        );
        let mut m2 = HotSwitch::new(base, BUFFERS_DOUBLE, false);
        let busy = match plan_switch(base, sq(1280), true, false) {
            Ok(pl) => {
                let _ = m2.begin(pl);
                let _ = m2.stage(Segment::Chain);
                let plan_busy = plan_switch(base, sq(1024), true, true);
                let begin_busy = match plan_switch(base, sq(1024), true, false) {
                    Ok(p2) => m2.begin(p2),
                    Err(e) => Err(e),
                };
                (plan_busy, begin_busy)
            }
            Err(e) => (Err(e), Err(e)),
        };
        s.add(
            "A52-在途-重入拒绝且原事务不受影响",
            busy.0 == Err(CODE_TX_BUSY)
                && busy.1 == Err(CODE_TX_BUSY)
                && m2.tx().is_some(),
            "在途时规划与建档双双报忙（闸序先忙后基线：忙才是唯一可操作的诊断），原事务不受影响",
        );
        let mut m3 = HotSwitch::new(base, BUFFERS_DOUBLE, false);
        let st = m3.stage(Segment::Chain);
        let cm = m3.commit();
        let ab = m3.abort_tx();
        s.add(
            "A52-相位错-无事务即段/提交/放弃拒",
            st == Err(CODE_PHASE)
                && cm == Err(CODE_PHASE)
                && ab == Err(CODE_PHASE)
                && m3.current() == base
                && m3.rollbacks() == 0,
            "无事务时备齐/提交/放弃即拒，且不计回退（调用方顺序 bug 不该污染系统账）",
        );
        let mut m4 = HotSwitch::new(base, BUFFERS_DOUBLE, false);
        let noop = SwitchPlan { from: base, to: base, noop: true };
        s.add(
            "A52-相位错-noop免事务",
            m4.begin(noop) == Ok(()) && m4.tx().is_none() && m4.switches_ok() == 0,
            "同分辨率同 DPI 的 no-op 直通且不开事务（不制造假切换）",
        );
        s.add(
            "A52-相位错-非帧边界拒",
            plan_switch(base, sq(1280), false, false) == Err(CODE_PHASE)
                && plan_switch(base, sq(1280), true, false).is_ok(),
            "非帧边界切换拒绝，帧边界处合法（贴线不误拒）",
        );
        // 显式放弃也留回退痕。
        let mut m5 = HotSwitch::new(base, BUFFERS_DOUBLE, false);
        let ab2 = match plan_switch(base, sq(1280), true, false) {
            Ok(pl) => {
                let _ = m5.begin(pl);
                let _ = m5.stage(Segment::Chain);
                let a = m5.abort_tx();
                (a, m5.current(), m5.rollbacks(), m5.active_generation())
            }
            Err(e) => (Err(e), base, 0, 0),
        };
        s.add(
            "A52-显式放弃-留痕且现役不动",
            ab2.0 == Ok(())
                && ab2.1 == base
                && ab2.2 == 1
                && ab2.3 == 0
                && m5.tx().is_none(),
            "显式放弃记回退痕（它确实改变了用户预期），但现役与活动链一个都不动",
        );
    }

    // --- 判据 9：注入点族（段号一一对应且提交独立） ---
    {
        let mut ok9 = true;
        for inj in FaultInjector::ALL {
            let mut hits = 0usize;
            let mut k = 0usize;
            while k < Segment::COUNT {
                if inj.hits_segment(k) {
                    hits += 1;
                }
                k += 1;
            }
            if hits > 1 {
                ok9 = false;
            }
            if inj == FaultInjector::AtCommit {
                ok9 = ok9 && inj.hits_commit() && hits == 0;
            } else {
                ok9 = ok9 && !inj.hits_commit();
            }
            if inj == FaultInjector::None && hits != 0 {
                ok9 = false;
            }
            ok9 = ok9 && !inj.label().is_empty();
        }
        s.add("A52-注入-段号一一对应且提交独立", ok9, "五注入点：None 不命中、三个段各命中一段、提交拍独立命中");
    }

    // --- 判据 10：读屏面板七行双语 ---
    {
        let r = Resolution::new(1920, 1080, 100);
        let mut m = HotSwitch::new(r, BUFFERS_DOUBLE, false);
        if let Ok(pl) = plan_switch(r, Resolution::new(1280, 720, 100), true, false) {
            let _ = drive(&mut m, pl);
        }
        let _ = m.apply_dpi(150);
        // 1280@150% ⇒ 逻辑宽 853 ⇒ 853/96 = 8 列（手算，不取反推值）。
        let lines = m.a11y_lines();
        s.add(
            "A52-面板-七行双语且逐行绑定事实",
            lines.len() == PANEL_LINES
                && lines.iter().all(|l| !l.is_empty())
                && lines[0].contains("1280")
                && lines[0].contains("720")
                && lines[0].contains("@150%")
                && lines[1].contains("1280×720")
                && lines[2].contains("8 列")
                && lines[2].contains("缩放 150%")
                && lines[3].contains("switches: 1")
                && lines[4].contains("stage failures: 0")
                && lines[5].contains("flickers: 0")
                && lines[6].contains("dpi applied: 1")
                && lines[6].contains("rejected: 0")
                && lines.iter().all(|l| l.contains('/')),
            "面板七行逐行绑定：分辨率含 DPI / 视口 / UI 列数（手算 8 列）/成功与回退/段失败/呈现四类计数/DPI 计数，每行双语",
        );
    }

    // --- 判据 11：判据自身（码两两互异 + 段独占 + 兜底 + 条数对账） ---
    {
        let mut okc = true;
        let mut i = 0usize;
        while i < CODES.len() {
            let mut j = i + 1;
            while j < CODES.len() {
                if CODES[i] == CODES[j] {
                    okc = false;
                }
                j += 1;
            }
            i += 1;
        }
        s.add("A52-判据-九码两两互异", okc, "九码互异（按码归类的前提）");
        s.add(
            "A52-判据-码段独占0x8D",
            CODES.iter().all(|c| c & 0xFF00 == 0x8D00),
            "全码独占 0x8D 段（与 0x4A/0x4B/0x4C/0x4D/0x4E/0x58 段均互斥）",
        );
        s.add(
            "A52-判据-未知码兜底不panic",
            !explain(0x8DFF).is_empty() && explain(CODE_BAD_RESOLUTION) != explain(0x8DFF),
            "未知码有兜底人话（不崩也不静默）",
        );
        s.add(
            "A52-判据-条数对账",
            s.len() == 30,
            "判据条数恰 31（本条执行前已有 30 条，防悄悄增删）",
        );
    }

    s
}

/// 备齐余下两段并提交（供「DPI 在途拒绝后事务仍能走完」这条判据复用）。
fn drive_staged(m: &mut HotSwitch) -> Result<u64, u16> {
    let _ = m.stage(Segment::Viewport);
    let _ = m.stage(Segment::Ui);
    m.commit()
}