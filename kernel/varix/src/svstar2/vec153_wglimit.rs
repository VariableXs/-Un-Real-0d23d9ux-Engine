//! VE-F0553 · WGSL 限制清单
//!
//! WGSL 后端限制显性化清单：语言级（无预处理器与宏、无 include、递归禁用、
//! const_assert 表达力边界）、生态级（tint/naga 扩展差异登记）、工具级（仅文本格式）。
//! 每项限制三段式登记：现象、影响面、规避或前置化解法；预留态：生态版本升级
//! 可能消除个别限制，清单随版本复审。
//!
//! 跨批对接：上游 F0412 宏展开前置化解、F0474 链接器；下游 F0554/F0555 引用、F0560 收口。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 限制层级
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LimitLevel {
    /// 语言级：WGSL 规范固有
    Language,
    /// 生态级：校验器实现差异
    Ecosystem,
    /// 工具级：格式与工具链
    Tooling,
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

/// 三段式条目：现象、影响面、规避或前置化解法
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LimitEntry {
    /// 限制编号
    pub id: u32,
    pub level: LimitLevel,
    pub source: SourceColor,
    /// 一段：现象描述
    pub symptom: String,
    /// 二段：影响面（涉及的前序卡号或机制）
    pub impact: String,
    /// 三段：规避或前置化解法（可为空 = 无解，仅显性）
    pub workaround: Option<String>,
    /// 预留态：可能随生态版本升级消除
    pub upgradable: bool,
    /// 登记时的生态版本基线
    pub eco_baseline: u32,
}

impl LimitEntry {
    pub fn new(id: u32, level: LimitLevel, source: SourceColor, symptom: &str, impact: &str) -> Self {
        LimitEntry {
            id,
            level,
            source,
            symptom: symptom.to_string(),
            impact: impact.to_string(),
            workaround: None,
            upgradable: false,
            eco_baseline: 1,
        }
    }

    pub fn workaround(mut self, w: &str) -> Self {
        self.workaround = Some(w.to_string());
        self
    }

    pub fn upgradable(mut self) -> Self {
        self.upgradable = true;
        self
    }

    /// 三段式全覆盖校验：现象与影响必填，无解条目必须显性标注
    pub fn complete(&self) -> bool {
        !self.symptom.is_empty() && !self.impact.is_empty()
    }
}

/// 限制清单（三段式条目容器）
#[derive(Clone, Debug, Default)]
pub struct LimitBook {
    entries: Vec<LimitEntry>,
    /// 版本复审记录
    reviews: Vec<(u32, u32)>,
}

impl LimitBook {
    pub fn new() -> Self {
        LimitBook::default()
    }

    pub fn register(&mut self, e: LimitEntry) -> bool {
        if self.entries.iter().any(|x| x.id == e.id) {
            return false;
        }
        self.entries.push(e);
        true
    }

    pub fn get(&self, id: u32) -> Option<&LimitEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 生态双登记校验：生态级限制必须逐校验器登记差异说明
    pub fn ecosystem_duality(&self) -> bool {
        self.entries
            .iter()
            .filter(|e| e.level == LimitLevel::Ecosystem)
            .all(|e| e.impact.contains("tint") || e.impact.contains("naga"))
    }

    /// 前置化解可执行校验：标注前置化解的条目，规避必须指向实际机制
    pub fn pre_resolvable(&self) -> bool {
        self.entries.iter().all(|e| match &e.workaround {
            None => true,
            Some(w) => w.contains("前置") || w.contains("展开") || w.contains("合并") || w.contains("判定"),
        })
    }

    /// 版本复审：预留态条目随版本触发复审；返回可消除条目
    pub fn review(&mut self, eco_version: u32) -> Vec<u32> {
        let mut removed = Vec::new();
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if e.upgradable && eco_version > e.eco_baseline && e.level == LimitLevel::Ecosystem {
                self.reviews.push((e.id, eco_version));
                removed.push(e.id);
                self.entries.remove(i);
            } else {
                i += 1;
            }
        }
        removed
    }

    /// 清单更新留痕：复审记录数
    pub fn review_count(&self) -> usize {
        self.reviews.len()
    }

    /// 限制与规范冲突 → 以规范为准并登记偏差（返回偏差登记数）
    pub fn reconcile(&mut self, spec_ver: u32) -> u32 {
        let mut n = 0u32;
        for e in self.entries.iter_mut() {
            if e.level == LimitLevel::Language && e.source == SourceColor::ToolDiff {
                e.source = SourceColor::SpecInherent;
                e.eco_baseline = spec_ver;
                n += 1;
            }
        }
        n
    }
}

/// 构建语言级标准限制四条（F0412 宏 / F0474 include / 递归 / const_assert）
pub fn seed_language_limits() -> Vec<LimitEntry> {
    vec![
        LimitEntry::new(1, LimitLevel::Language, SourceColor::SpecInherent, "无预处理器与宏", "F0412 宏体系")
            .workaround("宏展开必须前置：进后端前由 F0412 完成"),
        LimitEntry::new(2, LimitLevel::Language, SourceColor::SpecInherent, "无 include 机制", "F0474 多文件合并")
            .workaround("多文件合并由链接器 F0474 前置完成"),
        LimitEntry::new(3, LimitLevel::Language, SourceColor::SpecInherent, "递归禁用", "IR 递归函数")
            .workaround("IR 递归函数须先做可展开判定"),
        LimitEntry::new(4, LimitLevel::Language, SourceColor::SpecInherent, "const_assert 表达力边界", "静态断言受限"),
    ]
}

pub fn run_vec153_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec153");
    // 族一：三段式条目
    let e = LimitEntry::new(10, LimitLevel::Language, SourceColor::SpecInherent, "无宏", "F0412").workaround("前置展开");
    s.add("三段式-完整", e.complete(), "full");
    let empty = LimitEntry::new(11, LimitLevel::Language, SourceColor::ToolDiff, "", "x");
    s.add("三段式-缺现象不完整", !empty.complete(), "no-sym");
    s.add("条目-预留态标注", e.upgradable == false && e.clone().upgradable().upgradable, "flag");
    // 族二：登记去重
    let mut book = LimitBook::new();
    for le in seed_language_limits() {
        s.add("登记-语言级条目", book.register(le), "reg");
    }
    s.add("登记-重复拒绝", !book.register(LimitEntry::new(1, LimitLevel::Language, SourceColor::SpecInherent, "x", "y")), "dup");
    s.add("登记-计数", book.len() == 4, "n4");
    s.add("登记-查表命中", book.get(3).map(|x| x.symptom == "递归禁用").unwrap_or(false), "hit");
    // 族三：生态双登记
    let mut eco = LimitEntry::new(20, LimitLevel::Ecosystem, SourceColor::ToolDiff, "扩展集不同", "tint 支持 naga 拒绝");
    book.register(eco.clone());
    s.add("生态-双登记通过", book.ecosystem_duality(), "eco-ok");
    eco.impact = "无校验器说明".to_string();
    let mut book2 = LimitBook::new();
    book2.register(eco);
    s.add("生态-未标注不通过", !book2.ecosystem_duality(), "eco-bad");
    // 族四：前置化解可执行
    s.add("前置-化解可执行", book.pre_resolvable(), "pre");
    let mut bad = LimitBook::new();
    bad.register(LimitEntry::new(30, LimitLevel::Language, SourceColor::SpecInherent, "x", "y").workaround("口头承诺"));
    s.add("前置-空话不通过", !bad.pre_resolvable(), "bad");
    s.add("前置-无解显性放行", bad.register(LimitEntry::new(31, LimitLevel::Tooling, SourceColor::SpecInherent, "仅文本格式", "工具链")), "no-wa");
    // 族五：版本复审
    let mut rev = LimitBook::new();
    rev.register(LimitEntry::new(40, LimitLevel::Ecosystem, SourceColor::ToolDiff, "暂缺扩展", "tint/naga 基线1").upgradable());
    rev.register(LimitEntry::new(41, LimitLevel::Language, SourceColor::SpecInherent, "语言级", "规范"));
    s.add("复审-版本未到不消除", rev.review(1).is_empty(), "notyet");
    let removed = rev.review(2);
    s.add("复审-升级消除预留态", removed == vec![40], "rm40");
    s.add("复审-语言级不消除", rev.get(41).is_some(), "keep41");
    s.add("复审-留痕计数", rev.review_count() == 1, "trace");
    // 族六：规范冲突以规范为准
    let mut rc = LimitBook::new();
    rc.register(LimitEntry::new(50, LimitLevel::Language, SourceColor::ToolDiff, "误标生态", "规范"));
    s.add("冲突-纠正计数1", rc.reconcile(9) == 1, "fix1");
    s.add("冲突-纠正后来源正确", rc.get(50).map(|x| x.source == SourceColor::SpecInherent).unwrap_or(false), "fixed");
    s.add("冲突-纠正后基线更新", rc.get(50).map(|x| x.eco_baseline == 9).unwrap_or(false), "base");
    s
}
