//! cgd23_selfcal — CGPU-F0503 均衡器自校准（滚动调优/审计/回退/分场景）。
//!
//! 判据原文（施工书锚点 CGPU-F0503）：自校准：策略参数自校准（均衡策略参
//! 数按实测效果滚动调优——不引入额外延迟的硬约束）；校准审计（参数变化可
//! 查——不悄悄漂移）；校准失败回退（自校准无效→默认参数+告警）；校准与画
//! 像联动（按场景分别校准——F0370 画像复用）。判据：自校准、审计、回退、
//! 分场景、判据。
//!
//! 模型：每场景独立参数（目标缓冲深度/切换灵敏度），按窗口效果分滚动调
//! 优；每次变化写审计账（旧值/新值/依据效果）；连续无效→回退默认+告警标
//! 志；调优硬约束：新参数不得引入额外延迟（深度只可降或维持）。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 诊断码段 0x9000（cgpu 域内唯一）。
pub const XC_CAL_LATENCY_VIOLATION: u32 = 0x9001;
pub const XC_CAL_SCENE_UNREGISTERED: u32 = 0x9002;
pub const XC_CAL_AUDIT_MISSING: u32 = 0x9003;
pub const XC_CAL_ROLLBACK_UNSET: u32 = 0x9004;
pub const XC_CAL_PARAM_OOB: u32 = 0x9005;

/// 单场景策略参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneParam {
    /// 目标缓冲深度（帧）。
    pub depth: u8,
    /// 切换灵敏度（0=迟钝 100=敏感）。
    pub sensitivity: u8,
}

impl SceneParam {
    /// 默认参数。
    pub const DEFAULT: SceneParam = SceneParam {
        depth: 2,
        sensitivity: 50,
    };

    /// 参数合法性：深度 1..=4、灵敏度 0..=100。
    pub const fn is_valid(&self) -> bool {
        self.depth >= 1 && self.depth <= 4 && self.sensitivity <= 100
    }

    /// 延迟硬约束：候选不得比当前更深。
    pub const fn no_deeper(&self, cand: &SceneParam) -> bool {
        cand.depth <= self.depth
    }
}

/// 一条审计账目。
#[derive(Clone, Copy, Debug)]
pub struct CalAudit {
    /// 旧参数。
    pub old: SceneParam,
    /// 新参数。
    pub new: SceneParam,
    /// 调优依据：窗口改善（千分比）。
    pub evidence_permille: u32,
}

/// 场景校准器（F0370 画像复用：场景名挂参数）。
#[derive(Debug)]
pub struct SceneCal {
    /// 场景名。
    pub scene: &'static str,
    /// 当前参数。
    pub param: SceneParam,
    /// 审计账。
    pub audits: Vec<CalAudit>,
    /// 连续无效回评计数。
    pub failed: u32,
    /// 已回退标记。
    pub rolled_back: bool,
}

impl SceneCal {
    /// 连续无效阈值：3 次回退。
    pub const FAIL_LIMIT: u32 = 3;

    /// 注册场景：参数取默认。
    pub fn new(scene: &'static str) -> Self {
        SceneCal {
            scene,
            param: SceneParam::DEFAULT,
            audits: Vec::new(),
            failed: 0,
            rolled_back: false,
        }
    }

    /// 自校准一步：以窗口效果（改善千分比）驱动新候选。
    /// 改善 ≥300‰ 敏感 +10；改善 <100‰ 或劣化（≤0）降敏 -10；
    /// 抖动大（效果介于其间）深度 -1（仅当 depth>1，满足延迟硬约束）。
    /// 任何变化写审计；不变不写账。
    pub fn calibrate(&mut self, evidence_permille: i32) -> Result<Option<CalAudit>, u32> {
        if self.rolled_back {
            return Err(XC_CAL_ROLLBACK_UNSET);
        }
        let old = self.param;
        let mut cand = old;
        if evidence_permille >= 300 {
            cand.sensitivity = (cand.sensitivity + 10).min(100);
        } else if evidence_permille < 100 {
            cand.sensitivity = cand.sensitivity.saturating_sub(10);
        } else if cand.depth > 1 {
            cand.depth -= 1;
        }
        if !cand.is_valid() {
            return Err(XC_CAL_PARAM_OOB);
        }
        // 延迟硬约束：不得比当前更深。
        if !old.no_deeper(&cand) {
            return Err(XC_CAL_LATENCY_VIOLATION);
        }
        if cand == old {
            // 连续无效累计。
            if evidence_permille < 100 {
                self.failed += 1;
                if self.failed >= Self::FAIL_LIMIT {
                    self.rollback()?;
                    return Ok(None);
                }
            }
            return Ok(None);
        }
        let a = CalAudit {
            old,
            new: cand,
            evidence_permille: evidence_permille.max(0) as u32,
        };
        self.param = cand;
        self.audits.push(a);
        self.failed = 0;
        Ok(Some(a))
    }

    /// 失败回退：参数回默认 + 告警置位。
    pub fn rollback(&mut self) -> Result<SceneParam, u32> {
        let old = self.param;
        self.param = SceneParam::DEFAULT;
        self.rolled_back = true;
        self.failed = 0;
        let _ = old;
        Ok(self.param)
    }

    /// 审计对账：每条账目的 new 必须与下一条的 old 严丝合缝（无悄悄漂移）。
    pub fn audit_chain_ok(&self) -> bool {
        let mut i = 1;
        while i < self.audits.len() {
            if self.audits[i].old != self.audits[i - 1].new {
                return false;
            }
            i += 1;
        }
        // 当前参数必须是最后一条账目的 new（或默认无账）。
        match self.audits.last() {
            Some(last) => self.param == last.new,
            None => self.param == SceneParam::DEFAULT,
        }
    }
}

/// 分场景注册表。
#[derive(Debug)]
pub struct CalRegistry {
    scenes: Vec<SceneCal>,
}

impl CalRegistry {
    /// 空注册表。
    pub fn new() -> Self {
        CalRegistry { scenes: Vec::new() }
    }

    /// 注册场景（重名拒）。
    pub fn register(&mut self, scene: &'static str) -> Result<(), u32> {
        if self.find(scene).is_some() {
            return Err(XC_CAL_SCENE_UNREGISTERED);
        }
        self.scenes.push(SceneCal::new(scene));
        Ok(())
    }

    /// 查场景。
    pub fn find(&mut self, scene: &str) -> Option<&mut SceneCal> {
        let mut i = 0;
        while i < self.scenes.len() {
            if self.scenes[i].scene == scene {
                return Some(&mut self.scenes[i]);
            }
            i += 1;
        }
        None
    }

    /// 分场景校准：未注册场景名即拒（画像缺位）。
    pub fn calibrate_scene(
        &mut self,
        scene: &str,
        evidence_permille: i32,
    ) -> Result<Option<CalAudit>, u32> {
        match self.find(scene) {
            Some(c) => c.calibrate(evidence_permille),
            None => Err(XC_CAL_SCENE_UNREGISTERED),
        }
    }

    /// 场景数。
    pub fn len(&self) -> usize {
        self.scenes.len()
    }

    /// 空？无意义恒可实现。
    pub fn is_empty(&self) -> bool {
        self.scenes.is_empty()
    }
}

impl Default for CalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 自校准总门面。
#[derive(Debug, Default)]
pub struct SelfCalEngine {
    /// 注册表。
    pub reg: CalRegistry,
}

impl SelfCalEngine {
    /// 自检一：滚动调优——效果驱动参数变化、灵敏钳制、账目落痕。
    pub fn selfcheck_calibrate(&mut self) -> Result<(), u32> {
        let mut c = SceneCal::new("battle");
        // 高改善→敏 +10。
        let a = c.calibrate(500)?.ok_or(XC_CAL_AUDIT_MISSING)?;
        if a.new.sensitivity != 60 || a.old != SceneParam::DEFAULT {
            return Err(XC_CAL_PARAM_OOB);
        }
        // 中间带→深度 2→1（延迟硬约束内）。
        let a2 = c.calibrate(200)?.ok_or(XC_CAL_AUDIT_MISSING)?;
        if a2.new.depth != 1 {
            return Err(XC_CAL_LATENCY_VIOLATION);
        }
        // 深度已 1：中间带不再降深（无变化不写账）。
        if c.calibrate(200)?.is_some() {
            return Err(XC_CAL_AUDIT_MISSING);
        }
        // 低效果连按 10 次敏 → 钳 0。
        let mut i = 0;
        while i < 12 {
            let _ = c.calibrate(50)?;
            i += 1;
        }
        if c.param.sensitivity != 0 {
            return Err(XC_CAL_PARAM_OOB);
        }
        if !c.audit_chain_ok() {
            return Err(XC_CAL_AUDIT_MISSING);
        }
        Ok(())
    }

    /// 自检二：审计——账链严丝合缝、断链即拒。
    pub fn selfcheck_audit(&self) -> Result<(), u32> {
        let mut c = SceneCal::new("menu");
        let _ = c.calibrate(400)?;
        let _ = c.calibrate(500)?;
        let _ = c.calibrate(150)?;
        if !c.audit_chain_ok() {
            return Err(XC_CAL_AUDIT_MISSING);
        }
        // 人工破坏账链：改中间 new。
        let mut bad = c.clone_param_ledger();
        if bad.len() >= 2 {
            bad[0].new.sensitivity = 99;
            if !chain_of(&bad, bad[1].new) {
                // 断链被检出才算对——此处构造后应报 false。
            } else {
                return Err(XC_CAL_AUDIT_MISSING);
            }
        }
        Ok(())
    }

    /// 自检三：回退——三次无效回默认+告警；回退后拒绝继续校准。
    pub fn selfcheck_rollback(&mut self) -> Result<(), u32> {
        let mut c = SceneCal::new("cutscene");
        // 无变化且低效果 ×3 → 回退。
        let mut i = 0;
        while i < 2 {
            if c.calibrate(0)?.is_some() {
                return Err(XC_CAL_AUDIT_MISSING);
            }
            i += 1;
        }
        if c.rolled_back || c.failed != 2 {
            return Err(XC_CAL_ROLLBACK_UNSET);
        }
        let _ = c.calibrate(0)?;
        if !c.rolled_back {
            return Err(XC_CAL_ROLLBACK_UNSET);
        }
        if c.param != SceneParam::DEFAULT {
            return Err(XC_CAL_ROLLBACK_UNSET);
        }
        // 回退后继续校准→拒。
        if c.calibrate(500).is_ok() {
            return Err(XC_CAL_ROLLBACK_UNSET);
        }
        Ok(())
    }

    /// 自检四：分场景——注册/重名/未注册画像拒、场景间参数隔离。
    pub fn selfcheck_scenes(&mut self) -> Result<(), u32> {
        let mut r = CalRegistry::new();
        r.register("battle")?;
        r.register("menu")?;
        // 重名拒。
        if r.register("battle").is_ok() {
            return Err(XC_CAL_SCENE_UNREGISTERED);
        }
        // 未注册拒。
        if r.calibrate_scene("cutscene", 500).is_ok() {
            return Err(XC_CAL_SCENE_UNREGISTERED);
        }
        // 隔离：只动 battle，menu 不漂。
        let _ = r.calibrate_scene("battle", 500)?;
        let b = r.find("battle").ok_or(XC_CAL_SCENE_UNREGISTERED)?;
        if b.param.sensitivity != 60 {
            return Err(XC_CAL_PARAM_OOB);
        }
        let m = r.find("menu").ok_or(XC_CAL_SCENE_UNREGISTERED)?;
        if m.param != SceneParam::DEFAULT {
            return Err(XC_CAL_PARAM_OOB);
        }
        Self::segment_guard()?;
        Ok(())
    }

    /// 自检五：延迟硬约束专项——任何审计 new.depth ≤ old.depth。
    pub fn selfcheck_latency(&self) -> Result<(), u32> {
        let mut c = SceneCal::new("video");
        let mut i = 0;
        while i < 8 {
            let _ = c.calibrate(200 + (i % 3) * 100)?;
            i += 1;
        }
        for a in &c.audits {
            if a.new.depth > a.old.depth {
                return Err(XC_CAL_LATENCY_VIOLATION);
            }
            if !a.new.is_valid() {
                return Err(XC_CAL_PARAM_OOB);
            }
        }
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x9000..0x9100 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_CAL_LATENCY_VIOLATION,
            XC_CAL_SCENE_UNREGISTERED,
            XC_CAL_AUDIT_MISSING,
            XC_CAL_ROLLBACK_UNSET,
            XC_CAL_PARAM_OOB,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x9000 || codes[i] >= 0x9100 {
                return Err(codes[i]);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(codes[i]);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}

/// 辅助：克隆审计账（自检二用）。
impl SceneCal {
    fn clone_param_ledger(&self) -> Vec<CalAudit> {
        self.audits.clone()
    }
}

/// 辅助：账链一致性（对给定账与当前参数）。
fn chain_of(audits: &[CalAudit], cur: SceneParam) -> bool {
    let mut i = 1;
    while i < audits.len() {
        if audits[i].old != audits[i - 1].new {
            return false;
        }
        i += 1;
    }
    match audits.last() {
        Some(last) => cur == last.new,
        None => cur == SceneParam::DEFAULT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgd23_selfchecks() {
        let mut e = SelfCalEngine::default();
        e.reg.register("battle").unwrap();
        e.selfcheck_calibrate().unwrap();
        e.selfcheck_audit().unwrap();
        e.selfcheck_rollback().unwrap();
        e.selfcheck_scenes().unwrap();
        e.selfcheck_latency().unwrap();
        assert_eq!(SelfCalEngine::segment_guard(), Ok(()));
    }
}
