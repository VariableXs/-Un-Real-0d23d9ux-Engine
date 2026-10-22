//! cga151_jointver — CGPU-F0151 A 域与 VE 域联合验证记录。
//!
//! 判据原文（施工书锚点 CGPU-F0151）：A 域（算力层）与 VE 域（视觉
//! 层）的联合验证记录：VE-D 合成引擎使用 A 域算子的联合测试（同场景
//! CPU 渲染 vs GPU 渲染 vs 混合的端到端对拍记录）、联合性能基准
//! （VE 场景在 CGPU 调度下的帧率）。联合验证是两卷协作正确性的证明。
//! 判据：联合对拍记录、端到端一致、性能数据、协作缺陷处置、归档。
//!
//! **设计定盘**：联合对拍账：三路对拍闭集（CPU 路/GPU 路/混合路），
//! 每场景三路输出指纹必须全同（端到端一致判据——视觉层语义不因
//! 算力路径而变）。联合帧率账 = VE 场景在 CGPU 调度下的帧率行
//! （场景 + 帧/千 tick 定点）。协作缺陷 = 两卷边界缺陷账（缺陷号 +
//! 归属卷 + 处置态），未决缺陷必须清零。归档 = 对拍 + 帧率 + 缺陷
//! 三段指纹链。
//!
//! 诊断码段 0x0360~0x036F（0x03 段第 7 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA151_VERSION: &str = "cga151-jointver-v1";

/// 诊断子区基值（0x03 段第 7 子区）。
pub const SEG_BASE: u16 = 0x0360;

/// 对拍三路闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderPath {
    /// CPU 路径。
    Cpu,
    /// GPU 路径。
    Gpu,
    /// 混合路径。
    Mixed,
}

impl RenderPath {
    /// 全集稳定序。
    pub const ALL: [RenderPath; 3] = [
        RenderPath::Cpu,
        RenderPath::Gpu,
        RenderPath::Mixed,
    ];

    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            RenderPath::Cpu => 0,
            RenderPath::Gpu => 1,
            RenderPath::Mixed => 2,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 一条对拍记录（场景 × 路 → 输出指纹）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ShotRow {
    /// VE 场景号。
    pub scene: u32,
    /// 路径。
    pub path: RenderPath,
    /// 输出指纹。
    pub out_fp: u64,
}

/// 一条联合帧率行（帧/千 tick × 256 定点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FpsRow {
    /// VE 场景号。
    pub scene: u32,
    /// 帧率定点值。
    pub fps_fp256: u64,
}

/// 协作缺陷归属卷闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OwnerDomain {
    /// A 域（算力层）。
    CgpuA,
    /// VE 域（视觉层）。
    VeD,
    /// 边界（两卷交互面）。
    Boundary,
}

impl OwnerDomain {
    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            OwnerDomain::CgpuA => SEG_BASE + 4,
            OwnerDomain::VeD => SEG_BASE + 5,
            OwnerDomain::Boundary => SEG_BASE + 6,
        }
    }
}

/// 联合验证账。
pub struct JointVerLedger {
    /// 对拍记录。
    shots: Vec<ShotRow>,
    /// 帧率行。
    fps: Vec<FpsRow>,
    /// 协作缺陷（缺陷号, 归属, 处置态）。
    defects: Vec<(u32, OwnerDomain, bool)>,
    /// 归档。
    archived: Option<u64>,
}

impl JointVerLedger {
    /// 空账。
    pub fn new() -> Self {
        JointVerLedger {
            shots: Vec::new(),
            fps: Vec::new(),
            defects: Vec::new(),
            archived: None,
        }
    }

    /// 对拍登记（同场景同路重复即拒；指纹为零即拒——零指纹是
    /// 没渲染不是黑帧）。
    pub fn shot(&mut self, scene: u32, path: RenderPath, out_fp: u64) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 7);
        }
        if scene == 0 || out_fp == 0 {
            return Err(SEG_BASE + 8);
        }
        if self.shots.iter().any(|s| s.scene == scene && s.path == path) {
            return Err(SEG_BASE + 9);
        }
        self.shots.push(ShotRow { scene, path, out_fp });
        Ok(())
    }

    /// 端到端一致核验（判据"端到端一致"：每个已对拍场景的三路
    /// 指纹全同；三路不齐的场景拒——不齐即未完成对拍）。
    pub fn paths_agree(&self) -> Result<(), u16> {
        let mut scenes: Vec<u32> = Vec::new();
        for s in &self.shots {
            if !scenes.contains(&s.scene) {
                scenes.push(s.scene);
            }
        }
        if scenes.is_empty() {
            return Err(SEG_BASE + 10);
        }
        for sc in scenes {
            let fps: Vec<u64> = RenderPath::ALL
                .iter()
                .filter_map(|p| {
                    self.shots
                        .iter()
                        .find(|s| s.scene == sc && s.path == *p)
                        .map(|s| s.out_fp)
                })
                .collect();
            if fps.len() != 3 {
                return Err(SEG_BASE + 11);
            }
            if fps[0] != fps[1] || fps[1] != fps[2] {
                return Err(SEG_BASE + 12);
            }
        }
        Ok(())
    }

    /// 帧率登记（判据"性能数据"；同场景重复即拒）。
    pub fn fps(&mut self, scene: u32, fps_fp256: u64) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 7);
        }
        if scene == 0 || fps_fp256 == 0 {
            return Err(SEG_BASE + 13);
        }
        if self.fps.iter().any(|r| r.scene == scene) {
            return Err(SEG_BASE + 14);
        }
        self.fps.push(FpsRow { scene, fps_fp256 });
        Ok(())
    }

    /// 帧率最低场景（判据"性能数据"的定位面）。
    pub fn worst_fps(&self) -> Option<(u32, u64)> {
        let mut best: Option<(u32, u64)> = None;
        for r in &self.fps {
            match best {
                Some((_, f)) if f <= r.fps_fp256 => {}
                _ => best = Some((r.scene, r.fps_fp256)),
            }
        }
        best
    }

    /// 协作缺陷登记（判据"协作缺陷处置"；同号重复即拒）。
    pub fn defect(&mut self, id: u32, owner: OwnerDomain) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 7);
        }
        if id == 0 || self.defects.iter().any(|(i, _, _)| *i == id) {
            return Err(SEG_BASE + 15);
        }
        self.defects.push((id, owner, false));
        Ok(())
    }

    /// 缺陷核销（判据"协作缺陷处置"的闭环面；未登记即拒）。
    pub fn resolve_defect(&mut self, id: u32) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 7);
        }
        let d = match self.defects.iter_mut().find(|(i, _, _)| *i == id) {
            Some(d) => d,
            None => return Err(SEG_BASE + 16),
        };
        if d.2 {
            return Err(SEG_BASE + 17);
        }
        d.2 = true;
        Ok(())
    }

    /// 未决缺陷数。
    pub fn open_defects(&self) -> usize {
        self.defects.iter().filter(|(_, _, r)| !r).count()
    }

    /// 对拍场景数。
    pub fn scene_count(&self) -> usize {
        let mut scenes: Vec<u32> = Vec::new();
        for s in &self.shots {
            if !scenes.contains(&s.scene) {
                scenes.push(s.scene);
            }
        }
        scenes.len()
    }

    /// 归档（判据"归档"：对拍一致 + 帧率非空 + 缺陷清零）。
    pub fn archive(&mut self) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 18);
        }
        self.paths_agree()?;
        if self.fps.is_empty() {
            return Err(SEG_BASE + 19);
        }
        if self.open_defects() != 0 {
            return Err(SEG_BASE + 20);
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for s in &self.shots {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ s.out_fp;
        }
        for r in &self.fps {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ r.fps_fp256;
        }
        self.archived = Some(fp);
        Ok(fp)
    }

    /// 归档指纹。
    pub fn archive_fp(&self) -> Option<u64> {
        self.archived
    }
}

impl Default for JointVerLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl JointVerLedger {
    /// 同景冲突检查（判据「对拍一致」的可算化）：同场景同路径
    /// 出现两条不同输出指纹 = 路径不确定，即失配。
    pub fn shot_conflicts(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for (i, s) in self.shots.iter().enumerate() {
            for (j, t) in self.shots.iter().enumerate() {
                if i < j && s.scene == t.scene && s.path == t.path
                    && s.out_fp != t.out_fp
                    && !bad.contains(&s.scene)
                {
                    bad.push(s.scene);
                }
            }
        }
        bad
    }

    /// 帧率达标率（千分比）：帧率定点值 ≥ 地板线的行占比。
    pub fn fps_pass_permille(&self, floor_fp256: u64) -> Option<u32> {
        if self.fps.is_empty() {
            return None;
        }
        let ok = self.fps.iter().filter(|f| f.fps_fp256 >= floor_fp256).count();
        Some(((ok * 1000) / self.fps.len()) as u32)
    }

    /// 缺陷归属普查：三域各多少条。
    pub fn defect_census(&self) -> [usize; 3] {
        let mut out = [0usize; 3];
        for (_, owner, _) in &self.defects {
            out[*owner as usize % 3] += 1;
        }
        out
    }

    /// 归档前置总检：无未清缺陷 + 对拍零冲突 + 帧率全达标。
    pub fn pre_archive(&self, floor_fp256: u64) -> Result<(), u16> {
        if self.open_defects() > 0 {
            return Err(SEG_BASE + 20);
        }
        if !self.shot_conflicts().is_empty() {
            return Err(SEG_BASE + 21);
        }
        match self.fps_pass_permille(floor_fp256) {
            Some(1000) => Ok(()),
            _ => Err(SEG_BASE + 22),
        }
    }
}
