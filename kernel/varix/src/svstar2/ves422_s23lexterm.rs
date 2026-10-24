//! ves422_s23lexterm — VE-F4182 · T 域交互词典与一致性终审
//! 终审：T 域交互词典冻结（切换/回退/格式/翻译/地区五类术语+操作定义）；
//! 一致性终审（八组用法与词典逐条比对）；与家族词典（S 版 F3982）对拍；
//! 用法越词→改词或改用法（二选一留痕）；词典冲突→仲裁；冻结后发现→变更纪律（F4196）；
//! 词典含多语言朗读措辞规范（域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1000;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LxCode {
    Ok = 0,
    OutOfWord = SEG_BASE + 0x01,   // 0x1001
    Conflict = SEG_BASE + 0x02,    // 0x1002
    NotFrozen = SEG_BASE + 0x03,   // 0x1003
    WordMissing = SEG_BASE + 0x04, // 0x1004
}

/// 词典五类术语。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TermKind { Switch = 0, Fallback = 1, Format = 2, Translate = 3, Region = 4 }

impl TermKind {
    pub const ALL: [TermKind; 5] = [
        TermKind::Switch, TermKind::Fallback, TermKind::Format, TermKind::Translate, TermKind::Region];
}

/// 词典条目：词 → 类别 + 操作定义 + 朗读措辞（域本色）。
#[derive(Clone)]
pub struct LexEntry {
    pub word: String,
    pub kind: TermKind,
    pub op_def: String,
    pub sr_wording: String, // 多语言朗读措辞规范（域本色）
}

/// T 域交互词典（冻结态）。
pub struct Lexicon {
    pub entries: Vec<LexEntry>,
    pub frozen: bool,
}

impl Lexicon {
    pub fn new() -> Self { Lexicon { entries: Vec::new(), frozen: false } }
    pub fn put(&mut self, word: &str, kind: TermKind, op: &str, sr: &str) {
        self.entries.push(LexEntry {
            word: String::from(word), kind, op_def: String::from(op), sr_wording: String::from(sr),
        });
    }
    pub fn has(&self, word: &str) -> bool { self.entries.iter().any(|e| e.word == word) }
    /// 冻结前置：五类术语各至少一条且朗读措辞非空。
    pub fn freeze(&mut self) -> Result<(), LxCode> {
        for k in TermKind::ALL {
            if !self.entries.iter().any(|e| e.kind == k) { return Err(LxCode::WordMissing); }
        }
        if self.entries.iter().any(|e| e.sr_wording.is_empty()) { return Err(LxCode::NotFrozen); }
        self.frozen = true;
        Ok(())
    }
}

/// 八组用法比对：用法引用的词必须在冻结词典内。
pub struct UsageBook { pub groups: [Vec<String>; 8] }

impl UsageBook {
    pub fn audit(&self, lex: &Lexicon) -> Vec<(usize, String)> {
        let mut bad = Vec::new();
        for (gi, g) in self.groups.iter().enumerate() {
            for w in g {
                if !lex.has(w) { bad.push((gi, w.clone())); }
            }
        }
        bad
    }
}

/// 词典冲突仲裁：同词双定义→按优先级（后定义者需显式声明覆盖）。
pub fn arbitrate<'a>(a: &'a str, b: &'a str, override_declared: bool) -> Result<&'a str, LxCode> {
    if a == b { return Ok(a); }
    if override_declared { Ok(b) } else { Err(LxCode::Conflict) }
}

/// 家族词典对拍：S 版（F3982）与本域共词集合一致性。
pub fn family_agrees(own: &Lexicon, s_words: &[&str]) -> bool {
    s_words.iter().all(|w| own.has(w))
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves422_checks() -> CheckSet {
    let mut set = CheckSet::new("ves422");
    let mut lex = Lexicon::new();

    // C422-01 五类术语缺类拒冻。
    lex.put("切换语言", TermKind::Switch, "reload-ui", "switch to language");
    chk(&mut set, "C422-01", lex.freeze() == Err(LxCode::WordMissing), "缺类拒冻");

    // C422-02 朗读措辞为空拒冻（域本色）。
    lex.put("回退语言", TermKind::Fallback, "use-fallback", "");
    lex.put("日期格式", TermKind::Format, "fmt-date", "date format");
    lex.put("机翻", TermKind::Translate, "mt-flag", "machine translated");
    lex.put("地区单位", TermKind::Region, "region-unit", "regional unit");
    chk(&mut set, "C422-02", lex.freeze() == Err(LxCode::NotFrozen), "空朗读措辞拒");

    // C422-03 补齐措辞后冻结通过。
    lex.entries[1].sr_wording = String::from("fallback language");
    chk(&mut set, "C422-03", lex.freeze().is_ok() && lex.frozen, "五类齐冻结");

    // C422-04 八组用法逐条比对全过。
    let mut ub = UsageBook { groups: [vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![]] };
    for i in 0..8 { ub.groups[i].push(String::from("切换语言")); }
    chk(&mut set, "C422-04", ub.audit(&lex).is_empty(), "八组比对全过");

    // C422-05 越词定位（组号+词）。
    ub.groups[3].push(String::from("生造词"));
    let bad = ub.audit(&lex);
    chk(&mut set, "C422-05", bad.len() == 1 && bad[0].0 == 3 && bad[0].1 == "生造词", "越词定位");

    // C422-06 冻结态下 has 查询。
    chk(&mut set, "C422-06", lex.has("地区单位") && !lex.has("不存在"), "词典查询");

    // C422-07 词典冲突未声明覆盖→Err。
    chk(&mut set, "C422-07", arbitrate("旧义", "新义", false) == Err(LxCode::Conflict), "冲突需仲裁");

    // C422-08 显式声明覆盖→采新义（二选一留痕路径）。
    chk(&mut set, "C422-08", arbitrate("旧义", "新义", true) == Ok("新义"), "覆盖留痕采新");

    // C422-09 同词同义直接通过。
    chk(&mut set, "C422-09", arbitrate("同词", "同词", false) == Ok("同词"), "同词无冲突");

    // C422-10 家族对拍：S 版共词全部在册。
    chk(&mut set, "C422-10", family_agrees(&lex, &["切换语言", "回退语言"]), "家族共词一致");

    // C422-11 家族对拍：S 版词缺于本域→失配。
    chk(&mut set, "C422-11", !family_agrees(&lex, &["切换语言", "S 版专有词"]), "家族共词失配");

    // C422-12 五类单源齐备断言。
    chk(&mut set, "C422-12", TermKind::ALL.len() == 5
        && TermKind::ALL[0] == TermKind::Switch
        && TermKind::ALL[4] == TermKind::Region, "五类单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves422_smoke() {
        let set = run_ves422_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves422 red: {:?}", red);
    }
}
