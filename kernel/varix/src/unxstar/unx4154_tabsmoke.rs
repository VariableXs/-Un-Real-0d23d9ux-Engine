//! unx4154_tabsmoke — UNX-F4154 语义表冻结回归账（逐函数冒烟）（诊断段 0x9500）。
//!
//! 判据原文：
//!   J1 三表 32 函数逐一冒烟（fake 负载）全过（32/32）。
//!   J2 冒烟账与冻结快照版本号绑定可追溯。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三表定编：inode 表 10 函数 + file 表 12 函数 + dir 表 10 函数
//!      = 32 函数闭集，函数号按表内偏移编址，越界号拒收。
//!   2. 逐函数冒烟一账一条：fake 负载非空才放行，同号重复冒烟拒收
//!      （幂等审计口径：以首账为准）。
//!   3. 冻结快照版本号绑定：每条冒烟账落账时快照版本必须非零，
//!      版本中途变更判失守（追溯性红线）。
//!   4. 守恒：冒烟账条数 = 三表函数总数，通过数 = 总数才算绿。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9500;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NO_SUCH_FN: u16 = SEG_BASE + 1;  // 函数号越界（表外注入）
const E_DUP_SMOKE: u16 = SEG_BASE + 2;   // 同号重复冒烟
const E_EMPTY_LOAD: u16 = SEG_BASE + 3;  // fake 负载为空
const E_NO_SNAPSHOT: u16 = SEG_BASE + 4; // 快照版本未绑定先冒烟
const E_SNAP_SHIFT: u16 = SEG_BASE + 5;  // 冒烟中途快照版本漂移
const E_SMOKE_MISS: u16 = SEG_BASE + 6;  // 冒烟账缺函数（不足 32/32）
const E_SMOKE_FAIL: u16 = SEG_BASE + 7;  // 有函数冒烟未过
const E_BAL: u16 = SEG_BASE + 8;         // 账数守恒失守
const E_EMPTY_SUM: u16 = SEG_BASE + 9;   // 写回摘要为空

/// 三表定编（冻结口径，不得增删）。
pub const TBL_INODE: u8 = 0;
pub const TBL_FILE: u8 = 1;
pub const TBL_DIR: u8 = 2;
pub const INODE_FN_COUNT: u8 = 10; // inode 表 10 函数
pub const FILE_FN_COUNT: u8 = 12;  // file 表 12 函数
pub const DIR_FN_COUNT: u8 = 10;   // dir 表 10 函数
pub const TOTAL_FN: u8 = INODE_FN_COUNT + FILE_FN_COUNT + DIR_FN_COUNT; // 32

/// 表编号→该表函数数（越界表拒收）。
pub fn table_fn_count(tbl: u8) -> Result<u8, u16> {
    match tbl {
        TBL_INODE => Ok(INODE_FN_COUNT),
        TBL_FILE => Ok(FILE_FN_COUNT),
        TBL_DIR => Ok(DIR_FN_COUNT),
        _ => Err(E_NO_SUCH_FN),
    }
}

/// 冒烟结果状态机：Pending 未冒 / Passed 已过 / Failed 未过。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmokeState {
    Pending,
    Passed,
    Failed,
}

/// 单函数冒烟账。
pub struct SmokeRecord {
    pub tbl: u8,     // 表号
    pub fn_off: u8,  // 表内偏移 0..count
    pub state: SmokeState,
    pub load_len: u32,        // fake 负载长度
    pub snapshot_ver: u32,    // 落账时冻结快照版本
}

/// 冒烟账枢纽：三表 32 槽 + 快照版本绑定 + 全绿判定。
pub struct SmokeLedger {
    pub snapshot_ver: u32, // 冻结快照版本号（0=未绑定）
    records: Vec<SmokeRecord>,
    pub passed_total: u32, // 通过计数
    pub failed_total: u32, // 未过计数
}

impl SmokeLedger {
    pub fn new() -> SmokeLedger {
        SmokeLedger {
            snapshot_ver: 0,
            records: Vec::new(),
            passed_total: 0,
            failed_total: 0,
        }
    }

    /// 绑定冻结快照版本（非零才有效；绑定后变更判漂移）。
    pub fn bind_snapshot(&mut self, ver: u32) -> Result<(), u16> {
        if ver == 0 {
            return Err(E_NO_SNAPSHOT);
        }
        if self.snapshot_ver != 0 && self.snapshot_ver != ver {
            return Err(E_SNAP_SHIFT);
        }
        self.snapshot_ver = ver;
        Ok(())
    }

    fn fn_exists(tbl: u8, fn_off: u8) -> Result<(), u16> {
        let n = table_fn_count(tbl)?;
        if fn_off >= n {
            return Err(E_NO_SUCH_FN);
        }
        Ok(())
    }

    fn already(&self, tbl: u8, fn_off: u8) -> bool {
        for r in &self.records {
            if r.tbl == tbl && r.fn_off == fn_off {
                return true;
            }
        }
        false
    }

    /// 逐函数冒烟：fake 负载非空 + 快照已绑定 + 号在表内 + 不重复。
    /// 负载长度为奇数视作 fake 故障注入（冒烟未过）。
    pub fn smoke(&mut self, tbl: u8, fn_off: u8, load_len: u32) -> Result<bool, u16> {
        Self::fn_exists(tbl, fn_off)?;
        if load_len == 0 {
            return Err(E_EMPTY_LOAD);
        }
        if self.snapshot_ver == 0 {
            return Err(E_NO_SNAPSHOT);
        }
        if self.already(tbl, fn_off) {
            return Err(E_DUP_SMOKE);
        }
        let pass = load_len % 2 == 0; // fake 判定：偶长负载视为应答正常
        self.records.push(SmokeRecord {
            tbl,
            fn_off,
            state: if pass { SmokeState::Passed } else { SmokeState::Failed },
            load_len,
            snapshot_ver: self.snapshot_ver,
        });
        if pass {
            self.passed_total += 1;
        } else {
            self.failed_total += 1;
        }
        Ok(pass)
    }

    /// 冒烟账条数（32 槽覆盖核验用）。
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// 全绿判定：32/32 且全部 Passed 且版本全程一致（判据 J1/J2）。
    pub fn verdict(&self) -> Result<(), u16> {
        if self.snapshot_ver == 0 {
            return Err(E_NO_SNAPSHOT);
        }
        if self.records.len() != TOTAL_FN as usize {
            return Err(E_SMOKE_MISS);
        }
        for r in &self.records {
            if r.state != SmokeState::Passed {
                return Err(E_SMOKE_FAIL);
            }
            if r.snapshot_ver != self.snapshot_ver {
                return Err(E_SNAP_SHIFT);
            }
        }
        if self.failed_total != 0 {
            return Err(E_SMOKE_FAIL);
        }
        if self.passed_total != TOTAL_FN as u32 {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 完成写回（判据 J3）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_BAL);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程：R1 全表 32/32 冒烟 → R2 边界 → R3 注入拦截 → R4 追溯对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：绑快照 → 三表逐一冒烟（32 函数）→ 全绿判定。
    let mut l = SmokeLedger::new();
    l.bind_snapshot(0x2026)?;
    let mut tbl = TBL_INODE;
    let mut smoke_rounds = 0u32;
    while smoke_rounds < 3 {
        let n = table_fn_count(tbl)?;
        let mut off = 0u8;
        while off < n {
            l.smoke(tbl, off, 16)?; // 偶长负载 = fake 正常应答
            off += 1;
        }
        tbl += 1;
        smoke_rounds += 1;
    }
    l.verdict()?;
    l.writeback(true, 0x80)?;
    hits += 1;

    // R2 边界：表号越界 / 函数偏移越界 / 空负载 / 未绑快照。
    let mut l = SmokeLedger::new();
    l.bind_snapshot(7)?;
    if l.smoke(3, 0, 8).is_ok() {
        return Err(E_NO_SUCH_FN);
    }
    if l.smoke(TBL_INODE, INODE_FN_COUNT, 8).is_ok() {
        return Err(E_NO_SUCH_FN);
    }
    if l.smoke(TBL_FILE, 0, 0).is_ok() {
        return Err(E_EMPTY_LOAD);
    }
    let mut l2 = SmokeLedger::new();
    if l2.smoke(TBL_INODE, 0, 8).is_ok() {
        return Err(E_NO_SNAPSHOT);
    }
    hits += 1;

    // R3 注入拦截：重复冒烟拒收；奇长负载判未过；版本漂移拒收。
    let mut l = SmokeLedger::new();
    l.bind_snapshot(5)?;
    l.smoke(TBL_DIR, 1, 8)?;
    if l.smoke(TBL_DIR, 1, 8).is_ok() {
        return Err(E_DUP_SMOKE);
    }
    let mut l2 = SmokeLedger::new();
    l2.bind_snapshot(5)?;
    if l2.smoke(TBL_DIR, 2, 7)? {
        return Err(E_SMOKE_FAIL); // 奇长负载必须判未过
    }
    if l2.failed_total != 1 {
        return Err(E_BAL);
    }
    let mut l3 = SmokeLedger::new();
    l3.bind_snapshot(5)?;
    if l3.bind_snapshot(6).is_ok() {
        return Err(E_SNAP_SHIFT);
    }
    hits += 1;

    // R4 追溯对账：32 槽账齐 + 版本逐条一致 + verdict 通过。
    let mut l = SmokeLedger::new();
    l.bind_snapshot(0xBEEF)?;
    let mut tbl = TBL_INODE;
    let mut k = 0u32;
    while k < 3 {
        let n = table_fn_count(tbl)?;
        let mut off = 0u8;
        while off < n {
            l.smoke(tbl, off, 2)?;
            off += 1;
        }
        tbl += 1;
        k += 1;
    }
    if l.record_count() != TOTAL_FN as usize {
        return Err(E_SMOKE_MISS);
    }
    l.verdict()?;
    hits += 1;

    // R5 追加回归：同版本重绑幂等 + 三表命中读数逐表核对。
    let mut l = SmokeLedger::new();
    l.bind_snapshot(0x77)?;
    l.bind_snapshot(0x77)?; // 同版本重绑幂等放行
    if l.bind_snapshot(0).is_ok() {
        return Err(E_NO_SNAPSHOT);
    }
    let mut tbl = TBL_INODE;
    let mut k = 0u32;
    while k < 3 {
        let n = table_fn_count(tbl)?;
        let mut off = 0u8;
        while off < n {
            l.smoke(tbl, off, 4)?;
            off += 1;
        }
        tbl += 1;
        k += 1;
    }
    l.verdict()?;
    if l.passed_total != TOTAL_FN as u32 {
        return Err(E_BAL);
    }
    if l.record_count() != TOTAL_FN as usize {
        return Err(E_SMOKE_MISS);
    }
    // 追溯面复核：落账后版本仍锁定原值。
    if l.snapshot_ver != 0x77 {
        return Err(E_SNAP_SHIFT);
    }
    l.writeback(true, 0x33)?;
    hits += 1;

    Ok(hits)
}
