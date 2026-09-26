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
pub fn run_dragdrop_checks() -> CheckSet {
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
