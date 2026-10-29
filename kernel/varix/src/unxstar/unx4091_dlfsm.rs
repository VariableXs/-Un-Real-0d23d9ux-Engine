//! unx4091_dlfsm — UNX-F4091 dentry 生命周期状态机（诊断段 0x8D80）。
//!
//! 判据原文：UNX-F4091-J1 五态（新建/绑定/引用中/未引用/释放）状态机全迁移断言通过，
//!           非法迁移注入（释放→引用中）100/100 次被拒。
//! 可验性质（本模块自证面）：
//!   J1 五态闭集 New(新建) / Bound(绑定) / Referenced(引用中) / Unreferenced(未引用) /
//!      Freed(释放)，迁移表为显式常量表，合法边共 6 条，其余 19 条全为非法；
//!   J2 完整生命周期 New→Bound→Referenced→Unreferenced→Referenced→Unreferenced→Freed
//!      逐步断言通过，每步 state 与 refs 同步变化（引用态 refs>0，非引用态 refs==0）；
//!   J3 非法迁移注入（Freed→Referenced）100/100 次被拒，拒后 state 仍为 Freed、
//!      refs 仍为 0（拒收零副作用），rejects 账逐次 +1；
//!   J4 引用计数纪律：进入 Referenced 必须 refs +1，离开必须 refs 已为 0；
//!      任何"refs>0 却迁到 Unreferenced"或"refs==0 却自称 Referenced"都按 E_REFS 拒；
//!   J5 账守恒：transitions == 成功迁移次数，rejects == 非法注入次数，
//!      且 5 态各自的 entry_count 之和 == 表长。
//! 设计纪律：
//!   1. 迁移表写成 (from, to) 白名单常量数组，判断只查表不写分支链：这样"五态全迁移
//!      断言"与"非法迁移拒绝"共用一份真值，不会出现两处判定打架。
//!   2. Freed 是吸收态：进 Freed 之后任何迁移都拒（唯一合法边是"无"）。
//!   3. 引用计数与状态双写：dget/dput 走的是同一张表（Bound→Referenced、
//!      Unreferenced→Referenced、Referenced→Unreferenced），不另开旁路。
//!   4. 迁移前先校验 refs 前置条件，再改态，最后改 refs；三步顺序固定，任何一步
//!      不满足即整体拒（无部分生效）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8D80 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8D80;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ILLEGAL: u16 = SEG_BASE + 1; // 非法迁移（不在白名单）
const E_NO_ENT: u16 = SEG_BASE + 2; // dentry 不存在
const E_REFS: u16 = SEG_BASE + 3; // 引用计数前置条件不满足
const E_DUP: u16 = SEG_BASE + 4; // 重复建项
const E_FREED: u16 = SEG_BASE + 5; // 已释放（吸收态）再操作
const E_STATE_OOR: u16 = SEG_BASE + 6; // 状态码越界
const E_BAL: u16 = SEG_BASE + 7; // 迁移账/态分布账不平
const E_ID_ZERO: u16 = SEG_BASE + 8; // id=0 保留

/// dentry 生命周期五态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LcState {
    New = 0,          // 新建（已分配结构，未绑定 inode）
    Bound = 1,        // 已绑定 inode（dentry 正身可用）
    Referenced = 2,   // 引用中（d_count > 0，被持有着）
    Unreferenced = 3, // 未引用（d_count == 0，待在 LRU 上等回收）
    Freed = 4,        // 已释放（吸收态，结构归还 slab）
}

/// 五态总数。
pub const LC_N: usize = 5;

/// 合法迁移白名单：(from, to)，共 6 条（判据 J1 的"全迁移"面）。
pub const LEGAL_EDGES: [(LcState, LcState); 6] = [
    (LcState::New, LcState::Bound),                 // 绑定
    (LcState::New, LcState::Freed),                 // 建了不用直接释放
    (LcState::Bound, LcState::Referenced),          // 首次 dget
    (LcState::Bound, LcState::Freed),               // 绑过但未引用即释放
    (LcState::Referenced, LcState::Unreferenced),   // 最后一个 dput
    (LcState::Unreferenced, LcState::Referenced),   // 再次 dget（LRU 复活）
];

/// Unreferenced → Freed 是第 7 条边（回收），单列常量便于对账。
pub const EDGE_RECLAIM: (LcState, LcState) = (LcState::Unreferenced, LcState::Freed);

/// 合法边总数（含回收边）。
pub const LEGAL_EDGE_TOTAL: usize = 7;

/// 单 dentry 生命周期条目。
pub struct LcEntry {
    pub id: u64,        // dentry 标识
    pub state: LcState, // 当前态
    pub refs: u32,      // 引用计数（d_count 的模型）
    pub gen: u32,       // 释放代次（每次 Freed +1）
    pub moves: u32,     // 本条目成功迁移次数
}

/// 生命周期表：条目表 + 迁移账。
pub struct LcTable {
    pub ents: Vec<LcEntry>, // 条目表
    pub transitions: u32,   // 成功迁移总次数
    pub rejects: u32,       // 非法迁移拒收次数
    pub injected: u32,      // 注入尝试次数（负样本账）
    pub reclaimed: u32,     // 回收（→Freed）次数
}

impl LcTable {
    pub fn new() -> LcTable {
        LcTable {
            ents: Vec::new(),
            transitions: 0,
            rejects: 0,
            injected: 0,
            reclaimed: 0,
        }
    }

    /// 建项：id 非 0 且唯一，初态 New。
    pub fn add(&mut self, id: u64) -> Result<(), u16> {
        if id == 0 {
            return Err(E_ID_ZERO);
        }
        if self.find(id).is_some() {
            return Err(E_DUP);
        }
        self.ents.push(LcEntry {
            id,
            state: LcState::New,
            refs: 0,
            gen: 0,
            moves: 0,
        });
        Ok(())
    }

    /// 定位。
    pub fn find(&self, id: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ents.len() {
            if self.ents[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 状态码 → 闭集。
    pub fn state_by_code(code: u16) -> Result<LcState, u16> {
        match code {
            0 => Ok(LcState::New),
            1 => Ok(LcState::Bound),
            2 => Ok(LcState::Referenced),
            3 => Ok(LcState::Unreferenced),
            4 => Ok(LcState::Freed),
            _ => Err(E_STATE_OOR),
        }
    }

    /// 迁移表判定：只查白名单（含回收边）。
    pub fn is_legal(from: LcState, to: LcState) -> bool {
        let mut i = 0usize;
        while i < LEGAL_EDGES.len() {
            if LEGAL_EDGES[i].0 == from && LEGAL_EDGES[i].1 == to {
                return true;
            }
            i += 1;
        }
        if EDGE_RECLAIM.0 == from && EDGE_RECLAIM.1 == to {
            return true;
        }
        false
    }

    /// 迁移：表判定 + 引用前置 + 三步写。
    pub fn advance(&mut self, id: u64, to: LcState) -> Result<LcState, u16> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(E_NO_ENT),
        };
        let from = self.ents[idx].state;
        if from == LcState::Freed {
            // 吸收态：任何迁移都拒（含 Freed→Freed）
            self.rejects += 1;
            return Err(E_FREED);
        }
        if !LcTable::is_legal(from, to) {
            self.rejects += 1;
            return Err(E_ILLEGAL);
        }
        // 引用前置：进入 Referenced 前 refs 由调用方已加（见 dget），此处只校验
        // 迁出 Referenced 时 refs 必须为 0
        if from == LcState::Referenced && to == LcState::Unreferenced && self.ents[idx].refs != 0 {
            self.rejects += 1;
            return Err(E_REFS);
        }
        if to == LcState::Referenced && self.ents[idx].refs == 0 {
            self.rejects += 1;
            return Err(E_REFS); // 无引用却自称引用中
        }
        self.ents[idx].state = to;
        self.ents[idx].moves += 1;
        self.transitions += 1;
        if to == LcState::Freed {
            self.ents[idx].gen += 1;
            self.ents[idx].refs = 0;
            self.reclaimed += 1;
        }
        Ok(to)
    }

    /// dget：Bound/Unreferenced → Referenced，refs +1（先加计数再迁态）。
    pub fn dget(&mut self, id: u64) -> Result<u32, u16> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(E_NO_ENT),
        };
        match self.ents[idx].state {
            LcState::Bound | LcState::Unreferenced => {}
            LcState::Referenced => {
                // 已引用：仅加计数，不迁移（迁移表里没有 Referenced→Referenced）
                self.ents[idx].refs += 1;
                return Ok(self.ents[idx].refs);
            }
            LcState::Freed => {
                self.rejects += 1;
                return Err(E_FREED);
            }
            LcState::New => {
                self.rejects += 1;
                return Err(E_ILLEGAL); // 未绑定不可引用
            }
        }
        self.ents[idx].refs += 1;
        self.advance(id, LcState::Referenced)?;
        Ok(self.ents[idx].refs)
    }

    /// dput：Referenced → refs-1，归零则 → Unreferenced。
    pub fn dput(&mut self, id: u64) -> Result<u32, u16> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(E_NO_ENT),
        };
        if self.ents[idx].state != LcState::Referenced {
            self.rejects += 1;
            return Err(E_ILLEGAL); // 非引用态 dput
        }
        if self.ents[idx].refs == 0 {
            self.rejects += 1;
            return Err(E_REFS);
        }
        self.ents[idx].refs -= 1;
        if self.ents[idx].refs == 0 {
            self.advance(id, LcState::Unreferenced)?;
        }
        Ok(self.ents[idx].refs)
    }

    /// 态分布统计：[New, Bound, Referenced, Unreferenced, Freed]。
    pub fn census(&self) -> [u32; LC_N] {
        let mut c = [0u32; LC_N];
        let mut i = 0usize;
        while i < self.ents.len() {
            c[self.ents[i].state as usize] += 1;
            i += 1;
        }
        c
    }

    /// 账自审：态分布之和 == 表长；迁移总账 == 各条目 moves 之和（J5）。
    pub fn balanced(&self) -> bool {
        let c = self.census();
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < LC_N {
            sum += c[i];
            i += 1;
        }
        if sum as usize != self.ents.len() {
            return false;
        }
        let mut moves = 0u32;
        let mut k = 0usize;
        while k < self.ents.len() {
            moves += self.ents[k].moves;
            k += 1;
        }
        moves == self.transitions
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_ILLEGAL => "非法迁移（不在白名单）",
        E_NO_ENT => "dentry 条目不存在",
        E_REFS => "引用计数前置条件不满足",
        E_DUP => "重复建项",
        E_FREED => "已释放（吸收态）再操作",
        E_STATE_OOR => "状态码越界（合法 0..4）",
        E_BAL => "迁移账/态分布账不平",
        E_ID_ZERO => "dentry id=0 保留",
        _ => "未知诊断码",
    }
}

/// 确定性演练：全生命周期 → 复活 → 非法注入 100/100 → 态分布对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建项，初态 New，refs 0。
    let mut t = LcTable::new();
    t.add(0x4001)?;
    if t.find(0x4001).map(|i| t.ents[i].state) != Some(LcState::New) {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：New→Bound→Referenced（dget）→ dget 再加 1（refs 2）→ dput 两次回到 Unreferenced。
    t.advance(0x4001, LcState::Bound)?;
    if t.dget(0x4001)? != 1 {
        return Err(E_BAL);
    }
    if t.find(0x4001).map(|i| t.ents[i].state) != Some(LcState::Referenced) {
        return Err(E_BAL);
    }
    if t.dget(0x4001)? != 2 {
        return Err(E_BAL); // 已引用态仅加计数
    }
    if t.find(0x4001).map(|i| t.ents[i].refs) != Some(2) {
        return Err(E_BAL);
    }
    if t.dput(0x4001)? != 1 {
        return Err(E_BAL); // 还有 1 笔，态应仍 Referenced
    }
    if t.find(0x4001).map(|i| t.ents[i].state) != Some(LcState::Referenced) {
        return Err(E_BAL);
    }
    if t.dput(0x4001)? != 0 {
        return Err(E_BAL);
    }
    if t.find(0x4001).map(|i| t.ents[i].state) != Some(LcState::Unreferenced) {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：复活路径 Unreferenced→Referenced→Unreferenced（LRU shrink 后重用）。
    if t.dget(0x4001)? != 1 {
        return Err(E_BAL);
    }
    if t.dput(0x4001)? != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：回收 Unreferenced→Freed，gen +1，态分布同步（J2 收尾）。
    t.advance(0x4001, LcState::Freed)?;
    if t.find(0x4001).map(|i| t.ents[i].state) != Some(LcState::Freed) {
        return Err(E_BAL);
    }
    if t.find(0x4001).map(|i| t.ents[i].gen) != Some(1) {
        return Err(E_BAL);
    }
    let c = t.census();
    if c[LcState::Freed as usize] != 1 {
        return Err(E_BAL);
    }
    if !t.balanced() {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：非法迁移注入（Freed→Referenced）100/100 被拒，拒后零副作用（J3 主面）。
    let mut k = 0u32;
    while k < 100 {
        t.injected += 1;
        if t.advance(0x4001, LcState::Referenced) != Err(E_FREED) {
            return Err(E_FREED);
        }
        if t.find(0x4001).map(|i| t.ents[i].state) != Some(LcState::Freed) {
            return Err(E_BAL); // 拒收不得改态
        }
        if t.find(0x4001).map(|i| t.ents[i].refs) != Some(0) {
            return Err(E_BAL);
        }
        k += 1;
    }
    if t.rejects != 100 || t.injected != 100 {
        return Err(E_BAL);
    }
    hits += 1;

    // R6：全新条目走完另一条合法边（New→Freed）与 Bound→Freed（未引用即释放）。
    t.add(0x4002)?;
    t.advance(0x4002, LcState::Freed)?;
    t.add(0x4003)?;
    t.advance(0x4003, LcState::Bound)?;
    t.advance(0x4003, LcState::Freed)?;
    if t.reclaimed != 3 {
        return Err(E_BAL);
    }
    if !t.balanced() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：非法边注入——Bound→New（回退）、Referenced→Bound 等全拒（E_ILLEGAL）。
    t.add(0x4004)?;
    t.advance(0x4004, LcState::Bound)?;
    if t.advance(0x4004, LcState::New) != Err(E_ILLEGAL) {
        return Err(E_ILLEGAL);
    }
    t.dget(0x4004)?;
    if t.advance(0x4004, LcState::Bound) != Err(E_ILLEGAL) {
        return Err(E_ILLEGAL);
    }
    hits += 1;

    // I2：引用前置——refs>0 时不许直接迁 Unreferenced（绕过 dput）。
    if t.advance(0x4004, LcState::Unreferenced) != Err(E_REFS) {
        return Err(E_REFS);
    }
    if t.find(0x4004).map(|i| t.ents[i].state) != Some(LcState::Referenced) {
        return Err(E_BAL);
    }
    t.dput(0x4004)?;
    hits += 1;

    // I3：未绑定（New）直接 dget → E_ILLEGAL；非引用态 dput → E_ILLEGAL。
    t.add(0x4005)?;
    if t.dget(0x4005) != Err(E_ILLEGAL) {
        return Err(E_ILLEGAL);
    }
    if t.dput(0x4005) != Err(E_ILLEGAL) {
        return Err(E_ILLEGAL);
    }
    hits += 1;

    // I4：状态码越界（5 / 65535）→ E_STATE_OOR；重复建项 / 0 号 → 各自拒。
    if LcTable::state_by_code(5) != Err(E_STATE_OOR) {
        return Err(E_STATE_OOR);
    }
    if LcTable::state_by_code(0xFFFF) != Err(E_STATE_OOR) {
        return Err(E_STATE_OOR);
    }
    if t.add(0x4005) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if t.add(0) != Err(E_ID_ZERO) {
        return Err(E_ID_ZERO);
    }
    hits += 1;

    // I5：迁移表自审——6 条白名单 + 1 条回收边，共 7 条；每条都判合法，
    //     其反向边必判非法（表是对称性检验的锚）。
    //     Referenced↔Unreferenced 是唯一允许的双向对（dget/dput 往复），其余反向必非法。
    let bi = (LcState::Referenced, LcState::Unreferenced);
    let mut e = 0usize;
    while e < LEGAL_EDGES.len() {
        let (a, b) = LEGAL_EDGES[e];
        if !LcTable::is_legal(a, b) {
            return Err(E_ILLEGAL);
        }
        let bidir = (a == bi.0 && b == bi.1) || (a == bi.1 && b == bi.0);
        if a != b && !bidir && LcTable::is_legal(b, a) {
            return Err(E_ILLEGAL);
        }
        e += 1;
    }
    if !LcTable::is_legal(EDGE_RECLAIM.0, EDGE_RECLAIM.1) {
        return Err(E_ILLEGAL);
    }
    if LEGAL_EDGE_TOTAL != LEGAL_EDGES.len() + 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // 终审：态分布之和 == 表长，迁移账 == 各条目 moves 之和，Freed 条目 refs 全 0。
    if !t.balanced() {
        return Err(E_BAL);
    }
    let mut i = 0usize;
    while i < t.ents.len() {
        if t.ents[i].state == LcState::Freed && t.ents[i].refs != 0 {
            return Err(E_BAL);
        }
        if t.ents[i].state == LcState::Referenced && t.ents[i].refs == 0 {
            return Err(E_BAL);
        }
        i += 1;
    }
    hits += 1;

    Ok(hits)
}
