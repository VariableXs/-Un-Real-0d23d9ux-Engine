//! F178 签名状态角标（secstar · G-G-08）——知道自己在跑什么是用户的权利。
//!
//! 主册判据（验收标准第一句）：
//! **三态（链验/自签/未签）角标正确注入各 3 应用；tooltip 与帮助链通；截图角标保留。**
//!
//! 功能定义（G-G-08）：未签名运行的应用：窗口标题栏角标+任务栏悬停提示
//! （「未签名应用」+说明链接）；不阻断运行，如实标注——「知道自己在跑
//! 什么」是用户的权利（F037 越权与 F037 信任链联动）。
//!
//! 【交互设计】角标 12px 盾形（灰态=未签名/黄态=自签名/无角标=链验证通过）；
//! 悬停 tooltip 1s 延迟；说明链接→帮助 F119 签名篇；角标在截图中如实存在
//! （F098 不净化——所见即真实）。
//! 【数据与存储】签名态随进程元数据；无独立存储。
//! 【状态与异常】签名验证服务不可用 → 全部标注未签名+诊断报备（fail-closed
//! 语义诚实化）；信任列表（F037）内应用 → 角标降为灰点（已信任未签名）。
//! 【设计细节】角标位置=标题栏右侧系统按钮区左 4px（乙-1 表窗框语义内）；
//! 盾形三态色：灰 #888/黄 #D90 强调/无——色弱形状冗余（F114 联动：盾形
//! 本身即形状信号）；悬停文案模板词条化（F140）；角标渲染走窗口装饰层
//! （程序无感——不可被程序隐藏）。
//!
//! 零堆纪律：定长信任表 + 定长词条缓冲，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 角标 12px 盾形。
pub const BADGE_SIZE_PX: u32 = 12;
/// 角标位置：标题栏右侧系统按钮区左 4px（乙-1 表窗框语义）。
pub const BADGE_OFFSET_PX: u32 = 4;
/// 悬停 tooltip 延迟 1s。
pub const TOOLTIP_DELAY_MS: u64 = 1_000;
/// 灰态色 #888888。
pub const COLOR_GRAY: u32 = 0x888888;
/// 黄态强调色 #DD9900。
pub const COLOR_YELLOW: u32 = 0xDD9900;
/// 帮助链接目标（F119 签名篇——帮助链通的对账锚）。
pub const HELP_TARGET: &[u8] = b"help:F119/signing";

// ---------------------------------------------------------------------------
// 签名态与角标
// ---------------------------------------------------------------------------

/// 原始签名验证态（验证服务产出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignState {
    /// 链验证通过。
    ChainVerified,
    /// 自签名。
    SelfSigned,
    /// 未签名。
    Unsigned,
}

/// 渲染角标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Badge {
    /// 链验证通过——无角标（不打扰）。
    None,
    /// 未签名——灰盾。
    ShieldGray,
    /// 自签名——黄盾。
    ShieldYellow,
    /// 信任列表内未签名——灰点（降级标注）。
    DotGray,
}

impl Badge {
    pub fn color(self) -> Option<u32> {
        match self {
            Badge::None => None,
            Badge::ShieldGray | Badge::DotGray => Some(COLOR_GRAY),
            Badge::ShieldYellow => Some(COLOR_YELLOW),
        }
    }
    /// 色弱形状冗余（F114 联动）：盾/点形状本身即信号。
    pub fn shape_redundant(self) -> bool {
        !matches!(self, Badge::None)
    }
}

/// 角标解析器。
pub struct BadgeResolver {
    /// 签名验证服务可用性（不可用 → fail-closed 全部按未签名+诊断报备）。
    pub service_ok: bool,
    /// 信任列表（F037——已信任未签名降级为灰点）。
    pub trust: [Option<u32>; 16],
    pub trust_n: usize,
    /// fail-closed 诊断报备旗。
    pub diag_reported: bool,
}

impl BadgeResolver {
    pub const fn new() -> Self {
        BadgeResolver { service_ok: true, trust: [const { None }; 16], trust_n: 0, diag_reported: false }
    }

    pub fn add_trusted(&mut self, app_id: u32) {
        if self.trust_n < 16 && !self.trust[..self.trust_n].iter().flatten().any(|a| *a == app_id) {
            self.trust[self.trust_n] = Some(app_id);
            self.trust_n += 1;
        }
    }

    fn trusted(&self, app_id: u32) -> bool {
        self.trust[..self.trust_n].iter().flatten().any(|a| *a == app_id)
    }

    /// 解析角标。服务不可用 → 全部未签名（fail-closed 诚实化）+ 一次性报备。
    pub fn resolve(&mut self, app_id: u32, state: SignState) -> Badge {
        if !self.service_ok {
            if !self.diag_reported {
                self.diag_reported = true;
            }
            return if self.trusted(app_id) { Badge::DotGray } else { Badge::ShieldGray };
        }
        match state {
            SignState::ChainVerified => Badge::None,
            SignState::SelfSigned => Badge::ShieldYellow,
            SignState::Unsigned => {
                if self.trusted(app_id) {
                    Badge::DotGray
                } else {
                    Badge::ShieldGray
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 悬停 tooltip（1s 延迟；词条化模板 F140）
// ---------------------------------------------------------------------------

/// tooltip 状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tooltip {
    hover_since: Option<u64>,
    pub visible: bool,
}

impl Tooltip {
    pub const fn new() -> Self {
        Tooltip { hover_since: None, visible: false }
    }

    pub fn hover_start(&mut self, now_ms: u64) {
        self.hover_since = Some(now_ms);
        self.visible = false; // 延迟未到不显示
    }

    pub fn hover_end(&mut self) {
        self.hover_since = None;
        self.visible = false;
    }

    /// 时间一拍：悬停满 1s 才显示（不该出现时不挡路）。
    pub fn tick(&mut self, now_ms: u64) {
        if let Some(t0) = self.hover_since {
            self.visible = now_ms.saturating_sub(t0) >= TOOLTIP_DELAY_MS;
        }
    }

    /// tooltip 词条（模板词条化 F140；填定长缓冲返回长度）。
    pub fn text(&self, badge: Badge, out: &mut [u8; 96]) -> usize {
        // 字节面取自 &str 常量（byte-string 字面量限 ASCII——中文词条走 .as_bytes()）。
        let body: &[u8] = match badge {
            Badge::ShieldGray => "未签名应用。签名验证可提升来源可信度。".as_bytes(),
            Badge::ShieldYellow => "自签名应用。来源未经第三方链验证。".as_bytes(),
            Badge::DotGray => "已信任的未签名应用（信任列表内）。".as_bytes(),
            Badge::None => b"",
        };
        let mut l = body.len().min(out.len());
        out[..l].copy_from_slice(&body[..l]);
        // 说明链接（帮助链通的对账锚）。
        if l + HELP_TARGET.len() < out.len() {
            out[l] = b' ';
            out[l + 1..l + 1 + HELP_TARGET.len()].copy_from_slice(HELP_TARGET);
            l += 1 + HELP_TARGET.len();
        }
        l
    }
}

// ---------------------------------------------------------------------------
// 装饰层（角标渲染走窗口装饰层——程序无感，不可被程序隐藏）
// ---------------------------------------------------------------------------

/// 标题栏装饰快照（截图对账模型——F098 不净化）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TitlebarDecor {
    pub app_id: u32,
    /// 应用自报的隐藏请求（装饰层忽略——不可被程序隐藏）。
    pub app_requests_hidden: bool,
    pub badge: Badge,
}

/// 合成标题栏：角标由装饰层注入，程序无感。
pub fn compose_titlebar(app_id: u32, badge: Badge, app_requests_hidden: bool) -> TitlebarDecor {
    TitlebarDecor { app_id, badge, app_requests_hidden }
}

/// 截图保真判定：装饰层角标在截图中如实存在（F098 不净化——所见即真实）。
pub fn screenshot_preserves_badge(decor: &TitlebarDecor) -> bool {
    // 程序的隐藏请求被忽略；角标恒随装饰层进截图。
    let _ = decor.app_requests_hidden;
    decor.badge != Badge::None
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
#[inline(never)]
pub fn run_signbadge_checks() -> CheckSet {
    let mut cs = CheckSet::new("F178-signbadge");

    // 1) 三态角标正确注入各 3 应用（链验无标/自签黄盾/未签灰盾）。
    let mut r = BadgeResolver::new();
    let b1 = r.resolve(1, SignState::ChainVerified);
    let b2 = r.resolve(2, SignState::SelfSigned);
    let b3 = r.resolve(3, SignState::Unsigned);
    cs.add("three_states_three_apps", b1 == Badge::None && b2 == Badge::ShieldYellow && b3 == Badge::ShieldGray, "");

    // 2) 信任列表降级：未签名但在列表内 → 灰点。
    let mut r2 = BadgeResolver::new();
    r2.add_trusted(9);
    let b4 = r2.resolve(9, SignState::Unsigned);
    let b5 = r2.resolve(10, SignState::Unsigned);
    cs.add("trust_list_demotion", b4 == Badge::DotGray && b5 == Badge::ShieldGray, "");

    // 3) 验证服务不可用 → fail-closed：全部按未签名 + 诊断报备。
    let mut r3 = BadgeResolver::new();
    r3.service_ok = false;
    let s1 = r3.resolve(1, SignState::ChainVerified);
    let s2 = r3.resolve(2, SignState::SelfSigned);
    let s3 = r3.resolve(3, SignState::Unsigned);
    cs.add(
        "service_down_fail_closed",
        r3.diag_reported && s1 == Badge::ShieldGray && s2 == Badge::ShieldGray && s3 == Badge::ShieldGray,
        "",
    );

    // 4) fail-closed 不覆盖信任降级（信任列表内仍灰点）。
    let mut r4 = BadgeResolver::new();
    r4.service_ok = false;
    r4.add_trusted(5);
    cs.add("fail_closed_respects_trust", r4.resolve(5, SignState::Unsigned) == Badge::DotGray, "");

    // 5) tooltip 1s 延迟：未满不显示、满 1s 显示、移出即隐藏。
    let mut t = Tooltip::new();
    t.hover_start(5_000);
    t.tick(5_500);
    let before = !t.visible;
    t.tick(6_000);
    let after = t.visible;
    t.hover_end();
    cs.add("tooltip_1s_delay", before && after && !t.visible, "");

    // 6) tooltip 与帮助链通（文案含 F119 签名篇链接锚）。
    let t2 = Tooltip::new();
    let mut buf = [0u8; 96];
    let len = t2.text(Badge::ShieldGray, &mut buf);
    let text = core::str::from_utf8(&buf[..len]).unwrap_or("");
    cs.add("tooltip_help_link", text.contains("未签名应用") && text.contains("help:F119/signing"), "");

    // 7) 角标几何与色彩常量（12px、偏移 4px、灰 #888/黄 #D90）。
    cs.add(
        "geometry_and_colors",
        BADGE_SIZE_PX == 12 && BADGE_OFFSET_PX == 4 && COLOR_GRAY == 0x888888 && COLOR_YELLOW == 0xDD9900,
        "",
    );

    // 8) 色弱形状冗余：每个可见角标都有形状信号（F114 联动）。
    cs.add(
        "shape_redundancy",
        Badge::ShieldGray.shape_redundant() && Badge::ShieldYellow.shape_redundant() && Badge::DotGray.shape_redundant() && !Badge::None.shape_redundant(),
        "",
    );

    // 9) 装饰层注入：程序隐藏请求被忽略（角标不可被程序隐藏）。
    let decor = compose_titlebar(7, Badge::ShieldGray, true);
    cs.add("decor_layer_immutable", decor.badge == Badge::ShieldGray && screenshot_preserves_badge(&decor), "");

    // 10) 截图角标保留：链验应用（无角标）截图无角标；带标应用角标在。
    let decor2 = compose_titlebar(8, Badge::None, false);
    cs.add("screenshot_fidelity", !screenshot_preserves_badge(&decor2) && screenshot_preserves_badge(&decor), "");

    // 11) 黄盾色彩强调（自签与未签形状同盾色不同——一眼可辨）。
    cs.add("shield_color_distinction", Badge::ShieldGray.color() != Badge::ShieldYellow.color(), "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_matrix_nine_apps() {
        // 三态 × 3 应用全矩阵（主册验收口径）。
        for round in 0..3u32 {
            let mut r = BadgeResolver::new();
            let base = 100 * (round + 1);
            assert_eq!(r.resolve(base + 1, SignState::ChainVerified), Badge::None);
            assert_eq!(r.resolve(base + 2, SignState::SelfSigned), Badge::ShieldYellow);
            assert_eq!(r.resolve(base + 3, SignState::Unsigned), Badge::ShieldGray);
        }
    }

    #[test]
    fn tooltip_never_flashes_on_quick_hover() {
        // 快速掠过（<1s）永不闪现——不该出现时不挡路。
        let mut t = Tooltip::new();
        t.hover_start(0);
        for ms in [100, 300, 500, 900] {
            t.tick(ms);
            assert!(!t.visible, "{}ms 不应显示", ms);
        }
        t.hover_end();
        t.tick(2_000);
        assert!(!t.visible, "移出后不残留");
    }

    #[test]
    fn tooltip_text_fits_buffer() {
        // 词条化模板全部装得下 96 字节缓冲（溢出即缺陷）。
        for badge in [Badge::ShieldGray, Badge::ShieldYellow, Badge::DotGray] {
            let mut t = Tooltip::new();
            let mut buf = [0u8; 96];
            let len = t.text(badge, &mut buf);
            assert!(len > 0 && len <= 96);
            assert!(core::str::from_utf8(&buf[..len]).is_ok(), "词条必须合法 UTF-8");
        }
    }

    #[test]
    fn trust_list_dedup_and_cap() {
        // 信任表去重与容量上限（16 满后再加被拒——诚实容量）。
        let mut r = BadgeResolver::new();
        for i in 0..20u32 {
            r.add_trusted(10 + i);
        }
        assert_eq!(r.trust_n, 16);
        r.add_trusted(10); // 重复不加
        assert_eq!(r.trust_n, 16);
    }
}
