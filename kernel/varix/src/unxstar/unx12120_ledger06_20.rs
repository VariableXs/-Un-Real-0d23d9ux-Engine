//! UNX-F12120 B06 批域内集成账（含 RtlAllocateHeap 对接预告）（0x1a120 段）
//!
//! 卡面：B06 批域内集成——每卡绑定其堆/池挂点对（RtlAllocateHeap/
//! RtlFreeHeap 预告口径），挂点编号闭集、逐卡单挂、全挂闭合。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 卡号闭集：挂点绑定须落在 B06 卡号闭集 [12101,12118]，越界必拒。
//! - J2 挂点闭集：堆挂点编号闭集 [1,8]，越码必拒。
//! - J3 逐卡单挂：同卡二次绑定必拒。
//! - J4 挂点复用显性：同挂点绑多卡允许但逐对可查（一对一行账）。
//! - J5 全挂闭合：十八卡全绑后闭合可判。
//! - J6 缺挂显性：未绑卡清单可回读。
//! - J7 印账冻结：seal 后绑定必拒。
//! - J8 注入五况全抓。
//!
//! 结构：HookPair (卡号, 挂点) 绑定 + HookLedger 绑定账 + 折叠指纹
//! （211 进制卷积）+ 注入核（越卡号/越挂点/重卡绑/未绑闭合/印后绑定）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a120 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a120;
pub const H6_BAD_CARD: u32 = SEG_BASE + 0x01; // 越卡号
pub const H6_BAD_HOOK: u32 = SEG_BASE + 0x02; // 越挂点
pub const H6_DUP_CARD: u32 = SEG_BASE + 0x03; // 重卡绑
pub const H6_NOT_CLOSED: u32 = SEG_BASE + 0x04; // 未绑闭合
pub const H6_FROZEN: u32 = SEG_BASE + 0x05; // 印账后绑定

/// B06 卡号闭集。
pub const B06_LO: u32 = 12101;
pub const B06_HI: u32 = 12118;

/// 堆挂点闭集上界（RtlAllocateHeap/RtlFreeHeap 对编号 1..=8）。
pub const HOOK_HI: u32 = 8;

/// 挂点绑定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HookPair {
    pub card: u32,
    pub hook: u32,
}

impl HookPair {
    /// 绑定校验：卡号闭集 → 挂点闭集。
    pub fn validate(&self) -> Result<(), u32> {
        if self.card < B06_LO || self.card > B06_HI {
            return Err(H6_BAD_CARD);
        }
        if self.hook == 0 || self.hook > HOOK_HI {
            return Err(H6_BAD_HOOK);
        }
        Ok(())
    }
    pub fn render(&self) -> String {
        format!("H{}|hook{}", self.card, self.hook)
    }
}

/// 绑定账。
pub struct HookLedger {
    pub pairs: Vec<HookPair>,
    pub sealed: bool,
}

impl HookLedger {
    pub fn new() -> HookLedger {
        HookLedger { pairs: Vec::new(), sealed: false }
    }

    /// 绑定：对校验 → 逐卡单挂。
    pub fn bind(&mut self, card: u32, hook: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(H6_FROZEN);
        }
        let p = HookPair { card, hook };
        p.validate()?;
        if self.pairs.iter().any(|q| q.card == card) {
            return Err(H6_DUP_CARD);
        }
        self.pairs.push(p);
        Ok(())
    }

    /// 卡绑定查询。
    pub fn hook_of(&self, card: u32) -> Option<u32> {
        self.pairs.iter().find(|p| p.card == card).map(|p| p.hook)
    }

    /// 挂点占用清单（同挂点多卡允许）。
    pub fn cards_on_hook(&self, hook: u32) -> Vec<u32> {
        self.pairs.iter().filter(|p| p.hook == hook).map(|p| p.card).collect()
    }

    /// 未绑卡清单。
    pub fn missing_cards(&self) -> Vec<u32> {
        let mut v = Vec::new();
        for c in B06_LO..=B06_HI {
            if self.hook_of(c).is_none() {
                v.push(c);
            }
        }
        v
    }

    /// 全挂闭合：十八卡全绑。
    pub fn closed(&self) -> bool {
        self.pairs.len() == (B06_HI - B06_LO + 1) as usize
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for p in &self.pairs {
            h = h.wrapping_mul(211).wrapping_add(p.card);
            h = h.wrapping_mul(211).wrapping_add(p.hook);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("pairs={}/{}", self.pairs.len(), B06_HI - B06_LO + 1)
    }

    /// 诊断面：绑定渲染行。
    pub fn line_of(&self, card: u32) -> String {
        match self.hook_of(card) {
            Some(h) => format!("S{}|h{}", card, h),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut l = HookLedger::new();
    // 1) 越卡号必拒。
    got[0] = l.bind(12100, 1).err() == Some(H6_BAD_CARD);
    // 2) 越挂点必拒。
    got[1] = l.bind(12101, 9).err() == Some(H6_BAD_HOOK);
    // 3) 重卡绑必拒。
    let _ = l.bind(12101, 1);
    got[2] = l.bind(12101, 2).err() == Some(H6_DUP_CARD);
    // 4) 未绑闭合判假且缺挂清单非空。
    got[3] = !l.closed() && l.missing_cards().len() == 17;
    // 5) 印账后绑定必抓。
    l.seal();
    got[4] = l.bind(12102, 1).err() == Some(H6_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12120_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12120");

    // J1 卡号闭集：双向越界皆拒。
    let j1 = {
        let p1 = HookPair { card: 12100, hook: 1 };
        let p2 = HookPair { card: 12119, hook: 1 };
        p1.validate().err() == Some(H6_BAD_CARD)
            && p2.validate().err() == Some(H6_BAD_CARD)
    };
    s.add("J1-卡号闭集", j1, "12100 与 12119 皆拒");

    // J2 挂点闭集。
    let j2 = {
        let p0 = HookPair { card: 12101, hook: 0 };
        let p9 = HookPair { card: 12101, hook: 9 };
        let ok = HookPair { card: 12101, hook: HOOK_HI };
        p0.validate().err() == Some(H6_BAD_HOOK)
            && p9.validate().err() == Some(H6_BAD_HOOK)
            && ok.validate().is_ok()
    };
    s.add("J2-挂点闭集", j2, "0 与 9 拒、8 过");

    // J3 逐卡单挂。
    let j3 = {
        let mut l = HookLedger::new();
        let _ = l.bind(12105, 2);
        l.bind(12105, 3).err() == Some(H6_DUP_CARD)
    };
    s.add("J3-逐卡单挂", j3, "同卡二次必拒");

    // J4 挂点复用显性：多卡同挂逐对可查。
    let j4 = {
        let mut l = HookLedger::new();
        let _ = l.bind(12101, 1);
        let _ = l.bind(12102, 1);
        l.cards_on_hook(1).len() == 2
    };
    s.add("J4-挂点复用", j4, "同挂双卡在账");

    // J5 全挂闭合。
    let j5 = {
        let mut l = HookLedger::new();
        for c in B06_LO..=B06_HI {
            let _ = l.bind(c, (c % HOOK_HI) + 1);
        }
        l.closed() && l.missing_cards().is_empty()
    };
    s.add("J5-全挂闭合", j5, "十八卡全绑");

    // J6 缺挂显性。
    let j6 = {
        let mut l = HookLedger::new();
        let _ = l.bind(12101, 1);
        l.missing_cards().len() == 17 && l.summary().contains("pairs=1/18")
    };
    s.add("J6-缺挂显性", j6, "缺卡清单可回读");

    // J7 绑定回读。
    let j7 = {
        let mut l = HookLedger::new();
        let _ = l.bind(12108, 4);
        l.hook_of(12108) == Some(4) && l.hook_of(12109).is_none()
    };
    s.add("J7-绑定回读", j7, "按卡查挂点");

    // J8 行渲染。
    let j8 = {
        let mut l = HookLedger::new();
        let _ = l.bind(12110, 3);
        l.line_of(12110).contains("h3") && l.line_of(12111) == "missing"
    };
    s.add("J8-行渲染", j8, "绑定行可渲染");

    // J9 印账冻结。
    let j9 = {
        let mut l = HookLedger::new();
        let _ = l.bind(12101, 1);
        l.seal();
        l.bind(12102, 1).err() == Some(H6_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut l = HookLedger::new();
        let _ = l.bind(12101, 1);
        l.fold() == l.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账：字符串族三卡绑堆挂点。
    let mut l = HookLedger::new();
    let _ = l.bind(12101, 1);
    let _ = l.bind(12105, 2);
    let _ = l.bind(12114, 3);

    s.add("摘要回读", l.summary().contains("pairs=3/18"), "三绑在账");
    s.add("缺挂清单", l.missing_cards().len() == 15, "余十五卡未绑");
    s.add("渲染行", l.line_of(12114).contains("h3"), "12114 绑挂点 3");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12120_hook_flow() {
        let mut l = HookLedger::new();
        assert!(l.bind(12101, 1).is_ok());
        assert_eq!(l.bind(12101, 2), Err(H6_DUP_CARD));
        assert_eq!(l.bind(12102, 9), Err(H6_BAD_HOOK));
        assert!(!l.closed());
    }

    #[test]
    fn unx12120_checks_green() {
        let s = run_unx12120_checks();
        assert!(s.all_passed(), "UNX-F12120 判据未全绿");
    }

    #[test]
    fn pair_validation() {
        let ok = HookPair { card: 12118, hook: 8 };
        assert!(ok.validate().is_ok());
        assert!(ok.render().contains("12118"));
    }
}
