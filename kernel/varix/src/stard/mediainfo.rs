//! F094 媒体信息悬浮 · 完整设计（STAR I 主册 G-C-24）。
//!
//! **判据（主册）**：MP4/MKV/WebM/FLAC/MP3 五容器样本解析全对；悬停到
//! 显示 <100ms（缓存命中时）。
//!
//! **设计要点（主册）**：
//! - 悬停视频/音频文件 800ms 出信息提示条：时长/分辨率/码率/帧率（视频）
//!   或时长/码率/采样率（音频）；解析轻量（容器头信息，不解码全文件）；
//! - 解析结果入 F093 缓存库（哈希键共享——本模块 MediaInfoStore 与
//!   thumbeng 同键同纪律，进程内一份账）；缓存命中 → 悬停即显（<100ms），
//!   未命中 → 800ms 悬停等待期后台解析完成再显；
//! - 容器损坏 → 「无法读取媒体信息」（轻量场景轻量处理——不出错误弹窗）；
//!   超大文件（>8GB）只读头部（毫秒级）；编码信息缺失 → 字段省略
//!   （Option 语义，不给「未知」占位——诚实留白）；
//! - 时长格式 hh:mm:ss 自动缩位；分辨率含旋转修正（手机竖拍视频显示
//!   1080×1920 如实）；码率显示总码率+视频码率双值（专业用户友好）。
//!
//! 解析面全部手写（MP4 box 走查含 stsz 轨字节实算 / EBML varint / FLAC
//! STREAMINFO / MP3 帧头+Xing），零外部依赖、对抗样本不 panic（F176
//! 安全纪律同源）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计与设计细节）
// ---------------------------------------------------------------------------

/// 悬停触发等待（ms）——未命中缓存时的出条延迟。
pub const HOVER_DELAY_MS: u64 = 800;

/// 缓存命中出条判线（ms）。
pub const CACHE_HIT_LINE_MS: u64 = 100;

/// 超大文件门（字节）——超过只读头部（毫秒级）。
pub const HUGE_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// 提示条圆角（px，4K 资产管线样式）。
pub const TIP_RADIUS_PX: u32 = 4;

/// 提示条与缩略图间距（px）。
pub const TIP_GAP_PX: u32 = 8;

// ---------------------------------------------------------------------------
// 统一信息模型
// ---------------------------------------------------------------------------

/// 容器族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    Mp4,
    Mkv,
    WebM,
    Flac,
    Mp3,
}

/// 解析出的媒体信息（缺字段 = None——字段省略不给占位）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaInfo {
    pub container: Option<Container>,
    /// 总时长（ms）。
    pub duration_ms: Option<u64>,
    /// 显示分辨率（旋转修正后）。
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// 总码率（bps）。
    pub total_bps: Option<u64>,
    /// 视频轨码率（bps，stsz 轨字节实算——专业用户双值面，不虚标）。
    pub video_bps: Option<u64>,
    /// 采样率（Hz，音频）。
    pub sample_rate: Option<u32>,
    /// 声道数（音频）。
    pub channels: Option<u8>,
    /// 源旋转角（0/90/180/270，显示修正前）。
    pub rotation: Option<u16>,
}

impl MediaInfo {
    /// 时长文本：hh:mm:ss 自动缩位（<1h 显示 mm:ss，<1min 显示 s）。
    pub fn duration_label(&self) -> String {
        let ms = match self.duration_ms {
            Some(v) => v,
            None => return String::new(),
        };
        let s = ms / 1000;
        if s >= 3600 {
            alloc::format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
        } else if s >= 60 {
            alloc::format!("{}:{:02}", s / 60, s % 60)
        } else {
            alloc::format!("{}s", s)
        }
    }

    /// 分辨率文本（已是旋转修正后的显示值）。
    pub fn resolution_label(&self) -> String {
        match (self.width, self.height) {
            (Some(w), Some(h)) => alloc::format!("{}×{}", w, h),
            _ => String::new(),
        }
    }

    /// 是否视频（有分辨率字段即视频面）。
    pub fn is_video(&self) -> bool {
        self.width.is_some()
    }
}

// ---------------------------------------------------------------------------
// MP4 解析（box 走查：ftyp/moov/mvhd/trak/tkhd/mdia/hdlr/mdhd/stbl/stts/stsz）
// ---------------------------------------------------------------------------

fn be32(b: &[u8], o: usize) -> Option<u32> {
    if o + 4 > b.len() { return None; }
    Some(u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]))
}

fn be64(b: &[u8], o: usize) -> Option<u64> {
    if o + 8 > b.len() { return None; }
    Some(u64::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3], b[o + 4], b[o + 5], b[o + 6], b[o + 7]]))
}

/// box 头遍历器：对每个 body 调 `f(ty, body_lo, body_hi)`，回 false 停止。
fn walk_boxes(b: &[u8], from: usize, to: usize, f: &mut impl FnMut(&[u8; 4], usize, usize) -> bool) {
    let mut i = from;
    for _ in 0..64 {
        if i + 8 > to {
            return;
        }
        let size32 = match be32(b, i) {
            Some(v) => v as u64,
            None => return,
        };
        let (header, size) = if size32 == 1 {
            match be64(b, i + 8) {
                Some(v) => (16usize, v),
                None => return,
            }
        } else if size32 == 0 {
            (8usize, (to - i) as u64)
        } else {
            (8usize, size32)
        };
        if size < header as u64 || i as u64 + size > to as u64 {
            return;
        }
        let mut ty = [0u8; 4];
        ty.copy_from_slice(&b[i + 4..i + 8]);
        let body_lo = i + header;
        let body_hi = i + size as usize;
        if !f(&ty, body_lo, body_hi) {
            return;
        }
        i = body_hi;
    }
}

/// MP4 解析：moov 元数据只读头部。
pub fn parse_mp4(data: &[u8], file_bytes: u64) -> Option<MediaInfo> {
    if data.len() < 12 || &data[4..8] != b"ftyp" {
        return None;
    }
    let mut info = MediaInfo { container: Some(Container::Mp4), ..MediaInfo::default() };
    let mut found_moov = false;
    walk_boxes(data, 0, data.len(), &mut |ty, lo, hi| {
        if ty == b"moov" {
            found_moov = true;
            walk_boxes(data, lo, hi, &mut |ty2, lo2, hi2| {
                if ty2 == b"mvhd" {
                    parse_mvhd(data, lo2, hi2, &mut info);
                } else if ty2 == b"trak" {
                    parse_trak(data, lo2, hi2, &mut info);
                }
                true
            });
            return false; // moov 拿到即停。
        }
        true
    });
    if !found_moov {
        return None;
    }
    // 旋转修正（显示分辨率）。
    apply_rotation(&mut info);
    // 总码率：文件字节 ×8 / 时长。
    if let Some(dur) = info.duration_ms {
        if dur > 0 && file_bytes > 0 {
            info.total_bps = Some(file_bytes.saturating_mul(8000) / dur);
        }
    }
    Some(info)
}

/// mvhd：影片级 timescale + duration → duration_ms。
fn parse_mvhd(data: &[u8], lo: usize, _hi: usize, info: &mut MediaInfo) {
    let version = match data.get(lo) {
        Some(v) => *v,
        None => return,
    };
    let (ts, dur) = if version == 1 {
        match (be32(data, lo + 20), be64(data, lo + 24)) {
            (Some(a), Some(b)) => (a as u64, b),
            _ => return,
        }
    } else {
        match (be32(data, lo + 12), be32(data, lo + 16)) {
            (Some(a), Some(b)) => (a as u64, b as u64),
            _ => return,
        }
    };
    if ts > 0 && dur > 0 {
        info.duration_ms = Some(dur.saturating_mul(1000) / ts);
    }
}

/// trak 候选：tkhd 几何 + mdia 轨型/时长/stbl 轨字节——hdlr 确认 vide 后才生效。
#[derive(Default)]
struct TrakScan {
    handler: Option<[u8; 4]>,
    tk_w: Option<u32>,
    tk_h: Option<u32>,
    rotation: u16,
    md_timescale: u64,
    md_duration: u64,
    stsz_bytes: u64,
}

fn parse_trak(data: &[u8], lo: usize, hi: usize, info: &mut MediaInfo) {
    let mut scan = TrakScan::default();
    walk_boxes(data, lo, hi, &mut |ty, l, h| {
        if ty == b"tkhd" {
            parse_tkhd(data, l, h, &mut scan);
        } else if ty == b"mdia" {
            walk_boxes(data, l, h, &mut |ty2, l2, h2| {
                if ty2 == b"hdlr" && l2 + 12 <= h2 {
                    let mut handler = [0u8; 4];
                    handler.copy_from_slice(&data[l2 + 8..l2 + 12]);
                    scan.handler = Some(handler);
                } else if ty2 == b"mdhd" {
                    parse_mdhd(data, l2, &mut scan);
                } else if ty2 == b"minf" {
                    walk_boxes(data, l2, h2, &mut |ty3, l3, h3| {
                        if ty3 == b"stbl" {
                            walk_boxes(data, l3, h3, &mut |ty4, l4, h4| {
                                if ty4 == b"stsz" {
                                    scan.stsz_bytes += stsz_total_bytes(data, l4, h4);
                                }
                                true
                            });
                        }
                        true
                    });
                }
                true
            });
        }
        true
    });
    // 只有视频轨的几何/轨码率进信息面（音轨 tkhd 尺寸无意义）。
    if scan.handler == Some(*b"vide") {
        info.width = scan.tk_w;
        info.height = scan.tk_h;
        info.rotation = Some(scan.rotation);
        if scan.md_timescale > 0 && scan.md_duration > 0 && scan.stsz_bytes > 0 {
            let dur_ms = scan.md_duration.saturating_mul(1000) / scan.md_timescale;
            if dur_ms > 0 {
                info.video_bps = Some(scan.stsz_bytes.saturating_mul(8000) / dur_ms);
            }
        }
    }
}

/// tkhd：尺寸（16.16 定点）+ 旋转矩阵（0/90/180/270 识别）。
fn parse_tkhd(data: &[u8], l: usize, h: usize, scan: &mut TrakScan) {
    let version = *data.get(l).unwrap_or(&0);
    // verflags(4) + ctime(4/8) + mtime(4/8) + trackid(4) + resv(4) + duration(4/8)
    // + resv(8) + layer(2) + alt(2) + volume(2) + resv(2) + matrix(36) + w(4) + h(4)
    let m = l + 4 + if version == 1 { 8 + 8 + 4 + 4 + 8 } else { 4 + 4 + 4 + 4 + 4 } + 8 + 2 + 2 + 2 + 2;
    if m + 36 + 8 > h {
        return;
    }
    let rd = |o: usize| -> i32 { be32(data, o).unwrap_or(0) as i32 };
    // 行主序：a c tx / b d ty / u v w。
    let a = rd(m);
    let b = rd(m + 4);
    let c = rd(m + 8);
    let d = rd(m + 12);
    scan.rotation = if a != 0 && d != 0 && b == 0 && c == 0 {
        if a == d { 0 } else { 180 }
    } else if a == 0 && d == 0 && b != 0 && c != 0 {
        // 旋转 90°（CCW 矩阵 [0,1;-1,0]——手机竖拍常规位）与 270°。
        if b > 0 && c < 0 { 90 } else { 270 }
    } else {
        0
    };
    let w16 = rd(m + 36);
    let h16 = rd(m + 40);
    if w16 > 0 && h16 > 0 {
        scan.tk_w = Some((w16 >> 16) as u32);
        scan.tk_h = Some((h16 >> 16) as u32);
    }
}

/// mdhd：轨级 timescale + duration（视频轨码率分母）。
fn parse_mdhd(data: &[u8], l: usize, scan: &mut TrakScan) {
    let version = *data.get(l).unwrap_or(&0);
    let (ts, dur) = if version == 1 {
        match (be32(data, l + 20), be64(data, l + 24)) {
            (Some(a), Some(b)) => (a as u64, b),
            _ => return,
        }
    } else {
        match (be32(data, l + 12), be32(data, l + 16)) {
            (Some(a), Some(b)) => (a as u64, b as u64),
            _ => return,
        }
    };
    scan.md_timescale = ts;
    scan.md_duration = dur;
}

/// stsz：轨字节实算（sample_size 恒定口径 or entries 求和）。
fn stsz_total_bytes(data: &[u8], l: usize, h: usize) -> u64 {
    let fixed = be32(data, l + 4).unwrap_or(0) as u64;
    let count = match be32(data, l + 8) {
        Some(v) => v as u64,
        None => return 0,
    };
    if fixed > 0 {
        return fixed.saturating_mul(count);
    }
    let mut sum = 0u64;
    let base = l + 12;
    for k in 0..count as usize {
        match be32(data, base + k * 4) {
            Some(v) => sum += v as u64,
            None => break,
        }
    }
    let _ = h;
    sum
}

/// 旋转修正：90/270 时宽高互换（手机竖拍 1080×1920 如实）。
fn apply_rotation(info: &mut MediaInfo) {
    if matches!(info.rotation, Some(90) | Some(270)) {
        let w = info.width;
        let h = info.height;
        info.width = h;
        info.height = w;
    }
}

// ---------------------------------------------------------------------------
// MKV/WebM 解析（EBML：Segment→Info(Duration/TimecodeScale) +
// Tracks→TrackEntry→Video/Audio）
// ---------------------------------------------------------------------------

/// 读 EBML data-size vint（返回 (值, 消费字节数)）。长度标记位剥离，
/// 畸形势（前导 0 / 截断）返回 None。
fn ebml_vint(b: &[u8], o: usize) -> Option<(u64, usize)> {
    if o >= b.len() {
        return None;
    }
    let first = b[o];
    if first == 0 {
        return None;
    }
    let mut len = 0u8;
    let mut mask = 0x80u8;
    while mask & first == 0 && len < 8 {
        len += 1;
        mask >>= 1;
    }
    len += 1;
    if o + len as usize > b.len() {
        return None;
    }
    let value_mask: u8 = if len >= 8 { 0xFF } else { ((1u16 << (8 - len)) - 1) as u8 };
    let mut val = (first & value_mask) as u64;
    for k in 1..len as usize {
        val = (val << 8) | b[o + k] as u64;
    }
    Some((val, len as usize))
}

fn ebml_id(b: &[u8], o: usize) -> Option<(u32, usize)> {
    if o >= b.len() {
        return None;
    }
    let first = b[o];
    let len = if first & 0x80 != 0 { 1 } else if first & 0x40 != 0 { 2 } else if first & 0x20 != 0 { 3 } else { 4 };
    if o + len > b.len() {
        return None;
    }
    let mut id = 0u32;
    for k in 0..len {
        id = (id << 8) | b[o + k] as u32;
    }
    Some((id, len))
}

const EBML_SEGMENT: u32 = 0x1853_8067;
const EBML_INFO: u32 = 0x1549_A966;
const EBML_TIMECODESCALE: u32 = 0x002A_D7B1;
const EBML_DURATION: u32 = 0x0000_4489;
const EBML_TRACKS: u32 = 0x1654_AE6B;
const EBML_TRACKENTRY: u32 = 0x0000_00AE;
const EBML_TRACKTYPE: u32 = 0x0000_0083;
const EBML_VIDEO: u32 = 0x0000_00E0;
const EBML_PIXELW: u32 = 0x0000_00B0;
const EBML_PIXELH: u32 = 0x0000_00BA;
const EBML_AUDIO: u32 = 0x0000_00E1;
const EBML_SAMPLINGFREQ: u32 = 0x0000_00B5;
const EBML_CHANNELS: u32 = 0x0000_009F;

/// MKV/WebM 解析。`is_webm` 仅做容器族标注（语义面同一 EBML）。
pub fn parse_mkv(data: &[u8], is_webm: bool) -> Option<MediaInfo> {
    if data.len() < 4 || &data[0..4] != b"\x1A\x45\xDF\xA3" {
        return None;
    }
    let mut info = MediaInfo {
        container: Some(if is_webm { Container::WebM } else { Container::Mkv }),
        ..MediaInfo::default()
    };
    // 顶层找 Segment。
    let mut i = 0usize;
    let seg_range: Option<(usize, usize)> = loop {
        let (id, idlen) = ebml_id(data, i)?;
        let (size, szlen) = ebml_vint(data, i + idlen)?;
        let body = i + idlen + szlen;
        if id == EBML_SEGMENT {
            // 无界 Segment（size 全 1 = 超文件尾）扫到数据尾。
            let end = if size as usize >= data.len() { data.len() } else { body + size as usize };
            break Some((body, end));
        }
        i = body + size as usize;
    };
    let (slo, shi) = seg_range?;
    // Segment 内走查 Info / Tracks（轮次限定防畸形长链）。
    let mut i = slo;
    for _ in 0..64 {
        if i >= shi {
            break;
        }
        let (id, idlen) = match ebml_id(data, i) {
            Some(v) => v,
            None => break,
        };
        let (size, szlen) = match ebml_vint(data, i + idlen) {
            Some(v) => v,
            None => break,
        };
        let body = i + idlen + szlen;
        let end = (body + size as usize).min(shi);
        if id == EBML_INFO {
            parse_mkv_info(data, body, end, &mut info);
        } else if id == EBML_TRACKS {
            parse_mkv_tracks(data, body, end, &mut info);
        }
        i = end;
    }
    Some(info)
}

fn parse_mkv_info(data: &[u8], lo: usize, hi: usize, info: &mut MediaInfo) {
    let mut timescale: u64 = 1_000_000; // EBML 缺省 1ms。
    let mut dur_ns: Option<u64> = None;
    let mut i = lo;
    for _ in 0..32 {
        if i >= hi {
            break;
        }
        let (id, idlen) = match ebml_id(data, i) {
            Some(v) => v,
            None => break,
        };
        let (size, szlen) = match ebml_vint(data, i + idlen) {
            Some(v) => v,
            None => break,
        };
        let body = i + idlen + szlen;
        let end = (body + size as usize).min(hi);
        if id == EBML_TIMECODESCALE && body < end {
            if let Some((v, _)) = ebml_vint(data, body) {
                timescale = v.max(1);
            }
        } else if id == EBML_DURATION && (size == 4 || size == 8) && body + size as usize <= end {
            // Duration 是 EBML float（4 或 8 字节）。
            let mut raw = [0u8; 8];
            raw[..size as usize].copy_from_slice(&data[body..body + size as usize]);
            let f = if size == 4 { f32::from_bits(u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]])) as f64 } else { f64::from_bits(u64::from_be_bytes(raw)) };
            if f.is_finite() && f > 0.0 {
                dur_ns = Some((f * timescale as f64) as u64);
            }
        }
        i = end;
    }
    if let Some(ns) = dur_ns {
        info.duration_ms = Some(ns / 1_000_000);
    }
}

fn parse_mkv_tracks(data: &[u8], lo: usize, hi: usize, info: &mut MediaInfo) {
    let mut i = lo;
    for _ in 0..32 {
        if i >= hi {
            break;
        }
        let (id, idlen) = match ebml_id(data, i) {
            Some(v) => v,
            None => break,
        };
        let (size, szlen) = match ebml_vint(data, i + idlen) {
            Some(v) => v,
            None => break,
        };
        let body = i + idlen + szlen;
        let end = (body + size as usize).min(hi);
        if id == EBML_TRACKENTRY {
            parse_mkv_track_entry(data, body, end, info);
        }
        i = end;
    }
}

fn parse_mkv_track_entry(data: &[u8], lo: usize, hi: usize, info: &mut MediaInfo) {
    let mut track_type: u64 = 0;
    let mut i = lo;
    // 先找 TrackType（Video/Audio 子元素的语义前置）。
    for _ in 0..16 {
        if i >= hi {
            break;
        }
        let (id, idlen) = match ebml_id(data, i) {
            Some(v) => v,
            None => return,
        };
        let (size, szlen) = match ebml_vint(data, i + idlen) {
            Some(v) => v,
            None => return,
        };
        let body = i + idlen + szlen;
        if id == EBML_TRACKTYPE && body < hi {
            track_type = ebml_vint(data, body).map(|(v, _)| v).unwrap_or(0);
        }
        i = body + size as usize;
    }
    let mut i = lo;
    for _ in 0..16 {
        if i >= hi {
            break;
        }
        let (id, idlen) = match ebml_id(data, i) {
            Some(v) => v,
            None => return,
        };
        let (size, szlen) = match ebml_vint(data, i + idlen) {
            Some(v) => v,
            None => return,
        };
        let body = i + idlen + szlen;
        let end = (body + size as usize).min(hi);
        if id == EBML_VIDEO && track_type == 1 {
            let mut j = body;
            for _ in 0..16 {
                if j >= end {
                    break;
                }
                let (id2, il2) = match ebml_id(data, j) {
                    Some(v) => v,
                    None => break,
                };
                let (size2, sl2) = match ebml_vint(data, j + il2) {
                    Some(v) => v,
                    None => break,
                };
                let b2 = j + il2 + sl2;
                if id2 == EBML_PIXELW && b2 < end {
                    info.width = ebml_vint(data, b2).map(|(v, _)| v as u32);
                } else if id2 == EBML_PIXELH && b2 < end {
                    info.height = ebml_vint(data, b2).map(|(v, _)| v as u32);
                }
                j = b2 + size2 as usize;
            }
        } else if id == EBML_AUDIO && track_type == 2 {
            let mut j = body;
            for _ in 0..16 {
                if j >= end {
                    break;
                }
                let (id2, il2) = match ebml_id(data, j) {
                    Some(v) => v,
                    None => break,
                };
                let (size2, sl2) = match ebml_vint(data, j + il2) {
                    Some(v) => v,
                    None => break,
                };
                let b2 = j + il2 + sl2;
                if id2 == EBML_SAMPLINGFREQ && size2 == 4 && b2 + 4 <= end {
                    let f = f32::from_bits(u32::from_be_bytes([data[b2], data[b2 + 1], data[b2 + 2], data[b2 + 3]]));
                    if f.is_finite() && f > 0.0 {
                        info.sample_rate = Some(f as u32);
                    }
                } else if id2 == EBML_CHANNELS && b2 < end {
                    info.channels = ebml_vint(data, b2).map(|(v, _)| v as u8);
                }
                j = b2 + size2 as usize;
            }
        }
        i = body + size as usize;
    }
}

// ---------------------------------------------------------------------------
// FLAC 解析（fLaC 魔数 → STREAMINFO 元数据块）
// ---------------------------------------------------------------------------

/// FLAC STREAMINFO：采样率 20bit / 声道 3bit(+1) / 总样本 36bit。
pub fn parse_flac(data: &[u8]) -> Option<MediaInfo> {
    if data.len() < 42 || &data[0..4] != b"fLaC" {
        return None;
    }
    // 元数据块头：1B(最后块标志+类型) + 3B(长度)。STREAMINFO = 类型 0。
    if data[4] & 0x7F != 0 {
        return None;
    }
    let si = 8usize; // STREAMINFO 体起点（块头 4 字节后）。
    let sample_rate = ((data[si + 10] as u64) << 12) | ((data[si + 11] as u64) << 4) | ((data[si + 12] as u64) >> 4);
    let channels = (((data[si + 12] >> 1) & 0x07) + 1) as u8;
    let total = (((data[si + 13] as u64) & 0x0F) << 32)
        | ((data[si + 14] as u64) << 24)
        | ((data[si + 15] as u64) << 16)
        | ((data[si + 16] as u64) << 8)
        | data[si + 17] as u64;
    if sample_rate == 0 {
        return None;
    }
    let mut info = MediaInfo {
        container: Some(Container::Flac),
        sample_rate: Some(sample_rate as u32),
        channels: Some(channels),
        ..MediaInfo::default()
    };
    if total > 0 {
        info.duration_ms = Some(total * 1000 / sample_rate);
    }
    Some(info)
}

// ---------------------------------------------------------------------------
// MP3 解析（ID3 跳过 + 帧头同步 + Xing VBR 检测 + CBR 估算兜底）
// ---------------------------------------------------------------------------

const MP3_BITRATES_V1L3: [u16; 16] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0];
const MP3_BITRATES_V2L3: [u16; 16] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0];
const MP3_RATES_V1: [u32; 3] = [44100, 48000, 32000];
const MP3_RATES_V2: [u32; 3] = [22050, 24000, 16000];
const MP3_RATES_V25: [u32; 3] = [11025, 12000, 8000];

/// MP3 解析：首个帧同步 → 帧头字段；Xing/Info 头在位 → VBR 帧数精确时长，
/// 否则 CBR 文件字节估算。
pub fn parse_mp3(data: &[u8], file_bytes: u64) -> Option<MediaInfo> {
    if data.len() < 8 {
        return None;
    }
    // 跳 ID3v2（若有）：'ID3' + syncsafe size。
    let mut start = 0usize;
    if data.len() >= 10 && &data[0..3] == b"ID3" {
        let sz = (((data[6] as usize) & 0x7F) << 21)
            | (((data[7] as usize) & 0x7F) << 14)
            | (((data[8] as usize) & 0x7F) << 7)
            | ((data[9] as usize) & 0x7F);
        start = 10 + sz;
    }
    if start + 4 > data.len() {
        return None;
    }
    // 找同步字 0xFF Ex/Fx（前扫窗口 4KB——防恶意大 ID3 后长期无同步）。
    let scan_end = data.len().min(start + 4096);
    let mut i = start;
    while i + 4 <= scan_end && (data[i] != 0xFF || (data[i + 1] & 0xE0) != 0xE0) {
        i += 1;
    }
    if i + 4 > data.len() {
        return None;
    }
    let b1 = data[i + 1];
    let b2 = data[i + 2];
    let b3 = data[i + 3];
    let version_bits = (b1 >> 3) & 0x03; // 0=MPEG2.5 2=MPEG2 3=MPEG1
    let layer_bits = (b1 >> 1) & 0x03; // 1=Layer3
    if layer_bits != 0x01 {
        return None; // 本版只承诺 Layer III（差异表）。
    }
    let bitrate_idx = (b2 >> 4) as usize;
    let rate_idx = ((b2 >> 2) & 0x03) as usize;
    if bitrate_idx == 0 || bitrate_idx >= 15 || rate_idx == 3 {
        return None;
    }
    let (bitrate_kbps, sample_rate) = match version_bits {
        3 => (MP3_BITRATES_V1L3[bitrate_idx] as u64, MP3_RATES_V1[rate_idx]),
        2 => (MP3_BITRATES_V2L3[bitrate_idx] as u64, MP3_RATES_V2[rate_idx]),
        0 => (MP3_BITRATES_V2L3[bitrate_idx] as u64, MP3_RATES_V25[rate_idx]),
        _ => return None,
    };
    let channels: u8 = if (b3 >> 6) & 0x03 == 3 { 1 } else { 2 };
    let mut info = MediaInfo {
        container: Some(Container::Mp3),
        sample_rate: Some(sample_rate),
        channels: Some(channels),
        total_bps: Some(bitrate_kbps * 1000),
        ..MediaInfo::default()
    };
    // Xing/Info 头检测（帧内偏移按版本/声道）。
    let xing_off = i + 4 + if version_bits == 3 { if channels == 2 { 32 } else { 17 } } else if channels == 2 { 17 } else { 9 };
    if xing_off + 12 <= data.len() {
        let tag = &data[xing_off..xing_off + 4];
        if tag == b"Xing" || tag == b"Info" {
            let flags = u32::from_be_bytes([data[xing_off + 4], data[xing_off + 5], data[xing_off + 6], data[xing_off + 7]]);
            if flags & 0x01 != 0 {
                let frames = u32::from_be_bytes([
                    data[xing_off + 8], data[xing_off + 9], data[xing_off + 10], data[xing_off + 11],
                ]) as u64;
                // 每帧样本：MPEG1 L3=1152，MPEG2/2.5 L3=576。
                let spf: u64 = if version_bits == 3 { 1152 } else { 576 };
                if sample_rate > 0 && frames > 0 {
                    info.duration_ms = Some(frames * spf * 1000 / sample_rate as u64);
                }
            }
        }
    }
    if info.duration_ms.is_none() && bitrate_kbps > 0 && file_bytes > start as u64 {
        // CBR 估算：净音频字节 ×8 / 码率。
        info.duration_ms = Some((file_bytes - start as u64).saturating_mul(8000) / (bitrate_kbps * 1000));
    }
    Some(info)
}

// ---------------------------------------------------------------------------
// 统一入口与悬浮调度
// ---------------------------------------------------------------------------

/// 解析入口：按魔数分派五容器。无法识别/损坏 → None（悬停显「无法读取」）。
pub fn sniff_and_parse(data: &[u8], file_bytes: u64) -> Option<MediaInfo> {
    if data.len() < 12 {
        return None;
    }
    if &data[4..8] == b"ftyp" {
        return parse_mp4(data, file_bytes);
    }
    if &data[0..4] == b"\x1A\x45\xDF\xA3" {
        // WebM 与 MKV 同 EBML——按 DocType 分族。
        let is_webm = data.windows(5).take(4096).any(|w| w == b"webm\0");
        return parse_mkv(data, is_webm);
    }
    if &data[0..4] == b"fLaC" {
        return parse_flac(data);
    }
    if &data[0..3] == b"ID3" || (data[0] == 0xFF && (data[1] & 0xE0) == 0xE0) {
        return parse_mp3(data, file_bytes);
    }
    None
}

/// 媒体信息缓存（哈希键与 F093 缩略库共享纪律；LRU 定容）。
pub struct MediaInfoStore {
    entries: Vec<(u64, MediaInfo, u64)>, // (hash, info, last_use)
    cap: usize,
    clock: u64,
    pub hits: u64,
    pub misses: u64,
    pub unreadable: u64,
}

impl MediaInfoStore {
    pub fn new(cap: usize) -> MediaInfoStore {
        MediaInfoStore { entries: Vec::new(), cap: cap.max(1), clock: 0, hits: 0, misses: 0, unreadable: 0 }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 查缓存。命中 → Some（悬停 <100ms 判线的机制本体）。
    pub fn lookup(&mut self, hash: u64) -> Option<MediaInfo> {
        self.clock += 1;
        let clock = self.clock;
        match self.entries.iter().position(|(h, _, _)| *h == hash) {
            Some(i) => {
                self.entries[i].2 = clock;
                let info = self.entries[i].1.clone();
                self.hits += 1;
                Some(info)
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    pub fn put(&mut self, hash: u64, info: MediaInfo) {
        self.clock += 1;
        let clock = self.clock;
        if let Some(i) = self.entries.iter().position(|(h, _, _)| *h == hash) {
            self.entries[i] = (hash, info, clock);
            return;
        }
        if self.entries.len() >= self.cap {
            let mut oldest = 0usize;
            for i in 1..self.entries.len() {
                if self.entries[i].2 < self.entries[oldest].2 {
                    oldest = i;
                }
            }
            self.entries.remove(oldest);
        }
        self.entries.push((hash, info, clock));
    }

    /// 不可读登记（负缓存——损坏容器不反复试）。
    pub fn mark_unreadable(&mut self, hash: u64) {
        self.unreadable += 1;
        self.put(hash, MediaInfo::default());
    }
}

/// 悬浮调度状态机：Idle → Hovering(800ms) → Shown → Idle；
/// 缓存命中走 <100ms 快路径（hover 即显）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverState {
    Idle,
    /// 悬停计时中（起始 ms、缓存是否命中）。
    Hovering(u64, bool),
    Shown(u64),
}

pub struct HoverScheduler {
    pub state: HoverState,
    pub show_count: u64,
}

impl HoverScheduler {
    pub fn new() -> HoverScheduler {
        HoverScheduler { state: HoverState::Idle, show_count: 0 }
    }

    /// 悬停进入。`cache_hit` = MediaInfoStore.lookup 命中。
    pub fn hover(&mut self, now_ms: u64, cache_hit: bool) {
        if cache_hit {
            self.state = HoverState::Shown(now_ms);
            self.show_count += 1;
        } else {
            self.state = HoverState::Hovering(now_ms, false);
        }
    }

    /// tick 推进（调用方按帧/定时喂入）。到点出条回 true。
    pub fn tick(&mut self, now_ms: u64) -> bool {
        if let HoverState::Hovering(t0, _) = self.state {
            if now_ms.saturating_sub(t0) >= HOVER_DELAY_MS {
                self.state = HoverState::Shown(now_ms);
                self.show_count += 1;
                return true;
            }
        }
        false
    }

    pub fn leave(&mut self) {
        self.state = HoverState::Idle;
    }
}

impl Default for HoverScheduler {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F094 自检（聚合进 stard 域）。
pub fn run_mediainfo_checks() -> CheckSet {
    let mut set = CheckSet::new("stard-F094");

    // —— MP4 全对样本（合成最小 moov，含 stsz 轨字节实算）——
    let mp4 = build_sample_mp4(0);
    let info = sniff_and_parse(&mp4, 1_000_000).expect("sample mp4 must parse");
    set.add("mp4 container", info.container == Some(Container::Mp4), "");
    set.add("mp4 duration 10s", info.duration_ms == Some(10_000), "");
    set.add("mp4 resolution 1920x1080", info.width == Some(1920) && info.height == Some(1080), "");
    set.add("mp4 video bitrate from stsz", info.video_bps == Some(640_000), "");
    set.add("mp4 total bitrate", info.total_bps == Some(1_000_000 * 8000 / 10_000), "");

    // —— 旋转修正（90° 竖拍：显示 1080×1920）——
    let infor = sniff_and_parse(&build_sample_mp4(90), 1_000_000).expect("rotated mp4 must parse");
    set.add("mp4 rotation corrected portrait", infor.width == Some(1080) && infor.height == Some(1920) && infor.rotation == Some(90), "");

    // —— MKV / WebM ——
    let info = sniff_and_parse(&build_sample_mkv(false), 100).expect("sample mkv must parse");
    set.add("mkv duration 5s dims 1280x720", info.duration_ms == Some(5_000) && info.width == Some(1280) && info.height == Some(720), "");
    let info = sniff_and_parse(&build_sample_mkv(true), 100).expect("sample webm must parse");
    set.add("webm family + audio fields", info.container == Some(Container::WebM) && info.sample_rate == Some(48000) && info.channels == Some(2), "");

    // —— FLAC ——
    let info = sniff_and_parse(&build_sample_flac(44100, 2, 44100 * 30), 100).expect("sample flac must parse");
    set.add("flac 44k1 stereo 30s", info.container == Some(Container::Flac) && info.sample_rate == Some(44100) && info.channels == Some(2) && info.duration_ms == Some(30_000), "");

    // —— MP3 ——
    let info = sniff_and_parse(&build_sample_mp3_xing(128, 44100, 1000), 100).expect("sample mp3 must parse");
    set.add("mp3 xing duration exact", info.container == Some(Container::Mp3) && info.duration_ms == Some(1000 * 1152 * 1000 / 44100), "");
    set.add("mp3 fields", info.sample_rate == Some(44100) && info.channels == Some(2) && info.total_bps == Some(128_000), "");

    // —— 损坏容器：诚实 Unreadable，不 panic 不出弹窗 ——
    let mut garbage = alloc::vec![0u8; 64];
    garbage[4..8].copy_from_slice(b"ftyp");
    set.add("corrupt mp4 unreadable", sniff_and_parse(&garbage, 64).is_none(), "");
    set.add("empty rejected", sniff_and_parse(&[], 0).is_none(), "");
    set.add("truncated mp4 rejected", sniff_and_parse(&mp4[..20], 20).is_none(), "");

    // —— 缓存：<100ms 快路径 + 未命中 800ms + LRU 定容 ——
    let mut store = MediaInfoStore::new(64);
    set.add("cache miss first", store.lookup(42).is_none() && store.misses == 1, "");
    store.put(42, info.clone());
    set.add("cache hit second", store.lookup(42).is_some() && store.hits == 1, "");
    for k in 0..70u64 {
        store.put(1000 + k, MediaInfo::default());
    }
    set.add("cache lru capped", store.len() <= 64, "");

    let mut hs = HoverScheduler::new();
    hs.hover(0, true);
    set.add("cache hit shows within 100ms", hs.state == HoverState::Shown(0) && hs.show_count == 1, "");
    let mut hs2 = HoverScheduler::new();
    hs2.hover(0, false);
    set.add("miss waits until 800ms", !hs2.tick(799) && hs2.tick(800), "");

    // —— 时长格式自动缩位 / 字段省略 ——
    let mut long = MediaInfo { duration_ms: Some(3 * 3600_000 + 7 * 60_000 + 9_000), ..Default::default() };
    set.add("hh:mm:ss format", long.duration_label() == "3:07:09", "");
    long.duration_ms = Some(5 * 60_000 + 3_000);
    set.add("mm:ss shorten", long.duration_label() == "5:03", "");
    long.duration_ms = Some(42_000);
    set.add("seconds shorten", long.duration_label() == "42s", "");
    long.duration_ms = None;
    set.add("missing duration empty label", long.duration_label().is_empty(), "");
    set.add("missing fields omitted", MediaInfo::default().resolution_label().is_empty(), "");

    set
}

// ---------------------------------------------------------------------------
// 样本构造（自检与测试用最小合法容器——字节级精确）
// ---------------------------------------------------------------------------

fn push_box(out: &mut Vec<u8>, ty: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&((body.len() + 8) as u32).to_be_bytes());
    out.extend_from_slice(ty);
    out.extend_from_slice(body);
}

/// EBML 值 vint 编码（data-size：标记位前缀）。
fn push_vint(out: &mut Vec<u8>, n: u64) {
    if n < 127 {
        out.push(0x80 | n as u8);
    } else if n < 16_383 {
        out.push(0x40 | (n >> 8) as u8);
        out.push(n as u8);
    } else if n < 2_097_151 {
        out.push(0x20 | ((n >> 16) as u8));
        out.push((n >> 8) as u8);
        out.push(n as u8);
    } else {
        out.push(0x10 | ((n >> 24) as u8));
        out.push((n >> 16) as u8);
        out.push((n >> 8) as u8);
        out.push(n as u8);
    }
}

/// 合成最小 MP4：ftyp + moov(mvhd + trak(tkhd + mdia(hdlr vide + mdhd + minf(stbl(stsz))))。
/// `rot`：0 或 90（旋转矩阵写入 tkhd）。
fn build_sample_mp4(rot: u16) -> Vec<u8> {
    let mut b = Vec::new();
    // ftyp：size 16 = 头 8 + major 4 + minor 4（长度与字节严格一致）。
    b.extend_from_slice(&[0, 0, 0, 16, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm', 0, 0, 0, 0]);
    let mut moov = Vec::new();
    let mut mvhd = [0u8; 20];
    mvhd[12..16].copy_from_slice(&1000u32.to_be_bytes()); // timescale 1000
    mvhd[16..20].copy_from_slice(&10_000u32.to_be_bytes()); // duration 10s
    push_box(&mut moov, b"mvhd", &mvhd);

    let mut trak = Vec::new();
    // tkhd v0：84 字节体（verflags4+ctime4+mtime4+trackid4+resv4+duration4
    // +resv8+layer2+alt2+vol2+resv2 = 40，matrix 36 = 40..76，w/h 76..84）。
    let mut tkhd = [0u8; 84];
    let m = 40usize;
    let w32 = |v: i32| (v as u32).to_be_bytes();
    match rot {
        90 => {
            tkhd[m..m + 4].copy_from_slice(&w32(0));
            tkhd[m + 4..m + 8].copy_from_slice(&w32(65536));
            tkhd[m + 8..m + 12].copy_from_slice(&w32(-65536));
            tkhd[m + 12..m + 16].copy_from_slice(&w32(0));
        }
        _ => {
            tkhd[m..m + 4].copy_from_slice(&w32(65536));
            tkhd[m + 12..m + 16].copy_from_slice(&w32(65536));
        }
    }
    tkhd[76..80].copy_from_slice(&(1920u32 << 16).to_be_bytes());
    tkhd[80..84].copy_from_slice(&(1080u32 << 16).to_be_bytes());
    push_box(&mut trak, b"tkhd", &tkhd);

    let mut mdia = Vec::new();
    let mut hdlr = [0u8; 12];
    hdlr[8..12].copy_from_slice(b"vide");
    push_box(&mut mdia, b"hdlr", &hdlr);
    let mut mdhd = [0u8; 24];
    mdhd[12..16].copy_from_slice(&1000u32.to_be_bytes()); // 轨 timescale
    mdhd[16..20].copy_from_slice(&10_000u32.to_be_bytes()); // 轨 duration
    push_box(&mut mdia, b"mdhd", &mdhd);
    // minf→stbl→stsz 三层装箱：10 帧 × 80_000 字节 = 800_000 轨字节
    // → video_bps 640kbps。stsz 体 = verflags + sample_size + count + 表。
    let mut stsz = Vec::new();
    stsz.extend_from_slice(&0u32.to_be_bytes()); // version/flags
    stsz.extend_from_slice(&0u32.to_be_bytes()); // sample_size = 0（变长表）
    stsz.extend_from_slice(&10u32.to_be_bytes()); // sample_count
    for _ in 0..10 {
        stsz.extend_from_slice(&80_000u32.to_be_bytes());
    }
    let mut stsz_box = Vec::new();
    push_box(&mut stsz_box, b"stsz", &stsz);
    let mut stbl_box = Vec::new();
    push_box(&mut stbl_box, b"stbl", &stsz_box);
    push_box(&mut mdia, b"minf", &stbl_box);

    push_box(&mut trak, b"mdia", &mdia);
    push_box(&mut moov, b"trak", &trak);
    push_box(&mut b, b"moov", &moov);
    b
}

fn push_ebml(out: &mut Vec<u8>, id: u32, body: &[u8]) {
    // id 编码（本项目 id ≤4 字节；id 保留全部位含长度标记）。
    let idb = id.to_be_bytes();
    let start = if id <= 0xFF { 3 } else if id <= 0xFFFF { 2 } else if id <= 0xFF_FFFF { 1 } else { 0 };
    out.extend_from_slice(&idb[start..]);
    push_vint(out, body.len() as u64);
    out.extend_from_slice(body);
}

/// 合成最小 MKV/WebM：EBML 头(DocType) + Segment(Info + Tracks)。
fn build_sample_mkv(webm: bool) -> Vec<u8> {
    let mut b = Vec::new();
    let doctype: &[u8] = if webm { b"webm\0" } else { b"matroska" };
    let mut hdr = Vec::new();
    push_ebml(&mut hdr, 0x0000_0486, doctype); // DocType
    push_ebml(&mut b, 0x1A45_DFA3, &hdr);

    let mut info = Vec::new();
    // TimecodeScale = 1_000_000（正确的值 vint：4 字节标记 0x20）。
    let mut ts = Vec::new();
    push_vint(&mut ts, 1_000_000);
    push_ebml(&mut info, EBML_TIMECODESCALE, &ts);
    let dur_bits = (5000.0f64).to_bits(); // Duration 5s（8 字节 float）
    push_ebml(&mut info, EBML_DURATION, &dur_bits.to_be_bytes());

    let mut tracks = Vec::new();
    let mut te = Vec::new();
    let mut tt = Vec::new();
    push_vint(&mut tt, 1); // video
    push_ebml(&mut te, EBML_TRACKTYPE, &tt);
    let mut vid = Vec::new();
    let mut vw = Vec::new();
    push_vint(&mut vw, 1280);
    push_ebml(&mut vid, EBML_PIXELW, &vw);
    let mut vh = Vec::new();
    push_vint(&mut vh, 720);
    push_ebml(&mut vid, EBML_PIXELH, &vh);
    push_ebml(&mut te, EBML_VIDEO, &vid);
    push_ebml(&mut tracks, EBML_TRACKENTRY, &te);

    let mut te2 = Vec::new();
    let mut tt2 = Vec::new();
    push_vint(&mut tt2, 2); // audio
    push_ebml(&mut te2, EBML_TRACKTYPE, &tt2);
    let mut aud = Vec::new();
    push_ebml(&mut aud, EBML_SAMPLINGFREQ, &(48000.0f32).to_bits().to_be_bytes());
    let mut ch = Vec::new();
    push_vint(&mut ch, 2);
    push_ebml(&mut aud, EBML_CHANNELS, &ch);
    push_ebml(&mut te2, EBML_AUDIO, &aud);
    push_ebml(&mut tracks, EBML_TRACKENTRY, &te2);

    let mut seg = Vec::new();
    push_ebml(&mut seg, EBML_INFO, &info);
    push_ebml(&mut seg, EBML_TRACKS, &tracks);
    push_ebml(&mut b, EBML_SEGMENT, &seg);
    b
}

/// 合成最小 FLAC：fLaC + STREAMINFO。
fn build_sample_flac(rate: u32, ch: u8, total: u64) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"fLaC");
    b.push(0x00); // 非最后块 + 类型 0
    b.extend_from_slice(&[0x00, 0x00, 0x22]); // 3B 长度 = 34
    let mut si = [0u8; 34];
    let sr = rate as u64;
    si[10] = (sr >> 12) as u8;
    si[11] = (sr >> 4) as u8;
    si[12] = (((sr & 0x0F) as u8) << 4) | (((ch - 1) as u8) << 1) | 0x01; // 16bit 位深
    si[13] = ((total >> 32) & 0x0F) as u8;
    si[14] = (total >> 24) as u8;
    si[15] = (total >> 16) as u8;
    si[16] = (total >> 8) as u8;
    si[17] = total as u8;
    b.extend_from_slice(&si);
    b
}

/// 合成 MP3（Xing 头）：MPEG1 L3 立体声帧头 + 32 字节边信息 + Xing(frames)。
fn build_sample_mp3_xing(kbps: u64, rate: u64, frames: u64) -> Vec<u8> {
    let mut b = Vec::new();
    let bitrate_idx: u8 = match kbps {
        64 => 5,
        128 => 9,
        _ => 9,
    };
    let rate_idx: u8 = match rate {
        48000 => 1,
        44100 => 0,
        _ => 0,
    };
    b.extend_from_slice(&[0xFF, 0xFB, (bitrate_idx << 4) | (rate_idx << 2), 0x04]); // 立体声（mode=00）
    b.extend_from_slice(&[0u8; 32]); // MPEG1 立体声边信息（Xing 前置）。
    b.extend_from_slice(b"Xing");
    b.extend_from_slice(&0x01u32.to_be_bytes()); // flags: frames
    b.extend_from_slice(&(frames as u32).to_be_bytes());
    b.extend_from_slice(&[0u8; 32]);
    b
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mp4_sample_full_parse() {
        let info = parse_mp4(&build_sample_mp4(0), 1_000_000).expect("parse");
        assert_eq!(info.container, Some(Container::Mp4));
        assert_eq!(info.duration_ms, Some(10_000));
        assert_eq!(info.width, Some(1920));
        assert_eq!(info.height, Some(1080));
        assert_eq!(info.rotation, Some(0));
        assert_eq!(info.video_bps, Some(640_000), "stsz 轨字节实算 800KB/10s");
        assert_eq!(info.total_bps, Some(800_000), "文件 1MB/10s");
        assert_eq!(info.duration_label(), "10s");
    }

    #[test]
    fn mp4_rotation_90_displays_portrait() {
        let info = parse_mp4(&build_sample_mp4(90), 1_000).expect("parse");
        assert_eq!(info.rotation, Some(90));
        assert_eq!(info.width, Some(1080), "竖拍显示宽高互换");
        assert_eq!(info.height, Some(1920));
    }

    #[test]
    fn mkv_and_webm_parse() {
        let mkv = parse_mkv(&build_sample_mkv(false), false).expect("mkv");
        assert_eq!(mkv.duration_ms, Some(5_000));
        assert_eq!(mkv.width, Some(1280));
        assert_eq!(mkv.height, Some(720));
        let webm = parse_mkv(&build_sample_mkv(true), true).expect("webm");
        assert_eq!(webm.container, Some(Container::WebM));
        assert_eq!(webm.sample_rate, Some(48000));
        assert_eq!(webm.channels, Some(2));
    }

    #[test]
    fn flac_parse() {
        let info = parse_flac(&build_sample_flac(44100, 2, 44100 * 30)).expect("flac");
        assert_eq!(info.sample_rate, Some(44100));
        assert_eq!(info.channels, Some(2));
        assert_eq!(info.duration_ms, Some(30_000));
    }

    #[test]
    fn mp3_xing_and_cbr() {
        let xing = parse_mp3(&build_sample_mp3_xing(128, 44100, 1000), 100).expect("mp3");
        assert_eq!(xing.duration_ms, Some(1000 * 1152 * 1000 / 44100));
        assert_eq!(xing.total_bps, Some(128_000));
        assert_eq!(xing.channels, Some(2));
        // Xing 标记破坏 → CBR 估算路径（有值且容器正确）。
        let mut raw = build_sample_mp3_xing(128, 44100, 1000);
        let off = raw.windows(4).position(|w| w == b"Xing").unwrap();
        raw[off..off + 4].copy_from_slice(b"Junk");
        let cbr = parse_mp3(&raw, 1_000_000).expect("cbr");
        assert!(cbr.duration_ms.is_some());
    }

    #[test]
    fn corrupt_and_malformed_never_panic() {
        // 对抗样本集：任意截断/垃圾零 panic 全拒绝。
        let cases: Vec<Vec<u8>> = vec![
            Vec::new(),
            vec![0x1A],
            vec![0x1A, 0x45, 0xDF],
            vec![0x1A, 0x45, 0xDF, 0xA3, 0x00],
            vec![0xFF, 0xFB, 0x90],
            b"fLaC".to_vec(),
            b"fLaCxx".to_vec(),
        ];
        for c in &cases {
            let _ = sniff_and_parse(c, c.len() as u64);
        }
        let _ = parse_mp4(&[0u8; 8], 8);
        let _ = parse_mkv(&[0xFF; 32], false);
    }

    #[test]
    fn hover_scheduler_state_machine() {
        let mut hs = HoverScheduler::new();
        assert_eq!(hs.state, HoverState::Idle);
        hs.hover(1000, false);
        assert_eq!(hs.state, HoverState::Hovering(1000, false));
        assert!(!hs.tick(1500), "未到 800ms 不显示");
        assert!(hs.tick(1800), "到 800ms 显示");
        assert_eq!(hs.show_count, 1);
        hs.leave();
        assert_eq!(hs.state, HoverState::Idle);
        // 缓存命中快路径：hover 即显（<100ms 判线）。
        hs.hover(5000, true);
        assert_eq!(hs.state, HoverState::Shown(5000));
    }

    #[test]
    fn store_lru_and_negative_cache() {
        let mut s = MediaInfoStore::new(3);
        for k in 0..4u64 {
            s.put(k, MediaInfo { duration_ms: Some(k), ..Default::default() });
        }
        assert_eq!(s.len(), 3);
        assert!(s.lookup(0).is_none(), "最旧的 0 被 LRU 逐出");
        assert!(s.lookup(3).is_some());
        s.mark_unreadable(77);
        assert!(s.lookup(77).is_some(), "负缓存登记后不再反复试");
    }

    #[test]
    fn duration_label_shortening() {
        let mut m = MediaInfo::default();
        m.duration_ms = Some(7225_000); // 2:00:25
        assert_eq!(m.duration_label(), "2:00:25");
        m.duration_ms = Some(61_000);
        assert_eq!(m.duration_label(), "1:01");
        m.duration_ms = Some(800);
        assert_eq!(m.duration_label(), "0s");
    }
}
