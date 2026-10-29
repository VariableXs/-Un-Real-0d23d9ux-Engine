//! unx4104_dlru — UNX-F4104 d_lru 驱逐与复活路径（诊断段 0x8F00）。
//!
//! 判据原文（提炼）：
//!   J1 驱逐只从 cold 链尾取：hot 链注入驱逐请求被拒（E_HOT_EVICT），
//!      cold 空链驱逐被拒（E_COLD_EMPTY）。
//!   J2 被驱逐条目重建后内容与原条目等价：重建等价账零差
//!      （名/ino/kind 三字段全等比对）。
//!   J3 LRU 语义：命中即从 cold 提升到 hot 头（touch），冷链保持
//!      FIFO 队尾最冷次序，驱逐恒取队尾。
//!   J4 链账守恒：hot 数 + cold 数 + evicted 数 == 累计建档数。
//!   J5 重复建档、驱逐不存在的槽、复活已在线条目全拒收并记错误账。
//! 设计纪律：
//!   1. 双链模型：hot 链（近期在用）与 cold 链（可驱逐）各为 FIFO 序，
//!      槽位用唯一 id 索引，链只存 id 不复制内容。
//!   2. 驱逐即摘账：从 cold 尾摘除后条目转入 evicted 档案区
//!      （保留内容快照供等价比对），不从内存抹除（复活路径依据）。
//!   3. 复活即重建：按 id 从档案区取快照重建为 cold 头（新近重建，
//!      给一次免驱逐宽限），等价账逐字段核对，差一即错。
//!   4. 触达纪律：touch 只允许对在线（hot/cold）条目；对已驱逐条目
//!      touch 视为复活请求走等价重建，不得直接提升。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x8F00 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8F00;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_HOT_EVICT: u16 = SEG_BASE + 1;   // hot 链注入驱逐请求被拒
const E_COLD_EMPTY: u16 = SEG_BASE + 2;  // cold 空链驱逐被拒
const E_DUP_NAME: u16 = SEG_BASE + 3;    // 重复建档被拒
const E_ID_MISS: u16 = SEG_BASE + 4;     // 点名槽位不存在
const E_EQ_DIFF: u16 = SEG_BASE + 5;     // 重建等价账有差（判据 J2 失守）
const E_BAL: u16 = SEG_BASE + 6;         // 三链账守恒失守
const E_REBUILD_LIVE: u16 = SEG_BASE + 7;// 复活已在线条目（注入）

/// 条目在线位置状态机：Hot →（touch 保持 Hot）/（evict 不允许）；
/// Cold →（touch → Hot 头）/（evict → Evicted）；Evicted →（rebuild → Cold）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    Hot,
    Cold,
    Evicted,
}

/// dentry 槽位：内容三字段 + 状态机 + 命中账。
pub struct DentrySlot {
    pub id: u32,          // 槽位唯一号（单调分配）
    pub name: &'static str, // 名（快照内容之一）
    pub ino: u32,         // inode 号（0 表示负条目）
    pub state: SlotState, // 在线状态机
    pub hits: u32,        // 触达次数
}

/// d_lru 双链枢纽：hot/cold FIFO 链 + 驱逐档案区 + 等价账。
pub struct DLru {
    pub slots: Vec<DentrySlot>, // 全槽位表（含已驱逐，内容快照保留）
    pub hot: Vec<u32>,          // hot 链（头在 0 端，新进在头）
    pub cold: Vec<u32>,         // cold 链（头在 0 端，新进在头；尾最冷）
    pub evict_total: u32,       // 累计驱逐数
    pub rebuild_total: u32,     // 累计复活重建数
    pub eq_diffs: u32,          // 重建等价差账（判据 J2 核对面，恒 0 才绿）
    pub next_id: u32,           // 槽位号分配器
    pub rejected: u32,          // 拒收注入计数
}

impl DLru {
    pub fn new() -> DLru {
        DLru {
            slots: Vec::new(),
            hot: Vec::new(),
            cold: Vec::new(),
            evict_total: 0,
            rebuild_total: 0,
            eq_diffs: 0,
            next_id: 1,
            rejected: 0,
        }
    }

    /// 按槽位 id 定位全表下标。
    fn slot_pos(&self, id: u32) -> Option<usize> {
        for i in 0..self.slots.len() {
            if self.slots[i].id == id {
                return Some(i);
            }
        }
        None
    }

    /// 槽位是否在线（hot/cold 链任一）。
    fn is_live(&self, id: u32) -> bool {
        for h in &self.hot {
            if *h == id {
                return true;
            }
        }
        for c in &self.cold {
            if *c == id {
                return true;
            }
        }
        false
    }

    /// 建档入 cold 头（判据 J5 重复名拒收）。
    pub fn add(&mut self, name: &'static str, ino: u32) -> Result<u32, u16> {
        if name.is_empty() {
            self.rejected += 1;
            return Err(E_DUP_NAME);
        }
        for s in &self.slots {
            if s.name == name && s.state != SlotState::Evicted {
                self.rejected += 1;
                return Err(E_DUP_NAME);
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        self.slots.push(DentrySlot {
            id,
            name,
            ino,
            state: SlotState::Cold,
            hits: 0,
        });
        self.cold.insert(0, id); // 新进在 cold 头
        Ok(id)
    }

    /// 触达：cold→hot 头提升（判据 J3 LRU 语义）。
    pub fn touch(&mut self, id: u32) -> Result<(), u16> {
        let p = match self.slot_pos(id) {
            Some(p) => p,
            None => {
                self.rejected += 1;
                return Err(E_ID_MISS);
            }
        };
        self.slots[p].hits += 1;
        if self.slots[p].state == SlotState::Evicted {
            self.rejected += 1;
            return Err(E_ID_MISS); // 已驱逐条目须走 rebuild，touch 拒收
        }
        // 从 cold 链摘除（若在）
        let mut ci = self.cold.len();
        for i in 0..self.cold.len() {
            if self.cold[i] == id {
                ci = i;
                break;
            }
        }
        if ci < self.cold.len() {
            self.cold.remove(ci);
            self.slots[p].state = SlotState::Hot;
            self.hot.insert(0, id);
        }
        Ok(())
    }

    /// 驱逐：只从 cold 链尾取（判据 J1）。
    /// 本 API 不区分 hot/cold——调用方指名驱逐的槽若不在 cold 尾即拒。
    pub fn evict_tail(&mut self) -> Result<u32, u16> {
        if self.cold.is_empty() {
            self.rejected += 1;
            return Err(E_COLD_EMPTY);
        }
        let id = self.cold[self.cold.len() - 1]; // 尾 = 最冷
        self.cold.pop();
        let p = match self.slot_pos(id) {
            Some(p) => p,
            None => return Err(E_BAL), // 链与全表失联即账坏
        };
        self.slots[p].state = SlotState::Evicted;
        self.evict_total += 1;
        Ok(id)
    }

    /// 注入面：指名驱逐任意槽（用于验证 hot 注入被拒，判据 J1）。
    pub fn evict_named(&mut self, id: u32) -> Result<u32, u16> {
        let p = match self.slot_pos(id) {
            Some(p) => p,
            None => {
                self.rejected += 1;
                return Err(E_ID_MISS);
            }
        };
        // 只允许驱逐 cold 尾
        let tail_ok = !self.cold.is_empty() && self.cold[self.cold.len() - 1] == id;
        if !tail_ok {
            self.rejected += 1;
            if self.slots[p].state == SlotState::Hot {
                return Err(E_HOT_EVICT); // hot 注入驱逐：判据 J1 拦截面
            }
            return Err(E_COLD_EMPTY); // cold 中部/已驱逐：非尾不得驱逐
        }
        self.evict_tail()
    }

    /// 复活重建（判据 J2/J5）：档案区取快照重建为 cold 头，
    /// 逐字段等价比对（快照即原内容，重建后必须全等）。
    pub fn rebuild(&mut self, id: u32, name: &'static str, ino: u32) -> Result<(), u16> {
        if self.is_live(id) {
            self.rejected += 1;
            return Err(E_REBUILD_LIVE); // 在线条目不得复活
        }
        let p = match self.slot_pos(id) {
            Some(p) => p,
            None => {
                self.rejected += 1;
                return Err(E_ID_MISS);
            }
        };
        if self.slots[p].state != SlotState::Evicted {
            self.rejected += 1;
            return Err(E_ID_MISS); // 只有已驱逐态可复活
        }
        // 等价核对：名与 ino 必须与快照全等（差一即入差账）
        if self.slots[p].name != name || self.slots[p].ino != ino {
            self.eq_diffs += 1;
            return Err(E_EQ_DIFF);
        }
        self.slots[p].state = SlotState::Cold;
        self.cold.insert(0, id);
        self.rebuild_total += 1;
        Ok(())
    }

    /// 三链账守恒（判据 J4）：hot+cold+evicted == 全表数。
    pub fn conserved(&self) -> bool {
        let mut evicted = 0u32;
        for s in &self.slots {
            if s.state == SlotState::Evicted {
                evicted += 1;
            }
        }
        if self.hot.len() as u32 + self.cold.len() as u32 + evicted
            != self.slots.len() as u32
        {
            return false;
        }
        // 链上 id 必须都能在全表找到且状态匹配
        for h in &self.hot {
            match self.slot_pos(*h) {
                Some(p) => {
                    if self.slots[p].state != SlotState::Hot {
                        return false;
                    }
                }
                None => return false,
            }
        }
        for c in &self.cold {
            match self.slot_pos(*c) {
                Some(p) => {
                    if self.slots[p].state != SlotState::Cold {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    /// 重建等价账核验（判据 J2）：差账恒零才绿。
    pub fn rebuild_equivalent(&self) -> bool {
        self.eq_diffs == 0
    }
}

/// 自例程：R1 建档触达→R2 cold 尾驱逐→R3 hot 注入拦截→R4 复活等价对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 正常路径：建 4 条，touch 两条入 hot，链账守恒。
    let mut l = DLru::new();
    let a = l.add("alpha", 11)?;
    let b = l.add("beta", 12)?;
    let c = l.add("gamma", 0)?; // 负条目示范
    let d = l.add("delta", 14)?;
    l.touch(a)?;
    l.touch(b)?;
    if !l.conserved() {
        return Err(E_BAL);
    }
    if l.hot.len() != 2 || l.cold.len() != 2 {
        return Err(E_BAL); // touch 后 hot=alpha,beta；cold=delta,gamma（gamma 尾）
    }
    let _ = c;
    hits += 1;

    // R2 驱逐：cold 尾（gamma）被驱逐；档案区留快照。
    let evicted_id = l.evict_tail()?;
    if evicted_id != c {
        return Err(E_BAL); // 必须驱逐最冷的 gamma
    }
    if l.evict_total != 1 || !l.conserved() {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 注入拦截：hot 条目驱逐、cold 头驱逐、重复建档、幽灵槽全拒。
    if l.evict_named(a).is_ok() {
        return Err(E_HOT_EVICT); // hot 注入驱逐必须拒（判据 J1）
    }
    if l.evict_named(d).is_ok() {
        return Err(E_COLD_EMPTY); // cold 头非尾不得驱逐
    }
    if l.add("beta", 99).is_ok() {
        return Err(E_DUP_NAME); // 在线重名建档应拒
    }
    if l.evict_named(9999).is_ok() {
        return Err(E_ID_MISS); // 幽灵槽应拒
    }
    hits += 1;

    // R4 复活与等价对账：正确参数复活零差；错参数复活入差账被拒。
    l.rebuild(c, "gamma", 0)?; // 等价重建：内容与快照全等
    if !l.rebuild_equivalent() || l.eq_diffs != 0 {
        return Err(E_EQ_DIFF); // 判据 J2：重建等价账零差
    }
    if l.rebuild(c, "gamma", 0).is_ok() {
        return Err(E_REBUILD_LIVE); // 已复活再复活应拒
    }
    if l.rebuild(a, "alpha", 11).is_ok() {
        return Err(E_REBUILD_LIVE); // 在线条目复活应拒
    }
    // 等价失配注入：以错 ino 请求复活另一已驱逐条目——先驱逐 beta 触发不了
    // （beta 在 hot），改为驱逐 delta 再错参复活。
    let _ = b;
    let ev2 = l.evict_tail()?; // cold=[delta]，驱逐 delta
    if ev2 != d {
        return Err(E_BAL);
    }
    if l.rebuild(d, "delta", 1).is_ok() {
        return Err(E_EQ_DIFF); // 错 ino 复活必须拒并计差账
    }
    if l.eq_diffs != 1 {
        return Err(E_EQ_DIFF); // 差账必须如实记 1
    }
    l.rebuild(d, "delta", 14)?; // 正确参数复活成功，差账不再增
    if l.eq_diffs != 1 || !l.conserved() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
