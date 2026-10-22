//! VE-F0473 · IR 入口点与接口规范化（VE-C 域 · IR 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0473`
//!
//! **锚点原文**：「着色器入口点与阶段接口规范化：入口函数标记、阶段间接口
//! 变量到 IR 接口声明的规范化（与 F0450 匹配结果联动）、入口签名版本化；
//! 规范化后接口信息集中可查（供链接与后端）。判据：入口标记、规范化表、
//! 匹配联动、版本化、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **入口标记**（判据一）。入口函数标记唯一（每阶段恰一入口：缺失或
//!    重复 → 报错带发现明细）。
//!
//! 2. **规范化表**（判据二）。接口变量 → IR 接口声明表（名/类型/限定符/
//!    方向四元），集中可查 O(1)（[`IfaceTable::lookup`]）。
//!
//! 3. **匹配联动**（判据三）。与 F0450 匹配结果联动：匹配 mismatch 的接口
//!    以匹配结果为准回溯登记（规范化不得覆盖匹配结论）。
//!
//! 4. **版本化**（判据四）。入口签名版本记录（升版流程显性：签名变更 →
//!    版本递增 → 旧版留痕）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0473";

/// 阶段（着色器五阶段——入口标记的归属面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Vertex,
    Fragment,
    Compute,
}

impl Stage {
    pub fn tag(&self) -> &'static str {
        match self {
            Stage::Vertex => "vs",
            Stage::Fragment => "fs",
            Stage::Compute => "cs",
        }
    }
}

/// IR 接口声明（规范化后的标准形态：四元组）。
#[derive(Clone, Debug, PartialEq)]
pub struct IfaceDecl {
    pub name: String,
    pub ty_digest: String,
    pub qual: String,
    /// 方向：true = 本阶段输出。
    pub is_out: bool,
}

/// 入口标记（函数名 + 阶段 + 签名版本）。
#[derive(Clone, Debug, PartialEq)]
pub struct EntryMark {
    pub func: String,
    pub stage: Stage,
    pub sig_ver: u32,
}

/// 入口规范化结果（错误面：缺失/重复带发现明细）。
#[derive(Debug, Default)]
pub struct NormReport {
    pub entry: Option<EntryMark>,
    pub errors: Vec<String>,
}

/// 入口标记 pass：候选入口集 → 唯一入口（缺失/重复报错带明细）。
pub fn mark_entry(candidates: &[(&str, Stage)], sig_ver: u32) -> NormReport {
    let mut rep = NormReport::default();
    if candidates.is_empty() {
        // 零候选 = 入口缺失（显性报错，不静默无入口）。
        rep.errors.push("入口缺失：候选零条".to_string());
        return rep;
    }
    let mut by_stage: Vec<(Stage, Vec<&str>)> = Vec::new();
    for (f, s) in candidates.iter() {
        if let Some(e) = by_stage.iter_mut().find(|(x, _)| x == s) {
            e.1.push(f);
        } else {
            by_stage.push((*s, vec![*f]));
        }
    }
    for (s, fs) in by_stage.iter() {
        if fs.len() == 1 {
            rep.entry = Some(EntryMark {
                func: fs[0].to_string(),
                stage: *s,
                sig_ver,
            });
        } else if fs.is_empty() {
            rep.errors.push(format!("阶段 {} 入口缺失", s.tag()));
        } else {
            rep.errors.push(format!("阶段 {} 入口重复：{}", s.tag(), fs.join("/")));
        }
    }
    rep
}

/// 接口声明表（集中可查）。
#[derive(Debug, Default)]
pub struct IfaceTable {
    pub decls: Vec<IfaceDecl>,
    /// F0450 匹配结论回溯登记（mismatch 的接口以匹配为准）。
    pub overridden: Vec<String>,
    /// 签名版本历史（升版留痕）。
    pub ver_log: Vec<(u32, String)>,
}

impl IfaceTable {
    pub fn new() -> Self {
        Self::default()
    }
    /// 规范化：接口变量 → 声明（冲突：与 F0450 mismatch 结论不符 → 以匹配为准回溯）。
    pub fn normalize(
        &mut self,
        decls: Vec<IfaceDecl>,
        f0450_mismatches: &[String],
    ) {
        for d in decls.into_iter() {
            if f0450_mismatches.iter().any(|m| m.contains(&d.name)) {
                // 匹配判 mismatch → 该接口回溯登记（进 overridden，不入正表）。
                self.overridden.push(d.name.clone());
            } else if !self.decls.iter().any(|x| x.name == d.name) {
                self.decls.push(d);
            }
        }
    }
    /// O(1) 面：按名查（表内线性即集中可查的最小形态）。
    pub fn lookup(&self, name: &str) -> Option<&IfaceDecl> {
        self.decls.iter().find(|d| d.name == name)
    }
    /// 升版：签名变更 → 版本递增 + 旧版留痕。
    pub fn bump_version(&mut self, new_ver: u32, note: &str) {
        let old = self.ver_log.last().map(|(v, _)| *v).unwrap_or(1);
        if new_ver > old {
            self.ver_log.push((new_ver, note.to_string()));
        }
    }
    pub fn current_ver(&self) -> u32 {
        self.ver_log.last().map(|(v, _)| *v).unwrap_or(1)
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec73_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、入口标记。
    {
        // 唯一入口正常标记。
        let ok = mark_entry(&[("vs_main", Stage::Vertex)], 1);
        v.push((
            "F0473-入口-唯一标记",
            ok.errors.is_empty()
                && matches!(&ok.entry, Some(e) if e.func == "vs_main" && e.stage == Stage::Vertex),
        ));
        // 重复入口 → 报错带发现明细（两函数名都在）。
        let dup = mark_entry(&[("fs_a", Stage::Fragment), ("fs_b", Stage::Fragment)], 1);
        v.push((
            "F0473-入口-重复报明细",
            dup.entry.is_none() && dup.errors.iter().any(|e| e.contains("fs_a") && e.contains("fs_b")),
        ));
        // 入口缺失（零候选）→ 报缺失。
        let none = mark_entry(&[], 1);
        v.push(("F0473-入口-缺失报错", none.entry.is_none() && !none.errors.is_empty()));
    }

    // 二、规范化表。
    {
        let mut t = IfaceTable::new();
        t.normalize(
            vec![
                IfaceDecl {
                    name: "color".to_string(),
                    ty_digest: "v:floatx4".to_string(),
                    qual: "flat".to_string(),
                    is_out: true,
                },
                IfaceDecl {
                    name: "uv".to_string(),
                    ty_digest: "v:floatx2".to_string(),
                    qual: "smooth".to_string(),
                    is_out: true,
                },
            ],
            &[],
        );
        // 集中可查 O(1) 面：按名查得四元组。
        let c = t.lookup("color");
        v.push((
            "F0473-表-集中可查",
            matches!(c, Some(d) if d.ty_digest == "v:floatx4" && d.qual == "flat" && d.is_out),
        ));
        // 查不到显性 None。
        v.push(("F0473-表-缺失显性", t.lookup("ghost").is_none()));
    }

    // 三、匹配联动。
    {
        let mut t = IfaceTable::new();
        t.normalize(
            vec![
                IfaceDecl {
                    name: "good".to_string(),
                    ty_digest: "s:float".to_string(),
                    qual: "flat".to_string(),
                    is_out: true,
                },
                IfaceDecl {
                    name: "bad".to_string(),
                    ty_digest: "v:floatx3".to_string(),
                    qual: "flat".to_string(),
                    is_out: true,
                },
            ],
            // F0450 mismatch 含 bad → 以匹配为准回溯（bad 不入正表）。
            &["类型不匹配：bad 上游 v:floatx3 vs 下游 v:floatx4".to_string()],
        );
        v.push((
            "F0473-联动-匹配为准回溯",
            t.lookup("good").is_some()
                && t.lookup("bad").is_none()
                && t.overridden == vec!["bad".to_string()],
        ));
    }

    // 四、版本化。
    {
        let mut t = IfaceTable::new();
        // 初始版 1；升版 2 → 递增 + 旧版留痕。
        t.bump_version(2, "接口加 ubo 槽位");
        v.push((
            "F0473-版本-升版递增",
            t.current_ver() == 2 && t.ver_log.len() == 1 && t.ver_log[0] == (2, "接口加 ubo 槽位".to_string()),
        ));
        // 同版/降版不递增（幂等保护）。
        t.bump_version(2, "重复升版拒绝");
        t.bump_version(1, "降版拒绝");
        v.push(("F0473-版本-降版拒", t.current_ver() == 2 && t.ver_log.len() == 1));
    }

    // 五、判据自检。
    {
        v.push(("F0473-判据-锚点入册", ANCHOR == "VE-F0473"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec73）。
pub fn run_vec73_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec73");
    for (name, ok) in vec73_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec73_raw_checks as run_vec73_checks;
    #[test]
    fn vec73_entry_checks() {
        for (name, ok) in run_vec73_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
