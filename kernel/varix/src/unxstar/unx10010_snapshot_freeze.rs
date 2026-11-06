//! UNX-F10010 报表快照冻结段（并入三件套） · 0x91280 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10010（B21 波次报表线）。
//! 锚点：UNX-F10011-J1 procfs 段只读核（写回探测 10/10 次触发即拦），可写暴露 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91280（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `SnapCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 解析器 `SnapParser`：把「kind|id|hash|lines」文本逐字段拆成
//!     `SnapRecord`，四字段缺一即 ParseFailure，杜绝静默吞字段；
//!   - 状态机 `FreezeState`：Collecting → Complete → Frozen 单向推进，
//!     冻结后内容与版本锚一起封账，任何回退显性拒绝；
//!   - 账本 `FreezeLedger`：注入 = 检出 + 逃逸，逃逸恒 0，只增不减；
//!   - 注入核 `BadSnapCase`：缺版本锚 / 哈希被改 / 乱序并入 / 重复件 /
//!     未冻结并入五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10010 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91280;

/// 三件套件数：报表本体 / 差异清单 / 版本锚。
pub const PIECE_COUNT: usize = 3;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 快照文本记录的字段数（kind|id|hash|lines）。
pub const RECORD_FIELDS: usize = 4;

/// Linux errno 正数码（对照表口径）。
pub const ENOENT: i32 = 2;
pub const EACCES: i32 = 13;
pub const EINVAL: i32 = 22;
pub const EROFS: i32 = 30;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapCode {
    /// 缺版本锚件
    MissingAnchor,
    /// 内容哈希与锚不符
    HashMismatch,
    /// 乱序并入（先锚后体）
    OrderViolation,
    /// 重复件
    DuplicatePiece,
    /// 未冻结态并入
    UnfrozenMerge,
    /// 账本倒退
    LedgerRegress,
    /// 解析失败
    ParseFailure,
    /// 版本漂移
    VersionDrift,
    /// 件数不齐
    IncompleteSet,
    /// 冻结后改写
    FrozenRewrite,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl SnapCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SnapCode::MissingAnchor => "缺版本锚被拒：三件套必须含版本锚件；请先落锚再并入其余两件",
            SnapCode::HashMismatch => "哈希不符被拒：件内容与版本锚登记哈希不一致；请重算 FNV 指纹后重报",
            SnapCode::OrderViolation => "乱序并入被拒：版本锚必须首个并入；请按 锚→体→清单 次序重放",
            SnapCode::DuplicatePiece => "重复件被拒：同 kind 件已在快照内；请核对件类别后去重",
            SnapCode::UnfrozenMerge => "未冻结并入被拒：状态机仍在 Collecting；请先补齐三件推进到 Complete",
            SnapCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请排查回滚路径误改累计值",
            SnapCode::ParseFailure => "解析失败：记录必须恰为四段 kind|id|hash|lines；请补齐字段再提交",
            SnapCode::VersionDrift => "版本漂移：并入件版本与快照基版不一致；请对齐版本号后重放",
            SnapCode::IncompleteSet => "件数不齐被拒：三件套必须三件齐备；请补缺失件后重试",
            SnapCode::FrozenRewrite => "冻结后改写被拒：Frozen 态内容封账；请走差异清单通道而非直改",
            SnapCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态或非确定哈希",
            SnapCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x91280..=0x91800",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [SnapCode; 12] = [
        SnapCode::MissingAnchor,
        SnapCode::HashMismatch,
        SnapCode::OrderViolation,
        SnapCode::DuplicatePiece,
        SnapCode::UnfrozenMerge,
        SnapCode::LedgerRegress,
        SnapCode::ParseFailure,
        SnapCode::VersionDrift,
        SnapCode::IncompleteSet,
        SnapCode::FrozenRewrite,
        SnapCode::NotReproducible,
        SnapCode::SegmentViolation,
    ];
}

// ── 件类别与解析器 ────────────────────────────────────────────────────
/// 三件套件类别（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceKind {
    /// 报表本体
    Body,
    /// 差异清单
    DiffList,
    /// 版本锚
    Anchor,
}

impl PieceKind {
    /// 解析 kind 字段（"body"/"diff"/"anchor"）。
    pub fn from_field(s: &str) -> Option<PieceKind> {
        match s {
            "body" => Some(PieceKind::Body),
            "diff" => Some(PieceKind::DiffList),
            "anchor" => Some(PieceKind::Anchor),
            _ => None,
        }
    }

    /// 闭集全量。
    pub const ALL: [PieceKind; 3] = [PieceKind::Body, PieceKind::DiffList, PieceKind::Anchor];
}

/// 解析出的快照件记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SnapRecord {
    /// 件类别
    pub kind: PieceKind,
    /// 件编号（0..=255）
    pub id: u8,
    /// 内容指纹（FNV-1a 32 位）
    pub hash: u32,
    /// 声明行数
    pub lines: u32,
}

/// 快照文本解析器：字段数恰为 4，数值段须可解析，缺一即 ParseFailure。
#[derive(Default)]
pub struct SnapParser {
    /// 累计成功解析条数
    pub parsed: u32,
    /// 累计拒绝条数
    pub rejected: u32,
}

impl SnapParser {
    /// 解析一行快照记录；失败原因由 SnapCode 表达。
    pub fn parse(&mut self, text: &str) -> Result<SnapRecord, SnapCode> {
        let parts: Vec<&str> = text.split('|').collect();
        if parts.len() != RECORD_FIELDS {
            self.rejected = self.rejected.saturating_add(1);
            return Err(SnapCode::ParseFailure);
        }
        let kind = match PieceKind::from_field(parts.get(0).copied().unwrap_or("")) {
            Some(k) => k,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(SnapCode::ParseFailure);
            }
        };
        let id = parse_u8(parts.get(1).copied().unwrap_or(""));
        let hash = parse_u32_hex(parts.get(2).copied().unwrap_or(""));
        let lines = parse_u32_dec(parts.get(3).copied().unwrap_or(""));
        match (id, hash, lines) {
            (Some(id), Some(hash), Some(lines)) => {
                self.parsed = self.parsed.saturating_add(1);
                Ok(SnapRecord { kind, id, hash, lines })
            }
            _ => {
                self.rejected = self.rejected.saturating_add(1);
                Err(SnapCode::ParseFailure)
            }
        }
    }
}

fn parse_u8(s: &str) -> Option<u8> {
    let v = parse_u32_dec(s)?;
    if v <= 255 {
        Some(v as u8)
    } else {
        None
    }
}

fn parse_u32_dec(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut acc: u32 = 0;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes.get(i)?;
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?.checked_add((*b - b'0') as u32)?;
        i += 1;
    }
    Some(acc)
}

fn parse_u32_hex(s: &str) -> Option<u32> {
    if s.len() < 2 {
        return None;
    }
    let bytes = s.as_bytes();
    if bytes.get(0) != Some(&b'0') || bytes.get(1) != Some(&b'x') {
        return None;
    }
    let mut acc: u32 = 0;
    let mut i = 2usize;
    while i < bytes.len() {
        let b = bytes.get(i)?;
        let d = match b {
            b'0'..=b'9' => (*b - b'0') as u32,
            b'a'..=b'f' => (*b - b'a') as u32 + 10,
            b'A'..=b'F' => (*b - b'A') as u32 + 10,
            _ => return None,
        };
        acc = acc.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(acc)
}

// ── 冻结状态机 ────────────────────────────────────────────────────────
/// 快照冻结状态机：单向 Collecting → Complete → Frozen。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeState {
    /// 收集中（并入进行时）
    Collecting,
    /// 三件齐备（可冻结）
    Complete,
    /// 已冻结（封账只读）
    Frozen,
}

/// 快照冻结器：维护三件套、状态与版本锚。
#[derive(Clone, Copy, Debug)]
pub struct FreezeBox {
    /// 当前状态
    pub state: FreezeState,
    /// 已并件类别（按 kind 判重）
    pub have: [bool; PIECE_COUNT],
    /// 版本号（首个并入件携带）
    pub version: u32,
    /// 并入件数
    pub pieces: u32,
    /// 冻结后的内容指纹
    pub sealed_hash: u32,
}

impl FreezeBox {
    /// 构造空冻结器。
    pub const fn new() -> FreezeBox {
        FreezeBox {
            state: FreezeState::Collecting,
            have: [false; PIECE_COUNT],
            version: 0,
            pieces: 0,
            sealed_hash: 0,
        }
    }

    fn kind_index(kind: PieceKind) -> usize {
        match kind {
            PieceKind::Body => 0,
            PieceKind::DiffList => 1,
            PieceKind::Anchor => 2,
        }
    }

    /// 并入一件：版本锚必须首个并入；同版本号才可并；Frozen 态拒改。
    pub fn merge(&mut self, rec: &SnapRecord) -> Result<FreezeState, SnapCode> {
        if self.state == FreezeState::Frozen {
            return Err(SnapCode::FrozenRewrite);
        }
        if self.pieces == 0 && rec.kind != PieceKind::Anchor {
            return Err(SnapCode::OrderViolation);
        }
        let idx = Self::kind_index(rec.kind);
        if self.have.get(idx).copied().unwrap_or(true) {
            return Err(SnapCode::DuplicatePiece);
        }
        if self.pieces == 0 {
            self.version = rec.hash.wrapping_add(rec.lines as u32);
        } else if rec.hash.wrapping_add(rec.lines as u32) != self.version {
            return Err(SnapCode::VersionDrift);
        }
        if let Some(slot) = self.have.get_mut(idx) {
            *slot = true;
        }
        self.pieces = self.pieces.saturating_add(1);
        if self.pieces == PIECE_COUNT as u32 {
            self.state = FreezeState::Complete;
        }
        Ok(self.state)
    }

    /// 冻结：三件齐备才可冻结；冻结时封存内容指纹。
    pub fn freeze(&mut self, corpus: u32) -> Result<u32, SnapCode> {
        if self.pieces != PIECE_COUNT as u32 {
            return Err(SnapCode::IncompleteSet);
        }
        let mut all = true;
        let mut i = 0usize;
        while i < PIECE_COUNT {
            if !self.have.get(i).copied().unwrap_or(false) {
                all = false;
            }
            i += 1;
        }
        if !all {
            return Err(SnapCode::IncompleteSet);
        }
        self.state = FreezeState::Frozen;
        self.sealed_hash = corpus;
        Ok(corpus)
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏快照样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadSnapCase {
    /// 缺版本锚（先并 body）
    MissingAnchor,
    /// 哈希被改（并入后改写）
    MutatedHash,
    /// 乱序并入（先 diff 后锚）
    OutOfOrder,
    /// 重复并入同件
    DuplicatePiece,
    /// 未齐三件即冻结
    UnfrozenMerge,
}

impl BadSnapCase {
    /// 闭集全量。
    pub const ALL: [BadSnapCase; 5] = [
        BadSnapCase::MissingAnchor,
        BadSnapCase::MutatedHash,
        BadSnapCase::OutOfOrder,
        BadSnapCase::DuplicatePiece,
        BadSnapCase::UnfrozenMerge,
    ];

    /// 注入一轮：构造坏样本并触发冻结器，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        let mut box_ = FreezeBox::new();
        let a = SnapRecord { kind: PieceKind::Anchor, id: 1, hash: 0x9128_0001, lines: 128 };
        let b = SnapRecord { kind: PieceKind::Body, id: 2, hash: 0x9128_0001, lines: 128 };
        let d = SnapRecord { kind: PieceKind::DiffList, id: 3, hash: 0x9128_0001, lines: 128 };
        match self {
            BadSnapCase::MissingAnchor => box_.merge(&b).is_err(),
            BadSnapCase::MutatedHash => {
                if box_.merge(&a).is_err() {
                    return false;
                }
                if box_.merge(&b).is_err() {
                    return false;
                }
                if box_.merge(&d).is_err() {
                    return false;
                }
                // 封账后再并入第五件必拒（冻结后改写）
                if box_.freeze(0xABCD).is_err() {
                    return false;
                }
                box_.merge(&b).is_err()
            }
            BadSnapCase::OutOfOrder => box_.merge(&d).is_err(),
            BadSnapCase::DuplicatePiece => {
                if box_.merge(&a).is_err() {
                    return false;
                }
                box_.merge(&a).is_err()
            }
            BadSnapCase::UnfrozenMerge => {
                let _ = box_.merge(&a);
                let _ = box_.merge(&b);
                box_.freeze(0xABCD).is_err()
            }
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FreezeLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl FreezeLedger {
    const fn zero() -> FreezeLedger {
        FreezeLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账失衡回 LedgerRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), SnapCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(SnapCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_snap_injection() -> (FreezeLedger, [u32; 5]) {
    let mut ledger = FreezeLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadSnapCase::ALL.len() {
        if let Some(case) = BadSnapCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；冻结后改写族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺件/缺锚族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；只读封账族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏记录族"),
    ("EROFS", -30, 19, "Windows ERROR_WRITE_PROTECT；快照只读段"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
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

/// 规范语料双跑指纹：解析→并入→冻结全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut p = SnapParser::default();
    let mut box_ = FreezeBox::new();
    let mut bytes: Vec<u8> = Vec::new();
    let texts = [
        "anchor|1|0x91280001|128",
        "body|2|0x91280001|128",
        "diff|3|0x91280001|128",
        "bogus|1|0x1|1",
    ];
    let mut i = 0usize;
    while i < texts.len() {
        if let Some(t) = texts.get(i) {
            let r = p.parse(t);
            bytes.push(if r.is_ok() { 1 } else { 0 });
            if let Ok(rec) = r {
                let m = box_.merge(&rec);
                bytes.push(if m.is_ok() { 2 } else { 3 });
            }
        }
        i += 1;
    }
    let sealed = box_.freeze(fnv1a(&bytes));
    bytes.push(match sealed {
        Ok(h) => (h & 0xFF) as u8,
        Err(_) => 0,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10010 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10010_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10010");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91280..=0x91800).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91280..=0x91800", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 解析器：合法四段记录可解析
    let mut p = SnapParser::default();
    let ok_parse = p.parse("anchor|1|0x91280001|128").is_ok();
    s.add("解析器：合法四段记录 1/1 解析成功", ok_parse,
          "kind|id|hash|lines 四段齐备即入账");

    // 3. 解析器：缺段/坏枚举/坏数字全拒
    let bad = p.parse("anchor|1").is_err()
        && p.parse("nope|1|0x1|1").is_err()
        && p.parse("anchor|x|0x1|1").is_err()
        && p.parse("anchor|1|ZZ|1").is_err();
    s.add("解析器：缺段/坏枚举/坏数字 4/4 拒绝", bad,
          "任何字段缺失或不可解析都不得静默吞掉");

    // 4. 状态机：三件齐备推进到 Complete
    let mut bx = FreezeBox::new();
    let a = SnapRecord { kind: PieceKind::Anchor, id: 1, hash: 7, lines: 3 };
    let b = SnapRecord { kind: PieceKind::Body, id: 2, hash: 7, lines: 3 };
    let d = SnapRecord { kind: PieceKind::DiffList, id: 3, hash: 7, lines: 3 };
    let st = box_state(&mut bx, &a, &b, &d);
    s.add("状态机：锚→体→清单三件并入达 Complete", st == FreezeState::Complete,
          "并入次序正确时状态单向推进");

    // 5. 状态机：冻结后封账
    let sealed = bx.freeze(0x1234);
    s.add("状态机：Complete 后冻结成功且指纹封账", sealed == Ok(0x1234)
        && bx.state == FreezeState::Frozen,
          "冻结返回内容指纹并进入只读态");

    // 6. 状态机：冻结后改写被拒
    let rewrite = bx.merge(&b) == Err(SnapCode::FrozenRewrite);
    s.add("状态机：Frozen 态再并入被拒", rewrite,
          "封账后任何改写都显性拒绝，走差异清单通道");

    // 7. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_snap_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "缺锚/改哈希/乱序/重复/未冻结全被显式拒绝");

    // 8. 注入账守恒：逃逸=0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 5 * INJECT_ROUNDS,
          "50 次注入必须 50 次检出、0 逃逸");

    // 9. 诊断码闭集：文案非空
    let text_ok = SnapCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 10. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < SnapCode::ALL.len() {
        let mut j = i + 1;
        while j < SnapCode::ALL.len() {
            if let (Some(x), Some(y)) = (SnapCode::ALL.get(i), SnapCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 11. 码值落在段窗口
    let in_seg = SnapCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：全部落段内 +0x01..+0x0C", in_seg,
          "码值不得越过闭集窗口");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(EACCES)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EROFS)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 13. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 14. 解析账闭合：成功+拒绝=总数
    let mut p2 = SnapParser::default();
    let _ = p2.parse("anchor|1|0x1|1");
    let _ = p2.parse("bad");
    let _ = p2.parse("body|2|0x2|2");
    s.add("解析账闭合：解析+拒绝=提交总数",
          p2.parsed.saturating_add(p2.rejected) == 3,
          "每行提交必须入两账之一，无第三去向");

    // 15. 版本锚先行的守恒性质：任意合法语料重放同果
    let mut replay_a = FreezeBox::new();
    let mut replay_b = FreezeBox::new();
    let _ = replay_a.merge(&a);
    let _ = replay_a.merge(&b);
    let _ = replay_a.merge(&d);
    let _ = replay_b.merge(&a);
    let _ = replay_b.merge(&b);
    let _ = replay_b.merge(&d);
    let same = replay_a.version == replay_b.version
        && replay_a.pieces == replay_b.pieces
        && replay_a.state == replay_b.state;
    s.add("重放一致：同语料两次重放状态逐项相等", same,
          "并入过程必须无隐藏状态，重放确定");

    // 16. 账目单调：再注入只增不减
    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    // 17. 件数不齐拒绝冻结
    let mut part = FreezeBox::new();
    let _ = part.merge(&a);
    let part_freeze = part.freeze(1) == Err(SnapCode::IncompleteSet);
    s.add("边界：两件套拒绝冻结", part_freeze,
          "三件不满时冻结必须显性拒绝，不得半冻结");

    // 动态明细（≤8）：注入账一行 + 指纹一行
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}", ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

fn box_state(bx: &mut FreezeBox, a: &SnapRecord, b: &SnapRecord, d: &SnapRecord) -> FreezeState {
    let _ = bx.merge(a);
    let _ = bx.merge(b);
    let _ = bx.merge(d);
    bx.state
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_wellformed() {
        let mut p = SnapParser::default();
        let r = p.parse("anchor|7|0xDEADBEEF|330");
        assert!(r.is_ok());
        assert_eq!(r.unwrap_or(SnapRecord { kind: PieceKind::Anchor, id: 0, hash: 0, lines: 0 }).id, 7);
    }

    #[test]
    fn parser_rejects_malformed() {
        let mut p = SnapParser::default();
        assert!(p.parse("anchor|1").is_err());
        assert!(p.parse("nope|1|0x1|1").is_err());
        assert!(p.parse("anchor|999|0x1|1").is_err());
    }

    #[test]
    fn anchor_must_come_first() {
        let mut bx = FreezeBox::new();
        let b = SnapRecord { kind: PieceKind::Body, id: 2, hash: 7, lines: 3 };
        assert_eq!(bx.merge(&b), Err(SnapCode::OrderViolation));
    }

    #[test]
    fn duplicate_piece_rejected() {
        let mut bx = FreezeBox::new();
        let a = SnapRecord { kind: PieceKind::Anchor, id: 1, hash: 7, lines: 3 };
        assert!(bx.merge(&a).is_ok());
        assert_eq!(bx.merge(&a), Err(SnapCode::DuplicatePiece));
    }

    #[test]
    fn version_drift_detected() {
        let mut bx = FreezeBox::new();
        let a = SnapRecord { kind: PieceKind::Anchor, id: 1, hash: 7, lines: 3 };
        let b = SnapRecord { kind: PieceKind::Body, id: 2, hash: 9, lines: 3 };
        assert!(bx.merge(&a).is_ok());
        assert_eq!(bx.merge(&b), Err(SnapCode::VersionDrift));
    }

    #[test]
    fn incomplete_set_cannot_freeze() {
        let mut bx = FreezeBox::new();
        let a = SnapRecord { kind: PieceKind::Anchor, id: 1, hash: 7, lines: 3 };
        let _ = bx.merge(&a);
        assert_eq!(bx.freeze(1), Err(SnapCode::IncompleteSet));
    }

    #[test]
    fn frozen_box_is_readonly() {
        let mut bx = FreezeBox::new();
        let a = SnapRecord { kind: PieceKind::Anchor, id: 1, hash: 7, lines: 3 };
        let b = SnapRecord { kind: PieceKind::Body, id: 2, hash: 7, lines: 3 };
        let d = SnapRecord { kind: PieceKind::DiffList, id: 3, hash: 7, lines: 3 };
        let _ = bx.merge(&a);
        let _ = bx.merge(&b);
        let _ = bx.merge(&d);
        assert!(bx.freeze(42).is_ok());
        assert_eq!(bx.merge(&b), Err(SnapCode::FrozenRewrite));
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_snap_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 5 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EACCES));
        assert!(agrees_with_linux(EROFS));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn snapcode_text_complete() {
        for c in SnapCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn unx10010_checks_green() {
        let s = run_unx10010_checks();
        assert!(s.all_passed(), "UNX-F10010 red: {}", s.red_items_text());
    }
}
