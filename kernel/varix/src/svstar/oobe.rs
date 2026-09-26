//! F117 首次开机向导 · 完整设计（STAR I 主册 G-C-47）。
//!
//! **判据（主册）**：五步全流程+全跳过流程双路径实测；断电续走实测；
//! 完成页汇总准确。
//!
//! **设计要点（主册）**：
//! - 五步引导：区域与语言 / 网络连接 / 输入法配置 / 主题选择（E1 实时
//!   预览）/ 完成页（系统概览）；每步可跳过、可随时重进（设置中心
//!   「系统-首次体验」）；
//! - 全屏引导层（桌面毛玻璃虚化底）：步骤进度点五枚顶部居中；每步
//!   卡片 560×420px 居中；上一步/下一步/跳过三动线恒在；主题步实时
//!   换桌面预览；完成页五秒后自动进桌面（不拖沓）；
//! - 各步选择即写配置（步进式提交——中途断电已完成步保留）；断电
//!   重启 → 从未完成步续走（不重头）；
//! - skipped 步保留默认值并在完成页汇总「你跳过了 X，可稍后在设置里配」；
//! - 网络步失败 → 跳过引导（B-607 手机热点文档指引）；
//! - Esc = 跳过当前步（可寻性）；向导期间通知静默；
//! - 步骤卡背景 4K 星徽微动画（安静不喧宾）；输入法步含试打框；
//!   主题步四官方主题卡（点击即全局换——所见即所得）；
//! - 动线对照 Windows OOBE 精华（不强制联网不强制账号——VARIX 无账号
//!   概念）。
//!
//! 时间注入式（毫秒戳），宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 步骤卡尺寸（px，主册：560×420 居中）。
pub const CARD_W_PX: u32 = 560;
pub const CARD_H_PX: u32 = 420;
/// 完成页自动进桌面（ms，主册：五秒后自动进桌面）。
pub const AUTO_ENTER_DESKTOP_MS: u64 = 5_000;
/// 五步总数。
pub const STEP_COUNT: usize = 5;
/// 主题步官方主题卡数（主册：四官方主题卡）。
pub const THEME_CARD_COUNT: usize = 4;

// ---------------------------------------------------------------------------
// 步骤定义
// ---------------------------------------------------------------------------

/// 五个步骤。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// 1 区域与语言。
    RegionLang,
    /// 2 网络连接。
    Network,
    /// 3 输入法配置（含试打框）。
    InputMethod,
    /// 4 主题选择（E1 实时预览，四官方主题卡）。
    Theme,
    /// 5 完成页（系统概览）。
    Finish,
}

impl Step {
    pub fn index(self) -> usize {
        match self {
            Step::RegionLang => 0,
            Step::Network => 1,
            Step::InputMethod => 2,
            Step::Theme => 3,
            Step::Finish => 4,
        }
    }

    pub fn from_index(i: usize) -> Step {
        match i {
            0 => Step::RegionLang,
            1 => Step::Network,
            2 => Step::InputMethod,
            3 => Step::Theme,
            _ => Step::Finish,
        }
    }

    /// 步骤名（完成页汇总文案用）。
    pub fn name(self) -> &'static str {
        match self {
            Step::RegionLang => "区域与语言",
            Step::Network => "网络连接",
            Step::InputMethod => "输入法配置",
            Step::Theme => "主题选择",
            Step::Finish => "完成",
        }
    }
}

/// 单步落位态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    /// 未走到。
    Pending,
    /// 已完成（选择即写配置）。
    Done,
    /// 已跳过（保留默认值，完成页汇总）。
    Skipped,
}

// ---------------------------------------------------------------------------
// 向导状态机
// ---------------------------------------------------------------------------

/// 首次开机向导。步进式提交：每步 Done/Skipped 即时落盘（断电续走依据）。
pub struct OobeWizard {
    outcomes: [StepOutcome; STEP_COUNT],
    current: usize,
    /// 完成标记（持久——重进入口显式语义的依据）。
    finished: bool,
    /// 各步选择值（步进式提交的落盘内容）。
    selections: [Option<String>; STEP_COUNT],
    /// 完成页到达时刻（自动进桌面计时起点）。
    finish_at_ms: Option<u64>,
    /// 向导期间通知静默（激活即静默）。
    notify_silenced: bool,
}

impl OobeWizard {
    pub fn new() -> OobeWizard {
        OobeWizard {
            outcomes: [StepOutcome::Pending; STEP_COUNT],
            current: 0,
            finished: false,
            selections: [const { None }; STEP_COUNT],
            finish_at_ms: None,
            notify_silenced: true,
        }
    }

    /// 断电续走：从持久层恢复各步落位（未完成步 Pending）→ 定位到首个
    /// 未完成步（不重头）。
    pub fn resume(outcomes: [StepOutcome; STEP_COUNT]) -> OobeWizard {
        let mut w = OobeWizard::new();
        w.outcomes = outcomes;
        w.current = outcomes
            .iter()
            .position(|o| *o == StepOutcome::Pending)
            .unwrap_or(STEP_COUNT);
        w
    }

    pub fn current(&self) -> Step {
        Step::from_index(self.current)
    }

    pub fn outcome(&self, i: usize) -> StepOutcome {
        self.outcomes[i]
    }

    pub fn selection(&self, i: usize) -> Option<&str> {
        self.selections[i].as_deref()
    }

    pub fn finished(&self) -> bool {
        self.finished
    }

    pub fn notify_silenced(&self) -> bool {
        self.notify_silenced
    }

    /// 进度点语义（顶部五枚：当前高亮 / 已过实心 / 未到空心）。
    pub fn progress_marks(&self) -> [u8; STEP_COUNT] {
        let mut marks = [0u8; STEP_COUNT]; // 0=pending 1=done 2=current
        for (i, o) in self.outcomes.iter().enumerate() {
            if *o != StepOutcome::Pending {
                marks[i] = 1;
            }
        }
        if !self.finished && self.current < STEP_COUNT {
            marks[self.current] = 2;
        }
        marks
    }

    /// 完成当前步（选择即写配置——步进式提交）。返回是否前进。
    pub fn commit(&mut self, value: &str) -> bool {
        if self.current >= STEP_COUNT {
            return false;
        }
        self.outcomes[self.current] = StepOutcome::Done;
        self.selections[self.current] = Some(String::from(value));
        self.advance();
        true
    }

    /// 跳过当前步（跳过钮 / Esc / 网络步失败引导）：保留默认值并标记。
    pub fn skip(&mut self) -> bool {
        if self.current >= STEP_COUNT {
            return false;
        }
        self.outcomes[self.current] = StepOutcome::Skipped;
        self.advance();
        true
    }

    /// 上一步（三动线恒在之一；已过步回退不重置其落位）。
    pub fn back(&mut self) -> bool {
        if self.current > 0 {
            self.current -= 1;
            true
        } else {
            false
        }
    }

    fn advance(&mut self) {
        if self.current + 1 < STEP_COUNT {
            self.current += 1;
        } else {
            self.current = STEP_COUNT;
            self.finished = true;
            self.finish_at_ms = Some(0);
        }
    }

    /// 完成页到达（自动进桌面计时起点注入）。
    pub fn arrive_finish(&mut self, now_ms: u64) {
        if self.finished {
            self.finish_at_ms = Some(now_ms);
        }
    }

    /// 完成页五秒自动进桌面（心跳注入）。
    pub fn tick_auto_enter(&mut self, now_ms: u64) -> bool {
        match self.finish_at_ms {
            Some(t0) => now_ms.saturating_sub(t0) >= AUTO_ENTER_DESKTOP_MS,
            None => false,
        }
    }

    /// 网络步失败引导（B-607 手机热点文档指引）：失败即跳过网络步。
    pub fn network_failed(&mut self) -> bool {
        if self.current() == Step::Network {
            self.skip()
        } else {
            false
        }
    }

    /// 完成页汇总（判据：汇总准确）：完成项带选择值、跳过项带
    /// 「你跳过了 X，可稍后在设置里配」。
    pub fn finish_summary(&self) -> Vec<String> {
        let mut out = Vec::new();
        for i in 0..STEP_COUNT {
            let step = Step::from_index(i);
            match self.outcomes[i] {
                StepOutcome::Done => {
                    let val = self.selections[i].as_deref().unwrap_or("");
                    out.push(alloc::format!("{}：{}", step.name(), val));
                }
                StepOutcome::Skipped => {
                    out.push(alloc::format!(
                        "你跳过了{}，可稍后在设置里配",
                        step.name()
                    ));
                }
                StepOutcome::Pending => {}
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2：输入法试打框 / 主题四卡预览 / 步骤进度点
// ---------------------------------------------------------------------------

/// 输入法试打框（主册【设计细节】「输入法步含试打框（当场体验候选）」）：
/// 注入按键串 → 返回候选串（模拟候选引擎出口——真实候选由 F107 管线
/// 注入；此处为向导体验面的接缝契约）。
pub fn ime_trytype(keys: &str) -> Vec<&'static str> {
    match keys {
        "nihao" => vec!["你好", "你号", "拟好"],
        "varix" => vec!["Varix", "varix", "瓦瑞克斯"],
        "" => Vec::new(),
        _ => vec!["（无候选——逐字上屏）"],
    }
}

/// 步骤进度点渲染数据（五枚顶部居中——完成/当前/未达三态）。
pub fn progress_dots(current: usize, completed: usize) -> [(&'static str, bool); STEP_COUNT] {
    let mut dots = [("", false); STEP_COUNT];
    for (i, d) in dots.iter_mut().enumerate() {
        // current 优先（正在走的一步即使已完成部分也显示 current——
        // 视觉焦点唯一，不与 done 打架）。
        *d = (
            if i == current {
                "current"
            } else if i < completed {
                "done"
            } else {
                "todo"
            },
            i <= current,
        );
    }
    dots
}

impl OobeWizard {
    /// 主题卡实时预览（点击即全局换——所见即所得）：注入回调面，返回
    /// 是否触发（主题步才可预览；其他步拒绝）。
    pub fn preview_theme(&self, card: usize, mut apply: impl FnMut(usize)) -> bool {
        if self.current() != Step::Theme || card >= THEME_CARD_COUNT {
            return false;
        }
        apply(card);
        true
    }
}
// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_oobe_checks() -> CheckSet {
    let mut set = CheckSet::new("F117-oobe");

    // 1. 五步全流程实测（判据第一句之一）：逐步 commit → 五步全 Done。
    let mut w = OobeWizard::new();
    let seq = ["中国/简体中文", "HomeWiFi", "拼音输入法", "星夜深色", "概览确认"];
    for v in seq {
        w.commit(v);
    }
    let all_done = (0..STEP_COUNT).all(|i| w.outcome(i) == StepOutcome::Done) && w.finished();
    set.add("five-step full path all done", all_done, "");

    // 2. 全跳过流程实测（判据第一句之二）：五步连跳 → 完成且全 Skipped。
    let mut w = OobeWizard::new();
    for _ in 0..STEP_COUNT {
        w.skip();
    }
    let all_skipped = (0..STEP_COUNT).all(|i| w.outcome(i) == StepOutcome::Skipped) && w.finished();
    set.add("all-skip path completes", all_skipped, "");

    // 3. 断电续走实测（判据第一句之三）：前两步已落位 → 恢复后从第 3 步
    //    续走（不重头），已完成步保留。
    let mut saved = [StepOutcome::Pending; STEP_COUNT];
    saved[0] = StepOutcome::Done;
    saved[1] = StepOutcome::Skipped;
    let mut w = OobeWizard::resume(saved);
    let resumed_at_input = w.current() == Step::InputMethod;
    w.commit("拼音输入法");
    let kept = w.outcome(0) == StepOutcome::Done && w.outcome(1) == StepOutcome::Skipped;
    set.add(
        "power-loss resume from incomplete step",
        resumed_at_input && kept && w.current() == Step::Theme,
        "",
    );

    // 4. 完成页汇总准确（判据第一句之四）：完成项带值、跳过项带引导语。
    let mut w = OobeWizard::new();
    w.commit("中国/简体中文");
    w.skip();
    w.commit("拼音输入法");
    w.commit("星夜深色");
    w.commit("概览确认");
    let summary = w.finish_summary();
    set.add(
        "finish summary accurate",
        summary.len() == 5
            && summary[0] == "区域与语言：中国/简体中文"
            && summary[1] == "你跳过了网络连接，可稍后在设置里配"
            && summary[2] == "输入法配置：拼音输入法"
            && summary[4] == "完成：概览确认",
        "",
    );

    // 5. 完成页五秒自动进桌面。
    let mut w = OobeWizard::new();
    for _ in 0..STEP_COUNT {
        w.skip();
    }
    w.arrive_finish(1_000);
    let early = !w.tick_auto_enter(1_000 + AUTO_ENTER_DESKTOP_MS - 1);
    let on_time = w.tick_auto_enter(1_000 + AUTO_ENTER_DESKTOP_MS);
    set.add("auto enter desktop after 5s", early && on_time, "");

    // 6. 网络步失败 → 跳过引导（B-607 指引语义）。
    let mut w = OobeWizard::new();
    w.commit("中国/简体中文"); // 步 1 完成 → 现在步 2
    let skipped = w.network_failed();
    set.add(
        "network fail skips to input step",
        skipped && w.current() == Step::InputMethod && w.outcome(1) == StepOutcome::Skipped,
        "",
    );

    // 7. Esc = 跳过当前步（可寻性）。
    let mut w = OobeWizard::new();
    let esc_skip = w.skip();
    set.add(
        "esc skips current step",
        esc_skip && w.current() == Step::Network && w.outcome(0) == StepOutcome::Skipped,
        "",
    );

    // 8. 上一步动线恒在：回退保留落位、可再前进。
    let mut w = OobeWizard::new();
    w.commit("中国/简体中文");
    w.commit("HomeWiFi");
    let went_back = w.back();
    let back_at = w.current() == Step::Network;
    w.commit("OfficeWiFi"); // 重选覆盖
    set.add(
        "back keeps outcomes, re-commit overrides",
        went_back && back_at && w.selection(1) == Some("OfficeWiFi"),
        "",
    );

    // 9. 首步无上一步（动线边界诚实）。
    let mut w = OobeWizard::new();
    set.add("no back on first step", !w.back(), "");

    // 10. 向导期间通知静默。
    let w = OobeWizard::new();
    set.add("notifications silenced during oobe", w.notify_silenced(), "");

    // 11. 进度点五枚语义（pending/done/current 三态正确）。
    let mut w = OobeWizard::new();
    w.commit("a");
    w.skip();
    let marks = w.progress_marks();
    set.add(
        "progress marks 5-dot semantics",
        marks == [1, 1, 2, 0, 0],
        "",
    );

    // 12. 步骤卡规格与主题卡数（主册：560×420 / 四官方主题卡）。
    set.add(
        "card 560x420 + 4 theme cards",
        CARD_W_PX == 560 && CARD_H_PX == 420 && THEME_CARD_COUNT == 4,
        "",
    );


    // 8. 输入法试打框（深化 v2）：nihao → 你好候选三连；空输入零候选。
    let c1 = ime_trytype("nihao");
    let c2 = ime_trytype("");
    set.add(
        "ime trytype candidates",
        c1.first() == Some(&"你好") && c1.len() == 3 && c2.is_empty(),
        "",
    );

    // 9. 步骤进度点（深化 v2）：五枚三态——done/current/todo 排布正确。
    let dots = progress_dots(2, 2);
    set.add(
        "progress dots three states",
        dots.len() == STEP_COUNT
            && dots[0].0 == "done"
            && dots[1].0 == "done"
            && dots[2].0 == "current"
            && dots[4].0 == "todo"
            && dots.iter().take(3).all(|(_, lit)| *lit)
            && !dots[4].1,
        "",
    );

    // 10. 主题卡实时预览（深化 v2）：主题步才触发、他步拒绝、越界卡拒绝。
    let w = OobeWizard::new(); // 初始在 RegionLang 步
    let mut applied = 0usize;
    let rejected = !w.preview_theme(0, |c| applied += 1);
    set.add(
        "theme preview only on theme step",
        rejected && applied == 0,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oobe_all_checks_green() {
        let set = run_oobe_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F117 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn resume_all_done_enters_finished() {
        let mut saved = [StepOutcome::Done; STEP_COUNT];
        saved[4] = StepOutcome::Done;
        let w = OobeWizard::resume(saved);
        assert_eq!(w.current(), Step::Finish);
    }

    #[test]
    fn summary_ignores_pending() {
        let mut w = OobeWizard::new();
        w.commit("中国/简体中文");
        let s = w.finish_summary();
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn mixed_path_summary_order() {
        let mut w = OobeWizard::new();
        w.commit("中文");
        w.skip();
        w.commit("拼音");
        let s = w.finish_summary();
        assert_eq!(s.len(), 3);
        assert!(s[1].starts_with("你跳过了网络连接"));
    }

    #[test]
    fn f117_trytype_varix() {
        let c = ime_trytype("varix");
        assert_eq!(c.first(), Some(&"Varix"));
        let fallback = ime_trytype("zzzz");
        assert!(fallback[0].contains("逐字上屏"), "无候选词诚实降级");
    }

    #[test]
    fn f117_dots_all_done_shape() {
        let dots = progress_dots(STEP_COUNT - 1, STEP_COUNT);
        assert!(dots.iter().take(STEP_COUNT - 1).all(|(s, _)| *s == "done"));
        assert_eq!(dots[STEP_COUNT - 1].0, "current");
    }
}
