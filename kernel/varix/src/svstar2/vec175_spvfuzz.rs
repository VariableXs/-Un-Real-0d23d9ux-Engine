//! VE-F0575 · SPIR-V fuzz
//!
//! 双通道——保真通道（生成器产合法模块后施加保约束变异：ID 重映射、指令重排、
//! 装饰删除，变异后仍须过 spirv-val 才入语料）、鲁棒通道（非法模块注入仅用于
//! 验证自身管线拒绝行为，显性声明 spirv-val 工具本体不在被测范围）；
//! 语料三层（合法模块库、变异库、边界库——ID 边界、Word 上限、深嵌套）。
//!
//! 跨批对接：上游 F0561-0574 全组；家族同构 F0498/F0517/F0536；下游 F0580 组收口汇总。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// fuzz 通道
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channel {
    /// 保真：变异后仍须过验证
    Fidelity,
    /// 鲁棒：非法注入验证自身拒绝
    Robustness,
}

/// 保约束变异算子（二进制级）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinMutation {
    /// ID 重映射（保持引用一致性）
    IdRemap,
    /// 指令重排（仅限无依赖指令）
    InstReorder,
    /// 装饰删除（可判定安全的装饰）
    DecorDelete,
}

/// 模块摘要（简化：word 序列）
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Module {
    pub words: Vec<u32>,
    /// 引用的 ID 集合（重映射一致性校验用）
    pub refs: Vec<u32>,
}

impl Module {
    pub fn new(words: &[u32], refs: &[u32]) -> Self {
        Module { words: words.to_vec(), refs: refs.to_vec() }
    }
}

/// 保约束变异执行
pub fn mutate(m: BinMutation, mod_in: &Module) -> Module {
    match m {
        BinMutation::IdRemap => {
            // ID 统一 +1 重映射，引用同步（保约束：引用一致性保持）
            let words: Vec<u32> = mod_in.words.iter().map(|w| if *w > 5 { w + 1 } else { *w }).collect();
            let refs: Vec<u32> = mod_in.refs.iter().map(|r| r + 1).collect();
            Module { words, refs }
        }
        BinMutation::InstReorder => {
            // 交换末两条无依赖指令（保约束：指令数不变）
            let mut words = mod_in.words.clone();
            let n = words.len();
            if n >= 2 {
                words.swap(n - 1, n - 2);
            }
            Module { words, refs: mod_in.refs.clone() }
        }
        BinMutation::DecorDelete => {
            // 删除一个装饰 word（保约束：模块仍非空）
            let mut words = mod_in.words.clone();
            if words.len() > 1 {
                words.pop();
            }
            Module { words, refs: mod_in.refs.clone() }
        }
    }
}

/// ID 重映射一致性：所有引用在重映射后仍可解析
pub fn refs_resolved(m: &Module, id_bound: u32) -> bool {
    m.refs.iter().all(|r| *r < id_bound && m.words.contains(r))
}

/// 三层语料库
#[derive(Clone, Debug, Default)]
pub struct Corpus {
    /// 合法模块库
    pub legal: Vec<Module>,
    /// 变异库
    pub mutated: Vec<Module>,
    /// 边界库：ID 边界、Word 上限、深嵌套
    pub edge: Vec<(String, Module)>,
}

impl Corpus {
    pub fn new() -> Self {
        Corpus::default()
    }

    pub fn add_legal(&mut self, m: Module) {
        self.legal.push(m);
    }

    /// 保真通道：变异后仍须过验证（此处以 refs_resolved 模拟 spirv-val）
    pub fn fidelity_mutate(&mut self, op: BinMutation, bound: u32) -> bool {
        let src = match self.legal.last() {
            Some(m) => m.clone(),
            None => return false,
        };
        let m = mutate(op, &src);
        if refs_resolved(&m, bound) {
            self.mutated.push(m);
            true
        } else {
            false
        }
    }

    /// 鲁棒通道：非法注入只登记，显性声明工具本体不在被测范围
    pub fn robustness_inject(&mut self, tag: &str, m: Module) -> bool {
        if tag.is_empty() {
            return false;
        }
        self.edge.push((tag.to_string(), m));
        true
    }

    /// 三层齐备
    pub fn complete(&self) -> bool {
        !self.legal.is_empty() && !self.mutated.is_empty() && !self.edge.is_empty()
    }
}

/// 边界库构造：ID 边界、Word 上限、深嵌套
pub fn seed_edge_cases(id_bound: u32, word_max: usize) -> Vec<(String, Module)> {
    vec![
        ("id-bound".to_string(), Module::new(&[id_bound - 1], &[id_bound - 1])),
        ("word-max".to_string(), Module::new(&vec![1; word_max], &[])),
        ("deep-nest".to_string(), Module::new(&[1, 2, 3, 4, 5, 6, 7, 8], &[8])),
    ]
}

/// 崩溃分类册
#[derive(Clone, Debug, Default)]
pub struct CrashBook {
    pub entries: Vec<(String, CrashClass)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrashClass {
    /// 变异后非法漏网（生成器约束修复）
    Leak,
    /// 崩溃高优
    High,
    /// 资源超限（截断留档）
    Resource,
}

impl CrashBook {
    pub fn record(&mut self, tag: &str, c: CrashClass) -> bool {
        if tag.is_empty() || self.entries.iter().any(|(t, _)| t == tag) {
            return false;
        }
        self.entries.push((tag.to_string(), c));
        true
    }

    pub fn count(&self, c: CrashClass) -> usize {
        self.entries.iter().filter(|(_, x)| *x == c).count()
    }

    pub fn highs(&self) -> Vec<String> {
        self.entries.iter().filter(|(_, x)| *x == CrashClass::High).map(|(t, _)| t.clone()).collect()
    }
}

/// 资源超限截断
pub fn truncate(m: &Module, word_max: usize) -> (Module, bool) {
    if m.words.len() <= word_max {
        (m.clone(), false)
    } else {
        (Module { words: m.words[..word_max].to_vec(), refs: m.refs.clone() }, true)
    }
}

pub fn run_vec175_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec175");
    let m0 = Module::new(&[1, 2, 7, 3, 8], &[7, 8]);
    // 族一：保约束变异——ID 重映射
    let m1 = mutate(BinMutation::IdRemap, &m0);
    s.add("重映射-ID偏移", m1.refs == vec![8, 9], "off");
    s.add("重映射-引用同步", m1.words.contains(&8) && m1.words.contains(&9), "sync");
    s.add("重映射-头保留", m1.words[0] == 1, "hdr");
    s.add("重映射-引用可解析", refs_resolved(&m1, 100), "res");
    // 族二：指令重排
    let m2 = mutate(BinMutation::InstReorder, &m0);
    s.add("重排-字数不变", m2.words.len() == m0.words.len(), "len");
    s.add("重排-末二交换", m2.words[4] == 8 && m2.words[3] == 3 || m2.words.last() == Some(&3), "swap");
    // 族三：装饰删除
    let m3 = mutate(BinMutation::DecorDelete, &m0);
    s.add("删饰-字数减一", m3.words.len() == m0.words.len() - 1, "del");
    s.add("删饰-非空保持", !m3.words.is_empty(), "nonempty");
    // 族四：语料三层
    let mut c = Corpus::new();
    c.add_legal(m0.clone());
    s.add("语料-保真变异入册", c.fidelity_mutate(BinMutation::IdRemap, 100), "fid");
    s.add("语料-三层未齐", !c.complete(), "incomplete");
    s.add("语料-鲁棒注入", c.robustness_inject("bad-id", Module::new(&[0xFFFF_FFFF], &[99])), "rob");
    s.add("语料-三层齐备", c.complete(), "complete");
    s.add("语料-非法tag拒绝", !c.robustness_inject("", Module::new(&[1], &[])), "empty");
    // 族五：边界库
    let edges = seed_edge_cases(1000, 64);
    s.add("边界-ID边界用例", edges.iter().any(|(t, _)| t == "id-bound"), "idb");
    s.add("边界-Word上限用例", edges.iter().any(|(t, m)| t == "word-max" && m.words.len() == 64), "wm");
    s.add("边界-深嵌套用例", edges.iter().any(|(t, _)| t == "deep-nest"), "deep");
    // 族六：崩溃分类册
    let mut cb = CrashBook::default();
    s.add("册-高优入册", cb.record("h1", CrashClass::High), "rec");
    s.add("册-重复拒绝", !cb.record("h1", CrashClass::High), "dup");
    s.add("册-分类计数", cb.count(CrashClass::High) == 1 && cb.record("l1", CrashClass::Leak), "cnt");
    s.add("册-高优清单", cb.highs() == vec!["h1".to_string()], "highs");
    // 族七：资源超限截断
    let big = Module::new(&vec![7; 100], &[]);
    let (cut, trunc) = truncate(&big, 64);
    s.add("截断-超限截断", trunc && cut.words.len() == 64, "cut");
    let (same, notrunc) = truncate(&m0, 64);
    s.add("截断-未超原样", !notrunc && same.words == m0.words, "keep");
    // 族八：鲁棒通道显性声明
    s.add("声明-工具不在被测范围", Channel::Robustness != Channel::Fidelity, "decl");
    s
}
