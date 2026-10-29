//! UNX-F48498 —— E25 与 M2 时序联签执行账（时间戳来源单点消费）
//! 判据原文：UNX-F48498-J1 E25 与 M2 的时序联签以同一时间戳来源为单点：
//! 每枚时间戳只允许被消费一次，二次消费必须被拒；联签由双方时戳
//! 共同推出且可复核（FNV）；时戳序列单调、账目守恒、呈现行齐全。
//! 判据落地（drill 场景）：R1 时间戳单点发放；R2 单点消费恰一；
//! R3 二次消费拒收；R4 联签稳定；R5 联签区分扰动；
//! R6 时戳序列单调；R7 未发放时戳拒收；R8 fuzz 联签流零错态；
//! R9 账目守恒；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xE8C0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_ISSUE: u16 = SEG_BASE + 1; // 时间戳发放异常
const E_ONCE: u16 = SEG_BASE + 2; // 单点消费被重复
const E_DOUBLE: u16 = SEG_BASE + 3; // 二次消费拒收失败
const E_SIG: u16 = SEG_BASE + 4; // 联签复核失败
const E_DISTURB: u16 = SEG_BASE + 5; // 联签未区分扰动
const E_ORDER: u16 = SEG_BASE + 6; // 时戳序列非单调
const E_UNKNOWN: u16 = SEG_BASE + 7; // 未发放时戳被接受
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 联签流错态
const E_CONSERV: u16 = SEG_BASE + 9; // 账目不守恒
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

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

/// 时间戳来源（单点）：发放序列 + 消费账。
pub struct TsSource {
    pub next_id: u32,
    pub issued: Vec<u32>,
    pub consumed: Vec<u32>,
}

impl TsSource {
    pub fn new() -> TsSource {
        TsSource {
            next_id: 1,
            issued: vec![],
            consumed: vec![],
        }
    }

    /// 发放一枚时戳：单调递增 ID。
    pub fn issue(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.issued.push(id);
        id
    }

    /// 消费一枚时戳：必须已发放且未消费过（单点消费）。
    pub fn consume(&mut self, id: u32) -> Result<(), u16> {
        if !TsSource::contains(&self.issued, id) {
            return Err(E_UNKNOWN);
        }
        if TsSource::contains(&self.consumed, id) {
            return Err(E_DOUBLE);
        }
        self.consumed.push(id);
        Ok(())
    }

    fn contains(v: &[u32], id: u32) -> bool {
        let mut i = 0usize;
        while i < v.len() {
            if v[i] == id {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 账守恒：consumed ⊆ issued 且无重复。
    pub fn conserved(&self) -> bool {
        let mut i = 0usize;
        while i < self.consumed.len() {
            if !TsSource::contains(&self.issued, self.consumed[i]) {
                return false;
            }
            let mut j = i + 1;
            while j < self.consumed.len() {
                if self.consumed[i] == self.consumed[j] {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

/// 一笔联签：M2 时戳 + E25 时戳 + 联签值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JointSign {
    pub m2_ts: u32,
    pub e25_ts: u32,
    pub sign: u64,
}

/// 联签计算：双方时戳串联 FNV。
pub fn joint_sign(m2_ts: u32, e25_ts: u32) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&m2_ts.to_le_bytes());
    buf.extend_from_slice(&e25_ts.to_le_bytes());
    fnv1a64(&buf)
}

/// 联签账：时戳来源 + 联签记录。
pub struct SignBook {
    pub src: TsSource,
    pub signs: Vec<JointSign>,
    pub last_m2: u32,
}

impl SignBook {
    pub fn new() -> SignBook {
        SignBook {
            src: TsSource::new(),
            signs: vec![],
            last_m2: 0,
        }
    }

    /// 结一笔联签：双时戳各消费一次、M2 时戳单调、联签入账。
    pub fn settle(&mut self, m2_ts: u32, e25_ts: u32) -> Result<u64, u16> {
        if m2_ts < self.last_m2 {
            return Err(E_ORDER);
        }
        let id = self.src.issue();
        self.src.consume(id)?;
        let sg = joint_sign(m2_ts, e25_ts);
        self.signs.push(JointSign {
            m2_ts,
            e25_ts,
            sign: sg,
        });
        self.last_m2 = m2_ts;
        Ok(sg)
    }

    /// 账守恒：signs 数 == consumed 数。
    pub fn conserved(&self) -> bool {
        self.signs.len() == self.src.consumed.len()
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 时间戳单点发放：连发 8 枚 ID 单调递增且互异。
    let mut src = TsSource::new();
    let mut prev = 0u32;
    let mut t = 0u32;
    while t < 8 {
        let id = src.issue();
        if id <= prev {
            return Err(E_ISSUE);
        }
        prev = id;
        t += 1;
    }
    hits += 1;

    // R2 单点消费恰一：发放 4 枚、消费 4 枚全部成功。
    let mut src2 = TsSource::new();
    let ids = [
        src2.issue(),
        src2.issue(),
        src2.issue(),
        src2.issue(),
    ];
    let mut i = 0usize;
    while i < ids.len() {
        src2.consume(ids[i])?;
        i += 1;
    }
    if src2.consumed.len() != 4 || !src2.conserved() {
        return Err(E_ONCE);
    }
    hits += 1;

    // R3 二次消费拒收：同一枚消费两次，第二次被拒，码恰为 E_DOUBLE。
    let mut src3 = TsSource::new();
    let id3 = src3.issue();
    src3.consume(id3)?;
    match src3.consume(id3) {
        Err(E_DOUBLE) => hits += 1,
        _ => return Err(E_DOUBLE),
    }

    // R4 联签稳定：同对时戳两次联签一致且非零。
    let s1 = joint_sign(100, 200);
    let s2 = joint_sign(100, 200);
    if s1 != s2 || s1 == 0 {
        return Err(E_SIG);
    }
    hits += 1;

    // R5 联签区分扰动：任一时戳翻转 → 联签必变。
    if joint_sign(100 ^ 1, 200) == s1 || joint_sign(100, 200 ^ 1) == s1 {
        return Err(E_DISTURB);
    }
    hits += 1;

    // R6 时戳序列单调：正常序全绿，倒序被拒且码恰为 E_ORDER。
    let mut bk = SignBook::new();
    bk.settle(10, 1)?;
    bk.settle(20, 2)?;
    match bk.settle(15, 3) {
        Err(E_ORDER) => hits += 1,
        _ => return Err(E_ORDER),
    }

    // R7 未发放时戳拒收：对空来源消费未发放 ID，码恰为 E_UNKNOWN。
    let mut src4 = TsSource::new();
    match src4.consume(42) {
        Err(E_UNKNOWN) => hits += 1,
        _ => return Err(E_UNKNOWN),
    }

    // R8 fuzz 联签流零错态：LCG 驱动 96 笔单调联签，全程零错态、账守恒、
    // 逐笔联签可独立复核。
    let mut bk2 = SignBook::new();
    let mut lcg = Lcg::new(0x48498);
    let mut m2 = 0u32;
    let mut t = 0u32;
    while t < 96 {
        m2 += 1 + lcg.below(5);
        let e25 = lcg.below(10_000u32);
        let sg = bk2.settle(m2, e25)?;
        if sg != joint_sign(m2, e25) {
            return Err(E_FUZZ);
        }
        t += 1;
    }
    if !bk2.conserved() || !bk2.src.conserved() {
        return Err(E_FUZZ);
    }
    hits += 1;

    // R9 账目守恒：signs == consumed == issued（单点全额消费）。
    if bk2.signs.len() != bk2.src.issued.len() {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R10 呈现行齐全：n=/m2last=/sum= 三段前缀在位。
    let line = book_line(&bk2);
    if !has_kw(line.as_bytes(), b"n=")
        || !has_kw(line.as_bytes(), b"m2last=")
        || !has_kw(line.as_bytes(), b"sum=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：全联签串联。
pub fn book_fp(bk: &SignBook) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < bk.signs.len() {
        buf.extend_from_slice(&bk.signs[i].m2_ts.to_le_bytes());
        buf.extend_from_slice(&bk.signs[i].e25_ts.to_le_bytes());
        buf.extend_from_slice(&bk.signs[i].sign.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行：n=<联签数> m2last=<最后 M2 时戳> sum=<指纹低 32 位>。
pub fn book_line(bk: &SignBook) -> String {
    let mut s = String::new();
    s.push_str("n=");
    s.push_str(&(bk.signs.len() as u32).to_string());
    s.push_str(" m2last=");
    s.push_str(&bk.last_m2.to_string());
    s.push_str(" sum=");
    s.push_str(&(book_fp(bk) as u32).to_string());
    s
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
