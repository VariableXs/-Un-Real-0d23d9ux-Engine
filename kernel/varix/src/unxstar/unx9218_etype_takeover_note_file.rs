//! UNX-F9218 文件族 E 型升级接管注记 · 0x7A400段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9218 逐字）：
//!   ① 纯功能代码 ≥280 行；② 判据原文：UNX-F9218-J1 接管边界声明判据过，
//!   存量错误路径零重写承诺格过，增量清单落账格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 接管边界是本册唯一入口：族位图 + errno 环位图 + fd 上界三维同时成立
//!      才允许登记一条存量错误路径，越界拒收**先于**任何处置意见判定。
//!   ② 零重写承诺格只约束**在役**路径：ZeroRewrite 被标 rewritten=true 即
//!      PromiseViolated；已退役路径的历史改写不破诺——这条豁免正是「一刀切
//!      禁改」与「接管」的分水岭，正反两格都要钉住。
//!   ③ 注记状态机 Draft→Declared→Ratified→Archived：Declared 允许退回 Draft
//!      补正，Ratified 之后只能前进，任何倒退或越级一律 IllegalTransition。
//!   ④ 增量清单三段账 guards/asserts/recons 与台账可复算对齐（guards=包裹+
//!      移交，asserts=总数+零重写，recons=族数），对不上即 CountMismatch；
//!      注入器三类违约各 10 轮，账本守恒 注入=检出+逃逸，逃逸恒 0。
//!   ⑤ 与 B03/B07/B11/B15 逐族对账 + 指纹双跑：接管是把旧账搬进可复算的
//!      格子，不是新开一本孤账。
//! ─────────────────────────────────────────────────────────────────────
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9218 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A400;
/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 接管台账容量。
pub const MAX_PATHS: usize = 24;
/// 文件族数量（open/stat/unlink/dup）。
pub const FAMILY_COUNT: usize = 4;
/// errno 环长度。
pub const ERRNO_RING_LEN: usize = 8;
/// 注入类别数。
pub const INJECT_CLASSES: u32 = 3;
pub const ENOENT: i32 = 2;
pub const EBADF: i32 = 9;
pub const EACCES: i32 = 13;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
pub const ENFILE: i32 = 23;
pub const EMFILE: i32 = 24;
pub const EROFS: i32 = 30;
/// errno 环：位图下标 ⇄ errno 数值的唯一权威映射。
pub const ERRNO_RING: [i32; ERRNO_RING_LEN] =
    [EBADF, ENOENT, EFAULT, EINVAL, ENFILE, EMFILE, EACCES, EROFS];

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（0x7A400 段内 +0x01..+0x0E 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TkCode {
    BoundaryOverreach, ErrnoOutOfRing, FdOutOfBoundary, PromiseViolated,
    ManifestMissingEvidence, DuplicatePath, CapacityFull, IllegalTransition,
    NoteNotSealed, SummaryEmpty, CountMismatch, LedgerRegress,
    NotReproducible, SegmentViolation,
}

impl TkCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            TkCode::BoundaryOverreach => "接管边界越界：目标路径所属文件族未在边界位图内；请先在边界声明里补该族，或把该路径留在旧册不回搬",
            TkCode::ErrnoOutOfRing => "errno 出环：该错误码不在本册 errno 环内；请先扩环并补文案再登记，禁止让环外码借道吞掉",
            TkCode::FdOutOfBoundary => "fd 提示越界：登记的 fd 位高于边界上界；请复核该路径的 fd 空间归属，或上调上界并重跑全部格",
            TkCode::PromiseViolated => "零重写承诺被破：在役路径标了 ZeroRewrite 却被改写；请回退该处改动，或改判 Wrapped/Delegated 后重新落账",
            TkCode::ManifestMissingEvidence => "增量清单缺证据：证据代际为 0 等于空口落账；请补一次可复算的检查凭证（代际 ≥1）",
            TkCode::DuplicatePath => "路径号重复：同一 id 登记两次会掩掉旧格；请改用未占用 id，或在旧格上显式改判",
            TkCode::CapacityFull => "接管台账已满：容量 24 条；请先归档已退役路径腾位或另开续册，不许静默丢弃新路径",
            TkCode::IllegalTransition => "注记状态非法转移：只允许 Draft→Declared→Ratified→Archived（Declared 可退回 Draft 补正），请按序推进",
            TkCode::NoteNotSealed => "注记未封装：零路径的注记不能封装；请先登记至少一条存量错误路径",
            TkCode::SummaryEmpty => "结果摘要为空：写回面要求摘要非空；请把接管条数、状态与零重写条数写进摘要",
            TkCode::CountMismatch => "计数不守恒：声明条数与四类处置之和不等；请逐类复算，禁止用总量掩盖分类丢账",
            TkCode::LedgerRegress => "账目倒退：累计计数只增不减；请检查是否有回滚分支误改累计值",
            TkCode::NotReproducible => "双跑不一致：同输入两次指纹不同；请排查隐藏状态或未定序遍历",
            TkCode::SegmentViolation => "段位越界：段基址须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量。
    pub const ALL: [TkCode; 14] = [
        TkCode::BoundaryOverreach, TkCode::ErrnoOutOfRing, TkCode::FdOutOfBoundary,
        TkCode::PromiseViolated, TkCode::ManifestMissingEvidence, TkCode::DuplicatePath,
        TkCode::CapacityFull, TkCode::IllegalTransition, TkCode::NoteNotSealed,
        TkCode::SummaryEmpty, TkCode::CountMismatch, TkCode::LedgerRegress,
        TkCode::NotReproducible, TkCode::SegmentViolation,
    ];
}

// ── 文件族 / 处置 / 状态 ──────────────────────────────────────────────
/// 文件族：接管注记的族维（对应 B03/B07/B11/B15 存量册）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// open 族
    Open,
    /// stat 族
    Stat,
    /// unlink 族
    Unlink,
    /// dup 族
    Dup,
}

impl Family {
    /// 族位（边界位图用）。
    pub const fn mask(self) -> u8 {
        match self { Family::Open => 0x01, Family::Stat => 0x02, Family::Unlink => 0x04, Family::Dup => 0x08 }
    }
}

/// 存量错误路径的处置意见。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposition {
    /// 零重写：保留旧行为，承诺不碰
    ZeroRewrite,
    /// 包裹：加注记外壳，内核行为不变
    Wrapped,
    /// 移交：判定权交给新判定器
    Delegated,
    /// 退役：路径已不存在，仅存档
    Retired,
}

/// 接管注记状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoteState {
    /// 草稿：可增删路径
    Draft,
    /// 已封装：清单冻结
    Declared,
    /// 已批准：对外生效
    Ratified,
    /// 已归档：只读冻结
    Archived,
}

impl NoteState {
    /// 状态名。
    pub const fn label(self) -> &'static str {
        match self {
            NoteState::Draft => "draft", NoteState::Declared => "declared",
            NoteState::Ratified => "ratified", NoteState::Archived => "archived",
        }
    }
    /// 转移合法性表（同态自转亦拒：幂等重推属调用方 bug）。
    pub const fn can_go(self, to: NoteState) -> bool {
        match (self, to) {
            (NoteState::Draft, NoteState::Declared) => true,
            // 封装后发现漏项，允许退回草稿补正（清单仍可复算）。
            (NoteState::Declared, NoteState::Draft) => true,
            (NoteState::Declared, NoteState::Ratified) => true,
            (NoteState::Ratified, NoteState::Archived) => true,
            // 含 Ratified→Draft 倒退、Draft→Ratified 越级、Archived→* 冻结。
            _ => false,
        }
    }
}

// ── 接管边界 ──────────────────────────────────────────────────────────
/// 接管边界声明：族位图 + errno 环位图 + fd 上界 + 退役许可。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TakeoverBoundary {
    /// 允许的族位图（bit0..bit3）
    pub families: u8,
    /// 允许的 errno 环位图（bit_i 对应 ERRNO_RING[i]）
    pub errno_mask: u16,
    /// fd 提示上界（含）；负值表示「本路径不涉 fd」
    pub max_fd: i32,
    /// 是否允许退役路径进册
    pub allow_retired: bool,
}

impl TakeoverBoundary {
    /// 文件族全边界：四族全开、环全开、fd ≤63、允许退役。
    pub const fn file_family_full() -> TakeoverBoundary {
        TakeoverBoundary { families: 0x0F, errno_mask: 0x00FF, max_fd: 63, allow_retired: true }
    }
    /// 族维是否覆盖。
    pub const fn admits_family(&self, f: Family) -> bool { self.families & f.mask() != 0 }
    /// errno 是否在环内。
    pub fn admits_errno(&self, code: i32) -> bool {
        let mut i = 0usize;
        while i < ERRNO_RING_LEN {
            if let Some(v) = ERRNO_RING.get(i) {
                if *v == code {
                    return self.errno_mask & (1u16 << i) != 0;
                }
            }
            i += 1;
        }
        false
    }
    /// fd 提示是否在界内（负值恒允许：表示不涉 fd）。
    pub const fn admits_fd(&self, fd: i32) -> bool { fd < 0 || fd <= self.max_fd }
}

// ── 路径记录 / 分类守恒 ───────────────────────────────────────────────
/// 一条存量错误路径记录（接管台账的行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PathRecord {
    /// 路径号（沿用旧册编号，便于对账）
    pub id: u16,
    /// 所属文件族
    pub family: Family,
    /// 旧册登记的 errno（正数口径）
    pub errno: i32,
    /// fd 提示（-1 表示不涉 fd）
    pub fd_hint: i32,
    /// 处置意见
    pub disp: Disposition,
    /// 证据代际（≥1 才可落账）
    pub evidence_gen: u16,
    /// 该路径是否已被实际改写（承诺格输入）
    pub rewritten: bool,
}

impl PathRecord {
    /// 构造记录（证据代际默认 1、未改写）。
    pub const fn new(id: u16, family: Family, errno: i32, fd: i32, disp: Disposition) -> PathRecord {
        PathRecord { id, family, errno, fd_hint: fd, disp, evidence_gen: 1, rewritten: false }
    }
}

/// 处置分类守恒快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Conservation {
    /// 零重写条数
    pub zero_rewrite: u32,
    /// 包裹条数
    pub wrapped: u32,
    /// 移交条数
    pub delegated: u32,
    /// 退役条数
    pub retired: u32,
    /// 台账总数
    pub total: u32,
}

impl Conservation {
    /// 空快照。
    pub const fn empty() -> Conservation {
        Conservation { zero_rewrite: 0, wrapped: 0, delegated: 0, retired: 0, total: 0 }
    }
    /// 守恒：分类和 == 总数。
    pub fn balanced(&self) -> bool {
        self.zero_rewrite.saturating_add(self.wrapped)
            .saturating_add(self.delegated).saturating_add(self.retired) == self.total
    }
}

// ── 接管注记 ──────────────────────────────────────────────────────────
/// 文件族 E 型升级接管注记（本册主体）。
#[derive(Clone, Copy, Debug)]
pub struct TakeoverNote {
    state: NoteState,
    boundary: TakeoverBoundary,
    paths: [Option<PathRecord>; MAX_PATHS],
    count: usize,
    declares: u32,
    rejects: u32,
    ratifies: u32,
}

impl TakeoverNote {
    /// 以给定边界开一本空注记。
    pub const fn new(boundary: TakeoverBoundary) -> TakeoverNote {
        TakeoverNote {
            state: NoteState::Draft, boundary, paths: [None; MAX_PATHS],
            count: 0, declares: 0, rejects: 0, ratifies: 0,
        }
    }
    pub fn state(&self) -> NoteState { self.state }
    pub fn boundary(&self) -> TakeoverBoundary { self.boundary }
    pub fn count(&self) -> usize { self.count }
    pub fn declares(&self) -> u32 { self.declares }
    pub fn rejects(&self) -> u32 { self.rejects }
    pub fn ratifies(&self) -> u32 { self.ratifies }
    /// 第 idx 条记录（越界或空槽回 None）。
    pub fn path_at(&self, idx: usize) -> Option<PathRecord> {
        match self.paths.get(idx) {
            Some(Some(p)) => Some(*p),
            _ => None,
        }
    }
    /// 判定次序核心：族 → errno 环 → fd 界 → 退役许可 → 零重写承诺 →
    /// 证据代际 → 路径号唯一。前六条与台账存量无关，可对「未来行」单判。
    pub fn admit(&self, rec: &PathRecord) -> Result<(), TkCode> {
        if !self.boundary.admits_family(rec.family) {
            return Err(TkCode::BoundaryOverreach);
        }
        if !self.boundary.admits_errno(rec.errno) {
            return Err(TkCode::ErrnoOutOfRing);
        }
        if !self.boundary.admits_fd(rec.fd_hint) {
            return Err(TkCode::FdOutOfBoundary);
        }
        if rec.disp == Disposition::Retired && !self.boundary.allow_retired {
            return Err(TkCode::BoundaryOverreach);
        }
        // 承诺格只约束在役路径；退役路径的历史改写不破诺。
        if rec.disp == Disposition::ZeroRewrite && rec.rewritten {
            return Err(TkCode::PromiseViolated);
        }
        if rec.evidence_gen == 0 {
            return Err(TkCode::ManifestMissingEvidence);
        }
        if (0..self.count).filter_map(|i| self.path_at(i)).any(|p| p.id == rec.id) {
            return Err(TkCode::DuplicatePath);
        }
        Ok(())
    }

    /// 登记一条路径。任何拒收都记入 rejects（拒收账不静默）。
    pub fn declare(&mut self, rec: PathRecord) -> Result<u16, TkCode> {
        if !matches!(self.state, NoteState::Draft) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TkCode::IllegalTransition);
        }
        if let Err(code) = self.admit(&rec) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(code);
        }
        let idx = self.count;
        if idx >= MAX_PATHS {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TkCode::CapacityFull);
        }
        if let Some(slot) = self.paths.get_mut(idx) {
            *slot = Some(rec);
            self.count = self.count.saturating_add(1);
            self.declares = self.declares.saturating_add(1);
            return Ok(rec.id);
        }
        self.rejects = self.rejects.saturating_add(1);
        Err(TkCode::CapacityFull)
    }

    /// 封装：Draft → Declared。要求至少一条路径且证据代际全 ≥1。
    pub fn seal(&mut self) -> Result<u32, TkCode> {
        if !self.state.can_go(NoteState::Declared) {
            return Err(TkCode::IllegalTransition);
        }
        if self.count == 0 {
            return Err(TkCode::NoteNotSealed);
        }
        if (0..self.count).filter_map(|i| self.path_at(i)).any(|p| p.evidence_gen == 0) {
            return Err(TkCode::ManifestMissingEvidence);
        }
        self.state = NoteState::Declared;
        Ok(self.count as u32)
    }

    /// 批准：Declared → Ratified。要求分类守恒且摘要非空。
    pub fn ratify(&mut self) -> Result<u32, TkCode> {
        if !self.state.can_go(NoteState::Ratified) {
            return Err(TkCode::IllegalTransition);
        }
        if !self.conservation().balanced() {
            return Err(TkCode::CountMismatch);
        }
        if self.summary().is_empty() {
            return Err(TkCode::SummaryEmpty);
        }
        self.state = NoteState::Ratified;
        self.ratifies = self.ratifies.saturating_add(1);
        Ok(self.count as u32)
    }

    /// 通用状态转移。
    pub fn transition(&mut self, to: NoteState) -> Result<NoteState, TkCode> {
        if !self.state.can_go(to) {
            return Err(TkCode::IllegalTransition);
        }
        self.state = to;
        Ok(to)
    }

    /// 逐族计数。
    pub fn per_family(&self, f: Family) -> u32 {
        (0..self.count).filter_map(|i| self.path_at(i)).filter(|p| p.family == f).count() as u32
    }
    /// 分类守恒快照。
    pub fn conservation(&self) -> Conservation {
        let mut c = Conservation::empty();
        for p in (0..self.count).filter_map(|i| self.path_at(i)) {
            match p.disp {
                Disposition::ZeroRewrite => c.zero_rewrite = c.zero_rewrite.saturating_add(1),
                Disposition::Wrapped => c.wrapped = c.wrapped.saturating_add(1),
                Disposition::Delegated => c.delegated = c.delegated.saturating_add(1),
                Disposition::Retired => c.retired = c.retired.saturating_add(1),
            }
            c.total = c.total.saturating_add(1);
        }
        c
    }
    /// 零重写承诺是否全部成立（在役零重写路径不得带改写标记）。
    pub fn promise_holds(&self) -> bool {
        (0..self.count)
            .filter_map(|i| self.path_at(i))
            .all(|p| !(p.disp == Disposition::ZeroRewrite && p.rewritten))
    }
    /// 结果摘要（写回面要求非空）。
    pub fn summary(&self) -> String {
        let c = self.conservation();
        format!("接管注记[{}] 路径 {} 条：零重写 {} / 包裹 {} / 移交 {} / 退役 {}；拒收 {}",
                self.state.label(), c.total, c.zero_rewrite, c.wrapped,
                c.delegated, c.retired, self.rejects)
    }
    /// 台账指纹（逐行含 id/族/errno/fd/处置/证据/改写位，FNV-1a）。
    pub fn fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        for p in (0..self.count).filter_map(|i| self.path_at(i)) {
            bytes.push((p.id & 0xFF) as u8);
            bytes.push((p.id >> 8) as u8);
            bytes.push(p.family.mask());
            bytes.push((p.errno & 0xFF) as u8);
            bytes.push(((p.fd_hint + 1) & 0xFF) as u8);
            bytes.push(p.disp as u8);
            bytes.push((p.evidence_gen & 0xFF) as u8);
            bytes.push(u8::from(p.rewritten));
        }
        bytes.push(self.state as u8);
        bytes.push(self.boundary.families);
        fnv1a(&bytes)
    }
}

// ── 增量清单 ──────────────────────────────────────────────────────────
/// 增量清单：三段计数（护栏/断言/对账）+ 提交位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IncrementManifest {
    /// 新增护栏条数（包裹 + 移交各一条）
    pub guards: u32,
    /// 新增断言条数（路径总数 + 零重写条数）
    pub asserts: u32,
    /// 新增对账行数（逐族一行）
    pub recons: u32,
    /// 是否已提交落账
    pub committed: bool,
}

impl IncrementManifest {
    /// 空清单。
    pub const fn empty() -> IncrementManifest {
        IncrementManifest { guards: 0, asserts: 0, recons: 0, committed: false }
    }
    /// 总条数。
    pub fn total(&self) -> u32 { self.guards.saturating_add(self.asserts).saturating_add(self.recons) }
    /// 提交：总条数非空、护栏非零、对账覆盖四族，三者同时成立才落账。
    pub fn commit(&mut self) -> Result<u32, TkCode> {
        if self.total() == 0 {
            return Err(TkCode::CountMismatch);
        }
        if self.guards == 0 {
            return Err(TkCode::ManifestMissingEvidence);
        }
        if self.recons != FAMILY_COUNT as u32 {
            return Err(TkCode::CountMismatch);
        }
        self.committed = true;
        Ok(self.total())
    }
    /// 与注记台账可复算地对齐。
    pub fn agrees_with(&self, note: &TakeoverNote) -> bool {
        let c = note.conservation();
        self.guards == c.wrapped.saturating_add(c.delegated)
            && self.asserts == c.total.saturating_add(c.zero_rewrite)
            && self.recons == FAMILY_COUNT as u32
            && self.committed
    }
}

/// 由台账推导应有的增量清单（对账的右侧）。
pub fn expected_manifest(note: &TakeoverNote) -> IncrementManifest {
    let c = note.conservation();
    IncrementManifest {
        guards: c.wrapped.saturating_add(c.delegated),
        asserts: c.total.saturating_add(c.zero_rewrite),
        recons: FAMILY_COUNT as u32,
        committed: true,
    }
}

// ── 与既有册对账 ──────────────────────────────────────────────────────
/// 对账表：(族, 本册声明数, 对照册名, 对照册登记数)。
pub const RECON_TABLE: [(Family, u32, &str, u32); FAMILY_COUNT] = [
    (Family::Open, 6, "B03 open 族册", 6),
    (Family::Stat, 5, "B07 stat 族册", 5),
    (Family::Unlink, 4, "B11 unlink 族册", 4),
    (Family::Dup, 5, "B15 dup 族册", 5),
];

/// 逐族对账：本册计数与对照册登记数逐项相等。
pub fn reconcile(note: &TakeoverNote) -> bool {
    let mut i = 0usize;
    while i < RECON_TABLE.len() {
        if let Some((fam, mine, _who, theirs)) = RECON_TABLE.get(i).copied() {
            if note.per_family(fam) != mine || mine != theirs {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 对账表总登记数（应为 20）。
pub fn recon_total() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < RECON_TABLE.len() {
        if let Some((_, mine, _, _)) = RECON_TABLE.get(i) {
            n = n.saturating_add(*mine);
        }
        i += 1;
    }
    n
}

// ── 种子台账 ──────────────────────────────────────────────────────────
/// 种子行：(id, 族, errno, fd 提示, 处置, 是否已改写)。
pub const SEED_ROWS: [(u16, Family, i32, i32, Disposition, bool); 20] = [
    (100, Family::Open, ENOENT, 3, Disposition::ZeroRewrite, false),
    (101, Family::Open, EBADF, -1, Disposition::ZeroRewrite, false),
    (102, Family::Open, EACCES, 5, Disposition::Wrapped, false),
    (103, Family::Open, EMFILE, 7, Disposition::Delegated, false),
    (104, Family::Open, ENFILE, 9, Disposition::Delegated, false),
    (105, Family::Open, EROFS, 11, Disposition::Wrapped, true),
    (200, Family::Stat, ENOENT, 12, Disposition::ZeroRewrite, false),
    (201, Family::Stat, EBADF, -1, Disposition::ZeroRewrite, false),
    (202, Family::Stat, EFAULT, 14, Disposition::Wrapped, false),
    (203, Family::Stat, EACCES, 15, Disposition::ZeroRewrite, false),
    (204, Family::Stat, EBADF, 16, Disposition::Retired, true),
    (300, Family::Unlink, ENOENT, 20, Disposition::ZeroRewrite, false),
    (301, Family::Unlink, EBADF, -1, Disposition::ZeroRewrite, false),
    (302, Family::Unlink, EACCES, 21, Disposition::Wrapped, false),
    (303, Family::Unlink, EROFS, 22, Disposition::Retired, true),
    (400, Family::Dup, EBADF, -1, Disposition::ZeroRewrite, false),
    (401, Family::Dup, EBADF, 30, Disposition::Delegated, false),
    (402, Family::Dup, EINVAL, 31, Disposition::Wrapped, false),
    (403, Family::Dup, EMFILE, 32, Disposition::Delegated, false),
    (404, Family::Dup, EBADF, 33, Disposition::Retired, true),
];

/// 构造种子台账（20 条，四族 6/5/4/5）。
pub fn seeded_note() -> Result<TakeoverNote, TkCode> {
    let mut note = TakeoverNote::new(TakeoverBoundary::file_family_full());
    let mut i = 0usize;
    while i < SEED_ROWS.len() {
        if let Some((id, fam, errno, fd, disp, rewritten)) = SEED_ROWS.get(i).copied() {
            let mut rec = PathRecord::new(id, fam, errno, fd, disp);
            rec.rewritten = rewritten;
            note.declare(rec)?;
        }
        i += 1;
    }
    Ok(note)
}

/// 种子台账，失败时退回空册（生产路径不 panic，空册会被判据当场红）。
pub fn seeded_or_empty() -> TakeoverNote {
    match seeded_note() {
        Ok(n) => n,
        Err(_) => TakeoverNote::new(TakeoverBoundary::file_family_full()),
    }
}

// ── 注入器 ────────────────────────────────────────────────────────────
/// 三类注入样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TkInject {
    /// 边界越界（fd 提示 4096）
    BoundaryOverreach,
    /// 承诺破坏（在役零重写被改写）
    PromiseBroken,
    /// 清单无证据（证据代际 0）
    ManifestNoEvidence,
}

impl TkInject {
    /// 样本记录。
    pub fn sample(self) -> PathRecord {
        match self {
            TkInject::BoundaryOverreach =>
                PathRecord::new(900, Family::Open, ENOENT, 4096, Disposition::Wrapped),
            TkInject::PromiseBroken => {
                let mut r = PathRecord::new(901, Family::Stat, EBADF, 5, Disposition::ZeroRewrite);
                r.rewritten = true;
                r
            }
            TkInject::ManifestNoEvidence => {
                let mut r = PathRecord::new(902, Family::Dup, EBADF, 6, Disposition::Delegated);
                r.evidence_gen = 0;
                r
            }
        }
    }
    /// 期望码。
    pub fn expected_code(self) -> TkCode {
        match self {
            TkInject::BoundaryOverreach => TkCode::FdOutOfBoundary,
            TkInject::PromiseBroken => TkCode::PromiseViolated,
            TkInject::ManifestNoEvidence => TkCode::ManifestMissingEvidence,
        }
    }
    /// 闭集全量。
    pub const ALL: [TkInject; 3] =
        [TkInject::BoundaryOverreach, TkInject::PromiseBroken, TkInject::ManifestNoEvidence];
}

/// 注入账（注入 = 检出 + 逃逸）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TkLedger {
    /// 注入次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须 0）
    pub escaped: u32,
}

impl TkLedger {
    /// 零账。
    pub const fn zero() -> TkLedger { TkLedger { injected: 0, detected: 0, escaped: 0 } }
    /// 记一笔。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }
    /// 守恒式。
    pub fn conserved(&self) -> bool { self.detected.saturating_add(self.escaped) == self.injected }
}

/// 跑三类注入 × 10 轮，返回账本与逐类检出数。
pub fn run_takeover_matrix() -> (TkLedger, [u32; 3]) {
    let mut ledger = TkLedger::zero();
    let mut per_class = [0u32; 3];
    let mut ci = 0usize;
    while ci < TkInject::ALL.len() {
        if let Some(kind) = TkInject::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let mut note = seeded_or_empty();
                let caught = note.declare(kind.sample()) == Err(kind.expected_code());
                ledger.record(caught);
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

/// 判定次序专项：同一行同时踩多条违约时先回哪条码。
pub fn judge_order_probe() -> (TkCode, TkCode, TkCode) {
    let note = seeded_or_empty();
    let pick = |r: Result<(), TkCode>| match r { Err(c) => c, Ok(()) => TkCode::CountMismatch };
    // ① 环外 errno + 承诺破坏 → 环外先。
    let mut a = PathRecord::new(950, Family::Open, 28, 3, Disposition::ZeroRewrite);
    a.rewritten = true;
    let first = pick(note.admit(&a));
    // ② 承诺破坏 + 证据缺失 → 承诺先。
    let mut b = PathRecord::new(951, Family::Stat, EBADF, 3, Disposition::ZeroRewrite);
    b.rewritten = true;
    b.evidence_gen = 0;
    let second = pick(note.admit(&b));
    // ③ 证据缺失 + 路径号重复（100 已占）→ 证据先。
    let mut c = PathRecord::new(100, Family::Dup, EBADF, 3, Disposition::Delegated);
    c.evidence_gen = 0;
    let third = pick(note.admit(&c));
    (first, second, third)
}

// ── 指纹 ──────────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
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

/// 规范化巡检：种子台账 → 封装 → 批准 → 指纹（含两次转移结果）。
pub fn patrol_fingerprint() -> u32 {
    let mut note = seeded_or_empty();
    let sealed = note.seal();
    let ratified = note.ratify();
    let mut acc = note.fingerprint();
    if let Ok(n) = sealed {
        acc ^= fnv1a(&[(n & 0xFF) as u8]);
    }
    if let Ok(n) = ratified {
        acc ^= fnv1a(&[(n & 0xFF) as u8]).wrapping_mul(3);
    }
    acc
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9218 判据集（22 条静态 + 5 条动态；add_dyn ≤8）。
pub fn run_unx9218_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9218");

    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址须唯一且落在本批窗口，越界即拒绝注册");

    let note0 = seeded_or_empty();
    s.add("正常流：20 条存量路径全部过边界与承诺格",
          note0.count() == 20 && note0.count() == recon_total() as usize,
          "四族 6/5/4/5 共 20 条须全部落账，且与对账表总数相等");

    let mut pipeline = note0;
    let sealed = pipeline.seal();
    let ratified = pipeline.ratify();
    s.add("正常流：seal→ratify 连过且状态=已批准",
          sealed == Ok(20) && ratified == Ok(20) && pipeline.state() == NoteState::Ratified,
          "合法注记必须能一路推进到 Ratified，中途不得误拦");

    let narrow = TakeoverBoundary { families: 0x03, errno_mask: 0x00FF, max_fd: 63, allow_retired: true };
    let mut nf = TakeoverNote::new(narrow);
    s.add("边界：族位图未覆盖 → BoundaryOverreach",
          nf.declare(PathRecord::new(1, Family::Unlink, ENOENT, 3, Disposition::Wrapped))
              == Err(TkCode::BoundaryOverreach),
          "unlink 族不在窄边界内，必须在进册前被拒");

    let mut nf2 = TakeoverNote::new(TakeoverBoundary::file_family_full());
    s.add("边界：环外 errno(ENOSPC) → ErrnoOutOfRing",
          nf2.declare(PathRecord::new(2, Family::Open, 28, 3, Disposition::Wrapped))
              == Err(TkCode::ErrnoOutOfRing),
          "ENOSPC 不在 errno 环内，一律拒收，不许借道吞码");

    let mut nf3 = TakeoverNote::new(TakeoverBoundary::file_family_full());
    s.add("边界：fd 上界+1 → FdOutOfBoundary",
          nf3.declare(PathRecord::new(3, Family::Dup, EBADF, 64, Disposition::Wrapped))
              == Err(TkCode::FdOutOfBoundary),
          "边界上界 63，fd 提示 64 必拒；负值表示不涉 fd 恒允许");

    let mut full = TakeoverNote::new(TakeoverBoundary::file_family_full());
    let mut k = 0u16;
    while k < MAX_PATHS as u16 {
        let _ = full.declare(PathRecord::new(600 + k, Family::Open, ENOENT, 3, Disposition::Wrapped));
        k += 1;
    }
    s.add("边界：满表第 25 条 → CapacityFull 且账不丢",
          full.declare(PathRecord::new(999, Family::Open, ENOENT, 3, Disposition::Wrapped))
              == Err(TkCode::CapacityFull) && full.count() == MAX_PATHS,
          "满表必须显性拒收，已有 24 条不得被覆盖或丢弃");

    let c0 = note0.conservation();
    s.add("承诺格正判据：8 条零重写路径全部未被改写",
          note0.promise_holds() && c0.zero_rewrite == 8,
          "在役零重写路径须保持 rewritten=false，退役路径的历史改写不算破诺");

    let mut np = TakeoverNote::new(TakeoverBoundary::file_family_full());
    let mut bad = PathRecord::new(700, Family::Stat, EBADF, 3, Disposition::ZeroRewrite);
    bad.rewritten = true;
    s.add("承诺格反判据：零重写却改写 → PromiseViolated",
          np.declare(bad) == Err(TkCode::PromiseViolated),
          "承诺格必须当场拒收，不能事后靠复核补漏");

    let mut nr = TakeoverNote::new(TakeoverBoundary::file_family_full());
    let mut retired = PathRecord::new(701, Family::Open, ENOENT, 3, Disposition::Retired);
    retired.rewritten = true;
    s.add("承诺格豁免：退役路径带改写位仍可进册", nr.declare(retired) == Ok(701),
          "退役即不在役，历史改写属存量事实，不得误判为破诺");

    let (first, second, third) = judge_order_probe();
    s.add("判定次序：errno 环 > 承诺 > 证据 > 唯一",
          first == TkCode::ErrnoOutOfRing && second == TkCode::PromiseViolated
              && third == TkCode::ManifestMissingEvidence,
          "同时违约时须按固定次序回码，次序错即判据失效");

    let mut st = TakeoverNote::new(TakeoverBoundary::file_family_full());
    let leap = st.ratify();
    let _ = st.declare(PathRecord::new(800, Family::Open, ENOENT, 3, Disposition::Wrapped));
    let seal2 = st.seal();
    let back = st.transition(NoteState::Draft);
    let _ = st.seal();
    let _ = st.ratify();
    let regress = st.transition(NoteState::Draft);
    let forward = st.transition(NoteState::Archived);
    s.add("状态机：越级/倒退/归档后转移全拒",
          leap == Err(TkCode::IllegalTransition) && seal2 == Ok(1)
              && back == Ok(NoteState::Draft) && regress == Err(TkCode::IllegalTransition)
              && forward == Ok(NoteState::Archived)
              && st.transition(NoteState::Draft) == Err(TkCode::IllegalTransition),
          "Draft→Ratified 越级拒；Declared→Draft 补正准；Ratified→Draft 拒；Archived 冻结");

    let mut st2 = note0;
    let _ = st2.seal();
    s.add("状态机：封装后新增路径被拒",
          st2.declare(PathRecord::new(801, Family::Open, ENOENT, 3, Disposition::Wrapped))
              == Err(TkCode::IllegalTransition),
          "清单在 Declared 态已冻结，补正须显式退回 Draft");

    let (ledger, per_class) = run_takeover_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入必抓：边界/承诺/无证据三类各 10/10", all_caught,
          "三类违约每类 10 轮全部检出，漏检即红");

    s.add("计数守恒：零重写+包裹+移交+退役 == 路径总数",
          c0.balanced() && c0.total == 20, "分类丢账会直接破坏守恒式，禁止用总量掩盖");
    s.add("对账一致：四族计数与 B03/B07/B11/B15 逐项相等", reconcile(&note0),
          "接管是把旧账搬进新格，任一族对不上即视为新开孤账");

    let mut m = expected_manifest(&note0);
    s.add("增量清单：guards/asserts/recons 与台账可复算对齐",
          m.commit() == Ok(41) && m.agrees_with(&note0),
          "guards=包裹+移交(9)，asserts=总数+零重写(28)，recons=四族，缺一不可");

    let mut empty_m = IncrementManifest::empty();
    s.add("增量清单：空清单提交 → CountMismatch",
          empty_m.commit() == Err(TkCode::CountMismatch) && !empty_m.committed,
          "零增量不得落账；commit 失败后 committed 必须保持 false");

    let fp_a = patrol_fingerprint();
    let fp_b = patrol_fingerprint();
    s.add("双跑一致：巡检指纹两次相等", fp_a == fp_b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    s.add("文案闭集：14 个诊断码均带中文建议",
          TkCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到非空且可操作的说明");

    let uniq = TkCode::ALL.iter().enumerate().all(|(i, a)| {
        TkCode::ALL.iter().enumerate().all(|(j, b)| i == j || a.val() != b.val())
    });
    s.add("诊断码：14 个码值两两唯一", uniq, "段内闭集码不得重复，否则判据无法定位");

    let mut mono = note0;
    let before = mono.declares();
    let _ = mono.declare(PathRecord::new(802, Family::Dup, EBADF, 3, Disposition::Wrapped));
    let _ = mono.declare(PathRecord::new(802, Family::Dup, EBADF, 3, Disposition::Wrapped));
    s.add("账目单调：登记成功 +1、重复登记计拒收",
          mono.declares() == before.saturating_add(1) && mono.rejects() >= 1,
          "计数只增不减；重复 id 必须落在 rejects 而非 declares");

    // 动态明细（5 条，≤8）
    let text = format!("注入 {} / 检出 {} / 逃逸 {}", ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("注入账快照", ledger.conserved() && ledger.escaped == 0, &text);
    let per = format!("边界 {} / 承诺 {} / 无证据 {}",
                      per_class.first().copied().unwrap_or(0),
                      per_class.get(1).copied().unwrap_or(0),
                      per_class.get(2).copied().unwrap_or(0));
    s.add_dyn("逐类检出快照", all_caught, &per);
    let cons_text = format!("零重写 {} / 包裹 {} / 移交 {} / 退役 {} = {}",
                            c0.zero_rewrite, c0.wrapped, c0.delegated, c0.retired, c0.total);
    s.add_dyn("分类守恒快照", c0.balanced(), &cons_text);
    let fp_text = format!("巡检指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);
    let sum = note0.summary();
    s.add_dyn("结果摘要非空", !sum.is_empty(), &sum);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn note() -> TakeoverNote { seeded_or_empty() }

    #[test]
    fn seed_registers_twenty() {
        let n = note();
        assert_eq!((n.count(), n.declares(), n.rejects()), (20, 20, 0));
        assert_eq!(n.state(), NoteState::Draft);
        assert_eq!(n.per_family(Family::Open), 6);
        assert_eq!(n.per_family(Family::Stat), 5);
        assert_eq!(n.per_family(Family::Unlink), 4);
        assert_eq!(n.per_family(Family::Dup), 5);
    }

    #[test]
    fn seal_ratify_pipeline() {
        let mut n = note();
        assert_eq!(n.seal(), Ok(20));
        assert_eq!(n.state(), NoteState::Declared);
        assert_eq!(n.ratify(), Ok(20));
        assert_eq!(n.state(), NoteState::Ratified);
        assert_eq!(n.ratifies(), 1);
    }

    #[test]
    fn boundary_precedes_promise() {
        let narrow = TakeoverBoundary {
            families: Family::Open.mask(), errno_mask: 0x0001, max_fd: 3, allow_retired: false,
        };
        let n = TakeoverNote::new(narrow);
        let mut a = PathRecord::new(1, Family::Dup, 28, 99, Disposition::ZeroRewrite);
        a.rewritten = true;
        assert_eq!(n.admit(&a), Err(TkCode::BoundaryOverreach));
        assert_eq!(n.admit(&PathRecord::new(2, Family::Open, 28, 3, Disposition::Wrapped)),
                   Err(TkCode::ErrnoOutOfRing));
        assert_eq!(n.admit(&PathRecord::new(3, Family::Open, EBADF, 4, Disposition::Wrapped)),
                   Err(TkCode::FdOutOfBoundary));
        assert_eq!(n.admit(&PathRecord::new(4, Family::Open, EBADF, -1, Disposition::Retired)),
                   Err(TkCode::BoundaryOverreach));
    }

    #[test]
    fn promise_and_retired_exemption() {
        let mut n = TakeoverNote::new(TakeoverBoundary::file_family_full());
        let mut broken = PathRecord::new(10, Family::Open, ENOENT, 3, Disposition::ZeroRewrite);
        broken.rewritten = true;
        assert_eq!(n.declare(broken), Err(TkCode::PromiseViolated));
        let mut retired = PathRecord::new(11, Family::Open, ENOENT, 3, Disposition::Retired);
        retired.rewritten = true;
        assert_eq!(n.declare(retired), Ok(11));
        assert!(n.promise_holds());
        assert_eq!(n.declare(PathRecord::new(11, Family::Dup, EBADF, 3, Disposition::Wrapped)),
                   Err(TkCode::DuplicatePath));
    }

    #[test]
    fn capacity_full_keeps_ledger() {
        let mut n = TakeoverNote::new(TakeoverBoundary::file_family_full());
        let mut k = 0u16;
        while k < MAX_PATHS as u16 {
            assert!(n.declare(PathRecord::new(k, Family::Open, ENOENT, 3, Disposition::Wrapped)).is_ok());
            k += 1;
        }
        assert_eq!(n.declare(PathRecord::new(999, Family::Open, ENOENT, 3, Disposition::Wrapped)),
                   Err(TkCode::CapacityFull));
        assert_eq!(n.count(), MAX_PATHS);
        assert!(n.rejects() >= 1);
    }

    #[test]
    fn state_machine_guards() {
        let mut n = note();
        assert_eq!(n.ratify(), Err(TkCode::IllegalTransition));
        assert_eq!(n.seal(), Ok(20));
        assert_eq!(n.transition(NoteState::Draft), Ok(NoteState::Draft));
        assert_eq!(n.seal(), Ok(20));
        assert_eq!(n.ratify(), Ok(20));
        assert_eq!(n.transition(NoteState::Draft), Err(TkCode::IllegalTransition));
        assert_eq!(n.transition(NoteState::Archived), Ok(NoteState::Archived));
        assert_eq!(n.transition(NoteState::Ratified), Err(TkCode::IllegalTransition));
    }

    #[test]
    fn injection_catches_all_ten() {
        let (l, per) = run_takeover_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, INJECT_CLASSES * INJECT_ROUNDS);
        for v in per.iter() {
            assert_eq!(*v, INJECT_ROUNDS);
        }
        let (a, b, c) = judge_order_probe();
        assert_eq!((a, b, c), (TkCode::ErrnoOutOfRing, TkCode::PromiseViolated,
                               TkCode::ManifestMissingEvidence));
    }

    #[test]
    fn conservation_recon_manifest() {
        let n = note();
        let c = n.conservation();
        assert!(c.balanced());
        assert_eq!((c.zero_rewrite, c.wrapped, c.delegated, c.retired), (8, 5, 4, 3));
        assert!(reconcile(&n));
        assert_eq!(recon_total(), 20);
        let mut m = expected_manifest(&n);
        assert_eq!((m.guards, m.asserts, m.recons), (9, 28, 4));
        assert_eq!(m.commit(), Ok(41));
        assert!(m.agrees_with(&n));
    }

    #[test]
    fn checks_all_green() {
        assert_eq!(patrol_fingerprint(), patrol_fingerprint());
        assert_eq!(note().fingerprint(), note().fingerprint());
        let s = run_unx9218_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert!(s.len() >= 12);
    }
}
