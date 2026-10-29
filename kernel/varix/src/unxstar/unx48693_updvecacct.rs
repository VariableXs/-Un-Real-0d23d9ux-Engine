//! UNX-F48693 —— I35 向量账（更新用例集+校验和）
//! 判据原文：UNX-F48693-J1 用例 ×20 可复跑；判据锚定 §AI-61 §8。
//! 判据落地（drill 场景）：R1 登记 ×20；R2 校验和稳定；R3 复跑写账；
//! R4 同种子重放；R5 篡改检出；R6 类别闭集；R7 空名拒收；
//! R8 重复 ID 拒收；R9 呈现行；R10 冻结序守恒。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：更新向量账四类用例（签名/换装/回滚/拒装）逐例
//! 确定性执行，expect 首轮写账、次轮必等；冻结序与登记序一致。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0E00;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_KIND: u16 = SEG_BASE + 1; // 用例类别越界未拒收
const E_CSUM: u16 = SEG_BASE + 2; // 校验和漂移
const E_RERUN: u16 = SEG_BASE + 3; // 复跑结果不一致
const E_REPLAY: u16 = SEG_BASE + 4; // 同种子重放漂移
const E_TAMPER: u16 = SEG_BASE + 5; // 账面篡改未检出
const E_NAME: u16 = SEG_BASE + 6; // 空名未拒收
const E_DUP: u16 = SEG_BASE + 7; // 重复 ID 未拒收
const E_COUNT: u16 = SEG_BASE + 8; // 用例数不足 20
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段
const E_ORDER: u16 = SEG_BASE + 10; // 冻结序漂移

/// 更新用例类别闭集。
pub const K_SIG: u8 = 1; // 签名校验
pub const K_SWAP: u8 = 2; // 原子换装
pub const K_ROLL: u8 = 3; // 回滚
pub const K_REJ: u8 = 4; // 拒装
/// 冻结用例数。
pub const CASE_N: usize = 20;

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

/// 一条更新用例向量。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UpdCase {
    pub id: u32,
    pub name: String,
    pub kind: u8,
    pub seed: u32,
    pub steps: u32,
    pub expect: u64,
}

impl UpdCase {
    pub fn new(id: u32, name: &str, kind: u8, seed: u32, steps: u32) -> UpdCase {
        UpdCase { id, name: name.to_string(), kind, seed, steps, expect: 0 }
    }

    /// 类别闭集判定。
    pub fn kind_ok(k: u8) -> bool {
        k == K_SIG || k == K_SWAP || k == K_ROLL || k == K_REJ
    }

    /// 静态指纹。
    pub fn checksum(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&self.id.to_le_bytes());
        buf.extend_from_slice(self.name.as_bytes());
        buf.push(self.kind);
        buf.extend_from_slice(&self.seed.to_le_bytes());
        buf.extend_from_slice(&self.steps.to_le_bytes());
        fnv1a64(&buf)
    }

    /// 确定性执行。
    pub fn exec(&self) -> u64 {
        let mut g = Lcg::new(self.seed);
        let mut acc: u64 = 0;
        let mut t: u32 = 0;
        while t < self.steps {
            acc = acc.wrapping_add(g.next_u32() as u64);
            t += 1;
        }
        acc ^ self.checksum()
    }
}

/// 更新向量账。
#[derive(Clone)]
pub struct UpdVectorLedger {
    pub cases: Vec<UpdCase>,
    pub frozen: Vec<u64>,
}

impl UpdVectorLedger {
    pub fn new() -> UpdVectorLedger {
        UpdVectorLedger { cases: vec![], frozen: vec![] }
    }

    /// 登记。
    pub fn register(&mut self, c: UpdCase) -> Result<(), u16> {
        if !UpdCase::kind_ok(c.kind) {
            return Err(E_KIND);
        }
        if c.name.is_empty() {
            return Err(E_NAME);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].id == c.id {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.cases.push(c);
        Ok(())
    }

    /// 冻结。
    pub fn freeze(&mut self) {
        self.frozen.clear();
        let mut i = 0usize;
        while i < self.cases.len() {
            self.frozen.push(self.cases[i].checksum());
            i += 1;
        }
    }

    /// 核验。
    pub fn verify(&self) -> Result<(), u16> {
        if self.frozen.len() != self.cases.len() {
            return Err(E_ORDER);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.frozen[i] != self.cases[i].checksum() {
                return Err(E_CSUM);
            }
            if self.cases[i].expect != self.cases[i].exec() {
                return Err(E_RERUN);
            }
            i += 1;
        }
        Ok(())
    }

    /// 全量复跑。
    pub fn rerun_all(&mut self) -> Result<u32, u16> {
        let mut ok: u32 = 0;
        let mut i = 0usize;
        while i < self.cases.len() {
            let got = self.cases[i].exec();
            let want = self.cases[i].expect;
            if want != 0 && want != got {
                return Err(E_REPLAY);
            }
            self.cases[i].expect = got;
            ok += 1;
            i += 1;
        }
        Ok(ok)
    }

    /// 按类计数。
    pub fn count_kind(&self, k: u8) -> u32 {
        let mut c: u32 = 0;
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].kind == k {
                c += 1;
            }
            i += 1;
        }
        c
    }
}

/// 呈现行：cases=<n> csum=<首例低 32 位> frozen=<冻结数>。
pub fn ledger_line(l: &UpdVectorLedger) -> String {
    let mut out = String::new();
    out.push_str("cases=");
    out.push_str(&l.cases.len().to_string());
    out.push_str(" csum=");
    if l.cases.is_empty() {
        out.push_str("0");
    } else {
        let c = l.cases[0].checksum() as u32;
        out.push_str(&c.to_string());
    }
    out.push_str(" frozen=");
    out.push_str(&l.frozen.len().to_string());
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

    // R1 登记 ×20：四类轮转。
    let mut led = UpdVectorLedger::new();
    let mut i: u32 = 0;
    while i < CASE_N as u32 {
        let kind = (1 + (i % 4)) as u8;
        let name = match kind {
            K_SIG => "sig-case",
            K_SWAP => "swap-case",
            K_ROLL => "roll-case",
            _ => "rej-case",
        };
        let c = UpdCase::new(486_930 + i, name, kind, 0x0E00_0000 + i, 10 + i);
        led.register(c)?;
        i += 1;
    }
    if led.cases.len() != CASE_N {
        return Err(E_COUNT);
    }
    if led.count_kind(K_SIG) != 5 || led.count_kind(K_REJ) != 5 {
        return Err(E_KIND);
    }
    hits += 1;

    // R2 校验和稳定。
    let c0a = led.cases[0].checksum();
    let c0b = led.cases[0].checksum();
    if c0a != c0b || c0a == 0 {
        return Err(E_CSUM);
    }
    hits += 1;

    // R3 复跑写账 + 冻结核验。
    led.rerun_all()?;
    led.freeze();
    led.verify()?;
    hits += 1;

    // R4 同种子重放。
    led.rerun_all()?;
    led.verify()?;
    hits += 1;

    // R5 篡改检出。
    let mut bad = led.clone();
    bad.cases[9].expect = bad.cases[9].expect.wrapping_add(7);
    match bad.verify() {
        Err(E_RERUN) => hits += 1,
        _ => return Err(E_TAMPER),
    }

    // R6 类别闭集：kind=0 与 kind=5 拒收。
    let mut probe = UpdVectorLedger::new();
    match probe.register(UpdCase::new(1, "k0", 0, 1, 2)) {
        Err(E_KIND) => {}
        _ => return Err(E_KIND),
    }
    match probe.register(UpdCase::new(1, "k5", 5, 1, 2)) {
        Err(E_KIND) => hits += 1,
        _ => return Err(E_KIND),
    }

    // R7 空名拒收。
    match probe.register(UpdCase::new(2, "", K_SWAP, 3, 4)) {
        Err(E_NAME) => hits += 1,
        _ => return Err(E_NAME),
    }

    // R8 重复 ID 拒收。
    probe.register(UpdCase::new(9, "a", K_SIG, 1, 2))?;
    match probe.register(UpdCase::new(9, "b", K_ROLL, 2, 3)) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R9 呈现行：cases=20 frozen=20。
    let line = ledger_line(&led);
    if !has_kw(line.as_bytes(), b"cases=20") || !has_kw(line.as_bytes(), b"frozen=20") {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 冻结序守恒。
    let f1 = led.frozen.clone();
    led.freeze();
    if f1 != led.frozen {
        return Err(E_ORDER);
    }
    hits += 1;

    Ok(hits)
}
