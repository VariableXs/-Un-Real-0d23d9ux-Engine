//! unx4187_loopdetect — UNX-F4187 自指/互指环路检测回归（诊断段 0x98E0）。
//!
//! 判据原文：
//!   J1 UNX-F4187-J1 自指（a→a）与互指（a→b→a）两类环 100/100 次返回
//!      ELOOP 不挂死不崩，深环（40 层自构造链）与计数上限行为一致。
//! 设计纪律：
//!   1. 环检测双口径：命中检测（走过槽位 revisit 即环，先于计数上限
//!      触发）与计数上限（40 层 MAXSYMLINKS 口径），两类最终都归 ELOOP。
//!   2. 环分类闭集：SelfLoop（a→a 单槽自指）、Mutual（a→b→a 互指）、
//!      CapBound（无 revisit 但触 40 层上限）、None（无环落点）。
//!   3. 一致性：自指/互指/深环三类注入的返回码同为 ELOOP，错误码不分
//!      环形（POSIX 口径），分类账仅作内部诊断分账。
//!   4. 不挂死不崩：所有展开以显式循环承载，任何输入至多 41 次迭代即
//!      终局（检测命中或上限触界），无递归无墙钟。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x98E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ELOOP: u16 = SEG_BASE + 1;     // 环命中或超 40 层（统一 ELOOP 口径）
const E_NOT_ABS: u16 = SEG_BASE + 2;   // 路径非绝对
const E_BAD_COMP: u16 = SEG_BASE + 3;  // 空分量
const E_NOENT: u16 = SEG_BASE + 4;     // 分量/链接目标不存在
const E_BAD_NODE: u16 = SEG_BASE + 5;  // 节点槽越界
const E_DUP_NAME: u16 = SEG_BASE + 6;  // 同目录重名
const E_LEDGER: u16 = SEG_BASE + 7;    // 分类账与注入账不平

/// Linux MAXSYMLINKS 口径：40（深环与计数上限一致性基准）。
pub const MAXSYMLINKS: u32 = 40;
/// 判据注入次数（自指/互指各 100 次）。
pub const INJECT_TIMES: u32 = 100;

/// 环分类闭集（内部诊断分账，错误码不分环形）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CycleKind {
    None,     // 无环落点
    SelfLoop, // a→a 单槽自指
    Mutual,   // a→b→a 互指
    CapBound, // 无 revisit 但触 40 层上限
}

/// 检测状态机：Scanning→Detected→(触界)Capped→Settled。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DetectPhase {
    Scanning,
    Detected,
    Capped,
    Settled,
}

/// 分类账：四类计数 + 注入命中账。
pub struct CycleBook {
    pub self_loops: u32,
    pub mutuals: u32,
    pub cap_bounds: u32,
    pub non_loops: u32,
    pub eloop_total: u32, // ELOOP 返回总数（三类环同码合计）
}

impl CycleBook {
    pub fn new() -> CycleBook {
        CycleBook {
            self_loops: 0,
            mutuals: 0,
            cap_bounds: 0,
            non_loops: 0,
            eloop_total: 0,
        }
    }

    /// 一致性判：ELOOP 总数=三类环账合计（同码不分环）。
    pub fn consistent(&self) -> bool {
        self.eloop_total == self.self_loops + self.mutuals + self.cap_bounds
    }
}

/// 环检测解析树。
pub struct CNode {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub target: String,
    pub children: Vec<usize>,
}

pub struct LoopTree {
    pub nodes: Vec<CNode>,
    pub book: CycleBook,
    pub phase: DetectPhase,
}

impl LoopTree {
    pub fn new() -> LoopTree {
        LoopTree {
            nodes: vec![CNode {
                name: String::from("/"),
                is_dir: true,
                is_link: false,
                target: String::new(),
                children: Vec::new(),
            }],
            book: CycleBook::new(),
            phase: DetectPhase::Settled,
        }
    }

    pub fn add(&mut self, parent: usize, name: &str, is_dir: bool) -> Result<usize, u16> {
        self.add_any(parent, name, is_dir, false, "")
    }

    pub fn add_link(&mut self, parent: usize, name: &str, target: &str) -> Result<usize, u16> {
        self.add_any(parent, name, false, true, target)
    }

    fn add_any(
        &mut self,
        parent: usize,
        name: &str,
        is_dir: bool,
        is_link: bool,
        target: &str,
    ) -> Result<usize, u16> {
        if parent >= self.nodes.len() {
            return Err(E_BAD_NODE);
        }
        if name.is_empty() || name.contains('/') {
            return Err(E_BAD_COMP);
        }
        if self.child_of(parent, name).is_some() {
            return Err(E_DUP_NAME);
        }
        let id = self.nodes.len();
        self.nodes.push(CNode {
            name: name.to_string(),
            is_dir,
            is_link,
            target: target.to_string(),
            children: Vec::new(),
        });
        self.nodes[parent].children.push(id);
        Ok(id)
    }

    fn child_of(&self, dir: usize, name: &str) -> Option<usize> {
        if dir >= self.nodes.len() {
            return None;
        }
        for &c in &self.nodes[dir].children {
            if c < self.nodes.len() && self.nodes[c].name == name {
                return Some(c);
            }
        }
        None
    }

    fn split<'a>(&self, path: &'a str) -> Result<Vec<&'a str>, u16> {
        if !path.starts_with('/') {
            return Err(E_NOT_ABS);
        }
        let body = &path[1..]; // '/' 单字节，前缀判定后切片安全
        if body.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for c in body.split('/') {
            if c.is_empty() {
                return Err(E_BAD_COMP);
            }
            out.push(c);
        }
        Ok(out)
    }

    fn resolve_abs(&self, path: &str) -> Result<usize, u16> {
        let comps = self.split(path)?;
        let mut cur = 0usize;
        for c in comps {
            cur = self.child_of(cur, c).ok_or(E_NOENT)?;
        }
        Ok(cur)
    }

    /// 展开层检测：visited 账（槽位 revisit 即环）+ 40 层上限双口径。
    /// 返回（落点或触界层，分类）。
    fn expand(&self, mut node: usize) -> (Result<usize, u16>, CycleKind) {
        let mut seen = Vec::new();
        let mut layers = 0u32;
        loop {
            if !self.nodes[node].is_link {
                return (Ok(node), CycleKind::None);
            }
            // revisit 检测：走过槽位再现即环（先于计数上限）。
            let mut i = 0usize;
            while i < seen.len() {
                if seen[i] == node {
                    let kind = if seen.len() == 1 {
                        CycleKind::SelfLoop // 单槽自指
                    } else {
                        CycleKind::Mutual // 多槽互指
                    };
                    return (Err(E_ELOOP), kind);
                }
                i += 1;
            }
            seen.push(node);
            if layers >= MAXSYMLINKS {
                // 无 revisit 但计数触界：深环与上限行为一致。
                return (Err(E_ELOOP), CycleKind::CapBound);
            }
            layers += 1;
            match self.resolve_abs(&self.nodes[node].target.clone()) {
                Ok(t) => node = t,
                Err(e) => return (Err(e), CycleKind::None),
            }
        }
    }

    /// walk 主入口：对链接分量做环检测展开，分类入账。
    pub fn walk(&mut self, path: &str) -> Result<usize, u16> {
        let comps = self.split(path)?;
        if comps.is_empty() {
            self.phase = DetectPhase::Settled;
            self.book.non_loops += 1;
            return Ok(0);
        }
        self.phase = DetectPhase::Scanning;
        let mut cur = 0usize;
        let n = comps.len();
        let mut i = 0usize;
        while i < n {
            let next = self.child_of(cur, comps[i]).ok_or(E_NOENT)?;
            if self.nodes[next].is_link {
                let (res, kind) = self.expand(next);
                match res {
                    Ok(t) => {
                        self.book.non_loops += 1;
                        cur = t;
                    }
                    Err(e) => {
                        match kind {
                            CycleKind::SelfLoop => self.book.self_loops += 1,
                            CycleKind::Mutual => self.book.mutuals += 1,
                            CycleKind::CapBound => self.book.cap_bounds += 1,
                            CycleKind::None => {}
                        }
                        if e == E_ELOOP {
                            self.book.eloop_total += 1;
                        }
                        self.phase = DetectPhase::Detected;
                        return Err(e);
                    }
                }
            } else {
                cur = next;
            }
            i += 1;
        }
        self.phase = DetectPhase::Settled;
        Ok(cur)
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_ELOOP => "环命中或超 40 层（统一 ELOOP 口径）",
        E_NOT_ABS => "路径非绝对",
        E_BAD_COMP => "空分量",
        E_NOENT => "分量/链接目标不存在",
        E_BAD_NODE => "节点槽越界",
        E_DUP_NAME => "同目录重名",
        E_LEDGER => "分类账与注入账不平",
        _ => "未知诊断码",
    }
}

/// 确定性演练：自指 100→互指 100→深环一致→无环对照→注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建面——真身 /real、自指 a→a、互指 b→c、c→b。
    let mut t = LoopTree::new();
    let real = t.add(0, "real", true)?;
    t.add_link(0, "a", "/a")?;   // 自指
    t.add_link(0, "b", "/c")?;   // 互指半环
    t.add_link(0, "c", "/b")?;   // 互指回环
    hits += 1;

    // R2：自指 100/100——a→a 每次返回 ELOOP，分类账 self_loops。
    let mut n = 0u32;
    while n < INJECT_TIMES {
        if t.walk("/a") != Err(E_ELOOP) {
            return Err(E_ELOOP);
        }
        n += 1;
    }
    if t.book.self_loops != INJECT_TIMES {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3：互指 100/100——b→c→b 每次返回 ELOOP，分类账 mutuals。
    let mut n = 0u32;
    while n < INJECT_TIMES {
        if t.walk("/b") != Err(E_ELOOP) {
            return Err(E_ELOOP);
        }
        n += 1;
    }
    if t.book.mutuals != INJECT_TIMES {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4：深环（40 层自构造链）与计数上限行为一致——c1→…→c41 触界
    // 亦返回 ELOOP（同码），分类账 cap_bounds。
    let mut tgt = String::from("/real");
    let mut idx = 40usize;
    while idx >= 1 {
        let mut name = String::from("d");
        name.push((b'0' + (idx / 10) as u8) as char);
        name.push((b'0' + (idx % 10) as u8) as char);
        t.add_link(0, &name, &tgt)?;
        tgt.clear();
        tgt.push('/');
        tgt.push_str(&name);
        idx -= 1;
    }
    t.add_link(0, "d41", "/d40")?; // 第 41 层触界点
    if t.walk("/d41") != Err(E_ELOOP) {
        return Err(E_ELOOP);
    }
    if t.book.cap_bounds != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5：无环对照——/real 正常落点，non_loops 入账，一致性对平。
    let r = t.walk("/real")?;
    if r != real {
        return Err(E_LEDGER);
    }
    if !t.book.consistent() {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I1：断裂目标（非环的 E_NOENT）与相对路径注入。
    t.add_link(0, "dead", "/no/such")?;
    if t.walk("/dead") != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    if t.walk("real") != Err(E_NOT_ABS) {
        return Err(E_NOT_ABS);
    }
    if t.walk("/real//x") != Err(E_BAD_COMP) {
        return Err(E_BAD_COMP);
    }
    hits += 1;

    // I2：不挂死不崩复核——对环名连续 3 次快速注入，至多 2~41 次迭代
    // 即终局（结构保证），账面一致性再对平。
    let mut n = 0u32;
    while n < 3 {
        if t.walk("/a") != Err(E_ELOOP) {
            return Err(E_ELOOP);
        }
        n += 1;
    }
    if !matches!(t.phase, DetectPhase::Detected) || !t.book.consistent() {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R6：终审——三类环账合计=ELOOP 总账。
    if t.book.eloop_total != INJECT_TIMES * 2 + 1 + 3 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
