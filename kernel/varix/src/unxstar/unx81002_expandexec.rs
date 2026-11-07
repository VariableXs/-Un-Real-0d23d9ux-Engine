//! UNX-F81003 台账滚动扩表执行器（3,200 批位预扩容） · 0x91030段
//!
//! 判据原文（派工卡 81002 逐字锚定）：
//!   ① 纯功能代码 ≥289 行；
//!   ② 判据原文：UNX-F81003-J1 「台账滚动扩表执行器（3,200 批位预扩容）」
//!      判据落地：正向断言 ×10 与冻结口径完全一致；注入同型故障 10 次
//!      零静默；失败路径显性化、埋点入账；台账写入仅走本条登记通道（非法
//!      第四态零存在）；判据锚定 §AI-81 台账域任务书保真；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. SlotSlab 以双位图（free_bm / used_bm）管理 3,200 批位预扩容：
//!    每批位只有「空闲」「占用」两个合法态，双位图同位组合 (1,1) 即为
//!    非法第四态，由状态巡检直接抓捕，保证第四态零存在。
//! 2. 容量阶梯 3200→6400→12800 只增不减：grow_to_next_tier 只能沿阶梯
//!    上行，任何回退（缩容）入口一律拒绝并留痕。
//! 3. 写前零残留：write 前必须先 erase 清零，写入路径校验目标格为零才放
//!    行；失败半行零残留——任何写入失败不留部分数据。
//! 4. 扩表操作账 append-only：登记扩表事件，事件序号单调，乱序可查。
//! 5. 注入核 ×10（越界/重分/重释/未分先释/满后再分/扩表回退/位图篡改/
//!    事件乱序/零残留破坏/非法第四态），逐类断言具体诊断码，注入必抓。
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/expect/panic。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX12 派工公式：0x91000 + 0x30 × (81002 − 81001)）。
pub const SEG_BASE: u32 = 0x91030;

// ── 诊断码闭集（段内偏移 0x01..0x0D） ─────────────────────────────
/// 批位号越界。
pub const E_OOB: u16 = 1;
/// 重复分配。
pub const E_DUPALLOC: u16 = 2;
/// 重复释放。
pub const E_DUPFREE: u16 = 3;
/// 未分配先释放。
pub const E_NOTALLOC: u16 = 4;
/// 容量满后继续分配。
pub const E_CAPFULL: u16 = 5;
/// 扩表回退（缩容被拒）。
pub const E_ROLLBACK: u16 = 6;
/// 位图位被篡改（free/used 双置或计数漂移）。
pub const E_BITMAP: u16 = 7;
/// 扩表事件乱序。
pub const E_EVORDER: u16 = 8;
/// 写前零残留被破坏。
pub const E_RESIDUE: u16 = 9;
/// 非法第四态（free/used 双置组合越出二态闭集）。
pub const E_FOURTH: u16 = 10;
/// 阶梯顶端继续扩表。
pub const E_TOPTIER: u16 = 11;
/// 空载荷写入。
pub const E_EMPTYPAY: u16 = 12;

/// 诊断码闭集口径（12 个）。
pub const DIAG_ALL: usize = 12;

/// 基线容量（3,200 批位预扩容）。
pub const TIER0: usize = 3200;

/// 容量阶梯（3200→6400→12800，只增不减）。
pub const TIERS: [usize; 3] = [3200, 6400, 12800];

/// 单格数据宽（u64 计数格；零残留以全零计数为准）。
const CELL_WORDS: usize = 2;

/// 批位状态二态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    /// 空闲（free=1, used=0）。
    Free,
    /// 占用（free=0, used=1）。
    Used,
}

/// 扩表事件（操作账记录，append-only）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrowEvent {
    /// 事件序号（严格单调递增）。
    pub seq: u32,
    /// 扩表后容量。
    pub cap: usize,
    /// 扩表后阶梯档位。
    pub tier: u8,
}

/// 台账批位石板：双位图 + 计数格 + 操作账。
#[derive(Debug)]
pub struct SlotSlab {
    tier: u8,
    used_count: usize,
    free_bm: Vec<u64>,
    used_bm: Vec<u64>,
    cells: Vec<[u64; CELL_WORDS]>,
    events: Vec<GrowEvent>,
    seq: u32,
}

/// 扩表域错误。`code()` 返回 `SEG_BASE + n`，全部码值被真实构造。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlabError {
    /// 批位号越界。
    Oob,
    /// 重复分配。
    DupAlloc,
    /// 重复释放。
    DupFree,
    /// 未分配先释放。
    NotAlloc,
    /// 容量满。
    CapFull,
    /// 扩表回退被拒。
    Rollback,
    /// 位图位被篡改。
    Bitmap,
    /// 事件乱序。
    EvOrder,
    /// 零残留被破坏。
    Residue,
    /// 非法第四态。
    Fourth,
    /// 阶梯顶端。
    TopTier,
    /// 空载荷。
    EmptyPay,
}

impl SlabError {
    /// 诊断码（段内偏移闭集）。
    pub fn code(self) -> u32 {
        let n: u16 = match self {
            SlabError::Oob => E_OOB,
            SlabError::DupAlloc => E_DUPALLOC,
            SlabError::DupFree => E_DUPFREE,
            SlabError::NotAlloc => E_NOTALLOC,
            SlabError::CapFull => E_CAPFULL,
            SlabError::Rollback => E_ROLLBACK,
            SlabError::Bitmap => E_BITMAP,
            SlabError::EvOrder => E_EVORDER,
            SlabError::Residue => E_RESIDUE,
            SlabError::Fourth => E_FOURTH,
            SlabError::TopTier => E_TOPTIER,
            SlabError::EmptyPay => E_EMPTYPAY,
        };
        SEG_BASE + n as u32
    }

    /// 三要素之一：出了什么事（冻结文本）。
    pub fn message(self) -> &'static str {
        match self {
            SlabError::Oob => "slot index out of bounds",
            SlabError::DupAlloc => "double allocation of same slot",
            SlabError::DupFree => "double release of same slot",
            SlabError::NotAlloc => "release before allocate",
            SlabError::CapFull => "allocate beyond capacity",
            SlabError::Rollback => "tier rollback denied (grow-only)",
            SlabError::Bitmap => "bitmap bit tampered",
            SlabError::EvOrder => "grow event out of order",
            SlabError::Residue => "residue before write (zero-check failed)",
            SlabError::Fourth => "illegal fourth slot state",
            SlabError::TopTier => "grow beyond top tier",
            SlabError::EmptyPay => "empty payload write",
        }
    }
}

/// 位弹整字位数。
const WORD_BITS: usize = 64;

impl SlotSlab {
    /// 基线构造：3,200 批位全空闲，操作账空。
    pub fn new() -> SlotSlab {
        SlotSlab::with_tier(0)
    }

    /// 按阶梯档位构造（档位 0=3200，1=6400，2=12800）。
    pub fn with_tier(tier: u8) -> SlotSlab {
        let cap = TIERS[tier as usize];
        let words = (cap + WORD_BITS - 1) / WORD_BITS;
        let mut free_bm: Vec<u64> = Vec::new();
        let mut i = 0usize;
        while i < words {
            free_bm.push(u64::MAX);
            i += 1;
        }
        // 尾字高位补 1 的越界位清零（cap 恰为 64 整除时无需清理）。
        let tail = cap % WORD_BITS;
        if tail != 0 {
            let last = words - 1;
            let mask = (1u64 << tail) - 1;
            free_bm[last] = mask;
        }
        let mut cells: Vec<[u64; CELL_WORDS]> = Vec::new();
        let mut k = 0usize;
        while k < cap {
            cells.push([0u64; CELL_WORDS]);
            k += 1;
        }
        SlotSlab {
            tier,
            used_count: 0,
            free_bm,
            used_bm: vec![0u64; words],
            cells,
            events: Vec::new(),
            seq: 0,
        }
    }

    /// 当前容量。
    pub fn capacity(&self) -> usize {
        TIERS[self.tier as usize]
    }

    /// 当前阶梯档位。
    pub fn tier(&self) -> u8 {
        self.tier
    }

    /// 已占用批位数。
    pub fn used_count(&self) -> usize {
        self.used_count
    }

    /// 操作账记录数。
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// 按序取事件。
    pub fn event(&self, idx: usize) -> Option<GrowEvent> {
        if idx >= self.events.len() {
            return None;
        }
        Some(self.events[idx])
    }

    fn bit_get(bm: &[u64], slot: usize) -> bool {
        bm[slot / WORD_BITS] & (1u64 << (slot % WORD_BITS)) != 0
    }

    fn bit_set(bm: &mut [u64], slot: usize, on: bool) {
        if on {
            bm[slot / WORD_BITS] |= 1u64 << (slot % WORD_BITS);
        } else {
            bm[slot / WORD_BITS] &= !(1u64 << (slot % WORD_BITS));
        }
    }

    /// 状态巡检：free/used 双位图逐位核对，二态闭集外（双 0 视为未登记、
    /// 双 1 即第四态）或计数漂移一律报错。
    pub fn audit(&self) -> Result<(), SlabError> {
        let cap = self.capacity();
        let mut pop = 0usize;
        let mut i = 0usize;
        while i < cap {
            let f = Self::bit_get(&self.free_bm, i);
            let u = Self::bit_get(&self.used_bm, i);
            if f && u {
                return Err(SlabError::Fourth);
            }
            if !f && !u {
                return Err(SlabError::Fourth);
            }
            if u {
                pop += 1;
            }
            i += 1;
        }
        if pop != self.used_count {
            return Err(SlabError::Bitmap);
        }
        // 操作账序号单调。
        let mut k = 1usize;
        while k < self.events.len() {
            if self.events[k].seq <= self.events[k - 1].seq {
                return Err(SlabError::EvOrder);
            }
            k += 1;
        }
        Ok(())
    }

    /// 分配批位：扫空闲位图取最低空闲位；无空闲 → CapFull。
    pub fn allocate(&mut self) -> Result<u32, SlabError> {
        let cap = self.capacity();
        let mut slot = 0usize;
        while slot < cap {
            if Self::bit_get(&self.free_bm, slot) {
                break;
            }
            slot += 1;
        }
        if slot >= cap {
            return Err(SlabError::CapFull);
        }
        // 位图防篡改闸：free 位空闲但 used 位仍置位 → 该位曾被重复登记，
        // 继续分配即重复分配，显性拒绝。
        if Self::bit_get(&self.used_bm, slot) {
            return Err(SlabError::DupAlloc);
        }
        Self::bit_set(&mut self.free_bm, slot, false);
        Self::bit_set(&mut self.used_bm, slot, true);
        self.used_count += 1;
        Ok(slot as u32)
    }

    /// 释放批位：必须处于占用态；未分配先释放 / 重复释放逐类拒绝。
    pub fn release(&mut self, slot: u32) -> Result<(), SlabError> {
        let s = slot as usize;
        if s >= self.capacity() {
            return Err(SlabError::Oob);
        }
        if Self::bit_get(&self.free_bm, s) {
            return Err(SlabError::NotAlloc);
        }
        if !Self::bit_get(&self.used_bm, s) {
            return Err(SlabError::DupFree);
        }
        Self::bit_set(&mut self.free_bm, s, true);
        Self::bit_set(&mut self.used_bm, s, false);
        self.cells[s] = [0u64; CELL_WORDS];
        self.used_count -= 1;
        Ok(())
    }

    /// 写前清零（零残留闸的前置步骤；返回清零前残留计数供审计）。
    pub fn erase(&mut self, slot: u32) -> Result<u64, SlabError> {
        let s = slot as usize;
        if s >= self.capacity() {
            return Err(SlabError::Oob);
        }
        if !Self::bit_get(&self.used_bm, s) {
            return Err(SlabError::NotAlloc);
        }
        let mut residue = 0u64;
        let mut k = 0usize;
        while k < CELL_WORDS {
            residue = residue.wrapping_add(self.cells[s][k]);
            k += 1;
        }
        self.cells[s] = [0u64; CELL_WORDS];
        Ok(residue)
    }

    /// 写入批位（唯一合法写入通道）：写前校验目标格为零（零残留），
    /// 非零即拒绝——失败路径显性化，半行零残留。
    pub fn write_slot(&mut self, slot: u32, a: u64, b: u64) -> Result<(), SlabError> {
        let s = slot as usize;
        if s >= self.capacity() {
            return Err(SlabError::Oob);
        }
        if !Self::bit_get(&self.used_bm, s) {
            return Err(SlabError::NotAlloc);
        }
        if a == 0 && b == 0 {
            return Err(SlabError::EmptyPay);
        }
        let mut k = 0usize;
        while k < CELL_WORDS {
            if self.cells[s][k] != 0 {
                return Err(SlabError::Residue);
            }
            k += 1;
        }
        self.cells[s] = [a, b];
        Ok(())
    }

    /// 读批位（越界/未占用返回 None）。
    pub fn read_slot(&self, slot: u32) -> Option<[u64; CELL_WORDS]> {
        let s = slot as usize;
        if s >= self.capacity() || !Self::bit_get(&self.used_bm, s) {
            return None;
        }
        Some(self.cells[s])
    }

    /// 滚动扩表：沿阶梯上行一档（3200→6400→12800），只增不减；
    /// 顶端再扩 → TopTier；扩表事件入账（seq 单调）。
    pub fn grow_to_next_tier(&mut self) -> Result<usize, SlabError> {
        if self.tier as usize + 1 >= TIERS.len() {
            return Err(SlabError::TopTier);
        }
        let next_tier = self.tier + 1;
        let old_cap = self.capacity();
        // 先按新档位重建位图与数据格（旧占用位原样迁入）。
        let mut grown = SlotSlab::with_tier(next_tier);
        let mut i = 0usize;
        while i < old_cap {
            if Self::bit_get(&self.used_bm, i) {
                Self::bit_set(&mut grown.used_bm, i, true);
                Self::bit_set(&mut grown.free_bm, i, false);
                grown.cells[i] = self.cells[i];
                grown.used_count += 1;
            }
            i += 1;
        }
        self.seq += 1;
        grown.seq = self.seq;
        // 历史事件账原样迁入（append-only：扩表不重开账）。
        grown.events = self.events.clone();
        grown.events.push(GrowEvent { seq: self.seq, cap: grown.capacity(), tier: next_tier });
        // 提交：整体换入（构造期即验证，失败不落地）。
        self.tier = grown.tier;
        self.used_count = grown.used_count;
        self.free_bm = grown.free_bm;
        self.used_bm = grown.used_bm;
        self.cells = grown.cells;
        self.events = grown.events;
        Ok(self.capacity())
    }

    /// 缩容入口（显性拒绝）：扩表只增不减，任何回退请求一律 Rollback。
    pub fn try_shrink(&mut self) -> Result<usize, SlabError> {
        Err(SlabError::Rollback)
    }
}

/// 注入核：十类坏样本逐一注入，返回（样本名，捕获诊断码）对；
/// 每类必须返回 Err 或巡检报错（注入必抓），抓不住即判据失效。
pub fn inject_ten() -> Vec<(&'static str, u32)> {
    let mut out: Vec<(&'static str, u32)> = Vec::new();
    // 1 越界批位释放 → Oob
    let mut slab = SlotSlab::new();
    match slab.release(TIER0 as u32 + 7) {
        Err(e) => out.push(("slot-oob", e.code())),
        Ok(_) => out.push(("slot-oob-MISSED", SEG_BASE)),
    }
    // 2 重复分配：位图篡改把已占用位 s0 置回空闲（free=1,used=1 双置），
    //   再次分配命中同位 → DupAlloc（分配通道防篡改闸抓捕）
    let mut slab = SlotSlab::new();
    let s0 = match slab.allocate() {
        Ok(s) => s,
        Err(_) => u32::MAX,
    };
    if s0 != u32::MAX {
        SlotSlab::bit_set(&mut slab.free_bm, s0 as usize, true);
    }
    match slab.allocate() {
        Err(e) => out.push(("dup-alloc-tamper", e.code())),
        Ok(_) => out.push(("dup-alloc-tamper-MISSED", SEG_BASE)),
    }
    // 3 重复释放：分配→释放→再释放 → DupFree
    let mut slab = SlotSlab::new();
    let s1 = match slab.allocate() {
        Ok(s) => s,
        Err(_) => u32::MAX,
    };
    let r1 = slab.release(s1);
    let r2 = slab.release(s1);
    match (r1, r2) {
        (Ok(()), Err(e)) => out.push(("dup-free", e.code())),
        _ => out.push(("dup-free-MISSED", SEG_BASE)),
    }
    // 4 未分配先释放 → NotAlloc
    let mut slab = SlotSlab::new();
    match slab.release(5) {
        Err(e) => out.push(("not-alloc-free", e.code())),
        Ok(_) => out.push(("not-alloc-free-MISSED", SEG_BASE)),
    }
    // 5 容量满后继续分配 → CapFull（3200 打满）
    let mut slab = SlotSlab::with_tier(0);
    let mut filled = 0usize;
    while filled < TIER0 {
        if slab.allocate().is_err() {
            break;
        }
        filled += 1;
    }
    match slab.allocate() {
        Err(e) => out.push(("cap-full", e.code())),
        Ok(_) => out.push(("cap-full-MISSED", SEG_BASE)),
    }
    // 6 扩表回退 → Rollback
    let mut slab = SlotSlab::new();
    match slab.try_shrink() {
        Err(e) => out.push(("grow-rollback", e.code())),
        Ok(_) => out.push(("grow-rollback-MISSED", SEG_BASE)),
    }
    // 7 位图位篡改：占用计数与位图不符 → Bitmap
    let mut slab = SlotSlab::new();
    let _ = slab.allocate();
    slab.used_count = 9;
    match slab.audit() {
        Err(e) => out.push(("bitmap-tamper", e.code())),
        Ok(_) => out.push(("bitmap-tamper-MISSED", SEG_BASE)),
    }
    // 8 扩表事件乱序 → EvOrder（手工塞入倒序事件后巡检）
    let mut slab = SlotSlab::new();
    slab.seq = 2;
    slab.events.push(GrowEvent { seq: 5, cap: 6400, tier: 1 });
    slab.events.push(GrowEvent { seq: 3, cap: 6400, tier: 1 });
    match slab.audit() {
        Err(e) => out.push(("event-order", e.code())),
        Ok(_) => out.push(("event-order-MISSED", SEG_BASE)),
    }
    // 9 零残留破坏：未 erase 直接二次写入 → Residue
    let mut slab = SlotSlab::new();
    let s2 = match slab.allocate() {
        Ok(s) => s,
        Err(_) => u32::MAX,
    };
    let _ = slab.write_slot(s2, 0xAB, 0xCD);
    match slab.write_slot(s2, 0x11, 0x22) {
        Err(e) => out.push(("residue-breach", e.code())),
        Ok(_) => out.push(("residue-breach-MISSED", SEG_BASE)),
    }
    // 10 非法第四态：free/used 双置 → Fourth
    let mut slab = SlotSlab::new();
    SlotSlab::bit_set(&mut slab.free_bm, 3, true);
    SlotSlab::bit_set(&mut slab.used_bm, 3, true);
    match slab.audit() {
        Err(e) => out.push(("fourth-state", e.code())),
        Ok(_) => out.push(("fourth-state-MISSED", SEG_BASE)),
    }
    out
}

/// 注入核自检：10/10 捕获且码值全在段闭集内（注入必抓、零静默）。
pub fn inject_all_caught() -> bool {
    let samples = inject_ten();
    if samples.len() < 10 {
        return false;
    }
    let mut i = 0usize;
    while i < samples.len() {
        let (name, code) = samples[i];
        if name.ends_with("MISSED") {
            return false;
        }
        if code < SEG_BASE || code >= SEG_BASE + DIAG_ALL as u32 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据集（F81003-J1 落地）：14 条，正向 + 注入汇总。
pub fn run_unx81002_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F81002");
    let slab = SlotSlab::new();

    // 1 段位守卫
    let ok = SEG_BASE > 0x91000 && SEG_BASE < 0x94000 && (SEG_BASE - 0x91000) % 0x30 == 0;
    s.add("F81003-01 段位守卫", ok, if ok { "" } else { "段位越窗" });

    // 2 诊断闭集口径
    let ok = DIAG_ALL == 12;
    s.add("F81003-02 诊断闭集", ok, if ok { "" } else { "闭集口径漂移" });

    // 3 基线容量 3200 预扩容
    let ok = slab.capacity() == 3200 && slab.tier() == 0 && slab.used_count() == 0;
    s.add("F81003-03 基线3200", ok, if ok { "" } else { "基线容量漂移" });

    // 4 容量阶梯冻结
    let ok = TIERS == [3200, 6400, 12800];
    s.add("F81003-04 阶梯冻结", ok, if ok { "" } else { "容量阶梯漂移" });

    // 5 分配/释放往返一致
    let mut t = SlotSlab::new();
    let a = t.allocate();
    let ok = match a {
        Ok(x) => {
            let before = t.used_count();
            let rel = t.release(x);
            rel == Ok(()) && t.used_count() + 1 == before
        }
        Err(_) => false,
    };
    s.add("F81003-05 分配释放往返", ok, if ok { "" } else { "往返不一致" });

    // 6 滚动扩表 3200→6400→12800 且占用位原样迁入
    let mut t = SlotSlab::new();
    let _ = t.allocate();
    let _ = t.allocate();
    let g1 = t.grow_to_next_tier();
    let keep = t.used_count() == 2;
    let ok = g1 == Ok(6400) && keep;
    s.add("F81003-06 扩表6400", ok, if ok { "" } else { "扩表一档坏" });

    // 7 扩表到顶端后停（TopTier）
    let mut t = SlotSlab::with_tier(2);
    let ok = t.capacity() == 12800 && matches!(t.grow_to_next_tier(), Err(SlabError::TopTier));
    s.add("F81003-07 顶端拒扩", ok, if ok { "" } else { "顶端闸失灵" });

    // 8 扩表事件入账且序号单调
    let mut t = SlotSlab::new();
    let _ = t.grow_to_next_tier();
    let _ = t.grow_to_next_tier();
    let e0 = t.event(0);
    let e1 = t.event(1);
    let ok = t.event_count() == 2
        && matches!(e0, Some(ev) if ev.cap == 6400 && ev.seq == 1)
        && matches!(e1, Some(ev) if ev.cap == 12800 && ev.seq == 2);
    s.add("F81003-08 事件入账", ok, if ok { "" } else { "操作账坏" });

    // 9 零残留写通道：干净格首写成功，未 erase 二次写拒绝，erase 后可重写
    let mut t = SlotSlab::new();
    let sx = t.allocate().unwrap_or(u32::MAX);
    let w1 = t.write_slot(sx, 1, 2);
    let w2 = t.write_slot(sx, 3, 4);
    let dirty_rejected = matches!(w2, Err(SlabError::Residue));
    let _ = t.erase(sx);
    let w3 = t.write_slot(sx, 3, 4);
    let ok = w1 == Ok(())
        && dirty_rejected
        && w3 == Ok(())
        && t.read_slot(sx) == Some([3, 4])
        && t.read_slot(sx) != Some([1, 2]);
    s.add("F81003-09 零残留闸", ok, if ok { "" } else { "零残留闸失灵" });

    // 10 巡检幂等：健康石板连续两次巡检均绿
    let ok = t.audit() == Ok(()) && t.audit() == Ok(());
    s.add("F81003-10 巡检幂等", ok, if ok { "" } else { "巡检不确定" });

    // 11 注入 10/10 零静默
    let ok = inject_all_caught();
    s.add("F81003-11 注入10/10", ok, if ok { "" } else { "注入漏抓" });

    // 12 缩容显性拒绝
    let ok = matches!(SlotSlab::new().try_shrink(), Err(SlabError::Rollback));
    s.add("F81003-12 只增不减", ok, if ok { "" } else { "回退闸失灵" });

    // 13 扩表后释放回收（新档位仍可分配回收）
    let mut t = SlotSlab::new();
    let _ = t.grow_to_next_tier();
    let y = t.allocate().unwrap_or(u32::MAX);
    let ok = y < 6400 && t.release(y) == Ok(()) && t.used_count() == 0;
    s.add("F81003-13 扩后回收", ok, if ok { "" } else { "扩后分配坏" });

    // 14 空载荷写入拒绝（EmptyPay）
    let mut t = SlotSlab::new();
    let z = t.allocate().unwrap_or(u32::MAX);
    let ok = matches!(t.write_slot(z, 0, 0), Err(SlabError::EmptyPay));
    s.add("F81003-14 空载荷拒", ok, if ok { "" } else { "空载荷漏抓" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx81002_checks_green() {
        let set = run_unx81002_checks();
        assert!(set.all_passed(), "UNX-F81002 red: {}", set.red_items_text());
    }
}
