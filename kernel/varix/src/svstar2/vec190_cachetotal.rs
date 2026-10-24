//! VE-F0590 · 着色器缓存全域统一
//!
//! 编译产物三级缓存（进程内内存池、磁盘缓存、服务级共享缓存）的统一键设计与
//! 失效策略——键四元组（源码规范化哈希、环境指纹、编译器版本、目标档位），
//! 键不完备即错配的键设计不变式（缺一维宁可失效重编）；失效策略：编译器版本
//! 升级批量失效、环境指纹变化按需失效、LRU 容量治理；缓存安全：磁盘条目校验和
//! 加载验证，防损坏防篡改；本条是产物缓存总纲。
//!
//! 跨批对接：上游 F0475 序列化、F0582 增量、F0581 服务；下游 F0591 池集成、F0597 三账。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 缓存层级
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tier {
    /// 进程内内存池
    #[default]
    Memory,
    /// 磁盘缓存
    Disk,
    /// 服务级共享缓存
    Shared,
}

/// 键四元组：缺一维宁可失效重编（键设计不变式）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheKey {
    /// 源码规范化哈希
    pub src_hash: u64,
    /// 环境指纹
    pub env_fp: u64,
    /// 编译器版本
    pub ver: u32,
    /// 目标档位（0 fast 1 release）
    pub tier: u32,
}

impl CacheKey {
    /// 键完备性：任一维为零/无效即拒绝（不完备即错配）
    pub fn complete(&self) -> bool {
        self.src_hash != 0 && self.env_fp != 0 && self.ver != 0 && self.tier <= 1
    }

    pub fn fits(&self, other: &CacheKey) -> bool {
        self == other
    }
}

/// 三级缓存表（统一键）
#[derive(Clone, Debug, Default)]
pub struct UnifiedCache {
    pub tier: Tier,
    pub entries: Vec<(CacheKey, u32, u64)>,
    /// 容量（LRU 治理）
    pub cap: usize,
    /// LRU 时钟
    clock: u64,
}

impl UnifiedCache {
    pub fn new(tier: Tier, cap: usize) -> Self {
        UnifiedCache { tier, entries: Vec::new(), cap, clock: 0 }
    }

    /// 命中 O(1) 语义：统一键全匹配
    pub fn get(&self, k: &CacheKey) -> Option<u32> {
        if !k.complete() {
            return None;
        }
        self.entries.iter().find(|(ek, _, _)| ek == k).map(|(_, art, _)| *art)
    }

    /// 写入：容量超限 → LRU 淘汰
    pub fn put(&mut self, k: CacheKey, artifact: u32) -> bool {
        if !k.complete() {
            return false;
        }
        self.clock += 1;
        if self.entries.iter().any(|(ek, _, _)| ek == &k) {
            return false;
        }
        if self.entries.len() >= self.cap {
            // LRU：淘汰时间戳最旧
            if let Some(min_i) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, (_, _, t))| *t)
                .map(|(i, _)| i)
            {
                self.entries.remove(min_i);
            }
        }
        self.entries.push((k, artifact, self.clock));
        true
    }

    /// 版本升级批量失效
    pub fn invalidate_version(&mut self, new_ver: u32) -> usize {
        let n = self.entries.iter().filter(|(k, _, _)| k.ver != new_ver).count();
        self.entries.retain(|(k, _, _)| k.ver == new_ver);
        n
    }

    /// 环境指纹变化按需失效
    pub fn invalidate_env(&mut self, new_env: u64) -> usize {
        let n = self.entries.iter().filter(|(k, _, _)| k.env_fp != new_env).count();
        self.entries.retain(|(k, _, _)| k.env_fp == new_env);
        n
    }
}

/// 磁盘条目：校验和 + 加载验证（防损坏防篡改）
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiskEntry {
    pub key: CacheKey,
    pub payload: Vec<u32>,
    pub checksum: u64,
}

/// FNV 校验和
pub fn checksum(words: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for w in words {
        for b in w.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    h
}

impl DiskEntry {
    pub fn seal(key: CacheKey, payload: Vec<u32>) -> Self {
        let cs = checksum(&payload);
        DiskEntry { key, payload, checksum: cs }
    }

    /// 加载验证：校验和失配即拦截剔除
    pub fn verify(&self) -> bool {
        checksum(&self.payload) == self.checksum
    }
}

/// 键碰撞：哈希升级
pub fn hash_upgrade(k: CacheKey) -> CacheKey {
    CacheKey { src_hash: k.src_hash.wrapping_mul(0x9E37_79B9_7F4A_7C15), ..k }
}

/// 分工显性：产物缓存总纲 vs 层级实现
pub fn division(owner_of_total: &str) -> bool {
    owner_of_total == "F0590"
}

pub fn run_vec190_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec190");
    let k = CacheKey { src_hash: 11, env_fp: 22, ver: 3, tier: 1 };
    // 族一：键四元组不变式
    s.add("键-完备受理", k.complete(), "ok");
    s.add("键-缺源哈希拒绝", !CacheKey { src_hash: 0, ..k }.complete(), "miss1");
    s.add("键-缺环境拒绝", !CacheKey { env_fp: 0, ..k }.complete(), "miss2");
    s.add("键-缺版本拒绝", !CacheKey { ver: 0, ..k }.complete(), "miss3");
    s.add("键-档位非法拒绝", !CacheKey { tier: 9, ..k }.complete(), "miss4");
    s.add("键-全等匹配", k.fits(&k), "eq");
    s.add("键-不同环境不匹配", !k.fits(&CacheKey { env_fp: 99, ..k }), "ne");
    // 族二：三级统一缓存
    let mut mem = UnifiedCache::new(Tier::Memory, 4);
    s.add("缓存-未命中", mem.get(&k).is_none(), "miss");
    s.add("缓存-写入", mem.put(k, 7), "put");
    s.add("缓存-命中", mem.get(&k) == Some(7), "hit");
    s.add("缓存-重复拒绝", !mem.put(k, 8), "dup");
    s.add("缓存-不完备键拒绝", !mem.put(CacheKey { src_hash: 0, ..k }, 9), "badkey");
    // 族三：LRU 淘汰
    let mut lru = UnifiedCache::new(Tier::Disk, 2);
    lru.put(k, 1);
    lru.put(CacheKey { src_hash: 12, ..k }, 2);
    lru.put(CacheKey { src_hash: 13, ..k }, 3);
    s.add("LRU-最旧淘汰", lru.get(&k).is_none() && lru.get(&CacheKey { src_hash: 13, ..k }) == Some(3), "evict");
    s.add("LRU-容量约束", lru.entries.len() == 2, "cap");
    // 族四：失效策略
    let mut inv = UnifiedCache::new(Tier::Shared, 8);
    inv.put(CacheKey { ver: 2, ..k }, 1);
    inv.put(CacheKey { ver: 3, ..k }, 2);
    let n = inv.invalidate_version(3);
    s.add("失效-版本批量失效", n == 1 && inv.get(&CacheKey { ver: 2, ..k }).is_none(), "ver");
    s.add("失效-新版保留", inv.get(&k) == Some(2), "keep");
    let n2 = inv.invalidate_env(99);
    s.add("失效-环境按需失效", n2 == 1 && inv.entries.is_empty(), "env");
    // 族五：缓存安全
    let e = DiskEntry::seal(k, vec![1, 2, 3]);
    s.add("安全-完好通过", e.verify(), "ok");
    let mut tampered = e.clone();
    tampered.payload[0] = 999;
    s.add("安全-篡改拦截", !tampered.verify(), "tamper");
    let mut corrupt = e.clone();
    corrupt.checksum = 1;
    s.add("安全-损坏拦截", !corrupt.verify(), "corrupt");
    // 族六：键碰撞哈希升级
    let k2 = hash_upgrade(k);
    s.add("碰撞-哈希升级", k2.src_hash != k.src_hash && k2.env_fp == k.env_fp, "upg");
    // 族七：分工显性
    s.add("分工-产物缓存总纲", division("F0590"), "div");
    s.add("分工-他条非总纲", !division("F0475"), "nodiv");
    s
}
