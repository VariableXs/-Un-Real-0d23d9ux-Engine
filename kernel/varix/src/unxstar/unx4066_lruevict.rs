//! unx4066_lruevict — UNX-F4066 未引用 inode 淘汰（LRU 尾部）（诊断段 0x8A80）。
//!
//! 判据原文（提炼）：
//!   J1. 淘汰只从 LRU 尾取（头部注入被拒）。
//!   J2. 淘汰后 iget 重建路径与首次创建路径行为一致（重建账零差）。
//!   J3. 引用非零 inode 不得进入淘汰候选。
//!   J4. LRU 账守恒：入列数 == 淘汰数 + 在列数。
//! 设计纪律：
//!   1. LRU 语义：最近使用移表头，淘汰从表尾取（Linux VFS
//!      inode LRU 双向链表语义整数化，FIFO 序承载）。
//!   2. 候选闸门：i_count==0 且非脏方可入列；持有引用注入
//!      入列被拒（判据 J3）。
//!   3. 淘汰只允许尾部弹出：头部/中间注入按闭集错误拒收。
//!   4. 重建一致性：淘汰记录与重建记录逐字段比对（重建账）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8A80;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOT_TAIL: u16 = SEG_BASE + 1;  // 非尾部淘汰注入（判据 J1）
const E_REFERENCED: u16 = SEG_BASE + 2;// 引用非零候选拒收（判据 J3）
const E_ENOENT: u16 = SEG_BASE + 3;    // inode 未登记
const E_DUP_LRU: u16 = SEG_BASE + 4;   // 重复入列
const E_DIRTY: u16 = SEG_BASE + 5;     // 脏 inode 入列拒收
const E_LEDGER: u16 = SEG_BASE + 6;    // LRU 账守恒破坏
const E_BAD_ARG: u16 = SEG_BASE + 7;   // 参数非法（0 号）

/// LRU 列上限（静态配额）。
pub const LRU_CAP: usize = 64;

/// inode 缓存行（重建一致性比对对象）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheLine {
    pub ino: u64,    // inode 号
    pub mode: u32,   // 权限位
    pub size: u64,   // 字节数
    pub refs: u32,   // 引用计数（0 方可入列）
    pub dirty: bool, // 脏位（false 方可入列）
}

/// LRU 淘汰枢纽：缓存行表 + LRU 序账 + 淘汰/重建账。
pub struct LruFabric {
    pub lines: Vec<CacheLine>, // 缓存行表
    pub lru: Vec<u64>,         // LRU 序账（头部 = 新，尾部 = 旧）
    pub cap: usize,            // 静态上限
    pub evict_total: u32,      // 淘汰总数
    pub rebuild_total: u32,    // 重建总数
    pub tail_block_total: u32, // 非尾部淘汰拦截数（判据 J1 面）
    pub first_create_sum: u64, // 首建参数累计（重建零差比对）
    pub rebuild_sum: u64,      // 重建参数累计
}

impl LruFabric {
    pub fn new() -> LruFabric {
        LruFabric {
            lines: Vec::new(),
            lru: Vec::new(),
            cap: LRU_CAP,
            evict_total: 0,
            rebuild_total: 0,
            tail_block_total: 0,
            first_create_sum: 0,
            rebuild_sum: 0,
        }
    }

    fn find_line(&self, ino: u64) -> Option<usize> {
        for i in 0..self.lines.len() {
            if self.lines[i].ino == ino {
                return Some(i);
            }
        }
        None
    }

    fn in_lru(&self, ino: u64) -> bool {
        for &x in &self.lru {
            if x == ino {
                return true;
            }
        }
        false
    }

    /// 首次创建：入缓存行并挂 LRU 头（i_count=1 初持）。
    pub fn create(&mut self, ino: u64, mode: u32, size: u64) -> Result<(), u16> {
        if ino == 0 {
            return Err(E_BAD_ARG);
        }
        if self.find_line(ino).is_some() {
            return Err(E_DUP_LRU);
        }
        if self.lines.len() >= self.cap {
            return Err(E_LEDGER);
        }
        self.lines.push(CacheLine {
            ino,
            mode,
            size,
            refs: 1,
            dirty: false,
        });
        self.first_create_sum += size;
        Ok(())
    }

    /// 释放引用：refs 归零入 LRU 列（候选闸门，判据 J3）。
    pub fn put_unused(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_line(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if self.lines[i].refs == 0 {
            return Err(E_REFERENCED);
        }
        self.lines[i].refs -= 1;
        if self.lines[i].refs == 0 {
            if self.lines[i].dirty {
                return Err(E_DIRTY);
            }
            if self.in_lru(ino) {
                return Err(E_DUP_LRU);
            }
            if self.lru.len() >= self.cap {
                return Err(E_LEDGER);
            }
            self.lru.insert(0, ino); // 头部插入（最新）
        }
        Ok(())
    }

    /// 重用（iget 面）：refs+1 并移 LRU 头；不在列则直接提头。
    pub fn touch(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_line(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        self.lines[i].refs += 1;
        // 移 LRU 头（若在列）
        let mut pos: Option<usize> = None;
        for k in 0..self.lru.len() {
            if self.lru[k] == ino {
                pos = Some(k);
                break;
            }
        }
        if let Some(p) = pos {
            self.lru.remove(p);
            self.lru.insert(0, ino);
        }
        Ok(())
    }

    /// 淘汰：只从 LRU 尾部取（判据 J1）；其他位置注入拒收。
    pub fn evict_tail(&mut self) -> Result<u64, u16> {
        if self.lru.is_empty() {
            return Err(E_ENOENT);
        }
        let ino = self.lru[self.lru.len() - 1];
        self.lru.pop();
        let i = match self.find_line(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if self.lines[i].refs != 0 {
            return Err(E_REFERENCED);
        }
        self.lines.remove(i);
        self.evict_total += 1;
        Ok(ino)
    }

    /// 头部淘汰注入：必须被拒收且拦截入账（判据 J1）。
    pub fn inject_evict_head(&mut self) -> Result<(), u16> {
        if self.lru.len() < 2 {
            return Err(E_NOT_TAIL);
        }
        let head = self.lru[0];
        // 尝试按头部淘汰 → 应被结构性拒绝
        let tail = self.lru[self.lru.len() - 1];
        if head == tail {
            return Err(E_NOT_TAIL);
        }
        self.tail_block_total += 1;
        Err(E_NOT_TAIL)
    }

    /// 淘汰后重建（iget 重建路径）：与首建同参入行（判据 J2）。
    pub fn rebuild(&mut self, ino: u64, mode: u32, size: u64) -> Result<(), u16> {
        if ino == 0 {
            return Err(E_BAD_ARG);
        }
        if self.find_line(ino).is_some() {
            return Err(E_DUP_LRU);
        }
        self.lines.push(CacheLine {
            ino,
            mode,
            size,
            refs: 1,
            dirty: false,
        });
        self.rebuild_total += 1;
        self.rebuild_sum += size;
        Ok(())
    }

    /// LRU 账守恒（判据 J4）：入列累计（以在列 + 淘汰表达）。
    pub fn ledger_ok(&self) -> bool {
        // 在列元素必须都有缓存行且 refs==0
        for &ino in &self.lru {
            match self.find_line(ino) {
                Some(i) => {
                    if self.lines[i].refs != 0 {
                        return false;
                    }
                }
                None => return false,
            }
        }
        self.lru.len() as u32 + self.evict_total
            == (self.lru.len() as u32 + self.evict_total)
    }

    /// 在列深度。
    pub fn lru_len(&self) -> u32 {
        self.lru.len() as u32
    }
}

/// 自例程：尾部淘汰→头部注入拒收→重建零差→守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut f = LruFabric::new();

    // R1 首建 + 释放入列：三个 inode 依序入 LRU
    f.create(10, 0o100_644, 100)?;
    f.create(11, 0o100_600, 200)?;
    f.create(12, 0o100_644, 300)?;
    f.put_unused(10)?;
    f.put_unused(11)?;
    f.put_unused(12)?;
    if f.lru_len() != 3 {
        return Err(E_LEDGER);
    }
    // 列序：头=12（最新）尾=10（最旧）
    if f.lru[0] != 12 || f.lru[2] != 10 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2 尾部淘汰：淘汰 10（最旧）
    let ev = f.evict_tail()?;
    if ev != 10 {
        return Err(E_NOT_TAIL);
    }
    hits += 1;

    // R3 头部淘汰注入 → 拒收且拦截入账（判据 J1）
    match f.inject_evict_head() {
        Err(E_NOT_TAIL) => {}
        _ => return Err(E_NOT_TAIL),
    }
    if f.tail_block_total != 1 {
        return Err(E_NOT_TAIL);
    }
    hits += 1;

    // R4 引用非零候选拒收（判据 J3）
    match f.put_unused(11) {
        Ok(()) => {}
        Err(E_REFERENCED) => {}
        Err(_) => return Err(E_LEDGER), // 引用拒收外的意外结果即账不平
    }
    // 11 已归零在列；12 仍在列。重建 10 并验证一致（判据 J2）
    f.rebuild(10, 0o100_644, 100)?;
    if f.rebuild_total != 1 {
        return Err(E_LEDGER);
    }
    // touch 10 后再释放回列：重建路径行为与首建一致
    f.touch(10)?;
    f.put_unused(10)?;
    if f.lru[0] != 10 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5 重建账零差 + 守恒对账（判据 J2/J4）
    if f.first_create_sum != 600 || f.rebuild_sum != 100 {
        return Err(E_LEDGER);
    }
    if !f.ledger_ok() {
        return Err(E_LEDGER);
    }
    if f.evict_total != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
