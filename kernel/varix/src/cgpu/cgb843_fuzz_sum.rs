//! cgb843_fuzz_sum — CGPU-F0843 Intel fuzz 汇总。
//!
//! 判据原文（验收行摘录）：Intel fuzz 汇总：种子账汇总（种子数/输入数/
//! 违例数三计账）；违例分型账（按防护闸三类归账：魔数/长度/校验和）；
//! 去重种子账（同种子多次轮跑只记一轮——重复轮拒绝）；汇总守恒（输入数
//! = 通过 + 违例 + 跳过）。判据：三计账、分型账、去重、守恒。
//!
//! 诊断段 0x73E0（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；定容汇总账；守恒每步可验；drill() 自例程
//! 返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb843-v1";

/// 诊断段基（本单独占 0x73E0）。
pub const SEG_BASE: u16 = 0x73E0;

/// 种子重复（去重命中——同种子二次轮跑拒收）。
pub const XC_SEED_DUP: u16 = SEG_BASE + 1;
/// 防护闸未知（违例码不在三闸闭集）。
pub const XC_TRAP_UNKNOWN: u16 = SEG_BASE + 2;
/// 汇总账漂移（输入数 ≠ 通过+违例+跳过）。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE + 3;
/// 种子账满（定容耗尽）。
pub const XC_SEED_FULL: u16 = SEG_BASE + 4;
/// 零输入轮（一轮 0 输入——非法轮账）。
pub const XC_EMPTY_ROUND: u16 = SEG_BASE + 5;
/// 未收口（轮账在挂即读总账）。
pub const XC_ROUND_OPEN: u16 = SEG_BASE + 6;

/// 三防护闸闭集（判据"分型账"三类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trap {
    /// 魔数闸。
    Magic,
    /// 长度闸。
    Length,
    /// 校验和闸。
    Sum,
}

impl Trap {
    /// 闸代号（违例码映射）。
    pub fn code(self) -> u16 {
        match self {
            Trap::Magic => 1,
            Trap::Length => 2,
            Trap::Sum => 3,
        }
    }
    /// 由违例码反查闸。
    pub fn from_code(c: u16) -> Result<Trap, u16> {
        match c {
            1 => Ok(Trap::Magic),
            2 => Ok(Trap::Length),
            3 => Ok(Trap::Sum),
            _ => Err(XC_TRAP_UNKNOWN),
        }
    }
    /// 闸名。
    pub fn name(self) -> &'static str {
        match self {
            Trap::Magic => "magic",
            Trap::Length => "length",
            Trap::Sum => "checksum",
        }
    }
}

/// 单轮账。
#[derive(Clone, Debug)]
pub struct Round {
    /// 种子。
    pub seed: u64,
    /// 本轮输入数。
    pub inputs: u64,
    /// 本轮通过数。
    pub passed: u64,
    /// 本轮违例数。
    pub faults: u64,
    /// 本轮跳过数。
    pub skipped: u64,
}

/// fuzz 汇总账。
pub struct FuzzSum {
    /// 汇总名。
    pub name: String,
    /// 轮账（定容，种子去重）。
    pub rounds: Vec<Round>,
    /// 分型违例账（三闸计数——守恒项）。
    pub trap_cnt: [u64; 3],
    /// 输入总数。
    pub inputs_total: u64,
    /// 通过总数。
    pub passed_total: u64,
    /// 违例总数。
    pub faults_total: u64,
    /// 跳过总数。
    pub skipped_total: u64,
}

/// 守恒恒等式：inputs_total == passed_total + faults_total + skipped_total
/// 且 faults_total == Σ trap_cnt（分型账之和）；每轮内同式亦须成立。
impl FuzzSum {
    /// 构造。
    pub fn new(name: &str) -> Self {
        FuzzSum {
            name: name.to_string(),
            rounds: Vec::new(),
            trap_cnt: [0; 3],
            inputs_total: 0,
            passed_total: 0,
            faults_total: 0,
            skipped_total: 0,
        }
    }

    /// 收一轮账（判据"去重"：同种子二次轮跑拒收）。
    pub fn add_round(
        &mut self,
        seed: u64,
        passed: u64,
        faults: &[(u16, u64)],
        skipped: u64,
    ) -> Result<(), u16> {
        if self.rounds.len() >= 16 {
            return Err(XC_SEED_FULL);
        }
        for r in self.rounds.iter() {
            if r.seed == seed {
                return Err(XC_SEED_DUP);
            }
        }
        // 零输入轮非法（判据"零输入轮"）。
        let mut fin = 0u64;
        for (_, n) in faults.iter() {
            fin += *n;
        }
        let inputs = passed + fin + skipped;
        if inputs == 0 {
            return Err(XC_EMPTY_ROUND);
        }
        // 分型归账。
        for (code, n) in faults.iter() {
            match Trap::from_code(*code) {
                Ok(t) => {
                    let idx = match t {
                        Trap::Magic => 0,
                        Trap::Length => 1,
                        Trap::Sum => 2,
                    };
                    self.trap_cnt[idx] += *n;
                }
                Err(e) => return Err(e),
            }
        }
        self.rounds.push(Round { seed, inputs, passed, faults: fin, skipped });
        self.inputs_total += inputs;
        self.passed_total += passed;
        self.faults_total += fin;
        self.skipped_total += skipped;
        self.verify()?;
        Ok(())
    }

    /// 轮数（判据"种子数"——去重后）。
    pub fn seed_count(&self) -> usize {
        self.rounds.len()
    }

    /// 某闸违例计数。
    pub fn trap_count(&self, t: Trap) -> Result<u64, u16> {
        Ok(match t {
            Trap::Magic => self.trap_cnt[0],
            Trap::Length => self.trap_cnt[1],
            Trap::Sum => self.trap_cnt[2],
        })
    }

    /// 违例率（千分比整数：违例 * 1000 / 输入）。
    pub fn fault_rate(&self) -> Result<u64, u16> {
        if self.inputs_total == 0 {
            return Err(XC_EMPTY_ROUND);
        }
        Ok(self.faults_total * 1000 / self.inputs_total)
    }

    /// 查种子轮。
    pub fn find_seed(&self, seed: u64) -> Result<&Round, u16> {
        for r in self.rounds.iter() {
            if r.seed == seed {
                return Ok(r);
            }
        }
        Err(XC_SEED_DUP)
    }

    /// 收口核验（判据"未收口"——账必须非空且自洽）。
    pub fn close_check(&self) -> Result<(), u16> {
        if self.rounds.is_empty() {
            return Err(XC_ROUND_OPEN);
        }
        self.verify()?;
        Ok(())
    }

    /// 守恒校验。
    pub fn verify(&self) -> Result<(), u16> {
        if self.inputs_total != self.passed_total + self.faults_total + self.skipped_total {
            return Err(XC_LEDGER_DRIFT);
        }
        let tsum = self.trap_cnt[0] + self.trap_cnt[1] + self.trap_cnt[2];
        if tsum != self.faults_total {
            return Err(XC_LEDGER_DRIFT);
        }
        // 轮内守恒。
        let mut ti = 0u64;
        let mut tp = 0u64;
        let mut tf = 0u64;
        let mut ts = 0u64;
        for r in self.rounds.iter() {
            if r.inputs != r.passed + r.faults + r.skipped {
                return Err(XC_LEDGER_DRIFT);
            }
            ti += r.inputs;
            tp += r.passed;
            tf += r.faults;
            ts += r.skipped;
        }
        if ti != self.inputs_total || tp != self.passed_total || tf != self.faults_total || ts != self.skipped_total {
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb843[");
        s.push_str(&self.name);
        s.push_str("] seeds=");
        s.push_str(&self.seed_count().to_string());
        s.push_str(" inputs=");
        s.push_str(&self.inputs_total.to_string());
        s.push_str(" pass=");
        s.push_str(&self.passed_total.to_string());
        s.push_str(" fault=");
        s.push_str(&self.faults_total.to_string());
        s.push_str(" skip=");
        s.push_str(&self.skipped_total.to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut f = FuzzSum::new("w3-drill-843");

    // 负样本 1：零输入轮——应拒。
    match f.add_round(1, 0, &[], 0) {
        Err(code) if code == XC_EMPTY_ROUND => {}
        Ok(_) => return Err(XC_EMPTY_ROUND),
        Err(other) => return Err(other),
    }
    // 负样本 2：未知违例码——应拒。
    match f.add_round(2, 1, &[(9, 1)], 0) {
        Err(code) if code == XC_TRAP_UNKNOWN => {}
        Ok(_) => return Err(XC_TRAP_UNKNOWN),
        Err(other) => return Err(other),
    }

    // 正样本：三轮账。
    // 种子 100：8 输入 = 3 过 + 4 违例（魔数 2、长度 1、校验和 1）+ 1 跳。
    f.add_round(100, 3, &[(Trap::Magic.code(), 2), (Trap::Length.code(), 1), (Trap::Sum.code(), 1)], 1)?;
    // 种子 200：8 输入 = 5 过 + 2 违例（校验和 2）+ 1 跳。
    f.add_round(200, 5, &[(Trap::Sum.code(), 2)], 1)?;
    // 种子 300：8 输入 = 7 过 + 1 违例（魔数 1）。
    f.add_round(300, 7, &[(Trap::Magic.code(), 1)], 0)?;

    // 负样本 3：种子去重——同种子二次轮跑。
    match f.add_round(100, 1, &[], 7) {
        Err(code) if code == XC_SEED_DUP => {}
        Ok(_) => return Err(XC_SEED_DUP),
        Err(other) => return Err(other),
    }
    if f.seed_count() != 3 {
        return Err(XC_LEDGER_DRIFT);
    }

    // 计数核对：24 输入 = 15 过 + 7 违例 + 2 跳。
    if f.inputs_total != 24 || f.passed_total != 15 || f.faults_total != 7 || f.skipped_total != 2 {
        return Err(XC_LEDGER_DRIFT);
    }
    // 分型账：魔数 3、长度 1、校验和 3。
    if f.trap_count(Trap::Magic)? != 3 || f.trap_count(Trap::Length)? != 1 || f.trap_count(Trap::Sum)? != 3 {
        return Err(XC_LEDGER_DRIFT);
    }
    // 违例率：7 * 1000 / 24 = 291。
    if f.fault_rate()? != 291 {
        return Err(XC_LEDGER_DRIFT);
    }
    // 查种子轮。
    let r100 = f.find_seed(100)?;
    if r100.inputs != 8 || r100.faults != 4 {
        return Err(XC_LEDGER_DRIFT);
    }
    // 负样本 4：查账外种子。
    match f.find_seed(999) {
        Err(code) if code == XC_SEED_DUP => {}
        Ok(_) => return Err(XC_SEED_DUP),
        Err(other) => return Err(other),
    }
    // 收口。
    f.close_check()?;
    f.verify()?;

    // 空账收口拒。
    let e = FuzzSum::new("w3-drill-843-empty");
    match e.close_check() {
        Err(code) if code == XC_ROUND_OPEN => {}
        Ok(_) => return Err(XC_ROUND_OPEN),
        Err(other) => return Err(other),
    }
    // 闸代号映射自检。
    for (t, c) in [(Trap::Magic, 1u16), (Trap::Length, 2), (Trap::Sum, 3)] {
        if t.code() != c || Trap::from_code(c)? != t {
            return Err(XC_TRAP_UNKNOWN);
        }
    }
    Ok(0)
}
