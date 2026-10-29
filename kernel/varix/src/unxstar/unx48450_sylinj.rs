//! UNX-F48450 —— E·音节表越界索引注入（反：非法索引 ×10 全拒）
//! 判据原文：UNX-F48450-J1 音节组合器对非法索引（等于表长/大于表长/
//!           u16 满位/组合序列夹带越界项/空序列等 ×10 形态）全部拒绝；
//!           拒后组合器零污染（状态指纹不变）；合法索引序列正常出串；
//!           LCG 随机非法索引 ×512 全拒零崩；拒收账三型封闭；
//!           守恒 seq = 拒收 + 采纳；重演指纹一致。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::String;

pub const SEG_BASE: u16 = 0xDCE0; // E23 音节表越界注入段

const E_INJ_PASS: u16 = SEG_BASE + 1; // 非法索引放行
const E_STATE_DIRTY: u16 = SEG_BASE + 2; // 拒后状态污染
const E_KIND_BAD: u16 = SEG_BASE + 3; // 拒收分类越界
const E_COUNT_MISS: u16 = SEG_BASE + 4; // 拒收计数不符
const E_LEGAL_MISS: u16 = SEG_BASE + 5; // 合法序列误拒
const E_REPLAY_DIFF: u16 = SEG_BASE + 6; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 7; // 守恒破
const E_SHAPE_BAD: u16 = SEG_BASE + 8; // 形态校验失准

/// 拒收型。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum InjKind {
    SoloOob = 0,   // 单索引越界
    SeqOob = 1,    // 序列夹带越界
    EmptySeq = 2,  // 空序列
}

/// 音节表：固定容量，条目为编码三元组。
pub struct SylBank {
    cap: u16,
    entries: Vec<(u8, u8, u8)>,
    rejects: Vec<(u64, InjKind)>,
    accept_count: u32,
    seq: u64,
}

impl SylBank {
    pub fn new(cap: u16) -> Self {
        let mut entries = vec![(0u8, 0u8, 0u8); cap as usize];
        let mut i = 0usize;
        while i < cap as usize {
            entries[i] = ((i % 8 + 1) as u8, (i % 5 + 1) as u8, (i % 4 + 1) as u8);
            i += 1;
        }
        SylBank { cap, entries, rejects: Vec::new(), accept_count: 0, seq: 0 }
    }

    fn state_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.entries.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        let mut i = 0usize;
        while i < self.entries.len() {
            h ^= self.entries[i].0 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.entries[i].1 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.entries[i].2 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    /// 组合：校验全部索引在 [0, cap) 后才出串；任一越界整串拒收。
    pub fn compose(&mut self, idx: &[u16]) -> Result<String, u16> {
        self.seq += 1;
        let before = self.state_fp();
        if idx.is_empty() {
            self.rejects.push((self.seq, InjKind::EmptySeq));
            if self.state_fp() != before {
                return Err(E_STATE_DIRTY);
            }
            return Err(E_INJ_PASS);
        }
        let mut i = 0usize;
        while i < idx.len() {
            if idx[i] >= self.cap {
                let kind = if idx.len() == 1 { InjKind::SoloOob } else { InjKind::SeqOob };
                self.rejects.push((self.seq, kind));
                if self.state_fp() != before {
                    return Err(E_STATE_DIRTY);
                }
                return Err(E_INJ_PASS);
            }
            i += 1;
        }
        // 全合法：出串（音节码映射为小写字母序列，确定性）
        let mut s = String::new();
        let mut i = 0usize;
        while i < idx.len() {
            let e = self.entries[idx[i] as usize];
            let c1 = (b'a' + e.0 - 1) as char;
            let c2 = (b'a' + e.1 - 1) as char;
            let c3 = (b'0' + e.2) as char;
            s.push(c1);
            s.push(c2);
            s.push(c3);
            i += 1;
        }
        self.accept_count += 1;
        Ok(s)
    }

    /// 单查：越界索引直接拒（供注入清单逐项复验）。
    pub fn probe(&mut self, idx: u16) -> Result<(u8, u8, u8), u16> {
        self.seq += 1;
        let before = self.state_fp();
        if idx >= self.cap {
            self.rejects.push((self.seq, InjKind::SoloOob));
            if self.state_fp() != before {
                return Err(E_STATE_DIRTY);
            }
            return Err(E_INJ_PASS);
        }
        self.accept_count += 1;
        Ok(self.entries[idx as usize])
    }

    pub fn audit_kind_closed(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.rejects.len() {
            if self.rejects[i].1 as u16 > 2 {
                return Err(E_KIND_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
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

    pub fn cap(&self) -> u16 {
        self.cap
    }
}

/// 非法索引注入 ×10 清单：(probe 形态 / compose 序列形态)。
pub fn inj_ten() -> Vec<Vec<u16>> {
    let cap = 32u16;
    vec![
        vec![cap],                // == cap
        vec![cap + 1],            // == cap+1
        vec![u16::MAX],           // 满位
        vec![1, 2, cap],          // 序列尾越界
        vec![cap, 1, 2],          // 序列首越界
        vec![0, cap + 9, 3],      // 序列中越界
        vec![cap, cap, cap],      // 全越界
        vec![0],                  // 合法单索引（必须采纳）
        vec![0, 1, 2],            // 合法序列（必须采纳）
        vec![],                   // 空序列
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
    let cap = 32u16;

    // R1 注入 ×10：8 拒 2 采
    let mut b = SylBank::new(cap);
    let injs = inj_ten();
    let mut i = 0usize;
    while i < injs.len() {
        let r = b.compose(&injs[i]);
        if i == 7 || i == 8 {
            match r {
                Ok(_) => {}
                _ => return Err(E_LEGAL_MISS),
            }
        } else {
            match r {
                Err(E_INJ_PASS) => {}
                _ => return Err(E_INJ_PASS),
            }
        }
        i += 1;
    }
    if b.reject_count() != 8 {
        return Err(E_COUNT_MISS);
    }
    if b.accept_count() != 2 {
        return Err(E_LEGAL_MISS);
    }
    hits += 1;

    // R2 拒后零污染
    let fp0 = b.state_fp();
    let mut i = 0usize;
    while i < injs.len() {
        let _ = b.compose(&injs[i]);
        i += 1;
    }
    if b.state_fp() != fp0 {
        return Err(E_STATE_DIRTY);
    }
    hits += 1;

    // R3 probe 形态：cap/cap+1/u16::MAX 全拒，cap-1 放行
    let mut b2 = SylBank::new(cap);
    match b2.probe(cap) {
        Err(E_INJ_PASS) => {}
        _ => return Err(E_SHAPE_BAD),
    }
    match b2.probe(cap + 1) {
        Err(E_INJ_PASS) => {}
        _ => return Err(E_SHAPE_BAD),
    }
    match b2.probe(u16::MAX) {
        Err(E_INJ_PASS) => {}
        _ => return Err(E_SHAPE_BAD),
    }
    match b2.probe(cap - 1) {
        Ok(e) => {
            if e.0 == 0 {
                return Err(E_SHAPE_BAD);
            }
        }
        _ => return Err(E_SHAPE_BAD),
    }
    hits += 1;

    // R4 LCG 随机非法索引 ×512 全拒零崩
    let mut rng = Lcg::new(0x48450);
    let mut k = 0u32;
    while k < 512 {
        let v = cap + (rng.below(0x10000 - cap as u64) as u16);
        match b2.probe(v) {
            Err(E_INJ_PASS) => {}
            _ => return Err(E_INJ_PASS),
        }
        b2.audit_kind_closed()?;
        k += 1;
    }
    if b2.reject_count() != 515 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R5 合法序列穿插恢复：出串内容确定性
    let mut b3 = SylBank::new(cap);
    let s1 = b3.compose(&[0, 1, 2])?;
    let s2 = b3.compose(&[0, 1, 2])?;
    if s1 != s2 || s1.is_empty() {
        return Err(E_LEGAL_MISS);
    }
    // 越界拒绝后合法串依旧
    let _ = b3.compose(&[cap, 0]);
    let s3 = b3.compose(&[0, 1, 2])?;
    if s3 != s1 {
        return Err(E_LEGAL_MISS);
    }
    hits += 1;

    // R6 拒收账三型封闭
    b3.audit_kind_closed()?;
    let mut seen = [0u32; 3];
    let mut b4 = SylBank::new(cap);
    let _ = b4.compose(&[cap]);        // SoloOob
    let _ = b4.compose(&[1, cap]);     // SeqOob
    let _ = b4.compose(&[]);           // EmptySeq
    let mut i = 0usize;
    while i < b4.rejects.len() {
        seen[b4.rejects[i].1 as usize] += 1;
        i += 1;
    }
    if seen[0] == 0 || seen[1] == 0 || seen[2] == 0 {
        return Err(E_KIND_BAD);
    }
    hits += 1;

    // R7 重演一致：同种子指纹相等
    let f1 = b2.fingerprint();
    let mut b5 = SylBank::new(cap);
    let _ = b5.probe(cap);
    let _ = b5.probe(cap + 1);
    let _ = b5.probe(u16::MAX);
    let _ = b5.probe(cap - 1);
    let mut rng2 = Lcg::new(0x48450);
    let mut k = 0u32;
    while k < 512 {
        let v = cap + (rng2.below(0x10000 - cap as u64) as u16);
        let _ = b5.probe(v);
        k += 1;
    }
    if f1 != b5.fingerprint() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 守恒：seq = 拒收 + 采纳（b2: 515 拒 + 1 采 = 516）
    let total = b2.reject_count() + b2.accept_count();
    if b2.seq() != total as u64 {
        return Err(E_BAL_MISS);
    }
    hits += 1;

    Ok(hits)
}
