//! ves300_s14close — VE-F4060 · T03 多语言排版引擎组收口
//! 二十项核验汇总 + 五证据族（单点性/判据注册/联测绿/清账闭环/单源复述）+ 双签（自查签/复核签分离禁同人）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x86C0;

pub const CLOSE_MAX: u8 = 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum T3CloseCode {
    Ok = 0,
    EvidenceMissing = SEG_BASE + 0x01,
    DualSignBad = SEG_BASE + 0x02,
    CloseIncomplete = SEG_BASE + 0x03,
    SignerClash = SEG_BASE + 0x04,
    AlreadyClosed = SEG_BASE + 0x05,
    NotAllChecks = SEG_BASE + 0x06,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EvKind { Singularity = 0, ChecksRegistered = 1, IntegGreen = 2, LegacyClosed = 3, RetellSingle = 4 }

impl EvKind {
    pub const ALL: [EvKind; 5] = [
        EvKind::Singularity, EvKind::ChecksRegistered, EvKind::IntegGreen, EvKind::LegacyClosed, EvKind::RetellSingle];
}

#[derive(Clone)]
pub struct EvItem {
    pub kind: EvKind,
    pub present: bool,
}

impl EvItem {
    pub fn mark(&mut self) -> Result<(), T3CloseCode> {
        if self.present { return Err(T3CloseCode::AlreadyClosed); }
        self.present = true;
        Ok(())
    }
}

#[derive(Clone)]
pub struct CloseItem {
    pub label: String,
    pub done: bool,
}

pub struct Closeout {
    pub items: Vec<CloseItem>,
    pub ev: Vec<EvItem>,
    pub self_sign: Option<String>,
    pub review_sign: Option<String>,
    pub closed: bool,
}

impl Closeout {
    pub fn new(labels: &[&str]) -> Self {
        let mut items = Vec::new();
        for l in labels.iter() {
            items.push(CloseItem { label: String::from(*l), done: false });
        }
        let mut ev = Vec::new();
        for k in EvKind::ALL.iter() {
            ev.push(EvItem { kind: *k, present: false });
        }
        Closeout { items, ev, self_sign: None, review_sign: None, closed: false }
    }

    pub fn confirm(&mut self, i: usize) -> Result<(), T3CloseCode> {
        if self.closed { return Err(T3CloseCode::AlreadyClosed); }
        if i >= self.items.len() { return Err(T3CloseCode::CloseIncomplete); }
        self.items[i].done = true;
        Ok(())
    }

    pub fn checks_complete(&self) -> bool {
        self.items.len() == CLOSE_MAX as usize && self.items.iter().all(|it| it.done)
    }

    pub fn mark_ev(&mut self, k: EvKind) -> Result<(), T3CloseCode> {
        if self.closed { return Err(T3CloseCode::AlreadyClosed); }
        for e in self.ev.iter_mut() {
            if e.kind == k { return e.mark(); }
        }
        Err(T3CloseCode::EvidenceMissing)
    }

    pub fn ev_complete(&self) -> bool {
        self.ev.iter().all(|e| e.present)
    }

    pub fn sign(&mut self, role: u8, who: &str) -> Result<(), T3CloseCode> {
        if self.closed { return Err(T3CloseCode::AlreadyClosed); }
        if !self.checks_complete() { return Err(T3CloseCode::NotAllChecks); }
        if !self.ev_complete() { return Err(T3CloseCode::EvidenceMissing); }
        match role {
            0 => {
                if self.self_sign.is_some() { return Err(T3CloseCode::AlreadyClosed); }
                self.self_sign = Some(String::from(who));
                Ok(())
            }
            1 => {
                if self.review_sign.is_some() { return Err(T3CloseCode::AlreadyClosed); }
                match &self.self_sign {
                    None => Err(T3CloseCode::DualSignBad),
                    Some(s) if s == who => Err(T3CloseCode::SignerClash),
                    Some(_) => {
                        self.review_sign = Some(String::from(who));
                        Ok(())
                    }
                }
            }
            _ => Err(T3CloseCode::DualSignBad),
        }
    }

    pub fn seal(&mut self) -> Result<(), T3CloseCode> {
        if self.closed { return Err(T3CloseCode::AlreadyClosed); }
        if self.self_sign.is_none() || self.review_sign.is_none() { return Err(T3CloseCode::DualSignBad); }
        self.closed = true;
        Ok(())
    }
}

fn close_labels() -> [String; CLOSE_MAX as usize] {
    [
        String::from("四族单源"),
        String::from("五段签名"),
        String::from("CJK 禁则"),
        String::from("泰印度锚定"),
        String::from("阿拉伯整形"),
        String::from("拉丁微排"),
        String::from("混排基线"),
        String::from("断行连字"),
        String::from("N02 协同"),
        String::from("质量对拍"),
        String::from("排版性能"),
        String::from("调试器"),
        String::from("测试语料"),
        String::from("示例库"),
        String::from("三册文档"),
        String::from("fuzz 不变量"),
        String::from("Q 字体协同"),
        String::from("S 无障碍协同"),
        String::from("联调 36 格"),
        String::from("预留清账"),
    ]
}

pub fn run_ves300_checks() -> CheckSet {
    let mut set = CheckSet::new("ves300_s14close");

    let build = || -> Closeout {
        let labels = close_labels();
        let mut items = Vec::new();
        for l in labels.iter() {
            items.push(CloseItem { label: l.clone(), done: true });
        }
        let mut ev = Vec::new();
        for k in EvKind::ALL.iter() { ev.push(EvItem { kind: *k, present: false }); }
        Closeout { items, ev, self_sign: None, review_sign: None, closed: false }
    };

    // 1) 20 项齐。
    let mut c = build();
    set.add("C4060-01", c.checks_complete() && c.items.len() == 20, "二十项齐");

    // 2) 未全确认拒签。
    let mut c2 = Closeout::new(&["a"]);
    set.add("C4060-02", c2.sign(0, "a") == Err(T3CloseCode::NotAllChecks), "未全确认拒");

    // 3) 缺证据拒签。
    let mut c3 = build();
    set.add("C4060-03", c3.sign(0, "a") == Err(T3CloseCode::EvidenceMissing), "缺证据拒");

    // 4) 五证据齐。
    for k in EvKind::ALL.iter() { let _ = c3.mark_ev(*k); }
    set.add("C4060-04", c3.ev_complete(), "五证据齐");

    // 5) 自查签。
    set.add("C4060-05", c3.sign(0, "self").is_ok(), "自查签");

    // 6) 同人拒签。
    set.add("C4060-06", c3.sign(1, "self") == Err(T3CloseCode::SignerClash), "同人拒");

    // 7) 无自查先复核拒。
    let mut c7 = build();
    for k in EvKind::ALL.iter() { let _ = c7.mark_ev(*k); }
    set.add("C4060-07", c7.sign(1, "r") == Err(T3CloseCode::DualSignBad), "无自查拒");

    // 8) 异人双签收口。
    set.add("C4060-08", c3.sign(1, "review").is_ok() && c3.seal().is_ok() && c3.closed, "双签收口");

    // 9) 收口封账。
    set.add("C4060-09", c3.sign(0, "x") == Err(T3CloseCode::AlreadyClosed) && c3.seal() == Err(T3CloseCode::AlreadyClosed), "收口封账");

    // 10) 证据幂等。
    let mut c10 = build();
    let _ = c10.mark_ev(EvKind::Singularity);
    set.add("C4060-10", c10.mark_ev(EvKind::Singularity) == Err(T3CloseCode::AlreadyClosed), "证据幂等");

    // 11) 非法角色拒。
    let mut c11 = build();
    for k in EvKind::ALL.iter() { let _ = c11.mark_ev(*k); }
    set.add("C4060-11", c11.sign(7, "x") == Err(T3CloseCode::DualSignBad), "非法角色拒");

    // 12) 越界确认拒。
    let mut c12 = Closeout::new(&["a"]);
    set.add("C4060-12", c12.confirm(9) == Err(T3CloseCode::CloseIncomplete), "越界确认拒");

    // 13) 收口项无重复。
    let labels = close_labels();
    let mut dup = false;
    for i in 0..labels.len() {
        for j in (i + 1)..labels.len() {
            if labels[i] == labels[j] { dup = true; }
        }
    }
    set.add("C4060-13", !dup, "收口项无重复");

    // 14) 收口项非空。
    set.add("C4060-14", labels.iter().all(|l| !l.is_empty()), "收口项非空");

    // 15) 覆盖 T03 全域。
    set.add("C4060-15", labels.iter().any(|l| l.contains("禁则")) && labels.iter().any(|l| l.contains("整形")), "全域覆盖");

    // 16) 诊断码唯一。
    let codes = [T3CloseCode::Ok, T3CloseCode::EvidenceMissing, T3CloseCode::DualSignBad,
                 T3CloseCode::CloseIncomplete, T3CloseCode::SignerClash, T3CloseCode::AlreadyClosed,
                 T3CloseCode::NotAllChecks];
    let mut cdup = false;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] { cdup = true; }
        }
    }
    set.add("C4060-16", !cdup, "诊断码唯一");

    // 17) 段位自查。
    set.add("C4060-17", SEG_BASE == 0x86C0, "段位 0x86C0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dual_sign() {
        let mut c = build();
        for k in EvKind::ALL.iter() { let _ = c.mark_ev(*k); }
        assert!(c.sign(0, "a").is_ok());
        assert_eq!(c.sign(1, "a"), Err(T3CloseCode::SignerClash));
        assert!(c.sign(1, "b").is_ok());
        assert!(c.seal().is_ok());
    }
}
