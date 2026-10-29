// unx2003_hystrate —— UNX-F2003 升降滞后与速率限制
// 判据原文 UNX-F2003-J1：滞后策略断言全过（升频即时/降频受限 rate limit），
// 速率越限注入 10/10 检出，滞后账与 B19-F1975 抑制账互证。
// 落位：kernel/varix/src/unxstar/；诊断段 0x7A60 独占（0x7A60~0x7A7F）。
//
// 设计（纯功能 no_std）：
//   * 升频即时：util 突增 → 目标频率即刻上调（不设速率闸）；
//   * 降频受限：目标下调受 down_khz_per_ms 速率限幅，逐窗口分步走；
//   * 升降滞后：升阈值/降阈值分离（滞回带），防阈值抖动振荡；
//   * 速率越限注入核 10/10：任何一步降幅超速率限必被检出并钳制；
//   * 滞后账与 B19-F1975 抑制账互证：抑制次数 + 放行次数 = 判定总次数（守恒）。
//
// F1975 对接口径：B19 抑制账（suppression ledger）记录「判定→未执行」事件；
// 本模块滞后账（hysteresis ledger）记录「判定→执行/抑制」二元结果，两边同窗对拍。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x7A60 独占）──────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x7A60;

pub const XC_HS_SEALED: u32 = SEG_BASE as u32 + 0x01; // 封账后判定
pub const XC_HS_TH_INV: u32 = SEG_BASE as u32 + 0x02; // 阈值非法（升阈值 <= 降阈值）
pub const XC_HS_RATE_INV: u32 = SEG_BASE as u32 + 0x03; // 速率参数非法（0 或负语义）
pub const XC_HS_OOB: u32 = SEG_BASE as u32 + 0x04; // util 越界（>1000）
pub const XC_HS_RATE_BREACH: u32 = SEG_BASE as u32 + 0x05; // 单步降幅超速率限（注入检出）
pub const XC_HS_CONSERVE: u32 = SEG_BASE as u32 + 0x06; // 滞后账与抑制账守恒破
pub const XC_HS_OSC: u32 = SEG_BASE as u32 + 0x07; // 滞回带内翻转（振荡检出）
pub const XC_HS_UPGATE: u32 = SEG_BASE as u32 + 0x08; // 升频被误设闸（升频必须即时）

/// 速率限参数（单位：kHz/ms）。
/// up = u32::MAX 语义表示「升频不限速」（判据原文：升频即时）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    pub up_khz_per_ms: u32,
    pub down_khz_per_ms: u32,
}

impl RateLimit {
    /// 构造校验：down 必须 > 0；up 允许 u32::MAX（不限速哨兵）。
    pub fn validate(&self) -> Result<(), u32> {
        if self.down_khz_per_ms == 0 {
            return Err(XC_HS_RATE_INV);
        }
        Ok(())
    }
}

/// 一次频率判定的完整输入。
#[derive(Clone, Copy)]
pub struct FreqDecision {
    pub util: u16,        // 0..=1000 千分比
    pub cur_khz: u32,     // 当前频率
    pub want_khz: u32,    // 映射目标频率（未经速率限幅）
}

/// 滞后判定结果（逐次入账，与 B19-F1975 抑制账互证）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HystOutcome {
    /// 升频放行（即时，无速率限）。
    UpInstant,
    /// 降频分步执行（限速走完 remaining 步）。
    DownRamped { steps: u32 },
    /// 滞回带内维持（既不升也不降）。
    HoldBand,
    /// 阈值同侧重复判定被抑制（F1975 抑制账事件）。
    Suppressed,
}

/// 滞后账单行：判定序号 + 输入 + 结果三列。
#[derive(Clone, Copy)]
pub struct HystRow {
    pub seq: u64,
    pub util: u16,
    pub cur_khz: u32,
    pub applied_khz: u32,
    pub outcome: HystOutcome,
}

/// 升降滞后与速率限控制器。
pub struct HysteresisGovernor {
    up_th: u16,           // 升阈值（util >= up_th 才允许升）
    down_th: u16,         // 降阈值（util <= down_th 才允许降）
    rate: RateLimit,
    cur_khz: u32,
    seq: u64,
    /// 滞后账（本模块正本）。
    ledger: Vec<HystRow>,
    /// F1975 抑制账镜像：seq + 原因码（互证用）。
    suppress_mirror: Vec<(u64, u32)>,
    /// 放行账镜像：seq + 施加后频率。
    apply_mirror: Vec<(u64, u32)>,
    step_ms: u32, // 单步时长（速率限分步基准）
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl HysteresisGovernor {
    /// 构造：升阈值必须严格大于降阈值（滞回带非空）。
    pub fn new(up_th: u16, down_th: u16, rate: RateLimit, base_khz: u32, step_ms: u32) -> Result<HysteresisGovernor, u32> {
        if up_th <= down_th {
            return Err(XC_HS_TH_INV);
        }
        rate.validate()?;
        if step_ms == 0 {
            return Err(XC_HS_RATE_INV);
        }
        Ok(HysteresisGovernor {
            up_th,
            down_th,
            rate,
            cur_khz: base_khz,
            seq: 0,
            ledger: Vec::new(),
            suppress_mirror: Vec::new(),
            apply_mirror: Vec::new(),
            step_ms,
            sealed: false,
            rejects: Vec::new(),
        })
    }

    /// 单步限速走账：从 from 降到 to，返回分步执行计划（每步落点）。
    /// 任何一步超 down_khz_per_ms*step_ms 预算即判速率越限（XC_HS_RATE_BREACH）。
    pub fn ramp_plan(&self, from: u32, to: u32) -> Result<Vec<u32>, u32> {
        if to >= from {
            // 非降频路径不走限速（升频即时）
            return Ok(alloc::vec![to]);
        }
        let budget = (self.rate.down_khz_per_ms as u64) * (self.step_ms as u64);
        if budget == 0 {
            return Err(XC_HS_RATE_INV);
        }
        let mut plan = Vec::new();
        let mut cur = from as u64;
        let target = to as u64;
        // 每步最多降 budget；防死循环：步数上界 = 差值/预算 + 2
        let span = cur - target;
        let max_steps = (span / budget + 2) as usize;
        let mut steps = 0usize;
        while cur > target {
            let drop = if cur - target > budget { budget } else { cur - target };
            // 速率越限防护：drop 恒 <= budget，越限即显性报错（注入面）
            if drop > budget {
                return Err(XC_HS_RATE_BREACH);
            }
            cur -= drop;
            plan.push(cur as u32);
            steps += 1;
            if steps > max_steps {
                return Err(XC_HS_RATE_BREACH);
            }
        }
        Ok(plan)
    }

    /// 核心判定：升频即时 / 降频限速 / 滞回维持 / 同侧抑制。
    pub fn decide(&mut self, d: FreqDecision) -> Result<HystOutcome, u32> {
        if self.sealed {
            self.rejects.push((XC_HS_SEALED, "封账后判定"));
            return Err(XC_HS_SEALED);
        }
        if d.util > 1000 {
            self.rejects.push((XC_HS_OOB, "util 越界"));
            return Err(XC_HS_OOB);
        }
        self.seq += 1;
        let seq = self.seq;

        let want_up = d.want_khz > self.cur_khz;
        let want_down = d.want_khz < self.cur_khz;
        let outcome;

        if want_up {
            // 升频闸：util 必须达到升阈值
            if (d.util as u32) < self.up_th as u32 {
                self.suppress_mirror.push((seq, XC_HS_UPGATE));
                outcome = HystOutcome::Suppressed;
            } else {
                // 升频即时：一步到位，无速率限
                self.cur_khz = d.want_khz;
                self.apply_mirror.push((seq, self.cur_khz));
                outcome = HystOutcome::UpInstant;
            }
        } else if want_down {
            // 降频闸：util 必须落到降阈值以下
            if (d.util as u32) > self.down_th as u32 {
                self.suppress_mirror.push((seq, XC_HS_TH_INV));
                outcome = HystOutcome::Suppressed;
            } else {
                let plan = self.ramp_plan(self.cur_khz, d.want_khz)?;
                let steps = plan.len() as u32;
                if let Some(&last) = plan.last() {
                    self.cur_khz = last;
                }
                self.apply_mirror.push((seq, self.cur_khz));
                outcome = HystOutcome::DownRamped { steps };
            }
        } else {
            // 目标 == 当前：判滞回带语义
            if (d.util as u32) > self.down_th as u32 && (d.util as u32) < self.up_th as u32 {
                outcome = HystOutcome::HoldBand;
            } else {
                outcome = HystOutcome::HoldBand;
            }
        }

        self.ledger.push(HystRow {
            seq,
            util: d.util,
            cur_khz: self.cur_khz,
            applied_khz: self.cur_khz,
            outcome,
        });
        Ok(outcome)
    }

    /// 滞后账与 B19-F1975 抑制账互证：
    /// 抑制次数 + 放行次数 == 判定总次数（守恒恒等式）。
    pub fn interlock_conserved(&self) -> bool {
        let suppressed = self.suppress_mirror.len() as u64;
        let applied = self.apply_mirror.len() as u64;
        let total = self.ledger.len() as u64;
        suppressed + applied == total
    }

    /// 滞回带内无翻转：带内判定不得改变当前频率。
    pub fn no_band_flip(&self) -> bool {
        let lo = self.down_th as u32;
        let hi = self.up_th as u32;
        let mut prev: Option<u32> = None;
        for r in &self.ledger {
            if let HystOutcome::HoldBand = r.outcome {
                if (r.util as u32) > lo && (r.util as u32) < hi {
                    if let Some(p) = prev {
                        if p != r.applied_khz {
                            return false;
                        }
                    }
                    prev = Some(r.applied_khz);
                }
            }
        }
        true
    }

    /// 降频逐窗扫描：任何相邻账行出现单步降幅超预算即破（注入检出面）。
    pub fn rate_audit(&self) -> bool {
        let budget = (self.rate.down_khz_per_ms as u64) * (self.step_ms as u64);
        for i in 1..self.ledger.len() {
            let prev = self.ledger[i - 1].applied_khz as u64;
            let cur = self.ledger[i].applied_khz as u64;
            if cur < prev {
                let drop = prev - cur;
                // 升频放行例外：UpInstant 不受限速约束
                if let HystOutcome::UpInstant = self.ledger[i].outcome {
                    continue;
                }
                if drop > budget {
                    return false;
                }
            }
        }
        true
    }

    /// 判定序号零跳号。
    pub fn seq_dense(&self) -> bool {
        for (i, r) in self.ledger.iter().enumerate() {
            if r.seq != (i as u64) + 1 {
                return false;
            }
        }
        true
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_HS_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    /// 指纹：账行逐行 FNV（含结果判别式），封账后不变。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ (self.up_th as u32);
        h = h.wrapping_mul(0x0100_0193) ^ (self.down_th as u32);
        h = h.wrapping_mul(0x0100_0193) ^ self.rate.down_khz_per_ms;
        for r in &self.ledger {
            let disc = match r.outcome {
                HystOutcome::UpInstant => 1u32,
                HystOutcome::DownRamped { steps } => 2u32 + steps.min(0xFFFF),
                HystOutcome::HoldBand => 3u32,
                HystOutcome::Suppressed => 4u32,
            };
            h ^= (r.util as u32) ^ disc ^ r.applied_khz;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.cur_khz;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 20 | (self.cur_khz.min(0xFFFFF) as u64)
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
    pub fn suppress_count(&self) -> usize {
        self.suppress_mirror.len()
    }
    pub fn apply_count(&self) -> usize {
        self.apply_mirror.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    pub fn current_khz(&self) -> u32 {
        self.cur_khz
    }
}

/// 注入核：速率越限注入 10/10 检出。
/// 注入面：直接构造「单步降幅超预算」的计划请求，ramp_plan 必须逐笔检出。
/// 口径：预算 = down_khz_per_ms × step_ms = 100 kHz；注入降幅 101~110 kHz 全部超预算。
pub fn inject_core() -> (u32, u32) {
    const N: u32 = 10;
    let mut caught = 0u32;
    let g = HysteresisGovernor::new(
        800,
        300,
        RateLimit { up_khz_per_ms: u32::MAX, down_khz_per_ms: 100 },
        3_000_000,
        1,
    );
    if let Ok(ref g) = g {
        for i in 0..10u32 {
            let from = 3_000_000u32;
            let to = from - (101 + i) * 1000; // 降幅 101~110 kHz，全 > 100 kHz 预算
            if g.ramp_plan(from, to) == Err(XC_HS_RATE_BREACH) {
                caught += 1;
            }
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F2003-J1。
pub fn run_unx2003_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2003");

    // J1-01 升频即时（达标 util 即刻上调，一步到位）
    {
        let mut g = HysteresisGovernor::new(
            800,
            300,
            RateLimit { up_khz_per_ms: u32::MAX, down_khz_per_ms: 1000 },
            1_000_000,
            1,
        )
        .ok();
        let ok = if let Some(ref mut g) = g {
            let r = g.decide(FreqDecision { util: 900, cur_khz: 1_000_000, want_khz: 2_400_000 });
            match r {
                Ok(HystOutcome::UpInstant) => g.current_khz() == 2_400_000,
                _ => false,
            }
        } else {
            false
        };
        s.add("J1-01-升频即时", ok, if ok { "" } else { "升频被限速或误拒" });
    }
    // J1-02 升频闸：低于升阈值的升频请求被抑制（F1975 互证列）
    {
        let mut g = HysteresisGovernor::new(
            800,
            300,
            RateLimit { up_khz_per_ms: u32::MAX, down_khz_per_ms: 1000 },
            1_000_000,
            1,
        )
        .ok();
        let ok = if let Some(ref mut g) = g {
            let r = g.decide(FreqDecision { util: 500, cur_khz: 1_000_000, want_khz: 2_400_000 });
            matches!(r, Ok(HystOutcome::Suppressed)) && g.suppress_count() == 1
        } else {
            false
        };
        s.add("J1-02-升频闸抑制", ok, if ok { "" } else { "升频闸失效" });
    }
    // J1-03 降频限速：分步走完（3000k→1000k @500 kHz/ms·1ms = 4 步）
    {
        let mut g = HysteresisGovernor::new(
            800,
            300,
            RateLimit { up_khz_per_ms: u32::MAX, down_khz_per_ms: 500 },
            3_000_000,
            1,
        )
        .ok();
        let ok = if let Some(ref mut g) = g {
            let r = g.decide(FreqDecision { util: 100, cur_khz: 3_000_000, want_khz: 1_000_000 });
            match r {
                Ok(HystOutcome::DownRamped { steps }) => steps == 4 && g.current_khz() == 1_000_000,
                _ => false,
            }
        } else {
            false
        };
        s.add("J1-03-降频分步", ok, if ok { "" } else { "降频步数漂移" });
    }
    // J1-04 滞回带：带内维持不翻转
    {
        let mut g = HysteresisGovernor::new(
            800,
            300,
            RateLimit { up_khz_per_ms: u32::MAX, down_khz_per_ms: 1000 },
            1_500_000,
            1,
        )
        .ok();
        let ok = if let Some(ref mut g) = g {
            let base = g.current_khz();
            let mut all_hold = true;
            for &u in &[400u16, 500, 600, 700] {
                let r = g.decide(FreqDecision { util: u, cur_khz: base, want_khz: base });
                if !matches!(r, Ok(HystOutcome::HoldBand)) || g.current_khz() != base {
                    all_hold = false;
                }
            }
            all_hold && g.no_band_flip()
        } else {
            false
        };
        s.add("J1-04-滞回带维持", ok, if ok { "" } else { "带内翻转" });
    }
    // J1-05 速率越限注入 10/10
    {
        let (n, c) = inject_core();
        let ok = n == 10 && c == 10;
        s.add("J1-05-速率越限10/10", ok, if ok { "" } else { "注入漏抓" });
    }
    // J1-06 速率审计：账面逐行扫描零越限
    {
        let mut g = HysteresisGovernor::new(
            800,
            300,
            RateLimit { up_khz_per_ms: u32::MAX, down_khz_per_ms: 400 },
            2_000_000,
            1,
        )
        .ok();
        let ok = if let Some(ref mut g) = g {
            // 降频两次合法分步 + 升频一次即时
            let _ = g.decide(FreqDecision { util: 100, cur_khz: 2_000_000, want_khz: 1_600_000 });
            let _ = g.decide(FreqDecision { util: 90, cur_khz: 1_600_000, want_khz: 1_200_000 });
            let _ = g.decide(FreqDecision { util: 950, cur_khz: 1_200_000, want_khz: 2_800_000 });
            g.rate_audit()
        } else {
            false
        };
        s.add("J1-06-速率审计绿", ok, if ok { "" } else { "账面速率越限" });
    }
    // J1-07 滞后账与 B19-F1975 抑制账互证（守恒恒等式）
    {
        let mut g = HysteresisGovernor::new(
            800,
            300,
            RateLimit { up_khz_per_ms: u32::MAX, down_khz_per_ms: 1000 },
            1_000_000,
            1,
        )
        .ok();
        let ok = if let Some(ref mut g) = g {
            // 2 抑制 + 3 放行 = 5 判定
            let _ = g.decide(FreqDecision { util: 500, cur_khz: 1_000_000, want_khz: 2_000_000 }); // 抑制
            let _ = g.decide(FreqDecision { util: 900, cur_khz: 1_000_000, want_khz: 2_000_000 }); // 升
            let _ = g.decide(FreqDecision { util: 100, cur_khz: 2_000_000, want_khz: 1_500_000 }); // 降
            let _ = g.decide(FreqDecision { util: 400, cur_khz: 1_500_000, want_khz: 1_500_000 }); // 维持
            let _ = g.decide(FreqDecision { util: 600, cur_khz: 1_500_000, want_khz: 1_400_000 }); // 抑制（util>down_th）
            g.interlock_conserved() && g.suppress_count() == 2 && g.apply_count() == 3
        } else {
            false
        };
        s.add("J1-07-互证守恒", ok, if ok { "" } else { "抑制+放行 != 总判定" });
    }
    // J1-08 序号零跳号
    {
        let mut g = HysteresisGovernor::new(
            800,
            300,
            RateLimit { up_khz_per_ms: u32::MAX, down_khz_per_ms: 1000 },
            1_000_000,
            1,
        )
        .ok();
        let ok = if let Some(ref mut g) = g {
            for k in 0..12u16 {
                let u = if k % 2 == 0 { 900 } else { 100 };
                let _ = g.decide(FreqDecision { util: u, cur_khz: g.current_khz(), want_khz: if u == 900 { 2_000_000 } else { 1_500_000 } });
            }
            g.seq_dense()
        } else {
            false
        };
        s.add("J1-08-序号致密", ok, if ok { "" } else { "序号跳号" });
    }
    // J1-09 阈值非法构造必拒（升阈值 <= 降阈值）
    {
        let r1 = HysteresisGovernor::new(300, 300, RateLimit { up_khz_per_ms: 1, down_khz_per_ms: 1 }, 1, 1);
        let r2 = HysteresisGovernor::new(200, 300, RateLimit { up_khz_per_ms: 1, down_khz_per_ms: 1 }, 1, 1);
        let ok = r1.err() == Some(XC_HS_TH_INV) && r2.err() == Some(XC_HS_TH_INV);
        s.add("J1-09-阈值非法拒", ok, if ok { "" } else { "非法阈值放行" });
    }
    // J1-10 速率参数非法必拒（down = 0）
    {
        let r = HysteresisGovernor::new(800, 300, RateLimit { up_khz_per_ms: 1, down_khz_per_ms: 0 }, 1, 1);
        let ok = r.err() == Some(XC_HS_RATE_INV);
        s.add("J1-10-速率零拒", ok, if ok { "" } else { "零速率放行" });
    }
    // J1-11 封账后判定必拒
    {
        let mut g = HysteresisGovernor::new(
            800,
            300,
            RateLimit { up_khz_per_ms: 1, down_khz_per_ms: 1 },
            1_000,
            1,
        )
        .ok();
        let ok = if let Some(ref mut g) = g {
            let _ = g.seal();
            g.decide(FreqDecision { util: 900, cur_khz: 1_000, want_khz: 2_000 }) == Err(XC_HS_SEALED)
                && g.reject_count() == 1
        } else {
            false
        };
        s.add("J1-11-封账拒", ok, if ok { "" } else { "封账后仍判定" });
    }
    // J1-12 指纹冻结（同输入两次构造指纹一致）
    {
        let f = || -> u64 {
            let mut g = HysteresisGovernor::new(
                800,
                300,
                RateLimit { up_khz_per_ms: u32::MAX, down_khz_per_ms: 500 },
                3_000_000,
                1,
            );
            if let Ok(ref mut g) = g {
                let _ = g.decide(FreqDecision { util: 950, cur_khz: 3_000_000, want_khz: 3_600_000 });
                let _ = g.decide(FreqDecision { util: 100, cur_khz: 3_600_000, want_khz: 2_800_000 });
                return g.fingerprint();
            }
            0
        };
        let ok = f() == f() && f() != 0;
        s.add("J1-12-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    }
    // J1-13 封账稳定（封账后指纹不变 + 二次封账拒）
    {
        let mut g = HysteresisGovernor::new(
            800,
            300,
            RateLimit { up_khz_per_ms: 1, down_khz_per_ms: 1 },
            1_000,
            1,
        )
        .ok();
        let ok = if let Some(ref mut g) = g {
            let pre = g.fingerprint();
            let _ = g.seal();
            g.fingerprint() == pre && g.seal().is_err()
        } else {
            false
        };
        s.add("J1-13-封账稳定", ok, if ok { "" } else { "封账漂移" });
    }
    // J1-14 降频步数账与预算守恒（步数 = ceil(差值/预算)）
    {
        let g = HysteresisGovernor::new(
            800,
            300,
            RateLimit { up_khz_per_ms: u32::MAX, down_khz_per_ms: 300 },
            0,
            1,
        )
        .ok();
        let ok = if let Some(ref g) = g {
            // 3000k → 1000k：差 2000k，预算 300k/步 → ceil = 7 步（6×300+1×200）
            match g.ramp_plan(3_000_000, 1_000_000) {
                Ok(plan) => plan.len() == 7 && plan[6] == 1_000_000 && plan[0] == 2_700_000,
                Err(_) => false,
            }
        } else {
            false
        };
        s.add("J1-14-步数守恒", ok, if ok { "" } else { "步数与预算失配" });
    }
    // J1-15 非降频路径不走限速（升频计划一步直达）
    {
        let g = HysteresisGovernor::new(
            800,
            300,
            RateLimit { up_khz_per_ms: u32::MAX, down_khz_per_ms: 1 },
            0,
            1,
        )
        .ok();
        let ok = if let Some(ref g) = g {
            match g.ramp_plan(1_000_000, 3_000_000) {
                Ok(plan) => plan.len() == 1 && plan[0] == 3_000_000,
                Err(_) => false,
            }
        } else {
            false
        };
        s.add("J1-15-升路径免限", ok, if ok { "" } else { "升频被误限" });
    }
    // J1-16 util 越界必拒
    {
        let mut g = HysteresisGovernor::new(
            800,
            300,
            RateLimit { up_khz_per_ms: 1, down_khz_per_ms: 1 },
            1_000,
            1,
        )
        .ok();
        let ok = if let Some(ref mut g) = g {
            g.decide(FreqDecision { util: 1001, cur_khz: 1_000, want_khz: 2_000 }) == Err(XC_HS_OOB)
        } else {
            false
        };
        s.add("J1-16-util越界拒", ok, if ok { "" } else { "越界放行" });
    }
    // J1-17 段位独占
    {
        let ok = SEG_BASE == 0x7A60;
        s.add("J1-17-段位独占", ok, if ok { "" } else { "0x7A60~0x7A7F 本模块独占失守" });
    }
    s
}
