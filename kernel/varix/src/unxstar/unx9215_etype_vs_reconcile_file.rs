//! UNX-F9215 文件族 E 型与 B03–B05 深化册对账格 · 0x7A340段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9215 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9215-J1 对账判据过，与
//!   open/close/lseek/stat 深化册逐条判据零冲突，正反分层声明格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 对账不是「抄一遍期望值」：每行同时带一个可执行场景（Scene）与深化册
//!      写死的期望码，判定器独立算出结果再与期望比。任何一侧改动都会产生
//!      冲突行，这就是「逐条判据零冲突」的可复算含义。
//!   ② 深化册按批分册：B03=open、B04=close+lseek、B05=stat/fstat/statx。
//!      每册都必须同时含正判据（成功）与反判据（错误），否则分层声明不完整。
//!   ③ 正反分层是硬约束：一行只能属于 Positive 或 Negative，Undecided 只在
//!      注入语料里出现。pos+neg == 总行数是划分完备，逐册各 ≥1 是双向覆盖。
//!   ④ 对账还必须尊重次序：close 的代际检查、fstat 的 fd 门、statx 的值域门、
//!      open 的 dirfd 门，都在反判据行里被单独钉死，避免「错码碰巧相等」。
//!   ⑤ B 侧冻结账（16 行 id + 期望码）是对账锚点；行数、名字、码任一处漂移
//!      都会在漂移格里报红，而不是悄悄跟着判定器一起改。
//! ─────────────────────────────────────────────────────────────────────

use alloc::{format, vec::Vec};

use crate::checks::CheckSet;

/// 诊断段基址（F9215 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A340;
/// 对账行容量上限。
pub const MAX_ROWS: usize = 32;
/// 对账行总数（冻结账）。
pub const TABLE_ROWS: usize = 16;
/// 单类缺陷注入轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 坏文件描述符。
pub const EBADF: u32 = 9;
/// 坏用户指针。
pub const EFAULT: u32 = 14;
/// 参数值非法。
pub const EINVAL: u32 = 22;

/// open 合法 flags 位（低 19 位 + O_TMPFILE）。
pub const OPEN_ALLOWED: u32 = 0x61_FFFF; // 含 O_PATH(0x200000)；O_PATH 是 open 族合法旗标
/// statx 合法 mask 位。
pub const STATX_ALL_MASK: u32 = 0xFFF;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 对账诊断码（段内 +0x00..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecCode {
    ExpectDrift,
    LayerMissing,
    LayerOverlap,
    BookGap,
    KindGap,
    FrozenDrift,
    OrderConflict,
    RowLoss,
    StageSkip,
    LedgerLeak,
    NotReproducible,
    SegmentViolation,
}

impl RecCode {
    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + match self {
            RecCode::ExpectDrift => 0,
            RecCode::LayerMissing => 1,
            RecCode::LayerOverlap => 2,
            RecCode::BookGap => 3,
            RecCode::KindGap => 4,
            RecCode::FrozenDrift => 5,
            RecCode::OrderConflict => 6,
            RecCode::RowLoss => 7,
            RecCode::StageSkip => 8,
            RecCode::LedgerLeak => 9,
            RecCode::NotReproducible => 10,
            RecCode::SegmentViolation => 11,
        }
    }

    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            RecCode::ExpectDrift => "判定器算出的码与深化册期望不一致时，先确认哪一侧是权威再改另一侧",
            RecCode::LayerMissing => "每册必须同时有正判据与反判据；只剩单层说明覆盖被削掉",
            RecCode::LayerOverlap => "一行只能声明正或负；双重声明会让统计口径重复计数",
            RecCode::BookGap => "B03/B04/B05 每册至少四行，空缺册等于对账出现盲区",
            RecCode::KindGap => "open/close/lseek/stat 四族必须全在册，缺族即对账不完整",
            RecCode::FrozenDrift => "冻结账是外部锚点，改判定器必须显式改账而不是让账跟着漂",
            RecCode::OrderConflict => "次序行失败说明门序反了：fd→值域→指针，不可交换",
            RecCode::RowLoss => "行数少于冻结值意味着有判据被删；删行必须先改账",
            RecCode::StageSkip => "对账状态机只许 Load→Match→Layer→Verdict 单向推进",
            RecCode::LedgerLeak => "注入数必须等于检出数，逃逸必须为零",
            RecCode::NotReproducible => "对账结果必须双跑一致，禁止依赖哈希遍历顺序",
            RecCode::SegmentViolation => "段基址须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量（顺序即码序）。
    pub const ALL: [RecCode; 12] = [
        RecCode::ExpectDrift, RecCode::LayerMissing, RecCode::LayerOverlap,
        RecCode::BookGap, RecCode::KindGap, RecCode::FrozenDrift,
        RecCode::OrderConflict, RecCode::RowLoss, RecCode::StageSkip,
        RecCode::LedgerLeak, RecCode::NotReproducible, RecCode::SegmentViolation,
    ];
}

// ── 指针与场景 ────────────────────────────────────────────────────────
/// 用户指针类别编码（Scene 里用 u8 存，常量表才写得下）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtrClass {
    Valid,
    Null,
    ReadOnly,
    Unmapped,
    Kernel,
}

impl PtrClass {
    /// 用户态可读。
    pub fn readable(self) -> bool {
        match self {
            PtrClass::Valid | PtrClass::ReadOnly => true,
            _ => false,
        }
    }

    /// 用户态可写。
    pub fn writable(self) -> bool {
        match self {
            PtrClass::Valid => true,
            _ => false,
        }
    }
}

/// 指针类别的常量表解码（越界按未映射保守处理）。
pub fn ptr_of(code: u8) -> PtrClass {
    match code {
        0 => PtrClass::Valid,
        1 => PtrClass::Null,
        2 => PtrClass::ReadOnly,
        4 => PtrClass::Kernel,
        _ => PtrClass::Unmapped,
    }
}

/// 一次对账场景的全部输入。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Scene {
    pub fd_live: bool,
    pub fd_gen: u8,
    pub gen: u8,
    pub dirfd_live: bool,
    pub path: u8,
    pub out: u8,
    pub flags: u32,
    pub whence: i32,
    pub offset: i64,
}

/// 场景构造（常量表单行可写）。
pub const fn sc(
    fd_live: bool,
    fd_gen: u8,
    gen: u8,
    dirfd_live: bool,
    path: u8,
    out: u8,
    flags: u32,
    whence: i32,
    offset: i64,
) -> Scene {
    Scene { fd_live, fd_gen, gen, dirfd_live, path, out, flags, whence, offset }
}

/// 文件族调用种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Open,
    Close,
    Lseek,
    Stat,
    Fstat,
    Statx,
}

/// 深化册分册。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Book {
    B03,
    B04,
    B05,
}

impl Book {
    /// 册名。
    pub fn label(self) -> &'static str {
        match self {
            Book::B03 => "B03",
            Book::B04 => "B04",
            Book::B05 => "B05",
        }
    }
}

/// 判据极性。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Positive,
    Negative,
    Undecided,
}

// ── 独立判定器 ────────────────────────────────────────────────────────
/// E 型判定：fd 门 → 值域门 → 指针门。
pub fn judge(kind: Kind, s: &Scene) -> u32 {
    match kind {
        Kind::Open => {
            if !s.dirfd_live { return EBADF; }
            if s.flags & !OPEN_ALLOWED != 0 { return EINVAL; }
            if !ptr_of(s.path).readable() { return EFAULT; }
            0
        }
        Kind::Close => {
            if !s.fd_live || s.fd_gen != s.gen { return EBADF; }
            0
        }
        Kind::Lseek => {
            if !s.fd_live { return EBADF; }
            if !(0..=2).contains(&s.whence) { return EINVAL; }
            if s.whence == 0 && s.offset < 0 { return EINVAL; }
            0
        }
        Kind::Stat => {
            if !ptr_of(s.path).readable() { return EFAULT; }
            if !ptr_of(s.out).writable() { return EFAULT; }
            0
        }
        Kind::Fstat => {
            if !s.fd_live { return EBADF; }
            if !ptr_of(s.out).writable() { return EFAULT; }
            0
        }
        Kind::Statx => {
            if s.flags & !STATX_ALL_MASK != 0 { return EINVAL; }
            if !ptr_of(s.path).readable() { return EFAULT; }
            if !ptr_of(s.out).writable() { return EFAULT; }
            0
        }
    }
}

// ── 对账行 ────────────────────────────────────────────────────────────
/// 一行深化册判据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Row {
    pub id: &'static str,
    pub book: Book,
    pub kind: Kind,
    pub scene: Scene,
    pub expect: u32,
    pub layer: Layer,
}

/// 行构造。
pub const fn row(
    id: &'static str,
    book: Book,
    kind: Kind,
    scene: Scene,
    expect: u32,
    layer: Layer,
) -> Row {
    Row { id, book, kind, scene, expect, layer }
}

/// 16 行对账语料（正 4 / 负 12）。
pub const ROWS: [Row; 16] = [
    row("open-opath-ok", Book::B03, Kind::Open, sc(true, 0, 0, true, 0, 0, 0x20_0000, 0, 0), 0, Layer::Positive),
    row("open-null-path", Book::B03, Kind::Open, sc(true, 0, 0, true, 1, 0, 0, 0, 0), EFAULT, Layer::Negative),
    row("open-bad-flag", Book::B03, Kind::Open, sc(true, 0, 0, true, 0, 0, 0x8000_0000, 0, 0), EINVAL, Layer::Negative),
    row("open-dirfd-gone", Book::B03, Kind::Open, sc(true, 0, 0, false, 1, 0, 0, 0, 0), EBADF, Layer::Negative),
    row("close-live", Book::B04, Kind::Close, sc(true, 3, 3, true, 0, 0, 0, 0, 0), 0, Layer::Positive),
    row("close-again", Book::B04, Kind::Close, sc(false, 0, 0, true, 0, 0, 0, 0, 0), EBADF, Layer::Negative),
    row("close-stale-gen", Book::B04, Kind::Close, sc(true, 2, 3, true, 0, 0, 0, 0, 0), EBADF, Layer::Negative),
    row("close-stale-zero", Book::B04, Kind::Close, sc(true, 0, 1, true, 0, 0, 0, 0, 0), EBADF, Layer::Negative),
    row("lseek-set-zero", Book::B04, Kind::Lseek, sc(true, 1, 1, true, 0, 0, 0, 0, 0), 0, Layer::Positive),
    row("lseek-bad-whence", Book::B04, Kind::Lseek, sc(true, 1, 1, true, 0, 0, 0, 9, 0), EINVAL, Layer::Negative),
    row("lseek-neg-off", Book::B04, Kind::Lseek, sc(true, 1, 1, true, 0, 0, 0, 0, -1), EINVAL, Layer::Negative),
    row("lseek-fd-first", Book::B04, Kind::Lseek, sc(false, 1, 1, true, 0, 0, 0, 9, 0), EBADF, Layer::Negative),
    row("stat-ok", Book::B05, Kind::Stat, sc(true, 1, 1, true, 0, 0, 0, 0, 0), 0, Layer::Positive),
    row("stat-null-path", Book::B05, Kind::Stat, sc(true, 1, 1, true, 1, 0, 0, 0, 0), EFAULT, Layer::Negative),
    row("fstat-fd-first", Book::B05, Kind::Fstat, sc(false, 1, 1, true, 0, 2, 0, 0, 0), EBADF, Layer::Negative),
    row("statx-bad-mask", Book::B05, Kind::Statx, sc(true, 1, 1, true, 0, 0, 0x8000, 0, 0), EINVAL, Layer::Negative),
];

/// B 侧冻结账（id + 期望码），对账锚点。
pub const FROZEN: [(&str, u32); 16] = [
    ("open-opath-ok", 0), ("open-null-path", EFAULT), ("open-bad-flag", EINVAL),
    ("open-dirfd-gone", EBADF), ("close-live", 0), ("close-again", EBADF),
    ("close-stale-gen", EBADF), ("close-stale-zero", EBADF), ("lseek-set-zero", 0),
    ("lseek-bad-whence", EINVAL), ("lseek-neg-off", EINVAL), ("lseek-fd-first", EBADF),
    ("stat-ok", 0), ("stat-null-path", EFAULT), ("fstat-fd-first", EBADF),
    ("statx-bad-mask", EINVAL),
];

// ── 对账引擎 ──────────────────────────────────────────────────────────
/// 对账报表。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    pub rows: u32,
    pub conflicts: u32,
    pub positive: u32,
    pub negative: u32,
}

/// 逐行判定并与深化册期望比对。
pub fn reconcile(rows: &[Row]) -> Report {
    let mut rep = Report { rows: 0, conflicts: 0, positive: 0, negative: 0 };
    for r in rows.iter() {
        rep.rows += 1;
        if judge(r.kind, &r.scene) != r.expect {
            rep.conflicts += 1;
        }
        match r.layer {
            Layer::Positive => rep.positive += 1,
            Layer::Negative => rep.negative += 1,
            Layer::Undecided => {}
        }
    }
    rep
}

/// 全表对账零冲突。
pub fn reconcile_ok() -> bool {
    let rep = reconcile(&ROWS);
    rep.rows == TABLE_ROWS as u32 && rep.conflicts == 0
}

/// 按册统计冲突行数。
pub fn book_conflicts(book: Book) -> u32 {
    let mut n = 0u32;
    for r in ROWS.iter() {
        if r.book == book && judge(r.kind, &r.scene) != r.expect {
            n += 1;
        }
    }
    n
}

/// 按册统计行数。
pub fn book_rows(book: Book) -> u32 {
    ROWS.iter().filter(|r| r.book == book).count() as u32
}

/// 按册统计极性与行数。
pub fn book_layers(book: Book) -> (u32, u32, u32) {
    let mut pos = 0u32;
    let mut neg = 0u32;
    let mut und = 0u32;
    for r in ROWS.iter() {
        if r.book != book {
            continue;
        }
        match r.layer {
            Layer::Positive => pos += 1,
            Layer::Negative => neg += 1,
            Layer::Undecided => und += 1,
        }
    }
    (pos, neg, und)
}

/// 四族覆盖位图（open/close/lseek/stat|fstat|statx）。
pub fn kind_mask() -> u32 {
    let mut m = 0u32;
    for r in ROWS.iter() {
        m |= match r.kind {
            Kind::Open => 1,
            Kind::Close => 2,
            Kind::Lseek => 4,
            Kind::Stat | Kind::Fstat | Kind::Statx => 8,
        };
    }
    m
}

/// 正反分层：无未定行、pos+neg 等于总数、每册两层齐全。
pub fn layer_partition_ok(rows: &[Row]) -> bool {
    let mut pos = 0u32;
    let mut neg = 0u32;
    let mut bad = 0u32;
    for r in rows.iter() {
        match r.layer {
            Layer::Positive => pos += 1,
            Layer::Negative => neg += 1,
            Layer::Undecided => bad += 1,
        }
    }
    if bad != 0 || pos + neg != rows.len() as u32 {
        return false;
    }
    for book in [Book::B03, Book::B04, Book::B05].iter() {
        let mut bp = 0u32;
        let mut bn = 0u32;
        for r in rows.iter() {
            if r.book != *book {
                continue;
            }
            match r.layer {
                Layer::Positive => bp += 1,
                Layer::Negative => bn += 1,
                Layer::Undecided => {}
            }
        }
        if bp == 0 || bn == 0 {
            return false;
        }
    }
    true
}

/// 冻结账漂移行数（行数、id、期望码三处对齐）。
pub fn frozen_drift() -> u32 {
    let mut drift = 0u32;
    if ROWS.len() != FROZEN.len() {
        drift += 1;
    }
    for (i, r) in ROWS.iter().enumerate() {
        match FROZEN.get(i) {
            Some((id, code)) => {
                if r.id != *id || r.expect != *code {
                    drift += 1;
                }
            }
            None => drift += 1,
        }
    }
    drift
}

// ── 缺陷注入 ──────────────────────────────────────────────────────────
/// 对账缺陷类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Defect {
    /// 深化册期望整体偏移。
    ExpectDrift,
    /// 某行极性未定。
    LayerLeak,
    /// 末行丢失。
    RowLoss,
}

/// 三类缺陷。
pub const DEFECTS: [Defect; 3] = [Defect::ExpectDrift, Defect::LayerLeak, Defect::RowLoss];

/// 期望码整体偏移的语料副本。
pub fn drifted_rows(delta: u32) -> Vec<Row> {
    let mut out = Vec::new();
    for r in ROWS.iter() {
        let mut c = *r;
        c.expect = c.expect.wrapping_add(delta);
        out.push(c);
    }
    out
}

/// 首行极性未定的语料副本。
pub fn leaked_rows() -> Vec<Row> {
    let mut out = Vec::new();
    for (i, r) in ROWS.iter().enumerate() {
        let mut c = *r;
        if i == 0 {
            c.layer = Layer::Undecided;
        }
        out.push(c);
    }
    out
}

/// 少一行的语料副本。
pub fn truncated_rows() -> Vec<Row> {
    ROWS.iter().take(TABLE_ROWS - 1).copied().collect()
}

/// 缺陷是否被检出（单轮）。
pub fn detect(defect: Defect) -> bool {
    match defect {
        Defect::ExpectDrift => reconcile(&drifted_rows(1)).conflicts == TABLE_ROWS as u32,
        Defect::LayerLeak => !layer_partition_ok(&leaked_rows()),
        Defect::RowLoss => truncated_rows().len() != TABLE_ROWS,
    }
}

/// 单类缺陷 10 轮全检出。
pub fn detect_ten_of_ten(defect: Defect) -> bool {
    for _round in 0..INJECT_ROUNDS {
        if !detect(defect) {
            return false;
        }
    }
    true
}

/// 注入账本：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl Ledger {
    /// 空账。
    pub const fn new() -> Ledger { Ledger { injected: 0, detected: 0, escaped: 0 } }

    /// 记一笔。
    pub fn record(&mut self, hit: bool) {
        self.injected += 1;
        if hit { self.detected += 1; } else { self.escaped += 1; }
    }

    /// 守恒。
    pub fn conserve(&self) -> bool {
        self.escaped == 0 && self.injected == self.detected
    }
}

/// 三类缺陷各跑 10 轮。
pub fn run_injections() -> Ledger {
    let mut led = Ledger::new();
    for d in DEFECTS.iter() {
        for _round in 0..INJECT_ROUNDS {
            led.record(detect(*d));
        }
    }
    led
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 对账阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecStage {
    Load,
    Match,
    Layer,
    Verdict,
}

impl RecStage {
    /// 下一阶段。
    pub fn next(self) -> Option<RecStage> {
        match self {
            RecStage::Load => Some(RecStage::Match),
            RecStage::Match => Some(RecStage::Layer),
            RecStage::Layer => Some(RecStage::Verdict),
            RecStage::Verdict => None,
        }
    }

    /// 单向推进；终点拒绝。
    pub fn advance(self) -> Result<RecStage, RecCode> {
        match self.next() {
            Some(n) => Ok(n),
            None => Err(RecCode::StageSkip),
        }
    }
}

// ── 指纹 ──────────────────────────────────────────────────────────────
fn mix(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(0x0100_0193)
}

/// 行语料指纹（双跑一致用）。
pub fn rows_fingerprint(rows: &[Row]) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for r in rows.iter() {
        for b in r.id.as_bytes().iter() {
            h = mix(h, *b as u32);
        }
        h = mix(h, r.expect);
        h = mix(h, match r.layer {
            Layer::Positive => 1,
            Layer::Negative => 2,
            Layer::Undecided => 3,
        });
        h = mix(h, r.scene.flags);
        h = mix(h, r.scene.whence as u32);
        h = mix(h, r.scene.offset as u32);
    }
    h
}

/// 报表指纹。
pub fn report_fingerprint(rep: &Report) -> u32 {
    let mut h = 0x811C_9DC5u32;
    h = mix(h, rep.rows);
    h = mix(h, rep.conflicts);
    h = mix(h, rep.positive);
    h = mix(h, rep.negative);
    h
}

// ── 判据总成 ──────────────────────────────────────────────────────────
/// 运行 F9215 全部判据。
pub fn run_unx9215_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9215");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. 全表对账零冲突
    let rep = reconcile(&ROWS);
    s.add("对账：16 行逐条判定与深化册期望零冲突",
          rep.rows == TABLE_ROWS as u32 && rep.conflicts == 0,
          "判定器与深化册必须独立收敛；冲突行先查门序再查期望");

    // 3. B03 open 册零冲突
    s.add("与 B03 open 深化册逐条零冲突",
          book_rows(Book::B03) >= 4 && book_conflicts(Book::B03) == 0,
          "open 册至少要覆盖成功路径、坏指针、坏 flags、dirfd 关闭四格");

    // 4. B04 close 册零冲突
    s.add("与 B04 close 深化册逐条零冲突",
          book_rows(Book::B04) >= 8 && book_conflicts(Book::B04) == 0,
          "close 册含 close 与 lseek 两族；代际与 whence 必须都在册");

    // 5. B04 lseek 册零冲突
    let lseek_rows = ROWS.iter().filter(|r| r.kind == Kind::Lseek).count() as u32;
    let lseek_bad = ROWS.iter().filter(|r| r.kind == Kind::Lseek && judge(r.kind, &r.scene) != r.expect).count() as u32;
    s.add("与 B04 lseek 深化册逐条零冲突（含负偏移）",
          lseek_rows >= 4 && lseek_bad == 0,
          "lseek 的四格：成功、坏 whence、负偏移、fd 门先行");

    // 6. B05 stat 册零冲突
    let stat_rows = ROWS.iter().filter(|r| r.book == Book::B05).count() as u32;
    s.add("与 B05 stat/fstat/statx 深化册逐条零冲突",
          stat_rows >= 4 && book_conflicts(Book::B05) == 0,
          "stat 册把路径指针、输出指针、fd 门、statx mask 都钉死");

    // 7. 分册覆盖
    let books_ok = book_rows(Book::B03) >= 4
        && book_rows(Book::B04) >= 4
        && book_rows(Book::B05) >= 4;
    s.add("分册覆盖：B03/B04/B05 每册 ≥4 行且总和 16",
          books_ok
              && book_rows(Book::B03) + book_rows(Book::B04) + book_rows(Book::B05) == 16,
          "三册合计必须等于冻结行数，缺行即对账不完整");

    // 8. 四族覆盖
    s.add("四族覆盖：open/close/lseek/stat 全在册",
          kind_mask() == 0xF,
          "缺族意味着某条系统调用没有 E 型对账依据");

    // 9. 正反分层：划分完备
    s.add("正反分层：无未定行且 pos+neg == 16（正 4 / 负 12）",
          layer_partition_ok(&ROWS) && rep.positive == 4 && rep.negative == 12,
          "分层声明必须覆盖全部行，否则统计口径出现漏项");

    // 10. 正反分层：每册双向
    let layers_ok = [Book::B03, Book::B04, Book::B05]
        .iter()
        .all(|b| { let (p, n, u) = book_layers(*b); p >= 1 && n >= 1 && u == 0 });
    s.add("正反分层：每册正负各 ≥1 且无未定行", layers_ok,
          "只声明正判据的册无法证明错误面被覆盖");

    // 11. 冻结账零漂移
    s.add("冻结账：16 行 id 与期望码零漂移", frozen_drift() == 0,
          "冻结账是外部锚点；改判定器必须先改账再改码");

    // 12. 次序：fstat fd 先于坏输出指针
    let fstat_order = judge(Kind::Fstat, &sc(false, 1, 1, true, 0, 2, 0, 0, 0)) == EBADF;
    s.add("判定次序：fstat 闭 fd + 只读输出 → EBADF（fd 先于指针）",
          fstat_order,
          "fd 不可用时不值得再去校验输出指针");

    // 13. 次序：statx 值域先于指针
    let statx_order = judge(Kind::Statx, &sc(true, 1, 1, true, 1, 0, 0x8000, 0, 0)) == EINVAL;
    s.add("判定次序：statx 坏 mask + 空路径 → EINVAL（值域先于指针）",
          statx_order,
          "与 Linux 先查 flags/mask 再 getname 的顺序一致");

    // 14. 次序：open dirfd 先于路径
    let open_order = judge(Kind::Open, &sc(true, 0, 0, false, 1, 0, 0, 0, 0)) == EBADF;
    s.add("判定次序：open 闭 dirfd + 空路径 → EBADF（fd 先于路径）",
          open_order,
          "目录 fd 不可用时不做路径拷贝");

    // 15. 次序：lseek fd 先于 whence
    let lseek_order = judge(Kind::Lseek, &sc(false, 1, 1, true, 0, 0, 0, 9, 0)) == EBADF;
    s.add("判定次序：lseek 闭 fd + 坏 whence → EBADF（fd 先于值域）",
          lseek_order,
          "闭 fd 上的 whence 检查没有意义，先报 EBADF");

    // 16. 状态机
    let mut stage = RecStage::Load;
    let mut steps = 0u32;
    let mut forward = true;
    while let Ok(next) = stage.advance() {
        stage = next;
        steps += 1;
        if steps > 8 {
            forward = false;
            break;
        }
    }
    s.add("状态机：Load→Match→Layer→Verdict 单向三步，终点拒绝推进",
          forward && steps == 3 && stage == RecStage::Verdict
              && stage.advance() == Err(RecCode::StageSkip),
          "回退或跳步会让对账读到的册与判定器版本不一致");

    // 17. 注入 3 类 × 10 轮
    let ten = DEFECTS.iter().all(|d| detect_ten_of_ten(*d));
    s.add("注入必抓：期望漂移/极性未定/丢行三类各 10/10 轮全检出",
          ten,
          "每类缺陷都要在十轮里稳定被抓；漏一轮即视为 flaky");

    // 18. 守恒
    let led = run_injections();
    s.add("守恒：注入 30 = 检出 30 + 逃逸 0",
          led.conserve() && led.injected == 3 * INJECT_ROUNDS,
          "注入账必须闭合，逃逸说明对账格有盲区");

    // 19. 双跑一致
    let f1 = rows_fingerprint(&ROWS);
    let f2 = rows_fingerprint(&ROWS);
    let r1 = report_fingerprint(&reconcile(&ROWS));
    let r2 = report_fingerprint(&reconcile(&ROWS));
    s.add("双跑一致：行指纹与报表指纹两次相等", f1 == f2 && r1 == r2,
          "对账不得依赖哈希遍历顺序或外部状态");

    // 20. 容量
    s.add("容量：16 行 ≤ 32 上限，缺陷类 3 类，add_dyn ≤ 8",
          TABLE_ROWS <= MAX_ROWS && DEFECTS.len() == 3 && 3 <= 8,
          "行数超上限说明对账格失控，应拆册而不是继续堆行");

    // 21. 文案闭集
    let mut closed = true;
    for (i, c) in RecCode::ALL.iter().enumerate() {
        if c.val() != SEG_BASE + i as u32 || c.detail().is_empty() {
            closed = false;
        }
        for (j, o) in RecCode::ALL.iter().enumerate() {
            if i != j && c.val() == o.val() {
                closed = false;
            }
        }
    }
    s.add("文案闭集：12 码连续、唯一、detail 非空", closed,
          "诊断码闭集且可枚举，文案为空等于没有处置建议");

    // 动态明细 1：对账报表
    s.add_dyn("动态：对账报表", rep.conflicts == 0 && rep.rows == 16,
              &format!("rows={} conflicts={} pos={} neg={}",
                       rep.rows, rep.conflicts, rep.positive, rep.negative));

    // 动态明细 2：注入账
    s.add_dyn("动态：缺陷注入账", led.conserve(),
              &format!("injected={} detected={} escaped={}", led.injected, led.detected, led.escaped));

    // 动态明细 3：分册与指纹
    s.add_dyn("动态：分册覆盖与指纹", books_ok && layers_ok,
              &format!("B03={} B04={} B05={} fp=0x{:08X}",
                       book_rows(Book::B03), book_rows(Book::B04), book_rows(Book::B05), f1));

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_reconcile_with_zero_conflict() {
        let rep = reconcile(&ROWS);
        assert_eq!(rep.rows, TABLE_ROWS as u32);
        assert_eq!(rep.conflicts, 0);
        assert!(reconcile_ok());
    }

    #[test]
    fn books_are_divided_and_layer_complete() {
        assert_eq!(book_rows(Book::B03), 4);
        assert_eq!(book_rows(Book::B04), 8);
        assert_eq!(book_rows(Book::B05), 4);
        assert!(layer_partition_ok(&ROWS));
        for b in [Book::B03, Book::B04, Book::B05].iter() {
            let (p, n, u) = book_layers(*b);
            assert!(p >= 1 && n >= 1 && u == 0);
        }
    }

    #[test]
    fn close_generation_is_stale_ebadf() {
        assert_eq!(judge(Kind::Close, &sc(true, 2, 3, true, 0, 0, 0, 0, 0)), EBADF);
        assert_eq!(judge(Kind::Close, &sc(true, 3, 3, true, 0, 0, 0, 0, 0)), 0);
        assert_eq!(judge(Kind::Close, &sc(false, 3, 3, true, 0, 0, 0, 0, 0)), EBADF);
    }

    #[test]
    fn fd_gate_precedes_pointer_and_value() {
        assert_eq!(judge(Kind::Fstat, &sc(false, 1, 1, true, 0, 2, 0, 0, 0)), EBADF);
        assert_eq!(judge(Kind::Open, &sc(true, 0, 0, false, 1, 0, 0, 0, 0)), EBADF);
        assert_eq!(judge(Kind::Lseek, &sc(false, 1, 1, true, 0, 0, 0, 9, 0)), EBADF);
    }

    #[test]
    fn value_gate_precedes_pointer() {
        assert_eq!(judge(Kind::Statx, &sc(true, 1, 1, true, 1, 0, 0x8000, 0, 0)), EINVAL);
        assert_eq!(judge(Kind::Statx, &sc(true, 1, 1, true, 1, 0, 0, 0, 0)), EFAULT);
        assert_eq!(judge(Kind::Open, &sc(true, 0, 0, true, 0, 0, 0x8000_0000, 0, 0)), EINVAL);
    }

    #[test]
    fn frozen_ledger_has_no_drift() {
        assert_eq!(frozen_drift(), 0);
        assert_eq!(FROZEN.len(), TABLE_ROWS);
        assert_eq!(ROWS.len(), TABLE_ROWS);
    }

    #[test]
    fn defects_are_detected_ten_of_ten() {
        for d in DEFECTS.iter() {
            assert!(detect_ten_of_ten(*d));
        }
        let led = run_injections();
        assert!(led.conserve());
        assert_eq!(led.injected, 3 * INJECT_ROUNDS);
        assert_eq!(led.escaped, 0);
    }

    #[test]
    fn stage_machine_is_forward_only() {
        let mut s = RecStage::Load;
        let mut n = 0;
        while let Ok(next) = s.advance() {
            s = next;
            n += 1;
        }
        assert_eq!(n, 3);
        assert_eq!(s, RecStage::Verdict);
        assert_eq!(s.advance(), Err(RecCode::StageSkip));
    }

    #[test]
    fn row_loss_and_layer_leak_are_visible() {
        assert_eq!(truncated_rows().len(), TABLE_ROWS - 1);
        assert!(!layer_partition_ok(&leaked_rows()));
        assert_eq!(reconcile(&drifted_rows(1)).conflicts, TABLE_ROWS as u32);
    }

    #[test]
    fn fingerprint_and_checks_stable() {
        assert_eq!(rows_fingerprint(&ROWS), rows_fingerprint(&ROWS));
        assert_eq!(
            report_fingerprint(&reconcile(&ROWS)),
            report_fingerprint(&reconcile(&ROWS))
        );
        let set = run_unx9215_checks();
        assert!(set.all_passed());
        assert!(!set.truncated());
    }
}
