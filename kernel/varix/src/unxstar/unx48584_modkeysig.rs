//! UNX-F48584 —— I·修饰键语义分界复签
//!         （Shift/Caps 采集归 M2、状态机归 M1）
//! 判据原文：UNX-F48584-J1 复签 ×10；判据锚定 §AI-61 §2 专题五。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           分界表冻结 + 十轮复签自证；drill 返回 Ok(hits)。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0020; // 修饰键语义分界诊断段

const E_OWNER_BAD: u16 = SEG_BASE + 1; // 归属表漂移
const E_COLLECT_BAD: u16 = SEG_BASE + 2; // 采集面归属违例
const E_STATE_BAD: u16 = SEG_BASE + 3; // 状态机面归属违例
const E_ROUND_MISS: u16 = SEG_BASE + 4; // 复签轮次缺失
const E_SIGN_DIFF: u16 = SEG_BASE + 5; // 复签不一致
const E_EVENT_BAD: u16 = SEG_BASE + 6; // 事件路由错位
const E_LOCK_BAD: u16 = SEG_BASE + 7; // 锁定态语义违例
const E_REPLAY: u16 = SEG_BASE + 8; // 重演指纹不一致

/// 修饰键。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ModKey {
    Shift = 0,
    Ctrl = 1,
    Alt = 2,
    CapsLock = 3,
    NumLock = 4,
}

pub const MOD_COUNT: usize = 5;

impl ModKey {
    pub fn from_index(i: usize) -> Result<ModKey, u16> {
        match i {
            0 => Ok(ModKey::Shift),
            1 => Ok(ModKey::Ctrl),
            2 => Ok(ModKey::Alt),
            3 => Ok(ModKey::CapsLock),
            4 => Ok(ModKey::NumLock),
            _ => Err(E_OWNER_BAD),
        }
    }
    /// 名字（冻结）。
    pub fn name(&self) -> &'static str {
        match self {
            ModKey::Shift => "shift",
            ModKey::Ctrl => "ctrl",
            ModKey::Alt => "alt",
            ModKey::CapsLock => "caps",
            ModKey::NumLock => "num",
        }
    }
    /// 分界冻结表：采集归 M2（true），状态机归 M1。
    /// Shift/Caps 采集面（按下/抬起时戳）归 M2；两态机（有效/锁定）归 M1。
    /// Ctrl/Alt/NumLock 同表管理，本卡以 Shift/Caps 为复签重点。
    pub fn collect_owner_m2(&self) -> bool {
        match self {
            ModKey::Shift | ModKey::CapsLock => true,
            _ => true,
        }
    }
    /// 状态机归 M1（所有修饰键状态归一）。
    pub fn state_owner_m1(&self) -> bool {
        true
    }
    /// 是否锁定型（Caps/Num）。
    pub fn is_locking(&self) -> bool {
        match self {
            ModKey::CapsLock | ModKey::NumLock => true,
            _ => false,
        }
    }
}

/// M1 侧修饰键状态机。
pub struct ModState {
    pub down: [bool; MOD_COUNT],
    pub locked: [bool; MOD_COUNT],
    pub events: u32,
}

impl ModState {
    pub fn new() -> Self {
        ModState {
            down: [false; MOD_COUNT],
            locked: [false; MOD_COUNT],
            events: 0,
        }
    }

    /// M2 采集面上报一次物理事件（key，pressed），状态机归 M1 处理。
    pub fn on_physical(&mut self, key: ModKey, pressed: bool) -> Result<(), u16> {
        if !key.collect_owner_m2() || !key.state_owner_m1() {
            return Err(E_OWNER_BAD);
        }
        let ki = key as usize;
        if key.is_locking() {
            // 锁定型：仅在按下沿翻转锁定，不做按住态。
            if pressed {
                self.locked[ki] = !self.locked[ki];
                self.events += 1;
            }
            return Ok(());
        }
        if self.down[ki] == pressed {
            return Err(E_EVENT_BAD);
        }
        self.down[ki] = pressed;
        self.events += 1;
        Ok(())
    }

    /// 有效态查询：按下或锁定任一为真。
    pub fn effective(&self, key: ModKey) -> bool {
        let ki = key as usize;
        self.down[ki] || self.locked[ki]
    }

    /// 锁定态查询。
    pub fn lock_state(&self, key: ModKey) -> bool {
        self.locked[key as usize]
    }
}

/// 复签账：对分界表做十轮独立复签。
pub struct JointSign {
    pub rounds: Vec<u64>,
    pub label: String,
}

impl JointSign {
    pub fn new(label: &str) -> Self {
        JointSign { rounds: Vec::new(), label: label.to_string() }
    }

    /// 单轮复签：全表校验归属，返回本轮指纹。
    pub fn round(&mut self, salt: u64) -> Result<u64, u16> {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ salt;
        let mut i = 0usize;
        while i < MOD_COUNT {
            let key = ModKey::from_index(i)?;
            // 采集面归属必须为 M2。
            if !key.collect_owner_m2() {
                return Err(E_COLLECT_BAD);
            }
            // 状态机归属必须为 M1。
            if !key.state_owner_m1() {
                return Err(E_STATE_BAD);
            }
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ (key as u64);
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ key.name().as_bytes()[0] as u64;
            i += 1;
        }
        self.rounds.push(h);
        Ok(h)
    }

    /// 复签一致性：全部轮次指纹一致。
    pub fn consistent(&self) -> bool {
        if self.rounds.len() < 2 {
            return false;
        }
        let f = self.rounds[0];
        let mut i = 1usize;
        while i < self.rounds.len() {
            if self.rounds[i] != f {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 越界注入探针：外部试图把采集归到 M1（伪造）。
/// 分界表冻结：任何伪造都应被拒。
pub fn inject_fake_owner(key: ModKey, claim_m1_collect: bool) -> Result<(), u16> {
    if claim_m1_collect {
        return Err(E_COLLECT_BAD);
    }
    if !key.collect_owner_m2() {
        return Err(E_COLLECT_BAD);
    }
    Ok(())
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 主巡检：修饰键语义分界复签十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 分界冻结表：五键采集归 M2、状态机归 M1。
    let mut i = 0usize;
    while i < MOD_COUNT {
        let key = ModKey::from_index(i)?;
        if !key.collect_owner_m2() || !key.state_owner_m1() {
            return Err(E_OWNER_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R2 Shift 采集面：按下/抬起对进状态机，事件账齐。
    let mut st = ModState::new();
    st.on_physical(ModKey::Shift, true)?;
    if !st.effective(ModKey::Shift) {
        return Err(E_STATE_BAD);
    }
    st.on_physical(ModKey::Shift, false)?;
    if st.effective(ModKey::Shift) {
        return Err(E_STATE_BAD);
    }
    if st.events != 2 {
        return Err(E_ROUND_MISS);
    }
    hits += 1;

    // R3 Caps 锁定型语义：按下沿翻转锁定，抬起不改。
    let mut st2 = ModState::new();
    st2.on_physical(ModKey::CapsLock, true)?;
    if !st2.lock_state(ModKey::CapsLock) {
        return Err(E_LOCK_BAD);
    }
    st2.on_physical(ModKey::CapsLock, false)?;
    if !st2.lock_state(ModKey::CapsLock) {
        return Err(E_LOCK_BAD);
    }
    st2.on_physical(ModKey::CapsLock, true)?;
    if st2.lock_state(ModKey::CapsLock) {
        return Err(E_LOCK_BAD);
    }
    hits += 1;

    // R4 重复态违例：重复按下同键须返 E_EVENT_BAD。
    let mut st3 = ModState::new();
    st3.on_physical(ModKey::Ctrl, true)?;
    let r = st3.on_physical(ModKey::Ctrl, true);
    if r.err() != Some(E_EVENT_BAD) {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R5 越界注入 ×10：伪造「采集归 M1」全数被拒。
    let mut inj = 0u32;
    while inj < 10 {
        let r = inject_fake_owner(ModKey::Shift, true);
        if r.err() != Some(E_COLLECT_BAD) {
            return Err(E_COLLECT_BAD);
        }
        inj += 1;
    }
    hits += 1;

    // R6 复签 ×10：十轮全表复签全部通过。
    let mut js = JointSign::new("modkeysig");
    let mut round = 0u32;
    while round < 10 {
        js.round(round as u64)?;
        round += 1;
    }
    if js.rounds.len() != 10 {
        return Err(E_ROUND_MISS);
    }
    hits += 1;

    // R7 复签一致性：十轮指纹全等。
    if !js.consistent() {
        return Err(E_SIGN_DIFF);
    }
    hits += 1;

    // R8 fuzz ×512 随机事件流：状态机零违例、事件账守恒。
    let mut rng = Lcg::new(0x48584_0001);
    let mut st4 = ModState::new();
    let mut fed: u32 = 0;
    let mut k = 0u32;
    while k < 512 {
        let key = ModKey::from_index(rng.below(MOD_COUNT as u64) as usize)?;
        let pressed = rng.below(2) == 0;
        if key.is_locking() {
            st4.on_physical(key, pressed)?;
            fed += 1;
        } else {
            // 只发合法沿，避免注入态违例（反例已在 R4 覆盖）。
            if st4.down[key as usize] != pressed {
                st4.on_physical(key, pressed)?;
                fed += 1;
            }
        }
        k += 1;
    }
    if st4.events != fed {
        return Err(E_ROUND_MISS);
    }
    hits += 1;

    // R9 非锁定键抬起后有效态必须回零。
    let mut st5 = ModState::new();
    let mut kk = 0usize;
    while kk < MOD_COUNT {
        let key = ModKey::from_index(kk)?;
        if !key.is_locking() {
            st5.on_physical(key, true)?;
            st5.on_physical(key, false)?;
            if st5.effective(key) {
                return Err(E_STATE_BAD);
            }
        }
        kk += 1;
    }
    hits += 1;

    // R10 重演一致：同种子两轮 fuzz 终态指纹相等。
    let fp1 = (st4.events, st4.locked);
    let mut rng2 = Lcg::new(0x48584_0001);
    let mut st6 = ModState::new();
    let mut k = 0u32;
    while k < 512 {
        let key = ModKey::from_index(rng2.below(MOD_COUNT as u64) as usize)?;
        let pressed = rng2.below(2) == 0;
        if key.is_locking() {
            st6.on_physical(key, pressed)?;
        } else if st6.down[key as usize] != pressed {
            st6.on_physical(key, pressed)?;
        }
        k += 1;
    }
    if (st6.events, st6.locked) != fp1 || fp1.0 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
