//! ves312_s16sec — VE-F4072 · T04 翻译安全
//! 注入面三查（译文回注占位符/标签完整性断言 + 机翻出站白名单+边界 + 术语表投毒双人审）+ 占位符断裂阻断回退 + 用户内容永不外发红线。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8D40;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum TsCode {
    Ok = 0,
    PlaceholderBroken = SEG_BASE + 0x01,
    TagBroken = SEG_BASE + 0x02,
    OutboundBlocked = SEG_BASE + 0x03,
    TbFrozen = SEG_BASE + 0x04,
    PrivacyBreach = SEG_BASE + 0x05,
}

/// 占位符集合（{0} {1} {name} 形式）。
pub fn placeholders_of(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '{' {
            let mut j = i + 1;
            let mut name = String::new();
            while j < bytes.len() && bytes[j] != '}' {
                name.push(bytes[j]);
                j += 1;
            }
            if j < bytes.len() && !name.is_empty() {
                out.push(name);
                i = j;
            }
        }
        i += 1;
    }
    out
}

/// 占位符完整性断言：译文占位符集合与原文一致。
pub fn placeholder_assert(src: &str, tgt: &str) -> Result<(), TsCode> {
    let a = placeholders_of(src);
    let b = placeholders_of(tgt);
    if a.len() != b.len() { return Err(TsCode::PlaceholderBroken); }
    for x in a.iter() {
        if !b.contains(x) { return Err(TsCode::PlaceholderBroken); }
    }
    Ok(())
}

/// 标签完整性：<b>…</b> 成对。
pub fn tag_assert(s: &str) -> Result<(), TsCode> {
    let open = s.matches("<b>").count();
    let close = s.matches("</b>").count();
    if open != close { return Err(TsCode::TagBroken); }
    Ok(())
}

/// 出站白名单 + 边界。
pub struct OutboundGate {
    pub whitelist: Vec<&'static str>,
    pub tls_pinned: bool,
}

impl OutboundGate {
    pub fn new() -> Self {
        OutboundGate { whitelist: vec!["mt.example.api"], tls_pinned: true }
    }
    /// 出站检查：域名白名单 + TLS 固定 + 用户内容永不外发。
    pub fn check(&self, host: &str, payload_has_user_content: bool) -> Result<(), TsCode> {
        if !self.whitelist.contains(&host) { return Err(TsCode::OutboundBlocked); }
        if !self.tls_pinned { return Err(TsCode::OutboundBlocked); }
        if payload_has_user_content { return Err(TsCode::PrivacyBreach); }
        Ok(())
    }
}

/// TB 投毒防护：变更双人审。
pub struct TbGuard {
    pub pending_change: Option<(String, String, String)>, // (术语, 旧译, 新译)
    pub approvals: Vec<String>,
}

impl TbGuard {
    pub fn new() -> Self { TbGuard { pending_change: None, approvals: Vec::new() } }
    pub fn propose(&mut self, term: &str, old: &str, new: &str) {
        self.pending_change = Some((String::from(term), String::from(old), String::from(new)));
        self.approvals.clear();
    }
    pub fn approve(&mut self, who: &str) -> bool {
        if self.pending_change.is_none() { return false; }
        if self.approvals.contains(&String::from(who)) { return false; }
        self.approvals.push(String::from(who));
        true
    }
    /// 双人审通过才可变更。
    pub fn ready(&self) -> bool {
        self.approvals.len() >= 2
    }
    pub fn frozen(&self) -> bool {
        self.pending_change.is_some() && !self.ready()
    }
}

pub fn run_ves312_checks() -> CheckSet {
    let mut set = CheckSet::new("ves312_s16sec");

    // 1) 占位符提取。
    set.add("C4072-01", placeholders_of("Hello {name}, you have {0} items") == ["name", "0"], "占位符提取");

    // 2) 完整性断言过。
    set.add("C4072-02", placeholder_assert("Hi {name}", "你好 {name}").is_ok(), "占位符完整");

    // 3) 占位符断裂阻断。
    set.add("C4072-03", placeholder_assert("Hi {name}", "你好") == Err(TsCode::PlaceholderBroken), "断裂阻断");

    // 4) 标签成对。
    set.add("C4072-04", tag_assert("<b>加粗</b>").is_ok(), "标签成对");

    // 5) 标签断裂。
    set.add("C4072-05", tag_assert("<b>加粗") == Err(TsCode::TagBroken), "标签断裂");

    // 6) 出站白名单。
    let gate = OutboundGate::new();
    set.add("C4072-06", gate.check("mt.example.api", false).is_ok(), "白名单放行");

    // 7) 越界出站拒。
    set.add("C4072-07", gate.check("evil.api", false) == Err(TsCode::OutboundBlocked), "越界拒");

    // 8) 用户内容永不外发红线。
    set.add("C4072-08", gate.check("mt.example.api", true) == Err(TsCode::PrivacyBreach), "隐私红线");

    // 9) TB 双人审。
    let mut g = TbGuard::new();
    g.propose("file", "文件", "档案");
    set.add("C4072-09", !g.ready() && g.frozen(), "单审冻结");

    // 10) 同人重复审不算。
    let _ = g.approve("alice");
    set.add("C4072-10", !g.approve("alice") && !g.ready(), "同人重复拒");

    // 11) 双人审放行。
    let _ = g.approve("bob");
    set.add("C4072-11", g.ready() && !g.frozen(), "双人审放行");

    // 12) 段位自查。
    set.add("C4072-12", SEG_BASE == 0x8D40, "段位 0x8D40");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec_checks() {
        assert!(placeholder_assert("{0} 项", "{0} 项").is_ok());
        assert!(placeholder_assert("{0} 项", "{1} 项").is_err());
        let mut g = TbGuard::new();
        g.propose("a", "b", "c");
        let _ = g.approve("x");
        let _ = g.approve("y");
        assert!(g.ready());
    }
}
