//! UNX-F48697 —— I·J3 密钥轮换预案登记（轮换时词库重加密路径预告）
//! 判据原文：UNX-F48697-J1 预案在册禁实装声明（§40）；判据锚定 §40
//! 幻觉红线。
//! 判据落地（drill 场景）：R1 预案在册；R2 禁实装声明在位；
//! R3 实装尝试拒收；R4 路径预告字段齐；R5 预案指纹；R6 重复登记拒；
//! R7 查询；R8 呈现行；R9 §40 锚定；R10 指纹稳定。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0E80;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_PLAN: u16 = SEG_BASE + 1; // 预案未在册
const E_IMPL: u16 = SEG_BASE + 2; // 禁实装声明缺失
const E_ATTEMPT: u16 = SEG_BASE + 3; // 实装尝试未被拒
const E_FIELD: u16 = SEG_BASE + 4; // 路径预告字段缺失
const E_DUP: u16 = SEG_BASE + 5; // 重复登记
const E_NOTFOUND: u16 = SEG_BASE + 6; // 查无预案
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_ANCHOR: u16 = SEG_BASE + 9; // §40 锚缺失

/// 预案编号（冻结）。
pub const PLAN_ID: u32 = 9_697;
/// §40 幻觉红线锚（预案只登记不实装）。
pub const ANCHOR_S40: &str = "S40-NOIMPL";

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 密钥轮换预案（登记态，禁实装）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RotatePlan {
    pub plan_id: u32,
    pub anchor: String,       // §40 锚，恒为 ANCHOR_S40
    pub implemented: bool,    // 恒 false（禁实装）
    pub path_userlex: String, // 用户词库重加密路径预告
    pub path_cfg: String,     // 配置重加密路径预告
}

impl RotatePlan {
    /// 构造登记态预案：implemented 恒 false。
    pub fn declare() -> RotatePlan {
        RotatePlan {
            plan_id: PLAN_ID,
            anchor: ANCHOR_S40.to_string(),
            implemented: false,
            path_userlex: "re-encrypt:userlex@next-rotation".to_string(),
            path_cfg: "re-encrypt:cfg@next-rotation".to_string(),
        }
    }

    /// 预案合法：锚在位、禁实装声明在位、双路径预告非空。
    pub fn plan_ok(&self) -> Result<(), u16> {
        if self.anchor != ANCHOR_S40 {
            return Err(E_ANCHOR);
        }
        if self.implemented {
            return Err(E_IMPL);
        }
        if self.path_userlex.is_empty() || self.path_cfg.is_empty() {
            return Err(E_FIELD);
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&self.plan_id.to_le_bytes());
        buf.extend_from_slice(self.anchor.as_bytes());
        buf.push(self.implemented as u8);
        buf.extend_from_slice(self.path_userlex.as_bytes());
        buf.extend_from_slice(self.path_cfg.as_bytes());
        fnv1a64(&buf)
    }
}

/// 预案登记簿。
pub struct PlanLedger {
    pub plans: Vec<RotatePlan>,
    pub impl_attempts: u32, // 实装尝试计数（全部被拒）
}

impl PlanLedger {
    pub fn new() -> PlanLedger {
        PlanLedger { plans: vec![], impl_attempts: 0 }
    }

    /// 登记：预案合法 + 编号唯一。
    pub fn register(&mut self, p: RotatePlan) -> Result<(), u16> {
        p.plan_ok()?;
        let mut i = 0usize;
        while i < self.plans.len() {
            if self.plans[i].plan_id == p.plan_id {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.plans.push(p);
        Ok(())
    }

    /// 查询。
    pub fn lookup(&self, plan_id: u32) -> Result<&RotatePlan, u16> {
        let mut i = 0usize;
        while i < self.plans.len() {
            if self.plans[i].plan_id == plan_id {
                return Ok(&self.plans[i]);
            }
            i += 1;
        }
        Err(E_NOTFOUND)
    }

    /// 实装尝试：一律拒收（§40 红线），计数留痕。
    pub fn attempt_impl(&mut self, plan_id: u32) -> Result<(), u16> {
        self.impl_attempts += 1;
        match self.lookup(plan_id) {
            Ok(_) => Err(E_ATTEMPT),
            Err(e) => Err(e),
        }
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.plans.len() {
            buf.extend_from_slice(&self.plans[i].fp().to_le_bytes());
            i += 1;
        }
        buf.extend_from_slice(&self.impl_attempts.to_le_bytes());
        fnv1a64(&buf)
    }
}

/// 呈现行：plans=<n> impl=<0|1> att=<尝试数>。
pub fn ledger_line(l: &PlanLedger) -> String {
    let mut impl_ok: u32 = 0;
    let mut i = 0usize;
    while i < l.plans.len() {
        if !l.plans[i].implemented {
            impl_ok += 1;
        }
        i += 1;
    }
    let mut out = String::new();
    out.push_str("plans=");
    out.push_str(&l.plans.len().to_string());
    out.push_str(" impl=");
    out.push_str(&impl_ok.to_string());
    out.push_str(" att=");
    out.push_str(&l.impl_attempts.to_string());
    out
}

/// 子串判定（朴素扫描）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x486_97);

    // R1 预案在册。
    let mut led = PlanLedger::new();
    led.register(RotatePlan::declare())?;
    if led.plans.len() != 1 {
        return Err(E_PLAN);
    }
    hits += 1;

    // R2 禁实装声明在位：锚 + implemented=false + 双路径预告。
    let p = led.lookup(PLAN_ID)?;
    p.plan_ok()?;
    if p.anchor != "S40-NOIMPL" || p.implemented {
        return Err(E_IMPL);
    }
    hits += 1;

    // R3 实装尝试拒收：拒后 implemented 仍 false。
    match led.attempt_impl(PLAN_ID) {
        Err(E_ATTEMPT) => {}
        _ => return Err(E_ATTEMPT),
    }
    let p2 = led.lookup(PLAN_ID)?;
    if p2.implemented {
        return Err(E_IMPL);
    }
    hits += 1;

    // R4 路径预告字段齐。
    if p2.path_userlex.is_empty() || p2.path_cfg.is_empty() {
        return Err(E_FIELD);
    }
    if !has_kw(p2.path_userlex.as_bytes(), b"re-encrypt") {
        return Err(E_FIELD);
    }
    hits += 1;

    // R5 预案指纹稳定。
    let f1 = led.fp();
    let f2 = led.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R6 重复登记拒。
    match led.register(RotatePlan::declare()) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R7 查询：在册可查、查无返回红。
    led.lookup(PLAN_ID)?;
    match led.lookup(999) {
        Err(E_NOTFOUND) => hits += 1,
        _ => return Err(E_NOTFOUND),
    }

    // R8 呈现行：plans=1 impl=1 att=1。
    let line = ledger_line(&led);
    if !has_kw(line.as_bytes(), b"plans=1") || !has_kw(line.as_bytes(), b"impl=1")
        || !has_kw(line.as_bytes(), b"att=1")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R9 §40 锚定：锚串与预案常量全等。
    if ANCHOR_S40 != "S40-NOIMPL" {
        return Err(E_ANCHOR);
    }
    let mut tampered = RotatePlan::declare();
    tampered.anchor = "OTHER".to_string();
    match tampered.plan_ok() {
        Err(E_ANCHOR) => hits += 1,
        _ => return Err(E_ANCHOR),
    }

    // R10 指纹稳定（二次）+ 实装翻转检出。
    if led.fp() != f1 {
        return Err(E_FP);
    }
    let mut flip = RotatePlan::declare();
    flip.implemented = true;
    match flip.plan_ok() {
        Err(E_IMPL) => hits += 1,
        _ => return Err(E_IMPL),
    }
    let _ = g.below(10);

    Ok(hits)
}
