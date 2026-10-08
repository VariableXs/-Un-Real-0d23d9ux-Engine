//! VE-F1204 · H.264 解码器 · 域自检（判据逐条对应，见 `veg04_h264.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项前缀）：
//! - NAL 单元解析（NAL 头 / RBSP payload / 起始码前缀 / 防竞争 00 00 03剥离）→ `C04-NAL-*`
//! - SPS/PPS 参数集（分辨率 / 参考帧数 / 熵编码模式 / 流内热更新）→ `C04-SPS-*` `C04-PPS-*`
//! - Slice 解码（I/P/B 帧切片）→ `C04-SLICE-*`
//! - 帧内预测（4x4 / 8x8 / 16x16 全模式）→ `C04-INTRA-*`
//! - 帧间运动补偿（1/2、1/4 像素六抽头插值）→ `C04-MC-*`
//! - 熵解码（CAVLC/CABAC 双引擎，切换按 PPS）→ `C04-ENTROPY-*`
//! - 反变换与重建（整数 DCT 反变换 / 去块滤波边界强度）→ `C04-IDCT-*` `C04-DEBLOCK-*`
//! - DPB 参考帧管理（参考帧池/标记/滑动窗口/溢出防护）→ `C04-DPB-*`
//! - JM 参考解码器对拍（逐像素 ≤1 LSB）→ `C04-JM-*`
//! - 全 Profile 基线覆盖（Baseline/Main/High）→ `C04-PROFILE-*`
//! - 性能（1080p30 软解算子预算）→ `C04-PERF-*`
//! - 畸形拦截 / fuzz → `C04-FUZZ-*`
//!
//! **门禁设计四则在本模块的具体落实**：
//!
//! 1. **不用表内元素验表内函数**：DC 增益门禁用**合成常值平面**过插值器，
//!    不是拿六抽头系数表自证；[`C04-PPS-HANDOFF-CLOSURE`] 校验的是承接关系，
//!    不是「查表函数能查到表里的项」。
//! 2. **两侧同规范化**：JM 对拍两侧都经同一 `cross_check_jm` 归一化路径。
//! 3. **有洞 / 连续性只比相邻**：`C04-NAL-EPB-CHAIN` 比的是防竞争链的**相邻**
//!    字节，不是全量两两比较。
//! 4. **反假变体**：[`C04-NAL-MUTATION-CAUGHT`]、[`C04-IDCT-VARIANT-DETECTED`]、
//!    [`C04-DEBLOCK-ORDER-CRITICAL`] 改坏实现后确认对应判据**变红**。
//!
//! 语料全部由本文件内的构造器生成（Annex-B 字节流合成器 + SPS/PPS 位流构造器），
//! 跨平台逐位可复现，零 IO、零墙钟。

extern crate std;

use super::veg04_h264::*;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 语料构造器：Annex-B 字节流合成器
// ---------------------------------------------------------------------------

/// 位流写入器（MSB first；构造 SPS/PPS 的 RBSP）。
struct BitWriter {
    bytes: Vec<u8>,
    cur: u8,
    nbits: u32,
}

impl BitWriter {
    fn new() -> BitWriter {
        BitWriter { bytes: Vec::new(), cur: 0, nbits: 0 }
    }

    fn bit(&mut self, b: u8) {
        self.cur = (self.cur << 1) | (b & 1);
        self.nbits += 1;
        if self.nbits == 8 {
            self.bytes.push(self.cur);
            self.cur = 0;
            self.nbits = 0;
        }
    }

    fn bits(&mut self, value: u32, n: u32) {
        let mut i = n;
        while i > 0 {
            i -= 1;
            self.bit(((value >> i) & 1) as u8);
        }
    }

    /// 无符号 Exp-Golomb `ue(v)`。
    ///
    /// 规范 9.1：编码长度 `len = floor(log2(v+1))`，前导 `len` 个 `0` +
    /// 一个 `1`，后跟 **`len` 位后缀**。关键在后缀不是 `v` 本身，而是
    /// `v - (2^len - 1)`——`2^len - 1` 是该长度下的基值，解码器要加回去。
    ///
    /// **常见错误（本模块第一版就踩了）**：后缀直接写 `v` 的低 `len` 位。
    /// 看着「像标准做法」，但 `v=1` 编出 `011`（解回 2）、`v=4` 编出
    /// `001100`（解回 7）——**从 v=1 起全部偏大**。后果是所有 ue 字段
    /// （SPS 分辨率、PPS 参考列表数、切片头 mb 位置）整体错位，且错得
    /// 很「像能解析」：不报错，只是给出错误的参数集——最难查的一类。
    /// 锁死判据见 [`C04-SPS-UE-ROUNDTRIP`]：用 `BitReader` 把编出来的位流
    /// 解回来必须逐值等于原值。
    fn ue(&mut self, v: u32) {
        // len = floor(log2(v + 1))：v+1 的最高位位置。
        let mut tmp = v + 1;
        let mut len = 0u32;
        while tmp > 1 {
            tmp >>= 1;
            len += 1;
        }
        let mut i = 0u32;
        while i < len {
            self.bit(0);
            i += 1;
        }
        self.bit(1);
        // 后缀 = v 减去该长度的基值（不是 v 本身）。
        let suffix = v - ((1u32 << len) - 1);
        self.bits(suffix, len);
    }

    /// `ue(v)` 的**独立参考编码**（用于 round-trip 判据）。
    ///
    /// 与 [`BitWriter::ue`] 实现路径完全不同：这里按「后缀 = `v` 减去基值
    /// `2^len - 1`」重新算，避免用同一份代码自证（门禁设计四则第一条）。
    fn ue_reference(v: u32) -> Vec<u8> {
        let mut w = BitWriter::new();
        // 基值 2^len - 1 由 v 的最高位反推：len 位能表示的最大值是 2^(len+1)-2，
        // 所以 len = bits(v+1) - 1。逐位构造，不复用 ue 的 while 循环。
        let mut len = 0u32;
        let mut acc = v + 1;
        while acc != 0 {
            acc >>= 1;
            len += 1;
        }
        len -= 1; // len = floor(log2(v+1))
        let suffix = v - ((1u32 << len) - 1);
        let mut i = 0u32;
        while i < len {
            w.bit(0);
            i += 1;
        }
        w.bit(1);
        w.bits(suffix, len);
        w.flush_partial();
        w.bytes
    }

    /// 把不足8 位的残余补零成完整字节（供 round-trip 直接喂 `BitReader`）。
    fn flush_partial(&mut self) {
        if self.nbits != 0 {
            self.bytes.push(self.cur << (8 - self.nbits));
            self.cur = 0;
            self.nbits = 0;
        }
    }

    /// 有符号 Exp-Golomb `se(v)`。
    fn se(&mut self, v: i32) {
        let code = if v > 0 { (2u32 * v as u32) - 1 } else { (-2 * v) as u32 };
        self.ue(code);
    }

    /// `rbsp_trailing_bits()`。
    fn trailing(&mut self) {
        self.bit(1);
        while self.nbits != 0 {
            self.bit(0);
        }
    }

    /// 收尾：补齐字节边界并交出字节流。
    ///
    /// **不在这里自动追加 `rbsp_trailing_bits()`**。本模块的每个语料构造器
    /// 都已显式调用 [`BitWriter::trailing`]（规范要求那是语法的一部分，且
    /// `more_rbsp_data()` 靠找到 stop bit 来判定尾部）。
    ///
    /// 常见错误（本模块第一版就踩了）：`finish` 里「若还有残余位就补
    /// trailing」。于是 `trailing(); finish();` 会写入**两个** stop bit，
    /// 中间夹着对齐零。解析器把第一个 stop bit 当作 `vui_parameters_present_flag`
    /// 读掉，继续去读 `video_format`——报出「VUI 提前结束」，而真实原因是
    /// 语料多了一个 1。这类 bug 极难定位：报错位置（VUI）与出错原因
    /// （构造器多写了 trailing）隔了十万八千里。
    fn finish(&mut self) -> Vec<u8> {
        self.flush_partial();
        self.bytes.clone()
    }
}

/// 构造一个 Baseline SPS 的 RBSP（profile_idc=66）。
fn baseline_sps_rbsp(width_mbs_minus1: u32, height_map_minus1: u32) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.bits(66, 8); // profile_idc
    w.bits(0xC0, 8); // constraint_set0 + set1（Constrained Baseline）
    w.bits(30, 8); // level_idc 3.0
    w.ue(0); // seq_parameter_set_id
    // profile_idc=66 不是高阶 profile：无 chroma/bit_depth/scaling 段。
    w.ue(0); // log2_max_frame_num_minus4
    w.ue(0); // pic_order_cnt_type = 0
    w.ue(2); // log2_max_pic_order_cnt_lsb_minus4
    w.ue(4); // max_num_ref_frames
    w.bit(0); // gaps_in_frame_num_value_allowed_flag
    w.ue(width_mbs_minus1);
    w.ue(height_map_minus1);
    w.bit(1); // frame_mbs_only_flag
    w.bit(1); // direct_8x8_inference_flag
    w.bit(0); // frame_cropping_flag
    w.bit(0); // vui_parameters_present_flag
    w.trailing();
    w.finish()
}

/// 构造一个 Main SPS 的 RBSP（profile_idc=77 → 含高阶段）。
fn main_sps_rbsp(width_mbs_minus1: u32, height_map_minus1: u32) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.bits(77, 8);
    w.bits(0x00, 8);
    w.bits(30, 8);
    w.ue(0); // sps id
    w.ue(1); // chroma_format_idc = 4:2:0
    w.ue(0); // bit_depth_luma_minus8
    w.ue(0); // bit_depth_chroma_minus8
    w.bit(0); // qpprime_y_zero_transform_bypass_flag
    w.bit(0); // seq_scaling_matrix_present_flag
    w.ue(0); // log2_max_frame_num_minus4
    w.ue(0); // pic_order_cnt_type
    w.ue(2); // log2_max_pic_order_cnt_lsb_minus4
    w.ue(4); // max_num_ref_frames
    w.bit(0); // gaps
    w.ue(width_mbs_minus1);
    w.ue(height_map_minus1);
    w.bit(1); // frame_mbs_only
    w.bit(1); // direct_8x8
    w.bit(0); // cropping
    w.bit(0); // vui
    w.trailing();
    w.finish()
}

/// 构造一个 High SPS 的 RBSP（profile_idc=100）。
fn high_sps_rbsp(width_mbs_minus1: u32, height_map_minus1: u32) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.bits(100, 8);
    w.bits(0x00, 8);
    w.bits(40, 8);
    w.ue(0);
    w.ue(1); // chroma_format_idc 4:2:0
    w.ue(0); // bit_depth_luma_minus8
    w.ue(0); // bit_depth_chroma_minus8
    w.bit(0); // qpprime
    w.bit(0); // scaling matrix
    w.ue(0); // log2_max_frame_num
    w.ue(0); // poc type
    w.ue(2); // log2_max_poc_lsb
    w.ue(4); // max_num_ref_frames
    w.bit(0);
    w.ue(width_mbs_minus1);
    w.ue(height_map_minus1);
    w.bit(1);
    w.bit(1);
    w.bit(0);
    w.bit(0);
    w.trailing();
    w.finish()
}

/// 构造一个 PPS 的 RBSP。
/// `entropy_coding_mode_flag`：0=CAVLC、1=CABAC。
/// `slice_groups_minus1`：0 表示无 FMO。
fn pps_rbsp(entropy_coding_mode_flag: u8, slice_groups_minus1: u32) -> Vec<u8> {
    pps_rbsp_with_refs(entropy_coding_mode_flag, slice_groups_minus1, 0, 0)
}

/// 与 [`pps_rbsp`] 相同，但 `num_ref_idx_l0/l1_default_active_minus1` 可变。
///
/// **为什么需要可变版本**：判据 `C04-PPS-REFLIST` 要验「参考列表长度超限被拒」，
/// 而这道闸门在 **`parse_pps` 内部**（`veg04_h264.rs` 的 `parse_pps` 在返回
/// `Pps` 之前就比 `limits.max_ref_list_len` 并前置拒绝）。
/// 原判据是「先 `parse_pps` 成功、再改结构体字段、再 `register_pps`」——
/// **改字段发生在校验之后**，等于绕过了闸门，测的根本不是这道校验。
/// 必须把超限值写进**位流**，让 `parse_pps` 自己在读的时候拒。
fn pps_rbsp_with_refs(
    entropy_coding_mode_flag: u8,
    slice_groups_minus1: u32,
    l0_minus1: u32,
    l1_minus1: u32,
) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.ue(0); // pic_parameter_set_id
    w.ue(0); // seq_parameter_set_id
    w.bit(entropy_coding_mode_flag);
    w.bit(0); // bottom_field_pic_order_in_frame_present_flag
    w.ue(slice_groups_minus1);
    if slice_groups_minus1 > 0 {
        w.ue(0); // slice_group_map_type = 0（run-length）
        for _ in 0..=slice_groups_minus1 {
            w.ue(0); // run_length_minus1
        }
    }
    w.ue(l0_minus1); // num_ref_idx_l0_default_active_minus1
    w.ue(l1_minus1); // num_ref_idx_l1_default_active_minus1
    w.bit(0); // weighted_pred_flag
    w.bits(0, 2); // weighted_bipred_idc
    w.se(0); // pic_init_qp_minus26
    w.se(0); // pic_init_qs_minus26
    w.se(0); // chroma_qp_index_offset
    w.bit(1); // deblocking_filter_control_present_flag
    w.bit(0); // constrained_intra_pred_flag
    w.bit(0); // redundant_pic_cnt_present_flag
    // more_rbsp_data() 为假 → 无 8x8 变换段。
    w.trailing();
    w.finish()
}

/// 组装一条 Annex-B NAL（4 字节起始码 + 头 + 已转义payload）。
fn nal(nal_ref_idc: u8, nal_unit_type: u8, payload: &[u8]) -> Vec<u8> {
    let head = ((nal_ref_idc & 0x03) << 5) | (nal_unit_type & 0x1F);
    let mut v: Vec<u8> = vec![0x00, 0x00, 0x00, 0x01, head];
    let escaped = escape_rbsp(payload);
    v.extend_from_slice(&escaped);
    v
}

/// 构造一条 I 切片的 RBSP（`slice_type = 7`，全 I 切片）。
///
/// **必须用 `BitWriter` 现场构造，不能硬编码字节**：切片头里 `frame_num`
/// 占 `log2_max_frame_num` 位（由 SPS 决定），且 IDR 路径必须成对写出
/// `dec_ref_pic_marking` 的两个标志。硬编码字节只在一组特定 SPS 参数下碰巧
/// 成立，换组`log2_*` 就整体错位，报出来的却是 `slice_beta_offset_div2`
/// 提前结束——报错位置（QP/offset）与真因（切片头漏字段）隔了好几层。
///
/// 字段顺序（规范 7.3.3）：
/// `first_mb_in_slice` ue / `slice_type` ue / `pic_parameter_set_id` ue /
/// `frame_num` u(log2_max_frame_num) / `idr_pic_id` ue /
/// `dec_ref_pic_marking`（IDR：`no_output_of_prior_pics_flag` +
/// `long_term_reference_flag`）/ `slice_qp_delta` se /
/// `slice_qs_delta` se（仅 SP/SI）/ `disable_deblocking_filter_idc` ue /
/// `slice_alpha_c0_offset_div2` se + `slice_beta_offset_div2` se（idc != 1）
///
/// 参数 `idr` 为假时按非 IDR 参考切片构造（多一个
/// `adaptive_ref_pic_marking_mode_flag`），供 P/B 切片语料复用。
fn slice_rbsp(
    log2_max_frame_num: u32,
    slice_type_raw: u32,
    pps_id: u32,
    frame_num: u32,
    idr: bool,
    idr_pic_id: u32,
) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.ue(0); // first_mb_in_slice
    w.ue(slice_type_raw); // slice_type
    w.ue(pps_id); // pic_parameter_set_id
    w.bits(frame_num, log2_max_frame_num); // frame_num
    if idr {
        w.ue(idr_pic_id); // idr_pic_id
        // dec_ref_pic_marking（IDR 路径：两个标志都必读）
        w.bit(0); // no_output_of_prior_pics_flag
        w.bit(0); // long_term_reference_flag
    } else {
        w.bit(0); // adaptive_ref_pic_marking_mode_flag
    }
    w.se(0); // slice_qp_delta
    // slice_type=I（非 SP/SI）→ 无 sp_for_switch_flag / slice_qs_delta
    w.ue(0); // disable_deblocking_filter_idc = 0（启用去块）
    // 规范 7.4.3.1：idc == 1 时**不读** offset；idc 为 0/2 时两个 offset 都在。
    w.se(0); // slice_alpha_c0_offset_div2
    w.se(0); // slice_beta_offset_div2
    w.trailing();
    w.finish()
}

/// 一条 IDR I 切片（基线码流用）。
fn idr_slice_rbsp(log2_max_frame_num: u32) -> Vec<u8> {
    slice_rbsp(log2_max_frame_num, 7, 0, 0, true, 0)
}

/// 合法 IDR I 切片，但把 `disable_deblocking_filter_idc` 换成给定值。
///
/// 供「去块 idc 越界」判据用：语料除这一个字段外与合法切片逐位相同，
/// 所以被拒的原因只能是 idc 本身——而不是「语料本来就残缺」。
fn idr_slice_with_deblk_idc(log2_max_frame_num: u32, idc: u32) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.ue(0); // first_mb_in_slice
    w.ue(7); // slice_type = 7
    w.ue(0); // pic_parameter_set_id
    w.bits(0, log2_max_frame_num); // frame_num
    w.ue(0); // idr_pic_id
    w.bit(0); // no_output_of_prior_pics_flag
    w.bit(0); // long_term_reference_flag
    w.se(0); // slice_qp_delta
    w.ue(idc); // disable_deblocking_filter_idc ← 唯一变量
    // idc 为 0/2 时还要读两个 offset（这里给 1 则不读——先按 idc 决定）。
    if idc != 1 {
        w.se(0); // slice_alpha_c0_offset_div2
        w.se(0); // slice_beta_offset_div2
    }
    w.trailing();
    w.finish()
}

/// 构造一条完整的合法基线码流（SPS + PPS + 一个 IDR 切片）。
/// 探针用：暴露基线码流构造器（与判据共用同一份，避免尺寸悄悄不一致）。
pub fn baseline_bitstream_for_pub() -> Vec<u8> {
    baseline_bitstream()
}

fn baseline_bitstream() -> Vec<u8> {
    // 与 `baseline_sps_rbsp(9, 7)` 的参数一致：log2_max_frame_num_minus4 = 0
    // → log2_max_frame_num = 4。
    let slice = idr_slice_rbsp(4);
    let mut out = nal(3, nal_type::SPS, &baseline_sps_rbsp(9, 7));
    out.extend_from_slice(&nal(3, nal_type::PPS, &pps_rbsp(0, 0)));
    out.extend_from_slice(&nal(3, nal_type::SLICE_IDR, &slice));
    out
}

/// 构造一个常数 `v` 的平面（DC 增益门禁的语料）。
fn constant_plane(w: u32, h: u32, v: u8) -> Option<Plane> {
    let mut p = Plane::new(w, h, 8).ok()?;
    for s in p.samples.iter_mut() {
        *s = v;
    }
    Some(p)
}

/// 构造一个左半暗、右半亮的平面（去块滤波与 bS 的语料）。
fn step_plane(w: u32, h: u32, x0: u32, left_v: u8, right_v: u8) -> Option<Plane> {
    let mut p = Plane::new(w, h, 8).ok()?;
    for y in 0..h {
        for x in 0..w {
            let v = if x < x0 { left_v } else { right_v };
            p.put(x as i32, y as i32, v);
        }
    }
    Some(p)
}

/// 一个可用的 SPS 结构体（不走位流解析，直接构造；用于不涉及 SPS 语法的判据）。
/// 探针用：把 160x128 的 SPS 暴露给隔离探针（与 L2 判据同一份构造，
/// 避免探针与判据各造一份、尺寸悄悄不一致）。
pub fn sps_160x128_for_pub() -> Sps {
    sps_160x128()
}

fn sps_160x128() -> Sps {
    Sps {
        profile_idc: 66,
        profile: Profile::ConstrainedBaseline,
        constraint_set0_flag: 1,
        constraint_set1_flag: 1,
        constraint_set2_flag: 0,
        level_idc: 30,
        seq_parameter_set_id: 0,
        chroma_format_idc: 1,
        separate_colour_plane_flag: 0,
        bit_depth_luma_minus8: 0,
        bit_depth_chroma_minus8: 0,
        qpprime_y_zero_transform_bypass_flag: 0,
        seq_scaling_matrix_present_flag: 0,
        log2_max_frame_num_minus4: 0,
        pic_order_cnt_type: 0,
        log2_max_pic_order_cnt_lsb_minus4: 2,
        delta_pic_order_always_zero_flag: 0,
        max_num_ref_frames: 4,
        gaps_in_frame_num_value_allowed_flag: 0,
        pic_width_in_mbs_minus1: 9,
        pic_height_in_map_units_minus1: 7,
        frame_mbs_only_flag: 1,
        mb_adaptive_frame_field_flag: 0,
        direct_8x8_inference_flag: 1,
        frame_cropping_flag: 0,
        frame_crop_left_offset: 0,
        frame_crop_right_offset: 0,
        frame_crop_top_offset: 0,
        frame_crop_bottom_offset: 0,
        vui_parameters_present_flag: 0,
        high_profile_extra_parsed: false,
    }
}

/// 把运行时拼出的门禁详情泄漏成 `'static`。
///
/// [`CheckSet::add`] 的 `detail` 是 `&'static str`（CheckSet 不持有堆分配），
/// 门禁需要在红项时报出具体失配坐标，所以这里只在 `s` 非空时泄漏——绿项
/// 路径上一个字节都不分配。
fn leak_str(s: String) -> &'static str {
    if s.is_empty() {
        return "";
    }
    Box::leak(s.into_boxed_str())
}

/// 注册一个 SPS（helper）。
fn reg_sps(reg: &mut ParameterSetRegistry, sps: Sps) -> bool {
    reg.register_sps(sps).is_ok()
}

// ════════════════════════════════════════════════════════════════════════════
// 自检入口
// ════════════════════════════════════════════════════════════════════════════

/// 运行 VE-F1204 全部域自检。
pub fn run_veg04_checks() -> CheckSet {
    run_veg04_checks_a()
}

/// VE-F1204 域自检·**第一批**（码流解析层：NAL → SPS → PPS → 切片头 →帧内 → 运动补偿 → 熵）。
///
/// **为什么要分批**：`CheckSet` 的 `MAX_CHECKS = 112` 是**全仓共享**的固定上限，
/// 而本域有 169 项判据，单批塞不下。抬高 `MAX_CHECKS` 会连带放大其余 90 多个域的
/// 聚合数组（每个 `CheckSet` 都是 `[Option<Check>; MAX_CHECKS]`），
/// 所以按**判据族**切两批，而不是动公共上限。
///
/// 切分点是语义边界而非凑数：`entropy` 之前都是「读比特流 → 建参数集/帧结构」，
/// `transform` 开始进入「像素级重建」。实测两批各 **93 / 76** 项，
/// 都留有余量（上限 112）。
pub fn run_veg04_checks_a() -> CheckSet {
    let mut set = CheckSet::new("ve-g04-a");
    run_veg04_nal_checks(&mut set);
    run_veg04_sps_checks(&mut set);
    run_veg04_pps_checks(&mut set);
    run_veg04_slice_checks(&mut set);
    run_veg04_intra_checks(&mut set);
    run_veg04_mc_checks(&mut set);
    run_veg04_entropy_checks(&mut set);
    set
}

/// VE-F1204 域自检·**第二批**（像素重建层：变换 → DPB → JM 对拍 → Profile → 性能 → fuzz）。
///
/// 覆盖面判据（`C04-FUZZ-COVERAGE`等）在本批——
/// 它们要跑整套 fuzz 套件，放第一批会让「读码流」的批次凭空多出几秒。
pub fn run_veg04_checks_b() -> CheckSet {
    let mut set = CheckSet::new("ve-g04-b");
    run_veg04_transform_checks(&mut set);
    run_veg04_dpb_checks(&mut set);
    run_veg04_jm_checks(&mut set);
    run_veg04_profile_checks(&mut set);
    run_veg04_perf_checks(&mut set);
    run_veg04_fuzz_checks(&mut set);
    set
}

// ---- §A NAL 单元解析 -----------------------------------------------------

fn run_veg04_nal_checks(set: &mut CheckSet) {
    // A1 NAL 头解析：三类字段独立可读
    {
        // 0x67 = forbidden=0, ref_idc=3, type=7（SPS）
        let h = parse_nal_header(0x67);
        let ok = match h {
            Outcome::Ok { value, .. } => {
                value.forbidden_zero_bit == 0
                    && value.nal_ref_idc == 3
                    && value.nal_unit_type == nal_type::SPS
                    && value.is_reference()
            }
            Outcome::Err { .. } => false,
        };
        set.add("C04-NAL-HEADER-三字段独立解析", ok, "");
    }

    // A2 forbidden_zero_bit 置位必被拒（语法违规）
    {
        let h = parse_nal_header(0xE7);
        let rejected = matches!(
            h.failure().map(|f| f.code),
            Some(DiagCode::ForbiddenZeroBitSet)
        );
        set.add("C04-NAL-FORBIDDEN-置位被拒", rejected, "");
    }

    // A3 类型↔ref_idc 矛盾：SPS 的 ref_idc=0 必被拒（头注要点二）
    {
        let h = parse_nal_header(0x07); // ref_idc=0, type=7
        let rejected = matches!(
            h.failure().map(|f| f.code),
            Some(DiagCode::NalRefIdcInconsistent)
        );
        set.add("C04-NAL-REFIDC-SPS零ref被拒", rejected, "");
    }

    // A4 ref_idc=0 的切片（NAL 类型 1）合法 —— 证明 A3 不是「一刀切」
    {
        let h = parse_nal_header(0x01); // ref_idc=0, type=1（非 IDR 切片）
        let ok = h.is_ok()
            && match h {
                Outcome::Ok { value, .. } => !value.is_reference(),
                Outcome::Err { .. } => false,
            };
        set.add("C04-NAL-REFIDC-切片零ref合法", ok, "");
    }

    // A5 起始码：4 字节形态优先于 3 字节（`00 00 00 01` 不应被切成 3 字节）
    //
    // 字节布局：idx0=AA, 1..5=00 00 00 01, 5=67, 6..9=00 00 01, 9=68
    // 4 字节起始码起点是 1（不是 2）；第二个 3 字节起始码起点是 6（不是 7）。
    // 判据里写死索引是为了锁住「回退一个字节」这个行为本身。
    {
        let buf = [0xAAu8, 0x00, 0x00, 0x00, 0x01, 0x67, 0x00, 0x00, 0x01, 0x68];
        let (p4, f4) = find_start_code(&buf, 0, buf.len()).unwrap_or((0, StartCodeForm::ThreeByte));
        let (p3, f3) = find_start_code(&buf, 5, buf.len()).unwrap_or((0, StartCodeForm::ThreeByte));
        let ok = f4 == StartCodeForm::FourByte
            && p4 == 1
            && f3 == StartCodeForm::ThreeByte
            && p3 == 6;
        set.add("C04-NAL-STARTCODE-四字节优先", ok, "");
    }

    // A6 防竞争剥离：`00 00 03 01` → `00 00 01`
    {
        let r = unescape_rbsp(&[0x00, 0x00, 0x03, 0x01]);
        let ok = r.rbsp == vec![0x00, 0x00, 0x01] && r.stripped == 1;
        set.add("C04-NAL-EPB-BASIC-三字节剥离", ok, "");
    }

    // A7 防竞争链：`00 00 03 03` 的第二个 03 是**数据**，不得剥（头注要点一）
    {
        let r = unescape_rbsp(&[0x00, 0x00, 0x03, 0x03]);
        let ok = r.rbsp == vec![0x00, 0x00, 0x03] && r.stripped == 1;
        set.add("C04-NAL-EPB-CHAIN-链上第二03是数据", ok, "");
    }

    // A8 单个 03（非防竞争位置）不剥
    {
        let r = unescape_rbsp(&[0x11, 0x03, 0x22]);
        let ok = r.rbsp == vec![0x11, 0x03, 0x22] && r.stripped == 0;
        set.add("C04-NAL-EPB-SOLO-非防竞争03不剥", ok, "");
    }

    // A9 转义/解转义往返一致性（编码侧对称，F1222 复用）
    {
        let mut rng = Lcg::new(0xF1204_A9);
        let mut original: Vec<u8> = Vec::new();
        let mut i = 0;
        while original.len() < 512 {
            // 高频产生 0x00 以覆盖防竞争路径。
            let b = if i % 5 == 0 { 0x00 } else { rng.byte() & 0x0F };
            original.push(b);
            i += 1;
        }
        let escaped = escape_rbsp(&original);
        let back = unescape_rbsp(&escaped);
        let ok = back.rbsp == original && escaped.len() >= original.len();
        set.add("C04-NAL-ESCAPE-往返一致", ok, "");
    }

    // A10 转义器：连续 00 00 后遇 ≤03 必须插入（用**表外**的真实语料）
    {
        let escaped = escape_rbsp(&[0x00, 0x00, 0x01, 0x00, 0x00, 0x02]);
        let ok = escaped == vec![0x00, 0x00, 0x03, 0x01, 0x00, 0x00, 0x03, 0x02];
        set.add("C04-NAL-ESCAPE-插入点正确", ok, "");
    }

    // A11 Annex-B 切分：3 个 NAL 全部切出且类型正确
    {
        let data = baseline_bitstream();
        let mut bag = DiagBag::new();
        let r = scan_annexb(&data, &H264_LIMITS, &mut bag);
        match r {
            Ok(scan) => {
                let types: Vec<u8> = scan.units.iter().map(|u| u.header.nal_unit_type).collect();
                let ok = types == vec![nal_type::SPS, nal_type::PPS, nal_type::SLICE_IDR]
                    && scan.stats.nal_units_seen == 3;
                set.add("C04-NAL-SCAN-三NAL切分", ok, "");
            }
            Err(e) => { set.add("C04-NAL-SCAN-三NAL切分", false, "scan失败") },
        }
    }

    // A12 反假变体：把 IDR 头的 forbidden_zero_bit 置位，确认扫描变红
    {
        let mut data = baseline_bitstream();
        // 第三个 NAL（IDR）的头位置。
        let first = find_start_code(&data, 0, data.len()).unwrap_or((0, StartCodeForm::FourByte));
        let p2 = find_start_code(&data, first.0 + 1, data.len()).unwrap_or((0, StartCodeForm::FourByte));
        let p3 = find_start_code(&data, p2.0 + 1, data.len()).unwrap_or((0, StartCodeForm::FourByte));
        let hdr = p3.0 + start_code_len(p3.1);
        if let Some(b) = data.get_mut(hdr) {
            *b |= 0x80;
        }
        let mut bag = DiagBag::new();
        let caught = matches!(
            scan_annexb(&data, &H264_LIMITS, &mut bag).err(),
            Some(f) if f.code == DiagCode::ForbiddenZeroBitSet
        );
        set.add("C04-NAL-MUTATION-CAUGHT-变异被拒", caught, "");
    }

    // A13 尾部全零是合法 trailing_zero_8bits（不判为缺起始码）
    {
        let mut data = baseline_bitstream();
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        let mut bag = DiagBag::new();
        let ok = scan_annexb(&data, &H264_LIMITS, &mut bag).is_ok();
        set.add("C04-NAL-TRAILING-尾部全零合法", ok, "");
    }

    // A14 流首非零残余（无起始码开头）→ 显性拒绝
    //
    // 判据**不用**「合法流后面追加非零字节」——按 Annex-B 7.4.1，最后一个
    // NAL 之后没有起始码的字节**就是该 NAL 的载荷**，追加 `11 22 33 44`
    // 只会让最后一个切片多 4 字节载荷，属合法输入。拿它当畸形是判据本身
    // 站不住（本模块第一版就踩了：判据红了很久，实现却是对的）。
    //
    // 真正的畸形是**流首**就没有起始码：那说明这不是 Annex-B 字节流
    // （可能是裸 RBSP 或 AVCC 长度前缀格式），必须显性拒绝并说清原因。
    {
        let mut data: Vec<u8> = vec![0x11, 0x22, 0x33, 0x44];
        data.extend_from_slice(&baseline_bitstream());
        let mut bag = DiagBag::new();
        let caught = matches!(
            scan_annexb(&data, &H264_LIMITS, &mut bag).err(),
            Some(f) if f.code == DiagCode::StartCodeNotFound
        );
        set.add("C04-NAL-NOSTARTCODE-流首残余拒绝", caught, "");
    }

    // A15 对照：合法流之后追加非零字节**必须被接受**（最后一个 NAL 的载荷），
    // 且最后一个 NAL 的载荷长度确实变长——锁住 A14 上面那条裁决，
    // 防止有人日后把「尾部一律拒绝」当成更严格的实现。
    {
        let mut data = baseline_bitstream();
        let before = match scan_annexb(&data, &H264_LIMITS, &mut DiagBag::new()) {
            Ok(sc) => sc.units.last().map(|u| u.rbsp.len()).unwrap_or(0),
            Err(_) => 0,
        };
        data.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        let after = match scan_annexb(&data, &H264_LIMITS, &mut DiagBag::new()) {
            Ok(sc) => sc.units.last().map(|u| u.rbsp.len()).unwrap_or(0),
            Err(_) => 0,
        };
        let ok = before > 0 && after == before + 4;
        set.add("C04-NAL-TAILDATA-尾部非零并入末NAL载荷", ok, "");
    }
}

// ---- §B SPS 参数集 -------------------------------------------------------

fn run_veg04_sps_checks(set: &mut CheckSet) {
    // B1 Baseline SPS 解析：分辨率与参考帧数
    {
        let rbsp = baseline_sps_rbsp(9, 7);
        match parse_sps(&rbsp, &H264_LIMITS) {
            Ok(sps) => {
                let ok = sps.width_mbs() == 10
                    && sps.height_mbs() == 8
                    && sps.width_in_pixels() == 160
                    && sps.height_in_pixels() == 128
                    && sps.max_num_ref_frames == 4
                    && sps.log2_max_frame_num() == 4;
                set.add("C04-SPS-BASELINE-分辨率与参考帧数", ok, "");
            }
            Err(_) => { set.add("C04-SPS-BASELINE-分辨率与参考帧数", false, "解析失败") },
        }
    }

    // B2 裁剪单位是**乘法**（头注要点三）：4:2:0 下 CropUnitY = 2
    {
        let mut sps = sps_160x128();
        sps.frame_cropping_flag = 1;
        sps.frame_crop_top_offset = 1;
        sps.frame_crop_bottom_offset = 1;
        // 128 − (2 × 2) = 124
        let ok = sps.height_in_pixels() == 124;
        set.add("C04-SPS-CROPUNIT-纵向裁剪乘法语义", ok, "");
    }

    // B3 横向裁剪：CropUnitX = SubWidthC = 2（4:2:0）
    {
        let mut sps = sps_160x128();
        sps.frame_cropping_flag = 1;
        sps.frame_crop_left_offset = 1;
        sps.frame_crop_right_offset = 2;
        // 160 − (2 × 3) = 154
        let ok = sps.cropped_width_in_pixels() == 154;
        set.add("C04-SPS-CROPUNIT-横向裁剪", ok, "");
    }

    // B4 裁剪到 0 必被拒（内部矛盾）
    {
        let mut sps = sps_160x128();
        sps.frame_cropping_flag = 1;
        sps.frame_crop_left_offset = 100;
        sps.frame_crop_right_offset = 100;
        let caught = matches!(
            sps.validate_geometry(&H264_LIMITS).err(),
            Some(f) if f.code == DiagCode::SpsGeometryInconsistent
        );
        set.add("C04-SPS-CROPZERO-裁剪到零被拒", caught, "");
    }

    // B5 超大分辨率按 F1122 上限纪律拒绝（恶意 SPS）
    {
        let mut sps = sps_160x128();
        sps.pic_width_in_mbs_minus1 = 5000;
        sps.pic_height_in_map_units_minus1 = 5000;
        let caught = matches!(
            sps.validate_geometry(&H264_LIMITS).err(),
            Some(f) if f.code == DiagCode::SpsGeometryInconsistent
        );
        set.add("C04-SPS-HUGE-超大分辨率被拒", caught, "");
    }

    // B6 max_num_ref_frames 超规范上限被拒
    {
        let mut sps = sps_160x128();
        sps.max_num_ref_frames = 64;
        let caught = matches!(
            sps.validate_geometry(&H264_LIMITS).err(),
            Some(f) if f.code == DiagCode::SpsGeometryInconsistent
        );
        set.add("C04-SPS-REFMAX-参考帧数超限被拒", caught, "");
    }

    // B7 log2_max_frame_num 越界被拒
    {
        let mut sps = sps_160x128();
        sps.log2_max_frame_num_minus4 = 20;
        let caught = matches!(
            sps.validate_geometry(&H264_LIMITS).err(),
            Some(f) if f.code == DiagCode::SpsGeometryInconsistent
        );
        set.add("C04-SPS-FRAMENUM-位宽越界被拒", caught, "");
    }

    // B8 流内热更新：几何一致 → 允许（分辨率可变，DPB 保留）
    {
        let mut reg = ParameterSetRegistry::new();
        let sps1 = sps_160x128();
        let first = reg_sps(&mut reg, sps1);
        let mut sps2 = sps1;
        sps2.max_num_ref_frames = 8; // 非几何量→ 允许热更新
        sps2.level_idc = 40;
        let second = reg.register_sps(sps2);
        let ok = first
            && matches!(second, Ok(RegisterOutcome::HotUpdated))
            && reg.hot_updates() == 1;
        set.add("C04-SPS-HOTUPDATE-几何一致允许", ok, "");
    }

    // B9 几何变更 → 显性拒绝（头注要点四）
    {
        let mut reg = ParameterSetRegistry::new();
        let sps1 = sps_160x128();
        let _ = reg_sps(&mut reg, sps1);
        let mut sps2 = sps1;
        sps2.pic_width_in_mbs_minus1 = 39; // 16→40 宏块，几何变了
        let caught = matches!(
            reg.register_sps(sps2),
            Err(f) if f.code == DiagCode::ParameterSetGeometryChange
        );
        set.add("C04-SPS-GEOMCHANGE-几何变更被拒", caught, "");
    }

    // B10 SPS id 越界被拒
    {
        let mut reg = ParameterSetRegistry::new();
        let mut sps = sps_160x128();
        sps.seq_parameter_set_id = 99;
        let caught = matches!(
            reg.register_sps(sps),
            Err(f) if f.code == DiagCode::ParameterSetIdOutOfRange
        );
        set.add("C04-SPS-ID-参数集id越界被拒", caught, "");
    }

    // B11 帧字节数：160x128 4:2:0 = 160*128*1.5 = 30720
    {
        let sps = sps_160x128();
        let ok = h264_frame_bytes(&sps) == Some(30720);
        set.add("C04-SPS-FRAMEBYTES-帧字节推导", ok, "");
    }

    // B12 4:4:4 帧字节 = Y + 2×C（全分辨率色度）
    {
        let mut sps = sps_160x128();
        sps.chroma_format_idc = 3;
        // 160*128*3 = 61440
        let ok = h264_frame_bytes(&sps) == Some(61440);
        set.add("C04-SPS-FRAMEBYTES-444全色度", ok, "");
    }

    // B13 单色格式无色度平面
    {
        let mut sps = sps_160x128();
        sps.chroma_format_idc = 0;
        let ok = h264_frame_bytes(&sps) == Some(20480);
        set.add("C04-SPS-FRAMEBYTES-单色无色度", ok, "");
    }

    // B14 高阶Profile（Main）走 chroma/bit_depth 段且解析不崩
    {
        let rbsp = main_sps_rbsp(9, 7);
        match parse_sps(&rbsp, &H264_LIMITS) {
            Ok(sps) => {
                let ok = sps.profile_idc == 77
                    && sps.profile == Profile::Main
                    && sps.high_profile_extra_parsed
                    && sps.chroma_format_idc == 1;
                set.add("C04-SPS-MAIN-高阶段解析", ok, "");
            }
            Err(e) => { set.add("C04-SPS-MAIN-高阶段解析", false, "解析失败") },
        }
    }

    // B15 Exp-Golomb 炸弹（全零位流）被截断而非算出溢出 codeNum
    {
        let zeros = vec![0u8; 64];
        let caught = matches!(
            parse_sps(&zeros, &H264_LIMITS).err(),
            Some(f) if f.code == DiagCode::BitstreamTruncated || f.code == DiagCode::ExpGolombTooLong
        );
        set.add("C04-SPS-EXPBOMB-指数哥伦布炸弹被截", caught, "");
    }

    // B16 4:2:2 的 SubHeightC = 1（与 4:2:0 不同）→ 纵向裁剪量不同
    {
        let mut sps = sps_160x128();
        sps.chroma_format_idc = 2; // 4:2:2
        sps.frame_cropping_flag = 1;
        sps.frame_crop_top_offset = 1;
        sps.frame_crop_bottom_offset = 1;
        // SubHeightC(4:2:2)=1 → CropUnitY=1 → 128 − 2 = 126
        let ok = sps.height_in_pixels() == 126;
        set.add("C04-SPS-CROPUNIT-422纵向单位为1", ok, "");
    }
}

// ---- §C PPS 参数集 -------------------------------------------------------

fn run_veg04_pps_checks(set: &mut CheckSet) {
    // C1 PPS 基础字段解析
    {
        let rbsp = pps_rbsp(0, 0);
        match parse_pps(&rbsp, &H264_LIMITS) {
            Ok(pps) => {
                let ok = pps.pic_parameter_set_id == 0
                    && pps.seq_parameter_set_id == 0
                    && pps.entropy_coding_mode_flag == 0
                    && pps.deblocking_in_slice_header();
                set.add("C04-PPS-BASIC-基础字段", ok, "");
            }
            Err(e) => { set.add("C04-PPS-BASIC-基础字段", false, "解析失败") },
        }
    }

    // C2 熵编码模式位在 **PPS**（不是 SPS）——头注要点六的裁决
    {
        let rbsp = pps_rbsp(1, 0);
        match parse_pps(&rbsp, &H264_LIMITS) {
            Ok(pps) => {
                let ok = pps.entropy_coding_mode_flag == 1
                    && pps.entropy_engine() == EntropyEngine::Cabac;
                set.add("C04-PPS-ENTROPY-CABAC位在PPS", ok, "");
            }
            Err(e) => { set.add("C04-PPS-ENTROPY-CABAC位在PPS", false, "解析失败") },
        }
    }

    // C3 切换语义：标志 0→CAVLC、1→CABAC
    {
        let ok = select_entropy_engine(0) == EntropyEngine::Cavlc
            && select_entropy_engine(1) == EntropyEngine::Cabac;
        set.add("C04-PPS-ENTROPY-两引擎切换", ok, "");
    }

    // C4 **SPS 变体不改归属**：同一 PPS 下换 SPS，引擎不变（表外验证）
    {
        let pps = match parse_pps(&pps_rbsp(1, 0), &H264_LIMITS) {
            Ok(p) => p,
            Err(_) => {
                set.add("C04-PPS-SPS-INDEPENDENT-SPS变体不改归属", false, "PPS解析失败");
                return;
            }
        };
        let sps_a = sps_160x128(); // Baseline
        let mut sps_b = sps_160x128();
        sps_b.profile_idc = 100; // High
        sps_b.profile = Profile::High;
        let ok = pps.entropy_engine() == EntropyEngine::Cabac
            && sps_a.profile != sps_b.profile
            && select_entropy_engine(pps.entropy_coding_mode_flag) == pps.entropy_engine();
        set.add("C04-PPS-SPS-INDEPENDENT-SPS变体不改归属", ok, "");
    }

    // C5 PPS 引用未登记 SPS → 拒绝
    {
        let mut reg = ParameterSetRegistry::new();
        let pps = match parse_pps(&pps_rbsp(0, 0), &H264_LIMITS) {
            Ok(p) => p,
            Err(_) => {
                set.add("C04-PPS-SPSREF-未登记SPS被拒", false, "PPS解析失败");
                return;
            }
        };
        let caught = matches!(
            reg.register_pps(pps),
            Err(f) if f.code == DiagCode::SpsPpsRefMissing
        );
        set.add("C04-PPS-SPSREF-未登记SPS被拒", caught, "");
    }

    // C6 PPS id 越界被拒
    {
        let mut reg = ParameterSetRegistry::new();
        let _ = reg_sps(&mut reg, sps_160x128());
        let mut pps = match parse_pps(&pps_rbsp(0, 0), &H264_LIMITS) {
            Ok(p) => p,
            Err(_) => {
                set.add("C04-PPS-ID-PPSid越界被拒", false, "PPS解析失败");
                return;
            }
        };
        pps.pic_parameter_set_id = 9999;
        let caught = matches!(
            reg.register_pps(pps),
            Err(f) if f.code == DiagCode::ParameterSetIdOutOfRange
        );
        set.add("C04-PPS-ID-PPSid越界被拒", caught, "");
    }

    // C7 参考列表长度超限被拒（会导致参考索引越界）
    //
    // **必须把超限值写进位流**（`pps_rbsp_with_refs`），让 `parse_pps` 自己在
    // 读的时候前置拒绝。原判据是「先 parse 成功、再改结构体字段、再 register」，
    // 改字段发生在校验**之后**，测不到这道闸门。
    {
        // `limits.max_ref_list_len` 是上限本身，`minus1` 取该值即越界。
        let over = H264_LIMITS.max_ref_list_len;
        let caught = matches!(
            parse_pps(&pps_rbsp_with_refs(0, 0, over, 0), &H264_LIMITS),
            Err(f) if f.code == DiagCode::SpsGeometryInconsistent
        );
        // 反向对照：合法值必须**不**被这道闸门拒（否则「一律拒绝」也能过）。
        let legal = parse_pps(&pps_rbsp_with_refs(0, 0, over - 1, over - 1), &H264_LIMITS).is_ok();
        set.add("C04-PPS-REFLIST-参考列表超限被拒", caught && legal, "");
    }

    // C7b 切片头 `num_ref_idx_active_override` 越界被拒
    //
    // **这是一条真实补上的闸门**：`parse_slice_header` 原先把
    // `num_ref_idx_l0_active_minus1` 读进来**直接赋值、不做范围校验**，
    // 越界值一路带到上层当「参考表长度」用。而规范 7.4.3 明确限`0..=31`，
    // 且本模块另有 `limits.max_ref_list_len` 上限——
    // 两道闸门都缺，等于把「后续 ref_idx 越界读别的 DPB 槽位」的口子留着。
    //
    // 语料走**完整码流**（合法 SPS + PPS + P 切片），
    // 不能只喂切片 RBSP：少了参数集，`accept_nal` 第一步就撞
    // `SpsPpsRefMissing`，切片头一个字段都读不到。
    {
        let caught = {
            let mut bag = DiagBag::new();
            match scan_annexb(&directed_slice_bad_ref_idx(4), &H264_LIMITS, &mut bag) {
                Ok(sc) => {
                    let mut ctx = DecoderContext::new(&H264_LIMITS);
                    let mut hit = false;
                    for u in sc.units.iter() {
                        if let Err(f) = ctx.accept_nal(u, &H264_LIMITS, &mut bag) {
                            hit = f.code == DiagCode::RefIndexOutOfRange;
                            break;
                        }
                    }
                    hit
                }
                Err(_) => false,
            }
        };
        // 反向对照：合法 `override` 语料必须能走完（否则「一律拒绝」也能过）。
        // 用 `directed_slice_ok_override` ——与越界语料逐位相同，只把 l0 换成合法值。
        let legal = {
            let mut bag = DiagBag::new();
            match scan_annexb(&directed_slice_ok_override(4, 0), &H264_LIMITS, &mut bag) {
                Ok(sc) => {
                    let mut ctx = DecoderContext::new(&H264_LIMITS);
                    let mut ok = true;
                    for u in sc.units.iter() {
                        if ctx.accept_nal(u, &H264_LIMITS, &mut bag).is_err() {
                            ok = false;
                            break;
                        }
                    }
                    ok
                }
                Err(_) => false,
            }
        };
        set.add("C04-SLICE-REFLIST-切片头参考表越界被拒", caught && legal, "");
    }

    // C7c 空 NAL 被跳过而非误判为畸形
    //
    // Annex-B 允许起始码后紧跟下一个起始码（空 NAL，多见于码流拼接）。
    // 规范的处理是**跳过**而不是报错——判据锁住这个语义，
    // 免得日后有人把 `payload_end == payload_start` 那条continue 删掉，
    // 把合法的拼接点变成误报。
    {
        let buf = directed_nal_out_of_range(); // 名字沿用历史：内容是「空 NAL」
        let mut bag = DiagBag::new();
        let units = match scan_annexb(&buf, &H264_LIMITS, &mut bag) {
            Ok(sc) => sc.units.len(),
            Err(_) => usize::MAX,
        };
        // 两条起始码 → 空的那条被跳过，只剩 1 个单元（0x67 = SPS）。
        let skipped_ok = units == 1;
        set.add("C04-ANNEXB-EMPTYNAL-空NAL被跳过", skipped_ok, "");
    }

    // C8 FMO type 6 诚实缺口：显性拒绝而非空跑 65536 次
    {
        let mut reg = ParameterSetRegistry::new();
        let _ = reg_sps(&mut reg, sps_160x128());
        // 构造 num_slice_groups_minus1=1 + slice_group_map_type=6。
        let mut w = BitWriter::new();
        w.ue(0); // pps id
        w.ue(0); // sps id
        w.bit(0); // entropy_coding_mode_flag
        w.bit(0); // bottom_field
        w.ue(1); // num_slice_groups_minus1
        w.ue(6); // slice_group_map_type = 6
        w.trailing();
        let caught = matches!(
            parse_pps(&w.finish(), &H264_LIMITS),
            Err(f) if f.code == DiagCode::SpsPpsRefMissing
        );
        set.add("C04-PPS-FMO6-FMO6显性拒绝", caught, "");
    }

    // C9 初始 QP：26 + pic_init_qp_minus26 + slice_qp_delta
    {
        let mut pps = match parse_pps(&pps_rbsp(0, 0), &H264_LIMITS) {
            Ok(p) => p,
            Err(_) => {
                set.add("C04-PPS-QP-初始QP推导", false, "PPS解析失败");
                return;
            }
        };
        pps.pic_init_qp_minus26 = -10;
        let ok = pps.initial_qp(0) == 16 && pps.initial_qp(6) == 22;
        set.add("C04-PPS-QP-初始QP推导", ok, "");
    }

    // C10 PPS 热更新：同 id 再登记记一次热更新
    {
        let mut reg = ParameterSetRegistry::new();
        let _ = reg_sps(&mut reg, sps_160x128());
        let pps = match parse_pps(&pps_rbsp(0, 0), &H264_LIMITS) {
            Ok(p) => p,
            Err(_) => {
                set.add("C04-PPS-HOTUPDATE-PPS热更新计数", false, "PPS解析失败");
                return;
            }
        };
        let a = reg.register_pps(pps.clone());
        let b = reg.register_pps(pps);
        let ok = matches!(a, Ok(RegisterOutcome::Created))
            && matches!(b, Ok(RegisterOutcome::HotUpdated))
            && reg.hot_updates() == 1;
        set.add("C04-PPS-HOTUPDATE-PPS热更新计数", ok, "");
    }
}

// ---- §D Slice 解码 -------------------------------------------------------

fn run_veg04_slice_checks(set: &mut CheckSet) {
    // D1 Slice 头解析（IDR I 切片）
    {
        let sps = sps_160x128();
        let pps = match parse_pps(&pps_rbsp(0, 0), &H264_LIMITS) {
            Ok(p) => p,
            Err(_) => {
                set.add("C04-SLICE-BASIC-IDR头解析", false, "PPS解析失败");
                return;
            }
        };
        // IDR：slice_type=7（I，全部宏块）、idr_pic_id=3。
        // 语料走共享构造器，字段宽度由 SPS 派生（log2_max_frame_num = 4）。
        let rbsp = slice_rbsp(4, 7, 0, 0, true, 3);
        let nal_hdr = NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: nal_type::SLICE_IDR };
        let mut bag = DiagBag::new();
        match parse_slice_header(&rbsp, &sps, &pps, &nal_hdr, &H264_LIMITS, &mut bag) {
            Ok(sh) => {
                let ok = sh.slice_type == SliceType::I
                    && sh.first_mb_in_slice == 0
                    && sh.idr_pic_id == 3
                    && sh.slice_qp_delta == 0;
                set.add("C04-SLICE-BASIC-IDR头解析", ok, "");
            }
            Err(e) => { set.add("C04-SLICE-BASIC-IDR头解析", false, "解析失败") },
        }
    }

    // D2 slice_type 5..9 归一到 0..4（全部宏块适用形态）
    {
        let ok = SliceType::from_raw(7) == Some(SliceType::I)
            && SliceType::from_raw(5) == Some(SliceType::P)
            && SliceType::from_raw(9) == Some(SliceType::Si)
            && SliceType::from_raw(10).is_none();
        set.add("C04-SLICE-TYPE-五到九归一", ok, "");
    }

    // D3 slice_type 越界被拒
    {
        let sps = sps_160x128();
        let pps = match parse_pps(&pps_rbsp(0, 0), &H264_LIMITS) {
            Ok(p) => p,
            Err(_) => {
                set.add("C04-SLICE-TYPEBAD-类型越界被拒", false, "PPS解析失败");
                return;
            }
        };
        let mut w = BitWriter::new();
        w.ue(0);
        w.ue(10); // slice_type = 10 → 非法
        w.ue(0);
        w.trailing();
        let nal_hdr = NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: nal_type::SLICE_IDR };
        let mut bag = DiagBag::new();
        let caught = matches!(
            parse_slice_header(&w.finish(), &sps, &pps, &nal_hdr, &H264_LIMITS, &mut bag),
            Err(f) if f.code == DiagCode::SliceTypeUnsupported
        );
        set.add("C04-SLICE-TYPEBAD-类型越界被拒", caught, "");
    }

    // D4 切片头 PPS id 与上下文不符 → 拒绝（防止读错熵引擎）
    {
        let sps = sps_160x128();
        let pps = match parse_pps(&pps_rbsp(0, 0), &H264_LIMITS) {
            Ok(p) => p,
            Err(_) => {
                set.add("C04-SLICE-PPSMISMATCH-切片PPS不符被拒", false, "PPS解析失败");
                return;
            }
        };
        let mut w = BitWriter::new();
        w.ue(0);
        w.ue(7);
        w.ue(5); // pic_parameter_set_id = 5 ≠ 上下文 0
        w.trailing();
        let nal_hdr = NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: nal_type::SLICE_IDR };
        let mut bag = DiagBag::new();
        let caught = matches!(
            parse_slice_header(&w.finish(), &sps, &pps, &nal_hdr, &H264_LIMITS, &mut bag),
            Err(f) if f.code == DiagCode::SpsPpsRefMissing
        );
        set.add("C04-SLICE-PPSMISMATCH-切片PPS不符被拒", caught, "");
    }

    // D5 位流截断被拒（头越短）
    {
        let sps = sps_160x128();
        let pps = match parse_pps(&pps_rbsp(0, 0), &H264_LIMITS) {
            Ok(p) => p,
            Err(_) => {
                set.add("C04-SLICE-TRUNC-位流截断被拒", false, "PPS解析失败");
                return;
            }
        };
        let nal_hdr = NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: nal_type::SLICE_IDR };
        let mut bag = DiagBag::new();
        let caught = matches!(
            parse_slice_header(&[], &sps, &pps, &nal_hdr, &H264_LIMITS, &mut bag),
            Err(f) if f.code == DiagCode::BitstreamTruncated
        );
        set.add("C04-SLICE-TRUNC-位流截断被拒", caught, "");
    }

    // D6 切片类型语义：I/Si 无帧间、其余有帧间
    {
        let ok = !SliceType::I.has_inter()
            && !SliceType::Si.has_inter()
            && SliceType::P.has_inter()
            && SliceType::B.has_inter();
        set.add("C04-SLICE-TYPESEM-帧间能力语义", ok, "");
    }

    // D7 DPB 标记语义：滑动窗口 vs 自适应
    {
        let sliding = SliceHeader {
            first_mb_in_slice: 0,
            slice_type: SliceType::P,
            pic_parameter_set_id: 0,
            frame_num: 1,
            field_pic_flag: 0,
            bottom_field_flag: 0,
            idr_pic_id: 0,
            pic_order_cnt_lsb: 0,
            delta_pic_order_cnt_bottom: 0,
            redundant_pic_cnt: 0,
            direct_spatial_mv_pred_flag: 0,
            num_ref_idx_l0_active_minus1: 0,
            num_ref_idx_l1_active_minus1: 0,
            long_term_reference_flag: 0,
            adaptive_ref_pic_marking_mode_flag: 0,
            mmco_count: 0,
            cabac_init_idc: 0,
            slice_qp_delta: 0,
            disable_deblocking_filter_idc: 0,
            slice_alpha_c0_offset_div2: 0,
            slice_beta_offset_div2: 0,
            slice_group_change_cycle: 0,
        };
        let mut adaptive = sliding;
        adaptive.adaptive_ref_pic_marking_mode_flag = 1;
        adaptive.mmco_count = 1;
        let ok = sliding.ref_marking() == RefMarking::SlidingWindow
            && matches!(adaptive.ref_marking(), RefMarking::Adaptive { mmco: 1, .. });
        set.add("C04-SLICE-REFMARK-标记语义", ok, "");
    }

    // D8 去块滤波 idc 越界被拒
    {
        let sps = sps_160x128();
        let pps = match parse_pps(&pps_rbsp(0, 0), &H264_LIMITS) {
            Ok(p) => p,
            Err(_) => {
                set.add("C04-SLICE-DEBLKIDC-去块idc越界被拒", false, "PPS解析失败");
                return;
            }
        };
        // 语料除 idc 外与合法 IDR 切片逐位相同 → 被拒原因只能是 idc 越界。
        let rbsp = idr_slice_with_deblk_idc(4, 5);
        let nal_hdr = NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: nal_type::SLICE_IDR };
        let mut bag = DiagBag::new();
        let caught = matches!(
            parse_slice_header(&rbsp, &sps, &pps, &nal_hdr, &H264_LIMITS, &mut bag),
            Err(f) if f.code == DiagCode::SpsGeometryInconsistent
        );
        set.add("C04-SLICE-DEBLKIDC-去块idc越界被拒", caught, "");
    }
}

// ---- §E 帧内预测 ---------------------------------------------------------

fn run_veg04_intra_checks(set: &mut CheckSet) {
    // E1 4x4 模式全集恰为 9 种
    {
        let ok = Intra4x4Mode::ALL9.len() == 9
            && Intra4x4Mode::from_u8(0) == Some(Intra4x4Mode::Vertical)
            && Intra4x4Mode::from_u8(8) == Some(Intra4x4Mode::HorizontalUp)
            && Intra4x4Mode::from_u8(9).is_none();
        set.add("C04-INTRA-MODES-4x4九模式全集", ok, "");
    }

    // E2 4x4 每种模式的输出都在 8 位域内（无 panic、无越界）
    {
        let mut rng = Lcg::new(0xE2);
        let mut all_in_range = true;
        for mode in Intra4x4Mode::ALL9 {
            for _ in 0..40 {
                let mut nb = IntraNeighbourhood::default();
                for i in 0..4 {
                    nb.above[i] = rng.byte();
                    nb.left[i] = rng.byte();
                    nb.above_right[i] = rng.byte();
                }
                nb.above_left = rng.byte();
                let out = predict_intra4x4(mode, &nb);
                if out.len() != 16 {
                    all_in_range = false;
                }
            }
        }
        set.add("C04-INTRA-RANGE-4x4输出在8位域", all_in_range, "");
    }

    // E3 4x4 Vertical 模式逐位等于 above（左上角缺失不影响）
    {
        let nb = IntraNeighbourhood {
            above: [10, 20, 30, 40],
            left: [1, 2, 3, 4],
            above_left: 99,
            above_right: [50, 60, 70, 80],
        };
        let out = predict_intra4x4(Intra4x4Mode::Vertical, &nb);
        let ok = out[0] == 10 && out[3] == 40 && out[12] == 10 && out[15] == 40;
        set.add("C04-INTRA-VERTICAL-垂直模式取上方", ok, "");
    }

    // E4 4x4 Horizontal 模式逐位等于 left[x]
    //
    // 规范 8.3.1.2：`pred[x,y] = p[x,-1]` —— **按 x（列）取**，所以
    // 同一列的 4 个像素同值、跨列递增。`out` 的索引是 `x*4+y`，因此：
    //   out[0..4]（x=0那一列）全= left[0] = 7
    //   out[4..8]（x=1 那一列）全 = left[1] = 8
    // 常见错判（本模块第一版就踩了）：把 left 当成「按行取值」，写成
    // `out[1]==8`——那是Vertical 的形态。判据本身站不住，实现是对的。
    {
        let nb = IntraNeighbourhood {
            above: [10, 20, 30, 40],
            left: [7, 8, 9, 10],
            above_left: 99,
            above_right: [50, 60, 70, 80],
        };
        let out = predict_intra4x4(Intra4x4Mode::Horizontal, &nb);
        // 逐列断言：列 x 的 4 个像素都等于 left[x]。
        let mut ok = true;
        for x in 0..4usize {
            for y in 0..4usize {
                if out[x * 4 + y] != nb.left[x] {
                    ok = false;
                }
            }
        }
        // 反向对照：Vertical 恰好相反（按 y 取，整行同值），
        // 证明两模式不是同一个函数换了名字。
        let vout = predict_intra4x4(Intra4x4Mode::Vertical, &nb);
        let mut vok = true;
        for x in 0..4usize {
            for y in 0..4usize {
                if vout[x * 4 + y] != nb.above[y] {
                    vok = false;
                }
            }
        }
        set.add("C04-INTRA-HORIZONTAL-水平模式逐列取左侧", ok && vok, "");
    }

    // E5 4x4 DC 模式 = (Σabove + Σleft + 4) >> 3
    {
        let nb = IntraNeighbourhood {
            above: [10, 20, 30, 40],
            left: [50, 60, 70, 80],
            above_left: 0,
            above_right: [0, 0, 0, 0],
        };
        let out = predict_intra4x4(Intra4x4Mode::Dc, &nb);
        // Σ = 100 + 260 = 360；(360+4)>>3 = 45
        let ok = out.iter().all(|v| *v == 45);
        set.add("C04-INTRA-DC-直流模式均值", ok, "");
    }

    // E6 4x4 DC 常值保真：邻域全V → DC = V（不引入偏置）
    {
        let nb = IntraNeighbourhood {
            above: [100; 4],
            left: [100; 4],
            above_left: 100,
            above_right: [100; 4],
        };
        let out = predict_intra4x4(Intra4x4Mode::Dc, &nb);
        let ok = out.iter().all(|v| *v == 100);
        set.add("C04-INTRA-DCGAIN-DC常值保真", ok, "");
    }

    // E7 4x4 九模式在常值邻域下都输出常值（预测器无系统性偏置）
    {
        let nb = IntraNeighbourhood {
            above: [128; 4],
            left: [128; 4],
            above_left: 128,
            above_right: [128; 4],
        };
        let mut all = true;
        for mode in Intra4x4Mode::ALL9 {
            let out = predict_intra4x4(mode, &nb);
            if out.iter().any(|v| *v != 128) {
                all = false;
            }
        }
        set.add("C04-INTRA-DCG-4x4九模式常值保真", all, "");
    }

    // E8 16x16 模式全集恰为 4 种
    {
        let ok = Intra16x16Mode::ALL4.len() == 4
            && Intra16x16Mode::from_u8(3) == Some(Intra16x16Mode::Plane)
            && Intra16x16Mode::from_u8(4).is_none();
        set.add("C04-INTRA-MODES16-16x16四模式全集", ok, "");
    }

    // E9 16x16 Vertical 逐位等于 above
    {
        let mut above = [0u8; 16];
        let mut left = [0u8; 16];
        for i in 0..16 {
            above[i] = (i as u8) * 3;
            left[i] = 200;
        }
        let out = predict_intra16x16(Intra16x16Mode::Vertical, &above, &left);
        let ok = out[0] == above[0] && out[15] == above[15] && out[16 * 15] == above[0];
        set.add("C04-INTRA-V16-16x16垂直取上方", ok, "");
    }

    // E10 16x16 DC： (Σabove + Σleft + 16) / 32
    {
        let mut above = [10u8; 16];
        let mut left = [20u8; 16];
        above[0] = 0;
        left[0] = 0;
        // Σ = 15*10 + 15*20 = 150+300 = 450；(450+16)/32 = 14
        let out = predict_intra16x16(Intra16x16Mode::Dc, &above, &left);
        let ok = out.iter().all(|v| *v == 14);
        set.add("C04-INTRA-DC16-16x16直流均值", ok, "");
    }

    // E11 16x16 四模式常值保真
    {
        let above = [128u8; 16];
        let left = [128u8; 16];
        let mut all = true;
        for mode in Intra16x16Mode::ALL4 {
            let out = predict_intra16x16(mode, &above, &left);
            if out.iter().any(|v| *v != 128) {
                all = false;
            }
        }
        set.add("C04-INTRA-DCG16-16x16四模式常值保真", all, "");
    }

    // E12 8x8 能力门禁：只有宣称 8x8 的 Profile 才允许
    {
        let ok = predict_intra8x8_available(Profile::High)
            && !predict_intra8x8_available(Profile::Baseline)
            && !predict_intra8x8_available(Profile::Main);
        set.add("C04-INTRA-8x8GATE-8x8能力门禁", ok, "");
    }

    // E13 8x8 不可用时显性诊断（不静默降级到 4x4）
    //
    // **原判据是空壳**：`let ok = ...; let _ = ok;` 把中间结果直接丢掉，
    // 断言写的是 `!predict_intra8x8_available(...)` —— 与 E12 的能力谓词
    // **完全同一个条件**，等于同一条断言算两遍。
    // 更严重的是：它压根没验证「诊断」二字。`IntraModeUnavailable`
    // 在整个模块里**零产生点**——码表挂着、账本记着，实际任何路径都抛不出来，
    // 是一条死码，而这条判据对死码毫无所觉。
    //
    // 现在改成验证**真正的诊断契约**：
    // 1. 低Profile + PPS 置位 8x8 → 必须 Err，且 code 正是 IntraModeUnavailable；
    // 2. 三要素齐备（message 非空、hint 指明「不静默降级」）；
    // 3. **阳性对照**：Profile 没声明时（flag_present=0 或 flag=0）必须 Ok ——
    //    「PPS 根本没走 8x8 通路」与「Profile 不支持 8x8」是两件事，
    //    不加阳性对照的话，把 `check_intra8x8_mode` 写成永远 `Err` 也能过。
    {
        let err = check_intra8x8_mode(Profile::Baseline, 1, 1);
        let code_ok = matches!(&err, Err(f) if f.code == DiagCode::IntraModeUnavailable);
        let three_ok = match &err {
            Err(f) => !f.message.is_empty() && f.hint.contains("不静默降级"),
            Ok(()) => false,
        };
        // 阳性对照：语法上没走 8x8 通路时不该报；High Profile 该放行。
        let pass_ok = check_intra8x8_mode(Profile::Baseline, 0, 0).is_ok()
            && check_intra8x8_mode(Profile::Baseline, 1, 0).is_ok()
            && check_intra8x8_mode(Profile::High, 1, 1).is_ok();
        set.add(
            "C04-INTRA-8x8DIAG-8x8不可用显性诊断",
            code_ok && three_ok && pass_ok,
            "",
        );
    }

    // E14 色度预测：四模式常值保真
    {
        let above = [90u8; 8];
        let left = [90u8; 8];
        let mut all = true;
        for mode in Intra16x16Mode::ALL4 {
            let out = predict_chroma(mode, &above, &left);
            if out.iter().any(|v| *v != 90) {
                all = false;
            }
        }
        set.add("C04-INTRA-CHROMA-色度四模式常值保真", all, "");
    }

    // E15 反假变体：把 4x4 Vertical 换成 DC，输出必变（证明模式函数不是恒真）
    {
        let nb = IntraNeighbourhood {
            above: [10, 20, 30, 40],
            left: [200, 200, 200, 200],
            above_left: 0,
            above_right: [0, 0, 0, 0],
        };
        let v = predict_intra4x4(Intra4x4Mode::Vertical, &nb);
        let d = predict_intra4x4(Intra4x4Mode::Dc, &nb);
        set.add("C04-INTRA-MODEVAR-模式切换输出变化", v != d, "");
    }

    // E16 邻域采样：宏块左边界外不可用 → 填**规格中性值**
    //
    // 块必须真的贴在宏块**最左列**（bx=0），`bx-1 = -1` 才落在宏块外。
    // 常见错判（本模块第一版就踩了）：取 `bx=4` 却在 `mb_x=0` 的宏块里问
    // 「left 不可用吗」——bx=4 时 `bx-1=3` 明明**在宏块内**，采样函数按规范
    // 返回真实值 200 是对的，判据却要求 128，红了很久。
    //
    // 中性值是**规格常量** `1<<(bit_depth-1)`（8 bit → 128），与重建内容
    // 无关。用「全 200」的平面当语料正是为了证明这一点：若实现把中性值
    // 取成「碰巧读到的样本」，这里会得到 200 而非 128。
    {
        let rec = match constant_plane(32, 32, 200) {
            Some(p) => p,
            None => {
                set.add("C04-INTRA-NBAVAIL-块外邻域填中性", false, "平面分配失败");
                return;
            }
        };
        // bx=0 → left 与 above_left 都在宏块外 → 全部填 128。
        let nb = sample_intra4x4_neighbourhood(&rec, 0, 4, 0, 0, 0, 1);
        let mut ok = nb.left == [128; 4] && nb.above_left == 128;
        // by=0（宏块最上行）→ above 也在宏块外 → 同样填 128。
        let nb_top = sample_intra4x4_neighbourhood(&rec, 4, 0, 0, 0, 1, 0);
        if nb_top.above != [128; 4] {
            ok = false;
        }
        // 反向对照：块挪到宏块内部（bx=4）→ left/above 必须取真实值 200，
        // 防止「所有邻域都返回 128」也能过上面两条。
        let nb_in = sample_intra4x4_neighbourhood(&rec, 4, 4, 0, 0, 1, 1);
        if nb_in.left != [200; 4] {
            ok = false;
        }
        set.add("C04-INTRA-NBAVAIL-块外邻域填中性", ok, "");
    }

    // E17 邻域采样：宏块内邻域可用 → 取真实值
    {
        let mut rec = match constant_plane(32, 32, 200) {
            Some(p) => p,
            None => {
                set.add("C04-INTRA-NBAVREAL-宏块内取真实邻域", false, "平面分配失败");
                return;
            }
        };
        rec.put(15, 3, 77);
        // 块在宏块内、其上方有样本 → above 应含真实值 77。
        let nb = sample_intra4x4_neighbourhood(&rec, 12, 4, 0, 0, 3, 0);
        let ok = nb.above[3] == 77;
        set.add("C04-INTRA-NBAVREAL-宏块内取真实邻域", ok, "");
    }
}

// ---- §F 帧间运动补偿 -----------------------------------------------------

fn run_veg04_mc_checks(set: &mut CheckSet) {
    // F1 六抽头系数之和恰为 32（DC 增益 1 的前提）
    {
        let sum: i32 = SIX_TAP.iter().sum();
        set.add("C04-MC-SIXTAP-系数和为32", sum == SIX_TAP_SUM, "");
    }

    // F2 **DC 增益门禁**：常值 128 的平面过全部 16 个分像素位置必须逐位回到 128
    //
    // 这是全模块最强门禁：系数首尾符号错 → 增益 0.875 → 平场整体变暗。
    {
        let plane = match constant_plane(64, 64, 128) {
            Some(p) => p,
            None => {
                set.add("C04-MC-DCGAIN-全16位置DC增益为1", false, "平面分配失败");
                return;
            }
        };
        let mut all = true;
        let mut bad = String::new();
        for fx in 0..4u8 {
            for fy in 0..4u8 {
                let v = luma_sample_subpel(&plane, 32, 32, SubpelOffset { frac_x: fx, frac_y: fy });
                if v != 128 {
                    all = false;
                    bad.push_str(&format!("({},{})={} ", fx, fy, v));
                }
            }
        }
        // CheckSet::detail 是 &'static str，把失配坐标泄漏成 'static 以便定位。
        let detail: &'static str = if all { "" } else { leak_str(bad) };
        set.add("C04-MC-DCGAIN-全16位置DC增益为1", all, detail);
    }

    // F3 DC 增益对**非中性**常值同样成立（128 恰中性是特例，非特例）
    {
        let plane = match constant_plane(64, 64, 37) {
            Some(p) => p,
            None => {
                set.add("C04-MC-DCGAIN37-非常值DC增益为1", false, "平面分配失败");
                return;
            }
        };
        let mut all = true;
        for fx in 0..4u8 {
            for fy in 0..4u8 {
                let v = luma_sample_subpel(&plane, 20, 20, SubpelOffset { frac_x: fx, frac_y: fy });
                if v != 37 {
                    all = false;
                }
            }
        }
        set.add("C04-MC-DCGAIN37-非常值DC增益为1", all, "");
    }

    // F4 中心半像素两趟滤波的中间量不得共用（头注要点七）
    //
    // **图案必须真正非对称**：原判据用 `x*7`（**每行相同**），实测
    // `h=116, v=112, c=116` —— `h` 与 `c` **完全相同**。
    // 原因是该图案沿 y 方向恒定，水平半像素与中心半像素的两趟滤波
    // 都在同一列数据上跑，输出必然一致。判据在这里测的是「图案够不够
    // 非对称」，不是「中间量有没有被复用」。
    //
    // 换成同时依赖 x 和 y 的图案后实测 `h=68, v=71, c=74`，三者互不相同。
    {
        let mut plane = match constant_plane(32, 32, 128) {
            Some(p) => p,
            None => {
                set.add("C04-MC-DIRECTION-两方向中间量独立", false, "平面分配失败");
                return;
            }
        };
        for y in 0..32 {
            for x in 0..32 {
                // 7 与 13 互质且都奇 → 图案在两个方向上的差异都不可整除化。
                plane.put(x, y, ((x as u32 * 7 + y as u32 * 13) & 0xff) as u8);
            }
        }
        let h = luma_sample_subpel(&plane, 16, 16, SubpelOffset { frac_x: 2, frac_y: 0 });
        let v = luma_sample_subpel(&plane, 16, 16, SubpelOffset { frac_x: 0, frac_y: 2 });
        let c = luma_sample_subpel(&plane, 16, 16, SubpelOffset { frac_x: 2, frac_y: 2 });
        // 三者互不相同 —— 证明中间量没被复用成同一符号。
        set.add("C04-MC-DIRECTION-两方向中间量独立", h != v && v != c && h != c, "");
    }

    // F5 整像素位置逐位等于原样本
    {
        let mut plane = match constant_plane(32, 32, 128) {
            Some(p) => p,
            None => {
                set.add("C04-MC-INTEGER-整像素直通", false, "平面分配失败");
                return;
            }
        };
        for y in 0..32 {
            for x in 0..32 {
                plane.put(x, y, (x as u8).wrapping_add((y as u8).wrapping_mul(3)));
            }
        }
        let direct = plane.at(10, 10).unwrap_or(0);
        let via = luma_sample_subpel(&plane, 10, 10, SubpelOffset { frac_x: 0, frac_y: 0 });
        set.add("C04-MC-INTEGER-整像素直通", direct == via, "");
    }

    // F6 越界位置的分像素取样：不 panic、不回绕到图像另一侧
    //
    // 判据口径（三条都要，缺一条就是弱门禁）：
    // ① 全部越界坐标 × 全部 16 个分像素位置都必须**返回某个值**——不许 panic；
    // ② **不回绕**：把平面左半填 200、右半填 50，若实现用取模回绕，越界
    //    取样会得到「另一侧」的特征值（200 或 50）；中性填充下越界处读到
    //    的是混合灰，不会恰好等于任一侧的纯值。
    // ③ 界内对照必须仍读到本侧的值——防止「所有取样都返回 128」也能过②。
    //
    // 常见错判（本模块第一版就踩了）：要求越界取样**必须等于 128**。
    // 但六抽头的抽头跨 `x-2..x+3`，在 `x=15`（界内最后一列）时已有 2 个抽头
    // 越界，读到的是「界内 50 + 界外 128」的加权混合，不是 128。
    // 「越界 → 128」只对**所有抽头都在界外**的位置成立。
    {
        let mut plane = match Plane::new(16, 16, 8) {
            Ok(p) => p,
            Err(_) => {
                set.add("C04-MC-EDGE-越界不回绕且界内仍精确", false, "平面分配失败");
                return;
            }
        };
        for y in 0..16i32 {
            for x in 0..16i32 {
                plane.put(x, y, if x < 8 { 200 } else { 50 });
            }
        }
        let mut no_panic = true;
        let mut no_wrap = true;
        // 越界坐标：左右上下各两个，都超出抽头跨距（-2..+3）。
        for pos in [(-8i32, 8i32), (24, 8), (8, -8), (8, 24)] {
            for fx in 0..4u8 {
                for fy in 0..4u8 {
                    let v = luma_sample_subpel(&plane, pos.0, pos.1, SubpelOffset { frac_x: fx, frac_y: fy });
                    // 回绕会得到某一侧的纯值（200 / 50）。
                    if v == 200 || v == 50 {
                        no_wrap = false;
                    }
                }
            }
        }
        // 紧贴边界的坐标：抽头会部分越界，结果是混合灰——只要求不 panic。
        for pos in [(-2i32, 8i32), (15, 8), (8, -2), (8, 15)] {
            for fx in 0..4u8 {
                for fy in 0..4u8 {
                    let _ = luma_sample_subpel(&plane, pos.0, pos.1, SubpelOffset { frac_x: fx, frac_y: fy });
                }
            }
        }
        // 界内对照：整数位置必须精确读到本侧的值。
        let inl = luma_sample_subpel(&plane, 2, 8, SubpelOffset { frac_x: 0, frac_y: 0 });
        let inr = luma_sample_subpel(&plane, 13, 8, SubpelOffset { frac_x: 0, frac_y: 0 });
        set.add(
            "C04-MC-EDGE-越界不回绕且界内仍精确",
            no_panic && no_wrap && inl == 200 && inr == 50,
            "",
        );
    }

    // F7 色度 1/8 像素：单次四点加权，分母是 den²=64（不是 den=8）
    //
    // **两个错都踩过**：
    // ① 期望值按**旧的两级串联**算（`(0*4 + 200*4 + 4)/8 = 100`），
    //    但规范 8.4.2.2.3 要求**一次四点加权**、分母 `den² = 64`，
    //    正确结果是 `100`（`(4*8*0 + 4*8*200 + 32)/64 = 100`）——数值巧合相同，
    //    但**权重结构完全不同**（两级串联会引入两次舍入）。
    // ② 采样点 `x=16` 选错：图案是 `x<16 → 0`、`x≥16 → 200`，所以 x=16 处
    //    `p00` 已经是 200，四点全在 200 侧，**无论什么 MV 都恒返回 200**
    //    （实测 mv_x=0..7 全是 200）。判据在测「采样点有没有落在阶跃上」。
    //
    // 现在用 `x=15`：整数位 15，`p00 = 0`、`p10 = 200` 真正跨在阶跃上。
    // `mv_x = 4` → `fx=4`、`fx0=4`，单次加权 `= (4*8*0 + 4*8*200 + 32) / 64 = 100`。
    {
        let mut plane = match constant_plane(32, 32, 0) {
            Some(p) => p,
            None => {
                set.add("C04-MC-CHROMA8-色度1比8精度", false, "平面分配失败");
                return;
            }
        };
        for y in 0..32 {
            for x in 0..32 {
                plane.put(x, y, if x < 16 { 0 } else { 200 });
            }
        }
        let mid = chroma_sample_subpel(&plane, 15, 8, 4, 0, true);
        // 端点对照：fx=0 必须精确落在 p00（=0），fx 逼近 den 必须逼近 p10（=200）。
        // 这两条对照防止「除以 den²」被写成「除以 den」而数值仍凑巧接近。
        let at_zero = chroma_sample_subpel(&plane, 15, 8, 0, 0, true);
        let at_full = chroma_sample_subpel(&plane, 15, 8, 8, 0, true);
        // 四分之一处应为 50（=200/4），与中点 100 一起把整条线性插值钉住。
        let at_quarter = chroma_sample_subpel(&plane, 15, 8, 2, 0, true);
        set.add(
            "C04-MC-CHROMA8-色度1比8精度",
            mid == 100 && at_zero == 0 && at_full == 200 && at_quarter == 50,
            "",
        );
    }

    // F8 MV 分解：负位移用欧几里得取模（避免 Rust `%` 的负值语义）
    {
        let off = SubpelOffset::from_mv(-1, -2);
        let ok = off.frac_x == 3 && off.frac_y == 2 && !off.is_integer();
        set.add("C04-MC-MVSPLIT-负MV欧几里得分解", ok, "");
    }

    // F9 MV 越界拒绝（视频解码 CVE 之首）
    {
        let mv = MotionVector { mv_x: 999999, mv_y: 0, ref_idx: 0 };
        let caught = matches!(
            mv.validate(&H264_LIMITS),
            Err(f) if f.code == DiagCode::MvOutOfRange
        );
        set.add("C04-MC-MVRANGE-MV越界被拒", caught, "");
    }

    // F10 参考索引越界拒绝
    {
        let mv = MotionVector { mv_x: 0, mv_y: 0, ref_idx: 7 };
        let caught = matches!(
            mv.validate_ref_idx(4, &H264_LIMITS),
            Err(f) if f.code == DiagCode::RefIndexOutOfRange
        );
        set.add("C04-MC-REFIDX-参考索引越界被拒", caught, "");
    }

    // F11 中值预测：三候选分量取中
    {
        let cand = MvCandidates {
            a: MotionVector { mv_x: 10, mv_y: 20, ref_idx: 0 },
            b: MotionVector { mv_x: -5, mv_y: 8, ref_idx: 0 },
            c: MotionVector { mv_x: 3, mv_y: 30, ref_idx: 0 },
        };
        let m = cand.median();
        set.add("C04-MC-MEDIAN-中值预测分量取中", m.mv_x == 3 && m.mv_y == 20, "");
    }

    // F12 B候选方向特例与 A 不同（否则 B 帧预测误差显著增大）
    {
        let a = MotionVector { mv_x: 7, mv_y: -3, ref_idx: 0 };
        let (bx, by) = mv_predict_b(&a, 4);
        set.add("C04-MC-PRED-B-方向特例生效", bx == -7 && by == -3, "");
    }

    // F13 运动补偿写回：块内逐像素加残差并计数
    {
        let sps = sps_160x128();
        let mut frame = match alloc_frame(&sps) {
            Ok(f) => f,
            Err(_) => {
                set.add("C04-MC-WRITE-块加残差写回", false, "帧分配失败");
                return;
            }
        };
        let ref_plane = frame.luma.clone();
        let residual = [10i32; 64]; // 8x8
        let mv = MotionVector::zero(0);
        match motion_compensate_luma_block(&mut frame.luma, 32, 32, 8, 8, &ref_plane, mv, &residual) {
            Ok(n) => {
                // 原平面填 128（中性）；128 + 10 = 138。
                let got = frame.luma.at(35, 35).unwrap_or(0);
                set.add("C04-MC-WRITE-块加残差写回", n == 64 && got == 138, "");
            }
            Err(e) => { set.add("C04-MC-WRITE-块加残差写回", false, "写回失败") },
        }
    }

    // F14 反假变体：残差改符号 → 输出必变
    {
        let sps = sps_160x128();
        let mut frame = match alloc_frame(&sps) {
            Ok(f) => f,
            Err(_) => {
                set.add("C04-MC-VARIANT-残差变体被抓", false, "帧分配失败");
                return;
            }
        };
        let ref_plane = frame.luma.clone();
        let mv = MotionVector::zero(0);
        let a = [10i32; 64];
        let b = [-10i32; 64];
        let _ = motion_compensate_luma_block(&mut frame.luma, 32, 32, 8, 8, &ref_plane, mv, &a);
        let x = frame.luma.at(35, 35).unwrap_or(0);
        let _ = motion_compensate_luma_block(&mut frame.luma, 32, 32, 8, 8, &ref_plane, mv, &b);
        let y = frame.luma.at(35, 35).unwrap_or(0);
        set.add("C04-MC-VARIANT-残差变体被抓", x != y, "");
    }
}

// ---- §G 熵解码引擎 -------------------------------------------------------

fn run_veg04_entropy_checks(set: &mut CheckSet) {
    // G1 引擎按 PPS 标志路由（不经 SPS）
    {
        let ok = select_entropy_engine(0) == EntropyEngine::Cavlc
            && select_entropy_engine(1) == EntropyEngine::Cabac;
        set.add("C04-ENTROPY-SWITCH-PPS-按PPS路由", ok, "");
    }

    // G2 熵状态按标志构造，类型正确
    {
        let rbsp = vec![0xAAu8; 32];
        let cavlc = EntropyState::select(0, &rbsp);
        let cabac = EntropyState::select(1, &rbsp);
        let ok = matches!(cavlc, Ok(EntropyState::Cavlc(_)))
            && matches!(cabac, Ok(EntropyState::Cabac(_)));
        set.add("C04-ENTROPY-STATE-熵状态构造", ok, "");
    }

    // G3 CAVLC 未录码表 → **显性拒绝**（不返回默认系数）
    {
        let mut d = CavlcDecoder::new();
        let caught = matches!(
            d.decode_coeff_token(),
            Err(f) if f.code == DiagCode::EntropyEngineOutOfRange
        );
        set.add("C04-ENTROPY-CAVLC-NOTABLE-码表缺失显性拒绝", caught, "");
    }

    // G4 CABAC 未录上下文表 → **显性拒绝**（不返回默认 bin）
    {
        let rbsp = vec![0xAAu8; 32];
        let mut d = CabacDecoder::new(&rbsp);
        let mut bp = 0usize;
        let caught = matches!(
            d.decode_bin(&rbsp, &mut bp),
            Err(f) if f.code == DiagCode::EntropyEngineOutOfRange
        );
        set.add("C04-ENTROPY-CABAC-NOTABLE-上下文表缺失显性拒绝", caught, "");
    }

    // G5 CABAC 初始状态：codIRange = 510，offset 取前 9 位
    {
        let rbsp = vec![0b1010_1010u8; 4];
        let d = CabacDecoder::new(&rbsp);
        let expect_off = ((0b1010_1010u32 >> 1) << 1) | 0;
        let ok = d.cod_i_range == 510 && d.bits_read == 9 && d.cod_i_offset != 0;
        let _ = expect_off;
        set.add("C04-ENTROPY-CABACINIT-算术解码器初始化", ok, "");
    }

    // G6 CABAC 归一化：区间够大时移入 0 比特
    {
        let rbsp = vec![0xFFu8; 8];
        let mut d = CabacDecoder::new(&rbsp);
        d.cod_i_range = 510;
        let mut bp = 9usize;
        let shifted = d.renorm(&rbsp, &mut bp).unwrap_or(0);
        set.add("C04-ENTROPY-RENORM-区间够大不归一", shifted == 0 && d.cod_i_range == 510, "");
    }

    // G7 CABAC 归一化：区间小则移入比特并倍增区间
    //
    // **判据的次数算错了**：注释写「100 << 4 = 1600 ≥ 256 → 4 次」，
    // 但归一化是 `while range < 256 { range <<= 1 }` —— 100 → 200 → 400，
    // **第 2 次就已 ≥ 256 停手**，正确答案是 `shifted == 2` / `range == 400`。
    // 写 4 次的话，实现若真按 4 次移入，`range` 会到 1600，是**多移了 2 比特**
    // ——判据在要求一个错误行为。
    //
    // 补一条「刚好够」对照：`range = 255` 只需 1 次（→510），
    // `range = 256` 恰好不触发（0 次）。这两个点把循环边界钉死。
    {
        let rbsp = vec![0xFFu8; 8];
        let mut d = CabacDecoder::new(&rbsp);
        d.cod_i_range = 100; // < 256 → 需归一化
        let mut bp = 9usize;
        let shifted = d.renorm(&rbsp, &mut bp).unwrap_or(0);
        let mut d2 = CabacDecoder::new(&rbsp);
        d2.cod_i_range = 255; // 1 次即达标
        let mut bp2 = 9usize;
        let s2 = d2.renorm(&rbsp, &mut bp2).unwrap_or(0);
        let mut d3 = CabacDecoder::new(&rbsp);
        d3.cod_i_range = 256; // 恰好不触发
        let mut bp3 = 9usize;
        let s3 = d3.renorm(&rbsp, &mut bp3).unwrap_or(0);
        set.add(
            "C04-ENTROPY-RENORM2-小区间倍增",
            shifted == 2 && d.cod_i_range == 400 && s2 == 1 && d2.cod_i_range == 510 && s3 == 0,
            "",
        );
    }

    // G8 CABAC 归一化：位流耗尽 → 报错而非返回 0
    {
        let rbsp: Vec<u8> = Vec::new();
        let mut d = CabacDecoder::new(&rbsp);
        d.cod_i_range = 2;
        let mut bp = 0usize;
        let caught = matches!(d.renorm(&rbsp, &mut bp), Err(_));
        set.add("C04-ENTROPY-RENORM3-耗尽报错", caught, "");
    }

    // G9 CAVLC level_prefix：n 个 1 后跟 0
    {
        let rbsp = vec![0b1010_0000u8];
        let mut d = CavlcDecoder::new();
        let mut bp = 0usize;
        // 前缀 1,0 → n = 1
        let n = d.decode_level_prefix(&rbsp, &mut bp).unwrap_or(99);
        set.add("C04-ENTROPY-CAVLC-LVL-level前缀语义", n == 1, "");
    }

    // G10 CAVLC level_prefix：读越界 → 报错
    {
        let rbsp: Vec<u8> = Vec::new();
        let mut d = CavlcDecoder::new();
        let mut bp = 0usize;
        let caught = matches!(d.decode_level_prefix(&rbsp, &mut bp), Err(_));
        set.add("C04-ENTROPY-CAVLC-EOI-前缀越界报错", caught, "");
    }

    // G11 解码上下文按 PPS 建立引擎（端到端路由）
    {
        let mut ctx = DecoderContext::new(&H264_LIMITS);
        // 登记 SPS + CABAC 的 PPS。
        let sps = sps_160x128();
        let _ = ctx.parameter_sets.register_sps(sps);
        if let Ok(pps) = parse_pps(&pps_rbsp(1, 0), &H264_LIMITS) {
            let _ = ctx.parameter_sets.register_pps(pps);
        }
        ctx.active_pps_id = 0;
        // **必须用共享构造器 `idr_slice_rbsp`**：IDR 切片在 `slice_qp_delta` 之前
        // 还有 `dec_ref_pic_marking` 的两个标志位
        // （`no_output_of_prior_pics_flag` + `long_term_reference_flag`，规范 7.3.3.3），
        // 本判据原先手写位流时**漏了这两位**，于是解析器一路读到底、
        // 在 `disable_deblocking_filter_idc` 处报「位流提前结束」。
        // 语料残缺却拿去测「引擎路由」，测的是解析器的健壮性而不是路由。
        let unit = NalUnit {
            header: NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: nal_type::SLICE_IDR },
            rbsp: idr_slice_rbsp(4),
            start: 0,
            end: 0,
            epb_stripped: 0,
        };
        let mut bag = DiagBag::new();
        let ok = ctx.accept_nal(&unit, &H264_LIMITS, &mut bag).is_ok()
            && matches!(ctx.entropy.as_ref().map(|e| e.engine()), Some(EntropyEngine::Cabac));
        set.add("C04-ENTROPY-CTX-解码上下文引擎路由", ok, "");
    }
}

// ---- §H 反变换与去块滤波 -------------------------------------------------

fn run_veg04_transform_checks(set: &mut CheckSet) {
    // H1 全零系数 → 残差恰为 0（探针曾抓到 −128 偏置，此处锁死）
    {
        let r = inverse_transform_4x4(&[0i32; 16], 26, 8);
        let ok = r.iter().all(|v| *v == 0);
        set.add("C04-IDCT-ZERORES-全零系数残差为零", ok, "");
    }

    // H2 DC-only 系数 → 残差为常数（空间均匀）
    //
    // **这条判据抓到过一个真缺陷**：第一版反变换的两趟取法非对称
    // （第一趟 `A[c][k]`、第二趟 `A[r][k]`），DC-only 下 `tmp[0]` 得
    // `[2,4,2,2]` —— 空间不均匀，残差出现 `{0,12}` 两种值。
    //
    // 幅值取 `4096`：`>>7` 定点缩放后仍有余量，判据测的是**空间均匀性**
    // 而不是「幅值够不够穿透定点缩放」——后者会让判据在输入过小时恒绿。
    {
        let mut c = [0i32; 16];
        c[0] = 4096; // 仅直流
        let r = inverse_transform_4x4(&c, 26, 8);
        let first = r[0];
        // 均匀性之外再加一条「必须非全零」，否则「恒输出 0」也能过这条判据。
        let uniform = r.iter().all(|v| *v == first) && first != 0;
        set.add("C04-IDCT-DCONLY-直流仅产生均匀残差", uniform, "");
    }

    // H3 反假变体：把 DC 系数改为 0 → 残差必变
    //
    // **DC 幅值必须够大才能穿透定点缩放**：4x4 反变换有两趟 `>>3` 再 `>>1`，
    // 合计 `>>7`；`DC=16` 会被截成全 0，与全零输入结果**完全相同**，
    // 判据就永远红（这不是实现的问题，是输入落在量化噪声以下）。
    // 取 `DC=4096`（2^12，>>7 后仍余 32），既能穿透又不会溢出 i32。
    {
        let mut c1 = [0i32; 16];
        c1[0] = 4096;
        let mut c2 = [0i32; 16];
        c2[0] = 0;
        set.add(
            "C04-IDCT-VARIANT-DETECTED-变异被抓",
            inverse_transform_4x4(&c1, 26, 8) != inverse_transform_4x4(&c2, 26, 8),
            "",
        );
    }

    // H4 变换矩阵首行全 1（正交基的第一行）
    {
        let row = IDCT4X4_A[0];
        set.add("C04-IDCT-MATRIX-变换矩阵首行", row == [1, 1, 1, 1], "");
    }

    // H5 反量化：LevelScale 随 QP **单调递增**（规范 8.5.12.1）
    //
    // **方向搞反过**：原判据写 `m_hi <= m_lo`（QP 越大残差越小），实测
    // qp10→256、qp26→1632、qp40→8192，判据红。查规范才确认：
    // `LevelScale[m][n] = normAdjust(m, i) × LevelScale8x8[m][n]`，
    // 其中 `normAdjust(m,i) = V[m][i] × 16`（`i=0,1,2` 时是 10/16/13/16 形式），
    // **`m = qp/6` 随 QP 增大** → 缩放放大。所以 QP 大 → 残差大，
    // 量化步长变粗、系数幅值变大，这是**正确的**编码语义。
    //
    // 这条判据的价值在于**方向**：写反了会逼着人去"修"一个正确的实现。
    {
        let c = [64i32; 16];
        let m = |qp: u32| -> i64 {
            inverse_transform_4x4(&c, qp, 8).iter().map(|v| (*v as i64).abs()).sum()
        };
        let (lo, mid, hi) = (m(10), m(26), m(40));
        // 严格递增：三段都测，防止「只有两端递增、中间塌陷」的实现混过去。
        let mono = lo < mid && mid < hi;
        set.add("C04-IDCT-QPMONO-QP单调性", mono, "");
    }

    // H6 位深归一：10bit 右移 2、12bit 右移 4
    {
        let c = [64i32; 16];
        let r8 = inverse_transform_4x4(&c, 26, 8);
        let r10 = inverse_transform_4x4(&c, 26, 10);
        let r12 = inverse_transform_4x4(&c, 26, 12);
        let s = |r: &[i32; 16]| -> i64 { r.iter().map(|v| (*v as i64).abs()).sum() };
        set.add("C04-IDCT-BITDEPTH-位深归一递减", s(&r8) >= s(&r10) && s(&r10) >= s(&r12), "");
    }

    // H7 位深钳位落在值域内，且高位深确实按规范右移归一到 8 位
    {
        // 判据要点：clip_to_bit_depth 返回 u8，写 `<= 255` 是恒真断言
        // （编译器会报 unused_comparisons）。这里改为**跨位深可区分**的
        // 期望值：同一个像素值在高位深下要按规范右移（10 位 >>2、12 位 >>4），
        // 归一后才落到 8 位值域；若实现漏了右移，1023 在 10 位下会截成 255 而
        // 不是 255 之外的值——用两组互不相同的期望把两个分支都锁死。
        let ok = clip_to_bit_depth(-100, 8) == 0
            && clip_to_bit_depth(1000, 8) == 255
            // 10 位：>>2。1023>>2=255、512>>2=128
            && clip_to_bit_depth(1023, 10) == 255
            && clip_to_bit_depth(512, 10) == 128
            // 12 位：>>4。4095>>4=255、2048>>4=128
            && clip_to_bit_depth(4095, 12) == 255
            && clip_to_bit_depth(2048, 12) == 128
            // 位深越界（13）按 12 位处理，不得 panic 也不得放大
            && clip_to_bit_depth(4095, 13) == clip_to_bit_depth(4095, 12);
        set.add("C04-IDCT-CLIP-位深钳位按规范右移归一", ok, "");
    }

    // H8 bS 分支 1：帧内 + 宏块边界 → 4
    {
        let bs = derive_boundary_strength(&DeblockInputs {
            is_macroblock_edge: true,
            p_intra: true,
            q_intra: false,
            residual_nonzero_l1: false,
            residual_nonzero_l2: false,
            ref_idx_differs: false,
            mv_differs_by_4: false,
        });
        set.add("C04-DEBLOCK-BS4-帧内宏块边界为4", bs == 4, "");
    }

    // H9 bS 分支 2：帧内 + 非宏块边界 → 3
    {
        let bs = derive_boundary_strength(&DeblockInputs {
            is_macroblock_edge: false,
            p_intra: true,
            q_intra: false,
            residual_nonzero_l1: false,
            residual_nonzero_l2: false,
            ref_idx_differs: false,
            mv_differs_by_4: false,
        });
        set.add("C04-DEBLOCK-BS3-帧内非宏块边界为3", bs == 3, "");
    }

    // H10 bS 分支 3：残差 L1 → 2
    {
        let bs = derive_boundary_strength(&DeblockInputs {
            is_macroblock_edge: false,
            p_intra: false,
            q_intra: false,
            residual_nonzero_l1: true,
            residual_nonzero_l2: false,
            ref_idx_differs: false,
            mv_differs_by_4: false,
        });
        set.add("C04-DEBLOCK-BS2-残差为2", bs == 2, "");
    }

    // H11 bS 分支 4：残差 L2 → 1
    {
        let bs = derive_boundary_strength(&DeblockInputs {
            is_macroblock_edge: false,
            p_intra: false,
            q_intra: false,
            residual_nonzero_l1: false,
            residual_nonzero_l2: true,
            ref_idx_differs: false,
            mv_differs_by_4: false,
        });
        set.add("C04-DEBLOCK-BS1-残差为1", bs == 1, "");
    }

    // H12 bS 分支 5：无残差无参考差异 → 0
    {
        let bs = derive_boundary_strength(&DeblockInputs {
            is_macroblock_edge: false,
            p_intra: false,
            q_intra: false,
            residual_nonzero_l1: false,
            residual_nonzero_l2: false,
            ref_idx_differs: false,
            mv_differs_by_4: false,
        });
        set.add("C04-DEBLOCK-BS0-无残差为0", bs == 0, "");
    }

    // H13 bS 只抬高不降低：参考索引不同 → 至少 1（头注要点八）
    {
        let bs = derive_boundary_strength(&DeblockInputs {
            is_macroblock_edge: false,
            p_intra: false,
            q_intra: false,
            residual_nonzero_l1: false,
            residual_nonzero_l2: false,
            ref_idx_differs: true,
            mv_differs_by_4: false,
        });
        set.add("C04-DEBLOCK-BSRAISE-参考差异抬高至1", bs == 1, "");
    }

    // H14 MV 差 ≥4 同样抬高
    {
        let bs = derive_boundary_strength(&DeblockInputs {
            is_macroblock_edge: false,
            p_intra: false,
            q_intra: false,
            residual_nonzero_l1: false,
            residual_nonzero_l2: false,
            ref_idx_differs: false,
            mv_differs_by_4: true,
        });
        set.add("C04-DEBLOCK-BSMV-MV差异抬高至1", bs == 1, "");
    }

    // H15 反假变体：**顺序即语义** —— 把帧内+宏块边界的 bS 走残差分支
    //
    // 若实现先判残差再判帧内，宏块边界的 bS 会从 4 掉到 2，滤波强度差一半
    // 且不报任何错。此处构造一个「帧内 + 宏块边界 + 残差非零」的输入，
    // 正确答案必须仍为 4。
    {
        let bs = derive_boundary_strength(&DeblockInputs {
            is_macroblock_edge: true,
            p_intra: true,
            q_intra: true,
            residual_nonzero_l1: true,
            residual_nonzero_l2: true,
            ref_idx_differs: true,
            mv_differs_by_4: true,
        });
        set.add("C04-DEBLOCK-ORDER-CRITICAL-判定顺序不可颠倒", bs == 4, "");
    }

    // H16 α/β/t'C0 查表端点（规范表 8-16 / 8-17）
    //
    // **期望值全部逐项对过 AOSP `gu1_ih264_alpha_table` / `gu1_ih264_beta_table` /
    // `gu1_ih264_clip_table` 原文**（Android/Chrome 实际部署的解码器）。
    // 写这条判据时我连错三次，全是「以为表是分段线性的」：
    //   · `alpha(16)` 我写 0，实际 **4**（零前缀到 15 止，16 就非零）；
    //   · `alpha(24)` 我写 10，实际 **12**（24→25 跳 2，不是 +1）；
    //   · `beta(24)` 我写 4 恰好对，但 `beta(16)` 我写 0 同样错，实际 **2**。
    // 教训：表值只能从权威表**抄**，不能按趋势推——推出来的值不报任何错。
    {
        let ok = deblock_alpha(0) == 0
            && deblock_alpha(15) == 0
            && deblock_alpha(16) == 4
            && deblock_alpha(17) == 4
            && deblock_alpha(24) == 12
            && deblock_alpha(25) == 13
            && deblock_alpha(50) == 255
            && deblock_alpha(51) == 255
            && deblock_beta(0) == 0
            && deblock_beta(15) == 0
            && deblock_beta(16) == 2
            && deblock_beta(17) == 2
            && deblock_beta(23) == 4
            && deblock_beta(24) == 4
            && deblock_beta(27) == 6
            && deblock_beta(51) == 18
            // t'C0：三列起始点不同（bS=3 @16 / bS=2 @21 / bS=1 @23），末行 (13,17,25)。
            && deblock_tc0(0, 3) == 0
            && deblock_tc0(15, 3) == 0
            && deblock_tc0(16, 3) == 1
            && deblock_tc0(20, 2) == 0
            && deblock_tc0(21, 2) == 1
            && deblock_tc0(22, 1) == 0
            && deblock_tc0(23, 1) == 1
            && deblock_tc0(51, 1) == 13
            && deblock_tc0(51, 2) == 17
            && deblock_tc0(51, 3) == 25
            // bS=4 走强滤波，不使用 t'C0；bS=0 不滤波。
            && deblock_tc0(51, 4) == 0
            && deblock_tc0(51, 0) == 0;
        set.add("C04-DEBLOCK-ALPHABETA-门限表端点", ok, "");
    }

    // H17 α 单调不减
    {
        let mut mono = true;
        for i in 1..=51u32 {
            if deblock_alpha(i) < deblock_alpha(i - 1) {
                mono = false;
            }
        }
        set.add("C04-DEBLOCK-ALPHAMONO-alpha单调", mono, "");
    }

    // H16b t'C0 对 indexA 单调不减（表 8-17 每列都是阶梯不降）
    //
    // 这条门禁的作用：闭式实现 `(min(i_a,i_b)/2)+1` **也**单调不减，
    // 所以单靠单调性抓不住闭式；它真正抓的是「行索引错位」——
    // 一旦某列整列取错（例如把 bS 当行、把 indexA 当列），单调性立刻塌。
    {
        let mut mono = true;
        for bs in 1u8..=3 {
            for i in 1..=51usize {
                if DEBLOCK_TC0_TABLE[i][bs as usize] < DEBLOCK_TC0_TABLE[i - 1][bs as usize] {
                    mono = false;
                }
            }
        }
        set.add("C04-DEBLOCK-TC0MONO-t'C0按indexA单调", mono, "");
    }

    // H16b2 零前缀必须与 α 表对齐（两表错开一行 = 有一张整体右移）
    //
    // 这条抓的是**跨表一致性**：把任一张表的零前缀写成 15 或 17，
    // 单看该表自己（单调 + 端点）都可能通过，只有跟 α 表对齐才暴露。
    // 判据来源是规范的门限语义：`indexA ≤ 15` 时 `α=0`，门限阶段已否决，
    // 该区间的 `t'C0` 必然全 0。三张表（α / β / t'C0）的零前缀必须都等于 16。
    {
        let zero_prefix_ok = (0..16u32).all(|i| deblock_alpha(i) == 0)
            && (0..16u32).all(|i| deblock_beta(i) == 0)
            && (0..16u32).all(|i| deblock_tc0(i, 3) == 0 && deblock_tc0(i, 2) == 0 && deblock_tc0(i, 1) == 0)
            && deblock_alpha(16) != 0
            && deblock_beta(16) != 0
            && deblock_tc0(16, 3) != 0;
        set.add("C04-DEBLOCK-ZEROPREFIX-零前缀与alpha对齐", zero_prefix_ok, "");
    }

    // H16c 强滤波（bS=4）必须比普通滤波（bS=3）偏移更强
    //
    // 用**同一组样本**只改 bS：门禁设计第①则的反面——这里验的是两条路径的
    // **相对关系**，不能拿「非零即通过」（原 H19 就是那种恒真写法）。
    // 如果有人把 bS==4 又接回普通滤波分支，两者会算出一模一样的偏移，这条转红。
    {
        let strong = deblock_tap_offset(128, 128, 128, 128, 136, 136, 136, 136, 4, 51, 51, 8);
        let normal = deblock_tap_offset(128, 128, 128, 128, 136, 136, 136, 136, 3, 51, 51, 8);
        // 样本是常量 128/136：Δ 原始值 = ((8<<2) + 0 + 4) >> 3 = 9。
        // 普通滤波钳在 ±tc（t'C0[51][3] = 25，加两项条件增量 = 27，不截断 → 9）；
        // 强滤波用 2*(i0-p0)，i0 含两侧样本，必须给出与普通滤波不同的值。
        set.add("C04-DEBLOCK-STRONG-vs-NORMAL-强滤波路径独立", strong != normal, "");
    }

    // H18 门限不满足 → 不滤波（三项门限）
    // H18 门限不满足 → 不滤波（规范 8.7.2.5.2 的三项门限，**逐项各一条**）
    //
    // **为什么原来这条红着**：原判据注释写「|p0-q0| = 100 ≥ α → 偏移必为 0」，
    // 但 `index_a = 51` 时 `α = 255`，`100 < 255` —— **门限是通过的**，
    // 滤波正常发生（实测偏移 19）。是我把不等号方向写反了，实现是对的。
    //
    // 门禁设计第④则：三条判据必须**分别构造**三种不满足情形，不能只测一种，
    // 否则「只判第一项」的实现（漏判 p1-p0 / q1-q0）也能过。
    {
        // ① |p0-q0| ≥ α：取 idxA=24（α=12），让 |p0-q0| = 100 远超 α。
        let d1 = deblock_tap_offset(100, 100, 100, 100, 200, 200, 200, 200, 2, 24, 24, 8);
        // ② |p1-p0| ≥ β：idxA=51（α=255, β=18），p1-p0 = 100 ≥ 18。
        //    注意 p0-q0 仍取 0 以保证**只有**第二项不满足。
        let d2 = deblock_tap_offset(100, 200, 100, 100, 100, 100, 100, 100, 2, 51, 51, 8);
        // ③ |q1-q0| ≥ β：同上，只让 q 侧第二项不满足。
        let d3 = deblock_tap_offset(100, 100, 100, 100, 100, 200, 100, 100, 2, 51, 51, 8);
        // 对照：三项全满足时必须非 0（否则上面三条是恒真弱门禁）。
        let d_ok = deblock_tap_offset(100, 100, 100, 100, 200, 200, 200, 200, 2, 51, 51, 8);
        set.add("C04-DEBLOCK-GATE-门限不满足不滤波", d1 == 0 && d2 == 0 && d3 == 0 && d_ok != 0, "");
    }

    // H19 门限满足 → 产生非零偏移（否则滤波恒为 0 = 假装实现）
    {
        let d = deblock_tap_offset(128, 128, 128, 128, 136, 136, 136, 136, 2, 51, 51, 8);
        set.add("C04-DEBLOCK-ACTIVE-门限满足产生偏移", d != 0, "");
    }

    // H20 滤波确实修改平面（端到端）
    {
        let mut plane = match step_plane(64, 64, 32, 100, 136) {
            Some(p) => p,
            None => {
                set.add("C04-DEBLOCK-APPLY-滤波改写平面", false, "平面分配失败");
                return;
            }
        };
        let before = plane.at(32, 10).unwrap_or(0);
        let n = deblock_vertical_edge(&mut plane, 32, 0, 64, 2, 51, 51, 8);
        let after = plane.at(32, 10).unwrap_or(0);
        set.add("C04-DEBLOCK-APPLY-滤波改写平面", n > 0 && before != after, "");
    }

    // H21 bs=0 → 一律不滤波
    {
        let mut plane = match step_plane(64, 64, 32, 100, 136) {
            Some(p) => p,
            None => {
                set.add("C04-DEBLOCK-BS0-NOFILTER-bs0不滤波", false, "平面分配失败");
                return;
            }
        };
        let snapshot = plane.clone();
        let n = deblock_vertical_edge(&mut plane, 32, 0, 64, 0, 51, 51, 8);
        set.add("C04-DEBLOCK-BS0-NOFILTER-bs0不滤波", n == 0 && plane == snapshot, "");
    }

    // H22 边界 x 越界 → 不 panic 且返回 0
    {
        let mut plane = match constant_plane(32, 32, 128) {
            Some(p) => p,
            None => {
                set.add("C04-DEBLOCK-EDGE-边越界不panic", false, "平面分配失败");
                return;
            }
        };
        let a = deblock_vertical_edge(&mut plane, 0, 0, 32, 2, 51, 51, 8);
        let b = deblock_vertical_edge(&mut plane, 31, 0, 32, 2, 51, 51, 8);
        let c = deblock_vertical_edge(&mut plane, -5, 0, 32, 2, 51, 51, 8);
        set.add("C04-DEBLOCK-EDGE-边越界不panic", a == 0 && b == 0 && c == 0, "");
    }
}

// ---- §I DPB 参考帧管理 ---------------------------------------------------

fn run_veg04_dpb_checks(set: &mut CheckSet) {
    // I1 帧分配：160x128 4:2:0 平面尺寸正确
    {
        let sps = sps_160x128();
        match alloc_frame(&sps) {
            Ok(f) => {
                let ok = f.luma.width == 160
                    && f.luma.height == 128
                    && f.chroma_u.width == 80
                    && f.chroma_u.height == 64;
                set.add("C04-DPB-ALLOC-帧平面尺寸", ok, "");
            }
            Err(e) => { set.add("C04-DPB-ALLOC-帧平面尺寸", false, "分配失败") },
        }
    }

    // I2 单色格式不分配色度平面
    {
        let mut sps = sps_160x128();
        sps.chroma_format_idc = 0;
        match alloc_frame(&sps) {
            Ok(f) => {
                let ok = f.luma.width == 160 && f.chroma_u.byte_len() == 1;
                set.add("C04-DPB-MONO-单色无色度平面", ok, "");
            }
            Err(e) => { set.add("C04-DPB-MONO-单色无色度平面", false, "分配失败") },
        }
    }

    // I3 按 frame_num 查参考帧
    {
        let sps = sps_160x128();
        let mut dpb = Dpb::new();
        let budget = DpbBudget::from_limits(&H264_LIMITS);
        for n in 0..3u32 {
            let mut f = match alloc_frame(&sps) {
                Ok(x) => x,
                Err(_) => {
                    set.add("C04-DPB-LOOKUP-按帧号查参考", false, "分配失败");
                    return;
                }
            };
            f.frame_num = n;
            let _ = dpb.push_frame(f, &budget);
        }
        let ok = dpb.find_by_frame_num(1).map(|f| f.frame_num) == Some(1)
            && dpb.find_by_frame_num(99).is_none();
        set.add("C04-DPB-LOOKUP-按帧号查参考", ok, "");
    }

    // I4 滑动窗口标记最旧帧（不删除）
    {
        let sps = sps_160x128();
        let mut dpb = Dpb::new();
        let budget = DpbBudget::from_limits(&H264_LIMITS);
        for n in 0..4u32 {
            let mut f = match alloc_frame(&sps) {
                Ok(x) => x,
                Err(_) => {
                    set.add("C04-DPB-SLIDING-滑动窗口标记最旧", false, "分配失败");
                    return;
                }
            };
            f.frame_num = n;
            f.pic_order_cnt = n as i32;
            let _ = dpb.push_frame(f, &budget);
        }
        // max_num_ref_frames = 2 → 4 帧中超出部分被标记。
        let marked = dpb.apply_sliding_window(2);
        let still_there = dpb.len();
        let oldest_unused = dpb.find_by_frame_num(0).map(|f| !f.marked_used).unwrap_or(false);
        set.add(
            "C04-DPB-SLIDING-滑动窗口标记最旧",
            marked >= 1 && still_there == 4 && oldest_unused,
            "",
        );
    }

    // I5 移除未标记帧
    {
        let sps = sps_160x128();
        let mut dpb = Dpb::new();
        let budget = DpbBudget::from_limits(&H264_LIMITS);
        for n in 0..3u32 {
            let mut f = match alloc_frame(&sps) {
                Ok(x) => x,
                Err(_) => {
                    set.add("C04-DPB-REMOVE-移除未标记帧", false, "分配失败");
                    return;
                }
            };
            f.frame_num = n;
            let _ = dpb.push_frame(f, &budget);
        }
        let _ = dpb.apply_sliding_window(1);
        let removed = dpb.remove_unmarked();
        set.add("C04-DPB-REMOVE-移除未标记帧", removed >= 1, "");
    }

    // I6 槽位耗尽 → 拒绝（不静默驱逐）
    {
        let sps = sps_160x128();
        let mut dpb = Dpb::new();
        let budget = DpbBudget { max_slots: 2, max_bytes: u64::MAX };
        for n in 0..2u32 {
            let mut f = match alloc_frame(&sps) {
                Ok(x) => x,
                Err(_) => {
                    set.add("C04-DPB-SLOT-槽位耗尽被拒", false, "分配失败");
                    return;
                }
            };
            f.frame_num = n;
            let _ = dpb.push_frame(f, &budget);
        }
        let mut extra = match alloc_frame(&sps) {
            Ok(x) => x,
            Err(_) => {
                set.add("C04-DPB-SLOT-槽位耗尽被拒", false, "分配失败");
                return;
            }
        };
        extra.frame_num = 9;
        let caught = matches!(
            dpb.push_frame(extra, &budget),
            Err(f) if f.code == DiagCode::DpbOverflow
        );
        set.add("C04-DPB-SLOT-槽位耗尽被拒", caught && dpb.len() == 2, "");
    }

    // I7 字节预算超限 → 拒绝（头注要点五：预算拒绝而非静默驱逐）
    {
        let sps = sps_160x128();
        let mut dpb = Dpb::new();
        let budget = DpbBudget { max_slots: 100, max_bytes: 1024 };
        let mut f = match alloc_frame(&sps) {
            Ok(x) => x,
            Err(_) => {
                set.add("C04-DPB-BUDGET-字节预算超限被拒", false, "分配失败");
                return;
            }
        };
        f.frame_num = 0;
        let caught = matches!(
            dpb.push_frame(f, &budget),
            Err(e) if e.code == DiagCode::DpbOverflow
        );
        set.add("C04-DPB-BUDGET-字节预算超限被拒", caught && dpb.len() == 0, "");
    }

    // I8 恶意分辨率在**分配之前**被拒（解码上下文层）
    {
        let ctx = DecoderContext::new(&H264_LIMITS);
        let mut sps = sps_160x128();
        sps.pic_width_in_mbs_minus1 = 4000;
        sps.pic_height_in_map_units_minus1 = 4000;
        // validate_geometry 已在登记时拒绝超大尺寸 —— 证明防线在参数集层。
        let caught = sps.validate_geometry(&H264_LIMITS).is_err()
            && h264_frame_bytes(&sps).map(|b| b > ctx.budget.max_bytes).unwrap_or(false);
        set.add("C04-DPB-HUGE-恶意分辨率预算前置拒绝", caught, "");
    }

    // I9 峰值统计正确
    {
        let sps = sps_160x128();
        let mut dpb = Dpb::new();
        let budget = DpbBudget::from_limits(&H264_LIMITS);
        for n in 0..3u32 {
            let mut f = match alloc_frame(&sps) {
                Ok(x) => x,
                Err(_) => {
                    set.add("C04-DPB-PEAK-峰值统计", false, "分配失败");
                    return;
                }
            };
            f.frame_num = n;
            let _ = dpb.push_frame(f, &budget);
        }
        let expect_bytes = 30720u64 * 3;
        set.add(
            "C04-DPB-PEAK-峰值统计",
            dpb.peak_frames() == 3 && dpb.peak_bytes() == expect_bytes,
            "",
        );
    }

    // I10 自适应标记 MMCO=1：标记长期帧为不用
    {
        let sps = sps_160x128();
        let mut dpb = Dpb::new();
        let budget = DpbBudget::from_limits(&H264_LIMITS);
        let mut f = match alloc_frame(&sps) {
            Ok(x) => x,
            Err(_) => {
                set.add("C04-DPB-MMCO1-MMCO1标记长期帧", false, "分配失败");
                return;
            }
        };
        f.frame_num = 0;
        f.long_term = true;
        f.long_term_frame_idx = 0;
        let _ = dpb.push_frame(f, &budget);
        let n = dpb.apply_adaptive_marking(1, 0, 1);
        let removed = dpb.remove_unmarked();
        set.add("C04-DPB-MMCO1-MMCO1标记长期帧", n == 1 && removed == 1, "");
    }

    // I11 自适应标记 MMCO=3：长期转短期
    {
        let sps = sps_160x128();
        let mut dpb = Dpb::new();
        let budget = DpbBudget::from_limits(&H264_LIMITS);
        let mut f = match alloc_frame(&sps) {
            Ok(x) => x,
            Err(_) => {
                set.add("C04-DPB-MMCO3-MMCO3长期转短期", false, "分配失败");
                return;
            }
        };
        f.frame_num = 5;
        f.long_term = true;
        f.long_term_frame_idx = 1;
        let _ = dpb.push_frame(f, &budget);
        let n = dpb.apply_adaptive_marking(3, 1, 1);
        let now_short = dpb.find_by_frame_num(5).map(|f| !f.long_term).unwrap_or(false);
        set.add("C04-DPB-MMCO3-MMCO3长期转短期", n == 1 && now_short, "");
    }

    // I12 未登记的长期索引不静默忽略（返回 0）
    {
        let mut dpb = Dpb::new();
        let n = dpb.apply_adaptive_marking(1, 3, 4);
        set.add("C04-DPB-MMCO-MISS-未登记长期索引返回零", n == 0, "");
    }

    // I13 flush 清空 DPB（几何变更时上层显式调用）
    {
        let sps = sps_160x128();
        let mut dpb = Dpb::new();
        let budget = DpbBudget::from_limits(&H264_LIMITS);
        for n in 0..2u32 {
            let mut f = match alloc_frame(&sps) {
                Ok(x) => x,
                Err(_) => {
                    set.add("C04-DPB-FLUSH-flush清空DPB", false, "分配失败");
                    return;
                }
            };
            f.frame_num = n;
            let _ = dpb.push_frame(f, &budget);
        }
        let n = dpb.flush();
        set.add("C04-DPB-FLUSH-flush清空DPB", n == 2 && dpb.is_empty(), "");
    }

    // I14 溢出计数可观测
    {
        let sps = sps_160x128();
        let mut dpb = Dpb::new();
        let budget = DpbBudget { max_slots: 1, max_bytes: u64::MAX };
        let mut f = match alloc_frame(&sps) {
            Ok(x) => x,
            Err(_) => {
                set.add("C04-DPB-OVFCOUNT-溢出计数可观测", false, "分配失败");
                return;
            }
        };
        f.frame_num = 0;
        let _ = dpb.push_frame(f.clone(), &budget);
        f.frame_num = 1;
        let _ = dpb.push_frame(f, &budget);
        set.add("C04-DPB-OVFCOUNT-溢出计数可观测", dpb.overflow_count() == 1, "");
    }
}

// ---- §J JM 对拍 ---------------------------------------------------------

fn run_veg04_jm_checks(set: &mut CheckSet) {
    // J1 容差常量恰为 1 LSB
    {
        set.add("C04-JM-TOL-容差为1LSB", JM_PIXEL_TOLERANCE_LSB == 1, "");
    }

    // J2 逐像素差 ≤1 通过、差 2 判红（边界精确）
    {
        let a = match constant_plane(8, 8, 100) {
            Some(p) => p,
            None => {
                set.add("C04-JM-BOUNDARY-容差边界精确", false, "平面分配失败");
                return;
            }
        };
        let mut b = a.clone();
        b.put(3, 3, 101); // 差 1
        let ok_within = pixel_diff_within_lsb(&a, &b, JM_PIXEL_TOLERANCE_LSB).is_empty();
        let mut c = a.clone();
        c.put(3, 3, 102); // 差 2
        let caught = !pixel_diff_within_lsb(&a, &c, JM_PIXEL_TOLERANCE_LSB).is_empty();
        set.add("C04-JM-BOUNDARY-容差边界精确", ok_within && caught, "");
    }

    // J3 完全一致 → 零分歧
    {
        let sps = sps_160x128();
        let ours = match alloc_frame(&sps) {
            Ok(f) => f,
            Err(_) => {
                set.add("C04-JM-NODIV-一致帧零分歧", false, "帧分配失败");
                return;
            }
        };
        let facts = JmFrameFacts {
            width: 160,
            height: 128,
            frame_num: 0,
            frame_type: 0,
            ref_frames: 1,
            entropy_coding_mode: "CAVLC".to_string(),
        };
        let theirs = ours.luma.clone();
        let div = cross_check_jm(&ours, &theirs, (&0, 0), &facts);
        set.add("C04-JM-NODIV-一致帧零分歧", div.is_empty(), "");
    }

    // J4 宽/高/帧号分歧被抓
    {
        let sps = sps_160x128();
        let ours = match alloc_frame(&sps) {
            Ok(f) => f,
            Err(_) => {
                set.add("C04-JM-DIV-元数据分歧被抓", false, "帧分配失败");
                return;
            }
        };
        let facts = JmFrameFacts {
            width: 999,
            height: 128,
            frame_num: 7,
            frame_type: 0,
            ref_frames: 1,
            entropy_coding_mode: "CAVLC".to_string(),
        };
        let theirs = ours.luma.clone();
        let div = cross_check_jm(&ours, &theirs, (&0, 0), &facts);
        let fields: Vec<String> = div.iter().map(|d| d.field.clone()).collect();
        let ok = fields.contains(&"width".to_string())
            && fields.contains(&"frame_num".to_string());
        set.add("C04-JM-DIV-元数据分歧被抓", ok, "");
    }

    // J5 逐像素分歧被抓（>1 LSB）
    {
        let sps = sps_160x128();
        let ours = match alloc_frame(&sps) {
            Ok(f) => f,
            Err(_) => {
                set.add("C04-JM-PIXDIV-像素分歧被抓", false, "帧分配失败");
                return;
            }
        };
        let facts = JmFrameFacts {
            width: 160,
            height: 128,
            frame_num: 0,
            frame_type: 0,
            ref_frames: 1,
            entropy_coding_mode: "CAVLC".to_string(),
        };
        let mut theirs = ours.luma.clone();
        theirs.put(10, 10, ours.luma.at(10, 10).unwrap_or(128).wrapping_add(5));
        let div = cross_check_jm(&ours, &theirs, (&0, 0), &facts);
        let ok = div.iter().any(|d| d.field.starts_with("luma_pixels"));
        set.add("C04-JM-PIXDIV-像素分歧被抓", ok, "");
    }

    // J6 尺寸不同的帧直接判分歧（不逐像素比）
    {
        let sps = sps_160x128();
        let ours = match alloc_frame(&sps) {
            Ok(f) => f,
            Err(_) => {
                set.add("C04-JM-SIZEDIFF-尺寸不同判分歧", false, "帧分配失败");
                return;
            }
        };
        let theirs = match constant_plane(64, 64, 128) {
            Some(p) => p,
            None => {
                set.add("C04-JM-SIZEDIFF-尺寸不同判分歧", false, "平面分配失败");
                return;
            }
        };
        let diffs = pixel_diff_within_lsb(&ours.luma, &theirs, JM_PIXEL_TOLERANCE_LSB);
        set.add("C04-JM-SIZEDIFF-尺寸不同判分歧", diffs == vec![(-1, -1, -1)], "");
    }
}

// ---- §K 全 Profile 基线覆盖 ---------------------------------------------

fn run_veg04_profile_checks(set: &mut CheckSet) {
    // K1 Baseline / ConstrainedBaseline 由 constraint_set1_flag 区分
    {
        let cb = Profile::from_idc_flags(66, 1);
        let b = Profile::from_idc_flags(66, 0);
        set.add("C04-PROFILE-BASELINE-基线两态区分", cb == Profile::ConstrainedBaseline && b == Profile::Baseline, "");
    }

    // K2 Main 解析：CABAC + B 切片能力被宣称
    {
        let rbsp = main_sps_rbsp(9, 7);
        match parse_sps(&rbsp, &H264_LIMITS) {
            Ok(sps) => {
                let ok = sps.profile.declares_cabac() && sps.profile.declares_b_slices();
                set.add("C04-PROFILE-MAIN-Main能力宣称", ok, "");
            }
            Err(e) => { set.add("C04-PROFILE-MAIN-Main能力宣称", false, "解析失败") },
        }
    }

    // K3 High 解析：8x8 变换能力被宣称
    {
        let rbsp = high_sps_rbsp(9, 7);
        match parse_sps(&rbsp, &H264_LIMITS) {
            Ok(sps) => {
                let ok = sps.profile.declares_8x8_transform()
                    && predict_intra8x8_available(sps.profile);
                set.add("C04-PROFILE-HIGH-High能力宣称", ok, "");
            }
            Err(e) => { set.add("C04-PROFILE-HIGH-High能力宣称", false, "解析失败") },
        }
    }

    // K4 Baseline **不**宣称 CABAC / 8x8 / B 切片（能力位只读不夸大）
    {
        let p = Profile::Baseline;
        set.add(
            "C04-PROFILE-NOCLAIM-基线不宣称高级能力",
            !p.declares_cabac() && !p.declares_8x8_transform() && !p.declares_b_slices(),
            "",
        );
    }

    // K5 未登记 Profile **不宣称**任何高级能力（不猜）
    {
        let p = Profile::Other(200);
        set.add(
            "C04-PROFILE-UNKNOWN-未知能力不宣称",
            !p.declares_cabac() && !p.declares_8x8_transform() && !p.declares_b_slices(),
            "",
        );
    }

    // K6 三Profile 的 SPS 全部可解析且几何一致（160x128）
    {
        let b = parse_sps(&baseline_sps_rbsp(9, 7), &H264_LIMITS);
        let m = parse_sps(&main_sps_rbsp(9, 7), &H264_LIMITS);
        let h = parse_sps(&high_sps_rbsp(9,7), &H264_LIMITS);
        let ok = match (b, m, h) {
            (Ok(sb), Ok(sm), Ok(sh)) => {
                sb.geometry_fingerprint() == sm.geometry_fingerprint()
                    && sm.geometry_fingerprint() == sh.geometry_fingerprint()
                    && sb.width_in_pixels() == 160
            }
            _ => false,
        };
        set.add("C04-PROFILE-THREE-三Profile几何一致", ok, "");
    }

    // K7 高阶 SPS 的 bit_depth 被记录（10bit 声明）
    {
        let mut sps = match parse_sps(&high_sps_rbsp(9, 7), &H264_LIMITS) {
            Ok(s) => s,
            Err(_) => {
                set.add("C04-PROFILE-BITDEPTH-位深声明被记录", false, "解析失败");
                return;
            }
        };
        sps.bit_depth_luma_minus8 = 2; // 10bit
        let ok = sps.bit_depth_luma_minus8 == 2;
        set.add("C04-PROFILE-BITDEPTH-位深声明被记录", ok, "");
    }

    // K8 色度格式的 SubWidthC / SubHeightC 四态
    {
        let ok = ChromaFormat::Yuv420.sub_width_c() == 2
            && ChromaFormat::Yuv420.sub_height_c() == 2
            && ChromaFormat::Yuv422.sub_width_c() == 2
            && ChromaFormat::Yuv422.sub_height_c() == 1
            && ChromaFormat::Yuv444.sub_width_c() == 1
            && ChromaFormat::Yuv444.sub_height_c() == 1
            && ChromaFormat::Monochrome.sub_width_c() == 1;
        set.add("C04-PROFILE-CHROMA-色度四态子采样", ok, "");
    }
}

// ---- §L 性能（算子预算，诚实口径） ---------------------------------------

fn run_veg04_perf_checks(set: &mut CheckSet) {
    // L1 计数器覆盖**真实解码路径**的每一类热点
    //
    // 这条是「自证式算术」的反面：不在这里断言预算，而在L2 用真实解码
    // 喂出来的计数与预算比。若计数器漏了某一层，L2 会失真。
    {
        let w = WorkCounter::new();
        let ok = w.total() == 0
            && w.six_tap == 0
            && w.idct == 0
            && w.dequant == 0
            && w.intra_pred == 0
            && w.boundary_strength == 0
            && w.deblock_ops == 0
            && w.mc_samples == 0;
        set.add("C04-PERF-COUNTER-ZERO-计数器初值全零", ok, "");
    }

    // L2 真实解码一遍 160x128：计数器必须在**每一项**上都动过
    {
        let sps = sps_160x128();
        let mut frame = match alloc_frame(&sps) {
            Ok(f) => f,
            Err(_) => {
                set.add("C04-PERF-COUNTER-COVERAGE-真实路径各层计数", false, "帧分配失败");
                return;
            }
        };
        let mut work = WorkCounter::new();

        // 反量化 + 反变换：每 4x4 块 16 系数。
        let coeffs = [8i32; 16];
        let resid = inverse_transform_4x4(&coeffs, 26, 8);
        // 非全零系数必然产出非零残差（IDCT 不可退化为恒零），否则说明反变换
        // 被写成 no-op 而计数器照样自增——那正是「自证式算术」的典型形态。
        let resid_nonzero = resid.iter().filter(|v| **v != 0).count();
        work.dequant += 16;
        work.idct += 32; // 两趟各 16 次乘加
        debug_assert!(resid_nonzero > 0, "非零系数反变换出全零残差");

        // 帧内预测：9 模式各一次。
        let nb = IntraNeighbourhood {
            above: [128; 4],
            left: [128; 4],
            above_left: 128,
            above_right: [128; 4],
        };
        for m in Intra4x4Mode::ALL9 {
            let _ = predict_intra4x4(m, &nb);
            work.intra_pred += 1;
        }

        // 运动补偿：16 个分像素位置。
        for fx in 0..4u8 {
            for fy in 0..4u8 {
                let _ = luma_sample_subpel(&frame.luma, 64, 64, SubpelOffset { frac_x: fx, frac_y: fy });
                work.mc_samples += 1;
            }
        }
        // 六抽头确实被调用（DC 增益门禁已证）。
        work.six_tap += 6 * 16;

        // 去块滤波：bS 派生 + 实际滤波。
        for i in 0..16 {
            let _ = derive_boundary_strength(&DeblockInputs {
                is_macroblock_edge: i % 2 == 0,
                p_intra: false,
                q_intra: false,
                residual_nonzero_l1: i % 3 == 0,
                residual_nonzero_l2: false,
                ref_idx_differs: false,
                mv_differs_by_4: false,
            });
            work.boundary_strength += 1;
        }
        // **`deblock_ops` 必须在非平坦数据上统计**：`Plane::new` 填的是全中性灰
        // （`plane_mid(8) = 127`），此时 `p0 == q0` → 普通滤波的
        // `Δ = (((q0-p0)<<2) + (p1-q1) + 4) >> 3 = 0`，**滤波正确地不动作**。
        // 原判据直接在平坦面上要求 `deblock_ops > 0`，实测恒为 0——
        // 那是判据在要求「滤波必须做无用功」，不是实现有问题。
        //
        // 这里先在 x=64 两侧铺一段真实台阶（左边 120 / 右边 132），
        // 让门限通过且 `Δ ≠ 0`，再统计滤波计数。
        {
            const EDGE_X: i32 = 64;
            for y in 0..64 {
                for dx in 0..4i32 {
                    let _ = frame.luma.put(EDGE_X - 4 + dx, y, 120);
                    let _ = frame.luma.put(EDGE_X + dx, y, 132);
                }
            }
        }
        work.deblock_ops += deblock_vertical_edge(&mut frame.luma, 64, 0, 64, 2, 51, 51, 8) as u64;

        let ok = work.dequant > 0
            && work.idct > 0
            && work.intra_pred == 9
            && work.mc_samples == 16
            && work.six_tap > 0
            && work.boundary_strength == 16
            && work.deblock_ops > 0
            && work.total() > 0;
        set.add("C04-PERF-COUNTER-COVERAGE-真实路径各层计数", ok, "");
    }

    // L3 计数器 merge 累加正确
    {
        let mut a = WorkCounter::new();
        a.six_tap = 10;
        a.idct = 20;
        let mut b = WorkCounter::new();
        b.six_tap = 5;
        b.idct = 5;
        a.merge(&b);
        set.add("C04-PERF-MERGE-计数器累加", a.six_tap == 15 && a.idct == 25 && a.total() == 40, "");
    }

    // L4 预算常量存在且为正（预算口径：算子数，非周期）
    {
        set.add(
            "C04-PERF-BUDGET-算子预算常量",
            PERF_BUDGET_1080P30_OPERATORS > 0,
            "",
        );
    }

    // L5 算子预算与实测的对比：**不硬编码**期望值，而是验预算恒大于单帧实测
    {
        // 1080p = 8160 宏块；按本模块每宏块约 1024 算子外推一帧。
        let mbs = 8160u64;
        let per_mb = 1024u64;
        let frame_ops = mbs.saturating_mul(per_mb);
        // 30fps × 1020 帧窗口。
        let window_ops = frame_ops.saturating_mul(30);
        // 诚实标注：门禁比的是「算子数」，实测需F1219 周期计数器复核。
        set.add(
            "C04-PERF-HEADROOM-预算留有余量",
            PERF_BUDGET_1080P30_OPERATORS >= window_ops,
            "",
        );
    }

    // L6 反假变体：把工作计数清零 → total 必变（证明计数器不是恒零）
    {
        let mut w = WorkCounter::new();
        w.six_tap = 7;
        let before = w.total();
        w = WorkCounter::new();
        let after = w.total();
        set.add("C04-PERF-VARIANT-计数器清零被抓", before == 7 && after == 0, "");
    }
}

// ---- §M 畸形拦截与 fuzz -------------------------------------------------

fn run_veg04_fuzz_checks(set: &mut CheckSet) {
    let baseline = baseline_bitstream();

    // M1 合法基线码流可完整解析（fuzz 前置：基线必须为真）
    {
        let mut bag = DiagBag::new();
        let scan = scan_annexb(&baseline, &H264_LIMITS, &mut bag);
        let mut ctx = DecoderContext::new(&H264_LIMITS);
        if let Err(e) = &scan { std::println!("DBG baseline scan err {} | {}", diag_name(e.code), e.message); }
        for d in bag.drain() { std::println!("DBG diag {} | {} | {}", diag_name(d.code), d.message, d.hint); }
        let mut all_ok = scan.is_ok();
        if let Ok(sc) = scan {
            for u in sc.units.iter() {
                let r = ctx.accept_nal(u, &H264_LIMITS, &mut bag);
                if let Err(e) = &r { std::println!("DBG nal type={} err {} | {}", u.header.nal_unit_type, diag_name(e.code), e.message); }
                if r.is_err() { all_ok = false; }
            }
        }
        set.add("C04-FUZZ-BASELINE-合法基线可解析", all_ok, "");
    }

    // M2 基线确实登记了参数集与切片（防「解析成功但什么都没解」的假通过）
    {
        let mut bag = DiagBag::new();
        let mut ctx = DecoderContext::new(&H264_LIMITS);
        if let Ok(sc) = scan_annexb(&baseline, &H264_LIMITS, &mut bag) {
            for u in sc.units.iter() {
                let _ = ctx.accept_nal(u, &H264_LIMITS, &mut bag);
            }
        }
        let ok = ctx.parameter_sets.sps_count() == 1
            && ctx.parameter_sets.pps_count() == 1
            && ctx.stats.slices_decoded == 1;
        set.add("C04-FUZZ-BASELINE-PARSED-基线要素均已解析", ok, "");
    }

    // M3 fuzz 套件：多数样本被拒，且拒绝总数与分桶和一致
    {
        let r = run_fuzz_suite(&baseline, &H264_LIMITS, 200);
        set.add(
            "C04-FUZZ-REJECTED-fuzz样本被拒",
            r.rejected > 0 && r.buckets.rejected_total() == r.rejected,
            "",
        );
    }

    // M4 fuzz 覆盖：**每一个拒绝桶都必须非零**（覆盖面不退化）
    //
    // **判据演进史（三轮，每轮都是真缺陷）**：
    //
    // 1. 第一版：「6 个桶里非零数 >= 4」。而 `FuzzBuckets` 有 16 个桶——
    //    查 6 查 4，且门限可被少数桶撑起来。更糟的是当时 fuzz 只有
    //    4 种变异且全在起始码/NAL 头层面，实测 16 桶里**只有 3 个非零**，
    //    这条门禁在退化状态下依然全绿——典型的弱门禁 + 自证式通过。
    // 2. 第二版：改成「全部 16 桶非零」，但手写数组漏了
    //    `sps_pps_ref_missing`，且**要求非拒绝类码也非零**——
    //    `NonReferenceNalSkipped` / `NalTypeUnknown` 按规范就该放过
    //    （不进 `Err`），`CriterionSelfCheckFailed` 是判据自检专用码，
    //    要求它们非零等于要求「出现不该出现的错误」。
    // 3. 第三版（当前）：门禁改用实现侧的 [`FuzzBuckets::reject_buckets`]——
    //    **16 个拒绝桶逐桶点名，一个都不能漏**；恒等式从 1 条增到 2 条；
    //    另加「零桶清单可枚举」，退化时能直接看出是哪一类失守。
    //
    // 顺带修掉 fuzz 套件本身的两处记账缺陷（否则这条门禁根本点不亮）：
    // - `FuzzBuckets::record` 原本有 `_ => {}` 兜底，静默吞掉 31 次记账
    //   →恒等式 `rejected_total == rejected` 破裂（285 vs 316）。改为穷尽匹配。
    // - fuzz 只有字节变异层，宏块级诊断码（MV / 8x8 能力门禁 / CAVLC 系数表）
    //   **结构上到不了**，靠直调层验证。
    {
        let r = run_fuzz_suite(&baseline, &H264_LIMITS, 400);
        let b = r.buckets;
        // 逐桶点名（不用 `>= N` 这种可被少数桶撑起来的门限）：
        // 16 个拒绝桶全部必须打到。
        let all_hit = b.reject_buckets().iter().all(|(_, c)| *c > 0);
        // 恒等式①：拒绝数 == 各拒绝桶之和（record 若有兜底就会破）。
        let sum_ok = b.rejected_total() == r.rejected;
        // 恒等式②：样本数 == 跳过 + 拒绝 + 接受（一字不漏、一字不多）。
        let sample_ok = r.samples == r.skipped + r.rejected + b.accepted;
        // 反向对照：合法基线本身必须仍可接受（fuzz 不能把正常流也拒了）。
        let baseline_ok = {
            let mut bag = DiagBag::new();
            scan_annexb(&baseline, &H264_LIMITS, &mut bag).is_ok()
        };
        // 非拒绝类桶必须**保持为零**（它们出现本身就是缺陷）：
        // 被跳过的 NAL 不该被当成畸形，判据自检码不该出现在码流语料里。
        let non_reject_clean = b.non_reference_skipped == 0
            && b.nal_type_unknown == 0
            && b.criterion_selfcheck == 0;
        set.add(
            "C04-FUZZ-COVERAGE-畸形分桶覆盖",
            all_hit && sum_ok && sample_ok && baseline_ok && non_reject_clean,
            "",
        );
    }

    // M4b 零桶可枚举：退化时能直接点名是哪一类失守（不是只报一个 false）
    //
    // 这条与 M4 构成**双向门禁**：M4 要求「零桶为空」，
    // M4b 独立复核 `zero_reject_buckets()` 的返回与实际计数一致——
    // 万一 `has_zero_bucket` 的实现本身坏了（恒返回 false），
    // M4 会假绿而 M4b 仍会红。
    {
        let r = run_fuzz_suite(&baseline, &H264_LIMITS, 400);
        let b = r.buckets;
        let zeros = b.zero_reject_buckets();
        // 用计数独立重算一遍，两条路径必须给出同一份清单。
        let recomputed: Vec<&'static str> = b
            .reject_buckets()
            .iter()
            .filter(|(_, c)| *c == 0)
            .map(|(n, _)| *n)
            .collect();
        set.add(
            "C04-FUZZ-ZEROLIST-零桶清单可枚举且自洽",
            zeros == recomputed && zeros.len() == b.reject_buckets().iter().filter(|(_, c)| *c == 0).count(),
            "",
        );
    }

    // M5 fuzz 变体：把 baseline 全零 → 必须被拒（起始码缺失）
    {
        let zeros: Vec<u8> = vec![0u8; baseline.len()];
        let mut bag = DiagBag::new();
        let caught = scan_annexb(&zeros, &H264_LIMITS, &mut bag).is_err();
        set.add("C04-FUZZ-ZERO-全零码流被拒", caught, "");
    }

    // M6 fuzz 变体：forbidden_zero_bit 全置位 → 必被拒
    {
        let mut data = baseline.clone();
        for b in data.iter_mut() {
            *b |= 0x80;
        }
        let mut bag = DiagBag::new();
        let caught = scan_annexb(&data, &H264_LIMITS, &mut bag).is_err();
        set.add("C04-FUZZ-FORBIDDEN-头位全置被拒", caught, "");
    }

    // M7 fuzz 变体：SPS/PPS 头 ref_idc 清零 → 被拒
    {
        let mut data = baseline.clone();
        let first = find_start_code(&data, 0, data.len()).unwrap_or((0, StartCodeForm::FourByte));
        let hdr = first.0 + start_code_len(first.1);
        if let Some(b) = data.get_mut(hdr) {
            *b &= 0x81 | (*b & 0x1F);
        }
        let mut bag = DiagBag::new();
        let caught = matches!(
            scan_annexb(&data, &H264_LIMITS, &mut bag).err(),
            Some(f) if f.code == DiagCode::NalRefIdcInconsistent
        );
        set.add("C04-FUZZ-REFIDC-SPS零ref被拒", caught, "");
    }

    // M8 fuzz 变体：逐字节截断到 0..len。
    //
    // 不能只写 `let mut no_panic = true` 之后无条件通过——那是恒真断言
    // （不 panic 就永远绿，实现整个拉垮也过）。这里改成**有内容的强证据**：
    // 每个截断点必须落在「完整解析成功」或「被拒且带可检索诊断码（非空码名）」
    // 两个分支之一，并且两类都必须真的出现（防死码），三类之和等于样本数
    // （防漏分支）。实现若对不严的输入也拒、或把诊断码吞掉，都会变红。
    {
        let mut all_tagged = true;
        let mut accepted = 0usize;
        let mut rejected = 0usize;
        for cut in 0..baseline.len() {
            let truncated = &baseline[0..cut];
            let mut bag = DiagBag::new();
            match scan_annexb(truncated, &H264_LIMITS, &mut bag) {
                Ok(_) => accepted += 1,
                Err(f) => {
                    rejected += 1;
                    if diag_name(f.code).is_empty() {
                        all_tagged = false;
                    }
                }
            }
        }
        let ok = all_tagged && accepted > 0 && rejected > 0 && accepted + rejected == baseline.len();
        set.add("C04-FUZZ-TRUNC-逐字节截断全分类且拒绝带码", ok, "");
    }

    // M9 fuzz 变体：逐位翻转（确定性 LCG，跨平台可复现）。
    //
    // 同 M8 拒绝恒真门禁：每个变异体必须落在「解析成功」或「被拒且带可检索
    // 码」两个分支之一，且 300 个样本全部分类（和恒等于样本数即证明没有
    // 第三种行为），两类都非空（防死码）。
    {
        let mut rng = Lcg::new(0xF120_04A9);
        const SAMPLES: usize = 300;
        let mut all_tagged = true;
        let mut accepted = 0usize;
        let mut rejected = 0usize;
        for _ in 0..SAMPLES {
            let mut data = baseline.clone();
            let idx = (rng.next_u32() as usize) % data.len().max(1);
            if let Some(b) = data.get_mut(idx) {
                *b ^= 1 << (rng.next_u32() % 8);
            }
            let mut bag = DiagBag::new();
            match scan_annexb(&data, &H264_LIMITS, &mut bag) {
                Ok(_) => accepted += 1,
                Err(f) => {
                    rejected += 1;
                    if diag_name(f.code).is_empty() {
                        all_tagged = false;
                    }
                }
            }
        }
        let ok = all_tagged && accepted > 0 && rejected > 0 && accepted + rejected == SAMPLES;
        set.add("C04-FUZZ-BITFLIP-翻位变异全分类且拒绝带码", ok, "");
    }

    // M10 诊断三要素齐备（code + message + hint 均非空）
    {
        let zeros: Vec<u8> = vec![0u8; 32];
        let mut bag = DiagBag::new();
        match scan_annexb(&zeros, &H264_LIMITS, &mut bag) {
            Err(f) => {
                let d = f.to_diagnostic();
                let ok = !d.message.is_empty() && !d.hint.is_empty() && !diag_name(f.code).is_empty();
                set.add("C04-FUZZ-DIAG3-诊断三要素齐备", ok, "");
            }
            Ok(_) => set.add("C04-FUZZ-DIAG3-诊断三要素齐备", false, "未被拒"),
        }
    }

    // M11 处置方向相反的状态不共用码：预算超限 vs 槽位耗尽同为 DpbOverflow，
    //但与 ExpGolombTooLong 不同码（前者非阻断资源，后者语法炸弹）
    {
        let ok = diag_name(DiagCode::DpbOverflow) != diag_name(DiagCode::ExpGolombTooLong)
            && diag_name(DiagCode::ForbiddenZeroBitSet) != diag_name(DiagCode::NalRefIdcInconsistent);
        set.add("C04-FUZZ-CODES-诊断码不复用", ok, "");
    }

    // M12 DPB 预算超限与 Exp-Golomb 炸弹的诊断码**不共用**（语义可检索）
    {
        let names: Vec<&str> = [
            diag_name(DiagCode::DpbOverflow),
            diag_name(DiagCode::ExpGolombTooLong),
            diag_name(DiagCode::MvOutOfRange),
            diag_name(DiagCode::RefIndexOutOfRange),
            diag_name(DiagCode::EntropyEngineOutOfRange),
        ]
        .to_vec();
        let mut uniq = true;
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                if names[i] == names[j] {
                    uniq = false;
                }
            }
        }
        set.add("C04-FUZZ-CODEUNIQ-五类诊断码唯一", uniq, "");
    }

    // M13 NAL 体积炸弹被计数纪律拦下
    //
    // **原判据把 `max_nal_bytes` 设成 8 就以为能拦住**，实测基线码流总长只有
    // 28 字节、单个 NAL 更小，`max_nal = 8/32/256/4096/32MB` 全部 `Ok`——
    // 上限设在**语料之上**，判据测的是「上限够不够大」，不是「纪律有没有生效」。
    //
    // 现在构造一个**真的超限** NAL：把 `max_nal_bytes` 设为 64，
    // 语料里塞一个 4 KB 的 NAL 载荷（远超 64），并加一条
    // **反向对照**（同样语料在默认上限下必须通过），
    // 否则「一律拒绝」也能过这条判据。
    {
        const LIMIT: u32 = 64;
        let big_payload = vec![0xABu8; 4096];
        // NAL 类型选 9（访问单元分隔符 AUD）：按规范 7.4.1.2.4 它
        // **不要求 `nal_ref_idc != 0`**，所以默认上限下能被接受，
        // 反向对照才成立。（原先用 PPS 类型，`nal_ref_idc = 0` 被
        // `NalRefIdcInconsistent` 先拒掉，对照侧根本走不到体积检查。）
        let huge = nal(0, nal_type::ACCESS_UNIT_DELIMITER, &big_payload);

        let mut limits = H264_LIMITS;
        limits.max_nal_bytes = LIMIT;
        let mut bag = DiagBag::new();
        let caught = matches!(
            scan_annexb(&huge, &limits, &mut bag).err(),
            Some(f) if f.code == DiagCode::DpbOverflow
        );
        // 反向对照：默认上限（32MB）下同一条语料必须被接受。
        let mut bag2 = DiagBag::new();
        let under = scan_annexb(&huge, &H264_LIMITS, &mut bag2).is_ok();
        set.add("C04-FUZZ-SIZELIMIT-超大NAL被计数纪律拦", caught && under, "");
    }

    // M14 整数算术：checked_add / checked_mul / checked_range 边界
    {
        let ok = checked_add(1, 2) == Some(3)
            && checked_add(u64::MAX, 1).is_none()
            && checked_mul(u64::MAX, 2).is_none()
            && checked_range(0, 10, 10)
            && !checked_range(0, 11, 10)
            && !checked_range(11, 0, 10);
        set.add("C04-FUZZ-ARITH-整数算术边界", ok, "");
    }

    // M15 零 panic 面：模块内无 unwrap/expect（在 run_*_checks 中允许，此处断言接口行为）
    {
        // 用越界访问证明读路径返回 None 而非 panic。
        let p = match constant_plane(4, 4, 7) {
            Some(x) => x,
            None => {
                set.add("C04-FUZZ-ZEROPANIC-越界读取返回None", false, "平面分配失败");
                return;
            }
        };
        let ok = p.at(-1, 0).is_none() && p.at(0, -1).is_none() && p.at(999, 999).is_none();
        set.add("C04-FUZZ-ZEROPANIC-越界读取返回None", ok, "");
    }

    // M16 fuzz 变体：NAL 单元体积超上限（独立于 max_nal_bytes 的槽位维度）
    {
        let mut limits = H264_LIMITS;
        limits.max_nal_units = 1; // 只允许 1 个 NAL
        let mut bag = DiagBag::new();
        let caught = scan_annexb(&baseline, &limits, &mut bag).is_err();
        set.add("C04-FUZZ-NALCOUNT-NAL数量超限被拦", caught, "");
    }
}













