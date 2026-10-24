//! VE-F4291 · 工具集成测试（U05 工具集成组）
//! 职责：九大工具面集成测试（九面×三环境全格实测）+ 各面与事件总线双端
//! 契约测试（复用 U02）+ 故障注入三景（总线断连/适配器崩溃/回写失败）
//! + 矩阵漏格补测排程 + 契约漂移版本戳拦截 + 隔离断言兜底。
//! 段位：0x2DA0（续表 idx525）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2DA0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItCode {
    /// 矩阵漏格。
    MatrixHole = SEG_BASE,
    /// 契约漂移（版本戳拦截）。
    ContractDrift = SEG_BASE + 0x01,
    /// 注入致崩溃未被隔离。
    NotIsolated = SEG_BASE + 0x02,
}

/// 三环境。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Env {
    Dev = 0,
    Staging = 1,
    Prod = 2,
}
pub const ENV_N: usize = 3;
pub const FACE9: usize = 9;

/// 故障三景。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault3 {
    BusDown = 0,
    AdapterCrash = 1,
    WritebackFail = 2,
}
pub const FAULT3_N: usize = 3;

/// 集成测试器。
pub struct IntegTest {
    /// 九×三矩阵格：已实测。
    pub grid: [[bool; ENV_N]; FACE9],
    /// 契约版本戳：(面, 戳)。
    pub contract_vers: [u32; FACE9],
    /// 总线侧戳。
    pub bus_ver: u32,
    /// 注入剧本结果：(景, 隔离生效)。
    pub drills: [(bool, bool); FAULT3_N],
    pub reruns: u32,
}

impl IntegTest {
    pub fn new(bus_ver: u32) -> Self {
        IntegTest { grid: [[false; ENV_N]; FACE9], contract_vers: [bus_ver; FACE9], bus_ver, drills: [(false, false); FAULT3_N], reruns: 0 }
    }

    /// 格实测登记。
    pub fn test_cell(&mut self, face: usize, env: Env, pass: bool) -> Result<(), ItCode> {
        if face >= FACE9 || !pass {
            if face < FACE9 {
                self.reruns += 1;
            }
            return Err(ItCode::MatrixHole);
        }
        self.grid[face][env as usize] = true;
        Ok(())
    }

    /// 矩阵漏格清单。
    pub fn holes(&self) -> Vec<(usize, u8)> {
        let mut out = Vec::new();
        for f in 0..FACE9 {
            for e in 0..ENV_N {
                if !self.grid[f][e] {
                    out.push((f, e as u8));
                }
            }
        }
        out
    }

    /// 双端契约对拍：面戳 vs 总线戳。
    pub fn contract_check(&self, face: usize) -> Result<(), ItCode> {
        if face >= FACE9 {
            return Err(ItCode::ContractDrift);
        }
        if self.contract_vers[face] != self.bus_ver {
            return Err(ItCode::ContractDrift);
        }
        Ok(())
    }

    /// 故障注入：登记隔离生效。
    pub fn drill(&mut self, f: Fault3, isolated: bool) {
        self.drills[f as usize] = (true, isolated);
    }

    /// 三景全过（隔离生效）。
    pub fn drills_ok(&self) -> bool {
        self.drills.iter().all(|(d, iso)| *d && *iso)
    }

    /// 全矩阵齐备。
    pub fn full(&self) -> bool {
        self.holes().is_empty()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C525-01..12。
pub fn run_ves525_checks() -> CheckSet {
    let mut set = CheckSet::new("ves525");

    let mut it = IntegTest::new(7);
    // 矩阵
    for f in 0..FACE9 {
        for e in 0..ENV_N {
            let _ = it.test_cell(f, match e { 0 => Env::Dev, 1 => Env::Staging, _ => Env::Prod }, true);
        }
    }
    chk(&mut set, "C525-01", it.full(), "九面×三环境全格实测");
    chk(&mut set, "C525-02", it.test_cell(9, Env::Dev, true) == Err(ItCode::MatrixHole), "越界面拒收");
    let _ = it.test_cell(0, Env::Dev, false);
    chk(&mut set, "C525-03", it.reruns == 1 && it.holes() == alloc::vec![(0, 0)], "失败格=漏格补测排程");

    // 契约
    for f in 0..FACE9 {
        let _ = it.contract_check(f);
    }
    chk(&mut set, "C525-04", it.contract_check(0).is_ok(), "双端契约对拍通过");
    it.bus_ver = 8;
    chk(&mut set, "C525-05", it.contract_check(0) == Err(ItCode::ContractDrift), "总线升级面未跟=契约漂移拦截");
    chk(&mut set, "C525-06", it.contract_check(99) == Err(ItCode::ContractDrift), "越界面对拍拒绝");
    it.contract_vers[0] = 8;
    chk(&mut set, "C525-07", it.contract_check(0).is_ok(), "版本戳跟齐恢复");

    // 故障三景
    it.drill(Fault3::BusDown, true);
    it.drill(Fault3::AdapterCrash, true);
    chk(&mut set, "C525-08", !it.drills_ok(), "两景注入未齐");
    it.drill(Fault3::WritebackFail, false);
    chk(&mut set, "C525-09", !it.drills_ok(), "回写失败景隔离未生效");
    it.drills[2].1 = true;
    chk(&mut set, "C525-10", it.drills_ok(), "三景齐且隔离生效");

    let _ = it.test_cell(0, Env::Dev, true);
    chk(&mut set, "C525-11", it.holes().len() == 0, "补测后全格齐");
    chk(&mut set, "C525-12", it.reruns >= 1 && ENV_N == 3, "矩阵维度与重跑账自洽");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integ_matrix() {
        let mut it = IntegTest::new(1);
        assert!(it.test_cell(0, Env::Dev, true).is_ok());
        assert_eq!(it.test_cell(9, Env::Dev, true), Err(ItCode::MatrixHole));
        assert_eq!(it.holes().len(), FACE9 * ENV_N - 1);
        it.drill(Fault3::BusDown, true);
        assert!(!it.drills_ok());
    }
}
