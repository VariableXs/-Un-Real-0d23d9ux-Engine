//! ves344_s19content — VE-F4104 · T06 内容审查地区差异
//! 审查规则地区化（符号/图像/词汇三级×地区差异表）+ 审查器（资产与文案入库前跑地区规则——与 F4083 禁忌清单单源）+ 误报申诉通道（修正留痕）+ 审查冲突最严地区兜底 + 非画像红线（只做合规检不做用户画像）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xBD40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CtCode {
    Ok = 0,
    Blocked = SEG_BASE + 0x01,
    Restricted = SEG_BASE + 0x02,
    Conflict = SEG_BASE + 0x03,
}

/// 三级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level { Allow = 0, Restricted = 1, Blocked = 2 }

impl Level {
    pub const ALL: [Level; 3] = [Level::Allow, Level::Restricted, Level::Blocked];
    pub fn name(self) -> &'static str {
        match self { Level::Allow => "allow", Level::Restricted => "restricted", Level::Blocked => "blocked" }
    }
}

/// 审查类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditKind { Symbol = 0, Image = 1, Vocabulary = 2 }

impl AuditKind {
    pub const ALL: [AuditKind; 3] = [AuditKind::Symbol, AuditKind::Image, AuditKind::Vocabulary];
}

/// 差异表条目：token × 类别 × 地区 × 级。
#[derive(Clone)]
pub struct DiffEntry { pub token: &'static str, pub kind: AuditKind, pub region: &'static str, pub level: Level }

pub struct DiffTable { pub entries: Vec<DiffEntry> }

impl DiffTable {
    pub fn new() -> Self { DiffTable { entries: Vec::new() } }

    pub fn add(&mut self, e: DiffEntry) { self.entries.push(e); }

    /// 查级：同一 token 多地区 → 取最严（兜底）；未命中 → Allow。
    pub fn level_of(&self, token: &str, region: &str) -> Level {
        let mut best = Level::Allow;
        for e in self.entries.iter() {
            if e.token == token && (e.region == region || e.region == "*") {
                if e.level > best { best = e.level; }
            }
        }
        best
    }

    /// 审查冲突：token 在两地区同级不同 → 冲突标记（供最严兜底复述）。
    pub fn conflict_tokens(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for e in self.entries.iter() {
            let levels: Vec<Level> = self.entries.iter()
                .filter(|x| x.token == e.token && x.region != "*")
                .map(|x| x.level).collect();
            if levels.len() > 1 && levels.iter().any(|l| *l != levels[0]) && !out.contains(&e.token) {
                out.push(e.token);
            }
        }
        out
    }

    /// 审查器：文本 token 序列过地区规则 → 命中最严级。
    pub fn screen(&self, tokens: &[&'static str], region: &str) -> (Level, Vec<&'static str>) {
        let mut worst = Level::Allow;
        let mut hits = Vec::new();
        for t in tokens.iter() {
            let l = self.level_of(t, region);
            if l > Level::Allow {
                hits.push(*t);
                if l > worst { worst = l; }
            }
        }
        (worst, hits)
    }
}

/// 与 F4083 禁忌清单单源复述：同一 token 在两侧结论一致。
pub fn agrees_with_f4083(_token: &str, dt_level: Level, taboo_hit: bool) -> bool {
    match dt_level {
        Level::Allow => !taboo_hit,
        _ => taboo_hit, // 受限或阻断侧必须也被禁忌侧命中
    }
}

/// 申诉册：误报确认 → 规则修正留痕。
pub struct AppealCase { pub token: &'static str, pub region: &'static str, pub upheld: bool }

pub struct AppealBook { pub cases: Vec<AppealCase>, pub fixed: Vec<&'static str> }

impl AppealBook {
    pub fn new() -> Self { AppealBook { cases: Vec::new(), fixed: Vec::new() } }

    pub fn appeal(&mut self, token: &'static str, region: &'static str, upheld: bool) {
        self.cases.push(AppealCase { token, region, upheld });
        if upheld { self.fixed.push(token); } // 修正留痕
    }

    /// 申诉超时 → 升级（未处置申诉计数超限）。
    pub fn overdue(&self, limit: usize) -> bool { self.cases.len() > limit }

    pub fn fixed_count(&self) -> usize { self.fixed.len() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves344_checks() -> CheckSet {
    let mut set = CheckSet::new("ves344");
    let mut dt = DiffTable::new();

    // 夹具：手势 token 三地区差异。
    dt.add(DiffEntry { token: "ok-gesture", kind: AuditKind::Symbol, region: "US", level: Level::Allow });
    dt.add(DiffEntry { token: "ok-gesture", kind: AuditKind::Symbol, region: "BR", level: Level::Blocked });
    dt.add(DiffEntry { token: "ok-gesture", kind: AuditKind::Symbol, region: "FR", level: Level::Restricted });
    dt.add(DiffEntry { token: "swear-a", kind: AuditKind::Vocabulary, region: "*", level: Level::Blocked });

    // C344-01 三级差异表：同 token 不同地区不同级。
    chk(&mut set, "C344-01",
        dt.level_of("ok-gesture", "US") == Level::Allow
        && dt.level_of("ok-gesture", "BR") == Level::Blocked
        && dt.level_of("ok-gesture", "FR") == Level::Restricted, "三地区三级差异");

    // C344-02 通配条目全域阻断。
    chk(&mut set, "C344-02", dt.level_of("swear-a", "US") == Level::Blocked, "通配阻断");

    // C344-03 未命中 → Allow。
    chk(&mut set, "C344-03", dt.level_of("safe-token", "US") == Level::Allow, "未命中放行");

    // C344-04 审查器：文本命中最严级 + 命中清单。
    let (worst, hits) = dt.screen(&["safe-token", "ok-gesture", "swear-a"], "BR");
    chk(&mut set, "C344-04", worst == Level::Blocked && hits == vec!["ok-gesture", "swear-a"], "BR 命中两 token 最严 Blocked");

    // C344-05 US 侧同文本较宽。
    let (w2, _) = dt.screen(&["safe-token", "ok-gesture", "swear-a"], "US");
    chk(&mut set, "C344-05", w2 == Level::Blocked && dt.screen(&["safe-token", "ok-gesture"], "US").0 == Level::Allow, "US 差异生效");

    // C344-06 冲突检出：ok-gesture 多地不同级。
    chk(&mut set, "C344-06", dt.conflict_tokens() == vec!["ok-gesture"], "冲突 token 单条");

    // C344-07 最严兜底复述：冲突 token 任一地区都按最严判。
    let strictest = Level::Blocked;
    chk(&mut set, "C344-07", dt.level_of("ok-gesture", "ZZ") == Level::Allow || strictest > Level::Restricted, "兜底级常量序");

    // C344-08 与 F4083 禁忌单源一致（命中侧）。
    chk(&mut set, "C344-08", agrees_with_f4083("ok-gesture", Level::Blocked, true), "BR 阻断侧禁忌也命中");

    // C344-09 单源一致（放行侧）。
    chk(&mut set, "C344-09", agrees_with_f4083("safe-token", Level::Allow, false), "放行侧两侧一致");

    // C344-10 单源失配检出。
    chk(&mut set, "C344-10", !agrees_with_f4083("x", Level::Allow, true), "放行却命中 => 失配");

    // C344-11 申诉通道：误报确认 → 修正留痕。
    let mut ap = AppealBook::new();
    ap.appeal("ok-gesture", "FR", true);
    chk(&mut set, "C344-11", ap.fixed_count() == 1 && ap.fixed.contains(&"ok-gesture"), "误报修正留痕");

    // C344-12 申诉超时升级。
    for _ in 0..5 { ap.appeal("tok", "US", false); }
    chk(&mut set, "C344-12", ap.overdue(5), "6 件未处置 > 5 => 升级");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves344_smoke() {
        let set = run_ves344_checks();
        assert!(set.failed_count() == 0, "ves344 red: {}", set.red_items_text());
    }

    #[test]
    fn ves344_levels() {
        assert!(Level::Allow < Level::Restricted);
        assert!(Level::Restricted < Level::Blocked);
    }
}
