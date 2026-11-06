//! UNX-F50041 文本水印引擎（角度 / 透明度 / 字号三参数）（0x43140 段）
//!
//! 锚点：UNX-F50041-J1 三参数组合 ×30 渲染逐项一致（模拟器档）；
//! 判据锚定 §AI-63 §2 专题二。
//!
//! 数据段：`SEG_BASE = 0x43140`，诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 11`，
//! 逐项见 [`MarkError`]。
//!
//! 水印是**打印路径上最容易「看起来对」的一步**：角度差两度、透明度差 5%、
//! 字号差 2 px，肉眼几乎分不出，但套印后整页的灰度会漂。所以这里不做
//! 「渲染一张图给人看」，而是把渲染做成可复算的函数：
//!
//! ```text
//! (canvas, params) --render--> Raster { px, ink, changed, digest }
//!        ^                                        |
//!        +---------- WmBook 逐条重算比对 ----------+
//! ```
//!
//! 覆盖度用 2×2 超采样（每像素 4 个子样本），命中数 0..=4 直接进整数混色：
//! `darken = bg × opacity × hits / 400`。全程整数混色意味着「透明度 0 必等于
//! 背景」「透明度增大逐像素变暗」这两条不是近似而是恒等式；只有旋转用的
//! `sinf/cosf` 走 `libm`。
//!
//! 三参数闭集：角度 {−45,−30,0,30,45} × 透明度 {15,40,75} × 字号 {10,28}
//! = 30 个组合，与 J1 的「×30 渲染逐项一致」一一对应。
//!
//! 结构清单：
//! * [`Glyph`] + [`FONT`]：5×7 点阵字形表（A–Z / 0–9 / 空格）；
//! * [`WmParams`] + [`combos`] + [`parse_spec`]：三参数与文本的规格解析；
//! * [`Canvas`]：确定性背景（8×8 棋盘，值域 232/240）；
//! * [`render`] → [`Raster`]：旋转 + 超采样 + 整数混色；
//! * [`WmEntry`] + [`WmBook`]：30 组合账本（严格入账 / 非信任导入 / 全量复核 / 收口）；
//! * [`injection_core`]：8 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::{cosf, sinf};

/// 诊断段（0x43140 独占 0x20）。
pub const SEG_BASE: u32 = 0x43140;

const E_ANGLE: u16 = 1;
const E_OPACITY: u16 = 2;
const E_SIZE: u16 = 3;
const E_EMPTY_TEXT: u16 = 4;
const E_TEXT_LONG: u16 = 5;
const E_GLYPH: u16 = 6;
const E_CANVAS: u16 = 7;
const E_DIGEST: u16 = 8;
const E_FULL: u16 = 9;
const E_DUPLICATE: u16 = 10;
const E_MISSING: u16 = 11;

/// 水印引擎错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkError {
    /// 角度越出 −180..=180。
    Angle,
    /// 不透明度越出 0..=100（百分比）。
    Opacity,
    /// 字号越出 6..=96（像素）。
    Size,
    /// 文本为空。
    EmptyText,
    /// 文本超过 24 字节。
    TextLong,
    /// 文本含字形表里没有的字符（含小写）。
    Glyph,
    /// 画布尺寸为 0。
    Canvas,
    /// 复算摘要与记录不符。
    Digest,
    /// 账本容量已满。
    Full,
    /// 同一 (角度, 透明度, 字号) 重复入账。
    Duplicate,
    /// 收口时条目不足 30。
    Missing,
}

impl MarkError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            MarkError::Angle => E_ANGLE,
            MarkError::Opacity => E_OPACITY,
            MarkError::Size => E_SIZE,
            MarkError::EmptyText => E_EMPTY_TEXT,
            MarkError::TextLong => E_TEXT_LONG,
            MarkError::Glyph => E_GLYPH,
            MarkError::Canvas => E_CANVAS,
            MarkError::Digest => E_DIGEST,
            MarkError::Full => E_FULL,
            MarkError::Duplicate => E_DUPLICATE,
            MarkError::Missing => E_MISSING,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            MarkError::Angle => "angle outside -180..=180",
            MarkError::Opacity => "opacity outside 0..=100",
            MarkError::Size => "font size outside 6..=96",
            MarkError::EmptyText => "watermark text empty",
            MarkError::TextLong => "watermark text longer than 24 bytes",
            MarkError::Glyph => "character not in the 5x7 font",
            MarkError::Canvas => "canvas has a zero dimension",
            MarkError::Digest => "raster digest mismatch",
            MarkError::Full => "watermark book full",
            MarkError::Duplicate => "angle/opacity/size triple already booked",
            MarkError::Missing => "watermark book has fewer than 30 entries",
        }
    }
}

/// 5×7 点阵字形：7 行，每行低 5 位自左向右（bit4 = 最左列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    /// 字符（仅 ASCII 大写 / 数字 / 空格）。
    pub ch: u8,
    /// 7 行点阵。
    pub rows: [u8; 7],
}

/// 字形表。手写 5×7 点阵：只有 37 个字符，足够覆盖水印常用的
/// 「DRAFT / CONFIDENTIAL / 编号」，又不必引入字体文件（no_std 无 IO）。
pub const FONT: [Glyph; 37] = [
    Glyph { ch: b'A', rows: [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001] },
    Glyph { ch: b'B', rows: [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110] },
    Glyph { ch: b'C', rows: [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110] },
    Glyph { ch: b'D', rows: [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110] },
    Glyph { ch: b'E', rows: [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111] },
    Glyph { ch: b'F', rows: [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000] },
    Glyph { ch: b'G', rows: [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111] },
    Glyph { ch: b'H', rows: [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001] },
    Glyph { ch: b'I', rows: [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110] },
    Glyph { ch: b'J', rows: [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100] },
    Glyph { ch: b'K', rows: [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001] },
    Glyph { ch: b'L', rows: [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111] },
    Glyph { ch: b'M', rows: [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001] },
    Glyph { ch: b'N', rows: [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001] },
    Glyph { ch: b'O', rows: [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110] },
    Glyph { ch: b'P', rows: [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000] },
    Glyph { ch: b'Q', rows: [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101] },
    Glyph { ch: b'R', rows: [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001] },
    Glyph { ch: b'S', rows: [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110] },
    Glyph { ch: b'T', rows: [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100] },
    Glyph { ch: b'U', rows: [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110] },
    Glyph { ch: b'V', rows: [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100] },
    Glyph { ch: b'W', rows: [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001] },
    Glyph { ch: b'X', rows: [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001] },
    Glyph { ch: b'Y', rows: [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100] },
    Glyph { ch: b'Z', rows: [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111] },
    Glyph { ch: b'0', rows: [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110] },
    Glyph { ch: b'1', rows: [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110] },
    Glyph { ch: b'2', rows: [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111] },
    Glyph { ch: b'3', rows: [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110] },
    Glyph { ch: b'4', rows: [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010] },
    Glyph { ch: b'5', rows: [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110] },
    Glyph { ch: b'6', rows: [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110] },
    Glyph { ch: b'7', rows: [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000] },
    Glyph { ch: b'8', rows: [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110] },
    Glyph { ch: b'9', rows: [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100] },
    Glyph { ch: b' ', rows: [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000] },
];

/// 字形行数。
pub const GLYPH_ROWS: usize = 7;
/// 字形列数。
pub const GLYPH_COLS: usize = 5;
/// 字间步进（列为单位：5 列字形 + 1 列间距）。
pub const ADVANCE_COLS: f32 = 6.0;
/// 文本字节上限。
pub const TEXT_CAP: usize = 24;
/// 组合用的固定水印文本。
pub const COMBO_TEXT: &'static str = "DRAFT";
/// 角度闭集（度）。
pub const ANGLES: [i16; 5] = [-45, -30, 0, 30, 45];
/// 不透明度闭集（百分比）。
pub const OPACITIES: [u8; 3] = [15, 40, 75];
/// 字号闭集（像素）。
pub const SIZES: [u8; 2] = [10, 28];
/// J1 的组合总数。
pub const COMBO_COUNT: usize = 30;
/// 账本容量（30 组合 + 6 个复测位）。
pub const MAX_ENTRIES: usize = 36;

/// 按字符取字形；大小写敏感（水印文本统一大写，避免两套点阵）。
pub fn glyph_of(ch: u8) -> Option<&'static Glyph> {
    let mut i = 0usize;
    while i < FONT.len() {
        if let Some(g) = FONT.get(i) {
            if g.ch == ch {
                return Some(g);
            }
        }
        i += 1;
    }
    None
}

/// 三参数水印规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WmParams {
    /// 旋转角（度，逆时针为正）。
    pub angle: i16,
    /// 不透明度（百分比 0..=100）。
    pub opacity: u8,
    /// 字号（像素，指字形总高）。
    pub size: u8,
    /// 文本（仅字形表内字符）。
    pub text: &'static str,
}

impl WmParams {
    /// 规格校验：三类参数 + 文本三类，全部单点判定。
    pub fn validate(&self) -> Result<(), MarkError> {
        if self.angle < -180 || self.angle > 180 {
            return Err(MarkError::Angle);
        }
        if self.opacity > 100 {
            return Err(MarkError::Opacity);
        }
        if self.size < 6 || self.size > 96 {
            return Err(MarkError::Size);
        }
        if self.text.is_empty() {
            return Err(MarkError::EmptyText);
        }
        if self.text.len() > TEXT_CAP {
            return Err(MarkError::TextLong);
        }
        let b = self.text.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            match b.get(i) {
                Some(c) => {
                    if glyph_of(*c).is_none() {
                        return Err(MarkError::Glyph);
                    }
                }
                None => return Err(MarkError::Glyph),
            }
            i += 1;
        }
        Ok(())
    }

    /// 规格文本（`a=30,o=40,s=28,DRAFT`），与 [`parse_spec`] 互逆。
    pub fn to_spec(&self) -> String {
        format!("a={},o={},s={},{}", self.angle, self.opacity, self.size, self.text)
    }

    /// 三元组键（用于查重）。
    pub fn key(&self) -> (i16, u8, u8) {
        (self.angle, self.opacity, self.size)
    }
}

/// 解析规格文本。字段缺失 / 非数字 / 越界分别落到不同错误码。
pub fn parse_spec(s: &'static str) -> Result<WmParams, MarkError> {
    let mut angle: Option<i16> = None;
    let mut opacity: Option<u8> = None;
    let mut size: Option<u8> = None;
    let mut text: Option<&'static str> = None;
    for part in s.split(',') {
        let p = part.trim();
        if let Some(rest) = p.strip_prefix("a=") {
            angle = Some(parse_i16(rest)?);
        } else if let Some(rest) = p.strip_prefix("o=") {
            opacity = Some(parse_u8(rest)?);
        } else if let Some(rest) = p.strip_prefix("s=") {
            size = Some(parse_u8(rest)?);
        } else if !p.is_empty() {
            text = Some(p);
        }
    }
    let p = WmParams {
        angle: angle.ok_or(MarkError::Angle)?,
        opacity: opacity.ok_or(MarkError::Opacity)?,
        size: size.ok_or(MarkError::Size)?,
        text: text.ok_or(MarkError::EmptyText)?,
    };
    p.validate()?;
    Ok(p)
}

fn parse_i16(s: &str) -> Result<i16, MarkError> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 4 {
        return Err(MarkError::Angle);
    }
    let neg = b.first() == Some(&b'-');
    let start = if neg { 1 } else { 0 };
    if start >= b.len() {
        return Err(MarkError::Angle);
    }
    let mut v: i32 = 0;
    let mut i = start;
    while i < b.len() {
        match b.get(i) {
            Some(c) => {
                if !c.is_ascii_digit() {
                    return Err(MarkError::Angle);
                }
                v = v * 10 + (*c - b'0') as i32;
            }
            None => return Err(MarkError::Angle),
        }
        i += 1;
    }
    if neg {
        v = -v;
    }
    if v < i16::MIN as i32 || v > i16::MAX as i32 {
        return Err(MarkError::Angle);
    }
    Ok(v as i16)
}

fn parse_u8(s: &str) -> Result<u8, MarkError> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 3 {
        return Err(MarkError::Opacity);
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(c) => {
                if !c.is_ascii_digit() {
                    return Err(MarkError::Opacity);
                }
                v = v * 10 + (*c - b'0') as u32;
            }
            None => return Err(MarkError::Opacity),
        }
        i += 1;
    }
    if v > u8::MAX as u32 {
        return Err(MarkError::Opacity);
    }
    Ok(v as u8)
}

/// 30 个组合：角度 × 透明度 × 字号 的确定性笛卡尔积，id 由下标隐式给出。
pub fn combos() -> Vec<WmParams> {
    let mut out: Vec<WmParams> = Vec::with_capacity(COMBO_COUNT);
    let mut ai = 0usize;
    while ai < ANGLES.len() {
        let mut oi = 0usize;
        while oi < OPACITIES.len() {
            let mut si = 0usize;
            while si < SIZES.len() {
                if let (Some(a), Some(o), Some(s)) = (ANGLES.get(ai), OPACITIES.get(oi), SIZES.get(si)) {
                    out.push(WmParams {
                        angle: *a,
                        opacity: *o,
                        size: *s,
                        text: COMBO_TEXT,
                    });
                }
                si += 1;
            }
            oi += 1;
        }
        ai += 1;
    }
    out
}

/// 画布：确定性背景（8×8 棋盘，两档灰度 240 / 232）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canvas {
    /// 宽（像素）。
    pub w: u16,
    /// 高（像素）。
    pub h: u16,
    /// 背景字节（行主序，长度 = w×h）。
    pub bg: Vec<u8>,
}

impl Canvas {
    /// 建画布；任一维为 0 即拒。
    pub fn checker(w: u16, h: u16) -> Result<Canvas, MarkError> {
        if w == 0 || h == 0 {
            return Err(MarkError::Canvas);
        }
        let n = w as usize * h as usize;
        let mut bg: Vec<u8> = Vec::with_capacity(n);
        let mut y = 0usize;
        while y < h as usize {
            let mut x = 0usize;
            while x < w as usize {
                let cell = (x / 8 + y / 8) % 2;
                bg.push(if cell == 0 { 240 } else { 232 });
                x += 1;
            }
            y += 1;
        }
        Ok(Canvas { w, h, bg })
    }

    /// 像素总数。
    pub fn len(&self) -> usize {
        self.bg.len()
    }
}

/// 渲染结果（含可审计的中间量）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raster {
    /// 宽。
    pub w: u16,
    /// 高。
    pub h: u16,
    /// 输出像素（行主序）。
    pub px: Vec<u8>,
    /// 墨量 = Σ(背景 − 输出)，即「压暗了多少」。
    pub ink: u32,
    /// 被压暗的像素数。
    pub changed: u32,
    /// 输出摘要（覆盖参数 + 像素）。
    pub digest: u32,
}

fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(0x0100_0193)
}

fn mix32(h: u32, v: u32) -> u32 {
    let a = (h ^ (v & 0xffff)).wrapping_mul(0x0100_0193);
    (a ^ (v >> 16)).wrapping_mul(0x0100_0193)
}

/// 主渲染入口：把 `text` 按 `angle/opacity/size` 旋转铺到画布上。
///
/// 关键约定：
/// * 字号 = 字形总高（7 行），所以 `scale = size / 7`；
/// * 旋转围绕画布中心，中心取 `(w-1)/2`（像素中心坐标 0.5 的对称轴）；
/// * 每像素 2×2 超采样，覆盖度只有 0/1/2/3/4 五档，混色全整数。
pub fn render(canvas: &Canvas, p: &WmParams) -> Result<Raster, MarkError> {
    p.validate()?;
    if canvas.w == 0 || canvas.h == 0 || canvas.len() != canvas.w as usize * canvas.h as usize {
        return Err(MarkError::Canvas);
    }
    let theta = p.angle as f32 * 3.14159265_f32 / 180.0;
    let c = cosf(theta);
    let s = sinf(theta);
    let scale = p.size as f32 / GLYPH_ROWS as f32;
    let advance = ADVANCE_COLS * scale;
    let n_chars = p.text.len();
    let tw = n_chars as f32 * advance - scale;
    let th = GLYPH_ROWS as f32 * scale;
    let cx = (canvas.w as f32 - 1.0) * 0.5;
    let cy = (canvas.h as f32 - 1.0) * 0.5;
    let w = canvas.w as usize;
    let h = canvas.h as usize;
    let mut px: Vec<u8> = Vec::with_capacity(w * h);
    let mut ink: u32 = 0;
    let mut changed: u32 = 0;
    let mut y = 0usize;
    while y < h {
        let mut x = 0usize;
        while x < w {
            let idx = y * w + x;
            let bg = match canvas.bg.get(idx) {
                Some(v) => *v,
                None => 240,
            };
            let mut hits = 0u32;
            let mut sy = 0usize;
            while sy < 2 {
                let mut sx = 0usize;
                while sx < 2 {
                    // 子样本偏移 0.25 / 0.75：四个样本对像素中心对称。
                    let ox = if sx == 0 { 0.25 } else { 0.75 };
                    let oy = if sy == 0 { 0.25 } else { 0.75 };
                    let dx = x as f32 + ox - cx;
                    let dy = y as f32 + oy - cy;
                    // 逆旋转到文本坐标系，再平移到文本左上角。
                    let u = dx * c + dy * s + tw * 0.5;
                    let v = -dx * s + dy * c + th * 0.5;
                    if u >= 0.0 && v >= 0.0 && u < tw && v < th {
                        if let Some(hit) = sample_glyph(p.text, u, v, scale, advance) {
                            if hit {
                                hits += 1;
                            }
                        }
                    }
                    sx += 1;
                }
                sy += 1;
            }
            // 整数混色：alpha = opacity/100 × hits/4；压深 = bg × opacity × hits / 400。
            let alpha_num = p.opacity as u32 * hits;
            let dark = (bg as u32 * alpha_num) / 400;
            let out = if dark >= bg as u32 { 0 } else { bg - dark as u8 };
            if dark > 0 {
                ink = ink.saturating_add(dark);
                changed = changed.saturating_add(1);
            }
            px.push(out);
            x += 1;
        }
        y += 1;
    }
    let mut digest = 0x811c_9dc5u32;
    digest = mix32(digest, p.angle as i32 as u32);
    digest = mix32(digest, p.opacity as u32);
    digest = mix32(digest, p.size as u32);
    digest = mix32(digest, canvas.w as u32);
    digest = mix32(digest, canvas.h as u32);
    let tb = p.text.as_bytes();
    let mut ti = 0usize;
    while ti < tb.len() {
        if let Some(ch) = tb.get(ti) {
            digest = fnv_byte(digest, *ch);
        }
        ti += 1;
    }
    let mut i = 0usize;
    while i < px.len() {
        if let Some(v) = px.get(i) {
            digest = fnv_byte(digest, *v);
        }
        i += 1;
    }
    Ok(Raster {
        w: canvas.w,
        h: canvas.h,
        px,
        ink,
        changed,
        digest,
    })
}

/// 文本坐标系采样：返回 `Some(true/false)` 表示落在字形格内（有/无墨），
/// 落在字间间距或文本外返回 `None`。
fn sample_glyph(text: &'static str, u: f32, v: f32, scale: f32, advance: f32) -> Option<bool> {
    let ci = (u / advance) as usize;
    if ci >= text.len() {
        return None;
    }
    let gx = u - ci as f32 * advance;
    if gx >= GLYPH_COLS as f32 * scale {
        return None;
    }
    let col = (gx / scale) as usize;
    let row = (v / scale) as usize;
    if col >= GLYPH_COLS || row >= GLYPH_ROWS {
        return None;
    }
    let ch = match text.as_bytes().get(ci) {
        Some(c) => *c,
        None => return None,
    };
    let g = match glyph_of(ch) {
        Some(v) => v,
        None => return None,
    };
    let row_bits = match g.rows.get(row) {
        Some(v) => *v,
        None => return None,
    };
    let shift = GLYPH_COLS - 1 - col;
    Some((row_bits >> shift) & 1 == 1)
}

/// 渲染结果里的改动像素包围盒 (x0, y0, x1, y1)；无改动时返回 None。
pub fn changed_bbox(canvas: &Canvas, r: &Raster) -> Option<(u16, u16, u16, u16)> {
    let mut x0 = u16::MAX;
    let mut y0 = u16::MAX;
    let mut x1 = 0u16;
    let mut y1 = 0u16;
    let mut found = false;
    let w = canvas.w as usize;
    let h = canvas.h as usize;
    let mut y = 0usize;
    while y < h {
        let mut x = 0usize;
        while x < w {
            let idx = y * w + x;
            let bg = canvas.bg.get(idx).copied().unwrap_or(0);
            let out = r.px.get(idx).copied().unwrap_or(bg);
            if out != bg {
                found = true;
                if (x as u16) < x0 {
                    x0 = x as u16;
                }
                if (y as u16) < y0 {
                    y0 = y as u16;
                }
                if x as u16 > x1 {
                    x1 = x as u16;
                }
                if y as u16 > y1 {
                    y1 = y as u16;
                }
            }
            x += 1;
        }
        y += 1;
    }
    if found {
        Some((x0, y0, x1, y1))
    } else {
        None
    }
}

/// 账本条目：参数 + 渲染指纹（墨量 / 改动数 / 摘要）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WmEntry {
    /// 参数。
    pub params: WmParams,
    /// 墨量。
    pub ink: u32,
    /// 改动像素数。
    pub changed: u32,
    /// 摘要。
    pub digest: u32,
}

impl WmEntry {
    /// 由一次渲染封装条目。
    pub fn seal(params: WmParams, r: &Raster) -> WmEntry {
        WmEntry {
            params,
            ink: r.ink,
            changed: r.changed,
            digest: r.digest,
        }
    }

    /// 一行台账。
    pub fn ledger_line(&self) -> String {
        format!(
            "a={} o={} s={} {} ink={} chg={} dg={:08x}",
            self.params.angle,
            self.params.opacity,
            self.params.size,
            self.params.text,
            self.ink,
            self.changed,
            self.digest
        )
    }
}

/// 水印组合账本。
pub struct WmBook {
    entries: Vec<WmEntry>,
    capacity: usize,
    rejected: u32,
}

impl WmBook {
    /// 建空账。
    pub fn new(capacity: usize) -> WmBook {
        let cap = if capacity == 0 { 1 } else { capacity };
        WmBook {
            entries: Vec::with_capacity(cap),
            capacity: cap,
            rejected: 0,
        }
    }

    /// 默认容量账本。
    pub fn with_default_capacity() -> WmBook {
        WmBook::new(MAX_ENTRIES)
    }

    fn reject<T>(&mut self, e: MarkError) -> Result<T, MarkError> {
        self.rejected = self.rejected.saturating_add(1);
        Err(e)
    }

    fn find(&self, key: (i16, u8, u8)) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.params.key() == key {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 累计被拒次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 按参数取条目。
    pub fn entry(&self, key: (i16, u8, u8)) -> Option<&WmEntry> {
        match self.find(key) {
            Some(i) => self.entries.get(i),
            None => None,
        }
    }

    /// 严格入账：重渲染并比对摘要/墨量/改动数，三处全中才收。
    pub fn admit(&mut self, e: WmEntry, canvas: &Canvas) -> Result<(), MarkError> {
        if self.entries.len() >= self.capacity {
            return self.reject(MarkError::Full);
        }
        if self.find(e.params.key()).is_some() {
            return self.reject(MarkError::Duplicate);
        }
        let r = match render(canvas, &e.params) {
            Ok(v) => v,
            Err(err) => return self.reject(err),
        };
        if r.digest != e.digest || r.ink != e.ink || r.changed != e.changed {
            return self.reject(MarkError::Digest);
        }
        self.entries.push(WmEntry { digest: r.digest, ink: r.ink, changed: r.changed, params: e.params });
        Ok(())
    }

    /// 非信任导入：只查结构（空 / 重号 / 超容），内容真伪交给 `verify_all`。
    pub fn import(&mut self, items: &[WmEntry]) -> Result<usize, MarkError> {
        if items.is_empty() {
            return self.reject(MarkError::Missing);
        }
        let mut i = 0usize;
        while i < items.len() {
            let e = match items.get(i) {
                Some(v) => *v,
                None => return self.reject(MarkError::Missing),
            };
            if self.entries.len() >= self.capacity {
                return self.reject(MarkError::Full);
            }
            if self.find(e.params.key()).is_some() {
                return self.reject(MarkError::Duplicate);
            }
            self.entries.push(e);
            i += 1;
        }
        Ok(self.entries.len())
    }

    /// 全量复核：逐条重渲染并比对，任何一条不一致即 `Digest`。
    pub fn verify_all(&self, canvas: &Canvas) -> Result<usize, MarkError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                let r = render(canvas, &e.params)?;
                if r.digest != e.digest || r.ink != e.ink || r.changed != e.changed {
                    return Err(MarkError::Digest);
                }
            }
            i += 1;
        }
        Ok(self.entries.len())
    }

    /// 批摘要：按入账序折叠 (key, digest, ink)。
    pub fn root(&self) -> u32 {
        let mut h = 0x811c_9dc5u32 ^ 0x0000_4314;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = mix32(h, e.params.angle as i32 as u32);
                h = mix32(h, e.params.opacity as u32);
                h = mix32(h, e.params.size as u32);
                h = mix32(h, e.digest);
                h = mix32(h, e.ink);
            }
            i += 1;
        }
        h
    }

    /// 墨量合计。
    pub fn total_ink(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                n = n.saturating_add(e.ink);
            }
            i += 1;
        }
        n
    }

    /// 收口：必须 30 条且全量复核通过。
    pub fn close(&self, canvas: &Canvas) -> Result<CloseStamp, MarkError> {
        if self.entries.len() != COMBO_COUNT {
            return Err(MarkError::Missing);
        }
        self.verify_all(canvas)?;
        Ok(CloseStamp {
            count: self.entries.len(),
            ink_total: self.total_ink(),
            root: self.root(),
        })
    }

    /// 全表台账文本。
    pub fn ledger_text(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                out.push_str(&e.ledger_line());
                out.push('\n');
            }
            i += 1;
        }
        out
    }
}

/// 收口印。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloseStamp {
    /// 条目数（应为 30）。
    pub count: usize,
    /// 墨量合计。
    pub ink_total: u32,
    /// 批摘要。
    pub root: u32,
}

/// 按 30 组合建账（严格入账）。
pub fn build_book(canvas: &Canvas) -> WmBook {
    let mut b = WmBook::with_default_capacity();
    let list = combos();
    let mut i = 0usize;
    while i < list.len() {
        if let Some(p) = list.get(i) {
            if let Ok(r) = render(canvas, p) {
                let _ = b.admit(WmEntry::seal(*p, &r), canvas);
            }
        }
        i += 1;
    }
    b
}

/// 注入核：8 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;
    let canvas = match Canvas::checker(128, 96) {
        Ok(c) => c,
        Err(_) => return (0, total),
    };
    let base = WmParams { angle: 0, opacity: 40, size: 28, text: "DRAFT" };

    // 1) 角度越界。
    if render(&canvas, &WmParams { angle: 181, ..base }).err() == Some(MarkError::Angle) {
        caught += 1;
    }
    // 2) 不透明度越界。
    if render(&canvas, &WmParams { opacity: 101, ..base }).err() == Some(MarkError::Opacity) {
        caught += 1;
    }
    // 3) 字号越界（下界 5 / 上界 97 都拒）。
    let lo = render(&canvas, &WmParams { size: 5, ..base }).err() == Some(MarkError::Size);
    let hi = render(&canvas, &WmParams { size: 97, ..base }).err() == Some(MarkError::Size);
    if lo && hi {
        caught += 1;
    }
    // 4) 空文本。
    if render(&canvas, &WmParams { text: "", ..base }).err() == Some(MarkError::EmptyText) {
        caught += 1;
    }
    // 5) 字形表外字符（小写不在表里）。
    if render(&canvas, &WmParams { text: "draft", ..base }).err() == Some(MarkError::Glyph) {
        caught += 1;
    }
    // 6) 文本超长（25 字节）。
    if render(&canvas, &WmParams { text: "AAAAAAAAAAAAAAAAAAAAAAAAA", ..base }).err() == Some(MarkError::TextLong) {
        caught += 1;
    }
    // 7) 零尺寸画布。
    if Canvas::checker(0, 96).err() == Some(MarkError::Canvas) {
        caught += 1;
    }
    // 8) 入账时摘要被篡改。
    if let Ok(r) = render(&canvas, &base) {
        let mut e = WmEntry::seal(base, &r);
        e.digest ^= 1;
        let mut b = WmBook::with_default_capacity();
        if b.admit(e, &canvas).err() == Some(MarkError::Digest) {
            caught += 1;
        }
    }

    (caught, total)
}

/// UNX-F50041 自检：16 条判据。
pub fn run_unx50041_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50041");
    let canvas = match Canvas::checker(128, 96) {
        Ok(c) => c,
        Err(_) => Canvas { w: 1, h: 1, bg: Vec::new() },
    };

    // J1-1 组合闭集：30 条，三参数各取 5/3/2 档，三元组互不重复。
    let list = combos();
    let mut unique = true;
    let mut i = 0usize;
    while i < list.len() {
        if let Some(a) = list.get(i) {
            let mut j = i + 1;
            while j < list.len() {
                if let Some(b) = list.get(j) {
                    if a.key() == b.key() {
                        unique = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    let ok = list.len() == COMBO_COUNT && unique && list.iter().all(|p| p.validate().is_ok());
    let det = format!("combos={} unique={}", list.len(), unique);
    s.add_dyn("50041-J1-组合闭集", ok, &det);

    // J1-2 30 组合逐项渲染：每条都产出非空墨量，像素数与画布一致。
    let mut rendered: Vec<Raster> = Vec::with_capacity(COMBO_COUNT);
    let mut shape_ok = list.len() == COMBO_COUNT;
    let mut k = 0usize;
    while k < list.len() {
        if let Some(p) = list.get(k) {
            match render(&canvas, p) {
                Ok(r) => {
                    if r.px.len() != canvas.len() || r.ink == 0 || r.changed == 0 || r.w != canvas.w {
                        shape_ok = false;
                    }
                    rendered.push(r);
                }
                Err(_) => shape_ok = false,
            }
        }
        k += 1;
    }
    let det = format!("rendered={}", rendered.len());
    s.add_dyn("50041-J1-逐项渲染", shape_ok, &det);

    // J1-3 渲染确定性：同参数两遍像素逐位相等、摘要相同。
    let probe = WmParams { angle: 30, opacity: 40, size: 28, text: "DRAFT" };
    let a = render(&canvas, &probe);
    let b = render(&canvas, &probe);
    let ok = match (a, b) {
        (Ok(x), Ok(y)) => x.digest == y.digest && x.px == y.px && x.ink == y.ink,
        _ => false,
    };
    s.add("50041-J1-渲染确定", ok, if ok { "" } else { "两遍渲染不一致" });

    // J1-4 30 组合摘要互异（任一参数不同必换摘要）。
    let mut distinct = true;
    let mut p = 0usize;
    while p < rendered.len() {
        let mut q = p + 1;
        while q < rendered.len() {
            match (rendered.get(p), rendered.get(q)) {
                (Some(x), Some(y)) => {
                    if x.digest == y.digest {
                        distinct = false;
                    }
                }
                _ => distinct = false,
            }
            q += 1;
        }
        p += 1;
    }
    s.add("50041-J1-摘要互异", distinct, if distinct { "" } else { "不同参数撞了摘要" });

    // J1-5 透明度 0 恒等背景（逐像素相等，墨量为 0）。
    let zero = WmParams { angle: 22, opacity: 0, size: 28, text: "DRAFT" };
    let ok = match render(&canvas, &zero) {
        Ok(r) => r.ink == 0 && r.changed == 0 && r.px == canvas.bg,
        Err(_) => false,
    };
    s.add("50041-J1-零透明恒等", ok, if ok { "" } else { "透明度 0 仍改了像素" });

    // J1-6 墨量随透明度单调不减（逐角度 × 字号验证，且全程严格有变化）。
    let mut mono = true;
    let mut ai = 0usize;
    while ai < ANGLES.len() {
        let mut si = 0usize;
        while si < SIZES.len() {
            let mut prev = 0u32;
            let mut oi = 0usize;
            while oi < OPACITIES.len() {
                if let (Some(a0), Some(o0), Some(s0)) = (ANGLES.get(ai), OPACITIES.get(oi), SIZES.get(si)) {
                    let p0 = WmParams { angle: *a0, opacity: *o0, size: *s0, text: COMBO_TEXT };
                    match render(&canvas, &p0) {
                        Ok(r) => {
                            if r.ink < prev {
                                mono = false;
                            }
                            prev = r.ink;
                        }
                        Err(_) => mono = false,
                    }
                }
                oi += 1;
            }
            if prev == 0 {
                mono = false;
            }
            si += 1;
        }
        ai += 1;
    }
    s.add("50041-J1-透明度单调", mono, if mono { "" } else { "墨量随透明度回退" });

    // J1-7 墨量随字号递增：28 px 的墨量必大于 10 px（同角度/透明度）。
    let mut size_ok = true;
    let mut ai = 0usize;
    while ai < ANGLES.len() {
        let mut oi = 0usize;
        while oi < OPACITIES.len() {
            if let (Some(a0), Some(o0), Some(sm), Some(sg)) = (ANGLES.get(ai), OPACITIES.get(oi), SIZES.get(0), SIZES.get(1)) {
                let small = WmParams { angle: *a0, opacity: *o0, size: *sm, text: COMBO_TEXT };
                let big = WmParams { angle: *a0, opacity: *o0, size: *sg, text: COMBO_TEXT };
                match (render(&canvas, &small), render(&canvas, &big)) {
                    (Ok(x), Ok(y)) => {
                        if y.ink <= x.ink || y.changed <= x.changed {
                            size_ok = false;
                        }
                    }
                    _ => size_ok = false,
                }
            }
            oi += 1;
        }
        ai += 1;
    }
    s.add("50041-J1-字号递增", size_ok, if size_ok { "" } else { "大字墨量未增长" });

    // J1-8 旋转语义：0° 包围盒宽高比 > 2；45° 包围盒显著变高。
    let flat = WmParams { angle: 0, opacity: 75, size: 28, text: COMBO_TEXT };
    let tilt = WmParams { angle: 45, opacity: 75, size: 28, text: COMBO_TEXT };
    let (flat_box, tilt_box) = (
        render(&canvas, &flat).ok().and_then(|r| changed_bbox(&canvas, &r)),
        render(&canvas, &tilt).ok().and_then(|r| changed_bbox(&canvas, &r)),
    );
    let ok = match (flat_box, tilt_box) {
        (Some(f), Some(t)) => {
            let fw = f.2 as i32 - f.0 as i32 + 1;
            let fh = f.3 as i32 - f.1 as i32 + 1;
            let th = t.3 as i32 - t.1 as i32 + 1;
            fh > 0 && fw > 2 * fh && th > 2 * fh
        }
        _ => false,
    };
    let det = match (flat_box, tilt_box) {
        (Some(f), Some(t)) => format!("flat_h={} tilt_h={}", f.3 - f.1 + 1, t.3 - t.1 + 1),
        _ => "bbox missing".to_string(),
    };
    s.add_dyn("50041-J1-旋转包围盒", ok, &det);

    // J1-9 30 组合账本：30/30 严格入账，无被拒。
    let book = build_book(&canvas);
    let ok = book.len() == COMBO_COUNT && book.rejected() == 0 && book.total_ink() > 0;
    let det = format!("len={} rej={} ink={}", book.len(), book.rejected(), book.total_ink());
    s.add_dyn("50041-J1-三十条入账", ok, &det);

    // J1-10 全量复核：30 条重渲染逐项一致。
    let verified = book.verify_all(&canvas);
    let ok = verified.as_ref().ok() == Some(&COMBO_COUNT);
    let det = match verified {
        Ok(n) => format!("verified={}", n),
        Err(e) => format!("verify err={:#x}", e.code()),
    };
    s.add_dyn("50041-J1-全量复核", ok, &det);

    // J1-11 收口：30 条 + 复核通过才出印，印面条数/墨量/根摘要齐备。
    let closed = book.close(&canvas);
    let (ok, det) = match closed {
        Ok(st) => (st.count == COMBO_COUNT && st.ink_total > 0 && st.root != 0, format!("30/30 ink={} root={:08x}", st.ink_total, st.root)),
        Err(e) => (false, format!("close err={:#x}", e.code())),
    };
    s.add_dyn("50041-J1-收口印", ok, &det);

    // J1-12 篡改必拒：导入一条摘要被改的记录，全量复核抓 Digest。
    let mut tamper_ok = false;
    if let Ok(r) = render(&canvas, &probe) {
        let mut e = WmEntry::seal(probe, &r);
        e.digest ^= 0x0100_0000;
        let mut b = WmBook::with_default_capacity();
        let imported = b.import(&[e]).is_ok();
        tamper_ok = imported && b.verify_all(&canvas).err() == Some(MarkError::Digest) && b.len() == 1;
    }
    s.add("50041-J1-篡改必拒", tamper_ok, if tamper_ok { "" } else { "被改记录通过了复核" });

    // J1-13 规格解析往返：解析后重新序列化与原串一致；越界字段各落各码。
    let spec = "a=-30,o=75,s=28,DRAFT";
    let parsed = parse_spec(spec);
    let round = match parsed {
        Ok(p) => p.to_spec() == spec.to_string() && p.validate().is_ok(),
        Err(_) => false,
    };
    let bad_angle = parse_spec("a=400,o=40,s=28,DRAFT").err() == Some(MarkError::Angle);
    let bad_op = parse_spec("a=0,o=140,s=28,DRAFT").err() == Some(MarkError::Opacity);
    let bad_size = parse_spec("a=0,o=40,s=4,DRAFT").err() == Some(MarkError::Size);
    let bad_text = parse_spec("a=0,o=40,s=28,dr@ft").err() == Some(MarkError::Glyph);
    let ok = round && bad_angle && bad_op && bad_size && bad_text;
    s.add("50041-J1-规格往返", ok, if ok { "" } else { "规格解析/往返不符" });

    // J1-14 容量与重号防线：容量 2 的账本第 3 条 Full；同三元组 Duplicate。
    let mut cap_ok = false;
    if let Ok(r1) = render(&canvas, &probe) {
        let p2 = WmParams { opacity: 15, ..probe };
        let p3 = WmParams { size: 10, ..probe };
        if let Ok(r2) = render(&canvas, &p2) {
            if let Ok(r3) = render(&canvas, &p3) {
                let mut small = WmBook::new(2);
                let first = small.admit(WmEntry::seal(probe, &r1), &canvas).is_ok();
                let dup = small.admit(WmEntry::seal(probe, &r1), &canvas).err() == Some(MarkError::Duplicate);
                let second = small.admit(WmEntry::seal(p2, &r2), &canvas).is_ok();
                let full = small.admit(WmEntry::seal(p3, &r3), &canvas).err() == Some(MarkError::Full);
                cap_ok = first && dup && second && full && small.len() == 2 && small.rejected() == 2;
            }
        }
    }
    s.add("50041-J1-容量重号", cap_ok, if cap_ok { "" } else { "容量/重号防线失效" });

    // J1-15 收口门槛：19 条不许收口（Missing），补满 30 条才放行。
    let mut gate_ok = false;
    let mut partial = WmBook::with_default_capacity();
    let mut n = 0usize;
    while n < 19 {
        if let Some(p0) = list.get(n) {
            if let Ok(r) = render(&canvas, p0) {
                let _ = partial.admit(WmEntry::seal(*p0, &r), &canvas);
            }
        }
        n += 1;
    }
    if partial.len() == 19 {
        let blocked = partial.close(&canvas).err() == Some(MarkError::Missing);
        while n < list.len() {
            if let Some(p0) = list.get(n) {
                if let Ok(r) = render(&canvas, p0) {
                    let _ = partial.admit(WmEntry::seal(*p0, &r), &canvas);
                }
            }
            n += 1;
        }
        gate_ok = blocked && partial.close(&canvas).is_ok();
    }
    s.add("50041-J1-收口门槛", gate_ok, if gate_ok { "" } else { "未满 30 条也出了印" });

    // J1-16 八类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50041-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50041_checks_green() {
        let set = run_unx50041_checks();
        assert!(set.all_passed(), "UNX-F50041 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50041_thirty_combos_verify() {
        let c = match Canvas::checker(128, 96) {
            Ok(v) => v,
            Err(_) => panic!("canvas failed"),
        };
        let b = build_book(&c);
        assert_eq!(b.len(), COMBO_COUNT);
        assert_eq!(b.verify_all(&c).ok(), Some(COMBO_COUNT));
        let st = match b.close(&c) {
            Ok(v) => v,
            Err(e) => panic!("close failed: {:#x}", e.code()),
        };
        assert_eq!(st.count, COMBO_COUNT);
        assert!(st.ink_total > 0);
    }

    #[test]
    fn unx50041_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 8);
    }
}
