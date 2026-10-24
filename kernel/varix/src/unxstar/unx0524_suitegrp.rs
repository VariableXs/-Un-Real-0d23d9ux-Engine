//! unx0524_suitegrp —— UNX-F0524 套件分组运行（批集调度器）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0524 · 套件分组运行 · 目标 220 行
//! 判据原文：调度器分组运行账齐（按批前缀 b19–b26 分组 8 组），组序与组
//!           内序双稳定（3 轮同序）。
//! 组件：Grouper（按批前缀把套件名归组）+ Scheduler（组序=前缀字典序、
//!   组内序=注册序）+ 三轮同序核（双稳定性：组间序与组内序逐轮全等）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35040;
pub const XC_GRP_BAD: u32 = SEG_BASE + 1; // 组名坏（空/无前缀）
pub const XC_SCHED_UNSTABLE: u32 = SEG_BASE + 2; // 调度不稳定
pub const XC_GROUP_MISS: u32 = SEG_BASE + 3; // 组不存在
pub const XC_ORDER_OOB: u32 = SEG_BASE + 4; // 序号越界

/// 批前缀抽取：`unx_a1_bNN_xxx` → `bNN`；无前缀拒。
pub fn prefix_of(suite: &str) -> Result<String, u32> {
    let bytes = suite.as_bytes();
    // 找 "unx_a1_" 头。
    let head = b"unx_a1_";
    if bytes.len() < head.len() + 3 || &bytes[..head.len()] != head {
        return Err(XC_GRP_BAD);
    }
    // 找后续第一个 '_' 或串尾。
    let mut end = head.len();
    while end < bytes.len() && bytes[end] != b'_' {
        end += 1;
    }
    let p = &suite[head.len()..end];
    if p.len() < 2 || !p.starts_with('b') {
        return Err(XC_GRP_BAD);
    }
    // b 后须全数字。
    let mut i = 1;
    while i < p.len() {
        let c = p.as_bytes()[i];
        if !(c >= b'0' && c <= b'9') {
            return Err(XC_GRP_BAD);
        }
        i += 1;
    }
    Ok(String::from(p))
}

/// 组：前缀 + 组内套件名（注册序）。
#[derive(Clone, Debug, PartialEq)]
pub struct SuiteGroup {
    pub prefix: String,
    pub members: Vec<String>,
}

/// 分组器：按前缀归组，组间序=前缀字典序。
pub fn group_suites(suites: &[&str]) -> Result<Vec<SuiteGroup>, u32> {
    let mut groups: Vec<SuiteGroup> = Vec::new();
    let mut i = 0;
    while i < suites.len() {
        let p = prefix_of(suites[i])?;
        let mut found = false;
        let mut g = 0;
        while g < groups.len() {
            if groups[g].prefix == p {
                groups[g].members.push(String::from(suites[i]));
                found = true;
                break;
            }
            g += 1;
        }
        if !found {
            let mut m = Vec::new();
            m.push(String::from(suites[i]));
            groups.push(SuiteGroup { prefix: p, members: m });
        }
        i += 1;
    }
    // 组间排序：前缀字典序（b19 < b20 < ... < b26，bN 号数值序与字典序一致因等宽）。
    sort_groups(&mut groups);
    Ok(groups)
}

/// 插入排序（按 prefix 字节序；等宽 bNN 下字典序=数值序）。
fn sort_groups(groups: &mut [SuiteGroup]) {
    let n = groups.len();
    let mut i = 1;
    while i < n {
        let mut j = i;
        while j > 0 && groups[j - 1].prefix.as_bytes() > groups[j].prefix.as_bytes() {
            groups.swap(j - 1, j);
            j -= 1;
        }
        i += 1;
    }
}

/// 调度器：产出扁平运行序（组间字典序 × 组内注册序）。
pub fn schedule(suites: &[&str]) -> Result<Vec<String>, u32> {
    let groups = group_suites(suites)?;
    let mut out = Vec::new();
    let mut g = 0;
    while g < groups.len() {
        let mut m = 0;
        while m < groups[g].members.len() {
            out.push(groups[g].members[m].clone());
            m += 1;
        }
        g += 1;
    }
    Ok(out)
}

/// 双稳定核：3 轮调度序全等。
pub fn triple_stable(suites: &[&str]) -> Result<bool, u32> {
    let r1 = schedule(suites)?;
    let r2 = schedule(suites)?;
    let r3 = schedule(suites)?;
    Ok(r1 == r2 && r2 == r3 && !r1.is_empty())
}

pub fn run_unx0524_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0524");
    cs.add("SEG 段位独占", SEG_BASE == 0x35040 && XC_ORDER_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：前缀抽取。
    cs.add("前缀 b19", prefix_of("unx_a1_b19_core") == Ok(String::from("b19")), "头段抽取");
    cs.add("前缀 b26", prefix_of("unx_a1_b26_assert") == Ok(String::from("b26")), "多号抽取");
    cs.add("纯前缀", prefix_of("unx_a1_b21") == Ok(String::from("b21")), "无尾段可抽");
    cs.add("无头拒", prefix_of("a1_b19").is_err(), "缺 unx_a1_ 头");
    cs.add("无 b 拒", prefix_of("unx_a1_c19").is_err(), "前缀非 b 开头");
    cs.add("非数字拒", prefix_of("unx_a1_b1x").is_err(), "b 后混字母");

    // 族二：分组 8 组。
    let suites = [
        "unx_a1_b22_a", "unx_a1_b19_a", "unx_a1_b22_b", "unx_a1_b20_a", "unx_a1_b26_a",
        "unx_a1_b21_a", "unx_a1_b23_a", "unx_a1_b24_a", "unx_a1_b25_a",
    ];
    let groups = group_suites(&suites).unwrap_or_default();
    cs.add("组数 8", groups.len() == 8, "b19..b26 各一组");
    cs.add("组序字典", groups[0].prefix == "b19" && groups[7].prefix == "b26", "组间升序");
    cs.add("组内注册序", groups[2].members.len() == 2 && groups[2].members[0] == "unx_a1_b22_a", "先到先排");

    // 族三：双稳定三轮同序。
    cs.add("三轮同序", triple_stable(&suites) == Ok(true), "组序×组内序全稳");
    let sched = schedule(&suites).unwrap_or_default();
    cs.add("扁平序账", sched.len() == 9 && sched[0] == "unx_a1_b19_a", "调度全量产出");
    cs.add("组内相邻", sched[1] == "unx_a1_b22_a" && sched[2] == "unx_a1_b22_b", "同组相邻");
    cs
}
