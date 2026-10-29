//! unx4034_fsnamespace — UNX-F4034 fs 类型名空间与重名拒绝（诊断段 0x86C0）。
//!
//! 判据原文：
//!   J1 类型名 32 字节内合法字符集校验生效；
//!   J2 重名注册注入返回 EEXIST 10/10 次；
//!   J3 类型表无重名不变式成立。
//! 设计纪律：
//!   1. 名空间模型：全局类型表存（名, 注册序号）二元组，
//!      名为 32 字节定长（零填充），注册序号单调递增。
//!   2. 字符集闭集：首字符限字母，后续限字母/数字/下划线/点/
//!      连字符——闭集外字节（含控制符与非 ASCII）一律拒收。
//!   3. 重名语义：注册前全表查重，命中返回 EEXIST 且表不变
//!      （注入 10 次表长度零变化）。
//!   4. 无重名不变式：任意两表项名两两互异；注销后同名为合法
//!      再注册（名空间回收语义）。
//!   5. 边界：空名、33 字节超长名、非法首字符、表满——全部
//!      显性拒收并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x86C0;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_EXIST: u16 = SEG_BASE + 1;       // 重名注册（EEXIST 语义）
const E_NAME_EMPTY: u16 = SEG_BASE + 2;  // 空名
const E_NAME_LONG: u16 = SEG_BASE + 3;   // 名超 32 字节
const E_NAME_BADC: u16 = SEG_BASE + 4;   // 非法字符（闭集外）
const E_NAME_BADF: u16 = SEG_BASE + 5;   // 非法首字符（首字符限字母）
const E_NO_TYPE: u16 = SEG_BASE + 6;     // 查找/注销不存在的类型
const E_TABLE_FULL: u16 = SEG_BASE + 7;  // 类型表满
const E_BAL: u16 = SEG_BASE + 8;         // 注册/注销账不平

/// 类型名定长字节。
pub const NAME_CAP: usize = 32;
/// 类型表静态容量。
pub const TABLE_CAP: usize = 16;

/// 类型表项：名（零填充）+ 有效长度 + 注册序号。
pub struct TypeEntry {
    pub name: [u8; NAME_CAP],
    pub name_len: usize,
    pub seq: u32, // 注册序号（单调）
}

/// fs 类型名空间：类型表 + 注册/注销账。
pub struct FsNamespace {
    pub entries: Vec<TypeEntry>,
    pub reg_ok: u32,     // 注册成功数
    pub reg_dup: u32,    // 重名拒收数（判据 J2 账面）
    pub reg_bad: u32,    // 名非法拒收数
    pub unreg_ok: u32,   // 注销成功数
    pub seq_next: u32,   // 下一个注册序号
}

impl FsNamespace {
    pub fn new() -> FsNamespace {
        FsNamespace {
            entries: Vec::new(),
            reg_ok: 0,
            reg_dup: 0,
            reg_bad: 0,
            unreg_ok: 0,
            seq_next: 1,
        }
    }

    /// 首字符校验：限 ASCII 字母。
    fn first_ok(b: u8) -> bool {
        (b >= b'a' && b <= b'z') || (b >= b'A' && b <= b'Z')
    }

    /// 后续字符闭集：字母/数字/下划线/点/连字符。
    fn rest_ok(b: u8) -> bool {
        Self::first_ok(b)
            || (b >= b'0' && b <= b'9')
            || b == b'_'
            || b == b'.'
            || b == b'-'
    }

    /// 全名合法性校验（判据 J1 正向）。
    pub fn name_valid(name: &[u8]) -> Result<(), u16> {
        if name.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        if name.len() > NAME_CAP {
            return Err(E_NAME_LONG);
        }
        if !Self::first_ok(name[0]) {
            return Err(E_NAME_BADF);
        }
        let mut i = 1;
        while i < name.len() {
            if !Self::rest_ok(name[i]) {
                return Err(E_NAME_BADC);
            }
            i += 1;
        }
        Ok(())
    }

    /// 查重：命中返回表下标。
    fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if e.name_len == name.len() {
                let mut j = 0;
                let mut same = true;
                while j < name.len() {
                    if j >= NAME_CAP || e.name[j] != name[j] {
                        same = false;
                        break;
                    }
                    j += 1;
                }
                if same {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 注册：名校验 → 查重（EEXIST）→ 入表。
    pub fn register(&mut self, name: &[u8]) -> Result<u32, u16> {
        if let Err(e) = Self::name_valid(name) {
            self.reg_bad += 1;
            return Err(e);
        }
        if self.find(name).is_some() {
            // 判据 J2：重名注入返回 EEXIST 且表不变。
            self.reg_dup += 1;
            return Err(E_EXIST);
        }
        if self.entries.len() >= TABLE_CAP {
            self.reg_bad += 1;
            return Err(E_TABLE_FULL);
        }
        let mut nm = [0u8; NAME_CAP];
        let mut i = 0;
        while i < name.len() {
            nm[i] = name[i];
            i += 1;
        }
        let seq = self.seq_next;
        self.entries.push(TypeEntry { name: nm, name_len: name.len(), seq });
        self.seq_next += 1;
        self.reg_ok += 1;
        Ok(seq)
    }

    /// 注销：命中移出（名空间回收）；未命中拒收。
    pub fn unregister(&mut self, name: &[u8]) -> Result<(), u16> {
        match self.find(name) {
            Some(i) => {
                // 索引前先比长度（铁律 5）。
                if i >= self.entries.len() {
                    return Err(E_NO_TYPE);
                }
                self.entries.remove(i);
                self.unreg_ok += 1;
                Ok(())
            }
            None => Err(E_NO_TYPE),
        }
    }

    /// 查找类型序号。
    pub fn lookup(&self, name: &[u8]) -> Result<u32, u16> {
        match self.find(name) {
            Some(i) => Ok(self.entries[i].seq),
            None => Err(E_NO_TYPE),
        }
    }

    /// 名空间占用视图：返回（表长，剩余容量，下一序号）。
    pub fn space_view(&self) -> (usize, usize, u32) {
        (self.entries.len(), TABLE_CAP - self.entries.len(), self.seq_next)
    }

    /// 表长度查询。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 无重名不变式（判据 J3）：两两互异逐对核对。
    pub fn no_dup_invariant(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < self.entries.len() {
            let mut j = i + 1;
            while j < self.entries.len() {
                let a = &self.entries[i];
                let b = &self.entries[j];
                if a.name_len == b.name_len {
                    let mut k = 0;
                    let mut same = true;
                    while k < a.name_len {
                        if a.name[k] != b.name[k] {
                            same = false;
                            break;
                        }
                        k += 1;
                    }
                    if same {
                        return Err(E_EXIST);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 守恒对账：注册成功 - 注销 = 表长；各账非负。
    pub fn audit(&self) -> Result<(), u16> {
        if self.reg_ok < self.unreg_ok {
            return Err(E_BAL);
        }
        if self.reg_ok - self.unreg_ok != self.entries.len() as u32 {
            return Err(E_BAL);
        }
        self.no_dup_invariant()
    }

    /// 导出快照（表长，注册账，重名账，注销账）。
    pub fn ledger(&self) -> (usize, u32, u32, u32) {
        (self.entries.len(), self.reg_ok, self.reg_dup, self.unreg_ok)
    }
}

/// 自例程：R1 注册查找 → R2 重名注入 10/10 → R3 字符集注入 → R4 注销回收 → R5 不变式守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut ns = FsNamespace::new();

    // R1 正常注册：合法名入表，序号单调，查找命中。
    let s1 = ns.register(b"ramfs")?;
    let s2 = ns.register(b"proc2")?;
    if s1 >= s2 || ns.lookup(b"ramfs")? != s1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 重名注入 10/10：十次重复注册全部 EEXIST 且表长不变。
    let before = ns.len();
    let mut hit10 = 0;
    let mut k = 0;
    while k < 10 {
        if ns.register(b"ramfs") == Err(E_EXIST) {
            hit10 += 1;
        }
        k += 1;
    }
    if hit10 != 10 || ns.len() != before {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 字符集注入：空名/超长/坏首字符/闭集外字符分别被拒。
    if ns.register(b"") != Err(E_NAME_EMPTY) {
        return Err(E_BAL);
    }
    let long = [0x61u8; NAME_CAP + 1];
    if ns.register(&long) != Err(E_NAME_LONG) {
        return Err(E_BAL);
    }
    if ns.register(b"1ramfs") != Err(E_NAME_BADF) {
        return Err(E_BAL);
    }
    if ns.register(b"ram\xffs") != Err(E_NAME_BADC) {
        return Err(E_BAL);
    }
    if ns.register(b"ram fs") != Err(E_NAME_BADC) {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 注销与回收：注销后同名可再注册，序号继续单调。
    ns.unregister(b"proc2")?;
    if ns.lookup(b"proc2").is_ok() {
        return Err(E_BAL);
    }
    let s3 = ns.register(b"proc2")?;
    if s3 <= s2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 不变式与守恒：无重名逐对核对通过，账面闭合。
    ns.no_dup_invariant()?;
    ns.audit()?;
    let (ln, rok, rdup, uok) = ns.ledger();
    if ln != 2 || rok != 3 || rdup != 10 || uok != 1 {
        return Err(E_BAL);
    }
    // 名空间占用视图：表长 2、剩余 14、序号推进到 4。
    let (ln2, free2, seq2) = ns.space_view();
    if ln2 != 2 || free2 != TABLE_CAP - 2 || seq2 != 4 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
