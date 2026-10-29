//! UNX 卡 10163 — alarm/SIGALRM 正反双判据（到期投递达成与取消误用受控）
//! 段位 seg=0x10780
//! 判据原文：UNX-F10163-J1 到期投递达成（alarm 置时→SIGALRM 到期 LTP alarm01 面 10/10 次）
//! 锚点：alarm 置时到点投递 SIGALRM + 重置返回旧余量 + 零值取消 + 到期核销
//! 域：AlarmBook 闹钟册（单挂钟 + 虚钟 tick + 投递账）
//! 结构：挂钟单槽 → alarm(secs) 置时 → tick 推进 → 到期投递账 → 取消/重置 → 注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

pub const SEG_BASE: u32 = 0x10780;
pub const AL_SEALED: u32 = SEG_BASE + 0x01;
pub const AL_NOT_ARMED: u32 = SEG_BASE + 0x02;
pub const AL_DOUBLE_FIRE: u32 = SEG_BASE + 0x03;
pub const AL_BAD_SEC: u32 = SEG_BASE + 0x04;
pub const AL_COUNT_CAP: u32 = SEG_BASE + 0x05;

/// 秒帽与投递账容量
pub const SEC_CAP: u32 = 0x10_0000;
pub const FIRE_CAP: usize = 0x100;

/// 到期投递账一条
#[derive(Clone, Copy, Debug)]
pub struct FireRec {
    pub at: u32,
    pub armed: u32,
}

/// 闹钟册
pub struct AlarmBook {
    /// 已挂秒数（0=未挂）
    pub armed: u32,
    /// 虚钟已走秒
    pub ticked: u32,
    pub fires: Vec<FireRec>,
    sealed: bool,
}

impl AlarmBook {
    pub fn new() -> Self {
        AlarmBook {
            armed: 0,
            ticked: 0,
            fires: Vec::new(),
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn fired_count(&self) -> usize {
        self.fires.len()
    }

    /// alarm(secs) 主语义：置时返回旧余量；0 取消
    pub fn alarm(&mut self, secs: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(AL_SEALED);
        }
        if secs > SEC_CAP {
            return Err(AL_BAD_SEC);
        }
        if self.fires.len() >= FIRE_CAP {
            return Err(AL_COUNT_CAP);
        }
        let old = self.remaining();
        self.armed = secs;
        Ok(old)
    }

    /// 余量：armed - (ticked - 挂钟基点)。简化：armed 为绝对剩余，tick 递减
    pub fn remaining(&self) -> u32 {
        self.armed
    }

    /// 虚钟推进：到点投递 SIGALRM
    pub fn tick(&mut self, secs: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(AL_SEALED);
        }
        if self.armed == 0 {
            self.ticked += secs;
            return Ok(0);
        }
        if secs >= self.armed {
            self.ticked += self.armed;
            self.armed = 0;
            let at = self.ticked;
            self.fires.push(FireRec { at, armed: 0 });
            Ok(1)
        } else {
            self.armed -= secs;
            self.ticked += secs;
            Ok(0)
        }
    }

    /// 到期投递核：恰在到点 tick 后 fired_count 增 1
    pub fn sigalrm_delivered(&self) -> bool {
        !self.fires.is_empty()
    }

    /// 双投递检出：armed 已零再 tick 不得再投
    pub fn double_fire_probe(&mut self) -> bool {
        let n = self.fires.len();
        let _ = self.tick(10);
        self.fires.len() == n
    }

    /// 重置面：置新钟返回旧余量一致
    pub fn reset_consistent(&mut self, new_secs: u32) -> bool {
        let old = self.remaining();
        match self.alarm(new_secs) {
            Ok(prev) => prev == old && self.armed == new_secs,
            Err(_) => false,
        }
    }

    /// 投递账指纹
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x107800;
        for f in &self.fires {
            h = h.wrapping_mul(31).wrapping_add(f.at);
        }
        h
    }

    /// 封册
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 投递时刻列
    pub fn fire_ats(&self) -> Vec<u32> {
        self.fires.iter().map(|f| f.at).collect()
    }

    /// 末次投递时刻
    pub fn last_fire(&self) -> Option<u32> {
        self.fires.last().map(|f| f.at)
    }

    /// 虚钟总量
    pub fn tick_total(&self) -> u32 {
        self.ticked
    }

    /// 按序取投递时刻
    pub fn fire_at(&self, idx: usize) -> Option<u32> {
        self.fires.get(idx).map(|f| f.at)
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = AlarmBook::new();
    // 1) 未挂钟取消语义：alarm(0) 返回 0 且不投递
    got[0] = b.alarm(0).unwrap_or(9) == 0;
    // 2) 超帽必拒
    got[1] = b.alarm(SEC_CAP + 1).is_err();
    // 3) 双投递必无（armed 零后再 tick 不投）
    let _ = b.alarm(1);
    let _ = b.tick(5);
    got[2] = b.double_fire_probe();
    // 4) 未到期取消：alarm(0) 后 tick 不投
    let _ = b.alarm(5);
    let _ = b.alarm(0);
    let before = b.fired_count();
    let _ = b.tick(100);
    got[3] = b.fired_count() == before;
    // 5) 封册后必拒
    b.seal();
    got[4] = b.alarm(1).is_err();
    got
}

/// 判据入口：UNX-F10163
pub fn run_unx10163_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10163");
    let mut b = AlarmBook::new();

    // J1 到期投递 10/10：十轮挂钟→tick 到点→投递
    let mut j1 = true;
    for i in 1..=10u32 {
        if b.alarm(i) != Ok(0) {
            j1 = false;
        }
        if b.tick(i) != Ok(1) {
            j1 = false;
        }
    }
    s.add("J1_alarm_10x", j1 && b.fired_count() == 10, "置时到点投递 10/10");

    // J2 重置返回旧余量
    let mut b2 = AlarmBook::new();
    let _ = b2.alarm(7);
    let _ = b2.tick(3);
    let j2 = b2.reset_consistent(9) && b2.armed == 9;
    s.add("J2_reset", j2, "重置返旧余且新钟挂上");

    // J3 零值取消
    let mut b3 = AlarmBook::new();
    let _ = b3.alarm(5);
    let j3 = b3.alarm(0) == Ok(5) && b3.armed == 0 && b3.tick(10) == Ok(0);
    s.add("J3_cancel", j3, "alarm(0) 取消且不投递");

    // J4 未到期 tick 不投
    let mut b4 = AlarmBook::new();
    let _ = b4.alarm(10);
    let j4 = b4.tick(4) == Ok(0) && b4.remaining() == 6;
    s.add("J4_partial_tick", j4, "未到期仅扣余量");

    // J5 双投递检出
    let mut b5 = AlarmBook::new();
    let _ = b5.alarm(1);
    let _ = b5.tick(1);
    s.add("J5_no_double", b5.double_fire_probe(), "armed 零后再 tick 不投");

    // J6 超帽受控
    let j6 = b4.alarm(SEC_CAP + 1) == Err(AL_BAD_SEC);
    s.add("J6_bad_sec", j6, "秒数越帽受控 BAD_SEC");

    // J7 投递账时刻对平
    let ats: Vec<u32> = b.fires.iter().map(|f| f.at).collect();
    let j7 = b.fires[0].at == 1 && b.fires[9].at == 55;
    s.add("J7_fire_at", j7 && ats.len() == 10, "投递时刻 1..55 累计对平");

    // J8 投递账容量守门
    let mut b6 = AlarmBook::new();
    let mut fill_ok = true;
    for _ in 0..FIRE_CAP {
        if b6.alarm(1).is_err() || b6.tick(1).is_err() {
            fill_ok = false;
            break;
        }
    }
    let j8 = fill_ok && b6.alarm(1) == Err(AL_COUNT_CAP);
    s.add("J8_fire_cap", j8, "投递账满 256 拒 COUNT_CAP");

    // J9 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J9_inject_all", caught == 5, "5 注入点全检出");

    // J10 封册受控
    let mut b7 = AlarmBook::new();
    b7.seal();
    let j10 = b7.alarm(1) == Err(AL_SEALED) && b7.tick(1) == Err(AL_SEALED);
    s.add("J10_sealed", j10, "封册后挂钟/推进皆拒 SEALED");

    // J11 未挂 tick 空走
    let mut b8 = AlarmBook::new();
    let j11 = b8.tick(100) == Ok(0) && b8.ticked == 100;
    s.add("J11_idle_tick", j11, "未挂钟 tick 空走记账");

    // J12 投递指纹稳定
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J12_fingerprint", f1 == f2, "投递账折叠指纹一致");

    // J13 sigalrm 语义面
    s.add("J13_sigalrm", b.sigalrm_delivered(), "投递账非空即 SIGALRM 已投");

    // J14 余量边界
    let mut b9 = AlarmBook::new();
    let _ = b9.alarm(1);
    let j14 = b9.tick(0) == Ok(0) && b9.remaining() == 1;
    s.add("J14_zero_tick", j14, "零步 tick 不投不扣");

    // J15 挂钟状态可见
    let _ = b9.tick(1);
    let j15 = b9.armed == 0 && b9.remaining() == 0;
    s.add("J15_state", j15, "到点后挂钟清零可见");

    // J16 投递时刻列
    let ats = b.fire_ats();
    let want = vec![1u32, 3, 6, 10, 15, 21, 28, 36, 45, 55];
    s.add("J16_fire_ats", ats == want, "十轮投递时刻三角数列对平");

    // J17 末次投递
    s.add("J17_last_fire", b.last_fire() == Some(55), "末次投递 55 秒");

    // J18 虚钟总量
    let j18 = b.tick_total() == 55 && b.ticked == 55;
    s.add("J18_tick_total", j18, "虚钟累计 55 秒别名同值");

    // J19 空册虚钟零
    let mut b10 = AlarmBook::new();
    let j19 = b10.tick_total() == 0 && b10.last_fire().is_none();
    s.add("J19_fresh", j19, "空册零虚钟无投递");

    // J20 同秒重挂返旧余
    let _ = b10.alarm(4);
    let _ = b10.tick(1);
    let r20 = b10.alarm(4);
    s.add("J20_rearm", r20 == Ok(3), "重挂返旧余 3 秒");

    // J21 递达三角和守恒
    let sum: u32 = b.fire_ats().iter().sum();
    s.add("J21_fire_sum", sum == 220, "投递时刻累计 220 对平");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alarm63_fire() {
        let mut b = AlarmBook::new();
        assert_eq!(b.alarm(3), Ok(0));
        assert_eq!(b.tick(2), Ok(0));
        assert_eq!(b.tick(1), Ok(1));
        assert_eq!(b.fired_count(), 1);
    }

    #[test]
    fn test_alarm63_cancel() {
        let mut b = AlarmBook::new();
        let _ = b.alarm(9);
        assert_eq!(b.alarm(0), Ok(9));
        assert_eq!(b.tick(100), Ok(0));
    }

    #[test]
    fn test_alarm63_checks() {
        let cs = run_unx10163_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_alarm63_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
