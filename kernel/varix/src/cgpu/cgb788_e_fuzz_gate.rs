//! CGPU-F0788 · E 域 fuzz 总闸（目标 320 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥320 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0788，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E 域 fuzz 总闸——把 E 域所有 fuzz 口径收敛为一个总闸：
//!   种子语料 + 确定性整数突变（LCG）+ 批量跑闸 + 首违即停，
//!   语料账本定容守恒，闸门结论闭集（PASS/STOP/ERR）。
//!
//! 设计纪律：
//! - 突变算子闭集四种：Flip（位翻转）、Add（加常数）、Trunc（截断）、
//!   Swap（字节对换）——无第五种；突变由 LCG 种子确定性驱动，可重放。
//! - 跑闸判据：每个用例走 target（整数校验函数），返回 Err 即该用例
//!   记命中；命中数超过闸限 BUDGET_HITS → 总闸 STOP（发现真缺陷）。
//! - 语料账本定容：CORPUS_CAP=64 用例，超容逐出最旧并计数，
//!   守恒恒等式：total_cases == held + evicted。
//! - 总闸结论闭集三态：Pass（全部干净）/ Stop（命中超限）/
//!   Err（口径非法）；无第四种结论。
//! - 纯功能线：no_std + alloc，整数算术，确定性，零随机源外部依赖；
//!   诊断段 0x6DC0 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x6DC0，批内 CGPU-F0788 独占）。
pub const SEG_BASE: u16 = 0x6DC0;

/// 用例编号重复。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 语料账本已满（防御位）。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 用例载荷为空。
pub const E_EMPTY: u16 = SEG_BASE + 3;
/// 突变算子越界。
pub const E_OP: u16 = SEG_BASE + 4;
/// 跑闸轮次口径非法（0 轮）。
pub const E_ROUNDS: u16 = SEG_BASE + 5;
/// 被测函数口径非法。
pub const E_TARGET: u16 = SEG_BASE + 6;

/// 语料账本容量。
pub const CORPUS_CAP: usize = 64;
/// 命中闸限：超过即 STOP。
pub const BUDGET_HITS: u32 = 4;
/// LCG 乘子（Hull–Dobell 全周期参数之一）。
pub const LCG_A: u64 = 6_364_136_223_846_793_005;
/// LCG 增量。
pub const LCG_C: u64 = 1_442_695_040_888_963_407;

/// 突变算子（闭集四种）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutOp {
    /// 位翻转：对首字节异或掩码。
    Flip,
    /// 加常数：首字节加 1（回绕）。
    Add,
    /// 截断：载荷砍半（向下取整，至少留 1 字节）。
    Trunc,
    /// 字节对换：首尾对换。
    Swap,
}

impl MutOp {
    /// 从序号构造（越界报错）。
    pub fn from_ord(o: u8) -> Result<Self, u16> {
        Ok(match o % 8 {
            0 => MutOp::Flip,
            1 => MutOp::Add,
            2 => MutOp::Trunc,
            3 => MutOp::Swap,
            _ => return Err(E_OP),
        })
    }
}

/// 确定性 LCG（可重放）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(LCG_A)
            .wrapping_add(LCG_C);
        self.state
    }
}

/// 对载荷施加一次突变（返回新载荷）。
pub fn mutate(op: MutOp, payload: &[u8]) -> Result<Vec<u8>, u16> {
    if payload.is_empty() {
        return Err(E_EMPTY);
    }
    let mut out = Vec::with_capacity(payload.len());
    match op {
        MutOp::Flip => {
            for (i, b) in payload.iter().enumerate() {
                out.push(if i == 0 { b ^ 0x80 } else { *b });
            }
        }
        MutOp::Add => {
            for (i, b) in payload.iter().enumerate() {
                out.push(if i == 0 { b.wrapping_add(1) } else { *b });
            }
        }
        MutOp::Trunc => {
            let keep = (payload.len() / 2).max(1);
            for b in payload.iter().take(keep) {
                out.push(*b);
            }
        }
        MutOp::Swap => {
            let n = payload.len();
            for (i, b) in payload.iter().enumerate() {
                out.push(if i == 0 { payload[n - 1] } else if i == n - 1 { payload[0] } else { *b });
            }
        }
    }
    Ok(out)
}

/// fuzz 用例。
#[derive(Clone, Debug)]
pub struct FuzzCase {
    /// 用例编号（语料内唯一）。
    pub id: u32,
    /// 载荷。
    pub payload: Vec<u8>,
    /// 该用例历史命中数。
    pub hits: u32,
}

impl FuzzCase {
    pub fn new(id: u32, payload: &[u8]) -> Result<Self, u16> {
        if payload.is_empty() {
            return Err(E_EMPTY);
        }
        Ok(FuzzCase {
            id,
            payload: payload.iter().copied().collect(),
            hits: 0,
        })
    }
}

/// 语料账本（环形定容）。
#[derive(Clone, Debug)]
pub struct Corpus {
    ring: Vec<FuzzCase>,
    evicted: u64,
    total: u64,
}

impl Corpus {
    pub fn new() -> Self {
        Corpus {
            ring: Vec::new(),
            evicted: 0,
            total: 0,
        }
    }
    pub fn admit(&mut self, c: FuzzCase) -> Result<(), u16> {
        if self.ring.iter().any(|o| o.id == c.id) {
            return Err(E_DUP);
        }
        if self.ring.len() >= CORPUS_CAP {
            self.ring.remove(0);
            self.evicted += 1;
        }
        self.ring.push(c);
        self.total += 1;
        Ok(())
    }
    /// 守恒审计：total == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total != self.ring.len() as u64 + self.evicted {
            return Err(E_CAP);
        }
        Ok(())
    }
    pub fn held(&self) -> usize {
        self.ring.len()
    }
    pub fn iter(&self) -> core::slice::Iter<FuzzCase> {
        self.ring.iter()
    }
}

/// 被测目标签名：整数校验函数。
/// 返回 Ok(()) 表示干净；Err(_) 表示该输入命中缺陷口径。
pub type TargetFn = fn(&[u8]) -> Result<(), u16>;

/// 示例被测目标：要求载荷非空、首字节不等于 0xDE、长度 ≤ 8。
pub fn sample_target(p: &[u8]) -> Result<(), u16> {
    if p.is_empty() {
        return Err(E_TARGET);
    }
    if p[0] == 0xDE {
        return Err(E_TARGET);
    }
    if p.len() > 8 {
        return Err(E_TARGET);
    }
    Ok(())
}

/// 总闸结论（闭集三态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateVerdict {
    /// 全部干净。
    Pass,
    /// 命中超限，发现缺陷。
    Stop,
}

/// 跑闸：对语料执行 rounds 轮突变-回放；返回 (结论, 总命中数)。
/// 首违即停：任一轮命中累计超闸限立即返回 Stop。
pub fn run_gate(corpus: &Corpus, rounds: u32, seed: u64, target: TargetFn) -> Result<(GateVerdict, u32), u16> {
    if rounds == 0 {
        return Err(E_ROUNDS);
    }
    let mut hits = 0u32;
    let mut rng = Lcg::new(seed);
    for _ in 0..rounds {
        for c in corpus.iter() {
            let op = MutOp::from_ord((rng.next_u64() % 4) as u8)?;
            let mutated = mutate(op, &c.payload)?;
            if target(&mutated).is_err() {
                hits += 1;
                if hits > BUDGET_HITS {
                    return Ok((GateVerdict::Stop, hits));
                }
            }
        }
    }
    Ok((GateVerdict::Pass, hits))
}

/// 自例程：主路径（干净目标 Pass）+ 脆弱目标 Stop + 错误路径 + 守恒。
pub fn drill() -> Result<u32, u16> {
    // 主路径 1：温和目标（只拒绝空载荷）+ 温和语料 → Pass。
    let mut corpus = Corpus::new();
    corpus.admit(FuzzCase::new(1, &[0x11, 0x22, 0x33, 0x44]))?;
    corpus.admit(FuzzCase::new(2, &[0x55, 0x66]))?;
    corpus.audit()?;
    let gentle_target: TargetFn = |p: &[u8]| {
        if p.is_empty() {
            Err(E_TARGET)
        } else {
            Ok(())
        }
    };
    let (v1, h1) = run_gate(&corpus, 4, 0x5EED, gentle_target)?;
    if v1 != GateVerdict::Pass || h1 != 0 {
        return Err(E_TARGET);
    }
    // 主路径 2：脆弱目标（任何突变输入必中）→ Stop。
    let brittle_target: TargetFn = |_p: &[u8]| Err(E_TARGET);
    let mut corpus2 = Corpus::new();
    corpus2.admit(FuzzCase::new(1, &[0x11]))?;
    let (v2, h2) = run_gate(&corpus2, 8, 7, brittle_target)?;
    if v2 != GateVerdict::Stop || h2 != BUDGET_HITS + 1 {
        return Err(E_TARGET);
    }
    // 确定性重放：同种子同结论。
    let (v2b, h2b) = run_gate(&corpus2, 8, 7, brittle_target)?;
    if v2b != v2 || h2b != h2 {
        return Err(E_ROUNDS);
    }
    // 突变算子语义核对（闭集逐个验证）。
    let p = [0xA0u8, 0xBB, 0xCC];
    let m_flip = mutate(MutOp::Flip, &p)?;
    if m_flip[0] != 0x20 || m_flip.len() != 3 {
        return Err(E_OP);
    }
    let m_add = mutate(MutOp::Add, &p)?;
    if m_add[0] != 0xA1 {
        return Err(E_OP);
    }
    let m_trunc = mutate(MutOp::Trunc, &p)?;
    if m_trunc.len() != 1 {
        return Err(E_OP);
    }
    let m_swap = mutate(MutOp::Swap, &p)?;
    if m_swap[0] != 0xCC || m_swap[2] != 0xA0 {
        return Err(E_OP);
    }
    // from_ord 闭集：0..3 合法，4..7 越界。
    if MutOp::from_ord(4) != Err(E_OP) || MutOp::from_ord(7) != Err(E_OP) {
        return Err(E_OP);
    }
    // 错误路径 1：空载荷。
    if FuzzCase::new(9, &[]) != Err(E_EMPTY) || mutate(MutOp::Flip, &[]) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 错误路径 2：重复编号。
    let dup = FuzzCase::new(1, &[1, 2, 3])?;
    if corpus.admit(dup) != Err(E_DUP) {
        return Err(E_DUP);
    }
    // 错误路径 3：0 轮跑闸。
    if run_gate(&corpus, 0, 1, gentle_target) != Err(E_ROUNDS) {
        return Err(E_ROUNDS);
    }
    // 定容守恒：灌 68 个用例进 64 容语料，恒等式仍成立。
    let mut corpus3 = Corpus::new();
    for i in 0..68u32 {
        let payload = [i as u8, 0x99, 0x88];
        corpus3.admit(FuzzCase::new(i + 1, &payload)?)?;
    }
    corpus3.audit()?;
    if corpus3.held() != CORPUS_CAP || corpus3.total != 68 || corpus3.evicted != 4 {
        return Err(E_CAP);
    }
    // sample_target 口径：0xDE 首字节拒绝、超长拒绝。
    if sample_target(&[0xDE, 1]) != Err(E_TARGET) || sample_target(&[1; 9]) != Err(E_TARGET) {
        return Err(E_TARGET);
    }
    Ok(0)
}
