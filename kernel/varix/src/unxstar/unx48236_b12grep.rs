//! unx48236_b12grep — UNX-F48236 B12 批防重 grep（关键词族：cnentoggle/punctpair/fullwidth）（诊断段 0xBE60）。
//!
//! 判据原文：
//!   J1 批收口防重留痕；判据锚定 §6.1。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 防重语义：批收口前对关键词族（cnentoggle/punctpair/fullwidth）
//!      全库扫重——同族词在任何他人模块出现即「撞号」，必须留痕并清零
//!      （本批专属词只许本批模块占用）。
//!   2. 词族账：关键词 → (本批占用清单, 撞号清单)。撞号清零才算收口；
//!      撞号必须给出位置（账面以「登记册名」粒度记录）。
//!   3. 自占豁免：本批模块名（unx482xx 前缀）内的命中是自占，不算撞号。
//!   4. 留痕可复跑：扫描账封账指纹；重扫指纹不变（确定性扫描序）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;
use crate::alloc::string::String;

pub const SEG_BASE: u16 = 0xBE60;

const E_HIT_FOREIGN: u16 = SEG_BASE + 1; // 他人模块撞号
const E_WORD_UNKNOWN: u16 = SEG_BASE + 2; // 词族外关键词
const E_DUP_WORD: u16 = SEG_BASE + 3;     // 重复登记词
const E_SEAL_MISMATCH: u16 = SEG_BASE + 4; // 封账指纹不符
const E_GUARD_UNBAL: u16 = SEG_BASE + 5;  // 扫描账不平

/// 词族闭集（B12 批专属）。
pub const KW_CNENTOGGLE: &str = "cnentoggle";
pub const KW_PUNCTPAIR: &str = "punctpair";
pub const KW_FULLWIDTH: &str = "fullwidth";

/// 一条命中记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub word: &'static str,
    pub module: String, // 登记册名（模块名）
    pub own: bool,      // 是否本批自占
}

/// 防重扫描总账。
#[derive(Debug)]
pub struct B12Grep {
    words: Vec<&'static str>,
    hits: Vec<Hit>,
    foreign: Vec<Hit>, // 撞号清单
    sealed: Option<u32>,
    own_prefix: &'static str,
}

impl B12Grep {
    pub fn new(own_prefix: &'static str) -> Self {
        B12Grep {
            words: Vec::new(),
            hits: Vec::new(),
            foreign: Vec::new(),
            sealed: None,
            own_prefix,
        }
    }

    /// 登记词族成员。
    pub fn add_word(&mut self, w: &'static str) -> Result<(), u16> {
        if self.words.contains(&w) {
            return Err(E_DUP_WORD);
        }
        self.words.push(w);
        Ok(())
    }

    /// 模拟一次扫描回执：登记 (词, 模块名) 命中；自占判定按前缀。
    pub fn scan_hit(&mut self, word: &'static str, module: &str) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(E_SEAL_MISMATCH);
        }
        if !self.words.contains(&word) {
            return Err(E_WORD_UNKNOWN);
        }
        let own = module.starts_with(self.own_prefix);
        let h = Hit { word, module: str_to_string(module), own };
        if own {
            self.hits.push(h);
        } else {
            self.foreign.push(h.clone());
            return Err(E_HIT_FOREIGN);
        }
        Ok(())
    }

    /// 收口审计：撞号清零 + 词族全覆盖（每词至少一自占命中）。
    pub fn close_audit(&self) -> Result<(), u16> {
        if !self.foreign.is_empty() {
            return Err(E_HIT_FOREIGN);
        }
        for w in &self.words {
            if !self.hits.iter().any(|h| h.word == *w) {
                return Err(E_GUARD_UNBAL); // 词族有词零自占 = 漏扫
            }
        }
        Ok(())
    }

    /// 撞号清单导出（清零前留痕用）。
    pub fn foreign_list(&self) -> Vec<(&'static str, String)> {
        self.foreign.iter().map(|h| (h.word, h.module.clone())).collect()
    }

    /// 撞号清零（处置后调用，清空前清单必须已导出留痕——账面以导出计数守恒）。
    pub fn clear_foreign(&mut self, exported: usize) -> Result<(), u16> {
        if exported != self.foreign.len() {
            return Err(E_GUARD_UNBAL); // 未留痕先清 = 违规
        }
        self.foreign.clear();
        Ok(())
    }

    /// 封账：词族+命中账指纹。
    pub fn seal(&mut self) -> Result<u32, u16> {
        let mut h: u32 = 0x811C_9DC5;
        for w in &self.words {
            for b in w.bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        for hit in &self.hits {
            for b in hit.module.bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            h ^= hit.own as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.foreign.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        self.sealed = Some(h);
        Ok(h)
    }

    pub fn verify(&self, fp: u32) -> Result<(), u16> {
        match self.sealed {
            Some(s) if s == fp => Ok(()),
            _ => Err(E_SEAL_MISMATCH),
        }
    }

    pub fn hits(&self) -> usize {
        self.hits.len()
    }
    pub fn foreign_len(&self) -> usize {
        self.foreign.len()
    }
}

fn str_to_string(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        out.push(b as char);
    }
    out
}

/// 自例程判据：落实 J1「批收口防重留痕」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 场景：标准收口 —— 三词族登记 + 本批自占命中各一 → 收口审计过。
    let mut g = B12Grep::new("unx482");
    g.add_word(KW_CNENTOGGLE)?;
    g.add_word(KW_PUNCTPAIR)?;
    g.add_word(KW_FULLWIDTH)?;
    g.scan_hit(KW_CNENTOGGLE, "unx48221_cnstatem")?;
    g.scan_hit(KW_PUNCTPAIR, "unx48225_punctpair")?;
    g.scan_hit(KW_FULLWIDTH, "unx48226_fwhwtab")?;
    if g.hits() != 3 || g.foreign_len() != 0 {
        return Err(E_GUARD_UNBAL);
    }
    g.close_audit()?;
    hits += 1;

    // R2 场景：撞号显性 —— 他人模块名命中 → 拒收留痕 + 收口红。
    let mut g2 = B12Grep::new("unx482");
    g2.add_word(KW_CNENTOGGLE)?;
    g2.scan_hit(KW_CNENTOGGLE, "unx48221_cnstatem")?; // 自占
    match g2.scan_hit(KW_CNENTOGGLE, "ves394_ecoi18n") {
        Err(E_HIT_FOREIGN) => {}
        _ => return Err(E_HIT_FOREIGN),
    }
    if g2.foreign_len() != 1 {
        return Err(E_GUARD_UNBAL);
    }
    match g2.close_audit() {
        Err(E_HIT_FOREIGN) => {}
        _ => return Err(E_HIT_FOREIGN),
    }
    hits += 1;

    // R3 场景：留痕后清零 —— 导出清单再清零 → 收口复绿。
    let fl = g2.foreign_list();
    if fl.len() != 1 || fl[0].1 != "ves394_ecoi18n" {
        return Err(E_GUARD_UNBAL);
    }
    g2.clear_foreign(fl.len())?;
    g2.close_audit()?;
    hits += 1;

    // R4 场景：未留痕先清零拒收。
    let mut g4 = B12Grep::new("unx482");
    g4.add_word(KW_FULLWIDTH)?;
    g4.scan_hit(KW_FULLWIDTH, "unx48226_fwhwtab")?;
    let _ = g4.scan_hit(KW_FULLWIDTH, "other_mod");
    match g4.clear_foreign(0) {
        Err(E_GUARD_UNBAL) => {}
        _ => return Err(E_GUARD_UNBAL),
    }
    hits += 1;

    // R5 场景：词族外关键词拒收。
    let mut g5 = B12Grep::new("unx482");
    g5.add_word(KW_CNENTOGGLE)?;
    match g5.scan_hit("imecfg", "unx48201_tiprot") {
        Err(E_WORD_UNKNOWN) => {}
        _ => return Err(E_WORD_UNKNOWN),
    }
    hits += 1;

    // R6 场景：重复登记词拒收。
    let mut g6 = B12Grep::new("unx482");
    g6.add_word(KW_PUNCTPAIR)?;
    match g6.add_word(KW_PUNCTPAIR) {
        Err(E_DUP_WORD) => {}
        _ => return Err(E_DUP_WORD),
    }
    hits += 1;

    // R7 场景：漏扫显性 —— 三词只扫两词 → 收口红（词族覆盖不足）。
    let mut g7 = B12Grep::new("unx482");
    g7.add_word(KW_CNENTOGGLE)?;
    g7.add_word(KW_PUNCTPAIR)?;
    g7.add_word(KW_FULLWIDTH)?;
    g7.scan_hit(KW_CNENTOGGLE, "unx48221_cnstatem")?;
    g7.scan_hit(KW_PUNCTPAIR, "unx48225_punctpair")?;
    match g7.close_audit() {
        Err(E_GUARD_UNBAL) => {}
        _ => return Err(E_GUARD_UNBAL),
    }
    hits += 1;

    // R8 场景：封账与复核 —— 指纹敏感（加一条命中指纹必变）。
    let fp1 = g.seal()?;
    g.verify(fp1)?;
    g.scan_hit(KW_CNENTOGGLE, "unx48222_shifttemp")?;
    let fp2 = g.seal()?;
    if fp1 == fp2 {
        return Err(E_SEAL_MISMATCH);
    }
    match g.verify(fp1) {
        Err(E_SEAL_MISMATCH) => {}
        _ => return Err(E_SEAL_MISMATCH),
    }
    hits += 1;

    // R9 场景：封账后扫描拒收（冻结语义）。
    let mut g9 = B12Grep::new("unx482");
    g9.add_word(KW_CNENTOGGLE)?;
    let _ = g9.seal();
    match g9.scan_hit(KW_CNENTOGGLE, "unx48221_cnstatem") {
        Err(E_SEAL_MISMATCH) => {}
        _ => return Err(E_SEAL_MISMATCH),
    }
    hits += 1;

    // R10 场景：前缀豁免边界 —— 前缀精确匹配（unx482 不认 unx48 前缀的他人名）。
    let mut g10 = B12Grep::new("unx482xx");
    g10.add_word(KW_CNENTOGGLE)?;
    match g10.scan_hit(KW_CNENTOGGLE, "unx48221_cnstatem") {
        Err(E_HIT_FOREIGN) => {} // 前缀 unx482xx ≠ unx48221 → 他人
        _ => return Err(E_HIT_FOREIGN),
    }
    hits += 1;

    Ok(hits)
}
