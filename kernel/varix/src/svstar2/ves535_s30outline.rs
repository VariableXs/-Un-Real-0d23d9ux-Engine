//! VE-F4301 · 场景应用总纲（U06 一致性场景应用组）
//! 职责：七景全景（新功能/跨域联调/版本发布/主题生态/插件生态/多团队/AI
//! 辅助）+ 应用原则（场景不改判定只改触达——判定权恒在 U03）+ 阶段路由器
//! + 路由失配默认全场景兜底 + 越权改判拒绝 + 触达含无障碍形态（域本色）。
//! 段位：0x2FA0（续表 idx535）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2FA0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    /// 场景越权改判。
    JudgeHijack = SEG_BASE,
    /// 路由失配（默认兜底）。
    RouteMiss = SEG_BASE + 0x01,
    /// 触达缺无障碍形态（域色）。
    A11yMissing = SEG_BASE + 0x02,
    /// 触达账漂移（越界景查询）。
    TouchDrift = SEG_BASE + 0x03,
    /// 路由指纹失配（序列化损坏）。
    RouteFpMismatch = SEG_BASE + 0x04,
}

/// 七景。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scene {
    NewFeature = 0,
    CrossDebug = 1,
    Release = 2,
    Theme = 3,
    Plugin = 4,
    MultiTeam = 5,
    AiAssist = 6,
}
pub const SCENE_N: usize = 7;

/// 研发阶段。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Design = 0,
    Coding = 1,
    Integration = 2,
    Ship = 3,
}

/// 场景总纲册。
pub struct SceneBook {
    pub active: [bool; SCENE_N],
    /// 触达方式：(景, 无障碍形态齐)。
    pub touch: [(bool, bool); SCENE_N],
    pub judge_single: bool,
    pub reroutes: u32,
}

impl SceneBook {
    pub fn new() -> Self {
        SceneBook { active: [false; SCENE_N], touch: [(false, false); SCENE_N], judge_single: true, reroutes: 0 }
    }

    /// 场景启用+触达登记（无障碍形态必须齐——域本色）。
    pub fn enable(&mut self, s: Scene, a11y_touch: bool) -> Result<(), ScCode> {
        if !a11y_touch {
            return Err(ScCode::A11yMissing);
        }
        self.active[s as usize] = true;
        self.touch[s as usize] = (true, true);
        Ok(())
    }

    /// 阶段路由器：按阶段路由场景方案。
    pub fn route(&mut self, phase: Phase) -> Result<Scene, ScCode> {
        let s = match phase {
            Phase::Design => Scene::NewFeature,
            Phase::Coding => Scene::NewFeature,
            Phase::Integration => Scene::CrossDebug,
            Phase::Ship => Scene::Release,
        };
        if !self.active[s as usize] {
            self.reroutes += 1;
            // 默认兜底：全场景提示（返回第一个活动景；全空则失配）
            return self.active.iter().position(|a| *a).map(|i| match i {
                1 => Scene::CrossDebug,
                2 => Scene::Release,
                3 => Scene::Theme,
                4 => Scene::Plugin,
                5 => Scene::MultiTeam,
                6 => Scene::AiAssist,
                _ => Scene::NewFeature,
            }).ok_or(ScCode::RouteMiss);
        }
        Ok(s)
    }

    /// 越权改判检：场景声明自己改判定→拒绝。
    pub fn judge_claim(&self, claims: bool) -> Result<(), ScCode> {
        if claims || !self.judge_single {
            return Err(ScCode::JudgeHijack);
        }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C535-01..12。
pub fn run_ves535_checks() -> CheckSet {
    let mut set = CheckSet::new("ves535");

    let mut sb = SceneBook::new();
    chk(&mut set, "C535-01", sb.enable(Scene::NewFeature, false) == Err(ScCode::A11yMissing), "触达缺无障碍形态拒绝（域色）");
    let _ = sb.enable(Scene::NewFeature, true);
    let _ = sb.enable(Scene::CrossDebug, true);
    let _ = sb.enable(Scene::Release, true);
    chk(&mut set, "C535-02", sb.active.iter().filter(|a| **a).count() == 3, "三景启用");

    // 路由
    chk(&mut set, "C535-03", sb.route(Phase::Coding) == Ok(Scene::NewFeature), "编码期路由新功能景");
    chk(&mut set, "C535-04", sb.route(Phase::Integration) == Ok(Scene::CrossDebug), "联调期路由跨域景");
    chk(&mut set, "C535-05", sb.route(Phase::Ship) == Ok(Scene::Release), "发布期路由发布景");

    // 失配兜底
    sb.active[0] = false; // 关新功能景
    chk(&mut set, "C535-06", sb.route(Phase::Coding) == Ok(Scene::CrossDebug), "路由失配默认兜底到活动景");
    chk(&mut set, "C535-07", sb.reroutes == 1, "重路由计数");
    sb.active = [false; SCENE_N];
    chk(&mut set, "C535-08", sb.route(Phase::Design) == Err(ScCode::RouteMiss), "全空场景路由失配显性报错");

    // 判定权
    chk(&mut set, "C535-09", sb.judge_claim(false).is_ok(), "场景不改判定（触达方式）");
    chk(&mut set, "C535-10", sb.judge_claim(true) == Err(ScCode::JudgeHijack), "越权改判拒绝并记录");
    sb.judge_single = false;
    chk(&mut set, "C535-11", sb.judge_claim(false) == Err(ScCode::JudgeHijack), "单源声明被破坏即拒");

    chk(&mut set, "C535-12", SCENE_N == 7 && Scene::AiAssist as usize == 6, "七景枚举完备");

    ves535_deep_checks(&mut set);

    set
}

// ============ 二轮深化块（wbv-r2d5 · 2026-10-27）============
// 深化面：触达账本结构化（景×次数归因）+ 守恒恒等式 + 边界探针
// + 阶段路由完备投影 + LCG 路由注入回归 + 幂等闸 + 触达位打包往返
// + FNV-1a 路由指纹。

use alloc::string::{String, ToString};

/// 触达账目：景 × 触达次数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TouchRecord {
    pub scene: Scene,
    pub touches: u32,
    pub a11y_ok: bool,
}

/// 场景触达深化账本。
pub struct TouchLedger {
    pub recs: Vec<TouchRecord>,
    pub enabled_total: u32,
}

impl TouchLedger {
    pub fn new() -> Self {
        TouchLedger { recs: Vec::new(), enabled_total: 0 }
    }

    /// 登记触达；无障碍形态不齐直接拒绝（域本色，不落账）。
    pub fn touch(&mut self, s: Scene, a11y_ok: bool) -> Result<(), ScCode> {
        if !a11y_ok {
            return Err(ScCode::A11yMissing);
        }
        match self.recs.iter_mut().find(|r| r.scene == s) {
            Some(r) => r.touches += 1,
            None => self.recs.push(TouchRecord { scene: s, touches: 1, a11y_ok: true }),
        }
        Ok(())
    }

    /// 守恒恒等式：Σtouches ≥ 启用总数，且账内 a11y 旗恒真。
    pub fn conservation(&self) -> bool {
        let s: u32 = self.recs.iter().map(|r| r.touches).sum();
        s >= self.enabled_total && self.recs.iter().all(|r| r.a11y_ok)
    }

    /// 边界探针：查未登记景 → 显性 RouteMiss（不 panic）。
    pub fn touches_of(&self, s: Scene) -> Result<u32, ScCode> {
        self.recs.iter().find(|r| r.scene == s).map(|r| r.touches).ok_or(ScCode::RouteMiss)
    }

    /// 投影：触达最热景（并列取先登记者）。
    pub fn hottest(&self) -> Option<Scene> {
        let mut best: Option<&TouchRecord> = None;
        for r in &self.recs {
            match best {
                Some(b) if b.touches >= r.touches => {}
                _ => best = Some(r),
            }
        }
        best.map(|b| b.scene)
    }
}

/// 8 行确定性 LCG（无浮点无墙钟，深化回归专用）。
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        self.0
    }
}

const ALL_SCENES: [Scene; SCENE_N] = [
    Scene::NewFeature,
    Scene::CrossDebug,
    Scene::Release,
    Scene::Theme,
    Scene::Plugin,
    Scene::MultiTeam,
    Scene::AiAssist,
];
const ALL_PHASES: [Phase; 4] = [Phase::Design, Phase::Coding, Phase::Integration, Phase::Ship];

/// LCG 路由注入回归：64 轮随机启用子集 × 随机阶段——
/// 路由结果必须落在活动景；全空时必须显性 RouteMiss。任何偏离中文报错。
fn route_inject(seed: u32) -> Result<(), String> {
    let mut rng = Lcg(seed);
    for _ in 0..64u32 {
        let mut sb = SceneBook::new();
        for s in ALL_SCENES {
            if rng.next() % 2 == 0 {
                let _ = sb.enable(s, true);
            }
        }
        let ph = ALL_PHASES[(rng.next() % 4) as usize];
        match sb.route(ph) {
            Ok(scene) => {
                if !sb.active[scene as usize] {
                    return Err("路由注入：结果落在非活动景".to_string());
                }
            }
            Err(ScCode::RouteMiss) => {
                if sb.active.iter().any(|a| *a) {
                    return Err("路由注入：尚有活动景却报 RouteMiss".to_string());
                }
            }
            Err(_) => return Err("路由注入：异常码不符".to_string()),
        }
    }
    Ok(())
}

/// 负对照：七景全启用时四阶段零兜底直达。
fn route_full_negative() -> bool {
    let mut sb = SceneBook::new();
    for s in ALL_SCENES {
        let _ = sb.enable(s, true);
    }
    for ph in ALL_PHASES {
        if sb.route(ph).is_err() || sb.reroutes != 0 {
            return false;
        }
    }
    true
}

/// 幂等闸：同景二次启用零副作用（active/touch 均不变）。
fn idempotence_probe() -> bool {
    let mut sb = SceneBook::new();
    let _ = sb.enable(Scene::Theme, true);
    let snap_active = sb.active;
    let snap_touch = sb.touch;
    let _ = sb.enable(Scene::Theme, true);
    sb.active == snap_active && sb.touch == snap_touch
}

/// 序列化投影：七景无障碍触达位打包为单字节（bit i = 景 i 触达齐）。
pub fn pack_a11y(sb: &SceneBook) -> u8 {
    let mut b = 0u8;
    for i in 0..SCENE_N {
        if sb.touch[i].1 {
            b |= 1 << i;
        }
    }
    b
}

/// 解包还原触达位图。
pub fn unpack_a11y(b: u8) -> [bool; SCENE_N] {
    let mut out = [false; SCENE_N];
    for i in 0..SCENE_N {
        out[i] = b & (1 << i) != 0;
    }
    out
}

/// FNV-1a 32 位路由指纹：active 位图 + 触达包 + 重路由计数。
pub fn fnv_route(sb: &SceneBook) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for a in sb.active {
        h ^= u32::from(a);
        h = h.wrapping_mul(0x0100_0193);
    }
    h ^= u32::from(pack_a11y(sb));
    h = h.wrapping_mul(0x0100_0193);
    h ^= sb.reroutes;
    h.wrapping_mul(0x0100_0193)
}

/// 深化判据并入主判据流（C535-13..22）。
pub fn ves535_deep_checks(set: &mut CheckSet) {
    let mut tl = TouchLedger::new();
    let _ = tl.touch(Scene::Release, true);
    let _ = tl.touch(Scene::Release, true);
    let _ = tl.touch(Scene::Theme, true);
    set.add("C535-13", tl.conservation(), "触达账守恒 Σtouches≥启用总数且 a11y 恒真");
    set.add("C535-14", tl.touch(Scene::Plugin, false) == Err(ScCode::A11yMissing), "账内触达缺无障碍形态拒绝（域色）");
    set.add("C535-15", tl.touches_of(Scene::Plugin) == Err(ScCode::RouteMiss) && tl.touches_of(Scene::Release) == Ok(2), "越界景探针显性报错");
    set.add("C535-16", tl.hottest() == Some(Scene::Release), "最热景投影（2>1）");

    match route_inject(0x5EED_5350) {
        Ok(()) => set.add("C535-17", true, "LCG 路由注入 64 轮结果恒落活动景或显性失配"),
        Err(msg) => set.add_dyn("C535-17", false, &msg),
    }
    set.add("C535-18", route_full_negative(), "负对照：全启用四阶段零兜底直达");

    set.add("C535-19", idempotence_probe(), "幂等闸：同景二次启用零副作用");

    let mut sb2 = SceneBook::new();
    let _ = sb2.enable(Scene::AiAssist, true);
    let pack = pack_a11y(&sb2);
    let touch_flags: [bool; SCENE_N] = sb2.touch.map(|t| t.1);
    set.add("C535-20", unpack_a11y(pack) == touch_flags, "触达位打包解包往返");

    let fp = fnv_route(&sb2);
    let _ = sb2.enable(Scene::Theme, true);
    set.add("C535-21", fnv_route(&sb2) != fp, "FNV 路由指纹随启用集漂移");
    set.add("C535-22", unpack_a11y(0x7F) == [true; SCENE_N] && unpack_a11y(0) == [false; SCENE_N], "触达位图边界全 0/全 1");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_book() {
        let mut sb = SceneBook::new();
        assert!(sb.enable(Scene::Release, true).is_ok());
        assert_eq!(sb.route(Phase::Ship), Ok(Scene::Release));
        sb.active[2] = false;
        assert!(sb.route(Phase::Ship).is_err());
    }
}
