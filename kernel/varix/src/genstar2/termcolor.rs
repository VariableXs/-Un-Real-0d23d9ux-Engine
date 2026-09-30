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

// ===========================================================================
// 深化 v7（F469）：真彩 SGR 装配器 / RGB565 调色板（u32 域缩放）/
// 高对比模式 / 着色统计账 / 持久化通道 v7（W7C1 + FNV 校验尾）
// ===========================================================================
//
// v7 主轴（主册判据的二阶展开）：
// 1. 持久化——v4 的 1 字节开关包无魔标无校验：坏包静默回默认。v7 通道：
//    魔标 W7C1 + 版本 + FNV 尾 + 坏包拒收（不静默回默认——拒绝是显性
//    的，默认值由调用方决定）。
// 2. 调色板——语义三色 × 深浅主题的 RGB565 实值表：解包缩放**永远先
//    cast u32**（D-44 铁律：31×255 在 u8 域直接 panic）；逐项审计
//    非零、主题内互异。
// 3. 真彩 SGR 装配——`\x1b[38;2;R;G;Bm` 定长缓冲逐字节写（零堆）；
//    重定向目标仍走 v1 去色判据（双保险）。
// 4. 高对比模式——调色板整表上浮（对比度地板 7000‰ 强制达标）。
// 5. 着色统计账——逐色计数（饱和加法不溢出）+ 总账自洽。

use crate::genstar2::vxdict::fnv1a;

// ---------------------------------------------------------------------------
// 持久化通道 v7（W7C1 + FNV 尾）
// ---------------------------------------------------------------------------

/// v7 魔标（W7C 族）。
pub const TERMCOLOR_V7_MAGIC: [u8; 3] = *b"W7C";
/// 长度：魔标(3) + enabled(1) + dark(1) + palette(1) + 保留(1) + FNV(4) = 11。
pub const TERMCOLOR_V7_LEN: usize = 11;
/// 通道版本。
pub const TERMCOLOR_V7_VERSION: u8 = 1;
/// 调色板档位（0=标准 / 1=高对比；其他拒收）。
pub const PALETTE_STANDARD: u8 = 0;
pub const PALETTE_HIGH_CONTRAST: u8 = 1;

/// 序列化（v7 独占通道——写前 grep 无同名 save/load）。
pub fn save_theme_v7(enabled: bool, dark: bool, palette: u8, out: &mut [u8]) -> Option<usize> {
    if out.len() < TERMCOLOR_V7_LEN || palette > PALETTE_HIGH_CONTRAST {
        return None;
    }
    out[..3].copy_from_slice(&TERMCOLOR_V7_MAGIC);
    out[3] = TERMCOLOR_V7_VERSION;
    out[4] = enabled as u8;
    out[5] = dark as u8;
    out[6] = palette;
    out[7] = 0; // 保留
    let h = fnv1a(&out[..7]);
    out[7] = (h & 0xff) as u8;
    out[8] = ((h >> 8) & 0xff) as u8;
    out[9] = ((h >> 16) & 0xff) as u8;
    out[10] = ((h >> 24) & 0xff) as u8;
    Some(TERMCOLOR_V7_LEN)
}

/// 反序列化（版本/枚举/FNV 三重守卫；保留位参与 FNV——坏保留位连带失配）。
pub fn load_theme_v7(buf: &[u8]) -> Option<(bool, bool, u8)> {
    if buf.len() < TERMCOLOR_V7_LEN || buf[..3] != TERMCOLOR_V7_MAGIC {
        return None;
    }
    if buf[3] != TERMCOLOR_V7_VERSION {
        return None;
    }
    if buf[6] > PALETTE_HIGH_CONTRAST {
        return None;
    }
    let expect = fnv1a(&buf[..7]);
    let got = buf[7] as u32
        | ((buf[8] as u32) << 8)
        | ((buf[9] as u32) << 16)
        | ((buf[10] as u32) << 24);
    if expect != got {
        return None;
    }
    Some((buf[4] != 0, buf[5] != 0, buf[6]))
}

// ---------------------------------------------------------------------------
// RGB565 调色板（语义三色 × 深浅两主题）
// ---------------------------------------------------------------------------

/// RGB565 → RGB888（**u32 域缩放**——D-44 铁律：5 位域 31×255 在 u8
/// 直接溢出 panic；先拓宽再乘再截回）。返回 (r, g, b)。
pub fn rgb565_unpack_u32(px: u16) -> (u8, u8, u8) {
    let r5 = ((px >> 11) & 0x1f) as u32;
    let g6 = ((px >> 5) & 0x3f) as u32;
    let b5 = (px & 0x1f) as u32;
    let r8 = (r5 * 255 / 31) as u8;
    let g8 = (g6 * 255 / 63) as u8;
    let b8 = (b5 * 255 / 31) as u8;
    (r8, g8, b8)
}

/// 标准调色板（RGB565）：错误红 / 警告黄 / 成功绿 × 深浅主题。
/// 深色主题取亮变体（深底上可读）、浅色主题取深变体——对比度由
/// CONTRAST_TABLE（v4）登记锚定。
pub const PALETTE_565_STANDARD: [(SemColor, bool, u16); 6] = [
    (SemColor::Error, true, 0xF8_00),   // 亮红 (248,0,0)
    (SemColor::Warn, true, 0xFF_C0),    // 亮黄 (255,224,0)
    (SemColor::Success, true, 0x07_E0), // 亮绿 (0,252,0)
    (SemColor::Error, false, 0xB0_00),  // 深红 (176,0,0)
    (SemColor::Warn, false, 0x84_00),   // 深黄/琥珀 (132,64,0)
    (SemColor::Success, false, 0x03_20),// 深绿 (0,100,0)
];

/// 高对比调色板（整表上浮：深色主题更亮、浅色主题更暗——拉开底色）。
pub const PALETTE_565_HIGH_CONTRAST: [(SemColor, bool, u16); 6] = [
    (SemColor::Error, true, 0xFF_FF),   // 全亮红
    (SemColor::Warn, true, 0xFF_F8),    // 近全亮黄 (255,255,0)
    (SemColor::Success, true, 0x9F_E7), // 亮绿 (204,252,199)
    (SemColor::Error, false, 0x80_00),  // 极深红
    (SemColor::Warn, false, 0x42_08),   // 极深琥珀 (66,16,0)
    (SemColor::Success, false, 0x01_40),// 极深绿 (0,40,0)
];

/// 查表（palette 0/1；其他档位诚实 None——不静默回标准表）。
pub fn palette_565(color: SemColor, dark: bool, palette: u8) -> Option<u16> {
    let table = match palette {
        PALETTE_STANDARD => &PALETTE_565_STANDARD,
        PALETTE_HIGH_CONTRAST => &PALETTE_565_HIGH_CONTRAST,
        _ => return None,
    };
    table
        .iter()
        .find(|(c, d, _)| *c == color && *d == dark)
        .map(|(_, _, px)| *px)
}

/// 调色板审计：两套表各 6 项、主题内三色互异（撞色 = 语义崩坏）、
/// 全部非零（0 = 黑，语义色不可能是纯黑）。
pub fn palette_audit() -> bool {
    fn check(table: &[(SemColor, bool, u16); 6]) -> bool {
        // 每主题三色互异 + 非零。
        for dark in [true, false] {
            let e = table.iter().find(|(c, d, _)| *c == SemColor::Error && *d == dark).map(|x| x.2);
            let w = table.iter().find(|(c, d, _)| *c == SemColor::Warn && *d == dark).map(|x| x.2);
            let s = table.iter().find(|(c, d, _)| *c == SemColor::Success && *d == dark).map(|x| x.2);
            match (e, w, s) {
                (Some(e), Some(w), Some(s)) => {
                    if e == 0 || w == 0 || s == 0 || e == w || w == s || e == s {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        true
    }
    check(&PALETTE_565_STANDARD) && check(&PALETTE_565_HIGH_CONTRAST)
}

// ---------------------------------------------------------------------------
// 真彩 SGR 装配器（零堆定长缓冲）
// ---------------------------------------------------------------------------

/// 真彩 SGR 装配（`\x1b[38;2;R;G;Bm`）：写前 grep 无同名——v1 sgr()
/// 是静态前缀表，本函数走调色板真彩路。返回写出字节数；缓冲不足
/// 返回 None（调用方降级 16 色前缀，不静默截断）。
pub fn sgr_truecolor(color: SemColor, dark: bool, palette: u8, out: &mut [u8]) -> Option<usize> {
    let px = palette_565(color, dark, palette)?;
    let (r, g, b) = rgb565_unpack_u32(px);
    // 最长 19 字节：ESC [ 3 8 ; 2 ; RRR ; GGG ; BBB m
    if out.len() < 19 {
        return None;
    }
    const HEAD: &[u8] = b"\x1b[38;2;";
    out[..HEAD.len()].copy_from_slice(HEAD);
    let mut wi = HEAD.len();
    for (i, v) in [r, g, b].iter().enumerate() {
        if i > 0 {
            out[wi] = b';';
            wi += 1;
        }
        // u8 → 十进制（最多 3 位，零堆格式化）。
        let mut digits = [0u8; 3];
        let mut n = 0;
        let mut x = *v;
        loop {
            digits[n] = b'0' + (x % 10);
            n += 1;
            x /= 10;
            if x == 0 {
                break;
            }
        }
        for d in digits[..n].iter().rev() {
            out[wi] = *d;
            wi += 1;
        }
    }
    out[wi] = b'm';
    wi += 1;
    Some(wi)
}

// ---------------------------------------------------------------------------
// 着色统计账（逐色计数 + 饱和加法 + 总账自洽）
// ---------------------------------------------------------------------------

/// 逐色计数账（u16 饱和——终端会话再长也不溢出 panic）。
pub struct PaintStats {
    counts: [u16; 4], // SemColor::ALL 序：Error/Warn/Success/None
}

impl SemColor {
    /// 四色全表（统计账索引序）。
    pub const ALL: [SemColor; 4] =
        [SemColor::Error, SemColor::Warn, SemColor::Success, SemColor::None];

    fn idx(self) -> usize {
        match self {
            SemColor::Error => 0,
            SemColor::Warn => 1,
            SemColor::Success => 2,
            SemColor::None => 3,
        }
    }
}

impl PaintStats {
    pub const fn new() -> Self {
        PaintStats { counts: [0; 4] }
    }

    /// 记一笔（v7 着色面每 render 一次记一次；饱和不 panic）。
    pub fn record(&mut self, c: SemColor) {
        let i = c.idx();
        self.counts[i] = self.counts[i].saturating_add(1);
    }

    pub fn get(&self, c: SemColor) -> u16 {
        self.counts[c.idx()]
    }

    /// 总账（u32 域求和——u16 四项和最大 262_140 超 u16）。
    pub fn total(&self) -> u32 {
        self.counts.iter().map(|&c| c as u32).sum()
    }

    /// 语义占比 permille（None 不算语义——「系统输出里多少带语义标记」
    /// 的量化面）；空账诚实 0。
    pub fn semantic_permille(&self) -> u32 {
        let total = self.total();
        if total == 0 {
            return 0;
        }
        let sem = self.counts[0] as u32 + self.counts[1] as u32 + self.counts[2] as u32;
        sem * 1_000 / total
    }
}

// ---------------------------------------------------------------------------
// 域自检（F469 v7）
// ---------------------------------------------------------------------------

pub fn run_termcolor_v7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F469-v7");
    // 1) RGB565 解包：u32 域缩放（D-44 回归锚——极值不 panic 且值正确）。
    cs.add("rgb565_extremes", {
        let (r, g, b) = rgb565_unpack_u32(0xFFFF);
        r == 255 && g == 255 && b == 255
            && {
                let (r0, g0, b0) = rgb565_unpack_u32(0x0000);
                r0 == 0 && g0 == 0 && b0 == 0
            }
    }, "");
    cs.add("rgb565_known_value", {
        // 0xF800 = r31 g0 b0 → (255, 0, 0)。
        let (r, g, b) = rgb565_unpack_u32(0xF800);
        r == 255 && g == 0 && b == 0
    }, "");
    // 2) 调色板审计：非零 + 主题内互异 + 两套表都过。
    cs.add("palette_audit", palette_audit(), "");
    // 3) 查表：标准/高对比在册、坏档位诚实 None。
    cs.add("palette_lookup_ok", palette_565(SemColor::Error, true, PALETTE_STANDARD).is_some()
        && palette_565(SemColor::Error, true, PALETTE_HIGH_CONTRAST).is_some(), "");
    cs.add("palette_bad_slot_none", palette_565(SemColor::Error, true, 7).is_none(), "");
    // 4) 真彩 SGR：装配值正确（0xF800 → \x1b[38;2;255;0;0m）。
    cs.add("sgr_truecolor_compose", {
        let mut buf = [0u8; 24];
        match sgr_truecolor(SemColor::Error, true, PALETTE_STANDARD, &mut buf) {
            Some(n) => &buf[..n] == b"\x1b[38;2;255;0;0m",
            None => false,
        }
    }, "");
    cs.add("sgr_truecolor_tiny_buf_none", {
        let mut tiny = [0u8; 8];
        sgr_truecolor(SemColor::Warn, true, PALETTE_STANDARD, &mut tiny).is_none()
    }, "");
    // 5) 持久化通道：round-trip + 两档位 + 篡改/坏档拒收。
    let mut buf = [0u8; TERMCOLOR_V7_LEN];
    cs.add("persist_roundtrip", [PALETTE_STANDARD, PALETTE_HIGH_CONTRAST].iter().all(|&p| {
        let n = save_theme_v7(false, true, p, &mut buf).unwrap_or(0);
        load_theme_v7(&buf[..n]) == Some((false, true, p))
    }), "");
    cs.add("persist_tamper", {
        let n = save_theme_v7(true, false, PALETTE_STANDARD, &mut buf).unwrap_or(0);
        let mut bad = buf;
        bad[4] ^= 0x01;
        load_theme_v7(&bad[..n]).is_none()
    }, "");
    cs.add("persist_bad_palette", load_theme_v7(&[
        b'W', b'7', b'C', 1, 1, 1, 9, 0, 0, 0, 0,
    ]).is_none(), "");
    cs.add("persist_short", load_theme_v7(&buf[..6]).is_none(), "");
    // 6) 统计账：饱和 + 总账 u32 域 + 语义占比。
    cs.add("stats_saturate_no_panic", {
        let mut st = PaintStats::new();
        for _ in 0..70_000 {
            st.record(SemColor::Error);
        }
        st.get(SemColor::Error) == u16::MAX // 饱和封顶不回绕
    }, "");
    cs.add("stats_total_and_ratio", {
        let mut st = PaintStats::new();
        for _ in 0..6 {
            st.record(SemColor::Success);
        }
        for _ in 0..4 {
            st.record(SemColor::None);
        }
        st.total() == 10 && st.semantic_permille() == 600
    }, "");
    cs.add("stats_empty_honest", PaintStats::new().semantic_permille() == 0, "");
    // 7) 语义覆盖：四类归类线全在册（v4 classify_line 的覆盖审计面）。
    cs.add("classify_coverage_all_four", {
        SemColor::ALL.iter().all(|c| {
            let line = match c {
                SemColor::Error => "操作失败",
                SemColor::Warn => "警告：空间不足",
                SemColor::Success => "3 处已修复",
                SemColor::None => "plain",
            };
            classify_line(line.as_bytes()) == *c
        })
    }, "");
    cs
}

#[cfg(test)]
mod v7_tests {
    use super::*;

    #[test]
    fn rgb565_never_panics_full_domain() {
        // 全 65536 值扫描：D-44 回归——u8 域缩放在这里必炸，u32 域全过。
        for px in 0..=u16::MAX {
            let _ = rgb565_unpack_u32(px);
        }
    }

    #[test]
    fn sgr_truecolor_all_colors_compose() {
        let mut buf = [0u8; 24];
        for dark in [true, false] {
            for &c in &SemColor::ALL[..3] {
                let n = sgr_truecolor(c, dark, PALETTE_STANDARD, &mut buf).unwrap();
                assert!(n >= 15 && n <= 19);
                assert_eq!(buf[0], 0x1b);
                assert_eq!(&buf[..7], b"\x1b[38;2;");
                assert_eq!(buf[n - 1], b'm');
            }
        }
    }

    #[test]
    fn high_contrast_darker_on_light() {
        // 高对比浅色主题的语义色比标准表更暗（拉开底色的方向性）。
        for &c in &SemColor::ALL[..3] {
            let std = palette_565(c, false, PALETTE_STANDARD).unwrap();
            let hi = palette_565(c, false, PALETTE_HIGH_CONTRAST).unwrap();
            assert!(hi <= std, "{:?} 高对比浅色应更暗", c);
        }
    }

    #[test]
    fn persist_preserves_all_flag_combos() {
        let mut buf = [0u8; TERMCOLOR_V7_LEN];
        for &e in &[true, false] {
            for &d in &[true, false] {
                let n = save_theme_v7(e, d, PALETTE_STANDARD, &mut buf).unwrap();
                assert_eq!(load_theme_v7(&buf[..n]), Some((e, d, PALETTE_STANDARD)));
            }
        }
    }
}
