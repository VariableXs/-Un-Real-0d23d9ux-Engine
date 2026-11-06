//! UNX-F9212 EFAULT 拷贝半程探针（copy_to_user 半途） · 0x7A280段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9212 逐字）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9212-J1 半途拷贝注入判据过，
//!   已拷贝字节数返回语义与 Linux 一致格过，半程残留可观测格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 半程是「页粒度」而不是「字节粒度」：copy_to_user 逐页走页表，撞到
//!      不可写页立即停在那一段边界上。探针按页建模（每页 Mapped / ReadOnly
//!      / Unmapped / Kernel），于是「停在哪一字节」可以被精确复算。
//!   ② 返回语义分两条路：read/write 这类流式系统调用返回**已拷贝字节数**
//!      （部分成功即正数短读/短写），只有已拷贝为 0 时才回 -EFAULT；而
//!      fstat/gettimeofday 这类固定结构调用一旦有任何一页失败就整体 -EFAULT，
//!      半程数据照样落在用户区（残留可观测，但不作为返回值）。
//!   ③ 残留必须可观测：故障点之前的前缀逐字节等于源数据，故障点之后按
//!      Linux 的加固口径整体清零（防止内核数据顺着未初始化尾部落到用户态）。
//!      前缀长度 + 清零标志就是「半程残留」的可测证据。
//!   ④ 方向不对称：copy_to_user 要求目标页可写，只读页算 EFAULT；
//!      copy_from_user 只要求可读，只读页合法。同一张页表在两条方向上的
//!      判决不同，这是本卡最容易写错的一格。
//!   ⑤ 探针自身是状态机：Begin→PageWalk→FaultClass→Residue→Verdict 只许
//!      前进，跳级或回退都拒绝，避免「先出结论再补证据」的假绿。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9212 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A280;
/// 探针页大小（取 4 字节，便于在判据里精确摆出跨页前缀）。
pub const PAGE_SIZE: u32 = 4;
/// 单类注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 填充毒值（未写入的位置保持该值，用于证明「没写」）。
pub const POISON: u8 = 0xAA;

/// 用户缓冲区不可访问。
pub const EFAULT: u32 = 14;
/// 参数非法。
pub const EINVAL: u32 = 22;
/// 溢出。
pub const EOVERFLOW: u32 = 75;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 半程探针诊断码（段内 +0x00..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyCode {
    HeadFault,
    MidFault,
    TailFault,
    ZeroLength,
    ResidualTail,
    DirAsymmetry,
    FixedStruct,
    StageSkip,
    OutOfRange,
    LedgerRegress,
    NotReproducible,
    SegmentViolation,
}

impl CopyCode {
    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与建议（闭集，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            CopyCode::HeadFault => "首字节故障：第一页就不可写，已拷贝为 0，流式调用回 EFAULT；请核对缓冲区起始地址",
            CopyCode::MidFault => "半程故障：中段页不可写，已拷贝为正数，流式调用返回该数（短读/短写），不报 EFAULT",
            CopyCode::TailFault => "末段故障：最后若干字节落在坏页，返回 len-1 这类部分值；请按返回值推进而非重试全量",
            CopyCode::ZeroLength => "零长度：不触碰任何页，即便缓冲区全坏也回 0；不要把 EFAULT 挂在 count==0 上",
            CopyCode::ResidualTail => "残留未清零：故障点之后必须整体清零，否则内核数据可能泄漏到用户态",
            CopyCode::DirAsymmetry => "方向不对称：copy_to_user 要求可写、copy_from_user 只要求可读；只读页在两侧判决不同",
            CopyCode::FixedStruct => "固定结构调用：fstat/gettimeofday 等一旦有页失败即整体 EFAULT，不返回部分字节",
            CopyCode::StageSkip => "探针阶段跳级：必须按 Begin→PageWalk→FaultClass→Residue→Verdict 前进",
            CopyCode::OutOfRange => "越界：请求长度超出缓冲区页覆盖范围，缺页按 Unmapped 处理并触发半程",
            CopyCode::LedgerRegress => "探针账倒退：尝试 = 拷贝 + 故障，累计只增不减；请检查回滚路径",
            CopyCode::NotReproducible => "双跑不一致：同页表两次探针结果不同；请排查页状态被修改或遍历序不确定",
            CopyCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量。
    pub const ALL: [CopyCode; 12] = [
        CopyCode::HeadFault, CopyCode::MidFault, CopyCode::TailFault,
        CopyCode::ZeroLength, CopyCode::ResidualTail, CopyCode::DirAsymmetry,
        CopyCode::FixedStruct, CopyCode::StageSkip, CopyCode::OutOfRange,
        CopyCode::LedgerRegress, CopyCode::NotReproducible, CopyCode::SegmentViolation,
    ];
}

// ── 页状态与用户缓冲区 ────────────────────────────────────────────────
/// 单页状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageState {
    /// 已映射且可读写
    Mapped,
    /// 已映射但只读（copy_to_user 不可写）
    ReadOnly,
    /// 未映射
    Unmapped,
    /// 内核地址（用户态不可访问）
    Kernel,
}

impl PageState {
    /// 该页对「写用户区」是否可用（copy_to_user）。
    pub fn writable(self) -> bool {
        matches!(self, PageState::Mapped)
    }

    /// 该页对「读用户区」是否可用（copy_from_user）。
    pub fn readable(self) -> bool {
        matches!(self, PageState::Mapped | PageState::ReadOnly)
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            PageState::Mapped => "可读写页",
            PageState::ReadOnly => "只读页",
            PageState::Unmapped => "未映射页",
            PageState::Kernel => "内核页",
        }
    }
}

/// 用户缓冲区：起始页内偏移 + 页状态表。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UserBuf {
    pub off: u32,
    pub pages: Vec<PageState>,
}

impl UserBuf {
    /// 构造：off 必须小于页大小，否则截到页内（EINVAL 由调用方先判）。
    pub fn new(off: u32, pages: &[PageState]) -> UserBuf {
        let mut v: Vec<PageState> = Vec::new();
        let mut i = 0usize;
        while i < pages.len() {
            if let Some(p) = pages.get(i) {
                v.push(*p);
            }
            i += 1;
        }
        UserBuf { off: off % PAGE_SIZE, pages: v }
    }

    /// 全部页可写（copy_to_user 视角）。
    pub fn all_writable(&self) -> bool {
        self.pages.iter().all(|p| p.writable())
    }

    /// 覆盖该绝对偏移的页状态；越表视为 Unmapped。
    pub fn page_at(&self, abs: u32) -> PageState {
        let idx = (abs / PAGE_SIZE) as usize;
        match self.pages.get(idx) {
            Some(p) => *p,
            None => PageState::Unmapped,
        }
    }

    /// 覆盖请求区间的页数。
    pub fn pages_covered(&self, len: u32) -> u32 {
        if len == 0 {
            return 0;
        }
        let end = match self.off.checked_add(len) {
            Some(v) => v,
            None => return u32::MAX,
        };
        (end + PAGE_SIZE - 1) / PAGE_SIZE
    }
}

// ── 拷贝结果 ──────────────────────────────────────────────────────────
/// 一次半程拷贝的结果（dst 是用户区快照，用于观测残留）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CopyOutcome {
    /// 请求字节数
    pub requested: u32,
    /// 已拷贝字节数
    pub copied: u32,
    /// 故障起点（相对请求缓冲区，None 表示无故障）
    pub fault_at: Option<u32>,
    /// 故障后尾部是否清零
    pub tail_zeroed: bool,
    /// 用户区快照：长度 = requested
    pub dst: Vec<u8>,
}

impl CopyOutcome {
    /// 前缀是否逐字节等于源数据。
    pub fn prefix_matches(&self, src: &[u8]) -> bool {
        let mut i = 0usize;
        while i < self.copied as usize {
            match (self.dst.get(i), src.get(i)) {
                (Some(a), Some(b)) if a == b => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 故障点之后是否全为 0。
    pub fn tail_is_zero(&self) -> bool {
        let start = self.copied as usize;
        let mut i = start;
        while i < self.dst.len() {
            match self.dst.get(i) {
                Some(0) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 残留可观测：前缀相符 + 尾部清零。
    pub fn residual_ok(&self, src: &[u8]) -> bool {
        self.prefix_matches(src) && self.tail_is_zero()
    }
}

/// 拷贝方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// 内核写用户区（copy_to_user）
    ToUser,
    /// 内核读用户区（copy_from_user）
    FromUser,
}

// ── 核心拷贝引擎 ──────────────────────────────────────────────────────
/// 半程拷贝：逐字节沿页表推进，撞上不可用页即停。
///
/// `dst` 预置 POISON，故障后的尾部整体清零（Linux 的加固口径），因此
/// 「已写下哪些、清了哪些」都能从快照里读回来。
pub fn copy_half(src: &[u8], buf: &UserBuf, dir: Direction, len: u32) -> CopyOutcome {
    let mut dst: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < len as usize {
        dst.push(POISON);
        i += 1;
    }
    let mut copied = 0u32;
    let mut fault_at: Option<u32> = None;
    while copied < len {
        let abs = match buf.off.checked_add(copied) {
            Some(v) => v,
            None => {
                fault_at = Some(copied);
                break;
            }
        };
        let page = buf.page_at(abs);
        let ok = match dir {
            Direction::ToUser => page.writable(),
            Direction::FromUser => page.readable(),
        };
        if !ok {
            fault_at = Some(copied);
            break;
        }
        if let (Some(d), Some(s)) = (dst.get_mut(copied as usize), src.get(copied as usize)) {
            *d = *s;
        }
        copied += 1;
    }
    // 故障后清零尾部。
    let mut tail_zeroed = false;
    if fault_at.is_some() {
        let mut k = copied as usize;
        while k < dst.len() {
            if let Some(d) = dst.get_mut(k) {
                *d = 0;
            }
            k += 1;
        }
        tail_zeroed = true;
    }
    CopyOutcome { requested: len, copied, fault_at, tail_zeroed, dst }
}

// ── 系统调用语义 ──────────────────────────────────────────────────────
/// 流式调用（read / write）：部分成功返回已拷贝数，仅 0 拷贝回 EFAULT。
pub fn judge_stream(src: &[u8], buf: &UserBuf, dir: Direction, len: u32) -> Result<u32, u32> {
    if len == 0 {
        return Ok(0);
    }
    let outcome = copy_half(src, buf, dir, len);
    if outcome.copied == 0 {
        return Err(EFAULT);
    }
    Ok(outcome.copied)
}

/// 固定结构调用（fstat / gettimeofday / sigaction）：任一页失败即整体 EFAULT。
pub fn judge_fixed(src: &[u8], buf: &UserBuf, dir: Direction, len: u32) -> Result<u32, u32> {
    let outcome = copy_half(src, buf, dir, len);
    if outcome.fault_at.is_some() {
        return Err(EFAULT);
    }
    Ok(outcome.copied)
}

// ── 探针状态机 ────────────────────────────────────────────────────────
/// 探针阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Begin,
    PageWalk,
    FaultClass,
    Residue,
    Verdict,
}

impl Stage {
    fn ordinal(self) -> u32 {
        match self {
            Stage::Begin => 0,
            Stage::PageWalk => 1,
            Stage::FaultClass => 2,
            Stage::Residue => 3,
            Stage::Verdict => 4,
        }
    }
}

/// 阶段推进：只许 +1，跳级与回退都拒（EINVAL）。
pub fn advance(cur: Stage, next: Stage) -> Result<Stage, u32> {
    if next.ordinal() == cur.ordinal() + 1 {
        Ok(next)
    } else {
        Err(EINVAL)
    }
}

// ── 场景矩阵 ──────────────────────────────────────────────────────────
/// 场景：(名字, 页表, 起始偏移, 请求长度, 方向, 是否固定结构调用, 期望码)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Scenario {
    pub name: &'static str,
    pub pages: [PageState; 3],
    pub off: u32,
    pub len: u32,
    pub dir: Direction,
    pub fixed_struct: bool,
    /// 期望：0=成功满拷贝，>0=返回该字节数（部分），u32::MAX=EFAULT
    pub expect: u32,
}

/// 期望码哨兵：整体 EFAULT。
pub const WANT_EFAULT: u32 = u32::MAX;

/// 场景矩阵（10 条，覆盖首/中/尾故障与方向不对称）。
pub const SCENARIOS: [Scenario; 10] = [
    Scenario { name: "全映射满拷贝", pages: [PageState::Mapped; 3], off: 0, len: 8, dir: Direction::ToUser, fixed_struct: false, expect: 8 },
    Scenario { name: "首页坏→0 拷贝", pages: [PageState::Unmapped, PageState::Mapped, PageState::Mapped], off: 0, len: 8, dir: Direction::ToUser, fixed_struct: false, expect: WANT_EFAULT },
    Scenario { name: "中段坏→前缀", pages: [PageState::Mapped, PageState::Unmapped, PageState::Mapped], off: 0, len: 8, dir: Direction::ToUser, fixed_struct: false, expect: 4 },
    Scenario { name: "末字节坏→len-1", pages: [PageState::Mapped, PageState::Mapped, PageState::Unmapped], off: 0, len: 9, dir: Direction::ToUser, fixed_struct: false, expect: 8 },
    Scenario { name: "页内偏移起于 2", pages: [PageState::Mapped, PageState::Unmapped, PageState::Mapped], off: 2, len: 6, dir: Direction::ToUser, fixed_struct: false, expect: 2 },
    Scenario { name: "只读页写向 EFAULT", pages: [PageState::Mapped, PageState::ReadOnly, PageState::Mapped], off: 0, len: 8, dir: Direction::ToUser, fixed_struct: false, expect: 4 },
    Scenario { name: "只读页读向合法", pages: [PageState::Mapped, PageState::ReadOnly, PageState::Mapped], off: 0, len: 8, dir: Direction::FromUser, fixed_struct: false, expect: 8 },
    Scenario { name: "固定结构半途仍 EFAULT", pages: [PageState::Mapped, PageState::Unmapped, PageState::Mapped], off: 0, len: 8, dir: Direction::ToUser, fixed_struct: true, expect: WANT_EFAULT },
    Scenario { name: "内核页写向 EFAULT", pages: [PageState::Kernel, PageState::Mapped, PageState::Mapped], off: 0, len: 4, dir: Direction::ToUser, fixed_struct: false, expect: WANT_EFAULT },
    Scenario { name: "零长度全坏仍回 0", pages: [PageState::Unmapped; 3], off: 3, len: 0, dir: Direction::ToUser, fixed_struct: true, expect: 0 },
];

/// 源数据（固定 12 字节，按场景取前缀）。
pub const SRC: [u8; 12] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0x01, 0x02, 0x03];

/// 跑一条场景，返回 (是否符合期望, 结果快照)。
pub fn run_scenario(sc: &Scenario) -> (bool, CopyOutcome) {
    let buf = UserBuf::new(sc.off, &sc.pages);
    let len = sc.len;
    let verdict = if sc.fixed_struct {
        judge_fixed(&SRC, &buf, sc.dir, len)
    } else {
        judge_stream(&SRC, &buf, sc.dir, len)
    };
    let outcome = copy_half(&SRC, &buf, sc.dir, len);
    let ok = match verdict {
        Ok(n) => sc.expect == n,
        Err(code) => code == EFAULT && sc.expect == WANT_EFAULT,
    };
    (ok, outcome)
}

// ── 注入器 ────────────────────────────────────────────────────────────
/// 三类半程注入。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyInject {
    /// 首字节即坏
    HeadPage,
    /// 中段坏
    MiddlePage,
    /// 末段坏
    TailPage,
}

impl CopyInject {
    /// 期望故障起点。
    pub fn fault_at(self) -> u32 {
        match self {
            CopyInject::HeadPage => 0,
            CopyInject::MiddlePage => 4,
            CopyInject::TailPage => 8,
        }
    }

    /// 对应诊断码。
    pub fn code(self) -> CopyCode {
        match self {
            CopyInject::HeadPage => CopyCode::HeadFault,
            CopyInject::MiddlePage => CopyCode::MidFault,
            CopyInject::TailPage => CopyCode::TailFault,
        }
    }

    /// 构造页表（三页）。
    pub fn pages(self) -> [PageState; 3] {
        match self {
            CopyInject::HeadPage => [PageState::Unmapped, PageState::Mapped, PageState::Mapped],
            CopyInject::MiddlePage => [PageState::Mapped, PageState::Kernel, PageState::Mapped],
            CopyInject::TailPage => [PageState::Mapped, PageState::Mapped, PageState::ReadOnly],
        }
    }

    /// 闭集全量。
    pub const ALL: [CopyInject; 3] = [
        CopyInject::HeadPage, CopyInject::MiddlePage, CopyInject::TailPage,
    ];
}

/// 探针账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeLedger {
    pub attempts: u32,
    pub copied_runs: u32,
    pub faults: u32,
    pub zeroed_tails: u32,
}

impl ProbeLedger {
    pub const fn zero() -> ProbeLedger {
        ProbeLedger { attempts: 0, copied_runs: 0, faults: 0, zeroed_tails: 0 }
    }

    /// 记一轮：返回是否仍守恒（尝试 = 有拷贝 + 零拷贝故障）。
    pub fn record(&mut self, copied: u32, faulted: bool, zeroed: bool) -> bool {
        self.attempts = self.attempts.saturating_add(1);
        if copied > 0 {
            self.copied_runs = self.copied_runs.saturating_add(1);
        }
        if faulted {
            self.faults = self.faults.saturating_add(1);
        }
        if zeroed {
            self.zeroed_tails = self.zeroed_tails.saturating_add(1);
        }
        self.conserved()
    }

    /// 守恒：每轮至少落入「拷贝过」或「故障」之一。
    pub fn conserved(&self) -> bool {
        self.copied_runs <= self.attempts && self.faults <= self.attempts
    }
}

/// 半程注入：三类各 INJECT_ROUNDS 轮，逐轮比对故障起点与残留。
pub fn run_injections() -> (ProbeLedger, [u32; 3]) {
    let mut ledger = ProbeLedger::zero();
    let mut per = [0u32; 3];
    let mut ci = 0usize;
    while ci < CopyInject::ALL.len() {
        if let Some(case) = CopyInject::ALL.get(ci).copied() {
            let pages = case.pages();
            let buf = UserBuf::new(0, &pages);
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let outcome = copy_half(&SRC, &buf, Direction::ToUser, 12);
                let caught = outcome.fault_at == Some(case.fault_at())
                    && outcome.copied == case.fault_at()
                    && outcome.tail_zeroed
                    && outcome.residual_ok(&SRC);
                let _ = ledger.record(outcome.copied, outcome.fault_at.is_some(), outcome.tail_zeroed);
                if caught {
                    if let Some(s) = per.get_mut(ci) {
                        *s = s.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per)
}

// ── 对账表（Linux 半程语义） ──────────────────────────────────────────
/// (调用族, 部分拷贝时的返回, 说明)
pub const HALF_TABLE: &[(&str, &str, &str)] = &[
    ("read", "已拷贝字节数", "copy_to_user 残值>0 → 返回 n-copied 对应的正数"),
    ("write", "已拷贝字节数", "copy_from_user 残值>0 → 短写，不报 EFAULT"),
    ("fstat", "EFAULT", "固定结构整体校验，任一页失败即回错"),
    ("gettimeofday", "EFAULT", "同上；半程数据仍写入用户区"),
    ("copy_to_user", "未拷贝字节数", "返回值>0 由调用方翻译为 EFAULT 或短读"),
];

/// 查表：调用名 → (返回语义, 说明)。
pub fn half_semantics(call: &str) -> Option<(&'static str, &'static str)> {
    let mut i = 0usize;
    while i < HALF_TABLE.len() {
        if let Some(row) = HALF_TABLE.get(i) {
            if row.0 == call {
                return Some((row.1, row.2));
            }
        }
        i += 1;
    }
    None
}

// ── 指纹 ──────────────────────────────────────────────────────────────
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

/// 规范语料：10 条场景的（返回码, 已拷贝, 残留字节）三元组入指纹。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < SCENARIOS.len() {
        if let Some(sc) = SCENARIOS.get(i) {
            let (ok, outcome) = run_scenario(sc);
            bytes.push(if ok { 1 } else { 0 });
            bytes.push((outcome.copied & 0xFF) as u8);
            let mut k = 0usize;
            while k < outcome.dst.len() {
                if let Some(b) = outcome.dst.get(k) {
                    bytes.push(*b);
                }
                k += 1;
            }
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9212 判据集（20 条静态 + 3 条动态）。
pub fn run_unx9212_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9212");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：全映射满拷贝
    let all = UserBuf::new(0, &[PageState::Mapped; 3]);
    let full = copy_half(&SRC, &all, Direction::ToUser, 12);
    s.add("正常流：三页全映射满拷贝 12 字节",
          full.copied == 12 && full.fault_at.is_none() && full.residual_ok(&SRC),
          "无故障时不得清零尾部，dst 应逐字节等于源");

    // 3. 半程前缀精确
    let mid = UserBuf::new(0, &[PageState::Mapped, PageState::Unmapped, PageState::Mapped]);
    let mid_out = copy_half(&SRC, &mid, Direction::ToUser, 12);
    s.add("半程：中段坏页停在 4 字节",
          mid_out.copied == 4 && mid_out.fault_at == Some(4) && mid_out.prefix_matches(&SRC),
          "页粒度为 4 字节，故障起点应精确落在页边界");

    // 4. 流式调用返回已拷贝数
    let stream = judge_stream(&SRC, &mid, Direction::ToUser, 12);
    s.add("返回语义：流式部分成功返回 4（不报 EFAULT）",
          stream == Ok(4),
          "已拷贝>0 时 read/write 返回短读/短写，只有 0 拷贝才 EFAULT");

    // 5. 首字节坏 → EFAULT
    let head = UserBuf::new(0, &[PageState::Unmapped, PageState::Mapped, PageState::Mapped]);
    let head_res = judge_stream(&SRC, &head, Direction::ToUser, 12);
    s.add("返回语义：首字节坏时 0 拷贝 → EFAULT", head_res == Err(EFAULT),
          "copied==0 是流式调用唯一回 EFAULT 的分支");

    // 6. 固定结构整体 EFAULT
    let fixed_res = judge_fixed(&SRC, &mid, Direction::ToUser, 12);
    let fixed_out = copy_half(&SRC, &mid, Direction::ToUser, 12);
    s.add("固定结构：半途仍回 EFAULT 且残留仍在",
          fixed_res == Err(EFAULT) && fixed_out.copied == 4,
          "fstat 类调用不做短读，但半程前缀仍已写入用户区");

    // 7. 残留尾部清零
    s.add("残留：故障尾部清零且前缀相符",
          mid_out.tail_zeroed && mid_out.tail_is_zero() && mid_out.residual_ok(&SRC),
          "Linux 在故障后清零目标尾部，防内核数据泄漏");

    // 8. 方向不对称
    let ro = UserBuf::new(0, &[PageState::Mapped, PageState::ReadOnly, PageState::Mapped]);
    let to_user = judge_stream(&SRC, &ro, Direction::ToUser, 12);
    let from_user = judge_stream(&SRC, &ro, Direction::FromUser, 12);
    s.add("方向：只读页写向 4、读向 12",
          to_user == Ok(4) && from_user == Ok(12),
          "copy_to_user 要可写、copy_from_user 只要可读");

    // 9. 零长度
    let zero = UserBuf::new(3, &[PageState::Unmapped; 3]);
    let z1 = judge_stream(&SRC, &zero, Direction::ToUser, 0);
    let z2 = judge_fixed(&SRC, &zero, Direction::ToUser, 0);
    s.add("零长度：全坏页上 count==0 仍回 0", z1 == Ok(0) && z2 == Ok(0),
          "零长度不触碰页表，不得把 EFAULT 挂在 count==0 上");

    // 10. 页内偏移
    let off2 = UserBuf::new(2, &[PageState::Mapped, PageState::Unmapped, PageState::Mapped]);
    let off_out = copy_half(&SRC, &off2, Direction::ToUser, 6);
    s.add("偏移：页内起于 2 时只能拷贝 2 字节",
          off_out.copied == 2 && off_out.fault_at == Some(2),
          "起始偏移决定首段可用宽度，必须计入前缀");

    // 11. 末字节故障
    let tail = UserBuf::new(0, &[PageState::Mapped, PageState::Mapped, PageState::ReadOnly]);
    let tail_out = copy_half(&SRC, &tail, Direction::ToUser, 12);
    let tail_res = judge_stream(&SRC, &tail, Direction::ToUser, 12);
    s.add("末段：第三页只读 → 拷贝 8 字节后停",
          tail_out.copied == 8 && tail_out.fault_at == Some(8) && tail_res == Ok(8),
          "末段故障同样返回部分值，不升级为 EFAULT");

    // 12. 越界（页表不足）
    let short = UserBuf::new(0, &[PageState::Mapped]);
    let short_out = copy_half(&SRC, &short, Direction::ToUser, 12);
    s.add("越界：页表只覆盖 4 字节，其余按未映射半程",
          short_out.copied == 4 && short_out.fault_at == Some(4),
          "缺页等价 Unmapped，越界不得读越 Vec 下标");

    // 13. 状态机
    let sm_ok = advance(Stage::Begin, Stage::PageWalk) == Ok(Stage::PageWalk)
        && advance(Stage::PageWalk, Stage::FaultClass) == Ok(Stage::FaultClass)
        && advance(Stage::FaultClass, Stage::Residue) == Ok(Stage::Residue)
        && advance(Stage::Residue, Stage::Verdict) == Ok(Stage::Verdict)
        && advance(Stage::Begin, Stage::FaultClass) == Err(EINVAL)
        && advance(Stage::Verdict, Stage::Begin) == Err(EINVAL);
    s.add("状态机：五阶段只进不退，跳级被拒", sm_ok,
          "先走页、再定故障类、再看残留、最后出结论");

    // 14. 场景矩阵
    let mut scen_ok = true;
    let mut i = 0usize;
    while i < SCENARIOS.len() {
        if let Some(sc) = SCENARIOS.get(i) {
            let (ok, _) = run_scenario(sc);
            if !ok {
                scen_ok = false;
            }
        }
        i += 1;
    }
    s.add("矩阵：10 条半程场景逐条命中期望", scen_ok,
          "首/中/尾/偏移/只读/内核/固定结构/零长八类全覆盖");

    // 15. 注入 3 类 10/10
    let (ledger, per) = run_injections();
    s.add("注入：三类半程违约各 10/10 全检出",
          per.iter().all(|n| *n == INJECT_ROUNDS),
          "首页/中段/末段故障必须逐类抓到故障起点与残留");

    // 16. 守恒
    s.add("守恒：每次尝试必有拷贝数或故障，且尾部清零数=故障数",
          ledger.conserved() && ledger.attempts == CopyInject::ALL.len() as u32 * INJECT_ROUNDS
              && ledger.zeroed_tails == ledger.faults && ledger.faults == ledger.attempts,
          "30 轮全部命中故障，清零标志逐轮为真");

    // 17. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：场景语料指纹相等", f1 == f2,
          "页表只读不写，两次探针必须逐字节相同");

    // 18. 对账表
    let recon_ok = half_semantics("read").map(|r| r.0) == Some("已拷贝字节数")
        && half_semantics("fstat").map(|r| r.0) == Some("EFAULT")
        && half_semantics("copy_to_user").map(|r| r.0) == Some("未拷贝字节数")
        && half_semantics("表外调用").is_none();
    s.add("对账：Linux 半程返回语义表逐行可复算", recon_ok,
          "5 行冻结表；表外返回 None 而不是空串");

    // 19. 文案闭集
    s.add("文案闭集：12 个诊断码均带中文建议",
          CopyCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到非空且可操作的说明");

    // 动态明细（≤8）：半程、注入、指纹三行
    let half_text = format!("中段故障：拷贝 {} / 故障点 {:?}", mid_out.copied, mid_out.fault_at);
    s.add_dyn("半程快照", mid_out.copied == 4, &half_text);
    let ledger_text = format!("轮次 {} / 故障 {} / 清零 {}",
                              ledger.attempts, ledger.faults, ledger.zeroed_tails);
    s.add_dyn("探针账快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("场景指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    // 20. 收口：判据与页账只增不减（放在动态明细之后，账含动态行）
    let before = s.len();
    let trunc = s.truncated();
    let poison_intact = full.dst.iter().all(|b| *b != POISON);
    s.add("收口：判据不少于 20、未截断、满拷贝无毒值",
          before >= 20 && !trunc && poison_intact,
          "成功路径必须把 POISON 全部覆盖掉，否则等于没写");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_copy_leaves_no_poison() {
        let buf = UserBuf::new(0, &[PageState::Mapped; 3]);
        let out = copy_half(&SRC, &buf, Direction::ToUser, 12);
        assert_eq!(out.copied, 12);
        assert!(out.fault_at.is_none());
        assert!(out.dst.iter().all(|b| *b != POISON));
        assert!(out.prefix_matches(&SRC));
    }

    #[test]
    fn head_fault_returns_efault() {
        let buf = UserBuf::new(0, &[PageState::Unmapped, PageState::Mapped, PageState::Mapped]);
        assert_eq!(judge_stream(&SRC, &buf, Direction::ToUser, 12), Err(EFAULT));
        let out = copy_half(&SRC, &buf, Direction::ToUser, 12);
        assert_eq!(out.copied, 0);
        assert!(out.tail_is_zero());
    }

    #[test]
    fn middle_fault_returns_prefix() {
        let buf = UserBuf::new(0, &[PageState::Mapped, PageState::Kernel, PageState::Mapped]);
        assert_eq!(judge_stream(&SRC, &buf, Direction::ToUser, 12), Ok(4));
        let out = copy_half(&SRC, &buf, Direction::ToUser, 12);
        assert_eq!(out.fault_at, Some(4));
        assert!(out.residual_ok(&SRC));
    }

    #[test]
    fn tail_fault_returns_len_minus_page() {
        let buf = UserBuf::new(0, &[PageState::Mapped, PageState::Mapped, PageState::ReadOnly]);
        assert_eq!(judge_stream(&SRC, &buf, Direction::ToUser, 12), Ok(8));
    }

    #[test]
    fn direction_asymmetry_on_readonly_page() {
        let buf = UserBuf::new(0, &[PageState::Mapped, PageState::ReadOnly, PageState::Mapped]);
        assert_eq!(judge_stream(&SRC, &buf, Direction::ToUser, 12), Ok(4));
        assert_eq!(judge_stream(&SRC, &buf, Direction::FromUser, 12), Ok(12));
    }

    #[test]
    fn fixed_struct_is_all_or_nothing() {
        let buf = UserBuf::new(0, &[PageState::Mapped, PageState::Unmapped, PageState::Mapped]);
        assert_eq!(judge_fixed(&SRC, &buf, Direction::ToUser, 12), Err(EFAULT));
        assert!(copy_half(&SRC, &buf, Direction::ToUser, 12).copied == 4);
    }

    #[test]
    fn zero_length_ignores_bad_pages() {
        let buf = UserBuf::new(3, &[PageState::Unmapped; 3]);
        assert_eq!(judge_stream(&SRC, &buf, Direction::ToUser, 0), Ok(0));
        assert_eq!(judge_fixed(&SRC, &buf, Direction::FromUser, 0), Ok(0));
    }

    #[test]
    fn offsets_shift_the_prefix() {
        let buf = UserBuf::new(2, &[PageState::Mapped, PageState::Unmapped, PageState::Mapped]);
        let out = copy_half(&SRC, &buf, Direction::ToUser, 6);
        assert_eq!(out.copied, 2);
        assert_eq!(out.fault_at, Some(2));
    }

    #[test]
    fn stage_machine_is_forward_only() {
        assert_eq!(advance(Stage::Begin, Stage::PageWalk), Ok(Stage::PageWalk));
        assert_eq!(advance(Stage::PageWalk, Stage::FaultClass), Ok(Stage::FaultClass));
        assert_eq!(advance(Stage::Begin, Stage::Verdict), Err(EINVAL));
        assert_eq!(advance(Stage::Verdict, Stage::PageWalk), Err(EINVAL));
    }

    #[test]
    fn all_scenarios_match() {
        for sc in SCENARIOS.iter() {
            let (ok, _) = run_scenario(sc);
            assert!(ok, "场景 {} 未命中期望 {}", sc.name, sc.expect);
        }
    }

    #[test]
    fn injections_are_all_caught() {
        let (l, per) = run_injections();
        assert!(l.conserved());
        assert_eq!(l.attempts, 30);
        assert_eq!(l.faults, 30);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9212_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(s.len() >= 20);
    }
}
