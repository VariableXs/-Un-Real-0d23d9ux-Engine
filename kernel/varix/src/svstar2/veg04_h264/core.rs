//! VE-F1204 · H.264 解码器（VE-G 域 · G01 视频解码组 · L2/L3）
//!
//! **VE-F1204 · H.264 解码器 · §1-§2 诊断与结果类型 + 整数安全算术**
//!
//! 由 `veg04_h264.rs` 按职责拆分而来（L5 子模块拆分，行为保持）。
//! 全部符号经 `veg04_h264` 的 `pub use` 再导出，对外路径不变。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// §1 诊断与结果类型（零静默基础设施；与 F1201/F1202/F1203 同纪律，各自独立实现）
// ════════════════════════════════════════════════════════════════════════════

/// 诊断码：每种畸形独立可检索。处置方向不同的状态不共用码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagCode {
    /// NAL 头 `forbidden_zero_bit` 置位（比特流语法违规）。
    ForbiddenZeroBitSet,
    /// NAL 类型与 `nal_ref_idc` 矛盾（如 SPS 的 ref_idc==0）。
    NalRefIdcInconsistent,
    /// NAL 单元越界（长度超出 Annex-B 缓冲）。
    NalOutOfRange,
    /// 找不到起始码前缀（4 字节或 3 字节形式均试过）。
    StartCodeNotFound,
    /// 位流提前结束（读位越过 RBSP 末尾）。
    BitstreamTruncated,
    /// Exp-Golomb 前导零超长（恶意构造的 codeNum 炸弹）。
    ExpGolombTooLong,
    /// SPS 内部几何参数矛盾（如 crop 后尺寸为 0）。
    SpsGeometryInconsistent,
    /// SPS 引用了不存在的 PPS id。
    SpsPpsRefMissing,
    /// 参数集已登记后试图以不同几何参数改写。
    ParameterSetGeometryChange,
    /// 参数集 id 越界。
    ParameterSetIdOutOfRange,
    /// 参考帧索引越界（≥ DPB 槽位数）。
    RefIndexOutOfRange,
    /// DPB 帧数超出预算 / 单帧字节超预算。
    DpbOverflow,
    /// 熵引擎状态越界（CAVLC 表外码字 / CABAC range 非法）。
    EntropyEngineOutOfRange,
    /// 熵引擎按 PPS 切换时被要求执行当前 PPS 不支持的能力。
    EntropyEngineUnsupported,
    /// 运动矢量超出合法范围（半像素位移越出参考帧）。
    MvOutOfRange,
    /// 帧内预测模式在当前分块尺寸下不可用（如 High 以下的 8x8）。
    IntraModeUnavailable,
    /// 切片类型与 PPS 能力矛盾（如 I 切片却要P 的参考表）。
    SliceTypeUnsupported,
    /// 已登记的非参考 NAL（宽容跳过，非致命）。
    NonReferenceNalSkipped,
    /// 未登记的 NAL 类型（宽容跳过，非致命）。
    NalTypeUnknown,
    /// 判据自检不通过。
    CriterionSelfCheckFailed,
}

/// 诊断码的稳定字符串名（诊断台账与 grep 可检索）。
pub const fn diag_name(code: DiagCode) -> &'static str {
    match code {
        DiagCode::ForbiddenZeroBitSet => "H264_FORBIDDEN_ZERO_BIT_SET",
        DiagCode::NalRefIdcInconsistent => "H264_NAL_REF_IDC_INCONSISTENT",
        DiagCode::NalOutOfRange => "H264_NAL_OUT_OF_RANGE",
        DiagCode::StartCodeNotFound => "H264_START_CODE_NOT_FOUND",
        DiagCode::BitstreamTruncated => "H264_BITSTREAM_TRUNCATED",
        DiagCode::ExpGolombTooLong => "H264_EXP_GOLOMB_TOO_LONG",
        DiagCode::SpsGeometryInconsistent => "H264_SPS_GEOMETRY_INCONSISTENT",
        DiagCode::SpsPpsRefMissing => "H264_SPS_PPS_REF_MISSING",
        DiagCode::ParameterSetGeometryChange => "H264_PARAMETERSET_GEOMETRY_CHANGE",
        DiagCode::ParameterSetIdOutOfRange => "H264_PARAMETERSET_ID_OUT_OF_RANGE",
        DiagCode::RefIndexOutOfRange => "H264_REF_INDEX_OUT_OF_RANGE",
        DiagCode::DpbOverflow => "H264_DPB_OVERFLOW",
        DiagCode::EntropyEngineOutOfRange => "H264_ENTROPY_ENGINE_OUT_OF_RANGE",
        DiagCode::EntropyEngineUnsupported => "H264_ENTROPY_ENGINE_UNSUPPORTED",
        DiagCode::MvOutOfRange => "H264_MV_OUT_OF_RANGE",
        DiagCode::IntraModeUnavailable => "H264_INTRA_MODE_UNAVAILABLE",
        DiagCode::SliceTypeUnsupported => "H264_SLICE_TYPE_UNSUPPORTED",
        DiagCode::NonReferenceNalSkipped => "H264_NON_REFERENCE_NAL_SKIPPED",
        DiagCode::NalTypeUnknown => "H264_NAL_TYPE_UNKNOWN",
        DiagCode::CriterionSelfCheckFailed => "CRITERION_SELFCHECK_FAILED",
    }
}

/// 一条诊断：发生了什么（code+message）、影响什么、下一步怎么办（hint）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diagnostic {
    pub code: DiagCode,
    pub message: String,
    pub hint: String,
}

/// 内部失败载体：三要素齐备。
///
/// 刻意**不**用 `Outcome<()>` 之类的类型承载它——那会撞 core 的
/// `impl<T> From<T> for T`（E0119）。这里用独立 struct，内部函数返回
/// `Result<T, Failure>`，故可自由使用 `?`。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Failure {
    pub code: DiagCode,
    pub message: String,
    pub hint: String,
}

impl Failure {
    pub fn new(code: DiagCode, message: String, hint: &str) -> Failure {
        Failure { code, message, hint: hint.to_string() }
    }

    /// 诊断三要素视图（供诊断袋回填）。
    pub fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic { code: self.code, message: self.message.clone(), hint: self.hint.clone() }
    }
}

/// 对外结果判别联合：成功必带 value 与诊断袋，失败必带 code/message/hint。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Outcome<T> {
    Ok { value: T, diagnostics: Vec<Diagnostic> },
    Err { failure: Failure, diagnostics: Vec<Diagnostic> },
}

impl<T> Outcome<T> {
    pub fn ok(value: T, diagnostics: Vec<Diagnostic>) -> Outcome<T> {
        Outcome::Ok { value, diagnostics }
    }

    pub fn err(failure: Failure, diagnostics: Vec<Diagnostic>) -> Outcome<T> {
        Outcome::Err { failure, diagnostics }
    }

    /// 取值（失败时按值消费 `self` 返回兜底值——不强加 `T: Clone`）。
    pub fn value_or(self, fallback: T) -> T {
        match self {
            Outcome::Ok { value, .. } => value,
            Outcome::Err { .. } => fallback,
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, Outcome::Ok { .. })
    }

    /// 失败时的三要素（成功时返回 `None`）。
    pub fn failure(&self) -> Option<&Failure> {
        match self {
            Outcome::Ok { .. } => None,
            Outcome::Err { failure, .. } => Some(failure),
        }
    }

    /// 诊断列表（成功与失败都带）。
    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            Outcome::Ok { diagnostics, .. } => diagnostics,
            Outcome::Err { diagnostics, .. } => diagnostics,
        }
    }
}

/// 诊断袋：累积非致命发现。
#[derive(Clone, Default, Debug)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
}

impl DiagBag {
    pub fn new() -> DiagBag {
        DiagBag { items: Vec::new() }
    }

    pub fn push(&mut self, code: DiagCode, message: String, hint: &str) {
        self.items.push(Diagnostic { code, message, hint: hint.to_string() });
    }

    pub fn push_failure(&mut self, f: &Failure) {
        self.items.push(f.to_diagnostic());
    }

    pub fn all(&self) -> &[Diagnostic] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 某诊断码的出现次数（自检按码点名核对用）。
    pub fn count_of(&self, code: DiagCode) -> usize {
        self.items.iter().filter(|d| d.code == code).count()
    }

    /// 取出并清空（转成 `Vec`）。
    pub fn drain(&mut self) -> Vec<Diagnostic> {
        let out = self.items.clone();
        self.items.clear();
        out
    }
}

/// 解码统计：宽容处理与拒绝的**计数**（不回退为「无异常」）。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct DecodeStats {
    /// 扫描到的 NAL 单元数。
    pub nal_units_seen: u64,
    /// 防竞争字节（0x03）剥离次数。
    pub epb_stripped: u64,
    /// SPS 登记次数（含热更新）。
    pub sps_registered: u64,
    /// PPS 登记次数（含热更新）。
    pub pps_registered: u64,
    /// 参数集热更新次数。
    pub hot_updates: u64,
    /// 解码的切片数。
    pub slices_decoded: u64,
    /// 输出帧数。
    pub frames_output: u64,
    /// DPB 峰值占用（帧）。
    pub dpb_peak_frames: u64,
    /// DPB 峰值字节。
    pub dpb_peak_bytes: u64,
    /// 越界拒绝次数（DPB 预算 / 参考索引 / MV）。
    pub rejected: u64,
}

// ════════════════════════════════════════════════════════════════════════════
// §2 整数安全算术 + 计数上限（F1122 本地落地面）
// ════════════════════════════════════════════════════════════════════════════

pub fn checked_add(a: u64, b: u64) -> Option<u64> {
    a.checked_add(b)
}

pub fn checked_mul(a: u64, b: u64) -> Option<u64> {
    a.checked_mul(b)
}

/// 区间校验：`[offset, offset+size)` 是否完整落在 `[0, limit)` 内。
pub fn checked_range(offset: u64, size: u64, limit: u64) -> bool {
    if offset > limit || size > limit {
        return false;
    }
    match checked_add(offset, size) {
        Some(end) => end <= limit,
        None => false,
    }
}

/// H.264 计数上限（数据驱动，F1122 要求）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct H264Limits {
    /// 单码流 NAL 单元数上限。
    pub max_nal_units: usize,
    /// 单 NAL 字节数上限。
    pub max_nal_bytes: u32,
    /// 宏块宽/高上限（16K×16K 远超任何真实码流）。
    pub max_dim_mbs: u32,
    /// 参考帧数上限（规范 max_num_ref_frames ≤ 16）。
    pub max_num_ref_frames: u32,
    /// 参考列表长度上限。
    pub max_ref_list_len: u32,
    /// 运动矢量水平/垂直位移上限（1/4 像素单位）。
    pub max_mv_abs: i32,
    /// DPB 槽位上限。
    pub max_dpb_slots: usize,
    /// DPB 字节预算。
    pub dpb_budget_bytes: u64,
    /// Exp-Golomb 前导零上限。
    pub max_exp_golomb_leading: u32,
    /// 切片内宏块数上限。
    pub max_macroblocks_per_slice: u32,
}

/// 生产上限。
pub const H264_LIMITS: H264Limits = H264Limits {
    max_nal_units: 4_000_000,
    max_nal_bytes: 32 * 1024 * 1024,
    max_dim_mbs: 4096,
    max_num_ref_frames: 16,
    max_ref_list_len: 32,
    max_mv_abs: 32768,
    max_dpb_slots: 18,
    dpb_budget_bytes: 512 * 1024 * 1024,
    max_exp_golomb_leading: 32,
    max_macroblocks_per_slice: 256 * 256,
};

/// 内部统一的 Result 别名（`?` 可用；不暴露给 crate 外）。
pub(crate) type R<T> = Result<T, Failure>;

/// 越界失败构造。
pub fn range_failure(what: &str, offset: u64, size: u64, limit: u64) -> Failure {
    Failure::new(
        DiagCode::NalOutOfRange,
        format!("{} 越界：[{},+{}) 超出上界 {}", what, offset, size, limit),
        "声明长度超出缓冲；这是损坏文件或恶意构造的典型特征，拒绝解析（F1122 纪律）",
    )
}

/// 溢出失败构造。
pub(crate) fn overflow_failure(what: &str) -> Failure {
    Failure::new(
        DiagCode::DpbOverflow,
        format!("{} 溢出 u64 值域", what),
        "字节流声明的数值超出可表示范围；拒绝解析（F1122 纪律）",
    )
}

// ════════════════════════════════════════════════════════════════════════════
