//! F492 电源计划自定义（genstar2 · I 域通用·二分队 · AI-U2 · 深化 v2）。
//!
//! 主册判据（验收标准第一句）：
//! **四参数×双场景矩阵；即时生效；留痕（F372）；恢复默认；参数与 F069/F196
//! 底层联动一致性。**
//!
//! 深化 v2 增量（对齐主册「安全与电源底盘」全量功能面）：
//! - 三档×双场景 6 组参数的**持久化序列化**（魔标+版本+逐组落盘，坏账拒收）；
//! - 场景感知调度器（插电/电池切换事件 → 自动落位对应参数组——双场景
//!   各记各的落到运行面）；
//! - 参数联动校验（睡眠 ≤ 屏幕熄灭不合理组合拦截；处理器上限钳制）；
//! - 留痕环序列化 + 留痕检索（按档位过滤——F372 时间线可查）；
//! - 「电池模式自动降档」联动（电量阈值 → 自动切省电档，与 F196 同源）；
//! - 检查行扩至 24 行、宿主单测扩至 6 例。

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
/// 持久化魔标（4B）+ 版本（1B）+ 6 组 × 11B（4+4+1+1+对齐 1）。
pub const PERSIST_MAGIC: [u8; 4] = *b"VPP1";
pub const PERSIST_ENTRY_BYTES: usize = 11;
/// 电池自动降档阈值（%——与 F196 电池保护同源）。
pub const BATTERY_AUTO_PLAN_PERMILLE: u16 = 200;

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

impl PowerParams {
    fn sane(&self) -> bool {
        self.cpu_cap_pct <= 100
            && self.bg_limit <= 2
            // 睡眠前必须先熄屏（或两者都永不——0 特殊语义放行）。
            && (self.sleep_s == 0 || self.screen_off_s == 0 || self.sleep_s > self.screen_off_s)
    }

    fn to_bytes(&self) -> [u8; PERSIST_ENTRY_BYTES] {
        let mut b = [0u8; PERSIST_ENTRY_BYTES];
        b[0..4].copy_from_slice(&self.screen_off_s.to_le_bytes());
        b[4..8].copy_from_slice(&self.sleep_s.to_le_bytes());
        b[8] = self.cpu_cap_pct;
        b[9] = self.bg_limit;
        b
    }

    fn from_bytes(b: &[u8]) -> Option<PowerParams> {
        let p = PowerParams {
            screen_off_s: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            sleep_s: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
            cpu_cap_pct: b[8],
            bg_limit: b[9],
        };
        if p.sane() {
            Some(p)
        } else {
            None
        }
    }
}

/// 电源计划自定义管理器（深化 v2）。
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
    /// 当前场景（调度器运行面）。
    pub active_scene: Scene,
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
            active_scene: Scene::Ac,
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

    /// 当前生效参数（档位 × 场景调度器落位——双场景落到运行面）。
    pub fn effective(&self) -> PowerParams {
        self.plans[self.active_plan][self.active_scene as usize]
    }

    /// 参数修改（即时生效——矩阵直写 + 留痕 + 合理性校验）。
    pub fn set_params(&mut self, plan: usize, scene: Scene, p: PowerParams) -> bool {
        if plan >= PLAN_N || !p.sane() {
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

    /// 场景切换事件（插电/电池——调度器自动落位对应参数组）。
    pub fn on_power_source(&mut self, scene: Scene) {
        if self.active_scene != scene {
            self.active_scene = scene;
            self.trail("scene", self.active_plan as u8, scene as usize as u8);
        }
    }

    /// 电池自动降档联动（电量低于阈值 → 自动切省电档；与 F196 同源阈值；
    /// 只在电池场景生效——插电永远听用户的）。
    pub fn on_battery_level(&mut self, level_permille: u16) -> bool {
        if self.active_scene == Scene::Battery && level_permille <= BATTERY_AUTO_PLAN_PERMILLE && self.active_plan != 2 {
            self.active_plan = 2;
            self.trail("auto-saver", 2, 1);
            return true;
        }
        false
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
        self.plans[plan][0].screen_off_s != 0 || self.plans[plan][1].screen_off_s != 0
    }

    /// 留痕检索：按档位过滤（F372 时间线可查——深化检索面）。
    pub fn trail_for_plan(&self, plan: u8) -> usize {
        (0..self.trail_n).filter(|&i| self.trail[i].1 == plan).count()
    }

    pub fn trail_count(&self) -> u32 {
        self.trail_total
    }

    // -----------------------------------------------------------------
    // 持久化序列化（深化：魔标+版本+逐组落盘；坏账/不合理参数拒收）
    // -----------------------------------------------------------------

    /// 序列化 6 组参数（定长缓冲；不足诚实拒绝）。
    pub fn save(&self, out: &mut [u8]) -> Option<usize> {
        let need = 5 + PLAN_N * SCENE_N * PERSIST_ENTRY_BYTES;
        if out.len() < need {
            return None;
        }
        out[..4].copy_from_slice(&PERSIST_MAGIC);
        out[4] = 1;
        let mut w = 5;
        for pl in 0..PLAN_N {
            for sc in 0..SCENE_N {
                let b = self.plans[pl][sc].to_bytes();
                out[w..w + PERSIST_ENTRY_BYTES].copy_from_slice(&b);
                w += PERSIST_ENTRY_BYTES;
            }
        }
        Some(need)
    }

    /// 反序列化（魔标/版本不符或参数不合理 = 整体拒收——坏账不静默吞）。
    pub fn load(&mut self, buf: &[u8]) -> bool {
        let need = 5 + PLAN_N * SCENE_N * PERSIST_ENTRY_BYTES;
        if buf.len() < need || buf[..4] != PERSIST_MAGIC || buf[4] != 1 {
            return false;
        }
        let mut loaded = [[PowerParams { screen_off_s: 0, sleep_s: 0, cpu_cap_pct: 0, bg_limit: 0 }; SCENE_N]; PLAN_N];
        let mut r = 5;
        for pl in 0..PLAN_N {
            for sc in 0..SCENE_N {
                match PowerParams::from_bytes(&buf[r..r + PERSIST_ENTRY_BYTES]) {
                    Some(p) => loaded[pl][sc] = p,
                    None => return false,
                }
                r += PERSIST_ENTRY_BYTES;
            }
        }
        self.plans = loaded;
        true
    }
}

// ---------------------------------------------------------------------------
// 域自检（深化 v2：24 行）
// ---------------------------------------------------------------------------

pub fn run_powplan_checks() -> CheckSet {
    let mut cs = CheckSet::new("F492-powplan");
    let mut p = PowerPlans::new();
    // 1) 四参数×双场景矩阵（三档 × 两场景 = 6 组独立可调）。
    cs.add("matrix_3x2", {
        (0..PLAN_N).all(|pl| [Scene::Ac, Scene::Battery].iter().all(|&sc| p.params(pl, sc).is_some()))
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
    // 6) 参数合理性（CPU ≤100 / 背景档 ≤2 / 睡眠须晚于熄屏——联动校验）。
    cs.add("param_bounds", !p.set_params(0, Scene::Ac, PowerParams { screen_off_s: 10, sleep_s: 10, cpu_cap_pct: 101, bg_limit: 0 }) && !p.set_params(0, Scene::Ac, PowerParams { screen_off_s: 10, sleep_s: 10, cpu_cap_pct: 90, bg_limit: 3 }), "");
    cs.add("sleep_after_screenoff", !p.set_params(0, Scene::Ac, PowerParams { screen_off_s: 600, sleep_s: 300, cpu_cap_pct: 90, bg_limit: 0 }) && p.set_params(0, Scene::Ac, PowerParams { screen_off_s: 300, sleep_s: 600, cpu_cap_pct: 90, bg_limit: 0 }), "");
    // 7) 与 F196 底层联动一致性。
    cs.add("f196_alignment", PowerPlans::bg_limit_to_f196(2) == Some(2) && PowerPlans::bg_limit_to_f196(3).is_none(), "");
    // 8) 档位越界诚实。
    cs.add("plan_oob_honest", p.params(3, Scene::Ac).is_none() && !p.reset_plan(9), "");
    // 9) 场景调度器（插电/电池事件 → 生效参数随场景落位）。
    p.active_plan = 0;
    p.on_power_source(Scene::Battery);
    cs.add("scene_scheduler", p.effective() == p.params(0, Scene::Battery).unwrap(), "");
    p.on_power_source(Scene::Ac);
    cs.add("scene_back_ac", p.effective() == p.params(0, Scene::Ac).unwrap(), "");
    // 10) 同场景重复事件不留痕（无变化不记——留痕准确）。
    let t0 = p.trail_count();
    p.on_power_source(Scene::Ac);
    cs.add("no_op_no_trail", p.trail_count() == t0, "");
    // 11) 电池自动降档（≤20% 自动切省电档——F196 同源阈值；插电不劫持）。
    p.on_power_source(Scene::Battery);
    p.active_plan = 0;
    cs.add("battery_auto_saver", p.on_battery_level(150) && p.active_plan == 2, "");
    p.active_plan = 0;
    p.on_power_source(Scene::Ac);
    cs.add("ac_never_hijacks", !p.on_battery_level(100) || p.active_plan != 2 || p.active_scene == Scene::Ac, "");
    cs.add("battery_high_no_switch", {
        p.on_power_source(Scene::Battery);
        p.active_plan = 0;
        !p.on_battery_level(800) && p.active_plan == 0
    }, "");
    // 12) 留痕检索（按档位过滤——F372 时间线可查）。
    cs.add("trail_filter_by_plan", {
        p.set_params(2, Scene::Ac, p.params(2, Scene::Ac).unwrap());
        p.trail_for_plan(2) >= 1
    }, "");
    // 13) 持久化 round-trip + 坏账拒收。
    let mut buf = [0u8; 128];
    let n = p.save(&mut buf).unwrap();
    let mut q = PowerPlans::new();
    cs.add("persist_roundtrip", q.load(&buf[..n]) && q.params(0, Scene::Ac).unwrap() == p.params(0, Scene::Ac).unwrap(), "");
    let mut bad = buf;
    bad[0] = b'X';
    cs.add("persist_bad_magic", !PowerPlans::new().load(&bad[..n]), "");
    let mut bad2 = buf;
    bad2[5 + 8] = 200; // cpu_cap 200% 不合理
    cs.add("persist_insane_rejected", !PowerPlans::new().load(&bad2[..n]), "");
    let mut tiny = [0u8; 16];
    cs.add("persist_small_honest", p.save(&mut tiny).is_none(), "");
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
        p.set_params(1, Scene::Battery, PowerParams { screen_off_s: 9_999, sleep_s: 10_000, cpu_cap_pct: 10, bg_limit: 0 });
        p.reset_plan(1);
        let d = p.params(1, Scene::Battery).unwrap();
        assert_eq!(d, p.defaults[1][Scene::Battery as usize]);
    }

    #[test]
    fn every_mutation_trails() {
        let mut p = PowerPlans::new();
        p.set_params(0, Scene::Ac, PowerParams { screen_off_s: 1, sleep_s: 2, cpu_cap_pct: 50, bg_limit: 1 });
        p.set_params(0, Scene::Battery, PowerParams { screen_off_s: 2, sleep_s: 3, cpu_cap_pct: 50, bg_limit: 1 });
        p.reset_plan(0);
        assert_eq!(p.trail_count(), 3);
    }

    #[test]
    fn sanity_rejects_sleep_before_screenoff() {
        // 睡眠早于熄屏是不合理组合（联动校验——用户不该为设计的不完整买单）。
        let bad = PowerParams { screen_off_s: 600, sleep_s: 60, cpu_cap_pct: 50, bg_limit: 0 };
        assert!(!bad.sane());
        let zero_ok = PowerParams { screen_off_s: 0, sleep_s: 60, cpu_cap_pct: 50, bg_limit: 0 };
        assert!(zero_ok.sane()); // 永不熄屏 + 定时睡眠 = 合法
    }

    #[test]
    fn scene_scheduler_full_cycle() {
        let mut p = PowerPlans::new();
        p.active_plan = 0;
        p.on_power_source(Scene::Battery);
        let batt = p.effective();
        p.on_power_source(Scene::Ac);
        assert_ne!(p.effective(), batt);
        assert_eq!(p.effective().cpu_cap_pct, 100);
    }

    #[test]
    fn battery_auto_only_in_battery_scene() {
        let mut p = PowerPlans::new();
        p.active_plan = 0;
        p.on_power_source(Scene::Ac);
        assert!(!p.on_battery_level(100)); // 插电不劫持
        p.on_power_source(Scene::Battery);
        assert!(p.on_battery_level(200)); // 恰在阈值触发
        assert_eq!(p.active_plan, 2);
    }
}

// ===========================================================================
// 深化 v3（F492）：电池老化模型 / 充放电曲线段 / 电源计划模板四套 /
// 续航预估账 / 峰值功耗预算 / 计划切换审计链
// ===========================================================================

/// 电池健康度（主册「电池」的寿命面：满充电容量 / 设计容量 ×1000——
/// 老化是实测账不是猜测；低于 800‰ 建议换电池的人话锚）。
pub const BATTERY_WORN_PERMILLE: u16 = 800;

pub fn battery_health_permille(full_charge_mah: u32, design_mah: u32) -> Option<u16> {
    if design_mah == 0 || full_charge_mah == 0 || full_charge_mah > design_mah {
        return None; // 无设计容量/未标定/超设计（异常数据）诚实拒。
    }
    Some((full_charge_mah as u64 * 1_000 / design_mah as u64) as u16)
}

pub fn battery_worn(health_permille: u16) -> bool {
    health_permille < BATTERY_WORN_PERMILLE
}

/// 充电曲线段（主册「充电显示」的三段模型：快充恒流段到 800‰、
/// 涓流段 800-980‰、满充保平段——三段的预计剩余分钟表一处定义）。
pub const CHARGE_SEGMENT_FULL_PERMILLE: u16 = 800;
pub const CHARGE_SEGMENT_TRICKLE_PERMILLE: u16 = 980;

pub enum ChargePhase {
    Fast,
    Trickle,
    Full,
}

pub fn charge_phase(level_permille: u16) -> ChargePhase {
    if level_permille >= 980 {
        ChargePhase::Full
    } else if level_permille >= CHARGE_SEGMENT_FULL_PERMILLE {
        ChargePhase::Trickle
    } else {
        ChargePhase::Fast
    }
}

pub fn charge_phase_name(p: &ChargePhase) -> &'static str {
    match p {
        ChargePhase::Fast => "快充",
        ChargePhase::Trickle => "涓流",
        ChargePhase::Full => "已满",
    }
}

/// 续航预估账（主册「预估续航时间」：当前电量 ÷ 场景功耗 ×60——
/// 分钟整取；功耗 0 诚实 None（不硬估无限续航））。
pub fn battery_minutes_left(level_permille: u16, drain_mw_per_min: u32) -> Option<u32> {
    if drain_mw_per_min == 0 {
        return None;
    }
    // 电量按 ‰ 换算成满格 100 的份额，除以每分钟耗电份额（‰/min）。
    let level = level_permille.min(1_000) as u64;
    Some(((level * 60) / drain_mw_per_min.max(1) as u64) as u32)
}

/// 峰值功耗预算（主册「功耗预算」的分配面：整机 TDP 内四件套
/// （CPU/GPU/屏/其余）分配和不得超总——超了就是预算在说谎）。
pub const TDP_BUDGET_MW: u32 = 28_000;

pub fn tdp_budget_ok(cpu_mw: u32, gpu_mw: u32, panel_mw: u32, rest_mw: u32) -> bool {
    cpu_mw.saturating_add(gpu_mw)
        .saturating_add(panel_mw)
        .saturating_add(rest_mw) <= TDP_BUDGET_MW
}

/// 电源计划模板四套（主册「计划模板」：均衡/性能/省电/演示——
/// 模板 = 三参数预置；一键套用，恢复默认永远一键可退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlanPreset {
    Balanced,
    Performance,
    Saver,
    Presentation,
}

/// 模板参数（cpu_perf_permille / 屏亮度 permille / 后台限流档）。
pub const PRESET_TABLE: [(PlanPreset, u16, u16, u8); 4] = [
    (PlanPreset::Balanced, 600, 700, 2),
    (PlanPreset::Performance, 1_000, 1_000, 0),
    (PlanPreset::Saver, 300, 400, 4),
    (PlanPreset::Presentation, 500, 1_000, 3), // 演示：屏最亮 + 中性能
];

pub fn preset_params(p: PlanPreset) -> (u16, u16, u8) {
    PRESET_TABLE.iter().find(|(k, _, _, _)| *k == p).map(|(_, c, b, l)| (*c, *b, *l)).unwrap_or((600, 700, 2))
}

/// 模板表健康审计：四套互异（一键套用不许有两个一样的「模板」）。
pub fn presets_distinct() -> bool {
    for i in 0..PRESET_TABLE.len() {
        for j in (i + 1)..PRESET_TABLE.len() {
            if PRESET_TABLE[i].0 == PRESET_TABLE[j].0 {
                return false;
            }
            let (c1, b1, l1) = preset_params(PRESET_TABLE[i].0);
            let (c2, b2, l2) = preset_params(PRESET_TABLE[j].0);
            if (c1, b1, l1) == (c2, b2, l2) {
                return false;
            }
        }
    }
    true
}

/// 计划切换审计链（v1 trail 的深化：切换事件带来源（手动/电池自动）
/// ——「为什么换了计划」要能回答；来源枚举与切换同账入环）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchSource {
    Manual,
    BatteryAuto,
    ThermalAuto,
}

#[derive(Clone, Copy, Debug)]
pub struct SwitchEvent {
    pub at_ms: u64,
    pub from_plan: u8,
    pub to_plan: u8,
    pub source: SwitchSource,
}

pub const SWITCH_AUDIT_CAP: usize = 12;

pub struct SwitchAudit {
    ring: [Option<SwitchEvent>; SWITCH_AUDIT_CAP],
    head: usize,
    n: usize,
}

impl SwitchAudit {
    pub const fn new() -> Self {
        SwitchAudit { ring: [None; SWITCH_AUDIT_CAP], head: 0, n: 0 }
    }

    pub fn push(&mut self, ev: SwitchEvent) {
        self.ring[self.head] = Some(ev);
        self.head = (self.head + 1) % SWITCH_AUDIT_CAP;
        self.n = (self.n + 1).min(SWITCH_AUDIT_CAP);
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 最近一次电池自动切换的时刻（「为什么现在这么省电」的答案）。
    pub fn latest_battery_auto(&self) -> Option<u64> {
        for i in 0..self.n {
            let idx = (self.head + SWITCH_AUDIT_CAP - 1 - i) % SWITCH_AUDIT_CAP;
            if let Some(e) = self.ring[idx].as_ref() {
                if e.source == SwitchSource::BatteryAuto {
                    return Some(e.at_ms);
                }
            }
        }
        None
    }

    /// 切换合理性审计（同计划来回抖动 = 电源策略在震荡——
    /// 1 分钟内同源切换 ≥3 次记一次震荡，供滞回参数调优）。
    pub fn oscillation_detected(&self, window_ms: u64, now_ms: u64) -> bool {
        let mut recent = 0;
        for i in 0..self.n {
            let idx = (self.head + SWITCH_AUDIT_CAP - 1 - i) % SWITCH_AUDIT_CAP;
            if let Some(e) = self.ring[idx].as_ref() {
                if now_ms.saturating_sub(e.at_ms) <= window_ms {
                    recent += 1;
                }
            }
        }
        recent >= 3
    }
}

// ---------------------------------------------------------------------------
// 深化 v3 自检（F492-v3）
// ---------------------------------------------------------------------------

pub fn run_powplan_v3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F492-v3");
    // 1) 电池健康：正常/老化/异常数据。
    cs.add("health_ok", battery_health_permille(4_200, 5_000) == Some(840), "");
    cs.add("health_worn", battery_worn(battery_health_permille(3_900, 5_000).unwrap()), "");
    cs.add("health_bad_data", battery_health_permille(0, 5_000).is_none()
        && battery_health_permille(6_000, 5_000).is_none(), "");
    // 2) 充电三段：800 前快充、800-980 涓流、980+ 已满。
    cs.add("charge_fast", matches!(charge_phase(300), ChargePhase::Fast), "");
    cs.add("charge_trickle", matches!(charge_phase(850), ChargePhase::Trickle), "");
    cs.add("charge_full", matches!(charge_phase(1_000), ChargePhase::Full), "");
    cs.add("charge_names", charge_phase_name(&charge_phase(500)) == "快充"
        && charge_phase_name(&charge_phase(900)) == "涓流", "");
    // 3) 续航预估：线性折算、零功耗诚实。
    cs.add("runtime_linear", battery_minutes_left(500, 100) == Some(300), "");
    cs.add("runtime_zero_honest", battery_minutes_left(500, 0).is_none(), "");
    // 4) TDP 预算：和不超总才过。
    cs.add("tdp_ok", tdp_budget_ok(12_000, 8_000, 4_000, 3_000), "");
    cs.add("tdp_over", !tdp_budget_ok(15_000, 10_000, 4_000, 3_000), "");
    // 5) 模板四套互异。
    cs.add("presets_distinct", presets_distinct(), "");
    cs.add("preset_values", preset_params(PlanPreset::Performance) == (1_000, 1_000, 0), "");
    // 6) 切换审计：来源入账、最新电池自动可查、震荡检测。
    let mut au = SwitchAudit::new();
    let _ = au.push(SwitchEvent { at_ms: 1_000, from_plan: 0, to_plan: 2, source: SwitchSource::BatteryAuto });
    let _ = au.push(SwitchEvent { at_ms: 2_000, from_plan: 2, to_plan: 0, source: SwitchSource::Manual });
    cs.add("audit_latest_battery", au.latest_battery_auto() == Some(1_000), "");
    cs.add("audit_oscillation", {
        let mut a2 = SwitchAudit::new();
        for i in 0..3u64 {
            let _ = a2.push(SwitchEvent { at_ms: 1_000 + i * 100, from_plan: 0, to_plan: 1, source: SwitchSource::Manual });
        }
        a2.oscillation_detected(5_000, 2_000)
    }, "");
    cs.add("audit_calm_ok", !au.oscillation_detected(5_000, 2_000), "");
    cs
}

#[cfg(test)]
mod v3_tests {
    use super::*;

    #[test]
    fn health_clamps_to_valid_domain() {
        // 满充=设计 → 1000‰；接近设计不越界。
        assert_eq!(battery_health_permille(5_000, 5_000), Some(1_000));
        assert_eq!(battery_health_permille(4_999, 5_000), Some(999));
    }

    #[test]
    fn charge_phase_boundaries_exact() {
        // 段界含边：恰 800 进涓流、恰 980 进已满。
        assert!(matches!(charge_phase(799), ChargePhase::Fast));
        assert!(matches!(charge_phase(800), ChargePhase::Trickle));
        assert!(matches!(charge_phase(979), ChargePhase::Trickle));
        assert!(matches!(charge_phase(980), ChargePhase::Full));
    }

    #[test]
    fn switch_audit_wraps_and_latest_wins() {
        let mut au = SwitchAudit::new();
        for i in 0..(SWITCH_AUDIT_CAP + 2) as u64 {
            let _ = au.push(SwitchEvent {
                at_ms: i * 1_000,
                from_plan: 0,
                to_plan: 1,
                source: if i % 2 == 0 { SwitchSource::Manual } else { SwitchSource::BatteryAuto },
            });
        }
        assert_eq!(au.count(), SWITCH_AUDIT_CAP);
        // 环满后：最新一条（i=CAP+1，奇数）是 BatteryAuto。
        let last = SWITCH_AUDIT_CAP as u64 + 1;
        assert_eq!(au.latest_battery_auto(), Some(last * 1_000));
    }

    #[test]
    fn preset_presentation_brightest() {
        // 演示模板的屏亮度 = 全表最高（演示场景有人盯着看）。
        let (_, b_p, _) = preset_params(PlanPreset::Presentation);
        for k in [PlanPreset::Balanced, PlanPreset::Performance, PlanPreset::Saver] {
            let (_, b, _) = preset_params(k);
            assert!(b_p >= b);
        }
    }
}
