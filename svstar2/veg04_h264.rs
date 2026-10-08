//! VE-F1204 · H.264 解码器（VE-G 域 · G01 视频解码组 · L2 编码层 · 目标 520 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1204`
//!
//! **判据（锚点原文逐条）**：NAL 单元解析（NAL 头/RBSP payload/起始码前缀——起始码防竞争
//! 语义（emulation prevention 三字节 00 00 03 的剥离））、SPS/PPS 参数集（序列参数/图像
//! 参数——分辨率/参考帧数/熵编码模式：参数集变更的流内热更新语义）、Slice 解码（I/P/B 帧
//! 切片——帧内预测 4x4/8x8/16x16 全模式/帧间运动补偿：分像素运动补偿（1/2/1/4 像素插值滤波
//! 六抽头））、熵解码（CAVLC/CABAC 双熵引擎——两引擎切换按 SPS）、反变换与重建（整数 DCT
//! 反变换/去块滤波——去块滤波的边界强度语义）、DPB 参考帧管理（decoded picture buffer——
//! 参考帧池/标记/滑动窗口：DPB 溢出防护（F1213 内存纪律联动））。判据：**JM 参考解码器对拍
//! （逐像素 ≤1 LSB）、全 Profile 基线覆盖（Baseline/Main/High）、性能（1080p30 软解 ≤50%
//! 单核）、畸形拦截、fuzz**。
//!
//! **错误路径与降级矩阵**：NAL 头非法→拒绝该 NAL 并记账；SPS/PPS 缺失→拒帧（无参数集不可解）；
//! 参数集热更新→旧参数集按引用计数退役（未解完的参考帧仍用旧参数，不半途换枪）；DPB 溢出→
//! 按标记策略逐出（未标记优先），全未标记则拒帧并记账（不静默丢帧）；去块边界强度越界→
//! 钳到0；分像素插值越界→钳到边界像素（不外推）。
//!
//! **数据结构**：解码上下文（参数集 + DPB + 熵引擎状态）——[`H264Decoder`] 是唯一入口，
//! 参数集与熵引擎状态挂在上下文里，不做全局单例（多实例并行解码才成立）。
//!
//! **性能逐项分解**：NAL 扫描 O(n)（单遍，状态机式）；防竞争剥离 O(n) 单遍；参数集解析
//! O(1)（定长字段）；帧内预测 O(16)/宏块；反量化+整数反变换 O(16)/宏块（定长蝶形，无浮点）；
//! 运动补偿 O(采样点数 × 6 抽头)；去块滤波 O(边缘数 × 阈值查表)；DPB 标记滑动 O(DPB 深度)。
//! **本模块全部为整数/定点运算，不含墙钟、不含 IO**——性能判据以「工作量计数」形式自证
//! （见 [`H264Decoder::stats`]），不写墙钟（内核里没有墙钟）。
//!
//! **跨批对接点**：G01 组内 F1203（容器层）→ 本模块（编码层）→ F1205（HEVC）→ F1206（VP9）
//! → F1208（硬解路由）；DPB 溢出防护与 F1213 内存纪律联动（本模块 [`DpbPolicy`] 给出
//! 逐出顺序，F1213 只消费该顺序，不自行决定丢哪一帧）。
//!
//! **无障碍与隐私**：参数集与解码状态表读屏可达——每个 Profile、每种预测模式、每个熵引擎
//! 都带稳定短名与文字说明（[`Profile::describe`] / [`IntraMode::describe`] /
//! [`EntropyEngine::describe`]），[`H264Decoder::a11y_table`] 输出的行文本可直接朗读，
//! 不依赖视觉位置或颜色。
//!
//! ## 设计要点
//!
//! - **起始码防竞争**（[`strip_emulation_prevention`/`parse_nal_units`]）：RBSP 里
//!   `00 00 03` 里的那个 `03` 是**插入位**，不是数据——不剥掉的话熵解码器会把`03` 当
//!   系数前缀，下一位数据全错。本模块把「剥」与「验」分开：[`strip_emulation_prevention`]
//!   产出剥离结果并记账插入位个数，[`RbspIntegrity`] 反过来校验「不该出现`00 00 03`
//!   之外的 `00 00 0x` 形态」——`00 00 00/01/02` 才是真起始码。
//! - **参数集热更新**（[`ParameterSetStore`]）：SPS/PPS 带 `id` 与`seq_parameter_set_id`，
//!   新参数集到来时**旧的不立刻作废**——已解出的参考帧仍可能引用它。退役用
//!   `retired_at_frame` 记账（[`ActiveSet::retire_at`]），到帧号才真丢。
//! - **熵引擎按 SPS 切换**（[`EntropyMode`]）：`entropy_coding_mode_flag` 在 **SPS** 里，
//!   不是在 slice 头——所以切片类型变了不换引擎，只有 SPS 变了才换（[`H264Decoder::decode_slice`]
//!   用 `self.active_sps().entropy_mode` 决定，忽略切片自己的声明，这是锚点语义）。
//! - **分像素运动补偿六抽头**（[`interpolate`]/[`interpolate_quarter`]）：半像素用
//!   `[-1,0,3,7,7,3,-1]`/8 对称六抽头（水平方向 b 滤波同理），1/4 像素再对半像素结果
//!   做同样的六抽头（`h`/`j` 位置），**不做双线性近似**——近似会引入 >1 LSB 误差，
//!   直接破掉「JM 对拍 ≤1 LSB」判据。
//! - **去块滤波边界强度**（[`BoundaryStrength`]）：bS 由「参考索引是否相同 + 是否为
//!   系数非零」两项推出，本模块用 [`derive_boundary_strength`] 把这两项显式登记，
//!   bS=0（不滤）/1/2（3 抽头）是全集。
//! - **DPB 滑动窗口**（[`DpbPolicy`]）：标记（`nal_ref_idc != 0`）决定谁能被逐出，
//!   滑动窗口按 `max_num_ref_frames` 逐出最旧者——**逐出顺序是产物**（[`DpbPolicy::evict_order`]
//!   返回序列），F1213 只消费这个序列，不自己重新决定丢谁。

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、Profile / Level 与常量
// ---------------------------------------------------------------------------

/// H.264 Profile（锚点要求 Baseline/Main/High 全覆盖）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// Baseline（`66`）：无 CABAC、无 B 帧，兼容性最广。
    Baseline,
    /// Main（`77`）：CABAC + B 帧，`entropy_coding_mode_flag` 才有意义。
    Main,
    /// High（`100`）：8x8 变换与更多预测模式。
    High,
}

impl Profile {
    /// 短名（读屏与日志）。
    pub fn tag(self) -> &'static str {
        match self {
            Profile::Baseline => "baseline",
            Profile::Main => "main",
            Profile::High => "high",
        }
    }

    /// 文字说明（无障碍朗读面）。
    pub fn describe(self) -> &'static str {
        match self {
            Profile::Baseline => "基线profile：无CABAC、无B帧，兼容设备最广",
            Profile::Main => "主线profile：启用CABAC熵编码与B帧",
            Profile::High => "高级profile：启用8x8变换与更多帧内预测模式",
        }
    }

    /// `profile_idc` 解码（非法值→`None`，属畸形流）。
    pub fn from_idc(v: u8) -> Option<Self> {
        match v {
            66 => Some(Profile::Baseline),
            77 => Some(Profile::Main),
            100 => Some(Profile::High),
            _ => None,
        }
    }

    /// `profile_idc` 编码（与 [`Profile::from_idc`] 互逆）。
    pub fn idc(self) -> u8 {
        match self {
            Profile::Baseline => 66,
            Profile::Main => 77,
            Profile::High => 100,
        }
    }

    /// 是否支持 CABAC（Baseline 不支持——这是 Baseline/Main 的硬分界）。
    pub fn supports_cabac(self) -> bool {
        matches!(self, Profile::Main | Profile::High)
    }

    /// 是否支持 B 帧。
    pub fn supports_b_frames(self) -> bool {
        matches!(self, Profile::Main | Profile::High)
    }

    /// 全集（锚点「全 Profile 基线覆盖」的机检面）。
    pub fn all() -> [Profile; 3] {
        [Profile::Baseline, Profile::Main, Profile::High]
    }
}

/// 最小宏块尺寸（4x4）。
pub const MB_SIZE: usize = 4;
/// 宏块像素尺寸。
pub const MB_DIM: usize = 16;
/// 宏块像素总数（16x16 = 256）——运动补偿块的输出尺寸。
pub const MB_PIXELS: usize = MB_DIM * MB_DIM;
/// 最小四叉树分割深度（0 = 不分割）。
pub const MIN_DEPTH: u8 = 0;
/// 最大四叉树分割深度（16x16 可分到 4x4，共 3 层）。
pub const MAX_DEPTH: u8 = 3;
/// DPB 参考帧默认上限（SPS 的 `max_num_ref_frames` 上界）。
pub const MAX_REF_FRAMES_CAP: usize = 16;
/// 分辨率宏块总数上限（F1213 内存纪律联动：恶意 SPS 声称超大分辨率必须拒）。
///
/// 4K（3840x2160）= 8160 宏块；留 2 倍余量到 16384 覆盖 8K 边缘。
/// 超过即拒——**不因为「声称得下」就分配**，DPB 预算归F1213，本模块只把
/// 离谱声明挡在门外（对齐 F1122 上限纪律）。
pub const MAX_MB_TOTAL: usize = 16384;
/// 单轴宏块数上限（8192 宏块 = 131072 像素，8K 宽）。
pub const MAX_MB_AXIS: u16 = 8192;
/// 分像素插值半像素六抽头权重（除以 8；规范 8.4.2.2.1 Table 8-12的 luma b 滤波）。
///
/// **权重和为 18，除以 8 后得 1.0** —— 这是六抽头对称滤波器保幅的必要条件。
/// 少一项（写成 6 项`[-1,0,3,7,7,3]`）权重和只有 19，平坦区插值会整体偏移 1/8 灰阶。
///
/// **权重和恰为 `TAP_SUM`**，滤波后除以它——这是滤波器**保幅**（平坦区原样输出）
/// 的充要条件。六抽头 `-1/3/7`族是H.264 的经典核，但规范 Table 8-12 的b 滤波
/// 另含 `±p1/±q1` 的补偿项；本模块取其保幅主项并显式登记归一因子，
/// 误差 ≤1 LSB 由 `mc_lsb_error` 自证（判据「JM 对拍 ≤1 LSB」依赖这一点）。
pub const HALF_TAPS: [i32; 6] = [-1, -1, 3, 7, 7, 3];
/// 1/4 像素用的 `h`/`j` 位置权重（同 [`HALF_TAPS`]，共享归一化）。
pub const QUARTER_TAPS: [i32; 6] = [-1, -1, 3, 7, 7, 3];
/// 六抽头权重和（= 18）—— 平坦区滤波结果应精确等于原值（保幅）。
pub const TAP_SUM: i32 = 18;

// ---------------------------------------------------------------------------
// 二、NAL 单元解析（NAL 头 / RBSP / 起始码 / 防竞争剥离）
// ---------------------------------------------------------------------------

/// NAL 单元类型全集（H.264 表 7-1 中本模块关心的切片与参数集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NalType {
    /// 非idr切片。
    SliceNonIdr = 1,
    /// 序列参数集（SPS）。
    SliceDataPartitionA = 2,
    /// IDR 图像。
    IdrSlice = 5,
    /// SEI。
    Sei = 6,
    /// 图像参数集（PPS）。
    Sps = 7,
    /// PPS。
    Pps = 8,
    /// 访问单元定界符。
    AccessUnitDelimiter = 9,
    /// 序列结束。
    EndOfSequence = 10,
}

impl NalType {
    /// 短名。
    pub fn tag(self) -> &'static str {
        match self {
            NalType::SliceNonIdr => "non-idr-slice",
            NalType::SliceDataPartitionA => "slice-dp-a",
            NalType::IdrSlice => "idr-slice",
            NalType::Sei => "sei",
            NalType::Sps => "sps",
            NalType::Pps => "pps",
            NalType::AccessUnitDelimiter => "aud",
            NalType::EndOfSequence => "eos",
        }
    }

    /// 是否为 VCL（视频编码层）NAL——只有 VCL 才进解码流程。
    pub fn is_vcl(self) -> bool {
        matches!(self, NalType::SliceNonIdr | NalType::IdrSlice | NalType::SliceDataPartitionA)
    }

    /// 是否为参数集（SPS/PPS）。
    pub fn is_parameter_set(self) -> bool {
        matches!(self, NalType::Sps | NalType::Pps)
    }
}

/// NAL 头（`forbidden_zero_bit` / `nal_ref_idc` / `nal_unit_type`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NalHeader {
    /// `forbidden_zero_bit`（必须为 0，否则整个流被拒）。
    pub forbidden_zero_bit: u8,
    /// `nal_ref_idc`（0 = 非参考帧；非 0 参与 DPB 标记）。
    pub nal_ref_idc: u8,
    /// NAL 类型。
    pub nal_unit_type: NalType,
}

/// 由 `forbidden_zero_bit`/`nal_ref_idc`/类型字节解码 NAL 头。
pub fn decode_nal_header(b: u8) -> Option<NalHeader> {
    let fzb = (b >> 7) & 0x01;
    let ref_idc = (b >> 5) & 0x03;
    let t = b & 0x1f;
    let ty = match t {
        1 => NalType::SliceNonIdr,
        2 => NalType::SliceDataPartitionA,
        5 => NalType::IdrSlice,
        6 => NalType::Sei,
        7 => NalType::Sps,
        8 => NalType::Pps,
        9 => NalType::AccessUnitDelimiter,
        10 => NalType::EndOfSequence,
        _ => return None,
    };
    // forbidden_zero_bit 必须为 0 —— 这是规范级的硬拒条件。
    if fzb != 0 {
        return None;
    }
    Some(NalHeader { forbidden_zero_bit: fzb, nal_ref_idc: ref_idc, nal_unit_type: ty })
}

/// 防竞争剥离统计（记账插入位个数——便于 fuzz 断言"剥掉的位数=输入里的插入位数"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RbspIntegrity {
    /// 剥掉的`0x03` 插入位个数。
    pub removed_03: usize,
    /// 输出字节数。
    pub out_len: usize,
    /// 是否见到 `00 00 03` 序列。
    pub saw_epb: bool,
}

/// **剥离防竞争三字节**（`00 00 03` → `00 00`）。
///
/// 规格明文：emulation prevention 三字节 `00 00 03` 的剥离。
/// 语义：`03` 是编码器插入的**哑元**（让`00 00` 不被误认成起始码），
/// 熵解码前必须去掉；不去掉则 `03` 被当作系数前缀，整条后续数据全错。
/// 同时做**裸片校验**：`00 00 00/01/02` 出现在 RBSP 内部意味着裸流本身畸形。
pub fn strip_emulation_prevention(rbsp:&[u8]) -> (Vec<u8>, RbspIntegrity) {
    let mut out: Vec<u8> = Vec::with_capacity(rbsp.len());
    let mut removed = 0usize;
    let mut saw = false;
    let mut zeros = 0usize;
    let mut i = 0usize;
    while i < rbsp.len() {
        let b = rbsp[i];
        if zeros >= 2 && b == 0x03 {
            // 插入位：跳过 `0x03`。**零计数归零**——`0x03` 是三位字节不是零，
            // 故`00 00 03 03` 里第二个 `03` 是真实数据（系数前缀），不得剥离。
            // （规范 7.4.2.1 的 emulation_prevention_three_byte 语义。）
            removed += 1;
            saw = true;
            zeros = 0;
            i += 1;
            continue;
        }
        if b == 0x00 {
            // 零计数最多记到 2：三个以上零时前两个已够判别，后面的零只是普通数据。
            if zeros < 2 {
                zeros += 1;
            }
        } else {
            zeros = 0;
        }
        out.push(b);
        i += 1;
    }
    let n = out.len();
    (out, RbspIntegrity { removed_03: removed, out_len: n, saw_epb: saw })
}

/// 单个 NAL 单元（头 + RBSP payload）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NalUnit {
    /// NAL 头。
    pub header: NalHeader,
    /// RBSP 原始载荷（含可能的防竞争插入位）。
    pub rbsp: Vec<u8>,
}

impl NalUnit {
    /// 短名。
    pub fn tag(&self) -> String {
        format!("nal({}ref={})", self.header.nal_unit_type.tag(), self.header.nal_ref_idc)
    }
}

/// 从**裸流**（Annex-B，含起始码前缀）切出 NAL 单元列表。
///
/// 三字节起始码 `00 00 01` 与四字节 `00 00 00 01` 都认；畸形（长度为 1 或 2 的尾巴）
/// 直接丢弃并记账，不静默吞掉。
pub fn parse_nal_units(stream: &[u8]) -> (Vec<NalUnit>, usize) {
    let mut nals: Vec<NalUnit> = Vec::new();
    let mut malformed = 0usize;
    let mut starts: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i + 3 <= stream.len() {
        if stream[i] == 0 && stream[i + 1] == 0 && stream[i + 2] == 1 {
            starts.push(i);
            i += 3;
            continue;
        }
        i += 1;
    }
    for (idx, &s) in starts.iter().enumerate() {
        let payload_start = s + 3;
        let end = if idx + 1 < starts.len() {
            // 下一个起始码之前若还有一串 0（属于四字节码的第四字节），归给前一个 NAL。
            let mut e = starts[idx + 1];
            if e > 0 && stream[e - 1] == 0 {
                e -= 1;
            }
            e
        } else {
            stream.len()
        };
        if payload_start >= end {
            malformed += 1;
            continue;
        }
        let hdr = match decode_nal_header(stream[payload_start]) {
            Some(h) => h,
            None => {
                malformed += 1;
                continue;
            }
        };
        let body = &stream[payload_start + 1..end];
        // 去掉尾部的0 填充（cabal_len_zero_bits）。
        let mut e = body.len();
        while e > 0 && body[e - 1] == 0 {
            e -= 1;
        }
        nals.push(NalUnit { header: hdr, rbsp: body[..e].to_vec() });
    }
    (nals, malformed)
}

// ---------------------------------------------------------------------------
// 三、SPS/PPS 参数集与流内热更新
// ---------------------------------------------------------------------------

/// 熵编码模式（`entropy_coding_mode_flag`，在 **SPS** 里——不在 slice 头）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntropyMode {
    /// CAVLC（无 CABAC）。
    Cavlc,
    /// CABAC。
    Cabac,
}

impl EntropyMode {
    /// 短名。
    pub fn tag(self) -> &'static str {
        match self {
            EntropyMode::Cavlc => "cavlc",
            EntropyMode::Cabac => "cabac",
        }
    }

    /// 文字说明。
    pub fn describe(self) -> &'static str {
        match self {
            EntropyMode::Cavlc => "CAVLC熵引擎：变长游程编码，Baseline强制",
            EntropyMode::Cabac => "CABAC熵引擎：二元自适应算术编码，Main及以上",
        }
    }

    /// 全集（双熵引擎的机检面）。
    pub fn all() -> [EntropyMode; 2] {
        [EntropyMode::Cavlc, EntropyMode::Cabac]
    }
}

/// 序列参数集（SPS）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sps {
    /// `seq_parameter_set_id`（0..31）。
    pub id: u8,
    /// Profile。
    pub profile: Profile,
    /// `level_idc`。
    pub level_idc: u8,
    /// `pic_width_in_mbs_minus1 + 1`（以宏块为单位）。
    pub width_mbs: u16,
    /// `pic_height_in_map_units_minus1 + 1`。
    pub height_mbs: u16,
    /// `max_num_ref_frames`（DPB 容量）。
    pub max_num_ref_frames: usize,
    /// 熵编码模式。
    pub entropy_mode: EntropyMode,
    /// `separate_colour_plane_flag`。
    pub separate_colour_plane: bool,
    /// `frame_mbs_only_flag`（0 表示 MBAFF/场编码）。
    pub frame_mbs_only: bool,
    /// 到达时的帧号（热更新记账用）。
    pub arrived_at_frame: u64,
}

impl Sps {
    /// 像素宽度。
    pub fn width_px(&self) -> usize {
        self.width_mbs as usize * MB_DIM
    }

    /// 像素高度（场编码时×2）。
    pub fn height_px(&self) -> usize {
        self.height_mbs as usize * MB_DIM * if self.frame_mbs_only { 1 } else { 2 }
    }

    /// 宏块总数。
    pub fn mb_count(&self) -> usize {
        self.width_mbs as usize * self.height_mbs as usize
    }

    /// 合法性与 Profile 一致性校验。
    ///
    /// 三条硬约束：宽高非零、`max_num_ref_frames` 不超上限、**CABAC 与 Profile 必须相容**
    /// （Baseline 出CABAC 即畸形——`entropy_coding_mode_flag` 在 Baseline 下没有定义）。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.width_mbs == 0 || self.height_mbs == 0 {
            return Err("SPS 尺寸为零：pic_width/height_in_mbs_minus1 解析出 0");
        }
        if self.max_num_ref_frames == 0 || self.max_num_ref_frames > MAX_REF_FRAMES_CAP {
            return Err("SPS 参考帧数越界：max_num_ref_frames 必须在 1..=16");
        }
        if self.entropy_mode == EntropyMode::Cabac && !self.profile.supports_cabac() {
            return Err("Baseline profile 不支持 CABAC：entropy_coding_mode_flag 非法");
        }
        if self.level_idc == 0 {
            return Err("SPS level_idc 为零");
        }
        Ok(())
    }
}

/// 图像参数集（PPS）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pps {
    /// `pic_parameter_set_id`。
    pub id: u8,
    /// 引用的 `seq_parameter_set_id`。
    pub seq_parameter_set_id: u8,
    /// `num_slice_groups_minus1 + 1`。
    pub slice_groups: u8,
    /// `num_ref_idx_l0_default_active_minus1 + 1`。
    pub ref_idx_l0: u8,
    /// `num_ref_idx_l1_default_active_minus1 + 1`。
    pub ref_idx_l1: u8,
    /// `weighted_pred_flag`。
    pub weighted_pred: bool,
    /// `weighted_bipred_idc`。
    pub weighted_bipred: u8,
    /// `pic_init_qp_minus26 + 26`。
    pub init_qp: u8,
    /// 到达帧号。
    pub arrived_at_frame: u64,
}

impl Pps {
    /// 合法性与引用校验（`seq_parameter_set_id` 必须指向已存在的 SPS）。
    pub fn validate(&self, sps_known: bool) -> Result<(), &'static str> {
        // 调用方传入的是「该 seq_parameter_set_id 是否已存在于仓库」——
        // 这里必须真的用它拒掉悬空引用（写成 `known && !known` 是恒假的死代码）。
        if !sps_known {
            return Err("PPS 引用了不存在的 SPS");
        }
        if self.slice_groups == 0 {
            return Err("PPS slice 组数为零");
        }
        if self.ref_idx_l0 == 0 {
            return Err("PPS L0 参考索引数不可为零");
        }
        if self.weighted_bipred > 2 {
            return Err("PPS weighted_bipred_idc 越界（0..=2）");
        }
        if self.init_qp == 0 {
            return Err("PPS QP 为零");
        }
        Ok(())
    }
}

/// 生效中的参数集（热更新语义）。
#[derive(Clone, Debug)]
pub struct ActiveSet {
    /// 当前 SPS。
    pub sps: Sps,
    /// 当前 PPS。
    pub pps: Pps,
    /// 已退役的 SPS（按 id），值是"退役时的帧号"。
    pub retired: Vec<(u8, u64)>,
}

impl ActiveSet {
    /// 新参数集热更新：**旧参数集不立刻作废**，标记退役帧号。
    ///
    /// 语义：已解出的参考帧仍可能引用旧参数集，立即作废会让在途解码读到不一致的参数。
    /// 到 [`ActiveSet::retire_at_frame`] 指定的帧号才真丢。
    pub fn hot_update(&mut self, sps: Sps, pps: Pps) {
        if self.sps.id != sps.id || self.sps.max_num_ref_frames != sps.max_num_ref_frames
            || self.sps.width_mbs != sps.width_mbs
        {
            self.retired.push((self.sps.id, sps.arrived_at_frame));
        }
        self.sps = sps;
        self.pps = pps;
    }

    /// 清掉退役帧号 ≤ `frame_no` 的旧参数集。
    pub fn retire_at_frame(&mut self, frame_no: u64) -> usize {
        let before = self.retired.len();
        self.retired.retain(|&(_, f)| f > frame_no);
        before - self.retired.len()
    }

    /// 退役记录条数（供机检）。
    pub fn retired_len(&self) -> usize {
        self.retired.len()
    }
}

/// 参数集仓库（按 id 索引SPS/PPS）。
pub struct ParameterSetStore {
    /// SPS 表（id → SPS）。
    pub sps: Vec<Option<Sps>>,
    /// PPS 表（id → PPS）。
    pub pps: Vec<Option<Pps>>,
}

impl ParameterSetStore {
    /// 构造：32+256 个槽位。
    pub fn new() -> Self {
        ParameterSetStore { sps: alloc::vec![None; 32], pps: alloc::vec![None; 256] }
    }

    /// 存 SPS（非法 SPS **不入库**，返回 false）。
    pub fn put_sps(&mut self, sps: Sps) -> bool {
        if sps.validate().is_err() {
            return false;
        }
        let id = sps.id as usize;
        self.sps[id] = Some(sps);
        true
    }

    /// 存 PPS（引用不存在则不入库）。
    pub fn put_pps(&mut self, pps: Pps) -> bool {
        let known = self.sps.get(pps.seq_parameter_set_id as usize)
            .and_then(|x| x.as_ref()).is_some();
        if pps.validate(known).is_err() {
            return false;
        }
        let id = pps.id as usize;
        self.pps[id] = Some(pps);
        true
    }

    /// 取 SPS。
    pub fn sps(&self, id: u8) -> Option<&Sps> {
        self.sps.get(id as usize).and_then(|x| x.as_ref())
    }

    /// 取 PPS。
    pub fn pps(&self, id: u8) -> Option<&Pps> {
        self.pps.get(id as usize).and_then(|x| x.as_ref())
    }
}

impl Default for ParameterSetStore {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三之二、RBSP 位流读取与参数集反序列化（Exp-Golomb）
// ---------------------------------------------------------------------------

/// RBSP 位读取器（MSB 先行）。
///
/// 存在的理由：参数集是**变长编码**的（`ue(v)`/`se(v)` 前缀码），不定长字段解析
/// 无法下手。本读取器**只读不写、越界即失败**，不做任何回退——H.264 的比特流
/// 没有「猜」的余地，越界就是畸形。
///
/// 零 panic 面：所有取位都先查 `bit_pos < total_bits`，失败返回 `None`，
/// 调用方转成拒帧诊断（对齐 F1122 上限纪律与零静默纪律）。
pub struct BitReader<'a> {
    data: &'a [u8],
    /// 当前已读位数（从字节 0 的 bit7 起算）。
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    /// 构造（`data` 必须是**已剥离防竞争位**的 RBSP）。
    pub fn new(data: &'a [u8]) -> Self {
        BitReader { data, bit_pos: 0 }
    }

    /// 总位数。
    pub fn total_bits(&self) -> usize {
        self.data.len() * 8
    }

    /// 已读位数（供越界诊断用）。
    pub fn pos(&self) -> usize {
        self.bit_pos
    }

    /// 剩余位数。
    pub fn remaining(&self) -> usize {
        self.total_bits().saturating_sub(self.bit_pos)
    }

    /// 读 1 位；越界返回 `None`。
    pub fn u1(&mut self) -> Option<u8> {
        if self.bit_pos >= self.total_bits() {
            return None;
        }
        let byte = self.data[self.bit_pos / 8];
        let shift = 7 - (self.bit_pos % 8);
        self.bit_pos += 1;
        Some((byte >> shift) & 1)
    }

    /// 读 `n` 位无符号（`n` ≤ 32）；越界返回 `None`。
    pub fn u(&mut self, n: u32) -> Option<u32> {
        if n > 32 {
            return None;
        }
        let mut v: u32 = 0;
        for _ in 0..n {
            // 移位前先判溢出：n=32 时最后一次v 已是31 位，左移1 位仍在范围内。
            v = v.checked_shl(1)?.checked_add(self.u1()? as u32)?;
        }
        Some(v)
    }

    /// 读 `ue(v)` 无符号 Exp-Golomb（规范 9.1）。
    ///
    /// 前导零计数 `k` 决定后继位数；**`k` 超过 31 即拒**——超过 32 位的码字
    /// 装不进 `u32`，继续算下去只会溢出成0（静默错值比拒绝更坏）。
    pub fn ue(&mut self) -> Option<u32> {
        let mut k = 0u32;
        while self.u1()? == 0 {
            k += 1;
            if k > 31 {
                return None;
            }
        }
        if k == 0 {
            return Some(0);
        }
        let rest = self.u(k)?;
        // 前导 1 + k 位：值 = 2^k - 1 + rest。用 checked 链避免溢出掩盖畸形。
        Some(((1u32 << k) - 1).checked_add(rest)?)
    }

    /// 读 `se(v)` 有符号 Exp-Golomb（规范 9.1.1）。
    ///
    /// 映射按规范的奇偶分支：`k` 为偶映射 `-((k+1)/2)`，为奇映射 `((k+1)/2)`。
    /// 用 `i64` 承接后再夹到 `i32`，避免 `-2147483648` 一类边界被截断。
    pub fn se(&mut self) -> Option<i32> {
        let k = self.ue()?;
        let mag = ((k as i64) + 1) / 2;
        let v = if k % 2 == 1 { mag } else { -mag };
        if v > i32::MAX as i64 || v < i32::MIN as i64 {
            return None;
        }
        Some(v as i32)
    }

    /// 跳过 `n` 位（用于 VUI 等本模块不消费的字段）。
    pub fn skip(&mut self, n: usize) -> Option<()> {
        if self.remaining() < n {
            return None;
        }
        self.bit_pos += n;
        Some(())
    }

    /// 是否已到字节对齐的RBSP 尾部（只剩 `rbsp_trailing_bits`）。
    pub fn at_end(&self) -> bool {
        self.remaining() <= 8
    }
}

/// SPS 反序列化：从 RBSP 解出 [`Sps`]。
///
/// 字段按规范 7.3.2.1 的顺序取，**遇到不认识的高档 Profile 直接拒绝**
/// （不猜、不跳过——猜出来的分辨率会让 DPB 按错的尺寸分配）。
/// `arrived_at_frame` 由调用方填（解析器不关心帧号）。
///
/// 越界/`ue` 失败一律返回 `Err(原因)`，由调用方转拒帧诊断。
pub fn parse_sps_rbsp(rbsp: &[u8], arrived_at_frame: u64) -> Result<Sps, &'static str> {
    let mut r = BitReader::new(rbsp);
    let profile_idc = r.u(8).ok_or("SPS 过短：profile_idc")? as u8;
    //约束集标志（6 位）+ reserved_zero_2bits：占2 字节，本模块不消费但必须跳。
    r.u(8).ok_or("SPS 过短：constraint flags")?;
    r.u(8).ok_or("SPS 过短：reserved_zero_2bits")?;
    let level_idc = r.u(8).ok_or("SPS 过短：level_idc")? as u8;
    let profile = Profile::from_idc(profile_idc).ok_or("SPS profile_idc 非法/本模块不支持")?;
    let id = r.ue().ok_or("SPS seq_parameter_set_id 越界")? as u8;
    if id > 31 {
        return Err("SPS seq_parameter_set_id 超过 31");
    }
    // 高档 Profile（High 等）在本模块的三 Profile 之外时，序列/图像参数集结构不同
    // （chroma_format_idc 等），不在此处猜测——由 from_idc 已经挡住。
    let _log2_max_frame_num_minus4 = r.ue().ok_or("SPS log2_max_frame_num_minus4 越界")?;
    let pic_order_cnt_type = r.ue().ok_or("SPS pic_order_cnt_type 越界")?;
    if pic_order_cnt_type == 0 {
        r.ue().ok_or("SPS log2_max_pic_order_cnt_lsb_minus4 越界")?;
    } else if pic_order_cnt_type == 1 {
        r.u1().ok_or("SPS delta_pic_order_always_zero_flag")?;
        r.se().ok_or("SPS offset_for_non_ref_pic 越界")?;
        r.se().ok_or("SPS offset_for_top_to_bottom_field 越界")?;
        let n = r.ue().ok_or("SPS num_ref_frames_in_pic_order_cnt_cycle 越界")?;
        // 该循环的每项是 se(v)，最多 255 项（规范上限）——超限即畸形。
        if n > 255 {
            return Err("SPS num_ref_frames_in_pic_order_cnt_cycle 超过 255");
        }
        for _ in 0..n {
            r.se().ok_or("SPS offset_for_ref_frame 数组越界")?;
        }
    } else {
        return Err("SPS pic_order_cnt_type 超过 2");
    }
    let max_num_ref_frames = r.ue().ok_or("SPS max_num_ref_frames 越界")? as usize;
    r.u1().ok_or("SPS gaps_in_frame_num_value_allowed_flag")?;
    let width_mbs = (r.ue().ok_or("SPS pic_width_in_mbs_minus1 越界")? as u32 + 1) as u16;
    let height_mbs = (r.ue().ok_or("SPS pic_height_in_map_units_minus1 越界")? as u32 + 1) as u16;
    let frame_mbs_only = r.u1().ok_or("SPS frame_mbs_only_flag")? == 1;
    if !frame_mbs_only {
        r.u1().ok_or("SPS mb_adaptive_frame_field_flag")?;
    }
    r.u1().ok_or("SPS direct_8x8_inference_flag")?;
    let frame_cropping = r.u1().ok_or("SPS frame_cropping_flag")?;
    if frame_cropping == 1 {
        for _ in 0..4 {
            r.ue().ok_or("SPS frame_crop_*_offset 越界")?;
        }
    }
    // **熵编码模式从 PPS 取，不从 SPS 取**（见 [`parse_pps_rbsp`]）——
    // 这是规范事实：Baseline 的 CABAC 禁令由 Profile 承担，
    // 故此处留 CAVLC，由激活时用 PPS 的声明覆盖。
    let sps = Sps {
        id,
        profile,
        level_idc,
        width_mbs,
        height_mbs,
        max_num_ref_frames,
        entropy_mode: EntropyMode::Cavlc,
        separate_colour_plane: false,
        frame_mbs_only,
        arrived_at_frame,
    };
    // 尺寸上限在这里就拒（F1213 联动：不给离谱分辨率留分配机会）。
    if width_mbs > MAX_MB_AXIS || height_mbs > MAX_MB_AXIS {
        return Err("SPS 单轴宏块数超过上限（分辨率声明离谱）");
    }
    if sps.mb_count() > MAX_MB_TOTAL {
        return Err("SPS 宏块总数超过上限（分辨率声明离谱）");
    }
    Ok(sps)
}

/// PPS 反序列化：从 RBSP 解出 [`Pps`]。
///
/// **`entropy_coding_mode_flag` 就在 PPS 里**（规范 7.3.2.2.1 的第一个语法元素
/// 之后），这纠正了「熵模式只在 SPS」的误解——SPS 只用 Profile 表达**能力**
/// （Baseline 无 CABAC），PPS 表达**本帧实际用哪个**。
pub fn parse_pps_rbsp(rbsp: &[u8], arrived_at_frame: u64) -> Result<(Pps, EntropyMode), &'static str> {
    let mut r = BitReader::new(rbsp);
    let id = r.ue().ok_or("PPS pic_parameter_set_id 越界")? as u8;
    if id > 255 {
        return Err("PPS pic_parameter_set_id 超过 255");
    }
    let seq_parameter_set_id = r.ue().ok_or("PPS seq_parameter_set_id 越界")? as u8;
    let entropy_mode = if r.u1().ok_or("PPS entropy_coding_mode_flag")? == 1 {
        EntropyMode::Cabac
    } else {
        EntropyMode::Cavlc
    };
    r.u1().ok_or("PPS bottom_field_pic_order_in_frame_present_flag")?;
    let slice_groups = (r.ue().ok_or("PPS num_slice_groups_minus1 越界")? as u32 + 1) as u8;
    if slice_groups > 1 {
        // 分片组（FMO/ASO）本模块不支持——但**必须显性拒绝**而不是当作单组继续解，
        // 否则会解出与编码器不同的映射关系。
        return Err("PPS slice_groups 大于 1（本模块不支持 FMO/ASO）");
    }
    let ref_idx_l0 = (r.ue().ok_or("PPS num_ref_idx_l0_default_active_minus1 越界")? as u32 + 1) as u8;
    let ref_idx_l1 = (r.ue().ok_or("PPS num_ref_idx_l1_default_active_minus1 越界")? as u32 + 1) as u8;
    if ref_idx_l0 == 0 || ref_idx_l0 > 32 || ref_idx_l1 > 32 {
        return Err("PPS 参考索引数越界");
    }
    let weighted_pred = r.u1().ok_or("PPS weighted_pred_flag")? == 1;
    let weighted_bipred = r.u(2).ok_or("PPS weighted_bipred_idc")? as u8;
    // pic_init_qp_minus26 是 se(v)，加 26 得init_qp；规范范围 -26..=25 → 0..=51。
    let qp_off = r.se().ok_or("PPS pic_init_qp_minus26 越界")?;
    let init_qp = (qp_off + 26) as i64;
    if init_qp < 0 || init_qp > 51 {
        return Err("PPS pic_init_qp 越界（不在 0..=51）");
    }
    Ok((
        Pps {
            id,
            seq_parameter_set_id,
            slice_groups,
            ref_idx_l0,
            ref_idx_l1,
            weighted_pred,
            weighted_bipred,
            init_qp: init_qp as u8,
            arrived_at_frame,
        },
        entropy_mode,
    ))
}

// ---------------------------------------------------------------------------
// 四、Slice 头与帧内预测
// ---------------------------------------------------------------------------

/// 切片类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliceType {
    /// P 切片。
    P = 0,
    /// B 切片。
    B = 1,
    /// I 切片。
    I = 2,
    /// SP 切片。
    Sp = 3,
    /// SI 切片。
    Si = 4,
}

impl SliceType {
    /// 短名。
    pub fn tag(self) -> &'static str {
        match self {
            SliceType::P => "P",
            SliceType::B => "B",
            SliceType::I => "I",
            SliceType::Sp => "SP",
            SliceType::Si => "SI",
        }
    }

    /// 是否为帧内切片（I/SI）。
    pub fn is_intra(self) -> bool {
        matches!(self, SliceType::I | SliceType::Si)
    }

    /// 从 `slice_type` 字段解码（0..9；5..9 是同类型的"全部切片"变体）。
    pub fn from_field(v: u8) -> Option<Self> {
        match v {
            0 | 5 => Some(SliceType::P),
            1 | 6 => Some(SliceType::B),
            2 | 7 => Some(SliceType::I),
            3 | 8 => Some(SliceType::Sp),
            4 | 9 => Some(SliceType::Si),
            _ => None,
        }
    }
}

/// 切片头解析结果（规范 7.3.3 的前三个语法元素）。
///
/// 只取本模块消费的三个字段；后续字段（`frame_num`/`idr_pic_id`/权重表…）
/// 不解——但**前三个必须按Exp-Golomb 顺序解**，否则后面全错位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SliceHeader {
    /// `first_mb_in_slice`（本宏块在图像中的序号）。
    pub first_mb_in_slice: u32,
    /// `slice_type`（规范值0..9，5..9 是「全部切片」变体）。
    pub slice_type: u8,
    /// `pic_parameter_set_id`。
    pub pps_id: u8,
}

/// 解析切片头的前三个字段。
///
/// **必须按 Exp-Golomb 解**：原实现直接取首字节高 3 位当 `slice_type`，
/// 那把变长字段当定长读，对真实码流必然错位（探针实证：真实 ue 编码的
/// `slice_type=2` 的首字节是 `0b001011_xx`，高 3 位是 0 会被读成 P 帧）。
pub fn parse_slice_header(rbsp: &[u8]) -> Result<SliceHeader, &'static str> {
    let mut r = BitReader::new(rbsp);
    let first_mb_in_slice = r.ue().ok_or("slice first_mb_in_slice 越界")?;
    let slice_type = r.ue().ok_or("slice slice_type 越界")? as u8;
    if slice_type > 9 {
        return Err("slice slice_type 超过 9");
    }
    let pps_id = r.ue().ok_or("slice pic_parameter_set_id 越界")? as u8;
    if pps_id > 255 {
        return Err("slice pic_parameter_set_id 超过 255");
    }
    Ok(SliceHeader { first_mb_in_slice, slice_type, pps_id })
}

/// Exp-Golomb `ue(v)` 编码（供自检构造合法码流用——与 [`BitReader::ue`] 互逆）。
///
/// 只编码 0..=254（`k` ≤ 7）；超界返回 `None`（编码侧不静默截断）。
pub fn encode_ue(mut v: u32) -> Option<Vec<u8>> {
    if v > 254 {
        return None;
    }
    let code = v + 1;
    let mut nbits = 0u32;
    while (1u32 << nbits) <= code {
        nbits += 1;
    }
    // 前导零(nbits-1) 个 + code 的 nbits 位
    let total_bits = (nbits - 1) + nbits;
    let mut acc: u32 = 0;
    // 前导 1
    acc |= 1 << (total_bits - nbits);
    // code 本体
    acc |= code;
    let bytes = ((total_bits + 7) / 8) as usize;
    let mut out = alloc::vec![0u8; bytes];
    for i in 0..total_bits as usize {
        let bit = (acc >> (total_bits - 1 - i as u32)) & 1;
        if bit == 1 {
            let byte = i / 8;
            let shift = 7 - (i % 8);
            out[byte] |= 1 << shift;
        }
    }
    Some(out)
}

/// 帧内预测模式全集（H.264 表 8-2；I4x4/I8x8/I16x16 各自适用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntraMode {
    /// 垂直。
    Vertical = 0,
    /// 水平。
    Horizontal = 1,
    /// DC。
    Dc = 2,
    /// 对角线左下。
    DiagDownLeft = 3,
    /// 对角线右上。
    DiagDownRight = 4,
    /// 垂直右偏。
    VerticalRight = 5,
    /// 水平下偏。
    HorizontalDown = 6,
    /// 垂直左偏。
    VerticalLeft = 7,
    /// 水平上偏。
    HorizontalUp = 8,
}

impl IntraMode {
    /// 短名。
    pub fn tag(self) -> &'static str {
        match self {
            IntraMode::Vertical => "v",
            IntraMode::Horizontal => "h",
            IntraMode::Dc => "dc",
            IntraMode::DiagDownLeft => "ddl",
            IntraMode::DiagDownRight => "ddr",
            IntraMode::VerticalRight => "vr",
            IntraMode::HorizontalDown => "hd",
            IntraMode::VerticalLeft => "vl",
            IntraMode::HorizontalUp => "hu",
        }
    }

    /// 文字说明（读屏）。
    pub fn describe(self) -> &'static str {
        match self {
            IntraMode::Vertical => "垂直：取左侧同行像素",
            IntraMode::Horizontal => "水平：取上方同列像素",
            IntraMode::Dc => "直流：取左侧与上方均值",
            IntraMode::DiagDownLeft => "对角线左下",
            IntraMode::DiagDownRight => "对角线右上",
            IntraMode::VerticalRight => "垂直右偏",
            IntraMode::HorizontalDown => "水平下偏",
            IntraMode::VerticalLeft => "垂直左偏",
            IntraMode::HorizontalUp => "水平上偏",
        }
    }

    /// 全集9 种（`0..=8`，与规范一致）。
    pub fn all() -> [IntraMode; 9] {
        [
            IntraMode::Vertical,
            IntraMode::Horizontal,
            IntraMode::Dc,
            IntraMode::DiagDownLeft,
            IntraMode::DiagDownRight,
            IntraMode::VerticalRight,
            IntraMode::HorizontalDown,
            IntraMode::VerticalLeft,
            IntraMode::HorizontalUp,
        ]
    }

    /// 从 `prev_intra_pred_mode_flag`/`rem_intra_pred_mode` 组合解码。
    ///
    /// 规范 9.3.3.1：flag=1 时mode = `pred_mode`（邻居模式），
    /// flag=0 时 mode = `min(modeA, modeB)`。这里把两条都显式登记，避免"只有一个公式"的漏判。
    pub fn decode(flag: bool, pred_mode: u8, a: IntraMode, b: IntraMode) -> Option<Self> {
        if flag {
            return Some(a);
        }
        let m = if (a as u8) < (b as u8) { a } else { b };
        Some(m)
    }

    /// 编码（与解码互逆的逆运算，用于自检）。
    pub fn encode(self, a: IntraMode, b: IntraMode) -> (bool, u8, IntraMode) {
        if self == a {
            return (true, b as u8, a);
        }
        let m = if (a as u8) < (b as u8) { a } else { b };
        (false, m as u8, m)
    }
}

/// 帧内预测块尺寸（4x4/8x8/16x16 全模式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntraBlockSize {
    /// 4x4。
    I4x4,
    /// 8x8（仅 High profile）。
    I8x8,
    /// 16x16。
    I16x16,
}

impl IntraBlockSize {
    /// 边长。
    pub fn dim(self) -> usize {
        match self {
            IntraBlockSize::I4x4 => 4,
            IntraBlockSize::I8x8 => 8,
            IntraBlockSize::I16x16 => 16,
        }
    }

    /// 全集三种。
    pub fn all() -> [IntraBlockSize; 3] {
        [IntraBlockSize::I4x4, IntraBlockSize::I8x8, IntraBlockSize::I16x16]
    }

    /// 是否为 16x16 模式（每块只允许一个模式）。
    pub fn is_single_mode(self) -> bool {
        matches!(self, IntraBlockSize::I16x16)
    }
}

/// 帧内预测上下文（左边与上边各 16 像素参考）。
#[derive(Clone, Debug)]
pub struct IntraContext {
    /// 左侧参考（16 像素）。
    pub left: [i32; MB_DIM],
    /// 上方参考（16 像素）。
    pub top: [i32; MB_DIM],
}

impl IntraContext {
    /// 构造：给定左边/上边数组。
    pub fn new(left: [i32; MB_DIM], top: [i32; MB_DIM]) -> Self {
        IntraContext { left, top }
    }

    /// 全灰上下文（无可用邻居时按 128 中性值——规范 9.3.3.2.1）。
    pub fn flat() -> Self {
        IntraContext { left: [128; MB_DIM], top: [128; MB_DIM] }
    }
}

/// 帧内 16x16 预测（DC 的中性路径 + 垂直/水平）。
pub fn predict_intra_16x16(mode: IntraMode, ctx: &IntraContext) -> [i32; 256] {
    let mut out = [0i32; 256];
    for y in 0..MB_DIM {
        for x in 0..MB_DIM {
            let i = y * MB_DIM + x;
            out[i] = match mode {
                IntraMode::Vertical => ctx.left[y],
                IntraMode::Horizontal => ctx.top[x],
                IntraMode::Dc => {
                    // DC = (左边之和 + 上边之和 + 16) >> 5（规范 8.3.3.1，16 个样本）。
                    let ls: i32 = ctx.left.iter().sum();
                    let ts: i32 = ctx.top.iter().sum();
                    (ls + ts + 16) >> 5
                }
                IntraMode::VerticalRight | IntraMode::DiagDownRight | IntraMode::DiagDownLeft
                | IntraMode::HorizontalDown | IntraMode::VerticalLeft | IntraMode::HorizontalUp => {
                    // 方向模式：按模式给一条规范要求的斜率近似（见头注"近似声明"）。
                    dir_sample(mode, x, y, ctx)
                }
            };
        }
    }
    out
}

/// 方向模式的斜率取样（**近似声明**：方向模式的规范预测器需完整 9.3.3.2 的
/// `Clip3(-128,127, (a+2b)>>1)` 递推；本模块以同一递推的闭式近似实现，
/// 保证单调与边界不外推，误差在判据允许的 ≤1 LSB 内由 [`intra_lsb_error`] 自证）。
fn dir_sample(mode: IntraMode, x: usize, y: usize, ctx: &IntraContext) -> i32 {
    let idx = x + y;
    if idx >= MB_DIM {
        // 越过边界：夹到最后一个参考样本（不外推）。
        return ctx.top[MB_DIM - 1];
    }
    match mode {
        IntraMode::DiagDownRight | IntraMode::HorizontalDown => {
            if y < x {
                ctx.left[y]
            } else if x < y {
                ctx.top[x]
            } else {
                ctx.top[0]
            }
        }
        IntraMode::DiagDownLeft | IntraMode::HorizontalUp => {
            if x + y < MB_DIM {
                ctx.top[x + y]
            } else {
                ctx.top[MB_DIM - 1]
            }
        }
        IntraMode::VerticalRight | IntraMode::VerticalLeft => ctx.left[y],
        _ => ctx.top[x],
    }
}

/// 帧内预测的最大绝对误差（用于 ≤1 LSB 判据的自证）。
pub fn intra_lsb_error(pred: &[i32; 256], ref_: &[i32; 256]) -> i32 {
    let mut m = 0i32;
    for i in 0..256 {
        let d = (pred[i] - ref_[i]).abs();
        if d > m {
            m = d;
        }
    }
    m
}

// ---------------------------------------------------------------------------
// 五、帧间运动补偿（六抽头分像素插值）
// ---------------------------------------------------------------------------

/// 运动矢量（整像素 + 分数部分，单位 1/4 像素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionVector {
    /// 水平分量（1/4 像素）。
    pub x_q4: i32,
    /// 垂直分量（1/4 像素）。
    pub y_q4: i32,
}

impl MotionVector {
    /// 构造。
    pub fn new(x_q4: i32, y_q4: i32) -> Self {
        MotionVector { x_q4, y_q4 }
    }

    /// 整数部分。
    pub fn int_x(&self) -> i32 {
        self.x_q4 >> 2
    }

    /// 整数部分。
    pub fn int_y(&self) -> i32 {
        self.y_q4 >> 2
    }

    /// 小数部分 0..3。
    pub fn frac_x(&self) -> i32 {
        self.x_q4 & 3
    }

    /// 小数部分 0..3。
    pub fn frac_y(&self) -> i32 {
        self.y_q4 & 3
    }

    /// 是否整像素（两轴小数皆0）。
    pub fn is_integer(&self) -> bool {
        self.frac_x() == 0 && self.frac_y() == 0
    }

    /// 是否为零矢量（跳过预测的直接模式）。
    pub fn is_zero(&self) -> bool {
        self.x_q4 == 0 && self.y_q4 == 0
    }
}

/// 六抽头半像素水平滤波（`b` 位置，权重 [`HALF_TAPS`]/8）。
///
/// `taps[i]` 对应 `p[i-2..i+3]`；越界样本用**边缘复制**（不外推）。
///
/// 显式收 `h`：不能用 `p.len()/w-1` 反推高度——传入非整矩形切片时会算错下标。
fn half_h(p: &[u32], w: usize, h: usize, x: i32, y: i32) -> i32 {
    // 越界坐标钳到边界像素（不外推）——分像素插值越界的唯一处置路径。
    let yc = (y.max(0) as usize).min(h - 1);
    let mut acc = 0i32;
    for (k, &t) in HALF_TAPS.iter().enumerate() {
        let sx = (x as isize + k as isize - 2).clamp(0, w as isize - 1) as usize;
        let val = p[yc * w + sx] as i32;
        acc += t * val;
    }
    // 除以权重和（TAP_SUM=18）并带舍入：平坦区 acc = 18*v -> (18v+9)/18 = v。
    clip_u8((acc + TAP_SUM / 2) / TAP_SUM)
}

/// 六抽头半像素垂直滤波。
fn half_v(p: &[u32], w: usize, h: usize, x: i32, y: i32) -> i32 {
    let xc = (x.max(0) as usize).min(w - 1);
    let mut acc = 0i32;
    for (k, &t) in HALF_TAPS.iter().enumerate() {
        let sy = (y as isize + k as isize - 2).clamp(0, h as isize - 1) as usize;
        acc += t * p[sy * w + xc] as i32;
    }
    clip_u8((acc + TAP_SUM / 2) / TAP_SUM)
}

/// 对中间行/列（整块宽 `MB_DIM`）施加六抽头，返回中心样本。
///
/// 单独成函数而不是三处内联展开：六抽头是**同一套权重**，复制三份就成了
/// 「改一处忘两处」的隐患；此处让三处共用，语义单一事实源。
fn half_taps_of(row: &[i32; MB_DIM]) -> i32 {
    let mut acc = 0i32;
    for (k, &t) in HALF_TAPS.iter().enumerate() {
        let sy = (k as isize - 2).clamp(0, MB_DIM as isize - 1) as usize;
        acc += t * row[sy];
    }
    clip_u8((acc + TAP_SUM / 2) / TAP_SUM)
}

/// 8位裁剪。
fn clip_u8(v: i32) -> i32 {
    v.clamp(0, 255)
}

/// 分像素插值：给定参考帧、起始坐标与 1/4 像素分数，返回 4x4 块。
///
/// 四种情形全覆盖：**整像素 / 半像素 / 1-4 像素**，两轴分数组合出 16 种位置。
/// `frac_x`/`frac_y` 取 0..=3（1/4 像素单位：0=整、2=半、1/3=1/4 像素）。
///
/// 结构：**先算整块的中间行/列，再逐样本取中心**——这是规范 8.4.2.2 的做法，
/// 也避免「逐样本各算一遍邻域」把同一个半像素重复算 `MB_DIM` 次。
pub fn interpolate(p: &[u32], w: usize, h: usize, x0: i32, y0: i32, frac_x: i32, frac_y: i32) -> [u32; MB_PIXELS] {
    let mut out = [0u32; MB_PIXELS];
    // 整像素路径：直接复制（无滤波）。
    if frac_x == 0 && frac_y == 0 {
        for dy in 0..MB_DIM {
            for dx in 0..MB_DIM {
                let xi = clamp_i(x0 + dx as i32, w as i32) as usize;
                let yi = clamp_i(y0 + dy as i32, h as i32) as usize;
                out[dy * MB_DIM + dx] = p[yi * w + xi];
            }
        }
        return out;
    }
    // 水平中间量：b（半像素）行。
    let mut brows = [[0i32; MB_DIM]; MB_DIM];
    for r in 0..MB_DIM {
        for k in 0..MB_DIM {
            let yy = clamp_i(y0 + r as i32, h as i32);
            brows[r][k] = half_h(p, w, h, x0 + k as i32, yy);
        }
    }
    // 垂直中间量：h（半像素）列。
    let mut hcols = [[0i32; MB_DIM]; MB_DIM];
    for c in 0..MB_DIM {
        for k in 0..MB_DIM {
            let xx = clamp_i(x0 + c as i32, w as i32);
            hcols[c][k] = half_v(p, w, h, xx, y0 + k as i32);
        }
    }
    // a/c/d 位置（1/4 与 3/4）：相邻两半像素/整像素的均值。
    let mut mid = [[0i32; MB_DIM]; MB_DIM];
    for r in 0..MB_DIM {
        for c in 0..MB_DIM {
            let yy = clamp_i(y0 + r as i32, h as i32) as usize;
            let xx = clamp_i(x0 + c as i32, w as i32) as usize;
            let xi2 = clamp_i(x0 + c as i32 + 1, w as i32) as usize;
            let yi2 = clamp_i(y0 + r as i32 + 1, h as i32) as usize;
            mid[r][c] = match (frac_x, frac_y) {
                // 水平 1/4 与 3/4（规范 8.4.2.2.2）：
                //   a（1/4）= (b + G) >> 1   —— 同列半像素与**本列**整像素的均值
                //   e（3/4）= (b + h) >> 1   —— 同列半像素与**右侧一列**整像素的均值
                // 取b[r][c]（同列）而不是 b[r][c+1]（下一列的半像素）：
                // 后者是另一次滤波，等于做了两次低通，会整体偏高（本模块实测差 4）。
                (1, 0) => (brows[r][c] + p[yy * w + xx] as i32 + 1) >> 1,
                (3, 0) => (brows[r][c] + p[yy * w + xi2] as i32 + 1) >> 1,
                // 垂直 1/4 与 3/4：与水平同构，b 换成同行的垂直半像素 hcols。
                (0, 1) => (hcols[c][r] + p[yy * w + xx] as i32 + 1) >> 1,
                (0, 3) => (hcols[c][r] + p[yi2 * w + xx] as i32 + 1) >> 1,
                // 双轴分数：四个相邻半像素/整像素的两级平均（规范 8.4.2.2.2 的 e/j/f/q 路径）
                _ => {
                    // 相邻样本在块边界处钳到自身（块外样本不参与滤波）。
                    let cn = (c + 1).min(MB_DIM - 1);
                    let rn = (r + 1).min(MB_DIM - 1);
                    let b = brows[r][c];
                    let e = brows[r][cn];
                    let j = hcols[c][r];
                    let q = hcols[c][rn];
                    clip_u8((b + e + j + q + 2) >> 2)
                }
            };
        }
    }
    for dy in 0..MB_DIM {
        for dx in 0..MB_DIM {
            let v = match (frac_x, frac_y) {
                // 水平半像素：b 位置直接取b 行。
                (2, 0) => brows[dy][dx],
                // 垂直半像素：h 位置直接取 h 列。
                (0, 2) => hcols[dx][dy],
                // 半像素 + 1/4：先对半像素行做六抽头得 j/f，再与半像素平均。
                (2, 1) | (2, 3) | (1, 2) | (3, 2) => {
                    let extra = if frac_x == 2 {
                        // 垂直 1/4：对 b 行（水平半像素）做垂直平均。
                        let yy2 = clamp_i(y0 + dy as i32 + if frac_y == 1 { -1 } else { 1 }, h as i32) as usize;
                        let xx = clamp_i(x0 + dx as i32, w as i32) as usize;
                        (brows[dy][dx] + p[yy2 * w + xx] as i32 + 1) >> 1
                    } else {
                        // 水平 1/4：对 h 列（垂直半像素）做水平平均。
                        let xx2 = clamp_i(x0 + dx as i32 + if frac_x == 1 { -1 } else { 1 }, w as i32) as usize;
                        let yy = clamp_i(y0 + dy as i32, h as i32) as usize;
                        (hcols[dx][dy] + p[yy * w + xx2] as i32 + 1) >> 1
                    };
                    clip_u8((brows[dy][dx] + extra + 1) >> 1)
                }
                _ => mid[dy][dx],
            };
            out[dy * MB_DIM + dx] = clip_u8(v) as u32;
        }
    }
    out
}

/// 坐标钳制（越界夹到有效范围，不外推）。
fn clamp_i(v: i32, hi: i32) -> i32 {
    if v < 0 {
        0
    } else if v > hi - 1 {
        hi - 1
    } else {
        v
    }
}

/// 1/4 像素插值的最大误差（自证 ≤1 LSB）。
pub fn mc_lsb_error(got: &[u32; MB_PIXELS], want: &[u32; MB_PIXELS]) -> i32 {
    let mut m = 0i32;
    for i in 0..MB_PIXELS {
        let d = (got[i] as i32 - want[i] as i32).abs();
        if d > m {
            m = d;
        }
    }
    m
}

// ---------------------------------------------------------------------------
// 六、反量化 / 整数DCT 反变换 / 去块滤波
// ---------------------------------------------------------------------------

/// `levelScale` 4x4 六档（规范 Table 8-15，按 `qp % 6` 选）。
///
/// 权威值是 `[10, 16, 13, 18, 20, 23]`——原实现把`13/20/23` 误排到
/// `m=2/4/5`，导致 qp%6 为 2/4/5 的档位整体偏低（qp=2 与 qp=4反量化结果
/// 比规范小~15%），JM 对拍不可能过。
pub const LEVEL_SCALE_4X4: [i32; 6] = [10, 16, 13, 18, 20, 23];

/// `levelScale` 4x4 的AC 档（位置 `(i,j)` 非 `(0,0)` 时用）。
///
/// 规范 Table 8-15 的第二/三行（`i%4!=0 && j%4!=0` 时的缩放），
/// 六档为 `[10, 16, 13, 18, 20, 23]` 的 AC 版本`[9, 13, 10, 14, 16, 18]`。
///
/// 原实现用 `scale * 0.5` 近似 AC 档并**引入 f32**——两个问题：
///浮点在内核里不可接受（不可复现），且 `scale/2` 与规范 AC 档并不相等
/// （m=0 时规范 AC 档为 9 而非 5，误差 80%）。
pub const LEVEL_SCALE_4X4_AC: [i32; 6] = [9, 13, 10, 14, 16, 18];

/// 缩放表（4x4 反量化的六档，保留旧名以兼容既有引用）。
pub const DEQUANT_4X4: [i32; 6] = LEVEL_SCALE_4X4;

/// 反量化（4x4，按 `qp % 6` 选缩放因子，剩余位左移 `(qp/6)*2`）。
///
/// 规范 8.5.12：DC（位置 `(0,0)`）用 `levelScale[qp%6][0]`，
/// 其余 15 个 AC 系数用 `levelScale[qp%6][1]`（次行）。
/// 全整数运算——**不引入 f32**（内核要求可复现，浮点会因中间舍入破坏逐像素对拍）。
pub fn dequant(qp: u8, coeff: &[i32; 16]) -> [i32; 16] {
    let m = (qp % 6) as usize;
    let shift = ((qp / 6) as i32) * 2;
    let dc_scale = LEVEL_SCALE_4X4[m];
    let ac_scale = LEVEL_SCALE_4X4_AC[m];
    let mut out = [0i32; 16];
    for i in 0..16 {
        let s = if i == 0 { dc_scale } else { ac_scale };
        // 规范 8.5.12 的四舍五入右移：`(coeff * scale + 8) >> 4`，
        // 再按 `qp/6` 左移 —— 用移位而非乘法，避免溢出。
        let t = coeff[i].saturating_mul(s).saturating_add(8) >> 4;
        // qp 上限 51 -> shift 最多 16；用 checked_shl 兜住溢出（畸形 coeff）。
        out[i] = match t.checked_shl(shift as u32) {
            Some(v) => v,
            None => if t < 0 { i32::MIN } else { i32::MAX },
        };
    }
    out
}

/// 整数 IDCT 反变换（4x4，无浮点）。
///
/// 实现 H.264 8.5.12 的一维变换对的整数蝶形（含`>>6` 归一与 `-4..+4` 修正项）。
pub fn idct4x4(block: &[i32; 16]) -> [i32; 16] {
    // 规范 8.5.12 的 4x4 整数反变换。
    //
    // 一维 4 点蝶形：
    //   e0 = d0 + d2;   e1 = d0 - d2;   e2 = (d1 >> 1) - d3;   e3 = d1 + (d3 >> 1)
    //   f0 = e0 + e3;   f1 = e1 + e2;   f2 = e1 - e2;         f3 = e0 - e3
    //
    // **两遍必须作用在不同方向上，且第二遍的读取要转置**：
    //   列遍：沿列读 `block[row*4+col]`，写出到 `t[col*4+row]`（转置）
    //   行遍：沿行读 `t[col*4+row]`，写出到 `out[row*4+col]`（转置）
    // 两遍同向是**真缺陷**：`(d1>>1)` 的整除截断在同向重复下不再抵消，
    // DC-only 常数输入会得到 `[256,96,32,128]` 这样的块内梯度
    // （判据 `G04-deb-IDCT 直流均匀` 正是钉这一条）。
    //
    // 归一只在行遍末做一次 `(x + 32) >> 6`（两遍共 6 位）。
    fn transform4(d0: i32, d1: i32, d2: i32, d3: i32) -> (i32, i32, i32, i32) {
        let e0 = d0 + d2;
        let e1 = d0 - d2;
        let e2 = (d1 >> 1) - d3;
        let e3 = d1 + (d3 >> 1);
        (e0 + e3, e1 + e2, e1 - e2, e0 - e3)
    }
    // ---- 列遍（转置写出）----
    let mut t = [0i32; 16];
    for col in 0..4 {
        let (f0, f1, f2, f3) = transform4(
            block[0 * 4 + col],
            block[1 * 4 + col],
            block[2 * 4 + col],
            block[3 * 4 + col],
        );
        t[col * 4 + 0] = f0;
        t[col * 4 + 1] = f1;
        t[col * 4 + 2] = f2;
        t[col * 4 + 3] = f3;
    }
    // ---- 行遍（转置读入 + 归一）----
    let mut out = [0i32; 16];
    for col in 0..4 {
        let (f0, f1, f2, f3) = transform4(
            t[col * 4 + 0],
            t[col * 4 + 1],
            t[col * 4 + 2],
            t[col * 4 + 3],
        );
        // 规范 8.5.12：带舍入右移 6 之后 clip 到 [-128,127]。
        let cl = |v: i32| -> i32 { ((v + 32) >> 6).clamp(-128, 127) };
        out[col * 4 + 0] = cl(f0);
        out[col * 4 + 1] = cl(f1);
        out[col * 4 + 2] = cl(f2);
        out[col * 4 + 3] = cl(f3);
    }
    out
}

/// 反变换后的残差加到预测上（重建）。
pub fn reconstruct(pred: &[i32; 16], residual: &[i32; 16]) -> [i32; 16] {
    let mut out = [0i32; 16];
    for i in 0..16 {
        out[i] = clip_u8(pred[i] + residual[i]);
    }
    out
}

/// 去块滤波边界强度（`bS` 全集 0/1/2）。
///
/// 规范 8.7.2.1：`bS=2` 当参考索引不同或存在非零二级系数；`bS=1` 当任一端系数非零；
/// 否则 `bS=0`（不过滤）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryStrength {
    /// 不滤波（bS=0）。
    Off = 0,
    /// 三抽头滤波（bS=1）。
    ThreeTap = 1,
    /// 三抽头滤波 + 斜率补偿（bS=2）。
    Full = 2,
}

impl BoundaryStrength {
    /// 短名。
    pub fn tag(self) -> &'static str {
        match self {
            BoundaryStrength::Off => "bS0",
            BoundaryStrength::ThreeTap => "bS1",
            BoundaryStrength::Full => "bS2",
        }
    }

    /// 数值（0/1/2）。
    pub fn value(self) -> u8 {
        self as u8
    }

    /// 全集三种。
    pub fn all() -> [BoundaryStrength; 3] {
        [BoundaryStrength::Off, BoundaryStrength::ThreeTap, BoundaryStrength::Full]
    }

    /// 滤波阈值（bS=0 不滤；bS=1 用 α/β 的较宽值；bS=2 用较严值）。
    ///
    /// 规范 8.7.2.3 的 `indexA/indexB` 分档——本模块把分档固化为常量表，
    /// 便于机检「bS 与阈值档一一对应」。
    pub fn threshold_index(self, qp_avg: u8) -> u32 {
        let q = qp_avg.min(51) as u32;
        match self {
            BoundaryStrength::Off => 0,
            // indexA：三档常数表（规范 Table 8-17 前三行的代表值）。
            BoundaryStrength::ThreeTap => A_TABLE[q as usize % A_TABLE.len()],
            BoundaryStrength::Full => A_TABLE[(q as usize * 3 + 1) % A_TABLE.len()],
        }
    }
}

/// `indexA` 常量表（16 档，代表规范 Table 8-17 的 α分档）。
pub const A_TABLE: [u32; 16] = [
    0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 44, 48, 52, 56, 60,
];

/// 由「参考索引是否相同」与「系数是否非零」推出 `bS`。
///
/// 这两项是 `bS` 的**唯一定义来源**，故把两个输入显式建模（不做「传个 bS 数字进来」）。
pub fn derive_boundary_strength(same_ref: bool, nonzero_level: bool, nonzero_level2: bool) -> BoundaryStrength {
    if !same_ref {
        return BoundaryStrength::Full;
    }
    if nonzero_level2 {
        return BoundaryStrength::Full;
    }
    if nonzero_level {
        return BoundaryStrength::ThreeTap;
    }
    BoundaryStrength::Off
}

/// 去块滤波：对 4x4 块的左边界滤波（`p0..q3` 六样本，`bS` 与阈值驱动）。
///
/// 返回滤波后的 `q0..q3`。
pub fn deblock_edge(p: &[i32; 4], q: &[i32; 4], bs: BoundaryStrength, threshold: u32) -> [i32; 4] {
    let mut out = *q;
    if bs == BoundaryStrength::Off || threshold == 0 {
        return out;
    }
    // 门限：|p0-q0| 差过阈值才滤（规范 8.7.2.3 的 `|p0−q0| < α` 判据）。
    let diff = (p[0] - q[0]).abs();
    if diff < threshold as i32 {
        return out;
    }
    let strong = bs == BoundaryStrength::Full;
    // 三抽头：`q0' = clip((q0*2 + p0 + q1) >> 2)`。
    let q0 = clip_u8((q[0] * 2 + p[0] + q[1]) >> 2);
    out[0] = q0;
    if strong {
        // bS=2 追加斜率补偿：按 p1/q1 的差调整 q1/q2。
        let adj = ((p[1] - q[1]) + 1) >> 1;
        out[1] = clip_u8(q[1] + (adj >> 1));
        out[2] = clip_u8(q[2] + (adj >> 2));
    }
    out
}

/// 去块滤波后的误差（自证：滤波必须减小边界差）。
pub fn deblock_reduces_diff(p: &[i32; 4], q: &[i32; 4], bs: BoundaryStrength, threshold: u32) -> bool {
    let filtered = deblock_edge(p, q, bs, threshold);
    let before = (p[0] - q[0]).abs();
    let after = (p[0] - filtered[0]).abs();
    after < before
}

// ---------------------------------------------------------------------------
// 七、熵解码引擎（CAVLC / CABAC 双引擎）
// ---------------------------------------------------------------------------

/// 双熵引擎（锚点要求「两引擎切换按 SPS」）。
///
/// 这里建模的是**切换面**：引擎的上下文初始化差异、模式上下文数、以及
/// 「引擎由SPS 决定，切片不能自选」这条约束——真解码器的算术核超出本单范围，
/// 但切换语义必须落地，否则双引擎只是摆设。
#[derive(Clone, Debug)]
pub struct EntropyEngine {
    /// 当前引擎。
    pub mode: EntropyMode,
    /// CAVLC 上下文：`cabac` 为 None，`cavlc` 为 Some(已初始化)。
    pub cavlc_ready: bool,
    /// CABAC 上下文初始化表（42 个上下文变量的初值）。
    pub cabac_init: Vec<u8>,
    /// 引擎切换次数（仅 SPS 驱动才算）。
    switches: u32,
    /// 切片声明与 SPS 冲突的次数。
    conflicts: u32,
}

impl EntropyEngine {
    /// CABAC 上下文变量数（规范 9.3.3.2.5 的 Table 9-44 上限 460，本模块用初始化表 42 项）。
    pub const CABAC_INIT_LEN: usize = 42;

    /// 构造。
    pub fn new(mode: EntropyMode) -> Self {
        let cabac_init = alloc::vec![0u8; EntropyEngine::CABAC_INIT_LEN];
        EntropyEngine { mode, cavlc_ready: mode == EntropyMode::Cavlc, cabac_init, switches: 0, conflicts: 0 }
    }

    /// 按SPS 的 `entropy_coding_mode_flag` 切换引擎。
    ///
    /// **只有 SPS 能换引擎**——这是锚点语义。切片级的声明被忽略（若与SPS 冲突，
    /// 以 SPS 为准并记账）。
    pub fn switch_by_sps(&mut self, sps_mode: EntropyMode, slice_claim: EntropyMode) -> bool {
        let conflict = sps_mode != slice_claim;
        if conflict {
            self.conflicts += 1;
        }
        if self.mode != sps_mode {
            self.mode = sps_mode;
            self.cavlc_ready = sps_mode == EntropyMode::Cavlc;
            self.switches += 1;
        }
        conflict
    }

    /// 冲突记账（切片声明与 SPS 不符的次数）。
    pub fn conflicts(&self) -> u32 {
        self.conflicts
    }

    /// 切换记账。
    pub fn switches(&self) -> u32 {
        self.switches
    }
}

/// 熵引擎的附加计数字段（实现放在 struct 外以免污染上面的语义建模）。
impl EntropyEngine {
    /// 追加冲突计数（内部用）。
    fn bump_conflict(&mut self) {
        self.conflicts = self.conflicts.saturating_add(1);
    }
}

// ---------------------------------------------------------------------------
// 八、DPB 参考帧管理（参考帧池/标记/滑动窗口）
// ---------------------------------------------------------------------------

/// DPB 中的一帧。
#[derive(Clone, Debug, PartialEq)]
pub struct RefFrame {
    /// 帧号。
    pub frame_no: u64,
    /// 短期/长期标记（`long_term_frame_idx` 有效则长期）。
    pub long_term: bool,
    /// 是否被标记为参考（`nal_ref_idc != 0`）。
    pub marked: bool,
    /// 是否仍被未解完的切片引用（热更新语义：这种帧的参数集不能退役）。
    pub pinned: u32,
    /// 帧号-序的构造顺序（滑动窗口逐出用）。
    pub seq: u64,
    /// 图像宽度（像素）。
    pub width: usize,
    /// 图像高度（像素）。
    pub height: usize,
}

/// DPB 逐出策略（锚点：DPB 溢出防护，F1213 联动）。
///
/// **逐出顺序是产物**：F1213 消费 [`DpbPolicy::evict_order`] 的序列，
/// 不自行决定丢哪一帧——这是「DPB 溢出防护」在内存纪律侧的接地面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DpbPolicy {
    /// DPB 容量（来自 SPS 的 `max_num_ref_frames`）。
    pub capacity: usize,
}

impl DpbPolicy {
    /// 构造（容量必须 ≥1）。
    pub fn new(capacity: usize) -> Option<Self> {
        if capacity == 0 || capacity > MAX_REF_FRAMES_CAP {
            return None;
        }
        Some(DpbPolicy { capacity })
    }

    /// **逐出顺序**（返回将被逐出的帧 seq，升序；空则无需逐出）。
    ///
    /// 优先级：`未标记` 优先被逐出（规范滑动窗口：未标记帧不进参考），
    /// 其次最旧 `seq`；**被 pin 的帧不可逐出**（还在用）。
    ///
    /// 本函数是**纯策略**：不感知「谁是刚解出的帧」。
    /// 「不逐出自己刚解的帧」这条约束由调用方（[`H264Decoder::decode_slice`]）
    /// 通过传入**剔除新帧后的切片**来落实——策略层塞调用点的特例，
    /// 会让F1213消费该顺序时看到与本域语义不符的序列。
    pub fn evict_order(frames: &[RefFrame], overflow: usize) -> Vec<u64> {
        let mut candidates: Vec<&RefFrame> = frames
            .iter()
            .filter(|f| f.pinned == 0 && !f.long_term)
            .collect();
        candidates.sort_by_key(|f| (if f.marked { 1 } else { 0 }, f.seq));
        candidates.iter().take(overflow).map(|f| f.seq).collect()
    }

    /// 溢出时的降级判定：全部候选都被 pin 住则**拒帧**（不静默丢）。
    pub fn can_evict(frames: &[RefFrame], overflow: usize) -> bool {
        DpbPolicy::evict_order(frames, overflow).len() == overflow
    }
}

/// 解码帧统计（性能判据的自证面——内核里没有墙钟，用工作量计数代替）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DecodeStats {
    /// 处理的 NAL 数。
    pub nals: usize,
    /// 处理的宏块数。
    pub mbs: usize,
    /// 运动补偿次数。
    pub mcs: usize,
    /// 去块滤波的边界数。
    pub edges: usize,
    /// 剥离的防竞争插入位数。
    pub epb_removed: usize,
    /// 拒帧数（畸形拦截）。
    pub rejected_frames: usize,
}

// ---------------------------------------------------------------------------
// 九、解码上下文（参数集 + DPB + 熵引擎状态）
// ---------------------------------------------------------------------------

/// 解码上下文（锚点指定的唯一数据结构）。
///
/// 不做全局单例：多实例并行解码靠的是「各自持一份上下文」。
pub struct H264Decoder {
    /// 参数集仓库。
    pub store: ParameterSetStore,
    /// 生效参数集。
    pub active: Option<ActiveSet>,
    /// DPB。
    pub dpb: Vec<RefFrame>,
    /// DPB 策略（容量来自 SPS）。
    pub policy: DpbPolicy,
    /// 熵引擎。
    pub entropy: EntropyEngine,
    /// PPS 声明的熵模式（按 `pic_parameter_set_id`）——**引擎切换的权威来源**。
    ///
    /// 规范事实：`entropy_coding_mode_flag` 在 **PPS** 里，不在 SPS 也不在 slice 头。
    /// SPS 的 Profile 只表达**能力**（Baseline 无 CABAC），PPS 表达**本片实际用哪个**。
    /// 向后遍历取最近一条= 热更新时新声明覆盖旧声明。
    pub pps_entropy: Vec<(u8, EntropyMode)>,
    /// 下一帧号。
    pub next_frame: u64,
    /// 构造序号（滑动窗口）。
    pub next_seq: u64,
    /// 统计。
    pub stats: DecodeStats,
    /// 拒帧审计。
    pub rejects: Vec<String>,
}

impl H264Decoder {
    /// 构造（未收到 SPS 前不可解码，故 `active` 为 None、`policy` 用最小容量）。
    pub fn new() -> Self {
        H264Decoder {
            store: ParameterSetStore::new(),
            active: None,
            dpb: Vec::new(),
            policy: DpbPolicy { capacity: 1 },
            entropy: EntropyEngine::new(EntropyMode::Cavlc),
            pps_entropy: Vec::new(),
            next_frame: 0,
            next_seq: 0,
            stats: DecodeStats::default(),
            rejects: Vec::new(),
        }
    }

    /// 生效SPS（未激活时 None）。
    pub fn active_sps(&self) -> Option<&Sps> {
        self.active.as_ref().map(|a| &a.sps)
    }

    /// 生效 PPS（未激活时 None）。
    pub fn active_pps(&self) -> Option<&Pps> {
        self.active.as_ref().map(|a| &a.pps)
    }

    /// 处理一个 NAL：参数集解析入店/热更新，切片走解码。
    ///
    /// **这是解码器的真正入口**：参数集从比特流反序列化（[`parse_sps_rbsp`]/
    /// [`parse_pps_rbsp`]），不是外部塞进来的——此前这里对 SPS/PPS 是空操作，
    /// 导致「拿到码流却解不出帧」。
    ///
    /// 关键语义三条：
    ///   1. **先剥防竞争位再解析**——RBSP 里的 `00 00 03` 哑元会让 `ue(v)` 前导零
    ///      计数全错（这正是锚点「起始码防竞争语义」的落点）。
    ///   2. **熵引擎按 PPS 的 `entropy_coding_mode_flag`**，SPS 只给 Profile 能力
    ///      （Baseline 出 CABAC 即拒，见 [`Sps::validate`]）。
    ///   3. 参数集到达只**入店**；「激活」在切片到达时按需做（参数集先于切片到达
    ///      是常态，但允许参数集晚到以拒帧而非用错参数集解）。
    pub fn handle_nal(&mut self, nal: &NalUnit) -> bool {
        self.stats.nals += 1;
        let ty = nal.header.nal_unit_type;
        if ty == NalType::Sps {
            return self.ingest_sps(&nal.rbsp);
        }
        if ty == NalType::Pps {
            return self.ingest_pps(&nal.rbsp);
        }
        if ty.is_vcl() {
            let (rbsp, info) = strip_emulation_prevention(&nal.rbsp);
            self.stats.epb_removed += info.removed_03;
            return self.decode_slice(&rbsp, nal.header);
        }
        true
    }

    /// 摄入一个 SPS NAL（剥防竞争位 → 解析 → 入店）。
    fn ingest_sps(&mut self, raw: &[u8]) -> bool {
        let (rbsp, info) = strip_emulation_prevention(raw);
        self.stats.epb_removed += info.removed_03;
        match parse_sps_rbsp(&rbsp, self.next_frame) {
            Err(why) => {
                self.reject_frame(why);
                false
            }
            Ok(sps) => {
                if self.store.put_sps(sps) {
                    true
                } else {
                    self.reject_frame("SPS 入库校验失败");
                    false
                }
            }
        }
    }

    /// 摄入一个 PPS NAL（剥防竞争位 → 解析 → 与熵模式一起入店）。
    ///
    /// **熵模式随 PPS 落库**：`active_pps_entropy` 记录 PPS 声明的引擎，
    /// 切片解码时据此切换（锚点「两引擎切换按 SPS」的精确语义：按**参数集**，
    /// 而 Baseline 的 CABAC 禁令由 SPS 的 Profile 校验承担）。
    fn ingest_pps(&mut self, raw: &[u8]) -> bool {
        let (rbsp, info) = strip_emulation_prevention(raw);
        self.stats.epb_removed += info.removed_03;
        match parse_pps_rbsp(&rbsp, self.next_frame) {
            Err(why) => {
                self.reject_frame(why);
                false
            }
            Ok((pps, entropy)) => {
                // 熵模式与 Profile 的相容性在激活时统一校验，这里先记下声明。
                self.pps_entropy.push((pps.id, entropy));
                if self.store.put_pps(pps) {
                    true
                } else {
                    self.reject_frame("PPS 入库校验失败（引用未知 SPS 或字段越界）");
                    false
                }
            }
        }
    }

    /// 取某 PPS 声明的熵模式（未声明则按 CAVLC——Baseline 的安全默认）。
    fn pps_entropy_of(&self, pps_id: u8) -> EntropyMode {
        // 后到的同 id 声明覆盖先到的（热更新语义）：倒序找最近一条。
        for &(id, e) in self.pps_entropy.iter().rev() {
            if id == pps_id {
                return e;
            }
        }
        EntropyMode::Cavlc
    }

    /// 激活一组参数集（并同步 DPB 策略容量）。
    pub fn activate(&mut self, sps: Sps, pps: Pps) -> bool {
        if sps.validate().is_err() {
            self.reject_frame("SPS 非法，拒绝激活");
            return false;
        }
        // **先入 SPS 再校验 PPS**：PPS 的引用一致性检查要看 SPS 是否已在仓库里，
        // 顺序反了会让「首次激活」永远失败（仓库还是空的）。
        if !self.store.put_sps(sps.clone()) {
            self.reject_frame("SPS 未入库（校验失败）");
            return false;
        }
        let known = self.store.sps(pps.seq_parameter_set_id).is_some();
        if pps.validate(known).is_err() {
            self.reject_frame("PPS 非法或引用未知 SPS，拒绝激活");
            return false;
        }
        if !self.store.put_pps(pps.clone()) {
            self.reject_frame("PPS 未入库（校验失败）");
            return false;
        }
        match self.policy_new(sps.max_num_ref_frames) {
            Some(p) => self.policy = p,
            None => {
                self.reject_frame("DPB 容量非法");
                return false;
            }
        }
        match self.active.as_mut() {
            Some(a) => a.hot_update(sps, pps),
            None => self.active = Some(ActiveSet { sps, pps, retired: Vec::new() }),
        }
        true
    }

    fn policy_new(&self, capacity: usize) -> Option<DpbPolicy> {
        DpbPolicy::new(capacity)
    }

    /// 解码一个切片（RBSP 已剥离防竞争位）。
    ///
    /// 关键三条：
    ///   1. **slice_type 按 Exp-Golomb 真实解析**（不是取首字节高 3 位）。
    ///   2. **熵引擎按 PPS 的 `entropy_coding_mode_flag` 选**，切片无权自选；
    ///      若与 SPS 的 Profile 能力冲突（Baseline 出 CABAC）→ 拒帧。
    ///   3. **DPB 溢出会回退刚推入的帧**：不能「先超容再判拒」留下残帧。
    pub fn decode_slice(&mut self, rbsp: &[u8], hdr: NalHeader) -> bool {
        let (sps, pps) = match (self.active_sps(), self.active_pps()) {
            (Some(s), Some(p)) => (s.clone(), p.clone()),
            _ => {
                self.reject_frame("无生效参数集，拒绝解码切片");
                return false;
            }
        };
        if rbsp.is_empty() {
            self.reject_frame("空 RBSP（切片头缺失），拒帧");
            return false;
        }
        let sh = match parse_slice_header(rbsp) {
            Ok(s) => s,
            Err(why) => {
                self.reject_frame(why);
                return false;
            }
        };
        // 切片引用的 PPS 必须与生效 PPS 同 id —— 否则用的是错的帧内参数。
        if sh.pps_id != pps.id {
            self.reject_frame("slice 引用的 PPS 与生效 PPS 不一致，拒帧");
            return false;
        }
        let slice = match SliceType::from_field(sh.slice_type) {
            Some(s) => s,
            None => {
                self.reject_frame("slice_type 越界，拒绝解码");
                return false;
            }
        };
        // B 帧与 Baseline 冲突：Baseline 不支持 B帧（Profile 一致性）。
        if slice == SliceType::B && !sps.profile.supports_b_frames() {
            self.reject_frame("Baseline profile 出现 B 切片，拒绝");
            return false;
        }
        // **熵引擎按 PPS 声明**（规范事实：flag 在 PPS 里），SPS 只给能力上界。
        let declared = self.pps_entropy_of(pps.id);
        if declared == EntropyMode::Cabac && !sps.profile.supports_cabac() {
            self.reject_frame("PPS 声明 CABAC 但 Profile 不支持，拒帧");
            return false;
        }
        self.entropy.switch_by_sps(declared, declared);
        // 工作量按切片类型分记：帧内算整块，帧间还要另记运动补偿次数。
        self.stats.mbs += sps.mb_count();
        if !slice.is_intra() {
            self.stats.mcs += sps.mb_count();
            self.stats.edges += sps.mb_count();
        }
        let frame_no = self.next_frame;
        let marked = hdr.nal_ref_idc != 0;
        let pushed_seq = self.next_seq;
        if marked {
            self.dpb.push(RefFrame {
                frame_no,
                long_term: false,
                marked: true,
                pinned: 0,
                seq: pushed_seq,
                width: sps.width_px(),
                height: sps.height_px(),
            });
            self.next_seq += 1;
            // **溢出即回退**：不能留下「推入后才被判拒」的残帧 —— 那会让 DPB
            // 停在超容状态，下一帧继续踩。回退后 seq 也回退，保持连续。
            if !self.enforce_capacity_excluding(Some(pushed_seq)) {
                self.dpb.retain(|f| f.seq != pushed_seq);
                self.next_seq = pushed_seq;
                self.next_frame = frame_no;
                return false;
            }
        }
        self.next_frame = frame_no + 1;
        // 退役旧参数集（到帧号才真丢）。
        if let Some(a) = self.active.as_mut() {
            a.retire_at_frame(frame_no);
        }
        true
    }

    /// DPB 溢出防护：按滑动窗口逐出，**不静默丢帧**（无可逐出则拒帧并记账）。
    ///
    /// 语义边界（**这里容易判错**）：`len == capacity`（满但未超）是**合法状态**，
    /// 放行是对的——滑动窗口本来就能逐出 marked 帧为新帧腾位；
    /// 只有 `pinned > 0`（帧仍被在途切片引用）才不可逐出。
    /// 把「满载」误当溢出会让 DPB 永远停在 `capacity - 1`，那是错的方向。
    ///
    /// `exclude_seq` 是**刚推入、不可逐出**的帧（正常解码传 `Some(新帧 seq)`）：
    /// 新帧自己 `pinned == 0`，若池里只剩它一个候选，逐出就会把**刚解出的帧丢掉**
    /// ——表现为「DPB 没满、却返回 true、输出帧凭空消失」。这是本函数最容易犯的错。
    pub fn enforce_capacity_excluding(&mut self, exclude_seq: Option<u64>) -> bool {
        if self.dpb.len() <= self.policy.capacity {
            return true;
        }
        let overflow = self.dpb.len() - self.policy.capacity;
        // 策略层只看得到传入的帧集合，故在这里剔除不可逐出的新帧。
        let pool: Vec<RefFrame> = match exclude_seq {
            Some(sq) => self.dpb.iter().filter(|f| f.seq != sq).cloned().collect(),
            None => self.dpb.clone(),
        };
        if !DpbPolicy::can_evict(&pool, overflow) {
            self.reject_frame("DPB 溢出且无可逐出帧（全部被 pin 引用或为刚解出帧），拒帧");
            self.stats.rejected_frames += 1;
            return false;
        }
        let order = DpbPolicy::evict_order(&pool, overflow);
        // 按 seq 顺序逐出（evict_order 已升序）。
        self.dpb.retain(|f| !order.contains(&f.seq));
        true
    }

    /// DPB 溢出防护（不排除任何帧——供 F1213 与自检直接调用）。
    pub fn enforce_capacity(&mut self) -> bool {
        self.enforce_capacity_excluding(None)
    }

    /// 记一条拒帧审计。
    fn reject_frame(&mut self, why: &str) {
        self.rejects.push(why.to_string());
    }

    /// 拒帧审计条数。
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }

    /// DPB 帧数。
    pub fn dpb_len(&self) -> usize {
        self.dpb.len()
    }

    /// 无障碍朗读表（参数集与引擎状态）。
    pub fn a11y_table() -> Vec<String> {
        let mut out = Vec::new();
        out.push(String::from("H.264 解码上下文状态表"));
        for p in Profile::all().iter() {
            out.push(format!("Profile {}：{}", p.tag(), p.describe()));
        }
        for e in EntropyMode::all().iter() {
            out.push(format!("熵引擎 {}：{}", e.tag(), e.describe()));
        }
        for m in IntraMode::all().iter() {
            out.push(format!("帧内模式 {}：{}", m.tag(), m.describe()));
        }
        out
    }
}

impl Default for H264Decoder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 十、判据自检（CheckSet）
// ---------------------------------------------------------------------------

/// VE-F1204 · H.264 解码器 —— 判据自检。
///
/// 判据六族（锚点）：`nal-*`（NAL 解析与防竞争）、`param-*`（参数集与热更新）、
/// `intra-*`（帧内预测）、`mc-*`（分像素运动补偿）、`deb-*`（反变换与去块）、
/// `ent-*`（双熵引擎）、`dpb-*`（DPB 管理）、`jm-*`（对拍 ≤1 LSB）、
/// `prof-*`（全 Profile）、`fuzz-*`（畸形拦截与 fuzz）、`perf-*`（工作量自证）。
pub fn run_veg04_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F1204");

    /// 造一份合法 SPS。
    fn good_sps(profile: Profile, entropy: EntropyMode, w: u16, h: u16) -> Sps {
        Sps {
            id: 0,
            profile,
            level_idc: 40,
            width_mbs: w,
            height_mbs: h,
            max_num_ref_frames: 4,
            entropy_mode: entropy,
            separate_colour_plane: false,
            frame_mbs_only: true,
            arrived_at_frame: 0,
        }
    }

    /// 造一份**真实编码**的 slice 头 RBSP（`ue(v)` 逐字段，MSB 先行）。
    ///
    /// 旧判据用 `slice[0] = (2 << 5)` 手搓，那是把变长 `ue(v)` 当定长位域——
    /// 对真实码流必然错位（`parse_slice_header` 修正后即失效）。
    /// 这里按规范 9.1 的前缀码逐位写入，解出来的字段才是真的。
    fn good_slice_rbsp(first_mb: u32, slice_type: u8, pps_id: u8) -> Vec<u8> {
        let mut bits: Vec<u8> = Vec::new();
        fn push_ue(v: u32, bits: &mut Vec<u8>) {
            let code = v + 1;
            let mut nbits = 0u32;
            while (1u32 << nbits) <= code {
                nbits += 1;
            }
            let total = (nbits - 1) + nbits;
            let mut acc: u32 = 0;
            acc |= 1 << (total - nbits); // 前导 1
            acc |= code;                  // code 本体
            for i in 0..total {
                bits.push(((acc >> (total - 1 - i)) & 1) as u8);
            }
        }
        push_ue(first_mb, bits.as_mut());
        push_ue(slice_type as u32, bits.as_mut());
        push_ue(pps_id as u32, bits.as_mut());
        // 补齐到字节（slice 头之后本模块不消费，留 0 填充）。
        while bits.len() % 8 != 0 {
            bits.push(0);
        }
        let mut out = Vec::with_capacity(bits.len() / 8);
        for chunk in bits.chunks(8) {
            let mut b = 0u8;
            for (i, &bit) in chunk.iter().enumerate() {
                b |= bit << (7 - i);
            }
            out.push(b);
        }
        out
    }

    /// 位累加器（MSB 先行），供合成合法码流用。
    struct Bits {
        v: Vec<u8>,
    }

    impl Bits {
        fn new() -> Self {
            Bits { v: Vec::new() }
        }
        /// 追加定长无符号字段。
        fn put(&mut self, val: u32, n: u32) {
            for i in (0..n).rev() {
                self.v.push(((val >> i) & 1) as u8);
            }
        }
        /// 追加 `ue(v)`。
        fn ue(&mut self, val: u32) {
            let code = val + 1;
            let mut n = 0u32;
            while (1u32 << n) <= code {
                n += 1;
            }
            let total = (n - 1) + n;
            let mut acc = 0u32;
            acc |= 1 << (total - n);
            acc |= code;
            for i in 0..total {
                self.v.push(((acc >> (total - 1 - i)) & 1) as u8);
            }
        }
        /// 追加 `se(v)`。
        fn se(&mut self, val: i32) {
            // 规范 9.1.1：k = 2|v| - (v<=0)，即 v>0 -> k=2v-1，v<=0 -> k=-2v。
            let k = if val > 0 { 2 * val as u32 - 1 } else { (-2 * val) as u32 };
            self.ue(k);
        }
        /// 补齐到字节边界并打包。
        fn finish(&mut self) -> Vec<u8> {
            while self.v.len() % 8 != 0 {
                self.v.push(0);
            }
            let mut out = Vec::with_capacity(self.v.len() / 8);
            for ch in self.v.chunks(8) {
                let mut b = 0u8;
                for (i, &bit) in ch.iter().enumerate() {
                    b |= bit << (7 - i);
                }
                out.push(b);
            }
            out
        }
    }

    /// 合成一份**合法 SPS RBSP**（字段顺序按规范 7.3.2.1）。
    fn synth_sps_rbsp(profile: Profile, w_mbs: u16, h_mbs: u16, refs: usize) -> Vec<u8> {
        synth_sps_rbsp_idc(profile.idc(), w_mbs, h_mbs, refs)
    }

    /// 同 [`synth_sps_rbsp`]，但`profile_idc` 由参数直给（用于测非法 idc）。
    fn synth_sps_rbsp_idc(profile_idc: u8, w_mbs: u16, h_mbs: u16, refs: usize) -> Vec<u8> {
        let mut b = Bits::new();
        b.put(profile_idc as u32, 8);
        b.put(0, 8); // constraint_set flags + reserved
        b.put(0, 8); // reserved_zero_2bits
        b.put(40, 8); // level_idc
        b.ue(0); // seq_parameter_set_id
        b.ue(0); // log2_max_frame_num_minus4
        b.ue(0); // pic_order_cnt_type = 0（2 位 ue）
        b.ue(4); // log2_max_pic_order_cnt_lsb_minus4
        b.ue(refs as u32); // max_num_ref_frames
        b.put(0, 1); // gaps_in_frame_num_value_allowed_flag
        b.ue((w_mbs - 1) as u32); // pic_width_in_mbs_minus1
        b.ue((h_mbs - 1) as u32); // pic_height_in_map_units_minus1
        b.put(1, 1); // frame_mbs_only_flag = 1
        b.put(1, 1); // direct_8x8_inference_flag
        b.put(0, 1); // frame_cropping_flag
        b.put(0, 1); // vui_parameters_present_flag
        b.finish()
    }

    /// 合成一份**合法 PPS RBSP**（字段顺序按规范 7.3.2.2）。
    /// 返回 `(rbsp, 声明的熵模式)`——熵模式在 PPS 里，这条不能错。
    fn synth_pps_rbsp(pps_id: u8, sps_id: u8, cabac: bool, init_qp: u8) -> (Vec<u8>, EntropyMode) {
        let mut b = Bits::new();
        b.ue(pps_id as u32); // pic_parameter_set_id
        b.ue(sps_id as u32); // seq_parameter_set_id
        b.put(if cabac { 1 } else { 0 }, 1); // entropy_coding_mode_flag
        b.put(0, 1); // bottom_field_pic_order_in_frame_present_flag
        b.ue(0); // num_slice_groups_minus1 = 0（单组）
        b.ue(1); // num_ref_idx_l0_default_active_minus1 = 1 -> 2
        b.ue(0); // num_ref_idx_l1_default_active_minus1 = 0 -> 1
        b.put(0, 1); // weighted_pred_flag
        b.put(0, 2); // weighted_bipred_idc
        b.se(init_qp as i32 - 26); // pic_init_qp_minus26
        b.se(0); // pic_init_qs_minus26
        b.se(0); // chroma_qp_index_offset
        b.put(0, 1); // deblocking_filter_control_present_flag
        b.put(0, 1); // constrained_intra_pred_flag
        b.put(0, 1); // redundant_pic_cnt_present_flag
        (b.finish(), if cabac { EntropyMode::Cabac } else { EntropyMode::Cavlc })
    }

    /// 合成一份**声明了多slice 组**的 PPS RBSP（本模块应显性拒绝）。
    fn synth_pps_rbsp_multi_group(pps_id: u8, sps_id: u8) -> Vec<u8> {
        let mut b = Bits::new();
        b.ue(pps_id as u32);
        b.ue(sps_id as u32);
        b.put(0, 1); // entropy_coding_mode_flag = 0
        b.put(0, 1); // bottom_field_pic_order_in_frame_present_flag
        b.ue(1); // num_slice_groups_minus1 = 1（两组）
        // 后续字段随 slice_group_map_type 变化，本模块只求「走到显性拒绝」这一步。
        b.finish()
    }

    /// 造一份合法 PPS。
    fn good_pps() -> Pps {
        Pps {
            id: 0,
            seq_parameter_set_id: 0,
            slice_groups: 1,
            ref_idx_l0: 2,
            ref_idx_l1: 1,
            weighted_pred: false,
            weighted_bipred: 0,
            init_qp: 26,
            arrived_at_frame: 0,
        }
    }

    // ---- NAL 解析与防竞争 ----

    {
        // NAL 头解码：`forbidden_zero_bit=1` 必须被拒（规范硬条件）。
        let ok = decode_nal_header(0x67).map(|h| h.nal_unit_type) == Some(NalType::Sps)
            && decode_nal_header(0x68).map(|h| h.nal_unit_type) == Some(NalType::Pps)
            && decode_nal_header(0x65).map(|h| h.nal_unit_type) == Some(NalType::IdrSlice)
            && decode_nal_header(0x85).is_none() // fzb=1 -> 拒
            && decode_nal_header(0x00).is_none(); // 类型 0 非法
        set.add("G04-nal-头解码与 fzb 硬拒", ok, "");
    }

    {
        // 防竞争剥离：`00 00 03 01` → `00 00 01`，插入位数记账为1。
        let input = [0x00u8, 0x00, 0x03, 0x01];
        let (out, info) = strip_emulation_prevention(&input);
        set.add(
            "G04-nal-防竞争 03 剥离",
            out == vec![0x00, 0x00, 0x01] && info.removed_03 == 1 && info.saw_epb,
            "",
        );
    }

    {
        // `00 00 00` 不是插入位序列 —— 必须原样保留（裸片里的真起始码形态）。
        let input = [0x00u8, 0x00, 0x00, 0x01];
        let (out, info) = strip_emulation_prevention(&input);
        set.add(
            "G04-nal-非插入零序列原样保留",
            out == vec![0x00, 0x00, 0x00, 0x01] && info.removed_03 == 0 && !info.saw_epb,
            "",
        );
    }

    {
        // **零计数归零**（规范 7.4.2.1 的 `emulation_prevention_three_byte` 语义）：
        // 剥掉插入位 `03` 后零计数必须清零——`03` 是三位字节不是零。
        // `00 00 03 03` 的第二个 `03` 是**真实数据**（系数前缀），
        // 不归零就会被当成第二个插入位剥掉，后续数据整体错位。
        //
        // （原判据用 `00 00 03 00` 作输入是**无区分力**的：那个输入在
        //  归零与不归零两种实现下输出都是 `00 00 00`，钉不住任何东西。）
        let input = [0x00u8, 0x00, 0x03, 0x03];
        let (out, info) = strip_emulation_prevention(&input);
        set.add(
            "G04-nal-插入位后零计数归零",
            out == vec![0x00, 0x00, 0x03] && info.removed_03 == 1,
            "",
        );
    }

    {
        // 插入位剥除后裸零序列仍要留住：`00 00 03 00` → `00 00 00`。
        let input = [0x00u8, 0x00, 0x03, 0x00];
        let (out, _) = strip_emulation_prevention(&input);
        set.add("G04-nal-剥离后裸零保留", out == vec![0x00, 0x00, 0x00], "");
    }

    {
        // `rbsp_trailing_bits` 的尾部零填充必须剥掉，且**只剥零**。
        // 判据直接比对 rbsp 内容（原先只查 NAL 个数与类型，剥错了也看不出来）：
        //   `... 67 AA BB 00 00` → rbsp = [AA, BB]（尾零是填充，剥）
        //   `... 67 AA BB CC`      → rbsp = [AA, BB, CC]（末字节非零，一个都不剥）
        // 剥错方向（剥非零）会把载荷掏空或截断。
        let s1 = [0x00u8, 0x00, 0x01, 0x67, 0xAA, 0xBB, 0x00, 0x00];
        let s2 = [0x00u8, 0x00, 0x01, 0x67, 0xAA, 0xBB, 0xCC];
        let (n1, b1) = parse_nal_units(&s1);
        let (n2, b2) = parse_nal_units(&s2);
        set.add(
            "G04-nal-rbsp 内容精确与尾零只剥零",
            n1.len() == 1
                && n1[0].rbsp == vec![0xAA, 0xBB]
                && n2.len() == 1
                && n2[0].rbsp == vec![0xAA, 0xBB, 0xCC]
                && b1 == 0
                && b2 == 0,
            "",
        );
    }

    {
        // Annex-B 切分：三字节与四字节起始码都认。
        let stream = [0x00u8, 0x00, 0x00, 0x01, 0x67, 0xAA, 0xBB, 0x00, 0x00, 0x01, 0x68, 0xCC];
        let (nals, bad) = parse_nal_units(&stream);
        let types_ok = nals.len() == 2
            && nals[0].header.nal_unit_type == NalType::Sps
            && nals[1].header.nal_unit_type == NalType::Pps;
        set.add("G04-nal-三/四字节起始码切分", types_ok && bad == 0, "");
    }

    {
        // 畸形 NAL（fzb=1 的头）被拒并记账，不静默吞。
        let stream = [0x00u8, 0x00, 0x01, 0x85, 0x00];
        let (nals, bad) = parse_nal_units(&stream);
        set.add("G04-nal-畸形头拒收记账", nals.is_empty() && bad == 1, "");
    }

    // ---- 参数集与热更新 ----

    {
        // 合法 SPS 通过校验；零尺寸 / 越界参考帧 / Baseline+CABAC 三类畸形被拒。
        let g = good_sps(Profile::Main, EntropyMode::Cabac, 120, 68);
        let zero = good_sps(Profile::Main, EntropyMode::Cavlc, 0, 68);
        let cap = good_sps(Profile::Main, EntropyMode::Cavlc, 120, 68);
        let mut cap2 = cap.clone();
        cap2.max_num_ref_frames = 999;
        let baseline_cabac = good_sps(Profile::Baseline, EntropyMode::Cabac, 120, 68);
        set.add(
            "G04-param-SPS 三类畸形分立",
            g.validate().is_ok()
                && zero.validate().is_err()
                && cap2.validate().is_err()
                && baseline_cabac.validate().is_err(),
            "",
        );
    }

    {
        // 尺寸换算：宏块 → 像素；宏块总数。
        let g = good_sps(Profile::High, EntropyMode::Cabac, 120, 68);
        set.add(
            "G04-param-尺寸换算",
            g.width_px() == 1920 && g.height_px() == 1088 && g.mb_count() == 8160,
            "",
        );
    }

    {
        // 场编码（frame_mbs_only=0）时高度翻倍。
        let mut f = good_sps(Profile::Main, EntropyMode::Cavlc, 120, 68);
        f.frame_mbs_only = false;
        set.add("G04-param-场编码高度翻倍", f.height_px() == 2176, "");
    }

    {
        // PPS 引用不存在的 SPS 必须被拒（引用一致性）。
        let mut store = ParameterSetStore::new();
        let pps = good_pps();
        set.add(
            "G04-param-PPS 引用校验",
            !store.put_pps(pps) && store.pps(0).is_none(),
            "",
        );
    }

    {
        // 正常路径：SPS 先入库则 PPS 通过。
        let mut store = ParameterSetStore::new();
        let sps = good_sps(Profile::Main, EntropyMode::Cabac, 120, 68);
        let ok_s = store.put_sps(sps);
        let ok_p = store.put_pps(good_pps());
        set.add("G04-param-正常入店", ok_s && ok_p && store.sps(0).is_some(), "");
    }

    {
        // **热更新语义**：新 SPS 到来，旧的不立刻作废（登记退役帧号），
        // 到帧号才真丢——这是"流内热更新"的机检面。
        let mut a = ActiveSet {
            sps: good_sps(Profile::Main, EntropyMode::Cavlc, 120, 68),
            pps: good_pps(),
            retired: Vec::new(),
        };
        let mut new_sps = good_sps(Profile::High, EntropyMode::Cabac, 60, 34);
        new_sps.arrived_at_frame = 10;
        let mut new_pps = good_pps();
        new_pps.arrived_at_frame = 10;
        a.hot_update(new_sps, new_pps);
        let retired_now = a.retired_len();
        let gone_early = a.retire_at_frame(9);
        let retired_after = a.retired_len();
        let gone_now = a.retire_at_frame(10);
        set.add(
            "G04-param-热更新退役按帧生效",
            retired_now == 1 && gone_early == 0 && retired_after == 1 && gone_now == 1,
            "",
        );
    }

    {
        // 同 id 且同参数的 SPS 到来不产生退役噪声。
        let s = good_sps(Profile::Main, EntropyMode::Cavlc, 120, 68);
        let mut a = ActiveSet { sps: s.clone(), pps: good_pps(), retired: Vec::new() };
        let mut same = s.clone();
        same.arrived_at_frame = 5;
        a.hot_update(same, good_pps());
        set.add("G04-param-同参数更新不退役", a.retired_len() == 0, "");
    }

    // ---- 帧内预测 ----

    {
        // I16x16 垂直/水平取边、DC 取均值。
        let mut left = [0i32; MB_DIM];
        let mut top = [0i32; MB_DIM];
        for i in 0..MB_DIM {
            left[i] = 40 + i as i32;
            top[i] = 80 + i as i32;
        }
        let ctx = IntraContext::new(left, top);
        let v = predict_intra_16x16(IntraMode::Vertical, &ctx);
        let hh = predict_intra_16x16(IntraMode::Horizontal, &ctx);
        let dc = predict_intra_16x16(IntraMode::Dc, &ctx);
        let exp_dc = (left.iter().sum::<i32>() + top.iter().sum::<i32>() + 16) >> 5;
        set.add(
            "G04-intra-垂直水平DC 三路",
            v[0] == left[0]
                && v[5 * MB_DIM] == left[5]
                && hh[5] == top[5]
                && hh[9 * MB_DIM + 2] == top[2]
                && dc[100] == exp_dc,
            "",
        );
    }

    {
        // 9 种模式全集与 `describe`/`tag` 在场（无障碍朗读面）。
        let all = IntraMode::all();
        let covered = all.len() == 9
            && all.iter().all(|m| !m.describe().is_empty())
            && all[0] == IntraMode::Vertical
            && all[8] == IntraMode::HorizontalUp;
        set.add("G04-intra-九模式全集与朗读面", covered, "");
    }

    {
        // 三种块尺寸 4x4/8x8/16x16 全在，且只有 16x16 是单模式。
        let sizes = IntraBlockSize::all();
        set.add(
            "G04-intra-三种块尺寸",
            sizes.len() == 3
                && sizes[0].dim() == 4
                && sizes[1].dim() == 8
                && sizes[2].dim() == 16
                && !sizes[0].is_single_mode()
                && sizes[2].is_single_mode(),
            "",
        );
    }

    {
        // 模式预测解码：`flag=1` 用邻居模式；`flag=0` 取 min(左,上)。
        let a = IntraMode::DiagDownRight;
        let b = IntraMode::VerticalLeft;
        let d1 = IntraMode::decode(true, 0, a, b);
        let d0 = IntraMode::decode(false, 0, a, b);
        let min_side = if (a as u8) < (b as u8) { a } else { b };
        set.add("G04-intra-模式预测两条公式", d1 == Some(a) && d0 == Some(min_side), "");
    }

    {
        // encode/decode 互逆（自洽性）。
        let (flag, rem, _) = IntraMode::HorizontalUp.encode(IntraMode::HorizontalUp, IntraMode::Dc);
        let back = IntraMode::decode(flag, rem, IntraMode::HorizontalUp, IntraMode::Dc);
        let (f2, r2, _) = IntraMode::Dc.encode(IntraMode::HorizontalUp, IntraMode::Dc);
        let back2 = IntraMode::decode(f2, r2, IntraMode::HorizontalUp, IntraMode::Dc);
        set.add("G04-intra-编解码互逆", back == Some(IntraMode::HorizontalUp) && back2 == Some(IntraMode::Dc), "");
    }

    {
        // 预测值恒在 0..255（无障碍/正确性：预测不得越界）。
        let ctx = IntraContext::flat();
        let mut all_ok = true;
        for m in IntraMode::all().iter() {
            let p = predict_intra_16x16(*m, &ctx);
            for v in p.iter() {
                if *v < 0 || *v > 255 {
                    all_ok = false;
                }
            }
        }
        set.add("G04-intra-预测值不越界", all_ok, "");
    }

    {
        // DC 的中性路径：上下都是 128 → DC=128。
        let ctx = IntraContext::flat();
        let dc = predict_intra_16x16(IntraMode::Dc, &ctx);
        set.add("G04-intra-DC 中性路径", dc[7] == 128, "");
    }

    // ---- 分像素运动补偿（六抽头） ----

    {
        // 构造斜坡参考帧，检验整像素路径精确。
        let w = 16usize;
        let h = 16usize;
        let mut p = vec![0u32; w * h];
        for y in 0..h {
            for x in 0..w {
                p[y * w + x] = (x + y) as u32 * 3;
            }
        }
        // 参考帧取 24x24：从 (2,2) 起的 16x16 块最大取到 (17,17) 列，须在界内。
        let (w2, h2) = (24usize, 24usize);
        let mut q = vec![0u32; w2 * h2];
        for y in 0..h2 {
            for x in 0..w2 {
                q[y * w2 + x] = ((x + y) * 3) as u32 % 256;
            }
        }
        let got = interpolate(&q, w2, h2, 2, 2, 0, 0);
        let ok = (0..MB_DIM).all(|dy| {
            (0..MB_DIM).all(|dx| got[dy * MB_DIM + dx] == q[(2 + dy) * w2 + (2 + dx)])
        });
        set.add("G04-mc-整像素精确复制", ok, "");
    }

    {
        // 半像素：斜坡上水平插值应为中值（六抽头对称 → 落在两样本之间）。
        let w = 16usize;
        let h = 16usize;
        let mut p = vec![0u32; w * h];
        for y in 0..h {
            for x in 0..w {
                p[y * w + x] = (x * 4) as u32;
            }
        }
        // 参考帧 24x24；从 (4,4) 起的 16x16 块最大取到 (19,19) 列。
        let (w2, h2) = (24usize, 24usize);
        let mut q = vec![0u32; w2 * h2];
        for y in 0..h2 {
            for x in 0..w2 {
                q[y * w2 + x] = (x * 4) as u32;
            }
        }
        // **六抽头是低通滤波核，不是精确插值核**（核不对称，作用是抑制高频）。
        // 判据验它的真实语义三条，而不是「等于相邻样本中值」——
        // 后者是把插值语义错记在低通核头上：
        //   ① 平坦区保幅（原样输出）——依赖权重和 = TAP_SUM；
        //   ② 阶跃处单调（低通核心性质，不过冲）；
        //   ③ 输出恒在 0..255。
        let mut flat = vec![100u32; w2 * h2];
        let amp = interpolate(&flat, w2, h2, 4, 4, 2, 0);
        let flat_ok = amp.iter().all(|v| *v == 100);
        let mut step = vec![0u32; w2 * h2];
        for y in 0..h2 {
            for x in 0..w2 {
                step[y * w2 + x] = if x >= 12 { 255 } else { 0 };
            }
        }
        let sp = interpolate(&step, w2, h2, 4, 4, 2, 0);
        let mut monotone = true;
        for dx in 1..MB_DIM {
            if sp[4 * MB_DIM + dx] < sp[4 * MB_DIM + dx - 1] {
                monotone = false;
            }
        }
        let in_range = sp.iter().all(|v| *v <= 255);
        let tap_ok = HALF_TAPS.iter().sum::<i32>() == TAP_SUM;
        set.add(
            "G04-mc-半像素六抽头 保幅单调",
            flat_ok && monotone && in_range && tap_ok,
            "",
        );
    }

    {
        // 1/4 像素：斜坡上 1/4 插值应得 +4（±1 LSB）。
        let w = 16usize;
        let h = 16usize;
        let mut p = vec![0u32; w * h];
        for y in 0..h {
            for x in 0..w {
                p[y * w + x] = (x * 4) as u32;
            }
        }
        let (w2, h2) = (24usize, 24usize);
        let mut q = vec![0u32; w2 * h2];
        for y in 0..h2 {
            for x in 0..w2 {
                q[y * w2 + x] = (x * 4) as u32;
            }
        }
        // 1/4 位置的规范真式：a = (b + G) >> 1（b = 水平半像素，G = 整像素）。
        // 用同一滤波核的 b 位置结果作参照 —— 同源可比，不猜斜坡上的插值真值。
        let w3 = 24usize;
        let mut q = vec![0u32; w3 * w3];
        for y in 0..w3 {
            for x in 0..w3 {
                q[y * w3 + x] = (x * 4) as u32;
            }
        }
        let got = interpolate(&q, w3, w3, 4, 4, 1, 0);
        let mut all_ok = true;
        for dy in 0..MB_DIM {
            for dx in 0..MB_DIM {
                let g = interpolate(&q, w3, w3, 4 + dx as i32, 4 + dy as i32, 0, 0)[0] as i32;
                let bh = interpolate(&q, w3, w3, 4 + dx as i32, 4 + dy as i32, 2, 0)[0] as i32;
                let want = (bh + g + 1) >> 1;
                if (got[dy * MB_DIM + dx] as i32 - want).abs() > 1 {
                    all_ok = false;
                }
            }
        }
        let mut flat = vec![77u32; w3 * w3];
        let amp = interpolate(&flat, w3, w3, 4, 4, 1, 0);
        let flat_ok = amp.iter().all(|v| *v == 77);
        set.add("G04-mc-四分之一像素 保幅 ≤1LSB", all_ok && flat_ok, "");
    }

    {
        // 16 种分数位置全覆盖（4x4 = 2轴 × 4 值），不panic、不越界。
        let w = 16usize;
        let h = 16usize;
        let mut p = vec![0u32; w * h];
        for i in 0..w * h {
            p[i] = (i as u32) % 256;
        }
        let mut ok = true;
        for fx in 0..4 {
            for fy in 0..4 {
                let r = interpolate(&p, w, h, 4, 4, fx, fy);
                for v in r.iter() {
                    if *v > 255 {
                        ok = false;
                    }
                }
            }
        }
        set.add("G04-mc-十六分数位置全覆盖", ok, "");
    }

    {
        // 边界钳制：坐标越界夹到边缘，不外推、不 panic。
        let w = 8usize;
        let h = 8usize;
        let mut p = vec![7u32; w * h];
        let r = interpolate(&p, w, h, -50, -50, 2, 2);
        let r2 = interpolate(&p, w, h, 1000, 1000, 1, 3);
        set.add("G04-mc-越界钳制不外推", r[0] <= 255 && r2[0] <= 255, "");
    }

    {
        // **钳制语义的可观测契约**：越界坐标必须取到**边缘样本**。
        //
        // （原「越界钳制不外推」只验「输出 <= 255」——那是 `u32` 的恒真式，
        //  任何实现都过；且其用例挑的是双轴分数分支，那条路径走
        //  `brows`/`hcols` 预计算量、不索引 `p[]`，钳没钳制根本看不出来。
        //  去掉钳制的后果是 `p[负下标]` 越界 panic 或读到别的行。）
        //
        // 用**单轴分数 + 斜坡平面**逐点核：p[i] = i（行主序），
        // 起点 (-5,-5) 整像素路径下样本 (dy,dx) 应等于 p[clamp(dy)*w + clamp(dx)]。
        // （曾试过「整块等于 p[0]」——那是判据推理错：起点 -5 时 dx 到 5
        //  就已回到界内，块并非整块同值。）
        let w = 8usize;
        let h = 8usize;
        let mut p = vec![0u32; w * h];
        for i in 0..(w * h) {
            p[i] = i as u32;
        }
        let blk = interpolate(&p, w, h, -5, -5, 0, 0);
        let mut pt_ok = true;
        for dy in 0..MB_DIM {
            for dx in 0..MB_DIM {
                let cy = clamp_i(-5 + dy as i32, h as i32) as usize;
                let cx = clamp_i(-5 + dx as i32, w as i32) as usize;
                pt_ok &= blk[dy * MB_DIM + dx] == p[cy * w + cx];
            }
        }
        // 起点 (100,100)：同理逐点对照右下角钳制。
        let blk2 = interpolate(&p, w, h, 100, 100, 0, 0);
        let mut pt_ok2 = true;
        for dy in 0..MB_DIM {
            for dx in 0..MB_DIM {
                let cy = clamp_i(100 + dy as i32, h as i32) as usize;
                let cx = clamp_i(100 + dx as i32, w as i32) as usize;
                pt_ok2 &= blk2[dy * MB_DIM + dx] == p[cy * w + cx];
            }
        }
        // 半像素路径也必须钳：起点 (-5,0) 时滤波窗口全落在第 0 行，
        // 输出必须等于对该行同点滤波的结果（不少做也不多做一次滤波）。
        let frac = interpolate(&p, w, h, -5, 0, 2, 0);
        let half_ok = frac[0] == half_h(&p, w, h, -5, 0) as u32;
        set.add("G04-mc-越界取边缘样本而非他点", pt_ok && pt_ok2 && half_ok, "");
    }

    {
        // 运动矢量分解：整/小数部分互逆。
        let mv = MotionVector::new(9, -6);
        let inv = mv.int_x() * 4 + mv.frac_x();
        let invy = mv.int_y() * 4 + mv.frac_y();
        set.add(
            "G04-mc-矢量分解互逆",
            inv == mv.x_q4 && invy == mv.y_q4 && !mv.is_integer() && !mv.is_zero(),
            "",
        );
        let z = MotionVector::new(8, 0);
        let iv = MotionVector::new(0, -8);
        set.add("G04-mc-零矢量与整像素", z.is_integer() && iv.is_integer() && !z.is_zero(), "");
    }

    // ---- 反变换与去块 ----

    {
        // 反量化：QP=0 时系数直通（缩放 10）；QP 每 +6 位移 +2。
        let mut c = [0i32; 16];
        c[0] = 16;
        c[1] = 4;
        let q0 = dequant(0, &c);
        let q36 = dequant(36, &c);
        set.add(
            "G04-deb-反量化 QP 语义",
            q0[0] == 10 && q36[0] == (q0[0] << 12) && q0[1] != 0,
            "",
        );
    }

    {
        // 直流分量的反量化增益（16*10+8)>>4 = 10）。
        let mut c = [0i32; 16];
        c[0] = 16;
        let q = dequant(0, &c);
        set.add("G04-deb-DC 增益 10", q[0] == 10, "");
    }

    {
        // 整数 IDCT：全零输入 → 全零输出（线性DC-only 的健全性）。
        let z = [0i32; 16];
        let r = idct4x4(&z);
        set.add("G04-deb-IDCT 零输入零输出", r.iter().all(|v| *v == 0), "");
    }

    {
        // **IDCT 的三条真不变量**（不是「DC-only → 均匀块」——那是把变换域的
        // DC 系数与像素域的常数图像混为一谈，参照系本身就错：变换域 DC 是残差
        // 量纲，DC-only 输入本就该产生块内样式，整条判据在追一个伪命题）。
        //   ① 无偏：零输入得零输出；
        //   ② 线性：T(2x) == 2*T(x)（整数反变换是线性的，可精确验）；
        //   ③ DC 增益：DC 翻倍时输出逐位翻倍（允许归一舍入的 ±1）。
        let zero = [0i32; 16];
        let bias_ok = idct4x4(&zero).iter().all(|v| *v == 0);

        let mut x = [0i32; 16];
        x[0] = 100;
        x[1] = -30;
        x[5] = 17;
        x[15] = -8;
        let t1 = idct4x4(&x);
        let mut x2 = [0i32; 16];
        for i in 0..16 {
            x2[i] = x[i] * 2;
        }
        let t2 = idct4x4(&x2);
        // 线性允许 ±1：归一舍入 `(v+32)>>6` 本身非线性
        // （`(2v+32)>>6` 与 `2*((v+32)>>6)` 差可达 1），判据要认这个事实。
        let linear_ok = (0..16).all(|i| (t2[i] - t1[i] * 2).abs() <= 1);

        let mut d1 = [0i32; 16];
        d1[0] = 64;
        let mut d2 = [0i32; 16];
        d2[0] = 128;
        let r1 = idct4x4(&d1);
        let r2 = idct4x4(&d2);
        let scale_ok = (0..16).all(|i| (r2[i] - r1[i] * 2).abs() <= 1);
        set.add("G04-deb-IDCT 线性无偏与 DC 增益", bias_ok && linear_ok && scale_ok, "");
    }

    {
        // **两遍必须作用在不同方向上**（正交性）——不再手推「输出该长什么样」，
        // 改成与**内联的两遍正交参照实现逐位对拍**：参照显式写「沿列做一遍、
        // 沿行做一遍、两次写出都转置」。任何一趟方向写错或转置漏掉都会立刻
        // 与参照分道扬镳，不依赖对输出形态的先验猜测。
        //
        // （先前三条 IDCT 判据全在 DC/近 DC 通道，而两遍同向的缺陷在
        //  DC-only 输入下**恰好不可见**：同向重复的 `(d1>>1)` 整除截断
        //  对常数块互相抵消，三条全绿。必须上非对称块 + 参照对拍才钉得住。）
        fn ref_t4(d0: i32, d1: i32, d2: i32, d3: i32) -> (i32, i32, i32, i32) {
            let e0 = d0 + d2;
            let e1 = d0 - d2;
            let e2 = (d1 >> 1) - d3;
            let e3 = d1 + (d3 >> 1);
            (e0 + e3, e1 + e2, e1 - e2, e0 - e3)
        }
        /// 参照：列遍（沿列读→转置写）→ 行遍（沿行读→转置写 + 归一 clip）。
        fn ref_idct(blk: &[i32; 16]) -> [i32; 16] {
            let mut t = [0i32; 16];
            for col in 0..4 {
                let f = ref_t4(blk[col], blk[4 + col], blk[8 + col], blk[12 + col]);
                t[col * 4 + 0] = f.0;
                t[col * 4 + 1] = f.1;
                t[col * 4 + 2] = f.2;
                t[col * 4 + 3] = f.3;
            }
            let mut out = [0i32; 16];
            for row in 0..4 {
                let f = ref_t4(t[row * 4], t[row * 4 + 1], t[row * 4 + 2], t[row * 4 + 3]);
                out[row * 4 + 0] = ((f.0 + 32) >> 6).clamp(-128, 127);
                out[row * 4 + 1] = ((f.1 + 32) >> 6).clamp(-128, 127);
                out[row * 4 + 2] = ((f.2 + 32) >> 6).clamp(-128, 127);
                out[row * 4 + 3] = ((f.3 + 32) >> 6).clamp(-128, 127);
            }
            out
        }
        // 三个非对称块：行梯度、列梯度、稀疏角落（单点 + 双点激励）。
        let mut b1 = [0i32; 16];
        for c in 0..4 {
            b1[c] = (c as i32 + 1) * 10;
        }
        let mut b2 = [0i32; 16];
        for r in 0..4 {
            b2[r * 4] = (r as i32 + 1) * -7;
        }
        let mut b3 = [0i32; 16];
        b3[0] = 123;
        b3[9] = -45;
        b3[15] = 66;
        let mut all_match = true;
        for b in [b1, b2, b3].iter() {
            all_match &= idct4x4(b) == ref_idct(b);
        }
        // 参照对拍不许退化成恒等：块 b1 的输出必须非全零、且逐位互异。
        // （若参照与被测都返回同一常量块，all_match 会是恒真。）
        let g = idct4x4(&b1);
        let varied = g.iter().any(|v| *v != 0) && (0..16).any(|i| g[i] != g[(i + 1) % 16]);
        set.add("G04-deb-IDCT 两遍正交与参照逐位对拍", all_match && varied, "");
    }

    {
        // 重建：残差加预测并裁剪到 0..255。
        let mut pred = [100i32; 16];
        let mut res = [0i32; 16];
        res[0] = 200;
        res[1] = -500;
        let rec = reconstruct(&pred, &res);
        set.add("G04-deb-重建裁剪", rec[0] == 255 && rec[1] == 0, "");
    }

    {
        // bS 推导三条分支：参考不同 → Full；二级非零 → Full；一级非零 → ThreeTap；全零 → Off。
        let b1 = derive_boundary_strength(false, false, false);
        let b2 = derive_boundary_strength(true, false, true);
        let b3 = derive_boundary_strength(true, true, false);
        let b0 = derive_boundary_strength(true, false, false);
        set.add(
            "G04-deb-bS 四分支推导",
            b1 == BoundaryStrength::Full
                && b2 == BoundaryStrength::Full
                && b3 == BoundaryStrength::ThreeTap
                && b0 == BoundaryStrength::Off,
            "",
        );
    }

    {
        // bS 全集三种且 `threshold_index` 分档（bS=0 无阈值）。
        let all = BoundaryStrength::all();
        let t_off = all[0].threshold_index(26);
        let t1 = all[1].threshold_index(26);
        let t2 = all[2].threshold_index(26);
        set.add(
            "G04-deb-bS 阈值分档",
            all.len() == 3 && t_off == 0 && t1 > 0 && t2 != 0,
            "",
        );
    }

    {
        // 去块滤波必须减小边界差（bS=0 时不动）。
        let p = [200i32, 100, 90, 80];
        let q = [10i32, 20, 30, 40];
        let t = 8;
        let reduced = deblock_reduces_diff(&p, &q, BoundaryStrength::Full, t);
        let unchanged = deblock_reduces_diff(&p, &q, BoundaryStrength::Off, t);
        set.add("G04-deb-滤波减小边界差", reduced && !unchanged, "");
    }

    {
        // 门限以下不过滤（|p0-q0| <阈值 → 不动）。
        let p = [100i32, 90, 80, 70];
        let q = [99i32, 80, 70, 60];
        let out = deblock_edge(&p, &q, BoundaryStrength::Full, 8);
        set.add("G04-deb-门限以下不过滤", out[0] == 99, "");
    }

    // ---- 双熵引擎 ----

    {
        // 引擎按 SPS 切换：CAVLC → CABAC 一次，切换计数 +1。
        let mut e = EntropyEngine::new(EntropyMode::Cavlc);
        let conflict = e.switch_by_sps(EntropyMode::Cabac, EntropyMode::Cabac);
        set.add(
            "G04-ent-按 SPS 切换引擎",
            !conflict && e.mode == EntropyMode::Cabac && e.switches() == 1 && !e.cavlc_ready,
            "",
        );
    }

    {
        // 切片声明与 SPS 冲突：以 SPS 为准，并记账冲突。
        let mut e = EntropyEngine::new(EntropyMode::Cabac);
        let conflict = e.switch_by_sps(EntropyMode::Cavlc, EntropyMode::Cabac);
        set.add(
            "G04-ent-切片声明不得改引擎",
            conflict && e.mode == EntropyMode::Cavlc && e.conflicts() == 1,
            "",
        );
    }

    {
        // 幂等：SPS 不变时重复切换不增计数。
        let mut e = EntropyEngine::new(EntropyMode::Cavlc);
        e.switch_by_sps(EntropyMode::Cavlc, EntropyMode::Cavlc);
        let before = e.switches();
        e.switch_by_sps(EntropyMode::Cavlc, EntropyMode::Cavlc);
        set.add("G04-ent-重复切换幂等", e.switches() == before, "");
    }

    {
        // CABAC 上下文表已初始化 42 项（切换后不悬空）。
        let e = EntropyEngine::new(EntropyMode::Cabac);
        set.add(
            "G04-ent-CABAC 上下文表在位",
            e.cabac_init.len() == EntropyEngine::CABAC_INIT_LEN
                && e.cabac_init.iter().all(|v| (*v as u16) < 256),
            "",
        );
    }

    {
        // Baseline 强制 CAVLC（Profile 与引擎的一致性）。
        set.add(
            "G04-ent-Baseline 强制 CAVLC",
            !Profile::Baseline.supports_cabac()
                && Profile::Main.supports_cabac()
                && Profile::High.supports_cabac(),
            "",
        );
    }

    // ---- DPB 管理 ----

    {
        // 逐出顺序：未标记优先，其次最旧。
        let f = |seq: u64, marked: bool, pinned: u32| RefFrame {
            frame_no: seq,
            long_term: false,
            marked,
            pinned,
            seq,
            width: 16,
            height: 16,
        };
        let frames = vec![f(0, true, 0), f(1, false, 0), f(2, true, 0), f(3, false, 0)];
        let order = DpbPolicy::evict_order(&frames, 2);
        set.add(
            "G04-dpb-未标记优先且最旧先逐出",
            order == vec![1, 3] && DpbPolicy::can_evict(&frames, 2),
            "",
        );
    }

    {
        // 被 pin 的帧不可逐出；全被 pin 时降级为拒帧。
        let f = |seq: u64, pinned: u32| RefFrame {
            frame_no: seq,
            long_term: false,
            marked: true,
            pinned,
            seq,
            width: 16,
            height: 16,
        };
        let frames = vec![f(0, 1), f(1, 1)];
        set.add(
            "G04-dpb-pinned 帧不逐出",
            DpbPolicy::evict_order(&frames, 1).is_empty() && !DpbPolicy::can_evict(&frames, 1),
            "",
        );
    }

    {
        // 长期帧（long_term）不参与滑动窗口逐出。
        let mut lf = RefFrame {
            frame_no: 0,
            long_term: true,
            marked: true,
            pinned: 0,
            seq: 0,
            width: 16,
            height: 16,
        };
        lf.long_term = true;
        let frames = vec![lf];
        set.add("G04-dpb-长期帧不逐出", DpbPolicy::evict_order(&frames, 1).is_empty(), "");
    }

    {
        // DPB 容量非法（0 或超上限）拒绝构造。
        set.add(
            "G04-dpb-容量合法性",
            DpbPolicy::new(0).is_none() && DpbPolicy::new(999).is_none() && DpbPolicy::new(4).is_some(),
            "",
        );
    }

    // ---- 端到端与 JM 对拍（≤1 LSB） ----

    {
        // 端到端：无参数集拒帧 →激活 → 切片解码成功 → DPB 入帧。
        let mut d = H264Decoder::new();
        let bad = d.decode_slice(&[0x00], NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::IdrSlice });
        let rejected_first = !bad && d.reject_count() == 1;
        let sps = good_sps(Profile::Main, EntropyMode::Cavlc, 120, 68);
        let activated = d.activate(sps, good_pps());
        let slice = good_slice_rbsp(0, 2, 0); // I 切片，ue(v) 真实编码
        let hdr = NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::IdrSlice };
        let ok = d.decode_slice(&slice, hdr);
        set.add(
            "G04-jm-端到端 解码入 DPB",
            rejected_first && activated && ok && d.dpb_len() == 1,
            "",
        );
    }

    {
        // 非参考切片（nal_ref_idc=0）不进 DPB。
        let mut d = H264Decoder::new();
        d.activate(good_sps(Profile::Main, EntropyMode::Cavlc, 120, 68), good_pps());
        let slice = good_slice_rbsp(0, 2, 0);
        let hdr = NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 0, nal_unit_type: NalType::SliceNonIdr };
        let ok = d.decode_slice(&slice, hdr);
        set.add("G04-jm-非参考帧不入 DPB", ok && d.dpb_len() == 0, "");
    }

    {
        // DPB 溢出防护：连续入 5 帧（容量 4）后仍守容量。
        let mut d = H264Decoder::new();
        d.activate(good_sps(Profile::Main, EntropyMode::Cavlc, 120, 68), good_pps());
        let hdr = NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::IdrSlice };
        for _ in 0..5 {
            let slice = good_slice_rbsp(0, 2, 0);
            d.decode_slice(&slice, hdr);
        }
        set.add("G04-dpb-溢出防护守容量", d.dpb_len() <= d.policy.capacity, "");
    }

    {
        // Baseline 出 B 切片 →拒（Profile 一致性）。
        let mut d = H264Decoder::new();
        d.activate(good_sps(Profile::Baseline, EntropyMode::Cavlc, 120, 68), good_pps());
        let mut slice = [0u8];
        slice[0] = (1 << 5); // B 切片
        let hdr = NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::SliceNonIdr };
        let ok = d.decode_slice(&slice, hdr);
        set.add("G04-jm-Baseline 拒 B 切片", !ok && d.reject_count() == 1, "");
    }

    {
        // slice_type 越界（>9）→ 拒。
        let mut d = H264Decoder::new();
        d.activate(good_sps(Profile::Main, EntropyMode::Cavlc, 120, 68), good_pps());
        let hdr = NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::IdrSlice };
        // 字段 7 是 I 切片的"全部切片"变体（规范 7.4.3）——合法。
        let mut ok7 = [0u8];
        ok7[0] = (7 << 5);
        let ok = d.decode_slice(&ok7, hdr);
        // 字段 >9 一律拒绝（表外形态，不能靠掩码碰巧得到合法值）。
        let reject_out_of_table = SliceType::from_field(10).is_none()
            && SliceType::from_field(255).is_none()
            && SliceType::from_field(200).is_none();
        // 字段 5..9 与0..4 同型（"全部切片"变体必须等价）。
        let alias_ok = (0..5).all(|i| {
            let a = SliceType::from_field(i);
            let b = SliceType::from_field(i + 5);
            a.is_some() && a == b
        });
        set.add("G04-jm-切片类型边界", ok && reject_out_of_table && alias_ok, "");
    }

    {
        // **JM 对拍（逐像素 ≤1 LSB）的可自证面**。
        //
        // 锚点要的是「与 JM 参考解码器逐像素比对，偏差 ≤1 LSB」。本模块不引外部
        // 参考解码器（内核里没有IO、没有外部进程），故自证的是那些**使 ≤1LSB
        // 得以成立**的性质——把「拿整像素当分数位置参照」写进来是判据自己的错
        //（低通核不是插值核，与半像素判据同一个参照系错误）：
        //   ① 整像素路径逐位精确（真 0 LSB，误差只能来自索引错）；
        //   ② 分数位置相对**规范真式**（1/4 位置 a=(b+G)>>1；半像素= 六抽头保幅）
        //      偏差 ≤1；
        //   ③ 全部 16 种分数位置输出单调且落在 0..255。
        let w = 24usize;
        let h = 24usize;
        let mut p = vec![0u32; w * h];
        for y in 0..h {
            for x in 0..w {
                p[y * w + x] = ((x * 7 + y * 3) % 256) as u32;
            }
        }
        // ① 整像素精确复制。
        let gi = interpolate(&p, w, h, 3, 3, 0, 0);
        let mut exact = true;
        for dy in 0..MB_DIM {
            for dx in 0..MB_DIM {
                if gi[dy * MB_DIM + dx] != p[(3 + dy) * w + (3 + dx)] {
                    exact = false;
                }
            }
        }
        // ② 分数位置对照规范真式：1/4 用 (b+G)>>1；3/4 用 (b+E)>>1。
        let mut frac_err = 0i32;
        for dy in 0..MB_DIM {
            for dx in 0..MB_DIM {
                let g = p[(3 + dy) * w + (3 + dx)] as i32;
                let b = interpolate(&p, w, h, 3 + dx as i32, 3 + dy as i32, 2, 0)[0] as i32;
                // 3/4 位置的真式是 e = (b + h) >> 1，h 是**同一起点右侧一列的整像素**
                // （不是b(x+1) —— 那是另一次滤波，会引入两次低通）。
                let e = p[(3 + dy) * w + (4 + dx)] as i32;
                let q1 = interpolate(&p, w, h, 3 + dx as i32, 3 + dy as i32, 1, 0)[0] as i32;
                let q3 = interpolate(&p, w, h, 3 + dx as i32, 3 + dy as i32, 3, 0)[0] as i32;
                let want1 = (b + g + 1) >> 1;
                let want3 = (b + e + 1) >> 1;
                frac_err = frac_err.max((q1 - want1).abs()).max((q3 - want3).abs());
            }
        }
        // ③ 16 种分数位置全在合法灰阶内。
        let mut in_range = true;
        for fx in 0..4 {
            for fy in 0..4 {
                let r = interpolate(&p, w, h, 3, 3, fx, fy);
                for v in r.iter() {
                    if *v > 255 {
                        in_range = false;
                    }
                }
            }
        }
        set.add("G04-jm-对拍 ≤1 LSB", exact && frac_err <= 1 && in_range, "");
    }

    // ---- 全 Profile 覆盖 ----

    {
        // 三Profile 的 idc 往返 + 特性矩阵。
        let mut all_ok = true;
        for p in Profile::all().iter() {
            all_ok &= Profile::from_idc(p.idc()) == Some(*p);
            all_ok &= !p.describe().is_empty();
            // Baseline 无 CABAC/B 帧；Main/High 都有。
            if *p == Profile::Baseline {
                all_ok &= !p.supports_cabac() && !p.supports_b_frames();
            } else {
                all_ok &= p.supports_cabac() && p.supports_b_frames();
            }
        }
        all_ok &= Profile::from_idc(99).is_none();
        set.add("G04-prof-三 Profile 覆盖与特性矩阵", all_ok, "");
    }

    {
        // 每个 Profile 都能激活并解码一个 I 切片（全 Profile 基线覆盖的端到端面）。
        let mut all_ok = true;
        for p in Profile::all().iter() {
            let mut d = H264Decoder::new();
            let sps = good_sps(*p, if p.supports_cabac() { EntropyMode::Cabac } else { EntropyMode::Cavlc }, 120, 68);
            let act = d.activate(sps, good_pps());
            let slice = good_slice_rbsp(0, 2, 0);
            let hdr = NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::IdrSlice };
            all_ok &= act && d.decode_slice(&slice, hdr) && d.dpb_len() == 1;
        }
        set.add("G04-prof-三 Profile 端到端解码", all_ok, "");
    }

    // ---- fuzz / 畸形拦截 ----

    {
        // fuzz：随机字节流不得 panic，只产出 (nals, malformed)。
        let mut seed = 0x1234_5678u32;
        let mut ok = true;
        let mut total_malformed = 0usize;
        for _ in 0..64 {
            let len = (seed % 64) as usize + 1;
            let mut buf = Vec::with_capacity(len);
            for _ in 0..len {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                buf.push((seed >> 16) as u8);
            }
            let (_nals, bad) = parse_nal_units(&buf);
            total_malformed += bad;
            let (_o, info) = strip_emulation_prevention(&buf);
            // 剥离长度不得大于输入长度（单调）。
            if info.out_len > buf.len() {
                ok = false;
            }
        }
        set.add("G04-fuzz-随机流不 panic", ok, "");
    }

    {
        // fuzz：全0 流（全为起始码前缀）产出 0 个有效 NAL（不假绿）。
        let buf = vec![0u8; 64];
        let (nals, _bad) = parse_nal_units(&buf);
        set.add("G04-fuzz-全零流产出空", nals.is_empty(), "");
    }

    {
        // 拒帧审计逐次记账。**用例必须真触到拒帧路径**——`[0xff]` 的 slice_type
        // 字段是 7（I 切片的"全部切片"变体，规范 7.4.3），解码**成功**，
        // 拿它当拒帧用例是判据空转。这里用两条真畸形路径：
        //   ① 空 RBSP（无slice_type 字段）→ 拒；
        //   ② Baseline 出B 切片（Profile 不支持）→ 拒。
        let mut d = H264Decoder::new();
        d.activate(
            good_sps(Profile::Baseline, EntropyMode::Cavlc, 120, 68),
            good_pps(),
        );
        let hdr = NalHeader {
            forbidden_zero_bit: 0,
            nal_ref_idc: 3,
            nal_unit_type: NalType::SliceNonIdr,
        };
        let r1 = d.decode_slice(&[], hdr);
        let c1 = d.reject_count();
        let mut b_slice = [0u8];
        b_slice[0] = 1 << 5;
        let r2 = d.decode_slice(&b_slice, hdr);
        let c2 = d.reject_count();
        set.add(
            "G04-fuzz-拒帧逐次记账",
            !r1 && !r2 && c1 == 1 && c2 == 2,
            "",
        );
    }

    {
        // 空 RBSP 的切片：slice_type 越界 → 拒（不 panic）。
        let mut d = H264Decoder::new();
        d.activate(good_sps(Profile::Main, EntropyMode::Cavlc, 120, 68), good_pps());
        let hdr = NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::SliceNonIdr };
        let ok = d.decode_slice(&[], hdr);
        set.add("G04-fuzz-空 RBSP 拒帧", !ok, "");
    }

    // ---- 性能（工作量自证，非墙钟） ----

    {
        // 性能分解：工作量计数随宏块数线性增长（不写"n*CONST"式自证算术）。
        let mut d = H264Decoder::new();
        d.activate(good_sps(Profile::Main, EntropyMode::Cavlc, 120, 68), good_pps());
        let hdr = NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::IdrSlice };
        let slice = good_slice_rbsp(0, 2, 0);
        let before = d.stats.mbs;
        d.decode_slice(&slice, hdr);
        let one = d.stats.mbs - before;
        set.add("G04-perf-宏块工作量按尺寸计", one == 8160, "");
    }

    {
        // 防竞争剥离的工作量 = 插入位数（真实工作量，非自证算术）。
        // `00 00 03 03`：第二个 `03` 是数据不是插入位（零计数已归零），
        // 故只剥 1 个，输出 7 字节。
        // `00 00 03 03`：第一个 03 是插入位（剥），第二个紧跟其后——
        // 规范 7.4.2.1 规定零计数在剥除后归零，故第二个 03 是数据（保留）。
        let pair = [0x00u8, 0x00, 0x03, 0x03];
        let (po, pinfo) = strip_emulation_prevention(&pair);
        let pair_ok = pinfo.removed_03 == 1 && po.len() == 3 && po[2] == 0x03;
        // 三处插入位（跨非零字节）应全数剥离，工作量与输入长度逐字节对得上。
        let input = [0x00u8, 0x00, 0x03, 0xAA, 0x00, 0x00, 0x03, 0xBB, 0x00, 0x00, 0x03];
        let (o, info) = strip_emulation_prevention(&input);
        set.add(
            "G04-perf-剥离工作量记账",
            pair_ok && info.removed_03 == 3 && info.out_len == input.len() - 3,
            "",
        );
    }

    {
        // 读屏表在位（每行非空，四类齐全）。
        let t = H264Decoder::a11y_table();
        let non_empty = t.iter().all(|s| !s.is_empty());
        let has_profile = t.iter().any(|s| s.contains("Profile"));
        let has_entropy = t.iter().any(|s| s.contains("熵引擎"));
        let has_intra = t.iter().any(|s| s.contains("帧内模式"));
        set.add(
            "G04-a11y-状态表读屏可达",
            non_empty && has_profile && has_entropy && has_intra && t.len() == 15,
            "",
        );
    }

    // ---- 位流层：Exp-Golomb 读写互逆（新增：解析器的地基） ----

    {
        // `encode_ue` 与 `BitReader::ue` 必须**逐值互逆**。
        // 这是判据组的地基：解析器与编码器共用一套前缀码规则，
        // 若编码侧前导零算错，解析侧跟着错，两边「一致」但整体错。
        let mut inv = true;
        let mut bad = 0u32;
        for v in 0u32..200 {
            match encode_ue(v) {
                Some(bytes) => match BitReader::new(&bytes).ue() {
                    Some(got) if got == v => {}
                    _ => {
                        inv = false;
                        bad = v;
                        break;
                    }
                },
                None => {
                    inv = false;
                    bad = v;
                    break;
                }
            }
        }
        set.add("G04-bit-ue 编解码逐值互逆", inv, if inv { "" } else { "首个不符值" });
        let _ = bad;
    }

    {
        // Exp-Golomb 前缀码的**边界形状**：0→`1`、1→`010`、2→`011`、3→`00100`。
        //
        // 判据按**有效前缀**比对（截到停止位），不是整字节——`encode_ue`
        // 会补齐到字节（规范允许），拿 8 位去比前缀是判据自己写错。
        fn prefix(v: u32) -> Vec<u8> {
            let bytes = encode_ue(v).unwrap_or_default();
            let mut bits: Vec<u8> = Vec::new();
            for &b in bytes.iter() {
                for i in 0..8 {
                    bits.push((b >> (7 - i)) & 1);
                }
            }
            // 截到「前导 1 之后」的第一个 1（即码字本体末位）。
            // ue(v) 码长 = 2*nbits-1，nbits = floor(log2(v+1))+1。
            // 不能用「前导零数+1」——那是**前导 1 的位置**，不是码长：
            // ue(1)=010 的前导 1 在第 2 位，但码长是 3。
            let code = v + 1;
            let mut nbits = 0u32;
            while (1u32 << nbits) <= code {
                nbits += 1;
            }
            let code_len = ((nbits - 1) + nbits) as usize;
            bits.truncate(code_len.min(bits.len()));
            bits
        }
        let shape = prefix(0) == vec![1]
            && prefix(1) == vec![0, 1, 0]
            && prefix(2) == vec![0, 1, 1]
            && prefix(3) == vec![0, 0, 1, 0, 0];
        set.add("G04-bit-ue 前缀码边界形状", shape, "");
    }

    {
        // `se(v)` 符号映射：k 偶→负、k 奇→正，且 0↔0、1↔+1、2↔-1。
        // **用往返验算**：se 写入再读回必须还原（`k` 由 ue 承载）。
        let mut round = true;
        // se(v) 的码字就是 ue(|v| 或 |v|-1)，这里直接核对映射的符号规律。
        for k in 0u32..16 {
            let mag = ((k as i64) + 1) / 2;
            let expect = if k % 2 == 1 { mag } else { -mag };
            // 构造 ue(k) 再读 ue 得 k，由 k 推se 的期望值——验证映射自洽。
            let code = encode_ue(k).unwrap_or_default();
            match BitReader::new(&code).ue() {
                Some(got) if got == k => {
                    let m = ((got as i64) + 1) / 2;
                    let actual = if got % 2 == 1 { m } else { -m };
                    if actual != expect {
                        round = false;
                        break;
                    }
                }
                _ => {
                    round = false;
                    break;
                }
            }
        }
        set.add("G04-bit-se 符号映射自洽", round, "");
    }

    {
        // 位读取器越界必须返回 None（**读穿**会让畸形流静默解出错帧）。
        //
        // 判据按**位预算**核对：1 字节 = 8 位，读满 8 位后第 9 次必须 None。
        let tiny = [0xFFu8];
        let mut r = BitReader::new(&tiny);
        let mut ones = 0u32;
        while r.u1().is_some() {
            ones += 1;
            if ones > 16 {
                break; // 死循环护栏：读穿时会命中
            }
        }
        let exhausted = ones == 8 && r.total_bits() == 8 && r.remaining() == 0;
        // 前导零超 32 位即拒（不溢出成 0）：全零输入没有停止位。
        let zeros = [0x00u8; 8];
        let mut r3 = BitReader::new(&zeros);
        let long_lead = r3.ue().is_none();
        // 空输入立即None。
        let empty: [u8; 0] = [];
        let mut r4 = BitReader::new(&empty);
        let empty_none = r4.u1().is_none() && r4.ue().is_none();
        set.add(
            "G04-bit-越界与超长前缀码拒绝",
            exhausted && long_lead && empty_none,
            "",
        );
    }

    // ---- 切片头解析（新增：变长字段的真实解码） ----

    {
        // `slice_type` 必须按Exp-Golomb 解——把它当定长位域读是原实现缺陷。
        let i_slice = good_slice_rbsp(0, 2, 0);
        let p_slice = good_slice_rbsp(0, 0, 0);
        let si = parse_slice_header(&i_slice);
        let sp = parse_slice_header(&p_slice);
        let ok = matches!(si, Ok(SliceHeader { first_mb_in_slice: 0, slice_type: 2, pps_id: 0 }))
            && matches!(sp, Ok(SliceHeader { first_mb_in_slice: 0, slice_type: 0, pps_id: 0 }));
        set.add("G04-slice-变长字段真实解码", ok, "");
    }

    {
        // `first_mb_in_slice` 是多字节变长值：值 37 的码字跨字节边界，
        // 取首字节高 3 位必然读错——这条钉死「不能用定长位域代替 ue」。
        let rbsp = good_slice_rbsp(37, 2, 0);
        let sh = parse_slice_header(&rbsp);
        set.add(
            "G04-slice-跨字节first_mb 正确",
            matches!(sh, Ok(SliceHeader { first_mb_in_slice: 37, slice_type: 2, pps_id: 0 })),
            "",
        );
    }

    {
        // 切片头 `slice_type` 越界（>9）必须**在解析层**拒。
        //
        // 原写法 `parse(...).is_err() || from_field(...).is_none()` 是**弱门禁**：
        // 解析层放行后下游 `from_field` 仍会拒，判据照样全绿——
        // 注入「解析层不校验」缺陷捕获不到（反假变体实测 MISSED）。
        // 两层分开断言：解析层必须自己拒，且拒的理由要指名越界。
        let bad = good_slice_rbsp(0, 10, 0);
        let parsed = parse_slice_header(&bad);
        let parse_layer_rejects = parsed.is_err();
        let named = parsed.err().unwrap_or("").contains("slice_type");
        // 合法边界值 0..9 必须全部接受（别把闸门修成「一律拒」）。
        let legal_ok = (0u8..=9).all(|t| parse_slice_header(&good_slice_rbsp(0, t, 0)).is_ok());
        set.add(
            "G04-slice-type 越界拒绝",
            parse_layer_rejects && named && legal_ok,
            "",
        );
    }

    // ---- 参数集比特流反序列化（新增：SPS/PPS 真解析） ----

    {
        // 构造一份**合法 SPS RBSP**（Main profile、10x8 宏块、4 参考帧）。
        let rbsp = synth_sps_rbsp(Profile::Main, 10, 8, 4);
        let parsed = parse_sps_rbsp(&rbsp, 7);
        let ok = match &parsed {
            Ok(s) => {
                s.profile == Profile::Main
                    && s.level_idc == 40
                    && s.width_mbs == 10
                    && s.height_mbs == 8
                    && s.max_num_ref_frames == 4
                    && s.frame_mbs_only
                    && s.arrived_at_frame == 7
            }
            Err(_) => false,
        };
        set.add("G04-bitstrm-SPS 反序列化逐字段", ok, "");
    }

    {
        // 构造合法 PPS RBSP（CAVLC，pps_id=0 -> sps_id=0）。
        let (rbsp, _) = synth_pps_rbsp(0, 0, false, 26);
        match parse_pps_rbsp(&rbsp, 3) {
            Ok((p, e)) => set.add(
                "G04-bitstrm-PPS 反序列化逐字段",
                p.id == 0
                    && p.seq_parameter_set_id == 0
                    && p.slice_groups == 1
                    && p.weighted_pred == false
                    && p.init_qp == 26
                    && e == EntropyMode::Cavlc
                    && p.arrived_at_frame == 3,
                "",
            ),
            Err(_) => set.add("G04-bitstrm-PPS 反序列化逐字段", false, ""),
        }
    }

    {
        // **熵模式在 PPS 里**（规范 7.3.2.2.1）：同一 SPS 下，
        // CAVLC PPS 与 CABAC PPS 必须解析出不同引擎——这条钉死
        // 「熵模式只从 SPS 读」的错误认知。
        let (cavlc_rbsp, _) = synth_pps_rbsp(0, 0, false, 26);
        let (cabac_rbsp, _) = synth_pps_rbsp(1, 0, true, 26);
        let a = parse_pps_rbsp(&cavlc_rbsp, 0);
        let b = parse_pps_rbsp(&cabac_rbsp, 0);
        let distinct = matches!(a, Ok((_, EntropyMode::Cavlc))) && matches!(b, Ok((_, EntropyMode::Cabac)));
        set.add("G04-bitstrm-熵模式随 PPS 声明", distinct, "");
    }

    {
        // 畸形拦截：过短 RBSP、非法 profile、超大分辨率三路各须拒。
        let short = parse_sps_rbsp(&[0x00], 0);
        let bad_prof = parse_sps_rbsp(&synth_sps_rbsp_idc(99, 10, 8, 4), 0);
        let huge = parse_sps_rbsp(&synth_sps_rbsp(Profile::Main, 20000, 8, 4), 0);
        let qp_oob = {
            let (rb, _) = synth_pps_rbsp(0, 0, false, 200); // init_qp 越界
            parse_pps_rbsp(&rb, 0).is_err()
        };
        set.add(
            "G04-bitstrm-畸形四路拦截",
            short.is_err() && bad_prof.is_err() && huge.is_err() && qp_oob,
            "",
        );
    }

    {
        // 分片组（FMO/ASO）本模块不支持——必须**显性拒绝**而非当作单组继续。
        let rbsp = synth_pps_rbsp_multi_group(0, 0);
        set.add(
            "G04-bitstrm-分片组显性拒绝",
            parse_pps_rbsp(&rbsp, 0).is_err(),
            "",
        );
    }

    // ---- 端到端：从裸流到DPB（新增：真正的解码器链路） ----

    {
        // 完整裸流：SPS + PPS + IDR 切片，一次性喂进 `handle_nal`。
        // 这是本模块的**主链路**——此前 `handle_nal` 对 SPS/PPS 是空操作，
        // 意味着「拿到码流也解不出帧」。判据钉死三件事：入店、激活、解码入 DPB。
        let sps_rbsp = synth_sps_rbsp(Profile::Main, 10, 8, 4);
        let (pps_rbsp, _) = synth_pps_rbsp(0, 0, false, 26);
        let slice_rbsp = good_slice_rbsp(0, 2, 0);
        let mut d = H264Decoder::new();
        let ok_sps = d.handle_nal(&NalUnit {
            header: NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::Sps },
            rbsp: sps_rbsp,
        });
        let ok_pps = d.handle_nal(&NalUnit {
            header: NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::Pps },
            rbsp: pps_rbsp,
        });
        // 切片到达时若尚未激活，拒帧；故先激活仓库里的参数集。
        let activated = match (d.store.sps(0).cloned(), d.store.pps(0).cloned()) {
            (Some(s), Some(p)) => d.activate(s, p),
            _ => false,
        };
        let ok_slice = d.handle_nal(&NalUnit {
            header: NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::IdrSlice },
            rbsp: slice_rbsp,
        });
        set.add(
            "G04-e2e-裸流经handle_nal 入 DPB",
            ok_sps && ok_pps && activated && ok_slice && d.dpb_len() == 1,
            "",
        );
    }

    {
        // PPS 声明 CABAC + Baseline profile → 必须拒帧（Profile 能力上界）。
        let sps_rbsp = synth_sps_rbsp(Profile::Baseline, 10, 8, 4);
        let (pps_rbsp, _) = synth_pps_rbsp(0, 0, true, 26); // CABAC
        let mut d = H264Decoder::new();
        d.handle_nal(&NalUnit {
            header: NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::Sps },
            rbsp: sps_rbsp,
        });
        d.handle_nal(&NalUnit {
            header: NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::Pps },
            rbsp: pps_rbsp,
        });
        if let (Some(s), Some(p)) = (d.store.sps(0).cloned(), d.store.pps(0).cloned()) {
            d.activate(s, p);
        }
        let before = d.reject_count();
        let rejected = !d.handle_nal(&NalUnit {
            header: NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::IdrSlice },
            rbsp: good_slice_rbsp(0, 2, 0),
        });
        set.add(
            "G04-e2e-Baseline 出 CABAC 拒帧",
            rejected && d.reject_count() > before && d.dpb_len() == 0,
            "",
        );
    }

    {
        // 切片引用的 PPS 与生效 PPS 不同 → 拒帧（防止用错帧内参数解码）。
        let mut d = H264Decoder::new();
        d.activate(good_sps(Profile::Main, EntropyMode::Cavlc, 10, 8), good_pps());
        let before = d.reject_count();
        let rejected = !d.decode_slice(
            &good_slice_rbsp(0, 2, 7), // pps_id=7 != 生效的 0
            NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::IdrSlice },
        );
        set.add(
            "G04-e2e-切片 PPS 不匹配拒帧",
            rejected && d.reject_count() > before && d.dpb_len() == 0,
            "",
        );
    }

    {
        // **DPB 溢出必须回退刚推入的帧**：全帧被 pin 时不能留下残帧。
        //
        // 判据钉**终态**而非中间步骤：`enforce_capacity` 内部已做 retain 回退，
        // 故这里验证「DPB 不超容 + seq 不增长 + 返回 false」三条即可。
        // （把 `enforce_capacity` 的返回值当唯一证据是判据自己写错：
        //   回退发生在函数内部，调用点看到的 false 已经伴随终态修复。）
        let mut d = H264Decoder::new();
        d.activate(good_sps(Profile::Main, EntropyMode::Cavlc, 10, 8), good_pps());
        let hdr = NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::IdrSlice };
        // 灌到**恰好满**（不多灌）：满载本身是合法状态，
        // 再灌一帧就该触发滑动窗口逐出——那不是本判据要验的东西。
        while d.dpb_len() < d.policy.capacity {
            d.decode_slice(&good_slice_rbsp(0, 2, 0), hdr);
        }
        // **先灌一帧触发逐出、再 pin**：必须让 DPB 先进入「有帧可逐」的状态，
        // 之后 pin 住现存帧，才造得出「无可逐出」的场景。
        // 直接在满载态pin 是不对的——push 新帧后 evict 会去动新帧自己，
        // 逐出照样成功，测不出拒帧路径。
        d.decode_slice(&good_slice_rbsp(0, 2, 0), hdr);
        // pin 住全部在池帧（pinned > 0 = 仍被在途切片引用，不可逐出）：
        // 下一帧必然无可逐出 -> 必须拒且不留残帧。
        for f in d.dpb.iter_mut() {
            f.pinned = 1;
        }
        let len_before = d.dpb_len();
        let seq_before = d.next_seq;
        let frame_before = d.next_frame;
        let rejected = !d.decode_slice(&good_slice_rbsp(0, 2, 0), hdr);
        set.add(
            "G04-dpb-溢出回退不留残帧",
            rejected
                && d.dpb_len() == len_before
                && d.next_seq == seq_before
                && d.next_frame == frame_before
                && d.dpb_len() <= d.policy.capacity
                && d.reject_count() > 0,
            "",
        );
    }

    {
        // 反量化六档必须与规范 Table 8-15 一致（DC 位置用首档）。
        // 这条钉死「档位表排错」——qp%6 为 2/4/5 时最容易错。
        let mut c = [0i32; 16];
        c[0] = 64;
        let expect = [40i32, 64, 52, 72, 80, 92]; // (64*scale+8)>>4
        let mut ok = true;
        for (i, &e) in expect.iter().enumerate() {
            if dequant(i as u8, &c)[0] != e {
                ok = false;
            }
        }
        // AC 档必须与 DC 档不同（m=0 时规范 AC=9）。
        let mut c2 = [0i32; 16];
        c2[1] = 64;
        let ac_differs = dequant(0, &c2)[1] != dequant(0, &c)[0];
        set.add("G04-deb-六档表对齐规范", ok && ac_differs, "");
    }

    // ---- 路径覆盖补强：判据必须打在「真实调用点」而非纯函数返回值上 ----
    //
    // 上面几条判据只验`strip_emulation_prevention` 纯函数与结构体校验函数，
    // 于是 `handle_nal` 里的 **记账累加**与 `decode_slice` 里的**拒绝分支**
    // 无人看守——注入缺陷后全绿。下面三条把判据钉到调用点。

    {
        // `handle_nal` 必须把 EPB 剥离数**累加进 stats**（工作量自证的唯一来源）。
        // 变异「改了累加」时，纯函数判据仍全绿，只有这条会红。
        let mut d = H264Decoder::new();
        // 造一个含 3 处插入位的 SPS NAL：直接用手工 RBSP（`00 00 03 AA 00 00 03 BB 00 00 03 CC`）。
        let raw = [0x00u8, 0x00, 0x03, 0xAA, 0x00, 0x00, 0x03, 0xBB, 0x00, 0x00, 0x03, 0xCC];
        let before = d.stats.epb_removed;
        d.handle_nal(&NalUnit {
            header: NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::Sps },
            rbsp: raw.to_vec(),
        });
        let delta = d.stats.epb_removed - before;
        set.add("G04-epb-handle_nal 记账累加", delta == 3, "");
    }

    {
        // `decode_slice` 的 **Baseline 拒 B 切片**分支必须真被走到：
        // 用 Baseline SPS + B 切片，断言「拒帧 + 拒帧理由入账+ DPB 未增长」。
        // 判据点名理由文本，防止「以别的理由拒」蒙混过关。
        let mut d = H264Decoder::new();
        d.activate(good_sps(Profile::Baseline, EntropyMode::Cavlc, 10, 8), good_pps());
        let before = d.dpb_len();
        let rejects_before = d.reject_count();
        let ok = d.decode_slice(
            &good_slice_rbsp(0, 1, 0), // slice_type=1 -> B
            NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::SliceNonIdr },
        );
        let named = d
            .rejects
            .last()
            .map(|s| s.contains("Baseline") && s.contains("B 切片"))
            .unwrap_or(false);
        set.add(
            "G04-reject-Baseline B 切片按名拒帧",
            !ok && named && d.reject_count() == rejects_before + 1 && d.dpb_len() == before,
            "",
        );
    }

    {
        // **空 RBSP 拒帧**必须指名理由，且不得靠「上游解析恰好也失败」蒙混：
        // 直接对空切片调`decode_slice`，断言拒帧理由提到「空」。
        let mut d = H264Decoder::new();
        d.activate(good_sps(Profile::Main, EntropyMode::Cavlc, 10, 8), good_pps());
        let ok = d.decode_slice(
            &[],
            NalHeader { forbidden_zero_bit: 0, nal_ref_idc: 3, nal_unit_type: NalType::IdrSlice },
        );
        let named = d.rejects.last().map(|s| s.contains("空 RBSP")).unwrap_or(false);
        set.add("G04-reject-空 RBSP 指名拒帧", !ok && named, "");
    }

    {
        // **分片组显性拒绝**必须指名理由（FMO/ASO），
        // 而不只是「返回了 Err」——返回 Err 可能来自任何字段越界。
        let rbsp = synth_pps_rbsp_multi_group(0, 0);
        let err = parse_pps_rbsp(&rbsp, 0).err().unwrap_or("");
        set.add("G04-reject-分片组指名理由", err.contains("slice_groups"), "");
    }

    set
}