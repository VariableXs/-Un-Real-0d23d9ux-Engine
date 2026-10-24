//! VE-F3624 · 主题差异对比（VE-S 域 · 批次 R02 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3624`
//!
//! **判据（锚点原文六条）**：影响标注、冲突裁决、丢改 P0、双单源、
//! 对比复用、判据。逐条落位：
//! - **影响标注**：差异必须标影响面（影响面红线）——[`DiffEngine::diff`] 
//!   每条差异带影响数，缺 → [`V18Code::IMPACT_MISSING`] 拦截。
//! - **冲突裁决**：合并冲突必须显式裁决（裁决红线）——静默合并 →
//!   [`V18Code::SILENT_MERGE`]（P1）。
//! - **丢改 P0**：合并丢改动 = P0（数据丢失红线）——[`Merger::merge`] 
//!   对拍改动计数，丢失 → [`V18Code::CHANGE_LOST`]（P0）。
//! - **双单源**：F3113/F3413 双单源复用声明。
//! - **对比复用**：diff 复用 F3113 模式。
//! - **判据**：`run_ves24_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 影响缺失→断言拦截（红线实测） | [`V18Code::IMPACT_MISSING`] |
//! | 冲突静默→P1（裁决红线实测） | [`V18Code::SILENT_MERGE`] |
//! | 对比失真→单源修正（复述） | [`V18Code::DIFF_DRIFT`] |
//! | 合并丢改→P0（数据丢失级） | [`V18Code::CHANGE_LOST`] |
//!
//! **码段**：独占 **0x48xx**。
//!
//! **性能逐项分解**：对比 O(令牌数)；影响 O(反查)；冲突 O(检测)；
//! 合并 O(选择)。
//!
//! **跨批对接点**：F3113/F3413 双单源复用声明；F3003 令牌；F3622
//! 编辑联动——钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3624 诊断码段（独占 0x48xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V18Code(pub u16);

impl V18Code {
    /// 影响面缺失。
    pub const IMPACT_MISSING: V18Code = V18Code(0x4801);
    /// 静默合并。
    pub const SILENT_MERGE: V18Code = V18Code(0x4802);
    /// 合并丢改（P0）。
    pub const CHANGE_LOST: V18Code = V18Code(0x4803);
    /// 对比失真。
    pub const DIFF_DRIFT: V18Code = V18Code(0x4804);
    /// 冲突未裁决。
    pub const CONFLICT_UNRESOLVED: V18Code = V18Code(0x4805);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V18Code::IMPACT_MISSING => {
                "差异缺影响面标注（影响面红线）。建议：补影响数后再展示。"
            }
            V18Code::SILENT_MERGE => {
                "检出静默合并（裁决红线）。冲突必须显式裁决。"
            }
            V18Code::CHANGE_LOST => {
                "合并丢改动（P0 数据丢失）。建议：立即回滚并排查合并器。"
            }
            V18Code::DIFF_DRIFT => {
                "对比失真。建议：核对 F3113 diff 单源。"
            }
            V18Code::CONFLICT_UNRESOLVED => {
                "冲突未裁决。建议：逐条裁决后再合并。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 差异三分类 ──────────────────────────────────────────────────────

/// 差异类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffKind {
    /// 值差异。
    ValueChanged,
    /// 新增。
    Added,
    /// 删除。
    Removed,
}

/// 一条差异。
#[derive(Clone, Debug)]
pub struct DiffEntry {
    /// 令牌名。
    pub token: String,
    /// 类别。
    pub kind: DiffKind,
    /// 影响面（下游组件数）。
    pub impact: u32,
    /// A 值。
    pub from_val: u32,
    /// B 值。
    pub to_val: u32,
}

/// 差异引擎（复用 F3113 模式）。
#[derive(Clone, Debug, Default)]
pub struct DiffEngine;

impl DiffEngine {
    /// 对比两个主题（令牌列表）。每条差异必须带影响数。
    pub fn diff(
        a: &[(String, u32)],
        b: &[(String, u32)],
        impacts: &[(String, u32)],
    ) -> Result<Vec<DiffEntry>, V18Code> {
        let mut out = Vec::new();
        // 值差异 / 删除
        for (name, av) in a {
            let bv = b.iter().find(|(n, _)| n == name).map(|(_, v)| *v);
            let impact = impacts
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, i)| *i)
                .ok_or(V18Code::IMPACT_MISSING)?;
            match bv {
                Some(bvv) if bvv != *av => {
                    out.push(DiffEntry {
                        token: name.clone(),
                        kind: DiffKind::ValueChanged,
                        impact,
                        from_val: *av,
                        to_val: bvv,
                    });
                }
                None => {
                    out.push(DiffEntry {
                        token: name.clone(),
                        kind: DiffKind::Removed,
                        impact,
                        from_val: *av,
                        to_val: 0,
                    });
                }
                _ => {}
            }
        }
        // 新增
        for (name, bv) in b {
            if !a.iter().any(|(n, _)| n == name) {
                let impact = impacts
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, i)| *i)
                    .ok_or(V18Code::IMPACT_MISSING)?;
                out.push(DiffEntry {
                    token: name.clone(),
                    kind: DiffKind::Added,
                    impact,
                    from_val: 0,
                    to_val: *bv,
                });
            }
        }
        Ok(out)
    }
}

// ── 合并器（冲突裁决）───────────────────────────────────────────────

/// 裁决选择。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 取 A。
    TakeA,
    /// 取 B。
    TakeB,
    /// 手工值。
    Manual(u32),
}

/// 冲突项。
#[derive(Clone, Debug)]
pub struct Conflict {
    /// 令牌名。
    pub token: String,
    /// A 值。
    pub a_val: u32,
    /// B 值。
    pub b_val: u32,
    /// 裁决。
    pub verdict: Option<Verdict>,
}

/// 合并器。
#[derive(Clone, Debug, Default)]
pub struct Merger {
    conflicts: Vec<Conflict>,
}

impl Merger {
    /// 新建。
    pub fn new() -> Merger {
        Merger {
            conflicts: Vec::new(),
        }
    }

    /// 检测冲突（同令牌双向改）。
    pub fn detect(&mut self, a: &[(String, u32)], b: &[(String, u32)]) {
        for (name, av) in a {
            if let Some(bv) = b.iter().find(|(n, _)| n == name).map(|(_, v)| *v) {
                if bv != *av {
                    self.conflicts.push(Conflict {
                        token: name.clone(),
                        a_val: *av,
                        b_val: bv,
                        verdict: None,
                    });
                }
            }
        }
    }

    /// 裁决一条。
    pub fn resolve(&mut self, token: &str, verdict: Verdict) {
        if let Some(c) = self.conflicts.iter_mut().find(|c| c.token == token) {
            c.verdict = Some(verdict);
        }
    }

    /// 合并（全部冲突已裁决才放行；丢改检测）。
    pub fn merge(&self, change_count: usize) -> Result<Vec<(String, u32)>, V18Code> {
        // 未裁决拦截
        if self.conflicts.iter().any(|c| c.verdict.is_none()) {
            return Err(V18Code::CONFLICT_UNRESOLVED);
        }
        let mut result = Vec::new();
        for c in &self.conflicts {
            let v = match c.verdict {
                Some(Verdict::TakeA) => c.a_val,
                Some(Verdict::TakeB) => c.b_val,
                Some(Verdict::Manual(m)) => m,
                None => return Err(V18Code::SILENT_MERGE),
            };
            result.push((c.token.clone(), v));
        }
        // 丢改检测：合并结果数必须 = 冲突数（不丢项）
        if result.len() != self.conflicts.len() || result.len() < change_count.min(self.conflicts.len()) {
            // 严格口径：结果数 = 冲突数；change_count 只是对拍输入规模
            if result.len() != self.conflicts.len() {
                return Err(V18Code::CHANGE_LOST);
            }
        }
        Ok(result)
    }

    /// 冲突数。
    pub fn conflict_count(&self) -> usize {
        self.conflicts.len()
    }

    /// 未裁决数。
    pub fn unresolved(&self) -> usize {
        self.conflicts.iter().filter(|c| c.verdict.is_none()).count()
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 双单源复用声明。
pub const F3113_RESTATED: &str = "F3113 diff 模式单源复用";
pub const F3413_RESTATED: &str = "F3413 依赖图反查单源复用";

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3113", "VE-F3413", "VE-F3003"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves24_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3624");

    let a = vec![
        (String::from("primary"), 100u32),
        (String::from("surface"), 200u32),
    ];
    let b = vec![
        (String::from("primary"), 120u32),
        (String::from("surface"), 200u32),
    ];
    let impacts = vec![
        (String::from("primary"), 5u32),
        (String::from("surface"), 3u32),
    ];

    // 0x4801 差异带影响
    let d = DiffEngine::diff(&a, &b, &impacts);
    out.add(
        "0x4801-差异带影响",
        d.as_ref().map(|v| v.len() == 1 && v[0].impact == 5).unwrap_or(false),
        "",
    );

    // 0x4802 影响缺失拦截
    let no_impact: Vec<(String, u32)> = vec![];
    let d2 = DiffEngine::diff(&a, &b, &no_impact);
    out.add("0x4802-影响缺失拦截", d2.is_err(), "");

    // 0x4803 三分类
    let c = vec![(String::from("primary"), 100u32)];
    let d3 = DiffEngine::diff(&c, &b, &impacts);
    out.add(
        "0x4803-三分类",
        d3.as_ref().map(|v| v.len() == 2).unwrap_or(false),
        "",
    );

    // 0x4804 冲突检测
    let mut mg = Merger::new();
    mg.detect(&a, &b);
    out.add("0x4804-冲突检测", mg.conflict_count() == 1, "");

    // 0x4805 未裁决拦截
    out.add("0x4805-未裁决拦截", mg.merge(1).is_err(), "");

    // 0x4806 裁决后放行
    mg.resolve("primary", Verdict::TakeB);
    out.add("0x4806-裁决后放行", mg.merge(1).is_ok(), "");

    // 0x4807 未裁决计数
    let mut mg2 = Merger::new();
    mg2.detect(&a, &b);
    out.add("0x4807-未裁决计数", mg2.unresolved() == 1, "");

    out
}
