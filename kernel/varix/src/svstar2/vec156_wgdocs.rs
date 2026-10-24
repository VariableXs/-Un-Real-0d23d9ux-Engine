//! VE-F0556 · WGSL 后端文档
//!
//! WGSL 后端文档五件：架构章（近直通与强校验生成即合规哲学）、映射章（IR 到 WGSL
//! 全构造映射表）、限制章（引用 F0553 不重复展开）、操作章（缓存、对拍、错误解读 FAQ）、
//! 维护章（WGSL 特有：规范仍在演进的版本跟踪——规范版本与实现行为关联条目化）。
//! 文档与实现一致性抽检常态化。
//!
//! 跨批对接：上游 F0541-F0555；家族同构 F0518/F0537；下游 F0560 组收口归档。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 五件文档章名
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocChapter {
    /// 架构章
    Arch,
    /// 映射章
    Mapping,
    /// 限制章
    Limit,
    /// 操作章
    Ops,
    /// 维护章
    Maint,
}

impl DocChapter {
    pub fn name(self) -> &'static str {
        match self {
            DocChapter::Arch => "架构章",
            DocChapter::Mapping => "映射章",
            DocChapter::Limit => "限制章",
            DocChapter::Ops => "操作章",
            DocChapter::Maint => "维护章",
        }
    }
}

/// 映射表条目：IR 构造 → WGSL 构造
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MapEntry {
    /// IR 构造名
    pub ir_name: String,
    /// WGSL 产出构造
    pub wgsl_name: String,
    /// 是否有转换备注
    pub note: Option<String>,
}

impl MapEntry {
    pub fn new(ir: &str, wgsl: &str) -> Self {
        MapEntry { ir_name: ir.to_string(), wgsl_name: wgsl.to_string(), note: None }
    }

    pub fn note(mut self, n: &str) -> Self {
        self.note = Some(n.to_string());
        self
    }
}

/// 版本关联条目：规范版本与实现行为关联条目化
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VerEntry {
    /// 规范版本号
    pub spec_ver: u32,
    /// 行为描述
    pub behavior: String,
    /// 该版本引入的偏差是否已知
    pub known_deviation: bool,
}

impl VerEntry {
    pub fn new(spec_ver: u32, behavior: &str, known_deviation: bool) -> Self {
        VerEntry { spec_ver, behavior: behavior.to_string(), known_deviation }
    }
}

/// 后端文档（五件结构）
#[derive(Clone, Debug)]
pub struct WgDoc {
    /// 五章齐备标记（按章登记）
    chapters: Vec<(DocChapter, String)>,
    /// 映射表
    pub mapping: Vec<MapEntry>,
    /// 版本关联条目（维护章内容）
    pub versions: Vec<VerEntry>,
    /// 限制章引用条目数（引用 F0553 不重复展开）
    pub limit_refs: u32,
}

impl WgDoc {
    pub fn new() -> Self {
        WgDoc { chapters: Vec::new(), mapping: Vec::new(), versions: Vec::new(), limit_refs: 0 }
    }

    /// 登记章内容
    pub fn put(&mut self, ch: DocChapter, body: &str) -> bool {
        if body.is_empty() {
            return false;
        }
        if let Some(e) = self.chapters.iter_mut().find(|(c, _)| *c == ch) {
            e.1 = body.to_string();
        } else {
            self.chapters.push((ch, body.to_string()));
        }
        true
    }

    pub fn body(&self, ch: DocChapter) -> Option<&str> {
        self.chapters.iter().find(|(c, _)| *c == ch).map(|(_, b)| b.as_str())
    }

    /// 五件齐备
    pub fn complete(&self) -> bool {
        [DocChapter::Arch, DocChapter::Mapping, DocChapter::Limit, DocChapter::Ops, DocChapter::Maint]
            .iter()
            .all(|c| self.body(*c).is_some())
    }

    /// 映射登记
    pub fn map(&mut self, e: MapEntry) -> bool {
        if self.mapping.iter().any(|m| m.ir_name == e.ir_name) {
            return false;
        }
        self.mapping.push(e);
        true
    }

    /// 映射全覆盖校验：给定 IR 构造全集都在表中
    pub fn mapping_covers(&self, universe: &[&str]) -> bool {
        universe.iter().all(|u| self.mapping.iter().any(|m| &m.ir_name == u))
    }

    /// 版本跟踪登记
    pub fn track(&mut self, e: VerEntry) {
        self.versions.push(e);
        self.versions.sort_by_key(|v| v.spec_ver);
    }

    /// 限制章引用登记（去重展开）
    pub fn ref_limits(&mut self, count: u32) {
        self.limit_refs = count;
    }
}

/// 一致性抽检：文档条目与实现条目比对，漂移即发现
pub fn consistency_check(doc_claims: &[&str], impl_facts: &[&str]) -> Vec<String> {
    let mut drifted = Vec::new();
    for d in doc_claims {
        if !impl_facts.contains(d) {
            drifted.push(d.to_string());
        }
    }
    drifted
}

/// 限期回补判定：漂移数超阈值 → 限期
pub fn needs_fix_by(drift_count: usize, threshold: usize) -> bool {
    drift_count >= threshold
}

/// FAQ 条目（操作章：错误解读 FAQ）
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Faq {
    pub question: String,
    pub answer: String,
}

impl Faq {
    pub fn new(q: &str, a: &str) -> Self {
        Faq { question: q.to_string(), answer: a.to_string() }
    }

    pub fn usable(&self) -> bool {
        !self.question.is_empty() && !self.answer.is_empty()
    }
}

pub fn run_vec156_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec156");
    // 族一：五件齐备
    let mut doc = WgDoc::new();
    s.add("五件-空档不齐", !doc.complete(), "empty");
    doc.put(DocChapter::Arch, "近直通与强校验：生成即合规");
    doc.put(DocChapter::Limit, "引用 F0553 清单");
    doc.put(DocChapter::Ops, "缓存/对拍/错误FAQ");
    doc.put(DocChapter::Maint, "规范演进版本跟踪");
    s.add("五件-缺映射不齐", !doc.complete(), "no-map");
    doc.put(DocChapter::Mapping, "IR 到 WGSL 全构造映射");
    s.add("五件-补齐完整", doc.complete(), "full");
    s.add("章名-可读", DocChapter::Arch.name() == "架构章", "name");
    // 族二：章内容覆写
    doc.put(DocChapter::Arch, "更新后的架构描述");
    s.add("覆写-同章更新不重复", doc.body(DocChapter::Arch) == Some("更新后的架构描述"), "update");
    s.add("覆写-空内容拒绝", !doc.put(DocChapter::Ops, ""), "empty-put");
    // 族三：映射章全覆盖
    doc.map(MapEntry::new("BinOp", "a + b").note("运算符直映"));
    doc.map(MapEntry::new("If", "if/else"));
    s.add("映射-覆盖命中", doc.mapping_covers(&["BinOp", "If"]), "cover");
    s.add("映射-缺失检出", !doc.mapping_covers(&["BinOp", "Loop"]), "miss");
    s.add("映射-重复拒绝", !doc.map(MapEntry::new("BinOp", "x")), "dup");
    s.add("映射-备注登记", doc.mapping[0].note == Some("运算符直映".to_string()), "note");
    // 族四：维护章版本跟踪
    doc.track(VerEntry::new(3, "新增 const 表达式支持", false));
    doc.track(VerEntry::new(2, "基线行为", false));
    doc.track(VerEntry::new(4, "偏差已知", true));
    s.add("版本-按序整理", doc.versions[0].spec_ver == 2 && doc.versions[2].spec_ver == 4, "sort");
    s.add("版本-偏差标记", doc.versions[2].known_deviation, "dev");
    // 族五：限制章引用不重复展开
    doc.ref_limits(4);
    s.add("限制-引用登记", doc.limit_refs == 4, "ref");
    // 族六：一致性抽检
    let facts = ["近直通", "强校验", "双前端"];
    let drift = consistency_check(&["近直通", "强校验", "错误映射"], &facts);
    s.add("抽检-漂移定位", drift == vec!["错误映射".to_string()], "drift");
    s.add("抽检-无漂移", consistency_check(&["近直通"], &facts).is_empty(), "ok");
    s.add("限期-超阈回补", needs_fix_by(2, 2) && !needs_fix_by(1, 2), "by");
    // 族七：FAQ 可用性
    let faq = Faq::new("为何校验失败？", "见错误分类三段");
    let bad = Faq::new("q", "");
    s.add("FAQ-可用", faq.usable() && !bad.usable(), "faq");
    s
}
