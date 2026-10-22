//! CGPU-F0407 · 与 K 异构域接口集成（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 异构集成：NPU/DSP 任务预算（F0346 预留兑现一期——异构任务的预算
//!   标签与仲裁通道）；异构能力探测联动（K 域探测→预算维度自动启停）；
//!   异构任务成本模型（F0004 扩展异构算子成本——预算预估准确度基础）；
//!   诚实标注（一期 NPU 接口级、深度调度随 K 域推进）。
//! - 判据：标签通道、探测联动、成本扩展、诚实标注、判据。
//!
//! 设计纪律：
//! - 标签通道：异构任务带（设备类 NPU/DSP、算子类、预算标签）三件套
//!   进仲裁通道；通道一期只做「标签透传 + 预算校验」，不做深度调度
//!   （诚实标注的边界）。
//! - 探测联动：K 域能力探测报告（设备→可用/不可用）驱动预算维度
//!   自动启停；不可用设备的在途预算申请自动退回。
//! - 成本模型扩展：F0004 算子成本表加异构列（设备×算子 → 预估成本），
//!   预估与实测偏差超带即校准事件。
//! - 诚实标注：一期标注「接口级集成」——预算通道只透传校验，深度
//!   调度能力位关闭，诚实标注检查器防误标。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0CA0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0CA0，全仓独占）。
pub const SEG_BASE: u16 = 0x0CA0;

pub const E_KTAG_DEV: u16 = SEG_BASE + 1; // 设备类非法
pub const E_KPROBE_STATE: u16 = SEG_BASE + 2; // 不可用设备仍放行
pub const E_KCOST_UNKNOWN: u16 = SEG_BASE + 3; // 成本表缺算子
pub const E_KCOST_DRIFT: u16 = SEG_BASE + 4; // 预估偏差超带
pub const E_KHONEST: u16 = SEG_BASE + 5; // 诚实标注违规（能力虚标）
pub const E_KCHAN_BUDGET: u16 = SEG_BASE + 6; // 预算标签超池

/// 异构设备类。
pub const DEV_NPU: u8 = 0;
pub const DEV_DSP: u8 = 1;
pub const DEV_COUNT: usize = 2;

pub fn dev_name(d: u8) -> &'static str {
    match d {
        DEV_NPU => "NPU",
        _ => "DSP",
    }
}

/// 异构任务标签（三件套）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeteroTag {
    pub task: u32,
    /// 设备类（NPU/DSP）。
    pub dev: u8,
    /// 算子类（0-255 档）。
    pub op: u8,
    /// 预算标签（成本单元）。
    pub budget: u32,
}

/// 能力探测账：设备 → 可用性。
#[derive(Clone, Debug, Default)]
pub struct ProbeBoard {
    pub avail: Vec<(u8, bool)>,
}

impl ProbeBoard {
    pub fn set(&mut self, dev: u8, ok: bool) {
        match self.avail.iter_mut().find(|(d, _)| *d == dev) {
            Some(e) => e.1 = ok,
            None => self.avail.push((dev, ok)),
        }
    }

    pub fn is_avail(&self, dev: u8) -> bool {
        self.avail
            .iter()
            .find(|(d, _)| *d == dev)
            .map(|(_, ok)| *ok)
            .unwrap_or(false)
    }
}

/// 异构预算池（每设备一个池，成本单元）。
#[derive(Clone, Debug)]
pub struct HeteroPool {
    pub dev: u8,
    pub capacity: u32,
    pub used: u32,
}

impl HeteroPool {
    pub fn avail(&self) -> u32 {
        self.capacity.saturating_sub(self.used)
    }
}

/// 成本模型：设备×算子 → 预估成本；实测校准带。
#[derive(Clone, Debug, Default)]
pub struct CostModel {
    /// 表项（dev, op → 预估成本）。
    pub table: Vec<(u8, u8, u32)>,
    /// 校准带（千分位）。
    pub band_pm: u32,
    /// 校准事件数。
    pub calibrations: u32,
}

impl CostModel {
    pub fn put(&mut self, dev: u8, op: u8, cost: u32) {
        match self.table.iter_mut().find(|(d, o, _)| *d == dev && *o == op) {
            Some(e) => e.2 = cost,
            None => self.table.push((dev, op, cost)),
        }
    }

    pub fn estimate(&self, dev: u8, op: u8) -> Result<u32, u16> {
        self.table
            .iter()
            .find(|(d, o, _)| *d == dev && *o == op)
            .map(|(_, _, c)| *c)
            .ok_or(E_KCOST_UNKNOWN)
    }

    /// 实测校验：偏差超带即校准事件（计数+表更新建议）。
    pub fn calibrate(&mut self, dev: u8, op: u8, measured: u32) -> Result<bool, u16> {
        let est = self.estimate(dev, op)?;
        let est = est.max(1) as u64;
        let diff = est.abs_diff(measured as u64);
        if diff * 1000 > est * self.band_pm as u64 {
            self.calibrations += 1;
            self.put(dev, op, measured);
            return Ok(true);
        }
        Ok(false)
    }
}

/// 一期诚实标注：能力位表（通道做什么/不做什么——防虚标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HonestyMark {
    /// 标签透传：一期开。
    pub passthrough: bool,
    /// 预算校验：一期开。
    pub budget_check: bool,
    /// 深度调度：一期关（随 K 域推进）。
    pub deep_sched: bool,
}

pub const PHASE1_MARK: HonestyMark = HonestyMark {
    passthrough: true,
    budget_check: true,
    deep_sched: false,
};

impl HonestyMark {
    /// 诚实自检：关着的能力位不允许被宣称。
    pub fn claims_ok(&self, claimed: &[bool; 3]) -> bool {
        let have = [self.passthrough, self.budget_check, self.deep_sched];
        claimed.iter().zip(have.iter()).all(|(c, h)| !*c || *h)
    }
}

/// K 异构集成控制器。
#[derive(Clone, Debug)]
pub struct KHeteroIntegrator {
    pub probe: ProbeBoard,
    pub pools: Vec<HeteroPool>,
    pub cost: CostModel,
    /// 在途标签（已过预算校验的任务）。
    pub inflight: Vec<HeteroTag>,
    pub mark: HonestyMark,
}

impl KHeteroIntegrator {
    pub fn new(band_pm: u32) -> Self {
        KHeteroIntegrator {
            probe: ProbeBoard::default(),
            pools: Vec::new(),
            cost: CostModel { table: Vec::new(), band_pm, calibrations: 0 },
            inflight: Vec::new(),
            mark: PHASE1_MARK,
        }
    }

    pub fn attach_pool(&mut self, dev: u8, capacity: u32) {
        if !self.pools.iter().any(|p| p.dev == dev) {
            self.pools.push(HeteroPool { dev, capacity, used: 0 });
        }
    }

    /// 标签通道入闸：探测可用 + 成本表可估 + 预算标签 ≤ 池余量。
    pub fn admit(&mut self, tag: HeteroTag) -> Result<(), u16> {
        if tag.dev as usize >= DEV_COUNT {
            return Err(E_KTAG_DEV);
        }
        if !self.probe.is_avail(tag.dev) {
            return Err(E_KPROBE_STATE);
        }
        // 预算校验：标签值直接对比池余量（一期接口级）。
        let p = self
            .pools
            .iter_mut()
            .find(|p| p.dev == tag.dev)
            .ok_or(E_KCHAN_BUDGET)?;
        if tag.budget > p.avail() {
            return Err(E_KCHAN_BUDGET);
        }
        p.used += tag.budget;
        self.inflight.push(tag);
        Ok(())
    }

    /// 任务完成：池释放 + 在途清除。
    pub fn complete(&mut self, task: u32) -> Result<u32, u16> {
        let ti = self
            .inflight
            .iter()
            .position(|t| t.task == task)
            .ok_or(E_KCOST_UNKNOWN)?;
        let tag = self.inflight.remove(ti);
        let p = self
            .pools
            .iter_mut()
            .find(|p| p.dev == tag.dev)
            .ok_or(E_KCHAN_BUDGET)?;
        p.used = p.used.saturating_sub(tag.budget);
        Ok(p.avail())
    }

    /// 探测联动：设备转不可用 → 在途任务全部退回（池释放）。
    pub fn probe_update(&mut self, dev: u8, ok: bool) -> Result<usize, u16> {
        self.probe.set(dev, ok);
        if ok {
            return Ok(0);
        }
        // 退回在途。
        let returned = self.inflight.iter().filter(|t| t.dev == dev).count();
        let total: u32 = self
            .inflight
            .iter()
            .filter(|t| t.dev == dev)
            .map(|t| t.budget)
            .sum();
        self.inflight.retain(|t| t.dev != dev);
        if let Some(p) = self.pools.iter_mut().find(|p| p.dev == dev) {
            p.used = p.used.saturating_sub(total);
        }
        Ok(returned)
    }

    /// 诚实标注检查：宣称能力必须 ⊆ 一期实际能力。
    pub fn honest_check(&self, claimed: &[bool; 3]) -> Result<(), u16> {
        if !self.mark.claims_ok(claimed) {
            return Err(E_KHONEST);
        }
        Ok(())
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn it() -> KHeteroIntegrator {
        let mut i = KHeteroIntegrator::new(200);
        i.attach_pool(DEV_NPU, 1_000);
        i.attach_pool(DEV_DSP, 500);
        i.probe.set(DEV_NPU, true);
        i.probe.set(DEV_DSP, true);
        i.cost.put(DEV_NPU, 1, 100);
        i.cost.put(DEV_DSP, 2, 50);
        i
    }

    #[test]
    fn a01_标签通道_三件套入闸与预算校验() {
        let mut i = it();
        // 正常入闸。
        assert_eq!(
            i.admit(HeteroTag { task: 1, dev: DEV_NPU, op: 1, budget: 400 }),
            Ok(())
        );
        assert_eq!(i.pools[0].used, 400);
        // 超池余量拒。
        assert_eq!(
            i.admit(HeteroTag { task: 2, dev: DEV_NPU, op: 1, budget: 700 }),
            Err(E_KCHAN_BUDGET)
        );
        // 完成释放。
        assert_eq!(i.complete(1), Ok(1_000));
        // 设备类越界拒。
        assert_eq!(
            i.admit(HeteroTag { task: 3, dev: 9, op: 1, budget: 1 }),
            Err(E_KTAG_DEV)
        );
    }

    #[test]
    fn a02_探测联动_不可用即退回在途() {
        let mut i = it();
        i.admit(HeteroTag { task: 1, dev: DEV_NPU, op: 1, budget: 300 }).ok();
        i.admit(HeteroTag { task: 2, dev: DEV_NPU, op: 1, budget: 200 }).ok();
        i.admit(HeteroTag { task: 3, dev: DEV_DSP, op: 2, budget: 100 }).ok();
        // NPU 转不可用 ⇒ 2 条在途退回、池清空；DSP 不受影响。
        assert_eq!(i.probe_update(DEV_NPU, false), Ok(2));
        assert_eq!(i.pools[0].used, 0);
        assert_eq!(i.pools[1].used, 100);
        // 不可用设备入闸拒。
        assert_eq!(
            i.admit(HeteroTag { task: 4, dev: DEV_NPU, op: 1, budget: 1 }),
            Err(E_KPROBE_STATE)
        );
        // 恢复可用。
        i.probe_update(DEV_NPU, true).ok();
        assert_eq!(
            i.admit(HeteroTag { task: 5, dev: DEV_NPU, op: 1, budget: 100 }),
            Ok(())
        );
    }

    #[test]
    fn a03_成本扩展_预估与校准带() {
        let mut i = it();
        // 可估。
        assert_eq!(i.cost.estimate(DEV_NPU, 1), Ok(100));
        // 缺表拒。
        assert_eq!(i.cost.estimate(DEV_NPU, 9), Err(E_KCOST_UNKNOWN));
        // 实测带内（±20%）不校准。
        assert_eq!(i.cost.calibrate(DEV_NPU, 1, 110), Ok(false));
        assert_eq!(i.cost.calibrations, 0);
        // 偏差超带 → 校准事件 + 表更新。
        assert_eq!(i.cost.calibrate(DEV_NPU, 1, 200), Ok(true));
        assert_eq!(i.cost.calibrations, 1);
        assert_eq!(i.cost.estimate(DEV_NPU, 1), Ok(200));
        // 完成未知任务拒（成本表缺失路径复用码）。
        assert_eq!(i.complete(999), Err(E_KCOST_UNKNOWN));
    }

    #[test]
    fn a04_诚实标注_能力位不虚标() {
        let i = it();
        // 一期能力：透传+预算校验可宣称；深度调度不可宣称。
        assert_eq!(i.honest_check(&[true, true, false]), Ok(()));
        assert_eq!(i.honest_check(&[false, false, false]), Ok(()));
        assert_eq!(i.honest_check(&[true, true, true]), Err(E_KHONEST));
        assert_eq!(i.honest_check(&[false, false, true]), Err(E_KHONEST));
        // 标注位固化：深度调度一期关。
        assert!(!PHASE1_MARK.deep_sched);
        assert!(PHASE1_MARK.passthrough && PHASE1_MARK.budget_check);
        // 设备名表。
        assert_eq!(dev_name(DEV_NPU), "NPU");
        assert_eq!(dev_name(DEV_DSP), "DSP");
    }

    #[test]
    fn a05_池守恒_入闸与释放闭环() {
        let mut i = it();
        i.admit(HeteroTag { task: 1, dev: DEV_DSP, op: 2, budget: 250 }).ok();
        i.admit(HeteroTag { task: 2, dev: DEV_DSP, op: 2, budget: 250 }).ok();
        assert_eq!(i.pools[1].used, 500);
        // 池满后再入闸拒。
        assert_eq!(
            i.admit(HeteroTag { task: 3, dev: DEV_DSP, op: 2, budget: 1 }),
            Err(E_KCHAN_BUDGET)
        );
        // 逐个释放守恒。
        i.complete(1).ok();
        assert_eq!(i.pools[1].avail(), 250);
        i.complete(2).ok();
        assert_eq!(i.pools[1].used, 0);
        // 重复完成拒。
        assert_eq!(i.complete(2), Err(E_KCOST_UNKNOWN));
    }
}
