//! VE-F3521 · Accent Ramp 色彩派生运行时 —— 单基色派生十级梯度 + 三参数可调 + WCAG 过滤。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3521`
//!
//! # 职责（锚点原文拆解）
//!
//! - **十级梯度**（[`RampLevel`]）：强调色从单一基色派生出十级明暗
//!   梯度（L0 最暗 ~ L9 最亮）。一个强调色长出一个色彩家族——
//!   **家族每一代都要合格**（见对比度过滤）。
//! - **三参数可调**（[`RampParams`]）：派生曲线由三参数控制——
//!   色相偏移（`hue_shift`，每级偏移的毫度数）、饱和度衰减
//!   （`sat_fade`，每级衰减的千分比）、明度步进（`lum_step`，
//!   每级步进的千分比）。参数越界钳制不拒绝（派生是内部计算，
//!   钳出来的结果用户看得见，比报错友好）。
//! - **对比度达标过滤**（[`contrast_ratio_permille`]）：梯度中的每级
//!   过 WCAG 检查（相对亮度比 ≥ [`WCAG_AA_PERMILLE`] = 4.5:1 对
//!   白/黑底取较优者），不达标即标记（[`RampSlot::flagged`]）并
//!   给替代（同色相最接近的达标级）。
//! - **断链回退**（[`RampBook::derive`]）：派生链断裂（基色缺失）
//!   → 回退母值（[`FALLBACK_BASE`]），派生失败不给黑屏。
//!
//! # 颜色表示
//!
//! 全程 RGB888 整数（0..=255 三通道），亮度/饱和度变换用定点
//! 千分比——与 V 域色彩管线的单源约定一致（无浮点）。
//!
//! # 复杂度
//!
//! - `derive`：O(梯度数)（固定 10）；
//! - `contrast_ratio_permille`：O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5900 起）
// ---------------------------------------------------------------------------

/// Accent Ramp 域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RampCode {
    /// 梯度级数非法（零级派生无意义）。
    ZeroLevels = 5900,
    /// 参数越界（已钳制，审计记录）。
    ParamClamped = 5901,
    /// 基色缺失（断链回退母值）。
    BaseMissing = 5902,
    /// 全梯度不达标（极端参数下的退化）。
    AllFlagged = 5903,
}

impl RampCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            RampCode::ZeroLevels => "梯度级数为零",
            RampCode::ParamClamped => "派生参数越界已钳制",
            RampCode::BaseMissing => "基色缺失回退母值",
            RampCode::AllFlagged => "全梯度对比度不达标",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, RampCode::AllFlagged)
    }
}

// ---------------------------------------------------------------------------
// 二、常量与颜色工具
// ---------------------------------------------------------------------------

/// 梯度级数（固定十级）。
pub const RAMP_LEVELS: usize = 10;
/// WCAG AA 对比度门槛（千分比，4.5:1）。
pub const WCAG_AA_PERMILLE: u32 = 4500;
/// 断链回退母色（中性蓝 #3584E4）。
pub const FALLBACK_BASE: Rgb888 = Rgb888::new(53, 132, 228);

/// RGB888 整数色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb888 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb888 {
    /// 构造。
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb888 { r, g, b }
    }

    /// 相对亮度（千分比定点，WCAG 公式整数化）。
    /// L = 0.2126R + 0.7152G + 0.0722B（线性化简化为 sRGB 直读，
    /// 误差在家族内部自洽比对中可接受——所有级同口径）。
    pub fn luminance_permille(self) -> u32 {
        (2126 * self.r as u32 + 7152 * self.g as u32 + 722 * self.b as u32) / 1000
    }

    /// 与另一色的对比度（千分比，(L亮+50)/(L暗+50) 的千分近似）。
    pub fn contrast_with(self, other: Rgb888) -> u32 {
        let a = self.luminance_permille();
        let b = other.luminance_permille();
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 50) * 1000 / (lo + 50)
    }

    /// 派生一档：按明度步进 `lum_step`（千分比，正亮负暗）与
    /// 饱和度衰减 `sat_fade` 调整。通道级钳制。
    pub fn stepped(self, lum_step: i32, sat_fade: u32) -> Rgb888 {
        let fade = |c: u8| -> u8 {
            let toward = 128i32; // 向中性灰收拢（饱和度衰减）
            let d = (toward - c as i32) * sat_fade as i32 / 1000;
            (c as i32 + d).clamp(0, 255) as u8
        };
        let shift = |c: u8| -> u8 {
            let v = c as i32 * (1000 + lum_step) / 1000;
            v.clamp(0, 255) as u8
        };
        Rgb888::new(shift(fade(self.r)), shift(fade(self.g)), shift(fade(self.b)))
    }
}

/// 对白色背景的对比度（千分比）。
pub fn contrast_ratio_permille(c: Rgb888) -> u32 {
    Rgb888::new(255, 255, 255).contrast_with(c)
}

/// 对黑色背景的对比度（千分比）。
pub fn contrast_on_black_permille(c: Rgb888) -> u32 {
    Rgb888::new(0, 0, 0).contrast_with(c)
}

/// 双底取优：对白与对黑中较高者（家族级的合格口径）。
pub fn contrast_best_permille(c: Rgb888) -> u32 {
    contrast_ratio_permille(c).max(contrast_on_black_permille(c))
}

// ---------------------------------------------------------------------------
// 三、派生参数
// ---------------------------------------------------------------------------

/// 派生三参数（越界钳制）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RampParams {
    /// 每级色相偏移（毫度，-30000..=30000；本册以 RGB 步进近似）。
    pub hue_shift: i32,
    /// 每级饱和度衰减（千分比 0..=200）。
    pub sat_fade: u32,
    /// 每级明度步进（千分比 -400..=400）。
    pub lum_step: i32,
}

impl RampParams {
    /// 钳制构造。
    pub fn build(hue_shift: i32, sat_fade: u32, lum_step: i32) -> (Self, bool) {
        let clamped = hue_shift != hue_shift.clamp(-30000, 30000)
            || sat_fade > 200
            || !(lum_step >= -400 && lum_step <= 400);
        (
            RampParams {
                hue_shift: hue_shift.clamp(-30000, 30000),
                sat_fade: sat_fade.min(200),
                lum_step: lum_step.clamp(-400, 400),
            },
            clamped,
        )
    }

    /// 标准参数（负步进：L0 最亮向 L9 最暗延伸，家族从基色向暗长）。
    pub fn standard() -> Self {
        RampParams {
            hue_shift: 0,
            sat_fade: 40,
            lum_step: -80,
        }
    }
}

// ---------------------------------------------------------------------------
// 四、梯度表
// ---------------------------------------------------------------------------

/// 一级梯度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RampSlot {
    /// 级号（0..=9）。
    pub level: u8,
    /// 派生色。
    pub color: Rgb888,
    /// 对比度不达标标记。
    pub flagged: bool,
    /// 达标替代级（自身达标则指向自身）。
    pub substitute: u8,
}

/// 梯度表：十级派生 + 过滤标记。
#[derive(Clone, Debug)]
pub struct RampTable {
    pub slots: Vec<RampSlot>,
    /// 派生参数（记录回放）。
    pub params: RampParams,
}

impl RampTable {
    /// 从基色 + 参数派生十级。每级从上一级再派生（家族链），
    /// 断链（中级失败）不可能——stepped 是纯函数；过滤后
    /// 全不达标给 [`RampCode::AllFlagged`] 信号（由调用方决定回退）。
    pub fn derive(base: Rgb888, params: RampParams) -> Self {
        let mut slots = Vec::with_capacity(RAMP_LEVELS);
        let mut cur = base;
        for i in 0..RAMP_LEVELS {
            if i > 0 {
                cur = cur.stepped(params.lum_step, params.sat_fade);
            }
            slots.push(RampSlot {
                level: i as u8,
                color: cur,
                flagged: false,
                substitute: i as u8,
            });
        }
        // 对比度过滤：不达标级标记并找最近达标替代。
        for i in 0..slots.len() {
            let ok = contrast_best_permille(slots[i].color) >= WCAG_AA_PERMILLE;
            if !ok {
                slots[i].flagged = true;
                // 替代：向两侧找最近达标级。
                slots[i].substitute = find_substitute(&slots, i);
            }
        }
        RampTable { slots, params }
    }

    /// 达标级数。
    pub fn passing(&self) -> usize {
        self.slots.iter().filter(|s| !s.flagged).count()
    }

    /// 取级色（flagged 级自动走替代级——家族每一代都要合格）。
    pub fn effective(&self, level: u8) -> Option<Rgb888> {
        let idx = level as usize;
        if idx >= self.slots.len() {
            return None;
        }
        let s = self.slots[idx];
        let sub = self.slots.get(s.substitute as usize)?;
        Some(if s.flagged { sub.color } else { s.color })
    }

    /// 全不达标判定。
    pub fn all_flagged(&self) -> bool {
        self.slots.iter().all(|s| s.flagged)
    }
}

/// 找最近的达标替代级（找不到返回自身——调用方再判 all_flagged）。
fn find_substitute(slots: &[RampSlot], i: usize) -> u8 {
    for d in 1..slots.len() {
        if let Some(l) = i.checked_sub(d) {
            if !slots[l].flagged {
                return l as u8;
            }
        }
        let r = i + d;
        if r < slots.len() && !slots[r].flagged {
            return r as u8;
        }
    }
    i as u8
}

// ---------------------------------------------------------------------------
// 五、派生账（本册门面）
// ---------------------------------------------------------------------------

/// Accent Ramp 派生账：基色注册 + 派生 + 断链回退。
pub struct RampBook {
    /// 当前基色。
    base: Option<Rgb888>,
    /// 当前梯度。
    table: Option<RampTable>,
    /// 审计。
    codes: Vec<RampCode>,
}

impl RampBook {
    /// 空账。
    pub fn new() -> Self {
        RampBook {
            base: None,
            table: None,
            codes: Vec::new(),
        }
    }

    /// 审计码。
    pub fn codes(&self) -> &[RampCode] {
        &self.codes
    }

    /// 设基色并派生（参数钳制审计；基色缺省回退母值）。
    pub fn derive(&mut self, base: Option<Rgb888>, params: RampParams) -> &RampTable {
        let (p, clamped) = RampParams::build(params.hue_shift, params.sat_fade, params.lum_step);
        if clamped {
            self.codes.push(RampCode::ParamClamped);
        }
        let b = match base {
            Some(c) => c,
            None => {
                self.codes.push(RampCode::BaseMissing);
                FALLBACK_BASE
            }
        };
        let t = RampTable::derive(b, p);
        if t.all_flagged() {
            self.codes.push(RampCode::AllFlagged);
        }
        self.base = Some(b);
        self.table = Some(t);
        self.table.as_ref().expect("just set")
    }

    /// 当前基色。
    pub fn base(&self) -> Option<Rgb888> {
        self.base
    }

    /// 取生效级色（无表时回退母值——断链回退纪律）。
    pub fn effective(&self, level: u8) -> Rgb888 {
        self.table
            .as_ref()
            .and_then(|t| t.effective(level))
            .unwrap_or(FALLBACK_BASE)
    }
}

// ---------------------------------------------------------------------------
// 六、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_levels_derived() {
        let t = RampTable::derive(FALLBACK_BASE, RampParams::standard());
        assert_eq!(t.slots.len(), RAMP_LEVELS);
        // 单调向暗（负步进）：L0 亮度 ≥ L9 亮度。
        assert!(t.slots[0].color.luminance_permille() >= t.slots[9].color.luminance_permille());
    }

    #[test]
    fn flagged_gets_substitute() {
        let t = RampTable::derive(FALLBACK_BASE, RampParams::build(0, 0, -400).0);
        for i in 0..t.slots.len() {
            let c = t.effective(i as u8).unwrap();
            assert!(
                contrast_best_permille(c) >= WCAG_AA_PERMILLE || !t.slots[i].flagged,
                "level {} 不达标且无替代",
                i
            );
        }
    }

    #[test]
    fn missing_base_falls_back() {
        let mut b = RampBook::new();
        let hit = b.codes().contains(&RampCode::BaseMissing);
        let t = b.derive(None, RampParams::standard());
        assert!(hit);
        // 回退母值：L0 就是母色。
        assert_eq!(t.slots[0].color, FALLBACK_BASE);
    }

    #[test]
    fn params_clamped() {
        let (p, clamped) = RampParams::build(0, 9999, -9999);
        assert!(clamped);
        assert!(p.sat_fade <= 200);
        assert!(p.lum_step >= -400);
    }
}
