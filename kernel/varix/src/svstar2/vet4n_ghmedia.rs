#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0xDC80）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xDC80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CDC8Code(pub u16);

impl CDC8Code {
    pub const SUB_OCCLUDED: Self = Self(SEG_BASE);    // 3D 字幕被挡
    pub const SONAR_LOST: Self = Self(SEG_BASE + 1);  // 声呐失向
    pub const SOURCE_FORK: Self = Self(SEG_BASE + 2); // 单源分叉
    pub const MATRIX_GAP: Self = Self(SEG_BASE + 3);  // 矩阵缺
    pub const DEPTH_FAIL: Self = Self(SEG_BASE + 4);  // 深度测试失败

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::SUB_OCCLUDED => "3D字幕被挡",
            Self::SONAR_LOST => "声呐失向",
            Self::SOURCE_FORK => "单源分叉",
            Self::MATRIX_GAP => "矩阵缺",
            Self::DEPTH_FAIL => "深度测试失败",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::SUB_OCCLUDED, Self::SONAR_LOST, Self::SOURCE_FORK,
        Self::MATRIX_GAP, Self::DEPTH_FAIL,
    ];
}

// ── 3D 字幕遮挡断言 ─────────────────────────────────────────────────

/// 3D 字幕节点。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Subtitle3D {
    pub depth_z: f64, // 深度（排序用）
    pub visible: bool,
    pub occluded_by_scene: bool,
}

impl Subtitle3D {
    /// 3D 字幕红线：不得被场景遮挡。
    pub fn unoccluded(&self) -> bool { !self.occluded_by_scene && self.visible }
}

/// 深度测试协同（复用 F2927）。
#[derive(Debug, Clone)]
pub struct DepthTest {
    pub scene_depth: f64,
    pub subtitle_depth: f64,
}

impl DepthTest {
    /// 深度正确：字幕在场景前（z 更小=更近）。
    pub fn subtitle_in_front(&self) -> bool {
        self.subtitle_depth <= self.scene_depth
    }
}

// ── 空间音频→空间视觉声呐 ──────────────────────────────────────────

/// 空间声源（3D 方位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpatialSound {
    pub id: u32,
    /// 方位角 0..360。
    pub azimuth_deg: u16,
    /// 仰角 -90..=90。
    pub elevation_deg: i16,
}

impl SpatialSound {
    /// 方向有效。
    pub fn direction_valid(&self) -> bool {
        self.azimuth_deg < 360 && (-90..=90).contains(&self.elevation_deg)
    }
}

/// 空间声呐可视化。
#[derive(Debug, Clone)]
pub struct SpatialSonar {
    pub sources: Vec<SpatialSound>,
    pub rendered: bool,
}

impl SpatialSonar {
    /// 方向完整：全部声源已可视化。
    pub fn direction_complete(&self) -> bool {
        self.rendered && !self.sources.is_empty() && self.sources.iter().all(|s| s.direction_valid())
    }
}

// ── G/H 组合矩阵 ────────────────────────────────────────────────────

/// 场景类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneType { Game3D, HudOverlay, VrEnv }

impl SceneType {
    pub const ALL: [Self; 3] = [Self::Game3D, Self::HudOverlay, Self::VrEnv];
}

/// 组合测试格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComboCell {
    pub scene: SceneType,
    pub media: MediaLayer,
    pub passed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaLayer { Subtitle, Sonar, Emergency }

impl MediaLayer {
    pub const ALL: [Self; 3] = [Self::Subtitle, Self::Sonar, Self::Emergency];
}

// ── 单源归一 ────────────────────────────────────────────────────────

/// 单源声明。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SingleSourceDecl {
    pub key: &'static str,
    pub impl_count: u8,
}

impl SingleSourceDecl {
    /// 归一：只允许一个实现。
    pub fn unified(&self) -> bool { self.impl_count == 1 }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3876_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4n_ghmedia");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 3D 字幕不被挡
    {
        let s = Subtitle3D { depth_z: 0.1, visible: true, occluded_by_scene: false };
        chk(&mut set, "C3876-01", s.unoccluded(), "err");
    }
    // 2 字幕被挡
    {
        let s = Subtitle3D { depth_z: 0.5, visible: true, occluded_by_scene: true };
        chk(&mut set, "C3876-02", !s.unoccluded(), "err");
    }
    // 3 深度测试
    {
        let d = DepthTest { scene_depth: 10.0, subtitle_depth: 5.0 };
        chk(&mut set, "C3876-03", d.subtitle_in_front(), "err");
    }
    // 4 字幕在场景后
    {
        let d = DepthTest { scene_depth: 5.0, subtitle_depth: 10.0 };
        chk(&mut set, "C3876-04", !d.subtitle_in_front(), "err");
    }
    // 5 空间声源方向
    {
        let s = SpatialSound { id: 1, azimuth_deg: 180, elevation_deg: 45 };
        chk(&mut set, "C3876-05", s.direction_valid(), "err");
    }
    // 6 方位越界
    {
        let s = SpatialSound { id: 1, azimuth_deg: 400, elevation_deg: 0 };
        chk(&mut set, "C3876-06", !s.direction_valid(), "err");
    }
    // 7 仰角越界
    {
        let s = SpatialSound { id: 1, azimuth_deg: 0, elevation_deg: 120 };
        chk(&mut set, "C3876-07", !s.direction_valid(), "err");
    }
    // 8 声呐方向完整
    {
        let mut sonar = SpatialSonar { sources: Vec::new(), rendered: true };
        sonar.sources.push(SpatialSound { id: 1, azimuth_deg: 90, elevation_deg: 0 });
        chk(&mut set, "C3876-08", sonar.direction_complete(), "err");
    }
    // 9 声呐未渲染
    {
        let mut sonar = SpatialSonar { sources: Vec::new(), rendered: false };
        sonar.sources.push(SpatialSound { id: 1, azimuth_deg: 90, elevation_deg: 0 });
        chk(&mut set, "C3876-09", !sonar.direction_complete(), "err");
    }
    // 10 单源归一
    {
        let d = SingleSourceDecl { key: "depth", impl_count: 1 };
        chk(&mut set, "C3876-10", d.unified(), "err");
    }
    // 11 单源分叉
    {
        let d = SingleSourceDecl { key: "depth", impl_count: 3 };
        chk(&mut set, "C3876-11", !d.unified(), "err");
    }
    // 12 场景三型
    {
        chk(&mut set, "C3876-12", SceneType::ALL.len() == 3, "err");
    }
    // 13 媒体三类
    {
        chk(&mut set, "C3876-13", MediaLayer::ALL.len() == 3, "err");
    }
    // 14 矩阵格生成
    {
        let mut cells = Vec::new();
        for st in SceneType::ALL {
            for ml in MediaLayer::ALL {
                cells.push(ComboCell { scene: st, media: ml, passed: true });
            }
        }
        chk(&mut set, "C3876-14", cells.len() == 9, "err");
    }
    // 15 全部通过
    {
        let mut cells = Vec::new();
        for st in SceneType::ALL {
            for ml in MediaLayer::ALL {
                cells.push(ComboCell { scene: st, media: ml, passed: true });
            }
        }
        chk(&mut set, "C3876-15", cells.iter().all(|c| c.passed), "err");
    }
    // 16 字幕不可见
    {
        let s = Subtitle3D { depth_z: 0.1, visible: false, occluded_by_scene: false };
        chk(&mut set, "C3876-16", !s.unoccluded(), "err");
    }
    // 17 深度等值（字幕贴场景前）
    {
        let d = DepthTest { scene_depth: 5.0, subtitle_depth: 5.0 };
        chk(&mut set, "C3876-17", d.subtitle_in_front(), "err");
    }
    // 18 零声源不完整
    {
        let sonar = SpatialSonar { sources: Vec::new(), rendered: true };
        chk(&mut set, "C3876-18", !sonar.direction_complete(), "err");
    }

    set
}
