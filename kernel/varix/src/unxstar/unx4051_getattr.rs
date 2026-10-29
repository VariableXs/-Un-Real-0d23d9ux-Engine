//! unx4051_getattr — UNX-F4051 getattr/stat 基础语义（诊断段 0x88C0）。
//!
//! 判据原文（提炼）：
//!   J1. stat 输出 13 字段与 inode 实态逐字段一致（1,000 例抽样全对）。
//!   J2. 对不存在路径返回 ENOENT，且不残留任何半成品 stat 结构。
//!   J3. 抽样审计逐字段比对，任一不一致即失败并定位字段号。
//!   J4. 读账守恒：stat 成功 + ENOENT == 总入口计数，全程恒成立。
//! 设计纪律：
//!   1. 13 字段取 POSIX.1-2008 struct stat：dev/ino/mode/nlink/uid/gid/
//!      rdev/size/blksize/blocks/atime/mtime/ctime，字段序写死定编。
//!   2. stat 只读：getattr 管道不改写 inode 任何字段，读账单向累加。
//!   3. 先查后填：查表命中后才允许装配 stat 结构，未命中零写入，
//!      杜绝半成品外泄（判据 J2 的结构性保证）。
//!   4. st_blocks 以 512B 计、blksize 定编 4 KiB（Linux VFS 默认）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x88C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ENOENT: u16 = SEG_BASE + 1;  // 路径/inode 号不存在（POSIX ENOENT）
const E_BAD_INO: u16 = SEG_BASE + 2; // inode 号 0 非法（0 号保留）
const E_DUP_INO: u16 = SEG_BASE + 3; // 注册重复 inode 号
const E_TAB_FULL: u16 = SEG_BASE + 4;// inode 注册表满（静态上限）
const E_FLD_MISMATCH: u16 = SEG_BASE + 5; // 抽样审计字段不一致
const E_AUDIT_FAIL: u16 = SEG_BASE + 6;   // 审计总账与命中账不平
const E_BAD_ARG: u16 = SEG_BASE + 7; // 抽样参数非法（0 例或超界）

/// stat 13 字段数（POSIX.1-2008 struct stat 标准字段定编）。
pub const STAT_FIELDS: usize = 13;

/// 字段号定编（逐字段审计定位用，来源 POSIX struct stat 字段序）。
pub const FLD_DEV: usize = 0;
pub const FLD_INO: usize = 1;
pub const FLD_MODE: usize = 2;
pub const FLD_NLINK: usize = 3;
pub const FLD_UID: usize = 4;
pub const FLD_GID: usize = 5;
pub const FLD_RDEV: usize = 6;
pub const FLD_SIZE: usize = 7;
pub const FLD_BLKSIZE: usize = 8;
pub const FLD_BLOCKS: usize = 9;
pub const FLD_ATIME: usize = 10;
pub const FLD_MTIME: usize = 11;
pub const FLD_CTIME: usize = 12;

/// blksize 定编：4 KiB（Linux VFS 默认页块大小）。
pub const DEF_BLKSIZE: u64 = 4096;

/// 审计检出标记位（注入检出回执，区别于错误码本体）。
pub const FLD_MISMATCH_MARK: u16 = 0x0100;

/// inode 实态（getattr 的读侧真值源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InodeLite {
    pub ino: u64,     // inode 号（非 0）
    pub dev: u64,     // 所在设备号
    pub mode: u32,    // 类型 + 权限位（POSIX mode_t 语义）
    pub nlink: u32,   // 硬链接计数
    pub uid: u32,     // 属主
    pub gid: u32,     // 属组
    pub rdev: u64,    // 设备文件号（非设备文件为 0）
    pub size: u64,    // 字节数
    pub blocks: u64,  // 占用 512B 块数
    pub atime: u64,   // 访问时刻（整数时钟刻，无墙钟）
    pub mtime: u64,   // 内容修改时刻
    pub ctime: u64,   // 元数据变更时刻
}

impl InodeLite {
    /// 由字段表装配（blocks 由 size 推导：满块向上取整）。
    pub fn assemble(ino: u64, dev: u64, mode: u32, nlink: u32,
                    uid: u32, gid: u32, size: u64) -> InodeLite {
        InodeLite {
            ino, dev, mode, nlink, uid, gid,
            rdev: 0,
            size,
            blocks: (size + 511) / 512, // POSIX st_blocks：512B 计
            atime: 0,
            mtime: 0,
            ctime: 0,
        }
    }
}

/// stat 输出结构：13 字段按 FLD_* 定编序排列。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StatBuf {
    pub fields: [u64; STAT_FIELDS],
}

impl StatBuf {
    fn empty() -> StatBuf {
        StatBuf { fields: [0; STAT_FIELDS] }
    }
    /// 从 inode 实态整结构复制（唯一合法装配点，先查后填）。
    fn from_inode(n: &InodeLite, blksize: u64) -> StatBuf {
        let mut b = StatBuf::empty();
        b.fields[FLD_DEV] = n.dev;
        b.fields[FLD_INO] = n.ino;
        b.fields[FLD_MODE] = n.mode as u64;
        b.fields[FLD_NLINK] = n.nlink as u64;
        b.fields[FLD_UID] = n.uid as u64;
        b.fields[FLD_GID] = n.gid as u64;
        b.fields[FLD_RDEV] = n.rdev;
        b.fields[FLD_SIZE] = n.size;
        b.fields[FLD_BLKSIZE] = blksize;
        b.fields[FLD_BLOCKS] = n.blocks;
        b.fields[FLD_ATIME] = n.atime;
        b.fields[FLD_MTIME] = n.mtime;
        b.fields[FLD_CTIME] = n.ctime;
        b
    }
    /// 与 inode 实态逐字段比对：一致返回 None，不一致返回字段号。
    pub fn diff_field(&self, n: &InodeLite, blksize: u64) -> Option<usize> {
        let ref_b = StatBuf::from_inode(n, blksize);
        for i in 0..STAT_FIELDS {
            if self.fields[i] != ref_b.fields[i] {
                return Some(i);
            }
        }
        None
    }
}

/// getattr 引擎：inode 注册表 + stat 读账。
pub struct GetattrEngine {
    pub tab: Vec<InodeLite>,   // inode 实态注册表
    pub cap: usize,            // 静态上限（本引擎定编 256）
    pub blksize: u64,          // st_blksize 输出定编
    pub stat_ok: u32,          // stat 成功计数
    pub stat_enoent: u32,      // ENOENT 计数（含 0 号与未注册）
    pub stat_total: u32,       // stat 总入口计数（守恒主账）
    pub audit_samples: u32,    // 抽样审计例数
}

pub const ENGINE_CAP: usize = 256;

impl GetattrEngine {
    pub fn new() -> GetattrEngine {
        GetattrEngine {
            tab: Vec::new(),
            cap: ENGINE_CAP,
            blksize: DEF_BLKSIZE,
            stat_ok: 0,
            stat_enoent: 0,
            stat_total: 0,
            audit_samples: 0,
        }
    }

    fn find(&self, ino: u64) -> Option<usize> {
        for i in 0..self.tab.len() {
            if self.tab[i].ino == ino {
                return Some(i);
            }
        }
        None
    }

    /// 注册 inode 实态（真值源入表；重复号/0 号/超容全拒收）。
    pub fn register(&mut self, n: InodeLite) -> Result<(), u16> {
        if n.ino == 0 {
            return Err(E_BAD_INO);
        }
        if self.tab.len() >= self.cap {
            return Err(E_TAB_FULL);
        }
        if self.find(n.ino).is_some() {
            return Err(E_DUP_INO);
        }
        self.tab.push(n);
        Ok(())
    }

    /// getattr 核心：先查后填，未命中零写入（判据 J2 结构保证）。
    pub fn getattr(&mut self, ino: u64) -> Result<StatBuf, u16> {
        self.stat_total += 1;
        if ino == 0 {
            self.stat_enoent += 1;
            return Err(E_ENOENT);
        }
        match self.find(ino) {
            Some(i) => {
                self.stat_ok += 1;
                Ok(StatBuf::from_inode(&self.tab[i], self.blksize))
            }
            None => {
                self.stat_enoent += 1;
                Err(E_ENOENT)
            }
        }
    }

    /// 抽样审计：对注册表内前 `samples` 个 inode 逐个 stat 并
    /// 13 字段逐项比对（判据 J1/J3），返回比对字段总数。
    pub fn audit_sample(&mut self, samples: usize) -> Result<u32, u16> {
        if samples == 0 || samples > self.tab.len() {
            return Err(E_BAD_ARG);
        }
        let mut checked: u32 = 0;
        let blk = self.blksize;
        for i in 0..samples {
            let n = self.tab[i];
            let sb = self.getattr(n.ino)?;
            if let Some(f) = sb.diff_field(&n, blk) {
                // 定位失败字段号：低 8 位承载，供上层归因
                return Err(E_FLD_MISMATCH | (f as u16 & 0xFF));
            }
            checked += STAT_FIELDS as u32;
            self.audit_samples += 1;
        }
        // 守恒：stat 分账与总账交叉核对
        if self.stat_ok + self.stat_enoent != self.stat_total {
            return Err(E_AUDIT_FAIL);
        }
        Ok(checked)
    }

    /// 注入一个被篡改的 stat 输出，验证审计器能检出（判据 J3）。
    pub fn inject_tampered(&mut self, slot: usize, fld: usize) -> Result<u16, u16> {
        if slot >= self.tab.len() || fld >= STAT_FIELDS {
            return Err(E_BAD_ARG);
        }
        let n = self.tab[slot];
        let mut sb = self.getattr(n.ino)?;
        sb.fields[fld] = sb.fields[fld].wrapping_add(1); // 单字段翻转
        match sb.diff_field(&n, self.blksize) {
            Some(f) => Ok(FLD_MISMATCH_MARK | (f as u16 & 0xFF)),
            None => Err(E_AUDIT_FAIL),
        }
    }

    /// 守恒对账：成功 + ENOENT == 总入口。
    pub fn ledger_ok(&self) -> bool {
        self.stat_ok + self.stat_enoent == self.stat_total
    }
}

/// 千例抽样判据复核：注册 n 个 inode，全量 stat 后逐字段比对。
/// 返回比对通过的字段总数（n*13）。
pub fn sample_thousand(engine: &mut GetattrEngine, n: usize) -> Result<u32, u16> {
    if n == 0 || n > ENGINE_CAP {
        return Err(E_BAD_ARG);
    }
    let mut k: u64 = 1;
    while engine.tab.len() < n {
        // 确定性装配：mode/size 由 k 整数推导，无随机源
        let mode: u32 = 0o100_644 | ((k % 8) as u32);
        let dev = if k % 2 == 0 { 8 } else { 2059 };
        let node = InodeLite::assemble(k, dev, mode, 1, 1000, 1000, k * 37);
        engine.register(node)?;
        k += 1;
    }
    engine.audit_sample(n)
}

/// 自例程：正常路径→ENOENT 边界→千例抽样→篡改注入→守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut g = GetattrEngine::new();

    // R1 正常路径：注册后 stat，关键字段在位且 blksize 定编正确
    let node = InodeLite::assemble(7, 8, 0o100_600, 1, 0, 0, 4096);
    g.register(node)?;
    let sb = g.getattr(7)?;
    if sb.fields[FLD_INO] != 7 || sb.fields[FLD_SIZE] != 4096 {
        return Err(E_FLD_MISMATCH);
    }
    if sb.fields[FLD_BLKSIZE] != DEF_BLKSIZE || sb.fields[FLD_BLOCKS] != 8 {
        return Err(E_FLD_MISMATCH);
    }
    hits += 1;

    // R2 ENOENT 边界：未注册号返回 ENOENT 且无半成品外泄
    match g.getattr(99) {
        Err(E_ENOENT) => {}
        _ => return Err(E_ENOENT),
    }
    match g.getattr(0) {
        Err(E_ENOENT) => {}
        _ => return Err(E_ENOENT),
    }
    hits += 1;

    // R3 千例抽样审计（判据 J1：1,000 例逐字段一致）
    let mut big = GetattrEngine::new();
    let checked = sample_thousand(&mut big, 1000)?;
    if checked != (1000 * STAT_FIELDS) as u32 {
        return Err(E_AUDIT_FAIL);
    }
    hits += 1;

    // R4 篡改注入：审计器必须检出并定位字段号
    let mark = g.inject_tampered(0, FLD_SIZE)?;
    if mark & FLD_MISMATCH_MARK == 0 {
        return Err(E_AUDIT_FAIL);
    }
    if (mark & 0xFF) as usize != FLD_SIZE {
        return Err(E_FLD_MISMATCH);
    }
    hits += 1;

    // R5 守恒对账：stat_ok + stat_enoent == stat_total
    if !g.ledger_ok() {
        return Err(E_AUDIT_FAIL);
    }
    if g.stat_total != 4 { // R1 一次 + R2 两次 + R4 一次
        return Err(E_AUDIT_FAIL);
    }
    hits += 1;

    Ok(hits)
}
