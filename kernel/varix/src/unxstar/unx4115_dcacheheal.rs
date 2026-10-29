//! unx4115_dcacheheal — UNX-F4115 缓存自愈（损坏检测重建）（诊断段 0x9060）。
//!
//! 判据原文（提炼）：
//!   J1 哈希表/链表损坏注入（毒化指针）被检测器检出：桶内出现
//!      非法 id / 失联 id / 重复 id 任一即检出并定位。
//!   J2 检出后整表重建：从条目正本账重建全部桶与链，重建账完整
//!      （重建事件留痕：条目数/桶分布）。
//!   J3 重建前后查找语义等价：同一查询集在重建前后的应答逐条相等。
//!   J4 哨兵校验：每条目携带校验和哨兵（名哈希低位），毒化内容
//!      被哨兵复核检出。
//!   J5 健康态零误报：无注入时周期检测恒绿（误报计数恒 0）。
//! 设计纪律：
//!   1. 正本与索引分离：条目正本账（Vec）为唯一事实源，
//!      桶索引与链索引皆为可重建缓存——这是自愈的立足点。
//!   2. 检测四查：桶内 id 越界、id 失联（不在正本）、id 重复、
//!      条目哨兵失配；任一命中即判红并定位。
//!   3. 重建即全量重放：以正本账为输入重算全部桶（FNV-1a 8 桶），
//!      重建后强制复检绿才许申报痊愈。
//!   4. FNV-1a 32 位写死（offset 2166136261 / prime 16777619，
//!      Noll 公布参数），哨兵取哈希低 16 位。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9060 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9060;

/// 哈希桶数。
pub const BUCKETS: usize = 8;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ID_ILLEGAL: u16 = SEG_BASE + 1; // 桶内 id 越界/失联（毒化检出）
const E_ID_DUP: u16 = SEG_BASE + 2;     // 桶内 id 重复（毒化检出）
const E_CANARY_BAD: u16 = SEG_BASE + 3; // 哨兵失配（内容毒化检出）
const E_REBUILD_DIFF: u16 = SEG_BASE + 4; // 重建后语义不等价（判据 J3）
const E_DUP_INS: u16 = SEG_BASE + 5;    // 重复插入同名
const E_NOT_FOUND: u16 = SEG_BASE + 6;  // 查询不存在（语义等价集的负样本）
const E_BAL: u16 = SEG_BASE + 7;        // 重建账不完整（判据 J2）

/// 单条目正本：id + 名 + ino + 哨兵（判据 J4）。
pub struct HealEntry {
    pub id: u32,            // 唯一号
    pub name: &'static str, // 名
    pub ino: u32,           // inode 号（0 = 负条目）
    pub canary: u16,        // 哨兵 = 名哈希低 16 位
}

/// 健康状态机：Healthy →（检出）Suspect →（重建复检绿）Healed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableHealth {
    Healthy,
    Suspect,
    Healed,
}

/// 自愈枢纽：正本账 + 桶索引 + 检测器 + 重建账。
pub struct HealTable {
    pub entries: Vec<HealEntry>,   // 条目正本账（唯一事实源）
    pub buckets: Vec<Vec<u32>>,    // 桶索引（可毒化可重建）
    pub health: TableHealth,       // 健康状态机
    pub detects: u32,              // 检出次数（判据 J1）
    pub rebuilds: u32,             // 重建次数（判据 J2）
    pub false_alarms: u32,         // 误报计数（判据 J5，恒 0 才绿）
    pub next_id: u32,              // id 分配器
}

impl HealTable {
    pub fn new() -> HealTable {
        let mut buckets = Vec::new();
        let mut i = 0usize;
        while i < BUCKETS {
            buckets.push(Vec::new());
            i += 1;
        }
        HealTable {
            entries: Vec::new(),
            buckets,
            health: TableHealth::Healthy,
            detects: 0,
            rebuilds: 0,
            false_alarms: 0,
            next_id: 1,
        }
    }

    /// FNV-1a 32 位（参数来源见头注）。
    fn fnv1a(name: &str) -> u32 {
        let mut h: u32 = 2166136261;
        let b = name.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            h ^= b[i] as u32;
            h = h.wrapping_mul(16777619);
            i += 1;
        }
        h
    }

    fn canary_of(name: &str) -> u16 {
        (HealTable::fnv1a(name) & 0xFFFF) as u16
    }

    fn bucket_of(name: &str) -> usize {
        (HealTable::fnv1a(name) % BUCKETS as u32) as usize
    }

    /// 插入（正本 + 索引同步）。
    pub fn insert(&mut self, name: &'static str, ino: u32) -> Result<u32, u16> {
        if name.is_empty() {
            return Err(E_DUP_INS);
        }
        for e in &self.entries {
            if e.name == name {
                return Err(E_DUP_INS);
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        let canary = HealTable::canary_of(name);
        self.entries.push(HealEntry { id, name, ino, canary });
        let b = HealTable::bucket_of(name);
        self.buckets[b].push(id);
        Ok(id)
    }

    /// 查找（判据 J3 语义载体）：先哨兵后桶索引。
    /// 哨兵失配时返回 E_CANARY_BAD（内容毒化可见）。
    pub fn lookup(&self, name: &str) -> Result<u32, u16> {
        let want = HealTable::canary_of(name);
        let b = HealTable::bucket_of(name);
        for id in &self.buckets[b] {
            // id 必须能在正本定位（失联即毒化）
            let mut found = false;
            for e in &self.entries {
                if e.id == *id {
                    found = true;
                    if e.name == name {
                        // 命中：哨兵复核后应答（内容毒化可见）
                        if e.canary != want {
                            return Err(E_CANARY_BAD);
                        }
                        return Ok(e.ino);
                    }
                }
            }
            if !found {
                return Err(E_ID_ILLEGAL); // 桶内失联 id
            }
        }
        Err(E_NOT_FOUND)
    }

    /// 周期检测器（判据 J1/J4/J5）：四查全过绿；
    /// 任一命中判红转 Suspect 并定位返回 Err。
    pub fn health_check(&mut self) -> Result<(), u16> {
        // 查 1/2/3：逐桶逐 id——越界、失联、重复。
        let mut bi = 0usize;
        while bi < self.buckets.len() {
            let mut i = 0usize;
            while i < self.buckets[bi].len() {
                let id = self.buckets[bi][i];
                // 重复查（同桶内重复即毒化）
                let mut dup = false;
                let mut k = 0usize;
                while k < self.buckets[bi].len() {
                    if k != i && self.buckets[bi][k] == id {
                        dup = true;
                    }
                    k += 1;
                }
                if dup {
                    self.health = TableHealth::Suspect;
                    self.detects += 1;
                    return Err(E_ID_DUP);
                }
                // 失联查：id 必须在正本且哨兵匹配其名
                let mut ok = false;
                for e in &self.entries {
                    if e.id == id {
                        ok = e.canary == HealTable::canary_of(e.name);
                    }
                }
                if !ok {
                    self.health = TableHealth::Suspect;
                    self.detects += 1;
                    return Err(E_ID_ILLEGAL);
                }
                // 归桶查：正本名的哈希必须落回本桶（桶毒化检出）
                for e in &self.entries {
                    if e.id == id && HealTable::bucket_of(e.name) != bi {
                        self.health = TableHealth::Suspect;
                        self.detects += 1;
                        return Err(E_ID_ILLEGAL);
                    }
                }
                i += 1;
            }
            bi += 1;
        }
        // 查 4：正本哨兵全表复核
        for e in &self.entries {
            if e.canary != HealTable::canary_of(e.name) {
                self.health = TableHealth::Suspect;
                self.detects += 1;
                return Err(E_CANARY_BAD);
            }
        }
        Ok(())
    }

    /// 整表重建（判据 J2/J3）：以正本账重放全部桶，
    /// 重建后强制复检绿；随后语义等价由 verify_equivalent 完成。
    pub fn rebuild(&mut self) -> Result<u32, u16> {
        let mut bi = 0usize;
        while bi < self.buckets.len() {
            self.buckets[bi].clear();
            bi += 1;
        }
        for e in &self.entries {
            let b = HealTable::bucket_of(e.name);
            self.buckets[b].push(e.id);
        }
        self.rebuilds += 1;
        // 重建账完整性：桶总数必须 == 正本数（判据 J2）
        let mut bucket_sum = 0u32;
        for b in &self.buckets {
            bucket_sum += b.len() as u32;
        }
        if bucket_sum != self.entries.len() as u32 {
            self.health = TableHealth::Suspect;
            return Err(E_BAL);
        }
        // 复检：绿才许转 Healed
        self.health_check()?;
        self.health = TableHealth::Healed;
        Ok(bucket_sum)
    }

    /// 语义等价核验（判据 J3）：查询集应答与正本逐条相等。
    /// 正本真值表驱动（查全集 = 全部条目名 + 一个不存在的负样本）。
    pub fn verify_equivalent(&self) -> Result<(), u16> {
        for e in &self.entries {
            match self.lookup(e.name) {
                Ok(ino) => {
                    if ino != e.ino {
                        return Err(E_REBUILD_DIFF);
                    }
                }
                Err(E_NOT_FOUND) => return Err(E_REBUILD_DIFF),
                Err(_) => return Err(E_CANARY_BAD),
            }
        }
        // 负样本：不存在的名必须 NOT_FOUND（语义面完备）
        match self.lookup("::no_such::") {
            Err(E_NOT_FOUND) => Ok(()),
            Ok(_) => Err(E_REBUILD_DIFF),
            Err(_) => Err(E_REBUILD_DIFF),
        }
    }

    /// 健康态查询（审计面）。
    pub fn health(&self) -> TableHealth {
        self.health
    }
}

/// 自例程：R1 建表健康→R2 毒化检出→R3 重建复绿等价→R4 注入与零误报。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 建表 4 条，健康检测绿，查找语义正确（判据 J5 零误报基线）。
    let mut t = HealTable::new();
    t.insert("a", 11)?;
    t.insert("b", 0)?;
    t.insert("c", 13)?;
    t.insert("d", 14)?;
    t.health_check()?;
    if t.health() != TableHealth::Healthy || t.false_alarms != 0 {
        return Err(E_BAL);
    }
    if t.lookup("a")? != 11 || t.lookup("b")? != 0 {
        return Err(E_REBUILD_DIFF);
    }
    if t.lookup("::no_such::") != Err(E_NOT_FOUND) {
        return Err(E_REBUILD_DIFF);
    }
    hits += 1;

    // R2 毒化注入：把 a 的 id 从正确桶搬到错误桶 → 检出定位（判据 J1）。
    let right = HealTable::bucket_of("a");
    let wrong = (right + 1) % BUCKETS;
    // 从正确桶摘 a 的 id，塞进错误桶（模拟桶指针毒化）
    let mut aid = 0u32;
    for e in &t.entries {
        if e.name == "a" {
            aid = e.id;
        }
    }
    let mut tmp: Vec<u32> = Vec::new();
    for id in &t.buckets[right] {
        if *id != aid {
            tmp.push(*id);
        }
    }
    t.buckets[right] = tmp;
    t.buckets[wrong].push(aid);
    if t.health_check().is_ok() {
        return Err(E_BAL); // 毒化后必须检出
    }
    if t.health() != TableHealth::Suspect {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 整表重建：桶重放 → 复检绿 → 语义等价（判据 J2/J3）。
    let n = t.rebuild()?;
    if n != 4 || t.health() != TableHealth::Healed {
        return Err(E_BAL); // 重建账完整且痊愈
    }
    t.verify_equivalent()?;
    hits += 1;

    // R4 注入面收口：重复插入拒、哨兵毒化检出、重建后再毒化再痊愈。
    if t.insert("a", 99).is_ok() {
        return Err(E_DUP_INS);
    }
    // 内容毒化：直接改 b 的哨兵（绕过 API 模拟内存翻位）
    for e in &mut t.entries {
        if e.name == "b" {
            e.canary = 0xBEEF;
        }
    }
    if t.health_check().is_ok() {
        return Err(E_CANARY_BAD); // 必须检出哨兵失配
    }
    if t.lookup("b").is_ok() {
        return Err(E_CANARY_BAD); // 毒化条目查找不得静默成功
    }
    // 修复哨兵后重建复绿（检出→定位→修复→复验闭环）
    for e in &mut t.entries {
        if e.name == "b" {
            e.canary = HealTable::canary_of("b");
        }
    }
    t.rebuild()?;
    t.verify_equivalent()?;
    if t.detects < 2 {
        return Err(E_BAL); // 两次毒化两次检出
    }
    hits += 1;

    Ok(hits)
}
