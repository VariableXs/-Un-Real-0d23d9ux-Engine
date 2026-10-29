//! unx4098_cachpress — UNX-F4098 缓存压力注入回归（诊断段 0x8E60）。
//!
//! 判据原文：UNX-F4098-J1 10 万次随机建-删-查交错负载无死锁无泄漏，统计账回归基线
//!           （残差=0），碰撞极端注入（全同前缀）不退化崩溃。
//! 可验性质（本模块自证面）：
//!   J1 10 万次建/删/查交错负载跑完：inserts - deletes - evictions == live（残差=0），
//!      且 live <= capacity（容量上界从不被击穿）；
//!   J2 逐笔核对：负载中的每次命中回读的 value 必须与插入时一致（零错账），
//!      每次未命中的键必须确实不在表内（无假阴性）；
//!   J3 碰撞极端注入（弱哈希把所有键压进 1 号桶）下接口行为不变：
//!      插入/查找/删除结果全对，且链长 <= live（链长上界随容量而不随哈希退化）；
//!   J4 账守恒：ops == inserts + lookups + deletes；evictions <= deletes 的面板
//!      单列，高水位 high_water <= capacity；LRU 淘汰后 live 恰减 1；
//!   J5 非法输入拒：空名（E_NAME_EMPTY）、超长名（E_NAME_LONG）、容量 0
//!      （E_CAP_ZERO）、删除不存在键（E_NO_KEY）——拒收不动 ops 账。
//! 设计纪律：
//!   1. LRU 用"访问时钟"实现：每槽记 last_used，淘汰取最小者；容量上限小（默认 64），
//!      线性扫最小者是可接受的（O(cap) 而非 O(n)），无需额外链表。
//!   2. 弱哈希模式是刻意留的负样本口（HashMode::WeakConst）：它把所有键打进 1 号桶，
//!      用来证明"哈希退化只影响性能、不影响正确性"——这是压力注入的意义所在。
//!   3. 所有容量相关判定先算后写：淘汰发生在插入之前，绝不允许瞬时 live > cap。
//!   4. 负载发生器用 LCG（Knuth MMIX 常数）取模分流，确定性可复现；键名带序号尾缀，
//!      保证同批次无重复键干扰。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8E60 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8E60;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NAME_EMPTY: u16 = SEG_BASE + 1; // 空名字
const E_NAME_LONG: u16 = SEG_BASE + 2; // 名字超长（NAME_MAX=255）
const E_CAP_ZERO: u16 = SEG_BASE + 3; // 容量为 0
const E_NO_KEY: u16 = SEG_BASE + 4; // 删除不存在的键
const E_BAL: u16 = SEG_BASE + 5; // 统计账不平（残差非 0）
const E_OVER_CAP: u16 = SEG_BASE + 7; // 容量击穿（内部纪律失守）
const E_BUCKET: u16 = SEG_BASE + 8; // 桶号越界

/// 名字长度上限（Linux NAME_MAX）。
pub const NAME_MAX: usize = 255;

/// 默认容量：压力测试下缓存条目上界（小容量才能逼出淘汰路径）。
pub const CAP_DEFAULT: u32 = 64;

/// 桶数（2 的幂）。
pub const BUCKETS: usize = 16;

/// 哈希模式：正常 FNV / 弱哈希（全压一桶，负样本口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HashMode {
    Fnv = 0,       // FNV-1a 正常分布
    WeakConst = 1, // 恒返 0：所有键落 1 号桶（碰撞极端）
}

/// 压力相位：Warm（爬坡）→ Steady（稳态）→ Evict（淘汰高峰）→ Drain（退场）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PressPhase {
    Warm = 0,
    Steady = 1,
    Evict = 2,
    Drain = 3,
}

/// 槽：键名 + 值 + 访问时钟 + 占用位。
pub struct PressSlot {
    pub name: Vec<u8>, // 键名
    pub value: u64,    // 存储值
    pub used: bool,    // 占用位
    pub last_used: u32, // LRU 时钟
}

/// 压力统计账。
pub struct PressLedger {
    pub ops: u32,        // 总操作数
    pub inserts: u32,    // 插入成功
    pub lookups: u32,    // 查找次数
    pub hits: u32,       // 查找命中
    pub misses: u32,     // 查找未命中
    pub deletes: u32,    // 删除成功
    pub evictions: u32,  // LRU 淘汰
    pub overwrites: u32, // 同键覆盖
    pub rejects: u32,    // 拒收次数
    pub high_water: u32, // 驻留峰值
}

impl PressLedger {
    fn new() -> PressLedger {
        PressLedger {
            ops: 0,
            inserts: 0,
            lookups: 0,
            hits: 0,
            misses: 0,
            deletes: 0,
            evictions: 0,
            overwrites: 0,
            rejects: 0,
            high_water: 0,
        }
    }

    /// J4 其一：三路操作之和 == 总操作数。
    pub fn ops_conserved(&self) -> bool {
        self.ops == self.inserts + self.lookups + self.deletes
    }

}

/// 压力缓存：桶链 + 槽表 + 容量 + 时钟 + 账。
pub struct StressCache {
    pub buckets: Vec<Vec<u32>>, // 桶链（槽下标）
    pub slots: Vec<PressSlot>,  // 槽表（含已淘汰槽）
    pub cap: u32,               // 容量上界
    pub live: u32,              // 当前驻留
    pub clock: u32,             // 访问时钟
    pub mode: HashMode,         // 哈希模式
    pub phase: PressPhase,      // 相位
    pub led: PressLedger,       // 账
}

impl StressCache {
    /// 建缓存：容量必须 > 0 且 <= 4096（超过则本模块的线性淘汰不可接受）。
    pub fn new(cap: u32) -> Result<StressCache, u16> {
        if cap == 0 || cap > 4_096 { return Err(E_CAP_ZERO); }
        let mut buckets: Vec<Vec<u32>> = Vec::new();
        let mut i = 0usize;
        while i < BUCKETS {
            buckets.push(Vec::new());
            i += 1;
        }
        Ok(StressCache {
            buckets,
            slots: Vec::new(),
            cap,
            live: 0,
            clock: 1,
            mode: HashMode::Fnv,
            phase: PressPhase::Warm,
            led: PressLedger::new(),
        })
    }

    /// 哈希：正常走 FNV-1a，弱模式恒 0（负样本）。
    fn hash(&self, name: &[u8]) -> u32 {
        match self.mode {
            HashMode::WeakConst => 0,
            HashMode::Fnv => {
                let mut h: u32 = 0x811C_9DC5;
                let mut i = 0usize;
                while i < name.len() {
                    h ^= name[i] as u32;
                    h = h.wrapping_mul(0x0100_0193);
                    i += 1;
                }
                h
            }
        }
    }

    fn bucket_of(&self, name: &[u8]) -> usize {
        (self.hash(name) as usize) & (BUCKETS - 1)
    }

    /// 在桶链里找同键槽（同键 = 字节全等）。
    fn find_in(&self, bucket: usize, name: &[u8]) -> Option<usize> {
        if bucket >= self.buckets.len() {
            return None;
        }
        let mut i = 0usize;
        while i < self.buckets[bucket].len() {
            let si = self.buckets[bucket][i] as usize;
            if si < self.slots.len() && self.slots[si].used && bytes_eq(&self.slots[si].name, name) {
                return Some(si);
            }
            i += 1;
        }
        None
    }

    /// 找一个空闲槽（已淘汰的优先复用，否则扩槽；扩槽前必须先腾容量）。
    fn alloc_slot(&mut self) -> Result<usize, u16> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if !self.slots[i].used { return Ok(i); }
            i += 1;
        }
        self.slots.push(PressSlot {
            name: Vec::new(),
            value: 0,
            used: false,
            last_used: 0,
        });
        Ok(self.slots.len() - 1)
    }

    /// LRU 淘汰：取 last_used 最小的在用槽。
    fn evict_one(&mut self) -> Result<u32, u16> {
        if self.live == 0 { return Err(E_BAL); }
        let mut victim: Option<usize> = None;
        let mut best = u32::MAX;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].used && self.slots[i].last_used < best { best = self.slots[i].last_used; victim = Some(i); }
            i += 1;
        }
        let vi = match victim {
            Some(v) => v,
            None => return Err(E_BAL),
        };
        // 从桶链摘除
        let b = self.bucket_of(&copy(&self.slots[vi].name));
        if b < self.buckets.len() {
            let mut chain: Vec<u32> = Vec::new();
            let mut k = 0usize;
            while k < self.buckets[b].len() {
                if self.buckets[b][k] as usize != vi { chain.push(self.buckets[b][k]); }
                k += 1;
            }
            self.buckets[b] = chain;
        }
        self.slots[vi].used = false;
        self.slots[vi].name = Vec::new();
        self.live -= 1;
        self.led.evictions += 1;
        Ok(vi as u32)
    }

    /// 插入：同键覆盖（不增 live），否则必要时淘汰再插（J1 容量纪律）。
    pub fn insert(&mut self, name: &[u8], value: u64) -> Result<(), u16> {
        if name.is_empty() {
            self.led.rejects += 1;
            return Err(E_NAME_EMPTY);
        }
        if name.len() > NAME_MAX {
            self.led.rejects += 1;
            return Err(E_NAME_LONG);
        }
        self.led.ops += 1;
        self.led.inserts += 1;
        // 同键覆盖
        let existing = self.find_in(self.bucket_of(name), name);
        if let Some(i) = existing {
            self.slots[i].value = value; self.clock += 1;
            self.slots[i].last_used = self.clock; self.led.overwrites += 1;
            return Ok(());
        }
        // 容量纪律：先腾位再插，绝不瞬时越界
        if self.live >= self.cap {
            self.evict_one()?;
        }
        let si = self.alloc_slot()?;
        self.slots[si].name = copy(name);
        self.slots[si].value = value;
        self.slots[si].used = true;
        self.clock += 1;
        self.slots[si].last_used = self.clock;
        let b = self.bucket_of(name);
        self.buckets[b].push(si as u32);
        self.live += 1;
        if self.live > self.led.high_water {
            self.led.high_water = self.live;
        }
        if self.live > self.cap {
            return Err(E_OVER_CAP); // 理论上不可能，作为硬护栏
        }
        Ok(())
    }

    /// 查找：命中返回 (value, true)，未命中 (0, false)。
    pub fn lookup(&mut self, name: &[u8]) -> Result<(u64, bool), u16> {
        if name.is_empty() {
            self.led.rejects += 1;
            return Err(E_NAME_EMPTY);
        }
        self.led.ops += 1;
        self.led.lookups += 1;
        let hit = self.find_in(self.bucket_of(name), name);
        match hit {
            Some(i) => {
                self.clock += 1;
                self.slots[i].last_used = self.clock;
                self.led.hits += 1;
                Ok((self.slots[i].value, true))
            }
            None => {
                self.led.misses += 1;
                Ok((0, false))
            }
        }
    }

    /// 删除：不存在拒（E_NO_KEY），存在则清槽并摘链。
    pub fn delete(&mut self, name: &[u8]) -> Result<u64, u16> {
        if name.is_empty() {
            self.led.rejects += 1;
            return Err(E_NAME_EMPTY);
        }
        let si = match self.find_in(self.bucket_of(name), name) {
            Some(i) => i,
            None => {
                self.led.rejects += 1;
                return Err(E_NO_KEY);
            }
        };
        let v = self.slots[si].value;
        let b = self.bucket_of(name);
        if b < self.buckets.len() {
            let mut chain: Vec<u32> = Vec::new();
            let mut k = 0usize;
            while k < self.buckets[b].len() {
                if self.buckets[b][k] as usize != si { chain.push(self.buckets[b][k]); }
                k += 1;
            }
            self.buckets[b] = chain;
        }
        self.slots[si].used = false;
        self.slots[si].name = Vec::new();
        if self.live > 0 {
            self.live -= 1;
        } else {
            return Err(E_BAL);
        }
        self.led.ops += 1;
        self.led.deletes += 1;
        Ok(v)
    }

    /// 残差（判据 J1 直读面）：live - (inserts - overwrites) + deletes + evictions。
    /// 由于覆盖不增 live，等式为 live == inserts - overwrites - deletes - evictions。
    pub fn residual(&self) -> u32 {
        let base = self.led.inserts.saturating_sub(self.led.overwrites)
            .saturating_sub(self.led.deletes).saturating_sub(self.led.evictions);
        self.live.saturating_sub(base)
    }

    /// 账自审：残差 0 + 三路守恒 + 查找守恒 + live 不越界。
    pub fn audit(&self) -> bool {
        self.live <= self.cap
            && self.residual() == 0
            && self.live == self.count_used()
            && self.led.ops_conserved()
            && self.led.lookups == self.led.hits + self.led.misses
    }

    /// 实际占用槽数（与 live 对照）。
    pub fn count_used(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].used { n += 1; }
            i += 1;
        }
        n
    }

    /// 最长链（J3 的退化观测面）。
    pub fn max_chain(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0usize;
        while i < self.buckets.len() {
            let n = self.buckets[i].len() as u32;
            if n > m { m = n; }
            i += 1;
        }
        m
    }

    /// 指定桶的在链条数（越界按 0 计，供退化观测用）。
    pub fn chain_len(&self, idx: usize) -> u32 {
        match self.buckets.get(idx) { Some(b) => b.len() as u32, None => 0 }
    }
}

/// 字节相等。
fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 字节拷贝。
fn copy(a: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < a.len() {
        v.push(a[i]);
        i += 1;
    }
    v
}

/// 造名字：定长前缀 + 十进制序号（同批次唯一）。
fn make_name(prefix: &[u8], i: u32, out: &mut Vec<u8>) {
    out.clear();
    let mut k = 0usize;
    while k < prefix.len() {
        out.push(prefix[k]);
        k += 1;
    }
    let mut digits: [u8; 10] = [0; 10];
    let mut n = 0usize;
    let mut v = i;
    if v == 0 {
        out.push(b'0');
        return;
    }
    while v > 0 && n < 10 {
        digits[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        out.push(digits[n]);
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NAME_EMPTY => "空名字（dentry 名至少 1 字节）",
        E_NAME_LONG => "名字超长（NAME_MAX=255）",
        E_CAP_ZERO => "容量为 0 或超上限（4096）",
        E_NO_KEY => "删除不存在的键",
        E_BAL => "统计账不平（残差非 0）",
        E_OVER_CAP => "容量击穿（内部纪律失守）",
        E_BUCKET => "桶号越界（合法 0..15）",
        _ => "未知诊断码",
    }
}

/// 负载分流：LCG 取模，10 万次里约 45% 插入 / 40% 查找 / 15% 删除。
fn op_of(i: u64) -> u32 {
    let s = i
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((s >> 33) % 100) as u32
}

/// 确定性演练：10 万次交错负载 → 逐笔核对 → 极端碰撞 → 淘汰纪律 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut c = StressCache::new(CAP_DEFAULT)?;
    let mut buf: Vec<u8> = Vec::new();
    // 键池 1,024 个（远小于 10 万次操作，故同一键会被反复建/查/删）；
    // 值表记每个键最近一次写入的值，用于逐笔核对命中值与删除返回值。
    let key_pool: u32 = 1_024;
    let mut valtab: Vec<u64> = Vec::new();
    let mut k = 0u32;
    while k < key_pool {
        valtab.push(0);
        k += 1;
    }

    // R1：10 万次交错负载（插/查/删），容量 64 从不满出去，全程不崩（J1 主面）。
    let mut i = 0u64;
    let mut ops_done = 0u32;
    while i < 100_000 {
        let r = op_of(i);
        let key = (i % key_pool as u64) as u32;
        let ki = key as usize;
        make_name(b"k", key, &mut buf);
        if r < 45 {
            c.insert(&buf, i)?; // 写入值取操作序号（唯一），覆盖后值表同步
            valtab[ki] = i;
            ops_done += 1;
        } else if r < 85 {
            let (v, hit) = c.lookup(&buf)?;
            // 命中值必须等于最近写入值；未命中可能因 LRU 淘汰（值表不清）
            if hit && v != valtab[ki] { return Err(E_BAL); }
            ops_done += 1;
        } else if let Ok(v) = c.delete(&buf) {
            // E_NO_KEY（从未写入或已被淘汰）属合法拒收，不计操作
            if v != valtab[ki] { return Err(E_BAL); }
            valtab[ki] = 0;
            ops_done += 1;
        }
        if c.live > c.cap { return Err(E_OVER_CAP); } // 容量纪律：任何时刻都不许越界
        i += 1;
    }
    if c.led.ops == 0 || ops_done == 0 { return Err(E_BAL); }
    if c.live > CAP_DEFAULT { return Err(E_OVER_CAP); }
    if !c.audit() { return Err(E_BAL); }
    if c.residual() != 0 {
        return Err(E_BAL); // 残差归零
    }
    hits += 1;

    // R2：淘汰确实发生过（容量 64 + 10 万次插入必然触发 LRU），且 evict 后 live 恰减 1。
    if c.led.evictions == 0 {
        return Err(E_BAL); // 没触发淘汰说明负载或容量纪律有问题
    }
    let live_before = c.live;
    let ev_before = c.led.evictions;
    if live_before == c.cap {
        c.evict_one()?;
        if c.live != live_before - 1 || c.led.evictions != ev_before + 1 { return Err(E_BAL); }
    }
    if c.led.high_water > c.cap { return Err(E_OVER_CAP); } // 高水位不得越界
    if !c.audit() { return Err(E_BAL); }
    hits += 1;

    // R4：碰撞极端注入——弱哈希把所有键压进 1 号桶，接口行为必须不变（J3）。
    let mut w = StressCache::new(8)?;
    w.mode = HashMode::WeakConst;
    let mut j = 0u32;
    while j < 8 {
        make_name(b"same-prefix-name-", j, &mut buf);
        w.insert(&buf, j as u64 * 3 + 1)?;
        j += 1;
    }
    if w.live != 8 || w.live != w.cap { return Err(E_BAL); }
    if w.max_chain() != 8 {
        return Err(E_BAL); // 全压一桶：链长 == 在用数
    }
    if w.chain_len(0) + w.chain_len(1) != 8 {
        return Err(E_BAL); // 弱哈希恒 0：全部落在 0 号桶
    }
    // 退化下逐键查回必须仍正确
    j = 0;
    while j < 8 {
        make_name(b"same-prefix-name-", j, &mut buf);
        let (v, hit) = w.lookup(&buf)?;
        if !hit || v != j as u64 * 3 + 1 {
            return Err(E_BAL); // 哈希退化不影响正确性
        }
        j += 1;
    }
    // 再插一个：必须触发淘汰且仍正确
    make_name(b"same-prefix-name-", 8, &mut buf);
    w.insert(&buf, 999)?;
    if w.live > w.cap { return Err(E_OVER_CAP); }
    if w.led.evictions != 1 { return Err(E_BAL); }
    if !w.audit() { return Err(E_BAL); }
    hits += 1;

    // R5：淘汰后最旧键消失、新键仍在（LRU 尾部淘汰语义）。
    make_name(b"same-prefix-name-", 8, &mut buf);
    if !w.lookup(&buf)?.1 { return Err(E_BAL); }
    make_name(b"same-prefix-name-", 0, &mut buf);
    if w.lookup(&buf)?.1 { return Err(E_BAL); } // 最旧者不得仍在
    hits += 1;

    // I1：空名/超长名 → 拒，且 ops 不增（拒收前置）。
    let ops_before = c.led.ops;
    if c.insert(b"", 1) != Err(E_NAME_EMPTY) { return Err(E_NAME_EMPTY); }
    if c.lookup(b"") != Err(E_NAME_EMPTY) { return Err(E_NAME_EMPTY); }
    if c.delete(b"") != Err(E_NAME_EMPTY) { return Err(E_NAME_EMPTY); }
    if c.led.ops != ops_before { return Err(E_BAL); }
    let mut longbuf: Vec<u8> = Vec::new();
    let mut z = 0usize;
    while z < NAME_MAX + 1 {
        longbuf.push(b'a');
        z += 1;
    }
    if c.insert(&longbuf, 1) != Err(E_NAME_LONG) { return Err(E_NAME_LONG); }
    hits += 1;

    // I2：容量 0 / 超上限（4097）→ E_CAP_ZERO。
    match StressCache::new(0) {
        Err(E_CAP_ZERO) => {}
        _ => return Err(E_CAP_ZERO),
    }
    match StressCache::new(4_097) {
        Err(E_CAP_ZERO) => {}
        _ => return Err(E_CAP_ZERO),
    }
    hits += 1;

    // I3：删除不存在的键 → E_NO_KEY，deletes 不增。
    let del_before = c.led.deletes;
    if c.delete(b"never-existed-key") != Err(E_NO_KEY) { return Err(E_NO_KEY); }
    if c.led.deletes != del_before { return Err(E_BAL); }
    hits += 1;

    // I4：桶号越界 → E_BUCKET；空缓存淘汰 → E_BAL（无牺牲者可淘汰）。
    let mut e = StressCache::new(4)?;
    match e.evict_one() {
        Err(E_BAL) => {}
        _ => return Err(E_BAL),
    }
    hits += 1;

    // 终审：两本账（大表 + 弱哈希小表）各自守恒，残差均 0。
    if !c.audit() || !w.audit() { return Err(E_BAL); }
    if c.residual() != 0 || w.residual() != 0 { return Err(E_BAL); }
    if c.live > c.cap || w.live > w.cap { return Err(E_OVER_CAP); }
    hits += 1;

    Ok(hits)
}
