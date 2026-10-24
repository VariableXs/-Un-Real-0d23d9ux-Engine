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

    set
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
