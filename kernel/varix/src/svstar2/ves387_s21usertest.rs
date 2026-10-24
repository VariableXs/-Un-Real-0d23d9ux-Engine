//! ves387_s21usertest — VE-F4147 · i18n 用户测试
//! 五角色×多语种用户测试方案（母语者验证翻译自然度 + 方向语种用户验证 RTL 体验 + 辅助技术母语档（域本色））
//! + 任务脚本语种定制 + 录证知情同意流程（同意缺→禁录）+ 自然度评分低→回 T04 复审 + 任务不可达→立案。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xE340;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UtCode {
    Ok = 0,
    ConsentMissing = SEG_BASE + 0x01,
    TaskUnreachable = SEG_BASE + 0x02,
    NaturalnessLow = SEG_BASE + 0x03,
}

/// 五角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    NativeReviewer = 0,   // 母语者（翻译自然度）
    RtlUser = 1,          // 方向语种用户（RTL 体验）
    CjkUser = 2,          // CJK 用户
    AssistTechUser = 3,   // 辅助技术母语用户（域本色档）
    LocaleOpsUser = 4,    // 地区运营用户
}

impl Role {
    pub const ALL: [Role; 5] = [
        Role::NativeReviewer, Role::RtlUser, Role::CjkUser, Role::AssistTechUser, Role::LocaleOpsUser];
    pub fn name(self) -> &'static str {
        match self {
            Role::NativeReviewer => "native", Role::RtlUser => "rtl-user",
            Role::CjkUser => "cjk-user", Role::AssistTechUser => "assist-tech",
            Role::LocaleOpsUser => "locale-ops",
        }
    }
}

/// 测试参与者：角色 + 语种 + 知情同意。
pub struct Participant {
    pub role: Role,
    pub lang: &'static str,
    pub consented: bool, // 知情同意
}

impl Participant {
    /// 录证资格：同意缺→禁录。
    pub fn recordable(&self) -> bool { self.consented }
}

/// 任务脚本：语种定制 + 可达性。
pub struct TaskScript {
    pub task: &'static str,
    pub lang: &'static str,
    pub reachable: bool, // 任务可达（不可达→立案）
}

impl TaskScript {
    /// 语种定制校验：脚本语种须与参与者语种一致。
    pub fn fits(&self, p: &Participant) -> bool { self.lang == p.lang }
}

/// 测试结果：自然度评分（0-100）。
#[derive(Clone, Copy)]
pub struct TrialResult {
    pub role: Role,
    pub naturalness: u8, // 自然度评分
    pub rtl_ok: Option<bool>,
}

impl TrialResult {
    /// 自然度红线：低于 70 → 回 T04 复审。
    pub fn naturalness_ok(&self) -> bool { self.naturalness >= 70 }
    /// RTL 体验断言（仅 RtlUser 档要求）。
    pub fn rtl_experienced(&self) -> bool {
        self.role != Role::RtlUser || self.rtl_ok == Some(true)
    }
}

/// 用户测试方案：参与者 × 脚本 × 结果汇总。
pub struct TestPlan {
    pub participants: Vec<Participant>,
    pub scripts: Vec<TaskScript>,
    pub results: Vec<TrialResult>,
    pub cases: Vec<&'static str>, // 立案册
}

impl TestPlan {
    pub fn new() -> Self { TestPlan { participants: Vec::new(), scripts: Vec::new(), results: Vec::new(), cases: Vec::new() } }

    pub fn add_participant(&mut self, p: Participant) { self.participants.push(p); }
    pub fn add_script(&mut self, s: TaskScript) {
        if !s.reachable { self.cases.push("task-unreachable"); }
        self.scripts.push(s);
    }
    pub fn add_result(&mut self, r: TrialResult) {
        if !r.naturalness_ok() { self.cases.push("naturalness-low"); }
        self.results.push(r);
    }

    /// 五角色覆盖。
    pub fn roles_covered(&self) -> bool {
        Role::ALL.iter().all(|r| self.participants.iter().any(|p| p.role == *r))
    }

    /// 域本色母语档：辅助技术用户须存在且语种为母语标记（非空）。
    pub fn assist_tech_native(&self) -> bool {
        self.participants.iter().any(|p| p.role == Role::AssistTechUser && !p.lang.is_empty())
    }

    /// 禁录名单：未同意参与者。
    pub fn forbid_record(&self) -> Vec<&'static str> {
        self.participants.iter().filter(|p| !p.recordable()).map(|p| p.role.name()).collect()
    }

    /// RTL 体验全员过线。
    pub fn rtl_all_ok(&self) -> bool { self.results.iter().all(|r| r.rtl_experienced()) }

    /// 方案可执行：五角色覆盖 + 无不可达脚本。
    pub fn executable(&self) -> bool {
        self.roles_covered() && !self.cases.contains(&"task-unreachable")
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves387_checks() -> CheckSet {
    let mut set = CheckSet::new("ves387");
    let mut plan = TestPlan::new();

    // C387-01 五角色参与者注册（语种定制）。
    let langs = ["ar", "ar", "zh", "de", "fr"];
    for (i, r) in Role::ALL.iter().enumerate() {
        plan.add_participant(Participant { role: *r, lang: langs[i], consented: true });
    }
    chk(&mut set, "C387-01", plan.roles_covered(), "五角色覆盖");

    // C387-02 脚本语种定制：脚本须配参与者语种。
    let s_ar = TaskScript { task: "t-rtl-mirror", lang: "ar", reachable: true };
    let s_fr = TaskScript { task: "t-locale-fmt", lang: "fr", reachable: true };
    let p_rtl = &plan.participants[1];
    let p_ops = &plan.participants[4];
    chk(&mut set, "C387-02", s_ar.fits(p_rtl) && s_fr.fits(p_ops) && !s_ar.fits(p_ops), "语种定制匹配");

    // C387-03 任务不可达 → 立案。
    plan.add_script(TaskScript { task: "t-broken-flow", lang: "ar", reachable: false });
    chk(&mut set, "C387-03", plan.cases.contains(&"task-unreachable"), "不可达立案");

    // C387-04 方案不可执行（有不可达脚本）。
    chk(&mut set, "C387-04", !plan.executable(), "立案阻断执行");

    // C387-05 知情同意：未同意 → 禁录。
    let mut plan2 = TestPlan::new();
    plan2.add_participant(Participant { role: Role::NativeReviewer, lang: "ja", consented: false });
    plan2.add_participant(Participant { role: Role::AssistTechUser, lang: "ko", consented: true });
    chk(&mut set, "C387-05", plan2.forbid_record() == vec!["native"]
        && plan2.participants[1].recordable(), "同意缺禁录");

    // C387-06 自然度评分：低于 70 → 回 T04 复审立案。
    plan2.add_result(TrialResult { role: Role::NativeReviewer, naturalness: 55, rtl_ok: None });
    chk(&mut set, "C387-06", plan2.cases.contains(&"naturalness-low"), "自然度低立案");

    // C387-07 自然度过线（70 及以上）。
    plan2.add_result(TrialResult { role: Role::CjkUser, naturalness: 70, rtl_ok: None });
    chk(&mut set, "C387-07", plan2.results[1].naturalness_ok(), "70 分压线过");

    // C387-08 RTL 体验断言：RtlUser 档必须 rtl_ok。
    chk(&mut set, "C387-08", TrialResult { role: Role::RtlUser, naturalness: 90, rtl_ok: Some(true) }.rtl_experienced()
        && !TrialResult { role: Role::RtlUser, naturalness: 90, rtl_ok: Some(false) }.rtl_experienced()
        && TrialResult { role: Role::CjkUser, naturalness: 90, rtl_ok: None }.rtl_experienced(), "RTL 档强制断言");

    // C387-09 域本色母语档：辅助技术用户在册。
    chk(&mut set, "C387-09", plan2.assist_tech_native(), "辅助技术母语档在册");

    // C387-10 五角色名单单源。
    chk(&mut set, "C387-10", Role::ALL.len() == 5 && Role::ALL[0].name() == "native"
        && Role::ALL[3].name() == "assist-tech" && Role::ALL[4].name() == "locale-ops", "五角色单源");

    // C387-11 立案幂等。
    plan2.add_result(TrialResult { role: Role::NativeReviewer, naturalness: 40, rtl_ok: None });
    let n_low = plan2.cases.iter().filter(|c| **c == "naturalness-low").count();
    chk(&mut set, "C387-11", n_low == 1, "立案不重复");

    // C387-12 全员 RTL 过线判定（含非 RTL 档豁免）。
    chk(&mut set, "C387-12", plan2.rtl_all_ok(), "非 RTL 档豁免全过");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves387_smoke() {
        let set = run_ves387_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves387 red: {:?}", red);
    }

    #[test]
    fn ves387_consent_gate() {
        let p = Participant { role: Role::RtlUser, lang: "ar", consented: false };
        assert!(!p.recordable());
    }
}
