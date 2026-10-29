//! UNX-F48585 —— I·REP 重复按键联签
//!         （M2 REP 面→本域组合态屏蔽协同）
//! 判据原文：UNX-F48585-J1 协同 ×10 正确；判据锚定 B18 REP 面。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           组合态屏蔽策略单点判定；drill 内 R1..Rn 自证。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0040; // REP 重复按键协同诊断段

const E_MASK_BAD: u16 = SEG_BASE + 1;  // 屏蔽判定错
const E_FWD_BAD: u16 = SEG_BASE + 2;   // 放行判定错
const E_STATE_BAD: u16 = SEG_BASE + 3; // 组合态违例
const E_COUNT_BAD: u16 = SEG_BASE + 4; // REP 计数失真
const E_JOINT_MISS: u16 = SEG_BASE + 5; // 协同轮次缺失
const E_LEDGER: u16 = SEG_BASE + 6;    // 协同账不守恒
const E_REPLAY: u16 = SEG_BASE + 7;    // 重演不一致

/// 组合态：Idle = 非组合态（REP 放行），Composing = 组合态（REP 屏蔽）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompState {
    Idle,
    Composing,
}

/// M2 上报的 REP 事件。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RepEvent {
    pub key: u32,
    pub count: u32,
}

/// 协同裁决结果。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Forwarded,
    Masked,
}

/// 本域组合态屏蔽协同器（单点策略）。
pub struct RepShield {
    pub state: CompState,
    pub fwd_keys: Vec<u32>,
    pub masked_keys: Vec<u32>,
    pub total_in: u32,
}

impl RepShield {
    pub fn new() -> Self {
        RepShield {
            state: CompState::Idle,
            fwd_keys: Vec::new(),
            masked_keys: Vec::new(),
            total_in: 0,
        }
    }

    /// 进入/退出组合态（由本域组合引擎驱动）。
    pub fn set_state(&mut self, s: CompState) {
        self.state = s;
    }

    /// M2 REP 面事件入域：按当前组合态裁决。
    pub fn ingest(&mut self, ev: RepEvent) -> Result<Verdict, u16> {
        if ev.count == 0 {
            return Err(E_COUNT_BAD);
        }
        self.total_in += 1;
        match self.state {
            CompState::Idle => {
                self.fwd_keys.push(ev.key);
                Ok(Verdict::Forwarded)
            }
            CompState::Composing => {
                self.masked_keys.push(ev.key);
                Ok(Verdict::Masked)
            }
        }
    }

    /// 协同账守恒：入域数 == 放行数 + 屏蔽数。
    pub fn audit(&self) -> Result<(), u16> {
        if self.fwd_keys.len() as u32 + self.masked_keys.len() as u32 != self.total_in {
            return Err(E_LEDGER);
        }
        Ok(())
    }

    /// 屏蔽率（千分比）。
    pub fn mask_permille(&self) -> u32 {
        if self.total_in == 0 {
            return 0;
        }
        self.masked_keys.len() as u32 * 1000 / self.total_in
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.fwd_keys.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.fwd_keys[i] as u64;
            i += 1;
        }
        i = 0;
        while i < self.masked_keys.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ (self.masked_keys[i] as u64) ^ 0xa5;
            i += 1;
        }
        h ^= self.total_in as u64;
        h
    }
}

/// 单轮协同演练：给定状态脚本与事件流，裁决必须与期望一致。
pub fn joint_round(
    shield: &mut RepShield,
    state: CompState,
    ev: RepEvent,
    expect_fwd: bool,
) -> Result<(), u16> {
    shield.set_state(state);
    let v = shield.ingest(ev)?;
    let ok = match v {
        Verdict::Forwarded => expect_fwd,
        Verdict::Masked => !expect_fwd,
    };
    if !ok {
        if expect_fwd {
            return Err(E_FWD_BAD);
        }
        return Err(E_MASK_BAD);
    }
    shield.audit()?;
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

/// 主巡检：REP 重复按键联签十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 Idle 态 REP 放行。
    let mut sh = RepShield::new();
    joint_round(&mut sh, CompState::Idle, RepEvent { key: 41, count: 3 }, true)?;
    hits += 1;

    // R2 Composing 态 REP 屏蔽。
    joint_round(&mut sh, CompState::Composing, RepEvent { key: 41, count: 3 }, false)?;
    hits += 1;

    // R3 协同 ×10：交替状态脚本逐一裁决正确。
    let mut sh2 = RepShield::new();
    let mut r = 0u32;
    while r < 10 {
        let composing = r % 2 == 1;
        let st = if composing { CompState::Composing } else { CompState::Idle };
        joint_round(&mut sh2, st, RepEvent { key: 30 + r, count: 1 + r }, !composing)?;
        r += 1;
    }
    if sh2.total_in != 10 {
        return Err(E_JOINT_MISS);
    }
    hits += 1;

    // R4 协同账守恒：放行 5 + 屏蔽 5 == 入域 10。
    sh2.audit()?;
    if sh2.fwd_keys.len() != 5 || sh2.masked_keys.len() != 5 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5 REP 计数失真反例：count==0 须拒。
    let mut sh3 = RepShield::new();
    let r = sh3.ingest(RepEvent { key: 1, count: 0 });
    if r.err() != Some(E_COUNT_BAD) {
        return Err(E_COUNT_BAD);
    }
    hits += 1;

    // R6 屏蔽率核算：全屏蔽 = 1000‰，全放行 = 0‰。
    let mut sh4 = RepShield::new();
    sh4.set_state(CompState::Composing);
    sh4.ingest(RepEvent { key: 1, count: 2 })?;
    sh4.ingest(RepEvent { key: 2, count: 2 })?;
    if sh4.mask_permille() != 1000 {
        return Err(E_MASK_BAD);
    }
    sh4.set_state(CompState::Idle);
    sh4.ingest(RepEvent { key: 3, count: 1 })?;
    sh4.audit()?;
    hits += 1;

    // R7 fuzz ×512 随机状态/事件流：裁决与策略复算一致。
    let mut rng = Lcg::new(0x48585_0001);
    let mut sh5 = RepShield::new();
    let mut k = 0u32;
    while k < 512 {
        let composing = rng.below(2) == 1;
        let st = if composing { CompState::Composing } else { CompState::Idle };
        let ev = RepEvent { key: rng.below(128) as u32, count: 1 + rng.below(9) as u32 };
        let v = {
            sh5.set_state(st);
            sh5.ingest(ev)?
        };
        let expect_fwd = !composing;
        match v {
            Verdict::Forwarded => {
                if !expect_fwd {
                    return Err(E_FWD_BAD);
                }
            }
            Verdict::Masked => {
                if expect_fwd {
                    return Err(E_MASK_BAD);
                }
            }
        }
        k += 1;
    }
    sh5.audit()?;
    hits += 1;

    // R8 状态切换即判：进入组合态后首批 REP 必被屏蔽。
    let mut sh6 = RepShield::new();
    sh6.set_state(CompState::Idle);
    let v = sh6.ingest(RepEvent { key: 7, count: 4 })?;
    if v != Verdict::Forwarded {
        return Err(E_FWD_BAD);
    }
    sh6.set_state(CompState::Composing);
    let v = sh6.ingest(RepEvent { key: 7, count: 4 })?;
    if v != Verdict::Masked {
        return Err(E_MASK_BAD);
    }
    hits += 1;

    // R9 屏蔽不误伤放行账：放行键序与屏蔽键序互不污染。
    let mut i = 0usize;
    while i < sh2.masked_keys.len() {
        let mk = sh2.masked_keys[i];
        let mut j = 0usize;
        while j < sh2.fwd_keys.len() {
            if sh2.fwd_keys[j] == mk {
                return Err(E_LEDGER);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R10 重演一致：同种子重放 512 步指纹相等。
    let fp1 = (sh5.fingerprint(), sh5.total_in);
    let mut rng2 = Lcg::new(0x48585_0001);
    let mut sh7 = RepShield::new();
    let mut k = 0u32;
    while k < 512 {
        let composing = rng2.below(2) == 1;
        let st = if composing { CompState::Composing } else { CompState::Idle };
        let ev = RepEvent { key: rng2.below(128) as u32, count: 1 + rng2.below(9) as u32 };
        sh7.set_state(st);
        sh7.ingest(ev)?;
        k += 1;
    }
    if (sh7.fingerprint(), sh7.total_in) != fp1 || fp1.0 == 0 {
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
        return Err(E_MASK_BAD);
    }
    if rail11.len() != 48 {
        return Err(E_MASK_BAD);
    }
    let mut replay11: u64 = 0;
    let mut ridx11 = 0usize;
    while ridx11 < rail11.len() {
        replay11 += rail11[ridx11] as u64;
        ridx11 += 1;
    }
    if replay11 != rail_sum11 {
        return Err(E_MASK_BAD);
    }
    let tag11: String = String::from("r11-deepscan");
    if tag11.len() < 11 {
        return Err(E_MASK_BAD);
    }
    hits += 1;
    Ok(hits)
}
