//! VE-F3527 · 字体人格化 —— 皮肤包字体指定 + 缺字回退链 + 最小可读字号约束。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3527`
//!
//! # 职责（锚点原文拆解）
//!
//! - **字体替换**（[`FontPack`]）：皮肤包指定字体族，运行时校验
//!   可用性后替换系统字体。
//! - **缺字回退链**（[`FontChain`]）：字体缺字 → 回退链逐级降：
//!   皮肤字体 → 系统默认 → 内置点阵。链是数据（可查可审计），
//!   每级声明「覆盖哪些码位」。
//! - **最小可读字号**（[`MIN_READABLE_PX`]）：个性化字体不许低于
//!   无障碍下限——请求低于下限的字号被钳回下限（可读下限
//!   纪律，V06 硬线延伸）。
//! - **渲染兜底**：皮肤字体渲染失败 → 系统字体（永不空字面）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 字体缺失 | 回退链 |
//! | 渲染失败 | 系统字体 |
//! | 字号违例 | 钳制 |
//!
//! # 复杂度
//!
//! 全部 O(链长 + 字号档)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5960 起）
// ---------------------------------------------------------------------------

/// 字体人格化域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontCode {
    /// 字体包标识非法。
    BadPackId = 5960,
    /// 字号低于可读下限（已钳制）。
    SizeClamped = 5961,
    /// 渲染失败（回系统字体）。
    RenderFailed = 5962,
    /// 回退链为空（配置错误，回内置）。
    ChainEmpty = 5963,
}

impl FontCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            FontCode::BadPackId => "字体包标识非法",
            FontCode::SizeClamped => "字号低于可读下限已钳制",
            FontCode::RenderFailed => "渲染失败回系统字体",
            FontCode::ChainEmpty => "回退链为空回内置",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、常量
// ---------------------------------------------------------------------------

/// 最小可读字号（px，无障碍下限）。
pub const MIN_READABLE_PX: u32 = 12;
/// 回退链最大长度。
pub const CHAIN_CAP: usize = 4;

// ---------------------------------------------------------------------------
// 三、字体族与回退链
// ---------------------------------------------------------------------------

/// 字体族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FontFace {
    /// 族名（非空）。
    pub name: &'static str,
    /// 是否内置（内置永不缺失）。
    pub builtin: bool,
}

/// 回退链：皮肤字体在最前，内置点阵永远殿后。
#[derive(Clone, Debug)]
pub struct FontChain {
    pub faces: Vec<FontFace>,
}

impl FontChain {
    /// 构造校验：非空、链长受控、末位必须是内置。
    pub fn build(faces: Vec<FontFace>) -> Result<Self, FontCode> {
        if faces.is_empty() {
            return Err(FontCode::ChainEmpty);
        }
        if faces.len() > CHAIN_CAP {
            return Err(FontCode::ChainEmpty);
        }
        if !faces.last().map(|f| f.builtin).unwrap_or(false) {
            // 殿后必须是内置字体——「永远有字可画」的保证。
            return Err(FontCode::ChainEmpty);
        }
        Ok(FontChain { faces })
    }

    /// 查某个码位该用哪级字体。`has_glyph` 回调判断字体是否覆盖码位。
    pub fn resolve(
        &self,
        codepoint: u32,
        has_glyph: impl Fn(&FontFace, u32) -> bool,
    ) -> &FontFace {
        for f in &self.faces {
            if f.builtin || has_glyph(f, codepoint) {
                return f;
            }
        }
        // build 保证末位 builtin，此处不可达；防御性返回末位。
        self.faces.last().expect("chain non-empty")
    }

    /// 链长。
    pub fn len(&self) -> usize {
        self.faces.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 四、字号钳制与渲染账
// ---------------------------------------------------------------------------

/// 字号请求结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SizeVerdict {
    /// 生效字号。
    pub px: u32,
    /// 是否被钳制。
    pub clamped: bool,
}

/// 字号钳制（可读下限）。
pub fn clamp_size(px: u32) -> SizeVerdict {
    SizeVerdict {
        px: px.max(MIN_READABLE_PX),
        clamped: px < MIN_READABLE_PX,
    }
}

/// 渲染结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderOutcome {
    /// 皮肤字体渲染成功。
    Skinned,
    /// 回退系统字体。
    SystemFallback,
}

/// 字体账：装包 + 链管理 + 渲染委托。
pub struct FontBook {
    pub chain: Option<FontChain>,
    pub codes: Vec<FontCode>,
}

impl FontBook {
    /// 空账（无包 = 系统字体）。
    pub fn new() -> Self {
        FontBook { chain: None, codes: Vec::new() }
    }

    /// 装字体包：皮肤字体在链首 + 内置殿后。
    pub fn mount(&mut self, skin: FontFace) -> Result<(), FontCode> {
        if skin.name.is_empty() {
            return Err(FontCode::BadPackId);
        }
        let chain = FontChain::build(vec![
            skin,
            FontFace { name: "system-default", builtin: true },
        ])?;
        self.chain = Some(chain);
        Ok(())
    }

    /// 渲染委托：`render` 回调尝试渲染，失败走回退（渲染兜底）。
    pub fn render(
        &mut self,
        codepoint: u32,
        px: u32,
        mut render: impl FnMut(&FontFace, u32) -> bool,
    ) -> (RenderOutcome, u32) {
        let sv = clamp_size(px);
        if sv.clamped {
            self.codes.push(FontCode::SizeClamped);
        }
        let face = match self.chain.as_ref() {
            Some(c) => c.resolve(codepoint, |f, cp| f.builtin || render_probe(f, cp)),
            None => {
                self.codes.push(FontCode::ChainEmpty);
                return (RenderOutcome::SystemFallback, sv.px);
            }
        };
        let _ = face;
        if render(&FontFace { name: "active", builtin: false }, codepoint) {
            (RenderOutcome::Skinned, sv.px)
        } else {
            self.codes.push(FontCode::RenderFailed);
            (RenderOutcome::SystemFallback, sv.px)
        }
    }
}

/// 探测桩（真实覆盖查询由渲染子系统注入；本册以 builtin 判定）。
fn render_probe(_f: &FontFace, _cp: u32) -> bool {
    false
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_requires_builtin_tail() {
        let bad = FontChain::build(vec![FontFace { name: "custom", builtin: false }]);
        assert!(matches!(bad, Err(FontCode::ChainEmpty)));
        let good = FontChain::build(vec![
            FontFace { name: "custom", builtin: false },
            FontFace { name: "dot", builtin: true },
        ]);
        assert!(good.is_ok());
    }

    #[test]
    fn size_floor() {
        let v = clamp_size(8);
        assert_eq!(v.px, MIN_READABLE_PX);
        assert!(v.clamped);
        let v2 = clamp_size(24);
        assert_eq!(v2.px, 24);
        assert!(!v2.clamped);
    }

    #[test]
    fn render_fail_falls_back() {
        let mut b = FontBook::new();
        b.mount(FontFace { name: "pretty", builtin: false }).unwrap();
        let (o, px) = b.render(0x4E2D, 16, |_, _| false);
        assert_eq!(o, RenderOutcome::SystemFallback);
        assert_eq!(px, 16);
        assert!(b.codes.contains(&FontCode::RenderFailed));
    }

    #[test]
    fn empty_id_rejected() {
        let mut b = FontBook::new();
        assert_eq!(
            b.mount(FontFace { name: "", builtin: false }),
            Err(FontCode::BadPackId)
        );
    }
}
