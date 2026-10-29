//! UNX-F48764 C·AI-62 参演证据件（按键流侧证据归档）
//! 判据：UNX-F48764-J1 证据归档全绿；判据锚定 AI-62 联签。
//! 纪律：证据登记-指纹-归档三段闭环，指纹复算一致、序守恒、归档冻结不可逆。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1D80;

const E_SEQ_BAD: u16 = SEG_BASE + 1; // 事件序非法
const E_KIND_BAD: u16 = SEG_BASE + 2; // 种类越界
const E_FP_DIFF: u16 = SEG_BASE + 3; // 指纹漂移
const E_ARCH_DUP: u16 = SEG_BASE + 4; // 归档重复
const E_SEQ_GAP: u16 = SEG_BASE + 5; // 序断档
const E_EMPTY_EV: u16 = SEG_BASE + 6; // 空证据拒
const E_BAL_BREAK: u16 = SEG_BASE + 7; // 归档账守恒破
const E_FROZEN_TOUCH: u16 = SEG_BASE + 8; // 冻结后改

/// 事件种类白名单。
pub const K_DOWN: u8 = 1;
pub const K_UP: u8 = 2;
pub const K_REPEAT: u8 = 3;

/// 证据条目。
#[derive(Clone)]
pub struct EvItem {
    /// 事件序（1..N 连续）。
    pub seq: u64,
    /// 种类。
    pub kind: u8,
    /// 键码。
    pub code: u32,
    /// 指纹。
    pub fp: u32,
}

/// 证据归档簿。
pub struct EvArchive {
    items: Vec<EvItem>,
    frozen: bool,
    frozen_n: u32,
}

impl EvArchive {
    pub fn new() -> EvArchive {
        EvArchive {
            items: Vec::new(),
            frozen: false,
            frozen_n: 0,
        }
    }

    /// 指纹：seq/kind/code 联合 FNV。
    pub fn fp_of(seq: u64, kind: u8, code: u32) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= seq as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= kind as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= code;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 登记证据（序连续、种类白名单、码非零）。
    pub fn put(&mut self, seq: u64, kind: u8, code: u32) -> Result<u32, u16> {
        if self.frozen {
            return Err(E_FROZEN_TOUCH);
        }
        if code == 0 {
            return Err(E_EMPTY_EV);
        }
        if kind == 0 || kind > K_REPEAT {
            return Err(E_KIND_BAD);
        }
        if let Some(last) = self.items.last() {
            if seq != last.seq + 1 {
                if seq == last.seq {
                    return Err(E_ARCH_DUP);
                }
                return Err(E_SEQ_GAP);
            }
        } else if seq != 1 {
            return Err(E_SEQ_BAD);
        }
        let fp = EvArchive::fp_of(seq, kind, code);
        self.items.push(EvItem { seq, kind, code, fp });
        Ok(fp)
    }

    /// 指纹复算对拍：全部条目一致。
    pub fn assert_fps(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        for it in self.items.iter() {
            if EvArchive::fp_of(it.seq, it.kind, it.code) != it.fp {
                return Err(E_FP_DIFF);
            }
            n += 1;
        }
        Ok(n)
    }

    /// 序守恒：1..N 连续无断档。
    pub fn assert_seq(&self) -> Result<(), u16> {
        let mut want = 1u64;
        for it in self.items.iter() {
            if it.seq != want {
                return Err(E_SEQ_GAP);
            }
            want += 1;
        }
        Ok(())
    }

    /// 抽查一条：指纹对拍。
    pub fn check_one(&self, seq: u64) -> Result<u32, u16> {
        for it in self.items.iter() {
            if it.seq == seq {
                if EvArchive::fp_of(it.seq, it.kind, it.code) != it.fp {
                    return Err(E_FP_DIFF);
                }
                return Ok(it.fp);
            }
        }
        Err(E_EMPTY_EV)
    }

    /// 冻结归档。
    pub fn freeze(&mut self) -> Result<u32, u16> {
        let n = self.assert_fps()?;
        self.assert_seq()?;
        self.frozen = true;
        self.frozen_n = n;
        Ok(n)
    }

    /// 归档账守恒。
    pub fn assert_balance(&self) -> Result<(), u16> {
        if !self.frozen {
            return Err(E_BAL_BREAK);
        }
        if self.frozen_n != self.items.len() as u32 {
            return Err(E_BAL_BREAK);
        }
        Ok(())
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n.max(1)
    }
}

/// 参演证据归档主判据：按键流证据登记-指纹-归档全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 LCG 事件流 ×128 登记
    let mut ar = EvArchive::new();
    let mut rng = Lcg::new(0x48764);
    let mut s = 1u64;
    while s <= 128 {
        let kind = (rng.below(3) + 1) as u8;
        let code = (rng.below(200) + 1) as u32;
        ar.put(s, kind, code)?;
        s += 1;
    }
    let n = ar.assert_fps()?;
    if n != 128 {
        return Err(E_FP_DIFF);
    }
    ar.assert_seq()?;
    hits += 1;

    // R2 序断档必拒
    if ar.put(999, K_DOWN, 5).err() == Some(E_SEQ_GAP) {
        hits += 1;
    } else {
        return Err(E_SEQ_GAP);
    }

    // R3 序重复必拒
    if ar.put(128, K_UP, 5).err() == Some(E_ARCH_DUP) {
        hits += 1;
    } else {
        return Err(E_ARCH_DUP);
    }

    // R4 种类越界必拒
    if ar.put(129, 9, 5).err() == Some(E_KIND_BAD) {
        hits += 1;
    } else {
        return Err(E_KIND_BAD);
    }

    // R5 零码必拒
    if ar.put(129, K_DOWN, 0).err() == Some(E_EMPTY_EV) {
        hits += 1;
    } else {
        return Err(E_EMPTY_EV);
    }

    // R6 首序非 1 必拒
    let mut ar2 = EvArchive::new();
    if ar2.put(2, K_DOWN, 5).err() == Some(E_SEQ_BAD) {
        hits += 1;
    } else {
        return Err(E_SEQ_BAD);
    }

    // R7 抽查 ×256：指纹对拍恒绿
    let mut k = 0u32;
    while k < 256 {
        let pick = (rng.below(128) + 1) as u64;
        ar.check_one(pick)?;
        k += 1;
    }
    hits += 1;

    // R8 指纹漂移必抓：同三元组不同 seq 指纹必不同
    let f1 = EvArchive::fp_of(1, K_DOWN, 42);
    let f2 = EvArchive::fp_of(2, K_DOWN, 42);
    if f1 == f2 {
        return Err(E_FP_DIFF);
    }
    let f3 = EvArchive::fp_of(1, K_UP, 42);
    if f1 == f3 {
        return Err(E_FP_DIFF);
    }
    hits += 1;

    // R9 冻结归档 + 账守恒
    let fn_ = ar.freeze()?;
    if fn_ != 128 {
        return Err(E_BAL_BREAK);
    }
    ar.assert_balance()?;
    hits += 1;

    // R10 冻结后登记必拒
    if ar.put(129, K_DOWN, 5).err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }

    // R11 按键序列语义链：DOWN→UP 成对账守恒
    let mut ar3 = EvArchive::new();
    let mut s3 = 1u64;
    let mut pairs = 0u32;
    while pairs < 32 {
        ar3.put(s3, K_DOWN, 0x41 + pairs)?;
        s3 += 1;
        ar3.put(s3, K_UP, 0x41 + pairs)?;
        s3 += 1;
        pairs += 1;
    }
    let mut down_n = 0u32;
    let mut up_n = 0u32;
    for it in ar3.items.iter() {
        if it.kind == K_DOWN {
            down_n += 1;
        }
        if it.kind == K_UP {
            up_n += 1;
        }
    }
    if down_n != up_n || down_n != 32 {
        return Err(E_BAL_BREAK);
    }
    ar3.assert_fps()?;
    hits += 1;

    // R12 LCG 混合流 ×64 复归档对拍：双簿同种子同指纹序列
    let seed12 = 0x48764_0012;
    let mut a1 = EvArchive::new();
    let mut a2 = EvArchive::new();
    let mut r1 = Lcg(seed12);
    let mut s4 = 1u64;
    while s4 <= 64 {
        let kd = (r1.below(3) + 1) as u8;
        let cd = (r1.below(100) + 1) as u32;
        let f1 = a1.put(s4, kd, cd)?;
        let f2 = a2.put(s4, kd, cd)?;
        if f1 != f2 {
            return Err(E_FP_DIFF);
        }
        s4 += 1;
    }
    let n1 = a1.assert_fps()?;
    let n2 = a2.assert_fps()?;
    if n1 != n2 {
        return Err(E_BAL_BREAK);
    }
    hits += 1;

    Ok(hits)
}
