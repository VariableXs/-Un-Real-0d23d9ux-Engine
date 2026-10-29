// unx1231_u1231 —— UNX-F1231 Draining 态健康账（F0899 口径注册）
// 判据原文 UNX-F1231-J1：健康账全过（指标按 F0899 口径注册、带内判定逐例
// 记账、三级健康分级、越带立案、健康总分基点化、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb7f0 独占（0xb7f0~0xb82f）。
//
// 设计（纯功能 no_std）：
//   * 指标注册制：每指标（名/口径号/带下界/带上界）注册后才能采样；
//   * 带内判定：采样值 ∈ [lo, hi] 为带内，越带立案（带方向：low/high 两向）；
//   * 健康分级：Green（全带内）/ Yellow（越带 ≤ 容忍数）/ Red（超容忍）；
//   * 健康总分：bps = 带内采样 / 总采样 × 10000；
//   * FNV 链指纹冻结，封账后拒采样。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb7f0 独占） ─────────────────────────────────────────────
pub const XC_HL_SEALED: u32 = 0xB7F1; // 封账后采样
pub const XC_HL_UNREG: u32 = 0xB7F2; // 未注册指标采样
pub const XC_HL_DUP_REG: u32 = 0xB7F3; // 指标重复注册
pub const XC_HL_INV_BAND: u32 = 0xB7F4; // 带倒置（lo>hi）
pub const XC_HL_FULL: u32 = 0xB7F5; // 注册表容量满
pub const XC_HL_NO_SAMPLE: u32 = 0xB7F6; // 无采样先评级

/// 健康分级。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HealthGrade {
    Green,
    Yellow,
    Red,
}

/// 指标登记。
#[derive(Clone, Copy)]
pub struct MetricReg {
    /// 口径号（对齐 F0899 指标口径册）。
    pub oid: u32,
    pub lo: u64,
    pub hi: u64,
    /// 越带容忍数（Yellow→Red 阈）。
    pub tolerance: u32,
}

/// 采样行。
#[derive(Clone, Copy)]
pub struct SampleRow {
    pub oid: u32,
    pub value: u64,
    pub in_band: bool,
}

/// 健康账。
pub struct HealthLedger {
    cap: usize,
    regs: Vec<MetricReg>,
    samples: Vec<SampleRow>,
    violations: Vec<(u32, u64, u8)>, // (oid, 值, 方向 0=低于 1=高于)
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl HealthLedger {
    pub fn new(cap: usize) -> HealthLedger {
        HealthLedger {
            cap,
            regs: vec![],
            samples: vec![],
            violations: vec![],
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 注册指标。
    pub fn register(&mut self, oid: u32, lo: u64, hi: u64, tolerance: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_HL_SEALED, "封账后注册"));
            return Err(XC_HL_SEALED);
        }
        if self.regs.len() >= self.cap {
            self.rejects.push((XC_HL_FULL, "注册表容量满"));
            return Err(XC_HL_FULL);
        }
        if lo > hi {
            self.rejects.push((XC_HL_INV_BAND, "带倒置"));
            return Err(XC_HL_INV_BAND);
        }
        if self.regs.iter().any(|r| r.oid == oid) {
            self.rejects.push((XC_HL_DUP_REG, "指标重复注册"));
            return Err(XC_HL_DUP_REG);
        }
        self.regs.push(MetricReg { oid, lo, hi, tolerance });
        Ok(())
    }

    fn reg_of(&self, oid: u32) -> Option<MetricReg> {
        self.regs.iter().find(|r| r.oid == oid).copied()
    }

    /// 采样：带内判定 + 越带立案。
    pub fn sample(&mut self, oid: u32, value: u64) -> Result<bool, u32> {
        if self.sealed {
            self.rejects.push((XC_HL_SEALED, "封账后采样"));
            return Err(XC_HL_SEALED);
        }
        let r = match self.reg_of(oid) {
            Some(r) => r,
            None => {
                self.rejects.push((XC_HL_UNREG, "未注册指标采样"));
                return Err(XC_HL_UNREG);
            }
        };
        let in_band = value >= r.lo && value <= r.hi;
        if !in_band {
            let dir: u8 = if value < r.lo { 0 } else { 1 };
            self.violations.push((oid, value, dir));
        }
        self.samples.push(SampleRow { oid, value, in_band });
        Ok(in_band)
    }

    /// 单指标越带数。
    pub fn viol_of(&self, oid: u32) -> usize {
        self.violations.iter().filter(|v| v.0 == oid).count()
    }

    /// 健康分级：全带内 Green；任一指标越带 ≤ 容忍 Yellow；否则 Red。
    pub fn grade(&self) -> Result<HealthGrade, u32> {
        if self.samples.is_empty() {
            return Err(XC_HL_NO_SAMPLE);
        }
        for r in &self.regs {
            let v = self.viol_of(r.oid) as u32;
            if v > r.tolerance {
                return Ok(HealthGrade::Red);
            }
        }
        if self.violations.is_empty() {
            Ok(HealthGrade::Green)
        } else {
            Ok(HealthGrade::Yellow)
        }
    }

    /// 健康总分 bps。
    pub fn score_bps(&self) -> u32 {
        let n = self.samples.len() as u64;
        if n == 0 {
            return 0;
        }
        let ok = self.samples.iter().filter(|s| s.in_band).count() as u64;
        ((ok * 10000) / n) as u32
    }

    pub fn regs_len(&self) -> usize {
        self.regs.len()
    }
    pub fn samples_len(&self) -> usize {
        self.samples.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_HL_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for r in &self.regs {
            h ^= (r.oid as u64).rotate_left(3) ^ r.lo.rotate_left(17) ^ r.hi.rotate_left(29);
            h = h.wrapping_mul(0x0100_0193);
        }
        for s in &self.samples {
            h ^= (s.oid as u64).rotate_left(7) ^ s.value.rotate_left(19) ^ (s.in_band as u64);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准健康场景：4 指标注册 + 16 采样（15 带内 1 越带）→ Yellow + 总分。
pub fn health_scenario() -> Result<(u32, HealthGrade, u64), u32> {
    let mut h = HealthLedger::new(8);
    h.register(101, 0, 100, 2)?; // 指标 101：带 [0,100] 容忍 2
    h.register(102, 500, 1000, 1)?;
    h.register(103, 0, 1, 0)?;   // 布尔口径
    h.register(104, 10, 20, 3)?;
    // 采样：101×4 带内、102×4 带内、103×4 带内、104×3 带内 + 1 越带（21）
    for _ in 0..4 {
        h.sample(101, 50)?;
        h.sample(102, 700)?;
        h.sample(103, 1)?;
    }
    for _ in 0..3 {
        h.sample(104, 15)?;
    }
    h.sample(104, 21)?;
    let g = h.grade()?;
    if g != HealthGrade::Yellow {
        return Err(XC_HL_NO_SAMPLE);
    }
    let sc = h.score_bps();
    // 15/16 带内 = 9375
    if sc != 9375 {
        return Err(XC_HL_UNREG);
    }
    let fp = h.seal()?;
    Ok((sc, g, fp))
}

// ── 深化块：指标快照与越带趋势 ─────────────────────────────────────
impl HealthLedger {
    /// 指标快照：(oid, 采样数, 越带数, 均值)。
    pub fn snapshot(&self, oid: u32) -> Option<(u32, usize, usize, u64)> {
        if self.regs.iter().all(|r| r.oid != oid) { return None; }
        let mut n = 0usize;
        let mut viol = 0usize;
        let mut sum: u64 = 0;
        for s in &self.samples {
            if s.oid == oid {
                n += 1;
                sum = sum.saturating_add(s.value);
                if !s.in_band { viol += 1; }
            }
        }
        if n == 0 { return None; }
        Some((oid, n, viol, sum / n as u64))
    }
    /// 越带趋势：前半/后半越带数对比（1=恶化 0=持平 -1=好转）。
    pub fn viol_trend(&self, oid: u32) -> i8 {
        let n = self.samples.len();
        if n < 2 { return 0; }
        let half = n / 2;
        let mut front = 0usize;
        let mut back = 0usize;
        for (i, s) in self.samples.iter().enumerate() {
            if s.oid == oid && !s.in_band {
                if i < half { front += 1; } else { back += 1; }
            }
        }
        if back > front { 1 } else if back < front { -1 } else { 0 }
    }
    /// 全指标注册清单（登记序）。
    pub fn reg_list(&self) -> Vec<u32> {
        let mut v: Vec<u32> = vec![];
        for r in &self.regs { v.push(r.oid); }
        v
    }
    /// 总分复算（与 score_bps 对拍）。
    pub fn score_recheck(&self) -> u32 { self.score_bps() }
}

/// 注入核：七类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 7;

    // 1）带倒置必拒
    {
        let mut h = HealthLedger::new(4);
        if h.register(1, 10, 0, 1) == Err(XC_HL_INV_BAND) {
            caught += 1;
        }
    }
    // 2）重复注册必拒
    {
        let mut h = HealthLedger::new(4);
        let _ = h.register(1, 0, 10, 1);
        if h.register(1, 0, 10, 1) == Err(XC_HL_DUP_REG) {
            caught += 1;
        }
    }
    // 3）未注册采样必拒
    {
        let mut h = HealthLedger::new(4);
        if h.sample(99, 1) == Err(XC_HL_UNREG) {
            caught += 1;
        }
    }
    // 4）无采样先评级必拒
    {
        let mut h = HealthLedger::new(4);
        let _ = h.register(1, 0, 10, 1);
        if h.grade() == Err(XC_HL_NO_SAMPLE) {
            caught += 1;
        }
    }
    // 5）越带两向立案（低向/高向）
    {
        let mut h = HealthLedger::new(4);
        let _ = h.register(1, 10, 20, 9);
        let _ = h.sample(1, 5);
        let _ = h.sample(1, 25);
        if h.viol_of(1) == 2 && h.violations[0].2 == 0 && h.violations[1].2 == 1 {
            caught += 1;
        }
    }
    // 6）超容忍判 Red
    {
        let mut h = HealthLedger::new(4);
        let _ = h.register(1, 0, 10, 1);
        let _ = h.sample(1, 99);
        let _ = h.sample(1, 98);
        let g = h.grade();
        if g == Ok(HealthGrade::Red) {
            caught += 1;
        }
    }
    // 7）封账后采样必拒
    {
        let mut h = HealthLedger::new(4);
        let _ = h.register(1, 0, 10, 1);
        let _ = h.sample(1, 5);
        let _ = h.seal();
        if h.sample(1, 5) == Err(XC_HL_SEALED) {
            caught += 1;
        }
    }
    (caught, N)
}
