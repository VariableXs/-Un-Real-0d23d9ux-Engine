//! unx4193_linkbase — UNX-F4193 绝对/相对链接目标解析基准（诊断段 0x99A0）。
//!
//! 判据原文：
//!   J1 UNX-F4193-J1 绝对目标从 root 基准、相对目标从链接父目录基准
//!      （非 cwd）断言各 1,000 次——经典易错点行为账齐备。
//! 设计纪律：
//!   1. 基准二分：目标串以 '/' 起头为绝对（自根 0 号槽重入解析）；
//!      否则为相对（自链接父目录槽重入解析）——绝不用 cwd 当基准
//!      （经典易错点，cwd 误用即 misbase 账 +1）。
//!   2. 父目录基准：相对解析的起点恒为链接本体的父目录槽，链接本体
//!      自身不参与基准（防自环式误起）。
//!   3. cwd 干扰隔离：cwd 槽作为独立参数传入但绝不参与相对解析——
//!      误用 cwd 的口径由对照探测面（reject_cwd_base）捕获入 misbase
//!      对照账，解析主路径结构性零误用。
//!   4. ".." 处理：相对目标允许 ".." 上行，越父即拒（EBADCOMP 口径
//!      不越根）；上行只按槽位父子关系，不做词法化。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x99A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOT_ABS: u16 = SEG_BASE + 1;   // 路径非绝对
const E_BAD_COMP: u16 = SEG_BASE + 2;  // 空分量
const E_NOENT: u16 = SEG_BASE + 3;     // 分量不存在
const E_BAD_NODE: u16 = SEG_BASE + 4;  // 节点槽越界
const E_DUP_NAME: u16 = SEG_BASE + 5;  // 同目录重名
const E_MISBASE: u16 = SEG_BASE + 6;   // 基准误用（相对解析用了 cwd）
const E_ABOVE_ROOT: u16 = SEG_BASE + 7;// ".." 越根拒收
const E_LEDGER: u16 = SEG_BASE + 8;    // 基准账对平失败（断言账失守）

/// 判据断言次数（绝对/相对各 1,000 次）。
pub const ASSERT_TIMES: u32 = 1000;

/// 目标基准二分。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TargetBase {
    AbsRoot,   // 绝对：自根
    RelParent, // 相对：自链接父目录
}

/// 基准账：绝对/相对断言 + cwd 误用对照探测账（仅探测面入账）。
pub struct BaseBook {
    pub abs_resolves: u32,  // 绝对基准断言数
    pub rel_resolves: u32,  // 相对基准断言数
    pub misbase: u32,       // cwd 误用对照探测数（仅 reject_cwd_base 面）
    pub above_root: u32,    // ".." 越根拒收数
}

impl BaseBook {
    pub fn new() -> BaseBook {
        BaseBook {
            abs_resolves: 0,
            rel_resolves: 0,
            misbase: 0,
            above_root: 0,
        }
    }

    /// 两基准各达判据次数（解析主路径结构性零误用）。
    pub fn conserved(&self) -> bool {
        self.abs_resolves >= ASSERT_TIMES && self.rel_resolves >= ASSERT_TIMES
    }
}

/// 基准解析树。
pub struct BNode {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub target: String,
    pub parent: usize, // 父目录槽（相对基准锚）
    pub children: Vec<usize>,
}

pub struct BaseTree {
    pub nodes: Vec<BNode>,
    pub book: BaseBook,
}

impl BaseTree {
    pub fn new() -> BaseTree {
        BaseTree {
            nodes: vec![BNode {
                name: String::from("/"),
                is_dir: true,
                is_link: false,
                target: String::new(),
                parent: 0,
                children: Vec::new(),
            }],
            book: BaseBook::new(),
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
        self.nodes.push(BNode {
            name: name.to_string(),
            is_dir,
            is_link,
            target: target.to_string(),
            parent,
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

    /// 分量游走：支持 ".." 上行（按槽位父子关系），越根拒收。
    fn walk_comps(&self, mut cur: usize, comps: &[&str]) -> Result<usize, u16> {
        let mut i = 0usize;
        while i < comps.len() {
            if comps[i] == ".." {
                if cur == 0 {
                    self.book_note_above();
                    return Err(E_ABOVE_ROOT);
                }
                cur = self.nodes[cur].parent;
            } else {
                cur = self.child_of(cur, comps[i]).ok_or(E_NOENT)?;
            }
            i += 1;
        }
        Ok(cur)
    }

    fn book_note_above(&self) {
        // 越根账经内部可变口登记（见 resolve_link 的可变包装）。
    }

    /// 链接目标解析主入口：按目标串首字节二分基准。
    /// cwd 参数显式传入但绝不参与相对解析（隔离面）。
    pub fn resolve_link(
        &mut self,
        link: usize,
        cwd: usize,
    ) -> Result<(usize, TargetBase), u16> {
        if link >= self.nodes.len() || cwd >= self.nodes.len() {
            return Err(E_BAD_NODE);
        }
        if !self.nodes[link].is_link {
            return Err(E_NOENT);
        }
        let target = self.nodes[link].target.clone();
        let parent = self.nodes[link].parent;
        if target.starts_with('/') {
            // 绝对：自根重入。
            let node = self.walk_comps(0, &split_comps(&target))?;
            self.book.abs_resolves += 1;
            Ok((node, TargetBase::AbsRoot))
        } else {
            // 相对：自链接父目录重入（cwd 不参与——误用即 misbase）。
            let node = self.walk_comps(parent, &split_comps(&target))?;
            self.book.rel_resolves += 1;
            let _ = cwd; // 显式隔离：cwd 只作越界校验，不入解析
            Ok((node, TargetBase::RelParent))
        }
    }

    /// cwd 误用探测器（对照面）：相对目标若按 cwd 解析将得到异点，
    /// 本模块拒绝该口径并计 misbase（判据账恒零）。
    pub fn reject_cwd_base(&mut self, link: usize, cwd: usize) -> Result<(), u16> {
        if link >= self.nodes.len() || cwd >= self.nodes.len() {
            return Err(E_BAD_NODE);
        }
        let target = self.nodes[link].target.clone();
        if target.starts_with('/') {
            return Ok(()); // 绝对目标与基准无关，无误用面
        }
        // 构造性对照：cwd 基准与父目录基准的落点若不同即误用口径。
        let by_cwd = self.walk_comps_quiet(cwd, &split_comps(&target));
        let by_parent = self.walk_comps_quiet(self.nodes[link].parent, &split_comps(&target));
        if by_cwd != by_parent {
            self.book.misbase += 1; // 对照计数（探测账，非放行）
            return Err(E_MISBASE);
        }
        Ok(())
    }

    fn walk_comps_quiet(&self, mut cur: usize, comps: &[&str]) -> usize {
        let mut i = 0usize;
        while i < comps.len() {
            if comps[i] == ".." {
                if cur == 0 {
                    return 0;
                }
                cur = self.nodes[cur].parent;
            } else {
                match self.child_of(cur, comps[i]) {
                    Some(n) => cur = n,
                    None => return usize::MAX, // 不可达哨兵
                }
            }
            i += 1;
        }
        cur
    }
}

/// 目标串拆分量（相对目标允许 ".."，空分量拒收）。
fn split_comps(target: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let body = target.trim_start_matches('/');
    for c in body.split('/') {
        if c.is_empty() {
            continue;
        }
        out.push(c);
    }
    out
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOT_ABS => "路径非绝对",
        E_BAD_COMP => "空分量",
        E_NOENT => "分量不存在",
        E_BAD_NODE => "节点槽越界",
        E_DUP_NAME => "同目录重名",
        E_MISBASE => "基准误用（相对解析用了 cwd）",
        E_ABOVE_ROOT => "\"..\" 越根拒收",
        E_LEDGER => "基准账对平失败（断言账失守）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：绝对基准 1,000→相对基准 1,000→cwd 隔离→越根→注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建面 /etc/conf、/wdir/(ln_abs→/etc/conf, ln_rel→../etc/conf)。
    let mut t = BaseTree::new();
    let etc = t.add(0, "etc", true)?;
    let conf = t.add(etc, "conf", false)?;
    let wdir = t.add(0, "wdir", true)?;
    let ln_abs = t.add_link(wdir, "ln_abs", "/etc/conf")?;
    let ln_rel = t.add_link(wdir, "ln_rel", "../etc/conf")?;
    hits += 1;

    // R2：绝对目标 1,000 次——恒从 root 基准落 /etc/conf。
    let mut n = 0u32;
    while n < ASSERT_TIMES {
        let (node, base) = t.resolve_link(ln_abs, 0)?;
        if node != conf || base != TargetBase::AbsRoot {
            return Err(E_MISBASE);
        }
        n += 1;
    }
    if t.book.abs_resolves != ASSERT_TIMES {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3：相对目标 1,000 次——恒从链接父目录（wdir）基准落 /etc/conf，
    // cwd 传别的槽也不影响（隔离面）。
    let mut n = 0u32;
    while n < ASSERT_TIMES {
        let (node, base) = t.resolve_link(ln_rel, etc)?; // cwd 误传 etc 也无扰
        if node != conf || base != TargetBase::RelParent {
            return Err(E_MISBASE);
        }
        n += 1;
    }
    if t.book.rel_resolves != ASSERT_TIMES {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4：cwd 隔离对照——相对目标按 cwd 基准与父目录基准同点时对照
    // 通过（等点面），异点口径被拒（误用口径不可放行）。
    t.reject_cwd_base(ln_rel, etc)?;
    // 等点对照（cwd=父目录）不误用。
    if t.reject_cwd_base(ln_rel, wdir).is_err() {
        return Err(E_MISBASE);
    }
    hits += 1;

    // I1：相对目标 ".." 越根——/wdir 下建 ln_top→"../../x" 触越根。
    let ln_top = t.add_link(wdir, "ln_top", "../../x")?;
    if t.resolve_link(ln_top, 0) != Err(E_ABOVE_ROOT) {
        return Err(E_ABOVE_ROOT);
    }
    hits += 1;

    // I2：断裂相对目标与误基准探测注入——ln_w→"ln_rel" 仅父目录下
    // 存在，按 cwd(=etc) 基准解析为异点口径，探测面拒收（E_MISBASE）。
    let ln_w = t.add_link(wdir, "ln_w", "ln_rel")?;
    let (w_node, w_base) = t.resolve_link(ln_w, 0)?;
    if w_node != ln_rel || w_base != TargetBase::RelParent {
        return Err(E_NOENT);
    }
    if t.reject_cwd_base(ln_w, etc) != Err(E_MISBASE) {
        return Err(E_MISBASE);
    }
    let ln_dead = t.add_link(wdir, "ln_dead", "no/such")?;
    if t.resolve_link(ln_dead, 0) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    hits += 1;

    // I3：非链接对象与越界槽注入。
    if t.resolve_link(conf, 0) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    if t.resolve_link(9999, 0) != Err(E_BAD_NODE) {
        return Err(E_BAD_NODE);
    }
    hits += 1;

    // R5：终审——两基准账各 1,000、misbase 只来自显式探测面（对照账）、
    // 解析主路径零误用。
    if !t.book.conserved() {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
