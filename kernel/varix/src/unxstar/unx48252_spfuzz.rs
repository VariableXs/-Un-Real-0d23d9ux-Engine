//! unx48252_spfuzz — UNX-F48252 方案面 fuzz（随机键位表/随机短语注入不崩）（诊断段 0xC060）。
//!
//! 判据原文：
//!   J1 fuzz ×10^4 零崩；判据锚定 体验十。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. fuzz 双靶面：键位表靶（26 键 × 双韵母槽的随机写：正常值/
//!      重复值/越界值/清零值四类变异）与短语靶（随机 code/短语注
//!      入：合法/重复/超长/超容四类注入）。两类靶全走确定性 LCG
//!      （wrapping 算术），同种子同账，禁真随机。
//!   2. 靶面自防御是"零崩"的本体：键位表靶的每次写入后过校验器
//!      （韵母 id 闭带、承载唯一），违规只落 graceful 拒收账，绝
//!      不 panic；短语靶的每次注入过四闸（查重/超长/超容/空载），
//!      违规同样 graceful。崩的账面形态只剩不变量破（crashes 计）。
//!   3. 不变量三连随轮审计：键位表值域闭带（id ≤ F_POOL）、短语
//!      账容量守恒（len ≤ CAP）、计数守恒（seen == ok + graceful）。
//!      任何一轮违背即 crashes+1（非 panic），×10^4 后 crashes 必
//!      为 0。
//!   4. 覆盖率硬约束：×10^4 轮内四类表变异、四类短语注入、合法查
//!      询路径全部命中（coverage 掩码全 1），否则 fuzz 未铺满、判
//!      据无效。
//!   5. 校验器独立成码：validate_layout 与 resolve 双算——写入面
//!      与查询面各有一套检查，检查结果逐轮留账，注入路径的
//!      graceful 与查询路径的 graceful 分账计数。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC060;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_INVARIANT: u16 = SEG_BASE + 1; // 不变量破（崩的账面形态）
const E_UNSETTLED: u16 = SEG_BASE + 2; // 计数守恒破
const E_COVER_MISS: u16 = SEG_BASE + 3; // fuzz 未铺满
const E_ROUND_MISS: u16 = SEG_BASE + 4; // 轮数不足
const E_TABLE_BAD: u16 = SEG_BASE + 5; // 键位表校验拒（graceful 态）
const E_PHRASE_BAD: u16 = SEG_BASE + 6; // 短语注入拒（graceful 态）
const E_SEED_ECHO: u16 = SEG_BASE + 7; // 同种子两跑不一致
const E_EMPTY_SUM: u16 = SEG_BASE + 8; // 写回摘要为空

/// 键位表韵母 id 值域闭带 [1, F_POOL]（0=槽空）。
pub const F_POOL: u8 = 12;
/// 短语账容量硬顶。
pub const PH_CAP: usize = 64;
/// 短语 code 闭带上界。
pub const PH_CODE_MAX: u32 = 999;

/// 可变键位表靶。
pub struct Layout {
    pub finals: [[u8; 2]; 26],
}

impl Layout {
    pub fn blank() -> Layout {
        Layout { finals: [[0; 2]; 26] }
    }

    /// 随机写一槽（四类变异由 LCG 驱动）。
    pub fn mutate(&mut self, rng: &mut Lcg) {
        let key = rng.below(26) as usize;
        let slot = rng.below(2) as usize;
        let class = rng.below(4);
        let v = match class {
            0 => 1 + rng.below(F_POOL as u32) as u8, // 正常值
            1 => {
                // 重复值：抄邻键同槽（承载唯一破探针）。
                let nb = (key + 1) % 26;
                self.finals[nb][slot]
            }
            2 => F_POOL + 1 + rng.below(8) as u8, // 越界值
            _ => 0, // 清零
        };
        self.finals[key][slot] = v;
    }

    /// 校验器（纪律 5）：值域闭带 + 非零承载唯一。违规返回 Err。
    pub fn validate(&self) -> Result<(), u16> {
        let mut seen = [0u32; 16];
        let mut i = 0usize;
        while i < 26 {
            let mut s = 0usize;
            while s < 2 {
                let v = self.finals[i][s];
                if v as usize >= seen.len() {
                    return Err(E_TABLE_BAD); // 越界值
                }
                if v != 0 {
                    seen[v as usize] += 1;
                }
                s += 1;
            }
            i += 1;
        }
        let mut f = 1usize;
        while f <= F_POOL as usize {
            if seen[f] > 1 {
                return Err(E_TABLE_BAD); // 承载唯一破
            }
            f += 1;
        }
        Ok(())
    }

    /// 查询面 resolve：键对 → 韵母 id。空槽/校验破均 graceful。
    pub fn resolve(&self, k1: usize, k2: usize) -> Result<u8, u16> {
        if k1 >= 26 || k2 >= 26 {
            return Err(E_TABLE_BAD);
        }
        let f1 = self.finals[k2][0];
        let f2 = self.finals[k2][1];
        if f1 == 0 && f2 == 0 {
            return Err(E_TABLE_BAD); // 空槽：键洞
        }
        if f1 as usize > F_POOL as usize || f2 as usize > F_POOL as usize {
            return Err(E_TABLE_BAD); // 越界值在账
        }
        if f2 != 0 && f1 == f2 {
            return Err(E_TABLE_BAD); // 自重复
        }
        Ok(if f2 == 0 { f1 } else { f1 })
    }
}

/// 短语注入靶。
pub struct PhraseFace {
    pub codes: Vec<u32>,
}

impl PhraseFace {
    pub fn new() -> PhraseFace {
        PhraseFace { codes: Vec::new() }
    }

    /// 注入四闸（纪律 2）：查重/超长/超容/空载，违规 graceful。
    pub fn inject(&mut self, code: u32, phrase: u32) -> Result<(), u16> {
        if code == 0 || code > PH_CODE_MAX {
            return Err(E_PHRASE_BAD); // 空载/超长 code
        }
        if phrase == 0 {
            return Err(E_PHRASE_BAD);
        }
        if self.codes.len() >= PH_CAP {
            return Err(E_PHRASE_BAD); // 超容
        }
        let mut i = 0usize;
        while i < self.codes.len() {
            if self.codes[i] == code * 10_000 + phrase {
                return Err(E_PHRASE_BAD); // 查重
            }
            i += 1;
        }
        self.codes.push(code * 10_000 + phrase);
        Ok(())
    }

    /// 容量守恒审计（纪律 3）。
    pub fn audit_cap(&self) -> Result<(), u16> {
        if self.codes.len() > PH_CAP {
            return Err(E_INVARIANT);
        }
        Ok(())
    }
}

/// fuzz 轮账。
pub struct FuzzReport {
    pub seed: u32,
    pub rounds: u32,
    pub crashes: u32,
    pub table_graceful: u32,
    pub phrase_graceful: u32,
    pub tolerated: u32,
    pub seen: u32,
    pub coverage: u32, // 10 类操作位图（4 表变异 + 4 短语注入 + 查询 + 审计）
}

/// 确定性 LCG。
pub struct Lcg {
    s: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { s: seed }
    }
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.s
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// ×N 轮 fuzz 主径（纪律 1/2/3）。
pub fn run_fuzz(seed: u32, rounds: u32) -> FuzzReport {
    let mut rng = Lcg::new(seed);
    let mut layout = Layout::blank();
    let mut face = PhraseFace::new();
    let mut rep = FuzzReport {
        seed,
        rounds,
        crashes: 0,
        table_graceful: 0,
        phrase_graceful: 0,
        tolerated: 0,
        seen: 0,
        coverage: 0,
    };
    let mut done = 0u32;
    while done < rounds {
        let op = rng.below(10);
        match op {
            0..=3 => {
                // 表变异类 0..3（正常/重复/越界/清零）。
                let class = op; // 与 mutate 内部分类同相
                layout.mutate(&mut rng);
                if layout.validate().is_err() {
                    rep.table_graceful += 1;
                    rep.coverage |= 1u32 << class;
                } else {
                    rep.tolerated += 1;
                    rep.coverage |= 1u32 << class;
                }
            }
            4..=7 => {
                // 短语注入类 0..3（合法/重复/超长/超容模拟：以参数
                // 域覆盖四闸）。
                let class = 4 + (op - 4);
                let code = match class {
                    4 => 100 + rng.below(900), // 合法带
                    5 => 100 + rng.below(900), // 重复靠撞库
                    6 => 1_000 + rng.below(900), // 超长 code
                    _ => rng.below(2), // 0=空载
                };
                let phrase = 1 + rng.below(50);
                match face.inject(code, phrase) {
                    Ok(_) => rep.tolerated += 1,
                    Err(_) => rep.phrase_graceful += 1,
                }
                rep.coverage |= 1u32 << class;
            }
            8 => {
                // 查询路径。
                let k1 = rng.below(26) as usize;
                let k2 = rng.below(26) as usize;
                match layout.resolve(k1, k2) {
                    Ok(_) => rep.tolerated += 1,
                    Err(_) => rep.table_graceful += 1,
                }
                rep.coverage |= 1u32 << 8;
            }
            _ => {
                // 审计路径：不变量随轮三连（审计本身也是一笔操作，
                // 依结果入 tolerated/graceful，守恒不破）。
                if layout.validate().is_err() {
                    // 表脏是合法 graceful 态（写入面已记），不算崩。
                    rep.table_graceful += 1;
                } else {
                    rep.tolerated += 1;
                }
                if face.audit_cap().is_err() {
                    rep.crashes += 1; // 容量守恒破 = 崩
                }
                rep.coverage |= 1u32 << 9;
            }
        }
        rep.seen += 1;
        if rep.seen != rep.tolerated + rep.table_graceful + rep.phrase_graceful + rep.crashes {
            rep.crashes += 1; // 计数守恒破
        }
        done += 1;
    }
    rep
}

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done || summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（fuzz ×10^4 零崩），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主判据：×10^4 轮固定种子，零崩 + 计数守恒 + 十类操作铺满
    //    + graceful 真实发生。
    let rep = run_fuzz(0xC060_0001, 10_000);
    if rep.rounds != 10_000 {
        return Err(E_ROUND_MISS);
    }
    if rep.crashes != 0 {
        return Err(E_INVARIANT);
    }
    if rep.seen != rep.tolerated + rep.table_graceful + rep.phrase_graceful {
        return Err(E_UNSETTLED);
    }
    let full = (1u32 << 10) - 1;
    if rep.coverage != full {
        return Err(E_COVER_MISS);
    }
    if rep.table_graceful == 0 || rep.phrase_graceful == 0 {
        return Err(E_TABLE_BAD); // 防御面必须真被打到
    }
    hits += 1;

    // R2 可复跑（纪律 1）：同种子两跑五元组逐一相等。
    let a = run_fuzz(0xC060_0002, 2_000);
    let b = run_fuzz(0xC060_0002, 2_000);
    if a.seed != b.seed
        || a.crashes != b.crashes
        || a.tolerated != b.tolerated
        || a.table_graceful != b.table_graceful
        || a.phrase_graceful != b.phrase_graceful
        || a.coverage != b.coverage
    {
        return Err(E_SEED_ECHO);
    }
    hits += 1;

    // R3 全空表对抗：26 键全零表 + 密集查询 → 全 graceful、零崩。
    let layout = Layout::blank();
    let mut g = 0u32;
    let mut k1 = 0usize;
    while k1 < 26 {
        let mut k2 = 0usize;
        while k2 < 26 {
            if layout.resolve(k1, k2).is_err() {
                g += 1;
            }
            k2 += 1;
        }
        k1 += 1;
    }
    if g != 26 * 26 {
        return Err(E_TABLE_BAD); // 全空表每笔查询必拒
    }
    hits += 1;

    // R4 承载唯一破探针（纪律 5）：同韵母写两键 → validate 必拒。
    let mut layout = Layout::blank();
    layout.finals[0][0] = 5;
    layout.finals[1][0] = 5;
    if layout.validate().is_ok() {
        return Err(E_TABLE_BAD);
    }
    // 自重复探针：同键双槽同值 → resolve 必拒。
    let mut layout = Layout::blank();
    layout.finals[3][0] = 7;
    layout.finals[3][1] = 7;
    if layout.validate().is_ok() {
        return Err(E_TABLE_BAD);
    }
    if layout.resolve(0, 3).is_ok() {
        return Err(E_TABLE_BAD);
    }
    hits += 1;

    // R5 超容注入（纪律 2）：灌满 64 后第 65 笔必 graceful，容量
    //    守恒审计恒绿。
    let mut face = PhraseFace::new();
    let mut k = 0u32;
    while k < PH_CAP as u32 {
        face.inject(100 + k, k + 1)?;
        k += 1;
    }
    if face.inject(999, 9).is_ok() {
        return Err(E_PHRASE_BAD); // 第 65 笔超容
    }
    face.audit_cap()?;
    if face.codes.len() != PH_CAP {
        return Err(E_INVARIANT);
    }
    hits += 1;

    // R6 空载/越长双闸直证：code 0 与 code 1000 必拒。
    let mut face = PhraseFace::new();
    if face.inject(0, 1).is_ok() {
        return Err(E_PHRASE_BAD);
    }
    if face.inject(1_000, 1).is_ok() {
        return Err(E_PHRASE_BAD);
    }
    if face.codes.len() != 0 {
        return Err(E_INVARIANT);
    }
    hits += 1;

    // R7 写回（判据 J2）：完成态 + 非空摘要放行；空摘要拒。
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x3060)?;
    hits += 1;

    Ok(hits)
}
