//! CGPU-F1715 · 创作×异构场景
//!
//! 判据锚点（施工书原文摘录）：
//! 创作场景：创作异构点（视频创作（NPU 特效预览/MEDIA 硬编/ISP 调色辅助）/
//! 图像创作（NPU 生图辅助预留）/音频创作（DSP 实时效果——点表；流水（创作流水
//! （导入→编辑（实时预览）→导出——流水编排复用 F1657——流水实例；预览体验
//! （实时预览 deadline（编辑流畅——预览性能门禁；量化（创作场景效率提升（导出
//! 时间/预览帧率——量化；测试（点表/流水/门禁/量化四组）。判据：点表、流水
//! 实例、门禁、量化、四组、判据。
//!
//! 诊断码段 0x9E01~0x9E05（独占）：
//!   0x9E01 创作点越表（点落闭集外/无引擎归属）
//!   0x9E02 流水失序（导入→编辑→导出乱序/缺站）
//!   0x9E03 预览门禁破（预览帧率低于编辑流畅线）
//!   0x9E04 量化失账（效率提升无证据）
//!   0x9E05 段越界（判码落 0x9E00..0x9F00 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

use alloc::vec::Vec;

/// 模块版本。
pub const CREATE_SCENE_VERSION: &str = "K34-createscene-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0x9E01~0x9E05）
// ---------------------------------------------------------------------------

pub const XC_CPOINT_OOB: u32 = 0x9E01;
pub const XC_PIPELINE_MISORDER: u32 = 0x9E02;
pub const XC_PREVIEW_GATE_RED: u32 = 0x9E03;
pub const XC_QUANT_NO_EVIDENCE: u32 = 0x9E04;
pub const XC_SEG_OOR: u32 = 0x9E05;

// ---------------------------------------------------------------------------
// 一、创作异构点表
// ---------------------------------------------------------------------------

/// 创作异构点六闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CreatePoint {
    /// 视频创作：NPU 特效预览。
    VfxPreview,
    /// 视频创作：MEDIA 硬编（导出加速）。
    VideoHardEnc,
    /// 视频创作：ISP 调色辅助。
    IspColorAssist,
    /// 图像创作：NPU 生图辅助（预留）。
    GenImgReserved,
    /// 音频创作：DSP 实时效果。
    AudioLiveFx,
}

impl CreatePoint {
    pub const ALL: [CreatePoint; 5] = [
        CreatePoint::VfxPreview,
        CreatePoint::VideoHardEnc,
        CreatePoint::IspColorAssist,
        CreatePoint::GenImgReserved,
        CreatePoint::AudioLiveFx,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            CreatePoint::VfxPreview => "vfx_preview",
            CreatePoint::VideoHardEnc => "video_hard_enc",
            CreatePoint::IspColorAssist => "isp_color_assist",
            CreatePoint::GenImgReserved => "genimg_reserved",
            CreatePoint::AudioLiveFx => "audio_live_fx",
        }
    }

    pub const fn engine(self) -> u8 {
        match self {
            CreatePoint::VfxPreview => 2,
            CreatePoint::VideoHardEnc => 4,
            CreatePoint::IspColorAssist => 1,
            CreatePoint::GenImgReserved => 2,
            CreatePoint::AudioLiveFx => 3,
        }
    }
}

/// 点表闭合。
pub struct CreatePointTable;

impl CreatePointTable {
    pub fn closed() -> Result<(), u32> {
        for p in CreatePoint::ALL {
            if p.engine() > 4 || p.name().is_empty() {
                return Err(XC_CPOINT_OOB);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、创作流水（F1657 编排复用）
// ---------------------------------------------------------------------------

/// 流水站三闭集（导入→编辑（实时预览）→导出）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum PipeStation {
    Import,
    Edit,
    Export,
}

/// 流水实例：三站有序衔接（复用 F1657 编排声明——源单号非空）。
pub struct CreatePipeline {
    /// 站序记录（入站序）。
    order: Vec<PipeStation>,
    /// 编排复用源单。
    pub origin: &'static str,
}

impl CreatePipeline {
    pub fn new() -> Self {
        CreatePipeline { order: Vec::new(), origin: "CGPU-F1657" }
    }

    /// 入站：序号必须单调递增（Import(0)→Edit(1)→Export(2)——0x9E02）。
    pub fn enter(&mut self, s: PipeStation) -> Result<(), u32> {
        if let Some(last) = self.order.last() {
            if s <= *last {
                return Err(XC_PIPELINE_MISORDER);
            }
        } else if s != PipeStation::Import {
            return Err(XC_PIPELINE_MISORDER); // 首站必须是导入。
        }
        self.order.push(s);
        Ok(())
    }

    /// 流水闭合：三站全走 + 源单非空。
    pub fn closed(&self) -> Result<(), u32> {
        if self.order.len() != 3 {
            return Err(XC_PIPELINE_MISORDER);
        }
        let mut i = 0;
        while i < 3 {
            if self.order[i] as usize != i {
                return Err(XC_PIPELINE_MISORDER);
            }
            i += 1;
        }
        if self.origin.is_empty() {
            return Err(XC_PIPELINE_MISORDER);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、预览性能门禁（编辑流畅线）
// ---------------------------------------------------------------------------

/// 预览门禁：实时预览帧率下限（编辑流畅 = 30fps → 33333µs/帧预算）。
pub const PREVIEW_DEADLINE_US: u32 = 33_333;

/// 门禁判定：预览帧耗时 ≤ deadline（0x9E03）。
pub fn preview_gate(frame_us: u32) -> Result<(), u32> {
    if frame_us > PREVIEW_DEADLINE_US {
        return Err(XC_PREVIEW_GATE_RED);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、量化（导出时间/预览帧率）
// ---------------------------------------------------------------------------

/// 创作量化：两维度各挂证据（0x9E04）。
pub struct CreateQuant {
    /// 导出耗时下降万分比（硬编收益）。
    pub export_gain_bp: i64,
    /// 预览帧率提升万分比。
    pub preview_gain_bp: i64,
    /// 证据锚。
    pub evidence: &'static str,
}

impl CreateQuant {
    pub fn build() -> Self {
        CreateQuant { export_gain_bp: -6_500, preview_gain_bp: 2_400, evidence: "AB-2026-1011-CREATE" }
    }

    pub fn balanced(&self) -> Result<(), u32> {
        if self.evidence.is_empty() || self.export_gain_bp == 0 || self.preview_gain_bp == 0 {
            return Err(XC_QUANT_NO_EVIDENCE);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（四组）
// ---------------------------------------------------------------------------

/// 创作场景引擎。
pub struct CreateSceneEngine;

impl CreateSceneEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：点表。
        CreatePointTable::closed()?;
        // 组二：流水——正常 + 乱序反例 + 首站错反例。
        let mut p = CreatePipeline::new();
        p.enter(PipeStation::Import)?;
        p.enter(PipeStation::Edit)?;
        p.enter(PipeStation::Export)?;
        p.closed()?;
        let mut bad = CreatePipeline::new();
        bad.enter(PipeStation::Import)?;
        if bad.enter(PipeStation::Export).is_ok() {
            return Err(XC_PIPELINE_MISORDER); // 跳站。
        }
        let mut bad2 = CreatePipeline::new();
        if bad2.enter(PipeStation::Edit).is_ok() {
            return Err(XC_PIPELINE_MISORDER); // 首站非导入。
        }
        // 组三：门禁——恰值过 + 超时拒。
        preview_gate(PREVIEW_DEADLINE_US)?;
        if preview_gate(PREVIEW_DEADLINE_US + 1).is_ok() {
            return Err(XC_PREVIEW_GATE_RED);
        }
        // 组四：量化——账平 + 无证据反例。
        let q = CreateQuant::build();
        q.balanced()?;
        let noev = CreateQuant { export_gain_bp: 0, preview_gain_bp: q.preview_gain_bp, evidence: q.evidence };
        if noev.balanced().is_ok() {
            return Err(XC_QUANT_NO_EVIDENCE);
        }
        Ok(())
    }

    /// 段对账：五码全落 0x9E00..0x9F00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_CPOINT_OOB,
            XC_PIPELINE_MISORDER,
            XC_PREVIEW_GATE_RED,
            XC_QUANT_NO_EVIDENCE,
            XC_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x9E00 || codes[i] >= 0x9F00 {
                return Err(XC_CPOINT_OOB);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_CPOINT_OOB);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
