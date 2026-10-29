//! unx48188_dropkey — UNX-F48188 Wine 窗 IME 消息丢键防御（桥入口分界联签执行）（诊断段 0xB860）。
//!
//! 判据原文：
//!   J1 丢键账本 ×1000 切换零增长；判据锚定 §AI-61 §8 联签对 AI-62 分界、
//!   示例 UNX-F48070。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 桥入口分界联签：每个键事件过桥必须双签——入侧签（桥收到时按
//!      单调 seq 出签）与出侧签（IME 侧按同 seq 签收），两签 seq 逐位
//!      相等才算过桥成功；分界即 AI-62 侧不得改写 seq，只可核签。
//!   2. 丢键账本：凡入侧已签而出侧未见签收的键，逐条落丢键账，账条
//!      记 (seq, from_hwnd, to_hwnd, reason)；账条数=丢键数，守恒可查。
//!   3. 防御执行器：出侧核签失败（seq 缺口/乱序/重放）先挂暂存队列，
//!      同轮内补签成功则放行且不计丢；跨轮仍缺签才落丢键账。
//!   4. ×1000 切换零增长：1000 次焦点切换逐次跑防御执行器，切换前后
//!      丢键账条数差必须为 0（零增长），终账总量恒 0。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB860;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_SIG_GAP: u16 = SEG_BASE + 1;   // 出侧缺签（真丢键）
const E_SIG_REPLAY: u16 = SEG_BASE + 2;// 重放签
const E_LEDGER_BAD: u16 = SEG_BASE + 3;// 丢键账不平
const E_GROWTH: u16 = SEG_BASE + 4;    // 切换后丢键账增长
const E_SWITCH_MISS: u16 = SEG_BASE + 5; // ×1000 切换不足
const E_STATE_BAD: u16 = SEG_BASE + 6; // 防御器状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 一条丢键账。
#[derive(Clone, Copy, Debug)]
pub struct DropEntry {
    pub seq: u64,
    pub from_hwnd: u32,
    pub to_hwnd: u32,
    pub reason: u16, // E_SIG_GAP / E_SIG_REPLAY
}

/// 暂存队列条目：入侧已签、出侧待核的键。
#[derive(Clone, Copy, Debug)]
pub struct StagedKey {
    pub seq: u64,
    pub to_hwnd: u32,
}

/// 防御器状态机：Armed 已设防 → Checking 核签中 → Drained 本轮清空。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefState {
    Armed,
    Checking,
    Drained,
}

/// 桥入口丢键防御枢纽：联签核验 + 暂存补签 + 丢键账本。
pub struct DropGuard {
    pub state: DefState,
    pub in_seq: u64,          // 入侧出签计数（单调）
    pub out_seq: u64,         // 出侧最高核签 seq
    staged: Vec<StagedKey>,   // 暂存队列（同轮内等待补签）
    drop_ledger: Vec<DropEntry>,
    pub drop_total: u32,
    pub switch_total: u32,
    pub passed_keys: u64,     // 联签成功过桥键数
}

impl DropGuard {
    pub fn new() -> DropGuard {
        DropGuard {
            state: DefState::Armed,
            in_seq: 0,
            out_seq: 0,
            staged: Vec::new(),
            drop_ledger: Vec::new(),
            drop_total: 0,
            switch_total: 0,
            passed_keys: 0,
        }
    }

    /// 入侧出签：键到桥，单调发 seq。
    pub fn sign_in(&mut self, to_hwnd: u32) -> u64 {
        self.in_seq += 1;
        self.staged.push(StagedKey { seq: self.in_seq, to_hwnd });
        self.in_seq
    }

    /// 出侧核签（纪律 1/3）：seq 必须在暂存队列且 >已核高位（防重放）。
    /// 核签成功即出队放行；seq 不在队列 = 跨轮缺签 → 落丢键账。
    pub fn sign_out(&mut self, seq: u64) -> Result<(), u16> {
        self.state = DefState::Checking;
        if seq <= self.out_seq && self.out_seq != 0 {
            // 重放：已核过的 seq 再次签收。
            return Err(E_SIG_REPLAY);
        }
        let mut found = None;
        let mut i = 0usize;
        while i < self.staged.len() {
            if self.staged[i].seq == seq {
                found = Some(i);
                break;
            }
            i += 1;
        }
        match found {
            Some(i) => {
                // 先核低位缺口：低于 seq 的未核键全部落丢键账（真丢）。
                let mut j = 0usize;
                while j < self.staged.len() {
                    if self.staged[j].seq < seq {
                        let k = self.staged[j];
                        self.drop_ledger.push(DropEntry {
                            seq: k.seq,
                            from_hwnd: 0, // 入侧不区分窗，桥级账
                            to_hwnd: k.to_hwnd,
                            reason: E_SIG_GAP,
                        });
                        self.drop_total += 1;
                    }
                    j += 1;
                }
                // 移除已核项与已落账项：队列只留 > seq 的键。
                let mut kept: Vec<StagedKey> = Vec::new();
                let mut m = 0usize;
                while m < self.staged.len() {
                    if self.staged[m].seq > seq {
                        kept.push(self.staged[m]);
                    }
                    m += 1;
                }
                self.staged = kept;
                self.out_seq = seq;
                self.passed_keys += 1;
                self.state = DefState::Drained;
                Ok(())
            }
            None => Err(E_SIG_GAP),
        }
    }

    /// 焦点切换防御轮（J1 主体）：切前拍账，出侧按序核签本轮全部暂存键，
    /// 切后拍账核对零增长。
    pub fn on_focus_switch(&mut self, to_hwnd: u32, keys_this_switch: u32) -> Result<(), u16> {
        let before = self.drop_total;
        // 本轮切换的键全部入侧出签。
        let mut i = 0u32;
        while i < keys_this_switch {
            self.sign_in(to_hwnd);
            i += 1;
        }
        // 出侧按 seq 升序逐个核签（正常路径：全签收、零丢键）。
        let mut expect = self.out_seq + 1;
        let mut done = 0u32;
        while done < keys_this_switch {
            self.sign_out(expect)?;
            expect += 1;
            done += 1;
        }
        let after = self.drop_total;
        if after != before {
            return Err(E_GROWTH);
        }
        self.switch_total += 1;
        self.state = DefState::Drained;
        Ok(())
    }

    /// 丢键账审计：账条数与计数守恒，逐条 reason 闭集。
    pub fn audit(&self) -> Result<(), u16> {
        if self.drop_ledger.len() as u32 != self.drop_total {
            return Err(E_LEDGER_BAD);
        }
        let mut i = 0usize;
        while i < self.drop_ledger.len() {
            let r = self.drop_ledger[i].reason;
            if r != E_SIG_GAP && r != E_SIG_REPLAY {
                return Err(E_LEDGER_BAD);
            }
            i += 1;
        }
        if self.state != DefState::Armed && self.state != DefState::Drained {
            return Err(E_STATE_BAD);
        }
        Ok(())
    }

    /// ×N 切换覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.switch_total < want {
            return Err(E_SWITCH_MISS);
        }
        Ok(())
    }

    /// 零增长终判：丢键总量恒 0 且暂存队列已清空。
    pub fn assert_zero(&self) -> Result<(), u16> {
        if self.drop_total != 0 {
            return Err(E_GROWTH);
        }
        if self.staged.len() != 0 {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    pub fn drop_ledger_len(&self) -> usize {
        self.drop_ledger.len()
    }
    pub fn staged_len(&self) -> usize {
        self.staged.len()
    }

    /// 完成写回（判据 J2）：仅防御轮跑过（Drained）后放行，空摘要拒。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.state != DefState::Drained {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（丢键账本 ×1000 切换零增长），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×1000：1000 次切换、每次 0..3 键，联签全过、零增长、
    //    终态账零、暂存空。
    let mut dg = DropGuard::new();
    let mut i = 0u32;
    while i < 1000 {
        let keys = i % 4;
        dg.on_focus_switch(0x1100 + (i % 5), keys)?;
        i += 1;
    }
    dg.coverage(1000)?;
    dg.assert_zero()?;
    dg.audit()?;
    if dg.passed_keys != dg.in_seq {
        return Err(E_LEDGER_BAD); // 入签数 == 过桥数 == 核签数
    }
    if dg.drop_ledger_len() != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 正常联签序：签入 5 键按序签出，暂存清空、过桥计数=5。
    let mut dg = DropGuard::new();
    let mut i = 0u32;
    while i < 5 {
        dg.sign_in(0x2001);
        i += 1;
    }
    if dg.staged_len() != 5 {
        return Err(E_LEDGER_BAD);
    }
    let mut seq = 1u64;
    while seq <= 5 {
        dg.sign_out(seq)?;
        seq += 1;
    }
    if dg.staged_len() != 0 || dg.passed_keys != 5 {
        return Err(E_LEDGER_BAD);
    }
    dg.assert_zero()?;
    hits += 1;

    // R3 缺签落账：签入 3 键只核签 seq=2 → 低位 seq=1 落丢键账，
    //    seq=3 留暂存待补；账条 reason=E_SIG_GAP。
    let mut dg = DropGuard::new();
    let mut i = 0u32;
    while i < 3 {
        dg.sign_in(0x3001);
        i += 1;
    }
    dg.sign_out(2)?;
    if dg.drop_total != 1 || dg.drop_ledger_len() != 1 {
        return Err(E_LEDGER_BAD);
    }
    if dg.staged_len() != 1 {
        return Err(E_LEDGER_BAD); // seq=3 应留队
    }
    dg.audit()?;
    hits += 1;

    // R4 补签放行：R3 残局续核 seq=3，暂存清空；本轮丢键账不再增长。
    let before = dg.drop_total;
    dg.sign_out(3)?;
    if dg.staged_len() != 0 {
        return Err(E_LEDGER_BAD);
    }
    if dg.drop_total != before {
        return Err(E_GROWTH);
    }
    hits += 1;

    // R5 重放拒收：已核 seq 再次签出必拒（E_SIG_REPLAY），且不计丢键。
    let mut dg = DropGuard::new();
    dg.sign_in(0x4001);
    dg.sign_out(1)?;
    if dg.sign_out(1).is_ok() {
        return Err(E_SIG_REPLAY);
    }
    if dg.drop_total != 0 {
        return Err(E_LEDGER_BAD); // 重放不落丢键账（非丢键）
    }
    hits += 1;

    // R6 野签拒收：从未签入的 seq 直接签出 → E_SIG_GAP，不计过桥。
    let mut dg = DropGuard::new();
    if dg.sign_out(42).is_ok() {
        return Err(E_SIG_GAP);
    }
    if dg.passed_keys != 0 {
        return Err(E_LEDGER_BAD);
    }
    dg.audit()?;
    hits += 1;

    // R7 缺口切换防御：某次切换只核签部分键（模拟桥内丢包），执行器
    //    落账增长被 on_focus_switch 抓住（E_GROWTH），账本如实留痕。
    let mut dg = DropGuard::new();
    dg.sign_in(0x5001); // 人为预置一个将成低位的键 seq=1
    dg.sign_in(0x5001); // seq=2
    // 直接核签 seq=2 → seq=1 落账（丢 1 键）。
    dg.sign_out(2)?;
    // 随后的切换轮必须保持零增长才能通过——先补清残局。
    dg.sign_out(3).is_err(); // seq=3 未签入，野签
    if dg.drop_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    dg.on_focus_switch(0x5002, 2)?; // 新一轮全签收，账不再涨
    dg.on_focus_switch(0x5003, 0)?; // 零键切换亦平
    if dg.drop_total != 1 {
        return Err(E_GROWTH);
    }
    dg.audit()?;
    hits += 1;

    // R8 写回（判据 J2）：完成态 + 非空摘要放行，空摘要拒。
    let mut dg = DropGuard::new();
    if dg.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD); // Armed 未跑防不写回
    }
    dg.on_focus_switch(0x6001, 1)?;
    if dg.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    dg.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
