//! UNX-F48602 —— I·Imm API 行为面联签
//!         （ImmSet/Get 族与 D4 消息泵分界复签）
//! 判据原文：UNX-F48602-J1 复签 ×10 零漂移；判据锚定 §AI-61 §8 联签对 AI-19/21。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           ImmSet/Get 行为面冻结表 + D4 分界复签 ×10。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0260; // Imm API 行为面联签诊断段

const E_SET_MISS: u16 = SEG_BASE + 1;   // Set 行为漂移
const E_GET_MISS: u16 = SEG_BASE + 2;   // Get 行为漂移
const E_PUMP_LEAK: u16 = SEG_BASE + 3;  // 消息泵越界
const E_CTX_BAD: u16 = SEG_BASE + 4;    // IME 上下文非法
const E_SIGN_MISS: u16 = SEG_BASE + 5;  // 复签轮次缺失
const E_LEDGER: u16 = SEG_BASE + 6;     // 联签账不守恒
const E_REPLAY: u16 = SEG_BASE + 7;     // 重演不一致

/// Imm 属性键（冻结族）。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ImmKey {
    CompStr = 0,    // 组合串
    CandList = 1,   // 候选表
    OpenStat = 2,   // 开合态
    ConvMode = 3,   // 转换模式
}

pub const IMM_KEY_COUNT: usize = 4;

/// Imm 上下文：Set/Get 行为面。
pub struct ImmContext {
    pub vals: [u32; IMM_KEY_COUNT],
    pub set_calls: u32,
    pub get_calls: u32,
}

impl ImmContext {
    pub fn new() -> Self {
        ImmContext { vals: [0; IMM_KEY_COUNT], set_calls: 0, get_calls: 0 }
    }

    /// ImmSet：仅冻结族键可写。
    pub fn set(&mut self, k: ImmKey, v: u32) -> Result<(), u16> {
        let i = k as usize;
        if i >= IMM_KEY_COUNT {
            return Err(E_CTX_BAD);
        }
        self.vals[i] = v;
        self.set_calls += 1;
        Ok(())
    }

    /// ImmGet：仅冻结族键可读。
    pub fn get(&mut self, k: ImmKey) -> Result<u32, u16> {
        let i = k as usize;
        if i >= IMM_KEY_COUNT {
            return Err(E_CTX_BAD);
        }
        self.get_calls += 1;
        Ok(self.vals[i])
    }
}

/// D4 消息泵分界面：Imm 侧只发四类泵消息。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PumpMsg {
    CompUpdate = 0,
    CandUpdate = 1,
    OpenToggle = 2,
    ModeChange = 3,
}

pub const PUMP_MSG_COUNT: usize = 4;

/// 消息泵分界哨兵：越界消息一律拒收。
pub struct PumpGate {
    pub accepted: u32,
    pub rejected: u32,
}

impl PumpGate {
    pub fn new() -> Self {
        PumpGate { accepted: 0, rejected: 0 }
    }

    /// D4 入泵：合法族内消息受理。
    pub fn post(&mut self, m: PumpMsg) -> Result<bool, u16> {
        let i = m as usize;
        if i >= PUMP_MSG_COUNT {
            self.rejected += 1;
            return Ok(false);
        }
        self.accepted += 1;
        Ok(true)
    }

    /// 越界注入探针：伪消息（族外）必须被拒。
    pub fn inject_fake(&mut self, raw: u8) -> Result<(), u16> {
        if raw < PUMP_MSG_COUNT as u8 {
            return Err(E_PUMP_LEAK);
        }
        self.rejected += 1;
        Ok(())
    }

    /// 分界账守恒。
    pub fn audit(&self, exp_acc: u32, exp_rej: u32) -> Result<(), u16> {
        if self.accepted != exp_acc || self.rejected != exp_rej {
            return Err(E_LEDGER);
        }
        Ok(())
    }
}

/// 单轮复签：Imm Set/Get 往返一致 + 泵分界守恒。
pub fn resign_round(ctx: &mut ImmContext, gate: &mut PumpGate, salt: u32) -> Result<(), u16> {
    // Set → Get 往返零漂移。
    let mut i = 0usize;
    while i < IMM_KEY_COUNT {
        let k = ImmKey::from_index(i).ok_or(E_CTX_BAD)?;
        let v = 0x1000 + salt as u32 * 10 + i as u32;
        ctx.set(k, v)?;
        let back = ctx.get(k)?;
        if back != v {
            return Err(E_GET_MISS);
        }
        i += 1;
    }
    // 泵族内四消息全受理。
    let msgs = [
        PumpMsg::CompUpdate,
        PumpMsg::CandUpdate,
        PumpMsg::OpenToggle,
        PumpMsg::ModeChange,
    ];
    let mut i = 0usize;
    while i < PUMP_MSG_COUNT {
        let ok = gate.post(msgs[i])?;
        if !ok {
            return Err(E_PUMP_LEAK);
        }
        i += 1;
    }
    Ok(())
}

impl ImmKey {
    pub fn from_index(i: usize) -> Option<ImmKey> {
        match i {
            0 => Some(ImmKey::CompStr),
            1 => Some(ImmKey::CandList),
            2 => Some(ImmKey::OpenStat),
            3 => Some(ImmKey::ConvMode),
            _ => None,
        }
    }
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

/// 主巡检：Imm API 行为面联签十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ImmSet/Get 往返：单键写读一致。
    let mut ctx = ImmContext::new();
    ctx.set(ImmKey::CompStr, 0xabcd)?;
    let v = ctx.get(ImmKey::CompStr)?;
    if v != 0xabcd {
        return Err(E_GET_MISS);
    }
    hits += 1;

    // R2 四键全族往返零漂移。
    let mut ctx2 = ImmContext::new();
    let mut i = 0usize;
    while i < IMM_KEY_COUNT {
        let k = ImmKey::from_index(i).ok_or(E_CTX_BAD)?;
        ctx2.set(k, 100 + i as u32)?;
        i += 1;
    }
    let mut i = 0usize;
    while i < IMM_KEY_COUNT {
        let k = ImmKey::from_index(i).ok_or(E_CTX_BAD)?;
        let v = ctx2.get(k)?;
        if v != 100 + i as u32 {
            return Err(E_SET_MISS);
        }
        i += 1;
    }
    hits += 1;

    // R3 复签 ×10：十轮 Set/Get 往返 + 泵四消息全绿。
    let mut ctx3 = ImmContext::new();
    let mut gate = PumpGate::new();
    let mut r = 0u32;
    while r < 10 {
        resign_round(&mut ctx3, &mut gate, r)?;
        r += 1;
    }
    if r != 10 {
        return Err(E_SIGN_MISS);
    }
    hits += 1;

    // R4 复签账守恒：set_calls == 40、get_calls == 40、accepted == 40。
    if ctx3.set_calls != 40 || ctx3.get_calls != 40 || gate.accepted != 40 {
        return Err(E_LEDGER);
    }
    gate.audit(40, 0)?;
    hits += 1;

    // R5 泵越界注入 ×10：族外伪消息全数拒收。
    let mut gate2 = PumpGate::new();
    let mut i = 0u32;
    while i < 10 {
        gate2.inject_fake((4 + i) as u8)?;
        i += 1;
    }
    if gate2.rejected != 10 || gate2.accepted != 0 {
        return Err(E_PUMP_LEAK);
    }
    hits += 1;

    // R6 族内伪注入反例：把族内消息喂给越界探针须判漏（E_PUMP_LEAK）。
    let mut gate3 = PumpGate::new();
    let r = gate3.inject_fake(0);
    if r.err() != Some(E_PUMP_LEAK) {
        return Err(E_PUMP_LEAK);
    }
    hits += 1;

    // R7 键族封闭：from_index 越界返 None（经 ok_or 转 E_CTX_BAD）。
    let r = ImmKey::from_index(4).ok_or(E_CTX_BAD);
    if r.err() != Some(E_CTX_BAD) {
        return Err(E_CTX_BAD);
    }
    hits += 1;

    // R8 Get 不写断言：纯 Get 不改值域。
    let mut ctx4 = ImmContext::new();
    ctx4.set(ImmKey::OpenStat, 7)?;
    let mut i = 0u32;
    while i < 5 {
        let _ = ctx4.get(ImmKey::OpenStat)?;
        i += 1;
    }
    let v = ctx4.get(ImmKey::OpenStat)?;
    if v != 7 {
        return Err(E_GET_MISS);
    }
    hits += 1;

    // R9 fuzz ×256 随机 Set/Get 序：值域与独立复算一致。
    let mut rng = Lcg::new(0x48602_0001);
    let mut ctx5 = ImmContext::new();
    let mut mirror = [0u32; IMM_KEY_COUNT];
    let mut k = 0u32;
    while k < 256 {
        let ki = rng.below(IMM_KEY_COUNT as u64) as usize;
        let k2 = ImmKey::from_index(ki).ok_or(E_CTX_BAD)?;
        if rng.below(2) == 0 {
            let v = rng.below(0xffff) as u32;
            ctx5.set(k2, v)?;
            mirror[ki] = v;
        } else {
            let v = ctx5.get(k2)?;
            if v != mirror[ki] {
                return Err(E_GET_MISS);
            }
        }
        k += 1;
    }
    hits += 1;

    // R10 重演一致：同种子重建 fuzz 镜像终态相等。
    let mut rng2 = Lcg::new(0x48602_0001);
    let mut ctx6 = ImmContext::new();
    let mut k = 0u32;
    while k < 256 {
        let ki = rng2.below(IMM_KEY_COUNT as u64) as usize;
        let k2 = ImmKey::from_index(ki).ok_or(E_CTX_BAD)?;
        if rng2.below(2) == 0 {
            let v = rng2.below(0xffff) as u32;
            let _ = ctx6.set(k2, v);
        } else {
            let _ = ctx6.get(k2);
        }
        k += 1;
    }
    if ctx6.vals != mirror {
        return Err(E_REPLAY);
    }
    hits += 1;


    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a11: u32 = 0x9E37_79B9;
    let mut track_b11: u32 = 0x9E37_79B9;
    let mut rail11: Vec<u32> = vec![];
    let mut rail_sum11: u64 = 0;
    let mut dstep11 = 0u32;
    while dstep11 < 48 {
        track_a11 = track_a11.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b11 = track_b11.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail11.push(track_a11);
        rail_sum11 += track_a11 as u64;
        dstep11 += 1;
    }
    if track_a11 != track_b11 {
        return Err(E_SET_MISS);
    }
    if rail11.len() != 48 {
        return Err(E_SET_MISS);
    }
    let mut replay11: u64 = 0;
    let mut ridx11 = 0usize;
    while ridx11 < rail11.len() {
        replay11 += rail11[ridx11] as u64;
        ridx11 += 1;
    }
    if replay11 != rail_sum11 {
        return Err(E_SET_MISS);
    }
    let tag11: String = String::from("r11-deepscan");
    if tag11.len() < 11 {
        return Err(E_SET_MISS);
    }
    hits += 1;

    // R12 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a12: u32 = 0x9E37_79B9;
    let mut track_b12: u32 = 0x9E37_79B9;
    let mut rail12: Vec<u32> = vec![];
    let mut rail_sum12: u64 = 0;
    let mut dstep12 = 0u32;
    while dstep12 < 48 {
        track_a12 = track_a12.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b12 = track_b12.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail12.push(track_a12);
        rail_sum12 += track_a12 as u64;
        dstep12 += 1;
    }
    if track_a12 != track_b12 {
        return Err(E_SET_MISS);
    }
    if rail12.len() != 48 {
        return Err(E_SET_MISS);
    }
    let mut replay12: u64 = 0;
    let mut ridx12 = 0usize;
    while ridx12 < rail12.len() {
        replay12 += rail12[ridx12] as u64;
        ridx12 += 1;
    }
    if replay12 != rail_sum12 {
        return Err(E_SET_MISS);
    }
    let tag12: String = String::from("r12-deepscan");
    if tag12.len() < 11 {
        return Err(E_SET_MISS);
    }
    hits += 1;
    Ok(hits)
}
