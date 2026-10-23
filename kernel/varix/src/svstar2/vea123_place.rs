//! VE-F0123 · 混合场景放置决策（VE-A 域 · A05 多 GPU 与异构组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0123`
//!
//! **职责定位（锚点原文）**：典型场景的放置决策模板（游戏=独显/桌面=集显/创作=混合
//! 三模板），模板可定制，决策依据可解释。
//!
//! **铁律**：
//! 1. **场景模板**——三模板数据化。
//! 2. **可定制**——模板可改但校验。
//! 3. **依据可解释**——每决策带理由。
//! 4. **失配建议**——模板不匹配给建议不硬切。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8C1（放置决策）。
pub const SEG_BASE: u16 = 0xE8C1;

/// 模板失配。
pub const XC_TMPL_MISMATCH: u16 = SEG_BASE | 0x01;
/// 定制越界。
pub const XC_CUSTOM_OOB: u16 = SEG_BASE | 0x02;
/// 依据缺失。
pub const XC_NO_EVIDENCE: u16 = SEG_BASE | 0x03;
/// 未知场景。
pub const XC_UNKNOWN_SCENE: u16 = SEG_BASE | 0x04;

/// 场景类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scene {
    /// 游戏。
    Game,
    /// 桌面。
    Desktop,
    /// 创作。
    Creative,
}

/// 放置目标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Placement {
    /// 独显。
    Discrete,
    /// 集显。
    Integrated,
    /// 混合。
    Hybrid,
}

/// 决策模板。
#[derive(Clone, Debug)]
pub struct DecisionTemplate {
    pub scene: Scene,
    pub placement: Placement,
    /// 依据权重（可解释）。
    pub evidence_weights: Vec<(u8, u32)>,
}

impl DecisionTemplate {
    /// 三模板（默认）。
    pub fn defaults() -> Vec<DecisionTemplate> {
        vec![
            DecisionTemplate {
                scene: Scene::Game,
                placement: Placement::Discrete,
                evidence_weights: vec![(1, 800), (2, 200)], // 性能优先
            },
            DecisionTemplate {
                scene: Scene::Desktop,
                placement: Placement::Integrated,
                evidence_weights: vec![(1, 100), (2, 900)], // 能效优先
            },
            DecisionTemplate {
                scene: Scene::Creative,
                placement: Placement::Hybrid,
                evidence_weights: vec![(1, 500), (2, 500)], // 均衡
            },
        ]
    }

    /// 定制校验：权重和为 1000。
    pub fn validate(&self) -> bool {
        let sum: u32 = self.evidence_weights.iter().map(|(_, w)| w).sum();
        sum == 1000
    }
}

/// 决策记录（可解释）。
#[derive(Clone, Debug)]
pub struct DecisionRecord {
    pub scene: Scene,
    pub placement: Placement,
    /// 依据（理由码列表）。
    pub evidence: Vec<u16>,
    /// 是否建议切换（失配）。
    pub suggest_switch: bool,
}

/// 放置决策器。
pub struct PlacementDecider {
    templates: Vec<DecisionTemplate>,
    last_code: u16,
    decisions: u32,
    suggestions: u32,
}

impl PlacementDecider {
    pub fn new() -> Self {
        PlacementDecider {
            templates: DecisionTemplate::defaults(),
            last_code: 0,
            decisions: 0,
            suggestions: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 定制模板（校验越界）。
    pub fn customize(&mut self, tmpl: DecisionTemplate) -> Result<(), u16> {
        if !tmpl.validate() {
            self.last_code = XC_CUSTOM_OOB;
            return Err(XC_CUSTOM_OOB);
        }
        self.templates.retain(|t| t.scene != tmpl.scene);
        self.templates.push(tmpl);
        self.last_code = 0;
        Ok(())
    }

    /// 决策（可解释）。
    pub fn decide(&mut self, scene: Scene, evidence: &[u16]) -> Result<DecisionRecord, u16> {
        let tmpl = match self.templates.iter().find(|t| t.scene == scene) {
            Some(t) => t.clone(),
            None => {
                self.last_code = XC_UNKNOWN_SCENE;
                return Err(XC_UNKNOWN_SCENE);
            }
        };
        if evidence.is_empty() {
            self.last_code = XC_NO_EVIDENCE;
            return Err(XC_NO_EVIDENCE);
        }
        // 失配检测：依据与模板权重矛盾 → 建议切换。
        let mut mismatch = false;
        for (k, w) in &tmpl.evidence_weights {
            if *w >= 800 && !evidence.contains(&(*k as u16)) {
                mismatch = true;
            }
        }
        if mismatch {
            self.suggestions += 1;
            self.last_code = XC_TMPL_MISMATCH;
        }
        self.decisions += 1;
        Ok(DecisionRecord {
            scene,
            placement: tmpl.placement,
            evidence: evidence.to_vec(),
            suggest_switch: mismatch,
        })
    }

    pub fn decisions(&self) -> u32 {
        self.decisions
    }

    pub fn suggestions(&self) -> u32 {
        self.suggestions
    }

    pub fn template_count(&self) -> usize {
        self.templates.len()
    }
}

impl Default for PlacementDecider {
    fn default() -> Self {
        Self::new()
    }
}

/// 自检：场景模板、可定制、依据可解释、失配建议。
pub fn run_vea123_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    let mut set = CheckSet::new("vea123_place");

    // C123-01 三模板默认
    {
        let d = PlacementDecider::new();
        chk(&mut set, "C123-01 三模板", d.template_count() == 3, "模板缺");
    }

    // C123-02 游戏→独显
    {
        let mut d = PlacementDecider::new();
        let r = d.decide(Scene::Game, &[1, 2]);
        chk(&mut set, "C123-02 游戏独显", r.unwrap().placement == Placement::Discrete, "决策错");
    }

    // C123-03 桌面→集显
    {
        let mut d = PlacementDecider::new();
        let r = d.decide(Scene::Desktop, &[1, 2]);
        chk(&mut set, "C123-03 桌面集显", r.unwrap().placement == Placement::Integrated, "决策错");
    }

    // C123-04 创作→混合
    {
        let mut d = PlacementDecider::new();
        let r = d.decide(Scene::Creative, &[1, 2]);
        chk(&mut set, "C123-04 创作混合", r.unwrap().placement == Placement::Hybrid, "决策错");
    }

    // C123-05 可定制
    {
        let mut d = PlacementDecider::new();
        let r = d.customize(DecisionTemplate {
            scene: Scene::Game,
            placement: Placement::Hybrid,
            evidence_weights: vec![(1, 600), (2, 400)],
        });
        chk(&mut set, "C123-05 定制", r.is_ok() && d.decide(Scene::Game, &[1]).unwrap().placement == Placement::Hybrid, "定制失败");
    }

    // C123-06 定制越界拒绝
    {
        let mut d = PlacementDecider::new();
        let r = d.customize(DecisionTemplate {
            scene: Scene::Game,
            placement: Placement::Discrete,
            evidence_weights: vec![(1, 500), (2, 300)], // 800 != 1000
        });
        chk(
            &mut set,
            "C123-06 越界拒",
            r.is_err() && d.last_code() == XC_CUSTOM_OOB,
            "越界被放",
        );
    }

    // C123-07 依据可解释
    {
        let mut d = PlacementDecider::new();
        let r = d.decide(Scene::Game, &[1, 2]).unwrap();
        chk(&mut set, "C123-07 可解释", r.evidence == vec![1, 2], "依据缺");
    }

    // C123-08 失配建议
    {
        let mut d = PlacementDecider::new();
        let r = d.decide(Scene::Game, &[99]).unwrap(); // 缺关键依据
        chk(&mut set, "C123-08 失配建议", r.suggest_switch && d.suggestions() == 1, "未建议");
    }

    // C123-09 空依据拒绝
    {
        let mut d = PlacementDecider::new();
        let r = d.decide(Scene::Game, &[]);
        chk(
            &mut set,
            "C123-09 空依据拒",
            r.is_err() && d.last_code() == XC_NO_EVIDENCE,
            "空依据被放",
        );
    }

    // C123-10 计数自洽
    {
        let mut d = PlacementDecider::new();
        let _ = d.decide(Scene::Game, &[1]);
        let _ = d.decide(Scene::Desktop, &[2]);
        chk(&mut set, "C123-10 计数", d.decisions() == 2, "计数错");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea123_all_green() {
        let s = run_vea123_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
