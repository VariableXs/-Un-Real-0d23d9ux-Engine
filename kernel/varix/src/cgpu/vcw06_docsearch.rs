//! CGPU-F3526 · 文档搜索（vcw06_docsearch）
//!
//! W 域文档搜索——**全文检索 + 过滤**双闸：倒排索引（term→文档集
//! BTreeMap/BTreeSet——B 族确定性序，同输入同输出不漂移）、多词
//! AND 交集语义（查询多词全部命中才返回——OR 是噪音放大器）、
//! CJK 单字切分与拉丁词切分双语通吃（中文按字、英文按词——同一
//! 分词器不分支）、TF 计分排序（命中词频高者先，分数同 doc_id 升
//! 序——**确定性排序不是运气排序**）、类型/标签双维过滤（先过滤
//! 后检索缩小面——过滤闭集外显性拒）、搜索热词统计（F3527 文档遥
//! 测的输入面——热词账本在此喂数）。
//!
//! 数据结构：倒排索引；文档元数据簿。
//!
//! 错误路径与降级矩阵：**空查询→显性拒**（空串查询是 bug 不是零结
//! 果）；**类型/标签闭集外→显性拒**（不静默空结果）；**索引缺词→
//! 空结果非错误**（查无此词是正常业务态——与闭集外拒分开）。
//! 性能：索引 O(文档×词)，检索 O(词数×倒排集)。跨批对接点：
//! vcw01 SDK 文档总架构（文档源）/vcw04 API 参考（ApiRef 类型源）/
//! F3527 文档遥测（热词账输入）。
//! 无障碍：结果读屏可达（spoken）。隐私面：热词只计数不记用户。
//!
//! 诊断码 0xE1 段三码互异：`E_DS_EMPTY_QUERY` / `E_DS_BAD_KIND` /
//! `E_DS_BAD_TAG`。

use crate::checks::CheckSet;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与常量
// ---------------------------------------------------------------------------

/// 文档搜索版本。
pub const DOCSEARCH_VERSION: &str = "W06-docsearch-v1";

// ---------------------------------------------------------------------------
// 诊断码与原因文案（0xE1 段三码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 空查询：查询串分词后为空（显性拒——不是零结果）。
pub const E_DS_EMPTY_QUERY: u16 = 0xE101;
/// 类型闭集外：过滤 kind 未声明。
pub const E_DS_BAD_KIND: u16 = 0xE102;
/// 标签闭集外：过滤 tag 未声明。
pub const E_DS_BAD_TAG: u16 = 0xE103;

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: u16) -> &'static str {
    match code {
        E_DS_EMPTY_QUERY => "空查询：查询串分词后为空，显性拒非零结果",
        E_DS_BAD_KIND => "类型闭集外：过滤 kind 未声明",
        E_DS_BAD_TAG => "标签闭集外：过滤 tag 未声明",
        _ => "未知诊断码",
    }
}

/// 立案簿（0xE1 段事件账——有界不静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagNote {
    pub code: u16,
    pub subject: String,
    pub detail: String,
}

#[derive(Debug)]
pub struct DiagBag {
    notes: Vec<DiagNote>,
    cap: usize,
    dropped: usize,
}

impl DiagBag {
    pub fn new() -> DiagBag {
        DiagBag { notes: Vec::new(), cap: 64, dropped: 0 }
    }

    pub fn push(&mut self, code: u16, subject: &str, detail: &str) {
        if self.notes.len() >= self.cap {
            self.notes.remove(0);
            self.dropped += 1;
        }
        self.notes.push(DiagNote {
            code,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn count_of(&self, code: u16) -> usize {
        self.notes.iter().filter(|n| n.code == code).count()
    }

    pub fn has(&self, code: u16) -> bool {
        self.count_of(code) > 0
    }

    pub fn spoken(&self) -> String {
        if self.notes.is_empty() {
            return "诊断簿空".to_string();
        }
        let mut out = format!("诊断簿共{}条", self.notes.len());
        for n in self.notes.iter() {
            out.push('；');
            out.push_str(reason_of(n.code));
            out.push('：');
            out.push_str(&n.subject);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 文档类型与标签闭集（过滤双维——闭集外显性拒）
// ---------------------------------------------------------------------------

/// 文档类型闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    /// API 参考。
    ApiRef,
    /// 指南。
    Guide,
    /// 教程。
    Tutorial,
    /// 变更日志。
    Changelog,
}

impl DocKind {
    pub const COUNT: usize = 4;

    pub fn all() -> [DocKind; DocKind::COUNT] {
        [DocKind::ApiRef, DocKind::Guide, DocKind::Tutorial, DocKind::Changelog]
    }

    pub fn zh(self) -> &'static str {
        match self {
            DocKind::ApiRef => "API参考",
            DocKind::Guide => "指南",
            DocKind::Tutorial => "教程",
            DocKind::Changelog => "变更日志",
        }
    }

    /// 从字符串解析（闭集外 None——过滤面显性拒用）。
    pub fn from_code(s: &str) -> Option<DocKind> {
        match s {
            "apiref" => Some(DocKind::ApiRef),
            "guide" => Some(DocKind::Guide),
            "tutorial" => Some(DocKind::Tutorial),
            "changelog" => Some(DocKind::Changelog),
            _ => None,
        }
    }
}

/// 标签闭集（六标签——过滤第二维）。
pub const TAGS: [&str; 6] =
    ["graphics", "compute", "memory", "power", "display", "toolchain"];

/// 标签是否在闭集。
pub fn is_valid_tag(tag: &str) -> bool {
    TAGS.iter().any(|t| *t == tag)
}

// ---------------------------------------------------------------------------
// 分词器（CJK 单字切分 + 拉丁词切分——同一分词器不分支）
// ---------------------------------------------------------------------------

/// 分词：小写化；连续 ASCII 字母数字为一个词；每个 CJK 字符独立成
/// 词（单字切分——中文检索粒度）；其他字符为分隔符。
pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            cur.push(ch.to_ascii_lowercase());
        } else if ch as u32 >= 0x4E00 && ch as u32 <= 0x9FFF {
            // 冲掉拉丁词。
            if !cur.is_empty() {
                out.push(cur.clone());
                cur.clear();
            }
            out.push(ch.to_string());
        } else {
            if !cur.is_empty() {
                out.push(cur.clone());
                cur.clear();
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

// ---------------------------------------------------------------------------
// 倒排索引（term→文档集 + 词频账——检索核心）
// ---------------------------------------------------------------------------

/// 倒排索引。
#[derive(Debug, Default)]
pub struct InvertedIndex {
    /// term → (doc_id 集合, term 在该 doc 的出现次数账)。
    postings: BTreeMap<String, BTreeMap<u32, u32>>,
    /// 文档词总数（计分归一用）。
    doc_len: BTreeMap<u32, u32>,
    /// 文档总数。
    pub doc_count: u32,
}

impl InvertedIndex {
    pub fn new() -> InvertedIndex {
        InvertedIndex::default()
    }

    /// 建索引（O(文档词数)——分词+倒排+词频）。
    pub fn add(&mut self, doc_id: u32, text: &str) {
        let mut total = 0u32;
        for term in tokenize(text) {
            let m = self.postings.entry(term).or_default();
            *m.entry(doc_id).or_insert(0) += 1;
            total += 1;
        }
        self.doc_len.insert(doc_id, total);
        self.doc_count += 1;
    }

    /// 单词倒排集（查无词→空集——正常业务态非错误）。
    pub fn postings_of(&self, term: &str) -> BTreeMap<u32, u32> {
        self.postings.get(term).cloned().unwrap_or_default()
    }

    /// 多词 AND 交集检索+TF 计分（确定性：分数同 doc_id 升序）。
    /// 返回 (doc_id, 分数) 降序表——分数=命中词的词频和×1000/文档
    /// 词总数（千分比密度——长文档稀释）。
    pub fn search(&self, terms: &[String]) -> Vec<(u32, u64)> {
        if terms.is_empty() {
            return Vec::new();
        }
        // 各词倒排集求交（首词集起，逐词 retain）。
        let mut acc: BTreeMap<u32, u32> = self.postings_of(&terms[0]);
        for t in &terms[1..] {
            let next = self.postings_of(t);
            acc.retain(|d, _| next.contains_key(d));
        }
        // TF 计分：Σ词频 × 1000 / 文档词总数。
        let mut scored: Vec<(u32, u64)> = acc
            .into_iter()
            .map(|(d, _)| {
                let sum: u32 = terms
                    .iter()
                    .map(|t| self.postings.get(t).and_then(|m| m.get(&d)).copied().unwrap_or(0))
                    .sum();
                let len = self.doc_len.get(&d).copied().unwrap_or(1).max(1);
                (d, sum as u64 * 1000 / len as u64)
            })
            .collect();
        // 排序：分数降序，分数同 doc_id 升序（确定性）。
        scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        scored
    }

    /// 索引词表大小。
    pub fn term_count(&self) -> usize {
        self.postings.len()
    }
}

// ---------------------------------------------------------------------------
// 文档元数据簿与搜索引擎（检索+过滤双闸+热词账）
// ---------------------------------------------------------------------------

/// 文档元数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocMeta {
    pub doc_id: u32,
    pub kind: DocKind,
    pub tag: &'static str,
}

/// 搜索结果单条。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub doc_id: u32,
    pub score: u64,
}

/// 搜索引擎（索引+元数据+双维过滤+热词账）。
#[derive(Debug)]
pub struct SearchEngine {
    pub index: InvertedIndex,
    pub metas: BTreeMap<u32, DocMeta>,
    /// 搜索热词账（F3527 文档遥测输入——只计数不记用户）。
    pub hot_terms: BTreeMap<String, u64>,
    pub bag: DiagBag,
}

impl SearchEngine {
    pub fn new() -> SearchEngine {
        SearchEngine {
            index: InvertedIndex::new(),
            metas: BTreeMap::new(),
            hot_terms: BTreeMap::new(),
            bag: DiagBag::new(),
        }
    }

    /// 登记文档（索引+元数据双写）。
    pub fn add_doc(&mut self, meta: DocMeta, text: &str) {
        self.index.add(meta.doc_id, text);
        self.metas.insert(meta.doc_id, meta);
    }

    /// 热词记账（查询分词逐词计数）。
    fn tally_hot(&mut self, terms: &[String]) {
        for t in terms {
            *self.hot_terms.entry(t.clone()).or_insert(0) += 1;
        }
    }

    /// 检索（全文——无过滤）。
    /// 返回 Ok(命中表) 或 Err(码)。
    pub fn search(&mut self, query: &str) -> Result<Vec<SearchHit>, u16> {
        let terms = tokenize(query);
        if terms.is_empty() {
            self.bag.push(E_DS_EMPTY_QUERY, "检索", &format!("查询串 {:?}", query));
            return Err(E_DS_EMPTY_QUERY);
        }
        self.tally_hot(&terms);
        let hits = self
            .index
            .search(&terms)
            .into_iter()
            .map(|(d, s)| SearchHit { doc_id: d, score: s })
            .collect();
        Ok(hits)
    }

    /// 检索+过滤双闸（先过滤后检索——kind/tag 任一可 None 跳过）。
    pub fn search_with_filter(
        &mut self,
        query: &str,
        kind: Option<&str>,
        tag: Option<&str>,
    ) -> Result<Vec<SearchHit>, u16> {
        // 闭集校验先行（显性拒——不静默空结果）。
        let kind_v = match kind {
            None => None,
            Some(k) => match DocKind::from_code(k) {
                Some(v) => Some(v),
                None => {
                    self.bag.push(E_DS_BAD_KIND, "过滤", &format!("kind {:?}", k));
                    return Err(E_DS_BAD_KIND);
                }
            },
        };
        let tag_v = match tag {
            None => None,
            Some(t) => {
                if !is_valid_tag(t) {
                    self.bag.push(E_DS_BAD_TAG, "过滤", &format!("tag {:?}", t));
                    return Err(E_DS_BAD_TAG);
                }
                Some(t)
            }
        };
        let terms = tokenize(query);
        if terms.is_empty() {
            self.bag.push(E_DS_EMPTY_QUERY, "检索", &format!("查询串 {:?}", query));
            return Err(E_DS_EMPTY_QUERY);
        }
        self.tally_hot(&terms);
        // 过滤集（双维交集——元数据簿过滤）。
        let allow: BTreeSet<u32> = self
            .metas
            .iter()
            .filter(|(_, m)| {
                kind_v.map_or(true, |k| m.kind == k) && tag_v.map_or(true, |t| m.tag == t)
            })
            .map(|(d, _)| *d)
            .collect();
        // 检索∩过滤。
        let hits = self
            .index
            .search(&terms)
            .into_iter()
            .filter(|(d, _)| allow.contains(d))
            .map(|(d, s)| SearchHit { doc_id: d, score: s })
            .collect();
        Ok(hits)
    }

    /// 热词榜（计数降序前 n——确定性：同计数词名字典序）。
    pub fn hot_top(&self, n: usize) -> Vec<(String, u64)> {
        let mut v: Vec<(String, u64)> = self.hot_terms.iter().map(|(k, c)| (k.clone(), *c)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.truncate(n);
        v
    }

    /// 结果读屏摘要。
    pub fn hits_spoken(hits: &[SearchHit]) -> String {
        if hits.is_empty() {
            return "无结果".to_string();
        }
        let mut out = format!("命中{}条", hits.len());
        for h in hits.iter().take(3) {
            out.push_str(&format!(" #{}分{}", h.doc_id, h.score));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 判据（八项——检索/AND交集/CJK切分/类型过滤/标签过滤/热词账/
// 闭集外拒/全链编排）
// ---------------------------------------------------------------------------

/// F3526 判据入口（返回式 CheckSet——全部预期常量独立手算）。
pub fn run_vcw06_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F3526");

    // 检-01 全文检索+TF 计分：3 文档索引——doc1 含 buffer×2（词总
    // 6）、doc2 含 buffer×1（词总 3）、doc3 无。查 buffer → doc1
    // 分 2×1000/6=333、doc2 分 1×1000/3=333——同分 doc_id 升序
    // （doc1 先）——确定性排序非运气。
    {
        let mut se = SearchEngine::new();
        se.add_doc(DocMeta { doc_id: 1, kind: DocKind::Guide, tag: "graphics" },
                   "buffer management for gpu buffer pool");
        se.add_doc(DocMeta { doc_id: 2, kind: DocKind::ApiRef, tag: "memory" },
                   "buffer create api");
        se.add_doc(DocMeta { doc_id: 3, kind: DocKind::Tutorial, tag: "compute" },
                   "first shader tutorial");
        let hits = se.search("buffer").unwrap_or_default();
        // 手算：doc1 词频 2/词总 6 → 333；doc2 词频 1/词总 3 → 333。
        let ok = hits.len() == 2
            && hits[0].doc_id == 1
            && hits[0].score == 333
            && hits[1].doc_id == 2
            && hits[1].score == 333;
        if ok {
            s.ok("F3526-检-01-全文检索TF计分确定性");
        } else {
            s.fail("F3526-检-01-全文检索TF计分确定性", "命中或计分手算不符");
        }
    }

    // 检-02 多词 AND 交集：查 "gpu memory" 只命中同时含两词文档。
    {
        let mut se = SearchEngine::new();
        se.add_doc(DocMeta { doc_id: 1, kind: DocKind::Guide, tag: "memory" },
                   "gpu memory pool design");
        se.add_doc(DocMeta { doc_id: 2, kind: DocKind::Guide, tag: "graphics" },
                   "gpu pipeline only");
        se.add_doc(DocMeta { doc_id: 3, kind: DocKind::Guide, tag: "memory" },
                   "memory budget without accelerator");
        let hits = se.search("gpu memory").unwrap_or_default();
        let ok = hits.len() == 1 && hits[0].doc_id == 1;
        if ok {
            s.ok("F3526-检-02-多词AND交集");
        } else {
            s.fail("F3526-检-02-多词AND交集", "交集语义不符");
        }
    }

    // 检-03 CJK 单字切分：中文文档"缓冲区管理"查"缓冲"命中（单字
    // 切分粒度——缓/冲/区 各自成词，两字全含即 AND 命中）。
    {
        let mut se = SearchEngine::new();
        se.add_doc(DocMeta { doc_id: 1, kind: DocKind::Guide, tag: "memory" },
                   "缓冲区管理策略");
        se.add_doc(DocMeta { doc_id: 2, kind: DocKind::Guide, tag: "graphics" },
                   "渲染管线概述");
        let hits = se.search("缓冲").unwrap_or_default();
        let hits2 = se.search("渲染管线").unwrap_or_default();
        // 手算：doc1 分词[缓,冲,区,管,理,策,略] 词总 7，命中词频 缓1+冲1=2
        // → 2×1000/7=285（285.7 截断）；查"渲染管线" doc2 "渲染管线概述"
        // 词总 6，查询分词[渲,染,管,线] 词频和=4 → 4×1000/6=666。
        let ok = hits.len() == 1
            && hits[0].doc_id == 1
            && hits[0].score == 285
            && hits2.len() == 1
            && hits2[0].score == 666;
        if ok {
            s.ok("F3526-检-03-CJK单字切分双语通吃");
        } else {
            s.fail("F3526-检-03-CJK单字切分双语通吃", "分词粒度或计分不符");
        }
    }

    // 过-01 类型过滤：kind=apiref 只返回 API 文档。
    {
        let mut se = SearchEngine::new();
        se.add_doc(DocMeta { doc_id: 1, kind: DocKind::ApiRef, tag: "memory" },
                   "buffer api reference");
        se.add_doc(DocMeta { doc_id: 2, kind: DocKind::Guide, tag: "memory" },
                   "buffer guide");
        let hits = se.search_with_filter("buffer", Some("apiref"), None).unwrap_or_default();
        let hits_all = se.search_with_filter("buffer", None, None).unwrap_or_default();
        let ok = hits.len() == 1
            && hits[0].doc_id == 1
            && hits_all.len() == 2;
        if ok {
            s.ok("F3526-过-01-类型过滤双闸");
        } else {
            s.fail("F3526-过-01-类型过滤双闸", "类型过滤不符");
        }
    }

    // 过-02 标签过滤：tag=memory 只返回内存标签文档。
    {
        let mut se = SearchEngine::new();
        se.add_doc(DocMeta { doc_id: 1, kind: DocKind::Guide, tag: "memory" },
                   "buffer pool");
        se.add_doc(DocMeta { doc_id: 2, kind: DocKind::Guide, tag: "power" },
                   "buffer power mode");
        let hits = se.search_with_filter("buffer", None, Some("memory")).unwrap_or_default();
        let ok = hits.len() == 1 && hits[0].doc_id == 1;
        if ok {
            s.ok("F3526-过-02-标签过滤");
        } else {
            s.fail("F3526-过-02-标签过滤", "标签过滤不符");
        }
    }

    // 热-01 热词账：多次查询→hot_terms 计数恰值+热词榜确定性。
    {
        let mut se = SearchEngine::new();
        se.add_doc(DocMeta { doc_id: 1, kind: DocKind::Guide, tag: "memory" }, "gpu buffer");
        let _ = se.search("gpu");
        let _ = se.search("gpu");
        let _ = se.search("buffer");
        let top = se.hot_top(2);
        // 手算：gpu 2 次、buffer 1 次——榜首 gpu 计 2。
        let ok = se.hot_terms.get("gpu").copied() == Some(2)
            && se.hot_terms.get("buffer").copied() == Some(1)
            && top.len() == 2
            && top[0].0 == "gpu"
            && top[0].1 == 2;
        if ok {
            s.ok("F3526-热-01-搜索热词账F3527输入");
        } else {
            s.fail("F3526-热-01-搜索热词账F3527输入", "热词计数不符");
        }
    }

    // 边-01 闭集外显性拒：空查询/未知 kind/未知 tag 三码各立案。
    {
        let mut se = SearchEngine::new();
        se.add_doc(DocMeta { doc_id: 1, kind: DocKind::Guide, tag: "memory" }, "content");
        let r1 = se.search("   "); // 纯分隔符——分词空
        let r2 = se.search_with_filter("content", Some("unknown"), None);
        let r3 = se.search_with_filter("content", None, Some("bogus"));
        let ok = matches!(r1, Err(E_DS_EMPTY_QUERY))
            && matches!(r2, Err(E_DS_BAD_KIND))
            && matches!(r3, Err(E_DS_BAD_TAG))
            && se.bag.count_of(E_DS_EMPTY_QUERY) == 1
            && se.bag.count_of(E_DS_BAD_KIND) == 1
            && se.bag.count_of(E_DS_BAD_TAG) == 1;
        if ok {
            s.ok("F3526-边-01-空查询与闭集外显性拒");
        } else {
            s.fail("F3526-边-01-空查询与闭集外显性拒", "显性拒或立案不符");
        }
    }

    // 编-01 全链编排：5 文档索引→检索+双维过滤+热词+双跑确定性对账
    // +读屏摘要非空+全程零 panic。
    {
        let mut se = SearchEngine::new();
        se.add_doc(DocMeta { doc_id: 1, kind: DocKind::ApiRef, tag: "graphics" },
                   "gpu command buffer api");
        se.add_doc(DocMeta { doc_id: 2, kind: DocKind::ApiRef, tag: "memory" },
                   "gpu buffer allocation api");
        se.add_doc(DocMeta { doc_id: 3, kind: DocKind::Guide, tag: "graphics" },
                   "gpu buffer usage guide");
        se.add_doc(DocMeta { doc_id: 4, kind: DocKind::Tutorial, tag: "toolchain" },
                   "build tools setup");
        se.add_doc(DocMeta { doc_id: 5, kind: DocKind::Changelog, tag: "display" },
                   "release notes v2");
        let h1 = se.search_with_filter("gpu buffer", Some("apiref"), None).unwrap_or_default();
        let h2 = se.search_with_filter("gpu buffer", None, Some("graphics")).unwrap_or_default();
        let h3 = se.search("gpu buffer").unwrap_or_default();
        // 手算：apiref∩{1,2}；graphics∩{1,3}；全量 {1,2,3}。
        // 计分：doc1 词总 4 词频 gpu1+buffer1=2 → 2×1000/4=500；
        // doc2 词总 4 词频 2 → 500；doc3 词总 4 词频 2 → 500。
        let chain_ok = h1.len() == 2
            && h1.iter().all(|h| h.doc_id == 1 || h.doc_id == 2)
            && h2.len() == 2
            && h2.iter().all(|h| h.doc_id == 1 || h.doc_id == 3)
            && h3.len() == 3
            && h3.iter().all(|h| h.score == 500)
            && se.hot_terms.get("gpu").copied() == Some(3)
            && se.index.doc_count == 5
            && SearchEngine::hits_spoken(&h3).contains("命中3条");
        // 双跑确定性：重建同索引同查询结果逐字一致。
        let mut se2 = SearchEngine::new();
        se2.add_doc(DocMeta { doc_id: 1, kind: DocKind::ApiRef, tag: "graphics" },
                    "gpu command buffer api");
        se2.add_doc(DocMeta { doc_id: 2, kind: DocKind::ApiRef, tag: "memory" },
                    "gpu buffer allocation api");
        se2.add_doc(DocMeta { doc_id: 3, kind: DocKind::Guide, tag: "graphics" },
                    "gpu buffer usage guide");
        se2.add_doc(DocMeta { doc_id: 4, kind: DocKind::Tutorial, tag: "toolchain" },
                    "build tools setup");
        se2.add_doc(DocMeta { doc_id: 5, kind: DocKind::Changelog, tag: "display" },
                    "release notes v2");
        let h3b = se2.search("gpu buffer").unwrap_or_default();
        let det_ok = h3 == h3b;
        if chain_ok && det_ok {
            s.ok("F3526-编-01-检索过滤热词全链确定性对账");
        } else {
            s.fail("F3526-编-01-检索过滤热词全链确定性对账", "编排或双跑对账不符");
        }
    }

    s
}
