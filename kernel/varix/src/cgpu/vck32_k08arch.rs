//! CGPU-F1713 · K08 开工与场景生态总纲
//!
//! 判据锚点（施工书原文摘录）：
//! 总纲：定位（K08=异构能力的场景化与生态化：让开发者和用户真正用上异构——
//! 落地声明；场景体系（游戏/创作/生产力/AI PC 四场景族——场景族映射（J07
//! 场景×异构深化——深化声明；生态五件（SDK/示例/基准/认证/兼容矩阵——生态
//! 五件套——五件清单；测试（定位/场景/五件三组）。判据：落地声明、四场景、
//! 五件套、三组、判据。
//!
//! 诊断码段 0x9A01~0x9A05（独占）：
//!   0x9A01 落地声明失格（受众缺/能力缺）
//!   0x9A02 场景族越表（场景落四闭集之外/J07 映射失锚）
//!   0x9A03 五件套缺件（SDK/示例/基准/认证/兼容矩阵任一缺席）
//!   0x9A04 段越界（判码落 0x9A00..0x9B00 区间外）
//!   0x9A05 保留哨（本段扩位预留——不派发）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。


/// 模块版本。
pub const K08_ARCH_VERSION: &str = "K32-k08arch-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0x9A01~0x9A05）
// ---------------------------------------------------------------------------

pub const XC_LANDING_MALFORMED: u32 = 0x9A01;
pub const XC_SCENE_OOB: u32 = 0x9A02;
pub const XC_ECO_MISS: u32 = 0x9A03;
pub const XC_SEG_OOR: u32 = 0x9A04;
pub const XC_SEG_RESERVED: u32 = 0x9A05;

// ---------------------------------------------------------------------------
// 一、落地声明：让开发者和用户真正用上异构
// ---------------------------------------------------------------------------

/// 受众二闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Audience {
    /// 开发者：SDK/示例/文档可上手。
    Developer,
    /// 用户：场景能力真实可感。
    EndUser,
}

impl Audience {
    pub const ALL: [Audience; 2] = [Audience::Developer, Audience::EndUser];

    pub const fn name(self) -> &'static str {
        match self {
            Audience::Developer => "developer",
            Audience::EndUser => "end_user",
        }
    }

    /// 每受众的落地能力锚（声明=受众×能力配对非空）。
    pub const fn landing_cap(self) -> &'static str {
        match self {
            Audience::Developer => "sdk+samples+docs",
            Audience::EndUser => "scene-capability-visible",
        }
    }
}

/// 落地声明：两受众全声明 + 能力锚非空（0x9A01）。
pub struct LandingDecl {
    declared: [bool; 2],
}

impl LandingDecl {
    pub fn build() -> Self {
        LandingDecl { declared: [true, true] }
    }

    pub fn complete(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < 2 {
            if !self.declared[i] || Audience::ALL[i].landing_cap().is_empty() {
                return Err(XC_LANDING_MALFORMED);
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、场景体系：四场景族 + J07 深化映射
// ---------------------------------------------------------------------------

/// 场景族四闭集（判据"游戏/创作/生产力/AI PC"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneFamily {
    Game,
    Create,
    Productivity,
    AiPc,
}

impl SceneFamily {
    pub const ALL: [SceneFamily; 4] = [
        SceneFamily::Game,
        SceneFamily::Create,
        SceneFamily::Productivity,
        SceneFamily::AiPc,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            SceneFamily::Game => "game",
            SceneFamily::Create => "create",
            SceneFamily::Productivity => "productivity",
            SceneFamily::AiPc => "ai_pc",
        }
    }

    /// J07 深化映射锚（场景族 → J07 场景×异构源单——深化声明非空）。
    pub const fn j07_anchor(self) -> &'static str {
        match self {
            SceneFamily::Game => "CGPU-J07-GAME",
            SceneFamily::Create => "CGPU-J07-CREATE",
            SceneFamily::Productivity => "CGPU-J07-PROD",
            SceneFamily::AiPc => "CGPU-J07-AIPC",
        }
    }
}

/// 场景映射账：四族全挂 J07 锚。
pub struct SceneMap;

impl SceneMap {
    /// 四族映射闭合（0x9A02）。
    pub fn closed() -> Result<(), u32> {
        for f in SceneFamily::ALL {
            if f.j07_anchor().is_empty() {
                return Err(XC_SCENE_OOB);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、生态五件套
// ---------------------------------------------------------------------------

/// 生态件五闭集（判据"SDK/示例/基准/认证/兼容矩阵"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EcoItem {
    Sdk,
    Samples,
    Benchmark,
    Certification,
    CompatMatrix,
}

impl EcoItem {
    pub const ALL: [EcoItem; 5] = [
        EcoItem::Sdk,
        EcoItem::Samples,
        EcoItem::Benchmark,
        EcoItem::Certification,
        EcoItem::CompatMatrix,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            EcoItem::Sdk => "sdk",
            EcoItem::Samples => "samples",
            EcoItem::Benchmark => "benchmark",
            EcoItem::Certification => "certification",
            EcoItem::CompatMatrix => "compat_matrix",
        }
    }

    /// 五件在 K 域/前域的源单锚（生态件不是空话——每件挂既有成果）。
    pub const fn origin(self) -> &'static str {
        match self {
            EcoItem::Sdk => "CGPU-F1719",
            EcoItem::Samples => "CGPU-F1720",
            EcoItem::Benchmark => "CGPU-F1721",
            EcoItem::Certification => "CGPU-F1722",
            EcoItem::CompatMatrix => "CGPU-F1723",
        }
    }
}

/// 五件套清单：全件在册 + 源单非空（0x9A03）。
pub struct EcoFive {
    ready: [bool; 5],
}

impl EcoFive {
    pub fn build() -> Self {
        EcoFive { ready: [true; 5] }
    }

    pub fn complete(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < 5 {
            if !self.ready[i] || EcoItem::ALL[i].origin().is_empty() {
                return Err(XC_ECO_MISS);
            }
            i += 1;
        }
        Ok(())
    }

    /// 缺件反例注入口。
    pub fn unready(&mut self, idx: usize) {
        if idx < 5 {
            self.ready[idx] = false;
        }
    }
}

// ---------------------------------------------------------------------------
// 自检（三组：定位/场景/五件）
// ---------------------------------------------------------------------------

/// K08 总纲引擎。
pub struct K08Engine {
    pub eco: EcoFive,
}

impl K08Engine {
    pub fn new() -> Self {
        K08Engine { eco: EcoFive::build() }
    }

    /// 组一：定位——落地声明两受众齐 + 断言反例。
    pub fn selfcheck_landing(&self) -> Result<(), u32> {
        LandingDecl::build().complete()?;
        // 断言反例：用户受众未声明。
        let mut bad = LandingDecl::build();
        bad.declared[1] = false;
        if bad.complete().is_ok() {
            return Err(XC_LANDING_MALFORMED);
        }
        Ok(())
    }

    /// 组二：场景——四族映射闭合 + 锚非空 + 族名非空。
    pub fn selfcheck_scenes(&self) -> Result<(), u32> {
        SceneMap::closed()?;
        for f in SceneFamily::ALL {
            if f.name().is_empty() {
                return Err(XC_SCENE_OOB);
            }
        }
        Ok(())
    }

    /// 组三：五件套——全件齐 + 缺件反例 + 源单锚非空。
    pub fn selfcheck_eco(&mut self) -> Result<(), u32> {
        self.eco.complete()?;
        // 缺件反例：抽走认证件。
        let mut bad = EcoFive::build();
        bad.unready(3);
        if bad.complete().is_ok() {
            return Err(XC_ECO_MISS);
        }
        // 五件源单锚逐一非空。
        for it in EcoItem::ALL {
            if it.origin().is_empty() || it.name().is_empty() {
                return Err(XC_ECO_MISS);
            }
        }
        Ok(())
    }

    /// 全量自检入口（三组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        Self::segment_guard()?;
        self.selfcheck_landing()?;
        self.selfcheck_scenes()?;
        self.selfcheck_eco()?;
        Ok(())
    }

    /// 段对账：五码全落 0x9A00..0x9B00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_LANDING_MALFORMED,
            XC_SCENE_OOB,
            XC_ECO_MISS,
            XC_SEG_OOR,
            XC_SEG_RESERVED,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x9A00 || codes[i] >= 0x9B00 {
                return Err(XC_LANDING_MALFORMED);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_LANDING_MALFORMED);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
