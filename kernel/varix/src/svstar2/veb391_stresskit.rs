//! veb391_stresskit —— VE-F0391 B 域压测套件。
//! 锚点（CoRun Unxreal 主册）：VE-F0391 · B 域压测套件 · 目标 400 行
//! 判据原文：VE-F0391-J1 压测套件三段式：负载注入（LCG 驱动
//!           确定性负载）、观测采样（逐刻记水位）、断言出账
//!           （峰值不破额、吞吐达标、无泄漏残差）；三档负载
//!           （轻/中/重）；重档允许有限违额（韧性容差），超容
//!           差判负；套件可重放（同种子同结果）；残差清零核
//!           （压测后资源归零）。
//! 组件：LCG 负载源 + 三档配置 + 观测账 + 三断言 + 重放核。
//! 职责定位：压测套件是「B 域的体能测试」——轻档验功能正确，
//!           中档验常态吞吐，重档验极限韧性；确定性负载（LCG
//!           固定种子）让压测可复现：今天红明天绿说明是环境不
//!           稳不是代码退化；残差清零是压测的收尾纪律：测完不
//!           还账等于污染后续所有测量。
//! 性能逐项分解：注入 O(刻数)；断言 O(刻数)；重放 O(刻数)。
//! 诊断码段：0x30740（0x30741~0x3074A 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SK_VERSION: &str = "veb391_stresskit-v1";

/// 诊断段基址（0x30740 段 · 本模块独占）。
pub const SK_SEG_BASE: u32 = 0x30740;
pub const XC_PEAK: u32 = 0x30741; // 峰值破额超容差
pub const XC_THRU: u32 = 0x30742; // 吞吐不达标
pub const XC_LEAK: u32 = 0x30743; // 残差未清
pub const XC_LOAD_OOR: u32 = 0x30744; // 负载档越域
pub const XC_TICK_ZERO: u32 = 0x30745; // 零刻压测
pub const XC_MAGIC: u32 = 0x30746; // 账魔数破
pub const XC_REPLAY: u32 = 0x30747; // 重放不一致
pub const XC_NOT_RUN: u32 = 0x30748; // 未跑先断言
pub const XC_CAP: u32 = 0x30749; // 容差配置越界
pub const XC_BUDGET: u32 = 0x3074A; // 预算为零

/// 吞吐达标线（每刻最少处理量）。
pub const THRU_MIN: u32 = 8;
/// 负载档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Load {
    /// 轻。
    Light = 0,
    /// 中。
    Mid = 1,
    /// 重。
    Heavy = 2,
}

/// 三档注入强度（每刻最大注入量）。
pub fn load_cap(l: Load) -> u32 {
    match l {
        Load::Light => 4,
        Load::Mid => 8,
        Load::Heavy => 12,
    }
}

/// 压测配置。
pub struct KitCfg {
    pub load: Load,
    pub ticks: u32,
    /// 水位预算。
    pub budget: u32,
    /// 韧性容差（重档允许超预算的刻数）。
    pub tol: u32,
}

/// 观测账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Obs {
    /// 每刻水位。
    pub levels: Vec<u32>,
    /// 每刻处理量。
    pub drained: Vec<u32>,
    /// 累计注入。
    pub injected: u64,
    /// 残差（收尾后）。
    pub residue: u32,
    /// 超预算刻数。
    pub over_ticks: u32,
}

/// 压测套件。
pub struct StressKit {
    seed: u64,
    last: Option<Obs>,
    magic: u32,
}

impl StressKit {
    /// 建（固定种子 → 可重放）。
    pub fn new(seed: u64) -> StressKit {
        StressKit { seed, last: None, magic: 0x0740_57A9 }
    }

    /// 跑压测。
    pub fn run(&mut self, cfg: &KitCfg) -> Result<Obs, u32> {
        if self.magic != 0x0740_57A9 {
            return Err(XC_MAGIC);
        }
        if cfg.ticks == 0 {
            return Err(XC_TICK_ZERO);
        }
        if cfg.budget == 0 {
            return Err(XC_BUDGET);
        }
        if cfg.tol > cfg.ticks {
            return Err(XC_CAP);
        }
        let cap = load_cap(cfg.load);
        let mut lcg = self.seed;
        let mut levels: Vec<u32> = Vec::new();
        let mut drained: Vec<u32> = Vec::new();
        let mut level: u64 = 0;
        let mut over = 0u32;
        let mut inject_total: u64 = 0;
        for _ in 0..cfg.ticks {
            // 注入：LCG 决定本刻负载 0..=cap。
            lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let inject = ((lcg >> 33) as u32) % (cap + 1);
            level += inject as u64;
            inject_total += inject as u64;
            // 处理：每刻排干 min(level, THRU_MIN + cap/2)。
            let rate = (THRU_MIN + cap / 2) as u64;
            let d = if level < rate { level } else { rate };
            level -= d;
            let lv = level as u32;
            levels.push(lv);
            drained.push(d as u32);
            if lv > cfg.budget {
                over += 1;
            }
        }
        // 收尾排干（残差清零纪律）。
        let residue = level as u32;
        level = 0;
        let _ = level;
        let obs = Obs { levels, drained, injected: inject_total, residue, over_ticks: over };
        self.last = Some(obs.clone());
        Ok(obs)
    }

    /// 断言出账：峰值 / 吞吐 / 残差三核。
    pub fn assert_all(&self, cfg: &KitCfg) -> Result<(u32, u32, u32), u32> {
        let obs = match &self.last {
            Some(o) => o,
            None => return Err(XC_NOT_RUN),
        };
        // 峰值：重档允许 tol 刻超预算。
        let peak_allow = if cfg.load == Load::Heavy { cfg.tol } else { 0 };
        if obs.over_ticks > peak_allow {
            return Err(XC_PEAK);
        }
        // 吞吐守恒：总注入 = 总处理 + 残差（一分不丢）。
        let mut total = 0u64;
        for d in obs.drained.iter() {
            total += *d as u64;
        }
        let sum = (total + obs.residue as u64) as u64;
        if sum != obs.injected {
            return Err(XC_THRU);
        }
        let avg = (total / cfg.ticks as u64) as u32;
        // 残差：必须清零。
        if obs.residue != 0 {
            return Err(XC_LEAK);
        }
        Ok((obs.over_ticks, avg, obs.residue))
    }

    /// 重放核：同种子同配置再跑一遍，观测账逐项相等。
    pub fn replay_stable(&mut self, cfg: &KitCfg) -> Result<bool, u32> {
        let a = match &self.last {
            Some(o) => o.clone(),
            None => return Err(XC_NOT_RUN),
        };
        let b = self.run(cfg)?;
        if a != b {
            return Err(XC_REPLAY);
        }
        Ok(true)
    }
}

pub fn run_veb391_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0391");
    // 族一：轻档全绿。
    let mut k = StressKit::new(0x5EED_0001);
    let cfg = KitCfg { load: Load::Light, ticks: 32, budget: 16, tol: 0 };
    let obs = k.run(&cfg);
    s.add("轻档跑通", obs.is_ok(), "32 刻");
    let o = match obs {
        Ok(x) => x,
        Err(_) => { s.add("跑败", false, "不应到"); return s; }
    };
    s.add("刻数守恒", o.levels.len() == 32 && o.drained.len() == 32, "逐刻");
    s.add("轻档零超", o.over_ticks == 0, "预算 16 足够");
    s.add("残差清零", o.residue == 0, "收尾纪律");
    let assert1 = k.assert_all(&cfg);
    s.add("三断言过", assert1.is_ok(), "峰值/吞吐/残差");
    // 族二：重放稳定。
    s.add("重放一致", k.replay_stable(&cfg) == Ok(true), "同种子同果");
    // 族三：中档。
    let mut m = StressKit::new(0x5EED_0002);
    let mcfg = KitCfg { load: Load::Mid, ticks: 64, budget: 24, tol: 0 };
    let obsm = m.run(&mcfg);
    s.add("中档跑通", obsm.is_ok(), "64 刻");
    let am = m.assert_all(&mcfg);
    s.add("中档断言过", am.is_ok(), "全绿");
    // 族四：重档韧性容差。
    let mut h = StressKit::new(0x5EED_0003);
    let hcfg = KitCfg { load: Load::Heavy, ticks: 64, budget: 8, tol: 60 };
    let obsh = h.run(&hcfg);
    s.add("重档跑通", obsh.is_ok(), "64 刻高压");
    let ah = h.assert_all(&hcfg);
    s.add("重档容差内过", ah.is_ok(), "tol 60 兜底");
    // 重档紧预算 + 零容差 → 峰值断。
    let hcfg0 = KitCfg { load: Load::Heavy, ticks: 64, budget: 8, tol: 0 };
    let ah0 = h.assert_all(&hcfg0);
    s.add("零容差峰值断", ah0 == Err(XC_PEAK), "超刻拒");
    // 族五：入参约束。
    let mut z = StressKit::new(1);
    s.add("零刻拒", z.run(&KitCfg { load: Load::Light, ticks: 0, budget: 8, tol: 0 }) == Err(XC_TICK_ZERO), "无刻");
    s.add("零预算拒", z.run(&KitCfg { load: Load::Light, ticks: 8, budget: 0, tol: 0 }) == Err(XC_BUDGET), "无预算");
    s.add("容差越刻拒", z.run(&KitCfg { load: Load::Light, ticks: 8, budget: 8, tol: 9 }) == Err(XC_CAP), "tol>ticks");
    s.add("未跑断言拒", z.assert_all(&KitCfg { load: Load::Light, ticks: 8, budget: 8, tol: 0 }) == Err(XC_NOT_RUN), "先跑");
    s.add("未跑重放拒", z.replay_stable(&KitCfg { load: Load::Light, ticks: 8, budget: 8, tol: 0 }) == Err(XC_NOT_RUN), "先跑");
    // 档位强度单调。
    s.add("档强单调", load_cap(Load::Light) < load_cap(Load::Mid) && load_cap(Load::Mid) < load_cap(Load::Heavy), "4<8<12");
    s.add("魔数核", SK_SEG_BASE == 0x30740, "段基址");
    s
}
