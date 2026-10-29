// unx1192_u1192 —— UNX-F1192 亲和面健康账（F0899 口径注册）
// 判据原文 UNX-F1192-J1：健康账全过（指标按 F0899 口径注册、带内判定逐例
// 记账、三级分级、越带立案、健康总分基点化、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x8e30 独占（0x8e30~0x8e6f）。
//
// 设计（纯功能 no_std）：
//   * 指标注册制：迁移延迟/公平度/约束违例三口径；
//   * 带内判定 + 两向越带立案 + 三级分级（Green/Yellow/Red）；
//   * 总分 bps = 带内采样/总采样 ×10000；
//   * FNV 链指纹冻结，封账后拒采样。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x8e30 独占） ─────────────────────────────────────────────
pub const XC_AH_SEALED: u32 = 0x8E31; // 封账后采样
pub const XC_AH_UNREG: u32 = 0x8E32; // 未注册指标采样
pub const XC_AH_DUP: u32 = 0x8E33; // 指标重复注册
pub const XC_AH_INV_BAND: u32 = 0x8E34; // 带倒置
pub const XC_AH_NO_SAMPLE: u32 = 0x8E35; // 无采样先评级

/// 分级。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AhGrade {
    Green,
    Yellow,
    Red,
}

/// 指标登记。
#[derive(Clone, Copy)]
pub struct AhReg {
    pub oid: u32,
    pub lo: u64,
    pub hi: u64,
    pub tolerance: u32,
}

/// 采样行。
#[derive(Clone, Copy)]
pub struct AhSample {
    pub oid: u32,
    pub value: u64,
    pub in_band: bool,
}

/// 亲和面健康账。
pub struct AffHealth {
    regs: Vec<AhReg>,
    samples: Vec<AhSample>,
    violations: Vec<(u32, u64, u8)>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl AffHealth {
    pub fn new() -> AffHealth {
        AffHealth { regs: vec![], samples: vec![], violations: vec![], sealed: false, rejects: vec![] }
    }

    pub fn register(&mut self, oid: u32, lo: u64, hi: u64, tolerance: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_AH_SEALED, "封账后注册"));
            return Err(XC_AH_SEALED);
        }
        if lo > hi {
            self.rejects.push((XC_AH_INV_BAND, "带倒置"));
            return Err(XC_AH_INV_BAND);
        }
        if self.regs.iter().any(|r| r.oid == oid) {
            self.rejects.push((XC_AH_DUP, "指标重复注册"));
            return Err(XC_AH_DUP);
        }
        self.regs.push(AhReg { oid, lo, hi, tolerance });
        Ok(())
    }

    pub fn sample(&mut self, oid: u32, value: u64) -> Result<bool, u32> {
        if self.sealed {
            self.rejects.push((XC_AH_SEALED, "封账后采样"));
            return Err(XC_AH_SEALED);
        }
        let r = match self.regs.iter().find(|r| r.oid == oid) {
            Some(r) => *r,
            None => {
                self.rejects.push((XC_AH_UNREG, "未注册指标采样"));
                return Err(XC_AH_UNREG);
            }
        };
        let in_band = value >= r.lo && value <= r.hi;
        if !in_band {
            let dir: u8 = if value < r.lo { 0 } else { 1 };
            self.violations.push((oid, value, dir));
        }
        self.samples.push(AhSample { oid, value, in_band });
        Ok(in_band)
    }

    pub fn viol_of(&self, oid: u32) -> usize {
        self.violations.iter().filter(|v| v.0 == oid).count()
    }

    /// 三级分级。
    pub fn grade(&mut self) -> Result<AhGrade, u32> {
        if self.samples.is_empty() {
            self.rejects.push((XC_AH_NO_SAMPLE, "无采样先评级"));
            return Err(XC_AH_NO_SAMPLE);
        }
        for r in &self.regs {
            if self.viol_of(r.oid) as u32 > r.tolerance {
                return Ok(AhGrade::Red);
            }
        }
        if self.violations.is_empty() {
            Ok(AhGrade::Green)
        } else {
            Ok(AhGrade::Yellow)
        }
    }

    /// 总分 bps。
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
            return Err(XC_AH_SEALED);
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

/// 标准场景：三口径注册 + 12 采样（11 带内 1 越带）→ Yellow。
pub fn aff_health_scenario() -> Result<(u32, AhGrade, u64), u32> {
    let mut h = AffHealth::new();
    h.register(201, 0, 6400, 2)?;   // 迁移延迟 ns
    h.register(202, 0, 300, 1)?;    // 基尼千分比
    h.register(203, 0, 0, 0)?;      // 约束违例数（必须 0）
    for _ in 0..4 {
        h.sample(201, 3200)?;
        h.sample(202, 150)?;
    }
    for _ in 0..4 {
        h.sample(203, 0)?;
    }
    h.sample(201, 7000)?; // 越带一例
    let g = h.grade()?;
    if g != AhGrade::Yellow {
        return Err(XC_AH_NO_SAMPLE);
    }
    // 11/12 带内 = 9166
    let sc = h.score_bps();
    if sc != 9166 {
        return Err(XC_AH_UNREG);
    }
    let fp = h.seal()?;
    Ok((sc, g, fp))
}

/// 注入核：六类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）带倒置必拒
    {
        let mut h = AffHealth::new();
        if h.register(1, 10, 0, 1) == Err(XC_AH_INV_BAND) {
            caught += 1;
        }
    }
    // 2）重复注册必拒
    {
        let mut h = AffHealth::new();
        let _ = h.register(1, 0, 10, 1);
        if h.register(1, 0, 10, 1) == Err(XC_AH_DUP) {
            caught += 1;
        }
    }
    // 3）未注册采样必拒
    {
        let mut h = AffHealth::new();
        if h.sample(9, 1) == Err(XC_AH_UNREG) {
            caught += 1;
        }
    }
    // 4）无采样先评级必拒
    {
        let mut h = AffHealth::new();
        let _ = h.register(1, 0, 10, 1);
        if h.grade() == Err(XC_AH_NO_SAMPLE) {
            caught += 1;
        }
    }
    // 5）超容忍 Red
    {
        let mut h = AffHealth::new();
        let _ = h.register(1, 0, 10, 0);
        let _ = h.sample(1, 99);
        if h.grade() == Ok(AhGrade::Red) {
            caught += 1;
        }
    }
    // 6）封账后采样必拒
    {
        let mut h = AffHealth::new();
        let _ = h.register(1, 0, 10, 1);
        let _ = h.sample(1, 5);
        let _ = h.seal();
        if h.sample(1, 5) == Err(XC_AH_SEALED) {
            caught += 1;
        }
    }
    (caught, N)
}
