//! VE-F1204 · H.264 解码器（VE-G 域 · G01 视频解码组 · L2/L3）
//!
//! **VE-F1204 · H.264 解码器 · §11-§12 DPB 与算子计数**
//!
//! 由 `veg04_h264.rs` 按职责拆分而来（L5 子模块拆分，行为保持）。
//! 全部符号经 `veg04_h264` 的 `pub use` 再导出，对外路径不变。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::core::{DiagCode, Failure, H264Limits, R, checked_add, checked_mul, overflow_failure};
use super::ps::{ChromaFormat, Sps};
use super::pix::{Plane};

// §11 DPB（参考帧池 / 标记 / 滑动窗口 / 预算防护）
// ════════════════════════════════════════════════════════════════════════════

/// 一帧图像（Y/U/V 三平面）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Frame {
    pub luma: Plane,
    pub chroma_u: Plane,
    pub chroma_v: Plane,
    pub frame_num: u32,
    pub pic_order_cnt: i32,
    /// 是否为长期参考帧（`long_term_frame_idx`）。
    pub long_term: bool,
    pub long_term_frame_idx: u32,
    /// 是否仍被标记为「使用中」。
    pub marked_used: bool,
    /// 帧几何指纹（来自 SPS）。
    pub geometry: (u32, u32, u32, u8),
}

impl Frame {
    /// 单帧字节数（Y + U + V）。
    pub fn byte_len(&self) -> u64 {
        self.luma.byte_len() + self.chroma_u.byte_len() + self.chroma_v.byte_len()
    }
}

/// 由 SPS 推导一帧的三个平面尺寸并分配。
pub fn alloc_frame(sps: &Sps) -> R<Frame> {
    let fmt = ChromaFormat::from_idc(sps.chroma_format_idc);
    let bit_depth = 8 + sps.bit_depth_luma_minus8;
    let w = sps.width_mbs() * 16;
    let h = sps.height_mbs() * 16;
    let cw = w / fmt.sub_width_c();
    let ch = h / fmt.sub_height_c();
    let luma = Plane::new(w, h, bit_depth)?;
    // 单色格式没有色度平面（分配 0x0）。
    let (chroma_u, chroma_v) = if fmt == ChromaFormat::Monochrome {
        (Plane::new(1, 1, bit_depth)?, Plane::new(1, 1, bit_depth)?)
    } else {
        (Plane::new(cw, ch, bit_depth)?, Plane::new(cw, ch, bit_depth)?)
    };
    Ok(Frame {
        luma,
        chroma_u,
        chroma_v,
        frame_num: 0,
        pic_order_cnt: 0,
        long_term: false,
        long_term_frame_idx: 0,
        marked_used: true,
        geometry: sps.geometry_fingerprint(),
    })
}

/// 按 SPS 推算**整帧字节数**（不解码即知；DPB 预算前置校验用）。
pub fn h264_frame_bytes(sps: &Sps) -> Option<u64> {
    let fmt = ChromaFormat::from_idc(sps.chroma_format_idc);
    let w = sps.width_mbs() as u64 * 16;
    let h = sps.height_mbs() as u64 * 16;
    let cw = w / fmt.sub_width_c() as u64;
    let ch = h / fmt.sub_height_c() as u64;
    let y = match w.checked_mul(h) {
        Some(v) => v,
        None => return None,
    };
    match fmt {
        ChromaFormat::Monochrome => Some(y),
        _ => {
            let c = match cw.checked_mul(ch) {
                Some(v) => v,
                None => return None,
            };
            match y.checked_add(c) {
                Some(v1) => v1.checked_add(c),
                None => None,
            }
        }
    }
}

/// DPB 预算（与 F1213 内存纪律联动）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DpbBudget {
    /// 槽位上限。
    pub max_slots: usize,
    /// 字节上限。
    pub max_bytes: u64,
}

impl DpbBudget {
    pub const fn from_limits(limits: &H264Limits) -> DpbBudget {
        DpbBudget { max_slots: limits.max_dpb_slots, max_bytes: limits.dpb_budget_bytes }
    }
}

/// DPB（decoded picture buffer）。
#[derive(Clone, Default, Debug)]
pub struct Dpb {
    frames: Vec<Frame>,
    peak_frames: u64,
    peak_bytes: u64,
    overflow_count: u64,
    frame_bytes: u64,
}

impl Dpb {
    pub fn new() -> Dpb {
        Dpb::default()
    }

    /// 声明本 DPB 的单帧字节数（由 SPS 推出）。
    pub fn set_frame_bytes(&mut self, bytes: u64) {
        self.frame_bytes = bytes;
    }

    /// 当前占用字节数。
    pub fn used_bytes(&self) -> u64 {
        self.frames.iter().map(|f| f.byte_len()).sum()
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn peak_frames(&self) -> u64 {
        self.peak_frames
    }

    pub fn peak_bytes(&self) -> u64 {
        self.peak_bytes
    }

    pub fn overflow_count(&self) -> u64 {
        self.overflow_count
    }

    /// 按 `frame_num` 取参考帧。
    pub fn find_by_frame_num(&self, frame_num: u32) -> Option<&Frame> {
        self.frames.iter().find(|f| f.frame_num == frame_num && !f.long_term)
    }

    /// 按长期索引取参考帧。
    pub fn find_long_term(&self, idx: u32) -> Option<&Frame> {
        self.frames.iter().find(|f| f.long_term && f.long_term_frame_idx == idx)
    }

    /// **预算前置校验**：若放入一帧会超预算则拒绝（不静默驱逐）。
    pub fn push_frame(&mut self, frame: Frame, budget: &DpbBudget) -> R<()> {
        let incoming = frame.byte_len();
        if self.frames.len() >= budget.max_slots {
            self.overflow_count += 1;
            return Err(Failure::new(
                DiagCode::DpbOverflow,
                format!("DPB 槽位已满（{} / {}）", self.frames.len(), budget.max_slots),
                "DPB 槽位耗尽；拒绝放入（应由滑动窗口先驱逐），不静默丢弃参考帧",
            ));
        }
        let used = self.used_bytes();
        let projected = match checked_add(used, incoming) {
            Some(v) => v,
            None => {
                self.overflow_count += 1;
                return Err(overflow_failure("DPB 占用字节"));
            }
        };
        if projected > budget.max_bytes {
            self.overflow_count += 1;
            return Err(Failure::new(
                DiagCode::DpbOverflow,
                format!(
                    "单帧 {} 字节放入后将达 {}，超过预算 {}",
                    incoming, projected, budget.max_bytes
                ),
                "DPB 字节预算超限；拒绝放入（F1213 内存纪律联动）。恶意 SPS 声称超大分辨率时必须在此被拒，而非「尽量分配一点」",
            ));
        }
        self.peak_frames = self.peak_frames.max(self.frames.len() as u64 + 1);
        self.peak_bytes = self.peak_bytes.max(projected);
        self.frames.push(frame);
        Ok(())
    }

    /// **滑动窗口**标记（规范 8.2.5.3）：把解码顺序最旧的短周期帧标记为「不用」。
    ///
    /// 滑动窗口的语义是「标记」而非「删除」——真正删除发生在
    /// [`Dpb::remove_unmarked`]（8.2.5.4 的输出阶段）。
    pub fn apply_sliding_window(&mut self, max_num_ref_frames: u32) -> usize {
        let short_count = self
            .frames
            .iter()
            .filter(|f| !f.long_term && f.marked_used)
            .count() as u32;
        if short_count < max_num_ref_frames.max(1) {
            return 0;
        }
        // 找解码顺序最旧的「使用中」短周期帧并标记为不用。
        let oldest = self
            .frames
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.long_term && f.marked_used)
            .min_by_key(|(_, f)| (f.frame_num, f.pic_order_cnt))
            .map(|(i, _)| i);
        match oldest {
            Some(i) => {
                if let Some(f) = self.frames.get_mut(i) {
                    f.marked_used = false;
                    1
                } else {
                    0
                }
            }
            None => 0,
        }
    }

    /// **自适应标记**：MMCO 语义的最小可执行形态。
    ///
    /// - `mmco == 1`：标记 `long_term_frame_idx` 对应帧为「不用」；
    /// - `mmco == 3`：把指定长期帧改为短期（`long_term = false`）；
    /// - `mmco == 4`：清空所有长期帧标记（`max_long_term_frame_idx_plus1 == 0`）。
    ///
    /// 未登记的长期索引**不静默忽略**——返回计数并由调用方出诊断。
    /// 调度骨架只做「mmco → 动作函数」映射，各臂语义下沉到
    /// [`Dpb::unmark_long_term`] / [`Dpb::demote_long_term`] /
    /// [`Dpb::clear_long_term_marks`]。
    pub fn apply_adaptive_marking(&mut self, mmco: u8, idx: u32, max_lt: u32) -> usize {
        match mmco {
            1 => self.unmark_long_term(idx),
            3 => self.demote_long_term(idx),
            4 => self.clear_long_term_marks(max_lt),
            _ => 0,
        }
    }

    /// `mmco == 1`：标记对应长期帧为「不用」；未登记返回 0。
    fn unmark_long_term(&mut self, idx: u32) -> usize {
        match self.frames.iter_mut().find(|f| f.long_term && f.long_term_frame_idx == idx) {
            Some(f) => {
                f.marked_used = false;
                1
            }
            None => 0,
        }
    }

    /// `mmco == 3`：把指定长期帧改回短期；未登记返回 0。
    fn demote_long_term(&mut self, idx: u32) -> usize {
        match self.frames.iter_mut().find(|f| f.long_term && f.long_term_frame_idx == idx) {
            Some(f) => {
                f.long_term = false;
                f.marked_used = true;
                1
            }
            None => 0,
        }
    }

    /// `mmco == 4`：`max_lt == 0` 时清空全部长期帧的「使用中」标记并计数。
    fn clear_long_term_marks(&mut self, max_lt: u32) -> usize {
        if max_lt != 0 {
            return 0;
        }
        let mut n = 0usize;
        for f in self.frames.iter_mut() {
            if f.long_term {
                f.marked_used = false;
                n += 1;
            }
        }
        n
    }

    /// 移除所有标记为「不用」的帧（8.2.5.4）。
    pub fn remove_unmarked(&mut self) -> usize {
        let before = self.frames.len();
        self.frames.retain(|f| f.marked_used);
        before - self.frames.len()
    }

    /// 清空（几何变更时由上层显式调用）。
    pub fn flush(&mut self) -> usize {
        let n = self.frames.len();
        self.frames.clear();
        n
    }
}

// ════════════════════════════════════════════════════════════════════════════
// §12 算子工作计数（性能自检的**真实**度量，非自证式算术）
// ════════════════════════════════════════════════════════════════════════════

/// 热点算子计数。
///
/// 计数在**真实解码路径**上逐处递增——不是 `n * CONST` 的自证式算术
/// （见头注要点九）。每项对应一类真实操作，自检按项点名核对。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct WorkCounter {
    /// 六抽头滤波的抽头乘加次数（每次 6 抽头 +1）。
    pub six_tap: u64,
    /// 4x4 反变换的「矩阵乘加 + 移位」次数。
    pub idct: u64,
    /// 反量化次数。
    pub dequant: u64,
    /// 帧内预测的模式求值次数。
    pub intra_pred: u64,
    /// 去块滤波的边界强度派生次数。
    pub boundary_strength: u64,
    /// 去块滤波的单样本比较/钳位次数。
    pub deblock_ops: u64,
    /// 运动补偿的分像素采样次数。
    pub mc_samples: u64,
}

impl WorkCounter {
    pub fn new() -> WorkCounter {
        WorkCounter::default()
    }

    /// 总算子数（各项之和）。
    pub fn total(&self) -> u64 {
        self.six_tap
            + self.idct
            + self.dequant
            + self.intra_pred
            + self.boundary_strength
            + self.deblock_ops
            + self.mc_samples
    }

    /// 合并（多帧累计）。
    pub fn merge(&mut self, o: &WorkCounter) {
        self.six_tap += o.six_tap;
        self.idct += o.idct;
        self.dequant += o.dequant;
        self.intra_pred += o.intra_pred;
        self.boundary_strength += o.boundary_strength;
        self.deblock_ops += o.deblock_ops;
        self.mc_samples += o.mc_samples;
    }
}

/// 1080p30 软解的**算子预算**（本模块的诚实口径，见头注要点九）。
///
/// 推导：1080p = 8160 宏块；每宏块 16 个 4x4 块 →
/// 反变换 16×（4×4 乘加 × 2 趟 + 移位）× 3 ≈ 1024 次算子/宏块
/// → 8160 × 1024 ≈ 8.36M/帧；30fps → ≈ 250M 算子/秒。
/// 留25% 余量后取**3.4亿算子/帧**为门禁（30 帧 = 1020 帧窗口）。
///
/// **这不是 CPU 周期**：真实百分比需 F1219 以周期计数器在目标机实测。
pub const PERF_BUDGET_1080P30_OPERATORS: u64 = 340_000_000;

// ════════════════════════════════════════════════════════════════════════════
