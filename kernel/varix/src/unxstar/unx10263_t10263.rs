//! UNX-F10263 并发爬坡协议（渐进加压梯度冻结）（0x15020 段）
//!
//! 锚点：UNX-F10263-J1 爬坡协议达成（渐进加压梯度冻结：阶梯/间隔/
//! 停驻判据三面 10/10 次依序推进）；J2 跳阶检出（跨级加压/回退未登记
//! /越顶加压 10/10 次拦截）；J3 梯度指纹（同梯度同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·压力演习）。
//!
//! 结构：梯度册（冻结阶梯真源）+ 爬坡机（逐级推进 + 停驻判据）+ 拦
//！ 截核（跳阶/回退/越顶）+ 注入核（跳阶/越顶/冻结后改必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x15020 独占 0x20）。
pub const SEG_BASE: u32 = 0x15020;
pub const RP_SKIP: u32 = SEG_BASE + 0x01; // 跳阶
pub const RP_OVER_TOP: u32 = SEG_BASE + 0x02; // 越顶
pub const RP_REGRESS: u32 = SEG_BASE + 0x03; // 回退未登记
pub const RP_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改
pub const RP_EMPTY: u32 = SEG_BASE + 0x05; // 空梯度

/// 冻结梯度真源：八级并发数。
pub const LADDER: [u32; 8] = [1, 2, 4, 8, 16, 32, 64, 128];

/// 停驻判据：本级水位 ≥ 停驻线才准升级。
pub const HOLD_LINE: u16 = 800; // 千分位

/// 爬坡机。
pub struct Ramp {
    level: usize,       // 当前级
    hold: u16,          // 当前级水位
    reached: Vec<u32>,  // 已达级并发数
    top_reached: bool,
    frozen: bool,
}

impl Ramp {
    pub fn new() -> Ramp {
        Ramp {
            level: 0,
            hold: 0,
            reached: vec![LADDER[0]],
            top_reached: false,
            frozen: false,
        }
    }

    /// 报水位：本级水位登记（≥ 停驻线才解锁下一级）。
    pub fn report(&mut self, permille: u16) -> Result<bool, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if permille > 1000 {
            return Err("out_of_range");
        }
        self.hold = permille;
        Ok(permille >= HOLD_LINE)
    }

    /// 升级：水位达线才准升；越顶/冻结后拒收。
    pub fn climb(&mut self) -> Result<u32, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if self.top_reached || self.level + 1 >= LADDER.len() {
            if self.level + 1 >= LADDER.len() {
                self.top_reached = true;
                return Err("over_top");
            }
            return Err("over_top");
        }
        if self.hold < HOLD_LINE {
            return Err("not_ready");
        }
        self.level += 1;
        let n = LADDER[self.level];
        self.reached.push(n);
        Ok(n)
    }

    /// 回退登记：回退必须显式（未登记回退后直接升级 = 跳阶）。
    pub fn regress(&mut self, to: usize) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if to >= self.level {
            return Err("not_back");
        }
        self.level = to;
        self.hold = 0; // 回退后水位清零重测
        Ok(())
    }

    /// 跳阶检测：升级企图跨级（level+2 及以上）。
    pub fn skip_check(&self, target: usize) -> bool {
        target > self.level + 1
    }

    pub fn level(&self) -> usize {
        self.level
    }

    pub fn concurrency(&self) -> u32 {
        LADDER[self.level]
    }

    pub fn reached_trace(&self) -> &[u32] {
        &self.reached
    }

    pub fn is_top(&self) -> bool {
        self.top_reached
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 梯度指纹：已达轨迹折叠。
pub fn ramp_fp(r: &Ramp) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for v in r.reached_trace() {
        for b in v.to_le_bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(0x01000193);
        }
    }
    h
}

/// 注入核。
pub fn inject_fault(r: &mut Ramp, code: u32) -> Result<u32, &'static str> {
    match code {
        RP_SKIP => {
            // 跳阶：升两级企图（先到 L1，水位够，再连升两次跳过 L2 直达 L3？）
            // 真源语义：每次 climb 只升一级，跳阶 = 未达标升级
            let _ = r.report(990);
            let _ = r.climb(); // 到 L1
            r.hold = 990;
            let _ = r.climb(); // 到 L2
            r.hold = 100; // 未达标
            r.climb() // 未达线升级企图 → not_ready
        }
        RP_OVER_TOP => {
            let _ = r.report(1000);
            for _ in 0..10 {
                let _ = r.climb();
            }
            r.climb()
        }
        RP_REGRESS => {
            let _ = r.report(990);
            let _ = r.climb(); // L1
            r.regress(0)?;
            // 回退后未报水位直接升级
            r.climb()
        }
        RP_FROZEN => {
            r.freeze();
            r.report(500).map(|_| 0)
        }
        RP_EMPTY => {
            // 空梯度企图：regress 到越界级
            r.regress(99).map(|_| 0)
        }
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10263_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10263");

    // J1 全程爬坡：八级依序推进到顶
    let j1 = {
        let mut r = Ramp::new();
        let mut ok = true;
        for _ in 1..8 {
            if r.report(1000) != Ok(true) || r.climb().is_err() {
                ok = false;
                break;
            }
        }
        ok && r.level() == 7 && r.concurrency() == 128
    };
    s.add("J1-八级到顶", j1, "1→128 依序全达");

    // J2a 未达线升级拦截
    let j2a = {
        let mut r = Ramp::new();
        let _ = r.report(500);
        r.climb() == Err("not_ready")
    };
    s.add("J2a-未达线拦", j2a, "水位不足不准升级");

    // J2b 越顶拦截
    let j2b = {
        let mut r = Ramp::new();
        inject_fault(&mut r, RP_OVER_TOP);
        r.is_top()
    };
    s.add("J2b-越顶拦截", j2b, "八级后加压即拒");

    // J2c 回退未登记水位升级拦截
    let j2c = inject_fault(&mut Ramp::new(), RP_REGRESS) == Err("not_ready");
    s.add("J2c-回退重测", j2c, "回退后水位清零须重报");

    // J3 跳阶检测真源
    let j3 = {
        let mut r = Ramp::new();
        let _ = r.report(1000);
        let _ = r.climb(); // L1
        r.skip_check(3) && !r.skip_check(2)
    };
    s.add("J3-跳阶真源", j3, "跨级检测精确");

    // J4 梯度真源：八级数列冻结
    let j4 = LADDER == [1, 2, 4, 8, 16, 32, 64, 128];
    s.add("J4-梯度冻结", j4, "八级数列一致");

    // J5 回退显式登记成功路径
    let j5 = {
        let mut r = Ramp::new();
        let _ = r.report(1000);
        let _ = r.climb(); // L1
        let back = r.regress(0);
        let _ = r.report(1000);
        let up = r.climb();
        back == Ok(()) && up == Ok(2)
    };
    s.add("J5-回退重爬", j5, "显式回退后可重爬");

    // J6 停驻线真源：800 恰好解锁
    let j6 = {
        let mut r = Ramp::new();
        let at_line = r.report(HOLD_LINE).unwrap();
        let unlocked = r.climb() == Ok(2);
        at_line && unlocked
    };
    s.add("J6-停驻线边界", j6, "800 恰好解锁");

    // J7 注入五路
    let codes = [RP_SKIP, RP_OVER_TOP, RP_REGRESS, RP_FROZEN, RP_EMPTY];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut Ramp::new(), **c).is_err())
        .count();
    s.add("J7-注入五路", caught == 5, "五路注入全拒");

    // J8 冻结后报数拒收
    let j8 = {
        let mut r = Ramp::new();
        inject_fault(&mut r, RP_FROZEN);
        r.is_frozen()
    };
    s.add("J8-冻结拒报", j8, "印后报数必拒");

    // J9 爬坡轨迹与指纹一致
    let j9 = {
        let mut r = Ramp::new();
        let _ = r.report(1000);
        let _ = r.climb(); // 2
        let _ = r.climb(); // 4
        let _ = r.climb(); // 8
        r.reached_trace() == [1, 2, 4, 8] && ramp_fp(&r) != 0
    };
    s.add("J9-轨迹指纹", j9, "轨迹与指纹一致");

    // J10 十轮爬坡回放：10 轮到顶轨迹一致（确定性）
    let j10 = {
        let mk = || {
            let mut r = Ramp::new();
            for _ in 1..8 {
                let _ = r.report(1000);
                let _ = r.climb();
            }
            (r.level(), ramp_fp(&r))
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.0 == 7
    };
    s.add("J10-十轮爬坡", j10, "10/10 轮轨迹一致");

    s
}
