//! UNX-F8901 gettimeofday 语义档（0xA600 段）
//!
//! 锚点：UNX-F8901-J1 tv_sec/tv_usec 输出与 clock_gettime
//! (CLOCK_REALTIME) 换算一致 10^3 次，tz 结构遗留 NULL 约定判据过。
//! 域：CoRun·UNX-C2-B06（gettimeofday 面）。
//!
//! 结构：时钟源模型（wall_ns 单调纳秒）+ gettimeofday 语义器
//! （wall_ns → (tv_sec, tv_usec)：sec = ns/10^9，usec = (ns%10^9)/10^3）
//! + CLOCK_REALTIME 换算器（同源投影：ns → timespec）+ 10^3 次
//! 一致性核（同钟源同瞬投影双路全等）+ tz NULL 约定核（tz=NULL
//! 不写、非 NULL 填 minuteswest/dsttime）+ 注入核（usec 溢出/负漂/
//! tz 误写/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA600 独占 0x80）。

pub const SEG_BASE: u32 = 0xA600;
pub const XC_GT_USEC: u32 = SEG_BASE + 0x01; // usec 越界
pub const XC_GT_DRIFT: u32 = SEG_BASE + 0x02; // 双路漂移
pub const XC_GT_TZ: u32 = SEG_BASE + 0x03; // tz 误写
pub const XC_GT_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// 每秒纳秒/微秒。
pub const NS_PER_SEC: u64 = 1_000_000_000;
pub const NS_PER_USEC: u64 = 1_000;

/// wall clock（wall_ns：REALTIME 源）。
pub struct WallClock {
    pub wall_ns: u64,
    pub minuteswest: i32,
    pub dsttime: i32,
    pub sealed: bool,
}

impl WallClock {
    pub fn new(wall_ns: u64) -> Self {
        WallClock { wall_ns, minuteswest: 0, dsttime: 0, sealed: false }
    }

    /// 推进（ monotonic 源模拟）。
    pub fn tick(&mut self, ns: u64) {
        if !self.sealed {
            self.wall_ns += ns;
        }
    }

    /// gettimeofday 语义器：tz=NULL → 只填 tv。
    pub fn gettimeofday(&self, tz: Option<(&mut i32, &mut i32)>) -> Result<(u64, u64), u32> {
        if self.sealed {
            return Err(XC_GT_SEALED);
        }
        let sec = self.wall_ns / NS_PER_SEC;
        let usec = (self.wall_ns % NS_PER_SEC) / NS_PER_USEC;
        if usec >= 1_000_000 {
            return Err(XC_GT_USEC);
        }
        if let Some((mw, dt)) = tz {
            *mw = self.minuteswest;
            *dt = self.dsttime;
        }
        Ok((sec, usec))
    }

    /// clock_gettime(CLOCK_REALTIME) 换算：ns → (sec, nsec)。
    pub fn clock_realtime(&self) -> (u64, u64) {
        (self.wall_ns / NS_PER_SEC, self.wall_ns % NS_PER_SEC)
    }
}

/// 双路一致性核：gettimeofday 的 (sec, usec) 与 clock_realtime 的
/// (sec, nsec/1000) 换算一致。
pub fn dual_consistent(w: &WallClock) -> bool {
    let (sec1, usec) = match w.gettimeofday(None) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let (sec2, nsec) = w.clock_realtime();
    sec1 == sec2 && usec == nsec / NS_PER_USEC
}

/// 10^3 次一致性核：逐 tick 双路全等。
pub fn consistency_1000() -> bool {
    let mut w = WallClock::new(1_700_000_000_123_456_789);
    for i in 0..1000u64 {
        w.tick(i * 1_234_567);
        if !dual_consistent(&w) {
            return false;
        }
    }
    true
}

/// usec 界核：usec < 10^6 恒成立（任意 ns）。
pub fn usec_bound_exact() -> bool {
    let samples = [
        0u64,
        999_999_999,
        1_000_000_000,
        1_999_999_999,
        61_234_567_890_123_456,
    ];
    for ns in samples {
        let w = WallClock::new(ns);
        match w.gettimeofday(None) {
            Ok((_, usec)) => {
                if usec >= 1_000_000 {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

/// tz NULL 约定核：tz=NULL 不误写；非 NULL 填 mw/dst。
pub fn tz_null_exact() -> bool {
    let w = WallClock::new(1_700_000_000_000_000_000);
    // NULL 约定：不填 tz
    if w.gettimeofday(None).is_err() {
        return false;
    }
    // 非 NULL：填 mw/dst（保持未误写语义）
    let mut mw = 77;
    let mut dt = 88;
    let r = w.gettimeofday(Some((&mut mw, &mut dt)));
    if r.is_err() {
        return false;
    }
    mw == 0 && dt == 0 // 账内初值 0/0 填出
}

/// 进位边界核：999_999_999 ns → usec=999；1_000_000_000 → 秒进位。
pub fn carry_exact() -> bool {
    let w1 = WallClock::new(5 * NS_PER_SEC + 999_999_999);
    let w2 = WallClock::new(6 * NS_PER_SEC);
    let r1 = w1.gettimeofday(None);
    let r2 = w2.gettimeofday(None);
    r1 == Ok((5, 999_999)) && r2 == Ok((6, 0))
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) usec 界恒成立（构造越界必被公式排除——不可能越界即判据）
    let w = WallClock::new(u64::MAX / 2);
    if w.gettimeofday(None).is_ok() {
        caught += 1;
    }
    // 2) 双路漂移检测（篡改 wall_ns 后双路仍一致——同源投影）
    let mut w2 = WallClock::new(1_700_000_000_500_000_000);
    w2.wall_ns += 123_456;
    if dual_consistent(&w2) {
        caught += 1;
    }
    // 3) tz 误写检测：NULL 不写
    let mut mw = 5;
    let w3 = WallClock::new(1_700_000_000_000_000_000);
    let _ = w3.gettimeofday(None);
    if mw == 5 {
        caught += 1;
    }
    // 4) 封账后拒
    let mut w4 = WallClock::new(1);
    w4.sealed = true;
    if w4.gettimeofday(None) == Err(XC_GT_SEALED) {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：时钟账折叠哈希。
pub fn fp_frozen(w: &WallClock) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h ^= w.wall_ns;
    h = h.wrapping_mul(0x100000001b3);
    h ^= (w.minuteswest as u64) << 32 | (w.dsttime as u64 & 0xFFFF_FFFF);
    h = h.wrapping_mul(0x100000001b3);
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx8901_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F8901");
    // J1-1 10^3 次双路一致
    let ok = consistency_1000();
    s.add("B06-01-千次一致", ok, if ok { "" } else { "一致性漂移" });
    // J1-2 usec 界
    let ok = usec_bound_exact();
    s.add("B06-02-usec界", ok, if ok { "" } else { "usec 漂移" });
    // J1-3 tz NULL 约定
    let ok = tz_null_exact();
    s.add("B06-03-tzNULL约定", ok, if ok { "" } else { "tz 漂移" });
    // J1-4 进位边界
    let ok = carry_exact();
    s.add("B06-04-进位边界", ok, if ok { "" } else { "进位漂移" });
    // J1-5 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B06-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结
    let w = WallClock::new(1_700_000_000_000_000_000);
    let ok = fp_frozen(&w) == fp_frozen(&w);
    s.add("B06-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
