//! VE-F0173 · 跨版本渲染回归门（VE-Test · 目标 360 行）
//!
//! 铁律：基线可钉住，指标可对比，越界可拦截，放行可审计。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8F3;
pub const XC_GATE_BLOCKED: u16 = SEG_BASE | 0x01;
pub const XC_METRIC_DRIFT: u16 = SEG_BASE | 0x02;
pub const XC_NO_BASELINE: u16 = SEG_BASE | 0x03;
pub const XC_BASELINE_LOCKED: u16 = SEG_BASE | 0x04;
pub const XC_EMPTY_SNAPSHOT: u16 = SEG_BASE | 0x05;
pub const XC_GATE_TRIPPED: u16 = SEG_BASE | 0x06;

/// 门禁裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateVerdict {
    Pass,
    Warn,
    Block,
}

/// 版本指标快照。
#[derive(Clone, Copy)]
pub struct MetricSnapshot {
    pub frame_us: u32,
    pub mem_bytes: u32,
    pub draw_calls: u32,
    pub image_hash: u64,
}

/// 基线：钉住的上一版本度量。
#[derive(Clone, Copy)]
pub struct Baseline {
    pub frame_us: u32,
    pub mem_bytes: u32,
    pub draw_calls: u32,
    pub image_hash: u64,
    pub locked: bool,
}

/// 越界容忍（基点）。
#[derive(Clone, Copy)]
pub struct Tolerance {
    pub frame_bps: u32,
    pub mem_bps: u32,
    pub draw_bps: u32,
}

impl Tolerance {
    pub const fn strict() -> Self {
        Self { frame_bps: 0, mem_bps: 0, draw_bps: 0 }
    }
    pub const fn loose() -> Self {
        Self { frame_bps: 1_000, mem_bps: 500, draw_bps: 200 }
    }
}

/// 单次门禁记录。
#[derive(Clone, Copy)]
pub struct GateRecord {
    pub verdict: GateVerdict,
    pub frame_delta_bps: i32,
    pub mem_delta_bps: i32,
    pub code: u16,
}

/// 跨版本渲染回归门。
pub struct RegressionGate {
    baseline: Option<Baseline>,
    tol: Tolerance,
    records: Vec<GateRecord>,
    blocks: u32,
    warns: u32,
    passes: u32,
    image_lock: bool,
    last_code: u16,
}

impl RegressionGate {
    pub fn new(tol: Tolerance) -> Self {
        Self {
            baseline: None,
            tol,
            records: Vec::new(),
            blocks: 0,
            warns: 0,
            passes: 0,
            image_lock: true,
            last_code: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 钉基线；已锁则拒改。
    pub fn set_baseline(&mut self, snap: MetricSnapshot) -> Result<u32, u16> {
        if self.baseline.map(|b| b.locked).unwrap_or(false) {
            self.last_code = XC_BASELINE_LOCKED;
            return Err(XC_BASELINE_LOCKED);
        }
        self.baseline = Some(Baseline {
            frame_us: snap.frame_us,
            mem_bytes: snap.mem_bytes,
            draw_calls: snap.draw_calls,
            image_hash: snap.image_hash,
            locked: false,
        });
        self.last_code = 0;
        Ok(snap.frame_us)
    }

    /// 锁定基线，后续不可改。
    pub fn lock_baseline(&mut self) -> Result<bool, u16> {
        match self.baseline.as_mut() {
            Some(b) => {
                b.locked = true;
                self.last_code = 0;
                Ok(true)
            }
            None => {
                self.last_code = XC_NO_BASELINE;
                Err(XC_NO_BASELINE)
            }
        }
    }

    pub fn has_baseline(&self) -> bool {
        self.baseline.is_some()
    }

    /// 指标相对基线的漂移（基点，带符号）。
    fn drift_bps(cur: u32, base: u32) -> i32 {
        if base == 0 {
            return if cur == 0 { 0 } else { 10_000 };
        }
        ((cur as i64 - base as i64) * 10_000 / base as i64) as i32
    }

    /// 跑门禁：无基线/空快照/指标越界/图像漂移 → 对应裁决。
    pub fn evaluate(&mut self, snap: MetricSnapshot) -> Result<GateVerdict, u16> {
        let base = match self.baseline {
            Some(b) => b,
            None => {
                self.last_code = XC_NO_BASELINE;
                return Err(XC_NO_BASELINE);
            }
        };
        if snap.frame_us == 0 && snap.mem_bytes == 0 && snap.draw_calls == 0 {
            self.last_code = XC_EMPTY_SNAPSHOT;
            return Err(XC_EMPTY_SNAPSHOT);
        }
        let fd = Self::drift_bps(snap.frame_us, base.frame_us);
        let md = Self::drift_bps(snap.mem_bytes, base.mem_bytes);
        let dd = Self::drift_bps(snap.draw_calls, base.draw_calls);
        let frame_over = fd > self.tol.frame_bps as i32;
        let mem_over = md > self.tol.mem_bps as i32;
        let draw_over = dd > self.tol.draw_bps as i32;
        let image_drift = self.image_lock && snap.image_hash != base.image_hash;
        let (verdict, code) = if image_drift || (frame_over && mem_over) {
            (GateVerdict::Block, XC_GATE_BLOCKED)
        } else if frame_over || mem_over || draw_over {
            (GateVerdict::Warn, XC_METRIC_DRIFT)
        } else {
            (GateVerdict::Pass, 0)
        };
        match verdict {
            GateVerdict::Block => self.blocks += 1,
            GateVerdict::Warn => self.warns += 1,
            GateVerdict::Pass => self.passes += 1,
        }
        self.records.push(GateRecord {
            verdict,
            frame_delta_bps: fd,
            mem_delta_bps: md,
            code,
        });
        self.last_code = code;
        Ok(verdict)
    }

    /// 图像 hash 锁开关：关掉后仅比指标。
    pub fn set_image_lock(&mut self, on: bool) {
        self.image_lock = on;
    }

    pub fn blocks(&self) -> u32 {
        self.blocks
    }

    pub fn warns(&self) -> u32 {
        self.warns
    }

    pub fn passes(&self) -> u32 {
        self.passes
    }

    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// 最近一次记录。
    pub fn last_record(&self) -> Option<&GateRecord> {
        self.records.last()
    }

    /// 最近一次是否放行（Pass 或 Warn）。
    pub fn last_released(&self) -> bool {
        matches!(self.last_record(), Some(r) if r.verdict != GateVerdict::Block)
    }

    /// 历史里是否出现过拦截。
    pub fn ever_blocked(&self) -> bool {
        self.blocks > 0
    }

    /// 放行率（基点）：Block 计入分母。
    pub fn release_rate_bps(&self) -> u32 {
        let total = self.blocks + self.warns + self.passes;
        if total == 0 {
            return 10_000;
        }
        (((self.warns + self.passes) * 10_000) / total) as u32
    }

    /// 重估容限。
    pub fn set_tolerance(&mut self, tol: Tolerance) {
        self.tol = tol;
    }

    /// 清空历史记录（基线保留）。
    pub fn reset_records(&mut self) {
        self.records.clear();
        self.blocks = 0;
        self.warns = 0;
        self.passes = 0;
        self.last_code = 0;
    }

    /// 汇总闸门：全绿才 Ok(true)。
    pub fn summary_green(&self) -> bool {
        self.baseline.is_some() && self.blocks == 0 && self.warns == 0
    }
}

pub fn run_vea173_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("vea173_regress");
    let snap = MetricSnapshot { frame_us: 1000, mem_bytes: 2000, draw_calls: 100, image_hash: 0xC0FFEE };
    {
        let g = RegressionGate::new(Tolerance::strict());
        chk(&mut set, "C173-01", !g.has_baseline() && g.blocks() == 0, "err");
    }
    {
        let mut g = RegressionGate::new(Tolerance::strict());
        let r = g.evaluate(snap);
        chk(&mut set, "C173-02", r.err() == Some(XC_NO_BASELINE), "err");
    }
    {
        let mut g = RegressionGate::new(Tolerance::strict());
        let r = g.set_baseline(snap);
        chk(&mut set, "C173-03", r == Ok(1000) && g.has_baseline(), "err");
    }
    {
        let mut g = RegressionGate::new(Tolerance::strict());
        g.set_baseline(snap);
        g.lock_baseline();
        let r = g.set_baseline(snap);
        chk(&mut set, "C173-04", r.err() == Some(XC_BASELINE_LOCKED), "err");
    }
    {
        let mut g = RegressionGate::new(Tolerance::strict());
        g.set_baseline(snap);
        let v = g.evaluate(snap);
        chk(&mut set, "C173-05", v == Ok(GateVerdict::Pass) && g.passes() == 1, "err");
    }
    {
        let mut g = RegressionGate::new(Tolerance::strict());
        g.set_baseline(snap);
        let worse = MetricSnapshot { frame_us: 2000, mem_bytes: 2000, draw_calls: 100, image_hash: 0xC0FFEE };
        let v = g.evaluate(worse);
        chk(&mut set, "C173-06", v == Ok(GateVerdict::Warn) && g.warns() == 1, "err");
    }
    {
        let mut g = RegressionGate::new(Tolerance::strict());
        g.set_baseline(snap);
        let drifted = MetricSnapshot { frame_us: 1000, mem_bytes: 2000, draw_calls: 100, image_hash: 0xBAD };
        let v = g.evaluate(drifted);
        chk(&mut set, "C173-07", v == Ok(GateVerdict::Block) && g.ever_blocked(), "err");
    }
    {
        let mut g = RegressionGate::new(Tolerance::strict());
        g.set_baseline(snap);
        let empty = MetricSnapshot { frame_us: 0, mem_bytes: 0, draw_calls: 0, image_hash: 0 };
        let v = g.evaluate(empty);
        chk(&mut set, "C173-08", v.err() == Some(XC_EMPTY_SNAPSHOT), "err");
    }
    {
        let mut g = RegressionGate::new(Tolerance::strict());
        g.set_baseline(snap);
        g.evaluate(snap);
        chk(&mut set, "C173-09", g.last_released() && g.release_rate_bps() == 10_000, "err");
    }
    {
        let mut g = RegressionGate::new(Tolerance::strict());
        g.set_baseline(snap);
        let drifted = MetricSnapshot { frame_us: 1000, mem_bytes: 2000, draw_calls: 100, image_hash: 0xBAD };
        g.evaluate(drifted);
        chk(&mut set, "C173-10", !g.last_released() && !g.summary_green(), "err");
    }
    {
        let mut g = RegressionGate::new(Tolerance::loose());
        g.set_baseline(snap);
        g.set_image_lock(false);
        let drifted = MetricSnapshot { frame_us: 1050, mem_bytes: 2000, draw_calls: 100, image_hash: 0xBAD };
        let v = g.evaluate(drifted);
        chk(&mut set, "C173-11", v == Ok(GateVerdict::Pass), "err");
    }
    {
        let mut g = RegressionGate::new(Tolerance::strict());
        g.set_baseline(snap);
        g.evaluate(snap);
        g.reset_records();
        chk(&mut set, "C173-12", g.record_count() == 0 && g.summary_green(), "err");
    }
    set
}
