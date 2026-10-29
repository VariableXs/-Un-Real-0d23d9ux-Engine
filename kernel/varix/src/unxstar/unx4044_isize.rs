//! unx4044_isize — UNX-F4044 i_size 语义与目录大小约定（诊断段 0x87E0）。
//!
//! 判据原文：
//!   J1 常规文件 i_size 随写入/截断更新账零偏差；
//!   J2 目录 i_size 按 fs 约定（ramfs 取条目聚合计）并有账；
//!   J3 负 size 注入被拒。
//! 设计纪律：
//!   1. 常规文件账：write_at 扩展 size 至 off+len（稀疏补零），
//!      truncate 单向收缩（扩张拒收）；size 账与写入/截断事件
//!      逐笔闭合（判据 J1 零偏差）。
//!   2. 目录约定账：目录 i_size 不独立存储，按 ramfs 约定取条目
//!      聚合计（每条目 = 名长 + 定长元数据），加删条目即刷新。
//!   3. 守恒式：size 增量 = 写入字节净增；目录 size = Σ 条目计；
//!      文件 size ≥ 0（u64 通道天然非负，负值注入在入口拦截）。
//!   4. 负值注入面：外部语义以 i64 通道传入 size 时，负数一律
//!      拒收（判据 J3），账面不动。
//!   5. 边界：截断超原长（拒）、off 越界写（拒）、空写（零事件）——
//!      全部显性分支并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x87E0;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_NEG_SIZE: u16 = SEG_BASE + 1;    // 负 size 注入（判据 J3）
const E_TRUNC_GROW: u16 = SEG_BASE + 2;  // 截断扩张（单向收缩违例）
const E_WR_OOB: u16 = SEG_BASE + 3;      // 写偏移越界（超 size 语义外）
const E_NO_FILE: u16 = SEG_BASE + 4;     // 不存在的 inode
const E_DIR_DIRECT: u16 = SEG_BASE + 5;  // 目录 size 直写（约定违例）
const E_BAL: u16 = SEG_BASE + 6;         // size 账与事件账不平

/// 目录条目元数据定长（ramfs 约定：名长 + 固定 8 字节头）。
pub const DIRENT_META: u64 = 8;

/// inode 大小账：常规文件与目录分域。
pub struct SizeIno {
    pub i_ino: u64,      // inode 号
    pub is_dir: bool,    // 目录类型位
    pub i_size: u64,     // 当前 size（常字节）
    pub wr_events: u32,  // 写事件账
    pub tr_events: u32,  // 截断事件账
    pub sz_gained: u64,  // 累计净增
    pub sz_shrunk: u64,  // 累计净减
}

/// 条目聚合型目录账（判据 J2：目录 size = Σ(名长+META)）。
pub struct DirEntLedger {
    pub name_lens: Vec<u64>, // 条目名长账
}

impl DirEntLedger {
    fn new() -> DirEntLedger {
        DirEntLedger { name_lens: Vec::new() }
    }

    /// 聚合 size（ramfs 约定）。
    fn agg(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0;
        while i < self.name_lens.len() {
            if i < self.name_lens.len() {
                s += self.name_lens[i] + DIRENT_META;
            }
            i += 1;
        }
        s
    }
}

/// i_size 枢纽：inode 账 + 目录约定账。
pub struct ISizeHub {
    pub inos: Vec<SizeIno>,
    pub dir: DirEntLedger,
    pub rejects: u32,       // 错误账
    pub size_total: u64,    // 全部常规文件 size 汇总
}

impl ISizeHub {
    pub fn new() -> ISizeHub {
        ISizeHub {
            inos: Vec::new(),
            dir: DirEntLedger::new(),
            rejects: 0,
            size_total: 0,
        }
    }

    /// 建 inode：常规文件 size 0；目录 size 由聚合账派生。
    pub fn create(&mut self, i_ino: u64, is_dir: bool) -> Result<usize, u16> {
        self.inos.push(SizeIno {
            i_ino,
            is_dir,
            i_size: 0,
            wr_events: 0,
            tr_events: 0,
            sz_gained: 0,
            sz_shrunk: 0,
        });
        Ok(self.inos.len() - 1)
    }

    /// 负值注入入口（判据 J3）：外部 i64 通道语义，负数拒收。
    pub fn set_size_checked(&mut self, idx: usize, want: i64) -> Result<u64, u16> {
        if idx >= self.inos.len() {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        if want < 0 {
            self.rejects += 1;
            return Err(E_NEG_SIZE);
        }
        self.inos[idx].i_size = want as u64;
        Ok(self.inos[idx].i_size)
    }

    /// 常规文件写入：off ≤ size 校验后扩展，size 账逐笔闭合。
    pub fn write_at(&mut self, idx: usize, off: u64, len: u64) -> Result<u64, u16> {
        if idx >= self.inos.len() {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        if self.inos[idx].is_dir {
            self.rejects += 1;
            return Err(E_DIR_DIRECT);
        }
        if off > self.inos[idx].i_size {
            self.rejects += 1;
            return Err(E_WR_OOB);
        }
        if len == 0 {
            // 空写：零事件（账不动，合法返回）。
            return Ok(self.inos[idx].i_size);
        }
        let new_size = off + len;
        if new_size > self.inos[idx].i_size {
            let delta = new_size - self.inos[idx].i_size;
            self.inos[idx].i_size = new_size;
            self.inos[idx].sz_gained += delta;
            self.size_total += delta;
        }
        self.inos[idx].wr_events += 1;
        Ok(self.inos[idx].i_size)
    }

    /// 截断：单向收缩（扩张拒收）。
    pub fn truncate(&mut self, idx: usize, want: u64) -> Result<u64, u16> {
        if idx >= self.inos.len() {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        if self.inos[idx].is_dir {
            self.rejects += 1;
            return Err(E_DIR_DIRECT);
        }
        if want > self.inos[idx].i_size {
            self.rejects += 1;
            return Err(E_TRUNC_GROW);
        }
        let delta = self.inos[idx].i_size - want;
        self.inos[idx].i_size = want;
        self.inos[idx].sz_shrunk += delta;
        self.inos[idx].tr_events += 1;
        self.size_total -= delta;
        Ok(self.inos[idx].i_size)
    }

    /// 目录条目加/删：聚合 size 刷新（判据 J2 账面）。
    pub fn dir_add(&mut self, idx: usize, name_len: u64) -> Result<u64, u16> {
        if idx >= self.inos.len() || !self.inos[idx].is_dir {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        self.dir.name_lens.push(name_len);
        self.inos[idx].i_size = self.dir.agg();
        Ok(self.inos[idx].i_size)
    }

    pub fn dir_del(&mut self, idx: usize, slot: usize) -> Result<u64, u16> {
        if idx >= self.inos.len() || !self.inos[idx].is_dir {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        if slot >= self.dir.name_lens.len() {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        self.dir.name_lens.remove(slot);
        self.inos[idx].i_size = self.dir.agg();
        Ok(self.inos[idx].i_size)
    }

    /// 守恒对账（判据 J1）：size = 累计净增 - 累计净减；
    /// 汇总账 = Σ 常规文件 size。
    pub fn audit(&self) -> Result<(), u16> {
        let mut sum = 0u64;
        let mut i = 0;
        while i < self.inos.len() {
            if i < self.inos.len() {
                let f = &self.inos[i];
                if !f.is_dir {
                    if f.i_size != f.sz_gained - f.sz_shrunk {
                        return Err(E_BAL);
                    }
                    sum += f.i_size;
                }
            }
            i += 1;
        }
        if sum != self.size_total {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 导出快照（inode 数，size 汇总，目录聚合 size，拒收账）。
    pub fn ledger(&self) -> (usize, u64, u64, u32) {
        (self.inos.len(), self.size_total, self.dir.agg(), self.rejects)
    }

    /// 单 inode 事件账视图：返回（写事件，截断事件，净增，净减）。
    pub fn ino_events(&self, idx: usize) -> Result<(u32, u32, u64, u64), u16> {
        if idx >= self.inos.len() {
            return Err(E_NO_FILE);
        }
        let f = &self.inos[idx];
        Ok((f.wr_events, f.tr_events, f.sz_gained, f.sz_shrunk))
    }

    /// 目录条目数视图（聚合账长度）。
    pub fn dir_entries(&self) -> usize {
        self.dir.name_lens.len()
    }
}

/// 自例程：R1 写入扩张 → R2 截断收缩 → R3 目录聚合 → R4 负值/越界注入 → R5 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut hub = ISizeHub::new();

    // R1 写入扩张（判据 J1 正向）：三笔写，size 账逐笔闭合。
    let f = hub.create(10, false)?;
    hub.write_at(f, 0, 100)?;
    hub.write_at(f, 100, 50)?;
    let s = hub.write_at(f, 120, 30)?;
    if s != 150 || hub.inos[f].i_size != 150 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 截断收缩：100 截断，size 与净增/净减账闭合。
    let s2 = hub.truncate(f, 100)?;
    if s2 != 100 || hub.inos[f].sz_gained != 150 || hub.inos[f].sz_shrunk != 50 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 目录聚合（判据 J2）：三条目加 + 一条目删，size = Σ(名长+8)。
    let d = hub.create(11, true)?;
    hub.dir_add(d, 4)?; // 12
    hub.dir_add(d, 2)?; // 10
    hub.dir_add(d, 7)?; // 15 → 37
    if hub.inos[d].i_size != 4 + 8 + 2 + 8 + 7 + 8 {
        return Err(E_BAL);
    }
    hub.dir_del(d, 1)?;
    if hub.inos[d].i_size != 12 + 15 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 注入拒收（判据 J3 + 边界）：负 size、截断扩张、越界写、目录直写。
    if hub.set_size_checked(f, -1) != Err(E_NEG_SIZE) {
        return Err(E_BAL);
    }
    if hub.truncate(f, 200) != Err(E_TRUNC_GROW) {
        return Err(E_BAL);
    }
    if hub.write_at(f, 101, 1) != Err(E_WR_OOB) {
        return Err(E_BAL);
    }
    if hub.write_at(d, 0, 1) != Err(E_DIR_DIRECT) {
        return Err(E_BAL);
    }
    if hub.inos[f].i_size != 100 {
        // 被拒注入不得动账。
        return Err(E_BAL);
    }
    hits += 1;

    // R5 守恒对账：size = 净增 - 净减；汇总账闭合。
    hub.audit()?;
    let (n, st, dag, _rj) = hub.ledger();
    if n != 2 || st != 100 || dag != 27 {
        return Err(E_BAL);
    }
    // 事件账视图：f 三写一截断，净增 150 / 净减 50；目录 2 条目。
    let (wr, tr, gn, sh) = hub.ino_events(f)?;
    if wr != 3 || tr != 1 || gn != 150 || sh != 50 {
        return Err(E_BAL);
    }
    if hub.dir_entries() != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
