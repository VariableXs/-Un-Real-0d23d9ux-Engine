//! UNX-F10096 双判据防重协议（正反同 ID 约束）· 0x927C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10097-J1 分配表冻结（五战线×批次全覆盖），战线缺位 10/10 次检出
//!
//! SEG_BASE = 0x927C0（本卡独占 0x40 窗口，下一卡 0x92800）；
//! 诊断码闭集 SEG_BASE+0x01..=+0x0D 共 13 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器：册别解析器（main/batch/deep 三册闭集，闭集外拒绝）+
//!      结构指纹解析器（面 token × 复测轮数，形制 `面@<n>`），双解析器
//!      是判据登记的唯一入口。
//!   ② 注册表：全域 ID 注册表 IdRegistry，同 ID 的 J1/J2 双侧必须
//!      同册同构：册别不同 → 拆册异构，结构指纹不同 → 结构漂移，
//!      同侧重登 → 撞号，均逐次显性拒绝。
//!   ③ 账本：全域唯一性账（ID → 双侧登记），对账审计要求每 ID 双侧
//!      齐备且同册同构；缺反判据逐 ID 检出。
//!   ④ 注入核：拆册异构 / 结构漂移 / 同侧撞号 / 缺反判据 四类坏样本
//!      逐类 10/10 次拒绝，注入账 注入=检出+逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10096 独占 0x40 窗口，下一卡 0x92800）；诊断码闭集
/// SEG_BASE+0x01..=+0x0D 共 13 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x927C0;

/// 注入复测轮数（判据要求 10/10；结构指纹的标准轮数同源）。
pub const INJECT_ROUNDS: u32 = 10;

/// 全域注册表容量上限（ID 数）。
pub const REGISTRY_CAP: usize = 64;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0D 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DedupeCode {
    /// 未知册别（三册闭集外）
    UnknownBook,
    /// 非法结构指纹
    SpecMalformed,
    /// 同侧重复登记（撞号）
    DupSide,
    /// 同 ID 拆册（J1/J2 不同册）
    CrossBookSplit,
    /// 同 ID 双侧结构漂移
    StructureMismatch,
    /// 缺反判据（J2 侧缺失）
    MissingCounterpart,
    /// 零号 ID（保留键）
    ZeroId,
    /// 轮数不达标准
    ShortRounds,
    /// 注册表容量满
    RegistryFull,
    /// 未知 ID
    UnknownId,
    /// 拒绝后注册表被改动
    RejectedTrace,
    /// 册别标签往返不一致
    BookRoundTrip,
    /// 段位越界
    SegmentViolation,
}

impl DedupeCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            DedupeCode::UnknownBook => "未知册别被拒：册别只认 main|batch|deep 闭集；\
                请改用闭集内写法",
            DedupeCode::SpecMalformed => "非法结构指纹：形制必须 面@<n>；\
                面名非空且轮数为正",
            DedupeCode::DupSide => "同侧撞号被拒：同 ID 同侧只能登记一次；\
                修订请走重测而非覆盖",
            DedupeCode::CrossBookSplit => "拆册异构被拒：同 ID 的 J1/J2 必须落在同一册；\
                跨册拆分即防重违规",
            DedupeCode::StructureMismatch => "结构漂移被拒：同 ID 双侧的复测轮数必须同构；\
                请核对 10/10 口径",
            DedupeCode::MissingCounterpart => "缺反判据被拒：每 ID 必须 J1/J2 双侧齐备；\
                单侧条目防重审计必须检出",
            DedupeCode::ZeroId => "零号 ID：零是保留键，禁止入表；请换用正数 ID",
            DedupeCode::ShortRounds => "轮数不达：双侧复测轮数必须各达 INJECT_ROUNDS；\
                请补足复测",
            DedupeCode::RegistryFull => "注册表满：已达容量上限；请先归档旧表再续",
            DedupeCode::UnknownId => "未知 ID：表中无此条目号；请先登记再操作",
            DedupeCode::RejectedTrace => "拒绝留痕被拒：被拒登记不得改动任何已有条目；\
                请排查拒绝路径是否误提交",
            DedupeCode::BookRoundTrip => "册别往返不一致：label 再解析必须回到原册别；\
                请检查闭集文案与解析器是否同步",
            DedupeCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [DedupeCode; 13] = [
        DedupeCode::UnknownBook,
        DedupeCode::SpecMalformed,
        DedupeCode::DupSide,
        DedupeCode::CrossBookSplit,
        DedupeCode::StructureMismatch,
        DedupeCode::MissingCounterpart,
        DedupeCode::ZeroId,
        DedupeCode::ShortRounds,
        DedupeCode::RegistryFull,
        DedupeCode::UnknownId,
        DedupeCode::RejectedTrace,
        DedupeCode::BookRoundTrip,
        DedupeCode::SegmentViolation,
    ];
}

// ── 册别闭集与解析器 ──────────────────────────────────────────────────
/// 册别闭集：三册。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BookKind {
    /// 总册
    Main,
    /// 批册
    Batch,
    /// 深化册
    Deep,
}

impl BookKind {
    /// 长名（成文与往返用）。
    pub fn label(self) -> &'static str {
        match self {
            BookKind::Main => "main",
            BookKind::Batch => "batch",
            BookKind::Deep => "deep",
        }
    }

    /// 闭集全量。
    pub const ALL: [BookKind; 3] = [BookKind::Main, BookKind::Batch, BookKind::Deep];
}

/// 册别解析：只认 main|batch|deep，其余（含空串/大小写漂移）拒绝。
pub fn parse_book(tag: &str) -> Result<BookKind, DedupeCode> {
    match tag {
        "main" => Ok(BookKind::Main),
        "batch" => Ok(BookKind::Batch),
        "deep" => Ok(BookKind::Deep),
        _ => Err(DedupeCode::UnknownBook),
    }
}

/// 结构指纹：面 token + 复测轮数。同构 = 双侧同面名口径与同轮数。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StructFp {
    /// 面 token（非空）
    pub aspect: String,
    /// 复测轮数（正数）
    pub rounds: u32,
}

/// 结构指纹解析：形制 `面@<n>`；面名非空、轮数为正数。
pub fn parse_struct_fp(text: &str) -> Result<StructFp, DedupeCode> {
    let at = match text.find('@') {
        Some(p) => p,
        None => return Err(DedupeCode::SpecMalformed),
    };
    let aspect = match text.get(..at) {
        Some(a) => a,
        None => return Err(DedupeCode::SpecMalformed),
    };
    let num = match text.get(at + 1..) {
        Some(n) => n,
        None => return Err(DedupeCode::SpecMalformed),
    };
    if aspect.is_empty() || num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) {
        return Err(DedupeCode::SpecMalformed);
    }
    let mut rounds = 0u32;
    let nb = num.as_bytes();
    let mut ni = 0usize;
    while ni < nb.len() {
        if let Some(b) = nb.get(ni) {
            rounds = rounds.wrapping_mul(10).wrapping_add((*b - b'0') as u32);
        }
        ni += 1;
    }
    if rounds == 0 {
        return Err(DedupeCode::SpecMalformed);
    }
    Ok(StructFp { aspect: String::from(aspect), rounds })
}

// ── 全域 ID 注册表 ────────────────────────────────────────────────────
/// 单 ID 登记条目：册别 + J1/J2 双侧结构指纹。
#[derive(Clone, Debug)]
pub struct IdRecord {
    /// 条目号
    pub id: u32,
    /// 所属册别（双侧必须同册）
    pub book: BookKind,
    /// 正判据结构指纹
    pub j1: Option<StructFp>,
    /// 反判据结构指纹
    pub j2: Option<StructFp>,
}

/// 全域注册表：同 ID 双侧同册同构是唯一合法形态。
#[derive(Clone, Debug)]
pub struct IdRegistry {
    recs: Vec<IdRecord>,
    cap: usize,
}

impl IdRegistry {
    /// 空表构造（容量 REGISTRY_CAP）。
    pub fn new() -> IdRegistry {
        IdRegistry { recs: Vec::new(), cap: REGISTRY_CAP }
    }

    /// 登记一侧：ID 新建或补另一侧；同册同构约束在此把关。
    /// 合法路径：首登任意侧（先 J1 后 J2 或反之皆可），补侧必须同册同构。
    pub fn enroll(
        &mut self,
        id: u32,
        book: BookKind,
        j1_side: bool,
        fp: &StructFp,
    ) -> Result<(), DedupeCode> {
        if id == 0 {
            return Err(DedupeCode::ZeroId);
        }
        if fp.rounds < INJECT_ROUNDS {
            return Err(DedupeCode::ShortRounds);
        }
        // 已有条目：同册约束与同侧撞号把关。
        let mut idx: Option<usize> = None;
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.id == id {
                    idx = Some(i);
                }
            }
            i += 1;
        }
        match idx {
            None => {
                if self.recs.len() >= self.cap {
                    return Err(DedupeCode::RegistryFull);
                }
                let rec = IdRecord { id, book, j1: None, j2: None };
                self.recs.push(rec);
                let rec = match self.recs.last_mut() {
                    Some(r) => r,
                    None => return Err(DedupeCode::RegistryFull),
                };
                if j1_side {
                    rec.j1 = Some(StructFp { aspect: fp.aspect.clone(), rounds: fp.rounds });
                } else {
                    rec.j2 = Some(StructFp { aspect: fp.aspect.clone(), rounds: fp.rounds });
                }
                Ok(())
            }
            Some(ix) => {
                let rec = match self.recs.get_mut(ix) {
                    Some(r) => r,
                    None => return Err(DedupeCode::UnknownId),
                };
                if rec.book != book {
                    return Err(DedupeCode::CrossBookSplit);
                }
                // 先只读校验（避免借用冲突）：同侧撞号 + 同构约束。
                let (target_filled, other_rounds) = if j1_side {
                    (rec.j1.is_some(), rec.j2.as_ref().map(|o| o.rounds))
                } else {
                    (rec.j2.is_some(), rec.j1.as_ref().map(|o| o.rounds))
                };
                if target_filled {
                    return Err(DedupeCode::DupSide);
                }
                if let Some(or) = other_rounds {
                    if or != fp.rounds {
                        return Err(DedupeCode::StructureMismatch);
                    }
                }
                let slot = if j1_side { &mut rec.j1 } else { &mut rec.j2 };
                *slot = Some(StructFp { aspect: fp.aspect.clone(), rounds: fp.rounds });
                Ok(())
            }
        }
    }

    /// 已登记 ID 数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    /// 指定 ID 双侧是否齐备。
    pub fn paired(&self, id: u32) -> bool {
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.id == id {
                    return r.j1.is_some() && r.j2.is_some();
                }
            }
            i += 1;
        }
        false
    }

    /// 全域防重审计：每 ID 双侧齐备（缺反判据 = 0 条）。
    pub fn all_paired(&self) -> bool {
        let mut i = 0usize;
        while i < self.recs.len() {
            match self.recs.get(i) {
                None => return false,
                Some(r) => {
                    if r.j1.is_none() || r.j2.is_none() {
                        return false;
                    }
                }
            }
            i += 1;
        }
        true
    }

    /// 全域同册同构审计：每 ID 双侧轮数一致。
    pub fn all_same_shape(&self) -> bool {
        let mut i = 0usize;
        while i < self.recs.len() {
            match self.recs.get(i) {
                None => return false,
                Some(r) => {
                    let ok = match (&r.j1, &r.j2) {
                        (Some(a), Some(b)) => a.rounds == b.rounds,
                        _ => false,
                    };
                    if !ok {
                        return false;
                    }
                }
            }
            i += 1;
        }
        true
    }
}

// ── 注入核：坏登记样本矩阵 ────────────────────────────────────────────
/// 四类坏登记样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadEnroll {
    /// 拆册异构：J1 main、J2 batch
    CrossBookSplit,
    /// 结构漂移：双侧轮数不一致
    StructureMismatch,
    /// 同侧撞号
    DupSide,
    /// 缺反判据
    MissingCounterpart,
}

impl BadEnroll {
    /// 闭集全量。
    pub const ALL: [BadEnroll; 4] = [
        BadEnroll::CrossBookSplit,
        BadEnroll::StructureMismatch,
        BadEnroll::DupSide,
        BadEnroll::MissingCounterpart,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadEnroll::CrossBookSplit => "拆册异构",
            BadEnroll::StructureMismatch => "结构漂移",
            BadEnroll::DupSide => "同侧撞号",
            BadEnroll::MissingCounterpart => "缺反判据",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
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

    /// 记一次注入结果；账失守恒返回 RejectedTrace。
    pub fn record(&mut self, caught: bool) -> Result<(), DedupeCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(DedupeCode::RejectedTrace);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏登记注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_enroll_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadEnroll::ALL.len() {
        let case = match BadEnroll::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadEnroll::CrossBookSplit => {
                    let mut g = IdRegistry::new();
                    let fp = StructFp { aspect: String::from("read 面"), rounds: INJECT_ROUNDS };
                    let ok1 = g.enroll(1, BookKind::Main, true, &fp).is_ok();
                    ok1 && g.enroll(1, BookKind::Batch, false, &fp)
                        .err() == Some(DedupeCode::CrossBookSplit)
                }
                BadEnroll::StructureMismatch => {
                    let mut g = IdRegistry::new();
                    let f1 = StructFp { aspect: String::from("read 面"), rounds: INJECT_ROUNDS };
                    let f2 = StructFp { aspect: String::from("write 面"), rounds: 11 };
                    let ok1 = g.enroll(2, BookKind::Main, true, &f1).is_ok();
                    ok1 && g.enroll(2, BookKind::Main, false, &f2)
                        .err() == Some(DedupeCode::StructureMismatch)
                }
                BadEnroll::DupSide => {
                    let mut g = IdRegistry::new();
                    let fp = StructFp { aspect: String::from("open 面"), rounds: INJECT_ROUNDS };
                    let ok1 = g.enroll(3, BookKind::Deep, true, &fp).is_ok();
                    ok1 && g.enroll(3, BookKind::Deep, true, &fp)
                        .err() == Some(DedupeCode::DupSide)
                }
                BadEnroll::MissingCounterpart => {
                    let mut g = IdRegistry::new();
                    let fp = StructFp { aspect: String::from("pipe 面"), rounds: INJECT_ROUNDS };
                    let ok1 = g.enroll(4, BookKind::Main, true, &fp).is_ok();
                    ok1 && !g.paired(4) && !g.all_paired()
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 注册表成文（纯函数） ──────────────────────────────────────────────
/// 条目成文：「E<n>(<册>)=J1<有|无>/J2<有|无>;同构=<是|否>」；账外先拒。
pub fn compose_id_summary(g: &IdRegistry, id: u32) -> Result<String, DedupeCode> {
    if g.is_empty() {
        return Err(DedupeCode::UnknownId);
    }
    let mut i = 0usize;
    while i < g.recs.len() {
        if let Some(r) = g.recs.get(i) {
            if r.id == id {
                let mark = |o: &Option<StructFp>| if o.is_some() { "有" } else { "无" };
                let same = match (&r.j1, &r.j2) {
                    (Some(a), Some(b)) => a.rounds == b.rounds,
                    _ => false,
                };
                let out = format!("E{}({})=J1{}/J2{};同构={}",
                    r.id, r.book.label(), mark(&r.j1), mark(&r.j2),
                    if same { "是" } else { "否" });
                if out.is_empty() {
                    return Err(DedupeCode::UnknownId);
                }
                return Ok(out);
            }
        }
        i += 1;
    }
    Err(DedupeCode::UnknownId)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；同侧撞号族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；非法指纹/未知册别族"),
    ("EXDEV", -18, 17, "Windows ERROR_NOT_SAME_DEVICE（近似）；拆册异构族"),
    ("ENOLINK", -67, 111, "Windows ERROR_REM_NOT_LIST（近似）；缺反判据断链族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；注册表满族"),
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

/// 规范语料双跑指纹：三 ID 双侧登记 → 逐条成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut g = IdRegistry::new();
    let spec: [(u32, BookKind, &str, &str); 3] = [
        (11, BookKind::Main, "read 面@10", "坏 fd 面@10"),
        (12, BookKind::Batch, "write 面@10", "只读 fd 面@10"),
        (13, BookKind::Deep, "open 面@10", "缺路径 面@10"),
    ];
    let mut si = 0usize;
    while si < spec.len() {
        if let Some(row) = spec.get(si) {
            let f1 = parse_struct_fp(row.2).unwrap_or(StructFp {
                aspect: String::new(),
                rounds: 0,
            });
            let f2 = parse_struct_fp(row.3).unwrap_or(StructFp {
                aspect: String::new(),
                rounds: 0,
            });
            let _ = g.enroll(row.0, row.1, true, &f1);
            let _ = g.enroll(row.0, row.1, false, &f2);
        }
        si += 1;
    }
    let mut bytes: Vec<u8> = Vec::new();
    let mut e = 0u32;
    while e < 3 {
        match compose_id_summary(&g, 11 + e) {
            Ok(s) => {
                bytes.extend_from_slice(s.as_bytes());
                bytes.push(b';');
            }
            Err(_) => {}
        }
        e += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10096 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10096_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10096");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x927C0..=0x927FF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 册别解析器：三册闭集全通过，闭集外 10/10 拒绝
    let parse_ok = parse_book("main") == Ok(BookKind::Main)
        && parse_book("batch") == Ok(BookKind::Batch)
        && parse_book("deep") == Ok(BookKind::Deep);
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_book("Main").is_ok() || parse_book("").is_ok()
            || parse_book("total").is_ok() || parse_book("deep ").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("册别解析器：main|batch|deep 全通过且闭集外 10/10 拒绝", parse_ok && amb_ok,
          "册别歧义必须逐次显性拒绝，不许静默归并到相邻册");

    // 3. 结构指纹解析器：面@轮数 全解析
    let fp_ok = match parse_struct_fp("read 面@10") {
        Ok(f) => f.aspect == "read 面" && f.rounds == 10,
        Err(_) => false,
    } && parse_struct_fp("@10").is_err()
        && parse_struct_fp("面@").is_err()
        && parse_struct_fp("面@x").is_err()
        && parse_struct_fp("面").is_err();
    s.add("结构指纹解析器：面@<n> 全解析且缺面/缺轮 10/10 拒绝", fp_ok,
          "结构指纹是同构判定的唯一依据，缺项即拒");

    // 4. 双侧登记合法路径：先 J1 后 J2 同册同构
    let mut g = IdRegistry::new();
    let f1 = StructFp { aspect: String::from("read 面"), rounds: INJECT_ROUNDS };
    let f2 = StructFp { aspect: String::from("坏 fd 面"), rounds: INJECT_ROUNDS };
    let enroll_ok = g.enroll(11, BookKind::Main, true, &f1).is_ok()
        && g.enroll(11, BookKind::Main, false, &f2).is_ok()
        && g.paired(11) && g.all_paired() && g.all_same_shape();
    s.add("双侧登记：先 J1 后 J2 同册同构成对成功", enroll_ok,
          "同 ID 双侧必须同册同构，缺一侧即不成对");

    // 5. 注入核：四类坏登记样本各 10/10 拒绝
    let (inj, per_class) = run_bad_enroll_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏登记样本各 10/10 拒绝", all_caught,
          "拆册异构/结构漂移/同侧撞号/缺反判据必须逐类逐次拒绝");

    // 6. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadEnroll::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 7. 拆册异构拦截（独立复测）：同 ID 换册登记被拒且原册不变
    let mut g2 = IdRegistry::new();
    let _ = g2.enroll(21, BookKind::Batch, true, &f1);
    let split_ok = g2.enroll(21, BookKind::Deep, false, &f2)
        .err() == Some(DedupeCode::CrossBookSplit)
        && !g2.paired(21);
    s.add("拆册拦截：换册补侧被拒且原条目不成对", split_ok,
          "拆册即防重违规（DM 通道），必须拒绝且不留痕");

    // 8. 结构漂移拦截：补侧轮数与先登侧不一致
    let f8 = StructFp { aspect: String::from("坏 fd 面"), rounds: 11 };
    let mut g3 = IdRegistry::new();
    let _ = g3.enroll(31, BookKind::Main, true, &f1);
    let drift_ok = g3.enroll(31, BookKind::Main, false, &f8)
        .err() == Some(DedupeCode::StructureMismatch);
    s.add("漂移拦截：补侧轮数与先登侧不一致被拒", drift_ok,
          "同构 = 双侧同轮数口径（10/10），漂移即混口径");

    // 9. 轮数下限：低于 INJECT_ROUNDS 的指纹直接拒绝
    let f3 = StructFp { aspect: String::from("read 面"), rounds: 3 };
    let low_ok = g3.enroll(32, BookKind::Main, true, &f3).err() == Some(DedupeCode::ShortRounds)
        && g3.enroll(0, BookKind::Main, true, &f1).err() == Some(DedupeCode::ZeroId);
    s.add("下限守卫：轮数不达标准与零号 ID 显性拒绝", low_ok,
          "复测轮数必须达 INJECT_ROUNDS；零号是保留键");

    // 10. 全域审计：单侧条目暴露、补齐后翻绿
    let mut g4 = IdRegistry::new();
    let _ = g4.enroll(41, BookKind::Main, true, &f1);
    let audit_red = !g4.all_paired();
    let _ = g4.enroll(41, BookKind::Main, false, &f2);
    let audit_ok = audit_red && g4.all_paired() && g4.all_same_shape();
    s.add("全域审计：单侧条目暴露、双侧补齐后翻绿", audit_ok,
          "缺反判据 = 0 条是防重收口前置，审计逐 ID 核对");

    // 11. 拒绝不留痕：撞号被拒后先登侧不变
    let mut g5 = IdRegistry::new();
    let _ = g5.enroll(51, BookKind::Main, true, &f1);
    let before = g5.len();
    let _ = g5.enroll(51, BookKind::Main, true, &f1);
    let trace_ok = g5.len() == before && !g5.paired(51);
    s.add("拒绝不留痕：撞号被拒不改注册表", trace_ok,
          "被拒登记不得改动任何已有条目");

    // 12. 成文非空且含册别与双侧标记 + 双跑一致
    let sum = compose_id_summary(&g, 11);
    let text_ok = match &sum {
        Ok(t) => t.contains("(main)") && t.contains("J1有/J2有") && t.contains("同构=是"),
        Err(_) => false,
    };
    s.add("条目成文：册别/双侧/同构齐备且两次成文逐字节一致",
          text_ok && sum == compose_id_summary(&g, 11),
          "成文必须携带 册别/双侧/同构 标记且为纯函数");

    // 13. 账外成文拒绝
    s.add("账外成文拒绝：未知 ID 不得产出小结",
          compose_id_summary(&g, 999).err() == Some(DedupeCode::UnknownId),
          "空表与账外 ID 都无对账对象，产出小结即虚报");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(17) && agrees_with_linux(22) && agrees_with_linux(18)
        && agrees_with_linux(67) && agrees_with_linux(28)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：13 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < DedupeCode::ALL.len() {
        let mut j = i + 1;
        while j < DedupeCode::ALL.len() {
            if let (Some(a), Some(b)) = (DedupeCode::ALL.get(i), DedupeCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = DedupeCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：13 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 册别往返 + 双跑指纹
    let rt_ok = BookKind::ALL.iter().all(|b| parse_book(b.label()) == Ok(*b));
    let f1v = corpus_fingerprint();
    let f2v = corpus_fingerprint();
    s.add("往返与指纹：册别标签可回解析且双跑哈希相等", rt_ok && f1v == f2v,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1v);
    s.add_dyn("指纹快照", f1v == f2v, &fp_text);
    let sum_len = match &sum { Ok(t) => t.len(), Err(_) => 0 };
    let sum_text = format!("条目小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn fp(aspect: &str, rounds: u32) -> StructFp {
        StructFp { aspect: String::from(aspect), rounds }
    }

    #[test]
    fn book_closed_set() {
        assert_eq!(parse_book("batch"), Ok(BookKind::Batch));
        assert_eq!(parse_book("").err(), Some(DedupeCode::UnknownBook));
        assert_eq!(parse_book("Main").err(), Some(DedupeCode::UnknownBook));
    }

    #[test]
    fn struct_fp_parser() {
        let f = parse_struct_fp("write 面@10").unwrap_or(fp("", 0));
        assert_eq!(f.rounds, 10);
        assert!(parse_struct_fp("面@0").is_err());
        assert!(parse_struct_fp("noseparator").is_err());
    }

    #[test]
    fn pairing_and_audit() {
        let mut g = IdRegistry::new();
        let _ = g.enroll(1, BookKind::Main, true, &fp("a", 10));
        assert!(!g.all_paired());
        let _ = g.enroll(1, BookKind::Main, false, &fp("b", 10));
        assert!(g.paired(1) && g.all_paired() && g.all_same_shape());
    }

    #[test]
    fn cross_book_rejected() {
        let mut g = IdRegistry::new();
        let _ = g.enroll(2, BookKind::Main, true, &fp("a", 10));
        assert_eq!(g.enroll(2, BookKind::Batch, false, &fp("b", 10)).err(),
                   Some(DedupeCode::CrossBookSplit));
        assert!(!g.paired(2));
    }

    #[test]
    fn shape_drift_rejected() {
        let mut g = IdRegistry::new();
        let _ = g.enroll(3, BookKind::Deep, true, &fp("a", 10));
        assert_eq!(g.enroll(3, BookKind::Deep, false, &fp("b", 11)).err(),
                   Some(DedupeCode::StructureMismatch));
    }

    #[test]
    fn dup_side_rejected() {
        let mut g = IdRegistry::new();
        let _ = g.enroll(4, BookKind::Main, true, &fp("a", 10));
        assert_eq!(g.enroll(4, BookKind::Main, true, &fp("a", 10)).err(),
                   Some(DedupeCode::DupSide));
    }

    #[test]
    fn zero_id_and_short_rounds() {
        let mut g = IdRegistry::new();
        assert_eq!(g.enroll(0, BookKind::Main, true, &fp("a", 10)).err(),
                   Some(DedupeCode::ZeroId));
        assert_eq!(g.enroll(5, BookKind::Main, true, &fp("a", 1)).err(),
                   Some(DedupeCode::ShortRounds));
    }

    #[test]
    fn bad_enroll_matrix_green() {
        let (inj, per) = run_bad_enroll_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn summary_marks_unpaired() {
        let mut g = IdRegistry::new();
        let _ = g.enroll(6, BookKind::Batch, true, &fp("a", 10));
        let s = compose_id_summary(&g, 6).unwrap_or_default();
        assert!(s.contains("J2无"));
        assert!(s.contains("同构=否"));
        assert!(compose_id_summary(&g, 99).is_err());
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(18));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(67), -67);
    }

    #[test]
    fn unx10096_checks_green() {
        let set = run_unx10096_checks();
        assert!(set.all_passed(), "UNX-F10096 red: {}", set.red_items_text());
    }
}
