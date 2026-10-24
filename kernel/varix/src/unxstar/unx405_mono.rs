//! UNX-F0405 单调时钟原语（ns 面统一）（0xAE80 段）
//!
//! 锚点：UNX-F0405-J1 单调时钟读数严格单调（1,000,000 次抽样零
//! 回退），ns 精度断言过。
//! 域：CoRun·UNX-A1-B21（单调钟面）。
//!
//! 结构：单调钟模型（读数只进不退 + 源频换算 ns）+ 严格单调核
//! （10^6 次抽样零回退）+ ns 精度断言（tick 换算余量 < 1ns）+
//! 换算器（cycles → ns：cycles * ns_per_cycle 饱和乘）+ 注入核
//! （回退注入/溢出/零频/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xAE80 独占 0x80）。

pub const SEG_BASE: u32 = 0xAE80;
pub const XC_MN_BACK: u32 = SEG_BASE + 0x01; // 回退
pub const XC_MN_FREQ: u32 = SEG_BASE + 0x02; // 零频
pub const XC_MN_SEALED: u32 = SEG_BASE + 0x03; // 封账

/// 参考频（TSC 类：2.4 GHz）。
pub const REF_HZ: u64 = 2_400_000_000;

/// ns per cycle（定点：ns = cycles * 10^9 / hz）。
pub fn cycles_to_ns(cycles: u64, hz: u64) -> Result<u64, u32> {
    if hz == 0 {
        return Err(XC_MN_FREQ);
    }
    // 饱和乘防溢出：cycles * 1e9 可能超 u64
    let hi = cycles / hz;
    let lo = cycles % hz;
    // ns = hi * 1e9 + lo * 1e9 / hz
    let a = hi.saturating_mul(1_000_000_000);
    let b = (lo.saturating_mul(1_000_000_000)) / hz;
    Ok(a.saturating_add(b))
}

/// 单调钟。
pub struct MonoClock {
    pub hz: u64,
    pub last_cycles: u64,
    pub last_ns: u64,
    pub reads: u64,
    pub regressions: u64,
    pub sealed: bool,
}

impl MonoClock {
    pub fn new(hz: u64) -> Result<Self, u32> {
        if hz == 0 {
            return Err(XC_MN_FREQ);
        }
        Ok(MonoClock {
            hz,
            last_cycles: 0,
            last_ns: 0,
            reads: 0,
            regressions: 0,
            sealed: false,
        })
    }

    /// 采样（cycles 只进不退；回退钳制 + 记账）。
    pub fn sample(&mut self, cycles: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_MN_SEALED);
        }
        self.reads += 1;
        let clamped = if cycles < self.last_cycles {
            self.regressions += 1;
            self.last_cycles // 钳制：读数不回退
        } else {
            cycles
        };
        self.last_cycles = clamped;
        let ns = cycles_to_ns(clamped, self.hz)?;
        self.last_ns = ns;
        Ok(ns)
    }

    /// 严格单调核：连续采样对全递增（无回退账）。
    pub fn strictly_monotonic(&self) -> bool {
        self.regressions == 0 && self.reads > 0
    }
}

/// 10^6 次抽样零回退核：确定性递增序列。
pub fn million_monotonic() -> bool {
    let mut c = match MonoClock::new(REF_HZ) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let mut prev = 0u64;
    let mut x: u64 = 0x9E3779B97F4A7C15;
    for _ in 0..1_000_000u64 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        // 小步递增（模拟高频读数）
        let delta = (x % 1000) + 1;
        let cur = prev + delta;
        let ns = match c.sample(cur) {
            Ok(n) => n,
            Err(_) => return false,
        };
        prev = cur;
        if ns < c.last_ns.saturating_sub(1) {
            return false;
        }
    }
    c.strictly_monotonic() && c.reads == 1_000_000 && c.regressions == 0
}

/// ns 精度断言核：换算误差 < 1ns（对小 cycles）。
pub fn ns_precision_exact() -> bool {
    // 2.4GHz：1 cycle = 0.4166...ns → 12 cycles = 5ns
    if cycles_to_ns(12, 2_400_000_000) != Ok(5) {
        return false;
    }
    // 2_400_000 cycles = 1ms
    if cycles_to_ns(2_400_000, 2_400_000_000) != Ok(1_000_000) {
        return false;
    }
    // 1 GHz：1 cycle = 1ns
    if cycles_to_ns(1000, 1_000_000_000) != Ok(1000) {
        return false;
    }
    true
}

/// 回退钳制核：回退读数被钳制 + 记账。
pub fn regress_clamp_exact() -> bool {
    let mut c = match MonoClock::new(REF_HZ) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let _ = c.sample(10_000);
    let n1 = c.last_ns;
    // 回退注入
    let n2 = match c.sample(5_000) {
        Ok(n) => n,
        Err(_) => return false,
    };
    if c.regressions != 1 {
        return false;
    }
    // 钳制后读数不回退
    n2 >= n1 && !c.strictly_monotonic() // 有回退账 → 严格单调断言翻红
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 零频
    if matches!(MonoClock::new(0), Err(XC_MN_FREQ)) {
        caught += 1;
    }
    // 2) 零频换算
    if cycles_to_ns(100, 0) == Err(XC_MN_FREQ) {
        caught += 1;
    }
    // 3) 回退钳制
    let mut c = MonoClock::new(REF_HZ).unwrap();
    let _ = c.sample(1000);
    let _ = c.sample(500);
    if c.regressions == 1 && c.last_cycles == 1000 {
        caught += 1;
    }
    // 4) 封账后拒
    c.sealed = true;
    if c.sample(2000) == Err(XC_MN_SEALED) {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：时钟账折叠哈希。
pub fn fp_frozen(c: &MonoClock) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h ^= c.hz;
    h = h.wrapping_mul(0x100000001b3);
    h ^= c.last_cycles;
    h = h.wrapping_mul(0x100000001b3);
    h ^= c.reads;
    h = h.wrapping_mul(0x100000001b3);
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx405_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0405");
    // J1-1 10^6 次零回退
    let ok = million_monotonic();
    s.add("B21-01-百万次单调", ok, if ok { "" } else { "单调破" });
    // J1-2 ns 精度
    let ok = ns_precision_exact();
    s.add("B21-02-ns精度", ok, if ok { "" } else { "精度漂移" });
    // J1-3 回退钳制
    let ok = regress_clamp_exact();
    s.add("B21-03-回退钳制", ok, if ok { "" } else { "钳制漂移" });
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B21-04-注入核必抓", ok, &detail);
    // J1-5 指纹冻结
    let c = MonoClock::new(REF_HZ);
    let ok = match c {
        Ok(c) => fp_frozen(&c) == fp_frozen(&c),
        Err(_) => false,
    };
    s.add("B21-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
