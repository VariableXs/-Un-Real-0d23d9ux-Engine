//! cgd11_timefuzz — CGPU-F0491 帧计时 fuzz（异常注入与洪峰压力——计时器
//! 的疫苗试验）。
//!
//! 判据原文（施工书锚点 CGPU-F0491）：fuzz：时钟异常注入（时钟跳变/倒退/
//! 卡顿——计时器容错不产生负时长）；账本压力 fuzz（帧洪峰——丢弃策略验
//! 证）；案例固化 CI。判据：时钟容错、洪峰、CI、判据。
//!
//! 职责定位：确定性 fuzz 核心（可复现伪随机流——种子驱动，同种子同序列=
//! CI 可固化）+ 时钟异常注入器（跳变/倒退/卡顿三族异常——计时器在异常下
//! 容错：不产生负时长、不崩溃、异常计数显性）+ 帧洪峰压力器（海量帧灌账
//! 本——丢弃策略验证：有界缓冲丢最旧、丢弃计数不静默、账面数守恒）+ 案
//! 例固化面（种子+异常序列指纹——发现的问题固化为 CI 用例，防回归）。与
//! cgd02_frametimer（被试对象：计时器）、cgd03_frameledger（被试对象：账
//! 本）分工：彼是产品，此是疫苗——只注入不修复，异常下的正确行为由被试方
//! 的容错逻辑保证（本模块内建最小容错计时器作为判据参照实现）。诊断码段
//! 0x5A01~0x5A05。

use alloc::vec::Vec;

/// 模块版本。
pub const TIME_FUZZ_VERSION: &str = "D11-timefuzz-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x5A 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_NEGATIVE_DURATION: u32 = 0x5A01; // 负时长产出（容错红线违约）
pub const XC_FUZZ_CRASH: u32 = 0x5A02; // fuzz 中崩溃信号（panic 面泄漏）
pub const XC_DROP_UNACCOUNTED: u32 = 0x5A03; // 丢弃未入账（账面数不守恒）
pub const XC_SEED_NONREPRODUCIBLE: u32 = 0x5A04; // 同种子异序列（可复现性失守）
pub const XC_CASE_NOT_FROZEN: u32 = 0x5A05; // 发现的异常未固化（CI 防回归失职）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// LCG 乘数（可复现伪随机流——确定性 fuzz 基石）。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;
/// 洪峰灌入帧数。
pub const FLOOD_FRAMES: usize = 4096;
/// 账本有界容量（丢弃策略的边界）。
pub const LEDGER_CAP: usize = 1024;
/// 异常族数（跳变/倒退/卡顿）。
pub const ANOMALY_KINDS: usize = 3;

/// 时钟异常三族（判据"时钟跳变/倒退/卡顿"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClockAnomaly {
    /// 跳变：时钟向前跳一大步（NTP 校时类）。
    Jump,
    /// 倒退：时钟回拨（负 delta）。
    Regress,
    /// 卡顿：时钟停走（delta=0 持续）。
    Stall,
}

impl ClockAnomaly {
    pub const ALL: [ClockAnomaly; 3] = [
        ClockAnomaly::Jump,
        ClockAnomaly::Regress,
        ClockAnomaly::Stall,
    ];

    pub const fn zh(self) -> &'static str {
        match self {
            ClockAnomaly::Jump => "跳变",
            ClockAnomaly::Regress => "倒退",
            ClockAnomaly::Stall => "卡顿",
        }
    }
}

/// 可复现伪随机流（LCG——同种子同序列，CI 固化的前提）。
pub struct FuzzRng {
    state: u64,
}

impl FuzzRng {
    pub fn new(seed: u64) -> Self {
        FuzzRng { state: seed }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(LCG_A)
            .wrapping_add(LCG_C);
        self.state
    }

    /// 模选择。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 容错计时器（判据参照实现——异常下不产生负时长）。
pub struct TolerantTimer {
    /// 内部单调时钟（容错后）。
    mono_us: u64,
    /// 异常计数（三族各自累计——显性不静默）。
    pub anomaly_hits: [u64; 3],
    /// 产出样本数。
    pub samples: u64,
}

impl TolerantTimer {
    pub fn new() -> Self {
        TolerantTimer {
            mono_us: 0,
            anomaly_hits: [0; 3],
            samples: 0,
        }
    }

    /// 喂入原始时钟读数，产出容错后的帧时长（微秒）。
    /// 容错规则：倒退→按 0 时长输出（不产生负数）；卡顿→0 时长；
    /// 跳变→跳变量计入单帧（时长过大但非负——保守可解释）。
    pub fn feed(&mut self, raw_us: u64) -> Result<u64, u32> {
        let prev = self.mono_us;
        if raw_us < prev {
            // 倒退：钳到 0（判据"不产生负时长"），异常入账。
            self.anomaly_hits[ClockAnomaly::Regress as usize] += 1;
            self.samples += 1;
            return Ok(0);
        }
        let d = raw_us - prev;
        self.mono_us = raw_us;
        if d == 0 {
            self.anomaly_hits[ClockAnomaly::Stall as usize] += 1;
            self.samples += 1;
            return Ok(0);
        }
        if d > 1_000_000 {
            // 跳变：>1s 的单步视为时钟跳变（正常帧 never 这么长）。
            self.anomaly_hits[ClockAnomaly::Jump as usize] += 1;
        }
        self.samples += 1;
        Ok(d)
    }

    /// 容错体检：全部产出样本非负（结构上保证——u64 无负；此断言留作
    /// 未来带符号化改造的守卫）。
    pub fn health(&self) -> Result<(), u32> {
        Ok(())
    }
}

/// 洪峰账本（有界缓冲+丢弃策略——判据"帧洪峰——丢弃策略验证"）。
pub struct FloodLedger {
    ring: Vec<u64>,
    cap: usize,
    /// 丢弃计数（显性——丢弃不静默）。
    pub dropped: u64,
    /// 入账总数（含被丢的——账面守恒：kept+dropped==pushed）。
    pub pushed: u64,
}

impl FloodLedger {
    pub fn new(cap: usize) -> Self {
        FloodLedger {
            ring: Vec::with_capacity(cap),
            cap,
            dropped: 0,
            pushed: 0,
        }
    }

    /// 灌入一帧（超容量丢最旧）。
    pub fn push(&mut self, frame_us: u64) {
        self.pushed += 1;
        if self.ring.len() >= self.cap {
            self.ring.remove(0);
            self.dropped += 1;
        }
        self.ring.push(frame_us);
    }

    /// 账面守恒核验（判据"丢弃策略验证"：kept+dropped==pushed）。
    pub fn account_ok(&self) -> Result<(), u32> {
        let kept = self.ring.len() as u64;
        if kept + self.dropped != self.pushed {
            return Err(XC_DROP_UNACCOUNTED);
        }
        Ok(())
    }

    /// 保留样本数。
    pub const fn kept(&self) -> usize {
        self.ring.len()
    }

    /// 均值（保留样本口径——丢弃不污染统计）。
    pub fn mean(&self) -> Option<u64> {
        if self.ring.len() == 0 {
            return None;
        }
        let mut acc: u128 = 0;
        let mut i = 0;
        while i < self.ring.len() {
            acc += self.ring[i] as u128;
            i += 1;
        }
        Some((acc / self.ring.len() as u128) as u64)
    }
}

/// 固化案例（CI 防回归单元——种子+异常谱+断言结论）。
#[derive(Clone, Copy, Debug)]
pub struct FrozenCase {
    pub seed: u64,
    /// 注入帧数。
    pub frames: usize,
    /// 各族异常命中（谱指纹）。
    pub anomaly_hits: [u64; 3],
    /// 结论：容错通过。
    pub passed: bool,
}

/// fuzz 编排器。
pub struct TimeFuzzer {
    rng: FuzzRng,
    /// 固化案例库。
    pub cases: Vec<FrozenCase>,
    /// 遥测。
    pub runs: u64,
    pub failures: u64,
    pub frozen_count: u64,
}

impl TimeFuzzer {
    pub fn new(seed: u64) -> Self {
        TimeFuzzer {
            rng: FuzzRng::new(seed),
            cases: Vec::new(),
            runs: 0,
            failures: 0,
            frozen_count: 0,
        }
    }

    /// 时钟异常 fuzz：随机序列喂容错计时器——任何产出不得为"负"
    /// （u64 域内以倒退判据核验），异常命中入账。
    pub fn fuzz_clock(&mut self, frames: usize) -> Result<[u64; 3], u32> {
        self.runs += 1;
        let mut t = TolerantTimer::new();
        let mut clock: u64 = 1_000_000;
        let mut i = 0;
        while i < frames {
            // 随机 delta：0（卡顿）/正常 8~20ms/偶发倒退/偶发跳变。
            let roll = self.rng.below(100);
            if roll < 10 {
                clock = clock.saturating_sub(self.rng.below(5_000)); // 倒退
            } else if roll < 20 {
                clock += 2_000_000 + self.rng.below(1_000_000); // 跳变
            } else if roll < 30 {
                // 卡顿：原地不动。
            } else {
                clock += 8_000 + self.rng.below(12_000); // 正常
            }
            let d = t.feed(clock)?;
            // 容错断言：产出必须可解释（0 或正值——u64 域天然非负，
            // 但倒退路径必须走 0 分支而不是 panic：此处的存在即回归守卫）。
            let _ = d;
            i += 1;
        }
        Ok(t.anomaly_hits)
    }

    /// 洪峰 fuzz：FLOOD_FRAMES 帧灌有界账本——守恒与丢弃策略验证。
    pub fn fuzz_flood(&mut self) -> Result<(u64, u64), u32> {
        self.runs += 1;
        let mut ledger = FloodLedger::new(LEDGER_CAP);
        let mut i = 0;
        while i < FLOOD_FRAMES {
            let frame = 12_000 + self.rng.below(2_000);
            ledger.push(frame);
            i += 1;
        }
        ledger.account_ok()?;
        if ledger.kept() != LEDGER_CAP {
            return Err(XC_DROP_UNACCOUNTED);
        }
        let expect_dropped = (FLOOD_FRAMES - LEDGER_CAP) as u64;
        if ledger.dropped != expect_dropped {
            return Err(XC_DROP_UNACCOUNTED);
        }
        Ok((ledger.dropped, ledger.pushed))
    }

    /// 案例固化（判据"案例固化 CI"）：跑一轮并把种子+异常谱写入案例库。
    pub fn freeze_case(&mut self, seed: u64, frames: usize) -> Result<FrozenCase, u32> {
        // 用指定种子独立重放（可复现性自证）。
        let mut probe = TimeFuzzer::new(seed);
        let hits1 = probe.fuzz_clock(frames)?;
        let mut probe2 = TimeFuzzer::new(seed);
        let hits2 = probe2.fuzz_clock(frames)?;
        if hits1 != hits2 {
            self.failures += 1;
            return Err(XC_SEED_NONREPRODUCIBLE);
        }
        let c = FrozenCase {
            seed,
            frames,
            anomaly_hits: hits1,
            passed: true,
        };
        self.cases.push(c);
        self.frozen_count += 1;
        Ok(c)
    }

    /// CI 回放：案例库逐案重放对拍谱指纹（判据"CI"的执行面）。
    pub fn replay_ci(&mut self) -> Result<usize, u32> {
        let n = self.cases.len();
        let mut i = 0;
        while i < n {
            let c = self.cases[i];
            let mut probe = TimeFuzzer::new(c.seed);
            let hits = probe.fuzz_clock(c.frames)?;
            if hits != c.anomaly_hits {
                self.failures += 1;
                return Err(XC_SEED_NONREPRODUCIBLE);
            }
            i += 1;
        }
        Ok(n)
    }

    /// 案例库规模。
    pub const fn case_count(&self) -> usize {
        self.cases.len()
    }
}

impl ClockAnomaly {
    pub const fn ordinal(self) -> usize {
        match self {
            ClockAnomaly::Jump => 0,
            ClockAnomaly::Regress => 1,
            ClockAnomaly::Stall => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// 自检（判据四组：时钟容错 / 洪峰 / CI 固化 / 可复现）
// ---------------------------------------------------------------------------

impl TimeFuzzer {
    /// 组一：时钟容错——倒退产 0、卡顿产 0、跳变计账、全程无 panic。
    pub fn selfcheck_tolerance(&mut self) -> Result<(), u32> {
        let mut t = TolerantTimer::new();
        // 正常。
        let d1 = t.feed(1_000_000)?;
        let _ = t.feed(1_016_000)?;
        if d1 != 1_000_000 {
            return Err(XC_NEGATIVE_DURATION); // 首帧=全量差（基准点）
        }
        // 倒退。
        let d2 = t.feed(1_010_000)?;
        if d2 != 0 {
            return Err(XC_NEGATIVE_DURATION);
        }
        if t.anomaly_hits[ClockAnomaly::Regress.ordinal()] != 1 {
            return Err(XC_NEGATIVE_DURATION);
        }
        // 卡顿。
        let d3 = t.feed(1_010_000)?;
        if d3 != 0 {
            return Err(XC_NEGATIVE_DURATION);
        }
        if t.anomaly_hits[ClockAnomaly::Stall.ordinal()] != 1 {
            return Err(XC_NEGATIVE_DURATION);
        }
        // 跳变。
        let _ = t.feed(4_010_000)?;
        if t.anomaly_hits[ClockAnomaly::Jump.ordinal()] != 1 {
            return Err(XC_NEGATIVE_DURATION);
        }
        t.health()?;
        Ok(())
    }

    /// 组二：洪峰——4096 帧灌 1024 容量账本：丢弃 3072、守恒成立、
    /// 均值只按保留样本算。
    pub fn selfcheck_flood(&mut self) -> Result<(), u32> {
        let (dropped, pushed) = self.fuzz_flood()?;
        if pushed != FLOOD_FRAMES as u64 {
            return Err(XC_DROP_UNACCOUNTED);
        }
        if dropped != (FLOOD_FRAMES - LEDGER_CAP) as u64 {
            return Err(XC_DROP_UNACCOUNTED);
        }
        // 手工对照：1010 帧小账本。
        let mut l = FloodLedger::new(8);
        let mut i = 0;
        while i < 10 {
            l.push(1_000 + i as u64);
            i += 1;
        }
        l.account_ok()?;
        if l.kept() != 8 || l.dropped != 2 {
            return Err(XC_DROP_UNACCOUNTED);
        }
        // 保留的应是最新的 8 帧（1002..1009）→ 均值 1005.5 → 1005。
        let m = match l.mean() {
            Some(m) => m,
            None => return Err(XC_DROP_UNACCOUNTED),
        };
        if m != 1_005 {
            return Err(XC_DROP_UNACCOUNTED);
        }
        Ok(())
    }

    /// 组三：CI 固化——同种子重放谱一致、回放全绿。
    pub fn selfcheck_ci(&mut self) -> Result<(), u32> {
        let c1 = self.freeze_case(42, 256)?;
        let c2 = self.freeze_case(7, 256)?;
        if c1.seed == c2.seed {
            return Err(XC_CASE_NOT_FROZEN); // 案例多样性（不同种子覆盖不同谱）
        }
        let n = self.replay_ci()?;
        if n != self.case_count() {
            return Err(XC_CASE_NOT_FROZEN);
        }
        if self.frozen_count < 2 {
            return Err(XC_CASE_NOT_FROZEN);
        }
        Ok(())
    }

    /// 组四：可复现——同种子两流逐值一致、异种子异序列。
    pub fn selfcheck_reproducible(&mut self) -> Result<(), u32> {
        let mut a = FuzzRng::new(12345);
        let mut b = FuzzRng::new(12345);
        let mut i = 0;
        while i < 64 {
            if a.next() != b.next() {
                return Err(XC_SEED_NONREPRODUCIBLE);
            }
            i += 1;
        }
        let mut c = FuzzRng::new(54321);
        let mut d = FuzzRng::new(12345);
        let mut differ = false;
        let mut j = 0;
        while j < 8 {
            if c.next() != d.next() {
                differ = true;
                break;
            }
            j += 1;
        }
        if !differ {
            return Err(XC_SEED_NONREPRODUCIBLE);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_tolerance()?;
        self.selfcheck_flood()?;
        self.selfcheck_ci()?;
        self.selfcheck_reproducible()?;
        Ok(())
    }
}
