//! VE-F0573 · SPIR-V 限制清单
//!
//! SPIR-V 后端限制显性化清单（三段式清单法与 F0553 家族同构）：语言级（结构化
//! 控制流强制——非结构化区域须先经 F0503 重建；无函数指针与动态调度——间接调用
//! 模式显性不支持；逻辑寻址模型无指针算术；二进制无标准文本格式——反汇编仅作
//! 调试介质）、生态级（spirv-val 与 spirv-opt 各自版本支持范围差异登记、工具版本
//! 冻结策略）、工具级（Word 长度与 ID 空间上边界）。每项三段式。
//!
//! 跨批对接：上游 F0503 重建前置、F0567 工具链；下游 F0580 组收口归档、F0574/F0575 引用。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 限制层级
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Language,
    Ecosystem,
    Tooling,
}

/// 三色来源
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    SpecInherent,
    ToolDiff,
    VePolicy,
}

/// 三段式条目
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    pub id: u32,
    pub level: Level,
    pub source: Source,
    /// 一段：现象
    pub symptom: String,
    /// 二段：影响面
    pub impact: String,
    /// 三段：规避或前置化解法
    pub workaround: Option<String>,
    pub upgradable: bool,
    pub eco_baseline: u32,
}

impl Entry {
    pub fn new(id: u32, level: Level, source: Source, symptom: &str, impact: &str) -> Self {
        Entry { id, level, source, symptom: symptom.to_string(), impact: impact.to_string(), workaround: None, upgradable: false, eco_baseline: 1 }
    }

    pub fn workaround(mut self, w: &str) -> Self {
        self.workaround = Some(w.to_string());
        self
    }

    pub fn upgradable(mut self) -> Self {
        self.upgradable = true;
        self
    }

    pub fn complete(&self) -> bool {
        !self.symptom.is_empty() && !self.impact.is_empty()
    }
}

/// 限制清单容器
#[derive(Clone, Debug, Default)]
pub struct LimitBook {
    entries: Vec<Entry>,
    /// 版本冻结记录
    freezes: Vec<(String, u32)>,
    reviews: Vec<(u32, u32)>,
}

impl LimitBook {
    pub fn new() -> Self {
        LimitBook::default()
    }

    pub fn register(&mut self, e: Entry) -> bool {
        if self.entries.iter().any(|x| x.id == e.id) {
            return false;
        }
        self.entries.push(e);
        true
    }

    pub fn get(&self, id: u32) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 工具版本冻结登记
    pub fn freeze(&mut self, tool: &str, ver: u32) -> bool {
        if tool.is_empty() || self.freezes.iter().any(|(t, _)| t == tool) {
            return false;
        }
        self.freezes.push((tool.to_string(), ver));
        true
    }

    pub fn frozen_ver(&self, tool: &str) -> Option<u32> {
        self.freezes.iter().find(|(t, _)| t == tool).map(|(_, v)| *v)
    }

    /// 生态差异登记校验：生态级条目必须逐工具说明
    pub fn eco_duality(&self) -> bool {
        self.entries
            .iter()
            .filter(|e| e.level == Level::Ecosystem)
            .all(|e| e.impact.contains("spirv-val") || e.impact.contains("spirv-opt"))
    }

    /// 前置化解可执行校验
    pub fn pre_resolvable(&self) -> bool {
        self.entries.iter().all(|e| match &e.workaround {
            None => true,
            Some(w) => w.contains("重建") || w.contains("前置") || w.contains("冻结") || w.contains("上边界") || w.contains("判定"),
        })
    }

    /// 版本复审：预留态条目随生态升级消除
    pub fn review(&mut self, eco_version: u32) -> Vec<u32> {
        let mut removed = Vec::new();
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if e.upgradable && e.level == Level::Ecosystem && eco_version > e.eco_baseline {
                self.reviews.push((e.id, eco_version));
                removed.push(e.id);
                self.entries.remove(i);
            } else {
                i += 1;
            }
        }
        removed
    }

    pub fn review_count(&self) -> usize {
        self.reviews.len()
    }

    /// 限制与规范冲突 → 以规范为准并登记偏差
    pub fn reconcile(&mut self, spec_ver: u32) -> u32 {
        let mut n = 0u32;
        for e in self.entries.iter_mut() {
            if e.level == Level::Language && e.source == Source::ToolDiff {
                e.source = Source::SpecInherent;
                e.eco_baseline = spec_ver;
                n += 1;
            }
        }
        n
    }
}

/// 构建语言级标准限制四条
pub fn seed_language_limits() -> Vec<Entry> {
    vec![
        Entry::new(1, Level::Language, Source::SpecInherent, "结构化控制流强制", "非结构化区域")
            .workaround("非结构化区域须先经 F0503 重建前置"),
        Entry::new(2, Level::Language, Source::SpecInherent, "无函数指针与动态调度", "间接调用模式").workaround("显性不支持判定"),
        Entry::new(3, Level::Language, Source::SpecInherent, "逻辑寻址无指针算术", "地址计算").workaround("展开为逐元素访问前置"),
        Entry::new(4, Level::Language, Source::SpecInherent, "二进制无标准文本格式", "调试介质").workaround("反汇编仅作调试介质冻结"),
    ]
}

pub fn run_vec173_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec173");
    // 族一：三段式
    let e = Entry::new(10, Level::Language, Source::SpecInherent, "结构化强制", "F0503").workaround("前置重建");
    s.add("条目-完整", e.complete(), "ok");
    s.add("条目-空现象不完整", !Entry::new(11, Level::Language, Source::ToolDiff, "", "x").complete(), "empty");
    s.add("条目-预留标注", e.clone().upgradable().upgradable, "flag");
    // 族二：登记
    let mut book = LimitBook::new();
    for le in seed_language_limits() {
        s.add("登记-语言级", book.register(le), "reg");
    }
    s.add("登记-计数", book.len() == 4, "n4");
    s.add("登记-重复拒绝", !book.register(Entry::new(1, Level::Language, Source::SpecInherent, "x", "y")), "dup");
    s.add("登记-查表", book.get(2).map(|x| x.symptom == "无函数指针与动态调度").unwrap_or(false), "hit");
    // 族三：工具版本冻结
    s.add("冻结-登记", book.freeze("spirv-val", 202403), "frz");
    s.add("冻结-同工具重复拒绝", !book.freeze("spirv-val", 202404), "dup");
    s.add("冻结-查询", book.frozen_ver("spirv-val") == Some(202403), "q");
    s.add("冻结-未登记None", book.frozen_ver("naga").is_none(), "none");
    // 族四：生态双登记
    book.register(Entry::new(20, Level::Ecosystem, Source::ToolDiff, "版本支持范围不同", "spirv-val 与 spirv-opt 差异"));
    s.add("生态-双登记通过", book.eco_duality(), "eco");
    let mut bad = LimitBook::new();
    bad.register(Entry::new(21, Level::Ecosystem, Source::ToolDiff, "x", "无工具说明"));
    s.add("生态-未标注不通过", !bad.eco_duality(), "bad");
    // 族五：前置化解
    s.add("前置-化解可执行", book.pre_resolvable(), "pre");
    let mut bad2 = LimitBook::new();
    bad2.register(Entry::new(30, Level::Language, Source::SpecInherent, "x", "y").workaround("空话"));
    s.add("前置-空话不通过", !bad2.pre_resolvable(), "bad2");
    // 族六：版本复审
    let mut rev = LimitBook::new();
    rev.register(Entry::new(40, Level::Ecosystem, Source::ToolDiff, "暂缺", "spirv-opt 基线").upgradable());
    rev.register(Entry::new(41, Level::Language, Source::SpecInherent, "语言级", "规范"));
    s.add("复审-版本未到不消除", rev.review(1).is_empty(), "notyet");
    s.add("复审-升级消除", rev.review(2) == vec![40], "rm");
    s.add("复审-语言级保留", rev.get(41).is_some(), "keep");
    s.add("复审-留痕", rev.review_count() == 1, "trace");
    // 族七：冲突以规范为准
    let mut rc = LimitBook::new();
    rc.register(Entry::new(50, Level::Language, Source::ToolDiff, "误标", "规范"));
    s.add("冲突-纠正计数", rc.reconcile(9) == 1, "fix");
    s.add("冲突-纠正后正确", rc.get(50).map(|x| x.source == Source::SpecInherent).unwrap_or(false), "fixed");
    // 族八：工具级边界
    s.add("工具-Word上限条目", book.register(Entry::new(60, Level::Tooling, Source::SpecInherent, "Word长度与ID空间上边界", "超大模块").workaround("边界预判定")), "tool");
    s
}
