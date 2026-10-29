//! UNX-F11275 musl 时钟与睡眠消费（0x1f4b0 段）
//! 纯功能模型：clock_gettime 读时钟账（实时/单调/启动），clock_nanosleep
//! 按「相对/绝对」语义登记睡眠请求；纳秒域必须 <1e9、绝对目标不得早于
//! 当前时刻、单调钟不得回退；被信号打断的睡眠记剩余量并返回中断。
//! 同参重复请求单独立错，封册后不再接单。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段（0x1f4b0 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f4b0;
/// 时钟号未登记。
pub const CK_ID: u32 = SEG_BASE + 0x01;
/// 纳秒域越界（>= 1e9）。
pub const CK_NS: u32 = SEG_BASE + 0x02;
/// 绝对睡眠目标早于当前时刻。
pub const CK_NEG: u32 = SEG_BASE + 0x03;
/// 单调钟读回退。
pub const CK_ORDER: u32 = SEG_BASE + 0x04;
/// 睡眠被信号打断。
pub const CK_INTR: u32 = SEG_BASE + 0x05;
/// 同参睡眠请求重复。
pub const CK_DUP: u32 = SEG_BASE + 0x06;
/// 封册后写账。
pub const CK_FROZEN: u32 = SEG_BASE + 0x07;
/// 睡眠账已满。
pub const CK_CAP: u32 = SEG_BASE + 0x08;

/// 一秒的纳秒数（归一基准）。
pub const NS_PER_SEC: u64 = 1_000_000_000;
/// 时钟号：实时。
pub const CLOCK_REALTIME: u32 = 0;
/// 时钟号：单调。
pub const CLOCK_MONOTONIC: u32 = 1;
/// 时钟号：启动（含休眠）。
pub const CLOCK_BOOTTIME: u32 = 7;
/// 睡眠账定容。
pub const SLEEP_CAP: usize = 48;
/// 时钟表定容。
pub const CLOCK_CAP: usize = 8;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_75;

/// 时间戳：秒 + 纳秒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timespec {
    pub sec: u64,
    pub nsec: u32,
}

impl Timespec {
    /// 逐字渲染 `sec.nnnnnnnnn`。
    pub fn render(&self) -> String {
        format!("{}.{:09}", self.sec, self.nsec)
    }

    /// 折算纳秒总量。
    pub fn to_ns(&self) -> u64 {
        self.sec.wrapping_mul(NS_PER_SEC).wrapping_add(self.nsec as u64)
    }
}

/// 一条睡眠请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SleepReq {
    pub clock: u32,
    pub dur: Timespec,
    pub abs: bool,
    pub interrupted: bool,
}

impl SleepReq {
    /// 渲染 `c<clock> <sec.nsec><r|a>`。
    pub fn render(&self) -> String {
        format!("c{} {}{}", self.clock, self.dur.render(), if self.abs { "a" } else { "r" })
    }
}

/// 一只时钟：号 + 当前读数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    pub id: u32,
    pub base: Timespec,
}

/// 时钟账：注册 → 读 → 走表 → 睡眠登记。
#[derive(Clone, Debug)]
pub struct ClockBook {
    pub clocks: Vec<Clock>,
    pub sleeps: Vec<SleepReq>,
    pub accepts: u32,
    pub rejects: u32,
    pub interrupts: u32,
    pub pending: bool,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl ClockBook {
    pub fn new() -> ClockBook {
        ClockBook {
            clocks: Vec::new(),
            sleeps: Vec::new(),
            accepts: 0,
            rejects: 0,
            interrupts: 0,
            pending: false,
            frozen: false,
            root: None,
        }
    }

    /// 注册时钟。判定序：封册 → 纳秒域 → 号重复 → 定容。
    pub fn register(&mut self, id: u32, sec: u64, nsec: u32) -> Result<usize, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CK_FROZEN);
        }
        if nsec >= NS_PER_SEC as u32 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CK_NS);
        }
        if self.clocks.iter().any(|c| c.id == id) {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CK_ID);
        }
        if self.clocks.len() >= CLOCK_CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CK_CAP);
        }
        self.clocks.push(Clock { id, base: Timespec { sec, nsec } });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.clocks.len().wrapping_sub(1))
    }

    /// clock_gettime：未登记时钟返回 CK_ID。
    pub fn now(&self, id: u32) -> Result<Timespec, u32> {
        let mut k = 0usize;
        while k < self.clocks.len() {
            if let Some(c) = self.clocks.get(k) {
                if c.id == id {
                    return Ok(c.base);
                }
            }
            k = k.wrapping_add(1);
        }
        Err(CK_ID)
    }

    /// 找时钟下标（不计数）。
    fn index_of(&self, id: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.clocks.len() {
            if let Some(c) = self.clocks.get(k) {
                if c.id == id {
                    return Some(k);
                }
            }
            k = k.wrapping_add(1);
        }
        None
    }

    /// 走表（模拟时间流逝）：单调钟只许前进。
    pub fn tick(&mut self, id: u32, dsec: u64, dnsec: u32) -> Result<Timespec, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CK_FROZEN);
        }
        if dnsec >= NS_PER_SEC as u32 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CK_NS);
        }
        let i = match self.index_of(id) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(CK_ID);
            }
        };
        let old = match self.clocks.get(i) {
            Some(c) => c.base,
            None => return Err(CK_ID),
        };
        let add = dsec.wrapping_mul(NS_PER_SEC).wrapping_add(dnsec as u64);
        let tot = old.to_ns().wrapping_add(add);
        if tot < old.to_ns() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CK_ORDER);
        }
        let ts = Timespec { sec: tot / NS_PER_SEC, nsec: (tot % NS_PER_SEC) as u32 };
        if let Some(c) = self.clocks.get_mut(i) {
            c.base = ts;
        }
        self.accepts = self.accepts.wrapping_add(1);
        Ok(ts)
    }

    /// 同参请求是否已登记（被打断的请求不算占用）。
    fn seen(&self, req: &SleepReq) -> bool {
        self.sleeps.iter().any(|s| {
            s.clock == req.clock && s.dur == req.dur && s.abs == req.abs && !s.interrupted
        })
    }

    /// 预置一次信号中断（下一笔睡眠被截断）。
    pub fn arm_interrupt(&mut self) {
        self.pending = true;
    }

    /// clock_nanosleep：成功返回剩余量（零）。
    /// 判定序：封册 → 时钟 → 纳秒域 → 绝对目标 → 重复 → 中断 → 定容。
    pub fn sleep(&mut self, clock: u32, sec: u64, nsec: u32, abs: bool)
        -> Result<Timespec, u32>
    {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CK_FROZEN);
        }
        let base = match self.now(clock) {
            Ok(v) => v,
            Err(e) => {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(e);
            }
        };
        if nsec >= NS_PER_SEC as u32 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CK_NS);
        }
        let dur = Timespec { sec, nsec };
        if abs && dur.to_ns() < base.to_ns() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CK_NEG);
        }
        let req = SleepReq { clock, dur, abs, interrupted: false };
        if self.seen(&req) {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CK_DUP);
        }
        if self.pending {
            self.pending = false;
            self.sleeps.push(SleepReq { interrupted: true, ..req });
            self.interrupts = self.interrupts.wrapping_add(1);
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CK_INTR);
        }
        if self.sleeps.len() >= SLEEP_CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CK_CAP);
        }
        self.sleeps.push(req);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(Timespec { sec: 0, nsec: 0 })
    }

    /// 睡眠账面总时长（纳秒，含被打断请求的完整请求量）。
    pub fn total_sleep_ns(&self) -> u64 {
        let mut n = 0u64;
        let mut k = 0usize;
        while k < self.sleeps.len() {
            if let Some(s) = self.sleeps.get(k) {
                n = n.wrapping_add(s.dur.to_ns());
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// clock_settime：实时钟可回拨，单调/启动钟回拨判 CK_ORDER。
    pub fn set_time(&mut self, id: u32, sec: u64, nsec: u32) -> Result<Timespec, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CK_FROZEN);
        }
        if nsec >= NS_PER_SEC as u32 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CK_NS);
        }
        let i = match self.index_of(id) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(CK_ID);
            }
        };
        let old = match self.clocks.get(i) {
            Some(c) => c.base.to_ns(),
            None => return Err(CK_ID),
        };
        let want = Timespec { sec, nsec };
        if id != CLOCK_REALTIME && want.to_ns() < old {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CK_ORDER);
        }
        if let Some(c) = self.clocks.get_mut(i) {
            c.base = want;
        }
        self.accepts = self.accepts.wrapping_add(1);
        Ok(want)
    }

    /// FNV-1a：时钟读数与睡眠序敏感。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < self.clocks.len() {
            if let Some(c) = self.clocks.get(k) {
                h = (h ^ c.id as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.base.sec).wrapping_mul(FNV_PRIME);
                h = (h ^ c.base.nsec as u64).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        let mut j = 0usize;
        while j < self.sleeps.len() {
            if let Some(s) = self.sleeps.get(j) {
                h = (h ^ s.clock as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.dur.sec).wrapping_mul(FNV_PRIME);
                h = (h ^ s.dur.nsec as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.abs as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.interrupted as u64).wrapping_mul(FNV_PRIME);
            }
            j = j.wrapping_add(1);
        }
        h
    }

    /// 封册：根幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.root;
        }
        let r = match self.root {
            Some(v) => v,
            None => self.fold(),
        };
        self.root = Some(r);
        self.frozen = true;
        Some(r)
    }
}

/// 三钟样本：实时 / 单调 / 启动。
pub fn boot_clocks() -> ClockBook {
    let mut b = ClockBook::new();
    let _ = b.register(CLOCK_REALTIME, 1_700_000_000, 250_000_000);
    let _ = b.register(CLOCK_MONOTONIC, 12_345, 678_000_000);
    let _ = b.register(CLOCK_BOOTTIME, 12_350, 0);
    b
}

/// 四探针：(a) 未登记时钟，(b) 同参重复，(c) 纳秒越界，(d) 封册后接单。
pub fn inject_four() -> [bool; 4] {
    let mut a = boot_clocks();
    let bad_a = a.sleep(99, 1, 0, false).err() == Some(CK_ID);

    let mut b = boot_clocks();
    let _ = b.sleep(CLOCK_MONOTONIC, 1, 0, false);
    let bad_b = b.sleep(CLOCK_MONOTONIC, 1, 0, false).err() == Some(CK_DUP);

    let mut c = boot_clocks();
    let bad_c = c.sleep(CLOCK_MONOTONIC, 0, 1_000_000_000, false).err() == Some(CK_NS);

    let mut d = boot_clocks();
    let _ = d.freeze();
    let bad_d = d.sleep(CLOCK_MONOTONIC, 2, 0, false).err() == Some(CK_FROZEN);
    [bad_a, bad_b, bad_c, bad_d]
}

/// Thirteen contract checks over clock reads, sleep semantics and gates.
pub fn run_unx11275_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11275");

    s.add("X1-SEGMENT-DOMAIN", SEG_BASE == 0x1f4b0 && CK_ID == SEG_BASE + 0x01
        && CK_NS == SEG_BASE + 0x02 && CK_NEG == SEG_BASE + 0x03
        && CK_ORDER == SEG_BASE + 0x04 && CK_INTR == SEG_BASE + 0x05
        && CK_DUP == SEG_BASE + 0x06 && CK_FROZEN == SEG_BASE + 0x07
        && CK_CAP == SEG_BASE + 0x08, "错误码落在 0x1f4b0 段连续偏移上");

    let c = boot_clocks();
    s.add("X2-CLOCK-READ", c.clocks.len() == 3 && c.accepts == 3
        && c.now(CLOCK_MONOTONIC) == Ok(Timespec { sec: 12_345, nsec: 678_000_000 })
        && c.now(9).is_err(), "三钟注册后 clock_gettime 读数逐字段正确");

    let mut r = boot_clocks();
    let zero = r.sleep(CLOCK_MONOTONIC, 1, 500_000_000, false);
    s.add("X3-SLEEP-REL", zero == Ok(Timespec { sec: 0, nsec: 0 })
        && r.sleeps.len() == 1 && r.accepts == 4
        && r.total_sleep_ns() == 1_500_000_000, "相对睡眠 1.5s 登记且剩余量为零");

    let mut ns = boot_clocks();
    let e1 = ns.sleep(CLOCK_MONOTONIC, 0, 1_000_000_000, false).err();
    s.add("X4-NS-GATE", e1 == Some(CK_NS) && ns.sleeps.is_empty() && ns.rejects == 1,
        "纳秒域 >=1e9 被 CK_NS 拒绝");

    let mut ab = boot_clocks();
    let e2 = ab.sleep(CLOCK_REALTIME, 1, 0, true).err();
    s.add("X5-ABS-PAST", e2 == Some(CK_NEG) && ab.sleeps.is_empty(),
        "绝对目标早于当前时刻被 CK_NEG 拒绝");

    let mut it = boot_clocks();
    it.arm_interrupt();
    let e3 = it.sleep(CLOCK_MONOTONIC, 2, 0, false);
    let rec = it.sleeps.first().map(|x| x.interrupted);
    s.add("X6-INTERRUPT", e3 == Err(CK_INTR) && rec == Some(true) && it.interrupts == 1
        && it.total_sleep_ns() == 2_000_000_000, "被信号打断的睡眠保留请求量并记中断");

    let mut o = boot_clocks();
    let reg = o.set_time(CLOCK_MONOTONIC, 1, 0).err();
    let rt = o.set_time(CLOCK_REALTIME, 1, 0);
    s.add("X7-ORDER-GATE", reg == Some(CK_ORDER) && rt == Ok(Timespec { sec: 1, nsec: 0 })
        && o.rejects == 1, "单调钟回拨判 CK_ORDER，实时钟可回拨");

    let mut d = boot_clocks();
    let _ = d.sleep(CLOCK_BOOTTIME, 3, 0, false);
    let ed = d.sleep(CLOCK_BOOTTIME, 3, 0, false).err();
    s.add("X8-DUP-GATE", ed == Some(CK_DUP) && d.sleeps.len() == 1 && d.rejects == 1,
        "同钟同参睡眠被 CK_DUP 拒绝");

    let mut id = boot_clocks();
    let e1 = id.sleep(42, 1, 0, false).err();
    let e2 = id.register(CLOCK_MONOTONIC, 0, 0).err();
    s.add("X9-ID-GATE", e1 == Some(CK_ID) && e2 == Some(CK_ID) && id.rejects == 2,
        "未登记时钟与重号注册均判 CK_ID");

    let mut cap = boot_clocks();
    let mut i = 1u64;
    while cap.sleeps.len() < SLEEP_CAP {
        let _ = cap.sleep(CLOCK_MONOTONIC, i, 0, false);
        i = i.wrapping_add(1);
    }
    let ec = cap.sleep(CLOCK_MONOTONIC, 999, 0, false).err();
    s.add("X10-CAP", cap.sleeps.len() == SLEEP_CAP && ec == Some(CK_CAP)
        && cap.accepts == 3 + SLEEP_CAP as u32 && cap.rejects == 1,
        "睡眠账满 48 条后继续接单被 CK_CAP 拒绝");

    let a = boot_clocks();
    let mut b = boot_clocks();
    let h0 = a.fold();
    let h1 = b.fold();
    let _ = b.tick(CLOCK_MONOTONIC, 1, 0);
    s.add("X11-FOLD", h0 != 0 && h0 == h1 && b.fold() != h0,
        "同构时钟账指纹相同，走表后指纹变化");

    let mut f = boot_clocks();
    let r1 = f.freeze();
    let r2 = f.freeze();
    s.add("X12-FREEZE-IDEM", r1.is_some() && r1 == r2 && f.frozen,
        "封册返回根且二次调用同值");

    let mut z = boot_clocks();
    let _ = z.freeze();
    let ez = z.sleep(CLOCK_MONOTONIC, 4, 0, false).err();
    let e4 = z.register(9, 0, 0).err();
    s.add_dyn("X13-FROZEN-GATE", ez == Some(CK_FROZEN) && e4 == Some(CK_FROZEN)
        && z.rejects == 2 && z.sleeps.is_empty(),
        &format!("封后睡眠={:?} 注册={:?}", ez, e4));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11275_flow() {
        let mut b = boot_clocks();
        assert_eq!(b.clocks.len(), 3);
        assert_eq!(b.accepts, 3);
        assert_eq!(b.now(CLOCK_REALTIME),
            Ok(Timespec { sec: 1_700_000_000, nsec: 250_000_000 }));
        assert_eq!(b.now(CLOCK_BOOTTIME), Ok(Timespec { sec: 12_350, nsec: 0 }));
        assert_eq!(b.tick(CLOCK_MONOTONIC, 5, 0),
            Ok(Timespec { sec: 12_350, nsec: 678_000_000 }));
        assert_eq!(b.sleep(CLOCK_MONOTONIC, 0, 250_000_000, false),
            Ok(Timespec { sec: 0, nsec: 0 }));
        assert_eq!(b.total_sleep_ns(), 250_000_000);
        assert_eq!(b.accepts, 5);
        let root = b.freeze();
        assert!(root.is_some());
        assert_eq!(root, b.freeze());
    }

    #[test]
    fn unx11275_gates() {
        let mut b = boot_clocks();
        assert!(b.sleep(CLOCK_BOOTTIME, 1, 0, false).is_ok());
        assert_eq!(b.sleep(CLOCK_BOOTTIME, 1, 0, false).err(), Some(CK_DUP));
        assert_eq!(b.sleep(9, 1, 0, false).err(), Some(CK_ID));
        assert_eq!(b.sleep(CLOCK_MONOTONIC, 0, 1_000_000_000, false).err(), Some(CK_NS));
        assert_eq!(b.sleep(CLOCK_REALTIME, 1, 0, true).err(), Some(CK_NEG));
        assert_eq!(b.set_time(CLOCK_MONOTONIC, 0, 0).err(), Some(CK_ORDER));
        assert_eq!(b.register(CLOCK_MONOTONIC, 1, 0).err(), Some(CK_ID));
        assert_eq!(b.accepts, 4);
        assert_eq!(b.rejects, 6);
        b.arm_interrupt();
        assert_eq!(b.sleep(CLOCK_MONOTONIC, 2, 0, false).err(), Some(CK_INTR));
        assert_eq!(b.interrupts, 1);
        assert_eq!(b.rejects, 7);
        let _ = b.freeze();
        assert_eq!(b.sleep(CLOCK_MONOTONIC, 3, 0, false).err(), Some(CK_FROZEN));
        assert_eq!(b.rejects, 8);

        let mut c = boot_clocks();
        let mut i = 1u64;
        while c.sleeps.len() < SLEEP_CAP {
            assert!(c.sleep(CLOCK_BOOTTIME, i, 0, false).is_ok());
            i = i.wrapping_add(1);
        }
        assert_eq!(c.sleep(CLOCK_BOOTTIME, 999, 0, false).err(), Some(CK_CAP));
        assert_eq!(c.accepts, 3 + SLEEP_CAP as u32);
        assert_eq!(c.rejects, 1);
    }

    #[test]
    fn unx11275_checks_green() {
        let s = run_unx11275_checks();
        assert!(s.all_passed());
        assert_eq!(s.red_items_text(), "");
    }
}
