//! ves389_s21apiref — VE-F4149 · i18n API 参考
//! T 域对外 API 面参考册：四族 API（格式化/切换/资源/审查）× 三件套（参数域钳制+示例+错误码）
//! + 无障碍文本接口族（域本色第五族）+ 与 F4066 清点式对账（API 面 vs 文档一致性断言）；
//! 文档漂移→对账修正留痕；参数越域→钳制标注；错误码缺→补齐。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xEAC0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArCode {
    Ok = 0,
    DocDrift = SEG_BASE + 0x01,
    ParamOutOfRange = SEG_BASE + 0x02,
    ErrCodeMissing = SEG_BASE + 0x03,
}

/// 五族 API（四核心 + 域本色无障碍文本接口族）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiFamily { Format = 0, Switch = 1, Resource = 2, Review = 3, A11yText = 4 }

impl ApiFamily {
    pub const CORE: [ApiFamily; 4] =
        [ApiFamily::Format, ApiFamily::Switch, ApiFamily::Resource, ApiFamily::Review];
    pub fn name(self) -> &'static str {
        match self {
            ApiFamily::Format => "format", ApiFamily::Switch => "switch",
            ApiFamily::Resource => "resource", ApiFamily::Review => "review",
            ApiFamily::A11yText => "a11y-text",
        }
    }
}

/// API 条目：族 + 参数域钳制 + 示例 + 错误码（三件套）。
pub struct ApiEntry {
    pub name: &'static str,
    pub family: ApiFamily,
    pub param_range: (i32, i32),  // 参数域钳制
    pub example: Option<&'static str>,
    pub err_codes: u8,            // 错误码数量（0 = 缺）
}

impl ApiEntry {
    /// 参数域钳制：越域值钳到边界。
    pub fn clamp(&self, v: i32) -> (i32, bool) {
        if v < self.param_range.0 { (self.param_range.0, true) }
        else if v > self.param_range.1 { (self.param_range.1, true) }
        else { (v, false) }
    }
    /// 三件套齐备：示例 + 错误码。
    pub fn triple_complete(&self) -> bool { self.example.is_some() && self.err_codes > 0 }
}

/// 参考册：条目 + 对账记录。
pub struct ApiRefBook {
    pub entries: Vec<ApiEntry>,
    pub fixes: Vec<&'static str>, // 对账修正留痕
}

impl ApiRefBook {
    pub fn new() -> Self { ApiRefBook { entries: Vec::new(), fixes: Vec::new() } }

    pub fn put(&mut self, e: ApiEntry) {
        match self.entries.iter_mut().find(|x| x.name == e.name) {
            Some(x) => *x = e,
            None => self.entries.push(e),
        }
    }

    pub fn get(&self, name: &str) -> Option<&ApiEntry> {
        self.entries.iter().find(|x| x.name == name)
    }

    /// 族覆盖：四核心族（+可选域本色族）全部有至少一条。
    pub fn families_covered(&self, with_a11y: bool) -> bool {
        let fams: Vec<ApiFamily> = if with_a11y {
            vec![ApiFamily::Format, ApiFamily::Switch, ApiFamily::Resource,
                 ApiFamily::Review, ApiFamily::A11yText]
        } else {
            ApiFamily::CORE.to_vec()
        };
        fams.iter().all(|f| self.entries.iter().any(|e| e.family == *f))
    }

    /// 清点式对账：文档面（名单）与册面对拍，漂移 → 修正留痕。
    pub fn reconcile(&mut self, doc_names: &[&'static str]) -> usize {
        let mut drifts = 0;
        for n in doc_names {
            if self.get(n).is_none() {
                self.fixes.push("doc-only");
                drifts += 1;
            }
        }
        for e in self.entries.iter() {
            if !doc_names.contains(&e.name) {
                self.fixes.push("code-only");
                drifts += 1;
            }
        }
        drifts
    }

    /// 错误码缺 → 补齐。
    pub fn fill_err_codes(&mut self, name: &str, n: u8) -> bool {
        match self.entries.iter_mut().find(|x| x.name == name) {
            Some(e) => { e.err_codes = n; true }
            None => false,
        }
    }

    /// 三件套全齐核验。
    pub fn all_triples(&self) -> bool { self.entries.iter().all(|e| e.triple_complete()) }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves389_checks() -> CheckSet {
    let mut set = CheckSet::new("ves389");
    let mut book = ApiRefBook::new();

    // C389-01 四族条目注册（格式化/切换/资源/审查）。
    book.put(ApiEntry { name: "fmt.date", family: ApiFamily::Format, param_range: (0, 6), example: Some("fmt.date(2)"), err_codes: 2 });
    book.put(ApiEntry { name: "sw.lang", family: ApiFamily::Switch, param_range: (0, 1), example: Some("sw.lang(1)"), err_codes: 1 });
    book.put(ApiEntry { name: "res.load", family: ApiFamily::Resource, param_range: (0, 9), example: None, err_codes: 3 });
    book.put(ApiEntry { name: "rv.scan", family: ApiFamily::Review, param_range: (0, 4), example: Some("rv.scan(0)"), err_codes: 0 });
    chk(&mut set, "C389-01", book.families_covered(false) && book.entries.len() == 4, "四族覆盖");

    // C389-02 域本色第五族（无障碍文本接口族）。
    book.put(ApiEntry { name: "a11y.read", family: ApiFamily::A11yText, param_range: (0, 3), example: Some("a11y.read(0)"), err_codes: 1 });
    chk(&mut set, "C389-02", book.families_covered(true), "五族含 a11y-text");

    // C389-03 参数域钳制：界内原样、界外钳回。
    let fmt = book.get("fmt.date").unwrap();
    let boundary_eq = fmt.clamp(0) == (0, false) && fmt.clamp(6) == (6, false);
    chk(&mut set, "C389-03", fmt.clamp(3) == (3, false) && fmt.clamp(-2) == (0, true)
        && fmt.clamp(9) == (6, true), "钳制三态");
    drop(fmt);

    // C389-04 三件套缺检：示例缺 + 错误码缺。
    chk(&mut set, "C389-04", !book.get("res.load").unwrap().triple_complete()
        && !book.get("rv.scan").unwrap().triple_complete(), "两处三件套缺");

    // C389-05 错误码补齐。
    book.fill_err_codes("rv.scan", 2);
    chk(&mut set, "C389-05", book.get("rv.scan").unwrap().err_codes == 2, "错误码补齐");

    // C389-06 补示例后全册三件套齐。
    book.put(ApiEntry { name: "res.load", family: ApiFamily::Resource, param_range: (0, 9), example: Some("res.load(0)"), err_codes: 3 });
    chk(&mut set, "C389-06", book.all_triples(), "全册三件套齐");

    // C389-07 清点式对账：文档漂移检出（doc-only）。
    let drifts = book.reconcile(&["fmt.date", "sw.lang", "res.load", "rv.scan", "a11y.read", "ghost.api"]);
    chk(&mut set, "C389-07", drifts == 1 && book.fixes.contains(&"doc-only"), "doc-only 漂移一");

    // C389-08 code-only 漂移（册面多出）。
    book.put(ApiEntry { name: "extra.api", family: ApiFamily::Format, param_range: (0, 1), example: Some("x"), err_codes: 1 });
    let d2 = book.reconcile(&["fmt.date", "sw.lang", "res.load", "rv.scan", "a11y.read"]);
    chk(&mut set, "C389-08", d2 == 1 && book.fixes.contains(&"code-only"), "code-only 漂移一");

    // C389-09 对账零漂移（文档与册面一致）。
    book.entries.retain(|e| e.name != "extra.api");
    let d3 = book.reconcile(&["fmt.date", "sw.lang", "res.load", "rv.scan", "a11y.read"]);
    chk(&mut set, "C389-09", d3 == 0, "对账归零");

    // C389-10 族名单单源（含域本色族名）。
    chk(&mut set, "C389-10", ApiFamily::CORE.len() == 4 && ApiFamily::CORE[0].name() == "format"
        && ApiFamily::CORE[3].name() == "review" && ApiFamily::A11yText.name() == "a11y-text", "五族单源");

    // C389-11 钳制边界等值不标越域（边界值在 C389-03 处已物化）。
    chk(&mut set, "C389-11", boundary_eq, "边界等值不算越域");

    // C389-12 覆盖登记幂等：同名 put 覆盖不增行。
    let before = book.entries.len();
    book.put(ApiEntry { name: "fmt.date", family: ApiFamily::Format, param_range: (0, 6), example: Some("fmt.date(2)"), err_codes: 2 });
    chk(&mut set, "C389-12", book.entries.len() == before, "put 幂等");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves389_smoke() {
        let set = run_ves389_checks();
        assert!(set.failed_count() == 0, "ves389 red: {}", set.red_items_text());
    }

    #[test]
    fn ves389_clamp_only() {
        let e = ApiEntry { name: "x", family: ApiFamily::Format, param_range: (1, 2), example: None, err_codes: 0 };
        assert_eq!(e.clamp(1), (1, false));
        assert_eq!(e.clamp(0), (1, true));
    }
}
