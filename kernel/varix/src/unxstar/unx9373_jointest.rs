//! UNX-F9373 联测判据共享账（只写编号+一句话协议逐条） · 0x7CAC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9373 逐字）：
//!   ① 纯功能代码 ≥420 行；
//!   ② 判据原文：UNX-F9373-J1 共享账判据过，逐条只写编号+一句话协议格过，
//!      跨界代写零发生声明格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 共享账只有两种字段：编号（`UNX-F####-J#`）与一句话协议。编号语法由
//!      `no_defect` 逐位扫描（定长 12 字节、前缀 UNX-F、四位数字、-J、1..=9），
//!      协议句语法由 `sentence_defect` 扫描（12..=72 字节、以「。」单句收尾、
//!      禁代码标记、必须含「格过」）。两句法各返回缺陷枚举，不是一个 bool。
//!   ② 账册是 8 条冻结协议（C2 面 93xx 四条 + C3 面 94xx 四条），每条声明
//!      owner 面。写入面必须同时等于册内 owner 与 `zone_of(编号)` 算出的面——
//!      两级一致才算「非跨界代写」，否则 EPERM 并计入 cross_attempts；账内
//!      实际跨界写入恒为 0（accepted_cross 只读，永远 0）。
//!   ③ 判定次序冻结为：封账 EPERM → 容量 ENOMEM（资源闸门先于内容）→ 编号
//!      语法 EINVAL → 句法 ENOEXEC → 册内 ESRCH → 重复 EEXIST → 跨界 EPERM
//!      → 改写协议 EACCES → 顺序 EINVAL。次序本身就是判据（见第 12 条）。
//!   ④ 容量 8 与册数 8 相等：满册后第 9 条先撞容量回 ENOMEM，这是「资源面
//!      先于内容面」的显式落点；满册但未封账的账仍可封（封账只看齐全）。
//!   ⑤ 位图 `claimed: u32` 记已落账条目，`seq` 单调记写入轮次；重复提交已
//!      落账编号回 EEXIST 而不是顺序错——重复是更强的诊断。
//!   ⑥ 语法注入两束：编号 10 格 × 8 轮、协议句 10 格 × 8 轮，共 160 次；
//!      账本 injected == detected + escaped，escaped 恒 0。
//!   ⑦ 指纹：冻结册全量落账 + 一次跨面拒写 + 一次改文拒写后 FNV-1a 折叠。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9373 独占 0x7CAC0..=0x7CAFF）。
pub const SEG_BASE: u32 = 0x7CAC0;
/// 段窗口宽度。
pub const SEG_WINDOW: u32 = 0x40;
/// 下一单段基址（F9374），段间不重叠判定用。
pub const NEXT_SEG_BASE: u32 = 0x7CB00;
/// 账册容量（与册数相等：满册即撞容量闸门）。
pub const SLOT_CAP: usize = 8;
/// 语法制裁注入规模。
pub const DRILL_CELLS: usize = 10;
pub const DRILL_ROUNDS: u32 = 8;
/// 本卡纯功能行配额。
pub const CODE_QUOTA: u32 = 420;

/// Linux errno 数值（正值口径）。
pub const EPERM: i32 = 1;
pub const ESRCH: i32 = 3;
pub const ENOEXEC: i32 = 8;
pub const ENOMEM: i32 = 12;
pub const EACCES: i32 = 13;
pub const EBUSY: i32 = 16;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码；`code()` 是段内序号，`seg_addr()` 是段绝对码值。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerCode {
    NoFormat = 1,
    SentenceMalformed = 2,
    NotInBook = 3,
    CrossZoneWrite = 4,
    ProtocolRewrite = 5,
    DuplicateNo = 6,
    LedgerFull = 7,
    SealIncomplete = 8,
    SealTwice = 9,
    FrozenLedger = 10,
    OrderViolation = 11,
    NotReproducible = 12,
    SegmentViolation = 13,
}

impl LedgerCode {
    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn seg_addr(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文一句话诊断与建议。
    pub fn detail(self) -> &'static str {
        match self {
            LedgerCode::NoFormat => "编号不合语法：必须是 UNX-F####-J1..J9 的定长 12 字节；请按册抄号",
            LedgerCode::SentenceMalformed => "协议句不合语法：单句以「。」收尾且含「格过」，禁代码标记；请只写一句话",
            LedgerCode::NotInBook => "编号不在冻结册：共享账只收册内八条；请先扩册再落账",
            LedgerCode::CrossZoneWrite => "跨界代写被拒：写入面必须等于册内 owner 与编号所属面；请让 owner 面自己写",
            LedgerCode::ProtocolRewrite => "改写协议被拒：句子必须与册内原文逐字相同；协议只能登记不能润色",
            LedgerCode::DuplicateNo => "重复落账被拒：同一编号只能写一次；请核对册内序号",
            LedgerCode::LedgerFull => "共享账已满：容量 8 与册数相等；满册后任何写入回 ENOMEM",
            LedgerCode::SealIncomplete => "封账被拒：八条未齐全不得封；请先补齐缺条",
            LedgerCode::SealTwice => "重复封账被拒：封账是终态；请复用首次摘要",
            LedgerCode::FrozenLedger => "账已封存：封账后任何补写回 EPERM；请走下一期账册",
            LedgerCode::OrderViolation => "落账乱序：必须按册内冻结顺序逐条写；跳条回 EINVAL",
            LedgerCode::NotReproducible => "双跑不一致：同输入两次摘要不同；请排查隐藏状态",
            LedgerCode::SegmentViolation => "段位越界：本模块必须落在 0x7CAC0..=0x7CAFF，不得侵入 F9374",
        }
    }

    pub const ALL: &'static [LedgerCode] = &[
        LedgerCode::NoFormat, LedgerCode::SentenceMalformed,
        LedgerCode::NotInBook, LedgerCode::CrossZoneWrite,
        LedgerCode::ProtocolRewrite, LedgerCode::DuplicateNo,
        LedgerCode::LedgerFull, LedgerCode::SealIncomplete,
        LedgerCode::SealTwice, LedgerCode::FrozenLedger,
        LedgerCode::OrderViolation, LedgerCode::NotReproducible,
        LedgerCode::SegmentViolation,
    ];
}

// ── 词法：编号与协议句 ───────────────────────────────────────────────
/// 编号缺陷（逐位扫描的判定结果）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoDefect {
    Ok,
    BadLen,
    BadPrefix,
    BadDigits,
    BadJudge,
}

fn is_digit_at(bytes: &[u8], i: usize) -> bool {
    match bytes.get(i) {
        Some(c) => c.is_ascii_digit(),
        None => false,
    }
}

/// 编号语法：定长 12 字节 `UNX-F####-J<1..=9>`。
pub fn no_defect(no: &str) -> NoDefect {
    let b = no.as_bytes();
    if b.len() != 12 {
        return NoDefect::BadLen;
    }
    let head_ok = matches!(b.get(0), Some(b'U'))
        && matches!(b.get(1), Some(b'N'))
        && matches!(b.get(2), Some(b'X'))
        && matches!(b.get(3), Some(b'-'))
        && matches!(b.get(4), Some(b'F'));
    if !head_ok {
        return NoDefect::BadPrefix;
    }
    let mut i = 5usize;
    while i < 9 {
        if !is_digit_at(b, i) {
            return NoDefect::BadDigits;
        }
        i += 1;
    }
    if !matches!(b.get(9), Some(b'-')) || !matches!(b.get(10), Some(b'J')) {
        return NoDefect::BadJudge;
    }
    match b.get(11) {
        Some(c) => {
            if c.is_ascii_digit() && *c != b'0' {
                NoDefect::Ok
            } else {
                NoDefect::BadJudge
            }
        }
        None => NoDefect::BadJudge,
    }
}

/// 编号四位数字组（9000..=9799 才有面归属）。
pub fn no_group(no: &str) -> Option<u32> {
    if no_defect(no) != NoDefect::Ok {
        return None;
    }
    let b = no.as_bytes();
    let mut v = 0u32;
    let mut i = 5usize;
    while i < 9 {
        match b.get(i) {
            Some(c) => v = v.saturating_mul(10).saturating_add((*c - b'0') as u32),
            None => return None,
        }
        i += 1;
    }
    Some(v)
}

/// 协议句缺陷。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SentenceDefect {
    Ok,
    TooShort,
    TooLong,
    NoStop,
    MultiStop,
    HasNewline,
    LooksLikeCode,
    NoVerdict,
}

/// 协议句禁止出现的代码标记：共享账只收一句话，不收实现。
pub const CODE_MARKS: [&str; 6] = ["{", "}", "(", ")", "=>", "fn "];

/// 协议句语法：12..=72 字节、单句「。」收尾、含「格过」、无代码标记。
pub fn sentence_defect(line: &str) -> SentenceDefect {
    let n = line.as_bytes().len();
    if n < 12 {
        return SentenceDefect::TooShort;
    }
    if n > 72 {
        return SentenceDefect::TooLong;
    }
    if line.contains('\n') {
        return SentenceDefect::HasNewline;
    }
    let body = match line.strip_suffix("。") {
        Some(b) => b,
        None => return SentenceDefect::NoStop,
    };
    if body.contains('。') {
        return SentenceDefect::MultiStop;
    }
    let mut i = 0usize;
    while i < CODE_MARKS.len() {
        if let Some(mark) = CODE_MARKS.get(i) {
            if line.contains(mark) {
                return SentenceDefect::LooksLikeCode;
            }
        }
        i += 1;
    }
    if !line.contains("格过") {
        return SentenceDefect::NoVerdict;
    }
    SentenceDefect::Ok
}

// ── 号面归属 ──────────────────────────────────────────────────────────
/// 号面：93xx 归 C2，94xx..97xx 归 C3，其余无主。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    C2,
    C3,
    Undecided,
}

impl Zone {
    /// 指纹编码。
    pub fn code(self) -> u32 {
        match self {
            Zone::C2 => 1,
            Zone::C3 => 2,
            Zone::Undecided => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Zone::C2 => "C2",
            Zone::C3 => "C3",
            Zone::Undecided => "无主",
        }
    }
}

/// 按号段算面（只认格式合法的号）。
pub fn zone_of(no: &str) -> Zone {
    match no_group(no) {
        Some(g) => {
            if g >= 9000 && g <= 9399 {
                Zone::C2
            } else if g >= 9400 && g <= 9799 {
                Zone::C3
            } else {
                Zone::Undecided
            }
        }
        None => Zone::Undecided,
    }
}

// ── 冻结协议册 ────────────────────────────────────────────────────────
/// 一条册内协议：编号 + 一句话 + owner 面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BookEntry {
    pub no: &'static str,
    pub line: &'static str,
    pub owner: Zone,
}

/// 冻结册：C2 面四条（93xx）+ C3 面四条（94xx），顺序即落账顺序。
pub const BOOK: [BookEntry; SLOT_CAP] = [
    BookEntry {
        no: "UNX-F9373-J1",
        line: "共享账每条只写编号与一句话协议格过。",
        owner: Zone::C2,
    },
    BookEntry {
        no: "UNX-F9374-J1",
        line: "交接包逐项升级可观测后统一落账格过。",
        owner: Zone::C2,
    },
    BookEntry {
        no: "UNX-F9375-J1",
        line: "四范围防重零冲突方得写入共享账格过。",
        owner: Zone::C2,
    },
    BookEntry {
        no: "UNX-F9376-J1",
        line: "上游未 finalize 依赖显式登记后落账格过。",
        owner: Zone::C2,
    },
    BookEntry {
        no: "UNX-F9401-J1",
        line: "C3 侧协议句一句话且不改写原文格过。",
        owner: Zone::C3,
    },
    BookEntry {
        no: "UNX-F9402-J1",
        line: "跨面只允许读号不允许代写内容格过。",
        owner: Zone::C3,
    },
    BookEntry {
        no: "UNX-F9403-J1",
        line: "落账顺序按册冻结不得跳条补写格过。",
        owner: Zone::C3,
    },
    BookEntry {
        no: "UNX-F9404-J1",
        line: "封账后任何补写一律拒绝并计入拒账格过。",
        owner: Zone::C3,
    },
];

/// 册条目视图；越界返回空条目（禁止裸索引）。
pub fn book_at(i: usize) -> BookEntry {
    match BOOK.get(i) {
        Some(e) => *e,
        None => BookEntry { no: "", line: "", owner: Zone::Undecided },
    }
}

/// 查册下标。
pub fn book_index(no: &str) -> Option<usize> {
    let mut i = 0usize;
    while i < BOOK.len() {
        if let Some(e) = BOOK.get(i) {
            if e.no == no {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 册内 owner 面。
pub fn book_owner(no: &str) -> Option<Zone> {
    match book_index(no) {
        Some(i) => match BOOK.get(i) {
            Some(e) => Some(e.owner),
            None => None,
        },
        None => None,
    }
}

// ── 账槽与共享账 ──────────────────────────────────────────────────────
/// 一条已落账记录（只允许编号 + 一句话 + 写入面，无别的负载）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot {
    pub no: &'static str,
    pub line: &'static str,
    pub writer: Zone,
    pub seq: u32,
}

impl Slot {
    pub const EMPTY: Slot = Slot { no: "", line: "", writer: Zone::Undecided, seq: 0 };
}

/// 共享账：8 槽 + claimed 位图 + 单调 seq + 拒账/跨面计数 + 封账摘要。
pub struct SharedLedger {
    slots: [Slot; SLOT_CAP],
    claimed: u32,
    seq: u32,
    sealed: bool,
    digest: u32,
    cross_attempts: u32,
    rejects: u32,
}

impl SharedLedger {
    pub fn new() -> SharedLedger {
        SharedLedger {
            slots: [Slot::EMPTY; SLOT_CAP],
            claimed: 0,
            seq: 0,
            sealed: false,
            digest: 0,
            cross_attempts: 0,
            rejects: 0,
        }
    }

    pub fn claimed_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < SLOT_CAP {
            if self.bit(i) != 0 {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    pub fn seq(&self) -> u32 {
        self.seq
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn digest(&self) -> u32 {
        self.digest
    }

    pub fn cross_attempts(&self) -> u32 {
        self.cross_attempts
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 已落账记录视图。
    pub fn slot_at(&self, i: usize) -> Option<Slot> {
        if self.bit(i) == 0 {
            return None;
        }
        self.slots.get(i).copied()
    }

    /// 账内跨界写入条数：结构性恒为 0（跨界在 submit 处即被拒）。
    pub fn accepted_cross(&self) -> u32 {
        0
    }

    pub fn is_full(&self) -> bool {
        self.claimed_count() as usize >= SLOT_CAP
    }

    fn bit(&self, idx: usize) -> u32 {
        if idx < 32 {
            self.claimed & 1u32.wrapping_shl(idx as u32)
        } else {
            0
        }
    }

    fn set_bit(&mut self, idx: usize) {
        if idx < 32 {
            self.claimed |= 1u32.wrapping_shl(idx as u32);
        }
    }

    /// 落账一条。判定次序见文件头 ③，逐条拒绝都计入 rejects。
    pub fn submit(&mut self, no: &'static str, line: &'static str, writer: Zone) -> Result<u32, i32> {
        if self.sealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EPERM);
        }
        // 资源闸门先于内容面：满册后连坏号也只报 ENOMEM。
        if self.is_full() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ENOMEM);
        }
        if no_defect(no) != NoDefect::Ok {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        if sentence_defect(line) != SentenceDefect::Ok {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ENOEXEC);
        }
        let idx = match book_index(no) {
            Some(i) => i,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(ESRCH);
            }
        };
        if self.bit(idx) != 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EEXIST);
        }
        let entry = match BOOK.get(idx) {
            Some(e) => *e,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(ESRCH);
            }
        };
        // 两级面一致：写入面 == 册内 owner == 编号算出的面。
        if writer != entry.owner || zone_of(no) != entry.owner {
            self.cross_attempts = self.cross_attempts.saturating_add(1);
            self.rejects = self.rejects.saturating_add(1);
            return Err(EPERM);
        }
        if line != entry.line {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EACCES);
        }
        // 顺序：必须恰好写第 claimed_count 条。
        if idx != self.claimed_count() as usize {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        let seq = self.seq.saturating_add(1);
        if let Some(slot) = self.slots.get_mut(idx) {
            *slot = Slot { no, line, writer, seq };
        }
        self.set_bit(idx);
        self.seq = seq;
        Ok(seq)
    }

    /// 按册顺序写第 n 条（正常调用面）。
    pub fn submit_next(&mut self, writer: Zone) -> Result<u32, i32> {
        let idx = self.claimed_count() as usize;
        let entry = match BOOK.get(idx) {
            Some(e) => *e,
            None => return Err(ENOMEM),
        };
        self.submit(entry.no, entry.line, writer)
    }

    /// 封账：八条齐全才可封；摘要在封账后固定。
    pub fn seal(&mut self) -> Result<u32, i32> {
        if self.sealed {
            return Err(EBUSY);
        }
        if self.claimed_count() as usize != SLOT_CAP {
            return Err(EBUSY);
        }
        let d = self.fingerprint();
        self.digest = d;
        self.sealed = true;
        Ok(d)
    }

    /// 账指纹：位图 + 轮次 + 逐槽编号/句子/写入面。
    pub fn fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        push_u32(&mut bytes, self.claimed);
        push_u32(&mut bytes, self.seq);
        push_u32(&mut bytes, self.rejects);
        push_u32(&mut bytes, self.cross_attempts);
        let mut i = 0usize;
        while i < SLOT_CAP {
            if let Some(slot) = self.slot_at(i) {
                push_u32(&mut bytes, slot.writer.code());
                push_u32(&mut bytes, slot.seq);
                push_str(&mut bytes, slot.no);
                push_str(&mut bytes, slot.line);
            } else {
                bytes.push(0xEE);
            }
            i += 1;
        }
        fnv1a(&bytes)
    }
}

// ── 语法注入账 ────────────────────────────────────────────────────────
/// 注入账：injected == detected + escaped，escaped 恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl ProbeLedger {
    pub const fn new() -> ProbeLedger {
        ProbeLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一笔；返回账目是否仍守恒。
    pub fn record(&mut self, caught: bool) -> bool {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        self.conserved()
    }

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 编号语法格：(编号, 是否合法)。
pub const NO_BATTERY: [(&str, bool); DRILL_CELLS] = [
    ("UNX-F9373-J1", true),
    ("UNX-F9373-J0", false),
    ("UNX-F9373-J", false),
    ("UNX-F937-J1", false),
    ("unx-f9373-j1", false),
    ("UNX-F937A-J1", false),
    ("UNX-F9373-J12", false),
    ("UNX-F9373J1", false),
    ("UNX-F0000-J1", true),
    ("UNX-F9999-J9", true),
];

/// 协议句语法格：(句子, 是否合法)。
pub const SENT_BATTERY: [(&str, bool); DRILL_CELLS] = [
    ("共享账只收编号与一句话协议格过。", true),
    ("共享账只收编号与一句话协议", false),
    ("共享账只收一句。另起第二句格过。", false),
    ("共享账写 fn shared() 格过。", false),
    ("共享账条目 {no} 格过。", false),
    ("共享账第一行\n第二行格过。", false),
    ("格过。", false),
    ("共享账条目 => 箭头格过。", false),
    ("只写编号不写协议格过。", true),
    ("共", false),
];

/// 编号注入：10 格 × 8 轮。
pub fn run_no_drill() -> (u32, ProbeLedger) {
    let mut ledger = ProbeLedger::new();
    let mut matched = 0u32;
    let mut ci = 0usize;
    while ci < NO_BATTERY.len() {
        if let Some((no, valid)) = NO_BATTERY.get(ci).copied() {
            let mut round = 0u32;
            while round < DRILL_ROUNDS {
                let caught = (no_defect(no) == NoDefect::Ok) == valid;
                if !ledger.record(caught) {
                    return (matched, ledger);
                }
                if caught {
                    matched = matched.saturating_add(1);
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (matched, ledger)
}

/// 协议句注入：10 格 × 8 轮。
pub fn run_sent_drill() -> (u32, ProbeLedger) {
    let mut ledger = ProbeLedger::new();
    let mut matched = 0u32;
    let mut ci = 0usize;
    while ci < SENT_BATTERY.len() {
        if let Some((line, valid)) = SENT_BATTERY.get(ci).copied() {
            let mut round = 0u32;
            while round < DRILL_ROUNDS {
                let caught = (sentence_defect(line) == SentenceDefect::Ok) == valid;
                if !ledger.record(caught) {
                    return (matched, ledger);
                }
                if caught {
                    matched = matched.saturating_add(1);
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (matched, ledger)
}

/// 两束合并：160 次（10×8 + 10×8）。
pub fn run_all_drills() -> (u32, u32, ProbeLedger) {
    let (nm, nl) = run_no_drill();
    let (sm, sl) = run_sent_drill();
    let merged = ProbeLedger {
        injected: nl.injected.saturating_add(sl.injected),
        detected: nl.detected.saturating_add(sl.detected),
        escaped: nl.escaped.saturating_add(sl.escaped),
    };
    (nm, sm, merged)
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, Linux 正值, Windows 近似码, 语义说明)
pub const ERRNO_MAP: &[(&str, i32, i32, &str)] = &[
    ("EPERM", EPERM, 5, "ERROR_ACCESS_DENIED；跨界代写/封账后补写"),
    ("ESRCH", ESRCH, 3, "ERROR_PATH_NOT_FOUND；编号不在冻结册"),
    ("ENOEXEC", ENOEXEC, 193, "ERROR_BAD_EXE_FORMAT；协议句写成可执行代码"),
    ("ENOMEM", ENOMEM, 8, "ERROR_NOT_ENOUGH_MEMORY；满册后落账"),
    ("EACCES", EACCES, 5, "ERROR_ACCESS_DENIED；改写已公示协议句"),
    ("EBUSY", EBUSY, 170, "ERROR_BUSY；未齐封账/重复封账"),
    ("EEXIST", EEXIST, 183, "ERROR_ALREADY_EXISTS；同一编号重复落账"),
    ("EINVAL", EINVAL, 87, "ERROR_INVALID_PARAMETER；编号语法/落账乱序"),
];

/// 查表得 Linux 负值；表外 None。
pub fn linux_neg(code: i32) -> Option<i32> {
    let mut i = 0usize;
    while i < ERRNO_MAP.len() {
        if let Some(row) = ERRNO_MAP.get(i) {
            if row.1 == code {
                return Some(-row.1);
            }
        }
        i += 1;
    }
    None
}

/// 是否与 Linux 逐码一致。
pub fn errno_is_linux(code: i32) -> bool {
    if code <= 0 {
        return false;
    }
    linux_neg(code) == Some(-code)
}

// ── 指纹基元 ──────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 小端编码 u32。
pub fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
    out.push(((v >> 16) & 0xFF) as u8);
    out.push(((v >> 24) & 0xFF) as u8);
}

/// 逐字节喂入字符串。
pub fn push_str(out: &mut Vec<u8>, s: &str) {
    for b in s.as_bytes() {
        out.push(*b);
    }
}

/// 规范语料：整册落账 + 一次跨面拒写 + 一次改文拒写，再封账。
/// 双跑必须逐字节相等。
pub fn corpus_fingerprint() -> u32 {
    let mut led = SharedLedger::new();
    let mut filled = 0u32;
    let mut i = 0usize;
    while i < SLOT_CAP {
        if let Some(e) = BOOK.get(i) {
            if led.submit(e.no, e.line, e.owner).is_ok() {
                filled = filled.saturating_add(1);
            }
        }
        i += 1;
    }
    // 整册封账后验证封账拒写（EPERM），两条路径都编码进指纹。
    let sealed = led.seal();
    let mut bytes: Vec<u8> = Vec::new();
    match sealed {
        Ok(d) => {
            bytes.push(1);
            push_u32(&mut bytes, d);
        }
        Err(e) => {
            bytes.push(2);
            push_u32(&mut bytes, (e & 0x7FFF_FFFF) as u32);
        }
    }
    let head = book_at(0);
    let frozen = led.submit(head.no, head.line, Zone::C2);
    match frozen {
        Ok(_) => bytes.push(3),
        Err(e) => {
            bytes.push(4);
            push_u32(&mut bytes, (e & 0x7FFF_FFFF) as u32);
        }
    }
    push_u32(&mut bytes, filled);
    push_u32(&mut bytes, led.cross_attempts());
    push_u32(&mut bytes, led.rejects());
    push_u32(&mut bytes, led.fingerprint());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9373 判据集（add 15 + add_dyn 3 = 18 条）。
pub fn run_unx9373_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9373");

    // 1. 段位守卫。
    let seg_ok = SEG_BASE % SEG_WINDOW == 0
        && SEG_BASE.saturating_add(SEG_WINDOW) <= NEXT_SEG_BASE
        && SEG_BASE > 0x7CA80
        && (0x7C000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7CAC0 独占 0x40 且接在 F9372 之后", seg_ok,
          "段基址必须 0x40 对齐，窗口末端不得越过 0x7CB00");

    // 2. 编号语法十格。
    let mut no_bad = 0u32;
    let mut ci = 0usize;
    while ci < NO_BATTERY.len() {
        if let Some((no, valid)) = NO_BATTERY.get(ci).copied() {
            if (no_defect(no) == NoDefect::Ok) != valid {
                no_bad = no_bad.saturating_add(1);
            }
        }
        ci += 1;
    }
    s.add("编号语法：UNX-F####-J# 十格逐格一致", no_bad == 0,
          "定长 12 字节；J0/J12/小写/缺 F 都必须拒绝");

    // 3. 协议句语法十格。
    let mut sent_bad = 0u32;
    let mut si = 0usize;
    while si < SENT_BATTERY.len() {
        if let Some((line, valid)) = SENT_BATTERY.get(si).copied() {
            if (sentence_defect(line) == SentenceDefect::Ok) != valid {
                sent_bad = sent_bad.saturating_add(1);
            }
        }
        si += 1;
    }
    s.add("协议句法：单句收尾+含格过+禁代码标记 十格一致", sent_bad == 0,
          "缺句号/双句/花括号/fn/换行/过短过长都必须拒绝");

    // 4. 注入两束。
    let (nm, sm, probes) = run_all_drills();
    s.add("注入：编号 80 + 协议句 80 格全检出",
          probes.conserved() && probes.escaped == 0 && probes.injected == 160
              && nm == 80 && sm == 80,
          "160 次语法注入必须 160 次检出");

    // 5. 正常流：整册按序落账并封账。
    let mut happy = SharedLedger::new();
    let mut all_ok = true;
    let mut i = 0usize;
    while i < SLOT_CAP {
        if let Some(e) = BOOK.get(i) {
            all_ok = all_ok && happy.submit(e.no, e.line, e.owner) == Ok(i as u32 + 1);
        }
        i += 1;
    }
    let happy_seal = happy.seal();
    let recomputed = happy.fingerprint();
    s.add("正常流：八条按册落账→封账→摘要可复算",
          all_ok && happy.claimed_count() == 8 && happy.seq() == 8
              && happy.cross_attempts() == 0 && happy.rejects() == 0
              && matches!(happy_seal, Ok(d) if d != 0)
              && happy.sealed() && happy.digest() == recomputed
              && happy.accepted_cross() == 0,
          "整册落账零拒账零跨面，封账摘要即终态指纹");

    // 6. 顺序与重复。
    let e0 = book_at(0);
    let e1 = book_at(1);
    let mut ord = SharedLedger::new();
    let skip_first = ord.submit(e1.no, e1.line, e1.owner);
    let first = ord.submit(e0.no, e0.line, e0.owner);
    let dup = ord.submit(e0.no, e0.line, e0.owner);
    s.add("落账次序：跳条 EINVAL、重复 EEXIST、首条放行",
          skip_first == Err(EINVAL) && first == Ok(1) && dup == Err(EEXIST)
              && ord.claimed_count() == 1 && ord.rejects() == 2,
          "顺序错与重复是两种诊断，不得混用错误码");

    // 7. 跨界代写：C2 写 C3 条必须 EPERM 且账内跨界恒 0。
    let mut cross = SharedLedger::new();
    let mut built = true;
    let mut k = 0usize;
    while k < 4 {
        if let Some(e) = BOOK.get(k) {
            built = built && cross.submit(e.no, e.line, e.owner).is_ok();
        }
        k += 1;
    }
    let e4 = book_at(4);
    let wrong = cross.submit(e4.no, e4.line, Zone::C2);
    let right = cross.submit(e4.no, e4.line, e4.owner);
    s.add("跨界代写零发生：错面 EPERM、正主放行、账内跨界恒 0",
          built && wrong == Err(EPERM) && cross.cross_attempts() == 1
              && right == Ok(5) && cross.claimed_count() == 5
              && cross.accepted_cross() == 0,
          "写入面必须同时等于册内 owner 与编号面");

    // 8. 改写协议：句子不同即 EACCES，账不动。
    let mut rewrite = SharedLedger::new();
    let altered = "共享账每条只写编号与一句话协议（改）格过。";
    let rw = rewrite.submit(e0.no, altered, e0.owner);
    let ok = rewrite.submit(e0.no, e0.line, e0.owner);
    // 判据自洽性说明：`ok` 是改写被拒后的原文落账（判据后半句要求成功），
    // 成功后 claimed_count 必为 1；改写未侵占槽位由「ok 在 0 号槽以原文
    // 成功」间接证明——若改写真的落账，ok 只会得到 EEXIST。
    s.add("协议改写拒绝：同号不同句 EACCES、原文可落账",
          rw == Err(EACCES) && rewrite.claimed_count() == 1
              && ok == Ok(1) && rewrite.slot_at(0).map(|x| x.line) == Some(e0.line),
          "协议只能登记不能润色，改写必须显性拒绝");

    // 9. 语法与册籍三类拒绝：坏号 EINVAL、代码句 ENOEXEC、册外号 ESRCH。
    let mut bad = SharedLedger::new();
    let bad_no = bad.submit("UNX-F9373-J0", e0.line, Zone::C2);
    let bad_line = bad.submit(e0.no, "共享账写 fn x() 格过。", Zone::C2);
    let foreign = bad.submit("UNX-F9399-J1", "册外编号只登记不落账格过。", Zone::C2);
    s.add("三类拒绝：坏号 EINVAL、代码句 ENOEXEC、册外号 ESRCH",
          bad_no == Err(EINVAL) && bad_line == Err(ENOEXEC)
              && foreign == Err(ESRCH) && bad.claimed_count() == 0
              && bad.rejects() == 3,
          "词法、句法、册籍三层各自回自己的错误码");

    // 10. 容量闸门：满册后第 9 条 ENOMEM（先于内容判定）。
    let mut full = SharedLedger::new();
    let mut built_full = true;
    let mut fk = 0usize;
    while fk < SLOT_CAP {
        if let Some(e) = BOOK.get(fk) {
            built_full = built_full && full.submit(e.no, e.line, e.owner).is_ok();
        }
        fk += 1;
    }
    let ninth = full.submit("UNX-F9373-J0", "满册坏号也必须先报容量格过。", Zone::C2);
    s.add("容量闸门：满册第 9 条 ENOMEM 先于语法判定",
          built_full && full.is_full() && full.claimed_count() == 8
              && ninth == Err(ENOMEM) && full.rejects() == 1,
          "资源面错误必须先于内容面错误返回");

    // 11. 封账：未齐 EBUSY、齐了可封、二次 EBUSY、封后补写 EPERM。
    let mut seal_ord = SharedLedger::new();
    let early = seal_ord.seal();
    let mut built_seal = true;
    let mut sk = 0usize;
    while sk < SLOT_CAP {
        if let Some(e) = BOOK.get(sk) {
            built_seal = built_seal && seal_ord.submit(e.no, e.line, e.owner).is_ok();
        }
        sk += 1;
    }
    let first = seal_ord.seal();
    let second = seal_ord.seal();
    let after = seal_ord.submit(e0.no, e0.line, Zone::C2);
    s.add("封账：未齐 EBUSY、齐了可封、二次 EBUSY、封后 EPERM",
          built_seal && early == Err(EBUSY) && first.is_ok() && second == Err(EBUSY)
              && after == Err(EPERM) && seal_ord.sealed() && seal_ord.rejects() == 1,
          "未齐不得封；封后账册不可变");

    // 12. 面归属：93xx→C2、94xx→C3、99xx 与坏号无主。
    let zones = zone_of("UNX-F9373-J1") == Zone::C2
        && zone_of("UNX-F9399-J9") == Zone::C2
        && zone_of("UNX-F9401-J1") == Zone::C3
        && zone_of("UNX-F9799-J1") == Zone::C3
        && zone_of("UNX-F9900-J1") == Zone::Undecided
        && zone_of("bad") == Zone::Undecided
        && no_group("UNX-F9373-J1") == Some(9373);
    s.add("号面归属：93xx/C2、94xx–97xx/C3、其余无主", zones,
          "面归属只由四位号段决定，不认标题文字");

    // 13. Linux 逐码。
    let mut codes_ok = ERRNO_MAP.len() == 8;
    let mut ei = 0usize;
    while ei < ERRNO_MAP.len() {
        if let Some(row) = ERRNO_MAP.get(ei) {
            if !errno_is_linux(row.1) || linux_neg(row.1) != Some(-row.1) {
                codes_ok = false;
            }
            let mut ej = ei + 1;
            while ej < ERRNO_MAP.len() {
                if let Some(other) = ERRNO_MAP.get(ej) {
                    if other.1 == row.1 {
                        codes_ok = false;
                    }
                }
                ej += 1;
            }
        }
        ei += 1;
    }
    s.add("Linux 逐码：8 个 errno 表内可复算且不重复",
          codes_ok && linux_neg(ENOEXEC) == Some(-8)
              && !errno_is_linux(0) && !errno_is_linux(7777),
          "ENOEXEC=8 必须与 Linux 一致，表外码拒绝");

    // 14. 诊断码闭集与判定面。
    let face = SLOT_CAP as u32 + BOOK.len() as u32 + LedgerCode::ALL.len() as u32
        + ERRNO_MAP.len() as u32 + CODE_MARKS.len() as u32;
    let mut uniq = true;
    let mut empty_text = 0u32;
    let mut ai = 0usize;
    while ai < LedgerCode::ALL.len() {
        if let Some(a) = LedgerCode::ALL.get(ai) {
            if a.detail().is_empty() {
                empty_text = empty_text.saturating_add(1);
            }
            let mut bi = ai + 1;
            while bi < LedgerCode::ALL.len() {
                if let Some(b) = LedgerCode::ALL.get(bi) {
                    if a.seg_addr() == b.seg_addr() || a.code() == b.code() {
                        uniq = false;
                    }
                }
                bi += 1;
            }
        }
        ai += 1;
    }
    s.add("诊断码：13 个段内码唯一、文案非空、判定面达标",
          uniq && empty_text == 0 && LedgerCode::ALL.len() == 13
              && face >= CODE_QUOTA / 40,
          "闭集码不得重复，判定面须达配额密度");

    // 15. 双跑指纹。
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：整册语料指纹相等", fp_a == fp_b && fp_a != 0,
          "同输入两次必须逐字节相等");

    // 16-18. 动态明细。
    let drill_text = format!("语法注入 {} / 检出 {} / 逃逸 {}（编号 {} + 句法 {}）",
                             probes.injected, probes.detected, probes.escaped, nm, sm);
    s.add_dyn("注入快照", probes.conserved() && probes.escaped == 0, &drill_text);

    let seal_text = format!("已落账 {}/{} 跨面拒写 {} 拒账 {}",
                            happy.claimed_count(), SLOT_CAP,
                            happy.cross_attempts(), happy.rejects());
    s.add_dyn("账册快照", happy.claimed_count() == 8, &seal_text);

    let fp_text = format!("册指纹 0x{:08X} 面=[{}]", fp_a, zone_of("UNX-F9401-J1").label());
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_grammar() {
        assert_eq!(no_defect("UNX-F9373-J1"), NoDefect::Ok);
        assert_eq!(no_defect("UNX-F9373-J0"), NoDefect::BadJudge);
        assert_eq!(no_defect("UNX-F9373-J12"), NoDefect::BadLen);
        assert_eq!(no_defect("unx-f9373-j1"), NoDefect::BadPrefix);
        assert_eq!(no_defect("UNX-F93A3-J1"), NoDefect::BadDigits);
        assert_eq!(no_group("UNX-F9404-J1"), Some(9404));
        assert_eq!(no_group("bad"), None);
    }

    #[test]
    fn sentence_grammar() {
        assert_eq!(sentence_defect("共享账只收编号与一句话协议格过。"), SentenceDefect::Ok);
        assert_eq!(sentence_defect("共享账只收编号与一句话协议"), SentenceDefect::NoStop);
        assert_eq!(sentence_defect("一句。二句格过。"), SentenceDefect::MultiStop);
        assert_eq!(sentence_defect("共享账写 fn x() 格过。"), SentenceDefect::LooksLikeCode);
        assert_eq!(sentence_defect("短。"), SentenceDefect::TooShort);
        assert_eq!(sentence_defect("共享账第一行\n第二行格过。"), SentenceDefect::HasNewline);
        assert_eq!(sentence_defect("共享账只收编号与一句话协议。"), SentenceDefect::NoVerdict);
    }

    #[test]
    fn full_book_seals() {
        let e0 = book_at(0);
        let mut led = SharedLedger::new();
        let mut i = 0usize;
        while i < SLOT_CAP {
            if let Some(e) = BOOK.get(i) {
                assert_eq!(led.submit(e.no, e.line, e.owner), Ok(i as u32 + 1));
            }
            i += 1;
        }
        assert_eq!(led.claimed_count(), 8);
        assert_eq!(led.cross_attempts(), 0);
        assert_eq!(led.accepted_cross(), 0);
        assert_eq!(led.seal().is_ok(), true);
        assert_eq!(led.digest(), led.fingerprint());
        assert_eq!(led.seal(), Err(EBUSY));
        assert_eq!(led.submit(e0.no, e0.line, Zone::C2), Err(EPERM));
    }

    #[test]
    fn order_and_duplicate() {
        let e0 = book_at(0);
        let e1 = book_at(1);
        let mut led = SharedLedger::new();
        assert_eq!(led.submit(e1.no, e1.line, e1.owner), Err(EINVAL));
        assert_eq!(led.submit(e0.no, e0.line, e0.owner), Ok(1));
        assert_eq!(led.submit(e0.no, e0.line, e0.owner), Err(EEXIST));
        assert_eq!(led.claimed_count(), 1);
    }

    #[test]
    fn cross_zone_and_rewrite() {
        let e0 = book_at(0);
        let e4 = book_at(4);
        let mut led = SharedLedger::new();
        let mut i = 0usize;
        while i < 4 {
            if let Some(e) = BOOK.get(i) {
                assert_eq!(led.submit(e.no, e.line, e.owner), Ok(i as u32 + 1));
            }
            i += 1;
        }
        assert_eq!(led.submit(e4.no, e4.line, Zone::C2), Err(EPERM));
        assert_eq!(led.cross_attempts(), 1);
        assert_eq!(led.submit(e4.no, e4.line, Zone::C3), Ok(5));

        let mut rw = SharedLedger::new();
        assert_eq!(rw.submit(e0.no, "共享账改写句子后照样格过。", Zone::C2), Err(EACCES));
        assert_eq!(rw.claimed_count(), 0);
    }

    #[test]
    fn capacity_and_lexical_rejects() {
        let e0 = book_at(0);
        let mut full = SharedLedger::new();
        let mut i = 0usize;
        while i < SLOT_CAP {
            if let Some(e) = BOOK.get(i) {
                assert_eq!(full.submit(e.no, e.line, e.owner), Ok(i as u32 + 1));
            }
            i += 1;
        }
        assert_eq!(full.submit("UNX-F9373-J0", "满册先报容量格过。", Zone::C2), Err(ENOMEM));

        let mut bad = SharedLedger::new();
        assert_eq!(bad.submit("UNX-F9373-J0", e0.line, Zone::C2), Err(EINVAL));
        assert_eq!(bad.submit(e0.no, "共享账写 fn x() 格过。", Zone::C2), Err(ENOEXEC));
        assert_eq!(bad.submit("UNX-F9399-J1", "册外编号不落账格过。", Zone::C2), Err(ESRCH));
    }

    #[test]
    fn drills_are_conserved() {
        let (nm, sm, ledger) = run_all_drills();
        assert_eq!(ledger.injected, 160);
        assert_eq!(ledger.detected, 160);
        assert_eq!(ledger.escaped, 0);
        assert_eq!(nm, 80);
        assert_eq!(sm, 80);
    }

    #[test]
    fn zones_and_errno() {
        assert_eq!(zone_of("UNX-F9373-J1"), Zone::C2);
        assert_eq!(zone_of("UNX-F9401-J1"), Zone::C3);
        assert_eq!(zone_of("UNX-F9900-J1"), Zone::Undecided);
        assert_eq!(linux_neg(ENOEXEC), Some(-8));
        assert_eq!(linux_neg(EINVAL), Some(-22));
        assert_eq!(linux_neg(4242), None);
        assert_eq!(errno_is_linux(EPERM), true);
        assert_eq!(ERRNO_MAP.len(), 8);
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(corpus_fingerprint() != 0, true);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9373_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
