//! unx48345_stickykey — UNX-F48345 按键重复/粘滞键语义适配（REP 面与 M2 联签消费）（诊断段 0xCC00）。
//!
//! 判据原文：
//!   J1 适配 ×10 行为正确；判据锚定 §AI-61 §4 上游 AI-62。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 粘滞键三态机：修饰键 Idle→Latched（单击挂起）→Locked（再击
//!      锁定）→Idle（三击解锁）；普通键消耗全部 Latched 位，Locked
//!      位跨普通键存续至修饰键再击。
//!   2. REP 面纯数据：重复面只含 initial_delay/repeat_ms/max_events
//!      三参数 + 合法域校验，按 hold_ms 确定性推导事件数，不含驱动
//!      实现面。
//!   3. 适配 ×10：十例覆盖单挂/双挂/锁存/多修饰/重复节律/下限拒收
//!      等路径，逐例行为判据唯一确定，错一例即 E_SEQ_BAD。
//!   4. M2 联签消费：逐例适配回执 (案例指纹, 判定, 封印) 交 M2 消费
//!      入账，一例一签重签判重；回执篡改封印即失配。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCC00;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_PROFILE_BAD: u16 = SEG_BASE + 1; // REP 面参数出合法域
const E_SEQ_BAD: u16 = SEG_BASE + 2; // 适配行为与判据不符
const E_SEAL_BAD: u16 = SEG_BASE + 3; // 回执封印失配
const E_TAMPER: u16 = SEG_BASE + 4; // 三态机账面被改
const E_DUP_SIGN: u16 = SEG_BASE + 5; // 重复联签
const E_COVER_MISS: u16 = SEG_BASE + 6; // ×10 覆盖缺口
const E_STATE_BAD: u16 = SEG_BASE + 7; // 粘滞三态非法

/// 修饰键编号。
pub const M_SHIFT: u8 = 0;
pub const M_CTRL: u8 = 1;
pub const M_ALT: u8 = 2;
pub const MODS: u8 = 3;

/// 粘滞三态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ModState {
    Idle,
    Latched,
    Locked,
}

/// 粘滞键盘：三修饰键三态机 + 行为计数账。
pub struct StickyBoard {
    st: [ModState; 3],
    pub latches: u32,
    pub locks: u32,
    pub consumes: u32,
    pub plain_pass: u32,
}

impl StickyBoard {
    pub fn new() -> Self {
        StickyBoard { st: [ModState::Idle; 3], latches: 0, locks: 0, consumes: 0, plain_pass: 0 }
    }

    pub fn state(&self, m: u8) -> Result<ModState, u16> {
        if m as usize >= MODS as usize {
            return Err(E_STATE_BAD);
        }
        Ok(self.st[m as usize])
    }

    /// 修饰键击键：Idle→Latched，Latched→Locked，Locked→Idle（循环）。
    pub fn press_mod(&mut self, m: u8) -> Result<ModState, u16> {
        if m as usize >= MODS as usize {
            return Err(E_STATE_BAD);
        }
        let i = m as usize;
        let next = match self.st[i] {
            ModState::Idle => {
                self.latches += 1;
                ModState::Latched
            }
            ModState::Latched => {
                self.locks += 1;
                ModState::Locked
            }
            ModState::Locked => ModState::Idle,
        };
        self.st[i] = next;
        Ok(next)
    }

    /// 普通键击键：消耗全部 Latched（发出组合键），Locked 存续。
    /// 返回组合修饰键个数。
    pub fn press_normal(&mut self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MODS as usize {
            if self.st[i] == ModState::Latched {
                self.st[i] = ModState::Idle;
                n += 1;
            }
            i += 1;
        }
        if n > 0 {
            self.consumes += 1;
        } else {
            self.plain_pass += 1;
        }
        n
    }

    pub fn latched_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MODS as usize {
            if self.st[i] == ModState::Latched {
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn locked_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MODS as usize {
            if self.st[i] == ModState::Locked {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// REP 面：重复三参数 + 合法域校验。
#[derive(Clone, Copy)]
pub struct RepeatPlane {
    pub initial_delay: u32,
    pub repeat_ms: u32,
    pub max_events: u32,
}

impl RepeatPlane {
    pub fn validate(&self) -> Result<(), u16> {
        if self.initial_delay < 100 || self.initial_delay > 2000 {
            return Err(E_PROFILE_BAD);
        }
        if self.repeat_ms < 15 || self.repeat_ms > 500 {
            return Err(E_PROFILE_BAD);
        }
        if self.max_events < 1 || self.max_events > 64 {
            return Err(E_PROFILE_BAD);
        }
        if self.repeat_ms > self.initial_delay {
            return Err(E_PROFILE_BAD);
        }
        Ok(())
    }

    /// 持续 hold_ms 派生的重复事件数：0（未过首延）否则
    /// 1 + min(迟延内完整周期数, max_events-1)。
    pub fn hold_events(&self, hold_ms: u32) -> u32 {
        if hold_ms < self.initial_delay {
            return 0;
        }
        let extra = (hold_ms - self.initial_delay) / self.repeat_ms;
        let capped = if extra > self.max_events - 1 { self.max_events - 1 } else { extra };
        1 + capped
    }
}

/// FNV-1a 64 位指纹（u32 流）。
pub fn fp_u32s(vals: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < vals.len() {
        let mut b = 0usize;
        while b < 4 {
            h ^= ((vals[i] >> (b * 8)) & 0xFF) as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            b += 1;
        }
        i += 1;
    }
    h
}

/// 适配回执：案例 id + 判定（1=正确）+ 封印。
#[derive(Clone)]
pub struct AdaptRec {
    pub case_id: u32,
    pub verdict: u32,
    seal: u64,
}

impl AdaptRec {
    fn seal_of(case_id: u32, verdict: u32) -> u64 {
        fp_u32s(&[case_id, verdict, 0x5757_5757]).wrapping_mul(0x0000_0100_0000_01b3)
    }

    pub fn new(case_id: u32, verdict: u32) -> Self {
        AdaptRec { case_id, verdict, seal: Self::seal_of(case_id, verdict) }
    }

    pub fn verify(&self) -> Result<(), u16> {
        if self.seal != Self::seal_of(self.case_id, self.verdict) {
            return Err(E_SEAL_BAD);
        }
        Ok(())
    }
}

/// M2 消费方：逐例入账，重签判重。
pub struct M2Ledger {
    pub recs: Vec<AdaptRec>,
}

impl M2Ledger {
    pub fn new() -> Self {
        M2Ledger { recs: Vec::new() }
    }

    pub fn sign(&mut self, rec: AdaptRec) -> Result<(), u16> {
        rec.verify()?;
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].case_id == rec.case_id {
                return Err(E_DUP_SIGN);
            }
            i += 1;
        }
        self.recs.push(rec);
        Ok(())
    }
}

/// 适配案例执行器：每例构造确定场景并给出唯一正确行为判定。
/// 返回 1=行为正确（判据命中），否则报错。
pub fn run_case(case_id: u32) -> Result<u32, u16> {
    match case_id {
        // C1：Shift 挂起 + 普通键 → 消耗 1，Shift 回 Idle。
        1 => {
            let mut b = StickyBoard::new();
            b.press_mod(M_SHIFT)?;
            if b.state(M_SHIFT)? != ModState::Latched {
                return Err(E_SEQ_BAD);
            }
            let n = b.press_normal();
            if n != 1 || b.state(M_SHIFT)? != ModState::Idle || b.consumes != 1 {
                return Err(E_SEQ_BAD);
            }
            Ok(1)
        }
        // C2：Ctrl 挂起 + 普通键 → 消耗 1，Ctrl 回 Idle。
        2 => {
            let mut b = StickyBoard::new();
            b.press_mod(M_CTRL)?;
            let n = b.press_normal();
            if n != 1 || b.state(M_CTRL)? != ModState::Idle {
                return Err(E_SEQ_BAD);
            }
            Ok(1)
        }
        // C3：双击 Shift 锁定 → 普通键消耗但锁存续 → Shift 再击解锁。
        3 => {
            let mut b = StickyBoard::new();
            b.press_mod(M_SHIFT)?;
            if b.press_mod(M_SHIFT)? != ModState::Locked {
                return Err(E_SEQ_BAD);
            }
            if b.press_normal() != 1 || b.state(M_SHIFT)? != ModState::Locked {
                return Err(E_SEQ_BAD);
            }
            if b.press_mod(M_SHIFT)? != ModState::Idle {
                return Err(E_SEQ_BAD);
            }
            Ok(1)
        }
        // C4：三修饰全挂 + 一普通键 → 单次消耗 3，全回 Idle。
        4 => {
            let mut b = StickyBoard::new();
            b.press_mod(M_SHIFT)?;
            b.press_mod(M_CTRL)?;
            b.press_mod(M_ALT)?;
            if b.latched_count() != 3 || b.press_normal() != 3 || b.latched_count() != 0 {
                return Err(E_SEQ_BAD);
            }
            Ok(1)
        }
        // C5：REP 面节律：delay=200/repeat=100，hold=550 → 1+3=4。
        5 => {
            let p = RepeatPlane { initial_delay: 200, repeat_ms: 100, max_events: 64 };
            p.validate()?;
            if p.hold_events(550) != 4 {
                return Err(E_SEQ_BAD);
            }
            Ok(1)
        }
        // C6：hold 低于首延 → 0 事件。
        6 => {
            let p = RepeatPlane { initial_delay: 300, repeat_ms: 50, max_events: 8 };
            p.validate()?;
            if p.hold_events(299) != 0 || p.hold_events(0) != 0 {
                return Err(E_SEQ_BAD);
            }
            Ok(1)
        }
        // C7：重复事件封顶：hold 极长也只到 max_events。
        7 => {
            let p = RepeatPlane { initial_delay: 100, repeat_ms: 15, max_events: 5 };
            p.validate()?;
            if p.hold_events(100_000) != 5 {
                return Err(E_SEQ_BAD);
            }
            Ok(1)
        }
        // C8：REP 面出域拒收：repeat 超首延 / 出上下限。
        8 => {
            let p1 = RepeatPlane { initial_delay: 200, repeat_ms: 300, max_events: 8 };
            let p2 = RepeatPlane { initial_delay: 50, repeat_ms: 30, max_events: 8 };
            let p3 = RepeatPlane { initial_delay: 500, repeat_ms: 100, max_events: 0 };
            if p1.validate().is_ok() || p2.validate().is_ok() || p3.validate().is_ok() {
                return Err(E_SEQ_BAD);
            }
            Ok(1)
        }
        // C9：Idle 普通键直通 → 0 消耗 1 直通。
        9 => {
            let mut b = StickyBoard::new();
            if b.press_normal() != 0 || b.plain_pass != 1 || b.consumes != 0 {
                return Err(E_SEQ_BAD);
            }
            Ok(1)
        }
        // C10：混合节律：锁定 Ctrl + 挂起 Alt，两普通键——第一键耗
        // Alt（1 修饰）且 Ctrl 锁存续，第二键仍耗 1；后 Alt 早已 Idle。
        10 => {
            let mut b = StickyBoard::new();
            b.press_mod(M_CTRL)?;
            b.press_mod(M_CTRL)?;
            b.press_mod(M_ALT)?;
            if b.press_normal() != 1 || b.state(M_CTRL)? != ModState::Locked {
                return Err(E_SEQ_BAD);
            }
            if b.press_normal() != 1 || b.locked_count() != 1 {
                return Err(E_SEQ_BAD);
            }
            if b.press_mod(M_CTRL)? != ModState::Idle {
                return Err(E_SEQ_BAD);
            }
            Ok(1)
        }
        _ => Err(E_COVER_MISS),
    }
}

/// drill：J1 适配 ×10 行为正确 + M2 联签消费。
pub fn drill() -> Result<u32, u16> {
    let mut ledger = M2Ledger::new();

    // ── R1 主判据：十例逐跑，行为判定唯一为 1，逐例入 M2 账 ──
    let mut cid = 1u32;
    while cid <= 10 {
        let v = run_case(cid)?;
        if v != 1 {
            return Err(E_SEQ_BAD);
        }
        ledger.sign(AdaptRec::new(cid, v))?;
        cid += 1;
    }
    if ledger.recs.len() != 10 {
        return Err(E_COVER_MISS);
    }

    // ── R2 重签判重：同例再签必拒 ────────────────────────────
    if ledger.sign(AdaptRec::new(3, 1)) != Err(E_DUP_SIGN) {
        return Err(E_DUP_SIGN);
    }

    // ── R3 回执封印：篡改 verdict 或 seal 即失配 ─────────────
    let mut bad = AdaptRec::new(7, 1);
    bad.verdict = 0;
    if bad.verify() != Err(E_SEAL_BAD) {
        return Err(E_SEAL_BAD);
    }
    let mut bad2 = AdaptRec::new(7, 1);
    bad2.seal = bad2.seal.wrapping_add(1);
    if bad2.verify() != Err(E_SEAL_BAD) {
        return Err(E_SEAL_BAD);
    }

    // ── R4 越界案例：0 与 11 号案例必拒 ──────────────────────
    if run_case(0).is_ok() || run_case(11).is_ok() {
        return Err(E_COVER_MISS);
    }

    // ── R5 粘滞账自洽：latches = consumes + 锁存现存量（10 例已跑
    // 后的全局键盘无独立残留，用新板核对守恒）─────────────────
    let mut b = StickyBoard::new();
    b.press_mod(M_SHIFT)?;
    b.press_mod(M_CTRL)?;
    b.press_normal();
    // latches=2，消耗 1（Shift），Ctrl 仍 Latched → 守恒 2=1+1。
    if b.latches != b.consumes + b.latched_count() {
        return Err(E_TAMPER);
    }
    b.press_normal();
    if b.latches != b.consumes {
        return Err(E_TAMPER);
    }

    // ── R6 REP 面单调性：hold 单调不减、跨首延必非零 ─────────
    let p = RepeatPlane { initial_delay: 200, repeat_ms: 100, max_events: 16 };
    p.validate()?;
    if p.hold_events(100) > p.hold_events(150) || p.hold_events(400) < p.hold_events(300) {
        return Err(E_SEQ_BAD);
    }
    if p.hold_events(200) != 1 {
        return Err(E_SEQ_BAD);
    }

    // J2 写回摘要非空：十例联签账即摘要实质。
    if ledger.recs.is_empty() {
        return Err(E_COVER_MISS);
    }
    Ok(ledger.recs.len() as u32)
}
