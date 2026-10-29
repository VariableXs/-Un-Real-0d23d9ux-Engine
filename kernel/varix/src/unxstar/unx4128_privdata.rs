//! unx4128_privdata — UNX-F4128 私有数据指针契约（fs 嫁接侧）（诊断段 0x91E0）。
//!
//! 判据原文：UNX-F4128-J1 private_data 生命周期契约（open 置/release 清）断言通过，
//!           close 后访问注入被毒化值检测拦截。
//!   J2 令牌三元组（槽位/代次/魔数）三重校验。
//!   J3 release 为覆盖写毒化值，不留旧令牌。
//!   J4 置清配对守恒：差值为当前 Set 槽数。
//! 设计纪律：
//!   1. private_data 的生命周期被钉在 open/release 之间：open 置（状态 Unset→Set，
//!      代次 gen 加一），release 清（Set→Cleared，槽内写入毒化值而非留旧值）。
//!      留旧值等于给「close 后仍可读」开后门，故清的动作必须是覆盖写。
//!   2. 以「槽位号 + 代次 + 魔数」三元组代替裸指针：槽位号标识归属，代次防 ABA
//!      （同一槽位被下一次 open 复用后，旧令牌因代次不符而失效），魔数防伪造。
//!   3. 毒化值 POISON_MAGIC 与合法魔数 LIVE_MAGIC 互异且不互为子串关系位；
//!      读到毒化值判 E_POISON，读到旧代次判 E_STALE，两者错误码分离便于归因。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x91E0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x91E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOTSET: u16 = SEG_BASE + 1; // 未置私有数据即访问
const E_POISON: u16 = SEG_BASE + 2; // 读到毒化值（close 后访问）
const E_STALE: u16 = SEG_BASE + 3; // 代次不符（ABA 复用后的旧令牌）
const E_DOUBLE_OPEN: u16 = SEG_BASE + 4; // 已置状态下重复 open
const E_DOUBLE_FREE: u16 = SEG_BASE + 5; // 重复 release 清
const E_BADMAGIC: u16 = SEG_BASE + 6; // 魔数不在闭集（伪造令牌）
const E_SLOT: u16 = SEG_BASE + 7; // 槽位越界
const E_ACCT: u16 = SEG_BASE + 8; // 置/清/毒化三账不平

/// 合法令牌魔数（"PRIV"）。
pub const LIVE_MAGIC: u32 = 0x5052_4956;
/// 毒化值魔数（"DEAD"），release 后写入槽位，任何读路径必先比此值。
pub const POISON_MAGIC: u32 = 0xDEAD_DEAD;
/// 未初始化魔数（"NONE"）。
pub const NONE_MAGIC: u32 = 0x4E4F_4E45;

/// 私有数据槽状态机：Unset → Set → Cleared；Cleared 之后只可经 open 回到 Set。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrivState {
    Unset,
    Set,
    Cleared,
}

/// 私有数据令牌：代替裸指针的三元组。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PrivToken {
    /// 槽位号（fs 侧对象池下标）
    pub slot: u32,
    /// 代次（每次 open 递增，防 ABA）
    pub gen: u32,
    /// 魔数（合法必为 LIVE_MAGIC）
    pub magic: u32,
}

impl PrivToken {
    /// 毒化令牌：release 后槽内保留它，读路径据此判 E_POISON。
    pub fn poisoned(slot: u32, gen: u32) -> PrivToken {
        PrivToken {
            slot,
            gen,
            magic: POISON_MAGIC,
        }
    }

    pub fn is_poison(&self) -> bool {
        self.magic == POISON_MAGIC
    }

    pub fn is_live(&self) -> bool {
        self.magic == LIVE_MAGIC
    }
}

/// 私有数据槽：状态 + 令牌 + 代次生成器。
pub struct PrivSlot {
    /// 槽位号
    pub slot: u32,
    /// 状态
    pub state: PrivState,
    /// 槽内令牌（毒化期存 POISON 令牌，未置期存 NONE 令牌）
    pub token: PrivToken,
    /// 代次生成器（单调递增，不回收）
    pub gen_seq: u32,
    /// 置次数
    pub sets: u32,
    /// 清次数
    pub clears: u32,
}

impl PrivSlot {
    pub fn new(slot: u32) -> Result<PrivSlot, u16> {
        if slot == 0 {
            return Err(E_SLOT); // 0 为保留槽位（与空指针同义）
        }
        Ok(PrivSlot {
            slot,
            state: PrivState::Unset,
            token: PrivToken {
                slot,
                gen: 0,
                magic: NONE_MAGIC,
            },
            gen_seq: 0,
            sets: 0,
            clears: 0,
        })
    }

    /// open 侧：置私有数据。仅 Unset/Cleared 可置，Set 期重复 open 即拒。
    pub fn open_set(&mut self) -> Result<PrivToken, u16> {
        match self.state {
            PrivState::Set => return Err(E_DOUBLE_OPEN),
            PrivState::Unset | PrivState::Cleared => {}
        }
        if self.gen_seq >= u32::MAX - 1 {
            return Err(E_ACCT); // 代次耗尽：宁可拒服务也不回绕
        }
        self.gen_seq += 1;
        let t = PrivToken {
            slot: self.slot,
            gen: self.gen_seq,
            magic: LIVE_MAGIC,
        };
        self.token = t;
        self.state = PrivState::Set;
        self.sets += 1;
        Ok(t)
    }

    /// release 侧：清私有数据（覆盖写毒化值，不留旧令牌）。
    pub fn release_clear(&mut self) -> Result<(), u16> {
        match self.state {
            PrivState::Unset => return Err(E_NOTSET),
            PrivState::Cleared => return Err(E_DOUBLE_FREE),
            PrivState::Set => {}
        }
        self.token = PrivToken::poisoned(self.slot, self.gen_seq);
        self.state = PrivState::Cleared;
        self.clears += 1;
        Ok(())
    }

    /// 读路径：三重校验（魔数 → 毒化 → 代次）。
    pub fn access(&self, t: &PrivToken) -> Result<u32, u16> {
        if t.magic == POISON_MAGIC || self.token.is_poison() {
            return Err(E_POISON); // 毒化优先：close 后访问的主判据
        }
        if t.magic != LIVE_MAGIC {
            return Err(E_BADMAGIC);
        }
        if self.state != PrivState::Set {
            return Err(E_NOTSET);
        }
        if t.slot != self.slot {
            return Err(E_SLOT);
        }
        if t.gen != self.token.gen {
            return Err(E_STALE); // ABA：槽位被下一次 open 复用
        }
        Ok(t.gen)
    }

    /// 契约不变式：Set ⟺ 槽内为 LIVE 令牌且状态一致。
    pub fn contract_holds(&self) -> bool {
        match self.state {
            PrivState::Set => self.token.is_live() && self.token.gen == self.gen_seq,
            PrivState::Cleared => self.token.is_poison(),
            PrivState::Unset => self.token.magic == NONE_MAGIC,
        }
    }
}

/// fs 嫁接侧私有数据总账：多槽 + 毒化命中账 + 过期命中账。
pub struct PrivFabric {
    /// 槽表
    pub slots: Vec<PrivSlot>,
    /// 置总数
    pub open_sets: u32,
    /// 清总数
    pub close_clears: u32,
    /// 毒化命中（close 后访问被拦）
    pub poison_hits: u32,
    /// 过期命中（旧代次被拦）
    pub stale_hits: u32,
}

impl PrivFabric {
    pub fn with_slots(n: u32) -> Result<PrivFabric, u16> {
        let mut v = Vec::new();
        let mut i = 1u32;
        while i <= n {
            v.push(PrivSlot::new(i)?);
            i += 1;
        }
        Ok(PrivFabric {
            slots: v,
            open_sets: 0,
            close_clears: 0,
            poison_hits: 0,
            stale_hits: 0,
        })
    }

    fn at(&self, idx: usize) -> Result<&PrivSlot, u16> {
        match self.slots.get(idx) {
            Some(s) => Ok(s),
            None => Err(E_SLOT),
        }
    }

    fn at_mut(&mut self, idx: usize) -> Result<&mut PrivSlot, u16> {
        match self.slots.get_mut(idx) {
            Some(s) => Ok(s),
            None => Err(E_SLOT),
        }
    }

    pub fn open(&mut self, idx: usize) -> Result<PrivToken, u16> {
        let t = match self.at_mut(idx) {
            Ok(s) => s.open_set()?,
            Err(e) => return Err(e),
        };
        self.open_sets += 1;
        Ok(t)
    }

    pub fn close(&mut self, idx: usize) -> Result<(), u16> {
        match self.at_mut(idx) {
            Ok(s) => s.release_clear()?,
            Err(e) => return Err(e),
        };
        self.close_clears += 1;
        Ok(())
    }

    /// 带账访问：毒化/过期命中各自入账。
    pub fn read(&mut self, idx: usize, t: &PrivToken) -> Result<u32, u16> {
        let r = match self.at(idx) {
            Ok(s) => s.access(t),
            Err(e) => return Err(e),
        };
        match r {
            Ok(g) => Ok(g),
            Err(E_POISON) => {
                self.poison_hits += 1;
                Err(E_POISON)
            }
            Err(E_STALE) => {
                self.stale_hits += 1;
                Err(E_STALE)
            }
            Err(e) => Err(e),
        }
    }

    /// 置/清配对守恒：open_sets 与 close_clears 之差 == 当前 Set 槽数。
    pub fn paired(&self) -> bool {
        let mut set = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].state == PrivState::Set {
                set += 1;
            }
            i += 1;
        }
        self.open_sets >= self.close_clears && (self.open_sets - self.close_clears) == set
    }

    /// 全槽契约复核。
    pub fn all_contracts_hold(&self) -> bool {
        let mut i = 0usize;
        while i < self.slots.len() {
            if !self.slots[i].contract_holds() {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOTSET => "未置私有数据即访问",
        E_POISON => "读到毒化值（close 后访问）",
        E_STALE => "令牌代次不符（ABA 复用后的旧令牌）",
        E_DOUBLE_OPEN => "Set 期重复 open",
        E_DOUBLE_FREE => "重复 release 清",
        E_BADMAGIC => "魔数不在闭集（伪造令牌）",
        E_SLOT => "槽位越界或保留槽 0",
        E_ACCT => "置/清/毒化三账不平",
        _ => "未知诊断码",
    }
}

/// 确定性演练：置/清契约 → close 后注入 → ABA → 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：open 置 → 读通；状态 Set、令牌 Live、契约成立。
    let mut fab = PrivFabric::with_slots(4)?;
    let t1 = fab.open(0)?;
    if fab.read(0, &t1)? != 1 {
        return Err(E_ACCT); // 首代次必为 1
    }
    if !fab.all_contracts_hold() {
        return Err(E_ACCT);
    }
    hits += 1;

    // R2：release 清 → 槽内为毒化值，且不留旧令牌（覆盖写）。
    fab.close(0)?;
    {
        let s = match fab.slots.get(0) {
            Some(x) => x,
            None => return Err(E_SLOT),
        };
        if s.state != PrivState::Cleared {
            return Err(E_ACCT);
        }
        if s.token.gen != 1 || !s.token.is_poison() {
            return Err(E_POISON); // 毒化值必须带新代次，且魔数是 POISON
        }
    }
    hits += 1;

    // R3：close 后访问注入 10/10 次全部 E_POISON（判据主断言）。
    let mut i = 0u32;
    while i < 10 {
        if fab.read(0, &t1) != Err(E_POISON) {
            return Err(E_POISON);
        }
        i += 1;
    }
    if fab.poison_hits != 10 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R4：ABA——同槽再 open（代次 2），旧令牌 t1 用过即 E_STALE。
    let t2 = fab.open(0)?;
    if t2.gen != 2 {
        return Err(E_ACCT);
    }
    if fab.read(0, &t1) != Err(E_STALE) {
        return Err(E_STALE);
    }
    if fab.stale_hits != 1 {
        return Err(E_ACCT);
    }
    if fab.read(0, &t2)? != 2 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R5：多槽独立——槽 2/3 各自置清互不影响，配对守恒成立。
    let a = fab.open(1)?;
    let b = fab.open(2)?;
    fab.close(1)?;
    if fab.read(1, &a) != Err(E_POISON) {
        return Err(E_POISON);
    }
    if fab.read(2, &b)? != 1 {
        return Err(E_ACCT); // 槽 3 未受影响
    }
    if !fab.paired() {
        return Err(E_ACCT);
    }
    hits += 1;

    // I1：未置即访问 → E_NOTSET（槽 4 全程未动）。
    let probe = PrivToken {
        slot: 4,
        gen: 1,
        magic: LIVE_MAGIC,
    };
    if fab.read(3, &probe) != Err(E_NOTSET) {
        return Err(E_NOTSET);
    }
    hits += 1;

    // I2：伪造魔数 → E_BADMAGIC（不是 E_POISON，归因可区分）。
    let forged = PrivToken {
        slot: 3,
        gen: b.gen,
        magic: 0x1234_5678,
    };
    if fab.read(2, &forged) != Err(E_BADMAGIC) {
        return Err(E_BADMAGIC);
    }
    hits += 1;

    // I3：重复 open（Set 期）→ E_DOUBLE_OPEN；重复 release → E_DOUBLE_FREE。
    if fab.open(0).is_ok() {
        return Err(E_DOUBLE_OPEN);
    }
    fab.close(0)?;
    if fab.close(0) != Err(E_DOUBLE_FREE) {
        return Err(E_DOUBLE_FREE);
    }
    hits += 1;

    // I4：保留槽 0 与越界槽 → E_SLOT。
    if PrivSlot::new(0).is_ok() {
        return Err(E_SLOT);
    }
    if fab.read(99, &probe) != Err(E_SLOT) {
        return Err(E_SLOT);
    }
    hits += 1;

    // C1：配对守恒终审：open 4 次（槽1×2、槽2、槽3）、close 3 次，剩 1 个 Set。
    if fab.open_sets != 4 || fab.close_clears != 3 {
        return Err(E_ACCT);
    }
    if !fab.paired() {
        return Err(E_ACCT);
    }
    if !fab.all_contracts_hold() {
        return Err(E_ACCT);
    }
    hits += 1;

    Ok(hits)
}
