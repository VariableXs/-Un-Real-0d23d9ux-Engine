//! CGPU-F2084 · 效果重建管线（CGPU-N 域 · 效果重建域 · N02 组 · 目标 340 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2084`
//!
//! **判据（锚点原文）**：四段、采样规范、验证复用、三组、判据。
//!
//! **职责定位（锚点原文）**：重建管线：管线（效果重建管线（捕获→分析→
//! 重建→验证——四段——管线规范；捕获（原生效果捕获（截图采样——采样
//! 规范；验证（重建 vs 原生 diff——验证复用家族——验证复用。
//!
//! ## 一、四段管线（判据一）
//!
//! 效果重建四段单向流（[`PipePhase`] 闭集）：捕获（原生效果取证——
//! 截图与参数快照）→ 分析（效果分解——哪几层效果叠出这个画面）→
//! 重建（在 Varix 侧用自有原语重建同效画面）→ 验证（重建 vs 原生
//! diff）。单向纪律：跳段/回退显性拒（[`RebuildPipe`]）——没捕获就
//! 重建等于凭想象画，没验证就交付等于没交付。段间传递**效果描述**不
//! 传递像素原图（像素是证据不是接口——接口是结构化的效果分解，否则
//! 重建退化成贴图拷贝）。
//!
//! ## 二、捕获采样规范（判据二）
//!
//! 截图采样三纪律（[`CaptureSpec`]）：①锚点帧选取（参数稳定后取帧，
//! 动画进行中取帧会把中间态当效果——锚点判据：连续 N 帧参数哈希
//! 不变才取样）；②采样密度（每效果至少三采样点：左上/中心/右下——
//! 单点采样分不清渐变方向）；③元数据同拍（截图与参数账同帧取——
//! 错拍的对不上账）。
//!
//! ## 三、验证复用（判据三）
//!
//! 重建 vs 原生 diff **复用** N 域验证家族（F2083 三原则的等价分级）：
//! 像素级等价（逐像素差 ≤ 容差）/ 感知等级（块级差）两级判定
//! （[`verify_rebuild`]）——本模块不另立验证口径（验证复用家族纪律），
//! 只把 diff 结论装配进管线闭环（验证不过→重建段打回，打回计数留痕）。
//!
//! **对接**：F2083（重建原则——等价分级的上游口径）；F2085+（分解器/
//! 重建器的下游深化）；M 域（合成——重建画面最终走合成上屏）。零
//! panic 面、零 IO、零墙钟、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const REBUILD_PIPE_VERSION: &str = "N02-rebuildpipe-v1";

/// 锚点帧稳定窗（连续 N 帧参数哈希不变才可取样；当前策略值，可调）。
pub const ANCHOR_STABLE_FRAMES: u32 = 8;

/// 像素级等价容差（0-255 每通道；当前策略值，可调）。
pub const PIXEL_TOLERANCE: u8 = 2;

// ---------------------------------------------------------------------------
// 二、诊断码（0x37 段独占：N02 重建管线面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x37 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 管线相位违规（跳段/回退）。
    PhaseViolation,
    /// 采样规范违反（未达稳定窗/单点采样/元数据错拍）。
    CaptureSpecViolation,
    /// 效果分解缺失（分析段没产出就进重建）。
    AnalysisMissing,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::PhaseViolation => "管线相位违规：捕获→分析→重建→验证单向推进，跳段/回退显性拒",
            XcCode::CaptureSpecViolation => "采样规范违反：未达稳定窗/单点采样/元数据错拍，按 CaptureSpec 三纪律重采",
            XcCode::AnalysisMissing => "效果分解缺失：分析段无产出不许进重建，重建不是贴图拷贝",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、四段单向流（判据一）
// ---------------------------------------------------------------------------

/// 管线相位闭集（四段单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipePhase {
    /// ①捕获（原生效果取证）。
    Capture,
    /// ②分析（效果分解）。
    Analysis,
    /// ③重建（Varix 侧重建）。
    Rebuild,
    /// ④验证（diff 闭环）。
    Verify,
    /// 完成（终态）。
    Done,
    /// 失败（终态；打回计数留痕）。
    Failed,
}

impl PipePhase {
    /// 序号（单向推进的深度轴；Failed 不占深度轴——终态判定用）。
    pub const fn depth(self) -> u8 {
        match self {
            PipePhase::Capture => 0,
            PipePhase::Analysis => 1,
            PipePhase::Rebuild => 2,
            PipePhase::Verify => 3,
            PipePhase::Done => 4,
            PipePhase::Failed => 4,
        }
    }

    /// 段名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            PipePhase::Capture => "捕获",
            PipePhase::Analysis => "分析",
            PipePhase::Rebuild => "重建",
            PipePhase::Verify => "验证",
            PipePhase::Done => "完成",
            PipePhase::Failed => "失败",
        }
    }
}

/// 效果采样点（捕获面的最小单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SamplePoint {
    /// 采样位置（归一化 ×1000：0=左/上，500=中，1000=右/下）。
    pub x_milli: u16,
    pub y_milli: u16,
    /// 该点像素 RGB（8bit×3 打包 u32）。
    pub rgb: u32,
    /// 同拍元数据哈希（与截图同帧的参数账哈希——错拍检查用）。
    pub meta_hash: u64,
}

/// 重建管线（四段单向推进 + 打回账）。
#[derive(Clone, Debug)]
pub struct RebuildPipe {
    phase: PipePhase,
    /// 稳定帧计数（捕获段的锚点判据状态）。
    stable_frames: u32,
    /// 采样点账。
    samples: Vec<SamplePoint>,
    /// 分析产出的效果分解（结构化——不是像素）。
    layers: Vec<String>,
    /// 验证打回计数。
    verify_rejects: u64,
}

impl RebuildPipe {
    /// 新管线（捕获段起步）。
    pub const fn new() -> RebuildPipe {
        RebuildPipe {
            phase: PipePhase::Capture,
            stable_frames: 0,
            samples: Vec::new(),
            layers: Vec::new(),
            verify_rejects: 0,
        }
    }

    /// 相位只读。
    pub const fn phase(&self) -> PipePhase {
        self.phase
    }

    /// 捕获段：喂帧稳定计数 + 采样点（判据二三纪律即检即拒）。
    pub fn capture(
        &mut self,
        stable_tick: bool,
        s: SamplePoint,
    ) -> Result<(), XcCode> {
        if self.phase != PipePhase::Capture {
            return Err(XcCode::PhaseViolation);
        }
        if stable_tick {
            self.stable_frames = self.stable_frames.saturating_add(1);
        } else {
            self.stable_frames = 0; // 参数漂移重数稳定窗——动画进行中不算
        }
        if self.stable_frames < ANCHOR_STABLE_FRAMES {
            return Err(XcCode::CaptureSpecViolation);
        }
        // 元数据同拍占位校验：零哈希=无元数据（错拍面）。
        if s.meta_hash == 0 {
            return Err(XcCode::CaptureSpecViolation);
        }
        self.samples.push(s);
        Ok(())
    }

    /// 采样充分性判定（三采样点：覆盖左上/中心/右下象限）。
    pub fn capture_complete(&self) -> bool {
        if self.samples.len() < 3 {
            return false;
        }
        // 象限覆盖：至少一点 x<300、一点 300..700、一点 >700（横向布点）。
        let (mut lo, mut mid, mut hi) = (false, false, false);
        let mut i = 0usize;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                if s.x_milli < 300 {
                    lo = true;
                } else if s.x_milli <= 700 {
                    mid = true;
                } else {
                    hi = true;
                }
            }
            i += 1;
        }
        lo && mid && hi
    }

    /// 推进到分析段（采样不充分显性拒）。
    pub fn advance_to_analysis(&mut self) -> Result<(), XcCode> {
        if self.phase != PipePhase::Capture {
            return Err(XcCode::PhaseViolation);
        }
        if !self.capture_complete() {
            return Err(XcCode::CaptureSpecViolation);
        }
        self.phase = PipePhase::Analysis;
        Ok(())
    }

    /// 分析段：登记效果层（结构化分解；至少一层）。
    pub fn analyze(&mut self, layer_desc: String) -> Result<(), XcCode> {
        if self.phase != PipePhase::Analysis {
            return Err(XcCode::PhaseViolation);
        }
        self.layers.push(layer_desc);
        Ok(())
    }

    /// 推进到重建段（零分解显性拒——重建不是凭空画）。
    pub fn advance_to_rebuild(&mut self) -> Result<(), XcCode> {
        if self.phase != PipePhase::Analysis {
            return Err(XcCode::PhaseViolation);
        }
        if self.layers.is_empty() {
            return Err(XcCode::AnalysisMissing);
        }
        self.phase = PipePhase::Rebuild;
        Ok(())
    }

    /// 推进到验证段（枚举比较非 const，用普通 fn）。
    pub fn advance_to_verify(&mut self) -> Result<(), XcCode> {
        if self.phase != PipePhase::Rebuild {
            return Err(XcCode::PhaseViolation);
        }
        self.phase = PipePhase::Verify;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、验证复用（判据三；等价分级复用 F2083 家族口径）
// ---------------------------------------------------------------------------

/// 等价判定两级（复用家族：像素级/感知级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquivLevel {
    /// 像素级等价（逐像素差 ≤ 容差）。
    Pixel,
    /// 感知等价（块级差在容差内、像素级未达）。
    Perceptual,
    /// 不等价（打回重建）。
    None,
}

/// 重建 vs 原生 diff（判据三的验证面：逐点比对，返回等价级别与
/// 最大差值——结论可复算）。
pub fn verify_rebuild(native: &[SamplePoint], rebuilt: &[SamplePoint]) -> (EquivLevel, u32) {
    if native.len() != rebuilt.len() || native.is_empty() {
        return (EquivLevel::None, u32::MAX);
    }
    let mut max_diff: u32 = 0;
    let mut i = 0usize;
    while i < native.len() {
        let (a, b) = match (native.get(i), rebuilt.get(i)) {
            (Some(a), Some(b)) => (a, b),
            _ => return (EquivLevel::None, u32::MAX),
        };
        // 三通道差的最大值（打包 u32：R|G|B 各 8bit）。
        let mut ch = 0u32;
        while ch < 3 {
            let shift = 16 - ch * 8;
            let ca = (a.rgb >> shift) & 0xFF;
            let cb = (b.rgb >> shift) & 0xFF;
            let d = if ca > cb { ca - cb } else { cb - ca };
            if d > max_diff {
                max_diff = d;
            }
            ch += 1;
        }
        i += 1;
    }
    if max_diff <= PIXEL_TOLERANCE as u32 {
        (EquivLevel::Pixel, max_diff)
    } else if max_diff <= (PIXEL_TOLERANCE as u32) * 8 {
        (EquivLevel::Perceptual, max_diff)
    } else {
        (EquivLevel::None, max_diff)
    }
}

impl RebuildPipe {
    /// 验证段收口（复用 diff 结论：像素级→Done；感知级→Done 但记级；
    /// 不等价→打回重建段，打回计数留痕——打回不是失败终态，可再建再验）。
    pub fn verify_and_close(&mut self, level: EquivLevel) -> Result<PipePhase, XcCode> {
        if self.phase != PipePhase::Verify {
            return Err(XcCode::PhaseViolation);
        }
        match level {
            EquivLevel::None => {
                self.verify_rejects = self.verify_rejects.saturating_add(1);
                self.phase = PipePhase::Rebuild; // 打回重建（单向轴允许段内打回重走验证）
                Ok(self.phase)
            }
            _ => {
                self.phase = PipePhase::Done;
                Ok(self.phase)
            }
        }
    }

    /// 管线摘要（人读）。
    pub fn summary(&self) -> String {
        let mut s = String::from("重建管线：段=");
        s.push_str(self.phase.zh());
        s.push_str("，采样 ");
        s.push_str(&self.samples.len().to_string());
        s.push_str("，效果层 ");
        s.push_str(&self.layers.len().to_string());
        s.push_str("，打回 ");
        s.push_str(&self.verify_rejects.to_string());
        s
    }
}

impl Default for RebuildPipe {
    fn default() -> RebuildPipe {
        RebuildPipe::new()
    }
}
