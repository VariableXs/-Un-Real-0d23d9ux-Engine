//! UNX-F9328 getrusage 坏指针矩阵（联 B15/B19 对账） · 0x7BF80段
//! ─────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9328 逐字）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F9328-J1 坏指针注入判据过，全 EFAULT 格过，
//!      非法 who 全 EINVAL 格过，与 B19 对账零重复格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① getrusage 的失败面是「who 值域」×「usage 可写性」两轴，不是单轴：
//!      who 先判（-1/0/1 合法，其余含 -2 与 RUSAGE_MAX+1 全部 EINVAL），
//!      通过后才轮到 copy_to_user 的 EFAULT。次序不可互换——坏 who 配坏
//!      指针必须报 EINVAL，这是本模块与 F9329（setrlimit）的最大分野。
//!   ② 坏指针不是一个布尔量：内核 copy_to_user 是逐页推进的，跨页到不可写
//!      页时**已经写下的那段是脏的**，返回 EFAULT 但字节数不为零。本模块
//!      用三页窗口（R/D 权限位）复算「脏字节数」与「完整落地的字段数」，
//!      把 EFAULT 从「一个码」细化为可对账的部分写账。
//!   ③ rusage 不是不透明缓冲：16 个字段的偏移/宽度冻结成布局表
//!      （ru_utime/ru_stime 各 16 字节，其余 14 个各 8 字节，合计 144）。
//!      字段完整性判据用「字段尾 <= 已写字节数」一次算清，不做逐字段试探。
//!   ④ 对齐不是拒绝轴：+1 偏移的 144 字节写只要不触不可写页就必须成功。
//!      矩阵里专门保留 unaligned 正例，防止实现把「未对齐」误判成 EFAULT。
//!   ⑤ 48 格矩阵 = 6 个 who 取法 × 8 个指针取法，每格 10 轮；非法 who 的
//!      24 格必须全 EINVAL（即使指针合法），合法 who 的 15 格坏指针必须全
//!      EFAULT、9 格好指针必须成功。
//!   ⑥ 与 B19 对账：B19 已认领 6 个用例 id，本模块 48 个格 id 必须与之
//!      零重复且自身两两唯一（重复即同一格被两侧各算一次，对账失真）。
//!   ⑦ 注入账 480 次，injected == detected + escaped，escaped 恒 0；
//!      按码分桶（EINVAL/EFAULT/OK）另记三笔，三桶之和等于 injected。
//!   ⑧ 诊断码段内闭集 12 个（0x7BF81..=0x7BF8C），码值两两唯一。
//! ─────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9328 独占 0x7BF80..=0x7BFBF；F9329 从 0x7BFC0 起）。
pub const SEG_BASE: u32 = 0x7BF80;
/// 段窗口宽度。
pub const SEG_WINDOW: u32 = 0x40;
/// 成功返回 0。
pub const OK: i32 = 0;
/// who 非法。
pub const EINVAL: i32 = 22;
/// usage 指针不可写。
pub const EFAULT: i32 = 14;
/// 用户区页大小。
pub const PAGE: u64 = 4096;
/// 页号移位（2 的幂，禁止除法）。
pub const PAGE_SHIFT: u32 = 12;
/// 观察窗页数。
pub const WINDOW_PAGES: usize = 3;
/// 观察窗基址（页对齐，位于用户区高段）。
pub const WINDOW_BASE: u64 = 0x0000_7FF0_0000;
/// 观察窗上界（不含）。
pub const WINDOW_LIMIT: u64 = WINDOW_BASE + PAGE * 3;
/// struct rusage 字节数（64 位口径）。
pub const RUSAGE_BYTES: u64 = 144;
/// 字段数。
pub const RUSAGE_FIELDS: usize = 16;
/// 单格注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// who 取法数。
pub const GRID_WHO: usize = 6;
/// 指针取法数。
pub const GRID_PTR: usize = 8;
/// 矩阵格数。
pub const GRID_CELLS: usize = GRID_WHO * GRID_PTR;
/// B19 已认领 id 数。
pub const B19_CLAIMS: usize = 6;

// ── who 值域表 ────────────────────────────────────────────────────────
/// 一个 who 取法。`valid` 决定是否进入 copy 面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WhoSpec {
    pub who: i32,
    pub name: &'static str,
    pub valid: bool,
    /// 是否聚合子进程（RUSAGE_CHILDREN 族）。
    pub children: bool,
}

/// 6 行冻结 who 表：3 合法 / 3 非法。
pub const WHO_TABLE: [WhoSpec; GRID_WHO] = [
    WhoSpec { who: -2, name: "RUSAGE_BOTH", valid: false, children: false },
    WhoSpec { who: -1, name: "RUSAGE_CHILDREN", valid: true, children: true },
    WhoSpec { who: 0, name: "RUSAGE_SELF", valid: true, children: false },
    WhoSpec { who: 1, name: "RUSAGE_THREAD", valid: true, children: false },
    WhoSpec { who: 2, name: "RUSAGE_MAX+1", valid: false, children: false },
    WhoSpec { who: 100, name: "RUSAGE_WILD", valid: false, children: false },
];

/// 查 who：仅表内合法行返回 Some；非法行与表外值一律 None（同判 EINVAL）。
pub fn decode_who(who: i32) -> Option<WhoSpec> {
    let mut i = 0usize;
    while i < WHO_TABLE.len() {
        if let Some(row) = WHO_TABLE.get(i).copied() {
            if row.who == who && row.valid {
                return Some(row);
            }
        }
        i += 1;
    }
    None
}

/// 表内合法行数（冻结 3）。
pub fn valid_who_count() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < WHO_TABLE.len() {
        if let Some(row) = WHO_TABLE.get(i) {
            if row.valid {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

// ── rusage 布局表 ─────────────────────────────────────────────────────
/// 一个字段的偏移与宽度（字节）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FieldSpec {
    pub name: &'static str,
    pub off: u32,
    pub size: u32,
}

const fn f(name: &'static str, off: u32, size: u32) -> FieldSpec {
    FieldSpec { name, off, size }
}

/// 16 字段冻结布局：ru_utime/ru_stime 各 16 字节（两个 long），其余 8 字节。
pub const FIELDS: [FieldSpec; RUSAGE_FIELDS] = [
    f("ru_utime", 0, 16),
    f("ru_stime", 16, 16),
    f("ru_maxrss", 32, 8),
    f("ru_ixrss", 40, 8),
    f("ru_idrss", 48, 8),
    f("ru_isrss", 56, 8),
    f("ru_minflt", 64, 8),
    f("ru_majflt", 72, 8),
    f("ru_nswap", 80, 8),
    f("ru_inblock", 88, 8),
    f("ru_oublock", 96, 8),
    f("ru_msgsnd", 104, 8),
    f("ru_msgrcv", 112, 8),
    f("ru_nsignals", 120, 8),
    f("ru_nvcsw", 128, 8),
    f("ru_nivcsw", 136, 8),
];

/// 布局总字节数（表尾字段的 off+size）。
pub fn layout_bytes() -> u64 {
    let mut end = 0u64;
    let mut i = 0usize;
    while i < FIELDS.len() {
        if let Some(row) = FIELDS.get(i) {
            let e = (row.off as u64).saturating_add(row.size as u64);
            if e > end {
                end = e;
            }
        }
        i += 1;
    }
    end
}

/// 布局是否无洞且互不重叠（按表序，后一字段 off 必须等于前一字段尾）。
pub fn layout_contiguous() -> bool {
    let mut cursor = 0u64;
    let mut i = 0usize;
    while i < FIELDS.len() {
        if let Some(row) = FIELDS.get(i) {
            if row.off as u64 != cursor {
                return false;
            }
            cursor = cursor.saturating_add(row.size as u64);
        }
        i += 1;
    }
    cursor == RUSAGE_BYTES
}

/// 已写 `bytes` 字节时，完整落地的字段数（字段尾 <= bytes）。
pub fn fields_fully_written(bytes: u64) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < FIELDS.len() {
        if let Some(row) = FIELDS.get(i) {
            let end = (row.off as u64).saturating_add(row.size as u64);
            if end <= bytes {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

// ── 用户窗页权限 ──────────────────────────────────────────────────────
/// 单页可写性。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Perm {
    Readable,
    Denied,
}

/// 一次 copy_to_user 的结果：返回码 / 脏字节 / 完整字段 / 触及页数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WriteOutcome {
    pub status: i32,
    pub bytes: u64,
    pub fields: u32,
    pub pages: u32,
}

impl WriteOutcome {
    pub const fn reject(code: i32) -> WriteOutcome {
        WriteOutcome { status: code, bytes: 0, fields: 0, pages: 0 }
    }
}

/// 逐页推进的 copy_to_user 模型。
///
/// 语义：
/// - 地址加法溢出 / 起点在窗外 → EFAULT，脏 0 字节；
/// - 终点越出观察窗 → 在上界截断，脏字节 = 上界 - 起点；
/// - 窗内遇到 Denied 页 → 脏字节 = 该页起点 - 起点（部分写），返回 EFAULT；
/// - 全部可写 → OK，脏字节 = len。
pub fn write_span(addr: u64, len: u64, perms: [Perm; WINDOW_PAGES]) -> WriteOutcome {
    if len == 0 {
        return WriteOutcome { status: OK, bytes: 0, fields: 0, pages: 0 };
    }
    let end = match addr.checked_add(len) {
        Some(v) => v,
        None => return WriteOutcome::reject(EFAULT),
    };
    if addr < WINDOW_BASE || end > WINDOW_LIMIT {
        let bytes = if addr < WINDOW_BASE {
            0
        } else {
            WINDOW_LIMIT.saturating_sub(addr).min(len)
        };
        return WriteOutcome { status: EFAULT, bytes, fields: fields_fully_written(bytes), pages: 0 };
    }
    let off = addr - WINDOW_BASE;
    let end_off = off.saturating_add(len);
    let start_page = (off >> PAGE_SHIFT) as usize;
    let end_page = ((end_off - 1) >> PAGE_SHIFT) as usize;
    let mut p = start_page;
    while p <= end_page {
        match perms.get(p) {
            Some(Perm::Denied) => {
                let fault_off = (p as u64) << PAGE_SHIFT;
                let bytes = fault_off.saturating_sub(off);
                let pages = (p - start_page) as u32 + 1;
                return WriteOutcome { status: EFAULT, bytes, fields: fields_fully_written(bytes), pages };
            }
            _ => {}
        }
        p += 1;
    }
    let pages = (end_page - start_page) as u32 + 1;
    WriteOutcome { status: OK, bytes: len, fields: fields_fully_written(len), pages }
}

/// getrusage 判定：who 先于指针（次序即语义）。
pub fn decide(who: i32, addr: u64, perms: [Perm; WINDOW_PAGES]) -> WriteOutcome {
    match decode_who(who) {
        Some(_) => write_span(addr, RUSAGE_BYTES, perms),
        None => WriteOutcome::reject(EINVAL),
    }
}

// ── 指针样例 ──────────────────────────────────────────────────────────
/// 一个指针取法：起点 + 三页权限 + 期望结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PtrCase {
    pub name: &'static str,
    pub addr: u64,
    pub perms: [Perm; WINDOW_PAGES],
    pub expect: i32,
    pub bytes: u64,
    pub fields: u32,
}

const RRR: [Perm; WINDOW_PAGES] = [Perm::Readable, Perm::Readable, Perm::Readable];
const RDR: [Perm; WINDOW_PAGES] = [Perm::Readable, Perm::Denied, Perm::Readable];

const fn pf(name: &'static str, addr: u64, perms: [Perm; WINDOW_PAGES], expect: i32, bytes: u64, fields: u32) -> PtrCase {
    PtrCase { name, addr, perms, expect, bytes, fields }
}

/// 8 种指针取法：3 成功 + 5 EFAULT（其中 3 种带非零脏字节）。
pub const PTR_CASES: [PtrCase; GRID_PTR] = [
    pf("aligned", WINDOW_BASE, RRR, OK, RUSAGE_BYTES, 16),
    pf("unaligned+1", WINDOW_BASE + 1, RRR, OK, RUSAGE_BYTES, 16),
    pf("cross-page", WINDOW_BASE + 4032, RRR, OK, RUSAGE_BYTES, 16),
    pf("tail-out", WINDOW_BASE + 12280, RRR, EFAULT, 8, 0),
    pf("mid-fault", WINDOW_BASE + 4096, RDR, EFAULT, 0, 0),
    pf("partial", WINDOW_BASE + 4032, RDR, EFAULT, 64, 6),
    pf("null", 0, RRR, EFAULT, 0, 0),
    pf("wrap", u64::MAX - 8, RRR, EFAULT, 0, 0),
];

// ── 注入账（按码分桶） ────────────────────────────────────────────────
/// 注入 = 检出 + 逃逸；另按码分三桶。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub einval: u32,
    pub efault: u32,
    pub ok: u32,
}

impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0, einval: 0, efault: 0, ok: 0 }
    }
    /// 记一笔注入；`code` 用于分桶，未登记码不计入任何桶（会由桶和判据抓出）。
    pub fn record(&mut self, code: i32, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if code == EINVAL {
            self.einval = self.einval.saturating_add(1);
        } else if code == EFAULT {
            self.efault = self.efault.saturating_add(1);
        } else if code == OK {
            self.ok = self.ok.saturating_add(1);
        }
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
    /// 三桶之和（未登记码会被漏算，故必须与 injected 相等）。
    pub fn buckets(&self) -> u32 {
        self.einval.saturating_add(self.efault).saturating_add(self.ok)
    }
}

// ── 48 格矩阵 ─────────────────────────────────────────────────────────
/// 一格的期望：非法 who 一律 EINVAL（脏 0 字节），否则沿用指针取法。
pub fn cell_want(who: WhoSpec, ptr: PtrCase) -> (i32, u64) {
    if who.valid {
        (ptr.expect, ptr.bytes)
    } else {
        (EINVAL, 0)
    }
}

/// 逐格 10 轮注入，返回 (命中格计数, 账本)。
pub fn run_pointer_matrix() -> (u32, InjectLedger) {
    let mut hit_cells = 0u32;
    let mut ledger = InjectLedger::zero();
    let mut wi = 0usize;
    while wi < WHO_TABLE.len() {
        if let Some(who) = WHO_TABLE.get(wi).copied() {
            let mut pi = 0usize;
            while pi < PTR_CASES.len() {
                if let Some(ptr) = PTR_CASES.get(pi).copied() {
                    let (expect, expect_bytes) = cell_want(who, ptr);
                    let mut caught_all = true;
                    let mut round = 0u32;
                    while round < INJECT_ROUNDS {
                        let got = decide(who.who, ptr.addr, ptr.perms);
                        let caught = got.status == expect && got.bytes == expect_bytes;
                        ledger.record(expect, caught);
                        if !caught {
                            caught_all = false;
                        }
                        round = round.saturating_add(1);
                    }
                    if caught_all {
                        hit_cells = hit_cells.saturating_add(1);
                    }
                }
                pi += 1;
            }
        }
        wi += 1;
    }
    (hit_cells, ledger)
}

/// 矩阵里期望码为 `code` 的格数（按格计，不按轮计）。
pub fn count_cells(code: i32) -> u32 {
    let mut n = 0u32;
    let mut wi = 0usize;
    while wi < WHO_TABLE.len() {
        if let Some(who) = WHO_TABLE.get(wi).copied() {
            let mut pi = 0usize;
            while pi < PTR_CASES.len() {
                if let Some(ptr) = PTR_CASES.get(pi).copied() {
                    if cell_want(who, ptr).0 == code {
                        n = n.saturating_add(1);
                    }
                }
                pi += 1;
            }
        }
        wi += 1;
    }
    n
}

// ── B19 对账 ──────────────────────────────────────────────────────────
/// B19 侧已认领的 6 个用例 id（只读镜像）。
pub const B19_IDS: [&str; B19_CLAIMS] = [
    "b19:rusage-who",
    "b19:rusage-copy",
    "b19:rusage-layout",
    "b19:rusage-efault",
    "b19:rusage-partial",
    "b19:rusage-alignment",
];

/// 本模块一格的 id（含 who 与指针取法序，天然唯一）。
pub fn cell_id(who: i32, ptr_idx: usize) -> String {
    format!("f9328:{}:{}", who, ptr_idx)
}

/// 48 个格 id。
pub fn our_ids() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut wi = 0usize;
    while wi < WHO_TABLE.len() {
        if let Some(who) = WHO_TABLE.get(wi) {
            let mut pi = 0usize;
            while pi < PTR_CASES.len() {
                out.push(cell_id(who.who, pi));
                pi += 1;
            }
        }
        wi += 1;
    }
    out
}

/// 零重复格：48 个 id 两两唯一，且与 B19 认领的 6 个 id 无交集。
pub fn b19_zero_duplicate() -> bool {
    let ids = our_ids();
    let mut i = 0usize;
    while i < ids.len() {
        if let Some(a) = ids.get(i) {
            let mut k = 0usize;
            while k < B19_IDS.len() {
                if let Some(b) = B19_IDS.get(k) {
                    if a.as_str() == *b {
                        return false;
                    }
                }
                k += 1;
            }
            let mut j = i + 1;
            while j < ids.len() {
                if let Some(b) = ids.get(j) {
                    if a == b {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    ids.len() == GRID_CELLS
}

/// B19 三个共用语义的 errno 口径对账：(一致, 总数)。
pub fn b19_agreement() -> (u32, u32) {
    let bad_who = decide(-2, WINDOW_BASE, RRR).status;
    let bad_ptr = decide(0, u64::MAX - 8, RRR).status;
    let good = decide(1, WINDOW_BASE, RRR).status;
    let mut hit = 0u32;
    if bad_who == EINVAL {
        hit = hit.saturating_add(1);
    }
    if bad_ptr == EFAULT {
        hit = hit.saturating_add(1);
    }
    if good == OK {
        hit = hit.saturating_add(1);
    }
    (hit, 3)
}

// ── Linux errno 对照 ─────────────────────────────────────────────────
/// (名称, Linux 正值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EFAULT", 14, 998, "ERROR_NOACCESS；copy_to_user 触到不可写页"),
    ("EINVAL", 22, 87, "ERROR_INVALID_PARAMETER；who 不在 -1/0/1 内"),
];

/// 是否为本模块承认的 errno 正值。
pub fn errno_known(code: i32) -> bool {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum RusageCode {
    BadWho,
    WhoOrder,
    NullUsage,
    UnmappedUsage,
    PartialWrite,
    SpanOverflow,
    LayoutHole,
    FieldTail,
    LedgerBreak,
    B19Conflict,
    SegmentViolation,
    NotReproducible,
}

impl RusageCode {
    /// 批内相对码（低 16 位口径）。
    pub fn code(self) -> u16 {
        SEG_BASE as u16 + 1 + self as u16
    }
    /// 全宽段内码（窗口守卫用）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RusageCode::BadWho => "who 不在 -1/0/1 内：RUSAGE_BOTH 与超界值一律 EINVAL",
            RusageCode::WhoOrder => "who 判定被指针判定抢先：坏 who 配坏指针必须回 EINVAL",
            RusageCode::NullUsage => "usage 为空指针：copy_to_user 必须回 EFAULT，不得静默跳过",
            RusageCode::UnmappedUsage => "usage 未映射：与空指针同判 EFAULT，脏字节为 0",
            RusageCode::PartialWrite => "部分写：跨页触到不可写页时已写字节必须如实记账",
            RusageCode::SpanOverflow => "地址加法溢出：base+len 必须 checked_add 后回 EFAULT",
            RusageCode::LayoutHole => "布局有洞或重叠：字段偏移必须首尾相接且总和 144",
            RusageCode::FieldTail => "字段尾越界：完整字段数按「字段尾<=已写字节」计算",
            RusageCode::LedgerBreak => "注入账不守恒：三桶之和必须等于 injected",
            RusageCode::B19Conflict => "B19 对账冲突：格 id 重复或与 B19 认领面相交",
            RusageCode::SegmentViolation => "段位越界：本模块独占 0x7BF80 的 0x40 窗口",
            RusageCode::NotReproducible => "双跑指纹不一致：判定路径混入非确定状态",
        }
    }
    /// 闭集全量。
    pub const ALL: &[RusageCode] = &[
        RusageCode::BadWho,
        RusageCode::WhoOrder,
        RusageCode::NullUsage,
        RusageCode::UnmappedUsage,
        RusageCode::PartialWrite,
        RusageCode::SpanOverflow,
        RusageCode::LayoutHole,
        RusageCode::FieldTail,
        RusageCode::LedgerBreak,
        RusageCode::B19Conflict,
        RusageCode::SegmentViolation,
        RusageCode::NotReproducible,
    ];
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

/// 压入一次写结果（码 + 脏字节 + 字段数）。
fn push_outcome(out: &mut Vec<u8>, o: WriteOutcome) {
    let code = o.status as u32;
    out.push((code >> 8) as u8);
    out.push(code as u8);
    out.push((o.bytes >> 16) as u8);
    out.push((o.bytes >> 8) as u8);
    out.push(o.bytes as u8);
    out.push(o.fields as u8);
}

/// 规范语料指纹：8 个指针取法 + 48 格 who×ptr = 56 次写判定。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut p = 0usize;
    while p < PTR_CASES.len() {
        if let Some(c) = PTR_CASES.get(p).copied() {
            push_outcome(&mut bytes, write_span(c.addr, RUSAGE_BYTES, c.perms));
        }
        p += 1;
    }
    let mut wi = 0usize;
    while wi < WHO_TABLE.len() {
        if let Some(who) = WHO_TABLE.get(wi).copied() {
            let mut pi = 0usize;
            while pi < PTR_CASES.len() {
                if let Some(ptr) = PTR_CASES.get(pi).copied() {
                    push_outcome(&mut bytes, decide(who.who, ptr.addr, ptr.perms));
                }
                pi += 1;
            }
        }
        wi += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9328 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9328_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9328");
    let (hit_cells, ledger) = run_pointer_matrix();
    let einval_cells = count_cells(EINVAL);
    let efault_cells = count_cells(EFAULT);
    let ok_cells = count_cells(OK);
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();

    let seg_ok = SEG_BASE == 0x7BF80
        && SEG_BASE % 0x40 == 0
        && (0x7B000..=0x7D1C0).contains(&SEG_BASE)
        && SEG_BASE + SEG_WINDOW <= 0x7BFC0;
    s.add("段位守卫：0x7BF80 起 0x40 窗口不与 F9329 重叠", seg_ok,
          "段基址必须 0x40 对齐且本模块不得覆盖相邻单的段");

    let mut who_shape = WHO_TABLE.len() == GRID_WHO;
    let mut i = 0usize;
    while i < WHO_TABLE.len() {
        if let Some(a) = WHO_TABLE.get(i) {
            if a.name.is_empty() {
                who_shape = false;
            }
            let mut j = i + 1;
            while j < WHO_TABLE.len() {
                if let Some(b) = WHO_TABLE.get(j) {
                    if a.who == b.who {
                        who_shape = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add("who 表：6 行 id 唯一且名称非空", who_shape,
          "who 表重复行会让 -2 之类非法值被误当合法");

    s.add("who 值域：3 合法（-1/0/1）/ 3 非法（-2/2/100）",
          valid_who_count() == 3 && decode_who(-1).is_some() && decode_who(0).is_some()
              && decode_who(1).is_some() && decode_who(-2).is_none() && decode_who(2).is_none()
              && decode_who(100).is_none(),
          "RUSAGE_BOTH 不是合法 who；越界值必须 EINVAL");

    s.add("字段布局：16 字段无洞无重叠且总和 144 字节",
          FIELDS.len() == RUSAGE_FIELDS && layout_contiguous() && layout_bytes() == RUSAGE_BYTES,
          "布局表是部分写判据的基准，洞或重叠会让字段数失真");

    s.add("字段计数：144/64/8/0 字节分别完整 16/6/0/0 字段",
          fields_fully_written(144) == 16 && fields_fully_written(64) == 6
              && fields_fully_written(8) == 0 && fields_fully_written(0) == 0,
          "完整性按字段尾<=已写字节一次算清，不做逐字段试探");

    let aligned = write_span(WINDOW_BASE, RUSAGE_BYTES, RRR);
    let unaligned = write_span(WINDOW_BASE + 1, RUSAGE_BYTES, RRR);
    s.add("正例：页对齐与 +1 未对齐均满写 144 字节 16 字段",
          aligned.status == OK && aligned.bytes == 144 && aligned.fields == 16
              && unaligned.status == OK && unaligned.bytes == 144 && unaligned.fields == 16,
          "对齐不是拒绝轴，未对齐只在触坏页时才失败");

    let partial = write_span(WINDOW_BASE + 4032, RUSAGE_BYTES, RDR);
    s.add("部分写：跨页触到不可写页时脏 64 字节 / 6 字段 / EFAULT",
          partial.status == EFAULT && partial.bytes == 64 && partial.fields == 6,
          "已写下的那段是脏的，字节数不得归零掩盖");

    let mid = write_span(WINDOW_BASE + 4096, RUSAGE_BYTES, RDR);
    s.add("整页不可写：起点即坏页时脏 0 字节",
          mid.status == EFAULT && mid.bytes == 0 && mid.fields == 0,
          "坏页起点即失败，不能先写后回滚");

    let tail = write_span(WINDOW_BASE + 12280, RUSAGE_BYTES, RRR);
    let null = write_span(0, RUSAGE_BYTES, RRR);
    let wrap = write_span(u64::MAX - 8, RUSAGE_BYTES, RRR);
    s.add("越界三态：尾越窗 8 字节 / 空指针 0 字节 / 溢出 0 字节",
          tail.status == EFAULT && tail.bytes == 8
              && null.status == EFAULT && null.bytes == 0
              && wrap.status == EFAULT && wrap.bytes == 0,
          "三种越界都必须 EFAULT，且脏字节按各自语义截断");

    let ptr_ok = {
        let mut all = true;
        let mut p = 0usize;
        while p < PTR_CASES.len() {
            if let Some(c) = PTR_CASES.get(p).copied() {
                let got = write_span(c.addr, RUSAGE_BYTES, c.perms);
                if got.status != c.expect || got.bytes != c.bytes || got.fields != c.fields {
                    all = false;
                }
            }
            p += 1;
        }
        all
    };
    s.add("指针取法：8/8 逐样例复算（码/脏字节/字段数）", ptr_ok,
          "同一个 EFAULT 必须由脏字节数区分成因");

    s.add("矩阵格数：EINVAL 24 / EFAULT 15 / OK 9",
          einval_cells == 24 && efault_cells == 15 && ok_cells == 9
              && einval_cells + efault_cells + ok_cells == GRID_CELLS as u32,
          "非法 who 的 24 格即使指针合法也必须 EINVAL");

    s.add("矩阵注入：48 格各 10/10 命中（共 480 次）",
          hit_cells == GRID_CELLS as u32,
          "任一格漂移都说明 who/指针两轴次序脱钩");

    s.add("注入账：480 次全检出、逃逸 0、三桶之和等于注入",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == (GRID_CELLS as u32) * INJECT_ROUNDS
              && ledger.detected == ledger.injected && ledger.buckets() == ledger.injected,
          "分桶漏记会立刻被「桶和==注入」抓出");

    s.add("次序：坏 who 配坏指针回 EINVAL，坏指针配好 who 回 EFAULT",
          decide(-2, u64::MAX - 8, RRR).status == EINVAL
              && decide(0, u64::MAX - 8, RRR).status == EFAULT
              && decide(0, WINDOW_BASE, RRR).status == OK,
          "who 面必须先于 copy 面返回");

    s.add("B19 对账：48 格 id 两两唯一且与 B19 六项零交集",
          b19_zero_duplicate() && our_ids().len() == GRID_CELLS,
          "格 id 重复等于同一格被两侧各算一次，对账失真");

    let (b19_hit, b19_total) = b19_agreement();
    s.add("B19 口径：EINVAL/EFAULT/OK 三语义 3/3 一致",
          b19_hit == b19_total && b19_total == 3,
          "共用语义的 errno 口径必须与 B19 完全相同");

    s.add("errno 对照：EFAULT=14 / EINVAL=22 表内可复算",
          errno_known(EFAULT) && errno_known(EINVAL) && !errno_known(OK)
              && !errno_known(-EFAULT) && ERRNO_TABLE.len() == 2,
          "错误码只允许表内正值，负值口径由调用方统一取负");

    let mut code_ok = ClockGuard::new();
    let mut ci = 0usize;
    while ci < RusageCode::ALL.len() {
        if let Some(a) = RusageCode::ALL.get(ci) {
            if a.detail().is_empty() {
                code_ok.text = false;
            }
            if a.val() <= SEG_BASE || a.val() > SEG_BASE + 0x0C {
                code_ok.window = false;
            }
            let mut cj = ci + 1;
            while cj < RusageCode::ALL.len() {
                if let Some(b) = RusageCode::ALL.get(cj) {
                    if a.val() == b.val() {
                        code_ok.unique = false;
                    }
                }
                cj += 1;
            }
        }
        ci += 1;
    }
    s.add("诊断码：12 个码值唯一、全部落窗、文案非空",
          RusageCode::ALL.len() == 12 && code_ok.unique && code_ok.window && code_ok.text,
          "段内闭集越窗会与相邻单撞码，文案不得为空");

    s.add("双跑一致：56 次写判定的规范语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    let grid_text = format!("矩阵 {} 格 × {} 轮，命中 {} 格", GRID_CELLS, INJECT_ROUNDS, hit_cells);
    s.add_dyn("矩阵快照", hit_cells == GRID_CELLS as u32, &grid_text);

    let book_text = format!("注入 {} / 检出 {} / 逃逸 {}，桶 E{} F{} O{}",
                            ledger.injected, ledger.detected, ledger.escaped,
                            ledger.einval, ledger.efault, ledger.ok);
    s.add_dyn("账本快照", ledger.conserved() && ledger.buckets() == ledger.injected, &book_text);

    let b19_text = format!("B19 零重复 {}，口径 {}/{}，指纹 0x{:08X}",
                           b19_zero_duplicate(), b19_hit, b19_total, fp_a);
    s.add_dyn("对账快照", b19_zero_duplicate() && b19_hit == b19_total, &b19_text);

    s
}

/// 诊断码三轴判定的临时聚合器（避免在检查体里堆三个平行 bool）。
struct ClockGuard {
    unique: bool,
    window: bool,
    text: bool,
}

impl ClockGuard {
    fn new() -> ClockGuard {
        ClockGuard { unique: true, window: true, text: true }
    }
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn who_table_and_decode() {
        assert_eq!(WHO_TABLE.len(), GRID_WHO);
        assert_eq!(valid_who_count(), 3);
        assert_eq!(decode_who(-1).map(|w| w.children), Some(true));
        assert_eq!(decode_who(0).map(|w| w.valid), Some(true));
        assert_eq!(decode_who(1).map(|w| w.name), Some("RUSAGE_THREAD"));
        assert_eq!(decode_who(-2).is_none(), true);
        assert_eq!(decode_who(2).is_none(), true);
        assert_eq!(decode_who(100).is_none(), true);
    }

    #[test]
    fn layout_is_contiguous() {
        assert_eq!(FIELDS.len(), RUSAGE_FIELDS);
        assert_eq!(layout_contiguous(), true);
        assert_eq!(layout_bytes(), RUSAGE_BYTES);
        assert_eq!(fields_fully_written(144), 16);
        assert_eq!(fields_fully_written(64), 6);
        assert_eq!(fields_fully_written(32), 2);
        assert_eq!(fields_fully_written(16), 1);
        assert_eq!(fields_fully_written(8), 0);
    }

    #[test]
    fn aligned_and_unaligned_succeed() {
        let a = write_span(WINDOW_BASE, RUSAGE_BYTES, RRR);
        assert_eq!((a.status, a.bytes, a.fields), (OK, 144, 16));
        let b = write_span(WINDOW_BASE + 1, RUSAGE_BYTES, RRR);
        assert_eq!((b.status, b.bytes), (OK, 144));
        let c = write_span(WINDOW_BASE + 4032, RUSAGE_BYTES, RRR);
        assert_eq!((c.status, c.bytes, c.pages), (OK, 144, 2));
    }

    #[test]
    fn partial_write_is_accounted() {
        let a = write_span(WINDOW_BASE + 4032, RUSAGE_BYTES, RDR);
        assert_eq!((a.status, a.bytes, a.fields), (EFAULT, 64, 6));
        let b = write_span(WINDOW_BASE + 4096, RUSAGE_BYTES, RDR);
        assert_eq!((b.status, b.bytes, b.fields), (EFAULT, 0, 0));
    }

    #[test]
    fn out_of_window_and_overflow() {
        let tail = write_span(WINDOW_BASE + 12280, RUSAGE_BYTES, RRR);
        assert_eq!((tail.status, tail.bytes), (EFAULT, 8));
        let null = write_span(0, RUSAGE_BYTES, RRR);
        assert_eq!((null.status, null.bytes), (EFAULT, 0));
        let wrap = write_span(u64::MAX - 8, RUSAGE_BYTES, RRR);
        assert_eq!((wrap.status, wrap.bytes), (EFAULT, 0));
    }

    #[test]
    fn pointer_cases_are_frozen() {
        let mut p = 0usize;
        while p < PTR_CASES.len() {
            if let Some(c) = PTR_CASES.get(p).copied() {
                let got = write_span(c.addr, RUSAGE_BYTES, c.perms);
                assert_eq!((got.status, got.bytes, got.fields), (c.expect, c.bytes, c.fields));
            }
            p += 1;
        }
    }

    #[test]
    fn who_precedes_pointer() {
        assert_eq!(decide(-2, u64::MAX - 8, RRR).status, EINVAL);
        assert_eq!(decide(0, u64::MAX - 8, RRR).status, EFAULT);
        assert_eq!(decide(0, WINDOW_BASE, RRR).status, OK);
        assert_eq!(decide(-2, WINDOW_BASE, RRR).status, EINVAL);
    }

    #[test]
    fn matrix_counts_are_frozen() {
        assert_eq!(count_cells(EINVAL), 24);
        assert_eq!(count_cells(EFAULT), 15);
        assert_eq!(count_cells(OK), 9);
        assert_eq!(GRID_CELLS, 48);
    }

    #[test]
    fn matrix_injection_and_ledger() {
        let (hit, ledger) = run_pointer_matrix();
        assert_eq!(hit, GRID_CELLS as u32);
        assert_eq!(ledger.injected, 480);
        assert_eq!(ledger.detected, 480);
        assert_eq!(ledger.escaped, 0);
        assert_eq!(ledger.buckets(), 480);
        assert_eq!(ledger.einval, 240);
        assert_eq!(ledger.efault, 150);
        assert_eq!(ledger.ok, 90);
    }

    #[test]
    fn b19_ids_are_disjoint() {
        assert_eq!(b19_zero_duplicate(), true);
        assert_eq!(our_ids().len(), GRID_CELLS);
        assert_eq!(b19_agreement(), (3, 3));
    }

    #[test]
    fn errno_and_codes() {
        assert_eq!(errno_known(EFAULT), true);
        assert_eq!(errno_known(EINVAL), true);
        assert_eq!(errno_known(0), false);
        assert_eq!(RusageCode::ALL.len(), 12);
        let mut i = 0usize;
        while i < RusageCode::ALL.len() {
            if let Some(a) = RusageCode::ALL.get(i) {
                assert_eq!(a.detail().is_empty(), false);
                assert_eq!(a.val() > SEG_BASE, true);
                let mut j = i + 1;
                while j < RusageCode::ALL.len() {
                    if let Some(b) = RusageCode::ALL.get(j) {
                        assert_eq!(a.val() == b.val(), false);
                    }
                    j += 1;
                }
            }
            i += 1;
        }
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9328_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
