//! ves479_s26interact — VE-F4245 · 交互一致性验证
//! 交互词典规则运行验证：快捷键/浮层出路/焦点归还三族行为脚本化验证（S 域标准平台化执行）+
//! 脚本库（词典条目→验证脚本单源转译）；脚本与词典失配→重转译；行为不符→发现项+回 S 域；环境漂移→重跑；
//! 含键盘全程可达脚本族（域本色硬线）；脚本数据合成。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1B00;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IaCode {
    Ok = 0,
    ScriptMismatch = SEG_BASE + 0x01, // 0x1B01
    BehaviorFail = SEG_BASE + 0x02,   // 0x1B02
    EnvDrift = SEG_BASE + 0x03,       // 0x1B03
}

/// 三族行为脚本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScriptFamily { Hotkey = 0, LayerEscape = 1, FocusReturn = 2, KeyboardReach = 3 } // 第 4 族=域本色硬线

impl ScriptFamily {
    pub const ALL: [ScriptFamily; 4] = [
        ScriptFamily::Hotkey, ScriptFamily::LayerEscape, ScriptFamily::FocusReturn, ScriptFamily::KeyboardReach];
}

/// 验证脚本（词典条目单源转译）。
pub struct Script { pub family: ScriptFamily, pub lex_rev: u64, pub script_rev: u64, pub behavior_ok: bool }

/// 交互验证册。
pub struct InteractBook {
    pub scripts: Vec<Script>,
    pub lex_rev: u64, // 词典版本（单源）
    pub findings: Vec<ScriptFamily>,
    pub reruns: u32,
}

impl InteractBook {
    pub fn new() -> Self { InteractBook { scripts: Vec::new(), lex_rev: 1, findings: Vec::new(), reruns: 0 } }
    pub fn put(&mut self, s: Script) {
        self.scripts.retain(|x| x.family != s.family);
        self.scripts.push(s);
    }
    /// 单源转译核验：脚本版本须与词典版本一致（失配→重转译）。
    pub fn sync_check(&mut self) -> Result<(), IaCode> {
        let mut stale = false;
        for s in self.scripts.iter_mut() {
            if s.script_rev != self.lex_rev { s.script_rev = self.lex_rev; stale = true; }
        }
        if stale { Err(IaCode::ScriptMismatch) } else { Ok(()) }
    }
    /// 行为执行：不符→发现项+回 S 域。
    pub fn execute(&mut self) -> Result<(), IaCode> {
        for s in &self.scripts {
            if !s.behavior_ok && !self.findings.contains(&s.family) {
                self.findings.push(s.family);
            }
        }
        if self.findings.is_empty() { Ok(()) } else { Err(IaCode::BehaviorFail) }
    }
    /// 环境漂移重跑。
    pub fn rerun(&mut self) { self.reruns += 1; }
    /// 域本色硬线：键盘全程可达脚本族必须在册且行为达标。
    pub fn keyboard_hardline(&self) -> bool {
        self.scripts.iter().any(|s| s.family == ScriptFamily::KeyboardReach && s.behavior_ok)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves479_checks() -> CheckSet {
    let mut set = CheckSet::new("ves479");
    let mut bk = InteractBook::new();
    for (f, ok) in [(ScriptFamily::Hotkey, true), (ScriptFamily::LayerEscape, true),
                    (ScriptFamily::FocusReturn, true), (ScriptFamily::KeyboardReach, true)] {
        bk.put(Script { family: f, lex_rev: 1, script_rev: 1, behavior_ok: ok });
    }

    // C479-01 四族脚本在册+单源转译一致。
    chk(&mut set, "C479-01", bk.scripts.len() == 4 && bk.sync_check().is_ok(), "四族单源一致");

    // C479-02 行为执行全过。
    chk(&mut set, "C479-02", bk.execute().is_ok(), "行为全过");

    // C479-03 词典更新→重转译（失配检出+自动对齐）。
    bk.lex_rev = 2;
    chk(&mut set, "C479-03", bk.sync_check() == Err(IaCode::ScriptMismatch)
        && bk.scripts.iter().all(|s| s.script_rev == 2), "重转译对齐");

    // C479-04 行为不符→发现项+回 S 域。
    bk.scripts[1].behavior_ok = false;
    chk(&mut set, "C479-04", bk.execute() == Err(IaCode::BehaviorFail)
        && bk.findings == vec![ScriptFamily::LayerEscape], "不符发现");

    // C479-05 修复后复执行清发现。
    bk.scripts[1].behavior_ok = true;
    bk.findings.clear();
    chk(&mut set, "C479-05", bk.execute().is_ok(), "修复复执行");

    // C479-06 环境漂移重跑。
    bk.rerun();
    chk(&mut set, "C479-06", bk.reruns == 1, "漂移重跑");

    // C479-07 域本色硬线：键盘可达族达标。
    chk(&mut set, "C479-07", bk.keyboard_hardline(), "键盘硬线");

    // C479-08 硬线破坏检出。
    bk.scripts[3].behavior_ok = false;
    chk(&mut set, "C479-08", !bk.keyboard_hardline(), "硬线破坏");
    bk.scripts[3].behavior_ok = true;

    // C479-09 四族名单单源。
    chk(&mut set, "C479-09", ScriptFamily::ALL.len() == 4
        && ScriptFamily::ALL[3] == ScriptFamily::KeyboardReach, "四族单源");

    // C479-10 脚本覆盖幂等。
    bk.put(Script { family: ScriptFamily::Hotkey, lex_rev: 2, script_rev: 2, behavior_ok: true });
    chk(&mut set, "C479-10", bk.scripts.len() == 4, "脚本幂等");

    // C479-11 发现项幂等。
    bk.scripts[0].behavior_ok = false;
    bk.execute().unwrap_err();
    bk.execute().unwrap_err();
    chk(&mut set, "C479-11", bk.findings.len() == 1, "发现幂等");

    // C479-12 错误码段位单源。
    chk(&mut set, "C479-12", IaCode::BehaviorFail as u16 == SEG_BASE + 2, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves479_smoke() {
        let set = run_ves479_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves479 red: {:?}", red);
    }
}
