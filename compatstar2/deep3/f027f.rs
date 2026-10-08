//! F027 深化批次四 · IME 词典与学习面（compatstar2/deep3 · G-A-27）。
//!
//! 批次一~三覆盖 IMM32 桥接/GCS 语义/候选窗定位等程序可见面；本批补齐
//! 主册【功能定义】「全语义对齐」的序列化/账本/容错面：词条定长哈希查找
//! 表（64 槽，FNV-1a 定槽 + 线性探测，命中/未命中/表满三态显性）、候选
//! 评分（频度 × 新近度加权定序，稳定排序）、用户词学习账（定长 32 LRU：
//! 命中提频、淘汰最旧并记账）、自动上屏阈值逻辑（分差 ≥ 阈值才上屏）。
//! 判据对账：主册 G-A-27【设计细节】数字键选词/空格首选提交语义 + MS
//! IMM32 GCS_* 文档语义对拍（词典/学习为 VARIX IME 引擎内账面，域内
//! 口径定容）。零堆纪律：定长槽表 + 定长学习账，无 Vec/String/Box/
//! format!，错误一律 Err 或计数账面，零静默。

use crate::checks::CheckSet;

/// 词典表容量（64 槽 = 2^6，槽位取哈希低 6 位——域内口径）。
pub const DICT_SLOTS: usize = 64;
/// 用户词学习账容量（LRU 定长 32）。
pub const LRU_CAPACITY: usize = 32;
/// 自动上屏分差阈值（首选 - 次选 ≥ 30 才上屏——域内口径）。
pub const AUTO_COMMIT_DIFF: u32 = 30;
/// FNV-1a 32 位偏移基（hash 域内口径，标准常数）。
pub const FNV_OFFSET_BASIS: u32 = 0x811C_9DC5;
/// FNV-1a 32 位素数。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// FNV-1a 32 位（逐字节异或后乘素数——确定性、无堆）。
pub fn fnv1a(key: &str) -> u32 {
    let mut h = FNV_OFFSET_BASIS;
    for b in key.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 槽位：表长 64 = 2^6 → 低 6 位即槽。
pub fn slot_of(key: &str) -> usize {
    (fnv1a(key) as usize) & (DICT_SLOTS - 1)
}

/// 一个词条（键 + 频度 + 新近度）。
#[derive(Clone, Copy)]
pub struct WordEntry {
    pub key: &'static str,
    pub freq: u32,
    pub recency: u32,
}

/// 64 槽词典：FNV-1a 定槽 + 线性探测；命中/未命中/表满三态显性。
pub struct DictTable {
    pub slots: [Option<WordEntry>; DICT_SLOTS],
    /// 查找次数（账面）。
    pub lookups: u32,
    /// 累计探测步数（账面——可观测不静默）。
    pub probe_steps: u32,
}

impl DictTable {
    pub const fn new() -> Self {
        DictTable { slots: [None; DICT_SLOTS], lookups: 0, probe_steps: 0 }
    }

    /// 线性探测定位：返回 (槽位, 步数)。idx == DICT_SLOTS 表示表满未命中。
    fn locate(&self, key: &str) -> (usize, usize) {
        let home = slot_of(key);
        for step in 0..DICT_SLOTS {
            let idx = (home + step) % DICT_SLOTS;
            match self.slots[idx] {
                None => return (idx, step),
                Some(e) if e.key == key => return (idx, step),
                _ => {}
            }
        }
        (DICT_SLOTS, DICT_SLOTS)
    }

    /// 词条插入：重复键显性拒绝（dup-key）；表满显性拒绝（table-full）。
    pub fn insert(&mut self, key: &'static str, freq: u32) -> Result<usize, &'static str> {
        let (idx, _step) = self.locate(key);
        if idx == DICT_SLOTS { return Err("table-full"); }
        if self.slots[idx].is_some() { return Err("dup-key"); }
        self.slots[idx] = Some(WordEntry { key, freq, recency: 1 });
        Ok(idx)
    }

    /// 查找：命中返回槽号；未命中 None（探测步数入账）。
    pub fn lookup(&mut self, key: &str) -> Option<usize> {
        self.lookups += 1;
        let (idx, step) = self.locate(key);
        self.probe_steps += step as u32 + 1;
        if idx < DICT_SLOTS && self.slots[idx].map_or(false, |e| e.key == key) {
            Some(idx)
        } else {
            None
        }
    }

    /// 只读探测距离（命中该键所需线性探测步数——碰撞语义可测）。
    pub fn probe_len(&self, key: &str) -> usize {
        self.locate(key).1
    }
}

// 候选评分与自动上屏 --------------------------------------------------------

/// 一个候选（文本 + 频度 + 新近度）。
#[derive(Clone, Copy)]
pub struct Candidate {
    pub text: &'static str,
    pub freq: u32,
    pub recency: u32,
}

/// 评分 = 频度 ×（新近度 + 1）——加权定序的单一出处。
pub fn candidate_score(c: &Candidate) -> u64 {
    c.freq as u64 * (c.recency as u64 + 1)
}

/// 稳定排序选前二：返回 (首选下标, 次选下标)；严格大于比较 → 同分保持
/// 输入序（稳定语义）。候选不足两条返回 None。
pub fn rank_candidates(cands: &[Candidate]) -> Option<(usize, usize)> {
    if cands.len() < 2 {
        return None;
    }
    let (mut top1, mut top2) = if candidate_score(&cands[1]) > candidate_score(&cands[0]) {
        (1usize, 0usize)
    } else {
        (0usize, 1usize)
    };
    for i in 2..cands.len() {
        let s = candidate_score(&cands[i]);
        if s > candidate_score(&cands[top1]) {
            top2 = top1;
            top1 = i;
        } else if s > candidate_score(&cands[top2]) {
            top2 = i;
        }
    }
    Some((top1, top2))
}

/// 自动上屏阈值逻辑：首选与次选分差 ≥ 阈值才上屏；否则显性等待。
pub fn auto_commit(cands: &[Candidate]) -> Result<&'static str, &'static str> {
    match rank_candidates(cands) {
        Some((a, b)) => {
            let diff = candidate_score(&cands[a]) - candidate_score(&cands[b]);
            if diff >= AUTO_COMMIT_DIFF as u64 { Ok(cands[a].text) } else { Err("wait") }
        }
        None => Err("no-candidates"),
    }
}

// 用户词学习账（定长 LRU） ---------------------------------------------------

/// 一条用户词（词 + 频度 + 时钟戳）。
#[derive(Clone, Copy)]
pub struct LearnedWord {
    pub word: &'static str,
    pub freq: u32,
    pub stamp: u64,
}

/// 学习结果三态（显性，不静默）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LearnOutcome {
    Bumped,
    Inserted,
    EvictedOldest,
}

/// 用户词学习账：命中提频（LRU 提新）；满则淘汰最旧并记账。
pub struct LearnLedger {
    pub words: [Option<LearnedWord>; LRU_CAPACITY],
    pub clock: u64,
    pub evictions: u32,
    pub bumps: u32,
}

impl LearnLedger {
    pub const fn new() -> Self {
        LearnLedger { words: [None; LRU_CAPACITY], clock: 0, evictions: 0, bumps: 0 }
    }

    /// 学习一词：命中 → 提频 + 提新；未命中 → 空位插入；满 → 淘汰 stamp
    /// 最旧者（LRU 语义，账面显性）。
    pub fn learn(&mut self, word: &'static str) -> LearnOutcome {
        self.clock += 1;
        for slot in self.words.iter_mut() {
            if let Some(w) = slot {
                if w.word == word {
                    w.freq += 1;
                    w.stamp = self.clock;
                    self.bumps += 1;
                    return LearnOutcome::Bumped;
                }
            }
        }
        for slot in self.words.iter_mut() {
            if slot.is_none() {
                *slot = Some(LearnedWord { word, freq: 1, stamp: self.clock });
                return LearnOutcome::Inserted;
            }
        }
        let mut oldest = 0usize;
        for i in 1..LRU_CAPACITY {
            if self.words[i].unwrap().stamp < self.words[oldest].unwrap().stamp { oldest = i; }
        }
        self.words[oldest] = Some(LearnedWord { word, freq: 1, stamp: self.clock });
        self.evictions += 1;
        LearnOutcome::EvictedOldest
    }

    /// 查词频度（只读账面）。
    pub fn freq_of(&self, word: &str) -> Option<u32> {
        self.words.iter().flatten().find(|w| w.word == word).map(|w| w.freq)
    }

    /// 占用条数（账面）。
    pub fn occupied(&self) -> usize {
        self.words.iter().flatten().count()
    }
}

/// 静态词池（探测碰撞搜索用；&'static str 纪律下的确定性样本）。
pub const KEY_POOL: [&str; 40] = [
    "ni", "hao", "shi", "jie", "varix", "pin", "yin", "ceshi", "win", "bin", "tan",
    "pan", "fan", "dan", "nan", "lan", "gan", "kan", "han", "ban", "xing", "ying",
    "ming", "ding", "ling", "jing", "qing", "bing", "ping", "ning", "abc", "abd",
    "abe", "abf", "abg", "abh", "abi", "abj", "abk", "abl",
];

/// 在静态池中确定性找一对同槽键（同槽 → 线性探测语义可测）。
pub fn find_colliding_pair() -> Option<(usize, usize)> {
    for i in 0..KEY_POOL.len() {
        for j in (i + 1)..KEY_POOL.len() {
            if slot_of(KEY_POOL[i]) == slot_of(KEY_POOL[j]) {
                return Some((i, j));
            }
        }
    }
    None
}

/// LRU 满账测试词池（33 > 32 容量，触发淘汰）。
pub const LEARN_POOL: [&str; 33] = [
    "lw01", "lw02", "lw03", "lw04", "lw05", "lw06", "lw07", "lw08", "lw09", "lw10",
    "lw11", "lw12", "lw13", "lw14", "lw15", "lw16", "lw17", "lw18", "lw19", "lw20",
    "lw21", "lw22", "lw23", "lw24", "lw25", "lw26", "lw27", "lw28", "lw29", "lw30",
    "lw31", "lw32", "lw33",
];

/// 检查用候选构造件（&'static str 纪律下的紧凑样本）。
fn cand(text: &'static str, freq: u32, recency: u32) -> Candidate {
    Candidate { text, freq, recency }
}

/// 域自检（F027 深化批次四 · 词典/评分/学习/自动上屏）。
pub fn run_f027f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F027-imelearn-d4");
    // 1) FNV-1a 偏移基：空串哈希 == 标准偏移基（算法身份核对）。
    cs.add("fnv_offset_basis", fnv1a("") == FNV_OFFSET_BASIS, "");
    // 2) 词典插入/查找：命中返回槽号；未命中 None。
    let mut dt = DictTable::new();
    let _ = dt.insert("ni", 5);
    let _ = dt.insert("hao", 3);
    cs.add("dict_insert_lookup", dt.lookup("ni").is_some() && dt.lookup("hao").is_some() && dt.lookup("zzz").is_none(), "");
    // 3) 线性探测：静态池内确定性同槽对 → 后者探测步数 1 且可命中。
    let mut probe_ok = false;
    if let Some((i, j)) = find_colliding_pair() {
        let mut dp = DictTable::new();
        let _ = dp.insert(KEY_POOL[i], 1);
        let _ = dp.insert(KEY_POOL[j], 1);
        probe_ok = dp.probe_len(KEY_POOL[j]) == 1 && dp.lookup(KEY_POOL[j]).is_some();
    }
    cs.add("dict_linear_probe", probe_ok, "");
    // 4) 候选评分定序：频度 ×（新近度+1）→ A(60) > C(50) > B(36)。
    let cands = [cand("A", 10, 5), cand("B", 6, 5), cand("C", 10, 4)];
    cs.add("candidate_score_order", rank_candidates(&cands) == Some((0, 2)), "");
    // 5) 稳定排序：同分保持输入序（严格大于比较）。
    let ties = [cand("X", 5, 5), cand("Y", 5, 5)];
    cs.add("candidate_stable_tie", rank_candidates(&ties) == Some((0, 1)), "");
    // 6) 自动上屏：分差 40 ≥ 阈值 30 → 上屏首选。
    let above = [cand("yes", 10, 5), cand("no", 2, 5)];
    cs.add("auto_commit_above_threshold", auto_commit(&above) == Ok("yes"), "");
    // 7) 分差不足 → 显性等待。
    let below = [cand("p", 10, 5), cand("q", 10, 4)];
    cs.add("auto_commit_below_waits", auto_commit(&below) == Err("wait"), "");
    // 8) 学习账命中提频（Bumped，freq 2）。
    let mut ll = LearnLedger::new();
    let o1 = ll.learn("pin");
    let o2 = ll.learn("pin");
    cs.add("lru_hit_bumps_freq", o1 == LearnOutcome::Inserted && o2 == LearnOutcome::Bumped && ll.freq_of("pin") == Some(2), "");
    // 9) LRU 满淘汰最旧并记账：33 词入 32 容量 → 淘汰 1，最旧不在、最新在。
    let mut full = LearnLedger::new();
    let mut evicted = false;
    for w in LEARN_POOL.iter() {
        if full.learn(w) == LearnOutcome::EvictedOldest {
            evicted = true;
        }
    }
    cs.add("lru_evict_oldest_ledger", evicted && full.evictions == 1 && full.occupied() == LRU_CAPACITY && full.freq_of("lw01").is_none() && full.freq_of("lw33").is_some(), "");
    // 10) 探测步数入账：未命中探测 1 步（家槽即空）——账面可观测。
    let mut dz = DictTable::new();
    let _ = dz.lookup("missing");
    cs.add("probe_steps_ledger", dz.probe_steps == 1 && dz.lookups == 1, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dict_full_reports_err() {
        // 测试域可用 std：Box::leak 造 64 个 'static 键填满 → 第 65 个显性拒。
        let mut dt = DictTable::new();
        for i in 0..DICT_SLOTS {
            let k: &'static str = Box::leak(format!("w{i}").into_boxed_str());
            assert!(dt.insert(k, 1).is_ok());
        }
        let extra: &'static str = Box::leak(String::from("overflow-key").into_boxed_str());
        assert_eq!(dt.insert(extra, 1), Err("table-full"));
    }

    #[test]
    fn candidate_score_values() {
        // 评分公式单点标定：10×(5+1)=60；新近度 0 时退化为频度本身。
        assert_eq!(candidate_score(&Candidate { text: "x", freq: 10, recency: 5 }), 60);
        assert_eq!(candidate_score(&Candidate { text: "y", freq: 7, recency: 0 }), 7);
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f027f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
