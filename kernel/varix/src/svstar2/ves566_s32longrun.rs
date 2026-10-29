//! VE-F4332 · 一致性长稳（U07 性能与预算组）
//! 职责：一致性系统长时间稳定性——长稳三景（72 小时连续扫描 / 万次判定
//! 循环 / 反复降级回升）+ 观测三探针（内存曲线 / 判定错误率 / 队列深度）
//! + 退出判据（三景全过 + 曲线平稳）+ 曲线爬升泄漏联动 + 错误率抬升即刻
//! 中止归因 + 队列淤积限流重排。
//! 段位：0x3620（续表 idx566）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3620;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LsCode {
    /// 错误率抬升（即刻中止归因）。
    ErrorRateUp = SEG_BASE,
    /// 曲线爬升（泄漏联动立案）。
    CurveClimbing = SEG_BASE + 0x01,
    /// 队列淤积（限流重排）。
    QueueSilted = SEG_BASE + 0x02,
    /// 退出判据未达（继续跑）。
    NotYet = SEG_BASE + 0x03,
    /// 趋势上行（滑动均值斜率持续为正）。
    SlopeUpward = SEG_BASE + 0x04,
}

/// 三景。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Scene {
    /// 72 小时连续扫描。
    #[default]
    Scan72h,
    /// 万次判定循环。
    Loop10k,
    /// 反复降级回升。
    DegCycle,
}

/// 中止事件：第几号采样 + 抬升幅（bp）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AbortEvent {
    pub at_sample: u32,
    pub overshoot_bp: u32,
}

/// 长稳台。
#[derive(Default)]
pub struct LongStab {
    /// 三景通过位。
    pub scenes: [(Scene, bool); 3],
    /// 内存曲线采样（KB）。
    pub mem_curve: Vec<u32>,
    /// 判定错误率采样（bp）。
    pub err_rates: Vec<u32>,
    /// 队列深度采样。
    pub queue_depth: Vec<u32>,
    pub aborts: u32,
    pub requeues: u32,
    /// 深化：中止事件账（时间序，只增不删）。
    pub aborts_ledger: Vec<AbortEvent>,
    /// 深化：限流档（0 正常 / 1 减速 / 2 半速 / 3 冻结新入队）。
    pub throttle_level: u8,
    /// 深化：连续淤积连击。
    pub silt_run: u32,
}

/// 错误率抬升阈（bp）。
pub const ERR_ABORT_BP: u32 = 100;

impl LongStab {
    pub fn new() -> Self {
        LongStab {
            scenes: [(Scene::Scan72h, false), (Scene::Loop10k, false), (Scene::DegCycle, false)],
            ..Default::default()
        }
    }

    /// 内存曲线采样。
    pub fn sample_mem(&mut self, kb: u32) {
        self.mem_curve.push(kb);
    }

    /// 错误率采样（即刻中止核验：最新样本 > 阈即中止）。
    pub fn sample_err(&mut self, bp: u32) -> Result<(), LsCode> {
        self.err_rates.push(bp);
        if bp > ERR_ABORT_BP {
            self.aborts += 1;
            return Err(LsCode::ErrorRateUp);
        }
        Ok(())
    }

    /// 队列深度采样（淤积核验：深度 > 1000 限流重排）。
    pub fn sample_queue(&mut self, depth: u32) -> Result<(), LsCode> {
        self.queue_depth.push(depth);
        if depth > 1000 {
            self.requeues += 1;
            return Err(LsCode::QueueSilted);
        }
        Ok(())
    }

    /// 曲线平稳核验：末段窗口内最大波动（max−min）不超预算（KB）。
    pub fn curve_stable(&self, window: usize, budget_kb: u32) -> Result<bool, LsCode> {
        if self.mem_curve.len() < window {
            return Err(LsCode::NotYet);
        }
        let tail = &self.mem_curve[self.mem_curve.len() - window..];
        let mx = tail.iter().max().copied().unwrap_or(0);
        let mn = tail.iter().min().copied().unwrap_or(0);
        Ok(mx - mn <= budget_kb)
    }

    /// 曲线爬升检出：整体单调递增且末值超初值 50%（泄漏联动立案信号）。
    pub fn climbing(&self) -> Result<bool, LsCode> {
        if self.mem_curve.len() < 4 {
            return Err(LsCode::NotYet);
        }
        let mono = self.mem_curve.windows(2).all(|w| w[1] >= w[0]);
        let first = self.mem_curve[0] as u64;
        let last = *self.mem_curve.last().unwrap_or(&0) as u64;
        Ok(mono && last * 2 > first * 3)
    }

    /// 景通过登记。
    pub fn pass(&mut self, s: Scene) {
        for e in self.scenes.iter_mut() {
            if e.0 == s {
                e.1 = true;
            }
        }
    }

    /// 退出判据：三景全过 + 曲线平稳。
    pub fn can_exit(&self, window: usize, budget_kb: u32) -> Result<bool, LsCode> {
        let all_pass = self.scenes.iter().all(|s| s.1);
        if !all_pass {
            return Err(LsCode::NotYet);
        }
        self.curve_stable(window, budget_kb)
    }

    // ─── 深化块：中止事件账（结构化 + 守恒）───

    /// 错误率采样入账版：正常采样与中止都留痕，事件账可对账。
    pub fn sample_err_ledger(&mut self, bp: u32) -> Result<(), LsCode> {
        let idx = self.err_rates.len() as u32 + 1;
        let r = self.sample_err(bp);
        if let Err(LsCode::ErrorRateUp) = r {
            self.aborts_ledger.push(AbortEvent {
                at_sample: idx,
                overshoot_bp: bp - ERR_ABORT_BP,
            });
        }
        r
    }

    /// 事件账守恒：账面条数 ≡ aborts 计数（篡改即检出）。
    pub fn aborts_conserves(&self) -> bool {
        self.aborts_ledger.len() as u32 == self.aborts
    }

    /// 超抬幅峰值（无事件恒零）。
    pub fn peak_overshoot(&self) -> u32 {
        self.aborts_ledger.iter().map(|e| e.overshoot_bp).max().unwrap_or(0)
    }

    // ─── 深化块：滑动均值剖面 + 趋势斜率 ───

    /// 整数滑动均值（奇数窗口，端部收缩）。
    pub fn rolling_mean(&self, w: usize) -> Vec<u32> {
        if w == 0 || self.mem_curve.is_empty() {
            return Vec::new();
        }
        let n = self.mem_curve.len();
        let half = w / 2;
        let mut out = Vec::new();
        for i in 0..n {
            let lo = i.saturating_sub(half);
            let hi = (i + half + 1).min(n);
            let seg = &self.mem_curve[lo..hi];
            let sum: u64 = seg.iter().map(|v| *v as u64).sum();
            out.push((sum / seg.len() as u64) as u32);
        }
        out
    }

    /// 趋势斜率符号：剖面首尾对比（+1 上行 / -1 下行 / 0 平）。
    /// 平 = 差值在容差内。上行且连续 w 段同向即 SlopeUpward。
    pub fn slope_sign(&self, w: usize, tol: u32) -> i8 {
        let prof = self.rolling_mean(w);
        if prof.len() < 4 {
            return 0;
        }
        let first = prof[0] as i64;
        let last = prof[prof.len() - 1] as i64;
        let d = last - first;
        if d > tol as i64 {
            1
        } else if d < -(tol as i64) {
            -1
        } else {
            0
        }
    }

    /// 缓涨检出：滚动剖面三段（首中尾）逐段上行——比 climbing() 更灵敏
    /// 的泄漏前兆（爬升未到 50% 即预警）。
    pub fn creeping(&self, w: usize, tol: u32) -> Result<bool, LsCode> {
        let prof = self.rolling_mean(w);
        if prof.len() < 6 {
            return Err(LsCode::NotYet);
        }
        let third = prof.len() / 3;
        let a = prof[0] as i64;
        let b = prof[third] as i64;
        let c = prof[third * 2] as i64;
        let d = prof[prof.len() - 1] as i64;
        Ok(b - a > tol as i64 && c - b > tol as i64 && d - c > tol as i64)
    }

    // ─── 深化块：限流升级器（连续淤积 N 次升级）───

    /// 队列深度采样带限流升级：连续 3 次淤积升一档（封顶 3 档）。
    pub fn sample_queue_throttled(&mut self, depth: u32) -> Result<(), LsCode> {
        let r = self.sample_queue(depth);
        match r {
            Err(LsCode::QueueSilted) => {
                self.silt_run += 1;
                if self.silt_run >= 3 && self.throttle_level < 3 {
                    self.throttle_level += 1;
                    self.silt_run = 0;
                }
                Err(LsCode::QueueSilted)
            }
            other => {
                self.silt_run = 0;
                other
            }
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves566_checks() -> CheckSet {
    let mut set = CheckSet::new("ves566");
    let mut ls = LongStab::new();

    // C566-01 错误率低位正常采样。
    chk(&mut set, "C566-01", ls.sample_err(10).is_ok() && ls.sample_err(20).is_ok(), "错误率低位正常");

    // C566-02 错误率抬升即刻中止（计数）。
    chk(&mut set, "C566-02", ls.sample_err(200) == Err(LsCode::ErrorRateUp) && ls.aborts == 1, "抬升即刻中止");

    // C566-03 阈值边界（恰 100 不中止）。
    chk(&mut set, "C566-03", ls.sample_err(ERR_ABORT_BP).is_ok(), "阈值边界不误杀");

    // C566-04 队列深度正常。
    chk(&mut set, "C566-04", ls.sample_queue(500).is_ok(), "队列深度正常");

    // C566-05 队列淤积限流重排。
    chk(&mut set, "C566-05", ls.sample_queue(1500) == Err(LsCode::QueueSilted) && ls.requeues == 1, "淤积限流重排");

    // C566-06 曲线平稳核验（样本不足不判）。
    chk(&mut set, "C566-06", ls.curve_stable(8, 50) == Err(LsCode::NotYet), "样本不足不判");

    // C566-07 平稳曲线通过（波动 20KB < 预算 50）。
    for v in [1000, 1010, 1005, 1015, 1000, 1010, 1008, 1012] {
        ls.sample_mem(v);
    }
    chk(&mut set, "C566-07", ls.curve_stable(8, 50) == Ok(true), "平稳曲线通过");

    // C566-08 波动超预算拒绝。
    chk(&mut set, "C566-08", ls.curve_stable(8, 5) == Ok(false), "波动超预算拒");

    // C566-09 爬升检出（单调 +50% 超）。
    let mut ls2 = LongStab::new();
    for v in [100, 130, 160, 200] {
        ls2.sample_mem(v);
    }
    chk(&mut set, "C566-09", ls2.climbing() == Ok(true), "爬升检出");

    // C566-10 稳定曲线不误报爬升。
    chk(&mut set, "C566-10", ls.climbing() == Ok(false), "稳定不误报");

    // C566-11 三景未全过退出判据拒绝。
    ls.pass(Scene::Scan72h);
    ls.pass(Scene::Loop10k);
    chk(&mut set, "C566-11", ls.can_exit(8, 50) == Err(LsCode::NotYet), "缺景不放行");

    // C566-12 三景全过 + 曲线平稳 = 可退出。
    ls.pass(Scene::DegCycle);
    chk(&mut set, "C566-12", ls.can_exit(8, 50) == Ok(true), "三景全过退出");

    // ── 深化判据：中止事件账（守恒 + 峰值超抬幅）──
    let mut la = LongStab::new();
    let _ = la.sample_err_ledger(10);
    let _ = la.sample_err_ledger(20);
    let _ = la.sample_err_ledger(150); // 超抬 50
    let _ = la.sample_err_ledger(300); // 超抬 200
    chk(&mut set, "C566-13", la.aborts_conserves() && la.aborts == 2
        && la.aborts_ledger.len() == 2, "事件账与计数守恒");
    chk(&mut set, "C566-14", la.peak_overshoot() == 200
        && la.aborts_ledger[0].at_sample == 3, "峰值超抬幅与首事件序号");
    // 篡改检出：计数与账面不一致即破。
    let mut lat = LongStab::new();
    let _ = lat.sample_err_ledger(150);
    lat.aborts = 9;
    chk(&mut set, "C566-15", !lat.aborts_conserves(), "计数篡改守恒即破");

    // ── 深化判据：滑动均值剖面 + 趋势斜率 ──
    let mut lp = LongStab::new();
    for v in [1000, 1002, 1004, 1006, 1008, 1010] {
        lp.sample_mem(v);
    }
    let prof = lp.rolling_mean(3);
    chk(&mut set, "C566-16", prof.len() == 6 && prof[0] == 1001
        && prof[3] == 1006, "滑动均值端部收缩正确");
    chk(&mut set, "C566-17", lp.slope_sign(3, 5) == 1, "缓升剖面斜率 +1");
    chk(&mut set, "C566-18", ls.slope_sign(3, 50) == 0, "平稳剖面斜率 0");
    // 缓涨检出（未到 climbing 的 50% 线即预警）。
    chk(&mut set, "C566-19", lp.creeping(3, 1) == Ok(true)
        && lp.climbing() == Ok(false), "缓涨预警先于爬升立案");

    // ── 深化判据：限流升级器（连击升档 + 干净采样复位连击）──
    let mut lt = LongStab::new();
    for _ in 0..3 {
        let _ = lt.sample_queue_throttled(2000);
    }
    chk(&mut set, "C566-20", lt.throttle_level == 1 && lt.silt_run == 0, "连击 3 升一档");
    let _ = lt.sample_queue_throttled(100); // 干净采样复位连击
    let _ = lt.sample_queue_throttled(1500);
    let _ = lt.sample_queue_throttled(1500);
    chk(&mut set, "C566-21", lt.throttle_level == 1 && lt.silt_run == 2,
        "干净采样打断连击不升档");
    for _ in 0..6 {
        let _ = lt.sample_queue_throttled(2000);
    }
    chk(&mut set, "C566-22", lt.throttle_level == 3, "累计升到封顶 3 档");

    // ── 深化判据：万轮长稳仿真（LCG，错误率低位 + 偶发尖刺全抓）──
    let mut rng: u64 = 0x5660_10A0;
    let mut sim = LongStab::new();
    let mut caught = 0u32;
    let mut rounds = 0u32;
    for i in 0..10000u64 {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let spike = i % 2500 == 0;
        let bp = if spike { 200 + (rng % 300) as u32 } else { (rng % 90) as u32 };
        rounds += 1;
        if sim.sample_err(bp).is_err() {
            caught += 1;
        }
    }
    chk(&mut set, "C566-23", caught == 4 && sim.aborts_conserves(),
        "万轮仿真尖刺 4/4 全抓");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abort_on_error_rate() {
        let mut ls = LongStab::new();
        assert!(ls.sample_err(50).is_ok());
        assert_eq!(ls.sample_err(101), Err(LsCode::ErrorRateUp));
        assert_eq!(ls.aborts, 1);
    }

    #[test]
    fn queue_limit() {
        let mut ls = LongStab::new();
        assert!(ls.sample_queue(1000).is_ok()); // 边界不误杀
        assert_eq!(ls.sample_queue(1001), Err(LsCode::QueueSilted));
        assert_eq!(ls.requeues, 1);
    }

    #[test]
    fn exit_gate() {
        let mut ls = LongStab::new();
        for v in [100; 6] {
            ls.sample_mem(v);
        }
        for s in [Scene::Scan72h, Scene::Loop10k, Scene::DegCycle] {
            ls.pass(s);
        }
        assert_eq!(ls.can_exit(6, 10), Ok(true));
        assert_eq!(ls.can_exit(100, 10), Err(LsCode::NotYet)); // 窗口不足
    }

    #[test]
    fn abort_ledger_conserves() {
        let mut ls = LongStab::new();
        let _ = ls.sample_err_ledger(50);
        let _ = ls.sample_err_ledger(400);
        assert!(ls.aborts_conserves());
        assert_eq!(ls.peak_overshoot(), 300);
        assert_eq!(ls.aborts_ledger[0].at_sample, 1);
    }

    #[test]
    fn throttle_escalation() {
        let mut ls = LongStab::new();
        for _ in 0..6 {
            let _ = ls.sample_queue_throttled(2000);
        }
        assert_eq!(ls.throttle_level, 2);
        let _ = ls.sample_queue_throttled(10);
        assert_eq!(ls.silt_run, 0);
    }

    #[test]
    fn creep_early_warning() {
        let mut ls = LongStab::new();
        for v in [100, 102, 104, 106, 108, 110, 112, 114, 116] {
            ls.sample_mem(v);
        }
        assert_eq!(ls.creeping(3, 1), Ok(true));
        assert_eq!(ls.climbing(), Ok(false)); // 未到 50%
    }
}
