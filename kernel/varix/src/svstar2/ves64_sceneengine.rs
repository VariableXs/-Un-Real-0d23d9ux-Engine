//! VE-F3664 · 个性化与场景联动（VE-S 域 · 批次 R04 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3664`
//!
//! **职责定位（锚点原文）**：场景感知个性化——
//! - **场景规则红线**：触发规则必须用户显式配置（系统自动猜场景改个性化 =
//!   主权侵犯，复述推荐分离）；
//! - **规则模型**：条件（时段/位置/应用）→ 动作（个性化切换）；
//! - **冲突显性**：多规则命中 → 优先级仲裁（最近创建优先 + 冲突提示）；
//! - **位置隐私**：位置规则本地评估不上传。
//!
//! **判据（锚点原文六条）**：场景规则显性、本地位置、冲突提示、
//! 事务复用、主权执法、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 系统擅自切→P1（红线实测——审计扫描无主规则） | [`S64Code::UNOWNED_RULE`] |
//! | 规则冲突静默→提示（红线实测） | [`S64Code::CONFLICT_SILENT`] |
//! | 切换闪断→事务复用（复述） | [`S64Code::SWITCH_GLITCH`] |
//! | 位置隐私（本地判定）（位置红线） | [`S64Code::LOCATION_UPLOAD`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3664 诊断码段（独占 0x70xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct S64Code(pub u16);

impl S64Code {
    /// 无主规则（系统擅自，P1）。
    pub const UNOWNED_RULE: S64Code = S64Code(0x7001);
    /// 冲突静默（红线实测）。
    pub const CONFLICT_SILENT: S64Code = S64Code(0x7002);
    /// 切换闪断（事务复用破）。
    pub const SWITCH_GLITCH: S64Code = S64Code(0x7003);
    /// 位置上传（隐私红线）。
    pub const LOCATION_UPLOAD: S64Code = S64Code(0x7004);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            S64Code::UNOWNED_RULE => "无主场景规则（P1 红线实测）。建议：规则必须用户显式创建。",
            S64Code::CONFLICT_SILENT => "规则冲突静默（红线实测）。建议：冲突显性提示。",
            S64Code::SWITCH_GLITCH => "切换闪断。建议：走原子事务复用。",
            S64Code::LOCATION_UPLOAD => "位置上传（隐私红线）。建议：位置规则本地评估。",
            _ => "未知诊断码。",
        }
    }
}

// ── 条件与动作 ──────────────────────────────────────────────────────

/// 触发条件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Condition {
    /// 时段（起止小时×100 口径）。
    TimeWindow(u16, u16),
    /// 位置（本地判定，仅哈希）。
    PlaceHash(u32),
    /// 前台应用。
    AppId(u32),
}

/// 动作（个性化切换）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// 切暗色。
    DarkMode(bool),
    /// 切勿扰。
    DoNotDisturb(bool),
    /// 切主题 id。
    Theme(u16),
}

// ── 场景规则 ────────────────────────────────────────────────────────

/// 规则（条件动作对）。
#[derive(Clone, Copy)]
pub struct SceneRule {
    pub id: u32,
    pub cond: Condition,
    pub act: Action,
    /// 用户显式创建位（红线：必须 true）。
    pub user_owned: bool,
    /// 创建序（最近优先仲裁）。
    pub created_seq: u32,
}

// ── 冲突记录 ────────────────────────────────────────────────────────

/// 冲突提示（显性）。
#[derive(Clone, Copy)]
pub struct ConflictNote {
    pub rule_a: u32,
    pub rule_b: u32,
}

// ── 场景引擎 ────────────────────────────────────────────────────────

/// 场景联动引擎。
pub struct SceneEngine {
    rules: Vec<SceneRule>,
    conflicts: Vec<ConflictNote>,
    /// 位置是否保持本地（隐私红线执法位）。
    location_local: bool,
    unowned_hits: u32,
    glitch_hits: u32,
    seq: u32,
    last_code: S64Code,
    has_error: bool,
}

impl SceneEngine {
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            conflicts: Vec::new(),
            location_local: true,
            unowned_hits: 0,
            glitch_hits: 0,
            seq: 0,
            last_code: S64Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> S64Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: S64Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 建规则；无主规则即红线（P1）。
    pub fn add_rule(&mut self, id: u32, cond: Condition, act: Action, user_owned: bool) -> Result<usize, S64Code> {
        if !user_owned {
            self.unowned_hits += 1;
            self.fail(S64Code::UNOWNED_RULE);
            return Err(S64Code::UNOWNED_RULE);
        }
        self.seq += 1;
        self.rules.push(SceneRule { id, cond, act, user_owned, created_seq: self.seq });
        self.last_code = S64Code(0);
        self.has_error = false;
        Ok(self.rules.len())
    }

    /// 无主规则审计（审计扫描口径）。
    pub fn audit_ownership(&mut self) -> Result<u32, S64Code> {
        let bad = self.rules.iter().filter(|r| !r.user_owned).count();
        if bad > 0 {
            self.unowned_hits += bad as u32;
            self.fail(S64Code::UNOWNED_RULE);
            return Err(S64Code::UNOWNED_RULE);
        }
        self.last_code = S64Code(0);
        self.has_error = false;
        Ok(self.rules.len() as u32)
    }

    /// 规则命中（条件求值）。
    pub fn matches(rule: &SceneRule, now: u16, place: u32, app: u32) -> bool {
        match rule.cond {
            Condition::TimeWindow(a, b) => now >= a && now <= b,
            Condition::PlaceHash(h) => h == place,
            Condition::AppId(id) => id == app,
        }
    }

    /// 仲裁：多规则命中取 created_seq 最大者，冲突显性登记。
    pub fn arbitrate(&mut self, hits: &[u32]) -> Result<u32, S64Code> {
        if hits.len() < 2 {
            return Ok(hits.first().copied().unwrap_or(0));
        }
        // 冲突显性：两两登记提示
        for i in 0..hits.len() {
            for j in (i + 1)..hits.len() {
                self.conflicts.push(ConflictNote { rule_a: hits[i], rule_b: hits[j] });
            }
        }
        // 最近创建优先
        let mut best: Option<&SceneRule> = None;
        for id in hits {
            if let Some(r) = self.rules.iter().find(|r| r.id == *id) {
                best = match best {
                    Some(b) if b.created_seq >= r.created_seq => Some(b),
                    _ => Some(r),
                };
            }
        }
        self.last_code = S64Code(0);
        self.has_error = false;
        Ok(best.map(|r| r.id).unwrap_or(0))
    }

    /// 冲突提示数（显性复述）。
    pub fn conflict_notes(&self) -> usize {
        self.conflicts.len()
    }

    /// 位置本地执法：上传即红线。
    pub fn set_location_policy(&mut self, local: bool) -> Result<bool, S64Code> {
        self.location_local = local;
        if local {
            self.last_code = S64Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(S64Code::LOCATION_UPLOAD);
            Err(S64Code::LOCATION_UPLOAD)
        }
    }

    /// 原子切换（事务复用）：`atomic=true` 无闪断。
    pub fn apply_switch(&mut self, atomic: bool) -> Result<bool, S64Code> {
        if atomic {
            self.last_code = S64Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.glitch_hits += 1;
            self.fail(S64Code::SWITCH_GLITCH);
            Err(S64Code::SWITCH_GLITCH)
        }
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    pub fn unowned_hits(&self) -> u32 {
        self.unowned_hits
    }

    pub fn glitch_hits(&self) -> u32 {
        self.glitch_hits
    }

    pub fn location_local(&self) -> bool {
        self.location_local
    }

    /// 主权执法总评：全规则有主 + 位置本地。
    pub fn sovereignty_green(&self) -> bool {
        self.rules.iter().all(|r| r.user_owned) && self.location_local
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves64_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves64_sceneengine");
    {
        let e = SceneEngine::new();
        chk(&mut set, "C3664-01", e.rule_count() == 0 && e.location_local(), "err");
    }
    {
        let mut e = SceneEngine::new();
        let r = e.add_rule(1, Condition::TimeWindow(2000, 600), Action::DarkMode(true), true);
        chk(&mut set, "C3664-02", r == Ok(1), "err");
    }
    {
        let mut e = SceneEngine::new();
        let r = e.add_rule(1, Condition::AppId(7), Action::DoNotDisturb(true), false);
        chk(&mut set, "C3664-03", r.err() == Some(S64Code::UNOWNED_RULE) && e.unowned_hits() == 1, "err");
    }
    {
        let mut e = SceneEngine::new();
        e.add_rule(1, Condition::TimeWindow(2000, 600), Action::DarkMode(true), true);
        let r = e.audit_ownership();
        chk(&mut set, "C3664-04", r == Ok(1), "err");
    }
    {
        let rule = SceneRule { id: 1, cond: Condition::TimeWindow(2000, 600), act: Action::DarkMode(true), user_owned: true, created_seq: 1 };
        chk(&mut set, "C3664-05", SceneEngine::matches(&rule, 2200, 0, 0), "err");
    }
    {
        let rule = SceneRule { id: 1, cond: Condition::PlaceHash(0xAB), act: Action::Theme(2), user_owned: true, created_seq: 1 };
        chk(&mut set, "C3664-06", !SceneEngine::matches(&rule, 0, 0xCD, 0), "err");
    }
    {
        let mut e = SceneEngine::new();
        e.add_rule(1, Condition::AppId(1), Action::DarkMode(true), true);
        e.add_rule(2, Condition::AppId(1), Action::Theme(3), true);
        let r = e.arbitrate(&[1, 2]);
        chk(&mut set, "C3664-07", r == Ok(2) && e.conflict_notes() == 1, "err");
    }
    {
        let mut e = SceneEngine::new();
        let r = e.set_location_policy(true);
        chk(&mut set, "C3664-08", r == Ok(true), "err");
    }
    {
        let mut e = SceneEngine::new();
        let r = e.set_location_policy(false);
        chk(&mut set, "C3664-09", r.err() == Some(S64Code::LOCATION_UPLOAD), "err");
    }
    {
        let mut e = SceneEngine::new();
        let r = e.apply_switch(true);
        chk(&mut set, "C3664-10", r == Ok(true) && e.glitch_hits() == 0, "err");
    }
    {
        let mut e = SceneEngine::new();
        let r = e.apply_switch(false);
        chk(&mut set, "C3664-11", r.err() == Some(S64Code::SWITCH_GLITCH), "err");
    }
    {
        let mut e = SceneEngine::new();
        e.add_rule(1, Condition::TimeWindow(0, 2359), Action::DarkMode(false), true);
        chk(&mut set, "C3664-12", e.sovereignty_green(), "err");
    }
    set
}
