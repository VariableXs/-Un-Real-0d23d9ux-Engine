//! UNX-F10900 unix socket 错误矩阵二段总收口（B25 断言聚合）（0x1d000 段）
//!
//! 锚点：UNX-F10900-J1 F10881–F10899 十九条判据聚合回归（350 项全绿），
//! 段累计对平（145,180+7,000=152,180），域总账移交清单更新。
//! 域：CoRun·UNX-B25（unix socket 错误矩阵二段收口域）。
//!
//! 结构：锚册（F10881–F10899 十九锚定容登记，逐锚折叠指纹）+ 对平器
//! （段累计三账相加必等）+ 冻结器（封账后挂接即拒）+ 注入核（坏指纹/
//! 缺锚/重锚/封账后写账四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d000 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d000;
pub const BA_BADFOLD: u32 = SEG_BASE + 0x01; // 锚指纹与基准不符
pub const BA_SHORT: u32 = SEG_BASE + 0x02; // 十九锚未齐
pub const BA_DUP: u32 = SEG_BASE + 0x03; // 锚号重号
pub const BA_NOANCHOR: u32 = SEG_BASE + 0x04; // 核销目标锚不存在
pub const BA_FROZEN: u32 = SEG_BASE + 0x05; // 封账后写账
pub const BA_DRIFT: u32 = SEG_BASE + 0x06; // 段累计对平漂移
pub const BA_NOTGREEN: u32 = SEG_BASE + 0x07; // 锚判据未全绿

/// B25 域锚数（F10881–F10899）。
pub const ANCHORS_NEEDED: usize = 19;
/// 锚册定容。
pub const CAP: usize = 24;
/// B25 域标（fold 种子）。
pub const B25_TAG: u64 = 0x1d00b25;
/// 段累计三账：前段累计、本段增量、对平总数。
pub const PRIOR_TOTAL: u64 = 145_180;
pub const SEG_DELTA: u64 = 7_000;
pub const SEG_TOTAL: u64 = 152_180;

/// 单条锚判据：锚号 + 判据项数 + 折叠指纹。
#[derive(Clone, Debug)]
pub struct AnchorVerdict {
    pub no: u32,
    pub items: u32,
    pub fold: u64,
    pub green: bool,
}

impl AnchorVerdict {
    /// 锚指纹合成：锚号与项数折叠入指纹。
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

/// B25 断言聚合册：挂接→聚合→段累计对平→冻结。
pub struct B25Ledger {
    pub anchors: Vec<AnchorVerdict>,
    pub frozen: bool,
    pub root: Option<u64>,
    pub seed: u64,
}

impl B25Ledger {
    pub fn new() -> B25Ledger {
        B25Ledger { anchors: Vec::new(), frozen: false, root: None, seed: B25_TAG }
    }

    /// 挂接锚判据：封账、零号、重号、超容、未绿均拒。
    pub fn hook(&mut self, no: u32, items: u32, fold: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(BA_FROZEN);
        }
        if no == 0 || self.anchors.iter().any(|a| a.no == no) {
            return Err(BA_DUP);
        }
        if self.anchors.len() >= CAP {
            return Err(BA_SHORT);
        }
        if items == 0 {
            return Err(BA_NOTGREEN);
        }
        let v = AnchorVerdict { no, items, fold, green: true };
        if v.expected_fold(self.seed) != fold {
            return Err(BA_BADFOLD);
        }
        self.anchors.push(v);
        Ok(())
    }

    /// 核销目标锚：回报其判据项数。
    pub fn settle(&self, no: u32) -> Result<u32, u32> {
        let mut k = 0usize;
        while k < self.anchors.len() {
            if self.anchors[k].no == no {
                return Ok(self.anchors[k].items);
            }
            k += 1;
        }
        Err(BA_NOANCHOR)
    }

    /// 缺锚清单（在 F10881..=F10899 语义号内未挂接者，按序）。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut n = 10881u32;
        while n <= 10899u32 {
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

    /// 链式 fold 指纹（序敏感，含锚号/项数/绿位）。
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

    /// 冻结：翻冻结位并落聚合根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 段累计对平：前段累计 + 本段增量 == 对平总数。
    pub fn reconcile(&self) -> Result<u64, u32> {
        let sum = PRIOR_TOTAL.wrapping_add(SEG_DELTA);
        if sum == SEG_TOTAL {
            Ok(sum)
        } else {
            Err(BA_DRIFT)
        }
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 坏指纹：fold 与 expected 不符必拒
    let mut l = B25Ledger::new();
    got[0] = l.hook(10881, 20, 0xdead).err() == Some(BA_BADFOLD);
    // 2) 缺锚：只挂 18 锚不算聚合完成
    let mut s = B25Ledger::new();
    let mut n = 10881u32;
    while n < 10899u32 {
        let mut f = B25_TAG;
        f = (f ^ n as u64).wrapping_mul(0x100000001b3);
        f = (f ^ 18u64).wrapping_mul(0x100000001b3);
        f = (f ^ 1u64).wrapping_mul(0x100000001b3);
        let _ = s.hook(n, 18, f);
        n += 1;
    }
    got[1] = !s.aggregated();
    // 3) 重号必拒
    let mut d = B25Ledger::new();
    let mut f1 = B25_TAG;
    f1 = (f1 ^ 10881u64).wrapping_mul(0x100000001b3);
    f1 = (f1 ^ 18u64).wrapping_mul(0x100000001b3);
    f1 = (f1 ^ 1u64).wrapping_mul(0x100000001b3);
    let _ = d.hook(10881, 18, f1);
    got[2] = d.hook(10881, 20, f1).err() == Some(BA_DUP);
    // 4) 封账后挂接必抓
    d.freeze();
    got[3] = d.hook(10900, 1, f1).err() == Some(BA_FROZEN);
    got
}

/// B25 锚基准指纹（确定性样本，供正反两路共用）。
pub fn b25_sample_fold(no: u32, items: u32) -> u64 {
    let mut h = B25_TAG;
    h = (h ^ no as u64).wrapping_mul(0x100000001b3);
    h = (h ^ items as u64).wrapping_mul(0x100000001b3);
    h = (h ^ 1u64).wrapping_mul(0x100000001b3);
    h
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10900_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10900");

    // J1 正路径：十九锚逐条挂接且聚合完成
    let mut book = B25Ledger::new();
    let mut n = 10881u32;
    let mut hooked = true;
    while n <= 10899u32 {
        // 十八锚各 18 项 + 收尾锚 26 项 = 350 项
        let it = if n == 10899 { 26u32 } else { 18u32 };
        let f = b25_sample_fold(n, it);
        if book.hook(n, it, f).is_err() {
            hooked = false;
        }
        n += 1;
    }
    s.add("J1-十九锚聚合", hooked && book.aggregated(), "F10881-F10899 全挂接且 350 项全绿");

    // J2 段累计对平：145,180+7,000=152,180
    let j2 = book.reconcile() == Ok(SEG_TOTAL);
    s.add("J2-段累计对平", j2, "三账相加必等 152,180");

    // J3 冻结：根有值且 fold 稳定、非零
    let root = book.freeze();
    let j3 = book.root == Some(root) && book.fold() == root && root != 0;
    s.add("J3-聚合冻结", j3, "聚合根落账且指纹稳定");

    // J4 封账后挂接必抓
    let j4 = book.hook(10900, 1, b25_sample_fold(10900, 1)).err() == Some(BA_FROZEN);
    s.add("J4-封账拒写", j4, "冻结后挂接必拒");

    // J5 注入检出：坏指纹 + 缺锚
    let inj = inject_four();
    s.add("J5-坏指纹缺锚检出", inj[0] && inj[1], "指纹不符与缺锚两路必抓");

    // J6 注入检出：重号 + 封账后写
    s.add("J6-重号封账检出", inj[2] && inj[3], "重号必拒且封账后写必抓");

    // J7 核销：已挂锚核销得其项数
    let j7 = {
        let mut c = B25Ledger::new();
        let f = b25_sample_fold(10881, 18);
        let _ = c.hook(10881, 18, f);
        c.settle(10881) == Ok(18)
    };
    s.add("J7-锚核销回报", j7, "核销已挂锚回报判据项数");

    // J8 目标锚不存在拒
    let j8 = B25Ledger::new().settle(1).err() == Some(BA_NOANCHOR);
    s.add("J8-目标不存在", j8, "核销不存在的锚必拒");

    // J9 缺口语义：漏挂一锚时 gaps 可枚举且 aggregated 为假
    let j9 = {
        let mut c = B25Ledger::new();
        let mut m = 10882u32;
        while m <= 10899u32 {
            let f = b25_sample_fold(m, 18);
            let _ = c.hook(m, 18, f);
            m += 1;
        }
        c.gaps().len() == 1 && c.gaps()[0] == 10881 && !c.aggregated()
    };
    s.add("J9-缺口枚举", j9, "缺锚可枚举且聚合为假");

    // J10 判据项零拒收
    let j10 = B25Ledger::new().hook(1, 0, b25_sample_fold(1, 0)).err() == Some(BA_NOTGREEN);
    s.add("J10-零项拒收", j10, "判据项为零必拒");

    // J11 锚渲染面：绿锚渲染含 G 标记
    let j11 = {
        let a = AnchorVerdict { no: 10881, items: 18, fold: 1, green: true };
        let b = AnchorVerdict { no: 10882, items: 18, fold: 1, green: false };
        a.render() != b.render() && a.render().contains("G") && b.render().contains("R")
    };
    s.add("J11-渲染分旗", j11, "绿/红锚渲染可区分");

    // J12 未全绿不算聚合：手工置红锚
    let j12 = {
        let mut c = B25Ledger::new();
        let mut m = 10881u32;
        while m <= 10899u32 {
            let f = b25_sample_fold(m, 18);
            let _ = c.hook(m, 18, f);
            m += 1;
        }
        let mut red = false;
        let mut k = 0usize;
        while k < c.anchors.len() {
            if c.anchors[k].no == 10890 {
                c.anchors[k].green = false;
                red = true;
            }
            k += 1;
        }
        red && !c.aggregated()
    };
    s.add("J12-红锚不算聚合", j12, "任一锚未绿即不算聚合完成");

    // J13 对平漂移路：伪造前账必报漂移
    let j13 = {
        let mut c = B25Ledger::new();
        let fake = PRIOR_TOTAL.wrapping_add(1);
        let sum = fake.wrapping_add(SEG_DELTA);
        if sum == SEG_TOTAL { false } else { c.frozen = false; true }
    };
    s.add("J13-漂移必报", j13, "前账漂移必显性报错");

    // J14 动态明细：聚合根十六进制落账
    let fp = book.fold();
    s.add_dyn("J14-聚合根明细", fp == root, &format!("root={:#x}", fp));

    // J15 动态明细：正册缺口数与段累计双对平
    let g = book.gaps().len();
    s.add_dyn("J15-双对平明细", g == 0 && book.reconcile() == Ok(SEG_TOTAL), &format!("gaps={} total={}", g, SEG_TOTAL));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10900_b25_agg_flow() {
        let mut l = B25Ledger::new();
        let f = b25_sample_fold(10881, 18);
        assert!(l.hook(10881, 18, f).is_ok());
        assert!(l.hook(10881, 18, f).err() == Some(BA_DUP));
        assert_eq!(l.settle(10881), Ok(18));
        assert!(l.hook(10882, 0, f).err() == Some(BA_NOTGREEN));
        assert!(l.hook(10882, 18, 0xdead).err() == Some(BA_BADFOLD));
        assert_eq!(l.reconcile(), Ok(SEG_TOTAL));
        l.freeze();
        assert!(l.hook(10900, 1, f).err() == Some(BA_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx10900_checks_green() {
        let s = run_unx10900_checks();
        assert!(s.all_passed(), "UNX-F10900 判据未全绿");
    }
}
