//! unx4036_emergroro — UNX-F4036 sb 强制只读切换（emergency remount-ro 类）（诊断段 0x8700）。
//!
//! 判据原文：
//!   J1 强制只读切换后脏页写回先行完成（写回完成标志先于位翻转）；
//!   J2 切换后写路径返回 EROFS 10/10 次。
//! 设计纪律：
//!   1. 双相位切换协议：emergency_ro 分两相位——相位 A 脏页全部
//!      写回（写回完成标志置位），相位 B 才翻转只读位；顺序账
//!      以单调序号记录，写回序号 < 翻转序号为不变式。
//!   2. 只读屏障：只读位翻转后所有写路径（write/append/truncate）
//!      一律返回 EROFS，脏账不得再增长（写拒绝不产生新脏页）。
//!   3. 水位模型：脏页入账（页号去重），写回时逐页清账；
//!      写回计数 = 切换时脏页数（全量写回语义）。
//!   4. 状态机：Rw → Syncing（相位 A 进行中）→ Ro（终态），
//!      单向迁移；Rw 期可追加脏页，Syncing 期写路径已封。
//!   5. 边界：空脏账切换（零写回合法）、重复切换、只读期写、
//!      只读期重复写回——全部显性分支并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8700;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_ROFS: u16 = SEG_BASE + 1;        // 只读屏障（EROFS 语义）
const E_ALREADY_RO: u16 = SEG_BASE + 2;  // 已只读再切换（重复切换）
const E_NOT_RO: u16 = SEG_BASE + 3;      // 未只读期查询只读后状态
const E_DIRTY_LIVE: u16 = SEG_BASE + 4;  // 翻转后脏账非空（切换失守）
const E_ORDER_INV: u16 = SEG_BASE + 5;   // 写回序号未先于翻转序号（相位违例）
const E_BAD_PAGE: u16 = SEG_BASE + 6;    // 非法页号（0 保留）
const E_BAL: u16 = SEG_BASE + 7;         // 脏账/写回账不平

/// 页账容量（静态配额）。
pub const DIRTY_CAP: usize = 64;

/// sb 读写状态机：单向迁移，Ro 终态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RwPhase {
    Rw,       // 可读写
    Syncing,  // 相位 A：写回进行中（写路径已封）
    Ro,       // 相位 B 完成：强制只读
}

/// 单页脏账条目。
pub struct DirtyPage {
    pub pgno: u32,     // 页号（非 0）
    pub write_seq: u32, // 脏时刻序号
}

/// 强制只读切换枢纽。
pub struct EmergRo {
    pub phase: RwPhase,
    pub dirty: Vec<DirtyPage>, // 脏页账（页号去重）
    pub seq: u32,              // 全局单调序号（写/写回/翻转共用）
    pub flush_seq: u32,        // 写回完成时的序号（0 表未发生）
    pub flip_seq: u32,         // 只读位翻转时的序号（0 表未发生）
    pub writeback_total: u32,  // 写回页总数
    pub ro_writes_blocked: u32, // 只读期写被拒数（判据 J2 账面）
    pub rejects: u32,          // 错误账
}

impl EmergRo {
    pub fn new() -> EmergRo {
        EmergRo {
            phase: RwPhase::Rw,
            dirty: Vec::new(),
            seq: 0,
            flush_seq: 0,
            flip_seq: 0,
            writeback_total: 0,
            ro_writes_blocked: 0,
            rejects: 0,
        }
    }

    /// 写路径（Rw 期）：页号入脏账（去重），序号推进。
    pub fn write_page(&mut self, pgno: u32) -> Result<u32, u16> {
        if self.phase != RwPhase::Rw {
            // 判据 J2 主通道：只读/Syncing 期写一律 EROFS。
            self.ro_writes_blocked += 1;
            return Err(E_ROFS);
        }
        if pgno == 0 {
            self.rejects += 1;
            return Err(E_BAD_PAGE);
        }
        // 页号去重：已在脏账则只推序号不重复入账。
        let mut i = 0;
        while i < self.dirty.len() {
            if i < self.dirty.len() && self.dirty[i].pgno == pgno {
                self.seq += 1;
                self.dirty[i].write_seq = self.seq;
                return Ok(self.seq);
            }
            i += 1;
        }
        if self.dirty.len() >= DIRTY_CAP {
            self.rejects += 1;
            return Err(E_BAL);
        }
        self.seq += 1;
        self.dirty.push(DirtyPage { pgno, write_seq: self.seq });
        Ok(self.seq)
    }

    /// 相位 A：脏页全量写回，写回完成标志（flush_seq）置位。
    fn flush_all(&mut self) -> u32 {
        let n = self.dirty.len() as u32;
        self.seq += 1;
        self.flush_seq = self.seq; // 写回完成标志（先于翻转）
        self.writeback_total += n;
        // 清空脏账（写回完成）。
        self.dirty.clear();
        n
    }

    /// 强制只读切换：相位 A 写回 → 相位 B 翻转，顺序账落定。
    pub fn emergency_ro(&mut self) -> Result<(u32, u32), u16> {
        if self.phase != RwPhase::Rw {
            self.rejects += 1;
            return Err(E_ALREADY_RO);
        }
        self.phase = RwPhase::Syncing;
        // 相位 A：写回先行。
        let flushed = self.flush_all();
        // 相位 B：翻转只读位（必须晚于写回标志）。
        self.seq += 1;
        self.flip_seq = self.seq;
        self.phase = RwPhase::Ro;
        Ok((flushed, self.flip_seq))
    }

    /// 截断路径（走同一只读屏障）。
    pub fn truncate(&mut self, _pgno: u32) -> Result<(), u16> {
        if self.phase != RwPhase::Rw {
            self.ro_writes_blocked += 1;
            return Err(E_ROFS);
        }
        Ok(())
    }

    /// 只读期状态查询：切换账（写回数，序号对，脏账余量）。
    pub fn switch_view(&mut self) -> Result<(u32, u32, u32, usize), u16> {
        if self.phase != RwPhase::Ro {
            self.rejects += 1;
            return Err(E_NOT_RO);
        }
        Ok((self.writeback_total, self.flush_seq, self.flip_seq, self.dirty.len()))
    }

    /// 守恒对账：写回完成序号 < 翻转序号（判据 J1 不变式）；
    /// 翻转后脏账必须为空；脏账入账 = 写回 + 拒绝外溢为零。
    pub fn audit(&self) -> Result<(), u16> {
        if self.phase == RwPhase::Ro {
            if self.flush_seq == 0 || self.flip_seq == 0 {
                return Err(E_ORDER_INV);
            }
            if self.flush_seq >= self.flip_seq {
                return Err(E_ORDER_INV);
            }
            if !self.dirty.is_empty() {
                return Err(E_DIRTY_LIVE);
            }
        }
        if self.flush_seq > self.seq || self.flip_seq > self.seq {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 导出快照（相位，写回账，写拒账，脏账长）。
    pub fn ledger(&self) -> (RwPhase, u32, u32, usize) {
        (self.phase, self.writeback_total, self.ro_writes_blocked, self.dirty.len())
    }

    /// 序号视图：返回（当前序号，写回序号，翻转序号）——判据 J1
    /// 顺序账的只读观测面。
    pub fn seq_view(&self) -> (u32, u32, u32) {
        (self.seq, self.flush_seq, self.flip_seq)
    }

    /// 相位只读查询。
    pub fn phase_view(&self) -> RwPhase {
        self.phase
    }

    /// 拒收细分：写路径屏障拒收（EROFS 通道）与非法入参拒收二分。
    pub fn reject_split(&self) -> (u32, u32) {
        (self.ro_writes_blocked, self.rejects)
    }

    /// 脏页在账判定（去重账查询面）。
    pub fn is_dirty(&self, pgno: u32) -> bool {
        let mut i = 0;
        while i < self.dirty.len() {
            if self.dirty[i].pgno == pgno {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 相位迁移合法性核验（单向迁移不变式）：
    /// Rw 期无序号落定；Syncing 期写回已落、翻转未落；Ro 期双落。
    pub fn transition_ok(&self) -> bool {
        match self.phase {
            RwPhase::Rw => self.flush_seq == 0 && self.flip_seq == 0,
            RwPhase::Syncing => self.flush_seq > 0 && self.flip_seq == 0,
            RwPhase::Ro => self.flush_seq > 0 && self.flip_seq > self.flush_seq,
        }
    }
}

/// 自例程：R1 脏账积累 → R2 切换顺序账 → R3 EROFS 10/10 → R4 注入拒收 → R5 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut sb = EmergRo::new();

    // R1 脏账积累：5 页脏入（含重复页号去重验证）。
    sb.write_page(1)?;
    sb.write_page(2)?;
    sb.write_page(3)?;
    sb.write_page(2)?; // 重复：去重只推序号
    sb.write_page(4)?;
    if sb.dirty.len() != 4 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 切换顺序账（判据 J1）：写回标志先于翻转标志，写回 4 页。
    let (flushed, flip) = sb.emergency_ro()?;
    if flushed != 4 || sb.flush_seq >= flip || sb.phase != RwPhase::Ro {
        return Err(E_BAL);
    }
    sb.audit()?;
    hits += 1;

    // R3 EROFS 10/10（判据 J2）：切换后写路径十连击全部被拒。
    let mut blocked10 = 0;
    let mut k = 0;
    while k < 10 {
        if sb.write_page(9) == Err(E_ROFS) {
            blocked10 += 1;
        }
        k += 1;
    }
    if blocked10 != 10 || sb.ro_writes_blocked != 10 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 注入拒收：非法页号（Rw 期）、重复切换、未只读查询、截断屏障。
    let mut sb2 = EmergRo::new();
    if sb2.write_page(0) != Err(E_BAD_PAGE) {
        return Err(E_BAL);
    }
    sb2.emergency_ro()?;
    if sb2.emergency_ro() != Err(E_ALREADY_RO) {
        // 已 Ro 后二次切换被拒（重复切换屏障）。
        return Err(E_BAL);
    }
    hits += 1;

    // R5 空脏账切换（零写回合法）+ 未只读查询拒收 + 终局对账。
    let mut sb3 = EmergRo::new();
    if sb3.switch_view() != Err(E_NOT_RO) {
        return Err(E_BAL);
    }
    let (f3, fl3) = sb3.emergency_ro()?;
    if f3 != 0 || fl3 == 0 {
        return Err(E_BAL);
    }
    if sb3.truncate(1) != Err(E_ROFS) {
        return Err(E_BAL);
    }
    sb3.audit()?;
    let (ph, wb, blk, dl) = sb3.ledger();
    if ph != RwPhase::Ro || wb != 0 || blk != 1 || dl != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R6 顺序账与相位核验：序号视图、单向迁移、脏页在账三面互证。
    let (sq, fs6, fl6) = sb.seq_view();
    if fs6 == 0 || fl6 != flip || fl6 <= fs6 || sq < fl6 {
        return Err(E_BAL);
    }
    if sb.phase_view() != RwPhase::Ro || !sb.transition_ok() {
        return Err(E_BAL);
    }
    if sb.is_dirty(2) || sb.is_dirty(9) {
        // 写回后脏账必须清空（去重页 2 亦不残留）。
        return Err(E_BAL);
    }
    let (blk_ro, blk_bad) = sb.reject_split();
    if blk_ro != 10 || blk_bad != 0 {
        return Err(E_BAL);
    }
    // sb2 单切换路径：transition_ok 终态核验。
    if !sb2.transition_ok() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
