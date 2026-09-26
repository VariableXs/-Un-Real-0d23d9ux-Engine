//! F118 欢迎中心 · 完整设计（STAR I 主册 G-C-48）。
//!
//! **判据（主册）**：五卡内容过审（概念准确：双域/无商店/交接三大心智
//! 零歧义）；行动钮直跳路径全对；不二弹实测。
//!
//! **设计要点（主册）**：
//! - 开机首日弹「了解 VARIX」交互卡五张：双域概念 / 软件下载（无商店
//!   原则宣导）/ 域切换 / 个性化入口 / 帮助中心入口；看完即走、可找回
//!   （设置+帮助中心）；不再自动弹第二次；
//! - 卡片流：全屏层内横向卡轮播（560×420px）+ 进度点 + 「跳过全部」
//!   恒在（首卡即显——不绑架）；每卡一个核心图示 + 标题 + 两行文案
//!   （≤60 字）+ 行动钮（直跳对应功能）；完成态卡「开始使用」收尾；
//! - 行动钮直跳路径：下载卡开浏览器 / 个性化卡开 E1 页 / 帮助卡开
//!   F119 / 双域卡开域切换；
//! - 完成标记持久；「再次查看」入口在帮助中心（F119）；完成标记可
//!   重置（演示/教学场景）；
//! - 中途退出 → 记录进度下次不再自动弹（只可手动看）；
//! - 插画资产缺失 → 纯文字版降级；
//! - 卡片切换 250ms 横滑；文案走 F140 词条库（可本地化）；
//! - 图示风格=壁纸引擎同族（视觉统一）。
//!
//! 时间注入式（毫秒戳），宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 卡片尺寸（px，主册：560×420）。
pub const CARD_W_PX: u32 = 560;
pub const CARD_H_PX: u32 = 420;
/// 卡片切换横滑时长（ms，主册：250ms）。
pub const SLIDE_MS: u64 = 250;
/// 卡片数（主册：五张）。
pub const CARD_COUNT: usize = 5;
/// 每卡文案上限（字，主册：每卡文案 ≤60 字）。
pub const COPY_MAX_CHARS: usize = 60;

// ---------------------------------------------------------------------------
// 卡片定义（五张 · 三大心智零歧义）
// ---------------------------------------------------------------------------

/// 行动钮直跳目标（路径注册制——新增卡必须登记直跳路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JumpTarget {
    /// 双域概念卡 → 域切换面。
    DomainSwitch,
    /// 软件下载卡 → 浏览器（无商店原则）。
    Browser,
    /// 域切换卡 → 域切换面（与首卡同口——双域心智强化）。
    DomainSwitch2,
    /// 个性化入口卡 → E1 个性化设置页。
    PersonalizeE1,
    /// 帮助中心入口卡 → F119。
    HelpCenter,
}

/// 一张欢迎卡。
#[derive(Clone, Copy, Debug)]
pub struct WelcomeCard {
    pub id: &'static str,
    pub title: &'static str,
    /// 两行文案（≤60 字——构建期即锁）。
    pub copy: &'static str,
    pub jump: JumpTarget,
    /// 核心图示资产键（缺失 → 纯文字版降级）。
    pub illustration: &'static str,
}

/// 官方五卡（主册定义顺序；文案已按 ≤60 字核定）。
pub const CARDS: [WelcomeCard; CARD_COUNT] = [
    WelcomeCard {
        id: "dual-domain",
        title: "一个系统，两个世界",
        copy: "VARIX 双域：本机域干活，兼容域跑 Windows 软件。互相隔离，互不打扰。",
        jump: JumpTarget::DomainSwitch,
        illustration: "wallpaper-family/dual-globe",
    },
    WelcomeCard {
        id: "no-store",
        title: "软件去浏览器下载",
        copy: "VARIX 不建软件商店。用浏览器从官网下载，安装包格式开放可查。",
        jump: JumpTarget::Browser,
        illustration: "wallpaper-family/compass",
    },
    WelcomeCard {
        id: "domain-switch",
        title: "随时切换域",
        copy: "交接预检器盯着每次切换：文件交接、时钟同步，一切自动完成。",
        jump: JumpTarget::DomainSwitch2,
        illustration: "wallpaper-family/bridge",
    },
    WelcomeCard {
        id: "personalize",
        title: "把它变成你的",
        copy: "主题、壁纸、指针、声音都能换。改了立即生效，不喜欢随时还原。",
        jump: JumpTarget::PersonalizeE1,
        illustration: "wallpaper-family/palette",
    },
    WelcomeCard {
        id: "help",
        title: "遇到问题找帮助",
        copy: "帮助中心离线可用，每篇指南都能一键跳到对应设置页。",
        jump: JumpTarget::HelpCenter,
        illustration: "wallpaper-family/lighthouse",
    },
];

/// 卡文案字数（构建期口径：每卡 ≤60 字——三行文案逐卡断言）。
pub fn card_copy_len(c: &WelcomeCard) -> usize {
    c.copy.chars().count()
}

/// 行动钮直跳路径全对判据：每卡的跳转目标与其心智一一对应（注册制
/// 校验——改卡必须同步改路径，错位即红）。
pub fn jump_paths_consistent() -> bool {
    CARDS[0].jump == JumpTarget::DomainSwitch
        && CARDS[1].jump == JumpTarget::Browser
        && CARDS[2].jump == JumpTarget::DomainSwitch2
        && CARDS[3].jump == JumpTarget::PersonalizeE1
        && CARDS[4].jump == JumpTarget::HelpCenter
}

// ---------------------------------------------------------------------------
// 欢迎中心状态机
// ---------------------------------------------------------------------------

/// 欢迎中心状态。
pub struct WelcomeCenter {
    /// 当前卡索引。
    cursor: usize,
    /// 自动弹出资格（完成标记持久：不再自动弹第二次——不二弹判据）。
    auto_eligible: bool,
    /// 退出时记录的进度（中途退出 → 记录进度，下次不再自动弹）。
    exit_progress: Option<usize>,
    /// 切换动画起点（250ms 横滑对账）。
    slide_started_ms: Option<u64>,
    /// 插画资产可用性（缺失 → 纯文字版降级）。
    illustrations_ok: bool,
    /// 「跳过全部」恒在（首卡即显）。
    skip_all_visible: bool,
}

impl WelcomeCenter {
    pub fn new(auto_eligible: bool) -> WelcomeCenter {
        WelcomeCenter {
            cursor: 0,
            auto_eligible,
            exit_progress: None,
            slide_started_ms: None,
            illustrations_ok: true,
            skip_all_visible: true,
        }
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn auto_eligible(&self) -> bool {
        self.auto_eligible
    }

    pub fn exit_progress(&self) -> Option<usize> {
        self.exit_progress
    }

    pub fn skip_all_visible(&self) -> bool {
        self.skip_all_visible
    }

    /// 自动弹出资格判定（开机首日）：完成标记未置 + 无中途退出记录。
    pub fn should_auto_popup(&self) -> bool {
        self.auto_eligible && self.exit_progress.is_none()
    }

    /// 下一卡（250ms 横滑起表）。
    pub fn next(&mut self, now_ms: u64) -> bool {
        if self.cursor + 1 < CARD_COUNT {
            self.cursor += 1;
            self.slide_started_ms = Some(now_ms);
            true
        } else {
            false
        }
    }

    /// 切换动画完成确认（注入耗时 ≤250ms 判线）。
    pub fn slide_done(&mut self, elapsed_ms: u64) -> bool {
        self.slide_started_ms = None;
        elapsed_ms <= SLIDE_MS
    }

    /// 中途退出：记录进度 + 撤销自动弹资格（下次不再自动弹——只可
    /// 手动看）。
    pub fn exit_early(&mut self) -> usize {
        self.exit_progress = Some(self.cursor);
        self.auto_eligible = false;
        self.cursor
    }

    /// 看完即走：完成标记置位（持久），自动弹资格撤销。
    pub fn complete(&mut self) {
        self.auto_eligible = false;
        self.exit_progress = None;
    }

    /// 完成标记重置（演示/教学场景——主册：完成标记可重置）。
    pub fn reset_completion(&mut self) {
        self.auto_eligible = true;
        self.exit_progress = None;
        self.cursor = 0;
    }

    /// 插画资产缺失降级（纯文字版）。
    pub fn probe_illustrations_missing(&mut self) {
        self.illustrations_ok = false;
    }

    /// 渲染形态（图示可用 = 图卡，缺失 = 纯文字卡）。
    pub fn render_mode(&self) -> &'static str {
        if self.illustrations_ok {
            "illustrated"
        } else {
            "text-only"
        }
    }

    /// 「再次查看」手动入口（帮助中心 F119 呼出）——不依赖自动资格。
    pub fn open_manually(&mut self) {
        self.cursor = 0;
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2：横滑判线 / 中途进度记录
// ---------------------------------------------------------------------------

/// 横滑判线结果（250ms 横滑——主册【设计细节】）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwipeVerdict {
    /// 位移过阈值 → 翻下一卡。
    Advance,
    /// 位移过阈值（反向）→ 回上一卡。
    Back,
    /// 位移不足 → 回弹原位（橡皮筋）。
    RubberBand,
}

/// 横滑判线：拖拽位移 px 过 ±120px 判翻页，否则回弹。
pub fn swipe_verdict(dx_px: i32) -> SwipeVerdict {
    const THRESHOLD_PX: i32 = 120;
    if dx_px <= -THRESHOLD_PX {
        SwipeVerdict::Advance
    } else if dx_px >= THRESHOLD_PX {
        SwipeVerdict::Back
    } else {
        SwipeVerdict::RubberBand
    }
}
// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_welcome_checks() -> CheckSet {
    let mut set = CheckSet::new("F118-welcome");

    // 1. 五卡内容过审：三大心智零歧义（双域/无商店/交接逐卡钉文）。
    set.add(
        "five cards carry three mindsets",
        CARDS[0].title.contains("两个世界")
            && CARDS[1].title.contains("浏览器")
            && CARDS[2].copy.contains("交接预检器"),
        "",
    );

    // 2. 每卡文案 ≤60 字且内容足量（五卡逐卡字数实测——≤60 上限 +
    //    ≥20 下限防注水文案）。
    let lens: [usize; CARD_COUNT] = [
        card_copy_len(&CARDS[0]),
        card_copy_len(&CARDS[1]),
        card_copy_len(&CARDS[2]),
        card_copy_len(&CARDS[3]),
        card_copy_len(&CARDS[4]),
    ];
    set.add(
        "all card copy within 60 chars, above 20",
        lens.iter().all(|&l| l <= COPY_MAX_CHARS) && lens.iter().all(|&l| l >= 20),
        "",
    );

    // 3. 行动钮直跳路径全对（判据第一句之二：注册制逐卡校验）。
    set.add("jump paths all consistent", jump_paths_consistent(), "");

    // 4. 不二弹实测（判据第一句之三）：看完即走 → 不再自动弹；
    //    中途退出 → 也不自动弹（只可手动看）。
    let mut w = WelcomeCenter::new(true);
    let first_popup = w.should_auto_popup();
    w.complete();
    let never_again_after_complete = !w.should_auto_popup();
    let mut w2 = WelcomeCenter::new(true);
    w2.next(0);
    w2.exit_early();
    let never_again_after_exit = !w2.should_auto_popup() && w2.exit_progress() == Some(1);
    set.add(
        "no second auto popup (complete or exit)",
        first_popup && never_again_after_complete && never_again_after_exit,
        "",
    );

    // 5. 卡片轮播 + 「跳过全部」首卡即显（不绑架）：轮播到末卡即停
    //    （连推十次恰前进四步——第五张后拒绝）。
    let mut w = WelcomeCenter::new(true);
    let mut advanced = 0;
    for t in 0..10u64 {
        if w.next(t * 10) {
            advanced += 1;
        }
    }
    let stuck_at_end = advanced == CARD_COUNT - 1 && w.cursor() == CARD_COUNT - 1;
    set.add(
        "carousel bounded + skip-all always visible",
        stuck_at_end && w.skip_all_visible() && WelcomeCenter::new(true).skip_all_visible(),
        "",
    );

    // 6. 卡片切换 250ms 横滑（判线注入实测）。
    let mut w = WelcomeCenter::new(true);
    w.next(1_000);
    let in_budget = w.slide_done(250) && !w.slide_done(251);
    set.add("slide transition within 250ms", in_budget, "");

    // 7. 完成标记重置（演示/教学场景）→ 恢复自动弹资格 + 回首卡。
    let mut w = WelcomeCenter::new(true);
    w.complete();
    w.reset_completion();
    set.add(
        "completion resettable for demo",
        w.should_auto_popup() && w.cursor() == 0,
        "",
    );

    // 8. 插画缺失 → 纯文字版降级（诚实降级路径）。
    let mut w = WelcomeCenter::new(true);
    w.probe_illustrations_missing();
    set.add("missing illustrations degrade text-only", w.render_mode() == "text-only", "");

    // 9. 「再次查看」手动入口：不依赖自动资格（帮助中心呼出）。
    let mut w = WelcomeCenter::new(false);
    w.open_manually();
    set.add(
        "manual re-entry independent of auto flag",
        w.cursor() == 0 && !w.auto_eligible(),
        "",
    );

    // 10. 卡片规格与五卡数（560×420 / 五张）。
    set.add(
        "card 560x420 + 5 cards",
        CARD_W_PX == 560 && CARD_H_PX == 420 && CARDS.len() == 5,
        "",
    );


    // 8. 横滑判线（深化 v2）：-121 翻下卡 / +121 回上卡 / ±119 回弹。
    set.add(
        "swipe verdict three branches",
        swipe_verdict(-121) == SwipeVerdict::Advance
            && swipe_verdict(121) == SwipeVerdict::Back
            && swipe_verdict(0) == SwipeVerdict::RubberBand
            && swipe_verdict(119) == SwipeVerdict::RubberBand
            && swipe_verdict(-120) == SwipeVerdict::Advance,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_all_checks_green() {
        let set = run_welcome_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F118 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn card_copy_lengths_stable() {
        // 文案是过审件——字数漂移即审失（逐卡复核）。
        for (i, c) in CARDS.iter().enumerate() {
            assert!(
                card_copy_len(c) <= COPY_MAX_CHARS,
                "卡 {} 文案超字数：{}",
                i,
                card_copy_len(c)
            );
        }
    }

    #[test]
    fn exit_early_records_cursor() {
        let mut w = WelcomeCenter::new(true);
        w.next(0);
        w.next(1);
        w.next(2);
        let at = w.exit_early();
        assert_eq!(at, 3);
        assert_eq!(w.exit_progress(), Some(3));
    }

    #[test]
    fn fresh_session_defaults() {
        let w = WelcomeCenter::new(true);
        assert_eq!(w.render_mode(), "illustrated");
        assert!(w.skip_all_visible());
    }

    #[test]
    fn f118_swipe_symmetric() {
        // 对称性：等大反向位移判线互为镜像（手感一致性）。
        for dx in [-125i32, -121, 121, 125] {
            let a = swipe_verdict(dx);
            let b = swipe_verdict(-dx);
            assert_eq!(
                matches!(a, SwipeVerdict::Advance),
                matches!(b, SwipeVerdict::Back)
            );
        }
    }
}
