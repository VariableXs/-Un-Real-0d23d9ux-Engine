// unx1055_u1055 —— UNX-F1055 LAPIC 定时器长跑资产（24h——随闸门）
// 判据原文 UNX-F1055-J1：长跑资产就绪（24h 随闸门登记），2h 档 QEMU 先行，与 F1019/F1038 三线资产同 schema。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2A80 独占（0x2A81~0x2A8F）。
//
// 设计（纯功能 no_std）：
//   * 三线同 schema：F1019/F1038/F1055 三线资产共用 schema 版本与样张结构；
//   * 档位：2h（QEMU 先行）/24h（随闸门）逐档登记与样张；
//   * 三线对齐：同 schema 校验三线版本一致才可封账；
//   * 质量分修正回灌与 saturating 防溢出；
//   * FNV 指纹 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2A80 独占） ─────────────────────────────────────────────
pub const XC_LL_OOB: u32 = 0x2A81; // 档位/线号越界
pub const XC_LL_SCHEMA: u32 = 0x2A82; // 三线 schema 不一致
pub const XC_LL_SEALED: u32 = 0x2A83; // 封账后写样张
pub const XC_LL_PPM: u32 = 0x2A84; // ppm 越限
pub const XC_LL_LOST: u32 = 0x2A85; // 回灌消费不守恒

/// 三线资产线号。
pub const LINE_F1019: usize = 0;
pub const LINE_F1038: usize = 1;
pub const LINE_F1055: usize = 2;
pub const LINES: usize = 3;
/// 共用 schema 版本。
pub const SHARED_SCHEMA: u16 = 0x0A80;
pub const TIER_2H: u8 = 2;
pub const TIER_24H: u8 = 24;
pub const PPM_LIMIT: i32 = 500;

#[derive(Clone, Copy)]
pub struct LongSample {
    pub line: usize,   // 三线之一
    pub tier: u8,      // 2/24
    pub nominal: u64,
    pub measured: u64,
}

/// LAPIC 长跑资产账本。
pub struct LapicLongRun {
    schema_ver: [u16; LINES], // 三线各自 schema 版本
    samples: Vec<LongSample>,
    enrolled: Vec<(usize, u8)>, // (线, 档) 登记
    quality: u32,
    corrections: u32,
    consumed: u32,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl LapicLongRun {
    pub fn new() -> LapicLongRun {
        LapicLongRun {
            schema_ver: [SHARED_SCHEMA; LINES],
            samples: Vec::new(),
            enrolled: Vec::new(),
            quality: 1000,
            corrections: 0,
            consumed: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 登记档位（线×档去重）。
    pub fn enroll(&mut self, line: usize, tier: u8) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_LL_SEALED, "封账后登记"));
            return Err(XC_LL_SEALED);
        }
        if line >= LINES || (tier != TIER_2H && tier != TIER_24H) {
            self.rejects.push((XC_LL_OOB, "线号或档位越界"));
            return Err(XC_LL_OOB);
        }
        for &(l, t) in &self.enrolled {
            if l == line && t == tier {
                self.rejects.push((XC_LL_OOB, "同线同档重复"));
                return Err(XC_LL_OOB);
            }
        }
        self.enrolled.push((line, tier));
        Ok(())
    }

    /// 写样张（三线 schema 一致性前置校验）。
    pub fn sample(&mut self, s: LongSample) -> Result<i32, u32> {
        if self.sealed {
            self.rejects.push((XC_LL_SEALED, "封账后写样张"));
            return Err(XC_LL_SEALED);
        }
        if s.line >= LINES || s.nominal == 0 {
            self.rejects.push((XC_LL_OOB, "线号越界或名义零"));
            return Err(XC_LL_OOB);
        }
        if self.schema_ver[s.line] != SHARED_SCHEMA {
            self.rejects.push((XC_LL_SCHEMA, "三线 schema 不一致"));
            return Err(XC_LL_SCHEMA);
        }
        let enrolled = self.enrolled.iter().any(|&(l, t)| l == s.line && t == s.tier);
        if !enrolled {
            self.rejects.push((XC_LL_OOB, "线档未登记"));
            return Err(XC_LL_OOB);
        }
        let diff = if s.measured >= s.nominal {
            (s.measured - s.nominal).min(i32::MAX as u64) as i64
        } else {
            -((s.nominal - s.measured).min(i32::MAX as u64) as i64)
        };
        let ppm64 = diff.saturating_mul(1_000_000).saturating_div(s.nominal as i64);
        let ppm = ppm64.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        if ppm.abs() > PPM_LIMIT {
            self.rejects.push((XC_LL_PPM, "ppm 越限"));
            return Err(XC_LL_PPM);
        }
        self.samples.push(s);
        let penalty = (ppm.abs() as u32).saturating_mul(2);
        self.quality = self.quality.saturating_sub(penalty);
        Ok(ppm)
    }

    /// 三线 schema 对齐核验。
    pub fn three_line_aligned(&self) -> bool {
        self.schema_ver.iter().all(|&v| v == SHARED_SCHEMA)
    }

    /// 某线某档漂移改写 schema（注入用，触发不一致）。
    pub fn corrupt_schema(&mut self, line: usize) -> Result<(), u32> {
        if line >= LINES {
            return Err(XC_LL_OOB);
        }
        self.schema_ver[line] = self.schema_ver[line].wrapping_add(1);
        Ok(())
    }

    /// 质量分回灌（消费配对）。
    pub fn push_correction(&mut self, corr_ppm: i32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_LL_SEALED, "封账后回灌"));
            return Err(XC_LL_SEALED);
        }
        if corr_ppm.abs() > PPM_LIMIT {
            self.rejects.push((XC_LL_PPM, "修正量越限"));
            return Err(XC_LL_PPM);
        }
        self.corrections = self.corrections.saturating_add(1);
        if corr_ppm < 0 {
            let q = self.quality.saturating_add(((-corr_ppm) as u32).saturating_mul(2));
            self.quality = if q > 1000 { 1000 } else { q };
        } else {
            self.quality = self.quality.saturating_sub((corr_ppm as u32).saturating_mul(2));
        }
        Ok(())
    }

    pub fn consume_correction(&mut self) -> Result<(), u32> {
        if self.consumed >= self.corrections {
            self.rejects.push((XC_LL_LOST, "无待消费修正"));
            return Err(XC_LL_LOST);
        }
        self.consumed = self.consumed.saturating_add(1);
        Ok(())
    }

    /// 长跑就绪：F1055 线两档齐且有样张。
    pub fn ready(&self) -> bool {
        let has2 = self.enrolled.iter().any(|&(l, t)| l == LINE_F1055 && t == TIER_2H);
        let has24 = self.enrolled.iter().any(|&(l, t)| l == LINE_F1055 && t == TIER_24H);
        let sampled = self.samples.iter().any(|s| s.line == LINE_F1055 && s.tier == TIER_2H)
            && self.samples.iter().any(|s| s.line == LINE_F1055 && s.tier == TIER_24H);
        has2 && has24 && sampled
    }

    /// 守恒：消费 = 回灌数。
    pub fn conserved(&self) -> bool {
        self.consumed == self.corrections && !self.samples.is_empty()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_LL_SEALED);
        }
        if !self.ready() || !self.three_line_aligned() {
            self.rejects.push((XC_LL_SCHEMA, "未就绪或 schema 不齐禁封"));
            return Err(XC_LL_SCHEMA);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        for &v in &self.schema_ver {
            h ^= v as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        for s in &self.samples {
            h ^= (s.line as u32).wrapping_mul(0x9E37)
                ^ (s.tier as u32)
                ^ (s.nominal as u32)
                ^ (s.measured as u32);
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.consumed;
        h = h.wrapping_mul(0x0100_0193);
        h as u64
    }

    pub fn quality(&self) -> u32 {
        self.quality
    }
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）线号/档位越界必拒（10/10）
    {
        let mut a = LapicLongRun::new();
        let mut hits = 0;
        for k in 3..13u8 {
            if a.enroll(k as usize % 7 + 3, k).is_err() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）同线同档重复必拒
    {
        let mut a = LapicLongRun::new();
        let _ = a.enroll(LINE_F1055, TIER_2H);
        if a.enroll(LINE_F1055, TIER_2H) == Err(XC_LL_OOB) {
            caught += 1;
        }
    }
    // 3）schema 破坏后写样张必拒
    {
        let mut a = LapicLongRun::new();
        let _ = a.enroll(LINE_F1055, TIER_2H);
        let _ = a.corrupt_schema(LINE_F1055);
        let r = a.sample(LongSample { line: LINE_F1055, tier: TIER_2H, nominal: 1_000, measured: 1_000 });
        if r == Err(XC_LL_SCHEMA) {
            caught += 1;
        }
    }
    // 4）ppm 越限必拒
    {
        let mut a = LapicLongRun::new();
        let _ = a.enroll(LINE_F1055, TIER_24H);
        let r = a.sample(LongSample { line: LINE_F1055, tier: TIER_24H, nominal: 1_000, measured: 1_900 });
        if r == Err(XC_LL_PPM) {
            caught += 1;
        }
    }
    // 5）无回灌消费必拒
    {
        let mut a = LapicLongRun::new();
        if a.consume_correction() == Err(XC_LL_LOST) {
            caught += 1;
        }
    }
    // 6）未登记线档写样张必拒
    {
        let mut a = LapicLongRun::new();
        let _ = a.enroll(LINE_F1019, TIER_2H);
        let r = a.sample(LongSample { line: LINE_F1019, tier: TIER_24H, nominal: 1_000, measured: 1_000 });
        if r == Err(XC_LL_OOB) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1055-J1。
pub fn run_unx1055_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1055");

    // C55-01 三线同 schema
    {
        let a = LapicLongRun::new();
        s.add("C55-01-三线同schema", a.three_line_aligned(), if a.three_line_aligned() { "" } else { "schema漂移" });
    }
    // C55-02 2h 档 QEMU 先行
    {
        let mut a = LapicLongRun::new();
        let _ = a.enroll(LINE_F1055, TIER_2H);
        let r = a.sample(LongSample { line: LINE_F1055, tier: TIER_2H, nominal: 7_200_000_000, measured: 7_203_600_000 });
        s.add("C55-02-2h先行", r == Ok(500), if r == Ok(500) { "" } else { "样张漂移" });
    }
    // C55-03 24h 档随闸门
    {
        let mut a = LapicLongRun::new();
        let _ = a.enroll(LINE_F1055, TIER_24H);
        let r = a.sample(LongSample { line: LINE_F1055, tier: TIER_24H, nominal: 86_400_000_000, measured: 86_443_200_000 });
        s.add("C55-03-24h随闸门", r == Ok(500), if r == Ok(500) { "" } else { "样张漂移" });
    }
    // C55-04 三线资产交叉登记
    {
        let mut a = LapicLongRun::new();
        let mut all_ok = true;
        for line in 0..LINES {
            let tier = if line == 0 { TIER_2H } else { TIER_24H };
            if a.enroll(line, tier).is_err() {
                all_ok = false;
            }
        }
        s.add("C55-04-三线登记", all_ok && a.enrolled_len() == 3, if all_ok { "" } else { "登记缺" });
    }
    // C55-05 质量分回灌消费配对
    {
        let mut a = LapicLongRun::new();
        for _ in 0..10 {
            let _ = a.push_correction(-20);
        }
        for _ in 0..10 {
            if a.consume_correction().is_err() {
                // 不应发生
            }
        }
        s.add("C55-05-回灌配对", a.consumed == 10 && a.quality() == 1000, "");
    }
    // C55-06 schema 破坏检测
    {
        let mut a = LapicLongRun::new();
        let _ = a.corrupt_schema(LINE_F1038);
        s.add("C55-06-schema破检", !a.three_line_aligned(), if !a.three_line_aligned() { "" } else { "漏检" });
    }
    // C55-07 长跑就绪判定
    {
        let mut a = LapicLongRun::new();
        let _ = a.enroll(LINE_F1055, TIER_2H);
        let _ = a.enroll(LINE_F1055, TIER_24H);
        let _ = a.sample(LongSample { line: LINE_F1055, tier: TIER_2H, nominal: 1_000, measured: 1_000 });
        let _ = a.sample(LongSample { line: LINE_F1055, tier: TIER_24H, nominal: 1_000, measured: 1_000_500 });
        s.add("C55-07-就绪判定", a.ready(), if a.ready() { "" } else { "就绪缺" });
    }
    // C55-08 守恒
    {
        let mut a = LapicLongRun::new();
        let _ = a.enroll(LINE_F1055, TIER_2H);
        let _ = a.enroll(LINE_F1055, TIER_24H);
        let _ = a.sample(LongSample { line: LINE_F1055, tier: TIER_2H, nominal: 1, measured: 1 });
        let _ = a.sample(LongSample { line: LINE_F1055, tier: TIER_24H, nominal: 1, measured: 1 });
        for _ in 0..3 {
            let _ = a.push_correction(5);
            let _ = a.consume_correction();
        }
        s.add("C55-08-守恒对平", a.conserved(), if a.conserved() { "" } else { "守恒破" });
    }
    // C55-09 封账与指纹
    {
        let mut a = LapicLongRun::new();
        let _ = a.enroll(LINE_F1055, TIER_2H);
        let _ = a.enroll(LINE_F1055, TIER_24H);
        let _ = a.sample(LongSample { line: LINE_F1055, tier: TIER_2H, nominal: 1, measured: 1 });
        let _ = a.sample(LongSample { line: LINE_F1055, tier: TIER_24H, nominal: 1, measured: 1 });
        let sealed = a.seal();
        s.add("C55-09-封账指纹", sealed.is_ok(), "");
    }
    // C55-10 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C55-10-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    s
}

impl LapicLongRun {
    pub fn enrolled_len(&self) -> usize {
        self.enrolled.len()
    }
}
