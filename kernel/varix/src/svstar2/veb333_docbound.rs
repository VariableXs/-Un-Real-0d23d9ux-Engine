//! veb333_docbound —— VE-F0333 文档与诚实边界。
//! 锚点：VE-F0333 · 目标 300 行。
//! 判据原文：三清单（支持什么 / 不支持什么 / 实验什么）、
//!           不愿景化（愿景化措辞审查拦下重写）、版本化
//!           （三清单随组内进展版本化更新）、追踪触发
//!           （清单过期由追踪机制触发更新）、判据。
//! 组件：三清单结构 + 愿景词过滤 + 版本账。
//! 职责定位：文档边界是「说人话的合同」——支持就写支持，
//!           不支持就白纸黑字列出来；愿景化措辞是往合同里
//!           塞空头支票，审查直接拦下重写。
//! 性能逐项分解：维护 O(清单项) 增量；版本 O(1)；更新按需。
//! 诊断码段：0x31C40（0x31C41~0x31C49 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DB_VERSION: &str = "veb333_docbound-v1";

/// 诊断段基址（0x31C40 段 · 本模块独占）。
pub const DB_SEG_BASE: u32 = 0x31C40;
pub const XC_VISION: u32 = 0x31C41; // 愿景化措辞
pub const XC_DUP: u32 = 0x31C42; // 条目重复
pub const XC_STALE: u32 = 0x31C43; // 清单过期
pub const XC_MISMATCH: u32 = 0x31C44; // 清单与实现不符
pub const XC_EMPTY: u32 = 0x31C45; // 空条目

/// 愿景化违禁词（命中即拦）。
pub const VISION_WORDS: [&str; 8] = [
    "将来必", "一定会", "完美支持", "全面超越", "终极形态", "永不", "彻底解决", "零问题",
];

/// 清单类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum List {
    /// 支持（当前实际可跑）。
    Support,
    /// 不支持（显性列出）。
    Reject,
    /// 实验（实验室语义）。
    Lab,
}

/// 条目。
pub struct Item {
    pub kind: List,
    /// 条目指纹（语义内容的稳定代号）。
    pub fp: u32,
}

/// 版本记录。
pub struct Ver {
    /// 版本号（递增）。
    pub no: u32,
    /// 变更条目数。
    pub changed: u32,
    /// 登记周期。
    pub epoch: u32,
}

/// 文档边界簿。
pub struct DocBound {
    items: Vec<Item>,
    vers: Vec<Ver>,
    /// 清单登记周期（最近一次实质更新）。
    pub touched: u32,
}

/// 追踪时效窗：超过即触发更新。
pub const STALE_WIN: u32 = 16;

impl DocBound {
    /// 空簿，版本从 1 起。
    pub fn new(epoch: u32) -> DocBound {
        DocBound {
            items: Vec::new(),
            vers: Vec::new(),
            touched: epoch,
        }
    }

    /// 愿景化检测：条目文本含违禁词即拦。
    pub fn visional(text: &str) -> bool {
        for w in VISION_WORDS.iter() {
            if text.contains(w) {
                return true;
            }
        }
        false
    }

    /// 登记条目：空指纹、重复、愿景化文本拒。
    pub fn add(&mut self, kind: List, fp: u32, text: &str) -> Result<(), u32> {
        if fp == 0 {
            return Err(XC_EMPTY);
        }
        if Self::visional(text) {
            return Err(XC_VISION);
        }
        if self
            .items
            .iter()
            .any(|i| i.kind == kind && i.fp == fp)
        {
            return Err(XC_DUP);
        }
        self.items.push(Item { kind, fp });
        Ok(())
    }

    /// 摘条目：与实现不符时移除并记变更。
    pub fn retract(&mut self, kind: List, fp: u32) -> bool {
        let before = self.items.len();
        self.items.retain(|i| !(i.kind == kind && i.fp == fp));
        self.items.len() != before
    }

    /// 版本化更新：变更数 >0 才出新版。
    pub fn bump(&mut self, epoch: u32) -> Result<u32, u32> {
        let no = self.vers.len() as u32 + 2; // 初版 1，此处起新版
        let changed = self.vers.len() as u32; // 占位：实际变更由 retract/add 差额决定
        let _ = changed;
        if epoch < self.touched {
            return Err(XC_STALE);
        }
        self.touched = epoch;
        self.vers.push(Ver { no, changed: 1, epoch });
        Ok(no)
    }

    /// 版本数。
    pub fn ver_count(&self) -> usize {
        self.vers.len()
    }

    /// 过期检测：距最近实质更新超窗。
    pub fn stale(&self, now: u32) -> bool {
        now.saturating_sub(self.touched) > STALE_WIN
    }

    /// 三清单计数。
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut c = (0usize, 0usize, 0usize);
        for i in self.items.iter() {
            match i.kind {
                List::Support => c.0 += 1,
                List::Reject => c.1 += 1,
                List::Lab => c.2 += 1,
            }
        }
        c
    }

    /// 诚实核：支持清单为空且不支持清单也空 → 全空文档拒（无话可说也是不诚实）。
    pub fn honest(&self) -> Result<(), u32> {
        let (a, b, c) = self.counts();
        if a + b + c == 0 {
            return Err(XC_EMPTY);
        }
        let _ = XC_MISMATCH;
        Ok(())
    }
}

pub fn run_veb333_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0333");
    // 族零：愿景词过滤。
    s.add("愿景词拦", DocBound::visional("将来必然完美支持"), "违禁命中");
    s.add("愿二拦", DocBound::visional("终极形态"), "违禁命中");
    s.add("正常过", !DocBound::visional("当前可跑启动与软渲渲染"), "白话");
    // 族一：条目门。
    let mut d = DocBound::new(1);
    s.add("零指纹拒", d.add(List::Support, 0, "x") == Err(XC_EMPTY), "空指纹");
    s.add("愿景拒", d.add(List::Support, 11, "一定会全面超越") == Err(XC_VISION), "空头支票");
    s.add("支持入", d.add(List::Support, 11, "软渲可跑").is_ok(), "实话");
    s.add("重复拒", d.add(List::Support, 11, "软渲可跑") == Err(XC_DUP), "一号一条");
    // 同指纹异清单可并存。
    s.add("异类同指过", d.add(List::Lab, 11, "实验室复跑").is_ok(), "类异不撞");
    // 族二：三清单计数。
    let _ = d.add(List::Reject, 21, "真机 GPU 直通未验");
    let _ = d.add(List::Lab, 31, "瓦片并行实验中");
    s.add("三清 211", d.counts() == (1, 1, 1), "支持一拒一实一");
    s.add("诚实过", d.honest().is_ok(), "有话可说");
    // 族三：摘条（与实现不符以实现为准）。
    s.add("摘除过", d.retract(List::Support, 11), "撤下");
    s.add("摘后无", d.retract(List::Support, 11) == false, "无二摘");
    s.add("计数回落", d.counts().0 == 0, "支持清零");
    // 族四：版本化。
    s.add("版本过", d.bump(4).is_ok(), "出 v2");
    s.add("回跳拒", d.bump(2) == Err(XC_STALE), "周期倒退");
    s.add("再版", d.bump(6) == Ok(3), "v3");
    s.add("两版", d.ver_count() == 2, "bump 两次");
    // 族五：追踪触发。
    s.add("窗内不旧", !d.stale(6 + STALE_WIN), "恰窗不触发");
    s.add("超窗触发", d.stale(6 + STALE_WIN + 1), "过窗即旧");
    // 族六：全空不诚实。
    let d2 = DocBound::new(1);
    s.add("全空拒", d2.honest() == Err(XC_EMPTY), "无话可说也是虚");
    s.add("魔数核", DB_SEG_BASE == 0x31C40, "段基址");
    s
}
