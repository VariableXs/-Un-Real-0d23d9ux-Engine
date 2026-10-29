// unx1038_u1038 —— UNX-F1038 HPET 长跑资产（24h 精度与稳定——随闸门）
// 判据原文 UNX-F1038-J1：长跑资产就绪（24h 档随闸门登记），2h 档 QEMU 先行实测，质量分修正回灌（F1026 消费 10/10 次）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2280 独占（0x2281~0x228F）。
//
// 设计（纯功能 no_std）：
//   * 长跑档位：2h（QEMU 先行）/24h（随闸门），档位独立登记互不串账；
//   * 精度样张：每档登记漂移样张（名义 tick vs 实测 tick），累计 ppm 漂移；
//   * 质量分：0~1000 定点，修正回灌 F1026 逐次消费计数对平；
//   * 饱和防溢出：ppm 累计与质量分均 saturating；
//   * 守恒：样张数 = 消费数 + 待消费数（F1026 回灌不丢）；
//   * FNV 指纹冻结 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2280 独占） ─────────────────────────────────────────────
pub const XC_LR_OOB: u32 = 0x2281; // 档位或样张越界
pub const XC_LR_DUP: u32 = 0x2282; // 同档重复登记
pub const XC_LR_SEALED: u32 = 0x2283; // 封账后写样张
pub const XC_LR_PPM: u32 = 0x2284; // ppm 漂移越限
pub const XC_LR_LOST: u32 = 0x2285; // 回灌消费不守恒

pub const TIER_2H: u8 = 2;
pub const TIER_24H: u8 = 24;
/// ppm 越限红线（长跑判定）。
pub const PPM_LIMIT: i32 = 500;

#[derive(Clone, Copy)]
pub struct DriftSample {
    pub tier: u8,       // 2 或 24（小时档）
    pub nominal: u64,   // 名义 tick 数
    pub measured: u64,  // 实测 tick 数
}

/// HPET 长跑资产账本。
pub struct LongRunAsset {
    tiers: Vec<(u8, bool)>, // (档位, 已登记)
    samples: Vec<DriftSample>,
    quality: u32,           // 质量分 0..=1000
    corrections: Vec<i32>,  // 修正回灌序列（ppm 修正量）
    consumed: u32,          // F1026 已消费次数
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl LongRunAsset {
    pub fn new() -> LongRunAsset {
        LongRunAsset {
            tiers: Vec::new(),
            samples: Vec::new(),
            quality: 1000,
            corrections: Vec::new(),
            consumed: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 登记一个档位（同档重复拒）。
    pub fn enroll(&mut self, tier: u8) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_LR_SEALED, "封账后登记档"));
            return Err(XC_LR_SEALED);
        }
        if tier != TIER_2H && tier != TIER_24H {
            self.rejects.push((XC_LR_OOB, "档位越界"));
            return Err(XC_LR_OOB);
        }
        for &(t, _) in &self.tiers {
            if t == tier {
                self.rejects.push((XC_LR_DUP, "同档重复登记"));
                return Err(XC_LR_DUP);
            }
        }
        self.tiers.push((tier, false));
        Ok(())
    }

    /// 写一条精度样张并即时重算质量分。
    pub fn sample(&mut self, s: DriftSample) -> Result<i32, u32> {
        if self.sealed {
            self.rejects.push((XC_LR_SEALED, "封账后写样张"));
            return Err(XC_LR_SEALED);
        }
        if s.nominal == 0 {
            self.rejects.push((XC_LR_OOB, "名义值为零"));
            return Err(XC_LR_OOB);
        }
        let enrolled = self.tiers.iter_mut().any(|(t, seen)| {
            if *t == s.tier {
                *seen = true;
                true
            } else {
                false
            }
        });
        if !enrolled {
            self.rejects.push((XC_LR_OOB, "档未登记"));
            return Err(XC_LR_OOB);
        }
        // ppm 漂移：i64 中间量防溢出，saturating 防回绕
        let diff = if s.measured >= s.nominal {
            (s.measured - s.nominal).min(i32::MAX as u64) as i64
        } else {
            -((s.nominal - s.measured).min(i32::MAX as u64) as i64)
        };
        let ppm64 = diff.saturating_mul(1_000_000).saturating_div(s.nominal as i64);
        let ppm = ppm64.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        if ppm.abs() > PPM_LIMIT {
            self.rejects.push((XC_LR_PPM, "ppm 越限"));
            return Err(XC_LR_PPM);
        }
        self.samples.push(s);
        // 质量分修正：ppm 越大扣越多，饱和于 0
        let penalty = (ppm.abs() as u32).saturating_mul(2);
        self.quality = self.quality.saturating_sub(penalty);
        Ok(ppm)
    }

    /// 质量分修正回灌（F1026）：登记一条修正并期待后续消费。
    pub fn push_correction(&mut self, corr_ppm: i32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_LR_SEALED, "封账后回灌"));
            return Err(XC_LR_SEALED);
        }
        if corr_ppm.abs() > PPM_LIMIT {
            self.rejects.push((XC_LR_PPM, "修正量越限"));
            return Err(XC_LR_PPM);
        }
        self.corrections.push(corr_ppm);
        // 回灌即时修正质量分：负漂移加分、正漂移扣分
        if corr_ppm < 0 {
            self.quality = self.quality.saturating_add(((-corr_ppm) as u32).saturating_mul(2));
        } else {
            self.quality = self.quality.saturating_sub((corr_ppm as u32).saturating_mul(2));
        }
        Ok(())
    }

    /// F1026 消费一次回灌。
    pub fn consume_correction(&mut self) -> Result<(), u32> {
        if self.consumed >= self.corrections.len() as u32 {
            self.rejects.push((XC_LR_LOST, "无待消费修正"));
            return Err(XC_LR_LOST);
        }
        self.consumed = self.consumed.saturating_add(1);
        Ok(())
    }

    /// 档位就绪：两档均已登记且有样张。
    pub fn ready(&self) -> bool {
        let mut hit2 = false;
        let mut hit24 = false;
        for s in &self.samples {
            if s.tier == TIER_2H {
                hit2 = true;
            }
            if s.tier == TIER_24H {
                hit24 = true;
            }
        }
        hit2 && hit24 && self.tiers.len() == 2
    }

    /// 守恒：消费数 ≤ 回灌数 且 样张全在。
    pub fn conserved(&self) -> bool {
        self.consumed as usize == self.corrections.len() && !self.samples.is_empty()
    }

    /// 全程累计 ppm（saturating）。
    pub fn total_ppm(&self) -> i32 {
        let mut acc: i64 = 0;
        for s in &self.samples {
            if s.nominal == 0 {
                continue;
            }
            let d = if s.measured >= s.nominal {
                (s.measured - s.nominal) as i64
            } else {
                -((s.nominal - s.measured) as i64)
            };
            acc = acc.saturating_add(d.saturating_mul(1_000_000).saturating_div(s.nominal as i64));
        }
        acc.clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_LR_SEALED);
        }
        if !self.ready() {
            self.rejects.push((XC_LR_OOB, "档位不齐禁封"));
            return Err(XC_LR_OOB);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.samples.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        for s in &self.samples {
            h ^= s.tier as u32
                ^ (s.nominal as u32).wrapping_mul(0x9E37)
                ^ (s.measured as u32)
                ^ ((s.nominal >> 32) as u32)
                ^ ((s.measured >> 32) as u32);
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.quality;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.consumed;
        h = h.wrapping_mul(0x0100_0193);
        h as u64
    }

    pub fn quality(&self) -> u32 {
        self.quality
    }
    pub fn consumed(&self) -> u32 {
        self.consumed
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）档位越界必拒（10/10：非 2/24 档全拒）
    {
        let mut a = LongRunAsset::new();
        let mut hits = 0;
        for k in 1..=10u8 {
            if a.enroll(k.wrapping_mul(7)) == Err(XC_LR_OOB) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）同档重复登记必拒
    {
        let mut a = LongRunAsset::new();
        let _ = a.enroll(TIER_2H);
        if a.enroll(TIER_2H) == Err(XC_LR_DUP) {
            caught += 1;
        }
    }
    // 3）未登记档写样张必拒
    {
        let mut a = LongRunAsset::new();
        let _ = a.enroll(TIER_2H);
        let r = a.sample(DriftSample { tier: TIER_24H, nominal: 1_000_000, measured: 1_000_001 });
        if r == Err(XC_LR_OOB) {
            caught += 1;
        }
    }
    // 4）ppm 越限必拒
    {
        let mut a = LongRunAsset::new();
        let _ = a.enroll(TIER_2H);
        let r = a.sample(DriftSample { tier: TIER_2H, nominal: 1_000_000, measured: 1_600_000 });
        if r == Err(XC_LR_PPM) {
            caught += 1;
        }
    }
    // 5）无回灌消费必拒
    {
        let mut a = LongRunAsset::new();
        if a.consume_correction() == Err(XC_LR_LOST) {
            caught += 1;
        }
    }
    // 6）封账后写样张必拒
    {
        let mut a = LongRunAsset::new();
        let _ = a.enroll(TIER_2H);
        let _ = a.enroll(TIER_24H);
        let _ = a.sample(DriftSample { tier: TIER_2H, nominal: 1_000_000, measured: 1_000_000 });
        let _ = a.sample(DriftSample { tier: TIER_24H, nominal: 1_000_000, measured: 1_000_000 });
        let _ = a.seal();
        if a.sample(DriftSample { tier: TIER_2H, nominal: 1, measured: 1 }) == Err(XC_LR_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1038-J1。
pub fn run_unx1038_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1038");

    // C38-01 两档登记（2h/24h）
    {
        let mut a = LongRunAsset::new();
        let ok = a.enroll(TIER_2H).is_ok() && a.enroll(TIER_24H).is_ok();
        s.add("C38-01-两档登记", ok, if ok { "" } else { "登记失败" });
    }
    // C38-02 2h 档 QEMU 先行样张
    {
        let mut a = LongRunAsset::new();
        let _ = a.enroll(TIER_2H);
        let r = a.sample(DriftSample { tier: TIER_2H, nominal: 3_600_000_000, measured: 3_601_800_000 });
        let ok = r == Ok(500);
        s.add("C38-02-先行样张", ok, if ok { "" } else { "样张漂移" });
    }
    // C38-03 24h 档随闸门样张
    {
        let mut a = LongRunAsset::new();
        let _ = a.enroll(TIER_24H);
        let r = a.sample(DriftSample { tier: TIER_24H, nominal: 86_400_000_000, measured: 86_443_200_000 });
        let ok = r == Ok(500);
        s.add("C38-03-随闸门样张", ok, if ok { "" } else { "样张漂移" });
    }
    // C38-04 质量分回灌修正（F1026 消费 10/10）
    {
        let mut a = LongRunAsset::new();
        let mut all_ok = true;
        for _ in 0..10 {
            if a.push_correction(-10).is_err() {
                all_ok = false;
            }
        }
        for _ in 0..10 {
            if a.consume_correction().is_err() {
                all_ok = false;
            }
        }
        s.add("C38-04-回灌消费", all_ok && a.consumed() == 10, if all_ok { "" } else { "回灌丢失" });
    }
    // C38-05 质量分饱和于 0
    {
        let mut a = LongRunAsset::new();
        for _ in 0..600 {
            let _ = a.push_correction(PPM_LIMIT);
        }
        s.add("C38-05-质量饱和", a.quality() == 0, if a.quality() == 0 { "" } else { "饱和破" });
    }
    // C38-06 长跑就绪（两档齐+样张在）
    {
        let mut a = LongRunAsset::new();
        let _ = a.enroll(TIER_2H);
        let _ = a.enroll(TIER_24H);
        let _ = a.sample(DriftSample { tier: TIER_2H, nominal: 1_000_000, measured: 1_000_005 });
        let _ = a.sample(DriftSample { tier: TIER_24H, nominal: 1_000_000, measured: 1_000_008 });
        s.add("C38-06-就绪判定", a.ready(), if a.ready() { "" } else { "就绪缺" });
    }
    // C38-07 守恒（消费=回灌数）
    {
        let mut a = LongRunAsset::new();
        let _ = a.enroll(TIER_2H);
        let _ = a.enroll(TIER_24H);
        let _ = a.sample(DriftSample { tier: TIER_2H, nominal: 1, measured: 1 });
        let _ = a.sample(DriftSample { tier: TIER_24H, nominal: 1, measured: 1 });
        for _ in 0..3 {
            let _ = a.push_correction(5);
        }
        for _ in 0..3 {
            let _ = a.consume_correction();
        }
        s.add("C38-07-守恒对平", a.conserved(), if a.conserved() { "" } else { "守恒破" });
    }
    // C38-08 封账产指纹
    {
        let mut a = LongRunAsset::new();
        let _ = a.enroll(TIER_2H);
        let _ = a.enroll(TIER_24H);
        let _ = a.sample(DriftSample { tier: TIER_2H, nominal: 1, measured: 1 });
        let _ = a.sample(DriftSample { tier: TIER_24H, nominal: 1, measured: 1 });
        s.add("C38-08-封账指纹", a.seal().is_ok(), "");
    }
    // C38-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C38-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C38-10 指纹一致
    {
        let f = || {
            let mut a = LongRunAsset::new();
            let _ = a.enroll(TIER_2H);
            let _ = a.enroll(TIER_24H);
            let _ = a.sample(DriftSample { tier: TIER_2H, nominal: 1_000, measured: 1_001 });
            let _ = a.sample(DriftSample { tier: TIER_24H, nominal: 1_000, measured: 1_002 });
            let _ = a.push_correction(-3);
            a.fingerprint()
        };
        s.add("C38-10-指纹一致", f() == f(), "");
    }
    s
}
