//! UNX 卡 10164 — setitimer 正反双判据（三类定时达成与非法 which 误用受控）
//! 段位 seg=0x107a0
//! 判据原文：UNX-F10164-J1 三类定时达成（ITIMER_REAL 到期 LTP setitimer01 面 10/10 次）
//! 锚点：ITIMER_REAL/VIRTUAL/PROF 三槽独立定时 + 非法 which 受控 + 间隔重装
//! 域：ItimerBook 三槽定时册（value/interval 双针 + 虚钟 tick + 到期账）
//! 结构：三槽（REAL/VIRT/PROF）→ setitimer 置双针 → tick 扣减 → 到期重装 interval → 注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x107a0;
pub const IT_BAD_WHICH: u32 = SEG_BASE + 0x01;
pub const IT_SEALED: u32 = SEG_BASE + 0x02;
pub const IT_BAD_TV: u32 = SEG_BASE + 0x03;
pub const IT_CAP: u32 = SEG_BASE + 0x04;
pub const IT_EXPIRE_CAP: u32 = SEG_BASE + 0x05;

/// which 闭集
pub const W_REAL: usize = 0;
pub const W_VIRT: usize = 1;
pub const W_PROF: usize = 2;
pub const NSLOTS: usize = 3;

/// 秒帽与到期账容量
pub const TV_SEC_CAP: u32 = 0x10_0000;
pub const EXPIRE_CAP: usize = 0x80;

/// 定时器双针（微秒精度）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItvSpec {
    pub value_usec: u64,
    pub interval_usec: u64,
}

impl ItvSpec {
    pub fn ok(&self) -> bool {
        self.value_usec <= TV_SEC_CAP as u64 * 1_000_000
            && self.interval_usec <= TV_SEC_CAP as u64 * 1_000_000
    }
}

/// 单槽状态
#[derive(Clone, Copy)]
pub struct Slot {
    pub spec: ItvSpec,
    pub expiries: u32,
}

/// 三槽定时册
pub struct ItimerBook {
    pub slots: [Slot; NSLOTS],
    /// 虚钟（微秒），tick 源
    pub clock_usec: u64,
    pub sealed: bool,
}

impl ItimerBook {
    pub fn new() -> Self {
        let zero = Slot {
            spec: ItvSpec { value_usec: 0, interval_usec: 0 },
            expiries: 0,
        };
        ItimerBook {
            slots: [zero, zero, zero],
            clock_usec: 0,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// which 校验
    pub fn which_ok(which: usize) -> bool {
        which < NSLOTS
    }

    /// setitimer 主语义：置双针返回旧 value
    pub fn setitimer(&mut self, which: usize, spec: ItvSpec) -> Result<ItvSpec, u32> {
        if self.sealed {
            return Err(IT_SEALED);
        }
        if !Self::which_ok(which) {
            return Err(IT_BAD_WHICH);
        }
        if !spec.ok() {
            return Err(IT_BAD_TV);
        }
        let old = self.slots[which].spec;
        self.slots[which].spec = spec;
        Ok(old)
    }

    /// getitimer 面
    pub fn getitimer(&self, which: usize) -> Result<ItvSpec, u32> {
        if !Self::which_ok(which) {
            return Err(IT_BAD_WHICH);
        }
        Ok(self.slots[which].spec)
    }

    /// tick 推进：扣减活动槽 value，到期重装 interval 并计一次 expiry
    pub fn tick(&mut self, usec: u64) -> Result<usize, u32> {
        if self.sealed {
            return Err(IT_SEALED);
        }
        if self.clock_usec + usec > TV_SEC_CAP as u64 * 1_000_000 {
            return Err(IT_CAP);
        }
        self.clock_usec += usec;
        let mut fired = 0usize;
        for w in 0..NSLOTS {
            let v = self.slots[w].spec.value_usec;
            if v == 0 {
                continue;
            }
            if usec >= v {
                // 到期：重装 interval
                fired += 1;
                self.slots[w].expiries += 1;
                if self.slots[w].expiries as usize > EXPIRE_CAP {
                    return Err(IT_EXPIRE_CAP);
                }
                self.slots[w].spec.value_usec = self.slots[w].spec.interval_usec;
            } else {
                self.slots[w].spec.value_usec = v - usec;
            }
        }
        Ok(fired)
    }

    /// 三槽独立审计：REAL 到期不带动 VIRT/PROF 到期账
    pub fn isolation(&mut self) -> bool {
        let v0 = self.slots[W_VIRT].expiries;
        let p0 = self.slots[W_PROF].expiries;
        let _ = self.setitimer(W_REAL, ItvSpec { value_usec: 1, interval_usec: 0 });
        let _ = self.tick(10);
        self.slots[W_VIRT].expiries == v0 && self.slots[W_PROF].expiries == p0
    }

    /// 到期账合计
    pub fn total_expiries(&self) -> u32 {
        self.slots.iter().map(|s| s.expiries).sum()
    }

    /// 三槽到期指纹
    pub fn render_slots(&self) -> String {
        let mut s = String::new();
        for (i, sl) in self.slots.iter().enumerate() {
            s.push_str(&format!("{}:{};", i, sl.expiries));
        }
        s
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 指定槽到期账
    pub fn slot_expiries(&self, which: usize) -> Result<u32, u32> {
        if !Self::which_ok(which) {
            return Err(IT_BAD_WHICH);
        }
        Ok(self.slots[which].expiries)
    }

    /// 虚钟总量
    pub fn clock_total(&self) -> u64 {
        self.clock_usec
    }

    /// 指定槽双针读取
    pub fn slot_spec(&self, which: usize) -> Result<ItvSpec, u32> {
        if !Self::which_ok(which) {
            return Err(IT_BAD_WHICH);
        }
        Ok(self.slots[which].spec)
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ItimerBook::new();
    // 1) 非法 which 必拒
    got[0] = b.setitimer(9, ItvSpec { value_usec: 1, interval_usec: 0 }).is_err();
    // 2) value 越帽必拒
    got[1] = b.setitimer(W_REAL, ItvSpec { value_usec: TV_SEC_CAP as u64 * 2_000_000, interval_usec: 0 }).is_err();
    // 3) 三槽到期独立（REAL 到期不带动 VIRT）
    got[2] = b.isolation();
    // 4) 零 value 槽不 tick 触发
    let _ = b.setitimer(W_VIRT, ItvSpec { value_usec: 0, interval_usec: 50 });
    let before = b.total_expiries();
    let _ = b.tick(100);
    got[3] = b.total_expiries() == before;
    // 5) 封册后必拒
    b.seal();
    got[4] = b.setitimer(W_REAL, ItvSpec { value_usec: 1, interval_usec: 0 }).is_err();
    got
}

/// 判据入口：UNX-F10164
pub fn run_unx10164_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10164");
    let mut b = ItimerBook::new();

    // J1 三类定时 10/10：REAL 十轮置时到期
    let mut j1 = true;
    for i in 0..10u64 {
        let spec = ItvSpec { value_usec: (i + 1) * 100, interval_usec: 0 };
        if b.setitimer(W_REAL, spec) != Ok(ItvSpec { value_usec: 0, interval_usec: 0 }) {
            j1 = false;
        }
        if b.tick((i + 1) * 100) != Ok(1) {
            j1 = false;
        }
    }
    s.add("J1_real_10x", j1 && b.slots[W_REAL].expiries == 10, "ITIMER_REAL 到期 10/10");

    // J2 三槽并存置时
    let j2 = b.setitimer(W_VIRT, ItvSpec { value_usec: 500, interval_usec: 250 }).is_ok()
        && b.setitimer(W_PROF, ItvSpec { value_usec: 800, interval_usec: 400 }).is_ok()
        && b.getitimer(W_VIRT).unwrap().value_usec == 500
        && b.getitimer(W_PROF).unwrap().value_usec == 800;
    s.add("J2_three_slots", j2, "VIRT/PROF 双针在册可读");

    // J3 interval 重装
    let j3 = b.tick(500) == Ok(1) && b.getitimer(W_VIRT).unwrap().value_usec == 250;
    s.add("J3_interval_reload", j3, "VIRT 到期后重装 interval 250");

    // J4 再 tick 扣减
    let j4 = b.tick(100) == Ok(0) && b.getitimer(W_VIRT).unwrap().value_usec == 150;
    s.add("J4_partial", j4, "未到期仅扣减 100");

    // J5 非法 which 受控
    let j5 = b.setitimer(3, ItvSpec { value_usec: 1, interval_usec: 0 }) == Err(IT_BAD_WHICH)
        && b.getitimer(9) == Err(IT_BAD_WHICH)
        && b.tick(1).is_ok();
    s.add("J5_bad_which", j5, "set/get 双路非法 which 受控");

    // J6 value 越帽受控
    let j6 = b.setitimer(W_REAL, ItvSpec { value_usec: TV_SEC_CAP as u64 * 1_000_000 + 1, interval_usec: 0 }) == Err(IT_BAD_TV);
    s.add("J6_bad_tv", j6, "value 越帽受控 BAD_TV");

    // J7 旧值返回
    let mut b2 = ItimerBook::new();
    let _ = b2.setitimer(W_PROF, ItvSpec { value_usec: 300, interval_usec: 0 });
    let old = b2.setitimer(W_PROF, ItvSpec { value_usec: 600, interval_usec: 0 });
    s.add("J7_old_value", old == Ok(ItvSpec { value_usec: 300, interval_usec: 0 }), "置新针返旧 value");

    // J8 三槽独立
    s.add("J8_isolation", b.isolation(), "REAL 到期不带动 VIRT/PROF");

    // J9 到期账合计
    let want = b.slots.iter().map(|sl| sl.expiries).sum::<u32>();
    s.add("J9_expire_sum", b.total_expiries() == want && b.total_expiries() >= 11, "到期账合计 ≥11 对平");

    // J10 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J10_inject_all", caught == 5, "5 注入点全检出");

    // J11 封册受控
    let mut b3 = ItimerBook::new();
    b3.seal();
    let j11 = b3.setitimer(W_REAL, ItvSpec { value_usec: 1, interval_usec: 0 }) == Err(IT_SEALED)
        && b3.tick(1) == Err(IT_SEALED);
    s.add("J11_sealed", j11, "封册后置针/推进皆拒 SEALED");

    // J12 零 value 取消定时
    let mut b4 = ItimerBook::new();
    let _ = b4.setitimer(W_VIRT, ItvSpec { value_usec: 200, interval_usec: 0 });
    let old = b4.setitimer(W_VIRT, ItvSpec { value_usec: 0, interval_usec: 0 });
    let j12 = old.is_ok() && b4.tick(1000) == Ok(0);
    s.add("J12_zero_cancel", j12, "value 零取消后 tick 不触发");

    // J13 指纹稳定
    let f1 = b.render_slots();
    let f2 = b.render_slots();
    s.add("J13_fingerprint", f1 == f2 && f1.contains("0:"), "三槽到期指纹稳定");

    // J14 ItvSpec 合法面
    let j14 = ItvSpec { value_usec: 0, interval_usec: TV_SEC_CAP as u64 * 1_000_000 }.ok();
    s.add("J14_spec_ok", j14, "边界双针合法");

    // J15 虚钟推进对平
    let c0 = b.clock_usec;
    let _ = b.tick(123);
    s.add("J15_clock", b.clock_usec == c0 + 123, "虚钟推进 123 微秒对平");

    // J16 槽账分列
    let j16 = b.slot_expiries(W_REAL) == Ok(11) && b.slot_expiries(W_VIRT) == Ok(1)
        && b.slot_expiries(W_PROF) == Ok(0);
    s.add("J16_slots", j16, "REAL 11 次、VIRT 1 次、PROF 零");

    // J17 非法槽账查询受控
    let j17 = b.slot_expiries(9) == Err(IT_BAD_WHICH) && b.clock_total() == b.clock_usec;
    s.add("J17_slot_guard", j17, "野槽查询拒且虚钟别名同值");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_itimer64_expire() {
        let mut b = ItimerBook::new();
        let _ = b.setitimer(W_REAL, ItvSpec { value_usec: 100, interval_usec: 50 });
        assert_eq!(b.tick(100), Ok(1));
        assert_eq!(b.getitimer(W_REAL).unwrap().value_usec, 50);
    }

    #[test]
    fn test_itimer64_bad() {
        let mut b = ItimerBook::new();
        assert_eq!(b.setitimer(7, ItvSpec { value_usec: 1, interval_usec: 0 }), Err(IT_BAD_WHICH));
        assert_eq!(b.getitimer(9), Err(IT_BAD_WHICH));
    }

    #[test]
    fn test_itimer64_checks() {
        let cs = run_unx10164_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_itimer64_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
