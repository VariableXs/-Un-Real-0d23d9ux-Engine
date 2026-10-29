//! CGPU-F0810 · 认证 fuzz（目标 300 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0810，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：认证 fuzz——对认证记录解析器（结构化整数载荷）做
//!   畸形输入轰炸：定长字段被变异后解析器必须给出闭集错误码，
//!   任何"静默接受畸形"都算 fuzz 命中；语料定容守恒。
//!
//! 设计纪律：
//! - 载荷口径：8 字节定长 → [magic(u8)=0xC5, ver(u8), verdict(u8),
///   signs(u8), subject_hash(u32 LE)]。
//! - 解析闭集错误码：magic 错 / ver 越界 / verdict 越界 /
///   signs 为 0 / hash 为 0——五码之外不得有第六种拒绝理由。
//! - fuzz 命中定义：解析器对畸形载荷返回 Ok 或返回闭集外错误码。
//! - 突变算子闭集四种：Flip / Add / ByteSet / Zero——确定性 LCG 驱动。
//! - 语料账本定容：FUZZ_CAP=72 用例，超容逐出最旧并计数，
///   守恒恒等式：total_cases == held + evicted。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
///   诊断段 0x6FC0 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x6FC0，批内 CGPU-F0810 独占）。
pub const SEG_BASE: u16 = 0x6FC0;

/// 用例编号重复。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 语料账本已满（防御位）。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 载荷长度非法（非 8 字节）。
pub const E_LEN: u16 = SEG_BASE + 3;
/// magic 错。
pub const E_MAGIC: u16 = SEG_BASE + 4;
/// ver 越界（>2）。
pub const E_VER: u16 = SEG_BASE + 5;
/// verdict 越界（>2）。
pub const E_VERDICT: u16 = SEG_BASE + 6;
/// signs 为 0。
pub const E_SIGNS: u16 = SEG_BASE + 7;
/// hash 为 0。
pub const E_HASH: u16 = SEG_BASE + 8;

/// 语料账本容量。
pub const FUZZ_CAP: usize = 72;
/// 载荷定长。
pub const PAYLOAD_LEN: usize = 8;
/// magic 常量。
pub const MAGIC: u8 = 0xC5;
/// ver 上界（0..=2）。
pub const VER_MAX: u8 = 2;
/// verdict 上界（0..=2）。
pub const VERDICT_MAX: u8 = 2;

/// LCG（确定性）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    s: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg { s: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.s = self
            .s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.s
    }
}

/// 合法载荷构造（闭集字段）。
pub fn make_payload(ver: u8, verdict: u8, signs: u8, hash: u32) -> [u8; PAYLOAD_LEN] {
    let mut p = [0u8; PAYLOAD_LEN];
    p[0] = MAGIC;
    p[1] = ver;
    p[2] = verdict;
    p[3] = signs;
    p[4] = (hash & 0xFF) as u8;
    p[5] = ((hash >> 8) & 0xFF) as u8;
    p[6] = ((hash >> 16) & 0xFF) as u8;
    p[7] = ((hash >> 24) & 0xFF) as u8;
    p
}

/// 认证载荷解析器（被测目标）：五码闭集拒绝。
pub fn parse_cert_payload(p: &[u8]) -> Result<(u8, u8, u8, u32), u16> {
    if p.len() != PAYLOAD_LEN {
        return Err(E_LEN);
    }
    if p[0] != MAGIC {
        return Err(E_MAGIC);
    }
    if p[1] > VER_MAX {
        return Err(E_VER);
    }
    if p[2] > VERDICT_MAX {
        return Err(E_VERDICT);
    }
    if p[3] == 0 {
        return Err(E_SIGNS);
    }
    let h = (p[4] as u32)
        | ((p[5] as u32) << 8)
        | ((p[6] as u32) << 16)
        | ((p[7] as u32) << 24);
    if h == 0 {
        return Err(E_HASH);
    }
    Ok((p[1], p[2], p[3], h))
}

/// 拒绝码闭集核对：Err 码必须在五码之内。
pub fn is_closed_set_reject(code: u16) -> bool {
    code == E_MAGIC
        || code == E_VER
        || code == E_VERDICT
        || code == E_SIGNS
        || code == E_HASH
}

/// 突变算子（闭集四种）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutOp {
    Flip,
    Add,
    ByteSet,
    Zero,
}

/// 施加突变（返回新载荷）。
pub fn mutate(op: MutOp, p: &[u8; PAYLOAD_LEN]) -> [u8; PAYLOAD_LEN] {
    let mut out = *p;
    match op {
        MutOp::Flip => out[1] ^= 0x04,
        MutOp::Add => out[2] = out[2].wrapping_add(3),
        MutOp::ByteSet => out[0] = 0x00,
        MutOp::Zero => out[3] = 0,
    }
    out
}

/// fuzz 用例。
#[derive(Clone, Debug)]
pub struct FuzzCase {
    pub id: u32,
    pub payload: [u8; PAYLOAD_LEN],
}

/// 语料账本（环形定容）。
#[derive(Clone, Debug)]
pub struct FuzzCorpus {
    ring: Vec<FuzzCase>,
    evicted: u64,
    total: u64,
}

impl FuzzCorpus {
    pub fn new() -> Self {
        FuzzCorpus {
            ring: Vec::new(),
            evicted: 0,
            total: 0,
        }
    }
    pub fn admit(&mut self, c: FuzzCase) -> Result<(), u16> {
        if self.ring.iter().any(|o| o.id == c.id) {
            return Err(E_DUP);
        }
        if self.ring.len() >= FUZZ_CAP {
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
}

/// 轰炸一轮：对语料每个用例做一次突变并解析；
/// 返回（轮次用例数, 命中数）。命中 = Ok 或闭集外错误码。
pub fn bombard_round(corpus: &FuzzCorpus, seed: u64) -> Result<(u32, u32), u16> {
    let mut rng = Lcg::new(seed);
    let mut n = 0u32;
    let mut hits = 0u32;
    for c in corpus.ring.iter() {
        let op = match rng.next() % 4 {
            0 => MutOp::Flip,
            1 => MutOp::Add,
            2 => MutOp::ByteSet,
            _ => MutOp::Zero,
        };
        let m = mutate(op, &c.payload);
        n += 1;
        match parse_cert_payload(&m) {
            Ok(_) => hits += 1,
            Err(code) => {
                if !is_closed_set_reject(code) {
                    hits += 1;
                }
            }
        }
    }
    Ok((n, hits))
}

/// 自例程：主路径（合法载荷通过）+ 五码闭集逐个触发 + 命中口径 + 守恒。
pub fn drill() -> Result<u32, u16> {
    // 主路径 1：合法载荷。
    let good = make_payload(1, 0, 3, 0x11223344);
    let (v, vd, sg, h) = parse_cert_payload(&good)?;
    if v != 1 || vd != 0 || sg != 3 || h != 0x11223344 {
        return Err(E_VER);
    }
    // 主路径 2：五码闭集逐个触发。
    // magic 错。
    let mut p = good;
    p[0] = 0x00;
    if parse_cert_payload(&p) != Err(E_MAGIC) {
        return Err(E_MAGIC);
    }
    // ver 越界。
    let mut p = good;
    p[1] = 3;
    if parse_cert_payload(&p) != Err(E_VER) {
        return Err(E_VER);
    }
    // verdict 越界。
    let mut p = good;
    p[2] = 9;
    if parse_cert_payload(&p) != Err(E_VERDICT) {
        return Err(E_VERDICT);
    }
    // signs 为 0。
    let mut p = good;
    p[3] = 0;
    if parse_cert_payload(&p) != Err(E_SIGNS) {
        return Err(E_SIGNS);
    }
    // hash 为 0。
    let zero_hash = make_payload(1, 1, 2, 0);
    if parse_cert_payload(&zero_hash) != Err(E_HASH) {
        return Err(E_HASH);
    }
    // 长度非法。
    if parse_cert_payload(&good[..7]) != Err(E_LEN) || parse_cert_payload(&[]) != Err(E_LEN) {
        return Err(E_LEN);
    }
    // 闭集核对：五码都在闭集内，E_LEN/E_DUP 不在。
    if !is_closed_set_reject(E_MAGIC)
        || !is_closed_set_reject(E_VER)
        || !is_closed_set_reject(E_VERDICT)
        || !is_closed_set_reject(E_SIGNS)
        || !is_closed_set_reject(E_HASH)
        || is_closed_set_reject(E_LEN)
        || is_closed_set_reject(E_DUP)
    {
        return Err(E_MAGIC);
    }
    // 突变算子语义。
    let m0 = mutate(MutOp::Flip, &good);
    if m0[1] != good[1] ^ 0x04 {
        return Err(E_VER);
    }
    let m1 = mutate(MutOp::Add, &good);
    if m1[2] != good[2].wrapping_add(3) {
        return Err(E_VERDICT);
    }
    let m2 = mutate(MutOp::ByteSet, &good);
    if m2[0] != 0 {
        return Err(E_MAGIC);
    }
    let m3 = mutate(MutOp::Zero, &good);
    if m3[3] != 0 {
        return Err(E_SIGNS);
    }
    // 主路径 3：畸形语料轰炸——ByteSet/Zero 突变必命中（magic 或 signs 被破坏
    // 但仍返回闭集内错误码，不算命中）；Flip 破坏 ver → 闭集内错误。
    // 构造"静默接受"用例：解析器无洞时命中应为 0（对闭集内拒绝）。
    let mut corpus = FuzzCorpus::new();
    corpus.admit(FuzzCase {
        id: 1,
        payload: make_payload(2, 2, 9, 0xAABBCCDD),
    })?;
    let (n, hits) = bombard_round(&corpus, 42)?;
    if n != 1 {
        return Err(E_CAP);
    }
    // 命中数必须 ≤ 1 且解析器对全部突变只给闭集内错误或合法结果。
    // （payload ver=2,verdict=2,signs=9：Flip→ver^4=6→E_VER 闭集内；
    //   Add→verdict+3=5→E_VERDICT 闭集内；ByteSet→E_MAGIC；Zero→E_SIGNS。）
    // 因此本解析器命中恒为 0。
    if hits != 0 {
        return Err(E_MAGIC);
    }
    // 主路径 4：定容守恒——76 个用例进 72 容语料。
    let mut corpus2 = FuzzCorpus::new();
    for i in 0..76u32 {
        corpus2.admit(FuzzCase {
            id: i + 1,
            payload: make_payload((i % 3) as u8, (i % 3) as u8, ((i % 250) + 1) as u8, i),
        })?;
    }
    corpus2.audit()?;
    if corpus2.held() != FUZZ_CAP || corpus2.evicted != 4 {
        return Err(E_CAP);
    }
    // 大语料轰炸两轮，全部闭集内拒绝或合法。
    let (_, hits2) = bombard_round(&corpus2, 7)?;
    let (_, hits2b) = bombard_round(&corpus2, 7)?;
    if hits2 != 0 || hits2 != hits2b {
        return Err(E_MAGIC);
    }
    // 错误路径：重复编号。
    if corpus
        .admit(FuzzCase {
            id: 1,
            payload: good,
        })
        != Err(E_DUP)
    {
        return Err(E_DUP);
    }
    Ok(0)
}
