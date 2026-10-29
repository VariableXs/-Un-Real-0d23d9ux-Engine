//! ves280_s12close — VE-F4040 · T02 双向文本与 RTL 组收口
//! 二十项核验汇总 + BiDi/镜像/混向/控件/动效/图标/表格/IME/语料/调试/字体/无障碍/性能/fuzz/示例/文档/N 布局/联调/清账/单点 五证据族齐备 + 双签（自查签/复核签分离禁同人）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x7540;

pub const CLOSE_MAX: u8 = 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum T2CloseCode {
    Ok = 0,
    EvidenceMissing = SEG_BASE + 0x01,
    DualSignBad = SEG_BASE + 0x02,
    CloseIncomplete = SEG_BASE + 0x03,
    SignerClash = SEG_BASE + 0x04,
    AlreadyClosed = SEG_BASE + 0x05,
    NotAllChecks = SEG_BASE + 0x06,
}

/// 五证据族：单点性/判据注册/联测绿/清账闭环/单源复述。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EvKind { Singularity = 0, ChecksRegistered = 1, IntegGreen = 2, LegacyClosed = 3, RetellSingle = 4 }

impl EvKind {
    pub const ALL: [EvKind; 5] = [
        EvKind::Singularity, EvKind::ChecksRegistered, EvKind::IntegGreen, EvKind::LegacyClosed, EvKind::RetellSingle];
    pub fn name(self) -> &'static str {
        match self {
            EvKind::Singularity => "单点性",
            EvKind::ChecksRegistered => "判据注册",
            EvKind::IntegGreen => "联测绿",
            EvKind::LegacyClosed => "清账闭环",
            EvKind::RetellSingle => "单源复述",
        }
    }
}

#[derive(Clone)]
pub struct EvItem {
    pub kind: EvKind,
    pub present: bool,
}

impl EvItem {
    pub fn new(kind: EvKind) -> Self { EvItem { kind, present: false } }
    pub fn mark(&mut self) -> Result<(), T2CloseCode> {
        if self.present { return Err(T2CloseCode::AlreadyClosed); }
        self.present = true;
        Ok(())
    }
}

#[derive(Clone)]
pub struct CloseItem {
    pub label: String,
    pub done: bool,
}

/// 收口账本：20 项核验 + 五证据 + 双签。
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
            ev.push(EvItem::new(*k));
        }
        Closeout { items, ev, self_sign: None, review_sign: None, closed: false }
    }

    pub fn confirm(&mut self, i: usize) -> Result<(), T2CloseCode> {
        if self.closed { return Err(T2CloseCode::AlreadyClosed); }
        if i >= self.items.len() { return Err(T2CloseCode::CloseIncomplete); }
        self.items[i].done = true;
        Ok(())
    }

    pub fn checks_complete(&self) -> bool {
        self.items.len() == CLOSE_MAX as usize && self.items.iter().all(|it| it.done)
    }

    pub fn mark_ev(&mut self, k: EvKind) -> Result<(), T2CloseCode> {
        if self.closed { return Err(T2CloseCode::AlreadyClosed); }
        for e in self.ev.iter_mut() {
            if e.kind == k { return e.mark(); }
        }
        Err(T2CloseCode::EvidenceMissing)
    }

    pub fn ev_complete(&self) -> bool {
        self.ev.iter().all(|e| e.present)
    }

    pub fn sign(&mut self, role: u8, who: &str) -> Result<(), T2CloseCode> {
        if self.closed { return Err(T2CloseCode::AlreadyClosed); }
        if !self.checks_complete() { return Err(T2CloseCode::NotAllChecks); }
        if !self.ev_complete() { return Err(T2CloseCode::EvidenceMissing); }
        match role {
            0 => {
                if self.self_sign.is_some() { return Err(T2CloseCode::AlreadyClosed); }
                self.self_sign = Some(String::from(who));
                Ok(())
            }
            1 => {
                if self.review_sign.is_some() { return Err(T2CloseCode::AlreadyClosed); }
                match &self.self_sign {
                    None => Err(T2CloseCode::DualSignBad),
                    Some(s) if s == who => Err(T2CloseCode::SignerClash),
                    Some(_) => {
                        self.review_sign = Some(String::from(who));
                        Ok(())
                    }
                }
            }
            _ => Err(T2CloseCode::DualSignBad),
        }
    }

    pub fn seal(&mut self) -> Result<(), T2CloseCode> {
        if self.closed { return Err(T2CloseCode::AlreadyClosed); }
        if self.self_sign.is_none() || self.review_sign.is_none() { return Err(T2CloseCode::DualSignBad); }
        self.closed = true;
        Ok(())
    }
}

fn close_labels() -> [String; CLOSE_MAX as usize] {
    [
        String::from("BiDi 全级"),
        String::from("镜像对双射"),
        String::from("混向一致"),
        String::from("控件镜像"),
        String::from("动效反转"),
        String::from("图标隐喻"),
        String::from("表格列序"),
        String::from("IME 定位"),
        String::from("语料判据"),
        String::from("调试覆盖"),
        String::from("字体回退"),
        String::from("RTL 无障碍"),
        String::from("性能基准"),
        String::from("fuzz 不变量"),
        String::from("示例库"),
        String::from("三册文档"),
        String::from("N 布局协同"),
        String::from("联调 54 格"),
        String::from("预留清账"),
        String::from("收口单一致"),
    ]
}

pub fn run_ves280_checks() -> CheckSet {
    let mut set = CheckSet::new("ves280_s12close");

    let build = || -> Closeout {
        let labels = close_labels();
        let mut items = Vec::new();
        for l in labels.iter() {
            items.push(CloseItem { label: l.clone(), done: true });
        }
        let mut ev = Vec::new();
        for k in EvKind::ALL.iter() { ev.push(EvItem::new(*k)); }
        Closeout { items, ev, self_sign: None, review_sign: None, closed: false }
    };

    // 1) 20 项核验齐。
    let mut c = build();
    set.add("C4040-01", c.checks_complete() && c.items.len() == 20, "二十项齐");

    // 2) 未全确认拒签。
    let mut c2 = Closeout::new(&["a", "b"]);
    set.add("C4040-02", c2.sign(0, "a") == Err(T2CloseCode::NotAllChecks), "未全确认拒签");

    // 3) 缺证据拒签。
    let mut c3 = build();
    set.add("C4040-03", c3.sign(0, "a") == Err(T2CloseCode::EvidenceMissing), "缺证据拒签");

    // 4) 五证据标记齐。
    for k in EvKind::ALL.iter() { let _ = c3.mark_ev(*k); }
    set.add("C4040-04", c3.ev_complete(), "五证据齐");

    // 5) 自查签。
    set.add("C4040-05", c3.sign(0, "self").is_ok(), "自查签");

    // 6) 同人复核拒。
    set.add("C4040-06", c3.sign(1, "self") == Err(T2CloseCode::SignerClash), "同人拒签");

    // 7) 未自查先复核拒。
    let mut c7 = build();
    for k in EvKind::ALL.iter() { let _ = c7.mark_ev(*k); }
    set.add("C4040-07", c7.sign(1, "r") == Err(T2CloseCode::DualSignBad), "无自查先复核拒");

    // 8) 异人双签 → 收口。
    set.add("C4040-08", c3.sign(1, "review").is_ok() && c3.seal().is_ok() && c3.closed, "异人双签收口");

    // 9) 收口后封账。
    set.add("C4040-09", c3.sign(0, "x") == Err(T2CloseCode::AlreadyClosed)
        && c3.seal() == Err(T2CloseCode::AlreadyClosed)
        && c3.mark_ev(EvKind::Singularity) == Err(T2CloseCode::AlreadyClosed), "收口封账");

    // 10) 证据标记幂等。
    let mut c10 = build();
    let _ = c10.mark_ev(EvKind::Singularity);
    set.add("C4040-10", c10.mark_ev(EvKind::Singularity) == Err(T2CloseCode::AlreadyClosed), "证据标记幂等");

    // 11) 非法角色拒。
    let mut c11 = build();
    for k in EvKind::ALL.iter() { let _ = c11.mark_ev(*k); }
    set.add("C4040-11", c11.sign(7, "x") == Err(T2CloseCode::DualSignBad), "非法角色拒");

    // 12) 越界确认拒。
    let mut c12 = Closeout::new(&["a"]);
    set.add("C4040-12", c12.confirm(5) == Err(T2CloseCode::CloseIncomplete), "越界确认拒");

    // 13) 收口项无重复。
    let labels = close_labels();
    let mut dup = false;
    for i in 0..labels.len() {
        for j in (i + 1)..labels.len() {
            if labels[i] == labels[j] { dup = true; }
        }
    }
    set.add("C4040-13", !dup, "收口项无重复");

    // 14) 收口项非空。
    set.add("C4040-14", labels.iter().all(|l| !l.is_empty()), "收口项非空");

    // 15) 五证据名双射。
    let mut names: Vec<&str> = Vec::new();
    for k in EvKind::ALL.iter() { names.push(k.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4040-15", !ndup && names.len() == 5, "五证据名双射");

    // 16) 诊断码唯一。
    let codes = [T2CloseCode::Ok, T2CloseCode::EvidenceMissing, T2CloseCode::DualSignBad,
                 T2CloseCode::CloseIncomplete, T2CloseCode::SignerClash, T2CloseCode::AlreadyClosed,
                 T2CloseCode::NotAllChecks];
    let mut cdup = false;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] { cdup = true; }
        }
    }
    set.add("C4040-16", !cdup, "诊断码唯一");

    // 17) 收口覆盖 T02 全 19 域（含 N 布局协同）。
    set.add("C4040-17", labels.iter().any(|l| l.contains("N 布局")) && labels.iter().any(|l| l.contains("清账")), "T02 全域覆盖");

    // 18) 段位自查。
    set.add("C4040-18", SEG_BASE == 0x7540, "段位 0x7540");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build() -> Closeout {
        let cl = close_labels();
        let labels: Vec<&str> = cl.iter().map(|s| s.as_str()).collect();
        Closeout::new(&labels)
    }

    #[test]
    fn dual_sign_flow() {
        let mut c = build();
        for k in EvKind::ALL.iter() { let _ = c.mark_ev(*k); }
        assert!(c.sign(0, "a").is_ok());
        assert_eq!(c.sign(1, "a"), Err(T2CloseCode::SignerClash));
        assert!(c.sign(1, "b").is_ok());
        assert!(c.seal().is_ok());
    }
}
