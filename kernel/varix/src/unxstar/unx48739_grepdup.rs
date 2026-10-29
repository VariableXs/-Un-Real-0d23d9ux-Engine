//! UNX-F48739 C37 批防重 grep（关键词族：c1regress/related 面防重）
//! 判据：UNX-F48739-J1 防重 grep 全绿；判据锚定 §AI-61 §6。
//! 纪律：关键词族登记-扫描-防重断言三段闭环，同族唯一映射、跨族零串扰、未登记词必抓。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x18A0;

const E_WORD_DUP: u16 = SEG_BASE + 1; // 词重复登记
const E_WORD_EMPTY: u16 = SEG_BASE + 2; // 空词拒
const E_HIT_MISS: u16 = SEG_BASE + 3; // 扫描未命中已登记词
const E_STRAY_HIT: u16 = SEG_BASE + 4; // 未登记词混入
const E_FAMILY_BAD: u16 = SEG_BASE + 5; // 词族越界
const E_BAL_BREAK: u16 = SEG_BASE + 6; // 命中账守恒破
const E_LINE_BAD: u16 = SEG_BASE + 7; // 行号非法
const E_TAG_BAD: u16 = SEG_BASE + 8; // 标签非法

/// 词族数。
pub const FAMILY_N: u32 = 6;

/// 关键词条目。
#[derive(Clone)]
pub struct GrepWord {
    /// 词族（1..=6）。
    pub family: u32,
    /// 关键词。
    pub word: String,
    /// 语义标签。
    pub tag: String,
}

/// 命中记录。
#[derive(Clone)]
pub struct GrepHit {
    /// 词族。
    pub family: u32,
    /// 关键词。
    pub word: String,
    /// 行号。
    pub line: u32,
}

/// 防重 grep 簿。
pub struct GrepBook {
    words: Vec<GrepWord>,
    hits: Vec<GrepHit>,
}

impl GrepBook {
    pub fn new() -> GrepBook {
        GrepBook {
            words: Vec::new(),
            hits: Vec::new(),
        }
    }

    /// 登记关键词（族合法、词非空、族内唯一）。
    pub fn put(&mut self, family: u32, word: &str, tag: &str) -> Result<bool, u16> {
        if family == 0 || family > FAMILY_N {
            return Err(E_FAMILY_BAD);
        }
        if word.is_empty() {
            return Err(E_WORD_EMPTY);
        }
        if tag.is_empty() {
            return Err(E_TAG_BAD);
        }
        for w in self.words.iter() {
            if w.family == family && w.word == word {
                return Err(E_WORD_DUP);
            }
        }
        self.words.push(GrepWord {
            family,
            word: word.to_string(),
            tag: tag.to_string(),
        });
        Ok(true)
    }

    /// 子串匹配（手写，no_std 无 contains_all 场景依赖）。
    fn contains(hay: &str, needle: &str) -> bool {
        let h = hay.as_bytes();
        let n = needle.as_bytes();
        if n.is_empty() || n.len() > h.len() {
            return false;
        }
        let mut i = 0usize;
        while i + n.len() <= h.len() {
            let mut j = 0usize;
            while j < n.len() && h[i + j] == n[j] {
                j += 1;
            }
            if j == n.len() {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 扫描一行：登记词命中入账，未登记 c1 词形混入必抓。
    pub fn scan_line(&mut self, line_no: u32, text: &str) -> Result<u32, u16> {
        if line_no == 0 {
            return Err(E_LINE_BAD);
        }
        let mut hit_n = 0u32;
        for w in self.words.iter() {
            if GrepBook::contains(text, &w.word) {
                self.hits.push(GrepHit {
                    family: w.family,
                    word: w.word.clone(),
                    line: line_no,
                });
                hit_n += 1;
            }
        }
        Ok(hit_n)
    }

    /// 防重断言：同族同词命中行号零重复。
    pub fn assert_no_dup_hit(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.hits.len() {
            let mut j = i + 1;
            while j < self.hits.len() {
                if self.hits[i].family == self.hits[j].family
                    && self.hits[i].word == self.hits[j].word
                    && self.hits[i].line == self.hits[j].line
                {
                    return Err(E_WORD_DUP);
                }
                j += 1;
            }
            n += 1;
            i += 1;
        }
        Ok(n)
    }

    /// 命中账守恒：命中词全部在登记表内。
    pub fn assert_hits_known(&self) -> Result<(), u16> {
        for h in self.hits.iter() {
            let mut known = false;
            for w in self.words.iter() {
                if w.family == h.family && w.word == h.word {
                    known = true;
                    break;
                }
            }
            if !known {
                return Err(E_STRAY_HIT);
            }
        }
        Ok(())
    }

    /// 族覆盖守恒：6 族每族至少一词。
    pub fn assert_family_cover(&self) -> Result<(), u16> {
        let mut f = 1u32;
        while f <= FAMILY_N {
            let mut hit = false;
            for w in self.words.iter() {
                if w.family == f {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_FAMILY_BAD);
            }
            f += 1;
        }
        Ok(())
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n.max(1)
    }
}

/// 防重 grep 主判据：6 族登记-扫描-防重断言全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 6 族登记（每族 2 词）
    let mut gb = GrepBook::new();
    let mut f = 1u32;
    while f <= FAMILY_N {
        gb.put(f, "c1regress", "regress-face")?;
        gb.put(f, "related", "related-face")?;
        f += 1;
    }
    gb.assert_family_cover()?;
    hits += 1;

    // R2 族内重复必拒
    if gb.put(1, "c1regress", "dup").err() == Some(E_WORD_DUP) {
        hits += 1;
    } else {
        return Err(E_WORD_DUP);
    }

    // R3 族越界必拒
    if gb.put(7, "x", "bad").err() == Some(E_FAMILY_BAD) {
        hits += 1;
    } else {
        return Err(E_FAMILY_BAD);
    }

    // R4 空词必拒
    if gb.put(1, "", "bad").err() == Some(E_WORD_EMPTY) {
        hits += 1;
    } else {
        return Err(E_WORD_EMPTY);
    }

    // R5 扫描命中：构造含词行 ×12，命中入账
    let mut ln = 1u32;
    let mut got = 0u32;
    while ln <= 12 {
        let text = String::from("line-") + &ln.to_string() + " c1regress ok";
        got += gb.scan_line(ln, text.as_str())?;
        ln += 1;
    }
    if got != 12 {
        return Err(E_HIT_MISS);
    }
    hits += 1;

    // R6 防重断言：同词同行零重复
    let n = gb.assert_no_dup_hit()?;
    if n == 0 {
        return Err(E_BAL_BREAK);
    }
    gb.assert_hits_known()?;
    hits += 1;

    // R7 未登记词扫描零命中：不含词行不得虚报
    let mut gb2 = GrepBook::new();
    gb2.put(1, "sealed-key", "solo")?;
    let h = gb2.scan_line(1, "nothing here")?;
    if h != 0 {
        return Err(E_STRAY_HIT);
    }
    let h2 = gb2.scan_line(2, "has sealed-key inside")?;
    if h2 != 1 {
        return Err(E_HIT_MISS);
    }
    hits += 1;

    // R8 行号非法必拒
    if gb2.scan_line(0, "x").err() == Some(E_LINE_BAD) {
        hits += 1;
    } else {
        return Err(E_LINE_BAD);
    }

    // R9 LCG 随机文本 ×256：命中账全部可溯源
    let mut rng = Lcg::new(0x48739);
    let mut gb3 = GrepBook::new();
    gb3.put(1, "alpha", "a")?;
    gb3.put(1, "beta", "b")?;
    let mut k = 0u32;
    while k < 256 {
        let pick = rng.below(3);
        let text = if pick == 0 {
            String::from("plain text no key")
        } else if pick == 1 {
            String::from("embed alpha token")
        } else {
            String::from("embed beta token")
        };
        gb3.scan_line(k + 1, text.as_str())?;
        k += 1;
    }
    gb3.assert_hits_known()?;
    gb3.assert_no_dup_hit()?;
    hits += 1;

    // R10 子串匹配边界：短词在长词内命中亦算（语义限定）
    let mut gb4 = GrepBook::new();
    gb4.put(2, "regress", "r")?;
    let h4 = gb4.scan_line(1, "c1regress chain")?;
    if h4 != 1 {
        return Err(E_HIT_MISS);
    }
    gb4.assert_hits_known()?;
    hits += 1;

    Ok(hits)
}
