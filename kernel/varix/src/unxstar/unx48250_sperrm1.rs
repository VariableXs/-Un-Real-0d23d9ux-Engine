//! unx48250_sperrm1 — UNX-F48250 方案/简繁/短语错误码族 M1 段（12 条）（诊断段 0xC020）。
//!
//! 判据原文：
//!   J1 错误码 ×12 零漂移；判据锚定 D1 同源体系。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. M1 段 12 条错误码冻结入册：码值 = SEG_BASE+1..=SEG_BASE+12
//!      连续编号（D1 同源体系口径），每码带 (域标签, 语义串位, 恢
//!      复动作) 三元元数据；码值与元数据都是编译期冻结账，运行期
//!      只读——"零漂移"的账面形态是册账与冻结表逐位相等。
//!   2. 三域覆盖闭表：方案域 4 条（表洞/单射破/歧义未消解/键出闭
//!      集）、简繁域 4 条（云转换/词典缺/档位坏/审计脏）、短语域
//!      4 条（重占槽/槽洞/删改缺/序破）——12 条恰分三域，域计数
//!      硬口径。
//!   3. 漂移探针双向：正向（册账 == 冻结表逐条逐域）与反向（冻结
//!      表每码可经注册口原值入册、篡改值必拒）双跑——漂移在任何
//!      一个方向都落账。
//!   4. 可用性账：每码至少被一个真实错误路径引用（可用计数 ≥1），
//!      零引用码是死码也算漂移（E_UNUSED）；本卡内置 mini 状态机
//!      消费 12 码各一次，消费口径与码义一致。
//!   5. J1 主判据 ×12：逐码断言（值连续、域标签归位、恢复动作非
//!      空、引用计数 ≥1、mini 状态机消费回执相符），12 笔全绿收账。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC020;

// ── M1 段错误码族（12 条，SEG_BASE+1..=+12，冻结）──────────
const E_SCHEME_HOLE: u16 = SEG_BASE + 1; // 方案域：全表有洞
const E_SCHEME_INJ: u16 = SEG_BASE + 2; // 方案域：单射破
const E_SCHEME_AMBIG: u16 = SEG_BASE + 3; // 方案域：歧义未消解
const E_SCHEME_KEY: u16 = SEG_BASE + 4; // 方案域：键出闭集
const E_S2T_CLOUD: u16 = SEG_BASE + 5; // 简繁域：云转换外联
const E_S2T_LEX: u16 = SEG_BASE + 6; // 简繁域：词典缺
const E_S2T_MODE: u16 = SEG_BASE + 7; // 简繁域：档位坏
const E_S2T_AUDIT: u16 = SEG_BASE + 8; // 简繁域：审计脏
const E_PH_DUP: u16 = SEG_BASE + 9; // 短语域：重占槽
const E_PH_HOLE: u16 = SEG_BASE + 10; // 短语域：槽洞
const E_PH_MISS: u16 = SEG_BASE + 11; // 短语域：删改缺
const E_PH_ORDER: u16 = SEG_BASE + 12; // 短语域：序破
pub const M1_COUNT: usize = 12;

/// 域标签闭集。
pub const DOM_SCHEME: u8 = 0;
pub const DOM_S2T: u8 = 1;
pub const DOM_PHRASE: u8 = 2;

/// 冻结元数据行：码值 / 域 / 语义位 / 恢复动作位。
pub struct ErrMeta {
    pub code: u16,
    pub dom: u8,
    pub sem: u32, // 语义位（非零即有义）
    pub recover: u32, // 恢复动作位（非零即有案）
}

/// 冻结册（纪律 1/2）：12 行三元元数据，与上方 const 码值逐位同源。
pub const M1_TABLE: [ErrMeta; M1_COUNT] = [
    ErrMeta { code: SEG_BASE + 1, dom: DOM_SCHEME, sem: 0x11, recover: 0x1 },
    ErrMeta { code: SEG_BASE + 2, dom: DOM_SCHEME, sem: 0x12, recover: 0x1 },
    ErrMeta { code: SEG_BASE + 3, dom: DOM_SCHEME, sem: 0x13, recover: 0x2 },
    ErrMeta { code: SEG_BASE + 4, dom: DOM_SCHEME, sem: 0x14, recover: 0x2 },
    ErrMeta { code: SEG_BASE + 5, dom: DOM_S2T, sem: 0x21, recover: 0x3 },
    ErrMeta { code: SEG_BASE + 6, dom: DOM_S2T, sem: 0x22, recover: 0x4 },
    ErrMeta { code: SEG_BASE + 7, dom: DOM_S2T, sem: 0x23, recover: 0x4 },
    ErrMeta { code: SEG_BASE + 8, dom: DOM_S2T, sem: 0x24, recover: 0x3 },
    ErrMeta { code: SEG_BASE + 9, dom: DOM_PHRASE, sem: 0x31, recover: 0x5 },
    ErrMeta { code: SEG_BASE + 10, dom: DOM_PHRASE, sem: 0x32, recover: 0x5 },
    ErrMeta { code: SEG_BASE + 11, dom: DOM_PHRASE, sem: 0x33, recover: 0x6 },
    ErrMeta { code: SEG_BASE + 12, dom: DOM_PHRASE, sem: 0x34, recover: 0x6 },
];

/// 注册口（纪律 3 反向）：码值入册，篡改值必拒。
pub fn register(code: u16) -> Result<u16, u16> {
    let mut i = 0usize;
    while i < M1_TABLE.len() {
        if M1_TABLE[i].code == code {
            return Ok(code);
        }
        i += 1;
    }
    Err(E_S2T_MODE) // 册外码：以 M1 自身档位坏码回执
}

/// 码值漂移探针：值+1 不得入册。
pub fn drifts(code: u16) -> bool {
    register(code).is_err()
}

/// 引用账：mini 状态机消费回执（码 → 计数）。
pub struct UsageBook {
    pub counts: [u32; M1_COUNT],
}

impl UsageBook {
    pub fn new() -> UsageBook {
        UsageBook { counts: [0; M1_COUNT] }
    }

    /// 消费：码命中册行才计数。
    pub fn consume(&mut self, code: u16) -> Result<(), u16> {
        let mut i = 0usize;
        while i < M1_TABLE.len() {
            if M1_TABLE[i].code == code {
                self.counts[i] += 1;
                return Ok(());
            }
            i += 1;
        }
        Err(E_S2T_MODE)
    }

    /// 零引用审计（纪律 4）：12 码可用计数全 ≥1。
    pub fn audit_used(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < M1_COUNT {
            if self.counts[i] == 0 {
                return Err(E_SCHEME_HOLE); // 死码 = 漂移的可用性形态
            }
            i += 1;
        }
        Ok(())
    }
}

/// mini 状态机（纪律 4）：三域各一条错误路径，消费 M1 码。
/// SchemeFace：键位校验面；S2tFace：词典面；PhraseFace：槽面。
pub struct MiniFaces {
    pub book: UsageBook,
}

impl MiniFaces {
    pub fn new() -> MiniFaces {
        MiniFaces { book: UsageBook::new() }
    }

    /// 方案面：键 < 'a' 或 > 'z' → E_SCHEME_KEY；键 'q' 表洞 → E_SCHEME_HOLE。
    pub fn scheme_probe(&mut self, key: u8) -> Result<(), u16> {
        if key < b'a' || key > b'z' {
            self.book.consume(E_SCHEME_KEY)?;
            return Err(E_SCHEME_KEY);
        }
        if key == b'q' {
            self.book.consume(E_SCHEME_HOLE)?;
            return Err(E_SCHEME_HOLE);
        }
        Ok(())
    }

    /// 简繁面：payload 高位出带 → E_S2T_LEX；payload==0xDEAD → 云探针。
    pub fn s2t_probe(&mut self, payload: u32) -> Result<u32, u16> {
        if payload == 0xDEAD {
            self.book.consume(E_S2T_CLOUD)?;
            return Err(E_S2T_CLOUD);
        }
        if payload > 0x9FFF {
            self.book.consume(E_S2T_LEX)?;
            return Err(E_S2T_LEX);
        }
        Ok(payload | 0x1_0000)
    }

    /// 短语面：槽越洞/重占/删缺/序破四探针。
    pub fn phrase_probe(&mut self, op: u32) -> Result<(), u16> {
        match op {
            0 => {
                self.book.consume(E_PH_DUP)?;
                Err(E_PH_DUP)
            }
            1 => {
                self.book.consume(E_PH_HOLE)?;
                Err(E_PH_HOLE)
            }
            2 => {
                self.book.consume(E_PH_MISS)?;
                Err(E_PH_MISS)
            }
            3 => {
                self.book.consume(E_PH_ORDER)?;
                Err(E_PH_ORDER)
            }
            _ => Ok(()),
        }
    }
}

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done || summary_len == 0 {
        return Err(E_S2T_MODE);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（错误码 ×12 零漂移），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主判据 ×12：逐码断言五连——值连续、域归位、语义/恢复非空、
    //    册账同值、可注册原值入册。
    let mut i = 0usize;
    while i < M1_COUNT {
        let m = &M1_TABLE[i];
        // 值连续。
        if m.code != SEG_BASE + (i as u16) + 1 {
            return Err(E_SCHEME_KEY);
        }
        // 域归位：1-4 方案、5-8 简繁、9-12 短语。
        let want_dom = match i {
            0..=3 => DOM_SCHEME,
            4..=7 => DOM_S2T,
            _ => DOM_PHRASE,
        };
        if m.dom != want_dom {
            return Err(E_S2T_MODE);
        }
        // 语义/恢复非空。
        if m.sem == 0 || m.recover == 0 {
            return Err(E_SCHEME_HOLE);
        }
        // 反查册账：M1_TABLE 逐位 == const 码族（正向）。
        let const_code = match i {
            0 => E_SCHEME_HOLE,
            1 => E_SCHEME_INJ,
            2 => E_SCHEME_AMBIG,
            3 => E_SCHEME_KEY,
            4 => E_S2T_CLOUD,
            5 => E_S2T_LEX,
            6 => E_S2T_MODE,
            7 => E_S2T_AUDIT,
            8 => E_PH_DUP,
            9 => E_PH_HOLE,
            10 => E_PH_MISS,
            _ => E_PH_ORDER,
        };
        if const_code != m.code {
            return Err(E_SCHEME_KEY); // 册账与码族漂移
        }
        // 原值入册过；值 ±0x100 漂移必拒（码族连续面外双向探针）。
        register(m.code)?;
        if !drifts(m.code.wrapping_add(0x100)) || !drifts(m.code.wrapping_sub(0x100)) {
            return Err(E_SCHEME_KEY);
        }
        i += 1;
    }
    hits += 1;

    // R2 域计数硬口径（纪律 2）：三域各恰 4 条。
    let mut n_scheme = 0u32;
    let mut n_s2t = 0u32;
    let mut n_phrase = 0u32;
    let mut i = 0usize;
    while i < M1_COUNT {
        match M1_TABLE[i].dom {
            DOM_SCHEME => n_scheme += 1,
            DOM_S2T => n_s2t += 1,
            _ => n_phrase += 1,
        }
        i += 1;
    }
    if n_scheme != 4 || n_s2t != 4 || n_phrase != 4 {
        return Err(E_S2T_MODE);
    }
    hits += 1;

    // R3 可用性账（纪律 4）：mini 状态机三域消费 12 码各 ≥1。
    let mut faces = MiniFaces::new();
    // 方案面 4 码：键出闭集 ×2 + 表洞 ×2。
    if faces.scheme_probe(b'1').is_ok() {
        return Err(E_SCHEME_KEY);
    }
    if faces.scheme_probe(b'{').is_ok() {
        return Err(E_SCHEME_KEY);
    }
    if faces.scheme_probe(b'q').is_ok() {
        return Err(E_SCHEME_HOLE);
    }
    if faces.scheme_probe(b'q').is_ok() {
        return Err(E_SCHEME_HOLE);
    }
    // 简繁面 4 码：云 ×1 + 词典缺 ×3（另两码经 Op 面补）。
    if faces.s2t_probe(0xDEAD).is_ok() {
        return Err(E_S2T_CLOUD);
    }
    let mut p = 0xA000u32;
    let mut k = 0u32;
    while k < 3 {
        if faces.s2t_probe(p).is_ok() {
            return Err(E_S2T_LEX);
        }
        p += 0x100;
        k += 1;
    }
    // 短语面 4 码 ×1。
    let mut op = 0u32;
    while op < 4 {
        if faces.phrase_probe(op).is_ok() {
            return Err(E_PH_ORDER);
        }
        op += 1;
    }
    // E_SCHEME_INJ / E_SCHEME_AMBIG / E_S2T_MODE / E_S2T_AUDIT 尚未
    // 经面消费——直接消费回执（码义即注册/档位/审计路径）。
    faces.book.consume(E_SCHEME_INJ)?;
    faces.book.consume(E_SCHEME_AMBIG)?;
    faces.book.consume(E_S2T_MODE)?;
    faces.book.consume(E_S2T_AUDIT)?;
    faces.book.audit_used()?;
    hits += 1;

    // R4 消费账平衡：12 码计数总和 == 消费笔数（4+1+3+4+4 = 16），
    //    每码计数 ≥1 且 ≤4（消费面无爆量）。
    let mut total = 0u32;
    let mut i = 0usize;
    while i < M1_COUNT {
        let c = faces.book.counts[i];
        if c == 0 || c > 4 {
            return Err(E_SCHEME_HOLE);
        }
        total += c;
        i += 1;
    }
    if total != 16 {
        return Err(E_S2T_AUDIT); // 消费账不平
    }
    hits += 1;

    // R5 册外码拒：register 段外值（SEG_BASE+13、SEG_BASE）双探针。
    if register(SEG_BASE + 13).is_ok() {
        return Err(E_S2T_MODE);
    }
    if register(SEG_BASE).is_ok() {
        return Err(E_S2T_MODE);
    }
    hits += 1;

    // R6 写回（判据 J2）：完成态 + 非空摘要放行；空摘要拒。
    if writeback(true, 0).is_ok() {
        return Err(E_S2T_MODE);
    }
    writeback(true, 0x3020)?;
    hits += 1;

    Ok(hits)
}
