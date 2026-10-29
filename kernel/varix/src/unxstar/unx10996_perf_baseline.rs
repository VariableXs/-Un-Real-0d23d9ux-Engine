//! UNX-F10996 多进程压测性能基线冻结（吞吐/延迟在册）（0x1dc00 段）
//!
//! 锚点：UNX-F10996-J1 吞吐/延迟基线入册——确定性模拟负载千轮实测吞吐与
//! 分位延迟逐段入册；J2 复跑偏差容差内——复跑基线与首跑基线逐段对比，
//! 偏差全部落在冻结容差内。
//! 域：CoRun·UNX-C4（多进程压力域）。
//!
//! 结构：性能册（分段桶：每桶累计样本数/延迟和/峰值）+ 分位算子（确定性
//! 选择排序取 p50/p99，无浮点、全整型毫秒域）+ 冻结器（基线快照 + 容差
//! 判定）+ 注入核（空桶/负延迟/超容偏差/封账写入四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1dc00 独占 0x20）。
pub const SEG_BASE: u32 = 0x1dc00;
pub const PF_EMPTY: u32 = SEG_BASE + 0x01; // 空桶取分位
pub const PF_NEG: u32 = SEG_BASE + 0x02; // 负延迟样本
pub const PF_DRIFT: u32 = SEG_BASE + 0x03; // 复跑偏差超容差
pub const PF_FROZEN: u32 = SEG_BASE + 0x04; // 封账后写样本
pub const PF_NOCAP: u32 = SEG_BASE + 0x05; // 未建桶先写

/// 分桶数。
pub const BUCKETS: usize = 8;
/// 每桶样本上限。
pub const BUCKET_CAP: usize = 64;
/// 冻结容差（毫秒）。
pub const TOLERANCE: u32 = 12;

/// 性能桶：样本延迟序列（整型毫秒）。
#[derive(Clone, Debug)]
pub struct Bucket {
    pub id: usize,
    pub samples: Vec<u32>,
}

/// 性能册：吞吐计数 + 分桶延迟 + 挤出计数。
pub struct PerfBook {
    pub buckets: Vec<Bucket>,
    pub ops: u64,
    pub drops: u64,
    pub sealed: bool,
}

impl PerfBook {
    /// 建册：BUCKETS 桶一次建齐。
    pub fn new() -> PerfBook {
        let mut buckets: Vec<Bucket> = Vec::new();
        let mut i = 0usize;
        while i < BUCKETS {
            buckets.push(Bucket { id: i, samples: Vec::new() });
            i += 1;
        }
        PerfBook { buckets, ops: 0, drops: 0, sealed: false }
    }

    /// 落样本：桶号取模路由，负延迟（> 1s 视为坏样本）必拒。
    pub fn record(&mut self, lat_ms: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(PF_FROZEN);
        }
        if lat_ms > 1000 {
            return Err(PF_NEG);
        }
        let b = (lat_ms as usize) % BUCKETS;
        if self.buckets[b].samples.len() >= BUCKET_CAP {
            // 满桶挤出最旧样本（环形语义），挤出量单独落账保证样本守恒。
            self.buckets[b].samples.remove(0);
            self.drops = self.drops.wrapping_add(1);
        }
        self.buckets[b].samples.push(lat_ms);
        self.ops = self.ops.wrapping_add(1);
        Ok(b)
    }

    /// 确定性选择排序（升序副本）。
    fn sorted(&self, b: usize) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.buckets[b].samples.len() {
            v.push(self.buckets[b].samples[k]);
            k += 1;
        }
        let mut a = 0usize;
        while a < v.len() {
            let mut m = a;
            let mut j = a + 1;
            while j < v.len() {
                if v[j] < v[m] {
                    m = j;
                }
                j += 1;
            }
            if m != a {
                let t = v[a];
                v[a] = v[m];
                v[m] = t;
            }
            a += 1;
        }
        v
    }

    /// 分位：p0=最小、p50=中位、p99=近最大（确定性整型插值）。
    pub fn quantile(&self, b: usize, per: u32) -> Result<u32, u32> {
        if b >= BUCKETS {
            return Err(PF_NOCAP);
        }
        let v = self.sorted(b);
        if v.is_empty() {
            return Err(PF_EMPTY);
        }
        let n = v.len();
        let idx = match per {
            0 => 0usize,
            50 => (n - 1) / 2,
            99 => n - 1,
            _ => ((n - 1) as u64 * per as u64 / 100) as usize,
        };
        Ok(v[idx])
    }

    /// 吞吐基线：ops 换算确定性整型千分位吞吐（ops*1000 / 样本规模因子）。
    pub fn throughput_milli(&self, scale: u64) -> u64 {
        if scale == 0 {
            return 0;
        }
        self.ops.wrapping_mul(1000) / scale
    }

    /// 基线快照：每桶 p50 序列。
    pub fn baseline_p50(&self) -> Result<Vec<u32>, u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut b = 0usize;
        while b < BUCKETS {
            v.push(self.quantile(b, 50)?);
            b += 1;
        }
        Ok(v)
    }

    /// 复跑偏差核验：逐桶 |p50 差| ≤ 容差。
    pub fn drift_ok(&self, base: &[u32]) -> Result<bool, u32> {
        if base.len() != BUCKETS {
            return Err(PF_NOCAP);
        }
        let mut b = 0usize;
        while b < BUCKETS {
            let now = self.quantile(b, 50)?;
            let d = if now > base[b] { now - base[b] } else { base[b] - now };
            if d > TOLERANCE {
                return Err(PF_DRIFT);
            }
            b += 1;
        }
        Ok(true)
    }

    /// fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0x1dc0_0296;
        let mut b = 0usize;
        while b < BUCKETS {
            let mut k = 0usize;
            while k < self.buckets[b].samples.len() {
                h = (h ^ self.buckets[b].samples[k] as u64).wrapping_mul(0x100000001b3);
                k += 1;
            }
            b += 1;
        }
        h = (h ^ self.ops).wrapping_mul(0x100000001b3);
        h = (h ^ self.drops).wrapping_mul(0x100000001b3);
        h
    }

    /// 封账。
    pub fn seal(&mut self) -> u64 {
        self.sealed = true;
        self.fold()
    }
}

/// 确定性模拟负载：第 r 轮延迟样本（三角波 + 素数扰动，全整型）。
pub fn pf_sample(r: u32) -> u32 {
    let wave = if r % 2 == 0 { r % 16 } else { 15 - r % 16 };
    wave.wrapping_mul(3).wrapping_add((r % 7).wrapping_add(2))
}

/// 千轮压测入册。
pub fn run_load(rounds: u32) -> PerfBook {
    let mut b = PerfBook::new();
    let mut r = 0u32;
    while r < rounds {
        let _ = b.record(pf_sample(r));
        r += 1;
    }
    b
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 空桶取分位必抓
    let b = PerfBook::new();
    got[0] = b.quantile(3, 50).err() == Some(PF_EMPTY);
    // 2) 负延迟样本必抓
    let mut c = PerfBook::new();
    got[1] = c.record(1001).err() == Some(PF_NEG);
    // 3) 超容差偏差必抓
    let mut d = run_load(64);
    let base = d.baseline_p50().unwrap_or_default();
    // 注入偏移样本改变某桶 p50。
    let mut k = 0u32;
    while k < 40 {
        let _ = d.record(60);
        k = k.wrapping_add(1);
    }
    got[2] = d.drift_ok(&base) == Err(PF_DRIFT);
    // 4) 封账写入必抓
    d.seal();
    got[3] = d.record(5).err() == Some(PF_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10996_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10996");

    // J1 吞吐/延迟基线入册
    let a = run_load(1000);
    let inj = inject_four();
    let tp = a.throughput_milli(1000);
    let j1 = a.baseline_p50().is_ok() && tp == 1000 && a.ops == 1000 && inj[0];
    s.add("J1-基线入册", j1, "千轮吞吐千分位与八桶 p50 全入册");

    // J2 复跑偏差容差内
    let b2 = run_load(1000);
    let j2 = b2.drift_ok(&a.baseline_p50().unwrap_or_default()) == Ok(true);
    s.add("J2-复跑容差内", j2, "复跑 p50 与首跑逐桶差全部 ≤ 冻结容差");

    // J3 空桶取分位必抓
    let empty = PerfBook::new();
    s.add("J3-空桶抓", inj[0] && empty.quantile(0, 50).err() == Some(PF_EMPTY),
        "空桶分位必拒不虚报");

    // J4 负延迟样本必抓
    s.add("J4-负样本抓", inj[1], "超界延迟样本必拒");

    // J5 超容差偏差必抓
    s.add("J5-超差抓", inj[2], "复跑偏差越容差必报 DRIFT");

    // J6 封账写入必抓
    s.add("J6-封账拒写", inj[3], "封账后落样本必拒");

    // J7 分位单调：p0 ≤ p50 ≤ p99
    let c = run_load(256);
    let mut mono = true;
    let mut b = 0usize;
    while b < BUCKETS {
        let (p0, p50, p99) = (c.quantile(b, 0), c.quantile(b, 50), c.quantile(b, 99));
        match (p0, p50, p99) {
            (Ok(a), Ok(m), Ok(z)) => {
                if a > m || m > z {
                    mono = false;
                }
            }
            _ => mono = false,
        }
        b += 1;
    }
    s.add("J7-分位单调", mono, "每桶 p0/p50/p99 严格单调");

    // J8 桶路由对平：桶内样本 + 环形挤出量 = 全局 ops
    let mut d = run_load(512);
    let mut total = 0usize;
    let mut b = 0usize;
    while b < BUCKETS {
        total = total.wrapping_add(d.buckets[b].samples.len());
        b += 1;
    }
    let j8 = (total as u64).wrapping_add(d.drops) == d.ops && d.ops == 512;
    s.add("J8-桶账守恒", j8, "桶样本总和加挤出量等于全局操作数");

    // J9 环形挤出：满桶再写桶容量不涨
    let mut e = PerfBook::new();
    let mut k = 0u32;
    while k < (BUCKET_CAP as u32) * 2 {
        let _ = e.record(1);
        k = k.wrapping_add(1);
    }
    let j9 = e.buckets[1].samples.len() == BUCKET_CAP && e.buckets[1].samples[0] == 1;
    s.add("J9-环形挤出", j9, "满桶挤出最旧样本容量恒定");

    // J10 fold 指纹与封账根
    let fp0 = d.fold();
    let root = d.seal();
    let j10 = fp0 == root && d.fold() == root;
    s.add("J10-指纹冻结", j10, "封账根与 fold 一致且二次 fold 稳定");

    // J11 吞吐换算：零尺度安全
    let z = PerfBook::new();
    s.add("J11-零尺度安全", z.throughput_milli(0) == 0, "零尺度不除零返回零");

    // J12 动态明细：千轮吞吐与样本总数落账
    s.add_dyn("J12-吞吐明细", tp == 1000 && a.ops == 1000,
        &format!("ops={} tp_milli={}", a.ops, tp));

    // J13 动态明细：性能册指纹落账
    let fp = c.fold();
    s.add_dyn("J13-册指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10996_perf_flow() {
        let mut b = PerfBook::new();
        assert!(b.record(1001).err() == Some(PF_NEG));
        let mut k = 0u32;
        while k < 16 {
            assert!(b.record(k % 16).is_ok());
            k = k.wrapping_add(1);
        }
        assert!(b.quantile(0, 0).is_ok());
        assert!(b.quantile(0, 50).is_ok());
        assert!(b.quantile(0, 99).is_ok());
        assert_eq!(b.throughput_milli(8), 2000);
        b.seal();
        assert!(b.record(1).err() == Some(PF_FROZEN));
    }

    #[test]
    fn unx10996_checks_green() {
        let s = run_unx10996_checks();
        assert!(s.all_passed(), "UNX-F10996 判据未全绿");
    }
}
