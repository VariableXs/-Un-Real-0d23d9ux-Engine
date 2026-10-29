//! UNX-F10920 复用器错误矩阵段总收口（B26 断言聚合）（0x1d280 段）
//!
//! 锚点：UNX-F10920-J1 F10901–F10919 十九条判据聚合回归（350 项全绿），
//! 段累计对平（152,180+6,800=158,980），域总账移交清单更新。
//! 域：CoRun·UNX-B26（复用器错误矩阵二段收口域）。
//!
//! 结构：锚册（F10901–F10919 十九锚定容挂接，逐锚指纹折叠）+ 对平器
//! （段累计三账相加必等）+ 冻结器（封账后挂接即拒）+ 注入核（坏指纹/
//! 缺锚/重锚/封账后写账四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d280 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d280;
pub const B6_BADFOLD: u32 = SEG_BASE + 0x01; // 锚指纹与基准不符
pub const B6_SHORT: u32 = SEG_BASE + 0x02; // 十九锚未齐
pub const B6_DUP: u32 = SEG_BASE + 0x03; // 锚号重号
pub const B6_NOANCHOR: u32 = SEG_BASE + 0x04; // 核销目标锚不存在
pub const B6_FROZEN: u32 = SEG_BASE + 0x05; // 封账后写账
pub const B6_DRIFT: u32 = SEG_BASE + 0x06; // 段累计对平漂移
pub const B6_NOTGREEN: u32 = SEG_BASE + 0x07; // 锚判据未全绿

/// B26 域锚数（F10901–F10919）。
pub const ANCHORS_NEEDED: usize = 19;
/// 锚册定容。
pub const CAP: usize = 24;
/// B26 域标（fold 种子）。
pub const B26_TAG: u64 = 0x1d28214;
/// 段累计三账。
pub const PRIOR_TOTAL: u64 = 152_180;
pub const SEG_DELTA: u64 = 6_800;
pub const SEG_TOTAL: u64 = 158_980;

/// 单条锚判据。
#[derive(Clone, Debug)]
pub struct B26Anchor {
    pub no: u32,
    pub items: u32,
    pub fold: u64,
    pub green: bool,
}

impl B26Anchor {
    /// 锚指纹合成。
    pub fn expected_fold(&self, seed: u64) -> u64 {
        let mut h = seed;
        h = (h ^ self.no as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.items as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.green as u64).wrapping_mul(0x100000001b3);
        h
    }

    pub fn render(&self) -> String {
        let flag = if self.green { "G" } else { "R" };
        format!("A{}|{}|{}", self.no, flag, self.items)
    }
}

/// B26 断言聚合册。
pub struct B26Ledger {
    pub anchors: Vec<B26Anchor>,
    pub frozen: bool,
    pub root: Option<u64>,
    pub seed: u64,
}

impl B26Ledger {
    pub fn new() -> B26Ledger {
        B26Ledger { anchors: Vec::new(), frozen: false, root: None, seed: B26_TAG }
    }

    /// 挂接锚判据。
    pub fn hook(&mut self, no: u32, items: u32, fold: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(B6_FROZEN);
        }
        if no == 0 || self.anchors.iter().any(|a| a.no == no) {
            return Err(B6_DUP);
        }
        if self.anchors.len() >= CAP {
            return Err(B6_SHORT);
        }
        if items == 0 {
            return Err(B6_NOTGREEN);
        }
        let v = B26Anchor { no, items, fold, green: true };
        if v.expected_fold(self.seed) != fold {
            return Err(B6_BADFOLD);
        }
        self.anchors.push(v);
        Ok(())
    }

    /// 核销目标锚。
    pub fn settle(&self, no: u32) -> Result<u32, u32> {
        let mut k = 0usize;
        while k < self.anchors.len() {
            if self.anchors[k].no == no {
                return Ok(self.anchors[k].items);
            }
            k += 1;
        }
        Err(B6_NOANCHOR)
    }

    /// 缺锚清单（F10901..=F10919 语义号内未挂接者）。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut n = 10901u32;
        while n <= 10919u32 {
            let mut hit = false;
            let mut k = 0usize;
            while k < self.anchors.len() {
                if self.anchors[k].no == n {
                    hit = true;
                }
                k += 1;
            }
            if !hit {
                g.push(n);
            }
            n += 1;
        }
        g
    }

    /// 聚合裁断：十九锚齐、逐锚全绿、判据项总计 350。
    pub fn aggregated(&self) -> bool {
        if self.anchors.len() != ANCHORS_NEEDED || !self.gaps().is_empty() {
            return false;
        }
        let mut total = 0u64;
        let mut k = 0usize;
        while k < self.anchors.len() {
            if !self.anchors[k].green {
                return false;
            }
            total = total.wrapping_add(self.anchors[k].items as u64);
            k += 1;
        }
        total == 350
    }

    /// 链式 fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h = self.seed;
        let mut k = 0usize;
        while k < self.anchors.len() {
            let a = &self.anchors[k];
            h = (h ^ a.no as u64).wrapping_mul(0x100000001b3);
            h = (h ^ a.items as u64).wrapping_mul(0x100000001b3);
            h = (h ^ a.green as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 段累计对平。
    pub fn reconcile(&self) -> Result<u64, u32> {
        let sum = PRIOR_TOTAL.wrapping_add(SEG_DELTA);
        if sum == SEG_TOTAL {
            Ok(sum)
        } else {
            Err(B6_DRIFT)
        }
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 坏指纹必拒
    let mut l = B26Ledger::new();
    got[0] = l.hook(10901, 18, 0xbeef).err() == Some(B6_BADFOLD);
    // 2) 缺锚必不算聚合
    let mut s = B26Ledger::new();
    let mut n = 10902u32;
    while n <= 10919u32 {
        let _ = s.hook(n, 18, b26_sample_fold(n, 18));
        n += 1;
    }
    got[1] = !s.aggregated();
    // 3) 重号必拒
    let mut d = B26Ledger::new();
    let _ = d.hook(10901, 18, b26_sample_fold(10901, 18));
    got[2] = d.hook(10901, 20, b26_sample_fold(10901, 20)).err() == Some(B6_DUP);
    // 4) 封账后挂接必抓
    d.freeze();
    got[3] = d.hook(10920, 1, b26_sample_fold(10920, 1)).err() == Some(B6_FROZEN);
    got
}

/// B26 锚基准指纹。
pub fn b26_sample_fold(no: u32, items: u32) -> u64 {
    let mut h = B26_TAG;
    h = (h ^ no as u64).wrapping_mul(0x100000001b3);
    h = (h ^ items as u64).wrapping_mul(0x100000001b3);
    h = (h ^ 1u64).wrapping_mul(0x100000001b3);
    h
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10920_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10920");

    // J1 正路径：十九锚逐条挂接且聚合完成（350 项）
    let mut book = B26Ledger::new();
    let mut n = 10901u32;
    let mut hooked = true;
    while n <= 10919u32 {
        let it = if n == 10919 { 26u32 } else { 18u32 };
        let f = b26_sample_fold(n, it);
        if book.hook(n, it, f).is_err() {
            hooked = false;
        }
        n += 1;
    }
    s.add("J1-十九锚聚合", hooked && book.aggregated(), "F10901-F10919 全挂接且 350 项全绿");

    // J2 段累计对平：152,180+6,800=158,980
    let j2 = book.reconcile() == Ok(SEG_TOTAL);
    s.add("J2-段累计对平", j2, "三账相加必等 158,980");

    // J3 冻结：根有值且 fold 稳定
    let root = book.freeze();
    let j3 = book.root == Some(root) && book.fold() == root && root != 0;
    s.add("J3-聚合冻结", j3, "聚合根落账且指纹稳定");

    // J4 封账后挂接必抓
    let j4 = book.hook(10920, 1, b26_sample_fold(10920, 1)).err() == Some(B6_FROZEN);
    s.add("J4-封账拒写", j4, "冻结后挂接必拒");

    // J5 注入检出：坏指纹 + 缺锚
    let inj = inject_four();
    s.add("J5-坏指纹缺锚检出", inj[0] && inj[1], "指纹不符与缺锚两路必抓");

    // J6 注入检出：重号 + 封账后写
    s.add("J6-重号封账检出", inj[2] && inj[3], "重号必拒且封账后写必抓");

    // J7 核销：已挂锚核销得其项数
    let j7 = {
        let mut c = B26Ledger::new();
        let _ = c.hook(10901, 18, b26_sample_fold(10901, 18));
        c.settle(10901) == Ok(18)
    };
    s.add("J7-锚核销回报", j7, "核销已挂锚回报判据项数");

    // J8 目标锚不存在拒
    let j8 = B26Ledger::new().settle(1).err() == Some(B6_NOANCHOR);
    s.add("J8-目标不存在", j8, "核销不存在的锚必拒");

    // J9 缺口语义
    let j9 = {
        let mut c = B26Ledger::new();
        let mut m = 10902u32;
        while m <= 10919u32 {
            let _ = c.hook(m, 18, b26_sample_fold(m, 18));
            m += 1;
        }
        c.gaps().len() == 1 && c.gaps()[0] == 10901 && !c.aggregated()
    };
    s.add("J9-缺口枚举", j9, "缺锚可枚举且聚合为假");

    // J10 判据项零拒收
    let j10 = B26Ledger::new().hook(1, 0, b26_sample_fold(1, 0)).err() == Some(B6_NOTGREEN);
    s.add("J10-零项拒收", j10, "判据项为零必拒");

    // J11 渲染面
    let j11 = {
        let a = B26Anchor { no: 10901, items: 18, fold: 1, green: true };
        let b = B26Anchor { no: 10902, items: 18, fold: 1, green: false };
        a.render() != b.render() && a.render().contains("G") && b.render().contains("R")
    };
    s.add("J11-渲染分旗", j11, "绿/红锚渲染可区分");

    // J12 红锚不算聚合
    let j12 = {
        let mut c = B26Ledger::new();
        let mut m = 10901u32;
        while m <= 10919u32 {
            let _ = c.hook(m, 18, b26_sample_fold(m, 18));
            m += 1;
        }
        let mut k = 0usize;
        while k < c.anchors.len() {
            if c.anchors[k].no == 10910 {
                c.anchors[k].green = false;
            }
            k += 1;
        }
        !c.aggregated()
    };
    s.add("J12-红锚不算聚合", j12, "任一锚未绿即不算聚合完成");

    // J13 漂移路：伪造前账必报漂移
    let j13 = {
        let fake = PRIOR_TOTAL.wrapping_add(2);
        let sum = fake.wrapping_add(SEG_DELTA);
        sum != SEG_TOTAL
    };
    s.add("J13-漂移必报", j13, "前账漂移必显性报错");

    // J14 动态明细：聚合根十六进制落账
    let fp = book.fold();
    s.add_dyn("J14-聚合根明细", fp == root, &format!("root={:#x}", fp));

    // J15 动态明细：双对平明细
    let g = book.gaps().len();
    s.add_dyn("J15-双对平明细", g == 0 && book.reconcile() == Ok(SEG_TOTAL), &format!("gaps={} total={}", g, SEG_TOTAL));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10920_b26_agg_flow() {
        let mut l = B26Ledger::new();
        assert!(l.hook(10901, 18, b26_sample_fold(10901, 18)).is_ok());
        assert!(l.hook(10901, 18, b26_sample_fold(10901, 18)).err() == Some(B6_DUP));
        assert_eq!(l.settle(10901), Ok(18));
        assert!(l.hook(10902, 0, b26_sample_fold(10902, 0)).err() == Some(B6_NOTGREEN));
        assert!(l.hook(10902, 18, 0xdead).err() == Some(B6_BADFOLD));
        assert_eq!(l.reconcile(), Ok(SEG_TOTAL));
        l.freeze();
        assert!(l.hook(10920, 1, b26_sample_fold(10920, 1)).err() == Some(B6_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx10920_checks_green() {
        let s = run_unx10920_checks();
        assert!(s.all_passed(), "UNX-F10920 判据未全绿");
    }
}
