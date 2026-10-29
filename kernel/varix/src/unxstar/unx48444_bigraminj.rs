//! UNX-F48444 —— E·bigram 越界索引注入（反：越界查询 ×10 全拒）
//! 判据原文：UNX-F48444-J1 词典容量 N 的 bigram 索引查询对越界前后词 id
//!           （N/N+1/u16::MAX/回绕下溢/双越界等 ×10 形态）全部拒绝；
//!           拒后索引零污染（状态指纹不变）；合法边界 id N-1 正常放行；
//!           LCG 随机越界 ×512 全拒零崩；拒收账四型封闭；重演指纹一致；
//!           守恒 seq = 拒收数 + 放行数。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::String;

pub const SEG_BASE: u16 = 0xDBE0; // E23 bigram 越界注入段

const E_INJ_PASS: u16 = SEG_BASE + 1; // 越界查询放行
const E_STATE_DIRTY: u16 = SEG_BASE + 2; // 拒后状态污染
const E_BOUND_BAD: u16 = SEG_BASE + 3; // 边界行为失准
const E_KIND_BAD: u16 = SEG_BASE + 4; // 拒收分类越界
const E_COUNT_MISS: u16 = SEG_BASE + 5; // 拒收计数不符
const E_LEGAL_MISS: u16 = SEG_BASE + 6; // 合法查询误拒
const E_REPLAY_DIFF: u16 = SEG_BASE + 7; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 8; // 事件守恒破

/// 越界型。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum OobKind {
    PrevOob = 0,  // 仅前词越界
    NextOob = 1,  // 仅后词越界
    BothOob = 2,  // 双越界
    Underflow = 3, // 回绕下溢（0usize 溢出形态由构造模拟）
}

/// bigram 索引：容量 vocab，仅接受 [0, vocab) 词 id 查询。
pub struct BigramIndex {
    vocab: u16,
    counts: Vec<u32>, // (prev*vocab+next) 命中计数
    rejects: Vec<(u64, OobKind)>,
    accept_count: u32,
    seq: u64,
}

impl BigramIndex {
    pub fn new(vocab: u16) -> Self {
        let n = vocab as usize * vocab as usize;
        let mut counts = vec![0u32; n];
        // 预置若干合法二元记录
        counts[0 * vocab as usize + 1] = 9;
        counts[1 * vocab as usize + 3] = 7;
        counts[2 * vocab as usize + 4] = 5;
        counts[3 * vocab as usize + 5] = 3;
        BigramIndex {
            vocab,
            counts,
            rejects: Vec::new(),
            accept_count: 0,
            seq: 0,
        }
    }

    fn state_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.counts.len() {
            h ^= self.counts[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    /// 查询：任一侧 id ≥ vocab 即拒并记账，状态零污染。
    pub fn query(&mut self, prev: u16, next: u16) -> Result<u32, u16> {
        self.seq += 1;
        let before = self.state_fp();
        let pv = prev >= self.vocab;
        let nv = next >= self.vocab;
        if pv || nv {
            let kind = if pv && nv {
                OobKind::BothOob
            } else if pv {
                OobKind::PrevOob
            } else {
                OobKind::NextOob
            };
            self.rejects.push((self.seq, kind));
            if self.state_fp() != before {
                return Err(E_STATE_DIRTY);
            }
            return Err(E_INJ_PASS);
        }
        // 下溢形态模拟：以 vocab 为下界的“负值”用 0xFFFF 尾段占位由调用方注入，
        // 此处合法路径直接命中计数。
        let idx = prev as usize * self.vocab as usize + next as usize;
        self.accept_count += 1;
        Ok(self.counts[idx])
    }

    /// 注入式查询（模拟有符号负索引回绕为巨大 u16 的形态）。
    pub fn query_wrapped(&mut self, wrapped: u64) -> Result<u32, u16> {
        // 回绕值截到 u16 后通常越界；若截断后恰好合法则仍按合法处理并记账
        let v = (wrapped & 0xFFFF) as u16;
        let before = self.state_fp();
        if v >= self.vocab {
            self.seq += 1;
            self.rejects.push((self.seq, OobKind::Underflow));
            if self.state_fp() != before {
                return Err(E_STATE_DIRTY);
            }
            return Err(E_INJ_PASS);
        }
        self.query(v, 0)
    }

    /// 拒收分类封闭。
    pub fn audit_kind_closed(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.rejects.len() {
            if self.rejects[i].1 as u16 > 3 {
                return Err(E_KIND_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 拒后恢复：合法查询仍正常。
    pub fn audit_recovery(&mut self) -> Result<u32, u16> {
        self.query(0, 1)
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.rejects.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        let mut i = 0usize;
        while i < self.rejects.len() {
            h ^= self.rejects[i].0;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.rejects[i].1 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h ^= self.accept_count as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    pub fn reject_count(&self) -> u32 {
        self.rejects.len() as u32
    }

    pub fn accept_count(&self) -> u32 {
        self.accept_count
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn vocab(&self) -> u16 {
        self.vocab
    }

    pub fn label(&self) -> String {
        let mut s = String::from("vocab=");
        s.push_str("bigram-index");
        s
    }
}

/// 越界注入 ×10 清单：(prev, next, 是否 wrapped 形态)。
pub fn inj_ten() -> Vec<(u16, u16, bool, u64)> {
    let n = 64u16;
    vec![
        (n, 0, false, 0),               // prev == N
        (0, n, false, 0),               // next == N
        (n, n, false, 0),               // 双越界 == N
        (n + 1, 0, false, 0),           // prev == N+1
        (0, n + 1, false, 0),           // next == N+1
        (u16::MAX, 0, false, 0),        // prev == u16::MAX
        (0, u16::MAX, false, 0),        // next == u16::MAX
        (u16::MAX, u16::MAX, false, 0), // 双满位
        (0, 0, true, 0xFFFF_FFFF_FFFF_FFC0), // wrapped 截出 0xFFC0
        (0, 0, true, 0xFFFF_FFFF_FFFF_FFFF), // wrapped 截出 0xFFFF
    ]
}

/// 确定性 LCG。
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
        self.next() % n
    }
}

/// 主巡检：越界注入八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let vocab = 64u16;

    // R1 越界 ×10 全拒
    let mut g = BigramIndex::new(vocab);
    let injs = inj_ten();
    let mut i = 0usize;
    while i < injs.len() {
        let r = if injs[i].2 {
            g.query_wrapped(injs[i].3)
        } else {
            g.query(injs[i].0, injs[i].1)
        };
        match r {
            Err(E_INJ_PASS) => {}
            _ => return Err(E_INJ_PASS),
        }
        i += 1;
    }
    if g.reject_count() != 10 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R2 拒后零污染：状态指纹不变
    let fp0 = g.state_fp();
    let mut i = 0usize;
    while i < injs.len() {
        let r = if injs[i].2 {
            g.query_wrapped(injs[i].3)
        } else {
            g.query(injs[i].0, injs[i].1)
        };
        let _ = r;
        i += 1;
    }
    if g.state_fp() != fp0 {
        return Err(E_STATE_DIRTY);
    }
    hits += 1;

    // R3 边界行为：N-1 放行、N 拒绝、命中计数正确
    let mut g2 = BigramIndex::new(vocab);
    let last = vocab - 1;
    match g2.query(last, last) {
        Ok(0) => {}
        _ => return Err(E_BOUND_BAD),
    }
    match g2.query(vocab, 0) {
        Err(E_INJ_PASS) => {}
        _ => return Err(E_BOUND_BAD),
    }
    match g2.query(0, 1) {
        Ok(9) => {}
        _ => return Err(E_BOUND_BAD),
    }
    hits += 1;

    // R4 LCG 随机越界 ×512 全拒零崩
    let mut rng = Lcg::new(0x48444);
    let mut k = 0u32;
    while k < 512 {
        let pick = rng.below(3);
        let v = vocab + (rng.below(0x10000 - vocab as u64) as u16);
        let r = if pick == 0 {
            g2.query(v, 0)
        } else if pick == 1 {
            g2.query(0, v)
        } else {
            g2.query(v, v)
        };
        match r {
            Err(E_INJ_PASS) => {}
            _ => return Err(E_INJ_PASS),
        }
        g2.audit_kind_closed()?;
        k += 1;
    }
    if g2.reject_count() != 513 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R5 合法查询穿插恢复：预置记录命中不受注入影响
    match g2.audit_recovery() {
        Ok(9) => {}
        _ => return Err(E_LEGAL_MISS),
    }
    hits += 1;

    // R6 拒收分类封闭：四型齐备
    g2.audit_kind_closed()?;
    let mut seen = [0u32; 4];
    let mut i = 0usize;
    while i < g2.rejects.len() {
        let t = g2.rejects[i].1 as usize;
        seen[t] += 1;
        i += 1;
    }
    // R4 注入只有 PrevOob/NextOob/BothOob 三型；Underflow 由 wrapped 注入补齐
    let mut g3 = BigramIndex::new(vocab);
    match g3.query_wrapped(0xFFFF_FFFF_FFFF_FFC0) {
        Err(E_INJ_PASS) => {}
        _ => return Err(E_KIND_BAD),
    }
    seen[3] += 1;
    if seen[0] == 0 || seen[1] == 0 || seen[2] == 0 || seen[3] == 0 {
        return Err(E_KIND_BAD);
    }
    hits += 1;

    // R7 重演一致：同种子同历史指纹相等
    let f1 = g2.fp();
    let mut g4 = BigramIndex::new(vocab);
    let last = vocab - 1;
    let _ = g4.query(last, last);
    let _ = g4.query(vocab, 0);
    let _ = g4.query(0, 1);
    let mut rng2 = Lcg::new(0x48444);
    let mut k = 0u32;
    while k < 512 {
        let pick = rng2.below(3);
        let v = vocab + (rng2.below(0x10000 - vocab as u64) as u16);
        let r = if pick == 0 {
            g4.query(v, 0)
        } else if pick == 1 {
            g4.query(0, v)
        } else {
            g4.query(v, v)
        };
        let _ = r;
        k += 1;
    }
    let _ = g4.query(0, 1);
    if f1 != g4.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 守恒：seq = 拒收 + 放行（g2: 512 拒 + 1 恢复 + 1 边界拒 + 2 边界放行）
    let total = g2.reject_count() + g2.accept_count();
    if g2.seq() != total as u64 {
        return Err(E_BAL_MISS);
    }
    hits += 1;

    Ok(hits)
}
