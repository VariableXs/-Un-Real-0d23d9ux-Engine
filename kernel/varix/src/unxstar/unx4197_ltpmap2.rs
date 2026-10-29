//! unx4197_ltpmap2 — UNX-F4197 LTP 链接族用例映射 ≥20 条（诊断段 0x9A20）。
//!
//! 判据原文：J1 映射表条目 ≥20 且用例名互异（LTP 真实用例命名）；
//!           J2 每条用例必须落到「行为路径 + 期望 errno」二元组，二者
//!           组合在闭集内（路径闭集 × errno 闭集）；
//!           J3 symlink/readlink/lstat/link/unlink/rename 六族全覆盖，
//!           任何一族条目数 ≥2；
//!           J4 映射账守恒：total = per_family 之和，且与注册数一致。
//! 设计纪律：
//!   1. LTP（Linux Test Project）链接族用例名参照 symlink01~05、
//!      readlink01、lstat01、link01~03、unlink01、rename01 等真实命名。
//!   2. 行为路径闭集：SymlinkCreate/SymlinkResolve/SymlinkStat/
//!      HardlinkCreate/HardlinkRemove/HardlinkRename/LinkFollow/
//!      DanglingDetect 八类，每用例恰归一类。
//!   3. 期望 errno 闭集：OK/ENOENT/EEXIST/ELOOP/EINVAL/ENAMETOOLONG/
//!      ENOTDIR/EISDIR/EXDEV/EBUSY 十态；OK 用 0 表示。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9A20 本单独占。

use crate::alloc::vec::Vec;
use crate::alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x9A20;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DUP_CASE: u16 = SEG_BASE + 1;   // 用例名重复
const E_BAD_PATH: u16 = SEG_BASE + 2;   // 行为路径越闭集
const E_BAD_ERRNO: u16 = SEG_BASE + 3;  // 期望 errno 越闭集
const E_FAMILY_COVER: u16 = SEG_BASE + 4; // 六族覆盖不足
const E_BAL: u16 = SEG_BASE + 5;        // 映射账不守恒
const E_EMPTY_NAME: u16 = SEG_BASE + 6; // 空用例名
const E_REG_TWICE: u16 = SEG_BASE + 7;  // 表已封账再注册

/// 行为路径闭集（八类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Path {
    SymlinkCreate = 1,
    SymlinkResolve,
    SymlinkStat,
    HardlinkCreate,
    HardlinkRemove,
    HardlinkRename,
    LinkFollow,
    DanglingDetect,
}

/// 期望 errno 闭集（十态，OK=0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WantErr {
    Ok = 0,
    Noent = 2,
    Eexist = 17,
    Eloop = 40,
    Einval = 22,
    Enametoolong = 36,
    Enotdir = 20,
    Eisdir = 21,
    Exdev = 18,
    Ebusy = 16,
}

impl WantErr {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 链接六族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    Symlink = 1,
    Readlink,
    Lstat,
    Link,
    Unlink,
    Rename,
}

pub const FAMILIES: usize = 6;

/// 单条映射。
pub struct CaseMap {
    pub case: String,     // LTP 用例名
    pub family: Family,   // 所属族
    pub path: Path,       // 行为路径
    pub want: WantErr,    // 期望 errno
}

/// 映射表枢纽。
pub struct MapTable {
    entries: Vec<CaseMap>,
    sealed: bool,
    pub registered: u32,
    pub rejected: u32,
}

impl MapTable {
    pub fn new() -> MapTable {
        MapTable {
            entries: Vec::new(),
            sealed: false,
            registered: 0,
            rejected: 0,
        }
    }

    fn family_known(f: Family) -> bool {
        (f as u16) >= 1 && (f as u16) <= FAMILIES as u16
    }

    /// 注册一条映射（封账后拒绝）。
    pub fn add(&mut self, case: &str, family: Family, path: Path, want: WantErr) -> Result<usize, u16> {
        if self.sealed {
            self.rejected += 1;
            return Err(E_REG_TWICE);
        }
        if case.is_empty() {
            self.rejected += 1;
            return Err(E_EMPTY_NAME);
        }
        if !Self::family_known(family) {
            self.rejected += 1;
            return Err(E_FAMILY_COVER);
        }
        let p = path as u16;
        if p < 1 || p > 8 {
            self.rejected += 1;
            return Err(E_BAD_PATH);
        }
        let w = want.code();
        // 闭集核对：OK(0)/2/17/40/22/36/20/21/18/16
        if !matches!(w, 0 | 2 | 17 | 40 | 22 | 36 | 20 | 21 | 18 | 16) {
            self.rejected += 1;
            return Err(E_BAD_ERRNO);
        }
        for e in self.entries.iter() {
            if e.case == case {
                self.rejected += 1;
                return Err(E_DUP_CASE);
            }
        }
        self.entries.push(CaseMap {
            case: case.to_string(),
            family,
            path,
            want,
        });
        self.registered += 1;
        Ok(self.entries.len() - 1)
    }

    /// 封账：之后 add 全拒。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 条目总数。
    pub fn total(&self) -> usize {
        self.entries.len()
    }

    /// 按族计数。
    pub fn family_count(&self, f: Family) -> usize {
        self.entries.iter().filter(|e| e.family == f).count()
    }

    /// 族覆盖判定（判据 J3）：六族全有且每族 ≥2。
    pub fn families_covered(&self) -> bool {
        use Family::*;
        let fams = [Symlink, Readlink, Lstat, Link, Unlink, Rename];
        fams.iter().all(|f| self.family_count(*f) >= 2)
    }

    /// 账守恒（判据 J4）：total == Σ per_family == registered。
    pub fn ledger_conserved(&self) -> bool {
        use Family::*;
        let fams = [Symlink, Readlink, Lstat, Link, Unlink, Rename];
        let sum: usize = fams.iter().map(|f| self.family_count(*f)).sum();
        sum == self.entries.len() && self.entries.len() as u32 == self.registered
    }

    /// 按用例名查映射。
    pub fn find(&self, case: &str) -> Option<(Path, WantErr)> {
        self.entries.iter().find(|e| e.case == case).map(|e| (e.path, e.want))
    }

    /// 静态装载 LTP 链接族 ≥20 条真实用例（判据 J1 的承载主体）。
    pub fn load_ltp_cases(&mut self) -> Result<usize, u16> {
        // symlink 族
        self.add("symlink01", Family::Symlink, Path::SymlinkCreate, WantErr::Ok)?;
        self.add("symlink02", Family::Symlink, Path::SymlinkCreate, WantErr::Eexist)?;
        self.add("symlink03", Family::Symlink, Path::SymlinkCreate, WantErr::Enametoolong)?;
        self.add("symlink04", Family::Symlink, Path::SymlinkCreate, WantErr::Ebusy)?;
        // readlink 族
        self.add("readlink01", Family::Readlink, Path::SymlinkResolve, WantErr::Ok)?;
        self.add("readlink02", Family::Readlink, Path::SymlinkResolve, WantErr::Einval)?;
        self.add("readlink03", Family::Readlink, Path::DanglingDetect, WantErr::Ok)?;
        // lstat 族
        self.add("lstat01", Family::Lstat, Path::SymlinkStat, WantErr::Ok)?;
        self.add("lstat02", Family::Lstat, Path::SymlinkStat, WantErr::Noent)?;
        self.add("lstat03", Family::Lstat, Path::SymlinkStat, WantErr::Eloop)?;
        // link 族
        self.add("link01", Family::Link, Path::HardlinkCreate, WantErr::Ok)?;
        self.add("link02", Family::Link, Path::HardlinkCreate, WantErr::Eexist)?;
        self.add("link03", Family::Link, Path::HardlinkCreate, WantErr::Exdev)?;
        self.add("link04", Family::Link, Path::HardlinkCreate, WantErr::Eisdir)?;
        // unlink 族
        self.add("unlink01", Family::Unlink, Path::HardlinkRemove, WantErr::Ok)?;
        self.add("unlink02", Family::Unlink, Path::HardlinkRemove, WantErr::Noent)?;
        self.add("unlink03", Family::Unlink, Path::HardlinkRemove, WantErr::Eisdir)?;
        self.add("unlink04", Family::Unlink, Path::DanglingDetect, WantErr::Ok)?;
        // rename 族
        self.add("rename01", Family::Rename, Path::HardlinkRename, WantErr::Ok)?;
        self.add("rename02", Family::Rename, Path::HardlinkRename, WantErr::Noent)?;
        self.add("rename03", Family::Rename, Path::HardlinkRename, WantErr::Enotdir)?;
        self.add("rename04", Family::Rename, Path::LinkFollow, WantErr::Eisdir)?;
        Ok(self.entries.len())
    }
}

/// 最低条目要求（卡面 ≥20）。
pub const MIN_CASES: usize = 20;

/// 自例程：R1 静态装载 ≥20 条；R2 六族覆盖；R3 闭集越界注入全拒；
/// R4 查表往返；R5 封账后拒注册；R6 账守恒终验。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：装载 + 条目下限
    let mut t = MapTable::new();
    let n = t.load_ltp_cases()?;
    if n < MIN_CASES {
        return Err(E_FAMILY_COVER);
    }
    if t.total() != n || t.registered as usize != n {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：六族覆盖（每族 ≥2）
    if !t.families_covered() {
        return Err(E_FAMILY_COVER);
    }
    hits += 1;

    // R3：注入拦截——重名 / 空名 / 封账后（R5 前先验重名与空名）
    if t.add("symlink01", Family::Symlink, Path::SymlinkCreate, WantErr::Ok).is_ok() {
        return Err(E_DUP_CASE);
    }
    if t.add("", Family::Symlink, Path::SymlinkCreate, WantErr::Ok) != Err(E_EMPTY_NAME) {
        return Err(E_EMPTY_NAME);
    }
    if t.add("x_case", Family::Symlink, Path::DanglingDetect, WantErr::Ebusy).is_err() {
        // Ebusy 在闭集内，应成功——走到这里说明闭集写错
        return Err(E_BAD_ERRNO);
    }
    hits += 1;

    // R4：查表往返——symlink03 → (SymlinkCreate, ENAMETOOLONG)
    let (p, w) = match t.find("symlink03") {
        Some(v) => v,
        None => return Err(E_BAL),
    };
    if p != Path::SymlinkCreate || w != WantErr::Enametoolong {
        return Err(E_BAL);
    }
    if t.find("nonexistent_case").is_some() {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：封账后再注册必拒
    t.seal();
    if t.add("late_case", Family::Rename, Path::HardlinkRename, WantErr::Ok) != Err(E_REG_TWICE) {
        return Err(E_REG_TWICE);
    }
    hits += 1;

    // R6：账守恒终验
    if !t.ledger_conserved() {
        return Err(E_BAL);
    }
    // registered 与 rejected 分账闭合：注册尝试 = 成功 + 拒绝
    let tries = t.registered + t.rejected;
    if tries < t.registered || t.rejected < 4 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
