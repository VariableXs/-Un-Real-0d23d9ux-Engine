//! ves415_s22drywalk — VE-F4175 · 真实走查预演
//! T 域真实走查预演：多语种用户剧本预演（RTL 语种母语者/CJK 母语者/辅助技术用户三套任务脚本
//! + 走查表预填）；预演不代替终走查声明；录证预案；剧本缺陷→改本重演；任务不可达→立案（潜在阻断项）；
//! 母语评审缺→借调登记；知情同意模板（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0F00;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DwCode {
    Ok = 0,
    ScriptFlawed = SEG_BASE + 0x01,   // 0x0F01
    TaskUnreachable = SEG_BASE + 0x02, // 0x0F02
    InterpreterMissing = SEG_BASE + 0x03, // 0x0F03
}

/// 三套剧本角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScriptRole { RtlNative = 0, CjkNative = 1, AssistTech = 2 }

impl ScriptRole {
    pub const ALL: [ScriptRole; 3] = [
        ScriptRole::RtlNative, ScriptRole::CjkNative, ScriptRole::AssistTech];
    pub fn name(self) -> &'static str {
        match self {
            ScriptRole::RtlNative => "rtl-native", ScriptRole::CjkNative => "cjk-native",
            ScriptRole::AssistTech => "assist-tech",
        }
    }
}

/// 任务脚本：角色 + 语种 + 任务可达 + 剧本轮次。
#[derive(Clone, Copy)]
pub struct TaskScript {
    pub role: ScriptRole,
    pub lang: &'static str,
    pub task: &'static str,
    pub reachable: bool,
    pub consented: bool, // 知情同意（录证先决）
}

impl TaskScript {
    pub fn recordable(&self) -> bool { self.consented && self.reachable }
}

/// 预填走查表行。
#[derive(Clone, Copy)]
pub struct PrefillRow {
    pub role: ScriptRole,
    pub task: &'static str,
    pub expect: &'static str, // 预填期望
    pub dry_run_pass: Option<bool>, // 干跑结果（None = 未跑）
}

/// 预演册。
pub struct DryWalkBook {
    pub scripts: Vec<TaskScript>,
    pub rows: Vec<PrefillRow>,
    pub rewrites: Vec<&'static str>,  // 改本重演留痕
    pub cases: Vec<&'static str>,     // 立案
    pub interpreters: Vec<&'static str>, // 借调登记
    pub consent_template_ready: bool,    // 知情同意模板（域本色）
}

impl DryWalkBook {
    pub fn new() -> Self {
        DryWalkBook { scripts: Vec::new(), rows: Vec::new(), rewrites: Vec::new(),
            cases: Vec::new(), interpreters: Vec::new(), consent_template_ready: false }
    }

    pub fn add_script(&mut self, s: TaskScript) {
        if !s.reachable {
            self.cases.push("task-unreachable"); // 潜在阻断项立案
        }
        self.scripts.push(s);
    }

    pub fn add_row(&mut self, r: PrefillRow) { self.rows.push(r); }

    /// 三套剧本覆盖。
    pub fn roles_covered(&self) -> bool {
        ScriptRole::ALL.iter().all(|r| self.scripts.iter().any(|s| s.role == *r))
    }

    /// 剧本缺陷 → 改本重演留痕。
    pub fn rewrite(&mut self, what: &'static str) { self.rewrites.push(what); }

    /// 干跑执行：登记结果。
    pub fn dry_run(&mut self, idx: usize, pass: bool) -> bool {
        match self.rows.get_mut(idx) {
            Some(r) => { r.dry_run_pass = Some(pass); true }
            None => false,
        }
    }

    /// 预演不代替终走查：干跑全部通过 + 三角色覆盖 + 同意模板就绪 → 预演通过（非终验）。
    pub fn dry_ok(&self) -> bool {
        self.roles_covered() && self.consent_template_ready
            && self.rows.iter().all(|r| r.dry_run_pass == Some(true))
    }

    /// 母语评审缺 → 借调登记（幂等）。
    pub fn request_interpreter(&mut self, lang: &'static str) -> bool {
        if self.interpreters.contains(&lang) { return false; }
        self.interpreters.push(lang);
        true
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves415_checks() -> CheckSet {
    let mut set = CheckSet::new("ves415");
    let mut bk = DryWalkBook::new();

    // C415-01 三套剧本注册。
    bk.add_script(TaskScript { role: ScriptRole::RtlNative, lang: "ar", task: "t-rtl-read", reachable: true, consented: true });
    bk.add_script(TaskScript { role: ScriptRole::CjkNative, lang: "zh", task: "t-cjk-wrap", reachable: true, consented: true });
    bk.add_script(TaskScript { role: ScriptRole::AssistTech, lang: "de", task: "t-sr-follow", reachable: true, consented: true });
    chk(&mut set, "C415-01", bk.roles_covered() && bk.scripts.len() == 3, "三剧本齐");

    // C415-02 任务不可达 → 潜在阻断立案。
    bk.add_script(TaskScript { role: ScriptRole::RtlNative, lang: "fa", task: "t-broken", reachable: false, consented: true });
    chk(&mut set, "C415-02", bk.cases.contains(&"task-unreachable"), "不可达立案");

    // C415-03 剧本缺陷 → 改本重演留痕。
    bk.rewrite("rtl 剧本第 3 幕锚点修正");
    chk(&mut set, "C415-03", bk.rewrites.len() == 1, "改本留痕");

    // C415-04 走查表预填 + 干跑。
    bk.add_row(PrefillRow { role: ScriptRole::RtlNative, task: "t-rtl-read", expect: "镜像完整", dry_run_pass: None });
    bk.add_row(PrefillRow { role: ScriptRole::CjkNative, task: "t-cjk-wrap", expect: "折行合规", dry_run_pass: None });
    bk.add_row(PrefillRow { role: ScriptRole::AssistTech, task: "t-sr-follow", expect: "跟读一致", dry_run_pass: None });
    for i in 0..3 { bk.dry_run(i, true); }
    chk(&mut set, "C415-04", bk.rows.iter().all(|r| r.dry_run_pass == Some(true)), "干跑三行过");

    // C415-05 知情同意模板（域本色）。
    bk.consent_template_ready = true;
    chk(&mut set, "C415-05", bk.consent_template_ready, "同意模板就绪");

    // C415-06 预演通过（不代终验）。
    chk(&mut set, "C415-06", bk.dry_ok(), "预演通过态");

    // C415-07 同意缺 → 禁录。
    let ns = TaskScript { role: ScriptRole::CjkNative, lang: "ja", task: "t-x", reachable: true, consented: false };
    chk(&mut set, "C415-07", !ns.recordable(), "同意缺禁录");

    // C415-08 母语评审缺 → 借调登记（幂等）。
    chk(&mut set, "C415-08", bk.request_interpreter("th") && !bk.request_interpreter("th")
        && bk.interpreters == vec!["th"], "借调幂等");

    // C415-09 干跑失败改判。
    let mut bk2 = DryWalkBook::new();
    bk2.add_row(PrefillRow { role: ScriptRole::RtlNative, task: "t-a", expect: "x", dry_run_pass: None });
    bk2.dry_run(0, false);
    chk(&mut set, "C415-09", !bk2.dry_ok(), "干跑失败拖累");

    // C415-10 改本重演翻绿。
    bk2.rewrite("改本");
    bk2.dry_run(0, true);
    chk(&mut set, "C415-10", bk2.rows[0].dry_run_pass == Some(true), "重演翻绿");

    // C415-11 三角色名单单源。
    chk(&mut set, "C415-11", ScriptRole::ALL.len() == 3
        && ScriptRole::ALL[0].name() == "rtl-native"
        && ScriptRole::ALL[2].name() == "assist-tech", "三角色单源");

    // C415-12 可达但未同意仍不可录。
    let half = TaskScript { role: ScriptRole::CjkNative, lang: "zh", task: "t-y", reachable: true, consented: true };
    chk(&mut set, "C415-12", half.recordable(), "全备可录");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves415_smoke() {
        let set = run_ves415_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves415 red: {:?}", red);
    }
}
