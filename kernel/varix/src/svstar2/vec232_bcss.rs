//! VE-F0632 混合与 CSS 语义对齐验证（VE-D 域 · 目标 320 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0632`
//!
//! **判据（锚点原文逐条）**：
//! - **术语同构**：mix-blend-mode / background-blend-mode / isolation → VE 语义逐项映射；
//! - **差异显性**：VE 未采纳项显性列表（每项含不采纳理由），不静默偏差；
//! - **参考诚实**：Web 参考渲染不可得时以规范公式金标准替代并显性登记；
//! - **判据**：映射可查、差异可审。

use crate::checks::CheckSet;
use super::ved21_blendreg::ModeKey;

/// CSS 属性面（mix-blend-mode / background-blend-mode 同关键词集；isolation 独立布尔）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CssProp {
    MixBlendMode,
    BackgroundBlendMode,
    Isolation,
}

/// 同构映射行：CSS 关键词 → VE 模式键
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapRow {
    pub keyword: &'static str,
    pub ve: ModeKey,
    /// 该映射适用于哪些属性面
    pub props: u8,
}

/// props 位标
pub const P_MIX: u8 = 1;
pub const P_BG: u8 = 2;

/// 全量映射表（CSS 混合关键词集与 VE 18 语义的规范同构）
pub fn map_table() -> alloc::vec::Vec<MapRow> {
    use ModeKey::*;
    let rows = [
        ("normal", Normal),
        ("multiply", Multiply),
        ("screen", Screen),
        ("overlay", Overlay),
        ("darken", Darken),
        ("lighten", Lighten),
        ("color-dodge", ColorDodge),
        ("color-burn", ColorBurn),
        ("hard-light", HardLight),
        ("soft-light", SoftLight),
        ("difference", Difference),
        ("exclusion", Exclusion),
        ("hue", Hue),
        ("saturation", Saturation),
        ("color", Color),
        ("luminosity", Luminosity),
        ("plus-darker", PlusDarker),
        ("plus-lighter", PlusLighter),
    ];
    rows.iter()
        .map(|&(k, v)| MapRow { keyword: k, ve: v, props: P_MIX | P_BG })
        .collect()
}

/// 差异登记行：VE 未采纳项（显性，不静默偏差）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffRow {
    pub item: &'static str,
    pub reason: &'static str,
}

/// 未采纳清单（每项含理由）
pub fn diff_registry() -> alloc::vec::Vec<DiffRow> {
    alloc::vec![
        DiffRow { item: "isolation:auto", reason: "继承语义依赖 DOM 树——VE 以 F0605 显性 IsoCond 判定替代" },
        DiffRow { item: "background-attachment 混合底固定", reason: "滚动语义属 DOM 合成层——VE 合成域不引入滚动概念" },
    ]
}

/// isolation 映射：CSS isolation:isolate → VE IsoCond（F0605/F0606 语义）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IsoMap {
    /// CSS isolation 显性 isolate
    pub css_isolate: bool,
}

impl IsoMap {
    /// 映射到 VE 隔离条件（isolation:isolate 等价于强制 blend 非恒等触发面）
    pub fn to_ve_cond(self) -> super::vec226_isogrp::IsoCond {
        super::vec226_isogrp::IsoCond { blend_nonnormal: self.css_isolate, ..super::vec226_isogrp::IsoCond::NONE }
    }
}

/// 对齐色卡：CSS 语义关键色卡（规范公式金标准替代 Web 参考——参考诚实）
pub fn css_swatch_set() -> alloc::vec::Vec<(&'static str, f32, f32)> {
    alloc::vec![
        ("normal-identity", 0.5, 0.5),
        ("multiply-black-bg", 0.0, 0.7),
        ("screen-white-bg", 1.0, 0.3),
        ("overlay-mid", 0.5, 0.5),
        ("difference-inverse", 0.25, 0.75),
    ]
}

pub fn run_vec232_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec232");
    // 族一：术语同构
    let tab = map_table();
    s.add("css-映射全量", tab.len() == 18, "18 keywords");
    s.add("css-关键词唯一", {
        let mut u = true;
        for i in 0..tab.len() {
            for j in i + 1..tab.len() {
                if tab[i].keyword == tab[j].keyword {
                    u = false;
                }
            }
        }
        u
    }, "unique kw");
    s.add("css-VE键唯一", {
        let mut u = true;
        for i in 0..tab.len() {
            for j in i + 1..tab.len() {
                if tab[i].ve == tab[j].ve {
                    u = false;
                }
            }
        }
        u
    }, "unique ve");
    // 反查：keyword → ve
    let find = |kw: &str| tab.iter().find(|r| r.keyword == kw).map(|r| r.ve);
    s.add("css-multiply映射", find("multiply") == Some(ModeKey::Multiply), "lookup");
    s.add("css-color-dodge", find("color-dodge") == Some(ModeKey::ColorDodge), "dodge kw");
    s.add("css-plus-lighter", find("plus-lighter") == Some(ModeKey::PlusLighter), "plus lighter");
    s.add("css-未收录", find("vivid-light").is_none(), "not in spec");
    // 族二：属性面
    s.add("css-双属性面", tab.iter().all(|r| r.props & P_MIX != 0 && r.props & P_BG != 0), "mix+bg");
    // 族三：差异显性
    let dif = diff_registry();
    s.add("css-差异条数", dif.len() == 2, "2 diffs");
    s.add("css-理由非空", dif.iter().all(|d| !d.reason.is_empty()), "reasons");
    s.add("css-无静默", dif.iter().any(|d| d.item.contains("isolation")), "isolation explicit");
    // 族四：isolation 映射
    let im = IsoMap { css_isolate: true };
    let cond = im.to_ve_cond();
    s.add("css-isolate→隔离", cond.needs_isolation(), "isolate triggers");
    let im2 = IsoMap { css_isolate: false };
    s.add("css-auto→不强制", !im2.to_ve_cond().needs_isolation(), "auto no force");
    // 族五：对齐色卡
    let sw = css_swatch_set();
    s.add("css-色卡数", sw.len() == 5, "5 swatches");
    s.add("css-命名非空", sw.iter().all(|(n, _, _)| !n.is_empty()), "named");
    s.add("css-值域", sw.iter().all(|&(_, a, b)| (0.0..=1.0).contains(&a) && (0.0..=1.0).contains(&b)), "range");
    // 族六：金标准替代验证（色卡在 oracle 下自洽）
    use super::ved22_separable::{oracle, SepMode};
    let (kw, cb, cs) = sw[1];
    let _ = kw;
    let o = oracle(SepMode::Multiply, cb, cs);
    s.add("css-金标准可算", o.is_finite() && o == 0.0, "multiply black bg → 0");
    let (kw2, cb2, cs2) = sw[2];
    let _ = kw2;
    let o2 = oracle(SepMode::Screen, cb2, cs2);
    s.add("css-screen白底", o2 == 1.0, "screen white bg → 1");
    s
}
