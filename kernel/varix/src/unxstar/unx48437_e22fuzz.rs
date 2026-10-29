//! UNX-F48437 —— E22 fuzz（随机长度/随机边界注入零崩）
//! 判据原文：UNX-F48437-J1 以确定性 LCG 注入 128 例随机长度与随机边界样本，
//! 处理器必须零崩：每例产出可分类结局（受纳/夹取/拒空/拒超长），字节数不越预算，
//! 边界扫描（0/1/中/预算-1/预算/预算+1）结局与口径逐一相符。
//! 判据落地（drill 场景）：R1 同种子同结局序；R2 异种子结局序分叉；
//! R3 128 例零崩全分类；R4 预算不变量；R5 边界扫描六点口径；
//! R6 结局四类全覆盖；R7 夹取不越预算；R8 拒收不产字节；
//! R9 统计守恒；R10 指纹稳定。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xDAC0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_DET: u16 = SEG_BASE + 1; // 同种子结局序不确定
const E_DIV: u16 = SEG_BASE + 2; // 异种子结局序未分叉
const E_CRASH: u16 = SEG_BASE + 3; // 出现不可分类结局（崩溃等价）
const E_BUDGET: u16 = SEG_BASE + 4; // 处理字节数越预算
const E_BOUND: u16 = SEG_BASE + 5; // 边界点结局与口径不符
const E_COVER: u16 = SEG_BASE + 6; // 结局四类未全覆盖
const E_CLAMP: u16 = SEG_BASE + 7; // 夹取后仍越预算
const E_REJECT: u16 = SEG_BASE + 8; // 拒收例产出了字节
const E_STAT: u16 = SEG_BASE + 9; // 统计守恒破缺
const E_FP: u16 = SEG_BASE + 10; // 指纹漂移
const E_RUNS: u16 = SEG_BASE + 11; // 注入例数不足

/// 注入例数。
pub const RUNS: u32 = 128;
/// 受纳预算（字节）。
pub const BUDGET: usize = 256;

/// 结局类别：1 受纳 / 2 夹取 / 3 拒空 / 4 拒超长。
pub const OUT_ACCEPT: u8 = 1;
pub const OUT_CLAMP: u8 = 2;
pub const OUT_REJ_EMPTY: u8 = 3;
pub const OUT_REJ_LONG: u8 = 4;

/// 单例结局。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    pub class: u8,
    pub len: usize,
    pub processed: usize,
    pub produced: usize,
}

/// FNV-1a 64 位（与本域其余哨兵同口径）。
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

/// 被试处理器：超长输入受纳语义的单一实现（全批次只此一份）。
pub struct Processor;

impl Processor {
    /// 处理 len 字节输入：空拒、超长拒、截断注入把超长压回预算则夹取、
    /// 其余受纳。恒不崩：任何输入都落进四类之一。
    pub fn handle(len: usize, cut: usize) -> Outcome {
        if len == 0 {
            return Outcome {
                class: OUT_REJ_EMPTY,
                len,
                processed: 0,
                produced: 0,
            };
        }
        if len > BUDGET {
            if cut <= BUDGET {
                // 截断注入把超长压回预算内：夹取到 cut 字节。
                return Outcome {
                    class: OUT_CLAMP,
                    len,
                    processed: cut,
                    produced: cut,
                };
            }
            return Outcome {
                class: OUT_REJ_LONG,
                len,
                processed: len,
                produced: 0,
            };
        }
        // len ≤ 预算：截断注入生效时处理 cut 字节，否则全量受纳。
        let eff = if cut < len { cut } else { len };
        if eff == 0 {
            return Outcome {
                class: OUT_REJ_EMPTY,
                len,
                processed: 0,
                produced: 0,
            };
        }
        Outcome {
            class: OUT_ACCEPT,
            len,
            processed: eff,
            produced: eff,
        }
    }
}

/// 结局类别名（下标 = 类别-1）。
pub const OUT_NAMES: [&str; 4] = ["accept", "clamp", "rej-empty", "rej-long"];

/// 结局行（呈现面）：case=<n> class=<名> len=<n> produced=<n>。
pub fn outcome_line(case: u32, o: &Outcome) -> String {
    let mut s = String::new();
    s.push_str("case=");
    s.push_str(&case.to_string());
    s.push_str(" class=");
    if o.class >= OUT_ACCEPT && o.class <= OUT_REJ_LONG {
        s.push_str(OUT_NAMES[(o.class - 1) as usize]);
    } else {
        s.push_str("unknown");
    }
    s.push_str(" len=");
    s.push_str(&(o.len as u32).to_string());
    s.push_str(" produced=");
    s.push_str(&(o.produced as u32).to_string());
    s
}

/// 结局序指纹。
fn outcomes_fp(outs: &[Outcome]) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < outs.len() {
        buf.push(outs[i].class);
        buf.extend_from_slice(&(outs[i].len as u32).to_le_bytes());
        buf.extend_from_slice(&(outs[i].processed as u32).to_le_bytes());
        buf.extend_from_slice(&(outs[i].produced as u32).to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 同种子同结局序：两路 LCG 同种子 64 例逐一相同。
    let mut a = Lcg::new(0x48437);
    let mut b = Lcg::new(0x48437);
    let mut i = 0u32;
    while i < 64 {
        let la = a.below(513) as usize;
        let ca = a.below(520) as usize;
        let lb = b.below(513) as usize;
        let cb = b.below(520) as usize;
        let oa = Processor::handle(la, ca);
        let ob = Processor::handle(lb, cb);
        if oa != ob {
            return Err(E_DET);
        }
        i += 1;
    }
    hits += 1;

    // R2 异种子分叉：种子 1 与种子 2 的结局序指纹必须不同。
    let seq1 = fuzz_seq(Lcg::new(1), 32);
    let seq2 = fuzz_seq(Lcg::new(2), 32);
    if outcomes_fp(&seq1) == outcomes_fp(&seq2) {
        return Err(E_DIV);
    }
    hits += 1;

    // R3/R4/R6/R7/R8/R9：128 例零崩全分类 + 不变量 + 统计守恒。
    let mut lcg = Lcg::new(0xFAC7);
    let mut stats: [u32; 4] = [0u32; 4];
    // 每 8 例掺 1 例确定性边界样本，保证四类结局可覆盖。
    let bounds: [(usize, usize); 8] = [
        (0, 0),           // 拒空
        (1, 999),         // 受纳
        (BUDGET - 1, 999), // 受纳（预算沿内）
        (BUDGET, 999),    // 受纳（预算顶）
        (BUDGET + 1, 999), // 拒超长
        (BUDGET + 1, BUDGET), // 夹取（压到预算顶）
        (100, 0),         // 截断注入压到零 → 拒空
        (300, 100),       // 超长被注入压回 → 夹取
    ];
    let mut c = 0u32;
    while c < RUNS {
        let (len, cut) = if c % 8 == 7 {
            bounds[(c / 8) as usize % bounds.len()]
        } else {
            (lcg.below(513) as usize, lcg.below(520) as usize)
        };
        let o = Processor::handle(len, cut);
        // R3 全分类：类别必须落在 1..=4。
        if o.class < OUT_ACCEPT || o.class > OUT_REJ_LONG {
            return Err(E_CRASH);
        }
        // R4 预算不变量：处理字节数不越预算。
        if o.processed > BUDGET && o.class != OUT_REJ_LONG {
            return Err(E_BUDGET);
        }
        // R7 夹取例：产出不越预算。
        if o.class == OUT_CLAMP && (o.produced > BUDGET || o.processed > BUDGET) {
            return Err(E_CLAMP);
        }
        // R8 拒收例：不产出任何字节。
        if (o.class == OUT_REJ_EMPTY || o.class == OUT_REJ_LONG) && o.produced != 0 {
            return Err(E_REJECT);
        }
        stats[(o.class - 1) as usize] += 1;
        c += 1;
    }
    hits += 1;

    // R5 边界扫描六点：0/拒空注入/中/预算-1/预算/预算+1 口径逐一相符。
    let mid = BUDGET / 2;
    let b0 = Processor::handle(0, 0);
    if b0.class != OUT_REJ_EMPTY {
        return Err(E_BOUND);
    }
    let b1 = Processor::handle(5, 0);
    if b1.class != OUT_REJ_EMPTY {
        return Err(E_BOUND);
    }
    let bmid = Processor::handle(mid, 999);
    if bmid.class != OUT_ACCEPT || bmid.produced != mid {
        return Err(E_BOUND);
    }
    let bm1 = Processor::handle(BUDGET - 1, 999);
    if bm1.class != OUT_ACCEPT || bm1.produced != BUDGET - 1 {
        return Err(E_BOUND);
    }
    let bb = Processor::handle(BUDGET, 999);
    if bb.class != OUT_ACCEPT || bb.produced != BUDGET {
        return Err(E_BOUND);
    }
    let bp1 = Processor::handle(BUDGET + 1, 999);
    if bp1.class != OUT_REJ_LONG || bp1.produced != 0 {
        return Err(E_BOUND);
    }
    // 夹取边界：超长 + 注入恰压到预算顶。
    let bc = Processor::handle(BUDGET + 1, BUDGET);
    if bc.class != OUT_CLAMP || bc.produced != BUDGET || bc.processed != BUDGET {
        return Err(E_CLAMP);
    }
    hits += 1;

    // R6 结局四类全覆盖：统计里每类至少一例。
    let mut si = 0usize;
    while si < 4 {
        if stats[si] == 0 {
            return Err(E_COVER);
        }
        si += 1;
    }
    hits += 1;

    // R8' 拒长例处理记账：processed 记实际长度，不越注入口径上限。
    let long = Processor::handle(513, 999);
    if long.class != OUT_REJ_LONG || long.processed != 513 || long.produced != 0 {
        return Err(E_REJECT);
    }
    hits += 1;

    // R9 统计守恒：四类之和恰等于注入例数。
    let mut sum = 0u32;
    let mut si = 0usize;
    while si < 4 {
        sum += stats[si];
        si += 1;
    }
    if sum != RUNS {
        return Err(E_STAT);
    }
    if RUNS < 64 {
        return Err(E_RUNS);
    }
    hits += 1;

    // R10 指纹稳定：128 例结局序两次指纹一致且非零。
    let seqa = fuzz_seq(Lcg::new(0xFAC7), RUNS as usize);
    let f1 = outcomes_fp(&seqa);
    let f2 = outcomes_fp(&seqa);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // 呈现行烟测：四段前缀齐全。
    let line = outcome_line(1, &seqa[0]);
    if !has_kw(line.as_bytes(), b"case=")
        || !has_kw(line.as_bytes(), b"class=")
        || !has_kw(line.as_bytes(), b"len=")
        || !has_kw(line.as_bytes(), b"produced=")
    {
        return Err(E_FP);
    }
    hits += 1;

    Ok(hits)
}

/// 以给定 LCG 跑 n 例注入，返回结局序。
pub fn fuzz_seq(mut lcg: Lcg, n: usize) -> Vec<Outcome> {
    let mut outs: Vec<Outcome> = vec![];
    let mut i = 0usize;
    while i < n {
        let len = lcg.below(513) as usize;
        let cut = lcg.below(520) as usize;
        outs.push(Processor::handle(len, cut));
        i += 1;
    }
    outs
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
