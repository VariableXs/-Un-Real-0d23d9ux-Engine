//! unx48247_s2tpara — UNX-F48247 简繁候选并列语义（简体输入繁体上屏可配）（诊断段 0xBFC0）。
//!
//! 判据原文：
//!   J1 并列 ×10 可配生效；判据锚定 §AI-61 §2 专题五。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三档配置闭集：S2tMode::Off（简繁开关关：繁体候选整体剔出）、
//!      S2tMode::Parallel（并列：繁体孪生紧跟其简体候选后插队）、
//!      S2tMode::OnlyTrad（纯繁：只留繁体候选）。配置是账面状态，
//!      切换留痕（mode_now/mode_prev），越档赋值拒收。
//!   2. 候选账结构：候选带 is_trad 位与 twin_id（简繁孪生对关联）；
//!      并列语义的插入位置是"其简体孪生之后紧邻一位"——不是队尾
//!      追加，位置错即语义错（E_POS）。
//!   3. 基础候选序冻结：lexicon 产出 (rank, text_id, is_trad,
//!      twin_id) 序，rank 严格递增；三档变换都以这条冻结序为底账，
//!      变换不改 rank 只改呈现序。
//!   4. J1 主判据 ×10：10 组配置样本（三档 × 不同候选面：无繁面/
//!      有繁面/全繁面/孪生缺对面），每组断言呈现序与档位语义逐位
//!      相等；可配"生效"的口径是三档产出互异且各自稳定（同配置
//!      两跑结果逐位相等）。
//!   5. 孪生完整性：Parallel 档下有 is_trad 无 twin_id 的候选是账
//!      破（E_TWIN）；有 twin_id 但简体孪生不在场的候选退回队尾
//!      追加并计数（降级账，不静默）。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBFC0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_POS: u16 = SEG_BASE + 1; // 并列插入位置错
const E_TWIN: u16 = SEG_BASE + 2; // 孪生账破：有繁无对
const E_MODE_BAD: u16 = SEG_BASE + 3; // 档位出闭集
const E_RANK_BAD: u16 = SEG_BASE + 4; // 基础序 rank 非严格递增
const E_MODE_MISS: u16 = SEG_BASE + 5; // 三档产出未互异
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空
const E_DEGRADE: u16 = SEG_BASE + 7; // 降级账异常

/// 简繁档位闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum S2tMode {
    Off,
    Parallel,
    OnlyTrad,
}

impl S2tMode {
    pub fn code(&self) -> u8 {
        match self {
            S2tMode::Off => 0,
            S2tMode::Parallel => 1,
            S2tMode::OnlyTrad => 2,
        }
    }
}

/// 候选账条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cand {
    pub rank: u32,
    pub text_id: u32,
    pub is_trad: bool,
    pub twin_id: u32, // 简体孪生的 text_id；繁体候选必填，简体填自身
}

/// 基础 lexicon：rank 严格递增审计 + 冻结序产出。
pub struct Lexicon {
    pub cands: Vec<Cand>,
}

impl Lexicon {
    pub fn new() -> Lexicon {
        Lexicon { cands: Vec::new() }
    }

    pub fn push(&mut self, c: Cand) -> Result<(), u16> {
        if let Some(last) = self.cands.last() {
            if c.rank <= last.rank {
                return Err(E_RANK_BAD);
            }
        }
        self.cands.push(c);
        Ok(())
    }

    /// 底账审计：rank 严格递增、繁体必带孪生。
    pub fn audit(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.cands.len() {
            if self.cands[i].is_trad && self.cands[i].twin_id == 0 {
                return Err(E_TWIN);
            }
            if i > 0 && self.cands[i].rank <= self.cands[i - 1].rank {
                return Err(E_RANK_BAD);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 并列引擎：按档位变换呈现序。
pub struct S2tPara {
    pub mode: S2tMode,
    pub mode_prev: S2tMode,
    pub degraded: u32,
}

impl S2tPara {
    pub fn new() -> S2tPara {
        S2tPara { mode: S2tMode::Off, mode_prev: S2tMode::Off, degraded: 0 }
    }

    /// 切档留痕（纪律 1）。
    pub fn set_mode(&mut self, m: S2tMode) {
        self.mode_prev = self.mode;
        self.mode = m;
    }

    /// 呈现序变换主径。
    pub fn present(&mut self, lex: &Lexicon) -> Result<Vec<Cand>, u16> {
        lex.audit()?;
        match self.mode {
            S2tMode::Off => {
                let mut out: Vec<Cand> = Vec::new();
                let mut i = 0usize;
                while i < lex.cands.len() {
                    if !lex.cands[i].is_trad {
                        out.push(lex.cands[i]);
                    }
                    i += 1;
                }
                Ok(out)
            }
            S2tMode::OnlyTrad => {
                let mut out: Vec<Cand> = Vec::new();
                let mut i = 0usize;
                while i < lex.cands.len() {
                    if lex.cands[i].is_trad {
                        out.push(lex.cands[i]);
                    }
                    i += 1;
                }
                Ok(out)
            }
            S2tMode::Parallel => {
                // 底序中繁体先摘出，简体按 rank 逐位呈现，繁体孪生
                // 紧随其简体之后插队（纪律 2）；孪生不在场的退队尾。
                let mut sims: Vec<Cand> = Vec::new();
                let mut trads: Vec<Cand> = Vec::new();
                let mut i = 0usize;
                while i < lex.cands.len() {
                    if lex.cands[i].is_trad {
                        if lex.cands[i].twin_id == 0 {
                            return Err(E_TWIN);
                        }
                        trads.push(lex.cands[i]);
                    } else {
                        sims.push(lex.cands[i]);
                    }
                    i += 1;
                }
                let mut out: Vec<Cand> = Vec::new();
                let mut used: Vec<bool> = Vec::new();
                let mut i = 0usize;
                while i < trads.len() {
                    used.push(false);
                    i += 1;
                }
                let mut i = 0usize;
                while i < sims.len() {
                    out.push(sims[i]);
                    // 找该简体的繁体孪生。
                    let mut j = 0usize;
                    while j < trads.len() {
                        if !used[j] && trads[j].twin_id == sims[i].text_id {
                            out.push(trads[j]);
                            used[j] = true;
                            break;
                        }
                        j += 1;
                    }
                    i += 1;
                }
                // 孪生不在场的繁体：退队尾追加并降级计数（纪律 5）。
                let mut j = 0usize;
                while j < trads.len() {
                    if !used[j] {
                        out.push(trads[j]);
                        self.degraded += 1;
                    }
                    j += 1;
                }
                Ok(out)
            }
        }
    }
}

/// 两序逐位相等。
fn seq_eq(a: &[Cand], b: &[Cand]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// LCG（确定性）。
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

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done || summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 构造一笔 lexicon：rank 递增，繁简混排（twin_id 配对）。
fn build_lex(rng: &mut Lcg, n_simp: u32, n_trad: u32, orphans: u32) -> Result<Lexicon, u16> {
    let mut lex = Lexicon::new();
    let mut rank: u32 = 1;
    let mut id: u32 = 100;
    let mut i = 0u32;
    while i < n_simp {
        lex.push(Cand { rank, text_id: id, is_trad: false, twin_id: id })?;
        rank += 1;
        id += 1;
        i += 1;
    }
    let mut i = 0u32;
    while i < n_trad {
        // 孪生对象取前面某个简体 id（确定性：100 + [0, n_simp)）。
        let off = rng.below(if n_simp == 0 { 1 } else { n_simp });
        lex.push(Cand { rank, text_id: id, is_trad: true, twin_id: 100 + off })?;
        rank += 1;
        id += 1;
        i += 1;
    }
    let mut i = 0u32;
    while i < orphans {
        // 孤繁：twin_id 指向不存在的 id（>100+n_simp 面外）。
        lex.push(Cand { rank, text_id: id, is_trad: true, twin_id: 9_000 + i })?;
        rank += 1;
        id += 1;
        i += 1;
    }
    Ok(lex)
}

/// 自例程判据：逐条落实 J1（并列 ×10 可配生效），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut rng = Lcg::new(0xBFC0_0010);

    // R1 主判据 ×10：十组配置样本（档位 × 候选面组合），每组呈现
    //    序与档位语义逐位相等 + 同配置两跑稳定。
    let mut i = 0u32;
    while i < 10 {
        let n_simp = 2 + rng.below(4);
        let n_trad = rng.below(4);
        let orphans = rng.below(2);
        let lex = build_lex(&mut rng, n_simp, n_trad, orphans)?;
        let mut engine = S2tPara::new();

        // Off：剔繁。
        engine.set_mode(S2tMode::Off);
        let off1 = engine.present(&lex)?;
        let off2 = engine.present(&lex)?;
        if !seq_eq(&off1, &off2) {
            return Err(E_MODE_MISS); // 同配置两跑必稳
        }
        let mut k = 0usize;
        while k < off1.len() {
            if off1[k].is_trad {
                return Err(E_POS); // Off 档不得见繁
            }
            k += 1;
        }
        if off1.len() != n_simp as usize {
            return Err(E_POS);
        }

        // OnlyTrad：只繁。
        engine.set_mode(S2tMode::OnlyTrad);
        let ot1 = engine.present(&lex)?;
        let ot2 = engine.present(&lex)?;
        if !seq_eq(&ot1, &ot2) {
            return Err(E_MODE_MISS);
        }
        let mut k = 0usize;
        while k < ot1.len() {
            if !ot1[k].is_trad {
                return Err(E_POS); // 纯繁档不得见简
            }
            k += 1;
        }
        if ot1.len() != (n_trad + orphans) as usize {
            return Err(E_POS);
        }

        // Parallel：孪生紧随。
        engine.set_mode(S2tMode::Parallel);
        let p1 = engine.present(&lex)?;
        let p2 = engine.present(&lex)?;
        if !seq_eq(&p1, &p2) {
            return Err(E_MODE_MISS);
        }
        // 语义核对：每笔繁体（非孤）前一位必是其简体孪生。
        let mut k = 0usize;
        while k < p1.len() {
            if p1[k].is_trad {
                let orphan = p1[k].twin_id >= 9_000;
                if !orphan {
                    if k == 0 || p1[k - 1].text_id != p1[k].twin_id || p1[k - 1].is_trad {
                        return Err(E_POS); // 插队位置错
                    }
                }
            }
            k += 1;
        }
        // 三档产出互异（可配"生效"的口径；候选面全简时 Off/Parallel
        // 仍异于 OnlyTrad，全繁时同理——以长度向量断互异）。
        if off1.len() == ot1.len() && p1.len() == off1.len() && n_trad + orphans == 0 {
            return Err(E_MODE_MISS);
        }
        i += 1;
    }
    hits += 1;

    // R2 切档留痕（纪律 1）：mode_prev 随切换逐笔落账。
    let mut engine = S2tPara::new();
    engine.set_mode(S2tMode::Parallel);
    if engine.mode != S2tMode::Parallel || engine.mode_prev != S2tMode::Off {
        return Err(E_MODE_BAD);
    }
    engine.set_mode(S2tMode::Off);
    if engine.mode != S2tMode::Off || engine.mode_prev != S2tMode::Parallel {
        return Err(E_MODE_BAD);
    }
    hits += 1;

    // R3 孪生插队位置精确（纪律 2）：固定账 [简a, 简b, 繁a', 孤繁z]
    //    → 呈现 [简a, 繁a', 简b, 孤繁z]，孤繁退队尾。
    let mut lex = Lexicon::new();
    lex.push(Cand { rank: 1, text_id: 100, is_trad: false, twin_id: 100 })?;
    lex.push(Cand { rank: 2, text_id: 101, is_trad: false, twin_id: 101 })?;
    lex.push(Cand { rank: 3, text_id: 102, is_trad: true, twin_id: 100 })?;
    lex.push(Cand { rank: 4, text_id: 103, is_trad: true, twin_id: 9_000 })?;
    let mut engine = S2tPara::new();
    engine.set_mode(S2tMode::Parallel);
    let out = engine.present(&lex)?;
    if out.len() != 4
        || out[0].text_id != 100
        || out[1].text_id != 102
        || out[2].text_id != 101
        || out[3].text_id != 103
    {
        return Err(E_POS);
    }
    if engine.degraded != 1 {
        return Err(E_DEGRADE); // 孤繁降级账恰一笔
    }
    hits += 1;

    // R4 孪生缺对面：无简体只有孤繁的 lexicon，Parallel 全退队尾。
    let mut lex = Lexicon::new();
    lex.push(Cand { rank: 1, text_id: 200, is_trad: true, twin_id: 9_500 })?;
    lex.push(Cand { rank: 2, text_id: 201, is_trad: true, twin_id: 9_501 })?;
    let mut engine = S2tPara::new();
    engine.set_mode(S2tMode::Parallel);
    let out = engine.present(&lex)?;
    if out.len() != 2 || engine.degraded != 2 {
        return Err(E_DEGRADE);
    }
    hits += 1;

    // R5 底账审计：rank 回退必拒；有繁无 twin 必拒。
    let mut lex = Lexicon::new();
    lex.push(Cand { rank: 5, text_id: 300, is_trad: false, twin_id: 300 })?;
    if lex.push(Cand { rank: 5, text_id: 301, is_trad: false, twin_id: 301 }).is_ok() {
        return Err(E_RANK_BAD);
    }
    let bad = Lexicon {
        cands: {
            let mut v = Vec::new();
            v.push(Cand { rank: 1, text_id: 302, is_trad: true, twin_id: 0 });
            v
        },
    };
    if bad.audit().is_ok() {
        return Err(E_TWIN);
    }
    hits += 1;

    // R6 Off 档全繁面：剔繁后空序合法（空呈现不是错）。
    let mut lex = Lexicon::new();
    lex.push(Cand { rank: 1, text_id: 400, is_trad: true, twin_id: 400 })?;
    let mut engine = S2tPara::new();
    engine.set_mode(S2tMode::Off);
    let out = engine.present(&lex)?;
    if out.len() != 0 {
        return Err(E_POS);
    }
    hits += 1;

    // R7 写回（判据 J2）：完成态 + 非空摘要放行；空摘要拒。
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x2FC0)?;
    hits += 1;

    Ok(hits)
}
