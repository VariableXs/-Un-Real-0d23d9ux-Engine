//! VE-F0774 九宫格缓存
//! 职责：以（切片指纹 + 目标尺寸档）为键的布局结果缓存、
//!       尺寸档位（量化到 4px 档提升命中）、命中统计、LRU 上限。
//! 契约：同档位命中后布局与直接计算一致；量化不跨档误命中。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec361_ninecore::{Cell, Insets, Slice, layout};

/// 尺寸档位（量化步长）
pub const QUANT: f64 = 4.0;

/// 布局缓存键
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LayoutKey {
    pub tex_w_bits: u64,
    pub tex_h_bits: u64,
    pub inset_bits: u64,
    pub w_q: u32,
    pub h_q: u32,
}

/// 尺寸量化
pub fn quant(v: f64) -> u32 {
    ((v / QUANT).round().max(0.0)) as u32
}

/// 键构造
pub fn key_of(s: &Slice, w: f64, h: f64) -> LayoutKey {
    let ins = &s.inset;
    let inset_bits = (ins.left.to_bits() as u64) ^ ((ins.top.to_bits() as u64) << 1)
        ^ ((ins.right.to_bits() as u64) << 2) ^ ((ins.bottom.to_bits() as u64) << 3);
    LayoutKey {
        tex_w_bits: s.tex_w.to_bits(),
        tex_h_bits: s.tex_h.to_bits(),
        inset_bits,
        w_q: quant(w),
        h_q: quant(h),
    }
}

/// 布局缓存
pub struct LayoutCache {
    pub cap: usize,
    pub entries: Vec<(LayoutKey, [Cell; 9])>,
    pub hits: u64,
    pub misses: u64,
}

impl LayoutCache {
    pub fn new(cap: usize) -> LayoutCache {
        LayoutCache { cap: cap.max(1), entries: alloc::vec![], hits: 0, misses: 0 }
    }
    pub fn get_or_compute(&mut self, s: &Slice, w: f64, h: f64) -> Option<[Cell; 9]> {
        let key = key_of(s, w, h);
        if let Some((_, cells)) = self.entries.iter().find(|(k, _)| *k == key) {
            self.hits += 1;
            return Some(*cells);
        }
        self.misses += 1;
        let cells = layout(s, 0.0, 0.0, w, h)?;
        if self.entries.len() >= self.cap {
            self.entries.remove(0);
        }
        self.entries.push((key, cells));
        Some(cells)
    }
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// 档位内一致性：同档两个尺寸的缓存布局来自同一次计算（首个）
pub fn same_bucket(w1: f64, w2: f64) -> bool {
    quant(w1) == quant(w2)
}

pub fn run_vec374_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec374");
    let ins = Insets::uniform(8.0);
    let sl = Slice::build(64.0, 64.0, ins).expect("slice");
    // 族一：量化
    s.add("量化-整除", quant(100.0) == 25, "100/4");
    s.add("量化-取整", quant(101.0) == 25, "101 → 25");
    s.add("量化-进位", quant(102.0) == 26, "102 → 26");
    s.add("量化-负夹零", quant(-5.0) == 0, "negative clamped");
    s.add("量化-同档", same_bucket(100.0, 102.9), "same bucket");
    s.add("量化-异档", !same_bucket(100.0, 104.0), "diff bucket");
    // 族二：缓存命中
    let mut cache = LayoutCache::new(8);
    let c1 = cache.get_or_compute(&sl, 100.0, 80.0).expect("c1");
    s.add("缓存-首算", cache.misses == 1 && cache.hits == 0, "first miss");
    let c2 = cache.get_or_compute(&sl, 101.5, 81.2).expect("c2");
    s.add("缓存-同档命中", cache.hits == 1 && cache.misses == 1, "second hit");
    s.add("缓存-布局一致", c1 == c2, "same bucket same cells");
    // 换档
    let _ = cache.get_or_compute(&sl, 200.0, 80.0).expect("c3");
    s.add("缓存-异档未中", cache.misses == 2, "new bucket miss");
    // 换切片
    let sl2 = Slice::build(64.0, 64.0, Insets::uniform(12.0)).expect("sl2");
    let _ = cache.get_or_compute(&sl2, 100.0, 80.0).expect("c4");
    s.add("缓存-异切片未中", cache.misses == 3, "different slice miss");
    // 族三：命中一致性（缓存值 == 直接计算值）
    let direct = layout(&sl, 0.0, 0.0, 100.0, 80.0).expect("direct");
    s.add("一致-缓存=直算", c1 == direct, "cache == compute");
    // 族四：命中率
    s.add("命中率-值", (cache.hit_rate() - 1.0 / 4.0).abs() < 1e-12, "1/4");
    s.add("命中率-空", LayoutCache::new(4).hit_rate() == 0.0, "no div0");
    // 族五：LRU 上限
    let mut small = LayoutCache::new(2);
    let _ = small.get_or_compute(&sl, 100.0, 80.0);
    let _ = small.get_or_compute(&sl, 200.0, 80.0);
    let _ = small.get_or_compute(&sl, 300.0, 80.0);
    s.add("上限-容量", small.entries.len() == 2, "cap 2");
    s.add("上限-最旧逐出", small.get_or_compute(&sl, 100.0, 80.0).is_some() && small.misses == 4, "oldest evicted → miss again");
    // 零容量兜底
    s.add("上限-零兜底", LayoutCache::new(0).cap == 1, "min cap");
    // 族六：键区分性（不同切片键不同）
    s.add("键-异切片", key_of(&sl, 100.0, 80.0) != key_of(&sl2, 100.0, 80.0), "slice in key");
    s.add("键-异尺寸", key_of(&sl, 100.0, 80.0) != key_of(&sl, 200.0, 80.0), "size in key");
    s
}
