//! VE-F3548 · persona 与开机动画联动 —— 可选连带 + 2 秒硬上限延续 + 未确认不连 + 失效独立。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3548`
//!
//! # 职责（锚点原文拆解）
//!
//! - **可选连带**（[`BootLink`]）：换 Persona 时问一句
//!   要不要连带开机动画——联动是选项不是强制。
//! - **硬上限延续**（[`BOOT_ANIM_LIMIT_MS`]）：E07 的 2 秒红线
//!   在联动场景**不放松**——连带动画一样 2 秒封顶。
//! - **未确认不连**：用户没确认 → 只换 Persona 不换动画。
//! - **失效独立**（[`LinkOutcome::Independent`]）：联动失败
//!   （动画资产缺失等）→ Persona 照常换，动画不动（独立降级）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 连带未确认 | 不连带 |
//! | 动画超时 | 跳过动画（Persona 仍生效） |
//! | 联动失效 | 独立降级 |
//!
//! # 复杂度
//!
//! O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E19 段续：6140 起）
// ---------------------------------------------------------------------------

/// 开机联动诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootCode {
    /// 连带未确认（已拒绝联动）。
    Unconfirmed = 6140,
    /// 动画时长超硬上限（已截断）。
    OverLimit = 6141,
    /// 联动失效（独立降级）。
    LinkFailed = 6142,
    /// 动画资产缺失。
    AssetMissing = 6143,
}

impl BootCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            BootCode::Unconfirmed => "连带未确认已拒绝",
            BootCode::OverLimit => "动画超限已截断",
            BootCode::LinkFailed => "联动失效已独立降级",
            BootCode::AssetMissing => "动画资产缺失",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、硬上限
// ---------------------------------------------------------------------------

/// 开机动画时长硬上限（毫秒）——E07 2 秒红线延续，联动场景不放松。
pub const BOOT_ANIM_LIMIT_MS: u32 = 2000;

// ---------------------------------------------------------------------------
// 三、联动器
// ---------------------------------------------------------------------------

/// 联动结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkOutcome {
    /// Persona + 动画一起换。
    Linked,
    /// 只换 Persona（未确认或失效独立）。
    Independent,
    /// 全部跳过（Persona 也没换——仅当 persona 自身失败）。
    Skipped,
}

impl LinkOutcome {
    /// 结果短名（读屏播报）。
    pub const fn label(self) -> &'static str {
        match self {
            LinkOutcome::Linked => "已连带开机动画",
            LinkOutcome::Independent => "仅更换人格未联动动画",
            LinkOutcome::Skipped => "全部跳过",
        }
    }
}

/// 动画规格。
#[derive(Clone, Copy, Debug)]
pub struct BootAnimSpec {
    /// 资产是否可用。
    pub asset_present: bool,
    /// 期望时长（毫秒，超限截断）。
    pub duration_ms: u32,
}

impl BootAnimSpec {
    /// 截断到硬上限后的有效时长。
    pub const fn clamped_duration(&self) -> u32 {
        if self.duration_ms > BOOT_ANIM_LIMIT_MS {
            BOOT_ANIM_LIMIT_MS
        } else {
            self.duration_ms
        }
    }

    /// 是否被截断。
    pub const fn was_clamped(&self) -> bool {
        self.duration_ms > BOOT_ANIM_LIMIT_MS
    }
}

/// 联动器。
pub struct BootLink {
    /// 最近一次联动结果。
    last: Option<LinkOutcome>,
    /// 截断次数。
    pub clamps: u32,
    /// 独立降级次数。
    pub independents: u32,
}

impl BootLink {
    /// 新建。
    pub fn new() -> Self {
        Self {
            last: None,
            clamps: 0,
            independents: 0,
        }
    }

    /// 换 Persona 并询问联动。
    ///
    /// - `confirmed`：用户是否确认连带动画。
    /// - `anim`：动画规格（None = 无动画资产）。
    pub fn switch(
        &mut self,
        confirmed: bool,
        anim: Option<BootAnimSpec>,
    ) -> Result<LinkOutcome, BootCode> {
        // 未确认不连：只换 Persona。
        if !confirmed {
            self.last = Some(LinkOutcome::Independent);
            self.independents = self.independents.saturating_add(1);
            return Ok(LinkOutcome::Independent);
        }
        // 确认了但没动画资产：失效独立。
        let Some(spec) = anim else {
            self.last = Some(LinkOutcome::Independent);
            self.independents = self.independents.saturating_add(1);
            return Ok(LinkOutcome::Independent);
        };
        if !spec.asset_present {
            self.last = Some(LinkOutcome::Independent);
            self.independents = self.independents.saturating_add(1);
            return Ok(LinkOutcome::Independent);
        }
        // 超限截断（不拒播——截断到 2 秒内仍播）。
        if spec.was_clamped() {
            self.clamps = self.clamps.saturating_add(1);
        }
        // 有效时长守住硬上限后联动成功。
        let _effective = spec.clamped_duration();
        self.last = Some(LinkOutcome::Linked);
        Ok(LinkOutcome::Linked)
    }

    /// 上次结果。
    pub fn last_outcome(&self) -> Option<LinkOutcome> {
        self.last
    }

    /// 播报文案。
    pub fn announce(&self) -> &'static str {
        self.last.map(|o| o.label()).unwrap_or("尚未切换")
    }
}

impl Default for BootLink {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// 开机联动判据集。
pub fn run_boot_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10h_bootanim");

    let good_anim = BootAnimSpec {
        asset_present: true,
        duration_ms: 1500,
    };
    let long_anim = BootAnimSpec {
        asset_present: true,
        duration_ms: 5000,
    };
    let missing_anim = BootAnimSpec {
        asset_present: false,
        duration_ms: 1000,
    };

    // 1. 确认 + 有动画 → 联动。
    let mut l = BootLink::new();
    let linked = l.switch(true, Some(good_anim)) == Ok(LinkOutcome::Linked);
    set.add("6140-确认联动", linked, "");

    // 2. 未确认不连。
    let mut l2 = BootLink::new();
    let indep = l2.switch(false, Some(good_anim)) == Ok(LinkOutcome::Independent);
    set.add("6141-未确认不连", indep, "");

    // 3. 超限截断到 2 秒。
    set.add(
            "6142-硬上限截断",
            long_anim.clamped_duration() == BOOT_ANIM_LIMIT_MS && long_anim.was_clamped(),
            "",
        );

    // 4. 资产缺失独立降级。
    let mut l3 = BootLink::new();
    let miss = l3.switch(true, Some(missing_anim)) == Ok(LinkOutcome::Independent);
    set.add("6143-缺失独立", miss, "");

    // 5. None 动画独立降级。
    let mut l4 = BootLink::new();
    let none_case = l4.switch(true, None) == Ok(LinkOutcome::Independent);
    set.add("6144-无动画独立", none_case, "");

    // 6. 播报可达。
    let mut l5 = BootLink::new();
    let _ = l5.switch(true, Some(good_anim));
    set.add("6145-播报可达", l5.announce() == "已连带开机动画", "");

    // 7. 硬上限常量正确。
    set.add("6146-红线延续", BOOT_ANIM_LIMIT_MS == 2000, "");

    set
}
