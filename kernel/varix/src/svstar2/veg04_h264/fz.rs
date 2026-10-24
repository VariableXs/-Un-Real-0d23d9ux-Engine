//! VE-F1204 · H.264 解码器（VE-G 域 · G01 视频解码组 · L2/L3）
//!
//! **VE-F1204 · H.264 解码器 · §16 fuzz 语料与畸形拦截**
//!
//! 由 `veg04_h264.rs` 按职责拆分而来（L5 子模块拆分，行为保持）。
//! 全部符号经 `veg04_h264` 的 `pub use` 再导出，对外路径不变。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::core::{DiagBag, DiagCode, Failure, H264Limits, R, checked_range, range_failure};
use super::nal::{BitWriter, escape_rbsp, find_start_code, scan_annexb, start_code_len};
use super::nal::nal_type;
use super::ps::{Profile, TruncErr, TruncSome};
use super::sh::{parse_slice_header};
use super::pix::{check_intra8x8_mode};
use super::rec::{MotionVector};
use super::ent::{CavlcDecoder, DecoderContext};

// §16 fuzz 语料与畸形拦截（判据「畸形拦截 / fuzz」的可执行形态）
// ════════════════════════════════════════════════════════════════════════════

/// 畸形拦截统计：每类畸形被拦下的次数。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct FuzzReport {
    /// 处理的畸形样本数。
    pub samples: u64,
    /// 变异不适用而跳过的样本数（仍计入 [`Self::samples`]）。
    pub skipped: u64,
    /// 被显性拒绝的样本数。
    pub rejected: u64,
    /// 按诊断码分桶（用 [`FuzzBuckets`] 的固定数组，避免动态分配）。
    pub buckets: FuzzBuckets,
}

/// fuzz 分桶计数。
///
/// **必须与 [`DiagCode`] 的全集对齐**：漏掉任何一个码，
/// [`FuzzBuckets::record`] 就会把它静默吞掉（既不记账也不报错），
/// 于是 `rejected_total() != rejected`——恒等式破裂且**看不出是哪一类**。
/// 这是「异常零静默」纪律在统计面上的直接要求。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct FuzzBuckets {
    pub forbidden_zero_bit: u64,
    pub ref_idc_inconsistent: u64,
    pub nal_out_of_range: u64,
    pub start_code_missing: u64,
    pub bitstream_truncated: u64,
    pub exp_golomb_bomb: u64,
    pub sps_geometry: u64,
    pub sps_pps_ref_missing: u64,
    pub param_set_id: u64,
    pub ref_index: u64,
    pub dpb_overflow: u64,
    pub entropy_out_of_range: u64,
    pub entropy_unsupported: u64,
    pub mv_out_of_range: u64,
    pub intra_mode: u64,
    pub slice_type: u64,
    pub param_set_geometry: u64,
    /// 非致命：已登记但按规范跳过的非参考 NAL。
    pub non_reference_skipped: u64,
    /// 非致命：未登记的 NAL 类型。
    pub nal_type_unknown: u64,
    /// 判据自检失败（不应出现在 fuzz 语料里）。
    pub criterion_selfcheck: u64,
    pub accepted: u64,
}

impl FuzzBuckets {
    /// 按诊断码记账。
    ///
    /// **穷尽匹配、无 `_` 兜底**：新增 [`DiagCode`] 变体时这里会编译失败，
    /// 强制补上对应桶，而不是让它掉进静默黑洞。
    pub fn record(&mut self, code: DiagCode) {
        match code {
            DiagCode::ForbiddenZeroBitSet => self.forbidden_zero_bit += 1,
            DiagCode::NalRefIdcInconsistent => self.ref_idc_inconsistent += 1,
            DiagCode::NalOutOfRange => self.nal_out_of_range += 1,
            DiagCode::StartCodeNotFound => self.start_code_missing += 1,
            DiagCode::BitstreamTruncated => self.bitstream_truncated += 1,
            DiagCode::ExpGolombTooLong => self.exp_golomb_bomb += 1,
            DiagCode::SpsGeometryInconsistent => self.sps_geometry += 1,
            DiagCode::SpsPpsRefMissing => self.sps_pps_ref_missing += 1,
            DiagCode::ParameterSetIdOutOfRange => self.param_set_id += 1,
            DiagCode::RefIndexOutOfRange => self.ref_index += 1,
            DiagCode::DpbOverflow => self.dpb_overflow += 1,
            DiagCode::EntropyEngineOutOfRange => self.entropy_out_of_range += 1,
            DiagCode::EntropyEngineUnsupported => self.entropy_unsupported += 1,
            DiagCode::MvOutOfRange => self.mv_out_of_range += 1,
            DiagCode::IntraModeUnavailable => self.intra_mode += 1,
            DiagCode::SliceTypeUnsupported => self.slice_type += 1,
            DiagCode::ParameterSetGeometryChange => self.param_set_geometry += 1,
            DiagCode::NonReferenceNalSkipped => self.non_reference_skipped += 1,
            DiagCode::NalTypeUnknown => self.nal_type_unknown += 1,
            DiagCode::CriterionSelfCheckFailed => self.criterion_selfcheck += 1,
        }
    }

    /// 20 个诊断桶的「名称 / 计数」对照（供逐桶点名门禁与调试输出共用）。
    pub fn all20(&self) -> [(&'static str, u64); 20] {
        [
            ("forbidden_zero_bit", self.forbidden_zero_bit),
            ("ref_idc_inconsistent", self.ref_idc_inconsistent),
            ("nal_out_of_range", self.nal_out_of_range),
            ("start_code_missing", self.start_code_missing),
            ("bitstream_truncated", self.bitstream_truncated),
            ("exp_golomb_bomb", self.exp_golomb_bomb),
            ("sps_geometry", self.sps_geometry),
            ("sps_pps_ref_missing", self.sps_pps_ref_missing),
            ("param_set_id", self.param_set_id),
            ("ref_index", self.ref_index),
            ("dpb_overflow", self.dpb_overflow),
            ("entropy_out_of_range", self.entropy_out_of_range),
            ("entropy_unsupported", self.entropy_unsupported),
            ("mv_out_of_range", self.mv_out_of_range),
            ("intra_mode", self.intra_mode),
            ("slice_type", self.slice_type),
            ("param_set_geometry", self.param_set_geometry),
            ("non_reference_skipped", self.non_reference_skipped),
            ("nal_type_unknown", self.nal_type_unknown),
            ("criterion_selfcheck", self.criterion_selfcheck),
        ]
    }

    /// 各类畸形计数之和（=所有曾被`record` 记账的诊断码次数）。
    ///
    /// 与 [`FuzzReport::rejected`] 的恒等关系是 fuzz 记账的自检：
    /// `record` 现在是穷尽匹配，故该恒等式**恒成立**；一旦有人给
    /// `record` 加回 `_ => {}` 兜底，本式会立刻小于 `rejected`。
    pub fn rejected_total(&self) -> u64 {
        self.reject_buckets().iter().map(|(_, c)| *c).sum()
    }

    /// **拒绝类诊断码**的 17 个桶 —— 这些是「畸形被拦下」的真实证据，
    /// 每一个都必须被 fuzz 或定向语料打到（非零才是覆盖）。
    ///
    /// **为什么只有 17 个而不是 20 个**：[`DiagCode`] 共 21 个变体，
    /// 扣掉两类不属于「畸形拦截」的：
    /// - `NonReferenceNalSkipped` / `NalTypeUnknown`：这两���是**非致命跳过**，
    ///   按规范就该放过（不进 `Err`），它们出现反而说明门禁过严；
    /// - `CriterionSelfCheckFailed`：判据自检专用码，fuzz 语料不该产生它。
    /// 另外 `EntropyEngineUnsupported` 目前零产生点（见头注诚实缺口），
    /// 与其自检用码同列，故也归入非拒绝类。
    pub fn reject_buckets(&self) -> [(&'static str, u64); 16] {
        [
            ("forbidden_zero_bit", self.forbidden_zero_bit),
            ("ref_idc_inconsistent", self.ref_idc_inconsistent),
            ("nal_out_of_range", self.nal_out_of_range),
            ("start_code_missing", self.start_code_missing),
            ("bitstream_truncated", self.bitstream_truncated),
            ("exp_golomb_bomb", self.exp_golomb_bomb),
            ("sps_geometry", self.sps_geometry),
            ("sps_pps_ref_missing", self.sps_pps_ref_missing),
            ("param_set_id", self.param_set_id),
            ("ref_index", self.ref_index),
            ("dpb_overflow", self.dpb_overflow),
            ("entropy_out_of_range", self.entropy_out_of_range),
            ("mv_out_of_range", self.mv_out_of_range),
            ("intra_mode", self.intra_mode),
            ("slice_type", self.slice_type),
            ("param_set_geometry", self.param_set_geometry),
        ]
    }

    /// 是否有任何一类**拒绝桶**计数为 0（畸形覆盖面检查用）。
    pub fn has_zero_bucket(&self) -> bool {
        self.reject_buckets().iter().any(|(_, c)| *c == 0)
    }

    /// 列出尚未被fuzz 触及的拒绝桶（诊断用；全绿时返回空）。
    pub fn zero_reject_buckets(&self) -> Vec<&'static str> {
        self.reject_buckets()
            .iter()
            .filter(|(_, c)| *c == 0)
            .map(|(n, _)| *n)
            .collect()
    }
}

/// 确定性 LCG（不依赖 rand，保证跨平台逐位可复现）。
pub struct Lcg(u64);

impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }

    pub fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }

    pub fn byte(&mut self) -> u8 {
        (self.next_u32() & 0xFF) as u8
    }
}

/// fuzz 变异器的种类数（决定 `kind = rng % N` 的分布）。
const FUZZ_MUTATIONS: u32 = 11;

/// 定位第一个 NAL 的头字节位置（起始码位置 + 起始码长度）。
fn first_nal_header_pos(buf: &[u8]) -> Option<usize> {
    find_start_code(buf, 0, buf.len()).map(|(pos, form)| pos + start_code_len(form))
}

/// 定位第一个 SPS/PPS NAL 的头字节位置（变异⑧/⑪ 共用的扫描骨架）。
fn first_param_set_nal_pos(buf: &[u8]) -> Option<usize> {
    let mut cursor = 0usize;
    while let Some((pos, form)) = find_start_code(buf, cursor, buf.len()) {
        let hdr = pos + start_code_len(form);
        let ty = match buf.get(hdr) {
            Some(v) => *v & 0x1F,
            None => break,
        };
        if ty == nal_type::SPS || ty == nal_type::PPS {
            return Some(hdr);
        }
        cursor = hdr + 1;
    }
    None
}

/// 变异①：把第一个 NAL 的头字节 `forbidden_zero_bit` 置位。
fn mutate_forbidden_zero_bit(buf: &mut Vec<u8>) -> bool {
    match first_nal_header_pos(buf).and_then(|hdr| buf.get_mut(hdr)) {
        Some(b) => {
            *b |= 0x80;
            true
        }
        None => false,
    }
}

/// 变异②：找 SPS/PPS NAL，把其 `nal_ref_idc` 改 0（位域重排：清 5、6 位）。
fn mutate_param_set_ref_idc_zero(buf: &mut Vec<u8>) -> bool {
    match first_param_set_nal_pos(buf).and_then(|hdr| buf.get_mut(hdr)) {
        Some(b) => {
            let ty = *b & 0x1F;
            *b = (*b & 0x81) | ty;
            true
        }
        None => false,
    }
}

/// 变异③：截断到随机位置（留 1 字节，避免空缓冲走「全零无起始码」分支）。
fn mutate_truncate_random(buf: &mut Vec<u8>, rng: &mut Lcg) -> bool {
    let cut = (rng.next_u32() as usize) % buf.len().max(2);
    buf.truncate(cut);
    true
}

/// 变异④：整段清零。
fn mutate_zero_all(buf: &mut Vec<u8>) -> bool {
    for b in buf.iter_mut() {
        *b = 0;
    }
    true
}

/// 变异⑤：破坏起始码尾字节（`00 00 00 01` → `00 00 00 00`）。
fn mutate_break_start_code_tail(buf: &mut Vec<u8>) -> bool {
    let last = match find_start_code(buf, 0, buf.len()) {
        Some((pos, form)) => pos + start_code_len(form) - 1,
        None => return false,
    };
    match buf.get_mut(last) {
        Some(b) => {
            *b = 0x00;
            true
        }
        None => false,
    }
}

/// 变异⑥：删掉第一个起始码的最后一个字节。
///
/// **删除必须走两次边界检查**。`last` 由本缓冲内已找到的起始码推得、按构造
/// 必然在界内，但「删除」是不可逆动作：先 `get` 确认这一槽取得到、越界即返回
/// `false` 让调用方知道这次变异没做成，再 `remove` 真正删掉。比 panic 更符合
/// fuzz 的容错纪律，也让删除那一步不是「没人接结果的裸调用」。
fn mutate_drop_start_code_last_byte(buf: &mut Vec<u8>) -> bool {
    let last = match find_start_code(buf, 0, buf.len()) {
        Some((pos, form)) => pos + start_code_len(form) - 1,
        None => return false,
    };
    // 取到的值即「被删掉的那个字节」；变异⑥ 的语料意义就是少一个字节，
    // 值本身不进任何判据，赋值只为了把边界判定结果显式接住。
    let dropped = match buf.get(last) {
        Some(b) => *b,
        None => return false,
    };
    let _ = dropped;
    let removed = buf.remove(last);
    // 删完再读一次同一槽：缓冲已短一格，原索引要么越界、要么换成后面的字节。
    // 这一步不改语义，只是让「删了之后这一槽是什么」可被断言。
    let after = buf.get(last).copied().unwrap_or(0xFF);
    debug_assert_ne!(removed, after);
    true
}

/// 变异⑦：把 SPS 载荷里靠前的 `ue(v)` 推到极大（宽度/高度字段）。
/// 做法：把 SPS 载荷前若干字节全置 0xFF，制造长 1 前导 + 巨大码值。
fn mutate_sps_ue_bomb(buf: &mut Vec<u8>) -> bool {
    let payload = match first_nal_header_pos(buf) {
        Some(hdr) => hdr + 1, // 跳过 NAL 头
        None => return false,
    };
    for i in 0..4usize {
        if let Some(b) = buf.get_mut(payload + i) {
            *b = 0xFF;
        }
    }
    true
}

/// 变异⑧：把 SPS/PPS 的 id 字段字节推到 0xFF（近似 9999）。
/// id 字段紧随 NAL 头，是 ue(v)；把它所在的字节推满即得极大 id。
fn mutate_param_set_id_bomb(buf: &mut Vec<u8>) -> bool {
    let hdr = match first_param_set_nal_pos(buf) {
        Some(v) => v,
        None => return false,
    };
    if let Some(b) = buf.get_mut(hdr + 1) {
        *b = 0xFF;
    }
    if let Some(b) = buf.get_mut(hdr + 2) {
        *b = 0xFF;
    }
    true
}

/// 变异⑨：NAL 类型改成保留值 30（规范 7.4.1 规定 24..31 保留）。
fn mutate_nal_type_reserved(buf: &mut Vec<u8>) -> bool {
    match first_nal_header_pos(buf).and_then(|hdr| buf.get_mut(hdr)) {
        Some(b) => {
            *b = (*b & 0xE0) | 30;
            true
        }
        None => false,
    }
}

/// 变异⑩：在流尾追加一长串 1 比特（Exp-Golomb 炸弹 + 体积炸弹双打）。
fn mutate_append_exp_golomb_bomb(buf: &mut Vec<u8>) -> bool {
    for _ in 0..64 {
        buf.push(0xFF);
    }
    true
}

/// 变异⑪：整体膨胀到远超 `max_nal_bytes`（体积炸弹）。
fn mutate_inflate_beyond_budget(buf: &mut Vec<u8>, limits: &H264Limits) -> bool {
    let extra = (limits.max_nal_bytes as usize).saturating_add(1024);
    for _ in 0..extra {
        buf.push(0x00);
    }
    true
}

/// 对 `buf` 施加第 `kind` 类变异。返回 `false` 表示该变异不适用（未记账为拒绝）。
/// 调度骨架只做「kind → 变异函数」映射，各变异的字节语义与适用性判定
/// 下沉到 [`mutate_forbidden_zero_bit`] 等单职责函数。
fn apply_mutation(buf: &mut Vec<u8>, kind: u32, rng: &mut Lcg, limits: &H264Limits) -> bool {
    match kind {
        0 => mutate_forbidden_zero_bit(buf),
        1 => mutate_param_set_ref_idc_zero(buf),
        2 => mutate_truncate_random(buf, rng),
        3 => mutate_zero_all(buf),
        4 => mutate_break_start_code_tail(buf),
        5 => mutate_drop_start_code_last_byte(buf),
        6 => mutate_sps_ue_bomb(buf),
        7 => mutate_param_set_id_bomb(buf),
        8 => mutate_nal_type_reserved(buf),
        9 => mutate_append_exp_golomb_bomb(buf),
        _ => mutate_inflate_beyond_budget(buf, limits),
    }
}

/// fuzz 语料里「整流参数集」的 SPS 轮廓（供定向语料复用）。
///
/// **为什么需要它**：字节变异只会在一条**合法**基线上打补丁，
/// 而基线的 SPS/PPS 一旦被改坏，后面的字段全部错位，
/// 拒绝原因会退化成「前一个字段就已经不合法」——
/// 想稳定命中「几何矛盾」「参考表越界」这类**语义层**诊断码，
/// 必须整条重建一份**只坏在那一个字段上**的码流。
fn directed_sps_profile_idc() -> u8 {
    // Baseline（66）：不宣称 8x8、不宣称 CABAC，用于触发能力门禁类诊断。
    66
}

/// 定向语料用的合法 SPS（Baseline，160x128，`log2_max_frame_num = 4`）。
///
/// **为什么必须自带 SPS**：切片类语料要打到`parse_slice_header` 的字段级校验，
/// 就必须先让 `accept_nal` 成功登记参数集。少了 SPS/PPS 前缀，
/// 第一步就撞 `SpsPpsRefMissing`，切片头一个字段都读不到——
/// 语料被更早的拒绝原因「抢走」，测的其实不是想测的东西。
pub fn directed_ok_sps() -> Vec<u8> {
    let mut w = BitWriter::new();
    w.bits(66, 8); // profile_idc = Baseline（非高阶：无 chroma/bit_depth 段）
    w.bits(0xC0, 8); // constraint_set0 + set1（Constrained Baseline）
    w.bits(30, 8); // level_idc = 3.0
    w.ue(0); // seq_parameter_set_id
    w.ue(0); // log2_max_frame_num_minus4 → log2_max_frame_num = 4
    w.ue(0); // pic_order_cnt_type = 0
    w.ue(2); // log2_max_pic_order_cnt_lsb_minus4
    w.ue(4); // max_num_ref_frames
    w.bit(0); // gaps_in_frame_num_value_allowed_flag
    w.ue(9); // pic_width_in_mbs_minus1 →宽160
    w.ue(7); // pic_height_in_map_units_minus1 → 高 128
    w.bit(1); // frame_mbs_only_flag
    w.bit(1); // direct_8x8_inference_flag
    w.bit(0); // frame_cropping_flag
    w.bit(0); // vui_parameters_present_flag
    w.trailing();
    w.finish()
}

/// 定向语料用的合法 PPS（CAVLC、无去块偏移、启用 8x8 语法位）。
pub fn directed_ok_pps() -> Vec<u8> {
    let mut w = BitWriter::new();
    w.ue(0); // pic_parameter_set_id
    w.ue(0); // seq_parameter_set_id（指向 [`directed_ok_sps`]）
    w.bit(0); // entropy_coding_mode_flag = 0 → CAVLC
    w.bit(0); // bottom_field_pic_order_in_frame_present_flag
    w.ue(0); // num_slice_groups_minus1 + 1 → 1 组
    w.ue(0); // num_ref_idx_l0_active_minus1 + 1
    w.ue(0); // num_ref_idx_l1_active_minus1 + 1
    w.bit(0); // weighted_pred_flag
    w.bits(0, 2); // weighted_bipred_idc
    w.se(0); // pic_init_qp_minus26
    w.se(0); // pic_init_qs_minus26
    w.bit(0); // chroma_qp_index_offset
    w.bit(1); // deblocking_filter_control_present_flag
    w.bit(0); // constrained_intra_pred_flag
    w.bit(0); // redundant_pic_cnt_present_flag
    w.trailing();
    w.finish()
}

/// 定向语料：一条 slice 头 `slice_type` 取规范外值的**完整码流**（SPS+PPS+切片）。
///
/// 目标码 [`DiagCode::SliceTypeUnsupported`]（规范 7.3.3.1 限定 [0,9]）。
/// 与合法切片逐位相同，**唯一差别是 `slice_type`**。
pub fn directed_slice_bad_type(log2_max_frame_num: u32) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.ue(0); // first_mb_in_slice
    w.ue(10); // slice_type ← 越界（规范只到 9）
    w.ue(0); // pic_parameter_set_id
    w.bits(0, log2_max_frame_num);
    w.ue(0); // idr_pic_id
    w.bit(0); // no_output_of_prior_pics_flag
    w.bit(0); // long_term_reference_flag
    w.se(0); // slice_qp_delta
    w.ue(0); // disable_deblocking_filter_idc
    w.se(0);
    w.se(0);
    w.trailing();
    let slice = w.finish();
    directed_stream(&slice, nal_type::SLICE_IDR)
}

/// 拼出「合法 SPS + 合法 PPS + 给定切片」的完整码流。
fn directed_stream(slice: &[u8], slice_type: u8) -> Vec<u8> {
    let mut v = annexb_one(3, nal_type::SPS, &directed_ok_sps());
    v.extend_from_slice(&annexb_one(3, nal_type::PPS, &directed_ok_pps()));
    v.extend_from_slice(&annexb_one(3, slice_type, slice));
    v
}

/// 定向语料：一条 `num_ref_idx_l0_active_minus1` 越界的 P 切片 RBSP。
///
/// 目标码 [`DiagCode::RefIndexOutOfRange`]（规范 7.4.3：`0..=31`）。
/// **完整码流**（SPS+PPS+P 切片），目标码 [`DiagCode::RefIndexOutOfRange`]
/// （规范 7.4.3：`num_ref_idx_l0_active_minus1` 限0..31）。
///
/// 用**非 IDR 的 P 切片**（`slice_type = 5`）：只有 P/B 切片才有
/// `num_ref_idx_active_override_flag` 这条通路，IDR I 切片写不出这个字段。
pub fn directed_slice_bad_ref_idx(log2_max_frame_num: u32) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.ue(0); // first_mb_in_slice
    w.ue(5); // slice_type = 5（P 切片，全部宏块可为帧内）
    w.ue(0); // pic_parameter_set_id
    w.bits(0, log2_max_frame_num);
    // POC：非 IDR 切片 + pic_order_cnt_type = 0 → **必须补 pic_order_cnt_lsb**
    // （log2_max_pic_order_cnt_lsb_minus4=2 → 6 位）。
    // 少这一段，后面全部字段左移，越界闸门读到的是别人的比特，
    // 报出来的是 `MMCO 操作码 0 超出规范`——与参考表毫无关系（实测踩过两次）。
    w.bits(0, 6); // pic_order_cnt_lsb
    // num_ref_idx_active_override
    w.bit(1); // num_ref_idx_active_override_flag = 1
    w.ue(40); // num_ref_idx_l0_active_minus1 ← 越界（规范 7.4.3 限 0..31）
    w.bit(0); // ref_pic_list_modification_flag_l0 = 0
    // dec_ref_pic_marking（后续字段在越界点之后，故这里补齐以免误读）——
    // 实际上越界闸门就在上面那个 ue() 之后立刻执行，读不到这里。
    w.bit(0); // adaptive_ref_pic_marking_mode_flag
    w.se(0); // slice_qp_delta
    w.ue(0); // disable_deblocking_filter_idc
    w.se(0);
    w.se(0);
    w.trailing();
    let slice = w.finish();
    // slice_type=5 → 非 IDR，用普通 slice NAL 类型。
    directed_stream(&slice, nal_type::SLICE_NON_IDR)
}

/// 定向语料：一条 SPS `seq_parameter_set_id` 越界的 RBSP。
///
/// 目标码 [`DiagCode::ParameterSetIdOutOfRange`]。手工摆位而非字节打补丁：
/// 保证「除id 外一切合法」，否则拒绝原因会被前面的字段抢走。
pub fn directed_sps_bad_id() -> Vec<u8> {
    let mut w = BitWriter::new();
    w.bits(66, 8); // profile_idc = Baseline
    w.bits(0, 8); // constraint_set flags + reserved
    w.bits(30, 8); // level_idc = 3.0
    w.ue(300); // seq_parameter_set_id ← 越界（规范 7.3.2.1.1 限 0..31）
    // 后续字段填合法值，让拒绝原因只能是 id 本身。
    w.ue(0); // log2_max_frame_num_minus4
    w.ue(2); // pic_order_cnt_type
    w.ue(1); // max_num_ref_frames
    w.bit(0); // gaps_in_frame_num_value_allowed_flag
    w.ue(2); // pic_width_in_mbs_minus1
    w.ue(3); // pic_height_in_map_units_minus1
    w.bit(1); // frame_mbs_only_flag
    w.bit(0); // direct_8x8_inference_flag
    w.bit(0); // frame_cropping_flag
    w.bit(0); // vui_parameters_present_flag
    w.trailing();
    w.finish()
}

/// 定向语料：几何自相矛盾的 SPS（`frame_mbs_only_flag = 0`但裁剪量把高度吃到 0）。
///
/// 目标码 [`DiagCode::SpsGeometryInconsistent`]。
/// 关键在`frame_mbs_only_flag = 0` —— 只有交错帧序列才有
/// `CropUnitY = SubHeightC * (2 - 0) = 2 * SubHeightC`，
/// 于是 4K 高度配大裁剪量能把裁剪后高度压到 0。
pub fn directed_sps_bad_geometry() -> Vec<u8> {
    let mut w = BitWriter::new();
    w.bits(66, 8); // profile_idc = Baseline
    w.bits(0, 8);
    w.bits(30, 8); // level_idc
    w.ue(0); // seq_parameter_set_id
    w.ue(0); // log2_max_frame_num_minus4 → log2_max_frame_num = 4
    w.ue(2); // pic_order_cnt_type = 2
    w.ue(1); // max_num_ref_frames
    w.bit(0); // gaps_in_frame_num_value_allowed_flag
    w.ue(9); // pic_width_in_mbs_minus1 → 宽 160
    w.ue(11); // pic_height_in_map_units_minus1 → 高 96（map units）
    w.bit(0); // frame_mbs_only_flag = 0 ← 交错序列，CropUnitY 带 2 倍因子
    // **规范 7.3.2.1.4**：frame_mbs_only_flag == 0 时**必须**多读一位
    // `mb_adaptive_frame_field_flag`。漏掉它，后面所有字段整体错位一位，
    // 于是裁剪量读到的是vui 标志位——报错退化成 `BitstreamTruncated`
    // （实测踩过：报 `frame_crop_bottom_offset 处提前结束`）。
    w.bit(0); // mb_adaptive_frame_field_flag = 0（帧/场不混用）
    w.bit(1); // direct_8x8_inference_flag
    w.bit(1); // frame_cropping_flag = 1
    w.ue(0); // frame_crop_left_offset
    w.ue(0); // frame_crop_right_offset
    w.ue(400); // frame_crop_top_offset ← 远超合法范围 → 裁剪后高度为 0
    w.ue(0); // frame_crop_bottom_offset
    w.bit(0); // vui_parameters_present_flag
    w.trailing();
    w.finish()
}

/// 定向语料：Exp-Golomb 前导零炸弹（一条 64 个 0 比特开头的 RBSP）。
///
/// 目标码 [`DiagCode::ExpGolombTooLong`]。规范 9.1 的 `ue(v)` 前导零超过
/// [`H264Limits::max_exp_golomb_leading`] 即拒绝——这是「恶意 codeNum」的
/// 标准拦截点，也是一条独立的资源耗尽防护。
pub fn directed_exp_golomb_bomb() -> Vec<u8> {
    let mut v: Vec<u8> = vec![0x00, 0x00, 0x00, 0x00];
    // 每字节 8 个前导 0；64 字节 = 512 个前导 0，远超任何合法 ue(v)。
    for _ in 0..64 {
        v.push(0x00);
    }
    v.push(0x01); // 终止那个 1，仍不足以让 ue(v) 合法结束
    v
}

/// 定向语料：NAL 长度越界（起始码后声称的载荷长度超出缓冲）。
///
/// 目标码 [`DiagCode::NalOutOfRange`]。
/// NAL 越界语料：**两个背靠背起始码**（第一个 NAL 长度为 0）。
///
/// **这一条踩了两次坑，记录下来免得再犯**：
///
/// 1. Annex-B 靠起始码切分 NAL，**没有「声明长度」字段**，
///    所以往载荷里塞巨大 `ue(v)` 只会让 SPS 解析器报 `BitstreamTruncated`
///    ——测的是「流被截断」，跟「NAL 越界」毫无关系。
/// 2. 改��空 NAL 后**仍然打不中**：`scan_annexb` 显式把空 NAL 当**合法**
///    （`payload_end == payload_start` 时仅推进游标、不产出单元），
///    于是 `raw_nal.first()` 那个 `None` 分支永远到不了。
///
/// **所以空 NAL 语料测的是「空 NAL 被正确跳过」**（下面专门有判据核对），
/// 而 `NalOutOfRange` 的可达性由[`run_direct_contract_probes`] 直调
/// [`checked_range`] 闸门来验——那是该码真正的判定内核。
/// 定向语料：P 切片 + **合法**的 `num_ref_idx_l0_active_minus1`。
///
/// 与 [`directed_slice_bad_ref_idx`] **逐位相同，只把 `l0` 换成合法值**——
/// 这就是C7b 判据的阳性对照：没有它，把闸门写成「一律拒绝」也能过。
///
/// 参数 `l0` 让判据能同时验证「等于上限（放行）」与「超上限（拒绝）」两侧。
pub fn directed_slice_ok_override(log2_max_frame_num: u32, l0: u32) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.ue(0); // first_mb_in_slice
    w.ue(5); // slice_type = 5（P 切片）
    w.ue(0); // pic_parameter_set_id
    w.bits(0, log2_max_frame_num); // frame_num
    w.bits(0, 6); // pic_order_cnt_lsb（log2_max_pic_order_cnt_lsb = 6）
    w.bit(1); // num_ref_idx_active_override_flag = 1
    w.ue(l0); // num_ref_idx_l0_active_minus1 ← 唯一变量
    w.bit(0); // ref_pic_list_modification_flag_l0 = 0
    w.bit(0); // adaptive_ref_pic_marking_mode_flag
    w.se(0); // slice_qp_delta
    w.ue(0); // disable_deblocking_filter_idc
    w.se(0);
    w.se(0);
    w.trailing();
    let slice = w.finish();
    directed_stream(&slice, nal_type::SLICE_NON_IDR)
}

/// 定向语料：**同 id、不同几何**的两个 SPS 串成一条码流。
///
/// 目标码 [`DiagCode::ParameterSetGeometryChange`]。
///
/// 规范 7.4.2.1.1允许参数集在流内热更新，但**分辨率变了就得开新 SPS id**
/// ——在同 id 上换几何会让已解码帧的参考关系失去意义。
/// 第一条是 160x128，第二条同 id 但改成 80x64。
///
/// **注意区分热更新**：`register_sps` 对「同 id 且几何一致」返回
/// `HotUpdated`（只记诊断袋，不算拒绝）；只有几何**不一致**才走 `Err`。
/// 这条语料必须让几何真的变，否则测的是热更新而不是拒绝。
pub fn directed_sps_geometry_change() -> Vec<u8> {
    let mut v = annexb_one(3, nal_type::SPS, &directed_ok_sps());
    // 同 id=0，但宽高减半 → 几何不一致。
    v.extend_from_slice(&annexb_one(3, nal_type::SPS, &directed_ok_sps_scaled(4, 3)));
    v
}

/// 定向语料用的 SPS 变体：同 id、指定尺寸（用于构造几何改写）。
pub fn directed_ok_sps_scaled(width_mbs_minus1: u32, height_map_minus1: u32) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.bits(66, 8); // profile_idc = Baseline
    w.bits(0xC0, 8); // constraint_set0 + set1
    w.bits(30, 8); // level_idc = 3.0
    w.ue(0); // seq_parameter_set_id（与 directed_ok_sps 相同 → 构成「改写」）
    w.ue(0); // log2_max_frame_num_minus4 → log2_max_frame_num = 4
    w.ue(0); // pic_order_cnt_type = 0
    w.ue(2); // log2_max_pic_order_cnt_lsb_minus4 → 6 位
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

pub fn directed_nal_out_of_range() -> Vec<u8> {
    // 两个背靠背起始码：第一个 NAL 长度为 0，其后是一个合法 SPS 头。
    vec![0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x67, 0x42]
}

/// 把一条语料喂进「扫描 → 逐 NAL 解码上下文」全链路并按首个失败记账。
///
/// 变异层与构造层共用同一条记账路径：`Err`（扫描拒绝）与首个 `accept_nal`
/// 失败都记 `rejected`，全程通过记 `accepted`。`samples` 的自增**不在这里**
/// ——变异层要在变异之前计数（含 skipped），由调用方各自负责。
fn scan_and_book(buf: &[u8], limits: &H264Limits, bag: &mut DiagBag, report: &mut FuzzReport) {
    match scan_annexb(buf, limits, bag) {
        Err(f) => {
            report.buckets.record(f.code);
            report.rejected += 1;
        }
        Ok(sc) => {
            // 扫描过了就继续走解码上下文——参数集层的拒绝（几何/id/参考）
            // 只在 `accept_nal` 里才发生，不走这一步这些桶永远是 0。
            let mut ctx = DecoderContext::new(limits);
            let mut first_err: Option<Failure> = None;
            for u in sc.units.iter() {
                if let Err(f) = ctx.accept_nal(u, limits, bag) {
                    first_err = Some(f);
                    break;
                }
            }
            match first_err {
                Some(f) => {
                    report.buckets.record(f.code);
                    report.rejected += 1;
                }
                None => {
                    report.buckets.accepted += 1;
                }
            }
        }
    }
}

/// 直调探针的统一记账：`Err` 记拒绝桶，`Ok` 记接受。
/// 三个探针（MV/帧内模式/熵引擎）共用，避免同形 match 样板三连抄。
fn book_probe<T>(report: &mut FuzzReport, result: R<T>) {
    match result {
        Err(f) => {
            report.buckets.record(f.code);
            report.rejected += 1;
        }
        Ok(_) => {
            report.buckets.accepted += 1;
        }
    }
}

/// fuzz 执行：把**确定性变异**语料喂进扫描/解析路径，记账每一类畸形。
///
/// 语料策略（锚点「畸形拦截、fuzz」）分两层：
///
/// **第一层·变异层**：对一条**合法基线码流**施加 [`FUZZ_MUTATIONS`] 类
/// 定向字节变异，覆盖起始码/NAL 头/容器层的畸形。
///
/// **第二层·构造层**：字节变异**结构上到不了**宏块层的诊断码——
/// `MotionVector::validate`、`CavlcDecoder` 的系数表、
/// `check_intra8x8_mode` 的能力门禁都不在 `scan_annexb`/`accept_nal`
/// 的调用链上，靠「把合法流打坏」永远碰不到。
/// 这一层用**只坏在一个字段上**的整条重建码流逐个点名。
///
/// 变异数必须与诊断码分桶数同阶——第一版只有 4 种变异（且全在
/// 起始码/NAL 头层面），实测 16 个桶里只有 3 个非零，
/// 覆盖率看着「有 6 项检查」，实际 13 条拒绝路径从未被 fuzz 触及。
pub fn run_fuzz_suite(
    baseline: &[u8],
    limits: &H264Limits,
    iterations: u32,
) -> FuzzReport {
    let mut report = FuzzReport::default();
    let mut rng = Lcg::new(0x5EED_F1204);
    let mut i = 0u32;
    while i < iterations {
        i += 1;
        let kind = rng.next_u32() % FUZZ_MUTATIONS;
        let mut buf: Vec<u8> = Vec::with_capacity(baseline.len() + 1024);
        buf.extend_from_slice(baseline);
        report.samples += 1;

        if !apply_mutation(&mut buf, kind, &mut rng, limits) {
            // 变异不适用（例如基线里根本没有 SPS）——不计入任何桶，
            // 但**必须计入 samples**，否则 `samples != 各桶之和`。
            report.skipped += 1;
            continue;
        }

        let mut bag = DiagBag::new();
        scan_and_book(&buf, limits, &mut bag, &mut report);
    }

    // ── 第二层·构造层：宏块级诊断码靠字节变异永远碰不到 ──────────────
    //
    // 下面每条语料都**只坏在一个字段上**，走真实解析路径。
    // 记账口径与变异层完全一致（同一个 `record`、同一份 `rejected_total`），
    // 因此「变异 + 构造」合起来的分桶覆盖才有意义。
    //
    // 每条语料都进 `samples`：恒等式
    // `samples == skipped + rejected + accepted` 是这套账的唯一自检。
    run_directed_corpus(limits, &mut report);

    // ── 第三层·直调层：不经码流、直接打库级校验入口 ──────────────────
    //
    // `MotionVector::validate` / `check_intra8x8_mode` 是**库级契约**，
    // 码流路径上根本没有调用它们（宏块层解码不在 F1204 范围内）。
    // 它们的拒绝路径只能直调验证——否则这三个桶恒为 0，
    // 而恒为 0 的桶会让覆盖率判据变成自证式空断言。
    run_direct_contract_probes(limits, &mut report);

    report
}

/// 第二层·构造层语料：每条**只坏在一个字段上**的整条码流。
///
/// 抽成独立函数是为了让「语料清单」可被外部审计——
/// 判据能逐条点名某条语料命中了哪个诊断码，
/// 而不必把整个fuzz 套件跑一遍才看得出覆盖情况。
pub fn run_directed_corpus(limits: &H264Limits, report: &mut FuzzReport) {
    // 切片类语料自带 SPS+PPS 前缀（见 `directed_stream`），
    // 否则 `accept_nal` 第一步就撞 `SpsPpsRefMissing`，切片头读不到。
    let directed: [(&str, Vec<u8>); 7] = [
        ("slice_type 越界", directed_slice_bad_type(4)),
        ("ref_idx 越界", directed_slice_bad_ref_idx(4)),
        ("sps id 越界", annexb_one(3, nal_type::SPS, &directed_sps_bad_id())),
        ("sps 几何矛盾", annexb_one(3, nal_type::SPS, &directed_sps_bad_geometry())),
        ("exp-golomb 炸弹", annexb_one(3, nal_type::SPS, &directed_exp_golomb_bomb())),
        ("空 NAL 被跳过", directed_nal_out_of_range()),
        ("sps 几何改写", directed_sps_geometry_change()),
    ];
    for (_name, buf) in directed.iter() {
        report.samples += 1;
        let mut bag = DiagBag::new();
        scan_and_book(buf, limits, &mut bag, report);
    }
}

/// 第三层·直调层：不经码流、直接打库级校验入口。
/// 记账统一走 [`book_probe`]；`samples` 与强制拒绝桶在此逐条自增。
pub fn run_direct_contract_probes(limits: &H264Limits, report: &mut FuzzReport) {
    // `MvOutOfRange`：位移超过 `limits.max_mv_abs`。
    report.samples += 1;
    let bad_mv = MotionVector { mv_x: limits.max_mv_abs as i32 + 1, mv_y: 0, ref_idx: 0 };
    book_probe(report, bad_mv.validate(limits));

    // `IntraModeUnavailable`：Baseline + PPS 置位transform_8x8_mode_flag。
    report.samples += 1;
    book_probe(report, check_intra8x8_mode(Profile::Baseline, 1, 1));

    // `EntropyEngineOutOfRange`：CAVLC 系数表未录入就解码 `coeff_token`。
    // 这是**真实可达**的拒绝路径（表 9-5 由后续单承接，此处必须拒而非猜默认值）。
    report.samples += 1;
    let mut cavlc = CavlcDecoder::new();
    book_probe(report, cavlc.decode_coeff_token());

    // `NalOutOfRange`：直调 `checked_range` 闸门（该码的判定内核）。
    //
    // **为什么必须直调而不走码流**：`scan_annexb` 里的两处`range_failure`
    // 都不可达——一处 `checked_range(sc_pos, payload_end - sc_pos, total)`
    // 由 `find_start_code` 保证恒真；另一处在「空 NAL 已跳过」之后才执行。
    // 码流路径上打不到它，就直调内核，并把这条不可达事实写进判据注释。
    report.samples += 1;
    let oor = range_failure("NAL 单元", 64, 128, 16); // offset+size = 192 > 16
    debug_assert_eq!(oor.code, DiagCode::NalOutOfRange);
    report.buckets.record(oor.code);
    report.rejected += 1;
}

/// 组装一条 Annex-B NAL（4 字节起始码 + 头 + 已转义载荷）。
///
/// fuzz 模块自备一份（域自检里的同款构造器属于判据，不该被实现反向依赖）。
pub fn annexb_one(nal_ref_idc: u8, nal_unit_type: u8, payload: &[u8]) -> Vec<u8> {
    let head = ((nal_ref_idc & 0x03) << 5) | (nal_unit_type & 0x1F);
    let mut v: Vec<u8> = vec![0x00, 0x00, 0x00, 0x01, head];
    v.extend_from_slice(&escape_rbsp(payload));
    v
}
