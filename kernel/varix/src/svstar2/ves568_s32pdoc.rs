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
    /// 快照指纹不符（历史账对账失败）。
    SnapFpMismatch = SEG_BASE + 0x04,
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
    /// 深化：快照覆盖历史账 (caliber, fp)。
    pub attach_history: Vec<(u32, u32)>,
    /// 深化：发布履历（成功/阻断计数）。
    pub pub_ok: u32,
    pub pub_blocked: u32,
}

impl PerfDocs {
    pub fn new() -> Self {
        PerfDocs { ..Default::default() }
    }

    // ─── 深化块：快照指纹 ───

    /// 快照 FNV-1a 指纹（四字段逐字节）。
    pub fn snap_fp(s: &Snapshot) -> u32 {
        let mut h: u32 = 0x811C9DC5;
        for v in [s.caliber, s.p95_ms, s.unit_cost, s.bench_mean] {
            let mut x = v;
            for _ in 0..4 {
                h ^= x & 0xFF;
                h = h.wrapping_mul(0x01000193);
                x >>= 8;
            }
        }
        h
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

    // ─── 深化块：快照指纹历史账（覆盖不丢账）───

    /// 覆盖式挂载的历史账版：旧快照指纹先入史再覆盖（账不丢）。
    /// 注：字段 attach_history/pub_ok/pub_blocked 在 struct 定义处。

    /// 挂带历史的 attach：当前快照在册时先记账。
    pub fn attach_keep(&mut self, s: Snapshot) {
        if let Some(old) = self.snap {
            self.attach_history.push((old.caliber, Self::snap_fp(&old)));
        }
        self.snap = Some(s);
    }

    /// 历史账对账：某口径的旧快照指纹可查（覆盖追溯）。
    pub fn history_has(&self, caliber: u32, fp: u32) -> bool {
        self.attach_history.iter().any(|(c, f)| *c == caliber && *f == fp)
    }

    // ─── 深化块：案例收益 top-K 投影 ───

    /// 收益 top-K：按收益 bp 降序取前 K 个案例 id（K=0 空表）。
    pub fn top_gains(&self, k: usize) -> Vec<u32> {
        let mut ranked: Vec<(i64, u32)> = self.cases.iter()
            .map(|c| (((c.before_ms as i64 - c.after_ms as i64) * 10000 / c.before_ms.max(1) as i64), c.id))
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0));
        ranked.into_iter().take(k).map(|(_, id)| id).collect()
    }

    // ─── 深化块：发布履历守恒 ───

    /// 发布闸带履历：成功/阻断都计数，守恒可对账。
    pub fn publish_gated(&mut self, cur_caliber: u32) -> Result<(), PdCode> {
        match self.publish_gate(cur_caliber) {
            Ok(()) => {
                self.pub_ok += 1;
                self.publishing = true;
                Ok(())
            }
            Err(e) => {
                self.pub_blocked += 1;
                Err(e)
            }
        }
    }

    /// 履历守恒：成功+阻断 ≡ 尝试总数（外部传入尝试数）。
    pub fn pub_conserves(&self, attempts: u32) -> bool {
        self.pub_ok + self.pub_blocked == attempts
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

    // ── 深化判据：快照指纹（确定性 + 差异敏感）──
    let sa = Snapshot { caliber: 8, p95_ms: 85, unit_cost: 98, bench_mean: 52 };
    let sb = Snapshot { caliber: 8, p95_ms: 85, unit_cost: 98, bench_mean: 53 };
    chk(&mut set, "C568-13", PerfDocs::snap_fp(&sa) == PerfDocs::snap_fp(&sa)
        && PerfDocs::snap_fp(&sa) != PerfDocs::snap_fp(&sb), "指纹确定且差异敏感");

    // ── 深化判据：覆盖历史账（覆盖不丢 + 可追溯）──
    let mut ph = PerfDocs::new();
    ph.attach_keep(sa);
    let fp_a = PerfDocs::snap_fp(&sa);
    let sc = Snapshot { caliber: 9, p95_ms: 80, unit_cost: 95, bench_mean: 50 };
    ph.attach_keep(sc);
    chk(&mut set, "C568-14", ph.attach_history.len() == 1
        && ph.history_has(8, fp_a), "覆盖旧快照入史可追溯");

    // ── 深化判据：发布履历守恒（3 阻断 + 1 成功）──
    let mut pg = PerfDocs::new();
    for k in [DocKind::PerfArch, DocKind::BudgetTable, DocKind::BenchProtocol, DocKind::TuningManual] {
        pg.register(k);
    }
    let _ = pg.publish_gated(8); // 缺快照阻断
    pg.attach_keep(sa);
    let _ = pg.publish_gated(8); // 成功
    let _ = pg.publish_gated(9); // 漂移阻断
    let _ = pg.publish_gated(9); // 漂移阻断
    chk(&mut set, "C568-15", pg.pub_ok == 1 && pg.pub_blocked == 3
        && pg.pub_conserves(4), "发布履历 1 成功 3 阻断守恒");
    chk(&mut set, "C568-16", !pg.pub_conserves(5), "尝试数不符守恒即破");

    // ── 深化判据：收益 top-K 投影 ──
    let mut pt = PerfDocs::new();
    let _ = pt.add_case(1, 8, 100, 90, true);  // 10%
    let _ = pt.add_case(2, 8, 100, 50, true);  // 50%
    let _ = pt.add_case(3, 8, 100, 70, true);  // 30%
    let _ = pt.add_case(4, 8, 100, 80, true);  // 20%
    chk(&mut set, "C568-17", pt.top_gains(2) == [2u32, 3u32]
        && pt.top_gains(0).is_empty() && pt.top_gains(10).len() == 4, "收益 top-K 降序");

    // ── 深化判据：LCG 快照注入（指纹无碰撞 256 轮）──
    let mut rng: u64 = 0x5680_F00D;
    let mut fps: Vec<u32> = Vec::new();
    let mut collide = false;
    for _ in 0..256 {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let s = Snapshot {
            caliber: (rng % 64) as u32,
            p95_ms: 50 + (rng >> 8) as u32 % 200,
            unit_cost: 80 + (rng >> 16) as u32 % 200,
            bench_mean: 30 + (rng >> 24) as u32 % 200,
        };
        let fp = PerfDocs::snap_fp(&s);
        if fps.contains(&fp) {
            collide = true;
        }
        fps.push(fp);
    }
    chk(&mut set, "C568-18", !collide, "256 轮指纹零碰撞");

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

    #[test]
    fn snap_fp_determinism() {
        let s = Snapshot { caliber: 3, p95_ms: 10, unit_cost: 20, bench_mean: 30 };
        assert_eq!(PerfDocs::snap_fp(&s), PerfDocs::snap_fp(&s));
        let s2 = Snapshot { caliber: 4, p95_ms: 10, unit_cost: 20, bench_mean: 30 };
        assert_ne!(PerfDocs::snap_fp(&s), PerfDocs::snap_fp(&s2));
    }

    #[test]
    fn top_gains_order() {
        let mut pd = PerfDocs::new();
        let _ = pd.add_case(1, 1, 100, 80, true);
        let _ = pd.add_case(2, 1, 100, 50, true);
        assert_eq!(pd.top_gains(1), [2]);
        assert_eq!(pd.top_gains(5), [2, 1]);
    }

    #[test]
    fn publish_ledger_conserves() {
        let mut pd = PerfDocs::new();
        let _ = pd.publish_gated(1); // 阻断（无四族）
        assert_eq!(pd.pub_blocked, 1);
        assert!(pd.pub_conserves(1));
    }
}
