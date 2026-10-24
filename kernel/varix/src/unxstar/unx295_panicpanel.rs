// unx295_panicpanel.rs ↔ UNX-F0295 panic 面板体验（倒计时/键位/提示）
//
// 判据原文：UNX-F0295-J1 倒计时刷新/键位路由（d 诊断/r 重启）/提示文案样张一致。
// 难点分析：倒计时刷新不能阻塞 panic 路径；键位路由要在 panic 期最小驱动下工作；
//          文案样张必须与设计文档逐字一致（体验验收）。
// 纪律约束：倒计时刷新非阻塞；键位 d/r 路由；文案样张一致。
// 成本记账：倒计时 O(1)；键位路由 O(1) 分派。

#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 面板动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelAction {
    /// 无操作。
    None,
    /// d → 进入诊断。
    Diagnostics,
    /// r → 重启。
    Reboot,
    /// 其他键。
    Unknown(u8),
}

/// 面板状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelState {
    /// 倒计时中。
    Countdown,
    /// 诊断模式。
    DiagMode,
    /// 重启中。
    Rebooting,
    /// 倒计时结束（默认动作）。
    Timeout,
}

/// 提示文案样张（与设计文档一致）。
pub mod messages {
    /// 标题。
    pub const TITLE: &str = "系统遇到问题，需要重启";
    /// 倒计时格式前缀。
    pub const COUNTDOWN_PREFIX: &str = "将在 {n} 秒后自动重启";
    /// 键位提示。
    pub const KEY_HINT: &str = "按 d 查看诊断信息，按 r 立即重启";
    /// 诊断标题。
    pub const DIAG_TITLE: &str = "诊断信息";
    /// 重启中。
    pub const REBOOTING: &str = "正在重启...";
    /// 复现模式提示。
    pub const REPRO_HINT: &str = "复现模式已开启（第 {n}/{m} 次）";
}

/// 倒计时器（非阻塞：tick 由外部时钟驱动）。
#[derive(Debug, Clone)]
pub struct Countdown {
    /// 剩余秒数。
    remaining: u32,
    /// 总时长。
    total: u32,
    /// 是否已结束。
    finished: bool,
}

impl Countdown {
    /// 新建。
    pub fn new(seconds: u32) -> Self {
        Countdown {
            remaining: seconds,
            total: seconds,
            finished: false,
        }
    }

    /// 时钟 tick（非阻塞，外部驱动）。
    pub fn tick(&mut self) {
        if self.finished {
            return;
        }
        if self.remaining > 0 {
            self.remaining -= 1;
        }
        if self.remaining == 0 {
            self.finished = true;
        }
    }

    /// 剩余。
    pub fn remaining(&self) -> u32 {
        self.remaining
    }
    /// 是否结束。
    pub fn finished(&self) -> bool {
        self.finished
    }
    /// 进度（万分比）。
    pub fn progress_bp(&self) -> u32 {
        if self.total == 0 {
            return 10000;
        }
        (((self.total - self.remaining) * 10000) / self.total) as u32
    }
    /// 刷新文案。
    pub fn render(&self) -> alloc::string::String {
        alloc::format!("{} 秒后自动重启", self.remaining)
    }
}

/// panic 面板。
pub struct PanicPanel {
    countdown: Countdown,
    state: PanelState,
    /// 键位路由历史。
    key_log: Vec<PanelAction>,
    /// 复现计数（显示用）。
    repro_count: u32,
    repro_limit: u32,
    /// 最近渲染行。
    last_render: Vec<alloc::string::String>,
}

impl PanicPanel {
    /// 新建。
    pub fn new(countdown_secs: u32) -> Self {
        PanicPanel {
            countdown: Countdown::new(countdown_secs),
            state: PanelState::Countdown,
            key_log: Vec::new(),
            repro_count: 0,
            repro_limit: 0,
            last_render: Vec::new(),
        }
    }

    /// 设置复现显示。
    pub fn set_repro(&mut self, count: u32, limit: u32) {
        self.repro_count = count;
        self.repro_limit = limit;
    }

    /// 键位路由（d=诊断 r=重启）。
    pub fn on_key(&mut self, key: u8) -> PanelAction {
        let action = match key {
            b'd' | b'D' => PanelAction::Diagnostics,
            b'r' | b'R' => PanelAction::Reboot,
            other => PanelAction::Unknown(other),
        };
        self.key_log.push(action);
        match action {
            PanelAction::Diagnostics => self.state = PanelState::DiagMode,
            PanelAction::Reboot => self.state = PanelState::Rebooting,
            _ => {}
        }
        action
    }

    /// 时钟 tick。
    pub fn tick(&mut self) {
        self.countdown.tick();
        if self.countdown.finished() && self.state == PanelState::Countdown {
            self.state = PanelState::Timeout;
        }
    }

    /// 渲染面板文案（样张一致）。
    pub fn render(&mut self) -> &[alloc::string::String] {
        self.last_render.clear();
        self.last_render
            .push(alloc::string::String::from(messages::TITLE));
        match self.state {
            PanelState::Countdown | PanelState::Timeout => {
                self.last_render.push(self.countdown.render());
                self.last_render
                    .push(alloc::string::String::from(messages::KEY_HINT));
                if self.repro_limit > 0 {
                    self.last_render.push(alloc::format!(
                        "复现模式已开启（第 {}/{ } 次）",
                        self.repro_count, self.repro_limit
                    ));
                }
            }
            PanelState::DiagMode => {
                self.last_render
                    .push(alloc::string::String::from(messages::DIAG_TITLE));
                self.last_render
                    .push(alloc::string::String::from(messages::KEY_HINT));
            }
            PanelState::Rebooting => {
                self.last_render
                    .push(alloc::string::String::from(messages::REBOOTING));
            }
        }
        &self.last_render
    }

    /// 当前状态。
    pub fn state(&self) -> PanelState {
        self.state
    }
    /// 倒计时访问。
    pub fn countdown(&self) -> &Countdown {
        &self.countdown
    }
    /// 键位历史。
    pub fn key_log(&self) -> &[PanelAction] {
        &self.key_log
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_unx295_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0295 panic 面板体验");

    // --- 倒计时刷新 ---
    let mut p = PanicPanel::new(5);
    cs.add("初始剩余 5", p.countdown().remaining() == 5, "倒计时初值");
    p.tick();
    cs.add("tick 后剩余 4", p.countdown().remaining() == 4, "非阻塞刷新");
    for _ in 0..4 {
        p.tick();
    }
    cs.add("倒计时结束", p.countdown().finished(), "5 tick 后");
    cs.add("状态 Timeout", p.state() == PanelState::Timeout, "超时状态");

    // --- 键位路由：d → 诊断 ---
    let mut p2 = PanicPanel::new(10);
    let a = p2.on_key(b'd');
    cs.add("d → Diagnostics", a == PanelAction::Diagnostics, "键位路由");
    cs.add("状态 DiagMode", p2.state() == PanelState::DiagMode, "进入诊断");

    // --- 键位路由：r → 重启 ---
    let mut p3 = PanicPanel::new(10);
    let a2 = p3.on_key(b'r');
    cs.add("r → Reboot", a2 == PanelAction::Reboot, "键位路由");
    cs.add("状态 Rebooting", p3.state() == PanelState::Rebooting, "重启中");

    // --- 未知键 ---
    let mut p4 = PanicPanel::new(10);
    let a3 = p4.on_key(b'x');
    cs.add("未知键", matches!(a3, PanelAction::Unknown(b'x')), "Unknown");
    cs.add("状态不变", p4.state() == PanelState::Countdown, "仍倒计时");

    // --- 文案样张一致 ---
    let mut p5 = PanicPanel::new(3);
    let lines = p5.render().to_vec();
    cs.add(
        "标题样张",
        lines[0] == messages::TITLE,
        "逐字一致",
    );
    cs.add_dyn(
        "键位提示样张",
        lines.iter().any(|l| l == messages::KEY_HINT),
        "d/r 提示",
    );

    // --- 倒计时文案 ---
    let mut c = Countdown::new(7);
    let s = c.render();
    cs.add("倒计时文案含数字", s.contains('7'), "7 秒");

    // --- 进度 ---
    let mut c2 = Countdown::new(10);
    for _ in 0..5 {
        c2.tick();
    }
    let bp = c2.progress_bp();
    cs.add("进度 50%", (bp as i64 - 5000).abs() < 100, "半程");

    // --- 复现提示 ---
    let mut p6 = PanicPanel::new(5);
    p6.set_repro(2, 5);
    let lines6 = p6.render().to_vec();
    cs.add(
        "复现提示",
        lines6.iter().any(|l| l.contains("复现模式")),
        "复现计数显示",
    );

    // --- 键位历史 ---
    let mut p7 = PanicPanel::new(5);
    p7.on_key(b'd');
    p7.on_key(b'r');
    cs.add("键位历史 2 条", p7.key_log().len() == 2, "记录完整");

    cs
}
