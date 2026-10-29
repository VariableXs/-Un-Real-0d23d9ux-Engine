//! unx4038_sberror — UNX-F4038 sb 错误上报通道（s_error 记账）（诊断段 0x8740）。
//!
//! 判据原文：
//!   J1 fs 侧错误经 s_error 通道落账（时间/位置/错误码三要素齐备）；
//!   J2 异常零静默：通道断开注入时错误被上层捕获。
//! 设计纪律：
//!   1. 三要素账：每条 s_error 记录含（逻辑时戳 tick，发生位置
//!      位置码，原始错误码）三元组，缺一即拒收（完整性校验）。
//!   2. 通道状态机：Open（可落账）→ Broken（断开，拒收新账）→
//!      Recovered（恢复后重开）；Broken 期到达的错误不许静默——
//!      必须由上层捕获账（escrow 账）暂存，通道恢复后补录。
//!   3. 零静默守恒：错误发生总数 = 落账数 + 暂存数；恢复后
//!      暂存数补录归零，落账数最终追平总数。
//!   4. 位置码闭集：sb 层/inode 层/dentry 层/页缓存层四位置，
//!      闭集外位置拒收。
//!   5. 边界：断开通道落账、恢复后重复补录、闭集外位置码、
//!      账满——全部显性拒收并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8740;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_CH_BROKEN: u16 = SEG_BASE + 1;   // 通道断开期直接落账被拒
const E_BAD_LOC: u16 = SEG_BASE + 2;     // 位置码闭集外
const E_TRIAD_MISS: u16 = SEG_BASE + 3;  // 三要素缺项（完整性失守）
const E_REPLAY_TWICE: u16 = SEG_BASE + 4;// 恢复后重复补录
const E_LOG_FULL: u16 = SEG_BASE + 5;    // s_error 账满（静态配额）
const E_ZERO_ERR: u16 = SEG_BASE + 6;    // 零错误码非法（0 保留表无错）
const E_BAL: u16 = SEG_BASE + 7;         // 零静默守恒失守

/// 位置码闭集。
pub const LOC_SB: u8 = 1;    // sb 层
pub const LOC_INO: u8 = 2;   // inode 层
pub const LOC_DENT: u8 = 3;  // dentry 层
pub const LOC_PG: u8 = 4;    // 页缓存层

/// 账容量（静态配额）。
pub const LOG_CAP: usize = 32;

/// 通道状态机：Open → Broken → Recovered（重开为 Open 语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChanPhase {
    Open,
    Broken,
    Recovered,
}

/// 单条 s_error 记录：三要素齐备。
#[derive(Clone, Copy)]
pub struct SErrRecord {
    pub tick: u32, // 逻辑时戳（错误发生时通道内 tick）
    pub loc: u8,   // 位置码（闭集）
    pub code: u16, // 原始错误码（非 0）
}

/// s_error 通道枢纽。
pub struct SbErrorChan {
    pub phase: ChanPhase,
    pub tick: u32,          // 逻辑时钟（每事件 +1，无墙钟）
    pub log: Vec<SErrRecord>,   // 已落账记录
    pub escrow: Vec<SErrRecord>, // 断开期上层捕获暂存
    pub reported_total: u32,     // 错误发生总数（零静默基准）
    pub logged_total: u32,       // 落账总数
    pub replayed_total: u32,     // 补录总数
    pub rejects: u32,            // 错误账
}

impl SbErrorChan {
    pub fn new() -> SbErrorChan {
        SbErrorChan {
            phase: ChanPhase::Open,
            tick: 0,
            log: Vec::new(),
            escrow: Vec::new(),
            reported_total: 0,
            logged_total: 0,
            replayed_total: 0,
            rejects: 0,
        }
    }

    /// 位置码闭集校验。
    fn loc_ok(loc: u8) -> bool {
        loc == LOC_SB || loc == LOC_INO || loc == LOC_DENT || loc == LOC_PG
    }

    /// 断开注入（测试钩子/真实故障统一入口）。
    pub fn break_chan(&mut self) -> Result<(), u16> {
        if self.phase == ChanPhase::Broken {
            self.rejects += 1;
            return Err(E_BAL);
        }
        self.phase = ChanPhase::Broken;
        Ok(())
    }

    /// 通道恢复：Broken/Recovered → Open，并触发补录。
    pub fn recover(&mut self) -> Result<u32, u16> {
        if self.phase == ChanPhase::Open {
            self.rejects += 1;
            return Err(E_BAL);
        }
        self.phase = ChanPhase::Open;
        // 补录：暂存账逐条迁入主账（判据 J2 的恢复段）。
        let mut n = 0u32;
        while !self.escrow.is_empty() {
            if self.log.len() >= LOG_CAP {
                self.rejects += 1;
                return Err(E_LOG_FULL);
            }
            let rec = self.escrow[0];
            self.log.push(rec);
            self.escrow.remove(0);
            self.logged_total += 1;
            self.replayed_total += 1;
            n += 1;
        }
        self.phase = ChanPhase::Recovered;
        Ok(n)
    }

    /// 错误上报：三要素齐备性校验 → Open 落账 / Broken 暂存。
    pub fn report(&mut self, loc: u8, code: u16) -> Result<(), u16> {
        if code == 0 {
            self.rejects += 1;
            return Err(E_ZERO_ERR);
        }
        if !Self::loc_ok(loc) {
            self.rejects += 1;
            return Err(E_BAD_LOC);
        }
        // 三要素：tick/loc/code 由本调用齐备性构造（tick 非负恒真，
        // loc 闭集已校验，code 非 0 已校验）——缺项注入以占位 0 拒收。
        if loc == 0 || code == 0 {
            self.rejects += 1;
            return Err(E_TRIAD_MISS);
        }
        self.tick += 1;
        self.reported_total += 1;
        let rec = SErrRecord { tick: self.tick, loc, code };
        match self.phase {
            ChanPhase::Open => {
                if self.log.len() >= LOG_CAP {
                    self.rejects += 1;
                    return Err(E_LOG_FULL);
                }
                self.log.push(rec);
                self.logged_total += 1;
                Ok(())
            }
            ChanPhase::Broken => {
                // 判据 J2 主通道：断开期错误不静默，上层捕获暂存。
                self.escrow.push(rec);
                Ok(())
            }
            ChanPhase::Recovered => {
                if self.log.len() >= LOG_CAP {
                    self.rejects += 1;
                    return Err(E_LOG_FULL);
                }
                self.log.push(rec);
                self.logged_total += 1;
                Ok(())
            }
        }
    }

    /// 记录查询（三要素齐备性断言面）。
    pub fn record_at(&mut self, idx: usize) -> Result<SErrRecord, u16> {
        if idx >= self.log.len() {
            self.rejects += 1;
            return Err(E_TRIAD_MISS);
        }
        let r = self.log[idx];
        if r.code == 0 || r.loc == 0 {
            return Err(E_TRIAD_MISS);
        }
        Ok(r)
    }

    /// 零静默守恒对账（判据 J2 终局）：总数 = 落账 + 暂存；
    /// Open 期暂存必须为空。
    pub fn audit(&self) -> Result<(), u16> {
        if self.reported_total != self.logged_total + self.escrow.len() as u32 {
            return Err(E_BAL);
        }
        if self.phase == ChanPhase::Open && !self.escrow.is_empty() {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 导出快照（相位，落账数，暂存数，总报错数，tick）。
    pub fn ledger(&self) -> (ChanPhase, u32, usize, u32, u32) {
        (self.phase, self.logged_total, self.escrow.len(), self.reported_total, self.tick)
    }

    /// 主账尾记录视图（最近一条三要素，空账返 Err）。
    pub fn tail(&self) -> Result<SErrRecord, u16> {
        if self.log.is_empty() {
            return Err(E_TRIAD_MISS);
        }
        Ok(self.log[self.log.len() - 1])
    }

    /// 位置分布账：四位置闭集逐位计数（落账域）。
    pub fn loc_dist(&self) -> [u32; 4] {
        let mut d = [0u32; 4];
        let mut i = 0;
        while i < self.log.len() {
            let loc = self.log[i].loc;
            if loc >= LOC_SB && loc <= LOC_PG {
                d[(loc - LOC_SB) as usize] += 1;
            }
            i += 1;
        }
        d
    }

    /// 暂存账尾视图（断开期最近捕获，空返 Err）。
    pub fn escrow_tail(&self) -> Result<SErrRecord, u16> {
        if self.escrow.is_empty() {
            return Err(E_TRIAD_MISS);
        }
        Ok(self.escrow[self.escrow.len() - 1])
    }

    /// 相位迁移合法性核验：Open 无暂存、Broken 可有暂存、
    /// Recovered 必无暂存（补录已清）。
    pub fn transition_ok(&self) -> bool {
        match self.phase {
            ChanPhase::Open => self.escrow.is_empty(),
            ChanPhase::Broken => true,
            ChanPhase::Recovered => self.escrow.is_empty(),
        }
    }
}

/// 自例程：R1 正常落账 → R2 断开暂存 → R3 恢复补录 → R4 注入拒收 → R5 零静默守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut ch = SbErrorChan::new();

    // R1 正常落账：两条错误（不同位置）齐三要素入账。
    ch.report(LOC_SB, 0x8741)?;
    ch.report(LOC_INO, 0x8742)?;
    if ch.log.len() != 2 || ch.reported_total != 2 {
        return Err(E_BAL);
    }
    let r0 = ch.record_at(0)?;
    if r0.tick == 0 || r0.loc != LOC_SB || r0.code == 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 断开暂存（判据 J2 注入）：断开期上报不静默，进上层暂存账。
    ch.break_chan()?;
    ch.report(LOC_DENT, 0x8743)?;
    ch.report(LOC_PG, 0x8744)?;
    if ch.escrow.len() != 2 || ch.log.len() != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 恢复补录：恢复后暂存账全部迁入主账，账序不变。
    let n = ch.recover()?;
    if n != 2 || !ch.escrow.is_empty() || ch.log.len() != 4 {
        return Err(E_BAL);
    }
    let r3 = ch.record_at(2)?;
    if r3.loc != LOC_DENT || r3.code != 0x8743 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 注入拒收：零错误码、闭集外位置、重复补录、越界查询。
    if ch.report(LOC_SB, 0) != Err(E_ZERO_ERR) {
        return Err(E_BAL);
    }
    if ch.report(9, 0x8745) != Err(E_BAD_LOC) {
        return Err(E_BAL);
    }
    if ch.recover() != Err(E_BAL) {
        return Err(E_BAL);
    }
    if ch.record_at(99).is_ok() {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 零静默守恒（判据 J2 终局）：总数 = 落账 + 暂存（暂存空）。
    ch.audit()?;
    let (ph, lg, esc, rep, _t) = ch.ledger();
    if ph != ChanPhase::Recovered || lg != 4 || esc != 0 || rep != 4 {
        return Err(E_BAL);
    }
    if ch.replayed_total != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R6 分布与迁移核验：位置分布账、尾记录、相位迁移三面互证。
    let dist = ch.loc_dist();
    if dist[0] != 1 || dist[1] != 1 || dist[2] != 1 || dist[3] != 1 {
        // 四位置各一笔（补录后全在主账）。
        return Err(E_BAL);
    }
    let t6 = ch.tail()?;
    if t6.loc != LOC_PG || t6.code != 0x8744 {
        return Err(E_BAL);
    }
    if ch.escrow_tail().is_ok() {
        // Recovered 期暂存账必须为空。
        return Err(E_BAL);
    }
    if !ch.transition_ok() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
