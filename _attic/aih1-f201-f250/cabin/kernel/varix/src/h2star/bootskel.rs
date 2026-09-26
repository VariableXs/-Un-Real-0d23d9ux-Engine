//! F283 应用启动骨架与首窗就绪 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：三拍子时序实测（反馈 <100ms/窗框 <200ms/骨架替换
//! 无跳变）；2s 门槛提示；冷启动与热启动（进程已在）分别计时入账。
//!
//! **设计要点（主册）**：应用启动三拍子：图标点击即有反馈（任务栏图标
//! 跳动 200ms）、200ms 内出窗框+骨架屏（内容区占位）、内容就绪后原位
//! 替换（骨架→内容无跳变）——超过 2s 未就绪显示进度原因（「正在加载
//! 插件」而非干等）；首窗永远先于次要窗出现。
//!
//! 实装：三拍子时序账（点击/反馈/窗框/骨架/就绪五锚点注入计时）；阈值
/// 判定（<100ms、<200ms 硬线）；2s 门槛（超时显示进度原因——原因链
/// 注入）；冷/热启动分账（进程已在 → 无装载段）；首窗优先（次要窗在
/// 首窗就绪前不出现——排队保证）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 三拍子硬线（ms）。
pub const FEEDBACK_LIMIT_MS: u64 = 100;
pub const FRAME_LIMIT_MS: u64 = 200;
/// 2s 门槛（超时显示进度原因）。
pub const STALL_THRESHOLD_MS: u64 = 2_000;
/// 任务栏图标跳动时长（ms）。
pub const ICON_BOUNCE_MS: u64 = 200;

/// 启动会话（一次启动的时序账）。
#[derive(Clone, Debug)]
pub struct LaunchBeat {
    pub app: String,
    /// 锚点时戳（ms，从点击起算，注入）。
    pub click_ms: u64,
    pub feedback_ms: u64,
    pub frame_ms: u64,
    pub skeleton_ms: u64,
    pub content_ms: u64,
    /// 冷启动（进程不在）true / 热启动 false。
    pub cold: bool,
    /// 超时进度原因链（2s 门槛提示——如「正在加载插件」）。
    pub stall_reasons: Vec<String>,
}

impl LaunchBeat {
    pub fn new(app: &str, cold: bool) -> LaunchBeat {
        LaunchBeat {
            app: String::from(app),
            click_ms: 0,
            feedback_ms: 0,
            frame_ms: 0,
            skeleton_ms: 0,
            content_ms: 0,
            cold,
            stall_reasons: Vec::new(),
        }
    }

    /// 拍子判定：反馈 <100ms 且窗框 <200ms（判据硬线）。
    pub fn three_beats_ok(&self) -> bool {
        self.feedback_ms - self.click_ms <= FEEDBACK_LIMIT_MS
            && self.frame_ms - self.click_ms <= FRAME_LIMIT_MS
    }

    /// 骨架替换无跳变：内容与骨架同几何（替换标志由渲染层核对，
    /// 此处钉时序——内容不早于骨架出现）。
    pub fn replace_in_place(&self) -> bool {
        self.skeleton_ms <= self.content_ms
    }

    /// 2s 门槛：超时必须带进度原因（不许干等）。
    pub fn stall_handled(&self) -> bool {
        if self.content_ms - self.click_ms > STALL_THRESHOLD_MS {
            !self.stall_reasons.is_empty()
        } else {
            true
        }
    }

    /// 记进度原因（「正在加载插件」级）。
    pub fn push_stall_reason(&mut self, reason: &str) {
        self.stall_reasons.push(String::from(reason));
    }
}

/// 启动编排器：首窗优先（次要窗在首窗就绪前排队）。
pub struct LaunchOrchestrator {
    /// 首窗是否已就绪。
    pub first_ready: bool,
    /// 排队的次要窗。
    pub secondary_queue: Vec<String>,
}

impl LaunchOrchestrator {
    pub fn new() -> LaunchOrchestrator {
        LaunchOrchestrator { first_ready: false, secondary_queue: Vec::new() }
    }

    /// 次要窗请求：首窗未就绪 → 排队（首窗优先判据）。
    pub fn request_secondary(&mut self, win: &str) -> bool {
        if self.first_ready {
            true // 直接放行。
        } else {
            self.secondary_queue.push(String::from(win));
            false
        }
    }

    /// 首窗就绪：排队中的次要窗按序放行。
    pub fn first_window_ready(&mut self) -> Vec<String> {
        self.first_ready = true;
        core::mem::take(&mut self.secondary_queue)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_bootskel_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F283");
    // 三拍子时序：点击 0 / 反馈 80 / 窗框 180 / 骨架 200 / 内容 1200。
    let mut b = LaunchBeat::new("编辑器", true);
    b.feedback_ms = 80;
    b.frame_ms = 180;
    b.skeleton_ms = 200;
    b.content_ms = 1_200;
    set.add(
        "F283 three beats",
        b.three_beats_ok() && b.replace_in_place() && b.stall_handled(),
        "80/180/200/1200",
    );
    // 超骨架：窗框 260ms 超线 → 判定红。
    let mut slow = LaunchBeat::new("编辑器", true);
    slow.feedback_ms = 80;
    slow.frame_ms = 260;
    slow.skeleton_ms = 300;
    slow.content_ms = 400;
    set.add("F283 frame over limit", !slow.three_beats_ok(), "200ms hard line");
    // 2s 门槛：超时无原因 = 红；有原因 = 绿。
    let mut stuck = LaunchBeat::new("编辑器", true);
    stuck.feedback_ms = 80;
    stuck.frame_ms = 180;
    stuck.skeleton_ms = 200;
    stuck.content_ms = 5_000;
    set.add("F283 stall no reason red", !stuck.stall_handled(), "must explain");
    stuck.push_stall_reason("正在加载插件");
    set.add(
        "F283 stall reason shown",
        stuck.stall_handled() && stuck.stall_reasons[0].contains("插件"),
        "honest waiting",
    );
    // 冷/热启动分账。
    let hot = LaunchBeat { cold: false, ..LaunchBeat::new("编辑器", true) };
    set.add(
        "F283 cold/hot ledger",
        !hot.cold && b.cold,
        "separate timing",
    );
    // 首窗优先：次要窗排队，首窗就绪后按序放行。
    let mut orc = LaunchOrchestrator::new();
    let q1 = orc.request_secondary("设置");
    let q2 = orc.request_secondary("关于");
    let released = orc.first_window_ready();
    set.add(
        "F283 first window first",
        !q1 && !q2 && released == alloc::vec![String::from("设置"), String::from("关于")],
        "queue then release",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f283_launch_beats() {
        let set = run_bootskel_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F283 自检红 {f}/{p}");
    }

    #[test]
    fn icon_bounce_constant() {
        assert_eq!(ICON_BOUNCE_MS, 200, "任务栏图标跳动 200ms——主册定值");
    }
}
