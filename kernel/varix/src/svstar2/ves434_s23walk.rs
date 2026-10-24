//! ves434_s23walk — VE-F4194 · T 域真实走查
//! 走查：T 域真实走查执行——三套剧本真实用户走查（RTL 母语者/CJK 母语者/辅助技术用户——F4175 预演转正式）
//! +录证采集+走查表终填+发现分级（阻断/重要/建议）；
//! 阻断发现→回 F4172 链路+复走；不可达任务→最高级立案；同意缺→禁录；
//! 走查即域本色终验（真实母语用户复核）；录证全程知情同意+脱敏。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1300;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WkCode {
    Ok = 0,
    ConsentMissing = SEG_BASE + 0x01, // 0x1301
    TaskUnreachable = SEG_BASE + 0x02, // 0x1302
    BlockerFound = SEG_BASE + 0x03,  // 0x1303
}

/// 三套剧本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScriptKind { RtlNative = 0, CjkNative = 1, AssistTech = 2 }

impl ScriptKind {
    pub const ALL: [ScriptKind; 3] = [ScriptKind::RtlNative, ScriptKind::CjkNative, ScriptKind::AssistTech];
}

/// 发现分级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity { Suggestion = 0, Major = 1, Blocker = 2 }

/// 走查会话（真实用户）。
pub struct WalkSession {
    pub script: ScriptKind,
    pub consented: bool,     // 知情同意（缺→禁录）
    pub sanitized: bool,     // 录证脱敏
    pub tasks_done: usize,
    pub tasks_total: usize,
    pub findings: Vec<Severity>,
    pub rewalked: bool,
}

/// 走查执行册。
pub struct WalkBook { pub sessions: Vec<WalkSession>, pub filed_critical: u32, pub rewalks: u32 }

impl WalkBook {
    pub fn new() -> Self { WalkBook { sessions: Vec::new(), filed_critical: 0, rewalks: 0 } }
    pub fn add(&mut self, script: ScriptKind, consent: bool) {
        self.sessions.push(WalkSession { script, consented: consent, sanitized: false,
            tasks_done: 0, tasks_total: 0, findings: Vec::new(), rewalked: false });
    }
    /// 录证采集：同意缺→禁录（域本色红线：全程知情同意）。
    pub fn record(&mut self, script: ScriptKind) -> Result<(), WkCode> {
        let s = self.sessions.iter_mut().find(|s| s.script == script).ok_or(WkCode::TaskUnreachable)?;
        if !s.consented { return Err(WkCode::ConsentMissing); }
        s.sanitized = true;
        Ok(())
    }
    pub fn fill(&mut self, script: ScriptKind, done: usize, total: usize, findings: Vec<Severity>) -> Result<(), WkCode> {
        let s = self.sessions.iter_mut().find(|s| s.script == script).ok_or(WkCode::TaskUnreachable)?;
        s.tasks_done = done;
        s.tasks_total = total;
        s.findings = findings;
        if done < total { self.filed_critical += 1; return Err(WkCode::TaskUnreachable); }
        Ok(())
    }
    /// 阻断发现检测。
    pub fn blockers(&self) -> Vec<ScriptKind> {
        self.sessions.iter().filter(|s| s.findings.contains(&Severity::Blocker))
            .map(|s| s.script).collect()
    }
    /// 阻断→回 F4172 链路复走。
    pub fn rewalk(&mut self, script: ScriptKind, new_findings: Vec<Severity>) -> bool {
        self.rewalks += 1;
        for s in self.sessions.iter_mut() {
            if s.script == script { s.findings = new_findings.clone(); s.rewalked = true; }
        }
        self.blockers().is_empty()
    }
    /// 终填核验：三剧本齐+全录证脱敏+零阻断。
    pub fn final_ok(&self) -> Result<(), WkCode> {
        if self.sessions.len() < 3 { return Err(WkCode::TaskUnreachable); }
        if self.sessions.iter().any(|s| !s.sanitized) { return Err(WkCode::ConsentMissing); }
        if !self.blockers().is_empty() { return Err(WkCode::BlockerFound); }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves434_checks() -> CheckSet {
    let mut set = CheckSet::new("ves434");
    let mut bk = WalkBook::new();
    for s in ScriptKind::ALL { bk.add(s, true); }
    bk.sessions[1].consented = false; // CJK 用户未同意

    // C434-01 同意缺→禁录（红线）。
    chk(&mut set, "C434-01", bk.record(ScriptKind::CjkNative) == Err(WkCode::ConsentMissing), "同意缺禁录");

    // C434-02 同意齐→录证+脱敏标记。
    bk.sessions[1].consented = true;
    for s in ScriptKind::ALL { bk.record(s).unwrap(); }
    chk(&mut set, "C434-02", bk.sessions.iter().all(|s| s.sanitized), "录证脱敏全真");

    // C434-03 走查表终填：任务全达。
    chk(&mut set, "C434-03", bk.fill(ScriptKind::RtlNative, 8, 8, vec![Severity::Suggestion]).is_ok(), "RTL 全达");

    // C434-04 不可达任务→最高级立案。
    chk(&mut set, "C434-04", bk.fill(ScriptKind::CjkNative, 6, 8, vec![]) == Err(WkCode::TaskUnreachable)
        && bk.filed_critical == 1, "不可达立案");

    // C434-05 补走全达复填。
    chk(&mut set, "C434-05", bk.fill(ScriptKind::CjkNative, 8, 8, vec![Severity::Major]).is_ok(), "复填全达");

    // C434-06 阻断发现定位。
    bk.fill(ScriptKind::AssistTech, 8, 8, vec![Severity::Blocker]).unwrap();
    chk(&mut set, "C434-06", bk.blockers() == vec![ScriptKind::AssistTech], "阻断定位");

    // C434-07 终填核验：有阻断→拒。
    chk(&mut set, "C434-07", bk.final_ok() == Err(WkCode::BlockerFound), "阻断拒收官");

    // C434-08 阻断→回 F4172 链路复走复绿。
    chk(&mut set, "C434-08", bk.rewalk(ScriptKind::AssistTech, vec![Severity::Major])
        && bk.rewalks == 1, "复走复绿");

    // C434-09 复走后终填核验全过。
    chk(&mut set, "C434-09", bk.final_ok().is_ok(), "终填全过");

    // C434-10 分级排序单源（建议 < 重要 < 阻断）。
    chk(&mut set, "C434-10", Severity::Suggestion < Severity::Major && Severity::Major < Severity::Blocker, "三级单源");

    // C434-11 三剧本单源断言。
    chk(&mut set, "C434-11", ScriptKind::ALL.len() == 3
        && ScriptKind::ALL[0] == ScriptKind::RtlNative
        && ScriptKind::ALL[2] == ScriptKind::AssistTech, "三剧本单源");

    // C434-12 复走标记留痕。
    chk(&mut set, "C434-12", bk.sessions.iter().find(|s| s.script == ScriptKind::AssistTech).unwrap().rewalked
        && !bk.sessions.iter().find(|s| s.script == ScriptKind::RtlNative).unwrap().rewalked, "复走留痕");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves434_smoke() {
        let set = run_ves434_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves434 red: {:?}", red);
    }
}
