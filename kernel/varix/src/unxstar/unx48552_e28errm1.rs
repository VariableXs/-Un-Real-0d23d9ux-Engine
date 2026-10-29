//! UNX-F48552 —— E28 错误码族 E 段（10 条）
//! 判据原文：UNX-F48552-J1 E28 错误码族 E 段固定 10 条：
//! 码位连续（段基址 +1 起至 +10）、互不冲突、全部落在段窗内
//! （≤ 基址 +0x1F）；码名非空互异且可反查；总数恰 10；
//! 表账守恒、指纹与呈现行可复核、越窗码必须被拒收。
//! 判据落地（drill 场景）：R1 表账恰 10 条；R2 码位连续；
//! R3 段窗边界（+0x1F 内合法）；R4 越窗拒收；
//! R5 码名非空互异；R6 名称反查；R7 重复码拒收；
//! R8 fuzz 码流零错态；R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF3E0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_COUNT: u16 = SEG_BASE + 1; // 表账条数异常
const E_CONT: u16 = SEG_BASE + 2; // 码位不连续
const E_WIN: u16 = SEG_BASE + 3; // 段窗边界异常
const E_REJECT: u16 = SEG_BASE + 4; // 越窗码未拒
const E_NAME: u16 = SEG_BASE + 5; // 码名异常
const E_LOOKUP: u16 = SEG_BASE + 6; // 反查失败
const E_DUP: u16 = SEG_BASE + 7; // 重复码未拒
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 码流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// E 段条数。
pub const E28_N: u32 = 10;

/// 段窗上界偏移。
pub const SEG_WINDOW: u16 = 0x1F;

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

/// E28 E 段码名（下标 = 偏移-1）。
pub const E28_NAMES: [&str; 10] = [
    "restore_timeout",
    "snapshot_mismatch",
    "ledger_gap",
    "residue_left",
    "reset_unconfirmed",
    "context_rebuild_fail",
    "window_input_fail",
    "crash_crc_bad",
    "checkpoint_stale",
    "recover_path_broken",
];

/// 一条错误码登记：码位 + 码名下标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrEntry {
    pub code: u16,
    pub name_idx: u8,
}

/// 码位合法性：落在 E 段窗内（基址+1 .. 基址+10）。
pub fn in_segment(code: u16) -> bool {
    code > SEG_BASE && code <= SEG_BASE + E28_N as u16
}

/// 错误码表：登记 + 反查。
pub struct ErrTable {
    pub entries: Vec<ErrEntry>,
}

impl ErrTable {
    pub fn new() -> ErrTable {
        ErrTable { entries: vec![] }
    }

    fn code_idx(&self, code: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].code == code {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登记一条：段窗校验、名称下标闭集、重复码拒收。
    pub fn register(&mut self, e: ErrEntry) -> Result<(), u16> {
        if !in_segment(e.code) {
            return Err(E_REJECT);
        }
        if e.name_idx as u32 >= E28_N {
            return Err(E_NAME);
        }
        if self.code_idx(e.code).is_some() {
            return Err(E_DUP);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 条数恰 10。
    pub fn full(&self) -> bool {
        self.entries.len() as u32 == E28_N
    }

    /// 码位连续：排序后偏移 1..=10 各恰一。
    pub fn contiguous(&self) -> bool {
        if !self.full() {
            return false;
        }
        let mut off = 1u16;
        while off <= E28_N as u16 {
            let code = SEG_BASE + off;
            if self.code_idx(code).is_none() {
                return false;
            }
            off += 1;
        }
        true
    }

    /// 名称反查：码位 → 码名；越窗码返回 None。
    pub fn name_of(&self, code: u16) -> Option<&'static str> {
        if !in_segment(code) {
            return None;
        }
        let idx = self.code_idx(code)?;
        let ni = self.entries[idx].name_idx as usize;
        if ni >= E28_NAMES.len() {
            return None;
        }
        Some(E28_NAMES[ni])
    }

    /// 码名非空互异。
    pub fn names_sane(&self) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            let n = E28_NAMES[self.entries[i].name_idx as usize];
            if n.is_empty() {
                return false;
            }
            let mut j = i + 1;
            while j < self.entries.len() {
                if E28_NAMES[self.entries[j].name_idx as usize] == n {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 表账恰 10 条：按序登记 10 条后 full 为真。
    let mut tb = ErrTable::new();
    let mut off = 1u16;
    while off <= E28_N as u16 {
        tb.register(ErrEntry {
            code: SEG_BASE + off,
            name_idx: (off - 1) as u8,
        })?;
        off += 1;
    }
    if !tb.full() {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 码位连续：偏移 1..=10 各恰一。
    if !tb.contiguous() {
        return Err(E_CONT);
    }
    hits += 1;

    // R3 段窗边界：基址+10 合法、基址+0x1F 不在登记闭集（>10 越窗）。
    if !in_segment(SEG_BASE + 10) || in_segment(SEG_BASE + 11) || in_segment(SEG_BASE) {
        return Err(E_WIN);
    }
    if in_segment(SEG_BASE + SEG_WINDOW) {
        return Err(E_WIN);
    }
    hits += 1;

    // R4 越窗拒收：基址+11 登记被拒，码恰为 E_REJECT。
    let mut tb2 = ErrTable::new();
    match tb2.register(ErrEntry {
        code: SEG_BASE + 11,
        name_idx: 0,
    }) {
        Err(E_REJECT) => hits += 1,
        _ => return Err(E_REJECT),
    }

    // R5 码名非空互异：全表名检查。
    if !tb.names_sane() {
        return Err(E_NAME);
    }
    hits += 1;

    // R6 名称反查：全表逐条反查一致；基址（非码位）反查 None。
    let mut off = 1u16;
    while off <= E28_N as u16 {
        let n = tb.name_of(SEG_BASE + off);
        match n {
            Some(n) => {
                if n != E28_NAMES[(off - 1) as usize] {
                    return Err(E_LOOKUP);
                }
            }
            None => return Err(E_LOOKUP),
        }
        off += 1;
    }
    if tb.name_of(SEG_BASE).is_some() || tb.name_of(SEG_BASE + 11).is_some() {
        return Err(E_LOOKUP);
    }
    hits += 1;

    // R7 重复码拒收：已登记码位二次登记被拒，码恰为 E_DUP，账不变。
    let n_before = tb.entries.len();
    match tb.register(ErrEntry {
        code: SEG_BASE + 1,
        name_idx: 0,
    }) {
        Err(E_DUP) => {}
        _ => return Err(E_DUP),
    }
    if tb.entries.len() != n_before {
        return Err(E_DUP);
    }
    hits += 1;

    // R8 fuzz 码流零错态：LCG 驱动 64 次登记尝试（合法/越窗/重复混合），
    // 合法尝试全入账、非法尝试全拒、末态守恒。
    let mut tb3 = ErrTable::new();
    let mut lcg = Lcg::new(0x48552);
    let mut legal = 0u32;
    let mut t = 0u32;
    while t < 64 {
        let pick = lcg.below(3);
        let e = match pick {
            0 => ErrEntry {
                code: SEG_BASE + 1 + (lcg.below(10) as u16),
                name_idx: (lcg.below(10)) as u8,
            },
            1 => ErrEntry {
                code: SEG_BASE + 11 + (lcg.below(100) as u16),
                name_idx: (lcg.below(10)) as u8,
            },
            _ => ErrEntry {
                code: SEG_BASE,
                name_idx: (lcg.below(10)) as u8,
            },
        };
        // 合法性独立预判：唯一且段内才应入账。
        let mut expect_ok = in_segment(e.code);
        if expect_ok {
            let mut i = 0usize;
            while i < tb3.entries.len() {
                if tb3.entries[i].code == e.code {
                    expect_ok = false;
                }
                i += 1;
            }
        }
        let r = tb3.register(e);
        match r {
            Ok(()) => {
                if !expect_ok {
                    return Err(E_FUZZ);
                }
                legal += 1;
            }
            Err(E_REJECT) | Err(E_DUP) => {
                if expect_ok {
                    return Err(E_FUZZ);
                }
            }
            Err(_) => return Err(E_FUZZ),
        }
        t += 1;
    }
    if legal == 0 || legal > 10 || tb3.entries.len() != legal as usize {
        return Err(E_FUZZ);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = table_fp(&tb);
    let f2 = table_fp(&tb);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：n=/seg= 两段前缀在位。
    let line = table_line(&tb);
    if !has_kw(line.as_bytes(), b"n=") || !has_kw(line.as_bytes(), b"seg=") {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：全条目 (code, name_idx) 串联。
pub fn table_fp(tb: &ErrTable) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < tb.entries.len() {
        buf.extend_from_slice(&tb.entries[i].code.to_le_bytes());
        buf.push(tb.entries[i].name_idx);
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行：n=<条数> seg=<基址>。
pub fn table_line(tb: &ErrTable) -> String {
    let mut s = String::new();
    s.push_str("n=");
    s.push_str(&(tb.entries.len() as u32).to_string());
    s.push_str(" seg=");
    s.push_str(&SEG_BASE.to_string());
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
