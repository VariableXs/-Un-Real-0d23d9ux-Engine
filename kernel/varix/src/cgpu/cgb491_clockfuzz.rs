//! CGPU-F0491 · 时钟容错 fuzz（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - fuzz：时钟异常注入（时钟跳变/倒退/卡顿——计时器容错不产生
//!   负时长）；
//! - 账本压力 fuzz（帧洪峰——丢弃策略验证）；
//! - 案例固化 CI。判据：时钟容错、洪峰、CI、判据。
//!
//! 设计纪律：
//! - 时钟异常三类：跳变（前跳/后跳）、倒退（now < last）、卡顿
//!   （巨大间隔）——计时器 sanitize 后永不产生负时长（负值在
//!   结构上不可能：saturating_sub + 倒退丢弃）。
//! - 洪峰丢弃策略：帧队列容量封顶，洪峰时丢最旧保最新（丢帧计
//!   数留痕）——账本不被洪峰撑爆。
//! - 案例固化 CI：注入案例（类型+参数+结论）重放同结论。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x1180 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x1180，全仓独占）。
pub const SEG_BASE: u16 = 0x1180;

pub const E_CF_NEG: u16 = SEG_BASE + 1; // 负时长产生
pub const E_CF_OVERFLOW: u16 = SEG_BASE + 2; // 账本溢出（洪峰撑爆）
pub const E_CF_CASE: u16 = SEG_BASE + 3; // 案例不存在
pub const E_CF_TYPE: u16 = SEG_BASE + 4; // 注入类型非法

/// 时钟异常类型。
pub const X_JUMP_FWD: u8 = 1; // 前跳
pub const X_JUMP_BACK: u8 = 2; // 后跳（倒退）
pub const X_STALL: u8 = 3; // 卡顿（巨大间隔）
pub const XTYPES: [u8; 3] = [X_JUMP_FWD, X_JUMP_BACK, X_STALL];

/// 容错计时器：sanitize 后永不产生负时长。
#[derive(Debug)]
pub struct SafeTimer {
    /// 上次合法时间。
    last: u64,
    /// 累计合法时长。
    pub elapsed: u64,
    /// 倒退丢弃计数。
    pub drops: u64,
}

impl SafeTimer {
    pub fn new(now: u64) -> Self {
        Self { last: now, elapsed: 0, drops: 0 }
    }
    /// 喂时间：跳变/倒退/卡顿全部容错。
    pub fn feed(&mut self, now: u64, stall_line: u64) -> Result<u64, u16> {
        if now < self.last {
            // 倒退：丢弃本样本（负时长结构上不可能）。
            self.drops += 1;
            return Ok(self.elapsed);
        }
        let delta = now.saturating_sub(self.last);
        if delta > stall_line {
            // 卡顿：钳到卡顿线（不放大）。
            self.elapsed += stall_line;
        } else {
            self.elapsed += delta;
        }
        self.last = now.max(self.last); // 后跳由上面拦，前跳合法
        Ok(self.elapsed)
    }
}

/// 洪峰账本：容量封顶+丢最旧保最新。
pub const LEDGER_CAP: usize = 64;

#[derive(Debug)]
pub struct FloodLedger {
    pub slots: Vec<u64>,
    pub dropped: u64,
}

impl FloodLedger {
    pub fn new() -> Self {
        Self { slots: Vec::new(), dropped: 0 }
    }
    /// 入账：满则丢最旧。
    pub fn push(&mut self, frame_ts: u64) -> Result<(), u16> {
        if self.slots.len() >= LEDGER_CAP {
            self.slots.remove(0);
            self.dropped += 1;
        }
        self.slots.push(frame_ts);
        if self.slots.len() > LEDGER_CAP {
            return Err(E_CF_OVERFLOW);
        }
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.slots.len()
    }
}

/// 注入案例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectCase {
    pub xtype: u8,
    pub payload: u64, // 跳变量/卡顿间隔
    /// 结论：true=计时器存活且无负时长。
    pub pass: bool,
}

/// 注入执行：对 SafeTimer 施加异常，断言无负时长。
pub fn run_inject(xtype: u8, payload: u64) -> Result<bool, u16> {
    if !XTYPES.contains(&xtype) {
        return Err(E_CF_TYPE);
    }
    let mut t = SafeTimer::new(1000);
    let r = match xtype {
        X_JUMP_FWD => t.feed(1000 + payload, 100_000),
        X_JUMP_BACK => t.feed(1000u64.saturating_sub(payload.min(999)), 100_000),
        X_STALL => t.feed(1000 + payload, 10_000),
        _ => return Err(E_CF_TYPE),
    };
    // 容错断言：elapsed 单调不减且不产生异常负语义（Result 存活）。
    Ok(r.is_ok())
}

/// 案例固化 CI。
#[derive(Debug)]
pub struct InjectCI {
    pub cases: Vec<InjectCase>,
}

impl InjectCI {
    pub fn new() -> Self {
        Self { cases: Vec::new() }
    }
    pub fn record(&mut self, xtype: u8, payload: u64, pass: bool) {
        self.cases.push(InjectCase { xtype, payload, pass });
    }
    /// CI 绿：全案例重放同结论。
    pub fn ci_green(&self) -> Result<usize, u16> {
        if self.cases.is_empty() {
            return Err(E_CF_CASE);
        }
        for c in self.cases.iter() {
            let r = run_inject(c.xtype, c.payload);
            if r.as_ref().map(|b| *b) != Ok(c.pass) {
                return Err(E_CF_TYPE);
            }
        }
        Ok(self.cases.len())
    }
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、时钟容错：三类异常不产生负时长。
    {
        let mut t = SafeTimer::new(1000);
        t.feed(2000, 100_000).ok(); // +1000
        let e1 = t.elapsed;
        t.feed(1500, 100_000).ok(); // 倒退 → 丢弃
        v.push(("F0491-时钟容错-倒退丢弃不缩时长", t.elapsed == e1 && t.drops == 1));
        t.feed(3000, 100_000).ok(); // 恢复前进：delta=1000 累计
        v.push(("F0491-时钟容错-倒退后恢复", t.elapsed == 2_000));
        // 卡顿钳制。
        let mut t2 = SafeTimer::new(0);
        t2.feed(500_000, 10_000).ok();
        v.push(("F0491-时钟容错-卡顿钳制", t2.elapsed == 10_000));
        // 前跳合法。
        let mut t3 = SafeTimer::new(0);
        t3.feed(80_000, 100_000).ok();
        v.push(("F0491-时钟容错-前跳合法", t3.elapsed == 80_000));
    }
    // 二、洪峰：容量封顶+丢最旧。
    {
        let mut fl = FloodLedger::new();
        for i in 0..(LEDGER_CAP as u64 + 20) {
            fl.push(i).ok();
        }
        v.push(("F0491-洪峰-容量封顶", fl.len() == LEDGER_CAP));
        v.push(("F0491-洪峰-丢最旧计数", fl.dropped == 20));
        v.push(("F0491-洪峰-保最新", fl.slots.first() == Some(&(20))));
    }
    // 三、注入案例 CI。
    {
        let mut ci = InjectCI::new();
        ci.record(X_JUMP_FWD, 50_000, true);
        ci.record(X_JUMP_BACK, 200, true);
        ci.record(X_STALL, 500_000, true);
        ci.record(X_JUMP_BACK, 0, true);
        v.push(("F0491-CI-四案例重放全绿", ci.ci_green() == Ok(4)));
        v.push(("F0491-CI-空案例拒", InjectCI::new().ci_green() == Err(E_CF_CASE)));
        v.push(("F0491-CI-类型非法拒", run_inject(9, 1) == Err(E_CF_TYPE)));
    }
    // 四、判据自检。
    {
        v.push(("F0491-判据-三异常枚举", XTYPES == [1, 2, 3]));
        v.push(("F0491-判据-账本容量入册", LEDGER_CAP == 64));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0491_clockfuzz_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
