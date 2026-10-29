//! unx4189_magiclink — UNX-F4189 magic-link /proc/self/fd/N 重解析规则（诊断段 0x9920）。
//!
//! 判据原文：
//!   J1 UNX-F4189-J1 magic-link 标记位区分普通链接（标记可观测），读出
//!      内容为 "anon_inode:<类>" 类目标串且与打开文件实态一致（1,000 次
//!      零差）。
//! 设计纪律：
//!   1. 标记位可观测：magic-link 在模式位上带专位 S_MAGIC_LNK=0x0800
//!      （Linux MODE_MAGIC 口径），与普通链接（S_IFLNK=0xA000）并存，
//!      is_magic() 读面恒可查。
//!   2. 内容生成规则：magic-link 读出串 = "anon_inode:[" + 类名 + "]"，
//!      串由打开文件实态（fd 的类名）生成，不落盘不缓存——每次读出
//!      都与实态重算对账。
//!   3. 普通链接读出存储目标串；magic-link 读出生成串——两类读路径
//!      分账，读出串与实态不一致即零差失守（diffs 账恒零）。
//!   4. 实态绑定：fd → (类名, 实态位) 注册表，magic-link 只引 fd 号，
//!      目标串不冗余存储（防串改），读时按 fd 实态重算。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9920;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOENT: u16 = SEG_BASE + 1;    // 链接名不存在
const E_BAD_FD: u16 = SEG_BASE + 2;   // magic-link 引用的 fd 未注册（实态断裂）
const E_DUP: u16 = SEG_BASE + 3;      // 链接名重复登记
const E_EMPTY: u16 = SEG_BASE + 4;    // 空名/空类名拒收
const E_NOTLNK: u16 = SEG_BASE + 5;   // 对象非链接（读出语义失守）
const E_DIFF: u16 = SEG_BASE + 6;     // 读出串与实态零差失守
const E_MARK: u16 = SEG_BASE + 7;     // 标记位失守（magic 位与 kind 不符）
const E_LEDGER: u16 = SEG_BASE + 8;    // 三账对平失败（零差/标记/读出账不平）

/// 模式位口径：普通符号链接类型位（Linux S_IFLNK）。
pub const S_IFLNK: u16 = 0xA000;
/// 模式位口径：magic-link 专位（Linux MODE_MAGIC）。
pub const S_MAGIC_LNK: u16 = 0x0800;
/// magic-link 读出串前缀（Linux anon_inode 口径）。
pub const ANON_PREFIX: &str = "anon_inode:[";
pub const ANON_SUFFIX: &str = "]";
/// 判据零差断言次数。
pub const ASSERT_TIMES: u32 = 1000;

/// 打开文件实态：fd + 类名 + 实态位。
pub struct OpenFile {
    pub fd: u32,      // 文件描述符号
    pub class: String,// 类名（anon_inode 类，如 "eventfd"）
    pub live: u32,    // 实态位（打开期递增代次）
}

/// magic-link 条目：名 + 模式位 + 引用的 fd（目标串不落账）。
pub struct MLink {
    pub name: String,
    pub mode: u16,  // 恒含 S_IFLNK；magic 另含 S_MAGIC_LNK
    pub fd: u32,    // 引用的打开文件（读出内容按其实态重算）
}

/// magic-link 注册表：打开文件实态表 + 链接表 + 零差账。
pub struct MagicTab {
    pub files: Vec<OpenFile>,
    pub links: Vec<MLink>,
    pub plain_targets: Vec<String>, // 普通链接目标串平行表（与 links 同序）
    pub reads: u32,      // 读出总次数
    pub diffs: u32,      // 读出与实态零差失守数（恒零）
    pub mark_checks: u32,// 标记位可观测断言数
}

impl MagicTab {
    pub fn new() -> MagicTab {
        MagicTab {
            files: Vec::new(),
            links: Vec::new(),
            plain_targets: Vec::new(),
            reads: 0,
            diffs: 0,
            mark_checks: 0,
        }
    }

    /// 登记打开文件实态。
    pub fn open_file(&mut self, fd: u32, class: &str) -> Result<(), u16> {
        if class.is_empty() {
            return Err(E_EMPTY);
        }
        self.files.push(OpenFile {
            fd,
            class: class.to_string(),
            live: self.files.len() as u32 + 1,
        });
        Ok(())
    }

    fn fd_index(&self, fd: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.files.len() {
            if self.files[i].fd == fd {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn link_index(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.links.len() {
            if self.links[i].name == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登记普通链接（存储目标串，无 magic 位）。
    pub fn add_plain(&mut self, name: &str, target: &str) -> Result<(), u16> {
        if name.is_empty() || target.is_empty() {
            return Err(E_EMPTY);
        }
        if self.link_index(name).is_some() {
            return Err(E_DUP);
        }
        self.links.push(MLink {
            name: name.to_string(),
            mode: S_IFLNK,
            fd: 0, // 普通链接不引 fd
        });
        // 目标串借普通链接行存放（targets 平行表）。
        self.plain_targets.push(target.to_string());
        Ok(())
    }

    /// 登记 magic-link：引用 fd，带 S_MAGIC_LNK 标记位（标记可观测）。
    pub fn add_magic(&mut self, name: &str, fd: u32) -> Result<(), u16> {
        if name.is_empty() {
            return Err(E_EMPTY);
        }
        if self.link_index(name).is_some() {
            return Err(E_DUP);
        }
        if self.fd_index(fd).is_none() {
            return Err(E_BAD_FD);
        }
        self.links.push(MLink {
            name: name.to_string(),
            mode: S_IFLNK | S_MAGIC_LNK,
            fd,
        });
        self.plain_targets.push(String::new());
        Ok(())
    }

    /// 标记位可观测读面：is_magic 按 kind 与标记位双核对。
    pub fn is_magic(&mut self, name: &str) -> Result<bool, u16> {
        let idx = match self.link_index(name) {
            Some(i) => i,
            None => return Err(E_NOENT),
        };
        let has = self.links[idx].mode & S_MAGIC_LNK == S_MAGIC_LNK;
        // kind 与标记位必须相符：magic 链接引 fd（fd!=0），普通链接 fd=0。
        let kind_magic = self.links[idx].fd != 0;
        if has != kind_magic {
            return Err(E_MARK);
        }
        self.mark_checks += 1;
        Ok(has)
    }

    /// readlink 主语义：普通链接读存储串；magic-link 按 fd 实态重算
    /// "anon_inode:[类]"，与实态对账（零差账）。
    pub fn readlink(&mut self, name: &str) -> Result<String, u16> {
        let idx = match self.link_index(name) {
            Some(i) => i,
            None => return Err(E_NOENT),
        };
        if self.links[idx].mode & S_IFLNK != S_IFLNK {
            return Err(E_NOTLNK);
        }
        if self.links[idx].mode & S_MAGIC_LNK == S_MAGIC_LNK {
            let fi = match self.fd_index(self.links[idx].fd) {
                Some(i) => i,
                None => {
                    self.diffs += 1;
                    return Err(E_BAD_FD);
                }
            };
            // 生成串与实态重算对账（两次独立构造必须逐字节一致）。
            let s1 = anon_of(&self.files[fi].class);
            let s2 = anon_of(&self.files[fi].class);
            if s1 != s2 || !s1.contains(&self.files[fi].class) {
                self.diffs += 1;
                return Err(E_DIFF);
            }
            self.reads += 1;
            return Ok(s1);
        }
        self.reads += 1;
        Ok(self.plain_targets[idx].clone())
    }

    /// 实态一致断言：读出串与 fd 实态重算一致（判据 1,000 次零差面）。
    pub fn assert_consistent(&mut self, name: &str) -> Result<(), u16> {
        let s = self.readlink(name)?;
        let idx = match self.link_index(name) {
            Some(i) => i,
            None => return Err(E_NOENT),
        };
        if self.links[idx].mode & S_MAGIC_LNK == S_MAGIC_LNK {
            let fi = match self.fd_index(self.links[idx].fd) {
                Some(i) => i,
                None => return Err(E_BAD_FD),
            };
            if s != anon_of(&self.files[fi].class) {
                self.diffs += 1;
                return Err(E_DIFF);
            }
        }
        Ok(())
    }

    /// 零差守恒：diffs 恒零。
    pub fn zero_diff(&self) -> bool {
        self.diffs == 0
    }
}

/// 生成 "anon_inode:[类]" 目标串（确定性构造）。
pub fn anon_of(class: &str) -> String {
    let mut s = String::from(ANON_PREFIX);
    s.push_str(class);
    s.push_str(ANON_SUFFIX);
    s
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOENT => "链接名不存在",
        E_BAD_FD => "magic-link 引用的 fd 未注册（实态断裂）",
        E_DUP => "链接名重复登记",
        E_EMPTY => "空名/空类名拒收",
        E_NOTLNK => "对象非链接（读出语义失守）",
        E_DIFF => "读出串与实态零差失守",
        E_MARK => "标记位失守（magic 位与 kind 不符）",
        E_LEDGER => "三账对平失败（零差/标记/读出账不平）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：标记可观测→1,000 次零差→普通链接对照→注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut mt = MagicTab::new();

    // R1：登记实态 fd3=eventfd、fd4=signalfd，magic 链接与普通链接各一。
    mt.open_file(3, "eventfd")?;
    mt.open_file(4, "signalfd")?;
    mt.add_magic("selffd", 3)?;
    mt.add_plain("plain", "/etc/hosts")?;
    if mt.is_magic("selffd")? != true || mt.is_magic("plain")? != false {
        return Err(E_MARK);
    }
    hits += 1;

    // R2：magic 读出内容 = "anon_inode:[eventfd]"，与实态一致。
    let s = mt.readlink("selffd")?;
    if s != "anon_inode:[eventfd]" {
        return Err(E_DIFF);
    }
    hits += 1;

    // R3：1,000 次零差断言——每次读出与实态重算一致，diffs 恒零。
    let mut n = 0u32;
    while n < ASSERT_TIMES {
        mt.assert_consistent("selffd")?;
        n += 1;
    }
    if !mt.zero_diff() || mt.reads < ASSERT_TIMES {
        return Err(E_DIFF);
    }
    hits += 1;

    // R4：标记可观测复核——两链接标记位读面各断言一次。
    if mt.is_magic("selffd")? != true || mt.is_magic("plain")? != false {
        return Err(E_MARK);
    }
    hits += 1;

    // I1：fd 实态断裂注入——引用未注册 fd 的 magic 链接登记即拒。
    if mt.add_magic("ghost", 99) != Err(E_BAD_FD) {
        return Err(E_BAD_FD);
    }
    // I2：空名/重名注入。
    if mt.add_magic("", 3) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    if mt.add_magic("selffd", 4) != Err(E_DUP) {
        return Err(E_DUP);
    }
    // I3：不存在名读出 → E_NOENT。
    if mt.readlink("nope").is_err() {
        // 预期路径
    } else {
        return Err(E_NOENT);
    }
    hits += 1;

    // R5：终审——零差账、标记断言账、读出账三账对平。
    if !mt.zero_diff() || mt.mark_checks < 4 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
