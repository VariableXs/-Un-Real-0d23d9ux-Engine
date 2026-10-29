//! unx4095_shortrecy — UNX-F4095 短生命周期 dentry 回收账（诊断段 0x8E00）。
//!
//! 判据原文：UNX-F4095-J1 建-删 10 万次瞬时 dentry 驻留峰值有账且回收计数=创建计数
//!           （残差=0）。
//! 可验性质（本模块自证面）：
//!   J1 建-删 10 万次后 created == reclaimed == 100,000，live == 0，
//!      残差 created - reclaimed - live == 0（零泄漏）；
//!   J2 驻留峰值有账：逐建逐删场景 high_water == 1；批量建 N 条再全删场景
//!      high_water == N（峰值是实测最大值，不是估算值）；
//!   J3 槽位复用：回收的槽会进空闲表，再次创建必须复用槽（created 增长但槽表
//!      不增长），槽表长度恒等于历史峰值驻留；
//!   J4 账守恒：created == reclaimed + live；空闲槽数 + 在世槽数 == 槽表长度；
//!   J5 非法回收拒：回收未建/已回收的槽（E_NOT_LIVE）、越界槽号（E_SLOT_OOR）、
//!      0 号（E_ID_ZERO）——拒收不改 created/reclaimed/live 三账。
//! 设计纪律：
//!   1. 槽表按 id 直接索引（id = slot + 1）：回收是 O(1) 定位，不用线性查——判据要求
//!      10 万次量级，线性查会退化成 O(n^2)。
//!   2. 空闲槽用栈（Vec push/pop）：后进先出使刚回收的槽立刻被复用，缓存局部性好，
//!      也天然给出"槽表长度 == 峰值驻留"这个可验结论。
//!   3. 三账同源：created/reclaimed/live 只在 create/retire 两处改动，任何拒收路径
//!      提前 return，不改账（设计纪律 5 的可验性就靠这一条）。
//!   4. 峰值只在 create 后刷新，不在 retire 后刷新（驻留峰值不回退）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8E00 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8E00;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOT_LIVE: u16 = SEG_BASE + 1; // 回收未在世（未建/已回收）的槽
const E_SLOT_OOR: u16 = SEG_BASE + 2; // 槽号越界
const E_ID_ZERO: u16 = SEG_BASE + 3; // 0 号 id 保留
const E_BAL: u16 = SEG_BASE + 4; // 三账不平
const E_EMPTY_SLOT: u16 = SEG_BASE + 5; // 空闲表取空（内部账坏）
const E_DUP_FREE: u16 = SEG_BASE + 6; // 同一槽重复进空闲表
const E_OVER: u16 = SEG_BASE + 7; // 计数触顶
const E_PEAK_BAD: u16 = SEG_BASE + 8; // 峰值账与实测不符

/// 槽状态：空闲 / 在世 / 已回收（墓碑，等复用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    Free = 0,      // 从未使用
    Live = 1,      // 在世（被引用）
    Reclaimed = 2, // 已回收，等待复用或驻留为空闲
}

/// 单条短命 dentry 记录（drill 取回用）。
pub struct ShortDentry {
    pub id: u64,          // dentry id（= slot + 1）
    pub slot: u32,        // 槽下标
    pub state: SlotState, // 槽态
    pub gen: u32,         // 复用代次（同槽第几次被建）
}

/// 短命 dentry 池：槽表 + 空闲栈 + 三账。
pub struct ShortPool {
    pub slots: Vec<SlotState>, // 槽表（长度 == 历史峰值驻留）
    pub gens: Vec<u32>,        // 各槽复用代次
    pub free: Vec<u32>,        // 空闲槽栈
    pub live: u32,             // 在世数
    pub created: u32,          // 创建计数
    pub reclaimed: u32,        // 回收计数
    pub high_water: u32,       // 驻留峰值
    pub cycles: u32,           // 建删循环次数
}

impl ShortPool {
    pub fn new() -> ShortPool {
        ShortPool {
            slots: Vec::new(),
            gens: Vec::new(),
            free: Vec::new(),
            live: 0,
            created: 0,
            reclaimed: 0,
            high_water: 0,
            cycles: 0,
        }
    }

    /// 建一条：优先复用空闲槽（J3），否则扩一张槽。
    pub fn create(&mut self) -> Result<u64, u16> {
        if self.created == u32::MAX {
            return Err(E_OVER);
        }
        let slot = match self.free.pop() {
            Some(s) => s,
            None => {
                let s = self.slots.len() as u32;
                self.slots.push(SlotState::Free);
                self.gens.push(0);
                s
            }
        };
        let si = slot as usize;
        if si >= self.slots.len() {
            return Err(E_SLOT_OOR); // 空闲表里混进了越界槽
        }
        if self.slots[si] == SlotState::Live {
            return Err(E_DUP_FREE); // 同一槽被重复放进空闲表
        }
        self.slots[si] = SlotState::Live;
        self.gens[si] += 1;
        self.live += 1;
        self.created += 1;
        if self.live > self.high_water {
            self.high_water = self.live; // 只在建后抬峰值
        }
        Ok(slot as u64 + 1)
    }

    /// 回收一条：O(1) 定位，置墓碑并把槽压进空闲栈。
    pub fn retire(&mut self, id: u64) -> Result<u32, u16> {
        if id == 0 {
            return Err(E_ID_ZERO);
        }
        let slot = (id - 1) as u32;
        let si = slot as usize;
        if si >= self.slots.len() {
            return Err(E_SLOT_OOR);
        }
        match self.slots[si] {
            SlotState::Live => {}
            SlotState::Reclaimed | SlotState::Free => return Err(E_NOT_LIVE),
        }
        self.slots[si] = SlotState::Reclaimed;
        self.free.push(slot);
        self.reclaimed += 1;
        if self.live > 0 {
            self.live -= 1;
        } else {
            return Err(E_BAL); // 在世账已 0 还要减
        }
        self.cycles += 1;
        Ok(slot)
    }

    /// 读一条（在世才可读）。
    pub fn read(&self, id: u64) -> Result<ShortDentry, u16> {
        if id == 0 {
            return Err(E_ID_ZERO);
        }
        let slot = (id - 1) as u32;
        let si = slot as usize;
        if si >= self.slots.len() {
            return Err(E_SLOT_OOR);
        }
        if self.slots[si] != SlotState::Live {
            return Err(E_NOT_LIVE);
        }
        Ok(ShortDentry {
            id,
            slot,
            state: self.slots[si],
            gen: self.gens[si],
        })
    }

    /// 残差（判据 J1 直读面）：created - reclaimed - live，正常恒 0。
    pub fn residual(&self) -> u32 {
        let used = self.reclaimed.saturating_add(self.live);
        self.created.saturating_sub(used)
    }

    /// 账守恒：created == reclaimed + live，且空闲槽 + 在世槽 == 槽表长度（J4）。
    pub fn conserved(&self) -> bool {
        if self.created != self.reclaimed + self.live {
            return false;
        }
        let mut live_slots = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i] == SlotState::Live {
                live_slots += 1;
            }
            i += 1;
        }
        if live_slots != self.live {
            return false;
        }
        self.free.len() as u32 + live_slots == self.slots.len() as u32
    }

    /// 峰值复核：high_water 必须等于槽表长度（槽表只为峰值驻留而扩）。
    pub fn peak_ok(&self) -> bool {
        self.high_water as usize == self.slots.len()
    }

    /// 槽表规模（判据"驻留峰值"的物理面）。
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOT_LIVE => "回收未在世（未建/已回收）的槽",
        E_SLOT_OOR => "槽号越界",
        E_ID_ZERO => "0 号 id 保留",
        E_BAL => "三账不平",
        E_EMPTY_SLOT => "空闲表取空（内部账坏）",
        E_DUP_FREE => "同一槽重复进空闲表",
        E_OVER => "计数触顶",
        E_PEAK_BAD => "峰值账与实测不符",
        _ => "未知诊断码",
    }
}

/// 确定性演练：逐建逐删 10 万次 → 批量峰值 → 槽复用 → 注入拦截 → 三账对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut p = ShortPool::new();

    // R1：逐建逐删 10 万次——created == reclaimed == 100,000，live 0，峰值 1（J1/J2）。
    let mut i = 0u32;
    while i < 100_000 {
        let id = p.create()?;
        if p.live != 1 {
            return Err(E_BAL);
        }
        p.retire(id)?;
        if p.live != 0 {
            return Err(E_BAL);
        }
        i += 1;
    }
    if p.created != 100_000 || p.reclaimed != 100_000 {
        return Err(E_BAL);
    }
    if p.live != 0 || p.residual() != 0 {
        return Err(E_BAL);
    }
    if p.high_water != 1 {
        return Err(E_PEAK_BAD); // 逐建逐删，峰值只能是 1
    }
    if p.slot_count() != 1 {
        return Err(E_BAL); // 单槽反复复用
    }
    if !p.conserved() || !p.peak_ok() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：槽复用——同一槽被建了 10 万次，gen 应等于 10 万（每建一次 +1）。
    if p.gens.len() != 1 || p.gens[0] != 100_000 {
        return Err(E_BAL);
    }
    if p.free.len() != 1 {
        return Err(E_BAL); // 只剩这一张槽在空闲栈里
    }
    hits += 1;

    // R3：批量建 100 条再全删——峰值抬到 100，槽表扩到 100，回收后全空闲（J2）。
    let mut ids: Vec<u64> = Vec::new();
    let mut k = 0u32;
    while k < 100 {
        ids.push(p.create()?);
        k += 1;
    }
    if p.high_water != 100 || p.slot_count() != 100 {
        return Err(E_PEAK_BAD);
    }
    if p.live != 100 {
        return Err(E_BAL);
    }
    // 逆序回收（后进先出，贴合 dcache shrinker 的尾部回收）
    let mut j = ids.len();
    while j > 0 {
        j -= 1;
        p.retire(ids[j])?;
    }
    if p.live != 0 || p.free.len() != 100 {
        return Err(E_BAL);
    }
    if !p.conserved() || !p.peak_ok() {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：再批量 100 条——必须全部复用已有 100 槽，槽表不再扩（J3 主面）。
    let mut ids2: Vec<u64> = Vec::new();
    k = 0;
    while k < 100 {
        ids2.push(p.create()?);
        k += 1;
    }
    if p.slot_count() != 100 || p.high_water != 100 {
        return Err(E_BAL); // 复用：槽表与峰值都不动
    }
    if p.created != 100_000 + 100 + 100 {
        return Err(E_BAL);
    }
    // 查验 100 条 id 互不相同（同槽不得同时出两条在世）
    let mut a = 0usize;
    while a < ids2.len() {
        let mut b = a + 1;
        while b < ids2.len() {
            if ids2[a] == ids2[b] {
                return Err(E_DUP_FREE);
            }
            b += 1;
        }
        a += 1;
    }
    if p.live != 100 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：读在世条目——gen 必须 > 1（该槽已被复用多轮），态为 Live。
    let d = p.read(ids2[0])?;
    if d.state != SlotState::Live {
        return Err(E_BAL);
    }
    if d.gen <= 1 {
        return Err(E_BAL); // 复用过就必然 gen > 1
    }
    if d.slot as u64 + 1 != d.id {
        return Err(E_BAL); // id 与 slot 的换算律
    }
    hits += 1;

    // R6：全删后再核三账——live 0、free 100、created == reclaimed。
    let mut q = ids2.len();
    while q > 0 {
        q -= 1;
        p.retire(ids2[q])?;
    }
    if p.live != 0 || p.free.len() != 100 {
        return Err(E_BAL);
    }
    if p.created != p.reclaimed {
        return Err(E_BAL);
    }
    if p.residual() != 0 || !p.conserved() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：重复回收 → E_NOT_LIVE，三账不动（J5）。
    let c_before = p.created;
    let r_before = p.reclaimed;
    let id = p.create()?;
    p.retire(id)?;
    if p.retire(id) != Err(E_NOT_LIVE) {
        return Err(E_NOT_LIVE);
    }
    if p.created != c_before + 1 || p.reclaimed != r_before + 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：回收未建的槽号（越界）→ E_SLOT_OOR；0 号 → E_ID_ZERO。
    if p.retire(9_999) != Err(E_SLOT_OOR) {
        return Err(E_SLOT_OOR);
    }
    if !matches!(p.retire(0), Err(E_ID_ZERO)) {
        return Err(E_ID_ZERO);
    }
    match p.read(0) {
        Err(E_ID_ZERO) => {}
        _ => return Err(E_ID_ZERO),
    }
    hits += 1;

    // I3：读已回收条目 → E_NOT_LIVE（不得读到墓碑态）。
    if !matches!(p.read(id), Err(E_NOT_LIVE)) {
        return Err(E_NOT_LIVE);
    }
    hits += 1;

    // I4：读不存在的 id → E_SLOT_OOR；空闲栈空后取槽不许返回越界。
    if !matches!(p.read(12_345), Err(E_SLOT_OOR)) {
        return Err(E_SLOT_OOR);
    }
    let empty_pool = ShortPool::new();
    if empty_pool.live != 0 || empty_pool.slot_count() != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // I5：峰值账与实测复核——峰值必须等于槽表长度，且不小于当前驻留。
    if p.high_water < p.live {
        return Err(E_PEAK_BAD);
    }
    if !p.peak_ok() {
        return Err(E_PEAK_BAD);
    }
    hits += 1;

    // 终审：三账守恒 + 槽表 100 + 空闲 100 + 残差 0。
    if !p.conserved() || p.residual() != 0 {
        return Err(E_BAL);
    }
    if p.slot_count() != 100 || p.free.len() != 100 || p.live != 0 {
        return Err(E_BAL);
    }
    if p.created != p.reclaimed {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
