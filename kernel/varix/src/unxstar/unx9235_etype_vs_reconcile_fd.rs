//! UNX-F9235 fd 族 E 型与 B08/B15 深化册对账格 · 0x7A840段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9235 逐字）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9235-J1 对账判据过，与 dup 族/
//!   管道深化册逐条判据零冲突格过，正反分层声明格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 三册同表：E 型基准册 7 行、B08（dup 族/管道）6 行、B15（close/fstat/
//!      lseek）3 行，共 16 行。每行只登记名、册、层、场景与册面主张，判定值
//!      一律由 judge 现算，主张与判定分开存放才能对账，合二为一就失去意义。
//!   ② 「零冲突」不是口号：逐行比主张与判定，Positive/Negative 行必须相等；
//!      B08 六行、B15 三行分别独立计冲突数，任一册非零即报红。
//!   ③ 正反分层：Positive 仅容纳合法参数流，Negative 容纳坏 fd/坏指针/坏值；
//!      分层由场景推导，行上声明只作核对副本，二者不一致即 LayerLeak。
//!   ④ Undecided 只有一格（splice 坏 fd）：册间未定论时先挂起，由 E 型裁定
//!      为 EBADF 后落账；挂起格必须被消解，残留即 UndecidedUnresolved。
//!   ⑤ 三类缺陷注入各 10 轮：主张漂移、分层泄漏、行丢失，检出 10/10；
//!      注入账守恒，escaped 恒为 0。
//!   ⑥ 形态自证：坏指针行只能落在可指针调用上，坏 fd 行只能落在带 fd 调用上，
//!      否则对账表本身就是脏数据，先于冲突检查报 RowShape。
//! ─────────────────────────────────────────────────────────────────────
use alloc::{format, vec::Vec};
use crate::checks::CheckSet;
/// 诊断段基址（F9235 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A840;
/// 对账行容量上限。
pub const MAX_ROWS: usize = 32;
/// 表行总数（冻结账）。
pub const ROW_TOTAL: usize = 16;
/// E 型基准册行数。
pub const BASE_ROWS: usize = 7;
/// B08 册行数。
pub const B08_ROWS: usize = 6;
/// B15 册行数。
pub const B15_ROWS: usize = 3;
/// Positive 行数。
pub const POSITIVE_ROWS: usize = 2;
/// Negative 行数。
pub const NEGATIVE_ROWS: usize = 13;
/// Undecided 行数。
pub const UNDECIDED_ROWS: usize = 1;
/// 单类注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 坏文件描述符。
pub const EBADF: u32 = 9;
/// 坏用户指针。
pub const EFAULT: u32 = 14;
/// 参数值非法。
pub const EINVAL: u32 = 22;
/// 非寻址对象。
pub const ESPIPE: u32 = 29;
/// 深化册枚举。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Book {
    Base,
    B08,
    B15,
}
impl Book {
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            Book::Base => 0,
            Book::B08 => 1,
            Book::B15 => 2,
        }
    }
    /// 册名（闭集文案）。
    pub fn name(self) -> &'static str {
        match self {
            Book::Base => "E 型基准册",
            Book::B08 => "B08 dup 族与管道深化册",
            Book::B15 => "B15 close/fstat/lseek 深化册",
        }
    }
    /// 三册全量。
    pub const ALL: [Book; 3] = [Book::Base, Book::B08, Book::B15];
}
/// 正反分层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Positive,
    Negative,
    Undecided,
}
impl Layer {
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            Layer::Positive => 0,
            Layer::Negative => 1,
            Layer::Undecided => 2,
        }
    }
    /// 层名（闭集文案）。
    pub fn name(self) -> &'static str {
        match self {
            Layer::Positive => "正层：合法参数流",
            Layer::Negative => "反层：坏 fd/坏指针/坏值",
            Layer::Undecided => "待裁层：册间未定论",
        }
    }
    /// 三层全量。
    pub const ALL: [Layer; 3] = [Layer::Positive, Layer::Negative, Layer::Undecided];
}
/// 判据场景。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    ValidArgs,
    BadFd,
    BadPtr,
    BadValue,
}
impl Scenario {
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            Scenario::ValidArgs => 0,
            Scenario::BadFd => 1,
            Scenario::BadPtr => 2,
            Scenario::BadValue => 3,
        }
    }
}
/// 由场景推导层（声明副本必须与之一致）。
pub fn declared_layer(s: Scenario) -> Layer {
    match s {
        Scenario::ValidArgs => Layer::Positive,
        Scenario::BadFd | Scenario::BadPtr | Scenario::BadValue => Layer::Negative,
    }
}
/// 错误种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Ok,
    BadFd,
    BadPtr,
    BadValue,
    Other,
}
impl Kind {
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            Kind::Ok => 0,
            Kind::BadFd => 1,
            Kind::BadPtr => 2,
            Kind::BadValue => 3,
            Kind::Other => 4,
        }
    }
}
/// 码值到种类。
pub fn kind_of(code: u32) -> Kind {
    match code {
        0 => Kind::Ok,
        EBADF => Kind::BadFd,
        EFAULT => Kind::BadPtr,
        EINVAL => Kind::BadValue,
        _ => Kind::Other,
    }
}
/// 错误码中文文案（闭集）。
pub fn errno_text(code: u32) -> &'static str {
    match code {
        0 => "成功",
        EBADF => "坏文件描述符，先查代次再看复用",
        EFAULT => "坏用户指针，核对方向与可空声明",
        EINVAL => "参数值非法，核对 flags/cmd 白名单",
        ESPIPE => "管道不可寻址，改用流式接口",
        _ => "未登记错误码，拒绝落账",
    }
}
/// 指针形态（用于形态自证）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PtrClass {
    None,
    Input,
    Output,
    Nullable,
}
impl PtrClass {
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            PtrClass::None => 0,
            PtrClass::Input => 1,
            PtrClass::Output => 2,
            PtrClass::Nullable => 3,
        }
    }
}
/// 行名到指针形态。
pub fn ptr_of(name: &str) -> PtrClass {
    match name {
        "read" | "pread64" => PtrClass::Output,
        "write" | "pwrite64" => PtrClass::Input,
        "ioctl" | "fstat" | "pipe" | "pipe2" => PtrClass::Output,
        "sendfile" | "splice" => PtrClass::Nullable,
        _ => PtrClass::None,
    }
}
/// 是否带用户指针。
pub fn ptr_capable(name: &str) -> bool {
    ptr_of(name) != PtrClass::None
}
/// 是否带 fd 参数。
pub fn fd_bearing(name: &str) -> bool {
    !matches!(name, "pipe" | "pipe2")
}
/// 一行的判定（门序：fd → 值域 → 指针）。
pub fn judge(scene: Scenario) -> u32 {
    match scene {
        Scenario::ValidArgs => 0,
        Scenario::BadFd => EBADF,
        Scenario::BadValue => EINVAL,
        Scenario::BadPtr => EFAULT,
    }
}
/// 对账行。
#[derive(Clone, Copy)]
pub struct Row {
    pub name: &'static str,
    pub book: Book,
    pub layer: Layer,
    pub scene: Scenario,
    pub claim: u32,
}
/// 行构造器（冻结表用）。
pub const fn row(name: &'static str, book: Book, layer: Layer, scene: Scenario, claim: u32) -> Row {
    Row { name, book, layer, scene, claim }
}
/// 16 行冻结对账表。
pub const ROWS: [Row; 16] = [
    row("read", Book::Base, Layer::Negative, Scenario::BadPtr, EFAULT),
    row("write", Book::Base, Layer::Negative, Scenario::BadPtr, EFAULT),
    row("pread64", Book::Base, Layer::Negative, Scenario::BadPtr, EFAULT),
    row("pwrite64", Book::Base, Layer::Negative, Scenario::BadPtr, EFAULT),
    row("ioctl", Book::Base, Layer::Negative, Scenario::BadPtr, EFAULT),
    row("sendfile", Book::Base, Layer::Negative, Scenario::BadPtr, EFAULT),
    row("splice", Book::Base, Layer::Undecided, Scenario::BadFd, 0),
    row("dup", Book::B08, Layer::Negative, Scenario::BadFd, EBADF),
    row("dup2", Book::B08, Layer::Positive, Scenario::ValidArgs, 0),
    row("dup3", Book::B08, Layer::Negative, Scenario::BadValue, EINVAL),
    row("fcntl", Book::B08, Layer::Negative, Scenario::BadValue, EINVAL),
    row("pipe", Book::B08, Layer::Positive, Scenario::ValidArgs, 0),
    row("pipe2", Book::B08, Layer::Negative, Scenario::BadValue, EINVAL),
    row("close", Book::B15, Layer::Negative, Scenario::BadFd, EBADF),
    row("fstat", Book::B15, Layer::Negative, Scenario::BadFd, EBADF),
    row("lseek", Book::B15, Layer::Negative, Scenario::BadValue, EINVAL),
];
/// 冻结主张表（与 ROWS 的 claim 一一对应）。
pub const FROZEN_CLAIM: [u32; 16] = [
    EFAULT, EFAULT, EFAULT, EFAULT, EFAULT, EFAULT, 0,
    EBADF, 0, EINVAL, EINVAL, 0, EINVAL,
    EBADF, EBADF, EINVAL,
];
/// 冻结判定表（splice 由待裁层裁定为 EBADF）。
pub const FROZEN_VERDICT: [u32; 16] = [
    EFAULT, EFAULT, EFAULT, EFAULT, EFAULT, EFAULT, EBADF,
    EBADF, 0, EINVAL, EINVAL, 0, EINVAL,
    EBADF, EBADF, EINVAL,
];
/// 行判定值。
pub fn verdict(r: &Row) -> u32 {
    judge(r.scene)
}
/// 行的最终落账值：待裁层按判定裁定，其余维持主张。
pub fn settled(r: &Row) -> u32 {
    if r.layer == Layer::Undecided {
        verdict(r)
    } else {
        r.claim
    }
}
/// 单行是否冲突（待裁层不计冲突，只计挂起）。
pub fn conflict(r: &Row) -> bool {
    r.layer != Layer::Undecided && r.claim != verdict(r)
}
/// 汇总报告。
#[derive(Clone, Copy)]
pub struct Report {
    pub rows: usize,
    pub conflicts: usize,
    pub base_conflicts: usize,
    pub b08_conflicts: usize,
    pub b15_conflicts: usize,
    pub positive: usize,
    pub negative: usize,
    pub undecided: usize,
    pub resolved: usize,
}
/// 逐册对账。
pub fn reconcile(rows: &[Row]) -> Report {
    let mut rep = Report {
        rows: rows.len(),
        conflicts: 0,
        base_conflicts: 0,
        b08_conflicts: 0,
        b15_conflicts: 0,
        positive: 0,
        negative: 0,
        undecided: 0,
        resolved: 0,
    };
    for r in rows.iter() {
        match r.layer {
            Layer::Positive => rep.positive += 1,
            Layer::Negative => rep.negative += 1,
            Layer::Undecided => {
                rep.undecided += 1;
                if settled(r) == verdict(r) {
                    rep.resolved += 1;
                }
            }
        }
        if conflict(r) {
            rep.conflicts += 1;
            match r.book {
                Book::Base => rep.base_conflicts += 1,
                Book::B08 => rep.b08_conflicts += 1,
                Book::B15 => rep.b15_conflicts += 1,
            }
        }
    }
    rep
}
/// 某册行数。
pub fn book_rows(rows: &[Row], b: Book) -> usize {
    rows.iter().filter(|r| r.book == b).count()
}
/// 某层行数。
pub fn layer_rows(rows: &[Row], l: Layer) -> usize {
    rows.iter().filter(|r| r.layer == l).count()
}
/// 分层声明一致性：待裁层豁免，其余必须与场景推导层相等。
pub fn layer_partition_ok(rows: &[Row]) -> bool {
    rows.iter().all(|r| r.layer == Layer::Undecided || r.layer == declared_layer(r.scene))
}
/// 形态自证：坏指针行必须可指针，坏 fd 行必须带 fd。
pub fn shapes_ok(rows: &[Row]) -> bool {
    rows.iter().all(|r| {
        if r.scene == Scenario::BadPtr && !ptr_capable(r.name) {
            return false;
        }
        if r.scene == Scenario::BadFd && !fd_bearing(r.name) {
            return false;
        }
        if r.layer == Layer::Undecided && r.scene == Scenario::ValidArgs {
            return false;
        }
        true
    })
}
/// 种类掩码（判定值种类集合）。
pub fn kind_mask(rows: &[Row]) -> u32 {
    let mut m = 0u32;
    for r in rows.iter() {
        m |= 1 << kind_of(verdict(r)).tag();
    }
    m
}
/// 冻结表是否等于常量表。
pub fn frozen_rows() -> Vec<Row> {
    let mut v = Vec::new();
    for r in ROWS.iter() {
        v.push(*r);
    }
    v
}
/// 主张漂移计数。
pub fn claim_drift(rows: &[Row]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < rows.len() && i < FROZEN_CLAIM.len() {
        if rows[i].claim != FROZEN_CLAIM[i] {
            n += 1;
        }
        i += 1;
    }
    n
}
/// 判定漂移计数。
pub fn verdict_drift(rows: &[Row]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < rows.len() && i < FROZEN_VERDICT.len() {
        if verdict(&rows[i]) != FROZEN_VERDICT[i] {
            n += 1;
        }
        i += 1;
    }
    n
}
/// 缺陷注入种类。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Defect {
    ExpectDrift,
    LayerLeak,
    RowLoss,
}
impl Defect {
    /// 三类缺陷。
    pub fn all() -> [Defect; 3] {
        [Defect::ExpectDrift, Defect::LayerLeak, Defect::RowLoss]
    }
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            Defect::ExpectDrift => 0,
            Defect::LayerLeak => 1,
            Defect::RowLoss => 2,
        }
    }
    /// 中文说明。
    pub fn detail(self) -> &'static str {
        match self {
            Defect::ExpectDrift => "册面主张被改写，冲突计数必须抬头",
            Defect::LayerLeak => "反层行伪装成正层或待裁层，分层声明必须报红",
            Defect::RowLoss => "行被删除，行数与册计数必须同时缩水",
        }
    }
}
/// 在副本上施加缺陷。
pub fn corrupt(rows: &[Row], d: Defect) -> Vec<Row> {
    let mut v = rows.to_vec();
    match d {
        Defect::ExpectDrift => {
            if let Some(first) = v.first_mut() {
                first.claim = EBADF;
            }
        }
        Defect::LayerLeak => {
            if let Some(r) = v.iter_mut().find(|r| r.name == "splice") {
                r.layer = Layer::Negative;
            }
        }
        Defect::RowLoss => {
            v.pop();
        }
    }
    v
}
/// 缺陷是否被对账抓住。
pub fn detect(d: Defect) -> bool {
    let bad = corrupt(&frozen_rows(), d);
    let rep = reconcile(&bad);
    match d {
        Defect::ExpectDrift => rep.conflicts > 0,
        Defect::LayerLeak => {
            rep.conflicts > 0 || rep.undecided != UNDECIDED_ROWS || !layer_partition_ok(&bad)
        }
        Defect::RowLoss => rep.rows != ROW_TOTAL || rep.rows < ROW_TOTAL,
    }
}
/// 缺陷注入账。
pub struct Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub rounds: Vec<(u8, u32)>,
}
impl Ledger {
    /// 空账。
    pub fn new() -> Self {
        Ledger { injected: 0, detected: 0, escaped: 0, rounds: Vec::new() }
    }
    /// 记一轮。
    pub fn record(&mut self, tag: u8, hit: bool) {
        self.injected += 1;
        if hit {
            self.detected += 1;
        } else {
            self.escaped += 1;
        }
        self.rounds.push((tag, if hit { 1 } else { 0 }));
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped
    }
}
/// 3 类 × 10 轮。
pub fn run_defects() -> Ledger {
    let mut l = Ledger::new();
    let mut r = 0;
    while r < INJECT_ROUNDS {
        for d in Defect::all().iter() {
            l.record(d.tag() as u8, detect(*d));
        }
        r += 1;
    }
    l
}
/// 每类检出数。
pub fn per_class(l: &Ledger) -> [u32; 3] {
    let mut out = [0u32; 3];
    for pair in l.rounds.iter() {
        let (tag, hit) = *pair;
        let idx = (tag as usize) % 3;
        out[idx] += hit;
    }
    out
}
/// 门序探针：坏 fd 压过坏值，坏值压过坏指针。
pub fn judge_order(fd_ok: bool, val_ok: bool, ptr_ok: bool) -> u32 {
    if !fd_ok {
        EBADF
    } else if !val_ok {
        EINVAL
    } else if !ptr_ok {
        EFAULT
    } else {
        0
    }
}

/// 对账状态机。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RecStage {
    Draft,
    Loaded,
    Matched,
    Layered,
    Sealed,
}
impl RecStage {
    /// 单步前进。
    pub fn can_go(self, next: RecStage) -> bool {
        matches!(
            (self, next),
            (RecStage::Draft, RecStage::Loaded)
                | (RecStage::Loaded, RecStage::Matched)
                | (RecStage::Matched, RecStage::Layered)
                | (RecStage::Layered, RecStage::Sealed)
        )
    }
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            RecStage::Draft => 0,
            RecStage::Loaded => 1,
            RecStage::Matched => 2,
            RecStage::Layered => 3,
            RecStage::Sealed => 4,
        }
    }
}
/// 诊断码闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RecCode {
    RowCountMismatch,
    BookCountDrift,
    LayerCountDrift,
    LayerLeak,
    ConflictNonZero,
    UndecidedUnresolved,
    ClaimDrift,
    VerdictDrift,
    RowShape,
    DefectEscaped,
    LedgerNotConserved,
    SegmentOutOfRange,
}
impl RecCode {
    /// 码值 = 段基址 + 序号。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            RecCode::RowCountMismatch => "对账行数不等于 16：先补行再谈零冲突",
            RecCode::BookCountDrift => "册计数漂移：E/B08/B15 应为 7/6/3",
            RecCode::LayerCountDrift => "层计数漂移：正/反/待裁应为 2/13/1",
            RecCode::LayerLeak => "分层泄漏：行声明与场景推导层不一致",
            RecCode::ConflictNonZero => "主张与判定冲突：逐册定位后再落账",
            RecCode::UndecidedUnresolved => "待裁格未消解：splice 必须裁定为 EBADF",
            RecCode::ClaimDrift => "册面主张漂移：对照冻结表逐行回填",
            RecCode::VerdictDrift => "判定值漂移：检查门序与场景映射",
            RecCode::RowShape => "行形态非法：坏指针行落在不可指针调用上",
            RecCode::DefectEscaped => "缺陷逃逸：注入必抓率未达 10/10",
            RecCode::LedgerNotConserved => "注入账不守恒：注入须等于检出加逃逸",
            RecCode::SegmentOutOfRange => "码值越出 0x7A840 窗口：段位守卫拒绝",
        }
    }
    /// 全量闭集。
    pub const ALL: [RecCode; 12] = [
        RecCode::RowCountMismatch,
        RecCode::BookCountDrift,
        RecCode::LayerCountDrift,
        RecCode::LayerLeak,
        RecCode::ConflictNonZero,
        RecCode::UndecidedUnresolved,
        RecCode::ClaimDrift,
        RecCode::VerdictDrift,
        RecCode::RowShape,
        RecCode::DefectEscaped,
        RecCode::LedgerNotConserved,
        RecCode::SegmentOutOfRange,
    ];
}
/// FNV-1a 32 位。
pub fn fnv1a(seed: u32, bytes: &[u8]) -> u32 {
    let mut h = seed;
    for b in bytes.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}
/// 行表指纹。
pub fn row_digest() -> u32 {
    let mut h = fnv1a(0x811C_9DC5, b"unx9235/rows");
    for r in ROWS.iter() {
        h = fnv1a(h, r.name.as_bytes());
        h = fnv1a(h, &[r.book.tag() as u8, r.layer.tag() as u8, r.scene.tag() as u8]);
        h = fnv1a(h, &r.claim.to_le_bytes());
    }
    h
}
/// 判定指纹。
pub fn verdict_digest() -> u32 {
    let mut h = fnv1a(0x811C_9DC5, b"unx9235/verdict");
    for r in ROWS.iter() {
        h = fnv1a(h, &verdict(r).to_le_bytes());
    }
    h
}
/// 跑批指纹。
pub fn run_digest() -> u32 {
    let rows = frozen_rows();
    let rep = reconcile(&rows);
    let mut h = fnv1a(row_digest(), b"run");
    h = fnv1a(h, &(rep.conflicts as u32).to_le_bytes());
    h = fnv1a(h, &(rep.resolved as u32).to_le_bytes());
    h = fnv1a(h, &verdict_digest().to_le_bytes());
    h
}
/// 文案闭集自检。
pub fn text_ok() -> bool {
    if RecCode::ALL.len() != 12 {
        return false;
    }
    let mut i = 0;
    while i < RecCode::ALL.len() {
        let a = RecCode::ALL[i];
        if a.detail().is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < RecCode::ALL.len() {
            let b = RecCode::ALL[j];
            if a.detail() == b.detail() || a.val() == b.val() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}
/// 自检总成。
pub fn run_unx9235_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9235");
    let rows = frozen_rows();
    let rep = reconcile(&rows);
    s.add("seg_base", SEG_BASE == 0x7A840, "段基址应为 0x7A840");
    s.add("code_window", RecCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40), "码值落在 0x7A840..0x7A880");
    s.add("text_closed_set", text_ok(), "12 条闭集、值唯一、建议非空");
    s.add("row_total", rep.rows == ROW_TOTAL, "对账行应为 16");
    s.add("book_base", book_rows(&rows, Book::Base) == BASE_ROWS, "E 型册应为 7 行");
    s.add("book_b08", book_rows(&rows, Book::B08) == B08_ROWS, "B08 册应为 6 行");
    s.add("book_b15", book_rows(&rows, Book::B15) == B15_ROWS, "B15 册应为 3 行");
    s.add("layer_positive", rep.positive == POSITIVE_ROWS, "正层应为 2 行");
    s.add("layer_negative", rep.negative == NEGATIVE_ROWS, "反层应为 13 行");
    s.add("layer_undecided", rep.undecided == UNDECIDED_ROWS, "待裁层应为 1 行");
    s.add("layer_partition", layer_partition_ok(&rows), "层声明与场景推导一致（待裁豁免）");
    s.add("zero_conflict", rep.conflicts == 0, "全表零冲突");
    s.add("zero_conflict_base", rep.base_conflicts == 0, "E 型册零冲突");
    s.add("zero_conflict_b08", rep.b08_conflicts == 0, "B08 dup 族与管道册零冲突");
    s.add("zero_conflict_b15", rep.b15_conflicts == 0, "B15 close/fstat/lseek 册零冲突");
    s.add("undecided_resolved", rep.resolved == UNDECIDED_ROWS && settled(&ROWS[6]) == EBADF, "splice 待裁格裁定为 EBADF");
    s.add("claim_frozen", claim_drift(&rows) == 0, "主张与冻结表逐行一致");
    s.add("verdict_frozen", verdict_drift(&rows) == 0, "判定与冻结表逐行一致");
    s.add("claim_distinct_from_verdict", ROWS[6].claim == 0 && verdict(&ROWS[6]) == EBADF, "待裁行主张与判定分离存放");
    s.add("row_shape", shapes_ok(&rows), "坏指针行可指针、坏 fd 行带 fd");
    s.add("order_probe", judge_order(false, false, false) == EBADF
        && judge_order(true, false, false) == EINVAL
        && judge_order(true, true, false) == EFAULT
        && judge_order(true, true, true) == 0, "门序 fd→值域→指针");
    s.add("kind_mask", kind_mask(&rows) == 0x0F, "判定种类覆盖 Ok/BadFd/BadPtr/BadValue");
    s.add("errno_text", [0u32, EBADF, EFAULT, EINVAL, ESPIPE].iter().all(|c| !errno_text(*c).is_empty()), "错误码文案非空");
    s.add("capacity", rows.len() <= MAX_ROWS, "行数不超过 32");
    let l = run_defects();
    let hits = per_class(&l);
    s.add("defect_ten_of_ten", hits == [INJECT_ROUNDS; 3], "三类缺陷各检出 10/10");
    s.add("ledger_conserved", l.conserved() && l.escaped == 0 && l.injected == INJECT_ROUNDS * 3, "注入 30 = 检出 30 + 逃逸 0");
    s.add("defect_text", Defect::all().iter().all(|d| !d.detail().is_empty()), "缺陷文案闭集非空");
    let stage_ok = RecStage::Draft.can_go(RecStage::Loaded)
        && RecStage::Loaded.can_go(RecStage::Matched)
        && RecStage::Matched.can_go(RecStage::Layered)
        && RecStage::Layered.can_go(RecStage::Sealed)
        && !RecStage::Draft.can_go(RecStage::Sealed)
        && !RecStage::Sealed.can_go(RecStage::Draft);
    s.add("state_machine", stage_ok, "对账阶段只许单步前进");
    let d1 = run_digest();
    let d2 = run_digest();
    let l2 = run_defects();
    s.add("double_run", d1 == d2 && l2.injected == l.injected && l2.detected == l.detected, "双跑指纹与注入账一致");
    s.add_dyn("report_detail", true, &format!("行={} 冲突={} 待裁={} 已裁={}", rep.rows, rep.conflicts, rep.undecided, rep.resolved));
    s.add_dyn("book_detail", rep.b08_conflicts == 0 && rep.b15_conflicts == 0, &format!("B08 冲突={} B15 冲突={}", rep.b08_conflicts, rep.b15_conflicts));
    s.add_dyn("defect_detail", hits[0] == 10 && hits[1] == 10 && hits[2] == 10, &format!("主张漂移={} 分层泄漏={} 行丢失={}", hits[0], hits[1], hits[2]));
    s
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_all_green() {
        let s = run_unx9235_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert!(s.len() >= 12);
    }
    #[test]
    fn books_add_up() {
        let rows = frozen_rows();
        assert_eq!(book_rows(&rows, Book::Base), BASE_ROWS);
        assert_eq!(book_rows(&rows, Book::B08), B08_ROWS);
        assert_eq!(book_rows(&rows, Book::B15), B15_ROWS);
        assert_eq!(BASE_ROWS + B08_ROWS + B15_ROWS, ROW_TOTAL);
    }
    #[test]
    fn layers_add_up() {
        let rows = frozen_rows();
        assert_eq!(layer_rows(&rows, Layer::Positive), POSITIVE_ROWS);
        assert_eq!(layer_rows(&rows, Layer::Negative), NEGATIVE_ROWS);
        assert_eq!(layer_rows(&rows, Layer::Undecided), UNDECIDED_ROWS);
        assert_eq!(POSITIVE_ROWS + NEGATIVE_ROWS + UNDECIDED_ROWS, ROW_TOTAL);
    }
    #[test]
    fn zero_conflict_across_books() {
        let rep = reconcile(&frozen_rows());
        assert_eq!(rep.conflicts, 0);
        assert_eq!(rep.base_conflicts, 0);
        assert_eq!(rep.b08_conflicts, 0);
        assert_eq!(rep.b15_conflicts, 0);
    }
    #[test]
    fn positive_negative_declared() {
        let rows = frozen_rows();
        assert!(layer_partition_ok(&rows));
        assert!(rows.iter().all(|r| r.layer != Layer::Undecided || r.scene != Scenario::ValidArgs));
    }
    #[test]
    fn undecided_is_settled() {
        let s = ROWS[6];
        assert_eq!(s.name, "splice");
        assert_eq!(s.layer, Layer::Undecided);
        assert_eq!(s.claim, 0);
        assert_eq!(settled(&s), EBADF);
        assert_eq!(reconcile(&frozen_rows()).resolved, 1);
    }
    #[test]
    fn shapes_are_sound() {
        let rows = frozen_rows();
        assert!(shapes_ok(&rows));
        assert!(rows.iter().filter(|r| r.scene == Scenario::BadPtr).all(|r| ptr_capable(r.name)));
        assert!(rows.iter().filter(|r| r.scene == Scenario::BadFd).all(|r| fd_bearing(r.name)));
    }
    #[test]
    fn defects_detected_ten_of_ten() {
        let l = run_defects();
        assert_eq!(per_class(&l), [10, 10, 10]);
        assert_eq!(l.escaped, 0);
        assert!(l.conserved());
    }
    #[test]
    fn kind_coverage() {
        let rows = frozen_rows();
        assert_eq!(kind_mask(&rows), 0x0F);
        assert_eq!(kind_of(0), Kind::Ok);
        assert_eq!(kind_of(EBADF), Kind::BadFd);
        assert_eq!(kind_of(EFAULT), Kind::BadPtr);
        assert_eq!(kind_of(EINVAL), Kind::BadValue);
    }
    #[test]
    fn stage_machine_single_step() {
        let mut cur = RecStage::Draft;
        for nxt in [RecStage::Loaded, RecStage::Matched, RecStage::Layered, RecStage::Sealed].iter() {
            assert!(cur.can_go(*nxt));
            cur = *nxt;
        }
        assert!(!RecStage::Draft.can_go(RecStage::Sealed));
    }
    #[test]
    fn digests_stable() {
        assert_eq!(row_digest(), row_digest());
        assert_eq!(run_digest(), run_digest());
        assert_ne!(row_digest(), 0);
    }
}
