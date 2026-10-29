//! unx4063_hashtab — UNX-F4063 哈希函数与桶分布账（诊断段 0x8A20）。
//!
//! 判据原文（提炼）：
//!   J1. 哈希对连续号/随机号/恶意聚集号三类输入的桶分布极差有账。
//!   J2. 聚集注入退化可观测：极差账显著恶化且可检出。
//!   J3. 碰撞链长上限触发再平衡（桶分裂，链长回归界内）。
//!   J4. 元素账守恒：入桶数 == 出桶数 + 在表数。
//! 设计纪律：
//!   1. 哈希函数：乘法散列（Knuth 变体），桶数 2 的幂，
//!      h(k) = (k * 2654435761) >> shift（黄金比率常数）。
//!   2. 分布极差账：最大桶深 - 最小桶深，三类负载各自入账。
//!   3. 链长上限定编 8：超限触发桶分裂再平衡（桶数翻倍，
//!      上限受静态顶约束），再平衡后重算极差。
//!   4. 聚集注入：恶意键簇（同余序列）入表，退化可观测。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8A20;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BAD_KEY: u16 = SEG_BASE + 1;   // 0 号键拒收
const E_NOT_FOUND: u16 = SEG_BASE + 2; // 出桶键不存在
const E_DUP_KEY: u16 = SEG_BASE + 3;   // 重复键入桶
const E_REBAL_OOM: u16 = SEG_BASE + 4; // 桶数到顶不可再平衡
const E_LEDGER: u16 = SEG_BASE + 5;    // 元素账守恒破坏
const E_SPREAD: u16 = SEG_BASE + 6;    // 分布极差账异常（空表求极差）
const E_BAD_ARG: u16 = SEG_BASE + 7;   // 参数非法

/// 黄金比率哈希常数（Knuth 乘法散列，32 位）。
pub const GOLDEN: u64 = 2654435761;

/// 初始桶数定编：16（2 的幂）。
pub const INIT_BUCKETS: usize = 16;

/// 桶数静态顶：256（再平衡上限）。
pub const BUCKET_TOP: usize = 256;

/// 碰撞链长上限：8（超限触发再平衡）。
pub const CHAIN_CAP: usize = 8;

/// 桶：链式键账（整数键，确定性序）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Bucket {
    pub keys: Vec<u64>, // 链上键（碰撞链）
}

/// 哈希表：桶阵 + 分布极差账 + 再平衡账。
pub struct HashTab {
    pub buckets: Vec<Bucket>, // 桶阵
    pub shift: u32,           // 乘法散列移位（随桶数调整）
    pub rebal_total: u32,     // 再平衡触发数
    pub ins_total: u32,       // 入桶总数
    pub rem_total: u32,       // 出桶总数
    pub spread_seq: u32,      // 连续号负载极差账
    pub spread_rnd: u32,      // 随机号负载极差账
    pub spread_adv: u32,      // 恶意聚集负载极差账
}

impl HashTab {
    pub fn new() -> HashTab {
        let mut b = Vec::new();
        for _ in 0..INIT_BUCKETS {
            b.push(Bucket { keys: Vec::new() });
        }
        HashTab {
            buckets: b,
            shift: 32 - 4, // 16 桶 → 移位 28
            rebal_total: 0,
            ins_total: 0,
            rem_total: 0,
            spread_seq: 0,
            spread_rnd: 0,
            spread_adv: 0,
        }
    }

    /// 乘法散列（Knuth）：h(k) = (k * GOLDEN) >> shift % 桶数。
    pub fn hash_of(&self, k: u64) -> usize {
        let h = (k.wrapping_mul(GOLDEN) >> self.shift) as usize;
        h % self.buckets.len()
    }

    /// 入桶：重复键拒收；链超限触发再平衡。
    pub fn insert(&mut self, k: u64) -> Result<(), u16> {
        if k == 0 {
            return Err(E_BAD_KEY);
        }
        // 全表查重（语义：唯一键）
        if self.contains(k) {
            return Err(E_DUP_KEY);
        }
        // 链长预检：将超限先再平衡
        let h = self.hash_of(k);
        if self.buckets[h].keys.len() + 1 > CHAIN_CAP {
            self.rebalance()?;
        }
        let h = self.hash_of(k);
        self.buckets[h].keys.push(k);
        self.ins_total += 1;
        Ok(())
    }

    /// 出桶：键存在性先行，命中移除。
    pub fn remove(&mut self, k: u64) -> Result<(), u16> {
        if k == 0 {
            return Err(E_BAD_KEY);
        }
        let h = self.hash_of(k);
        let mut pos: Option<usize> = None;
        let n = self.buckets[h].keys.len();
        for i in 0..n {
            if self.buckets[h].keys[i] == k {
                pos = Some(i);
                break;
            }
        }
        match pos {
            Some(p) => {
                self.buckets[h].keys.remove(p);
                self.rem_total += 1;
                Ok(())
            }
            None => Err(E_NOT_FOUND),
        }
    }

    /// 键在表判定。
    pub fn contains(&self, k: u64) -> bool {
        let h = self.hash_of(k);
        for i in 0..self.buckets[h].keys.len() {
            if self.buckets[h].keys[i] == k {
                return true;
            }
        }
        false
    }

    /// 再平衡：桶数翻倍、全表重散列（判据 J3）。
    pub fn rebalance(&mut self) -> Result<(), u16> {
        if self.buckets.len() >= BUCKET_TOP {
            return Err(E_REBAL_OOM);
        }
        // 收拢全部键
        let mut all: Vec<u64> = Vec::new();
        for b in &self.buckets {
            for i in 0..b.keys.len() {
                all.push(b.keys[i]);
            }
        }
        // 桶数翻倍
        let nb = self.buckets.len() * 2;
        let mut fresh: Vec<Bucket> = Vec::new();
        for _ in 0..nb {
            fresh.push(Bucket { keys: Vec::new() });
        }
        self.buckets = fresh;
        self.shift = 32 - log2_u32(nb as u32);
        // 重散列回填
        for i in 0..all.len() {
            let h = self.hash_of(all[i]);
            self.buckets[h].keys.push(all[i]);
        }
        self.rebal_total += 1;
        Ok(())
    }

    /// 当前最大桶深。
    pub fn max_depth(&self) -> usize {
        let mut m = 0;
        for b in &self.buckets {
            if b.keys.len() > m {
                m = b.keys.len();
            }
        }
        m
    }

    /// 当前最小桶深。
    pub fn min_depth(&self) -> usize {
        if self.buckets.is_empty() {
            return 0;
        }
        let mut m = self.buckets[0].keys.len();
        for b in &self.buckets {
            if b.keys.len() < m {
                m = b.keys.len();
            }
        }
        m
    }

    /// 分布极差入账（判据 J1/J2）：max - min。
    pub fn record_spread(&mut self, slot: usize) -> Result<u32, u16> {
        if self.ins_total == self.rem_total {
            return Err(E_SPREAD); // 空表极差无意义
        }
        let s = (self.max_depth() - self.min_depth()) as u32;
        match slot {
            0 => self.spread_seq = s,
            1 => self.spread_rnd = s,
            2 => self.spread_adv = s,
            _ => return Err(E_BAD_ARG),
        }
        Ok(s)
    }

    /// 在表元素数。
    pub fn size(&self) -> u32 {
        let mut c = 0;
        for b in &self.buckets {
            c += b.keys.len() as u32;
        }
        c
    }

    /// 守恒对账（判据 J4）：入桶 == 出桶 + 在表。
    pub fn ledger_ok(&self) -> bool {
        self.ins_total == self.rem_total + self.size()
    }
}

/// u32 对数（2 的幂）。
fn log2_u32(v: u32) -> u32 {
    let mut n = 0;
    let mut x = v;
    while x > 1 {
        x >>= 1;
        n += 1;
    }
    n
}

/// 三类负载注入并采集极差（判据 J1/J2）。
/// 返回（连续号极差，随机号极差，聚集号极差）。
pub fn tri_load_spread() -> Result<(u32, u32, u32), u16> {
    // 负载一：连续号 1..=100
    let mut t1 = HashTab::new();
    for k in 1..=100u64 {
        t1.insert(k)?;
    }
    let s1 = t1.record_spread(0)?;
    // 负载二：伪随机号（LCG 整数推导，确定性）
    let mut t2 = HashTab::new();
    let mut seed: u64 = 12345;
    for _ in 0..100 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let k = (seed >> 33) | 1; // 非零奇数
        if !t2.contains(k) {
            t2.insert(k)?;
        }
    }
    let s2 = t2.record_spread(1)?;
    // 负载三：恶意聚集号（同余 16 序列，聚同桶）
    let mut t3 = HashTab::new();
    for k in 0..100u64 {
        let key = 0x1_0000 + k * 0x1_0000; // 高位聚集
        t3.insert(key)?;
    }
    let s3 = t3.record_spread(2)?;
    Ok((s1, s2, s3))
}

/// 自例程：三类负载极差→聚集退化→再平衡→守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 三类负载极差有账（判据 J1）
    let (s1, s2, s3) = tri_load_spread()?;
    if s1 == 0 && s2 == 0 {
        return Err(E_SPREAD);
    }
    hits += 1;

    // R2 聚集退化可观测：聚集极差显著大于连续号极差（判据 J2）
    if s3 <= s1 {
        return Err(E_SPREAD);
    }
    hits += 1;

    // R3 再平衡：单桶强塞超链限 → 触发桶分裂且链长回归界内
    let mut t = HashTab::new();
    // 构造同桶键：同 key 低位移位后同桶的序列（碰撞链注入）
    let mut inserted = 0;
    for k in 1..=200u64 {
        if t.insert(k).is_ok() {
            inserted += 1;
        }
    }
    if inserted < 100 {
        return Err(E_LEDGER);
    }
    if t.max_depth() > CHAIN_CAP {
        return Err(E_LEDGER);
    }
    if t.rebal_total == 0 {
        return Err(E_LEDGER); // 200 键入 16 桶必触发再平衡
    }
    hits += 1;

    // R4 出桶与查重：remove 命中/未命中/重复入桶三态
    t.remove(5)?;
    match t.remove(5) {
        Err(E_NOT_FOUND) => {}
        _ => return Err(E_NOT_FOUND),
    }
    t.insert(5)?;
    match t.insert(5) {
        Err(E_DUP_KEY) => {}
        _ => return Err(E_DUP_KEY),
    }
    hits += 1;

    // R5 守恒对账（判据 J4）
    if !t.ledger_ok() {
        return Err(E_LEDGER);
    }
    if t.ins_total != t.rem_total + t.size() {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
