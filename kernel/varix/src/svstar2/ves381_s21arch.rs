//! ves381_s21arch — VE-F4141 · T08 开工与测试工具总纲
//! i18n 测试工具组定位：测试工具家族复用声明（S08 框架 + T08 特化 = 工具生态第二落点）；
//! DX 五标准复用执法（快/稳/可 diff/可集成/可扩展——T 版）；十七项范围映射。
//! 工具不达五标准→缺陷立案（红线实测）；映射缺→补；范围越界→退回；读屏多语言仿真（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xDCC0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TgCode {
    Ok = 0,
    StandardShort = SEG_BASE + 0x01,
    ScopeGap = SEG_BASE + 0x02,
    OutOfScope = SEG_BASE + 0x03,
    CaseOpened = SEG_BASE + 0x04,
}

/// DX 五标准（T 版复用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DxStandard { Fast = 0, Stable = 1, Diffable = 2, Integratable = 3, Extensible = 4 }

impl DxStandard {
    pub const ALL: [DxStandard; 5] = [
        DxStandard::Fast, DxStandard::Stable, DxStandard::Diffable,
        DxStandard::Integratable, DxStandard::Extensible];
    pub fn name(self) -> &'static str {
        match self {
            DxStandard::Fast => "fast", DxStandard::Stable => "stable",
            DxStandard::Diffable => "diffable", DxStandard::Integratable => "integratable",
            DxStandard::Extensible => "extensible",
        }
    }
}

/// 十七项范围映射表（单源常量）。
pub const SCOPE_ITEMS: [&'static str; 17] = [
    "automation", "linter", "simulator", "corpus", "regression", "user-test",
    "dashboard", "api-ref", "examples", "docs", "extension", "third-party",
    "compat", "fuzz-gate", "walkthrough", "assets", "evolution"];

/// 工具五标准审计：达标集合 + 立案册。
pub struct ToolAudit {
    pub tool: &'static str,
    pub met: Vec<DxStandard>,
    pub cases: Vec<&'static str>, // 立案（不达标准项名）
}

impl ToolAudit {
    pub fn new(tool: &'static str) -> Self { ToolAudit { tool, met: Vec::new(), cases: Vec::new() } }

    pub fn mark(&mut self, s: DxStandard) { if !self.met.contains(&s) { self.met.push(s); } }

    /// 执法审计：五标准逐条核，不达→立案（红线实测）。返回立案数。
    pub fn enforce(&mut self) -> usize {
        for s in DxStandard::ALL.iter() {
            if !self.met.contains(s) && !self.cases.contains(&s.name()) {
                self.cases.push(s.name());
            }
        }
        self.cases.len()
    }

    pub fn all_met(&self) -> bool { self.met.len() == 5 && self.cases.is_empty() }
}

/// 范围映射册：工具→范围项映射；缺项补齐；越界退回。
pub struct ScopeMap { pub entries: Vec<(&'static str, &'static str)> } // (工具, 范围项)

impl ScopeMap {
    pub fn new() -> Self { ScopeMap { entries: Vec::new() } }

    /// 挂映射；越界（范围项不在十七项表内）→ 退回。
    pub fn bind(&mut self, tool: &'static str, scope: &'static str) -> Result<(), TgCode> {
        if !SCOPE_ITEMS.contains(&scope) { return Err(TgCode::OutOfScope); }
        if self.entries.iter().any(|x| x.0 == tool) { return Err(TgCode::OutOfScope); }
        self.entries.push((tool, scope));
        Ok(())
    }

    /// 缺项：十七项中未映射的范围项。
    pub fn gaps(&self) -> Vec<&'static str> {
        SCOPE_ITEMS.iter()
            .filter(|s| !self.entries.iter().any(|x| x.1 == **s))
            .map(|s| *s).collect()
    }

    /// 补挂缺口。
    pub fn fill(&mut self, tool: &'static str, scope: &'static str) -> bool {
        if SCOPE_ITEMS.contains(&scope)
            && !self.entries.iter().any(|x| x.0 == tool)
            && self.gaps().contains(&scope) {
            self.entries.push((tool, scope));
            return true;
        }
        false
    }

    pub fn complete(&self) -> bool { self.gaps().is_empty() }
}

/// 第二落点声明：S08 框架单源 + T08 特化；读屏多语言仿真工具位（域本色）。
pub struct SecondLanding {
    pub framework_source: &'static str, // S08 单源锚
    pub specialization: Vec<&'static str>,
    pub screen_reader_sim: bool, // 读屏多语言仿真位（域本色）
}

impl SecondLanding {
    pub fn new() -> Self {
        SecondLanding {
            framework_source: "S08-framework",
            specialization: Vec::new(),
            screen_reader_sim: false,
        }
    }
    pub fn specialize(&mut self, what: &'static str) { self.specialization.push(what); }
    pub fn enable_screen_reader(&mut self) { self.screen_reader_sim = true; }
    /// 第二落点成立：框架单源锚定 + 特化非空 + 读屏仿真位开启。
    pub fn landed(&self) -> bool {
        self.framework_source == "S08-framework"
            && !self.specialization.is_empty()
            && self.screen_reader_sim
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves381_checks() -> CheckSet {
    let mut set = CheckSet::new("ves381");

    // C381-01 五标准表单源。
    chk(&mut set, "C381-01", DxStandard::ALL.len() == 5
        && DxStandard::ALL[0].name() == "fast" && DxStandard::ALL[4].name() == "extensible", "五标准单源");

    // C381-02 全达标工具执法零立案。
    let mut t1 = ToolAudit::new("sim-t");
    for s in DxStandard::ALL.iter() { t1.mark(*s); }
    chk(&mut set, "C381-02", t1.enforce() == 0 && t1.all_met(), "全达标零立案");

    // C381-03 缺两标准 → 立案两条（红线实测）。
    let mut t2 = ToolAudit::new("lint-t");
    t2.mark(DxStandard::Fast);
    t2.mark(DxStandard::Stable);
    t2.mark(DxStandard::Diffable);
    let n = t2.enforce();
    chk(&mut set, "C381-03", n == 2 && t2.cases.contains(&"integratable")
        && t2.cases.contains(&"extensible"), "缺两标准立案二");

    // C381-04 立案幂等：重跑不增条。
    let n2 = t2.enforce();
    chk(&mut set, "C381-04", n2 == 2, "立案幂等");

    // C381-05 十七项范围表单源。
    chk(&mut set, "C381-05", SCOPE_ITEMS.len() == 17 && SCOPE_ITEMS[0] == "automation"
        && SCOPE_ITEMS[16] == "evolution", "十七项单源");

    // C381-06 映射挂载：合法项通过。
    let mut sm = ScopeMap::new();
    chk(&mut set, "C381-06", sm.bind("t-autotest", "automation").is_ok()
        && sm.bind("t-linter", "linter").is_ok() && sm.entries.len() == 2, "合法映射过");

    // C381-07 越界退回：范围项不在表内。
    chk(&mut set, "C381-07", sm.bind("x-tool", "not-a-scope") == Err(TgCode::OutOfScope), "越界退回");

    // C381-08 缺项定位：十七项中未映射 15 项。
    chk(&mut set, "C381-08", sm.gaps().len() == 15 && sm.gaps()[0] == "simulator", "缺项定位");

    // C381-09 补挂缺口直至完备。
    let rest: Vec<&'static str> = sm.gaps();
    for (i, g) in rest.iter().enumerate() {
        let tool = match *g {
            "simulator" => "t-sim", "corpus" => "t-corpus", "regression" => "t-regress",
            "user-test" => "t-utest", "dashboard" => "t-board", "api-ref" => "t-apiref",
            "examples" => "t-ex", "docs" => "t-docs", "extension" => "t-ext",
            "third-party" => "t-3p", "compat" => "t-compat", "fuzz-gate" => "t-fuzz",
            "walkthrough" => "t-walk", "assets" => "t-asset", _ => "t-evo",
        };
        let _ = i;
        assert!(sm.fill(tool, g));
    }
    chk(&mut set, "C381-09", sm.complete() && sm.entries.len() == 17, "补齐十七项完备");

    // C381-10 重复工具名退回；fill 对已完备项无效。
    chk(&mut set, "C381-10", sm.bind("t-autotest", "linter").is_err() && !sm.fill("t-x", "linter"), "重复退回");

    // C381-11 第二落点：S08 单源 + 特化 + 读屏仿真位（域本色）。
    let mut sl = SecondLanding::new();
    sl.specialize("t-linter-rules");
    sl.specialize("t-corpus-grading");
    sl.enable_screen_reader();
    chk(&mut set, "C381-11", sl.landed() && sl.specialization.len() == 2, "第二落点成立");

    // C381-12 读屏仿真位未开 → 未落地。
    let mut sl2 = SecondLanding::new();
    sl2.specialize("t-sim-dims");
    chk(&mut set, "C381-12", !sl2.landed(), "缺读屏位未落地");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves381_smoke() {
        let set = run_ves381_checks();
        assert!(set.failed_count() == 0, "ves381 red: {}", set.red_items_text());
    }

    #[test]
    fn ves381_scope_count() {
        assert_eq!(SCOPE_ITEMS.len(), 17);
        assert!(SCOPE_ITEMS.contains(&"fuzz-gate"));
        assert!(SCOPE_ITEMS.contains(&"walkthrough"));
    }
}
