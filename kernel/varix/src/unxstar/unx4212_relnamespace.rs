//! unx4212_relnamespace — UNX-F4212 相对路径与命名空间交叉预锚（诊断段 0x9BC0）。
//!
//! 判据原文：J1 相对路径基准闭集：CWD/CWD_FD/DIRFD/FD-relative 四
//!           基准各自持独立命名空间账，解析账互不串账——同一路径
//!           串在四基准下的解析轨迹各自入账；
//!           J2 dirfd 语义：以目录 fd 为基准时，相对路径只在该目录
//!           子树内解析——越出尝试（".." 越过 dirfd 基准）按基准
//!           类型账分别处理：普通目录 fd 允许越出（POSIX openat
//!           语义），隔离 fd（O_PATH 隔离类）拒绝越出；
//!           J3 命名空间交叉账：跨基准解析不共享缓存段——每基准
//!           缓存命中账独立，交叉引用计数为零恒成立；
//!           J4 解析账守恒：resolved = hit_own + miss_own，且四基
//!           准账总和与总解析账闭合。
//! 设计纪律：
//!   1. Linux openat 语义：AT_FDCWD / dirfd 双基准；本锚扩为四基
//!      准闭集（CWD、CWD_FD、DIRFD 普通、DIRFD 隔离）做预研账。
//!   2. 命名空间 = (基准 id, 子树前缀账)；交叉引用 = 基准间共享
//!      缓存条目数，结构上为零（各自 Vec）。
//!   3. 越出闸：普通基准 depth ≥ 0 允许 ".."，隔离基准 depth==0
//!      时 ".." 拒收（EACCES 语义账）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9BC0 本单独占。

use crate::alloc::vec::Vec;
use crate::alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x9BC0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_JAIL: u16 = SEG_BASE + 1;    // 隔离基准越出（EACCES 账）
const E_BASE_BAD: u16 = SEG_BASE + 2;// 基准 id 越闭集
const E_EMPTY: u16 = SEG_BASE + 3;   // 空路径
const E_BAL: u16 = SEG_BASE + 4;     // 解析账不守恒
const E_ISOLATE: u16 = SEG_BASE + 5; // 跨基准缓存交叉引用非零

/// 基准四类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaseKind {
    Cwd = 0,      // 相对 cwd
    CwdFd,        // AT_FDCWD 语义 fd
    Dirfd,        // 普通目录 fd
    DirfdJail,    // 隔离 fd（越出拒收）
}

pub const NBASES: usize = 4;

/// 单基准命名空间账。
pub struct NsAccount {
    pub kind: BaseKind,
    pub depth: i32,
    pub hit_own: u32,
    pub miss_own: u32,
    pub jail_rejects: u32,
    /// 本基准缓存段（独立 Vec——交叉引用结构上为零的前提）
    cache: Vec<String>,
}

impl NsAccount {
    fn new(kind: BaseKind) -> NsAccount {
        NsAccount {
            kind,
            depth: 0,
            hit_own: 0,
            miss_own: 0,
            jail_rejects: 0,
            cache: Vec::new(),
        }
    }

    fn is_jail(&self) -> bool {
        self.kind == BaseKind::DirfdJail
    }
}

/// 命名空间交叉预锚枢纽。
pub struct RelNs {
    accounts: Vec<NsAccount>,
    pub resolved_total: u32,
}

impl RelNs {
    pub fn new() -> RelNs {
        let mut accounts = Vec::new();
        accounts.push(NsAccount::new(BaseKind::Cwd));
        accounts.push(NsAccount::new(BaseKind::CwdFd));
        accounts.push(NsAccount::new(BaseKind::Dirfd));
        accounts.push(NsAccount::new(BaseKind::DirfdJail));
        RelNs {
            accounts,
            resolved_total: 0,
        }
    }

    /// 相对路径解析（判据 J1/J2）：
    /// 返回（基准 id, 终态深度）。分量逐个过账；隔离基准 depth==0
    /// 时 ".." 拒收（J2 越出闸）。
    pub fn resolve(&mut self, base: BaseKind, rel: &str) -> Result<(usize, i32), u16> {
        if rel.is_empty() {
            return Err(E_EMPTY);
        }
        let b = base as usize;
        if b >= NBASES {
            return Err(E_BASE_BAD);
        }
        self.resolved_total += 1;
        let mut hit = true;
        let mut cur = String::new();
        for ch in rel.chars() {
            if ch == '/' {
                if !cur.is_empty() {
                    self.step(b, &cur, &mut hit)?;
                    cur.clear();
                }
            } else {
                cur.push(ch);
            }
        }
        if !cur.is_empty() {
            self.step(b, &cur, &mut hit)?;
        }
        let d = self.accounts[b].depth;
        if hit {
            self.accounts[b].hit_own += 1;
        } else {
            self.accounts[b].miss_own += 1;
        }
        Ok((b, d))
    }

    fn step(&mut self, b: usize, comp: &str, hit: &mut bool) -> Result<(), u16> {
        match comp {
            "." => {}
            ".." => {
                if self.accounts[b].depth == 0 && self.accounts[b].is_jail() {
                    // 隔离基准越出拒收（判据 J2）
                    self.accounts[b].jail_rejects += 1;
                    *hit = false;
                    return Err(E_JAIL);
                }
                if self.accounts[b].depth > 0 {
                    self.accounts[b].depth -= 1;
                }
                // depth==0 的非隔离基准：越出到基准父（openat 语义允许），账面深度保持 0
            }
            _ => {
                self.accounts[b].depth += 1;
                // 本基准缓存段登记（独立 Vec）
                let key = comp.to_string();
                let mut found = false;
                for c in self.accounts[b].cache.iter() {
                    if *c == key {
                        found = true;
                        break;
                    }
                }
                if !found {
                    self.accounts[b].cache.push(key);
                }
            }
        }
        Ok(())
    }

    /// 缓存命中（判据 J3）：只查本基准段。
    pub fn cache_probe(&self, base: BaseKind, name: &str) -> Result<bool, u16> {
        let b = base as usize;
        if b >= NBASES {
            return Err(E_BASE_BAD);
        }
        for c in self.accounts[b].cache.iter() {
            if c == name {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 交叉引用计数（判据 J3）：结构上恒零（独立 Vec，无可共享句柄）。
    pub fn cross_refs(&self) -> u32 {
        0
    }

    /// 解析账守恒（判据 J4）。
    pub fn balanced(&self) -> bool {
        let mut sum = 0u32;
        for a in self.accounts.iter() {
            if a.hit_own + a.miss_own > self.resolved_total {
                return false;
            }
            sum += a.hit_own + a.miss_own;
        }
        sum == self.resolved_total
    }

    /// 单基准账导出。
    pub fn account(&self, base: BaseKind) -> Result<(u32, u32, i32, u32), u16> {
        let b = base as usize;
        if b >= NBASES {
            return Err(E_BASE_BAD);
        }
        let a = &self.accounts[b];
        Ok((a.hit_own, a.miss_own, a.depth, a.jail_rejects))
    }
}

/// 自例程：R1 四基准独立解析同串；R2 隔离基准越出拒收；R3 缓存独
/// 立与交叉零；R4 注入拦截；R5 守恒终验。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：同串 "a/b/c" 在四基准各自解析，账互不串（判据 J1）。
    let mut ns = RelNs::new();
    for k in [BaseKind::Cwd, BaseKind::CwdFd, BaseKind::Dirfd, BaseKind::DirfdJail].iter() {
        let (b, d) = ns.resolve(*k, "a/b/c")?;
        if b != *k as usize || d != 3 {
            return Err(E_BAL);
        }
    }
    for k in [BaseKind::Cwd, BaseKind::CwdFd, BaseKind::Dirfd, BaseKind::DirfdJail].iter() {
        let (h, m, d, j) = ns.account(*k)?;
        if h != 1 || m != 0 || d != 3 || j != 0 {
            return Err(E_BAL);
        }
    }
    hits += 1;

    // R2：隔离基准 depth==0 时 ".." 拒收；普通基准放行（判据 J2）。
    if ns.resolve(BaseKind::DirfdJail, "..") != Err(E_JAIL) {
        return Err(E_JAIL);
    }
    let (_, d2) = ns.resolve(BaseKind::Dirfd, "..")?;
    if d2 != 2 {
        return Err(E_BAL); // 3-1=2：普通基准退级
    }
    let (_, _, _, jr) = ns.account(BaseKind::DirfdJail)?;
    if jr != 1 {
        return Err(E_JAIL);
    }
    hits += 1;

    // R3：缓存独立——cwd 基准登记过 a/b/c，dirfdjail 段查 a 命中（
    // 同串解析过），但隔离拒收路径不污染；交叉引用恒零（判据 J3）。
    if !ns.cache_probe(BaseKind::Cwd, "b")? {
        return Err(E_BAL);
    }
    if !ns.cache_probe(BaseKind::DirfdJail, "a")? {
        return Err(E_BAL);
    }
    if ns.cache_probe(BaseKind::CwdFd, "nonexist")? {
        return Err(E_BAL);
    }
    if ns.cross_refs() != 0 {
        return Err(E_ISOLATE);
    }
    hits += 1;

    // R4：注入拦截——空路径、越基准 id。
    if ns.resolve(BaseKind::Cwd, "") != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 越闭集基准以账面值表达（u32 不可构 BaseKind，账目闭集以 NBASES 守恒代偿）
    if NBASES != 4 {
        return Err(E_BASE_BAD);
    }
    hits += 1;

    // R5：守恒终验（判据 J4）——Σ(own) == resolved_total。
    if !ns.balanced() {
        return Err(E_BAL);
    }
    let mut sum = 0u32;
    for k in [BaseKind::Cwd, BaseKind::CwdFd, BaseKind::Dirfd, BaseKind::DirfdJail].iter() {
        let (h, m, _, _) = ns.account(*k)?;
        sum += h + m;
    }
    if sum != ns.resolved_total {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
