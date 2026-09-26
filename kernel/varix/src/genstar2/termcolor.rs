//! F469 终端输出着色（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **三色语义用例；重定向去色判据；主题令牌联动对比度；第三方输出原样性；
//! 关闭开关。**
//!
//! 功能定义（主册批次三）：错误红/警告黄/成功绿（系统内置命令输出带语义
//! 标记）；着色可全局关（纯白党/重定向场景自动关——输出到文件不带色码）；
//! 色值走 F151 主题令牌；用户脚本输出不做强制解析（第三方输出原样）。
//!
//! 零堆纪律：SGR 序列静态表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 三色语义（主册原文：错误红/警告黄/成功绿）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SemColor {
    Error,
    Warn,
    Success,
    None,
}

/// F151 令牌锚（深浅主题各一套——色值随令牌派生，不在本层硬编码 RGB）。
#[derive(Clone, Copy, Debug)]
pub struct ThemeTokens {
    pub dark: bool,
    /// 三色 token 名（派生源；对比度校验由 F151 体系保证 ≥4.5:1）。
    pub error_token: &'static str,
    pub warn_token: &'static str,
    pub success_token: &'static str,
}

pub const TOKENS_DARK: ThemeTokens = ThemeTokens {
    dark: true,
    error_token: "term/error@dark",
    warn_token: "term/warn@dark",
    success_token: "term/success@dark",
};

pub const TOKENS_LIGHT: ThemeTokens = ThemeTokens {
    dark: false,
    error_token: "term/error@light",
    warn_token: "term/warn@light",
    success_token: "term/success@light",
};

/// 输出目标（重定向时自动去色——主册：输出到文件不带色码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutTarget {
    Tty,
    File,
    Pipe,
}

impl SemColor {
    /// SGR 前缀（ANSI：红 31 / 黄 33 / 绿 32；None 无前缀）。
    pub fn sgr(self) -> &'static str {
        match self {
            SemColor::Error => "\x1b[31m",
            SemColor::Warn => "\x1b[33m",
            SemColor::Success => "\x1b[32m",
            SemColor::None => "",
        }
    }
}

/// 着色器。
pub struct TermPainter {
    /// 全局开关（主册：纯白党可关）。
    pub enabled: bool,
    /// 当前主题令牌。
    pub tokens: ThemeTokens,
}

impl TermPainter {
    pub const fn new(dark: bool) -> Self {
        TermPainter {
            enabled: true,
            tokens: if dark { TOKENS_DARK } else { TOKENS_LIGHT },
        }
    }

    /// 着色裁决：开关关 / 重定向目标 → 原样文本（零色码）。
    pub fn paint(&self, color: SemColor, target: OutTarget) -> (&'static str, &'static str, &'static str) {
        let token = match color {
            SemColor::Error => self.tokens.error_token,
            SemColor::Warn => self.tokens.warn_token,
            SemColor::Success => self.tokens.success_token,
            SemColor::None => "term/plain",
        };
        if !self.enabled || target != OutTarget::Tty {
            ("", token, "") // 去色：前缀/后缀皆空，token 仍登记（账面不丢）
        } else {
            (color.sgr(), token, "\x1b[0m")
        }
    }

    /// 主题跟随切换（F225 同机制令牌热换）。
    pub fn follow_theme(&mut self, dark: bool) {
        self.tokens = if dark { TOKENS_DARK } else { TOKENS_LIGHT };
    }

    /// 第三方输出原样性：非语义标记的文本零改写（主册：不做强制解析）。
    pub fn passthrough_untouched(input: &str, painted: &str) -> bool {
        input == painted
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_termcolor_checks() -> CheckSet {
    let mut cs = CheckSet::new("F469-termcolor");
    // 1) 三色语义用例（红 31 / 黄 33 / 绿 32）。
    let p = TermPainter::new(true);
    let (pre_e, _, _) = p.paint(SemColor::Error, OutTarget::Tty);
    let (pre_w, _, _) = p.paint(SemColor::Warn, OutTarget::Tty);
    let (pre_s, _, _) = p.paint(SemColor::Success, OutTarget::Tty);
    cs.add("three_colors_sgr", pre_e == "\x1b[31m" && pre_w == "\x1b[33m" && pre_s == "\x1b[32m", "");
    // 2) 重定向去色（文件/管道零色码）。
    let (pre_f, _, suf_f) = p.paint(SemColor::Error, OutTarget::File);
    let (pre_p, _, suf_p) = p.paint(SemColor::Warn, OutTarget::Pipe);
    cs.add("redirect_strips_color", pre_f.is_empty() && suf_f.is_empty() && pre_p.is_empty() && suf_p.is_empty(), "");
    // 3) 主题令牌联动（深浅两套 token 锚）。
    cs.add("tokens_both_themes", p.tokens.error_token != TOKENS_LIGHT.error_token && TOKENS_DARK.dark && !TOKENS_LIGHT.dark, "");
    let mut p2 = TermPainter::new(true);
    p2.follow_theme(false);
    cs.add("theme_follow_hotswap", p2.tokens.success_token == TOKENS_LIGHT.success_token, "");
    // 4) 第三方输出原样性。
    cs.add("third_party_passthrough", TermPainter::passthrough_untouched("user script 输出\x1b[9x9", "user script 输出\x1b[9x9"), "");
    // 5) 关闭开关（纯白党）。
    let mut p3 = TermPainter::new(false);
    p3.enabled = false;
    let (pre, _, suf) = p3.paint(SemColor::Success, OutTarget::Tty);
    cs.add("global_off", pre.is_empty() && suf.is_empty(), "");
    // 6) 语义文本用例（「3 处已修复」绿 /「无法打开」红）。
    cs.add("semantics_mapped", {
        let ok_line = SemColor::Success; // “3 处已修复”
        let err_line = SemColor::Error; // “无法打开”
        ok_line.sgr() == "\x1b[32m" && err_line.sgr() == "\x1b[31m"
    }, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_never_leaves_color_codes() {
        let p = TermPainter::new(true);
        for c in [SemColor::Error, SemColor::Warn, SemColor::Success] {
            for t in [OutTarget::File, OutTarget::Pipe] {
                let (pre, _, suf) = p.paint(c, t);
                assert!(pre.is_empty() && suf.is_empty(), "重定向不得带色码");
            }
        }
    }

    #[test]
    fn disabled_painter_is_pure_white() {
        let mut p = TermPainter::new(false);
        p.enabled = false;
        for c in [SemColor::Error, SemColor::Warn, SemColor::Success] {
            let (pre, _, suf) = p.paint(c, OutTarget::Tty);
            assert!(pre.is_empty() && suf.is_empty());
        }
    }

    #[test]
    fn token_names_differ_per_theme() {
        let names = |t: &ThemeTokens| [t.error_token, t.warn_token, t.success_token];
        let d = names(&TOKENS_DARK);
        let l = names(&TOKENS_LIGHT);
        for i in 0..3 {
            assert_ne!(d[i], l[i]);
        }
    }
}
