//! VE-F1204 · H.264 解码器（VE-G 域 · G01 视频解码组 · L2/L3）
//!
//! **VE-F1204 · H.264 解码器 · §13-§15 熵解码 / 上下文 / JM 对拍**
//!
//! 由 `veg04_h264.rs` 按职责拆分而来（L5 子模块拆分，行为保持）。
//! 全部符号经 `veg04_h264` 的 `pub use` 再导出，对外路径不变。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::core::{DecodeStats, DiagBag, DiagCode, Failure, H264Limits, R};
use super::nal::{NalUnit, nal_type_is_slice, scan_annexb};
use super::nal::nal_type;
use super::ps::{EntropyEngine, ParameterSetRegistry, parse_pps, parse_sps, select_entropy_engine};
use super::sh::{parse_slice_header};
use super::pix::{Plane};
use super::dpb::{Dpb, DpbBudget, Frame, WorkCounter, h264_frame_bytes};

// §13 熵解码引擎（CAVLC 变长表 + CABAC 算术解码器的**状态语义**）
// ════════════════════════════════════════════════════════════════════════════

/// CAVLC `coeff_token` 的 VLC 表（规范表 9-5 的**可执行子集**）。
///
/// 完整表有 4 个 `nC` 区段×19 项，为避免巨表化，本模块按
/// `TrailingOnes`/`TotalCoeff` 的**解码语义**实现：即
/// 「读若干比特 → 按`(0,0)`/`(0,1)`/… 的前缀树判定」。
///
/// **诚实缺口（必须点名）**：完整的 `coeff_token` VLC 码表、
/// `total_zeros` 表、`run_before` 表（共 3 张大表）**未逐项录入**本模块。
/// 本单交付的是**熵引擎的路由语义 + 状态机 + 边界防护**；
/// 码表本身由 F1204 的后续单（或 F1214fuzz 的语料表）承接。
/// 当前实现对**未录入的码表**一律走 [`CavlcDecoder::decode_coeff_token`]
/// 的显性拒绝分支（`EntropyEngineOutOfRange`），**不猜测、不返回默认值**——
/// 这是零静默纪律在本层的落实：表缺失时拒绝，而非产出错误的系数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CavlcState {
    /// 已读比特数。
    pub bits_read: u64,
    /// 已解出的非零系数个数。
    pub coeffs_decoded: u32,
    /// 已解出的 `TotalZeros`。
    pub total_zeros: u32,
}

impl Default for CavlcState {
    fn default() -> CavlcState {
        CavlcState { bits_read: 0, coeffs_decoded: 0, total_zeros: 0 }
    }
}

/// CAVLC 解码器状态。
#[derive(Clone, Debug)]
pub struct CavlcDecoder {
    /// 状态。
    pub state: CavlcState,
    /// 是否已录入 `coeff_token` 码表（未录入 → 显性拒绝）。
    pub coeff_token_table_ready: bool,
}

impl CavlcDecoder {
    pub fn new() -> CavlcDecoder {
        CavlcDecoder { state: CavlcState::default(), coeff_token_table_ready: false }
    }

    /// 解 `coeff_token`。
    ///
    /// 码表未录入 → **显性拒绝**（`EntropyEngineOutOfRange`），
    /// 附 hint 指明承接单。绝不返回 `TotalCoeff = 0` 冒充成功——
    /// 那会让整帧残差静默归零，解出来的画面是纯预测值而无任何报错。
    pub fn decode_coeff_token(&mut self) -> R<(u8, u32)> {
        if !self.coeff_token_table_ready {
            return Err(Failure::new(
                DiagCode::EntropyEngineOutOfRange,
                "CAVLC coeff_token 码表未录入，拒绝解码".to_string(),
                "H.264 表 9-5 的 4 个 nC 区段码表由 F1204 后续单承接；未录入时必须拒绝而非猜默认值",
            ));
        }
        // 码表录入后的真实解码入口（当前未接入，见上方诚实缺口）。
        Ok((0, 0))
    }

    /// 解 `level_prefix` / `level_suffix`（规范 9.2.1）。
    ///
    /// `level_prefix` 是**一元前缀**：n 个 1 后跟一个 0；超过 14 时用escape。
    /// 此函数实现该**前缀语义**（纯位级，不依赖大表），故可真实执行。
    pub fn decode_level_prefix(&mut self, rbsp: &[u8], bit_pos: &mut usize) -> R<u32> {
        let total = rbsp.len().saturating_mul(8);
        let mut n: u32 = 0;
        loop {
            if *bit_pos >= total {
                return Err(Failure::new(
                    DiagCode::EntropyEngineOutOfRange,
                    "level_prefix 读取越过 RBSP 末尾".to_string(),
                    "位流在变长前缀中结束；拒绝解码",
                ));
            }
            let byte = rbsp.get(*bit_pos >> 3).copied().unwrap_or(0);
            let off = *bit_pos & 7;
            let bit = (byte >> (7 - off)) & 1;
            *bit_pos += 1;
            self.state.bits_read += 1;
            if bit == 0 {
                break;
            }
            n += 1;
            if n > 15 {
                // escape 模式：14 个 1 + 0 之后是 escape 码。
                return Ok(15);
            }
        }
        Ok(n)
    }
}

impl Default for CavlcDecoder {
    fn default() -> CavlcDecoder {
        CavlcDecoder::new()
    }
}

/// CABAC 算术解码器（规范 9.3.4）。
///
/// 本模块实现**算术解码器的状态机与归一化语义**（`codIRange`/`codIOffset`/
/// `renorm`），因为这是「引擎」的本体；上下文模型表（表 9-12..9-44，
/// 共 1024 项）同样属大表，按上述诚实缺口处理。
#[derive(Clone, Debug)]
pub struct CabacDecoder {
    /// 当前码区间 `codIRange`。
    pub cod_i_range: u32,
    /// 当前区间偏移 `codIOffset`。
    pub cod_i_offset: u32,
    /// 上下文模型索引（每个bin 的 ctxIdx）。
    pub ctx_index: u32,
    /// 已读比特数。
    pub bits_read: u64,
    /// 上下文模型表是否已录入。
    pub context_models_ready: bool,
    /// `codIRange` 的合法下界（低于此必须归一化或报错）。
    pub range_floor: u32,
}

impl CabacDecoder {
    /// 初始化（规范 9.3.4.1.1：`codIRange = 510`、`codIOffset` 取前 9 位）。
    pub fn new(rbsp: &[u8]) -> CabacDecoder {
        // 取前 9 位作为初始 offset。
        let mut off: u32 = 0;
        for i in 0..9usize {
            let byte = rbsp.get(i >> 3).copied().unwrap_or(0);
            let bit = (byte >> (7 - (i & 7))) & 1;
            off = (off << 1) | bit as u32;
        }
        CabacDecoder {
            cod_i_range: 510,
            cod_i_offset: off,
            ctx_index: 0,
            bits_read: 9,
            context_models_ready: false,
            range_floor: 2,
        }
    }

    /// 归一化（`renorm`）：区间缩到 `codIRange < 256` 时移入比特。
    ///
    /// 返回移入的比特数（0 表示无需归一化）。
    pub fn renorm(&mut self, rbsp: &[u8], bit_pos: &mut usize) -> R<u32> {
        let total = rbsp.len().saturating_mul(8);
        let mut shifted: u32 = 0;
        while self.cod_i_range < 256 {
            let byte = match rbsp.get(*bit_pos >> 3) {
                Some(v) => *v,
                None => {
                    return Err(Failure::new(
                        DiagCode::EntropyEngineOutOfRange,
                        "CABAC renorm 读取越过 RBSP 末尾".to_string(),
                        "位流在算术解码归一化中结束；拒绝解码（截断）",
                    ))
                }
            };
            let off = *bit_pos & 7;
            let bit = (byte >> (7 - off)) & 1;
            *bit_pos += 1;
            self.bits_read += 1;
            self.cod_i_range <<= 1;
            self.cod_i_offset = (self.cod_i_offset << 1) | bit as u32;
            shifted += 1;
            if *bit_pos >= total && shifted > 32 {
                return Err(Failure::new(
                    DiagCode::EntropyEngineOutOfRange,
                    "CABAC renorm 连续移入超过 32 比特仍不收敛".to_string(),
                    "码区间未收敛；这是损坏或恶意的算术码流，拒绝解码",
                ));
            }
        }
        Ok(shifted)
    }

    /// 解一个 bin（`DecodeDecision` 的范围缩放语义）。
    ///
    /// 上下文模型表未录入 → **显性拒绝**，不返回默认 bin 值
    /// （默认 bin 会让整帧熵解码结果全错且**不报任何错**）。
    pub fn decode_bin(&mut self, rbsp: &[u8], bit_pos: &mut usize) -> R<u8> {
        if !self.context_models_ready {
            return Err(Failure::new(
                DiagCode::EntropyEngineOutOfRange,
                format!("CABAC 上下文模型表未录入（ctxIdx {}），拒绝解码", self.ctx_index),
                "H.264 表 9-12..9-44 的 1024 项上下文初始化表由 F1204 后续单承接；未录入时必须拒绝而非猜默认 bin",
            ));
        }
        // 表录入后的真实解码入口（当前未接入，见上方诚实缺口）。
        let _ = rbsp;
        let _ = bit_pos;
        Ok(0)
    }
}

/// 熵引擎状态（解码上下文的一部分）。
#[derive(Clone, Debug)]
pub enum EntropyState {
    /// CAVLC 状态。
    Cavlc(CavlcDecoder),
    /// CABAC 状态。
    Cabac(CabacDecoder),
}

impl EntropyState {
    /// 按 PPS 的`entropy_coding_mode_flag` 建立引擎状态。
    pub fn select(entropy_coding_mode_flag: u8, rbsp: &[u8]) -> R<EntropyState> {
        match select_entropy_engine(entropy_coding_mode_flag) {
            EntropyEngine::Cavlc => Ok(EntropyState::Cavlc(CavlcDecoder::new())),
            EntropyEngine::Cabac => Ok(EntropyState::Cabac(CabacDecoder::new(rbsp))),
        }
    }

    pub fn engine(&self) -> EntropyEngine {
        match self {
            EntropyState::Cavlc(_) => EntropyEngine::Cavlc,
            EntropyState::Cabac(_) => EntropyEngine::Cabac,
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// §14 解码上下文（参数集 + DPB + 熵引擎状态 —— 锚点指定的单一数据结构）
// ════════════════════════════════════════════════════════════════════════════

/// 解码上下文（锚点「数据结构：解码上下文（参数集+DPB+熵引擎状态）」）。
#[derive(Clone, Debug)]
pub struct DecoderContext {
    /// 参数集登记。
    pub parameter_sets: ParameterSetRegistry,
    /// 参考帧缓冲。
    pub dpb: Dpb,
    /// 当前激活的参数集 id。
    pub active_sps_id: u32,
    pub active_pps_id: u32,
    /// 当前熵引擎状态。
    pub entropy: Option<EntropyState>,
    /// 统计。
    pub stats: DecodeStats,
    /// 工作计数（性能自检的真实度量）。
    pub work: WorkCounter,
    /// 预算。
    pub budget: DpbBudget,
}

impl DecoderContext {
    /// 新建解码上下文。
    pub fn new(limits: &H264Limits) -> DecoderContext {
        DecoderContext {
            parameter_sets: ParameterSetRegistry::new(),
            dpb: Dpb::new(),
            active_sps_id: 0,
            active_pps_id: 0,
            entropy: None,
            stats: DecodeStats::default(),
            work: WorkCounter::new(),
            budget: DpbBudget::from_limits(limits),
        }
    }

    /// 登记一个 NAL（自动按类型分派到 SPS/PPS/切片）。
    /// 主体只保留「按类型分派」骨架，各分支语义下沉到
    /// [`DecoderContext::accept_sps`] / [`DecoderContext::accept_pps`] /
    /// [`DecoderContext::accept_slice`]。
    pub fn accept_nal(&mut self, unit: &NalUnit, limits: &H264Limits, bag: &mut DiagBag) -> R<()> {
        match unit.header.nal_unit_type {
            nal_type::SPS | nal_type::SPS_EXT => self.accept_sps(unit, limits, bag),
            nal_type::PPS => self.accept_pps(unit, limits),
            t if nal_type_is_slice(t) => self.accept_slice(unit, limits, bag),
            _ => {
                // 非解码类型：宽容跳过，已在 scan_annexb 计数。
                Ok(())
            }
        }
    }

    /// SPS/SPS_EXT 分支：解析 → 参数集登记 → 热更新计数 → 帧字节数随几何更新。
    fn accept_sps(&mut self, unit: &NalUnit, limits: &H264Limits, bag: &mut DiagBag) -> R<()> {
        let sps = parse_sps(&unit.rbsp, limits)?;
        let outcome = self.parameter_sets.register_sps(sps)?;
        if outcome.is_hot_update() {
            self.stats.hot_updates += 1;
            bag.push(
                DiagCode::ParameterSetGeometryChange,
                format!(
                    "SPS {} 热更新（几何一致：{}x{}）",
                    sps.seq_parameter_set_id,
                    sps.width_mbs(),
                    sps.height_mbs()
                ),
                "同 id 且几何参数一致，允许热更新：分辨率/参考帧数/熵编码模式可变，DPB 保留",
            );
        } else {
            self.stats.sps_registered += 1;
        }
        self.active_sps_id = sps.seq_parameter_set_id;
        // 帧字节数随 SPS 更新（几何一致，故不需 flush DPB）。
        if let Some(b) = h264_frame_bytes(&sps) {
            self.dpb.set_frame_bytes(b);
        }
        Ok(())
    }

    /// PPS 分支：解析 → 参数集登记 → 热更新计数。
    fn accept_pps(&mut self, unit: &NalUnit, limits: &H264Limits) -> R<()> {
        let pps = parse_pps(&unit.rbsp, limits)?;
        let outcome = self.parameter_sets.register_pps(pps.clone())?;
        if outcome.is_hot_update() {
            self.stats.hot_updates += 1;
        } else {
            self.stats.pps_registered += 1;
        }
        self.active_pps_id = pps.pic_parameter_set_id;
        Ok(())
    }

    /// 切片分支：参数集齐备检查 → 预算前置闸 → 切片头解析 → 熵引擎按 **PPS** 切换
    ///（见头注要点六）。
    fn accept_slice(&mut self, unit: &NalUnit, limits: &H264Limits, bag: &mut DiagBag) -> R<()> {
        let sps = match self.parameter_sets.sps(self.active_sps_id) {
            Some(v) => *v,
            None => {
                return Err(Failure::new(
                    DiagCode::SpsPpsRefMissing,
                    format!("切片到达时SPS {} 未登记", self.active_sps_id),
                    "切片依赖参数集；先登记 SPS/PPS 再解切片",
                ))
            }
        };
        let pps = match self.parameter_sets.pps(self.active_pps_id) {
            Some(v) => v,
            None => {
                return Err(Failure::new(
                    DiagCode::SpsPpsRefMissing,
                    format!("切片到达时 PPS {} 未登记", self.active_pps_id),
                    "切片依赖参数集；先登记 SPS/PPS 再解切片",
                ))
            }
        };
        // **预算前置校验**：整帧字节超预算即拒绝，绝不先分配再判断。
        if let Some(need) = h264_frame_bytes(&sps) {
            if need > self.budget.max_bytes {
                self.stats.rejected += 1;
                return Err(Failure::new(
                    DiagCode::DpbOverflow,
                    format!(
                        "SPS 声称的帧尺寸需 {} 字节，超过预算 {}",
                        need, self.budget.max_bytes
                    ),
                    "恶意分辨率防护（SPS 声称 64K 分辨率时必须在此被拒）；不分配、不降级、不静默截断",
                ));
            }
            self.dpb.set_frame_bytes(need);
        }
        let mut local_bag = DiagBag::new();
        let slice = parse_slice_header(
            &unit.rbsp,
            &sps,
            pps,
            &unit.header,
            limits,
            &mut local_bag,
        )?;
        for d in local_bag.all() {
            bag.push(d.code, d.message.clone(), &d.hint);
        }
        self.entropy = Some(EntropyState::select(pps.entropy_coding_mode_flag, &unit.rbsp)?);
        self.stats.slices_decoded += 1;
        self.stats.epb_stripped += unit.epb_stripped as u64;
        let _ = slice;
        Ok(())
    }

    /// 把一帧放入 DPB（受预算防护）。
    pub fn output_frame(&mut self, frame: Frame) -> R<()> {
        self.dpb.push_frame(frame, &self.budget)?;
        self.stats.frames_output += 1;
        Ok(())
    }
}

// ════════════════════════════════════════════════════════════════════════════
// §15 JM 参考解码器对拍（逐像素 ≤1 LSB）
// ════════════════════════════════════════════════════════════════════════════

/// JM 参考解码器输出的一帧事实（fixture 形态的外部事实）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JmFrameFacts {
    /// 宽度（像素）。
    pub width: u32,
    /// 高度（像素）。
    pub height: u32,
    /// 帧序号。
    pub frame_num: u32,
    /// 帧类型（0=I、1=P、2=B）。
    pub frame_type: u8,
    /// 参考帧个数（含当前帧）。
    pub ref_frames: u32,
    /// 熵引擎名（"CAVLC" / "CABAC"）。
    pub entropy_coding_mode: String,
}

/// 对拍分歧项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CrossDivergence {
    /// 分歧字段名。
    pub field: String,
    /// 本模块的值。
    pub ours: String,
    /// JM 的值。
    pub theirs: String,
}

/// 逐像素比较两帧，返回超过容差的像素位置。
///
/// 容差为 **≤1 LSB**（锚点判据）。注意是「**不超过** 1」——
/// 差 1 通过、差 2 判红。
pub fn pixel_diff_within_lsb(a: &Plane, b: &Plane, tolerance: i32) -> Vec<(i32, i32, i32)> {
    let mut out: Vec<(i32, i32, i32)> = Vec::new();
    if a.width != b.width || a.height != b.height {
        return vec![(-1, -1, -1)];
    }
    for y in 0..a.height as i32 {
        for x in 0..a.width as i32 {
            let va = a.at(x, y).unwrap_or(0) as i32;
            let vb = b.at(x, y).unwrap_or(0) as i32;
            let d = (va - vb).abs();
            if d > tolerance {
                out.push((x, y, d));
            }
        }
    }
    out
}

/// 与 JM 的**逐像素对拍**（锚点判据「JM 参考解码器对拍，逐像素 ≤1 LSB」）。
///
/// 容差常量 [`JM_PIXEL_TOLERANCE_LSB`] = 1。
pub fn cross_check_jm(
    ours: &Frame,
    theirs_luma: &Plane,
    ours_meta: (&u32, u32),
    facts: &JmFrameFacts,
) -> Vec<CrossDivergence> {
    let mut div: Vec<CrossDivergence> = Vec::new();
    if facts.width != ours.luma.width {
        div.push(CrossDivergence {
            field: "width".to_string(),
            ours: ours.luma.width.to_string(),
            theirs: facts.width.to_string(),
        });
    }
    if facts.height != ours.luma.height {
        div.push(CrossDivergence {
            field: "height".to_string(),
            ours: ours.luma.height.to_string(),
            theirs: facts.height.to_string(),
        });
    }
    if ours_meta.0 != &facts.frame_num {
        div.push(CrossDivergence {
            field: "frame_num".to_string(),
            ours: ours_meta.0.to_string(),
            theirs: facts.frame_num.to_string(),
        });
    }
    // 逐像素：容差 ≤1 LSB。
    let diffs = pixel_diff_within_lsb(&ours.luma, theirs_luma, JM_PIXEL_TOLERANCE_LSB);
    if !diffs.is_empty() {
        let max_d = diffs.iter().map(|(_, _, d)| *d).max().unwrap_or(0);
        div.push(CrossDivergence {
            field: format!("luma_pixels(>{}LSB)", JM_PIXEL_TOLERANCE_LSB),
            ours: format!("{} pixels off, max {}", diffs.len(), max_d),
            theirs: "0 pixels off".to_string(),
        });
    }
    div
}

/// JM 对拍的像素容差（LSB）。
pub const JM_PIXEL_TOLERANCE_LSB: i32 = 1;

// ════════════════════════════════════════════════════════════════════════════
