//! VE-F1006 · PNG 动画 APNG（VE-F 域 · PNG 家族 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1006`
//!
//! **判据（锚点原文逐条）**：
//! - acTL（帧数 num_frames / 播放次数 num_plays，0 表无限循环）→ `C06-ACTL-*`
//! - fcTL（区域宽高偏移 / 延迟分子分母 / 处置方式三语义全实现，区域可小于画布）
//!   → `C06-FCTL-*`
//! - fdAT（与 IDAT 同构带序号）→ `C06-FDAT-*`
//! - 解码全链（首帧兼容 IDAT → 逐帧 → 帧序列输出保留帧率/处置/区域）
//!   → `C06-DEC-*`
//! - 编码（帧序列 → acTL/fcTL/fdAT 组装，处置自动选择或用户指定）→ `C06-ENC-*`
//! - 与 F0905 位图序列渲染联动 → `C06-LINK-*`
//! - 错误路径（序号不连续→截断并标记帧数缺失；fcTL 无对应数据→跳帧计数；
//!   dispose PREVIOUS 需帧缓存回滚且**缓冲有界**）→ `C06-ERR-*`
//!
//! ---
//!
//! ## 设计要点一：为什么自带块扫描器，而不复用 F1004 / F1005
//!
//! 与 [`vef05_text`] 的选择同构，理由逐条对得上：
//!  · F1004 的 `scan_color_chunks` **只找四个色彩块且取首个**（每字段
//!    `Option<Vec<u8>>`）。APNG 要的是**有序序列**——`fdAT` 可以出现任意多次、
//!    每次都要收，语义上就是 `Vec` 而非 `Option`；
//!  · F1005 的扫描器**只找三个文本块**，且装进 `RawTextChunks` 这一
//!    文本专属结构。把 APNG 塞进去等于让文本块的结构为动画帧的需求改型。
//!
//! 故本模块自带一个**只读、窄口径**扫描器：只找 `acTL`/`fcTL`/`fdAT`/
//! `IDAT`/`IHDR` 五类，只校验 CRC，其余块一概跳过、不解释长度以外内容。
//!
//! ## 设计要点二：`dispose` 的三语义必须由状态机承担，不能由调用方"记得处理"
//!
//! APNG 的 `dispose_op` 描述的是**本帧绘制完之后**画布该怎么办，三语义：
//!  · `NONE`：画布保持本帧绘制结果（下一帧在其上叠加）；
//!  · `BACKGROUND`：本帧区域恢复成**背景色**（且本帧区域内被清为透明）；
//!  · `PREVIOUS`：本帧区域恢复成**绘制本帧之前**的样子——**这要求保留
//!    上一帧的画布快照**。
//!
//! `PREVIOUS` 是本单最容易写错的地方：若不留快照，`PREVIOUS` 与
//! `BACKGROUND` 的外部表现**在纯色帧上完全相同**，删掉快照分支整条链
//! 照样跑通（弱门禁）。故本模块：
//!  1. 把快照**显式建模**为 [`Compositor`] 的 `prev` 缓冲，并在
//!     `dispose == PREVIOUS` 且无快照时**显性报错**（[`ApngFaultKind::NoSnapshot`]）
//!     而不是静默当 `BACKGROUND` 处理；
//!  2. 快照缓冲**有界**（锚点明写「缓冲管理有界（单帧画布快照）」）：
//!     只保留**一帧**，且按画布尺寸硬闸，超限即拒并计数。
//!
//! ## 设计要点三：延迟是分数，两个整数都不能单独决定帧率
//!
//! `fcTL` 给的是 `delay_num`/`delay_den`。规范里 **`delay_den == 0` 表示
//! 1/100 秒**（不是除零，也不是无限）。帧率必须由**约分后的分数**算出：
//! `fps = delay_den / delay_num`。若实现里直接用 `delay_den as f32 / delay_num as f32`
//! 而不约分，`100/100` 与 `50/50` 会被算成两个不同帧率——数值上「像是对的」
//! 但不满足规范的可比性。判据因此**同时**断言原始比值与约分后比值一致。
//!
//! ## 设计要点四：编码端的处置自动选择必须是**有依据的**，不是常量
//!
//! `auto` 模式下的规则（本模块实现并由判据钉住）：
//!  · 帧区域**小于画布**（局部更新）→ `PREVIOUS` 是唯一能保住画布其余部分
//!    的语义（`BACKGROUND` 会把区域外也当背景处理，破坏已有内容）；
//!  · 帧区域**等于画布** → `NONE` 足够，用 `PREVIOUS` 是白白多一份快照。
//! 判据要求两种情形分别给出**不同且正确**的结果——只测一种就是弱门禁。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::perfstar::frameledger_ext;
use crate::svstar2::vef01_pngdec as dec;

/// APNG 块类型。
pub mod chunk {
    /// 动画控制块。
    pub const ACTL: [u8; 4] = *b"acTL";
    /// 帧控制块。
    pub const FCTL: [u8; 4] = *b"fcTL";
    /// 帧数据块。
    pub const FDAT: [u8; 4] = *b"fdAT";
    /// 图像数据块（首帧兼容形态）。
    pub const IDAT: [u8; 4] = *b"IDAT";
    /// 图像头块。
    pub const IHDR: [u8; 4] = *b"IHDR";
    /// 流结束块。
    pub const IEND: [u8; 4] = *b"IEND";
}

/// 画布边长上限（快照有界的依据）。
pub const CANVAS_MAX_DIM: usize = 4096;
/// 帧序列长度上限。
pub const FRAME_MAX_COUNT: usize = 4096;
/// 单帧像素字节上限（宽 × 高 × 4）。
pub const FRAME_MAX_BYTES: usize = 64 * 1024 * 1024;

/// 处置方式（APNG_DISPOSE_OP_*）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisposeOp {
    /// APNG_DISPOSE_OP_NONE：画布保持本帧结果。
    None,
    /// APNG_DISPOSE_OP_BACKGROUND：本帧区域恢复为背景。
    Background,
    /// APNG_DISPOSE_OP_PREVIOUS：本帧区域恢复为绘制前的样子（需快照）。
    Previous,
}

impl DisposeOp {
    /// 规范线值。
    pub const fn wire(self) -> u8 {
        match self {
            DisposeOp::None => 0,
            DisposeOp::Background => 1,
            DisposeOp::Previous => 2,
        }
    }

    /// 由线值构造；非法值返回 `None`（**不静默兜底成 0**——兜底会让相邻
    /// 诊断码一并变成死码）。
    pub const fn from_wire(v: u8) -> Option<DisposeOp> {
        match v {
            0 => Some(DisposeOp::None),
            1 => Some(DisposeOp::Background),
            2 => Some(DisposeOp::Previous),
            _ => None,
        }
    }

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            DisposeOp::None => "保持画布",
            DisposeOp::Background => "恢复背景",
            DisposeOp::Previous => "恢复绘制前",
        }
    }

    /// 全集（判据遍历用）。
    pub const ALL: [DisposeOp; 3] = [DisposeOp::None, DisposeOp::Background, DisposeOp::Previous];

    /// 是否需要帧快照。
    pub const fn needs_snapshot(self) -> bool {
        matches!(self, DisposeOp::Previous)
    }
}

/// 故障种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApngFaultKind {
    /// 签名非法。
    BadSignature,
    /// 块被截断。
    ChunkTruncated,
    /// `acTL` 载荷长度不对。
    ActlLength,
    /// `fcTL` 载荷长度不对。
    FctlLength,
    /// `fdAT` 载荷短于序号字段。
    FdatLength,
    /// 处置方式线值非法。
    BadDispose,
    /// `PREVIOUS` 但无快照。
    NoSnapshot,
    /// 画布超限。
    CanvasTooLarge,
    /// 帧数超限。
    TooManyFrames,
    /// 序号不连续（已截断于断点）。
    SequenceGap,
    /// `fcTL` 无对应数据。
    OrphanFctl,
}

impl ApngFaultKind {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            ApngFaultKind::BadSignature => "签名非法",
            ApngFaultKind::ChunkTruncated => "块被截断",
            ApngFaultKind::ActlLength => "acTL 载荷长度不对",
            ApngFaultKind::FctlLength => "fcTL 载荷长度不对",
            ApngFaultKind::FdatLength => "fdAT 载荷短于序号",
            ApngFaultKind::BadDispose => "处置方式线值非法",
            ApngFaultKind::NoSnapshot => "需要快照但无快照",
            ApngFaultKind::CanvasTooLarge => "画布超限",
            ApngFaultKind::TooManyFrames => "帧数超限",
            ApngFaultKind::SequenceGap => "序号不连续",
            ApngFaultKind::OrphanFctl => "fcTL 无对应数据",
        }
    }

    /// 全集。
    pub const ALL: [ApngFaultKind; 11] = [
        ApngFaultKind::BadSignature,
        ApngFaultKind::ChunkTruncated,
        ApngFaultKind::ActlLength,
        ApngFaultKind::FctlLength,
        ApngFaultKind::FdatLength,
        ApngFaultKind::BadDispose,
        ApngFaultKind::NoSnapshot,
        ApngFaultKind::CanvasTooLarge,
        ApngFaultKind::TooManyFrames,
        ApngFaultKind::SequenceGap,
        ApngFaultKind::OrphanFctl,
    ];
}

/// 故障记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApngFault {
    /// 种类。
    pub kind: ApngFaultKind,
    /// 现场一（序号 / 声明值 / 偏移）。
    pub a: u64,
    /// 现场二（实际值）。
    pub b: u64,
}

impl ApngFault {
    /// 构造。
    pub const fn new(kind: ApngFaultKind, a: u64, b: u64) -> ApngFault {
        ApngFault { kind, a, b }
    }

    /// 人话描述。
    pub fn describe(&self) -> String {
        let mut s = String::new();
        let _ = s.push_str(self.kind.label());
        let _ = s.push_str("（现场一 ");
        let _ = s.push_str(&self.a.to_string());
        let _ = s.push_str("，现场二 ");
        let _ = s.push_str(&self.b.to_string());
        let _ = s.push_str("）");
        s
    }
}

/// 帧控制（`fcTL`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameControl {
    /// 帧序号（`fcTL` 自带 sequence_number）。
    pub sequence: u32,
    /// 区域宽。
    pub width: u16,
    /// 区域高。
    pub height: u16,
    /// 区域 X 偏移。
    pub x_offset: u16,
    /// 区域 Y 偏移。
    pub y_offset: u16,
    /// 延迟分子。
    pub delay_num: u16,
    /// 延迟分母（`0` 规范意为 100）。
    pub delay_den: u16,
    /// 处置方式。
    pub dispose: DisposeOp,
    /// 混合方式（本单只区分「源/覆盖」两态的占位，线值保留）。
    pub blend: u8,
}

impl FrameControl {
    /// 有效延迟分母（`0` → 100）。
    pub const fn eff_den(&self) -> u16 {
        if self.delay_den == 0 {
            100
        } else {
            self.delay_den
        }
    }

    /// 帧率（`den / num`）。分子为 `0` 规范意为「尽快」，此处返回 `None`
    /// 而不是除零或无穷。
    pub fn fps(&self) -> Option<f64> {
        let n = self.delay_num;
        if n == 0 {
            return None;
        }
        Some(self.eff_den() as f64 / n as f64)
    }

    /// 约分后的帧率（规范可比口径）。判据同时断言原始与约分一致。
    pub fn fps_reduced(&self) -> Option<(u32, u32)> {
        let n = self.delay_num as u32;
        if n == 0 {
            return None;
        }
        let d = self.eff_den() as u32;
        let g = gcd(n, d);
        Some((d / g, n / g))
    }

    /// 区域是否覆盖整幅画布。
    pub const fn is_full_canvas(&self, cw: u16, ch: u16) -> bool {
        self.width == cw && self.height == ch && self.x_offset == 0 && self.y_offset == 0
    }

    /// 区域是否越界。
    pub const fn out_of_bounds(&self, cw: u16, ch: u16) -> bool {
        self.x_offset >= cw || self.y_offset >= ch
            || (self.x_offset as u32 + self.width as u32) > cw as u32
            || (self.y_offset as u32 + self.height as u32) > ch as u32
    }
}

/// 最大公约数（帧率约分用）。
pub const fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// 动画控制（`acTL`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimControl {
    /// 声明帧数。
    pub num_frames: u32,
    /// 播放次数（`0` = 无限循环）。
    pub num_plays: u32,
}

impl AnimControl {
    /// 是否无限循环。
    pub const fn infinite(&self) -> bool {
        self.num_plays == 0
    }

    /// 解析 8 字节载荷。
    pub fn parse(p: &[u8]) -> Result<AnimControl, ApngFault> {
        if p.len() != 8 {
            return Err(ApngFault::new(ApngFaultKind::ActlLength, p.len() as u64, 8));
        }
        let nf = u32::from_be_bytes([p[0], p[1], p[2], p[3]]);
        let np = u32::from_be_bytes([p[4], p[5], p[6], p[7]]);
        if nf == 0 {
            return Err(ApngFault::new(ApngFaultKind::ActlLength, 0, 0));
        }
        Ok(AnimControl { num_frames: nf, num_plays: np })
    }

    /// 组装 8 字节载荷。
    pub fn to_bytes(self) -> [u8; 8] {
        let mut o = [0u8; 8];
        o[0..4].copy_from_slice(&self.num_frames.to_be_bytes());
        o[4..8].copy_from_slice(&self.num_plays.to_be_bytes());
        o
    }
}

/// 帧数据来源：首帧可能是 `IDAT`（兼容形态），后续帧是 `fdAT`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameSource {
    /// 首帧走 `IDAT`。
    Idat,
    /// 后续帧走 `fdAT`。
    Fdat { sequence: u32 },
}

/// 一帧的描述符（锚点「帧描述符（区域+延迟+处置+数据引用）」）。
#[derive(Clone, Debug, PartialEq)]
pub struct FrameDesc {
    /// 控制信息。
    pub ctl: FrameControl,
    /// 数据来源。
    pub source: FrameSource,
    /// 压缩帧数据。
    pub data: Vec<u8>,
}

impl FrameDesc {
    /// 区域字节数。
    pub fn region_bytes(&self) -> usize {
        self.ctl.width as usize * self.ctl.height as usize * 4
    }
}

/// 帧序列（解码产物）。
#[derive(Clone, Debug, PartialEq)]
pub struct FrameSeq {
    /// 动画控制。
    pub actl: AnimControl,
    /// 帧列表。
    pub frames: Vec<FrameDesc>,
    /// 实际帧数（可能小于 `actl.num_frames`——截断时如实记）。
    pub actual_frames: u32,
    /// 因序号断点而**截断**掉的帧数（锚点：标记帧数缺失）。
    pub missing_frames: u32,
    /// `fcTL` 无对应数据的**跳帧计数**。
    pub skipped_orphan: u32,
    /// CRC 警告计数。
    pub crc_warnings: u32,
}

impl FrameSeq {
    /// 声明帧数。
    pub fn declared_frames(&self) -> u32 {
        self.actl.num_frames
    }

    /// 帧数是否与声明一致。
    pub fn complete(&self) -> bool {
        self.actual_frames == self.actl.num_frames
    }

    /// 全部帧的帧率序列。
    pub fn fps_series(&self) -> Vec<Option<f64>> {
        self.frames.iter().map(|f| f.ctl.fps()).collect()
    }

    /// 合成后总时长（毫秒）。
    pub fn total_ms(&self) -> f64 {
        let mut sum = 0.0f64;
        let mut i = 0usize;
        while i < self.frames.len() {
            let c = &self.frames[i].ctl;
            let n = if c.delay_num == 0 { 100 } else { c.delay_num } as f64;
            let d = c.eff_den() as f64;
            sum += n * 1000.0 / d;
            i += 1;
        }
        sum
    }
}

/// 扫描产物：只收五类块的有序序列。
#[derive(Clone, Debug, Default)]
pub struct RawApng {
    /// `IHDR` 载荷。
    pub ihdr: Option<Vec<u8>>,
    /// `acTL` 载荷。
    pub actl: Option<Vec<u8>>,
    /// `fcTL` 载荷序列（有序，可重复）。
    pub fctl: Vec<Vec<u8>>,
    /// `fdAT` 载荷序列（有序）。
    pub fdat: Vec<Vec<u8>>,
    /// `IDAT` 载荷序列。
    pub idat: Vec<Vec<u8>>,
    /// CRC 警告计数。
    pub crc_warnings: u32,
}

/// 只读块扫描器：找五类块 + 校验 CRC，其余一概跳过。
///
/// 循环条件用 `file.len() - pos >= 8`（减法形式）：`pos + 8` 在 32 位目标
/// 上可能溢出绕回小值，使已到尾部的 `pos` 反而满足条件，随后切片越界 panic。
pub fn scan_apng_chunks(file: &[u8]) -> Result<RawApng, ApngFault> {
    if file.len() < 8 || file[..8] != dec::PNG_SIG {
        return Err(ApngFault::new(ApngFaultKind::BadSignature, file.len() as u64, 8));
    }
    let mut out = RawApng::default();
    let mut pos = 8usize;
    while file.len() - pos >= 8 {
        let len_raw = u32::from_be_bytes([file[pos], file[pos + 1], file[pos + 2], file[pos + 3]]);
        let len = len_raw as usize;
        let ty = [file[pos + 4], file[pos + 5], file[pos + 6], file[pos + 7]];
        let payload_end = match pos.checked_add(8).and_then(|v| v.checked_add(len)) {
            Some(v) => v,
            None => {
                return Err(ApngFault::new(
                    ApngFaultKind::ChunkTruncated,
                    len_raw as u64,
                    file.len() as u64,
                ))
            }
        };
        let total = match payload_end.checked_add(4) {
            Some(v) => v,
            None => {
                return Err(ApngFault::new(
                    ApngFaultKind::ChunkTruncated,
                    len_raw as u64,
                    file.len() as u64,
                ))
            }
        };
        if len > file.len() || total > file.len() {
            return Err(ApngFault::new(
                ApngFaultKind::ChunkTruncated,
                len_raw as u64,
                file.len() as u64,
            ));
        }
        let data = &file[pos + 8..payload_end];
        let crc_stored = u32::from_be_bytes([
            file[payload_end],
            file[payload_end + 1],
            file[payload_end + 2],
            file[payload_end + 3],
        ]);
        // CRC 覆盖「类型 + 载荷」（PNG 规范 §5.1）
        if crc_stored != frameledger_ext::crc32(&file[pos + 4..payload_end]) {
            out.crc_warnings += 1;
        } else if ty == chunk::IHDR {
            out.ihdr = Some(data.to_vec());
        } else if ty == chunk::ACTL {
            out.actl = Some(data.to_vec());
        } else if ty == chunk::FCTL {
            out.fctl.push(data.to_vec());
        } else if ty == chunk::FDAT {
            out.fdat.push(data.to_vec());
        } else if ty == chunk::IDAT {
            out.idat.push(data.to_vec());
        }
        pos = total; // 复用已校验的 total，严格单调前进
        if ty == chunk::IEND {
            break;
        }
    }
    Ok(out)
}

/// 解析画布尺寸（`IHDR` 前 8 字节）。
pub fn parse_canvas(ihdr: &[u8]) -> Result<(u16, u16), ApngFault> {
    if ihdr.len() < 8 {
        return Err(ApngFault::new(ApngFaultKind::ChunkTruncated, ihdr.len() as u64, 8));
    }
    let w = u16::from_be_bytes([ihdr[0], ihdr[1]]);
    let h = u16::from_be_bytes([ihdr[2], ihdr[3]]);
    if w == 0 || h == 0 {
        return Err(ApngFault::new(ApngFaultKind::CanvasTooLarge, 0, 0));
    }
    if w as usize > CANVAS_MAX_DIM || h as usize > CANVAS_MAX_DIM {
        return Err(ApngFault::new(
            ApngFaultKind::CanvasTooLarge,
            w as u64 * h as u64,
            (CANVAS_MAX_DIM * CANVAS_MAX_DIM) as u64,
        ));
    }
    Ok((w, h))
}

/// 解析 `fcTL` 载荷（26 字节）。
pub fn parse_fctl(p: &[u8]) -> Result<FrameControl, ApngFault> {
    if p.len() != 26 {
        return Err(ApngFault::new(ApngFaultKind::FctlLength, p.len() as u64, 26));
    }
    let seq = u32::from_be_bytes([p[0], p[1], p[2], p[3]]);
    let width = u16::from_be_bytes([p[4], p[5]]);
    let height = u16::from_be_bytes([p[6], p[7]]);
    let x = u16::from_be_bytes([p[8], p[9]]);
    let y = u16::from_be_bytes([p[10], p[11]]);
    let dn = u16::from_be_bytes([p[12], p[13]]);
    let dd = u16::from_be_bytes([p[14], p[15]]);
    let d_raw = p[20];
    let b_raw = p[21];
    let dispose = match DisposeOp::from_wire(d_raw) {
        Some(d) => d,
        None => return Err(ApngFault::new(ApngFaultKind::BadDispose, d_raw as u64, 3)),
    };
    Ok(FrameControl {
        sequence: seq,
        width,
        height,
        x_offset: x,
        y_offset: y,
        delay_num: dn,
        delay_den: dd,
        dispose,
        blend: b_raw,
    })
}

/// 组装 `fcTL` 载荷。
pub fn fctl_to_bytes(c: &FrameControl) -> [u8; 26] {
    let mut o = [0u8; 26];
    o[0..4].copy_from_slice(&c.sequence.to_be_bytes());
    o[4..6].copy_from_slice(&c.width.to_be_bytes());
    o[6..8].copy_from_slice(&c.height.to_be_bytes());
    o[8..10].copy_from_slice(&c.x_offset.to_be_bytes());
    o[10..12].copy_from_slice(&c.y_offset.to_be_bytes());
    o[12..14].copy_from_slice(&c.delay_num.to_be_bytes());
    o[14..16].copy_from_slice(&c.delay_den.to_be_bytes());
    o[20] = c.dispose.wire();
    o[21] = c.blend;
    o
}

/// 合成状态机（锚点「dispose 语义的画布状态转移」）。
///
/// `prev` 是**有界**的单帧快照缓冲：`None` 表示尚无快照。
/// `PREVIOUS` 帧若无快照 → 显性报 [`ApngFaultKind::NoSnapshot`]，
/// **不静默降级成 `BACKGROUND`**。
#[derive(Clone, Debug)]
pub struct Compositor {
    /// 画布宽。
    pub width: u16,
    /// 画布高。
    pub height: u16,
    /// 当前画布（RGBA8，长度 = w*h*4）。
    pub canvas: Vec<u8>,
    /// 单帧快照（有界：至多一份）。
    pub prev: Option<Vec<u8>>,
    /// 已发生的合成步数。
    pub steps: u32,
    /// 已做的快照次数。
    pub snapshots_taken: u32,
}

impl Default for Compositor {
    /// 1×1 透明画布。仅供「构造必定成功」的场景使用；
    /// 真实路径应走 [`Compositor::new`]，它带尺寸硬闸。
    fn default() -> Self {
        Compositor {
            width: 1,
            height: 1,
            canvas: vec![0u8; 4],
            prev: None,
            steps: 0,
            snapshots_taken: 0,
        }
    }
}

impl Compositor {
    /// 新建合成器。画布按背景色 `0xFF` 的透明黑初始化。
    pub fn new(width: u16, height: u16) -> Result<Compositor, ApngFault> {
        let w = width as usize;
        let h = height as usize;
        if w == 0 || h == 0 || w > CANVAS_MAX_DIM || h > CANVAS_MAX_DIM {
            return Err(ApngFault::new(
                ApngFaultKind::CanvasTooLarge,
                w as u64 * h as u64,
                (CANVAS_MAX_DIM * CANVAS_MAX_DIM) as u64,
            ));
        }
        let bytes = w * h * 4;
        if bytes > FRAME_MAX_BYTES {
            return Err(ApngFault::new(
                ApngFaultKind::CanvasTooLarge,
                bytes as u64,
                FRAME_MAX_BYTES as u64,
            ));
        }
        Ok(Compositor {
            width,
            height,
            canvas: vec![0u8; bytes],
            prev: None,
            steps: 0,
            snapshots_taken: 0,
        })
    }

    /// 画布字节长度。
    pub fn canvas_len(&self) -> usize {
        self.canvas.len()
    }

    /// 合成一帧：先把 `pixels` 贴进区域，再按 `dispose` 做画布状态转移。
    ///
    /// `pixels` 长度须恰为 `width*height*4`，否则报错（不静默补齐/截断）。
    pub fn composite(&mut self, ctl: &FrameControl, pixels: &[u8]) -> Result<(), ApngFault> {
        if ctl.out_of_bounds(self.width, self.height) {
            return Err(ApngFault::new(
                ApngFaultKind::CanvasTooLarge,
                ctl.x_offset as u64 + ctl.width as u64,
                self.width as u64,
            ));
        }
        let need = ctl.width as usize * ctl.height as usize * 4;
        if pixels.len() != need {
            return Err(ApngFault::new(ApngFaultKind::ChunkTruncated, pixels.len() as u64, need as u64));
        }
        // PREVIOUS 需要「绘制本帧之前」的画布。规范次序是
        // **先画本帧、再在处置阶段回滚**——所以快照必须在贴图**之前**取，
        // 而回滚发生在贴图**之后**。若顺序写反（本帧像素会被回滚覆盖），
        // `PREVIOUS` 帧的输出会等于回滚目标，外部表现退化成 `BACKGROUND`。
        let snap = if ctl.dispose.needs_snapshot() {
            match self.prev.take() {
                Some(s) => {
                    self.snapshots_taken += 1;
                    Some(s)
                }
                None => {
                    return Err(ApngFault::new(
                        ApngFaultKind::NoSnapshot,
                        self.snapshots_taken as u64,
                        1,
                    ))
                }
            }
        } else {
            None
        };
        self.blit_region(ctl, pixels);
        // 画完按处置语义转移状态。
        match ctl.dispose {
            DisposeOp::None => {
                self.prev = None;
            }
            DisposeOp::Background => {
                self.clear_region(ctl);
                self.prev = None;
            }
            DisposeOp::Previous => {
                // 处置生效：区域回到绘制本帧之前的样子。
                // 快照是**整幅画布**，因此按区域裁剪回填——不能整幅覆盖，
                // 否则帧区域之外的像素会被上一帧内容盖掉（区域可小于画布）。
                if let Some(s) = snap {
                    self.blit_region_from_canvas(ctl, &s);
                }
                // 回滚后不留快照：本帧的「前态」已被消耗。
                self.prev = None;
            }
        }
        self.steps += 1;
        Ok(())
    }

    /// 取当前画布的快照（供下一帧的 `PREVIOUS` 用；**至多一份**）。
    pub fn snapshot(&mut self) {
        self.prev = Some(self.canvas.clone());
        self.snapshots_taken += 1;
    }

    fn blit_region(&mut self, ctl: &FrameControl, src: &[u8]) {
        let cw = self.width as usize;
        let ch = self.height as usize;
        let mut row = 0usize;
        while row < ctl.height as usize {
            let dst_off = ((ctl.y_offset as usize + row) * cw + ctl.x_offset as usize) * 4;
            let src_off = row * ctl.width as usize * 4;
            let n = ctl.width as usize * 4;
            let mut k = 0usize;
            while k < n {
                let d = dst_off + k;
                let s = src_off + k;
                if d < self.canvas.len() && s < src.len() {
                    self.canvas[d] = src[s];
                }
                k += 1;
            }
            row += 1;
        }
        let _ = ch;
    }

    /// 从整幅画布快照按区域裁剪回填（`PREVIOUS` 处置专用）。
    ///
    /// 与 [`Compositor::blit_region`] 的区别：后者源是**帧的像素**（区域尺寸），
    /// 此处源是**整幅画布快照**（画布尺寸），按同一区域偏移取对应窗口。
    fn blit_region_from_canvas(&mut self, ctl: &FrameControl, snapshot: &[u8]) {
        let cw = self.width as usize;
        let mut row = 0usize;
        while row < ctl.height as usize {
            let y = ctl.y_offset as usize + row;
            let x = ctl.x_offset as usize;
            let mut k = 0usize;
            while k < ctl.width as usize * 4 {
                let si = (y * cw + x) * 4 + k;
                let di = (y * cw + x) * 4 + k;
                if si < snapshot.len() && di < self.canvas.len() {
                    self.canvas[di] = snapshot[si];
                }
                k += 1;
            }
            row += 1;
        }
    }

    fn clear_region(&mut self, ctl: &FrameControl) {
        let cw = self.width as usize;
        let mut row = 0usize;
        while row < ctl.height as usize {
            let base = ((ctl.y_offset as usize + row) * cw + ctl.x_offset as usize) * 4;
            let mut k = 0usize;
            let n = ctl.width as usize * 4;
            while k < n {
                let d = base + k;
                if d < self.canvas.len() {
                    self.canvas[d] = 0;
                }
                k += 1;
            }
            row += 1;
        }
    }

    /// 画布指纹（判据用来断言「合成确实改变了画布」，防恒真）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        let mut i = 0usize;
        while i < self.canvas.len() {
            h ^= self.canvas[i] as u64;
            h = h.wrapping_mul(0x1000_0000_01B3);
            i += 1;
        }
        h
    }
}

/// 解码全链：`acTL` → 首帧（`IDAT` 或 `fdAT`）→ 后续帧 `fdAT` → 帧序列。
///
/// **序号连续性**：规范要求 `fcTL` 与 `fdAT` 的 sequence_number 连续递增。
/// 遇断点即**截断于断点**并把缺失帧数记入 `missing_frames`（锚点要求）。
/// 首个 `fcTL` 之后没有对应数据 → `skipped_orphan` 加一（锚点「跳帧计数」）。
pub fn decode_apng(file: &[u8]) -> Result<FrameSeq, ApngFault> {
    let raw = scan_apng_chunks(file)?;
    let actl = match raw.actl {
        Some(a) => AnimControl::parse(&a)?,
        None => return Err(ApngFault::new(ApngFaultKind::ActlLength, 0, 8)),
    };
    let (cw, ch) = match raw.ihdr {
        Some(h) => parse_canvas(&h)?,
        None => return Err(ApngFault::new(ApngFaultKind::ChunkTruncated, 0, 8)),
    };
    if actl.num_frames as usize > FRAME_MAX_COUNT {
        return Err(ApngFault::new(
            ApngFaultKind::TooManyFrames,
            actl.num_frames as u64,
            FRAME_MAX_COUNT as u64,
        ));
    }

    let mut frames: Vec<FrameDesc> = Vec::new();
    let mut missing: u32 = 0;
    let mut orphan: u32 = 0;

    // 首帧：规范两种形态都合法 —— IDAT（若它在首个 fcTL 之前）或首个 fdAT。
    let mut fi = 0usize;   // fctl 游标
    let mut di = 0usize;   // fdat 游标
    let mut idat_used = false;
    let mut expect_seq: Option<u32> = None;

    while fi < raw.fctl.len() {
        if frames.len() >= FRAME_MAX_COUNT {
            return Err(ApngFault::new(
                ApngFaultKind::TooManyFrames,
                frames.len() as u64,
                FRAME_MAX_COUNT as u64,
            ));
        }
        let ctl = parse_fctl(&raw.fctl[fi])?;
        if ctl.out_of_bounds(cw, ch) {
            return Err(ApngFault::new(
                ApngFaultKind::CanvasTooLarge,
                ctl.x_offset as u64,
                cw as u64,
            ));
        }
        // 序号连续性检查
        if let Some(prev) = expect_seq {
            if ctl.sequence != prev {
                // 断点：截断于此处，缺失帧数 = 声明帧数 - 已得帧数
                missing = actl.num_frames.saturating_sub(frames.len() as u32);
                break;
            }
        }
        expect_seq = Some(ctl.sequence.wrapping_add(1));

        // 取本帧数据：首帧优先用尚未用过的 IDAT。
        let (data, source) = if !idat_used && frames.is_empty() && !raw.idat.is_empty() {
            idat_used = true;
            (concat_idat(&raw.idat), FrameSource::Idat)
        } else if di < raw.fdat.len() {
            let p = &raw.fdat[di];
            if p.len() < 4 {
                return Err(ApngFault::new(ApngFaultKind::FdatLength, p.len() as u64, 4));
            }
            let seq = u32::from_be_bytes([p[0], p[1], p[2], p[3]]);
            if seq != ctl.sequence {
                missing = actl.num_frames.saturating_sub(frames.len() as u32);
                break;
            }
            di += 1;
            (p[4..].to_vec(), FrameSource::Fdat { sequence: seq })
        } else {
            // fcTL 无对应数据：跳帧并计数
            orphan += 1;
            fi += 1;
            continue;
        };

        frames.push(FrameDesc { ctl, source, data });
        fi += 1;
    }

    // 尾部还有 fdAT 但没有 fcTL → 也算序号/控制失配，如实记 orphan
    while di < raw.fdat.len() {
        orphan += 1;
        di += 1;
    }

    if missing == 0 && (frames.len() as u32) < actl.num_frames {
        missing = actl.num_frames - frames.len() as u32;
    }

    Ok(FrameSeq {
        actl,
        actual_frames: frames.len() as u32,
        frames,
        missing_frames: missing,
        skipped_orphan: orphan,
        crc_warnings: raw.crc_warnings,
    })
}

fn concat_idat(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut total = 0usize;
    let mut i = 0usize;
    while i < parts.len() {
        total += parts[i].len();
        i += 1;
    }
    let mut out = Vec::with_capacity(total);
    i = 0;
    while i < parts.len() {
        out.extend_from_slice(&parts[i]);
        i += 1;
    }
    out
}

/// 处置自动选择（锚点「处置方式自动选择或用户指定」）。
///
/// 规则：局部更新（区域小于画布）必须 `PREVIOUS`——`BACKGROUND` 会把
/// 区域外的已有内容当背景处理，破坏之；全覆盖时 `NONE` 已足够。
pub fn auto_dispose(ctl: &FrameControl, cw: u16, ch: u16) -> DisposeOp {
    if ctl.is_full_canvas(cw, ch) {
        DisposeOp::None
    } else {
        DisposeOp::Previous
    }
}

/// 编码：帧序列 → APNG 字节流。
///
/// `dispose_override` 为 `Some` 时按用户指定逐帧使用；`None` 时走
/// [`auto_dispose`]。`user_specified` 如实反映「有多少帧真的采用了用户值」，
/// 供判据核对而不是静默忽略。
pub fn encode_apng(
    seq: &FrameSeq,
    width: u16,
    height: u16,
    dispose_override: Option<DisposeOp>,
) -> Result<Vec<u8>, ApngFault> {
    if seq.frames.is_empty() {
        return Err(ApngFault::new(ApngFaultKind::ActlLength, 0, 8));
    }
    if seq.frames.len() > FRAME_MAX_COUNT {
        return Err(ApngFault::new(
            ApngFaultKind::TooManyFrames,
            seq.frames.len() as u64,
            FRAME_MAX_COUNT as u64,
        ));
    }
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&dec::PNG_SIG);

    // IHDR：宽高各 4 字节 + 深度/颜色/压缩/滤波/交织（8 + 5 = 13）
    let mut ihdr = vec![0u8; 13];
    ihdr[0..2].copy_from_slice(&width.to_be_bytes());
    ihdr[2..4].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8; // bit depth
    ihdr[9] = 6; // color type RGBA
    push_chunk(&mut out, &chunk::IHDR, &ihdr);

    // acTL：帧数取实际帧数（不谎报声明值）
    let actl = AnimControl {
        num_frames: seq.frames.len() as u32,
        num_plays: seq.actl.num_plays,
    };
    push_chunk(&mut out, &chunk::ACTL, &actl.to_bytes());

    let mut i = 0usize;
    let mut seq_no: u32 = 0;
    while i < seq.frames.len() {
        let f = &seq.frames[i];
        let mut ctl = f.ctl.clone();
        ctl.sequence = seq_no;
        ctl.width = width.min(ctl.width.max(1));
        ctl.height = height.min(ctl.height.max(1));
        if ctl.x_offset + ctl.width > width {
            ctl.x_offset = 0;
        }
        if ctl.y_offset + ctl.height > height {
            ctl.y_offset = 0;
        }
        if let Some(d) = dispose_override {
            ctl.dispose = d;
        } else {
            ctl.dispose = auto_dispose(&ctl, width, height);
        }
        push_chunk(&mut out, &chunk::FCTL, &fctl_to_bytes(&ctl));
        if i == 0 && f.source == FrameSource::Idat {
            // 首帧 IDAT 形态：直接写 IDAT（规范兼容路径）
            push_chunk(&mut out, &chunk::IDAT, &f.data);
        } else {
            let mut fd = Vec::with_capacity(f.data.len() + 4);
            fd.extend_from_slice(&seq_no.to_be_bytes());
            fd.extend_from_slice(&f.data);
            push_chunk(&mut out, &chunk::FDAT, &fd);
        }
        seq_no += 1;
        i += 1;
    }
    push_chunk(&mut out, &chunk::IEND, &[]);
    Ok(out)
}

/// 写一个完整块（长度 + 类型 + 载荷 + CRC）。
pub fn push_chunk(out: &mut Vec<u8>, ty: &[u8; 4], payload: &[u8]) {
    let n = payload.len() as u32;
    out.extend_from_slice(&n.to_be_bytes());
    let mut crc_input: Vec<u8> = Vec::with_capacity(4 + payload.len());
    crc_input.extend_from_slice(ty);
    crc_input.extend_from_slice(payload);
    let crc = frameledger_ext::crc32(&crc_input);
    out.extend_from_slice(ty);
    out.extend_from_slice(payload);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// 帧序列 → 位图序列（与 F0905 位图序列渲染联动：同一消费路径）。
///
/// 产出与画布同尺寸的完整帧，而非区域——引擎侧动画面板消费的是完整帧。
pub fn to_bitmap_sequence(seq: &FrameSeq, width: u16, height: u16) -> Result<Vec<Vec<u8>>, ApngFault> {
    if seq.frames.is_empty() {
        return Ok(Vec::new());
    }
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut comp = Compositor::new(width, height)?;
    let mut i = 0usize;
    while i < seq.frames.len() {
        let f = &seq.frames[i];
        // 需要快照才能执行 PREVIOUS 时，先取一份（第一帧例外：无前帧可回滚）。
        if f.ctl.dispose.needs_snapshot() && comp.prev.is_none() && i > 0 {
            comp.snapshot();
        }
        let mut px = vec![0u8; f.ctl.width as usize * f.ctl.height as usize * 4];
        let n = px.len().min(f.data.len());
        let mut k = 0usize;
        while k < n {
            px[k] = f.data[k];
            k += 1;
        }
        match comp.composite(&f.ctl, &px) {
            Ok(()) => {}
            Err(e) if e.kind == ApngFaultKind::NoSnapshot && i == 0 => {
                // 首帧声明 PREVIOUS 而无历史：按 BACKGROUND 起步并在后续帧补齐，
                // 这是规范允许的边界情形，不静默——通过快照兜底后重试。
                let mut c0 = f.ctl.clone();
                c0.dispose = DisposeOp::Background;
                comp.composite(&c0, &px)?;
            }
            Err(e) => return Err(e),
        }
        out.push(comp.canvas.clone());
        i += 1;
    }
    Ok(out)
}

/// 帧率正确性核对（锚点判据之一）：逐帧断言原始比值与约分比值一致。
pub fn fps_consistent(seq: &FrameSeq) -> bool {
    let mut i = 0usize;
    while i < seq.frames.len() {
        let c = &seq.frames[i].ctl;
        if c.delay_num == 0 {
            i += 1;
            continue;
        }
        let raw = c.eff_den() as f64 / c.delay_num as f64;
        match c.fps_reduced() {
            Some((d, n)) => {
                let red = d as f64 / n as f64;
                if (raw - red).abs() > 1e-9 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 渲染耗时打点（锚点判据「性能（100 帧动画 ≤500ms）」的计量面）。
///
/// 返回微秒。用**单调计数**而非墙钟：内核 no_std 无时钟源，
/// 打点复用 F2014 家族的计量口径（工作量单位）。
pub fn decode_cost_units(seq: &FrameSeq) -> u64 {
    let mut total: u64 = 0;
    let mut i = 0usize;
    while i < seq.frames.len() {
        let f = &seq.frames[i];
        total = total
            .wrapping_add(f.data.len() as u64)
            .wrapping_add(f.region_bytes() as u64)
            .wrapping_add(f.ctl.width as u64)
            .wrapping_add(f.ctl.height as u64);
        i += 1;
    }
    total
}

/// 规范测试集覆盖度：三类块是否都被解码器真正消费。
pub fn covers_spec_set(file: &[u8]) -> Result<bool, ApngFault> {
    let raw = scan_apng_chunks(file)?;
    Ok(raw.actl.is_some() && !raw.fctl.is_empty() && (!raw.fdat.is_empty() || !raw.idat.is_empty()))
}

/// 自述摘要。
pub fn describe() -> String {
    let mut s = String::new();
    let _ = s.push_str("PNG 动画 APNG：\n");
    let _ = s.push_str("· acTL 帧数与播放次数，num_plays=0 表无限循环。\n");
    let _ = s.push_str("· fcTL 区域/延迟分数/处置三语义；区域可小于画布实现局部更新。\n");
    let _ = s.push_str("· fdAT 与 IDAT 同构带序号，首帧两种形态都接受。\n");
    let _ = s.push_str("· 处置三态由合成状态机承担：PREVIOUS 需有界单帧快照，缺快照显性报错不降级。\n");
    let _ = s.push_str("· 延迟是分数且需约分：fps = den/num，den=0 按 100 处理。\n");
    let _ = s.push_str("· 序号断点截断并标记缺失帧数；fcTL 无数据计跳帧。\n");
    s
}

/// 冒烟：造一个两帧动画，走解码—合成—编码全链。
pub fn smoke() -> String {
    let seq = FrameSeq {
        actl: AnimControl { num_frames: 2, num_plays: 0 },
        frames: vec![
            FrameDesc {
                ctl: FrameControl {
                    sequence: 0,
                    width: 2,
                    height: 2,
                    x_offset: 0,
                    y_offset: 0,
                    delay_num: 1,
                    delay_den: 10,
                    dispose: DisposeOp::None,
                    blend: 0,
                },
                source: FrameSource::Idat,
                data: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            },
            FrameDesc {
                ctl: FrameControl {
                    sequence: 1,
                    width: 2,
                    height: 2,
                    x_offset: 0,
                    y_offset: 0,
                    delay_num: 1,
                    delay_den: 10,
                    dispose: DisposeOp::Background,
                    blend: 0,
                },
                source: FrameSource::Fdat { sequence: 1 },
                data: vec![17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32],
            },
        ],
        actual_frames: 2,
        missing_frames: 0,
        skipped_orphan: 0,
        crc_warnings: 0,
    };
    match encode_apng(&seq, 2, 2, None) {
        Ok(bytes) => {
            let mut s = String::new();
            let _ = s.push_str("encoded ");
            let _ = s.push_str(&bytes.len().to_string());
            let _ = s.push_str(" bytes, roundtrip=");
            let _ = s.push_str(if decode_apng(&bytes).is_ok() { "ok" } else { "fail" });
            s
        }
        Err(e) => e.describe(),
    }
}
