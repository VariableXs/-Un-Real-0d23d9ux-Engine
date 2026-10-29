//! unx48225_punctpair — UNX-F48225 标点配对表（中文引号/省略号/顿号）与逐项行为账（诊断段 0xBD00）。
//!
//! 判据原文：
//!   J1 配对 ×100 行为对齐微软拼音缺省锚（ADR-UNX-008，L2）；判据锚定 §AI-61 §2 专题五。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 配对表闭集（微软拼音缺省锚，L2 口径）：双引号 " → “”交替、
//!      单引号 ' → ‘’交替、书名号 < → 《》、圆括号 ( → （）、顿号
//!      \ → 、、省略号 ^ → ……（六点一次成对出）、破折号 _ → ——。
//!   2. 行为分型三类：QuotePair（开闭交替，会话内首个出开引号）、
//!      BracketPair（一次出全对，开+闭连出）、Fixed（一次出定长符号，
//!      顿号/省略号/破折号）；分型决定按键行为，行为决定账页。
//!   3. 逐项行为账：每次按键落一条 (key, 分型, 输出码点序列, 序内位次)，
//!      ×100 = 对表内 8 键轮转 100 次按键，逐条与锚语义复算核对，
//!      交替位次（QuotePair 奇出开偶出闭）逐条可复算。
//!   4. 码点全整数：输出以 Unicode 码点定长组表示（≤2 码点），查表
//!      返回固定组，禁止运行期拼串——账面与输出逐码点比对。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBD00;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_BAD_KEY: u16 = SEG_BASE + 1;    // 键不在配对表闭集
const E_PAIR_BROKEN: u16 = SEG_BASE + 2;// 输出与锚语义不符
const E_ALT_BROKEN: u16 = SEG_BASE + 3; // 引号交替位次错乱
const E_PAIR_MISS: u16 = SEG_BASE + 4;  // ×100 样本不足
const E_LEDGER_BAD: u16 = SEG_BASE + 5; // 行为账不平
const E_STATE_BAD: u16 = SEG_BASE + 6;  // 会话态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7;  // 写回摘要为空

/// 配对分型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PunctKind {
    QuotePair,  // 开闭交替
    BracketPair,// 一次全对
    Fixed,      // 一次定长
}

/// 一条配对表项：ASCII 键 → 锚输出码点组。
#[derive(Clone, Copy, Debug)]
pub struct PunctEntry {
    pub key: u8,
    pub kind: PunctKind,
    /// 输出码点组（微软拼音缺省锚，L2）。
    pub out: [u32; 2],
    /// 有效码点个数（1 或 2）。
    pub n: u8,
}

/// 微软拼音缺省锚配对表（8 键）。
pub const PUNCT_TABLE: [PunctEntry; 8] = [
    // 双引号：开 “ U+201C / 闭 ” U+201D（交替，单码点出）
    PunctEntry { key: b'"', kind: PunctKind::QuotePair, out: [0x201C, 0x201D], n: 2 },
    // 单引号：开 ‘ U+2018 / 闭 ’ U+2019（交替，单码点出）
    PunctEntry { key: b'\'', kind: PunctKind::QuotePair, out: [0x2018, 0x2019], n: 2 },
    // 书名号：《 U+300A 》 U+300B（全对）
    PunctEntry { key: b'<', kind: PunctKind::BracketPair, out: [0x300A, 0x300B], n: 2 },
    // 圆括号：（ U+FF08 ） U+FF09（全对）
    PunctEntry { key: b'(', kind: PunctKind::BracketPair, out: [0xFF08, 0xFF09], n: 2 },
    // 顿号：、 U+3001（单出）
    PunctEntry { key: b'\\', kind: PunctKind::Fixed, out: [0x3001, 0], n: 1 },
    // 省略号：…… 两个 U+2026（六点成对）
    PunctEntry { key: b'^', kind: PunctKind::Fixed, out: [0x2026, 0x2026], n: 2 },
    // 破折号：—— 两个 U+2014
    PunctEntry { key: b'_', kind: PunctKind::Fixed, out: [0x2014, 0x2014], n: 2 },
    // 中文问号全对形：？ U+FF1F（单出）
    PunctEntry { key: b'?', kind: PunctKind::Fixed, out: [0xFF1F, 0], n: 1 },
];

/// 查表：ASCII 键 → 表项。
pub fn lookup(key: u8) -> Option<PunctEntry> {
    let mut i = 0usize;
    while i < PUNCT_TABLE.len() {
        if PUNCT_TABLE[i].key == key {
            return Some(PUNCT_TABLE[i]);
        }
        i += 1;
    }
    None
}

/// 一条行为账。
#[derive(Clone, Copy, Debug)]
pub struct BehavRecord {
    pub key: u8,
    pub kind: PunctKind,
    pub out: [u32; 2], // 实际输出码点组
    pub n: u8,
    pub alt_pos: u32,  // QuotePair 的交替位次（0 起）：偶出开、奇出闭
}

/// 会话内引号交替计数器（按键独立）。
pub struct PunctPair {
    quote_alt: [u32; 8], // 每键交替计数
    ledger: Vec<BehavRecord>,
    pub press_total: u32,
    pub session_open: bool,
}

impl PunctPair {
    pub fn new() -> PunctPair {
        PunctPair {
            quote_alt: [0; 8],
            ledger: Vec::new(),
            press_total: 0,
            session_open: false,
        }
    }

    pub fn open_session(&mut self) -> Result<(), u16> {
        if self.session_open {
            return Err(E_STATE_BAD);
        }
        self.session_open = true;
        self.quote_alt = [0; 8]; // 新会话引号从开引号起
        Ok(())
    }

    pub fn close_session(&mut self) -> Result<(), u16> {
        if !self.session_open {
            return Err(E_STATE_BAD);
        }
        self.session_open = false;
        Ok(())
    }

    /// 按键主径：查表 → 分型行为 → 落账（纪律 2/3）。
    pub fn press(&mut self, key: u8) -> Result<[u32; 2], u16> {
        if !self.session_open {
            return Err(E_STATE_BAD);
        }
        let entry = match lookup(key) {
            Some(e) => e,
            None => return Err(E_BAD_KEY),
        };
        let idx = self.table_index(key);
        let mut out = [0u32; 2];
        let mut alt_pos = 0u32;
        match entry.kind {
            PunctKind::QuotePair => {
                // 交替：偶位次出开引号、奇位次出闭引号。
                alt_pos = self.quote_alt[idx];
                if alt_pos % 2 == 0 {
                    out[0] = entry.out[0];
                } else {
                    out[0] = entry.out[1];
                }
                self.quote_alt[idx] += 1;
            }
            PunctKind::BracketPair => {
                // 全对连出。
                out[0] = entry.out[0];
                out[1] = entry.out[1];
            }
            PunctKind::Fixed => {
                out[0] = entry.out[0];
                out[1] = entry.out[1];
            }
        }
        let n = if out[1] == 0 { 1u8 } else { 2u8 };
        self.ledger.push(BehavRecord { key, kind: entry.kind, out, n, alt_pos });
        self.press_total += 1;
        Ok(out)
    }

    fn table_index(&self, key: u8) -> usize {
        let mut i = 0usize;
        while i < PUNCT_TABLE.len() {
            if PUNCT_TABLE[i].key == key {
                return i;
            }
            i += 1;
        }
        0
    }

    /// ×N 覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.press_total < want {
            return Err(E_PAIR_MISS);
        }
        Ok(())
    }

    /// 行为账逐条复算（J1 主体）：与锚语义重放比对。
    pub fn audit_replay(&self) -> Result<(), u16> {
        // 重放用独立交替计数器，从账首逐条推演。
        let mut alt = [0u32; 8];
        let mut i = 0usize;
        while i < self.ledger.len() {
            let rec = &self.ledger[i];
            let entry = match lookup(rec.key) {
                Some(e) => e,
                None => return Err(E_BAD_KEY),
            };
            match rec.kind {
                PunctKind::QuotePair => {
                    let pos = alt[self.table_index(rec.key)];
                    if pos != rec.alt_pos {
                        return Err(E_ALT_BROKEN);
                    }
                    let want = if pos % 2 == 0 { entry.out[0] } else { entry.out[1] };
                    if rec.out[0] != want || rec.out[1] != 0 {
                        return Err(E_PAIR_BROKEN);
                    }
                    alt[self.table_index(rec.key)] += 1;
                }
                PunctKind::BracketPair => {
                    if rec.out[0] != entry.out[0] || rec.out[1] != entry.out[1] {
                        return Err(E_PAIR_BROKEN);
                    }
                }
                PunctKind::Fixed => {
                    if rec.out[0] != entry.out[0] || rec.out[1] != entry.out[1] {
                        return Err(E_PAIR_BROKEN);
                    }
                }
            }
            i += 1;
        }
        if self.ledger.len() as u32 != self.press_total {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 引号闭合守恒：会话内每键的开/闭引号数差 ≤1，且闭数=开数或闭数=开数−1。
    pub fn audit_quote_balance(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < 8 {
            if PUNCT_TABLE[i].kind == PunctKind::QuotePair {
                let a = self.quote_alt[i];
                if a > 0 {
                    let opens = (a + 1) / 2;
                    let closes = a / 2;
                    if closes > opens || opens - closes > 1 {
                        return Err(E_ALT_BROKEN);
                    }
                }
            }
            i += 1;
        }
        Ok(())
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（配对 ×100 行为对齐微软拼音缺省锚），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主扫 ×100：会话内 8 键轮转 100 次，逐条输出与锚一致，账重放全对。
    let mut pp = PunctPair::new();
    pp.open_session()?;
    let mut i = 0u32;
    while i < 100 {
        let key = PUNCT_TABLE[(i % 8) as usize].key;
        let out = pp.press(key)?;
        let entry = lookup(key).unwrap_or(PUNCT_TABLE[0]);
        match entry.kind {
            PunctKind::QuotePair => {
                let pos = i / 8; // 同键第 pos 次（8 键轮转）
                let want = if pos % 2 == 0 { entry.out[0] } else { entry.out[1] };
                if out[0] != want || out[1] != 0 {
                    return Err(E_PAIR_BROKEN);
                }
            }
            _ => {
                if out[0] != entry.out[0] || (entry.n == 2 && out[1] != entry.out[1]) {
                    return Err(E_PAIR_BROKEN);
                }
            }
        }
        i += 1;
    }
    pp.coverage(100)?;
    pp.audit_replay()?;
    pp.audit_quote_balance()?;
    if pp.ledger_len() != 100 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 双引号交替：连按 4 次出 “ ” “ ”（U+201C/U+201D 交替）。
    let mut pp = PunctPair::new();
    pp.open_session()?;
    let want: [u32; 4] = [0x201C, 0x201D, 0x201C, 0x201D];
    let mut i = 0usize;
    while i < 4 {
        let out = pp.press(b'"')?;
        if out[0] != want[i] {
            return Err(E_ALT_BROKEN);
        }
        i += 1;
    }
    pp.audit_quote_balance()?;
    hits += 1;

    // R3 单双引号独立交替：双、单、双、单 → “ ‘ “ ‘（计数器按键分立）。
    let mut pp = PunctPair::new();
    pp.open_session()?;
    if pp.press(b'"')?[0] != 0x201C {
        return Err(E_ALT_BROKEN);
    }
    if pp.press(b'\'')?[0] != 0x2018 {
        return Err(E_ALT_BROKEN);
    }
    if pp.press(b'"')?[0] != 0x201D {
        return Err(E_ALT_BROKEN); // 双引号自己的第 2 次：闭
    }
    if pp.press(b'\'')?[0] != 0x2019 {
        return Err(E_ALT_BROKEN); // 单引号自己的第 2 次：闭
    }
    hits += 1;

    // R4 全对与定长键：书名号出《》、圆括号出（）、顿号出、省略号出
    //    ……、破折号出 ——、问号出？，逐键逐码点核对。
    let mut pp = PunctPair::new();
    pp.open_session()?;
    let out = pp.press(b'<')?;
    if out[0] != 0x300A || out[1] != 0x300B {
        return Err(E_PAIR_BROKEN);
    }
    let out = pp.press(b'(')?;
    if out[0] != 0xFF08 || out[1] != 0xFF09 {
        return Err(E_PAIR_BROKEN);
    }
    let out = pp.press(b'\\')?;
    if out[0] != 0x3001 || out[1] != 0 {
        return Err(E_PAIR_BROKEN);
    }
    let out = pp.press(b'^')?;
    if out[0] != 0x2026 || out[1] != 0x2026 {
        return Err(E_PAIR_BROKEN);
    }
    let out = pp.press(b'_')?;
    if out[0] != 0x2014 || out[1] != 0x2014 {
        return Err(E_PAIR_BROKEN);
    }
    let out = pp.press(b'?')?;
    if out[0] != 0xFF1F || out[1] != 0 {
        return Err(E_PAIR_BROKEN);
    }
    hits += 1;

    // R5 表外键拒收：字母/数字不在配对表闭集，拒收且不落账。
    let mut pp = PunctPair::new();
    pp.open_session()?;
    if pp.press(b'a').is_ok() {
        return Err(E_BAD_KEY);
    }
    if pp.press(b'1').is_ok() {
        return Err(E_BAD_KEY);
    }
    if pp.press(0x7F).is_ok() {
        return Err(E_BAD_KEY);
    }
    if pp.press_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 会话门禁：未开会话按键拒、重复开会话拒、未开会话关会话拒。
    let mut pp = PunctPair::new();
    if pp.press(b'"').is_ok() {
        return Err(E_STATE_BAD);
    }
    pp.open_session()?;
    if pp.open_session().is_ok() {
        return Err(E_STATE_BAD);
    }
    pp.close_session()?;
    if pp.close_session().is_ok() {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R7 会话重置交替：新会话引号从开引号重新起（会话内配对语义，
    //    跨会话不串位次）。
    let mut pp = PunctPair::new();
    pp.open_session()?;
    pp.press(b'"')?;
    pp.press(b'"')?; // 上一会话内：开+闭
    pp.close_session()?;
    pp.open_session()?;
    if pp.press(b'"')?[0] != 0x201C {
        return Err(E_ALT_BROKEN); // 新会话第 1 次必出开引号
    }
    pp.audit_quote_balance()?;
    hits += 1;

    // R8 写回（判据 J2）：完成态 + 非空摘要放行，空摘要拒。
    let mut pp = PunctPair::new();
    if pp.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD); // 零按键不写回
    }
    pp.open_session()?;
    pp.press(b'^')?;
    pp.close_session()?;
    if pp.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    pp.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
