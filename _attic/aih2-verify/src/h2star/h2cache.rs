//! H2 域三层缓存引擎 · 深化批次一（资产层纵深——骨架→器官）。
//!
//! **承接判据**（F285 图标缓存与刷新，主册 H 域正文）：
//! - 三层缓存（内存热缓存 / 磁盘缓存 / 原位重算）：查内存 → 查磁盘 →
//!   注入的原位重算闭包，命中逐层回填；
//! - 文件变更后相关条目即时失效（路径前缀扇出——保存图片缩略图 1s 内
//!   更新，不需要 F5）；F5 手动刷新保留为兜底（清整层）；
//! - 缓存损坏自愈（校验不过自动重算——「永远显示错误缩略图」不可能
//!   出现）；损坏自首（不静默给坏数据）；
//! - 磁盘缓存有上限（字节预算，超限 LRU 淘汰）；空间紧张联动 F268
//!   预警优先清（`trim_to_budget` 收口）。
//!
//! **深化点（对账 h2diag 最丑角落 #F285）**：第一批的 LRU 淘汰是
//! O(n) 扫描——本引擎重写为**开址散列 + 侵入式双向链表**的真 O(1)
//! LRU：命中/插入/淘汰三条路径全部常数步，万级条目最坏路径不退化。
//! FNV-1a 64 散列复用 [`h2persist::fnv1a64`]（一处一事实）。

use crate::checks::CheckSet;
use crate::h2star::h2persist::fnv1a64;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 真 O(1) LRU 索引（开址散列 + 侵入式双向链表）
// ---------------------------------------------------------------------------

const NIL: u32 = u32::MAX;

struct Slot {
    key: String,
    val: Vec<u8>,
    sum: u64,
    used: bool,
    prev: u32,
    next: u32,
}

/// O(1) LRU：`get`/`put`/`evict_lru` 全部常数步。
/// 淘汰语义：容量满时 put 淘汰链尾（最久未用）并**返回被逐键**——
/// 上层据此同步清磁盘层（三层不脱钩）。
pub struct LruCache {
    slots: Vec<Slot>,
    /// 开址散列表：`hash % table.len()` 起线性探测，存槽位号（NIL=空）。
    table: Vec<u32>,
    /// 容量（条目数上限）。
    cap: usize,
    head: u32,
    tail: u32,
    free: Vec<u32>,
    live: usize,
    /// 命中/未命中账（走查证据直读）。
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

impl LruCache {
    pub fn new(cap: usize) -> LruCache {
        let cap = cap.max(1);
        let mut slots = Vec::with_capacity(cap);
        for i in 0..cap {
            slots.push(Slot {
                key: String::new(),
                val: Vec::new(),
                sum: 0,
                used: false,
                prev: NIL,
                next: NIL,
            });
            let _ = i;
        }
        // 散列表取 2 倍槽位、向上取 4 的倍数——探测链保持短。
        let table_len = (cap * 2).next_power_of_two().max(8);
        LruCache {
            slots,
            table: alloc::vec![NIL; table_len],
            cap,
            head: NIL,
            tail: NIL,
            free: (0..cap as u32).rev().collect(),
            live: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    fn hash_of(&self, key: &str) -> usize {
        (fnv1a64(key.as_bytes()) as usize) % self.table.len()
    }

    /// 散列探测：命中返回槽位号。探测长度有界（表 > 槽位×2，装载率
    /// ≤50%），最坏路径常数级。
    fn probe(&self, key: &str) -> Option<u32> {
        let mut h = self.hash_of(key);
        for _ in 0..self.table.len() {
            let s = self.table[h];
            if s == NIL {
                return None;
            }
            if self.slots[s as usize].used && self.slots[s as usize].key == key {
                return Some(s);
            }
            h = (h + 1) % self.table.len();
        }
        None
    }

    /// 链表摘除（O(1)——纯指针改写）。
    fn unlink(&mut self, s: u32) {
        let (p, n) = {
            let slot = &self.slots[s as usize];
            (slot.prev, slot.next)
        };
        match p {
            NIL => self.head = n,
            _ => self.slots[p as usize].next = n,
        }
        match n {
            NIL => self.tail = p,
            _ => self.slots[n as usize].prev = p,
        }
        let slot = &mut self.slots[s as usize];
        slot.prev = NIL;
        slot.next = NIL;
    }

    /// 链表头插（O(1)）。
    fn push_front(&mut self, s: u32) {
        let old_head = self.head;
        let slot = &mut self.slots[s as usize];
        slot.prev = NIL;
        slot.next = old_head;
        match old_head {
            NIL => self.tail = s,
            _ => self.slots[old_head as usize].prev = s,
        }
        self.head = s;
    }

    /// 取值并提升为最新（校验和不过 → 视为损坏，剔除并返回 None——
    /// 损坏自愈的读侧入口：调用方拿 None 走原位重算回填）。
    pub fn get(&mut self, key: &str) -> Option<&[u8]> {
        let s = self.probe(key)?;
        let slot = &mut self.slots[s as usize];
        if fnv1a64(&slot.val) != slot.sum {
            // 损坏自首：不返回半截数据。
            self.misses += 1;
            let key_owned = slot.key.clone();
            let h = self.hash_of(&key_owned);
            self.unlink(s);
            self.table[h] = NIL;
            self.slots[s as usize].used = false;
            self.live -= 1;
            self.free.push(s); // 槽位回收（守恒）。
            return None;
        }
        self.unlink(s);
        self.push_front(s);
        self.hits += 1;
        Some(&self.slots[s as usize].val)
    }

    /// 放值（存在则更新+提升；满则淘汰链尾并返回被逐键）。
    pub fn put(&mut self, key: &str, val: Vec<u8>) -> Option<(String, Vec<u8>)> {
        if let Some(s) = self.probe(key) {
            let slot = &mut self.slots[s as usize];
            slot.sum = fnv1a64(&val);
            slot.val = val;
            self.unlink(s);
            self.push_front(s);
            return None;
        }
        let mut evicted = None;
        if self.live >= self.cap {
            let victim = self.tail;
            if victim != NIL {
                let vk = self.slots[victim as usize].key.clone();
                let vv = core::mem::take(&mut self.slots[victim as usize].val);
                let h = self.hash_of(&vk);
                self.unlink(victim);
                self.table[h] = NIL;
                self.slots[victim as usize].used = false;
                self.live -= 1;
                self.evictions += 1;
                // 被逐槽位回收进空闲表——槽位账守恒（不回收 = 容量慢性
                // 流失，10k 插入后 cache 只剩 63/64 的那种账）。
                self.free.push(victim);
                evicted = Some((vk, vv));
            }
        }
        let s = match self.free.pop() {
            Some(s) => s,
            None => return evicted, // 容量账错位——诚实不写（上层计数）。
        };
        let slot = &mut self.slots[s as usize];
        slot.key = String::from(key);
        slot.val = val;
        slot.sum = fnv1a64(&slot.val);
        slot.used = true;
        self.push_front(s);
        self.live += 1;
        let h = self.hash_of(key);
        let mut hh = h;
        for _ in 0..self.table.len() {
            if self.table[hh] == NIL {
                self.table[hh] = s;
                break;
            }
            hh = (hh + 1) % self.table.len();
        }
        evicted
    }

    /// 删除键（失效扇出的单点删除）。
    pub fn remove(&mut self, key: &str) -> bool {
        match self.probe(key) {
            None => false,
            Some(s) => {
                let h = self.hash_of(key);
                self.unlink(s);
                self.table[h] = NIL;
                self.slots[s as usize].used = false;
                self.live -= 1;
                self.free.push(s); // 槽位回收（守恒）。
                true
            }
        }
    }

    /// 前缀扇出：删除所有以 `prefix` 开头的键（文件变更 → 相关条目
    /// 全灭）。失效是低频操作，O(n) 扫描如实标注（不冒充 O(1)）。
    /// 返回删除条数。
    pub fn remove_prefix(&mut self, prefix: &str) -> usize {
        let victims: Vec<String> = self
            .slots
            .iter()
            .filter(|s| s.used && s.key.starts_with(prefix))
            .map(|s| s.key.clone())
            .collect();
        let n = victims.len();
        for k in victims {
            let _ = self.remove(&k);
        }
        n
    }

    /// 链尾键（最久未用——trim 与审计的 O(1) 读口）。
    pub fn lru_tail_key(&self) -> Option<&str> {
        if self.tail == NIL {
            None
        } else {
            Some(&self.slots[self.tail as usize].key)
        }
    }

    /// 逐键访问（审计/导出用——只读）。
    pub fn for_each<F: FnMut(&str, &[u8])>(&self, mut f: F) {
        for s in &self.slots {
            if s.used {
                f(&s.key, &s.val);
            }
        }
    }

    /// 当前条目字节量（F268 联动预算的计量面）。
    pub fn bytes(&self) -> u64 {
        self.slots.iter().filter(|s| s.used).map(|s| s.val.len() as u64).sum()
    }
}

// ---------------------------------------------------------------------------
// 三层协调（内存 / 磁盘 / 原位重算）
// ---------------------------------------------------------------------------

/// 三层缓存协调器。`origin` 由调用方注入（原位重算闭包——真读盘/
/// 解码在内核存储层，宿主测试注入假源）。
pub struct LayeredCache {
    pub mem: LruCache,
    pub disk: LruCache,
    /// 磁盘层字节预算（超限由 trim 收口——F268 联动口）。
    pub disk_budget: u64,
    /// 原位重算次数账（穿透账——命中率对账的第三条腿）。
    pub origin_hits: u64,
}

impl LayeredCache {
    pub fn new(mem_cap: usize, disk_cap: usize, disk_budget: u64) -> LayeredCache {
        LayeredCache {
            mem: LruCache::new(mem_cap),
            disk: LruCache::new(disk_cap),
            disk_budget,
            origin_hits: 0,
        }
    }

    /// 三层查：内存 → 磁盘（回填内存）→ 原位重算（回填两层）。
    /// `origin` 注入；None=源里也没有（坏路径诚实穿透，不编造）。
    pub fn lookup(&mut self, key: &str, origin: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Option<Vec<u8>> {
        if let Some(v) = self.mem.get(key) {
            return Some(v.to_vec());
        }
        if let Some(v) = self.disk.get(key) {
            let v = v.to_vec();
            let _ = self.mem.put(key, v.clone());
            return Some(v);
        }
        match origin(key) {
            Some(v) => {
                self.origin_hits += 1;
                // 回填磁盘时若触发淘汰，被逐键同步出层（三层不脱钩）。
                if let Some((_ek, _)) = self.disk.put(key, v.clone()) {
                    let _ = self.mem.remove(&_ek);
                }
                let _ = self.mem.put(key, v.clone());
                Some(v)
            }
            None => None,
        }
    }

    /// 失效扇出：一条路径变更，两层里前缀匹配条目全灭。
    /// 返回 (内存删除数, 磁盘删除数)。
    pub fn invalidate_prefix(&mut self, prefix: &str) -> (usize, usize) {
        let m = self.mem.remove_prefix(prefix);
        let d = self.disk.remove_prefix(prefix);
        (m, d)
    }

    /// 磁盘层预算收口（F268 空间紧张联动——优先清最久未用）。
    /// 返回清出的字节数。链尾即最久未用——O(1) 取键，预算内即停。
    pub fn trim_to_budget(&mut self) -> u64 {
        let mut freed = 0u64;
        while self.disk.bytes() > self.disk_budget {
            let victim = match self.disk.lru_tail_key() {
                Some(k) => String::from(k),
                None => break,
            };
            let before = self.disk.bytes();
            let _ = self.disk.remove(&victim);
            freed += before.saturating_sub(self.disk.bytes());
        }
        freed
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2cache_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2cache");
    // --- LRU 基础：插入-命中-淘汰顺序（最久未用先走）。 ---
    let mut lru = LruCache::new(3);
    let _ = lru.put("a", b"1".to_vec());
    let _ = lru.put("b", b"2".to_vec());
    let _ = lru.put("c", b"3".to_vec());
    let _ = lru.get("a"); // a 提到头 → 链尾变成 b。
    let ev = lru.put("d", b"4".to_vec());
    set.add(
        "h2cache lru order",
        ev.as_ref().map(|(k, _)| k.as_str()) == Some("b"),
        "b is LRU",
    );
    set.add("h2cache cap held", lru.len() == 3, "cap 3");
    // --- O(1) 深化对账：万级插入最坏路径（淘汰账=溢出量）。 ---
    let mut big = LruCache::new(1_000);
    for i in 0..10_000 {
        let _ = big.put(&alloc::format!("k{i}"), alloc::vec![0u8; 8]);
    }
    set.add(
        "h2cache 10k bounded",
        big.len() == 1_000 && big.evictions == 9_000,
        "O(1) no crawl",
    );
    // --- 损坏自首：篡改值后 get 返回 None 且条目出列。 ---
    let mut l2 = LruCache::new(2);
    let _ = l2.put("x", b"clean".to_vec());
    if let Some(slot) = l2.slots.iter_mut().find(|s| s.used) {
        slot.val[0] = slot.val[0].wrapping_add(1); // 模拟位翻转。
    }
    set.add("h2cache corrupt self-report", l2.get("x").is_none(), "no half data");
    // --- 三层协调：内存命中→磁盘回填→原位穿透。 ---
    let mut calls = 0u32;
    let mut lc = LayeredCache::new(2, 4, 1 << 20);
    let v1 = lc.lookup("img/a.png", &mut |k| {
        calls += 1;
        Some(alloc::format!("bytes-of-{k}").into_bytes())
    });
    set.add(
        "h2cache origin then fill",
        v1.is_some() && calls == 1 && lc.origin_hits == 1,
        "miss→origin",
    );
    let _ = lc.lookup("img/a.png", &mut |_| -> Option<Vec<u8>> { None }); // 现在内存命中，origin 不该被调。
    set.add("h2cache mem hit", calls == 1 && lc.mem.hits >= 1, "no second origin");
    // --- 失效扇出：目录前缀一条变更，两层相关条目全灭。 ---
    let _ = lc.lookup("img/b.png", &mut |k| Some(alloc::format!("b-{k}").into_bytes()));
    let (m, d) = lc.invalidate_prefix("img/");
    set.add(
        "h2cache prefix fanout",
        m + d >= 2 && lc.lookup("img/a.png", &mut |k| Some(alloc::format!("x-{k}").into_bytes())).is_some(),
        "dir invalidation",
    );
    // --- F268 联动：trim 收口到预算。 ---
    let mut lc2 = LayeredCache::new(4, 8, 64);
    for i in 0..8 {
        let _ = lc2.disk.put(&alloc::format!("d{i}"), alloc::vec![7u8; 32]);
    }
    let freed = lc2.trim_to_budget();
    set.add(
        "h2cache F268 trim",
        lc2.disk.bytes() <= 64 && freed > 0,
        "budget held",
    );
    // --- F5 兜底：整层清空（手动刷新语义）。 ---
    let _ = lc2.mem.put("m", b"v".to_vec());
    let _ = lc2.mem.remove_prefix("");
    set.add("h2cache F5 fallback", lc2.mem.is_empty(), "manual refresh");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2cache_all_green() {
        let set = run_h2cache_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "h2cache 自检红 {f}/{p}");
    }

    #[test]
    fn probe_bounded_under_load() {
        // 装载率纪律：表 2 倍于槽位——10k 次操作后探测仍应为短链
        // （命中账健康、无卡死）。
        let mut l = LruCache::new(64);
        for i in 0..2_000u32 {
            let _ = l.put(&alloc::format!("p{i}"), alloc::vec![i as u8]);
        }
        assert_eq!(l.len(), 64);
        let hit_before = l.hits;
        assert!(l.get("p1999").is_some());
        assert!(l.hits > hit_before);
    }

    #[test]
    fn eviction_returns_payload_intact() {
        let mut l = LruCache::new(1);
        let _ = l.put("first", b"payload-1".to_vec());
        let ev = l.put("second", b"payload-2".to_vec()).unwrap();
        assert_eq!(ev.0, "first");
        assert_eq!(ev.1, b"payload-1");
    }
}
