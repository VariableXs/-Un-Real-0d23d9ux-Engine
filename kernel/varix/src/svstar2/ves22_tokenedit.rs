//! VE-F3622 · 可视化令牌编辑（VE-S 域 · 批次 R02 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3622`
//!
//! **判据（锚点原文六条）**：控件全覆盖、关系保持、undo 复用、批量一致、
//! 覆盖红线、判据。逐条落位：
//! - **控件全覆盖**：所有令牌类型有对应可视化控件（控件覆盖断言）——
//!   [`ControlMap`] 覆盖 F3003 三族，缺格 → [`V16Code::CONTROL_GAP`]。
//! - **关系保持**：批量操作保持令牌间关系（批量一致红线）——
//!   [`BatchEditor::apply`] 对拍关系约束图，破坏 → [`V16Code::RELATION_BREAK`]。
//! - **undo 复用**：撤销复用 undo 链（粒度 = 令牌编辑）。
//! - **批量一致**：批量改不破坏相对关系（关系保持断言）。
//! - **覆盖红线**：某令牌无编辑控件 = 手改代码 = 放弃可视化承诺。
//! - **判据**：`run_ves22_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 覆盖缺格→补控件（红线实测） | [`V16Code::CONTROL_GAP`] |
//! | 批量破坏关系→断言拦截（红线实测） | [`V16Code::RELATION_BREAK`] |
//! | undo 粒度错→修正（复述） | [`V16Code::UNDO_GRANULARITY`] |
//! | 控件值域错→校验（复用钳制） | [`V16Code::VALUE_CLAMP`] |
//!
//! **码段**：独占 **0x46xx**。
//!
//! **性能逐项分解**：控件 O(1) 映射；批量 O(令牌数)；关系 O(约束图)；
//! undo O(1)。
//!
//! **跨批对接点**：F3003 令牌单源；F3413 依赖图（关系源）；F3049 数字
//! 控件复用——钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3622 诊断码段（独占 0x46xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V16Code(pub u16);

impl V16Code {
    /// 控件覆盖缺格。
    pub const CONTROL_GAP: V16Code = V16Code(0x4601);
    /// 批量破坏关系。
    pub const RELATION_BREAK: V16Code = V16Code(0x4602);
    /// undo 粒度错。
    pub const UNDO_GRANULARITY: V16Code = V16Code(0x4603);
    /// 控件值越界被钳。
    pub const VALUE_CLAMP: V16Code = V16Code(0x4604);
    /// 未知令牌类型。
    pub const TOKEN_UNKNOWN: V16Code = V16Code(0x4605);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V16Code::CONTROL_GAP => {
                "令牌类型缺可视化控件（覆盖红线）。建议：补控件，不许退回手改代码。"
            }
            V16Code::RELATION_BREAK => {
                "批量操作破坏令牌关系。建议：按约束图修正批量参数。"
            }
            V16Code::UNDO_GRANULARITY => {
                "undo 粒度异常。建议：每次令牌编辑 = 一步 undo。"
            }
            V16Code::VALUE_CLAMP => {
                "控件值越界已钳制。建议：在合法值域内调整。"
            }
            V16Code::TOKEN_UNKNOWN => {
                "未知令牌类型。建议：先在 F3003 令牌表登记。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 令牌三族与控件映射 ──────────────────────────────────────────────

/// 令牌族（F3003 三族）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenFamily {
    /// 颜色族。
    Color,
    /// 间距族。
    Spacing,
    /// 字号族。
    TypeScale,
}

impl TokenFamily {
    /// 三族全集。
    pub fn all() -> [TokenFamily; 3] {
        [TokenFamily::Color, TokenFamily::Spacing, TokenFamily::TypeScale]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            TokenFamily::Color => "颜色",
            TokenFamily::Spacing => "间距",
            TokenFamily::TypeScale => "字号",
        }
    }
}

/// 控件类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Control {
    /// 拾色器。
    ColorPicker,
    /// 间距滑杆。
    Slider,
    /// 字号阶拖拽。
    ScaleDrag,
}

/// 控件映射表（令牌族 → 控件）。
#[derive(Clone, Debug, Default)]
pub struct ControlMap {
    entries: Vec<(TokenFamily, Control)>,
}

impl ControlMap {
    /// 全覆盖映射。
    pub fn full() -> ControlMap {
        ControlMap {
            entries: vec![
                (TokenFamily::Color, Control::ColorPicker),
                (TokenFamily::Spacing, Control::Slider),
                (TokenFamily::TypeScale, Control::ScaleDrag),
            ],
        }
    }

    /// 查控件。
    pub fn lookup(&self, family: TokenFamily) -> Option<Control> {
        self.entries
            .iter()
            .find(|(f, _)| *f == family)
            .map(|(_, c)| *c)
    }

    /// 覆盖审计：三族各有控件。
    pub fn audit_coverage(&self) -> Result<(), V16Code> {
        for f in TokenFamily::all() {
            if self.lookup(f).is_none() {
                return Err(V16Code::CONTROL_GAP);
            }
        }
        Ok(())
    }
}

// ── 令牌编辑值域（钳制）─────────────────────────────────────────────

/// 令牌值（整数域，避免浮点）。
#[derive(Clone, Copy, Debug)]
pub struct TokenValue {
    /// 族。
    pub family: TokenFamily,
    /// 数值（色值 0..255 / 间距 0..512 / 字号 8..96）。
    pub raw: u32,
}

impl TokenValue {
    /// 合法值域。
    pub fn clamp_range(family: TokenFamily) -> (u32, u32) {
        match family {
            TokenFamily::Color => (0, 255),
            TokenFamily::Spacing => (0, 512),
            TokenFamily::TypeScale => (8, 96),
        }
    }

    /// 钳制。
    pub fn clamp(&mut self) -> Result<(), V16Code> {
        let (lo, hi) = Self::clamp_range(self.family);
        if self.raw < lo {
            self.raw = lo;
            return Err(V16Code::VALUE_CLAMP);
        }
        if self.raw > hi {
            self.raw = hi;
            return Err(V16Code::VALUE_CLAMP);
        }
        Ok(())
    }
}

// ── 关系约束图（批量一致）───────────────────────────────────────────

/// 关系约束（如 primary 与 surface 色对比度）。
#[derive(Clone, Debug)]
pub struct Relation {
    /// 源令牌名。
    pub from: String,
    /// 目标令牌名。
    pub to: String,
    /// 最小差值（保持相对关系）。
    pub min_delta: i32,
}

/// 关系图。
#[derive(Clone, Debug, Default)]
pub struct RelationGraph {
    relations: Vec<Relation>,
}

impl RelationGraph {
    /// 空图。
    pub fn new() -> RelationGraph {
        RelationGraph {
            relations: Vec::new(),
        }
    }

    /// 加关系。
    pub fn add(&mut self, from: &str, to: &str, min_delta: i32) {
        self.relations.push(Relation {
            from: String::from(from),
            to: String::from(to),
            min_delta,
        });
    }

    /// 批量后关系审计：每条关系的差值仍满足。
    pub fn audit(&self, values: &[(String, i32)]) -> Result<(), V16Code> {
        for r in &self.relations {
            let a = values.iter().find(|(n, _)| *n == r.from).map(|(_, v)| *v);
            let b = values.iter().find(|(n, _)| *n == r.to).map(|(_, v)| *v);
            if let (Some(av), Some(bv)) = (a, b) {
                if (av - bv).abs() < r.min_delta.abs() {
                    return Err(V16Code::RELATION_BREAK);
                }
            }
        }
        Ok(())
    }
}

// ── 批量编辑器 ──────────────────────────────────────────────────────

/// 批量编辑器。
#[derive(Clone, Debug, Default)]
pub struct BatchEditor {
    /// 关系图。
    pub relations: RelationGraph,
    /// undo 链（令牌级粒度）。
    undo_stack: Vec<(String, u32)>,
}

impl BatchEditor {
    /// 新建。
    pub fn new() -> BatchEditor {
        BatchEditor {
            relations: RelationGraph::new(),
            undo_stack: Vec::new(),
        }
    }

    /// 批量偏移同类令牌（保持关系）。
    pub fn apply(
        &mut self,
        names: &[String],
        values: &mut Vec<(String, u32)>,
        delta: i32,
    ) -> Result<(), V16Code> {
        // 保存 undo 点（每个令牌一步）
        for n in names {
            if let Some((_, v)) = values.iter().find(|(nn, _)| nn == n) {
                self.undo_stack.push((n.clone(), *v));
            }
        }
        // 应用偏移
        for n in names {
            if let Some(slot) = values.iter_mut().find(|(nn, _)| nn == n) {
                let new_v = (slot.1 as i32 + delta).max(0) as u32;
                slot.1 = new_v;
            }
        }
        // 关系审计
        let i32_vals: Vec<(String, i32)> = values
            .iter()
            .map(|(n, v)| (n.clone(), *v as i32))
            .collect();
        self.relations.audit(&i32_vals)
    }

    /// undo 一步（令牌级粒度）。
    pub fn undo(&mut self, values: &mut Vec<(String, u32)>) -> bool {
        if let Some((name, old)) = self.undo_stack.pop() {
            if let Some(slot) = values.iter_mut().find(|(n, _)| *n == name) {
                slot.1 = old;
            }
            true
        } else {
            false
        }
    }

    /// undo 链深度。
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3003", "VE-F3413", "VE-F3049"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves22_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3622");

    // 0x4601 控件全覆盖
    let cm = ControlMap::full();
    out.add("0x4601-控件全覆盖", cm.audit_coverage().is_ok(), "");

    // 0x4602 缺格检出
    let cm2 = ControlMap {
        entries: vec![(TokenFamily::Color, Control::ColorPicker)],
    };
    out.add("0x4602-缺格检出", cm2.audit_coverage().is_err(), "");

    // 0x4603 值域钳制
    let mut tv = TokenValue {
        family: TokenFamily::Color,
        raw: 300,
    };
    out.add("0x4603-值域钳制", tv.clamp().is_err() && tv.raw == 255, "");

    // 0x4604 值域内放行
    let mut tv2 = TokenValue {
        family: TokenFamily::Color,
        raw: 128,
    };
    out.add("0x4604-值域内放行", tv2.clamp().is_ok(), "");

    // 0x4605 批量保持关系
    let mut be = BatchEditor::new();
    be.relations.add("a", "b", 10);
    let mut vals = vec![
        (String::from("a"), 100u32),
        (String::from("b"), 80u32),
    ];
    let names = vec![String::from("a"), String::from("b")];
    out.add(
        "0x4605-批量保持关系",
        be.apply(&names, &mut vals, 5).is_ok(),
        "",
    );

    // 0x4606 批量破坏关系
    let mut be2 = BatchEditor::new();
    be2.relations.add("a", "b", 50);
    let mut vals2 = vec![
        (String::from("a"), 100u32),
        (String::from("b"), 99u32),
    ];
    let names2 = vec![String::from("a")];
    // a 变 105，b 99，差 6 < 50 → 破坏
    out.add(
        "0x4606-批量破坏关系",
        be2.apply(&names2, &mut vals2, 5).is_err(),
        "",
    );

    // 0x4607 undo 粒度
    let mut be3 = BatchEditor::new();
    let mut vals3 = vec![(String::from("x"), 10u32)];
    let names3 = vec![String::from("x")];
    be3.apply(&names3, &mut vals3, 5).ok();
    out.add(
        "0x4607-undo粒度",
        be3.undo_depth() == 1 && be3.undo(&mut vals3) && vals3[0].1 == 10,
        "",
    );

    out
}
