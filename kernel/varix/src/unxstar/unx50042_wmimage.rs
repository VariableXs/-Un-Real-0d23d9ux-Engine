//! UNX-F50042 图像水印（透明 PNG 合成与缩放定位）（0x43160 段）
//!
//! 锚点：UNX-F50042-J1 合成 ×20 定位偏差 ≤1px；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段 `SEG_BASE = 0x43160`：诊断码闭集 13 条（1..=13）。每条码都在注入核
//! 或判据里被真实构造，不存在只声明不触发的装饰码。
//!
//! 结构清单：
//! * `PngImage` 解析器：PNG 分块（IHDR/IDAT/IEND/未知关键块）+ CRC32 + zlib
//!   STORED 块解压 + 五种行滤波复原；
//! * `Canvas`/`Rect`：RGBA8 画布与整数落位矩形；
//! * `Placement`/`Anchor`：九宫锚点 + 缩放 + 页边距的落位规划器；
//! * `Composited`/`CompositeLedger`：合成结果账本（矩形/像素数/平均 alpha/摘要）；
//! * 注入核：十一类坏样本（空/签名/截断/CRC/未知关键块/调色板/16bit/隔行/
//!   压缩块非 STORED/Adler 错/坏滤波码）逐类断言具体诊断码。
//!
//! 为什么只解 STORED 块：水印图由本模块编码器产出（见 `encode_png`），deflate
//! 的固定/动态 Huffman 需要数 KiB 码表并显著膨胀内核代码段；水印图尺寸受
//! `MAX_DIM` 约束，未压缩流体积可接受。遇到压缩块**显式拒绝**而不是猜解——
//! 把错误像素合成到正文上，比拒绝一张图危险得多。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::{ceilf, fabsf, floorf};

/// 诊断段（0x43160 独占 0x20）。
pub const SEG_BASE: u32 = 0x43160;

/// 单边最大像素数。水印图不会是一张整页照片；超过即视为配置错误。
const MAX_DIM: u32 = 4096;
/// 解压后原始流上限（32 MiB），防「压缩炸弹」式膨胀。
const MAX_RAW: usize = 32 * 1024 * 1024;

pub const E_EMPTY: u16 = 1;
pub const E_SIGNATURE: u16 = 2;
pub const E_TRUNCATED: u16 = 3;
pub const E_CRC: u16 = 4;
pub const E_CHUNK: u16 = 5;
pub const E_COLOR_TYPE: u16 = 6;
pub const E_BIT_DEPTH: u16 = 7;
pub const E_INTERLACE: u16 = 8;
pub const E_ZLIB: u16 = 9;
pub const E_ADLER: u16 = 10;
pub const E_FILTER: u16 = 11;
pub const E_CANVAS: u16 = 12;
pub const E_PLACE: u16 = 13;

/// 图像水印域错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WmError {
    Empty,
    Signature,
    Truncated,
    Crc,
    Chunk,
    ColorType,
    BitDepth,
    Interlace,
    Zlib,
    Adler,
    Filter,
    Canvas,
    Place,
}

impl WmError {
    pub fn code(self) -> u32 {
        let n = match self {
            WmError::Empty => E_EMPTY,
            WmError::Signature => E_SIGNATURE,
            WmError::Truncated => E_TRUNCATED,
            WmError::Crc => E_CRC,
            WmError::Chunk => E_CHUNK,
            WmError::ColorType => E_COLOR_TYPE,
            WmError::BitDepth => E_BIT_DEPTH,
            WmError::Interlace => E_INTERLACE,
            WmError::Zlib => E_ZLIB,
            WmError::Adler => E_ADLER,
            WmError::Filter => E_FILTER,
            WmError::Canvas => E_CANVAS,
            WmError::Place => E_PLACE,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            WmError::Empty => "empty png buffer",
            WmError::Signature => "png signature mismatch",
            WmError::Truncated => "chunk or stream truncated",
            WmError::Crc => "chunk crc mismatch",
            WmError::Chunk => "chunk order or critical chunk invalid",
            WmError::ColorType => "color type not rgba/rgb8",
            WmError::BitDepth => "bit depth is not 8",
            WmError::Interlace => "interlaced png unsupported",
            WmError::Zlib => "zlib stream is not stored-block",
            WmError::Adler => "zlib adler32 mismatch",
            WmError::Filter => "unknown scanline filter",
            WmError::Canvas => "canvas size or rgb length invalid",
            WmError::Place => "placement outside canvas",
        }
    }
}

/// 整数像素矩形（右下开区间由 `x + w` / `y + h` 表达）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: u32, y: u32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn area(&self) -> u32 {
        self.w * self.h
    }

    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// 解码后的 RGBA8 图像。`channels` 记录源文件的通道数（3 或 4），
/// 像素缓冲统一升格成 RGBA，合成路径只认一种布局。
#[derive(Clone, Debug, PartialEq)]
pub struct PngImage {
    pub w: u32,
    pub h: u32,
    pub channels: u8,
    pub pixels: Vec<u8>,
}

impl PngImage {
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.w || y >= self.h {
            return None;
        }
        let i = (y as usize * self.w as usize + x as usize) * 4;
        match (
            self.pixels.get(i),
            self.pixels.get(i + 1),
            self.pixels.get(i + 2),
            self.pixels.get(i + 3),
        ) {
            (Some(a), Some(b), Some(c), Some(d)) => Some([*a, *b, *c, *d]),
            _ => None,
        }
    }
}

/// RGBA8 合成画布（行主序，非预乘）。
#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Canvas {
    /// 新建全透明画布；零尺寸或超限返回 `WmError::Canvas`。
    pub fn new(w: u32, h: u32) -> Result<Canvas, WmError> {
        if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
            return Err(WmError::Canvas);
        }
        let n = w as usize * h as usize * 4;
        let mut px: Vec<u8> = Vec::new();
        px.resize(n, 0u8);
        Ok(Canvas { w, h, px })
    }

    pub fn get(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.w || y >= self.h {
            return None;
        }
        let i = (y as usize * self.w as usize + x as usize) * 4;
        match (
            self.px.get(i),
            self.px.get(i + 1),
            self.px.get(i + 2),
            self.px.get(i + 3),
        ) {
            (Some(a), Some(b), Some(c), Some(d)) => Some([*a, *b, *c, *d]),
            _ => None,
        }
    }

    /// 写一个像素，越界返回 `false`（不静默截断）。
    pub fn put(&mut self, x: u32, y: u32, c: [u8; 4]) -> bool {
        if x >= self.w || y >= self.h {
            return false;
        }
        let i = (y as usize * self.w as usize + x as usize) * 4;
        if i + 3 >= self.px.len() {
            return false;
        }
        self.px[i] = c[0];
        self.px[i + 1] = c[1];
        self.px[i + 2] = c[2];
        self.px[i + 3] = c[3];
        true
    }

    /// 不透明覆盖盒（alpha > 0 的最小包围矩形）；全透明返回 `None`。
    pub fn ink_bbox(&self) -> Option<Rect> {
        let mut x0 = self.w;
        let mut y0 = self.h;
        let mut x1 = 0u32;
        let mut y1 = 0u32;
        let mut any = false;
        let mut y = 0u32;
        while y < self.h {
            let mut x = 0u32;
            while x < self.w {
                if let Some(p) = self.get(x, y) {
                    if p[3] > 0 {
                        any = true;
                        if x < x0 {
                            x0 = x;
                        }
                        if y < y0 {
                            y0 = y;
                        }
                        if x + 1 > x1 {
                            x1 = x + 1;
                        }
                        if y + 1 > y1 {
                            y1 = y + 1;
                        }
                    }
                }
                x += 1;
            }
            y += 1;
        }
        if !any {
            return None;
        }
        Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
    }
}

/// 九宫锚点（水印的常规落位集合）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    TopCenter,
    TopRight,
    MidLeft,
    Center,
    MidRight,
    BotLeft,
    BotCenter,
    BotRight,
}

impl Anchor {
    pub fn code(self) -> u8 {
        match self {
            Anchor::TopLeft => 0,
            Anchor::TopCenter => 1,
            Anchor::TopRight => 2,
            Anchor::MidLeft => 3,
            Anchor::Center => 4,
            Anchor::MidRight => 5,
            Anchor::BotLeft => 6,
            Anchor::BotCenter => 7,
            Anchor::BotRight => 8,
        }
    }

    pub fn from_code(c: u8) -> Result<Anchor, WmError> {
        match c {
            0 => Ok(Anchor::TopLeft),
            1 => Ok(Anchor::TopCenter),
            2 => Ok(Anchor::TopRight),
            3 => Ok(Anchor::MidLeft),
            4 => Ok(Anchor::Center),
            5 => Ok(Anchor::MidRight),
            6 => Ok(Anchor::BotLeft),
            7 => Ok(Anchor::BotCenter),
            8 => Ok(Anchor::BotRight),
            _ => Err(WmError::Place),
        }
    }
}

/// 落位参数。`scale` 上限 8.0：水印放大超过 8 倍必然糊成色块，属于配置误用。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub anchor: Anchor,
    pub scale: f32,
    pub margin: u32,
}

impl Placement {
    pub const fn new(anchor: Anchor, scale: f32, margin: u32) -> Placement {
        Placement { anchor, scale, margin }
    }
}

/// 缩放后边长：四舍五入（`floor(x + 0.5)`），下限 1px——0px 的「水印」等于没有。
fn scale_dim(v: u32, s: f32) -> u32 {
    let f = floorf(v as f32 * s + 0.5);
    if f < 1.0 {
        1
    } else if f > MAX_DIM as f32 {
        MAX_DIM
    } else {
        f as u32
    }
}

/// 规划水印落位矩形。整数算术求锚点，居中一律 `(画布 - 图) / 2` 下取整，
/// 保证「规划值」与「合成后实测包围盒」逐像素一致（J1 的 ≤1px 基准）。
pub fn plan_rect(
    img_w: u32,
    img_h: u32,
    cv_w: u32,
    cv_h: u32,
    p: &Placement,
) -> Result<Rect, WmError> {
    if img_w == 0 || img_h == 0 || cv_w == 0 || cv_h == 0 {
        return Err(WmError::Place);
    }
    // 用取反判断，NaN 会落进错误分支而不是溜进计算。
    if !(p.scale > 0.0 && p.scale <= 8.0) {
        return Err(WmError::Place);
    }
    let tw = scale_dim(img_w, p.scale);
    let th = scale_dim(img_h, p.scale);
    let m2 = match p.margin.checked_mul(2) {
        Some(v) => v,
        None => return Err(WmError::Place),
    };
    if tw + m2 > cv_w || th + m2 > cv_h {
        return Err(WmError::Place);
    }
    let x = match p.anchor {
        Anchor::TopLeft | Anchor::MidLeft | Anchor::BotLeft => p.margin,
        Anchor::TopCenter | Anchor::Center | Anchor::BotCenter => (cv_w - tw) / 2,
        Anchor::TopRight | Anchor::MidRight | Anchor::BotRight => cv_w - tw - p.margin,
    };
    let y = match p.anchor {
        Anchor::TopLeft | Anchor::TopCenter | Anchor::TopRight => p.margin,
        Anchor::MidLeft | Anchor::Center | Anchor::MidRight => (cv_h - th) / 2,
        Anchor::BotLeft | Anchor::BotCenter | Anchor::BotRight => cv_h - th - p.margin,
    };
    Ok(Rect::new(x, y, tw, th))
}

// ---------------------------------------------------------------------------
// 字节序 / 校验和
// ---------------------------------------------------------------------------

fn be_u32(v: &[u8], at: usize) -> Result<u32, WmError> {
    match (v.get(at), v.get(at + 1), v.get(at + 2), v.get(at + 3)) {
        (Some(a), Some(b), Some(c), Some(d)) => {
            Ok(((*a as u32) << 24) | ((*b as u32) << 16) | ((*c as u32) << 8) | *d as u32)
        }
        _ => Err(WmError::Truncated),
    }
}

/// 逐位 CRC32（PNG 多项式 0xEDB88320）。不建表：一张 1KiB 表换 8 次移位，
/// 内核里不值得为校验常驻数据。
fn crc32(bytes: &[u8]) -> u32 {
    let mut c: u32 = 0xffff_ffff;
    let mut i = 0usize;
    while i < bytes.len() {
        c ^= bytes[i] as u32;
        let mut k = 0u32;
        while k < 8 {
            if c & 1 != 0 {
                c = 0xedb8_8320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            k += 1;
        }
        i += 1;
    }
    c ^ 0xffff_ffff
}

fn adler32(bytes: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        a = (a + bytes[i] as u32) % 65521;
        b = (b + a) % 65521;
        i += 1;
    }
    (b << 16) | a
}

fn ty_is(ty: &[u8], name: &[u8; 4]) -> bool {
    if ty.len() != 4 {
        return false;
    }
    let mut i = 0usize;
    let mut ok = true;
    while i < 4 {
        match ty.get(i) {
            Some(v) => {
                if *v != name[i] {
                    ok = false;
                }
            }
            None => ok = false,
        }
        i += 1;
    }
    ok
}

// ---------------------------------------------------------------------------
// zlib STORED 解压 / 压缩
// ---------------------------------------------------------------------------

/// 解 STORED 块的 zlib 流（BTYPE=00）。LEN/NLEN 必须互补，Adler32 必须吻合。
fn inflate_stored(z: &[u8]) -> Result<Vec<u8>, WmError> {
    if z.len() < 6 {
        return Err(WmError::Truncated);
    }
    let cmf = match z.first() {
        Some(v) => *v,
        None => return Err(WmError::Truncated),
    };
    let flg = match z.get(1) {
        Some(v) => *v,
        None => return Err(WmError::Truncated),
    };
    if cmf & 0x0f != 8 || (cmf >> 4) > 7 {
        return Err(WmError::Zlib);
    }
    if (((cmf as u32) << 8) | flg as u32) % 31 != 0 {
        return Err(WmError::Zlib);
    }
    // FDICT=1 表示流里带预置字典，本解码器不支持，显式拒绝。
    if flg & 0x20 != 0 {
        return Err(WmError::Zlib);
    }
    let mut at = 2usize;
    let mut out: Vec<u8> = Vec::new();
    loop {
        let hdr = match z.get(at) {
            Some(v) => *v,
            None => return Err(WmError::Truncated),
        };
        at += 1;
        let last = hdr & 1 != 0;
        let btype = (hdr >> 1) & 3;
        if btype != 0 {
            return Err(WmError::Zlib);
        }
        if at + 4 > z.len() {
            return Err(WmError::Truncated);
        }
        let lo = match z.get(at) {
            Some(v) => *v as usize,
            None => return Err(WmError::Truncated),
        };
        let hi = match z.get(at + 1) {
            Some(v) => *v as usize,
            None => return Err(WmError::Truncated),
        };
        let nlo = match z.get(at + 2) {
            Some(v) => *v as usize,
            None => return Err(WmError::Truncated),
        };
        let nhi = match z.get(at + 3) {
            Some(v) => *v as usize,
            None => return Err(WmError::Truncated),
        };
        let len = lo | (hi << 8);
        let nlen = nlo | (nhi << 8);
        if len ^ 0xffff != nlen {
            return Err(WmError::Zlib);
        }
        at += 4;
        if at + len > z.len() {
            return Err(WmError::Truncated);
        }
        if out.len() + len > MAX_RAW {
            return Err(WmError::Zlib);
        }
        match z.get(at..at + len) {
            Some(seg) => out.extend_from_slice(seg),
            None => return Err(WmError::Truncated),
        }
        at += len;
        if last {
            break;
        }
    }
    let want = be_u32(z, at)?;
    if adler32(&out) != want {
        return Err(WmError::Adler);
    }
    Ok(out)
}

/// 把原始字节封成 zlib STORED 流（65535 字节一块 + Adler32 尾巴）。
fn deflate_stored(raw: &[u8]) -> Vec<u8> {
    let mut z: Vec<u8> = Vec::with_capacity(raw.len() + 16);
    z.push(0x78);
    z.push(0x01);
    let mut off = 0usize;
    loop {
        let remain = raw.len() - off;
        let n = if remain > 65535 { 65535 } else { remain };
        let last = off + n >= raw.len();
        z.push(if last { 1u8 } else { 0u8 });
        let nl = !(n as u16);
        z.push((n & 0xff) as u8);
        z.push(((n >> 8) & 0xff) as u8);
        z.push((nl & 0xff) as u8);
        z.push(((nl >> 8) & 0xff) as u8);
        if let Some(seg) = raw.get(off..off + n) {
            z.extend_from_slice(seg);
        }
        off += n;
        if last {
            break;
        }
    }
    z.extend_from_slice(&adler32(raw).to_be_bytes());
    z
}

// ---------------------------------------------------------------------------
// 行滤波（PNG 15.2 的五种）
// ---------------------------------------------------------------------------

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i32 + b as i32 - c as i32;
    let pa = (p - a as i32).abs();
    let pb = (p - b as i32).abs();
    let pc = (p - c as i32).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn predictor(code: u8, left: u8, up: u8, up_left: u8) -> Result<u8, WmError> {
    match code {
        0 => Ok(0),
        1 => Ok(left),
        2 => Ok(up),
        3 => Ok((((left as u32) + (up as u32)) / 2) as u8),
        4 => Ok(paeth(left, up, up_left)),
        _ => Err(WmError::Filter),
    }
}

/// 滤波复原。PNG 规定逐字节模 256 累加，`as u8` 的截断语义恰好等价。
fn unfilter(raw: &[u8], w: usize, h: usize, bpp: usize) -> Result<Vec<u8>, WmError> {
    let stride = w * bpp;
    let need = h * (stride + 1);
    if stride == 0 || raw.len() != need {
        return Err(WmError::Truncated);
    }
    let mut out: Vec<u8> = Vec::new();
    out.resize(h * stride, 0u8);
    let mut y = 0usize;
    while y < h {
        let row = y * (stride + 1);
        let code = match raw.get(row) {
            Some(v) => *v,
            None => return Err(WmError::Truncated),
        };
        if code > 4 {
            return Err(WmError::Filter);
        }
        let src = match raw.get(row + 1..row + 1 + stride) {
            Some(v) => v,
            None => return Err(WmError::Truncated),
        };
        let mut x = 0usize;
        while x < stride {
            let left = if x >= bpp {
                match out.get(y * stride + x - bpp) {
                    Some(v) => *v,
                    None => 0,
                }
            } else {
                0
            };
            let up = if y > 0 {
                match out.get((y - 1) * stride + x) {
                    Some(v) => *v,
                    None => 0,
                }
            } else {
                0
            };
            let ul = if y > 0 && x >= bpp {
                match out.get((y - 1) * stride + x - bpp) {
                    Some(v) => *v,
                    None => 0,
                }
            } else {
                0
            };
            let pred = predictor(code, left, up, ul)?;
            // 复原是「编码减、解码加」：PNG 规定逐字节模 256 累加。
            let v = match src.get(x) {
                Some(s) => s.wrapping_add(pred),
                None => return Err(WmError::Truncated),
            };
            out[y * stride + x] = v;
            x += 1;
        }
        y += 1;
    }
    Ok(out)
}

fn filter_row(code: u8, cur: &[u8], prev: &[u8], bpp: usize, out: &mut Vec<u8>) {
    out.push(code);
    let mut x = 0usize;
    while x < cur.len() {
        let left = if x >= bpp {
            match cur.get(x - bpp) {
                Some(v) => *v,
                None => 0,
            }
        } else {
            0
        };
        let up = match prev.get(x) {
            Some(v) => *v,
            None => 0,
        };
        let ul = if x >= bpp {
            match prev.get(x - bpp) {
                Some(v) => *v,
                None => 0,
            }
        } else {
            0
        };
        let pred = match code {
            0 => 0u8,
            1 => left,
            2 => up,
            3 => ((left as u32 + up as u32) / 2) as u8,
            _ => paeth(left, up, ul),
        };
        let v = match cur.get(x) {
            Some(c) => c.wrapping_sub(pred),
            None => 0,
        };
        out.push(v);
        x += 1;
    }
}

// ---------------------------------------------------------------------------
// PNG 解析
// ---------------------------------------------------------------------------

/// 解析 PNG。只接受 8bit 的 RGB(2)/RGBA(6) 非隔行图；IHDR 必须是首块，
/// 未知**关键块**直接拒绝（ancillary 小写块可忽略）。
pub fn parse_png(data: &[u8]) -> Result<PngImage, WmError> {
    if data.is_empty() {
        return Err(WmError::Empty);
    }
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if data.len() < 8 {
        return Err(WmError::Truncated);
    }
    let mut i = 0usize;
    while i < 8 {
        match data.get(i) {
            Some(v) => {
                if *v != SIG[i] {
                    return Err(WmError::Signature);
                }
            }
            None => return Err(WmError::Truncated),
        }
        i += 1;
    }
    let mut at = 8usize;
    let mut w = 0u32;
    let mut h = 0u32;
    let mut channels: u8 = 0;
    let mut seen_ihdr = false;
    let mut seen_iend = false;
    let mut idat: Vec<u8> = Vec::new();
    while at < data.len() {
        if at + 8 > data.len() {
            return Err(WmError::Truncated);
        }
        let len = be_u32(data, at)? as usize;
        if len > 8_388_608 {
            return Err(WmError::Chunk);
        }
        let ty = match data.get(at + 4..at + 8) {
            Some(v) => v,
            None => return Err(WmError::Truncated),
        };
        let dstart = at + 8;
        let dend = dstart + len;
        if dend > data.len() {
            return Err(WmError::Truncated);
        }
        let body = match data.get(dstart..dend) {
            Some(v) => v,
            None => return Err(WmError::Truncated),
        };
        let crc = be_u32(data, dend)?;
        let mut crc_in: Vec<u8> = Vec::with_capacity(4 + body.len());
        crc_in.extend_from_slice(ty);
        crc_in.extend_from_slice(body);
        if crc32(&crc_in) != crc {
            return Err(WmError::Crc);
        }
        if !seen_ihdr && !ty_is(ty, b"IHDR") {
            return Err(WmError::Chunk);
        }
        if ty_is(ty, b"IHDR") {
            if seen_ihdr || body.len() != 13 {
                return Err(WmError::Chunk);
            }
            seen_ihdr = true;
            w = be_u32(body, 0)?;
            h = be_u32(body, 4)?;
            if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
                return Err(WmError::Chunk);
            }
            let depth = match body.get(8) {
                Some(v) => *v,
                None => return Err(WmError::Truncated),
            };
            let color = match body.get(9) {
                Some(v) => *v,
                None => return Err(WmError::Truncated),
            };
            let inter = match body.get(12) {
                Some(v) => *v,
                None => return Err(WmError::Truncated),
            };
            if depth != 8 {
                return Err(WmError::BitDepth);
            }
            if color != 2 && color != 6 {
                return Err(WmError::ColorType);
            }
            if inter != 0 {
                return Err(WmError::Interlace);
            }
            channels = if color == 6 { 4 } else { 3 };
        } else if ty_is(ty, b"IDAT") {
            idat.extend_from_slice(body);
        } else if ty_is(ty, b"IEND") {
            seen_iend = true;
            break;
        } else {
            let critical = match ty.first() {
                Some(v) => v.is_ascii_uppercase(),
                None => return Err(WmError::Chunk),
            };
            if critical {
                return Err(WmError::Chunk);
            }
        }
        at = dend + 4;
    }
    if !seen_ihdr {
        return Err(WmError::Chunk);
    }
    if !seen_iend {
        return Err(WmError::Truncated);
    }
    if idat.is_empty() {
        return Err(WmError::Truncated);
    }
    let bpp = channels as usize;
    let plain = inflate_stored(&idat)?;
    let rows = unfilter(&plain, w as usize, h as usize, bpp)?;
    let n = w as usize * h as usize;
    let mut px: Vec<u8> = Vec::with_capacity(n * 4);
    if bpp == 4 {
        px.extend_from_slice(&rows);
    } else {
        let mut k = 0usize;
        while k < n {
            let o = k * 3;
            let r = match rows.get(o) {
                Some(v) => *v,
                None => return Err(WmError::Truncated),
            };
            let g = match rows.get(o + 1) {
                Some(v) => *v,
                None => return Err(WmError::Truncated),
            };
            let b = match rows.get(o + 2) {
                Some(v) => *v,
                None => return Err(WmError::Truncated),
            };
            px.push(r);
            px.push(g);
            px.push(b);
            px.push(255);
            k += 1;
        }
    }
    Ok(PngImage {
        w,
        h,
        channels,
        pixels: px,
    })
}

fn push_chunk(out: &mut Vec<u8>, ty: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(ty);
    out.extend_from_slice(body);
    let mut crc_in: Vec<u8> = Vec::with_capacity(4 + body.len());
    crc_in.extend_from_slice(ty);
    crc_in.extend_from_slice(body);
    out.extend_from_slice(&crc32(&crc_in).to_be_bytes());
}

/// 组装 PNG 字节流：IHDR 字段全可控、压缩流可替换、可插入未知关键块。
/// 编码器与注入核共用同一份组帧代码，避免「测试用假 PNG」和真实解析器脱节。
fn build_png_raw(
    w: u32,
    h: u32,
    depth: u8,
    color: u8,
    inter: u8,
    raw: &[u8],
    zlib: Option<&[u8]>,
    extra_critical: bool,
) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    let mut ihdr: Vec<u8> = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.push(depth);
    ihdr.push(color);
    ihdr.push(0);
    ihdr.push(0);
    ihdr.push(inter);
    push_chunk(&mut out, b"IHDR", &ihdr);
    if extra_critical {
        push_chunk(&mut out, b"ZZZZ", &[]);
    }
    let z = match zlib {
        Some(v) => {
            let mut t: Vec<u8> = Vec::with_capacity(v.len());
            t.extend_from_slice(v);
            t
        }
        None => deflate_stored(raw),
    };
    push_chunk(&mut out, b"IDAT", &z);
    push_chunk(&mut out, b"IEND", &[]);
    out
}

/// 行滤波策略。`Cycle` 逐行轮换 0..4，用于一次性覆盖五种逆滤波。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowFilter {
    None,
    Sub,
    Up,
    Average,
    Paeth,
    Cycle,
}

impl RowFilter {
    pub fn code(self) -> u8 {
        match self {
            RowFilter::None => 0,
            RowFilter::Sub => 1,
            RowFilter::Up => 2,
            RowFilter::Average => 3,
            RowFilter::Paeth => 4,
            RowFilter::Cycle => 0,
        }
    }
}

/// 编码 RGBA8 → PNG（zlib STORED）。像素长度必须恰好 `w*h*4`。
pub fn encode_png(w: u32, h: u32, rgba: &[u8], filter: RowFilter) -> Result<Vec<u8>, WmError> {
    if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
        return Err(WmError::Canvas);
    }
    let stride = w as usize * 4;
    let need = stride * h as usize;
    if rgba.len() != need {
        return Err(WmError::Canvas);
    }
    let mut raw: Vec<u8> = Vec::with_capacity(h as usize * (stride + 1));
    let mut prev: Vec<u8> = Vec::new();
    prev.resize(stride, 0u8);
    let mut y = 0usize;
    while y < h as usize {
        let cur = match rgba.get(y * stride..y * stride + stride) {
            Some(v) => v,
            None => return Err(WmError::Truncated),
        };
        let code = if filter == RowFilter::Cycle {
            (y % 5) as u8
        } else {
            filter.code()
        };
        filter_row(code, cur, &prev, 4, &mut raw);
        prev.clear();
        prev.extend_from_slice(cur);
        y += 1;
    }
    Ok(build_png_raw(w, h, 8, 6, 0, &raw, None, false))
}

// ---------------------------------------------------------------------------
// 重采样与合成
// ---------------------------------------------------------------------------

fn overlap(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    let lo = if a0 > b0 { a0 } else { b0 };
    let hi = if a1 < b1 { a1 } else { b1 };
    let d = hi - lo;
    if d > 0.0 {
        d
    } else {
        0.0
    }
}

fn quant(v: f32) -> u8 {
    if v <= 0.0 {
        0
    } else if v >= 255.0 {
        255
    } else {
        floorf(v + 0.5) as u8
    }
}

/// 面积平均重采样（盒式滤波）。下采样按源覆盖面积加权，上采样自然退化为
/// 邻域分段平均：常量图重采样后仍是常量（判据「缩放保色」的基础）。
/// 不预乘 alpha——水印图的半透明像素是文字抗锯齿边缘，按 alpha 加权会把
/// 边缘色往背景色拉，反而脏。
fn resample(img: &PngImage, tw: u32, th: u32) -> Result<Vec<u8>, WmError> {
    let sw = img.w;
    let sh = img.h;
    if sw == 0 || sh == 0 || tw == 0 || th == 0 || tw > MAX_DIM || th > MAX_DIM {
        return Err(WmError::Place);
    }
    let mut out: Vec<u8> = Vec::with_capacity(tw as usize * th as usize * 4);
    let mut dy = 0u32;
    while dy < th {
        let v0 = dy as f32 * sh as f32 / th as f32;
        let v1 = (dy + 1) as f32 * sh as f32 / th as f32;
        let mut iy0 = floorf(v0) as u32;
        if iy0 >= sh {
            iy0 = sh - 1;
        }
        let mut iy1 = ceilf(v1) as u32;
        if iy1 > sh {
            iy1 = sh;
        }
        if iy1 <= iy0 {
            iy1 = iy0 + 1;
        }
        let mut dx = 0u32;
        while dx < tw {
            let u0 = dx as f32 * sw as f32 / tw as f32;
            let u1 = (dx + 1) as f32 * sw as f32 / tw as f32;
            let mut ix0 = floorf(u0) as u32;
            if ix0 >= sw {
                ix0 = sw - 1;
            }
            let mut ix1 = ceilf(u1) as u32;
            if ix1 > sw {
                ix1 = sw;
            }
            if ix1 <= ix0 {
                ix1 = ix0 + 1;
            }
            let mut acc = [0.0f32; 4];
            let mut wsum = 0.0f32;
            let mut sy = iy0;
            while sy < iy1 {
                let wy = overlap(v0, v1, sy as f32, (sy + 1) as f32);
                let mut sx = ix0;
                while sx < ix1 {
                    let wx = overlap(u0, u1, sx as f32, (sx + 1) as f32);
                    let w = wx * wy;
                    if w > 0.0 {
                        if let Some(p) = img.pixel(sx, sy) {
                            acc[0] += p[0] as f32 * w;
                            acc[1] += p[1] as f32 * w;
                            acc[2] += p[2] as f32 * w;
                            acc[3] += p[3] as f32 * w;
                            wsum += w;
                        }
                    }
                    sx += 1;
                }
                sy += 1;
            }
            if wsum <= 0.0 {
                // 覆盖面积恒 >0，此分支只是防御除零；取最近邻不至于产生空洞。
                if let Some(p) = img.pixel(ix0, iy0) {
                    acc = [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32];
                    wsum = 1.0;
                }
            }
            out.push(quant(acc[0] / wsum));
            out.push(quant(acc[1] / wsum));
            out.push(quant(acc[2] / wsum));
            out.push(quant(acc[3] / wsum));
            dx += 1;
        }
        dy += 1;
    }
    Ok(out)
}

/// 非预乘 source-over：`ao = as + ad(1-as)`，`co = (cs·as + cd·ad(1-as)) / ao`。
/// 全透明源直接原样返回目标像素（既省算力，也保证「零 alpha 幂等」）。
fn src_over(s: [u8; 4], d: [u8; 4]) -> [u8; 4] {
    let a_s = s[3] as u32;
    if a_s == 0 {
        return d;
    }
    if a_s == 255 {
        return s;
    }
    let a_d = d[3] as u32;
    let a_o = a_s + a_d * (255 - a_s) / 255;
    let mut o = [0u8; 4];
    let mut i = 0usize;
    while i < 3 {
        if a_o == 0 {
            o[i] = 0;
        } else {
            let num = s[i] as u32 * a_s * 255 + d[i] as u32 * a_d * (255 - a_s);
            o[i] = (num / (255 * a_o)) as u8;
        }
        i += 1;
    }
    o[3] = a_o as u8;
    o
}

/// 一次合成的落账记录。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Composited {
    pub rect: Rect,
    pub pixels: u32,
    /// 矩形内平均输出 alpha（未触达像素按 0 计），用于判断水印是否「够实」。
    pub mean_alpha: u8,
}

impl Composited {
    /// 一行可审计描述（日志/账本导出用）。
    pub fn describe(&self) -> String {
        format!(
            "rect={}x{}@{},{} px={} alpha={}",
            self.rect.w, self.rect.h, self.rect.x, self.rect.y, self.pixels, self.mean_alpha
        )
    }
}

fn fnv_step(h: &mut u64, v: u32) {
    *h ^= v as u64;
    *h = h.wrapping_mul(0x0000_0100_0000_01b3);
}

/// 合成账本：按序记录每次合成，摘要用于「同输入同账」的确定性断言。
#[derive(Clone, Debug, PartialEq)]
pub struct CompositeLedger {
    pub entries: Vec<Composited>,
    pub total_pixels: u64,
}

impl CompositeLedger {
    pub fn new() -> CompositeLedger {
        CompositeLedger {
            entries: Vec::new(),
            total_pixels: 0,
        }
    }

    pub fn record(&mut self, c: Composited) {
        self.total_pixels += c.pixels as u64;
        self.entries.push(c);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                fnv_step(&mut h, e.rect.x);
                fnv_step(&mut h, e.rect.y);
                fnv_step(&mut h, e.rect.w);
                fnv_step(&mut h, e.rect.h);
                fnv_step(&mut h, e.mean_alpha as u32);
            }
            i += 1;
        }
        h
    }
}

/// 把图像按 `rect` 缩放合成到画布。矩形越界**显式报错**而不是裁剪：
/// 半张水印比没有水印更容易误导读者。
pub fn composite_over(
    cv: &mut Canvas,
    rect: Rect,
    img: &PngImage,
) -> Result<Composited, WmError> {
    if rect.w == 0 || rect.h == 0 {
        return Err(WmError::Place);
    }
    let x1 = match rect.x.checked_add(rect.w) {
        Some(v) => v,
        None => return Err(WmError::Place),
    };
    let y1 = match rect.y.checked_add(rect.h) {
        Some(v) => v,
        None => return Err(WmError::Place),
    };
    if x1 > cv.w || y1 > cv.h {
        return Err(WmError::Place);
    }
    let scaled = resample(img, rect.w, rect.h)?;
    let mut sum_a: u64 = 0;
    let mut y = 0u32;
    while y < rect.h {
        let mut x = 0u32;
        while x < rect.w {
            let i = (y as usize * rect.w as usize + x as usize) * 4;
            let s = match (
                scaled.get(i),
                scaled.get(i + 1),
                scaled.get(i + 2),
                scaled.get(i + 3),
            ) {
                (Some(a), Some(b), Some(c), Some(d)) => [*a, *b, *c, *d],
                _ => return Err(WmError::Truncated),
            };
            if s[3] > 0 {
                if let Some(d) = cv.get(rect.x + x, rect.y + y) {
                    let o = src_over(s, d);
                    cv.put(rect.x + x, rect.y + y, o);
                    sum_a += o[3] as u64;
                }
            }
            x += 1;
        }
        y += 1;
    }
    let n = rect.w as u64 * rect.h as u64;
    let mean = if n == 0 { 0 } else { (sum_a / n) as u8 };
    Ok(Composited {
        rect,
        pixels: rect.w * rect.h,
        mean_alpha: mean,
    })
}

// ---------------------------------------------------------------------------
// 注入核：十一类坏 PNG 必须逐类被抓
// ---------------------------------------------------------------------------

fn solid_image(w: u32, h: u32, c: [u8; 4]) -> PngImage {
    let mut px: Vec<u8> = Vec::with_capacity(w as usize * h as usize * 4);
    let mut i = 0usize;
    let n = w as usize * h as usize;
    while i < n {
        px.push(c[0]);
        px.push(c[1]);
        px.push(c[2]);
        px.push(c[3]);
        i += 1;
    }
    PngImage {
        w,
        h,
        channels: 4,
        pixels: px,
    }
}

fn gradient_image(w: u32, h: u32) -> PngImage {
    let mut px: Vec<u8> = Vec::with_capacity(w as usize * h as usize * 4);
    let dw = if w > 1 { w - 1 } else { 1 };
    let dh = if h > 1 { h - 1 } else { 1 };
    let mut y = 0u32;
    while y < h {
        let mut x = 0u32;
        while x < w {
            px.push((x * 255 / dw) as u8);
            px.push((y * 255 / dh) as u8);
            px.push(128);
            px.push(255);
            x += 1;
        }
        y += 1;
    }
    PngImage {
        w,
        h,
        channels: 4,
        pixels: px,
    }
}

/// 合法基线：4×4 全零 RGBA，行滤波 0。
fn baseline_raw(w: usize, h: usize) -> Vec<u8> {
    let mut raw: Vec<u8> = Vec::new();
    raw.resize(h * (w * 4 + 1), 0u8);
    raw
}

/// 返回（抓到的坏样本数, 总数）。每类只断言**具体**诊断码，不做「任意错误即可」。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 11usize;
    let raw = baseline_raw(4, 4);
    let good = build_png_raw(4, 4, 8, 6, 0, &raw, None, false);

    if parse_png(&[]).err() == Some(WmError::Empty) {
        caught += 1;
    }
    let mut sig = good.clone();
    if let Some(b) = sig.first_mut() {
        *b ^= 0xff;
    }
    if parse_png(&sig).err() == Some(WmError::Signature) {
        caught += 1;
    }
    let cut = match good.get(0..20) {
        Some(v) => v,
        None => &good[..],
    };
    if parse_png(cut).err() == Some(WmError::Truncated) {
        caught += 1;
    }
    let mut crc = good.clone();
    if crc.len() > 29 {
        crc[29] ^= 0xff;
    }
    if parse_png(&crc).err() == Some(WmError::Crc) {
        caught += 1;
    }
    let extra = build_png_raw(4, 4, 8, 6, 0, &raw, None, true);
    if parse_png(&extra).err() == Some(WmError::Chunk) {
        caught += 1;
    }
    if parse_png(&build_png_raw(4, 4, 8, 3, 0, &raw, None, false)).err()
        == Some(WmError::ColorType)
    {
        caught += 1;
    }
    if parse_png(&build_png_raw(4, 4, 16, 2, 0, &raw, None, false)).err()
        == Some(WmError::BitDepth)
    {
        caught += 1;
    }
    if parse_png(&build_png_raw(4, 4, 8, 6, 1, &raw, None, false)).err()
        == Some(WmError::Interlace)
    {
        caught += 1;
    }
    // BTYPE=01（固定 Huffman）——本解码器只吃 STORED，必须拒绝。
    let mut fake_z: Vec<u8> = Vec::new();
    fake_z.extend_from_slice(&[0x78, 0x01, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    if parse_png(&build_png_raw(4, 4, 8, 6, 0, &raw, Some(&fake_z), false)).err()
        == Some(WmError::Zlib)
    {
        caught += 1;
    }
    let mut bad_adler = deflate_stored(&raw);
    if let Some(last) = bad_adler.last_mut() {
        *last ^= 0xff;
    }
    if parse_png(&build_png_raw(4, 4, 8, 6, 0, &raw, Some(&bad_adler), false)).err()
        == Some(WmError::Adler)
    {
        caught += 1;
    }
    let mut bad_filter = baseline_raw(4, 4);
    if let Some(first) = bad_filter.first_mut() {
        *first = 9;
    }
    if parse_png(&build_png_raw(4, 4, 8, 6, 0, &bad_filter, None, false)).err()
        == Some(WmError::Filter)
    {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

fn diff_u32(a: u32, b: u32) -> u32 {
    if a > b {
        a - b
    } else {
        b - a
    }
}

fn max4(a: u32, b: u32, c: u32, d: u32) -> u32 {
    let mut m = a;
    if b > m {
        m = b;
    }
    if c > m {
        m = c;
    }
    if d > m {
        m = d;
    }
    m
}

/// J1 语料：九宫 × 四档缩放 × 四档边距 × 递增画布，共 20 组。
fn placement_probe() -> (usize, u32) {
    let anchors = [
        Anchor::TopLeft,
        Anchor::TopCenter,
        Anchor::TopRight,
        Anchor::MidLeft,
        Anchor::Center,
        Anchor::MidRight,
        Anchor::BotLeft,
        Anchor::BotCenter,
        Anchor::BotRight,
    ];
    let scales = [1.0f32, 0.5, 0.25, 1.5];
    let margins = [0u32, 3, 7, 11];
    let img = solid_image(32, 24, [200, 30, 30, 255]);
    let mut ok = 0usize;
    let mut worst = 0u32;
    let mut i = 0usize;
    while i < 20 {
        let cvw = 96 + (i as u32) * 4;
        let cvh = 80 + (i as u32) * 3;
        let p = Placement::new(anchors[i % 9], scales[i % 4], margins[i % 4]);
        if let Ok(r) = plan_rect(32, 24, cvw, cvh, &p) {
            if let Ok(mut cv) = Canvas::new(cvw, cvh) {
                if composite_over(&mut cv, r, &img).is_ok() {
                    if let Some(b) = cv.ink_bbox() {
                        let dev = max4(
                            diff_u32(r.x, b.x),
                            diff_u32(r.y, b.y),
                            diff_u32(r.w, b.w),
                            diff_u32(r.h, b.h),
                        );
                        if dev > worst {
                            worst = dev;
                        }
                        if dev <= 1 {
                            ok += 1;
                        }
                    }
                }
            }
        }
        i += 1;
    }
    (ok, worst)
}

fn fill(cv: &mut Canvas, c: [u8; 4]) {
    let mut y = 0u32;
    while y < cv.h {
        let mut x = 0u32;
        while x < cv.w {
            cv.put(x, y, c);
            x += 1;
        }
        y += 1;
    }
}

fn region_pixels(cv: &Canvas, r: Rect) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(r.area() as usize * 4);
    let mut y = r.y;
    while y < r.y + r.h {
        let mut x = r.x;
        while x < r.x + r.w {
            if let Some(p) = cv.get(x, y) {
                out.push(p[0]);
                out.push(p[1]);
                out.push(p[2]);
                out.push(p[3]);
            }
            x += 1;
        }
        y += 1;
    }
    out
}

fn mean_channel(px: &[u8], ch: usize) -> f32 {
    let mut sum = 0u64;
    let mut n = 0u64;
    let mut i = ch;
    while i < px.len() {
        if let Some(v) = px.get(i) {
            sum += *v as u64;
            n += 1;
        }
        i += 4;
    }
    if n == 0 {
        0.0
    } else {
        sum as f32 / n as f32
    }
}

pub fn run_unx50042_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50042");
    let img = solid_image(32, 24, [200, 30, 30, 255]);

    // J1-1 20 组合成：规划矩形 vs 实测不透明包围盒，逐边偏差 ≤1px
    let (place_ok, worst) = placement_probe();
    let ok = place_ok == 20 && worst <= 1;
    let det = format!("{}/20 worst={}px", place_ok, worst);
    s.add_dyn("50042-J1-定位≤1px", ok, &det);

    // J1-2 居中落位：九宫求心的整数解与实测盒逐边相等
    let mut ok = false;
    if let Ok(r) = plan_rect(32, 24, 120, 90, &Placement::new(Anchor::Center, 1.0, 0)) {
        if let Ok(mut cv) = Canvas::new(120, 90) {
            if composite_over(&mut cv, r, &img).is_ok() {
                ok = cv.ink_bbox() == Some(r) && r.x == 44 && r.y == 33;
            }
        }
    }
    s.add("50042-J1-居中盒一致", ok, if ok { "" } else { "包围盒与规划不符" });

    // J1-3 六种行滤波（含逐行轮换）编解码往返逐字节一致
    let src = gradient_image(12, 9);
    let kinds = [
        RowFilter::None,
        RowFilter::Sub,
        RowFilter::Up,
        RowFilter::Average,
        RowFilter::Paeth,
        RowFilter::Cycle,
    ];
    let mut rt = 0usize;
    let mut mask = 0u32;
    let mut last_err = 0u32;
    let mut first_diff = 0usize;
    let mut k = 0usize;
    while k < kinds.len() {
        if let Ok(bytes) = encode_png(src.w, src.h, &src.pixels, kinds[k]) {
            match parse_png(&bytes) {
                Ok(back) => {
                    if back.w == src.w && back.h == src.h && back.pixels == src.pixels {
                        rt += 1;
                        mask |= 1u32 << k;
                    } else {
                        let mut d = 0usize;
                        let mut found = false;
                        while d < src.pixels.len() && !found {
                            if back.pixels.get(d) != src.pixels.get(d) {
                                found = true;
                                first_diff = d;
                            }
                            d += 1;
                        }
                    }
                }
                Err(e) => last_err = e.code(),
            }
        }
        k += 1;
    }
    let ok = rt == 6;
    let det = format!("往返 {}/6 mask={} err={} at={}", rt, mask, last_err, first_diff);
    s.add_dyn("50042-J1-滤波往返", ok, &det);

    // J1-4 确定性与账本一致：同输入两次合成 → 像素与摘要逐位相同
    let mut ok = false;
    let p = Placement::new(Anchor::BotRight, 0.5, 5);
    if let Ok(r) = plan_rect(32, 24, 160, 120, &p) {
        let mut led_a = CompositeLedger::new();
        let mut led_b = CompositeLedger::new();
        if let (Ok(mut a), Ok(mut b)) = (Canvas::new(160, 120), Canvas::new(160, 120)) {
            if let (Ok(ca), Ok(cb)) = (composite_over(&mut a, r, &img), composite_over(&mut b, r, &img)) {
                led_a.record(ca);
                led_b.record(cb);
                ok = a.px == b.px && led_a.digest() == led_b.digest() && a.ink_bbox() == Some(r);
            }
        }
    }
    s.add("50042-J1-合成确定", ok, if ok { "" } else { "两次合成不一致" });

    // J1-5 零 alpha 幂等：全透明水印对画布零影响
    let ghost = solid_image(16, 16, [10, 20, 30, 0]);
    let mut ok = false;
    if let (Ok(mut base), Ok(mut target)) = (Canvas::new(20, 20), Canvas::new(20, 20)) {
        fill(&mut base, [5, 6, 7, 255]);
        fill(&mut target, [5, 6, 7, 255]);
        if let Ok(r) = plan_rect(16, 16, 20, 20, &Placement::new(Anchor::TopLeft, 1.0, 0)) {
            if let Ok(c) = composite_over(&mut target, r, &ghost) {
                ok = c.mean_alpha == 0 && target.px == base.px;
            }
        }
    }
    s.add("50042-J1-零alpha幂等", ok, if ok { "" } else { "透明源改写了画布" });

    // J1-6 alpha 单调：源 alpha 递增 → 输出 alpha 非降且终点 255
    let mut prev_a = 0u8;
    let mut mono = true;
    let mut last = 0u8;
    let steps = [0u8, 64, 128, 192, 255];
    let mut i = 0usize;
    while i < steps.len() {
        if let Ok(mut cv) = Canvas::new(1, 1) {
            cv.put(0, 0, [50, 50, 50, 128]);
            let one = solid_image(1, 1, [100, 0, 0, steps[i]]);
            if composite_over(&mut cv, Rect::new(0, 0, 1, 1), &one).is_ok() {
                if let Some(px) = cv.get(0, 0) {
                    if i > 0 && px[3] < prev_a {
                        mono = false;
                    }
                    prev_a = px[3];
                    last = px[3];
                }
            }
        }
        i += 1;
    }
    let ok = mono && last == 255;
    let det = format!("末值={} 单调={}", last, mono);
    s.add_dyn("50042-J1-alpha单调", ok, &det);

    // J1-7 常量图缩放保色：64→16 盒式滤波后四角与中心仍是原色
    let flat = solid_image(64, 64, [12, 180, 90, 255]);
    let mut ok = false;
    if let Ok(r) = plan_rect(64, 64, 32, 32, &Placement::new(Anchor::TopLeft, 0.25, 0)) {
        if let Ok(mut cv) = Canvas::new(32, 32) {
            if composite_over(&mut cv, r, &flat).is_ok() {
                let probes = [(0u32, 0u32), (15, 0), (0, 15), (15, 15), (8, 8)];
                let mut good = 0usize;
                let mut j = 0usize;
                while j < probes.len() {
                    if let Some(p) = cv.get(probes[j].0, probes[j].1) {
                        if p == [12, 180, 90, 255] {
                            good += 1;
                        }
                    }
                    j += 1;
                }
                ok = good == probes.len();
            }
        }
    }
    s.add("50042-J1-缩放保色", ok, if ok { "" } else { "常量图缩放失真" });

    // J1-8 渐变缩放均值守恒（容差 2 个码值，来自 8bit 量化）
    let mut ok = false;
    let mut mean_r = 0.0f32;
    let mut mean_g = 0.0f32;
    if let Ok(r) = plan_rect(16, 16, 40, 40, &Placement::new(Anchor::Center, 0.25, 0)) {
        if let Ok(mut cv) = Canvas::new(40, 40) {
            let grad = gradient_image(16, 16);
            if composite_over(&mut cv, r, &grad).is_ok() {
                let got = region_pixels(&cv, r);
                let want = &grad.pixels;
                mean_r = fabsf(mean_channel(&got, 0) - mean_channel(want, 0));
                mean_g = fabsf(mean_channel(&got, 1) - mean_channel(want, 1));
                ok = mean_r <= 2.0 && mean_g <= 2.0;
            }
        }
    }
    let det = format!("dR={:.2} dG={:.2}", mean_r, mean_g);
    s.add_dyn("50042-J1-缩放均值守恒", ok, &det);

    // J1-9 CRC 篡改必拒（诊断码固定为 E_CRC）
    let raw = baseline_raw(4, 4);
    let good = build_png_raw(4, 4, 8, 6, 0, &raw, None, false);
    let mut crc = good.clone();
    if crc.len() > 29 {
        crc[29] ^= 0xff;
    }
    let ok = parse_png(&crc).err() == Some(WmError::Crc);
    s.add("50042-J1-CRC必拒", ok, if ok { "" } else { "坏 CRC 未被拦" });

    // J1-10 签名/截断两拒
    let mut sig = good.clone();
    if let Some(b) = sig.first_mut() {
        *b ^= 0xff;
    }
    let cut = match good.get(0..20) {
        Some(v) => v,
        None => &good[..],
    };
    let ok = parse_png(&sig).err() == Some(WmError::Signature)
        && parse_png(cut).err() == Some(WmError::Truncated);
    s.add("50042-J1-签名截断拒", ok, if ok { "" } else { "签名/截断未拦" });

    // J1-11 IHDR 三拒：调色板 / 16bit / 隔行
    let ok = parse_png(&build_png_raw(4, 4, 8, 3, 0, &raw, None, false)).err()
        == Some(WmError::ColorType)
        && parse_png(&build_png_raw(4, 4, 16, 2, 0, &raw, None, false)).err()
            == Some(WmError::BitDepth)
        && parse_png(&build_png_raw(4, 4, 8, 6, 1, &raw, None, false)).err()
            == Some(WmError::Interlace);
    s.add("50042-J1-IHDR三拒", ok, if ok { "" } else { "IHDR 非法项漏过" });

    // J1-12 压缩块与 Adler 两拒（本解码器只认 STORED）
    let mut fake_z: Vec<u8> = Vec::new();
    fake_z.extend_from_slice(&[0x78, 0x01, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    let mut bad_adler = deflate_stored(&raw);
    if let Some(last) = bad_adler.last_mut() {
        *last ^= 0xff;
    }
    let ok = parse_png(&build_png_raw(4, 4, 8, 6, 0, &raw, Some(&fake_z), false)).err()
        == Some(WmError::Zlib)
        && parse_png(&build_png_raw(4, 4, 8, 6, 0, &raw, Some(&bad_adler), false)).err()
            == Some(WmError::Adler);
    s.add("50042-J1-zlib两拒", ok, if ok { "" } else { "压缩流非法未拦" });

    // J1-13 坏滤波码必拒
    let mut bad_filter = baseline_raw(4, 4);
    if let Some(first) = bad_filter.first_mut() {
        *first = 9;
    }
    let ok = parse_png(&build_png_raw(4, 4, 8, 6, 0, &bad_filter, None, false)).err()
        == Some(WmError::Filter);
    s.add("50042-J1-坏滤波拒", ok, if ok { "" } else { "滤波码 9 漏过" });

    // J1-14 未知关键块必拒（ZZZZ 大写首字母 = critical）
    let ok = parse_png(&build_png_raw(4, 4, 8, 6, 0, &raw, None, true)).err()
        == Some(WmError::Chunk);
    s.add("50042-J1-关键块拒", ok, if ok { "" } else { "未知关键块漏过" });

    // J1-15 画布与落位两拒
    let ok = Canvas::new(0, 0).err() == Some(WmError::Canvas)
        && plan_rect(32, 24, 40, 40, &Placement::new(Anchor::Center, 2.0, 10)).err()
            == Some(WmError::Place)
        && plan_rect(32, 24, 40, 40, &Placement::new(Anchor::Center, -1.0, 0)).err()
            == Some(WmError::Place);
    s.add("50042-J1-画布落位拒", ok, if ok { "" } else { "越界未拦" });

    // J1-16 注入核汇总：十一类坏样本逐类命中具体码
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50042-J1-注入必抓", ok, &det);

    // J1-17 RGB(3 通道) 源升格 RGBA：alpha 补 255，通道数如实上报
    let mut raw_rgb: Vec<u8> = Vec::new();
    raw_rgb.resize(2 * (2 * 3 + 1), 0u8);
    let mut ok = false;
    if let Ok(back) = parse_png(&build_png_raw(2, 2, 8, 2, 0, &raw_rgb, None, false)) {
        ok = back.channels == 3
            && back.pixels.len() == 2 * 2 * 4
            && back.pixels.get(3) == Some(&255u8);
    }
    s.add("50042-J1-RGB升格", ok, if ok { "" } else { "3 通道未升格" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50042_checks_green() {
        let set = run_unx50042_checks();
        assert!(set.all_passed(), "UNX-F50042 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50042_png_roundtrip_all_filters() {
        let src = gradient_image(9, 7);
        let kinds = [
            RowFilter::None,
            RowFilter::Sub,
            RowFilter::Up,
            RowFilter::Average,
            RowFilter::Paeth,
            RowFilter::Cycle,
        ];
        let mut i = 0usize;
        while i < kinds.len() {
            let bytes = match encode_png(src.w, src.h, &src.pixels, kinds[i]) {
                Ok(v) => v,
                Err(_) => Vec::new(),
            };
            let back = parse_png(&bytes);
            match back {
                Ok(p) => {
                    assert_eq!(p.w, src.w);
                    assert_eq!(p.h, src.h);
                    assert_eq!(p.pixels, src.pixels);
                }
                Err(e) => assert!(false, "decode failed: {:?}", e),
            }
            i += 1;
        }
    }

    #[test]
    fn unx50042_placement_margin_identity() {
        let p = Placement::new(Anchor::BotRight, 1.0, 5);
        let r = match plan_rect(20, 10, 200, 100, &p) {
            Ok(v) => v,
            Err(_) => Rect::new(0, 0, 0, 0),
        };
        assert_eq!(r.x + r.w + 5, 200);
        assert_eq!(r.y + r.h + 5, 100);
    }
}
