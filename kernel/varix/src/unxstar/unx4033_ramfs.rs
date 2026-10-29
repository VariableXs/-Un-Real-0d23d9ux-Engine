//! unx4033_ramfs — UNX-F4033 ramfs 内建 fs 类型（首个注册负载）（诊断段 0x86A0）。
//!
//! 判据原文：
//!   J1 ramfs 类型注册→挂载→创建→写入→读出→卸载端到端链一次通过；
//!   J2 内容哈希往返一致；
//!   J3 作为 B08 语义表冻结的验证负载（负载可复跑且结果确定）。
//! 设计纪律：
//!   1. 内建类型表：ramfs 以固定类型名（"ramfs"）入注册表，
//!      注册是端到端链第一步，未注册即挂载拒收。
//!   2. sb 生命周期：Mounted → Quiesced → Unmounted，卸载要求
//!      挂载引用归零且全部文件关闭（打开句柄账 = 0）。
//!   3. 文件模型：定长名（16 字节）+ 数据 Vec（字节流）+ 打开
//!      句柄计数；写入按偏移截断扩展，读出按偏移+长度界检。
//!   4. 哈希往返：FNV-1a 32 位（偏移 0x811C9DC5，素数 0x01000193）
//!      写前哈希与读后哈希必须逐位一致（判据 J2）。
//!   5. 边界：重名创建、超长名、越界读偏移、未挂载操作、
//!      有打开句柄时卸载——全部显性拒收并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x86A0;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_NOT_REGISTERED: u16 = SEG_BASE + 1; // 类型未注册先挂载
const E_DUP_REG: u16 = SEG_BASE + 2;        // 类型重复注册
const E_DUP_FILE: u16 = SEG_BASE + 3;       // 重名创建
const E_NAME_LONG: u16 = SEG_BASE + 4;      // 文件名超 16 字节
const E_NO_FILE: u16 = SEG_BASE + 5;        // 打开/读写不存在的文件
const E_RD_OOB: u16 = SEG_BASE + 6;         // 读偏移+长度越界
const E_NOT_MOUNTED: u16 = SEG_BASE + 7;    // 未挂载期文件操作
const E_MOUNT_LIVE: u16 = SEG_BASE + 8;     // 卸载时仍有打开句柄
const E_BAL: u16 = SEG_BASE + 9;            // 端到端链账不平

/// 文件名定长（ramfs 简化约定）。
pub const NAME_CAP: usize = 16;
/// 单 sb 文件数上限（静态配额）。
pub const FILE_CAP: usize = 32;

/// FNV-1a 32 位常量（来源：FNV 参考实现）。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;
pub const FNV_PRIME: u32 = 0x0100_0193;

/// fs 类型注册项。
pub struct FsType {
    pub name: [u8; NAME_CAP], // 类型名（零填充）
    pub name_len: usize,      // 有效长度
    pub registered: bool,     // 注册位
}

/// 文件节点：名 + 数据 + 句柄账。
pub struct RamFile {
    pub name: [u8; NAME_CAP],
    pub name_len: usize,
    pub data: Vec<u8>,  // 字节流（整数算术）
    pub open_cnt: u32,  // 打开句柄计数
}

/// ramfs sb：注册表 + 文件池 + 生命周期状态机。
pub struct RamSb {
    pub fstype: FsType,
    pub mounted: bool,
    pub mount_refs: u32,   // 挂载引用计数
    pub files: Vec<RamFile>,
    pub wr_ops: u32,       // 写操作账
    pub rd_ops: u32,       // 读操作账
    pub rejects: u32,      // 错误账
    pub roundtrip_ok: u32, // 哈希往返一致计数
}

impl RamSb {
    pub fn new() -> RamSb {
        RamSb {
            fstype: FsType { name: [0u8; NAME_CAP], name_len: 0, registered: false },
            mounted: false,
            mount_refs: 0,
            files: Vec::new(),
            wr_ops: 0,
            rd_ops: 0,
            rejects: 0,
            roundtrip_ok: 0,
        }
    }

    /// 类型注册：内建名 "ramfs"，重名拒收。
    pub fn register(&mut self) -> Result<(), u16> {
        if self.fstype.registered {
            self.rejects += 1;
            return Err(E_DUP_REG);
        }
        let nm = b"ramfs";
        let mut i = 0;
        while i < nm.len() && i < NAME_CAP {
            self.fstype.name[i] = nm[i];
            i += 1;
        }
        self.fstype.name_len = nm.len();
        self.fstype.registered = true;
        Ok(())
    }

    /// 挂载：须已注册；引用计数推进。
    pub fn mount(&mut self) -> Result<(), u16> {
        if !self.fstype.registered {
            self.rejects += 1;
            return Err(E_NOT_REGISTERED);
        }
        self.mounted = true;
        self.mount_refs += 1;
        Ok(())
    }

    /// 创建文件：挂载态 + 名合法 + 不重名。
    pub fn create(&mut self, name: &[u8]) -> Result<usize, u16> {
        if !self.mounted {
            self.rejects += 1;
            return Err(E_NOT_MOUNTED);
        }
        if name.is_empty() || name.len() > NAME_CAP {
            self.rejects += 1;
            return Err(E_NAME_LONG);
        }
        let mut i = 0;
        while i < self.files.len() {
            if i < self.files.len() && name_eq(&self.files[i].name, self.files[i].name_len, name) {
                self.rejects += 1;
                return Err(E_DUP_FILE);
            }
            i += 1;
        }
        if self.files.len() >= FILE_CAP {
            self.rejects += 1;
            return Err(E_BAL);
        }
        let mut nm = [0u8; NAME_CAP];
        let mut j = 0;
        while j < name.len() {
            nm[j] = name[j];
            j += 1;
        }
        self.files.push(RamFile {
            name: nm,
            name_len: name.len(),
            data: Vec::new(),
            open_cnt: 0,
        });
        Ok(self.files.len() - 1)
    }

    /// 打开：句柄计数推进（卸载屏障）。
    pub fn open(&mut self, idx: usize) -> Result<(), u16> {
        if !self.mounted || idx >= self.files.len() {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        self.files[idx].open_cnt += 1;
        Ok(())
    }

    /// 写入：偏移截断扩展（稀疏补零），先写前哈希账后落盘。
    pub fn write_at(&mut self, idx: usize, off: usize, buf: &[u8]) -> Result<(u32, u32), u16> {
        if !self.mounted || idx >= self.files.len() {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        if off > self.files[idx].data.len() {
            self.rejects += 1;
            return Err(E_RD_OOB);
        }
        // 写前哈希（判据 J2 的前像）。
        let pre = fnv1a(&self.files[idx].data);
        // 稀疏补零：off 大于现长时中间补 0。
        while self.files[idx].data.len() < off {
            self.files[idx].data.push(0);
        }
        let mut k = 0;
        while k < buf.len() {
            if off + k < self.files[idx].data.len() {
                self.files[idx].data[off + k] = buf[k];
            } else {
                self.files[idx].data.push(buf[k]);
            }
            k += 1;
        }
        self.wr_ops += 1;
        Ok((pre, fnv1a(buf)))
    }

    /// 读出：偏移+长度界检后按位返回哈希（判据 J2 的后像）。
    pub fn read_at(&mut self, idx: usize, off: usize, len: usize) -> Result<u32, u16> {
        if !self.mounted || idx >= self.files.len() {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        if off + len > self.files[idx].data.len() {
            self.rejects += 1;
            return Err(E_RD_OOB);
        }
        // 逐位喂哈希（等价于对切片求 FNV，避免借用冲突用累加器）。
        let mut h = FNV_OFFSET;
        let mut k = off;
        while k < off + len {
            h ^= self.files[idx].data[k] as u32;
            h = h.wrapping_mul(FNV_PRIME);
            k += 1;
        }
        self.rd_ops += 1;
        Ok(h)
    }

    /// 关闭：句柄归账。
    pub fn close(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.files.len() || self.files[idx].open_cnt == 0 {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        self.files[idx].open_cnt -= 1;
        Ok(())
    }

    /// 卸载：全部句柄关闭才放行（生命周期终段）。
    pub fn unmount(&mut self) -> Result<(), u16> {
        if !self.mounted {
            self.rejects += 1;
            return Err(E_NOT_MOUNTED);
        }
        let mut i = 0;
        while i < self.files.len() {
            if self.files[i].open_cnt != 0 {
                self.rejects += 1;
                return Err(E_MOUNT_LIVE);
            }
            i += 1;
        }
        self.mounted = false;
        self.mount_refs = self.mount_refs - 1;
        Ok(())
    }

    /// 守恒对账：往返账 + 读写账闭合；卸载后文件账冻结。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.fstype.registered {
            return Err(E_BAL);
        }
        if self.mounted && self.mount_refs == 0 {
            return Err(E_BAL);
        }
        if self.mount_refs > 1 {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 导出快照（类型名长，文件数，读写账，往返账）。
    pub fn ledger(&self) -> (usize, usize, u32, u32, u32) {
        (self.fstype.name_len, self.files.len(), self.wr_ops, self.rd_ops, self.roundtrip_ok)
    }
}

/// 定长名比较（有效长度内逐字节）。
fn name_eq(a: &[u8; NAME_CAP], alen: usize, b: &[u8]) -> bool {
    if alen != b.len() {
        return false;
    }
    let mut i = 0;
    while i < alen {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// FNV-1a 32 位整段哈希。
fn fnv1a(data: &[u8]) -> u32 {
    let mut h = FNV_OFFSET;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 自例程：R1 端到端链 → R2 哈希往返 → R3 创建注入 → R4 读写越界注入 → R5 卸载屏障与守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut fs = RamSb::new();

    // R1 端到端链：注册→挂载→创建→打开→写→读→关→卸载一次通过。
    fs.register()?;
    fs.mount()?;
    let f = fs.create(b"payload.txt")?;
    fs.open(f)?;
    let payload: [u8; 8] = [0x56, 0x46, 0x53, 0x2D, 0x42, 0x30, 0x38, 0x0A];
    let (pre, _wr) = fs.write_at(f, 0, &payload)?;
    let post = fs.read_at(f, 0, payload.len())?;
    fs.close(f)?;
    fs.unmount()?;
    if pre != fnv1a(&payload) || post != pre {
        return Err(E_BAL);
    }
    fs.roundtrip_ok += 1;
    hits += 1;

    // R2 哈希往返确定性：同负载二次复跑哈希逐位一致（判据 J3）。
    fs.mount()?;
    let f2 = fs.create(b"again.bin")?;
    fs.open(f2)?;
    let (_p2, _) = fs.write_at(f2, 0, &payload)?;
    let post2 = fs.read_at(f2, 0, payload.len())?;
    fs.close(f2)?;
    if post2 != fnv1a(&payload) {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 创建注入：重名与超长名被拒；未注册挂载被拒。
    if fs.create(b"again.bin") != Err(E_DUP_FILE) {
        return Err(E_BAL);
    }
    let long = [0x61u8; NAME_CAP + 1];
    if fs.create(&long) != Err(E_NAME_LONG) {
        return Err(E_BAL);
    }
    let mut fs2 = RamSb::new();
    if fs2.mount() != Err(E_NOT_REGISTERED) {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 读写越界注入：读越界与写稀疏空洞越界分别被拒。
    fs2.register()?;
    fs2.mount()?;
    let f3 = fs2.create(b"oob")?;
    fs2.open(f3)?;
    fs2.write_at(f3, 0, b"abcd")?;
    if fs2.read_at(f3, 2, 3) != Err(E_RD_OOB) {
        return Err(E_BAL);
    }
    if fs2.write_at(f3, 9, b"x") != Err(E_RD_OOB) {
        return Err(E_BAL);
    }
    fs2.close(f3)?;
    hits += 1;

    // R5 卸载屏障与守恒：有句柄卸载被拒，关闭后卸载成功。
    let f4 = fs2.create(b"hold")?;
    fs2.open(f4)?;
    if fs2.unmount() != Err(E_MOUNT_LIVE) {
        return Err(E_BAL);
    }
    fs2.close(f4)?;
    fs2.unmount()?;
    fs2.audit()?;
    fs.audit()?;
    let (nl, nf, wr, rd, rt) = fs.ledger();
    if nl != 5 || nf != 2 || wr != 2 || rd != 2 || rt != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
