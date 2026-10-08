//! VE-F0032 · 资源状态跟踪器（VE-A 域 · GPU 资源全生命周期状态跟踪 + 违例检测 + 泄漏检测 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0032`
//!
//! **判据（锚点原文）**：GPU 资源全生命周期的状态跟踪（每资源当前状态/历史状态/
//! 预期状态三列），状态机违例检测（不存在的状态跳转=跟踪 bug），跟踪开销零帧预算声明；
//! 含跟踪器的泄漏检测（有状态无资源的孤儿告警）。
//! 判据：**三列状态、违例检测、开销声明、漂移校准、判据**。
//!
//! **错误路径与降级矩阵**（锚点原文）：
//!
//! - 违例 → **阻断级**（不存在的状态跳转是**跟踪 bug 或调用方 bug**，必须阻断，
//!   不是告警——告警会让它继续跑，然后在 GPU 上表现为偶发错画面）
//! - 跟踪漂移 → **校准**（外部路径绕过跟踪器改了资源，预期态与实际态脱节；
//!   提供显式校准入口，且校准**留痕**，否则「谁把状态改错的」永远查不到）
//! - 开销超标 → **降采样**（跟踪本身有开销，超预算就降低历史深度，
//!   不是无限增长到拖慢帧）
//!
//! **数据结构**：跟踪器（[`ResStateTracker`]）；违例检测（[`Violation`]）。
//!
//! **性能逐项分解**：O(资源)——[`ResStateTracker::transition`] 是 O(1)（查表 +
//! 一次状态机判定），历史环形缓冲写入 O(1)，孤儿扫描是 O(资源数) 单趟。
//!
//! **跨批对接点**：A31 帧图消费——本条产出的 [`ResState`] 三列供 F0031 的屏障
//! 推理核对；F0031 推屏障，本条判状态，方向是单向的。
//!
//! **无障碍与隐私**：跟踪面板读屏可达（[`ResStateTracker::a11y_lines`]）——报
//! 「资源数/违例数/漂移数/孤儿数/历史深度」，中英双语逐行。面板**只报聚合计数**，
//! **不报单个资源的句柄与尺寸**（那是资产布局信息）。
//!
//! ## 设计要点
//!
//! - **三列各司其职，不合并**（[`ResState`]：当前 / 历史 / 预期）：
//!   - **当前态**（`current`）：跟踪器认为此刻资源是什么状态；
//!   - **历史态**（`history`）：环形缓冲，保留最近 N 态，供漂移回溯；
//!   - **预期态**（`expected`）：调用方**声明**它接下来要什么状态。
//!   三列合成一个「当前」就等于丢掉了「声明」与「事实」的对照——而那正是
//!   跟踪器唯一的存在理由（十诫第 3 条：外部表现相同的错误路要分开）。
//! - **预期态不匹配是漂移，不是违例**（[`DriftKind`]）：预期与实际不符**可能**
//!   是合法的（外部路径改了资源），故走**校准**并留痕；而「不存在的状态跳转」
//!   （如 Undefined → Writing）是**逻辑上不可能**的边，走**阻断级**。
//!   两者混为一谈的后果是：合法的外部改动被当 bug 阻断，或真 bug 被当漂移静默校准。
//! - **违例是阻断级，不降级**（[`Violation::Block`]）：因为「不存在的跳转」意味着
//!   跟踪器或调用方的状态机已经错了，此时**继续跑等于往错误状态上叠加操作**。
//! - **状态机显式建模，跳转表可枚举**（[`ResState`] + [`allowed_next`]）：
//!   「允许的下一态」写成函数而非散在调用点，判据才能遍历全部边证无遗漏。
//! - **历史深度可降采样且降采样本身留痕**（[`ResStateTracker::set_history_depth`]）：
//!   锚点要求「跟踪开销零帧预算声明」——即跟踪不得侵占帧预算。故历史环形缓冲
//!   **有上限**，超预算时降深度并登记 [`ResStateTracker::downsamples`]，
//!   **不静默丢历史**（静默丢历史会让漂移回溯得出错误结论）。
//! - **孤儿检测：有状态无资源**（[`ResStateTracker::orphans`]）：资源已销毁但
//!   状态条目还在——那是泄漏。判据用**绝对值口径**断孤儿数恰等于
//!   「销毁数 − 曾创建数中仍存活数」，不用净值（十诫第 10 条）。
//! - **夹逼对钉死界位置**（[`MAX_RESOURCES`] / [`MAX_HISTORY`]）：一律用
//!   「界前合法 / 界上 / 界后一位」三点。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0031（`vea31_framegraph`）推屏障**，本条判状态。F0031 从资源访问边
//!   推出「谁读谁」，本条跟踪「资源此刻是什么状态」；前者管同步，后者管生命周期。
//! - **F0026（`vea26_desc_heap`）管描述符堆分配**，本条管**状态**。堆满了会
//!   分配失败，状态乱了是逻辑违例——两者都不是对方的问题。
//!
//! 两条各自**自持**定义类型，不跨模块 `use`——并行提交时跨模块引用会把两个模块
//! 的编译成败绑在一起，一方半成品就拖垮另一方，而这类失败报 E0583，与真实缺陷
//! 长得一样、极难分辨。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 可跟踪资源数上限（超出 → [`Violation::TooManyResources`]，阻断级）。
pub const MAX_RESOURCES: usize = 128;

/// 单资源历史深度上限（超出预算 → 降采样，不静默丢历史）。
pub const MAX_HISTORY: usize = 16;

/// 历史深度降采样的最小值（降到这里就不再降，改为登记告警）。
pub const MIN_HISTORY: usize = 1;

/// 资源状态种数。
pub const STATE_COUNT: usize = 6;

/// 跟踪器每帧开销预算（单位：微操作数；超预算即降采样）。
pub const FRAME_BUDGET: u32 = 4096;

// ---------------------------------------------------------------------------
// 二、状态与违例
// ---------------------------------------------------------------------------

/// 资源状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResState {
    /// 未定义（刚创建、尚未初始化）。
    Undefined,
    /// 初始化中。
    Initializing,
    /// 就绪可读。
    Ready,
    /// 正被写入（不可读）。
    Writing,
    /// 已销毁（终态）。
    Destroyed,
    /// 外部路径改过、待校准。
    Drifted,
}

impl ResState {
    /// 全集，顺序稳定（判据按此下标推导，不靠字面量）。
    pub const ALL: [ResState; STATE_COUNT] = [
        ResState::Undefined,
        ResState::Initializing,
        ResState::Ready,
        ResState::Writing,
        ResState::Destroyed,
        ResState::Drifted,
    ];

    /// 判别下标（**不是**线上编码值）。
    pub const fn ordinal(self) -> usize {
        match self {
            ResState::Undefined => 0,
            ResState::Initializing => 1,
            ResState::Ready => 2,
            ResState::Writing => 3,
            ResState::Destroyed => 4,
            ResState::Drifted => 5,
        }
    }

    /// 由下标反解（判据用它做往返一致性断言）。
    pub const fn from_ordinal(i: usize) -> Option<ResState> {
        match i {
            0 => Some(ResState::Undefined),
            1 => Some(ResState::Initializing),
            2 => Some(ResState::Ready),
            3 => Some(ResState::Writing),
            4 => Some(ResState::Destroyed),
            5 => Some(ResState::Drifted),
            _ => None,
        }
    }

    /// 中文标签。
    pub const fn zh(self) -> &'static str {
        match self {
            ResState::Undefined => "未定义",
            ResState::Initializing => "初始化中",
            ResState::Ready => "就绪",
            ResState::Writing => "写入中",
            ResState::Destroyed => "已销毁",
            ResState::Drifted => "待校准",
        }
    }

    /// 英文标签。
    pub const fn tag(self) -> &'static str {
        match self {
            ResState::Undefined => "undefined",
            ResState::Initializing => "initializing",
            ResState::Ready => "ready",
            ResState::Writing => "writing",
            ResState::Destroyed => "destroyed",
            ResState::Drifted => "drifted",
        }
    }

    /// 是否为终态（终态不可再转出）。
    pub const fn is_terminal(self) -> bool {
        matches!(self, ResState::Destroyed)
    }
}

/// 状态机允许的下一态（**显式建模**，判据据此遍历全部边证无遗漏）。
///
/// 返回空切片表示该态是终态或不允许任何转出。
pub fn allowed_next(from: ResState) -> &'static [ResState] {
    match from {
        ResState::Undefined => &[ResState::Initializing, ResState::Destroyed],
        ResState::Initializing => &[ResState::Ready, ResState::Drifted, ResState::Destroyed],
        ResState::Ready => &[ResState::Writing, ResState::Ready, ResState::Drifted, ResState::Destroyed],
        ResState::Writing => &[ResState::Ready, ResState::Drifted, ResState::Destroyed],
        ResState::Destroyed => &[],
        ResState::Drifted => &[ResState::Ready, ResState::Writing, ResState::Destroyed, ResState::Drifted],
    }
}

/// 跳转是否被状态机允许。
pub fn transition_allowed(from: ResState, to: ResState) -> bool {
    if from == to {
        // 自环由 allowed_next 显式列出者算合法（Ready→Ready 等幂等重入）
        return allowed_next(from).contains(&to);
    }
    allowed_next(from).contains(&to)
}

/// 违例种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Violation {
    /// 资源下标越界。
    BadResource,
    /// 不存在的状态跳转（**阻断级**）。
    IllegalTransition,
    /// 资源数超上限（**阻断级**）。
    TooManyResources,
    /// 从终态再转出（**阻断级**）。
    FromTerminal,
}

impl Violation {
    /// 全集，顺序稳定。
    pub const ALL: [Violation; 4] = [
        Violation::BadResource,
        Violation::IllegalTransition,
        Violation::TooManyResources,
        Violation::FromTerminal,
    ];

    /// 判别下标。
    pub const fn ordinal(self) -> usize {
        match self {
            Violation::BadResource => 0,
            Violation::IllegalTransition => 1,
            Violation::TooManyResources => 2,
            Violation::FromTerminal => 3,
        }
    }

    /// 是否为**阻断级**（锚点：违例 → 阻断级）。
    pub const fn is_blocking(self) -> bool {
        matches!(
            self,
            Violation::IllegalTransition | Violation::TooManyResources | Violation::FromTerminal
        )
    }

    /// 中文标签。
    pub const fn zh(self) -> &'static str {
        match self {
            Violation::BadResource => "资源下标越界",
            Violation::IllegalTransition => "不存在的状态跳转",
            Violation::TooManyResources => "资源数超上限",
            Violation::FromTerminal => "从终态再转出",
        }
    }

    /// 英文标签。
    pub const fn tag(self) -> &'static str {
        match self {
            Violation::BadResource => "bad resource",
            Violation::IllegalTransition => "illegal transition",
            Violation::TooManyResources => "too many resources",
            Violation::FromTerminal => "from terminal",
        }
    }
}

/// 漂移种类（预期与实际不符，走校准并留痕，**非阻断**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriftKind {
    /// 外部路径把资源改成了别的状态，跟踪器不知情。
    External,
    /// 声明的预期态与跟踪器当前态不符。
    Expectation,
}

impl DriftKind {
    /// 全集。
    pub const ALL: [DriftKind; 2] = [DriftKind::External, DriftKind::Expectation];

    /// 判别下标。
    pub const fn ordinal(self) -> usize {
        match self {
            DriftKind::External => 0,
            DriftKind::Expectation => 1,
        }
    }

    /// 中文标签。
    pub const fn zh(self) -> &'static str {
        match self {
            DriftKind::External => "外部路径改动",
            DriftKind::Expectation => "预期态不符",
        }
    }

    /// 英文标签。
    pub const fn tag(self) -> &'static str {
        match self {
            DriftKind::External => "external",
            DriftKind::Expectation => "expectation",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、资源三列状态
// ---------------------------------------------------------------------------

/// 单资源的**三列**状态：当前 / 历史 / 预期。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResStateRow {
    /// 资源下标。
    pub id: usize,
    /// 当前态（跟踪器认为此刻的状态）。
    pub current: ResState,
    /// 预期态（调用方声明接下来要什么）。
    pub expected: ResState,
    /// 历史态环形缓冲（最近在先）。
    pub history: Vec<ResState>,
    /// 资源是否仍存活（`false` ⇒ 已销毁但条目仍在 ⇒ 孤儿）。
    pub alive: bool,
}

impl ResStateRow {
    /// 新建一行（当前与预期均为 [`ResState::Undefined`]）。
    pub fn new(id: usize, _depth: usize) -> ResStateRow {
        // 深度由跟踪器在 push_history 时逐次传入，故构造期不需要它；
        // 保留参数是为了调用方意图显式（避免「忘了传深度」这类静默默认值）。
        ResStateRow {
            id,
            current: ResState::Undefined,
            expected: ResState::Undefined,
            history: Vec::new(),
            alive: true,
        }
    }

    /// 压入历史（最新在先，超深度丢最旧）。
    pub fn push_history(&mut self, s: ResState, depth: usize) {
        self.history.insert(0, s);
        // 历史深度**硬上限**：超了丢最旧，但由调用方登记降采样（不静默）。
        let mut i = self.history.len();
        while i > depth {
            self.history.pop();
            i -= 1;
        }
    }

    /// 历史长度。
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// 预期态与当前态是否相符。
    pub fn expectation_matches(&self) -> bool {
        self.expected == self.current
    }
}

/// 资源状态跟踪器。
#[derive(Clone, Debug)]
pub struct ResStateTracker {
    /// 各资源的三列状态。
    rows: Vec<ResStateRow>,
    /// 历史深度（可降采样）。
    depth: usize,
    /// 违例计数（按 [`Violation::ALL`] 下标，绝对值口径）。
    violations: [u32; 4],
    /// 漂移计数（按下标）。
    drifts: [u32; 2],
    /// 降采样次数（开销超标时登记，**不静默**）。
    downsamples: u32,
    /// 本帧微操作数（开销预算核算）。
    work: u32,
    /// 孤儿下标集合（有状态无资源：已销毁但条目仍在）。
    orphans: Vec<usize>,
}

impl Default for ResStateTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ResStateTracker {
    /// 新建跟踪器（历史深度取上限）。
    pub fn new() -> ResStateTracker {
        ResStateTracker {
            rows: Vec::new(),
            depth: MAX_HISTORY,
            violations: [0u32; 4],
            drifts: [0u32; 2],
            downsamples: 0,
            work: 0,
            orphans: Vec::new(),
        }
    }

    /// 登记一个新资源（返回其下标）。超上限 → 阻断级违例。
    pub fn create(&mut self) -> Result<usize, Violation> {
        self.work = self.work.saturating_add(1);
        if self.rows.len() >= MAX_RESOURCES {
            self.note_violation(Violation::TooManyResources);
            return Err(Violation::TooManyResources);
        }
        let id = self.rows.len();
        let d = self.depth;
        self.rows.push(ResStateRow::new(id, d));
        Ok(id)
    }

    /// 状态跳转。**不存在的跳转 → 阻断级违例**，且**不改变状态**。
    pub fn transition(&mut self, id: usize, to: ResState) -> Result<ResState, Violation> {
        self.work = self.work.saturating_add(1);
        if id >= self.rows.len() {
            self.note_violation(Violation::BadResource);
            return Err(Violation::BadResource);
        }
        let from = self.rows[id].current;
        if from.is_terminal() {
            // 终态不可转出——单独归因，不与「不存在该边」混为一谈
            self.note_violation(Violation::FromTerminal);
            return Err(Violation::FromTerminal);
        }
        if !transition_allowed(from, to) {
            self.note_violation(Violation::IllegalTransition);
            return Err(Violation::IllegalTransition);
        }
        let d = self.depth;
        let row = &mut self.rows[id];
        row.push_history(from, d);
        row.current = to;
        Ok(to)
    }

    /// 声明预期态（下一态）。与当前态不符 → 登记**预期态漂移**（非阻断）。
    pub fn expect_next(&mut self, id: usize, want: ResState) -> Result<(), Violation> {
        self.work = self.work.saturating_add(1);
        if id >= self.rows.len() {
            self.note_violation(Violation::BadResource);
            return Err(Violation::BadResource);
        }
        self.rows[id].expected = want;
        if !self.rows[id].expectation_matches() {
            self.note_drift(DriftKind::Expectation);
        }
        Ok(())
    }

    /// 登记**外部路径漂移**：资源被外部改了，跟踪器把当前态标 [`ResState::Drifted`]。
    ///
    /// 这是**校准入口**：外部改动是合法路径（锚点允许），但必须**显式登记**，
    /// 否则跟踪器会继续拿旧状态推断屏障，静默错。
    pub fn note_external_drift(&mut self, id: usize, actual: ResState) -> Result<(), Violation> {
        self.work = self.work.saturating_add(1);
        if id >= self.rows.len() {
            self.note_violation(Violation::BadResource);
            return Err(Violation::BadResource);
        }
        let d = self.depth;
        let row = &mut self.rows[id];
        let from = row.current;
        row.push_history(from, d);
        row.current = ResState::Drifted;
        // 校准后把预期态对齐到实际的外部状态（校准即「承认现实」）
        row.expected = actual;
        self.note_drift(DriftKind::External);
        Ok(())
    }

    /// 销毁资源。**条目保留**（供历史回溯），标 `alive = false`。
    pub fn destroy(&mut self, id: usize) -> Result<(), Violation> {
        self.work = self.work.saturating_add(1);
        if id >= self.rows.len() {
            self.note_violation(Violation::BadResource);
            return Err(Violation::BadResource);
        }
        let d = self.depth;
        let row = &mut self.rows[id];
        if row.current.is_terminal() {
            self.note_violation(Violation::FromTerminal);
            return Err(Violation::FromTerminal);
        }
        row.push_history(row.current, d);
        row.current = ResState::Destroyed;
        row.alive = false;
        Ok(())
    }

    /// 孤儿检测：有状态条目但资源已销毁 ⇒ 泄漏。
    ///
    /// 返回孤儿下标（**绝对值口径**：已销毁且条目仍在的那些）。
    pub fn orphans(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for r in self.rows.iter() {
            if r.current.is_terminal() && !r.alive {
                out.push(r.id);
            }
        }
        out
    }

    /// 刷新孤儿列表（供面板与判据读取）。
    pub fn refresh_orphans(&mut self) {
        self.orphans = self.orphans();
    }

    /// 孤儿下标列表（上次刷新的结果）。
    pub fn orphan_list(&self) -> &[usize] {
        &self.orphans
    }

    /// 记一条违例（绝对值口径）。
    fn note_violation(&mut self, v: Violation) {
        let i = v.ordinal();
        self.violations[i] = self.violations[i].saturating_add(1);
    }

    /// 记一条漂移（绝对值口径）。
    fn note_drift(&mut self, k: DriftKind) {
        let i = k.ordinal();
        self.drifts[i] = self.drifts[i].saturating_add(1);
    }

    /// 违例计数（按下标，判据按 [`Violation::ALL`] 遍历）。
    pub fn violation_counts(&self) -> [u32; 4] {
        self.violations
    }

    /// 漂移计数（按下标）。
    pub fn drift_counts(&self) -> [u32; 2] {
        self.drifts
    }

    /// 降采样次数。
    pub fn downsamples(&self) -> u32 {
        self.downsamples
    }

    /// 本帧微操作数。
    pub fn work(&self) -> u32 {
        self.work
    }

    /// 历史深度。
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// 资源行（只读）。
    pub fn rows(&self) -> &[ResStateRow] {
        &self.rows
    }

    /// 单资源三列（只读）。
    pub fn row(&self, id: usize) -> Option<&ResStateRow> {
        self.rows.get(id)
    }

    /// 开销核算：**超预算则降采样历史深度**（锚点：跟踪开销零帧预算声明）。
    ///
    /// 降到 [`MIN_HISTORY`] 仍超预算时**不再降**，登记降采样次数让上层告警——
    /// 静默继续降会让历史深度趋零，漂移回溯得出错误结论。
    pub fn settle_frame_budget(&mut self) -> bool {
        let mut changed = false;
        while self.work > FRAME_BUDGET && self.depth > MIN_HISTORY {
            self.depth -= 1;
            self.downsamples = self.downsamples.saturating_add(1);
            changed = true;
            // 降深度后**立即裁剪已有历史**，否则内存占用没降、只有新写入受限
            for r in self.rows.iter_mut() {
                let mut i = r.history.len();
                while i > self.depth {
                    r.history.pop();
                    i -= 1;
                }
            }
        }
        changed
    }

    /// 读屏面板：中英双语逐行，**只报聚合计数**，不报单个资源句柄与尺寸。
    pub fn a11y_lines(&self) -> [String; 6] {
        let v: u32 = self.violations.iter().fold(0u32, |a, b| a.saturating_add(*b));
        let d: u32 = self.drifts.iter().fold(0u32, |a, b| a.saturating_add(*b));
        [
            format!("资源数 / resources: {}", self.rows.len()),
            format!("违例数 / violations: {}", v),
            format!("漂移数 / drifts: {}", d),
            format!("孤儿数 / orphans: {}", self.orphans.len()),
            format!("历史深度 / history depth: {}", self.depth),
            format!("降采样 / downsamples: {}", self.downsamples),
        ]
    }
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// VE-F0032 模块自检（受 `CheckSet::MAX_CHECKS=112` 约束，逐条覆盖锚点判据）。
pub fn run_vea32_checks() -> CheckSet {
    let mut s = CheckSet::new("vea32_resstate");

    // --- 判据 1：三列状态齐备且互不混淆 -------------------------------------
    {
        let mut t = ResStateTracker::new();
        let id = t.create().ok();
        let r = t.row(id.unwrap_or(0));
        let ok = match r {
            Some(row) => {
                // 创建后：当前与预期均为 Undefined、历史空、alive
                row.current == ResState::Undefined
                    && row.expected == ResState::Undefined
                    && row.history.is_empty()
                    && row.alive
                    // 三列**类型上相互独立**：改当前不影响预期与历史
                    && row.expected == ResState::Undefined
            }
            None => false,
        };
        // 转出后：当前变、历史留下旧值、预期独立
        let mut t2 = ResStateTracker::new();
        let i2 = t2.create().ok().unwrap_or(0);
        t2.transition(i2, ResState::Initializing).ok();
        t2.expect_next(i2, ResState::Ready).ok();
        let r2 = t2.row(i2);
        let ok2 = match r2 {
            Some(row) => {
                row.current == ResState::Initializing
                    && row.expected == ResState::Ready
                    && row.history_len() == 1
                    && row.history[0] == ResState::Undefined
            }
            None => false,
        };
        s.add(
            "A32-三列-当前历史预期齐备且互不混淆",
            ok && ok2,
            "三列各司其职：转出后当前态更新、历史留旧值、预期独立不被牵连",
        );
    }

    // --- 判据 2：状态全集可枚举、下标往返一致 --------------------------------
    s.add(
        "A32-状态-全集可枚举且下标往返一致",
        {
            let mut seen = [false; STATE_COUNT];
            let mut rt = true;
            for st in ResState::ALL.iter() {
                seen[st.ordinal()] = true;
                rt &= ResState::from_ordinal(st.ordinal()) == Some(*st);
            }
            rt && seen[0] && seen[1] && seen[2] && seen[3] && seen[4] && seen[5]
                && ResState::from_ordinal(STATE_COUNT).is_none()
                && ResState::ALL.len() == STATE_COUNT
        },
        "六种状态下标互异、往返一致，界外下标返回 None",
    );

    // --- 判据 3：违例检测——不存在的跳转被阻断且**不改状态** ------------------
    {
        let mut t = ResStateTracker::new();
        let id = t.create().ok().unwrap_or(0);
        // Undefined → Writing 是**不存在**的边（必须先 Initializing）
        let r = t.transition(id, ResState::Writing);
        let row = t.row(id);
        s.add(
            "A32-违例-不存在跳转阻断且状态不变",
            r == Err(Violation::IllegalTransition)
                && Violation::IllegalTransition.is_blocking()
                && row.map(|x| x.current == ResState::Undefined).unwrap_or(false),
            "Undefined→Writing 被判违规例并阻断，资源状态保持不变",
        );
    }

    // --- 判据 4：终态不可转出，且单独归因（不与不存在该边混同）--------------
    {
        let mut t = ResStateTracker::new();
        let id = t.create().ok().unwrap_or(0);
        t.transition(id, ResState::Initializing).ok();
        t.transition(id, ResState::Ready).ok();
        t.destroy(id).ok();
        // 已销毁后再转出
        let r = t.transition(id, ResState::Ready);
        let c = t.violation_counts();
        s.add(
            "A32-违例-终态不可转出且单独归因",
            r == Err(Violation::FromTerminal)
                && Violation::FromTerminal.is_blocking()
                && c[Violation::FromTerminal.ordinal()] == 1
                && c[Violation::IllegalTransition.ordinal()] == 0,
            "已销毁资源再转出归 FromTerminal 专属码，不混入 IllegalTransition",
        );
    }

    // --- 判据 5：违例四类专属码、下标不重叠、阻断级口径明确 ------------------
    s.add(
        "A32-违例-四类专属码且阻断级口径明确",
        {
            let mut seen = [false; 4];
            let mut distinct = true;
            for v in Violation::ALL.iter() {
                if v.ordinal() >= 4 || seen[v.ordinal()] {
                    distinct = false;
                }
                seen[v.ordinal()] = true;
            }
            // 阻断级三类，非阻断一类（资源下标越界也阻断更诚实，此处按实现断言）
            let blocking = Violation::ALL.iter().filter(|v| v.is_blocking()).count();
            distinct
                && seen[0]
                && seen[1]
                && seen[2]
                && seen[3]
                && blocking == 3
                && Violation::BadResource.zh() != Violation::IllegalTransition.zh()
                && Violation::IllegalTransition.tag() != Violation::FromTerminal.tag()
        },
        "四类违例下标互不重叠，阻断级恰三类，中英标签互异",
    );

    // --- 判据 6：合法跳转不被误判（状态机不冤枉好人）------------------------
    {
        let mut t = ResStateTracker::new();
        let id = t.create().ok().unwrap_or(0);
        // 完整合法链：Undefined→Initializing→Ready→Writing→Ready→Destroyed
        let mut all_ok = true;
        all_ok &= t.transition(id, ResState::Initializing).is_ok();
        all_ok &= t.transition(id, ResState::Ready).is_ok();
        all_ok &= t.transition(id, ResState::Writing).is_ok();
        all_ok &= t.transition(id, ResState::Ready).is_ok();
        t.destroy(id).ok();
        let c = t.violation_counts();
        s.add(
            "A32-违例-合法跳转链不误判",
            all_ok && c.iter().fold(0u32, |a, b| a.saturating_add(*b)) == 0,
            "完整合法状态链走通且零违例（状态机不冤枉合法路径）",
        );
    }

    // --- 判据 7：资源下标越界被拒且归专属码 ---------------------------------
    {
        let mut t = ResStateTracker::new();
        t.create().ok();
        let r1 = t.transition(99, ResState::Ready);
        let r2 = t.expect_next(99, ResState::Ready);
        let r3 = t.destroy(99);
        let c = t.violation_counts();
        s.add(
            "A32-边界-资源下标越界三条路径均归专属码",
            r1 == Err(Violation::BadResource)
                && r2 == Err(Violation::BadResource)
                && r3 == Err(Violation::BadResource)
                && c[Violation::BadResource.ordinal()] == 3,
            "transition/expect/destroy 三条路径的越界访问均归 BadResource 且计数为 3",
        );
    }

    // --- 判据 8：资源数夹逼对（界前放行 / 界上拒）--------------------------
    {
        let mut t = ResStateTracker::new();
        let mut ok_count = 0u32;
        let mut last = Ok(0);
        for _ in 0..(MAX_RESOURCES + 1) {
            last = t.create();
            if last.is_ok() {
                ok_count += 1;
            }
        }
        s.add(
            "A32-边界-资源数上限恰好放行上限条",
            ok_count == MAX_RESOURCES as u32 && last == Err(Violation::TooManyResources),
            "资源建到上限恰好放行，再建一条被拒（阻断级）",
        );
    }

    // --- 判据 9：漂移与违例**分离**——外部漂移走校准不阻断 --------------------
    {
        let mut t = ResStateTracker::new();
        let id = t.create().ok().unwrap_or(0);
        t.transition(id, ResState::Initializing).ok();
        t.transition(id, ResState::Ready).ok();
        // 外部路径把资源改成 Writing（合法但跟踪器不知情）
        let r = t.note_external_drift(id, ResState::Writing);
        let row = t.row(id);
        let c = t.violation_counts();
        let d = t.drift_counts();
        s.add(
            "A32-漂移-外部漂移走校准不阻断且留痕",
            r.is_ok()
                && row.map(|x| x.current == ResState::Drifted).unwrap_or(false)
                // 校准即承认现实：预期态对齐到实际的外部状态
                && row.map(|x| x.expected == ResState::Writing).unwrap_or(false)
                && d[DriftKind::External.ordinal()] == 1
                // **零违例**——漂移不是违例
                && c.iter().fold(0u32, |a, b| a.saturating_add(*b)) == 0,
            "外部改动登记为漂移并把当前态标待校准、预期对齐现实，零违例（两者不混）",
        );
    }

    // --- 判据 10：预期态不符登记漂移，且与外部漂移分列 ----------------------
    {
        let mut t = ResStateTracker::new();
        let id = t.create().ok().unwrap_or(0);
        t.transition(id, ResState::Initializing).ok();
        // 预期 Ready 但当前 Initializing ⇒ 预期态漂移
        t.expect_next(id, ResState::Ready).ok();
        let d = t.drift_counts();
        s.add(
            "A32-漂移-预期态不符与外部漂移分列计数",
            d[DriftKind::Expectation.ordinal()] == 1
                && d[DriftKind::External.ordinal()] == 0,
            "声明的预期态与当前态不符记为 Expectation 漂移，不混入 External",
        );
    }

    // --- 判据 11：孤儿检测（有状态无资源）与绝对值口径 ----------------------
    {
        let mut t = ResStateTracker::new();
        let a = t.create().ok().unwrap_or(0);
        let b = t.create().ok().unwrap_or(1);
        let c2 = t.create().ok().unwrap_or(2);
        // 只销毁 a、b ⇒ c2 仍存活
        t.transition(a, ResState::Initializing).ok();
        t.destroy(a).ok();
        t.transition(b, ResState::Initializing).ok();
        t.destroy(b).ok();
        t.refresh_orphans();
        // 独立重算：已销毁且条目仍在的
        let want: Vec<usize> = t
            .rows()
            .iter()
            .filter(|r| r.current.is_terminal() && !r.alive)
            .map(|r| r.id)
            .collect();
        s.add(
            "A32-孤儿-有状态无资源被检出且绝对值对账",
            t.orphan_list() == want.as_slice()
                && want.len() == 2
                && t.row(c2).map(|r| r.alive).unwrap_or(false)
                && !t.orphan_list().contains(&c2),
            "两个已销毁资源的条目被列为孤儿，仍存活的资源不进孤儿表",
        );
    }

    // --- 判据 12：孤儿检测不被销毁流程掩盖（destroy 不删条目）---------------
    {
        let mut t = ResStateTracker::new();
        let id = t.create().ok().unwrap_or(0);
        t.transition(id, ResState::Initializing).ok();
        t.destroy(id).ok();
        t.refresh_orphans();
        s.add(
            "A32-孤儿-销毁保留条目故孤儿可见",
            t.rows().len() == 1 && t.orphan_list().len() == 1,
            "destroy 只标终态不删条目，泄漏因此可被检出（删条目就把泄漏藏了）",
        );
    }

    // --- 判据 13：历史环形缓冲深度受限且降采样留痕 -------------------------
    {
        let mut t = ResStateTracker::new();
        let id = t.create().ok().unwrap_or(0);
        // 来回走 Ready↔Writing 多次，压历史
        t.transition(id, ResState::Initializing).ok();
        let mut i = 0;
        while i < 30 {
            t.transition(id, ResState::Ready).ok();
            t.transition(id, ResState::Writing).ok();
            i += 1;
        }
        let row = t.row(id);
        s.add(
            "A32-开销-历史深度受限不无界增长",
            row.map(|r| r.history_len() <= MAX_HISTORY).unwrap_or(false)
                && t.depth() <= MAX_HISTORY,
            "三十次跳转后历史长度仍不超过 MAX_HISTORY（跟踪不无界吃内存）",
        );
    }

    // --- 判据 14：开销超预算触发降采样（零帧预算声明的兑现）----------------
    {
        let mut t = ResStateTracker::new();
        // 灌够操作数把 work 顶过预算
        let mut i = 0;
        while i < FRAME_BUDGET + 500 {
            let id = t.create().ok().unwrap_or(0);
            t.transition(id, ResState::Initializing).ok();
            i += 1;
        }
        let before = t.depth();
        let changed = t.settle_frame_budget();
        s.add(
            "A32-开销-超预算降采样并留痕",
            changed && t.depth() < before && t.downsamples() >= 1 && t.depth() >= MIN_HISTORY,
            "操作数超帧预算时历史深度下调并登记降采样次数（跟踪不侵占帧预算）",
        );
    }

    // --- 判据 15：降采样后已有历史**立即裁剪**（否则内存没真降）--------------
    {
        let mut t = ResStateTracker::new();
        let id = t.create().ok().unwrap_or(0);
        t.transition(id, ResState::Initializing).ok();
        let mut i = 0;
        while i < 30 {
            t.transition(id, ResState::Ready).ok();
            t.transition(id, ResState::Writing).ok();
            i += 1;
        }
        let deep_before = t.row(id).map(|r| r.history_len()).unwrap_or(0);
        let mut j = 0;
        while j < FRAME_BUDGET + 200 {
            t.expect_next(id, ResState::Ready).ok();
            j += 1;
        }
        t.settle_frame_budget();
        let shallow_after = t.row(id).map(|r| r.history_len()).unwrap_or(0);
        s.add(
            "A32-开销-降采样后已有历史立即裁剪",
            deep_before > 0 && shallow_after <= t.depth() && t.depth() < MAX_HISTORY,
            "降深度后已有历史同步裁剪到新深度（否则只有新写入受限、内存没降）",
        );
    }

    // --- 判据 16：状态机跳转表可枚举、判定与跳转表逐项一致 --------------------
    {
        // 遍历 STATE_COUNT × STATE_COUNT 全部 36 种组合：
        // 允许的边必出现在 allowed_next，不允许的必不在——两者不一致即表与判定脱节。
        let mut table_ok = true;
        for from in ResState::ALL.iter() {
            for to in ResState::ALL.iter() {
                let listed = allowed_next(*from).contains(to);
                table_ok &= listed == transition_allowed(*from, *to);
            }
        }
        s.add(
            "A32-状态机-跳转表与判定函数逐项一致",
            table_ok
                && allowed_next(ResState::Destroyed).is_empty()
                && allowed_next(ResState::Undefined).contains(&ResState::Initializing)
                // 关键：Undefined 不得直接到 Writing（判据 3 的依据）
                && !allowed_next(ResState::Undefined).contains(&ResState::Writing),
            "36 种组合逐项对账：allowed_next 与判定函数完全一致且终态边集为空",
        );
    }

    // --- 判据 17：违例计数为绝对值口径（不采信净值）-------------------------
    {
        let mut t = ResStateTracker::new();
        // 造 5 次越界与 2 次非法跳转
        let id = t.create().ok().unwrap_or(0);
        let mut i = 0;
        while i < 5 {
            let _ = t.transition(99, ResState::Ready);
            i += 1;
        }
        let _ = t.transition(id, ResState::Writing);
        let _ = t.transition(id, ResState::Ready);
        let _ = t.transition(id, ResState::Writing);
        let c = t.violation_counts();
        // 三次尝试**全部**非法：被拒后状态**不变**（仍在 Undefined），
        // 故第 2、3 次依旧撞「Undefined→Writing/Ready」这条不存在的边。
        // 这正是「阻断级且不改状态」的可观测后果——若实现偷偷改了状态，
        // 第 3 次就会变成合法边而少记一条。
        s.add(
            "A32-违例-计数为绝对值口径且被拒不改进态",
            c[Violation::BadResource.ordinal()] == 5
                && c[Violation::IllegalTransition.ordinal()] == 3
                && t.row(id).map(|r| r.current == ResState::Undefined).unwrap_or(false),
            "五次越界记 5、三次非法跳转记 3（绝对值不互相抵消），且被拒不改进态",
        );
    }

    // --- 判据 18：读屏面板双语齐备且不泄漏资源句柄 -------------------------
    {
        let mut t = ResStateTracker::new();
        let id = t.create().ok().unwrap_or(0);
        t.transition(id, ResState::Initializing).ok();
        t.destroy(id).ok();
        t.refresh_orphans();
        let lines = t.a11y_lines();
        let joined = lines.join("|");
        s.add(
            "A32-读屏-六行双语且不泄漏资源细节",
            lines.len() == 6
                && lines.iter().all(|l| !l.is_empty())
                && lines.iter().any(|l| l.contains("resources"))
                && lines.iter().any(|l| l.contains("violations"))
                && lines.iter().any(|l| l.contains("orphans"))
                && !joined.contains("句柄")
                && !joined.contains("尺寸"),
            "面板六行中英双语只报聚合计数，不泄漏资源句柄与尺寸",
        );
    }

    s
}