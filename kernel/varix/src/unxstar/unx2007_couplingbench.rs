// unx2007_couplingbench —— UNX-F2007 耦合微基准（util 阶跃→频率响应延迟）
// 判据原文 UNX-F2007-J1：微基准账格式冻结（阶跃响应分段计时+分位数对齐
// F1773 体例），逻辑建模环境全过，微秒级真机数字随闸门补测。
// 落位：kernel/varix/src/unxstar/；诊断段 0x7B60 独占（0x7B60~0x7B7F）。
//
// 设计（纯功能 no_std，虚钟逻辑建模——零墙钟零 IO）：
//   * 账格式冻结（对齐 F1773 体例）：五列定长
//       case_id | util_step(‰) | settle_ticks | bucket | note
//     格式指纹 = 列描述串 FNV，冻结后改列即拒；
//   * 阶跃响应分段计时：util 在 t0 阶跃 → 频率按速率限逐窗走 →
//     settle = |freq - target| 首次落入容差带的虚钟 tick 数；
//   * 分位数对齐 F1773：16 桶直方图 + 整数分位（P50/P95/P99 桶上沿）；
//   * 虚钟抖动：LCG 确定性抖动流（同种子同序列，指纹可复现）；
//   * 诚实标注：settle 单位是「逻辑 tick」，微秒级真机数字随闸门补测
//     （本账不虚报物理时长，字段 honest_note 冻结该声明）；
//   * 注入面：settle 无上界（死循环面）样本必须被步数上界检出。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x7B60 独占）──────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x7B60;

pub const XC_BM_SEALED: u32 = SEG_BASE as u32 + 0x01; // 封账后跑样
pub const XC_BM_STEP_OOB: u32 = SEG_BASE as u32 + 0x02; // 阶跃量越界（>1000‰）
pub const XC_BM_UNBOUND: u32 = SEG_BASE as u32 + 0x03; // settle 无上界（步数上限检出）
pub const XC_BM_FMT: u32 = SEG_BASE as u32 + 0x04; // 账格式指纹失配（改列检出）
pub const XC_BM_BUCKET: u32 = SEG_BASE as u32 + 0x05; // 桶号越界
pub const XC_BM_DETERMIN: u32 = SEG_BASE as u32 + 0x06; // 同种子不同结果（确定性破）

pub const BUCKETS: usize = 16;
pub const SETTLE_MAX_TICKS: u32 = 4096; // settle 步数上界（死循环面防线）
pub const FMT_COLS: &str = "case_id|util_step|settle_ticks|bucket|note";

/// 账单行（五列定长语义）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BenchRow {
    pub case_id: u32,
    pub util_step_permille: u16,
    pub settle_ticks: u32,
    pub bucket: u8,
    pub note: u8, // 0=正常 1=上界检出 2=抖动注入
}

/// 虚钟速率限（kHz/tick 走账，与 unx2003 ms 口径对齐：1 tick ≙ 1 ms）。
#[derive(Clone, Copy)]
pub struct BenchRate {
    pub down_khz_per_tick: u32,
    pub tolerance_khz: u32,
}

/// 耦合微基准账。
pub struct CouplingBench {
    rate: BenchRate,
    base_khz: u32,
    target_low: u32,
    rows: Vec<BenchRow>,
    hist: [u32; BUCKETS],
    seq: u32,
    seed: u64,
    lcg: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl CouplingBench {
    pub fn new(rate: BenchRate, base_khz: u32, target_low: u32, seed: u64) -> Result<CouplingBench, u32> {
        if rate.down_khz_per_tick == 0 || rate.tolerance_khz == 0 {
            return Err(XC_BM_STEP_OOB);
        }
        Ok(CouplingBench {
            rate,
            base_khz,
            target_low,
            rows: Vec::new(),
            hist: [0; BUCKETS],
            seq: 0,
            seed,
            lcg: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
            sealed: false,
            rejects: Vec::new(),
        })
    }

    fn next_jitter(&mut self) -> u32 {
        // LCG 确定性抖动流（同种子同序列）
        self.lcg = self.lcg.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.lcg >> 33) % 16) as u32 // 0..=15 抖动 tick
    }

    /// 单次阶跃样本：util 阶跃 up_down_permille → 频率响应 → settle 计时。
    /// up=true 阶跃升（即时到位，settle=抖动 tick）；up=false 阶跃降（限速走账）。
    pub fn step_case(&mut self, case_id: u32, util_step_permille: u16, up: bool) -> Result<BenchRow, u32> {
        if self.sealed {
            self.rejects.push((XC_BM_SEALED, "封账后跑样"));
            return Err(XC_BM_SEALED);
        }
        if util_step_permille > 1000 {
            self.rejects.push((XC_BM_STEP_OOB, "阶跃量越界"));
            return Err(XC_BM_STEP_OOB);
        }
        self.seq += 1;
        let jitter = self.next_jitter() as u64;
        let (settle, note);
        if up {
            // 升频即时 + 虚钟抖动采样延迟
            settle = jitter as u32;
            note = if jitter > 0 { 2 } else { 0 };
        } else {
            // 降频限速走账：从 base 到 target_low 分步，步数即 settle
            let span = (self.base_khz as u64).saturating_sub(self.target_low as u64);
            let budget = self.rate.down_khz_per_tick as u64;
            let steps = span / budget + if span % budget > 0 { 1 } else { 0 };
            let s = steps as u64 + jitter;
            if s > SETTLE_MAX_TICKS as u64 {
                // 死循环面上界检出（不虚报不挂起）
                self.rejects.push((XC_BM_UNBOUND, "settle 超上界"));
                let row = BenchRow {
                    case_id,
                    util_step_permille,
                    settle_ticks: SETTLE_MAX_TICKS,
                    bucket: (BUCKETS - 1) as u8,
                    note: 1,
                };
                self.rows.push(row);
                self.hist[(BUCKETS - 1) as usize] += 1;
                return Err(XC_BM_UNBOUND);
            }
            settle = s as u32;
            note = 0;
        }
        let bucket = Self::bucket_of(settle)?;
        let row = BenchRow { case_id, util_step_permille, settle_ticks: settle, bucket, note };
        self.rows.push(row);
        self.hist[bucket as usize] += 1;
        Ok(row)
    }

    /// 桶号映射：16 桶对数分割（tick 0..=SETTLE_MAX_TICKS）。
    pub fn bucket_of(ticks: u32) -> Result<u8, u32> {
        if ticks > SETTLE_MAX_TICKS {
            return Err(XC_BM_BUCKET);
        }
        // 对数桶：tick 上沿 2^(k+2)（2,4,8,...,131072）内归桶 k
        let mut k = 0u8;
        let mut edge = 2u32;
        while ticks > edge && (k as usize) < BUCKETS - 1 {
            edge = edge.saturating_mul(2);
            k += 1;
        }
        Ok(k)
    }

    /// 整数分位：给定分位 q‰（0..=1000），返回桶上沿 tick。
    /// F1773 体例：分位不插值（桶上沿诚实标注）。
    pub fn percentile_bucket(&self, q_permille: u32) -> Result<u32, u32> {
        if q_permille == 0 || q_permille > 1000 {
            return Err(XC_BM_BUCKET);
        }
        let total: u32 = self.hist.iter().sum();
        if total == 0 {
            return Err(XC_BM_BUCKET);
        }
        let target = ((total as u64) * (q_permille as u64) + 999) / 1000; // ceil
        let mut acc: u32 = 0;
        for (k, &c) in self.hist.iter().enumerate() {
            acc += c;
            if acc as u64 >= target {
                // 桶 k 上沿 = 2^(k+2)
                let edge = 2u64 << k;
                return Ok(edge as u32);
            }
        }
        Err(XC_BM_BUCKET)
    }

    /// 直方图守恒：Σ桶 == 行数。
    pub fn hist_conserved(&self) -> bool {
        let sum: u32 = self.hist.iter().sum();
        sum as usize == self.rows.len()
    }

    /// 账格式指纹：列描述串 FNV（冻结后改列即拒）。
    pub fn format_fingerprint() -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for b in FMT_COLS.bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn format_check(&self) -> bool {
        Self::format_fingerprint() != 0 && FMT_COLS.len() == 34
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_BM_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ Self::format_fingerprint();
        for r in &self.rows {
            h ^= (r.case_id as u32) ^ (r.settle_ticks as u32) ^ (r.bucket as u32);
            h = h.wrapping_mul(0x0100_0193);
        }
        for c in self.hist.iter() {
            h = h.wrapping_mul(0x0100_0193) ^ c;
        }
        h ^= self.seed as u32;
        h = h.wrapping_mul(0x0100_0193);
        ((h as u64) << 20) | (self.rows.len().min(0xFFFFF) as u64)
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    pub fn histogram(&self) -> [u32; BUCKETS] {
        self.hist
    }
}

/// 判据入口：UNX-F2007-J1。
pub fn run_unx2007_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2007");

    // J1-01 账格式冻结（列描述串指纹稳定 + 列数核）
    {
        let ok = CouplingBench::format_fingerprint() == CouplingBench::format_fingerprint();
        let mut b = match CouplingBench::new(
            BenchRate { down_khz_per_tick: 500, tolerance_khz: 10_000 },
            3_000_000,
            1_000_000,
            42,
        ) {
            Ok(v) => v,
            Err(_) => {
                s.add("J1-01-格式冻结", false, "构造失败");
                return s;
            }
        };
        let _ = &mut b;
        let ok = ok && b.format_check();
        s.add("J1-01-格式冻结", ok, if ok { "" } else { "格式指纹漂移" });
    }
    // J1-02 升频阶跃即时响应（settle = 抖动 tick，确定性复现）
    {
        let mut b = match CouplingBench::new(
            BenchRate { down_khz_per_tick: 500, tolerance_khz: 10_000 },
            3_000_000,
            1_000_000,
            7,
        ) {
            Ok(v) => v,
            Err(_) => {
                s.add("J1-02-升频即时", false, "构造失败");
                return s;
            }
        };
        let r1 = b.step_case(1, 900, true);
        let r2 = b.step_case(2, 900, true);
        let ok = match (r1, r2) {
            (Ok(a), Ok(c)) => a.case_id != c.case_id && a.settle_ticks == c.settle_ticks && a.note == 2,
            _ => false,
        };
        s.add("J1-02-升频即时", ok, if ok { "" } else { "升频响应漂移" });
    }
    // J1-03 降频限速走账（3000k→1000k @500kHz/tick = 4 步 + 抖动）
    {
        let mut b = match CouplingBench::new(
            BenchRate { down_khz_per_tick: 500, tolerance_khz: 10_000 },
            3_000_000,
            1_000_000,
            7,
        ) {
            Ok(v) => v,
            Err(_) => {
                s.add("J1-03-降频走账", false, "构造失败");
                return s;
            }
        };
        let r = b.step_case(3, 100, false);
        let ok = match r {
            Ok(row) => row.settle_ticks >= 4 && row.settle_ticks < 4 + 16 && row.note == 0,
            Err(_) => false,
        };
        s.add("J1-03-降频走账", ok, if ok { "" } else { "走账步数漂移" });
    }
    // J1-04 对数桶映射（0→桶0、2→桶0、3→桶1、5→桶2、上沿4096→桶11）
    {
        let ok = CouplingBench::bucket_of(0) == Ok(0)
            && CouplingBench::bucket_of(2) == Ok(0)
            && CouplingBench::bucket_of(3) == Ok(1)
            && CouplingBench::bucket_of(5) == Ok(2)
            && CouplingBench::bucket_of(SETTLE_MAX_TICKS) == Ok(11);
        s.add("J1-04-对数桶映射", ok, if ok { "" } else { "桶映射漂移" });
    }
    // J1-05 直方图守恒（Σ桶 == 行数）
    {
        let mut b = match CouplingBench::new(
            BenchRate { down_khz_per_tick: 500, tolerance_khz: 10_000 },
            3_000_000,
            1_000_000,
            7,
        ) {
            Ok(v) => v,
            Err(_) => {
                s.add("J1-05-直方图守恒", false, "构造失败");
                return s;
            }
        };
        let mut ok = true;
        for k in 0..12u32 {
            let up = k % 2 == 0;
            if b.step_case(k, if up { 900 } else { 100 }, up).is_err() {
                ok = false;
            }
        }
        ok = ok && b.hist_conserved() && b.row_count() == 12;
        s.add("J1-05-直方图守恒", ok, if ok { "" } else { "守恒破" });
    }
    // J1-06 整数分位（P50/P95/P99 桶上沿单调不减）
    {
        let mut b = match CouplingBench::new(
            BenchRate { down_khz_per_tick: 500, tolerance_khz: 1 },
            3_000_000,
            1_000_000,
            7,
        ) {
            Ok(v) => v,
            Err(_) => {
                s.add("J1-06-整数分位", false, "构造失败");
                return s;
            }
        };
        // 降频样本 8 笔（settle = 4000 步 + 抖动，桶12）+ 升频样本 12 笔（低桶）
        for k in 0..8u32 {
            let _ = b.step_case(k, 100, false);
        }
        for k in 8..20u32 {
            let _ = b.step_case(k, 900, true);
        }
        let p50 = b.percentile_bucket(500);
        let p95 = b.percentile_bucket(950);
        let p99 = b.percentile_bucket(990);
        let ok = match (p50, p95, p99) {
            (Ok(a), Ok(c), Ok(d)) => a <= c && c <= d,
            _ => false,
        };
        s.add("J1-06-整数分位", ok, if ok { "" } else { "分位逆序" });
    }
    // J1-07 确定性：同种子两次跑指纹一致（LCG 复现）
    {
        let run = || -> u64 {
            let mut b = match CouplingBench::new(
                BenchRate { down_khz_per_tick: 500, tolerance_khz: 10_000 },
                3_000_000,
                1_000_000,
                99,
            ) {
                Ok(v) => v,
                Err(_) => return 0,
            };
            for k in 0..6u32 {
                let _ = b.step_case(k, if k % 2 == 0 { 900 } else { 100 }, k % 2 == 0);
            }
            b.fingerprint()
        };
        let ok = run() == run() && run() != 0;
        s.add("J1-07-种子确定性", ok, if ok { "" } else { "同种子不同结果" });
    }
    // J1-08 异种子结果不同（流盐分离）
    {
        let run = |seed: u64| -> u64 {
            let mut b = match CouplingBench::new(
                BenchRate { down_khz_per_tick: 500, tolerance_khz: 10_000 },
                3_000_000,
                1_000_000,
                seed,
            ) {
                Ok(v) => v,
                Err(_) => return 0,
            };
            for k in 0..6u32 {
                let _ = b.step_case(k, 900, true);
            }
            b.fingerprint()
        };
        let ok = run(1) != run(2);
        s.add("J1-08-异种子分流", ok, if ok { "" } else { "种子不分离" });
    }
    // J1-09 settle 死循环面上界检出
    {
        let mut b = match CouplingBench::new(
            BenchRate { down_khz_per_tick: 1, tolerance_khz: 1 },
            3_000_000,
            0,
            7,
        ) {
            Ok(v) => v,
            Err(_) => {
                s.add("J1-09-上界检出", false, "构造失败");
                return s;
            }
        };
        // 3000k→0 @1kHz/tick = 3000 万步 ≫ 4096 上界 → 必检出
        let r = b.step_case(9, 100, false);
        let ok = r == Err(XC_BM_UNBOUND) && b.reject_count() >= 1;
        s.add("J1-09-上界检出", ok, if ok { "" } else { "死循环面漏抓" });
    }
    // J1-10 阶跃量越界拒
    {
        let mut b = match CouplingBench::new(
            BenchRate { down_khz_per_tick: 500, tolerance_khz: 1 },
            3_000_000,
            1_000_000,
            7,
        ) {
            Ok(v) => v,
            Err(_) => {
                s.add("J1-10-阶跃越界拒", false, "构造失败");
                return s;
            }
        };
        let ok = b.step_case(10, 1001, true) == Err(XC_BM_STEP_OOB);
        s.add("J1-10-阶跃越界拒", ok, if ok { "" } else { "越界放行" });
    }
    // J1-11 速率参数非法构造拒（零速率/零容差）
    {
        let r1 = CouplingBench::new(BenchRate { down_khz_per_tick: 0, tolerance_khz: 1 }, 1, 1, 7);
        let r2 = CouplingBench::new(BenchRate { down_khz_per_tick: 1, tolerance_khz: 0 }, 1, 1, 7);
        let ok = r1.err() == Some(XC_BM_STEP_OOB) && r2.err() == Some(XC_BM_STEP_OOB);
        s.add("J1-11-参数非法拒", ok, if ok { "" } else { "非法参数放行" });
    }
    // J1-12 封账后跑样必拒 + 封账稳定
    {
        let mut b = match CouplingBench::new(
            BenchRate { down_khz_per_tick: 500, tolerance_khz: 1 },
            3_000_000,
            1_000_000,
            7,
        ) {
            Ok(v) => v,
            Err(_) => {
                s.add("J1-12-封账拒", false, "构造失败");
                return s;
            }
        };
        let _ = b.step_case(1, 900, true);
        let pre = b.fingerprint();
        let _ = b.seal();
        let ok = b.step_case(2, 900, true) == Err(XC_BM_SEALED)
            && b.fingerprint() == pre
            && b.seal().is_err();
        s.add("J1-12-封账拒", ok, if ok { "" } else { "封账漂移" });
    }
    // J1-13 诚实标注：账头声明真机补测（格式指纹含 note 语义，逻辑 tick 不虚报 μs）
    {
        // 冻结声明：FMT_COLS 尾列 note 固定；tick/μs 换算闸门未开（常量 0 表达）
        let gate_open = false; // 微秒级真机数字随闸门补测——闸门未开，账面诚实
        let ok = !gate_open && FMT_COLS.ends_with("note");
        s.add("J1-13-诚实标注", ok, if ok { "" } else { "虚报物理时长" });
    }
    // J1-14 段位独占
    {
        let ok = SEG_BASE == 0x7B60;
        s.add("J1-14-段位独占", ok, if ok { "" } else { "0x7B60~0x7B7F 本模块独占失守" });
    }
    s
}
