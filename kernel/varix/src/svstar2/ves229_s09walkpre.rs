//! VE-F3975 · 真实走查预演
//!
//! S 域真实走查预演：五角色剧本预演（视障/低视/色觉障碍/认知障碍/运动
//! 障碍各一套任务脚本）+ 走查表预填。预演不代替终走查声明。录证预案
//! （知情同意模板前置）。
//! 错误路径：剧本缺陷→改本重演；任务不可达→立案（潜在阻断项）；设备缺
//! →借调登记。
//! 性能：预演 O(剧本)；改本 O(缺)；登记 O(1)。
//! 跨批对接：F3963/F3964 上游；F3994 终走查下游。
//! 无障碍与隐私：预演即无障碍本体演练（域本色）；录证预案含知情同意
//! 模板。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x3580）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x3580;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9OCode(pub u16);

impl S9OCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const SCRIPT_DEFECT: Self = Self(SEG_BASE + 1);
    pub const TASK_UNREACHABLE: Self = Self(SEG_BASE + 2);
    pub const DEVICE_MISSING: Self = Self(SEG_BASE + 3);
    pub const CONSENT_MISSING: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::SCRIPT_DEFECT => "剧本缺陷→改本重演",
            Self::TASK_UNREACHABLE => "任务不可达→立案",
            Self::DEVICE_MISSING => "设备缺→借调登记",
            Self::CONSENT_MISSING => "同意缺→禁录",
            _ => "未知",
        }
    }
}

// ── 五角色剧本 ───────────────────────────────────────────────────────

/// 五角色。
pub const ROLES: [&str; 5] = ["视障", "低视", "色觉障碍", "认知障碍", "运动障碍"];

/// 任务脚本条目。
#[derive(Debug, Clone)]
pub struct TaskScript {
    pub name: &'static str,
    /// 角色（下标对齐 ROLES）。
    pub role: usize,
    /// 任务步骤。
    pub steps: Vec<&'static str>,
    /// 改本次数（缺陷修复计数）。
    pub revisions: u8,
}

impl TaskScript {
    pub fn reachable(&self) -> bool {
        !self.steps.is_empty() && self.steps.iter().all(|s| !s.is_empty())
    }
}

/// 剧本册：五角色各至少一套。
#[derive(Debug, Clone)]
pub struct ScriptBook {
    pub scripts: Vec<TaskScript>,
}

impl ScriptBook {
    pub fn new() -> Self {
        Self { scripts: Vec::new() }
    }

    pub fn add(&mut self, s: TaskScript) -> S9OCode {
        if s.role >= ROLES.len() {
            return S9OCode::SCRIPT_DEFECT;
        }
        self.scripts.push(s);
        S9OCode::OK
    }

    /// 角色覆盖缺口。
    pub fn role_gaps(&self) -> Vec<usize> {
        (0..ROLES.len()).filter(|r| !self.scripts.iter().any(|s| s.role == *r && s.reachable())).collect()
    }

    /// 剧本缺陷检出：不可达任务。
    pub fn defects(&self) -> Vec<&'static str> {
        self.scripts.iter().filter(|s| !s.reachable()).map(|s| s.name).collect()
    }

    /// 改本重演：缺陷本修订。
    pub fn revise(&mut self, name: &str, steps: Vec<&'static str>) -> S9OCode {
        match self.scripts.iter_mut().find(|s| s.name == name) {
            Some(s) => {
                s.steps = steps;
                s.revisions += 1;
                S9OCode::OK
            }
            None => S9OCode::SCRIPT_DEFECT,
        }
    }
}

impl Default for ScriptBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 走查表预填 ───────────────────────────────────────────────────────

/// 预填走查表行。
#[derive(Debug, Clone)]
pub struct PreWalkRow {
    pub task: &'static str,
    pub role: usize,
    /// 预填预期结果。
    pub expect: &'static str,
    /// 正式走查结果（预演期为空）。
    pub actual: Option<&'static str>,
}

impl PreWalkRow {
    /// 不代终查断言：预演期 actual 必须为空。
    pub fn not_final(&self) -> bool {
        self.actual.is_none()
    }
}

/// 预填表。
#[derive(Debug, Clone)]
pub struct PreWalkSheet {
    pub rows: Vec<PreWalkRow>,
}

impl PreWalkSheet {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn prefill(&mut self, r: PreWalkRow) -> S9OCode {
        if r.role >= ROLES.len() {
            return S9OCode::SCRIPT_DEFECT;
        }
        self.rows.push(r);
        S9OCode::OK
    }

    /// 全部未代终查。
    pub fn none_final(&self) -> bool {
        self.rows.iter().all(|r| r.not_final())
    }

    /// 终填转正（终走查阶段才可填）。
    pub fn finalize(&mut self, task: &str, actual: &'static str) -> S9OCode {
        match self.rows.iter_mut().find(|r| r.task == task) {
            Some(r) => {
                r.actual = Some(actual);
                S9OCode::OK
            }
            None => S9OCode::TASK_UNREACHABLE,
        }
    }
}

impl Default for PreWalkSheet {
    fn default() -> Self {
        Self::new()
    }
}

// ── 录证预案（知情同意）──────────────────────────────────────────────

/// 录证预案。
#[derive(Debug, Clone)]
pub struct RecordPlan {
    /// 参与者同意（知情同意模板已签）。
    pub consent: bool,
    /// 脱敏预案。
    pub sanitize_plan: bool,
    /// 设备清单。
    pub devices: Vec<&'static str>,
    /// 借调登记。
    pub borrowed: Vec<&'static str>,
}

impl RecordPlan {
    pub fn new() -> Self {
        Self { consent: false, sanitize_plan: false, devices: Vec::new(), borrowed: Vec::new() }
    }

    /// 可录断言：同意 + 脱敏预案齐备。
    pub fn can_record(&self) -> bool {
        self.consent && self.sanitize_plan
    }

    /// 设备缺→借调登记。
    pub fn borrow(&mut self, device: &'static str) -> S9OCode {
        if self.devices.iter().any(|d| *d == device) {
            return S9OCode::DEVICE_MISSING;
        }
        self.borrowed.push(device);
        S9OCode::OK
    }

    pub fn device_ready(&self, needed: &[&'static str]) -> bool {
        needed.iter().all(|n| self.devices.contains(n) || self.borrowed.contains(n))
    }
}

impl Default for RecordPlan {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves229_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves229_s09walkpre");

    // 01 五角色清单守恒
    {
        chk(&mut set, "C3975-01", ROLES.len() == 5 && ROLES[0] == "视障" && ROLES[4] == "运动障碍", "err");
    }
    // 02 五角色剧本全覆盖
    {
        let mut b = ScriptBook::new();
        for (i, r) in ROLES.iter().enumerate() {
            b.add(TaskScript { name: r, role: i, steps: vec!["打开", "操作", "确认"], revisions: 0 });
        }
        chk(&mut set, "C3975-02", b.role_gaps().is_empty() && b.defects().is_empty(), "err");
    }
    // 03 角色覆盖缺口
    {
        let mut b = ScriptBook::new();
        b.add(TaskScript { name: "只视障", role: 0, steps: vec!["s"], revisions: 0 });
        chk(&mut set, "C3975-03", b.role_gaps().len() == 4, "err");
    }
    // 04 剧本缺陷检出（空步骤）
    {
        let mut b = ScriptBook::new();
        b.add(TaskScript { name: "空本", role: 1, steps: vec![], revisions: 0 });
        chk(&mut set, "C3975-04", b.defects() == vec!["空本"], "err");
    }
    // 05 改本重演实测
    {
        let mut b = ScriptBook::new();
        b.add(TaskScript { name: "缺陷本", role: 2, steps: vec![], revisions: 0 });
        chk(&mut set, "C3975-05", b.revise("缺陷本", vec!["新步骤"]) == S9OCode::OK && b.defects().is_empty() && b.scripts[0].revisions == 1, "err");
    }
    // 06 角色号越界拒收
    {
        let mut b = ScriptBook::new();
        chk(&mut set, "C3975-06", b.add(TaskScript { name: "越界", role: 5, steps: vec!["s"], revisions: 0 }) == S9OCode::SCRIPT_DEFECT, "err");
    }
    // 07 预填表不代终查
    {
        let mut s = PreWalkSheet::new();
        s.prefill(PreWalkRow { task: "结账", role: 3, expect: "三分钟内完成", actual: None });
        chk(&mut set, "C3975-07", s.none_final(), "err");
    }
    // 08 预填越界拒收 + 终填转正
    {
        let mut s = PreWalkSheet::new();
        chk(&mut set, "C3975-08", s.prefill(PreWalkRow { task: "x", role: 9, expect: "e", actual: None }) == S9OCode::SCRIPT_DEFECT
            && { s.prefill(PreWalkRow { task: "结账", role: 0, expect: "e", actual: None }); s.finalize("结账", "4 分钟") == S9OCode::OK && !s.none_final() }, "err");
    }
    // 09 终填不存在任务拒收
    {
        let mut s = PreWalkSheet::new();
        chk(&mut set, "C3975-09", s.finalize("幽灵", "x") == S9OCode::TASK_UNREACHABLE, "err");
    }
    // 10 录证：同意缺→禁录
    {
        let mut p = RecordPlan::new();
        p.sanitize_plan = true;
        chk(&mut set, "C3975-10", !p.can_record() && { p.consent = true; p.can_record() }, "err");
    }
    // 11 设备缺→借调登记
    {
        let mut p = RecordPlan::new();
        p.consent = true;
        p.sanitize_plan = true;
        p.devices = vec!["读屏器甲"];
        chk(&mut set, "C3975-11", p.borrow("盲文点显器") == S9OCode::OK && p.device_ready(&["读屏器甲", "盲文点显器"]), "err");
    }
    // 12 重复借调拒收
    {
        let mut p = RecordPlan::new();
        p.devices = vec!["甲"];
        chk(&mut set, "C3975-12", p.borrow("甲") == S9OCode::DEVICE_MISSING, "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3975-13", S9OCode::CONSENT_MISSING.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9OCode::OK, S9OCode::SCRIPT_DEFECT, S9OCode::TASK_UNREACHABLE, S9OCode::DEVICE_MISSING, S9OCode::CONSENT_MISSING];
        chk(&mut set, "C3975-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_walkpre_all_green() {
        let s = run_ves229_checks();
        assert!(s.all_passed(), "ves229 red: {}", s.red_items_text());
    }

    #[test]
    fn test_final_transfer_gate() {
        let mut s = PreWalkSheet::new();
let _ =         s.prefill(PreWalkRow { task: "t", role: 4, expect: "e", actual: None });
        assert!(s.none_final());
let _ =         s.finalize("t", "ok");
        assert!(!s.none_final());
        assert_eq!(s.rows[0].actual, Some("ok"));
    }

    #[test]
    fn test_record_full_gate() {
        let mut p = RecordPlan::new();
        assert!(!p.can_record());
        p.consent = true;
        assert!(!p.can_record());
        p.sanitize_plan = true;
        assert!(p.can_record());
    }
}
