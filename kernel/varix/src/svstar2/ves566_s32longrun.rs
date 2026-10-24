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
}
