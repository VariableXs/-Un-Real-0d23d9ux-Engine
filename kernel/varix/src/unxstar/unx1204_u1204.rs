// unx1204_u1204 —— UNX-F1204 Polling 态功耗画像账（纯轮询档 CPU 占用实测与预算）
// 判据原文 UNX-F1204-J1：纯轮询档 CPU 占用率逐核实测可查（采样率 100%），
// 占用与预算偏差 ≤5%，功耗代价随态迁移决策可见（决策输入联通断言）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x9890 独占（0x9890~0x98cf）。
//
// 设计（纯功能 no_std）：
//   * 逐核占用像：busy/total 双计数器 → 占用率（千分比）；
//   * 预算带：occup ∈ [budget−500, budget+500]（≤5% 偏差）；
//   * 采样率账：登记核全部有像 = 100%；
//   * 态迁移决策：超预算核 → 建议降档（Polling→Irq），决策输入联通；
//   * 守恒：busy ≤ total；预算带内核数 + 超带核数 = 登记核数。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x9890 独占） ─────────────────────────────────────────────
pub const XC_PW2_SEALED: u32 = 0x9891; // 封账后采样
pub const XC_PW2_NOCORE: u32 = 0x9892; // 未登记核采样
pub const XC_PW2_DUP: u32 = 0x9893; // 重复登记
pub const XC_PW2_OOB: u32 = 0x9894; // busy > total
pub const XC_PW2_NOBUDGET: u32 = 0x9895; // 预算未登记
pub const XC_PW2_FULL: u32 = 0x9896; // 容量满

pub const BUDGET_TOL: u32 = 500; // 偏差带 500bps = 5%

/// 单核占用像。
#[derive(Clone, Copy)]
pub struct CoreOccup {
    pub core: u32,
    pub busy: u64,
    pub total: u64,
    pub budget_bps: u32, // 预算占用（千分比，10000=100%）
}

impl CoreOccup {
    /// 实测占用率（千分比）。
    pub fn occup_bps(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.busy * 10_000 / self.total) as u32
    }

    /// 预算带内判定（≤5% 偏差）。
    pub fn in_band(&self) -> bool {
        let occ = self.occup_bps() as i64;
        let bud = self.budget_bps as i64;
        (occ - bud).abs() <= BUDGET_TOL as i64
    }
}

/// 功耗画像账。
pub struct PwrPortrait {
    cap: usize,
    cores: Vec<CoreOccup>,
    samples: u64, // 采样轮数（每轮逐核刷新）
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl PwrPortrait {
    pub fn new(cap: usize) -> PwrPortrait {
        PwrPortrait { cap, cores: vec![], samples: 0, sealed: false, rejects: vec![] }
    }

    /// 登记核（带预算）。
    pub fn enroll(&mut self, core: u32, budget_bps: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_PW2_SEALED, "封账后登记"));
            return Err(XC_PW2_SEALED);
        }
        if self.cores.len() >= self.cap {
            self.rejects.push((XC_PW2_FULL, "容量满"));
            return Err(XC_PW2_FULL);
        }
        if budget_bps > 10_000 {
            self.rejects.push((XC_PW2_NOBUDGET, "预算越界"));
            return Err(XC_PW2_NOBUDGET);
        }
        if self.cores.iter().any(|c| c.core == core) {
            self.rejects.push((XC_PW2_DUP, "重复登记"));
            return Err(XC_PW2_DUP);
        }
        self.cores.push(CoreOccup { core, busy: 0, total: 0, budget_bps });
        Ok(())
    }

    /// 采样一轮：逐核 busy/total 增量（busy ≤ total 校验）。
    pub fn sample(&mut self, core: u32, busy_delta: u64, total_delta: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_PW2_SEALED, "封账后采样"));
            return Err(XC_PW2_SEALED);
        }
        if total_delta == 0 || busy_delta > total_delta {
            self.rejects.push((XC_PW2_OOB, "busy 越总量"));
            return Err(XC_PW2_OOB);
        }
        let idx = match self.cores.iter().position(|c| c.core == core) {
            Some(i) => i,
            None => {
                self.rejects.push((XC_PW2_NOCORE, "未登记核采样"));
                return Err(XC_PW2_NOCORE);
            }
        };
        self.cores[idx].busy += busy_delta;
        self.cores[idx].total += total_delta;
        self.samples += 1;
        Ok(())
    }

    /// 采样率（千分比）：登记核全部有像 = 100%。
    pub fn coverage_bps(&self) -> u32 {
        if self.cores.is_empty() {
            return 0;
        }
        let hit = self.cores.iter().filter(|c| c.total > 0).count() as u64;
        (hit * 10_000 / self.cores.len() as u64) as u32
    }

    /// 预算带内核数。
    pub fn in_band_cnt(&self) -> usize {
        self.cores.iter().filter(|c| c.total > 0 && c.in_band()).count()
    }

    /// 超带核名单（决策输入）。
    pub fn over_budget_cores(&self) -> Vec<u32> {
        self.cores
            .iter()
            .filter(|c| c.total > 0 && !c.in_band())
            .map(|c| c.core)
            .collect()
    }

    /// 态迁移决策联通：超带核 → 降档建议（Polling→Irq）确定性产出。
    pub fn migration_advice(&self) -> Vec<(u32, u8)> {
        self.over_budget_cores().iter().map(|&c| (c, 1u8)).collect()
    }

    /// 守恒：带内 + 超带 = 已采样核数。
    pub fn conserved(&self) -> bool {
        let sampled = self.cores.iter().filter(|c| c.total > 0).count();
        self.in_band_cnt() + self.over_budget_cores().len() == sampled
    }

    /// 逐核占用快照（决策输入联通断言用）。
    pub fn occup_of(&self, core: u32) -> Option<u32> {
        self.cores.iter().find(|c| c.core == core).map(|c| c.occup_bps())
    }

    pub fn cores_len(&self) -> usize {
        self.cores.len()
    }

    pub fn samples(&self) -> u64 {
        self.samples
    }

    pub fn seal(&mut self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&self.cores.len().to_le_bytes());
        buf.extend_from_slice(&self.samples.to_le_bytes());
        for c in &self.cores {
            buf.extend_from_slice(&c.busy.to_le_bytes());
        }
        self.sealed = true;
        fnv64(&buf)
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    pub fn rejects(&self) -> &[(u32, &'static str)] {
        &self.rejects
    }
}

fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ── 深化块：画像压测编排（判据主体路径） ─────────────────────────────

/// 全核画像编排：n 核逐轮采样，占用收敛到预算带。
/// 节奏（确定性）：核 c 每轮 busy_delta = budget/10、total_delta = 1000。
pub fn portrait_run(n: u32, budget_bps: u32, rounds: u64) -> Result<(u32, usize), u32> {
    let mut pp = PwrPortrait::new(n as usize + 4);
    for c in 0..n {
        pp.enroll(c, budget_bps)?;
    }
    for _ in 0..rounds {
        for c in 0..n {
            let b = (budget_bps as u64) / 10;
            pp.sample(c, b, 1000)?;
        }
    }
    let cov = pp.coverage_bps();
    if cov != 10_000 || !pp.conserved() {
        return Err(XC_PW2_NOCORE);
    }
    Ok((cov, pp.in_band_cnt()))
}

/// 决策联通演练：注入超带核 → 建议表非空且核号对得上。
pub fn decision_link_run() -> Result<usize, u32> {
    let mut pp = PwrPortrait::new(8);
    pp.enroll(0, 3_000)?; // 预算 30%
    pp.enroll(1, 9_000)?; // 预算 90%
    // 核 0 实测 90%（超带）；核 1 实测 90%（带内）。
    pp.sample(0, 9_000, 10_000)?;
    pp.sample(1, 9_000, 10_000)?;
    let adv = pp.migration_advice();
    let link_ok = adv.len() == 1 && adv[0].0 == 0 && adv[0].1 == 1;
    if !link_ok || pp.conserved() == false {
        return Err(XC_PW2_NOBUDGET);
    }
    Ok(adv.len())
}

// ── 注入核：六类破坏全抓 ─────────────────────────────────────────────
// 破坏面：①未登记核采样 ②busy 越总量 ③重复登记 ④预算越界
// ⑤封账后采样 ⑥零总量采样。

pub fn inject_core() -> (u32, u32) {
    let mut hit = 0u32;
    let mut total = 0u32;

    // ① 未登记核采样 → XC_PW2_NOCORE
    total += 1;
    let mut pp = PwrPortrait::new(4);
    if pp.sample(9, 1, 10) == Err(XC_PW2_NOCORE) {
        hit += 1;
    }

    // ② busy 越总量 → XC_PW2_OOB
    total += 1;
    let mut pp2 = PwrPortrait::new(4);
    let _ = pp2.enroll(0, 5_000);
    if pp2.sample(0, 11, 10) == Err(XC_PW2_OOB) {
        hit += 1;
    }

    // ③ 重复登记 → XC_PW2_DUP
    total += 1;
    let mut pp3 = PwrPortrait::new(4);
    let _ = pp3.enroll(0, 5_000);
    if pp3.enroll(0, 5_000) == Err(XC_PW2_DUP) {
        hit += 1;
    }

    // ④ 预算越界 → XC_PW2_NOBUDGET
    total += 1;
    let mut pp4 = PwrPortrait::new(4);
    if pp4.enroll(0, 10_001) == Err(XC_PW2_NOBUDGET) {
        hit += 1;
    }

    // ⑤ 封账后采样 → XC_PW2_SEALED
    total += 1;
    let mut pp5 = PwrPortrait::new(4);
    let _ = pp5.enroll(0, 5_000);
    let _ = pp5.seal();
    if pp5.sample(0, 1, 10) == Err(XC_PW2_SEALED) {
        hit += 1;
    }

    // ⑥ 零总量采样 → XC_PW2_OOB
    total += 1;
    let mut pp6 = PwrPortrait::new(4);
    let _ = pp6.enroll(0, 5_000);
    if pp6.sample(0, 0, 0) == Err(XC_PW2_OOB) {
        hit += 1;
    }

    (hit, total)
}
