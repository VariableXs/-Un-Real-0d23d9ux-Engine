//! VE-F4311 · 场景应用性能（U06 场景应用组）
//! 职责：场景触达性能工程——性能目标三件（场景提示 P95<100ms / 预检不
//! 阻塞主流程 / 探针开销<1%）+ 降级（高负载场景提示降为异步摘要——超时
//! 静默入摘要不阻塞）+ 场景性能隔离舱（单场景劣化熔断不扩散）+ 摘要积压
//! 批量合并 + 异步摘要通知读屏可达（域本色）。
//! 段位：0x3180（续表 idx545）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3180;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpCode {
    /// 提示 P95 超阈（100ms）。
    HintP95Over = SEG_BASE,
    /// 探针开销超阈（1%）。
    ProbeOver = SEG_BASE + 0x01,
    /// 熔断（隔离舱单场景劣化即断）。
    Tripped = SEG_BASE + 0x02,
    /// 摘要积压（批量合并触发）。
    DigestBacklog = SEG_BASE + 0x03,
    /// 负载信号越界。
    LoadRange = SEG_BASE + 0x04,
}

/// 性能目标卡（三件阈值单源）。
pub const P95_BUDGET_MS: u32 = 100;
pub const PROBE_BUDGET_BP: u32 = 100; // 1% = 100bp
/// 摘要队列积压触发线。
pub const DIGEST_TRIGGER: usize = 8;

/// 一条异步摘要（读屏可达 = 附文本替代 + 优先级）。
#[derive(Clone, Debug, PartialEq)]
pub struct Digest {
    pub scene: u8,
    pub text_alt: &'static str,
    pub urgent: bool,
}

/// 隔离舱状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BayState {
    Open,
    Tripped(u64), // 熔断时刻
}

/// 场景性能台。
#[derive(Default)]
pub struct ScenePerf {
    /// 各场景最近 P95（ms）。
    pub p95: Vec<(u8, u32)>,
    /// 探针开销 bp。
    pub probe_bp: u32,
    /// 隔离舱。
    pub bays: Vec<(u8, BayState)>,
    /// 异步摘要队列。
    pub digest: Vec<Digest>,
    /// 合并批次数。
    pub merged_batches: u32,
    now: u64,
}

impl ScenePerf {
    pub fn new(now: u64) -> Self {
        ScenePerf { now, ..Default::default() }
    }

    /// 上报 P95 样本。
    pub fn report_p95(&mut self, scene: u8, ms: u32) {
        if let Some(e) = self.p95.iter_mut().find(|e| e.0 == scene) {
            e.1 = ms;
        } else {
            self.p95.push((scene, ms));
        }
    }

    /// P95 目标核验（三件之一）。
    pub fn p95_ok(&self, scene: u8) -> Result<bool, SpCode> {
        let e = self.p95.iter().find(|e| e.0 == scene).ok_or(SpCode::LoadRange)?;
        Ok(e.1 <= P95_BUDGET_MS)
    }

    /// 探针开销核验（三件之三）。
    pub fn probe_ok(&self) -> bool {
        self.probe_bp <= PROBE_BUDGET_BP
    }

    /// 高负载降级：场景提示降为异步摘要（读屏可达必附文本替代）。
    pub fn degrade_to_digest(&mut self, scene: u8, text_alt: &'static str, urgent: bool) -> Result<(), SpCode> {
        if text_alt.is_empty() {
            return Err(SpCode::LoadRange); // 无文本替代 = 读屏不可达 = 拒（域色）
        }
        self.digest.push(Digest { scene, text_alt, urgent });
        if self.digest.len() >= DIGEST_TRIGGER {
            return Err(SpCode::DigestBacklog);
        }
        Ok(())
    }

    /// 摘要积压批量合并（合并后队列清空，批次计数留痕）。
    pub fn merge_digest(&mut self) -> usize {
        let n = self.digest.len();
        if n > 0 {
            self.merged_batches += 1;
            self.digest.clear();
        }
        n
    }

    /// 隔离舱熔断：单场景负载信号越阈即熔断（不扩散其他场景）。
    pub fn trip(&mut self, scene: u8, load_bp: u32) -> Result<BayState, SpCode> {
        if load_bp > 10000 {
            return Err(SpCode::LoadRange);
        }
        let st = if load_bp > 9000 {
            let s = BayState::Tripped(self.now);
            self.upsert_bay(scene, s);
            s
        } else {
            BayState::Open
        };
        Ok(st)
    }

    fn upsert_bay(&mut self, scene: u8, st: BayState) {
        if let Some(b) = self.bays.iter_mut().find(|b| b.0 == scene) {
            b.1 = st;
        } else {
            self.bays.push((scene, st));
        }
    }

    /// 熔断不扩散断言：熔断场景拒服，其余场景照常。
    pub fn bay_serving(&self, scene: u8) -> bool {
        match self.bays.iter().find(|b| b.0 == scene) {
            Some((_, BayState::Open)) | None => true,
            Some((_, BayState::Tripped(_))) => false,
        }
    }

    /// 资源回升逐级恢复（熔断 60s 后自动复位）。
    pub fn recover(&mut self) -> usize {
        let now = self.now;
        let mut n = 0;
        for b in self.bays.iter_mut() {
            if let BayState::Tripped(t) = b.1 {
                if now >= t + 60 {
                    b.1 = BayState::Open;
                    n += 1;
                }
            }
        }
        n
    }

    pub fn advance(&mut self, dt: u64) {
        self.now += dt;
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves545_checks() -> CheckSet {
    let mut set = CheckSet::new("ves545");
    let mut sp = ScenePerf::new(0);

    // C545-01 P95 预算内核验（三件之一）。
    sp.report_p95(1, 88);
    chk(&mut set, "C545-01", sp.p95_ok(1) == Ok(true), "P95 预算内");

    // C545-02 P95 超阈检出。
    sp.report_p95(2, 140);
    chk(&mut set, "C545-02", sp.p95_ok(2) == Ok(false), "P95 超阈检出");

    // C545-03 无样本场景拒绝（不臆测）。
    chk(&mut set, "C545-03", sp.p95_ok(9) == Err(SpCode::LoadRange), "无样本不臆测");

    // C545-04 探针开销预算内（三件之三）。
    sp.probe_bp = 99;
    chk(&mut set, "C545-04", sp.probe_ok(), "探针开销预算内");

    // C545-05 探针超阈检出。
    sp.probe_bp = 120;
    chk(&mut set, "C545-05", !sp.probe_ok(), "探针超阈检出");

    // C545-06 降级为异步摘要（读屏可达必附文本替代）。
    chk(&mut set, "C545-06", sp.degrade_to_digest(1, "扫描延迟提示", false).is_ok() && sp.digest.len() == 1, "降级入摘要");

    // C545-07 无文本替代拒收（读屏不可达即拒——域色）。
    chk(&mut set, "C545-07", sp.degrade_to_digest(1, "", true).is_err(), "无替代文本拒");

    // C545-08 摘要积压触发批量合并信号。
    let mut backlogged = false;
    for i in 0..DIGEST_TRIGGER {
        if sp.degrade_to_digest(2, if i % 2 == 0 { "提示甲" } else { "提示乙" }, false).is_err() {
            backlogged = true;
        }
    }
    chk(&mut set, "C545-08", backlogged && sp.digest.len() >= DIGEST_TRIGGER, "积压触发信号");

    // C545-09 批量合并清队留痕。
    let merged = sp.merge_digest();
    chk(&mut set, "C545-09", merged >= DIGEST_TRIGGER && sp.digest.is_empty() && sp.merged_batches == 1, "合并清队留痕");

    // C545-10 隔离舱熔断单场景。
    let t = sp.trip(3, 9500);
    chk(&mut set, "C545-10", t == Ok(BayState::Tripped(0)) && !sp.bay_serving(3), "单场景熔断");

    // C545-11 熔断不扩散（他场景照常）。
    chk(&mut set, "C545-11", sp.bay_serving(1) && sp.bay_serving(2), "熔断不扩散");

    // C545-12 资源回升复位（60s 后）+ 负载信号越界拒。
    sp.advance(59);
    let r1 = sp.recover();
    sp.advance(1);
    let r2 = sp.recover();
    chk(&mut set, "C545-12", r1 == 0 && r2 == 1 && sp.bay_serving(3) && sp.trip(4, 20000).is_err(), "回升复位+越界拒");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p95_budget() {
        let mut sp = ScenePerf::new(0);
        sp.report_p95(1, 100);
        assert_eq!(sp.p95_ok(1), Ok(true)); // 边界 = 预算内
        sp.report_p95(1, 101);
        assert_eq!(sp.p95_ok(1), Ok(false));
    }

    #[test]
    fn digest_backlog_merge() {
        let mut sp = ScenePerf::new(0);
        for i in 0..DIGEST_TRIGGER {
            let _ = sp.degrade_to_digest(1, "t", i == 0);
        }
        assert_eq!(sp.digest.len(), DIGEST_TRIGGER);
        assert_eq!(sp.merge_digest(), DIGEST_TRIGGER);
        assert!(sp.digest.is_empty());
    }

    #[test]
    fn bay_isolation_and_recover() {
        let mut sp = ScenePerf::new(0);
        assert_eq!(sp.trip(2, 9500), Ok(BayState::Tripped(0)));
        assert!(!sp.bay_serving(2));
        assert!(sp.bay_serving(3));
        sp.advance(60);
        assert_eq!(sp.recover(), 1);
        assert!(sp.bay_serving(2));
    }
}
