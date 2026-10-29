//! UNX-F12053 PEB AtlThunkSListPtr ATL Thunk 槽语义
//! 分段: 0x198c0 批次: B03 领域: PEB
//! 验收: 折叠指纹 + 封存 + 注入五样 + 12~20 条判据全绿
//!
//! 语义模型：AtlThunkSListPtr 是环境块中为 ATL thunk
//! （SList 无锁链分配器）预留的槽位指针。本卡以
//! "无锁链节点池"建模：
//!   - 池容量 16 节点，节点 32 字节对齐；
//!   - 分配返回对齐后伪 VA，回收按伪 VA 索引；
//!   - SList Push/Pop 遵循 LIFO；
//!   - 对齐不符、重复回收、池满、封存分配均为语义错误。

use crate::alloc::format;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x198C0;
pub const SEG_ATL_ALIGN: u32 = SEG_BASE + 0x01;
pub const SEG_ATL_DUPFREE: u32 = SEG_BASE + 0x02;
pub const SEG_ATL_POOLFULL: u32 = SEG_BASE + 0x03;
pub const SEG_ATL_SEALED: u32 = SEG_BASE + 0x04;
pub const SEG_ATL_NOTNODE: u32 = SEG_BASE + 0x05;

pub const POOL_CAP: u32 = 16;
pub const ALIGN: u32 = 32;

/// 节点池槽
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlNode {
    pub index: u32,
    pub in_use: bool,
    pub push_epoch: u32,
}

impl AtlNode {
    /// 伪 VA：基址 0x7000_0000 + index << 8（天然 32 对齐）
    pub fn pseudo_va(&self) -> u32 {
        0x7000_0000u32.wrapping_add(self.index.wrapping_shl(8))
    }
}

/// ATL Thunk 池
#[derive(Debug, Clone)]
pub struct AtlThunkPool {
    pub nodes: Vec<AtlNode>,
    pub slist: Vec<u32>,   // 栈：存伪 VA
    pub sealed: bool,
    pub epoch: u32,
}

impl AtlThunkPool {
    pub fn new() -> AtlThunkPool {
        let mut nodes = Vec::new();
        let mut i = 0;
        while i < POOL_CAP {
            nodes.push(AtlNode { index: i, in_use: false, push_epoch: 0 });
            i += 1;
        }
        AtlThunkPool { nodes, slist: Vec::new(), sealed: false, epoch: 0 }
    }

    /// 分配节点：封存 → SEALED；池满 → POOLFULL
    pub fn alloc_node(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(SEG_ATL_SEALED);
        }
        let mut found: Option<u32> = None;
        for n in &self.nodes {
            if !n.in_use {
                found = Some(n.index);
                break;
            }
        }
        let idx = match found {
            Some(i) => i,
            None => return Err(SEG_ATL_POOLFULL),
        };
        self.epoch += 1;
        self.nodes[idx as usize].in_use = true;
        self.nodes[idx as usize].push_epoch = self.epoch;
        Ok(self.nodes[idx as usize].pseudo_va())
    }

    /// Push 到 SList：非本池节点 → NOTNODE
    pub fn push(&mut self, va: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(SEG_ATL_SEALED);
        }
        if !self.in_pool(va) {
            return Err(SEG_ATL_NOTNODE);
        }
        self.slist.push(va);
        Ok(self.slist.len())
    }

    /// Pop：LIFO
    pub fn pop(&mut self) -> Result<u32, u32> {
        match self.slist.pop() {
            Some(v) => Ok(v),
            None => Err(SEG_ATL_NOTNODE),
        }
    }

    /// 回收：不在池 → NOTNODE；未用（重复回收）→ DUPFREE
    pub fn free_node(&mut self, va: u32) -> Result<(), u32> {
        if !self.in_pool(va) {
            return Err(SEG_ATL_NOTNODE);
        }
        let idx = (va.wrapping_sub(0x7000_0000)).wrapping_shr(8) as usize;
        if !self.nodes[idx].in_use {
            return Err(SEG_ATL_DUPFREE);
        }
        self.nodes[idx].in_use = false;
        // 从 slist 中摘除
        let mut i = 0;
        while i < self.slist.len() {
            if self.slist[i] == va {
                self.slist.remove(i);
            } else {
                i += 1;
            }
        }
        Ok(())
    }

    /// 伪 VA 是否属于本池（含对齐检查）
    pub fn in_pool(&self, va: u32) -> bool {
        if va % ALIGN != 0 {
            return false;
        }
        let off = va.wrapping_sub(0x7000_0000);
        if off >= POOL_CAP.wrapping_shl(8) {
            return false;
        }
        let idx = off.wrapping_shr(8);
        // 低 8 位必须为 0（节点粒度 0x100）
        (off & 0xFF) == 0 && idx < POOL_CAP
    }

    pub fn used_count(&self) -> u32 {
        self.nodes.iter().filter(|n| n.in_use).count() as u32
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 折叠指纹：211 进制卷积，种子 SEG_BASE
pub fn fold(pool: &AtlThunkPool) -> u32 {
    let mut h: u64 = SEG_BASE as u64;
    for n in &pool.nodes {
        h = (h.wrapping_mul(211).wrapping_add(n.in_use as u64)) & 0xFFFF_FFFF;
        h = (h.wrapping_mul(211).wrapping_add(n.push_epoch as u64)) & 0xFFFF_FFFF;
    }
    for v in &pool.slist {
        h = (h.wrapping_mul(211).wrapping_add(*v as u64)) & 0xFFFF_FFFF;
    }
    h = (h.wrapping_mul(211).wrapping_add(pool.sealed as u64)) & 0xFFFF_FFFF;
    h as u32
}

/// 注入五样：每个坏样命中一个独立错误分支
pub fn inject_five() -> [bool; 5] {
    let mut ok = [false; 5];
    // 样1: 未对齐 VA → NOTNODE（对齐分支）
    let mut p = AtlThunkPool::new();
    if !p.in_pool(0x7000_0010) && p.push(0x7000_0010) == Err(SEG_ATL_NOTNODE) {
        ok[0] = true;
    }
    // 样2: 重复回收 → DUPFREE
    let mut p2 = AtlThunkPool::new();
    let va = p2.alloc_node().unwrap_or(0);
    let _ = p2.free_node(va);
    if p2.free_node(va) == Err(SEG_ATL_DUPFREE) {
        ok[1] = true;
    }
    // 样3: 池满 → POOLFULL
    let mut p3 = AtlThunkPool::new();
    let mut n = 0;
    while n < POOL_CAP {
        let _ = p3.alloc_node();
        n += 1;
    }
    if p3.alloc_node() == Err(SEG_ATL_POOLFULL) {
        ok[2] = true;
    }
    // 样4: 池外 VA push → NOTNODE
    let mut p4 = AtlThunkPool::new();
    if p4.push(0x8000_0000) == Err(SEG_ATL_NOTNODE) {
        ok[3] = true;
    }
    // 样5: 封存后分配 → SEALED
    let mut p5 = AtlThunkPool::new();
    p5.seal();
    if p5.alloc_node() == Err(SEG_ATL_SEALED) {
        ok[4] = true;
    }
    ok
}

/// 自检：12 条以上判据
pub fn run_unx12053_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F12053");
    // J1 分配返回对齐伪 VA
    let mut p = AtlThunkPool::new();
    let va = p.alloc_node().unwrap_or(0);
    cs.add("J1", va % ALIGN == 0 && va >= 0x7000_0000, "aligned va");
    // J2 in_pool 接受本池 VA
    cs.add("J2", p.in_pool(va), "in pool");
    // J3 未对齐拒绝
    cs.add("J3", !p.in_pool(0x7000_0004), "unaligned reject");
    // J4 池外高位拒绝
    cs.add("J4", !p.in_pool(0x7000_1000), "out of pool");
    // J5 LIFO push/pop
    let _ = p.push(va);
    let va2 = p.alloc_node().unwrap_or(0);
    let _ = p.push(va2);
    cs.add("J5", p.pop() == Ok(va2) && p.pop() == Ok(va), "lifo order");
    // J6 空栈 pop → NOTNODE
    cs.add("J6", p.pop().err() == Some(SEG_ATL_NOTNODE), "empty pop");
    // J7 free 后 used 归零
    let _ = p.free_node(va2);
    cs.add("J7", p.used_count() == 1, "one left");
    // J8 重复回收
    cs.add("J8", p.free_node(va2).err() == Some(SEG_ATL_DUPFREE), "dup free");
    // J9 回收从 slist 摘除
    let mut p2 = AtlThunkPool::new();
    let a = p2.alloc_node().unwrap_or(0);
    let _ = p2.push(a);
    let _ = p2.free_node(a);
    cs.add("J9", p2.slist.is_empty(), "slist purged");
    // J10 池满
    let mut p3 = AtlThunkPool::new();
    let mut n = 0;
    while n < POOL_CAP {
        let _ = p3.alloc_node();
        n += 1;
    }
    cs.add("J10", p3.alloc_node().err() == Some(SEG_ATL_POOLFULL), "pool full");
    // J11 释放后可再分配
    let _ = p3.free_node(0x7000_0000);
    cs.add("J11", p3.alloc_node().is_ok(), "realloc after free");
    // J12 封存
    let mut p4 = AtlThunkPool::new();
    p4.seal();
    cs.add("J12", p4.alloc_node().err() == Some(SEG_ATL_SEALED), "sealed alloc");
    // J13 push 封存
    cs.add("J13", p4.push(0x7000_0000).err() == Some(SEG_ATL_SEALED), "sealed push");
    // J14 epoch 递增
    let mut p5 = AtlThunkPool::new();
    let _ = p5.alloc_node();
    let _ = p5.alloc_node();
    cs.add("J14", p5.epoch == 2, "epoch 2");
    // J15 注入五样
    let inj = inject_five();
    cs.add("J15", inj.iter().all(|&b| b), "inject five");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifo() {
        let mut p = AtlThunkPool::new();
        let a = p.alloc_node().ok().unwrap_or(0);
        let b = p.alloc_node().ok().unwrap_or(0);
        let _ = p.push(a);
        let _ = p.push(b);
        assert_eq!(p.pop().ok(), Some(b));
        assert_eq!(p.pop().ok(), Some(a));
    }

    #[test]
    fn test_pool_full_then_reuse() {
        let mut p = AtlThunkPool::new();
        let mut vas = Vec::new();
        let mut n = 0;
        while n < POOL_CAP {
            if let Ok(v) = p.alloc_node() {
                vas.push(v);
            }
            n += 1;
        }
        assert!(p.alloc_node().is_err());
        let _ = p.free_node(vas[0]);
        assert!(p.alloc_node().is_ok());
    }

    #[test]
    fn test_fold_stable() {
        let mut p = AtlThunkPool::new();
        let _ = p.alloc_node();
        let a = fold(&p);
        let b = fold(&p);
        assert_eq!(a, b);
        assert_ne!(a, SEG_BASE);
    }
}
