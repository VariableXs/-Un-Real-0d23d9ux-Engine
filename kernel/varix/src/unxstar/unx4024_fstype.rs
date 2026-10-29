//! unx4024_fstype — UNX-F4024 file_system_type 结构与 fs 类型表（诊断段 0x8580）。
//!
//! 判据原文：
//!   J1 file_system_type 字段集（名/标志/get_sb/kill_sb/owner）实例化断言
//!      通过：五字段逐一可访问，get_sb 与 kill_sb 钩子非空。
//!   J2 类型表插入/查找/删除三操作语义账全过：插入唯一、查找命中、删除
//!      后不可再查得。
//!   J3 标志位闭集：FS_REQUIRES_DEV / FS_HAS_SUBTYPE 等四位，越界拒收。
//!   J4 表账守恒：表长 == 插入成功 - 删除成功，拒绝操作不改表长。
//! 设计纪律：
//!   1. file_system_type 是静态描述（每种 fs 一份），运行期实例在 sb 侧；
//!      本模块只维护类型描述与类型表，不创建 sb。
//!   2. get_sb / kill_sb 以「钩子槽号」建模（0 为空）；实例化强制两者非空
//!      ——这是 mount 路径「类型必须提供挂载与卸载能力」的投影。
//!   3. 类型名以长度 + 哈希为键（不持有字符串）：名长 1..=15（fs 名上限），
//!      同名（哈希 + 长相等）视作重名。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8580;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_HOOK_NULL: u16 = SEG_BASE + 1;  // get_sb/kill_sb 钩子为空
const E_NAME_BAD: u16 = SEG_BASE + 2;   // 类型名为空或超 15 字节
const E_FLAG_BAD: u16 = SEG_BASE + 3;   // 标志位越出闭集
const E_TYPE_DUP: u16 = SEG_BASE + 4;   // 类型重名（插入拒绝）
const E_TYPE_MISS: u16 = SEG_BASE + 5;  // 查找/删除未命中
const E_OWNER_NULL: u16 = SEG_BASE + 6; // owner 模块槽为空
const E_BAL: u16 = SEG_BASE + 7;        // 类型表账不平

/// fs 名长度上限（Linux FS_NAME_MAX = 16，含终止符故内容 ≤15）。
pub const FS_NAME_MAX: usize = 15;
/// 类型表容量上限（内建 + 模块 fs 合计）。
pub const FSTYPE_MAX: usize = 64;

/// fs 类型标志位闭集（Linux file_system_type.fs_flags 子集）。
pub const FS_REQUIRES_DEV: u32 = 0x0001;  // 需要块设备
pub const FS_BINARY_MOUNTDATA: u32 = 0x0002; // 二进制挂载数据
pub const FS_HAS_SUBTYPE: u32 = 0x0004;   // 有子类型
pub const FS_USERNS_MOUNT: u32 = 0x0008;  // 允许用户命名空间挂载
pub const FS_FLAG_MASK: u32 =
    FS_REQUIRES_DEV | FS_BINARY_MOUNTDATA | FS_HAS_SUBTYPE | FS_USERNS_MOUNT;

/// 类型注册状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeState {
    Declared = 0,  // 已声明未入表
    Registered = 1, // 已入表可查找
    Removed = 2,   // 已删除（终态）
}

/// file_system_type 结构锚（五字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FsType {
    pub name_len: u8,   // 类型名长度
    pub name_hash: u32, // 类型名哈希（键）
    pub flags: u32,     // fs_flags
    pub get_sb: u16,    // 挂载钩子槽号（0=空）
    pub kill_sb: u16,   // 卸载钩子槽号（0=空）
    pub owner: u16,     // 所属模块槽号（0=空）
    pub state: TypeState, // 注册状态
}

impl FsType {
    /// 实例化：五字段校验（判据 J1）——两钩子与 owner 均非空。
    pub fn new(name: &[u8], flags: u32, get_sb: u16, kill_sb: u16, owner: u16) -> Result<FsType, u16> {
        if name.is_empty() || name.len() > FS_NAME_MAX {
            return Err(E_NAME_BAD);
        }
        if flags & !FS_FLAG_MASK != 0 {
            return Err(E_FLAG_BAD);
        }
        if get_sb == 0 || kill_sb == 0 {
            return Err(E_HOOK_NULL);
        }
        if owner == 0 {
            return Err(E_OWNER_NULL);
        }
        Ok(FsType {
            name_len: name.len() as u8,
            name_hash: FsType::hash(name),
            flags,
            get_sb,
            kill_sb,
            owner,
            state: TypeState::Declared,
        })
    }

    /// 名字哈希：FNV-1a 32 位。
    pub fn hash(name: &[u8]) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for b in name.iter() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    /// 五字段清单导出（字段存在性断言用）。
    pub fn fields(&self) -> [&'static str; 5] {
        ["name", "flags", "get_sb", "kill_sb", "owner"]
    }

    /// 钩子齐全校验。
    pub fn hooks_ok(&self) -> bool {
        self.get_sb != 0 && self.kill_sb != 0 && self.owner != 0
    }

    /// 需要块设备判定（mount 路径前置）。
    pub fn requires_dev(&self) -> bool {
        self.flags & FS_REQUIRES_DEV != 0
    }
}

/// fs 类型表：插入/查找/删除三操作 + 账。
pub struct FsTypeTable {
    pub entries: Vec<FsType>, // 类型表
    pub inserts: u32,         // 插入成功账
    pub dels: u32,            // 删除成功账
    pub finds: u32,           // 查找命中账
    pub misses: u32,          // 查找未命中账
    pub dups: u32,            // 重名拒绝账
}

impl FsTypeTable {
    pub fn new() -> FsTypeTable {
        FsTypeTable { entries: Vec::new(), inserts: 0, dels: 0, finds: 0, misses: 0, dups: 0 }
    }

    /// 插入：重名（哈希与名长同）拒收，容量校验（判据 J2 插入面）。
    pub fn insert(&mut self, mut t: FsType) -> Result<usize, u16> {
        if t.state != TypeState::Declared {
            return Err(E_TYPE_DUP);
        }
        if !t.hooks_ok() {
            return Err(E_HOOK_NULL);
        }
        if self.entries.len() >= FSTYPE_MAX {
            return Err(E_BAL);
        }
        for e in self.entries.iter() {
            if e.name_hash == t.name_hash && e.name_len == t.name_len {
                self.dups += 1;
                return Err(E_TYPE_DUP);
            }
        }
        t.state = TypeState::Registered;
        self.entries.push(t);
        self.inserts += 1;
        Ok(self.entries.len())
    }

    /// 查找：按名哈希 + 名长定位，命中返回槽号（判据 J2 查找面）。
    pub fn find(&mut self, name: &[u8]) -> Result<usize, u16> {
        let h = FsType::hash(name);
        let l = name.len();
        let mut i = 0usize;
        for e in self.entries.iter() {
            if e.name_hash == h && e.name_len as usize == l && e.state == TypeState::Registered {
                self.finds += 1;
                return Ok(i);
            }
            i += 1;
        }
        self.misses += 1;
        Err(E_TYPE_MISS)
    }

    /// 删除：命中后置 Removed 并从表移除（判据 J2 删除面）。
    pub fn remove(&mut self, name: &[u8]) -> Result<usize, u16> {
        let idx = match self.find(name) {
            Ok(i) => i,
            Err(e) => return Err(e),
        };
        self.entries.remove(idx);
        self.dels += 1;
        Ok(self.entries.len())
    }

    /// 只读遍历辅助：按槽取类型。
    pub fn at(&self, idx: usize) -> Option<&FsType> {
        if idx >= self.entries.len() {
            return None;
        }
        Some(&self.entries[idx])
    }

    /// 表账守恒：len == inserts - dels（判据 J4）。
    pub fn conserve(&self) -> bool {
        self.entries.len() as i64 == self.inserts as i64 - self.dels as i64
    }

    /// 需要块设备的类型清单（mount 前置检查的读面）。
    pub fn dev_types(&self) -> u32 {
        let mut c = 0u32;
        for e in self.entries.iter() {
            if e.requires_dev() {
                c += 1;
            }
        }
        c
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_HOOK_NULL => "get_sb/kill_sb 钩子为空",
        E_NAME_BAD => "类型名为空或超 15 字节",
        E_FLAG_BAD => "fs_flags 越出闭集",
        E_TYPE_DUP => "类型重名（插入拒绝/状态重复）",
        E_TYPE_MISS => "查找/删除未命中",
        E_OWNER_NULL => "owner 模块槽为空",
        E_BAL => "类型表账不平或容量超限",
        _ => "未知诊断码",
    }
}

/// 确定性演练：五字段 → 三操作 → 标志闭集 → 注入拦截 → 账守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut tab = FsTypeTable::new();

    // R1：五字段实例化断言（判据 J1）——ext4 类型。
    let t = FsType::new(b"ext4", FS_REQUIRES_DEV, 11, 12, 3)?;
    if t.name_len != 4 || t.flags != FS_REQUIRES_DEV {
        return Err(E_NAME_BAD);
    }
    if t.get_sb != 11 || t.kill_sb != 12 || t.owner != 3 {
        return Err(E_HOOK_NULL);
    }
    if !t.hooks_ok() || !t.requires_dev() {
        return Err(E_HOOK_NULL);
    }
    if t.fields().len() != 5 {
        return Err(E_NAME_BAD);
    }
    hits += 1;

    // R2：插入三种类型（ext4/xfs/tmpfs），表长 3（判据 J2 插入面）。
    tab.insert(t)?;
    tab.insert(FsType::new(b"xfs", FS_REQUIRES_DEV | FS_HAS_SUBTYPE, 21, 22, 3)?)?;
    tab.insert(FsType::new(b"tmpfs", FS_USERNS_MOUNT, 31, 32, 4)?)?;
    if tab.entries.len() != 3 || tab.inserts != 3 {
        return Err(E_BAL);
    }
    if !tab.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：查找命中 ext4（槽 0），未命中 btrfs。
    if tab.find(b"ext4")? != 0 {
        return Err(E_TYPE_MISS);
    }
    if tab.find(b"btrfs") != Err(E_TYPE_MISS) {
        return Err(E_TYPE_MISS);
    }
    if tab.finds != 1 || tab.misses != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：删除 xfs 后不可再查得（判据 J2 删除面）。
    if tab.remove(b"xfs")? != 2 {
        return Err(E_BAL);
    }
    if tab.find(b"xfs") != Err(E_TYPE_MISS) {
        return Err(E_TYPE_MISS);
    }
    if tab.dels != 1 || !tab.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：设备类类型计数——ext4 需要设备、tmpfs 不需要，删 xfs 后 dev 类 1。
    if tab.dev_types() != 1 {
        return Err(E_FLAG_BAD);
    }
    hits += 1;

    // R6：标志位闭集——FS_REQUIRES_DEV|FS_USERNS_MOUNT 组合合法。
    let combo = FsType::new(b"fuse", FS_REQUIRES_DEV | FS_USERNS_MOUNT, 41, 42, 5)?;
    if combo.flags & FS_FLAG_MASK != combo.flags {
        return Err(E_FLAG_BAD);
    }
    tab.insert(combo)?;
    if tab.entries.len() != 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：重名注入被拒（同名同长），不入表且账 +1（判据 J2 拒绝面）。
    if tab.insert(FsType::new(b"ext4", 0, 51, 52, 6)?) != Err(E_TYPE_DUP) {
        return Err(E_TYPE_DUP);
    }
    if tab.dups != 1 || tab.entries.len() != 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：钩子为空注入被拒（get_sb / kill_sb 两向）。
    if FsType::new(b"bad1", 0, 0, 52, 6).err() != Some(E_HOOK_NULL) {
        return Err(E_HOOK_NULL);
    }
    if FsType::new(b"bad2", 0, 51, 0, 6).err() != Some(E_HOOK_NULL) {
        return Err(E_HOOK_NULL);
    }
    hits += 1;

    // I3：owner 为空注入被拒。
    if FsType::new(b"bad3", 0, 51, 52, 0).err() != Some(E_OWNER_NULL) {
        return Err(E_OWNER_NULL);
    }
    hits += 1;

    // I4：名超 15 字节注入被拒。
    let long = [b'z'; 16];
    if FsType::new(&long, 0, 51, 52, 6).err() != Some(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    hits += 1;

    // I5：标志位越界注入被拒。
    if FsType::new(b"bad4", 0x100, 51, 52, 6).err() != Some(E_FLAG_BAD) {
        return Err(E_FLAG_BAD);
    }
    hits += 1;

    // I6：删除未命中注入被拒，表长不变。
    let before = tab.entries.len();
    if tab.remove(b"nosuch") != Err(E_TYPE_MISS) {
        return Err(E_TYPE_MISS);
    }
    if tab.entries.len() != before || !tab.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // R7：末态表账守恒复核。
    if !tab.conserve() || tab.inserts != 4 || tab.dels != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
