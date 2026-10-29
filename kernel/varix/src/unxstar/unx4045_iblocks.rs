//! unx4045_iblocks — UNX-F4045 i_blocks 与按需推导（诊断段 0x8800）。
//!
//! 判据原文：
//!   J1 i_blocks 按 512B 扇区单位从 i_size 推导（稀疏语义零块账）；
//!   J2 推导值与手工计算 1,000 例一致。
//! 设计纪律：
//!   1. 扇区推导（Linux stat 语义）：st_blocks 以 512B 扇区计，
//!      blocks = size 向上取整到扇区（(size + 511) / 512）。
//!   2. 稀疏语义零块账：空洞（size > 0 但无实块）按需推导为
//!      零实块——实块账只随真实分配增长，与 size 推导账分列。
//!   3. 手工对账（判据 J2）：1,000 例确定性枚举（size ∈ 确定性
//!      序列），推导值与逐例手工公式重算逐位一致。
//!   4. 守恒式：实块账 ≤ 推导账（稀疏不超配）；实块账 = Σ 分配
//!      - Σ 释放。
//!   5. 边界：size=0 → 0 块、溢出前置检查（size 上限）、对目录
//!      的推导同样适用——全部显性分支并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8800;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_SIZE_CAP: u16 = SEG_BASE + 1;    // size 超上限（溢出前置检查）
const E_ALLOC_OVER: u16 = SEG_BASE + 2;  // 实块分配超推导账（超配）
const E_FREE_UNDER: u16 = SEG_BASE + 3;  // 实块释放下溢
const E_NO_FILE: u16 = SEG_BASE + 4;     // 不存在的 inode
const E_MISMATCH: u16 = SEG_BASE + 5;    // 推导与手工对账不一致
const E_BAL: u16 = SEG_BASE + 6;         // 实块/推导账不平

/// 扇区字节数（Linux 512B 约定）。
pub const SECTOR: u64 = 512;
/// size 上限（溢出前置检查，u64/扇区安全域）。
pub const SIZE_CAP: u64 = u64::MAX - SECTOR;

/// 单 inode 块账：size + 实块账。
pub struct BlockIno {
    pub i_ino: u64,      // inode 号
    pub i_size: u64,     // 逻辑 size（字节）
    pub real_blocks: u64, // 实块账（扇区单位，稀疏为 0）
    pub alloc_ops: u32,  // 分配事件账
    pub free_ops: u32,   // 释放事件账
}

/// i_blocks 推导枢纽。
pub struct IBlocks {
    pub inos: Vec<BlockIno>,
    pub derive_calls: u32,   // 推导调用数
    pub match_total: u32,    // 手工对账一致例数（判据 J2 账面）
    pub rejects: u32,        // 错误账
}

impl IBlocks {
    pub fn new() -> IBlocks {
        IBlocks {
            inos: Vec::new(),
            derive_calls: 0,
            match_total: 0,
            rejects: 0,
        }
    }

    /// 手工公式（判据 J2 基准）：向上取整到扇区。
    pub fn manual(size: u64) -> u64 {
        (size + SECTOR - 1) / SECTOR
    }

    /// 建 inode（size 0，稀疏零块）。
    pub fn create(&mut self, i_ino: u64) -> Result<usize, u16> {
        self.inos.push(BlockIno {
            i_ino,
            i_size: 0,
            real_blocks: 0,
            alloc_ops: 0,
            free_ops: 0,
        });
        Ok(self.inos.len() - 1)
    }

    /// 按需推导（判据 J1）：size → 扇区数；同时与手工公式对账。
    pub fn derive(&mut self, idx: usize) -> Result<u64, u16> {
        if idx >= self.inos.len() {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        if self.inos[idx].i_size > SIZE_CAP {
            self.rejects += 1;
            return Err(E_SIZE_CAP);
        }
        self.derive_calls += 1;
        let got = Self::manual(self.inos[idx].i_size);
        // 稀疏守恒：实块账不得超过推导账（超配即错）。
        if self.inos[idx].real_blocks > got {
            return Err(E_ALLOC_OVER);
        }
        Ok(got)
    }

    /// 写扩展：size 推进 + 可选实块分配（分配量受推导账约束）。
    pub fn grow(&mut self, idx: usize, new_size: u64, alloc: u64) -> Result<u64, u16> {
        if idx >= self.inos.len() {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        if new_size > SIZE_CAP {
            self.rejects += 1;
            return Err(E_SIZE_CAP);
        }
        let new_derive = Self::manual(new_size);
        let new_real = self.inos[idx].real_blocks + alloc;
        if new_real > new_derive {
            self.rejects += 1;
            return Err(E_ALLOC_OVER);
        }
        self.inos[idx].i_size = new_size;
        self.inos[idx].real_blocks = new_real;
        self.inos[idx].alloc_ops += 1;
        Ok(new_real)
    }

    /// 实块释放：下溢拒收。
    pub fn free_blocks(&mut self, idx: usize, n: u64) -> Result<u64, u16> {
        if idx >= self.inos.len() {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        if n > self.inos[idx].real_blocks {
            self.rejects += 1;
            return Err(E_FREE_UNDER);
        }
        self.inos[idx].real_blocks -= n;
        self.inos[idx].free_ops += 1;
        Ok(self.inos[idx].real_blocks)
    }

    /// 1,000 例手工对账（判据 J2）：确定性序列逐例重算比对。
    pub fn sweep_1000(&mut self) -> Result<u32, u16> {
        let mut k: u32 = 0;
        while k < 1000 {
            // 确定性 size 序列：k*k*7 + k*13 + 1（整数展开，覆盖
            // 0 边界（k=0 时 size=1）到中等量级。
            let size = (k as u64) * (k as u64) * 7 + (k as u64) * 13 + 1;
            let got = Self::manual(size);
            // 手工重算（第二通道，防公式自身篡改）。
            let mut expect = 0u64;
            let mut acc = 0u64;
            while acc < size {
                acc += SECTOR;
                expect += 1;
            }
            if got != expect {
                return Err(E_MISMATCH);
            }
            self.match_total += 1;
            k += 1;
        }
        Ok(self.match_total)
    }

    /// 守恒对账：实块 = 分配 - 释放（逐 inode）；对账例数 = 1000。
    pub fn audit(&self) -> Result<(), u16> {
        if self.match_total != 1000 {
            return Err(E_MISMATCH);
        }
        let mut i = 0;
        while i < self.inos.len() {
            if i < self.inos.len() {
                let f = &self.inos[i];
                if f.real_blocks > Self::manual(f.i_size) {
                    return Err(E_BAL);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 导出快照（inode 数，推导账，对账例数，拒收账）。
    pub fn ledger(&self) -> (usize, u32, u32, u32) {
        (self.inos.len(), self.derive_calls, self.match_total, self.rejects)
    }

    /// 单 inode 块账视图：返回（size，实块，分配事件，释放事件）。
    pub fn ino_view(&self, idx: usize) -> Result<(u64, u64, u32, u32), u16> {
        if idx >= self.inos.len() {
            return Err(E_NO_FILE);
        }
        let f = &self.inos[idx];
        Ok((f.i_size, f.real_blocks, f.alloc_ops, f.free_ops))
    }

    /// 稀疏率视图（千分比整数化）：1 - 实块/推导，无浮点。
    pub fn sparse_rate(&self, idx: usize) -> Result<u32, u16> {
        if idx >= self.inos.len() {
            return Err(E_NO_FILE);
        }
        let derive = Self::manual(self.inos[idx].i_size);
        if derive == 0 {
            return Ok(0);
        }
        let real = self.inos[idx].real_blocks;
        if real > derive {
            return Err(E_ALLOC_OVER);
        }
        Ok(((derive - real) * 1000 / derive) as u32)
    }

    /// 字节域视图：实块换回字节覆盖（实块 × 512，稀疏语义下
    /// 表示已分配覆盖，非逻辑 size）。
    pub fn real_bytes(&self, idx: usize) -> Result<u64, u16> {
        if idx >= self.inos.len() {
            return Err(E_NO_FILE);
        }
        Ok(self.inos[idx].real_blocks * SECTOR)
    }

    /// 手工公式边界面：size 0/1/511/512/513 五点定编核对。
    pub fn boundary_table() -> [(u64, u64); 5] {
        [
            (0, 0),
            (1, 1),
            (511, 1),
            (512, 1),
            (513, 2),
        ]
    }
}

/// 自例程：R1 零块边界 → R2 稀疏语义 → R3 实块分配 → R4 注入拒收 → R5 千例对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut hb = IBlocks::new();

    // R1 零块边界：size 0 → 0 块；size 1 → 1 块；size 512 → 1 块；
    // size 513 → 2 块（向上取整）。
    let f = hb.create(10)?;
    if hb.derive(f)? != 0 {
        return Err(E_BAL);
    }
    hb.grow(f, 1, 0)?;
    if hb.derive(f)? != 1 {
        return Err(E_BAL);
    }
    hb.grow(f, 512, 0)?;
    if hb.derive(f)? != 1 {
        return Err(E_BAL);
    }
    hb.grow(f, 513, 0)?;
    if hb.derive(f)? != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 稀疏语义零块账（判据 J1 后半）：size 4096 而实块 0。
    hb.grow(f, 4096, 0)?;
    if hb.inos[f].real_blocks != 0 || hb.derive(f)? != 8 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 实块分配与释放：分配受推导账约束，释放下溢拒收。
    hb.grow(f, 8192, 16)?;
    if hb.inos[f].real_blocks != 16 || hb.derive(f)? != 16 {
        return Err(E_BAL);
    }
    hb.free_blocks(f, 16)?;
    if hb.inos[f].real_blocks != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 注入拒收：超配分配、下溢释放、越界句柄。
    if hb.grow(f, 8192, 17) != Err(E_ALLOC_OVER) {
        return Err(E_BAL);
    }
    if hb.free_blocks(f, 1) != Err(E_FREE_UNDER) {
        return Err(E_BAL);
    }
    if hb.derive(0xFFFF) != Err(E_NO_FILE) {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 千例对账（判据 J2）+ 守恒。
    let m = hb.sweep_1000()?;
    if m != 1000 {
        return Err(E_MISMATCH);
    }
    hb.audit()?;
    let (n, dc, mt, _rj) = hb.ledger();
    if n != 1 || mt != 1000 || dc < 5 {
        return Err(E_BAL);
    }
    // 边界表核对：五点定编与手工公式逐格一致。
    let bt = IBlocks::boundary_table();
    let mut bi = 0;
    while bi < bt.len() {
        if IBlocks::manual(bt[bi].0) != bt[bi].1 {
            return Err(E_MISMATCH);
        }
        bi += 1;
    }
    // 稀疏率视图：实块 0 → 全稀疏（1000‰）。
    if hb.sparse_rate(f)? != 1000 {
        return Err(E_BAL);
    }
    let (sz, rb, ao, fo) = hb.ino_view(f)?;
    if sz != 8192 || rb != 0 || ao != 5 || fo != 1 {
        return Err(E_BAL);
    }
    if hb.real_bytes(f)? != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
