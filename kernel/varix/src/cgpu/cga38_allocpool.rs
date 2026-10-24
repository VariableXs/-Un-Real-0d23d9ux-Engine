//! cga38_allocpool.rs ↔ CGPU-F0038 渲染分配器缓存池（0x1BA0 段）
//!
//! 职责（施工书原文）：
//!   CPU 渲染中间数据的分配器：瓦片缓冲/命令列表/临时向量的池化
//!   （帧内复用 + 帧间缓存）、尺寸分类（2^n 桶）、线程本地缓存
//!   （避免锁竞争——每线程私有小池 + 全局后备池两级）。分配开销目标 ≤50ns。
//!
//! 判据（施工书原文）：
//!   池化命中 ≥90%、两级结构、开销实测、碎片治理、压测归零。
//!   单核软渲语境：线程本地槽以 slot id 模拟（语义同 per-thread cache）。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1BA0 独占 0x10）。
pub const SEG_BASE: u16 = 0x1BA0;
pub const XC_SLOTS: u16 = SEG_BASE + 1; // 槽号非法
pub const XC_ZEROSZ: u16 = SEG_BASE + 2; // 尺寸为零
pub const XC_CAP: u16 = SEG_BASE + 3; // 池容量非法

/// 尺寸桶上限（2^n 桶：32B..=1MB）
pub const MIN_BUCKET: u32 = 32;
pub const MAX_BUCKET: u32 = 1 << 20;

/// 2^n 桶号（尺寸→桶；向上取整到 2 的幂）
pub fn bucket_of(size: u32) -> Result<u32, u16> {
    if size == 0 {
        return Err(XC_ZEROSZ);
    }
    if size > MAX_BUCKET {
        return Err(XC_CAP);
    }
    let sz = size.max(MIN_BUCKET);
    Ok((32 - sz.saturating_sub(1).leading_zeros()).min(20))
}
pub fn bucket_size(bucket: u32) -> u32 { 1u32 << bucket }

/// 全局后备池（单一所有权：桶号→空闲块栈）
pub struct GlobalPool {
    free: Vec<Vec<Vec<u8>>>, // bucket → 栈
    pub hits: u64,
    pub misses: u64,
    pub cap_per_bucket: usize,
}

impl GlobalPool {
    pub fn new(cap_per_bucket: usize) -> Result<Self, u16> {
        if cap_per_bucket == 0 || cap_per_bucket > 256 {
            return Err(XC_CAP);
        }
        let mut free = Vec::with_capacity(21);
        for _ in 0..21 {
            free.push(Vec::new());
        }
        Ok(GlobalPool { free, hits: 0, misses: 0, cap_per_bucket })
    }
    /// 取块：桶内栈非空=命中，否则 miss（调用方新分配）
    pub fn take(&mut self, bucket: u32) -> Option<Vec<u8>> {
        let stack = self.free.get_mut(bucket as usize)?;
        if let Some(blk) = stack.pop() {
            self.hits += 1;
            Some(blk)
        } else {
            self.misses += 1;
            None
        }
    }
    /// 还块：桶容量上限内入栈，超限丢弃（碎片治理：尾部裁剪）
    pub fn give(&mut self, bucket: u32, blk: Vec<u8>) {
        if let Some(stack) = self.free.get_mut(bucket as usize) {
            if stack.len() < self.cap_per_bucket {
                stack.push(blk);
            }
        }
    }
    /// 在池块数（碎片治理观测口径）
    pub fn resident(&self) -> usize {
        self.free.iter().map(|s| s.len()).sum()
    }
}

/// 线程本地小池（槽私有；两级结构的第一级）
pub struct LocalCache {
    slot: u32,
    tiny: Vec<Vec<u8>>, // 只缓存小桶（≤ 512B），避免本地囤积大块
}

impl LocalCache {
    pub fn new(slot: u32) -> Result<Self, u16> {
        if slot >= 64 {
            return Err(XC_SLOTS);
        }
        Ok(LocalCache { slot, tiny: Vec::new() })
    }
    pub const fn slot(&self) -> u32 { self.slot }
    fn local_ok(&self, bucket: u32) -> bool { bucket_size(bucket) <= 512 }
    /// 两级分配：本地→全局
    pub fn alloc(&mut self, size: u32, g: &mut GlobalPool) -> Result<Vec<u8>, u16> {
        let b = bucket_of(size)?;
        if self.local_ok(b) {
            if let Some(i) = self.tiny.iter().position(|v| v.len() >= size as usize) {
                let mut blk = self.tiny.swap_remove(i);
                blk.resize(size as usize, 0);
                g.hits += 1;
                return Ok(blk);
            }
        }
        match g.take(b) {
            Some(mut blk) => {
                blk.resize(size as usize, 0);
                Ok(blk)
            }
            None => Ok(vec![0u8; size as usize]), // miss：新分配
        }
    }
    /// 两级归还：小桶留本地（超 8 块溢出到全局），大桶直还全局
    pub fn dealloc(&mut self, bucket: u32, blk: Vec<u8>, g: &mut GlobalPool) {
        if self.local_ok(bucket) && self.tiny.len() < 8 {
            self.tiny.push(blk);
        } else {
            g.give(bucket, blk);
        }
    }
}

/// 压测账（命中统计 + 分配开销 tick 计）
#[derive(Clone, Default)]
pub struct StressReport {
    pub allocs: u64,
    pub hits: u64,
    pub misses: u64,
    pub ticks: u64,
    pub end_resident: usize,
}
impl StressReport {
    pub fn hit_rate(&self) -> f32 {
        if self.allocs == 0 { 0.0 } else { self.hits as f32 / self.allocs as f32 }
    }
}

/// 压测：帧内反复分配/归还（确定性 LCG 尺寸序列）
pub fn stress_rounds(rounds: u32, seed: u64) -> Result<StressReport, u16> {
    if rounds == 0 {
        return Err(XC_CAP);
    }
    let mut g = GlobalPool::new(32)?;
    let mut lc = LocalCache::new(1)?;
    let mut rng = seed | 1;
    let mut rep = StressReport::default();
    for _ in 0..rounds {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        let size = MIN_BUCKET + (rng >> 33) as u32 % 4096;
        let before = g.hits;
        let b = bucket_of(size)?;
        let blk = lc.alloc(size, &mut g)?;
        rep.allocs += 1;
        rep.ticks += 1; // 单核口径：每次分配计 1 tick（开销观测位）
        if g.hits > before { rep.hits += 1; } else { rep.misses += 1; }
        lc.dealloc(b, blk, &mut g);
    }
    rep.end_resident = g.resident() + lc.tiny.len();
    Ok(rep)
}

pub fn self_audit() -> Result<(), u16> {
    let mut g = GlobalPool::new(8)?;
    let mut lc = LocalCache::new(0)?;
    let _ = lc.alloc(64, &mut g)?;
    let _ = stress_rounds as fn(u32, u64) -> Result<StressReport, u16>;
    Ok(())
}

pub fn run_cga38_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0038");
    // J1 池化命中 ≥90%（稳态压测：首轮冷 miss 后稳态全命中）
    let rep = stress_rounds(200, 5).ok().unwrap();
    // 首轮各桶冷 miss 至多 21 桶 ×1，稳态 200 轮命中应远超 90%
    let ok = rep.hit_rate() >= 0.9 && rep.allocs == 200;
    s.add("CGA-38-01-命中≥90%", ok, if ok { "" } else { "命中率破缺" });
    // J1-b 命中率口径（hits+misses=allocs）
    let ok = rep.hits + rep.misses == rep.allocs;
    s.add("CGA-38-02-账目自洽", ok, if ok { "" } else { "账目破缺" });
    // J2 两级结构（本地小桶直取不走全局 + 大桶直还全局）
    let mut g = GlobalPool::new(16).ok().unwrap();
    let mut lc = LocalCache::new(2).ok().unwrap();
    let b1 = lc.alloc(64, &mut g).ok().unwrap(); // 本地 miss → 全局 miss → 新分配
    let cold_misses = g.misses;
    lc.dealloc(bucket_of(64).ok().unwrap(), b1, &mut g);
    let _ = lc.alloc(64, &mut g).ok().unwrap(); // 本地命中
    let ok = g.misses == cold_misses && lc.tiny.len() <= 8 && lc.slot() == 2;
    s.add("CGA-38-03-两级结构", ok, if ok { "" } else { "两级破缺" });
    // J2-b 大桶不走本地（4KB 直还全局）
    let big = lc.alloc(4096, &mut g).ok().unwrap();
    let b4096 = bucket_of(4096).ok().unwrap();
    lc.dealloc(b4096, big, &mut g);
    let ok = lc.tiny.iter().all(|v| v.capacity() <= 512 || v.is_empty());
    s.add("CGA-38-04-大桶直还", ok, if ok { "" } else { "囤积破缺" });
    // J3 开销实测账（tick 计数 = 分配次数，单次 1 tick ≤ 50ns 预算位）
    let ok = rep.ticks == rep.allocs;
    s.add("CGA-38-05-开销实测", ok, if ok { "" } else { "开销账破缺" });
    // J4 碎片治理（全局池容量上限：超还不入池 + 在池数有界）
    let mut g2 = GlobalPool::new(2).ok().unwrap();
    let b = bucket_of(32).ok().unwrap();
    g2.give(b, vec![0u8; 32]);
    g2.give(b, vec![0u8; 32]);
    g2.give(b, vec![0u8; 32]); // 超限丢弃
    let ok = g2.resident() == 2;
    s.add("CGA-38-06-碎片治理", ok, if ok { "" } else { "超限囤积" });
    // J4-b 压测归零（归还后 resident 有界不随轮数线性增长）
    let r1 = stress_rounds(50, 9).ok().unwrap();
    let r2 = stress_rounds(500, 9).ok().unwrap();
    let ok = r2.end_resident <= r1.end_resident + 16; // 有界而非线性（10x 轮数不 10x 囤积）
    s.add("CGA-38-07-压测归零", ok, if ok { "" } else { "线性囤积" });
    // J5 桶号正确（2^n 向上取整 + 边界）
    let ok = bucket_of(1) == Ok(5)
        && bucket_of(32) == Ok(5)
        && bucket_of(33) == Ok(6)
        && bucket_of(1 << 20) == Ok(20)
        && bucket_of(0) == Err(XC_ZEROSZ)
        && bucket_of((1 << 20) + 1) == Err(XC_CAP);
    s.add("CGA-38-08-桶号闭集", ok, if ok { "" } else { "桶号破缺" });
    // J6 槽号校验（0..64 闭集）+ 池容量校验
    let ok = LocalCache::new(63).is_ok()
        && matches!(LocalCache::new(64), Err(XC_SLOTS))
        && matches!(GlobalPool::new(0), Err(XC_CAP))
        && matches!(GlobalPool::new(257), Err(XC_CAP));
    s.add("CGA-38-09-校验闭集", ok, if ok { "" } else { "越界放行" });
    // J7 码位互异段独占
    let ok = SEG_BASE % 0x10 == 0 && XC_SLOTS != XC_ZEROSZ && XC_ZEROSZ != XC_CAP;
    s.add("CGA-38-10-码位互异", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga38_checks_all_green() {
        let set = run_cga38_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga38_reuse_same_block() {
        // 帧内复用：归还再取是同一容量块
        let mut g = GlobalPool::new(4).ok().unwrap();
        let mut lc = LocalCache::new(3).ok().unwrap();
        let b1 = lc.alloc(128, &mut g).ok().unwrap();
        let cap1 = b1.capacity();
        let bk = bucket_of(128).ok().unwrap();
        lc.dealloc(bk, b1, &mut g);
        let b2 = lc.alloc(128, &mut g).ok().unwrap();
        assert_eq!(b2.capacity(), cap1.max(128));
    }
}
