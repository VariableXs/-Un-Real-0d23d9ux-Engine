//! UNX-F48653 —— I33 向量账（出厂用例集+校验和）
//! 判据原文：UNX-F48653-J1 用例 ×20 可复跑；判据锚定 §AI-61 §8。
//! 判据落地（drill 场景）：R1 登记 ×20；R2 校验和稳定；R3 复跑写账；
//! R4 同种子重放；R5 篡改检出；R6 类别闭集；R7 空名拒收；
//! R8 重复 ID 拒收；R9 呈现行；R10 冻结序守恒。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG（wrapping_mul/wrapping_add）；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（本卡专用 0x20 窗）。
pub const SEG_BASE: u16 = 0x08C0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_REG: u16 = SEG_BASE + 1; // 合法用例登记被拒
const E_CSUM: u16 = SEG_BASE + 2; // 校验和漂移
const E_RERUN: u16 = SEG_BASE + 3; // 复跑结果与期望不一致
const E_REPLAY: u16 = SEG_BASE + 4; // 同种子重放漂移
const E_TAMPER: u16 = SEG_BASE + 5; // 账面篡改未检出
const E_KIND: u16 = SEG_BASE + 6; // 类别越界未拒收
const E_NAME: u16 = SEG_BASE + 7; // 空名未拒收
const E_DUP: u16 = SEG_BASE + 8; // 重复 ID 未拒收
const E_COUNT: u16 = SEG_BASE + 9; // 用例数不足 20
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段
const E_ORDER: u16 = SEG_BASE + 11; // 冻结序漂移

/// 出厂用例类别闭集。
pub const KIND_BOOT: u8 = 1; // 首启可用
pub const KIND_LEX: u8 = 2; // 词面齐备
pub const KIND_CFG: u8 = 3; // 配置基线
pub const KIND_LANG: u8 = 4; // 语言边界
/// 向量账冻结用例数。
pub const CASE_N: usize = 20;

/// FNV-1a 64 位（与本域哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
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

/// 一条出厂用例向量。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VectorCase {
    pub id: u32,
    pub name: String,
    pub kind: u8,
    pub seed: u32,
    pub steps: u32,
    pub expect: u64, // 期望结果（复跑校验和）
}

impl VectorCase {
    /// 构造：expect 置零，待首轮复跑写账。
    pub fn new(id: u32, name: &str, kind: u8, seed: u32, steps: u32) -> VectorCase {
        VectorCase { id, name: name.to_string(), kind, seed, steps, expect: 0 }
    }

    /// 类别闭集判定。
    pub fn kind_ok(k: u8) -> bool {
        k == KIND_BOOT || k == KIND_LEX || k == KIND_CFG || k == KIND_LANG
    }

    /// 静态指纹：字段串联 FNV（不含 expect，改值不漂移静态账）。
    pub fn checksum(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&self.id.to_le_bytes());
        buf.extend_from_slice(self.name.as_bytes());
        buf.push(self.kind);
        buf.extend_from_slice(&self.seed.to_le_bytes());
        buf.extend_from_slice(&self.steps.to_le_bytes());
        fnv1a64(&buf)
    }

    /// 确定性执行：LCG 驱动 steps 步求和并与静态指纹混签。
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

/// 向量账：登记-冻结-复跑-核验。
#[derive(Clone)]
pub struct VectorLedger {
    pub cases: Vec<VectorCase>,
    pub frozen: Vec<u64>,
}

impl VectorLedger {
    pub fn new() -> VectorLedger {
        VectorLedger { cases: vec![], frozen: vec![] }
    }

    /// 登记一条用例：类别闭集 + 非空名 + ID 唯一。
    pub fn register(&mut self, c: VectorCase) -> Result<(), u16> {
        if !VectorCase::kind_ok(c.kind) {
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

    /// 冻结：记录全部静态校验和，序与登记序一致。
    pub fn freeze(&mut self) {
        self.frozen.clear();
        let mut i = 0usize;
        while i < self.cases.len() {
            self.frozen.push(self.cases[i].checksum());
            i += 1;
        }
    }

    /// 账面核验：冻结序全对 + 期望值全对。
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

    /// 全量复跑：逐例重放并写回 expect（首轮写账，次轮必等）。
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
}

/// 呈现行：cases=<n> csum=<首例校验和低 32 位> frozen=<冻结数>。
pub fn ledger_line(l: &VectorLedger) -> String {
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

/// 子串判定（朴素扫描，统一口径）。
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

    // R1 登记 ×20：四类轮转，ID 连续不重。
    let mut led = VectorLedger::new();
    let mut i: u32 = 0;
    while i < CASE_N as u32 {
        let kind = (1 + (i % 4)) as u8;
        let name = match kind {
            KIND_BOOT => "bootcase",
            KIND_LEX => "lexcase",
            KIND_CFG => "cfgcase",
            _ => "langcase",
        };
        let c = VectorCase::new(486_530 + i, name, kind, 0x0800_0000 + i, 16 + i);
        led.register(c)?;
        i += 1;
    }
    if led.cases.len() != CASE_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 校验和稳定：同例两次相等且非零。
    let c0a = led.cases[0].checksum();
    let c0b = led.cases[0].checksum();
    if c0a != c0b || c0a == 0 {
        return Err(E_CSUM);
    }
    hits += 1;

    // R3 复跑写账：首轮写期望，冻结后核验全对。
    led.rerun_all()?;
    led.freeze();
    led.verify()?;
    hits += 1;

    // R4 同种子重放：二次复跑逐例全等。
    led.rerun_all()?;
    led.verify()?;
    hits += 1;

    // R5 篡改检出：改一处期望 → 复跑核验即红。
    let mut bad = led.clone();
    bad.cases[3].expect = bad.cases[3].expect.wrapping_add(1);
    match bad.verify() {
        Err(E_RERUN) => hits += 1,
        _ => return Err(E_TAMPER),
    }

    // R6 类别闭集：kind=5 拒收。
    let mut probe = VectorLedger::new();
    let c_bad = VectorCase::new(1, "badkind", 5, 1, 4);
    match probe.register(c_bad) {
        Err(E_KIND) => hits += 1,
        _ => return Err(E_KIND),
    }

    // R7 空名拒收。
    let c_empty = VectorCase::new(2, "", KIND_CFG, 7, 3);
    match probe.register(c_empty) {
        Err(E_NAME) => hits += 1,
        _ => return Err(E_NAME),
    }

    // R8 重复 ID 拒收。
    probe.register(VectorCase::new(9, "first", KIND_BOOT, 1, 2))?;
    let c_dup = VectorCase::new(9, "second", KIND_LEX, 2, 3);
    match probe.register(c_dup) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R9 呈现行：三段关键字在位。
    let line = ledger_line(&led);
    if !has_kw(line.as_bytes(), b"cases=") || !has_kw(line.as_bytes(), b"csum=")
        || !has_kw(line.as_bytes(), b"frozen=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 冻结序守恒：freeze 幂等，序不漂移。
    let f1 = led.frozen.clone();
    led.freeze();
    if f1 != led.frozen {
        return Err(E_ORDER);
    }
    hits += 1;

    Ok(hits)
}
