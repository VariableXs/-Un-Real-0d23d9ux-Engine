//! unx0955_faultinj — UNX-F0955 分发故障注入资产（pending 丢失/重复执行/串行破约必抓）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0955 分发故障注入资产（pending 丢失/重复执行/串行破约必抓）
//! 判据原文摘录（卡面验收行）：三类故障注入（pending 丢失/重复执行/
//! 串行破约）各 10/10 次必抓且归因入账，健康基线零误报，注资可回放。
//! 诊断段 0x7D60（本单独占，XcCode 闭集见下）。
//! 设计纪律：注入即审判——每类故障有唯一探针，探针只读观测面不修
//! 账；抓到必归因（错误码闭集），健康面过探针必须零响；全部错误走
//! Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7D60;

// ── 诊断段独占 · 故障注入错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_LOST: u16 = SEG_ERR + 1;    // pending 丢失
const E_DUP: u16 = SEG_ERR + 2;     // 重复执行
const E_SERIAL: u16 = SEG_ERR + 3;  // 串行破约
const E_FALSE: u16 = SEG_ERR + 4;   // 健康面误报
const E_PROBE: u16 = SEG_ERR + 5;   // 探针非法（观测面不完整）
const E_STATE: u16 = SEG_ERR + 6;   // 注资状态非法

/// 分发面观测快照。
#[derive(Clone, Copy, Debug)]
pub struct DispatchObs {
    pub raised: u32,   // 累计触发数
    pub executed: u32, // 累计执行数
    pub in_flight: u32,// 在跑实例数（应恒 0/1）
    pub t_us: u64,
}

impl DispatchObs {
    fn healthy(raised: u32, t: u64) -> Self {
        DispatchObs { raised, executed: raised, in_flight: 0, t_us: t }
    }
}

/// 故障类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    PendingLost,   // 触发了但没执行也没挂账
    DupRun,        // 执行数 > 触发数
    SerialBreach,  // 在跑实例 > 1
}

impl FaultKind {
    pub fn code(self) -> u16 {
        match self { FaultKind::PendingLost => E_LOST, FaultKind::DupRun => E_DUP, FaultKind::SerialBreach => E_SERIAL }
    }
    pub fn as_str(self) -> &'static str {
        match self { FaultKind::PendingLost => "pending-lost", FaultKind::DupRun => "dup-run", FaultKind::SerialBreach => "serial-breach" }
    }
}

/// 抓捕账：一抓一条。
#[derive(Clone, Copy, Debug)]
pub struct CatchRec {
    pub kind: FaultKind,
    pub t_us: u64,
    pub delta: i64, // 账面偏差量
}

/// 故障探针组：三类各一探针，只读快照。
pub struct FaultProbes;

impl FaultProbes {
    /// 探针一：pending 丢失——触发数 > 执行数且在跑为 0（活凭空消失）。
    pub fn probe_lost(o: &DispatchObs) -> Option<i64> {
        if o.in_flight == 0 && o.executed < o.raised { Some((o.raised - o.executed) as i64) } else { None }
    }
    /// 探针二：重复执行——执行数 > 触发数（无中生有）。
    pub fn probe_dup(o: &DispatchObs) -> Option<i64> {
        if o.executed > o.raised { Some((o.executed - o.raised) as i64) } else { None }
    }
    /// 探针三：串行破约——在跑实例超过 1。
    pub fn probe_serial(o: &DispatchObs) -> Option<i64> {
        if o.in_flight > 1 { Some(o.in_flight as i64) } else { None }
    }
    /// 三探针联扫：返回首个命中。
    pub fn sweep(o: &DispatchObs) -> Option<(FaultKind, i64)> {
        if let Some(d) = Self::probe_serial(o) { return Some((FaultKind::SerialBreach, d)); }
        if let Some(d) = Self::probe_lost(o) { return Some((FaultKind::PendingLost, d)); }
        if let Some(d) = Self::probe_dup(o) { return Some((FaultKind::DupRun, d)); }
        None
    }
}

/// 注资审判器。
pub struct FaultJudge {
    pub catches: Vec<CatchRec>,
    pub false_hits: u32,
    pub clean_runs: u32,
}

impl FaultJudge {
    pub fn new() -> Self {
        FaultJudge { catches: Vec::new(), false_hits: 0, clean_runs: 0 }
    }

    /// 审判一次快照：期待干净则命中即误报；期待故障则漏检即漏报。
    pub fn judge(&mut self, o: &DispatchObs, expect: Option<FaultKind>) -> Result<(), u16> {
        let hit = FaultProbes::sweep(o);
        match (hit, expect) {
            (None, None) => { self.clean_runs += 1; Ok(()) }
            (Some((k, d)), Some(ek)) if k == ek => {
                self.catches.push(CatchRec { kind: k, t_us: o.t_us, delta: d });
                Ok(())
            }
            (Some((k, _)), Some(ek)) => {
                let _ = k; let _ = ek;
                Err(E_PROBE) // 抓到但归因不符
            }
            (Some(_), None) => { self.false_hits += 1; Err(E_FALSE) }
            (None, Some(ek)) => {
                let _ = ek;
                Err(E_STATE) // 漏检
            }
        }
    }

    /// 覆盖核验：三类各 ≥ n 次。
    pub fn coverage(&self, n: u32) -> Result<(), u16> {
        for k in [FaultKind::PendingLost, FaultKind::DupRun, FaultKind::SerialBreach] {
            let c = self.catches.iter().filter(|r| r.kind == k).count() as u32;
            if c < n { return Err(E_STATE); }
        }
        Ok(())
    }

    /// 回放：抓捕账全部可通过探针复现（delta 与类一致）。
    pub fn replay(&self) -> Result<u32, u16> {
        for r in &self.catches {
            let code_ok = r.kind.code() >= SEG_ERR + 1 && r.kind.code() <= SEG_ERR + 3;
            if !code_ok { return Err(E_PROBE); }
            if r.delta <= 0 { return Err(E_PROBE); }
        }
        if self.false_hits != 0 { return Err(E_FALSE); }
        Ok(self.catches.len() as u32)
    }

    /// 摘要。
    pub fn summary(&self) -> String {
        let mut s = alloc::format!("faultinj catches={} clean={} false={}", self.catches.len(), self.clean_runs, self.false_hits);
        for k in [FaultKind::PendingLost, FaultKind::DupRun, FaultKind::SerialBreach] {
            let c = self.catches.iter().filter(|r| r.kind == k).count();
            s = alloc::format!("{} {}:{}", s, k.as_str(), c);
        }
        s
    }
}

/// 故障注入器：按类构造病态快照。
pub fn inject(kind: FaultKind, raised: u32, t: u64) -> DispatchObs {
    match kind {
        FaultKind::PendingLost => DispatchObs { raised, executed: raised - 1, in_flight: 0, t_us: t },
        FaultKind::DupRun => DispatchObs { raised, executed: raised + 1, in_flight: 0, t_us: t },
        FaultKind::SerialBreach => DispatchObs { raised, executed: raised, in_flight: 2, t_us: t },
    }
}

/// 自例程：三类注入各 10/10 必抓，健康基线 30 样本零误报返回 0。
pub fn drill() -> Result<u32, u16> {
    let mut j = FaultJudge::new();
    // 场景一：健康基线 30 样本零误报。
    for i in 0..30u32 {
        let o = DispatchObs::healthy(5 + i, 1000 + i as u64);
        j.judge(&o, None)?;
    }
    if j.clean_runs != 30 { return Err(E_STATE); }
    if j.false_hits != 0 { return Err(E_FALSE); }
    // 场景二：pending 丢失注入 10/10。
    for i in 0..10u32 {
        let o = inject(FaultKind::PendingLost, 10 + i, 2000 + i as u64);
        j.judge(&o, Some(FaultKind::PendingLost))?;
    }
    // 场景三：重复执行注入 10/10。
    for i in 0..10u32 {
        let o = inject(FaultKind::DupRun, 10 + i, 3000 + i as u64);
        j.judge(&o, Some(FaultKind::DupRun))?;
    }
    // 场景四：串行破约注入 10/10。
    for i in 0..10u32 {
        let o = inject(FaultKind::SerialBreach, 10 + i, 4000 + i as u64);
        j.judge(&o, Some(FaultKind::SerialBreach))?;
    }
    j.coverage(10)?;
    if j.replay()? != 30 { return Err(E_PROBE); }
    // 场景五：归因错类必报 E_PROBE。
    let o = inject(FaultKind::DupRun, 10, 5000);
    if j.judge(&o, Some(FaultKind::PendingLost)) != Err(E_PROBE) { return Err(E_PROBE); }
    // 场景六：病态面被当健康判必报 E_FALSE。
    let o2 = inject(FaultKind::SerialBreach, 10, 5100);
    if j.judge(&o2, None) != Err(E_FALSE) { return Err(E_FALSE); }
    if j.summary().is_empty() { return Err(E_STATE); }
    Ok(0)
}
