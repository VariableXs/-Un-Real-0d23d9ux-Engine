//! VE-F3412 · 高对比度主题运行时 —— 7:1 起步的无障碍令牌基线 + 色弱安全 + 正交组合。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3412`
//!
//! # 职责（锚点原文拆解）
//!
//! - **7:1 起步**：高对比令牌基线对 HC 表面（深空单源）做 WCAG AAA
//!   （7:1）断言起步——AA 是及格线，高对比域从 AAA 起算，低于 7:1
//!   一律阻断（`ContrastLow`）；
//! - **色弱安全（不只靠色相区分）**：每个令牌除色相外必带**非色相通道**
//!   （图案/形状标签）——色弱用户丢的是色相通道，只靠色相区分等于
//!   对他们不存在区分。通道未声明 → 违例 → 整改（按语义类默认图案
//!   补齐）并记账；四个语义类的默认墨色两两亮度差 ≥ 30（亮度是
//!   第二条区分通道，判据侧独立对拍）；
//! - **与明暗主题的正交组合**：HC 是独立于明/暗的轴——组合器按路径
//!   仲裁：HC 覆盖的路径 HC 优先（无障碍优先），未覆盖的回落到明暗
//!   基线 sided 值；两表同路径即冲突，仲裁结果入账（`ComposeConflict`
//!   不是错误，是「谁盖住了谁」的可审计事实）；
//! - **表面冻结**：7:1 断言全部相对 HC 表面——表面随开随换等于把
//!   已通过的断言全部作废，故表面建库时定、之后只读（`SurfaceFrozen`）；
//! - **跨批对接点 V06 无障碍联动**：联动挂点（强度 ‰），越界即拒；
//! - **降级矩阵**：对比度不足→阻断；组合冲突→仲裁；色弱违例→整改。
//!
//! # 为什么色弱违例是整改不是阻断
//!
//! 锚点降级矩阵明文：色弱违例→整改。缺非色相通道是「少写了一维」，
//!   不是「值错了」——按语义类默认图案补上即可恢复区分力，用户拿到
//!   的令牌照常可用，账目记下补过什么。若阻断，一个忘标 shape 的
//!   令牌会让整条无障碍路径开路不通——比色弱风险更伤可达性。
//!
//! # 零 panic 面
//!
//! `[i]` / `unwrap()` / `expect()` 只出现在 `#[cfg(test)]`；
//! 判据区一律 match 记红。

use crate::svstar2::ver01k_dualtheme::{contrast_permille, PairTable, ThemeSide, CONTRAST_AAA_PERMILLE, PATH_MAX};

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（自建；E14 段独占，与 E11/E12/E13 等既有段零重叠）
// ---------------------------------------------------------------------------

/// 高对比域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HcCode {
    /// 令牌路径非法（空或超长）。
    TokenEmpty,
    /// 语义类非法（闭集外类号）。
    ClassUnknown,
    /// 对比度不足 7:1（AAA 起步，阻断）。
    ContrastLow,
    /// 色弱违例（缺非色相通道，整改补齐，非阻断）。
    ColorBlind,
    /// 组合冲突（HC 与基线同路径，仲裁入账，非阻断）。
    ComposeConflict,
    /// V06 无障碍联动状态非法（强度越界）。
    LinkageState,
    /// HC 表面冻结（建库后只读，改表面即拒）。
    SurfaceFrozen,
}

impl HcCode {
    /// 全部码（判据据此核对无遗漏）。
    pub const ALL: [HcCode; 7] = [
        HcCode::TokenEmpty,
        HcCode::ClassUnknown,
        HcCode::ContrastLow,
        HcCode::ColorBlind,
        HcCode::ComposeConflict,
        HcCode::LinkageState,
        HcCode::SurfaceFrozen,
    ];

    /// 线上短码（E14 段独占）。
    pub const fn code(self) -> &'static str {
        match self {
            HcCode::TokenEmpty => "E14-TOKEN-EMPTY",
            HcCode::ClassUnknown => "E14-CLASS-UNKNOWN",
            HcCode::ContrastLow => "E14-CONTRAST-LOW",
            HcCode::ColorBlind => "E14-COLOR-BLIND",
            HcCode::ComposeConflict => "E14-COMPOSE-CONFLICT",
            HcCode::LinkageState => "E14-LINKAGE-STATE",
            HcCode::SurfaceFrozen => "E14-SURFACE-FROZEN",
        }
    }

    /// 是否阻断。
    pub const fn blocking(self) -> bool {
        !matches!(self, HcCode::ColorBlind | HcCode::ComposeConflict)
    }

    /// 是否事件类（整改/仲裁记账）。
    pub const fn eventful(self) -> bool {
        matches!(self, HcCode::ColorBlind | HcCode::ComposeConflict)
    }

    /// 读屏可达句子。
    pub fn spoken(self) -> String {
        let s = match self {
            HcCode::TokenEmpty => "令牌路径非法。",
            HcCode::ClassUnknown => "语义类非法。",
            HcCode::ContrastLow => "对比度未达 7:1，已阻断。",
            HcCode::ColorBlind => "缺非色相通道，已按类默认图案整改。",
            HcCode::ComposeConflict => "组合冲突，高对比优先已仲裁。",
            HcCode::LinkageState => "无障碍联动状态非法。",
            HcCode::SurfaceFrozen => "高对比表面已冻结，不可更改。",
        };
        format!("{}{}", s, self.code())
    }
}

// ---------------------------------------------------------------------------
// 二、契约常量
// ---------------------------------------------------------------------------

/// 契约版本（冻结）。
pub const HC_CONTRACT: &str = "E14-highcontrast-v1";
/// 色弱安全：通道最小亮度差（亮度是色相之外的第二区分通道）。
pub const CB_LUM_DELTA: u32 = 30;
/// 联动强度上限（‰）。
pub const PERMILLE_MAX: u32 = 1000;
/// 语义类数（闭集）。
pub const HC_CLASSES: usize = 4;
/// 图案通道数（闭集：Plain/Underline/Ring/Hatch）。
pub const HC_SHAPES: usize = 4;

fn validate_path(p: &str) -> Result<(), HcCode> {
    if p.is_empty() || p.len() > PATH_MAX {
        return Err(HcCode::TokenEmpty);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、语义类与非色相通道（色弱安全两通道）
// ---------------------------------------------------------------------------

/// 语义类号（闭集 0..=3；开集 u8 入口，越界即拒）。
pub const CLASS_INK: u8 = 0;
pub const CLASS_ACCENT: u8 = 1;
pub const CLASS_OK: u8 = 2;
pub const CLASS_ALERT: u8 = 3;

/// 类号闭集反查（越界 None）。
pub const fn class_zh(c: u8) -> Option<&'static str> {
    match c {
        CLASS_INK => Some("正文"),
        CLASS_ACCENT => Some("强调"),
        CLASS_OK => Some("成功"),
        CLASS_ALERT => Some("警示"),
        _ => None,
    }
}

/// 非色相通道（图案/形状标签）——色弱用户可用的第二区分通道。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapeTag {
    /// 平面（默认通道本身也是一个取值：已声明）。
    Plain = 0,
    /// 下划线。
    Underline = 1,
    /// 环标。
    Ring = 2,
    /// 斜纹。
    Hatch = 3,
}

impl ShapeTag {
    /// 全集（判据据此核对无遗漏）。
    pub const ALL: [ShapeTag; HC_SHAPES] = [
        ShapeTag::Plain,
        ShapeTag::Underline,
        ShapeTag::Ring,
        ShapeTag::Hatch,
    ];

    /// wire 序号。
    pub const fn wire(self) -> u8 {
        self as u8
    }

    /// 序号反查（越界 None）。
    pub const fn of_wire(v: u8) -> Option<ShapeTag> {
        match v {
            0 => Some(ShapeTag::Plain),
            1 => Some(ShapeTag::Underline),
            2 => Some(ShapeTag::Ring),
            3 => Some(ShapeTag::Hatch),
            _ => None,
        }
    }
}

/// 语义类默认图案（非色相通道补齐单源：整改按此填）。
pub const fn class_default_shape(c: u8) -> Option<ShapeTag> {
    match c {
        CLASS_INK => Some(ShapeTag::Plain),
        CLASS_ACCENT => Some(ShapeTag::Underline),
        CLASS_OK => Some(ShapeTag::Ring),
        CLASS_ALERT => Some(ShapeTag::Hatch),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 四、类默认墨色表（冻结单源；亮度两两分离见判据侧独立对拍）
// ---------------------------------------------------------------------------

/// 类默认墨色（冻结表：亮 255 / 180 / 211 / 134，两两差 ≥ CB_LUM_DELTA）。
pub const fn class_default_ink(c: u8) -> Option<(u8, u8, u8)> {
    match c {
        CLASS_INK => Some((255, 255, 255)),
        CLASS_ACCENT => Some((255, 170, 60)),
        CLASS_OK => Some((102, 255, 102)),
        CLASS_ALERT => Some((255, 102, 102)),
        _ => None,
    }
}

/// RGB 便捷构造（判据与构造同口径）。
const fn rgb(t: (u8, u8, u8)) -> crate::svstar2::ver01k_dualtheme::RGB888 {
    crate::svstar2::ver01k_dualtheme::RGB888::new(t.0, t.1, t.2)
}

// ---------------------------------------------------------------------------
// 五、高对比令牌与基线
// ---------------------------------------------------------------------------

/// 高对比令牌：墨色 + 非色相通道 + 语义类。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HcToken {
    /// 令牌路径。
    pub path: String,
    /// 语义类（0..=3）。
    pub class: u8,
    /// 墨色（对 HC 表面 7:1 起步）。
    pub ink: crate::svstar2::ver01k_dualtheme::RGB888,
    /// 非色相通道（整改后必有值）。
    pub shape: ShapeTag,
}

/// 高对比基线（表面冻结 + 类默认墨色单源 + 7:1 断言）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HcBaseline {
    /// HC 表面（深空单源；建库冻结）。
    pub surface: crate::svstar2::ver01k_dualtheme::RGB888,
    /// 令牌全集（按路径唯一）。
    pub tokens: Vec<HcToken>,
    /// 色弱整改账（哪些令牌补过非色相通道）。
    pub remediated: Vec<String>,
}

impl HcBaseline {
    /// 建基线：表面随建冻结，令牌空。
    pub fn new(surface: crate::svstar2::ver01k_dualtheme::RGB888) -> HcBaseline {
        HcBaseline {
            surface,
            tokens: Vec::new(),
            remediated: Vec::new(),
        }
    }

    /// 默认深空表面基线（纯黑：HC 单源表面）。
    pub fn deep() -> HcBaseline {
        HcBaseline::new(crate::svstar2::ver01k_dualtheme::RGB888::new(0, 0, 0))
    }

    /// 改表面一律拒绝（断言相对表面，换表=作废已过断言）。
    pub fn set_surface(&mut self, _s: crate::svstar2::ver01k_dualtheme::RGB888) -> Result<(), HcCode> {
        Err(HcCode::SurfaceFrozen)
    }

    /// 插入令牌（唯一入口：类闭集 + 7:1 + 色弱通道三道闸）。
    ///
    /// - 类号越界 → `Err(ClassUnknown)`；
    /// - 墨色对表面不达 7:1 → `Err(ContrastLow)`（阻断，整改不救——
    ///   色弱通道补的是区分力，补不了对比度）；
    /// - `shape=None` → 色弱违例 → 整改：按类默认图案补齐并记账，
    ///   返回 `Ok(Some(ColorBlind))`；
    /// - 同路径已存在 → 覆盖（三道闸不豁免）。
    pub fn insert(
        &mut self,
        path: &str,
        class: u8,
        ink: crate::svstar2::ver01k_dualtheme::RGB888,
        shape: Option<ShapeTag>,
    ) -> Result<Option<HcCode>, HcCode> {
        validate_path(path)?;
        let default_shape = match class_default_shape(class) {
            Some(s) => s,
            None => return Err(HcCode::ClassUnknown),
        };
        if contrast_permille(ink, self.surface) < CONTRAST_AAA_PERMILLE {
            return Err(HcCode::ContrastLow);
        }
        let (final_shape, note) = match shape {
            Some(s) => (s, None),
            None => (default_shape, Some(HcCode::ColorBlind)),
        };
        for t in self.tokens.iter_mut() {
            if t.path == path {
                t.class = class;
                t.ink = ink;
                t.shape = final_shape;
                if let Some(c) = note {
                    self.remediated.push(String::from(path));
                    return Ok(Some(c));
                }
                return Ok(None);
            }
        }
        if let Some(c) = note {
            self.remediated.push(String::from(path));
        }
        self.tokens.push(HcToken {
            path: String::from(path),
            class,
            ink,
            shape: final_shape,
        });
        Ok(note)
    }

    /// 按路径查令牌。
    pub fn get(&self, path: &str) -> Option<&HcToken> {
        self.tokens.iter().find(|t| t.path == path)
    }

    /// 按路径删令牌。
    pub fn remove(&mut self, path: &str) -> bool {
        let before = self.tokens.len();
        self.tokens.retain(|t| t.path != path);
        self.tokens.len() != before
    }

    /// 令牌数。
    pub fn count(&self) -> usize {
        self.tokens.len()
    }

    /// 类默认墨色的实际 RGB（冻结表单源出口）。
    pub fn class_ink(c: u8) -> Option<crate::svstar2::ver01k_dualtheme::RGB888> {
        class_default_ink(c).map(rgb)
    }
}

// ---------------------------------------------------------------------------
// 六、组合器（与明暗主题正交：HC 优先仲裁）
// ---------------------------------------------------------------------------

/// 组合裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ComposeOutcome {
    /// 取自 HC 基线（覆盖路径，无障碍优先）。
    Hc,
    /// 取自明/暗基线（HC 未覆盖，回落）。
    Base,
}

/// 组合器：HC 轴与明暗轴正交，按路径仲裁。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Composer {
    /// 冲突账（两表同路径的仲裁记录）。
    pub conflicts: Vec<String>,
}

impl Composer {
    /// 新建组合器（账空）。
    pub fn new() -> Composer {
        Composer {
            conflicts: Vec::new(),
        }
    }

    /// 组合取值：HC 覆盖路径 HC 优先并记账冲突；否则回落基线 sided 值。
    pub fn compose(
        &mut self,
        hc: &HcBaseline,
        base: &PairTable,
        path: &str,
        side: ThemeSide,
    ) -> Option<(crate::svstar2::ver01k_dualtheme::RGB888, ComposeOutcome)> {
        if let Some(t) = hc.get(path) {
            if base.get(path).is_some() {
                self.conflicts.push(String::from(path));
            }
            return Some((t.ink, ComposeOutcome::Hc));
        }
        base.value_of(path, side)
            .map(|v| (v, ComposeOutcome::Base))
    }

    /// 组合表面：HC 覆盖路径返回 HC 表面（墨与表面成对交付）。
    pub fn compose_surface(&self, hc: &HcBaseline, path: &str) -> Option<crate::svstar2::ver01k_dualtheme::RGB888> {
        if hc.get(path).is_some() {
            Some(hc.surface)
        } else {
            None
        }
    }

    /// 冲突账去重后的条数（重复仲裁不重复计）。
    pub fn conflict_count(&self) -> usize {
        let mut seen: Vec<&String> = Vec::new();
        for c in self.conflicts.iter() {
            if !seen.iter().any(|s| *s == c) {
                seen.push(c);
            }
        }
        seen.len()
    }
}

// ---------------------------------------------------------------------------
// 七、V06 无障碍联动挂点 + 读屏
// ---------------------------------------------------------------------------

/// V06 无障碍联动挂点（跨批对接点：强度 ‰，越界即拒）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct A11yLinkage {
    /// 联动是否启用。
    pub enabled: bool,
    /// 联动强度（‰，0..=1000）。
    pub strength_permille: u32,
}

impl A11yLinkage {
    /// 默认：关闭。
    pub const fn off() -> A11yLinkage {
        A11yLinkage {
            enabled: false,
            strength_permille: 0,
        }
    }

    /// 校验联动状态。
    pub fn validate(&self) -> Result<(), HcCode> {
        if self.strength_permille > PERMILLE_MAX {
            return Err(HcCode::LinkageState);
        }
        Ok(())
    }
}

/// 基线读屏播报：表面亮度 / 令牌数 / 整改数 / 契约版本。
pub fn baseline_spoken(hc: &HcBaseline) -> String {
    format!(
        "高对比基线：表面亮度{}，令牌{}项，整改{}项，7:1 起步；{}",
        hc.surface.luminance(),
        hc.count(),
        hc.remediated.len(),
        HC_CONTRACT
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_to_one_gate() {
        let mut hc = HcBaseline::deep();
        // 白对黑 20615‰ ≥ 7000 放行。
        assert!(hc.insert("c.fg", CLASS_INK, rgb((255, 255, 255)), None).is_ok());
        // 中灰对黑 (128+13)*1000/13 = 10846 ≥ 7000 也放行——7:1 起步不是只准纯白。
        assert!(hc.insert("c.gray", CLASS_ACCENT, rgb((128, 128, 128)), None).is_ok());
        // 暗红对黑 (54+13)*1000/13 = 5153 < 7000 阻断。
        assert_eq!(
            hc.insert("c.darkred", CLASS_ALERT, rgb((255, 0, 0)), None),
            Err(HcCode::ContrastLow)
        );
    }

    #[test]
    fn colorblind_remediation() {
        let mut hc = HcBaseline::deep();
        // 缺非色相通道 → 整改补齐（类默认图案）+ 记账。
        let r = hc.insert("c.b", CLASS_ACCENT, rgb((255, 170, 60)), None);
        assert_eq!(r, Ok(Some(HcCode::ColorBlind)));
        assert_eq!(
            hc.get("c.b").map(|t| t.shape),
            Some(ShapeTag::Underline)
        );
        assert_eq!(hc.remediated.len(), 1);
        // 显式声明通道 → 无事件。
        assert_eq!(
            hc.insert("c.c", CLASS_OK, rgb((102, 255, 102)), Some(ShapeTag::Ring)),
            Ok(None)
        );
    }

    #[test]
    fn class_gate_and_surface_freeze() {
        let mut hc = HcBaseline::deep();
        assert_eq!(
            hc.insert("c.x", 4, rgb((255, 255, 255)), None),
            Err(HcCode::ClassUnknown)
        );
        assert_eq!(
            hc.set_surface(rgb((255, 255, 255))),
            Err(HcCode::SurfaceFrozen)
        );
    }

    #[test]
    fn composer_arbitration() {
        let mut hc = HcBaseline::deep();
        hc.insert("c.fg", CLASS_INK, rgb((255, 255, 255)), None).ok();
        let mut base = PairTable::new(rgb((255, 255, 255)), rgb((0, 0, 0)));
        base.insert("c.fg", Some(rgb((0, 0, 0))), Some(rgb((255, 255, 255)))).ok();
        base.insert("c.bg", Some(rgb((0, 0, 0))), Some(rgb((255, 255, 255)))).ok();
        let mut cp = Composer::new();
        let (v, o) = cp.compose(&hc, &base, "c.fg", ThemeSide::Light).unwrap();
        assert_eq!(o, ComposeOutcome::Hc);
        assert_eq!(v, rgb((255, 255, 255)));
        assert_eq!(cp.conflict_count(), 1);
        let (v2, o2) = cp.compose(&hc, &base, "c.bg", ThemeSide::Light).unwrap();
        assert_eq!(o2, ComposeOutcome::Base);
        assert_eq!(v2, rgb((0, 0, 0)));
        assert!(cp.compose(&hc, &base, "c.none", ThemeSide::Light).is_none());
    }
}
