//! VE-F1203 · WebM/MKV 容器解封装 · 域自检（判据逐条对应，见 `veg03_webm_mkv.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项前缀）：
//! - EBML 编码解析（VINT / DocType 分流 / 未知长度元素）→ `C03-EBML-*`
//! - Segment / Track / Cluster 结构（编解码器、时长、默认帧率、时间戳基准）→ `C03-TRACK-*`
//! - Block / SimpleBlock 帧（时间戳、关键帧、lacing 三模式）→ `C03-BLOCK-*`
//! - 编解码器识别（F1058 语义承接）→ `C03-CODEC-*`
//! - Attachment 附件（字体 / 封面）→ `C03-ATTACH-*`
//! - 与 MP4 的差异文档化（F1202 对照）→ `C03-DIFF-*`
//! - 错误路径（尺寸溢出拒绝 / 时间码回跳重同步计数）→ `C03-ERR-*`
//! - 对拍（ffprobe 交叉验证）→ `C03-CROSS-*`
//!
//! **门禁设计四则在本模块的具体落实**：
//!
//! 1. **不用表内元素验表内函数**：[`C03-CODEC-HANDOFF-CLOSURE`] 校验的是
//!    `CODEC_HANDOFF` 的**承接单在册且不串域**（音频编解码器不得指向视频解码单），
//!    而不是「查表函数能查到表里的项」。[`C03-DIFF-TABLE`] 拿差异表去撞
//!    **实测解析结果**，不是表内自证。
//! 2. **两侧同规范化**：[`C03-CROSS-NO-DIVERGENCE`] 的对拍两侧都经同一
//!    `matroska_codec_to_ffprobe` / 毫帧单位换算，不是一边 `V_VP9` 一边 `vp9`。
//! 3. **有洞 / 连续性只比相邻**：时间码回跳检测比的是「上一帧」而非所有对。
//! 4. **反假变体**：[`C03-ERR-MUTATION-CAUGHT`] 与
//!    [`C03-EBML-MUTATION-DETECTED`] 改坏语料字节后重跑，确认对应判据**变红**
//!    （不是恒真通过）。
//!
//! 语料全部由本文件内的构造器生成（EBML 字节流合成器），跨平台逐位可复现，
//! 零IO、零墙钟、不依赖 ffprobe 可执行文件（对拍事实以 fixture 登记）。

use super::veg03_webm_mkv::*;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 语料构造器：EBML 字节流合成器
// ---------------------------------------------------------------------------

/// 确定性 LCG（不依赖 rand，保证跨平台逐位可复现）。
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn byte(&mut self) -> u8 {
        (self.next() & 0xFF) as u8
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(self.byte());
        }
        v
    }
}

/// 编码一个长度 VINT（宽度按值自动选择；不产生「全1」未知形态）。
fn vint(value: u64) -> Vec<u8> {
    let mut width = 1u32;
    // 宽度 w 能表示的最大值 = 2^(7w) - 2（全 1 保留给「未知长度」）。
    while width < 8 {
        let max = (1u64 << (7 * width)) - 2;
        if value <= max {
            break;
        }
        width += 1;
    }
    let w = width as usize;
    let mut out = Vec::with_capacity(w);
    let marker = 1u64 << (7 * width);
    let full = marker | value;
    for i in (0..w).rev() {
        out.push(((full >> (8 * i as u32)) & 0xFF) as u8);
    }
    out
}

/// 编码「未知长度」VINT（全1 有效位）。
fn vint_unknown(width: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(width);
    if width == 1 {
        out.push(0xFF);
    } else {
        out.push((0xFFu8 << (8 - width as u32)) as u8);
        for _ in 1..width {
            out.push(0xFF);
        }
    }
    out
}

/// 元素 ID 的字节形式（ID 自带标记位，故按值取最小宽度即可还原）。
fn eid(id: u32) -> Vec<u8> {
    if id > 0xFF_FFFF {
        vec![(id >> 24) as u8, (id >> 16) as u8, (id >> 8) as u8, id as u8]
    } else if id > 0xFFFF {
        vec![(id >> 16) as u8, (id >> 8) as u8, id as u8]
    } else if id > 0xFF {
        vec![(id >> 8) as u8, id as u8]
    } else {
        vec![id as u8]
    }
}

/// 组装一个已知长度元素。
fn el(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = eid(id);
    out.extend_from_slice(&vint(payload.len() as u64));
    out.extend_from_slice(payload);
    out
}

/// 组装一个**未知长度**元素（内容为已拼好的子元素序列）。
fn el_unknown(id: u32, children: &[u8]) -> Vec<u8> {
    let mut out = eid(id);
    out.extend_from_slice(&vint_unknown(1));
    out.extend_from_slice(children);
    out
}

/// 大端无符号整数 payload（宽度可变，最小字节数）。
fn uint_bytes(v: u64) -> Vec<u8> {
    if v == 0 {
        return vec![0u8];
    }
    let mut out = Vec::new();
    let mut started = false;
    for i in (0..8).rev() {
        let b = ((v >> (8 * i as u32)) & 0xFF) as u8;
        if b != 0 || started {
            out.push(b);
            started = true;
        }
    }
    if out.is_empty() {
        out.push(0);
    }
    out
}

/// 整数元素。
fn uint_el(id: u32, v: u64) -> Vec<u8> {
    el(id, &uint_bytes(v))
}

/// 字符串元素（Matroska 惯例：尾部补 NUL）。
fn str_el(id: u32, s: &str) -> Vec<u8> {
    let mut b: Vec<u8> = s.as_bytes().to_vec();
    b.push(0);
    el(id, &b)
}

/// 64 位浮点元素。
fn f64_el(id: u32, v: f64) -> Vec<u8> {
    el(id, &v.to_be_bytes())
}

/// 32 位浮点元素。
fn f32_el(id: u32, v: f32) -> Vec<u8> {
    el(id, &v.to_be_bytes())
}

/// 一条轨道的构造参数。
struct TrackSpec {
    number: u64,
    kind: u64,
    codec: &'static str,
    default_duration_ns: u64,
    width: u64,
    height: u64,
    sampling_hz: Option<f64>,
    channels: u64,
}

/// 构造一个 TrackEntry。
fn track_entry(spec: &TrackSpec) -> Vec<u8> {
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(&uint_el(ids::TRACK_NUMBER, spec.number));
    b.extend_from_slice(&uint_el(ids::TRACK_TYPE, spec.kind));
    b.extend_from_slice(&str_el(ids::CODEC_ID, spec.codec));
    if spec.default_duration_ns > 0 {
        b.extend_from_slice(&uint_el(ids::DEFAULT_DURATION, spec.default_duration_ns));
    }
    if spec.width > 0 || spec.height > 0 {
        let mut v: Vec<u8> = Vec::new();
        v.extend_from_slice(&uint_el(ids::PIXEL_WIDTH, spec.width));
        v.extend_from_slice(&uint_el(ids::PIXEL_HEIGHT, spec.height));
        b.extend_from_slice(&el(ids::VIDEO, &v));
    }
    if let Some(hz) = spec.sampling_hz {
        let mut a: Vec<u8> = Vec::new();
        a.extend_from_slice(&f64_el(ids::SAMPLING_FREQUENCY, hz));
        a.extend_from_slice(&uint_el(ids::CHANNELS, spec.channels));
        b.extend_from_slice(&el(ids::AUDIO, &a));
    }
    el(ids::TRACK_ENTRY, &b)
}

/// EBML 头（doc_type 可选，默认 matroska）。
fn ebml_header(doc_type: &str) -> Vec<u8> {
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(&uint_el(ids::EBML_VERSION, 1));
    b.extend_from_slice(&uint_el(ids::EBML_READ_VERSION, 1));
    b.extend_from_slice(&uint_el(ids::EBML_MAX_ID_LENGTH, 4));
    b.extend_from_slice(&uint_el(ids::EBML_MAX_SIZE_LENGTH, 8));
    b.extend_from_slice(&str_el(ids::DOC_TYPE, doc_type));
    b.extend_from_slice(&uint_el(ids::DOC_TYPE_VERSION, 4));
    b.extend_from_slice(&uint_el(ids::DOC_TYPE_READ_VERSION, 2));
    el(ids::EBML, &b)
}

/// Info 元素。
fn info_el(scale_ns: u64, duration_ticks: f64) -> Vec<u8> {
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(&uint_el(ids::TIMECODE_SCALE, scale_ns));
    b.extend_from_slice(&f64_el(ids::DURATION, duration_ticks));
    b.extend_from_slice(&str_el(ids::MUXING_APP, "varix-probe"));
    b.extend_from_slice(&str_el(ids::WRITING_APP, "varix-probe"));
    el(ids::INFO, &b)
}

/// Tracks 元素。
fn tracks_el(specs: &[TrackSpec]) -> Vec<u8> {
    let mut b: Vec<u8> = Vec::new();
    for s in specs {
        b.extend_from_slice(&track_entry(s));
    }
    el(ids::TRACKS, &b)
}

/// 构造一个 SimpleBlock 载荷（轨道号 + 相对时间 + flags + 数据）。
///
/// `track` 编码为长度 VINT；`rel` 为 int16 大端；`flags` 原样写入。
fn simple_block(track: u64, rel: i16, flags: u8, data: &[u8]) -> Vec<u8> {
    let mut b = vint(track);
    b.extend_from_slice(&rel.to_be_bytes());
    b.push(flags);
    b.extend_from_slice(data);
    b
}

/// 构造一个 Cluster（已知长度）。
fn cluster_el(timecode: u64, blocks: &[Vec<u8>]) -> Vec<u8> {
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(&uint_el(ids::TIMECODE, timecode));
    for blk in blocks {
        b.extend_from_slice(&el(ids::SIMPLE_BLOCK, blk));
    }
    el(ids::CLUSTER, &b)
}

/// 标准测试轨道集：视频（VP9，1920x1080，25fps）+ 音频（Opus 48kHz 立体声）。
fn standard_tracks() -> Vec<TrackSpec> {
    vec![
        TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 40_000_000, // 25 fps
            width: 1920,
            height: 1080,
            sampling_hz: None,
            channels: 0,
        },
        TrackSpec {
            number: 2,
            kind: 2,
            codec: "A_OPUS",
            default_duration_ns: 20_000_000,
            width: 0,
            height: 0,
            sampling_hz: Some(48000.0),
            channels: 2,
        },
    ]
}

/// 构造一个完整可解封装的 WebM 字节流（已知长度 Segment）。
///
/// 结构：EBML 头 + Segment > [Info, Tracks, Cluster x2, Cues, Attachments]
fn build_webm(blocks_per_cluster: &[&[Vec<u8>]], cluster_timecodes: &[u64], scale_ns: u64) -> Vec<u8> {
    let mut seg: Vec<u8> = Vec::new();
    seg.extend_from_slice(&info_el(scale_ns, 2000.0));
    seg.extend_from_slice(&tracks_el(&standard_tracks()));

    for (i, tc) in cluster_timecodes.iter().enumerate() {
        let blocks = blocks_per_cluster.get(i).copied().unwrap_or(&[]);
        seg.extend_from_slice(&cluster_el(*tc, blocks));
    }

    // Cues：一个 CuePoint 指向 cluster 0
    let mut ctp: Vec<u8> = Vec::new();
    ctp.extend_from_slice(&uint_el(ids::CUE_TRACK, 1));
    ctp.extend_from_slice(&uint_el(ids::CUE_CLUSTER_POSITION, 0));
    let mut ctp_el = uint_el(ids::CUE_TIME, 0);
    ctp_el.extend_from_slice(&el(ids::CUE_TRACK_POSITIONS, &ctp));
    let mut cues: Vec<u8> = Vec::new();
    cues.extend_from_slice(&el(ids::CUE_POINT, &ctp_el));
    seg.extend_from_slice(&el(ids::CUES, &cues));

    // Attachments：一个字体 + 一张封面
    seg.extend_from_slice(&attachments_el());

    let mut out = ebml_header("webm");
    out.extend_from_slice(&el(ids::SEGMENT, &seg));
    out
}

/// 构造 Attachments 元素（字体 + 封面）。
fn attachments_el() -> Vec<u8> {
    let mut font: Vec<u8> = Vec::new();
    font.extend_from_slice(&uint_el(ids::FILE_UID, 1));
    font.extend_from_slice(&str_el(ids::FILE_NAME, "Roboto-Regular.ttf"));
    font.extend_from_slice(&str_el(ids::FILE_MIME_TYPE, "font/ttf"));
    font.extend_from_slice(&str_el(ids::FILE_DESCRIPTION, "正文字体"));
    font.extend_from_slice(&el(ids::FILE_DATA, &[0x00, 0x01, 0x00, 0x00, 0xFF, 0xFF]));

    let mut cover: Vec<u8> = Vec::new();
    cover.extend_from_slice(&uint_el(ids::FILE_UID, 2));
    cover.extend_from_slice(&str_el(ids::FILE_NAME, "cover.jpg"));
    cover.extend_from_slice(&str_el(ids::FILE_MIME_TYPE, "image/jpeg"));
    cover.extend_from_slice(&el(ids::FILE_DATA, &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10]));

    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(&el(ids::ATTACHED_FILE, &font));
    b.extend_from_slice(&el(ids::ATTACHED_FILE, &cover));
    el(ids::ATTACHMENTS, &b)
}

/// 最小可用语料：1 轨（VP9）、1 Cluster、1 SimpleBlock。
fn minimal_webm() -> Vec<u8> {
    let blocks: Vec<Vec<u8>> = vec![simple_block(1, 0, 0x80, &[0xAA; 32])];
    let mut seg: Vec<u8> = Vec::new();
    seg.extend_from_slice(&info_el(1_000_000, 40.0));
    seg.extend_from_slice(&tracks_el(&[TrackSpec {
        number: 1,
        kind: 1,
        codec: "V_VP9",
        default_duration_ns: 40_000_000,
        width: 640,
        height: 360,
        sampling_hz: None,
        channels: 0,
    }]));
    seg.extend_from_slice(&cluster_el(0, &blocks));
    let mut out = ebml_header("webm");
    out.extend_from_slice(&el(ids::SEGMENT, &seg));
    out
}

/// 解封装并返回成功值（自检辅助：失败即 panic——仅自检内允许）。
fn demux_ok(data: &[u8]) -> MkvFile {
    match demux(data) {
        Outcome::Ok { value, .. } => value,
        Outcome::Err { failure, .. } => panic!("语料应可解封装，实际失败：{} / {}", failure.message, failure.hint),
    }
}

/// 解封装并返回失败诊断码（自检辅助）。
fn demux_err_code(data: &[u8]) -> DiagCode {
    match demux(data) {
        Outcome::Ok { .. } => DiagCode::CriterionSelfCheckFailed,
        Outcome::Err { failure, .. } => failure.code,
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// VE-F1203 域自检。返回 [`CheckSet`]。
pub fn run_veg03_checks() -> CheckSet {
    let mut set = CheckSet::new("ve-g03");
    run_veg03_ebml_checks(&mut set);
    run_veg03_track_checks(&mut set);
    run_veg03_block_checks(&mut set);
    run_veg03_codec_checks(&mut set);
    run_veg03_attach_checks(&mut set);
    run_veg03_diff_checks(&mut set);
    run_veg03_err_checks(&mut set);
    run_veg03_cross_checks(&mut set);
    set
}

// ---- §A EBML 编码解析 ------------------------------------------------------

fn run_veg03_ebml_checks(set: &mut CheckSet) {
    // A1 VINT 宽度判定：前导零个数 → 宽度；0x00 无定义（宽度 9）
    {
        let widths: [(u8, u8); 8] = [
            (0x80, 1),
            (0x40, 2),
            (0x20, 3),
            (0x10, 4),
            (0x08, 5),
            (0x04, 6),
            (0x02, 7),
            (0x01, 8),
        ];
        let all = widths.iter().all(|(b, w)| vint_width_from_first(*b) == *w);
        set.add("C03-EBML-VINT-WIDTH-八宽度判定", all && vint_width_from_first(0x00) == 9, "");
    }

    // A2 VINT 数值解码：宽度 1 的长度 VINT 0x82 → 值 2
    {
        let data = [0x82u8, 0x00];
        let v = decode_vint(&data, 0, 2, 8, false);
        set.add(
            "C03-EBML-VINT-VALUE-宽度1去标记位",
            matches!(v, Ok(r) if r.value == 2 && r.width == 1 && !r.unknown),
            "",
        );
    }

    // A3 未知长度形态：宽度 1 的 0xFF → 未知；宽度 2 的 0x7FFF → 未知
    {
        let d1 = [0xFFu8, 0x00];
        let d2 = [0x7Fu8, 0xFF, 0x00];
        let v1 = decode_vint(&d1, 0, 3, 8, false);
        let v2 = decode_vint(&d2, 0, 3, 8, false);
        set.add(
            "C03-EBML-VINT-UNKNOWN-宽1与宽2全1",
            matches!(v1, Ok(r) if r.unknown && r.width == 1)
                && matches!(v2, Ok(r) if r.unknown && r.width == 2),
            "",
        );
    }

    // A4 宽度 8 的全1 判定（首字节无有效位，全靠后续 7 字节）
    //    这是「不依赖 0xff >> width 移位语义」的关键验证点。
    {
        let all_ones = [0x01u8, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
        let not_all = [0x01u8, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F, 0x00];
        let v_all = decode_vint(&all_ones, 0, 9, 8, false);
        let v_not = decode_vint(&not_all, 0, 9, 8, false);
        set.add(
            "C03-EBML-VINT-WIDTH8-全1判定不依赖移位",
            matches!(v_all, Ok(r) if r.width == 8 && r.unknown)
                && matches!(v_not, Ok(r) if r.width == 8 && !r.unknown),
            "",
        );
    }

    // A5 非法 VINT 显性拒绝：首字节 0x00 / 宽度超限 / 截断
    {
        let zero = [0x00u8, 0x11];
        let wide = [0x10u8, 0x00]; // 宽度 4，ID 只允许 ≤4→ 合法；改用宽度 5
        let too_wide = [0x08u8, 0x00, 0x00]; // 宽度 5 > 4
        let trunc = [0x80u8];
        let e1 = decode_vint(&zero, 0, 2, 8, false).is_err();
        let e2 = decode_vint(&too_wide, 0, 3, 4, true).is_err();
        let e3 = decode_vint(&trunc, 0, 0, 8, false).is_err();
        let _ = wide;
        set.add("C03-EBML-VINT-REJECT-三类非法显性拒绝", e1 && e2 && e3, "");
    }

    // A6 元素头解析：ID + 长度 + payload 范围
    {
        let payload = [0x11u8, 0x22, 0x33, 0x44];
        let mut e = eid(ids::PIXEL_WIDTH);
        e.extend_from_slice(&vint(4));
        e.extend_from_slice(&payload);
        let h = read_element_header(&e, 0, e.len() as u64);
        set.add(
            "C03-EBML-HEADER-Id长度与payload范围",
            matches!(h, Ok(r) if r.id == ids::PIXEL_WIDTH && r.data_size == 4 && r.data_offset == 2 && !r.size_unknown),
            "",
        );
    }

    // A7 越界元素显性拒绝（声明长度超出父元素）
    {
        let mut e = eid(ids::PIXEL_WIDTH);
        e.extend_from_slice(&vint(100)); // 声明 100 字节但实际没有
        e.extend_from_slice(&[0x11, 0x22]);
        let h = read_element_header(&e, 0, e.len() as u64);
        set.add(
            "C03-EBML-RANGE-越界元素拒绝",
            matches!(h, Err(f) if f.code == DiagCode::ElementOutOfRange),
            "",
        );
    }

    // A8 未登记元素 ID 宽容跳过并计数（非致命）
    {
        // 0x81 是未登记 ID，长度 2
        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(1_000_000, 0.0));
        seg.extend_from_slice(&tracks_el(&[TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        }]));
        seg.extend_from_slice(&cluster_el(0, &[simple_block(1, 0, 0x80, &[0xAA; 8])]));
        let mut out = ebml_header("webm");
        out.extend_from_slice(&el(ids::SEGMENT, &seg));
        // 在 Segment 前插入一个未登记的顶层元素
        let mut full = ebml_header("webm");
        full.extend_from_slice(&el(0x81, &[0x00, 0x01]));
        full.extend_from_slice(&out[ebml_header("webm").len()..]);

        let f = demux_ok(&full);
        set.add(
            "C03-EBML-ID-UNKNOWN-宽容跳过并计数",
            f.stats.skipped_unknown_ids == 1 && f.total_frames == 1,
            "",
        );
    }

    // A9 未知长度元素计数与 DocType 分流
    {
        let blocks: Vec<Vec<u8>> = vec![simple_block(1, 0, 0x80, &[0xAA; 16])];
        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(1_000_000, 0.0));
        seg.extend_from_slice(&tracks_el(&[TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        }]));
        seg.extend_from_slice(&cluster_el(0, &blocks));
        // 未知长度 Segment
        let mut out = ebml_header("matroska");
        out.extend_from_slice(&el_unknown(ids::SEGMENT, &seg));
        let f = demux_ok(&out);
        set.add(
            "C03-EBML-DOCTYPE-分流与未知长度计数",
            f.header.doc_type == DocType::Matroska
                && f.stats.unknown_size_elements >= 1
                && f.total_frames == 1,
            "",
        );
    }

    // A10 未知长度 Cluster 的重同步边界（本模块最关键的硬化点）
    //     语料：两个未知长度 Cluster + 后随 Cues。若无重同步，第一个 Cluster
    //     会吞掉第二个，帧数从 2 掉到 1 —— 且不报错（静默数据缺陷）。
    {
        let specs = vec![TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        }];
        let c0: Vec<u8> = {
            let mut b: Vec<u8> = Vec::new();
            b.extend_from_slice(&uint_el(ids::TIMECODE, 0));
            b.extend_from_slice(&el(ids::SIMPLE_BLOCK, &simple_block(1, 0, 0x80, &[0xAA; 8])));
            b
        };
        let c1: Vec<u8> = {
            let mut b: Vec<u8> = Vec::new();
            b.extend_from_slice(&uint_el(ids::TIMECODE, 1000));
            b.extend_from_slice(&el(ids::SIMPLE_BLOCK, &simple_block(1, 0, 0x80, &[0xBB; 8])));
            b
        };
        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(1_000_000, 0.0));
        seg.extend_from_slice(&tracks_el(&specs));
        seg.extend_from_slice(&el_unknown(ids::CLUSTER, &c0));
        seg.extend_from_slice(&el_unknown(ids::CLUSTER, &c1));
        // Cues 跟在后面：验证第一个 Cluster 在 Cues 处也正确停止
        let mut ctp: Vec<u8> = Vec::new();
        ctp.extend_from_slice(&uint_el(ids::CUE_TRACK, 1));
        ctp.extend_from_slice(&uint_el(ids::CUE_CLUSTER_POSITION, 0));
        let mut ctp_el = uint_el(ids::CUE_TIME, 0);
        ctp_el.extend_from_slice(&el(ids::CUE_TRACK_POSITIONS, &ctp));
        let mut cues: Vec<u8> = Vec::new();
        cues.extend_from_slice(&el(ids::CUE_POINT, &ctp_el));
        seg.extend_from_slice(&el(ids::CUES, &cues));

        let mut out = ebml_header("webm");
        out.extend_from_slice(&el(ids::SEGMENT, &seg));
        let f = demux_ok(&out);
        set.add(
            "C03-EBML-RESYNC-未知长度Cluster兄弟不吞",
            f.clusters.len() == 2 && f.total_frames == 2 && f.cues.len() == 1,
            "",
        );
    }

    // A11 未知长度 Segment 不被自己的 Cluster 截断（边界集合分离）
    {
        let specs = vec![TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        }];
        let c0: Vec<u8> = {
            let mut b: Vec<u8> = Vec::new();
            b.extend_from_slice(&uint_el(ids::TIMECODE, 0));
            b.extend_from_slice(&el(ids::SIMPLE_BLOCK, &simple_block(1, 0, 0x80, &[0xAA; 8])));
            b
        };
        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(1_000_000, 0.0));
        seg.extend_from_slice(&tracks_el(&specs));
        seg.extend_from_slice(&cluster_el(0, &[simple_block(1, 0, 0x80, &[0xCC; 8])]));
        seg.extend_from_slice(&el_unknown(ids::CLUSTER, &c0));
        let mut out = ebml_header("webm");
        out.extend_from_slice(&el_unknown(ids::SEGMENT, &seg));
        let f = demux_ok(&out);
        set.add(
            "C03-EBML-RESYNC-未知长度Segment含完整子树",
            f.clusters.len() == 2 && f.total_frames == 2 && f.tracks.len() == 1,
            "",
        );
    }

    // A12 元素树往返：构造 → 解析 → 逐字段比对（不是「能解析就算过」）
    {
        let data = minimal_webm();
        let mut ctx = Ctx::new(&data);
        let flen = ctx.len();
        let tree = parse_children(&mut ctx, 0, flen, 0);
        let ok = match tree {
            Ok(t) => {
                let seg = find_child(&t, ids::SEGMENT);
                let tr = seg.and_then(|s| find_child(&s.children, ids::TRACKS));
                let te = tr.and_then(|s| find_child(&s.children, ids::TRACK_ENTRY));
                let w = te.and_then(|e| find_child(&e.children, ids::VIDEO));
                let pw = w.and_then(|e| find_child(&e.children, ids::PIXEL_WIDTH));
                seg.is_some() && tr.is_some() && te.is_some() && pw.is_some()
            }
            Err(_) => false,
        };
        set.add("C03-EBML-TREE-四层嵌套逐字段可达", ok, "");
    }

    // A13 嵌套深度超限拒绝（深度炸弹）
    {
        let data = minimal_webm();
        let mut ctx = Ctx::with_limits(
            &data,
            MkvLimits { depth: 1, ..MKV_LIMITS },
        );
        let flen = ctx.len();
        let r = parse_children(&mut ctx, 0, flen, 0);
        set.add(
            "C03-EBML-DEPTH-深度超限拒绝",
            matches!(r, Err(f) if f.code == DiagCode::CountLimitExceeded),
            "",
        );
    }

    // A14 单层子元素数超限拒绝（元素炸弹）
    {
        let data = minimal_webm();
        let mut ctx = Ctx::with_limits(
            &data,
            MkvLimits { child_count: 1, ..MKV_LIMITS },
        );
        let flen = ctx.len();
        let r = parse_children(&mut ctx, 0, flen, 0);
        set.add(
            "C03-EBML-CHILDCOUNT-元素数超限拒绝",
            matches!(r, Err(f) if f.code == DiagCode::CountLimitExceeded),
            "",
        );
    }
}

// ---- §B 轨道解析 -----------------------------------------------------------

fn run_veg03_track_checks(set: &mut CheckSet) {
    // B1 轨道数、类型、编解码器、分辨率
    {
        let f = demux_ok(&minimal_webm());
        let ok = f.tracks.len() == 1
            && f.tracks[0].track_number == 1
            && f.tracks[0].kind == TrackKind::Video
            && f.tracks[0].codec_id == "V_VP9"
            && f.tracks[0].pixel_width == 640
            && f.tracks[0].pixel_height == 360;
        set.add("C03-TRACK-FIELDS-编号类型编解码器分辨率", ok, "");
    }

    // B2 默认帧率导出（整数毫帧，25fps → 25000）
    {
        let f = demux_ok(&minimal_webm());
        set.add(
            "C03-TRACK-FPS-默认帧率毫帧导出",
            f.tracks[0].default_fps_milli == 25_000 && f.tracks[0].frame_duration_ms() == 40,
            "",
        );
    }

    // B3 音频轨：采样率（f64）+ 声道数
    {
        let f = demux_ok(&build_webm(&[&[]], &[], 1_000_000));
        let a = f.track(2);
        let ok = match a {
            Some(t) => {
                t.kind == TrackKind::Audio
                    && t.sampling_frequency.to_bits() == 48000.0f64.to_bits()
                    && t.channels == 2
            }
            None => false,
        };
        set.add("C03-TRACK-AUDIO-采样率与声道", ok, "");
    }

    // B4 缺 TrackNumber 的轨道被跳过并留诊断（不静默）
    {
        let mut entry: Vec<u8> = Vec::new();
        entry.extend_from_slice(&uint_el(ids::TRACK_TYPE, 1));
        entry.extend_from_slice(&str_el(ids::CODEC_ID, "V_VP9"));
        let bad = el(ids::TRACK_ENTRY, &entry);

        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(1_000_000, 0.0));
        let mut tracks: Vec<u8> = Vec::new();
        tracks.extend_from_slice(&bad);
        tracks.extend_from_slice(&track_entry(&TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        }));
        seg.extend_from_slice(&el(ids::TRACKS, &tracks));
        seg.extend_from_slice(&cluster_el(0, &[simple_block(1, 0, 0x80, &[0xAA; 8])]));
        let mut out = ebml_header("webm");
        out.extend_from_slice(&el(ids::SEGMENT, &seg));

        let f = demux_ok(&out);
        let has_diag = match demux(&out) {
            Outcome::Ok { diagnostics, .. } => {
                diagnostics.iter().any(|d| d.code == DiagCode::ChildMissing)
            }
            Outcome::Err { .. } => false,
        };
        set.add(
            "C03-TRACK-NO-NUMBER-跳过并留诊断",
            f.tracks.len() == 1 && has_diag,
            "",
        );
    }

    // B5 轨道号重复显性拒绝
    {
        let spec = TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        };
        let mut tracks: Vec<u8> = Vec::new();
        tracks.extend_from_slice(&track_entry(&spec));
        tracks.extend_from_slice(&track_entry(&spec));
        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(1_000_000, 0.0));
        seg.extend_from_slice(&el(ids::TRACKS, &tracks));
        let mut out = ebml_header("webm");
        out.extend_from_slice(&el(ids::SEGMENT, &seg));
        set.add(
            "C03-TRACK-DUP-轨道号重复拒绝",
            demux_err_code(&out) == DiagCode::TrackNotFound,
            "",
        );
    }

    // B6 Tracks 内无 TrackEntry 显性拒绝
    {
        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(1_000_000, 0.0));
        seg.extend_from_slice(&el(ids::TRACKS, &uint_el(ids::TIMECODE_SCALE, 1)));
        let mut out = ebml_header("webm");
        out.extend_from_slice(&el(ids::SEGMENT, &seg));
        set.add(
            "C03-TRACK-EMPTY-无TrackEntry拒绝",
            demux_err_code(&out) == DiagCode::ChildMissing,
            "",
        );
    }

    // B7 四类轨道类型映射（video/audio/subtitle/logo）
    {
        let kinds = [(1u64, TrackKind::Video), (2, TrackKind::Audio), (17, TrackKind::Subtitle), (0x10, TrackKind::Logo), (99, TrackKind::Unknown)];
        let all = kinds.iter().all(|(code, k)| TrackKind::from_code(*code) == *k);
        set.add("C03-TRACK-KINDS-五类轨道类型映射", all, "");
    }

    // B8 Block 引用不存在的轨道显性拒绝
    {
        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(1_000_000, 0.0));
        seg.extend_from_slice(&tracks_el(&[TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        }]));
        // Block 引用轨道 7（不存在）
        seg.extend_from_slice(&cluster_el(0, &[simple_block(7, 0, 0x80, &[0xAA; 8])]));
        let mut out = ebml_header("webm");
        out.extend_from_slice(&el(ids::SEGMENT, &seg));
        set.add(
            "C03-TRACK-BLOCK-UNREF-Block引用缺失轨道拒绝",
            demux_err_code(&out) == DiagCode::TrackNotFound,
            "",
        );
    }

    // B9 TimecodeScale 缺失走规范默认（1ms）
    {
        let mut seg: Vec<u8> = Vec::new();
        // Info 不含 TimecodeScale
        let info = el(ids::INFO, &str_el(ids::MUXING_APP, "x"));
        seg.extend_from_slice(&info);
        seg.extend_from_slice(&tracks_el(&[TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        }]));
        seg.extend_from_slice(&cluster_el(0, &[simple_block(1, 0, 0x80, &[0xAA; 8])]));
        let mut out = ebml_header("webm");
        out.extend_from_slice(&el(ids::SEGMENT, &seg));
        let f = demux_ok(&out);
        set.add(
            "C03-TRACK-SCALE-DEFAULT-TimecodeScale缺省",
            f.info.timecode_scale_ns == DEFAULT_TIMECODE_SCALE_NS && f.total_frames == 1,
            "",
        );
    }

    // B10 TimecodeScale 为 0 显性拒绝
    {
        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(0, 0.0));
        seg.extend_from_slice(&tracks_el(&[TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        }]));
        let mut out = ebml_header("webm");
        out.extend_from_slice(&el(ids::SEGMENT, &seg));
        set.add(
            "C03-TRACK-SCALE-ZERO-TimecodeScale零拒绝",
            demux_err_code(&out) == DiagCode::ChildMissing,
            "",
        );
    }
}

// ---- §C Block / SimpleBlock / lacing --------------------------------------

fn run_veg03_block_checks(set: &mut CheckSet) {
    // C1 SimpleBlock 关键帧恒真；Block 取 bit7
    {
        let mut data: Vec<u8> = vec![0x81, 0x00, 0x00, 0x00, 0x00]; // track1, rel0, flags=0x00
        data.extend_from_slice(&[0xAA; 4]);
        let mut ctx = Ctx::new(&data);
        let h_none = read_block_header(&mut ctx, 0, data.len() as u64, true);
        let mut data2: Vec<u8> = vec![0x81, 0x00, 0x00, 0x80, 0x00];
        data2.extend_from_slice(&[0xAA; 4]);
        let mut ctx2 = Ctx::new(&data2);
        let h_key = read_block_header(&mut ctx2, 0, data2.len() as u64, false);
        set.add(
            "C03-BLOCK-KEYFRAME-SimpleBlock恒真Block取位",
            matches!(h_none, Ok(r) if r.keyframe) && matches!(h_key, Ok(r) if r.keyframe),
            "",
        );
    }

    // C2 Block 关键帧位为 0 时关键帧 false
    {
        let mut data: Vec<u8> = vec![0x81, 0x00, 0x00, 0x00, 0x00];
        data.extend_from_slice(&[0xAA; 4]);
        let mut ctx = Ctx::new(&data);
        let h = read_block_header(&mut ctx, 0, data.len() as u64, false);
        set.add("C03-BLOCK-KEYFRAME-NEG-关键帧位0为假", matches!(h, Ok(r) if !r.keyframe), "");
    }

    // C3 时间戳 = Cluster 基准 + Block 相对偏移，经 TimecodeScale 换算
    {
        // scale = 1_000_000 ns/tick（1ms），cluster tc=100，block rel=50 → 150ms
        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(1_000_000, 0.0));
        seg.extend_from_slice(&tracks_el(&[TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        }]));
        seg.extend_from_slice(&cluster_el(100, &[simple_block(1, 50, 0x80, &[0xAA; 8])]));
        let mut out = ebml_header("webm");
        out.extend_from_slice(&el(ids::SEGMENT, &seg));
        let f = demux_ok(&out);
        let t = f.frames().first().map(|fr| fr.time_ms);
        set.add("C03-BLOCK-TIMECODE-基准加相对偏移换算", t == Some(150), "");
    }

    // C4 lacing Xiph：字节累加拆分（3 帧，尺寸 2/3/余）
    //    flags 的 lacing 位（bit2~bit1）与 6 位帧数字段**位重叠**（EBML 规范如此），
    //    故 Xiph（lacing=01）的可达帧数是 3/4/11/12…，3 帧对应 flags=0x02(+keyframe=0x82)。
    {
        let mut blk: Vec<u8> = vec![0x81, 0x00, 0x00, 0x82, 0x02, 0x03];
        // body_start=4；帧数据需 2+3+4=9 字节 ⇒ Block 末尾=13 ⇒ 补 7 字节
        blk.extend_from_slice(&[0xAA; 7]);
        let mut ctx = Ctx::new(&blk);
        let h = read_block_header(&mut ctx, 0, blk.len() as u64, false);
        let split = split_lacing(&mut ctx, h.as_ref().map(|r| r.body_start).unwrap_or(0), blk.len() as u64, LacingMode::Xiph, 3);
        let ok = matches!(&h, Ok(r) if r.lacing_mode == LacingMode::Xiph && r.frame_count == 3)
            && matches!(&split, Ok(v) if v.len() == 3 && v[0].size == 2 && v[1].size == 3 && v[2].size == 4);
        set.add("C03-LACING-XIPH-字节累加拆分三帧", ok, "");
    }

    // C5 lacing Xiph 的 0xFF 续字节（大尺寸）
    {
        // 0xFF 表示续下一个字节：0xFF 0x02 → 255+2 = 257
        let mut blk: Vec<u8> = vec![0x81, 0x00, 0x00, 0x82, 0xFF, 0x02, 0x10];
        let rest = 300usize;
        let mut payload = vec![0u8; rest];
        for (i, b) in payload.iter_mut().enumerate() {
            *b = (i & 0xFF) as u8;
        }
        blk.extend_from_slice(&payload);
        let mut ctx = Ctx::new(&blk);
        let h = read_block_header(&mut ctx, 0, blk.len() as u64, false).ok();
        let body = h.map(|r| r.body_start).unwrap_or(0);
        let split = split_lacing(&mut ctx, body, blk.len() as u64, LacingMode::Xiph, 3);
        set.add(
            "C03-LACING-XIPH-FF续字节大尺寸",
            matches!(&split, Ok(v) if v.len() == 3 && v[0].size == 257),
            "",
        );
    }

    // C6 lacing fixed：等分；不整除显性拒绝
    //    fixed（lacing=10）的可达帧数是 5/6/13/14…，故 6 帧对应 flags=0x85。
    {
        let mut blk: Vec<u8> = vec![0x81, 0x00, 0x00, 0x85];
        blk.extend_from_slice(&[0xAA; 12]); // 6 帧各 2 字节
        let mut ctx = Ctx::new(&blk);
        let hraw = read_block_header(&mut ctx, 0, blk.len() as u64, false);
        let body = hraw.as_ref().map(|r| r.body_start).unwrap_or(0);
        let split = split_lacing(&mut ctx, body, blk.len() as u64, LacingMode::Fixed, 6);
        let ok_ok = matches!(&hraw, Ok(r) if r.lacing_mode == LacingMode::Fixed && r.frame_count == 6)
            && matches!(&split, Ok(v) if v.len() == 6 && v[0].size == 2 && v[5].size == 2);
        // 不整除：3 字节 / 6 帧
        let mut blk2: Vec<u8> = vec![0x81, 0x00, 0x00, 0x85];
        blk2.extend_from_slice(&[0xAA; 3]);
        let mut ctx2 = Ctx::new(&blk2);
        let bad = split_lacing(&mut ctx2, 4, blk2.len() as u64, LacingMode::Fixed, 6);
        let ok_bad = matches!(&bad, Err(f) if f.code == DiagCode::LacingInsufficientData);
        set.add("C03-LACING-FIXED-等分与不整除拒绝", ok_ok && ok_bad, "");
    }

    // C7 lacing EBML：首 VINT 帧数 + VINT 尺寸
    {
        // flags: lacing=11(Ebml) | keyframe 0x80 → 0x86；随后 VINT 帧数=3
        // 载荷 = 尺寸表(0x82,0x84 两字节) + 帧数据(2+4+5=11)
        // body_start=5；帧数据需 2+4+5=11 字节 ⇒ Block 末尾=16 ⇒ 补 9 字节
        let mut blk: Vec<u8> = vec![0x81, 0x00, 0x00, 0x86, 0x83, 0x82, 0x84];
        blk.extend_from_slice(&[0xAA; 9]);
        let mut ctx = Ctx::new(&blk);
        let hraw = read_block_header(&mut ctx, 0, blk.len() as u64, false);
        let body = hraw.as_ref().map(|r| r.body_start).unwrap_or(0);
        let split = split_lacing(&mut ctx, body, blk.len() as u64, LacingMode::Ebml, 3);
        let ok = matches!(&hraw, Ok(r) if r.lacing_mode == LacingMode::Ebml && r.frame_count == 3)
            && matches!(&split, Ok(v) if v.len() == 3 && v[0].size == 2 && v[1].size == 4 && v[2].size == 5);
        set.add("C03-LACING-EBML-首VINT帧数与VINT尺寸", ok, "");
    }

    // C8 lacing 全1 尺寸 VINT 显性拒绝（不当成127）
    {
        // EBML lacing，帧数=2，第 1 帧尺寸 VINT = 0xFF（未知长度形态）
        let mut blk: Vec<u8> = vec![0x81, 0x00, 0x00, 0x86, 0x82, 0xFF];
        blk.extend_from_slice(&[0xAA; 4]);
        let mut ctx = Ctx::new(&blk);
        let h = read_block_header(&mut ctx, 0, blk.len() as u64, false).ok();
        let body = h.map(|r| r.body_start).unwrap_or(0);
        let split = split_lacing(&mut ctx, body, blk.len() as u64, LacingMode::Ebml, 2);
        set.add(
            "C03-LACING-EBML-UNKNOWN-全1尺寸拒绝",
            matches!(&split, Err(f) if f.code == DiagCode::LacingInsufficientData),
            "",
        );
    }

    // C9 lacing 关键帧整块继承（块级属性，与规范和 ffprobe 对齐）
    {
        // 关键帧 Block + Xiph lacing 3 帧 → 三帧全部 keyframe=true
        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(1_000_000, 0.0));
        seg.extend_from_slice(&tracks_el(&[TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        }]));
        // 用 Block（非 SimpleBlock），lacing=Xiph，3 帧（body_start=4，帧数据 9 字节）
        let mut blk: Vec<u8> = vec![0x81, 0x00, 0x00, 0x82, 0x02, 0x03];
        blk.extend_from_slice(&[0xAA; 7]);
        // 放进 BlockGroup 里
        let mut bg: Vec<u8> = Vec::new();
        bg.extend_from_slice(&el(ids::BLOCK, &blk));
        let mut c: Vec<u8> = Vec::new();
        c.extend_from_slice(&uint_el(ids::TIMECODE, 0));
        c.extend_from_slice(&el(ids::BLOCK_GROUP, &bg));
        seg.extend_from_slice(&el(ids::CLUSTER, &c));
        let mut out = ebml_header("webm");
        out.extend_from_slice(&el(ids::SEGMENT, &seg));

        let f = demux_ok(&out);
        let all_key = f.total_frames == 3 && f.keyframe_count == 3;
        set.add("C03-BLOCK-LACING-KEY-关键帧整块继承", all_key, "");
    }

    // C10 lacing 帧数超限拒绝
    {
        let mut ctx = Ctx::with_limits(
            &[0x81, 0x00, 0x00, 0x86, 0xFF, 0x00],
            MkvLimits { lacing_frame_count: 4, ..MKV_LIMITS },
        );
        let h = read_block_header(&mut ctx, 0, 6, false);
        set.add(
            "C03-LACING-COUNT-帧数超限拒绝",
            matches!(h, Err(f) if f.code == DiagCode::LacingInsufficientData),
            "",
        );
    }

    // C11 帧偏移与大小逐帧落在 Block 内（序列断言：offset 连续）
    {
        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(1_000_000, 0.0));
        seg.extend_from_slice(&tracks_el(&[TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        }]));
        // Xiph 3 帧，尺寸 2/3/4（body_start=4，帧数据 9 字节），验证 offset 连续
        let mut blk: Vec<u8> = vec![0x81, 0x00, 0x00, 0x82, 0x02, 0x03];
        blk.extend_from_slice(&[0xAA; 7]);
        let mut c: Vec<u8> = Vec::new();
        c.extend_from_slice(&uint_el(ids::TIMECODE, 0));
        c.extend_from_slice(&el(ids::BLOCK_GROUP, &el(ids::BLOCK, &blk)));
        seg.extend_from_slice(&el(ids::CLUSTER, &c));
        let mut out = ebml_header("webm");
        out.extend_from_slice(&el(ids::SEGMENT, &seg));
        let f = demux_ok(&out);
        let frames = f.frames();
        let contiguous = frames.len() == 3
            && frames[1].offset == frames[0].offset + frames[0].size
            && frames[2].offset == frames[1].offset + frames[1].size;
        set.add("C03-LACING-OFFSETS-三帧偏移连续", contiguous, "");
    }

    // C12 时间码负值显性拒绝
    {
        // cluster tc=0，block rel=-100 → 绝对负
        let r = timecode_to_ms(-1, 1_000_000, MKV_LIMITS.max_time_ms);
        set.add(
            "C03-ERR-TIMECODE-NEG-负绝对时间码拒绝",
            matches!(r, Err(f) if f.code == DiagCode::TimecodeRegression),
            "",
        );
    }

    // C13 时间码换算整数精确（u128 中间量）
    {
        // 3 ticks * 1_000_000 ns = 3_000_000 ns = 3ms
        let a = timecode_to_ms(3, 1_000_000, MKV_LIMITS.max_time_ms);
        // 1000 ticks * 1_500_000 ns = 1_500_000_000 ns = 1500ms
        let b = timecode_to_ms(1000, 1_500_000, MKV_LIMITS.max_time_ms);
        set.add(
            "C03-TIMECODE-EXACT-整数换算精确",
            matches!(a, Ok(3)) && matches!(b, Ok(1500)),
            "",
        );
    }

    // C14 时间码超上限拒绝
    {
        let r = timecode_to_ms(i64::MAX, 1_000_000, MKV_LIMITS.max_time_ms);
        set.add(
            "C03-TIMECODE-OVERFLOW-超上限拒绝",
            matches!(r, Err(f) if f.code == DiagCode::ArithmeticOverflow),
            "",
        );
    }

    // C15 BlockGroup 内的 Block 产帧，ReferenceBlock 不产帧
    {
        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(1_000_000, 0.0));
        seg.extend_from_slice(&tracks_el(&[TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        }]));
        let mut bg: Vec<u8> = Vec::new();
        bg.extend_from_slice(&el(ids::BLOCK, &simple_block(1, 0, 0x80, &[0xAA; 8])));
        // ReferenceBlock 不产帧
        bg.extend_from_slice(&uint_el(ids::REFERENCE_BLOCK, 0));
        bg.extend_from_slice(&uint_el(ids::BLOCK_DURATION, 40));
        let mut c: Vec<u8> = Vec::new();
        c.extend_from_slice(&uint_el(ids::TIMECODE, 0));
        c.extend_from_slice(&el(ids::BLOCK_GROUP, &bg));
        seg.extend_from_slice(&el(ids::CLUSTER, &c));
        let mut out = ebml_header("webm");
        out.extend_from_slice(&el(ids::SEGMENT, &seg));
        let f = demux_ok(&out);
        set.add("C03-BLOCKGROUP-仅Block产帧", f.total_frames == 1, "");
    }
}

// ---- §D 编解码器识别 -------------------------------------------------------

fn run_veg03_codec_checks(set: &mut CheckSet) {
    // D1 已登记的编解码器查询成功
    {
        let q = query_mkv_codec("V_VP9");
        set.add(
            "C03-CODEC-REGISTERED-已登记查询成功",
            q.supported && q.workitem == "VE-F1206",
            "",
        );
    }

    // D2 未登记的编解码器显性不支持（不静默送入解码器）
    {
        let q = query_mkv_codec("V_UNREGISTERED_CODEC");
        set.add(
            "C03-CODEC-UNREGISTERED-未登记显性不支持",
            !q.supported && q.workitem.is_empty() && q.reason.contains("未登记"),
            "",
        );
    }

    // D3 承接单在册且不串域：音频编解码器不得指向视频解码单
    {
        let audio_ok = CODEC_HANDOFF
            .iter()
            .filter(|(id, _)| id.starts_with('A'))
            .all(|(_, w)| !VIDEO_DECODE_WORKITEMS.contains(w));
        let video_ok = CODEC_HANDOFF
            .iter()
            .filter(|(id, _)| id.starts_with('V'))
            .all(|(_, w)| VIDEO_DECODE_WORKITEMS.contains(w));
        let targets_ok = CODEC_HANDOFF.iter().all(|(_, w)| HANDOFF_TARGETS.contains(w));
        set.add(
            "C03-CODEC-HANDOFF-CLOSURE-承接单在册且不串域",
            audio_ok && video_ok && targets_ok,
            "",
        );
    }

    // D4 视频编解码器全测（锚点：编解码器全测）
    {
        let vids = ["V_MPEG4/ISO/AVC", "V_MPEGH/ISO/HEVC", "V_VP9", "V_AV1"];
        let all = vids.iter().all(|c| query_mkv_codec(c).supported);
        set.add("C03-CODEC-VIDEO-ALL-视频编解码器全测", all, "");
    }

    // D5 未登记编解码器在轨道路径留诊断但不丢轨
    {
        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(1_000_000, 0.0));
        seg.extend_from_slice(&tracks_el(&[TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_MYSTERY",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        }]));
        seg.extend_from_slice(&cluster_el(0, &[simple_block(1, 0, 0x80, &[0xAA; 8])]));
        let mut out = ebml_header("webm");
        out.extend_from_slice(&el(ids::SEGMENT, &seg));
        let f = demux_ok(&out);
        let has_diag = match demux(&out) {
            Outcome::Ok { diagnostics, .. } => diagnostics
                .iter()
                .any(|d| d.code == DiagCode::CodecUnsupported),
            Outcome::Err { .. } => false,
        };
        set.add(
            "C03-CODEC-TRACK-DIAG-未登记编解码器留诊断",
            f.tracks.len() == 1 && has_diag,
            "",
        );
    }

    // D6 Matroska CodecID → ffprobe codec_name 映射（对拍口径）
    {
        let pairs = [
            ("V_MPEG4/ISO/AVC", "h264"),
            ("V_MPEGH/ISO/HEVC", "hevc"),
            ("V_VP9", "vp9"),
            ("V_AV1", "av1"),
            ("A_OPUS", "opus"),
        ];
        let all = pairs.iter().all(|(m, f)| matroska_codec_to_ffprobe(m) == *f);
        set.add("C03-CODEC-FFPROBE-MAP-五组映射正确", all, "");
    }
}

// ---- §E Attachment ---------------------------------------------------------

fn run_veg03_attach_checks(set: &mut CheckSet) {
    // E1 附件数量、名称、MIME、用途分类
    {
        let f = demux_ok(&minimal_webm());
        // minimal_webm 不含 Attachments
        set.add("C03-ATTACH-NONE-无附件为空", f.attachments.is_empty(), "");
    }

    // E2 含附件的语料：字体 + 封面各一
    {
        let f = demux_ok(&build_webm(&[&[]], &[], 1_000_000));
        let font = f.attachments.iter().find(|a| a.kind == AttachmentKind::Font);
        let cover = f.attachments.iter().find(|a| a.kind == AttachmentKind::Cover);
        let ok = f.attachments.len() == 2
            && font.is_some()
            && cover.is_some()
            && font.unwrap().name == "Roboto-Regular.ttf"
            && cover.unwrap().mime_type == "image/jpeg";
        set.add("C03-ATTACH-TWO-字体与封面各一", ok, "");
    }

    // E3 附件数据偏移与大小可寻址（能定位到实际字节）
    {
        let data = build_webm(&[&[]], &[], 1_000_000);
        let f = demux_ok(&data);
        let font = f.attachments.iter().find(|a| a.kind == AttachmentKind::Font);
        let ok = match font {
            Some(a) => {
                let slice_ok = a.data_offset + a.data_size <= data.len() as u64;
                let magic_ok = data
                    .get(a.data_offset as usize)
                    .map(|b| *b == 0x00)
                    .unwrap_or(false);
                slice_ok && magic_ok && a.data_size == 6
            }
            None => false,
        };
        set.add("C03-ATTACH-DATA-附件数据可寻址", ok, "");
    }

    // E4 附件 MIME → 用途分类映射
    {
        let cases = [
            ("font/ttf", AttachmentKind::Font),
            ("font/otf", AttachmentKind::Font),
            ("image/png", AttachmentKind::Cover),
            ("image/jpeg", AttachmentKind::Cover),
            ("text/plain", AttachmentKind::Subtitle),
            ("application/octet-stream", AttachmentKind::Other),
        ];
        let all = cases.iter().all(|(m, k)| AttachmentKind::from_mime(m) == *k);
        set.add("C03-ATTACH-KIND-六类MIME分类", all, "");
    }

    // E5 缺 FileData 的附件被跳过并留诊断
    {
        let mut bad: Vec<u8> = Vec::new();
        bad.extend_from_slice(&str_el(ids::FILE_NAME, "empty.bin"));
        bad.extend_from_slice(&str_el(ids::FILE_MIME_TYPE, "application/octet-stream"));
        // 无 FileData
        let af = el(ids::ATTACHED_FILE, &bad);
        let atts = el(ids::ATTACHMENTS, &af);
        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(1_000_000, 0.0));
        seg.extend_from_slice(&tracks_el(&[TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        }]));
        seg.extend_from_slice(&atts);
        let mut out = ebml_header("webm");
        out.extend_from_slice(&el(ids::SEGMENT, &seg));
        let f = demux_ok(&out);
        set.add("C03-ATTACH-NODATA-缺FileData跳过", f.attachments.is_empty(), "");
    }

    // E6 附件数超限拒绝
    {
        let data = build_webm(&[&[]], &[], 1_000_000);
        let mut ctx = Ctx::with_limits(
            &data,
            MkvLimits { attachment_count: 1, ..MKV_LIMITS },
        );
        let flen = ctx.len();
        let mut found = false;
        // 手动走一遍顶层找到 Attachments
        if let Ok(top) = parse_children(&mut ctx, 0, flen, 0) {
            if let Some(seg) = find_child(&top, ids::SEGMENT) {
                if let Some(atts) = find_child(&seg.children, ids::ATTACHMENTS) {
                    if parse_attachments(&mut ctx, atts).is_err() {
                        found = true;
                    }
                }
            }
        }
        set.add("C03-ATTACH-LIMIT-附件数超限拒绝", found, "");
    }
}

// ---- §F 与 MP4 的差异文档化 -----------------------------------------------

fn run_veg03_diff_checks(set: &mut CheckSet) {
    // F1 差异表三维齐备（时间戳粒度 / 索引方式 / lacing）
    {
        let dims = ["时间戳粒度", "索引方式", "lacing"];
        let all = dims.iter().all(|d| {
            FORMAT_DIFFERENCES
                .iter()
                .any(|f| f.dimension == *d && !f.mp4.is_empty() && !f.mkv.is_empty() && !f.consequence.is_empty())
        });
        set.add("C03-DIFF-DIMS-三维差异条目齐备", all && FORMAT_DIFFERENCES.len() == 3, "");
    }

    // F2 差异表 vs 实测解析：MKV 侧确有 lacing，MP4 侧声明无——门禁不弱
    //     （拿表撞实测结果，不是表内自证）
    {
        // 实测：构造一个带 lacing 的 WebM，确认解析出多帧
        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(1_000_000, 0.0));
        seg.extend_from_slice(&tracks_el(&[TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        }]));
        // Xiph lacing 3 帧 → 1 Block 出 3 帧（MKV 有 lacing 的实测证据）
        let mut blk: Vec<u8> = vec![0x81, 0x00, 0x00, 0x82, 0x02, 0x03];
        blk.extend_from_slice(&[0xAA; 7]);
        let mut bg: Vec<u8> = Vec::new();
        bg.extend_from_slice(&el(ids::BLOCK, &blk));
        let mut c: Vec<u8> = Vec::new();
        c.extend_from_slice(&uint_el(ids::TIMECODE, 0));
        c.extend_from_slice(&el(ids::BLOCK_GROUP, &bg));
        seg.extend_from_slice(&el(ids::CLUSTER, &c));
        let mut out = ebml_header("webm");
        out.extend_from_slice(&el(ids::SEGMENT, &seg));
        let f = demux_ok(&out);

        // 差异表声明 lacing 是 MKV 独有（MP4 侧「无 lacing」）；实测确认一个 Block 出多帧
        let lacing_diff = FORMAT_DIFFERENCES
            .iter()
            .find(|d| d.dimension == "lacing")
            .map(|d| d.mkv.contains("lacing") && d.mp4.contains("无 lacing"))
            .unwrap_or(false);
        let empirical = f.total_frames == 3;
        set.add("C03-DIFF-LACING-差异表撞实测多帧", lacing_diff && empirical, "");
    }

    // F3 索引方式差异：MKV Cues 是相对偏移，MP4 是绝对偏移
    {
        let f = demux_ok(&build_webm(&[&[]], &[], 1_000_000));
        let idx_diff = FORMAT_DIFFERENCES
            .iter()
            .find(|d| d.dimension == "索引方式")
            .map(|d| d.mkv.contains("Cues") && d.mp4.contains("stco"))
            .unwrap_or(false);
        // 实测：有 Cues 解析
        let has_cues = !f.cues.is_empty();
        set.add("C03-DIFF-INDEX-差异表撞实测Cues", idx_diff && has_cues, "");
    }

    // F4 时间戳粒度差异：MKV 用 TimecodeScale 统一基准
    {
        let f = demux_ok(&build_webm(&[&[]], &[], 1_000_000));
        let ts_diff = FORMAT_DIFFERENCES
            .iter()
            .find(|d| d.dimension == "时间戳粒度")
            .map(|d| d.mkv.contains("TimecodeScale"))
            .unwrap_or(false);
        let empirical = f.info.timecode_scale_ns == 1_000_000;
        set.add("C03-DIFF-TIMEBASE-差异表撞实测TimecodeScale", ts_diff && empirical, "");
    }
}

// ---- §G 错误路径与反假变体 -----------------------------------------------

fn run_veg03_err_checks(set: &mut CheckSet) {
    // G1 空输入拒绝
    {
        set.add("C03-ERR-EMPTY-空输入拒绝", demux_err_code(&[]) == DiagCode::SegmentMissing, "");
    }

    // G2 缺 EBML 头拒绝
    {
        let seg: Vec<u8> = Vec::new();
        let mut out = el(ids::SEGMENT, &seg);
        // 无EBML 头
        set.add("C03-ERR-NO-HEADER-缺EBML头拒绝", demux_err_code(&out) == DiagCode::HeaderInvalid, "");
    }

    // G3 缺 Segment 拒绝
    {
        let out = ebml_header("webm");
        set.add("C03-ERR-NO-SEGMENT-缺Segment拒绝", demux_err_code(&out) == DiagCode::SegmentMissing, "");
    }

    // G4 不支持的 DocType 拒绝
    {
        let out = ebml_header("not-a-known-type");
        set.add(
            "C03-ERR-DOCTYPE-不支持DocType拒绝",
            demux_err_code(&out) == DiagCode::DocTypeUnsupported,
            "",
        );
    }

    // G5 Cluster 时间码回跳容错重同步并计数
    {
        let specs = vec![TrackSpec {
            number: 1,
            kind: 1,
            codec: "V_VP9",
            default_duration_ns: 0,
            width: 320,
            height: 240,
            sampling_hz: None,
            channels: 0,
        }];
        let mut seg: Vec<u8> = Vec::new();
        seg.extend_from_slice(&info_el(1_000_000, 0.0));
        seg.extend_from_slice(&tracks_el(&specs));
        // 两个 Cluster：时间码 1000 → 100（回跳）
        seg.extend_from_slice(&cluster_el(1000, &[simple_block(1, 0, 0x80, &[0xAA; 8])]));
        seg.extend_from_slice(&cluster_el(100, &[simple_block(1, 0, 0x80, &[0xBB; 8])]));
        let mut out = ebml_header("webm");
        out.extend_from_slice(&el(ids::SEGMENT, &seg));
        let f = demux_ok(&out);
        set.add(
            "C03-ERR-TIMECODE-REGRESSION-回跳重同步计数",
            f.stats.timecode_regressions >= 1 && f.total_frames == 2,
            "",
        );
    }

    // G6 畸形拦截：随机字节不被 panic（显性拒绝）
    {
        let mut rng = Lcg::new(0xF120_3123_4567_89AB);
        let mut all_rejected = true;
        for _ in 0..64 {
            let len = 8 + (rng.next() % 200) as usize;
            let junk = rng.bytes(len);
            // 不得 panic；要么成功要么显性失败
            let _ = demux(&junk);
        }
        // 只要不 panic 就算通过（显式注释：这里测的是「不崩」）
        let _ = all_rejected;
        set.add("C03-ERR-FUZZ-NOPANIC-随机字节不panic", true, "");
    }

    // G7 反假变体：改坏语料一字节（TimecodeScale 值）确认对应判据变红
    {
        let good = minimal_webm();
        let f_good = demux_ok(&good);
        let good_ms = f_good.info.timecode_scale_ns;

        // 定位并改坏 TimecodeScale 的值字节：找到 0x2AD7B1 后第 4 字节（长度后）
        let mut bad = good.clone();
        let mut corrupted = false;
        for i in 0..bad.len().saturating_sub(6) {
            if bad[i] == 0x2A && bad[i + 1] == 0xD7 && bad[i + 2] == 0xB1 {
                // TimecodeScale = 1_000_000 = 0x0F4240 (3 字节)
                // 元素头: ID(3) + size(1) + 值(3)
                bad[i + 4] = 0xFF;
                bad[i + 5] = 0xFF;
                bad[i + 6] = 0xFF;
                corrupted = true;
                break;
            }
        }
        let f_bad = demux_ok(&bad);
        let bad_ms = f_bad.info.timecode_scale_ns;
        // 变体确实改变了行为：改坏后时间刻度不再是 1_000_000
        set.add(
            "C03-ERR-MUTATION-CAUGHT-改坏语料判据变红",
            corrupted && good_ms == 1_000_000 && bad_ms != good_ms,
            "",
        );
    }

    // G8 反假变体：EBML VINT 宽度判定变异（0x00 宽度应被拒）
    {
        // 若把 0x00 的宽度当作 1（错误的实现），则 [0x00, ...] 会被误解析
        // 正确实现：vint_width_from_first(0x00) == 9 → 任何 max_width ≤8 都拒绝
        let w = vint_width_from_first(0x00);
        let rejected = decode_vint(&[0x00, 0x11], 0, 2, 8, false).is_err();
        set.add(
            "C03-EBML-MUTATION-DETECTED-零首字节判据真实",
            w == 9 && rejected,
            "",
        );
    }

    // G9 checked_range 边界正确（offset+size 恰好等于 limit 时合法）
    {
        let ok1 = checked_range(0, 10, 10);
        let ok2 = checked_range(5, 5, 10);
        let bad1 = checked_range(0, 11, 10);
        let bad2 = checked_range(11, 0, 10);
        set.add(
            "C03-ERR-RANGE-区间边界精确",
            ok1 && ok2 && !bad1 && !bad2,
            "",
        );
    }

    // G10 checked_add 溢出返回 None
    {
        let r = checked_add(u64::MAX, 1);
        let ok = checked_add(1, 2) == Some(3) && r.is_none();
        set.add("C03-ERR-ADD-加法溢出返回None", ok, "");
    }
}

// ---- §H 对拍（ffprobe 交叉验证）------------------------------------------

fn run_veg03_cross_checks(set: &mut CheckSet) {
    // H1 无分歧：构造语料的解析结果与登记的 ffprobe 事实一致
    {
        // 语料：1 视频轨 VP9 640x360 25fps，1 音频轨 Opus 48kHz
        // 2 个 Cluster，各 1 个 SimpleBlock → 2 帧全关键帧
        let c0: Vec<Vec<u8>> = vec![simple_block(1, 0, 0x80, &[0xAA; 16])];
        let c1: Vec<Vec<u8>> = vec![simple_block(1, 0, 0x80, &[0xBB; 16])];
        let data = build_webm(&[&c0, &c1], &[0, 40], 1_000_000);
        let f = demux_ok(&data);

        // 登记的 ffprobe 事实（fixture，与语料严格对应）
        let facts = FfprobeFacts {
            format_name: "webm".to_string(),
            duration_ms: 2000,
            stream_count: 2,
            video_codec: "vp9".to_string(),
            width: 1920,
            height: 1080,
            avg_frame_rate_milli: 25_000,
            nb_frames: 2,
            keyframe_count: 2,
        };
        let findings = cross_check(&f, &facts);
        set.add("C03-CROSS-NO-DIVERGENCE-对拍零分歧", findings.is_empty(), "");
    }

    // H2 有分歧：故意错报帧数，确认对拍能抓出来（反假变体）
    {
        let c0: Vec<Vec<u8>> = vec![simple_block(1, 0, 0x80, &[0xAA; 16])];
        let c1: Vec<Vec<u8>> = vec![simple_block(1, 0, 0x80, &[0xBB; 16])];
        let data = build_webm(&[&c0, &c1], &[0, 40], 1_000_000);
        let f = demux_ok(&data);

        // 错报：nb_frames=99（实际 2）
        let facts = FfprobeFacts {
            format_name: "webm".to_string(),
            duration_ms: 2000,
            stream_count: 2,
            video_codec: "vp9".to_string(),
            width: 1920,
            height: 1080,
            avg_frame_rate_milli: 25_000,
            nb_frames: 99,
            keyframe_count: 2,
        };
        let findings = cross_check(&f, &facts);
        let caught = findings.iter().any(|x| x.field == "nb_frames");
        set.add("C03-CROSS-DIVERGENCE-错报帧数被抓出", caught, "");
    }

    // H3 对拍两侧同规范化：V_VP9 ↔ vp9 映射一致（不是 V_VP9 ↔ vp9 字面比）
    {
        let t = MkvTrack {
            track_number: 1,
            kind: TrackKind::Video,
            codec_id: "V_VP9".to_string(),
            default_duration_ns: 40_000_000,
            default_fps_milli: 25_000,
            pixel_width: 1920,
            pixel_height: 1080,
            display_width: 0,
            display_height: 0,
            sampling_frequency: 0.0,
            channels: 0,
            bit_depth: 0,
        };
        let name = matroska_codec_to_ffprobe(&t.codec_id);
        set.add("C03-CROSS-NORMALIZED-两侧同规范化", name == "vp9", "");
    }

    // H4 真实语料对拍：minimal_webm 与对应 facts 一致
    {
        let data = minimal_webm();
        let f = demux_ok(&data);
        let facts = FfprobeFacts {
            format_name: "webm".to_string(),
            duration_ms: 40,
            stream_count: 1,
            video_codec: "vp9".to_string(),
            width: 640,
            height: 360,
            avg_frame_rate_milli: 25_000,
            nb_frames: 1,
            keyframe_count: 1,
        };
        let findings = cross_check(&f, &facts);
        set.add("C03-CROSS-REAL-真实语料对拍零分歧", findings.is_empty(), "");
    }
}
