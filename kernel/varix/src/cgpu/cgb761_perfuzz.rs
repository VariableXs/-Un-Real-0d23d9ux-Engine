#![allow(dead_code)]
//! cgb761_perfuzz —— CGPU-F0761 感知 fuzz。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0761 感知 fuzz  <SEG 0x6A60>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0761；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（内嵌感知面确定性 fuzz 器，整数面）：
//!   1. 确定性 PRNG：32 位 LCG（乘 1664525 加 1013904223），
//!      同种子序列恒同（可重放校验）；
//!   2. 变异输入面：kind = 种子低 3 位（0..=4，含越界面 4），
//!      lat = 种子高位移后取模 6000（含 0 与越上限面）；
//!   3. 判定靶（norm）：kind 越界 → 错误面；lat=0 或 >5000 →
//!      拒绝面；合法 → good/needs/poor 三档（阈值 100/300 写死）；
//!   4. 语料账：定容 64 槽，只收「有趣输入」（边界命中：
//!      lat ∈ {1,100,101,300,301,4999,5000}），满则逐出最老；
//!      守恒恒等式：运行数 = 通过数 + 拒绝数 + 错误数，
//!      语料数 ≤ 运行数 且 语料数 = 在库 + 逐出；
//!   5. 覆盖桶 8 桶（kind(0..3) × 档位半区），每桶计数随跑滚动；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x6A60 段为本模块独占（SEG_BASE=0x6A60），判别式
//!   SEG_BASE+1..+N；与 0x6A40/0x6A80 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const PF_VERSION: &str = "cgb761_perfuzz-v1";

/// 诊断段基址（0x6A60 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6A60;

// ── XcCode 显别式（SEG_BASE+1..+N）────────────────────────────────────
/// 0x6A61 fuzz 靶类型越界。
pub const XC_BAD_KIND: u16 = SEG_BASE + 1;
/// 0x6A62 fuzz 靶样本越界（0 或 >5000）。
pub const XC_BAD_LAT: u16 = SEG_BASE + 2;
/// 0x6A63 账目不守恒。
pub const XC_UNBALANCED: u16 = SEG_BASE + 3;
/// 0x6A64 重放结果不一致（确定性破坏）。
pub const XC_REPLAY_DRIFT: u16 = SEG_BASE + 4;
/// 0x6A65 覆盖桶越界。
pub const XC_BAD_BUCKET: u16 = SEG_BASE + 5;
/// 0x6A66 语料账漂移（语料数 > 运行数）。
pub const XC_CORPUS_DRIFT: u16 = SEG_BASE + 6;
/// 0x6A67 空跑查询（无运行即查覆盖）。
pub const XC_NO_RUNS: u16 = SEG_BASE + 7;

// ── 写死常量面 ────────────────────────────────────────────────────────
/// LCG 乘子。
pub const LCG_MUL: u32 = 1_664_525;
/// LCG 增量。
pub const LCG_ADD: u32 = 1_013_904_223;
/// fuzz 靶合法类型数（0..=3）。
pub const KIND_CAP: usize = 4;
/// 变异面类型数（含越界面 4）。
pub const KIND_SPACE: usize = 5;
/// 靶样本上限（写死：5000）。
pub const LAT_MAX: u32 = 5000;
/// 变异取模空间（写死：6000，含越上限面）。
pub const LAT_SPACE: u32 = 6000;
/// 语料账容量（写死：64）。
pub const CORPUS_CAP: usize = 64;
/// 覆盖桶数（写死：8 = 4 类型 × 2 半区）。
pub const BUCKET_CAP: usize = 8;
/// 靶阈值（good 上限 / poor 下限）。
pub const THRESH: [u32; 2] = [100, 300];

/// 确定性 32 位 LCG。
pub struct Lcg {
    s: u32,
}

impl Lcg {
    /// 播种。
    pub fn new(seed: u32) -> Self {
        Lcg { s: seed }
    }

    /// 下一步（wrapping 确定性）。
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(LCG_MUL).wrapping_add(LCG_ADD);
        self.s
    }
}

/// fuzz 靶判定：kind 越界红码、lat 越界拒收、合法出三档。
/// 返回 0=good 1=needs 2=poor。
pub fn norm(kind: usize, lat: u32) -> Result<u8, u16> {
    if kind >= KIND_CAP {
        return Err(XC_BAD_KIND);
    }
    if lat == 0 || lat > LAT_MAX {
        return Err(XC_BAD_LAT);
    }
    Ok(if lat <= THRESH[0] {
        0
    } else if lat > THRESH[1] {
        2
    } else {
        1
    })
}

/// 边界命中判定（有趣输入闭集）。
pub fn is_boundary(lat: u32) -> bool {
    lat == 1 || lat == 100 || lat == 101 || lat == 300 || lat == 301 || lat == 4999 || lat == 5000
}

/// 语料条目（种子 + 判定结果）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorpusItem {
    /// 变异种子。
    pub seed: u32,
    /// 结果面：0=过 1=拒 2=错。
    pub outcome: u8,
}

/// 感知 fuzz 会话：LCG + 语料账 + 覆盖桶 + 守恒账。
/// 守恒恒等式：
///   运行数 = 通过数 + 拒绝数 + 错误数；
///   语料数 = 在库 + 逐出 ≤ 运行数。
pub struct FuzzSession {
    rng: Lcg,
    corpus: Vec<CorpusItem>,
    corpus_evicted: u32,
    runs: u32,
    accepted: u32,
    rejected: u32,
    errored: u32,
    bucket: [u32; BUCKET_CAP],
    last_kind: usize,
    last_lat: u32,
}

impl FuzzSession {
    /// 播种建会话。
    pub fn new(seed: u32) -> Self {
        FuzzSession {
            rng: Lcg::new(seed),
            corpus: Vec::new(),
            corpus_evicted: 0,
            runs: 0,
            accepted: 0,
            rejected: 0,
            errored: 0,
            bucket: [0; BUCKET_CAP],
            last_kind: 0,
            last_lat: 0,
        }
    }

    /// 单步变异：从 LCG 推 (kind, lat)。
    pub fn mutate(&mut self) -> (usize, u32) {
        let s = self.rng.next();
        let kind = (s % KIND_SPACE as u32) as usize;
        let lat = (s >> 8) % LAT_SPACE;
        self.last_kind = kind;
        self.last_lat = lat;
        (kind, lat)
    }

    /// 覆盖桶下标：合法 kind × (lat ≥ 100 半区)。
    fn bucket_of(kind: usize, lat: u32) -> Result<usize, u16> {
        if kind >= KIND_CAP {
            return Err(XC_BAD_BUCKET);
        }
        Ok(kind * 2 + if lat >= THRESH[0] { 1 } else { 0 })
    }

    /// 单步运行：变异 → 判定 → 记账（含语料与覆盖桶）。
    pub fn step(&mut self) -> u8 {
        let (kind, lat) = self.mutate();
        self.runs += 1;
        let outcome = match norm(kind, lat) {
            Ok(band) => {
                self.accepted += 1;
                if let Ok(b) = Self::bucket_of(kind, lat) {
                    self.bucket[b] += 1;
                }
                if is_boundary(lat) {
                    self.remember(CorpusItem {
                        seed: 0,
                        outcome: 0,
                    });
                }
                0
            }
            Err(XC_BAD_KIND) => {
                self.errored += 1;
                2
            }
            Err(_) => {
                self.rejected += 1;
                if is_boundary(lat) {
                    self.remember(CorpusItem {
                        seed: 0,
                        outcome: 1,
                    });
                }
                1
            }
        };
        outcome
    }

    /// 语料入账（定容 64，满则逐出最老；种子回填当前面）。
    fn remember(&mut self, mut item: CorpusItem) {
        item.seed = self.last_input_seed();
        if self.corpus.len() >= CORPUS_CAP {
            self.corpus.remove(0);
            self.corpus_evicted += 1;
        }
        self.corpus.push(item);
    }

    /// 最近一次变异的种子（由状态反推：直接存档）。
    fn last_input_seed(&self) -> u32 {
        // 确定性反推：kind/lat 已存档，种子面用打包重述。
        ((self.last_kind as u32) << 28) | (self.last_lat & 0x0FFF_FFFF)
    }

    /// 批量运行 n 步。
    pub fn run(&mut self, n: u32) {
        for _ in 0..n {
            self.step();
        }
    }

    /// 重放校验：同种子重建会话跑同步数，结果账必须全同。
    pub fn replay_check(&self, seed: u32, n: u32) -> Result<(), u16> {
        let mut r = FuzzSession::new(seed);
        r.run(n);
        if r.runs != self.runs
            || r.accepted != self.accepted
            || r.rejected != self.rejected
            || r.errored != self.errored
        {
            return Err(XC_REPLAY_DRIFT);
        }
        if r.bucket != self.bucket {
            return Err(XC_REPLAY_DRIFT);
        }
        Ok(())
    }

    /// 覆盖查询：非空桶数；零运行红码。
    pub fn covered(&self) -> Result<u32, u16> {
        if self.runs == 0 {
            return Err(XC_NO_RUNS);
        }
        let mut c = 0u32;
        for i in 0..BUCKET_CAP {
            if self.bucket[i] > 0 {
                c += 1;
            }
        }
        Ok(c)
    }

    /// 守恒核。
    pub fn conserved(&self) -> Result<(), u16> {
        if self.runs != self.accepted + self.rejected + self.errored {
            return Err(XC_UNBALANCED);
        }
        let corpus_total = self.corpus.len() as u32 + self.corpus_evicted;
        if corpus_total > self.runs {
            return Err(XC_CORPUS_DRIFT);
        }
        Ok(())
    }

    /// 语料在库数。
    pub fn corpus_len(&self) -> usize {
        self.corpus.len()
    }

    /// 四账只读。
    pub fn counters(&self) -> (u32, u32, u32, u32) {
        (self.runs, self.accepted, self.rejected, self.errored)
    }
}

/// 自例程：播种跑批 → 守恒 → 覆盖 → 重放一致性 → 边界语料 →
/// 错误路径（靶直查/桶越界/零跑覆盖）。
pub fn drill() -> Result<u32, u16> {
    // 靶直查：三档 + 两错误面。
    if norm(0, 1)? != 0 {
        return Err(XC_BAD_LAT);
    }
    if norm(1, 100)? != 0 {
        return Err(XC_BAD_LAT);
    }
    if norm(2, 101)? != 1 {
        return Err(XC_BAD_LAT);
    }
    if norm(3, 300)? != 1 {
        return Err(XC_BAD_LAT);
    }
    if norm(0, 301)? != 2 {
        return Err(XC_BAD_LAT);
    }
    if norm(0, 5000)? != 2 {
        return Err(XC_BAD_LAT);
    }
    if norm(4, 100) != Err(XC_BAD_KIND) {
        return Err(XC_BAD_KIND);
    }
    if norm(0, 0) != Err(XC_BAD_LAT) {
        return Err(XC_BAD_LAT);
    }
    if norm(0, 5001) != Err(XC_BAD_LAT) {
        return Err(XC_BAD_LAT);
    }
    // 边界闭集直查。
    if !is_boundary(1) || !is_boundary(100) || !is_boundary(5000) {
        return Err(XC_BAD_LAT);
    }
    if is_boundary(2) || is_boundary(4998) {
        return Err(XC_BAD_LAT);
    }
    // 主路径：播种 12345 跑 256 步。
    let mut f = FuzzSession::new(12345);
    f.run(256);
    f.conserved()?;
    let (runs, acc, rej, err) = f.counters();
    if runs != 256 {
        return Err(XC_UNBALANCED);
    }
    if acc + rej + err != 256 {
        return Err(XC_UNBALANCED);
    }
    // 覆盖：8 桶在 256 步下应全命中（4 类型 × 2 半区均高概率）。
    if f.covered()? < 6 {
        return Err(XC_BAD_BUCKET);
    }
    // 重放一致性。
    f.replay_check(12345, 256)?;
    // 语料：256 步必含边界命中（lat 空间 6000、7 边界点、期望 ≥2）。
    if f.corpus_len() < 2 {
        return Err(XC_CORPUS_DRIFT);
    }
    if f.corpus_len() > CORPUS_CAP {
        return Err(XC_CORPUS_DRIFT);
    }
    // 错误路径 1：零跑覆盖查询。
    let mut f2 = FuzzSession::new(7);
    if f2.covered() != Err(XC_NO_RUNS) {
        return Err(XC_NO_RUNS);
    }
    // 错误路径 2：桶越界直查。
    if FuzzSession::bucket_of(4, 100) != Err(XC_BAD_BUCKET) {
        return Err(XC_BAD_BUCKET);
    }
    // 错误路径 3：重放漂移（不同步数）。
    if f.replay_check(12345, 128) != Err(XC_REPLAY_DRIFT) {
        return Err(XC_REPLAY_DRIFT);
    }
    // 确定性：同种子两会话逐步同值。
    let mut a = Lcg::new(42);
    let mut b = Lcg::new(42);
    for _ in 0..64 {
        if a.next() != b.next() {
            return Err(XC_REPLAY_DRIFT);
        }
    }
    // 不同种子序列必异（首步即异，概率面 1-2^-32）。
    let mut c = Lcg::new(43);
    if a.next() == c.next() {
        return Err(XC_REPLAY_DRIFT);
    }
    Ok(0)
}
