//! VE-F4316 · 场景应用与词典（U06 场景应用组）
//! 职责：场景应用与交互词典（U01 三入约之一）深度集成——词典查询随场景
//! 上下文化（同词条按场景给出对应形态示例）+ 缺词反馈通道（场景发现缺词
//! →提案入演进流；积压批量评审）+ 词典版本随场景发布对齐（对齐失败冻结
//! 发版）+ 未命中最近似回退 + 词典即无障碍规则源（域本色）。
//! 段位：0x3240（续表 idx550）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3240;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DkCode {
    /// 词条缺失（提案入演进流）。
    MissingWord = SEG_BASE,
    /// 版本对齐失败（冻结场景发版）。
    VersionMismatch = SEG_BASE + 0x01,
    /// 提案积压（批量评审触发）。
    ProposalBacklog = SEG_BASE + 0x02,
}

/// 词条（按场景上下文化的形态示例）。
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub key: u32,
    /// 场景号 0 = 通用形态；1..=7 场景特化形态。
    pub scene: u8,
    pub form: &'static str,
}

/// 缺词提案。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Proposal {
    pub key: u32,
    pub scene: u8,
    pub reviewed: bool,
}

/// 词典台。
#[derive(Default)]
pub struct DictDesk {
    pub words: Vec<Word>,
    pub proposals: Vec<Proposal>,
    /// 词典版本单源。
    pub dict_ver: u32,
    /// 场景侧已对齐版本。
    pub scene_ver: u32,
    pub reviews: u32,
}

const PROPOSAL_TRIGGER: usize = 6;
/// 最近似回退：汉明距离（位差）最小者。
fn hamming(a: u32, b: u32) -> u32 {
    (a ^ b).count_ones()
}

impl DictDesk {
    pub fn new(ver: u32) -> Self {
        DictDesk { dict_ver: ver, scene_ver: ver, ..Default::default() }
    }

    /// 词条登记（通用 + 场景特化共用表）。
    pub fn put(&mut self, key: u32, scene: u8, form: &'static str) {
        if let Some(w) = self.words.iter_mut().find(|w| w.key == key && w.scene == scene) {
            w.form = form;
        } else {
            self.words.push(Word { key, scene, form });
        }
    }

    /// 上下文化查询：先查场景特化，再退通用；未命中 → 最近似词条 + 缺词
    /// 提案双回（提案登记入演进流）。
    pub fn query(&mut self, key: u32, scene: u8) -> (Option<&'static str>, bool) {
        if let Some(w) = self.words.iter().find(|w| w.key == key && w.scene == scene) {
            return (Some(w.form), false);
        }
        if let Some(w) = self.words.iter().find(|w| w.key == key && w.scene == 0) {
            return (Some(w.form), false);
        }
        // 未命中：登记缺词提案（幂等）。
        if !self.proposals.iter().any(|p| p.key == key && p.scene == scene) {
            self.proposals.push(Proposal { key, scene, reviewed: false });
        }
        let near = self.nearest(key);
        (near, true)
    }

    /// 最近似词条（位差最小；空表返回 None）。
    pub fn nearest(&self, key: u32) -> Option<&'static str> {
        let mut best: Option<(u32, &'static str)> = None;
        for w in self.words.iter() {
            let d = hamming(w.key, key);
            match best {
                Some((bd, _)) if bd <= d => {}
                _ => best = Some((d, w.form)),
            }
        }
        best.map(|(_, f)| f)
    }

    /// 提案积压检测。
    pub fn backlog(&self) -> bool {
        self.proposals.iter().filter(|p| !p.reviewed).count() >= PROPOSAL_TRIGGER
    }

    /// 批量评审：积压提案一批全审（通过 = 词条入册）。
    pub fn review_batch(&mut self, forms: &[(u32, u8, &'static str)]) -> usize {
        let mut accepted: Vec<(u32, u8, &'static str)> = Vec::new();
        for p in self.proposals.iter_mut() {
            if p.reviewed {
                continue;
            }
            if let Some((k, s, f)) = forms.iter().find(|(k, s, _)| *k == p.key && *s == p.scene) {
                accepted.push((*k, *s, *f));
                p.reviewed = true;
            }
        }
        let n = accepted.len();
        for (k, s, f) in accepted {
            self.put(k, s, f);
        }
        self.reviews += n as u32;
        n
    }

    /// 版本对齐：词典版本演进后场景侧须跟进；不符 = 冻结发版信号。
    pub fn aligned(&self) -> Result<(), DkCode> {
        if self.scene_ver == self.dict_ver {
            Ok(())
        } else {
            Err(DkCode::VersionMismatch)
        }
    }

    /// 场景侧跟版。
    pub fn sync(&mut self) {
        self.scene_ver = self.dict_ver;
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves550_checks() -> CheckSet {
    let mut set = CheckSet::new("ves550");
    let mut dd = DictDesk::new(41);

    // C550-01 通用词条 + 场景特化登记。
    dd.put(0x1001, 0, "确认");
    dd.put(0x1001, 3, "收口确认");
    let (f1, miss1) = dd.query(0x1001, 3);
    chk(&mut set, "C550-01", f1 == Some("收口确认") && !miss1, "场景特化优先");

    // C550-02 特化缺失退通用形态。
    let (f2, miss2) = dd.query(0x1001, 5);
    chk(&mut set, "C550-02", f2 == Some("确认") && !miss2, "退通用形态");

    // C550-03 未命中：最近似回退 + 缺词提案双回。
    let (f3, miss3) = dd.query(0x1002, 1);
    chk(&mut set, "C550-03", miss3 && f3.is_some() && dd.proposals.len() == 1, "最近似+提案双回");

    // C550-04 提案幂等（同词同景不重复登记）。
    let _ = dd.query(0x1002, 1);
    chk(&mut set, "C550-04", dd.proposals.len() == 1, "提案幂等");

    // C550-05 最近似语义：位差最小者胜。
    dd.put(0x1004, 0, "邻近词");
    chk(&mut set, "C550-05", dd.nearest(0x1006) == Some("邻近词"), "位差最近胜出");

    // C550-06 提案积压检测（阈值 6）。
    for k in 0x2000..0x2006u32 {
        let _ = dd.query(k, 2);
    }
    chk(&mut set, "C550-06", dd.backlog() && dd.proposals.len() == 7, "提案积压检测");

    // C550-07 批量评审入册（积压清空）。
    let forms: Vec<(u32, u8, &'static str)> = vec![
        (0x1002, 1, "提交"), (0x2000, 2, "甲词"), (0x2001, 2, "乙词"),
    ];
    let n = dd.review_batch(&forms);
    chk(&mut set, "C550-07", n == 3 && dd.reviews == 3, "批量评审入册");

    // C550-08 评审后词条可查（演进流闭环）。
    let (f8, miss8) = dd.query(0x1002, 1);
    chk(&mut set, "C550-08", f8 == Some("提交") && !miss8, "评审后可查");

    // C550-09 版本对齐：同版放行。
    chk(&mut set, "C550-09", dd.aligned().is_ok(), "同版对齐放行");

    // C550-10 版本漂移冻结发版信号 + 跟版解除。
    dd.dict_ver = 42;
    chk(&mut set, "C550-10", dd.aligned() == Err(DkCode::VersionMismatch), "漂移冻结信号");
    dd.sync();
    chk(&mut set, "C550-10b", dd.aligned().is_ok(), "跟版解除");

    // C550-11 评审幂等（已审不重复计）。
    let n2 = dd.review_batch(&forms);
    chk(&mut set, "C550-11", n2 == 0 && dd.reviews == 3, "评审幂等");

    // C550-12 空表最近似返回 None（不臆测）。
    let dd2 = DictDesk::new(1);
    chk(&mut set, "C550-12", dd2.nearest(9).is_none(), "空表不臆测");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contextual_lookup() {
        let mut dd = DictDesk::new(1);
        dd.put(1, 0, "通用");
        dd.put(1, 2, "特化");
        assert_eq!(dd.query(1, 2), (Some("特化"), false));
        assert_eq!(dd.query(1, 4), (Some("通用"), false));
        let (f, miss) = dd.query(9, 1);
        assert!(miss && f.is_none());
    }

    #[test]
    fn nearest_hamming() {
        let mut dd = DictDesk::new(1);
        dd.put(0b0001, 0, "甲");
        dd.put(0b1000, 0, "乙");
        assert_eq!(dd.nearest(0b0011), Some("甲"));
        assert_eq!(dd.nearest(0b1100), Some("乙"));
    }

    #[test]
    fn version_gate() {
        let mut dd = DictDesk::new(3);
        assert!(dd.aligned().is_ok());
        dd.dict_ver = 4;
        assert_eq!(dd.aligned(), Err(DkCode::VersionMismatch));
        dd.sync();
        assert!(dd.aligned().is_ok());
    }
}
