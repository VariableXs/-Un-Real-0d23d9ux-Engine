//! unx4105_evictquota — UNX-F4105 缓存驱逐上限与配额（诊断段 0x8F20）。
//!
//! 判据原文（提炼）：
//!   J1 配额耗尽触发驱逐且驱逐量 == 超额量：单事件账 evicted_this_round
//!      与超额数（驻留+新入-配额）逐事件相等。
//!   J2 配额可配置：set_quota 改参后行为随动（提额不驱逐，降额补驱逐）。
//!   J3 零配额极端不崩：quota==0 时任何插入拒收（E_QUOTA_ZERO 语义下
//!      不产生驱逐事件、不崩、账面守恒）。
//!   J4 驱逐按最冷优先：同轮驱逐从驻留序尾取（先入先出的可驱逐序），
//!      驱逐对象 id 全程留档可审计。
//!   J5 总账守恒：累计入账 == 驻留数 + 累计驱逐数，任何时刻成立。
//! 设计纪律：
//!   1. 配额参数写死初值：DEFAULT_QUOTA = 8（批内对照面），
//!      set_quota 可改但拒绝超上限 QUOTA_CAP = 1024（防失控）。
//!   2. 驻留序即可驱逐序：Vec 尾为最冷（先入者先驱逐），
//!      命中触达移到头（刷新热度），驱逐恒从尾取。
//!   3. 驱逐量精确制导：只驱逐恰好超额数，不整批清扫（判据 J1 的
//!      "量=超额量" 语义），驱逐明细入事件账。
//!   4. 降额补驱逐：set_quota 降额时立即驱逐到新配额以内，
//!      补驱逐量同样入事件账可对账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x8F20 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8F20;

/// 默认配额（批内对照面初值，来源：施工单判据对照设定）。
pub const DEFAULT_QUOTA: u32 = 8;
/// 配额上限（防失控注入防线）。
pub const QUOTA_CAP: u32 = 1024;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_QUOTA_CAP: u16 = SEG_BASE + 1;  // 配额超上限拒收
const E_QUOTA_ZERO: u16 = SEG_BASE + 2; // 零配额插入拒收（极端面，不崩）
const E_DUP_NAME: u16 = SEG_BASE + 3;   // 重复名插入拒收
const E_BAL: u16 = SEG_BASE + 4;        // 总账守恒失守（入账≠驻留+驱逐）
const E_EVICT_AMT: u16 = SEG_BASE + 5;  // 驱逐量≠超额量（判据 J1 失守）
const E_EVICT_OVER: u16 = SEG_BASE + 6; // 驻留超配额（驱逐失职）
const E_ID_MISS: u16 = SEG_BASE + 7;    // 点名条目不在驻留

/// 驱逐事件账：一次驱逐动作的完整留痕。
pub struct EvictEvent {
    pub ids: Vec<u32>,  // 本轮被驱逐的槽位 id（按驱逐序）
    pub overflow: u32,  // 本轮超额量（判据 J1 对账键）
}

/// 单条缓存驻留项。
pub struct QuotaEntry {
    pub id: u32,            // 槽位唯一号
    pub name: &'static str, // 名
    pub ino: u32,           // inode 号（0 为负条目）
    pub hits: u32,          // 触达次数
}

/// 配额缓存枢纽：驻留序 + 配额参数 + 驱逐事件账 + 总账。
pub struct QuotaCache {
    pub resident: Vec<QuotaEntry>, // 驻留序（尾最冷）
    pub quota: u32,                // 当前配额（可配置，判据 J2）
    pub quota_changes: u32,        // 配额变更次数（改参随动审计）
    pub inserted_total: u32,       // 累计入账数
    pub evicted_total: u32,        // 累计驱逐数
    pub events: Vec<EvictEvent>,   // 驱逐事件账（判据 J1 留痕）
    pub next_id: u32,              // 槽位分配器
    pub rejected: u32,             // 拒收注入计数
}

impl QuotaCache {
    pub fn new() -> QuotaCache {
        QuotaCache {
            resident: Vec::new(),
            quota: DEFAULT_QUOTA,
            quota_changes: 0,
            inserted_total: 0,
            evicted_total: 0,
            events: Vec::new(),
            next_id: 1,
            rejected: 0,
        }
    }

    /// 驻留定位。
    fn pos(&self, id: u32) -> Option<usize> {
        for i in 0..self.resident.len() {
            if self.resident[i].id == id {
                return Some(i);
            }
        }
        None
    }

    /// 驻留定位（公开审计面）。
    pub fn pos_of(&self, id: u32) -> Option<usize> {
        self.pos(id)
    }

    /// 改配额（判据 J2）：超上限拒收；降额立即补驱逐到新配额以内。
    pub fn set_quota(&mut self, q: u32) -> Result<(), u16> {
        if q > QUOTA_CAP {
            self.rejected += 1;
            return Err(E_QUOTA_CAP);
        }
        self.quota = q;
        self.quota_changes += 1;
        if (self.resident.len() as u32) > q {
            // 降额补驱逐：驱逐量 = 当前驻留 - 新配额（超额量语义同 J1）
            let over = self.resident.len() as u32 - q;
            self.evict_amount(over)?;
        }
        Ok(())
    }

    /// 内部驱逐 exactly n 条（从尾取最冷），事件入账。
    fn evict_amount(&mut self, n: u32) -> Result<(), u16> {
        if n as usize > self.resident.len() {
            return Err(E_EVICT_AMT); // 驱逐量超过驻留即账坏
        }
        let mut ids = Vec::new();
        let mut k = 0u32;
        while k < n {
            match self.resident.pop() {
                Some(e) => ids.push(e.id),
                None => return Err(E_EVICT_AMT),
            }
            k += 1;
        }
        self.evicted_total += n;
        self.events.push(EvictEvent { ids, overflow: n });
        Ok(())
    }

    /// 插入（判据 J1/J3）：超额即驱逐恰好超额数；零配额拒收不崩。
    pub fn insert(&mut self, name: &'static str, ino: u32) -> Result<u32, u16> {
        if name.is_empty() {
            self.rejected += 1;
            return Err(E_DUP_NAME);
        }
        for e in &self.resident {
            if e.name == name {
                self.rejected += 1;
                return Err(E_DUP_NAME);
            }
        }
        if self.quota == 0 {
            self.rejected += 1;
            return Err(E_QUOTA_ZERO); // 零配额极端：拒收不崩，无驱逐事件
        }
        let id = self.next_id;
        self.next_id += 1;
        // 新入插队头（最新最热），驱逐恒从尾取最冷
        self.resident.insert(0, QuotaEntry {
            id,
            name,
            ino,
            hits: 0,
        });
        self.inserted_total += 1;
        // 超额驱逐：超额量 = 驻留 - 配额（判据 J1 量的语义）
        let over = self.resident.len() as u32 - self.quota;
        if over > 0 {
            self.evict_amount(over)?;
        }
        Ok(id)
    }

    /// 触达：移到驻留序头（刷新热度，最不易被驱逐）。
    pub fn touch(&mut self, id: u32) -> Result<(), u16> {
        let p = match self.pos(id) {
            Some(p) => p,
            None => {
                self.rejected += 1;
                return Err(E_ID_MISS);
            }
        };
        self.resident[p].hits += 1;
        let e = self.resident.remove(p);
        self.resident.insert(0, e);
        Ok(())
    }

    /// 判据 J1 核验：每个驱逐事件驱逐量 == 超额量。
    pub fn evict_amounts_exact(&self) -> bool {
        let mut i = 0usize;
        while i < self.events.len() {
            let ev = &self.events[i];
            if ev.ids.len() as u32 != ev.overflow || ev.overflow == 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 总账守恒（判据 J5）与配额不破（判据 J1 推论）。
    pub fn conserved(&self) -> bool {
        if self.inserted_total
            != self.resident.len() as u32 + self.evicted_total
        {
            return false;
        }
        if (self.resident.len() as u32) > self.quota {
            return false; // 驻留超配额即驱逐失职
        }
        self.evict_amounts_exact()
    }

    /// 驻留数与驱逐数快照（对照面）。
    pub fn snapshot(&self) -> (u32, u32, u32) {
        (self.resident.len() as u32, self.evicted_total, self.quota)
    }

    /// 全量清池（审计面）：驻留全部驱逐入事件账（超额量语义保留），
    /// 返回本轮驱逐量；空池清池返回 0 不出错。
    pub fn drain(&mut self) -> Result<u32, u16> {
        let n = self.resident.len() as u32;
        if n == 0 {
            return Ok(0); // 空池清池幂等
        }
        let prev = self.evicted_total;
        self.evict_amount(n)?;
        Ok(self.evicted_total - prev) // 本轮驱逐量 = 账本差分
    }

    /// 按名查询驻留（审计面）：返回槽位号；不在驻留返回 E_ID_MISS。
    pub fn find_by_name(&self, name: &str) -> Result<u32, u16> {
        for e in &self.resident {
            if e.name == name {
                return Ok(e.id);
            }
        }
        Err(E_ID_MISS)
    }

    /// 事件账汇总：驱逐事件数与事件内驱逐总量（对账面）。
    /// 事件总量必须 == evicted_total（判据 J5 推论）。
    pub fn event_summary(&self) -> (u32, u32) {
        let mut cnt = 0u32;
        let mut sum = 0u32;
        for i in 0..self.events.len() {
            cnt += 1;
            sum += self.events[i].overflow;
        }
        (cnt, sum)
    }

    /// 驻留名清单导出（驻留序头→尾）。
    pub fn resident_names(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        for i in 0..self.resident.len() {
            v.push(self.resident[i].name);
        }
        v
    }
}

/// 自例程：R1 配额内逐入→R2 超额触发精确驱逐→R3 改参随动→R4 极端与守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 正常路径：配额 4，逐入 4 条不驱逐。
    let mut q = QuotaCache::new();
    q.set_quota(4)?;
    let a = q.insert("f1", 1)?;
    let b = q.insert("f2", 2)?;
    let c = q.insert("f3", 3)?;
    let d = q.insert("f4", 4)?;
    let (res, ev, _) = q.snapshot();
    if res != 4 || ev != 0 {
        return Err(E_BAL); // 配额内不驱逐
    }
    hits += 1;

    // R2 超额驱逐：第 5 条入账触发驱逐恰好 1（超额量 1，判据 J1）。
    q.touch(b)?; // b 刷热，最冷应是 a（队头插入序 f1 最先）
    let e = q.insert("f5", 5)?;
    if q.evicted_total != 1 {
        return Err(E_EVICT_AMT);
    }
    // 最冷为 a（未被 touch 且最早入账，居队尾被驱逐）
    if q.pos_of(a).is_some() {
        return Err(E_EVICT_AMT); // a 应已被驱逐
    }
    if q.pos_of(b).is_none() || q.pos_of(e).is_none() {
        return Err(E_BAL); // 刷热与新房客必须在驻留
    }
    hits += 1;

    // R3 改参随动（判据 J2）：提额不驱逐；降额补驱逐恰好差额。
    q.set_quota(8)?;
    if q.evicted_total != 1 {
        return Err(E_EVICT_AMT); // 提额不得引发驱逐
    }
    q.set_quota(2)?;
    // 驻留 4 → 降额到 2，补驱逐 2（超额量语义）
    if q.evicted_total != 3 || q.resident.len() != 2 {
        return Err(E_EVICT_AMT);
    }
    if q.quota_changes != 3 {
        return Err(E_BAL); // 改参账 3 次（4→8→2，含初始 set 4）
    }
    hits += 1;

    // R4 极端与注入：零配额清空驻留不崩、重复名拒、幽灵触达拒、守恒终审。
    q.set_quota(0)?;
    if q.insert("f9", 9).is_ok() {
        return Err(E_QUOTA_ZERO); // 零配额插入必须拒且不崩
    }
    if q.evicted_total != 5 || q.resident.len() != 0 {
        return Err(E_EVICT_AMT); // 零配额降额补驱逐到空，账面平
    }
    q.set_quota(2)?;
    let _ = q.insert("f6", 6)?;
    if q.insert("f6", 6).is_ok() {
        return Err(E_DUP_NAME); // 重复名应拒
    }
    if q.touch(9999).is_ok() {
        return Err(E_ID_MISS); // 幽灵触达应拒
    }
    if !q.conserved() {
        return Err(E_BAL); // 判据 J5 总账守恒终审
    }
    let _ = (c, d);
    hits += 1;

    // R5 审计面：按名查询、事件账汇总、驻留名导出、全量清池幂等。
    let _ = q.find_by_name("f6")?; // f6 在驻留（守恒终审后仍在）
    if q.find_by_name("f5").is_ok() {
        return Err(E_ID_MISS); // f5 已被驱逐不得在驻留
    }
    let (ec, es) = q.event_summary();
    if es != q.evicted_total {
        return Err(E_BAL); // 事件总量 == 累计驱逐
    }
    if ec == 0 {
        return Err(E_EVICT_AMT); // 本实验必有驱逐事件
    }
    let names = q.resident_names();
    if names.len() as u32 != q.resident.len() as u32 {
        return Err(E_BAL);
    }
    // 空池清池幂等：非空 drain 必驱逐；再 drain 返 0。
    let before = q.evicted_total;
    let dn = q.drain()?;
    if q.resident.len() != 0 {
        return Err(E_BAL); // 清池后驻留必须清零
    }
    if q.evicted_total - before != dn || dn == 0 {
        return Err(E_EVICT_AMT); // 非空清池驱逐量 == 返回值且非 0
    }
    let dn2 = q.drain()?;
    if dn2 != 0 {
        return Err(E_EVICT_AMT); // 空池清池幂等返 0
    }
    if !q.conserved() {
        return Err(E_BAL); // 清池后守恒仍绿（inserted == evicted）
    }
    hits += 1;

    Ok(hits)
}
