//! CGPU-F1723 · 异构兼容性矩阵
//!
//! 判据锚点（施工书原文摘录）：
//! 兼容矩阵：矩阵（硬件×驱动×场景支持矩阵（支持等级（完整/部分/不支持——矩阵
//! （矩阵公开（用户可查——公开承诺；维护（矩阵更新制度（驱动发布同步——同步制度；
//! 查询（兼容查询 API（安装前探测——探测复用；诚实（矩阵诚实（不夸大支持——诚实
//! 红线复用；测试（矩阵/同步/查询/诚实四组）。判据：支持等级、公开、同步、查询、
//! 诚实、四组、判据。
//!
//! 诊断码段 0xA601~0xA605（独占）：
//!   0xA601 支持等级非法（等级不在三级闭集/部分级无因）
//!   0xA602 矩阵失同步（矩阵版本落后驱动发布版本）
//!   0xA603 查询断裂（三维键缺维/查询越界）
//!   0xA604 夸大支持（宣传等级高于矩阵实况）
//!   0xA605 段越界（判码落 0xA600..0xA700 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const COMPAT_MX_VERSION: &str = "K42-compat-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xA601~0xA605）
// ---------------------------------------------------------------------------

pub const XC_CM_LEVEL_BAD: u32 = 0xA601;
pub const XC_CM_STALE: u32 = 0xA602;
pub const XC_CM_QUERY_BROKEN: u32 = 0xA603;
pub const XC_CM_OVERCLAIM: u32 = 0xA604;
pub const XC_CM_SEG_OOR: u32 = 0xA605;

// ---------------------------------------------------------------------------
// 一、支持等级（三级闭集）
// ---------------------------------------------------------------------------

/// 支持等级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SupportLevel {
    /// 完整支持：场景全功能 + 门禁全过。
    Full,
    /// 部分支持：降级路径可用（必附原因）。
    Partial,
    /// 不支持：诚实标注（探测可检出）。
    None,
}

impl SupportLevel {
    pub const ALL: [SupportLevel; 3] =
        [SupportLevel::Full, SupportLevel::Partial, SupportLevel::None];

    pub const fn name(self) -> &'static str {
        match self {
            SupportLevel::Full => "full",
            SupportLevel::Partial => "partial",
            SupportLevel::None => "none",
        }
    }
}

// ---------------------------------------------------------------------------
// 二、矩阵格（硬件×驱动×场景 三维键）
// ---------------------------------------------------------------------------

/// 矩阵格：三维键 + 等级 + 部分级原因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MxCell {
    /// 硬件代际（设备 ID 低 16 位）。
    pub hw: u16,
    /// 驱动版本号。
    pub drv: u32,
    /// 场景族（0=游戏 1=创作 2=生产力 3=AI PC 4=基准）。
    pub scene: u8,
    pub level: SupportLevel,
    /// 部分级原因（Partial 必非空）。
    pub partial_reason: &'static str,
}

impl MxCell {
    /// 键合法判：三维齐 + 等级与原因配对。
    pub fn sound(&self) -> Result<(), u32> {
        if self.scene > 4 {
            return Err(XC_CM_QUERY_BROKEN);
        }
        match self.level {
            SupportLevel::Partial => {
                if self.partial_reason.is_empty() {
                    return Err(XC_CM_LEVEL_BAD);
                }
            }
            SupportLevel::Full => {
                if !self.partial_reason.is_empty() {
                    return Err(XC_CM_LEVEL_BAD);
                }
            }
            SupportLevel::None => {}
        }
        Ok(())
    }
}

/// 矩阵本体：格集合（定长在册——no_std 定容）。
pub struct CompatMatrix {
    pub cells: [Option<MxCell>; 16],
    /// 矩阵版本（与驱动发布版本对齐判用）。
    pub mx_ver: u32,
    /// 矩阵公开锚（用户可查——公开承诺）。
    pub public_anchor: &'static str,
}

impl CompatMatrix {
    pub fn build(mx_ver: u32) -> Self {
        CompatMatrix { cells: [None; 16], mx_ver, public_anchor: "docs/compat-matrix" }
    }

    /// 放入一格（键重复拒——同三维键只一格实况）。
    pub fn put(&mut self, cell: MxCell) -> Result<(), u32> {
        cell.sound()?;
        let mut i = 0;
        while i < self.cells.len() {
            if let Some(c) = &self.cells[i] {
                if c.hw == cell.hw && c.drv == cell.drv && c.scene == cell.scene {
                    return Err(XC_CM_QUERY_BROKEN);
                }
            }
            i += 1;
        }
        let mut i = 0;
        while i < self.cells.len() {
            if self.cells[i].is_none() {
                self.cells[i] = Some(cell);
                return Ok(());
            }
            i += 1;
        }
        Err(XC_CM_QUERY_BROKEN)
    }

    /// 查询（安装前探测复用）：三维键查等级；无格即不支持（诚实默认）。
    pub fn probe(&self, hw: u16, drv: u32, scene: u8) -> Result<(SupportLevel, &'static str), u32> {
        if scene > 4 {
            return Err(XC_CM_QUERY_BROKEN);
        }
        let mut i = 0;
        while i < self.cells.len() {
            if let Some(c) = &self.cells[i] {
                if c.hw == hw && c.drv == drv && c.scene == scene {
                    return Ok((c.level, c.partial_reason));
                }
            }
            i += 1;
        }
        Ok((SupportLevel::None, "矩阵无格——默认不支持"))
    }

    /// 公开承诺判：锚非空。
    pub fn published(&self) -> Result<(), u32> {
        if self.public_anchor.is_empty() {
            return Err(XC_CM_OVERCLAIM);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、同步制度（驱动发布同步）
// ---------------------------------------------------------------------------

/// 同步账：矩阵版本必须跟上驱动发布版本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SyncRule {
    pub mx_ver: u32,
    pub driver_release_ver: u32,
}

impl SyncRule {
    /// 同步判：矩阵版本 >= 驱动发布版本（落后即失同步）。
    pub fn synced(&self) -> Result<(), u32> {
        if self.mx_ver == 0 || self.driver_release_ver == 0 {
            return Err(XC_CM_STALE);
        }
        if self.mx_ver < self.driver_release_ver {
            return Err(XC_CM_STALE);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、诚实红线（不夸大支持）
// ---------------------------------------------------------------------------

/// 夸大检测：宣传等级不得高于矩阵实况。
/// 返回实况格（None 格恒 None）。
pub fn overclaim_check(
    claimed: SupportLevel,
    actual: SupportLevel,
) -> Result<(), u32> {
    let rank = |l: SupportLevel| -> u8 {
        match l {
            SupportLevel::Full => 2,
            SupportLevel::Partial => 1,
            SupportLevel::None => 0,
        }
    };
    if rank(claimed) > rank(actual) {
        return Err(XC_CM_OVERCLAIM);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 自检（四组：矩阵/同步/查询/诚实）
// ---------------------------------------------------------------------------

/// 兼容矩阵引擎。
pub struct CompatEngine;

impl CompatEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：矩阵建格——合法格全入册。
        let mut mx = CompatMatrix::build(7);
        mx.put(MxCell { hw: 0x1001, drv: 7, scene: 0, level: SupportLevel::Full, partial_reason: "" })?;
        mx.put(MxCell { hw: 0x1001, drv: 7, scene: 1, level: SupportLevel::Partial, partial_reason: "硬编降级软编" })?;
        mx.put(MxCell { hw: 0x1002, drv: 6, scene: 3, level: SupportLevel::None, partial_reason: "" })?;
        mx.published()?;
        // 反例：Partial 无因 / Full 带因 / 键重复 / 场景越界。
        if mx
            .put(MxCell { hw: 0x2001, drv: 7, scene: 2, level: SupportLevel::Partial, partial_reason: "" })
            .is_ok()
        {
            return Err(XC_CM_LEVEL_BAD);
        }
        if mx
            .put(MxCell { hw: 0x2001, drv: 7, scene: 2, level: SupportLevel::Full, partial_reason: "误注" })
            .is_ok()
        {
            return Err(XC_CM_LEVEL_BAD);
        }
        if mx
            .put(MxCell { hw: 0x1001, drv: 7, scene: 0, level: SupportLevel::Full, partial_reason: "" })
            .is_ok()
        {
            return Err(XC_CM_QUERY_BROKEN);
        }
        if mx
            .put(MxCell { hw: 0x2001, drv: 7, scene: 9, level: SupportLevel::None, partial_reason: "" })
            .is_ok()
        {
            return Err(XC_CM_QUERY_BROKEN);
        }
        // 组二：同步——齐平过 / 落后拒 / 零版拒。
        SyncRule { mx_ver: 7, driver_release_ver: 7 }.synced()?;
        if (SyncRule { mx_ver: 6, driver_release_ver: 7 }).synced().is_ok() {
            return Err(XC_CM_STALE);
        }
        if (SyncRule { mx_ver: 0, driver_release_ver: 1 }).synced().is_ok() {
            return Err(XC_CM_STALE);
        }
        // 组三：查询——命中三态各返 + 未命中默认 None + 越界拒。
        let (lv, _) = mx.probe(0x1001, 7, 0)?;
        if lv != SupportLevel::Full {
            return Err(XC_CM_QUERY_BROKEN);
        }
        let (lv2, why) = mx.probe(0x1001, 7, 1)?;
        if lv2 != SupportLevel::Partial || why.is_empty() {
            return Err(XC_CM_QUERY_BROKEN);
        }
        let (lv3, _) = mx.probe(0xFFFF, 1, 4)?;
        if lv3 != SupportLevel::None {
            return Err(XC_CM_QUERY_BROKEN);
        }
        if mx.probe(0x1001, 7, 9).is_ok() {
            return Err(XC_CM_QUERY_BROKEN);
        }
        // 组四：诚实红线——夸大两形态（None 报 Full / Partial 报 Full）拦。
        if overclaim_check(SupportLevel::Full, SupportLevel::None).is_ok() {
            return Err(XC_CM_OVERCLAIM);
        }
        if overclaim_check(SupportLevel::Full, SupportLevel::Partial).is_ok() {
            return Err(XC_CM_OVERCLAIM);
        }
        // 同级与低报放行。
        overclaim_check(SupportLevel::Partial, SupportLevel::Partial)?;
        overclaim_check(SupportLevel::None, SupportLevel::Full)?;
        Ok(())
    }

    /// 段对账：五码全落 0xA600..0xA700 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_CM_LEVEL_BAD,
            XC_CM_STALE,
            XC_CM_QUERY_BROKEN,
            XC_CM_OVERCLAIM,
            XC_CM_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xA600 || codes[i] >= 0xA700 {
                return Err(XC_CM_LEVEL_BAD);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_CM_LEVEL_BAD);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
