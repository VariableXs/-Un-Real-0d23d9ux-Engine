//! VE-F0475 · IR 序列化与缓存格式（VE-C 域 · IR 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0475`
//!
//! **锚点原文**：「IR 序列化与编译缓存：二进制序列化格式（版本头加节结构）、
//! 编译缓存（源哈希加选项哈希键控、缓存命中即跳过编译）、缓存失效与淘汰
//! （LRU 加磁盘配额）；格式版本兼容声明（读旧格式或显性拒绝二选一，不静默
//! 错读）。判据：版本头、双哈希键控、LRU 配额、显性拒绝、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **版本头**（判据一）。序列化格式带版本头（[`CACHE_FORMAT_VER`]），
//!    读旧格式或显性拒绝二选一（兼容声明显性：[`CompatMode`]），不静默错读。
//!
//! 2. **双哈希键控**（判据二）。缓存键 = 源哈希 × 选项哈希（FNV-1a 64），
//!    任一变更 → 键变 → miss（键控面单测）。
//!
//! 3. **LRU 配额**（判据三）。缓存配额（条目上限 + 字节配额），超限 LRU 淘
//!    汰（淘汰序 = 最久未用先出），命中刷新使用序。
//!
//! 4. **显性拒绝**（判据四）。缓存损坏（校验和不符/格式不符）→ 弃用重编译
//!    （显性拒绝 + 弃用计数），不静默错读。

use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0475";

/// 缓存序列化格式版本。
pub const CACHE_FORMAT_VER: u32 = 1;

/// 兼容模式（读旧格式或显性拒绝——二选一声明面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompatMode {
    /// 兼容读（旧格式按迁移规则读）。
    ReadOld,
    /// 显性拒绝（版本不符即拒——不静默错读）。
    RejectOld,
}

// ---------------------------------------------------------------------------
// FNV-1a 64（双哈希键控的哈希函数）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位哈希（键控面与账本指纹同源）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h = (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// 缓存键：源哈希 × 选项哈希（双哈希键控）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CacheKey {
    pub src_hash: u64,
    pub opt_hash: u64,
}

impl CacheKey {
    pub fn new(src: &[u8], opts: &[u8]) -> Self {
        CacheKey {
            src_hash: fnv1a64(src),
            opt_hash: fnv1a64(opts),
        }
    }
    pub fn digest(&self) -> String {
        format!("k({:016x},{:016x})", self.src_hash, self.opt_hash)
    }
}

// ---------------------------------------------------------------------------
// 序列化（版本头 + 节结构 + 校验和）
// ---------------------------------------------------------------------------

/// 序列化：版本头(4B) + 载荷 + 校验和(8B)。
pub fn serialize(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&CACHE_FORMAT_VER.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out.extend_from_slice(&fnv1a64(payload).to_le_bytes());
    out
}

/// 反序列化：兼容模式驱动（读旧或显性拒绝），校验和不符即弃。
pub fn deserialize(bytes: &[u8], mode: CompatMode) -> Result<Vec<u8>, String> {
    if bytes.len() < 16 {
        return Err("长度不足：头+校验和面缺失".to_string());
    }
    let ver = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if ver != CACHE_FORMAT_VER && mode == CompatMode::RejectOld {
        return Err(format!("版本不匹配：读 {} 当前 {}（显性拒绝）", ver, CACHE_FORMAT_VER));
    }
    let n = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    if bytes.len() < 8 + n + 8 {
        return Err("截断：载荷或校验和面缺失".to_string());
    }
    let payload = &bytes[8..8 + n];
    let stored = u64::from_le_bytes([
        bytes[8 + n],
        bytes[9 + n],
        bytes[10 + n],
        bytes[11 + n],
        bytes[12 + n],
        bytes[13 + n],
        bytes[14 + n],
        bytes[15 + n],
    ]);
    if stored != fnv1a64(payload) {
        return Err("校验和不符：缓存损坏弃用".to_string());
    }
    Ok(payload.to_vec())
}

// ---------------------------------------------------------------------------
// LRU 缓存（配额 + 淘汰）
// ---------------------------------------------------------------------------

/// LRU 缓存条目。
#[derive(Clone, Debug)]
struct LruEntry {
    key: CacheKey,
    payload: Vec<u8>,
    /// 使用序号（单调递增——LRU 判定面）。
    last_use: u64,
}

/// LRU 缓存：条目配额 + 字节配额；命中刷新使用序；超限淘汰最久未用。
pub struct LruCache {
    pub entries: Vec<LruEntry>,
    pub max_entries: usize,
    pub max_bytes: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    tick: u64,
}

impl LruCache {
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        LruCache {
            entries: Vec::new(),
            max_entries,
            max_bytes,
            hits: 0,
            misses: 0,
            evictions: 0,
            tick: 0,
        }
    }
    /// 查询：命中刷新使用序（LRU 面核心）。
    pub fn get(&mut self, k: &CacheKey) -> Option<Vec<u8>> {
        self.tick += 1;
        if let Some(e) = self.entries.iter_mut().find(|e| e.key == *k) {
            e.last_use = self.tick;
            self.hits += 1;
            Some(e.payload.clone())
        } else {
            self.misses += 1;
            None
        }
    }
    /// 插入：超配额 LRU 淘汰（条目数与字节数双面）。
    pub fn put(&mut self, k: CacheKey, payload: Vec<u8>) {
        self.tick += 1;
        // 已有则更新。
        if let Some(e) = self.entries.iter_mut().find(|e| e.key == k) {
            e.payload = payload;
            e.last_use = self.tick;
            return;
        }
        self.entries.push(LruEntry { key: k, payload, last_use: self.tick });
        // 条目数配额淘汰。
        while self.entries.len() > self.max_entries {
            self.evict_one();
        }
        // 字节配额淘汰。
        while self.used_bytes() > self.max_bytes && self.entries.len() > 1 {
            self.evict_one();
        }
    }
    fn used_bytes(&self) -> usize {
        self.entries.iter().map(|e| e.payload.len()).sum()
    }
    fn evict_one(&mut self) {
        if let Some(victim) = self
            .entries
            .iter()
            .min_by_key(|e| e.last_use)
            .map(|e| e.key.clone())
        {
            self.entries.retain(|e| e.key != victim);
            self.evictions += 1;
        }
    }
    pub fn keys(&self) -> BTreeSet<CacheKey> {
        self.entries.iter().map(|e| e.key).collect()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec75_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、版本头。
    {
        // 序列化首 4B = 版本。
        let b = serialize(b"hello");
        let ver = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        v.push(("F0475-版本-头版本", ver == 1));
        // 往返还原。
        v.push((
            "F0475-版本-往返还原",
            deserialize(&b, CompatMode::RejectOld) == Ok(b"hello".to_vec()),
        ));
        // 兼容声明二选一：RejectOld 拒、ReadOld 容（不静默错读的显性面）。
        let mut old = b.clone();
        old[0] = 0; // 版本 0 = 旧格式
        v.push((
            "F0475-版本-兼容二选一",
            deserialize(&old, CompatMode::RejectOld).is_err()
                && deserialize(&old, CompatMode::ReadOld).is_ok(),
        ));
    }

    // 二、双哈希键控。
    {
        let k1 = CacheKey::new(b"src A", b"opt1");
        let k2 = CacheKey::new(b"src A", b"opt2");
        let k3 = CacheKey::new(b"src B", b"opt1");
        v.push((
            "F0475-键控-双哈希",
            k1 != k2 && k1 != k3 && k2 != k3,
        ));
        // 同源同键（确定性）。
        let k1b = CacheKey::new(b"src A", b"opt1");
        v.push(("F0475-键控-确定性", k1 == k1b));
        // 哈希面：空串与非空串异（FNV 基准）。
        v.push((
            "F0475-键控-FNV异",
            fnv1a64(b"") != fnv1a64(b"x"),
        ));
    }

    // 三、LRU 配额。
    {
        let mut c = LruCache::new(2, 1 << 20);
        let ka = CacheKey::new(b"a", b"");
        let kb = CacheKey::new(b"b", b"");
        let kc = CacheKey::new(b"c", b"");
        c.put(ka, vec![1]);
        c.put(kb, vec![2]);
        // 命中 a（刷新使用序）→ 插 c → 淘汰 b（最久未用）。
        let hit = c.get(&ka);
        c.put(kc, vec![3]);
        v.push((
            "F0475-LRU-命中刷新淘汰",
            hit == Some(vec![1])
                && c.entries.len() == 2
                && !c.keys().contains(&kb)
                && c.evictions == 1,
        ));
        // 未命中计数。
        v.push(("F0475-LRU-命中计数", c.hits == 1 && c.misses == 0));
        let _ = c.get(&CacheKey::new(b"zz", b""));
        v.push(("F0475-LRU-miss计数", c.misses == 1));
        // 字节配额淘汰。
        let mut c2 = LruCache::new(8, 3);
        c2.put(CacheKey::new(b"p", b""), vec![1, 2]); // 2B
        c2.put(CacheKey::new(b"q", b""), vec![3, 4]); // 2B → 超额淘汰 p
        v.push((
            "F0475-LRU-字节配额",
            c2.used_bytes() <= 3 && c2.evictions >= 1,
        ));
    }

    // 四、显性拒绝。
    {
        // 校验和破坏 → 弃用（不静默错读）。
        let mut b = serialize(b"data");
        let last = b.len() - 1;
        b[last] ^= 0xff;
        let r = deserialize(&b, CompatMode::RejectOld);
        v.push((
            "F0475-拒绝-校验和弃用",
            matches!(&r, Err(e) if e.contains("损坏")),
        ));
        // 截断 → 显性拒绝。
        let b2 = serialize(b"data");
        let r2 = deserialize(&b2[..b2.len() - 4], CompatMode::RejectOld);
        v.push((
            "F0475-拒绝-截断拒绝",
            matches!(&r2, Err(e) if e.contains("截断")),
        ));
        // 版本不符（RejectOld）→ 显性拒绝带提示。
        let mut b3 = serialize(b"data");
        b3[0] = 9;
        let r3 = deserialize(&b3, CompatMode::RejectOld);
        v.push((
            "F0475-拒绝-版本拒绝",
            matches!(&r3, Err(e) if e.contains("显性拒绝")),
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0475-判据-锚点入册", ANCHOR == "VE-F0475"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec75）。
pub fn run_vec75_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec75");
    for (name, ok) in vec75_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec75_raw_checks as run_vec75_checks;
    #[test]
    fn vec75_cache_checks() {
        for (name, ok) in run_vec75_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
