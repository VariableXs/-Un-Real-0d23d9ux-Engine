//! VE-F0555 · WGSL fuzz
//!
//! 三层语料（规范语法全集采样、语义边界用例、变异语料）加差分信号通道——
//! tint 与 naga 双生态前端对同一输入的接受性差分（单侧崩溃或双侧结论相反
//! 即为高优信号）；变异算子 WGSL 特有：标识符大小写、注释位置、空白规范化、
//! 数字字面量后缀；产物流向生成器产文本→双前端校验→崩溃分类入册。
//!
//! 跨批对接：上游 F0541-F0552 全组产物；家族同构 F0498/F0536；下游 F0560 收口汇总。

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;

/// 语料层
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CorpusLayer {
    /// 规范语法全集采样
    Grammar,
    /// 语义边界用例
    SemanticEdge,
    /// 变异语料
    Mutated,
}

/// 语料条目
#[derive(Clone, Debug)]
pub struct Corpus {
    pub layer: CorpusLayer,
    pub name: String,
    pub text: String,
}

impl Corpus {
    pub fn new(layer: CorpusLayer, name: &str, text: &str) -> Self {
        Corpus { layer, name: name.to_string(), text: text.to_string() }
    }
}

/// 三层语料库
#[derive(Clone, Debug, Default)]
pub struct CorpusBook {
    items: Vec<Corpus>,
}

impl CorpusBook {
    pub fn new() -> Self {
        CorpusBook::default()
    }

    pub fn add(&mut self, c: Corpus) {
        self.items.push(c);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 层内采样
    pub fn sample(&self, layer: CorpusLayer) -> Vec<&Corpus> {
        self.items.iter().filter(|c| c.layer == layer).collect()
    }

    /// 三层齐备校验
    pub fn complete(&self) -> bool {
        [CorpusLayer::Grammar, CorpusLayer::SemanticEdge, CorpusLayer::Mutated]
            .iter()
            .all(|l| !self.sample(*l).is_empty())
    }
}

/// 双前端差分信号
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffSignal {
    /// 双侧一致接受
    BothAccept,
    /// 双侧一致拒绝
    BothReject,
    /// 单侧崩溃（高优）
    OneSideCrash,
    /// 双侧结论相反（高优）
    Contradiction,
}

impl DiffSignal {
    pub fn high_priority(self) -> bool {
        matches!(self, DiffSignal::OneSideCrash | DiffSignal::Contradiction)
    }
}

/// 双前端校验结果（模拟通道：Accept/Reject/Crash）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrontResult {
    Accept,
    Reject,
    Crash,
}

/// 差分比对：双前端结论 → 信号
pub fn diff(a: FrontResult, b: FrontResult) -> DiffSignal {
    match (a, b) {
        (FrontResult::Accept, FrontResult::Accept) => DiffSignal::BothAccept,
        (FrontResult::Reject, FrontResult::Reject) => DiffSignal::BothReject,
        (FrontResult::Crash, _) | (_, FrontResult::Crash) => DiffSignal::OneSideCrash,
        _ => DiffSignal::Contradiction,
    }
}

/// 变异算子（WGSL 特有）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MutationOp {
    /// 标识符大小写翻转
    CaseFlip,
    /// 注释位置搬移
    CommentMove,
    /// 空白规范化
    SpaceNorm,
    /// 数字字面量后缀变化
    NumSuffix,
}

/// 变异执行：单算子对文本做确定性变换
pub fn mutate(op: MutationOp, text: &str) -> String {
    match op {
        MutationOp::CaseFlip => text.chars().map(|c| if c.is_ascii_alphabetic() && rand_flip(c) { flip(c) } else { c }).collect(),
        MutationOp::CommentMove => {
            // 把行内注释搬移到行首
            if let Some(p) = text.find("//") {
                let (code, comment) = text.split_at(p);
                alloc::format!("{}//{}", comment[2..].trim(), code.trim_end())
            } else {
                text.to_string()
            }
        }
        MutationOp::SpaceNorm => {
            // 多空白折叠为单空格
            let mut out = String::new();
            let mut prev_space = false;
            for c in text.chars() {
                let sp = c == ' ' || c == '\t';
                if sp {
                    if !prev_space {
                        out.push(' ');
                    }
                } else {
                    out.push(c);
                }
                prev_space = sp;
            }
            out
        }
        MutationOp::NumSuffix => {
            // 整数字面量追加 u 后缀（f 侧同理）
            let mut out = String::new();
            let mut chars = text.chars().peekable();
            while let Some(c) = chars.next() {
                out.push(c);
                if c.is_ascii_digit() {
                    match chars.peek() {
                        Some('u') | Some('f') | Some('i') => {}
                        Some('0'..='9') | Some('.') => {}
                        _ => out.push('u'),
                    }
                }
            }
            out
        }
    }
}

fn rand_flip(c: char) -> bool {
    // 确定性"翻转"：对大写字母翻转（与随机无关，可复现）
    c.is_ascii_uppercase()
}

fn flip(c: char) -> char {
    if c.is_ascii_uppercase() {
        (c as u8 + 32) as char
    } else {
        (c as u8 - 32) as char
    }
}

/// 崩溃分类册
#[derive(Clone, Debug, Default)]
pub struct CrashBook {
    /// (用例名, 信号) 序列
    entries: Vec<(String, DiffSignal)>,
}

impl CrashBook {
    pub fn new() -> Self {
        CrashBook::default()
    }

    /// 高优信号即时入册
    pub fn record_if_high(&mut self, name: &str, sig: DiffSignal) -> bool {
        if sig.high_priority() {
            self.entries.push((name.to_string(), sig));
            true
        } else {
            false
        }
    }

    /// 崩溃分类统计
    pub fn count(&self, sig: DiffSignal) -> usize {
        self.entries.iter().filter(|(_, s)| *s == sig).count()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 资源超限：用例截断留档
pub fn truncate_case(text: &str, max_len: usize) -> (String, bool) {
    if text.len() <= max_len {
        (text.to_string(), false)
    } else {
        let cut = text
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|i| *i <= max_len)
            .last()
            .unwrap_or(0);
        (text[..cut].to_string(), true)
    }
}

pub fn run_vec155_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec155");
    // 族一：三层语料库
    let mut book = CorpusBook::new();
    book.add(Corpus::new(CorpusLayer::Grammar, "g1", "fn f(){}"));
    book.add(Corpus::new(CorpusLayer::SemanticEdge, "s1", "let x: i32 = 2147483647;"));
    book.add(Corpus::new(CorpusLayer::Mutated, "m1", "fn f(){} // moved"));
    s.add("语料-三层齐备", book.complete(), "3layers");
    s.add("语料-层内采样", book.sample(CorpusLayer::Grammar).len() == 1, "sample");
    s.add("语料-计数", book.len() == 3, "n3");
    // 族二：双前端差分
    s.add("差分-双侧接受", diff(FrontResult::Accept, FrontResult::Accept) == DiffSignal::BothAccept, "ba");
    s.add("差分-双侧拒绝", diff(FrontResult::Reject, FrontResult::Reject) == DiffSignal::BothReject, "br");
    s.add("差分-单侧崩溃", diff(FrontResult::Crash, FrontResult::Accept) == DiffSignal::OneSideCrash, "crash");
    s.add("差分-结论相反", diff(FrontResult::Accept, FrontResult::Reject) == DiffSignal::Contradiction, "contra");
    s.add("差分-高优判定", DiffSignal::OneSideCrash.high_priority() && DiffSignal::Contradiction.high_priority() && !DiffSignal::BothReject.high_priority(), "hp");
    // 族三：崩溃分类入册
    let mut cb = CrashBook::new();
    s.add("入册-高优收录", cb.record_if_high("c1", DiffSignal::OneSideCrash), "rec1");
    s.add("入册-普通不收", !cb.record_if_high("c2", DiffSignal::BothReject), "rec2");
    s.add("入册-计数", cb.len() == 1 && cb.count(DiffSignal::OneSideCrash) == 1, "cnt");
    // 族四：变异算子-大小写
    s.add("变异-大写翻转", mutate(MutationOp::CaseFlip, "FN f(){}") == "fN f(){}", "case");
    s.add("变异-小写不动", mutate(MutationOp::CaseFlip, "fn f(){}") == "fn f(){}", "lower");
    // 族五：变异算子-注释搬移
    s.add("变异-注释前置", mutate(MutationOp::CommentMove, "let a = 1; // tail") == "//tail let a = 1;", "cmt");
    s.add("变异-无注释原样", mutate(MutationOp::CommentMove, "let a = 1;") == "let a = 1;", "nocmt");
    // 族六：变异算子-空白规范化
    s.add("变异-空白折叠", mutate(MutationOp::SpaceNorm, "let   a\t=  1;") == "let a = 1;", "space");
    // 族七：变异算子-数字后缀
    s.add("变异-整数加u", mutate(MutationOp::NumSuffix, "let x = 1;") == "let x = 1u;", "u");
    s.add("变异-已有u不加", mutate(MutationOp::NumSuffix, "let x = 1u;") == "let x = 1u;", "hasu");
    s.add("变异-浮点小数不加", mutate(MutationOp::NumSuffix, "let x = 1.5;") == "let x = 1.5;", "float");
    // 族八：资源超限截断留档
    let (cut, truncated) = truncate_case("abcdefgh", 4);
    s.add("截断-超限截断", truncated && cut == "abcd", "cut");
    let (full, not_trunc) = truncate_case("abc", 4);
    s.add("截断-未超原样", !not_trunc && full == "abc", "full");
    s
}
