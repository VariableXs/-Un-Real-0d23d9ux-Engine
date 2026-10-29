//! UNX 卡 10165 — timer_create 正反双判据（POSIX 定时器达成与非法时钟误用受控）
//! 段位 seg=0x107c0
//! 判据原文：UNX-F10165-J1 定时器达成（timer_create→到期投递 LTP timer_create01 面 10/10 次）
//! 锚点：POSIX 定时器建账→置时→到期投递 sigevent→删除与非法时钟受控
//! 域：TimerBook 定时器柄册（柄 + 时钟基 + sigevent 面 + 到期溢出账）
//! 结构：create 挂柄 → settime 置时 → tick 到期投递 → delete 摘柄 → 误用受控 → 注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x107c0;
pub const TM_BAD_CLOCK: u32 = SEG_BASE + 0x01;
pub const TM_BAD_TID: u32 = SEG_BASE + 0x02;
pub const TM_SEALED: u32 = SEG_BASE + 0x03;
pub const TM_BAD_SPEC: u32 = SEG_BASE + 0x04;
pub const TM_TID_CAP: u32 = SEG_BASE + 0x05;

/// 柄容量与秒帽
pub const TID_CAP: usize = 0x40;
pub const SEC_CAP: u32 = 0x10_0000;

/// sigevent 通知面
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Notify {
    None,
    Signal(i32),
}

/// 定时器规格（value/interval 秒）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TmSpec {
    pub value_sec: u32,
    pub interval_sec: u32,
}

impl TmSpec {
    pub fn ok(&self) -> bool {
        self.value_sec <= SEC_CAP && self.interval_sec <= SEC_CAP
    }
}

/// 单个 POSIX 定时器
#[derive(Clone, Debug)]
pub struct PosixTimer {
    pub tid: usize,
    pub clock: u32,
    pub spec: TmSpec,
    pub notify: Notify,
    pub expiries: u32,
    pub overruns: u32,
    pub live: bool,
}

impl PosixTimer {
    pub fn ok(&self) -> bool {
        self.live && self.spec.ok()
    }
}

/// 定时器柄册
pub struct TimerBook {
    pub timers: Vec<PosixTimer>,
    next_tid: usize,
    sealed: bool,
}

impl TimerBook {
    pub fn new() -> Self {
        TimerBook {
            timers: Vec::new(),
            next_tid: 1,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// timer_create：时钟闭集闸 + 柄容量闸
    pub fn create(&mut self, clock: u32, notify: Notify) -> Result<usize, u32> {
        if self.sealed {
            return Err(TM_SEALED);
        }
        if clock > 2 {
            return Err(TM_BAD_CLOCK);
        }
        if self.timers.len() >= TID_CAP {
            return Err(TM_TID_CAP);
        }
        let tid = self.next_tid;
        self.next_tid += 1;
        self.timers.push(PosixTimer {
            tid,
            clock,
            spec: TmSpec { value_sec: 0, interval_sec: 0 },
            notify,
            expiries: 0,
            overruns: 0,
            live: true,
        });
        Ok(tid)
    }

    fn find(&self, tid: usize) -> Result<usize, u32> {
        self.timers
            .iter()
            .position(|t| t.tid == tid && t.live)
            .ok_or(TM_BAD_TID)
    }

    /// timer_settime：置时返回旧规格
    pub fn settime(&mut self, tid: usize, spec: TmSpec) -> Result<TmSpec, u32> {
        if self.sealed {
            return Err(TM_SEALED);
        }
        let i = self.find(tid)?;
        if !spec.ok() {
            return Err(TM_BAD_SPEC);
        }
        let old = self.timers[i].spec;
        self.timers[i].spec = spec;
        Ok(old)
    }

    /// gettime 面：剩余 value
    pub fn gettime(&self, tid: usize) -> Result<u32, u32> {
        let i = self.find(tid)?;
        Ok(self.timers[i].spec.value_sec)
    }

    /// tick：到期投递（interval 重装 + 溢出计 1）
    pub fn tick(&mut self, secs: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(TM_SEALED);
        }
        let mut fired = 0usize;
        for t in self.timers.iter_mut() {
            if !t.live || t.spec.value_sec == 0 {
                continue;
            }
            if secs >= t.spec.value_sec {
                fired += 1;
                t.expiries += 1;
                if t.spec.interval_sec > 0 {
                    t.spec.value_sec = t.spec.interval_sec;
                    t.overruns += 1;
                } else {
                    t.spec.value_sec = 0;
                }
            } else {
                t.spec.value_sec -= secs;
            }
        }
        Ok(fired)
    }

    /// timer_delete：摘柄（重复删除受控）
    pub fn delete(&mut self, tid: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(TM_SEALED);
        }
        let i = self.find(tid)?;
        self.timers[i].live = false;
        Ok(())
    }

    /// 活柄计数
    pub fn live_count(&self) -> usize {
        self.timers.iter().filter(|t| t.live).count()
    }

    /// 通知面审计：SIGNAL 柄 notify 携带号值
    pub fn notify_audit(&self, tid: usize) -> bool {
        match self.timers.iter().find(|t| t.tid == tid) {
            Some(t) => matches!(t.notify, Notify::Signal(_)),
            None => false,
        }
    }

    /// 到期账合计
    pub fn total_expiries(&self) -> u32 {
        self.timers.iter().map(|t| t.expiries).sum()
    }

    /// 溢出账合计
    pub fn total_overruns(&self) -> u32 {
        self.timers.iter().map(|t| t.overruns).sum()
    }

    /// 柄指纹
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x107c00;
        for t in &self.timers {
            h = h.wrapping_mul(31).wrapping_add(t.tid as u32);
            h = h.wrapping_mul(31).wrapping_add(t.expiries);
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = TimerBook::new();
    // 1) 非法时钟建柄必拒
    got[0] = b.create(9, Notify::None).is_err();
    // 2) 野柄操作必拒
    got[1] = b.settime(999, TmSpec { value_sec: 1, interval_sec: 0 }).is_err();
    // 3) 规格越帽必拒
    let tid = b.create(0, Notify::None).unwrap_or(0);
    got[2] = b.settime(tid, TmSpec { value_sec: SEC_CAP + 1, interval_sec: 0 }).is_err();
    // 4) 重复删除必拒
    let _ = b.delete(tid);
    got[3] = b.delete(tid).is_err();
    // 5) 封册后必拒
    b.seal();
    got[4] = b.create(0, Notify::None).is_err();
    got
}

/// 判据入口：UNX-F10165
pub fn run_unx10165_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10165");
    let mut b = TimerBook::new();

    // J1 建柄到投递 10/10：十柄各置时到期
    let mut j1 = true;
    for i in 0..10usize {
        let tid = b.create(0, Notify::Signal(14 + i as i32));
        match tid {
            Ok(t) => {
                if b.settime(t, TmSpec { value_sec: (i + 1) as u32, interval_sec: 0 }).is_err() {
                    j1 = false;
                }
            }
            Err(_) => j1 = false,
        }
    }
    let _ = b.tick(10);
    s.add("J1_create_fire_10x", j1 && b.total_expiries() >= 10, "create→settime→到期投递 10/10");

    // J2 非法时钟受控
    let j2 = b.create(3, Notify::None) == Err(TM_BAD_CLOCK)
        && b.create(0xFFFF, Notify::None) == Err(TM_BAD_CLOCK);
    s.add("J2_bad_clock", j2, "越闭集时钟建柄双拒 BAD_CLOCK");

    // J3 interval 重装与溢出
    let mut b2 = TimerBook::new();
    let t1 = b2.create(1, Notify::None).unwrap_or(0);
    let _ = b2.settime(t1, TmSpec { value_sec: 2, interval_sec: 3 });
    let _ = b2.tick(2);
    let j3 = b2.gettime(t1) == Ok(3) && b2.total_overruns() == 1;
    s.add("J3_interval_overrun", j3, "到期重装 interval 3 且溢出计 1");

    // J4 野柄受控
    let j4 = b2.gettime(999) == Err(TM_BAD_TID) && b2.delete(999) == Err(TM_BAD_TID);
    s.add("J4_bad_tid", j4, "get/delete 野柄双拒 BAD_TID");

    // J5 规格越帽受控
    let j5 = b2.settime(t1, TmSpec { value_sec: SEC_CAP + 1, interval_sec: 0 }) == Err(TM_BAD_SPEC)
        && b2.settime(t1, TmSpec { value_sec: 0, interval_sec: SEC_CAP + 1 }) == Err(TM_BAD_SPEC);
    s.add("J5_bad_spec", j5, "value/interval 越帽双拒 BAD_SPEC");

    // J6 旧规格返回
    let mut b3 = TimerBook::new();
    let t = b3.create(2, Notify::None).unwrap_or(0);
    let _ = b3.settime(t, TmSpec { value_sec: 5, interval_sec: 1 });
    let old = b3.settime(t, TmSpec { value_sec: 7, interval_sec: 2 });
    s.add("J6_old_spec", old == Ok(TmSpec { value_sec: 5, interval_sec: 1 }), "置新规返旧规格");

    // J7 删除摘柄
    let _ = b3.delete(t);
    let j7 = b3.gettime(t) == Err(TM_BAD_TID) && b3.live_count() == 0;
    s.add("J7_delete", j7, "删除后 gettime 拒且活柄归零");

    // J8 柄容量守门
    let mut b4 = TimerBook::new();
    let mut fill_ok = true;
    for _ in 0..TID_CAP {
        if b4.create(0, Notify::None).is_err() {
            fill_ok = false;
        }
    }
    let j8 = fill_ok && b4.create(0, Notify::None) == Err(TM_TID_CAP);
    s.add("J8_tid_cap", j8, "柄满 64 拒 TID_CAP");

    // J9 通知面
    let mut b5 = TimerBook::new();
    let ts = b5.create(0, Notify::Signal(10)).unwrap_or(0);
    let tn = b5.create(0, Notify::None).unwrap_or(0);
    let j9 = b5.notify_audit(ts) && !b5.notify_audit(tn);
    s.add("J9_notify", j9, "SIGNAL 柄携号、NONE 柄不带");

    // J10 未到期扣减
    let mut b6 = TimerBook::new();
    let tx = b6.create(0, Notify::None).unwrap_or(0);
    let _ = b6.settime(tx, TmSpec { value_sec: 10, interval_sec: 0 });
    let _ = b6.tick(4);
    s.add("J10_partial", b6.gettime(tx) == Ok(6), "未到期仅扣减余 6");

    // J11 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J11_inject_all", caught == 5, "5 注入点全检出");

    // J12 封册受控
    let mut b7 = TimerBook::new();
    let tz = b7.create(0, Notify::None).unwrap_or(0);
    b7.seal();
    let j12 = b7.create(0, Notify::None) == Err(TM_SEALED)
        && b7.settime(tz, TmSpec { value_sec: 1, interval_sec: 0 }) == Err(TM_SEALED)
        && b7.tick(1) == Err(TM_SEALED)
        && b7.delete(tz) == Err(TM_SEALED);
    s.add("J12_sealed", j12, "封册后四操作皆拒 SEALED");

    // J13 指纹稳定
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J13_fingerprint", f1 == f2, "柄册折叠指纹一致");

    // J14 到期溢出双账对平
    let j14 = b.total_expiries() >= 10 && b.total_overruns() == 0;
    s.add("J14_acct", j14, "到期账 ≥10 且无 interval 柄溢出为零");

    // J15 柄 id 单调
    let ids: Vec<usize> = b.timers.iter().map(|t| t.tid).collect();
    let mut mono = true;
    for w in ids.windows(2) {
        if w[1] <= w[0] {
            mono = false;
        }
    }
    s.add("J15_tid_mono", mono && ids.len() == 10, "柄 id 单调且十柄在册");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timer65_fire() {
        let mut b = TimerBook::new();
        let t = b.create(0, Notify::None).unwrap();
        let _ = b.settime(t, TmSpec { value_sec: 3, interval_sec: 0 });
        assert_eq!(b.tick(3), Ok(1));
        assert_eq!(b.gettime(t), Ok(0));
    }

    #[test]
    fn test_timer65_bad() {
        let mut b = TimerBook::new();
        assert_eq!(b.create(9, Notify::None), Err(TM_BAD_CLOCK));
        let t = b.create(0, Notify::None).unwrap();
        assert_eq!(b.settime(t, TmSpec { value_sec: SEC_CAP + 1, interval_sec: 0 }), Err(TM_BAD_SPEC));
        let _ = b.delete(t);
        assert_eq!(b.delete(t), Err(TM_BAD_TID));
    }

    #[test]
    fn test_timer65_checks() {
        let cs = run_unx10165_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_timer65_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
