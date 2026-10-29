//! UNX-F10295 · 对接面 ktest 集成
//! 段位 0x11800 · 判据锚 UNX-F10295-J1 · 域 O3 对接面/质量闸
//! 结构：KtestGate（ktest 桥）+ ProbePair（集成/脱管配对探针）+ GateLedger（闸账）
//! 语义：对接面的判据链必须桥入 ktest 主回路——「fail==0 核」逐次执行，
//!       同时必须检出「探针脱管」（判据存在但未挂接 ktest 主回路）的假集成态。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x11800;

/// ktest 桥接状态
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BridgeState {
    /// 未挂接（脱管）
    Detached,
    /// 挂接中（闸门尚未放行过）
    Arming,
    /// 已桥入主回路且最近一轮 fail==0
    Bridged,
    /// 已桥入但最近一轮存在 fail
    BridgedRed,
}

impl BridgeState {
    pub fn is_managed(self) -> bool {
        matches!(self, BridgeState::Bridged | BridgeState::BridgedRed)
    }
    pub fn code(self) -> u16 {
        (SEG_BASE as u16) + match self {
            BridgeState::Detached => 1,
            BridgeState::Arming => 2,
            BridgeState::Bridged => 3,
            BridgeState::BridgedRed => 4,
        }
    }
}

/// ktest 主回路单轮结果快照
#[derive(Clone, Debug)]
pub struct KtestRound {
    pub round: u32,
    /// 本轮执行的判据核数
    pub cores_run: u32,
    /// 本轮失败的判据核数（集成达成的必要条件为 0）
    pub fails: u32,
    /// 本轮是否由 ktest 主回路调度（true=桥内，false=脱管自跑）
    pub scheduled_by_ktest: bool,
}

impl KtestRound {
    pub fn healthy(&self) -> bool {
        self.scheduled_by_ktest && self.fails == 0 && self.cores_run > 0
    }
}

/// 探针配对：一条判据在「桥内」与「脱管」两种执行路径下的观测
#[derive(Clone, Debug)]
pub struct ProbePair {
    pub name: String,
    /// 桥内执行结果（Some=已执行）
    pub bridged_run: Option<bool>,
    /// 脱管执行结果
    pub detached_run: Option<bool>,
}

impl ProbePair {
    pub fn new(name: &str) -> Self {
        ProbePair { name: name.to_string(), bridged_run: None, detached_run: None }
    }
    /// 脱管检出：桥内从未执行而脱管在跑，或两侧结论相悖
    pub fn is_runaway(&self) -> bool {
        match (self.bridged_run, self.detached_run) {
            (None, Some(_)) => true,
            (Some(b), Some(d)) => b != d,
            _ => false,
        }
    }
}

/// 对接面 ktest 闸账
pub struct GateLedger {
    pub bridge: BridgeState,
    rounds: Vec<KtestRound>,
    probes: Vec<ProbePair>,
    /// 脱管检出事件数
    runaway_hits: u32,
    /// 桥内 fail==0 达成次数
    green_bridges: u32,
}

impl GateLedger {
    pub fn new() -> Self {
        GateLedger {
            bridge: BridgeState::Detached,
            rounds: Vec::new(),
            probes: Vec::new(),
            runaway_hits: 0,
            green_bridges: 0,
        }
    }

    /// 注册探针（判据名），桥内/脱管两侧共享同名条目
    pub fn register(&mut self, name: &str) {
        if !self.probes.iter().any(|p| p.name == name) {
            self.probes.push(ProbePair::new(name));
        }
    }

    /// 记录一轮 ktest 主回路执行：仅当 scheduled_by_ktest 为真才推进桥状态
    pub fn record_round(&mut self, r: KtestRound) {
        self.bridge = if r.scheduled_by_ktest {
            if r.fails == 0 && r.cores_run > 0 {
                self.green_bridges += 1;
                BridgeState::Bridged
            } else {
                BridgeState::BridgedRed
            }
        } else {
            BridgeState::Detached
        };
        self.rounds.push(r);
    }

    /// 记录一次桥内执行
    pub fn record_bridged(&mut self, name: &str, pass: bool) {
        for p in self.probes.iter_mut() {
            if p.name == name {
                p.bridged_run = Some(pass);
                return;
            }
        }
    }

    /// 记录一次脱管执行并即时检出
    pub fn record_detached(&mut self, name: &str, pass: bool) -> bool {
        for p in self.probes.iter_mut() {
            if p.name == name {
                p.detached_run = Some(pass);
                if p.is_runaway() {
                    self.runaway_hits += 1;
                    return true;
                }
                return false;
            }
        }
        false
    }

    /// 集成达成判定：连续 need 轮桥内 fail==0，且全部探针受管
    pub fn integration_ok(&self, need: u32) -> bool {
        if self.probes.iter().any(|p| p.is_runaway()) {
            return false;
        }
        let tail = self.rounds.iter().rev().take(need as usize);
        let mut n = 0u32;
        for r in tail {
            if !r.scheduled_by_ktest || r.fails != 0 || r.cores_run == 0 {
                return false;
            }
            n += 1;
        }
        n == need
    }

    /// 桥内近 N 轮失败数合计（脱管轮不计入——那不是主回路的声音）
    pub fn recent_fails(&self, n: usize) -> u32 {
        self.rounds.iter().rev().take(n).filter(|r| r.scheduled_by_ktest).map(|r| r.fails).sum()
    }

    pub fn runaway_hits(&self) -> u32 {
        self.runaway_hits
    }
    pub fn green_bridges(&self) -> u32 {
        self.green_bridges
    }
    pub fn probe_count(&self) -> usize {
        self.probes.len()
    }

    /// 桥报告：段位折叠指纹（FNV-1a）
    pub fn report_fp(&self) -> u32 {
        let mut h: u32 = 0x811C9DC5;
        let mut feed = |b: u8, h: &mut u32| {
            *h ^= b as u32;
            *h = h.wrapping_mul(0x01000193);
        };
        feed((SEG_BASE & 0xFF) as u8, &mut h);
        feed(((SEG_BASE >> 8) & 0xFF) as u8, &mut h);
        feed(self.bridge.code() as u8, &mut h);
        feed((self.rounds.len() & 0xFF) as u8, &mut h);
        feed((self.green_bridges & 0xFF) as u8, &mut h);
        feed((self.runaway_hits & 0xFF) as u8, &mut h);
        h
    }

    /// 渲染集成摘要
    pub fn render(&self) -> String {
        format!(
            "ktest-gate seg={:#06x} bridge={:?} rounds={} green={} runaway={} probes={}",
            SEG_BASE,
            self.bridge,
            self.rounds.len(),
            self.green_bridges,
            self.runaway_hits,
            self.probes.len()
        )
    }
}

/// 受控注入核：kind=0 脱管执行漏记 / kind=1 桥内注入 fail / kind=2 桥状态伪造 Bridged
pub fn inject_fault(g: &mut GateLedger, kind: u8) -> &'static str {
    match kind {
        0 => {
            g.record_detached("injected-probe", true);
            "detached-run-accepted"
        }
        1 => {
            g.record_round(KtestRound { round: 999, cores_run: 10, fails: 2, scheduled_by_ktest: true });
            "bridged-fail-recorded"
        }
        2 => {
            g.bridge = BridgeState::Bridged;
            "forged-bridge"
        }
        _ => "no-op",
    }
}

pub fn run_unx10295_checks() -> CheckSet {
    let mut s = CheckSet::new("O3");
    // J1-01 基线：空账未集成
    {
        let g = GateLedger::new();
        s.add("基线-空账未集成", !g.integration_ok(10) && g.bridge == BridgeState::Detached, "空账必须 Detached 且不达成");
    }
    // J1-02 注册与同名去重
    {
        let mut g = GateLedger::new();
        g.register("probe-a");
        g.register("probe-a");
        g.register("probe-b");
        s.add("注册-同名去重", g.probe_count() == 2, "同名探针只记一条");
    }
    // J1-03 正路径：10 轮桥内 fail==0 达成集成
    {
        let mut g = GateLedger::new();
        g.register("probe-a");
        for i in 0..10u32 {
            g.record_round(KtestRound { round: i, cores_run: 12, fails: 0, scheduled_by_ktest: true });
            g.record_bridged("probe-a", true);
        }
        s.add("正路径-十轮绿达成", g.integration_ok(10) && g.bridge.is_managed(), "10/10 桥内 fail==0 须达成");
    }
    // J1-04 负路径：脱管执行即检出且集成不达成
    {
        let mut g = GateLedger::new();
        g.register("probe-a");
        for i in 0..10u32 {
            g.record_round(KtestRound { round: i, cores_run: 12, fails: 0, scheduled_by_ktest: true });
        }
        let hit = g.record_detached("probe-a", true);
        s.add("负路径-脱管即时检出", hit && g.runaway_hits() == 1, "桥内缺记+脱管在跑必须判脱管");
        s.add("负路径-脱管阻断集成", !g.integration_ok(10), "存在 runaway 探针集成必须拒绝");
    }
    // J1-05 桥内 fail 破坏连续绿窗
    {
        let mut g = GateLedger::new();
        for i in 0..10u32 {
            let fails = if i == 9 { 1 } else { 0 };
            g.record_round(KtestRound { round: i, cores_run: 12, fails, scheduled_by_ktest: true });
        }
        s.add("负路径-桥内红阻断", !g.integration_ok(10) && g.bridge == BridgeState::BridgedRed, "fail>0 轮必须 BridgedRed 且破窗");
    }
    // J1-06 两侧结论相悖判脱管
    {
        let mut g = GateLedger::new();
        g.register("probe-x");
        g.record_round(KtestRound { round: 1, cores_run: 5, fails: 0, scheduled_by_ktest: true });
        g.record_bridged("probe-x", true);
        let hit = g.record_detached("probe-x", false);
        s.add("检出-两侧相悖", hit && g.runaway_hits() == 1, "桥内 true 脱管 false 结论相悖判脱管");
    }
    // J1-07 注入检出 0/1/2 三型
    {
        let mut g0 = GateLedger::new();
        g0.register("injected-probe");
        let r0 = inject_fault(&mut g0, 0);
        s.add("注入-脱管漏记检出", r0 == "detached-run-accepted" && g0.runaway_hits() == 1, "注入0须被 runaway 通道捕获");
        let mut g1 = GateLedger::new();
        let r1 = inject_fault(&mut g1, 1);
        s.add("注入-桥内红入账", r1 == "bridged-fail-recorded" && g1.recent_fails(1) == 2, "注入1 fail 须入桥内账");
        let mut g2 = GateLedger::new();
        let r2 = inject_fault(&mut g2, 2);
        s.add("注入-伪造桥不等于集成", r2 == "forged-bridge" && !g2.integration_ok(10), "伪造 Bridged 无绿窗支撑不得达成");
    }
    // J1-08 recent_fails 只统计桥内轮
    {
        let mut g = GateLedger::new();
        g.record_round(KtestRound { round: 1, cores_run: 5, fails: 3, scheduled_by_ktest: true });
        g.record_round(KtestRound { round: 2, cores_run: 5, fails: 7, scheduled_by_ktest: false });
        s.add("口径-脱管轮不计 fail", g.recent_fails(2) == 3, "recent_fails 须剔除脱管轮");
    }
    // J1-09 边界：桥内绿但轮数不足 need
    {
        let mut g = GateLedger::new();
        for i in 0..7u32 {
            g.record_round(KtestRound { round: i, cores_run: 9, fails: 0, scheduled_by_ktest: true });
        }
        s.add("边界-绿窗不足拒达成", !g.integration_ok(10) && g.integration_ok(7), "7<10 拒绝，按 7 口径达成");
    }
    // J1-10 十轮回放：FP 稳定 + 摘要含段位
    {
        let mut g = GateLedger::new();
        g.register("probe-a");
        let mut stable = true;
        let fp0 = {
            for i in 0..10u32 {
                g.record_round(KtestRound { round: i, cores_run: 12, fails: 0, scheduled_by_ktest: true });
                g.record_bridged("probe-a", true);
            }
            g.report_fp()
        };
        for _ in 0..10 {
            let mut g2 = GateLedger::new();
            g2.register("probe-a");
            for i in 0..10u32 {
                g2.record_round(KtestRound { round: i, cores_run: 12, fails: 0, scheduled_by_ktest: true });
                g2.record_bridged("probe-a", true);
            }
            if g2.report_fp() != fp0 {
                stable = false;
            }
        }
        let txt = g.render();
        s.add(
            "回放-指纹稳定摘要含段位",
            stable && txt.contains("0x11800") && g.integration_ok(10),
            "十轮同构回放指纹一致且摘要带段位",
        );
    }
    s
}
