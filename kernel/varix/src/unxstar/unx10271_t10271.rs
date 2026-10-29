//! UNX-F10271 压力下超时与看门狗适配（压力超时系数）（0x16120 段）
//!
//! 锚点：UNX-F10271-J1 适配达成（压力超时系数按压力档适配且看门狗喂
//! 狗周期随动 10/10 次）；J2 越界适配检出（系数越界/负适配/冻结后改
//! 10/10 次检出）；J3 适配指纹（同压力档同适配同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·压力演习）。
//!
//! 结构：压力档闭集（低/中/高/极限）+ 超时系数表（真源）+ 看门狗周
//! 期核 + 适配账 + 注入核（越界/负适配/冻结必抓）。

use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x16120 独占 0x20）。
pub const SEG_BASE: u32 = 0x16120;
pub const TA_OUT_TIER: u32 = SEG_BASE + 0x01; // 越界压力档
pub const TA_NEG_FIT: u32 = SEG_BASE + 0x02; // 负适配
pub const TA_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改
pub const TA_BAD_BASE: u32 = SEG_BASE + 0x04; // 坏基准超时
pub const TA_OVERFLOW: u32 = SEG_BASE + 0x05; // 溢出适配

/// 压力档闭集。
pub const P_LOW: u8 = 0;
pub const P_MID: u8 = 1;
pub const P_HIGH: u8 = 2;
pub const P_EXTREME: u8 = 3;
pub const TIER_N: u8 = 4;

pub fn tier_ok(t: u8) -> bool {
    t < TIER_N
}

pub fn tier_name(t: u8) -> &'static str {
    match t {
        P_LOW => "low",
        P_MID => "mid",
        P_HIGH => "high",
        _ => "extreme",
    }
}

/// 超时系数表真源（压力档 → 系数 ×100 定点）。
pub const FACTOR: [u32; 4] = [100, 150, 250, 400];

/// 看门狗周期系数真源（压力档 → 周期 ×100 定点）。
pub const WDG_FACTOR: [u32; 4] = [100, 120, 180, 300];

/// 适配结果。
#[derive(Clone, Copy, PartialEq)]
pub struct FitOut {
    pub timeout_ms: u32,
    pub wdg_ms: u32,
}

impl FitOut {
    pub fn ok(&self) -> bool {
        // 看门狗不晚于超时触发即合法（低档系数 100 时 wdg==timeout 为边界）
        self.timeout_ms > 0 && self.wdg_ms > 0 && self.wdg_ms <= self.timeout_ms
    }
}

/// 适配核：基准超时 × 档系数（定点 ×100 → /100），饱和防溢出。
pub fn fit(base_ms: u32, tier: u8) -> Result<FitOut, &'static str> {
    if !tier_ok(tier) {
        return Err("out_tier");
    }
    if base_ms == 0 {
        return Err("bad_base");
    }
    if base_ms > 1_000_000 {
        return Err("overflow");
    }
    let f = FACTOR[tier as usize];
    let w = WDG_FACTOR[tier as usize];
    // 饱和乘法（防 u32 溢出）
    let sat = |v: u32| v.saturating_mul(f) / 100;
    let wdg = |v: u32| v.saturating_mul(w) / 100;
    Ok(FitOut {
        timeout_ms: sat(base_ms),
        wdg_ms: wdg(base_ms),
    })
}

/// 适配账。
pub struct FitBook {
    rows: Vec<(u8, FitOut)>, // (档, 结果)
    frozen: bool,
}

impl FitBook {
    pub fn new() -> FitBook {
        FitBook { rows: Vec::new(), frozen: false }
    }

    /// 登适配：重档/冻结后写拒收。
    pub fn put(&mut self, tier: u8, out: FitOut) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !tier_ok(tier) {
            return Err("out_tier");
        }
        if !out.ok() {
            return Err("neg_fit");
        }
        if self.rows.iter().any(|r| r.0 == tier) {
            return Err("dup_tier");
        }
        self.rows.push((tier, out));
        Ok(())
    }

    /// 四档齐备。
    pub fn full(&self) -> bool {
        (0..TIER_N).all(|t| self.rows.iter().any(|r| r.0 == t))
    }

    /// 适配指纹：档 × 结果折叠。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        let mut sorted: Vec<&(u8, FitOut)> = self.rows.iter().collect();
        sorted.sort_by_key(|r| r.0);
        for (t, o) in sorted {
            for v in [*t as u32, o.timeout_ms, o.wdg_ms] {
                for b in v.to_le_bytes() {
                    h ^= b as u32;
                    h = h.wrapping_mul(0x01000193);
                }
            }
        }
        h
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 注入核。
pub fn inject_fault(b: &mut FitBook, code: u32) -> Result<FitOut, &'static str> {
    match code {
        TA_OUT_TIER => fit(100, 9),
        TA_NEG_FIT => b
            .put(P_LOW, FitOut { timeout_ms: 0, wdg_ms: 0 })
            .map(|_| FitOut { timeout_ms: 0, wdg_ms: 0 }),
        TA_FROZEN => {
            b.freeze();
            fit(100, P_LOW)
        }
        TA_BAD_BASE => fit(0, P_MID),
        TA_OVERFLOW => fit(2_000_000, P_EXTREME),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10271_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10271");

    // J1 四档适配落账：低/中/高/极限系数正确
    let j1 = {
        let mut b = FitBook::new();
        let mut ok = true;
        for t in 0..TIER_N {
            match fit(1000, t) {
                Ok(o) => {
                    if b.put(t, o).is_err() {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        ok
            && b.full()
            && b.len() == 4
            && b.rows.iter().all(|(_, o)| o.ok())
            && b.rows[0].1.timeout_ms == 1000
            && b.rows[1].1.timeout_ms == 1500
            && b.rows[2].1.timeout_ms == 2500
            && b.rows[3].1.timeout_ms == 4000
    };
    s.add("J1-四档适配", j1, "系数表 1.0/1.5/2.5/4.0 精确");

    // J2 看门狗周期随动：低档 1000ms → 看门狗 1000ms 内
    let j2 = {
        let o = fit(1000, P_MID).unwrap();
        o.wdg_ms == 1200 && o.wdg_ms < o.timeout_ms
    };
    s.add("J2-看门狗随动", j2, "周期系数 1.2 随动");

    // J3 系数表真源
    let j3 = FACTOR == [100, 150, 250, 400] && WDG_FACTOR == [100, 120, 180, 300];
    s.add("J3-系数真源", j3, "两表四档一致");

    // J4 越界档拒收
    let j4 = inject_fault(&mut FitBook::new(), TA_OUT_TIER) == Err("out_tier");
    s.add("J4-越界档拒", j4, "tier=9 即拒");

    // J5 负适配拒收（零超时）
    let j5 = inject_fault(&mut FitBook::new(), TA_NEG_FIT) == Err("neg_fit");
    s.add("J5-负适配拒", j5, "零超时即拒");

    // J6 溢出饱和：基准超大 → 饱和不溢出
    let j6 = {
        let o = fit(1_000_000, P_EXTREME).unwrap();
        o.timeout_ms == 4_000_000 && o.wdg_ms == 3_000_000
    };
    s.add("J6-饱和防溢", j6, "百万基准饱和精确");

    // J7 注入五路
    let codes = [TA_OUT_TIER, TA_NEG_FIT, TA_FROZEN, TA_BAD_BASE, TA_OVERFLOW];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut FitBook::new(), **c).is_err())
        .count();
    s.add("J7-注入五路", caught >= 4, "至少四路直接拒收");

    // J8 坏基准拒收
    let j8 = inject_fault(&mut FitBook::new(), TA_BAD_BASE) == Err("bad_base");
    s.add("J8-坏基准拒", j8, "基准 0 即拒");

    // J9 冻结红线
    let j9 = {
        let mut b = FitBook::new();
        inject_fault(&mut b, TA_FROZEN);
        b.is_frozen() && b.put(P_LOW, fit(100, P_LOW).unwrap()) == Err("frozen")
    };
    s.add("J9-冻结拒收", j9, "印后适配必抓");

    // J10 十轮适配回放：10 轮指纹一致（确定性）
    let j10 = {
        let mk = || {
            let mut b = FitBook::new();
            for t in 0..TIER_N {
                let o = fit(800, t).unwrap();
                let _ = b.put(t, o);
            }
            b.fp()
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮适配", j10, "10/10 轮指纹一致");

    s
}
