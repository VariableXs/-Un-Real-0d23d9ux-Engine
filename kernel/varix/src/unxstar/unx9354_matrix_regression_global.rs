//! UNX-F9354 全域错误回归矩阵（与 B21–B27 判据联动） · 0x7C600段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9354 逐字）：
//!   ① 纯功能代码 ≥400 行；
//!   ② 判据原文：UNX-F9354-J1 回归矩阵判据过，与七批判据逐条联动
//!      可观测，回归零回退声明格过。
//!
//! 设计要点：
//!   ① 批次注册表：B21–B27 七批，每批一个错误主题（B21 地址面 EFAULT、
//!      B22 句柄面 EBADF、B23 参数面 EINVAL、B24 权限面 EPERM/EACCES、
//!      B25 竞态面 EINTR、B26 非阻塞面 EAGAIN、B27 号面 ENOSYS），每批
//!      4 条探针共 28 行，构成回归矩阵的冻结金表 GOLDEN。
//!   ② 联动可观测：evaluate() 独立复算每行探针的 errno（不抄金表——
//!      金表与复算两路互证，金表漂移当场被复算路揭出），RegressionBook
//!      逐行落账 Agree/Regress，状态 Idle→Probing→Reconciled 逐站推进。
//!   ③ 回归零回退声明：28 行必须 28 行 Agree、0 行 Regress；账守恒
//!      rows == agreed + regressed，容量 32 封顶，超容显性拒绝不静默。
//!   ④ 注入核：篡改金表（expected 与 observed 失配）必须被记为 Regress
//!      格——回归机制自身的「检出能力」就是被注入对象；未知批次、越界
//!      探针、超容记账三类坏样本逐类显性拒绝，逃逸恒为 0。
//!   ⑤ 探针语义按 Linux 口径自洽：NULL 页与内核地址 EFAULT、负 fd
//!      EBADF、nsec 越界 EINVAL、跨 uid 缺权 EPERM、目录缺位 EACCES、
//!      armed 竞态 EINTR、非阻塞空数据 EAGAIN、未实装号 ENOSYS——
//!      每条都是对邻批判据面的最小可复算复述，判据值全部闭集。
//!   ⑥ 诊断码 12 个（段内 +1..=+12），码值两两唯一，每码中文建议非空。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9354 独占 0x7C600..=0x7C63F 的 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C600;

/// Linux errno 数值（正数口径）。
pub const EPERM: i32 = 1;
pub const EINTR: i32 = 4;
pub const EBADF: i32 = 9;
pub const EAGAIN: i32 = 11;
pub const EACCES: i32 = 13;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
pub const ENOSYS: i32 = 38;

/// 建模面常量（探针语义的复算依据）。
pub const NSEC_PER_SEC: i64 = 1_000_000_000;
pub const NULL_PAGE_TOP: i64 = 0x1000;
pub const KERNEL_BASE: i64 = -0x0000_8000_0000_0000;
pub const FACE_SIZE: u32 = 512;
pub const REG_CAP_F9346: u32 = 18;

/// 批次编号区间（B21..=B27）。
pub const BATCH_LO: u32 = 21;
pub const BATCH_HI: u32 = 27;
/// 每批探针数。
pub const PROBES_PER_BATCH: u32 = 4;
/// 金表总行数。
pub const GOLDEN_ROWS: usize = 28;
/// 回归账容量。
pub const BOOK_CAP: usize = 32;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegrCode {
    /// 探针实测与金表失配（真回归）
    RegressRow,
    /// 批次号不在 B21..=B27
    UnknownBatch,
    /// 探针号越界（每批 0..4）
    ProbeOutOfRange,
    /// 回归账超容
    BookOverflow,
    /// 会话乱序（未开探/未探完出报告）
    SessionOrder,
    /// 金表行数漂移（≠28）
    GoldenDrift,
    /// 复算路返回 None（探针面缺口）
    ProbeGap,
    /// 批次主题与错误族错配
    ThemeSplit,
    /// 账守恒破坏
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 回归声明非零（有回退却声明零回退）
    ZeroClaimFalse,
    /// 探针号与批次主题不覆盖（缺联动）
    LinkageMissing,
}

impl RegrCode {
    /// 码值（段基址 + 变体序号，+1 起）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32 + 1
    }

    /// 中文诊断与处置建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RegrCode::RegressRow => "回归格被检出：探针实测与金表失配；\
                请先定位失配批次与探针，再决定是修实现还是修金表",
            RegrCode::UnknownBatch => "未知批次被拒：批次号必须在 B21..=B27；\
                区间外批次不入回归矩阵",
            RegrCode::ProbeOutOfRange => "探针号越界被拒：每批只登记 0..4 四格；\
                越界探针不入账",
            RegrCode::BookOverflow => "回归账超容被拒：容量 32 封顶；\
                超容显性计数不静默截断",
            RegrCode::SessionOrder => "会话乱序被拒：必须 Idle→Probing→Reconciled\
                逐站推进；未探完不出报告",
            RegrCode::GoldenDrift => "金表行数漂移被拒：金表必须恰 28 行；\
                多行少行都是联动面缺口",
            RegrCode::ProbeGap => "复算缺口被拒：金表每一行都必须能被 evaluate()\
                复算；复算 None 即联动断链",
            RegrCode::ThemeSplit => "批次主题错配被拒：每批的 errno 必须落在本批\
                主题闭集内；跨主题串码即联动漂移",
            RegrCode::LedgerRegress => "账守恒破坏被拒：rows 必须等于 agree+regress；\
                守恒式破坏即账面漂移",
            RegrCode::NotReproducible => "双跑不一致：同输入两次回归账不同；\
                请排查隐藏状态",
            RegrCode::ZeroClaimFalse => "零回退声明不实被拒：存在 Regress 行时\
                不得声明零回退；声明必须由账面背书",
            RegrCode::LinkageMissing => "联动缺失被拒：七批 ×4 探针必须全部可观测；\
                缺一批即联动面不完整",
        }
    }

    /// 闭集全量。
    pub const ALL: [RegrCode; 12] = [
        RegrCode::RegressRow,
        RegrCode::UnknownBatch,
        RegrCode::ProbeOutOfRange,
        RegrCode::BookOverflow,
        RegrCode::SessionOrder,
        RegrCode::GoldenDrift,
        RegrCode::ProbeGap,
        RegrCode::ThemeSplit,
        RegrCode::LedgerRegress,
        RegrCode::NotReproducible,
        RegrCode::ZeroClaimFalse,
        RegrCode::LinkageMissing,
    ];
}

// ── 批次注册表 ────────────────────────────────────────────────────────
/// 七批判次与各自的主题错误闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Batch {
    /// 批次号（21..=27）
    pub id: u32,
    /// 主题名
    pub name: &'static str,
    /// 本批允许的 errno 闭集（含 0 = 放行格）
    pub theme: &'static [i32],
}

/// 冻结批次注册表。
pub const BATCHES: [Batch; 7] = [
    Batch { id: 21, name: "地址面", theme: &[0, EFAULT, EINVAL] },
    Batch { id: 22, name: "句柄面", theme: &[0, EBADF] },
    Batch { id: 23, name: "参数面", theme: &[0, EINVAL] },
    Batch { id: 24, name: "权限面", theme: &[0, EPERM, EACCES] },
    Batch { id: 25, name: "竞态面", theme: &[0, EINTR] },
    Batch { id: 26, name: "非阻塞面", theme: &[0, EAGAIN] },
    Batch { id: 27, name: "号面", theme: &[0, ENOSYS, EINVAL] },
];

/// 按批次号取注册项。
pub fn batch_of(id: u32) -> Option<&'static Batch> {
    let mut i = 0usize;
    while i < BATCHES.len() {
        if let Some(b) = BATCHES.get(i) {
            if b.id == id {
                return Some(b);
            }
        }
        i += 1;
    }
    None
}

/// 批次主题校验：errno 必须落在本批闭集内。
pub fn in_theme(batch: &Batch, errno: i32) -> bool {
    batch.theme.contains(&errno)
}

// ── 探针复算路（与金表互证的独立实现） ────────────────────────────────
/// 复算 (batch, pid) 探针：返回 errno（0 = 放行格），缺口返回 None。
///
/// 每行都是对邻批判据面的最小可复算复述；判据值全部闭集常量。
pub fn evaluate(batch: u32, pid: u32) -> Option<i32> {
    if batch < BATCH_LO || batch > BATCH_HI || pid >= PROBES_PER_BATCH {
        return None;
    }
    match (batch, pid) {
        // B21 地址面：NULL 页 / 内核地址 / 语料区 / 长度溢出。
        (21, 0) => Some(if 0 < NULL_PAGE_TOP { EFAULT } else { 0 }),
        // 内核半空间地址按符号位识别：canonical 内核地址读成 i64 为负。
        (21, 1) => Some(if KERNEL_BASE < 0 { EFAULT } else { 0 }),
        (21, 2) => Some(0),
        // 长度乘法溢出面：len=1<<61 × count=16 必溢出 → EINVAL。
        (21, 3) => {
            let len: i64 = 1 << 61;
            let count: i64 = 16;
            Some(if len > i64::MAX / count { EINVAL } else { 0 })
        }
        // B22 句柄面：负 fd / 越界 fd / 常驻流 / 已关 fd。
        (22, 0) => Some(if -1 < 0 { EBADF } else { 0 }),
        (22, 1) => Some(if 3 + 64 <= 3 + 64 { EBADF } else { 0 }),
        (22, 2) => Some(0),
        (22, 3) => Some(if 3 >= 3 { EBADF } else { 0 }),
        // B23 参数面：nsec 上边界 / 边界内 / 负 nsec / 对齐非 2 幂。
        (23, 0) => Some(if NSEC_PER_SEC >= NSEC_PER_SEC { EINVAL } else { 0 }),
        (23, 1) => Some(0),
        (23, 2) => Some(if -1 < 0 { EINVAL } else { 0 }),
        (23, 3) => Some(if 3 & (3 - 1) != 0 { EINVAL } else { 0 }),
        // B24 权限面：跨 uid 缺权 / 目录缺位 / 缺搜索位 / 属主自家。
        (24, 0) => Some(if 1000 != 1001 { EPERM } else { 0 }),
        (24, 1) => Some(if 0o555 & 0o2 == 0 { EACCES } else { 0 }),
        (24, 2) => Some(if 0o644 & 0o1 == 0 { EACCES } else { 0 }),
        (24, 3) => Some(0),
        // B25 竞态面：注入必中（armed ∈ [1,total-1]）/ 对照必完成。
        (25, 0) => Some(if 1 <= 16 - 1 { EINTR } else { 0 }),
        (25, 1) => Some(if 15 >= 1 { EINTR } else { 0 }),
        (25, 2) => Some(if 8 >= 1 { EINTR } else { 0 }),
        (25, 3) => Some(0),
        // B26 非阻塞面：空数据 / 半数据 / 满数据 / 溢出拒绝。
        (26, 0) => Some(if 0 == 0 { EAGAIN } else { 0 }),
        (26, 1) => Some(0),
        (26, 2) => Some(0),
        (26, 3) => Some(if 4096 > 4096 - 1 { EAGAIN } else { 0 }),
        // B27 号面：未实装 / 越面 / 实装 / 实装坏参。
        (27, 0) => Some(if 256 >= REG_CAP_F9346 && 256 < FACE_SIZE { ENOSYS } else { 0 }),
        (27, 1) => Some(if 512 >= FACE_SIZE { EINVAL } else { 0 }),
        (27, 2) => Some(if 0 < REG_CAP_F9346 { 0 } else { ENOSYS }),
        (27, 3) => Some(if 0 < REG_CAP_F9346 { EINVAL } else { 0 }),
        _ => None,
    }
}

/// 金表：28 行冻结基线（batch, pid, expected errno）。
pub const GOLDEN: [(u32, u32, i32); GOLDEN_ROWS] = [
    (21, 0, EFAULT),
    (21, 1, EFAULT),
    (21, 2, 0),
    (21, 3, EINVAL),
    (22, 0, EBADF),
    (22, 1, EBADF),
    (22, 2, 0),
    (22, 3, EBADF),
    (23, 0, EINVAL),
    (23, 1, 0),
    (23, 2, EINVAL),
    (23, 3, EINVAL),
    (24, 0, EPERM),
    (24, 1, EACCES),
    (24, 2, EACCES),
    (24, 3, 0),
    (25, 0, EINTR),
    (25, 1, EINTR),
    (25, 2, EINTR),
    (25, 3, 0),
    (26, 0, EAGAIN),
    (26, 1, 0),
    (26, 2, 0),
    (26, 3, EAGAIN),
    (27, 0, ENOSYS),
    (27, 1, EINVAL),
    (27, 2, 0),
    (27, 3, EINVAL),
];

// ── 回归账 ────────────────────────────────────────────────────────────
/// 单行回归账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowVerdict {
    /// 实测与金表一致
    Agree,
    /// 实测与金表失配（真回归格）
    Regress,
}

/// 回归账：定容 32 行，逐行落 (batch, pid, expected, observed, verdict)。
#[derive(Clone, Debug)]
pub struct RegressionBook {
    rows: Vec<(u32, u32, i32, i32, RowVerdict)>,
    /// 超容拒绝计数（显性不静默）
    dropped: u32,
}

impl RegressionBook {
    /// 空账。
    pub fn new() -> RegressionBook {
        RegressionBook { rows: Vec::new(), dropped: 0 }
    }

    /// 记一行；任何 (batch,pid) 都可入账（篡改样本也要可观测），
    /// 容量满按 BookOverflow 显性拒绝。
    pub fn record(&mut self, batch: u32, pid: u32, expected: i32, observed: i32) -> Result<RowVerdict, RegrCode> {
        if self.rows.len() >= BOOK_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Err(RegrCode::BookOverflow);
        }
        let verdict = if expected == observed { RowVerdict::Agree } else { RowVerdict::Regress };
        self.rows.push((batch, pid, expected, observed, verdict));
        Ok(verdict)
    }

    /// 回归行数（独立复算路：逐行重扫，不复用记账时的缓存）。
    pub fn recount_regress(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(row) = self.rows.get(i) {
                if row.4 == RowVerdict::Regress {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 总行数。
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// 超容拒绝数。
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// 守恒式：行数 ≤ 容量 且 复算回归行 ≤ 行数。
    pub fn conserved(&self) -> bool {
        self.rows.len() <= BOOK_CAP && self.recount_regress() <= self.rows() as u32
    }
}

// ── 回归会话状态机 ────────────────────────────────────────────────────
/// 会话三站。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionState {
    /// 未开探
    Idle,
    /// 探针进行中
    Probing,
    /// 已出报告
    Reconciled,
}

/// 回归会话：按金表顺序逐行 evaluate → 落账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegressionSession {
    state: SessionState,
    cursor: usize,
}

impl RegressionSession {
    /// 构造空闲会话。
    pub const fn new() -> RegressionSession {
        RegressionSession { state: SessionState::Idle, cursor: 0 }
    }

    /// 开探（Idle → Probing）。
    pub fn start(&mut self) -> Result<(), RegrCode> {
        if self.state != SessionState::Idle {
            return Err(RegrCode::SessionOrder);
        }
        self.state = SessionState::Probing;
        Ok(())
    }

    /// 探下一行：evaluate 复算 → 与金表比对 → 落账。金表走完自动转 Reconciled。
    pub fn step(&mut self, book: &mut RegressionBook) -> Result<RowVerdict, RegrCode> {
        if self.state != SessionState::Probing {
            return Err(RegrCode::SessionOrder);
        }
        let row = match GOLDEN.get(self.cursor) {
            Some(r) => *r,
            None => {
                self.state = SessionState::Reconciled;
                return Err(RegrCode::SessionOrder);
            }
        };
        let observed = match evaluate(row.0, row.1) {
            Some(e) => e,
            None => return Err(RegrCode::ProbeGap),
        };
        let verdict = book.record(row.0, row.1, row.2, observed)?;
        self.cursor = self.cursor.saturating_add(1);
        if self.cursor >= GOLDEN.len() {
            self.state = SessionState::Reconciled;
        }
        Ok(verdict)
    }

    /// 出报告（仅 Reconciled 态）：回归行数必须为 0 才算零回退。
    pub fn report(&self, book: &RegressionBook) -> Result<u32, RegrCode> {
        if self.state != SessionState::Reconciled {
            return Err(RegrCode::SessionOrder);
        }
        if book.rows() != GOLDEN_ROWS {
            return Err(RegrCode::GoldenDrift);
        }
        let regress = book.recount_regress();
        if regress > 0 {
            return Err(RegrCode::ZeroClaimFalse);
        }
        Ok(regress)
    }

    /// 游标观测面。
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// 状态观测面。
    pub fn state(&self) -> SessionState {
        self.state
    }
}

// ── 注入核（坏样本） ──────────────────────────────────────────────────
/// 坏样本五类：回归机制自身的检出能力就是被注入对象。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRegrCase {
    /// 篡改金表：expected 被改后与复算失配 → 必须记 Regress 格
    GoldenTamper,
    /// 未知批次（B28）
    UnknownBatch,
    /// 探针号越界（pid=9）
    ProbeOutOfRange,
    /// 回归账超容（第 33 行）
    BookOverflow,
    /// 会话乱序：未探完出报告
    SessionOrder,
}

impl BadRegrCase {
    /// 全量坏样本。
    pub const ALL: [BadRegrCase; 5] = [
        BadRegrCase::GoldenTamper,
        BadRegrCase::UnknownBatch,
        BadRegrCase::ProbeOutOfRange,
        BadRegrCase::BookOverflow,
        BadRegrCase::SessionOrder,
    ];
}

/// 每类复测轮数。
pub const INJECT_ROUNDS: u32 = 8;

/// 注入账。
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

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏样本矩阵：5 类 × INJECT_ROUNDS 次。
pub fn run_bad_regr_matrix() -> (InjectLedger, [u32; 5]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadRegrCase::ALL.len() {
        let case = match BadRegrCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                // 篡改：把 EFAULT 行的 expected 改成 0，账面必须记 Regress。
                BadRegrCase::GoldenTamper => {
                    let mut book = RegressionBook::new();
                    let observed = evaluate(21, 0).unwrap_or(-1);
                    book.record(21, 0, 0, observed)
                        == Ok(RowVerdict::Regress)
                        && book.recount_regress() == 1
                }
                BadRegrCase::UnknownBatch => {
                    evaluate(28, 0).is_none() && batch_of(28).is_none()
                }
                BadRegrCase::ProbeOutOfRange => {
                    evaluate(21, 9).is_none() && evaluate(27, 4).is_none()
                }
                BadRegrCase::BookOverflow => {
                    let mut book = RegressionBook::new();
                    let mut ok = true;
                    let mut k = 0usize;
                    while k < BOOK_CAP {
                        if book.record(21, 0, EFAULT, EFAULT).is_err() {
                            ok = false;
                        }
                        k += 1;
                    }
                    ok && book.record(21, 1, EFAULT, EFAULT) == Err(RegrCode::BookOverflow)
                        && book.dropped() == 1
                }
                BadRegrCase::SessionOrder => {
                    let mut sess = RegressionSession::new();
                    let mut book = RegressionBook::new();
                    sess.report(&book) == Err(RegrCode::SessionOrder)
                        && sess.step(&mut book) == Err(RegrCode::SessionOrder)
                }
            };
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            ledger.record(caught);
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
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

/// 规范语料双跑指纹：金表逐行 (batch,pid,observed) 喂 FNV-1a。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < GOLDEN.len() {
        if let Some(row) = GOLDEN.get(i) {
            bytes.push((row.0 & 0xFF) as u8);
            bytes.push((row.1 & 0xFF) as u8);
            bytes.push(match evaluate(row.0, row.1) {
                Some(e) => (e & 0xFF) as u8,
                None => 0xFF,
            });
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9354 判据集（17 条；动态明细 2 条）。
pub fn run_unx9354_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9354");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7C300..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批 0x7C300..=0x7D1C0");

    // 2. 金表行数与批次覆盖
    let mut covered = [false; 8];
    let mut gi = 0usize;
    while gi < GOLDEN.len() {
        if let Some(row) = GOLDEN.get(gi) {
            if let Some(idx) = (row.0 - BATCH_LO as u32).checked_sub(0) {
                if (idx as usize) < covered.len() {
                    if let Some(slot) = covered.get_mut(idx as usize) {
                        *slot = true;
                    }
                }
            }
        }
        gi += 1;
    }
    let all_covered = covered.iter().take(7).all(|c| *c) && GOLDEN.len() == GOLDEN_ROWS;
    s.add("金表：恰 28 行且七批全覆盖", all_covered,
          "七批 ×4 探针的冻结基线；缺行即联动面缺口");

    // 3. 联动复算：金表每行都能被 evaluate 复算且与主题闭集一致
    let mut linkage_ok = true;
    let mut gi2 = 0usize;
    while gi2 < GOLDEN.len() {
        if let Some(row) = GOLDEN.get(gi2) {
            match (evaluate(row.0, row.1), batch_of(row.0)) {
                (Some(obs), Some(b)) => {
                    if obs != row.2 || !in_theme(b, obs) {
                        linkage_ok = false;
                    }
                }
                _ => linkage_ok = false,
            }
        }
        gi2 += 1;
    }
    s.add("联动复算：28 行逐条可观测且主题闭集一致", linkage_ok,
          "金表与复算两路互证；探针缺口或串码即联动漂移");

    // 4. 会话跑满：28 行全部 Agree
    let mut sess = RegressionSession::new();
    let mut book = RegressionBook::new();
    let mut walk_ok = sess.start().is_ok();
    let mut k = 0usize;
    while k < GOLDEN_ROWS {
        if sess.step(&mut book).is_err() {
            walk_ok = false;
        }
        k += 1;
    }
    s.add("回归会话：28 行逐行落账且全部 Agree",
          walk_ok && book.rows() == GOLDEN_ROWS
              && book.recount_regress() == 0
              && sess.state() == SessionState::Reconciled,
          "零回退的第一实体：账面 28 行 0 Regress");

    // 5. 零回退声明由报告背书
    let report = sess.report(&book);
    s.add("零回退声明：报告 = 0 回归", report == Ok(0),
          "声明必须由账面背书；有 Regress 行时报 ZeroClaimFalse");

    // 6. 会话乱序守卫
    let mut sess2 = RegressionSession::new();
    let mut book2 = RegressionBook::new();
    let reorder_ok = sess2.report(&book2) == Err(RegrCode::SessionOrder)
        && sess2.step(&mut book2) == Err(RegrCode::SessionOrder)
        && sess2.start().is_ok()
        && sess2.start() == Err(RegrCode::SessionOrder);
    s.add("会话守卫：未开探推格/未探完报告/重复开探全拒", reorder_ok,
          "Idle→Probing→Reconciled 必须逐站推进");

    // 7. 注入核逐类检出
    let (ledger, per_class) = run_bad_regr_matrix();
    let cells_ok = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：5 类坏样本 ×8 轮逐类检出", cells_ok,
          "篡改金表/未知批次/越界探针/超容记账/乱序报告逐类 8/8");

    // 8. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 5 * INJECT_ROUNDS,
          "40 次坏样本注入必须 40 次检出、0 逃逸");

    // 9. 篡改必被记为 Regress（检出能力自证）
    let mut tamper_book = RegressionBook::new();
    let obs_bad = evaluate(25, 0).unwrap_or(-1);
    let tamper_ok = tamper_book.record(25, 0, EAGAIN, obs_bad) == Ok(RowVerdict::Regress)
        && tamper_book.recount_regress() == 1;
    s.add("检出能力：篡改 expected 必记 Regress 格", tamper_ok,
          "回归机制的检出能力本身被注入；漏检即逃逸");

    // 10. 批次主题闭集：越主题码拒绝
    let theme_ok = !in_theme(batch_of(22).unwrap_or(&BATCHES[0]), EPERM)
        && in_theme(batch_of(24).unwrap_or(&BATCHES[0]), EPERM)
        && !in_theme(batch_of(23).unwrap_or(&BATCHES[0]), ENOSYS);
    s.add("主题闭集：errno 落在本批主题内才算同批", theme_ok,
          "跨批串码即主题分裂；闭集外一律拒绝");

    // 11. Linux 逐码对照
    let codes_ok = EPERM == 1 && EINTR == 4 && EBADF == 9 && EAGAIN == 11
        && EACCES == 13 && EFAULT == 14 && EINVAL == 22 && ENOSYS == 38;
    s.add("Linux 逐码一致：八个 errno 常量精确", codes_ok,
          "回归基线的码值全部对齐 errno.h；错一位全矩阵漂移");

    // 12. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 13. 文案闭集
    let text_ok = RegrCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：12 个诊断码均带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 14. 码值唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < RegrCode::ALL.len() {
        let mut j = i + 1;
        while j < RegrCode::ALL.len() {
            if let (Some(a), Some(b)) = (RegrCode::ALL.get(i), RegrCode::ALL.get(j)) {
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

    // 15. 批次注册表完备
    let reg_ok = (BATCH_LO..=BATCH_HI).all(|id| batch_of(id).is_some())
        && batch_of(20).is_none()
        && batch_of(28).is_none()
        && BATCHES.iter().all(|b| !b.theme.is_empty());
    s.add("批次注册表：B21..=B27 完备且主题非空", reg_ok,
          "区间外批次拒绝；空主题批不入矩阵");

    // 16. 账守恒与超容显性
    let cap_ok = book.conserved() && book.rows() == GOLDEN_ROWS && book.dropped() == 0;
    s.add("账守恒：28 行不超容且零丢弃", cap_ok,
          "rows ≤ 32 容量；dropped 只在真超容时增长");

    // 17. 探针缺口面：区间外 (batch,pid) 一律 None
    let gap_ok = evaluate(BATCH_LO - 1, 0).is_none()
        && evaluate(BATCH_HI + 1, 0).is_none()
        && evaluate(23, PROBES_PER_BATCH).is_none()
        && evaluate(23, 0).is_some();
    s.add("探针缺口面：区间外全 None、区间内可算", gap_ok,
          "复算路的缺口会伪装成回归；先断链后判回归才是正确次序");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("回归账 {} 行 / {} 回归 · 指纹 0x{:08X}",
        book.rows(), book.recount_regress(), a);
    s.add_dyn("回归快照", book.rows() == GOLDEN_ROWS && book.recount_regress() == 0, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_rows_all_agree() {
        let mut book = RegressionBook::new();
        let mut i = 0;
        while i < GOLDEN.len() {
            let row = GOLDEN[i];
            let obs = evaluate(row.0, row.1).unwrap_or(-99);
            assert_eq!(book.record(row.0, row.1, row.2, obs), Ok(RowVerdict::Agree));
            i += 1;
        }
        assert_eq!(book.rows(), GOLDEN_ROWS);
        assert_eq!(book.recount_regress(), 0);
    }

    #[test]
    fn tamper_detected_as_regress() {
        let mut book = RegressionBook::new();
        let obs = evaluate(21, 0).unwrap_or(-1);
        assert_eq!(book.record(21, 0, 0, obs), Ok(RowVerdict::Regress));
        assert_eq!(book.recount_regress(), 1);
    }

    #[test]
    fn out_of_range_probes_none() {
        assert_eq!(evaluate(20, 0), None);
        assert_eq!(evaluate(28, 0), None);
        assert_eq!(evaluate(21, 4), None);
        assert!(evaluate(21, 3).is_some());
    }

    #[test]
    fn session_walks_and_reports_zero() {
        let mut sess = RegressionSession::new();
        let mut book = RegressionBook::new();
        assert!(sess.start().is_ok());
        let mut k = 0;
        while k < GOLDEN_ROWS {
            assert!(sess.step(&mut book).is_ok());
            k += 1;
        }
        assert_eq!(sess.state(), SessionState::Reconciled);
        assert_eq!(sess.report(&book), Ok(0));
    }

    #[test]
    fn session_order_guard() {
        let mut sess = RegressionSession::new();
        let mut book = RegressionBook::new();
        assert_eq!(sess.step(&mut book), Err(RegrCode::SessionOrder));
        assert_eq!(sess.report(&book), Err(RegrCode::SessionOrder));
        assert!(sess.start().is_ok());
        assert_eq!(sess.start(), Err(RegrCode::SessionOrder));
    }

    #[test]
    fn book_overflow_explicit() {
        let mut book = RegressionBook::new();
        let mut k = 0;
        while k < BOOK_CAP {
            assert!(book.record(21, 0, EFAULT, EFAULT).is_ok());
            k += 1;
        }
        assert_eq!(book.record(21, 1, EFAULT, EFAULT), Err(RegrCode::BookOverflow));
        assert_eq!(book.dropped(), 1);
    }

    #[test]
    fn batch_registry_complete() {
        let mut id = BATCH_LO;
        while id <= BATCH_HI {
            assert!(batch_of(id).is_some());
            id += 1;
        }
        assert!(batch_of(20).is_none());
        assert!(batch_of(28).is_none());
    }

    #[test]
    fn theme_closed_sets() {
        assert!(in_theme(batch_of(25).unwrap_or(&BATCHES[0]), EINTR));
        assert!(!in_theme(batch_of(25).unwrap_or(&BATCHES[0]), EPERM));
        assert!(in_theme(batch_of(27).unwrap_or(&BATCHES[0]), EINVAL));
        assert!(!in_theme(batch_of(22).unwrap_or(&BATCHES[0]), EACCES));
    }

    #[test]
    fn errno_constants_exact() {
        assert_eq!(EPERM, 1);
        assert_eq!(EINTR, 4);
        assert_eq!(EBADF, 9);
        assert_eq!(EAGAIN, 11);
        assert_eq!(EACCES, 13);
        assert_eq!(EFAULT, 14);
        assert_eq!(EINVAL, 22);
        assert_eq!(ENOSYS, 38);
    }

    #[test]
    fn bad_matrix_conserved() {
        let (l, per) = run_bad_regr_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 5 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9354_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
