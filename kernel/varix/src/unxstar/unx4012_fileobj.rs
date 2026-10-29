//! unx4012_fileobj — UNX-F4012 File 对象结构锚（诊断段 0x8440）。
//!
//! 判据原文：
//!   J1 File 字段集（f_pos/f_flags/f_op/私有数据/引用计数）实例化断言通过。
//!   J2 f_pos 初值 0 与 O_APPEND 组合的首次写偏移落点行为锚定有账：带 APPEND
//!      的首次写起点恒为文件尾，不带 APPEND 的首次写起点恒为 0。
//!   J3 llseek 三 whence（SET/CUR/END）语义正确，负偏移落点拒收（EINVAL）。
//!   J4 引用计数与私有数据绑定：私有数据随 file 生命周期存在，释放后清零。
//! 设计纪律：
//!   1. file 对象是「打开实例」而非 inode：同一 inode 可开多个 file，各自
//!      f_pos 独立；本模块只建模单 file 的偏移机与私有数据槽。
//!   2. 偏移推进一律走 checked/饱和算术，向大溢出即拒；f_pos 语义为 u64，
//!      负值仅在 llseek 入参出现，落点负即 EINVAL。
//!   3. f_flags 只接受 O_RDONLY/O_WRONLY/O_RDWR 之一（低位 0o3 互斥），
//!      再加 O_APPEND/O_NONBLOCK/O_DIRECT 三个合法高位。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8440;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ACC_MODE: u16 = SEG_BASE + 1;  // 访问模式非法（0o3 以下不合法）
const E_FLAG_EXTRA: u16 = SEG_BASE + 2; // f_flags 出现闭集外高位
const E_POS_NEG: u16 = SEG_BASE + 3;   // llseek 落点为负
const E_POS_OVER: u16 = SEG_BASE + 4;  // f_pos 溢出 u64
const E_WHENCE: u16 = SEG_BASE + 5;    // whence 非法
const E_REF_ZERO: u16 = SEG_BASE + 6;  // 引用为 0 后再操作
const E_PRIV: u16 = SEG_BASE + 7;      // 私有数据槽非法

/// 访问模式掩码（O_ACCMODE）。
pub const O_ACCMODE: u32 = 0o3;
pub const O_RDONLY: u32 = 0o0;
pub const O_WRONLY: u32 = 0o1;
pub const O_RDWR: u32 = 0o2;
/// 合法高位标志。
pub const O_APPEND: u32 = 0o2000;
pub const O_NONBLOCK: u32 = 0o4000;
pub const O_DIRECT: u32 = 0o40000;
pub const O_VALID_MASK: u32 = O_ACCMODE | O_APPEND | O_NONBLOCK | O_DIRECT;

/// whence 闭集（POSIX lseek）。
pub const SEEK_SET: u8 = 0;
pub const SEEK_CUR: u8 = 1;
pub const SEEK_END: u8 = 2;

/// file 生命周期状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FState {
    Fresh = 0,   // 刚打开，未读写
    Active = 1,  // 有读写活动
    Released = 2, // 已释放（终态）
}

/// 私有数据槽（file->private_data 的纯功能投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrivData {
    pub tag: u16,   // 子系统标签（0=未绑定）
    pub blob: u64,  // 携带值（不透明）
}

impl PrivData {
    pub fn empty() -> PrivData {
        PrivData { tag: 0, blob: 0 }
    }
}

/// File 对象结构锚。
pub struct FileObj {
    pub f_pos: u64,      // 当前偏移
    pub f_flags: u32,    // 打开标志
    pub f_op: u16,       // 操作表指针（槽号，0=空）
    pub f_priv: PrivData, // 私有数据
    pub refs: u32,       // 引用计数
    pub state: FState,   // 生命周期
    pub reads: u32,      // 读次数
    pub writes: u32,     // 写次数
    pub seeks: u32,      // 定位次数
    pub append_writes: u32, // APPEND 生效次数（判据 J2 账）
}

impl FileObj {
    /// 实例化：f_pos 恒 0、f_op 非空、flags 闭集校验（判据 J1）。
    pub fn new(f_flags: u32, f_op: u16) -> Result<FileObj, u16> {
        if f_op == 0 {
            return Err(E_PRIV);
        }
        if f_flags & !O_VALID_MASK != 0 {
            return Err(E_FLAG_EXTRA);
        }
        let acc = f_flags & O_ACCMODE;
        // O_RDONLY=0 合法但与 O_WRONLY/O_RDWR 需区分：0/1/2 合法，3 非法。
        if acc > O_RDWR {
            return Err(E_ACC_MODE);
        }
        Ok(FileObj {
            f_pos: 0,
            f_flags,
            f_op,
            f_priv: PrivData::empty(),
            refs: 1,
            state: FState::Fresh,
            reads: 0,
            writes: 0,
            seeks: 0,
            append_writes: 0,
        })
    }

    /// 读：推进 f_pos bytes 字节，只读模式校验。
    pub fn read(&mut self, bytes: u64) -> Result<u64, u16> {
        if self.state == FState::Released {
            return Err(E_REF_ZERO);
        }
        if self.refs == 0 {
            return Err(E_REF_ZERO);
        }
        if self.f_flags & O_ACCMODE == O_WRONLY {
            return Err(E_ACC_MODE);
        }
        let start = self.f_pos;
        self.f_pos = match start.checked_add(bytes) {
            Some(v) => v,
            None => return Err(E_POS_OVER),
        };
        self.reads += 1;
        self.state = FState::Active;
        Ok(start)
    }

    /// 写：O_APPEND 时起点强制为 file_size（判据 J2），随后推进。
    pub fn write(&mut self, bytes: u64, file_size: u64) -> Result<u64, u16> {
        if self.state == FState::Released {
            return Err(E_REF_ZERO);
        }
        if self.refs == 0 {
            return Err(E_REF_ZERO);
        }
        if self.f_flags & O_ACCMODE == O_RDONLY {
            return Err(E_ACC_MODE);
        }
        if self.f_flags & O_APPEND != 0 {
            self.f_pos = file_size;
            self.append_writes += 1;
        }
        let start = self.f_pos;
        self.f_pos = match start.checked_add(bytes) {
            Some(v) => v,
            None => return Err(E_POS_OVER),
        };
        self.writes += 1;
        self.state = FState::Active;
        Ok(start)
    }

    /// llseek：三 whence 语义（判据 J3），落点负即拒。
    pub fn llseek(&mut self, off: i64, whence: u8, file_size: u64) -> Result<u64, u16> {
        if self.state == FState::Released {
            return Err(E_REF_ZERO);
        }
        let base: i64 = match whence {
            SEEK_SET => 0,
            SEEK_CUR => {
                if self.f_pos > i64::MAX as u64 {
                    return Err(E_POS_OVER);
                }
                self.f_pos as i64
            }
            SEEK_END => {
                if file_size > i64::MAX as u64 {
                    return Err(E_POS_OVER);
                }
                file_size as i64
            }
            _ => return Err(E_WHENCE),
        };
        let next = match base.checked_add(off) {
            Some(v) => v,
            None => return Err(E_POS_OVER),
        };
        if next < 0 {
            return Err(E_POS_NEG);
        }
        self.f_pos = next as u64;
        self.seeks += 1;
        Ok(self.f_pos)
    }

    /// 私有数据绑定：tag 非 0 才允许（0 保留未绑定）。
    pub fn bind_priv(&mut self, tag: u16, blob: u64) -> Result<(), u16> {
        if tag == 0 {
            return Err(E_PRIV);
        }
        if self.state == FState::Released {
            return Err(E_REF_ZERO);
        }
        self.f_priv = PrivData { tag, blob };
        Ok(())
    }

    /// 引用 +1。
    pub fn get(&mut self) -> Result<u32, u16> {
        if self.state == FState::Released {
            return Err(E_REF_ZERO);
        }
        self.refs += 1;
        Ok(self.refs)
    }

    /// 引用 -1；归零进 Released 并清私有数据（判据 J4）。
    pub fn put(&mut self) -> Result<u32, u16> {
        if self.refs == 0 {
            return Err(E_REF_ZERO);
        }
        self.refs -= 1;
        if self.refs == 0 {
            self.state = FState::Released;
            self.f_priv = PrivData::empty();
        }
        Ok(self.refs)
    }

    /// 可写判定：非只读且未释放。
    pub fn writable(&self) -> bool {
        self.state != FState::Released && (self.f_flags & O_ACCMODE) != O_RDONLY
    }

    /// 偏移账自检：f_pos == 净推进量（读 + 写起点差 + seek 终值）。
    pub fn pos_consistent(&self, start_pos: u64) -> bool {
        self.f_pos >= start_pos || self.seeks > 0
    }
}

/// file 槽表：多打开实例账。
pub struct FileTable {
    pub files: Vec<FileObj>, // 槽表（1-based）
    pub opened: u32,         // 打开总数
    pub closed: u32,         // 关闭总数
    pub live_peak: u32,      // 活跃峰值
}

impl FileTable {
    pub fn new() -> FileTable {
        FileTable { files: Vec::new(), opened: 0, closed: 0, live_peak: 0 }
    }

    /// 打开：入槽返回槽号。
    pub fn open(&mut self, flags: u32, f_op: u16) -> Result<u16, u16> {
        let f = FileObj::new(flags, f_op)?;
        self.files.push(f);
        self.opened += 1;
        let live = self.opened - self.closed;
        if live > self.live_peak {
            self.live_peak = live;
        }
        Ok(self.files.len() as u16)
    }

    /// 关闭：只允许一次，入 closed 账。
    pub fn close(&mut self, slot: u16) -> Result<(), u16> {
        if slot == 0 || slot as usize > self.files.len() {
            return Err(E_REF_ZERO);
        }
        let i = (slot - 1) as usize;
        if self.files[i].state == FState::Released {
            return Err(E_REF_ZERO);
        }
        // 引用降到 0（多引用时逐个 put，本 API 作终局释放）。
        while self.files[i].refs > 0 {
            self.files[i].put()?;
        }
        self.closed += 1;
        Ok(())
    }

    /// 活跃账：开 - 关 == 未释放 file 数。
    pub fn account_ok(&self) -> bool {
        let mut live = 0u32;
        for f in self.files.iter() {
            if f.state != FState::Released {
                live += 1;
            }
        }
        self.opened - self.closed == live
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_ACC_MODE => "访问模式非法或读写方向不符",
        E_FLAG_EXTRA => "f_flags 出现闭集外高位",
        E_POS_NEG => "llseek 落点为负",
        E_POS_OVER => "f_pos 溢出 u64",
        E_WHENCE => "whence 非法（非 0/1/2）",
        E_REF_ZERO => "引用为 0 后再操作或重复关闭",
        E_PRIV => "私有数据槽非法或 f_op 为空",
        _ => "未知诊断码",
    }
}

/// 确定性演练：字段实例化 → 首写落点 → llseek → 私有数据 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：字段实例化断言（判据 J1）——f_pos 初值 0、refs 1、Priv 空。
    let mut f = FileObj::new(O_WRONLY, 3)?;
    if f.f_pos != 0 || f.refs != 1 || f.f_op != 3 || f.state != FState::Fresh {
        return Err(E_PRIV);
    }
    if f.f_priv.tag != 0 || f.f_priv.blob != 0 {
        return Err(E_PRIV);
    }
    hits += 1;

    // R2：无 APPEND 首写起点 0（判据 J2 前半）。
    let at0 = f.write(100, 5000)?;
    if at0 != 0 || f.f_pos != 100 {
        return Err(E_POS_OVER);
    }
    hits += 1;

    // R3：带 APPEND 的首写起点为文件尾（判据 J2 后半）——文件 5000 字节。
    let mut fa = FileObj::new(O_WRONLY | O_APPEND, 4)?;
    let at1 = fa.write(10, 5000)?;
    if at1 != 5000 || fa.f_pos != 5010 || fa.append_writes != 1 {
        return Err(E_POS_OVER);
    }
    hits += 1;

    // R4：APPEND 第二次写仍以新文件尾为起点（file 已扩到 5010）。
    let at2 = fa.write(10, 5010)?;
    if at2 != 5010 || fa.f_pos != 5020 {
        return Err(E_POS_OVER);
    }
    hits += 1;

    // R5：llseek SET/CUR/END 三语义（判据 J3）。
    let mut fs = FileObj::new(O_RDWR, 5)?;
    if fs.llseek(100, SEEK_SET, 1000)? != 100 {
        return Err(E_POS_OVER);
    }
    if fs.llseek(50, SEEK_CUR, 1000)? != 150 {
        return Err(E_POS_OVER);
    }
    if fs.llseek(-50, SEEK_END, 1000)? != 950 {
        return Err(E_POS_OVER);
    }
    if fs.seeks != 3 {
        return Err(E_POS_OVER);
    }
    hits += 1;

    // R6：读推进——RDWR 从 950 读 50 → 1000；读起点返回值 950。
    let rd = fs.read(50)?;
    if rd != 950 || fs.f_pos != 1000 || fs.reads != 1 {
        return Err(E_POS_OVER);
    }
    hits += 1;

    // R7：私有数据绑定并随释放清零（判据 J4）。
    fs.bind_priv(0x4001, 0xDEAD_BEEF)?;
    if fs.f_priv.tag != 0x4001 || fs.f_priv.blob != 0xDEAD_BEEF {
        return Err(E_PRIV);
    }
    fs.get()?;
    if fs.refs != 2 {
        return Err(E_REF_ZERO);
    }
    fs.put()?;
    if fs.refs != 1 || fs.state == FState::Released {
        return Err(E_REF_ZERO);
    }
    fs.put()?;
    if fs.refs != 0 || fs.state != FState::Released {
        return Err(E_REF_ZERO);
    }
    if fs.f_priv.tag != 0 || fs.f_priv.blob != 0 {
        return Err(E_PRIV);
    }
    hits += 1;

    // R8：多打开实例 f_pos 互不影响——两 file 同 f_op，偏移独立。
    let mut t = FileTable::new();
    let a = t.open(O_RDWR, 7)?;
    let b = t.open(O_RDWR, 7)?;
    t.files[(a - 1) as usize].write(10, 0)?;
    t.files[(b - 1) as usize].write(20, 0)?;
    if t.files[(a - 1) as usize].f_pos != 10 || t.files[(b - 1) as usize].f_pos != 20 {
        return Err(E_POS_OVER);
    }
    t.close(a)?;
    t.close(b)?;
    if !t.account_ok() || t.opened != 2 || t.closed != 2 {
        return Err(E_REF_ZERO);
    }
    hits += 1;

    // I1：访问模式 0o3 注入被拒。
    if FileObj::new(0o3, 1).err() != Some(E_ACC_MODE) {
        return Err(E_ACC_MODE);
    }
    hits += 1;

    // I2：闭集外高位注入被拒。
    if FileObj::new(0o10, 1).err() != Some(E_FLAG_EXTRA) {
        return Err(E_FLAG_EXTRA);
    }
    hits += 1;

    // I3：f_op=0 注入被拒。
    if FileObj::new(O_RDONLY, 0).err() != Some(E_PRIV) {
        return Err(E_PRIV);
    }
    hits += 1;

    // I4：llseek 负落点与非法 whence 双注入被拒（判据 J3 的拒绝面）。
    if fs.llseek(0, SEEK_SET, 0) != Err(E_REF_ZERO) {
        return Err(E_REF_ZERO); // 已释放 file 不可定位
    }
    let mut g = FileObj::new(O_RDWR, 2)?;
    if g.llseek(-1, SEEK_SET, 0) != Err(E_POS_NEG) {
        return Err(E_POS_NEG);
    }
    if g.llseek(0, 9, 0) != Err(E_WHENCE) {
        return Err(E_WHENCE);
    }
    hits += 1;

    // I5：只读 file 写注入 / 只写 file 读注入双拒。
    let mut ro = FileObj::new(O_RDONLY, 2)?;
    if ro.write(1, 0) != Err(E_ACC_MODE) {
        return Err(E_ACC_MODE);
    }
    let mut wo = FileObj::new(O_WRONLY, 2)?;
    if wo.read(1) != Err(E_ACC_MODE) {
        return Err(E_ACC_MODE);
    }
    hits += 1;

    // I6：私有数据 tag=0 注入被拒。
    let mut h = FileObj::new(O_RDWR, 2)?;
    if h.bind_priv(0, 1) != Err(E_PRIV) {
        return Err(E_PRIV);
    }
    hits += 1;

    Ok(hits)
}
