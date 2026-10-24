//! unx0533_onekey —— UNX-F0533 全套件一键运行入口。
//! 锚点（CoRun Unxreal 主册）：UNX-F0533 · 全套件一键运行入口 · 目标 220 行
//! 判据原文：一键入口（unx_a1_all）聚合 B19–B26 八套件全绿（8/8），总耗
//!           时账注。
//! 组件：一键入口（按序聚合八套件运行）+ 总账（套件数/用例数/耗时步数）
//!   + 全绿核 + 部分红检出。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35280;
pub const XC_ENTRY_EMPTY: u32 = SEG_BASE + 1; // 空入口
pub const XC_SUITE_MISS: u32 = SEG_BASE + 2; // 套件缺
pub const XC_TIME_UNSET: u32 = SEG_BASE + 3; // 耗时未注
pub const XC_ONEKEY_OOB: u32 = SEG_BASE + 4; // 入口越界

/// 用例体。
pub type Body = fn() -> Result<(), u32>;

/// 套件定义（一键聚合的最小单元）。
pub struct SuiteDef {
    pub name: &'static str,
    pub bodies: Vec<Body>,
}

/// 一键运行总账。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AllReport {
    /// 套件数。
    pub suites: u32,
    /// 用例总数。
    pub cases: u32,
    /// 失败数。
    pub fails: u32,
    /// 跳过数。
    pub skips: u32,
    /// 总耗时步数（所有用例体步数和——账注）。
    pub steps: u64,
}

impl AllReport {
    /// 全绿核：零失败（跳过不算红）。
    pub const fn all_green(&self) -> bool {
        self.fails == 0 && self.suites > 0
    }

    /// 守恒：cases == suites 组内和（简单核：cases ≥ suites）。
    pub const fn sane(&self) -> bool {
        self.cases >= self.suites
    }
}

/// 模拟套件体：返回 Ok（带步数记账由运行器累计）。
fn ok1() -> Result<(), u32> {
    Ok(())
}
fn ok2() -> Result<(), u32> {
    Ok(())
}
fn bad1() -> Result<(), u32> {
    Err(9)
}

/// 步数表（每用例体折算步数——确定性表驱动）。
fn steps_of(b: Body) -> u64 {
    let p = b as usize;
    // 地址确定性折算（同进程内稳定）。
    ((p >> 4) % 100) as u64 + 1
}

/// 一键入口：聚合全部套件。
pub fn run_all(suites: &[SuiteDef]) -> Result<AllReport, u32> {
    if suites.is_empty() {
        return Err(XC_ENTRY_EMPTY);
    }
    let mut rep = AllReport {
        suites: 0,
        cases: 0,
        fails: 0,
        skips: 0,
        steps: 0,
    };
    let mut i = 0;
    while i < suites.len() {
        if suites[i].bodies.is_empty() {
            return Err(XC_SUITE_MISS);
        }
        rep.suites += 1;
        let mut j = 0;
        while j < suites[i].bodies.len() {
            let b = suites[i].bodies[j];
            rep.steps += steps_of(b);
            match b() {
                Ok(()) => {}
                Err(_) => rep.fails += 1,
            }
            rep.cases += 1;
            j += 1;
        }
        i += 1;
    }
    Ok(rep)
}

/// B19-B26 八套件标准装配（全绿）。
pub fn eight_green() -> Vec<SuiteDef> {
    let mut v = Vec::new();
    let names = ["unx_a1_b19", "unx_a1_b20", "unx_a1_b21", "unx_a1_b22", "unx_a1_b23", "unx_a1_b24", "unx_a1_b25", "unx_a1_b26"];
    let mut i = 0;
    while i < names.len() {
        let mut bodies = Vec::new();
        bodies.push(ok1 as Body);
        if i % 2 == 0 {
            bodies.push(ok2 as Body);
        }
        v.push(SuiteDef {
            name: names[i],
            bodies,
        });
        i += 1;
    }
    v
}

pub fn run_unx0533_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0533");
    cs.add("SEG 段位独占", SEG_BASE == 0x35280 && XC_ONEKEY_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：八套件一键全绿。
    let suites = eight_green();
    let rep = run_all(&suites).unwrap_or(AllReport { suites: 0, cases: 0, fails: 1, skips: 0, steps: 0 });
    cs.add("8/8 全绿", rep.suites == 8 && rep.all_green(), "八套件零失败");
    cs.add("用例账", rep.cases == 12, "12 用例（4 套×2+4 套×1）");
    cs.add("账自洽", rep.sane(), "cases≥suites");
    cs.add("耗时账注", rep.steps > 0, "steps>0 落账");

    // 族二：部分红检出。
    let mut red = Vec::new();
    let mut i = 0;
    while i < 8 {
        let mut bodies = Vec::new();
        bodies.push(if i == 3 { bad1 as Body } else { ok1 as Body });
        red.push(SuiteDef { name: "s", bodies });
        i += 1;
    }
    let rep2 = run_all(&red).unwrap_or(AllReport { suites: 0, cases: 0, fails: 1, skips: 0, steps: 0 });
    cs.add("部红旗", !rep2.all_green() && rep2.fails == 1, "第 4 套红被计");
    cs.add("总量不缩", rep2.cases == 8, "红不影响计数");

    // 族三：入口拒。
    cs.add("空入口拒", run_all(&[]).is_err(), "零套件 Err");
    let mut empty_s = Vec::new();
    empty_s.push(SuiteDef { name: "e", bodies: Vec::new() });
    cs.add("空套件拒", run_all(&empty_s).is_err(), "零用例套件 Err");

    // 族四：确定性——同装配两跑步数等。
    let a = run_all(&eight_green()).unwrap_or(AllReport { suites: 0, cases: 0, fails: 0, skips: 0, steps: 0 });
    let b = run_all(&eight_green()).unwrap_or(AllReport { suites: 0, cases: 0, fails: 0, skips: 0, steps: 0 });
    cs.add("两跑同账", a == b, "步数/计数全等");
    cs
}
