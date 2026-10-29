//! cgb825_fuzz — CGPU-F0825 Intel fuzz。
//!
//! 判据原文（验收行摘录）：Intel fuzz：确定性语料生成（种子驱动 LCG，同
//! 种子同序列——可复现）；变异策略（位翻转/字节替换/截断三型轮转）；挂
//! 钩目标（适配层入口参数包全字段喂入）；崩溃判定账本（违例码落账、种子
//! 回放定位——同种子重放必现）。判据：可复现、三型变异、全字段、回放
//! 定位。
//!
//! 诊断段 0x71A0（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；LCG 纯整数；账本守恒（执行数 = 通过+违例+
//! 跳过）；drill() 自例程返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb825-v1";

/// 诊断段基（本单独占 0x71A0）。
pub const SEG_BASE: u16 = 0x71A0;

/// 语料长度非法（0 长或超上限）。
pub const XC_CORPUS_LEN: u16 = SEG_BASE + 1;
/// 目标未挂钩（目标句柄不在册）。
pub const XC_TARGET_MISSING: u16 = SEG_BASE + 2;
/// 回放不现（同种子重放结果与首跑不一致——可复现性破坏）。
pub const XC_REPLAY_MISS: u16 = SEG_BASE + 3;
/// 账本漂移（执行数 ≠ 通过+违例+跳过）。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE + 4;
/// 变异策略未知（策略号超出三型闭集）。
pub const XC_BAD_MUTATION: u16 = SEG_BASE + 5;
/// 语料账满（定容违例账耗尽——防御性拒绝）。
pub const XC_FAULT_LEDGER_FULL: u16 = SEG_BASE + 6;

/// 语料长度上限。
pub const CORPUS_CAP: usize = 48;
/// 单轮变异执行数。
pub const RUNS_PER_SEED: u32 = 8;
/// 违例账定容。
pub const FAULT_LEDGER_CAP: usize = 16;

/// 变异三型（判据"三型轮转"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutation {
    /// 型 0：位翻转。
    BitFlip,
    /// 型 1：字节替换。
    ByteSwap,
    /// 型 2：截断。
    Truncate,
}

impl Mutation {
    /// 轮转取型（轮次 i 取 i % 3）。
    pub fn rotate(i: u32) -> Mutation {
        match i % 3 {
            0 => Mutation::BitFlip,
            1 => Mutation::ByteSwap,
            _ => Mutation::Truncate,
        }
    }
    /// 策略名。
    pub fn name(self) -> &'static str {
        match self {
            Mutation::BitFlip => "bitflip",
            Mutation::ByteSwap => "byteswap",
            Mutation::Truncate => "truncate",
        }
    }
}

/// 确定性 LCG（判据"同种子同序列"）。
pub struct Lcg {
    /// 状态。
    st: u64,
}

impl Lcg {
    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg { st: seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1 }
    }
    /// 取下一随机数。
    pub fn next(&mut self) -> u64 {
        self.st = self.st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.st ^ (self.st >> 33)
    }
    /// 取 [0, bound) 内整数。
    pub fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        (self.next() % bound as u64) as usize
    }
}

/// 语料生成（判据"确定性语料生成"——同种子同输出）。
pub fn gen_corpus(seed: u64, len: usize) -> Result<Vec<u8>, u16> {
    if len == 0 || len > CORPUS_CAP {
        return Err(XC_CORPUS_LEN);
    }
    let mut g = Lcg::new(seed);
    let mut v = Vec::new();
    for _ in 0..len {
        v.push((g.next() & 0xFF) as u8);
    }
    Ok(v)
}

/// 三型变异（判据"三型"——对语料原位改造）。
pub fn mutate(src: &[u8], m: Mutation, r: &mut Lcg) -> Result<Vec<u8>, u16> {
    if src.is_empty() || src.len() > CORPUS_CAP {
        return Err(XC_CORPUS_LEN);
    }
    let mut out = Vec::new();
    for b in src {
        out.push(*b);
    }
    match m {
        Mutation::BitFlip => {
            let at = r.below(out.len());
            let bit = r.below(8);
            out[at] ^= 1u8 << bit;
        }
        Mutation::ByteSwap => {
            let at = r.below(out.len());
            out[at] = (r.next() & 0xFF) as u8;
        }
        Mutation::Truncate => {
            let cut = 1 + r.below(out.len());
            out.truncate(cut);
        }
    }
    Ok(out)
}

/// 被挂钩目标（判据"适配层入口参数包全字段喂入"——模拟入口校验）。
/// 返回 Ok(()) 表示输入可接受；Err(违例码) 表示触发防护。
pub fn target_entry(input: &[u8]) -> Result<(), u16> {
    // 防护 1：空输入拒。
    if input.is_empty() {
        return Err(1);
    }
    // 防护 2：魔数首字节须为 0xA5。
    if input[0] != 0xA5 {
        return Err(2);
    }
    // 防护 3：字段和校验（余下字节和 mod 251 须等于 7）。
    let mut sum = 0u32;
    for b in &input[1..] {
        sum += *b as u32;
    }
    if sum % 251 != 7 {
        return Err(3);
    }
    Ok(())
}

/// 违例账条目（判据"回放定位"）。
#[derive(Clone, Debug)]
pub struct Fault {
    /// 种子。
    pub seed: u64,
    /// 轮内序号。
    pub run: u32,
    /// 变异型。
    pub m: Mutation,
    /// 违例码。
    pub code: u16,
    /// 触发输入摘要（前 8 字节）。
    pub digest: [u8; 8],
}

/// fuzz 执行器。
pub struct Fuzzer {
    /// 执行器名。
    pub name: String,
    /// 执行计数。
    pub execs: u64,
    /// 通过计数（目标 Ok）。
    pub passed: u64,
    /// 违例计数（目标 Err）。
    pub faults: u64,
    /// 跳过计数（生成/变异失败）。
    pub skipped: u64,
    /// 违例账（定容）。
    pub ledger: Vec<Fault>,
}

/// 守恒恒等式：execs == passed + faults + skipped。
/// 每轮执行后 verify() 复核；违例入账时定容满即整体拒（XC_FAULT_LEDGER_FULL）。
impl Fuzzer {
    /// 构造。
    pub fn new(name: &str) -> Self {
        Fuzzer { name: name.to_string(), execs: 0, passed: 0, faults: 0, skipped: 0, ledger: Vec::new() }
    }

    /// 单种子一轮：生成语料 + RUNS_PER_SEED 次三型轮转变异执行。
    pub fn run_seed(&mut self, seed: u64) -> Result<(u64, u64), u16> {
        let corpus = gen_corpus(seed, 16)?;
        let mut r = Lcg::new(seed);
        let mut p = 0u64;
        let mut f = 0u64;
        for i in 0..RUNS_PER_SEED {
            let m = Mutation::rotate(i);
            let input = mutate(&corpus, m, &mut r)?;
            if input.len() > CORPUS_CAP {
                self.skipped += 1;
                self.execs += 1;
                continue;
            }
            self.execs += 1;
            match target_entry(&input) {
                Ok(()) => {
                    self.passed += 1;
                    p += 1;
                }
                Err(code) => {
                    self.faults += 1;
                    f += 1;
                    if self.ledger.len() >= FAULT_LEDGER_CAP {
                        return Err(XC_FAULT_LEDGER_FULL);
                    }
                    let mut digest = [0u8; 8];
                    for (k, b) in input.iter().take(8).enumerate() {
                        digest[k] = *b;
                    }
                    self.ledger.push(Fault { seed, run: i, m, code, digest });
                }
            }
        }
        self.verify()?;
        Ok((p, f))
    }

    /// 回放定位（判据"同种子重放必现"）：按账目重放指定种子并比对违例码。
    pub fn replay_check(&self, seed: u64) -> Result<(), u16> {
        let mut expect: Vec<(u32, u16)> = Vec::new();
        for flt in self.ledger.iter() {
            if flt.seed == seed {
                expect.push((flt.run, flt.code));
            }
        }
        if expect.is_empty() {
            return Err(XC_TARGET_MISSING);
        }
        // 重放：同种子同序列。
        let corpus = gen_corpus(seed, 16)?;
        let mut r = Lcg::new(seed);
        for i in 0..RUNS_PER_SEED {
            let m = Mutation::rotate(i);
            let input = mutate(&corpus, m, &mut r)?;
            let got = match target_entry(&input) {
                Ok(()) => 0,
                Err(c) => c,
            };
            for (run, code) in expect.iter() {
                if *run == i && *code != got {
                    return Err(XC_REPLAY_MISS);
                }
            }
        }
        Ok(())
    }

    /// 守恒校验。
    pub fn verify(&self) -> Result<(), u16> {
        if self.execs != self.passed + self.faults + self.skipped {
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb825[");
        s.push_str(&self.name);
        s.push_str("] execs=");
        s.push_str(&self.execs.to_string());
        s.push_str(" pass=");
        s.push_str(&self.passed.to_string());
        s.push_str(" fault=");
        s.push_str(&self.faults.to_string());
        s.push_str(" skip=");
        s.push_str(&self.skipped.to_string());
        s.push_str(" ledger=");
        s.push_str(&self.ledger.len().to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut fz = Fuzzer::new("w3-drill-825");

    // 负样本 1：非法语料长度——应拒。
    match gen_corpus(1, 0) {
        Err(code) if code == XC_CORPUS_LEN => {}
        Ok(_) => return Err(XC_CORPUS_LEN),
        Err(other) => return Err(other),
    }
    match gen_corpus(1, CORPUS_CAP + 1) {
        Err(code) if code == XC_CORPUS_LEN => {}
        Ok(_) => return Err(XC_CORPUS_LEN),
        Err(other) => return Err(other),
    }

    // 可复现性单元：同种子两次生成必须逐字节一致。
    let c1 = gen_corpus(42, 16)?;
    let c2 = gen_corpus(42, 16)?;
    if c1.len() != c2.len() {
        return Err(XC_REPLAY_MISS);
    }
    for i in 0..c1.len() {
        if c1[i] != c2[i] {
            return Err(XC_REPLAY_MISS);
        }
    }
    // 不同种子应不同（概率性断言：首字节必异——LCG 性质保障）。
    let c3 = gen_corpus(43, 16)?;
    if c3[0] == c1[0] && c3[1] == c1[1] && c3[2] == c1[2] {
        return Err(XC_REPLAY_MISS);
    }

    // 三型单元：每种型各跑一次，输出长度受控。
    let mut r = Lcg::new(7);
    let short = [0u8; 4];
    let mb = mutate(&short, Mutation::BitFlip, &mut r)?;
    if mb.len() != 4 {
        return Err(XC_BAD_MUTATION);
    }
    let ms = mutate(&short, Mutation::ByteSwap, &mut r)?;
    if ms.len() != 4 {
        return Err(XC_BAD_MUTATION);
    }
    let mt = mutate(&short, Mutation::Truncate, &mut r)?;
    if mt.is_empty() || mt.len() > 4 {
        return Err(XC_BAD_MUTATION);
    }

    // 批量跑多种子（正样本走核心路径）。
    for seed in 100..112u64 {
        fz.run_seed(seed)?;
    }
    fz.verify()?;
    if fz.execs < 8 * 8 {
        return Err(XC_LEDGER_DRIFT);
    }
    // 回放定位：对账内每个种子回放核验。
    let mut seen: Vec<u64> = Vec::new();
    for flt in fz.ledger.iter() {
        let mut dup = false;
        for s in seen.iter() {
            if *s == flt.seed {
                dup = true;
                break;
            }
        }
        if !dup {
            seen.push(flt.seed);
        }
    }
    for s in seen.iter() {
        fz.replay_check(*s)?;
    }
    // 负样本 2：回放账外种子——应拒。
    match fz.replay_check(u64::MAX - 1) {
        Err(code) if code == XC_TARGET_MISSING => {}
        Ok(_) => return Err(XC_TARGET_MISSING),
        Err(other) => return Err(other),
    }
    Ok(0)
}
