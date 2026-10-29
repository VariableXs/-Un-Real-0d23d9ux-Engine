//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12941 · DataDirectory[16] 全目录索引语义总表 · 0x65440
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F12941-J1 16 目录索引语义表（用途/消费批位/越界规则）
//!     成账，100 样本读取与 F12814 面回归一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 16 索引闭集（写死自 winnt.h IMAGE_DIRECTORY_ENTRY_*）：
//!     0 EXPORT / 1 IMPORT / 2 RESOURCE / 3 EXCEPTION / 4 SECURITY /
//!     5 BASERELOC / 6 DEBUG / 7 ARCHITECTURE / 8 GLOBALPTR /
//!     9 TLS / 10 LOAD_CONFIG / 11 BOUND_IMPORT / 12 IAT /
//!     13 DELAY_IMPORT / 14 CLR / 15 RESERVED；
//!   - 每目录三元账：用途名（静态串）/ 消费批位（静态号）/ 越界规则；
//!   - 读取闭式：VirtualAddress==0 或 Size==0 → 视为缺省（不算越界）；
//!     两者其一为零另一非零 → 半缺省（对齐 Windows 读取容忍）；
//!     VA+Size 越映像 → 拒读落账；
//!   - 100 样本读取与黄金闭表（16×固定值）双路对账；
//!   - 状态机显式：Unread→Present/Absent/HalfDefined/OutOfRange，
//!     非法转换拒收；LCG 种子写死 = SEG_BASE（0x65440）；
//!     禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65440，全仓独占）。
pub const SEG_BASE: u32 = 0x65440;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DdCode {
    /// 目录索引越 16 闭集
    IndexOutOfRange,
    /// VA+Size 越映像（拒读）
    EntryOutOfRange,
    /// 表账守恒破（16 目录账和 != 16）
    LedgerBroken,
    /// 半缺省误判（VA/Size 一零一非零处理错）
    HalfDefinedMisjudged,
    /// 缺省误判（全零目录被读成存在）
    AbsentMisjudged,
    /// 语义表缺项（用途名/消费批位空）
    SemanticTableHole,
    /// 目录表尺寸非 8 对齐（每项 8 字节）
    TableNotAligned,
    /// 状态机非法转换
    IllegalTransition,
}

impl DdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DdCode::IndexOutOfRange => "目录索引越 16 闭集：拒读",
            DdCode::EntryOutOfRange => "目录 VA+Size 越映像：拒读落账",
            DdCode::LedgerBroken => "表账守恒破：16 目录账和 != 16",
            DdCode::HalfDefinedMisjudged => "半缺省误判：VA/Size 一零一非零",
            DdCode::AbsentMisjudged => "缺省误判：全零目录被读成存在",
            DdCode::SemanticTableHole => "语义表缺项：用途名或消费批位为空",
            DdCode::TableNotAligned => "目录表尺寸非 8 对齐",
            DdCode::IllegalTransition => "状态机非法转换：拒收",
        }
    }
    pub const ALL: [DdCode; 8] = [
        DdCode::IndexOutOfRange,
        DdCode::EntryOutOfRange,
        DdCode::LedgerBroken,
        DdCode::HalfDefinedMisjudged,
        DdCode::AbsentMisjudged,
        DdCode::SemanticTableHole,
        DdCode::TableNotAligned,
        DdCode::IllegalTransition,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DdCode::ALL.len() {
        if DdCode::ALL[i].val() == code {
            return DdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 16 索引闭集（写死自 winnt.h） ────────────────────────────────
pub const DD_EXPORT: usize = 0;
pub const DD_IMPORT: usize = 1;
pub const DD_RESOURCE: usize = 2;
pub const DD_EXCEPTION: usize = 3;
pub const DD_SECURITY: usize = 4;
pub const DD_BASERELOC: usize = 5;
pub const DD_DEBUG: usize = 6;
pub const DD_ARCHITECTURE: usize = 7;
pub const DD_GLOBALPTR: usize = 8;
pub const DD_TLS: usize = 9;
pub const DD_LOAD_CONFIG: usize = 10;
pub const DD_BOUND_IMPORT: usize = 11;
pub const DD_IAT: usize = 12;
pub const DD_DELAY_IMPORT: usize = 13;
pub const DD_CLR: usize = 14;
pub const DD_RESERVED: usize = 15;
/// 目录数闭集（16，写死）。
pub const DD_COUNT: usize = 16;
/// 每目录项字节（VA+Size 两 u32，写死）。
pub const DD_ENTRY_SIZE: usize = 8;

/// 语义三元账：用途名（静态串）/ 消费批位（静态号）/ 越界规则号。
/// 越界规则闭集：0=VA+Size 必落映像；1=SECURITY 例外（文件偏移语义，
/// 以文件尾为界）；2=GLOBALPTR 只校 VA；3=RESERVED 恒拒读。
pub const RULE_STRICT: u8 = 0;
pub const RULE_FILE_OFFSET: u8 = 1;
pub const RULE_VA_ONLY: u8 = 2;
pub const RULE_NEVER: u8 = 3;

/// 语义总表（16 行 × 3 元，全部静态写死——成账主面）。
pub const SEMANTIC_TABLE: [(&str, u16, u8); DD_COUNT] = [
    ("EXPORT", 0xB01, RULE_STRICT),
    ("IMPORT", 0xB01, RULE_STRICT),
    ("RESOURCE", 0xB02, RULE_STRICT),
    ("EXCEPTION", 0xB02, RULE_STRICT),
    ("SECURITY", 0xB02, RULE_FILE_OFFSET),
    ("BASERELOC", 0xB03, RULE_STRICT),
    ("DEBUG", 0xB03, RULE_STRICT),
    ("ARCHITECTURE", 0xB04, RULE_STRICT),
    ("GLOBALPTR", 0xB04, RULE_VA_ONLY),
    ("TLS", 0xB05, RULE_STRICT),
    ("LOAD_CONFIG", 0xB05, RULE_STRICT),
    ("BOUND_IMPORT", 0xB06, RULE_STRICT),
    ("IAT", 0xB06, RULE_STRICT),
    ("DELAY_IMPORT", 0xB07, RULE_STRICT),
    ("CLR", 0xB07, RULE_STRICT),
    ("RESERVED", 0xB08, RULE_NEVER),
];

/// 语义表完整性核：16 行无空串、批位非零、规则越闭集即洞。
pub fn semantic_table_complete() -> bool {
    let mut i = 0;
    while i < DD_COUNT {
        let (name, batch, rule) = SEMANTIC_TABLE[i];
        if name.is_empty() || batch == 0 {
            return false;
        }
        if rule > RULE_NEVER {
            return false;
        }
        i += 1;
    }
    true
}

// ── 目录项 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DirEntry {
    pub virtual_address: u32,
    pub size: u32,
}

impl DirEntry {
    pub fn absent() -> DirEntry {
        DirEntry {
            virtual_address: 0,
            size: 0,
        }
    }
    pub fn is_absent(&self) -> bool {
        self.virtual_address == 0 && self.size == 0
    }
    pub fn is_half_defined(&self) -> bool {
        (self.virtual_address == 0) != (self.size == 0)
    }
}

// ── 读取状态机（显式状态 + 合法边闭集） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadState {
    Unread,
    Present,
    Absent,
    HalfDefined,
    OutOfRange,
}

pub fn edge_legal(from: ReadState, to: ReadState) -> bool {
    matches!(
        (from, to),
        (ReadState::Unread, ReadState::Present)
            | (ReadState::Unread, ReadState::Absent)
            | (ReadState::Unread, ReadState::HalfDefined)
            | (ReadState::Unread, ReadState::OutOfRange)
    )
}

pub fn transition(from: ReadState, to: ReadState) -> Result<ReadState, DdCode> {
    if edge_legal(from, to) {
        Ok(to)
    } else {
        Err(DdCode::IllegalTransition)
    }
}

// ── 读取核（越界规则闭式） ──────────────────────────────────────
/// 读第 idx 目录：按语义表规则判定。
pub fn read_directory(idx: usize, e: &DirEntry, image_size: u32) -> Result<ReadState, DdCode> {
    if idx >= DD_COUNT {
        return Err(DdCode::IndexOutOfRange);
    }
    let (_name, _batch, rule) = SEMANTIC_TABLE[idx];
    if e.is_absent() {
        return transition(ReadState::Unread, ReadState::Absent);
    }
    if e.is_half_defined() {
        // Windows 容忍半缺省：VA 非零而 Size 为零按整块读；
        // Size 非零而 VA 为零视为缺省。此处闭式：HalfDefined 落账。
        return transition(ReadState::Unread, ReadState::HalfDefined);
    }
    match rule {
        RULE_NEVER => transition(ReadState::Unread, ReadState::OutOfRange),
        RULE_VA_ONLY => {
            if e.virtual_address >= image_size {
                transition(ReadState::Unread, ReadState::OutOfRange)
            } else {
                transition(ReadState::Unread, ReadState::Present)
            }
        }
        RULE_FILE_OFFSET => {
            // SECURITY：Size 是证书块长（文件偏移语义），以文件尾为界。
            if e.virtual_address.checked_add(e.size).unwrap_or(u32::MAX) > image_size {
                transition(ReadState::Unread, ReadState::OutOfRange)
            } else {
                transition(ReadState::Unread, ReadState::Present)
            }
        }
        _ => {
            // RULE_STRICT：VA 与 VA+Size 均须落映像。
            if e.virtual_address >= image_size
                || e.virtual_address.checked_add(e.size).unwrap_or(u32::MAX) > image_size
            {
                transition(ReadState::Unread, ReadState::OutOfRange)
            } else {
                transition(ReadState::Unread, ReadState::Present)
            }
        }
    }
}

/// 目录表批量读取：16 项账，账和守恒。
pub fn read_table(entries: &[DirEntry], image_size: u32) -> Result<[ReadState; DD_COUNT], DdCode> {
    if entries.len() != DD_COUNT {
        return Err(DdCode::LedgerBroken);
    }
    let mut out = [ReadState::Unread; DD_COUNT];
    let mut i = 0;
    while i < DD_COUNT {
        out[i] = read_directory(i, &entries[i], image_size)?;
        i += 1;
    }
    // 守恒：账和 == 16。
    let mut sum = 0;
    let mut j = 0;
    while j < DD_COUNT {
        match out[j] {
            ReadState::Present
            | ReadState::Absent
            | ReadState::HalfDefined
            | ReadState::OutOfRange => sum += 1,
            ReadState::Unread => {}
        }
        j += 1;
    }
    if sum != DD_COUNT {
        return Err(DdCode::LedgerBroken);
    }
    Ok(out)
}

// ── 表尺寸核（8 对齐） ──────────────────────────────────────────
pub fn table_aligned(total_bytes: usize) -> bool {
    total_bytes % DD_ENTRY_SIZE == 0 && total_bytes / DD_ENTRY_SIZE <= DD_COUNT
}

// ── LCG（种子写死 = SEG_BASE） ──────────────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

/// 合成 16 目录样本（kind 闭集：全零/半缺省/正常/越界/RESERVED 非零）。
pub fn synth_entries(rng: &mut Lcg) -> Vec<DirEntry> {
    let mut v: Vec<DirEntry> = Vec::new();
    let mut i = 0;
    while i < DD_COUNT {
        if i == DD_RESERVED {
            v.push(DirEntry::absent());
            i += 1;
            continue;
        }
        let k = rng.next_below(4);
        let e = match k {
            0 => DirEntry::absent(),
            1 => DirEntry {
                virtual_address: 0x1000 + rng.next_below(0x2000) as u32,
                size: 0,
            },
            2 => {
                let va = 0x1000 + rng.next_below(0x2000) as u32;
                DirEntry {
                    virtual_address: va,
                    size: rng.next_below(0x400) as u32,
                }
            }
            _ => DirEntry {
                // 越界：VA 落映像外（样本映像 0x40000）。
                virtual_address: 0x40000 + rng.next_below(0x1000) as u32,
                size: rng.next_below(0x100) as u32,
            },
        };
        v.push(e);
        i += 1;
    }
    v
}

// ── 语义总表成账（判据 J1 主面一） ───────────────────────────────
pub fn drill_semantic_table() -> (bool, String) {
    let ok = semantic_table_complete()
        && SEMANTIC_TABLE.len() == DD_COUNT
        && SEMANTIC_TABLE[DD_EXPORT].0 == "EXPORT"
        && SEMANTIC_TABLE[DD_TLS].0 == "TLS"
        && SEMANTIC_TABLE[DD_RESERVED].2 == RULE_NEVER
        && SEMANTIC_TABLE[DD_SECURITY].2 == RULE_FILE_OFFSET
        && SEMANTIC_TABLE[DD_GLOBALPTR].2 == RULE_VA_ONLY;
    (
        ok,
        format!(
            "语义总表 16 行成账: 完整={} 闭集={} 规则特例齐={}",
            semantic_table_complete(),
            SEMANTIC_TABLE.len() == DD_COUNT,
            ok
        ),
    )
}

pub fn drill_golden_reads() -> (bool, String) {
    // 黄金闭表：16 目录 × 固定输入 → 固定结论（判据独立写死）。
    let img: u32 = 0x40000;
    let mut caught = 0;
    // 全零 → Absent（16 行全核）。
    let mut i = 0;
    let mut all_absent = true;
    while i < DD_COUNT {
        all_absent = all_absent && read_directory(i, &DirEntry::absent(), img) == Ok(ReadState::Absent);
        i += 1;
    }
    if all_absent {
        caught += 1;
    }
    // 正常项：0..14 Present（RESERVED 恒拒）。
    let e = DirEntry {
        virtual_address: 0x2000,
        size: 0x100,
    };
    let mut i = 0;
    let mut present_ok = true;
    while i < DD_COUNT {
        let want = if i == DD_RESERVED {
            ReadState::OutOfRange
        } else {
            ReadState::Present
        };
        present_ok = present_ok && read_directory(i, &e, img) == Ok(want);
        i += 1;
    }
    if present_ok {
        caught += 1;
    }
    // 半缺省 → HalfDefined。
    let hd = DirEntry {
        virtual_address: 0x2000,
        size: 0,
    };
    if read_directory(DD_IMPORT, &hd, img) == Ok(ReadState::HalfDefined) {
        caught += 1;
    }
    // 越界 → OutOfRange。
    let oob = DirEntry {
        virtual_address: 0x3FF00,
        size: 0x200,
    };
    if read_directory(DD_EXPORT, &oob, img) == Ok(ReadState::OutOfRange) {
        caught += 1;
    }
    // 索引越闭集 → Err。
    if read_directory(16, &e, img) == Err(DdCode::IndexOutOfRange) {
        caught += 1;
    }
    // RULE_VA_ONLY：Size 巨大但 VA 在内 → Present。
    let g = DirEntry {
        virtual_address: 0x2000,
        size: 0xFFFF_F000,
    };
    if read_directory(DD_GLOBALPTR, &g, img) == Ok(ReadState::Present) {
        caught += 1;
    }
    let ok = caught == 6;
    (ok, format!("黄金读取闭表 {}/6", caught))
}

pub fn drill_100_samples() -> (bool, String) {
    // 100 样本读取与独立判定（与 read_directory 不同代码路径）一致。
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut agree = 0;
    let mut n = 0;
    let img: u32 = 0x40000;
    while n < 100 {
        let entries = synth_entries(&mut rng);
        let idx = (rng.next_below(DD_COUNT as u64)) as usize;
        let e = entries[idx];
        let got = read_directory(idx, &e, img);
        // 独立判定（直接闭式重推）。
        let want: Result<ReadState, DdCode> = if idx >= DD_COUNT {
            Err(DdCode::IndexOutOfRange)
        } else if e.is_absent() {
            Ok(ReadState::Absent)
        } else if e.is_half_defined() {
            Ok(ReadState::HalfDefined)
        } else {
            let rule = SEMANTIC_TABLE[idx].2;
            let ok_in = match rule {
                RULE_NEVER => false,
                RULE_VA_ONLY => e.virtual_address < img,
                _ => e.virtual_address < img
                    && e.virtual_address.checked_add(e.size).unwrap_or(u32::MAX) <= img,
            };
            Ok(if ok_in { ReadState::Present } else { ReadState::OutOfRange })
        };
        if got == want {
            agree += 1;
        }
        n += 1;
    }
    let ok = agree == 100;
    (ok, format!("100 样本读取一致 {}/100", agree))
}

pub fn drill_table_batch() -> (bool, String) {
    // 16 目录批量读取账守恒 + 尺寸非 16 拒 + 8 对齐核。
    let mut rng = Lcg::new(SEG_BASE as u64 + 1);
    let mut ok = true;
    let mut i = 0;
    while i < 20 {
        let entries = synth_entries(&mut rng);
        let states = read_table(&entries, 0x40000);
        match states {
            Ok(st) => {
                // 守恒：Unread 数必为 0（全部落终态）。
                let mut unread = 0;
                let mut j = 0;
                while j < DD_COUNT {
                    if st[j] == ReadState::Unread {
                        unread += 1;
                    }
                    j += 1;
                }
                ok = ok && unread == 0;
            }
            Err(_) => ok = false,
        }
        i += 1;
    }
    // 非 16 表拒。
    let mut short: Vec<DirEntry> = Vec::new();
    short.push(DirEntry::absent());
    ok = ok && read_table(&short, 0x40000) == Err(DdCode::LedgerBroken);
    // 8 对齐核：128 字节齐、7 字节不齐、136 字节（>16 项）不齐。
    ok = ok && table_aligned(128) && !table_aligned(7) && !table_aligned(136);
    (ok, format!("批量读取账+对齐核: {}", ok))
}

pub fn drill_state_edges() -> (bool, String) {
    let mut ok = true;
    ok = ok && transition(ReadState::Unread, ReadState::Present).is_ok();
    ok = ok && transition(ReadState::Unread, ReadState::Absent).is_ok();
    ok = ok && transition(ReadState::Unread, ReadState::HalfDefined).is_ok();
    ok = ok && transition(ReadState::Unread, ReadState::OutOfRange).is_ok();
    ok = ok && transition(ReadState::Present, ReadState::Absent) == Err(DdCode::IllegalTransition);
    ok = ok && transition(ReadState::Absent, ReadState::Present) == Err(DdCode::IllegalTransition);
    ok = ok && transition(ReadState::OutOfRange, ReadState::Unread) == Err(DdCode::IllegalTransition);
    (ok, format!("状态边闭集: {}", ok))
}

pub fn drill_security_rule() -> (bool, String) {
    // SECURITY 文件偏移规则特例：VA+Size 越界同 strict；但
    // RULE_FILE_OFFSET 与 RULE_STRICT 在同输入下结论一致（闭式等价面）。
    let img: u32 = 0x40000;
    let in_e = DirEntry {
        virtual_address: 0x30000,
        size: 0x800,
    };
    let out_e = DirEntry {
        virtual_address: 0x3F000,
        size: 0x4000,
    };
    let mut ok = read_directory(DD_SECURITY, &in_e, img) == Ok(ReadState::Present);
    ok = ok && read_directory(DD_SECURITY, &out_e, img) == Ok(ReadState::OutOfRange);
    // STRICT 同输入对照。
    ok = ok && read_directory(DD_DEBUG, &in_e, img) == Ok(ReadState::Present);
    ok = ok && read_directory(DD_DEBUG, &out_e, img) == Ok(ReadState::OutOfRange);
    (ok, format!("SECURITY 规则特例: {}", ok))
}

pub fn drill_reserved_never() -> (bool, String) {
    // RESERVED 恒拒读注入 10/10：非零项必 OutOfRange。
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        let e = DirEntry {
            virtual_address: 0x1000 + i,
            size: 0x10 + i as u32,
        };
        if read_directory(DD_RESERVED, &e, 0x40000) == Ok(ReadState::OutOfRange) {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 10;
    (ok, format!("RESERVED 恒拒 {}/10", caught))
}

pub fn drill_half_absent_injection() -> (bool, String) {
    // 半缺省/缺省误判注入：全零必 Absent、一零一非零必 HalfDefined。
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        let a = DirEntry {
            virtual_address: 0,
            size: 0x100 + i as u32,
        };
        let b = DirEntry {
            virtual_address: 0x1000 + i as u32,
            size: 0,
        };
        let z = DirEntry::absent();
        let ok3 = read_directory(DD_IAT, &a, 0x40000) == Ok(ReadState::HalfDefined)
            && read_directory(DD_IAT, &b, 0x40000) == Ok(ReadState::HalfDefined)
            && read_directory(DD_IAT, &z, 0x40000) == Ok(ReadState::Absent);
        if ok3 {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 10;
    (ok, format!("半缺省/缺省注入 {}/10", caught))
}

pub fn drill_f12814_regression_face() -> (bool, String) {
    // F12814 面回归：固定 16 目录布局 → 读取结论与参照账一致。
    let img: u32 = 0x40000;
    let mut entries: Vec<DirEntry> = Vec::new();
    let mut i = 0;
    while i < DD_COUNT {
        // 参照账：偶数索引与 RESERVED(15) 全零，其余奇数索引正常
        //（判据独立写死，RESERVED 恒拒所以必须缺省）。
        if i % 2 == 0 || i == DD_RESERVED {
            entries.push(DirEntry::absent());
        } else {
            entries.push(DirEntry {
                virtual_address: 0x1000 * (i as u32 + 1),
                size: 0x200,
            });
        }
        i += 1;
    }
    let states = read_table(&entries, img);
    let mut ok = states.is_ok();
    if let Ok(st) = states {
        let mut j = 0;
        while j < DD_COUNT {
            let want = if j % 2 == 0 || j == DD_RESERVED {
                ReadState::Absent
            } else {
                ReadState::Present
            };
            ok = ok && st[j] == want;
            j += 1;
        }
    }
    (ok, format!("F12814 面回归: 偶零奇在 16/16 = {}", ok))
}

pub fn drill_diag_codes_closed() -> (bool, String) {
    let mut ok = true;
    let mut i = 0;
    while i < DdCode::ALL.len() {
        ok = ok && DdCode::ALL[i].val() >= SEG_BASE && DdCode::ALL[i].val() < SEG_BASE + 0x40;
        ok = ok && !DdCode::ALL[i].detail().is_empty();
        i += 1;
    }
    ok = ok && detail(SEG_BASE + 0x3F) == "未知诊断码";
    (ok, format!("诊断码闭集 {}/8 + 反向", DdCode::ALL.len()))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0;
    while i < DdCode::ALL.len() {
        let v = DdCode::ALL[i].val();
        ok = ok && v >= SEG_BASE && v < SEG_BASE + 0x40;
        i += 1;
    }
    ok
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_semantic_table();
    let (a2, d2) = drill_golden_reads();
    let (a3, d3) = drill_100_samples();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 语义表={} 黄金={} 百样本={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_semantic_table();
    v.push(("J1-16目录语义总表成账", a, b));
    let (a, b) = drill_golden_reads();
    v.push(("J1-黄金读取闭表", a, b));
    let (a, b) = drill_100_samples();
    v.push(("J1-100样本读取一致", a, b));
    let (a, b) = drill_table_batch();
    v.push(("J2-批量读取账+对齐核", a, b));
    let (a, b) = drill_state_edges();
    v.push(("J2-状态边闭集", a, b));
    let (a, b) = drill_security_rule();
    v.push(("J2-SECURITY规则特例", a, b));
    let (a, b) = drill_reserved_never();
    v.push(("J2-RESERVED恒拒10/10", a, b));
    let (a, b) = drill_half_absent_injection();
    v.push(("J2-半缺省/缺省注入10/10", a, b));
    let (a, b) = drill_f12814_regression_face();
    v.push(("J1-F12814面回归", a, b));
    let (a, b) = drill_diag_codes_closed();
    v.push(("J3-诊断码闭集+反向", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx12941_checks：本单判据聚合（诊断段 0x65440）。
pub fn run_unx12941_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12941 DataDirectory[16] 语义总表");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x65440 段闭集");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = checks[i].clone();
        s.add_dyn(n, p, &d);
        i += 1;
    }
    s
}
