//! unx4188_atnofollow — UNX-F4188 AT_SYMLINK_NOFOLLOW 全 *at 面传播（诊断段 0x9900）。
//!
//! 判据原文：
//!   J1 UNX-F4188-J1 标志在 stat/fchmodat/fchownat/unlinkat 四族 *at 接口
//!      传播语义逐族断言（8 格矩阵全过），哨兵值 AT_SYMLINK_FOLLOW 特例
//!      行为账齐。
//! 设计纪律：
//!   1. Linux fcntl.h 口径常量：AT_SYMLINK_NOFOLLOW=0x0100、
//!      AT_SYMLINK_FOLLOW=0x0400（哨兵值，仅 fstatat 族有实效语义）。
//!   2. 四族闭集：fstatat、fchmodat、fchownat、unlinkat × {0, NOFOLLOW}
//!      共 8 格，每格绑定传播语义（0=跟随，NOFOLLOW=不跟随）。
//!   3. 传播规则：NOFOLLOW 只对末分量生效（中间分量恒跟随，POSIX/VFS
//!      口径）；未知标志位一律 EINVAL 拒收，不静默吞位。
//!   4. 哨兵特例：AT_SYMLINK_FOLLOW 传入 fstatat 时为跟随（覆盖扩展），
//!      传入其余三族为非法位组合（哨兵账分记）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9900;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_INVAL: u16 = SEG_BASE + 1;     // 未知标志位（EINVAL 口径）
const E_NOT_ABS: u16 = SEG_BASE + 2;   // 路径非绝对
const E_BAD_COMP: u16 = SEG_BASE + 3;  // 空分量
const E_NOENT: u16 = SEG_BASE + 4;     // 分量不存在
const E_BAD_NODE: u16 = SEG_BASE + 5;  // 节点槽越界
const E_DUP_NAME: u16 = SEG_BASE + 6;  // 同目录重名
const E_MATRIX: u16 = SEG_BASE + 7;    // 八格矩阵语义不符
const E_SENTINEL: u16 = SEG_BASE + 8;  // 哨兵值误用（非 fstatat 族）
const E_LEDGER: u16 = SEG_BASE + 9;    // 传播账对平失败（矩阵/哨兵账失守）

/// Linux fcntl.h 口径：AT_SYMLINK_NOFOLLOW。
pub const AT_SYMLINK_NOFOLLOW: u16 = 0x0100;
/// Linux fcntl.h 口径：AT_SYMLINK_FOLLOW（哨兵值）。
pub const AT_SYMLINK_FOLLOW: u16 = 0x0400;

/// *at 四族接口面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtFamily {
    FstatAt,  // fstatat —— 支持哨兵 FOLLOW 扩展
    FchmodAt, // fchmodat —— 哨兵位非法
    FchownAt, // fchownat —— 哨兵位非法
    UnlinkAt, // unlinkat —— 哨兵位非法
}

/// 传播结果二值（跟随侧语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtOutcome {
    Followed(usize), // 跟随解析到真身
    Body(usize),     // 不跟随停在链接本体
}

/// 传播账：八格 run/mismatch + 哨兵账。
pub struct AtBook {
    pub runs: [u32; 8],      // 八格执行数
    pub mismatch: [u32; 8],  // 八格语义不符数（恒零）
    pub sentinel_notes: u32, // 哨兵特例行为账
    pub inval_rejects: u32,  // 未知标志拒收数（错误账）
}

impl AtBook {
    pub fn new() -> AtBook {
        AtBook {
            runs: [0; 8],
            mismatch: [0; 8],
            sentinel_notes: 0,
            inval_rejects: 0,
        }
    }

    /// 八格全过判：每格至少一次且 mismatch 全零。
    pub fn all_pass(&self) -> bool {
        let mut i = 0usize;
        while i < 8 {
            if self.runs[i] == 0 || self.mismatch[i] != 0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 解析树（链接载体）。
pub struct ANode {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub target: String,
    pub children: Vec<usize>,
}

pub struct AtTree {
    pub nodes: Vec<ANode>,
}

impl AtTree {
    pub fn new() -> AtTree {
        AtTree {
            nodes: vec![ANode {
                name: String::from("/"),
                is_dir: true,
                is_link: false,
                target: String::new(),
                children: Vec::new(),
            }],
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
        self.nodes.push(ANode {
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

    fn follow_chain(&self, mut node: usize) -> Result<usize, u16> {
        let mut layers = 0u32;
        while self.nodes[node].is_link {
            layers += 1;
            if layers > 8 {
                return Err(E_NOENT); // 上限守门（正式口径归 F4186）
            }
            node = self.resolve_abs(&self.nodes[node].target.clone())?;
        }
        Ok(node)
    }

    /// *at 传播核心：flags→末分量语义，NOFOLLOW 只裁决末分量。
    pub fn at_resolve(
        &self,
        fam: AtFamily,
        flags: u16,
        path: &str,
        book: &mut AtBook,
    ) -> Result<AtOutcome, u16> {
        // 标志位合法性：四族只认 0 与 NOFOLLOW；fstatat 额外认哨兵 FOLLOW。
        let legal = match flags {
            0 => true,
            AT_SYMLINK_NOFOLLOW => true,
            AT_SYMLINK_FOLLOW => fam == AtFamily::FstatAt,
            _ => false,
        };
        if !legal {
            book.inval_rejects += 1;
            return Err(E_INVAL);
        }
        if flags == AT_SYMLINK_FOLLOW {
            book.sentinel_notes += 1; // 哨兵特例行为账（仅 fstatat 到此）
        }
        let comps = self.split(path)?;
        if comps.is_empty() {
            let slot = cell_of(fam, flags);
            book.runs[slot] += 1;
            return Ok(AtOutcome::Followed(0));
        }
        let mut cur = 0usize;
        let n = comps.len();
        let mut i = 0usize;
        while i + 1 < n {
            // 中间分量恒跟随（传播只作用于末分量）。
            let next = self.child_of(cur, comps[i]).ok_or(E_NOENT)?;
            cur = self.follow_chain(next)?;
            i += 1;
        }
        let last = self.child_of(cur, comps[n - 1]).ok_or(E_NOENT)?;
        let slot = cell_of(fam, flags);
        book.runs[slot] += 1;
        if flags == AT_SYMLINK_NOFOLLOW {
            if self.nodes[last].is_link {
                return Ok(AtOutcome::Body(last));
            }
            return Ok(AtOutcome::Followed(last)); // 非链接本体即真身
        }
        let real = self.follow_chain(last)?;
        if self.nodes[real].is_link {
            book.mismatch[slot] += 1;
            return Err(E_MATRIX); // 跟随侧落点不得仍是链接
        }
        Ok(AtOutcome::Followed(real))
    }
}

/// 八格槽位：族 × {0, NOFOLLOW}。
fn cell_of(fam: AtFamily, flags: u16) -> usize {
    let base = match fam {
        AtFamily::FstatAt => 0usize,
        AtFamily::FchmodAt => 2usize,
        AtFamily::FchownAt => 4usize,
        AtFamily::UnlinkAt => 6usize,
    };
    if flags == AT_SYMLINK_NOFOLLOW {
        base + 1
    } else {
        base
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_INVAL => "未知标志位（EINVAL 口径）",
        E_NOT_ABS => "路径非绝对",
        E_BAD_COMP => "空分量",
        E_NOENT => "分量不存在",
        E_BAD_NODE => "节点槽越界",
        E_DUP_NAME => "同目录重名",
        E_MATRIX => "八格矩阵语义不符",
        E_SENTINEL => "哨兵值误用（非 fstatat 族）",
        E_LEDGER => "传播账对平失败（矩阵/哨兵账失守）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：八格矩阵全过→哨兵特例→EINVAL 注入→传播边界注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建树 /d/(ln→/d/f, f)。
    let mut t = AtTree::new();
    let d = t.add(0, "d", true)?;
    let f = t.add(d, "f", false)?;
    let ln = t.add_link(d, "ln", "/d/f")?;
    let mut book = AtBook::new();
    hits += 1;

    // R2：八格矩阵——四族 × {0, NOFOLLOW} 对链接末分量全跑。
    let fams = [AtFamily::FstatAt, AtFamily::FchmodAt, AtFamily::FchownAt, AtFamily::UnlinkAt];
    let mut i = 0usize;
    while i < fams.len() {
        // flags=0：跟随 → 真身 f。
        match t.at_resolve(fams[i], 0, "/d/ln", &mut book)? {
            AtOutcome::Followed(n) => {
                if n != f {
                    return Err(E_MATRIX);
                }
            }
            AtOutcome::Body(_) => return Err(E_MATRIX),
        }
        // NOFOLLOW：停在链接本体。
        match t.at_resolve(fams[i], AT_SYMLINK_NOFOLLOW, "/d/ln", &mut book)? {
            AtOutcome::Body(n) => {
                if n != ln {
                    return Err(E_MATRIX);
                }
            }
            AtOutcome::Followed(_) => return Err(E_MATRIX),
        }
        i += 1;
    }
    if !book.all_pass() {
        return Err(E_MATRIX);
    }
    hits += 1;

    // R3：哨兵特例——fstatat + AT_SYMLINK_FOLLOW 跟随生效；其余三族
    // 哨兵位非法拒收（哨兵账只记 fstatat 一格）。
    match t.at_resolve(AtFamily::FstatAt, AT_SYMLINK_FOLLOW, "/d/ln", &mut book)? {
        AtOutcome::Followed(n) => {
            if n != f {
                return Err(E_MATRIX);
            }
        }
        AtOutcome::Body(_) => return Err(E_MATRIX),
    }
    if book.sentinel_notes != 1 {
        return Err(E_SENTINEL);
    }
    let mut i = 1usize;
    while i < fams.len() {
        if t.at_resolve(fams[i], AT_SYMLINK_FOLLOW, "/d/ln", &mut book) != Err(E_INVAL) {
            return Err(E_SENTINEL);
        }
        i += 1;
    }
    hits += 1;

    // I1：未知标志位注入——0x0200、混合位 0x0300 双拒。
    if t.at_resolve(AtFamily::FstatAt, 0x0200, "/d/ln", &mut book) != Err(E_INVAL) {
        return Err(E_INVAL);
    }
    if t.at_resolve(AtFamily::UnlinkAt, AT_SYMLINK_NOFOLLOW | 0x0200, "/d/ln", &mut book)
        != Err(E_INVAL)
    {
        return Err(E_INVAL);
    }
    if book.inval_rejects != 5 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I2：传播边界——中间分量不受理 NOFOLLOW（恒跟随）。
    t.add_link(0, "up", "/d")?; // 中间位链接 up→/d
    match t.at_resolve(AtFamily::FstatAt, AT_SYMLINK_NOFOLLOW, "/up/ln", &mut book)? {
        AtOutcome::Body(n) => {
            if n != ln {
                return Err(E_MATRIX);
            }
        }
        AtOutcome::Followed(_) => return Err(E_MATRIX),
    }
    // /up/f：中间 up 被强制跟随，末分量 f 非链接 → Followed(f)。
    match t.at_resolve(AtFamily::FstatAt, AT_SYMLINK_NOFOLLOW, "/up/f", &mut book)? {
        AtOutcome::Followed(n) => {
            if n != f {
                return Err(E_MATRIX);
            }
        }
        AtOutcome::Body(_) => return Err(E_MATRIX),
    }
    hits += 1;

    // I3：不存在分量 / 相对路径注入。
    if t.at_resolve(AtFamily::FstatAt, 0, "/d/none", &mut book) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    if t.at_resolve(AtFamily::FstatAt, 0, "d/ln", &mut book) != Err(E_NOT_ABS) {
        return Err(E_NOT_ABS);
    }
    hits += 1;

    // R4：终审——八格 runs 各 ≥1、mismatch 全零、哨兵账与拒收账对平。
    if !book.all_pass() || book.sentinel_notes != 1 || book.inval_rejects != 5 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
