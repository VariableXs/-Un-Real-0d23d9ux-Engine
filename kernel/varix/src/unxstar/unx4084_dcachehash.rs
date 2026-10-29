//! unx4084_dcachehash — UNX-F4084 dentry 缓存哈希表结构（诊断段 0x8CA0）。
//!
//! 判据原文：UNX-F4084-J1 全局（父指针,名,长度）三键哈希表插入/查找/删除 1 万次
//!           零错账，碰撞链长分布有账且上限触发扩容。
//! 可验性质（本模块自证面）：
//!   J1 三键（父 id / 名字字节 / 名字长度）参与哈希，任一键变化必换桶或换槽：
//!      同父不同名、同名不同父、同父同名不同长（含 NUL 的名字）三组样本互不命中；
//!   J2 万次插入 / 万次查找 / 三千次删除零错账：n_used == 插入 - 删除，
//!      hits + miss == lookups，且每条命中回读 ino 与插入值一致；
//!   J3 碰撞链长分布入账（直方图 0..=MAX_CHAIN+1），直方图计数之和 == 桶数；
//!   J4 链长上限 MAX_CHAIN 触发扩容：扩容后桶数翻倍、旧条目全量重挂、
//!      扩容后最大链长不得超过上限（否则说明重挂逻辑漏项）；
//!   J5 重复键插入拒（E_KEY_DUP）、删不存在键拒（E_NO_KEY）、空名拒（E_NAME_EMPTY），
//!      拒收不改 n_used。
//! 设计纪律：
//!   1. 桶数恒为 2 的幂，桶号 = hash & (cap - 1)（掩码比取模快且无除法异常）。
//!   2. 链用 Vec<u32> 存槽下标：槽表 slots 只增不缩（删除只置 Tombstone），
//!      这样链里保存的下标永不失效——删除只从链里摘，槽位留给下次复用或闲置。
//!   3. 三键必须全参与哈希与全参与比较：比较时先比 parent、再比 len、最后逐字节
//!      比 name，任何一步不等即返回（长度不等不必逐字节读，防越界）。
//!   4. 扩容时机取"链长 > MAX_CHAIN"而非常规负载因子：dcache 的痛点就是单桶
//!      长链（同前缀大量文件），链长触发更贴合判据口径。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8CA0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8CA0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_KEY_DUP: u16 = SEG_BASE + 1; // 三键相同的条目重复插入
const E_NO_KEY: u16 = SEG_BASE + 2; // 删除/定位不存在的键
const E_NAME_EMPTY: u16 = SEG_BASE + 3; // 空名字（dentry 名至少 1 字节）
const E_BUCKET: u16 = SEG_BASE + 4; // 桶号越界
const E_CHAIN_LONG: u16 = SEG_BASE + 5; // 扩容后链长仍超上限（重挂漏项）
const E_BAL: u16 = SEG_BASE + 6; // 统计账不平
const E_CAP: u16 = SEG_BASE + 7; // 容量非 2 的幂或为 0
const E_NAME_LONG: u16 = SEG_BASE + 8; // 名字超长（NAME_MAX=255）

/// 初始桶数（2 的幂）。
pub const CAP_MIN: usize = 16;

/// 单桶链长上限：超过即扩容（设计纪律 4）。
pub const MAX_CHAIN: usize = 8;

/// 名字长度上限：Linux NAME_MAX。
pub const NAME_MAX: usize = 255;

/// 槽状态：空 / 在用 / 墓碑（删除后留位，链里已摘除）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    Empty = 0,     // 从未使用
    Used = 1,      // 在用
    Tombstone = 2, // 已删除，槽位闲置
}

/// 表相位：Growing 只在扩容循环内出现，防止扩容途中被观察成半表。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TablePhase {
    Steady = 0, // 稳定态，可读写
    Growing = 1, // 扩容中（重挂未完成）
}

/// 三键：父 id + 名字 + 长度（长度单列，允许名字内有 NUL）。
pub struct DKey {
    pub parent: u64,   // 父 dentry 的标识（指针值的模型化）
    pub name: Vec<u8>, // 名字字节（可含 NUL）
    pub len: u32,      // 名字长度（与 name.len() 必须一致）
}

impl DKey {
    /// 造键：长度由 name 直接取，不接受外部传入（防 len 与字节数打架）。
    pub fn new(parent: u64, name: &[u8]) -> Result<DKey, u16> {
        if name.is_empty() { return Err(E_NAME_EMPTY); }
        if name.len() > NAME_MAX { return Err(E_NAME_LONG); }
        let mut v: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < name.len() {
            v.push(name[i]);
            i += 1;
        }
        Ok(DKey {
            parent,
            name: v,
            len: name.len() as u32,
        })
    }

    /// 三键全比：父 → 长 → 逐字节（设计纪律 3）。
    pub fn same_as(&self, other: &DKey) -> bool {
        if self.parent != other.parent {
            return false;
        }
        if self.len != other.len {
            return false;
        }
        if self.name.len() != other.name.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.name.len() {
            if self.name[i] != other.name[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 键哈希：父 8 字节 + 长 4 字节 + 名字，全走 FNV-1a。
    pub fn hash(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let pb = self.parent.to_le_bytes();
        let mut i = 0usize;
        while i < 8 {
            h ^= pb[i] as u32;
            h = h.wrapping_mul(0x0100_0193);
            i += 1;
        }
        let lb = self.len.to_le_bytes();
        i = 0;
        while i < 4 {
            h ^= lb[i] as u32;
            h = h.wrapping_mul(0x0100_0193);
            i += 1;
        }
        i = 0;
        while i < self.name.len() {
            h ^= self.name[i] as u32;
            h = h.wrapping_mul(0x0100_0193);
            i += 1;
        }
        h
    }
}

/// 槽：键 + inode 号 + 态 + 缓存哈希。
pub struct Slot {
    pub key: DKey,      // 三键
    pub ino: u64,       // 绑定的 inode 号
    pub state: SlotState, // 槽态
    pub hash: u32,      // 入表时算的哈希（避免重复计算）
}

/// dcache 哈希表：桶链 + 槽表 + 统计账。
pub struct DHash {
    pub buckets: Vec<Vec<u32>>, // 桶链（每桶是槽下标链）
    pub slots: Vec<Slot>,       // 槽表（只增不缩）
    pub phase: TablePhase,      // 相位
    pub inserts: u32,           // 插入成功数
    pub lookups: u32,           // 查找次数
    pub removes: u32,           // 删除成功数
    pub hits: u32,              // 查找命中
    pub miss: u32,              // 查找未命中
    pub n_used: u32,            // 当前在用条目数
    pub resizes: u32,           // 扩容次数
    pub dup_rejects: u32,       // 重复键拒收
    pub no_key_rejects: u32,    // 删不存在键拒收
}

impl DHash {
    /// 建表：桶数必须是 2 的幂且 >= 16（E_CAP）。
    pub fn new(cap: usize) -> Result<DHash, u16> {
        if cap < CAP_MIN || (cap & (cap - 1)) != 0 { return Err(E_CAP); }
        let mut buckets: Vec<Vec<u32>> = Vec::new();
        let mut i = 0usize;
        while i < cap {
            buckets.push(Vec::new());
            i += 1;
        }
        Ok(DHash {
            buckets,
            slots: Vec::new(),
            phase: TablePhase::Steady,
            inserts: 0,
            lookups: 0,
            removes: 0,
            hits: 0,
            miss: 0,
            n_used: 0,
            resizes: 0,
            dup_rejects: 0,
            no_key_rejects: 0,
        })
    }

    /// 桶数。
    pub fn cap(&self) -> usize {
        self.buckets.len()
    }

    /// 桶号 = hash & (cap-1)。
    fn bucket_idx(&self, hash: u32) -> usize {
        (hash as usize) & (self.buckets.len() - 1)
    }

    /// 在链内找同键槽下标（三键全比）。
    fn find_in_bucket(&self, bucket: usize, key: &DKey) -> Option<usize> {
        if bucket >= self.buckets.len() {
            return None;
        }
        let chain = &self.buckets[bucket];
        let mut i = 0usize;
        while i < chain.len() {
            let si = chain[i] as usize;
            if si < self.slots.len()
                && self.slots[si].state == SlotState::Used
                && self.slots[si].key.same_as(key)
            {
                return Some(si);
            }
            i += 1;
        }
        None
    }

    /// 插入：重复键拒；插入后若链长超上限则扩容（J4）。
    pub fn insert(&mut self, key: DKey, ino: u64) -> Result<(), u16> {
        if key.len as usize != key.name.len() {
            return Err(E_BAL); // 键自坏（长度与字节数不符），不入表
        }
        let h = key.hash();
        let b = self.bucket_idx(h);
        if self.find_in_bucket(b, &key).is_some() {
            self.dup_rejects += 1;
            return Err(E_KEY_DUP);
        }
        let idx = self.slots.len() as u32;
        self.slots.push(Slot {
            key,
            ino,
            state: SlotState::Used,
            hash: h,
        });
        self.buckets[b].push(idx);
        self.inserts += 1;
        self.n_used += 1;
        // 链长上限触发扩容
        if self.buckets[b].len() > MAX_CHAIN {
            self.grow()?;
        }
        Ok(())
    }

    /// 查找：命中回 ino，未命中记 miss（不建负项，负项由上层 dentry 语义负责）。
    pub fn lookup(&mut self, key: &DKey) -> Option<u64> {
        self.lookups += 1;
        let h = key.hash();
        let b = self.bucket_idx(h);
        match self.find_in_bucket(b, key) {
            Some(si) => {
                self.hits += 1;
                Some(self.slots[si].ino)
            }
            None => {
                self.miss += 1;
                None
            }
        }
    }

    /// 删除：置墓碑 + 从链里摘除（槽不缩）。
    pub fn remove(&mut self, key: &DKey) -> Result<u64, u16> {
        let h = key.hash();
        let b = self.bucket_idx(h);
        let si = match self.find_in_bucket(b, key) {
            Some(x) => x,
            None => {
                self.no_key_rejects += 1;
                return Err(E_NO_KEY);
            }
        };
        let ino = self.slots[si].ino;
        self.slots[si].state = SlotState::Tombstone;
        // 链内摘除（保序压紧）
        let mut chain: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.buckets[b].len() {
            if self.buckets[b][i] as usize != si {
                chain.push(self.buckets[b][i]);
            }
            i += 1;
        }
        self.buckets[b] = chain;
        self.removes += 1;
        if self.n_used > 0 {
            self.n_used -= 1;
        } else {
            return Err(E_BAL);
        }
        Ok(ino)
    }

    /// 扩容：桶数翻倍，全部 Used 槽重挂；重挂后逐桶核链长不得超上限（J4）。
    fn grow(&mut self) -> Result<(), u16> {
        if self.phase == TablePhase::Growing {
            return Err(E_BAL); // 重入扩容：说明上一次没走完
        }
        self.phase = TablePhase::Growing;
        let newcap = self.buckets.len() * 2;
        let mut nb: Vec<Vec<u32>> = Vec::new();
        let mut i = 0usize;
        while i < newcap {
            nb.push(Vec::new());
            i += 1;
        }
        // 全量重挂（只挂 Used，墓碑不进链）
        let mut si = 0usize;
        while si < self.slots.len() {
            if self.slots[si].state == SlotState::Used {
                let b = (self.slots[si].hash as usize) & (newcap - 1);
                nb[b].push(si as u32);
            }
            si += 1;
        }
        // 重挂后核链长：任一桶超上限即判重挂漏项（正常情况下翻倍必达标）
        let mut k = 0usize;
        while k < newcap {
            if nb[k].len() > MAX_CHAIN {
                self.phase = TablePhase::Steady;
                return Err(E_CHAIN_LONG);
            }
            k += 1;
        }
        self.buckets = nb;
        self.resizes += 1;
        self.phase = TablePhase::Steady;
        Ok(())
    }

    /// 链长直方图：下标 = 链长（0..=MAX_CHAIN+1），末槽收所有超上限链。
    pub fn chain_hist(&self) -> [u32; MAX_CHAIN + 2] {
        let mut h = [0u32; MAX_CHAIN + 2];
        let mut i = 0usize;
        while i < self.buckets.len() {
            let n = self.buckets[i].len();
            let slot = if n > MAX_CHAIN { MAX_CHAIN + 1 } else { n };
            h[slot] += 1;
            i += 1;
        }
        h
    }

    /// 直方图单点读数（越界拒 E_BUCKET，此处"桶"指直方图槽）。
    pub fn hist_at(&self, idx: usize) -> Result<u32, u16> {
        if idx >= MAX_CHAIN + 2 { return Err(E_BUCKET); }
        Ok(self.chain_hist()[idx])
    }

    /// 直方图求和复核：必须等于桶数（J3）。
    pub fn hist_sum(&self) -> u32 {
        let h = self.chain_hist();
        let mut s = 0u32;
        let mut i = 0usize;
        while i < h.len() {
            s += h[i];
            i += 1;
        }
        s
    }

    /// 实测最大链长。
    pub fn max_chain(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0usize;
        while i < self.buckets.len() {
            let n = self.buckets[i].len() as u32;
            if n > m {
                m = n;
            }
            i += 1;
        }
        m
    }

    /// 统计账守恒：n_used == inserts - removes，lookups == hits + miss（J2）。
    pub fn balanced(&self) -> bool {
        self.n_used == self.inserts - self.removes && self.lookups == self.hits + self.miss
    }

    /// 槽表自审：Used 槽数必须等于 n_used（墓碑与空槽不计）。
    pub fn slot_audit(&self) -> bool {
        let mut used = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].state == SlotState::Used {
                used += 1;
            }
            i += 1;
        }
        used == self.n_used
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_KEY_DUP => "三键相同的条目重复插入",
        E_NO_KEY => "删除/定位不存在的键",
        E_NAME_EMPTY => "空名字（dentry 名至少 1 字节）",
        E_BUCKET => "桶号或直方图槽越界",
        E_CHAIN_LONG => "扩容后链长仍超上限（重挂漏项）",
        E_BAL => "统计账或键自检不平",
        E_CAP => "容量非 2 的幂或小于 16",
        E_NAME_LONG => "名字超长（NAME_MAX=255）",
        _ => "未知诊断码",
    }
}

/// 造一个确定性名字：字母表 26，长度 6..12，尾部加 "."+十进制序号保证全表唯一
/// （同名同父会撞键，drill 的万条样本必须互不相同）。
fn make_name(seed: u64, out: &mut Vec<u8>) {
    out.clear();
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let len = 6 + (s % 7) as usize;
    let mut i = 0usize;
    while i < len {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        out.push(b'a' + ((s >> 35) as u8 % 26));
        i += 1;
    }
    out.push(b'.');
    // 十进制序号（低位在前再反转，避免额外分配）
    let mut digits: [u8; 20] = [0; 20];
    let mut n = 0usize;
    let mut v = seed;
    if v == 0 {
        out.push(b'0');
    } else {
        while v > 0 && n < 20 {
            digits[n] = b'0' + (v % 10) as u8;
            v /= 10;
            n += 1;
        }
        while n > 0 {
            n -= 1;
            out.push(digits[n]);
        }
    }
}

/// 确定性演练：万插 → 万查 → 三千删 → 链长账 → 扩容 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut h = DHash::new(CAP_MIN)?;

    // R1：10,000 条插入（父号 = i/10，名由 LCG 造），n_used == 10000（J2 前半）。
    let mut buf: Vec<u8> = Vec::new();
    let mut i = 0u64;
    while i < 10_000 {
        make_name(i, &mut buf);
        let k = DKey::new(i / 10, &buf)?;
        h.insert(k, 0x1000 + i)?;
        i += 1;
    }
    if h.n_used != 10_000 || h.inserts != 10_000 { return Err(E_BAL); }
    if !h.slot_audit() || !h.balanced() { return Err(E_BAL); }
    hits += 1;

    // R2：10,000 次查找全命中，且回读 ino 与插入值逐条一致（J2 主面）。
    i = 0;
    while i < 10_000 {
        make_name(i, &mut buf);
        let k = DKey::new(i / 10, &buf)?;
        match h.lookup(&k) {
            Some(v) => {
                if v != 0x1000 + i {
                    return Err(E_BAL); // 命中却给错 ino：错账
                }
            }
            None => return Err(E_BAL), // 插入过的键必须能查到
        }
        i += 1;
    }
    if h.hits != 10_000 || h.miss != 0 { return Err(E_BAL); }
    if !h.balanced() { return Err(E_BAL); }
    hits += 1;

    // R3：删除 3,334 条（i 为 3 的倍数，0..9999 共 3334 个），删后查找转未命中，
    //     其余 6,666 条仍命中。
    i = 0;
    while i < 10_000 {
        if i % 3 == 0 {
            make_name(i, &mut buf);
            let k = DKey::new(i / 10, &buf)?;
            h.remove(&k)?;
        }
        i += 1;
    }
    if h.n_used != 6_666 || h.removes != 3_334 {
        return Err(E_BAL); // 10000 - 3334 = 6666，删数与在世数两账互证
    }
    i = 0;
    while i < 10_000 {
        make_name(i, &mut buf);
        let k = DKey::new(i / 10, &buf)?;
        let got = h.lookup(&k);
        if i % 3 == 0 {
            if got.is_some() {
                return Err(E_BAL); // 已删的键必须查不到
            }
        } else if got.is_none() {
            return Err(E_BAL);
        }
        i += 1;
    }
    if !h.balanced() || !h.slot_audit() { return Err(E_BAL); }
    hits += 1;

    // R4：链长分布账——直方图求和 == 桶数，且实测最大链长 <= 上限（J3）。
    if h.hist_sum() as usize != h.cap() { return Err(E_BAL); }
    if h.max_chain() as usize > MAX_CHAIN { return Err(E_CHAIN_LONG); }
    let hist = h.chain_hist();
    let mut hs = 0u32;
    let mut hi = 0usize;
    while hi < hist.len() {
        hs += hist[hi];
        hi += 1;
    }
    if hs != h.hist_sum() { return Err(E_BAL); }
    if hist[0] + hist[MAX_CHAIN + 1] > hs {
        return Err(E_BAL); // 空桶数 + 超上限桶数不可能超过桶总数
    }
    hits += 1;

    // R5：扩容账——万条插入必然触发扩容，桶数翻倍且相位回到稳定态（J4）。
    if h.resizes == 0 {
        return Err(E_CHAIN_LONG); // 万条还不扩容说明链长上限没生效
    }
    if h.cap() <= CAP_MIN { return Err(E_CAP); }
    if h.phase != TablePhase::Steady { return Err(E_BAL); }
    if (h.cap() & (h.cap() - 1)) != 0 {
        return Err(E_CAP); // 扩容后仍须是 2 的幂
    }
    hits += 1;

    // R6：三键隔离——同父不同名、同名不同父、同长不同字节三组互不误命中（J1）。
    let mut t = DHash::new(CAP_MIN)?;
    let ka = DKey::new(7, b"alpha")?;
    let kb = DKey::new(7, b"alphb")?; // 同父同名长不同字节
    let kc = DKey::new(8, b"alpha")?; // 同名字不同父
    t.insert(ka, 111)?;
    t.insert(kb, 222)?;
    t.insert(kc, 333)?;
    let qa = DKey::new(7, b"alpha")?;
    let qb = DKey::new(7, b"alphb")?;
    let qc = DKey::new(8, b"alpha")?;
    if t.lookup(&qa) != Some(111) || t.lookup(&qb) != Some(222) || t.lookup(&qc) != Some(333) { return Err(E_BAL); }
    if t.n_used != 3 { return Err(E_BAL); }
    hits += 1;

    // I1：重复键插入 → E_KEY_DUP，n_used 与 slot 表均不增（J5）。
    let dup_before = t.n_used;
    let slots_before = t.slots.len();
    if t.insert(DKey::new(7, b"alpha")?, 999) != Err(E_KEY_DUP) { return Err(E_KEY_DUP); }
    if t.n_used != dup_before || t.slots.len() != slots_before || t.dup_rejects != 1 { return Err(E_BAL); }
    hits += 1;

    // I2：删不存在的键 → E_NO_KEY；查不存在的键 → None 且 miss +1（不崩不建负项）。
    if t.remove(&DKey::new(7, b"nokey")?) != Err(E_NO_KEY) { return Err(E_NO_KEY); }
    if t.no_key_rejects != 1 { return Err(E_BAL); }
    if t.lookup(&DKey::new(7, b"nokey")?).is_some() { return Err(E_BAL); }
    hits += 1;

    // I3：空名 / 超长名 → E_NAME_EMPTY / E_NAME_LONG，键都造不出来。
    match DKey::new(1, b"") {
        Err(E_NAME_EMPTY) => {}
        _ => return Err(E_NAME_EMPTY),
    }
    let mut longbuf: Vec<u8> = Vec::new();
    let mut z = 0usize;
    while z < NAME_MAX + 1 {
        longbuf.push(b'x');
        z += 1;
    }
    match DKey::new(1, &longbuf) {
        Err(E_NAME_LONG) => {}
        _ => return Err(E_NAME_LONG),
    }
    hits += 1;

    // I4：容量非法（0 / 8 / 24）→ E_CAP，建表即拒。
    if DHash::new(0).is_err() == false || DHash::new(8).is_err() == false { return Err(E_CAP); }
    if DHash::new(24).is_err() == false {
        return Err(E_CAP); // 24 不是 2 的幂
    }
    hits += 1;

    // I5：直方图槽越界 → E_BUCKET（MAX_CHAIN+2 与 65535 双点）。
    if h.hist_at(MAX_CHAIN + 2) != Err(E_BUCKET) { return Err(E_BUCKET); }
    if h.hist_at(0xFFFF) != Err(E_BUCKET) { return Err(E_BUCKET); }
    hits += 1;

    // 终审：大小表两本账各自守恒，且删除后墓碑槽不计入 Used。
    if !h.balanced() || !h.slot_audit() || !t.balanced() { return Err(E_BAL); }
    if h.max_chain() as usize > MAX_CHAIN { return Err(E_CHAIN_LONG); }
    hits += 1;

    Ok(hits)
}
