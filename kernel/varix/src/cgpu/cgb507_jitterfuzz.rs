//! CGPU-F0507 · 抖动 fuzz（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - fuzz：间隔异常注入 fuzz（极端间隔/负跳变/超长帧——均衡器容错
//!   不变量：抑制不增加延迟上限/不丢帧序）；策略参数 fuzz（非法参数
//!   钳制）；案例固化 CI。判据：异常注入、不变量、CI、判据。
//!
//! 设计纪律：
//! - 被测对象：均衡器最小模型 EqCore——输入呈现间隔序列，输出帧提交
//!   序（帧号单调 +1）与提交延迟；三策略处置面（直通/深缓冲/提前渲染）。
//! - 三异常注入：极端间隔（0 或 10×合同）、负跳变（时钟倒退）、超长帧
//!   （间隔 > 8×合同）。
//! - 容错不变量三条：① 帧序不丢（输出帧号严格连续）；② 抑制动作不抬
//!   延迟上限（提交延迟 ≤ 上限常量）；③ 非法参数钳制（策略/深度越界
//!   归位合法域，不 Err 不崩）。
//! - 案例固化：每轮注入序列的 FNV-1a 指纹入案例表——CI 逐案重放。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x5220 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x5220，批十独占）。
pub const SEG_BASE: u16 = 0x5220;

pub const E_JF_SEQ: u16 = SEG_BASE + 1; // 帧序断裂（不变量违例）
pub const E_JF_LAT: u16 = SEG_BASE + 2; // 延迟上限违例
pub const E_JF_CLAMP: u16 = SEG_BASE + 3; // 参数钳制失败
pub const E_JF_CASE: u16 = SEG_BASE + 4; // 固化案例重放失败

/// 合同帧时（80 帧）与延迟上限（1.5× 合同——抑制不得抬高）。
pub const CONTRACT_US: u64 = 12_500;
pub const LAT_CAP_US: u64 = 18_750;

/// FNV-1a 64（案例指纹）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性伪随机（LCG，fuzz 驱动源）。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 16
    }
}

/// 异常种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anomaly {
    /// 极端间隔：0us 或 10×合同。
    Extreme,
    /// 负跳变：时间倒退（间隔语义取绝对值再注入，帧序仍须连续）。
    NegJump,
    /// 超长帧：间隔 > 8×合同。
    LongFrame,
}

/// 均衡器最小模型：吃间隔，吐提交序。
#[derive(Clone, Debug)]
pub struct EqCore {
    strategy: u8,
    depth: u8,
    next_frame: u64,
    lat_cap: u64,
    submits: Vec<(u64, u64)>,
}

impl EqCore {
    /// 构造：参数越界即钳制（策略归直通，深度归 0..=7 饱和）。
    pub fn build(strategy: i64, depth: i64) -> Self {
        let s = if (0..=3).contains(&strategy) { strategy as u8 } else { 0 };
        let d = depth.clamp(0, 7) as u8;
        Self {
            strategy: s,
            depth: d,
            next_frame: 1,
            lat_cap: LAT_CAP_US,
            submits: Vec::new(),
        }
    }

    pub fn strategy(&self) -> u8 {
        self.strategy
    }

    pub fn depth(&self) -> u8 {
        self.depth
    }

    /// 提交一帧：返回帧号；间隔语义归一（负跳变取绝对值）后参与延迟。
    pub fn submit(&mut self, interval_us: i64) -> u64 {
        let iv = interval_us.unsigned_abs();
        let extra = match self.strategy {
            1 => self.depth as u64 * CONTRACT_US / 8, // 深缓冲：按深度吸收
            2 => CONTRACT_US / 16,                    // 提前渲染：固定折减
            _ => 0,
        };
        let lat = iv.saturating_sub(extra).min(self.lat_cap);
        let f = self.next_frame;
        self.next_frame += 1;
        self.submits.push((f, lat));
        f
    }

    /// 不变量 ①：帧序严格连续。
    pub fn seq_ok(&self) -> bool {
        for (i, (f, _)) in self.submits.iter().enumerate() {
            if *f != i as u64 + 1 {
                return false;
            }
        }
        true
    }

    /// 不变量 ②：提交延迟不越上限。
    pub fn lat_ok(&self) -> bool {
        self.submits.iter().all(|(_, l)| *l <= self.lat_cap)
    }
}

/// 固化案例：注入序列指纹 + 重放校验。
#[derive(Clone, Debug)]
pub struct CaseBook {
    cases: Vec<(u64, usize)>, // (指纹, 注入条数)
}

impl CaseBook {
    pub fn new() -> Self {
        Self { cases: Vec::new() }
    }

    /// 固化一轮注入序列。
    pub fn seal(&mut self, seq: &[i64]) -> u64 {
        let mut bytes = Vec::new();
        for x in seq {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
        let fp = fnv1a64(&bytes);
        self.cases.push((fp, seq.len()));
        fp
    }

    /// CI 重放：指纹与条数对得上才算过。
    pub fn replay_ok(&self, seq: &[i64], fp: u64) -> bool {
        let mut bytes = Vec::new();
        for x in seq {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
        fnv1a64(&bytes) == fp
            && self
                .cases
                .iter()
                .any(|(f, n)| *f == fp && *n == seq.len())
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }
}

/// fuzz 主循环：n 轮三异常注入，断言两不变量，全过返回 Ok(())。
pub fn fuzz_rounds(n: usize, seed: u64) -> Result<(), u16> {
    let mut rng = Lcg::new(seed);
    let mut core = EqCore::build(1, 3);
    for _ in 0..n {
        let pick = rng.next() % 3;
        let iv: i64 = match pick {
            0 => {
                if rng.next() % 2 == 0 {
                    0
                } else {
                    (CONTRACT_US * 10) as i64
                }
            }
            1 => -((rng.next() % 40_000) as i64),
            _ => (CONTRACT_US * 9) as i64,
        };
        core.submit(iv);
        if !core.seq_ok() {
            return Err(E_JF_SEQ);
        }
        if !core.lat_ok() {
            return Err(E_JF_LAT);
        }
    }
    Ok(())
}

// ==========================================================================
// 判据
// ==========================================================================

pub fn cgb507_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 1. 异常注入：三异常 600 轮全过（帧序 + 延迟上限双不变量）
    let ok_fuzz = fuzz_rounds(600, 0x0507).is_ok() && fuzz_rounds(600, 0xDEAD).is_ok();
    v.push(("异常注入-三异常600轮不变量", ok_fuzz));

    // 2. 帧序不丢：混合注入后帧号严格连续
    let mut c = EqCore::build(3, 5);
    for iv in [0i64, -3_000, 100_000, 12_500, -1] {
        c.submit(iv);
    }
    v.push(("不变量-帧序严格连续", c.seq_ok()));

    // 3. 抑制不抬延迟上限：深缓冲策略下超长帧延迟仍 ≤ LAT_CAP
    let mut d = EqCore::build(1, 7);
    d.submit((CONTRACT_US * 8) as i64);
    let lat_ok = d.lat_ok() && d.submits.last().map(|(_, l)| *l <= LAT_CAP_US).unwrap_or(false);
    v.push(("不变量-抑制不抬延迟上限", lat_ok));

    // 4. 参数钳制：非法策略归直通、深度饱和钳制
    let c1 = EqCore::build(9, -3);
    let c2 = EqCore::build(2, 42);
    v.push((
        "参数钳制-策略深度归位",
        c1.strategy() == 0 && c1.depth() == 0 && c2.strategy() == 2 && c2.depth() == 7,
    ));

    // 5. 案例固化与重放：指纹一致可通过，篡改序列被拒
    let mut book = CaseBook::new();
    let seq: Vec<i64> = vec![12_500, -2_000, 90_000, 0];
    let fp = book.seal(&seq);
    let ok_ci = book.replay_ok(&seq, fp) && book.len() == 1;
    let mut bad = seq.clone();
    bad[0] = 12_501;
    v.push(("案例固化-CI重放与篡改拒", ok_ci && !book.replay_ok(&bad, fp)));

    // 6. 确定性：同种子两轮注入产生同帧序（fuzz 可重放）
    let mut a = EqCore::build(1, 2);
    let mut b = EqCore::build(1, 2);
    let mut r = Lcg::new(77);
    for _ in 0..50 {
        let iv = (r.next() % 100_000) as i64 - 20_000;
        a.submit(iv);
        b.submit(iv);
    }
    v.push(("确定性-同种子同帧序", a.submits == b.submits));

    // 7. 直通策略零折减：正常间隔延迟=间隔
    let p = EqCore::build(0, 0);
    let mut p = p;
    p.submit(12_500);
    v.push((
        "策略-直通零折减",
        p.submits.last().map(|(_, l)| *l == 12_500).unwrap_or(false),
    ));

    v
}

pub fn run_cgb507_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("cgpu-cgb507");
    for (name, ok) in cgb507_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::cgb507_raw_checks as raw;
    #[test]
    fn t_cgb507() {
        for (n, ok) in raw() {
            assert!(ok, "cgb507 check failed: {}", n);
        }
    }
}
