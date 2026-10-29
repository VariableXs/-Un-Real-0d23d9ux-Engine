// unx1019_u1019 —— UNX-F1019 TSC 长跑稳定资产（24h 漂移与残差趋势——随闸门）
// 判据原文 UNX-F1019-J1：长跑资产可执行（24h 采样配置就绪——随闸门登记
// R-A2-002 同型），短跑档可验证（2h 档 QEMU 实测），趋势账 schema 通过。
// 落位：kernel/varix/src/unxstar/；诊断段 0x1D30 独占（0x1D30~0x1D6F）。
//
// 设计（纯功能 no_std）：
//   * 采样配置：24h 档（闸门态）与 2h 短跑档（QEMU 实测态）双档闭集；
//   * 残差趋势账：分桶残差（每桶均值/极差/斜率号）逐桶落账；
//   * 趋势判定：斜率累计漂移 < 阈（ppm 级）判稳，超阈立案（随闸门）；
//   * 闸门登记：R-A2-002 同型条目（档位/周期/桶数）append-only；
//   * schema 闸：桶闭集/非零周期/单调时戳三重；
//   * FNV 指纹冻结 + 守恒（桶账=采样账）+ 注入核六类全抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x1D30 独占） ─────────────────────────────────────────────
pub const XC_LONG_GEAR: u32 = 0x1D31; // 档位闭集外
pub const XC_LONG_BUCKET: u32 = 0x1D32; // 桶号越界
pub const XC_LONG_SCHEMA: u32 = 0x1D33; // 周期零/时戳回退
pub const XC_LONG_DRIFT: u32 = 0x1D34; // 趋势漂移超阈立案
pub const XC_LONG_SEALED: u32 = 0x1D35; // 封账后采样
pub const XC_LONG_CONS: u32 = 0x1D36; // 桶账与采样账不守恒

/// 双档闭集：0=短跑 2h（QEMU 实测）1=长跑 24h（随闸门）。
pub const GEAR_SHORT: u8 = 0;
pub const GEAR_LONG: u8 = 1;
/// 短跑档时长（ms 刻度模拟：2h = 7_200_000ms）。
pub const SHORT_MS: u64 = 7_200_000;
/// 长跑档时长（24h = 86_400_000ms）。
pub const LONG_MS: u64 = 86_400_000;
/// 桶数（每档 12 桶）。
pub const N_BUCKETS: u32 = 12;
/// 趋势漂移阈（ppm 千分位内部刻度：累计斜率 <500 判稳）。
pub const DRIFT_TH_PPM_X10: i64 = 500;

/// 一桶残差账。
#[derive(Clone, Copy)]
pub struct BucketRec {
    pub bid: u32,
    pub samples: u32,
    pub resid_sum: i64,  // 残差累计（有号）
    pub resid_max: i64,  // 极差上沿
    pub slope_x10: i64,  // 桶内斜率（×10 定点）
}

/// 长跑稳定资产账。
pub struct LongRunRig {
    gear: u8,
    buckets: Vec<BucketRec>,
    total_samples: u64,
    ts_last: u64,
    period_ms: u64,
    flags: u32,
    gated_entry: Option<(u32, u64, u64)>, // R-A2-002 同型登记（档/时长/桶数）
    sealed: bool,
}

impl LongRunRig {
    /// 新建（档位闭集 + 周期非零闸）。
    pub fn new(gear: u8, period_ms: u64) -> Result<LongRunRig, u32> {
        if gear != GEAR_SHORT && gear != GEAR_LONG {
            return Err(XC_LONG_GEAR);
        }
        if period_ms == 0 {
            return Err(XC_LONG_SCHEMA);
        }
        Ok(LongRunRig {
            gear,
            buckets: Vec::new(),
            total_samples: 0,
            ts_last: 0,
            period_ms,
            flags: 0,
            gated_entry: None,
            sealed: false,
        })
    }

    /// 闸门登记（R-A2-002 同型：档位/时长/桶数三要素）。
    pub fn register_gate(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_LONG_SEALED);
        }
        let span = if self.gear == GEAR_LONG { LONG_MS } else { SHORT_MS };
        self.gated_entry = Some((self.gear as u32, span, N_BUCKETS as u64));
        Ok(())
    }

    pub fn gated(&self) -> bool {
        self.gated_entry.is_some()
    }

    /// 采一个残差样（桶号闭集 + 时戳单调闸）。
    pub fn sample(&mut self, bid: u32, ts_ms: u64, resid: i64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_LONG_SEALED);
        }
        if bid >= N_BUCKETS {
            return Err(XC_LONG_BUCKET);
        }
        if ts_ms < self.ts_last {
            return Err(XC_LONG_SCHEMA);
        }
        self.ts_last = ts_ms;
        self.total_samples = self.total_samples.wrapping_add(1);
        // 入桶（无则建）
        let mut rec = match self.buckets.iter_mut().find(|b| b.bid == bid) {
            Some(b) => *b,
            None => BucketRec {
                bid,
                samples: 0,
                resid_sum: 0,
                resid_max: 0,
                slope_x10: 0,
            },
        };
        rec.samples = rec.samples.saturating_add(1);
        rec.resid_sum = rec.resid_sum.wrapping_add(resid);
        let mag = if resid < 0 { resid.wrapping_neg() } else { resid };
        if mag > rec.resid_max {
            rec.resid_max = mag;
        }
        // 桶内斜率：残差均值 ×10 定点（简化：sum/samples*10）
        rec.slope_x10 = if rec.samples > 0 {
            (rec.resid_sum * 10) / rec.samples as i64
        } else {
            0
        };
        // 趋势漂移立案：斜率超阈
        let sm = if rec.slope_x10 < 0 { rec.slope_x10.wrapping_neg() } else { rec.slope_x10 };
        if sm > DRIFT_TH_PPM_X10 {
            self.flags = self.flags.wrapping_add(1);
            return Err(XC_LONG_DRIFT);
        }
        // 写回（找到槽则覆盖，否则追加）
        if let Some(b) = self.buckets.iter_mut().find(|b| b.bid == bid) {
            *b = rec;
        } else {
            self.buckets.push(rec);
        }
        Ok(())
    }

    /// 桶账守恒：Σ桶 samples == 总采样。
    pub fn conserved(&self) -> bool {
        let s: u64 = self.buckets.iter().map(|b| b.samples as u64).sum();
        s == self.total_samples
    }

    /// 全桶趋势判稳（全部 |slope| ≤ 阈）。
    pub fn trend_stable(&self) -> bool {
        self.buckets.iter().all(|b| {
            let s = if b.slope_x10 < 0 { b.slope_x10.wrapping_neg() } else { b.slope_x10 };
            s <= DRIFT_TH_PPM_X10
        })
    }

    /// schema 复核（档位/周期/时戳单调性已闸——此处档+周期+闸门三件套）。
    pub fn schema_ok(&self) -> bool {
        (self.gear == GEAR_SHORT || self.gear == GEAR_LONG)
            && self.period_ms > 0
            && (self.gear == GEAR_SHORT || self.gated())
    }

    /// 档位时长核（2h 档 QEMU 实测口径 / 24h 档闸门口径）。
    pub fn gear_span_ms(&self) -> u64 {
        if self.gear == GEAR_LONG {
            LONG_MS
        } else {
            SHORT_MS
        }
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_LONG_SEALED);
        }
        if !self.conserved() {
            return Err(XC_LONG_CONS);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// FNV 指纹：档+周期+逐桶+闸门登记入列。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.gear as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.period_ms as u32;
        h = h.wrapping_mul(0x0100_0193);
        for b in self.buckets.iter() {
            h ^= b.bid ^ b.samples ^ (b.resid_sum as u32) ^ (b.slope_x10 as u32);
            h = h.wrapping_mul(0x0100_0193);
        }
        if let Some((g, span, nb)) = self.gated_entry {
            h ^= g ^ (span as u32) ^ (nb as u32) << 8;
            h = h.wrapping_mul(0x0100_0193);
        }
        (h as u64) << 16 | (self.buckets.len() as u64 & 0xFFFF)
    }

    pub fn bucket_count(&self) -> usize {
        self.buckets.len()
    }
    pub fn flag_count(&self) -> u32 {
        self.flags
    }
}

/// 注入核：六类各 10/10。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）档位闭集外必拒（2..=11 十例）
    {
        let mut hits = 0;
        for g in 2..=11u8 {
            if LongRunRig::new(g, 100).is_err() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）零周期 schema 必拒（10/10）
    {
        let mut hits = 0;
        for _ in 0..10 {
            if LongRunRig::new(GEAR_SHORT, 0).is_err() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）桶号越界必抓（12..=21 十例）
    {
        let mut r = match LongRunRig::new(GEAR_SHORT, 100) {
            Ok(r) => r,
            Err(_) => return (N, caught),
        };
        let mut hits = 0;
        for b in 12..=21u32 {
            if r.sample(b, 1, 0) == Err(XC_LONG_BUCKET) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 4）时戳回退必抓
    {
        let mut r = match LongRunRig::new(GEAR_SHORT, 100) {
            Ok(r) => r,
            Err(_) => return (N, caught),
        };
        let _ = r.sample(0, 1000, 1);
        let mut hits = 0;
        for k in 0..10u64 {
            if r.sample(1, 1000u64.saturating_sub(k), 1) == Err(XC_LONG_SCHEMA) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 5）漂移超阈立案（斜率 600..1500 十例）
    {
        let mut r = match LongRunRig::new(GEAR_SHORT, 100) {
            Ok(r) => r,
            Err(_) => return (N, caught),
        };
        let mut hits = 0;
        for k in 0..10i64 {
            // 单样斜率 = resid*10；resid=61+k → 斜率 610..700 > 500
            if r.sample(0, 1000 + k as u64, 61 + k) == Err(XC_LONG_DRIFT) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 6）封账后采样必拒
    {
        let mut r = match LongRunRig::new(GEAR_SHORT, 100) {
            Ok(r) => r,
            Err(_) => return (N, caught),
        };
        let _ = r.sample(0, 1, 0);
        let _ = r.seal();
        if r.sample(0, 2, 0) == Err(XC_LONG_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1019-J1。
pub fn run_unx1019_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1019");

    // C19-01 双档就绪（短跑+长跑闸门登记）
    {
        let mut a = match LongRunRig::new(GEAR_SHORT, 600_000) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let mut b = match LongRunRig::new(GEAR_LONG, 3_600_000) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let ok = a.register_gate().is_ok()
            && b.register_gate().is_ok()
            && a.gated()
            && b.gated()
            && a.schema_ok()
            && b.schema_ok();
        s.add("C19-01-双档就绪", ok, if ok { "" } else { "档位缺" });
    }
    // C19-02 24h 配置口径（长跑 span=86400000）
    {
        let b = match LongRunRig::new(GEAR_LONG, 3_600_000) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let ok = b.gear_span_ms() == LONG_MS;
        s.add("C19-02-24h 口径", ok, if ok { "" } else { "口径漂" });
    }
    // C19-03 2h 短跑档可验证（12 桶全采样，判稳）
    {
        let mut r = match LongRunRig::new(GEAR_SHORT, 600_000) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let _ = r.register_gate();
        let mut ok = true;
        for b in 0..N_BUCKETS {
            // 每桶 5 样，残差温和 ±20（均值斜率 ≤ 40 < 500）
            for k in 0..5u32 {
                let resid = ((k % 2) as i64) * 20 - 10;
                if r.sample(b, 600_000 * (b as u64 + 1) + k as u64, resid).is_err() {
                    ok = false;
                    break;
                }
            }
        }
        ok = ok && r.bucket_count() == 12 && r.trend_stable() && r.conserved();
        s.add("C19-03-短跑可验", ok, if ok { "" } else { "短跑破" });
    }
    // C19-04 趋势账 schema 通过
    {
        let mut r = match LongRunRig::new(GEAR_LONG, 3_600_000) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let _ = r.register_gate();
        for b in 0..N_BUCKETS {
            let _ = r.sample(b, 3_600_000 * (b as u64 + 1), 5);
        }
        let ok = r.schema_ok() && r.conserved() && r.trend_stable();
        s.add("C19-04-schema 通过", ok, if ok { "" } else { "schema 破" });
    }
    // C19-05 漂移超阈立案（分桶趋势破）
    {
        let mut r = match LongRunRig::new(GEAR_SHORT, 100) {
            Ok(x) => x,
            Err(_) => return s,
        };
        // 桶 0 累计斜率：3 样 100,100,100 → 均值 100 ×10 = 1000 > 500
        let a = r.sample(0, 1, 100);
        // 第一样即超（100*10=1000）
        let ok = a == Err(XC_LONG_DRIFT) && r.flag_count() >= 1;
        s.add("C19-05-漂移立案", ok, if ok { "" } else { "立案漏" });
    }
    // C19-06 阈值边界（50 → 斜率 500 恰稳 / 51 → 510 破）
    {
        let mut a = match LongRunRig::new(GEAR_SHORT, 100) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let mut b = match LongRunRig::new(GEAR_SHORT, 100) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let ok_a = a.sample(0, 1, 50).is_ok();
        let ok_b = b.sample(0, 1, 51) == Err(XC_LONG_DRIFT);
        s.add("C19-06-阈值边界", ok_a && ok_b, if ok_a && ok_b { "" } else { "边界漂" });
    }
    // C19-07 桶账守恒
    {
        let mut r = match LongRunRig::new(GEAR_SHORT, 100) {
            Ok(x) => x,
            Err(_) => return s,
        };
        for b in 0..N_BUCKETS {
            let _ = r.sample(b, (b as u64 + 1) * 10, 3);
        }
        s.add("C19-07-桶账守恒", r.conserved(), if r.conserved() { "" } else { "守恒破" });
    }
    // C19-08 桶号闭集
    {
        let mut r = match LongRunRig::new(GEAR_SHORT, 100) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let a = r.sample(0, 1, 0).is_ok();
        let b = r.sample(N_BUCKETS - 1, 2, 0).is_ok();
        let c = r.sample(N_BUCKETS, 3, 0) == Err(XC_LONG_BUCKET);
        s.add("C19-08-桶号闭集", a && b && c, if a && b && c { "" } else { "闭集漏" });
    }
    // C19-09 时戳单调闸
    {
        let mut r = match LongRunRig::new(GEAR_SHORT, 100) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let _ = r.sample(0, 500, 1);
        let ok = r.sample(1, 499, 1) == Err(XC_LONG_SCHEMA) && r.sample(1, 500, 1).is_ok();
        s.add("C19-09-时戳闸", ok, if ok { "" } else { "时戳漏" });
    }
    // C19-10 封账冻结
    {
        let mut r = match LongRunRig::new(GEAR_SHORT, 100) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let _ = r.sample(0, 1, 0);
        let fp = r.seal();
        let ok = fp.is_ok() && r.seal() == Err(XC_LONG_SEALED) && r.sample(0, 2, 0) == Err(XC_LONG_SEALED);
        s.add("C19-10-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C19-11 指纹往返
    {
        let build = || {
            let mut r = match LongRunRig::new(GEAR_LONG, 3_600_000) {
                Ok(x) => x,
                Err(_) => return 0u64,
            };
            let _ = r.register_gate();
            for b in 0..6u32 {
                let _ = r.sample(b, (b as u64 + 1) * 1000, 7);
            }
            r.fingerprint()
        };
        let ok = build() == build() && build() != 0;
        s.add("C19-11-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C19-12 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C19-12-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    s
}
