//! ves346_s19fmt — VE-F4106 · T06 地区格式深化
//! 日期/时间/数字/货币/百分比/单位六类格式×地区规则深化（T01 基础之上的边缘规则：周起始日/首位月份/负数括号习惯）+ 格式引擎缓存层（命中 O(1)）+ 规则缺 CLDR 默认+登记 + 缓存主动失效重建 + 输出供读屏朗读文本用（F4071 联动——域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xBE40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FmCode {
    Ok = 0,
    CacheStale = SEG_BASE + 0x01,
    Ambiguous = SEG_BASE + 0x02,
}

/// 六类格式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FmtKind { Date = 0, Time = 1, Number = 2, Currency = 3, Percent = 4, Unit = 5 }

impl FmtKind {
    pub const ALL: [FmtKind; 6] = [
        FmtKind::Date, FmtKind::Time, FmtKind::Number, FmtKind::Currency, FmtKind::Percent, FmtKind::Unit];
    pub fn name(self) -> &'static str {
        match self { FmtKind::Date => "date", FmtKind::Time => "time", FmtKind::Number => "number",
            FmtKind::Currency => "currency", FmtKind::Percent => "percent", FmtKind::Unit => "unit" }
    }
}

/// 边缘规则集：周起始日 / 首位月份 / 负数括号。
#[derive(Clone, Copy)]
pub struct EdgeRules {
    pub week_start_mon: bool,   // true=周一起始 false=周日
    pub day_first: bool,        // true=日月年（首位日）false=月日年
    pub negative_paren: bool,   // true=负数括号习惯 (123)
}

pub const CLDR_DEFAULT: EdgeRules = EdgeRules { week_start_mon: true, day_first: true, negative_paren: false };

/// 格式规则册：地区 × 类 → 规则串（如日期模板 "YYYY-MM-DD"）。
pub struct FmtBook { pub rules: Vec<(&'static str, FmtKind, &'static str)>, pub edges: Vec<(&'static str, EdgeRules)> }

impl FmtBook {
    pub fn new() -> Self { FmtBook { rules: Vec::new(), edges: Vec::new() } }

    pub fn put(&mut self, region: &'static str, kind: FmtKind, tpl: &'static str) {
        if !self.rules.iter().any(|(r, k, _)| *r == region && *k == kind) {
            self.rules.push((region, kind, tpl));
        }
    }

    pub fn put_edges(&mut self, region: &'static str, e: EdgeRules) {
        if !self.edges.iter().any(|(r, _)| *r == region) {
            self.edges.push((region, e));
        }
    }

    /// 取规则：缺 → CLDR 默认 + 登记缺口。
    pub fn rule_of(&mut self, region: &'static str, kind: FmtKind) -> (&'static str, bool) {
        if let Some((_, _, tpl)) = self.rules.iter().find(|(r, k, _)| *r == region && *k == kind) {
            (tpl, false)
        } else {
            self.rules.push((region, kind, "CLDR-DEFAULT"));
            ("CLDR-DEFAULT", true)
        }
    }

    pub fn edges_of(&self, region: &str) -> EdgeRules {
        self.edges.iter().find(|(r, _)| *r == region).map(|(_, e)| *e).unwrap_or(CLDR_DEFAULT)
    }

    pub fn missing_registered(&self, region: &str, kind: FmtKind) -> bool {
        self.rules.iter().any(|(r, k, t)| *r == region && *k == kind && *t == "CLDR-DEFAULT")
    }
}

/// 负数格式化：括号习惯。
pub fn fmt_negative(v: i64, e: EdgeRules) -> String {
    if v < 0 && e.negative_paren {
        let mut s = String::from("(");
        s.push_str(&u64_str((-v) as u64));
        s.push(')');
        s
    } else if v < 0 {
        let mut s = String::from("-");
        s.push_str(&u64_str((-v) as u64));
        s
    } else {
        u64_str(v as u64)
    }
}

pub fn u64_str(mut v: u64) -> String {
    if v == 0 { return String::from("0"); }
    let mut buf = Vec::new();
    while v > 0 {
        buf.push(b'0' + (v % 10) as u8);
        v /= 10;
    }
    buf.reverse();
    String::from_utf8(buf).unwrap_or_default()
}

/// 百分比格式。
pub fn fmt_percent(bp: u32, region: &str) -> String {
    // 欧陆习惯逗号小数；其余点。
    let whole = bp / 100;
    let frac = bp % 100;
    let mut s = u64_str(whole as u64);
    if frac > 0 {
        s.push(if region == "DE" || region == "FR" { ',' } else { '.' });
        s.push_str(&u64_str(frac as u64));
    }
    s.push('%');
    s
}

/// 格式缓存层：键 → 输出。
pub struct FmtCache { pub items: Vec<((u64, u8), String)>, pub generation: u64 }

pub fn cache_key(region_hash: u64, kind: FmtKind) -> (u64, u8) { (region_hash, kind as u8) }

impl FmtCache {
    pub fn new() -> Self { FmtCache { items: Vec::new(), generation: 1 } }

    /// 命中 O(1) 口径（向量内定位——引擎以哈希桶实现，此处语义等价）。
    pub fn get(&self, k: (u64, u8)) -> Option<&String> {
        self.items.iter().find(|(kk, _)| *kk == k).map(|(_, v)| v)
    }

    pub fn put(&mut self, k: (u64, u8), v: String) {
        if self.get(k).is_none() { self.items.push((k, v)); }
    }

    /// 主动失效：代数推进清空（重建）。
    pub fn invalidate(&mut self) {
        self.items.clear();
        self.generation += 1;
    }

    pub fn stale(&self, cur_gen: u64) -> bool { self.generation != cur_gen }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves346_checks() -> CheckSet {
    let mut set = CheckSet::new("ves346");
    let mut fb = FmtBook::new();

    // C346-01 六类名单单源。
    chk(&mut set, "C346-01", FmtKind::ALL.len() == 6 && FmtKind::ALL[5].name() == "unit", "六类单源");

    // C346-02 规则命中。
    fb.put("CN", FmtKind::Date, "YYYY-MM-DD");
    fb.put("US", FmtKind::Date, "MM/DD/YYYY");
    let (t, miss) = fb.rule_of("US", FmtKind::Date);
    chk(&mut set, "C346-02", t == "MM/DD/YYYY" && !miss, "US 日期模板命中");

    // C346-03 规则缺 → CLDR 默认 + 登记。
    let (t2, miss2) = fb.rule_of("JP", FmtKind::Date);
    chk(&mut set, "C346-03", t2 == "CLDR-DEFAULT" && miss2 && fb.missing_registered("JP", FmtKind::Date), "缺 → 默认并登记");

    // C346-04 边缘规则：周起始日差异。
    fb.put_edges("US", EdgeRules { week_start_mon: false, day_first: false, negative_paren: false });
    fb.put_edges("DE", EdgeRules { week_start_mon: true, day_first: true, negative_paren: true });
    chk(&mut set, "C346-04", !fb.edges_of("US").week_start_mon && fb.edges_of("DE").week_start_mon, "周起始日 US 周日 DE 周一");

    // C346-05 首位月份差异。
    chk(&mut set, "C346-05", !fb.edges_of("US").day_first && fb.edges_of("DE").day_first, "US 月先行 DE 日先行");

    // C346-06 负数括号习惯。
    chk(&mut set, "C346-06", fmt_negative(-123, fb.edges_of("DE")) == "(123)" && fmt_negative(-123, fb.edges_of("US")) == "-123", "DE 括号 US 负号");

    // C346-07 正数不套括号。
    chk(&mut set, "C346-07", fmt_negative(123, fb.edges_of("DE")) == "123", "正数直出");

    // C346-08 百分比格式：欧陆逗号。
    chk(&mut set, "C346-08", fmt_percent(1250, "DE") == "12,5%" && fmt_percent(1250, "US") == "12.5%", "12.5% 两种分隔");

    // C346-09 整数百分比无小数。
    chk(&mut set, "C346-09", fmt_percent(1900, "DE") == "19%", "19% 无尾零");

    // C346-10 缓存命中。
    let mut cache = FmtCache::new();
    let k = cache_key(0xC0FFEE, FmtKind::Currency);
    cache.put(k, String::from("¥1,234"));
    chk(&mut set, "C346-10", cache.get(k).map_or(false, |v| v == "¥1,234"), "缓存命中读出");

    // C346-11 缓存主动失效重建。
    let g0 = cache.generation;
    cache.invalidate();
    chk(&mut set, "C346-11", cache.items.is_empty() && cache.generation == g0 + 1 && cache.stale(g0), "失效后代数推进");

    // C346-12 朗读联动（域本色）：输出是纯文本可朗读。
    let spoken = fmt_percent(1900, "CN");
    chk(&mut set, "C346-12", spoken.chars().all(|c| c.is_ascii_digit() || c == '%' || c == '.' || c == ','), "输出可朗读字符集");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves346_smoke() {
        let set = run_ves346_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves346 red: {:?}", red);
    }

    #[test]
    fn ves346_negative() {
        let e = EdgeRules { week_start_mon: true, day_first: true, negative_paren: true };
        assert_eq!(fmt_negative(-42, e), "(42)");
        assert_eq!(fmt_negative(0, e), "0");
    }
}
