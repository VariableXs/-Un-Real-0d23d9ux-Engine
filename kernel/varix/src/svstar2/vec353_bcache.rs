//! VE-F0753 布尔结果缓存
//! 职责：以（输入对指纹 + 运算 + 规则 + 容差档）为键的布尔结果缓存、
//!       命中/未命中统计、指纹碰撞防御（二级形状摘要）、LRU 上限。
//! 契约：同键必命中；不同形状同指纹（碰撞）不得错误命中。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::{BoolOp, FillRule, Poly};

/// 形状指纹（面积 + 顶点数 + 首末点散列的合成）
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ShapeKey {
    pub verts: u32,
    pub area_bits: u64,
    pub seed: u64,
}

/// 形状摘要（防御指纹碰撞的二级校验）
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ShapeDigest {
    pub area: f64,
    pub centroid: (f64, f64),
}

pub fn key_of(p: &Poly) -> ShapeKey {
    let n = p.verts.len();
    let area = p.area();
    let mut seed = 0xcbf29ce484222325u64;
    for v in &p.verts {
        let x = v.x.to_bits();
        let y = v.y.to_bits();
        seed ^= x;
        seed = seed.wrapping_mul(0x100000001b3);
        seed ^= y;
        seed = seed.wrapping_mul(0x100000001b3);
    }
    ShapeKey { verts: n as u32, area_bits: area.to_bits(), seed }
}

pub fn digest_of(p: &Poly) -> ShapeDigest {
    let n = p.verts.len().max(1) as f64;
    let cx = p.verts.iter().map(|v| v.x).sum::<f64>() / n;
    let cy = p.verts.iter().map(|v| v.y).sum::<f64>() / n;
    ShapeDigest { area: p.area(), centroid: (cx, cy) }
}

/// 缓存键
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CacheKey {
    pub ka: ShapeKey,
    pub kb: ShapeKey,
    pub op: BoolOp,
    pub rule: FillRule,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CacheEntry {
    pub result_verts: usize,
    pub result_area: f64,
    pub digest_a: ShapeDigest,
    pub digest_b: ShapeDigest,
}

/// 布尔结果缓存（简化 LRU：容量上限，满时逐最旧）
pub struct BoolCache {
    pub cap: usize,
    pub entries: Vec<(CacheKey, CacheEntry)>,
    pub hits: u64,
    pub misses: u64,
}

impl BoolCache {
    pub fn new(cap: usize) -> BoolCache {
        BoolCache { cap: cap.max(1), entries: alloc::vec![], hits: 0, misses: 0 }
    }
    pub fn get(&self, k: CacheKey, a: &Poly, b: &Poly) -> Option<&CacheEntry> {
        let e = self.entries.iter().find(|(key, _)| *key == k);
        match e {
            Some((_, entry)) => {
                // 二级摘要防御
                if entry.digest_a == digest_of(a) && entry.digest_b == digest_of(b) {
                    Some(entry)
                } else {
                    None // 指纹碰撞 → 视为未命中
                }
            }
            None => None,
        }
    }
    pub fn hit(&mut self, k: CacheKey, a: &Poly, b: &Poly) -> bool {
        if self.get(k, a, b).is_some() {
            self.hits += 1;
            true
        } else {
            self.misses += 1;
            false
        }
    }
    pub fn put(&mut self, k: CacheKey, a: &Poly, b: &Poly, result: &Poly) {
        if self.entries.len() >= self.cap {
            self.entries.remove(0);
        }
        self.entries.push((k, CacheEntry {
            result_verts: result.verts.len(),
            result_area: result.area(),
            digest_a: digest_of(a),
            digest_b: digest_of(b),
        }));
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

pub fn run_vec353_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec353");
    let a = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    let b = Poly::new(alloc::vec![Pt::new(1.0, 1.0), Pt::new(3.0, 1.0), Pt::new(3.0, 3.0), Pt::new(1.0, 3.0)]);
    // 族一：指纹
    let ka = key_of(&a);
    s.add("指纹-稳定", key_of(&a) == ka, "deterministic");
    s.add("指纹-顶点数", ka.verts == 4, "4 verts");
    s.add("指纹-异形异键", key_of(&a) != key_of(&b), "different keys");
    // 平移后键变（顶点位参与散列）
    let a_moved = Poly::new(a.verts.iter().map(|v| v.add(Pt::new(1.0, 0.0))).collect());
    s.add("指纹-位置敏感", key_of(&a) != key_of(&a_moved), "position sensitive");
    // 摘要
    let da = digest_of(&a);
    s.add("摘要-面积", da.area == 16.0, "area");
    s.add("摘要-质心", da.centroid.0 == 2.0 && da.centroid.1 == 2.0, "centroid (2,2)");
    // 族二：缓存命中
    let mut cache = BoolCache::new(8);
    let key = CacheKey { ka, kb: key_of(&b), op: BoolOp::Inter, rule: FillRule::NonZero };
    let result = Poly::new(alloc::vec![Pt::new(1.0, 1.0), Pt::new(3.0, 1.0), Pt::new(3.0, 3.0), Pt::new(1.0, 3.0)]);
    cache.put(key, &a, &b, &result);
    s.add("缓存-命中", cache.hit(key, &a, &b), "hit");
    s.add("缓存-计数", cache.hits == 1 && cache.misses == 0, "1 hit");
    s.add("缓存-取回", cache.get(key, &a, &b).expect("e").result_area == 4.0, "area 4");
    // 未命中（不同键）
    let key2 = CacheKey { ka, kb: key_of(&b), op: BoolOp::Union, rule: FillRule::NonZero };
    s.add("缓存-异键未中", !cache.hit(key2, &a, &b), "miss on op change");
    s.add("缓存-计数二", cache.misses == 1, "1 miss");
    // 族三：碰撞防御（同键不同形状）
    let a2 = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(8.0, 0.0), Pt::new(8.0, 8.0), Pt::new(0.0, 8.0)]);
    // a2 与 a 顶点数同、area_bits 不同 → 键不同；但构造「键同形异」场景：直接用 key 查 a2
    s.add("碰撞-摘要拒绝", cache.get(key, &a2, &b).is_none(), "digest mismatch → none");
    // 族四：命中率
    let rate = cache.hit_rate();
    s.add("命中率-值", (rate - 0.5).abs() < 1e-12, "1/2");
    let empty = BoolCache::new(4);
    s.add("命中率-空零", empty.hit_rate() == 0.0, "no div0");
    // 族五：LRU 上限
    let mut small = BoolCache::new(2);
    let c = Poly::new(alloc::vec![Pt::new(9.0, 9.0), Pt::new(9.5, 9.0), Pt::new(9.5, 9.5)]);
    small.put(key, &a, &b, &result);
    small.put(key2, &a, &b, &result);
    small.put(CacheKey { ka, kb: key_of(&c), op: BoolOp::Diff, rule: FillRule::EvenOdd }, &a, &c, &result);
    s.add("上限-容量", small.entries.len() == 2, "cap 2");
    s.add("上限-最旧逐出", small.get(key, &a, &b).is_none(), "oldest evicted");
    // 族六：零容量兜底为 1
    let z = BoolCache::new(0);
    s.add("零容量-兜底", z.cap == 1, "min cap 1");
    s
}
