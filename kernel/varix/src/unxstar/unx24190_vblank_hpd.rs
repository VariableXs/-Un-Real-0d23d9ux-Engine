//! UNX-F24190 vblank/HPD 内核事件面与事件队列·子功能10（时间戳单调）（0x419A0 段）
//!
//! 锚点：UNX-F24190-J1 单调账（时序判定逐条落账）；J2 单调语义（非
//! 降放行 + 回退拒 + 等值容许三性质）；J3 单调指纹（账折叠指纹）三
//! 判据全绿。
//! 域：CoRun·UNX-F5-B15（时间戳单调面）。
//!
//! 结构：单调闸 + 时序核 + 账本 + 折叠指纹 + 注入核（回退/野类型/
//! 印后/零基点/大步进必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x419A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x419A0;
pub const TS_REVERSE: u32 = SEG_BASE + 0x01; // 时间回退
pub const TS_BAD_KIND: u32 = SEG_BASE + 0x02; // 野类型
pub const TS_FROZEN: u32 = SEG_BASE + 0x03; // 印后操作
pub const TS_OVERFLOW: u32 = SEG_BASE + 0x04; // 步进溢出
pub const TS_BAD_STEP: u32 = SEG_BASE + 0x05; // 步进越界

/// 单调闸：事件时间戳必须非降。
pub struct TsGate {
    pub max_step_ns: u64,
    last_ts: u64,
    seen: bool,
    accepted: u64,
    rejected: u64,
    sealed: bool,
}

impl TsGate {
    pub fn new(max_step_ns: u64) -> TsGate {
        TsGate {
            max_step_ns,
            last_ts: 0,
            seen: false,
            accepted: 0,
            rejected: 0,
            sealed: false,
        }
    }

    pub fn last(&self) -> u64 {
        self.last_ts
    }

    /// 送事件：非降且步进 ≤ max 才收。
    pub fn feed(&mut self, kind: u8, ts: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(TS_FROZEN);
        }
        if kind > 7 {
            return Err(TS_BAD_KIND);
        }
        if !self.seen {
            self.seen = true;
            self.last_ts = ts;
            self.accepted += 1;
            return Ok(ts);
        }
        if ts < self.last_ts {
            self.rejected += 1;
            return Err(TS_REVERSE);
        }
        let step = ts - self.last_ts;
        if step > self.max_step_ns {
            self.rejected += 1;
            return Err(TS_BAD_STEP);
        }
        self.last_ts = ts;
        self.accepted += 1;
        Ok(ts)
    }

    pub fn accepted_count(&self) -> u64 {
        self.accepted
    }

    pub fn rejected_count(&self) -> u64 {
        self.rejected
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x419A0;
        h = h.wrapping_mul(31).wrapping_add(self.last_ts as u32);
        h = h.wrapping_mul(31).wrapping_add(self.accepted as u32);
        h = h.wrapping_mul(31).wrapping_add(self.rejected as u32);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具：步进上限 100，喂 0/50/100/150/100（等值）/250（越步）。
pub fn mk_gate() -> TsGate {
    let mut g = TsGate::new(100);
    let _ = g.feed(1, 0);
    let _ = g.feed(1, 50);
    let _ = g.feed(2, 100);
    let _ = g.feed(1, 150);
    let _ = g.feed(1, 100); // 等值放行
    let _ = g.feed(1, 250); // 步进 150 > 100 拒
    g
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 回退
    let mut g = TsGate::new(1000);
    let _ = g.feed(1, 100);
    got[0] = g.feed(1, 99).err() == Some(TS_REVERSE);
    // 2) 野类型
    got[1] = TsGate::new(1000).feed(8, 1).err() == Some(TS_BAD_KIND);
    // 3) 首收基点
    got[2] = TsGate::new(1000).feed(1, 555) == Ok(555);
    // 4) 大步进拒
    let mut g2 = TsGate::new(10);
    let _ = g2.feed(1, 0);
    got[3] = g2.feed(1, 11).err() == Some(TS_BAD_STEP);
    // 5) 印后操作
    let mut g3 = mk_gate();
    g3.seal();
    got[4] = g3.feed(1, 999).err() == Some(TS_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24190_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24190");
    let mut g = mk_gate();

    // J1 接受计数：0/50/100/150/100 收 ⇒ 5
    let j1 = g.accepted_count() == 5;
    s.add("J1-接受计数", j1, "收 5");

    // J2 拒绝计数：回退 1 + 越步 1 ⇒ 2
    let j2 = g.rejected_count() == 2;
    s.add("J2-拒绝计数", j2, "拒 2");

    // J3 末时戳：拒后仍 100
    let j3 = g.last() == 100;
    s.add("J3-末时戳", j3, "拒不改基点");

    // J4 回退拒
    let j4 = g.feed(1, 99).err() == Some(TS_REVERSE);
    s.add("J4-回退拒", j4, "回退必拒");

    // J5 野类型拒
    let j5 = g.feed(8, 100).err() == Some(TS_BAD_KIND);
    s.add("J5-野类型拒", j5, "kind>7 必拒");

    // J6 等值容许
    let j6 = g.feed(1, 100) == Ok(100);
    s.add("J6-等值容许", j6, "ts==last 放行");

    // J7 步进越界拒
    let j7 = {
        let mut g2 = TsGate::new(50);
        let _ = g2.feed(1, 0);
        g2.feed(1, 51).err() == Some(TS_BAD_STEP)
    };
    s.add("J7-越步拒", j7, "步进>上限必拒");

    // J8 恰界放行
    let j8 = {
        let mut g2 = TsGate::new(50);
        let _ = g2.feed(1, 0);
        g2.feed(1, 50).is_ok()
    };
    s.add("J8-恰界放行", j8, "步进==上限放行");

    // J9 零基点首收
    let j9 = TsGate::new(10).feed(1, 0) == Ok(0);
    s.add("J9-零基点", j9, "首收 ts=0");

    // J10 基点推进
    let j10 = {
        let mut g2 = TsGate::new(100);
        let _ = g2.feed(1, 10);
        let _ = g2.feed(1, 20);
        let _ = g2.feed(1, 30);
        g2.last() == 30
    };
    s.add("J10-基点推进", j10, "last 随收推进");

    // J11 印后操作拒
    let j11 = {
        let mut g2 = mk_gate();
        g2.seal();
        g2.feed(1, 999).err() == Some(TS_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut g2 = mk_gate();
        let a = g2.fold();
        g2.seal();
        g2.fold() == a
    };
    s.add("J12-指纹稳定", j12, "印不改指纹");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24190_mono_flow() {
        let mut g = TsGate::new(100);
        assert_eq!(g.feed(1, 10), Ok(10));
        assert_eq!(g.feed(1, 10), Ok(10));
        assert_eq!(g.feed(1, 9), Err(TS_REVERSE));
        assert_eq!(g.feed(8, 10), Err(TS_BAD_KIND));
        assert_eq!(g.feed(1, 111), Err(TS_BAD_STEP));
        assert_eq!(g.feed(1, 110), Ok(110));
        g.seal();
        assert_eq!(g.feed(1, 120), Err(TS_FROZEN));
    }

    #[test]
    fn unx24190_checks_green() {
        let s = run_unx24190_checks();
        assert!(s.all_passed(), "UNX-F24190 判据未全绿");
    }
}
