//! VE-F0051 · 全屏独占与无边框切换（VE-A 域 · A03 同步与呈现组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0051`
//!
//! **职责定位（锚点原文）**：全屏独占与无边框切换——FSE（Fullscreen
//! Exclusive）与 BFS（Borderless Fullscreen）的切换管理（各自的**能力与代价
//! 声明**），切换**原子事务**，独占模式的**失败回退**（被系统抢占时平滑回落）；
//! 含切换的**用户确认**（代价声明后确认再切）。数据结构：切换管理。
//!
//! **判据（锚点原文）**：双模式、代价声明、原子切换、被抢回落、判据。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 被抢占 → **平滑回落** | [`SwitchMachine::on_preempted`]：独占态被系统抢占（UAC 安全桌面/他应用请求独占等）**回落到无边框**（[`FALLBACK_MODE`]）而非直接回窗口——保住「占满屏」的视觉语义只让出独占权；回落计数与抢占事实留痕；**在途事务一并回滚**（半切换状态最危险） |
//! | 切换失败 → **回退** | 三段原子事务（校验→应用→确认，F0046/F0048 同款）：任一段注入失败即回滚，`current` 保持切换前值，失败计数留痕——「重建后随机黑屏」最常见的成因就是失败后留在半切换态 |
//! | 能力差异 → **诚实声明** | [`CAPABILITIES`] 是**公开代价表**（数据不是注释）：独占=最低延迟+绕过合成器+切出慢；无边框=合成器协调+切出快。差异诚实写死进表，用户按事实确认，判据逐格钉死防表被悄悄改 |
//!
//! **用户确认（锚点：代价声明后确认再切）**：[`plan_switch`] 在
//! `user_confirmed=false` 时拒绝并返 [`CODE_CONFIRM_REQUIRED`]——确认是
//! **前置闸门不是事后告知**：代价声明（[`CAPABILITIES`]）在前、确认在中、
//! 切换在后，顺序不可倒。同模式请求是 no-op（无事可确认，不制造假事务）。
//!
//! **性能逐项分解（锚点原文）**：O(1)——状态机三段各是常数次比较与赋值；
//! 预估无循环无分配；读屏行拼装是定容 7 行。
//!
//! **跨批对接点**：**V01 模式联动**（前向声明）——模式可用性
//! （[`SwitchMachine::available`]）的唯一权威在 V01 窗口管理（本条只持有
//! 事实副本并如实拒绝，不在本条内裁决「系统支不支持」）；F0046/F0047/F0048
//! 的呈现三件套是本条的下游（模式决定交换链形态——本条只出事实不出策略）。
//!
//! **无障碍与隐私（锚点原文）**：模式状态**读屏播报**（[`SwitchMachine::
//! a11y_lines`]）——中英双语七行，只报模式事实与聚合计数，不泄漏窗口
//! 标题与内容。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **三态不是两态**：FSE/BFS 之外必须给 Windowed 基态——「从窗口切独占」
//!   与「从无边框切独占」是同一动作，但「被抢占回落到哪」必须有明确答案
//!   （无边框），没有基态的切换管理答不了这个问题。
//! - **代价表是判据的靶子**：诚实声明若只是头注文字，改一行注释就撒谎了；
//!   写成 `const` 表并逐格断言（独占必须 `compositor_bypass=true`、无边框
//!   必须 `false`），表被改判据就红。
//! - **抢占回落写死无边框**：回落目标是**语义决策**不是随手默认——独占
//!   被抢时用户要的是「继续占满屏」，直接回窗口化等于把全屏应用摔在桌面上；
//!   回无边框保住画面语义，只交出独占权。
//! - **确认拒绝也是事实**：用户不确认（[`CODE_CONFIRM_REQUIRED`]）与系统
//!   拒绝（不支持/忙）分码——混码会让遥测把「用户不想切」读成「切不了」。
//! - **基线闸是原子性的承重墙**：回退目标是 `tx.from`，若建档时不核对
//!   `plan.from == current`，一份过期计划就能把回退基准写成「切换前从未处于
//!   的模式」（独占切换失败后回退到窗口化，等于把全屏应用摔在桌面上）。
//!   故 [`SwitchMachine::begin`] 强制核对基线，不符即拒（[`CODE_BASELINE_MISMATCH`]，
//!   与相位错同属调用方 bug 类：不计切换失败、不动现役）。no-op 豁免——无操作
//!   本就无起点一致性可谈。
//! - **零 panic 面**：查表走 `get`/match、计数全 `saturating_add`、无
//!   unwrap/expect——判据区同样约束。

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x4Bxx，独占——全仓 grep 零占用后选定；vea50 用 0x4A）
// ---------------------------------------------------------------------------

/// 帧号回退类输入违例（通用）。
pub const CODE_BAD_REQUEST: u16 = 0x4B01;
/// 切换失败（事务段注入失败，回退已执行）。
pub const CODE_SWITCH_FAILED: u16 = 0x4B02;
/// 目标模式当前不可用（V01 事实，诚实拒绝）。
pub const CODE_MODE_UNSUPPORTED: u16 = 0x4B03;
/// 切换忙（在途事务未结束，拒绝重入）。
pub const CODE_SWITCH_BUSY: u16 = 0x4B04;
/// 用户未确认（代价声明后的前置闸门，非系统拒绝）。
pub const CODE_CONFIRM_REQUIRED: u16 = 0x4B05;
/// 独占被系统抢占（信息级：回落事实留痕）。
pub const CODE_PREEMPTED: u16 = 0x4B06;
/// 切换基线不符：计划起点与现役模式不一致（调用方拿了过期计划）。
pub const CODE_BASELINE_MISMATCH: u16 = 0x4B07;

/// 本域诊断码全集（判据对账：互异 + 独占 0x4B 段）。
pub const CODES: [u16; 7] = [
    CODE_BAD_REQUEST,
    CODE_SWITCH_FAILED,
    CODE_MODE_UNSUPPORTED,
    CODE_SWITCH_BUSY,
    CODE_CONFIRM_REQUIRED,
    CODE_PREEMPTED,
    CODE_BASELINE_MISMATCH,
];

/// 人话说明（后果 + 下一步，不能只说「失败」；未知码有兜底不 panic）。
pub const fn explain(code: u16) -> &'static str {
    match code {
        CODE_BAD_REQUEST => "非法请求（相位错/非帧边界）：检查调用方状态机时序后重放",
        CODE_SWITCH_FAILED => "切换失败已回退：现役模式未变，按失败码定位注入段后重试",
        CODE_MODE_UNSUPPORTED => "目标模式当前不可用（V01 事实）：诚实声明不硬切，可先切其余模式",
        CODE_SWITCH_BUSY => "切换忙：上一事务未结束，串行化调用方逻辑后再发起",
        CODE_CONFIRM_REQUIRED => "用户未确认：先展示代价声明（CAPABILITIES），确认后再切",
        CODE_PREEMPTED => "独占被系统抢占：已平滑回落无边框，如需独占请用户确认后重切",
        CODE_BASELINE_MISMATCH => "切换基线不符：计划起点与现役模式不一致，重读现役模式后重新规划",
        // 兜底：码外值给人话而不是崩掉。
        _ => "未知切换诊断码（未登记）",
    }
}

// ---------------------------------------------------------------------------
// 二、三态封闭集（窗口 / 无边框 / 独占）
// ---------------------------------------------------------------------------

/// 显示模式（三态封闭集：锚点「双模式」= FSE/BFS，加窗口基态供回落语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayMode {
    /// 窗口化（基态）。
    Windowed,
    /// 无边框全屏（BFS，合成器协调）。
    Borderless,
    /// 全屏独占（FSE，绕过合成器）。
    Exclusive,
}

/// 在册模式表（判据对账基准：恰三态）。
pub const MODES: [DisplayMode; 3] =
    [DisplayMode::Windowed, DisplayMode::Borderless, DisplayMode::Exclusive];

impl DisplayMode {
    /// 序号（查表用；与 `from_ordinal` 互为逆）。
    pub const fn ordinal(self) -> usize {
        match self {
            DisplayMode::Windowed => 0,
            DisplayMode::Borderless => 1,
            DisplayMode::Exclusive => 2,
        }
    }

    /// 由序号还原（越界 `None`，不留默认兜底）。
    pub const fn from_ordinal(i: usize) -> Option<DisplayMode> {
        match i {
            0 => Some(DisplayMode::Windowed),
            1 => Some(DisplayMode::Borderless),
            2 => Some(DisplayMode::Exclusive),
            _ => None,
        }
    }

    /// 线上码（显式映射，禁 `as u8` 直转——枚举判别值与线约可能分叉）。
    pub const fn wire(self) -> u8 {
        match self {
            DisplayMode::Windowed => 0x01,
            DisplayMode::Borderless => 0x02,
            DisplayMode::Exclusive => 0x03,
        }
    }

    /// 由线上码还原。
    pub const fn from_wire(w: u8) -> Option<DisplayMode> {
        match w {
            0x01 => Some(DisplayMode::Windowed),
            0x02 => Some(DisplayMode::Borderless),
            0x03 => Some(DisplayMode::Exclusive),
            _ => None,
        }
    }

    /// 中英双语标签（读屏用）。
    pub const fn label(self) -> &'static str {
        match self {
            DisplayMode::Windowed => "窗口化 / windowed",
            DisplayMode::Borderless => "无边框全屏 / borderless",
            DisplayMode::Exclusive => "全屏独占 / exclusive",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、能力与代价声明（公开表——诚实声明是数据，不是头注散文）
// ---------------------------------------------------------------------------

/// 单模式能力与代价（一格一事实，判据逐格钉死）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeCapability {
    /// 所属模式。
    pub mode: DisplayMode,
    /// 是否绕过合成器（独占=是：DWM 不参与，延迟最低）。
    pub compositor_bypass: bool,
    /// 是否最低延迟档（独占=是；无边框走合成器多一跳）。
    pub lowest_latency: bool,
    /// 切出（Alt+Tab 等）是否快（独占=否：模式切换慢是独占的固有代价）。
    pub alt_tab_fast: bool,
    /// 是否依赖合成器协调呈现节奏。
    pub compositor_synced: bool,
}

/// 公开代价表（**诚实声明**：每格都判据钉死，改表必红）。
///
/// 这是 [`plan_switch`] 用户确认步骤展示的**全部事实来源**：确认之前用户
/// 看到的代价就是这张表，表外没有隐藏代价。
pub const CAPABILITIES: [ModeCapability; 3] = [
    ModeCapability {
        mode: DisplayMode::Windowed,
        compositor_bypass: false,
        lowest_latency: false,
        alt_tab_fast: true,
        compositor_synced: true,
    },
    ModeCapability {
        mode: DisplayMode::Borderless,
        compositor_bypass: false,
        lowest_latency: false,
        alt_tab_fast: true,
        compositor_synced: true,
    },
    ModeCapability {
        mode: DisplayMode::Exclusive,
        compositor_bypass: true,
        lowest_latency: true,
        alt_tab_fast: false,
        compositor_synced: false,
    },
];

/// 按模式查能力（O(1) 线性扫三格）。
pub const fn capability(mode: DisplayMode) -> Option<&'static ModeCapability> {
    let mut i = 0;
    while i < CAPABILITIES.len() {
        // const fn 里用 ordinal() 比较（PartialEq 的 == 不是 const 运算符）。
        if CAPABILITIES[i].mode.ordinal() == mode.ordinal() {
            return Some(&CAPABILITIES[i]);
        }
        i += 1;
    }
    None
}

/// 抢占回落目标（**语义决策写死**：独占被抢回无边框，保住「占满屏」语义）。
pub const FALLBACK_MODE: DisplayMode = DisplayMode::Borderless;

// ---------------------------------------------------------------------------
// 四、切换计划（用户确认前置闸门 + 能力校验）
// ---------------------------------------------------------------------------

/// 一次切换计划（no-op 计划合法：同模式无事可做，不制造假事务）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchPlan {
    /// 切换前模式。
    pub from: DisplayMode,
    /// 切换后模式。
    pub to: DisplayMode,
    /// 是否 no-op（from==to；noop 不需要确认、不开事务）。
    pub noop: bool,
}

/// 规划一次切换（锚点：代价声明后确认再切；全部拒绝路径显性分码）。
///
/// 拒绝优先级：no-op（合法直通）→ 未确认 → 不可用（V01 事实）→ 非帧边界。
/// 顺序即语义：确认闸在能力闸之前——用户没确认就不该知道「系统让不让」，
/// 先问意愿再看能力，避免把「用户不想」误报成「系统不让」。
pub fn plan_switch(
    from: DisplayMode,
    to: DisplayMode,
    user_confirmed: bool,
    at_frame_boundary: bool,
    available: &[bool; 3],
) -> Result<SwitchPlan, u16> {
    if from == to {
        return Ok(SwitchPlan { from, to, noop: true });
    }
    if !user_confirmed {
        return Err(CODE_CONFIRM_REQUIRED);
    }
    let ok = match available.get(to.ordinal()) {
        Some(v) => *v,
        None => false,
    };
    if !ok {
        return Err(CODE_MODE_UNSUPPORTED);
    }
    if !at_frame_boundary {
        return Err(CODE_BAD_REQUEST);
    }
    Ok(SwitchPlan { from, to, noop: false })
}

// ---------------------------------------------------------------------------
// 五、切换状态机（三段原子事务 + 被抢回落；O(1) 零分配）
// ---------------------------------------------------------------------------

/// 事务相位（校验 → 应用 → 确认；F0046/F0048 同款三段原子事务）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxPhase {
    /// 校验段。
    Validate,
    /// 应用段。
    Apply,
    /// 确认段。
    Confirm,
}

/// 一笔在途切换事务。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchTx {
    /// 切换前模式（回退基准）。
    pub from: DisplayMode,
    /// 目标模式。
    pub to: DisplayMode,
    /// 当前相位。
    pub phase: TxPhase,
}

/// 切换管理状态机（锚点「数据结构：切换管理」）。
///
/// 不变式：`tx == None` 时 `current` 是唯一现役事实；`tx == Some` 时
/// `current` 仍是现役事实（事务只在 confirm 成功后才改它，apply 只是
/// 预登记目标——回退永远只回到 `tx.from`）。
///
/// 该不变式的承重前提是 **`tx.from` 恒等于建档时的现役模式**，由
/// [`SwitchMachine::begin`] 的基线闸保证；缺了它，「回退只回到切换前」会
/// 退化成「回退到计划里声称的切换前」，后者可能从未发生过。
#[derive(Clone, Copy, Debug)]
pub struct SwitchMachine {
    current: DisplayMode,
    tx: Option<SwitchTx>,
    available: [bool; 3],
    switches: u32,
    preemptions: u32,
    fallbacks: u32,
    failures: u32,
    declines: u32,
    last_code: u16,
}

impl SwitchMachine {
    /// 新状态机（初始模式 + V01 可用性事实副本；出厂三态全可用）。
    pub const fn new(initial: DisplayMode) -> SwitchMachine {
        SwitchMachine {
            current: initial,
            tx: None,
            available: [true, true, true],
            switches: 0,
            preemptions: 0,
            fallbacks: 0,
            failures: 0,
            declines: 0,
            last_code: 0,
        }
    }

    /// 现役模式。
    pub const fn current(&self) -> DisplayMode {
        self.current
    }

    /// 在途事务。
    pub const fn tx(&self) -> Option<SwitchTx> {
        self.tx
    }

    /// V01 可用性事实（判据与调用方只读）。
    pub const fn available(&self) -> &[bool; 3] {
        &self.available
    }

    /// 各计数（切换成功 / 抢占 / 回落 / 失败 / 用户未确认）。
    pub const fn counters(&self) -> (u32, u32, u32, u32, u32) {
        (self.switches, self.preemptions, self.fallbacks, self.failures, self.declines)
    }

    /// 最近一次诊断码（0 = 无）。
    pub const fn last_code(&self) -> u16 {
        self.last_code
    }

    /// V01 回填可用性事实（本条不裁决，只如实持有——单一事实来源在 V01）。
    pub fn set_available(&mut self, mode: DisplayMode, ok: bool) {
        self.available[mode.ordinal()] = ok;
    }

    /// 发起事务（计划非 no-op 且相位合法才建档；在途即拒 [`CODE_SWITCH_BUSY`]）。
    ///
    /// **基线闸**：计划起点必须等于现役模式，否则 [`CODE_BASELINE_MISMATCH`]。
    /// 这不是多余的防御——回退目标是 `tx.from`，若建档时不核对基线，一份
    /// 过期计划（调用方在别处切过模式、或直接构造了 [`SwitchPlan`]）会把
    /// 回退基准写成「切换前从未处于的模式」：独占→独占切换失败后回退到
    /// 窗口化，应用被摔在桌面上——原子事务最忌讳的半切换态以另一种形式
    /// 复活。拒绝属**调用方 bug 类**（同相位错）：只拒本次调用，不开事务、
    /// 不计切换失败、不动现役——过期计划不是系统的失败，不该污染失败账。
    ///
    /// **闸序：先忙后基线**。在途事务下 `current` 可能已被 apply 改过，调用方
    /// 手里的计划起点自然变「旧」；此时先报基线不符就是**误导**——照它的话去
    /// 重新规划，重新规划照样撞上忙。真正拦路的只有一件事（忙），报它即可；
    /// 不忙时才轮到基线闸，此时「计划过期」才是准确诊断。两种拒绝都不开事务，
    /// 闸序只影响诊断码的可操作性，不影响闸本身的强度。
    ///
    /// no-op 不受两道闸约束：它不建档、不改任何状态，「无操作」本就无从谈
    /// 起点一致与否。
    pub fn begin(&mut self, plan: SwitchPlan) -> Result<(), u16> {
        if plan.noop {
            return Ok(()); // no-op 不开事务（同模式不制造假事务）
        }
        if self.tx.is_some() {
            self.last_code = CODE_SWITCH_BUSY;
            return Err(CODE_SWITCH_BUSY);
        }
        if plan.from != self.current {
            self.last_code = CODE_BASELINE_MISMATCH;
            return Err(CODE_BASELINE_MISMATCH);
        }
        self.tx = Some(SwitchTx { from: plan.from, to: plan.to, phase: TxPhase::Validate });
        Ok(())
    }

    fn fail_tx(&mut self, code: u16) -> u16 {
        self.tx = None; // 回退：现役 current 未动过，丢弃事务即回到切换前
        self.failures = self.failures.saturating_add(1);
        self.last_code = code;
        code
    }

    /// 校验段（`fail` 注入失败；非校验相位 [`CODE_BAD_REQUEST`]——相位错只
    /// 拒绝本次调用，**不清账不计失败**：跳段是调用方 bug，丢弃合法在途
    /// 事务等于让调用方 bug 摧毁正常状态）。
    pub fn tx_validate(&mut self, fail: Option<u16>) -> Result<(), u16> {
        let in_validate = matches!(self.tx, Some(t) if t.phase == TxPhase::Validate);
        if !in_validate {
            self.last_code = CODE_BAD_REQUEST;
            return Err(CODE_BAD_REQUEST);
        }
        if let Some(code) = fail {
            return Err(self.fail_tx(code));
        }
        if let Some(t) = self.tx.as_mut() {
            t.phase = TxPhase::Apply;
        }
        Ok(())
    }

    /// 应用段（成功才预登记目标；失败回退）。
    pub fn tx_apply(&mut self, fail: Option<u16>) -> Result<(), u16> {
        let in_apply = matches!(self.tx, Some(t) if t.phase == TxPhase::Apply);
        if !in_apply {
            self.last_code = CODE_BAD_REQUEST;
            return Err(CODE_BAD_REQUEST);
        }
        if let Some(code) = fail {
            return Err(self.fail_tx(code));
        }
        if let Some(t) = self.tx.as_ref() {
            self.current = t.to;
        }
        if let Some(t) = self.tx.as_mut() {
            t.phase = TxPhase::Confirm;
        }
        Ok(())
    }

    /// 确认段（成功封账：事务清空、成功计数 +1；失败回退到 `tx.from`）。
    pub fn tx_confirm(&mut self, fail: Option<u16>) -> Result<(), u16> {
        let in_confirm = matches!(self.tx, Some(t) if t.phase == TxPhase::Confirm);
        if !in_confirm {
            self.last_code = CODE_BAD_REQUEST;
            return Err(CODE_BAD_REQUEST);
        }
        if let Some(code) = fail {
            // apply 已预登记目标，回退必须显式还原（否则半切换态落地）。
            if let Some(t) = self.tx {
                self.current = t.from;
            }
            return Err(self.fail_tx(code));
        }
        self.tx = None;
        self.switches = self.switches.saturating_add(1);
        self.last_code = 0;
        Ok(())
    }

    /// 独占被系统抢占 → **平滑回落**到 [`FALLBACK_MODE`]（锚点降级第一条）。
    ///
    /// - 独占现役：回落 + 抢占/回落双计数 + [`CODE_PREEMPTED`] 留痕；
    ///   在途事务一并清账（独占权已失，事务必然无法完成）。
    /// - 非独占现役但在途事务涉及独占（from 或 to 是独占——含 apply 已
    ///   预登记的半切换态）：事务必然无法完成，清账 + 失败/抢占计数，
    ///   现役保持（含「预登记目标恰为回落目标」的巧合态——保持即语义正确）。
    /// - 其余（非独占且无独占相关事务）：无独占权可抢，状态不变。
    pub fn on_preempted(&mut self) -> DisplayMode {
        if self.current != DisplayMode::Exclusive {
            let involves_exclusive = matches!(self.tx, Some(t)
                if t.from == DisplayMode::Exclusive || t.to == DisplayMode::Exclusive);
            if involves_exclusive {
                self.tx = None;
                self.failures = self.failures.saturating_add(1);
                self.preemptions = self.preemptions.saturating_add(1);
                self.last_code = CODE_PREEMPTED;
            }
            return self.current;
        }
        // 独占现役：在途事务（若有）清账——独占权被抢，事务无完成可能。
        if self.tx.is_some() {
            self.tx = None;
            self.failures = self.failures.saturating_add(1);
        }
        self.current = FALLBACK_MODE;
        self.preemptions = self.preemptions.saturating_add(1);
        self.fallbacks = self.fallbacks.saturating_add(1);
        self.last_code = CODE_PREEMPTED;
        self.current
    }

    /// 用户确认前置闸门的拒绝也留计数（区分「用户不想切」与「系统不让切」）。
    pub fn note_declined(&mut self) {
        self.declines = self.declines.saturating_add(1);
    }
}

/// 读屏面板行数（双语 7 行，与 F0048~F0050 家族口径一致）。
pub const PANEL_LINES_F: usize = 7;

impl SwitchMachine {
    /// 模式状态读屏播报（锚点：模式状态读屏播报；中英双语，只报事实与计数）。
    pub fn a11y_lines(&self) -> [alloc::string::String; PANEL_LINES_F] {
        use alloc::format;
        let (sw, pre, fb, fail, dec) = self.counters();
        let in_tx = match self.tx() {
            Some(t) => format!(
                "切换中：{} → {} / switching: {} → {}",
                t.from.label(),
                t.to.label(),
                t.from.label(),
                t.to.label()
            ),
            None => alloc::string::String::from("无在途切换 / no switch in progress"),
        };
        [
            format!("当前显示模式：{} / current display mode: {}", self.current.label(), self.current.label()),
            format!("累计切换成功 {} 次 / successful switches: {}", sw, sw),
            format!("独占被抢占 {} 次 / preemptions: {}", pre, pre),
            format!("平滑回落 {} 次 / smooth fallbacks: {}", fb, fb),
            format!("切换失败回退 {} 次 / failed switch rollbacks: {}", fail, fail),
            format!("用户未确认 {} 次 / unconfirmed requests: {}", dec, dec),
            in_tx,
        ]
    }
}

// ---------------------------------------------------------------------------
// 六、域自检（判据逐条映射锚点：双模式/代价声明/原子切换/被抢回落/判据）
// ---------------------------------------------------------------------------

/// F0051 域自检入口（聚合器调用；零 panic 面，失败逐条红不炸域）。
pub fn run_vea51_checks() -> CheckSet {
    let mut s = CheckSet::new("vea51_fullscreen");

    // --- 判据 1：双模式（三态封闭集：wire/序号往返、表完整性） ---
    {
        let mut ok = true;
        for m in MODES {
            ok = ok && DisplayMode::from_wire(m.wire()) == Some(m);
            ok = ok && DisplayMode::from_ordinal(m.ordinal()) == Some(m);
        }
        ok = ok && DisplayMode::from_wire(0).is_none() && DisplayMode::from_wire(4).is_none();
        ok = ok && DisplayMode::from_ordinal(3).is_none();
        s.add("A51-双模式-三态封闭集往返", ok, "wire/序号往返一致，越界 None（不留兜底）");
        let labels = [MODES[0].label(), MODES[1].label(), MODES[2].label()];
        s.add(
            "A51-双模式-标签互异且双语",
            labels[0] != labels[1]
                && labels[1] != labels[2]
                && labels[0] != labels[2]
                && labels.iter().all(|l| l.contains(" / ")),
            "三标签互异且双语（读屏可达）",
        );
    }

    // --- 判据 2：代价声明（诚实声明逐格钉死，改表必红） ---
    {
        let win = capability(DisplayMode::Windowed);
        let bfs = capability(DisplayMode::Borderless);
        let fse = capability(DisplayMode::Exclusive);
        s.add(
            "A51-代价声明-三模式各有能力格",
            win.is_some() && bfs.is_some() && fse.is_some(),
            "CAPABILITIES 三模式各一格（缺格即伪声明）",
        );
        s.add(
            "A51-代价声明-独占格诚实",
            matches!(fse, Some(c)
                if c.compositor_bypass && c.lowest_latency && !c.alt_tab_fast && !c.compositor_synced),
            "独占：绕过合成器+最低延迟+切出慢+不依赖合成器（四格齐钉）",
        );
        s.add(
            "A51-代价声明-无边框格诚实",
            matches!(bfs, Some(c)
                if !c.compositor_bypass && !c.lowest_latency && c.alt_tab_fast && c.compositor_synced),
            "无边框：合成器协调+切出快（与独占逐格相反）",
        );
        s.add(
            "A51-代价声明-窗口格诚实",
            matches!(win, Some(c) if !c.compositor_bypass && c.alt_tab_fast && c.compositor_synced),
            "窗口基态：不绕合成器、切出快",
        );
    }

    // --- 判据 3：用户确认（前置闸门：先问意愿再看能力） ---
    {
        let mut m = SwitchMachine::new(DisplayMode::Windowed);
        let avail = [true, true, true];
        let p = plan_switch(DisplayMode::Windowed, DisplayMode::Exclusive, false, true, &avail);
        s.add(
            "A51-用户确认-未确认拒且分码",
            p == Err(CODE_CONFIRM_REQUIRED),
            "未确认拒绝且专属码（与系统拒绝分码）",
        );
        if p == Err(CODE_CONFIRM_REQUIRED) {
            m.note_declined();
        }
        let noop = plan_switch(DisplayMode::Windowed, DisplayMode::Windowed, false, true, &avail);
        s.add(
            "A51-用户确认-同模式no-op免确认",
            matches!(noop, Ok(pl) if pl.noop),
            "同模式 no-op 不需要确认（无事可确认，不制造假事务）",
        );
        let ok = plan_switch(DisplayMode::Windowed, DisplayMode::Exclusive, true, true, &avail);
        s.add(
            "A51-用户确认-确认后计划成立",
            matches!(ok, Ok(pl) if !pl.noop && pl.to == DisplayMode::Exclusive),
            "确认后计划成立（合规放行侧）",
        );
        s.add(
            "A51-用户确认-拒绝计数留痕",
            m.counters().4 == 1,
            "确认拒绝计数可查（用户主权事实，不是错误）",
        );
    }

    // --- 判据 4：能力差异（V01 可用性事实诚实拒绝，不做硬切） ---
    {
        let mut avail = [true, true, true];
        avail[DisplayMode::Exclusive.ordinal()] = false;
        let p = plan_switch(DisplayMode::Windowed, DisplayMode::Exclusive, true, true, &avail);
        s.add(
            "A51-能力差异-不可用诚实拒绝",
            p == Err(CODE_MODE_UNSUPPORTED),
            "V01 报不可用即拒绝（诚实声明，不硬切）",
        );
        let p2 = plan_switch(
            DisplayMode::Windowed,
            DisplayMode::Borderless,
            true,
            true,
            &avail,
        );
        s.add(
            "A51-能力差异-其余模式不受牵连",
            p2.is_ok(),
            "独占不可用不妨碍切无边框（拒绝是按模式不是全局）",
        );
        let p3 = plan_switch(DisplayMode::Windowed, DisplayMode::Borderless, true, false, &avail);
        s.add(
            "A51-能力差异-非帧边界拒绝",
            p3 == Err(CODE_BAD_REQUEST),
            "非帧边界切换拒绝（贴线不误拒：边界处合法）",
        );
    }

    // --- 判据 5：原子切换（三段事务：全绿 / 各段失败回退 / 忙 / 相位错） ---
    {
        // 全绿主流程
        let mut m = SwitchMachine::new(DisplayMode::Windowed);
        let plan = plan_switch(
            DisplayMode::Windowed,
            DisplayMode::Exclusive,
            true,
            true,
            m.available(),
        );
        let ok = match plan {
            Ok(p) => {
                m.begin(p) == Ok(())
                    && m.tx_validate(None) == Ok(())
                    && m.tx_apply(None) == Ok(())
                    && m.current() == DisplayMode::Exclusive
                    && m.tx_confirm(None) == Ok(())
                    && m.current() == DisplayMode::Exclusive
                    && m.tx().is_none()
                    && m.counters().0 == 1
            }
            Err(_) => false,
        };
        s.add("A51-原子切换-三段全绿主流程", ok, "校验→应用→确认后事务清账、成功计数+1");
        // apply 段失败：current 未动（apply 前不改现役）
        let mut m2 = SwitchMachine::new(DisplayMode::Windowed);
        if let Ok(p) = plan_switch(
            DisplayMode::Windowed,
            DisplayMode::Exclusive,
            true,
            true,
            m2.available(),
        ) {
            let _ = m2.begin(p);
            let _ = m2.tx_validate(None);
            let r = m2.tx_apply(Some(CODE_SWITCH_FAILED));
            s.add(
                "A51-原子切换-应用段失败回退",
                r == Err(CODE_SWITCH_FAILED)
                    && m2.current() == DisplayMode::Windowed
                    && m2.tx().is_none()
                    && m2.counters().3 == 1,
                "应用段失败：现役不变、事务清空、失败计数+1",
            );
        }
        // confirm 段失败：显式还原 apply 预登记（半切换态必须回滚）
        let mut m3 = SwitchMachine::new(DisplayMode::Windowed);
        if let Ok(p) = plan_switch(
            DisplayMode::Windowed,
            DisplayMode::Exclusive,
            true,
            true,
            m3.available(),
        ) {
            let _ = m3.begin(p);
            let _ = m3.tx_validate(None);
            let _ = m3.tx_apply(None);
            let r = m3.tx_confirm(Some(CODE_SWITCH_FAILED));
            s.add(
                "A51-原子切换-确认段失败还原预登记",
                r == Err(CODE_SWITCH_FAILED)
                    && m3.current() == DisplayMode::Windowed
                    && m3.counters().3 == 1,
                "确认段失败：apply 预登记被显式还原（半切换态不落地）",
            );
        }
        // 忙：在途事务拒绝重入
        let mut m4 = SwitchMachine::new(DisplayMode::Windowed);
        if let Ok(p) = plan_switch(
            DisplayMode::Windowed,
            DisplayMode::Exclusive,
            true,
            true,
            m4.available(),
        ) {
            let _ = m4.begin(p);
            let again = plan_switch(
                DisplayMode::Exclusive,
                DisplayMode::Borderless,
                true,
                true,
                m4.available(),
            );
            let begin_again = match again {
                Ok(p2) => m4.begin(p2) == Err(CODE_SWITCH_BUSY),
                Err(_) => false,
            };
            s.add(
                "A51-原子切换-在途事务拒绝重入",
                begin_again && m4.tx().is_some(),
                "切换忙分码拒绝，原事务不受影响",
            );
        }
        // 相位错：跳段调用拒绝
        let mut m5 = SwitchMachine::new(DisplayMode::Windowed);
        if let Ok(p) = plan_switch(
            DisplayMode::Windowed,
            DisplayMode::Exclusive,
            true,
            true,
            m5.available(),
        ) {
            let _ = m5.begin(p);
            let skip = m5.tx_confirm(None) == Err(CODE_BAD_REQUEST);
            let still = m5.tx().is_some();
            s.add(
                "A51-原子切换-跳段拒绝",
                skip && still,
                "相位错拒绝（跳段不推进也不清账）",
            );
        }
        // no-op 计划不开事务
        let mut m6 = SwitchMachine::new(DisplayMode::Borderless);
        let noop = plan_switch(
            DisplayMode::Borderless,
            DisplayMode::Borderless,
            false,
            true,
            m6.available(),
        );
        let ok6 = match noop {
            Ok(p) => m6.begin(p) == Ok(()) && m6.tx().is_none() && m6.counters().0 == 0,
            Err(_) => false,
        };
        s.add("A51-原子切换-noop不开事务", ok6, "同模式 no-op：零事务零计数（不制造假事务）");
    }

    // --- 判据 5b：基线闸（回退基准必须等于切换前现役） ---
    //
    // 本组是承重判据：M1 变异把 begin 的基线核对删掉后，下面「过期计划被拒」
    // 一条立刻转红——因为不核对基线时，过期计划能建档，其 `from` 会被写进
    // tx，确认段失败时按 `tx.from` 还原，把应用回退到一个**从未处于**的模式。
    {
        // 现役真值 Borderless；过期计划声称起点是 Windowed。
        let mut m = SwitchMachine::new(DisplayMode::Borderless);
        let stale = SwitchPlan {
            from: DisplayMode::Windowed,
            to: DisplayMode::Exclusive,
            noop: false,
        };
        let r = m.begin(stale);
        s.add(
            "A51-基线闸-过期计划被拒且零副作用",
            r == Err(CODE_BASELINE_MISMATCH)
                && m.tx().is_none()
                && m.current() == DisplayMode::Borderless
                && m.counters() == (0, 0, 0, 0, 0)
                && m.last_code() == CODE_BASELINE_MISMATCH,
            "计划起点≠现役即拒（专属码）：不开事务、不动现役、切换成功/抢占/回落/失败/未确认五账全零——过期计划是调用方 bug 不是系统失败",
        );
        // 拒后机器仍可正常干活（健康度未被破坏）。
        let good = plan_switch(
            DisplayMode::Borderless,
            DisplayMode::Exclusive,
            true,
            true,
            m.available(),
        );
        let recovered = match good {
            Ok(p) => m.begin(p) == Ok(())
                && m.tx_validate(None) == Ok(())
                && m.tx_apply(None) == Ok(())
                && m.tx_confirm(None) == Ok(())
                && m.current() == DisplayMode::Exclusive
                && m.counters().0 == 1,
            Err(_) => false,
        };
        s.add(
            "A51-基线闸-拒绝后状态机仍健康",
            recovered,
            "基线闸拒绝后重新规划即可正常切换（三段全绿、成功计数 1），闸不留残状态",
        );
        // no-op 不受基线闸约束（起点一致性与「无操作」无关）。
        let mut m2 = SwitchMachine::new(DisplayMode::Exclusive);
        let noop_stale = SwitchPlan {
            from: DisplayMode::Windowed,
            to: DisplayMode::Windowed,
            noop: true,
        };
        s.add(
            "A51-基线闸-noop豁免不误伤",
            m2.begin(noop_stale) == Ok(()) && m2.tx().is_none() && m2.current() == DisplayMode::Exclusive,
            "no-op 计划不建档不受基线闸约束（无操作本就无起点一致性可谈）",
        );
        // 闸序：在途事务优先报忙（而非基线不符）——照基线不符去重规划仍会撞忙。
        let mut m4 = SwitchMachine::new(DisplayMode::Windowed);
        if let Ok(p) = plan_switch(
            DisplayMode::Windowed,
            DisplayMode::Exclusive,
            true,
            true,
            m4.available(),
        ) {
            let _ = m4.begin(p);
            // 事务在途时 current 仍是 Windowed；此计划起点恰也是 Windowed
            // （基线其实相符），报忙才是唯一可操作的诊断。
            let again = plan_switch(
                DisplayMode::Windowed,
                DisplayMode::Borderless,
                true,
                true,
                m4.available(),
            );
            let busy_first = match again {
                Ok(p2) => m4.begin(p2) == Err(CODE_SWITCH_BUSY),
                Err(_) => false,
            };
            s.add(
                "A51-基线闸-闸序先忙后基线",
                busy_first && m4.tx().is_some() && m4.last_code() == CODE_SWITCH_BUSY,
                "在途事务优先报忙：调用方照基线不符去重规划仍会撞忙，报忙才可操作；原事务不受影响",
            );
        }
        // 回退基准 == 切换前现役：正常路径下 tx.from 恒等于建档时现役。
        let mut m3 = SwitchMachine::new(DisplayMode::Borderless);
        let before = m3.current();
        if let Ok(p) = plan_switch(
            DisplayMode::Borderless,
            DisplayMode::Exclusive,
            true,
            true,
            m3.available(),
        ) {
            let _ = m3.begin(p);
            let _ = m3.tx_validate(None);
            let _ = m3.tx_apply(None);
            let _ = m3.tx_confirm(Some(CODE_SWITCH_FAILED));
            s.add(
                "A51-基线闸-回退基准即切换前现役",
                m3.current() == before
                    && m3.tx().is_none()
                    && m3.counters().3 == 1
                    && m3.counters().0 == 0,
                "确认段失败回退到的恰是切换前现役模式（基线闸保证 tx.from 不会撒谎），失败计数 1、成功计数 0",
            );
        }
    }

    // --- 判据 6：被抢回落（独占回落无边框、计数留痕、在途一并回滚） ---
    {
        let mut m = SwitchMachine::new(DisplayMode::Exclusive);
        let after = m.on_preempted();
        s.add(
            "A51-被抢回落-独占回落无边框",
            after == FALLBACK_MODE
                && after == DisplayMode::Borderless
                && m.current() == DisplayMode::Borderless,
            "抢占回落目标是语义决策（FALLBACK_MODE 写死无边框）",
        );
        s.add(
            "A51-被抢回落-抢占回落双计数",
            m.counters().1 == 1 && m.counters().2 == 1 && m.last_code() == CODE_PREEMPTED,
            "抢占与回落事实分别留痕（回落不是失败也不是切换成功）",
        );
        // 非独占态抢占：状态不变
        let mut m2 = SwitchMachine::new(DisplayMode::Windowed);
        let after2 = m2.on_preempted();
        s.add(
            "A51-被抢回落-非独占态状态不变",
            after2 == DisplayMode::Windowed
                && m2.current() == DisplayMode::Windowed
                && m2.counters().2 == 0,
            "无独占权可抢：不产生虚假回落",
        );
        // 在途事务遇抢占一并回滚
        let mut m3 = SwitchMachine::new(DisplayMode::Exclusive);
        if let Ok(p) = plan_switch(
            DisplayMode::Exclusive,
            DisplayMode::Borderless,
            true,
            true,
            m3.available(),
        ) {
            let _ = m3.begin(p);
            let _ = m3.tx_validate(None);
            let _ = m3.tx_apply(None); // 已预登记 Exclusive→Borderless
            let after3 = m3.on_preempted();
            s.add(
                "A51-被抢回落-在途事务一并回滚",
                after3 == DisplayMode::Borderless
                    && m3.tx().is_none()
                    && m3.current() == DisplayMode::Borderless
                    && m3.counters().3 == 1,
                "半切换态最危险：抢占时在途事务显式回滚（预登记还原）",
            );
        }
        // 独占现役 + 事务在校验段遇抢占：现役直接回落、事务清账。
        // （与上一条互补：上一条是 apply 后 current 已非独占的半切换态，
        //  这一条是 current 仍独占的在途态——两条都断，独占分支的清账
        //  才有承重判据，M5 变异曾因缺这条而幸存。）
        let mut m4 = SwitchMachine::new(DisplayMode::Exclusive);
        if let Ok(p) = plan_switch(
            DisplayMode::Exclusive,
            DisplayMode::Borderless,
            true,
            true,
            m4.available(),
        ) {
            let _ = m4.begin(p);
            let _ = m4.tx_validate(None); // 事务在 Apply 相位，current 仍独占
            let after4 = m4.on_preempted();
            s.add(
                "A51-被抢回落-独占现役在途态遇抢占",
                after4 == DisplayMode::Borderless
                    && m4.current() == DisplayMode::Borderless
                    && m4.tx().is_none()
                    && m4.counters().1 == 1
                    && m4.counters().2 == 1
                    && m4.counters().3 == 1,
                "独占现役+在途事务遇抢占：回落无边框、事务清账、抢占/回落/失败三计数齐",
            );
        }
    }

    // --- 判据 7：读屏播报（双语 7 行、含现役模式、O(1) 定容） ---
    {
        let m = SwitchMachine::new(DisplayMode::Exclusive);
        let lines = m.a11y_lines();
        let ok = lines.len() == PANEL_LINES_F
            && lines.iter().all(|l| !l.is_empty())
            && lines[0].contains(DisplayMode::Exclusive.label())
            && lines.iter().all(|l| l.contains("/"));
        s.add(
            "A51-读屏-七行双语含现役模式",
            ok,
            "定容 7 行、行行非空、含现役模式与双语分隔（读屏可达）",
        );
    }

    // --- 判据 8：判据（诊断码互异 + 段独占 + 兜底 + 条数对账） ---
    {
        let mut ok = true;
        let mut i = 0;
        while i < CODES.len() {
            let mut j = i + 1;
            while j < CODES.len() {
                if CODES[i] == CODES[j] {
                    ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        s.add("A51-判据-码位两两互异", ok, "七码互异（按码归类的前提）");
        let seg_ok = CODES.iter().all(|c| c & 0xFF00 == 0x4B00);
        s.add("A51-判据-码段独占0x4B", seg_ok, "全码独占 0x4B 段（与 0x4A/vea50 互斥）");
        s.add(
            "A51-判据-未知码兜底不panic",
            !explain(0x4BFF).is_empty() && explain(CODE_BAD_REQUEST) != explain(0x4BFF),
            "未知码有兜底人话（不崩也不静默）",
        );
        s.add(
            "A51-判据-基线码在册且有独立人话",
            CODES.contains(&CODE_BASELINE_MISMATCH)
                && explain(CODE_BASELINE_MISMATCH) != explain(CODE_BAD_REQUEST)
                && explain(CODE_BASELINE_MISMATCH) != explain(CODE_SWITCH_BUSY)
                && CODE_BASELINE_MISMATCH & 0xFF00 == 0x4B00,
            "基线不符码在 CODES 全表内、有专属人话（不与相位错/忙混码——混码会把「计划过期」读成「切不了」）",
        );
        s.add(
            "A51-判据-条数对账",
            s.len() == 34,
            "判据条数恰 35（本条执行前已有 34 条，防悄悄增删）",
        );
    }

    s
}

// 引入判据层类型。
use crate::checks::CheckSet;
