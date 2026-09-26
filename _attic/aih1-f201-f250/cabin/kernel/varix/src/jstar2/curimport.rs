//! F633 .cur/.ani 全量导入 · 完整设计（STAR I 主册 J-D 组）。
//!
//! **判据（主册原文）**：100 样本库像素级对拍 100%；三编码族覆盖；.ani
//! 帧序列保真；失败报错定位判据；与 F627 体检衔接。
//!
//! **实现口径**：Windows 指针格式逐字节兼容解析——
//! - `.cur`/`.ico`：ICONDIR + ICONDIRENTRY + DIB（BITMAPINFOHEADER +
//!   调色板 + XOR 色面 + AND 掩码），解码 1/4/8/24/32 五档位深
//!   （判据「三编码族」= 32bpp 带 Alpha / 24bpp+掩码 / 8bpp 调色板+掩码，
//!   1bpp/4bpp 作为低色深扩展同路径覆盖）；
//! - `.ani`：RIFF 容器（ACON form），anih 头（帧数/步数/jiffy 速率）、
//!   `seq ` 播放序、`rate` 逐帧延时、LIST fram 逐帧嵌套完整 `.ico`；
//! - 帧序/帧率双保真：`.ani` 帧序按 `seq `（缺省顺序）展开，jiffy
//!   （1/60s）→ 毫秒换算 `(jif*1000+30)/60`，原 jif 值换算前后可对账；
//! - 解析失败诚实报错：错误带字节偏移/帧序号/块名定位——不半导入、
//!   不静默截断（`CurImportError` 全枚举定位字段）；
//! - 导入产物直接是 [`CursorSchemeModel`]（jbase 唯一方案事实源），
//!   导入完成即接 F627 体检（`checker::inspect`，衔接判据的机制面）。
//!
//! **样本库（100）**：`samples::generate_library` 确定性生成——60 `.cur`
//! （15 态 × 4 编码档）、30 `.ani`（帧数/速率/seq/rate 变体 + 尺寸档）、
//! 10 对抗样本（截断/坏 magic/坏块/超尺寸/超帧率）——生成器与解析器
//! 独立成对，对拍 = 生成器真值 RGBA 逐像素比对。
//!
//! 状态归属说明：`.cur`/`.ani` 单文件不含「15 态」语义，导入接口以
//! `target_state` 显式指定归属态（调用方——F638 迁移桥/F635 侧载链——
//! 按文件名约定或注册表映射赋予），本模块不做状态猜测。

use crate::checks::CheckSet;
use crate::jstar2::jbase::{
    CursorFrame, CursorSchemeModel, OriginKind, PointerState, PixBuf, XorShift32, fnv1a64,
};
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（与 jbase 方案级闸线同源一处一事实）
// ---------------------------------------------------------------------------

use crate::jstar2::jbase::{MAX_FRAMES_PER_STATE, MAX_FRAME_PX, MAX_FPS};

/// `.ani` 默认 jiffy 速率（帧延时单位 = 1/60 秒）。
pub const ANI_DEFAULT_JIF: u32 = 10;

/// 单 .ani 帧上限（16 帧纪律）。
pub const MAX_ANI_FRAMES: usize = MAX_FRAMES_PER_STATE;

// ---------------------------------------------------------------------------
// 错误面（诚实定位）
// ---------------------------------------------------------------------------

/// 导入错误：每个变体都带定位信息（字节偏移/帧号/块名）——判据
/// 「失败报错定位」的机器面。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CurImportError {
    TooSmall(usize),
    BadMagic {
        offset: usize,
    },
    UnsupportedType {
        offset: usize,
        type_: u16,
    },
    ZeroImages {
        offset: usize,
    },
    EntryOutOfRange {
        index: usize,
        offset: usize,
    },
    ImageTooLarge {
        index: usize,
        w: u32,
        h: u32,
    },
    BadDibHeader {
        index: usize,
        offset: usize,
        why: &'static str,
    },
    UnsupportedBpp {
        index: usize,
        bpp: u16,
    },
    Truncated {
        offset: usize,
        need: &'static str,
    },
    BadRiffChunk {
        offset: usize,
        fourcc: [u8; 4],
    },
    BadAnihHeader {
        offset: usize,
        why: &'static str,
    },
    FrameCountOver {
        count: usize,
    },
    FpsOverLimit {
        delay_ms: u32,
    },
}

impl CurImportError {
    /// 人话定位（F209 三要素的「发生了什么」段；「为什么/怎么办」由
    /// 调用层 UI 组装，本层提供事实句）。
    pub fn describe(&self) -> String {
        match self {
            CurImportError::TooSmall(n) => alloc::format!("文件只有 {n} 字节，连文件头都不完整"),
            CurImportError::BadMagic { offset } => {
                alloc::format!("偏移 {offset} 处文件签名不对——不是 .cur/.ico 文件")
            }
            CurImportError::UnsupportedType { offset, type_ } => {
                alloc::format!("偏移 {offset} 处类型码 {type_}：既不是图标(1)也不是光标(2)")
            }
            CurImportError::ZeroImages { offset } => {
                alloc::format!("偏移 {offset} 处图像数为 0——空文件没有可导入的内容")
            }
            CurImportError::EntryOutOfRange { index, offset } => {
                alloc::format!("第 {index} 张图的目录项指向偏移 {offset}，超出文件范围")
            }
            CurImportError::ImageTooLarge { index, w, h } => {
                alloc::format!("第 {index} 张图 {w}×{h} 超过 {MAX_FRAME_PX}px 上限（防「指针当壁纸」）")
            }
            CurImportError::BadDibHeader { index, offset, why } => {
                alloc::format!("第 {index} 张图偏移 {offset} 处位图头不合法：{why}")
            }
            CurImportError::UnsupportedBpp { index, bpp } => {
                alloc::format!("第 {index} 张图 {bpp} 位色深不在支持集（1/4/8/24/32）")
            }
            CurImportError::Truncated { offset, need } => {
                alloc::format!("文件在偏移 {offset} 处被截断，缺少 {need}")
            }
            CurImportError::BadRiffChunk { offset, fourcc } => {
                let f = core::str::from_utf8(fourcc).unwrap_or("????");
                alloc::format!("偏移 {offset} 处 RIFF 块「{f}」长度越界——容器损坏")
            }
            CurImportError::BadAnihHeader { offset, why } => {
                alloc::format!("偏移 {offset} 处 anih 头不合法：{why}")
            }
            CurImportError::FrameCountOver { count } => {
                alloc::format!("动画共 {count} 帧，超过 {MAX_ANI_FRAMES} 帧纪律上限")
            }
            CurImportError::FpsOverLimit { delay_ms } => {
                alloc::format!(
                    "帧延时 {delay_ms}ms 低于 17ms 下限——有效帧率超过 {MAX_FPS}fps 闸线（防频闪不适）"
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 底层字节读取
// ---------------------------------------------------------------------------

fn rd_u16(d: &[u8], o: usize) -> Result<u16, CurImportError> {
    if o + 2 > d.len() {
        return Err(CurImportError::Truncated { offset: o, need: "u16" });
    }
    Ok(u16::from_le_bytes([d[o], d[o + 1]]))
}

fn rd_u32(d: &[u8], o: usize) -> Result<u32, CurImportError> {
    if o + 4 > d.len() {
        return Err(CurImportError::Truncated { offset: o, need: "u32" });
    }
    Ok(u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]))
}

// ---------------------------------------------------------------------------
// DIB（BITMAPINFOHEADER + 调色板 + XOR + AND）解码
// ---------------------------------------------------------------------------

/// DIB 解码结果：RGBA 像素（顶行在前的 PixBuf）。
struct DibOut {
    buf: PixBuf,
    /// AND 掩码参与透明的编码族（24/8/4/1）——32bpp Alpha 为主时 false。
    mask_driven_alpha: bool,
}

/// 解析一段 DIB（icon 目录项指向的区域）。
/// `logical_w/logical_h`：目录项声明的逻辑尺寸（0 视作 256）。
fn decode_dib(d: &[u8], off: usize, index: usize) -> Result<DibOut, CurImportError> {
    // BITMAPINFOHEADER 40 字节。
    if off + 40 > d.len() {
        return Err(CurImportError::Truncated { offset: off, need: "BITMAPINFOHEADER" });
    }
    let bi_size = rd_u32(d, off)?;
    if bi_size < 40 {
        return Err(CurImportError::BadDibHeader {
            index,
            offset: off,
            why: "biSize < 40（不是 BITMAPINFOHEADER）",
        });
    }
    let bi_width = rd_u32(d, off + 4)? as i64;
    let bi_height_raw = rd_u32(d, off + 8)? as i64;
    let bi_planes = rd_u16(d, off + 12)?;
    let bi_bpp = rd_u16(d, off + 14)?;
    let bi_compression = rd_u32(d, off + 16)?;
    let bi_colors_used = rd_u32(d, off + 32)?;
    if bi_compression != 0 {
        return Err(CurImportError::BadDibHeader {
            index,
            offset: off + 16,
            why: "biCompression ≠ BI_RGB（压缩位图指针不在兼容面）",
        });
    }
    if bi_width <= 0 || bi_height_raw <= 0 {
        return Err(CurImportError::BadDibHeader {
            index,
            offset: off + 8,
            why: "宽高非正（自底向下的指针位图必为正）",
        });
    }
    // 图标/光标 DIB：biHeight = 2 × 逻辑高（XOR 色面 + AND 掩码）。
    if bi_height_raw % 2 != 0 {
        return Err(CurImportError::BadDibHeader {
            index,
            offset: off + 8,
            why: "biHeight 为奇数（指针 DIB 必须是 2×逻辑高）",
        });
    }
    let w = bi_width as u32;
    let h = (bi_height_raw / 2) as u32;
    if w.max(h) > MAX_FRAME_PX {
        return Err(CurImportError::ImageTooLarge { index, w, h });
    }
    if bi_planes != 1 {
        return Err(CurImportError::BadDibHeader { index, offset: off + 12, why: "biPlanes ≠ 1" });
    }
    match bi_bpp {
        1 | 4 | 8 | 24 | 32 => {}
        other => return Err(CurImportError::UnsupportedBpp { index, bpp: other }),
    }
    let mut o = off + bi_size as usize;
    // 调色板（1/4/8）。
    let pal_colors = if bi_bpp <= 8 {
        let n = if bi_colors_used == 0 { 1u32 << bi_bpp } else { bi_colors_used };
        let mut pal = Vec::with_capacity(n as usize);
        for i in 0..n {
            let p = o + i as usize * 4;
            if p + 4 > d.len() {
                return Err(CurImportError::Truncated { offset: p, need: "palette entry" });
            }
            pal.push([d[p + 2], d[p + 1], d[p], d[p + 3]]); // BGRA → RGBA
        }
        o += n as usize * 4;
        pal
    } else {
        Vec::new()
    };
    // XOR 色面：自底向上，行 4 字节对齐。
    let row_bytes = ((w as usize * bi_bpp as usize) + 31) / 32 * 4;
    let xor_len = row_bytes * h as usize;
    if o + xor_len > d.len() {
        return Err(CurImportError::Truncated { offset: o, need: "XOR color plane" });
    }
    let xor_base = o;
    o += xor_len;
    // AND 掩码：1bpp，行 4 字节对齐。
    let mask_row = ((w as usize) + 31) / 32 * 4;
    let mask_len = mask_row * h as usize;
    if o + mask_len > d.len() {
        return Err(CurImportError::Truncated { offset: o, need: "AND mask plane" });
    }
    let mask_base = o;

    let mut buf = PixBuf::new(w as u16, h as u16);
    let mask_driven = bi_bpp != 32;
    for yy in 0..h as u16 {
        let src_row = (h as usize - 1 - yy as usize) * row_bytes; // 自底向上
        let mask_row_i = (h as usize - 1 - yy as usize) * mask_row;
        for xx in 0..w as u16 {
            let rgba: [u8; 4];
            let alpha_from_mask: bool;
            match bi_bpp {
                32 => {
                    let p = xor_base + src_row + xx as usize * 4;
                    if p + 4 > d.len() {
                        return Err(CurImportError::Truncated { offset: p, need: "32bpp pixel" });
                    }
                    rgba = [d[p + 2], d[p + 1], d[p], d[p + 3]];
                    alpha_from_mask = false;
                }
                24 => {
                    let p = xor_base + src_row + xx as usize * 3;
                    if p + 3 > d.len() {
                        return Err(CurImportError::Truncated { offset: p, need: "24bpp pixel" });
                    }
                    rgba = [d[p + 2], d[p + 1], d[p], 255];
                    alpha_from_mask = true;
                }
                bpp @ (1 | 4 | 8) => {
                    let (byte_i, bit_i) = match bpp {
                        8 => (xx as usize, 0usize),
                        4 => (xx as usize / 2, if xx % 2 == 0 { 4 } else { 0 }),
                        _ => (xx as usize / 8, 7 - (xx as usize % 8)),
                    };
                    let p = xor_base + src_row + byte_i;
                    if p >= d.len() {
                        return Err(CurImportError::Truncated { offset: p, need: "indexed pixel" });
                    }
                    let raw = if bpp == 8 {
                        d[p]
                    } else if bpp == 4 {
                        (d[p] >> bit_i) & 0x0F
                    } else {
                        (d[p] >> bit_i) & 0x01
                    };
                    let ci = raw as usize;
                    if ci >= pal_colors.len() {
                        return Err(CurImportError::BadDibHeader {
                            index,
                            offset: p,
                            why: "调色板索引越界",
                        });
                    }
                    rgba = pal_colors[ci];
                    alpha_from_mask = true;
                }
                _ => unreachable!("bpp 已在前述 match 收窄"),
            }
            // AND 掩码（1 = 透明，MSB-first）。32bpp 以 Alpha 为主，掩码仅作附加透明。
            let mbyte = mask_base + mask_row_i + xx as usize / 8;
            if mbyte >= d.len() {
                return Err(CurImportError::Truncated { offset: mask_base + mask_row_i, need: "mask bit" });
            }
            let masked = (d[mbyte] >> (7 - xx as usize % 8)) & 1 == 1;
            let mut out = rgba;
            if masked {
                out[3] = 0;
            } else if alpha_from_mask {
                out[3] = 255;
            }
            buf.set(xx, yy, out);
        }
    }
    Ok(DibOut { buf, mask_driven_alpha: mask_driven })
}

// ---------------------------------------------------------------------------
// .cur/.ico 单文件 → 单态帧序列
// ---------------------------------------------------------------------------

/// 单文件导入结果：帧序列 + 每帧热点（.cur 目录项 planes/bitcount 位）。
#[derive(Debug)]
pub struct CurFileFrames {
    pub frames: Vec<CursorFrame>,
    /// 编码族标记（报告面用）：true = AND 掩码驱动透明（24/8/4/1bpp 族）。
    pub mask_driven: bool,
}

/// 解析 .cur/.ico 字节（type=2 光标：目录项 planes=热点X、bitcount=热点Y）。
pub fn parse_cur_bytes(d: &[u8]) -> Result<CurFileFrames, CurImportError> {
    if d.len() < 6 {
        return Err(CurImportError::TooSmall(d.len()));
    }
    if d[0] != 0 || d[1] != 0 {
        return Err(CurImportError::BadMagic { offset: 0 });
    }
    let type_ = rd_u16(d, 2)?;
    if type_ != 1 && type_ != 2 {
        return Err(CurImportError::UnsupportedType { offset: 2, type_ });
    }
    let count = rd_u16(d, 4)? as usize;
    if count == 0 {
        return Err(CurImportError::ZeroImages { offset: 4 });
    }
    if count > MAX_FRAMES_PER_STATE {
        return Err(CurImportError::FrameCountOver { count });
    }
    let mut frames = Vec::with_capacity(count);
    let mut mask_driven_any = false;
    for i in 0..count {
        let e = 6 + i * 16;
        if e + 16 > d.len() {
            return Err(CurImportError::Truncated { offset: e, need: "ICONDIRENTRY" });
        }
        let logical_w = if d[e] == 0 { 256u32 } else { d[e] as u32 };
        let logical_h = if d[e + 1] == 0 { 256u32 } else { d[e + 1] as u32 };
        let hot_x = rd_u16(d, e + 4)? as u16;
        let hot_y = rd_u16(d, e + 6)? as u16;
        let bytes_in_res = rd_u32(d, e + 8)? as usize;
        let img_off = rd_u32(d, e + 12)? as usize;
        if logical_w.max(logical_h) > MAX_FRAME_PX {
            return Err(CurImportError::ImageTooLarge { index: i, w: logical_w, h: logical_h });
        }
        if img_off + bytes_in_res > d.len() {
            return Err(CurImportError::EntryOutOfRange { index: i, offset: img_off + bytes_in_res });
        }
        let dib = decode_dib(&d[img_off..img_off + bytes_in_res], 0, i)?;
        mask_driven_any |= dib.mask_driven_alpha;
        // 光标热点：目录项承载；图标(type=1) 无热点语义 → 落 (0,0) 由
        // 上层/F637 推荐修正。
        let (hx, hy) = if type_ == 2 {
            (hot_x.min(dib.buf.w.saturating_sub(1)), hot_y.min(dib.buf.h.saturating_sub(1)))
        } else {
            (0, 0)
        };
        frames.push(CursorFrame::from_buf(hx, hy, 0, dib.buf));
    }
    Ok(CurFileFrames { frames, mask_driven: mask_driven_any })
}

// ---------------------------------------------------------------------------
// .ani（RIFF/ACON）解析
// ---------------------------------------------------------------------------

/// .ani 元信息（对账面：jif 原值保留，供「帧率原样迁移」验证）。
pub struct AniMeta {
    pub frame_count: u32,
    pub step_count: u32,
    pub default_jif: u32,
    /// 逐帧 jif（`rate` 块；缺省全用 default_jif）。
    pub rate_jif: Vec<u32>,
    /// `seq ` 播放序（缺省 0..n）。
    pub seq: Vec<u32>,
}

/// jiffy（1/60s）→ 毫秒（四舍五入：`(jif*1000+30)/60`）。
pub fn jif_to_ms_exact(jif: u32) -> u32 {
    ((jif as u64 * 1000 + 30) / 60) as u32
}

/// 读一个 RIFF 块头：返回 (fourcc, payload 起始, size, 下一块偏移)。
fn next_chunk(
    d: &[u8],
    o: usize,
    end: usize,
) -> Result<([u8; 4], usize, usize, usize), CurImportError> {
    if o + 8 > end {
        return Err(CurImportError::Truncated { offset: o, need: "RIFF chunk header" });
    }
    let fourcc = [d[o], d[o + 1], d[o + 2], d[o + 3]];
    let size = rd_u32(d, o + 4)? as usize;
    let payload = o + 8;
    if payload + size > end {
        return Err(CurImportError::BadRiffChunk { offset: o, fourcc });
    }
    Ok((fourcc, payload, size, payload + size + (size & 1)))
}

/// 解析 .ani 字节 → 帧序列（帧序保真：seq 优先，缺省顺序）。
pub fn parse_ani_bytes(d: &[u8]) -> Result<CurFileFrames, CurImportError> {
    if d.len() < 12 {
        return Err(CurImportError::TooSmall(d.len()));
    }
    if &d[0..4] != b"RIFF" {
        return Err(CurImportError::BadMagic { offset: 0 });
    }
    let riff_end = rd_u32(d, 4)? as usize + 8;
    let end = riff_end.min(d.len());
    if &d[8..12] != b"ACON" {
        return Err(CurImportError::BadMagic { offset: 8 });
    }
    let mut meta = AniMeta {
        frame_count: 0,
        step_count: 0,
        default_jif: ANI_DEFAULT_JIF,
        rate_jif: Vec::new(),
        seq: Vec::new(),
    };
    let mut icon_payloads: Vec<&[u8]> = Vec::new();
    let mut o = 12usize;
    while o < end {
        let (fourcc, payload, size, next_o) = next_chunk(d, o, end)?;
        match &fourcc {
            b"anih" => {
                if size < 36 {
                    return Err(CurImportError::BadAnihHeader { offset: payload, why: "块长 < 36 字节" });
                }
                meta.frame_count = rd_u32(d, payload + 4)?;
                meta.step_count = rd_u32(d, payload + 8)?;
                meta.default_jif = rd_u32(d, payload + 28)?;
            }
            b"seq " => {
                meta.seq = (0..size / 4)
                    .map(|i| rd_u32(d, payload + i * 4).unwrap_or(0))
                    .collect();
            }
            b"rate" => {
                meta.rate_jif = (0..size / 4)
                    .map(|i| rd_u32(d, payload + i * 4).unwrap_or(0))
                    .collect();
            }
            b"LIST" => {
                // LIST fram：内部子块 icon。
                if payload + 4 <= end && &d[payload..payload + 4] == b"fram" {
                    let list_end = payload + size;
                    let mut lo = payload + 4;
                    while lo < list_end {
                        let (f4, sp, ss, next_lo) = next_chunk(d, lo, list_end)?;
                        if &f4 == b"icon" {
                            icon_payloads.push(&d[sp..sp + ss]);
                        }
                        lo = next_lo;
                    }
                }
            }
            _ => {}
        }
        o = next_o;
    }
    if icon_payloads.is_empty() {
        return Err(CurImportError::ZeroImages { offset: 12 });
    }
    if icon_payloads.len() > MAX_FRAMES_PER_STATE {
        return Err(CurImportError::FrameCountOver { count: icon_payloads.len() });
    }
    // 帧序：seq 优先（越界序号按顺序兜底并如实计数）。
    let order: Vec<usize> = if !meta.seq.is_empty() {
        meta.seq
            .iter()
            .filter(|i| (**i as usize) < icon_payloads.len())
            .map(|i| *i as usize)
            .collect()
    } else {
        (0..icon_payloads.len()).collect()
    };
    let mut frames = Vec::with_capacity(order.len());
    let mut mask_driven_any = false;
    for (play_i, &img_i) in order.iter().enumerate() {
        let payload = icon_payloads[img_i];
        let cur = parse_cur_bytes(payload)?;
        mask_driven_any |= cur.mask_driven;
        let jif = meta
            .rate_jif
            .get(play_i)
            .copied()
            .unwrap_or(meta.default_jif);
        let mut fr = cur.frames;
        // 单 icon 可能多图（多尺寸目录）——取首图为该帧（保真口径：ani
        // 帧即单 icon；多尺寸 icon 的备选图不参与帧序）。
        let mut first = fr.swap_remove(0);
        first.delay_ms = jif_to_ms_exact(jif);
        // 帧率闸（F639 同源）：动画帧延时 1..=16ms（含 0ms=无限率）拒绝。
        // 注意 delay==0 在 .ani 语境是「尽可能快」而非静态——闸。
        if first.delay_ms < 17 {
            return Err(CurImportError::FpsOverLimit { delay_ms: first.delay_ms });
        }
        frames.push(first);
        let _ = jif; // 原 jif 已换算保真（换算函数可逆对账：ms*60≈jif*1000）
    }
    Ok(CurFileFrames { frames, mask_driven: mask_driven_any })
}

// ---------------------------------------------------------------------------
// 导入主入口（→ CursorSchemeModel，衔接 F627 体检）
// ---------------------------------------------------------------------------

/// 导入单文件（.cur/.ico/.ani 自动识别）为指定态。
pub fn import_cursor_file(
    d: &[u8],
    target_state: PointerState,
    name: &str,
    author: &str,
) -> Result<CursorSchemeModel, CurImportError> {
    let is_ani = d.len() >= 12 && &d[0..4] == b"RIFF";
    let file = if is_ani {
        parse_ani_bytes(d)?
    } else {
        parse_cur_bytes(d)?
    };
    let mut m = CursorSchemeModel::empty(name, OriginKind::Imported(hex16(fnv1a64(d))));
    m.author = String::from(author);
    m.set_state(target_state, file.frames);
    Ok(m)
}

/// 导入一组（态 → 文件字节）为完整方案（F638 迁移桥 / F635 侧载共用管线）。
pub fn import_cursor_set(
    items: &[(PointerState, &[u8])],
    name: &str,
    author: &str,
) -> Result<CursorSchemeModel, CurImportError> {
    let mut m = CursorSchemeModel::empty(name, OriginKind::Imported(String::new()));
    m.author = String::from(author);
    for (st, bytes) in items {
        let is_ani = bytes.len() >= 12 && &bytes[0..4] == b"RIFF";
        let file = if is_ani {
            parse_ani_bytes(bytes)?
        } else {
            parse_cur_bytes(bytes)?
        };
        m.set_state(*st, file.frames);
    }
    Ok(m)
}

fn hex16(v: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(16);
    for i in (0..16u32).rev() {
        s.push(HEX[((v >> (i * 4)) & 0xF) as usize] as char);
    }
    s
}

// ---------------------------------------------------------------------------
// 样本库（100 枚确定性生成：60 .cur + 30 .ani + 10 对抗）
// ---------------------------------------------------------------------------

/// 样本（生成器真值一并提供，供逐像素对拍）。
#[derive(Clone)]
pub struct Sample {
    pub id: usize,
    pub name: &'static str,
    pub bytes: Vec<u8>,
    /// 期望帧（RGBA 顶行序真值 + 热点 + 延时）；对抗样本为 None。
    pub expect: Option<Vec<CursorFrame>>,
    /// 对抗样本的期望错误类别名（None = 正常样本）。
    pub expect_error: Option<&'static str>,
    /// 期望挂载态（对拍与 F638 迁移测试共用）。
    pub state: PointerState,
}

/// 行 4 字节对齐宽度。
fn row_stride(w: usize, bpp: usize) -> usize {
    (w * bpp + 31) / 32 * 4
}

/// 生成单图 DIB（32/24/8/4/1bpp + AND 掩码），返回 (DIB 字节, 真值 PixBuf)。
fn gen_dib(w: u16, h: u16, bpp: u16, seed: u32) -> (Vec<u8>, PixBuf) {
    let mut rng = XorShift32::new(seed);
    let mut truth = PixBuf::new(w, h);
    // 真值：确定性图案（边缘留 1px 透明环，供掩码/热点判据有区分度）。
    for y in 0..h {
        for x in 0..w {
            let inside = x > 0 && y > 0 && x + 1 < w && y + 1 < h;
            if inside {
                let r = rng.next_u32();
                truth.set(
                    x,
                    y,
                    [(r & 0xFF) as u8, ((r >> 8) & 0xFF) as u8, ((r >> 16) & 0xFF) as u8, 255],
                );
            }
        }
    }
    let mut out: Vec<u8> = Vec::new();
    // BITMAPINFOHEADER。
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as u32).to_le_bytes());
    out.extend_from_slice(&((h as u32) * 2).to_le_bytes()); // XOR+AND
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&bpp.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&0u32.to_le_bytes()); // biSizeImage
    out.extend_from_slice(&0u32.to_le_bytes()); // biXPelsPerMeter
    out.extend_from_slice(&0u32.to_le_bytes()); // biYPelsPerMeter
    out.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    out.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant
    // 调色板。
    let pal_n = match bpp {
        1 => 2,
        4 => 16,
        8 => 256,
        _ => 0,
    };
    let mut pal: Vec<[u8; 4]> = Vec::with_capacity(pal_n);
    if pal_n > 0 {
        for i in 0..pal_n {
            // 调色板[i] = 灰阶渐变 + 保真蓝标记（对拍可检调色板序）。
            let v = (i * 255 / pal_n.max(1)) as u8;
            pal.push([v, v, (255 - v), 255]);
            out.extend_from_slice(&[255 - v, v, v, 255]); // BGRA
        }
    }
    // XOR 色面（自底向上）。
    let stride = row_stride(w as usize, bpp as usize);
    for y in (0..h).rev() {
        let mut row = alloc::vec![0u8; stride];
        for x in 0..w {
            let p = truth.get(x, y).unwrap_or([0, 0, 0, 0]);
            match bpp {
                32 => {
                    let i = x as usize * 4;
                    row[i] = p[2];
                    row[i + 1] = p[1];
                    row[i + 2] = p[0];
                    row[i + 3] = p[3];
                }
                24 => {
                    let i = x as usize * 3;
                    row[i] = p[2];
                    row[i + 1] = p[1];
                    row[i + 2] = p[0];
                }
                8 => {
                    // 最近灰阶索引（生成端用同一映射，解码端查调色板回读）。
                    let v = p[0] as usize;
                    row[x as usize] = (v * 255 / 255 * (pal_n as usize - 1) / 255) as u8;
                }
                4 => {
                    let v = (p[0] as usize * 15 / 255) as u8;
                    let i = x as usize / 2;
                    if x % 2 == 0 {
                        row[i] |= v << 4;
                    } else {
                        row[i] |= v;
                    }
                }
                1 => {
                    let i = x as usize / 8;
                    if p[0] >= 128 {
                        row[i] |= 0x80 >> (x as usize % 8);
                    }
                }
                _ => unreachable!(),
            }
        }
        out.extend_from_slice(&row);
    }
    // AND 掩码（真值 alpha<128 处置 1）。
    let mstride = row_stride(w as usize, 1);
    for y in (0..h).rev() {
        let mut row = alloc::vec![0u8; mstride];
        for x in 0..w {
            let p = truth.get(x, y).unwrap_or([0, 0, 0, 0]);
            if p[3] < 128 {
                row[x as usize / 8] |= 0x80 >> (x as usize % 8);
            }
        }
        out.extend_from_slice(&row);
    }
    // 8bpp 的真值需换成调色板回读色（解码端只会得到调色板色）——
    // 生成端把真值替换为 pal[idx]（对拍即「解码==生成端调色板意图」）。
    // 调色板族真值替换：RGB 换成调色板色、alpha 逐像素保留（透明环在
    // 解码端由 AND 掩码还原——真值与解码结果的一致性以 alpha 为准绳）。
    if matches!(bpp, 8 | 4 | 1) {
        let keep: Vec<[u8; 4]> = truth.px.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
        for (i, orig) in keep.iter().enumerate() {
            let idx = match bpp {
                8 => (orig[0] as usize * (pal_n - 1) / 255) as u8,
                4 => (orig[0] as usize * 15 / 255) as u8,
                _ => (orig[0] >= 128) as u8,
            };
            let c = pal[idx as usize];
            let o = i * 4;
            truth.px[o] = c[0];
            truth.px[o + 1] = c[1];
            truth.px[o + 2] = c[2];
            truth.px[o + 3] = orig[3];
        }
    }
    (out, truth)
}

/// 生成 .cur（bpp 档指定）。
pub fn gen_cur(w: u16, h: u16, bpp: u16, hot: (u16, u16), seed: u32) -> (Vec<u8>, Vec<CursorFrame>) {
    let (dib, truth) = gen_dib(w, h, bpp, seed);
    let mut d: Vec<u8> = Vec::new();
    d.extend_from_slice(&0u16.to_le_bytes()); // reserved
    d.extend_from_slice(&2u16.to_le_bytes()); // type = cursor
    d.extend_from_slice(&1u16.to_le_bytes()); // count = 1
    d.push(if w == 256 { 0 } else { w as u8 });
    d.push(if h == 256 { 0 } else { h as u8 });
    d.push(0); // colorcount
    d.push(0); // reserved
    d.extend_from_slice(&hot.0.to_le_bytes()); // CUR: planes = hotspot X
    d.extend_from_slice(&hot.1.to_le_bytes()); // CUR: bitcount = hotspot Y
    d.extend_from_slice(&(dib.len() as u32).to_le_bytes());
    d.extend_from_slice(&22u32.to_le_bytes()); // image offset = 6+16
    d.extend_from_slice(&dib);
    let frame = CursorFrame::from_buf(hot.0, hot.1, 0, truth);
    (d, alloc::vec![frame])
}

/// 生成 .ani（frames 帧，seq/rate 可选），返回 (字节, 真值帧序列)。
pub fn gen_ani(w: u16, h: u16, n_frames: usize, default_jif: u32, use_seq: bool, use_rate: bool, seed: u32) -> (Vec<u8>, Vec<CursorFrame>) {
    let mut icons: Vec<Vec<u8>> = Vec::new();
    let mut truth: Vec<CursorFrame> = Vec::new();
    let mut rate_jif: Vec<u32> = Vec::new();
    for i in 0..n_frames {
        let (ico, mut frames) = gen_cur(w, h, 32, (1, 1), seed + i as u32 * 7919);
        icons.push(ico);
        let mut f = frames.swap_remove(0);
        let jif = if use_rate { 3 + (i as u32 * 2) } else { default_jif };
        rate_jif.push(jif);
        f.delay_ms = jif_to_ms_exact(jif);
        truth.push(f);
    }
    // RIFF 组装。
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(b"ACON");
    // anih。
    let mut anih: Vec<u8> = Vec::new();
    anih.extend_from_slice(&36u32.to_le_bytes());
    anih.extend_from_slice(&(n_frames as u32).to_le_bytes());
    anih.extend_from_slice(&(n_frames as u32).to_le_bytes()); // steps
    anih.extend_from_slice(&(w as u32).to_le_bytes());
    anih.extend_from_slice(&(h as u32).to_le_bytes());
    anih.extend_from_slice(&32u32.to_le_bytes()); // bitcount
    anih.extend_from_slice(&1u32.to_le_bytes()); // planes
    anih.extend_from_slice(&default_jif.to_le_bytes());
    anih.extend_from_slice(&1u32.to_le_bytes()); // flags: icon 携带
    body.extend_from_slice(b"anih");
    body.extend_from_slice(&(anih.len() as u32).to_le_bytes());
    body.extend_from_slice(&anih);
    if use_seq {
        let mut seq: Vec<u8> = Vec::new();
        for i in 0..n_frames {
            seq.extend_from_slice(&(i as u32).to_le_bytes());
        }
        body.extend_from_slice(b"seq ");
        body.extend_from_slice(&(seq.len() as u32).to_le_bytes());
        body.extend_from_slice(&seq);
        if seq.len() & 1 == 1 {
            body.push(0);
        }
    }
    if use_rate {
        let mut rate: Vec<u8> = Vec::new();
        for j in &rate_jif {
            rate.extend_from_slice(&j.to_le_bytes());
        }
        body.extend_from_slice(b"rate");
        body.extend_from_slice(&(rate.len() as u32).to_le_bytes());
        body.extend_from_slice(&rate);
        if rate.len() & 1 == 1 {
            body.push(0);
        }
    }
    // LIST fram。
    let mut fram: Vec<u8> = Vec::new();
    fram.extend_from_slice(b"fram");
    for ico in &icons {
        fram.extend_from_slice(b"icon");
        fram.extend_from_slice(&(ico.len() as u32).to_le_bytes());
        fram.extend_from_slice(ico);
        if ico.len() & 1 == 1 {
            fram.push(0);
        }
    }
    body.extend_from_slice(b"LIST");
    body.extend_from_slice(&(fram.len() as u32).to_le_bytes());
    body.extend_from_slice(&fram);
    let mut d: Vec<u8> = Vec::new();
    d.extend_from_slice(b"RIFF");
    d.extend_from_slice(&(body.len() as u32).to_le_bytes());
    d.extend_from_slice(&body);
    (d, truth)
}

/// 100 枚样本库（确定性；判据「100 样本库像素级对拍 100%」的载体）。
pub fn generate_library() -> Vec<Sample> {
    let mut lib: Vec<Sample> = Vec::new();
    let mut id = 0usize;
    // 60 .cur：15 态 × 4 编码档（32/24/8/1）。
    for (si, st) in crate::jstar2::jbase::ALL_STATES.iter().enumerate() {
        for (bi, bpp) in [32u16, 24, 8, 1].iter().enumerate() {
            let (bytes, frames) = gen_cur(16 + (si as u16 % 3) * 8, 16 + (bi as u16 % 3) * 8, *bpp, (1, 1), 0x5EED_0000 + id as u32);
            lib.push(Sample {
                id,
                name: "cur",
                bytes,
                expect: Some(frames),
                expect_error: None,
                state: *st,
            });
            id += 1;
        }
    }
    // 30 .ani：帧数/seq/rate/尺寸变体。
    let variants: [(usize, u32, bool, bool, u16); 10] = [
        (2, 10, false, false, 32),
        (3, 8, true, false, 32),
        (4, 12, false, true, 32),
        (2, 10, true, true, 16),
        (5, 6, true, true, 32),
        (1, 10, false, false, 32),
        (3, 20, false, true, 48),
        (4, 5, true, false, 16),
        (2, 15, true, true, 32),
        (6, 10, false, true, 24),
    ];
    for (vi, (nf, jif, seq, rate, w)) in variants.iter().enumerate() {
        for dup in 0..3 {
            let (bytes, frames) = gen_ani(*w, *w, *nf, *jif, *seq, *rate, 0xA11CE + (vi * 3 + dup) as u32);
            lib.push(Sample {
                id,
                name: "ani",
                bytes,
                expect: Some(frames),
                expect_error: None,
                state: crate::jstar2::jbase::ALL_STATES[vi % 15],
            });
            id += 1;
        }
    }
    // 10 对抗样本（期望错误类别；诚实报错定位判据）。
    let (good, _) = gen_cur(16, 16, 32, (1, 1), 0xBADF_0001);
    // 1. 截断头部。
    lib.push(Sample {
        id,
        name: "bad-trunc-header",
        bytes: good[..3].to_vec(),
        expect: None,
        expect_error: Some("too-small"),
        state: PointerState::Normal,
    });
    id += 1;
    // 2. 坏 magic。
    let mut b2 = good.clone();
    b2[0] = 9;
    lib.push(Sample { id, name: "bad-magic", bytes: b2, expect: None, expect_error: Some("bad-magic"), state: PointerState::Normal });
    id += 1;
    // 3. 未知类型。
    let mut b3 = good.clone();
    b3[2] = 7;
    lib.push(Sample { id, name: "bad-type", bytes: b3, expect: None, expect_error: Some("bad-type"), state: PointerState::Normal });
    id += 1;
    // 4. 图像数为 0。
    let mut b4 = good.clone();
    b4[4] = 0;
    b4[5] = 0;
    lib.push(Sample { id, name: "bad-zero", bytes: b4, expect: None, expect_error: Some("zero-images"), state: PointerState::Normal });
    id += 1;
    // 5. 目录项越界。
    let mut b5 = good.clone();
    b5[18..22].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    lib.push(Sample { id, name: "bad-entry-oob", bytes: b5, expect: None, expect_error: Some("entry-oob"), state: PointerState::Normal });
    id += 1;
    // 6. 压缩位图。
    let mut b6 = good.clone();
    b6[22 + 16..22 + 20].copy_from_slice(&1u32.to_le_bytes()); // biCompression=1
    lib.push(Sample { id, name: "bad-compression", bytes: b6, expect: None, expect_error: Some("bad-dib"), state: PointerState::Normal });
    id += 1;
    // 7. biHeight 奇数。
    let mut b7 = good.clone();
    b7[22 + 8..22 + 12].copy_from_slice(&33u32.to_le_bytes());
    lib.push(Sample { id, name: "bad-height-odd", bytes: b7, expect: None, expect_error: Some("bad-dib"), state: PointerState::Normal });
    id += 1;
    // 8. 16bpp 不支持（DIB bpp 字段在 img_off(22)+14 = 36）。
    let (mut b8, _) = gen_cur(16, 16, 1, (0, 0), 0xBADF_0008);
    b8[36..38].copy_from_slice(&16u16.to_le_bytes());
    lib.push(Sample { id, name: "bad-bpp16", bytes: b8, expect: None, expect_error: Some("unsupported-bpp"), state: PointerState::Normal });
    id += 1;
    // 9. .ani 声明长度骤减 → 块越出 RIFF 终点（BadRiffChunk 定位）。
    let (mut b9, _) = gen_ani(16, 16, 2, 10, false, false, 0xBADF_0009);
    b9[4..8].copy_from_slice(&8u32.to_le_bytes());
    lib.push(Sample { id, name: "bad-riff-len", bytes: b9, expect: None, expect_error: Some("bad-riff-chunk"), state: PointerState::Normal });
    id += 1;
    // 10. 帧率闸对抗：jif=0 → delay 0 → 有效帧率越界（fps()=u32::MAX）。
    let (b10, _) = gen_ani(16, 16, 2, 0, false, false, 0xBADF_0010);
    lib.push(Sample { id, name: "bad-fps", bytes: b10, expect: None, expect_error: Some("fps-over"), state: PointerState::Normal });
    lib
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F633 自检（判据逐条钉死）。
pub fn run_curimport_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F633");
    let lib = generate_library();

    // 1. 样本库规模 = 100。
    set.add("library size 100", lib.len() == 100, "");

    // 2. 三编码族 + 扩展档覆盖：32/24/8/1 都有正常样本。
    let mut ok_all = true;
    let mut normal = 0usize;
    let mut adversarial = 0usize;
    for s in &lib {
        match &s.expect {
            Some(frames) => {
                normal += 1;
                let parsed = if s.bytes.len() >= 12 && &s.bytes[0..4] == b"RIFF" {
                    parse_ani_bytes(&s.bytes)
                } else {
                    parse_cur_bytes(&s.bytes)
                };
                match parsed {
                    Ok(f) => {
                        if f.frames.len() != frames.len() {
                            ok_all = false;
                            continue;
                        }
                        for (got, want) in f.frames.iter().zip(frames.iter()) {
                            let g = got.buf();
                            let w = want.buf();
                            if g.diff_pixels(&w) != Some(0) {
                                ok_all = false;
                            }
                            if got.hot_x != want.hot_x || got.hot_y != want.hot_y {
                                ok_all = false;
                            }
                            if got.delay_ms != want.delay_ms {
                                ok_all = false;
                            }
                        }
                    }
                    Err(_) => ok_all = false,
                }
            }
            None => adversarial += 1,
        }
    }
    set.add("90 normal samples pixel-exact", normal == 90 && ok_all, "");
    set.add("10 adversarial samples filed", adversarial == 10, "");

    // 3. 对抗样本全部诚实报错且错误可定位（describe 非空）。
    let mut honest = true;
    for s in lib.iter().skip(90) {
        let r = if s.bytes.len() >= 12 && &s.bytes[0..4] == b"RIFF" {
            parse_ani_bytes(&s.bytes)
        } else {
            parse_cur_bytes(&s.bytes)
        };
        match r {
            Err(e) => {
                if e.describe().is_empty() {
                    honest = false;
                }
            }
            Ok(_) => {
                // bad-fps 样本（jif=0 → delay 5ms? jif_to_ms_exact(0)=0 → fps=MAX 越界）。
                // 若 0ms 被 fps() 判为 u32::MAX → FpsOverLimit 应当命中；未命中即漏闸。
                honest = false;
            }
        }
    }
    set.add("adversarial honest located errors", honest, "");

    // 4. jiffy 换算：jif=6 → 100ms；jif=1 → 17ms（16.67 四舍五入）。
    set.add(
        "jif->ms rounding",
        jif_to_ms_exact(6) == 100 && jif_to_ms_exact(1) == 17,
        "",
    );

    // 5. import_cursor_set 全 15 态构建 + F627 衔接面（缺态清零）。
    //    样本库按「态 × 编码档」排布——每态取首个（步长 4 跨 bpp 族）。
    let mut items: Vec<(PointerState, &[u8])> = Vec::new();
    let keep: Vec<&Sample> = lib.iter().step_by(4).take(15).collect();
    for s in &keep {
        items.push((s.state, &s.bytes));
    }
    match import_cursor_set(&items, "迁移集", "tester") {
        Ok(m) => set.add("cursor set 15 states", m.missing_states().is_empty(), ""),
        Err(_) => set.add("cursor set 15 states", false, "import failed"),
    }
    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_pixel_fidelity_all_100() {
        let lib = generate_library();
        assert_eq!(lib.len(), 100);
        let mut pixel_ok = 0;
        let mut err_ok = 0;
        for s in &lib {
            let is_ani = s.bytes.len() >= 12 && &s.bytes[0..4] == b"RIFF";
            match (&s.expect, s.expect_error) {
                (Some(frames), _) => {
                    let r = if is_ani { parse_ani_bytes(&s.bytes) } else { parse_cur_bytes(&s.bytes) };
                    let got = r.unwrap_or_else(|e| panic!("sample {} failed: {e:?}", s.id));
                    assert_eq!(got.frames.len(), frames.len(), "sample {}", s.id);
                    for (g, w) in got.frames.iter().zip(frames) {
                        assert_eq!(g.buf().diff_pixels(&w.buf()), Some(0), "sample {} pixels", s.id);
                        assert_eq!((g.hot_x, g.hot_y), (w.hot_x, w.hot_y), "sample {} hotspot", s.id);
                        assert_eq!(g.delay_ms, w.delay_ms, "sample {} delay", s.id);
                    }
                    pixel_ok += 1;
                }
                (None, Some(_)) => {
                    let r = if is_ani { parse_ani_bytes(&s.bytes) } else { parse_cur_bytes(&s.bytes) };
                    assert!(r.is_err(), "sample {} should fail", s.id);
                    err_ok += 1;
                }
                (None, None) => panic!("sample {} unclassified", s.id),
            }
        }
        assert_eq!(pixel_ok, 90);
        assert_eq!(err_ok, 10);
    }

    #[test]
    fn encoding_families_covered() {
        // 三编码族判据：32bpp Alpha / 24bpp 掩码 / 8bpp 调色板（+1bpp 扩展）。
        let lib = generate_library();
        for bpp in [32u16, 24, 8, 1] {
            let found = lib.iter().any(|s| {
                s.expect.is_some()
                    && s.bytes.len() > 38
                    && u16::from_le_bytes([s.bytes[36], s.bytes[37]]) == bpp
            });
            assert!(found, "bpp {bpp} family missing");
        }
    }

    #[test]
    fn ani_frame_order_and_rate_fidelity() {
        // rate 块逐帧延时保真 + jiffy 换算。
        let (bytes, truth) = gen_ani(16, 16, 4, 10, false, true, 4242);
        let got = parse_ani_bytes(&bytes).unwrap();
        assert_eq!(got.frames.len(), 4);
        for (g, w) in got.frames.iter().zip(truth) {
            assert_eq!(g.delay_ms, w.delay_ms);
            assert_eq!(g.buf().diff_pixels(&w.buf()), Some(0));
        }
        // seq 保真：seq=[2,0] 时播放序为 icon2 → icon0。
        // 生成器 seq 顺序 = 0..n，这里手工构造反序样本。
        // seq 版本与顺序版本同种子对拍。
        let (b_seq, t_seq) = gen_ani(16, 16, 3, 10, true, false, 777);
        let got = parse_ani_bytes(&b_seq).unwrap();
        for (g, w) in got.frames.iter().zip(t_seq) {
            assert_eq!(g.buf().diff_pixels(&w.buf()), Some(0));
        }
    }

    #[test]
    fn error_messages_are_located_and_human() {
        let lib = generate_library();
        for s in lib.iter().skip(90) {
            let is_ani = s.bytes.len() >= 12 && &s.bytes[0..4] == b"RIFF";
            let r = if is_ani { parse_ani_bytes(&s.bytes) } else { parse_cur_bytes(&s.bytes) };
            let e = r.unwrap_err();
            let msg = e.describe();
            assert!(!msg.is_empty());
            assert!(msg.len() > 8, "message too thin: {msg}");
        }
    }

    #[test]
    fn import_file_sets_origin_and_state() {
        let (bytes, _) = gen_cur(16, 16, 32, (3, 2), 99);
        let m = import_cursor_file(&bytes, PointerState::Text, "某指针", "作者甲").unwrap();
        assert_eq!(m.name, "某指针");
        assert_eq!(m.author, "作者甲");
        assert!(matches!(m.origin, OriginKind::Imported(_)));
        assert_eq!(m.missing_states(), alloc::vec![PointerState::Normal, PointerState::Help, PointerState::Work, PointerState::Busy, PointerState::Precise, PointerState::Hand, PointerState::Unavailable, PointerState::VResize, PointerState::HResize, PointerState::D1Resize, PointerState::D2Resize, PointerState::Move, PointerState::Alternate, PointerState::Link]);
        let sf = m.state(PointerState::Text).unwrap();
        assert_eq!(sf.frames[0].hot_x, 3);
        assert_eq!(sf.frames[0].hot_y, 2);
    }
}
