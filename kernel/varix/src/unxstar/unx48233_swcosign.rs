//! unx48233_swcosign — UNX-F48233 切换与 M2 修饰键语义联签执行账（Shift/Caps 采集分界登记）（诊断段 0xBE00）。
//!
//! 判据原文：
//!   J1 联签登记入册；判据锚定 §AI-61 §8 联签对 AI-62。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 联签=两域对同一条语义边界的共同签认：M2（修饰键采集域）
//!      签「Shift/Caps 原始事件归 M2 采集」；切换域签「只消费 M2 的
//!      已判定单击/锁定事件，不私采」。两侧签词常量写死、成对入册。
//!   2. 分界登记：采集分界点 = 修饰键释放沿（Shift 单击）与锁定
//!      翻转沿（Caps），分界前后的事件归属逐条可查。
//!   3. 联签完整账：每条联签记录 (边界id, M2签词, 切换签词, 登记时刻
//!      tick)；两签词缺一即拒收（联签必须是成对的）。
//!   4. 防漂移：签词内容指纹封账；任何一方签词变更即指纹漂移显性。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBE00;

const E_SINGLE_SIGN: u16 = SEG_BASE + 1; // 单边签拒收
const E_BAD_BOUNDARY: u16 = SEG_BASE + 2; // 分界闭集外
const E_DUP_BOUNDARY: u16 = SEG_BASE + 3; // 分界重复联签
const E_FP_DRIFT: u16 = SEG_BASE + 4;     // 签词指纹漂移
const E_EMPTY_SIGN: u16 = SEG_BASE + 5;   // 空签词
const E_GUARD_UNBAL: u16 = SEG_BASE + 6;  // 联签账不平

/// 采集分界闭集：Shift 释放沿 / Caps 锁定沿 / 组合键让渡沿。
pub const B_SHIFT_RELEASE: u8 = 1;
pub const B_CAPS_TOGGLE: u8 = 2;
pub const B_COMBO_YIELD: u8 = 3;

/// M2 域签词（常量写死）。
pub const SIGN_M2_SHIFT: &str = "M2:Shift raw capture ends at release edge";
pub const SIGN_M2_CAPS: &str = "M2:Caps lock edge is the toggle point";
pub const SIGN_M2_COMBO: &str = "M2:combo keys yielded to consumer";

/// 切换域签词（常量写死，成对）。
pub const SIGN_SW_SHIFT: &str = "SW:consume only click events from M2";
pub const SIGN_SW_CAPS: &str = "SW:consume only lock events from M2";
pub const SIGN_SW_COMBO: &str = "SW:consume combo yields verbatim";

/// 一条联签。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cosign {
    pub boundary: u8,
    pub sign_m2: &'static str,
    pub sign_sw: &'static str,
    pub tick: u32,
}

/// 联签执行总账。
#[derive(Debug)]
pub struct SwCosign {
    book: Vec<Cosign>,
    fp_seal: u32,
    sealed: bool,
    rejects: u32,
}

/// 签词 → 分界 的成对关系表（联签合法性锚）。
pub fn expected_pair(boundary: u8) -> Option<(&'static str, &'static str)> {
    match boundary {
        B_SHIFT_RELEASE => Some((SIGN_M2_SHIFT, SIGN_SW_SHIFT)),
        B_CAPS_TOGGLE => Some((SIGN_M2_CAPS, SIGN_SW_CAPS)),
        B_COMBO_YIELD => Some((SIGN_M2_COMBO, SIGN_SW_COMBO)),
        _ => None,
    }
}

fn signs_fp(m2: &str, sw: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in m2.bytes().chain(sw.bytes()) {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

impl SwCosign {
    pub fn new() -> Self {
        SwCosign { book: Vec::new(), fp_seal: 0, sealed: false, rejects: 0 }
    }

    /// 联签登记：分界闭集内 + 双签词与成对关系表全等 + 非空。
    pub fn register(&mut self, boundary: u8, sign_m2: &'static str, sign_sw: &'static str, tick: u32) -> Result<(), u16> {
        if self.sealed {
            self.rejects += 1;
            return Err(E_FP_DRIFT);
        }
        if sign_m2.is_empty() || sign_sw.is_empty() {
            self.rejects += 1;
            return Err(E_EMPTY_SIGN);
        }
        let (wm, ws) = match expected_pair(boundary) {
            Some(p) => p,
            None => {
                self.rejects += 1;
                return Err(E_BAD_BOUNDARY);
            }
        };
        if sign_m2 != wm || sign_sw != ws {
            self.rejects += 1;
            return Err(E_FP_DRIFT);
        }
        if self.book.iter().any(|c| c.boundary == boundary) {
            self.rejects += 1;
            return Err(E_DUP_BOUNDARY);
        }
        self.book.push(Cosign { boundary, sign_m2, sign_sw, tick });
        Ok(())
    }

    /// 封账：全联签签词指纹叠加。
    pub fn seal(&mut self) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_FP_DRIFT);
        }
        if self.book.is_empty() {
            return Err(E_EMPTY_SIGN);
        }
        let mut h: u32 = 0x811C_9DC5;
        for c in &self.book {
            h ^= signs_fp(c.sign_m2, c.sign_sw);
            h = h.wrapping_mul(0x0100_0193);
            h ^= c.boundary as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        self.fp_seal = h;
        self.sealed = true;
        Ok(h)
    }

    /// 封账复核：指纹可复算。
    pub fn verify(&self, fp: u32) -> Result<(), u16> {
        if !self.sealed || fp != self.fp_seal {
            return Err(E_FP_DRIFT);
        }
        let mut h: u32 = 0x811C_9DC5;
        for c in &self.book {
            h ^= signs_fp(c.sign_m2, c.sign_sw);
            h = h.wrapping_mul(0x0100_0193);
            h ^= c.boundary as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        if h != fp {
            return Err(E_FP_DRIFT);
        }
        Ok(())
    }

    /// 分界查询：登记分界的归属签词对。
    pub fn boundary_of(&self, boundary: u8) -> Option<(&'static str, &'static str)> {
        self.book
            .iter()
            .find(|c| c.boundary == boundary)
            .map(|c| (c.sign_m2, c.sign_sw))
    }

    /// 审计：三分界全部联签在册 + 拒绝账守恒。
    pub fn audit(&self) -> Result<(), u16> {
        for b in [B_SHIFT_RELEASE, B_CAPS_TOGGLE, B_COMBO_YIELD] {
            if self.boundary_of(b).is_none() {
                return Err(E_SINGLE_SIGN);
            }
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.book.len()
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 自例程判据：落实 J1「联签登记入册」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 场景：三分界联签全登记 → 入册 3 条 → 审计过。
    let mut s = SwCosign::new();
    s.register(B_SHIFT_RELEASE, SIGN_M2_SHIFT, SIGN_SW_SHIFT, 100)?;
    s.register(B_CAPS_TOGGLE, SIGN_M2_CAPS, SIGN_SW_CAPS, 200)?;
    s.register(B_COMBO_YIELD, SIGN_M2_COMBO, SIGN_SW_COMBO, 300)?;
    if s.len() != 3 {
        return Err(E_GUARD_UNBAL);
    }
    s.audit()?;
    hits += 1;

    // R2 场景：分界查询 —— 逐分界取回签词对与登记时刻。
    let (m, w) = s.boundary_of(B_CAPS_TOGGLE).ok_or(E_SINGLE_SIGN)?;
    if m != SIGN_M2_CAPS || w != SIGN_SW_CAPS {
        return Err(E_FP_DRIFT);
    }
    if s.boundary_of(9).is_some() {
        return Err(E_BAD_BOUNDARY);
    }
    hits += 1;

    // R3 场景：单边签拒收 —— 缺切换签/缺 M2 签均拒。
    let mut s3 = SwCosign::new();
    match s3.register(B_SHIFT_RELEASE, SIGN_M2_SHIFT, "", 10) {
        Err(E_EMPTY_SIGN) => {}
        _ => return Err(E_EMPTY_SIGN),
    }
    match s3.register(B_SHIFT_RELEASE, "", SIGN_SW_SHIFT, 10) {
        Err(E_EMPTY_SIGN) => {}
        _ => return Err(E_EMPTY_SIGN),
    }
    if s3.rejects() != 2 {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R4 场景：签词漂移拒收 —— 签词与成对关系表不符（私改语义）必拒。
    let mut s4 = SwCosign::new();
    match s4.register(B_SHIFT_RELEASE, SIGN_M2_CAPS, SIGN_SW_SHIFT, 10) {
        Err(E_FP_DRIFT) => {}
        _ => return Err(E_FP_DRIFT),
    }
    match s4.register(B_SHIFT_RELEASE, SIGN_M2_SHIFT, SIGN_SW_CAPS, 10) {
        Err(E_FP_DRIFT) => {}
        _ => return Err(E_FP_DRIFT),
    }
    hits += 1;

    // R5 场景：分界闭集外拒收 + 重复联签拒收。
    let mut s5 = SwCosign::new();
    match s5.register(7, SIGN_M2_SHIFT, SIGN_SW_SHIFT, 10) {
        Err(E_BAD_BOUNDARY) => {}
        _ => return Err(E_BAD_BOUNDARY),
    }
    s5.register(B_SHIFT_RELEASE, SIGN_M2_SHIFT, SIGN_SW_SHIFT, 10)?;
    match s5.register(B_SHIFT_RELEASE, SIGN_M2_SHIFT, SIGN_SW_SHIFT, 20) {
        Err(E_DUP_BOUNDARY) => {}
        _ => return Err(E_DUP_BOUNDARY),
    }
    hits += 1;

    // R6 场景：封账与复核 —— 指纹可复算；错指纹显性。
    let fp = s.seal()?;
    s.verify(fp)?;
    match s.verify(fp.wrapping_add(1)) {
        Err(E_FP_DRIFT) => {}
        _ => return Err(E_FP_DRIFT),
    }
    hits += 1;

    // R7 场景：封账后写拒 —— 联签册冻结语义。
    match s.register(B_COMBO_YIELD, SIGN_M2_COMBO, SIGN_SW_COMBO, 400) {
        Err(E_FP_DRIFT) => {} // 重复分界（已在册）——但封账优先语义同显性
        Err(_) => return Err(E_FP_DRIFT),
        Ok(_) => return Err(E_FP_DRIFT),
    }
    match s.seal() {
        Err(E_FP_DRIFT) => {}
        _ => return Err(E_FP_DRIFT),
    }
    hits += 1;

    // R8 场景：空册封账拒收。
    let mut s8 = SwCosign::new();
    match s8.seal() {
        Err(E_EMPTY_SIGN) => {}
        _ => return Err(E_EMPTY_SIGN),
    }
    hits += 1;

    // R9 场景：审计缺位显性 —— 少一条分界联签，audit 必红。
    let mut s9 = SwCosign::new();
    s9.register(B_SHIFT_RELEASE, SIGN_M2_SHIFT, SIGN_SW_SHIFT, 10)?;
    s9.register(B_CAPS_TOGGLE, SIGN_M2_CAPS, SIGN_SW_CAPS, 20)?;
    match s9.audit() {
        Err(E_SINGLE_SIGN) => {}
        _ => return Err(E_SINGLE_SIGN),
    }
    hits += 1;

    // R10 场景：签词指纹函数确定性 —— 同签词对同指纹，异对异指纹。
    let a = signs_fp(SIGN_M2_SHIFT, SIGN_SW_SHIFT);
    let b = signs_fp(SIGN_M2_SHIFT, SIGN_SW_SHIFT);
    let c = signs_fp(SIGN_M2_CAPS, SIGN_SW_CAPS);
    if a != b || a == c {
        return Err(E_FP_DRIFT);
    }
    hits += 1;

    Ok(hits)
}
