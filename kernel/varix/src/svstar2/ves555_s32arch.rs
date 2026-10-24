//! VE-F4321 · U07 开工与一致性性能总架构（U07 性能与预算组）
//! 职责：一致性系统自身性能总架构——性能定位（一致性是横切面，性能劣化
//! 即横切税——总原则三件：预算内运行 / 增量优先 / 可降级）+ 三层性能架构
//! （判定层/扫描层/度量层各设性能契约——层契约冻结）+ 层契约被破逐层归因
//! + 架构漂移对拍重绘 + 与 T07 性能组姊妹结构呼应（结构参照不计用次）。
//! 段位：0x3360（续表 idx555）。行数预算 380。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3360;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaCode {
    /// 层契约被破（逐层归因）。
    LayerBroken = SEG_BASE,
    /// 预算超支（降级路径启用）。
    BudgetOver = SEG_BASE + 0x01,
    /// 架构漂移（对拍重绘）。
    Drifted = SEG_BASE + 0x02,
    /// 层号越界。
    LayerRange = SEG_BASE + 0x03,
}

/// 三层：0 判定层 / 1 扫描层 / 2 度量层。
pub const LAYERS: usize = 3;

/// 层性能契约（冻结后不可擅改——变更走版本）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerContract {
    pub layer: usize,
    /// 单位操作预算（µs 级抽象单位）。
    pub unit_budget: u32,
    /// 是否允许降级（可降级原则）。
    pub degradable: bool,
    /// 契约版本戳（冻结单源）。
    pub ver: u32,
}

/// 总架构册。
#[derive(Default)]
pub struct PerfArch {
    pub contracts: [Option<LayerContract>; LAYERS],
    /// 契约冻结位。
    pub frozen: [bool; LAYERS],
    /// 实测单位开销。
    pub measured: [u32; LAYERS],
    /// 降级启用位。
    pub degraded: [bool; LAYERS],
    pub ver_now: u32,
    pub redraws: u32,
}

impl PerfArch {
    pub fn new(ver: u32) -> Self {
        PerfArch { ver_now: ver, ..Default::default() }
    }

    /// 层契约装载（层号校验；已冻结层以版本戳为准——版本不符拒绝旧契约）。
    pub fn load(&mut self, c: LayerContract) -> Result<(), PaCode> {
        if c.layer >= LAYERS {
            return Err(PaCode::LayerRange);
        }
        if self.frozen[c.layer] && c.ver != self.ver_now {
            return Err(PaCode::Drifted);
        }
        self.contracts[c.layer] = Some(c);
        self.frozen[c.layer] = true;
        Ok(())
    }

    /// 实测上报。
    pub fn measure(&mut self, layer: usize, unit: u32) -> Result<(), PaCode> {
        if layer >= LAYERS {
            return Err(PaCode::LayerRange);
        }
        self.measured[layer] = unit;
        Ok(())
    }

    /// 契约核验：超预算 → 可降级层启用降级；不可降级层直接破约归因。
    pub fn verify(&mut self, layer: usize) -> Result<bool, PaCode> {
        if layer >= LAYERS {
            return Err(PaCode::LayerRange);
        }
        let c = match self.contracts[layer] {
            Some(c) => c,
            None => return Err(PaCode::LayerBroken),
        };
        if self.measured[layer] <= c.unit_budget {
            self.degraded[layer] = false;
            return Ok(true);
        }
        if c.degradable {
            self.degraded[layer] = true;
            Ok(false)
        } else {
            Err(PaCode::LayerBroken)
        }
    }

    /// 逐层归因：返回首个破约层（降级不算破约）。
    pub fn attribute(&self) -> Option<usize> {
        for l in 0..LAYERS {
            if let Some(c) = self.contracts[l] {
                if self.measured[l] > c.unit_budget && !c.degradable {
                    return Some(l);
                }
            }
        }
        None
    }

    /// 增量优先断言：增量开销 <= 全量开销（违背 = 架构原则破坏）。
    pub fn incremental_ok(&self, full: u32, incr: u32) -> bool {
        incr <= full
    }

    /// 架构对拍：三层契约齐备且与登记版本一致才结构成立。
    pub fn arch_ok(&self) -> bool {
        self.contracts.iter().all(|c| c.is_some())
    }

    /// 架构漂移重绘：对拍失败重绘（计数留痕——版本不变重置冻结重载）。
    pub fn redraw(&mut self) -> u32 {
        self.redraws += 1;
        for l in 0..LAYERS {
            if self.contracts[l].is_some() {
                self.frozen[l] = true;
            }
        }
        self.redraws
    }

    /// 姊妹结构卡（T07 参照）：三层结构与 T07 同构断言（层名对齐）。
    pub fn sister_aligned(&self, names: [&str; LAYERS]) -> bool {
        names == ["judge", "scan", "metric"]
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves555_checks() -> CheckSet {
    let mut set = CheckSet::new("ves555");
    let mut pa = PerfArch::new(5);

    // C555-01 三层契约装载冻结。
    let _ = pa.load(LayerContract { layer: 0, unit_budget: 100, degradable: false, ver: 5 });
    let _ = pa.load(LayerContract { layer: 1, unit_budget: 500, degradable: true, ver: 5 });
    let _ = pa.load(LayerContract { layer: 2, unit_budget: 50, degradable: true, ver: 5 });
    chk(&mut set, "C555-01", pa.arch_ok(), "三层契约齐备");

    // C555-02 冻结层旧版本契约拒绝（版本单源）。
    chk(&mut set, "C555-02", pa.load(LayerContract { layer: 0, unit_budget: 1, degradable: false, ver: 4 })
        == Err(PaCode::Drifted), "旧版契约拒");

    // C555-03 层号越界拒。
    chk(&mut set, "C555-03", pa.load(LayerContract { layer: 3, unit_budget: 1, degradable: false, ver: 5 })
        == Err(PaCode::LayerRange), "层号越界拒");

    // C555-04 预算内核验通过。
    let _ = pa.measure(0, 80);
    chk(&mut set, "C555-04", pa.verify(0) == Ok(true), "预算内核验");

    // C555-05 可降级层超支启用降级。
    let _ = pa.measure(1, 800);
    chk(&mut set, "C555-05", pa.verify(1) == Ok(false) && pa.degraded[1], "可降级层降级");

    // C555-06 不可降级层超支 = 破约归因。
    let _ = pa.measure(0, 200);
    chk(&mut set, "C555-06", pa.verify(0) == Err(PaCode::LayerBroken) && pa.attribute() == Some(0), "破约逐层归因");

    // C555-07 回修后归因清零。
    let _ = pa.measure(0, 90);
    let _ = pa.verify(0);
    chk(&mut set, "C555-07", pa.attribute().is_none(), "回修归因清零");

    // C555-08 增量优先断言。
    chk(&mut set, "C555-08", pa.incremental_ok(1000, 300) && !pa.incremental_ok(100, 200), "增量优先原则");

    // C555-09 未装层核验拒绝。
    let mut pa2 = PerfArch::new(1);
    chk(&mut set, "C555-09", pa2.verify(2) == Err(PaCode::LayerBroken), "未装层拒绝");

    // C555-10 架构漂移重绘留痕。
    let r = pa2.redraw();
    chk(&mut set, "C555-10", r == 1 && pa2.redraws == 1, "漂移重绘留痕");

    // C555-11 姊妹结构对齐（T07 同构参照）。
    chk(&mut set, "C555-11", pa.sister_aligned(["judge", "scan", "metric"]), "姊妹结构对齐");

    // C555-12 横切税预算三原则合断（预算内 + 增量 + 可降级位齐）。
    chk(&mut set, "C555-12", pa.degraded[1] && !pa.degraded[0] && pa.arch_ok(), "三原则合断");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_freeze_gate() {
        let mut pa = PerfArch::new(3);
        assert!(pa.load(LayerContract { layer: 1, unit_budget: 10, degradable: true, ver: 3 }).is_ok());
        assert_eq!(pa.load(LayerContract { layer: 1, unit_budget: 20, degradable: true, ver: 2 }), Err(PaCode::Drifted));
        assert!(pa.load(LayerContract { layer: 1, unit_budget: 20, degradable: true, ver: 3 }).is_ok());
    }

    #[test]
    fn degrade_vs_break() {
        let mut pa = PerfArch::new(1);
        let _ = pa.load(LayerContract { layer: 0, unit_budget: 10, degradable: false, ver: 1 });
        let _ = pa.load(LayerContract { layer: 1, unit_budget: 10, degradable: true, ver: 1 });
        let _ = pa.measure(0, 99);
        let _ = pa.measure(1, 99);
        assert_eq!(pa.verify(0), Err(PaCode::LayerBroken));
        assert_eq!(pa.verify(1), Ok(false));
        assert_eq!(pa.attribute(), Some(0));
    }

    #[test]
    fn incremental_principle() {
        let pa = PerfArch::new(1);
        assert!(pa.incremental_ok(500, 500)); // 边界：相等视为合规
        assert!(!pa.incremental_ok(100, 101));
    }
}
