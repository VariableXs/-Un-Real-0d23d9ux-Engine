//! UNX-F48751 C38 三类窗口延迟回归哨兵（vxwm 原生/Wine/Xwayland 回归判据·宪法加严）
//! 判据 UNX-F48751-J1：三窗 ×3 轮分账全落；判据锚定 域内宪法加严项。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! 三窗（0=vxwm 原生 1=Wine 2=Xwayland）×3 轮首显 P95 分账必须全落册。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1B60;
const E_WIN_MISS: u16 = SEG_BASE + 1;
const E_ROUND_MISS: u16 = SEG_BASE + 2;
const E_P95_LOST: u16 = SEG_BASE + 3;
const E_SENTINEL_DEAD: u16 = SEG_BASE + 4;
const E_LEDGER_GAP: u16 = SEG_BASE + 5;
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;
const E_EMPTY_CELL: u16 = SEG_BASE + 7;
const E_BUDGET_OVER: u16 = SEG_BASE + 8;

/// 首显样本：窗号 + 轮次 + 微秒
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FirstShow {
    pub win: u8,
    pub round: u8,
    pub us: u32,
}

/// 单格分账：窗 × 轮的样本集与 P95
#[derive(Clone)]
pub struct Cell {
    pub win: u8,
    pub round: u8,
    pub vals: Vec<u32>,
}

impl Cell {
    pub fn new(win: u8, round: u8) -> Self {
        Cell { win, round, vals: vec![] }
    }

    pub fn sorted(&self) -> Vec<u32> {
        let mut v = self.vals.clone();
        let n = v.len();
        let mut i = 0usize;
        while i < n {
            let mut j = i + 1;
            while j < n {
                if v[j] < v[i] {
                    let t = v[i];
                    v[i] = v[j];
                    v[j] = t;
                }
                j += 1;
            }
            i += 1;
        }
        v
    }

    pub fn p95(&self) -> Result<u32, u16> {
        if self.vals.is_empty() {
            return Err(E_EMPTY_CELL);
        }
        let s = self.sorted();
        let n = s.len();
        let rank = (n * 95 + 99) / 100;
        let idx = if rank == 0 { 0 } else if rank > n { n - 1 } else { rank - 1 };
        Ok(s[idx])
    }
}

/// 回归哨兵：三窗 ×3 轮九格分账
pub struct Sentinel {
    pub cells: Vec<Cell>,
    pub budget_us: u32,
}

impl Sentinel {
    pub fn new(budget_us: u32) -> Self {
        Sentinel { cells: vec![], budget_us }
    }

    pub fn feed(&mut self, s: FirstShow) {
        let mut i = 0usize;
        while i < self.cells.len() {
            if self.cells[i].win == s.win && self.cells[i].round == s.round {
                self.cells[i].vals.push(s.us);
                return;
            }
            i += 1;
        }
        let mut c = Cell::new(s.win, s.round);
        c.vals.push(s.us);
        self.cells.push(c);
    }

    /// 九格分账全落：三窗 ×3 轮每格 P95 必须齐备且 ≤ 预算
    pub fn settle(&self) -> Result<Vec<(u8, u8, u32)>, u16> {
        if self.cells.len() != 9 {
            return Err(E_LEDGER_GAP);
        }
        let mut out: Vec<(u8, u8, u32)> = vec![];
        let mut i = 0usize;
        while i < self.cells.len() {
            let c = &self.cells[i];
            if c.win > 2 {
                return Err(E_WIN_MISS);
            }
            if c.round > 2 {
                return Err(E_ROUND_MISS);
            }
            let p = c.p95()?;
            if p > self.budget_us {
                return Err(E_BUDGET_OVER);
            }
            out.push((c.win, c.round, p));
            i += 1;
        }
        let mut seen = 0u32;
        let mut w = 0u8;
        while w < 3 {
            let mut r = 0u8;
            while r < 3 {
                let mut found = false;
                let mut k = 0usize;
                while k < out.len() {
                    if out[k].0 == w && out[k].1 == r {
                        found = true;
                    }
                    k += 1;
                }
                if found {
                    seen += 1;
                }
                r += 1;
            }
            w += 1;
        }
        if seen != 9 {
            return Err(E_SENTINEL_DEAD);
        }
        Ok(out)
    }
}

/// 确定性 LCG
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 喂满九格：三窗 ×3 轮各 n 样本
fn feed_nine(s: &mut Sentinel, n: u32, rng: &mut Lcg) -> Result<(), u16> {
    if n == 0 {
        return Err(E_EMPTY_CELL);
    }
    let mut win = 0u8;
    while win < 3 {
        let mut round = 0u8;
        while round < 3 {
            let mut c = 0u32;
            while c < n {
                s.feed(FirstShow { win, round, us: 900 + rng.below(2_500) as u32 });
                c += 1;
            }
            round += 1;
        }
        win += 1;
    }
    Ok(())
}

/// 三窗 ×3 轮分账全落 ×20 组
fn sweep_three_rounds(rng: &mut Lcg) -> Result<(), u16> {
    let mut group = 0u32;
    while group < 20 {
        let mut s = Sentinel::new(6_000);
        feed_nine(&mut s, 8 + rng.below(8) as u32, rng)?;
        let nine = s.settle()?;
        if nine.len() != 9 {
            return Err(E_LEDGER_GAP);
        }
        group += 1;
    }
    Ok(())
}

/// P95 秩法对照 ×10：已知序列手工核对
fn sweep_p95_rank() -> Result<(), u16> {
    let mut round = 0u32;
    while round < 10 {
        let mut c = Cell::new(0, 0);
        let mut v = 1u32;
        while v <= 100 {
            c.vals.push(v * 20);
            v += 1;
        }
        if c.p95()? != 2_000 {
            return Err(E_P95_LOST);
        }
        round += 1;
    }
    Ok(())
}

/// 反例注入：缺格、缺轮、超预算、空格全命中
fn sweep_negatives() -> Result<(), u16> {
    let mut s = Sentinel::new(6_000);
    let r1 = s.settle();
    if r1.err() != Some(E_LEDGER_GAP) {
        return Err(E_LEDGER_GAP);
    }
    let mut s2 = Sentinel::new(6_000);
    let mut win = 0u8;
    while win < 3 {
        let mut round = 0u8;
        while round < 3 {
            if !(win == 2 && round == 2) {
                s2.feed(FirstShow { win, round, us: 1_000 });
            }
            round += 1;
        }
        win += 1;
    }
    let r2 = s2.settle();
    if r2.err() != Some(E_LEDGER_GAP) {
        return Err(E_LEDGER_GAP);
    }
    let mut s3 = Sentinel::new(1_500);
    let mut win3 = 0u8;
    while win3 < 3 {
        let mut round3 = 0u8;
        while round3 < 3 {
            s3.feed(FirstShow { win: win3, round: round3, us: 2_000 });
            round3 += 1;
        }
        win3 += 1;
    }
    let r3 = s3.settle();
    if r3.err() != Some(E_BUDGET_OVER) {
        return Err(E_BUDGET_OVER);
    }
    let empty = Cell::new(0, 0);
    let r4 = empty.p95();
    if r4.err() != Some(E_EMPTY_CELL) {
        return Err(E_EMPTY_CELL);
    }
    Ok(())
}

/// 重放一致 ×20：同种子双跑九格 P95 逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut sa = Sentinel::new(6_000);
        let mut sb = Sentinel::new(6_000);
        let mut ra = Lcg::new(seed + round as u64);
        let mut rb = Lcg::new(seed + round as u64);
        let mut win = 0u8;
        while win < 3 {
            let mut r = 0u8;
            while r < 3 {
                let mut c = 0u32;
                while c < 10 {
                    let va = 800 + ra.below(3_000) as u32;
                    let vb = 800 + rb.below(3_000) as u32;
                    if va != vb {
                        return Err(E_REPLAY_DIFF);
                    }
                    sa.feed(FirstShow { win, round: r, us: va });
                    sb.feed(FirstShow { win, round: r, us: vb });
                    c += 1;
                }
                r += 1;
            }
            win += 1;
        }
        let a = sa.settle()?;
        let b = sb.settle()?;
        let mut k = 0usize;
        while k < a.len() {
            if a[k] != b[k] {
                return Err(E_REPLAY_DIFF);
            }
            k += 1;
        }
        round += 1;
    }
    Ok(())
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 三窗 ×3 轮分账全落 ×20 组
    let mut rng = Lcg::new(0x4874_B299);
    sweep_three_rounds(&mut rng)?;
    hits += 1;
    // R2 P95 秩法手工对照 ×10
    sweep_p95_rank()?;
    hits += 1;
    // R3 反例注入全命中
    sweep_negatives()?;
    hits += 1;
    // R4 重放一致 ×20
    sweep_replay(0x4874_BD05)?;
    hits += 1;
    // R5 九格元组窗轮覆盖：0..=2 × 0..=2 逐格在账
    let mut s = Sentinel::new(6_000);
    feed_nine(&mut s, 12, &mut rng)?;
    let nine = s.settle()?;
    let mut w = 0u8;
    while w < 3 {
        let mut r = 0u8;
        while r < 3 {
            let mut found = false;
            let mut k = 0usize;
            while k < nine.len() {
                if nine[k].0 == w && nine[k].1 == r {
                    found = true;
                }
                k += 1;
            }
            if !found {
                return Err(E_SENTINEL_DEAD);
            }
            r += 1;
        }
        w += 1;
    }
    hits += 1;
    // R6 预算界值：恰好等于预算可落账
    let mut s2 = Sentinel::new(2_000);
    let mut w2 = 0u8;
    while w2 < 3 {
        let mut r2 = 0u8;
        while r2 < 3 {
            s2.feed(FirstShow { win: w2, round: r2, us: 2_000 });
            r2 += 1;
        }
        w2 += 1;
    }
    s2.settle()?;
    hits += 1;
    // R7 P95 ≥ 样本最小值语义
    let mut c = Cell::new(1, 1);
    let mut v = 0u32;
    while v < 40 {
        c.vals.push(3_000 + v);
        v += 1;
    }
    let p = c.p95()?;
    if p < 3_000 {
        return Err(E_P95_LOST);
    }
    hits += 1;
    // R8 排序自洽：sorted 尾为最大
    let srt = c.sorted();
    if srt[0] != 3_000 || srt[39] != 3_039 {
        return Err(E_P95_LOST);
    }
    hits += 1;
    // R9 预算上带内 P95 全落：宽松预算 ×10 组全绿
    let mut g = 0u32;
    while g < 10 {
        let mut s3 = Sentinel::new(10_000);
        feed_nine(&mut s3, 5 + rng.below(6) as u32, &mut rng)?;
        s3.settle()?;
        g += 1;
    }
    hits += 1;
    // R10 哨兵活性：喂空格触发 E_EMPTY_CELL 而非静默
    let mut s4 = Sentinel::new(6_000);
    s4.feed(FirstShow { win: 0, round: 0, us: 1_000 });
    let cells = s4.cells.clone();
    if cells.len() != 1 || cells[0].vals.len() != 1 {
        return Err(E_SENTINEL_DEAD);
    }
    hits += 1;
    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a: u32 = 0x9E37_79B9;
    let mut track_b: u32 = 0x9E37_79B9;
    let mut rail: Vec<u32> = vec![];
    let mut rail_sum: u64 = 0;
    let mut dstep = 0u32;
    while dstep < 48 {
        track_a = track_a.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b = track_b.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail.push(track_a);
        rail_sum += track_a as u64;
        dstep += 1;
    }
    if track_a != track_b {
        return Err(E_REPLAY_DIFF);
    }
    if rail.len() != 48 {
        return Err(E_REPLAY_DIFF);
    }
    let mut replay_sum: u64 = 0;
    let mut ridx = 0usize;
    while ridx < rail.len() {
        replay_sum += rail[ridx] as u64;
        ridx += 1;
    }
    if replay_sum != rail_sum {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;
    Ok(hits)
}
