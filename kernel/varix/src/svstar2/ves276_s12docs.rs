//! ves276_s12docs — VE-F4036 · RTL 文档
//! 开发者册（BiDi/镜像规则/豁免清单——义务前置）+ 用户册（RTL 语言切换指南）+ 标准册（镜像表/隐喻审查公开）+ 同源纪律 + 双重自举 + 2 跳可达。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x70C0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum DocCode {
    Ok = 0,
    DutyMissing = SEG_BASE + 0x01,
    DriftDetected = SEG_BASE + 0x02,
    BootstrapFail = SEG_BASE + 0x03,
    MazeDeep = SEG_BASE + 0x04,
    NotPublic = SEG_BASE + 0x05,
    HandbookMissing = SEG_BASE + 0x06,
}

/// 三册结构。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BookKind { Dev = 0, User = 1, Standard = 2 }

impl BookKind {
    pub const ALL: [BookKind; 3] = [BookKind::Dev, BookKind::User, BookKind::Standard];
    pub fn name(self) -> &'static str {
        match self { BookKind::Dev => "开发者册", BookKind::User => "用户册", BookKind::Standard => "标准册" }
    }
}

#[derive(Clone)]
pub struct DocPage {
    pub book: BookKind,
    pub title: String,
    /// 义务前置：开发者册的镜像规则页必须带义务章。
    pub duty_section: bool,
    /// 公开标记（标准册镜像表/隐喻审查必须公开）。
    pub public: bool,
    /// 出链（页 → 页标题引用，用于 2 跳可达与自举）。
    pub links: Vec<String>,
}

impl DocPage {
    pub fn new(book: BookKind, title: &str) -> Self {
        DocPage { book, title: String::from(title), duty_section: false, public: false, links: Vec::new() }
    }
    pub fn link(&mut self, to: &str) { self.links.push(String::from(to)); }
}

pub struct RtlDocSet {
    pub pages: Vec<DocPage>,
}

impl RtlDocSet {
    pub fn new() -> Self { RtlDocSet { pages: Vec::new() } }

    pub fn add(&mut self, p: DocPage) -> Result<(), DocCode> {
        for it in self.pages.iter() {
            if it.book == p.book && it.title == p.title { return Err(DocCode::DriftDetected); }
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

    /// 义务前置审查：开发者册中 BiDi/镜像规则页必须带义务章。
    pub fn duty_audit(&self) -> Result<(), DocCode> {
        for p in self.pages.iter() {
            if p.book == BookKind::Dev && (p.title.contains("BiDi") || p.title.contains("镜像规则")) && !p.duty_section {
                return Err(DocCode::DutyMissing);
            }
        }
        Ok(())
    }

    /// 标准册公开审查：镜像表/隐喻审查页必须 public。
    pub fn public_audit(&self) -> Result<(), DocCode> {
        for p in self.pages.iter() {
            if p.book == BookKind::Standard && (p.title.contains("镜像表") || p.title.contains("隐喻审查")) && !p.public {
                return Err(DocCode::NotPublic);
            }
        }
        Ok(())
    }

    /// 用户册手册齐备：语言切换指南在册。
    pub fn handbook_present(&self) -> Result<(), DocCode> {
        if self.find("RTL 语言切换指南").is_some() { Ok(()) } else { Err(DocCode::HandbookMissing) }
    }

    /// 2 跳可达：从入口页最多两跳可达目标页。
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

    /// 迷宫审查：任何页出链不超过 2 跳仍可回到入口（此处断言出链目标存在）。
    pub fn link_integrity(&self) -> Result<(), DocCode> {
        for p in self.pages.iter() {
            for l in p.links.iter() {
                if self.find(l).is_none() { return Err(DocCode::MazeDeep); }
            }
        }
        Ok(())
    }

    /// 双重自举：文档集自身可由同源清单重建（此处断言三册齐 + 指南在册 + 义务章全过）。
    pub fn double_bootstrap(&self) -> Result<(), DocCode> {
        for k in BookKind::ALL.iter() {
            if !self.pages.iter().any(|p| p.book == *k) { return Err(DocCode::BootstrapFail); }
        }
        self.handbook_present()?;
        self.duty_audit()?;
        self.public_audit()?;
        Ok(())
    }

    /// 同源对账：同源清单条目（页标题集合）与在册页一致。
    pub fn reconcile(&self, manifest: &[&str]) -> bool {
        if manifest.len() != self.pages.len() { return false; }
        for m in manifest.iter() {
            if self.find(m).is_none() { return false; }
        }
        true
    }
}

pub fn run_ves276_checks() -> CheckSet {
    let mut set = CheckSet::new("ves276_s12docs");
    let mut ds = RtlDocSet::new();

    let mut dev_bidi = DocPage::new(BookKind::Dev, "BiDi 规则");
    dev_bidi.duty_section = true;
    dev_bidi.link("镜像规则");
    let mut dev_mirror = DocPage::new(BookKind::Dev, "镜像规则");
    dev_mirror.duty_section = true;
    let _ = ds.add(dev_bidi);
    let _ = ds.add(dev_mirror);
    let mut std_table = DocPage::new(BookKind::Standard, "镜像表");
    std_table.public = true;
    let mut std_meta = DocPage::new(BookKind::Standard, "隐喻审查");
    std_meta.public = true;
    let _ = ds.add(std_table);
    let _ = ds.add(std_meta);
    let _ = ds.add(DocPage::new(BookKind::User, "RTL 语言切换指南"));

    // 1) 三册齐。
    let mut books_seen = [false; 3];
    for p in ds.pages.iter() { books_seen[p.book as usize] = true; }
    set.add("C4036-01", books_seen == [true, true, true], "三册齐");

    // 2) 义务前置审查通过。
    set.add("C4036-02", ds.duty_audit().is_ok(), "义务前置过审");

    // 3) 义务章缺 → 拒。
    let mut ds2 = RtlDocSet::new();
    let _ = ds2.add(DocPage::new(BookKind::Dev, "BiDi 规则"));
    set.add("C4036-03", ds2.duty_audit() == Err(DocCode::DutyMissing), "义务章缺拒");

    // 4) 标准册公开审查通过。
    set.add("C4036-04", ds.public_audit().is_ok(), "标准册公开");

    // 5) 未公开 → 拒。
    let mut ds3 = RtlDocSet::new();
    let _ = ds3.add(DocPage::new(BookKind::Standard, "镜像表"));
    set.add("C4036-05", ds3.public_audit() == Err(DocCode::NotPublic), "未公开拒");

    // 6) 手册在册。
    set.add("C4036-06", ds.handbook_present().is_ok(), "手册在册");

    // 7) 手册缺 → 拒。
    set.add("C4036-07", ds3.handbook_present() == Err(DocCode::HandbookMissing), "手册缺拒");

    // 8) 2 跳可达：BiDi 规则 → 镜像规则（1 跳）。
    set.add("C4036-08", ds.reachable_2hop("BiDi 规则", "镜像规则"), "1 跳可达");

    // 9) 不存在目标 → 不可达。
    set.add("C4036-09", !ds.reachable_2hop("BiDi 规则", "不存在页"), "幽灵页不可达");

    // 10) 出链完整性。
    set.add("C4036-10", ds.link_integrity().is_ok(), "出链完整");

    // 11) 悬空出链检出。
    let mut ds4 = RtlDocSet::new();
    let mut dangling = DocPage::new(BookKind::Dev, "悬空页");
    dangling.link("无此页");
    let _ = ds4.add(dangling);
    set.add("C4036-11", ds4.link_integrity() == Err(DocCode::MazeDeep), "悬空链检出");

    // 12) 双重自举通过。
    set.add("C4036-12", ds.double_bootstrap().is_ok(), "双重自举");

    // 13) 同源对账。
    set.add("C4036-13", ds.reconcile(&["BiDi 规则", "镜像规则", "镜像表", "隐喻审查", "RTL 语言切换指南"]), "同源对账");

    // 14) 同源清单错位检出。
    set.add("C4036-14", !ds.reconcile(&["BiDi 规则", "镜像规则"]), "对账错位检出");

    // 15) 重复页拒。
    set.add("C4036-15", ds.add(DocPage::new(BookKind::Dev, "BiDi 规则")) == Err(DocCode::DriftDetected), "重复页拒");

    // 16) 三册名双射。
    let mut names: Vec<&str> = Vec::new();
    for k in BookKind::ALL.iter() { names.push(k.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4036-16", !ndup, "三册名双射");

    // 17) 段位自查。
    set.add("C4036-17", SEG_BASE == 0x70C0, "段位 0x70C0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duty_and_reach() {
        let mut ds = RtlDocSet::new();
        let mut p = DocPage::new(BookKind::Dev, "BiDi 规则");
        p.duty_section = true;
        let _ = ds.add(p);
        assert!(ds.duty_audit().is_ok());
    }
}
