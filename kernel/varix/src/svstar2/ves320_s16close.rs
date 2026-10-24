//! ves320_s16close — VE-F4080 · T04 翻译与文案流程组收口
//! 二十项核验汇总 + 五证据族（单点性/判据注册/联测绿/清账闭环/单源复述）+ 双签（自查签/复核签分离禁同人）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x9140;

pub const CLOSE_MAX: u8 = 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum T4CloseCode {
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
    pub fn mark(&mut self) -> Result<(), T4CloseCode> {
        if self.present { return Err(T4CloseCode::AlreadyClosed); }
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

    pub fn confirm(&mut self, i: usize) -> Result<(), T4CloseCode> {
        if self.closed { return Err(T4CloseCode::AlreadyClosed); }
        if i >= self.items.len() { return Err(T4CloseCode::CloseIncomplete); }
        self.items[i].done = true;
        Ok(())
    }

    pub fn checks_complete(&self) -> bool {
        self.items.len() == CLOSE_MAX as usize && self.items.iter().all(|it| it.done)
    }

    pub fn mark_ev(&mut self, k: EvKind) -> Result<(), T4CloseCode> {
        if self.closed { return Err(T4CloseCode::AlreadyClosed); }
        for e in self.ev.iter_mut() {
            if e.kind == k { return e.mark(); }
        }
        Err(T4CloseCode::EvidenceMissing)
    }

    pub fn ev_complete(&self) -> bool {
        self.ev.iter().all(|e| e.present)
    }

    pub fn sign(&mut self, role: u8, who: &str) -> Result<(), T4CloseCode> {
        if self.closed { return Err(T4CloseCode::AlreadyClosed); }
        if !self.checks_complete() { return Err(T4CloseCode::NotAllChecks); }
        if !self.ev_complete() { return Err(T4CloseCode::EvidenceMissing); }
        match role {
            0 => {
                if self.self_sign.is_some() { return Err(T4CloseCode::AlreadyClosed); }
                self.self_sign = Some(String::from(who));
                Ok(())
            }
            1 => {
                if self.review_sign.is_some() { return Err(T4CloseCode::AlreadyClosed); }
                match &self.self_sign {
                    None => Err(T4CloseCode::DualSignBad),
                    Some(s) if s == who => Err(T4CloseCode::SignerClash),
                    Some(_) => {
                        self.review_sign = Some(String::from(who));
                        Ok(())
                    }
                }
            }
            _ => Err(T4CloseCode::DualSignBad),
        }
    }

    pub fn seal(&mut self) -> Result<(), T4CloseCode> {
        if self.closed { return Err(T4CloseCode::AlreadyClosed); }
        if self.self_sign.is_none() || self.review_sign.is_none() { return Err(T4CloseCode::DualSignBad); }
        self.closed = true;
        Ok(())
    }
}

fn close_labels() -> [String; CLOSE_MAX as usize] {
    [
        String::from("四段流水线"),
        String::from("十八项映射"),
        String::from("五段签名"),
        String::from("抽取三要素"),
        String::from("TM 三档"),
        String::from("TB 双表"),
        String::from("机翻标识"),
        String::from("译后四步"),
        String::from("LQA 硬门"),
        String::from("风格五维"),
        String::from("上下文注入"),
        String::from("进度五态"),
        String::from("质量冻结线"),
        String::from("翻译无障碍"),
        String::from("安全三查"),
        String::from("性能基准"),
        String::from("调试追踪"),
        String::from("三层测试"),
        String::from("联调 18 站"),
        String::from("fuzz 三零线"),
    ]
}

pub fn run_ves320_checks() -> CheckSet {
    let mut set = CheckSet::new("ves320_s16close");

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
    set.add("C4080-01", c.checks_complete() && c.items.len() == 20, "二十项齐");

    // 2) 未全确认拒签。
    let mut c2 = Closeout::new(&["a"]);
    set.add("C4080-02", c2.sign(0, "a") == Err(T4CloseCode::NotAllChecks), "未全确认拒");

    // 3) 缺证据拒签。
    let mut c3 = build();
    set.add("C4080-03", c3.sign(0, "a") == Err(T4CloseCode::EvidenceMissing), "缺证据拒");

    // 4) 五证据齐。
    for k in EvKind::ALL.iter() { let _ = c3.mark_ev(*k); }
    set.add("C4080-04", c3.ev_complete(), "五证据齐");

    // 5) 自查签。
    set.add("C4080-05", c3.sign(0, "self").is_ok(), "自查签");

    // 6) 同人拒签。
    set.add("C4080-06", c3.sign(1, "self") == Err(T4CloseCode::SignerClash), "同人拒");

    // 7) 无自查先复核拒。
    let mut c7 = build();
    for k in EvKind::ALL.iter() { let _ = c7.mark_ev(*k); }
    set.add("C4080-07", c7.sign(1, "r") == Err(T4CloseCode::DualSignBad), "无自查拒");

    // 8) 异人双签收口。
    set.add("C4080-08", c3.sign(1, "review").is_ok() && c3.seal().is_ok() && c3.closed, "双签收口");

    // 9) 收口封账。
    set.add("C4080-09", c3.sign(0, "x") == Err(T4CloseCode::AlreadyClosed) && c3.seal() == Err(T4CloseCode::AlreadyClosed), "收口封账");

    // 10) 证据幂等。
    let mut c10 = build();
    let _ = c10.mark_ev(EvKind::Singularity);
    set.add("C4080-10", c10.mark_ev(EvKind::Singularity) == Err(T4CloseCode::AlreadyClosed), "证据幂等");

    // 11) 非法角色拒。
    let mut c11 = build();
    for k in EvKind::ALL.iter() { let _ = c11.mark_ev(*k); }
    set.add("C4080-11", c11.sign(7, "x") == Err(T4CloseCode::DualSignBad), "非法角色拒");

    // 12) 越界确认拒。
    let mut c12 = Closeout::new(&["a"]);
    set.add("C4080-12", c12.confirm(9) == Err(T4CloseCode::CloseIncomplete), "越界确认拒");

    // 13) 收口项无重复。
    let labels = close_labels();
    let mut dup = false;
    for i in 0..labels.len() {
        for j in (i + 1)..labels.len() {
            if labels[i] == labels[j] { dup = true; }
        }
    }
    set.add("C4080-13", !dup, "收口项无重复");

    // 14) 收口项非空。
    set.add("C4080-14", labels.iter().all(|l| !l.is_empty()), "收口项非空");

    // 15) 诊断码唯一。
    let codes = [T4CloseCode::Ok, T4CloseCode::EvidenceMissing, T4CloseCode::DualSignBad,
                 T4CloseCode::CloseIncomplete, T4CloseCode::SignerClash, T4CloseCode::AlreadyClosed,
                 T4CloseCode::NotAllChecks];
    let mut cdup = false;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] { cdup = true; }
        }
    }
    set.add("C4080-15", !cdup, "诊断码唯一");

    // 16) 段位自查。
    set.add("C4080-16", SEG_BASE == 0x9140, "段位 0x9140");

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
        assert_eq!(c.sign(1, "a"), Err(T4CloseCode::SignerClash));
        assert!(c.sign(1, "b").is_ok());
        assert!(c.seal().is_ok());
    }
}
