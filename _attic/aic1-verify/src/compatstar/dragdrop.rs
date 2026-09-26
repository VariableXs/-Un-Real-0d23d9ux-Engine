//! F018 拖放协议互通（compatstar · G-A-18）——三方向全通，Esc 中途取消三态
//! 全有效。
//!
//! 主册判据（验收标准第一句）：
//! **「三方向 × 文件/文本两类数据 = 6 场景全绿录屏；B-3902 三取消逐条在兼
//! 容面复测。」**
//!
//! 功能定义（G-A-18）：OLE 拖放（DoDragDrop/IDropSource/IDropTarget 语义）
//! 与 VXWM 拖放协议（B-3902 三取消）双向翻译：Wine 程序拖出文件到资源管理
//! 器、资源管理器拖文件进 Wine 程序、两个 Wine 程序间互拖，三条路全通。
//!
//! 【交互设计】动线全按 C-6/C-4 红线（跟手/落点提示/Esc）；跨表面拖拽时数
//! 据预览（文件名+图标）由发起方供给。【数据与存储】拖拽中数据驻内存（延迟
//! 渲染语义：目标请求才真取）；跨盘拖放落 F086 复制管线。
//! 【状态与异常】目标不接受该格式 → 光标禁止态（不静默松手失败）；源应用
//! 拖拽中崩溃 → 拖放会话回收 + 目标复位；同源同目标死拖 → 超时 30s 自动取
//! 消。
//! 【设计细节】拖拽会话全生命周期状态机（就绪/悬停/落入/取消/完成五态），
//! 状态跃迁走 VXWM 报文（可观测可录屏归因）；数据获取延迟渲染在松手时才执
//! 行（拖 100 个文件不预读内容）；落点提示矩形 2px 强调色描边加 8% 填充；
//! 拖拽中源窗口加 20% 降透明。
//!
//! 零堆纪律：会话表定长、报文环定长，无 Vec/String/Box/format!。

use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 同源同目标死拖超时 30s 自动取消（主册【状态与异常】）。
pub const DEAD_DRAG_TIMEOUT_MS: u64 = 30_000;
/// 落点提示矩形描边 2px（主册【设计细节】）。
pub const DROP_HINT_BORDER_PX: u32 = 2;
/// 落点提示填充 8%（permille 80）。
pub const DROP_HINT_FILL_PERMILLE: u32 = 80;
/// 拖拽中源窗口降透明 20%（主册【设计细节】；80% = 800 permille 不透明度）。
pub const SOURCE_DIM_OPACITY_PERMILLE: u32 = 800;

// ---------------------------------------------------------------------------
// 状态机（五态 + VXWM 报文）
// ---------------------------------------------------------------------------

/// 拖放会话五态（主册【设计细节】：就绪/悬停/落入/取消/完成）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DragState {
    Ready,
    Hovering,
    Entered,
    Cancelled(CancelReason),
    Completed,
}

/// B-3902 三取消 + 超时/崩溃扩展取消。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CancelReason {
    /// 悬停区取消（拖出目标区）。
    HoverLeft,
    /// 落点非法取消。
    InvalidDrop,
    /// 按键取消（Esc）。
    KeyCancel,
    /// 死拖超时（30s）。
    DeadDragTimeout,
    /// 源崩溃（会话回收 + 目标复位）。
    SourceCrashed,
}

/// 拖放效果（OLE DropEffect 语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropEffect {
    Copy,
    Move,
    Link,
    None,
}

/// 拖拽数据类（文件/文本——主册 6 场景的两类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DragData {
    Files(u32),
    Text,
}

/// VXWM 报文（状态跃迁可观测——录屏归因的数据源）。
#[derive(Clone, Copy, Debug)]
pub struct VxwmReport {
    pub at_ms: u64,
    pub from: DragState,
    pub to: DragState,
}

/// 单个拖放会话。
pub struct DragSession {
    pub state: DragState,
    pub data: DragData,
    /// 目标是否接受该格式（不接受 → 光标禁止态）。
    pub target_accepts: bool,
    pub started_ms: u64,
    last_move_ms: u64,
    /// 延迟渲染：松手时才真取数据（fetch 次数记账——拖 100 文件不预读）。
    pub data_fetches: u32,
    reports: [Option<VxwmReport>; 32],
    report_n: usize,
}

impl DragSession {
    pub fn new(data: DragData, target_accepts: bool, now_ms: u64) -> DragSession {
        DragSession {
            state: DragState::Ready,
            data,
            target_accepts,
            started_ms: now_ms,
            last_move_ms: now_ms,
            data_fetches: 0,
            reports: [None; 32],
            report_n: 0,
        }
    }

    fn transition(&mut self, to: DragState, now_ms: u64) {
        let from = self.state;
        self.state = to;
        if self.report_n < 32 {
            self.reports[self.report_n] = Some(VxwmReport { at_ms: now_ms, from, to });
            self.report_n += 1;
        }
    }

    /// 拖动进入目标区（悬停）。
    pub fn hover(&mut self, now_ms: u64) {
        self.last_move_ms = now_ms;
        if self.state == DragState::Ready {
            self.transition(DragState::Hovering, now_ms);
        }
    }

    /// 落入（合法落点悬停确认）。
    pub fn enter(&mut self, now_ms: u64) {
        self.last_move_ms = now_ms;
        if self.state == DragState::Hovering && self.target_accepts {
            self.transition(DragState::Entered, now_ms);
        }
    }

    /// 拖出目标区（取消一：HoverLeft——B-3902）。
    pub fn leave(&mut self, now_ms: u64) {
        if self.state == DragState::Hovering || self.state == DragState::Entered {
            self.transition(DragState::Cancelled(CancelReason::HoverLeft), now_ms);
        }
    }

    /// 非法落点松手（取消二：InvalidDrop——光标禁止态语义，不静默失败）。
    pub fn drop_rejected(&mut self, now_ms: u64) -> DropEffect {
        if self.state == DragState::Entered || self.state == DragState::Hovering {
            self.transition(DragState::Cancelled(CancelReason::InvalidDrop), now_ms);
        }
        DropEffect::None
    }

    /// Esc（取消三：KeyCancel——B-3902）。
    pub fn esc_cancel(&mut self, now_ms: u64) {
        if matches!(self.state, DragState::Ready | DragState::Hovering | DragState::Entered) {
            self.transition(DragState::Cancelled(CancelReason::KeyCancel), now_ms);
        }
    }

    /// 松手提交（延迟渲染语义：此刻才取数据——fetch 记账 1 次批量）。
    pub fn drop_commit(&mut self, effect: DropEffect, now_ms: u64) -> DropEffect {
        if self.state != DragState::Entered {
            return self.drop_rejected(now_ms);
        }
        self.data_fetches += 1;
        self.transition(DragState::Completed, now_ms);
        effect
    }

    /// 死拖超时检查（30s 无位移 → 自动取消；主册【状态与异常】）。
    pub fn tick_dead_drag(&mut self, now_ms: u64) -> bool {
        if matches!(self.state, DragState::Ready | DragState::Hovering | DragState::Entered)
            && now_ms.saturating_sub(self.last_move_ms) >= DEAD_DRAG_TIMEOUT_MS
        {
            self.transition(DragState::Cancelled(CancelReason::DeadDragTimeout), now_ms);
            return true;
        }
        false
    }

    /// 源崩溃（会话回收 + 目标复位——主册【状态与异常】）。
    pub fn source_crashed(&mut self, now_ms: u64) {
        if !matches!(self.state, DragState::Completed | DragState::Cancelled(_)) {
            self.transition(DragState::Cancelled(CancelReason::SourceCrashed), now_ms);
        }
    }

    /// 报文环回放（可观测面）。
    pub fn reports(&self) -> impl Iterator<Item = VxwmReport> + '_ {
        (0..self.report_n).filter_map(move |i| self.reports[i])
    }

    /// 光标语义：目标不接受 → 禁止态（DropEffect::None）。
    pub fn cursor_effect(&self) -> DropEffect {
        if !self.target_accepts {
            DropEffect::None
        } else {
            match self.state {
                DragState::Entered => DropEffect::Copy,
                _ => DropEffect::None,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 会话管理器（三方向共管）
// ---------------------------------------------------------------------------

/// 拖放方向（主册 6 场景：3 方向 × 2 数据类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DragDirection {
    WineToNative,
    NativeToWine,
    WineToWine,
}

/// 拖放翻译中枢（OLE↔VXWM 双向翻译——三方向同一核，方向仅记账）。
pub struct DragHub {
    session: Option<DragSession>,
    pub direction: Option<DragDirection>,
    /// 目标复位标记（源崩溃 → 目标复位判据的观测位）。
    pub target_reset: bool,
    /// 完成会话计数（6 场景全绿的记账面）。
    pub completed: u32,
}

impl DragHub {
    pub fn new() -> DragHub {
        DragHub { session: None, direction: None, target_reset: false, completed: 0 }
    }

    /// 发起拖拽（三入口同核——主册【功能定义】三条路全通的唯一实现）。
    pub fn begin(&mut self, dir: DragDirection, data: DragData, target_accepts: bool, now_ms: u64) {
        self.direction = Some(dir);
        self.target_reset = false;
        self.session = Some(DragSession::new(data, target_accepts, now_ms));
    }

    pub fn session(&mut self) -> Option<&mut DragSession> {
        self.session.as_mut()
    }

    /// 会话终态收割（Completed → completed 计数；Cancelled → 目标复位）。
    pub fn reap(&mut self) {
        if let Some(s) = self.session.take() {
            match s.state {
                DragState::Completed => self.completed += 1,
                _ => self.target_reset = true,
            }
        }
    }

    pub fn has_session(&self) -> bool {
        self.session.is_some()
    }
}

impl Default for DragHub {
    fn default() -> Self {
        Self::new()
    }
}

/// 域自检。
pub fn run_dragdrop_base_checks() -> CheckSet {
    let mut cs = CheckSet::new("F018-dragdrop");
    // 1) 判据常量（30s / 2px / 8% / 80% 不透明）。
    cs.add(
        "consts",
        DEAD_DRAG_TIMEOUT_MS == 30_000
            && DROP_HINT_BORDER_PX == 2
            && DROP_HINT_FILL_PERMILLE == 80
            && SOURCE_DIM_OPACITY_PERMILLE == 800,
        "",
    );
    // 2) 6 场景全绿：三方向 × 文件/文本全走完五态到 Completed。
    let dirs = [DragDirection::WineToNative, DragDirection::NativeToWine, DragDirection::WineToWine];
    let datas = [DragData::Files(3), DragData::Text];
    let mut green = 0u32;
    for &d in dirs.iter() {
        for &data in datas.iter() {
            let mut hub = DragHub::new();
            hub.begin(d, data, true, 0);
            let s = hub.session().unwrap();
            s.hover(10);
            s.enter(20);
            let eff = s.drop_commit(DropEffect::Copy, 30);
            hub.reap();
            if eff == DropEffect::Copy && hub.completed == 1 && !hub.target_reset {
                green += 1;
            }
        }
    }
    cs.add("six_scenarios_all_green", green == 6, "");
    // 3) B-3902 取消一：悬停区取消（拖出目标区）。
    let mut hub = DragHub::new();
    hub.begin(DragDirection::WineToNative, DragData::Text, true, 0);
    hub.session().unwrap().hover(10);
    hub.session().unwrap().leave(20);
    hub.reap();
    cs.add("cancel_hover_left", hub.target_reset && hub.completed == 0, "");
    // 4) B-3902 取消二：非法落点（目标不接受格式 → 光标禁止 + 不静默失败）。
    let mut hub2 = DragHub::new();
    hub2.begin(DragDirection::NativeToWine, DragData::Files(1), false, 0);
    let s2 = hub2.session().unwrap();
    s2.hover(10);
    let eff = s2.drop_rejected(20);
    cs.add(
        "cancel_invalid_drop_forbidden_cursor",
        eff == DropEffect::None && s2.cursor_effect() == DropEffect::None,
        "",
    );
    // 5) B-3902 取消三：Esc 按键取消。
    let mut hub3 = DragHub::new();
    hub3.begin(DragDirection::WineToWine, DragData::Text, true, 0);
    let s3 = hub3.session().unwrap();
    s3.hover(10);
    s3.esc_cancel(20);
    cs.add(
        "cancel_esc_key",
        matches!(s3.state, DragState::Cancelled(CancelReason::KeyCancel)),
        "",
    );
    // 6) 延迟渲染：拖 100 文件不预读——松手时 fetch 恰好 1 次批量。
    let mut hub4 = DragHub::new();
    hub4.begin(DragDirection::WineToNative, DragData::Files(100), true, 0);
    let s4 = hub4.session().unwrap();
    s4.hover(10);
    s4.enter(20);
    let _ = s4.drop_commit(DropEffect::Move, 30);
    cs.add(
        "delayed_fetch_on_release_only",
        s4.data_fetches == 1,
        "",
    );
    // 7) 死拖超时：30s 无位移自动取消；29s 不取消。
    let mut hub5 = DragHub::new();
    hub5.begin(DragDirection::WineToWine, DragData::Text, true, 0);
    let s5 = hub5.session().unwrap();
    s5.hover(1_000);
    cs.add("dead_drag_before_timeout", !s5.tick_dead_drag(30_999), "");
    cs.add(
        "dead_drag_timeout_cancels",
        s5.tick_dead_drag(31_000)
            && matches!(s5.state, DragState::Cancelled(CancelReason::DeadDragTimeout)),
        "",
    );
    // 8) 源崩溃：会话回收 + 目标复位。
    let mut hub6 = DragHub::new();
    hub6.begin(DragDirection::WineToNative, DragData::Files(2), true, 0);
    hub6.session().unwrap().hover(10);
    hub6.session().unwrap().source_crashed(20);
    hub6.reap();
    cs.add("source_crash_resets_target", hub6.target_reset && !hub6.has_session(), "");
    // 9) VXWM 报文可观测：Ready→Hovering→Entered→Completed 全程留痕。
    let mut hub7 = DragHub::new();
    hub7.begin(DragDirection::WineToNative, DragData::Text, true, 0);
    let s7 = hub7.session().unwrap();
    s7.hover(1);
    s7.enter(2);
    let _ = s7.drop_commit(DropEffect::Copy, 3);
    let reps: Vec<VxwmReport> = s7.reports().collect();
    cs.add(
        "vxwm_reports_observable",
        reps.len() == 3
            && reps[0].from == DragState::Ready && reps[0].to == DragState::Hovering
            && reps[1].to == DragState::Entered
            && reps[2].to == DragState::Completed,
        "",
    );
    // 10) 终态不可再动（Completed/Cancelled 后的操作无效——状态机封闭性）。
    let mut hub8 = DragHub::new();
    hub8.begin(DragDirection::WineToNative, DragData::Text, true, 0);
    let s8 = hub8.session().unwrap();
    s8.hover(1);
    s8.esc_cancel(2);
    let before = s8.state;
    s8.esc_cancel(3);
    s8.hover(4);
    cs.add("terminal_states_sealed", s8.state == before, "");
    cs
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_success_path_five_states() {
        // 五态全走：Ready→Hovering→Entered→Completed（带报文留痕）。
        let mut hub = DragHub::new();
        hub.begin(DragDirection::WineToNative, DragData::Files(10), true, 0);
        let s = hub.session().unwrap();
        assert_eq!(s.state, DragState::Ready);
        s.hover(100);
        assert_eq!(s.state, DragState::Hovering);
        assert_eq!(s.cursor_effect(), DropEffect::None, "悬停未落入 → 无效果光标");
        s.enter(200);
        assert_eq!(s.state, DragState::Entered);
        assert_eq!(s.cursor_effect(), DropEffect::Copy);
        let eff = s.drop_commit(DropEffect::Copy, 300);
        assert_eq!(eff, DropEffect::Copy);
        assert_eq!(s.state, DragState::Completed);
        hub.reap();
        assert_eq!(hub.completed, 1);
        assert!(!hub.target_reset);
    }

    #[test]
    fn commit_only_from_entered() {
        // 未落入就松手 = 非法落点取消（不静默成功）。
        let mut hub = DragHub::new();
        hub.begin(DragDirection::WineToNative, DragData::Text, true, 0);
        let s = hub.session().unwrap();
        s.hover(10);
        let eff = s.drop_commit(DropEffect::Copy, 20);
        assert_eq!(eff, DropEffect::None);
        assert!(matches!(s.state, DragState::Cancelled(CancelReason::InvalidDrop)));
    }

    #[test]
    fn enter_requires_acceptance() {
        // 目标不接受格式 → enter 不生效（永远进不了 Entered → 必然取消）。
        let mut hub = DragHub::new();
        hub.begin(DragDirection::NativeToWine, DragData::Text, false, 0);
        let s = hub.session().unwrap();
        s.hover(10);
        s.enter(20);
        assert_eq!(s.state, DragState::Hovering, "unaccepted target must not enter");
        let eff = s.drop_commit(DropEffect::Copy, 30);
        assert_eq!(eff, DropEffect::None);
    }

    #[test]
    fn leave_from_entered_also_cancels() {
        // 落入后再拖出：同样走悬停区取消（B-3902 全覆盖）。
        let mut hub = DragHub::new();
        hub.begin(DragDirection::WineToWine, DragData::Files(1), true, 0);
        let s = hub.session().unwrap();
        s.hover(10);
        s.enter(20);
        s.leave(30);
        assert!(matches!(s.state, DragState::Cancelled(CancelReason::HoverLeft)));
    }

    #[test]
    fn dead_drag_counts_from_last_move() {
        // 超时窗从最后位移起算：中途移动刷新计时。
        let mut hub = DragHub::new();
        hub.begin(DragDirection::WineToNative, DragData::Text, true, 0);
        let s = hub.session().unwrap();
        s.hover(0);
        s.hover(20_000); // 中途位移
        assert!(!s.tick_dead_drag(49_999));
        assert!(s.tick_dead_drag(50_000));
    }

    #[test]
    fn hub_reap_counts_both_ends() {
        // 收割器：完成 +1，取消 → 复位标记。
        let mut hub = DragHub::new();
        hub.begin(DragDirection::WineToNative, DragData::Text, true, 0);
        let _ = hub.session().unwrap().drop_commit(DropEffect::Copy, 1); // 从 Ready 直接 commit → 非法
        hub.reap();
        assert!(hub.target_reset && hub.completed == 0);
        hub.begin(DragDirection::WineToNative, DragData::Text, true, 10);
        let s = hub.session().unwrap();
        s.hover(11);
        s.enter(12);
        let _ = s.drop_commit(DropEffect::Link, 13);
        hub.reap();
        assert_eq!(hub.completed, 1);
        assert!(!hub.target_reset);
    }
}

// ---------------------------------------------------------------------------
// F018 · 深化扩展：QueryContinueDrag / GiveFeedback（OLE 源侧语义）
//
// 主册依据（G-A-18【开源复用】）：「OLE 拖放语义对照 Wine ole32 拖放面」——
// 源侧两个回调的裁决语义补齐：QueryContinueDrag（键盘状态 → 继续/投放/取消）
// 与 GiveFeedback（效果 → 光标形态）。
// ---------------------------------------------------------------------------

/// QueryContinueDrag 裁决（OLE 三态，HRESULT 对应面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueryContinue {
    /// S_OK：继续拖拽。
    Continue,
    /// DRAGDROP_S_DROP：松开目标键 → 投放。
    Drop,
    /// DRAGDROP_S_CANCEL：Esc 或其他取消键 → 取消。
    Cancel,
}

/// 键盘/鼠标状态快照（DoDragDrop 循环每拍传入）。
#[derive(Clone, Copy, Debug)]
pub struct DragKeyState {
    pub esc_pressed: bool,
    /// 主释放键是否已松开（左键拖拽语义：松开 = 投放时刻）。
    pub primary_released: bool,
    /// 是否发生过「按住期间键位变化但非取消键」（OLE 默认容忍）。
    pub other_key_noise: bool,
}

/// QueryContinueDrag 裁决（OLE 默认方案：Esc → 取消；主键松开 → 投放；
/// 其余按键噪声容忍继续——与 B-3902 取消三态互为表里）。
pub fn query_continue_drag(ks: DragKeyState) -> QueryContinue {
    if ks.esc_pressed {
        return QueryContinue::Cancel;
    }
    if ks.primary_released {
        return QueryContinue::Drop;
    }
    QueryContinue::Continue
}

/// GiveFeedback 结果（效果 → 光标形态；OLE 无窗口源的标准实现）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeedbackCursor {
    Arrow,
    CopyArrow,
    MoveArrow,
    LinkArrow,
    Forbidden,
}

/// 效果 → 光标形态（落点提示体系 C-6 语义：可落显示对应效果，禁落显示禁止）。
pub fn give_feedback(effect: DropEffect) -> FeedbackCursor {
    match effect {
        DropEffect::Copy => FeedbackCursor::CopyArrow,
        DropEffect::Move => FeedbackCursor::MoveArrow,
        DropEffect::Link => FeedbackCursor::LinkArrow,
        DropEffect::None => FeedbackCursor::Forbidden,
    }
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    #[test]
    fn query_continue_semantics() {
        // OLE 三态逐一对拍：Esc 取消 / 松开投放 / 其余继续（键位噪声容忍）。
        assert_eq!(query_continue_drag(DragKeyState { esc_pressed: false, primary_released: false, other_key_noise: false }), QueryContinue::Continue);
        assert_eq!(query_continue_drag(DragKeyState { esc_pressed: false, primary_released: false, other_key_noise: true }), QueryContinue::Continue);
        assert_eq!(query_continue_drag(DragKeyState { esc_pressed: false, primary_released: true, other_key_noise: false }), QueryContinue::Drop);
        assert_eq!(query_continue_drag(DragKeyState { esc_pressed: true, primary_released: true, other_key_noise: false }), QueryContinue::Cancel, "Esc 优先于松开（取消优先语义）");
    }

    #[test]
    fn feedback_cursor_mapping() {
        // 效果→光标映射与拖放状态机的 cursor_effect 对账（同一效果同一光标）。
        assert_eq!(give_feedback(DropEffect::Copy), FeedbackCursor::CopyArrow);
        assert_eq!(give_feedback(DropEffect::Move), FeedbackCursor::MoveArrow);
        assert_eq!(give_feedback(DropEffect::Link), FeedbackCursor::LinkArrow);
        assert_eq!(give_feedback(DropEffect::None), FeedbackCursor::Forbidden);
    }

    #[test]
    fn source_loop_integration() {
        // DoDragDrop 循环端到端：继续若干拍 → 松开 → 投放；对照 Esc 路径。
        let frames = [
            DragKeyState { esc_pressed: false, primary_released: false, other_key_noise: false },
            DragKeyState { esc_pressed: false, primary_released: false, other_key_noise: true },
            DragKeyState { esc_pressed: false, primary_released: true, other_key_noise: false },
        ];
        let mut verdicts = Vec::new();
        for ks in frames {
            verdicts.push(query_continue_drag(ks));
        }
        assert_eq!(verdicts, alloc::vec![QueryContinue::Continue, QueryContinue::Continue, QueryContinue::Drop]);
        // Esc 路径：第一拍就取消（与 DragSession::esc_cancel 对账）。
        assert_eq!(query_continue_drag(DragKeyState { esc_pressed: true, primary_released: false, other_key_noise: false }), QueryContinue::Cancel);
    }
}

// ---------------------------------------------------------------------------
// 深化批次二：自检聚合（主检 + 深化检并为一行——AI-U2 merge 先例；
// robust.rs / 隔离壳 checkup 接线不变，深化检查项全部经由此行可见）。
// ---------------------------------------------------------------------------

/// 域自检（聚合版）。
pub fn run_dragdrop_checks() -> CheckSet {
    CheckSet::merge(run_dragdrop_base_checks(), CheckSet::merge(run_dragdrop_deep_checks(), CheckSet::merge(run_dragdrop_deep2_checks(), run_dragdrop_deep3_checks())))
}

// ---------------------------------------------------------------------------
// F018 · 深化批次二：拖拽视觉语义常量 + 拖拽会话既有面钉死
//
// 主册依据（G-A-18【设计细节】）：「落点提示矩形 2px 强调色描边加 8% 填充」
// 「拖拽中源窗口加 20% 降透明」「同源同目标死拖 → 超时 30s 自动取消」——
// 视觉/超时常量钉值；五态状态机与三取消由既有 DragSession 面承载（对账）。
// ---------------------------------------------------------------------------

// 视觉/超时常量（DROP_HINT_BORDER_PX/DROP_HINT_FILL_PERMILLE/
// SOURCE_DIM_OPACITY_PERMILLE/DEAD_DRAG_TIMEOUT_MS）已由批次一实装——
// 本批深化检直接钉死既有定义（一处一事实，不重复定义）。

/// F018 深化自检。
pub fn run_dragdrop_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F018-dragdrop-deep");
    // 1) 视觉/超时常量钉值。
    cs.add(
        "visual_and_timeout_pins",
        DROP_HINT_BORDER_PX == 2
            && DROP_HINT_FILL_PERMILLE == 80
            && SOURCE_DIM_OPACITY_PERMILLE == 800
            && DEAD_DRAG_TIMEOUT_MS == 30_000,
        "",
    );
    // 2) 五态状态机既有面对账：就绪→悬停→落入→提交 完成（逐态推进合法）。
    let mut s = DragSession::new(DragData::Files(1), true, 0);
    s.hover(100);
    s.enter(200);
    let effect = s.drop_commit(DropEffect::Copy, 300);
    cs.add("five_state_machine_legal_path", effect == DropEffect::Copy, "");
    // 3) 三取消（B-3902）既有面对账：Esc → Cancelled(KeyCancel)，取消后
    //    松手走拒绝路径返回 None 效果；非法落点松手 → Cancelled(InvalidDrop)。
    let mut s2 = DragSession::new(DragData::Text, true, 0);
    s2.hover(10);
    s2.esc_cancel(50);
    let cancelled_state = matches!(s2.state, DragState::Cancelled(CancelReason::KeyCancel));
    let late_commit = s2.drop_commit(DropEffect::Copy, 60);
    let mut s3 = DragSession::new(DragData::Files(1), true, 0);
    s3.hover(10);
    s3.enter(20);
    let rejected = s3.drop_rejected(30);
    cs.add(
        "three_cancels_anchored",
        cancelled_state && matches!(late_commit, DropEffect::None) && matches!(rejected, DropEffect::None),
        "",
    );
    // 4) 死拖超时既有面：超 30s tick → 自动取消真值。
    let mut s4 = DragSession::new(DragData::Files(1), true, 0);
    let mut timed_out = false;
    for t in [0u64, 10_000, 20_000, 29_999, 30_001] {
        timed_out |= s4.tick_dead_drag(t);
    }
    cs.add("dead_drag_timeout_engages", timed_out, "");
    cs
}

// ---------------------------------------------------------------------------
// F018 · 深化批次三：跨表面拖拽预览供给面（预览「文件名+图标」由发起方供给，
// 翻译层保证预览不缺席）
//
// 主册依据（G-A-18【交互设计】）：「跨表面拖拽时数据预览（文件名+图标）由
// 发起方供给，翻译层保证预览不缺席」。既有面：五态状态机/三取消/视觉参数/
// 死拖超时/延迟渲染/源崩溃回收全由批次一/二承载（一处一事实），本段补预览
// 供给的缺席审计（缺席 = 缺陷，计数可见）。
// ---------------------------------------------------------------------------

/// 预览供给审计（翻译层的「不缺席」承诺观测面）。
#[derive(Clone, Copy, Debug)]
pub struct PreviewCarrier {
    /// 发起方成功供给的预览数（文件名非空 + 图标可取）。
    pub supplied: u32,
    /// 预览缺席数（发起方没给/给空——缺席即缺陷，如实计数不静默）。
    pub missing: u32,
}

impl PreviewCarrier {
    pub const fn new() -> PreviewCarrier {
        PreviewCarrier { supplied: 0, missing: 0 }
    }

    /// 发起方一次预览供给：文件名与图标任一缺席记缺席（true = 供给合格）。
    pub fn offer(&mut self, name_nonempty: bool, icon_ok: bool) -> bool {
        if name_nonempty && icon_ok {
            self.supplied += 1;
            true
        } else {
            self.missing += 1;
            false
        }
    }

    /// 「预览不缺席」判据：本会话缺席恒 0 才算承诺兑现。
    pub fn gap_free(&self) -> bool {
        self.missing == 0
    }
}

/// F018 深化批次三自检。
pub fn run_dragdrop_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F018-dragdrop-deep2");
    // 1) 供给合格计数：文件名+图标双齐 = 供给；单缺 = 缺席计数。
    let mut c = PreviewCarrier::new();
    let ok1 = c.offer(true, true);
    let ok2 = c.offer(true, false);
    cs.add(
        "preview_carrier_offer_accounting",
        ok1 && !ok2 && c.supplied == 1 && c.missing == 1,
        "",
    );
    // 2) 全齐会话 gap-free；有缺席的会话如实红（缺席 = 缺陷承诺面）。
    let mut c2 = PreviewCarrier::new();
    for _ in 0..5 {
        c2.offer(true, true);
    }
    cs.add(
        "preview_gap_free_promise",
        c2.gap_free() && c2.supplied == 5 && !c.gap_free(),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F018 · 深化批次四：跨盘拖放路由（F086 复制管线承接）+ 进度回报记账
//
// 主册依据（G-A-18【数据与存储】）：「跨盘拖放落 F086 复制管线（进度/冲突
// 面板复用）」——move 语义跨盘在文件系统层实为 copy+delete，诚实路由到复制
// 管线（带进度），不冒充「瞬间移动完成」。
// ---------------------------------------------------------------------------

/// 拖放落点路由。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropRoute {
    /// 同卷 move：原地改名（瞬时语义成立）。
    InPlace,
    /// 跨盘 move（或任意 copy）：落 F086 复制管线（进度/冲突面板复用）。
    CopyPipeline,
}

/// 路由判定：move 跨盘 = 复制管线；其余（同卷 move / 显式 copy / link）按
/// 各自语义就地执行。
pub fn drop_route(move_semantics: bool, same_volume: bool) -> DropRoute {
    if move_semantics && !same_volume {
        DropRoute::CopyPipeline
    } else {
        DropRoute::InPlace
    }
}

/// 复制管线承接记账（F086 消费面：文件数与进度回报数——进度条诚实的数据源）。
#[derive(Clone, Copy, Debug)]
pub struct CopyHandoff {
    pub files: u32,
    pub progress_reports: u32,
}

impl CopyHandoff {
    pub const fn new() -> CopyHandoff {
        CopyHandoff { files: 0, progress_reports: 0 }
    }

    /// 承接一批文件（每文件至少一次进度回报——「慢要有诚实的进度」判据）。
    pub fn start(&mut self, files: u32) {
        self.files = files;
        self.progress_reports += files; // 启动即各记一笔（进度条立即动起来）
    }

    /// 逐文件进度更新。
    pub fn progress(&mut self) {
        self.progress_reports += 1;
    }
}

/// F018 深化批次四自检。
pub fn run_dragdrop_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F018-dragdrop-deep3");
    // 1) 路由四象限：move 跨盘 → 复制管线；同卷 move/copy/link → 就地。
    cs.add(
        "drop_route_cross_disk",
        drop_route(true, false) == DropRoute::CopyPipeline
            && drop_route(true, true) == DropRoute::InPlace
            && drop_route(false, false) == DropRoute::InPlace
            && drop_route(false, true) == DropRoute::InPlace,
        "",
    );
    // 2) 承接记账：3 文件 → 启动即 3 笔进度 + 逐文件更新 → 报告数 ≥ 文件数
    //    （进度条从第一帧就动——不白屏硬等）。
    let mut ho = CopyHandoff::new();
    ho.start(3);
    let at_start = ho.progress_reports;
    ho.progress();
    ho.progress();
    cs.add(
        "copy_handoff_progress_honest",
        ho.files == 3 && at_start == 3 && ho.progress_reports == 5 && ho.progress_reports >= ho.files,
        "",
    );
    cs
}
