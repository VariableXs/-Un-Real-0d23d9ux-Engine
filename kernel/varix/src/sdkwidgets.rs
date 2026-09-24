//! 控件基类宪章默认与主题 token（WP-303 · B-1103 焦点环/三态/动画词典全
//! 默认 + B-1104 硬编码色值零例）。
//!
//! MD2 篇 11.3：vx crate UI 层的每件控件**默认行为内嵌宪章要求**——焦点环
//! 默认渲染（B-907）、三态齐备（常态/悬停/禁用，输入框加错误态与占位符）、
//! 按压反馈（缩进一像素与色阶变化）、动画时长取词典值（进场 120ms、退场
//! 80ms，缓动曲线统一）。主题 token（颜色/字号/间距命名集合）由系统下发，
//! 应用只能引用 token 不能硬编码色值——**合规不靠开发者自觉，靠默认**。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 动画词典（时长与缓动——全系统一致，控件不许自带私货）
// ---------------------------------------------------------------------------

/// 动画词典值（MD2 11.3：进场 120ms、退场 80ms）。
pub const ANIM_ENTER_MS: u32 = 120;
pub const ANIM_EXIT_MS: u32 = 80;

/// 统一缓动曲线标识（词典只发一个值——控件私设缓动即违规）。
pub const EASING_CURVE: &str = "standard";

// ---------------------------------------------------------------------------
// 控件基类（宪章要求是默认值，不是开发者可选项）
// ---------------------------------------------------------------------------

/// 控件三态（穷举——输入框另有错误态与占位符，见 TextFieldExtras）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WidgetState {
    Normal,
    Hover,
    Disabled,
}

/// 控件基类——宪章默认内嵌：焦点环默认开、三态齐备、按压反馈、动画取词典。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WidgetBase {
    /// 焦点环默认渲染（B-907 与 focring 同源纪律：默认开且无关闭构造）。
    pub focus_ring_on: bool,
    /// 三态样式表在册（下标 = state_index 序：常态/悬停/禁用，全 true 才算齐）。
    pub state_styles: [bool; 3],
    /// 按压反馈两要素（缩进一像素 + 色阶变化）。
    pub press_indent_px: u8,
    pub press_shade_shift: bool,
}

/// 三态到样式表下标的映射（0=常态 1=悬停 2=禁用——穷举映射，表外无态）。
pub fn state_index(s: WidgetState) -> usize {
    match s {
        WidgetState::Normal => 0,
        WidgetState::Hover => 1,
        WidgetState::Disabled => 2,
    }
}

impl WidgetBase {
    /// 基类默认构造（宪章要求在这里是**默认值**——控件作者不写也对）。
    pub const fn charter_default() -> WidgetBase {
        WidgetBase {
            focus_ring_on: true,
            state_styles: [true, true, true],
            press_indent_px: 1,
            press_shade_shift: true,
        }
    }

    /// 宪章合规判：焦点环开 + 三态齐 + 按压两要素。
    pub fn charter_compliant(&self) -> bool {
        self.focus_ring_on && self.state_styles == [true, true, true]
            && self.press_indent_px >= 1 && self.press_shade_shift
    }
}

/// 输入框附加义务（错误态 + 占位符——MD2 11.3 明文）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TextFieldExtras {
    pub error_state: bool,
    pub placeholder: bool,
}

/// 控件族注册审计（按钮/输入框/列表/对话框/标签页/菜单——交互词典的
/// 控件清单；新控件类型过 ADR，清单外不进基类族）。
pub const WIDGET_FAMILY: [&str; 6] = ["button", "input", "list", "dialog", "tabs", "menu"];

/// 控件族在册（清单外控件名不认——新类型走 ADR 而不是绕过基类）。
pub fn family_member(name: &str) -> bool {
    let mut i = 0;
    while i < WIDGET_FAMILY.len() {
        if WIDGET_FAMILY[i] == name {
            return true;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 主题 token（应用只能引用，不能硬编码——B-1104 审计脚本零例）
// ---------------------------------------------------------------------------

/// 主题 token（命名集合：颜色/字号/间距——由系统下发）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ThemeToken {
    pub name: &'static str,
    /// 色值（RGB 打包 u32——值住在系统里，应用只拿名字）。
    pub rgb: u32,
}

/// 系统下发的基础 token 集（语义名——应用引用名字，写死 0xRRGGBB 即违规）。
pub const TOKENS: [ThemeToken; 6] = [
    ThemeToken { name: "bg.primary", rgb: 0x1B1B1F },
    ThemeToken { name: "fg.primary", rgb: 0xE3E1E5 },
    ThemeToken { name: "accent", rgb: 0x7C9CFF },
    ThemeToken { name: "bg.surface", rgb: 0x24242A },
    ThemeToken { name: "fg.muted", rgb: 0x9A9AA5 },
    ThemeToken { name: "danger", rgb: 0xE5534B },
];

/// token 引用合法：名字在系统下发集里（引用不存在的 token 就是硬编码的马甲）。
pub fn token_exists(name: &str) -> bool {
    let mut i = 0;
    while i < TOKENS.len() {
        if TOKENS[i].name == name {
            return true;
        }
        i += 1;
    }
    false
}

/// 界面样式引用（应用侧只能落 token 名——rgb 字段不存在于此结构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StyleRef {
    pub token: &'static str,
}

/// **B-1104 审计脚本**：扫一批样式引用，全部命中系统下发集 = 硬编码色值
/// 零例（引用面没有 rgb 字段是结构面——审计是运行面的双保险）。
pub fn audit_no_hardcoded(refs: &[StyleRef]) -> bool {
    let mut i = 0;
    while i < refs.len() {
        if !token_exists(refs[i].token) {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// CheckSet（B-1103 · 3 项 + B-1104 · 2 项）
// ---------------------------------------------------------------------------

pub fn run_sdkwidgets_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1103/1104 控件宪章默认与主题 token");
    // 1. 宪章默认内嵌：基类默认构造直接合规（开发者不写也对）。
    let dflt = WidgetBase::charter_default();
    set.add(
        "B-1103 宪章默认内嵌",
        dflt.charter_compliant(),
        "焦点环默认渲染+三态齐备+按压反馈两要素——合规是默认值不是自觉",
    );
    // 2. 篡改基类默认立即不合规（宪章不是摆设）。
    let mut broken = WidgetBase::charter_default();
    broken.focus_ring_on = false;
    let mut half = WidgetBase::charter_default();
    half.state_styles = [true, true, false];
    set.add(
        "B-1103 篡改即红",
        !broken.charter_compliant() && !half.charter_compliant(),
        "关焦点环或缺任一态即不合规——宪章条款变成可判定的布尔面",
    );
    // 3. 控件族清单与动画词典（六件在册 + 120/80/统一缓动）。
    let mut all_family = true;
    let mut i = 0;
    while i < WIDGET_FAMILY.len() {
        if !family_member(WIDGET_FAMILY[i]) {
            all_family = false;
        }
        i += 1;
    }
    set.add(
        "B-1103 控件族与动画词典",
        all_family && !family_member("custom123") && ANIM_ENTER_MS == 120 && ANIM_EXIT_MS == 80 && EASING_CURVE == "standard"
            && state_index(WidgetState::Normal) == 0 && state_index(WidgetState::Hover) == 1 && state_index(WidgetState::Disabled) == 2,
        "按钮/输入框/列表/对话框/标签页/菜单在册——新控件走 ADR，动画取词典值，三态下标穷举",
    );
    // 4. token 系统下发集：引用名必须命中（不存在即硬编码马甲）。
    set.add(
        "B-1104 token 下发集",
        token_exists("accent") && !token_exists("my.cool.color"),
        "颜色/字号/间距住系统里——应用侧只拿名字，值不过手",
    );
    // 5. 硬编码色值零例审计（B-1104 达标线）：引用面无 rgb 字段 + 审计脚本双保险。
    let good_refs = [StyleRef { token: "bg.primary" }, StyleRef { token: "danger" }];
    let bad_refs = [StyleRef { token: "bg.primary" }, StyleRef { token: "0xRRGGBB" }];
    set.add(
        "B-1104 硬编码零例",
        audit_no_hardcoded(&good_refs) && !audit_no_hardcoded(&bad_refs),
        "样式引用逐个命中 token 集——深浅主题切换全系统一致（判例 15/WD-061 的 SDK 层保障）",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe08 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe08_charter_default_compliant() {
        let d = WidgetBase::charter_default();
        assert!(d.focus_ring_on);
        assert_eq!(d.state_styles, [true, true, true]);
        assert_eq!(d.press_indent_px, 1);
        assert!(d.press_shade_shift);
        assert!(d.charter_compliant());
    }

    #[test]
    fn fe08_tamper_any_pillar_fails() {
        // 四根支柱（焦点环/三态/缩进/色阶）动任意一根都不合规。
        let mut a = WidgetBase::charter_default();
        a.focus_ring_on = false;
        assert!(!a.charter_compliant());
        let mut b = WidgetBase::charter_default();
        b.state_styles[1] = false;
        assert!(!b.charter_compliant());
        let mut c = WidgetBase::charter_default();
        c.press_indent_px = 0;
        assert!(!c.charter_compliant());
        let mut e = WidgetBase::charter_default();
        e.press_shade_shift = false;
        assert!(!e.charter_compliant());
    }

    #[test]
    fn fe08_widget_family_closed_list() {
        // 六件在册、清单外拒认——新控件类型过 ADR 不走后门。
        assert!(family_member("button") && family_member("menu"));
        assert!(!family_member("buttonx"));
        assert!(!family_member(""));
        assert_eq!(WIDGET_FAMILY.len(), 6);
    }

    #[test]
    fn fe08_theme_token_audit() {
        // 动画词典冻结值。
        assert_eq!(ANIM_ENTER_MS, 120);
        assert_eq!(ANIM_EXIT_MS, 80);
        // token 集无重复名（名字是唯一钥匙，重复名就是两把钥匙开同一扇门）。
        let mut i = 0;
        while i < TOKENS.len() {
            let mut j = i + 1;
            while j < TOKENS.len() {
                assert_ne!(TOKENS[i].name, TOKENS[j].name);
                j += 1;
            }
            i += 1;
        }
        // 审计：全命中绿、一票未命中红。
        let refs = [StyleRef { token: "fg.muted" }, StyleRef { token: "accent" }];
        assert!(audit_no_hardcoded(&refs));
        let bad = [StyleRef { token: "fg.muted" }, StyleRef { token: "brand.pink" }];
        assert!(!audit_no_hardcoded(&bad));
    }
}
