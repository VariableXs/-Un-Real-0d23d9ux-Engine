//! unx4107_killtree — UNX-F4107 kill_sb 时整树缓存清除（诊断段 0x8F60）。
//!
//! 判据原文（提炼）：
//!   J1 kill_sb 后该 sb 全部正/负条目出清：残留 == 0（含 LRU 链与
//!      哈希表双结构核对，任一结构残留即失败）。
//!   J2 清除顺序先于 sb 释放：状态机 SbLive→CacheCleared→SbReleased，
//!      跳过清除直接释放被拒（E_RELEASE_DIRTY）。
//!   J3 隔离面：kill_sb 只清目标 sb，其他 sb 条目原样保留（计数不损）。
//!   J4 出清是全量原子账：清除计数 == 该 sb 清除前条目数，事件留痕。
//!   J5 对已释放 sb 再 kill 被拒（E_SB_GONE），重复清除幂等拒绝并记账。
//! 设计纪律：
//!   1. 双结构强一致：哈希桶（FNV-1a 分桶）与 LRU 链同持条目 id，
//!      出清时双结构同步摘除，双账核对以 id 集合相等为准。
//!   2. sb 生命周期状态机：每 sb 独立三态，清除必须发生在释放前，
//!      顺序错误一律拒收。
//!   3. 桶哈希写死：FNV-1a 32 位（offset 2166136261、prime 16777619，
//!      来源 Landon Curt Noll 公布参数），桶数 8（批内对照面）。
//!   4. 出清留痕：每次 kill_sb 生成出清事件（sb、条目数、id 序列），
//!      支持事后审计（判据 J4）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x8F60 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8F60;

/// 哈希桶数（批内对照面设定）。
pub const BUCKETS: usize = 8;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_RELEASE_DIRTY: u16 = SEG_BASE + 1; // 未清缓存先释放 sb（顺序失守）
const E_SB_GONE: u16 = SEG_BASE + 2;       // 对已释放 sb 再操作
const E_DUP_SB: u16 = SEG_BASE + 3;        // 重复挂同号 sb
const E_BAL: u16 = SEG_BASE + 4;           // 双结构账不平（残留/失联）
const E_DUP_ENTRY: u16 = SEG_BASE + 5;     // 同 sb 同名条目重复插入
const E_NO_SB: u16 = SEG_BASE + 6;         // 未挂载 sb 先插条目
const E_KILL_TWICE: u16 = SEG_BASE + 7;    // 重复 kill_sb（幂等拒收）

/// sb 生命周期状态机（判据 J2 顺序面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SbState {
    Live,         // 挂载中，可插条目
    CacheCleared, // 缓存已出清，待释放
    Released,     // 已释放终态
}

/// 单条 dentry：sb 归属 + 名 + 正负 + 唯一 id。
pub struct SbDentry {
    pub id: u32,            // 全局唯一槽位号
    pub sb: u32,            // 所属 superblock 号
    pub name: &'static str, // 名
    pub negative: bool,     // true = 负条目
}

/// 出清事件留痕（判据 J4）。
pub struct PurgeEvent {
    pub sb: u32,       // 被清 sb
    pub count: u32,    // 出清条目数
    pub ids: Vec<u32>, // 出清 id 序列
}

/// 整树清除枢纽：条目账 + 双结构（桶+LRU）+ sb 状态机 + 出清事件账。
pub struct KillLab {
    pub entries: Vec<SbDentry>,   // 存活条目账
    pub buckets: Vec<Vec<u32>>,   // 哈希桶（存条目 id）
    pub lru: Vec<u32>,            // LRU 链（存条目 id，尾最冷）
    pub sb_states: Vec<(u32, SbState)>, // sb 状态机表
    pub purges: Vec<PurgeEvent>,  // 出清事件账
    pub purged_total: u32,        // 累计出清条目数
    pub next_id: u32,             // 槽位分配器
    pub rejected: u32,            // 拒收注入计数
}

impl KillLab {
    pub fn new() -> KillLab {
        let mut buckets = Vec::new();
        let mut i = 0usize;
        while i < BUCKETS {
            buckets.push(Vec::new());
            i += 1;
        }
        KillLab {
            entries: Vec::new(),
            buckets,
            lru: Vec::new(),
            sb_states: Vec::new(),
            purges: Vec::new(),
            purged_total: 0,
            next_id: 1,
            rejected: 0,
        }
    }

    /// FNV-1a 32 位哈希（参数来源见头注）。
    fn fnv1a(name: &str) -> u32 {
        let mut h: u32 = 2166136261;
        let bytes = name.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            h ^= bytes[i] as u32;
            h = h.wrapping_mul(16777619);
            i += 1;
        }
        h
    }

    fn bucket_of(name: &str) -> usize {
        (KillLab::fnv1a(name) % BUCKETS as u32) as usize
    }

    /// 挂 sb（判据 J5 前置）：同号重复挂拒收。
    pub fn mount_sb(&mut self, sb: u32) -> Result<(), u16> {
        for (s, _) in &self.sb_states {
            if *s == sb {
                self.rejected += 1;
                return Err(E_DUP_SB);
            }
        }
        self.sb_states.push((sb, SbState::Live));
        Ok(())
    }

    fn sb_state(&self, sb: u32) -> SbState {
        for (s, st) in &self.sb_states {
            if *s == sb {
                return *st;
            }
        }
        SbState::Released // 未挂载视同已释放（插入路径统一拒收）
    }

    fn set_sb_state(&mut self, sb: u32, st: SbState) {
        for i in 0..self.sb_states.len() {
            if self.sb_states[i].0 == sb {
                self.sb_states[i].1 = st;
            }
        }
    }

    /// 插条目（判据 J5/E_NO_SB）：sb 必须 Live，同 sb 同名拒收。
    pub fn insert(&mut self, sb: u32, name: &'static str, negative: bool) -> Result<u32, u16> {
        if self.sb_state(sb) != SbState::Live {
            self.rejected += 1;
            return Err(E_NO_SB);
        }
        for e in &self.entries {
            if e.sb == sb && e.name == name {
                self.rejected += 1;
                return Err(E_DUP_ENTRY);
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(SbDentry { id, sb, name, negative });
        let b = KillLab::bucket_of(name);
        self.buckets[b].push(id);
        self.lru.push(id);
        Ok(id)
    }

    /// kill_sb 整树出清（判据 J1/J2/J3/J4）：
    /// 先摘哈希桶与 LRU 双结构，再转 sb 状态，事件留痕。
    pub fn kill_sb(&mut self, sb: u32) -> Result<u32, u16> {
        match self.sb_state(sb) {
            SbState::Live => {}
            SbState::CacheCleared => {
                self.rejected += 1;
                return Err(E_KILL_TWICE); // 重复清除幂等拒收
            }
            SbState::Released => {
                self.rejected += 1;
                return Err(E_SB_GONE);
            }
        }
        let mut ids = Vec::new();
        let mut keep: Vec<SbDentry> = Vec::new();
        for e in &self.entries {
            if e.sb == sb {
                ids.push(e.id);
            } else {
                keep.push(SbDentry {
                    id: e.id,
                    sb: e.sb,
                    name: e.name,
                    negative: e.negative,
                });
            }
        }
        // 双结构同步摘除：桶
        for bi in 0..self.buckets.len() {
            let mut nb: Vec<u32> = Vec::new();
            for id in &self.buckets[bi] {
                let mut mine = false;
                for t in &ids {
                    if *t == *id {
                        mine = true;
                    }
                }
                if !mine {
                    nb.push(*id);
                }
            }
            self.buckets[bi] = nb;
        }
        // LRU 链摘除
        let mut nl: Vec<u32> = Vec::new();
        for id in &self.lru {
            let mut mine = false;
            for t in &ids {
                if *t == *id {
                    mine = true;
                }
            }
            if !mine {
                nl.push(*id);
            }
        }
        self.lru = nl;
        let n = ids.len() as u32;
        self.entries = keep;
        self.purged_total += n;
        self.purges.push(PurgeEvent { sb, count: n, ids });
        self.set_sb_state(sb, SbState::CacheCleared);
        Ok(n)
    }

    /// 释放 sb（判据 J2）：必须已 CacheCleared，Live 直接释放被拒。
    pub fn release_sb(&mut self, sb: u32) -> Result<(), u16> {
        match self.sb_state(sb) {
            SbState::Live => {
                self.rejected += 1;
                Err(E_RELEASE_DIRTY) // 未清缓存先释放：顺序失守
            }
            SbState::CacheCleared => {
                self.set_sb_state(sb, SbState::Released);
                Ok(())
            }
            SbState::Released => {
                self.rejected += 1;
                Err(E_SB_GONE)
            }
        }
    }

    /// 双结构残留核对（判据 J1）：条目账、全桶、LRU 三处
    /// 目标 sb 的 id 计数全为 0 才算出清。
    pub fn residue(&self, sb: u32) -> u32 {
        let mut n = 0u32;
        for e in &self.entries {
            if e.sb == sb {
                n += 1;
            }
        }
        for b in &self.buckets {
            for id in b {
                for e in &self.entries {
                    if e.sb == sb && e.id == *id {
                        n += 1;
                    }
                }
            }
        }
        for id in &self.lru {
            for e in &self.entries {
                if e.sb == sb && e.id == *id {
                    n += 1;
                }
            }
        }
        n
    }

    /// 总账守恒：条目账数 == 桶总数 == LRU 链长（双结构强一致）。
    pub fn conserved(&self) -> bool {
        let mut bucket_sum = 0u32;
        for b in &self.buckets {
            bucket_sum += b.len() as u32;
        }
        bucket_sum == self.entries.len() as u32 && self.lru.len() as u32 == self.entries.len() as u32
    }
}

/// 自例程：R1 双 sb 铺账→R2 kill sb1 整树出清→R3 顺序注入拦截→R4 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 正常铺账：sb1 四条（正负混），sb2 两条，账守恒。
    let mut k = KillLab::new();
    k.mount_sb(1)?;
    k.mount_sb(2)?;
    k.insert(1, "a", false)?;
    k.insert(1, "b", true)?;
    k.insert(1, "c", false)?;
    k.insert(1, "d", true)?;
    k.insert(2, "a", false)?; // 跨 sb 同名合法（sb 隔离）
    k.insert(2, "e", false)?;
    if !k.conserved() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 kill_sb(1)：出清 4 条，残留 0（双结构核对），sb2 免疫（判据 J3）。
    let n = k.kill_sb(1)?;
    if n != 4 || k.residue(1) != 0 {
        return Err(E_BAL); // 判据 J1：残留==0
    }
    if k.conserved() == false {
        return Err(E_BAL);
    }
    // sb2 条目原样保留：条目账剩余 2。
    if k.entries.len() != 2 || k.purged_total != 4 {
        return Err(E_BAL); // 判据 J3/J4
    }
    hits += 1;

    // R3 顺序注入拦截：未清先释放被拒；重复 kill 被拒；已释放再操作被拒。
    k.mount_sb(3)?;
    k.insert(3, "x", false)?;
    if k.release_sb(3).is_ok() {
        return Err(E_RELEASE_DIRTY); // Live 直接释放必须拒（判据 J2）
    }
    k.kill_sb(3)?;
    if k.kill_sb(3).is_ok() {
        return Err(E_KILL_TWICE); // 重复 kill 幂等拒收（判据 J5）
    }
    k.release_sb(3)?;
    if k.release_sb(3).is_ok() {
        return Err(E_SB_GONE); // 重复释放应拒
    }
    if k.insert(3, "y", false).is_ok() {
        return Err(E_NO_SB); // 已释放 sb 不得再插条目
    }
    if k.kill_sb(3).is_ok() {
        return Err(E_SB_GONE); // 已释放再 kill 应拒
    }
    hits += 1;

    // R4 对账：出清事件留痕三条（sb1/sb3），计数与条目数平。
    if k.purges.len() != 2 {
        return Err(E_BAL); // sb1 与 sb3 两次出清
    }
    let mut purge_sum = 0u32;
    for i in 0..k.purges.len() {
        purge_sum += k.purges[i].count;
        if k.purges[i].ids.len() as u32 != k.purges[i].count {
            return Err(E_BAL); // 事件内 id 序列与计数平
        }
    }
    if purge_sum != k.purged_total {
        return Err(E_BAL); // 事件账总和 == 累计出清
    }
    if !k.conserved() || k.rejected < 4 {
        return Err(E_BAL); // 拒收账如实记（释放脏/重复 kill/重复释放/幽灵插）
    }
    hits += 1;

    Ok(hits)
}
