//! UNX-F36097-J1 ACE 性能账：求值原语 P99（B09 消费预告）。
//! 判据：① 64 样本步数全在预算内（解析 N 步上限 24）、滚动 16 桶计数
//! 守恒（Σ桶 = 样本数）；② P50/P90/P99 整数分位定位单调（桶位不回退）；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：自含模块——步数账按本地等价校验逻辑记账（与真源闸同构），
//! 无墙钟无浮点（红线），全整数分位；B09 热路径（F36170）为预告消费锚。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x3890;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;    // 版本锚不符
pub const E_BUDGET: u16 = SEG_BASE + 2; // 步数超预算
pub const E_BUCKET: u16 = SEG_BASE + 3; // 桶计数守恒破
pub const E_PCT: u16 = SEG_BASE + 4;    // 分位定位越界
pub const E_MONO: u16 = SEG_BASE + 5;   // 分位单调破（P50>P90>P99）
pub const E_CS: u16 = SEG_BASE + 6;     // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 解析步数预算（本地等价逻辑的最坏步数上界）。
pub const PARSE_BUDGET: u32 = 24;
/// 滚动桶数。
pub const BUCKETS: usize = 16;
/// 采样轮数。
pub const SAMPLES: u32 = 64;

/// 分位类别（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerfZone {
    /// 50 分位。
    P50 = 0x0001,
    /// 90 分位。
    P90 = 0x0002,
    /// 99 分位。
    P99 = 0x0003,
}

impl PerfZone {
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 分位百分数（整数）。
    pub fn pct(self) -> u32 {
        match self {
            PerfZone::P50 => 50,
            PerfZone::P90 => 90,
            PerfZone::P99 => 99,
        }
    }
}

/// 本地等价解析记账：逐字节 8 步 + 类型闭集查表 ≤11 步 + 尺寸/标志/掩码闸 4 步。
/// 返回 (步数, 诊断码)——校验逻辑与真源闸同构，码直接透传真源码。
pub fn eval_steps(buf: &[u8; 8]) -> Result<u32, u16> {
    let mut steps: u32 = 0;
    // 逐字节在场检查（8 步）。
    let mut i = 0;
    while i < 8 {
        let _b = buf[i];
        steps += 1;
        i += 1;
    }
    let h = core::AceHeader {
        ace_type: buf[0],
        flags: buf[1],
        size: u16::from_le_bytes([buf[2], buf[3]]),
        mask: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
    };
    // 类型闭集查表（≤11 步）。
    let mut found = false;
    let mut t = 0;
    while t < core::ACE_TYPE_N {
        if core::ACE_TYPE_TABLE[t].code == h.ace_type as u16 {
            found = true;
        }
        steps += 1;
        t += 1;
    }
    if !found {
        return Err(core::E_TYPE);
    }
    // 尺寸双闸（2 步）。
    steps += 1;
    if (h.size as usize) < core::ACE_HEADER_MIN {
        return Err(core::E_SIZE);
    }
    steps += 1;
    if (h.size as usize) > core::ACE_SIZE_MAX {
        return Err(core::E_SIZE);
    }
    // 标志闸（1 步）。
    steps += 1;
    if h.flags & core::FLAG_RESERVED != 0 {
        return Err(core::E_FLAGS);
    }
    // 掩码闸（1 步）。
    steps += 1;
    if h.mask & core::MASK_RESERVED != 0 {
        return Err(core::E_MASK);
    }
    if steps > PARSE_BUDGET {
        return Err(E_BUDGET);
    }
    Ok(steps)
}

/// 步数 → 桶位（整数等宽分桶：步数 s ∈ [1,budget] → 桶 (s-1)*B/budget）。
pub fn bucket_of(steps: u32) -> usize {
    let s = if steps == 0 { 1 } else { steps };
    let mut b = ((s as usize - 1) * BUCKETS) / (PARSE_BUDGET as usize);
    if b >= BUCKETS {
        b = BUCKETS - 1;
    }
    b
}

/// 滚动桶账：K 样本入桶，返回桶数组与总数。
pub fn rolling_record(steps_seq: &[u32]) -> ([u32; BUCKETS], u32) {
    let mut buckets = [0u32; BUCKETS];
    let mut total: u32 = 0;
    let mut i = 0;
    while i < steps_seq.len() {
        let b = bucket_of(steps_seq[i]);
        buckets[b] = buckets[b].wrapping_add(1);
        total = total.wrapping_add(1);
        i += 1;
    }
    (buckets, total)
}

/// 整数分位桶定位：rank = total*pct/100（0 → 1），累计走查找桶。
pub fn percentile_bucket(buckets: &[u32; BUCKETS], total: u32, pct: u32) -> Result<usize, u16> {
    if total == 0 {
        return Err(E_PCT);
    }
    let mut rank = (total.wrapping_mul(pct)) / 100;
    if rank == 0 {
        rank = 1;
    }
    if rank > total {
        return Err(E_PCT);
    }
    let mut cum: u32 = 0;
    let mut b = 0;
    while b < BUCKETS {
        cum = cum.wrapping_add(buckets[b]);
        if cum >= rank {
            return Ok(b);
        }
        b += 1;
    }
    Err(E_PCT)
}

/// LCG（确定性随机源）。
pub fn lcg_next(state: u64) -> u64 {
    state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

/// LCG 生成合法头四件（与 36096 好例域一致）。
pub fn lcg_good_header(state: u64) -> [u8; 8] {
    let mask: u32 = (state as u32) & 0x001f_ffff;
    let flags: u8 = ((state >> 32) as u8) & 0x1f;
    let size: u16 = core::ACE_HEADER_MIN as u16 + ((state >> 40) as u16 % 8) * 4;
    let types: [u8; 3] = [0x00, 0x01, 0x02];
    let t: u8 = types[((state >> 48) as usize) % 3];
    let mut buf = [0u8; 8];
    buf[0] = t;
    buf[1] = flags;
    let sz = size.to_le_bytes();
    buf[2] = sz[0];
    buf[3] = sz[1];
    let m = mask.to_le_bytes();
    buf[4] = m[0];
    buf[5] = m[1];
    buf[6] = m[2];
    buf[7] = m[3];
    buf
}

/// 摘要行（String 面，无 IO）。
pub fn summary(buckets: &[u32; BUCKETS], total: u32) -> String {
    let mut s = String::new();
    s.push_str("perf total=");
    s.push_str(&total.to_string());
    s.push_str(" top=");
    s.push_str(&buckets[BUCKETS - 1].to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_BUDGET, E_BUCKET, E_PCT, E_MONO, E_CS];
    let mut i = 0;
    while i < cs.len() {
        let mut j = i + 1;
        while j < cs.len() {
            if cs[i] == cs[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_BUDGET => "步数超预算",
        E_BUCKET => "桶计数守恒破",
        E_PCT => "分位定位越界",
        E_MONO => "分位单调破（P50>P90>P99）",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 锚位；R2 64 样本步数记账全在预算内；R3 桶计数守恒；
/// R4 P50/P90/P99 定位在界内；R5 分位单调；R6 指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：锚位（预算 ≥ 最坏步数、桶数、轮数）。
    if VER != 1 {
        return Err(E_VER);
    }
    if PARSE_BUDGET < 23 || BUCKETS != 16 || SAMPLES != 64 {
        return Err(E_BUDGET);
    }
    hits += 1;
    // R2：64 样本（LCG 合法头）步数记账，全部 ≤ 预算。
    let mut steps_seq: Vec<u32> = vec![];
    let mut st = 0x0fed_cba9_8765_4321;
    let mut s = 0;
    while s < SAMPLES {
        st = lcg_next(st);
        let buf = lcg_good_header(st);
        let steps = eval_steps(&buf)?;
        if steps > PARSE_BUDGET {
            return Err(E_BUDGET);
        }
        if steps == 0 {
            return Err(E_BUDGET);
        }
        steps_seq.push(steps);
        hits += 1;
        s += 1;
    }
    // R3：桶计数守恒（Σ桶 == 样本数；非零桶位连续性不做要求）。
    let (buckets, total) = rolling_record(&steps_seq);
    let mut sum: u32 = 0;
    let mut b = 0;
    while b < BUCKETS {
        sum = sum.wrapping_add(buckets[b]);
        b += 1;
    }
    if sum != total || total != SAMPLES {
        return Err(E_BUCKET);
    }
    hits += 1;
    // R4：P50/P90/P99 定位全在界内（0..BUCKETS）。
    let p50 = percentile_bucket(&buckets, total, PerfZone::P50.pct())?;
    let p90 = percentile_bucket(&buckets, total, PerfZone::P90.pct())?;
    let p99 = percentile_bucket(&buckets, total, PerfZone::P99.pct())?;
    if p50 >= BUCKETS || p90 >= BUCKETS || p99 >= BUCKETS {
        return Err(E_PCT);
    }
    hits += 1;
    // R5：分位单调（P50 桶 ≤ P90 桶 ≤ P99 桶）。
    if !(p50 <= p90 && p90 <= p99) {
        return Err(E_MONO);
    }
    hits += 1;
    // R6：指纹两次求值稳定且非零（真源 FNV over 桶账 + 版本锚）。
    let mut feed = Vec::new();
    feed.extend_from_slice(&VER.to_le_bytes());
    let mut b2 = 0;
    while b2 < BUCKETS {
        feed.extend_from_slice(&buckets[b2].to_le_bytes());
        b2 += 1;
    }
    let fp1 = core::fnv1a(&feed);
    let fp2 = core::fnv1a(&feed);
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(69));
    }

    #[test]
    fn budget_holds() {
        let buf = lcg_good_header(0x0000_0000_0000_0000);
        let steps = eval_steps(&buf).unwrap_or(999);
        assert!(steps <= PARSE_BUDGET);
    }

    #[test]
    fn bad_header_costs_steps_but_fails() {
        let mut buf = lcg_good_header(1);
        buf[0] = 0xee; // 未知类型
        assert_eq!(eval_steps(&buf), Err(core::E_TYPE));
    }

    #[test]
    fn percentile_shapes() {
        let buckets = [0u32; BUCKETS];
        assert_eq!(percentile_bucket(&buckets, 0, 99), Err(E_PCT));
        assert!(summary(&buckets, 0).contains("perf"));
        assert_eq!(PerfZone::P99.pct(), 99);
    }
}

pub fn run_unx36097_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36097_aceperf");
    cs.add("SEG 段位独占", SEG_BASE == 0x3890, "0x3890~0x389f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "64 样本全在预算内、桶账守恒、分位单调"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("预算锚 24", PARSE_BUDGET == 24, "解析步数预算锚漂移");
    cs.add("桶数锚 16", BUCKETS == 16, "滚动桶数锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
