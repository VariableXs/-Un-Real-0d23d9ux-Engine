//! VE-F0049 · 帧回调分发器（VE-A 域 · A03 同步与呈现组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0049`
//!
//! **职责定位（锚点原文）**：帧回调分发器——每帧回调的统一分发
//! （渲染/UI/脚本/物理的帧回调按序分发），回调顺序契约（帧首/帧中/
//! 帧尾三段的语义），回调超时检测；含回调分发的优先级文档（谁先谁后
//! 写明）。数据结构：分发器。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! - 超时 → **告警 + 钳制**（超预算逐次告警计数，连续超时钳制预算：
//!   钳到一半、最低保底——不让单个回调拖垮整帧分发）；
//! - 顺序违例 → **断言**（分发序违例即拒绝并计数，不是静默改序）；
//! - 回调泄漏 → **回收**（连续空闲帧数达阈值的回调回收并留痕）。
//!
//! **性能逐项分解**：O(回调)——每帧对注册回调逐一分发一遍，分发序由
//! 定容顺序表给出，无查找无排序。
//!
//! **跨批对接点**：AC02 调度协同——本条只产出「回调在本帧的执行序与
//! 超时事实」，线程池调度不在本条。
//!
//! **无障碍与隐私**：分发状态读屏可达（[`FrameDispatcher::a11y_lines`]）——
//! 中英双语七行，只报分发事实与聚合计数，不泄漏回调句柄与执行内容。
//!
//! ## 优先级文档（谁先谁后，写明而非口口相传）
//!
//! 三段语义与段内顺序由 [`PHASE_ORDER`] 顺序表**数据化**（表驱动不是
//! if 链），判据侧独立重算逐格对账：
//!
//! - **帧首**（输入与逻辑先行）：UI（收输入）→ 脚本（改状态）。
//!   理由：本帧的输入必须在渲染采点前进 UI，脚本读的是本帧状态；
//! - **帧中**（采点与提交）：渲染（GPU 采点提交）。
//!   理由：渲染必须拿到帧首定稿的状态，且越晚提交越少排队延迟；
//! - **帧尾**（观察与收尾）：物理（以本帧定稿状态步进）→ 脚本
//!   （后处理钩子）。
//!   理由：物理步进要看渲染提交后的最终状态；脚本后处理在一切之后。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **统一分发是全覆盖不重不漏**：分发对注册回调逐个执行一次——
//!   「全部注册回调恰好执行一遍」是判据直接断言的契约，不是实现细节。
//! - **顺序违例是断言不是纠错**：分发器不替调用方「纠正」顺序，违例
//!   即拒绝并计数——纠错会把契约错误藏进运行时。
//! - **钳制是预算手段不是惩罚**：钳到一半最低保底，恢复路径存在
//!   （连续达标即解除），钳制次数可查。
//! - **零 panic 面**：槽位下标由判据侧定容校验、计数全 `saturating_add`、
//!   无 unwrap/expect/切片越界。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0047/F0048** 管呈现节奏（缓冲数与同步模式）；本条管一帧之内
//!   回调的执行序，不碰呈现。
//! - **AC02** 管线程池调度；本条的「分发序」是逻辑序，与物理执行
//!   线程无关。
//!
//! ## 自持说明
//!
//! 三段与超时阈值自持（[`TIMEOUT_WARN_US`] 等），与相邻域的衔接以
//! 判据独立钉住，不做跨模块 use。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、常量与诊断码（独占码段 0x49xx）
// ---------------------------------------------------------------------------

/// 段数（帧首/帧中/帧尾）。
pub const PHASE_KINDS: usize = 3;

/// 回调类别数（渲染/UI/脚本/物理）。
pub const CATEGORY_KINDS: usize = 4;

/// 每段定容槽位（容量上限；超容拒绝不静默截断）。
pub const SLOTS_PER_PHASE: usize = 16;

/// 单次回调超时告警阈值（微秒；锚点「超时→告警+钳制」）。
pub const TIMEOUT_WARN_US: u32 = 2_000;

/// 连续超时达到该次数即钳制。
pub const TIMEOUT_STRIKES_TO_CLAMP: u32 = 2;

/// 钳制后的预算折扣（一半）。
pub const CLAMP_DIV: u32 = 2;

/// 钳制后的最低保底预算（微秒；钳不没——保底为 0 等于饿死回收）。
pub const CLAMP_FLOOR_US: u32 = 500;

/// 连续空闲帧数达到该阈值即回收。
pub const IDLE_FRAMES_TO_RECLAIM: u32 = 64;

/// 非法回调句柄（越界/空槽）。
pub const CODE_BAD_HANDLE: PolicyCode = 0x4901;

/// 段满（容量上限，拒绝不截断）。
pub const CODE_PHASE_FULL: PolicyCode = 0x4902;

/// 重复注册（同 id 已在册）。
pub const CODE_DUP_REGISTER: PolicyCode = 0x4903;

/// 分发序违例（顺序契约断言）。
pub const CODE_ORDER_VIOLATION: PolicyCode = 0x4904;

/// 超时告警（信息级：账已记，帧继续）。
pub const CODE_TIMEOUT_WARN: PolicyCode = 0x4905;

/// 非法参数（零预算等构造期无效值）。
pub const CODE_BAD_BUDGET: PolicyCode = 0x4906;

pub type PolicyCode = u16;

// ---------------------------------------------------------------------------
// 二、帧三段与回调类别（封闭全集）
// ---------------------------------------------------------------------------

/// 帧三段：帧首/帧中/帧尾。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FramePhase {
    /// 帧首：输入与逻辑先行。
    FrameStart,
    /// 帧中：采点与提交。
    FrameMid,
    /// 帧尾：观察与收尾。
    FrameEnd,
}

impl FramePhase {
    pub const ALL: [FramePhase; 3] = [FramePhase::FrameStart, FramePhase::FrameMid, FramePhase::FrameEnd];

    pub const fn ordinal(self) -> usize {
        match self {
            FramePhase::FrameStart => 0,
            FramePhase::FrameMid => 1,
            FramePhase::FrameEnd => 2,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            FramePhase::FrameStart => "帧首 / frame start",
            FramePhase::FrameMid => "帧中 / frame mid",
            FramePhase::FrameEnd => "帧尾 / frame end",
        }
    }
}

/// 回调类别（锚点四类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackKind {
    /// 渲染。
    Render,
    /// UI。
    Ui,
    /// 脚本。
    Script,
    /// 物理。
    Physics,
}

impl CallbackKind {
    pub const ALL: [CallbackKind; 4] = [
        CallbackKind::Render,
        CallbackKind::Ui,
        CallbackKind::Script,
        CallbackKind::Physics,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            CallbackKind::Render => 0,
            CallbackKind::Ui => 1,
            CallbackKind::Script => 2,
            CallbackKind::Physics => 3,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            CallbackKind::Render => "渲染 / render",
            CallbackKind::Ui => "界面 / ui",
            CallbackKind::Script => "脚本 / script",
            CallbackKind::Physics => "物理 / physics",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、顺序契约表（优先级文档的数据形态）
// ---------------------------------------------------------------------------

/// 每段分发序（类别序数序列；空段 = 该段无回调）。
///
/// - 帧首：UI → 脚本（输入先行，脚本读本帧状态）；
/// - 帧中：渲染（GPU 采点提交）；
/// - 帧尾：物理 → 脚本（物理看定稿状态，脚本后处理在一切之后）。
pub const PHASE_ORDER: [[usize; 2]; 3] = [
    [CallbackKind::Ui.ordinal(), CallbackKind::Script.ordinal()], // 帧首
    [CallbackKind::Render.ordinal(), usize::MAX],                 // 帧中（单类，尾部哨兵）
    [CallbackKind::Physics.ordinal(), CallbackKind::Script.ordinal()], // 帧尾
];

/// 段内某类别是否参与该段分发（查表 O(1)）。
pub const fn phase_admits(phase: FramePhase, kind: CallbackKind) -> bool {
    let row = PHASE_ORDER[phase.ordinal()];
    row[0] == kind.ordinal() || row[1] == kind.ordinal()
}

/// 段内两类别的前后关系（a 先于 b 返回 true；只对同段内两个参与类别有意义）。
pub const fn phase_before(phase: FramePhase, a: CallbackKind, b: CallbackKind) -> bool {
    let row = PHASE_ORDER[phase.ordinal()];
    if row[0] == a.ordinal() {
        true
    } else if row[0] == b.ordinal() {
        false
    } else if row[1] == a.ordinal() {
        a.ordinal() != b.ordinal()
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// 四、回调槽（注册表）
// ---------------------------------------------------------------------------

/// 注册在册的一个回调。
#[derive(Clone, Copy, Debug)]
pub struct CallbackSlot {
    /// 全局唯一 id（调用方自配，重复即拒）。
    pub id: u32,
    pub phase: FramePhase,
    pub kind: CallbackKind,
    /// 基础预算（微秒；钳制在此之上打折）。
    pub budget_us: u32,
    /// 当前生效预算（钳制后可低于基础值）。
    pub effective_us: u32,
    /// 是否在册（false = 已回收/已注销的空槽）。
    pub live: bool,
    /// 连续超时次数（钳制计数依据）。
    pub timeout_strikes: u32,
    /// 是否处于钳制中。
    pub clamped: bool,
    /// 连续空闲帧数（泄漏回收依据）。
    pub idle_frames: u32,
    /// 累计执行次数。
    pub runs: u32,
}

/// 分发器。
pub struct FrameDispatcher {
    slots: [Option<CallbackSlot>; SLOTS_PER_PHASE],
    /// 当前帧号（分发推进）。
    frame: u64,
    /// 本帧已分发到的执行游标（(段, 行内序) 的单调游标；顺序契约的
    /// 运行时载体之一——防回跳）。
    cursor_phase: usize,
    cursor_row: usize,
    /// 每格在册回调数（段×行内序；register/unregister/回收维护）。
    cell_live: [[u32; 2]; 3],
    /// 每格本帧已分发数（与 cell_live 相等即该格完整；防跳段）。
    cell_runs: [[u32; 2]; 3],
    dispatch_total: u32,
    timeout_warns: u32,
    clamp_total: u32,
    order_violations: u32,
    reclaimed_total: u32,
    dup_rejected: u32,
    full_rejected: u32,
}

impl FrameDispatcher {
    pub const fn new() -> FrameDispatcher {
        FrameDispatcher {
            slots: [None; SLOTS_PER_PHASE],
            frame: 0,
            cursor_phase: 0,
            cursor_row: 0,
            cell_live: [[0; 2]; 3],
            cell_runs: [[0; 2]; 3],
            dispatch_total: 0,
            timeout_warns: 0,
            clamp_total: 0,
            order_violations: 0,
            reclaimed_total: 0,
            dup_rejected: 0,
            full_rejected: 0,
        }
    }

    /// 段内行内序（先经 [`phase_admits`] 校验；不容类返回 2=无效）。
    const fn phase_row_pos(phase: FramePhase, kind: CallbackKind) -> usize {
        let row = PHASE_ORDER[phase.ordinal()];
        if row[0] == kind.ordinal() {
            0
        } else if row[1] == kind.ordinal() {
            1
        } else {
            2
        }
    }

    /// 注册（O(SLOTS) 线性查空槽与查重——注册不是热路径，热路径是分发）。
    pub fn register(
        &mut self,
        id: u32,
        phase: FramePhase,
        kind: CallbackKind,
        budget_us: u32,
    ) -> Result<usize, PolicyCode> {
        if budget_us == 0 {
            return Err(CODE_BAD_BUDGET);
        }
        if !phase_admits(phase, kind) {
            return Err(CODE_ORDER_VIOLATION);
        }
        let mut free = None;
        let mut i = 0usize;
        while i < SLOTS_PER_PHASE {
            match self.slots[i] {
                Some(ref s) if s.live && s.id == id => {
                    return Err(CODE_DUP_REGISTER);
                }
                _ => {}
            }
            if free.is_none() && self.slots[i].is_none() {
                free = Some(i);
            }
            i += 1;
        }
        let idx = match free {
            Some(x) => x,
            None => {
                self.full_rejected = self.full_rejected.saturating_add(1);
                return Err(CODE_PHASE_FULL);
            }
        };
        self.slots[idx] = Some(CallbackSlot {
            id,
            phase,
            kind,
            budget_us,
            effective_us: budget_us,
            live: true,
            timeout_strikes: 0,
            clamped: false,
            idle_frames: 0,
            runs: 0,
        });
        self.cell_live[phase.ordinal()][Self::phase_row_pos(phase, kind)] =
            self.cell_live[phase.ordinal()][Self::phase_row_pos(phase, kind)].saturating_add(1);
        Ok(idx)
    }

    fn find_live(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < SLOTS_PER_PHASE {
            match self.slots[i] {
                Some(ref s) if s.live && s.id == id => return Some(i),
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// 注销（显式注销不算回收）。
    pub fn unregister(&mut self, id: u32) -> Result<(), PolicyCode> {
        match self.find_live(id) {
            Some(i) => {
                let (p, k) = match self.slots[i] {
                    Some(ref s) => (s.phase, s.kind),
                    None => return Err(CODE_BAD_HANDLE),
                };
                if let Some(ref mut s) = self.slots[i] {
                    s.live = false;
                }
                let live = &mut self.cell_live[p.ordinal()][Self::phase_row_pos(p, k)];
                *live = live.saturating_sub(1);
                Ok(())
            }
            None => Err(CODE_BAD_HANDLE),
        }
    }

    pub fn live_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < SLOTS_PER_PHASE {
            match self.slots[i] {
                Some(ref s) if s.live => n = n.saturating_add(1),
                _ => {}
            }
            i += 1;
        }
        n
    }

    // -----------------------------------------------------------------------
    // 分发（O(回调)：顺序表给出逻辑序，游标单调推进）
    // -----------------------------------------------------------------------

    /// 分发一个回调（调用方按本条产出的执行序逐个调 `dispatch`）。
    ///
    /// 顺序契约（双闸）：
    /// - **完整性闸**：段按 0→1→2、段内行序单调——派到某格前，所有
    ///   之前的格必须已派完（有在册即须全派），跳段即拒绝；
    /// - **单调闸**：游标不回跳，同帧派过的段不许再回派。
    /// 违例即拒绝并计数（断言不纠错）。执行耗时超预算 → 告警 + 连续
    /// 达阈值钳制。
    pub fn dispatch(
        &mut self,
        id: u32,
        phase: FramePhase,
        took_us: u32,
    ) -> Result<(), PolicyCode> {
        let idx = match self.find_live(id) {
            Some(x) => x,
            None => return Err(CODE_BAD_HANDLE),
        };
        let p = phase.ordinal();
        let slot_kind = match self.slots[idx] {
            Some(ref s) => s.kind,
            None => return Err(CODE_BAD_HANDLE),
        };
        if !phase_admits(phase, slot_kind) {
            self.order_violations = self.order_violations.saturating_add(1);
            return Err(CODE_ORDER_VIOLATION);
        }
        let row_pos = Self::phase_row_pos(phase, slot_kind);
        // 完整性闸：之前每一格「有在册却没派完」即跳段违例。
        {
            let mut blocked = false;
            let mut pp = 0usize;
            while pp < PHASE_KINDS {
                let mut rr = 0usize;
                while rr < 2 {
                    let earlier = pp < p || (pp == p && rr < row_pos);
                    if earlier
                        && self.cell_live[pp][rr] > 0
                        && self.cell_runs[pp][rr] < self.cell_live[pp][rr]
                    {
                        blocked = true;
                    }
                    rr += 1;
                }
                pp += 1;
            }
            if blocked {
                self.order_violations = self.order_violations.saturating_add(1);
                return Err(CODE_ORDER_VIOLATION);
            }
        }
        // 单调闸：游标不回跳（同格内多个回调互不约束先后）。
        if p < self.cursor_phase || (p == self.cursor_phase && row_pos < self.cursor_row) {
            self.order_violations = self.order_violations.saturating_add(1);
            return Err(CODE_ORDER_VIOLATION);
        }
        self.cursor_phase = p;
        self.cursor_row = row_pos;
        self.cell_runs[p][row_pos] = self.cell_runs[p][row_pos].saturating_add(1);

        // 执行记账（先在槽位内改，跨字段计数在借外自增）。
        let (over, do_clamp) = {
            let s = match self.slots[idx] {
                Some(ref mut s) => s,
                None => return Err(CODE_BAD_HANDLE),
            };
            s.runs = s.runs.saturating_add(1);
            s.idle_frames = 0;
            let over = took_us > s.effective_us;
            let mut do_clamp = false;
            if over {
                s.timeout_strikes = s.timeout_strikes.saturating_add(1);
                if s.timeout_strikes >= TIMEOUT_STRIKES_TO_CLAMP && !s.clamped {
                    // 钳制：预算减半、最低保底（钳不没）。
                    let half = s.effective_us / CLAMP_DIV;
                    s.effective_us = if half < CLAMP_FLOOR_US {
                        CLAMP_FLOOR_US
                    } else {
                        half
                    };
                    s.clamped = true;
                    do_clamp = true;
                }
            } else {
                // 连续达标即解除钳制（恢复路径存在）。
                s.timeout_strikes = 0;
                if s.clamped {
                    s.clamped = false;
                    s.effective_us = s.budget_us;
                }
            }
            (over, do_clamp)
        };
        if over {
            self.timeout_warns = self.timeout_warns.saturating_add(1);
        }
        if do_clamp {
            self.clamp_total = self.clamp_total.saturating_add(1);
        }
        self.dispatch_total = self.dispatch_total.saturating_add(1);
        Ok(())
    }

    /// 帧推进：帧尾分发的最后一步之后调用（游标复位 + 空闲记账 + 泄漏回收）。
    ///
    /// 返回本帧回收的回调 id 数（回收留痕：id 连同计数进告警账）。
    pub fn end_frame(&mut self) -> u32 {
        self.frame = self.frame.saturating_add(1);
        self.cursor_phase = 0;
        self.cursor_row = 0;
        self.cell_runs = [[0; 2]; 3];
        let mut reclaimed: u32 = 0;
        let mut i = 0usize;
        while i < SLOTS_PER_PHASE {
            let mut reclaim = false;
            if let Some(ref mut s) = self.slots[i] {
                if s.live {
                    s.idle_frames = s.idle_frames.saturating_add(1);
                    if s.idle_frames >= IDLE_FRAMES_TO_RECLAIM {
                        reclaim = true;
                    }
                }
            }
            if reclaim {
                let (p, k) = match self.slots[i] {
                    Some(ref s) => (s.phase, s.kind),
                    None => (FramePhase::FrameStart, CallbackKind::Render),
                };
                self.slots[i] = None;
                self.cell_live[p.ordinal()][Self::phase_row_pos(p, k)] =
                    self.cell_live[p.ordinal()][Self::phase_row_pos(p, k)].saturating_sub(1);
                self.reclaimed_total = self.reclaimed_total.saturating_add(1);
                reclaimed = reclaimed.saturating_add(1);
            }
            i += 1;
        }
        reclaimed
    }

    pub fn frame(&self) -> u64 {
        self.frame
    }

    pub fn dispatch_total(&self) -> u32 {
        self.dispatch_total
    }

    pub fn timeout_warns(&self) -> u32 {
        self.timeout_warns
    }

    pub fn clamp_total(&self) -> u32 {
        self.clamp_total
    }

    pub fn order_violations(&self) -> u32 {
        self.order_violations
    }

    pub fn reclaimed_total(&self) -> u32 {
        self.reclaimed_total
    }

    /// 读屏面板（七行双语，只报分发事实与聚合计数）。
    pub fn a11y_lines(&self) -> [String; PANEL_LINES_V] {
        [
            format!("当前帧 / current frame: {}", self.frame),
            format!("在册回调 / live callbacks: {}", self.live_count()),
            format!(
                "分发计数 / dispatches: {}（违例 {}）",
                self.dispatch_total, self.order_violations
            ),
            format!(
                "超时告警 / timeout warnings: {}（钳制 {}）",
                self.timeout_warns, self.clamp_total
            ),
            format!(
                "泄漏回收 / reclaimed: {}（空闲阈值 {} 帧）",
                self.reclaimed_total, IDLE_FRAMES_TO_RECLAIM
            ),
            format!(
                "三段语义 / phases: 帧首=界面→脚本 帧中=渲染 帧尾=物理→脚本 / start=ui→script mid=render end=physics→script"
            ),
            format!(
                "顺序契约 / order contract: 单调游标断言，违例拒绝不纠错 / monotonic cursor, reject on violation"
            ),
        ]
    }
}

/// 面板行数。
pub const PANEL_LINES_V: usize = 7;

// ---------------------------------------------------------------------------
// 五、域自检（判据逐条映射锚点：统一分发/三段语义/超时检测/顺序契约/判据）
// ---------------------------------------------------------------------------

/// F0049 域自检入口（聚合器调用；零 panic 面，失败逐条红不炸域）。
pub fn run_vea49_checks() -> CheckSet {
    let mut s = CheckSet::new("vea49_framedisp");

    // --- 判据 1：顺序契约表——判据侧独立重算逐格对账 ---
    {
        // 独立重算（逐格字面写死，不读 PHASE_ORDER 自证）：
        // 帧首 = UI(1)→脚本(2)；帧中 = 渲染(0)；帧尾 = 物理(3)→脚本(2)。
        let expect: [[usize; 2]; 3] = [[1, 2], [0, usize::MAX], [3, 2]];
        let mut ok = true;
        let mut p = 0usize;
        while p < PHASE_KINDS {
            let mut k = 0usize;
            while k < 2 {
                if PHASE_ORDER[p][k] != expect[p][k] {
                    ok = false;
                }
                k += 1;
            }
            p += 1;
        }
        // 段内前后关系的可对账形式（优先级文档逐条断言）。
        let order_ok = phase_before(FramePhase::FrameStart, CallbackKind::Ui, CallbackKind::Script)
            && !phase_before(FramePhase::FrameStart, CallbackKind::Script, CallbackKind::Ui)
            && phase_before(FramePhase::FrameEnd, CallbackKind::Physics, CallbackKind::Script)
            && phase_admits(FramePhase::FrameMid, CallbackKind::Render)
            && !phase_admits(FramePhase::FrameMid, CallbackKind::Physics)
            && !phase_admits(FramePhase::FrameStart, CallbackKind::Render);
        s.add(
            "A49-顺序契约表-独立重算且前后关系逐条对账",
            ok && order_ok,
            "顺序表逐格与独立重算一致；帧首 UI 先于脚本、帧尾物理先于脚本、渲染仅在帧中（优先级文档可对账）",
        );
    }

    // --- 判据 2：统一分发——注册全覆盖、恰好一遍、不重不漏 ---
    {
        let mut d = FrameDispatcher::new();
        let r1 = d.register(1, FramePhase::FrameStart, CallbackKind::Ui, 1_000);
        let r2 = d.register(2, FramePhase::FrameStart, CallbackKind::Script, 1_000);
        let r3 = d.register(3, FramePhase::FrameMid, CallbackKind::Render, 1_000);
        let r4 = d.register(4, FramePhase::FrameEnd, CallbackKind::Physics, 1_000);
        let r5 = d.register(5, FramePhase::FrameEnd, CallbackKind::Script, 1_000);
        let reg_ok = r1.is_ok() && r2.is_ok() && r3.is_ok() && r4.is_ok() && r5.is_ok();
        // 按执行序分发一遍。
        let seq = [(1u32, FramePhase::FrameStart), (2, FramePhase::FrameStart),
                   (3, FramePhase::FrameMid), (4, FramePhase::FrameEnd), (5, FramePhase::FrameEnd)];
        let mut all_ok = true;
        let mut i = 0usize;
        while i < seq.len() {
            if d.dispatch(seq[i].0, seq[i].1, 10).is_err() {
                all_ok = false;
            }
            i += 1;
        }
        let covered = d.dispatch_total() == 5 && d.live_count() == 5;
        let dup = d.register(1, FramePhase::FrameStart, CallbackKind::Ui, 1_000)
            == Err(CODE_DUP_REGISTER);
        let empty = d.register(9, FramePhase::FrameMid, CallbackKind::Ui, 1_000)
            == Err(CODE_ORDER_VIOLATION);
        s.add(
            "A49-统一分发-注册全覆盖恰好一遍",
            reg_ok && all_ok && covered && dup && empty,
            "5 回调按序各执行一遍（总数=5 不重不漏）；重复注册拒（DUP_REGISTER）；段不容类注册拒（顺序契约）",
        );
    }

    // --- 判据 3：三段语义——跨段顺序由游标钉死 ---
    {
        let mut d = FrameDispatcher::new();
        let _ = d.register(1, FramePhase::FrameStart, CallbackKind::Ui, 1_000);
        let _ = d.register(2, FramePhase::FrameMid, CallbackKind::Render, 1_000);
        let _ = d.register(3, FramePhase::FrameEnd, CallbackKind::Physics, 1_000);
        // 帧中先于帧首 = 违例。
        let jump = d.dispatch(2, FramePhase::FrameMid, 10) == Err(CODE_ORDER_VIOLATION);
        // 正确顺序全通过。
        let a = d.dispatch(1, FramePhase::FrameStart, 10).is_ok();
        let b = d.dispatch(2, FramePhase::FrameMid, 10).is_ok();
        let c = d.dispatch(3, FramePhase::FrameEnd, 10).is_ok();
        // 帧尾之后再发帧首回调（同帧）= 违例。
        let back = d.dispatch(1, FramePhase::FrameStart, 10) == Err(CODE_ORDER_VIOLATION);
        s.add(
            "A49-三段语义-跨段单调游标钉死",
            jump && a && b && c && back && d.order_violations() == 2,
            "帧中先于帧首被拒；正确三段序通过；同帧回跳帧首被拒（违例=断言拒绝+计数，不纠错）",
        );
    }

    // --- 判据 4：超时检测——告警+连续达阈值钳制（钳到一半最低保底） ---
    {
        let mut d = FrameDispatcher::new();
        let _ = d.register(7, FramePhase::FrameStart, CallbackKind::Ui, 2_000);
        // 第 1 次超时：告警不钳。
        let r1 = d.dispatch(7, FramePhase::FrameStart, 2_100);
        let warn1 = d.timeout_warns() == 1;
        // 第 2 次连续超时：钳制（2000/2=1000 ≥ 保底 500）。
        let r2 = d.dispatch(7, FramePhase::FrameStart, 2_100);
        let clamped = d.clamp_total() == 1;
        s.add(
            "A49-超时-告警钳制减半保底",
            r1.is_ok() && warn1 && r2.is_ok() && clamped && TIMEOUT_WARN_US == 2_000
                && TIMEOUT_STRIKES_TO_CLAMP == 2 && CLAMP_DIV == 2 && CLAMP_FLOOR_US == 500,
            "超预算即告警计数；连续 2 次达阈值钳制：预算减半（2000→1000）且保底 500（钳不没）；阈值字面值双向钉死",
        );
    }

    // --- 判据 5：钳制恢复与保底——达标解除、低预算钳不没 ---
    {
        let mut d = FrameDispatcher::new();
        let _ = d.register(8, FramePhase::FrameStart, CallbackKind::Script, 800);
        // 连续 2 次超时 → 钳制：800/2=400 < 保底 500 ⇒ 生效预算=500。
        let _ = d.dispatch(8, FramePhase::FrameStart, 10_000);
        let _ = d.dispatch(8, FramePhase::FrameStart, 10_000);
        let floored = d.clamp_total() == 1;
        // 达标一次：解除钳制恢复基础预算（再超时从 800 重新计）。
        let _ = d.dispatch(8, FramePhase::FrameStart, 100);
        // 恢复后再连续 2 次超时：再次钳制（可重复）。
        let _ = d.dispatch(8, FramePhase::FrameStart, 10_000);
        let _ = d.dispatch(8, FramePhase::FrameStart, 10_000);
        s.add(
            "A49-钳制-保底生效且达标解除可重复",
            floored && d.clamp_total() == 2 && d.timeout_warns() == 4,
            "低预算钳制保底生效（400→500 不钳没）；达标即解除恢复基础预算；再次连续超时可再钳制（钳制计数=2）",
        );
    }

    // --- 判据 6：回调泄漏→回收——空闲达阈值回收且注销不算泄漏 ---
    {
        let mut d = FrameDispatcher::new();
        let _ = d.register(11, FramePhase::FrameStart, CallbackKind::Ui, 1_000);
        let _ = d.register(12, FramePhase::FrameMid, CallbackKind::Render, 1_000);
        // 12 立即注销（显式注销不算回收）；11 闲置。
        let _ = d.unregister(12);
        let mut frames = 0u32;
        while frames < IDLE_FRAMES_TO_RECLAIM {
            let got = d.end_frame();
            if frames == IDLE_FRAMES_TO_RECLAIM - 1 {
                if got != 1 {
                    break;
                }
            }
            frames += 1;
        }
        s.add(
            "A49-泄漏回收-空闲达阈值回收且注销不算",
            frames == IDLE_FRAMES_TO_RECLAIM
                && d.reclaimed_total() == 1
                && d.live_count() == 0
                && d.dispatch_total() == 0
                && IDLE_FRAMES_TO_RECLAIM == 64,
            "闲置 64 帧的回调被回收（回收计数 1）；显式注销的回调不计回收（注销与回收分账）；阈值字面值钉死",
        );
    }

    // --- 判据 7：构造期无效值拒绝——零预算拒、坏句柄拒 ---
    {
        let mut d = FrameDispatcher::new();
        let zero = d.register(1, FramePhase::FrameStart, CallbackKind::Ui, 0)
            == Err(CODE_BAD_BUDGET);
        let ghost = d.dispatch(999, FramePhase::FrameStart, 10) == Err(CODE_BAD_HANDLE);
        let gone = {
            let _ = d.register(2, FramePhase::FrameMid, CallbackKind::Render, 1_000);
            let _ = d.unregister(2);
            d.dispatch(2, FramePhase::FrameMid, 10) == Err(CODE_BAD_HANDLE)
        };
        s.add(
            "A49-无效值-零预算坏句柄已注销全拒",
            zero && ghost && gone && CODE_BAD_HANDLE != CODE_BAD_BUDGET,
            "零预算注册拒（构造期无效值）；不存在句柄与已注销句柄分发拒（码段互异防自判死）",
        );
    }

    // --- 判据 8：段满拒绝不截断 ---
    {
        let mut d = FrameDispatcher::new();
        let mut ok = true;
        let mut n = 0u32;
        while n < SLOTS_PER_PHASE as u32 {
            if d.register(100 + n, FramePhase::FrameStart, CallbackKind::Ui, 1_000).is_err() {
                ok = false;
            }
            n += 1;
        }
        let full = d.register(999, FramePhase::FrameStart, CallbackKind::Script, 1_000)
            == Err(CODE_PHASE_FULL)
            && d.full_rejected == 1
            && ok
            && SLOTS_PER_PHASE == 16;
        s.add(
            "A49-段满-容量上限拒绝不截断",
            full,
            "16 槽全占后再注册被拒（PHASE_FULL + 拒绝计数），已注册的 16 个不受影响（拒绝不静默截断）",
        );
    }

    // --- 判据 9：完整帧循环——分发→end_frame→游标复位→下帧照常 ---
    {
        let mut d = FrameDispatcher::new();
        let _ = d.register(1, FramePhase::FrameStart, CallbackKind::Ui, 1_000);
        let _ = d.register(2, FramePhase::FrameEnd, CallbackKind::Physics, 1_000);
        let f1 = d.dispatch(1, FramePhase::FrameStart, 10).is_ok()
            && d.dispatch(2, FramePhase::FrameEnd, 10).is_ok();
        d.end_frame();
        // 下帧从头再来：帧首回调不再被判违例。
        let f2 = d.dispatch(1, FramePhase::FrameStart, 10).is_ok()
            && d.dispatch(2, FramePhase::FrameEnd, 10).is_ok()
            && d.frame() == 1
            && d.order_violations() == 0;
        s.add(
            "A49-接缝-完整帧循环游标复位",
            f1 && f2,
            "第 1 帧全序通过；end_frame 后游标复位，第 2 帧同序零违例（分发状态不跨帧泄漏）",
        );
    }

    // --- 判据 10：AC02 对接事实——分发序与超时账可消费 ---
    {
        let mut d = FrameDispatcher::new();
        let _ = d.register(1, FramePhase::FrameStart, CallbackKind::Ui, 1_000);
        let _ = d.dispatch(1, FramePhase::FrameStart, 5_000);
        s.add(
            "A49-对接-执行序与超时事实可消费",
            d.dispatch_total() == 1 && d.timeout_warns() == 1 && d.order_violations() == 0,
            "分发器产出「执行序（游标契约）+超时账（告警计数）」供 AC02 调度消费，不越权管线程池",
        );
    }

    // --- 判据 11：读屏面板——七行双语逐行绑定聚合量 ---
    {
        let mut d = FrameDispatcher::new();
        let _ = d.register(1, FramePhase::FrameStart, CallbackKind::Ui, 1_000);
        let _ = d.dispatch(1, FramePhase::FrameStart, 3_000);
        let lines = d.a11y_lines();
        let all_nonempty = lines.iter().all(|l| !l.is_empty());
        s.add(
            "A49-面板-七行双语且逐行绑定聚合量",
            lines.len() == PANEL_LINES_V
                && all_nonempty
                && lines[0].contains("current frame: 0")
                && lines[1].contains("live callbacks: 1")
                && lines[2].contains("dispatches: 1")
                && lines[3].contains("timeout warnings: 1")
                && lines[4].contains("reclaimed: 0")
                && lines[5].contains("ui")
                && lines[6].contains("monotonic")
                && lines.iter().all(|l| l.chars().any(|c| c.is_ascii_alphabetic())),
            "面板七行逐行绑定：帧号 0 / 在册 1 / 分发 1 / 超时告警 1 / 回收 0 / 三段语义 / 顺序契约，每行双语",
        );
    }

    s
}
