//! UNX-F48657 —— I·出厂默认词库与用户词初始态联签（首启词面齐备）
//! 判据原文：UNX-F48657-J1 联签 ×10 首启齐备；判据锚定 §AI-61 §2 专题四。
//! 判据落地（drill 场景）：R1 核心词库非空；R2 用户词初始为空；
//! R3 版本冻结；R4 签名面有效；R5 联签 ×10；R6 词面计数守恒；
//! R7 越界态显性化；R8 重置幂等；R9 指纹稳定；R10 呈现行。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：默认词库为出厂只读面，用户词初始态恒为零；
//! 联签以词面签名为锚，签名漂移即判据红，留痕逐轮落账。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0940;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_CORE: u16 = SEG_BASE + 1; // 核心词库为空或不足下限
const E_USER: u16 = SEG_BASE + 2; // 用户词初始态非空
const E_VER: u16 = SEG_BASE + 3; // 版本漂移
const E_SIGN: u16 = SEG_BASE + 4; // 签名面校验失败
const E_JOIN: u16 = SEG_BASE + 5; // 联签轮次不足或失配
const E_CONSERV: u16 = SEG_BASE + 6; // 词面计数不守恒
const E_STATE: u16 = SEG_BASE + 7; // 词面状态非法
const E_RESET: u16 = SEG_BASE + 8; // 重置不幂等
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 联签轮次。
pub const JOIN_N: u32 = 10;
/// 出厂词库版本（冻结常数）。
pub const FACTORY_VERSION: u32 = 1_003;
/// 核心词库条目下限。
pub const MIN_CORE: u32 = 5_000;
/// 词面状态闭集：1 未初始化 / 2 已就绪 / 3 已污染。
pub const ST_INIT: u8 = 1;
pub const ST_READY: u8 = 2;
pub const ST_DIRTY: u8 = 3;

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

/// 出厂词面：默认词库 + 用户词初始态。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LexSeed {
    pub core_terms: u32,
    pub user_terms: u32,
    pub version: u32,
    pub state: u8,
}

impl LexSeed {
    /// 出厂构造：核心词库满额、用户词为零、状态未初始化。
    pub fn factory(core: u32) -> LexSeed {
        LexSeed { core_terms: core, user_terms: 0, version: FACTORY_VERSION, state: ST_INIT }
    }

    /// 词面总数守恒：核心 + 用户。
    pub fn total(&self) -> u32 {
        self.core_terms.wrapping_add(self.user_terms)
    }

    /// 签名面：四字段串联 FNV。
    pub fn sign(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&self.core_terms.to_le_bytes());
        buf.extend_from_slice(&self.user_terms.to_le_bytes());
        buf.extend_from_slice(&self.version.to_le_bytes());
        buf.push(self.state);
        fnv1a64(&buf)
    }

    /// 首启齐备判定：核心足额、用户词为零、版本冻结、状态合法。
    pub fn boot_ready(&self) -> Result<(), u16> {
        if self.core_terms < MIN_CORE {
            return Err(E_CORE);
        }
        if self.user_terms != 0 {
            return Err(E_USER);
        }
        if self.version != FACTORY_VERSION {
            return Err(E_VER);
        }
        if self.state != ST_INIT && self.state != ST_READY {
            return Err(E_STATE);
        }
        Ok(())
    }

    /// 越界注入：外部写入用户词 → 词面显性污染态。
    pub fn pollute(&mut self, n: u32) {
        self.user_terms = n;
        self.state = ST_DIRTY;
    }

    /// 重置回出厂初始态（幂等）。
    pub fn reset(&mut self) {
        self.user_terms = 0;
        self.version = FACTORY_VERSION;
        self.state = ST_INIT;
    }
}

/// 联签记录：每轮首启留痕。
/// round：轮次号（1..=JOIN_N）；seed_sign：该轮词面签名；
/// ok：该轮首启齐备判定结果。留痕一旦生成即为只读证据。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JoinRecord {
    pub round: u32,
    pub seed_sign: u64,
    pub ok: bool,
}

/// 联签 ×10：逐轮首启判定并留痕。
/// 任一轮判定失败即整链红（E_JOIN），不留半绿记录。
pub fn join_sign_rounds(seed: &LexSeed, rounds: u32) -> Result<Vec<JoinRecord>, u16> {
    let mut out: Vec<JoinRecord> = vec![];
    let mut r: u32 = 1;
    while r <= rounds {
        let ok = match seed.boot_ready() {
            Ok(()) => true,
            Err(_) => false,
        };
        out.push(JoinRecord { round: r, seed_sign: seed.sign(), ok });
        if !ok {
            return Err(E_JOIN);
        }
        r += 1;
    }
    Ok(out)
}

/// 呈现行：core=<n> user=<n> ver=<v> st=<s>。
pub fn seed_line(s: &LexSeed) -> String {
    let mut out = String::new();
    out.push_str("core=");
    out.push_str(&s.core_terms.to_string());
    out.push_str(" user=");
    out.push_str(&s.user_terms.to_string());
    out.push_str(" ver=");
    out.push_str(&s.version.to_string());
    out.push_str(" st=");
    out.push_str(&s.state.to_string());
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
    let mut g = Lcg::new(0x486_57);

    // R1 核心词库非空：出厂构造核心词 6000 ≥ 下限 5000。
    // 核心词库不足下限视为出厂词面缺件，判据直接红。
    let core = MIN_CORE + 1000;
    let seed = LexSeed::factory(core);
    if seed.core_terms < MIN_CORE {
        return Err(E_CORE);
    }
    hits += 1;

    // R2 用户词初始为空：首启前用户词计数必须为 0，
    // 任何非零即视为污染（越界写入），由 R7 显性化路径兜底。
    if seed.user_terms != 0 {
        return Err(E_USER);
    }
    hits += 1;

    // R3 版本冻结：出厂词库版本为冻结常数，
    // 升级路径不得在本域内改动该值（迁移面另行联测）。
    if seed.version != FACTORY_VERSION {
        return Err(E_VER);
    }
    hits += 1;

    // R4 签名面有效：同态两次一致且非零；变态即变签。
    let s1 = seed.sign();
    let s2 = seed.sign();
    if s1 != s2 || s1 == 0 {
        return Err(E_SIGN);
    }
    let mut alt = seed.clone();
    alt.user_terms = 7;
    if alt.sign() == s1 {
        return Err(E_SIGN);
    }
    hits += 1;

    // R5 联签 ×10：逐轮 boot_ready 全过，留痕全 ok。
    let recs = join_sign_rounds(&seed, JOIN_N)?;
    if recs.len() as u32 != JOIN_N {
        return Err(E_JOIN);
    }
    let mut i = 0usize;
    while i < recs.len() {
        if !recs[i].ok || recs[i].seed_sign != s1 {
            return Err(E_JOIN);
        }
        i += 1;
    }
    hits += 1;

    // R6 词面计数守恒：total == core + user（初始态下 user 为零，
    // 守恒退化为 total == core；污染态下由 R7 的显性化路径接管）。
    if seed.total() != core {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R7 越界态显性化：注入用户词 → boot_ready 拒 E_USER 且态为污染。
    let mut dirty = seed.clone();
    let n = 1 + g.below(100);
    dirty.pollute(n);
    match dirty.boot_ready() {
        Err(E_USER) => {}
        _ => return Err(E_USER),
    }
    if dirty.state != ST_DIRTY {
        return Err(E_STATE);
    }
    hits += 1;

    // R8 重置幂等：污染后重置两次，态与签名回到出厂值。
    dirty.reset();
    let r1 = dirty.sign();
    dirty.reset();
    if dirty.sign() != r1 || r1 != s1 || dirty.state != ST_INIT {
        return Err(E_RESET);
    }
    hits += 1;

    // R9 指纹稳定：联签留痕两次混签一致（同种子同留痕同指纹）。
    // 留痕序列化走 join_bytes，逐轮 round/sign/ok 三段入串。
    let recs2 = join_sign_rounds(&seed, JOIN_N)?;
    let f1 = fnv1a64(&join_bytes(&recs));
    let f2 = fnv1a64(&join_bytes(&recs2));
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行：core=/user=/ver= 关键字在位。
    let line = seed_line(&seed);
    if !has_kw(line.as_bytes(), b"core=6000") || !has_kw(line.as_bytes(), b"user=0")
        || !has_kw(line.as_bytes(), b"ver=1003")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 联签留痕序列化（指纹用）。
pub fn join_bytes(recs: &[JoinRecord]) -> Vec<u8> {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < recs.len() {
        buf.extend_from_slice(&recs[i].round.to_le_bytes());
        buf.extend_from_slice(&recs[i].seed_sign.to_le_bytes());
        buf.push(recs[i].ok as u8);
        i += 1;
    }
    buf
}
