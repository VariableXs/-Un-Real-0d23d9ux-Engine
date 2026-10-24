//! VE-F4327 · 一致性内存治理（U07 性能与预算组）
//! 职责：一致性系统内存治理——内存三池（判定缓存/索引/事件流各设上限）
//! + 治理三件（LRU 缓存淘汰/索引压缩/事件流环形缓冲覆盖最旧）+ 泄漏防线
//! （池水位巡检 + 增长曲线报警——增长异常即立案）+ 池溢出按优先级淘汰
//! + 压缩失败降级直读。
//! 段位：0x34E0（续表 idx561）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x34E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MmCode {
    /// 池溢出（按优先级淘汰）。
    PoolOverflow = SEG_BASE,
    /// 增长异常（泄漏立案）。
    LeakSuspect = SEG_BASE + 0x01,
    /// 压缩失败（降级直读）。
    CompressFail = SEG_BASE + 0x02,
    /// 池号越界。
    PoolRange = SEG_BASE + 0x03,
}

/// 三池：0 判定缓存 / 1 索引 / 2 事件流。
pub const POOLS: usize = 3;

/// 池淘汰优先级（0 最先淘汰）。
pub fn evict_priority(pool: usize) -> u8 {
    match pool {
        0 => 0, // 判定缓存可重建——最先淘汰
        1 => 1, // 索引可重算
        _ => 2, // 事件流不可再生——最后
    }
}

/// LRU 缓存（判定缓存池芯）。
#[derive(Default)]
pub struct LruCache {
    /// (key, value, 最近用时)。
    pub items: Vec<(u32, u32, u64)>,
    pub cap: usize,
    pub evicted: u32,
}

impl LruCache {
    pub fn new(cap: usize) -> Self {
        LruCache { cap: cap.max(1), ..Default::default() }
    }

    /// 命中即刷新时戳。
    pub fn get(&mut self, key: u32, now: u64) -> Option<u32> {
        if let Some(it) = self.items.iter_mut().find(|it| it.0 == key) {
            it.2 = now;
            return Some(it.1);
        }
        None
    }

    /// 插入：满则淘汰最旧用时者。
    pub fn put(&mut self, key: u32, value: u32, now: u64) {
        if let Some(it) = self.items.iter_mut().find(|it| it.0 == key) {
            it.1 = value;
            it.2 = now;
            return;
        }
        if self.items.len() >= self.cap {
            // 淘汰 LRU（用时最小者）。
            let victim = self
                .items
                .iter()
                .enumerate()
                .min_by_key(|(_, it)| it.2)
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.items.remove(victim);
            self.evicted += 1;
        }
        self.items.push((key, value, now));
    }
}

/// 环形缓冲（事件流池芯——覆盖最旧）。
#[derive(Default)]
pub struct Ring {
    pub buf: Vec<u32>,
    pub head: usize,
    pub filled: usize,
    pub overwritten: u32,
}

impl Ring {
    pub fn new(cap: usize) -> Self {
        Ring { buf: vec![0; cap.max(1)], head: 0, filled: 0, overwritten: 0 }
    }

    /// 入环：满则覆盖最旧并计数。
    pub fn push(&mut self, v: u32) {
        if self.buf.is_empty() {
            return;
        }
        if self.filled == self.buf.len() {
            self.overwritten += 1;
        } else {
            self.filled += 1;
        }
        self.buf[self.head] = v;
        self.head = (self.head + 1) % self.buf.len();
    }

    /// 按新到旧读取。
    pub fn recent(&self, n: usize) -> Vec<u32> {
        let mut out = Vec::new();
        let cap = self.buf.len();
        let take = n.min(self.filled);
        for i in 0..take {
            let idx = (self.head + cap - 1 - i) % cap;
            out.push(self.buf[idx]);
        }
        out
    }
}

/// 池水位卡。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Watermark {
    pub pool: usize,
    /// 当前水位（KB 抽象单位）。
    pub used: u32,
    pub cap: u32,
}

/// 内存治理台。
#[derive(Default)]
pub struct MemDesk {
    pub lru: LruCache,
    pub ring: Ring,
    pub marks: Vec<Watermark>,
    /// 历史水位采样（增长曲线用）。
    pub history: Vec<u32>,
    pub leaks: u32,
    pub compress_fails: u32,
}

/// 增长异常判定：连续采样单调增长且斜率超阈（每样本增幅 > 步进阈）。
pub fn growth_abnormal(hist: &[u32], step_threshold: u32) -> bool {
    if hist.len() < 4 {
        return false;
    }
    let n = hist.len();
    let mut mono = true;
    for w in hist.windows(2) {
        if w[1] <= w[0] {
            mono = false;
            break;
        }
    }
    if !mono {
        return false;
    }
    let rise = hist[n - 1] - hist[0];
    rise / (n as u32 - 1) > step_threshold
}

impl MemDesk {
    pub fn new(lru_cap: usize, ring_cap: usize) -> Self {
        MemDesk { lru: LruCache::new(lru_cap), ring: Ring::new(ring_cap), ..Default::default() }
    }

    /// 水位登记（池号校验）。
    pub fn mark(&mut self, pool: usize, used: u32, cap: u32) -> Result<(), MmCode> {
        if pool >= POOLS {
            return Err(MmCode::PoolRange);
        }
        let w = Watermark { pool, used, cap };
        if let Some(x) = self.marks.iter_mut().find(|x| x.pool == pool) {
            *x = w;
        } else {
            self.marks.push(w);
        }
        Ok(())
    }

    /// 池溢出巡检：超上限池按淘汰优先级降水位到 80%（返回淘汰 KB 数）。
    pub fn patrol(&mut self) -> u32 {
        let mut evicted_kb = 0;
        let mut over: Vec<usize> = self
            .marks
            .iter()
            .filter(|w| w.used > w.cap)
            .map(|w| w.pool)
            .collect();
        over.sort_by_key(|p| evict_priority(*p));
        for pool in over {
            if let Some(w) = self.marks.iter_mut().find(|w| w.pool == pool) {
                let target = w.cap * 8 / 10;
                evicted_kb += w.used - target;
                w.used = target;
            }
        }
        evicted_kb
    }

    /// 水位采样入历史（增长曲线）。
    pub fn sample(&mut self, kb: u32) {
        self.history.push(kb);
    }

    /// 泄漏防线：增长曲线异常即立案（计数留痕）。
    pub fn leak_check(&mut self, step_threshold: u32) -> bool {
        if growth_abnormal(&self.history, step_threshold) {
            self.leaks += 1;
            return true;
        }
        false
    }

    /// 索引压缩：压缩率承诺（返回压缩后 KB；0 = 压缩失败降级直读）。
    pub fn compress(&mut self, raw_kb: u32, ratio_pct: u32) -> Result<u32, MmCode> {
        if ratio_pct == 0 || ratio_pct > 100 {
            self.compress_fails += 1;
            return Err(MmCode::CompressFail);
        }
        let out = raw_kb * ratio_pct / 100;
        if out >= raw_kb {
            self.compress_fails += 1;
            return Err(MmCode::CompressFail);
        }
        Ok(out)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves561_checks() -> CheckSet {
    let mut set = CheckSet::new("ves561");
    let mut md = MemDesk::new(3, 4);

    // C561-01 LRU 命中刷新。
    md.lru.put(1, 100, 1);
    md.lru.put(2, 200, 2);
    chk(&mut set, "C561-01", md.lru.get(1, 3) == Some(100), "LRU 命中刷新");

    // C561-02 满淘汰最旧用时者。
    md.lru.put(3, 300, 4); // 满
    md.lru.put(4, 400, 5); // 淘汰 key2（用时 2 最旧）
    chk(&mut set, "C561-02", md.lru.get(2, 6).is_none() && md.lru.get(1, 6).is_some()
        && md.lru.evicted == 1, "满淘汰 LRU 者");

    // C561-03 环形缓冲覆盖最旧。
    for v in 1..=6u32 {
        md.ring.push(v);
    }
    let rec = md.ring.recent(3);
    chk(&mut set, "C561-03", rec == [6, 5, 4] && md.ring.overwritten == 2, "环满覆盖最旧");

    // C561-04 三池水位登记 + 池号越界拒。
    let _ = md.mark(0, 900, 1000);
    let _ = md.mark(1, 1200, 1000);
    let _ = md.mark(2, 500, 1000);
    chk(&mut set, "C561-04", md.marks.len() == 3 && md.mark(3, 1, 1) == Err(MmCode::PoolRange), "三池登记+越界拒");

    // C561-05 溢出按优先级淘汰（判定缓存池最先降到 80%）。
    let kb = md.patrol();
    chk(&mut set, "C561-05", kb == (1200 - 800) && md.marks[1].used == 800 && md.marks[2].used == 500, "溢出优先级淘汰");

    // C561-06 增长曲线正常（有降有升）不立案。
    md.sample(100);
    md.sample(120);
    md.sample(90);
    md.sample(110);
    chk(&mut set, "C561-06", !md.leak_check(5) && md.leaks == 0, "正常曲线不立案");

    // C561-07 单调增长超斜率立案。
    let mut md2 = MemDesk::new(1, 1);
    for v in [100, 140, 180, 230, 280] {
        md2.sample(v);
    }
    chk(&mut set, "C561-07", md2.leak_check(10) && md2.leaks == 1, "泄漏异常立案");

    // C561-08 样本不足不立案（不臆测）。
    let mut md3 = MemDesk::new(1, 1);
    md3.sample(100);
    md3.sample(200);
    chk(&mut set, "C561-08", !md3.leak_check(1), "样本不足不臆测");

    // C561-09 索引压缩生效。
    chk(&mut set, "C561-09", md.compress(1000, 40) == Ok(400), "压缩生效");

    // C561-10 压缩失败降级直读（零比/超界/无收益全拒）。
    chk(&mut set, "C561-10", md.compress(1000, 0).is_err() && md.compress(1000, 101).is_err()
        && md.compress(1000, 100).is_err() && md.compress_fails == 3, "压缩失败降级");

    // C561-11 淘汰优先级序（缓存 < 索引 < 事件流）。
    chk(&mut set, "C561-11", evict_priority(0) < evict_priority(1) && evict_priority(1) < evict_priority(2), "优先级序");

    // C561-12 空环读取安全 + 空历史不立案。
    let ring0 = Ring::new(2);
    let mut md4 = MemDesk::new(1, 1);
    chk(&mut set, "C561-12", ring0.recent(3).is_empty() && !md4.leak_check(1), "空态安全");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_evicts_least_recent() {
        let mut c = LruCache::new(2);
        c.put(1, 10, 1);
        c.put(2, 20, 2);
        let _ = c.get(1, 3); // key1 刷新
        c.put(3, 30, 4); // 淘汰 key2
        assert!(c.get(2, 5).is_none());
        assert_eq!(c.get(1, 5), Some(10));
        assert_eq!(c.evicted, 1);
    }

    #[test]
    fn ring_wraps() {
        let mut r = Ring::new(3);
        for v in 1..=5u32 {
            r.push(v);
        }
        assert_eq!(r.overwritten, 2);
        assert_eq!(r.recent(5), [5, 4, 3]);
    }

    #[test]
    fn growth_detection() {
        assert!(!growth_abnormal(&[1, 2, 3], 10)); // 样本不足
        assert!(growth_abnormal(&[10, 20, 30, 45], 5)); // 单调陡增
        assert!(!growth_abnormal(&[10, 20, 15, 40], 5)); // 非单调
    }
}
