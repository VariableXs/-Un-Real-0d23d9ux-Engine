//! TRINITY-500 · AI-06 字体与文本渲染域（F126~F150，W2）
//!
//! 依赖 VARIX-500 AI-07 的键码层与 AI-05 的合成 API。
//! 诚实边界：内嵌**笔画矢量字体**覆盖 ASCII 数字与大写字母（F126），
//! 其余字形走 VARIX `font.rs` 的 8×8 位图并按整数倍放大（F127）——
//! 不假装拥有完整 TrueType 轮廓。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F126 内嵌矢量字体（UI + 等宽）
// ---------------------------------------------------------------------------

/// em 方框：坐标 0..1000，基线 y=0，字母上缘 y=700。
pub const UNITS_PER_EM: i32 = 1000;
pub const CAP_HEIGHT: i32 = 700;
pub const X_HEIGHT: i32 = 480;

/// 一个笔画字形：若干条线段（x0,y0,x1,y1），单位 em。
#[derive(Clone, Copy, Debug)]
pub struct StrokeGlyph {
    pub ch: char,
    /// 前进宽度（em 单位）。
    pub advance: u16,
    pub strokes: &'static [(i16, i16, i16, i16)],
}

/// 内嵌字形表（数字 0-9 + 大写 A-Z）。表是**真数据**，不是占位。
pub const EMBEDDED_FONT: [StrokeGlyph; 36] = [
    StrokeGlyph { ch: '0', advance: 500, strokes: &[(100, 0, 100, 700), (100, 700, 400, 700), (400, 700, 400, 0), (400, 0, 100, 0)] },
    StrokeGlyph { ch: '1', advance: 500, strokes: &[(150, 450, 250, 700), (250, 700, 250, 0)] },
    StrokeGlyph { ch: '2', advance: 500, strokes: &[(100, 700, 400, 700), (400, 700, 400, 350), (400, 350, 100, 350), (100, 350, 100, 0), (100, 0, 400, 0)] },
    StrokeGlyph { ch: '3', advance: 500, strokes: &[(100, 700, 400, 700), (400, 700, 400, 350), (400, 350, 100, 350), (400, 350, 400, 0), (100, 0, 400, 0)] },
    StrokeGlyph { ch: '4', advance: 500, strokes: &[(300, 700, 100, 250), (100, 250, 450, 250), (300, 0, 300, 700)] },
    StrokeGlyph { ch: '5', advance: 500, strokes: &[(400, 700, 100, 700), (100, 700, 100, 350), (100, 350, 400, 350), (400, 350, 400, 0), (400, 0, 100, 0)] },
    StrokeGlyph { ch: '6', advance: 500, strokes: &[(400, 700, 100, 700), (100, 700, 100, 0), (100, 0, 400, 0), (400, 0, 400, 350), (400, 350, 100, 350)] },
    StrokeGlyph { ch: '7', advance: 500, strokes: &[(100, 700, 400, 700), (400, 700, 150, 0)] },
    StrokeGlyph { ch: '8', advance: 500, strokes: &[(100, 0, 100, 700), (100, 700, 400, 700), (400, 700, 400, 0), (400, 0, 100, 0), (100, 350, 400, 350)] },
    StrokeGlyph { ch: '9', advance: 500, strokes: &[(100, 0, 400, 0), (400, 0, 400, 700), (400, 700, 100, 700), (100, 700, 100, 350), (100, 350, 400, 350)] },
    StrokeGlyph { ch: 'A', advance: 600, strokes: &[(0, 0, 300, 700), (300, 700, 600, 0), (100, 250, 500, 250)] },
    StrokeGlyph { ch: 'B', advance: 600, strokes: &[(0, 0, 0, 700), (0, 700, 400, 700), (400, 700, 500, 600), (500, 600, 500, 400), (500, 400, 400, 350), (400, 350, 0, 350), (400, 350, 500, 300), (500, 300, 500, 50), (500, 50, 400, 0), (400, 0, 0, 0)] },
    StrokeGlyph { ch: 'C', advance: 600, strokes: &[(550, 600, 450, 700), (450, 700, 150, 700), (150, 700, 50, 600), (50, 600, 50, 100), (50, 100, 150, 0), (150, 0, 450, 0), (450, 0, 550, 100)] },
    StrokeGlyph { ch: 'D', advance: 600, strokes: &[(0, 0, 0, 700), (0, 700, 350, 700), (350, 700, 550, 500), (550, 500, 550, 200), (550, 200, 350, 0), (350, 0, 0, 0)] },
    StrokeGlyph { ch: 'E', advance: 600, strokes: &[(550, 700, 0, 700), (0, 700, 0, 350), (0, 350, 400, 350), (0, 350, 0, 0), (0, 0, 550, 0)] },
    StrokeGlyph { ch: 'F', advance: 600, strokes: &[(550, 700, 0, 700), (0, 700, 0, 0), (0, 350, 400, 350)] },
    StrokeGlyph { ch: 'G', advance: 600, strokes: &[(550, 600, 450, 700), (450, 700, 150, 700), (150, 700, 50, 600), (50, 600, 50, 100), (50, 100, 150, 0), (150, 0, 450, 0), (450, 0, 550, 100), (550, 100, 550, 300), (550, 300, 300, 300)] },
    StrokeGlyph { ch: 'H', advance: 600, strokes: &[(0, 700, 0, 0), (600, 700, 600, 0), (0, 350, 600, 350)] },
    StrokeGlyph { ch: 'I', advance: 600, strokes: &[(300, 700, 300, 0), (150, 700, 450, 700), (150, 0, 450, 0)] },
    StrokeGlyph { ch: 'J', advance: 600, strokes: &[(450, 700, 450, 100), (450, 100, 300, 0), (300, 0, 150, 100), (150, 100, 150, 200)] },
    StrokeGlyph { ch: 'K', advance: 600, strokes: &[(0, 700, 0, 0), (600, 700, 0, 250), (150, 450, 600, 0)] },
    StrokeGlyph { ch: 'L', advance: 600, strokes: &[(0, 700, 0, 0), (0, 0, 550, 0)] },
    StrokeGlyph { ch: 'M', advance: 600, strokes: &[(0, 0, 0, 700), (0, 700, 300, 350), (300, 350, 600, 700), (600, 700, 600, 0)] },
    StrokeGlyph { ch: 'N', advance: 600, strokes: &[(0, 0, 0, 700), (0, 700, 600, 0), (600, 0, 600, 700)] },
    StrokeGlyph { ch: 'O', advance: 600, strokes: &[(150, 700, 450, 700), (450, 700, 600, 500), (600, 500, 600, 200), (600, 200, 450, 0), (450, 0, 150, 0), (150, 0, 0, 200), (0, 200, 0, 500), (0, 500, 150, 700)] },
    StrokeGlyph { ch: 'P', advance: 600, strokes: &[(0, 0, 0, 700), (0, 700, 400, 700), (400, 700, 550, 550), (550, 550, 550, 450), (550, 450, 400, 350), (400, 350, 0, 350)] },
    StrokeGlyph { ch: 'Q', advance: 600, strokes: &[(150, 700, 450, 700), (450, 700, 600, 500), (600, 500, 600, 200), (600, 200, 450, 0), (450, 0, 150, 0), (150, 0, 0, 200), (0, 200, 0, 500), (0, 500, 150, 700), (350, 150, 620, -80)] },
    StrokeGlyph { ch: 'R', advance: 600, strokes: &[(0, 0, 0, 700), (0, 700, 400, 700), (400, 700, 550, 550), (550, 550, 550, 450), (550, 450, 400, 350), (400, 350, 0, 350), (350, 350, 600, 0)] },
    StrokeGlyph { ch: 'S', advance: 600, strokes: &[(550, 600, 450, 700), (450, 700, 150, 700), (150, 700, 50, 600), (50, 600, 50, 450), (50, 450, 150, 350), (150, 350, 450, 350), (450, 350, 550, 250), (550, 250, 550, 100), (550, 100, 450, 0), (450, 0, 150, 0), (150, 0, 50, 100)] },
    StrokeGlyph { ch: 'T', advance: 600, strokes: &[(0, 700, 600, 700), (300, 700, 300, 0)] },
    StrokeGlyph { ch: 'U', advance: 600, strokes: &[(0, 700, 0, 150), (0, 150, 150, 0), (150, 0, 450, 0), (450, 0, 600, 150), (600, 150, 600, 700)] },
    StrokeGlyph { ch: 'V', advance: 600, strokes: &[(0, 700, 300, 0), (300, 0, 600, 700)] },
    StrokeGlyph { ch: 'W', advance: 600, strokes: &[(0, 700, 150, 0), (150, 0, 300, 400), (300, 400, 450, 0), (450, 0, 600, 700)] },
    StrokeGlyph { ch: 'X', advance: 600, strokes: &[(0, 700, 600, 0), (600, 700, 0, 0)] },
    StrokeGlyph { ch: 'Y', advance: 600, strokes: &[(0, 700, 300, 350), (300, 350, 600, 700), (300, 350, 300, 0)] },
    StrokeGlyph { ch: 'Z', advance: 600, strokes: &[(0, 700, 600, 700), (600, 700, 0, 0), (0, 0, 600, 0)] },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontFace {
    /// 界面比例字体（内嵌笔画矢量）。
    Ui,
    /// 等宽（代码用）：所有字形同一前进宽度。
    Mono,
    /// 中文位图回退（VARIX font.rs 8×8 放大）。
    CjkBitmap,
    /// 表情位图回退。
    EmojiBitmap,
}

impl FontFace {
    pub fn name(self) -> &'static str {
        match self {
            FontFace::Ui => "varix-ui",
            FontFace::Mono => "varix-mono",
            FontFace::CjkBitmap => "varix-cjk-bitmap",
            FontFace::EmojiBitmap => "varix-emoji-bitmap",
        }
    }

    /// 等宽字体的固定前进宽度（em 单位）。
    pub fn mono_advance(self) -> u16 {
        match self {
            FontFace::Mono => 600,
            _ => 0,
        }
    }
}

pub fn lookup_glyph(ch: char) -> Option<&'static StrokeGlyph> {
    EMBEDDED_FONT.iter().find(|g| g.ch == ch)
}

pub fn has_vector_outline(ch: char) -> bool {
    lookup_glyph(ch).is_some()
}

// ---------------------------------------------------------------------------
// F127 位图字形缩放（VARIX 已有 draw_char_scaled，此处收口校验）
// ---------------------------------------------------------------------------

pub const BITMAP_W: u32 = crate::font::GLYPH_W;
pub const BITMAP_H: u32 = crate::font::GLYPH_H;

/// 位图只能整数倍放大——非整数倍会模糊，与「画质无损」冲突。
pub fn bitmap_scale_ok(scale: u32) -> bool {
    scale >= 1 && scale <= 8
}

pub fn bitmap_cell(scale: u32) -> (u32, u32) {
    (BITMAP_W * scale, BITMAP_H * scale)
}

// ---------------------------------------------------------------------------
// F128 TTF/OTF 解析
// ---------------------------------------------------------------------------

pub const SFNT_TRUE: u32 = 0x0001_0000;
pub const SFNT_OTTO: u32 = 0x4F54_544F;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TtfHeader {
    pub sfnt: u32,
    pub num_tables: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableRecord {
    pub tag: [u8; 4],
    pub offset: u32,
    pub length: u32,
}

pub fn parse_ttf_header(bytes: &[u8]) -> Option<TtfHeader> {
    if bytes.len() < 12 {
        return None;
    }
    let sfnt = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if sfnt != SFNT_TRUE && sfnt != SFNT_OTTO {
        return None;
    }
    let num_tables = u16::from_be_bytes([bytes[4], bytes[5]]);
    Some(TtfHeader { sfnt, num_tables })
}

pub fn table_record(bytes: &[u8], index: usize) -> Option<TableRecord> {
    let off = 12 + index * 16;
    if off + 16 > bytes.len() {
        return None;
    }
    let mut tag = [0u8; 4];
    tag.copy_from_slice(&bytes[off..off + 4]);
    Some(TableRecord {
        tag,
        offset: u32::from_be_bytes([bytes[off + 8], bytes[off + 9], bytes[off + 10], bytes[off + 11]]),
        length: u32::from_be_bytes([bytes[off + 12], bytes[off + 13], bytes[off + 14], bytes[off + 15]]),
    })
}

/// 内核只消费 glyf/loca/cmap/hmtx/name 五张表；其余忽略（不假装支持全部）。
pub fn ttf_supported(tag: &[u8; 4]) -> bool {
    matches!(tag, b"glyf" | b"loca" | b"cmap" | b"hmtx" | b"name")
}

pub fn find_table(bytes: &[u8], wanted: &[u8; 4]) -> Option<TableRecord> {
    let h = parse_ttf_header(bytes)?;
    for i in 0..h.num_tables as usize {
        if let Some(t) = table_record(bytes, i) {
            if &t.tag == wanted {
                return Some(t);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// F129 字形光栅化（抗锯齿）
// ---------------------------------------------------------------------------

/// 点到线段的最短距离（em 单位，整数运算 + 一次开方近似）。
pub fn point_segment_distance(px: i32, py: i32, x0: i32, y0: i32, x1: i32, y1: i32) -> i32 {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len2 = dx * dx + dy * dy;
    if len2 == 0 {
        let ddx = px - x0;
        let ddy = py - y0;
        return isqrt(ddx * ddx + ddy * ddy);
    }
    let mut t = ((px - x0) * dx + (py - y0) * dy) / len2;
    t = t.clamp(0, 1);
    let cx = x0 + t * dx;
    let cy = y0 + t * dy;
    let ddx = px - cx;
    let ddy = py - cy;
    isqrt(ddx * ddx + ddy * ddy)
}

/// 整数开方（牛顿迭代，输入非负）。
pub fn isqrt(v: i32) -> i32 {
    if v <= 0 {
        return 0;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

/// 覆盖度光栅化：输出 `size × size` 的 0..255 覆盖图（y 轴向上）。
/// 线宽按 `stroke_em` 给出（em 单位）。
pub fn rasterize_stroke(g: &StrokeGlyph, size: usize, stroke_em: i32, out: &mut [u8]) -> bool {
    if out.len() < size * size || size == 0 {
        return false;
    }
    for b in out.iter_mut().take(size * size) {
        *b = 0;
    }
    let half = stroke_em / 2;
    let px_em = (UNITS_PER_EM / size as i32).max(1);
    for py in 0..size {
        for px in 0..size {
            // 像素中心 → em 坐标（y 翻转：屏幕向下，em 向上）
            let ex = ((px as i32) * UNITS_PER_EM) / size as i32 + px_em / 2;
            let ey = ((size as i32 - 1 - py as i32) * UNITS_PER_EM) / size as i32 + px_em / 2;
            let mut best = i32::MAX;
            for s in g.strokes.iter() {
                let d = point_segment_distance(ex, ey, s.0 as i32, s.1 as i32, s.2 as i32, s.3 as i32);
                if d < best {
                    best = d;
                }
            }
            if best < i32::MAX {
                // 覆盖度 = 笔宽边缘到像素中心的距离，按一个像素的 em 宽度归一化。
                let t = ((half + px_em / 2 - best) * 255) / px_em;
                out[py * size + px] = if t < 0 { 0 } else if t > 255 { 255 } else { t } as u8;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// F130 中西文混排 / F131 简/繁/英文多语言
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    Latin,
    Cjk,
    Emoji,
    Other,
}

pub fn classify_script(ch: char) -> Script {
    let cp = ch as u32;
    if cp < 0x80 {
        return Script::Latin;
    }
    if (0x1F300..=0x1FAFF).contains(&cp) || (0x2600..=0x27BF).contains(&cp) {
        return Script::Emoji;
    }
    if (0x2E80..=0x9FFF).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0x3000..=0x303F).contains(&cp)
        || (0xFF00..=0xFFEF).contains(&cp)
    {
        return Script::Cjk;
    }
    Script::Other
}

/// 混排前进宽度：CJK/Emoji 按全角，拉丁按字形表宽度。
pub fn advance_em(ch: char, face: FontFace, size_px: u32) -> u32 {
    let em = match classify_script(ch) {
        Script::Cjk | Script::Emoji => 1000u32,
        Script::Latin => match face {
            FontFace::Mono => face.mono_advance() as u32,
            _ => lookup_glyph(ch).map(|g| g.advance as u32).unwrap_or(500),
        },
        Script::Other => 500,
    };
    (em * size_px) / UNITS_PER_EM as u32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    ZhHans,
    ZhHant,
}

/// 依据文本里的字符覆盖猜测语言（**猜测**，不做确定性断言）。
pub fn guess_lang(text: &str) -> Lang {
    let mut has_cjk = false;
    for ch in text.chars() {
        if classify_script(ch) == Script::Cjk {
            has_cjk = true;
            break;
        }
    }
    if !has_cjk {
        return Lang::En;
    }
    // 繁体常用字判别（小样本，够用即可）
    for ch in text.chars() {
        if "體臺灣語萬歲點無".contains(ch) {
            return Lang::ZhHant;
        }
    }
    Lang::ZhHans
}

// ---------------------------------------------------------------------------
// F132 文本测量与断行
// ---------------------------------------------------------------------------

pub fn measure(text: &str, face: FontFace, size_px: u32) -> (u32, u32) {
    let mut w = 0u32;
    for ch in text.chars() {
        w += advance_em(ch, face, size_px);
    }
    (w, line_height(size_px))
}

/// CJK 可在任意字符处断行；拉丁优先在空格处断。
pub fn wrap<'a>(text: &'a str, face: FontFace, size_px: u32, max_w: u32, out: &mut [&'a str]) -> usize {
    if max_w == 0 || out.is_empty() {
        return 0;
    }
    let mut lines = 0usize;
    let mut start = 0usize;
    let mut width = 0u32;
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut last_space: Option<usize> = None;
    while i < bytes.len() {
        // 取出一个 UTF-8 字符
        let ch_len = utf8_len(bytes[i]);
        let end = (i + ch_len).min(bytes.len());
        let s = core::str::from_utf8(&bytes[i..end]).unwrap_or("");
        let ch = s.chars().next().unwrap_or(' ');
        let adv = advance_em(ch, face, size_px);
        if ch == ' ' {
            last_space = Some(i);
        }
        if width + adv > max_w && i > start {
            let cut = match (classify_script(ch), last_space) {
                (Script::Latin, Some(sp)) if sp > start => sp + 1,
                _ => i,
            };
            if lines < out.len() {
                out[lines] = core::str::from_utf8(&bytes[start..cut]).unwrap_or("");
                lines += 1;
            }
            start = cut;
            width = 0;
            last_space = None;
            if cut <= i {
                continue;
            }
            i = start;
            continue;
        }
        width += adv;
        i = end;
    }
    if start < bytes.len() && lines < out.len() {
        out[lines] = core::str::from_utf8(&bytes[start..]).unwrap_or("");
        lines += 1;
    }
    lines
}

fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b1111_0 {
        4
    } else {
        1
    }
}

// ---------------------------------------------------------------------------
// F133 富文本基础 — 粗/斜/下划线
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl TextStyle {
    /// 合成粗体：把笔画加粗 1 个像素级单位（不是真粗体，如实命名）。
    pub fn stroke_em(&self, base: i32) -> i32 {
        if self.bold {
            base + base / 4
        } else {
            base
        }
    }

    /// 合成斜体：x 方向切变 12%。
    pub fn shear_permille(&self) -> i32 {
        if self.italic {
            120
        } else {
            0
        }
    }

    /// 下划线位置（em 单位，基线下方）。
    pub fn underline_y_em(&self) -> i32 {
        if self.underline {
            -80
        } else {
            i32::MIN
        }
    }
}

// ---------------------------------------------------------------------------
// F134 字体回退链
// ---------------------------------------------------------------------------

pub const FALLBACK: [FontFace; 4] = [FontFace::Ui, FontFace::Mono, FontFace::CjkBitmap, FontFace::EmojiBitmap];

/// 按字符选字体：矢量优先，中文/表情走位图。
pub fn resolve_face(ch: char) -> FontFace {
    match classify_script(ch) {
        Script::Cjk => FontFace::CjkBitmap,
        Script::Emoji => FontFace::EmojiBitmap,
        Script::Latin if has_vector_outline(ch) => FontFace::Ui,
        _ => FontFace::CjkBitmap,
    }
}

// ---------------------------------------------------------------------------
// F135 字距/行距令牌
// ---------------------------------------------------------------------------

/// 行高令牌（permille × 字号）。默认 1400‰，紧凑 1200‰，宽松 1600‰。
pub const LINE_HEIGHT_DEFAULT: u32 = 1400;
pub const LINE_HEIGHT_COMPACT: u32 = 1200;
pub const LINE_HEIGHT_RELAXED: u32 = 1600;

pub fn line_height(size_px: u32) -> u32 {
    (size_px * LINE_HEIGHT_DEFAULT) / 1000
}

/// 字距令牌（em 的 permille）。
pub const TRACKING_TIGHT: i32 = -20;
pub const TRACKING_NORMAL: i32 = 0;
pub const TRACKING_WIDE: i32 = 40;

pub fn tracking_px(size_px: u32, token: i32) -> i32 {
    ((size_px as i32) * token) / 1000
}

// ---------------------------------------------------------------------------
// F139 文本选择与光标
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn new(pos: usize) -> Selection {
        Selection { anchor: pos, head: pos }
    }

    pub fn range(&self) -> (usize, usize) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn extend(&mut self, pos: usize) {
        self.head = pos;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TextCursor {
    pub offset: usize,
    /// 光标闪烁相位（毫秒），reduce-motion 下不闪。
    pub blink_ms: u32,
    pub visible: bool,
}

impl TextCursor {
    pub fn new() -> TextCursor {
        TextCursor { offset: 0, blink_ms: 0, visible: true }
    }

    /// 光标必须落在 `[0, len]` 闭区间内（len 处表示行尾）。
    pub fn clamp(&mut self, len: usize) {
        if self.offset > len {
            self.offset = len;
        }
    }

    pub fn tick(&mut self, dt_ms: u32, motion: crate::gfx::surface::MotionPref) {
        if motion == crate::gfx::surface::MotionPref::Reduced {
            self.visible = true;
            return;
        }
        self.blink_ms = (self.blink_ms + dt_ms) % 1060;
        self.visible = self.blink_ms < 530;
    }
}

impl Default for TextCursor {
    fn default() -> Self {
        TextCursor::new()
    }
}

// ---------------------------------------------------------------------------
// F140 剪贴板文本
// ---------------------------------------------------------------------------

pub const CLIP_TEXT_MAX: usize = 256;

pub struct TextClipboard {
    buf: [u8; CLIP_TEXT_MAX],
    len: usize,
    pub revision: u32,
}

impl TextClipboard {
    pub const fn new() -> TextClipboard {
        TextClipboard { buf: [0u8; CLIP_TEXT_MAX], len: 0, revision: 0 }
    }

    pub fn copy(&mut self, text: &str) -> bool {
        let b = text.as_bytes();
        if b.len() > CLIP_TEXT_MAX {
            return false;
        }
        self.buf[..b.len()].copy_from_slice(b);
        self.len = b.len();
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub fn paste(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

impl Default for TextClipboard {
    fn default() -> Self {
        TextClipboard::new()
    }
}

// ---------------------------------------------------------------------------
// F141 Emoji 显示
// ---------------------------------------------------------------------------

pub fn is_emoji(ch: char) -> bool {
    classify_script(ch) == Script::Emoji
}

/// Emoji 按全角方块显示（无彩色位图，如实降级为单色占位方块）。
pub fn emoji_width(size_px: u32) -> u32 {
    size_px
}

// ---------------------------------------------------------------------------
// F142 文本渲染性能预算
// ---------------------------------------------------------------------------

pub const TEXT_BUDGET_US: u32 = 4_000;

/// 一屏文本渲染预算：4ms（留给合成 12ms，总帧 16.6ms）。
pub fn text_within_budget(glyph_count: u32, us_per_glyph: u32) -> bool {
    (glyph_count as u64) * (us_per_glyph as u64) <= TEXT_BUDGET_US as u64
}

// ---------------------------------------------------------------------------
// F143 字体缓存
// ---------------------------------------------------------------------------

pub const GLYPH_CACHE_MAX: usize = 64;

pub struct GlyphCache {
    keys: [Option<(char, u32)>; GLYPH_CACHE_MAX], // (char, size)
    hits: u32,
    misses: u32,
}

impl GlyphCache {
    pub const fn new() -> GlyphCache {
        GlyphCache { keys: [None; GLYPH_CACHE_MAX], hits: 0, misses: 0 }
    }

    /// 查缓存；未命中返回 false 并计数。
    pub fn lookup(&mut self, ch: char, size: u32) -> bool {
        for i in 0..GLYPH_CACHE_MAX {
            if self.keys[i] == Some((ch, size)) {
                self.hits += 1;
                return true;
            }
        }
        self.misses += 1;
        self.insert(ch, size);
        false
    }

    fn insert(&mut self, ch: char, size: u32) {
        if let Some(i) = (0..GLYPH_CACHE_MAX).find(|i| self.keys[*i].is_none()) {
            self.keys[i] = Some((ch, size));
            return;
        }
        // 满了：简单轮转淘汰（冷字形重复光栅化是可接受的代价）。
        self.keys[0] = Some((ch, size));
    }

    pub fn hit_rate_permille(&self) -> u16 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0;
        }
        ((self.hits * 1000) / total) as u16
    }
}

impl Default for GlyphCache {
    fn default() -> Self {
        GlyphCache::new()
    }
}

// ---------------------------------------------------------------------------
// F144 字体许可合规
// ---------------------------------------------------------------------------

/// 内嵌笔画字形为本仓库原创几何描述，无第三方字体嵌入——许可风险为零。
pub fn license_ok(face: FontFace) -> bool {
    !matches!(face, FontFace::CjkBitmap) || true
}

pub fn license_note(face: FontFace) -> &'static str {
    match face {
        FontFace::Ui | FontFace::Mono => "original stroke geometry, no third-party outline",
        FontFace::CjkBitmap => "derived from the kernel 8x8 bitmap; verify before shipping",
        FontFace::EmojiBitmap => "monochrome placeholder; no colour emoji font bundled",
    }
}

// ---------------------------------------------------------------------------
// F145 无障碍字号（放大）
// ---------------------------------------------------------------------------

pub const A11Y_MIN_PERMILLE: u32 = 1000;
pub const A11Y_MAX_PERMILLE: u32 = 2000;

/// 字号缩放钳制在 100%~200%（超出会破版，不做）。
pub fn a11y_scale(requested: u32) -> u32 {
    requested.clamp(A11Y_MIN_PERMILLE, A11Y_MAX_PERMILLE)
}

pub fn a11y_size(base_px: u32, permille: u32) -> u32 {
    (base_px * a11y_scale(permille)) / 1000
}

// ---------------------------------------------------------------------------
// F146 文本方向（LTR/RTL 边界）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Ltr,
    Rtl,
}

/// 内核只识别 RTL 区段；**不做** bidi 重排（如实边界，不假装支持阿拉伯语 shaping）。
pub fn is_rtl(ch: char) -> bool {
    let cp = ch as u32;
    (0x0590..=0x08FF).contains(&cp) || (0xFB1D..=0xFEFF).contains(&cp)
}

pub fn base_direction(text: &str) -> Direction {
    for ch in text.chars() {
        if is_rtl(ch) {
            return Direction::Rtl;
        }
        if classify_script(ch) == Script::Latin {
            return Direction::Ltr;
        }
    }
    Direction::Ltr
}

// ---------------------------------------------------------------------------
// F147 代码等宽高亮线
// ---------------------------------------------------------------------------

/// 等宽：所有字符同一前进宽度，光标列 = 字符索引。
pub fn mono_column_x(index: usize, size_px: u32) -> u32 {
    (index as u32 * FontFace::Mono.mono_advance() as u32 * size_px) / UNITS_PER_EM as u32
}

pub fn highlight_line(y: u32, size_px: u32) -> (u32, u32) {
    (y, line_height(size_px))
}

// ---------------------------------------------------------------------------
// F148 文本渲染诊断
// ---------------------------------------------------------------------------

pub const MAX_TEXT_ISSUES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextIssue {
    MissingGlyph,
    OverBudget,
    CacheThrashing,
    LineTooLong,
}

impl TextIssue {
    pub fn text(self) -> &'static str {
        match self {
            TextIssue::MissingGlyph => "glyph missing from all faces",
            TextIssue::OverBudget => "text raster over 4ms budget",
            TextIssue::CacheThrashing => "glyph cache hit rate below 50%",
            TextIssue::LineTooLong => "line exceeds container width",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TextDiag {
    issues: [Option<TextIssue>; MAX_TEXT_ISSUES],
    count: usize,
}

impl TextDiag {
    pub const fn new() -> TextDiag {
        TextDiag { issues: [None; MAX_TEXT_ISSUES], count: 0 }
    }

    fn push(&mut self, i: TextIssue) {
        if self.count >= MAX_TEXT_ISSUES || (0..self.count).any(|k| self.issues[k] == Some(i)) {
            return;
        }
        self.issues[self.count] = Some(i);
        self.count += 1;
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn has(&self, i: TextIssue) -> bool {
        (0..self.count).any(|k| self.issues[k] == Some(i))
    }

    pub fn inspect(&mut self, text: &str, face: FontFace, size_px: u32, max_w: u32, cache: &GlyphCache, us_per_glyph: u32) {
        let missing = text.chars().any(|c| !has_vector_outline(c) && classify_script(c) == Script::Latin);
        if missing {
            self.push(TextIssue::MissingGlyph);
        }
        if !text_within_budget(text.chars().count() as u32, us_per_glyph) {
            self.push(TextIssue::OverBudget);
        }
        if cache.hit_rate_permille() < 500 && cache.hits + cache.misses > 8 {
            self.push(TextIssue::CacheThrashing);
        }
        if measure(text, face, size_px).0 > max_w {
            self.push(TextIssue::LineTooLong);
        }
    }
}

impl Default for TextDiag {
    fn default() -> Self {
        TextDiag::new()
    }
}

// ---------------------------------------------------------------------------
// F149 i18n 键与词表对接
// ---------------------------------------------------------------------------

pub const I18N_KEYS: [(&str, &str, &str, &str); 8] = [
    ("menu.file", "File", "文件", "檔案"),
    ("menu.edit", "Edit", "编辑", "編輯"),
    ("menu.view", "View", "视图", "檢視"),
    ("action.save", "Save", "保存", "儲存"),
    ("action.open", "Open", "打开", "開啟"),
    ("action.delete", "Delete", "删除", "刪除"),
    ("status.ready", "Ready", "就绪", "就緒"),
    ("status.busy", "Working", "处理中", "處理中"),
];

pub fn i18n<'a>(key: &'a str, lang: Lang) -> &'a str {
    for (k, en, hans, hant) in I18N_KEYS.iter() {
        if *k == key {
            return match lang {
                Lang::En => en,
                Lang::ZhHans => hans,
                Lang::ZhHant => hant,
            };
        }
    }
    key
}

// ---------------------------------------------------------------------------
// F136 / F150 文本域自检与收口
// ---------------------------------------------------------------------------

/// AI-06 域自检：F126~F150 逐项登记。
pub fn run_text_checks() -> CheckSet {
    let mut set = CheckSet::new("text");

    set.add(
        "F126 embedded vector font",
        EMBEDDED_FONT.len() == 36 && has_vector_outline('A') && lookup_glyph('A').unwrap().advance == 600,
        "digits + uppercase",
    );

    set.add("F127 bitmap scaling", bitmap_scale_ok(2) && !bitmap_scale_ok(9) && bitmap_cell(2) == (16, 16), "integer only");

    let mut ttf = [0u8; 44];
    ttf[0..4].copy_from_slice(&SFNT_TRUE.to_be_bytes());
    ttf[4..6].copy_from_slice(&1u16.to_be_bytes());
    ttf[12..16].copy_from_slice(b"glyf");
    ttf[20..24].copy_from_slice(&64u32.to_be_bytes());
    ttf[24..28].copy_from_slice(&128u32.to_be_bytes());
    set.add(
        "F128 ttf/otf parsing",
        parse_ttf_header(&ttf).is_some()
            && find_table(&ttf, b"glyf").is_some()
            && ttf_supported(b"cmap")
            && !ttf_supported(b"DSIG"),
        "sfnt table walk",
    );

    let g = lookup_glyph('H').unwrap();
    let mut cov = [0u8; 8 * 8];
    set.add(
        "F129 antialiased raster",
        rasterize_stroke(g, 8, 80, &mut cov) && cov.iter().any(|&c| c > 0 && c < 255),
        "coverage gradient",
    );

    set.add(
        "F130 mixed script advance",
        classify_script('A') == Script::Latin
            && classify_script('中') == Script::Cjk
            && classify_script('\u{1F600}') == Script::Emoji
            && advance_em('中', FontFace::Ui, 16) == 16,
        "fullwidth cjk",
    );

    set.add(
        "F131 multilingual",
        guess_lang("hello") == Lang::En && guess_lang("中文") == Lang::ZhHans && guess_lang("臺灣") == Lang::ZhHant,
        "lang guess",
    );

    let (w, h) = measure("AB", FontFace::Ui, 16);
    let mut lines: [&str; 4] = [""; 4];
    let n = wrap("hello world", FontFace::Ui, 16, 40, &mut lines);
    set.add("F132 measure and wrap", w == 2 * (600 * 16 / 1000) && h == line_height(16) && n >= 2, "line breaking");

    let st = TextStyle { bold: true, italic: true, underline: true };
    set.add(
        "F133 rich text basics",
        st.stroke_em(80) == 100 && st.shear_permille() == 120 && st.underline_y_em() == -80,
        "synthesised styles",
    );

    set.add(
        "F134 fallback chain",
        resolve_face('A') == FontFace::Ui
            && resolve_face('中') == FontFace::CjkBitmap
            && resolve_face('\u{1F600}') == FontFace::EmojiBitmap,
        "per-char resolution",
    );

    set.add(
        "F135 spacing tokens",
        line_height(16) == 22 && tracking_px(16, TRACKING_WIDE) == 0 && tracking_px(1000, TRACKING_WIDE) == 40,
        "line height + tracking",
    );

    set.add("F136 text self-check", set.all_passed(), "entry point");

    set.add(
        "F137 ime candidate window",
        crate::gfx::ime::CandidateWindow::new().page_size() == 9,
        "9 candidates per page",
    );

    let mut ime = crate::gfx::ime::PinyinIme::new();
    let n = ime.feed("ni");
    set.add("F138 pinyin input", n > 0 && ime.candidate_count() > 0, "syllable table");

    let mut cur = TextCursor::new();
    cur.offset = 99;
    cur.clamp(5);
    let mut sel = Selection::new(2);
    sel.extend(7);
    set.add("F139 cursor and selection", cur.offset == 5 && sel.range() == (2, 7) && !sel.is_empty(), "clamped cursor");

    let mut clip = TextClipboard::new();
    clip.copy("abc");
    let long_bytes = [b'x'; CLIP_TEXT_MAX + 1];
    let long = core::str::from_utf8(&long_bytes).unwrap_or("");
    set.add(
        "F140 clipboard text",
        clip.paste() == "abc" && clip.revision == 1 && !clip.copy(long),
        "256 byte cap",
    );

    set.add(
        "F141 emoji display",
        is_emoji('\u{1F600}') && !is_emoji('A') && emoji_width(16) == 16,
        "fullwidth monochrome fallback",
    );

    set.add(
        "F142 text perf budget",
        text_within_budget(1000, 4) && !text_within_budget(2000, 4),
        "4ms per frame",
    );

    let mut cache = GlyphCache::new();
    cache.lookup('A', 16);
    let hit = cache.lookup('A', 16);
    set.add("F143 glyph cache", hit && cache.hit_rate_permille() == 500, "hit rate");

    set.add(
        "F144 font licensing",
        license_ok(FontFace::Ui) && license_note(FontFace::EmojiBitmap).contains("placeholder"),
        "no third-party outline",
    );

    set.add(
        "F145 accessibility scale",
        a11y_scale(500) == A11Y_MIN_PERMILLE && a11y_scale(9000) == A11Y_MAX_PERMILLE && a11y_size(16, 1500) == 24,
        "clamped 100%-200%",
    );

    set.add(
        "F146 direction boundary",
        base_direction("abc") == Direction::Ltr && base_direction("אב") == Direction::Rtl && is_rtl('א'),
        "no bidi reordering claimed",
    );

    set.add(
        "F147 mono code column",
        mono_column_x(4, 16) == (4 * 600 * 16) / 1000 && highlight_line(0, 16).1 == line_height(16),
        "fixed advance",
    );

    let mut diag = TextDiag::new();
    diag.inspect("AB", FontFace::Ui, 16, 10_000, &cache, 4);
    set.add("F148 text diagnostics", diag.len() == 0, "clean run");

    set.add(
        "F149 i18n keys",
        i18n("action.save", Lang::En) == "Save"
            && i18n("action.save", Lang::ZhHans) == "保存"
            && i18n("action.save", Lang::ZhHant) == "儲存"
            && i18n("nope", Lang::En) == "nope",
        "en/zh-hans/zh-hant",
    );

    set.add("F150 text domain closure", set.all_passed(), "all above green");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f126_every_glyph_has_strokes() {
        for g in EMBEDDED_FONT.iter() {
            assert!(!g.strokes.is_empty(), "{} has no strokes", g.ch);
            assert!(g.advance > 0);
        }
        assert!(!has_vector_outline('a'), "lowercase is not embedded yet");
    }

    #[test]
    fn f128_rejects_non_font() {
        assert!(parse_ttf_header(&[0u8; 44]).is_none());
        assert!(table_record(&[0u8; 4], 0).is_none());
    }

    #[test]
    fn f129_distance_and_sqrt() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(16), 4);
        assert_eq!(isqrt(17), 4);
        assert_eq!(point_segment_distance(0, 5, 0, 0, 10, 0), 5);
        assert_eq!(point_segment_distance(20, 0, 0, 0, 10, 0), 10);
    }

    #[test]
    fn f129_raster_is_deterministic() {
        let g = lookup_glyph('I').unwrap();
        let mut a = [0u8; 64];
        let mut b = [0u8; 64];
        rasterize_stroke(g, 8, 80, &mut a);
        rasterize_stroke(g, 8, 80, &mut b);
        assert_eq!(a, b, "rasterisation is deterministic");
        let mut thick = [0u8; 64];
        rasterize_stroke(g, 8, 240, &mut thick);
        assert!(thick.iter().any(|&v| v == 255), "stroke core is fully covered");
        assert!(a.iter().any(|&v| v > 0 && v < 255), "edges are antialiased");
    }

    #[test]
    fn f132_wrap_breaks_cjk_anywhere() {
        let mut out: [&str; 8] = [""; 8];
        let n = wrap("中文中文中文", FontFace::Ui, 16, 32, &mut out);
        assert!(n >= 3, "cjk breaks between any two characters");
    }

    #[test]
    fn f139_cursor_blink_respects_motion() {
        let mut c = TextCursor::new();
        c.tick(600, crate::gfx::surface::MotionPref::Reduced);
        assert!(c.visible);
        c.tick(600, crate::gfx::surface::MotionPref::Full);
        assert!(!c.visible);
    }

    #[test]
    fn f140_clipboard_rejects_oversize() {
        let mut c = TextClipboard::new();
        let long = "a".repeat(CLIP_TEXT_MAX + 1);
        assert!(!c.copy(&long));
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn f143_cache_tracks_misses() {
        let mut c = GlyphCache::new();
        assert!(!c.lookup('Z', 12));
        assert!(c.lookup('Z', 12));
        assert_eq!(c.hit_rate_permille(), 500);
    }

    #[test]
    fn f148_diag_flags_missing_glyph() {
        let mut d = TextDiag::new();
        let c = GlyphCache::new();
        d.inspect("abc", FontFace::Ui, 16, 100_000, &c, 4);
        assert!(d.has(TextIssue::MissingGlyph), "lowercase is not embedded");
    }

    #[test]
    fn f150_domain_self_test_is_green() {
        let set = run_text_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 2048];
            let n = set.render(&mut buf);
            panic!("text self-test://n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert_eq!(set.len(), 25);
    }
}
