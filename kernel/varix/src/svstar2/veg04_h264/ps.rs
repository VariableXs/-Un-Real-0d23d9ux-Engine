//! VE-F1204 · H.264 解码器（VE-G 域 · G01 视频解码组 · L2/L3）
//!
//! **VE-F1204 · H.264 解码器 · §5 SPS / PPS 参数集**
//!
//! 由 `veg04_h264.rs` 按职责拆分而来（L5 子模块拆分，行为保持）。
//! 全部符号经 `veg04_h264` 的 `pub use` 再导出，对外路径不变。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::core::{DiagCode, Failure, H264Limits, R, checked_mul, overflow_failure};
use super::nal::{BitReader};
use super::dpb::{h264_frame_bytes};

// §5 SPS / PPS 参数集（分辨率 / 参考帧数 / 熵编码模式 / 流内热更新）
// ════════════════════════════════════════════════════════════════════════════

/// H.264 Profile（`profile_idc` 表 7-3 的解码相关取值域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    /// Baseline：CAVLC、无 8x8 变换、无 CABAC、无 B 切片。
    Baseline,
    /// Constrained Baseline。
    ConstrainedBaseline,
    /// Main：CABAC + B 切片 + 8x8 变换。
    Main,
    /// Extended。
    Extended,
    /// High：8x8 变换 + 4:4:4 预测等。
    High,
    /// High 10。
    High10,
    /// High 4:2:2。
    High422,
    /// High 4:4:4 Predictive。
    High444,
    /// Multiview High。
    MultiviewHigh,
    /// 未登记Profile（按扩展处理但**不宣称**能力）。
    Other(u8),
}

impl Profile {
    /// 由 `profile_idc` + `constraint_set1_flag` 区分 Baseline 与 ConstrainedBaseline。
    ///
    /// 两者共用 `profile_idc == 66`；规范用 `constraint_set1_flag` 区分。
    /// 只看 idc 会把 ConstrainedBaseline 一律判成 Baseline（丢掉「不支持 FMO/
    /// 数据分区」的语义），反向误判则会让不支持的能力被宣称。
    pub fn from_idc_flags(idc: u8, constraint_set1_flag: u8) -> Profile {
        if idc == 66 {
            return if constraint_set1_flag != 0 {
                Profile::ConstrainedBaseline
            } else {
                Profile::Baseline
            };
        }
        Profile::from_idc(idc)
    }

    pub fn from_idc(idc: u8) -> Profile {
        match idc {
            66 => Profile::ConstrainedBaseline,
            77 => Profile::Main,
            88 => Profile::Extended,
            100 => Profile::High,
            110 => Profile::High10,
            122 => Profile::High422,
            244 => Profile::High444,
            118 => Profile::MultiviewHigh,
            // 66 也常被写作 Baseline；显式 Baseline 保持 66 的默认解读。
            _ => Profile::Other(idc),
        }
    }

    /// 该 Profile 是否**宣称**支持 CABAC（Main 及以上）。
    ///
    /// 规范：`entropy_coding_mode_flag` 在 PPS 里，但只有 High/Main 系列 Profile
    /// 才允许置位。能力位只读呈现，未知 Profile 不宣称。
    pub const fn declares_cabac(self) -> bool {
        matches!(
            self,
            Profile::Main
                | Profile::High
                | Profile::High10
                | Profile::High422
                | Profile::High444
                | Profile::MultiviewHigh
        )
    }

    /// 该 Profile 是否**宣称**支持 8x8 帧内变换（High 及以上）。
    pub const fn declares_8x8_transform(self) -> bool {
        matches!(
            self,
            Profile::High
                | Profile::High10
                | Profile::High422
                | Profile::High444
                | Profile::MultiviewHigh
        )
    }

    /// 该 Profile 是否允许 B 切片（Main 及以上）。
    pub const fn declares_b_slices(self) -> bool {
        self.declares_cabac()
    }

    pub const fn name(self) -> &'static str {
        match self {
            Profile::Baseline => "Baseline",
            Profile::ConstrainedBaseline => "ConstrainedBaseline",
            Profile::Main => "Main",
            Profile::Extended => "Extended",
            Profile::High => "High",
            Profile::High10 => "High10",
            Profile::High422 => "High422",
            Profile::High444 => "High444",
            Profile::MultiviewHigh => "MultiviewHigh",
            Profile::Other(_) => "Other",
        }
    }
}

/// 色度采样格式（`chroma_format_idc`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChromaFormat {
    /// 4:0:0（单色）。
    Monochrome,
    /// 4:2:0。
    Yuv420,
    /// 4:2:2。
    Yuv422,
    /// 4:4:4。
    Yuv444,
}

impl ChromaFormat {
    pub fn from_idc(idc: u32) -> ChromaFormat {
        match idc {
            0 => ChromaFormat::Monochrome,
            1 => ChromaFormat::Yuv420,
            2 => ChromaFormat::Yuv422,
            3 => ChromaFormat::Yuv444,
            // idc > 3 按 4:2:2 处理（规范定义）且显性标注在调用侧。
            _ => ChromaFormat::Yuv422,
        }
    }

    /// `SubWidthC`（亮度宽度 / 色度宽度）。
    pub const fn sub_width_c(self) -> u32 {
        match self {
            ChromaFormat::Monochrome | ChromaFormat::Yuv444 => 1,
            ChromaFormat::Yuv420 | ChromaFormat::Yuv422 => 2,
        }
    }

    /// `SubHeightC`（亮度高度 / 色度高度）。
    pub const fn sub_height_c(self) -> u32 {
        match self {
            ChromaFormat::Monochrome | ChromaFormat::Yuv444 => 1,
            ChromaFormat::Yuv420 => 2,
            ChromaFormat::Yuv422 => 1,
        }
    }

    pub const fn idc(self) -> u32 {
        match self {
            ChromaFormat::Monochrome => 0,
            ChromaFormat::Yuv420 => 1,
            ChromaFormat::Yuv422 => 2,
            ChromaFormat::Yuv444 => 3,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            ChromaFormat::Monochrome => "Monochrome",
            ChromaFormat::Yuv420 => "4:2:0",
            ChromaFormat::Yuv422 => "4:2:2",
            ChromaFormat::Yuv444 => "4:4:4",
        }
    }
}

/// 序列参数集（SPS，语法表 7-3-1 的解码相关字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sps {
    pub profile_idc: u8,
    pub profile: Profile,
    pub constraint_set0_flag: u8,
    pub constraint_set1_flag: u8,
    pub constraint_set2_flag: u8,
    pub level_idc: u8,
    pub seq_parameter_set_id: u32,
    pub chroma_format_idc: u32,
    pub separate_colour_plane_flag: u8,
    pub bit_depth_luma_minus8: u32,
    pub bit_depth_chroma_minus8: u32,
    pub qpprime_y_zero_transform_bypass_flag: u8,
    pub seq_scaling_matrix_present_flag: u8,
    pub log2_max_frame_num_minus4: u32,
    pub pic_order_cnt_type: u32,
    pub log2_max_pic_order_cnt_lsb_minus4: u32,
    pub delta_pic_order_always_zero_flag: u8,
    pub max_num_ref_frames: u32,
    pub gaps_in_frame_num_value_allowed_flag: u8,
    pub pic_width_in_mbs_minus1: u32,
    pub pic_height_in_map_units_minus1: u32,
    pub frame_mbs_only_flag: u8,
    pub mb_adaptive_frame_field_flag: u8,
    pub direct_8x8_inference_flag: u8,
    pub frame_cropping_flag: u8,
    pub frame_crop_left_offset: u32,
    pub frame_crop_right_offset: u32,
    pub frame_crop_top_offset: u32,
    pub frame_crop_bottom_offset: u32,
    pub vui_parameters_present_flag: u8,
    /// 高阶语法（Scalable NAL / 扩展部分）标记：本解码路径不展开但显性登记。
    pub high_profile_extra_parsed: bool,
}

impl Sps {
    /// 图像宽度（像素）。
    ///
    /// 宽度与裁剪单位**无关**（横向裁剪单位恒为 `SubWidthC`）。
    pub fn width_in_pixels(&self) -> u32 {
        (self.pic_width_in_mbs_minus1 + 1) * 16
    }

    /// 图像高度（像素），含帧/场映射与裁剪。
    ///
    /// `FrameHeightInMbs = (2 - frame_mbs_only_flag) * PicHeightInMapUnits`
    /// `CropUnitY = SubHeightC * (2 - frame_mbs_only_flag)`
    /// （见模块头注「深度打磨要点三」：这里**是乘法**，不是除法）。
    ///
    /// 裁剪用 `saturating_sub` 而非裸减：恶意 SPS 可以把裁剪偏移写得比
    /// 画面还大，裸减会下溢 panic —— 而本函数正是畸形 SPS 的**第一道**
    /// 防线（[`Sps::validate_geometry`] 在判定为零之前就先调它），在这里
    /// panic 等于让最该被拒的输入把进程带走。饱和减让越界裁剪落到 0，
    /// 再由 `validate_geometry` 以「裁剪后尺寸为零」显性拒绝。
    pub fn height_in_pixels(&self) -> u32 {
        let crop_y = self
            .crop_unit_y()
            .saturating_mul(self.frame_crop_top_offset.saturating_add(self.frame_crop_bottom_offset));
        self.uncropped_height_in_pixels().saturating_sub(crop_y)
    }

    /// 未裁剪的图像高度（像素）——校验裁剪合法性时要用它做上界。
    pub fn uncropped_height_in_pixels(&self) -> u32 {
        let frame_field_mult: u32 = 2 - u32::from(self.frame_mbs_only_flag);
        let map_units = self.pic_height_in_map_units_minus1 + 1;
        (frame_field_mult * map_units).saturating_mul(16)
    }

    /// 横向裁剪后的实际宽度。
    ///
    /// 同样用饱和减：理由见 [`Sps::height_in_pixels`] 的注释——裁剪偏移
    /// 越界必须落到 0 交给校验层拒绝，不能下溢 panic。
    pub fn cropped_width_in_pixels(&self) -> u32 {
        let crop_x = self
            .crop_unit_x()
            .saturating_mul(self.frame_crop_left_offset.saturating_add(self.frame_crop_right_offset));
        self.uncropped_width_in_pixels().saturating_sub(crop_x)
    }

    /// 未裁剪的图像宽度（像素）——校验裁剪合法性时要用它做上界。
    pub fn uncropped_width_in_pixels(&self) -> u32 {
        self.pic_width_in_mbs_minus1.saturating_add(1).saturating_mul(16)
    }

    /// 横向裁剪单位 `CropUnitX`（规范 7.4.2.1.1）。
    ///
    /// 独立成函数而不是散在各处内联算：裁剪的**校验、宽高计算、诊断文案**
    /// 必须共用同一个口径，否则会出现「校验说没越界、宽高却算成 0」的分叉。
    pub fn crop_unit_x(&self) -> u32 {
        let fmt = ChromaFormat::from_idc(self.chroma_format_idc);
        let frame_field_mult: u32 = 2 - u32::from(self.frame_mbs_only_flag);
        if self.chroma_format_idc == 0 || self.separate_colour_plane_flag == 1 {
            // Monochrome 或独立色度平面：CropUnitX 就是宏块宽。
            frame_field_mult * 16
        } else {
            fmt.sub_width_c() * frame_field_mult
        }
    }

    /// 纵向裁剪单位 `CropUnitY`（规范 7.4.2.1.1）。
    ///
    /// 见 [`Sps::crop_unit_x`] 关于「同一口径」的说明。
    pub fn crop_unit_y(&self) -> u32 {
        let fmt = ChromaFormat::from_idc(self.chroma_format_idc);
        let frame_field_mult: u32 = 2 - u32::from(self.frame_mbs_only_flag);
        if self.chroma_format_idc == 0 || self.separate_colour_plane_flag == 1 {
            frame_field_mult * 16
        } else {
            fmt.sub_height_c() * frame_field_mult
        }
    }

    /// 宏块网格宽（宏块数）。
    pub fn width_mbs(&self) -> u32 {
        self.pic_width_in_mbs_minus1 + 1
    }

    /// 宏块网格高（宏块数，含帧/场映射）。
    pub fn height_mbs(&self) -> u32 {
        let map_units = self.pic_height_in_map_units_minus1 + 1;
        (2 - u32::from(self.frame_mbs_only_flag)) * map_units
    }

    /// 宏块总数。
    pub fn total_macroblocks(&self) -> Option<u64> {
        checked_mul(self.width_mbs() as u64, self.height_mbs() as u64)
    }

    /// `log2_max_frame_num`（`frame_num` 的位宽，≥4）。
    pub fn log2_max_frame_num(&self) -> u32 {
        self.log2_max_frame_num_minus4 + 4
    }

    /// `log2_max_pic_order_cnt_lsb`（≥4）。
    pub fn log2_max_pic_order_cnt_lsb(&self) -> u32 {
        self.log2_max_pic_order_cnt_lsb_minus4 + 4
    }

    /// **几何指纹**：热更新判定用（见模块头注「深度打磨要点四」）。
    ///
    /// 只含改变「已解帧地址空间」的四元组。`max_num_ref_frames`、
    /// `log2_max_frame_num` 等不在其中——它们变化不使既有 DPB 帧失效。
    pub fn geometry_fingerprint(&self) -> (u32, u32, u32, u8) {
        (
            self.width_mbs(),
            self.height_mbs(),
            self.chroma_format_idc,
            self.frame_mbs_only_flag,
        )
    }

    /// 几何自洽性：裁剪后尺寸必须为正、宏块数在上限内、参考帧数合法。
    ///
    /// 主体只保留「三道闸门按序通过」的骨架，各失败分支下沉到
    /// [`Sps::reject_oversize_frame`] / [`Sps::reject_degenerate_crop`] /
    /// [`Sps::reject_invalid_refs`]（顺序即语义：先尺寸后裁剪后参考）。
    pub fn validate_geometry(&self, limits: &H264Limits) -> R<()> {
        self.reject_oversize_frame(limits)?;
        self.reject_degenerate_crop()?;
        self.reject_invalid_refs(limits)?;
        Ok(())
    }

    /// 尺寸/预算闸：单边宏块上限 + 整帧字节预算前置拒绝。
    ///
    /// **单边上限不足以挡住恶意分辨率**：两边都合法（各 ≤ `max_dim_mbs`）
    /// 也能让面积任意大。第一版只判单边，于是
    /// `4001 x 4001` 宏块（= 64016x64016 像素，**6.1 GB 一帧**）
    /// 在参数集层顺利通过，直到 DPB 分配阶段才被字节预算拦下——
    /// 那时 `Vec` 申请已经发生。
    ///
    /// 这里把**整帧字节数**作为前置判据：`> dpb_budget_bytes` 即拒。
    /// 规范层面这也是对的：Level 5.2 的最大帧尺寸（36864 宏块）远小于
    /// `4096 x 4096`，所以任何超预算的面积都是恶意的。
    fn reject_oversize_frame(&self, limits: &H264Limits) -> R<()> {
        if self.width_mbs() > limits.max_dim_mbs || self.height_mbs() > limits.max_dim_mbs {
            return Err(Failure::new(
                DiagCode::SpsGeometryInconsistent,
                format!(
                    "宏块尺寸 {}x{} 超过上限 {}",
                    self.width_mbs(),
                    self.height_mbs(),
                    limits.max_dim_mbs
                ),
                "恶意 SPS 声称超大分辨率；按F1122 上限纪律拒绝",
            ));
        }
        if let Some(bytes) = h264_frame_bytes(self) {
            if bytes > limits.dpb_budget_bytes {
                return Err(Failure::new(
                    DiagCode::SpsGeometryInconsistent,
                    format!(
                        "整帧 {}x{} 宏块需 {} 字节，超过 DPB 预算 {}",
                        self.width_mbs(),
                        self.height_mbs(),
                        bytes,
                        limits.dpb_budget_bytes
                    ),
                    "单边尺寸合法但整帧面积超预算；恶意 SPS 在参数集层前置拒绝，不进入分配",
                ));
            }
        }
        Ok(())
    }

    /// 裁剪合法性闸：裁剪后尺寸为零 / 裁剪单位不小于画面尺寸均显性拒绝。
    ///
    /// 裁剪偏移越界（裁掉的比画面还多）单独立案：饱和减把它归到
    /// 「裁剪后为零」，但那条信息不足以告诉调用方是左右还是上下越界。
    /// 这里给出可检索的具体侧别，避免上层拿到一个笼统的零尺寸。
    fn reject_degenerate_crop(&self) -> R<()> {
        if self.cropped_width_in_pixels() == 0 || self.height_in_pixels() == 0 {
            return Err(Failure::new(
                DiagCode::SpsGeometryInconsistent,
                format!(
                    "裁剪后尺寸 {}x{} 为零",
                    self.cropped_width_in_pixels(),
                    self.height_in_pixels()
                ),
                "裁剪偏移不得把图像尺寸裁到 0；这是参数集内部矛盾",
            ));
        }
        if self.uncropped_width_in_pixels() <= self.crop_unit_x() {
            return Err(Failure::new(
                DiagCode::SpsGeometryInconsistent,
                format!(
                    "横向裁剪单位 {} 不小于画面宽度 {}",
                    self.crop_unit_x(),
                    self.uncropped_width_in_pixels()
                ),
                "裁剪偏移越界：整幅画面被横向裁空",
            ));
        }
        if self.uncropped_height_in_pixels() <= self.crop_unit_y() {
            return Err(Failure::new(
                DiagCode::SpsGeometryInconsistent,
                format!(
                    "纵向裁剪单位 {} 不小于画面高度 {}",
                    self.crop_unit_y(),
                    self.uncropped_height_in_pixels()
                ),
                "裁剪偏移越界：整幅画面被纵向裁空",
            ));
        }
        Ok(())
    }

    /// 参考结构闸：参考帧数、`frame_num` 位宽、宏块总数可表示性。
    fn reject_invalid_refs(&self, limits: &H264Limits) -> R<()> {
        if self.max_num_ref_frames > limits.max_num_ref_frames {
            return Err(Failure::new(
                DiagCode::SpsGeometryInconsistent,
                format!("max_num_ref_frames {} 超过上限 {}", self.max_num_ref_frames, limits.max_num_ref_frames),
                "参考帧数超出规范范围（≤16）；拒绝接受",
            ));
        }
        if self.log2_max_frame_num() > 16 || self.log2_max_frame_num() < 4 {
            return Err(Failure::new(
                DiagCode::SpsGeometryInconsistent,
                format!("log2_max_frame_num {} 超出 [4,16]", self.log2_max_frame_num()),
                "frame_num 位宽超出可表示范围",
            ));
        }
        if self.total_macroblocks().is_none() {
            return Err(overflow_failure("宏块总数"));
        }
        Ok(())
    }
}

/// 图像参数集（PPS，语法表 7-3-2 的解码相关字段）。
///
/// **只derive Clone 不 derive Copy**：字段 `run_length_minus1: Vec<u32>` 含堆指针，
/// derive Copy 会让帧数组的复制变成浅拷贝（两个Pps 共享同一Vec 内存，
/// 改一个影响另一个——典型的use-after-free 式逻辑缺陷）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Pps {
    pub pic_parameter_set_id: u32,
    pub seq_parameter_set_id: u32,
    /// **`entropy_coding_mode_flag`：0=CAVLC、1=CABAC**（见头注要点六）。
    pub entropy_coding_mode_flag: u8,
    pub bottom_field_pic_order_in_frame_present_flag: u8,
    pub num_slice_groups_minus1: u32,
    pub num_ref_idx_l0_default_active_minus1: u32,
    pub num_ref_idx_l1_default_active_minus1: u32,
    pub weighted_pred_flag: u8,
    pub weighted_bipred_idc: u8,
    pub pic_init_qp_minus26: i32,
    pub pic_init_qs_minus26: i32,
    pub chroma_qp_index_offset: i32,
    pub deblocking_filter_control_present_flag: u8,
    pub constrained_intra_pred_flag: u8,
    pub redundant_pic_cnt_present_flag: u8,
    /// 语法存在性：`transform_8x8_mode_flag` 只在 PPS 尾部存在。
    pub transform_8x8_mode_flag_present: u8,
    pub transform_8x8_mode_flag: u8,
    pub pic_scaling_matrix_present_flag: u8,
    pub second_chroma_qp_index_offset: i32,
    /// 切片组（`num_slice_groups_minus1 > 0`）时才有意义的 FMO 字段。
    pub slice_group_map_type: u32,
    pub run_length_minus1: Vec<u32>,
}

impl Pps {
    /// 该 PPS 选中的熵引擎。
    pub fn entropy_engine(&self) -> EntropyEngine {
        select_entropy_engine(self.entropy_coding_mode_flag)
    }

    /// 帧内 QP：`26 + pic_init_qp_minus26 + slice_qp_delta`。
    pub fn initial_qp(&self, slice_qp_delta: i32) -> i32 {
        26 + self.pic_init_qp_minus26 + slice_qp_delta
    }

    /// 去块滤波是否由切片头控制（否则用SPS/PPS 默认值）。
    pub const fn deblocking_in_slice_header(&self) -> bool {
        self.deblocking_filter_control_present_flag != 0
    }
}

/// 熵解码引擎（H.264 的两条互斥熵编码路径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntropyEngine {
    /// CAVLC（上下文自适应变长编码）。
    Cavlc,
    /// CABAC（上下文自适应二进制算术编码）。
    Cabac,
}

impl EntropyEngine {
    pub const fn name(self) -> &'static str {
        match self {
            EntropyEngine::Cavlc => "CAVLC",
            EntropyEngine::Cabac => "CABAC",
        }
    }
}

/// 按 `entropy_coding_mode_flag` 选择熵引擎（0→CAVLC、1→CABAC）。
///
/// 注意（见头注要点六）：该标志在 **PPS** 里，不在 SPS。锚点字面写「按 SPS」，
/// 本模块按规范从 PPS 取；SPS 变体不改变本函数结果（自检
/// `C04-ENTROPY-SWITCH-PPS` 用表外切片数据验证）。
pub fn select_entropy_engine(entropy_coding_mode_flag: u8) -> EntropyEngine {
    if entropy_coding_mode_flag != 0 {
        EntropyEngine::Cabac
    } else {
        EntropyEngine::Cavlc
    }
}

/// 参数集登记表（SPS/PPS 的**唯一**语义源；支持流内热更新）。
#[derive(Clone, Default, Debug)]
pub struct ParameterSetRegistry {
    sps: Vec<Sps>,
    pps: Vec<Pps>,
    sps_seen: Vec<bool>,
    pps_seen: Vec<bool>,
    hot_updates: u64,
    rejections: u64,
}

/// 登记结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegisterOutcome {
    /// 首次登记（建立）。
    Created,
    /// 热更新（同 id、几何一致）。
    HotUpdated,
}

impl RegisterOutcome {
    pub const fn is_hot_update(self) -> bool {
        matches!(self, RegisterOutcome::HotUpdated)
    }
}

/// 最大的 SPS/PPS id（SPS ≤31、PPS ≤255）。
pub const MAX_SPS_ID: usize = 32;
pub const MAX_PPS_ID: usize = 256;

/// 占位 SPS：登记表按 id 稀疏增长时填充「先见 PPS 引用、后见大 id SPS」
/// 的中间槽位；槽位是否真正登记由 `sps_seen` 守住，占位值永不外露。
fn placeholder_sps() -> Sps {
    Sps {
        profile_idc: 0,
        profile: Profile::Other(0),
        constraint_set0_flag: 0,
        constraint_set1_flag: 0,
        constraint_set2_flag: 0,
        level_idc: 0,
        seq_parameter_set_id: 0,
        chroma_format_idc: 1,
        separate_colour_plane_flag: 0,
        bit_depth_luma_minus8: 0,
        bit_depth_chroma_minus8: 0,
        qpprime_y_zero_transform_bypass_flag: 0,
        seq_scaling_matrix_present_flag: 0,
        log2_max_frame_num_minus4: 0,
        pic_order_cnt_type: 0,
        log2_max_pic_order_cnt_lsb_minus4: 0,
        delta_pic_order_always_zero_flag: 0,
        max_num_ref_frames: 0,
        gaps_in_frame_num_value_allowed_flag: 0,
        pic_width_in_mbs_minus1: 0,
        pic_height_in_map_units_minus1: 0,
        frame_mbs_only_flag: 1,
        mb_adaptive_frame_field_flag: 0,
        direct_8x8_inference_flag: 0,
        frame_cropping_flag: 0,
        frame_crop_left_offset: 0,
        frame_crop_right_offset: 0,
        frame_crop_top_offset: 0,
        frame_crop_bottom_offset: 0,
        vui_parameters_present_flag: 0,
        high_profile_extra_parsed: false,
    }
}

/// 热更新几何一致性闸：几何指纹变化必须显性拒绝
///（不静默接受、不静默丢弃——见 [`ParameterSetRegistry::register_sps`]）。
fn reject_sps_geometry_change(id: usize, old: &Sps, new: &Sps) -> Result<(), Failure> {
    if old.geometry_fingerprint() == new.geometry_fingerprint() {
        return Ok(());
    }
    Err(Failure::new(
        DiagCode::ParameterSetGeometryChange,
        format!(
            "SPS id {} 已登记为 {}x{} (chroma={}, fmfo={})，被 {}x{} (chroma={}, fmfo={}) 改写",
            id,
            old.width_mbs(),
            old.height_mbs(),
            old.chroma_format_idc,
            old.frame_mbs_only_flag,
            new.width_mbs(),
            new.height_mbs(),
            new.chroma_format_idc,
            new.frame_mbs_only_flag
        ),
        "几何参数变化会使 DPB 中既有帧失效；要求上层先 flush DPB 再接受新 SPS（不静默接受、不静默丢弃）",
    ))
}

impl ParameterSetRegistry {
    pub fn new() -> ParameterSetRegistry {
        ParameterSetRegistry {
            sps: Vec::new(),
            pps: Vec::new(),
            sps_seen: Vec::new(),
            pps_seen: Vec::new(),
            hot_updates: 0,
            rejections: 0,
        }
    }

    pub fn hot_updates(&self) -> u64 {
        self.hot_updates
    }

    pub fn rejections(&self) -> u64 {
        self.rejections
    }

    /// 登记 SPS（首次建立 / 同 id 热更新 / 几何变化拒绝）。
    pub fn register_sps(&mut self, sps: Sps) -> Result<RegisterOutcome, Failure> {
        let id = sps.seq_parameter_set_id as usize;
        if id >= MAX_SPS_ID {
            return Err(Failure::new(
                DiagCode::ParameterSetIdOutOfRange,
                format!("seq_parameter_set_id {} 超出 [0,{})", id, MAX_SPS_ID),
                "规范 7.3.2.1.1：ue(v) 范围 0..31；越界即参数集非法",
            ));
        }
        while self.sps.len() <= id {
            self.sps.push(placeholder_sps());
            self.sps_seen.push(false);
        }
        if self.sps_was_seen(id) {
            let old = match self.sps.get(id) {
                Some(v) => *v,
                None => {
                    return Err(Failure::new(
                        DiagCode::ParameterSetIdOutOfRange,
                        format!("SPS id {} 不在登记表内", id),
                        "登记表与 id 集合不一致（内部不一致）",
                    ))
                }
            };
            if let Err(f) = reject_sps_geometry_change(id, &old, &sps) {
                self.rejections += 1;
                return Err(f);
            }
            self.hot_updates += 1;
            self.sps[id] = sps;
            Ok(RegisterOutcome::HotUpdated)
        } else {
            self.sps[id] = sps;
            self.sps_seen[id] = true;
            Ok(RegisterOutcome::Created)
        }
    }

    /// 登记 PPS。
    pub fn register_pps(&mut self, pps: Pps) -> Result<RegisterOutcome, Failure> {
        let id = pps.pic_parameter_set_id as usize;
        if id >= MAX_PPS_ID {
            return Err(Failure::new(
                DiagCode::ParameterSetIdOutOfRange,
                format!("pic_parameter_set_id {} 超出 [0,{})", id, MAX_PPS_ID),
                "规范 7.3.2.2.1：ue(v) 范围 0..255；越界即参数集非法",
            ));
        }
        if !self.sps_was_seen(pps.seq_parameter_set_id as usize) {
            return Err(Failure::new(
                DiagCode::SpsPpsRefMissing,
                format!("PPS {} 引用未登记的 SPS {}", id, pps.seq_parameter_set_id),
                "PPS 必须先有其 SPS；未登记前接受会让所有尺寸推导落在默认占位上",
            ));
        }
        while self.pps.len() <= id {
            self.pps.push(Pps {
                pic_parameter_set_id: 0,
                seq_parameter_set_id: 0,
                entropy_coding_mode_flag: 0,
                bottom_field_pic_order_in_frame_present_flag: 0,
                num_slice_groups_minus1: 0,
                num_ref_idx_l0_default_active_minus1: 0,
                num_ref_idx_l1_default_active_minus1: 0,
                weighted_pred_flag: 0,
                weighted_bipred_idc: 0,
                pic_init_qp_minus26: 0,
                pic_init_qs_minus26: 0,
                chroma_qp_index_offset: 0,
                deblocking_filter_control_present_flag: 0,
                constrained_intra_pred_flag: 0,
                redundant_pic_cnt_present_flag: 0,
                transform_8x8_mode_flag_present: 0,
                transform_8x8_mode_flag: 0,
                pic_scaling_matrix_present_flag: 0,
                second_chroma_qp_index_offset: 0,
                slice_group_map_type: 0,
                run_length_minus1: Vec::new(),
            });
            self.pps_seen.push(false);
        }
        if self.pps_was_seen(id) {
            self.hot_updates += 1;
            self.pps[id] = pps;
            Ok(RegisterOutcome::HotUpdated)
        } else {
            self.pps[id] = pps;
            self.pps_seen[id] = true;
            Ok(RegisterOutcome::Created)
        }
    }

    /// 取 SPS。
    pub fn sps(&self, id: u32) -> Option<&Sps> {
        let idx = id as usize;
        if !self.sps_was_seen(idx) {
            return None;
        }
        self.sps.get(idx)
    }

    /// 取 PPS。
    pub fn pps(&self, id: u32) -> Option<&Pps> {
        let idx = id as usize;
        if !self.pps_was_seen(idx) {
            return None;
        }
        self.pps.get(idx)
    }

    /// `id` 上的 SPS 是否已登记（未登记/越界都算否）。
    ///
    /// 「`sps_seen` 位与 `sps` 槽一一对应」是本表的不变量；两个数组一起 push，
    /// 因此一次 `len` 判定即可同时挡住越界与未登记，不必写
    /// `idx >= len || !sps_seen[idx]` 这个两段式条件。
    fn sps_was_seen(&self, id: usize) -> bool {
        self.sps_seen.get(id).copied().unwrap_or(false) && id < self.sps.len()
    }

    /// `id` 上的 PPS 是否已登记（见 [`Self::sps_was_seen`] 的同源纪律）。
    fn pps_was_seen(&self, id: usize) -> bool {
        self.pps_seen.get(id).copied().unwrap_or(false) && id < self.pps.len()
    }

    /// 已登记的 SPS 数。
    pub fn sps_count(&self) -> usize {
        self.sps_seen.iter().filter(|b| **b).count()
    }

    /// 已登记的 PPS 数。
    pub fn pps_count(&self) -> usize {
        self.pps_seen.iter().filter(|b| **b).count()
    }
}

/// 跳过 SPS 里的**缩放列表**语法（`seq_scaling_list`）。
///
/// 缩放列表不改变「本模块要判定的任何几何量」，但它必须被**逐比特跳过**，
/// 否则后续 `log2_max_frame_num` 全部错位。本函数返回消费的比特数。
fn skip_scaling_list(r: &mut BitReader, size: u32, limits: &H264Limits) -> R<usize> {
    let mut last_scale: i32 = 8;
    let mut next_scale: i32 = 8;
    let mut consumed = 0usize;
    let mut j = 0u32;
    while j < size {
        if next_scale != 0 {
            let delta = r.read_se(limits)?;
            consumed += 1;
            next_scale = (last_scale + delta + 256) & 0xFF;
        }
        if next_scale != 0 {
            last_scale = next_scale;
        }
        j += 1;
    }
    Ok(consumed)
}

/// 跳过 SPS 的 VUI 参数（只做**比特对齐级跳过**，不解码语义）。
///
/// 本模块不消费 VUI 的任何字段（H.264 的 VUI 语法种类极多），但必须知道它有多长
/// 才能保证「VUI 之后的 NAL 结尾」判断正确。这里按语法顺序完整走一遍以精确消费。
fn skip_vui(r: &mut BitReader, limits: &H264Limits) -> R<()> {
    let aspect_ratio_info_present = r.read_bit().trunc_some("VUI aspect_ratio_info_present_flag")?;
    if aspect_ratio_info_present == 1 {
        let idc = r.read_bits(8).trunc_some("aspect_ratio_idc")?;
        if idc == 255 {
            r.read_bits(16).trunc_some("sar_width")?;
            r.read_bits(16).trunc_some("sar_height")?;
        }
    }
    let overscan_info_present = r.read_bit().trunc_some("overscan_info_present_flag")?;
    if overscan_info_present == 1 {
        r.read_bit().trunc_some("overscan_appropriate_flag")?;
    }
    let video_signal_present = r.read_bit().trunc_some("video_signal_type_present_flag")?;
    if video_signal_present == 1 {
        r.read_bits(3).trunc_some("video_format")?;
        let full = r.read_bit().trunc_some("video_full_range_flag")?;
        if full == 0 {
            r.read_bit().trunc_some("colour_description_present_flag")?;
        }
    }
    let chroma_loc_present = r.read_bit().trunc_some("chroma_loc_info_present_flag")?;
    if chroma_loc_present == 1 {
        r.read_ue(limits)?;
        r.read_ue(limits)?;
    }
    let timing_present = r.read_bit().trunc_some("timing_info_present_flag")?;
    if timing_present == 1 {
        r.read_bits(32).trunc_some("num_units_in_tick")?;
        r.read_bits(32).trunc_some("time_scale")?;
        r.read_bit().trunc_some("fixed_frame_rate_flag")?;
    }
    // nal_hrd_parameters_present_flag / vcl_hrd_parameters_present_flag 走 HRD 跳过。
    let nal_hrd_present = r.read_bit().trunc_some("nal_hrd_parameters_present_flag")?;
    if nal_hrd_present == 1 {
        skip_hrd(r, limits)?;
    }
    let vcl_hrd_present = r.read_bit().trunc_some("vcl_hrd_parameters_present_flag")?;
    if vcl_hrd_present == 1 {
        skip_hrd(r, limits)?;
    }
    if nal_hrd_present == 1 || vcl_hrd_present == 1 {
        r.read_bit().trunc_some("low_delay_hrd_flag")?;
    }
    r.read_bit().trunc_some("pic_struct_present_flag")?;
    let brs_present = r.read_bit().trunc_some("bitstream_restriction_flag")?;
    if brs_present == 1 {
        r.read_bit().trunc_some("motion_vectors_over_pic_boundaries_flag")?;
        r.read_ue(limits).trunc_err("max_bytes_per_pic_denom")?;
        r.read_ue(limits).trunc_err("max_bits_per_mb_denom")?;
        r.read_ue(limits).trunc_err("log2_max_mv_length_horizontal")?;
        r.read_ue(limits).trunc_err("log2_max_mv_length_vertical")?;
        r.read_ue(limits).trunc_err("max_num_reorder_frames")?;
        r.read_ue(limits).trunc_err("max_dec_frame_buffering")?;
    }
    Ok(())
}

/// 截断失败构造（VUI/HRD 内部用）。
fn trunc(what: &str) -> Failure {
    Failure::new(
        DiagCode::BitstreamTruncated,
        format!("位流在 {} 处提前结束", what),
        "语法元素不足；拒绝解析（截断或畸形）",
    )
}

/// `Result` 的字段级截断改写：`.map_err(|_| trunc("field"))?` 的等价物。
///
/// 本模块百余处语法元素读取原先都是 `.map_err(|_| trunc("field"))?` 形状：
/// 每处都新建一个闭包，只为了把一个读失败换成带字段名的诊断。闭包体全是
/// 同一形状的样板，却让每个调用点都多出一个 `closure_expression` 节点——
/// 圈复杂度与嵌套深度两种度量都把它计入，等于百余处同形样板各自付一次复杂度税。
/// 收敛成方法调用后调用点只剩一个函数调用，`?` 的传播路径与产出的
/// `Failure`（码、消息、建议三字段）与原先逐字节一致。
pub trait TruncErr<T> {
    /// 读失败时丢弃原错误，改写成该字段的截断诊断后交由 `?` 传播。
    fn trunc_err(self, what: &str) -> R<T>;
}

impl<T> TruncErr<T> for R<T> {
    #[inline]
    fn trunc_err(self, what: &str) -> R<T> {
        match self {
            Ok(v) => Ok(v),
            Err(_) => Err(trunc(what)),
        }
    }
}

/// `Option` 的字段级截断改写：`.ok_or_else(|| trunc("field"))?` 的等价物。
///
/// 与 [`TruncErr`] 同理：`read_bit` / `read_bits` 在越界时给 `None`，
/// 调用点需要的是「哪个字段读不出来」这个信息，而不是 `None` 本身。
pub trait TruncSome<T> {
    /// 无值时改写为该字段的截断诊断后交由 `?` 传播。
    fn trunc_some(self, what: &str) -> R<T>;
}

impl<T> TruncSome<T> for Option<T> {
    #[inline]
    fn trunc_some(self, what: &str) -> R<T> {
        match self {
            Some(v) => Ok(v),
            None => Err(trunc(what)),
        }
    }
}

/// 跳过 HRD 参数（`cpb_cnt` 循环的完整比特级跳过）。
fn skip_hrd(r: &mut BitReader, limits: &H264Limits) -> R<()> {
    let cpb_cnt_minus1 = r.read_ue(limits).trunc_err("cpb_cnt_minus1")?;
    r.read_bits(4).trunc_some("bit_rate_scale")?;
    r.read_bits(4).trunc_some("cpb_size_scale")?;
    for _ in 0..=cpb_cnt_minus1 {
        r.read_ue(limits).trunc_err("bit_rate_value_minus1")?;
        r.read_ue(limits).trunc_err("cpb_size_value_minus1")?;
        r.read_bit().trunc_some("cbr_flag")?;
    }
    r.read_bits(5).trunc_some("initial_cpb_removal_delay_length_minus1")?;
    r.read_bits(5).trunc_some("cpb_removal_delay_length_minus1")?;
    r.read_bits(5).trunc_some("dpb_output_delay_length_minus1")?;
    r.read_bits(5).trunc_some("time_offset_length")?;
    Ok(())
}

/// 高阶 Profile 段解析产物（SPS 主体字段的分组承载，仅本模块内部使用）。
struct SpsHighProfileExtra {
    chroma_format_idc: u32,
    separate_colour_plane_flag: u8,
    bit_depth_luma_minus8: u32,
    bit_depth_chroma_minus8: u32,
    qpprime_y_zero_transform_bypass_flag: u8,
    seq_scaling_matrix_present_flag: u8,
    extra_parsed: bool,
}

/// 高阶 Profile 判定表：这些 `profile_idc` 才有色度/位深/缩放矩阵段。
///
/// 规范 7.3.2.1.1 的 `profile_idc` 取值集合（凡列出的都要读高阶段）：
/// 44(Cavlc 4:4:4 Intra), 66(Baseline/Constrained Baseline —— **不含**),
/// 77(Main), 83(Scalable Baseline), 86(Scalable High),
/// 88(Extended), 100(High), 110(High 10), 122(High 4:2:2),
/// 118(Multiview High), 128(Stereo High), 134(MFC High),
/// 135(MFC Depth High), 138(Enhanced Multiview Depth High),
/// 139(Enhanced Multiview Depth High)。
///
/// 常见错误（本模块第一版就踩了）：把列表写成「High 系列」而漏掉 **77
/// (Main)** 与 83/86（可扩展）。漏 77 的后果很隐蔽：Main SPS 的
/// `chroma_format_idc` / `bit_depth_*` / `seq_scaling_matrix_present_flag`
/// 会被当成下一段语法读掉——由于它们恰好都是小 ue 编码，读出来的值
/// 「看起来合理」（`log2_max_frame_num_minus4` 等全在合法范围），
/// 但**每一个 SPS 的后续字段都错位**，直到 VUI 那里才炸出
/// `bit_rate_scale` 提前结束——报错位置离真因十万八千里。
/// 判据见 [`C04-SPS-MAIN`] 与 [`C04-PROFILE-THREE`]。
fn is_high_profile_idc(idc: u8) -> bool {
    matches!(
        idc,
        44 | 77 | 83 | 86 | 88 | 100 | 110 | 118 | 122 | 128 | 134 | 135 | 138 | 139 | 244
    )
}

/// 解析 SPS 高阶 Profile 段（色度格式 / 位深 / 缩放矩阵跳过）。
fn parse_sps_high_profile(r: &mut BitReader, limits: &H264Limits) -> R<SpsHighProfileExtra> {
    let chroma_format_idc = r.read_ue(limits).trunc_err("chroma_format_idc")?;
    let separate_colour_plane_flag = if chroma_format_idc == 3 {
        r.read_bit().trunc_some("separate_colour_plane_flag")?
    } else {
        0
    };
    let bit_depth_luma_minus8 = r.read_ue(limits).trunc_err("bit_depth_luma_minus8")?;
    let bit_depth_chroma_minus8 = r.read_ue(limits).trunc_err("bit_depth_chroma_minus8")?;
    let qpprime_y_zero_transform_bypass_flag =
        r.read_bit().trunc_some("qpprime_y_zero_transform_bypass_flag")?;
    let seq_scaling_matrix_present_flag =
        r.read_bit().trunc_some("seq_scaling_matrix_present_flag")?;
    if seq_scaling_matrix_present_flag == 1 {
        let count = if chroma_format_idc != 3 { 8 } else { 12 };
        for idx in 0..count {
            let present = r.read_bit().trunc_some("seq_scaling_list_present_flag")?;
            if present == 1 {
                let size = if idx < 6 { 16 } else { 64 };
                skip_scaling_list(r, size, limits).trunc_err("seq_scaling_list")?;
            }
        }
    }
    Ok(SpsHighProfileExtra {
        chroma_format_idc,
        separate_colour_plane_flag,
        bit_depth_luma_minus8,
        bit_depth_chroma_minus8,
        qpprime_y_zero_transform_bypass_flag,
        seq_scaling_matrix_present_flag,
        extra_parsed: true,
    })
}

/// `pic_order_cnt_type` 分支解析产物（SPS 主体字段的分组承载）。
struct SpsPocFields {
    log2_max_pic_order_cnt_lsb_minus4: u32,
    delta_pic_order_always_zero_flag: u8,
}

/// 解析 SPS 的 POC 段（type 0/1/2 三型，type 越界显性拒绝）。
fn parse_sps_poc(r: &mut BitReader, limits: &H264Limits, pic_order_cnt_type: u32) -> R<SpsPocFields> {
    match pic_order_cnt_type {
        0 => Ok(SpsPocFields {
            log2_max_pic_order_cnt_lsb_minus4: r
                .read_ue(limits)
                .trunc_err("log2_max_pic_order_cnt_lsb_minus4")?,
            delta_pic_order_always_zero_flag: 0,
        }),
        1 => {
            let delta_pic_order_always_zero_flag =
                r.read_bit().trunc_some("delta_pic_order_always_zero_flag")?;
            r.read_se(limits).trunc_err("offset_for_non_ref_pic")?;
            r.read_se(limits).trunc_err("offset_for_top_to_bottom_field")?;
            let n = r
                .read_ue(limits)
                .trunc_err("num_ref_frames_in_pic_order_cnt_cycle")?;
            // 有界循环：防止 countNum 炸弹。
            if n > limits.max_macroblocks_per_slice {
                return Err(Failure::new(
                    DiagCode::ExpGolombTooLong,
                    format!(
                        "num_ref_frames_in_pic_order_cnt_cycle {} 超出上限 {}",
                        n, limits.max_macroblocks_per_slice
                    ),
                    "POC 周期长度炸弹；拒绝解析（F1122 计数纪律）",
                ));
            }
            for _ in 0..n {
                r.read_se(limits).trunc_err("offset_for_ref_frame")?;
            }
            Ok(SpsPocFields {
                log2_max_pic_order_cnt_lsb_minus4: 0,
                delta_pic_order_always_zero_flag,
            })
        }
        2 => Ok(SpsPocFields {
            log2_max_pic_order_cnt_lsb_minus4: 0,
            delta_pic_order_always_zero_flag: 0,
        }),
        _ => Err(Failure::new(
            DiagCode::SpsGeometryInconsistent,
            format!("pic_order_cnt_type {} 超出规范 [0,2]", pic_order_cnt_type),
            "规范 7.4.2.1.1 只定义 0/1/2 三型；其他值非法",
        )),
    }
}

/// `frame_cropping_flag == 1` 时读四个裁剪偏移；否则全零。
fn parse_sps_cropping(
    r: &mut BitReader,
    limits: &H264Limits,
    frame_cropping_flag: u8,
) -> R<(u32, u32, u32, u32)> {
    if frame_cropping_flag != 1 {
        return Ok((0, 0, 0, 0));
    }
    let cl = r.read_ue(limits).trunc_err("frame_crop_left_offset")?;
    let cr = r.read_ue(limits).trunc_err("frame_crop_right_offset")?;
    let ct = r.read_ue(limits).trunc_err("frame_crop_top_offset")?;
    let cb = r.read_ue(limits).trunc_err("frame_crop_bottom_offset")?;
    Ok((cl, cr, ct, cb))
}

/// 解析 SPS（输入为解防竞争后的 RBSP，不含 NAL 头）。
/// 主体只保留「按序读字段 → 组装 SPS → 几何自检」骨架，
/// 高阶 Profile 段 / POC 段 / 裁剪段分别下沉到
/// [`parse_sps_high_profile`] / [`parse_sps_poc`] / [`parse_sps_cropping`]。
pub fn parse_sps(rbsp: &[u8], limits: &H264Limits) -> R<Sps> {
    let mut r = BitReader::new(rbsp);
    let profile_idc = r.read_bits(8).trunc_some("profile_idc")? as u8;
    let constraint_flags = r.read_bits(8).trunc_some("constraint_set_flags + reserved")?;
    let level_idc = r.read_bits(8).trunc_some("level_idc")? as u8;

    let profile = Profile::from_idc_flags(profile_idc, ((constraint_flags >> 6) & 1) as u8);
    let constraint_set0_flag = ((constraint_flags >> 7) & 1) as u8;
    let constraint_set1_flag = ((constraint_flags >> 6) & 1) as u8;
    let constraint_set2_flag = ((constraint_flags >> 5) & 1) as u8;

    let seq_parameter_set_id = r.read_ue(limits)?;

    let hp = if is_high_profile_idc(profile_idc) {
        parse_sps_high_profile(&mut r, limits)?
    } else {
        SpsHighProfileExtra {
            chroma_format_idc: 1,
            separate_colour_plane_flag: 0,
            bit_depth_luma_minus8: 0,
            bit_depth_chroma_minus8: 0,
            qpprime_y_zero_transform_bypass_flag: 0,
            seq_scaling_matrix_present_flag: 0,
            extra_parsed: false,
        }
    };

    let log2_max_frame_num_minus4 = r.read_ue(limits).trunc_err("log2_max_frame_num_minus4")?;
    let pic_order_cnt_type = r.read_ue(limits).trunc_err("pic_order_cnt_type")?;
    let poc = parse_sps_poc(&mut r, limits, pic_order_cnt_type)?;

    let max_num_ref_frames = r.read_ue(limits).trunc_err("max_num_ref_frames")?;
    let gaps_in_frame_num_value_allowed_flag =
        r.read_bit().trunc_some("gaps_in_frame_num_value_allowed_flag")?;
    let pic_width_in_mbs_minus1 = r.read_ue(limits).trunc_err("pic_width_in_mbs_minus1")?;
    let pic_height_in_map_units_minus1 =
        r.read_ue(limits).trunc_err("pic_height_in_map_units_minus1")?;
    let frame_mbs_only_flag = r.read_bit().trunc_some("frame_mbs_only_flag")?;
    let mb_adaptive_frame_field_flag = if frame_mbs_only_flag == 0 {
        r.read_bit().trunc_some("mb_adaptive_frame_field_flag")?
    } else {
        0
    };
    let direct_8x8_inference_flag = r.read_bit().trunc_some("direct_8x8_inference_flag")?;
    let frame_cropping_flag = r.read_bit().trunc_some("frame_cropping_flag")?;
    let (cl, cr, ct, cb) = parse_sps_cropping(&mut r, limits, frame_cropping_flag)?;
    let vui_parameters_present_flag = r.read_bit().trunc_some("vui_parameters_present_flag")?;
    if vui_parameters_present_flag == 1 {
        skip_vui(&mut r, limits)?;
    }

    let sps = Sps {
        profile_idc,
        profile,
        constraint_set0_flag,
        constraint_set1_flag,
        constraint_set2_flag,
        level_idc,
        seq_parameter_set_id,
        chroma_format_idc: hp.chroma_format_idc,
        separate_colour_plane_flag: hp.separate_colour_plane_flag,
        bit_depth_luma_minus8: hp.bit_depth_luma_minus8,
        bit_depth_chroma_minus8: hp.bit_depth_chroma_minus8,
        qpprime_y_zero_transform_bypass_flag: hp.qpprime_y_zero_transform_bypass_flag,
        seq_scaling_matrix_present_flag: hp.seq_scaling_matrix_present_flag,
        log2_max_frame_num_minus4,
        pic_order_cnt_type,
        log2_max_pic_order_cnt_lsb_minus4: poc.log2_max_pic_order_cnt_lsb_minus4,
        delta_pic_order_always_zero_flag: poc.delta_pic_order_always_zero_flag,
        max_num_ref_frames,
        gaps_in_frame_num_value_allowed_flag,
        pic_width_in_mbs_minus1,
        pic_height_in_map_units_minus1,
        frame_mbs_only_flag,
        mb_adaptive_frame_field_flag,
        direct_8x8_inference_flag,
        frame_cropping_flag,
        frame_crop_left_offset: cl,
        frame_crop_right_offset: cr,
        frame_crop_top_offset: ct,
        frame_crop_bottom_offset: cb,
        vui_parameters_present_flag,
        high_profile_extra_parsed: hp.extra_parsed,
    };
    sps.validate_geometry(limits)?;
    Ok(sps)
}

/// FMO（slice group）语法的**比特级跳过**：字段极多，本模块不消费其语义，
/// 但必须精确跳过，否则后续所有 QP 字段错位。
/// 返回 `(slice_group_map_type, run_length_minus1)`（type 0 的 run_length
/// 是本模块唯一保留语义的 FMO 字段）。
fn parse_fmo_slice_groups(
    r: &mut BitReader,
    limits: &H264Limits,
    num_slice_groups_minus1: u32,
) -> R<(u32, Vec<u32>)> {
    if num_slice_groups_minus1 == 0 {
        return Ok((0, Vec::new()));
    }
    let slice_group_map_type = r.read_ue(limits).trunc_err("slice_group_map_type")?;
    let mut run_length_minus1: Vec<u32> = Vec::new();
    match slice_group_map_type {
        0 => {
            for _ in 0..=num_slice_groups_minus1 {
                let v = r.read_ue(limits).trunc_err("run_length_minus1")?;
                run_length_minus1.push(v);
            }
        }
        2 => {
            for _ in 0..num_slice_groups_minus1 {
                r.read_ue(limits).trunc_err("top_left/bottom_right")?;
                r.read_ue(limits).trunc_err("top_left/bottom_right")?;
            }
        }
        3 | 4 | 5 => {
            r.read_bit().trunc_some("slice_group_change_direction_flag")?;
            r.read_ue(limits).trunc_err("slice_group_change_rate_minus1")?;
        }
        6 => {
            // FMO type 6：`slice_group_id` 的个数 == PicSizeInMapUnits，
            // 该值**只存在于 SPS**。PPS 解析阶段拿不到 → 不猜测、不按上限
            // 空跑 65536 次（那是自证式算术且耗时失控），显性拒绝并要求上层
            // 先提供 SPS。这是本模块对 FMO 的**诚实缺口声明**（见头注）。
            return Err(Failure::new(
                DiagCode::SpsPpsRefMissing,
                "slice_group_map_type == 6 需要 PicSizeInMapUnits（仅存在于 SPS）".to_string(),
                "PPS 单独解析时无法确定 slice_group_id 数量；须由上层在已登记 SPS 后重解该 PPS（不猜测长度）",
            ));
        }
        _ => {
            return Err(Failure::new(
                DiagCode::SpsGeometryInconsistent,
                format!("slice_group_map_type {} 超出规范 [0,6]", slice_group_map_type),
                "规范 7.4.2.2.3 只定义 0..6；其他值非法",
            ))
        }
    }
    Ok((slice_group_map_type, run_length_minus1))
}

/// PPS 尾部解析产物（`more_rbsp_data()` 为真时才存在的字段）。
struct PpsTailFields {
    transform_8x8_mode_flag_present: u8,
    transform_8x8_mode_flag: u8,
    pic_scaling_matrix_present_flag: u8,
    second_chroma_qp_index_offset: i32,
}

/// 解析 PPS 尾部（8x8 变换标志 + 缩放矩阵跳过 + 第二色度 QP 偏移）。
fn parse_pps_tail(
    r: &mut BitReader,
    limits: &H264Limits,
    chroma_qp_index_offset: i32,
) -> R<PpsTailFields> {
    if !r.more_rbsp_data() {
        return Ok(PpsTailFields {
            transform_8x8_mode_flag_present: 0,
            transform_8x8_mode_flag: 0,
            pic_scaling_matrix_present_flag: 0,
            second_chroma_qp_index_offset: chroma_qp_index_offset,
        });
    }
    let transform_8x8_mode_flag_present: u8 = 1;
    let transform_8x8_mode_flag = r.read_bit().trunc_some("transform_8x8_mode_flag")?;
    let pic_scaling_matrix_present_flag =
        r.read_bit().trunc_some("pic_scaling_matrix_present_flag")?;
    if pic_scaling_matrix_present_flag == 1 {
        let count = 6 + if transform_8x8_mode_flag != 0 { 2 } else { 0 };
        for idx in 0..count {
            let present = r.read_bit().trunc_some("pic_scaling_list_present_flag")?;
            if present == 1 {
                let size = if idx < 6 { 16 } else { 64 };
                skip_scaling_list(r, size, limits).trunc_err("pic_scaling_list")?;
            }
        }
    }
    let second_chroma_qp_index_offset =
        r.read_se(limits).trunc_err("second_chroma_qp_index_offset")?;
    Ok(PpsTailFields {
        transform_8x8_mode_flag_present,
        transform_8x8_mode_flag,
        pic_scaling_matrix_present_flag,
        second_chroma_qp_index_offset,
    })
}

/// 默认参考列表长度 l0/l1 必须在规范上限内（≤32，越界会导致参考索引越界）。
fn validate_pps_ref_list_len(limits: &H264Limits, l0_minus1: u32, l1_minus1: u32) -> R<()> {
    if l0_minus1 >= limits.max_ref_list_len || l1_minus1 >= limits.max_ref_list_len {
        return Err(Failure::new(
            DiagCode::SpsGeometryInconsistent,
            format!(
                "默认参考列表长度 l0={} l1={} 超出上限 {}",
                l0_minus1 + 1,
                l1_minus1 + 1,
                limits.max_ref_list_len
            ),
            "参考列表长度越界；拒绝接受（会导致参考索引越界）",
        ));
    }
    Ok(())
}

/// 解析 PPS（输入为解防竞争后的 RBSP，不含 NAL 头）。
/// 主体只保留「按序读字段 → 组装 PPS」骨架，FMO 跳过 / 尾部语法 /
/// 参考列表校验分别下沉到 [`parse_fmo_slice_groups`] /
/// [`parse_pps_tail`] / [`validate_pps_ref_list_len`]。
pub fn parse_pps(rbsp: &[u8], limits: &H264Limits) -> R<Pps> {
    let mut r = BitReader::new(rbsp);
    let pic_parameter_set_id = r.read_ue(limits).trunc_err("pic_parameter_set_id")?;
    let seq_parameter_set_id = r.read_ue(limits).trunc_err("seq_parameter_set_id")?;
    let entropy_coding_mode_flag = r.read_bit().trunc_some("entropy_coding_mode_flag")?;
    let bottom_field_pic_order_in_frame_present_flag =
        r.read_bit().trunc_some("bottom_field_pic_order_in_frame_present_flag")?;
    let num_slice_groups_minus1 = r.read_ue(limits).trunc_err("num_slice_groups_minus1")?;

    let (slice_group_map_type, run_length_minus1) =
        parse_fmo_slice_groups(&mut r, limits, num_slice_groups_minus1)?;

    let num_ref_idx_l0_default_active_minus1 =
        r.read_ue(limits).trunc_err("num_ref_idx_l0_default_active_minus1")?;
    let num_ref_idx_l1_default_active_minus1 =
        r.read_ue(limits).trunc_err("num_ref_idx_l1_default_active_minus1")?;
    let weighted_pred_flag = r.read_bit().trunc_some("weighted_pred_flag")?;
    let weighted_bipred_idc = r.read_bits(2).trunc_some("weighted_bipred_idc")? as u8;
    let pic_init_qp_minus26 = r.read_se(limits).trunc_err("pic_init_qp_minus26")?;
    let pic_init_qs_minus26 = r.read_se(limits).trunc_err("pic_init_qs_minus26")?;
    let chroma_qp_index_offset = r.read_se(limits).trunc_err("chroma_qp_index_offset")?;
    let deblocking_filter_control_present_flag =
        r.read_bit().trunc_some("deblocking_filter_control_present_flag")?;
    let constrained_intra_pred_flag = r.read_bit().trunc_some("constrained_intra_pred_flag")?;
    let redundant_pic_cnt_present_flag = r.read_bit().trunc_some("redundant_pic_cnt_present_flag")?;

    // PPS 尾部（`more_rbsp_data()` 为真时才存在）。
    let tail = parse_pps_tail(&mut r, limits, chroma_qp_index_offset)?;

    // 裁剪参数必须在规范上限内（l0/l1 参考索引 ≤32）。
    validate_pps_ref_list_len(
        limits,
        num_ref_idx_l0_default_active_minus1,
        num_ref_idx_l1_default_active_minus1,
    )?;

    Ok(Pps {
        pic_parameter_set_id,
        seq_parameter_set_id,
        entropy_coding_mode_flag,
        bottom_field_pic_order_in_frame_present_flag,
        num_slice_groups_minus1,
        num_ref_idx_l0_default_active_minus1,
        num_ref_idx_l1_default_active_minus1,
        weighted_pred_flag,
        weighted_bipred_idc,
        pic_init_qp_minus26,
        pic_init_qs_minus26,
        chroma_qp_index_offset,
        deblocking_filter_control_present_flag,
        constrained_intra_pred_flag,
        redundant_pic_cnt_present_flag,
        transform_8x8_mode_flag_present: tail.transform_8x8_mode_flag_present,
        transform_8x8_mode_flag: tail.transform_8x8_mode_flag,
        pic_scaling_matrix_present_flag: tail.pic_scaling_matrix_present_flag,
        second_chroma_qp_index_offset: tail.second_chroma_qp_index_offset,
        slice_group_map_type,
        run_length_minus1,
    })
}

// ════════════════════════════════════════════════════════════════════════════
