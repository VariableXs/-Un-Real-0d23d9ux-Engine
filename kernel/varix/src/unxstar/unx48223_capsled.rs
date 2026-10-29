//! unx48223_capsled — UNX-F48223 CapsLock 英文锁定与 LED 同步（M2 LED 回写单点消费）（诊断段 0xBCC0）。
//!
//! 判据原文：
//!   J1 同步 ×100 一致、写失败降级记账；判据锚定 §AI-61 §2 专题五、M2 联签。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 锁定语义：CapsLock 按一次翻转英文锁定态（Off↔On），锁定态是
//!      唯一事实源；LED 只是锁定态的回写投影，禁止反向驱动。
//!   2. M2 联签单点消费：每次锁定态翻转恰好发一条 LED 回写命令（单调
//!      seq 联签），命令只允许被硬件面消费一次——重消费（重复点亮同一
//!      seq）与漏消费（翻转后命令滞留）都算失步，由消费账守恒抓。
//!   3. 写失败降级：LED 写失败时命令转入降级账（不丢 seq、不重发旧
//!      命令），LED 停留在旧态并明确记 stale 标志；写恢复后的第一条
//!      命令必须携带当前锁定态全量（幂等全量回写），恢复即重同步。
//!   4. 同步口径（J1 ×100）：100 次翻转逐次断言 LED 投影==锁定态；
//!      写失败段内允许 stale，恢复段内必须回到一致，降级账条数与
//!      失败次数守恒。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBCC0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_STATE_BAD: u16 = SEG_BASE + 1;  // 锁定态机非法迁移
const E_LED_DRIFT: u16 = SEG_BASE + 2;  // LED 投影与锁定态失步
const E_DOUBLE_CONSUME: u16 = SEG_BASE + 3; // 命令重消费
const E_CMD_STUCK: u16 = SEG_BASE + 4;  // 命令漏消费（滞留）
const E_DEGRADE_BAD: u16 = SEG_BASE + 5;// 降级账与失败次数不平
const E_SYNC_MISS: u16 = SEG_BASE + 6;  // ×100 同步样本不足
const E_EMPTY_SUM: u16 = SEG_BASE + 7;  // 写回摘要为空

/// CapsLock 锁定态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockState {
    Off,
    On,
}

/// 一条 LED 回写命令（M2 联签载荷）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedCmd {
    pub seq: u64,
    pub on: bool,
}

/// 命令消费态：Issued 已发 / Consumed 已单点消费 / Degraded 降级挂账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmdState {
    Issued,
    Consumed,
    Degraded,
}

/// 降级账条目。
#[derive(Clone, Copy, Debug)]
pub struct DegradeEntry {
    pub seq: u64,
    pub stale_led: bool, // LED 停旧态
}

/// CapsLock↔LED 同步枢纽。
pub struct CapsLed {
    pub lock: LockState,
    pub led: LockState,      // 投影
    pub led_stale: bool,     // 写失败期 stale 标志
    pub cmd_seq: u64,        // 已发命令单调 seq
    pub last_cmd: Option<LedCmd>,
    pub last_cmd_state: CmdState,
    pub consumed_total: u32,
    pub degrade_ledger: Vec<DegradeEntry>,
    pub fail_total: u32,
    pub toggle_total: u32,
}

impl CapsLed {
    pub fn new() -> CapsLed {
        CapsLed {
            lock: LockState::Off,
            led: LockState::Off,
            led_stale: false,
            cmd_seq: 0,
            last_cmd: None,
            last_cmd_state: CmdState::Consumed, // 初态命令视作已消费（无回写需求）
            consumed_total: 1,
            degrade_ledger: Vec::new(),
            fail_total: 0,
            toggle_total: 0,
        }
    }

    /// CapsLock 翻转：锁定态取反 + 发一条联签命令（单点待消费）。
    pub fn toggle(&mut self) -> Result<LockState, u16> {
        self.lock = match self.lock {
            LockState::Off => LockState::On,
            LockState::On => LockState::Off,
        };
        self.cmd_seq += 1;
        self.last_cmd = Some(LedCmd { seq: self.cmd_seq, on: self.lock == LockState::On });
        self.last_cmd_state = CmdState::Issued;
        self.toggle_total += 1;
        Ok(self.lock)
    }

    /// 硬件面单点消费：只有 Issued 态命令可消费，消费即投影刷新。
    pub fn consume_cmd(&mut self) -> Result<LedCmd, u16> {
        match (self.last_cmd, self.last_cmd_state) {
            (Some(c), CmdState::Issued) => {
                self.led = if c.on { LockState::On } else { LockState::Off };
                self.led_stale = false; // 全量幂等回写，恢复即重同步
                self.last_cmd_state = CmdState::Consumed;
                self.consumed_total += 1;
                Ok(c)
            }
            (Some(_), CmdState::Consumed) => Err(E_DOUBLE_CONSUME),
            (Some(_), CmdState::Degraded) => Err(E_DEGRADE_BAD),
            (None, _) => Err(E_CMD_STUCK),
        }
    }

    /// 硬件面写失败：命令转降级账，LED 停旧态 + stale 置位。
    pub fn fail_cmd(&mut self) -> Result<(), u16> {
        match (self.last_cmd, self.last_cmd_state) {
            (Some(c), CmdState::Issued) => {
                self.degrade_ledger.push(DegradeEntry { seq: c.seq, stale_led: true });
                self.led_stale = true; // LED 停留在旧态（不应用新值）
                self.last_cmd_state = CmdState::Degraded;
                self.fail_total += 1;
                Ok(())
            }
            _ => Err(E_STATE_BAD),
        }
    }

    /// 同步断言：非 stale 期 LED 投影必须等于锁定态。
    pub fn assert_in_sync(&self) -> Result<(), u16> {
        if !self.led_stale && self.led != self.lock {
            return Err(E_LED_DRIFT);
        }
        Ok(())
    }

    /// 消费守恒：翻转数 == 已发命令数 == 消费数 + 降级数。
    pub fn audit(&self) -> Result<(), u16> {
        let issued = self.toggle_total as u64;
        if self.cmd_seq != issued {
            return Err(E_CMD_STUCK);
        }
        let degraded = self.degrade_ledger.len() as u32;
        if self.consumed_total - 1 + degraded != self.toggle_total {
            return Err(E_DOUBLE_CONSUME);
        }
        if self.fail_total != degraded {
            return Err(E_DEGRADE_BAD);
        }
        Ok(())
    }

    /// ×N 同步覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.toggle_total < want {
            return Err(E_SYNC_MISS);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.toggle_total == 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（同步 ×100 一致、写失败降级记账），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主同步 ×100：100 次翻转，每次翻转→消费→同步断言，逐次一致。
    let mut cl = CapsLed::new();
    let mut i = 0u32;
    while i < 100 {
        let want_on = i % 2 == 0; // 第 1 次→On，第 2 次→Off，交替
        let st = cl.toggle()?;
        let expect = if want_on { LockState::On } else { LockState::Off };
        if st != expect {
            return Err(E_STATE_BAD);
        }
        cl.consume_cmd()?;
        cl.assert_in_sync()?;
        i += 1;
    }
    cl.coverage(100)?;
    cl.audit()?;
    if cl.led != LockState::Off {
        return Err(E_LED_DRIFT); // 100 次偶数翻转终态 Off
    }
    hits += 1;

    // R2 写失败降级：翻转→失败→降级账落条、LED stale 停旧态；
    //    下一次翻转→成功→全量回写恢复一致。
    let mut cl = CapsLed::new();
    cl.toggle()?; // seq=1 → On
    cl.fail_cmd()?;
    if cl.led_stale != true {
        return Err(E_DEGRADE_BAD);
    }
    if cl.led != LockState::Off {
        return Err(E_LED_DRIFT); // 旧态：仍 Off
    }
    if cl.degrade_ledger.len() != 1 || cl.degrade_ledger[0].seq != 1 {
        return Err(E_DEGRADE_BAD);
    }
    cl.assert_in_sync()?; // stale 期放行
    // 降级命令不得被消费（已 Degraded）。
    if cl.consume_cmd().is_ok() {
        return Err(E_DOUBLE_CONSUME);
    }
    // 恢复：再翻转（seq=2 → Off）并成功消费，全量回写带来当前态。
    cl.toggle()?;
    cl.consume_cmd()?;
    if cl.led_stale {
        return Err(E_DEGRADE_BAD);
    }
    if cl.led != LockState::Off {
        return Err(E_LED_DRIFT);
    }
    cl.audit()?;
    hits += 1;

    // R3 重消费拦截：同一命令消费两次必拒（单点消费红线）。
    let mut cl = CapsLed::new();
    cl.toggle()?;
    cl.consume_cmd()?;
    if cl.consume_cmd().is_ok() {
        return Err(E_DOUBLE_CONSUME);
    }
    cl.audit()?;
    hits += 1;

    // R4 间歇失败流：每 3 次翻转注入 1 次写失败，共 12 次翻转 4 次失败，
    //    失败后均以成功翻转恢复，终态一致、降级账 4 条守恒。
    let mut cl = CapsLed::new();
    let mut i = 0u32;
    let mut fails = 0u32;
    while i < 12 {
        cl.toggle()?;
        if i % 3 == 2 {
            cl.fail_cmd()?;
            fails += 1;
            // 降级即补一轮恢复翻转。
            cl.toggle()?;
            cl.consume_cmd()?;
        } else {
            cl.consume_cmd()?;
        }
        cl.assert_in_sync()?;
        i += 1;
    }
    if cl.fail_total != 4 || fails != 4 {
        return Err(E_DEGRADE_BAD);
    }
    if cl.degrade_ledger.len() != 4 {
        return Err(E_DEGRADE_BAD);
    }
    cl.audit()?;
    cl.coverage(12)?;
    hits += 1;

    // R5 连锁失败段：连续 3 次写失败（LED 长期 stale 停初态），随后
    //    连续恢复翻转重同步；账目逐条 seq 连续可回放。
    let mut cl = CapsLed::new();
    let mut i = 0u32;
    while i < 3 {
        cl.toggle()?;
        cl.fail_cmd()?;
        i += 1;
    }
    if cl.degrade_ledger.len() != 3 {
        return Err(E_DEGRADE_BAD);
    }
    if cl.degrade_ledger[0].seq != 1 || cl.degrade_ledger[1].seq != 2 || cl.degrade_ledger[2].seq != 3 {
        return Err(E_DEGRADE_BAD);
    }
    if cl.led != LockState::Off || !cl.led_stale {
        return Err(E_LED_DRIFT);
    }
    let mut i = 0u32;
    while i < 2 {
        cl.toggle()?;
        cl.consume_cmd()?;
        i += 1;
    }
    if cl.led_stale {
        return Err(E_DEGRADE_BAD);
    }
    cl.audit()?;
    hits += 1;

    // R6 空消费拦截：未翻转先消费必拒（漏消费口径的反向防线）。
    let mut cl = CapsLed::new();
    if cl.consume_cmd().is_ok() {
        return Err(E_CMD_STUCK);
    }
    if cl.toggle_total != 0 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R7 锁定态投影独立：锁定态翻转不被 LED 失败回拖（事实源单向）。
    let mut cl = CapsLed::new();
    cl.toggle()?; // On
    cl.fail_cmd()?; // LED 没跟上
    if cl.lock != LockState::On {
        return Err(E_STATE_BAD); // 事实源不受投影失败影响
    }
    cl.toggle()?; // Off
    cl.consume_cmd()?;
    if cl.lock != LockState::Off || cl.led != LockState::Off {
        return Err(E_LED_DRIFT);
    }
    hits += 1;

    // R8 写回（判据 J2）：完成态 + 非空摘要放行，空摘要与零翻转拒。
    let mut cl = CapsLed::new();
    if cl.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD); // 零翻转不得写回
    }
    cl.toggle()?;
    cl.consume_cmd()?;
    if cl.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    cl.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
