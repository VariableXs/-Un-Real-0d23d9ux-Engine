//! ves294_s14docs — VE-F4054 · T03 排版文档
//! 开发者册（排版规则/接入指南——义务前置）+ 标准册（禁则表/断行标准公开）+ 用户册（语言设置指南）+ 同源纪律 + 双重自举 + 2 跳可达。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x80C0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum TDocCode {
    Ok = 0,
    DutyMissing = SEG_BASE + 0x01,
    Drift = SEG_BASE + 0x02,
    BootstrapFail = SEG_BASE + 0x03,
    MazeDeep = SEG_BASE + 0x04,
    NotPublic = SEG_BASE + 0x05,
    HandbookMissing = SEG_BASE + 0x06,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BookKind { Dev = 0, Standard = 1, User = 2 }

impl BookKind {
    pub const ALL: [BookKind; 3] = [BookKind::Dev, BookKind::Standard, BookKind::User];
}

#[derive(Clone)]
pub struct DocPage {
    pub book: BookKind,
    pub title: String,
    pub duty_section: bool,
    pub public: bool,
    pub links: Vec<String>,
}

impl DocPage {
    pub fn new(book: BookKind, title: &str) -> Self {
        DocPage { book, title: String::from(title), duty_section: false, public: false, links: Vec::new() }
    }
    pub fn link(&mut self, to: &str) { self.links.push(String::from(to)); }
}

pub struct TDocSet {
    pub pages: Vec<DocPage>,
}

impl TDocSet {
    pub fn new() -> Self { TDocSet { pages: Vec::new() } }

    pub fn add(&mut self, p: DocPage) -> Result<(), TDocCode> {
        for it in self.pages.iter() {
            if it.book == p.book && it.title == p.title { return Err(TDocCode::Drift); }
        }
        self.pages.push(p);
        Ok(())
    }

    pub fn find(&self, title: &str) -> Option<usize> {
        for (i, p) in self.pages.iter().enumerate() {
            if p.title == title { return Some(i); }
        }
        None
    }

    /// 义务前置：开发者册排版规则/接入指南页必须带义务章。
    pub fn duty_audit(&self) -> Result<(), TDocCode> {
        for p in self.pages.iter() {
            if p.book == BookKind::Dev && (p.title.contains("排版规则") || p.title.contains("接入指南")) && !p.duty_section {
                return Err(TDocCode::DutyMissing);
            }
        }
        Ok(())
    }

    /// 标准册公开：禁则表/断行标准必须 public。
    pub fn public_audit(&self) -> Result<(), TDocCode> {
        for p in self.pages.iter() {
            if p.book == BookKind::Standard && (p.title.contains("禁则表") || p.title.contains("断行标准")) && !p.public {
                return Err(TDocCode::NotPublic);
            }
        }
        Ok(())
    }

    pub fn handbook_present(&self) -> Result<(), TDocCode> {
        if self.find("语言设置指南").is_some() { Ok(()) } else { Err(TDocCode::HandbookMissing) }
    }

    pub fn reachable_2hop(&self, from: &str, to: &str) -> bool {
        let start = match self.find(from) { Some(i) => i, None => return false };
        for l1 in self.pages[start].links.iter() {
            if l1 == to { return true; }
            if let Some(i1) = self.find(l1) {
                for l2 in self.pages[i1].links.iter() {
                    if l2 == to { return true; }
                }
            }
        }
        false
    }

    pub fn link_integrity(&self) -> Result<(), TDocCode> {
        for p in self.pages.iter() {
            for l in p.links.iter() {
                if self.find(l).is_none() { return Err(TDocCode::MazeDeep); }
            }
        }
        Ok(())
    }

    pub fn double_bootstrap(&self) -> Result<(), TDocCode> {
        for k in BookKind::ALL.iter() {
            if !self.pages.iter().any(|p| p.book == *k) { return Err(TDocCode::BootstrapFail); }
        }
        self.handbook_present()?;
        self.duty_audit()?;
        self.public_audit()?;
        Ok(())
    }

    pub fn reconcile(&self, manifest: &[&str]) -> bool {
        if manifest.len() != self.pages.len() { return false; }
        manifest.iter().all(|m| self.find(m).is_some())
    }
}

pub fn run_ves294_checks() -> CheckSet {
    let mut set = CheckSet::new("ves294_s14docs");
    let mut ds = TDocSet::new();

    let mut p1 = DocPage::new(BookKind::Dev, "排版规则");
    p1.duty_section = true;
    p1.link("断行标准");
    let mut p2 = DocPage::new(BookKind::Standard, "禁则表");
    p2.public = true;
    let mut p3 = DocPage::new(BookKind::Standard, "断行标准");
    p3.public = true;
    let _ = ds.add(p1);
    let _ = ds.add(p2);
    let _ = ds.add(p3);
    let _ = ds.add(DocPage::new(BookKind::User, "语言设置指南"));

    // 1) 三册齐。
    set.add("C4054-01", BookKind::ALL.iter().all(|k| ds.pages.iter().any(|p| p.book == *k)), "三册齐");

    // 2) 义务前置过审。
    set.add("C4054-02", ds.duty_audit().is_ok(), "义务前置");

    // 3) 义务章缺拒。
    let mut ds2 = TDocSet::new();
    let _ = ds2.add(DocPage::new(BookKind::Dev, "接入指南"));
    set.add("C4054-03", ds2.duty_audit() == Err(TDocCode::DutyMissing), "义务章缺拒");

    // 4) 标准册公开。
    set.add("C4054-04", ds.public_audit().is_ok(), "标准册公开");

    // 5) 未公开拒。
    let mut ds3 = TDocSet::new();
    let _ = ds3.add(DocPage::new(BookKind::Standard, "断行标准"));
    set.add("C4054-05", ds3.public_audit() == Err(TDocCode::NotPublic), "未公开拒");

    // 6) 手册在册。
    set.add("C4054-06", ds.handbook_present().is_ok(), "手册在册");

    // 7) 2 跳可达。
    set.add("C4054-07", ds.reachable_2hop("排版规则", "断行标准"), "可达");

    // 8) 幽灵页不可达。
    set.add("C4054-08", !ds.reachable_2hop("排版规则", "无此页"), "幽灵不可达");

    // 9) 出链完整。
    set.add("C4054-09", ds.link_integrity().is_ok(), "出链完整");

    // 10) 悬空链检出。
    let mut ds4 = TDocSet::new();
    let mut d4 = DocPage::new(BookKind::Dev, "悬空");
    d4.link("无此页");
    let _ = ds4.add(d4);
    set.add("C4054-10", ds4.link_integrity() == Err(TDocCode::MazeDeep), "悬空检出");

    // 11) 双重自举。
    set.add("C4054-11", ds.double_bootstrap().is_ok(), "双重自举");

    // 12) 同源对账。
    set.add("C4054-12", ds.reconcile(&["排版规则", "禁则表", "断行标准", "语言设置指南"]), "同源对账");

    // 13) 对账错位检出。
    set.add("C4054-13", !ds.reconcile(&["排版规则"]), "对账错位");

    // 14) 重复页拒。
    set.add("C4054-14", ds.add(DocPage::new(BookKind::Dev, "排版规则")) == Err(TDocCode::Drift), "重复页拒");

    // 15) 段位自查。
    set.add("C4054-15", SEG_BASE == 0x80C0, "段位 0x80C0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duty_and_boot() {
        let mut ds = TDocSet::new();
        let mut p = DocPage::new(BookKind::Dev, "排版规则");
        p.duty_section = true;
        let _ = ds.add(p);
        assert!(ds.duty_audit().is_ok());
    }
}
