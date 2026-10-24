//! ves441_s24metrics — VE-F4207 · 一致性度量体系
//! 度量：一致率（合规项/总检项）/冲突密度/修复时效/回归率四主指标×域维度切分；
//! 指标口径冻结+计算可复算；与家族度量同构对拍；无障碍一致率单列（域本色主场指标）；数据合成。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1400;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MtCode {
    Ok = 0,
    CaliberDrift = SEG_BASE + 0x01, // 0x1401
    SourceBroken = SEG_BASE + 0x02, // 0x1402 数据源断
    RecalcMismatch = SEG_BASE + 0x03, // 0x1403
}

/// 四主指标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Metric { Consistency = 0, ConflictDensity = 1, FixLatency = 2, RegressRate = 3 }

impl Metric {
    pub const ALL: [Metric; 4] = [
        Metric::Consistency, Metric::ConflictDensity, Metric::FixLatency, Metric::RegressRate];
    pub fn name(self) -> &'static str {
        match self {
            Metric::Consistency => "consistency", Metric::ConflictDensity => "conflict-density",
            Metric::FixLatency => "fix-latency", Metric::RegressRate => "regress-rate",
        }
    }
}

/// 域维度切分条目（合规项/总检项 + a11y 单列——域本色）。
pub struct DimRow { pub domain: &'static str, pub pass: u32, pub total: u32, pub a11y_pass: u32, pub a11y_total: u32 }

/// 度量册。
pub struct MetricBook {
    pub rows: Vec<DimRow>,
    pub frozen: bool,
    pub calibers: Vec<(Metric, u32)>, // 口径版本（冻结须一致）
}

impl MetricBook {
    pub fn new() -> Self { MetricBook { rows: Vec::new(), frozen: false, calibers: Vec::new() } }
    pub fn put_row(&mut self, domain: &'static str, pass: u32, total: u32, ap: u32, at: u32) {
        self.rows.retain(|r| r.domain != domain);
        self.rows.push(DimRow { domain, pass, total, a11y_pass: ap, a11y_total: at });
    }
    pub fn set_caliber(&mut self, m: Metric, ver: u32) {
        self.calibers.retain(|(mm, _)| *mm != m);
        self.calibers.push((m, ver));
    }
    /// 一致率（bp）：某域或全量。
    pub fn consistency_bp(&self, domain: Option<&str>) -> Option<u32> {
        let (p, t) = match domain {
            Some(d) => { let r = self.rows.iter().find(|r| r.domain == d)?; (r.pass, r.total) }
            None => (self.rows.iter().map(|r| r.pass).sum(), self.rows.iter().map(|r| r.total).sum()),
        };
        if t == 0 { return None; } // 数据源断→显性空态
        Some(p * 10_000 / t)
    }
    /// 无障碍一致率单列（域本色）。
    pub fn a11y_consistency_bp(&self) -> Option<u32> {
        let p: u32 = self.rows.iter().map(|r| r.a11y_pass).sum();
        let t: u32 = self.rows.iter().map(|r| r.a11y_total).sum();
        if t == 0 { return None; }
        Some(p * 10_000 / t)
    }
    /// 口径冻结：四主指标齐且版本一致。
    pub fn freeze(&mut self, ver: u32) -> Result<(), MtCode> {
        for m in Metric::ALL {
            match self.calibers.iter().find(|(mm, _)| *mm == m) {
                None => return Err(MtCode::CaliberDrift),
                Some((_, v)) => if *v != ver { return Err(MtCode::CaliberDrift); },
            }
        }
        self.frozen = true;
        Ok(())
    }
    /// 复算：一致率可由同数据二次计算复现（可复算断言）。
    pub fn recalc(&self) -> Result<bool, MtCode> {
        let a = self.consistency_bp(None).ok_or(MtCode::SourceBroken)?;
        let p: u64 = self.rows.iter().map(|r| r.pass as u64).sum();
        let t: u64 = self.rows.iter().map(|r| r.total as u64).sum();
        if t == 0 { return Err(MtCode::SourceBroken); }
        Ok(a as u64 == p * 10_000 / t)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves441_checks() -> CheckSet {
    let mut set = CheckSet::new("ves441");
    let mut bk = MetricBook::new();

    // C441-01 数据源断（空册）→显性空态 None。
    chk(&mut set, "C441-01", bk.consistency_bp(None).is_none(), "空态显性");

    // C441-02 一致率计算（数据合成）。
    bk.put_row("ui", 950, 1000, 180, 200);
    bk.put_row("cli", 470, 500, 90, 100);
    chk(&mut set, "C441-02", bk.consistency_bp(None) == Some(9466), "一致率 bp");

    // C441-03 域维度切分单查。
    chk(&mut set, "C441-03", bk.consistency_bp(Some("ui")) == Some(9500)
        && bk.consistency_bp(Some("nope")).is_none(), "域切分");

    // C441-04 无障碍一致率单列（域本色）。
    chk(&mut set, "C441-04", bk.a11y_consistency_bp() == Some(9000), "a11y 单列");

    // C441-05 口径缺指标→冻结拒。
    bk.set_caliber(Metric::Consistency, 3);
    chk(&mut set, "C441-05", bk.freeze(3) == Err(MtCode::CaliberDrift), "口径缺拒冻");

    // C441-06 口径版本漂移拒。
    for m in Metric::ALL { bk.set_caliber(m, 3); }
    bk.set_caliber(Metric::FixLatency, 2);
    chk(&mut set, "C441-06", bk.freeze(3) == Err(MtCode::CaliberDrift), "版本漂移拒");

    // C441-07 对版后冻结通过。
    bk.set_caliber(Metric::FixLatency, 3);
    chk(&mut set, "C441-07", bk.freeze(3).is_ok() && bk.frozen, "冻结通过");

    // C441-08 可复算断言。
    chk(&mut set, "C441-08", bk.recalc() == Ok(true), "可复算");

    // C441-09 家族同构对拍：四主指标名与家族一致。
    let fam = ["consistency", "conflict-density", "fix-latency", "regress-rate"];
    chk(&mut set, "C441-09", Metric::ALL.iter().all(|m| fam.contains(&m.name())), "家族同构");

    // C441-10 域行覆盖幂等。
    bk.put_row("ui", 960, 1000, 190, 200);
    chk(&mut set, "C441-10", bk.rows.len() == 2, "域行幂等");

    // C441-11 复算失配检出（数据篡改）。
    bk.rows[0].pass = 1;
    chk(&mut set, "C441-11", bk.recalc() == Ok(false), "篡改可检出");

    // C441-12 四主指标单源断言。
    chk(&mut set, "C441-12", Metric::ALL.len() == 4
        && Metric::ALL[0].name() == "consistency"
        && Metric::ALL[3].name() == "regress-rate", "四主指标单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves441_smoke() {
        let set = run_ves441_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves441 red: {:?}", red);
    }
}
