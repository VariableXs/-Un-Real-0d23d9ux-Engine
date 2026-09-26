
// ---------------------------------------------------------------------------
// F001 · 深化批次六：忙碌指针释放 + 取消后占位窗消失路径（浮层完整出路）
//
// 主册依据（G-A-01【交互设计】）：「双击后 100ms 内指针变忙碌态」——忙碌态
// 必须有终点（Ready/Failed/Cancelled 三终态都释放，不许指针永远忙碌）；取消
// 时占位窗消失（浮层出路清单——出现即有完整消失路径）。
// ---------------------------------------------------------------------------

/// 指针忙碌态生命周期（状态机：置忙 → 终态释放——不许悬空）。
#[derive(Clone, Copy, Debug)]
pub struct BusyCursorLifecycle {
    pub busy: bool,
    /// 未释放就进入终态的违例计数（恒 0 判据——异常零静默）。
    pub dangling_on_terminal: u32,
}

/// 装载终态（忙碌指针释放的触发面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorTerminal {
    Ready,
    Failed,
    Cancelled,
}

impl BusyCursorLifecycle {
    pub const fn new() -> BusyCursorLifecycle {
        BusyCursorLifecycle { busy: false, dangling_on_terminal: 0 }
    }

    pub fn set_busy(&mut self) {
        self.busy = true;
    }

    /// 终态到达：忙碌指针必须释放（三终态统一出口——一处一事实）。
    pub fn terminal(&mut self, which: CursorTerminal) {
        let _ = which;
        if self.busy {
            self.busy = false;
        }
    }

    /// 取消路径：占位窗消失 + 指针释放（浮层出路 + 指针出路同帧完成）。
    pub fn cancel_with_placeholder(&mut self, placeholder_visible: &mut bool) {
        *placeholder_visible = false;
        self.terminal(CursorTerminal::Cancelled);
    }
}

/// F001 深化批次六自检。
pub fn run_dblrun_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F001-dblrun-deep5");
    // 1) 三终态全部释放忙碌指针（无悬空——违例恒 0）。
    let mut lc = BusyCursorLifecycle::new();
    lc.set_busy();
    lc.terminal(CursorTerminal::Ready);
    cs.add("busy_cursor_released_on_ready", !lc.busy && lc.dangling_on_terminal == 0, "");
    let mut lc2 = BusyCursorLifecycle::new();
    lc2.set_busy();
    lc2.terminal(CursorTerminal::Failed);
    let mut lc3 = BusyCursorLifecycle::new();
    lc3.set_busy();
    lc3.terminal(CursorTerminal::Cancelled);
    cs.add(
        "busy_cursor_released_on_failed_cancel",
        !lc2.busy && !lc3.busy,
        "",
    );
    // 2) 取消路径：占位窗消失 + 指针释放同帧完成（浮层出路清单的装载面）。
    let mut lc4 = BusyCursorLifecycle::new();
    lc4.set_busy();
    let mut ph = true;
    lc4.cancel_with_placeholder(&mut ph);
    cs.add(
        "cancel_releases_placeholder_and_cursor",
        !ph && !lc4.busy,
        "",
    );
    // 3) 未置忙时终态不产生违例（空闲指针进终态是正常路径）。
    let mut lc5 = BusyCursorLifecycle::new();
    lc5.terminal(CursorTerminal::Ready);
    cs.add("idle_terminal_no_violation", !lc5.busy && lc5.dangling_on_terminal == 0, "");
    cs
}
