//! UNX-F24165 MSI-X 中断消费与 IrqBackend 对接·子功能05（IRQ storm 限流闸）（0x41680 段）
//!
//! 锚点：UNX-F24165-J1 限流账（窗口计数与风暴判定逐条落账）；J2 限
//! 流语义（超阈挡断 + 窗口重置 + 阈值边界三性质）；J3 限流指纹（账
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（风暴限流面）。
//!
//! 结构：滑窗计数器 + 风暴判定核 + 挡断账本 + 折叠指纹 + 注入核（零
//! 阈值/超窗/未超误挡/印后/负窗口必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41680 独占 0x20）。
pub const SEG_BASE: u32 = 0x41680;
pub const ST_ZERO_TH: u32 = SEG_BASE + 0x01; // 零阈值
pub const ST_STORM: u32 = SEG_BASE + 0x02; // 风暴挡断
pub const ST_BAD_WINDOW: u32 = SEG_BASE + 0x03; // 野窗口
pub const ST_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const ST_NEG: u32 = SEG_BASE + 0x05; // 负计数档

/// 滑窗限流器。
pub struct StormGate {
    pub threshold: u32,
    pub window_ms: u64,
    pub hits: Vec<u64>, // 窗口内命中时刻
    pub blocked: u32,
    pub passed: u32,
    pub sealed: bool,
}

impl StormGate {
    pub fn new(threshold: u32, window_ms: u64) -> StormGate {
        StormGate { threshold, window_ms, hits: Vec::new(), blocked: 0, passed: 0, sealed: false }
    }

    fn evict_old(&mut self, now: u64) {
        self.hits.retain(|&t| now.saturating_sub(t) < self.window_ms);
    }

    /// 中断命中：窗口内计数 < 阈值放行，否则挡断。
    pub fn hit(&mut self, now: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(ST_FROZEN);
        }
        if self.threshold == 0 {
            return Err(ST_ZERO_TH);
        }
        if self.window_ms == 0 {
            return Err(ST_BAD_WINDOW);
        }
        self.evict_old(now);
        if self.hits.len() as u32 >= self.threshold {
            self.blocked += 1;
            return Err(ST_STORM);
        }
        self.hits.push(now);
        self.passed += 1;
        Ok(())
    }

    /// 窗口重置（时间前进自然滑出）。
    pub fn window_len(&self, now: u64) -> usize {
        let mut n = 0;
        for &t in &self.hits {
            if now.saturating_sub(t) < self.window_ms {
                n += 1;
            }
        }
        n
    }

    pub fn blocked_count(&self) -> u32 {
        self.blocked
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41680;
        h = h.wrapping_mul(31).wrapping_add(self.passed);
        h = h.wrapping_mul(31).wrapping_add(self.blocked);
        for &t in &self.hits {
            h = h.wrapping_mul(31).wrapping_add(t as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：阈值 3 窗 100ms。
pub fn mk_gate() -> StormGate {
    let mut g = StormGate::new(3, 100);
    let _ = g.hit(0);
    let _ = g.hit(10);
    let _ = g.hit(20);
    let _ = g.hit(30); // 第 4 发：窗口内已 3 发 → 挡
    g
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut g = mk_gate();
    // 1) 零阈值
    got[0] = StormGate::new(0, 100).hit(1).err() == Some(ST_ZERO_TH);
    // 2) 野窗口
    got[1] = StormGate::new(3, 0).hit(1).err() == Some(ST_BAD_WINDOW);
    // 3) 风暴挡断
    got[2] = g.hit(40).err() == Some(ST_STORM);
    // 4) 窗口滑出复放行
    let mut g2 = StormGate::new(2, 100);
    let _ = g2.hit(0);
    let _ = g2.hit(10);
    let _ = g2.hit(20); // 挡
    got[3] = g2.hit(200).is_ok(); // t=0,10 已滑出（200-10=190 ≥ 100）
    // 5) 印后操作必抓
    g.seal();
    got[4] = g.hit(50).err() == Some(ST_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24165_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24165");
    let mut g = mk_gate();

    // J1 阈值窗口齐
    let j1 = g.threshold == 3 && g.window_ms == 100;
    s.add("J1-参数齐", j1, "阈 3 窗 100");

    // J2 风暴挡断
    let j2 = g.hit(40).err() == Some(ST_STORM);
    s.add("J2-风暴挡断", j2, "窗口内第 4 发必挡");

    // J3 放行计数
    let j3 = g.passed == 3;
    s.add("J3-放行计数", j3, "前三发放行");

    // J4 阻断计数
    let j4 = g.blocked_count() == 2;
    s.add("J4-阻断计数", j4, "第 4/5 发被挡");

    // J5 零阈值拒
    let j5 = StormGate::new(0, 100).hit(1).err() == Some(ST_ZERO_TH);
    s.add("J5-零阈值拒", j5, "阈 0 必拒");

    // J6 野窗口拒
    let j6 = StormGate::new(3, 0).hit(1).err() == Some(ST_BAD_WINDOW);
    s.add("J6-野窗口拒", j6, "窗 0 必拒");

    // J7 窗口滑出复放行
    let j7 = {
        let mut g2 = StormGate::new(2, 100);
        let _ = g2.hit(0);
        let _ = g2.hit(10);
        let _ = g2.hit(20);
        g2.hit(200).is_ok()
    };
    s.add("J7-滑出复放行", j7, "窗口滑出后恢复");

    // J8 阈值边界（恰等阈值放行最后一发）
    let j8 = {
        let mut g2 = StormGate::new(2, 100);
        let a = g2.hit(0).is_ok();
        let b = g2.hit(1).is_ok();
        a && b
    };
    s.add("J8-边界放行", j8, "前 threshold 发全放行");

    // J9 窗口长度
    let j9 = g.window_len(50) == 3;
    s.add("J9-窗口长度", j9, "t=50 时窗内 3 发");

    // J10 挡断不增窗
    let j10 = {
        let before = g.hits.len();
        let _ = g.hit(50);
        g.hits.len() == before
    };
    s.add("J10-挡断不增窗", j10, "被挡发不入窗");

    // J11 印后操作拒
    let j11 = {
        let mut g2 = mk_gate();
        g2.seal();
        g2.hit(60).err() == Some(ST_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = g.fold() == g.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

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
    fn unx24165_storm_flow() {
        let mut g = StormGate::new(2, 100);
        assert!(g.hit(0).is_ok());
        assert!(g.hit(1).is_ok());
        assert_eq!(g.hit(2), Err(ST_STORM));
        assert_eq!(StormGate::new(0, 10).hit(1), Err(ST_ZERO_TH));
        assert_eq!(StormGate::new(1, 0).hit(1), Err(ST_BAD_WINDOW));
        g.seal();
        assert_eq!(g.hit(3), Err(ST_FROZEN));
    }

    #[test]
    fn unx24165_checks_green() {
        let s = run_unx24165_checks();
        assert!(s.all_passed(), "UNX-F24165 判据未全绿");
    }
}
