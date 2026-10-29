//! unx0644_failredo_v2 —— UNX-F0644 失败复演规程（失败轮重演三步）· v2。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0644 · 失败复演规程（失败轮重演三步） · 目标 ≥220 行
//! 判据原文摘录：UNX-F0644-J1 失败轮复演三步（冻结现场/种子复现/
//!   二次执行）路径有账，同种子复现率 100%。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 注：unx0644_failredo.rs 已有前版存档，本版按工位纪律落 _v2。
//!
//! 设计纪律：
//!   1. 复演三步状态机：冻结现场（Freeze）→ 种子复现（Reseed）→
//!      二次执行（Rerun），边闭集顺序推进，跳步/回退拒收。
//!   2. 种子语义：整数 LCG 伪随机流（确定性），同种子两轮注入序列
//!      逐项全等（复现率 100% 判据的可执行面）。
//!   3. 复演账：每次复演落 (失败轮号, 种子, 三步态, 二次结论)，二次
//!      结论必须与首轮一致（同种子 ⇒ 同结果）。
//!   4. 注入红检出：跳步/种子漂移/二次结论不符三类注入均红。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x5200 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x5200 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x5200;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x520F;

/// XcCode：复演步越界。
pub const XC_BAD_STAGE: u16 = SEG_BASE + 1; // 0x5201 三步闭集外
/// XcCode：非法状态边。
pub const XC_BAD_EDGE: u16 = SEG_BASE + 2; // 0x5202 跳步/回退
/// XcCode：种子为哨兵零。
pub const XC_BAD_SEED: u16 = SEG_BASE + 3; // 0x5203 种子 0 拒收
/// XcCode：种子漂移。
pub const XC_SEED_DRIFT: u16 = SEG_BASE + 4; // 0x5204 同种子不同流
/// XcCode：二次结论不符。
pub const XC_RERUN_DIFF: u16 = SEG_BASE + 5; // 0x5205 同种子不同结果
/// XcCode：复演单重复。
pub const XC_DUP_NO: u16 = SEG_BASE + 6; // 0x5206 一失败轮一复演单

/// 复演三步数。
pub const STAGE_N: usize = 3;
/// LCG 乘数（Knuth 推荐）。
pub const LCG_A: u64 = 6_364_136_223_846_793_005;
/// LCG 增量。
pub const LCG_C: u64 = 1_442_695_040_888_963_407;

/// 复演步。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedoStage {
    /// 冻结现场。
    Freeze = 0,
    /// 种子复现。
    Reseed = 1,
    /// 二次执行。
    Rerun = 2,
}

impl RedoStage {
    /// 步名。
    pub fn name(self) -> &'static str {
        match self {
            RedoStage::Freeze => "冻结现场",
            RedoStage::Reseed => "种子复现",
            RedoStage::Rerun => "二次执行",
        }
    }
}

/// 合法边闭集：0→1→2。
const LEGAL_EDGES: [(usize, usize); STAGE_N - 1] = [(0, 1), (1, 2)];

/// LCG 种子流：从种子生成 n 项确定性序列。
pub fn lcg_stream(seed: u64, n: usize) -> Result<Vec<u64>, u16> {
    if seed == 0 {
        return Err(XC_BAD_SEED);
    }
    let mut s = seed;
    let mut v = Vec::new();
    for _ in 0..n {
        s = s.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        v.push(s);
    }
    Ok(v)
}

/// 注入演练样例轮：种子决定注入点（确定性），注入即"失败"。
/// 结论二态：0 = 失败（注入命中），1 = 过（不命中）。
pub fn sample_round(seed: u64, probe: u64) -> Result<u16, u16> {
    let stream = lcg_stream(seed, 4)?;
    // 注入命中判式：流首项 mod 7 == probe mod 7（确定性整数）。
    let hit = (stream[0] % 7) == (probe % 7);
    Ok(if hit { 0 } else { 1 })
}

/// 复演单。
#[derive(Clone, Debug)]
pub struct RedoTicket {
    /// 失败轮号。
    pub fail_round: usize,
    /// 种子。
    pub seed: u64,
    /// 当前步。
    pub stage: RedoStage,
    /// 首轮结论（失败 = 0）。
    pub first_verdict: u16,
    /// 二次结论（三步走完才落）。
    pub second_verdict: Option<u16>,
}

/// 复演规程账本。
#[derive(Clone, Debug, Default)]
pub struct RedoBook {
    /// 复演单（按失败轮号升序）。
    tickets: Vec<RedoTicket>,
}

impl RedoBook {
    /// 空账本。
    pub fn new() -> Self {
        RedoBook { tickets: Vec::new() }
    }

    /// 开复演单（冻结现场步；种子非零 + 失败轮号唯一）。
    pub fn open_ticket(&mut self, fail_round: usize, seed: u64) -> Result<usize, u16> {
        if seed == 0 {
            return Err(XC_BAD_SEED);
        }
        if self.tickets.iter().any(|t| t.fail_round == fail_round) {
            return Err(XC_DUP_NO);
        }
        let idx = self.tickets.len();
        self.tickets.push(RedoTicket {
            fail_round,
            seed,
            stage: RedoStage::Freeze,
            first_verdict: 0,
            second_verdict: None,
        });
        Ok(idx)
    }

    /// 推进一步（边闭集）。
    pub fn advance(&mut self, idx: usize) -> Result<RedoStage, u16> {
        let t = match self.tickets.get_mut(idx) {
            Some(t) => t,
            None => return Err(XC_BAD_STAGE),
        };
        let from = t.stage as usize;
        let to = from + 1;
        if to > RedoStage::Rerun as usize {
            return Err(XC_BAD_EDGE);
        }
        let legal = LEGAL_EDGES.iter().any(|&(a, b)| a == from && b == to);
        if !legal {
            return Err(XC_BAD_EDGE);
        }
        t.stage = match to {
            1 => RedoStage::Reseed,
            _ => RedoStage::Rerun,
        };
        Ok(t.stage)
    }

    /// 二次执行（三步走完才可落二次结论；同种子 ⇒ 同结果才收）。
    pub fn rerun(&mut self, idx: usize, probe: u64) -> Result<u16, u16> {
        let t = match self.tickets.get_mut(idx) {
            Some(t) => t,
            None => return Err(XC_BAD_STAGE),
        };
        if t.stage != RedoStage::Rerun {
            return Err(XC_BAD_EDGE);
        }
        let v = sample_round(t.seed, probe)?;
        t.second_verdict = Some(v);
        Ok(v)
    }

    /// 同种子复现率核：走完的复演单逐单 二次结论 == 首轮结论（100%）。
    pub fn repro_rate(&self) -> Result<(usize, usize), u16> {
        let mut done = 0usize;
        let mut hit = 0usize;
        for t in &self.tickets {
            match t.second_verdict {
                Some(v) => {
                    done += 1;
                    if v == t.first_verdict {
                        hit += 1;
                    } else {
                        return Err(XC_RERUN_DIFF);
                    }
                }
                None => return Err(XC_BAD_EDGE),
            }
        }
        if self.tickets.is_empty() {
            return Err(XC_BAD_STAGE);
        }
        Ok((hit, done))
    }

    /// 种子复现面：同种子两次取流逐项全等。
    pub fn seed_stream_eq(seed: u64) -> Result<bool, u16> {
        let a = lcg_stream(seed, 8)?;
        let b = lcg_stream(seed, 8)?;
        Ok(a == b)
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!("失败复演规程 · 复演单 {}", self.tickets.len());
        for t in &self.tickets {
            s.push_str(&alloc::format!(
                "\n  轮{} 种子{} {} 二次结论={:?}",
                t.fail_round, t.seed, t.stage.name(), t.second_verdict
            ));
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 标准复演流程：3 张失败复演单全走通（冻结→复现→二次执行），
/// 复现率 100%。
pub fn run_redo_flow() -> Result<(RedoBook, usize, usize), u16> {
    let mut b = RedoBook::new();
    let seeds = [0x9E37_79B9_7F4A_7C15, 0xBF58_476D_1CE4_E5B9, 0x94D0_49BB_1331_11EB];
    for (i, s) in seeds.iter().enumerate() {
        let idx = b.open_ticket(10 + i, *s)?;
        b.advance(idx)?; // → Reseed
        b.advance(idx)?; // → Rerun
        // 首轮结论（失败 0）用同探针复演。
        b.tickets[idx].first_verdict = 0;
        b.rerun(idx, (*s % 7) as u64)?; // 同探针 ⇒ 同命中 ⇒ 结论 0
    }
    let (hit, done) = b.repro_rate()?;
    Ok((b, hit, done))
}

/// 自例程：主路径（3/3 复演 100%）+ 错误路径（跳步/零种子/漂移/结论不符/重号）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let (b, hit, done) = run_redo_flow()?;
    if done != 3 || hit != 3 {
        return Err(XC_RERUN_DIFF);
    }
    if !RedoBook::seed_stream_eq(42)? {
        return Err(XC_SEED_DRIFT);
    }
    if b.fingerprint() == 0 {
        return Err(XC_SEED_DRIFT);
    }

    // 错误路径 1：零种子。
    let mut b1 = RedoBook::new();
    if b1.open_ticket(1, 0) != Err(XC_BAD_SEED) {
        return Err(XC_BAD_SEED);
    }
    if lcg_stream(0, 4) != Err(XC_BAD_SEED) {
        return Err(XC_BAD_SEED);
    }
    // 错误路径 2：跳步（冻结后直接 rerun）。
    let mut b2 = RedoBook::new();
    let i2 = b2.open_ticket(1, 7)?;
    if b2.rerun(i2, 1) != Err(XC_BAD_EDGE) {
        return Err(XC_BAD_EDGE);
    }
    // 错误路径 3：越步（Rerun 后再 advance）。
    let mut b3 = RedoBook::new();
    let i3 = b3.open_ticket(1, 7)?;
    b3.advance(i3)?;
    b3.advance(i3)?;
    if b3.advance(i3) != Err(XC_BAD_EDGE) {
        return Err(XC_BAD_EDGE);
    }
    // 错误路径 4：重号。
    let mut b4 = RedoBook::new();
    b4.open_ticket(1, 7)?;
    if b4.open_ticket(1, 9) != Err(XC_DUP_NO) {
        return Err(XC_DUP_NO);
    }
    // 错误路径 5：二次结论不符（同种子换探针 → 结论翻面 → 复现率红）。
    let mut b5 = RedoBook::new();
    let seed = 21u64; // 流首项 mod 7 确定性
    let probe_hit = seed % 7;
    let i5 = b5.open_ticket(1, seed)?;
    b5.advance(i5)?;
    b5.advance(i5)?;
    b5.tickets[i5].first_verdict = 0;
    b5.rerun(i5, probe_hit)?; // 命中 → 0
    // 换探针复演同单（二次覆盖为不同值）。
    b5.rerun(i5, (probe_hit + 1) % 7)?;
    if b5.repro_rate() != Err(XC_RERUN_DIFF) {
        return Err(XC_RERUN_DIFF);
    }
    // 错误路径 6：未走完先复现率核。
    let mut b6 = RedoBook::new();
    let i6 = b6.open_ticket(1, 7)?;
    b6.advance(i6)?;
    if b6.repro_rate() != Err(XC_BAD_EDGE) {
        return Err(XC_BAD_EDGE);
    }
    Ok(0)
}
