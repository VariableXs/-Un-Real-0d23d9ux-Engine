//! VE-F0576 · SPIR-V 后端文档
//!
//! 五件：架构章（二进制直出五段架构与 Word 流布局图）、指令参考章（生成指令族到
//! SPIR-V 规范条款的映射表）、限制章（引用 F0573 不重复展开）、操作章（验证流程、
//! 对拍方法、错误解读 FAQ）、维护章（版本矩阵跟踪——与 F0571 联动条目化）；
//! 家族同构 F0518/F0537/F0556（第四成员），聚合差异点：二进制介质特有的操作章
//! 内容（反汇编、验证工具用法）。
//!
//! 跨批对接：上游 F0561-0575；下游 F0580 组收口归档。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 五件章名
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chapter {
    Arch,
    /// 指令参考章
    InstRef,
    Limit,
    Ops,
    Maint,
}

impl Chapter {
    pub fn name(self) -> &'static str {
        match self {
            Chapter::Arch => "架构章",
            Chapter::InstRef => "指令参考章",
            Chapter::Limit => "限制章",
            Chapter::Ops => "操作章",
            Chapter::Maint => "维护章",
        }
    }
}

/// 指令映射条目：生成指令族 → SPIR-V 规范条款
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InstMap {
    pub family: String,
    pub opcode: u32,
    pub spec_clause: String,
}

impl InstMap {
    pub fn new(family: &str, opcode: u32, clause: &str) -> Self {
        InstMap { family: family.to_string(), opcode, spec_clause: clause.to_string() }
    }
}

/// 版本联动条目（与 F0571 联动）
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VerLink {
    pub feature: String,
    pub spv_min: u32,
    pub registered: bool,
}

impl VerLink {
    pub fn new(feature: &str, spv_min: u32) -> Self {
        VerLink { feature: feature.to_string(), spv_min, registered: true }
    }
}

/// 后端文档五件结构
#[derive(Clone, Debug)]
pub struct Doc {
    chapters: Vec<(Chapter, String)>,
    pub inst_map: Vec<InstMap>,
    pub ver_links: Vec<VerLink>,
    pub limit_refs: u32,
}

impl Doc {
    pub fn new() -> Self {
        Doc { chapters: Vec::new(), inst_map: Vec::new(), ver_links: Vec::new(), limit_refs: 0 }
    }

    pub fn put(&mut self, ch: Chapter, body: &str) -> bool {
        if body.is_empty() {
            return false;
        }
        match self.chapters.iter_mut().find(|(c, _)| *c == ch) {
            Some(e) => e.1 = body.to_string(),
            None => self.chapters.push((ch, body.to_string())),
        }
        true
    }

    pub fn body(&self, ch: Chapter) -> Option<&str> {
        self.chapters.iter().find(|(c, _)| *c == ch).map(|(_, b)| b.as_str())
    }

    pub fn complete(&self) -> bool {
        [Chapter::Arch, Chapter::InstRef, Chapter::Limit, Chapter::Ops, Chapter::Maint]
            .iter()
            .all(|c| self.body(*c).is_some())
    }

    pub fn map_inst(&mut self, m: InstMap) -> bool {
        if self.inst_map.iter().any(|x| x.family == m.family) {
            return false;
        }
        self.inst_map.push(m);
        true
    }

    /// 映射全覆盖：给定指令族全集都在表中
    pub fn covers(&self, universe: &[&str]) -> bool {
        universe.iter().all(|u| self.inst_map.iter().any(|m| &m.family == u))
    }

    pub fn link(&mut self, v: VerLink) -> bool {
        if self.ver_links.iter().any(|x| x.feature == v.feature) {
            return false;
        }
        self.ver_links.push(v);
        true
    }

    pub fn ref_limits(&mut self, n: u32) {
        self.limit_refs = n;
    }
}

/// FAQ 条目（操作章：验证流程、对拍方法、错误解读）
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Faq {
    pub q: String,
    pub a: String,
}

impl Faq {
    pub fn new(q: &str, a: &str) -> Self {
        Faq { q: q.to_string(), a: a.to_string() }
    }

    pub fn usable(&self) -> bool {
        !self.q.is_empty() && !self.a.is_empty()
    }
}

/// 一致性抽检：文档声明与实现事实比对，漂移限期回补
pub fn consistency(doc_claims: &[&str], impl_facts: &[&str]) -> Vec<String> {
    doc_claims.iter().filter(|d| !impl_facts.contains(d)).map(|d| d.to_string()).collect()
}

/// 限期回补判定
pub fn needs_fix(drift: usize, threshold: usize) -> bool {
    drift >= threshold
}

/// 工具升级 → 操作章同步流程标记
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SyncFlag {
    pub tool_bumped: bool,
    pub ops_chapter_synced: bool,
}

impl SyncFlag {
    pub fn pending(&self) -> bool {
        self.tool_bumped && !self.ops_chapter_synced
    }
}

pub fn run_vec176_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec176");
    // 族一：五件齐备
    let mut d = Doc::new();
    s.add("五件-空不齐", !d.complete(), "empty");
    d.put(Chapter::Arch, "二进制直出五段架构与 Word 流布局");
    d.put(Chapter::InstRef, "指令族到规范条款映射");
    d.put(Chapter::Limit, "引用 F0573 清单");
    d.put(Chapter::Ops, "验证流程/对拍/反汇编FAQ");
    s.add("五件-缺维护章", !d.complete(), "no-maint");
    d.put(Chapter::Maint, "版本矩阵跟踪");
    s.add("五件-齐备", d.complete(), "full");
    s.add("章名-可读", Chapter::InstRef.name() == "指令参考章", "name");
    // 族二：指令参考映射
    d.map_inst(InstMap::new("arith", 128, "3.32.8 OpIAdd"));
    d.map_inst(InstMap::new("sample", 87, "3.32.10 OpImageSampleImplicitLod"));
    d.map_inst(InstMap::new("control-flow", 247, "3.32.15 OpSelectionMerge"));
    s.add("映射-覆盖命中", d.covers(&["arith", "sample", "control-flow"]), "cover");
    s.add("映射-缺失检出", !d.covers(&["arith", "atomic"]), "miss");
    s.add("映射-重复拒绝", !d.map_inst(InstMap::new("arith", 129, "x")), "dup");
    s.add("映射-条款登记", d.inst_map[0].spec_clause.contains("3.32.8"), "clause");
    // 族三：版本联动（F0571）
    d.link(VerLink::new("ptr-buffer", 0x0001_0300));
    d.link(VerLink::new("shader-clock", 0x0001_0400));
    s.add("联动-登记生效", d.ver_links.len() == 2 && d.ver_links[0].registered, "link");
    s.add("联动-重复拒绝", !d.link(VerLink::new("ptr-buffer", 1)), "dup");
    // 族四：限制章引用不重复展开
    d.ref_limits(4);
    s.add("限制-引用登记", d.limit_refs == 4, "ref");
    // 族五：FAQ 可用性
    let faq = Faq::new("验证失败如何解读？", "按三分类解析诊断");
    s.add("FAQ-可用", faq.usable(), "ok");
    s.add("FAQ-空答不可用", !Faq::new("q", "").usable(), "empty");
    // 族六：一致性抽检
    let facts = ["五段架构", "Word流", "硬门禁"];
    let drift = consistency(&["五段架构", "硬门禁", "旧布局图"], &facts);
    s.add("抽检-漂移定位", drift == vec!["旧布局图".to_string()], "drift");
    s.add("抽检-无漂移", consistency(&["五段架构"], &facts).is_empty(), "clean");
    s.add("限期-超阈回补", needs_fix(2, 2) && !needs_fix(1, 2), "by");
    // 族七：工具升级同步流程
    let sync = SyncFlag { tool_bumped: true, ops_chapter_synced: false };
    s.add("同步-升级待同步", sync.pending(), "pend");
    let done = SyncFlag { tool_bumped: true, ops_chapter_synced: true };
    s.add("同步-完成不挂起", !done.pending(), "done");
    // 族八：同章覆写
    d.put(Chapter::Ops, "更新后的操作说明");
    s.add("覆写-不重复", d.body(Chapter::Ops) == Some("更新后的操作说明"), "upd");
    s
}
