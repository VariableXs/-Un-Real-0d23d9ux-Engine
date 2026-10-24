//! ves396_s21walkthrough — VE-F4156 · i18n 走查汇总
//! T 域五角色走查发现归集（F4147 用户测试 + S 域走查联动 + 体验日志挫败信号三方汇合）；
//! 发现分级（阻断/重要/建议）+ 处置回执；阻断未消→禁双签；回执缺→催办；分级争议→仲裁；
//! 走查即域本色终验手段（多语种母语档）；录证脱敏。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0020;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WkCode {
    Ok = 0,
    BlockOpen = SEG_BASE + 0x01,     // 0x0021
    ReceiptMissing = SEG_BASE + 0x02, // 0x0022
    GradeDisputed = SEG_BASE + 0x03,  // 0x0023
}

/// 发现来源三方。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WkSource { UserTest = 0, SDomainWalk = 1, FrustrationLog = 2 }

impl WkSource {
    pub const ALL: [WkSource; 3] =
        [WkSource::UserTest, WkSource::SDomainWalk, WkSource::FrustrationLog];
    pub fn name(self) -> &'static str {
        match self {
            WkSource::UserTest => "user-test", WkSource::SDomainWalk => "s-walk",
            WkSource::FrustrationLog => "frustration-log",
        }
    }
}

/// 发现分级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Grade { Suggestion = 0, Major = 1, Blocker = 2 }

impl Grade {
    pub fn name(self) -> &'static str {
        match self { Grade::Suggestion => "suggestion", Grade::Major => "major", Grade::Blocker => "blocker" }
    }
}

/// 走查发现：三方来源 + 语种 + 分级 + 处置回执 + 争议标记。
#[derive(Clone)]
pub struct Finding {
    pub id: u32,
    pub source: WkSource,
    pub lang: &'static str,     // 多语种母语档（域本色：发现须带语种）
    pub desc: &'static str,
    pub grade: Grade,
    pub receipt: bool,          // 处置回执
    pub disputed: bool,         // 分级争议
    pub resolved: bool,         // 阻断已消
}

impl Finding {
    pub fn needs_receipt(&self) -> bool { !self.receipt }
    pub fn is_open_blocker(&self) -> bool { self.grade == Grade::Blocker && !self.resolved }
}

/// 走查汇总册。
pub struct WalkBook {
    pub findings: Vec<Finding>,
    pub arbitrations: Vec<&'static str>, // 分级争议仲裁留痕
    pub urges: Vec<u32>,                 // 催办单
}

impl WalkBook {
    pub fn new() -> Self { WalkBook { findings: Vec::new(), arbitrations: Vec::new(), urges: Vec::new() } }

    /// 归集发现（幂等按 id）。
    pub fn add(&mut self, f: Finding) {
        match self.findings.iter_mut().find(|x| x.id == f.id) {
            Some(x) => *x = f,
            None => self.findings.push(f),
        }
    }

    /// 三方汇合完备。
    pub fn sources_covered(&self) -> bool {
        WkSource::ALL.iter().all(|s| self.findings.iter().any(|f| f.source == *s))
    }

    /// 分级清单。
    pub fn by_grade(&self, g: Grade) -> Vec<u32> {
        self.findings.iter().filter(|f| f.grade == g).map(|f| f.id).collect()
    }

    /// 阻断未消 → 禁双签。
    pub fn blockers_open(&self) -> Vec<u32> {
        self.findings.iter().filter(|f| f.is_open_blocker()).map(|f| f.id).collect()
    }

    pub fn double_sign_allowed(&self) -> bool { self.blockers_open().is_empty() }

    /// 回执缺 → 催办单生成（幂等）。
    pub fn urge_missing_receipts(&mut self) -> Vec<u32> {
        for f in self.findings.iter() {
            if f.needs_receipt() && !self.urges.contains(&f.id) { self.urges.push(f.id); }
        }
        self.urges.clone()
    }

    /// 分级争议 → 仲裁（留痕，按最严级别兜底）。
    pub fn arbitrate(&mut self, id: u32) -> bool {
        match self.findings.iter_mut().find(|f| f.id == id) {
            Some(f) if f.disputed => {
                f.disputed = false;
                if f.grade < Grade::Blocker { f.grade = Grade::Blocker; } // 争议兜最严
                self.arbitrations.push("grade-arbitrated-to-blocker");
                true
            }
            _ => false,
        }
    }

    /// 回执登记。
    pub fn give_receipt(&mut self, id: u32) -> bool {
        match self.findings.iter_mut().find(|f| f.id == id) {
            Some(f) => { f.receipt = true; true }
            None => false,
        }
    }

    /// 录证脱敏核验：发现描述不得含用户原文标记（红线——只走脱敏通道）。
    pub fn sanitized(&self) -> bool {
        self.findings.iter().all(|f| !f.desc.contains("RAW-USER-TEXT"))
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves396_checks() -> CheckSet {
    let mut set = CheckSet::new("ves396");
    let mut bk = WalkBook::new();

    // C396-01 三方发现汇合（含多语种母语档标注）。
    bk.add(Finding { id: 1, source: WkSource::UserTest, lang: "ar", desc: "RTL 镜像断裂", grade: Grade::Blocker, receipt: false, disputed: false, resolved: false });
    bk.add(Finding { id: 2, source: WkSource::SDomainWalk, lang: "ja", desc: "折行规则漂移", grade: Grade::Major, receipt: false, disputed: false, resolved: false });
    bk.add(Finding { id: 3, source: WkSource::FrustrationLog, lang: "de", desc: "切换挫败信号高频", grade: Grade::Suggestion, receipt: false, disputed: false, resolved: false });
    chk(&mut set, "C396-01", bk.sources_covered() && bk.findings.len() == 3, "三方汇合三条");

    // C396-02 分级清单。
    chk(&mut set, "C396-02", bk.by_grade(Grade::Blocker) == vec![1]
        && bk.by_grade(Grade::Major) == vec![2] && bk.by_grade(Grade::Suggestion) == vec![3], "三级各一");

    // C396-03 阻断未消 → 禁双签。
    chk(&mut set, "C396-03", bk.blockers_open() == vec![1] && !bk.double_sign_allowed(), "阻断禁双签");

    // C396-04 回执缺 → 催办单。
    let urges = bk.urge_missing_receipts();
    chk(&mut set, "C396-04", urges == vec![1, 2, 3], "催办三条");

    // C396-05 回执登记后催办清单收敛。
    bk.give_receipt(2);
    bk.give_receipt(3);
    let urges2 = bk.urge_missing_receipts();
    chk(&mut set, "C396-05", urges2 == vec![1], "催办收敛一条");

    // C396-06 分级争议 → 仲裁兜最严。
    bk.findings[1].disputed = true;
    chk(&mut set, "C396-06", bk.arbitrate(2) && bk.findings[1].grade == Grade::Blocker
        && bk.arbitrations.len() == 1, "仲裁兜 Blocker 留痕");

    // C396-07 非争议仲裁拒。
    chk(&mut set, "C396-07", !bk.arbitrate(3), "无争议不仲裁");

    // C396-08 阻断消解后双签放行。
    bk.findings[0].resolved = true;
    chk(&mut set, "C396-08", bk.blockers_open().is_empty() && bk.double_sign_allowed(), "阻断消解放行");

    // C396-09 录证脱敏红线。
    chk(&mut set, "C396-09", bk.sanitized(), "全脱敏合规");

    // C396-10 脏数据检出。
    bk.add(Finding { id: 4, source: WkSource::UserTest, lang: "ko", desc: "含 RAW-USER-TEXT 未脱敏", grade: Grade::Major, receipt: false, disputed: false, resolved: false });
    chk(&mut set, "C396-10", !bk.sanitized(), "未脱敏检出");

    // C396-11 三方名单单源 + 分级有序。
    chk(&mut set, "C396-11", WkSource::ALL.len() == 3 && WkSource::ALL[0].name() == "user-test"
        && Grade::Suggestion < Grade::Major && Grade::Major < Grade::Blocker, "三方三级单源");

    // C396-12 归集幂等覆盖。
    bk.add(Finding { id: 1, source: WkSource::UserTest, lang: "ar", desc: "RTL 镜像断裂", grade: Grade::Blocker, receipt: false, disputed: false, resolved: true });
    chk(&mut set, "C396-12", bk.findings.len() == 4 && bk.double_sign_allowed(), "覆盖不增行");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves396_smoke() {
        let set = run_ves396_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves396 red: {:?}", red);
    }

    #[test]
    fn ves396_urge_idempotent() {
        let mut bk = WalkBook::new();
        bk.add(Finding { id: 9, source: WkSource::UserTest, lang: "fr", desc: "x", grade: Grade::Major, receipt: false, disputed: false, resolved: false });
        let u1 = bk.urge_missing_receipts();
        let u2 = bk.urge_missing_receipts();
        assert_eq!(u1, u2);
    }
}
