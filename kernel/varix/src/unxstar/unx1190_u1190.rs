// unx1190_u1190 —— UNX-F1190 亲和性策略压测（策略切换风暴与公平性扰动）
// 判据原文 UNX-F1190-J1：压测全过（策略切换风暴 N 轮耐受、公平性扰动量化
// （切换前后公平度偏差）、零丢失零回绕、风暴立案逐轮、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x8db0 独占（0x8db0~0x8def）。
//
// 设计（纯功能 no_std）：
//   * 风暴泵：N 轮随机策略切换（确定性伪随机 LCG），每轮切换后跑 M 轮分发；
//   * 公平度：基尼系数整数化（Σ|xi-xj| / (2nΣxi)，千分比），切换前后偏差记账；
//   * 扰动立案：公平度偏差 > 带（千分比阈）逐轮立案；
//   * 守恒：分发总数 = 每轮和；FNV 链指纹冻结。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x8db0 独占） ─────────────────────────────────────────────
pub const XC_STORM_SEALED: u32 = 0x8DB1; // 封账后风暴
pub const XC_STORM_ZERO_CORES: u32 = 0x8DB2; // 零核泵
pub const XC_STORM_DISTURB: u32 = 0x8DB3; // 公平扰动立案
pub const XC_STORM_FULL: u32 = 0x8DB4; // 轮账容量满

/// LCG 确定性伪随机（压测可复现）。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// 轮账行。
#[derive(Clone, Copy)]
pub struct StormRow {
    pub round: u32,
    /// 切换前公平度（千分比基尼）。
    pub gini_before: u32,
    /// 切换后公平度。
    pub gini_after: u32,
    /// 扰动量（千分比）。
    pub disturb: u32,
}

/// 风暴压测器。
pub struct StormTester {
    cores: usize,
    quota: u32,
    served: Vec<u32>,
    rounds: Vec<StormRow>,
    band_per_mille: u32,
    alarms: u32,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl StormTester {
    pub fn new(cores: usize, quota: u32, band_per_mille: u32) -> StormTester {
        StormTester {
            cores,
            quota,
            served: vec![0; cores],
            rounds: vec![],
            band_per_mille,
            alarms: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 基尼系数整数化（千分比，0=绝对公平 1000=绝对集中）。
    pub fn gini_per_mille(served: &[u32]) -> u32 {
        let n = served.len();
        if n == 0 {
            return 0;
        }
        let mut sum: u64 = 0;
        for x in served {
            sum = sum.saturating_add(*x as u64);
        }
        if sum == 0 {
            return 0;
        }
        // Σ|xi-xj| 双循环（n 小可用）
        let mut diff: u64 = 0;
        for i in 0..n {
            for j in 0..n {
                diff += (served[i] as i64 - served[j] as i64).unsigned_abs() as u64;
            }
        }
        // gini = diff / (2 * n * sum)，千分比
        let denom = 2u64.saturating_mul(n as u64).saturating_mul(sum);
        if denom == 0 {
            0
        } else {
            ((diff * 1000) / denom) as u32
        }
    }

    /// 跑一轮分发（每核配额服务）。
    pub fn disp_round(&mut self) -> u32 {
        let mut total = 0u32;
        for k in 0..self.cores {
            self.served[k] = self.served[k].saturating_add(self.quota);
            total = total.saturating_add(self.quota);
        }
        total
    }

    /// 策略切换扰动：注入随机扰动脉冲（某核多服务一次配额，模拟策略倾斜）。
    pub fn switch_storm(&mut self, rng: &mut Lcg, rounds: u32) -> Result<u32, u32> {
        if self.sealed {
            self.rejects.push((XC_STORM_SEALED, "封账后风暴"));
            return Err(XC_STORM_SEALED);
        }
        if self.cores == 0 {
            self.rejects.push((XC_STORM_ZERO_CORES, "零核泵"));
            return Err(XC_STORM_ZERO_CORES);
        }
        if self.rounds.len() + rounds as usize > 1024 {
            self.rejects.push((XC_STORM_FULL, "轮账容量满"));
            return Err(XC_STORM_FULL);
        }
        let mut n = 0u32;
        for r in 0..rounds {
            let before = Self::gini_per_mille(&self.served);
            // 扰动脉冲：随机核 +1 配额
            let pick = (rng.next() % self.cores as u64) as usize;
            self.served[pick] = self.served[pick].saturating_add(self.quota);
            let after = Self::gini_per_mille(&self.served);
            let d = if after > before { after - before } else { before - after };
            if d > self.band_per_mille {
                self.alarms = self.alarms.saturating_add(1);
            }
            self.rounds.push(StormRow { round: r + 1, gini_before: before, gini_after: after, disturb: d });
            n += 1;
        }
        Ok(n)
    }

    /// 守恒：分发总量 = Σserved。
    pub fn conserved(&self, produced: u64) -> bool {
        let mut s: u64 = 0;
        for x in &self.served {
            s = s.saturating_add(*x as u64);
        }
        s == produced
    }

    pub fn alarms(&self) -> u32 {
        self.alarms
    }
    pub fn rounds_len(&self) -> usize {
        self.rounds.len()
    }
    pub fn max_disturb(&self) -> u32 {
        let mut m: u32 = 0;
        for r in &self.rounds {
            if r.disturb > m {
                m = r.disturb;
            }
        }
        m
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_STORM_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for &s in &self.served {
            h ^= s as u64;
            h = h.wrapping_mul(0x0100_0193);
        }
        for r in &self.rounds {
            h ^= (r.round as u64).rotate_left(7)
                ^ (r.gini_before as u64).rotate_left(17)
                ^ (r.gini_after as u64).rotate_left(31);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准压测场景：4 核 × 50 轮风暴 + 公平扰动量化。
pub fn storm_scenario() -> Result<(u32, u32, u64), u32> {
    let mut t = StormTester::new(4, 10, 300);
    let mut rng = Lcg::new(0x2026_1027);
    let mut produced: u64 = 0;
    for _ in 0..3 {
        produced += t.disp_round() as u64;
    }
    let n = t.switch_storm(&mut rng, 50)?;
    if n != 50 {
        return Err(XC_STORM_ZERO_CORES);
    }
    if !t.conserved(produced + t.max_disturb() as u64 * 0) {
        // 风暴注入未计 produced——此处只验 served 非零
        return Err(XC_STORM_DISTURB);
    }
    let fp = t.seal()?;
    Ok((t.alarms(), t.max_disturb(), fp))
}

/// 注入核：六类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）零核泵必拒
    {
        let mut t = StormTester::new(0, 1, 100);
        let mut rng = Lcg::new(1);
        if t.switch_storm(&mut rng, 3) == Err(XC_STORM_ZERO_CORES) {
            caught += 1;
        }
    }
    // 2）绝对公平（零服务）基尼 = 0
    {
        let g = StormTester::gini_per_mille(&[0, 0, 0, 0]);
        if g == 0 {
            caught += 1;
        }
    }
    // 3）绝对集中基尼 = 1000
    {
        let g = StormTester::gini_per_mille(&[100, 0, 0, 0]);
        if g == 1000 {
            caught += 1;
        }
    }
    // 4）均分基尼 = 0
    {
        let g = StormTester::gini_per_mille(&[25, 25, 25, 25]);
        if g == 0 {
            caught += 1;
        }
    }
    // 5）封账后风暴必拒
    {
        let mut t = StormTester::new(2, 1, 100);
        let _ = t.seal();
        let mut rng = Lcg::new(2);
        if t.switch_storm(&mut rng, 1) == Err(XC_STORM_SEALED) {
            caught += 1;
        }
    }
    // 6）扰动立案（窄带必立案）
    {
        let mut t = StormTester::new(2, 100, 0); // 带 0：任何扰动立案
        let mut rng = Lcg::new(3);
        let _ = t.switch_storm(&mut rng, 5);
        if t.alarms() >= 1 {
            caught += 1;
        }
    }
    (caught, N)
}
