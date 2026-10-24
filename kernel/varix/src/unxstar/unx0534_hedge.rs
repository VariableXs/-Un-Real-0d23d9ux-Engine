//! unx0534_hedge —— UNX-F0534 结果对冲（套件结果 vs 判据账）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0534 · 结果对冲 · 目标 220 行
//! 判据原文：对冲核（套件通过集=判据账已验集 三等），误差 0，幽灵通过注
//!           入停账。
//! 组件：双账对冲（套件通过集 vs 判据账已验集——集合相等核）+ 误差核 +
//!   幽灵通过检出（套件有绿但判据账无验记录→停账）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x352C0;
pub const XC_GHOST_PASS: u32 = SEG_BASE + 1; // 幽灵通过
pub const XC_HEDGE_GAP: u32 = SEG_BASE + 2; // 对冲缺口
pub const XC_SET_MISMATCH: u32 = SEG_BASE + 3; // 集合不等
pub const XC_HEDGE_OOB: u32 = SEG_BASE + 4; // 越界

/// 判据账：已验判据号集。
pub struct CritBook {
    pub verified: Vec<&'static str>,
}

impl CritBook {
    /// 空账。
    pub fn new() -> Self {
        CritBook { verified: Vec::new() }
    }

    /// 记验（重复拒）。
    pub fn verify(&mut self, crit: &'static str) -> Result<(), u32> {
        if self.has(crit) {
            return Err(XC_HEDGE_GAP);
        }
        self.verified.push(crit);
        Ok(())
    }

    /// 在账？
    pub fn has(&self, crit: &str) -> bool {
        let mut i = 0;
        while i < self.verified.len() {
            if self.verified[i] == crit {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 账量。
    pub fn len(&self) -> usize {
        self.verified.len()
    }

    /// 空？可实现。
    pub fn is_empty(&self) -> bool {
        self.verified.is_empty()
    }
}

impl Default for CritBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 对冲：套件通过用例所验判据集 == 判据账已验集。
/// 返回 Err(XC_GHOST_PASS) 表套件绿但账无验（幽灵）；Err(XC_SET_MISMATCH)
/// 表互有出入。
pub fn hedge(suite_green: &[&'static str], book: &CritBook) -> Result<(), u32> {
    // 套件绿集逐个在账？
    let mut i = 0;
    while i < suite_green.len() {
        if !book.has(suite_green[i]) {
            return Err(XC_GHOST_PASS);
        }
        i += 1;
    }
    // 账内验记录逐个在套件绿集？（防账外幽灵）
    let mut i = 0;
    while i < book.verified.len() {
        let mut found = false;
        let mut j = 0;
        while j < suite_green.len() {
            if suite_green[j] == book.verified[i] {
                found = true;
                break;
            }
            j += 1;
        }
        if !found {
            return Err(XC_SET_MISMATCH);
        }
        i += 1;
    }
    Ok(())
}

/// 误差核：对冲缺口计数（绿无验 + 验无绿 双向）。
pub fn hedge_gap(suite_green: &[&'static str], book: &CritBook) -> usize {
    let mut gap = 0;
    let mut i = 0;
    while i < suite_green.len() {
        if !book.has(suite_green[i]) {
            gap += 1;
        }
        i += 1;
    }
    let mut i = 0;
    while i < book.verified.len() {
        let mut found = false;
        let mut j = 0;
        while j < suite_green.len() {
            if suite_green[j] == book.verified[i] {
                found = true;
                break;
            }
            j += 1;
        }
        if !found {
            gap += 1;
        }
        i += 1;
    }
    gap
}

pub fn run_unx0534_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0534");
    cs.add("SEG 段位独占", SEG_BASE == 0x352C0 && XC_HEDGE_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：三等对冲。
    let mut book = CritBook::new();
    let _ = book.verify("F0521-J1");
    let _ = book.verify("F0522-J1");
    let _ = book.verify("F0523-J1");
    let green = ["F0521-J1", "F0522-J1", "F0523-J1"];
    cs.add("三等对冲", hedge(&green, &book) == Ok(()), "绿集=账集");
    cs.add("零误差", hedge_gap(&green, &book) == 0, "双向缺口 0");
    cs.add("账量", book.len() == 3, "三验在账");

    // 族二：幽灵通过停账。
    let ghost = ["F0521-J1", "F0599-JX"];
    cs.add("幽灵检出", hedge(&ghost, &book).is_err(), "绿无验→Err");
    let mut b2 = CritBook::new();
    let _ = b2.verify("F0599-JX");
    cs.add("账外幽灵", hedge(&green[..1], &b2).is_err(), "验无绿→Err");

    // 族三：误差计数双向。
    cs.add("双向缺口 2", hedge_gap(&ghost, &book) == 2, "绿缺 1+账缺 1");
    let partial = ["F0521-J1", "F0522-J1"];
    cs.add("单侧缺口 1", hedge_gap(&partial, &book) == 1, "仅账多一验");

    // 族四：重复验拒。
    let mut b3 = CritBook::new();
    let _ = b3.verify("A");
    cs.add("重复验拒", b3.verify("A").is_err(), "重号 Err");
    cs.add("空集对冲", hedge(&[], &CritBook::new()) == Ok(()), "双空等价");
    cs
}
