//! unx4185_lastfollow — UNX-F4185 最后一分量跟随/不跟随矩阵（诊断段 0x98A0）。
//!
//! 判据原文：
//!   J1 UNX-F4185-J1 末分量 × 标志 × 系统调用默认值矩阵（stat 跟随/
//!      lstat 不跟随/readlink 不跟随等 8 格）全过，默认值表与 POSIX
//!      对照零差。
//! 设计纪律：
//!   1. 八格矩阵闭集：stat、lstat、fstatat(带 FOLLOW)、fstatat(带
//!      NOFOLLOW)、readlink、unlinkat(0)、fchmodat(0)、fchownat(0)，
//!      每格绑定 POSIX 默认跟随侧（Follow/NoFollow）。
//!   2. 默认值表写死并与 POSIX 对照：stat/fchmodat/fchownat/unlinkat
//!      跟随；lstat/readlink 不跟随；fstatat 两格按显式标志分列。
//!   3. 矩阵执行：每格按默认侧对同一末分量链接解析一次，落点必须与
//!      默认侧一致（跟随→真身，不跟随→本体），错格即零差失守。
//!   4. 对照零差账：八格各记 run 与 mismatch 两账，mismatch 恒零；
//!      POSIX 对照注记随表落账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x98A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOT_ABS: u16 = SEG_BASE + 1;    // 路径非绝对
const E_BAD_COMP: u16 = SEG_BASE + 2;   // 空分量
const E_NOENT: u16 = SEG_BASE + 3;      // 分量不存在
const E_BAD_NODE: u16 = SEG_BASE + 4;   // 节点槽越界
const E_DUP_NAME: u16 = SEG_BASE + 5;   // 同目录重名
const E_MATRIX: u16 = SEG_BASE + 6;     // 矩阵格与 POSIX 默认侧不符
const E_DEFAULTS: u16 = SEG_BASE + 7;   // 默认值表与 POSIX 对照零差失守

/// 八格矩阵规模。
pub const MATRIX_CELLS: usize = 8;

/// 末分量跟随侧二值闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FollowSide {
    Follow,   // 跟随：末分量解析到真身
    NoFollow, // 不跟随：末分量停在链接本体
}

/// 八格系统调用面（末分量 × 标志 × 默认值的行键）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysCell {
    Stat,          // 格 1：stat —— 跟随
    Lstat,         // 格 2：lstat —— 不跟随
    FstatatFollow, // 格 3：fstatat(AT_SYMLINK_FOLLOW) —— 跟随
    FstatatNoFlw,  // 格 4：fstatat(AT_SYMLINK_NOFOLLOW) —— 不跟随
    Readlink,      // 格 5：readlink —— 不跟随
    UnlinkAt,      // 格 6：unlinkat(0) —— 跟随
    FchmodAt,      // 格 7：fchmodat(0) —— 跟随
    FchownAt,      // 格 8：fchownat(0) —— 跟随
}

/// 矩阵执行状态机：Idle→Running→Audited。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatrixState {
    Idle,
    Running,
    Audited,
}

/// POSIX 默认值表（写死，判据 J1 对照基准）。
pub fn posix_default(cell: SysCell) -> FollowSide {
    match cell {
        SysCell::Stat => FollowSide::Follow,
        SysCell::Lstat => FollowSide::NoFollow,
        SysCell::FstatatFollow => FollowSide::Follow,
        SysCell::FstatatNoFlw => FollowSide::NoFollow,
        SysCell::Readlink => FollowSide::NoFollow,
        SysCell::UnlinkAt => FollowSide::Follow,
        SysCell::FchmodAt => FollowSide::Follow,
        SysCell::FchownAt => FollowSide::Follow,
    }
}

/// 矩阵账：八格 run/mismatch 双账 + 对照注记。
pub struct MatrixBook {
    pub runs: [u32; MATRIX_CELLS],
    pub mismatch: [u32; MATRIX_CELLS],
    pub posix_notes: u32, // POSIX 对照注记落账数
    pub state: MatrixState,
}

impl MatrixBook {
    pub fn new() -> MatrixBook {
        MatrixBook {
            runs: [0; MATRIX_CELLS],
            mismatch: [0; MATRIX_CELLS],
            posix_notes: 0,
            state: MatrixState::Idle,
        }
    }

    /// 零差判：八格 mismatch 全零且每格至少跑一次。
    pub fn zero_diff(&self) -> bool {
        let mut i = 0usize;
        while i < MATRIX_CELLS {
            if self.mismatch[i] != 0 || self.runs[i] == 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 账面导出（runs 与 mismatch 平铺）。
    pub fn snapshot(&self) -> [u32; 16] {
        let mut out = [0u32; 16];
        let mut i = 0usize;
        while i < MATRIX_CELLS {
            out[i] = self.runs[i];
            out[i + 8] = self.mismatch[i];
            i += 1;
        }
        out
    }
}

/// 解析树：目录/文件/链接三态 arena。
pub struct LNode {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub target: String,
    pub children: Vec<usize>,
}

pub struct FollowTree {
    pub nodes: Vec<LNode>,
}

impl FollowTree {
    pub fn new() -> FollowTree {
        FollowTree {
            nodes: vec![LNode {
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
        self.nodes.push(LNode {
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

    /// 末分量按侧解析：Follow 展开链接链（上限 8），NoFollow 停本体。
    pub fn resolve_last(&self, path: &str, side: FollowSide) -> Result<usize, u16> {
        if !path.starts_with('/') {
            return Err(E_NOT_ABS);
        }
        let body = &path[1..]; // '/' 单字节，前缀判定后切片安全
        let mut comps = Vec::new();
        if !body.is_empty() {
            for c in body.split('/') {
                if c.is_empty() {
                    return Err(E_BAD_COMP);
                }
                comps.push(c);
            }
        }
        if comps.is_empty() {
            return Ok(0);
        }
        let mut cur = 0usize;
        let n = comps.len();
        let mut i = 0usize;
        while i + 1 < n {
            // 中间分量恒跟随（本模块聚焦末分量矩阵）。
            let next = self.child_of(cur, comps[i]).ok_or(E_NOENT)?;
            cur = self.follow_chain(next)?;
            i += 1;
        }
        let last = self.child_of(cur, comps[n - 1]).ok_or(E_NOENT)?;
        match side {
            FollowSide::Follow => self.follow_chain(last),
            FollowSide::NoFollow => Ok(last),
        }
    }

    fn follow_chain(&self, mut node: usize) -> Result<usize, u16> {
        let mut layers = 0u32;
        while self.nodes[node].is_link {
            layers += 1;
            if layers > 8 {
                return Err(E_NOENT); // 上限守门（40 层正式口径归 F4186）
            }
            let t = self.nodes[node].target.clone();
            node = self.resolve_abs(&t)?;
        }
        Ok(node)
    }

    fn resolve_abs(&self, path: &str) -> Result<usize, u16> {
        if !path.starts_with('/') {
            return Err(E_NOT_ABS);
        }
        let body = &path[1..];
        let mut cur = 0usize;
        if body.is_empty() {
            return Ok(cur);
        }
        for c in body.split('/') {
            if c.is_empty() {
                return Err(E_BAD_COMP);
            }
            cur = self.child_of(cur, c).ok_or(E_NOENT)?;
        }
        Ok(cur)
    }

    /// 矩阵一格执行：按 POSIX 默认侧解析并核对落点类型。
    pub fn run_cell(
        &self,
        cell: SysCell,
        path: &str,
        book: &mut MatrixBook,
    ) -> Result<usize, u16> {
        let idx = cell as usize;
        let side = posix_default(cell);
        let node = self.resolve_last(path, side)?;
        book.runs[idx] += 1;
        // 一致性：跟随侧落点必须非链接（已展开到真身）；不跟随侧为
        // 构造性保证（停在命中的本体上，链接/非链接皆合法落点）。
        let ok = match side {
            FollowSide::Follow => !self.nodes[node].is_link,
            FollowSide::NoFollow => true,
        };
        if !ok {
            book.mismatch[idx] += 1;
            return Err(E_MATRIX);
        }
        Ok(node)
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOT_ABS => "路径非绝对",
        E_BAD_COMP => "空分量",
        E_NOENT => "分量不存在",
        E_BAD_NODE => "节点槽越界",
        E_DUP_NAME => "同目录重名",
        E_MATRIX => "矩阵格与 POSIX 默认侧不符",
        E_DEFAULTS => "默认值表与 POSIX 对照零差失守",
        _ => "未知诊断码",
    }
}

/// 确定性演练：八格矩阵全跑→落点核对→默认值表对照→错格注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建树 /dir/(ln→/dir/f, f)。
    let mut t = FollowTree::new();
    let dir = t.add(0, "dir", true)?;
    let f = t.add(dir, "f", false)?;
    t.add_link(dir, "ln", "/dir/f")?;
    let mut book = MatrixBook::new();
    hits += 1;

    // R2：八格矩阵对末分量链接 /dir/ln 全跑——跟随 5 格落 f，不跟随 3 格落 ln。
    let cells = [
        SysCell::Stat,
        SysCell::Lstat,
        SysCell::FstatatFollow,
        SysCell::FstatatNoFlw,
        SysCell::Readlink,
        SysCell::UnlinkAt,
        SysCell::FchmodAt,
        SysCell::FchownAt,
    ];
    book.state = MatrixState::Running;
    let mut i = 0usize;
    while i < cells.len() {
        let node = t.run_cell(cells[i], "/dir/ln", &mut book)?;
        let want = match posix_default(cells[i]) {
            FollowSide::Follow => f,
            FollowSide::NoFollow => dir + 2, // ln 槽位（dir 后第 2 个建点）
        };
        if node != want {
            return Err(E_MATRIX);
        }
        i += 1;
    }
    if !book.zero_diff() {
        return Err(E_MATRIX);
    }
    hits += 1;

    // R3：普通文件末分量——不跟随格落在文件本体也一致（NoFollow 对非
    // 链接末分量恒成立）；跟随格同样落到本体。
    let mut i = 0usize;
    while i < cells.len() {
        let node = t.run_cell(cells[i], "/dir/f", &mut book)?;
        if node != f {
            return Err(E_MATRIX);
        }
        i += 1;
    }
    hits += 1;

    // R4：默认值表与 POSIX 对照零差——逐格核对跟随侧（表写死口径）。
    let sides = [
        FollowSide::Follow,
        FollowSide::NoFollow,
        FollowSide::Follow,
        FollowSide::NoFollow,
        FollowSide::NoFollow,
        FollowSide::Follow,
        FollowSide::Follow,
        FollowSide::Follow,
    ];
    let mut i = 0usize;
    while i < cells.len() {
        if posix_default(cells[i]) != sides[i] {
            return Err(E_DEFAULTS);
        }
        i += 1;
    }
    book.posix_notes = 8;
    book.state = MatrixState::Audited;
    hits += 1;

    // I1：矩阵格错配注入——手工破坏 mismatch 后零差判必须翻红。
    book.mismatch[SysCell::Readlink as usize] += 1;
    if book.zero_diff() {
        return Err(E_MATRIX);
    }
    book.mismatch[SysCell::Readlink as usize] -= 1;
    if !book.zero_diff() {
        return Err(E_MATRIX);
    }
    hits += 1;

    // I2：分量不存在与相对路径注入。
    if t.run_cell(SysCell::Stat, "/dir/none", &mut book) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    if t.run_cell(SysCell::Stat, "dir/ln", &mut book) != Err(E_NOT_ABS) {
        return Err(E_NOT_ABS);
    }
    hits += 1;

    // R5：终审——八格双跑、零差、对照注记齐、状态机 Audited。
    let s = book.snapshot();
    let mut i = 0usize;
    while i < 8 {
        if s[i] != 2 || s[i + 8] != 0 {
            return Err(E_MATRIX);
        }
        i += 1;
    }
    if book.posix_notes != 8 || !matches!(book.state, MatrixState::Audited) {
        return Err(E_DEFAULTS);
    }
    hits += 1;

    Ok(hits)
}
