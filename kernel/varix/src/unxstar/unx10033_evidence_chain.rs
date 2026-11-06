//! UNX-F10033 清偿证据链归档 · 0x91800段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10034-J1 清偿列三格齐（存量/已偿/占比 10/10 次取数正确），
//! 混列 10/10 次检出。
//!
//! SEG_BASE = 0x91800：F10033 独占 0x40 窗口；诊断码取 SEG_BASE + 变体序号
//! （+0x01..+0x0C 闭集），段外码一律非法。
//!
//! 设计要点：
//!   ① 证据链按「三件套同锚归档」建模：复测件（T）、判定件（V）、台账件
//!      （L）三类件必须同条目号同轮次才可归档；缺任一件或锚不齐 10/10 次
//!      拒收。
//!   ② 注册表 + 账本 + 状态机三件齐：收件注册表（逐件可查）、归档账本
//!      （链条守恒：收件数 = 归档件数 + 在途件数）、链条生命周期
//!      Incomplete→Complete→Archived 只进不退。
//!   ③ 注入核四类坏样本：缺复测件 / 缺判定件 / 缺台账件 / 锚不齐（同轮
//!      异条目），逐类 10 轮复测全部显性拒绝，逃逸恒为 0。
//!   ④ 与 Linux errno 冻结表逐码对照；FNV-1a 双跑指纹逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10033 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91800;

/// 收件注册表容量（件数上限，防无限增长）。
pub const PIECE_CAPACITY: usize = 24;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EPERM: i32 = 1;
pub const EBADF: i32 = 9;

// ── 证据件闭集 ────────────────────────────────────────────────────────
/// 证据件三类（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceKind {
    /// 复测件：逐条重测留痕
    Retest,
    /// 判定件：通过判定双档核
    Verdict,
    /// 台账件：清偿台账在册位
    LedgerPost,
}

impl PieceKind {
    /// 类别缩写（句式解析用）。
    pub fn tag(self) -> &'static str {
        match self {
            PieceKind::Retest => "T",
            PieceKind::Verdict => "V",
            PieceKind::LedgerPost => "L",
        }
    }

    /// 按缩写反查；表外回 Err。
    pub fn from_tag(t: &str) -> Result<PieceKind, ChainCode> {
        match t {
            "T" => Ok(PieceKind::Retest),
            "V" => Ok(PieceKind::Verdict),
            "L" => Ok(PieceKind::LedgerPost),
            _ => Err(ChainCode::UnknownPiece),
        }
    }

    /// 闭集全量。
    pub const ALL: [PieceKind; 3] = [
        PieceKind::Retest,
        PieceKind::Verdict,
        PieceKind::LedgerPost,
    ];
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainCode {
    /// 件句式坏：不符 `T:I02:3` 三段式
    BadPieceSyntax,
    /// 未知件类
    UnknownPiece,
    /// 重复收件：同类同锚已收
    DuplicatePiece,
    /// 缺复测件
    MissingRetest,
    /// 缺判定件
    MissingVerdict,
    /// 缺台账件
    MissingLedger,
    /// 锚不齐：同链内条目号/轮次不一致
    AnchorMismatch,
    /// 未齐链归档
    ArchiveBeforeComplete,
    /// 归档后再收件
    PieceAfterArchive,
    /// 状态回退
    StateRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl ChainCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn code(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ChainCode::BadPieceSyntax => "件句式坏被拒：证据件必须为 `K:INN:N` \
                三段式（K ∈ T/V/L）；请按类/条目/轮次重排",
            ChainCode::UnknownPiece => "未知件类被拒：件类不在 T/V/L 闭集内；\
                请按复测/判定/台账三件归类",
            ChainCode::DuplicatePiece => "重复收件被拒：同类同锚件已在册；\
                请先查册再补录，重复回 EEXIST",
            ChainCode::MissingRetest => "缺复测件被拒：证据链必须以逐条重测\
                留痕开头；请先补复测件",
            ChainCode::MissingVerdict => "缺判定件被拒：复测之后必须有通过判定；\
                请补判定件再归档",
            ChainCode::MissingLedger => "缺台账件被拒：判定之后必须落台账；\
                请补台账件再归档",
            ChainCode::AnchorMismatch => "锚不齐被拒：链内件必须同条目号同轮次；\
                异锚件请拆链归档",
            ChainCode::ArchiveBeforeComplete => "未齐链归档被拒：三件不齐不得\
                归档；缺哪件按缺件码定位",
            ChainCode::PieceAfterArchive => "归档后收件被拒：链条归档即冻结；\
                补件请开新一轮",
            ChainCode::StateRegress => "状态回退被拒：链条生命周期只进不退；\
                回退视为私改证据",
            ChainCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定遍历序",
            ChainCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且\
                落在 0x91800..=0x91880 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [ChainCode; 12] = [
        ChainCode::BadPieceSyntax,
        ChainCode::UnknownPiece,
        ChainCode::DuplicatePiece,
        ChainCode::MissingRetest,
        ChainCode::MissingVerdict,
        ChainCode::MissingLedger,
        ChainCode::AnchorMismatch,
        ChainCode::ArchiveBeforeComplete,
        ChainCode::PieceAfterArchive,
        ChainCode::StateRegress,
        ChainCode::NotReproducible,
        ChainCode::SegmentViolation,
    ];
}

// ── 证据件句式解析器 ──────────────────────────────────────────────────
/// 证据件：件类 + 条目号 + 轮次。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EvidencePiece {
    /// 件类（T/V/L 闭集）
    pub kind: PieceKind,
    /// 条目号（1..=99）
    pub item: u8,
    /// 轮次（1..=9）
    pub round: u8,
}

/// 件句式解析：`K:INN:N` 共 7 字节；N ∈ 1..=9。
pub fn parse_piece(text: &str) -> Result<EvidencePiece, ChainCode> {
    let bytes = text.as_bytes();
    if bytes.len() != 7 {
        return Err(ChainCode::BadPieceSyntax);
    }
    let tag: Vec<u8> = {
        let mut v = Vec::new();
        if let Some(b) = bytes.get(0) {
            v.push(*b);
        }
        v
    };
    let tag_str = match core::str::from_utf8(&tag) {
        Ok(s) => s,
        Err(_) => return Err(ChainCode::UnknownPiece),
    };
    let kind = PieceKind::from_tag(tag_str)?;
    let c1 = match bytes.get(1) {
        Some(b) => *b,
        None => return Err(ChainCode::BadPieceSyntax),
    };
    let c2 = match bytes.get(5) {
        Some(b) => *b,
        None => return Err(ChainCode::BadPieceSyntax),
    };
    if c1 != b':' || c2 != b':' {
        return Err(ChainCode::BadPieceSyntax);
    }
    // 第 2 字节必须为 'I'（条目号头）。
    match bytes.get(2) {
        Some(b) if *b == b'I' => {}
        _ => return Err(ChainCode::BadPieceSyntax),
    }
    let ih = match bytes.get(3) {
        Some(b) if b.is_ascii_digit() => b - b'0',
        _ => return Err(ChainCode::BadPieceSyntax),
    };
    let it = match bytes.get(4) {
        Some(b) if b.is_ascii_digit() => b - b'0',
        _ => return Err(ChainCode::BadPieceSyntax),
    };
    let item = ih * 10 + it;
    if item < 1 {
        return Err(ChainCode::BadPieceSyntax);
    }
    let rd = match bytes.get(6) {
        Some(b) if b.is_ascii_digit() => b - b'0',
        _ => return Err(ChainCode::BadPieceSyntax),
    };
    if rd < 1 {
        return Err(ChainCode::BadPieceSyntax);
    }
    Ok(EvidencePiece { kind, item, round: rd })
}

// ── 链条生命周期状态机 ────────────────────────────────────────────────
/// 链条生命周期：只进不退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainState {
    /// 收件中：三件未齐
    Incomplete,
    /// 三件齐：可归档
    Complete,
    /// 已归档：只读
    Archived,
}

/// 证据链归档账本：收件注册表 + 归档状态机。
#[derive(Clone, Debug)]
pub struct EvidenceChain {
    item: u8,
    round: u8,
    state: ChainState,
    pieces: Vec<EvidencePiece>,
}

impl EvidenceChain {
    /// 以条目号+轮次开链（锚冻结，链内件必须同锚）。
    pub fn new(item: u8, round: u8) -> EvidenceChain {
        EvidenceChain { item, round, state: ChainState::Incomplete, pieces: Vec::new() }
    }

    /// 当前状态。
    pub fn state(&self) -> ChainState {
        self.state
    }

    /// 链锚（条目号, 轮次）。
    pub fn anchor(&self) -> (u8, u8) {
        (self.item, self.round)
    }

    /// 收件：句式解析 + 同锚校验 + 重复拒绝 + 容量守卫。
    pub fn accept(&mut self, text: &str) -> Result<(), ChainCode> {
        if self.state == ChainState::Archived {
            return Err(ChainCode::PieceAfterArchive);
        }
        let p = parse_piece(text)?;
        if p.item != self.item || p.round != self.round {
            return Err(ChainCode::AnchorMismatch);
        }
        if self.pieces.len() >= PIECE_CAPACITY {
            return Err(ChainCode::DuplicatePiece);
        }
        let mut i = 0usize;
        while i < self.pieces.len() {
            if let Some(q) = self.pieces.get(i) {
                if q.kind == p.kind {
                    return Err(ChainCode::DuplicatePiece);
                }
            }
            i += 1;
        }
        self.pieces.push(p);
        if self.chain_complete() {
            self.state = ChainState::Complete;
        }
        Ok(())
    }

    /// 已收件数。
    pub fn piece_count(&self) -> usize {
        self.pieces.len()
    }

    /// 某件类是否在册。
    pub fn has_kind(&self, k: PieceKind) -> bool {
        let mut i = 0usize;
        while i < self.pieces.len() {
            if let Some(q) = self.pieces.get(i) {
                if q.kind == k {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 三件齐判定：T/V/L 逐类在册。
    pub fn chain_complete(&self) -> bool {
        self.has_kind(PieceKind::Retest)
            && self.has_kind(PieceKind::Verdict)
            && self.has_kind(PieceKind::LedgerPost)
    }

    /// 首个缺件码（T→V→L 顺序定位）。
    pub fn first_missing(&self) -> Option<ChainCode> {
        if !self.has_kind(PieceKind::Retest) {
            return Some(ChainCode::MissingRetest);
        }
        if !self.has_kind(PieceKind::Verdict) {
            return Some(ChainCode::MissingVerdict);
        }
        if !self.has_kind(PieceKind::LedgerPost) {
            return Some(ChainCode::MissingLedger);
        }
        None
    }

    /// 归档：Complete → Archived；未齐链按缺件码拒绝。
    pub fn archive(&mut self) -> Result<(), ChainCode> {
        if self.state == ChainState::Archived {
            return Err(ChainCode::StateRegress);
        }
        if let Some(miss) = self.first_missing() {
            return Err(miss);
        }
        self.state = ChainState::Archived;
        Ok(())
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账不平返回负诊断码。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(ChainCode::StateRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 构造齐链：T/V/L 三件同锚（条目 item、轮次 round）。
pub fn chain_complete_for(item: u8, round: u8) -> EvidenceChain {
    let mut c = EvidenceChain::new(item, round);
    let t = format!("T:I{:02}:{:01}", item, round);
    let v = format!("V:I{:02}:{:01}", item, round);
    let l = format!("L:I{:02}:{:01}", item, round);
    let _ = c.accept(&t);
    let _ = c.accept(&v);
    let _ = c.accept(&l);
    c
}

/// 四类注入：0 缺复测件、1 缺判定件、2 缺台账件、3 锚不齐。
/// 返回（收件清单，期望码序）。清单里每条 (文本, 是否直接收)。
pub fn inject_case(class: usize) -> (Vec<(&'static str, bool)>, usize) {
    match class {
        0 => {
            let rows: Vec<(&'static str, bool)> =
                vec![("V:I01:1", true), ("L:I01:1", true)];
            (rows, 0)
        }
        1 => {
            let rows: Vec<(&'static str, bool)> =
                vec![("T:I01:1", true), ("L:I01:1", true)];
            (rows, 1)
        }
        2 => {
            let rows: Vec<(&'static str, bool)> =
                vec![("T:I01:1", true), ("V:I01:1", true)];
            (rows, 2)
        }
        _ => {
            // 锚不齐：台账件挂异条目。
            let rows: Vec<(&'static str, bool)> =
                vec![("T:I01:1", true), ("V:I01:1", true), ("L:I02:1", true)];
            (rows, 3)
        }
    }
}

/// 执行注入矩阵：4 类坏样本 × 10 轮，逐类断言被显性拒绝。
pub fn run_injection_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut c = 0usize;
    while c < 4 {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let (rows, want) = inject_case(c);
            let mut chain = EvidenceChain::new(1, 1);
            let mut last_err: Option<ChainCode> = None;
            let mut i = 0usize;
            while i < rows.len() {
                if let Some((text, ok)) = rows.get(i) {
                    if *ok {
                        if let Err(e) = chain.accept(text) {
                            last_err = Some(e);
                        }
                    } else {
                        last_err = chain.accept(text).err();
                    }
                }
                i += 1;
            }
            let caught = match want {
                0 => chain.archive().err() == Some(ChainCode::MissingRetest),
                1 => chain.archive().err() == Some(ChainCode::MissingVerdict),
                2 => chain.archive().err() == Some(ChainCode::MissingLedger),
                _ => last_err == Some(ChainCode::AnchorMismatch),
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(c) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        c += 1;
    }
    (ledger, per_class)
}

/// 构造已归档链：三件齐并归档。
pub fn build_archived_chain() -> EvidenceChain {
    let mut c = chain_complete_for(2, 3);
    let _ = c.archive();
    c
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；归档后写入/状态回退族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺件族"),
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；锚不齐/错链族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复收件族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；句式/件类坏族"),
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

/// 链指纹：锚 + 状态 + 逐件（类/条目/轮次）入哈希。
pub fn chain_fingerprint(c: &EvidenceChain) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let (it, rd) = c.anchor();
    bytes.push(it);
    bytes.push(rd);
    bytes.push(match c.state() {
        ChainState::Incomplete => 0,
        ChainState::Complete => 1,
        ChainState::Archived => 2,
    });
    let mut n = 0usize;
    while n < PieceKind::ALL.len() {
        if let Some(k) = PieceKind::ALL.get(n) {
            bytes.push(if c.has_kind(*k) { 1 } else { 0 });
        }
        n += 1;
    }
    bytes.push(c.piece_count() as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10033 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10033_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10033");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91800..=0x91880).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91800..=0x91880", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 件类闭集冻结：三类逐类可查
    let kinds_ok = PieceKind::Retest.tag() == "T"
        && PieceKind::Verdict.tag() == "V"
        && PieceKind::LedgerPost.tag() == "L"
        && PieceKind::from_tag("T").is_ok()
        && PieceKind::from_tag("X").is_err();
    s.add("件类闭集：T/V/L 逐类可查、表外拒绝", kinds_ok,
          "证据件三分类是链条骨架，必须成文冻结");

    // 3. 件句式解析：合法样本全过
    let p1 = parse_piece("T:I02:3");
    let p2 = parse_piece("L:I01:1");
    let parse_ok = p1 == Ok(EvidencePiece { kind: PieceKind::Retest, item: 2, round: 3 })
        && p2.is_ok();
    s.add("件解析：`K:INN:N` 合法样本逐段命中", parse_ok,
          "证据件句式必须能解析回类/条目/轮次");

    // 4. 坏句式拒绝
    let bad = parse_piece("X:I01:1").err() == Some(ChainCode::UnknownPiece)
        && parse_piece("T:I01:0").err() == Some(ChainCode::BadPieceSyntax)
        && parse_piece("T:I1:1").err() == Some(ChainCode::BadPieceSyntax)
        && parse_piece("").err() == Some(ChainCode::BadPieceSyntax);
    s.add("句式拒绝：错类/零轮/短行逐类拒绝", bad,
          "句式破格必须显性报错，不许静默容忍");

    // 5. 完整链条：三件齐归档成功
    let full = build_archived_chain();
    s.add("链条闭环：三件齐且归档成功",
          full.state() == ChainState::Archived
              && full.piece_count() == 3
              && full.anchor() == (2, 3),
          "复测+判定+台账三件同锚齐备才能归档");

    // 6. 注入矩阵：4 类坏样本 × 10/10 检出
    let (ledger, per_class) = run_injection_matrix();
    let mut all_caught = true;
    let mut c = 0usize;
    while c < per_class.len() {
        if let Some(k) = per_class.get(c) {
            if *k != INJECT_ROUNDS {
                all_caught = false;
            }
        }
        c += 1;
    }
    s.add("注入核：4 类坏样本逐类 10/10 检出", all_caught,
          "缺复测/缺判定/缺台账/锚不齐必须全部显性拒绝");

    // 7. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 8. 缺件拒收（核心判据）：逐件抽走 10/10
    let mut miss_caught = 0u32;
    let mut drop = 0usize;
    while drop < PieceKind::ALL.len() {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut chain = EvidenceChain::new(1, 1);
            let mut n = 0usize;
            while n < PieceKind::ALL.len() {
                if n != drop {
                    if let Some(k) = PieceKind::ALL.get(n) {
                        let text = format!("{}:I01:1", k.tag());
                        let _ = chain.accept(&text);
                    }
                }
                n += 1;
            }
            let expect = match drop {
                0 => ChainCode::MissingRetest,
                1 => ChainCode::MissingVerdict,
                _ => ChainCode::MissingLedger,
            };
            if chain.archive().err() == Some(expect) {
                miss_caught += 1;
            }
            round += 1;
        }
        drop += 1;
    }
    s.add("缺件拒收：逐件抽走 30/30 次拒绝",
          miss_caught == PieceKind::ALL.len() as u32 * INJECT_ROUNDS,
          "三类件各缺一次 × 10 轮，全部必须按缺件码拒绝");

    // 9. 锚不齐拦截（复测口径：同一批 10 轮）
    let mut anchor_caught = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut chain = EvidenceChain::new(1, 1);
        let _ = chain.accept("T:I01:1");
        let r = chain.accept("V:I02:1");
        if r.err() == Some(ChainCode::AnchorMismatch) {
            anchor_caught += 1;
        }
        round += 1;
    }
    s.add("锚不齐拦截：异条目件 10/10 次拒收", anchor_caught == INJECT_ROUNDS,
          "链内件必须同条目号同轮次，异锚一律退回");

    // 10. 重复收件拒绝
    let mut dup = EvidenceChain::new(1, 1);
    let a = dup.accept("T:I01:1");
    let b = dup.accept("T:I01:1");
    s.add("重复拒绝：同类同锚二次收件被拒",
          a.is_ok() && b.err() == Some(ChainCode::DuplicatePiece),
          "重复收件必须显性拒绝，不许静默覆盖");

    // 11. 未齐链归档拒绝
    let mut thin = EvidenceChain::new(1, 1);
    let _ = thin.accept("T:I01:1");
    let thin_archive = thin.archive();
    s.add("缺件归档被拒：单件链条不得归档",
          thin_archive.err() == Some(ChainCode::MissingVerdict),
          "T 在册而 V 缺时归档必须按缺判定件拒绝");

    // 12. 归档后收件拒绝、状态只进不退
    let mut archived = build_archived_chain();
    let late = archived.accept("T:I02:3");
    s.add("状态机：归档后收件被拒、只进不退",
          late.err() == Some(ChainCode::PieceAfterArchive)
              && archived.state() == ChainState::Archived,
          "Incomplete→Complete→Archived 单向流，回退/越位都拒绝");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EBADF)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑指纹一致
    let fp_a = chain_fingerprint(&build_archived_chain());
    let fp_b = chain_fingerprint(&build_archived_chain());
    s.add("双跑一致：链条指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 15. 文案闭集
    let text_ok = ChainCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < ChainCode::ALL.len() {
        let mut j = i + 1;
        while j < ChainCode::ALL.len() {
            if let (Some(a), Some(b)) = (ChainCode::ALL.get(i), ChainCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("链条指纹 0x{:08X}（锚 I{:02}:R{:01}）",
        fp_a, full.anchor().0, full.anchor().1);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_parse() {
        assert_eq!(parse_piece("V:I07:2"),
                   Ok(EvidencePiece { kind: PieceKind::Verdict, item: 7, round: 2 }));
        assert_eq!(parse_piece("X:I01:1"), Err(ChainCode::UnknownPiece));
    }

    #[test]
    fn anchor_mismatch_rejected() {
        let mut c = EvidenceChain::new(1, 1);
        assert!(c.accept("T:I01:1").is_ok());
        assert_eq!(c.accept("L:I03:1"), Err(ChainCode::AnchorMismatch));
    }

    #[test]
    fn complete_chain_archives() {
        let mut c = chain_complete_for(4, 2);
        assert_eq!(c.state(), ChainState::Complete);
        assert!(c.archive().is_ok());
        assert_eq!(c.state(), ChainState::Archived);
    }

    #[test]
    fn missing_verdict_rejected() {
        let mut c = EvidenceChain::new(1, 1);
        let _ = c.accept("T:I01:1");
        let _ = c.accept("L:I01:1");
        assert_eq!(c.archive(), Err(ChainCode::MissingVerdict));
    }

    #[test]
    fn missing_ledger_rejected() {
        let mut c = EvidenceChain::new(1, 1);
        let _ = c.accept("T:I01:1");
        let _ = c.accept("V:I01:1");
        assert_eq!(c.archive(), Err(ChainCode::MissingLedger));
    }

    #[test]
    fn duplicate_piece_rejected() {
        let mut c = EvidenceChain::new(1, 1);
        assert!(c.accept("T:I01:1").is_ok());
        assert_eq!(c.accept("T:I01:1"), Err(ChainCode::DuplicatePiece));
    }

    #[test]
    fn piece_after_archive_rejected() {
        let mut c = build_archived_chain();
        assert_eq!(c.accept("T:I02:3"), Err(ChainCode::PieceAfterArchive));
    }

    #[test]
    fn first_missing_ordered() {
        let c = EvidenceChain::new(1, 1);
        assert_eq!(c.first_missing(), Some(ChainCode::MissingRetest));
    }

    #[test]
    fn injection_matrix_green() {
        let (l, per) = run_injection_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 4 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(chain_fingerprint(&build_archived_chain()),
                   chain_fingerprint(&build_archived_chain()));
    }

    #[test]
    fn errno_table_agrees() {
        assert!(agrees_with_linux(EBADF));
        assert_eq!(linux_neg(ENOENT), -ENOENT);
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn unx10033_checks_green() {
        let set = run_unx10033_checks();
        assert!(set.all_passed(), "UNX-F10033 red: {}", set.red_items_text());
    }
}
