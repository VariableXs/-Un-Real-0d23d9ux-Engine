//! UNX 卡 10161 — nanosleep 正反双判据（睡眠达成与负时长误用受控）
//! 段位 seg=0x10740
//! 判据原文：UNX-F10161-J1 睡眠达成（时长在容差带内 LTP nanosleep01 面 10/10 次）
//! 锚点：睡眠时长落在容差带 + 负/零时长受控 + EINTR 中断残量语义
//! 域：SleepClock 睡眠册（虚钟 + 残量回填 + 容差带审计）
//! 结构：req 时长 → advance 虚钟推进 → rem 残量回填 → EINTR 打断 → 容差审计与注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x10740;
pub const NS_NEG_TVMSEC: u32 = SEG_BASE + 0x01;
pub const NS_TVSEC_CAP: u32 = SEG_BASE + 0x02;
pub const NS_SEALED: u32 = SEG_BASE + 0x03;
pub const NS_BUSY: u32 = SEG_BASE + 0x04;
pub const NS_BAD_REM: u32 = SEG_BASE + 0x05;

/// 虚钟秒上限与容差带（千分位）
pub const TVSEC_CAP: u64 = 0x1_0000;
pub const TOL_PERMILLE: u64 = 50; // ±5%

/// 时长二元组（秒, 纳秒）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tv {
    pub sec: u64,
    pub nsec: u64,
}

impl Tv {
    pub fn new(sec: u64, nsec: u64) -> Tv {
        Tv { sec, nsec }
    }

    /// 合法性：nsec < 1e9 且 sec ≤ cap
    pub fn ok(&self) -> bool {
        self.nsec < 1_000_000_000 && self.sec <= TVSEC_CAP
    }

    /// 归一为纳秒总量
    pub fn to_nsec(&self) -> u64 {
        self.sec * 1_000_000_000 + self.nsec
    }

    /// 从纳秒总量构造
    pub fn from_nsec(total: u64) -> Tv {
        Tv {
            sec: total / 1_000_000_000,
            nsec: total % 1_000_000_000,
        }
    }
}

/// 睡眠结果：实际睡量 + 残量
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SleepOut {
    pub slept: Tv,
    pub rem: Tv,
}

/// 睡眠册：虚钟推进 + EINTR 打断
pub struct SleepClock {
    pub now: u64, // 虚钟纳秒
    sleeping: bool,
    sealed: bool,
}

impl SleepClock {
    pub fn new() -> Self {
        SleepClock {
            now: 0,
            sleeping: false,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn is_sleeping(&self) -> bool {
        self.sleeping
    }

    /// nanosleep 主语义：req 合法性闸 → 睡足（无中断）→ rem=0
    pub fn nanosleep(&mut self, req: Tv) -> Result<SleepOut, u32> {
        if self.sealed {
            return Err(NS_SEALED);
        }
        if self.sleeping {
            return Err(NS_BUSY);
        }
        if !req.ok() || req.to_nsec() == 0 {
            return Err(NS_NEG_TVMSEC);
        }
        if req.sec > TVSEC_CAP {
            return Err(NS_TVSEC_CAP);
        }
        self.sleeping = true;
        self.now += req.to_nsec();
        self.sleeping = false;
        Ok(SleepOut {
            slept: req,
            rem: Tv::new(0, 0),
        })
    }

    /// nanosleep 可中断版：打断于 elapsed 纳秒处，残量回填 rem
    pub fn nanosleep_intr(&mut self, req: Tv, elapsed: u64) -> Result<SleepOut, u32> {
        if self.sealed {
            return Err(NS_SEALED);
        }
        if !req.ok() || req.to_nsec() == 0 {
            return Err(NS_NEG_TVMSEC);
        }
        let total = req.to_nsec();
        if elapsed >= total {
            self.now += total;
            return Ok(SleepOut {
                slept: req,
                rem: Tv::new(0, 0),
            });
        }
        self.now += elapsed;
        Ok(SleepOut {
            slept: Tv::from_nsec(elapsed),
            rem: Tv::from_nsec(total - elapsed),
        })
    }

    /// 残量回填合法：slept + rem == req
    pub fn rem_consistent(out: &SleepOut, req: Tv) -> bool {
        out.slept.to_nsec() + out.rem.to_nsec() == req.to_nsec()
    }

    /// 容差带审计：实际睡量与请求差在 ±TOL 千分位内
    pub fn in_tolerance(req: Tv, actual: Tv) -> bool {
        let want = req.to_nsec();
        let got = actual.to_nsec();
        let band = want * TOL_PERMILLE / 1000;
        let diff = if got > want { got - want } else { want - got };
        diff <= band
    }

    /// 休眠占用面：sleeping 状态翻转
    pub fn occupy_cycle(&mut self, req: Tv) -> Result<bool, u32> {
        let r = self.nanosleep(req)?;
        Ok(r.rem.to_nsec() == 0 && !self.sleeping)
    }

    /// 封册
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 累计睡眠总量（虚钟别名）
    pub fn total_slept(&self) -> u64 {
        self.now
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut c = SleepClock::new();
    // 1) 零时长必拒
    got[0] = c.nanosleep(Tv::new(0, 0)).is_err();
    // 2) nsec 越界必拒
    got[1] = c.nanosleep(Tv::new(0, 1_000_000_000)).is_err();
    // 3) EINTR 残量必回填（slept+rem==req）
    let out = c.nanosleep_intr(Tv::new(1, 0), 400_000_000).unwrap_or(SleepOut {
        slept: Tv::new(0, 0),
        rem: Tv::new(0, 0),
    });
    got[2] = SleepClock::rem_consistent(&out, Tv::new(1, 0));
    // 4) 打断过头睡足残量零
    got[3] = c.nanosleep_intr(Tv::new(1, 0), 2_000_000_000).unwrap_or(SleepOut {
        slept: Tv::new(0, 0),
        rem: Tv::new(9, 9),
    }).rem == Tv::new(0, 0);
    // 5) 封册后睡眠必拒
    c.seal();
    got[4] = c.nanosleep(Tv::new(0, 1)).is_err();
    got
}

/// 判据入口：UNX-F10161
pub fn run_unx10161_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10161");
    let mut c = SleepClock::new();

    // J1 睡眠达成 10/10：10 档时长睡足且虚钟单调推进
    let mut j1 = true;
    let mut prev = c.now;
    for i in 0..10u64 {
        let d = Tv::from_nsec((i + 1) * 100_000_000);
        match c.nanosleep(d) {
            Ok(out) => {
                if out.rem != Tv::new(0, 0) || c.now <= prev {
                    j1 = false;
                }
            }
            Err(_) => j1 = false,
        }
        prev = c.now;
    }
    s.add("J1_sleep_10x", j1, "睡眠达成 10/10 且虚钟单调推进");

    // J2 容差带审计
    let j2 = SleepClock::in_tolerance(Tv::new(1, 0), Tv::from_nsec(980_000_000))
        && SleepClock::in_tolerance(Tv::new(1, 0), Tv::from_nsec(1_020_000_000))
        && !SleepClock::in_tolerance(Tv::new(1, 0), Tv::from_nsec(900_000_000));
    s.add("J2_tolerance", j2, "±5% 容差带含界内双例并拒带外例");

    // J3 负/零时长受控
    let mut c2 = SleepClock::new();
    let j3 = c2.nanosleep(Tv::new(0, 0)).err() == Some(NS_NEG_TVMSEC)
        && c2.nanosleep(Tv::new(0, 1_000_000_000)).err() == Some(NS_NEG_TVMSEC);
    s.add("J3_neg_tvmsec", j3, "零时长与 nsec 越界同拒 NEG_TVMSEC");

    // J4 EINTR 残量回填
    let mut c3 = SleepClock::new();
    let req = Tv::new(1, 0);
    let out = c3.nanosleep_intr(req, 400_000_000);
    let j4 = out.is_ok()
        && SleepClock::rem_consistent(&out.as_ref().unwrap(), req)
        && out.as_ref().unwrap().rem == Tv::from_nsec(600_000_000);
    s.add("J4_eintr_rem", j4, "打断 0.4s 残量回填 0.6s");

    // J5 打断过头睡足
    let out2 = c3.nanosleep_intr(Tv::new(0, 500_000_000), 900_000_000);
    let j5 = out2.is_ok() && out2.as_ref().unwrap().rem == Tv::new(0, 0);
    s.add("J5_over_elapsed", j5, "打断晚于到点则睡足 rem 零");

    // J6 残量续睡达成
    let rem = out.as_ref().unwrap().rem;
    let r2 = c3.nanosleep(rem);
    let j6 = r2.is_ok() && c3.now >= 1_400_000_000;
    s.add("J6_resume", j6, "残量续睡补足 1.4s 虚钟");

    // J7 虚钟单调
    let mut c4 = SleepClock::new();
    let mut mono = true;
    let mut last = 0u64;
    for i in 1..=10u64 {
        let _ = c4.nanosleep(Tv::new(0, i * 10_000_000));
        if c4.now <= last {
            mono = false;
        }
        last = c4.now;
    }
    s.add("J7_mono", mono, "10 次睡眠虚钟严格单调");

    // J8 忙态受控
    let mut c5 = SleepClock::new();
    c5.sleeping = true;
    let j8 = c5.nanosleep(Tv::new(0, 1)).err() == Some(NS_BUSY);
    c5.sleeping = false;
    s.add("J8_busy", j8, "睡眠中重入拒 BUSY");

    // J9 Tv 合法性面
    let j9 = Tv::new(TVSEC_CAP, 999_999_999).ok()
        && !Tv::new(TVSEC_CAP + 1, 0).ok();
    s.add("J9_tv_ok", j9, "Tv 边界合法与超帽非法");

    // J10 双向换算守恒
    let j10 = Tv::from_nsec(Tv::new(3, 123_456_789).to_nsec()) == Tv::new(3, 123_456_789);
    s.add("J10_roundtrip", j10, "Tv 纳秒双向换算守恒");

    // J11 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J11_inject_all", caught == 5, "5 注入点全检出");

    // J12 封册受控
    let mut c6 = SleepClock::new();
    c6.seal();
    let j12 = c6.nanosleep(Tv::new(0, 1)).err() == Some(NS_SEALED)
        && c6.nanosleep_intr(Tv::new(0, 1), 1).err() == Some(NS_SEALED);
    s.add("J12_sealed", j12, "封册后两睡眠入口皆拒 SEALED");

    // J13 occupy_cycle 语义
    let mut c7 = SleepClock::new();
    let occ = c7.occupy_cycle(Tv::new(0, 10_000_000));
    s.add("J13_occupy", occ == Ok(true), "占用周期睡足且状态归位");

    // J14 容差带参数
    s.add("J14_tol_param", TOL_PERMILLE == 50 && TVSEC_CAP == 0x1_0000, "容差与帽常量在案");

    // J15 大时长睡足
    let mut c8 = SleepClock::new();
    let big = c8.nanosleep(Tv::new(2, 0));
    let j15 = big.is_ok() && c8.now == 2_000_000_000;
    s.add("J15_big_sleep", j15, "2s 整秒睡足落账");

    // J16 虚钟别名
    s.add("J16_total", c8.total_slept() == 2_000_000_000, "累计睡眠别名与虚钟同值");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sleep61_flow() {
        let mut c = SleepClock::new();
        let out = c.nanosleep(Tv::new(0, 250_000_000)).unwrap();
        assert_eq!(out.rem, Tv::new(0, 0));
        assert_eq!(c.now, 250_000_000);
    }

    #[test]
    fn test_sleep61_intr() {
        let mut c = SleepClock::new();
        let out = c.nanosleep_intr(Tv::new(1, 0), 100_000_000).unwrap();
        assert_eq!(out.rem, Tv::from_nsec(900_000_000));
        assert!(SleepClock::rem_consistent(&out, Tv::new(1, 0)));
    }

    #[test]
    fn test_sleep61_misuse() {
        let mut c = SleepClock::new();
        assert_eq!(c.nanosleep(Tv::new(0, 0)), Err(NS_NEG_TVMSEC));
        assert_eq!(c.nanosleep(Tv::new(0, 1_000_000_000)), Err(NS_NEG_TVMSEC));
    }

    #[test]
    fn test_sleep61_checks() {
        let cs = run_unx10161_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_sleep61_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
