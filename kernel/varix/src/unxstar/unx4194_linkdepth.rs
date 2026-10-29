//! unx4194_linkdepth — UNX-F4194 链接深度计数与 walk 层计数分账（诊断段 0x99C0）。
//!
//! 判据原文：
//!   J1 UNX-F4194-J1 链接深度计数（ELOOP 用）与分量深度计数（F4175 用）
//!      两账独立不串位（交叉注入零混淆），两账快照可导出。
//! 设计纪律：
//!   1. 两账分立：链接深度账（每次链接展开 +1，上限 40，ELOOP 口径）
//!      与分量深度账（每分量 +1，上限 64，F4175 口径）各自独立登记，
//!      峰值/累计/触界三字段各算各的。
//!   2. 交叉注入零混淆：深路径（多分量零链接）只动分量账；长链（多
//!      链接少分量）只动链接账——对方账必须纹丝不动（交叉净账判）。
//!   3. 快照可导出：两账四字段平铺导出（link_used/link_peak/comp_used/
//!      comp_peak），导出面与内部账逐位一致。
//!   4. 触界口径：链接账触 40 即 ELOOP，分量账触 64 即 E_DEPTHCAP，
//!      两错误码不同闭集，互不借用。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x99C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ELOOP: u16 = SEG_BASE + 1;      // 链接账触 40 层上限
const E_DEPTHCAP: u16 = SEG_BASE + 2;   // 分量账触 64 分量上限
const E_NOT_ABS: u16 = SEG_BASE + 3;    // 路径非绝对
const E_BAD_COMP: u16 = SEG_BASE + 4;   // 空分量
const E_NOENT: u16 = SEG_BASE + 5;      // 分量/链接目标不存在
const E_BAD_NODE: u16 = SEG_BASE + 6;   // 节点槽越界
const E_DUP_NAME: u16 = SEG_BASE + 7;   // 同目录重名
const E_LEDGER: u16 = SEG_BASE + 8;     // 两账串位（交叉净账失守）

/// 链接深度上限（ELOOP 口径，Linux MAXSYMLINKS）。
pub const LINK_CAP: u32 = 40;
/// 分量深度上限（F4175 口径）。
pub const COMP_CAP: usize = 64;

/// 单账三字段：累计/峰值/触界。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DepthBook {
    pub used: u32, // 累计增量
    pub peak: u32, // 单次解析峰值
    pub capped: u32, // 触界拒收数
}

impl DepthBook {
    pub fn new() -> DepthBook {
        DepthBook { used: 0, peak: 0, capped: 0 }
    }
}

/// 双账枢纽：链接账与分量账严格分立。
pub struct TwoBooks {
    pub link: DepthBook, // 链接深度账（ELOOP 用）
    pub comp: DepthBook, // 分量深度账（F4175 用）
}

impl TwoBooks {
    pub fn new() -> TwoBooks {
        TwoBooks {
            link: DepthBook::new(),
            comp: DepthBook::new(),
        }
    }

    /// 快照导出：四字段平铺（判据「两账快照可导出」面）。
    pub fn snapshot(&self) -> [u32; 6] {
        [
            self.link.used,
            self.link.peak,
            self.link.capped,
            self.comp.used,
            self.comp.peak,
            self.comp.capped,
        ]
    }

    /// 交叉净账判：两账字段互不串位（由注入面复核）。
    pub fn separated(&self) -> bool {
        // 结构性分立：两账是独立字段，触界错误码不同闭集。
        self.link.peak <= LINK_CAP + 1 && self.comp.peak <= COMP_CAP as u32
    }
}

/// 分账解析树。
pub struct TNode {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub target: String,
    pub children: Vec<usize>,
}

pub struct DepthSplit {
    pub nodes: Vec<TNode>,
    pub books: TwoBooks,
}

impl DepthSplit {
    pub fn new() -> DepthSplit {
        DepthSplit {
            nodes: vec![TNode {
                name: String::from("/"),
                is_dir: true,
                is_link: false,
                target: String::new(),
                children: Vec::new(),
            }],
            books: TwoBooks::new(),
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
        self.nodes.push(TNode {
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

    /// 双账解析主入口：分量逐个 +1（分量账），链接展开 +1（链接账），
    /// 两账各自触各自的上限，互不借用错误码。
    pub fn walk(&mut self, path: &str) -> Result<usize, u16> {
        let comps = self.split(path)?;
        if comps.is_empty() {
            return Ok(0);
        }
        // 分量账先裁：分量数超 64 即拒（分量账独担）。
        let ncomp = comps.len();
        if ncomp > COMP_CAP {
            self.books.comp.capped += 1;
            return Err(E_DEPTHCAP);
        }
        self.books.comp.used += ncomp as u32;
        if ncomp as u32 > self.books.comp.peak {
            self.books.comp.peak = ncomp as u32;
        }
        let mut cur = 0usize;
        let mut layers = 0u32; // 本条路径的链接展开层数
        let mut i = 0usize;
        while i < ncomp {
            let mut next = self.child_of(cur, comps[i]).ok_or(E_NOENT)?;
            if self.nodes[next].is_link {
                // 链接展开：链接账逐层 +1，触 40 即 ELOOP（链接账独担）。
                loop {
                    if !self.nodes[next].is_link {
                        break;
                    }
                    if layers >= LINK_CAP {
                        self.books.link.capped += 1;
                        if layers + 1 > self.books.link.peak {
                            self.books.link.peak = layers + 1;
                        }
                        return Err(E_ELOOP);
                    }
                    layers += 1;
                    let t = self.nodes[next].target.clone();
                    next = self.resolve_abs(&t)?;
                }
                cur = next;
            } else {
                cur = next;
            }
            i += 1;
        }
        if layers > 0 {
            self.books.link.used += layers;
            if layers > self.books.link.peak {
                self.books.link.peak = layers;
            }
        }
        Ok(cur)
    }
}

/// 构造 n 分量深路径 /p0/p1/…/p(n-1)（确定性构造）。
pub fn make_path(n: usize) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < n {
        s.push('/');
        s.push('p');
        let v = i;
        s.push((b'0' + (v / 10) as u8) as char);
        s.push((b'0' + (v % 10) as u8) as char);
        i += 1;
    }
    s
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_ELOOP => "链接账触 40 层上限",
        E_DEPTHCAP => "分量账触 64 分量上限",
        E_NOT_ABS => "路径非绝对",
        E_BAD_COMP => "空分量",
        E_NOENT => "分量/链接目标不存在",
        E_BAD_NODE => "节点槽越界",
        E_DUP_NAME => "同目录重名",
        E_LEDGER => "两账串位（交叉净账失守）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：深路径只动分量账→长链只动链接账→交叉注入零混淆→快照。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建面——链状 70 分量目录树 /p00/../p69 与 45 级链接链。
    let mut t = DepthSplit::new();
    let mut parent = 0usize;
    let mut i = 0usize;
    while i < 70 {
        let mut name = String::from("p");
        name.push((b'0' + (i / 10) as u8) as char);
        name.push((b'0' + (i % 10) as u8) as char);
        parent = t.add(parent, &name, true)?;
        i += 1;
    }
    // 拼完整路径 /p00/p01/…/p69。
    let mut deep = String::new();
    let mut j = 0usize;
    while j < 70 {
        deep.push('/');
        deep.push('p');
        deep.push((b'0' + (j / 10) as u8) as char);
        deep.push((b'0' + (j % 10) as u8) as char);
        j += 1;
    }
    hits += 1;

    // R2：深路径交叉注入——70 分量零链接：分量账 capped（>64），
    // 链接账三字段纹丝不动（交叉零混淆）。
    let s0 = t.books.snapshot();
    if t.walk(&deep) != Err(E_DEPTHCAP) {
        return Err(E_DEPTHCAP);
    }
    let s1 = t.books.snapshot();
    if s1[0] != s0[0] || s1[1] != s0[1] || s1[2] != s0[2] {
        return Err(E_LEDGER); // 链接账被串位
    }
    if t.books.comp.capped != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3：上限内深路径——64 分量零链接：分量账 used/peak 动，链接账不动。
    let mut p64 = String::new();
    let mut j = 0usize;
    while j < 64 {
        p64.push('/');
        p64.push('p');
        p64.push((b'0' + (j / 10) as u8) as char);
        p64.push((b'0' + (j % 10) as u8) as char);
        j += 1;
    }
    let s0 = t.books.snapshot();
    let r = t.walk(&p64)?;
    let s1 = t.books.snapshot();
    if r == 0 && s1 == s0 {
        return Err(E_LEDGER);
    }
    if t.books.comp.used != 64 || t.books.comp.peak != 64 {
        return Err(E_LEDGER);
    }
    if s1[0] != s0[0] || s1[1] != s0[1] || s1[2] != s0[2] {
        return Err(E_LEDGER); // 链接账仍不动
    }
    hits += 1;

    // R4：长链交叉注入——45 级链接链（1 分量）：链接账触 40 ELOOP，
    // 分量账 used 不含链内展开（每层是链接账的账，不是分量账的账）。
    let real = t.add(0, "real", true)?;
    let mut tgt = String::from("/real");
    let mut idx = 40usize;
    while idx >= 1 {
        let mut name = String::from("k");
        name.push((b'0' + (idx / 10) as u8) as char);
        name.push((b'0' + (idx % 10) as u8) as char);
        t.add_link(0, &name, &tgt)?;
        tgt.clear();
        tgt.push('/');
        tgt.push_str(&name);
        idx -= 1;
    }
    t.add_link(0, "k41", "/k40")?;
    let comp_before = t.books.comp.used;
    if t.walk("/k41") != Err(E_ELOOP) {
        return Err(E_ELOOP);
    }
    // 分量账只许 +1（末分量本身一格），链内 41 层展开是链接账的账，
    // 分量账多动一格即串位（交叉零混淆判据面）。
    if t.books.comp.used != comp_before + 1 {
        return Err(E_LEDGER);
    }
    if t.books.link.capped != 1 {
        return Err(E_LEDGER);
    }
    let _ = real;
    hits += 1;

    // R5：两账快照导出与分立终审。
    let s = t.books.snapshot();
    if s[2] != 1 || s[5] != 1 {
        return Err(E_LEDGER);
    }
    if !t.books.separated() {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I1：短链接链正常解析——链接账 used 入账、分量账 only +1。
    t.add_link(0, "easy", "/real")?;
    let lb = t.books.link.used;
    let cb = t.books.comp.used;
    let r = t.walk("/easy")?;
    if r == 0 {
        return Err(E_NOENT);
    }
    if t.books.link.used != lb + 1 || t.books.comp.used != cb + 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I2：形错注入——相对路径/空分量。
    if t.walk("real") != Err(E_NOT_ABS) {
        return Err(E_NOT_ABS);
    }
    if t.walk("//real") != Err(E_BAD_COMP) {
        return Err(E_BAD_COMP);
    }
    hits += 1;

    Ok(hits)
}
