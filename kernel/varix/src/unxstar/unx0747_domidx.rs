//! UNX-F0747 · 域经检索索引（主题词→条目映射账） · 诊断段 0x5c60
//! 判据原文摘录：UNX-F0747-J1 索引完备断言过（每条目 ≥1 主题词可达 +
//! 主题词表封闭）且索引-登记簿一致性 100%；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。纯功能代码 ≥570 行。
//!
//! 设计纪律：
//! 1. 主题词表封闭：检索只认登记簿声明的闭集词表（16 词定容），
//!    词表外的词一律拒绝入索引——封闭性断言据此成立。
//! 2. 索引完备口径：每条目至少挂 1 个词表内主题词，且该词的反向映射
//!    能检索到该条目（正向挂载 + 反向可达双核）。
//! 3. 一致性 100% 口径：正向（词→条目集）与反向（条目→词集）互查，
//!    逐 (词, 条目) 对双向命中才算一致，任何单向幽灵映射即失败。
//! 4. 定容数组账本 + 守恒恒等式：正向映射对数 == 反向映射对数 == 落账对数。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5c60 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5c60;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_VOCAB_FULL: u16 = SEG_BASE + 1; // 词表已满（16 词闭集）
const E_VOCAB_DUP: u16 = SEG_BASE + 2; // 主题词重复入表
const E_WORD_EMPTY: u16 = SEG_BASE + 3; // 主题词为空
const E_WORD_LEN: u16 = SEG_BASE + 4; // 主题词长度越界（2..=16 字节）
const E_WORD_CLOSED: u16 = SEG_BASE + 5; // 词表封闭：未登记词拒绝入索引
const E_ENTRY_DUP: u16 = SEG_BASE + 6; // 条目号重复登记
const E_ENTRY_FULL: u16 = SEG_BASE + 7; // 登记簿已满
const E_ENTRY_NOTFOUND: u16 = SEG_BASE + 8; // 条目号不存在
const E_KW_LACK: u16 = SEG_BASE + 9; // 条目零主题词（索引不完备）
const E_KW_DUP: u16 = SEG_BASE + 10; // 条目同词重复挂载
const E_IDX_FULL: u16 = SEG_BASE + 11; // 索引对账已满
const E_IDX_ORPHAN: u16 = SEG_BASE + 12; // 幽灵映射：正向有反向无
const E_BAL: u16 = SEG_BASE + 13; // 守恒恒等式破裂（对数不守恒）
const E_SEALED: u16 = SEG_BASE + 14; // 索引已封账，拒绝再写
const E_INCOMPLETE: u16 = SEG_BASE + 15; // 索引完备断言失败

/// 词表闭集容量。
pub const VOCAB_CAP: usize = 16;

/// 登记簿容量。
pub const ENTRIES_CAP: usize = 24;

/// 索引对账容量（词×条目映射对上限）。
pub const PAIRS_CAP: usize = 96;

/// 主题词长度规范：2..=16 字节。
pub const WORD_MIN: usize = 2;
pub const WORD_MAX: usize = 16;

// ── FNV-1a 64（封账哈希口径）──────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_VOCAB_FULL => "词表已满 16 词：闭集即上限，先复用既有词",
        E_VOCAB_DUP => "主题词重复：词表内唯一",
        E_WORD_EMPTY => "主题词为空",
        E_WORD_LEN => "主题词长度越界：2..=16 字节",
        E_WORD_CLOSED => "词表封闭：新词先入词表再入索引",
        E_ENTRY_DUP => "条目号重复：登记序自动编号",
        E_ENTRY_FULL => "登记簿已满 24 条",
        E_ENTRY_NOTFOUND => "条目号不存在",
        E_KW_LACK => "条目零主题词：索引不完备，先挂词",
        E_KW_DUP => "条目同词重复挂载",
        E_IDX_FULL => "索引对账已满 96 对",
        E_IDX_ORPHAN => "幽灵映射：正向存在但反向缺失，账损坏",
        E_BAL => "守恒破裂：正向对数必须等于反向对数",
        E_SEALED => "索引已封账：封账后只读",
        E_INCOMPLETE => "索引不完备：存在条目无法由任何词检索到",
        _ => "未知诊断码",
    }
}

// ── 映射对 ───────────────────────────────────────────────────
/// 一条主题词→条目映射对（正向索引行，同时用于反向核）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairRow {
    /// 词表内词号（1 起）。
    pub word_id: u32,
    /// 条目号（1 起）。
    pub entry_id: u32,
}

// ── 登记条目 ─────────────────────────────────────────────────
/// 登记簿条目：主题 + 挂载词号集。
#[derive(Clone, Debug)]
pub struct IndexEntry {
    /// 条目号（1 起连续）。
    pub entry_id: u32,
    /// 条目主题（自由文本，检索经词表进行）。
    pub topic: String,
    /// 挂载词号集（≥1，词表内）。
    pub word_ids: Vec<u32>,
}

// ── 检索索引（主体）──────────────────────────────────────────
/// 域经检索索引：闭集词表 + 正向映射 + 反向核 + 完备/一致性断言。
pub struct DomExpIndex {
    /// 闭集词表。
    vocab: [Option<String>; VOCAB_CAP],
    vocab_count: usize,
    /// 登记簿。
    entries: [Option<IndexEntry>; ENTRIES_CAP],
    entry_count: usize,
    /// 正向映射对账（词号→条目号）。
    pairs: [Option<PairRow>; PAIRS_CAP],
    pair_count: usize,
    /// 是否封账。
    sealed: bool,
}

impl DomExpIndex {
    /// 开索引。
    pub fn new() -> DomExpIndex {
        DomExpIndex {
            vocab: core::array::from_fn(|_| None),
            vocab_count: 0,
            entries: core::array::from_fn(|_| None),
            entry_count: 0,
            pairs: [None; PAIRS_CAP],
            pair_count: 0,
            sealed: false,
        }
    }

    /// 词表登记：新词入闭集（封账后禁）。
    pub fn declare_word(&mut self, word: &str) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.vocab_count >= VOCAB_CAP {
            return Err(E_VOCAB_FULL);
        }
        if word.is_empty() {
            return Err(E_WORD_EMPTY);
        }
        let wb = word.as_bytes();
        if wb.len() < WORD_MIN || wb.len() > WORD_MAX {
            return Err(E_WORD_LEN);
        }
        let mut i = 0usize;
        while i < self.vocab_count {
            if let Some(w) = &self.vocab[i] {
                if w == word {
                    return Err(E_VOCAB_DUP);
                }
            }
            i += 1;
        }
        let word_id = (self.vocab_count + 1) as u32;
        self.vocab[self.vocab_count] = Some(word.to_string());
        self.vocab_count += 1;
        Ok(word_id)
    }

    /// 词号 → 词文本（词表封闭性消费口）。
    pub fn word_text(&self, word_id: u32) -> Result<String, u16> {
        if word_id == 0 || word_id as usize > self.vocab_count {
            return Err(E_WORD_CLOSED);
        }
        match &self.vocab[(word_id - 1) as usize] {
            Some(w) => Ok(w.clone()),
            None => Err(E_WORD_CLOSED),
        }
    }

    /// 词文本 → 词号（检索入口）。
    pub fn word_id_of(&self, word: &str) -> Result<u32, u16> {
        let mut i = 0usize;
        while i < self.vocab_count {
            if let Some(w) = &self.vocab[i] {
                if w == word {
                    return Ok((i + 1) as u32);
                }
            }
            i += 1;
        }
        Err(E_WORD_CLOSED)
    }

    /// 条目登记：主题非空 + 至少一个词表内词号。
    pub fn register_entry(&mut self, topic: &str, word_ids: &[u32]) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.entry_count >= ENTRIES_CAP {
            return Err(E_ENTRY_FULL);
        }
        if topic.is_empty() {
            return Err(E_WORD_EMPTY);
        }
        if word_ids.is_empty() {
            return Err(E_KW_LACK);
        }
        // 词表封闭性：挂载词必须全部在表。
        let mut i = 0usize;
        while i < word_ids.len() {
            self.word_text(word_ids[i])?;
            i += 1;
        }
        let entry_id = (self.entry_count + 1) as u32;
        self.entries[self.entry_count] = Some(IndexEntry {
            entry_id,
            topic: topic.to_string(),
            word_ids: Vec::from(word_ids),
        });
        self.entry_count += 1;
        // 正向映射对落账（词号→条目号）。
        i = 0;
        while i < word_ids.len() {
            self.add_pair(word_ids[i], entry_id)?;
            i += 1;
        }
        Ok(entry_id)
    }

    /// 加映射对：去重（同词同条目只一对）。
    fn add_pair(&mut self, word_id: u32, entry_id: u32) -> Result<(), u16> {
        if self.pair_count >= PAIRS_CAP {
            return Err(E_IDX_FULL);
        }
        let mut i = 0usize;
        while i < self.pair_count {
            if let Some(p) = &self.pairs[i] {
                if p.word_id == word_id && p.entry_id == entry_id {
                    return Err(E_KW_DUP);
                }
            }
            i += 1;
        }
        self.pairs[self.pair_count] = Some(PairRow { word_id, entry_id });
        self.pair_count += 1;
        Ok(())
    }

    /// 正向检索：词号 → 可达条目号集。
    pub fn entries_of_word(&self, word_id: u32) -> Result<Vec<u32>, u16> {
        self.word_text(word_id)?;
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.pair_count {
            if let Some(p) = &self.pairs[i] {
                if p.word_id == word_id {
                    out.push(p.entry_id);
                }
            }
            i += 1;
        }
        Ok(out)
    }

    /// 反向检索：条目号 → 挂载词号集。
    pub fn words_of_entry(&self, entry_id: u32) -> Result<Vec<u32>, u16> {
        if entry_id == 0 || entry_id as usize > self.entry_count {
            return Err(E_ENTRY_NOTFOUND);
        }
        match &self.entries[(entry_id - 1) as usize] {
            Some(e) => Ok(e.word_ids.clone()),
            None => Err(E_ENTRY_NOTFOUND),
        }
    }

    /// 索引完备断言：每条目 ≥1 词且该词正向检索可达该条目。
    pub fn completeness_assert(&self) -> Result<(), u16> {
        if self.entry_count == 0 {
            return Err(E_INCOMPLETE);
        }
        let mut e = 1u32;
        while e <= self.entry_count as u32 {
            let words = self.words_of_entry(e)?;
            if words.is_empty() {
                return Err(E_KW_LACK);
            }
            // 反向挂载词至少一个能在正向检索中命中本条目。
            let mut reachable = false;
            let mut i = 0usize;
            while i < words.len() {
                if let Ok(ids) = self.entries_of_word(words[i]) {
                    let mut j = 0usize;
                    while j < ids.len() {
                        if ids[j] == e {
                            reachable = true;
                        }
                        j += 1;
                    }
                }
                i += 1;
            }
            if !reachable {
                return Err(E_INCOMPLETE);
            }
            e += 1;
        }
        Ok(())
    }

    /// 一致性 100% 断言：逐对双向核（正向有 → 反向有）+ 无幽灵。
    pub fn consistency_assert(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.pair_count {
            let p = match &self.pairs[i] {
                Some(p) => *p,
                None => return Err(E_IDX_ORPHAN),
            };
            // 反向：条目必须挂载该词。
            let words = self.words_of_entry(p.entry_id)?;
            let mut back = false;
            let mut j = 0usize;
            while j < words.len() {
                if words[j] == p.word_id {
                    back = true;
                }
                j += 1;
            }
            if !back {
                return Err(E_IDX_ORPHAN);
            }
            // 正向：词必须真实在表（词表封闭）。
            self.word_text(p.word_id)?;
            i += 1;
        }
        Ok(())
    }

    /// 守恒恒等式：对账行数 == 逐条目词数之和（一对不丢不重）。
    pub fn conserved(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.entry_count {
            if let Some(e) = &self.entries[i] {
                sum += e.word_ids.len() as u32;
            }
            i += 1;
        }
        sum == self.pair_count as u32
    }

    /// 登记条数。
    pub fn entry_count(&self) -> usize {
        self.entry_count
    }

    /// 词表词数。
    pub fn vocab_count(&self) -> usize {
        self.vocab_count
    }

    /// 映射对数。
    pub fn pair_count(&self) -> usize {
        self.pair_count
    }

    /// 封账：完备 + 一致 + 守恒全过后冻结。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.completeness_assert()?;
        self.consistency_assert()?;
        if !self.conserved() {
            return Err(E_BAL);
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.vocab_count {
            if let Some(w) = &self.vocab[i] {
                buf.extend_from_slice(w.as_bytes());
                buf.push(0x1f);
            }
            i += 1;
        }
        i = 0;
        while i < self.pair_count {
            if let Some(p) = &self.pairs[i] {
                buf.extend_from_slice(p.word_id.to_le_bytes().as_slice());
                buf.extend_from_slice(p.entry_id.to_le_bytes().as_slice());
            }
            i += 1;
        }
        self.sealed = true;
        Ok(fnv1a64(&buf))
    }

    /// 索引审计行渲染。
    pub fn audit_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 域经检索索引 · 词→条目映射 ===".to_string());
        lines.push("词表: ".to_string() + &itoa(self.vocab_count as u64) + " 词");
        lines.push("条目: ".to_string() + &itoa(self.entry_count as u64) + " 条");
        lines.push("映射对: ".to_string() + &itoa(self.pair_count as u64) + " 对");
        let mut i = 0usize;
        while i < self.pair_count {
            if let Some(p) = &self.pairs[i] {
                let wtext = self.word_text(p.word_id).unwrap_or(String::from("?"));
                let mut line = String::from("[");
                line.push_str(&wtext);
                line.push_str("]->E");
                line.push_str(&itoa(p.entry_id as u64));
                lines.push(line);
            }
            i += 1;
        }
        lines
    }

    /// 词热度盘点：词号 → 命中条目数（检索面统计取数口）。
    pub fn word_hit_tally(&self) -> Vec<(u32, u32)> {
        let mut out: Vec<(u32, u32)> = Vec::new();
        let mut w = 1u32;
        while w as usize <= self.vocab_count {
            let hits = self.entries_of_word(w).unwrap_or(Vec::new());
            out.push((w, hits.len() as u32));
            w += 1;
        }
        out
    }

    /// 零命中词清单：词表内声明但尚无条目挂载的词（词表健康度取数口）。
    pub fn cold_words(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let tally = self.word_hit_tally();
        let mut i = 0usize;
        while i < tally.len() {
            if tally[i].1 == 0 {
                out.push(tally[i].0);
            }
            i += 1;
        }
        out
    }

    /// 词共现计数：同条目挂两词的对数（共现矩阵上三角计数）。
    pub fn cooccurrence_count(&self, word_a: u32, word_b: u32) -> u32 {
        if word_a == word_b {
            return 0;
        }
        // 取较小词号的条目集与较大词号的条目集求交。
        let (small, large) = if word_a < word_b {
            (word_a, word_b)
        } else {
            (word_b, word_a)
        };
        let sa = match self.entries_of_word(small) {
            Ok(v) => v,
            Err(_) => return 0,
        };
        let sb = match self.entries_of_word(large) {
            Ok(v) => v,
            Err(_) => return 0,
        };
        let mut n = 0u32;
        let mut i = 0usize;
        while i < sa.len() {
            let mut j = 0usize;
            while j < sb.len() {
                if sa[i] == sb[j] {
                    n += 1;
                }
                j += 1;
            }
            i += 1;
        }
        n
    }

    /// 条目主题只读快照（检索命中后的展示口）。
    pub fn topic_of(&self, entry_id: u32) -> Result<String, u16> {
        if entry_id == 0 || entry_id as usize > self.entry_count {
            return Err(E_ENTRY_NOTFOUND);
        }
        match &self.entries[(entry_id - 1) as usize] {
            Some(e) => Ok(e.topic.clone()),
            None => Err(E_ENTRY_NOTFOUND),
        }
    }
}

/// 极简无符号转十进制。
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：词表封闭 + 完备 + 一致性 100% + 封账 + 错误路径。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut ix = DomExpIndex::new();
    // 词表：五个主题词。
    let w1 = ix.declare_word("哈希")?;
    let w2 = ix.declare_word("账本")?;
    let w3 = ix.declare_word("时限")?;
    let w4 = ix.declare_word("断言")?;
    let w5 = ix.declare_word("封存")?;
    if ix.vocab_count() != 5 {
        return Err(SEG_BASE);
    }
    // 条目：四条，词覆盖完备（每条 ≥1 词）。
    let e1 = ix.register_entry("哈希先行校验纪律", &[w1])?;
    let e2 = ix.register_entry("定容账本守恒", &[w2, w4])?;
    let e3 = ix.register_entry("七日硬时限拦截", &[w3])?;
    let e4 = ix.register_entry("封存后写拦截", &[w5, w2])?;
    if ix.entry_count() != 4 {
        return Err(SEG_BASE);
    }
    let _ = (e1, e2, e3, e4);
    ix.completeness_assert()?;
    ix.consistency_assert()?;
    if !ix.conserved() {
        return Err(SEG_BASE);
    }
    // 检索：词"账本"应命中 E2 与 E4。
    let mut hits = ix.entries_of_word(w2)?;
    if hits.len() != 2 {
        return Err(SEG_BASE);
    }
    // 检索排序确定性：E2 < E4。
    if hits[0] != 2 || hits[1] != 4 {
        return Err(SEG_BASE);
    }
    let sealed = ix.seal()?;
    if sealed == 0 {
        return Err(SEG_BASE);
    }
    // 统计取数口：词热度与共现。
    let tally = ix.word_hit_tally();
    if tally.len() != 5 || tally[0].1 != 1 {
        return Err(SEG_BASE);
    }
    let cold = ix.cold_words();
    if !cold.is_empty() {
        return Err(SEG_BASE);
    }
    if ix.cooccurrence_count(w2, w5) != 1 {
        return Err(SEG_BASE);
    }
    let t = ix.topic_of(3)?;
    if t != "七日硬时限拦截" {
        return Err(SEG_BASE);
    }
    // 封账后登记拒绝。
    match ix.register_entry("封账后条目", &[w1]) {
        Err(c) if c == E_SEALED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：词表外词拒绝（封闭性）。
    let mut ix2 = DomExpIndex::new();
    let w = ix2.declare_word("表内词")?;
    match ix2.register_entry("野词条目", &[w + 1]) {
        Err(c) if c == E_WORD_CLOSED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：零词条目拒绝。
    match ix2.register_entry("零词条目", &[]) {
        Err(c) if c == E_KW_LACK => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：重复词入表。
    match ix2.declare_word("表内词") {
        Err(c) if c == E_VOCAB_DUP => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
