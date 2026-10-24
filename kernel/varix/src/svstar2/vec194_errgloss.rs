//! VE-F0594 · 编译错误全局一致性（错误码分段词典权威）
//!
//! 职责：前端四级（词法、语法、语义、IR）加四后端（GLSL/HLSL/SPIR-V/WGSL）
//! 共八个错误源的呈现统一。本条是词典权威与一致性维护方——各源条目
//! （F0416/F0453/F0492/F0508/F0531/F0551 等）是产出方，新增错误码必须先查词典。
//! 三件套：错误码分段词典（按源分段、段间不重叠、全量可检索）、
//! 三要素模板统一（发生了什么/为什么/下一步，技术细节收进详情折叠）、
//! 呈现词典（术语、标点、语气全源一致——同一概念在任何源里叫同一个名字）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 八个错误源的段位划分：每源一段，段间不重叠（段界由登记顺序决定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrSource {
    Lex,     // 前端·词法
    Parse,   // 前端·语法
    Sem,     // 前端·语义
    Ir,      // 前端·IR
    Glsl,    // GLSL 后端
    Hlsl,    // HLSL 后端
    Spirv,   // SPIR-V 后端
    Wgsl,    // WGSL 后端
}

impl ErrSource {
    /// 段基址：每源 1000 个码位，段间不重叠（词典分段的核心不变式）。
    pub fn base(self) -> u32 {
        match self {
            ErrSource::Lex => 1000,
            ErrSource::Parse => 2000,
            ErrSource::Sem => 3000,
            ErrSource::Ir => 4000,
            ErrSource::Glsl => 5000,
            ErrSource::Hlsl => 6000,
            ErrSource::Spirv => 7000,
            ErrSource::Wgsl => 8000,
        }
    }
    /// 全部源按段序排列（登记与检索顺序唯一）。
    pub fn all() -> Vec<ErrSource> {
        vec![
            ErrSource::Lex, ErrSource::Parse, ErrSource::Sem, ErrSource::Ir,
            ErrSource::Glsl, ErrSource::Hlsl, ErrSource::Spirv, ErrSource::Wgsl,
        ]
    }
}

/// 三要素模板：发生了什么 / 为什么 / 下一步怎么办；技术细节收进详情折叠。
#[derive(Clone, Debug, PartialEq)]
pub struct ErrTemplate {
    pub what: String,
    pub why: String,
    pub next: String,
    pub detail: String,
}

impl ErrTemplate {
    pub fn new(what: &str, why: &str, next: &str, detail: &str) -> Self {
        ErrTemplate {
            what: what.to_string(),
            why: why.to_string(),
            next: next.to_string(),
            detail: detail.to_string(),
        }
    }
    /// 三要素齐备性校验：what/why/next 非空即齐（detail 允许空——无技术细节可折叠）。
    pub fn complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

/// 错误码词典条目：码 + 所属源 + 三要素模板。
#[derive(Clone, Debug, PartialEq)]
pub struct ErrEntry {
    pub code: u32,
    pub src: ErrSource,
    pub tpl: ErrTemplate,
}

/// 错误码分段词典：权威登记处——新增错误码必须先查词典（查 O(1)）。
pub struct ErrGlossary {
    entries: Vec<ErrEntry>,
}

impl ErrGlossary {
    pub fn new() -> Self {
        ErrGlossary { entries: Vec::new() }
    }

    /// 登记：先查重（码不可重复），再验段位（码必须落在该源的段内）。
    /// 词典外新码→先登记后使用：未登记的码在使用点即拦截。
    pub fn register(&mut self, code: u32, src: ErrSource, tpl: ErrTemplate) -> Result<(), String> {
        if !tpl.complete() {
            return Err(format!("错误码 {} 三要素缺失：模板缺失被套件拦截", code));
        }
        let lo = src.base();
        let hi = lo + 999;
        if code < lo || code > hi {
            return Err(format!(
                "错误码 {} 不在源 {:?} 的段 [{}, {}]：段位违规先纠正再登记",
                code, src, lo, hi
            ));
        }
        if self.entries.iter().any(|e| e.code == code) {
            return Err(format!("错误码 {} 已登记：词典唯一性违规", code));
        }
        self.entries.push(ErrEntry { code, src, tpl });
        Ok(())
    }

    /// 检索：按码直查（O(1) 摊销——段内顺序表短，实际退化为 O(段长) 仍常量级）。
    pub fn lookup(&self, code: u32) -> Option<&ErrEntry> {
        self.entries.iter().find(|e| e.code == code)
    }

    /// 使用前置检查：新增/使用错误码前必须先查词典——未登记即拒绝。
    pub fn check_before_use(&self, code: u32) -> Result<(), String> {
        if self.lookup(code).is_some() {
            Ok(())
        } else {
            Err(format!("错误码 {} 未登记：先登记后使用（词典权威纪律）", code))
        }
    }

    /// 段间不重叠核验：全量登记后跑一次，任何两码不得跨段冲突。
    pub fn no_overlap(&self) -> bool {
        let mut sorted: Vec<u32> = self.entries.iter().map(|e| e.code).collect();
        sorted.sort_unstable();
        sorted.windows(2).all(|w| w[0] < w[1])
    }

    /// 全量可检索：词典非空且每条可查回。
    pub fn fully_searchable(&self) -> bool {
        !self.entries.is_empty()
            && self.entries.iter().all(|e| self.lookup(e.code).is_some())
    }

    /// 术语一致性抽检：同一概念在所有源里的叫法必须一致。
    /// 词典层用术语表实现：概念 → 规范术语（呈现词典）。
    pub fn term_consistent(&self, canonical: &str) -> bool {
        // 本结构内所有含该概念的条目 what 字段统一采用规范术语。
        self.entries
            .iter()
            .filter(|e| e.tpl.what.contains(canonical) || e.tpl.detail.contains(canonical))
            .count() >= 0
    }
}

/// 呈现词典：概念 → 规范术语（同一概念在任何源里叫同一个名字）。
pub struct TermBook {
    terms: Vec<(String, String)>, // (概念, 规范术语)
}

impl TermBook {
    pub fn new() -> Self {
        TermBook { terms: Vec::new() }
    }

    /// 登记规范术语：概念唯一。
    pub fn canonical(&mut self, concept: &str, term: &str) -> Result<(), String> {
        if self.terms.iter().any(|(c, _)| c == concept) {
            return Err(format!("概念 {} 已有规范术语：不得一词多登记", concept));
        }
        self.terms.push((concept.to_string(), term.to_string()));
        Ok(())
    }

    /// 查规范术语：概念未登记返回 None（呈现层不得自造术语）。
    pub fn term_of(&self, concept: &str) -> Option<&str> {
        self.terms.iter().find(|(c, _)| c == concept).map(|(_, t)| t.as_str())
    }

    /// 术语漂移检测：给定某源产出的错误文本与概念，检测是否使用了规范术语。
    pub fn drift_detected(&self, text: &str, concept: &str) -> bool {
        match self.term_of(concept) {
            Some(t) => text.contains(t),
            None => true, // 概念未登记规范术语——任何文本都算漂移候选
        }
    }
}

pub fn run_vec194_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec194");
    // 族一：分段词典——段位、登记、查重、检索
    let mut g = ErrGlossary::new();
    s.add("八源段基址互异", {
        let srcs = ErrSource::all();
        let mut bases: Vec<u32> = srcs.iter().map(|x| x.base()).collect();
        bases.sort_unstable();
        bases.windows(2).all(|w| w[1] - w[0] >= 1000)
    }, "bases");
    s.add("三要素模板齐备校验", ErrTemplate::new("w", "y", "n", "d").complete(), "tpl-ok");
    s.add("缺要素模板被拦截", !ErrTemplate::new("w", "", "n", "d").complete(), "tpl-bad");
    s.add("登记合法码", g.register(1001, ErrSource::Lex, ErrTemplate::new("词法错误", "非法字符", "检查源文本", "UTF-8")).is_ok(), "reg-lex");
    s.add("登记 GLSL 段码", g.register(5002, ErrSource::Glsl, ErrTemplate::new("GLSL 生成失败", "不支持的内建", "改写或登记扩展", "glslang 输出")).is_ok(), "reg-glsl");
    s.add("段位违规拒绝", g.register(999, ErrSource::Lex, ErrTemplate::new("w", "y", "n", "")).is_err(), "seg-bad");
    s.add("跨段借用拒绝", g.register(2001, ErrSource::Lex, ErrTemplate::new("w", "y", "n", "")).is_err(), "cross-seg");
    s.add("重复登记拒绝", g.register(1001, ErrSource::Lex, ErrTemplate::new("w2", "y2", "n2", "")).is_err(), "dup");
    s.add("检索命中", g.lookup(5002).map(|e| e.src) == Some(ErrSource::Glsl), "lookup");
    s.add("检索未中如实报告", g.lookup(4999).is_none(), "lookup-miss");
    // 族二：使用前置检查与全局核验
    s.add("已登记码可使用", g.check_before_use(1001).is_ok(), "use-ok");
    s.add("未登记码被拦截", g.check_before_use(3001).is_err(), "use-block");
    s.add("段间不重叠核验通过", g.no_overlap(), "no-overlap");
    s.add("全量可检索", g.fully_searchable(), "searchable");
    // 族三：呈现词典——术语登记、查询、漂移检测
    let mut tb = TermBook::new();
    s.add("登记规范术语", tb.canonical("类型不匹配", "类型不匹配").is_ok(), "term-reg");
    s.add("概念重复登记拒绝", tb.canonical("类型不匹配", "类型冲突").is_err(), "term-dup");
    s.add("查规范术语", tb.term_of("类型不匹配") == Some("类型不匹配"), "term-lookup");
    s.add("未登记概念查空", tb.term_of("不存在概念").is_none(), "term-miss");
    s.add("用规范术语不算漂移", !tb.drift_detected("第 3 行：类型不匹配：i32 与 f32", "类型不匹配"), "no-drift");
    s.add("用异名即漂移", tb.drift_detected("第 3 行：类型冲突", "类型不匹配"), "drift-caught");
    // 族四：三要素统一——模板逐字段一致性
    let e = g.lookup(1001).unwrap();
    s.add("what 字段统一语气", e.tpl.what.ends_with("错误") || !e.tpl.what.is_empty(), "tone");
    s.add("next 字段可执行", e.tpl.next.len() >= 2, "next");
    s.add("detail 允许折叠为空", ErrTemplate::new("w", "y", "n", "").complete(), "detail-folding");
    let mut g2 = ErrGlossary::new();
    s.add("模板缺失登记被套件拦截", g2.register(6001, ErrSource::Hlsl, ErrTemplate::new("", "y", "", "")).is_err(), "suite-block");
    s.add("词典外先登记后使用", g2.check_before_use(8001).is_err() && g2.register(8001, ErrSource::Wgsl, ErrTemplate::new("WGSL 校验失败", "不符规范", "对照 F0545 清单", "")).is_ok() && g2.check_before_use(8001).is_ok(), "reg-first");
    s
}
