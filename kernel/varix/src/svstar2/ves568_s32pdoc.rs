//! VE-F4334 · 一致性性能文档（U07 性能与预算组）
//! 职责：性能组文档——文档族四类（性能架构册 / 预算表 / 基准协议 / 调优
//! 手册）+ 性能数据快照随文档发布（防口径漂移——快照缺失阻断发布）+ 调优
//! 手册案例化（每案例含前后对比）+ 案例过时版本戳归档 + 口径漂移重出快照
//! + 文档读屏标准（域本色）。
//! 段位：0x36A0（续表 idx568）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x36A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PdCode {
    /// 快照缺失（阻断发布）。
    NoSnapshot = SEG_BASE,
    /// 口径漂移（重出快照）。
    CaliberDrift = SEG_BASE + 0x01,
    /// 案例缺前后对比（拒收）。
    NoBeforeAfter = SEG_BASE + 0x02,
    /// 文档类越界。
    DocRange = SEG_BASE + 0x03,
}

/// 四族文档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocKind {
    PerfArch,
    BudgetTable,
    BenchProtocol,
    TuningManual,
}

/// 数据快照（性能口径冻结点）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapshot {
    pub caliber: u32,
    /// 快照指标：p95_ms / 单位成本点 / 基准均值 ms。
    pub p95_ms: u32,
    pub unit_cost: u32,
    pub bench_mean: u32,
}

/// 调优案例。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TuneCase {
    pub id: u32,
    pub ver: u32,
    /// 前后对比（前后均值 ms）。
    pub before_ms: u32,
    pub after_ms: u32,
    /// 读屏标准（域色——结构齐备）。
    pub sr_ok: bool,
    pub archived: bool,
}

/// 性能文档台。
#[derive(Default)]
pub struct PerfDocs {
    /// 四族齐备位。
    pub kinds: Vec<DocKind>,
    pub snap: Option<Snapshot>,
    pub cases: Vec<TuneCase>,
    /// 发布中标记。
    pub publishing: bool,
    pub resnaps: u32,
    pub archived: u32,
}

impl PerfDocs {
    pub fn new() -> Self {
        PerfDocs { ..Default::default() }
    }

    /// 文档族登记（缺类补齐用；幂等）。
    pub fn register(&mut self, k: DocKind) {
        if !self.kinds.iter().any(|x| *x == k) {
            self.kinds.push(k);
        }
    }

    /// 快照挂载（覆盖式——口径版本单源）。
    pub fn attach(&mut self, s: Snapshot) {
        self.snap = Some(s);
    }

    /// 发布闸：四族齐 + 快照在册 + 快照口径 = 当前口径才可发布。
    pub fn publish_gate(&self, cur_caliber: u32) -> Result<(), PdCode> {
        if self.kinds.len() < 4 {
            return Err(PdCode::DocRange);
        }
        let s = self.snap.as_ref().ok_or(PdCode::NoSnapshot)?;
        if s.caliber != cur_caliber {
            return Err(PdCode::CaliberDrift);
        }
        Ok(())
    }

    /// 口径漂移重出快照：新口径快照覆盖旧（计数留痕）。
    pub fn resnap(&mut self, cur_caliber: u32, p95: u32, cost: u32, bench: u32) -> Snapshot {
        let s = Snapshot { caliber: cur_caliber, p95_ms: p95, unit_cost: cost, bench_mean: bench };
        self.snap = Some(s);
        self.resnaps += 1;
        s
    }

    /// 调优案例登记：缺前后对比拒收（before/after 都须 >0）；读屏缺拒（域色）。
    pub fn add_case(&mut self, id: u32, ver: u32, before: u32, after: u32, sr_ok: bool) -> Result<(), PdCode> {
        if before == 0 || after == 0 {
            return Err(PdCode::NoBeforeAfter);
        }
        if !sr_ok {
            return Err(PdCode::NoBeforeAfter);
        }
        if let Some(c) = self.cases.iter_mut().find(|c| c.id == id) {
            c.ver = ver;
            c.before_ms = before;
            c.after_ms = after;
        } else {
            self.cases.push(TuneCase { id, ver, before_ms: before, after_ms: after, sr_ok, archived: false });
        }
        Ok(())
    }

    /// 案例收益核算：优化比例 bp（正 = 真优化）。
    pub fn gain_bp(&self, id: u32) -> Result<i64, PdCode> {
        let c = self.cases.iter().find(|c| c.id == id).ok_or(PdCode::NoBeforeAfter)?;
        Ok((c.before_ms as i64 - c.after_ms as i64) * 10000 / c.before_ms as i64)
    }

    /// 案例过时归档：版本戳低于当前口径即归档（留痕）。
    pub fn archive_stale(&mut self, cur_caliber: u32) -> usize {
        let mut n = 0;
        for c in self.cases.iter_mut() {
            if c.ver < cur_caliber && !c.archived {
                c.archived = true;
                self.archived += 1;
                n += 1;
            }
        }
        n
    }

    /// 全册读屏标准断言（域本色）。
    pub fn sr_clean(&self) -> bool {
        self.cases.iter().all(|c| c.sr_ok)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves568_checks() -> CheckSet {
    let mut set = CheckSet::new("ves568");
    let mut pd = PerfDocs::new();

    // C568-01 四族文档登记（幂等）。
    pd.register(DocKind::PerfArch);
    pd.register(DocKind::BudgetTable);
    pd.register(DocKind::BenchProtocol);
    pd.register(DocKind::TuningManual);
    pd.register(DocKind::PerfArch); // 重复登记不膨胀
    chk(&mut set, "C568-01", pd.kinds.len() == 4, "四族登记幂等");

    // C568-02 快照缺失阻断发布。
    chk(&mut set, "C568-02", pd.publish_gate(7) == Err(PdCode::NoSnapshot), "缺快照阻断");

    // C568-03 快照口径一致放行。
    pd.attach(Snapshot { caliber: 7, p95_ms: 90, unit_cost: 100, bench_mean: 55 });
    chk(&mut set, "C568-03", pd.publish_gate(7).is_ok(), "口径一致放行");

    // C568-04 口径漂移阻断 + 重出快照解除。
    chk(&mut set, "C568-04", pd.publish_gate(8) == Err(PdCode::CaliberDrift), "漂移阻断");
    let s2 = pd.resnap(8, 85, 98, 52);
    chk(&mut set, "C568-04b", s2.caliber == 8 && pd.publish_gate(8).is_ok() && pd.resnaps == 1, "重出快照解除");

    // C568-05 案例缺前后对比拒收。
    chk(&mut set, "C568-05", pd.add_case(1, 7, 0, 50, true).is_err()
        && pd.add_case(1, 7, 100, 0, true).is_err(), "缺对比拒收");

    // C568-06 读屏结构缺失拒收（域色硬线）。
    chk(&mut set, "C568-06", pd.add_case(1, 7, 100, 50, false).is_err(), "读屏缺拒收");

    // C568-07 合规案例登记 + 收益核算。
    chk(&mut set, "C568-07", pd.add_case(1, 7, 100, 50, true).is_ok()
        && pd.gain_bp(1) == Ok(5000), "案例登记收益 50%");

    // C568-08 负收益（调劣化）如实呈现。
    pd.add_case(2, 7, 40, 60, true).unwrap_or(());
    chk(&mut set, "C568-08", pd.gain_bp(2) == Ok(-5000), "负收益如实");

    // C568-09 案例过时归档（版本戳低于当前口径）。
    pd.add_case(3, 3, 200, 100, true).unwrap_or(());
    let n = pd.archive_stale(8);
    chk(&mut set, "C568-09", n == 1 && pd.cases[2].archived && pd.archived == 1, "过时归档留痕");

    // C568-10 归档幂等（重复归档不重复计）。
    let n2 = pd.archive_stale(8);
    chk(&mut set, "C568-10", n2 == 0 && pd.archived == 1, "归档幂等");

    // C568-11 全册读屏标准断言。
    chk(&mut set, "C568-11", pd.sr_clean(), "全册读屏洁");

    // C568-12 无案例台账安全 + 收益查询无件拒。
    let pd2 = PerfDocs::new();
    chk(&mut set, "C568-12", pd2.sr_clean() && pd2.gain_bp(9).is_err(), "无件拒+空册洁");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_needs_snapshot() {
        let mut pd = PerfDocs::new();
        for k in [DocKind::PerfArch, DocKind::BudgetTable, DocKind::BenchProtocol, DocKind::TuningManual] {
            pd.register(k);
        }
        assert_eq!(pd.publish_gate(1), Err(PdCode::NoSnapshot));
        pd.attach(Snapshot { caliber: 1, p95_ms: 1, unit_cost: 1, bench_mean: 1 });
        assert!(pd.publish_gate(1).is_ok());
        assert_eq!(pd.publish_gate(2), Err(PdCode::CaliberDrift));
    }

    #[test]
    fn case_gain() {
        let mut pd = PerfDocs::new();
        assert_eq!(pd.add_case(1, 1, 100, 80, true), Ok(()));
        assert_eq!(pd.gain_bp(1), Ok(2000));
        assert!(pd.add_case(2, 1, 0, 80, true).is_err());
        assert!(pd.gain_bp(9).is_err());
    }
}
