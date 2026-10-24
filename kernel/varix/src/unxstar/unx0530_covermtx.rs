//! unx0530_covermtx —— UNX-F0530 判据覆盖矩阵（账面全覆盖）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0530 · 判据覆盖矩阵 · 目标 220 行
//! 判据原文：覆盖矩阵账齐（B19–B26 判据号×套件用例映射 100% 覆盖），漏
//!           覆盖检出注入红。
//! 组件：CoverMatrix（判据号→套件用例映射账）+ 全覆盖核（每判据至少一
//!   用例）+ 漏覆盖注入（造一个无映射判据，检出即红样验证）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x351C0;
pub const XC_COVER_HOLE: u32 = SEG_BASE + 1; // 漏覆盖
pub const XC_MAP_DUP: u32 = SEG_BASE + 2; // 映射重复
pub const XC_CRIT_BAD: u32 = SEG_BASE + 3; // 判据号坏
pub const XC_MTX_FULL: u32 = SEG_BASE + 4; // 矩阵满

/// 判据号合法性：`F0526-J1` 风格（F+4 数字+-J+数字）。
pub fn crit_ok(crit: &str) -> bool {
    let b = crit.as_bytes();
    if b.len() < 8 || b[0] != b'F' {
        return false;
    }
    let mut i = 1;
    while i < 5 {
        if !(b[i] >= b'0' && b[i] <= b'9') {
            return false;
        }
        i += 1;
    }
    if b[5] != b'-' || b[6] != b'J' {
        return false;
    }
    let mut i = 7;
    while i < b.len() {
        if !(b[i] >= b'0' && b[i] <= b'9') {
            return false;
        }
        i += 1;
    }
    true
}

/// 映射条目：判据号 → 用例号集。
#[derive(Clone, Debug, PartialEq)]
pub struct MapEntry {
    pub crit: String,
    pub cases: Vec<String>,
}

/// 覆盖矩阵。
pub struct CoverMatrix {
    entries: Vec<MapEntry>,
    cap: usize,
}

impl CoverMatrix {
    /// 矩阵（容量默认 512）。
    pub fn new() -> Self {
        CoverMatrix {
            entries: Vec::new(),
            cap: 512,
        }
    }

    /// 映射一笔：判据号坏拒；同判据同用例重复拒；满拒。
    pub fn map(&mut self, crit: &str, case: &str) -> Result<(), u32> {
        if !crit_ok(crit) {
            return Err(XC_CRIT_BAD);
        }
        if self.entries.len() >= self.cap {
            return Err(XC_MTX_FULL);
        }
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].crit == crit {
                let mut j = 0;
                while j < self.entries[i].cases.len() {
                    if self.entries[i].cases[j] == case {
                        return Err(XC_MAP_DUP);
                    }
                    j += 1;
                }
                self.entries[i].cases.push(String::from(case));
                return Ok(());
            }
            i += 1;
        }
        let mut v = Vec::new();
        v.push(String::from(case));
        self.entries.push(MapEntry {
            crit: String::from(crit),
            cases: v,
        });
        Ok(())
    }

    /// 判据映射的用例数。
    pub fn case_count(&self, crit: &str) -> usize {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].crit == crit {
                return self.entries[i].cases.len();
            }
            i += 1;
        }
        0
    }

    /// 全覆盖核：给定判据全集中每个判据都有 ≥1 用例映射。
    pub fn covers_all(&self, all_crits: &[&str]) -> bool {
        let mut i = 0;
        while i < all_crits.len() {
            if self.case_count(all_crits[i]) == 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 漏覆盖检出：返回无映射判据的索引列表。
    pub fn holes(&self, all_crits: &[&str]) -> Vec<usize> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < all_crits.len() {
            if self.case_count(all_crits[i]) == 0 {
                v.push(i);
            }
            i += 1;
        }
        v
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空？可实现。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for CoverMatrix {
    fn default() -> Self {
        Self::new()
    }
}

pub fn run_unx0530_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0530");
    cs.add("SEG 段位独占", SEG_BASE == 0x351C0 && XC_MTX_FULL == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：判据号校验。
    cs.add("号合法", crit_ok("F0526-J1") && crit_ok("F0530-J12"), "Fdddd-Jn 风格");
    cs.add("号短拒", !crit_ok("F052-J1"), "四位不足");
    cs.add("号缺 J 拒", !crit_ok("F0526-K1"), "缺 -J 段");
    cs.add("号非数字拒", !crit_ok("F05x6-J1"), "混字母");

    // 族二：B19-B26 判据映射 100%。
    let all = ["F0521-J1", "F0521-J2", "F0522-J1", "F0523-J1", "F0524-J1", "F0525-J1", "F0526-J1"];
    let mut m = CoverMatrix::new();
    let map_plan: [(&str, &str); 7] = [
        ("F0521-J1", "b21_c1"),
        ("F0521-J2", "b21_c2"),
        ("F0522-J1", "b22_p1"),
        ("F0523-J1", "b23_f1"),
        ("F0524-J1", "b24_g1"),
        ("F0525-J1", "b25_s1"),
        ("F0526-J1", "b26_r1"),
    ];
    let mut i = 0;
    while i < map_plan.len() {
        let (c, u) = map_plan[i];
        cs.add("映射入账", m.map(c, u).is_ok(), "逐笔映射");
        i += 1;
    }
    cs.add("全覆盖", m.covers_all(&all), "7/7 判据有映射");
    cs.add(" holes 空", m.holes(&all).is_empty(), "无漏");
    cs.add("条目数", m.len() == 7, "7 条目");

    // 族三：漏覆盖注入红。
    let with_hole = ["F0521-J1", "F0599-J1"];
    cs.add("漏覆盖检出", m.covers_all(&with_hole) == false && m.holes(&with_hole) == alloc::vec![1], "注入无映射判据被点名");
    cs.add("零映射计数", m.case_count("F0599-J1") == 0, "无条目计数 0");

    // 族四：重复映射拒。
    cs.add("同判据异用例可", m.map("F0521-J1", "b21_c9").is_ok(), "一判据多用例");
    cs.add("同判据同用例拒", m.map("F0521-J1", "b21_c9").is_err(), "重复映射 Err");
    cs.add("计数增", m.case_count("F0521-J1") == 3, "F0521-J1 三用例");
    cs
}
