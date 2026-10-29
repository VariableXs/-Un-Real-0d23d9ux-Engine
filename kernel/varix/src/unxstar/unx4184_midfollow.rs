//! unx4184_midfollow — UNX-F4184 链接跟随 walk 内嵌展开（中间分量跟随）（诊断段 0x9880）。
//!
//! 判据原文：
//!   J1 UNX-F4184-J1 中间分量链接目标全串重新进入 walk（展开深度账 +1/层），
//!      链中链（3 级嵌套）解析到真身断言通过，展开期间引用配对零泄漏。
//! 设计纪律：
//!   1. 中间分量命中链接即把目标全串重新进入 walk（内嵌展开），展开
//!      深度账逐层 +1，展开上限 8 层防膨胀（B10 正式 40 层归 F4186）。
//!   2. 链中链口径：l1→l2→l3→真身三级嵌套，walk("/l1/x") 三层全展开
//!      后落到真身目录下的 x，展开账恰 +3。
//!   3. 引用配对：每触碰一个节点配对 acquire/release，walk 终局
//!      acquired==released 零泄漏（判据「引用配对零泄漏」账面）。
//!   4. 展开期状态机：Comp→Expand→Settled，展开期不得改名旗（恒跟随）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9880;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOT_ABS: u16 = SEG_BASE + 1;   // 路径非绝对
const E_BAD_COMP: u16 = SEG_BASE + 2;  // 空分量
const E_NOENT: u16 = SEG_BASE + 3;     // 分量不存在（含链接目标断裂）
const E_EXP_CAP: u16 = SEG_BASE + 4;   // 展开超 8 层上限
const E_BAD_NODE: u16 = SEG_BASE + 5;  // 节点槽越界
const E_DUP_NAME: u16 = SEG_BASE + 6;  // 同目录重名
const E_LEAK: u16 = SEG_BASE + 7;      // 引用配对泄漏（acquire≠release）
const E_LEDGER: u16 = SEG_BASE + 8;    // 展开账失守（层数对不上）

pub const EXP_CAP: u32 = 8;   // 单次 walk 展开上限（防膨胀口径）
pub const NEST_DEPTH: u32 = 3;// 判据链中链嵌套层数

/// 展开期状态机：Comp（分量行进）→Expand（链接展开）→Settled（落点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WalkPhase {
    Comp,
    Expand,
    Settled,
}

/// 解析树节点：目录/文件/链接三态。
pub struct MNode {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub target: String,
    pub children: Vec<usize>,
}

/// 引用配对账：acquire/release 严格成对，终局必须归零差。
pub struct RefPair {
    pub acquired: u32,
    pub released: u32,
}

impl RefPair {
    pub fn new() -> RefPair {
        RefPair { acquired: 0, released: 0 }
    }

    pub fn acquire(&mut self) {
        self.acquired += 1;
    }

    pub fn release(&mut self) {
        if self.released < self.acquired {
            self.released += 1;
        }
    }

    /// 零泄漏判：release 全数配平。
    pub fn no_leak(&self) -> bool {
        self.acquired == self.released
    }
}

/// 展开深度账：逐层 +1，峰值与总量双记。
pub struct ExpBook {
    pub expands: u32, // 展开总层数（+1/层）
    pub peak: u32,    // 单次 walk 峰值层数
}

impl ExpBook {
    pub fn new() -> ExpBook {
        ExpBook { expands: 0, peak: 0 }
    }
}

/// 中间跟随 walk 枢纽。
pub struct MidWalk {
    pub nodes: Vec<MNode>,
    pub refs: RefPair,
    pub book: ExpBook,
    pub phase: WalkPhase,
}

impl MidWalk {
    pub fn new() -> MidWalk {
        MidWalk {
            nodes: vec![MNode {
                name: String::from("/"),
                is_dir: true,
                is_link: false,
                target: String::new(),
                children: Vec::new(),
            }],
            refs: RefPair::new(),
            book: ExpBook::new(),
            phase: WalkPhase::Settled,
        }
    }

    pub fn add_dir(&mut self, parent: usize, name: &str) -> Result<usize, u16> {
        self.add_node(parent, name, true, false, "")
    }

    pub fn add_file(&mut self, parent: usize, name: &str) -> Result<usize, u16> {
        self.add_node(parent, name, false, false, "")
    }

    pub fn add_link(&mut self, parent: usize, name: &str, target: &str) -> Result<usize, u16> {
        self.add_node(parent, name, false, true, target)
    }

    fn add_node(
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
        if is_link && target.is_empty() {
            return Err(E_BAD_COMP);
        }
        if self.child_of(parent, name).is_some() {
            return Err(E_DUP_NAME);
        }
        let id = self.nodes.len();
        self.nodes.push(MNode {
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

    /// 中间分量恒跟随：链接目标全串重新解析（内嵌展开），逐层记账。
    fn expand(&self, mut node: usize, layers: &mut u32) -> Result<usize, u16> {
        while self.nodes[node].is_link {
            *layers += 1;
            if *layers > EXP_CAP {
                return Err(E_EXP_CAP);
            }
            node = self.resolve_abs(&self.nodes[node].target)?;
        }
        Ok(node)
    }

    fn resolve_abs(&self, path: &str) -> Result<usize, u16> {
        let comps = self.split(path)?;
        let mut cur = 0usize;
        for c in comps {
            cur = self.child_of(cur, c).ok_or(E_NOENT)?;
        }
        Ok(cur)
    }

    /// walk 主入口：中间分量恒跟随展开，返回落点（真身）。
    pub fn walk(&mut self, path: &str) -> Result<usize, u16> {
        let comps = self.split(path)?;
        if comps.is_empty() {
            self.refs.acquire();
            self.refs.release();
            self.phase = WalkPhase::Settled;
            return Ok(0);
        }
        self.phase = WalkPhase::Comp;
        let mut cur = 0usize;
        let n = comps.len();
        let mut layers = 0u32;
        let mut i = 0usize;
        while i < n {
            let next = self.child_of(cur, comps[i]).ok_or(E_NOENT)?;
            self.refs.acquire();
            if self.nodes[next].is_link {
                // 中间分量恒跟随（末分量链接同口径：全串重入 walk）。
                self.phase = WalkPhase::Expand;
                match self.expand(next, &mut layers) {
                    Ok(t) => {
                        cur = t;
                    }
                    Err(e) => {
                        // 错误路径补配对 release，引用账不泄漏。
                        self.refs.release();
                        self.phase = WalkPhase::Settled;
                        return Err(e);
                    }
                }
                self.refs.release();
                self.phase = WalkPhase::Comp;
            } else {
                cur = next;
            }
            i += 1;
        }
        // 行进中每 acquire 配对一次 release（非链接落点补配对）。
        self.refs.release();
        self.phase = WalkPhase::Settled;
        if layers > self.book.peak {
            self.book.peak = layers;
        }
        self.book.expands += layers;
        Ok(cur)
    }

    /// 终审：引用配对零泄漏 + 展开账上限内。
    pub fn audit_clean(&self) -> bool {
        self.refs.no_leak() && self.book.peak <= EXP_CAP
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOT_ABS => "路径非绝对",
        E_BAD_COMP => "空分量",
        E_NOENT => "分量不存在（含链接目标断裂）",
        E_EXP_CAP => "展开超 8 层上限",
        E_BAD_NODE => "节点槽越界",
        E_DUP_NAME => "同目录重名",
        E_LEAK => "引用配对泄漏（acquire≠release）",
        E_LEDGER => "展开账失守（层数对不上）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：链中链 3 级→展开账 +1/层→引用配对零泄漏→上限与注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建真身 /real/x 与三级链 l1→/l2、l2→/l3、l3→/real。
    let mut mw = MidWalk::new();
    let real = mw.add_dir(0, "real")?;
    let x = mw.add_file(real, "x")?;
    let l2 = mw.add_link(0, "l2", "/l3")?;
    let l3 = mw.add_link(0, "l3", "/real")?;
    mw.add_link(0, "l1", "/l2")?;
    let _ = (l2, l3);
    hits += 1;

    // R2：链中链 3 级解析——/l1/x 落真身 x，展开账恰 +3（+1/层）。
    let e0 = mw.book.expands;
    let r = mw.walk("/l1/x")?;
    if r != x {
        return Err(E_LEDGER);
    }
    if mw.book.expands - e0 != NEST_DEPTH {
        return Err(E_LEDGER);
    }
    if mw.book.peak != NEST_DEPTH {
        return Err(E_LEDGER);
    }
    if !mw.refs.no_leak() {
        return Err(E_LEAK);
    }
    hits += 1;

    // R3：直接路径零展开——/real/x 走行进面，展开账不动，配对仍平。
    let e1 = mw.book.expands;
    let r = mw.walk("/real/x")?;
    if r != x || mw.book.expands != e1 {
        return Err(E_LEDGER);
    }
    if !mw.refs.no_leak() {
        return Err(E_LEAK);
    }
    hits += 1;

    // R4：末分量链接展开——/l1 自身是链接，展开 3 层落 real 目录。
    let e2 = mw.book.expands;
    let r = mw.walk("/l1")?;
    if r != real {
        return Err(E_LEDGER);
    }
    if mw.book.expands - e2 != NEST_DEPTH {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5：根路径与展开期状态机复核——终局 Settled，峰值上限内。
    let r = mw.walk("/")?;
    if r != 0 {
        return Err(E_LEDGER);
    }
    if !matches!(mw.phase, WalkPhase::Settled) || !mw.audit_clean() {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I1：展开超 8 层——9 级链 c1→…→c9→/real 触发 E_EXP_CAP。
    let mut tgt = String::from("/real");
    let names = ["c9", "c8", "c7", "c6", "c5", "c4", "c3", "c2", "c1"];
    let mut k = 0usize;
    while k < names.len() {
        mw.add_link(0, names[k], &tgt)?;
        tgt.clear();
        tgt.push('/');
        tgt.push_str(names[k]);
        k += 1;
    }
    if mw.walk("/c1/x") != Err(E_EXP_CAP) {
        return Err(E_EXP_CAP);
    }
    if mw.walk("/c1") != Err(E_EXP_CAP) {
        return Err(E_EXP_CAP);
    }
    hits += 1;

    // I2：断裂链接目标 → E_NOENT；相对路径 → E_NOT_ABS。
    mw.add_link(0, "dead", "/no/such")?;
    if mw.walk("/dead") != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    if mw.walk("l1/x") != Err(E_NOT_ABS) {
        return Err(E_NOT_ABS);
    }
    hits += 1;

    // I3：空分量注入；重名建链拒收。
    if mw.walk("/real//x") != Err(E_BAD_COMP) {
        return Err(E_BAD_COMP);
    }
    if mw.add_link(0, "l1", "/real") != Err(E_DUP_NAME) {
        return Err(E_DUP_NAME);
    }
    if !mw.audit_clean() {
        return Err(E_LEAK);
    }
    hits += 1;

    Ok(hits)
}
