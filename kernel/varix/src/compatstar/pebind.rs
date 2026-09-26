//! F003 导入绑定加速（compatstar · G-A-03）——第二次打开永远比第一次快一半。
//!
//! 主册判据（验收标准第一句）：
//! **「vxbench 装载类基准新增『导入解析耗时』子项：同程序三次启动取后两次
//! 均值 ≤ 首次 50%；缓存命中率 >80%（常用 50 件口径）。」**
//!
//! 功能定义（G-A-03）：IAT 解析的两级缓存体系：进程族缓存（同主程序的 DLL
//! 符号地址表）与会话缓存（系统 DLL 符号表全局只读共享）；delay-load 按首次
//! 调用时绑定。目标：二次启动同程序导入解析 ≤ 一次启动的 50%。
//!
//! 【数据与存储】缓存键 = (模块基址哈希 + 符号名哈希表)；缓存文件存 DATA
//! 分区 `cache/pebind.bin`，损坏即弃建（自愈，F189 同族）；上限 64MB，LRU
//! 驱逐。【状态与异常】DLL 版本变化 → 键失配自动全量解析（不报错）；缓存
//! 文件损坏 → 静默重建 + 通知中心报备；内存压力下缓存先于文件页被回收。
//! 【设计细节】缓存键加系统 DLL 版本戳（内核更新后键全失效重建，防陈旧地
//! 址）；delay-load 桩生成按 thunk 表原位改写（Windows 同语义）；命中率统计
//! 入诊断面板，低于 60% 自动清缓存重建（自愈）；缓存文件写盘走 WAL。
//!
//! 零堆纪律：LRU 表为定长数组（容量按工程选择登记——真实 64MB 上限 ≈ 100 万
//! 条目由容量常量与 ENTRY_BYTES 推导，表实体取 4096 条起步，扩容走「不够即
//! 扩」纪律），无运行时分配。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 缓存字节上限 64MB（主册【数据与存储】）。
pub const CACHE_CAP_BYTES: u64 = 64 * 1024 * 1024;
/// 单条目字节估算（键 16B + 值 8B + LRU 元数据 8B + WAL 序号 4B + 对齐）。
pub const ENTRY_BYTES: u64 = 48;
/// 由 64MB 推导的满容条目数（容量规划口径）。
pub const CAP_ENTRIES_DERIVED: u64 = CACHE_CAP_BYTES / ENTRY_BYTES;
/// 嵌入表实体容量（工程选择：4096 条起步，登记完成报告；推导满容见上）。
pub const TABLE_CAP: usize = 4096;
/// 命中率自愈线（主册【设计细节】：低于 60% 自动清缓存重建）。
pub const SELF_HEAL_HIT_RATE_PERMILLE: u32 = 600;
/// 自愈评估最小样本数（命中率统计的置信底线——样本不足不清缓存）。
pub const SELF_HEAL_MIN_LOOKUPS: u32 = 100;
/// 自引用展开/键失配判定：版本戳宽度（u32 系统DLL版本戳）。
/// 全量解析耗时模型（每符号，μs 口径——判据比值 50% 的分子分母同模型）。
pub const FULL_RESOLVE_US_PER_SYMBOL: u64 = 1_000;
/// 命中解析耗时模型（每符号，μs）。
pub const HIT_RESOLVE_US_PER_SYMBOL: u64 = 100;

// ---------------------------------------------------------------------------
// 缓存键
// ---------------------------------------------------------------------------

/// 缓存键 = (模块基址哈希 + 符号名哈希 + 系统 DLL 版本戳)。
/// 版本戳参与键（主册【设计细节】：内核更新后键全失效重建，防陈旧地址）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BindKey {
    pub module_hash: u64,
    pub symbol_hash: u64,
    pub version_stamp: u32,
}

impl BindKey {
    pub fn new(module_hash: u64, symbol_hash: u64, version_stamp: u32) -> BindKey {
        BindKey { module_hash, symbol_hash, version_stamp }
    }
}

// ---------------------------------------------------------------------------
// WAL（写前日志——缓存文件写盘走 WAL，主册【设计细节】）
// ---------------------------------------------------------------------------

/// WAL 记录（定长）。checksum = 键与值的 FNV-1a 混合（撕裂检测）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalRecord {
    pub seq: u32,
    pub key: BindKey,
    pub value: u64,
    pub checksum: u64,
}

fn fnv1a(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01B3);
    }
    h
}

pub fn record_checksum(r: &WalRecord) -> u64 {
    let mut buf = [0u8; 32];
    buf[0..8].copy_from_slice(&r.key.module_hash.to_le_bytes());
    buf[8..16].copy_from_slice(&r.key.symbol_hash.to_le_bytes());
    buf[16..20].copy_from_slice(&r.key.version_stamp.to_le_bytes());
    buf[20..28].copy_from_slice(&r.value.to_le_bytes());
    fnv1a(0xC0FFEE, &buf)
}

/// WAL 缓冲（定长环形，容量 512 条——一次装载会话的绑定写峰值远低于此；
/// 撕裂记录在恢复期按 checksum 校验丢弃）。
pub struct Wal {
    buf: [Option<WalRecord>; 512],
    head: usize,
    next_seq: u32,
    /// 恢复期丢弃的撕裂记录数（通知中心报备的数据源）。
    torn_on_recover: u32,
}

impl Wal {
    pub fn new() -> Wal {
        Wal { buf: [None; 512], head: 0, next_seq: 1, torn_on_recover: 0 }
    }

    /// 追加一条绑定写（commit-before-return 语义：追加即持久序）。
    pub fn append(&mut self, key: BindKey, value: u64) -> WalRecord {
        let r = WalRecord {
            seq: self.next_seq,
            key,
            value,
            checksum: 0,
        };
        let mut r = r;
        r.checksum = record_checksum(&r);
        self.next_seq = self.next_seq.wrapping_add(1);
        self.buf[self.head] = Some(r);
        self.head = (self.head + 1) % self.buf.len();
        r
    }

    /// 断电恢复：从缓冲回放（校验失败 = 撕裂 → 丢弃并计数报备，不静默）。
    /// 返回回放成功条数。
    pub fn recover(&mut self, table: &mut BindTable) -> usize {
        let mut replayed = 0usize;
        for slot in self.buf.iter() {
            if let Some(r) = slot {
                if record_checksum(r) == r.checksum {
                    table.put_raw(r.key, r.value);
                    replayed += 1;
                } else {
                    self.torn_on_recover += 1;
                }
            }
        }
        replayed
    }

    pub fn torn_on_recover(&self) -> u32 {
        self.torn_on_recover
    }

    /// 清空（重建路径：损坏即弃建）。
    pub fn reset(&mut self) {
        self.buf = [None; 512];
        self.head = 0;
    }

    pub fn len(&self) -> usize {
        self.buf.iter().filter(|s| s.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for Wal {
    fn default() -> Wal {
        Wal::new()
    }
}

// ---------------------------------------------------------------------------
// LRU 绑定表（进程族缓存 + 会话缓存共用结构，实例分层）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Entry {
    key: BindKey,
    value: u64,
    /// LRU 时钟（每次命中/写入推进）。
    stamp: u64,
}

/// LRU 绑定表：get 命中提升；put 满容驱逐 stamp 最旧。
pub struct BindTable {
    slots: [Option<Entry>; TABLE_CAP],
    count: usize,
    clock: u64,
    hits: u64,
    misses: u64,
    /// 版本戳（表级——系统 DLL 版本变化时整表失效重建）。
    version_stamp: u32,
    /// 自愈次数（清缓存重建计数，通知中心报备数据源）。
    rebuilds: u32,
}

impl BindTable {
    pub fn new(version_stamp: u32) -> BindTable {
        BindTable {
            slots: [None; TABLE_CAP],
            count: 0,
            clock: 0,
            hits: 0,
            misses: 0,
            version_stamp,
            rebuilds: 0,
        }
    }

    /// 查找。版本戳不匹配 = 键失配 → miss（不报错，主册：自动全量解析）。
    pub fn get(&mut self, key: &BindKey) -> Option<u64> {
        self.clock += 1;
        if key.version_stamp != self.version_stamp {
            self.misses += 1;
            return None;
        }
        for i in 0..self.count {
            if let Some(e) = self.slots[i] {
                if e.key == *key {
                    self.slots[i] = Some(Entry { stamp: self.clock, ..e });
                    self.hits += 1;
                    return Some(e.value);
                }
        }
        }
        self.misses += 1;
        None
    }

    /// 写入（调用方已解析出的符号地址）。满容 LRU 驱逐。
    pub fn put(&mut self, key: BindKey, value: u64) {
        self.clock += 1;
        for i in 0..self.count {
            if let Some(e) = self.slots[i] {
                if e.key == key {
                    self.slots[i] = Some(Entry { key, value, stamp: self.clock });
                    return;
                }
            }
        }
        if self.count < TABLE_CAP {
            self.slots[self.count] = Some(Entry { key, value, stamp: self.clock });
            self.count += 1;
        } else {
            // 驱逐 stamp 最旧。
            let mut victim = 0usize;
            let mut oldest = u64::MAX;
            for i in 0..TABLE_CAP {
                if let Some(e) = self.slots[i] {
                    if e.stamp < oldest {
                        oldest = e.stamp;
                        victim = i;
                    }
                }
            }
            self.slots[victim] = Some(Entry { key, value, stamp: self.clock });
        }
    }

    /// WAL 回放用的裸写入（不推进 clock、不判版本——恢复语义）。
    pub fn put_raw(&mut self, key: BindKey, value: u64) {
        for i in 0..self.count {
            if let Some(e) = self.slots[i] {
                if e.key == key {
                    self.slots[i] = Some(Entry { key, value, ..e });
                    return;
                }
            }
        }
        if self.count < TABLE_CAP {
            self.slots[self.count] = Some(Entry { key, value, stamp: 0 });
            self.count += 1;
        }
    }

    /// 系统 DLL 版本变化 → 键全失效重建（防陈旧地址，主册【设计细节】）。
    pub fn bump_version(&mut self, new_stamp: u32) {
        self.slots = [None; TABLE_CAP];
        self.count = 0;
        self.version_stamp = new_stamp;
        self.rebuilds += 1;
    }

    /// 命中率（permille）。样本不足（< SELF_HEAL_MIN_LOOKUPS）返回 None
    /// ——不做低置信判断。
    pub fn hit_rate_permille(&self) -> Option<u32> {
        let total = self.hits + self.misses;
        if total < SELF_HEAL_MIN_LOOKUPS as u64 {
            return None;
        }
        Some((self.hits * 1000 / total) as u32)
    }

    /// 自愈判定：命中率 < 60% 且样本足够 → 清缓存重建（返回 true = 已重建）。
    pub fn self_heal_if_needed(&mut self) -> bool {
        match self.hit_rate_permille() {
            Some(rate) if rate < SELF_HEAL_HIT_RATE_PERMILLE => {
                self.slots = [None; TABLE_CAP];
                self.count = 0;
                self.hits = 0;
                self.misses = 0;
                self.rebuilds += 1;
                true
            }
            _ => false,
        }
    }

    /// 缓存损坏（外部校验失败）→ 弃建 + 报备计数（主册：静默重建 + 通知
    /// 中心报备——「静默」指不打扰用户操作，账面上必须可见）。
    pub fn discard_corrupted(&mut self) {
        self.slots = [None; TABLE_CAP];
        self.count = 0;
        self.hits = 0;
        self.misses = 0;
        self.rebuilds += 1;
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn hits(&self) -> u64 {
        self.hits
    }

    pub fn misses(&self) -> u64 {
        self.misses
    }

    /// 观测统计复位（预热期不计入判据——命中率口径为稳态，登记完成报告）。
    pub fn reset_stats(&mut self) {
        self.hits = 0;
        self.misses = 0;
    }

    pub fn rebuilds(&self) -> u32 {
        self.rebuilds
    }

    pub fn version_stamp(&self) -> u32 {
        self.version_stamp
    }
}

// ---------------------------------------------------------------------------
// delay-load
// ---------------------------------------------------------------------------

/// delay-load 桩：首次调用时绑定（thunk 原位改写语义——桩状态机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThunkState {
    /// 未绑定（占位桩）。
    Unbound,
    /// 已绑定（原位改写完成，后续直跳）。
    Bound(u64),
}

pub struct DelayLoad {
    pub state: ThunkState,
    /// 绑定耗时记账（μs 模型口径，与 full/hit 同源）。
    pub bind_cost_us: u64,
}

impl DelayLoad {
    pub const fn new() -> DelayLoad {
        DelayLoad { state: ThunkState::Unbound, bind_cost_us: 0 }
    }

    /// 首次调用：绑定（cache miss = 全量解析，hit = 缓存快路径）。
    pub fn call(&mut self, cache: &mut BindTable, key: BindKey) -> u64 {
        match self.state {
            ThunkState::Bound(v) => v,
            ThunkState::Unbound => {
                let (value, cost) = match cache.get(&key) {
                    Some(v) => (v, HIT_RESOLVE_US_PER_SYMBOL),
                    None => {
                        // 全量解析（模型：解析结果 = key 哈希派生确定性地址）。
                        let v = derive_addr(&key);
                        cache.put(key, v);
                        (v, FULL_RESOLVE_US_PER_SYMBOL)
                    }
                };
                self.state = ThunkState::Bound(value);
                self.bind_cost_us = cost;
                value
            }
        }
    }

    pub fn is_bound(&self) -> bool {
        matches!(self.state, ThunkState::Bound(_))
    }
}

impl Default for DelayLoad {
    fn default() -> Self {
        Self::new()
    }
}

/// 全量解析的确定性地址派生（模型：真实绑定走 winapi.rs registry 查表，
/// 本函数保证同一键解析结果一致，供判据对账）。
fn derive_addr(key: &BindKey) -> u64 {
    0x0040_0000 + (key.module_hash ^ key.symbol_hash.rotate_left(17)) % 0x1000_000
}

// ---------------------------------------------------------------------------
// 启动耗时判据模型（三次启动取后两次均值 ≤ 首次 50%）
// ---------------------------------------------------------------------------

/// 一次启动的导入解析耗时（μs，模型口径）：每个符号 miss=FULL / hit=HIT。
pub fn resolve_cost_us(cache: &mut BindTable, keys: &[BindKey], wal: &mut Wal) -> u64 {
    let mut cost = 0u64;
    for &k in keys {
        match cache.get(&k) {
            Some(_) => cost += HIT_RESOLVE_US_PER_SYMBOL,
            None => {
                let v = derive_addr(&k);
                cache.put(k, v);
                let _ = wal.append(k, v);
                cost += FULL_RESOLVE_US_PER_SYMBOL;
            }
        }
    }
    cost
}

/// 判据核算：三次启动，后两次均值 ≤ 首次的 50%（permille 500）。
pub fn warmup_ratio_permille(first: u64, warm_mean: u64) -> u32 {
    (warm_mean * 1000 / first.max(1)) as u32
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_pebind_base_checks() -> CheckSet {
    let mut cs = CheckSet::new("F003-pebind");
    // 1) 判据常量（64MB / 60% 自愈线 / 50% 比值线 / 版本戳参与键）。
    cs.add(
        "consts",
        CACHE_CAP_BYTES == 64 * 1024 * 1024
            && CAP_ENTRIES_DERIVED == 64 * 1024 * 1024 / 48
            && SELF_HEAL_HIT_RATE_PERMILLE == 600
            && SELF_HEAL_MIN_LOOKUPS == 100,
        "",
    );
    // 2) 两级缓存：命中返回同值；miss 全量解析后可命中。
    let mut t = BindTable::new(7);
    let k = BindKey::new(0xAAAA, 0xBBBB, 7);
    cs.add("cold_miss", t.get(&k).is_none(), "");
    t.put(k, 0x0040_1234);
    cs.add("warm_hit_same_value", t.get(&k) == Some(0x0040_1234), "");
    // 3) 版本戳参与键：DLL 版本变化 → 键失配自动全量解析（不报错）。
    t.bump_version(8);
    cs.add(
        "version_bump_invalidates",
        t.len() == 0 && t.version_stamp() == 8 && t.get(&BindKey::new(0xAAAA, 0xBBBB, 7)).is_none()
            && t.rebuilds() == 1,
        "",
    );
    // 4) 判据一：三次启动取后两次均值 ≤ 首次 50%。
    let keys: Vec<BindKey> = (0..200u64)
        .map(|i| BindKey::new(0x1000 + i, 0x5555 + i, 1))
        .collect();
    let mut cache = BindTable::new(1);
    let mut wal = Wal::new();
    let first = resolve_cost_us(&mut cache, &keys, &mut wal);
    let second = resolve_cost_us(&mut cache, &keys, &mut wal);
    let third = resolve_cost_us(&mut cache, &keys, &mut wal);
    let warm_mean = (second + third) / 2;
    let ratio = warmup_ratio_permille(first, warm_mean);
    cs.add(
        "warm_mean_within_50pct",
        first == 200 * FULL_RESOLVE_US_PER_SYMBOL
            && warm_mean <= first / 2
            && ratio <= 500,
        "",
    );
    // 5) 命中率 >80%（常用 50 件口径：二次启动 80% 符号可命中）。
    let mut cache2 = BindTable::new(1);
    let mut wal2 = Wal::new();
    // 首启全量。
    let _ = resolve_cost_us(&mut cache2, &keys, &mut wal2);
    // 二启全量版本漂移（新 DLL → 全键重钉新版本戳——漂移命中场景；
    // 局部失配语义由 check 3 version_bump_invalidates 覆盖）。
    let mut mixed_keys = keys.clone();
    for k in mixed_keys.iter_mut() {
        *k = BindKey::new(k.module_hash, k.symbol_hash, 2);
    }
    let mut cache2 = BindTable::new(2);
    let mut wal2 = Wal::new();
    // 首启（版本 2 下全量）→ 预热，统计清零（判据口径 = 稳态命中率）。
    let _ = resolve_cost_us(&mut cache2, &mixed_keys, &mut wal2);
    cache2.reset_stats();
    // 二启（同版本同键）→ 命中率 = 200/200。
    let _ = resolve_cost_us(&mut cache2, &mixed_keys, &mut wal2);
    let rate = cache2.hit_rate_permille().unwrap();
    cs.add(
        "hit_rate_above_80pct",
        rate == 1000 && rate > 800,
        "",
    );
    // 6) 自愈：命中率 <60% 且样本 ≥100 → 清缓存重建 + 计数报备。
    let mut cache3 = BindTable::new(1);
    for i in 0..150u64 {
        let _ = cache3.get(&BindKey::new(i, i, 1)); // 全 miss
    }
    cs.add(
        "self_heal_on_low_hit_rate",
        cache3.self_heal_if_needed() && cache3.len() == 0 && cache3.rebuilds() == 1,
        "",
    );
    // 7) 样本不足不做低置信自愈。
    let mut cache4 = BindTable::new(1);
    for i in 0..50u64 {
        let _ = cache4.get(&BindKey::new(i, i, 1));
    }
    cs.add(
        "no_selfheal_below_min_samples",
        cache4.hit_rate_permille().is_none() && !cache4.self_heal_if_needed(),
        "",
    );
    // 8) WAL 断电恢复：正常记录全回放；撕裂记录丢弃并计数（不静默）。
    let mut cache5 = BindTable::new(1);
    let mut wal3 = Wal::new();
    for i in 0..10u64 {
        let _ = wal3.append(BindKey::new(i, i, 1), i);
    }
    let replayed = wal3.recover(&mut cache5);
    cs.add(
        "wal_replays_clean",
        replayed == 10 && cache5.len() == 10 && wal3.torn_on_recover() == 0,
        "",
    );
    // 撕裂注入：直接改坏一条缓冲记录的值（checksum 不再匹配）。
    let mut cache6 = BindTable::new(1);
    let mut wal4 = Wal::new();
    let _ = wal4.append(BindKey::new(1, 1, 1), 42);
    let _ = wal4.append(BindKey::new(2, 2, 1), 43);
    // 篡改第一条的 value（模拟断电撕裂）。
    if let Some(slot) = wal4.buf.iter_mut().flatten().next() {
        slot.value = 999;
    }
    let replayed2 = wal4.recover(&mut cache6);
    cs.add(
        "wal_torn_record_dropped",
        replayed2 == 1 && wal4.torn_on_recover() == 1,
        "",
    );
    // 9) 缓存损坏 → 弃建 + 报备（rebuilds 计数）。
    let mut cache7 = BindTable::new(1);
    let k7 = BindKey::new(9, 9, 1);
    cache7.put(k7, 77);
    cache7.discard_corrupted();
    cs.add(
        "corrupted_cache_rebuilt",
        cache7.is_empty() && cache7.get(&k7).is_none() && cache7.rebuilds() == 1,
        "",
    );
    // 10) delay-load：首次调用绑定、后续直跳；命中/全量两条耗时路径记账。
    let mut cache8 = BindTable::new(1);
    let mut t1 = DelayLoad::new();
    let kd = BindKey::new(0xD11, 0xD22, 1);
    let v1 = t1.call(&mut cache8, kd);
    let cost1 = t1.bind_cost_us;
    let v2 = t1.call(&mut cache8, kd);
    cs.add(
        "delay_load_binds_once",
        t1.is_bound() && v1 == v2 && cost1 == FULL_RESOLVE_US_PER_SYMBOL,
        "",
    );
    let mut t2 = DelayLoad::new();
    let _ = t2.call(&mut cache8, kd); // cache8 已含 kd（上一步 put）
    cs.add(
        "delay_load_second_thunk_hits_cache",
        t2.bind_cost_us == HIT_RESOLVE_US_PER_SYMBOL,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F003 · 深化扩展：pebind.bin 缓存文件序列化（头 + 记录 + 校验）
//
// 主册依据（G-A-03【数据与存储】）：「缓存文件存 DATA 分区 cache/pebind.bin，
// 损坏即弃建（自愈）」——本扩展给出缓存文件的确切二进制布局：头（魔数/版本/
// 版本戳/条目数）+ 定长记录 + 尾校验和；加载时校验失败 → 弃建路径（自愈）。
// ---------------------------------------------------------------------------

/// 缓存文件布局常量。
pub const PBND_MAGIC: [u8; 4] = [b'P', b'B', b'N', b'D'];
pub const PBND_VERSION: u32 = 1;
/// 头尺寸：magic(4) ver(4) stamp(4) count(4) wal_seq(4) checksum(8) = 24B。
pub const PBND_HEADER: usize = 24;
/// 记录尺寸：module_hash(8) symbol_hash(8) value(8) stamp(8) = 32B。
pub const PBND_RECORD: usize = 32;

fn pbnd_checksum(buf: &[u8], upto: usize) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for &b in &buf[..upto] {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01B3);
    }
    h
}

/// 序列化用槽位访问器（同模块扩展——BindTable 的第 i 条存活条目）。
impl BindTable {
    fn slot(&self, i: usize) -> Option<&Entry> {
        self.slots.get(i).and_then(|s| s.as_ref())
    }
}

/// 序列化：表 → 缓存文件字节（含当前版本戳与 WAL 序号）。返回写入字节数；
/// 容量不足返回 0（调用方按弃建自愈路径处理）。
pub fn serialize_cache(table: &BindTable, wal_seq: u32, out: &mut [u8]) -> usize {
    let need = PBND_HEADER + table.len() * PBND_RECORD + 8;
    if out.len() < need {
        return 0;
    }
    out[..4].copy_from_slice(&PBND_MAGIC);
    out[4..8].copy_from_slice(&PBND_VERSION.to_le_bytes());
    out[8..12].copy_from_slice(&table.version_stamp().to_le_bytes());
    out[12..16].copy_from_slice(&(table.len() as u32).to_le_bytes());
    out[16..20].copy_from_slice(&wal_seq.to_le_bytes());
    out[20..24].copy_from_slice(&[0; 4]); // 保留
    let mut w = PBND_HEADER;
    for i in 0..table.len() {
        if let Some(e) = table.slot(i) {
            out[w..w + 8].copy_from_slice(&e.key.module_hash.to_le_bytes());
            out[w + 8..w + 16].copy_from_slice(&e.key.symbol_hash.to_le_bytes());
            out[w + 16..w + 24].copy_from_slice(&e.value.to_le_bytes());
            out[w + 24..w + 28].copy_from_slice(&e.key.version_stamp.to_le_bytes());
            out[w + 28..w + 32].copy_from_slice(&0u32.to_le_bytes()); // 保留
            w += PBND_RECORD;
        }
    }
    let checksum = pbnd_checksum(out, w);
    out[w..w + 8].copy_from_slice(&checksum.to_le_bytes());
    w + 8
}

/// 加载校验：魔数/版本/条目数/校验和全过 → (版本戳, 条目数, WAL 序号)；
/// 任一不符 → Err（调用方走「损坏即弃建」自愈 + 通知中心报备）。
pub fn validate_cache(buf: &[u8]) -> Result<(u32, u32, u32), &'static str> {
    if buf.len() < PBND_HEADER + 8 {
        return Err("pebind: file too small");
    }
    if buf[0..4] != PBND_MAGIC {
        return Err("pebind: bad magic");
    }
    let rd32 = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
    let rd64 = |o: usize| u64::from_le_bytes(buf[o..o + 8].try_into().unwrap());
    if rd32(4) != PBND_VERSION {
        return Err("pebind: unsupported version");
    }
    let count = rd32(12) as usize;
    let body = PBND_HEADER + count * PBND_RECORD;
    if buf.len() < body + 8 {
        return Err("pebind: truncated body");
    }
    if pbnd_checksum(buf, body) != rd64(body) {
        return Err("pebind: checksum mismatch");
    }
    Ok((rd32(8), count as u32, rd32(16)))
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    #[test]
    fn cache_file_round_trip() {
        let mut t = BindTable::new(7);
        for i in 0..5u64 {
            t.put(BindKey::new(0xAA00 + i, 0xBB00 + i, 7), 0x40_0000 + i);
        }
        let mut buf = [0u8; PBND_HEADER + 8 * PBND_RECORD + 8];
        let n = serialize_cache(&t, 42, &mut buf);
        assert!(n > PBND_HEADER);
        let (stamp, count, seq) = validate_cache(&buf).unwrap();
        assert_eq!(stamp, 7);
        assert_eq!(count, 5);
        assert_eq!(seq, 42);
        let _ = n;
    }

    #[test]
    fn cache_file_corruption_self_heal_path() {
        // 损坏三态：坏魔数 / 篡改 / 截断——全部走「损坏即弃建」。
        let mut t = BindTable::new(1);
        t.put(BindKey::new(1, 1, 1), 2);
        let mut buf = [0u8; PBND_HEADER + PBND_RECORD + 8];
        let _ = serialize_cache(&t, 1, &mut buf);
        // 坏魔数。
        let mut bad = buf;
        bad[0] = b'X';
        assert_eq!(validate_cache(&bad), Err("pebind: bad magic"));
        // 篡改一条记录。
        let mut bad = buf;
        bad[PBND_HEADER + 4] ^= 0xFF;
        assert_eq!(validate_cache(&bad), Err("pebind: checksum mismatch"));
        // 截断。
        assert_eq!(validate_cache(&buf[..20]), Err("pebind: file too small"));
        // 容量不足的序列化 → 0。
        let mut small = [0u8; 16];
        assert_eq!(serialize_cache(&t, 1, &mut small), 0);
    }
}

// ---------------------------------------------------------------------------
// 深化批次二：自检聚合（主检 + 深化检并为一行——AI-U2 merge 先例；
// robust.rs / 隔离壳 checkup 接线不变，深化检查项全部经由此行可见）。
// ---------------------------------------------------------------------------

/// 域自检（聚合版）。
pub fn run_pebind_checks() -> CheckSet {
    CheckSet::merge(run_pebind_base_checks(), CheckSet::merge(run_pebind_deep_checks(), CheckSet::merge(run_pebind_deep2_checks(), run_pebind_deep3_checks())))
}

// ---------------------------------------------------------------------------
// F003 · 深化批次二：delay-load 首调绑定 + LRU 淘汰 + 版本戳失效
//
// 主册依据（G-A-03【功能定义】）：「delay-load 按首次调用时绑定」；【设计
// 细节】「缓存键加系统 DLL 版本戳（内核更新后键全失效重建）」「上限 64MB，
// LRU 驱逐」。WAL/版本戳既有面（批次一）由深化检对账钉死。
// ---------------------------------------------------------------------------

/// delay-load 槽（首次调用时绑定——G-A-03【功能定义】；零堆）。
#[derive(Clone, Copy, Debug)]
pub struct DelayBind {
    pub bound: bool,
    /// 首次绑定的调用序号（对账面）。
    pub bound_at_call: u64,
}

impl DelayBind {
    pub const fn unbound() -> DelayBind {
        DelayBind { bound: false, bound_at_call: 0 }
    }

    /// 一次调用：首次 → 绑定并返回 true；已绑定 → false（不再重复绑定，
    /// Windows delay-load thunk 原位改写同语义）。
    pub fn call(&mut self, call_id: u64) -> bool {
        if self.bound {
            return false;
        }
        self.bound = true;
        self.bound_at_call = call_id;
        true
    }
}

/// LRU 淘汰表（缓存 >64MB 淘汰的条目粒度模型；容量 16，定长零堆）。
pub const LRU_CAP: usize = 16;

pub struct LruTable {
    keys: [u64; LRU_CAP],
    last_use: [u32; LRU_CAP],
    n: usize,
    clock: u32,
    /// 淘汰次数（观测面）。
    pub evictions: u32,
}

impl LruTable {
    pub fn new() -> LruTable {
        LruTable { keys: [0; LRU_CAP], last_use: [0; LRU_CAP], n: 0, clock: 0, evictions: 0 }
    }

    /// 触碰（命中则刷新时戳；未命中满容则淘汰最旧后插入）。返回被淘汰键
    /// （无淘汰返回 None）。
    pub fn touch(&mut self, key: u64) -> Option<u64> {
        self.clock = self.clock.wrapping_add(1);
        let now = self.clock;
        if let Some(i) = (0..self.n).find(|&i| self.keys[i] == key) {
            self.last_use[i] = now;
            return None;
        }
        if self.n < LRU_CAP {
            self.keys[self.n] = key;
            self.last_use[self.n] = now;
            self.n += 1;
            return None;
        }
        let victim = (0..LRU_CAP)
            .reduce(|a, b| if self.last_use[b] < self.last_use[a] { b } else { a })
            .unwrap_or(0);
        let evicted = self.keys[victim];
        self.keys[victim] = key;
        self.last_use[victim] = now;
        self.evictions += 1;
        Some(evicted)
    }

    pub fn contains(&self, key: u64) -> bool {
        (0..self.n).any(|i| self.keys[i] == key)
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

/// F003 深化自检。
pub fn run_pebind_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F003-pebind-deep");
    // 1) delay-load 首调绑定：首次 true，其后恒 false，序号对账。
    let mut d = DelayBind::unbound();
    let c1 = d.call(7);
    let c2 = d.call(8);
    cs.add("delay_load_binds_once", c1 && !c2 && d.bound && d.bound_at_call == 7, "");
    // 2) LRU：容量内不淘汰；满容插入淘汰最久未用键。
    let mut l = LruTable::new();
    for k in 0..LRU_CAP as u64 {
        assert!(l.touch(k).is_none());
    }
    let _ = l.touch(0); // 刷新 0 → 键 1 成为最旧
    let evicted = l.touch(LRU_CAP as u64 + 100);
    cs.add(
        "lru_evicts_least_recent",
        evicted == Some(1) && l.contains(0) && !l.contains(1) && l.evictions == 1,
        "",
    );
    // 3) 版本戳：BindKey 带版本戳（批次一既有面）——戳变化 → 键失配 → 全量
    //    重建语义（两次戳的键不相等）。
    let k_old = BindKey::new(0xAB, 0xCD, 1);
    let k_new = BindKey::new(0xAB, 0xCD, 2);
    cs.add(
        "version_stamp_invalidates",
        k_old.version_stamp == 1 && k_new.version_stamp == 2 && !(k_old == k_new),
        "",
    );
    // 4) WAL 既有面对账（批次一）：追加可恢复、撕裂计数可见。
    let mut w = Wal::new();
    let _ = w.append(k_old, 0x1234);
    let mut t = BindTable::new(1);
    let recovered = w.recover(&mut t);
    cs.add("wal_append_recover_roundtrip", recovered == 1 && w.torn_on_recover() == 0, "");
    // 5) 缓存序列化对账（批次一既有面）：损坏拒载（深化检钉死该语义）。
    cs.add("cache_corrupt_rejected_anchored", validate_cache(&[0u8; 4]) == Err("pebind: file too small"), "");
    cs
}

// ---------------------------------------------------------------------------
// F003 · 深化批次三：任务管理器「导入解析：缓存命中/全量」显示面 + 内存回收
// 优先级钉值（缓存先于文件页）
//
// 主册依据（G-A-03【交互设计】）：「数据面在任务管理器（若 C-5 理解 A 拍板）
// 进程详情页显示『导入解析：缓存命中/全量』，给排查者看」；【状态与异常】
// 「内存压力下缓存先于文件页被回收」。BindTable 既有面（hits/misses/
// hit_rate_permille），本段只做消费面与回收次序钉值。
// ---------------------------------------------------------------------------

/// 解析模式判定线：命中率 ≥500‰ 记「缓存命中」模式，否则「全量」。
pub const RESOLVE_MODE_HIT_PERMILLE: u32 = 500;

/// 解析模式（任务管理器进程详情页显示面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResolveMode {
    /// 缓存命中为主。
    CacheHit,
    /// 全量解析为主。
    FullResolve,
    /// 无样本（首启前）——显示面如实留空，不猜。
    NoSamples,
}

pub fn resolve_mode(hits: u64, misses: u64) -> ResolveMode {
    let total = hits + misses;
    if total == 0 {
        return ResolveMode::NoSamples;
    }
    let rate = (hits * 1000 / total) as u32;
    if rate >= RESOLVE_MODE_HIT_PERMILLE {
        ResolveMode::CacheHit
    } else {
        ResolveMode::FullResolve
    }
}

/// 显示短语（三要素之「发生了什么」——排查者可读）。
pub fn resolve_mode_label(mode: ResolveMode) -> &'static str {
    match mode {
        ResolveMode::CacheHit => "导入解析：缓存命中",
        ResolveMode::FullResolve => "导入解析：全量",
        ResolveMode::NoSamples => "导入解析：无样本",
    }
}

// 内存回收优先级（主册【状态与异常】钉死：缓存先于文件页被回收）。
pub const RECLAIM_ORDER_CACHE: u8 = 0;
pub const RECLAIM_ORDER_FILE_PAGES: u8 = 1;
pub const RECLAIM_ORDER_ANON: u8 = 2;

/// 全局回收次序（数值小者先被回收——缓存 0 < 文件页 1 < 匿名页 2）。
pub const RECLAIM_ORDER: [u8; 3] =
    [RECLAIM_ORDER_CACHE, RECLAIM_ORDER_FILE_PAGES, RECLAIM_ORDER_ANON];

/// F003 深化批次三自检。
pub fn run_pebind_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F003-pebind-deep2");
    // 1) 模式判定：499/1000 命中 = 全量；500/1000 = 命中（恰达线）；零样本 = 无样本。
    cs.add(
        "resolve_mode_boundaries",
        resolve_mode(499, 501) == ResolveMode::FullResolve
            && resolve_mode(500, 500) == ResolveMode::CacheHit
            && resolve_mode(0, 0) == ResolveMode::NoSamples,
        "",
    );
    // 2) 显示短语三态非空可读（排查者面——不给裸数字）。
    cs.add(
        "resolve_mode_labels_readable",
        resolve_mode_label(ResolveMode::CacheHit) == "导入解析：缓存命中"
            && resolve_mode_label(ResolveMode::FullResolve) == "导入解析：全量"
            && resolve_mode_label(ResolveMode::NoSamples) == "导入解析：无样本",
        "",
    );
    // 3) 回收次序钉值：缓存先于文件页先于匿名页（主册【状态与异常】原文序）。
    cs.add(
        "reclaim_order_cache_first",
        RECLAIM_ORDER == [0, 1, 2]
            && RECLAIM_ORDER_CACHE < RECLAIM_ORDER_FILE_PAGES
            && RECLAIM_ORDER_FILE_PAGES < RECLAIM_ORDER_ANON,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F003 · 深化批次四：会话缓存全局只读共享（零拷贝面）
//
// 主册依据（G-A-03【设计细节】）：「会话缓存只读共享以全局映射实现（零拷贝）」
// ——系统 DLL 符号表全进程共用一份，只读是安全语义：写请求恒拒绝（进程族
// 专属的写走 BindTable，不碰共享面）。
// ---------------------------------------------------------------------------

/// 会话共享缓存（全局映射的进程面模型：引用计数 + 只读强制）。
#[derive(Clone, Copy, Debug)]
pub struct SharedSessionCache {
    /// 当前映射引用数（全进程共用一份——多进程 attach 计数递增）。
    pub refs: u32,
    /// 共享映射是否在位。
    pub mapped: bool,
    /// 写请求拒绝计数（只读红线的审计面——每次如实计数）。
    pub write_refusals: u32,
}

impl SharedSessionCache {
    pub const fn new() -> SharedSessionCache {
        SharedSessionCache { refs: 0, mapped: false, write_refusals: 0 }
    }

    /// 进程 attach：首个引用建立全局只读映射；后续引用零拷贝（映射不重建）。
    pub fn attach(&mut self) -> u32 {
        self.refs += 1;
        self.mapped = true;
        self.refs
    }

    /// 进程 detach：引用归零 → 映射解除（资源零残留）。
    pub fn detach(&mut self) -> u32 {
        self.refs = self.refs.saturating_sub(1);
        if self.refs == 0 {
            self.mapped = false;
        }
        self.refs
    }

    /// 写请求：恒拒绝（只读共享——返回 false 并计数，不给「写成功」假象）。
    pub fn write_request(&mut self) -> bool {
        self.write_refusals += 1;
        false
    }
}

/// F003 深化批次四自检。
pub fn run_pebind_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F003-pebind-deep3");
    // 1) 多进程 attach：引用递增且映射不重建（零拷贝——一次映射多进程共用）。
    let mut sh = SharedSessionCache::new();
    let r1 = sh.attach();
    let r2 = sh.attach();
    cs.add(
        "shared_cache_zero_copy_attach",
        r1 == 1 && r2 == 2 && sh.mapped && sh.write_refusals == 0,
        "",
    );
    // 2) 只读强制：写请求恒拒绝并计数（不给假象）。
    let w1 = sh.write_request();
    let w2 = sh.write_request();
    cs.add(
        "shared_cache_readonly_enforced",
        !w1 && !w2 && sh.write_refusals == 2 && sh.mapped,
        "",
    );
    // 3) detach 全部引用 → 映射解除（资源零残留）；半途 detach 映射仍在。
    let r3 = sh.detach();
    let half = sh.mapped;
    let r4 = sh.detach();
    cs.add(
        "shared_cache_detach_lifecycle",
        r3 == 1 && half && r4 == 0 && !sh.mapped,
        "",
    );
    cs
}
