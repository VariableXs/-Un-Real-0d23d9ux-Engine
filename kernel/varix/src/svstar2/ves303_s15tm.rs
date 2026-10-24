//! ves303_s15tm — VE-F4063 · T04 翻译记忆与术语库
//! TM 句对入库+模糊匹配三档（100%/95%/75%）+ TB 术语强制/禁用双表 + 指纹归一检索 + 版本戳不覆盖 + 句对脱敏红线。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x88C0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum TmCode {
    Ok = 0,
    NoMatch = SEG_BASE + 0x01,
    BelowThreshold = SEG_BASE + 0x02,
    TermConflict = SEG_BASE + 0x03,
    ForbiddenTerm = SEG_BASE + 0x04,
    LibraryDamaged = SEG_BASE + 0x05,
    PrivacyBreach = SEG_BASE + 0x06,
}

/// 匹配分档。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchTier { Exact = 100, High = 95, Fuzzy = 75, None = 0 }

impl MatchTier {
    pub fn from_rate(rate: u32) -> MatchTier {
        if rate >= 100 { MatchTier::Exact }
        else if rate >= 95 { MatchTier::High }
        else if rate >= 75 { MatchTier::Fuzzy }
        else { MatchTier::None }
    }
}

/// 指纹归一：小写化+去空白后 FNV。
pub fn normalize(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if !c.is_whitespace() {
            for lc in c.to_lowercase() { out.push(lc); }
        }
    }
    out
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn fingerprint(s: &str) -> u64 {
    fnv1a(normalize(s).as_bytes())
}

/// 简单模糊匹配率：公共字符比例（百分数）。
pub fn match_rate(a: &str, b: &str) -> u32 {
    let na = normalize(a);
    let nb = normalize(b);
    if na.is_empty() || nb.is_empty() { return 0; }
    let ca: Vec<char> = na.chars().collect();
    let cb: Vec<char> = nb.chars().collect();
    let mut common = 0usize;
    let mut used = vec![false; cb.len()];
    for x in ca.iter() {
        for (j, y) in cb.iter().enumerate() {
            if !used[j] && x == y {
                used[j] = true;
                common += 1;
                break;
            }
        }
    }
    let denom = ca.len().max(cb.len());
    (common * 100 / denom) as u32
}

/// TM 句对（带版本戳不覆盖）。
#[derive(Clone)]
pub struct TmEntry {
    pub src_fp: u64,
    pub src: String,
    pub tgt: String,
    pub version: u32,
}

pub struct TmLibrary {
    pub entries: Vec<TmEntry>,
}

impl TmLibrary {
    pub fn new() -> Self { TmLibrary { entries: Vec::new() } }

    /// 入库：同指纹新版本不覆盖，追加版本戳。
    pub fn put(&mut self, src: &str, tgt: &str, version: u32) -> Result<(), TmCode> {
        if src.contains("用户@") || src.contains("password") {
            return Err(TmCode::PrivacyBreach);
        }
        let fp = fingerprint(src);
        for e in self.entries.iter() {
            if e.src_fp == fp && e.version >= version && e.tgt == tgt {
                return Err(TmCode::LibraryDamaged);
            }
        }
        self.entries.push(TmEntry { src_fp: fp, src: String::from(src), tgt: String::from(tgt), version });
        Ok(())
    }

    /// 检索：取最高版本匹配。
    pub fn lookup(&self, src: &str) -> Option<(MatchTier, &TmEntry)> {
        let fp = fingerprint(src);
        let mut best: Option<&TmEntry> = None;
        for e in self.entries.iter() {
            if e.src_fp == fp {
                match best {
                    None => best = Some(e),
                    Some(b) if e.version > b.version => best = Some(e),
                    _ => {}
                }
            }
        }
        best.map(|e| (MatchTier::Exact, e))
    }

    /// 模糊检索：全库匹配率最高且达 75 档。
    pub fn fuzzy_lookup(&self, src: &str) -> Option<(MatchTier, &TmEntry)> {
        let mut best: Option<(MatchTier, &TmEntry)> = None;
        for e in self.entries.iter() {
            let r = match_rate(src, &e.src);
            let tier = MatchTier::from_rate(r);
            if tier == MatchTier::None { continue; }
            match best {
                None => best = Some((tier, e)),
                Some((bt, _)) if tier > bt => best = Some((tier, e)),
                _ => {}
            }
        }
        best
    }

    /// 快照回滚：快照点前条目保留。
    pub fn rollback(&mut self, at: usize) -> Result<(), TmCode> {
        if at > self.entries.len() { return Err(TmCode::LibraryDamaged); }
        self.entries.truncate(at);
        Ok(())
    }
}

/// TB 双表。
pub struct TermBook {
    pub forced: Vec<(String, String)>,
    pub forbidden: Vec<String>,
}

impl TermBook {
    pub fn new() -> Self { TermBook { forced: Vec::new(), forbidden: Vec::new() } }
    pub fn force(&mut self, src: &str, tgt: &str) -> Result<(), TmCode> {
        for f in self.forced.iter() {
            if f.0 == src && f.1 != tgt { return Err(TmCode::TermConflict); }
        }
        self.forced.push((String::from(src), String::from(tgt)));
        Ok(())
    }
    pub fn forbid(&mut self, term: &str) {
        self.forbidden.push(String::from(term));
    }
    /// 译文中含禁用术语 → 拒。
    pub fn check_forbidden(&self, tgt: &str) -> Result<(), TmCode> {
        for f in self.forbidden.iter() {
            if tgt.contains(f.as_str()) { return Err(TmCode::ForbiddenTerm); }
        }
        Ok(())
    }
    /// 强制术语检查。
    pub fn check_forced(&self, src: &str, tgt: &str) -> Result<(), TmCode> {
        for f in self.forced.iter() {
            if f.0 == src && !tgt.contains(f.1.as_str()) {
                return Err(TmCode::TermConflict);
            }
        }
        Ok(())
    }
}

pub fn run_ves303_checks() -> CheckSet {
    let mut set = CheckSet::new("ves303_s15tm");
    let mut tm = TmLibrary::new();

    // 1) 指纹归一：大小写与空白不影响指纹。
    set.add("C4063-01", fingerprint("Hello World") == fingerprint("hello  world"), "指纹归一");

    // 2) 精确检索。
    let _ = tm.put("Save file", "保存文件", 1);
    let hit = tm.lookup("save FILE");
    set.add("C4063-02", hit.is_some() && hit.unwrap().0 == MatchTier::Exact, "精确检索");

    // 3) 版本戳不覆盖：同源新版本追加。
    let _ = tm.put("Save file", "储存档案", 2);
    set.add("C4063-03", tm.entries.len() == 2 && tm.lookup("Save file").unwrap().1.version == 2, "版本戳");

    // 4) 模糊分档。
    set.add("C4063-04", MatchTier::from_rate(100) == MatchTier::Exact
        && MatchTier::from_rate(95) == MatchTier::High
        && MatchTier::from_rate(75) == MatchTier::Fuzzy
        && MatchTier::from_rate(40) == MatchTier::None, "三档分档");

    // 5) 模糊检索达档。
    let _ = tm.put("Open the settings panel", "打开设置面板", 1);
    let fz = tm.fuzzy_lookup("Open the settings panel!");
    set.add("C4063-05", fz.is_some() && fz.unwrap().0 >= MatchTier::Fuzzy, "模糊检索");

    // 6) 低档转人工：无库命中或低于 75 档。
    set.add("C4063-06", tm.fuzzy_lookup("zzz qqq vvv").map_or(true, |(t, _)| t == MatchTier::None), "低档转人工");

    // 7) 快照回滚。
    let _ = tm.rollback(2);
    set.add("C4063-07", tm.entries.len() == 2, "快照回滚");

    // 8) 回滚越界拒。
    set.add("C4063-08", tm.rollback(99) == Err(TmCode::LibraryDamaged), "回滚越界拒");

    // 9) 脱敏红线。
    set.add("C4063-09", tm.put("用户@张三的邮件", "x", 1) == Err(TmCode::PrivacyBreach), "脱敏红线");

    // 10) TB 强制术语。
    let mut tb = TermBook::new();
    let _ = tb.force("file", "文件");
    set.add("C4063-10", tb.check_forced("file", "保存文件").is_ok()
        && tb.check_forced("file", "保存档案") == Err(TmCode::TermConflict), "强制术语");

    // 11) 术语冲突仲裁：重注册不同译法拒。
    set.add("C4063-11", tb.force("file", "档案") == Err(TmCode::TermConflict), "术语冲突拒");

    // 12) 禁用术语。
    tb.forbid("重启电脑");
    set.add("C4063-12", tb.check_forbidden("请重启电脑") == Err(TmCode::ForbiddenTerm)
        && tb.check_forbidden("请重新启动").is_ok(), "禁用术语");

    // 13) 段位自查。
    set.add("C4063-13", SEG_BASE == 0x88C0, "段位 0x88C0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tm_flow() {
        assert_eq!(fingerprint("ABC"), fingerprint("abc"));
        let mut tm = TmLibrary::new();
        let _ = tm.put("hello", "你好", 1);
        assert!(tm.lookup("HELLO").is_some());
    }
}
