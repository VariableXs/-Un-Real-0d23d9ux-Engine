//! VE-F0558 · 三后端差异表
//!
//! 特性乘后端矩阵（行为语言特性与 VE 用法，列为 GLSL/HLSL/WGSL，格为支持态加备注：
//! 原生、需变换、不支持加规避链接）；来源三色标注：规范固有差异、驱动或生态工具差异、
//! VE 架构决策强制统一；每行附最小示例与规避方案指针；WGSL 列与 F0553 清单联动去重；
//! 维护：新差异即登记、季度复审、行级一致性抽检。
//!
//! 跨批对接：上游 F0557 框架、F0553 联动；下游 F0578 总对拍扩展 SPIR-V 列。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 后端列
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BeCol {
    Glsl,
    Hlsl,
    Wgsl,
    /// 预留：总对拍扩展列
    Spirv,
}

/// 支持态
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Support {
    /// 原生支持
    Native,
    /// 需变换（VE 侧变换后可用）
    NeedsTransform,
    /// 不支持（必须附规避链接）
    Unsupported,
}

/// 三色来源标注
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceColor {
    /// 规范固有差异
    SpecInherent,
    /// 驱动或生态工具差异
    ToolDiff,
    /// VE 架构决策强制统一
    VePolicy,
}

/// 单元格：支持态 + 备注
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Cell {
    pub support: Support,
    /// 备注 / 规避方案指针（不支持时必填）
    pub note: Option<String>,
}

impl Cell {
    pub fn native() -> Self {
        Cell { support: Support::Native, note: None }
    }

    pub fn transform(n: &str) -> Self {
        Cell { support: Support::NeedsTransform, note: Some(n.to_string()) }
    }

    pub fn unsupported(ptr: &str) -> Self {
        Cell { support: Support::Unsupported, note: Some(ptr.to_string()) }
    }

    /// 不支持条目必须有规避方案指针
    pub fn valid(&self) -> bool {
        match self.support {
            Support::Unsupported => self.note.is_some(),
            _ => true,
        }
    }
}

/// 行：特性 + 三列 + 来源色 + 最小示例
#[derive(Clone, Debug)]
pub struct Row {
    pub feature: String,
    pub cells: [(BeCol, Cell); 3],
    pub source: SourceColor,
    /// 最小示例
    pub example: String,
}

impl Row {
    pub fn new(feature: &str, g: Cell, h: Cell, w: Cell, source: SourceColor, example: &str) -> Self {
        Row {
            feature: feature.to_string(),
            cells: [(BeCol::Glsl, g), (BeCol::Hlsl, h), (BeCol::Wgsl, w)],
            source,
            example: example.to_string(),
        }
    }

    pub fn cell(&self, col: BeCol) -> Option<&Cell> {
        self.cells.iter().find(|(c, _)| *c == col).map(|(_, cell)| cell)
    }

    /// 行级校验：不支持必有规避指针、示例必填
    pub fn valid(&self) -> bool {
        !self.example.is_empty() && self.cells.iter().all(|(_, c)| c.valid())
    }
}

/// 差异矩阵
#[derive(Clone, Debug, Default)]
pub struct DiffMatrix {
    rows: Vec<Row>,
    /// 季度复审记录数
    reviews: u32,
}

impl DiffMatrix {
    pub fn new() -> Self {
        DiffMatrix::default()
    }

    /// 新差异即登记（重复特性名拒绝）
    pub fn register(&mut self, row: Row) -> bool {
        if !row.valid() || self.rows.iter().any(|r| r.feature == row.feature) {
            return false;
        }
        self.rows.push(row);
        true
    }

    pub fn get(&self, feature: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.feature == feature)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 行级一致性抽检：抽样比对行内最小示例与登记的支持态自洽
    pub fn spot_check(&self, sample: usize) -> usize {
        let n = self.rows.len().min(sample);
        let mut bad = 0;
        for r in &self.rows[..n] {
            if !r.valid() {
                bad += 1;
            }
        }
        bad
    }

    /// 季度复审：来源误标修正（示例含 "tool" 但标 SpecInherent → 修正 ToolDiff）
    pub fn quarterly_review(&mut self) -> u32 {
        let mut fixed = 0;
        for r in self.rows.iter_mut() {
            if r.source == SourceColor::SpecInherent && r.example.contains("tool-only") {
                r.source = SourceColor::ToolDiff;
                fixed += 1;
            }
        }
        self.reviews += 1;
        fixed
    }

    pub fn review_count(&self) -> u32 {
        self.reviews
    }

    /// 规避失效告警：规避指针指向的机制名可查（此处简化为非空 + 非占位）
    pub fn stale_workarounds(&self) -> Vec<String> {
        let mut out = Vec::new();
        for r in &self.rows {
            for (_, c) in &r.cells {
                if let Some(n) = &c.note {
                    if n == "TODO" || n.is_empty() {
                        out.push(r.feature.clone());
                    }
                }
            }
        }
        out
    }
}

/// 与 F0553 清单联动去重：WGSL 列引用条目不重复展开
pub fn wgsl_dedup(matrix: &DiffMatrix, f0553_ids: &[u32]) -> bool {
    // 校验：WGSL 列备注若引用 F0553 条目，必须带 "F0553:" 前缀而不是复制内容
    for r in &matrix.rows {
        if let Some(c) = r.cell(BeCol::Wgsl) {
            if let Some(n) = &c.note {
                if n.contains("F0553") && !n.starts_with("F0553:") {
                    return false;
                }
            }
        }
    }
    let _ = f0553_ids;
    true
}

/// 构建标准差异矩阵（三条示范行）
pub fn seed_matrix() -> DiffMatrix {
    let mut m = DiffMatrix::new();
    m.register(Row::new(
        "switch-in-loop",
        Cell::unsupported("F0503: 结构化重建前置"),
        Cell::transform("保持原生"),
        Cell::transform("保持原生"),
        SourceColor::SpecInherent,
        "loop { switch(x) {} }",
    ));
    m.register(Row::new(
        "identifier-case",
        Cell::native(),
        Cell::native(),
        Cell::native(),
        SourceColor::VePolicy,
        "统一小写策略",
    ));
    m.register(Row::new(
        "wave-intrinsic",
        Cell::transform("模拟降级"),
        Cell::transform("模拟降级"),
        Cell::unsupported("tool-only F0553:1"),
        SourceColor::ToolDiff,
        "wave-active-sum",
    ));
    m
}

pub fn run_vec158_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec158");
    // 族一：单元格有效性
    s.add("单元-原生有效", Cell::native().valid(), "nat");
    s.add("单元-变换有效", Cell::transform("x").valid(), "tr");
    s.add("单元-无指针无效", !Cell::unsupported("").valid(), "noptr");
    s.add("单元-有指针有效", Cell::unsupported("F0503").valid(), "ptr");
    // 族二：行级校验
    let m = seed_matrix();
    s.add("行-示例必填", !Row::new("f", Cell::native(), Cell::native(), Cell::native(), SourceColor::SpecInherent, "").valid(), "noex");
    s.add("行-登记三条", m.len() == 3, "n3");
    s.add("行-查表命中", m.get("switch-in-loop").is_some(), "hit");
    s.add("行-三列定位", m.get("identifier-case").map(|r| r.cell(BeCol::Hlsl).map(|c| c.support == Support::Native).unwrap_or(false)).unwrap_or(false), "col");
    // 族三：登记去重与无效拒绝
    let mut m2 = DiffMatrix::new();
    let dup = Row::new("f", Cell::native(), Cell::native(), Cell::native(), SourceColor::SpecInherent, "e");
    s.add("登记-首条成功", m2.register(dup.clone()), "reg");
    s.add("登记-重复拒绝", !m2.register(dup), "dup");
    let bad_row = Row::new("g", Cell::native(), Cell::native(), Cell::unsupported(""), SourceColor::SpecInherent, "e");
    s.add("登记-无效拒绝", !m2.register(bad_row), "bad");
    // 族四：三色来源与复审修正
    let mut m3 = seed_matrix();
    s.add("来源-三色齐备", [SourceColor::SpecInherent, SourceColor::ToolDiff, SourceColor::VePolicy].iter().all(|c| m3.rows.iter().any(|r| r.source == *c)), "3c");
    s.add("复审-误标修正", m3.quarterly_review() == 1, "fix");
    s.add("复审-修正后来源", m3.get("wave-intrinsic").map(|r| r.source == SourceColor::ToolDiff).unwrap_or(false), "fixed");
    s.add("复审-留痕", m3.review_count() == 1, "trace");
    // 族五：行级一致性抽检
    s.add("抽检-健康零坏行", m3.spot_check(10) == 0, "ok");
    let mut m4 = DiffMatrix::new();
    m4.register(Row::new("bad", Cell::native(), Cell::native(), Cell::unsupported("TODO"), SourceColor::SpecInherent, "x"));
    s.add("抽检-坏行检出", m4.spot_check(5) == 1, "bad");
    s.add("抽检-规避失效清单", !m4.stale_workarounds().is_empty(), "stale");
    // 族六：WGSL 列联动去重
    s.add("去重-引用前缀合规", wgsl_dedup(&m, &[1]), "dedup");
    let mut m5 = DiffMatrix::new();
    m5.register(Row::new("bad-ref", Cell::native(), Cell::native(), Cell::unsupported("引用 F0553 条目1 未带前缀"), SourceColor::ToolDiff, "x"));
    s.add("去重-未带前缀拒绝", !wgsl_dedup(&m5, &[1]), "nodup");
    s
}
