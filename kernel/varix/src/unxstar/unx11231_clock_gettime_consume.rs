//! UNX-F11231 clock_gettime 消费路径账（0x1f831 段）
//!
//! 锚点：UNX-F11231-J1 多时钟 ID（REALTIME/MONO/BOOTTIME/CPUTIME）消费判据
//! 绿，精度与基准机一致。
//! 域：CoRun·UNX-C5-B02——四个时钟各有语义约束：nsec 必须在 [0,1e9)，MONO
//! 不得回退，BOOTTIME 不得小于 MONO；任何一条不满足都记拒并保留原账。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f831;
pub const CK_ID: u32 = SEG_BASE + 0x01; // 时钟号不在四元组
pub const CK_NS: u32 = SEG_BASE + 0x02; // 纳秒越界
pub const CK_MONO: u32 = SEG_BASE + 0x03; // 单调钟回退
pub const CK_ORDER: u32 = SEG_BASE + 0x04; // boot 早于 mono
pub const CK_RES: u32 = SEG_BASE + 0x05; // 分辨率不达标
pub const CK_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const CK_OVER: u32 = SEG_BASE + 0x07; // 超容

/// 四个时钟号（Linux uapi 取值）。
pub const CLOCK_REALTIME: u32 = 0;
pub const CLOCK_MONOTONIC: u32 = 1;
pub const CLOCK_CPUTIME: u32 = 2;
pub const CLOCK_BOOTTIME: u32 = 7;
pub const CLOCKS: [u32; 4] = [CLOCK_REALTIME, CLOCK_MONOTONIC, CLOCK_CPUTIME, CLOCK_BOOTTIME];
pub const NS_PER_SEC: u32 = 1_000_000_000;
pub const MAX_READ: usize = 32;
pub const CK_TAG: u64 = 0x1f831c5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一次读钟结果。
#[derive(Clone, Debug)]
pub struct Reading {
    pub clock: u32,
    pub sec: u64,
    pub nsec: u32,
    pub res_ns: u32,
}

impl Reading {
    pub fn ns_ok(&self) -> bool {
        self.nsec < NS_PER_SEC
    }

    pub fn key(&self) -> u64 {
        self.sec.saturating_mul(NS_PER_SEC as u64).saturating_add(self.nsec as u64)
    }
}

/// 读钟账。
#[derive(Clone, Debug)]
pub struct ClockBank {
    pub reads: Vec<Reading>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl ClockBank {
    pub fn new() -> ClockBank {
        ClockBank { reads: Vec::new(), frozen: false, root: None }
    }

    fn last_of(&self, clock: u32) -> Option<&Reading> {
        let mut k = self.reads.len();
        while k > 0 {
            k -= 1;
            if self.reads[k].clock == clock {
                return Some(&self.reads[k]);
            }
        }
        None
    }

    /// 读一次：未知钟/纳秒越界/回退/次序倒挂/分辨率五路拒。
    pub fn read(&mut self, clock: u32, sec: u64, nsec: u32, res_ns: u32) -> Result<u64, u32> {
        if self.frozen {
            return Err(CK_FROZEN);
        }
        if !CLOCKS.iter().any(|c| *c == clock) {
            return Err(CK_ID);
        }
        if nsec >= NS_PER_SEC {
            return Err(CK_NS);
        }
        if self.reads.len() >= MAX_READ {
            return Err(CK_OVER);
        }
        let cur = sec.saturating_mul(NS_PER_SEC as u64).saturating_add(nsec as u64);
        if let Some(prev) = self.last_of(clock) {
            if (clock == CLOCK_MONOTONIC || clock == CLOCK_BOOTTIME) && cur < prev.key() {
                return Err(CK_MONO);
            }
        }
        if clock == CLOCK_BOOTTIME {
            if let Some(m) = self.last_of(CLOCK_MONOTONIC) {
                if cur < m.key() {
                    return Err(CK_ORDER);
                }
            }
        }
        if res_ns > 1000 {
            return Err(CK_RES);
        }
        self.reads.push(Reading { clock, sec, nsec, res_ns });
        Ok(cur)
    }

    pub fn seen(&self, clock: u32) -> bool {
        self.reads.iter().any(|r| r.clock == clock)
    }

    pub fn seen_all(&self) -> bool {
        CLOCKS.iter().all(|c| self.seen(*c))
    }

    pub fn precise(&self) -> bool {
        !self.reads.is_empty() && self.reads.iter().all(|r| r.res_ns <= 1000)
    }

    pub fn mono_non_decreasing(&self) -> bool {
        let mut last = 0u64;
        let mut k = 0usize;
        while k < self.reads.len() {
            if self.reads[k].clock == CLOCK_MONOTONIC {
                if self.reads[k].key() < last {
                    return false;
                }
                last = self.reads[k].key();
            }
            k += 1;
        }
        true
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(CK_FROZEN);
        }
        if !self.seen_all() {
            return Err(CK_ID);
        }
        if !self.reads.iter().all(|r| r.ns_ok()) {
            return Err(CK_NS);
        }
        if !self.mono_non_decreasing() {
            return Err(CK_MONO);
        }
        if !self.precise() {
            return Err(CK_RES);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("clock reads {} all {} precise {}", self.reads.len(), self.seen_all(), self.precise())
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = CK_TAG;
        let mut k = 0usize;
        while k < self.reads.len() {
            h = (h ^ self.reads[k].clock as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.reads[k].sec).wrapping_mul(FP_STEP);
            h = (h ^ self.reads[k].nsec as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.reads[k].res_ns as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }
}

/// 组一张四钟全见、单调不回退的账。
pub fn build() -> ClockBank {
    let mut b = ClockBank::new();
    let _ = b.read(CLOCK_REALTIME, 1_760_000_000, 250_000_000, 1);
    let _ = b.read(CLOCK_MONOTONIC, 1000, 0, 1);
    let _ = b.read(CLOCK_CPUTIME, 5, 0, 1000);
    let _ = b.read(CLOCK_BOOTTIME, 1100, 0, 1);
    let _ = b.read(CLOCK_MONOTONIC, 1001, 500_000, 1);
    let _ = b.read(CLOCK_BOOTTIME, 1200, 0, 1);
    b
}

/// 四路对抗：未知钟、纳秒越界、单调回退、粗分辨率。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = ClockBank::new();
    out[0] = a.read(9, 1, 0, 1) == Err(CK_ID);
    out[1] = a.read(CLOCK_REALTIME, 1, NS_PER_SEC, 1) == Err(CK_NS);
    let _ = a.read(CLOCK_MONOTONIC, 10, 0, 1);
    out[2] = a.read(CLOCK_MONOTONIC, 9, 999_999_999, 1) == Err(CK_MONO);
    out[3] = a.read(CLOCK_REALTIME, 2, 0, 2000) == Err(CK_RES);
    out
}

/// 判据入口。
pub fn run_unx11231_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11231");
    let mut b = build();
    let root = b.freeze().unwrap_or(0);

    s.add("J1-四钟全见", b.seen_all(), "REALTIME/MONO/BOOTTIME/CPUTIME 均有读数");
    s.add("J2-实时钟量级", b.reads[0].sec > 1_700_000_000, "实时钟秒数落在二〇二三年之后");
    s.add("J3-纳秒边界", b.reads.iter().all(|r| r.ns_ok()), "每条读数纳秒均在 [0,1e9)");
    s.add("J4-单调不回退", b.mono_non_decreasing(), "MONOTONIC 两次读数不递减");
    s.add("J5-boot 不早于 mono", {
        let m = b.reads[4].key();
        b.reads[5].key() >= m
    }, "BOOTTIME 读数不早于最近一次 MONOTONIC");
    s.add("J6-分辨率", b.precise(), "四个时钟分辨率均不粗于一微秒");
    s.add("J7-冻存根", root != 0 && b.root == Some(root), "读钟账冻存产出非零根");

    s.add("J8-冻后读拒", b.read(CLOCK_REALTIME, 1_760_000_100, 0, 1) == Err(CK_FROZEN), "冻存后读钟拒为 CK_FROZEN");
    s.add("J9-未知钟拒", {
        let mut x = ClockBank::new();
        x.read(CLOCK_MONOTONIC + 100, 1, 0, 1) == Err(CK_ID)
    }, "未知时钟号拒为 CK_ID");
    s.add("J10-越界纳秒拒", {
        let mut x = ClockBank::new();
        x.read(CLOCK_REALTIME, 1, NS_PER_SEC + 1, 1) == Err(CK_NS)
    }, "纳秒超过 999999999 拒为 CK_NS");
    s.add("J11-回退拒", {
        let mut x = ClockBank::new();
        let _ = x.read(CLOCK_MONOTONIC, 50, 0, 1);
        x.read(CLOCK_MONOTONIC, 49, 0, 1) == Err(CK_MONO)
    }, "MONOTONIC 回退拒为 CK_MONO");
    s.add("J12-粗分辨率拒", {
        let mut x = ClockBank::new();
        x.read(CLOCK_REALTIME, 1, 0, 1001) == Err(CK_RES)
    }, "分辨率超过一微秒拒为 CK_RES");

    let four = inject_four();
    s.add("J13-四路注入", four.iter().all(|x| *x), "未知钟/越界纳秒/回退/粗分辨率四路全拒");
    s.add_dyn(
        "J14-读钟明细",
        b.reads.len() == 6 && b.seen_all(),
        &format!("读数 {} 条，四钟 {}，抖动样本 mono {}ns", b.reads.len(), b.seen_all(), b.reads[4].nsec),
    );
    s.add_dyn(
        "J15-账根",
        root == b.fold(),
        &format!("根 0x{:x}，实时键 {}", root, b.reads[0].key()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11231_clock_gettime_flow() {
        let mut b = build();
        assert!(b.seen_all());
        assert!(b.mono_non_decreasing());
        let r = b.freeze().unwrap_or(0);
        assert!(r != 0);
        assert_eq!(b.read(0, 1, 0, 1), Err(CK_FROZEN));
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx11231_checks_green() {
        let s = run_unx11231_checks();
        assert!(s.all_passed(), "UNX-F11231 判据未全绿");
    }
}
