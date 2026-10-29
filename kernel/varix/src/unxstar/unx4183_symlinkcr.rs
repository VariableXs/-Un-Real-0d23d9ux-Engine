//! unx4183_symlinkcr — UNX-F4183 symlink 创建与 O_EXCL 语义锚（诊断段 0x9860）。
//!
//! 判据原文：
//!   J1 UNX-F4183-J1 symlink 创建对已存在目标名返回 EEXIST（10/10），链接
//!      本体类型位 S_IFLNK 落账，悬空目标允许创建（POSIX 口径）。
//! 设计纪律：
//!   1. EEXIST 精确边界：symlink(newpath, ...) 的 newpath 与任何既有条目
//!      （普通文件/目录/链接）同名即 EEXIST，与新目标是否悬空无关。
//!   2. 类型位落账：链接本体模式位恒 S_IFLNK=0xA000（Linux 模式位口径），
//!      每次创建成功入账一次，账面与创建数守恒。
//!   3. 悬空允许：目标串不做存在性校验（POSIX：symlink 不解析 oldpath），
//!      悬空创建计数独立入账。
//!   4. O_EXCL 语义锚：O_EXCL|O_CREAT 对链接名 open 不跟随链接本体，
//!      命中既有链接名同样 EEXIST（锚定注记落账）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9860;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_EEXIST: u16 = SEG_BASE + 1;   // 目标名已存在（判据 EEXIST 口径）
const E_EMPTY: u16 = SEG_BASE + 2;    // 空名/空目标串拒收
const E_BAD_NAME: u16 = SEG_BASE + 3; // 名含分隔符（单层命名空间口径）
const E_NOENT: u16 = SEG_BASE + 4;    // 查找的条目不存在
const E_NOTLNK: u16 = SEG_BASE + 5;   // 类型位失守（S_IFLNK 未落账）
const E_LEDGER: u16 = SEG_BASE + 6;   // 创建账与类型位账不平

/// Linux 模式位口径：符号链接类型位。
pub const S_IFLNK: u16 = 0xA000;
/// O_EXCL|O_CREAT 语义锚标志（Linux fcntl 口径）。
pub const O_EXCL: u16 = 0x0080;
/// 判据注入次数（EEXIST 10/10）。
pub const INJECT_TIMES: u32 = 10;

/// 命名空间条目种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NsKind {
    Reg,     // 普通文件
    Dir,     // 目录
    Symlink, // 符号链接
}

/// 条目状态机：New→Live→(重建注入)Rejected。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntState {
    New,
    Live,
}

/// 命名空间条目：名 + 种类 + 模式位 + 目标串 + 状态。
pub struct NsEnt {
    pub name: String,   // 条目名（单层命名空间）
    pub kind: NsKind,
    pub mode_bits: u16, // 模式位（链接本体恒含 S_IFLNK）
    pub target: String, // 链接目标串（悬空允许），非链接恒空
    pub state: EntState,
}

/// 单层命名空间：创建账 + EEXIST 账 + 类型位账 + 悬空账。
pub struct NameSpace {
    pub ents: Vec<NsEnt>,
    pub eexist_hits: u32,  // EEXIST 命中数（判据 10/10 账）
    pub iflnk_set: u32,    // S_IFLNK 落账数
    pub dangling_ok: u32,  // 悬空目标创建成功数
    pub oexcl_notes: u32,  // O_EXCL 语义锚注记数
}

impl NameSpace {
    pub fn new() -> NameSpace {
        NameSpace {
            ents: Vec::new(),
            eexist_hits: 0,
            iflnk_set: 0,
            dangling_ok: 0,
            oexcl_notes: 0,
        }
    }

    fn index_of(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ents.len() {
            if self.ents[i].name == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 建 symlink：同名（任何种类）→ EEXIST；悬空目标允许创建。
    pub fn symlink(&mut self, name: &str, target: &str) -> Result<(), u16> {
        if name.is_empty() || target.is_empty() {
            return Err(E_EMPTY);
        }
        if name.contains('/') {
            return Err(E_BAD_NAME);
        }
        match self.index_of(name) {
            Some(_) => {
                self.eexist_hits += 1; // EEXIST 精确边界：名在场即拒
                Err(E_EEXIST)
            }
            None => {
                self.ents.push(NsEnt {
                    name: name.to_string(),
                    kind: NsKind::Symlink,
                    mode_bits: S_IFLNK, // 类型位落账
                    target: target.to_string(),
                    state: EntState::Live,
                });
                self.iflnk_set += 1;
                Ok(())
            }
        }
    }

    /// 建普通文件（供 EEXIST 注入面）。
    pub fn create_reg(&mut self, name: &str) -> Result<(), u16> {
        if name.is_empty() {
            return Err(E_EMPTY);
        }
        if name.contains('/') {
            return Err(E_BAD_NAME);
        }
        match self.index_of(name) {
            Some(_) => {
                self.eexist_hits += 1;
                Err(E_EEXIST)
            }
            None => {
                self.ents.push(NsEnt {
                    name: name.to_string(),
                    kind: NsKind::Reg,
                    mode_bits: 0x8000, // S_IFREG 口径
                    target: String::new(),
                    state: EntState::Live,
                });
                Ok(())
            }
        }
    }

    /// 建 目录（供 EEXIST 注入面）。
    pub fn create_dir(&mut self, name: &str) -> Result<(), u16> {
        if name.is_empty() {
            return Err(E_EMPTY);
        }
        if name.contains('/') {
            return Err(E_BAD_NAME);
        }
        match self.index_of(name) {
            Some(_) => {
                self.eexist_hits += 1;
                Err(E_EEXIST)
            }
            None => {
                self.ents.push(NsEnt {
                    name: name.to_string(),
                    kind: NsKind::Dir,
                    mode_bits: 0x4000, // S_IFDIR 口径
                    target: String::new(),
                    state: EntState::Live,
                });
                Ok(())
            }
        }
    }

    /// O_EXCL 语义锚：对链接名 open(O_EXCL|O_CREAT) 不跟随本体，同名即拒。
    pub fn open_excl(&mut self, name: &str) -> Result<(), u16> {
        self.oexcl_notes += 1;
        match self.index_of(name) {
            Some(i) => {
                let _ = i;
                self.eexist_hits += 1;
                Err(E_EEXIST)
            }
            None => Err(E_NOENT),
        }
    }

    /// 悬空创建：目标串在命名空间无落点仍成功（悬空账 +1）。
    pub fn symlink_dangling(&mut self, name: &str, target: &str) -> Result<(), u16> {
        // 先按 symlink 口径创建成功，再核目标确无落点（悬空成立）。
        let existed = self.index_of(target).is_some();
        self.symlink(name, target)?;
        if !existed {
            self.dangling_ok += 1;
        }
        Ok(())
    }

    /// 类型位复核：链接条目模式位必含 S_IFLNK（判据落账终审）。
    pub fn type_bits_clean(&self) -> bool {
        let mut links = 0u32;
        for e in &self.ents {
            if e.kind == NsKind::Symlink {
                links += 1;
                if e.mode_bits & S_IFLNK != S_IFLNK {
                    return false;
                }
            }
        }
        links == self.iflnk_set
    }

    /// 按名查种类。
    pub fn kind_of(&self, name: &str) -> Result<NsKind, u16> {
        match self.index_of(name) {
            Some(i) => Ok(self.ents[i].kind),
            None => Err(E_NOENT),
        }
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_EEXIST => "目标名已存在（判据 EEXIST 口径）",
        E_EMPTY => "空名/空目标串拒收",
        E_BAD_NAME => "名含分隔符（单层命名空间口径）",
        E_NOENT => "查找的条目不存在",
        E_NOTLNK => "类型位失守（S_IFLNK 未落账）",
        E_LEDGER => "创建账与类型位账不平",
        _ => "未知诊断码",
    }
}

/// 确定性演练：建面→EEXIST 10/10→类型位落账→悬空创建→O_EXCL 锚→注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut ns = NameSpace::new();

    // R1：铺底——普通文件 f、目录 d、链接 l0。
    ns.create_reg("f")?;
    ns.create_dir("d")?;
    ns.symlink("l0", "/f")?;
    if ns.kind_of("l0") != Ok(NsKind::Symlink) {
        return Err(E_NOTLNK);
    }
    hits += 1;

    // R2：EEXIST 10/10——对既有名 f 连续 10 次 symlink 创建全拒。
    let mut n = 0u32;
    while n < INJECT_TIMES {
        if ns.symlink("f", "/anything") != Err(E_EEXIST) {
            return Err(E_EEXIST);
        }
        n += 1;
    }
    if ns.eexist_hits != INJECT_TIMES {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3：EEXIST 跨种类——目录名与链接名同样拒（10/10 之外的边界面）。
    if ns.symlink("d", "/x") != Err(E_EEXIST) {
        return Err(E_EEXIST);
    }
    if ns.symlink("l0", "/x") != Err(E_EEXIST) {
        return Err(E_EEXIST);
    }
    if ns.create_reg("f") != Err(E_EEXIST) {
        return Err(E_EEXIST);
    }
    hits += 1;

    // R4：类型位落账——新增两条链接，S_IFLNK 账与条目账守恒。
    ns.symlink("l1", "/f")?;
    ns.symlink("l2", "/nowhere")?;
    if ns.iflnk_set != 3 || !ns.type_bits_clean() {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5：悬空目标允许创建（POSIX 口径）——目标 /nowhere 无落点仍成。
    ns.symlink_dangling("l3", "/no/such/target")?;
    if ns.dangling_ok != 1 {
        return Err(E_LEDGER);
    }
    if ns.kind_of("l3") != Ok(NsKind::Symlink) {
        return Err(E_NOTLNK);
    }
    hits += 1;

    // R6：O_EXCL 语义锚——对链接名 l1 open(O_EXCL|O_CREAT) 命中 EEXIST。
    if ns.open_excl("l1") != Err(E_EEXIST) {
        return Err(E_EEXIST);
    }
    if ns.oexcl_notes != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I1：空名/空目标/含分隔符名三连注入。
    if ns.symlink("", "/x") != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    if ns.symlink("x", "") != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    if ns.symlink("a/b", "/x") != Err(E_BAD_NAME) {
        return Err(E_BAD_NAME);
    }
    hits += 1;

    // I2：不存在的条目查种类 → E_NOENT。
    if ns.kind_of("ghost") != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    hits += 1;

    // R7：终审——EEXIST 账、类型位账、悬空账三账对平。
    if ns.eexist_hits != INJECT_TIMES + 3 {
        return Err(E_LEDGER);
    }
    if !ns.type_bits_clean() || ns.iflnk_set != 4 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
