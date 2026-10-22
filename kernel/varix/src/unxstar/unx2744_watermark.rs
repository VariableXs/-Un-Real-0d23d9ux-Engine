//! UNX-F2744 · 目标水位推进与达成判定账 · 诊断段 0xB990
//! 验收锚点：UNX-F2744-J1 推进至 high 水位达成即休眠 10/10 轮，
//! 未达成不提前休眠断言，双判据缺一即批内红灯。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB990;

// ── 诊断段独占 · 水位判定错误域 ─────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_SLEEP: u16 = SEG_ERR + 1;  // 提前休眠
const E_NOTMET: u16 = SEG_ERR + 2; // 达成不休眠
const E_LEVEL: u16 = SEG_ERR + 3;  // 水位非法
const E_PUSH: u16 = SEG_ERR + 4;   // 推进越界
const E_BAL: u16 = SEG_ERR + 5;    // 账不平

/// 水位三档。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WaterLevel {
    Low = 0,
    Mid = 1,
    High = 2,
}

impl WaterLevel {
    pub fn threshold(self) -> u64 {
        match self {
            WaterLevel::Low => 100,
            WaterLevel::Mid => 500,
            WaterLevel::High => 900,
        }
    }
}

/// 水位池：当前量 + 目标档。
pub struct WaterPool {
    pub level: u64,
    pub target: WaterLevel,
    pub slept: u32,
    pub pushed: u32,
}

impl WaterPool {
    pub fn new(target: WaterLevel) -> Self {
        WaterPool { level: 0, target, slept: 0, pushed: 0 }
    }
    /// 推进一次：增量入池（越 target+余量拒）。
    pub fn push(&mut self, delta: u64) -> bool {
        if self.level + delta > self.target.threshold() + 1000 {
            return false; // 推进越界
        }
        self.level += delta;
        self.pushed += 1;
        true
    }
    /// 达成判定。
    pub fn met(&self) -> bool {
        self.level >= self.target.threshold()
    }
    /// 达成即休眠：未达成休眠拒（提前休眠拦截核心）。
    pub fn try_sleep(&mut self) -> Result<(), u16> {
        if !self.met() {
            return Err(E_SLEEP);
        }
        self.slept += 1;
        Ok(())
    }
    /// 休眠后排水重跑（下一轮）。
    pub fn drain_reset(&mut self) {
        self.level = 0;
        self.slept = 0;
    }
}

/// 推进器：模拟逐轮推进至目标。
pub fn run_rounds(pool: &mut WaterPool, per_push: u64, max_pushes: u32) -> (bool, bool) {
    let mut reached = false;
    for _ in 0..max_pushes {
        if pool.met() {
            reached = true;
            break;
        }
        let _ = pool.push(per_push);
    }
    if pool.met() {
        reached = true;
    }
    // 达成即休眠
    let sleep = pool.try_sleep().is_ok();
    (reached, sleep)
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx2744_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F2744 目标水位推进与达成判定");

    // 1. 推进至 high 达成即休眠 10/10 轮
    let mut ok = 0;
    for r in 0..10 {
        let mut p = WaterPool::new(WaterLevel::High);
        let (reached, slept) = run_rounds(&mut p, 100, 20);
        if reached && slept && p.slept == 1 && p.level >= 900 {
            ok += 1;
        }
        let _ = r;
    }
    s.add("WM-high10/10", ok == 10, if ok == 10 { "" } else { "high 轮断" });

    // 2. 未达成不提前休眠（低量时休眠拒）
    let mut p2 = WaterPool::new(WaterLevel::High);
    let _ = p2.push(300);
    let early = p2.try_sleep() == Err(E_SLEEP) && p2.slept == 0;
    s.add("WM-提前休眠拒", early, if early { "" } else { "提前休眠放行" });

    // 3. 恰达阈值即达成（恰 900）
    let mut p3 = WaterPool::new(WaterLevel::High);
    let _ = p3.push(900);
    let exact = p3.met() && p3.try_sleep().is_ok();
    s.add("WM-恰阈达成", exact, if exact { "" } else { "恰阈误判" });

    // 4. 差 1 未达成
    let mut p4 = WaterPool::new(WaterLevel::High);
    let _ = p4.push(899);
    s.add("WM-差1未达成", !p4.met() && p4.try_sleep() == Err(E_SLEEP),
        if !p4.met() { "" } else { "差1误判达成" });

    // 5. 三档阈值单调
    s.add("WM-阈值单调", WaterLevel::Low < WaterLevel::Mid
        && WaterLevel::Mid < WaterLevel::High
        && WaterLevel::Low.threshold() < WaterLevel::Mid.threshold()
        && WaterLevel::Mid.threshold() < WaterLevel::High.threshold(),
        if WaterLevel::Low < WaterLevel::High { "" } else { "阈值序错" });

    // 6. mid 档达标即休眠（不要求 high）
    let mut p5 = WaterPool::new(WaterLevel::Mid);
    let _ = p5.push(500);
    s.add("WM-mid达成", p5.met() && p5.try_sleep().is_ok(),
        if p5.met() { "" } else { "mid 误判" });

    // 7. 推进越界拒
    let mut p6 = WaterPool::new(WaterLevel::High);
    let oob = !p6.push(u64::MAX / 2) && !p6.push(5_000_000);
    s.add("WM-推进越界拒", oob, if oob { "" } else { "越界放行" });

    // 8. 排水重跑（下一轮零基）
    let mut p7 = WaterPool::new(WaterLevel::Low);
    let _ = p7.push(150);
    let _ = p7.try_sleep();
    p7.drain_reset();
    s.add("WM-排水重跑", p7.level == 0 && !p7.met() && p7.slept == 0,
        if p7.level == 0 { "" } else { "排水残留" });

    // 9. 推进计数对账
    let mut p8 = WaterPool::new(WaterLevel::High);
    for _ in 0..9 {
        let _ = p8.push(100);
    }
    s.add("WM-推进计数", p8.pushed == 9 && p8.level == 900,
        if p8.pushed == 9 { "" } else { "计数漂移" });

    // 10. 达成后再推不计（met 后 push 仍允许但休眠只记一次）
    let mut p9 = WaterPool::new(WaterLevel::Low);
    let _ = p9.push(120);
    let _ = p9.push(50);
    let s1 = p9.try_sleep().is_ok();
    let s2 = p9.try_sleep().is_ok();
    s.add("WM-重复休眠计次", s1 && s2 && p9.slept == 2,
        if p9.slept == 2 { "" } else { "休眠计次错" });

    // 11. 推进器未达成场景（每推 1、上限 10 推 → 未达 high 不休眠）
    let mut pa = WaterPool::new(WaterLevel::High);
    let (reached, slept) = run_rounds(&mut pa, 1, 10);
    s.add("WM-未达不休眠", !reached && !slept && pa.slept == 0,
        if !reached { "" } else { "未达误判" });

    // 12. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB990 && SEG_BASE < 0xB9B0;
    s.add("WM-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
