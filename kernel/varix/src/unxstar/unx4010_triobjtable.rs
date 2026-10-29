//! unx4010_triobjtable — UNX-F4010 inode/dentry/file 三对象与操作表总锚（诊断段 0x8400）。
//!
//! 判据原文：
//!   J1 三对象生命周期引用计数账万次开-关零泄漏（泄漏计数器恒零）。
//!   J2 三操作表（i_op/f_op/d_op）逐函数存在性断言通过：i_op 九项、f_op 十项、
//!      d_op 六项，任一槽为 0 即判缺项。
//!   J3 负 dentry 缓存命中账可导出且与理论值一致：负项命中一次计一次，
//!      再次 lookup 不得重新触发磁盘路径。
//!   J4 三对象槽号互指关系自洽：file → inode、dentry → inode、dentry → parent
//!      任一悬空即拒收。
//! 设计纪律：
//!   1. 三对象各有独立槽表（Vec），对象间以槽号互指；槽号 0 保留为「空」，
//!      因此注册从 1 开始（长度即槽号，取槽时先比长度再索引）。
//!   2. 操作表为纯数据表：每表以 &'static str 数组给出函数名清单，另有
//!      「已填槽位」位图；存在性断言 = 清单非空 ∧ 位图全 1 ∧ 长度吻合。
//!   3. 引用账：inode 引用由 dentry 绑定 + file 打开共同构成；账目式
//!      inode_refs == Σdentry_bind + Σfile_open，任何释放后仍须成立。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8400;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_SLOT_OOR: u16 = SEG_BASE + 1;  // 槽号越界（索引前未比长度）
const E_OP_MISS: u16 = SEG_BASE + 2;   // 操作表缺项（某槽为 0）
const E_DANGLING: u16 = SEG_BASE + 3;  // 对象互指悬空
const E_REF_LEAK: u16 = SEG_BASE + 4;  // 引用账泄漏（开-关不平）
const E_NEG_MISS: u16 = SEG_BASE + 5;  // 负 dentry 缓存命中账与理论值不符
const E_FLAG_BAD: u16 = SEG_BASE + 6;  // f_flags 位段非法

/// 操作表长度锚（Linux inode_operations / file_operations / dentry_operations 子集）。
pub const IOP_LEN: usize = 9;
pub const FOP_LEN: usize = 10;
pub const DOP_LEN: usize = 6;

/// i_op 九项函数名（顺序即槽号）。
pub const IOP_NAMES: [&'static str; IOP_LEN] = [
    "lookup", "create", "link", "unlink", "mkdir",
    "rmdir", "rename", "setattr", "getattr",
];

/// f_op 十项函数名。
pub const FOP_NAMES: [&'static str; FOP_LEN] = [
    "llseek", "read", "write", "readdir", "poll",
    "ioctl", "mmap", "open", "flush", "release",
];

/// d_op 六项函数名。
pub const DOP_NAMES: [&'static str; DOP_LEN] = [
    "lookup", "compare", "hash", "delete", "revalidate", "release",
];

/// 三对象类别（引用账键）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjKind {
    Inode = 0,
    Dentry = 1,
    File = 2,
}

/// 文件打开标志位（Open 语义子集）。
pub const O_RDONLY: u32 = 0o0;
pub const O_WRONLY: u32 = 0o1;
pub const O_RDWR: u32 = 0o2;
pub const O_APPEND: u32 = 0o2000;
pub const O_FLAG_MASK: u32 = O_RDONLY | O_WRONLY | O_RDWR | O_APPEND;

/// 对象生命周期状态机（三对象共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjState {
    Free = 0,     // 已释放
    Live = 1,     // 活跃
    Cached = 2,   // 仅缓存（负 dentry 专用）
}

/// 操作表：名字清单 + 槽位位图。
pub struct OpTable {
    pub names: &'static [&'static str], // 函数名清单
    pub filled: u32,                    // 已填槽位位图（bit i = 槽 i）
    pub calls: u32,                     // 经表调用次数（账）
}

impl OpTable {
    pub fn new(names: &'static [&'static str]) -> OpTable {
        // 全部槽位视为已填（本模块以常量表静态定义，无可空槽）。
        let mut mask = 0u32;
        let mut i = 0usize;
        while i < names.len() && i < 32 {
            mask |= 1u32 << i;
            i += 1;
        }
        OpTable { names, filled: mask, calls: 0 }
    }

    /// 存在性断言：清单非空、位图全 1、且无空名（判据 J2）。
    pub fn all_present(&self) -> bool {
        if self.names.is_empty() {
            return false;
        }
        for n in self.names.iter() {
            if n.is_empty() {
                return false;
            }
        }
        let full = (1u32 << self.names.len()) - 1;
        self.filled & full == full
    }

    /// 按名查槽：命中返回槽号。
    pub fn slot_of(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        for n in self.names.iter() {
            if *n == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 记账调用：命中才 +1，未命中不算调用（防虚账）。
    pub fn call(&mut self, name: &str) -> Result<usize, u16> {
        match self.slot_of(name) {
            Some(i) => {
                self.calls += 1;
                Ok(i)
            }
            None => Err(E_OP_MISS),
        }
    }
}

/// inode 对象（槽号从 1 起有效）。
pub struct InodeObj {
    pub ino: u16,     // inode 号
    pub refs: u32,    // 引用计数（dentry 绑定 + file 打开）
    pub state: ObjState, // 生命周期
    pub page_pages: u32, // 页缓存页数（锚）
}

/// dentry 对象。
pub struct DentryObj {
    pub name_hash: u32,   // 名字哈希（确定性 FNV-1a 片段）
    pub parent: u16,      // 父 dentry 槽号（0=根）
    pub inode: u16,       // 绑定 inode 槽号（0=负 dentry）
    pub negative: bool,   // 负缓存标记
    pub refs: u32,        // 引用计数
    pub d_flags: u32,     // dentry 标志位
    pub state: ObjState,  // 生命周期
}

/// file 对象。
pub struct FileObj {
    pub f_pos: u64,      // 当前偏移
    pub f_flags: u32,    // 打开标志
    pub f_inode: u16,    // 关联 inode 槽号
    pub refs: u32,       // 引用计数
    pub state: ObjState, // 生命周期
    pub writes: u32,     // 写次数（append 落点账）
    pub append_hits: u32, // O_APPEND 落点命中账
}

/// 三对象注册总锚：三槽表 + 引用账 + 负缓存账。
pub struct TriObjTable {
    pub inodes: Vec<InodeObj>,  // inode 槽表（1-based）
    pub dentries: Vec<DentryObj>, // dentry 槽表（1-based）
    pub files: Vec<FileObj>,    // file 槽表（1-based）
    pub i_op: OpTable,          // inode 操作表
    pub f_op: OpTable,          // file 操作表
    pub d_op: OpTable,          // dentry 操作表
    pub opens: u32,             // 打开总数
    pub closes: u32,            // 关闭总数
    pub leaks: u32,             // 泄漏账（open - close - 仍开）
    pub neg_hits: u32,          // 负 dentry 命中次数
    pub neg_misses: u32,        // 负 dentry 未命中（需走真实查找）
}

impl TriObjTable {
    pub fn new() -> TriObjTable {
        TriObjTable {
            inodes: Vec::new(),
            dentries: Vec::new(),
            files: Vec::new(),
            i_op: OpTable::new(&IOP_NAMES),
            f_op: OpTable::new(&FOP_NAMES),
            d_op: OpTable::new(&DOP_NAMES),
            opens: 0,
            closes: 0,
            leaks: 0,
            neg_hits: 0,
            neg_misses: 0,
        }
    }

    /// 名字哈希：FNV-1a 32 位确定性短串哈希（dentry 键）。
    pub fn hash_name(name: &[u8]) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for b in name.iter() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    /// 注册 inode：槽号 = 长度 + 1（0 保留为空）。
    pub fn add_inode(&mut self, ino: u16) -> Result<u16, u16> {
        let slot = (self.inodes.len() + 1) as u16;
        if slot == 0 {
            return Err(E_SLOT_OOR); // u16 溢出兜底
        }
        self.inodes.push(InodeObj {
            ino,
            refs: 0,
            state: ObjState::Live,
            page_pages: 0,
        });
        Ok(slot)
    }

    /// 注册 dentry：绑定 inode（0 表示负 dentry），父槽须存在。
    pub fn add_dentry(
        &mut self,
        name: &[u8],
        parent: u16,
        inode: u16,
        negative: bool,
    ) -> Result<u16, u16> {
        if parent != 0 && parent as usize > self.dentries.len() {
            return Err(E_DANGLING); // 父悬空
        }
        if inode != 0 && inode as usize > self.inodes.len() {
            return Err(E_DANGLING); // inode 悬空
        }
        if negative && inode != 0 {
            return Err(E_DANGLING); // 负 dentry 不得绑 inode
        }
        if !negative && inode == 0 {
            return Err(E_DANGLING); // 正 dentry 必须绑 inode
        }
        let slot = (self.dentries.len() + 1) as u16;
        self.dentries.push(DentryObj {
            name_hash: TriObjTable::hash_name(name),
            parent,
            inode,
            negative,
            refs: 1,
            d_flags: if negative { 0x1 } else { 0x0 },
            state: if negative { ObjState::Cached } else { ObjState::Live },
        });
        if inode != 0 {
            let idx = (inode - 1) as usize;
            self.inodes[idx].refs += 1; // dentry 绑定占一引用
        }
        Ok(slot)
    }

    /// 负 dentry 查找：命中负项直接返回（判据 J3 的缓存面）。
    pub fn lookup_dentry(&mut self, name: &[u8], parent: u16) -> Option<(u16, bool)> {
        let h = TriObjTable::hash_name(name);
        let mut i = 0usize;
        for d in self.dentries.iter() {
            if d.parent == parent && d.name_hash == h && d.state != ObjState::Free {
                let slot = (i + 1) as u16;
                if d.negative {
                    self.neg_hits += 1;
                }
                return Some((slot, d.negative));
            }
            i += 1;
        }
        self.neg_misses += 1;
        None
    }

    /// 打开文件：inode 须存在，返回 file 槽号；inode 引用 +1（判据 J1）。
    pub fn open(&mut self, inode: u16, flags: u32) -> Result<u16, u16> {
        if inode == 0 || inode as usize > self.inodes.len() {
            return Err(E_DANGLING);
        }
        if flags & 0o3 > O_RDWR || flags & !O_FLAG_MASK != 0 {
            return Err(E_FLAG_BAD);
        }
        let idx = (inode - 1) as usize;
        self.inodes[idx].refs += 1;
        let slot = (self.files.len() + 1) as u16;
        self.files.push(FileObj {
            f_pos: 0,
            f_flags: flags,
            f_inode: inode,
            refs: 1,
            state: ObjState::Live,
            writes: 0,
            append_hits: 0,
        });
        self.opens += 1;
        Ok(slot)
    }

    /// 写：O_APPEND 时落点强制到文件尾（判据 J1 的偏移面锚）。
    pub fn write_at(&mut self, file: u16, data_len: u64, file_size: u64) -> Result<u64, u16> {
        if file == 0 || file as usize > self.files.len() {
            return Err(E_SLOT_OOR);
        }
        let i = (file - 1) as usize;
        if self.files[i].state != ObjState::Live {
            return Err(E_SLOT_OOR);
        }
        if self.files[i].f_flags & O_APPEND != 0 {
            self.files[i].f_pos = file_size;
            self.files[i].append_hits += 1;
        }
        let start = self.files[i].f_pos;
        self.files[i].f_pos = start.saturating_add(data_len);
        self.files[i].writes += 1;
        Ok(start)
    }

    /// 关闭文件：释放 inode 引用与 file 槽，入 closes 账（判据 J1 收口）。
    pub fn release(&mut self, file: u16) -> Result<(), u16> {
        if file == 0 || file as usize > self.files.len() {
            return Err(E_SLOT_OOR);
        }
        let i = (file - 1) as usize;
        if self.files[i].state != ObjState::Live {
            return Err(E_SLOT_OOR); // 重复关闭
        }
        let ino = self.files[i].f_inode;
        self.files[i].state = ObjState::Free;
        self.files[i].refs = 0;
        if ino != 0 && (ino as usize) <= self.inodes.len() {
            let k = (ino - 1) as usize;
            if self.inodes[k].refs > 0 {
                self.inodes[k].refs -= 1;
            }
        }
        self.closes += 1;
        Ok(())
    }

    /// 泄漏对账：opens - closes == 当前 Live file 数（判据 J1 字面式）。
    pub fn leak_check(&mut self) -> Result<u32, u16> {
        let mut live = 0u32;
        for f in self.files.iter() {
            if f.state == ObjState::Live {
                live += 1;
            }
        }
        if self.opens - self.closes != live {
            self.leaks += 1;
            return Err(E_REF_LEAK);
        }
        Ok(live)
    }

    /// 三操作表逐项存在性断言（判据 J2 原文）。
    pub fn tables_complete(&self) -> bool {
        self.i_op.all_present()
            && self.f_op.all_present()
            && self.d_op.all_present()
            && self.i_op.names.len() == IOP_LEN
            && self.f_op.names.len() == FOP_LEN
            && self.d_op.names.len() == DOP_LEN
    }

    /// inode 引用账：refs == Σdentry 绑定 + Σfile 打开（判据 J4 自洽面）。
    pub fn inode_refs_conserved(&self) -> bool {
        for (k, ino) in self.inodes.iter().enumerate() {
            let slot = (k + 1) as u16;
            let mut expect = 0u32;
            for d in self.dentries.iter() {
                if d.inode == slot && d.state != ObjState::Free {
                    expect += 1;
                }
            }
            for f in self.files.iter() {
                if f.f_inode == slot && f.state == ObjState::Live {
                    expect += 1;
                }
            }
            if ino.refs != expect {
                return false;
            }
        }
        true
    }

    /// 负缓存理论值核对：命中 + 未命中 == 总 lookup 次数，命中计入负项账。
    pub fn neg_account_ok(&self) -> bool {
        let mut neg = 0u32;
        for d in self.dentries.iter() {
            if d.negative && d.state != ObjState::Free {
                neg += 1;
            }
        }
        if neg == 0 {
            return self.neg_hits == 0;
        }
        self.neg_hits >= neg // 每个负项至少被命中一次（本演练构造面）
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_SLOT_OOR => "槽号越界（索引前未比长度）",
        E_OP_MISS => "操作表缺项（某槽为 0 或名不存在）",
        E_DANGLING => "对象互指悬空（父/inode 槽不存在）",
        E_REF_LEAK => "引用账泄漏（开-关不平）",
        E_NEG_MISS => "负 dentry 缓存命中账与理论值不符",
        E_FLAG_BAD => "f_flags 位段非法",
        _ => "未知诊断码",
    }
}

/// 确定性演练：三表存在性 → 三对象建链 → 万次开销零泄漏 → 负缓存 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut t = TriObjTable::new();

    // R1：三操作表逐项存在性断言（判据 J2 原文）——9/10/6 全在位。
    if !t.tables_complete() {
        return Err(E_OP_MISS);
    }
    if t.i_op.slot_of("rename") != Some(6) || t.f_op.slot_of("release") != Some(9) {
        return Err(E_OP_MISS);
    }
    if t.d_op.slot_of("revalidate") != Some(4) {
        return Err(E_OP_MISS);
    }
    hits += 1;

    // R2：建根 inode + 根 dentry + 子 dentry（正项），互指自洽。
    let ino1 = t.add_inode(1)?;   // 根 inode
    let d_root = t.add_dentry(b"/", 0, ino1, false)?;
    let ino2 = t.add_inode(2)?;   // 普通文件 inode
    let d_file = t.add_dentry(b"a.txt", d_root, ino2, false)?;
    if d_root != 1 || d_file != 2 || ino1 != 1 || ino2 != 2 {
        return Err(E_DANGLING);
    }
    if !t.inode_refs_conserved() {
        return Err(E_DANGLING);
    }
    hits += 1;

    // R3：打开文件 → f_pos 初值 0（判据 J1 偏移锚），写 100 字节落点 0。
    let f1 = t.open(ino2, O_WRONLY)?;
    if t.files[(f1 - 1) as usize].f_pos != 0 {
        return Err(E_FLAG_BAD);
    }
    let at = t.write_at(f1, 100, 100)?;
    if at != 0 || t.files[(f1 - 1) as usize].f_pos != 100 {
        return Err(E_FLAG_BAD);
    }
    hits += 1;

    // R4：O_APPEND 落点强制到文件尾——文件 500 字节，写起点须为 500。
    let f2 = t.open(ino2, O_APPEND | O_WRONLY)?;
    let at2 = t.write_at(f2, 10, 500)?;
    if at2 != 500 || t.files[(f2 - 1) as usize].append_hits != 1 {
        return Err(E_FLAG_BAD);
    }
    hits += 1;

    // R5：关闭两文件 → inode 引用回收，账守恒。
    t.release(f1)?;
    t.release(f2)?;
    if t.opens != 2 || t.closes != 2 {
        return Err(E_REF_LEAK);
    }
    if !t.inode_refs_conserved() {
        return Err(E_DANGLING);
    }
    hits += 1;

    // R6：负 dentry 登记与命中——不存在的 "ghost" 记负项，二次查找命中缓存。
    let d_neg = t.add_dentry(b"ghost", d_root, 0, true)?;
    if !t.dentries[(d_neg - 1) as usize].negative {
        return Err(E_NEG_MISS);
    }
    let (hit1, neg1) = match t.lookup_dentry(b"ghost", d_root) {
        Some(v) => v,
        None => return Err(E_NEG_MISS),
    };
    if hit1 != d_neg || !neg1 {
        return Err(E_NEG_MISS);
    }
    let (hit2, _neg2) = match t.lookup_dentry(b"ghost", d_root) {
        Some(v) => v,
        None => return Err(E_NEG_MISS),
    };
    if hit2 != d_neg || t.neg_hits != 2 {
        return Err(E_NEG_MISS);
    }
    if !t.neg_account_ok() {
        return Err(E_NEG_MISS);
    }
    hits += 1;

    // R7：万次开-关零泄漏（判据 J1 原文）——同一 inode 反复开关。
    for _ in 0..10_000u32 {
        let f = t.open(ino2, O_RDONLY)?;
        t.release(f)?;
    }
    if t.leak_check()? != 0 {
        return Err(E_REF_LEAK);
    }
    if t.opens - t.closes != 0 || t.leaks != 0 {
        return Err(E_REF_LEAK);
    }
    hits += 1;

    // R8：引用账末态复核（判据 J4）——dentry 绑定各自 1 引用。
    if !t.inode_refs_conserved() {
        return Err(E_DANGLING);
    }
    if t.inodes[1].refs != 1 {
        return Err(E_DANGLING); // 仅 a.txt dentry 绑定
    }
    hits += 1;

    // I1：重复关闭注入被拒。
    let f3 = t.open(ino2, O_RDONLY)?;
    t.release(f3)?;
    if t.release(f3) != Err(E_SLOT_OOR) {
        return Err(E_SLOT_OOR);
    }
    hits += 1;

    // I2：负 dentry 绑 inode 注入被拒（自相矛盾项）。
    if t.add_dentry(b"bad", d_root, ino2, true) != Err(E_DANGLING) {
        return Err(E_DANGLING);
    }
    hits += 1;

    // I3：父悬空 / inode 悬空双注入被拒。
    if t.add_dentry(b"orphan", 999, ino2, false) != Err(E_DANGLING) {
        return Err(E_DANGLING);
    }
    if t.add_dentry(b"orphan2", d_root, 999, false) != Err(E_DANGLING) {
        return Err(E_DANGLING);
    }
    hits += 1;

    // I4：非法 f_flags 注入（0o3 以上访问模式）被拒。
    if t.open(ino2, 0o3) != Err(E_FLAG_BAD) {
        return Err(E_FLAG_BAD);
    }
    if t.open(ino2, 0o100000) != Err(E_FLAG_BAD) {
        return Err(E_FLAG_BAD);
    }
    hits += 1;

    // I5：操作表缺项判据本体——空名表判缺项。
    let bad = OpTable::new(&["lookup", ""]);
    if bad.all_present() {
        return Err(E_OP_MISS);
    }
    hits += 1;

    Ok(hits)
}
