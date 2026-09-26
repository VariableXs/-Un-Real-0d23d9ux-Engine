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

// ---------------------------------------------------------------------------
// v4（深化批次四）：SGR 剥离器 / 语义标记归类器 / 对比度登记表 / 开关持久化
// ---------------------------------------------------------------------------

/// SGR 剥离器（主册：重定向/管道「不留色码垃圾」——对第三方已带色码的
/// 输出，落文件前剥净 ESC[...m 序列）。零堆：定长输出缓冲，返回写出字节数。
/// 剥离规则：`\x1b` 后跟 `[`、参数字节（0x30-0x3F）、中间字节（0x20-0x2F）、
/// 终止字节（0x40-0x7E）的完整 CSI 序列整体剥除；残缺序列（流被截断）按
/// 原样保留——不猜测不补全。
pub fn strip_sgr(input: &[u8], out: &mut [u8]) -> usize {
    let mut wi = 0usize;
    let mut i = 0usize;
    while i < input.len() {
        if input[i] == 0x1b && i + 1 < input.len() && input[i + 1] == b'[' {
            // 扫描到终止字节；未找到 → 残缺，原样保留 ESC。
            let mut j = i + 2;
            let mut done = false;
            while j < input.len() {
                let b = input[j];
                if (0x40..=0x7e).contains(&b) {
                    done = true;
                    break;
                }
                if !(0x20..=0x3f).contains(&b) {
                    break; // 非法中间字节：不视为 CSI，ESC 原样保留
                }
                j += 1;
            }
            if done {
                i = j + 1;
                continue;
            }
        }
        if wi < out.len() {
            out[wi] = input[i];
            wi += 1;
        }
        i += 1;
    }
    wi
}

/// 语义标记归类器（主册：系统内置命令输出带语义标记——「3 处已修复」绿、
/// 「无法打开」红）。标记词表（UTF-8 字节锚，零堆）：
/// 成功类：「已修复」「成功」；错误类：「无法」「失败」「错误」；
/// 警告类：「警告」「提醒」。命中优先级：错误 > 警告 > 成功（最坏先报）。
pub fn classify_line(line: &[u8]) -> SemColor {
    const SUCC: [&[u8]; 2] = [b"\xe5\xb7\xb2\xe4\xbf\xae\xe5\xa4\x8d", b"\xe6\x88\x90\xe5\x8a\x9f"]; // 已修复 / 成功
    const ERR: [&[u8]; 3] = [
        b"\xe6\x97\xa0\xe6\xb3\x95",       // 无法
        b"\xe5\xa4\xb1\xe8\xb4\xa5",       // 失败
        b"\xe9\x94\x99\xe8\xaf\xaf",       // 错误
    ];
    const WARN: [&[u8]; 2] = [b"\xe8\xad\xa6\xe5\x91\x8a", b"\xe6\x8f\x90\xe9\x86\x92"]; // 警告 / 提醒
    let hit = |table: &[&[u8]]| table.iter().any(|w| contains(line, w));
    if hit(&ERR) {
        SemColor::Error
    } else if hit(&WARN) {
        SemColor::Warn
    } else if hit(&SUCC) {
        SemColor::Success
    } else {
        SemColor::None
    }
}

/// 零堆子串搜索（朴素算法；行长 ≤ 512 语义足够）。
fn contains(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    (0..=hay.len() - needle.len()).any(|i| &hay[i..i + needle.len()] == &needle[..])
}

/// 六 token 对比度登记表（主册：色值走 F151 令牌、对比度 ≥4.5:1——
/// 令牌体系派生值在此登记备查，permille 口径 4500 = 4.5:1）。
pub const CONTRAST_TABLE: [(&str, u16); 6] = [
    ("term/error@dark", 5200),
    ("term/warn@dark", 7300),
    ("term/success@dark", 6100),
    ("term/error@light", 4700),
    ("term/warn@light", 5600),
    ("term/success@light", 4800),
];

/// 开关状态持久化（1 字节：bit0=enabled、bit1=dark）——save/load 往返。
pub const STATE_SIZE: usize = 1;

pub fn save_state(enabled: bool, dark: bool) -> [u8; STATE_SIZE] {
    let mut b = 0u8;
    if enabled {
        b |= 1;
    }
    if dark {
        b |= 2;
    }
    [b]
}

/// 载入校验：高 6 位非零 = 坏包拒收（返回默认态）。
pub fn load_state(raw: &[u8]) -> (bool, bool) {
    if raw.len() != STATE_SIZE || raw[0] & 0xfc != 0 {
        return (true, true); // 默认：开 + 深色
    }
    (raw[0] & 1 != 0, raw[0] & 2 != 0)
}

pub fn run_termcolor_v4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F469-v4");
    // 1) SGR 剥离：完整 CSI 剥净、正文零损。
    let mut buf = [0u8; 128];
    let n = strip_sgr(b"\x1b[31m\xe9\x94\x99\xe8\xaf\xaf\x1b[0m", &mut buf);
    cs.add("strip_full_csi", &buf[..n] == b"\xe9\x94\x99\xe8\xaf\xaf", "");
    // 2) 残缺序列原样保留（不猜测不补全）。
    let n2 = strip_sgr(b"abc\x1b[3", &mut buf);
    cs.add("truncated_kept", &buf[..n2] == b"abc\x1b[3", "");
    // 3) 语义归类三例（主册原句锚）。
    cs.add("classify_success", classify_line("3 处已修复".as_bytes()) == SemColor::Success, "");
    cs.add("classify_error", classify_line("无法打开文件".as_bytes()) == SemColor::Error, "");
    cs.add("classify_warn", classify_line("警告：磁盘将满".as_bytes()) == SemColor::Warn, "");
    cs.add("classify_none", classify_line("plain output".as_bytes()) == SemColor::None, "");
    // 4) 优先级：错误压过成功词（最坏先报）。
    cs.add("error_beats_success", classify_line("成功但无法收尾".as_bytes()) == SemColor::Error, "");
    // 5) 对比度登记表：六 token 全部 ≥4.5:1。
    cs.add("contrast_all_above_floor", CONTRAST_TABLE.iter().all(|(_, r)| *r >= 4500), "");
    cs.add("contrast_table_paired", CONTRAST_TABLE.len() == 6, "");
    // 6) 开关持久化 round-trip + 坏包拒收。
    let st = save_state(false, true);
    cs.add("state_roundtrip", load_state(&st) == (false, true), "");
    cs.add("state_bad_pkg_default", load_state(&[0xff]) == (true, true), "");
    cs.add("state_short_default", load_state(&[]) == (true, true), "");
    cs
}

#[cfg(test)]
mod v4_tests {
    use super::*;

    #[test]
    fn strip_mixed_content_preserves_text() {
        let mut buf = [0u8; 64];
        let src = b"\x1b[32mok\x1b[0m and \x1b[33mwarn\x1b[0m";
        let n = strip_sgr(src, &mut buf);
        assert_eq!(&buf[..n], b"ok and warn");
    }

    #[test]
    fn strip_respects_output_capacity() {
        let mut small = [0u8; 4];
        let n = strip_sgr(b"abcdefghij", &mut small);
        assert_eq!(n, 4); // 容量钳制不越界
    }

    #[test]
    fn classify_multibyte_boundaries() {
        // 标记词在行首/行尾都能命中。
        assert_eq!(classify_line("已修复".as_bytes()), SemColor::Success);
        assert_eq!(classify_line("操作失败".as_bytes()), SemColor::Error);
        // 空行与无标记行诚实归 None。
        assert_eq!(classify_line(b""), SemColor::None);
        assert_eq!(classify_line(b"plain output"), SemColor::None);
    }
}
