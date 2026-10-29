//! ves390_s21examples — VE-F4150 · i18n 示例库汇总
//! 四源归集（T04 翻译示例 + T05 资源示例 + T06 地区示例 + 综合示例）+ 可执行索引（跑通即验收）
//! + 示例与 API 参考交叉链接（链接断→修复）+ 源缺示例→登记补齐 + 示例语料虚构（红线）+ 读屏多语言示例（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xEB40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExCode {
    Ok = 0,
    RunFailed = SEG_BASE + 0x01,
    LinkBroken = SEG_BASE + 0x02,
    SourceGap = SEG_BASE + 0x03,
}

/// 四源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExampleSource { T04Translation = 0, T05Resource = 1, T06Locale = 2, Composite = 3 }

impl ExampleSource {
    pub const ALL: [ExampleSource; 4] = [
        ExampleSource::T04Translation, ExampleSource::T05Resource,
        ExampleSource::T06Locale, ExampleSource::Composite];
    pub fn name(self) -> &'static str {
        match self {
            ExampleSource::T04Translation => "t04-translation",
            ExampleSource::T05Resource => "t05-resource",
            ExampleSource::T06Locale => "t06-locale",
            ExampleSource::Composite => "composite",
        }
    }
}

/// 示例条目：可执行 + 交叉链接 + 域本色标记。
pub struct Example {
    pub name: &'static str,
    pub source: ExampleSource,
    pub runnable: bool,          // 可执行
    pub passed: bool,            // 跑通即验收
    pub links: Vec<&'static str>, // 交叉链接（指向 API 参考条目名）
    pub synthetic: bool,         // 语料虚构（红线：必须 true）
    pub screen_reader: bool,     // 读屏多语言示例（域本色）
}

impl Example {
    /// 验收 = 可执行且跑通且语料虚构。
    pub fn accepted(&self) -> bool { self.runnable && self.passed && self.synthetic }
}

/// 示例总库。
pub struct ExampleLib {
    pub items: Vec<Example>,
    pub source_gaps: Vec<&'static str>, // 源缺示例登记
}

impl ExampleLib {
    pub fn new() -> Self { ExampleLib { items: Vec::new(), source_gaps: Vec::new() } }

    pub fn put(&mut self, e: Example) {
        match self.items.iter_mut().find(|x| x.name == e.name) {
            Some(x) => *x = e,
            None => self.items.push(e),
        }
    }

    /// 四源覆盖：缺源 → 登记补齐。
    pub fn audit_sources(&mut self) -> Vec<&'static str> {
        let gaps: Vec<&'static str> = ExampleSource::ALL.iter()
            .filter(|s| !self.items.iter().any(|e| e.source == **s))
            .map(|s| s.name()).collect();
        for g in gaps.iter() {
            if !self.source_gaps.contains(g) { self.source_gaps.push(g); }
        }
        gaps
    }

    /// 可执行索引：全部可执行示例名（跑通即验收的索引行）。
    pub fn exec_index(&self) -> Vec<&'static str> {
        self.items.iter().filter(|e| e.runnable).map(|e| e.name).collect()
    }

    /// 跑挂示例即缺陷（登记名）。
    pub fn failed_ones(&self) -> Vec<&'static str> {
        self.items.iter().filter(|e| e.runnable && !e.passed).map(|e| e.name).collect()
    }

    /// 交叉链接断检：链接目标不在 API 参考册名单中。
    pub fn broken_links(&self, api_names: &[&str]) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for e in self.items.iter() {
            for l in e.links.iter() {
                if !api_names.contains(l) && !out.contains(l) { out.push(l); }
            }
        }
        out
    }

    /// 全库验收：全部条目过验收。
    pub fn all_accepted(&self) -> bool { self.items.iter().all(|e| e.accepted()) }

    /// 域本色：读屏多语言示例在库。
    pub fn has_screen_reader(&self) -> bool {
        self.items.iter().any(|e| e.screen_reader && e.synthetic)
    }
}

fn mk_ex(name: &'static str, src: ExampleSource, runnable: bool, passed: bool,
         links: Vec<&'static str>, sr: bool) -> Example {
    Example { name, source: src, runnable, passed, links, synthetic: true, screen_reader: sr }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves390_checks() -> CheckSet {
    let mut set = CheckSet::new("ves390");
    let mut lib = ExampleLib::new();

    // C390-01 四源逐条归集。
    lib.put(mk_ex("ex-translate", ExampleSource::T04Translation, true, true, vec!["fmt.date"], false));
    lib.put(mk_ex("ex-pack", ExampleSource::T05Resource, true, true, vec!["res.load"], false));
    lib.put(mk_ex("ex-locale", ExampleSource::T06Locale, true, true, vec!["sw.lang"], false));
    lib.put(mk_ex("ex-composite", ExampleSource::Composite, true, true, vec!["rv.scan"], true));
    chk(&mut set, "C390-01", lib.items.len() == 4 && lib.audit_sources().is_empty(), "四源齐");

    // C390-02 可执行索引。
    chk(&mut set, "C390-02", lib.exec_index().len() == 4
        && lib.exec_index()[0] == "ex-translate", "可执行索引四行");

    // C390-03 跑挂即缺陷。
    lib.put(mk_ex("ex-locale", ExampleSource::T06Locale, true, false, vec!["sw.lang"], false));
    chk(&mut set, "C390-03", lib.failed_ones() == vec!["ex-locale"], "跑挂登记");

    // C390-04 修复复跑通过。
    lib.put(mk_ex("ex-locale", ExampleSource::T06Locale, true, true, vec!["sw.lang"], false));
    chk(&mut set, "C390-04", lib.failed_ones().is_empty() && lib.all_accepted(), "复跑全过");

    // C390-05 交叉链接断检。
    let api = ["fmt.date", "res.load", "sw.lang"];
    chk(&mut set, "C390-05", lib.broken_links(&api) == vec!["rv.scan"], "断链接定位");

    // C390-06 链接修复（改指有效 API）。
    lib.put(mk_ex("ex-composite", ExampleSource::Composite, true, true, vec!["fmt.date"], true));
    chk(&mut set, "C390-06", lib.broken_links(&api).is_empty(), "断链修复");

    // C390-07 源缺登记补齐：清空 T05 源后登记。
    lib.items.retain(|e| e.source != ExampleSource::T05Resource);
    chk(&mut set, "C390-07", lib.audit_sources() == vec!["t05-resource"]
        && lib.source_gaps.contains(&"t05-resource"), "源缺登记");

    // C390-08 补齐后源缺清零。
    lib.put(mk_ex("ex-pack2", ExampleSource::T05Resource, true, true, vec!["res.load"], false));
    chk(&mut set, "C390-08", lib.audit_sources().is_empty(), "补齐清零");

    // C390-09 语料虚构红线：非虚构示例不验收。
    let mut real = mk_ex("ex-real", ExampleSource::Composite, true, true, vec![], false);
    real.synthetic = false;
    lib.put(real);
    chk(&mut set, "C390-09", !lib.all_accepted(), "非虚构不验收");

    // C390-10 域本色：读屏多语言示例在库且虚构。
    chk(&mut set, "C390-10", lib.has_screen_reader(), "读屏示例在库");

    // C390-11 四源名单单源。
    chk(&mut set, "C390-11", ExampleSource::ALL.len() == 4
        && ExampleSource::ALL[0].name() == "t04-translation"
        && ExampleSource::ALL[3].name() == "composite", "四源单源");

    // C390-12 put 幂等覆盖。
    let before = lib.items.len();
    lib.put(mk_ex("ex-real", ExampleSource::Composite, true, true, vec![], false));
    chk(&mut set, "C390-12", lib.items.len() == before && !lib.all_accepted(), "覆盖不增行");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves390_smoke() {
        let set = run_ves390_checks();
        assert!(set.failed_count() == 0, "ves390 red: {}", set.red_items_text());
    }

    #[test]
    fn ves390_not_runnable_never_failed() {
        let mut lib = ExampleLib::new();
        lib.put(mk_ex("x", ExampleSource::Composite, false, false, vec![], false));
        assert!(lib.failed_ones().is_empty()); // 不可执行不算跑挂
        assert!(!lib.all_accepted());
    }
}
