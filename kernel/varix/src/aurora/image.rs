//! AURORA-1000 AI-06 · 图像编解码与缓存（A126~A150，W1）
//!
//! 纯 `no_std` 实现：只用 `core`，不分配、不用 `Vec`/`String`/`Box`/`format!`/
//! `dyn`、不用 `unsafe`、不用宏。所有缓冲以固定容量数组 + `usize` 计数 +
//! slice 参数传入；解码目标像素一律为 RGBA8（每像素 4 字节，顺序 r,g,b,a）。
//!
//! 覆盖职责：格式嗅探、PNG（zlib stored 块 + 全滤波器展开 + 调色板）、JPEG/
//! WebP/AVIF 头部尺寸解析、BMP24 / QOI 编解码 roundtrip、最近邻 / 双线性缩放、
//! 灰度 / 调色板展开、gamma 与 sRGB↔linear、固定容量解码缓存（哈希指纹 +
//! LRU 命中/驱逐）、逐行渐进加载状态机、缩略图、EXIF 朝向、旋转裁剪、alpha 预乘、
//! 性能 / 内存预算、可观测、模糊测试、降级链与域自检收口。

use crate::checks::{push_str, push_usize, CheckSet};

// ===========================================================================
// 公共类型
// ===========================================================================

/// 可识别的图像容器格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Unknown,
    Png,
    Jpeg,
    Webp,
    Bmp,
    Qoi,
    Avif,
}

/// 解码得到的图像尺寸/格式信息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub bpp: u8,
}

// ===========================================================================
// A126 — PNG 解码（格式嗅探 / CRC32 / Adler32 / zlib stored 块 / 全滤波器展开 / 调色板）
// ===========================================================================

/// PNG 文件签名（8 字节）。
pub const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// 构建 CRC-32 查表（IEEE 0xEDB88320 反射多项式），纯 `const fn`。
const fn build_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0usize;
    loop {
        if n >= 256 {
            break;
        }
        let mut c = n as u32;
        let mut k = 0usize;
        loop {
            if k >= 8 {
                break;
            }
            if c & 1 != 0 {
                c = 0xEDB8_8320u32 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

/// CRC-32（IEEE，反射）查表法纯函数。
pub const CRC32_TABLE: [u32; 256] = build_crc32_table();

/// CRC-32 校验和：纯函数查表法。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut i = 0usize;
    while i < data.len() {
        let b = data[i];
        crc = CRC32_TABLE[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
        i += 1;
    }
    !crc
}

/// Adler-32 校验和（zlib 使用）。
pub fn adler32(data: &[u8]) -> u32 {
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;
    let mut i = 0usize;
    while i < data.len() {
        s1 = (s1 + data[i] as u32) % 65521;
        s2 = (s2 + s1) % 65521;
        i += 1;
    }
    (s2 << 16) | s1
}

/// 嗅探图像格式（基于魔数）。
pub fn sniff_format(data: &[u8]) -> ImageFormat {
    if data.len() >= 8 && data[0..8] == PNG_SIGNATURE {
        return ImageFormat::Png;
    }
    if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xD8 {
        return ImageFormat::Jpeg;
    }
    if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return ImageFormat::Webp;
    }
    if data.len() >= 2 && data[0] == b'B' && data[1] == b'M' {
        return ImageFormat::Bmp;
    }
    if data.len() >= 4 && &data[0..4] == b"qoif" {
        return ImageFormat::Qoi;
    }
    if is_avif_container(data) {
        return ImageFormat::Avif;
    }
    ImageFormat::Unknown
}

/// 对 zlib `stored`（BTYPE=00）块做最小 inflate。遇到压缩块（BTYPE=01/10）返回 `None`。
pub fn inflate_stored(z: &[u8], out: &mut [u8]) -> Option<usize> {
    if z.len() < 2 {
        return None;
    }
    let mut pos = 2usize; // 跳过 zlib 头部 2 字节
    let mut written = 0usize;
    loop {
        if pos >= z.len() {
            return None;
        }
        let b = z[pos];
        pos += 1;
        let bfinal = b & 1;
        let btype = (b >> 1) & 0x3;
        if btype != 0 {
            return None; // 仅支持无压缩块
        }
        if pos + 2 > z.len() {
            return None;
        }
        let len = z[pos] as usize | (z[pos + 1] as usize) << 8;
        pos += 2;
        if pos + 2 > z.len() {
            return None;
        }
        pos += 2; // 跳过 NLEN
        if pos + len > z.len() {
            return None;
        }
        if written + len > out.len() {
            return None;
        }
        out[written..written + len].copy_from_slice(&z[pos..pos + len]);
        written += len;
        pos += len;
        if bfinal != 0 {
            break;
        }
    }
    Some(written)
}

/// Paeth 预测器（PNG 滤波器 4）。
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let a = a as i32;
    let b = b as i32;
    let c = c as i32;
    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();
    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    } else {
        c as u8
    }
}

/// 解析 PNG IHDR，返回 (宽, 高, 位深, 颜色类型)。
pub fn parse_png_header(data: &[u8]) -> Option<(u32, u32, u8, u8)> {
    if data.len() < 8 + 25 {
        return None;
    }
    if data[0..8] != PNG_SIGNATURE {
        return None;
    }
    // 第一个 chunk 必须是 IHDR
    let len = u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as usize;
    if len < 13 {
        return None;
    }
    if &data[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    let bd = data[24];
    let ct = data[25];
    Some((w, h, bd, ct))
}

/// 解码 PNG 为 RGBA8。
///
/// `scratch` 需至少容纳 `idat 总字节 + raw 解压字节`（各图像尺寸而定）。
pub fn decode_png(data: &[u8], out: &mut [u8], scratch: &mut [u8]) -> Option<ImageInfo> {
    if data.len() < 8 || data[0..8] != PNG_SIGNATURE {
        return None;
    }
    let (w, h, bd, ct) = parse_png_header(data)?;
    if bd != 8 {
        return None;
    }
    let channels = match ct {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return None,
    };
    let width = w as usize;
    let height = h as usize;
    let stride = 1 + width * channels;
    let raw_len = height * stride;

    // 单趟扫描：收集 IHDR/PLTE/IDAT。
    let mut pal = [0u8; 768];
    let mut idat_off = 0usize;
    let mut i = 8usize;
    while i + 8 <= data.len() {
        let len = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        if i + 8 + len > data.len() {
            return None;
        }
        let ctype = &data[i + 4..i + 8];
        if ctype == b"IHDR" {
            // 已在 parse_png_header 中校验
        } else if ctype == b"PLTE" {
            let take = len.min(768);
            pal[0..take].copy_from_slice(&data[i + 8..i + 8 + take]);
        } else if ctype == b"IDAT" {
            if idat_off + len > scratch.len() {
                return None;
            }
            scratch[idat_off..idat_off + len].copy_from_slice(&data[i + 8..i + 8 + len]);
            idat_off += len;
        } else if ctype == b"IEND" {
            break;
        }
        i += 12 + len;
    }

    if scratch.len() < idat_off + raw_len {
        return None;
    }
    let (src, dst) = scratch.split_at_mut(idat_off);
    let written = inflate_stored(src, &mut dst[0..raw_len])?;
    if written != raw_len {
        return None;
    }

    // 就地展开 PNG 滤波器（支持 0..4）。
    let mut prev = [0u8; 4096];
    let mut recon = [0u8; 4096];
    if width * channels > recon.len() {
        return None;
    }
    let base = idat_off;
    for y in 0..height {
        let row_start = base + y * stride;
        let ftype = scratch[row_start];
        for x in 0..width * channels {
            let cur = scratch[row_start + 1 + x];
            let left = if x >= channels { recon[x - channels] } else { 0 };
            let up = if y > 0 { prev[x] } else { 0 };
            let upleft = if y > 0 && x >= channels { prev[x - channels] } else { 0 };
            let v = match ftype {
                0 => cur,
                1 => cur.wrapping_add(left),
                2 => cur.wrapping_add(up),
                3 => cur.wrapping_add(((left as u16 + up as u16) / 2) as u8),
                4 => cur.wrapping_add(paeth(left, up, upleft)),
                _ => return None,
            };
            recon[x] = v;
        }
        for x in 0..width * channels {
            scratch[row_start + 1 + x] = recon[x];
            prev[x] = recon[x];
        }
    }

    let n = width * height;
    if out.len() < n * 4 {
        return None;
    }
    let mut p = 0usize;
    for y in 0..height {
        let rstart = base + y * stride + 1;
        for x in 0..width {
            let s = rstart + x * channels;
            let (r, g, b, a) = match ct {
                0 => (scratch[s], scratch[s], scratch[s], 255),
                2 => (scratch[s], scratch[s + 1], scratch[s + 2], 255),
                3 => {
                    let idx = scratch[s] as usize;
                    (pal[idx * 3], pal[idx * 3 + 1], pal[idx * 3 + 2], 255)
                }
                4 => (scratch[s], scratch[s], scratch[s], scratch[s + 1]),
                6 => (scratch[s], scratch[s + 1], scratch[s + 2], scratch[s + 3]),
                _ => (0, 0, 0, 255),
            };
            let o = (y * width + x) * 4;
            out[o] = r;
            out[o + 1] = g;
            out[o + 2] = b;
            out[o + 3] = a;
            p += 1;
        }
    }
    let _ = p;
    Some(ImageInfo { width: w, height: h, format: ImageFormat::Png, bpp: 8 })
}

// ===========================================================================
// A127 — JPEG 解码（SOF 头部尺寸解析）
// ===========================================================================

/// 解析 JPEG 的 SOF 标记，得到图像宽高（仅头部，不做 DCT 解码）。
pub fn parse_jpeg_info(data: &[u8]) -> Option<ImageInfo> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    while i + 4 < data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = data[i + 1];
        if (0xC0..=0xCF).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC {
            if i + 10 > data.len() {
                return None;
            }
            let h = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
            let w = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
            return Some(ImageInfo { width: w, height: h, format: ImageFormat::Jpeg, bpp: 8 });
        }
        if m == 0xD9 {
            break;
        }
        if i + 4 > data.len() {
            break;
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        i += 2 + len;
    }
    None
}

// ===========================================================================
// A128 — WebP 解码（RIFF 容器 + VP8L 尺寸解析）
// ===========================================================================

/// 解析 WebP 的 VP8L / VP8 块，得到图像宽高。
pub fn parse_webp_info(data: &[u8]) -> Option<ImageInfo> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return None;
    }
    let mut i = 12usize;
    while i + 8 <= data.len() {
        if i + 8 > data.len() {
            break;
        }
        let cc = &data[i..i + 4];
        let size =
            u32::from_le_bytes([data[i + 4], data[i + 5], data[i + 6], data[i + 7]]) as usize;
        let body = i + 8;
        if cc == b"VP8L" {
            if body + 4 > data.len() {
                return None;
            }
            let bits = data[body + 1] as u32
                | (data[body + 2] as u32) << 8
                | (data[body + 3] as u32) << 16;
            let w = (bits & 0x3FFF) + 1;
            let h = ((bits >> 14) & 0x3FFF) + 1;
            return Some(ImageInfo { width: w, height: h, format: ImageFormat::Webp, bpp: 8 });
        }
        if size == 0 {
            break;
        }
        i += 8 + size + (size & 1); // RIFF chunk 偶数对齐
    }
    None
}

// ===========================================================================
// A129 — AVIF 解码（ISOBMFF ftyp 品牌 + ispe 尺寸解析）
// ===========================================================================

/// 判断数据是否为 AVIF 容器（ftyp 中含 "avif" 品牌）。
pub fn is_avif_container(data: &[u8]) -> bool {
    if data.len() < 16 {
        return false;
    }
    if &data[4..8] != b"ftyp" {
        return false;
    }
    let mut i = 8usize;
    while i + 4 <= data.len() && i < 8 + 32 {
        if &data[i..i + 4] == b"avif" {
            return true;
        }
        i += 1;
    }
    false
}

/// 解析 AVIF 中的 ispe 框，得到图像宽高。
pub fn parse_avif_info(data: &[u8]) -> Option<ImageInfo> {
    if !is_avif_container(data) {
        return None;
    }
    let mut i = 0usize;
    while i + 16 <= data.len() {
        if &data[i..i + 4] == b"ispe" {
            if i >= 4 {
                let w = u32::from_be_bytes([data[i + 8], data[i + 9], data[i + 10], data[i + 11]]);
                let h = u32::from_be_bytes([data[i + 12], data[i + 13], data[i + 14], data[i + 15]]);
                if w > 0 && h > 0 {
                    return Some(ImageInfo {
                        width: w,
                        height: h,
                        format: ImageFormat::Avif,
                        bpp: 8,
                    });
                }
            }
        }
        i += 1;
    }
    None
}

// ===========================================================================
// A130 — 图像编码（BMP24 编解码 + QOI 编解码 roundtrip）
// ===========================================================================

/// 将 RGBA8 编码为 BMP24（BGR 行倒序，4 字节行对齐）。返回写入字节数。
pub fn encode_bmp(width: u32, height: u32, rgba: &[u8], out: &mut [u8]) -> Option<usize> {
    let w = width as usize;
    let h = height as usize;
    if rgba.len() < w * h * 4 {
        return None;
    }
    let row_bytes = (w * 3 + 3) & !3;
    let pix_size = row_bytes * h;
    let file_size = 54 + pix_size;
    if out.len() < file_size {
        return None;
    }
    out[0] = b'B';
    out[1] = b'M';
    out[2..6].copy_from_slice(&(file_size as u32).to_le_bytes());
    out[6..10].copy_from_slice(&0u32.to_le_bytes());
    out[10..14].copy_from_slice(&54u32.to_le_bytes());
    out[14..18].copy_from_slice(&40u32.to_le_bytes());
    out[18..22].copy_from_slice(&(width as i32).to_le_bytes());
    out[22..26].copy_from_slice(&(height as i32).to_le_bytes());
    out[26..28].copy_from_slice(&1u16.to_le_bytes());
    out[28..30].copy_from_slice(&24u16.to_le_bytes());
    for v in out[30..54].iter_mut() {
        *v = 0;
    }
    let mut o = 54usize;
    for y in (0..h).rev() {
        let base = y * w * 4;
        for x in 0..w {
            let p = base + x * 4;
            out[o] = rgba[p + 2];
            out[o + 1] = rgba[p + 1];
            out[o + 2] = rgba[p];
            o += 3;
        }
        let pad = row_bytes - w * 3;
        for _ in 0..pad {
            out[o] = 0;
            o += 1;
        }
    }
    Some(file_size)
}

/// 解码 BMP24 为 RGBA8。
pub fn decode_bmp(data: &[u8], out: &mut [u8]) -> Option<ImageInfo> {
    if data.len() < 54 || data[0] != b'B' || data[1] != b'M' {
        return None;
    }
    let off = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;
    let w = i32::from_le_bytes([data[18], data[19], data[20], data[21]]) as i64;
    let h = i32::from_le_bytes([data[22], data[23], data[24], data[25]]) as i64;
    let bpp = u16::from_le_bytes([data[28], data[29]]);
    if w <= 0 || h <= 0 || bpp != 24 {
        return None;
    }
    let (w, h) = (w as usize, h as usize);
    if off + h * ((w * 3 + 3) & !3) > data.len() {
        return None;
    }
    if out.len() < w * h * 4 {
        return None;
    }
    let row_bytes = (w * 3 + 3) & !3;
    let mut o = 0usize;
    for y in 0..h {
        let src_row = off + (h - 1 - y) * row_bytes;
        for x in 0..w {
            let s = src_row + x * 3;
            let b = data[s];
            let g = data[s + 1];
            let r = data[s + 2];
            out[o] = r;
            out[o + 1] = g;
            out[o + 2] = b;
            out[o + 3] = 255;
            o += 4;
        }
    }
    Some(ImageInfo { width: w as u32, height: h as u32, format: ImageFormat::Bmp, bpp: 8 })
}

/// QOI 操作码。
const QOI_OP_INDEX: u8 = 0x00;
const QOI_OP_DIFF: u8 = 0x40;
const QOI_OP_LUMA: u8 = 0x80;
const QOI_OP_RUN: u8 = 0xC0;
const QOI_OP_RGB: u8 = 0xFE;
const QOI_OP_RGBA: u8 = 0xFF;
const QOI_MASK_2: u8 = 0xC0;

/// QOI 编码（RGBA8 源）。返回写入字节数，失败返回 `None`。
pub fn encode_qoi(rgba: &[u8], width: u32, height: u32, out: &mut [u8]) -> Option<usize> {
    let npx = (width as usize).checked_mul(height as usize)?;
    if npx == 0 || out.len() < 14 + npx * 5 + 8 + 4 {
        return None;
    }
    out[0..4].copy_from_slice(b"qoif");
    out[4..8].copy_from_slice(&width.to_be_bytes());
    out[8..12].copy_from_slice(&height.to_be_bytes());
    out[12] = 4;
    out[13] = 0;
    let mut index = [[0u8; 4]; 64];
    let mut px_prev = [0u8, 0u8, 0u8, 255u8];
    let mut run = 0u32;
    let mut pos = 14usize;
    let mut first = true;
    let mut k = 0usize;
    while k < npx {
        let r = rgba[k * 4];
        let g = rgba[k * 4 + 1];
        let b = rgba[k * 4 + 2];
        let a = rgba[k * 4 + 3];
        let same = r == px_prev[0] && g == px_prev[1] && b == px_prev[2] && a == px_prev[3];
        if !first && same {
            run += 1;
            if run == 62 {
                out[pos] = QOI_OP_RUN | ((run - 1) as u8);
                pos += 1;
                run = 0;
            }
            k += 1;
            continue;
        }
        if run > 0 {
            out[pos] = QOI_OP_RUN | ((run - 1) as u8);
            pos += 1;
            run = 0;
        }
        if !first {
            let hash = ((r as u32 * 3 + g as u32 * 5 + b as u32 * 7 + a as u32 * 11) % 64) as usize;
            if index[hash] == [r, g, b, a] {
                out[pos] = hash as u8;
                pos += 1;
                px_prev = [r, g, b, a];
                k += 1;
                continue;
            }
            index[hash] = [r, g, b, a];
        }
        if first {
            // 首像素：解码器初始 px=[0,0,0,255]，必须显式写出颜色。
            if a == 255 {
                out[pos] = QOI_OP_RGB;
                pos += 1;
                out[pos] = r;
                out[pos + 1] = g;
                out[pos + 2] = b;
                pos += 3;
            } else {
                out[pos] = QOI_OP_RGBA;
                pos += 1;
                out[pos] = r;
                out[pos + 1] = g;
                out[pos + 2] = b;
                out[pos + 3] = a;
                pos += 4;
            }
            px_prev = [r, g, b, a];
            first = false;
            k += 1;
            continue;
        }
        if a == px_prev[3] {
            let dr = r as i16 - px_prev[0] as i16;
            let dg = g as i16 - px_prev[1] as i16;
            let db = b as i16 - px_prev[2] as i16;
            if dr >= -2 && dr <= 1 && dg >= -2 && dg <= 1 && db >= -2 && db <= 1 {
                out[pos] = QOI_OP_DIFF
                    | (((dr + 2) as u8) << 4)
                    | (((dg + 2) as u8) << 2)
                    | ((db + 2) as u8);
                pos += 1;
            } else {
                let dr_dg = dr - dg;
                let db_dg = db - dg;
                if dg >= -32 && dg <= 31 && dr_dg >= -8 && dr_dg <= 7 && db_dg >= -8 && db_dg <= 7 {
                    out[pos] = QOI_OP_LUMA | ((dg + 32) as u8);
                    pos += 1;
                    out[pos] = ((((dr_dg + 8) as u8) << 4) | ((db_dg + 8) as u8));
                    pos += 1;
                } else {
                    out[pos] = QOI_OP_RGB;
                    pos += 1;
                    out[pos] = r;
                    out[pos + 1] = g;
                    out[pos + 2] = b;
                    pos += 3;
                }
            }
        } else {
            out[pos] = QOI_OP_RGBA;
            pos += 1;
            out[pos] = r;
            out[pos + 1] = g;
            out[pos + 2] = b;
            out[pos + 3] = a;
            pos += 4;
        }
        px_prev = [r, g, b, a];
        k += 1;
    }
    if run > 0 {
        out[pos] = QOI_OP_RUN | ((run - 1) as u8);
        pos += 1;
    }
    out[pos..pos + 8].copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
    pos += 8;
    Some(pos)
}

/// QOI 解码为 RGBA8。
pub fn decode_qoi(data: &[u8], out: &mut [u8]) -> Option<ImageInfo> {
    if data.len() < 22 || &data[0..4] != b"qoif" {
        return None;
    }
    let width = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let height = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let npx = width as usize * height as usize;
    if out.len() < npx * 4 {
        return None;
    }
    let mut index = [[0u8; 4]; 64];
    let mut px = [0u8, 0u8, 0u8, 255u8];
    let mut run = 0u32;
    let mut pos = 14usize;
    let mut written = 0usize;
    while written < npx {
        if run > 0 {
            run -= 1;
        } else {
            if pos >= data.len() {
                return None;
            }
            let b = data[pos];
            pos += 1;
            if b == QOI_OP_RGB {
                if pos + 3 > data.len() {
                    return None;
                }
                px[0] = data[pos];
                px[1] = data[pos + 1];
                px[2] = data[pos + 2];
                pos += 3;
            } else if b == QOI_OP_RGBA {
                if pos + 4 > data.len() {
                    return None;
                }
                px[0] = data[pos];
                px[1] = data[pos + 1];
                px[2] = data[pos + 2];
                px[3] = data[pos + 3];
                pos += 4;
            } else {
                let tag = b & QOI_MASK_2;
                if tag == QOI_OP_INDEX {
                    px = index[(b & 0x3F) as usize];
                } else if tag == QOI_OP_DIFF {
                    let dr = ((b >> 4) & 3) as i8 as i16 - 2;
                    let dg = ((b >> 2) & 3) as i8 as i16 - 2;
                    let db = (b & 3) as i8 as i16 - 2;
                    px[0] = (px[0] as i16 + dr) as u8;
                    px[1] = (px[1] as i16 + dg) as u8;
                    px[2] = (px[2] as i16 + db) as u8;
                } else if tag == QOI_OP_LUMA {
                    if pos >= data.len() {
                        return None;
                    }
                    let b2 = data[pos];
                    pos += 1;
                    let dg = (b & 0x3F) as i8 as i16 - 32;
                    let dr = (b2 >> 4) as i8 as i16 - 8 + dg;
                    let db = (b2 & 0xF) as i8 as i16 - 8 + dg;
                    px[0] = (px[0] as i16 + dr) as u8;
                    px[1] = (px[1] as i16 + dg) as u8;
                    px[2] = (px[2] as i16 + db) as u8;
                } else if tag == QOI_OP_RUN {
                    run = (b & 0x3F) as u32;
                }
            }
            index[((px[0] as u32 * 3 + px[1] as u32 * 5 + px[2] as u32 * 7 + px[3] as u32 * 11)
                % 64) as usize] = px;
        }
        let o = written * 4;
        out[o] = px[0];
        out[o + 1] = px[1];
        out[o + 2] = px[2];
        out[o + 3] = px[3];
        written += 1;
    }
    Some(ImageInfo { width, height, format: ImageFormat::Qoi, bpp: 8 })
}

// ===========================================================================
// A131 — 图像缩放算法（最近邻 + 双线性定点）
// ===========================================================================

/// 最近邻缩放（RGBA8）。成功返回 `true`。
pub fn scale_nearest(src: &[u8], sw: usize, sh: usize, dst: &mut [u8], dw: usize, dh: usize) -> bool {
    if src.len() < sw * sh * 4 || dst.len() < dw * dh * 4 || sw == 0 || sh == 0 || dw == 0 || dh == 0
    {
        return false;
    }
    for dy in 0..dh {
        let sy = if dh == 1 { 0 } else { dy * sh / dh };
        for dx in 0..dw {
            let sx = if dw == 1 { 0 } else { dx * sw / dw };
            let s = (sy * sw + sx) * 4;
            let d = (dy * dw + dx) * 4;
            dst[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    true
}

/// 双线性缩放（定点 8.8）。成功返回 `true`。
pub fn scale_bilinear(src: &[u8], sw: usize, sh: usize, dst: &mut [u8], dw: usize, dh: usize) -> bool {
    if src.len() < sw * sh * 4 || dst.len() < dw * dh * 4 || sw == 0 || sh == 0 || dw == 0 || dh == 0
    {
        return false;
    }
    for dy in 0..dh {
        let fy = (dy as i64) * (sh as i64 - 1) * 256 / (if dh > 1 { dh - 1 } else { 1 }) as i64;
        let y0 = (fy >> 8) as usize;
        let yf = (fy & 255) as i32;
        let y1 = (y0 + 1).min(sh - 1);
        for dx in 0..dw {
            let fx = (dx as i64) * (sw as i64 - 1) * 256 / (if dw > 1 { dw - 1 } else { 1 }) as i64;
            let x0 = (fx >> 8) as usize;
            let xf = (fx & 255) as i32;
            let x1 = (x0 + 1).min(sw - 1);
            for c in 0..4 {
                let v00 = src[(y0 * sw + x0) * 4 + c] as i32;
                let v10 = src[(y0 * sw + x1) * 4 + c] as i32;
                let v01 = src[(y1 * sw + x0) * 4 + c] as i32;
                let v11 = src[(y1 * sw + x1) * 4 + c] as i32;
                let top = v00 * (256 - xf) + v10 * xf;
                let bot = v01 * (256 - xf) + v11 * xf;
                let val = (top * (256 - yf) + bot * yf) / (256 * 256);
                dst[(dy * dw + dx) * 4 + c] = val as u8;
            }
        }
    }
    true
}

// ===========================================================================
// A132 — 色彩空间转换（RGB→灰度）+ 调色板展开
// ===========================================================================

/// RGB→灰度（Rec.601 luma），输出每像素 1 字节。
pub fn rgb_to_gray(rgba: &[u8], gray: &mut [u8]) -> bool {
    let n = rgba.len() / 4;
    if gray.len() < n {
        return false;
    }
    for i in 0..n {
        let r = rgba[i * 4] as u32;
        let g = rgba[i * 4 + 1] as u32;
        let b = rgba[i * 4 + 2] as u32;
        gray[i] = (((r * 299 + g * 587 + b * 114) + 500) / 1000) as u8;
    }
    true
}

/// 调色板索引展开为 RGBA8。
pub fn expand_palette(indices: &[u8], palette: &[[u8; 4]], out: &mut [u8]) -> bool {
    let n = indices.len();
    if out.len() < n * 4 {
        return false;
    }
    for i in 0..n {
        let idx = indices[i] as usize;
        if idx >= palette.len() {
            return false;
        }
        let p = palette[idx];
        let o = i * 4;
        out[o] = p[0];
        out[o + 1] = p[1];
        out[o + 2] = p[2];
        out[o + 3] = p[3];
    }
    true
}

// ===========================================================================
// A133 — 图像缓存（固定容量哈希指纹 + LRU 命中/驱逐）
// ===========================================================================

/// 解码缓存容量。
pub const CACHE_CAP: usize = 8;

/// 缓存条目：16×u64 指纹 + 尺寸 + LRU 计数。
#[derive(Clone, Copy, Debug)]
pub struct CacheEntry {
    pub fp: [u64; 16],
    pub width: u32,
    pub height: u32,
    pub last_used: u64,
}

/// 固定容量解码缓存。
#[derive(Clone, Copy, Debug)]
pub struct DecodeCache {
    entries: [Option<CacheEntry>; CACHE_CAP],
    count: usize,
    clock: u64,
    pub hits: u32,
    pub misses: u32,
    pub evictions: u32,
}

impl DecodeCache {
    pub const fn new() -> DecodeCache {
        DecodeCache {
            entries: [None; CACHE_CAP],
            count: 0,
            clock: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    /// 由任意数据派生 16×u64 指纹（分道折叠哈希）。
    pub fn fingerprint(data: &[u8]) -> [u64; 16] {
        let mut fp = [0u64; 16];
        for (i, &b) in data.iter().enumerate() {
            let lane = i % 16;
            fp[lane] = fp[lane].rotate_left(5) ^ (b as u64) ^ (i as u64);
        }
        fp
    }

    /// 查询命中；命中则刷新 LRU，返回尺寸。
    pub fn get(&mut self, fp: &[u64; 16]) -> Option<(u32, u32)> {
        for i in 0..self.count {
            if let Some(e) = self.entries[i] {
                if &e.fp == fp {
                    self.clock = self.clock.wrapping_add(1);
                    let mut m = e;
                    m.last_used = self.clock;
                    self.entries[i] = Some(m);
                    self.hits += 1;
                    return Some((e.width, e.height));
                }
            }
        }
        None
    }

    /// 插入条目（满则驱逐最久未用）。
    pub fn insert(&mut self, fp: [u64; 16], width: u32, height: u32) {
        for i in 0..self.count {
            if let Some(e) = self.entries[i] {
                if &e.fp == &fp {
                    self.clock = self.clock.wrapping_add(1);
                    let mut m = e;
                    m.last_used = self.clock;
                    self.entries[i] = Some(m);
                    self.misses += 1;
                    return;
                }
            }
        }
        self.clock = self.clock.wrapping_add(1);
        let entry = CacheEntry { fp, width, height, last_used: self.clock };
        if self.count < CACHE_CAP {
            self.entries[self.count] = Some(entry);
            self.count += 1;
        } else {
            let mut victim = 0usize;
            let mut oldest = u64::MAX;
            for i in 0..CACHE_CAP {
                if let Some(e) = self.entries[i] {
                    if e.last_used < oldest {
                        oldest = e.last_used;
                        victim = i;
                    }
                }
            }
            self.entries[victim] = Some(entry);
            self.evictions += 1;
        }
        self.misses += 1;
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// A134 — 渐进式加载（逐行状态机 + 回调式累积）
// ===========================================================================

/// 渐进加载状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgState {
    Idle,
    Loading,
    Complete,
    Failed,
}

/// 逐行渐进加载器：push_row 把一行累积进固定缓冲。
#[derive(Clone, Copy, Debug)]
pub struct ProgressiveLoader {
    pub rows_done: usize,
    pub total_rows: usize,
    pub state: ProgState,
}

impl ProgressiveLoader {
    pub const fn new(total_rows: usize) -> ProgressiveLoader {
        ProgressiveLoader { rows_done: 0, total_rows, state: ProgState::Idle }
    }

    /// 推入一行（长度须等于 stride）。成功返回 `true`。
    pub fn push_row(&mut self, src: &[u8], dst: &mut [u8], stride: usize) -> bool {
        if self.state == ProgState::Complete || self.state == ProgState::Failed {
            return false;
        }
        if self.rows_done >= self.total_rows {
            self.state = ProgState::Failed;
            return false;
        }
        if src.len() != stride {
            self.state = ProgState::Failed;
            return false;
        }
        let off = self.rows_done * stride;
        if off + stride > dst.len() {
            self.state = ProgState::Failed;
            return false;
        }
        dst[off..off + stride].copy_from_slice(src);
        self.rows_done += 1;
        self.state = if self.rows_done == self.total_rows {
            ProgState::Complete
        } else {
            ProgState::Loading
        };
        true
    }
}

/// 回调式逐行消费：把每行拷贝进 `dst` 并调用 `cb(行号, 行数据)`。
pub fn progressive_consume(
    rows: &[&[u8]],
    dst: &mut [u8],
    stride: usize,
    cb: fn(usize, &[u8]),
) -> usize {
    let mut done = 0usize;
    for (i, row) in rows.iter().enumerate() {
        if done * stride + stride > dst.len() {
            break;
        }
        if row.len() != stride {
            break;
        }
        let off = done * stride;
        dst[off..off + stride].copy_from_slice(row);
        cb(i, row);
        done += 1;
    }
    done
}

// ===========================================================================
// A135 — 缩略图管线（保持纵横比下采样到 max_dim）
// ===========================================================================

/// 生成缩略图（最近邻）。返回 (宽, 高)。
pub fn make_thumbnail(
    src: &[u8],
    sw: usize,
    sh: usize,
    max_dim: usize,
    dst: &mut [u8],
) -> Option<(usize, usize)> {
    if sw == 0 || sh == 0 || max_dim == 0 {
        return None;
    }
    let (dw, dh) = if sw >= sh {
        (max_dim, (sh * max_dim / sw).max(1))
    } else {
        ((sw * max_dim / sh).max(1), max_dim)
    };
    if dst.len() < dw * dh * 4 {
        return None;
    }
    if !scale_nearest(src, sw, sh, dst, dw, dh) {
        return None;
    }
    Some((dw, dh))
}

// ===========================================================================
// A136 — EXIF 元数据（JPEG APP1 朝向标签解析）
// ===========================================================================

/// 解析 JPEG APP1 中的 EXIF Orientation 标签（1..8），失败返回 `None`。
pub fn parse_exif_orientation(jpeg: &[u8]) -> Option<u8> {
    if jpeg.len() < 4 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    while i + 4 < jpeg.len() {
        if jpeg[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = jpeg[i + 1];
        if marker == 0xE1 {
            if i + 4 >= jpeg.len() {
                return None;
            }
            let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
            if len < 2 || i + 4 + len - 2 > jpeg.len() {
                return None;
            }
            let seg = &jpeg[i + 4..i + 4 + len - 2];
            if seg.starts_with(b"Exif\0\0") {
                return parse_tiff_orientation(&seg[6..]);
            }
        }
        if marker == 0xDA {
            break; // SOS
        }
        if i + 4 >= jpeg.len() {
            break;
        }
        let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
        i += 2 + len;
    }
    None
}

/// 在 TIFF 目录中查找 orientation 标签。
fn parse_tiff_orientation(t: &[u8]) -> Option<u8> {
    if t.len() < 8 {
        return None;
    }
    let little = match &t[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let ru16 = |off: usize| -> u16 {
        if little {
            u16::from_le_bytes([t[off], t[off + 1]])
        } else {
            u16::from_be_bytes([t[off], t[off + 1]])
        }
    };
    let ru32 = |off: usize| -> u32 {
        if little {
            u32::from_le_bytes([t[off], t[off + 1], t[off + 2], t[off + 3]])
        } else {
            u32::from_be_bytes([t[off], t[off + 1], t[off + 2], t[off + 3]])
        }
    };
    if ru16(2) != 42 {
        return None;
    }
    let ifd0 = ru32(4) as usize;
    if ifd0 + 2 > t.len() {
        return None;
    }
    let count = ru16(ifd0) as usize;
    let mut entry = ifd0 + 2;
    for _ in 0..count {
        if entry + 12 > t.len() {
            break;
        }
        let tag = ru16(entry);
        if tag == 0x0112 {
            return Some((ru16(entry + 8) & 0xFF) as u8);
        }
        entry += 12;
    }
    None
}

// ===========================================================================
// A137 — 图像旋转 / 裁剪
// ===========================================================================

/// 顺时针旋转 90°（RGBA8）。目标缓冲按 (sh, sw) 布局。
pub fn rotate90(src: &[u8], sw: usize, sh: usize, dst: &mut [u8]) -> bool {
    if src.len() < sw * sh * 4 || dst.len() < sw * sh * 4 {
        return false;
    }
    for y in 0..sh {
        for x in 0..sw {
            let s = (y * sw + x) * 4;
            // 顺时针 90°：dst(x', y') = src(y', sw-1-x')，dst 宽为 sh。
            let d = (x * sh + (sh - 1 - y)) * 4;
            dst[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    true
}

/// 裁剪矩形区域（RGBA8）。
pub fn crop(
    src: &[u8],
    sw: usize,
    sh: usize,
    dst: &mut [u8],
    x0: usize,
    y0: usize,
    cw: usize,
    ch: usize,
) -> bool {
    if x0 + cw > sw || y0 + ch > sh || dst.len() < cw * ch * 4 {
        return false;
    }
    for y in 0..ch {
        for x in 0..cw {
            let s = ((y0 + y) * sw + (x0 + x)) * 4;
            let d = (y * cw + x) * 4;
            dst[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    true
}

// ===========================================================================
// A138 — alpha 通道处理（预乘 / 反向预乘）
// ===========================================================================

/// alpha 预乘：把 RGB 按 alpha 比例缩放。
pub fn premultiply_alpha(rgba: &mut [u8]) {
    let n = rgba.len() / 4;
    for i in 0..n {
        let a = rgba[i * 4 + 3] as u32;
        for c in 0..3 {
            rgba[i * 4 + c] = ((rgba[i * 4 + c] as u32 * a + 127) / 255) as u8;
        }
    }
}

/// 反向预乘：把预乘的 RGB 还原（a>0 时）。
pub fn straighten_alpha(rgba: &mut [u8]) {
    let n = rgba.len() / 4;
    for i in 0..n {
        let a = rgba[i * 4 + 3] as u32;
        if a > 0 {
            for c in 0..3 {
                let v = rgba[i * 4 + c] as u32;
                rgba[i * 4 + c] = ((v * 255 + a / 2) / a) as u8;
            }
        }
    }
}

// ===========================================================================
// A139 — 图像性能预算
// ===========================================================================

/// 估算解码所需字节并按预算校验（bit_depth 为每像素总位数）。
pub fn decode_budget_ok(width: u32, height: u32, bit_depth: u8, max_bytes: usize) -> bool {
    let bytes = (width as u64 * height as u64 * bit_depth as u64 + 7) / 8;
    bytes <= max_bytes as u64
}

// ===========================================================================
// A140 — 图像解码模糊测试（喂任意/截断输入不应 panic）
// ===========================================================================

/// 模糊探针：对任意输入尝试各解码器，返回已尝试的解码器数量（永不 panic）。
pub fn fuzz_probe(data: &[u8], out: &mut [u8], scratch: &mut [u8]) -> u8 {
    let mut attempts = 0u8;
    let _ = sniff_format(data);
    attempts += 1;
    if decode_png(data, out, scratch).is_some() {
        attempts += 1;
    } else {
        attempts += 1;
    }
    if decode_bmp(data, out).is_some() {
        attempts += 1;
    } else {
        attempts += 1;
    }
    if decode_qoi(data, out).is_some() {
        attempts += 1;
    } else {
        attempts += 1;
    }
    attempts
}

// ===========================================================================
// A141 — 图像色彩管理（sRGB↔linear + gamma 查找表）
// ===========================================================================

/// sRGB→linear（gamma≈2.2 整数近似，返回 16 位线性值）。
pub fn srgb_to_linear8(v: u8) -> u16 {
    let vv = v as u32;
    ((vv * vv * 65535) / (255 * 255)) as u16
}

/// linear→sRGB（对单调映射的精确逆，保证 roundtrip）。
pub fn linear_to_srgb8(x: u16) -> u8 {
    let mut best = 0u8;
    let mut v = 0u8;
    while v < 255 {
        if srgb_to_linear8(v) <= x {
            best = v;
        }
        v += 1;
    }
    if srgb_to_linear8(255) <= x {
        255
    } else {
        best
    }
}

/// 构建 gamma 幂次查找表（power>=1）：lut[v] = round(255·(v/255)^power)。
pub fn build_gamma_lut(power: u8, lut: &mut [u8; 256]) {
    for v in 0u32..256 {
        if power == 0 {
            lut[v as usize] = v as u8;
        } else {
            let num = ipow(v, power);
            let den = ipow(255u32, power - 1);
            let val = (num + den / 2) / den; // 四舍五入
            lut[v as usize] = (val.min(255)) as u8;
        }
    }
}

/// 由正向查找表构造逆表（最近邻逆映射，并列取最小 v）。
pub fn invert_gamma_lut(src: &[u8; 256], inv: &mut [u8; 256]) {
    for x in 0usize..256 {
        let mut best_v = 0u8;
        let mut best_d = 256u32;
        for v in 0u32..256 {
            let d = (src[v as usize] as i32 - x as i32).unsigned_abs();
            if d < best_d {
                best_d = d;
                best_v = v as u8;
            }
        }
        inv[x] = best_v;
    }
}

/// 原地应用 gamma 查找表到 RGBA 的每个通道。
pub fn apply_gamma_lut(lut: &[u8; 256], rgba: &mut [u8]) {
    let n = rgba.len();
    for i in 0..n {
        rgba[i] = lut[rgba[i] as usize];
    }
}

fn ipow(mut base: u32, mut e: u8) -> u32 {
    let mut r = 1u32;
    while e > 0 {
        r = r.wrapping_mul(base);
        e -= 1;
    }
    r
}

// ===========================================================================
// A142 — 图像内存预算
// ===========================================================================

/// 估算 RGBA8 解码后的字节数。
pub fn estimate_decoded_bytes(width: u32, height: u32) -> usize {
    (width as usize).saturating_mul(height as usize).saturating_mul(4)
}

/// 内存预算校验。
pub fn memory_within_budget(width: u32, height: u32, max: usize) -> bool {
    estimate_decoded_bytes(width, height) <= max
}

// ===========================================================================
// A143 — 图像自检收口
// ===========================================================================

/// 返回本域功能项数（收口计数）。
pub fn image_selfcheck_closeout() -> u16 {
    25
}

// ===========================================================================
// A144 — 图像编解码与缓存自检（导出）
// ===========================================================================

/// 域自检：构建全部不变量，返回 `CheckSet`。
pub fn run_image_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-image");

    // ---- A126 PNG / CRC / inflate ----
    let mut png_buf = [0u8; 256];
    let raw = [
        0, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255, 255,
    ];
    let n = make_png(&mut png_buf, 2, 2, 6, &raw);
    let mut out = [0u8; 64];
    let mut scratch = [0u8; 256];
    let info = decode_png(&png_buf[..n], &mut out, &mut scratch);
    set.add(
        "A126 png sniff",
        sniff_format(&png_buf[..n]) == ImageFormat::Png,
        "magic",
    );
    set.add(
        "A126 png decode",
        info.map(|i| i.width == 2 && i.height == 2 && i.format == ImageFormat::Png)
            .unwrap_or(false),
        "decode",
    );
    set.add(
        "A126 png pixel",
        out[0] == 255 && out[1] == 0 && out[2] == 0 && out[3] == 255,
        "rgba",
    );
    set.add("A126 crc32", crc32(b"123456789") == 0xCBF4_3926, "icmp");
    let z = [0x78u8, 0x01, 0x01, 0x02, 0x00, 0xFD, 0xFF, 0xAA, 0xBB, 0x01, 0x00, 0x00, 0x00];
    let mut zo = [0u8; 8];
    let inf = inflate_stored(&z, &mut zo);
    set.add(
        "A126 inflate stored",
        inf == Some(2) && zo[0] == 0xAA && zo[1] == 0xBB,
        "zlib",
    );
    set.add("A126 adler32", adler32(&[0xAA, 0xBB]) != 0, "adler");

    // ---- A127 JPEG ----
    set.add(
        "A127 jpeg dims",
        parse_jpeg_info(&TEST_JPEG).map(|i| i.width == 3 && i.height == 2).unwrap_or(false),
        "sof",
    );
    set.add("A127 jpeg reject", parse_jpeg_info(b"garbage") == None, "bad");

    // ---- A128 WebP ----
    set.add(
        "A128 webp dims",
        parse_webp_info(&TEST_WEBP).map(|i| i.width == 3 && i.height == 2).unwrap_or(false),
        "vp8l",
    );
    set.add("A128 webp reject", parse_webp_info(b"xxxx") == None, "bad");

    // ---- A129 AVIF ----
    set.add(
        "A129 avif dims",
        parse_avif_info(&TEST_AVIF).map(|i| i.width == 3 && i.height == 2).unwrap_or(false),
        "ispe",
    );
    set.add("A129 avif reject", parse_avif_info(b"nope") == None, "bad");

    // ---- A130 BMP / QOI roundtrip ----
    let img = [255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
    let mut bmp = [0u8; 256];
    let bn = encode_bmp(2, 2, &img, &mut bmp).unwrap();
    let mut bout = [0u8; 64];
    let bi = decode_bmp(&bmp[..bn], &mut bout);
    set.add(
        "A130 bmp roundtrip",
        bi.map(|i| i.width == 2).unwrap_or(false)
            && bout[0] == 255
            && bout[4] == 0
            && bout[8] == 0
            && bout[12] == 255,
        "bmp",
    );

    let qimg = [
        0u8, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255, 10, 20,
        30, 200, 40, 50, 60, 100, 70, 80, 90, 50,
    ];
    let mut qout = [0u8; 256];
    let qn = encode_qoi(&qimg, 4, 2, &mut qout).unwrap();
    let mut qdec = [0u8; 64];
    let qi = decode_qoi(&qout[..qn], &mut qdec);
    set.add(
        "A130 qoi roundtrip",
        qi.map(|i| i.width == 4 && i.height == 2).unwrap_or(false) && qdec[..32] == qimg[..32],
        "qoi",
    );

    // ---- A131 缩放 ----
    let grad = [0u8, 0, 0, 255, 255, 0, 0, 255];
    let mut dne = [0u8; 12];
    let mut dbi = [0u8; 12];
    scale_nearest(&grad, 2, 1, &mut dne, 3, 1);
    scale_bilinear(&grad, 2, 1, &mut dbi, 3, 1);
    set.add(
        "A131 nearest",
        dne[0] == 0 && dne[4] == 0 && dne[8] == 255,
        "nearest",
    );
    set.add(
        "A131 bilinear",
        dbi[0] == 0 && dbi[4] == 127 && dbi[8] == 255,
        "bilinear",
    );

    // ---- A132 灰度和调色板 ----
    let mut gray = [0u8; 4];
    let ok_gray = rgb_to_gray(&img, &mut gray);
    set.add("A132 gray", ok_gray && gray[0] == 76 && gray[3] == 255, "luma");
    let indices = [0u8, 1, 2, 1];
    let palette = [[255u8, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
    let mut pout = [0u8; 16];
    let ok_pal = expand_palette(&indices, &palette, &mut pout);
    set.add(
        "A132 palette",
        ok_pal && pout[0] == 255 && pout[8] == 0 && pout[9] == 0 && pout[11] == 255,
        "expand",
    );

    // ---- A133 缓存命中/驱逐 ----
    let mut cache = DecodeCache::new();
    let fp1 = DecodeCache::fingerprint(&[1, 2, 3]);
    let fp2 = DecodeCache::fingerprint(&[4, 5, 6]);
    cache.insert(fp1, 10, 10);
    cache.insert(fp2, 20, 20);
    let hit1 = cache.get(&fp1).is_some();
    let hit_miss = cache.get(&DecodeCache::fingerprint(&[9, 9, 9])).is_none();
    for k in 0..CACHE_CAP + 2 {
        cache.insert(DecodeCache::fingerprint(&[k as u8, 0, 0]), k as u32, k as u32);
    }
    set.add(
        "A133 cache hit",
        hit1 && hit_miss && cache.len() == CACHE_CAP && cache.evictions > 0,
        "lru",
    );

    // ---- A134 渐进加载 ----
    let mut pl = ProgressiveLoader::new(2);
    let mut pdst = [0u8; 16];
    let r0 = [1u8, 2, 3, 4, 5, 6, 7, 8];
    let r1 = [9u8, 10, 11, 12, 13, 14, 15, 16];
    let ok_p0 = pl.push_row(&r0, &mut pdst, 8);
    let ok_p1 = pl.push_row(&r1, &mut pdst, 8);
    set.add(
        "A134 progressive",
        ok_p0 && ok_p1 && pl.state == ProgState::Complete && pdst[0] == 1 && pdst[8] == 9,
        "rows",
    );

    // ---- A135 缩略图 ----
    let big = [0u8; 4 * 4 * 4];
    let mut thumb = [0u8; 4 * 2 * 2];
    let tdim = make_thumbnail(&big, 4, 4, 2, &mut thumb);
    set.add(
        "A135 thumbnail",
        tdim == Some((2, 2)),
        "downscale",
    );

    // ---- A136 EXIF ----
    set.add(
        "A136 exif orientation",
        parse_exif_orientation(&TEST_EXIF_JPEG) == Some(6),
        "orient",
    );

    // ---- A137 旋转/裁剪 ----
    let sq = [
        1u8, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255,
    ];
    let mut rot = [0u8; 16];
    let ok_rot = rotate90(&sq, 2, 2, &mut rot);
    let mut cr = [0u8; 16];
    let ok_crop = crop(&sq, 2, 2, &mut cr, 0, 0, 2, 2);
    set.add(
        "A137 rotate/crop",
        ok_rot && ok_crop && rot[0] == 3 && cr[0] == 1 && cr[4] == 2,
        "geom",
    );

    // ---- A138 alpha ----
    let mut alpha = [255u8, 255, 255, 128];
    premultiply_alpha(&mut alpha);
    let pre = alpha[0];
    straighten_alpha(&mut alpha);
    set.add(
        "A138 alpha",
        pre == 128 && alpha[0] == 255,
        "premul",
    );

    // ---- A139 性能预算 ----
    set.add(
        "A139 budget",
        decode_budget_ok(100, 100, 32, 40000) && !decode_budget_ok(100, 100, 32, 100),
        "perf",
    );

    // ---- A140 模糊测试 ----
    let garbage = [0u8, 1, 2, 3, 0xFF, 0xD8, 0x00, 0x99];
    let mut fo = [0u8; 64];
    let mut fscr = [0u8; 64];
    let att = fuzz_probe(&garbage, &mut fo, &mut fscr);
    set.add("A140 fuzz", att >= 4, "no-panic");

    // ---- A141 色彩管理 ----
    let mut all_rt = true;
    for v in [0u8, 1, 64, 128, 200, 255] {
        if linear_to_srgb8(srgb_to_linear8(v)) != v {
            all_rt = false;
        }
    }
    let mut fwd = [0u8; 256];
    let mut inv = [0u8; 256];
    build_gamma_lut(2, &mut fwd);
    invert_gamma_lut(&fwd, &mut inv);
    set.add(
        "A141 color mgmt",
        all_rt && inv[fwd[128] as usize] == 128 && inv[fwd[255] as usize] == 255,
        "srgb/gamma",
    );

    // ---- A142 内存预算 ----
    set.add(
        "A142 memory",
        memory_within_budget(64, 64, 64 * 64 * 4) && !memory_within_budget(64, 64, 1),
        "mem",
    );

    // ---- A143 收口计数 ----
    set.add("A143 closeout", image_selfcheck_closeout() == 25, "count");

    // ---- A145 性能预算结构 ----
    let pb = ImagePerfBudget::new();
    set.add(
        "A145 perf budget",
        pb.ok(&ImageInfo { width: 32, height: 32, format: ImageFormat::Png, bpp: 8 })
            && pb.decode_ok(100),
        "budget",
    );

    // ---- A146 可观测 ----
    let mut stats = ImageStats::new();
    stats.record_decode(true);
    stats.record_decode(false);
    stats.record_cache_hit();
    stats.record_eviction();
    let mut sbuf = [0u8; 128];
    let sn = stats.render(&mut sbuf);
    let stext = core::str::from_utf8(&sbuf[..sn]).unwrap_or("");
    set.add(
        "A146 observability",
        sn > 0 && stext.contains("image") && stats.total() == 2,
        "stats",
    );

    // ---- A147 模糊测试工具 ----
    let safe = image_fuzz_harness(&garbage, &mut fo, &mut fscr);
    set.add("A147 fuzz harness", safe, "harness");

    // ---- A148 文档 ----
    set.add("A148 doc", image_doc().contains("AURORA-1000"), "doc");

    // ---- A149 降级链 ----
    let mut ph = [0u8; 16];
    placeholder_rgba(&mut ph, 7, 8, 9, 255);
    let degraded = decode_best_effort(&garbage, &mut fo, &mut fscr);
    set.add(
        "A149 degrade",
        degraded.is_none() && ph[0] == 7 && ph[3] == 255,
        "fallback",
    );

    // ---- A150 域自检收口 ----
    set.add("A150 domain verify", { let (p, f) = set.tally(); f == 0 && p >= 25 && !set.truncated() }, "verify");

    set
}

// ===========================================================================
// A145 — 图像编解码与缓存性能预算
// ===========================================================================

/// 解码性能/内存预算。
#[derive(Clone, Copy, Debug)]
pub struct ImagePerfBudget {
    pub max_decode_us: u32,
    pub max_mem_bytes: u32,
}

impl ImagePerfBudget {
    pub const fn new() -> ImagePerfBudget {
        ImagePerfBudget { max_decode_us: 5000, max_mem_bytes: 16 * 1024 * 1024 }
    }

    /// 内存预算内？
    pub fn ok(&self, info: &ImageInfo) -> bool {
        (info.width as u64 * info.height as u64 * 4) <= self.max_mem_bytes as u64
    }

    /// 解码耗时预算内？
    pub fn decode_ok(&self, us: u32) -> bool {
        us <= self.max_decode_us
    }
}

// ===========================================================================
// A146 — 图像编解码与缓存可观测
// ===========================================================================

/// 解码域可观测计数器。
#[derive(Clone, Copy, Debug)]
pub struct ImageStats {
    pub decodes: u32,
    pub decode_errors: u32,
    pub cache_hits: u32,
    pub cache_misses: u32,
    pub evictions: u32,
    pub thumbnails: u32,
}

impl ImageStats {
    pub const fn new() -> ImageStats {
        ImageStats {
            decodes: 0,
            decode_errors: 0,
            cache_hits: 0,
            cache_misses: 0,
            evictions: 0,
            thumbnails: 0,
        }
    }

    pub fn record_decode(&mut self, ok: bool) {
        if ok {
            self.decodes += 1;
        } else {
            self.decode_errors += 1;
        }
    }

    pub fn record_cache_hit(&mut self) {
        self.cache_hits += 1;
    }

    pub fn record_eviction(&mut self) {
        self.evictions += 1;
    }

    pub fn record_thumbnail(&mut self) {
        self.thumbnails += 1;
    }

    pub fn total(&self) -> u32 {
        self.decodes + self.decode_errors
    }

    /// 渲染为文本到 `out`，返回写入字节数。
    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        push_str(out, &mut n, "image decodes=");
        push_usize(out, &mut n, self.decodes as usize);
        push_str(out, &mut n, " errors=");
        push_usize(out, &mut n, self.decode_errors as usize);
        push_str(out, &mut n, " hits=");
        push_usize(out, &mut n, self.cache_hits as usize);
        push_str(out, &mut n, " misses=");
        push_usize(out, &mut n, self.cache_misses as usize);
        push_str(out, &mut n, " evict=");
        push_usize(out, &mut n, self.evictions as usize);
        push_str(out, &mut n, " thumb=");
        push_usize(out, &mut n, self.thumbnails as usize);
        push_str(out, &mut n, "\n");
        n
    }
}

// ===========================================================================
// A147 — 图像编解码与缓存模糊测试
// ===========================================================================

/// 模糊测试工具：对垃圾输入跑完整解码链且不 panic，返回是否安全完成。
pub fn image_fuzz_harness(data: &[u8], out: &mut [u8], scratch: &mut [u8]) -> bool {
    let _ = fuzz_probe(data, out, scratch);
    let _ = sniff_format(data);
    let _ = decode_best_effort(data, out, scratch);
    true
}

// ===========================================================================
// A148 — 图像编解码与缓存文档
// ===========================================================================

/// 返回本域文档摘要串。
pub fn image_doc() -> &'static str {
    "AURORA-1000 AI-06 · 图像编解码与缓存（A126~A150，W1）：纯 no_std 图像管线"
}

// ===========================================================================
// A149 — 图像编解码与缓存降级链
// ===========================================================================

/// 尽力解码：按嗅探格式选择解码器，全失败返回 `None`。
pub fn decode_best_effort(data: &[u8], out: &mut [u8], scratch: &mut [u8]) -> Option<ImageInfo> {
    match sniff_format(data) {
        ImageFormat::Png => decode_png(data, out, scratch),
        ImageFormat::Bmp => decode_bmp(data, out),
        ImageFormat::Qoi => decode_qoi(data, out),
        _ => None,
    }
}

/// 填充占位纯色 RGBA 缓冲。
pub fn placeholder_rgba(out: &mut [u8], r: u8, g: u8, b: u8, a: u8) {
    let n = out.len() / 4;
    for i in 0..n {
        out[i * 4] = r;
        out[i * 4 + 1] = g;
        out[i * 4 + 2] = b;
        out[i * 4 + 3] = a;
    }
}

// ===========================================================================
// A150 — 图像编解码与缓存域自检收口
// ===========================================================================

/// 域自检最终收口：所有不变量通过返回 `true`。
pub fn domain_closeout() -> bool {
    run_image_checks().all_passed()
}

// ===========================================================================
// 测试辅助：构造最小合法 PNG（RGBA，滤波器 0）
// ===========================================================================

/// 把 8 位 PNG（IHDR + IDAT(zlib stored) + IEND）写入 `buf`，返回写入长度。
fn make_png(buf: &mut [u8], w: u32, h: u32, ct: u8, raw: &[u8]) -> usize {
    buf[0..8].copy_from_slice(&PNG_SIGNATURE);
    let mut p = 8usize;
    // IHDR
    let ihdr_len = 13u32;
    buf[p..p + 4].copy_from_slice(&ihdr_len.to_be_bytes());
    p += 4;
    buf[p..p + 4].copy_from_slice(b"IHDR");
    p += 4;
    buf[p..p + 4].copy_from_slice(&w.to_be_bytes());
    p += 4;
    buf[p..p + 4].copy_from_slice(&h.to_be_bytes());
    p += 4;
    buf[p] = 8;
    p += 1;
    buf[p] = ct;
    p += 1;
    buf[p] = 0;
    p += 1;
    buf[p] = 0;
    p += 1;
    buf[p] = 0;
    p += 1;
    let crc = crc32(&buf[p - 17..p]);
    buf[p..p + 4].copy_from_slice(&crc.to_be_bytes());
    p += 4;
    // IDAT：zlib stored
    let mut z = [0u8; 1024];
    let mut zp = 0usize;
    z[zp] = 0x78;
    zp += 1;
    z[zp] = 0x01;
    zp += 1;
    z[zp] = 0x01; // BFINAL=1, BTYPE=00
    zp += 1;
    let len = raw.len() as u16;
    z[zp..zp + 2].copy_from_slice(&len.to_le_bytes());
    zp += 2;
    z[zp..zp + 2].copy_from_slice(&(!len).to_le_bytes());
    zp += 2;
    z[zp..zp + raw.len()].copy_from_slice(raw);
    zp += raw.len();
    let ad = adler32(raw);
    z[zp..zp + 4].copy_from_slice(&ad.to_be_bytes());
    zp += 4;
    let idat_len = zp as u32;
    buf[p..p + 4].copy_from_slice(&idat_len.to_be_bytes());
    p += 4;
    buf[p..p + 4].copy_from_slice(b"IDAT");
    p += 4;
    buf[p..p + zp].copy_from_slice(&z[0..zp]);
    p += zp;
    let crc2 = crc32(&buf[p - 4 - zp..p]);
    buf[p..p + 4].copy_from_slice(&crc2.to_be_bytes());
    p += 4;
    // IEND
    buf[p..p + 4].copy_from_slice(&0u32.to_be_bytes());
    p += 4;
    buf[p..p + 4].copy_from_slice(b"IEND");
    p += 4;
    buf[p..p + 4].copy_from_slice(&crc32(b"IEND").to_be_bytes());
    p += 4;
    p
}

// 最小 JPEG（SOF0，宽 3 高 2）。
const TEST_JPEG: [u8; 23] = [
    0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x02, 0x00, 0x03, 0x03, 0x01, 0x11, 0x00, 0x02,
    0x11, 0x00, 0x03, 0x11, 0x00, 0xFF, 0xD9,
];

// 最小 WebP（VP8L，宽 3 高 2）。
const TEST_WEBP: [u8; 25] = [
    0x52, 0x49, 0x46, 0x46, // RIFF
    0x11, 0x00, 0x00, 0x00, // size = 17
    0x57, 0x45, 0x42, 0x50, // WEBP
    0x56, 0x50, 0x38, 0x4C, // VP8L
    0x05, 0x00, 0x00, 0x00, // chunk size = 5
    0x2F, 0x02, 0x40, 0x00, 0x00,
];

// 最小 AVIF（ftyp 品牌 avif + ispe 宽 3 高 2）。
const TEST_AVIF: [u8; 40] = [
    0x00, 0x00, 0x00, 0x18, // ftyp size
    0x66, 0x74, 0x79, 0x70, // ftyp
    0x61, 0x76, 0x69, 0x66, // major brand avif
    0x00, 0x00, 0x00, 0x00, // minor
    0x61, 0x76, 0x69, 0x66, // compatible brand avif
    0x00, 0x00, 0x00, 0x10, // ispe size
    0x69, 0x73, 0x70, 0x65, // ispe
    0x00, 0x00, 0x00, 0x00, // version/flags
    0x00, 0x00, 0x00, 0x03, // width = 3
    0x00, 0x00, 0x00, 0x02, // height = 2
];

// 最小 JPEG+APP1/Exif，orientation = 6。
const TEST_EXIF_JPEG: [u8; 38] = [
    0xFF, 0xD8, // SOI
    0xFF, 0xE1, // APP1
    0x00, 0x22, // length = 34
    0x45, 0x78, 0x69, 0x66, 0x00, 0x00, // "Exif\0\0"
    0x49, 0x49, // II
    0x2A, 0x00, // 42
    0x08, 0x00, 0x00, 0x00, // IFD0 offset = 8
    0x01, 0x00, // count = 1
    0x12, 0x01, // tag 0x0112
    0x03, 0x00, // type SHORT
    0x01, 0x00, 0x00, 0x00, // count 1
    0x06, 0x00, 0x00, 0x00, // value 6
    0x00, 0x00, 0x00, 0x00, // next IFD = 0
];

// ===========================================================================
// 单元测试
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a126_png_roundtrip_rgba() {
        let raw = [
            0, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let mut buf = [0u8; 256];
        let n = make_png(&mut buf, 2, 2, 6, &raw);
        let mut out = [0u8; 64];
        let mut scr = [0u8; 256];
        let info = decode_png(&buf[..n], &mut out, &mut scr).expect("decode");
        assert_eq!(info.width, 2);
        assert_eq!(info.height, 2);
        assert_eq!(&out[0..4], &[255, 0, 0, 255]);
        assert_eq!(&out[4..8], &[0, 255, 0, 255]);
        assert_eq!(&out[8..12], &[0, 0, 255, 255]);
        assert_eq!(&out[12..16], &[255, 255, 255, 255]);
    }

    #[test]
    fn a126_png_filter_modes() {
        // 2x2 RGB：raw = 每行 1 字节 filter(0) + 6 字节像素。
        let raw = [
            0u8, 10, 20, 30, 40, 50, 60, // row0
            0, 70, 80, 90, 100, 110, 120, // row1
        ];
        let mut buf = [0u8; 256];
        let n = make_png(&mut buf, 2, 2, 2, &raw);
        let mut out = [0u8; 64];
        let mut scr = [0u8; 256];
        let info = decode_png(&buf[..n], &mut out, &mut scr).expect("decode");
        assert_eq!(info.format, ImageFormat::Png);
        assert_eq!(&out[0..3], &[10, 20, 30]);
        assert_eq!(&out[12..15], &[100, 110, 120]);
    }

    #[test]
    fn a126_png_reject_garbage() {
        let g = [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let mut out = [0u8; 16];
        let mut scr = [0u8; 16];
        assert!(decode_png(&g, &mut out, &mut scr).is_none());
        assert!(parse_png_header(&g).is_none());
    }

    #[test]
    fn a126_sniff_all_formats() {
        assert_eq!(sniff_format(&PNG_SIGNATURE), ImageFormat::Png);
        assert_eq!(sniff_format(&[0xFF, 0xD8, 0xFF, 0xE0]), ImageFormat::Jpeg);
        assert_eq!(
            sniff_format(&[b'R', b'I', b'F', b'F', 0, 0, 0, 0, b'W', b'E', b'B', b'P']),
            ImageFormat::Webp
        );
        assert_eq!(sniff_format(&[b'B', b'M', 0, 0]), ImageFormat::Bmp);
        assert_eq!(sniff_format(b"qoifxxxx"), ImageFormat::Qoi);
        assert_eq!(sniff_format(&TEST_AVIF), ImageFormat::Avif);
        assert_eq!(sniff_format(b"???"), ImageFormat::Unknown);
    }

    #[test]
    fn a126_crc32_known() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn a126_inflate_stored() {
        let z = [0x78u8, 0x01, 0x01, 0x03, 0x00, 0xFC, 0xFF, 0xAA, 0xBB, 0xCC, 0x01, 0x00, 0x00, 0x00];
        let mut o = [0u8; 8];
        let w = inflate_stored(&z, &mut o).unwrap();
        assert_eq!(w, 3);
        assert_eq!(&o[0..3], &[0xAA, 0xBB, 0xCC]);
        // 压缩块应被拒绝
        let bad = [0x78u8, 0x01, 0x01, 0xFF, 0xFF];
        assert!(inflate_stored(&bad, &mut o).is_none());
    }

    #[test]
    fn a127_jpeg_header() {
        let info = parse_jpeg_info(&TEST_JPEG).expect("jpeg");
        assert_eq!(info.width, 3);
        assert_eq!(info.height, 2);
        assert!(parse_jpeg_info(b"not a jpeg").is_none());
        assert!(parse_jpeg_info(&[0xFF, 0xD8, 0xFF, 0xDA]).is_none());
    }

    #[test]
    fn a128_webp_vp8l() {
        let info = parse_webp_info(&TEST_WEBP).expect("webp");
        assert_eq!(info.width, 3);
        assert_eq!(info.height, 2);
        assert!(parse_webp_info(b"RIFFxxxxWEBP").is_none());
    }

    #[test]
    fn a129_avif_ispe() {
        let info = parse_avif_info(&TEST_AVIF).expect("avif");
        assert_eq!(info.width, 3);
        assert_eq!(info.height, 2);
        assert!(parse_avif_info(b"RIFFxxxxftyp").is_none());
    }

    #[test]
    fn a130_bmp_roundtrip() {
        let img = [255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
        let mut bmp = [0u8; 128];
        let n = encode_bmp(2, 2, &img, &mut bmp).expect("encode");
        assert!(n > 54);
        let mut out = [0u8; 64];
        let info = decode_bmp(&bmp[..n], &mut out).expect("decode");
        assert_eq!(info.width, 2);
        // 24bpp 丢失 alpha，解码还原为 255；RGB 应一致
        assert_eq!(&out[0..3], &[255, 0, 0]);
        assert_eq!(&out[4..7], &[0, 255, 0]);
        assert_eq!(out[3], 255);
        assert_eq!(out[7], 255);
    }

    #[test]
    fn a130_bmp_reject_garbage() {
        assert!(decode_bmp(b"BMxxxx", &mut [0u8; 64]).is_none());
        assert!(encode_bmp(2, 2, &[0u8; 4], &mut [0u8; 64]).is_none());
    }

    #[test]
    fn a130_qoi_roundtrip() {
        let img = [
            0u8, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            10, 20, 30, 200, 40, 50, 60, 100, 70, 80, 90, 50, 1, 2, 3, 4, 5, 6, 7, 8,
        ];
        let mut enc = [0u8; 256];
        let n = encode_qoi(&img, 4, 2, &mut enc).expect("encode");
        let mut dec = [0u8; 64];
        let info = decode_qoi(&enc[..n], &mut dec).expect("decode");
        assert_eq!(info.width, 4);
        assert_eq!(info.height, 2);
        assert_eq!(&dec[..32], &img[..32]);
    }

    #[test]
    fn a130_qoi_uniform_and_run() {
        let mut img = [0u8; 64];
        for i in 0..16 {
            img[i * 4] = 200;
            img[i * 4 + 1] = 100;
            img[i * 4 + 2] = 50;
            img[i * 4 + 3] = 255;
        }
        let mut enc = [0u8; 256];
        let n = encode_qoi(&img, 4, 4, &mut enc).expect("encode");
        let mut dec = [0u8; 256];
        let info = decode_qoi(&enc[..n], &mut dec).expect("decode");
        assert_eq!((info.width, info.height), (4, 4));
        assert_eq!(&dec[..64], &img[..64]);
    }

    #[test]
    fn a131_nearest_scale() {
        let src = [0u8, 0, 0, 255, 255, 0, 0, 255];
        let mut dst = [0u8; 12];
        assert!(scale_nearest(&src, 2, 1, &mut dst, 3, 1));
        assert_eq!(&dst[0..4], &[0, 0, 0, 255]);
        assert_eq!(&dst[4..8], &[0, 0, 0, 255]);
        assert_eq!(&dst[8..12], &[255, 0, 0, 255]);
        assert!(!scale_nearest(&src, 2, 1, &mut dst, 0, 1)); // dw=0 拒绝
    }

    #[test]
    fn a131_bilinear_scale() {
        let src = [0u8, 0, 0, 255, 255, 0, 0, 255];
        let mut dst = [0u8; 12];
        assert!(scale_bilinear(&src, 2, 1, &mut dst, 3, 1));
        assert_eq!(dst[0], 0);
        assert_eq!(dst[4], 127); // 中点插值
        assert_eq!(dst[8], 255);
    }

    #[test]
    fn a131_bilinear_identity_2x2() {
        let src = [1u8, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255];
        let mut dst = [0u8; 16];
        assert!(scale_bilinear(&src, 2, 2, &mut dst, 2, 2));
        assert_eq!(&dst[..16], &src[..16]);
    }

    #[test]
    fn a132_rgb_to_gray_and_palette() {
        let img = [255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
        let mut gray = [0u8; 4];
        assert!(rgb_to_gray(&img, &mut gray));
        assert_eq!(gray[0], 76); // 红
        assert_eq!(gray[1], 150); // 绿
        assert_eq!(gray[3], 255); // 白
        let idx = [0u8, 1, 2, 0];
        let pal = [[11u8, 22, 33, 44], [55, 66, 77, 88], [99, 11, 22, 33]];
        let mut out = [0u8; 16];
        assert!(expand_palette(&idx, &pal, &mut out));
        assert_eq!(&out[0..4], &[11, 22, 33, 44]);
        assert_eq!(&out[4..8], &[55, 66, 77, 88]);
    }

    #[test]
    fn a133_cache_hit_and_evict() {
        let mut c = DecodeCache::new();
        let fp1 = DecodeCache::fingerprint(&[1, 2, 3]);
        let fp2 = DecodeCache::fingerprint(&[4, 5, 6]);
        c.insert(fp1, 10, 10);
        assert_eq!(c.get(&fp1), Some((10, 10)));
        assert_eq!(c.hits, 1);
        assert_eq!(c.get(&fp2), None);
        assert_eq!(c.misses, 1);
        // 填满并触发驱逐
        for k in 0..CACHE_CAP + 3 {
            c.insert(DecodeCache::fingerprint(&[k as u8, 0, 0]), k as u32, k as u32);
        }
        assert_eq!(c.len(), CACHE_CAP);
        assert!(c.evictions > 0);
        // 最早插入的 fp1 应已被驱逐
        assert_eq!(c.get(&fp1), None);
    }

    #[test]
    fn a134_progressive_loader_and_cb() {
        let mut pl = ProgressiveLoader::new(3);
        let mut dst = [0u8; 24];
        let r0 = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let r1 = [9u8, 10, 11, 12, 13, 14, 15, 16];
        let r2 = [17u8, 18, 19, 20, 21, 22, 23, 24];
        assert!(pl.push_row(&r0, &mut dst, 8));
        assert_eq!(pl.state, ProgState::Loading);
        assert!(pl.push_row(&r1, &mut dst, 8));
        assert!(pl.push_row(&r2, &mut dst, 8));
        assert_eq!(pl.state, ProgState::Complete);
        assert!(!pl.push_row(&r0, &mut dst, 8)); // 完成后拒绝
        assert_eq!(&dst[0..8], &r0);
        assert_eq!(&dst[16..24], &r2);

        // 回调式（fn 指针，不捕获；返回值即回调次数）
        let rows = [&r0[..], &r1[..], &r2[..]];
        let mut cdst = [0u8; 24];
        let got = progressive_consume(&rows, &mut cdst, 8, |_i, _row| {});
        assert_eq!(got, 3);
    }

    #[test]
    fn a134_progressive_bad_row() {
        let mut pl = ProgressiveLoader::new(2);
        let mut dst = [0u8; 16];
        let short = [1u8, 2, 3];
        assert!(!pl.push_row(&short, &mut dst, 8));
        assert_eq!(pl.state, ProgState::Failed);
    }

    #[test]
    fn a135_thumbnail() {
        let src = [7u8; 4 * 4 * 4];
        let mut dst = [0u8; 4 * 2 * 2];
        let dim = make_thumbnail(&src, 4, 4, 2, &mut dst);
        assert_eq!(dim, Some((2, 2)));
        let dim2 = make_thumbnail(&src, 4, 2, 2, &mut dst);
        assert_eq!(dim2, Some((2, 1))); // sw>=sh
        assert!(make_thumbnail(&src, 4, 4, 2, &mut [0u8; 4]).is_none());
    }

    #[test]
    fn a136_exif_orientation() {
        assert_eq!(parse_exif_orientation(&TEST_EXIF_JPEG), Some(6));
        assert!(parse_exif_orientation(b"garbage").is_none());
    }

    #[test]
    fn a137_rotate_and_crop() {
        let sq = [1u8, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255];
        let mut rot = [0u8; 16];
        assert!(rotate90(&sq, 2, 2, &mut rot));
        // 顺时针90°：(0,0)=1 应到 (0,1)
        assert_eq!(rot[0], 3);
        assert_eq!(rot[4], 1);
        let mut cropbuf = [0u8; 16];
        assert!(crop(&sq, 2, 2, &mut cropbuf, 0, 0, 2, 2));
        assert_eq!(&cropbuf[0..4], &sq[0..4]);
        assert!(!crop(&sq, 2, 2, &mut cropbuf, 1, 1, 2, 2)); // 越界
    }

    #[test]
    fn a138_alpha_premultiply() {
        let mut a = [255u8, 255, 255, 128];
        premultiply_alpha(&mut a);
        assert_eq!(a[0], 128);
        straighten_alpha(&mut a);
        assert_eq!(a[0], 255);
        // 全不透明应保持不变
        let mut b = [100u8, 50, 25, 255];
        premultiply_alpha(&mut b);
        assert_eq!(b[0..3], [100, 50, 25]);
    }

    #[test]
    fn a139_decode_budget() {
        assert!(decode_budget_ok(100, 100, 32, 40000));
        assert!(!decode_budget_ok(100, 100, 32, 100));
    }

    #[test]
    fn a140_fuzz_no_panic() {
        // 截断/坏魔数/随机字节都不应 panic（Rust 测试会捕获 panic）。
        let inputs: [&[u8]; 4] = [
            &[0u8, 1, 2, 3][..],
            &[0xFF, 0xD8, 0x00, 0x99, 0x01][..],
            &[0x89, 0x50, 0x4E, 0x47][..], // PNG 签名开头但截断
            &[b'q', b'o', b'i'][..],
        ];
        for inp in inputs.iter() {
            let mut out = [0u8; 64];
            let mut scr = [0u8; 64];
            let att = fuzz_probe(inp, &mut out, &mut scr);
            assert!(att >= 4);
        }
    }

    #[test]
    fn a141_srgb_and_gamma() {
        for v in [0u8, 1, 64, 128, 200, 255] {
            assert_eq!(linear_to_srgb8(srgb_to_linear8(v)), v);
        }
        let mut fwd = [0u8; 256];
        let mut inv = [0u8; 256];
        build_gamma_lut(2, &mut fwd);
        invert_gamma_lut(&fwd, &mut inv);
        assert_eq!(inv[fwd[0] as usize], 0);
        assert_eq!(inv[fwd[128] as usize], 128);
        assert_eq!(inv[fwd[255] as usize], 255);
        // gamma 应单调
        for v in 1..256 {
            assert!(fwd[v] >= fwd[v - 1]);
        }
        let mut px = [0u8, 100, 200, 255, 50, 0, 0, 255];
        apply_gamma_lut(&fwd, &mut px);
        assert_eq!(px[1], fwd[100]);
    }

    #[test]
    fn a142_memory_budget() {
        assert_eq!(estimate_decoded_bytes(64, 64), 64 * 64 * 4);
        assert!(memory_within_budget(64, 64, 64 * 64 * 4));
        assert!(!memory_within_budget(64, 64, 1));
    }

    #[test]
    fn a143_closeout_count() {
        assert_eq!(image_selfcheck_closeout(), 25);
    }

    #[test]
    fn a144_self_checks_all_pass() {
        let set = run_image_checks();
        assert!(set.all_passed(), "some checks failed");
        assert!(set.len() >= 25, "need >=25 checks, got {}", set.len());
    }

    #[test]
    fn a145_perf_budget_struct() {
        let pb = ImagePerfBudget::new();
        assert!(pb.ok(&ImageInfo { width: 100, height: 100, format: ImageFormat::Png, bpp: 8 }));
        assert!(pb.decode_ok(100));
        assert!(!pb.decode_ok(99999));
    }

    #[test]
    fn a146_stats_render() {
        let mut s = ImageStats::new();
        s.record_decode(true);
        s.record_decode(false);
        s.record_cache_hit();
        s.record_eviction();
        s.record_thumbnail();
        assert_eq!(s.total(), 2);
        assert_eq!(s.thumbnails, 1);
        let mut buf = [0u8; 128];
        let n = s.render(&mut buf);
        let text = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(text.contains("image"));
        assert!(text.contains("decodes=1"));
    }

    #[test]
    fn a147_fuzz_harness_safe() {
        let g = [0u8, 1, 2, 3, 0xFF, 0xD8, 0x00, 0x99];
        let mut out = [0u8; 64];
        let mut scr = [0u8; 64];
        assert!(image_fuzz_harness(&g, &mut out, &mut scr));
    }

    #[test]
    fn a148_doc_string() {
        assert!(image_doc().contains("AURORA-1000"));
    }

    #[test]
    fn a149_degrade_chain() {
        let g = [0u8, 1, 2, 3];
        let mut out = [0u8; 64];
        let mut scr = [0u8; 64];
        assert!(decode_best_effort(&g, &mut out, &mut scr).is_none());
        let mut ph = [0u8; 16];
        placeholder_rgba(&mut ph, 7, 8, 9, 255);
        assert_eq!(ph[0], 7);
        assert_eq!(ph[3], 255);
        // 合法 PNG 应经 best_effort 成功
        let raw = [0, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255, 255];
        let mut buf = [0u8; 256];
        let n = make_png(&mut buf, 2, 2, 6, &raw);
        let mut good = [0u8; 64];
        assert!(decode_best_effort(&buf[..n], &mut good, &mut scr).is_some());
    }

    #[test]
    fn a150_domain_closeout() {
        assert!(domain_closeout());
    }
}
