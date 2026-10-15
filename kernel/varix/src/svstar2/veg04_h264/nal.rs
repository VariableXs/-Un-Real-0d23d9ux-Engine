//! VE-F1204 · H.264 解码器（VE-G 域 · G01 视频解码组 · L2/L3）
//!
//! **VE-F1204 · H.264 解码器 · §3-§4 NAL 单元层 + 位读取器**
//!
//! 由 `veg04_h264.rs` 按职责拆分而来（L5 子模块拆分，行为保持）。
//! 全部符号经 `veg04_h264` 的 `pub use` 再导出，对外路径不变。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::core::{DecodeStats, DiagBag, DiagCode, Diagnostic, Failure, H264Limits, Outcome, R, checked_add, checked_range, overflow_failure, range_failure};

// §3 NAL 单元层（NAL 头 / 起始码前缀 / RBSP 防竞争剥离）
// ════════════════════════════════════════════════════════════════════════════

/// H.264 NAL 单元类型（表 7-1 的解码相关子集）。
pub mod nal_type {
    /// 非 IDR 切片。
    pub const SLICE_NON_IDR: u8 = 1;
    /// 数据分区 A 切片。
    pub const DPA: u8 = 2;
    /// 数据分区 B 切片。
    pub const DPB: u8 = 3;
    /// 数据分区 C 切片。
    pub const DPC: u8 = 4;
    /// IDR 切片。
    pub const SLICE_IDR: u8 = 5;
    /// SEI。
    pub const SEI: u8 = 6;
    /// SPS。
    pub const SPS: u8 = 7;
    /// PPS。
    pub const PPS: u8 = 8;
    /// 访问单元分隔符。
    pub const ACCESS_UNIT_DELIMITER: u8 = 9;
    /// 序列结束。
    pub const END_OF_SEQUENCE: u8 = 10;
    /// 流结束。
    pub const END_OF_STREAM: u8 = 11;
    /// 填充数据。
    pub const FILLER_DATA: u8 = 12;
    /// 序列参数集扩展。
    pub const SPS_EXT: u8 = 13;
    /// 预留切片类型 14。
    pub const RESERVED_14: u8 = 14;
    /// 预留切片类型 15。
    pub const RESERVED_15: u8 = 15;
    /// 辅助编码层切片（扩展）。
    pub const SLICE_AUX: u8 = 19;
    /// 切片扩展。
    pub const SLICE_EXT: u8 = 20;
    /// 深度扩展。
    pub const DEPTH_EXT: u8 = 21;
}

/// 切片类型（表 7-6 的 `slice_type` 取值域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SliceType {
    /// P 切片（全部宏块可为帧内）。
    P,
    /// B 切片。
    B,
    /// I 切片（全部宏块为帧内）。
    I,
    /// SP 切片。
    Sp,
    /// SI 切片。
    Si,
}

impl SliceType {
    /// `slice_type` 的 5 位原始值。
    pub const fn raw(self) -> u8 {
        match self {
            SliceType::P => 0,
            SliceType::B => 1,
            SliceType::I => 2,
            SliceType::Sp => 3,
            SliceType::Si => 4,
        }
    }

    /// 由 `slice_type` 原始值构造（5..9 为「按切片内宏块全部适用」形态，归一到0..4）。
    pub fn from_raw(v: u32) -> Option<SliceType> {
        let base = if v >= 5 { v - 5 } else { v };
        match base {
            0 => Some(SliceType::P),
            1 => Some(SliceType::B),
            2 => Some(SliceType::I),
            3 => Some(SliceType::Sp),
            4 => Some(SliceType::Si),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            SliceType::P => "P",
            SliceType::B => "B",
            SliceType::I => "I",
            SliceType::Sp => "SP",
            SliceType::Si => "SI",
        }
    }

    /// 该类型是否**可能**包含帧间宏块（决定是否需要参考列表）。
    pub const fn has_inter(self) -> bool {
        !matches!(self, SliceType::I | SliceType::Si)
    }

    /// 该类型是否**允许**帧内宏块（P/SP/B 都允许）。
    pub const fn has_intra(self) -> bool {
        true
    }
}

/// NAL 头（表 7-1 的前 8 位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NalHeader {
    /// `forbidden_zero_bit`：必须为 0。
    pub forbidden_zero_bit: u8,
    /// `nal_ref_idc`（2 位）：0 表示非参考。
    pub nal_ref_idc: u8,
    /// `nal_unit_type`（5 位）。
    pub nal_unit_type: u8,
}

impl NalHeader {
    pub const fn is_reference(&self) -> bool {
        self.nal_ref_idc != 0
    }

    pub const fn is_idr_slice(&self) -> bool {
        self.nal_unit_type == nal_type::SLICE_IDR
    }
}

/// NAL 类型是否**必须** `nal_ref_idc != 0`（SPS/PPS/SEI/IDR 切片）。
///
/// 规范 7.4.1.2.1：`nal_ref_idc` 为 0 时该 NAL 不用于参考，且类型 5、7、8
/// 不允许 `nal_ref_idc == 0`。
pub const fn nal_type_requires_ref_idc(ty: u8) -> bool {
    ty == nal_type::SLICE_IDR || ty == nal_type::SEI || ty == nal_type::SPS || ty == nal_type::PPS
}

/// NAL 类型是否为切片（含数据分区与扩展切片）。
pub fn nal_type_is_slice(ty: u8) -> bool {
    matches!(
        ty,
        nal_type::SLICE_NON_IDR
            | nal_type::DPA
            | nal_type::DPB
            | nal_type::DPC
            | nal_type::SLICE_IDR
            | nal_type::RESERVED_14
            | nal_type::RESERVED_15
            | nal_type::SLICE_AUX
            | nal_type::SLICE_EXT
            | nal_type::DEPTH_EXT
    )
}

/// NAL 类型的稳定字符串名。
pub const fn nal_type_name(ty: u8) -> &'static str {
    match ty {
        nal_type::SLICE_NON_IDR => "SLICE_NON_IDR",
        nal_type::DPA => "SLICE_DP_A",
        nal_type::DPB => "SLICE_DP_B",
        nal_type::DPC => "SLICE_DP_C",
        nal_type::SLICE_IDR => "SLICE_IDR",
        nal_type::SEI => "SEI",
        nal_type::SPS => "SPS",
        nal_type::PPS => "PPS",
        nal_type::ACCESS_UNIT_DELIMITER => "ACCESS_UNIT_DELIMITER",
        nal_type::END_OF_SEQUENCE => "END_OF_SEQUENCE",
        nal_type::END_OF_STREAM => "END_OF_STREAM",
        nal_type::FILLER_DATA => "FILLER_DATA",
        nal_type::SPS_EXT => "SPS_EXT",
        nal_type::RESERVED_14 => "RESERVED_14",
        nal_type::RESERVED_15 => "RESERVED_15",
        nal_type::SLICE_AUX => "SLICE_AUX",
        nal_type::SLICE_EXT => "SLICE_EXT",
        nal_type::DEPTH_EXT => "DEPTH_EXT",
        _ => "UNKNOWN",
    }
}

/// 解析 NAL 头（1 字节）。
///
/// 拒绝两类畸形：`forbidden_zero_bit` 置位；类型↔`nal_ref_idc` 矛盾。
pub fn parse_nal_header(b: u8) -> Outcome<NalHeader> {
    let header = NalHeader {
        forbidden_zero_bit: (b >> 7) & 0x01,
        nal_ref_idc: (b >> 5) & 0x03,
        nal_unit_type: b & 0x1F,
    };
    let mut bag = DiagBag::new();
    if header.forbidden_zero_bit != 0 {
        let f = Failure::new(
            DiagCode::ForbiddenZeroBitSet,
            format!("NAL 头 forbidden_zero_bit=1（字节 0x{:02X}）", b),
            "比特流语法违规；拒绝该 NAL（不是编码器可产出的合法形态）",
        );
        bag.push_failure(&f);
        return Outcome::err(f, bag.drain());
    }
    if nal_type_requires_ref_idc(header.nal_unit_type) && header.nal_ref_idc == 0 {
        let f = Failure::new(
            DiagCode::NalRefIdcInconsistent,
            format!("NAL 类型 {} 要求 nal_ref_idc != 0，实际为 0", nal_type_name(header.nal_unit_type)),
            "规范 7.4.1.2.1：类型 5/7/8 的 nal_ref_idc 不得为 0；接受会让参数集静默错配",
        );
        bag.push_failure(&f);
        return Outcome::err(f, bag.drain());
    }
    Outcome::ok(header, bag.drain())
}

/// 起始码前缀的匹配形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StartCodeForm {
    /// 4 字节 `00 00 00 01`。
    FourByte,
    /// 3 字节 `00 00 01`（末位在字节流末或后继为 0）。
    ThreeByte,
}

/// 在 `buf` 的 `[from, limit)` 内定位起始码，返回 `(位置, 形态)`。
///
/// 4 字节形态优先：若`00 00 00 01`，返回 4 字节形态；否则 `00 00 01` 返回 3 字节。
/// 找不到返回 `None`（调用方产出 `StartCodeNotFound` 三要素）。
pub fn find_start_code(buf: &[u8], from: usize, limit: usize) -> Option<(usize, StartCodeForm)> {
    let end = if limit <= buf.len() { limit } else { buf.len() };
    let mut i = from;
    while i + 2 < end {
        if buf.get(i) == Some(&0) && buf.get(i + 1) == Some(&0) {
            if buf.get(i + 2) == Some(&1) {
                // `00 00 00 01`：4 字节形态优先（回退一个字节重新判定）。
                if i >= 1 && buf.get(i - 1) == Some(&0) {
                    return Some((i - 1, StartCodeForm::FourByte));
                }
                return Some((i, StartCodeForm::ThreeByte));
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    None
}

/// 起始码长度。
pub const fn start_code_len(form: StartCodeForm) -> usize {
    match form {
        StartCodeForm::FourByte => 4,
        StartCodeForm::ThreeByte => 3,
    }
}

/// 防竞争三字节剥离的结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RbspUnescape {
    /// 剥离后的 RBSP 字节。
    pub rbsp: Vec<u8>,
    /// 被剥离的防竞争字节（0x03）个数。
    pub stripped: u32,
    /// 原始长度（未剥离）。
    pub input_len: usize,
}

/// 把带防竞争字节的 NAL payload（不含 NAL 头）还原为 RBSP。
///
/// 规则（见模块头注「深度打磨要点一」）：维护 `zero_run`；当 `zero_run >= 2`
/// 且当前字节为 `0x03` 时剥离它，**且不重置** `zero_run`（连续的
/// `00 00 03 03` 第二个 `03` 是数据）；否则正常输出并维护 `zero_run`。
pub fn unescape_rbsp(payload: &[u8]) -> RbspUnescape {
    let mut rbsp: Vec<u8> = Vec::with_capacity(payload.len());
    let mut stripped: u32 = 0;
    let mut zero_run: usize = 0;
    let mut i = 0usize;
    while i < payload.len() {
        let b = match payload.get(i) {
            Some(v) => *v,
            None => break,
        };
        if zero_run >= 2 && b == 0x03 {
            stripped += 1;
            // 剥掉这个 03 后必须**清零 zero_run**：输出流此刻以 `00 00`
            // 结尾，下一个字节（若是 0x03）是真实数据。若保留 zero_run，
            // `00 00 03 03` 会被剥成 `00 00` ——第二个 03 是数据被吃掉，
            // 与编码侧 [`escape_rbsp`] 不成对（编码 `00 00 03` → `00 00 03 03`，
            // 解码却拿回 `00 00`）。规范 7.3.1 的语义也是「匹配到
            // `00 00 03` 就跳过这个 03」，不是「剥掉所有后续 03」。
            zero_run = 0;
            i += 1;
            continue;
        }
        if b == 0x00 {
            zero_run += 1;
        } else {
            zero_run = 0;
        }
        rbsp.push(b);
        i += 1;
    }
    RbspUnescape { rbsp, stripped, input_len: payload.len() }
}

/// 反向：给 RBSP 插入防竞争三字节（编码侧对称；F1222 编码器复用）。
///
/// 规则：当已输出 ≥2 个连续 0 且下一个待输出字节 ≤ `0x03` 时，先输出 `0x03`。
pub fn escape_rbsp(rbsp: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(rbsp.len() + rbsp.len() / 16 + 4);
    let mut zero_run: usize = 0;
    let mut i = 0usize;
    while i < rbsp.len() {
        let b = match rbsp.get(i) {
            Some(v) => *v,
            None => break,
        };
        if zero_run >= 2 && b <= 0x03 {
            out.push(0x03);
            zero_run = 0;
        }
        if b == 0x00 {
            zero_run += 1;
        } else {
            zero_run = 0;
        }
        out.push(b);
        i += 1;
    }
    out
}

/// 一个已切分并解escaped 的 NAL 单元。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NalUnit {
    /// NAL 头。
    pub header: NalHeader,
    /// 解防竞争后的 RBSP（不含 NAL 头与起始码）。
    pub rbsp: Vec<u8>,
    /// 在 Annex-B 缓冲中的原始范围`[start, end)`。
    pub start: usize,
    pub end: usize,
    /// 剥离的防竞争字节数。
    pub epb_stripped: u32,
}

/// Annex-B 字节流切分结果。
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct AnnexBScan {
    /// 切出的 NAL 单元。
    pub units: Vec<NalUnit>,
    /// 诊断（宽容跳过的类型、越界等）。
    pub diagnostics: Vec<Diagnostic>,
    /// 统计。
    pub stats: DecodeStats,
}

/// 处理「再无起始码」的尾部残余分支（规范 7.4.1.1 尾部语义）。
///
/// 返回 `Ok(())`：剩余为全零尾部且此前已产出过 NAL 单元，调用方可正常收尾
///（`trailing_zero_8bits` 只允许出现在最后一个 NAL 之后）。
/// 返回 `Err(f)`：显性拒绝——全零但一个 NAL 都没有（整段缓冲无起始码），
/// 或非全零残余（载荷未以起始码开始，损坏或截断）。
///
/// 常见错误（本模块第一版就踩了）：无条件「全零就 break」——从头到尾全零的
/// 缓冲被当成「只有尾部填充」而静默返回 0 个单元的 Ok，调用方以为解出了
/// 空流且零诊断。零静默纪律下必须区分「尾部填充的全零」与「无起始码的全零」。
fn reject_tail_without_start_code(
    buf: &[u8],
    cursor: usize,
    has_units: bool,
    bag: &mut DiagBag,
) -> Result<(), Failure> {
    let rest = buf.get(cursor..).unwrap_or(&[]);
    let all_zero = rest.iter().all(|b| *b == 0);
    if all_zero && has_units {
        return Ok(());
    }
    let f = if all_zero {
        // 一个 NAL 都没有：整段缓冲不含任何起始码。
        Failure::new(
            DiagCode::StartCodeNotFound,
            format!("缓冲全长 {} 字节均为零且不含任何起始码", rest.len()),
            "Annex-B 要求每个 NAL 前有起始码；全零缓冲不含 NAL，拒绝解析",
        )
    } else {
        Failure::new(
            DiagCode::StartCodeNotFound,
            format!("偏移 {} 起无起始码前缀且非全零尾部", cursor),
            "NAL 载荷未以起始码开始；拒绝解析（损坏或截断）",
        )
    };
    bag.push_failure(&f);
    Err(f)
}

/// 校验起始码之前的游离前缀（`sc_pos > cursor` 时）。
///
/// Annex-B 里第 0 个 NAL 必须从字节 0 开始的起始码起（`leading_zero_8bits`
/// 只允许是零）。`[cursor, sc_pos)` 若是既不属于上一个 NAL 载荷、又不含
/// 起始码的游离字节，则显性拒绝；全零前缀放行。
///
/// 常见错误（本模块第一版就踩了）：直接跳到 `sc_pos` 不看中间——流首带
/// 4 字节垃圾的流会被静默丢掉那 4 字节，格式错误零诊断。
fn reject_rogue_prefix(
    buf: &[u8],
    cursor: usize,
    sc_pos: usize,
    bag: &mut DiagBag,
) -> Result<(), Failure> {
    if sc_pos <= cursor {
        return Ok(());
    }
    let prefix = buf.get(cursor..sc_pos).unwrap_or(&[]);
    if prefix.iter().all(|b| *b == 0) {
        return Ok(());
    }
    let f = Failure::new(
        DiagCode::StartCodeNotFound,
        format!("偏移 {}..{} 是既无起始码又非全零的游离字节", cursor, sc_pos),
        "Annex-B 要求每个 NAL 前有起始码；该段无法归属任何 NAL，拒绝解析",
    );
    bag.push_failure(&f);
    Err(f)
}

/// 界定 NAL 载荷终点：下一个起始码之前的尾部 0x00 属于
/// `trailing_zero_8bits`，不计入载荷。
fn trim_trailing_zero_8bits(buf: &[u8], payload_start: usize, next_start: usize) -> usize {
    let mut e = next_start;
    while e > payload_start && buf.get(e.wrapping_sub(1)) == Some(&0) {
        e -= 1;
    }
    e
}

/// 取 NAL 首字节并解析 NAL 头（空载荷/畸形头显性拒绝，零静默）。
fn parse_unit_header(
    raw_nal: &[u8],
    payload_start: usize,
    bag: &mut DiagBag,
) -> Result<NalHeader, Failure> {
    let head_byte = match raw_nal.first() {
        Some(v) => *v,
        None => {
            let f = Failure::new(
                DiagCode::NalOutOfRange,
                format!("偏移 {} 处NAL 为空", payload_start),
                "NAL 必须至少有 1 字节 NAL 头；拒绝解析",
            );
            bag.push_failure(&f);
            return Err(f);
        }
    };
    match parse_nal_header(head_byte) {
        Outcome::Ok { value, .. } => Ok(value),
        Outcome::Err { failure, diagnostics } => {
            for d in diagnostics {
                bag.push(d.code, d.message, &d.hint);
            }
            bag.push_failure(&failure);
            Err(failure)
        }
    }
}

/// 非解码路径 NAL 的跳过诊断（留痕不拦截）：非切片且非参数集类按
/// 规范跳过并计数；非参考的参数集类同样计数留痕。
fn push_skip_diagnostics(bag: &mut DiagBag, header: &NalHeader, nal_bytes: u64) {
    if !nal_type_is_slice(header.nal_unit_type)
        && !matches!(
            header.nal_unit_type,
            nal_type::SPS | nal_type::PPS | nal_type::SEI | nal_type::SPS_EXT
        )
    {
        bag.push(
            DiagCode::NalTypeUnknown,
            format!("跳过非解码 NAL 类型 {}（{} 字节）", nal_type_name(header.nal_unit_type), nal_bytes),
            "该类型不参与本解码路径（填充/分隔符/结束标记），按规范跳过并计数",
        );
    } else if !header.is_reference() && !nal_type_is_slice(header.nal_unit_type) {
        bag.push(
            DiagCode::NonReferenceNalSkipped,
            format!("跳过非参考 NAL 类型 {}", nal_type_name(header.nal_unit_type)),
            "非参考的参数集类 NAL 仍会被上层忽略；此处计数留痕",
        );
    }
}

/// 扫描 Annex-B 字节流，切出全部 NAL 单元并逐个解防竞争。
///
/// `limits` 提供的上限防元素炸弹；`bag` 收非致命诊断。
/// 主体循环只保留「定位起始码 → 界定载荷 → 解析头 → 产出单元」骨架，
/// 各失败分支与诊断分类下沉到 [`reject_tail_without_start_code`] /
/// [`reject_rogue_prefix`] / [`parse_unit_header`] / [`push_skip_diagnostics`]。
pub fn scan_annexb(buf: &[u8], limits: &H264Limits, bag: &mut DiagBag) -> R<AnnexBScan> {
    let mut units: Vec<NalUnit> = Vec::new();
    let mut stats = DecodeStats::default();
    let total = buf.len() as u64;
    let mut cursor = 0usize;
    let mut guard = 0usize;

    while cursor < buf.len() {
        guard += 1;
        if guard > limits.max_nal_units {
            let f = Failure::new(
                DiagCode::DpbOverflow,
                format!("NAL 单元数超过上限 {}", limits.max_nal_units),
                "NAL 单元炸弹；拒绝继续扫描（F1122 计数纪律）",
            );
            bag.push_failure(&f);
            return Err(f);
        }
        let (sc_pos, form) = match find_start_code(buf, cursor, buf.len()) {
            Some(v) => v,
            None => {
                // 尾部残余处理：全零且已产出过 NAL 可收尾，其余显性拒绝
                //（零静默纪律——全零无 NAL / 非全零残余都不得当成功）。
                reject_tail_without_start_code(buf, cursor, !units.is_empty(), bag)?;
                break;
            }
        };
        // 起始码之前的游离前缀校验（全零放行，垃圾字节显性拒绝）。
        reject_rogue_prefix(buf, cursor, sc_pos, bag)?;
        let payload_start = match checked_add(sc_pos as u64, start_code_len(form) as u64) {
            Some(v) => v as usize,
            None => {
                let f = overflow_failure("起始码偏移");
                bag.push_failure(&f);
                return Err(f);
            }
        };
        // 下一个起始码（或流尾）界定本 NAL 载荷。
        let payload_end = match find_start_code(buf, payload_start, buf.len()) {
            Some((p, _)) => trim_trailing_zero_8bits(buf, payload_start, p),
            None => buf.len(),
        };
        if !checked_range(sc_pos as u64, (payload_end - sc_pos) as u64, total) {
            let f = range_failure("NAL 单元", sc_pos as u64, (payload_end - sc_pos) as u64, total);
            bag.push_failure(&f);
            return Err(f);
        }
        if payload_end == payload_start {
            // 空 NAL（起始码后紧接下一个起始码）：合法但不产出单元，仅推进游标。
            cursor = payload_end;
            continue;
        }
        let raw_nal = match buf.get(payload_start..payload_end) {
            Some(v) => v,
            None => {
                let f = range_failure("NAL 载荷", payload_start as u64, 0, total);
                bag.push_failure(&f);
                return Err(f);
            }
        };
        let nal_bytes = raw_nal.len() as u64;
        if nal_bytes > limits.max_nal_bytes as u64 {
            let f = Failure::new(
                DiagCode::DpbOverflow,
                format!("NAL 载荷 {} 字节超过上限 {}", nal_bytes, limits.max_nal_bytes),
                "单NAL 体积炸弹；拒绝解析（F1122 计数纪律）",
            );
            bag.push_failure(&f);
            return Err(f);
        }
        let header = parse_unit_header(raw_nal, payload_start, bag)?;
        let esc = unescape_rbsp(raw_nal.get(1..).unwrap_or(&[]));
        stats.nal_units_seen += 1;
        stats.epb_stripped += esc.stripped as u64;
        push_skip_diagnostics(bag, &header, nal_bytes);
        units.push(NalUnit {
            header,
            rbsp: esc.rbsp,
            start: sc_pos,
            end: payload_end,
            epb_stripped: esc.stripped,
        });
        cursor = payload_end;
    }
    Ok(AnnexBScan { units, diagnostics: bag.all().to_vec(), stats })
}

// ════════════════════════════════════════════════════════════════════════════
// §4 位读取器（Exp-Golomb / 定长 / RBSP 尾）
// ════════════════════════════════════════════════════════════════════════════

/// RBSP 位读取器（MSB first，H.264 的比特序）。
#[derive(Clone, Debug)]
pub struct BitReader<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> BitReader<'a> {
        BitReader { data, bit_pos: 0 }
    }

    pub fn bit_pos(&self) -> usize {
        self.bit_pos
    }

    pub fn bits_left(&self) -> usize {
        self.data.len().saturating_mul(8).saturating_sub(self.bit_pos)
    }

    /// 读 1 位。越界返回 `None`（不 panic，不补零）。
    pub fn read_bit(&mut self) -> Option<u8> {
        let byte = self.bit_pos >> 3;
        let off = self.bit_pos & 7;
        let b = self.data.get(byte)?;
        self.bit_pos += 1;
        Some((b >> (7 - off)) & 1)
    }

    /// 读 `n` 位无符号（`n > 32` 拒绝）。
    pub fn read_bits(&mut self, n: u32) -> Option<u32> {
        if n == 0 {
            return Some(0);
        }
        if n > 32 {
            return None;
        }
        let mut v: u32 = 0;
        let mut i = 0u32;
        while i < n {
            let b = self.read_bit()?;
            v = (v << 1) | (b as u32);
            i += 1;
        }
        Some(v)
    }

    /// 读无符号 Exp-Golomb `ue(v)`。
    ///
    /// 前导零计数受`limits.max_exp_golomb_leading` 约束——`00…0` 后必须跟一个 1，
    /// 恶意构造的全零位流会在这里被截断而不是把 `codeNum` 算到溢出。
    pub fn read_ue(&mut self, limits: &H264Limits) -> R<u32> {
        let mut lz = 0u32;
        loop {
            let b = self.read_bit().ok_or_else(|| {
                Failure::new(
                    DiagCode::BitstreamTruncated,
                    format!("ue(v) 前导零读取越界（已读 {} 位前导零）", lz),
                    "位流在 Exp-Golomb 前导阶段结束；拒绝解析（截断或畸形）",
                )
            })?;
            if b == 1 {
                break;
            }
            lz += 1;
            if lz > limits.max_exp_golomb_leading {
                return Err(Failure::new(
                    DiagCode::ExpGolombTooLong,
                    format!("ue(v) 前导零 {} 超过上限 {}", lz, limits.max_exp_golomb_leading),
                    "codeNum 炸弹（全零位流）；拒绝解析（F1122 计数纪律）",
                ));
            }
        }
        let suffix = self.read_bits(lz).ok_or_else(|| {
            Failure::new(
                DiagCode::BitstreamTruncated,
                format!("ue(v) 后缀 {} 位读取越界", lz),
                "位流在后缀阶段结束；拒绝解析（截断或畸形）",
            )
        })?;
        let base = (1u64 << lz)
            .checked_sub(1)
            .ok_or_else(|| overflow_failure("ue(v) codeNum 基值"))?;
        match base.checked_add(suffix as u64) {
            Some(v) if v <= u32::MAX as u64 => Ok(v as u32),
            _ => Err(overflow_failure("ue(v) codeNum")),
        }
    }

    /// 读有符号 Exp-Golomb `se(v)`。
    pub fn read_se(&mut self, limits: &H264Limits) -> R<i32> {
        let k = self.read_ue(limits)?;
        let kk = k as i64;
        // k 为偶 → -(k/2)；k 为奇 → (k+1)/2。
        let v = if kk & 1 == 1 { (kk + 1) / 2 } else { -(kk / 2) };
        if v > i32::MAX as i64 || v < i32::MIN as i64 {
            return Err(overflow_failure("se(v) 值"));
        }
        Ok(v as i32)
    }

    /// `more_rbsp_data()`：`rbsp_trailing_bits()` 之前的位是否还有数据。
    ///
    /// 规范 7.2 的定义是「当前位之后还存在**不是 `rbsp_trailing_bits()`** 的
    /// 数据」。`rbsp_trailing_bits()` 是一个 `1` 后接任意多个 `0`，所以判定
    /// 方式必须是：**从数据末尾往回找到最后一个 `1`，那一位就是 stop bit；
    /// 它之前还有位 → true**。
    ///
    /// 常见错误实现（第一版就踩了）：拿「最后一个**非零字节**的末位」当数据
    /// 末尾。可是 `rbsp_stop_one_bit` 本身就在某个非零字节里——当最后一个
    /// 语法字段的值为 0 时（比如 `redundant_pic_cnt_present_flag = 0`），
    /// 非零字节的末位就是 stop bit + 1，于是 `bit_pos < end_bit` 恒成立，
    /// 解析器会继续去读**根本不存在的** PPS 尾部字段，得到
    /// `BITSTREAM_TRUNCATED`。逐位扫描 stop bit 才能区分这两种情形。
    pub fn more_rbsp_data(&self) -> bool {
        let total_bits = self.data.len().saturating_mul(8);
        if self.bit_pos >= total_bits {
            return false;
        }
        // 从末尾往回找最低的置位 —— 那就是 rbsp_stop_one_bit。
        for p in (0..total_bits).rev() {
            let byte = match self.data.get(p / 8) {
                Some(v) => *v,
                None => continue,
            };
            if byte & (1u8 << (7 - (p % 8))) != 0 {
                // stop bit 在 p；它之前还有位才说明尾部之后仍有数据。
                return self.bit_pos < p;
            }
        }
        // 全零数据：没有 stop bit，按规范视为无更多数据（尾部全零是填充）。
        false
    }

    /// `byte_alignment()`：跳到下一个字节边界（等价 `1` 后接全0 的简化处理）。
    pub fn byte_align(&mut self) {
        let rem = self.bit_pos & 7;
        if rem != 0 {
            self.bit_pos += 8 - rem;
        }
    }

    /// `rbsp_stop_one_bit` + `rbsp_alignment_zero_bit` 是否成立（诊断用）。
    pub fn trailing_bits_ok(&self) -> bool {
        // 最低的 1 之后必须全为 0。
        let total_bits = self.data.len().saturating_mul(8);
        let mut seen_one = false;
        let mut p = total_bits;
        while p > self.bit_pos {
            p -= 1;
            let byte = p >> 3;
            let off = p & 7;
            let b = match self.data.get(byte) {
                Some(v) => *v,
                None => return false,
            };
            let bit = (b >> (7 - off)) & 1;
            if bit == 1 {
                if seen_one {
                    return false;
                }
                seen_one = true;
            } else if !seen_one {
                // stop bit 之前不能有 0 之外的位（对齐零位在stop bit 之后）。
                return false;
            }
        }
        seen_one
    }
}

/// 位写入器（[`BitReader`] 的逆运算）。
///
/// **为什么解码器需要它**：构造合规码流是解码器的基本能力——
/// 参数集登记、参考表改写、几何往返验证都要能「写出一条规范允许的流」。
/// 缺了它，畸形语料就只能靠手摆字节，而手摆字节既易错位、
/// 又无法把「只坏一个字段」这件事表达清楚。
///
/// 位序与 [`BitReader`] 严格镜像：MSB 先行。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BitWriter {
    buf: Vec<u8>,
    /// 当前字节已填位数（0..=8；`8` 表示正处在字节边界，下一个bit 开新字节）。
    filled: u32,
}

impl BitWriter {
    pub fn new() -> BitWriter {
        BitWriter { buf: Vec::new(), filled: 0 }
    }

    /// 写 1 位。
    pub fn bit(&mut self, v: u8) {
        if self.filled == 0 {
            self.buf.push(0);
        }
        let idx = self.buf.len() - 1;
        if v & 1 != 0 {
            self.buf[idx] |= 0x80 >> self.filled;
        }
        self.filled += 1;
        if self.filled == 8 {
            // 填满即封口：下一枚 bit 会走上面的 `filled == 0` 分支开新字节。
            self.filled = 0;
        }
    }

    /// 写 `n` 位无符号（`n > 32` 拒绝并返回 `false`）。
    pub fn bits(&mut self, v: u32, n: u32) -> bool {
        if n > 32 {
            return false;
        }
        let mut i = n;
        while i > 0 {
            i -= 1;
            self.bit(((v >> i) & 1) as u8);
        }
        true
    }

    /// 写 Exp-Golomb `ue(v)`（规范 9.1）。
    ///
    /// 编码长度 `len = floor(log2(v+1))`：前导 `len` 个 0 + 一个 1，
    /// 后跟 `len` 位后缀 `v - (2^len - 1)`。
    /// 长度超 32 位返回 `false`（与 [`BitReader::read_ue`] 的上限对齐）。
    pub fn ue(&mut self, v: u32) -> bool {
        // 规范 9.1：`len = floor(log2(v+1))`。
        // 用 `leading_zeros` 算就得写成 `63 - lz(v+1)`——
        // **`64 - lz` 是差一位的错式**：`v=0` 时 `lz(1)=63`，
        // `64-63=1` 而正确答案是 0，于是 `plus - (1<<1) = 1-2`
        // 在 debug 下直接下溢 panic。三个边界自查：
        //   v=0 → plus=1, lz=63, len=0（单比特 `1`）
        //   v=1 → plus=2, lz=62, len=1（`010`）
        //   v=3 → plus=4, lz=61, len=2（`00100`）
        // 全程 u64 并显式标注类型：字面量 `63` 若被推成 i32，
        // 移位量会按 i32 语义检查，症状同样是「subtract with overflow」。
        let plus: u64 = v as u64 + 1;
        let len: u32 = 63 - plus.leading_zeros(); // 0..=32
        if len > 32 {
            return false;
        }
        for _ in 0..len {
            self.bit(0);
        }
        self.bit(1);
        let suffix: u64 = plus - (1u64 << len); // v - (2^len - 1)
        self.bits(suffix as u32, len)
    }

    /// 写 Exp-Golomb `se(v)`（规范 9.1.1）。
    ///
    /// 映射：`v <= 0` 时 `k = -2v`，`v > 0` 时 `k = 2v - 1`，
    /// 再按 `ue(v)` 写出 `k`。
    /// `|k| > u32::MAX`（即 `i32::MIN`）返回 `false`——
    /// 那需要 33 位后缀，本模块的 `ue(v)` 上限是 32 位。
    pub fn se(&mut self, v: i32) -> bool {
        // 一律在 i64 上算：`-2 * (i32::MIN as i64)` = 2^32，不溢出 i64，
        // 再由下面的范围检查显式拒绝。直接在 i32 上 `-2 * v` 会 panic。
        let k: i64 = if v <= 0 {
            -2 * (v as i64)
        } else {
            2 * (v as i64) - 1
        };
        if k < 0 || k > u32::MAX as i64 {
            return false;
        }
        self.ue(k as u32)
    }

    /// 写 `rbsp_stop_one_bit` + 零位对齐（规范 7.2）。
    pub fn trailing(&mut self) {
        self.bit(1);
        while self.filled != 0 {
            self.bit(0);
        }
    }

    /// 取出 RBSP 字节。
    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}

// ════════════════════════════════════════════════════════════════════════════
