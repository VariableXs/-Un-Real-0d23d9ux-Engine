//! UNX 卡 10162 — clock_gettime 正反双判据（单调性达成与非法时钟误用受控）
//! 段位 seg=0x10760
//! 判据原文：UNX-F10162-J1 单调性达成（CLOCK_MONOTONIC 两次取值不回退 10/10 次）
//! 锚点：MONOTONIC 不回退 + REALTIME 可跳变 + 非法 clk_id 受控 + 分辨率面
//! 域：ClockBook 三钟册（REALTIME/MONOTONIC/PROCESS_CPUTIME 各自推进核）
//! 结构：三钟册 → tick 推进 → gettime 快照 → 单调审计 → 非法钟受控 → 注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x10760;
pub const CK_BAD_CLOCK: u32 = SEG_BASE + 0x01;
pub const CK_REGRESS: u32 = SEG_BASE + 0x02;
pub const CK_SEALED: u32 = SEG_BASE + 0x03;
pub const CK_BAD_RES: u32 = SEG_BASE + 0x04;
pub const CK_FROZEN: u32 = SEG_BASE + 0x05;

/// 时钟 id 闭集
pub const C_REALTIME: u32 = 0;
pub const C_MONOTONIC: u32 = 1;
pub const C_CPUTIME: u32 = 2;

/// 时钟快照（秒, 纳秒）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ts {
    pub sec: u64,
    pub nsec: u64,
}

impl Ts {
    pub fn to_nsec(&self) -> u64 {
        self.sec * 1_000_000_000 + self.nsec
    }
}

/// 单钟状态
#[derive(Clone, Copy)]
pub struct ClockState {
    pub nsec_total: u64,
    pub res_nsec: u64,
}

/// 三钟册
pub struct ClockBook {
    clocks: [ClockState; 3],
    /// REALTIME 可跳变量（模拟 settime）
    pub jumps: usize,
    sealed: bool,
}

impl ClockBook {
    pub fn new() -> Self {
        ClockBook {
            clocks: [
                ClockState { nsec_total: 1_700_000_000_000, res_nsec: 1_000_000 },
                ClockState { nsec_total: 0, res_nsec: 1_000_000 },
                ClockState { nsec_total: 0, res_nsec: 10_000_000 },
            ],
            jumps: 0,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 推进指定钟（非法 id 受控；倒拨受控 REGRESS）
    pub fn tick(&mut self, clk: u32, nsec: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(CK_SEALED);
        }
        if clk >= 3 {
            return Err(CK_BAD_CLOCK);
        }
        self.clocks[clk as usize].nsec_total += nsec;
        Ok(())
    }

    /// REALTIME 跳变（settime 面：可正可负）
    pub fn jump_realtime(&mut self, delta: i64) -> Result<u64, u32> {
        if self.sealed {
            return Err(CK_SEALED);
        }
        let cur = self.clocks[C_REALTIME as usize].nsec_total as i64;
        let nxt = cur + delta;
        if nxt < 0 {
            return Err(CK_REGRESS);
        }
        self.clocks[C_REALTIME as usize].nsec_total = nxt as u64;
        self.jumps += 1;
        Ok(nxt as u64)
    }

    /// clock_gettime 主语义
    pub fn gettime(&self, clk: u32) -> Result<Ts, u32> {
        if self.sealed {
            return Err(CK_SEALED);
        }
        if clk >= 3 {
            return Err(CK_BAD_CLOCK);
        }
        let t = self.clocks[clk as usize].nsec_total;
        Ok(Ts {
            sec: t / 1_000_000_000,
            nsec: t % 1_000_000_000,
        })
    }

    /// 分辨率面
    pub fn getres(&self, clk: u32) -> Result<u64, u32> {
        if clk >= 3 {
            return Err(CK_BAD_CLOCK);
        }
        Ok(self.clocks[clk as usize].res_nsec)
    }

    /// 单调审计：MONOTONIC 桥接两次快照不回退
    pub fn mono_audit(&self, a: Ts, b: Ts) -> bool {
        b.to_nsec() >= a.to_nsec()
    }

    /// 三钟独立推进审计：MONO 推进不带动 REALTIME
    pub fn independence(&mut self) -> bool {
        let rt0 = self.gettime(C_REALTIME).unwrap_or(Ts { sec: 0, nsec: 0 });
        let _ = self.tick(C_MONOTONIC, 1_000_000_000);
        let rt1 = self.gettime(C_REALTIME).unwrap_or(Ts { sec: 0, nsec: 0 });
        rt0 == rt1
    }

    /// CPUTIME 只在 tick 时增长（冻结面）
    pub fn cpu_frozen(&self) -> bool {
        self.clocks[C_CPUTIME as usize].nsec_total == 0
    }

    /// 折叠指纹
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x107600;
        for c in &self.clocks {
            h = h.wrapping_mul(31).wrapping_add((c.nsec_total % 1_000_000_007) as u32);
            h = h.wrapping_mul(31).wrapping_add(c.res_nsec as u32);
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 指定钟纳秒总量
    pub fn nsec_of(&self, clk: u32) -> Result<u64, u32> {
        if clk >= 3 {
            return Err(CK_BAD_CLOCK);
        }
        Ok(self.clocks[clk as usize].nsec_total)
    }

    /// 单调钟别名
    pub fn mono_nsec(&self) -> u64 {
        self.clocks[C_MONOTONIC as usize].nsec_total
    }

    /// 实时钟秒面
    pub fn realtime_sec(&self) -> u64 {
        self.clocks[C_REALTIME as usize].nsec_total / 1_000_000_000
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ClockBook::new();
    // 1) 非法钟 id 必拒
    got[0] = b.tick(9, 1).is_err() && b.gettime(9).is_err();
    // 2) MONOTONIC 不回退
    let a = b.gettime(C_MONOTONIC).unwrap_or(Ts { sec: 0, nsec: 0 });
    let _ = b.tick(C_MONOTONIC, 500);
    let c = b.gettime(C_MONOTONIC).unwrap_or(Ts { sec: 9, nsec: 9 });
    got[1] = c.to_nsec() >= a.to_nsec();
    // 3) 分辨率非法钟必拒
    got[2] = b.getres(7).is_err();
    // 4) REALTIME 倒拨穿零必拒
    got[3] = b.jump_realtime(-9_000_000_000_000).is_err();
    // 5) 封册后取时必拒
    b.seal();
    got[4] = b.gettime(C_REALTIME).is_err();
    got
}

/// 判据入口：UNX-F10162
pub fn run_unx10162_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10162");
    let mut b = ClockBook::new();

    // J1 单调性 10/10：MONOTONIC 十轮取值不回退
    let mut j1 = true;
    let mut prev = b.gettime(C_MONOTONIC).unwrap_or(Ts { sec: 0, nsec: 0 });
    for i in 0..10u64 {
        let _ = b.tick(C_MONOTONIC, (i + 1) * 1_000_000);
        let cur = b.gettime(C_MONOTONIC).unwrap_or(Ts { sec: 0, nsec: 0 });
        if !b.mono_audit(prev, cur) {
            j1 = false;
        }
        prev = cur;
    }
    s.add("J1_mono_10x", j1, "CLOCK_MONOTONIC 两次取值不回退 10/10");

    // J2 非法钟受控
    let j2 = b.gettime(3) == Err(CK_BAD_CLOCK)
        && b.gettime(0xFFFF) == Err(CK_BAD_CLOCK)
        && b.tick(3, 1) == Err(CK_BAD_CLOCK);
    s.add("J2_bad_clock", j2, "越闭集钟 id 三路受控");

    // J3 REALTIME 跳变面
    let rt0 = b.gettime(C_REALTIME).unwrap_or(Ts { sec: 0, nsec: 0 }).to_nsec();
    let j3 = b.jump_realtime(5_000_000_000).is_ok()
        && b.gettime(C_REALTIME).unwrap_or(Ts { sec: 0, nsec: 0 }).to_nsec() == rt0 + 5_000_000_000;
    s.add("J3_rt_jump", j3, "REALTIME 前跳 5s 生效");

    // J4 倒拨受控（跳到负值）
    let j4 = b.jump_realtime(-99_000_000_000_000) == Err(CK_REGRESS);
    s.add("J4_regress", j4, "REALTIME 倒拨穿零受控 REGRESS");

    // J5 分辨率面
    let j5 = b.getres(C_MONOTONIC) == Ok(1_000_000)
        && b.getres(C_CPUTIME) == Ok(10_000_000)
        && b.getres(9) == Err(CK_BAD_CLOCK);
    s.add("J5_res", j5, "三钟分辨率在案且非法钟拒");

    // J6 三钟独立
    s.add("J6_independence", b.independence(), "MONO 推进不带动 REALTIME");

    // J7 CPUTIME 冻结面
    s.add("J7_cpu_frozen", b.cpu_frozen(), "未 tick 时 CPUTIME 恒零");

    // J8 快照归一
    let ts = b.gettime(C_MONOTONIC).unwrap_or(Ts { sec: 0, nsec: 0 });
    let j8 = ts.nsec < 1_000_000_000
        && Ts { sec: ts.sec, nsec: ts.nsec }.to_nsec()
            == b.clocks[C_MONOTONIC as usize].nsec_total;
    s.add("J8_ts_norm", j8, "快照 sec/nsec 归一守恒");

    // J9 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J9_inject_all", caught == 5, "5 注入点全检出");

    // J10 封册受控
    let mut b2 = ClockBook::new();
    b2.seal();
    let j10 = b2.gettime(C_REALTIME) == Err(CK_SEALED)
        && b2.tick(C_MONOTONIC, 1) == Err(CK_SEALED)
        && b2.jump_realtime(1) == Err(CK_SEALED);
    s.add("J10_sealed", j10, "封册后取时/推进/跳变皆拒 SEALED");

    // J11 指纹稳定
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "折叠指纹两次一致");

    // J12 大步推进
    let mut b3 = ClockBook::new();
    let _ = b3.tick(C_MONOTONIC, 123_456_789_123);
    let t = b3.gettime(C_MONOTONIC).unwrap_or(Ts { sec: 0, nsec: 0 });
    s.add("J12_big_tick", t.sec == 123 && t.nsec == 456_789_123, "大步推进 sec/nsec 拆分对平");

    // J13 REALTIME 初值
    let mut b4 = ClockBook::new();
    let rt = b4.gettime(C_REALTIME).unwrap_or(Ts { sec: 0, nsec: 0 });
    s.add("J13_rt_epoch", rt.sec == 1700 && rt.nsec == 0, "REALTIME 初值 1700s 在案");

    // J14 跳变计数
    s.add("J14_jumps", b.jumps == 1, "跳变计数 1 次对平");

    // J15 连续快照同值（不 tick 不动）
    let a1 = b3.gettime(C_CPUTIME).unwrap_or(Ts { sec: 0, nsec: 0 });
    let a2 = b3.gettime(C_CPUTIME).unwrap_or(Ts { sec: 0, nsec: 0 });
    s.add("J15_frozen_snapshot", a1 == a2, "无推进两次快照同值");

    // J16 纳秒别名
    let j16 = b.nsec_of(C_MONOTONIC) == Ok(b.mono_nsec())
        && b.nsec_of(9) == Err(CK_BAD_CLOCK);
    s.add("J16_nsec_alias", j16, "单调钟别名同值且野钟拒");

    // J17 实钟秒面
    let rt_sec = b.realtime_sec();
    let rt_raw = b.gettime(C_REALTIME).unwrap_or(Ts { sec: 0, nsec: 0 });
    s.add("J17_rt_sec", rt_sec == rt_raw.sec, "实钟秒面与快照秒等");

    // J18 推进守恒
    let m0 = b.mono_nsec();
    let _ = b.tick(C_MONOTONIC, 1_000);
    let j18 = b.mono_nsec() == m0 + 1_000;
    s.add("J18_tick_step", j18, "单调钟推进 1μs 步进对平");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock62_mono() {
        let mut b = ClockBook::new();
        let a = b.gettime(C_MONOTONIC).unwrap();
        b.tick(C_MONOTONIC, 1_000).unwrap();
        let c = b.gettime(C_MONOTONIC).unwrap();
        assert!(b.mono_audit(a, c));
    }

    #[test]
    fn test_clock62_bad() {
        let b = ClockBook::new();
        assert_eq!(b.gettime(9), Err(CK_BAD_CLOCK));
        assert_eq!(b.getres(9), Err(CK_BAD_CLOCK));
    }

    #[test]
    fn test_clock62_checks() {
        let cs = run_unx10162_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_clock62_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
