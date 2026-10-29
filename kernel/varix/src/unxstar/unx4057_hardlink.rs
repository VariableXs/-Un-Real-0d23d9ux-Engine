//! unx4057_hardlink — UNX-F4057 硬链接计数一致性锚（诊断段 0x8980）。
//!
//! 判据原文（提炼）：
//!   J1. nlink 值与指向同 inode 的 dentry 实数严格相等（1,000 节点
//!       树全量核对零差）。
//!   J2. 不一致注入被断言器检出并能定位 inode 号。
//!   J3. link/unlink 双向账守恒：nlink 增减与 dentry 增减同步。
//!   J4. nlink 归零（最后一个 dentry 解除）触发回收标记。
//! 设计纪律：
//!   1. POSIX link 语义：硬链接共享 inode，nlink == 指向它的
//!      目录项数，unlink 减一、link 加一、创建含一个初值。
//!   2. 断言器全量核对：按 inode 逐个数 dentry，与 nlink 字段
//!      严格相等比对，差一即红（判据 J1）。
//!   3. 定位能力：不一致注入返回出错 inode 号（错误码低 8 位承载）。
//!   4. 千节点树用确定性整数推导装配，无随机源。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8980;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ENOENT: u16 = SEG_BASE + 1;    // inode/dentry 不存在
const E_DUP_NAME: u16 = SEG_BASE + 2;  // 同目录同名 dentry
const E_LINK_OVER: u16 = SEG_BASE + 3; // nlink 超上限（POSIX LINK_MAX 定编）
const E_INCONSIST: u16 = SEG_BASE + 4; // nlink 与 dentry 实数不一致
const E_ZERO_NLINK: u16 = SEG_BASE + 5;// nlink 为 0 仍被引用（悬空）
const E_BAD_ARG: u16 = SEG_BASE + 6;   // 参数非法（0 号）
const E_LEDGER: u16 = SEG_BASE + 7;    // link/unlink 账守恒破坏

/// nlink 上限定编：32767（ext4 默认 LINK_MAX 语义）。
pub const LINK_MAX: u32 = 32767;

/// 定位承载位：错误码低 8 位携带出错的 inode 序号。
pub const LOC_MASK: u16 = 0x00FF;

/// 节点：inode 号 + nlink 实态 + 回收标记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LinkNode {
    pub ino: u64,        // inode 号
    pub nlink: u32,      // 硬链接计数（真值断言对象）
    pub reaped: bool,    // nlink 归零回收标记（判据 J4）
}

/// 目录项账：指向 inode 的名字条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NameEntry {
    pub dir: u64, // 所在目录 inode 号
    pub ino: u64, // 指向 inode 号
    pub key: u64, // 名字键（整数化名字，确定性序）
}

/// 硬链接一致性枢纽：节点表 + 名字账 + 断言器。
pub struct HardlinkFabric {
    pub nodes: Vec<LinkNode>,   // inode 节点表
    pub names: Vec<NameEntry>,  // 目录项账
    pub cap: usize,             // 节点静态上限
    pub link_ops: u32,          // link 成功数
    pub unlink_ops: u32,        // unlink 成功数
    pub reaped_total: u32,      // 归零回收总数
}

pub const NODE_CAP: usize = 1024;

impl HardlinkFabric {
    pub fn new() -> HardlinkFabric {
        HardlinkFabric {
            nodes: Vec::new(),
            names: Vec::new(),
            cap: NODE_CAP,
            link_ops: 0,
            unlink_ops: 0,
            reaped_total: 0,
        }
    }

    fn find_node(&self, ino: u64) -> Option<usize> {
        for i in 0..self.nodes.len() {
            if self.nodes[i].ino == ino {
                return Some(i);
            }
        }
        None
    }

    /// 创建节点：初值 nlink=1（POSIX 创建即含一个链接）。
    pub fn create_node(&mut self, ino: u64, dir: u64, key: u64) -> Result<(), u16> {
        if ino == 0 {
            return Err(E_BAD_ARG);
        }
        if self.nodes.len() >= self.cap {
            return Err(E_LINK_OVER);
        }
        if self.find_node(ino).is_some() {
            return Err(E_DUP_NAME);
        }
        self.nodes.push(LinkNode { ino, nlink: 1, reaped: false });
        self.names.push(NameEntry { dir, ino, key });
        Ok(())
    }

    /// link：新增一个指向 ino 的目录项，nlink+1（判据 J3 正向）。
    pub fn link(&mut self, ino: u64, dir: u64, key: u64) -> Result<(), u16> {
        let i = match self.find_node(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if self.nodes[i].reaped {
            return Err(E_ZERO_NLINK);
        }
        // 同目录同名拒收
        for e in &self.names {
            if e.dir == dir && e.key == key {
                return Err(E_DUP_NAME);
            }
        }
        if self.nodes[i].nlink >= LINK_MAX {
            return Err(E_LINK_OVER);
        }
        self.names.push(NameEntry { dir, ino, key });
        self.nodes[i].nlink += 1;
        self.link_ops += 1;
        Ok(())
    }

    /// unlink：解除 dir 下 key 指向的链接，nlink-1；归零回收（判据 J3/J4）。
    pub fn unlink(&mut self, dir: u64, key: u64) -> Result<(), u16> {
        let mut pos: Option<usize> = None;
        for i in 0..self.names.len() {
            if self.names[i].dir == dir && self.names[i].key == key {
                pos = Some(i);
                break;
            }
        }
        let p = match pos {
            Some(p) => p,
            None => return Err(E_ENOENT),
        };
        let ino = self.names[p].ino;
        // 目录项先出账
        self.names.remove(p);
        let i = match self.find_node(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if self.nodes[i].nlink == 0 {
            return Err(E_ZERO_NLINK);
        }
        self.nodes[i].nlink -= 1;
        self.unlink_ops += 1;
        // nlink 归零：触发回收标记（判据 J4）
        if self.nodes[i].nlink == 0 {
            self.nodes[i].reaped = true;
            self.reaped_total += 1;
        }
        Ok(())
    }

    /// 数某 inode 的 dentry 实数（断言器基准值）。
    pub fn dentry_count(&self, ino: u64) -> u32 {
        let mut c = 0;
        for e in &self.names {
            if e.ino == ino {
                c += 1;
            }
        }
        c
    }

    /// 断言器：单 inode 核对 nlink == dentry 实数。
    pub fn assert_one(&self, ino: u64) -> Result<(), u16> {
        let i = match self.find_node(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        let d = self.dentry_count(ino);
        if self.nodes[i].nlink != d {
            // 定位：低 8 位承载节点序号（判据 J2）
            return Err(E_INCONSIST | ((i as u16) & LOC_MASK));
        }
        Ok(())
    }

    /// 断言器：全量核对（千节点树，判据 J1）。
    pub fn assert_all(&self) -> Result<u32, u16> {
        for i in 0..self.nodes.len() {
            let ino = self.nodes[i].ino;
            let d = self.dentry_count(ino);
            if self.nodes[i].nlink != d {
                return Err(E_INCONSIST | ((i as u16) & LOC_MASK));
            }
        }
        Ok(self.nodes.len() as u32)
    }

    /// 不一致注入：篡改某节点 nlink 值，验证断言器检出并定位。
    pub fn inject_drift(&mut self, slot: usize, delta: u32) -> Result<u16, u16> {
        if slot >= self.nodes.len() {
            return Err(E_BAD_ARG);
        }
        let ino = self.nodes[slot].ino;
        // 单向漂移（防下溢：只加不减）
        self.nodes[slot].nlink = self.nodes[slot].nlink.wrapping_add(delta);
        match self.assert_one(ino) {
            Err(e) => Ok(e),
            Ok(()) => Err(E_LEDGER), // 不该通过
        }
    }

    /// 复原注入（漂移回滚，供对账后清理）。
    pub fn heal(&mut self, slot: usize) -> Result<(), u16> {
        if slot >= self.nodes.len() {
            return Err(E_BAD_ARG);
        }
        let d = self.dentry_count(self.nodes[slot].ino);
        self.nodes[slot].nlink = d;
        Ok(())
    }

    /// 守恒对账：dentry 总账 == 全节点 nlink 合计。
    pub fn ledger_ok(&self) -> bool {
        let mut sum = 0u32;
        for n in &self.nodes {
            sum += n.nlink;
        }
        sum == self.names.len() as u32
    }
}

/// 千节点树装配：确定性推导（偶数号 2 链，奇数号 1 链）。
pub fn build_thousand(f: &mut HardlinkFabric) -> Result<(), u16> {
    for k in 1..=1000u64 {
        f.create_node(k, 1, k * 10)?;
        if k % 2 == 0 {
            f.link(k, 1, k * 10 + 1)?; // 偶数号再挂一条硬链接
        }
    }
    Ok(())
}

// ── 扩展面：链接查问与目录账诊断 ─────────────────────────────

impl HardlinkFabric {
    /// nlink 只读面（断言器复核前置读数）。
    pub fn nlink_of(&self, ino: u64) -> Result<u32, u16> {
        match self.find_node(ino) {
            Some(i) => Ok(self.nodes[i].nlink),
            None => Err(E_ENOENT),
        }
    }

    /// 目录项计数只读面（dentry 账读数）。
    pub fn dentry_total(&self) -> u32 {
        self.names.len() as u32
    }

    /// 某目录下的链接数（目录侧账，与 inode 侧账交叉复核面）。
    pub fn links_in_dir(&self, dir: u64) -> u32 {
        let mut c = 0;
        for e in &self.names {
            if e.dir == dir {
                c += 1;
            }
        }
        c
    }

    /// 回收只读面：nlink 归零节点数（reaped 标记账读数）。
    pub fn reaped_count(&self) -> u32 {
        let mut c = 0;
        for n in &self.nodes {
            if n.reaped {
                c += 1;
            }
        }
        c
    }

    /// 取还账快照：返回（link 数，unlink 数）二元组（判据 J3 面）。
    pub fn op_ledger(&self) -> (u32, u32) {
        (self.link_ops, self.unlink_ops)
    }
}

/// 自例程：千节点全量核对→注入定位→归零回收→守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut f = HardlinkFabric::new();

    // R1 千节点装配与全量核对零差（判据 J1）
    build_thousand(&mut f)?;
    let checked = f.assert_all()?;
    if checked != 1000 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2 不一致注入：断言器检出且低 8 位定位节点序号
    let mark = f.inject_drift(5, 1)?; // 6 号节点（slot5）多记 1
    if mark & 0x8000 == 0 && mark & !LOC_MASK == 0 {
        return Err(E_INCONSIST);
    }
    if (mark & LOC_MASK) as usize != 5 {
        return Err(E_INCONSIST);
    }
    f.heal(5)?;
    f.assert_all()?;
    hits += 1;

    // R3 unlink 归零回收：单链节点 unlink 后 nlink=0 且回收标记置位
    f.unlink(1, 70)?; // 7 号节点（key=70）
    if f.reaped_total != 1 {
        return Err(E_ZERO_NLINK);
    }
    let idx = match f.find_node(7) {
        Some(i) => i,
        None => return Err(E_ENOENT),
    };
    if !f.nodes[idx].reaped || f.nodes[idx].nlink != 0 {
        return Err(E_ZERO_NLINK);
    }
    hits += 1;

    // R4 已回收节点再 link → 拒收（悬空引用防护）
    match f.link(7, 1, 7777) {
        Err(E_ZERO_NLINK) => {}
        _ => return Err(E_ZERO_NLINK),
    }
    hits += 1;

    // R5 守恒对账：dentry 总账 == nlink 合计
    if !f.ledger_ok() {
        return Err(E_LEDGER);
    }
    // 装配 1000 节点 1500 链（500 双链），unlink 1 条 → 1499
    if f.names.len() != 1499 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
