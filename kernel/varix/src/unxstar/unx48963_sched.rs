//! UNX-F48963 高频事件调度压力账 · 0x71440
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B40 族第二槽）：
//!   ① 纯功能代码 ≥380 行；
//!   ② UNX-F48963-J1 压力 ×10 轮归因账可查（尾延迟 P99 按核分布归因
//!      ——攻坚①载体）；
//!   ③ UNX-F48964-J1 均衡 ×10 轮份额断言。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：核号合法性 → 延迟分桶 → 总额守恒 → P99 分位 → 尾部归因 →
//!           份额千分闭合 → 失衡搬移 → 批大小重排 → 份额断言。
//! 先核号再分桶：延迟落在哪个桶由核账定位，核号若非法，桶里的数就归属
//! 不明——归因账的第一要件是「每个数有主」，不是「总数对」。
//! 总额守恒放在分位之前：P99 是总数上的秩次，总数不守恒时分位数会漂到
//! 一个「看上去合理」的桶里，而那种错误不会自己暴露。
//! 归因放在分位之后：P99 定桶，桶及以上才是尾部；顺序反了就会拿一个
//! 未定的桶去算份额，份额自然怎么算都「闭合」。
//! 千分闭合先于失衡搬移：归因份额必须精确加总到 1000（最大余数法），
//! 否则「多数核占大头」这类结论会在舍入上失真，均衡策略就失去依据。
//! 批大小重排放最后：均衡改变的是核间份额，批大小只是份额的派生量；
//! 先搬移再重排，才能证明「不均时批量递交调整」真的跟着份额走。
//! 关键差异：本模块只回答「压力下 P99 落在哪个桶、按核怎么分、失衡怎么
//!           搬」，不做队列水位背压（B40-BACKPRESSURE）、不做批大小实测
//!           矩阵（B40-BACKPRESSURE）、不做 1,000Hz 计数对账（B40-RATE）。
//!           延迟值是确定性注入的微秒数，不是实测采样——本模块记的是
//!           归因账，不是性能曲线（曲线归 F48968）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71440，B40 族第二槽）。
pub const SEG_BASE: u32 = 0x71440;

/// 参与调度的逻辑核数（压力账按此规模分桶）。
pub const CORES: usize = 8;
/// 压力轮数（卡面 ×10）。
pub const STRESS_ROUNDS: u32 = 10;
/// 单轮事件数（够把 P99 推到稳定桶）。
pub const EVENTS_PER_ROUND: u32 = 10_000;
/// 延迟直方图桶数。
pub const BUCKETS: usize = 16;
/// 单桶宽度（微秒）。
pub const BUCKET_WIDTH_US: u32 = 8;
/// 延迟上限（微秒）= 桶数 × 桶宽；超过即判越界。
pub const LATENCY_CAP_US: u32 = (BUCKETS as u32) * BUCKET_WIDTH_US;
/// P99 的分子/分母（整数分位，无浮点）。
pub const P99_NUM: u32 = 99;
pub const P99_DEN: u32 = 100;
/// P50 的分子（压力账里用于单调性对照）。
pub const P50_NUM: u32 = 50;
/// 均衡轮数（卡面 ×10）。
pub const BALANCE_ROUNDS: u32 = 10;
/// 份额容差：搬移后最大值与最小值之差不得超过 1。
pub const SHARE_SLACK: u32 = 1;
/// 批大小下界/上界与折算除数。
pub const BATCH_MIN: u32 = 16;
pub const BATCH_MAX: u32 = 512;
pub const BATCH_DIV: u32 = 16;
/// 压力账渲染字段数。
pub const STRESS_FIELDS: usize = 6;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0C） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchedCode {
    /// 核号非法（零 / 越过 CORES）
    CoreBad,
    /// 延迟越界（超出 128µs 上限）
    TailBad,
    /// 归因非法（桶归属与 P99 不一致）
    AttributeBad,
    /// 份额非法（千分和不闭合或越出容差）
    ShareBad,
    /// 批大小非法（重排后与份额不自洽）
    BatchBad,
    /// 均衡非法（搬移不收敛或总额改变）
    BalanceBad,
    /// 次序非法（计数回退）
    OrderBad,
    /// 账不平（核计数之和 ≠ 总额）
    LedgerBad,
    /// 渲染非法（字段缺失/回读失败）
    RenderBad,
    /// 复算不一致（同压力重放不同）
    ReplayBad,
    /// 覆盖不足（轮数或事件数不足）
    CoverageBad,
    /// 段位越界（诊断码逃出 0x71440 窗口）
    SegmentBad,
}

impl SchedCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            SchedCode::CoreBad => "核号非法：零核或越过 CORES 上界",
            SchedCode::TailBad => "延迟越界：超过 128µs 上限或尾部为空",
            SchedCode::AttributeBad => "归因非法：尾部桶与 P99 定桶不一致",
            SchedCode::ShareBad => "份额非法：千分和不闭合或越出 ±1 容差",
            SchedCode::BatchBad => "批大小非法：重排后与核份额不自洽",
            SchedCode::BalanceBad => "均衡非法：搬移不收敛或总额改变",
            SchedCode::OrderBad => "次序非法：核计数回退",
            SchedCode::LedgerBad => "账不平：核计数之和 ≠ 事件总额",
            SchedCode::RenderBad => "渲染非法：字段缺失或回读失败",
            SchedCode::ReplayBad => "复算不一致：同压力重放结果不同",
            SchedCode::CoverageBad => "覆盖不足：轮数或事件数不足",
            SchedCode::SegmentBad => "段位越界：诊断码逃出 0x71440 窗口",
        }
    }

    pub const ALL: [SchedCode; 12] = [
        SchedCode::CoreBad,
        SchedCode::TailBad,
        SchedCode::AttributeBad,
        SchedCode::ShareBad,
        SchedCode::BatchBad,
        SchedCode::BalanceBad,
        SchedCode::OrderBad,
        SchedCode::LedgerBad,
        SchedCode::RenderBad,
        SchedCode::ReplayBad,
        SchedCode::CoverageBad,
        SchedCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < SchedCode::ALL.len() {
        if SchedCode::ALL[i].val() == code {
            return SchedCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 核账（计数 + 延迟直方图） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoreLedger {
    /// 活动核数（1..=CORES）
    pub cores: u8,
    /// 每核事件计数
    pub counts: [u32; CORES],
    /// 每核延迟直方图
    pub hist: [[u32; BUCKETS]; CORES],
    /// 已接受事件总数
    pub total: u32,
    /// 被拒事件数（非法核号 / 越界延迟）
    pub rejected: u32,
    /// 尝试总数 = total + rejected
    pub attempts: u32,
    pub digest: u64,
}

impl CoreLedger {
    pub fn new(cores: u8) -> Result<CoreLedger, u32> {
        if cores == 0 || cores as usize > CORES {
            return Err(SchedCode::CoreBad.val());
        }
        Ok(CoreLedger {
            cores,
            counts: [0; CORES],
            hist: [[0; BUCKETS]; CORES],
            total: 0,
            rejected: 0,
            attempts: 0,
            digest: p_core::FNV_OFF,
        })
    }

    /// 记一次事件：核号与延迟都必须先合法，否则只记 rejected、不入桶。
    ///
    /// 「先校后记」是归因账的底线：非法样本若被塞进某个桶，份额就被
    /// 污染，而污染后的份额仍然会加总到 1000——错而不自知。
    pub fn push(&mut self, core: usize, latency_us: u32) -> Result<(), u32> {
        self.attempts = self.attempts.saturating_add(1);
        if core >= self.cores as usize || core >= CORES {
            self.rejected = self.rejected.saturating_add(1);
            return Err(SchedCode::CoreBad.val());
        }
        let bucket = (latency_us / BUCKET_WIDTH_US) as usize;
        if bucket >= BUCKETS {
            self.rejected = self.rejected.saturating_add(1);
            return Err(SchedCode::TailBad.val());
        }
        if self.counts[core] == u32::MAX {
            self.rejected = self.rejected.saturating_add(1);
            return Err(SchedCode::OrderBad.val());
        }
        self.counts[core] += 1;
        self.hist[core][bucket] = self.hist[core][bucket].saturating_add(1);
        self.total = self.total.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, core as u32);
        self.digest = p_core::fold_u32(self.digest, latency_us);
        Ok(())
    }

    /// 单桶的跨核合计。
    pub fn bucket_count(&self, bucket: usize) -> u32 {
        let mut s = 0u32;
        let mut c = 0usize;
        while c < CORES {
            s = s.saturating_add(match self.hist[c].get(bucket) {
                Some(v) => *v,
                None => 0,
            });
            c += 1;
        }
        s
    }

    /// 整数分位：返回该秩所在桶的**上沿**（微秒）。
    ///
    /// 秩次 = ceil(num×total/den)，用 64 位中间量避免百万级乘法越界。
    /// 返回上沿而不是桶中心：上沿是保守值，尾延迟归因宁可高报不可低报。
    pub fn quantile(&self, num: u32, den: u32) -> Result<u32, u32> {
        if den == 0 || num == 0 || num > den {
            return Err(SchedCode::CoreBad.val());
        }
        if self.total == 0 {
            return Err(SchedCode::CoverageBad.val());
        }
        let prod = (num as u64).saturating_mul(self.total as u64);
        let rank = prod.saturating_add(den as u64 - 1) / den as u64;
        let mut cum = 0u64;
        let mut b = 0usize;
        while b < BUCKETS {
            cum = cum.saturating_add(self.bucket_count(b) as u64);
            if cum >= rank {
                return Ok(((b as u32).saturating_add(1)).saturating_mul(BUCKET_WIDTH_US));
            }
            b += 1;
        }
        Err(SchedCode::TailBad.val())
    }

    pub fn p50(&self) -> Result<u32, u32> {
        self.quantile(P50_NUM, P99_DEN)
    }

    pub fn p99(&self) -> Result<u32, u32> {
        self.quantile(P99_NUM, P99_DEN)
    }

    /// 总额守恒审计：核计数之和、直方图之和、尝试分解三者同时成立。
    pub fn audit(&self) -> Result<(), u32> {
        if self.cores == 0 || self.cores as usize > CORES {
            return Err(SchedCode::CoreBad.val());
        }
        let n = self.cores as usize;
        let mut sum = 0u32;
        let mut hist_sum = 0u32;
        let mut c = 0usize;
        while c < CORES {
            let cnt = match self.counts.get(c) {
                Some(v) => *v,
                None => return Err(SchedCode::CoreBad.val()),
            };
            if c < n {
                sum = sum.saturating_add(cnt);
            } else if cnt != 0 {
                // 非活动核带计数 = 有事件没有主，归因账直接失效。
                return Err(SchedCode::CoreBad.val());
            }
            let mut b = 0usize;
            while b < BUCKETS {
                hist_sum = hist_sum.saturating_add(match self.hist[c].get(b) {
                    Some(v) => *v,
                    None => 0,
                });
                b += 1;
            }
            c += 1;
        }
        if sum != self.total || hist_sum != self.total {
            return Err(SchedCode::LedgerBad.val());
        }
        if self.attempts != self.total.saturating_add(self.rejected) {
            return Err(SchedCode::LedgerBad.val());
        }
        if self.total == 0 {
            return Err(SchedCode::CoverageBad.val());
        }
        Ok(())
    }
}

// ── 尾部归因 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TailAttribution {
    /// P99 上沿（微秒）
    pub p99_us: u32,
    /// P99 所在桶
    pub bucket: usize,
    /// 该桶及以上（尾部）事件总数
    pub tail_total: u32,
    /// 每核落入尾部的事件数
    pub per_core: [u32; CORES],
    /// 每核尾部份额（千分，和恒为 1000）
    pub share_mille: [u32; CORES],
    /// 尾部占比最高的核（并列取最小核号）
    pub top_core: u8,
    pub digest: u64,
}

/// 千分份额（最大余数法：先取整，再按余数从大到小补 1，和必为 1000）。
///
/// 为什么不用四舍五入：8 个核各自四舍五入后总和可能是 997~1003，
/// 「份额断言」就变成一句没有固定真值的空话。最大余数法是唯一使
/// 整数份额同时满足「近似比例」与「精确闭合」的分配。
fn share_mille(per_core: &[u32; CORES], total: u32) -> Result<[u32; CORES], u32> {
    if total == 0 {
        return Err(SchedCode::TailBad.val());
    }
    let mut out = [0u32; CORES];
    let mut rem = [0u32; CORES];
    let mut awarded = [0u8; CORES];
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < CORES {
        let c = match per_core.get(i) {
            Some(v) => *v,
            None => return Err(SchedCode::ShareBad.val()),
        };
        let num = (c as u64).saturating_mul(1_000);
        let base = (num / total as u64) as u32;
        let r = (num % total as u64) as u32;
        out[i] = base;
        rem[i] = r;
        sum = sum.saturating_add(base);
        i += 1;
    }
    let mut left = 1_000u32.saturating_sub(sum);
    while left > 0 {
        let mut best = usize::MAX;
        let mut best_key = 0u64;
        let mut j = 0usize;
        while j < CORES {
            if awarded[j] == 0 {
                // 余数优先，等余数时核号小者优先（确定性平局裁决）。
                let key = (rem[j] as u64)
                    .saturating_mul(CORES as u64 + 1)
                    .saturating_add(CORES as u64 - j as u64);
                if best == usize::MAX || key > best_key {
                    best_key = key;
                    best = j;
                }
            }
            j += 1;
        }
        if best == usize::MAX {
            return Err(SchedCode::ShareBad.val());
        }
        out[best] = out[best].saturating_add(1);
        awarded[best] = 1;
        left -= 1;
    }
    let mut check = 0u32;
    let mut k = 0usize;
    while k < CORES {
        check = check.saturating_add(out[k]);
        k += 1;
    }
    if check != 1_000 {
        return Err(SchedCode::ShareBad.val());
    }
    Ok(out)
}

impl CoreLedger {
    /// 按 P99 定桶，把该桶及以上的事件按核归因。
    pub fn attribute(&self) -> Result<TailAttribution, u32> {
        if self.total == 0 {
            return Err(SchedCode::CoverageBad.val());
        }
        let p99 = self.p99()?;
        let bucket = (p99 / BUCKET_WIDTH_US).saturating_sub(1) as usize;
        if bucket >= BUCKETS {
            return Err(SchedCode::TailBad.val());
        }
        let mut per_core = [0u32; CORES];
        let mut tail_total = 0u32;
        let mut c = 0usize;
        while c < CORES {
            let mut b = bucket;
            let mut s = 0u32;
            while b < BUCKETS {
                s = s.saturating_add(match self.hist[c].get(b) {
                    Some(v) => *v,
                    None => 0,
                });
                b += 1;
            }
            per_core[c] = s;
            tail_total = tail_total.saturating_add(s);
            c += 1;
        }
        if tail_total == 0 {
            return Err(SchedCode::TailBad.val());
        }
        let share = share_mille(&per_core, tail_total)?;
        let mut top = 0usize;
        let mut t = 1usize;
        while t < self.cores as usize && t < CORES {
            if per_core[t] > per_core[top] {
                top = t;
            }
            t += 1;
        }
        let mut d = p_core::fold_u32(p_core::FNV_OFF, p99);
        d = p_core::fold_u32(d, bucket as u32);
        d = p_core::fold_u32(d, tail_total);
        let mut k = 0usize;
        while k < CORES {
            d = p_core::fold_u32(d, per_core[k]);
            k += 1;
        }
        Ok(TailAttribution {
            p99_us: p99,
            bucket,
            tail_total,
            per_core,
            share_mille: share,
            top_core: top as u8,
            digest: d,
        })
    }
}

// ── 压力账（×10 轮归因） ──────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StressReport {
    pub rounds: u32,
    pub events: u32,
    pub p99_max_us: u32,
    pub p99_min_us: u32,
    pub top_core: u8,
    /// 各轮尾部事件数之和（归因账可查的最小凭证）
    pub attributed: u32,
    pub digest: u64,
}

/// 压力 ×10 轮：每轮重新建账，逐轮复算总额、分位与归因份额。
pub fn stress_rounds(rounds: u32) -> Result<StressReport, u32> {
    if rounds == 0 || rounds > 100 {
        return Err(SchedCode::CoverageBad.val());
    }
    let mut p99_max = 0u32;
    let mut p99_min = u32::MAX;
    let mut attributed = 0u32;
    let mut top = 0u8;
    let mut h = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        let mut l = CoreLedger::new(CORES as u8)?;
        let mut i = 0u32;
        while i < EVENTS_PER_ROUND {
            // 热门核 0 吃 1/5 的流量，其余按轮相位轮转：制造可控不均。
            let core = if i % 5 == 0 {
                0usize
            } else {
                ((i as usize).wrapping_add(r as usize)) % CORES
            };
            let base = (i
                .wrapping_mul(7)
                .wrapping_add(r.wrapping_mul(13))
                % 89)
                .saturating_add(1);
            // 每 1013 个事件一个长尾尖峰（约 0.1%），把 P99 推离主体分布。
            // 尖峰固定落在 100..=127：必须留在 16 桶域内，越界样本会被
            // 拒账，压力账就退化成「拒账账」，归因也就无从谈起。
            let lat = if i % 1_013 == 0 {
                100u32.saturating_add(r % 28)
            } else {
                base
            };
            l.push(core, lat)?;
            i = i.saturating_add(1);
        }
        l.audit()?;
        if l.total != EVENTS_PER_ROUND {
            return Err(SchedCode::LedgerBad.val());
        }
        let p99 = l.p99()?;
        let att = l.attribute()?;
        if att.p99_us != p99 {
            return Err(SchedCode::AttributeBad.val());
        }
        let mut tsum = 0u32;
        let mut ssum = 0u32;
        let mut c = 0usize;
        while c < CORES {
            tsum = tsum.saturating_add(att.per_core[c]);
            ssum = ssum.saturating_add(att.share_mille[c]);
            c += 1;
        }
        if tsum != att.tail_total || ssum != 1_000 {
            return Err(SchedCode::ShareBad.val());
        }
        if p99 == 0 || p99 > LATENCY_CAP_US {
            return Err(SchedCode::TailBad.val());
        }
        if p99 > p99_max {
            p99_max = p99;
        }
        if p99 < p99_min {
            p99_min = p99;
        }
        attributed = attributed.saturating_add(att.tail_total);
        top = att.top_core;
        h = p_core::fold_u32(h, p99);
        h = p_core::fold_u32(h, att.top_core as u32);
        h = p_core::fold_u32(h, att.tail_total);
        r = r.saturating_add(1);
    }
    if attributed == 0 {
        return Err(SchedCode::TailBad.val());
    }
    Ok(StressReport {
        rounds,
        events: rounds.saturating_mul(EVENTS_PER_ROUND),
        p99_max_us: p99_max,
        p99_min_us: p99_min,
        top_core: top,
        attributed,
        digest: h,
    })
}

/// 核账 fuzz：随机核号与随机延迟下，守恒与分位必须始终成立。
pub fn fuzz_cores_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 {
        return Err(SchedCode::CoverageBad.val());
    }
    let mut l = CoreLedger::new(CORES as u8)?;
    let mut rng = p_core::Lcg::new(0x8A11_0063);
    let mut i = 0u32;
    while i < rounds {
        // 路由取 LCG 高位：低位周期 2^k，`below(8)` 会锁死在少数核上。
        let core = ((rng.next() >> 33) % CORES as u64) as usize;
        let lat = rng.below(LATENCY_CAP_US);
        l.push(core, lat)?;
        i = i.saturating_add(1);
    }
    l.audit()?;
    let _ = l.p99()?;
    let att = l.attribute()?;
    let mut s = 0u32;
    let mut k = 0usize;
    while k < CORES {
        s = s.saturating_add(att.share_mille[k]);
        k += 1;
    }
    if s != 1_000 {
        return Err(SchedCode::ShareBad.val());
    }
    Ok(l.digest)
}

// ── per-CPU 均衡与批大小重排 ──────────────────────────────────────
/// 单核批大小：份额越大批越大（减少唤醒），上下界夹紧。
pub fn batch_for(count: u32) -> u32 {
    let v = count / BATCH_DIV;
    if v < BATCH_MIN {
        BATCH_MIN
    } else if v > BATCH_MAX {
        BATCH_MAX
    } else {
        v
    }
}

/// 活动核批大小表（非活动核为 0，与核账口径一致）。
pub fn batch_plan(counts: &[u32; CORES], cores: u8) -> [u32; CORES] {
    let mut out = [0u32; CORES];
    let mut i = 0usize;
    let n = cores as usize;
    while i < CORES {
        if i < n {
            out[i] = match counts.get(i) {
                Some(v) => batch_for(*v),
                None => 0,
            };
        }
        i += 1;
    }
    out
}

/// 活动核批大小的极差。
pub fn batch_spread(counts: &[u32; CORES], cores: u8) -> u32 {
    let plan = batch_plan(counts, cores);
    let n = if cores as usize > CORES {
        CORES
    } else {
        cores as usize
    };
    if n == 0 {
        return 0;
    }
    let mut mx = plan[0];
    let mut mn = plan[0];
    let mut i = 1usize;
    while i < n {
        if plan[i] > mx {
            mx = plan[i];
        }
        if plan[i] < mn {
            mn = plan[i];
        }
        i = i.saturating_add(1);
    }
    mx.saturating_sub(mn)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BalancePlan {
    pub cores: u8,
    /// 搬移后的核计数
    pub counts: [u32; CORES],
    pub total: u32,
    /// 实际搬移的事件数
    pub moved: u32,
    /// 搬移前批大小
    pub batches_before: [u32; CORES],
    /// 搬移后批大小
    pub batches: [u32; CORES],
    pub spread_before: u32,
    pub spread_after: u32,
    pub digest: u64,
}

impl BalancePlan {
    /// 份额断言：活动核计数极差 ≤ 容差，且总额分毫不差。
    pub fn share_ok(&self) -> bool {
        let n = self.cores as usize;
        if n == 0 || n > CORES {
            return false;
        }
        let mut sum = 0u32;
        let mut mx = self.counts[0];
        let mut mn = self.counts[0];
        let mut i = 0usize;
        while i < n {
            sum = sum.saturating_add(self.counts[i]);
            if self.counts[i] > mx {
                mx = self.counts[i];
            }
            if self.counts[i] < mn {
                mn = self.counts[i];
            }
            i = i.saturating_add(1);
        }
        sum == self.total && mx.saturating_sub(mn) <= SHARE_SLACK
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.share_ok() {
            return Err(SchedCode::ShareBad.val());
        }
        if self.spread_before > SHARE_SLACK && self.moved == 0 {
            return Err(SchedCode::BalanceBad.val());
        }
        if self.spread_after > self.spread_before {
            return Err(SchedCode::BatchBad.val());
        }
        if self.spread_after != batch_spread(&self.counts, self.cores) {
            return Err(SchedCode::BatchBad.val());
        }
        Ok(())
    }
}

/// 失衡搬移：把最重核的事件搬给最轻核，直到极差 ≤ 容差。
///
/// 每次搬 `gap/2`：一步跨掉一半差距，收敛轮数是 O(log gap)；若每步只搬
/// 1，百万级事件的均衡会退化成百万次循环——那是压力账自身制造的尾延迟。
pub fn balance_counts(counts: [u32; CORES], cores: u8) -> Result<BalancePlan, u32> {
    if cores == 0 || cores as usize > CORES {
        return Err(SchedCode::CoreBad.val());
    }
    let n = cores as usize;
    let mut total = 0u32;
    let mut i = 0usize;
    while i < n {
        total = total.saturating_add(match counts.get(i) {
            Some(v) => *v,
            None => return Err(SchedCode::CoreBad.val()),
        });
        i += 1;
    }
    if total == 0 {
        return Err(SchedCode::CoverageBad.val());
    }
    // 非活动核不得预先带计数：否则守恒会被「看不见的核」补平。
    let mut k = n;
    while k < CORES {
        if let Some(v) = counts.get(k) {
            if *v != 0 {
                return Err(SchedCode::CoreBad.val());
            }
        }
        k += 1;
    }
    let batches_before = batch_plan(&counts, cores);
    let spread_before = batch_spread(&counts, cores);
    let mut w = counts;
    let mut moved = 0u32;
    let mut guard = 0u32;
    let limit = total.saturating_add(n as u32).saturating_add(2);
    loop {
        let mut mx = 0usize;
        let mut mn = 0usize;
        let mut j = 0usize;
        while j < n {
            if w[j] > w[mx] {
                mx = j;
            }
            if w[j] < w[mn] {
                mn = j;
            }
            j += 1;
        }
        let gap = w[mx].saturating_sub(w[mn]);
        if gap <= SHARE_SLACK {
            break;
        }
        let step = gap / 2;
        w[mx] = w[mx].saturating_sub(step);
        w[mn] = w[mn].saturating_add(step);
        moved = moved.saturating_add(step);
        guard = guard.saturating_add(1);
        if guard > limit {
            return Err(SchedCode::BalanceBad.val());
        }
    }
    let mut sum = 0u32;
    let mut q = 0usize;
    while q < n {
        sum = sum.saturating_add(w[q]);
        q += 1;
    }
    if sum != total {
        return Err(SchedCode::BalanceBad.val());
    }
    let batches = batch_plan(&w, cores);
    let spread_after = batch_spread(&w, cores);
    let mut d = p_core::FNV_OFF;
    let mut z = 0usize;
    while z < CORES {
        d = p_core::fold_u32(d, w[z]);
        d = p_core::fold_u32(d, batches[z]);
        z += 1;
    }
    let plan = BalancePlan {
        cores,
        counts: w,
        total,
        moved,
        batches_before,
        batches,
        spread_before,
        spread_after,
        digest: d,
    };
    plan.audit()?;
    Ok(plan)
}

/// 均衡 ×10 轮：每轮换一组可控不均，搬移后份额必须落在容差内。
pub fn balance_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 || rounds > 100 {
        return Err(SchedCode::CoverageBad.val());
    }
    let mut h = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        let total = 8_000u32.saturating_add(r.saturating_mul(137));
        let n = CORES as u32;
        let mut counts = [0u32; CORES];
        // 热门核 40%、次热核 20%，其余按整数分摊——总额精确等于 total。
        let hot = total.saturating_mul(40) / 100;
        let sub = total.saturating_mul(20) / 100;
        let rest = total.saturating_sub(hot).saturating_sub(sub);
        let each = rest / (n - 2);
        let mut rem = rest % (n - 2);
        counts[0] = hot;
        counts[1] = sub;
        let mut c = 2usize;
        while c < CORES {
            counts[c] = each;
            if rem > 0 {
                counts[c] = counts[c].saturating_add(1);
                rem -= 1;
            }
            c += 1;
        }
        let plan = balance_counts(counts, CORES as u8)?;
        if !plan.share_ok() {
            return Err(SchedCode::ShareBad.val());
        }
        if plan.moved == 0 || plan.spread_before == 0 {
            return Err(SchedCode::BalanceBad.val());
        }
        h = fold_u64_balance(h, plan.digest, plan.moved);
        r = r.saturating_add(1);
    }
    Ok(h)
}

/// 折叠辅助：把「搬移量」与「子摘要」混入同一个 64 位链。
///
/// 直接 `fold_u32(h, digest as u32)` 只保留低 32 位，两个相差 2^32 的
/// 子摘要会折叠成同一个值——复算判据就会被这个碰撞蒙住。
fn fold_u64_balance(h: u64, digest: u64, moved: u32) -> u64 {
    let mut x = p_core::fold_u32(h, (digest >> 32) as u32);
    x = p_core::fold_u32(x, digest as u32);
    p_core::fold_u32(x, moved)
}

// ── 压力账渲染 ────────────────────────────────────────────────────
pub fn render_stress(r: &StressReport) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        r.rounds, r.events, r.p99_max_us, r.p99_min_us, r.top_core, r.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(SchedCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(SchedCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(SchedCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(SchedCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(SchedCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(SchedCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(SchedCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(SchedCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != STRESS_FIELDS {
        return Err(SchedCode::RenderBad.val());
    }
    let rounds = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(SchedCode::RenderBad.val()),
    })?;
    let events = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(SchedCode::RenderBad.val()),
    })?;
    let pmax = parse_u32(match parts.get(2) {
        Some(v) => v,
        None => return Err(SchedCode::RenderBad.val()),
    })?;
    let pmin = parse_u32(match parts.get(3) {
        Some(v) => v,
        None => return Err(SchedCode::RenderBad.val()),
    })?;
    let top = parse_u32(match parts.get(4) {
        Some(v) => v,
        None => return Err(SchedCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(5) {
        Some(v) => v,
        None => return Err(SchedCode::RenderBad.val()),
    })?;
    if rounds == 0 || rounds > 100 {
        return Err(SchedCode::RenderBad.val());
    }
    if events != rounds.saturating_mul(EVENTS_PER_ROUND) {
        return Err(SchedCode::LedgerBad.val());
    }
    if pmax == 0 || pmax > LATENCY_CAP_US || pmin == 0 || pmin > pmax {
        return Err(SchedCode::TailBad.val());
    }
    if top as usize >= CORES {
        return Err(SchedCode::CoreBad.val());
    }
    if digest == 0 {
        return Err(SchedCode::RenderBad.val());
    }
    Ok(rounds)
}

pub fn golden_run() -> Result<(u32, u32, u32, u8, u64), u32> {
    let r = stress_rounds(STRESS_ROUNDS)?;
    let p = balance_rounds(BALANCE_ROUNDS)?;
    let text = render_stress(&r);
    if audit_render(&text)? != r.rounds {
        return Err(SchedCode::ReplayBad.val());
    }
    if p == 0 {
        return Err(SchedCode::BalanceBad.val());
    }
    Ok((r.rounds, r.events, r.p99_max_us, r.top_core, r.digest ^ p))
}

// ── 注入对账（四路缺陷必须被捕获） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 {
        return Err(SchedCode::CoverageBad.val());
    }
    let mut log = InjectLog {
        tries: [0; 4],
        caught: [0; 4],
    };
    let mut i = 0u32;
    while i < rounds {
        // ① 非法核号必须被拒，且不得入任何桶。
        let mut l = CoreLedger::new(CORES as u8)?;
        if l.push(CORES, 10).err() == Some(SchedCode::CoreBad.val()) && l.total == 0 {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);
        // ② 越界延迟必须被拒。
        let mut t = CoreLedger::new(CORES as u8)?;
        if t.push(0, LATENCY_CAP_US).err() == Some(SchedCode::TailBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);
        // ③ 人为打破总额守恒必须炸 LedgerBad。
        let mut c = CoreLedger::new(2)?;
        let _ = c.push(0, 5)?;
        c.total = c.total.saturating_add(1);
        if c.audit().err() == Some(SchedCode::LedgerBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);
        // ④ 渲染篡改必须被拒。
        let s = StressReport {
            rounds: 1,
            events: EVENTS_PER_ROUND,
            p99_max_us: 112,
            p99_min_us: 112,
            top_core: 0,
            attributed: 1,
            digest: 0x1234_5678_9abc_def0,
        };
        let bad = render_stress(&s).replacen("0x", "0y", 1);
        if audit_render(&bad).is_err() {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(SchedCode::CoverageBad.val());
    }
    Ok(log)
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    if CORES != 8 || BUCKETS != 16 || LATENCY_CAP_US != 128 {
        return Err(SchedCode::CoreBad.val());
    }
    let r = stress_rounds(1)?;
    if r.events != EVENTS_PER_ROUND || r.attributed == 0 {
        return Err(SchedCode::LedgerBad.val());
    }
    if r.p99_max_us == 0 || r.p99_max_us > LATENCY_CAP_US {
        return Err(SchedCode::TailBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 99 个 1µs + 1 个 120µs：P50/P99 都落在 0 号桶上沿（8µs），
    // 只有 P100 才够到最后一个桶（128µs）。这个构造专门钉「秩次算法」：
    // 若把 ceil 写成 floor，P99 的秩会从 99 掉到 98，仍在本例的桶 0 内，
    // 所以另加一例：99 个 1µs + 1 个 120µs 不算边界，下面用 98+2 校验。
    let mut l = CoreLedger::new(1)?;
    let mut i = 0u32;
    while i < 99 {
        l.push(0, 1)?;
        i += 1;
    }
    l.push(0, 120)?;
    l.audit()?;
    if l.p50()? != BUCKET_WIDTH_US || l.p99()? != BUCKET_WIDTH_US {
        return Err(SchedCode::TailBad.val());
    }
    if l.quantile(P99_DEN, P99_DEN)? != LATENCY_CAP_US {
        return Err(SchedCode::TailBad.val());
    }
    // 98×1µs + 2×120µs：rank=ceil(99×100/100)=99 → 第 99 个落在 120µs 桶。
    let mut m = CoreLedger::new(1)?;
    let mut k = 0u32;
    while k < 98 {
        m.push(0, 1)?;
        k += 1;
    }
    m.push(0, 120)?;
    m.push(0, 120)?;
    if m.p99()? != LATENCY_CAP_US {
        return Err(SchedCode::TailBad.val());
    }
    let att = m.attribute()?;
    if att.bucket != BUCKETS - 1 || att.tail_total != 2 || att.share_mille[0] != 1_000 {
        return Err(SchedCode::AttributeBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let mut counts = [0u32; CORES];
    counts[0] = 4_000;
    counts[1] = 2_000;
    let mut c = 2usize;
    while c < CORES {
        counts[c] = 500;
        c += 1;
    }
    let plan = balance_counts(counts, CORES as u8)?;
    if !plan.share_ok() || plan.moved == 0 {
        return Err(SchedCode::ShareBad.val());
    }
    if plan.spread_before == 0 || plan.spread_after != 0 {
        return Err(SchedCode::BatchBad.val());
    }
    // 搬移前热门核批大小必须显著更大，否则「批量调整跟着份额走」不成立。
    if plan.batches_before[0] <= plan.batches_before[7] {
        return Err(SchedCode::BatchBad.val());
    }
    if balance_rounds(BALANCE_ROUNDS)? == 0 {
        return Err(SchedCode::BalanceBad.val());
    }
    if !segment_guard() {
        return Err(SchedCode::SegmentBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = stress_rounds(2)?;
    let b = stress_rounds(2)?;
    if a.digest != b.digest || a.events != b.events || a.attributed != b.attributed {
        return Err(SchedCode::ReplayBad.val());
    }
    if fuzz_cores_rounds(300)? != fuzz_cores_rounds(300)? {
        return Err(SchedCode::ReplayBad.val());
    }
    if balance_rounds(BALANCE_ROUNDS)? != balance_rounds(BALANCE_ROUNDS)? {
        return Err(SchedCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < SchedCode::ALL.len() {
        let v = SchedCode::ALL[i].val();
        if v <= SEG_BASE || v > SEG_BASE + 0x3F {
            return false;
        }
        i += 1;
    }
    true
}

// ── 域检查项（≤8 条） ─────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    out.push((
        "F48963 核与桶锚定",
        CORES == 8
            && BUCKETS == 16
            && LATENCY_CAP_US == 128
            && P99_NUM == 99
            && P99_DEN == 100,
        format!(
            "cores={} buckets={} cap={}us p99={}/{}",
            CORES, BUCKETS, LATENCY_CAP_US, P99_NUM, P99_DEN
        ),
    ));

    let stress = stress_rounds(STRESS_ROUNDS);
    out.push((
        "F48963 压力×10归因可查",
        matches!(&stress, Ok(r) if r.rounds == STRESS_ROUNDS
            && r.events == STRESS_ROUNDS * EVENTS_PER_ROUND
            && r.attributed > 0
            && r.p99_max_us <= LATENCY_CAP_US),
        match &stress {
            Ok(r) => format!(
                "rounds={} events={} p99=[{},{}] top={} att={}",
                r.rounds, r.events, r.p99_min_us, r.p99_max_us, r.top_core, r.attributed
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // 守恒与单调合并：两者都在同一本账上复算，分开报只会让同一根因占两槽。
    let mut mono_ok = false;
    let mut mono_total = 0u32;
    let mut mono_p50 = 0u32;
    let mut mono_p99 = 0u32;
    if let Ok(mut led) = CoreLedger::new(CORES as u8) {
        let mut i = 0u32;
        while i < 4_000 {
            let core = (i as usize) % CORES;
            let lat = (i % 89).saturating_add(1);
            let _ = led.push(core, lat);
            i = i.saturating_add(1);
        }
        if let (Ok(p50), Ok(p99)) = (led.p50(), led.p99()) {
            mono_p50 = p50;
            mono_p99 = p99;
            mono_ok = led.audit().is_ok() && p99 >= p50;
        }
        mono_total = led.total;
    }
    out.push((
        "F48963 核总额守恒与分位单调",
        mono_ok && mono_total == 4_000,
        format!(
            "total={} p50={} p99={} ok={}",
            mono_total, mono_p50, mono_p99, mono_ok
        ),
    ));

    let att = CoreLedger::new(CORES as u8).and_then(|mut x| {
        let mut i = 0u32;
        while i < 2_000 {
            let core = ((i as usize) * 7) % CORES;
            let lat = if i % 271 == 0 { 120 } else { (i % 89).saturating_add(1) };
            x.push(core, lat)?;
            i += 1;
        }
        x.attribute()
    });
    out.push((
        "F48963 份额千分闭合",
        matches!(&att, Ok(a) if a.share_mille.iter().sum::<u32>() == 1_000
            && a.tail_total > 0 && (a.top_core as usize) < CORES),
        match &att {
            Ok(a) => format!(
                "p99={} bucket={} tail={} top={} share0={}",
                a.p99_us, a.bucket, a.tail_total, a.top_core, a.share_mille[0]
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let bal = balance_rounds(BALANCE_ROUNDS);
    out.push((
        "F48964 均衡×10份额断言",
        bal.is_ok(),
        match &bal {
            Ok(h) => format!("rounds={} digest=0x{:016X}", BALANCE_ROUNDS, h),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut skew = [0u32; CORES];
    skew[0] = 6_000;
    skew[1] = 3_000;
    let mut z = 2usize;
    while z < CORES {
        skew[z] = 250;
        z += 1;
    }
    let plan = balance_counts(skew, CORES as u8);
    out.push((
        "F48964 批量递交跟随份额",
        matches!(&plan, Ok(p) if p.share_ok()
            && p.spread_before > p.spread_after
            && p.batches_before[0] > p.batches_before[7]
            && p.spread_after == 0),
        match &plan {
            Ok(p) => format!(
                "moved={} spread {}->{} batch {}/{}",
                p.moved, p.spread_before, p.spread_after,
                p.batches_before[0], p.batches_before[7]
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let golden = golden_run();
    out.push((
        "F48963 账本可解析",
        golden.is_ok(),
        match &golden {
            Ok((r, e, p, t, d)) => format!("rounds={} events={} p99={} top={} d=0x{:X}", r, e, p, t, d),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48963 段位闭集",
        segment_guard(),
        format!("codes={} seg=0x{:04X}", SchedCode::ALL.len(), SEG_BASE),
    ));

    out
}

pub fn run_unx48963_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48963 高频事件调度压力账");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn ledger8() -> CoreLedger {
        let empty = CoreLedger {
            cores: 1,
            counts: [0; CORES],
            hist: [[0; BUCKETS]; CORES],
            total: 0,
            rejected: 0,
            attempts: 0,
            digest: 1,
        };
        match CoreLedger::new(CORES as u8) {
            Ok(v) => v,
            Err(_) => empty,
        }
    }

    #[test]
    fn constants_are_anchored() {
        assert_eq!(SEG_BASE, 0x71440);
        assert_eq!(CORES, 8);
        assert_eq!(LATENCY_CAP_US, 128);
        assert_eq!(SHARE_SLACK, 1);
        assert_eq!(STRESS_FIELDS, 6);
    }

    #[test]
    fn ledger_conserves_total() {
        let mut l = ledger8();
        let mut i = 0u32;
        while i < 800 {
            let _ = l.push((i as usize) % CORES, i % 128);
            i += 1;
        }
        assert_eq!(l.total, 800);
        assert!(l.audit().is_ok());
        let mut sum = 0u32;
        let mut c = 0usize;
        while c < CORES {
            sum += l.counts[c];
            c += 1;
        }
        assert_eq!(sum, l.total);
    }

    #[test]
    fn ledger_rejects_bad_core() {
        let mut l = ledger8();
        assert_eq!(l.push(CORES, 10), Err(SchedCode::CoreBad.val()));
        assert_eq!(l.total, 0);
        assert_eq!(l.rejected, 1);
        assert_eq!(l.attempts, 1);
    }

    #[test]
    fn ledger_rejects_over_cap_latency() {
        let mut l = ledger8();
        assert_eq!(l.push(0, LATENCY_CAP_US), Err(SchedCode::TailBad.val()));
        assert_eq!(l.total, 0);
    }

    #[test]
    fn rejected_are_accounted() {
        let mut l = ledger8();
        let _ = l.push(0, 8);
        let _ = l.push(9, 8);
        let _ = l.push(0, 999);
        assert_eq!(l.attempts, 3);
        assert_eq!(l.total, 1);
        assert_eq!(l.rejected, 2);
        assert_eq!(l.attempts, l.total + l.rejected);
    }

    #[test]
    fn quantile_matches_rank() {
        let mut l = match CoreLedger::new(1) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let mut i = 0u32;
        while i < 99 {
            let _ = l.push(0, 1);
            i += 1;
        }
        let _ = l.push(0, 120);
        assert_eq!(l.p50(), Ok(8));
        assert_eq!(l.p99(), Ok(8));
        assert_eq!(l.quantile(100, 100), Ok(128));
    }

    #[test]
    fn quantile_rejects_empty_ledger() {
        let l = match CoreLedger::new(1) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(l.p99(), Err(SchedCode::CoverageBad.val()));
        assert_eq!(l.quantile(0, 100), Err(SchedCode::CoreBad.val()));
        assert_eq!(l.quantile(99, 0), Err(SchedCode::CoreBad.val()));
    }

    #[test]
    fn p99_is_not_below_p50() {
        let mut l = ledger8();
        let mut i = 0u32;
        while i < 5_000 {
            let lat = if i % 500 == 0 { 120 } else { (i % 40) + 1 };
            let _ = l.push((i as usize) % CORES, lat);
            i += 1;
        }
        let p50 = match l.p50() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let p99 = match l.p99() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert!(p99 >= p50);
        assert!(p99 <= LATENCY_CAP_US);
    }

    #[test]
    fn attribution_shares_close_at_1000() {
        let mut l = ledger8();
        let mut i = 0u32;
        while i < 2_000 {
            let lat = if i % 271 == 0 { 120 } else { (i % 89) + 1 };
            let _ = l.push(((i as usize) * 7) % CORES, lat);
            i += 1;
        }
        let att = match l.attribute() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let mut sum = 0u32;
        let mut tails = 0u32;
        let mut c = 0usize;
        while c < CORES {
            sum += att.share_mille[c];
            tails += att.per_core[c];
            c += 1;
        }
        assert_eq!(sum, 1_000);
        assert_eq!(tails, att.tail_total);
        assert!(att.tail_total > 0);
    }

    #[test]
    fn attribution_top_core_is_argmax() {
        let mut l = ledger8();
        let mut i = 0u32;
        while i < 400 {
            // 核 3 独占所有长尾事件。
            if i % 4 == 0 {
                let _ = l.push(3, 120);
            } else {
                let _ = l.push((i as usize) % CORES, 1);
            }
            i += 1;
        }
        let att = match l.attribute() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(att.top_core, 3);
        assert_eq!(att.share_mille[3], 1_000);
    }

    #[test]
    fn stress_ten_rounds() {
        let r = match stress_rounds(STRESS_ROUNDS) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(r.rounds, STRESS_ROUNDS);
        assert_eq!(r.events, STRESS_ROUNDS * EVENTS_PER_ROUND);
        assert!(r.attributed > 0);
        assert!(r.p99_max_us >= r.p99_min_us);
        assert!(r.p99_max_us <= LATENCY_CAP_US);
    }

    #[test]
    fn stress_rejects_zero_and_over() {
        assert_eq!(stress_rounds(0), Err(SchedCode::CoverageBad.val()));
        assert_eq!(stress_rounds(101), Err(SchedCode::CoverageBad.val()));
    }

    #[test]
    fn balance_conserves_total() {
        let mut counts = [0u32; CORES];
        counts[0] = 5_000;
        let mut c = 1usize;
        while c < CORES {
            counts[c] = 100;
            c += 1;
        }
        let total: u32 = counts.iter().sum();
        let plan = match balance_counts(counts, CORES as u8) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(plan.total, total);
        assert_eq!(plan.counts.iter().sum::<u32>(), total);
        assert!(plan.moved > 0);
    }

    #[test]
    fn balance_share_within_slack() {
        let mut counts = [0u32; CORES];
        let mut c = 0usize;
        while c < CORES {
            counts[c] = (c as u32 + 1) * 1_000;
            c += 1;
        }
        let plan = match balance_counts(counts, CORES as u8) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert!(plan.share_ok());
        let mut mx = 0u32;
        let mut mn = u32::MAX;
        let mut i = 0usize;
        while i < CORES {
            if plan.counts[i] > mx {
                mx = plan.counts[i];
            }
            if plan.counts[i] < mn {
                mn = plan.counts[i];
            }
            i += 1;
        }
        assert!(mx - mn <= SHARE_SLACK);
        assert!(plan.audit().is_ok());
    }

    #[test]
    fn balance_shrinks_batch_spread() {
        let mut counts = [0u32; CORES];
        counts[0] = 8_000;
        counts[1] = 100;
        let plan = match balance_counts(counts, CORES as u8) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert!(plan.spread_before > plan.spread_after);
        assert_eq!(plan.spread_after, 0);
        assert!(plan.batches_before[0] > plan.batches_before[1]);
        assert_eq!(plan.batches[0], plan.batches[1]);
    }

    #[test]
    fn balance_rejects_inactive_counts() {
        let mut counts = [0u32; CORES];
        counts[0] = 100;
        counts[5] = 50;
        assert_eq!(
            balance_counts(counts, 4),
            Err(SchedCode::CoreBad.val())
        );
        assert_eq!(
            balance_counts(counts, 0),
            Err(SchedCode::CoreBad.val())
        );
        let zero = [0u32; CORES];
        assert_eq!(
            balance_counts(zero, 4),
            Err(SchedCode::CoverageBad.val())
        );
    }

    #[test]
    fn balance_rounds_ten() {
        assert!(balance_rounds(BALANCE_ROUNDS).is_ok());
        assert_eq!(balance_rounds(0), Err(SchedCode::CoverageBad.val()));
        assert_eq!(
            balance_rounds(BALANCE_ROUNDS),
            balance_rounds(BALANCE_ROUNDS)
        );
    }

    #[test]
    fn render_roundtrip() {
        let r = match stress_rounds(1) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let text = render_stress(&r);
        assert_eq!(audit_render(&text), Ok(1));
    }

    #[test]
    fn render_tamper_is_rejected() {
        let r = match stress_rounds(1) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let text = render_stress(&r);
        assert!(audit_render(&text.replacen("0x", "0y", 1)).is_err());
        // 事件数锚被改：轮数×每轮事件不再相等，账必拒。
        assert!(audit_render(&text.replacen("|10000|", "|10001|", 1)).is_err());
        assert!(audit_render("1|2|3").is_err());
    }

    #[test]
    fn injection_catches_four() {
        match inject_rounds(5) {
            Ok(l) => assert!(l.all_caught()),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn repro_is_deterministic() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn codes_are_unique_and_described() {
        let mut i = 0usize;
        while i < SchedCode::ALL.len() {
            let c = SchedCode::ALL[i];
            assert_eq!(detail(c.val()), c.detail());
            let mut k = i + 1;
            while k < SchedCode::ALL.len() {
                assert!(c.val() != SchedCode::ALL[k].val());
                k += 1;
            }
            i += 1;
        }
        assert_eq!(detail(0xDEAD_BEEF), "未知诊断码");
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48963_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
    }
}
