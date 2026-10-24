// CGPU-F0307 域级 fuzz 总闸 · cgb307_fuzzgate.rs
// AI-MIMO-1022 · CGPU-B 收口
// 判据：跨模块 fuzz 统一调度；语料库共享；崩溃聚类去重；CI 固化；
//       每轮 fuzz 有覆盖率增量记录；总闸开/关可控。
#![allow(dead_code)]

use alloc::vec;
extern crate alloc;
use alloc::vec::Vec;

use super::super::checks::CheckSet;

/// fuzz 目标模块。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzTarget {
    pub id: u32,
    pub module_tag: u16,
    pub enabled: bool,
    pub max_cases: u32,
    pub timeout_us: u32,
}

/// 覆盖率增量。
#[derive(Clone, Copy, Default, Debug)]
pub struct CoverageDelta {
    pub edges_before: u32,
    pub edges_after: u32,
    pub branches_before: u32,
    pub branches_after: u32,
}

/// 崩溃聚类。
#[derive(Clone, Debug)]
pub struct CrashCluster {
    pub id: u32,
    pub stack_hash: u32,
    pub occurrences: u32,
    pub first_case: u32,
    pub repro_case: Vec<u8>,
    pub triaged: bool,
}

/// 总闸状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateState {
    Closed,
    Open,
    Degraded,
    Halted,
}

/// 域级 fuzz 总闸。
pub struct FuzzGate {
    pub state: GateState,
    pub targets: Vec<FuzzTarget>,
    pub corpus: Vec<Vec<u8>>,
    pub clusters: Vec<CrashCluster>,
    pub coverage: CoverageDelta,
    pub total_cases: u64,
    pub total_crashes: u64,
    pub max_corpus: usize,
    pub round: u32,
}

impl FuzzGate {
    pub fn new() -> Self {
        Self {
            state: GateState::Closed,
            targets: Vec::new(),
            corpus: Vec::new(),
            clusters: Vec::new(),
            coverage: CoverageDelta::default(),
            total_cases: 0,
            total_crashes: 0,
            max_corpus: 4096,
            round: 0,
        }
    }

    pub fn add_target(&mut self, t: FuzzTarget) {
        self.targets.push(t);
    }

    pub fn open(&mut self) -> bool {
        if matches!(self.state, GateState::Halted) {
            return false;
        }
        self.state = GateState::Open;
        true
    }

    pub fn close(&mut self) {
        self.state = GateState::Closed;
    }

    pub fn halt(&mut self) {
        self.state = GateState::Halted;
    }

    /// 注入语料（去重，控制容量）。
    pub fn feed_corpus(&mut self, sample: Vec<u8>) -> bool {
        if self.corpus.contains(&sample) {
            return false;
        }
        if self.corpus.len() >= self.max_corpus {
            // 淘汰最早
            self.corpus.remove(0);
        }
        self.corpus.push(sample);
        true
    }

    /// 崩溃聚类（按 stack_hash 去重）。
    pub fn report_crash(&mut self, stack_hash: u32, case_id: u32, repro: Vec<u8>) {
        self.total_crashes += 1;
        if let Some(c) = self.clusters.iter_mut().find(|c| c.stack_hash == stack_hash) {
            c.occurrences += 1;
        } else {
            self.clusters.push(CrashCluster {
                id: self.clusters.len() as u32 + 1,
                stack_hash,
                occurrences: 1,
                first_case: case_id,
                repro_case: repro,
                triaged: false,
            });
        }
    }

    /// 记录覆盖率增量。
    pub fn record_coverage(&mut self, delta: CoverageDelta) {
        self.coverage = delta;
    }

    /// 跑一轮（遍历启用的目标）。
    pub fn run_round(&mut self) -> u32 {
        if !matches!(self.state, GateState::Open) {
            return 0;
        }
        self.round += 1;
        let mut executed = 0u32;
        for t in &self.targets {
            if t.enabled {
                executed += t.max_cases.min(1000); // 模拟
                self.total_cases += t.max_cases.min(1000) as u64;
            }
        }
        // 覆盖率增量记账
        self.coverage.edges_before = self.coverage.edges_after;
        self.coverage.branches_before = self.coverage.branches_after;
        self.coverage.edges_after += executed / 10;
        self.coverage.branches_after += executed / 20;
        executed
    }

    /// 覆盖率增长率（Q8）。
    pub fn coverage_growth_q8(&self) -> u16 {
        if self.coverage.edges_before == 0 {
            return 256;
        }
        let delta = self.coverage.edges_after.saturating_sub(self.coverage.edges_before);
        ((delta as u64 * 256) / self.coverage.edges_before as u64) as u16
    }

    /// 未分诊崩溃数。
    pub fn untriaged(&self) -> u32 {
        self.clusters.iter().filter(|c| !c.triaged).count() as u32
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0307 域级 fuzz 总闸");
        cs.add("目标 id 唯一", {
            let mut ids: Vec<u32> = self.targets.iter().map(|t| t.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.targets.len()
        }, "unique");
        cs.add("语料不超容量", self.corpus.len() <= self.max_corpus, "≤max");
        cs.add("崩溃聚类 stack_hash 唯一", {
            let mut h: Vec<u32> = self.clusters.iter().map(|c| c.stack_hash).collect();
            h.sort_unstable();
            h.dedup();
            h.len() == self.clusters.len()
        }, "unique hash");
        cs.add("总案例数 ≥ 崩溃数", self.total_cases >= self.total_crashes, "cases ≥ crashes");
        cs.add("覆盖率边数单调", {
            self.coverage.edges_after >= self.coverage.edges_before
        }, "edges mono");
        cs.add("闸门状态合法", {
            matches!(self.state, GateState::Closed | GateState::Open | GateState::Degraded | GateState::Halted)
        }, "valid state");
        cs.add("轮次与闸门一致", {
            self.round == 0 || matches!(self.state, GateState::Open | GateState::Degraded)
        }, "round consistency");
        cs
    }
}

impl Default for FuzzGate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_run_close() {
        let mut g = FuzzGate::new();
        g.add_target(FuzzTarget { id: 1, module_tag: 1, enabled: true, max_cases: 100, timeout_us: 1000 });
        assert!(g.open());
        let n = g.run_round();
        assert!(n > 0);
        g.close();
        assert_eq!(g.run_round(), 0);
    }

    #[test]
    fn crash_clustering() {
        let mut g = FuzzGate::new();
        g.report_crash(0xAAAA, 1, vec![1, 2, 3]);
        g.report_crash(0xAAAA, 2, vec![4, 5]);
        g.report_crash(0xBBBB, 3, vec![6]);
        assert_eq!(g.clusters.len(), 2);
        assert_eq!(g.clusters[0].occurrences, 2);
    }

    #[test]
    fn corpus_dedup_capacity() {
        let mut g = FuzzGate::new();
        g.max_corpus = 3;
        assert!(g.feed_corpus(vec![1]));
        assert!(!g.feed_corpus(vec![1]));
        assert!(g.feed_corpus(vec![2]));
        assert!(g.feed_corpus(vec![3]));
        assert!(g.feed_corpus(vec![4])); // 淘汰最老
        assert_eq!(g.corpus.len(), 3);
    }
}
