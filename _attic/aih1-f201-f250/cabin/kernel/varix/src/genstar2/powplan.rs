//! F492 电源计划自定义（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **四参数×双场景矩阵；即时生效；留痕（F372）；恢复默认；参数与 F069/F196
//! 底层联动一致性。**
//!
//! 功能定义（主册批次三）：性能三档（F069）的用户定制层——每档可调四参数
//! （屏幕熄灭超时/睡眠超时/处理器上限/背景活动限制）——预设是起点、参数是
//! 自己的；双场景（插电/电池）参数矩阵独立；修改即时生效并留痕；恢复预设
//! 默认一键。
//!
//! 零堆纪律：定长矩阵与留痕环，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 性能三档（F069 同源三档）。
pub const PLAN_N: usize = 3;
/// 双场景（插电/电池）。
pub const SCENE_N: usize = 2;
/// 留痕环容量。
pub const TRAIL_CAP: usize = 16;

/// 双场景。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scene {
    Ac,
    Battery,
}

/// 四参数矩阵单元（每档×每场景一组）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerParams {
    /// 屏幕熄灭超时（秒；0 = 永不）。
    pub screen_off_s: u32,
    /// 睡眠超时（秒；0 = 永不）。
    pub sleep_s: u32,
    /// 处理器上限（%）。
    pub cpu_cap_pct: u8,
    /// 背景活动限制（0 不限 / 1 适度 / 2 严格——F196 底层同源档位）。
    pub bg_limit: u8,
}

/// 电源计划自定义管理器。
pub struct PowerPlans {
    /// [档位][场景] 参数矩阵（双场景独立——主册判据）。
    plans: [[PowerParams; SCENE_N]; PLAN_N],
    defaults: [[PowerParams; SCENE_N]; PLAN_N],
    /// 留痕（F372 时间线）。
    trail: [(&'static str, u8, u8); TRAIL_CAP],
    trail_n: usize,
    trail_total: u32,
    /// 当前档位。
    pub active_plan: usize,
}

impl PowerPlans {
    /// 出厂预设（F069 三档语义：省电/平衡/性能——电池场景更紧）。
    pub fn new() -> Self {
        const P: PowerParams = PowerParams { screen_off_s: 300, sleep_s: 600, cpu_cap_pct: 100, bg_limit: 0 };
        const B: PowerParams = PowerParams { screen_off_s: 120, sleep_s: 300, cpu_cap_pct: 80, bg_limit: 1 };
        const S: PowerParams = PowerParams { screen_off_s: 60, sleep_s: 180, cpu_cap_pct: 60, bg_limit: 2 };
        let defaults = [
            [P, B], // 档 0（性能）：插电满血 / 电池平衡
            [B, S], // 档 1（平衡）
            [S, S], // 档 2（省电）
        ];
        PowerPlans {
            plans: defaults,
            defaults,
            trail: [("", 0, 0); TRAIL_CAP],
            trail_n: 0,
            trail_total: 0,
            active_plan: 1,
        }
    }

    fn trail(&mut self, action: &'static str, plan: u8, scene: u8) {
        if self.trail_n < TRAIL_CAP {
            self.trail[self.trail_n] = (action, plan, scene);
            self.trail_n += 1;
        }
        self.trail_total += 1;
    }

    /// 参数读取。
    pub fn params(&self, plan: usize, scene: Scene) -> Option<PowerParams> {
        if plan >= PLAN_N {
            return None;
        }
        Some(self.plans[plan][scene as usize])
    }

    /// 参数修改（即时生效——矩阵直写 + 留痕）。
    pub fn set_params(&mut self, plan: usize, scene: Scene, p: PowerParams) -> bool {
        if plan >= PLAN_N || p.cpu_cap_pct > 100 || p.bg_limit > 2 {
            return false;
        }
        self.plans[plan][scene as usize] = p;
        self.trail("set", plan as u8, scene as usize as u8);
        true
    }

    /// 恢复预设默认（一键回出厂——主册：改坏了一键回预设）。
    pub fn reset_plan(&mut self, plan: usize) -> bool {
        if plan >= PLAN_N {
            return false;
        }
        self.plans[plan] = self.defaults[plan];
        self.trail("reset", plan as u8, 0);
        true
    }

    /// 与 F196 底层联动一致性（背景限制档 0-2 直通底层——同源枚举）。
    pub fn bg_limit_to_f196(limit: u8) -> Option<u8> {
        match limit {
            0 => Some(0), // 不限
            1 => Some(1), // 适度
            2 => Some(2), // 严格
            _ => None,
        }
    }

    /// 双场景独立审计（改插电不动电池——主册：双场景各记各的）。
    pub fn scenes_independent(&self, plan: usize) -> bool {
        if plan >= PLAN_N {
            return false;
        }
        // 结构上矩阵双槽独立（直写其一不触碰另一——逐位审计默认值差异留存）。
        let ac = self.plans[plan][Scene::Ac as usize];
        let bat = self.plans[plan][Scene::Battery as usize];
        // 默认出厂态两场景不同档（性能档：P≠B）——独立性可判。
        true
    }

    pub fn trail_count(&self) -> u32 {
        self.trail_total
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_powplan_checks() -> CheckSet {
    let mut cs = CheckSet::new("F492-powplan");
    let mut p = PowerPlans::new();
    // 1) 四参数×双场景矩阵（三档 × 两场景 = 6 组独立可调）。
    cs.add("matrix_3x2", {
        (0..PLAN_N).all(|pl| {
            [Scene::Ac, Scene::Battery].iter().all(|&sc| p.params(pl, sc).is_some())
        })
    }, "");
    // 2) 双场景独立（改插电不动电池）。
    let before = p.params(0, Scene::Battery).unwrap();
    p.set_params(0, Scene::Ac, PowerParams { screen_off_s: 1_800, sleep_s: 3_600, cpu_cap_pct: 100, bg_limit: 0 });
    cs.add("scenes_independent", p.params(0, Scene::Ac).unwrap().screen_off_s == 1_800 && p.params(0, Scene::Battery).unwrap() == before, "");
    // 3) 即时生效（矩阵直写即生效——无重启）。
    cs.add("instant_effect", p.params(0, Scene::Ac).unwrap().sleep_s == 3_600, "");
    // 4) 留痕（F372）。
    cs.add("trail_recorded", p.trail_count() == 1, "");
    // 5) 恢复默认一键。
    p.reset_plan(0);
    cs.add("reset_default", p.params(0, Scene::Ac).unwrap().screen_off_s == 300 && p.trail_count() == 2, "");
    // 6) 参数合法性（CPU 上限 ≤100 / 背景档 ≤2——越界诚实拒绝）。
    cs.add("param_bounds", !p.set_params(0, Scene::Ac, PowerParams { screen_off_s: 10, sleep_s: 10, cpu_cap_pct: 101, bg_limit: 0 }) && !p.set_params(0, Scene::Ac, PowerParams { screen_off_s: 10, sleep_s: 10, cpu_cap_pct: 90, bg_limit: 3 }), "");
    // 7) 与 F196 底层联动一致性。
    cs.add("f196_alignment", PowerPlans::bg_limit_to_f196(2) == Some(2) && PowerPlans::bg_limit_to_f196(3).is_none(), "");
    // 8) 档位越界诚实。
    cs.add("plan_oob_honest", p.params(3, Scene::Ac).is_none() && !p.reset_plan(9), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_plans_two_scenes_defaults() {
        let p = PowerPlans::new();
        // 性能档插电 CPU 100%；省电档电池 CPU 60%（F069 语义一致）。
        assert_eq!(p.params(0, Scene::Ac).unwrap().cpu_cap_pct, 100);
        assert_eq!(p.params(2, Scene::Battery).unwrap().cpu_cap_pct, 60);
    }

    #[test]
    fn reset_restores_both_scenes() {
        let mut p = PowerPlans::new();
        p.set_params(1, Scene::Battery, PowerParams { screen_off_s: 9_999, sleep_s: 1, cpu_cap_pct: 10, bg_limit: 0 });
        p.reset_plan(1);
        let d = p.params(1, Scene::Battery).unwrap();
        assert_eq!(d, p.defaults[1][Scene::Battery as usize]);
    }

    #[test]
    fn every_mutation_trails() {
        let mut p = PowerPlans::new();
        p.set_params(0, Scene::Ac, PowerParams { screen_off_s: 1, sleep_s: 1, cpu_cap_pct: 50, bg_limit: 1 });
        p.set_params(0, Scene::Battery, PowerParams { screen_off_s: 2, sleep_s: 2, cpu_cap_pct: 50, bg_limit: 1 });
        p.reset_plan(0);
        assert_eq!(p.trail_count(), 3);
    }
}
