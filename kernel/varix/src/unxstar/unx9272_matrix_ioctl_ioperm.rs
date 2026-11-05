//! UNX-F9272 ioctl ioperm/iopl 禁用格 EPERM 矩阵 · 0x7B180段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9272 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9272-J1 ioperm/iopl 注入判据过，
//!   全 EPERM 拒绝格过，与 B16 红线注记（禁用格）对账一致格过；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 三条门长得像但不能合并：ioperm 是「权限门 → 值域门」，iopl 是「值域门 →
//!      权限门」，ioctl 红线是「策略门，身份无关」。次序取自 Linux：sys_ioperm
//!      先 capable(CAP_SYS_RAWIO) 再查端口上界；do_iopl 先 level>3 回 EINVAL 再
//!      查能力。把三者折成一个通用门，超级用户会拿到本该是 EINVAL 的码，反之
//!      越权端口会拿到 EINVAL 而不是 EPERM。
//!   ② 禁用格与未知请求必须分码：禁用格是「认识但按策略拒绝」→ EPERM；未知
//!      请求是「不认识」→ ENOTTY。二者都拒但码不同，否则调用方无法区分「换
//!      一个请求号」还是「放弃这条路径」。
//!   ③ 红线高于特权：TIOCSTI 这类注入通道对 root 也 EPERM。安全锁定（lockdown）
//!      档进一步压掉 ioperm/iopl——红线是策略层，不由 euid 决定。
//!   ④ iopl(0) 与 ioperm(num=0) 是合法的降权/空范围：非特权身份调用 iopl(0)
//!      必须成功（它只会降低 I/O 特权），空范围 ioperm 不触碰任何端口也成功。
//!      把「无实际效果」误判成拒绝，会让正常收尾路径失败。
//!   ⑤ 端口上界用 checked_add 求 first+num，溢出（u32 回绕）同样归 EINVAL，
//!      不用裸加法；上界 0x10000 是 x86 IO 位图容量。
//!   ⑥ 与 B16 对账看「注记 ↔ 表」双读：每条注记的请求号在请求表里必须标为
//!      Redline 且判定出 EPERM，两侧任一处改动都会在漂移格报红。
//! ─────────────────────────────────────────────────────────────────────
use alloc::format;

use crate::checks::CheckSet;

/// 段基址：F9272 独占 0x40 窗口。
pub const SEG_BASE: u32 = 0x7B180;
pub const MATRIX_CELLS: usize = 18; // 三档 × 三种门
pub const EPERM_CELLS: usize = 9; // 期望 EPERM 的格数
pub const INJECT_ROUNDS: u32 = 10; // 单格复测轮数
pub const B16_NOTES: usize = 4; // B16 红线注记条数

pub const EPERM: i32 = 1; // 权限不足
pub const EINVAL: i32 = 22; // 值域非法
pub const ENOTTY: i32 = 25; // 未知请求
pub const IOPORT_LIMIT: u32 = 0x1_0000; // x86 IO 位图容量
pub const IOPL_MAX: u32 = 3; // iopl 合法上界

/// 常用 ioctl 请求号（x86_64 asm-generic）。
pub const TCGETS: u32 = 0x5401;
pub const TIOCSTI: u32 = 0x5412;
pub const TIOCGWINSZ: u32 = 0x5413;
pub const FIONREAD: u32 = 0x541B;
pub const TIOCSSERIAL: u32 = 0x541F;
pub const KDSETMODE: u32 = 0x4B3A;
pub const UNKNOWN_REQ: u32 = 0xDEAD;

/// B16 侧账本标识（联对账锚点）。
pub const B16_BOOK: &str = "UNX-C2-B16-ioctl-redline";

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x00..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IoCode {
    GateSwap,        // 门序互换（ioperm/iopl 弄反）
    RedlineLeak,     // 禁用格未报 EPERM
    UnknownAsDenied, // 未知请求被误报 EPERM
    RedlineAsNotty,  // 禁用格被误报 ENOTTY
    PortOverflow,    // 端口区间越界未报 EINVAL
    DowngradeDenied, // iopl(0) 被误拒
    EmptyRangeDenied,// 空范围 ioperm 被误拒
    PrivLeak,        // 无权身份通过了能力门
    B16Drift,        // B16 注记与请求表不一致
    StageRegress,    // 状态机回退
    LedgerLeak,      // 注入账漏检
    SegmentViolation,// 段位越界
}

impl IoCode {
    pub fn val(self) -> u32 {
        SEG_BASE + match self {
            IoCode::GateSwap => 0, IoCode::RedlineLeak => 1, IoCode::UnknownAsDenied => 2,
            IoCode::RedlineAsNotty => 3, IoCode::PortOverflow => 4, IoCode::DowngradeDenied => 5,
            IoCode::EmptyRangeDenied => 6, IoCode::PrivLeak => 7, IoCode::B16Drift => 8,
            IoCode::StageRegress => 9, IoCode::LedgerLeak => 10, IoCode::SegmentViolation => 11,
        }
    }
    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            IoCode::GateSwap => "门序互换：ioperm 权限先于值域，iopl 值域先于权限；互换后错误码指向错因",
            IoCode::RedlineLeak => "禁用格未拒：TIOCSTI/TIOCSSERIAL/KDSETMODE 属红线，任何身份都必须 EPERM",
            IoCode::UnknownAsDenied => "未知请求误报 EPERM：不认识应回 ENOTTY，否则调用方无法区分换号与弃路",
            IoCode::RedlineAsNotty => "禁用格误报 ENOTTY：已知但被策略拒绝，必须 EPERM 而不是「不认识」",
            IoCode::PortOverflow => "端口区间越界：first+num 超过 0x10000 或溢出，必须 EINVAL；请用 checked_add 复算",
            IoCode::DowngradeDenied => "iopl(0) 被误拒：降权调用对非特权必须成功，否则收尾路径失败",
            IoCode::EmptyRangeDenied => "空范围被误拒：num=0 的 ioperm 不触碰端口，应直接成功",
            IoCode::PrivLeak => "能力门泄漏：无 CAP_SYS_RAWIO 且非 root 的身份通过了 ioperm/iopl(>0)",
            IoCode::B16Drift => "B16 注记漂移：注记请求号必须与请求表同码同类同期，先校正表再判定",
            IoCode::StageRegress => "状态机非法推进：只许 Probe→Classify→Gate→Verdict 单向",
            IoCode::LedgerLeak => "注入漏检：注入数必须等于检出数，逃逸恒为零",
            IoCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }
    /// 闭集全量（顺序即码序）。
    pub const ALL: [IoCode; 12] = [
        IoCode::GateSwap, IoCode::RedlineLeak, IoCode::UnknownAsDenied, IoCode::RedlineAsNotty,
        IoCode::PortOverflow, IoCode::DowngradeDenied, IoCode::EmptyRangeDenied, IoCode::PrivLeak,
        IoCode::B16Drift, IoCode::StageRegress, IoCode::LedgerLeak, IoCode::SegmentViolation,
    ];
}

// ── 身份档 ────────────────────────────────────────────────────────────
/// 三身份档：root / 普通用户 / 安全锁定档（root + lockdown）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    Root,
    User,
    Locked,
}

/// 一个身份档的可判定要素。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Raw {
    pub euid: u32,
    pub cap_sys_rawio: bool,
    pub lockdown: bool,
}

impl Raw {
    /// 能力门：lockdown 优先否决，其余看 euid==0 或 CAP_SYS_RAWIO。
    pub fn privileged(&self) -> bool {
        if self.lockdown {
            return false;
        }
        self.euid == 0 || self.cap_sys_rawio
    }
}

impl Profile {
    pub fn index(self) -> u8 {
        match self { Profile::Root => 0, Profile::User => 1, Profile::Locked => 2 }
    }
    pub fn name(self) -> &'static str {
        match self { Profile::Root => "root", Profile::User => "user", Profile::Locked => "locked" }
    }
    pub fn raw(self) -> Raw {
        match self {
            Profile::Root => Raw { euid: 0, cap_sys_rawio: true, lockdown: false },
            Profile::User => Raw { euid: 1000, cap_sys_rawio: false, lockdown: false },
            Profile::Locked => Raw { euid: 0, cap_sys_rawio: true, lockdown: true },
        }
    }
    pub const ALL: [Profile; 3] = [Profile::Root, Profile::User, Profile::Locked];
}

// ── 请求表（禁用格单源）────────────────────────────────────────────────
/// 请求类别：合法 / 红线禁用 / 未知。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReqClass {
    Allowed,
    Redline,
    Unknown,
}

/// 请求表行：名字 + 请求号 + 类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReqRow {
    pub name: &'static str,
    pub code: u32,
    pub class: ReqClass,
}

/// 请求表（六条：三条合法、三条红线）。禁用格只在此表登记一次，判定器查表
/// 而不是另写一份 match，故「禁用」只有一个数据源。
pub const REQS: [ReqRow; 6] = [
    ReqRow { name: "TCGETS", code: TCGETS, class: ReqClass::Allowed },
    ReqRow { name: "TIOCGWINSZ", code: TIOCGWINSZ, class: ReqClass::Allowed },
    ReqRow { name: "FIONREAD", code: FIONREAD, class: ReqClass::Allowed },
    ReqRow { name: "TIOCSTI", code: TIOCSTI, class: ReqClass::Redline },
    ReqRow { name: "TIOCSSERIAL", code: TIOCSSERIAL, class: ReqClass::Redline },
    ReqRow { name: "KDSETMODE", code: KDSETMODE, class: ReqClass::Redline },
];

/// 按请求号查类别（表外一律 Unknown）。
pub fn class_of(code: u32) -> ReqClass {
    let mut i = 0usize;
    while i < REQS.len() {
        if let Some(r) = REQS.get(i) {
            if r.code == code {
                return r.class;
            }
        }
        i += 1;
    }
    ReqClass::Unknown
}

/// 按名字取行（对账用，无裸索引）。
pub fn req_by_name(name: &str) -> Option<ReqRow> {
    let mut i = 0usize;
    while i < REQS.len() {
        if let Some(r) = REQS.get(i).copied() {
            if r.name == name {
                return Some(r);
            }
        }
        i += 1;
    }
    None
}

/// 请求号两两唯一（重复即单源被破坏）。
pub fn duplicate_codes() -> u32 {
    let mut dup = 0u32;
    let mut i = 0usize;
    while i < REQS.len() {
        let mut j = i + 1;
        while j < REQS.len() {
            if let (Some(a), Some(b)) = (REQS.get(i), REQS.get(j)) {
                if a.code == b.code {
                    dup = dup.saturating_add(1);
                }
            }
            j += 1;
        }
        i += 1;
    }
    dup
}

// ── 三道门 ────────────────────────────────────────────────────────────
/// ioperm：权限门先于值域门（Linux sys_ioperm 顺序）。num=0 是合法空范围。
pub fn judge_ioperm(raw: &Raw, first: u32, num: u32) -> i32 {
    if !raw.privileged() {
        return EPERM;
    }
    if num == 0 {
        return 0;
    }
    match first.checked_add(num) {
        Some(end) => {
            if end <= IOPORT_LIMIT {
                0
            } else {
                EINVAL
            }
        }
        None => EINVAL,
    }
}

/// iopl：值域门先于权限门（Linux do_iopl 顺序）；level=0 是合法降权。
pub fn judge_iopl(raw: &Raw, level: u32) -> i32 {
    if level > IOPL_MAX {
        return EINVAL;
    }
    if level == 0 {
        return 0;
    }
    if !raw.privileged() {
        return EPERM;
    }
    0
}

/// ioctl：策略门，身份无关——合法放行、红线 EPERM、未知 ENOTTY。
pub fn judge_ioctl(_raw: &Raw, req: u32) -> i32 {
    match class_of(req) {
        ReqClass::Allowed => 0,
        ReqClass::Redline => EPERM,
        ReqClass::Unknown => ENOTTY,
    }
}

// ── 矩阵 ──────────────────────────────────────────────────────────────
/// 三种门。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermOp {
    Ioperm,
    Iopl,
    Ioctl,
}

impl PermOp {
    pub fn index(self) -> u8 {
        match self { PermOp::Ioperm => 0, PermOp::Iopl => 1, PermOp::Ioctl => 2 }
    }
    pub fn name(self) -> &'static str {
        match self { PermOp::Ioperm => "ioperm", PermOp::Iopl => "iopl", PermOp::Ioctl => "ioctl" }
    }
    pub const ALL: [PermOp; 3] = [PermOp::Ioperm, PermOp::Iopl, PermOp::Ioctl];
}

/// 一格：身份档 + 门 + 主参 a + 副参 b（ioperm 的 num；其余为 0/请求号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub profile: Profile,
    pub op: PermOp,
    pub a: u32,
    pub b: u32,
    pub expect: i32,
}

/// 十八格矩阵：ioperm 六格、iopl 六格、ioctl 六格。
pub const MATRIX: [Cell; MATRIX_CELLS] = [
    // ioperm：root 放行、user/locked 全 EPERM；root 越界 EINVAL；user 越界仍 EPERM（权限先）
    Cell { profile: Profile::Root, op: PermOp::Ioperm, a: 0x3F8, b: 8, expect: 0 },
    Cell { profile: Profile::User, op: PermOp::Ioperm, a: 0x3F8, b: 8, expect: EPERM },
    Cell { profile: Profile::Locked, op: PermOp::Ioperm, a: 0x3F8, b: 8, expect: EPERM },
    Cell { profile: Profile::Root, op: PermOp::Ioperm, a: 0xFFF8, b: 16, expect: EINVAL },
    Cell { profile: Profile::User, op: PermOp::Ioperm, a: 0xFFF8, b: 16, expect: EPERM },
    Cell { profile: Profile::Root, op: PermOp::Ioperm, a: 0, b: 0, expect: 0 },
    // iopl：root 放行、user/locked 对 >0 全 EPERM；level>3 对谁都 EINVAL（值域先）
    Cell { profile: Profile::Root, op: PermOp::Iopl, a: 3, b: 0, expect: 0 },
    Cell { profile: Profile::User, op: PermOp::Iopl, a: 3, b: 0, expect: EPERM },
    Cell { profile: Profile::Locked, op: PermOp::Iopl, a: 3, b: 0, expect: EPERM },
    Cell { profile: Profile::User, op: PermOp::Iopl, a: 4, b: 0, expect: EINVAL },
    Cell { profile: Profile::Root, op: PermOp::Iopl, a: 4, b: 0, expect: EINVAL },
    Cell { profile: Profile::User, op: PermOp::Iopl, a: 0, b: 0, expect: 0 },
    // ioctl：合法放行、红线对 root 也 EPERM、未知 ENOTTY
    Cell { profile: Profile::User, op: PermOp::Ioctl, a: TCGETS, b: 0, expect: 0 },
    Cell { profile: Profile::User, op: PermOp::Ioctl, a: TIOCSTI, b: 0, expect: EPERM },
    Cell { profile: Profile::Root, op: PermOp::Ioctl, a: TIOCSTI, b: 0, expect: EPERM },
    Cell { profile: Profile::Locked, op: PermOp::Ioctl, a: TIOCSTI, b: 0, expect: EPERM },
    Cell { profile: Profile::User, op: PermOp::Ioctl, a: UNKNOWN_REQ, b: 0, expect: ENOTTY },
    Cell { profile: Profile::Root, op: PermOp::Ioctl, a: KDSETMODE, b: 0, expect: EPERM },
];

/// 跑一格。
pub fn run_cell(cell: Cell) -> i32 {
    let raw = cell.profile.raw();
    match cell.op {
        PermOp::Ioperm => judge_ioperm(&raw, cell.a, cell.b),
        PermOp::Iopl => judge_iopl(&raw, cell.a),
        PermOp::Ioctl => judge_ioctl(&raw, cell.a),
    }
}

/// 矩阵命中数。
pub fn matrix_hits() -> u32 {
    let mut hit = 0u32;
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i).copied() {
            if run_cell(c) == c.expect {
                hit = hit.saturating_add(1);
            }
        }
        i += 1;
    }
    hit
}

/// 期望 EPERM 的格数。
pub fn eperm_cell_count() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i) {
            if c.expect == EPERM {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

// ── B16 红线注记对账 ──────────────────────────────────────────────────
/// B16 一行注记：请求名 + 冻结请求号 + 冻结期望码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B16Note {
    pub name: &'static str,
    pub code: u32,
    pub expect: i32,
}

/// B16 冻结注记（外部权威：只读，不随判定器改）。
pub const B16_FROZEN: [B16Note; B16_NOTES] = [
    B16Note { name: "TIOCSTI", code: TIOCSTI, expect: EPERM },
    B16Note { name: "TIOCSSERIAL", code: TIOCSSERIAL, expect: EPERM },
    B16Note { name: "KDSETMODE", code: KDSETMODE, expect: EPERM },
    B16Note { name: "TIOCGWINSZ", code: TIOCGWINSZ, expect: 0 },
];

/// 单条注记对账：注记码/期望与请求表 + 判定器三方一致。
pub fn note_ok(note: B16Note) -> bool {
    let row = match req_by_name(note.name) {
        Some(r) => r,
        None => return false,
    };
    if row.code != note.code {
        return false;
    }
    let raw = Profile::User.raw();
    let got = judge_ioctl(&raw, note.code);
    let class_ok = match note.expect {
        EPERM => row.class == ReqClass::Redline,
        0 => row.class == ReqClass::Allowed,
        _ => false,
    };
    class_ok && got == note.expect
}

/// 注记命中数。
pub fn b16_hits() -> u32 {
    let mut hit = 0u32;
    let mut i = 0usize;
    while i < B16_FROZEN.len() {
        if let Some(n) = B16_FROZEN.get(i).copied() {
            if note_ok(n) {
                hit = hit.saturating_add(1);
            }
        }
        i += 1;
    }
    hit
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 四档状态机：探针 → 分类 → 门判 → 结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Idle,
    Probe,
    Classify,
    Gate,
    Verdict,
}

impl Stage {
    pub fn step(self) -> Option<Stage> {
        match self {
            Stage::Idle => Some(Stage::Probe),
            Stage::Probe => Some(Stage::Classify),
            Stage::Classify => Some(Stage::Gate),
            Stage::Gate => Some(Stage::Verdict),
            Stage::Verdict => None,
        }
    }
}

/// 只接受恰好下一档。
pub fn advance(from: Stage, to: Stage) -> Result<Stage, IoCode> {
    match from.step() {
        Some(next) => if next == to { Ok(to) } else { Err(IoCode::StageRegress) },
        None => Err(IoCode::StageRegress),
    }
}

/// 拒绝格停在 Gate，放行格走完 Verdict。
pub fn stage_path(ok: bool) -> Result<Stage, IoCode> {
    let mut st = Stage::Idle;
    st = advance(st, Stage::Probe)?;
    st = advance(st, Stage::Classify)?;
    st = advance(st, Stage::Gate)?;
    if ok {
        st = advance(st, Stage::Verdict)?;
    }
    Ok(st)
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl Ledger {
    pub const fn zero() -> Ledger {
        Ledger { injected: 0, detected: 0, escaped: 0 }
    }
    pub fn record(&mut self, caught: bool) -> Result<(), IoCode> {
        self.injected = self.injected.saturating_add(1);
        if caught { self.detected = self.detected.saturating_add(1); }
        else { self.escaped = self.escaped.saturating_add(1); }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(IoCode::LedgerLeak);
        }
        Ok(())
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// EPERM 九格 × 10 轮注入；回传账本与逐格检出数。
pub fn run_permission_matrix() -> (Ledger, [u32; MATRIX_CELLS]) {
    let mut ledger = Ledger::zero();
    let mut per = [0u32; MATRIX_CELLS];
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i).copied() {
            if c.expect == EPERM {
                let mut round = 0u32;
                while round < INJECT_ROUNDS {
                    let caught = run_cell(c) == EPERM;
                    let _ = ledger.record(caught);
                    if caught {
                        if let Some(slot) = per.get_mut(i) { *slot = slot.saturating_add(1); }
                    }
                    round += 1;
                }
            }
        }
        i += 1;
    }
    (ledger, per)
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

/// 语料指纹：请求表 + 矩阵判定 + B16 命中。
pub fn corpus_fingerprint() -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < REQS.len() {
        if let Some(r) = REQS.get(i) {
            h ^= r.code;
            h = h.wrapping_mul(0x0100_0193);
            h ^= match r.class {
                ReqClass::Allowed => 1,
                ReqClass::Redline => 2,
                ReqClass::Unknown => 3,
            };
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < MATRIX.len() {
        if let Some(c) = MATRIX.get(j).copied() {
            h ^= run_cell(c) as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= c.op.index() as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        j += 1;
    }
    h ^= b16_hits();
    h
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9272 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9272_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9272");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且落在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：root 两条 I/O 门 + 普通用户合法 ioctl
    let root = Profile::Root.raw();
    let user = Profile::User.raw();
    let normal = judge_ioperm(&root, 0x3F8, 8) == 0
        && judge_iopl(&root, 3) == 0
        && judge_ioctl(&user, TCGETS) == 0
        && judge_ioctl(&user, TIOCGWINSZ) == 0;
    s.add("正常流：root ioperm/iopl(3) 与用户 TCGETS 均放行", normal,
          "合法调用不得被权限门误伤，否则探针本身不可信");

    // 3. 全 EPERM 拒绝格
    let mut eperm_hit = 0u32;
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i).copied() {
            if c.expect == EPERM && run_cell(c) == EPERM {
                eperm_hit = eperm_hit.saturating_add(1);
            }
        }
        i += 1;
    }
    s.add("全 EPERM 拒绝格：9 格逐格命中",
          eperm_hit == EPERM_CELLS as u32 && eperm_cell_count() == EPERM_CELLS as u32,
          "越权端口/关卡的拒绝必须一律 EPERM，不得换码");

    // 4. 禁用格与未知格分码
    let split_ok = judge_ioctl(&user, TIOCSTI) == EPERM
        && judge_ioctl(&user, UNKNOWN_REQ) == ENOTTY
        && judge_ioctl(&user, TIOCSTI) != ENOTTY;
    s.add("分码：禁用格 EPERM、未知格 ENOTTY，二者不混", split_ok,
          "禁用是「认识但拒绝」，未知是「不认识」；混码让调用方无法择路");

    // 5. 红线高于特权
    let redline_ok = judge_ioctl(&Profile::Root.raw(), TIOCSTI) == EPERM
        && judge_ioctl(&Profile::Locked.raw(), TIOCSSERIAL) == EPERM
        && judge_ioctl(&Profile::Root.raw(), KDSETMODE) == EPERM;
    s.add("红线高于特权：root/lockdown 调红线注入通道仍 EPERM", redline_ok,
          "红线是策略层，不由 euid 或 CAP_SYS_RAWIO 放行");

    // 6. 判定次序：ioperm 权限先于值域
    let ioperm_order = judge_ioperm(&user, 0xFFFF, 0x10) == EPERM
        && judge_ioperm(&root, 0xFFFF, 0x10) == EINVAL;
    s.add("次序（ioperm）：权限门先于值域门", ioperm_order,
          "无权身份越界端口回 EPERM，root 才暴露 EINVAL");

    // 7. 判定次序：iopl 值域先于权限
    let iopl_order = judge_iopl(&user, 4) == EINVAL && judge_iopl(&user, 3) == EPERM;
    s.add("次序（iopl）：值域门先于权限门", iopl_order,
          "level>3 对任何身份都是 EINVAL，先报 EPERM 会掩盖非法档位");

    // 8. 合法降权与空范围
    let benign_ok = judge_iopl(&user, 0) == 0
        && judge_ioperm(&root, 0, 0) == 0
        && judge_iopl(&Profile::Locked.raw(), 0) == 0;
    s.add("良性面：iopl(0) 与 num=0 空范围一律放行", benign_ok,
          "无实际效果的调用被拒会让收尾路径失败");

    // 9. 端口上界与溢出
    let bound_ok = judge_ioperm(&root, 0xFFF8, 8) == 0
        && judge_ioperm(&root, 0xFFF8, 9) == EINVAL
        && judge_ioperm(&root, u32::MAX, 2) == EINVAL;
    s.add("端口界：恰满 0x10000 放行，越界与 u32 溢出均 EINVAL", bound_ok,
          "first+num 必须 checked_add，裸加法回绕会放行越界端口");

    // 10. 能力门不泄漏
    let mut leak = false;
    let mut k = 0u32;
    while k < 1_000 {
        if judge_ioperm(&Profile::User.raw(), k, 1) != EPERM {
            leak = true;
        }
        k += 1;
    }
    s.add("能力门：无 CAP_SYS_RAWIO 的 1000 个端口逐一 EPERM", !leak,
          "端口号不应影响能力判定，任何 k 都必须 EPERM");

    // 11. 注入 10/10
    let (ledger, per) = run_permission_matrix();
    let mut all_ten = true;
    let mut ci = 0usize;
    while ci < per.len() {
        if let Some(c) = MATRIX.get(ci).copied() {
            if c.expect == EPERM {
                if let Some(n) = per.get(ci) {
                    if *n != INJECT_ROUNDS { all_ten = false; }
                }
            }
        }
        ci += 1;
    }
    s.add("禁用格注入：九格各 10/10 全检出", all_ten,
          "每格每轮都必须复现 EPERM，一轮漏检即红线失效");

    // 12. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == EPERM_CELLS as u32 * INJECT_ROUNDS,
          "90 次注入对应 90 次检出，逃逸恒为零");

    // 13. B16 注记对账
    let hits = b16_hits();
    s.add("B16 对账：四条红线注记逐条与请求表/判定器一致", hits == B16_NOTES as u32,
          "注记码、类别、期望码三方必须同源，任一处改动即漂移");

    // 14. 矩阵十八格
    let m_hits = matrix_hits();
    s.add("矩阵十八格：三档 × 三门逐格命中", m_hits == MATRIX_CELLS as u32,
          "任一格漂移即说明门序或分码被改坏");

    // 15. 请求表单源：码唯一且红线恰三条
    let redlines = REQS.iter().filter(|r| r.class == ReqClass::Redline).count();
    s.add("请求表单源：请求号两两唯一且红线恰三条",
          duplicate_codes() == 0 && redlines == 3,
          "重复码会让禁用格查表命中错误类别；红线条数漂移即覆盖变窄");

    // 16. 状态机
    let stage_ok = advance(Stage::Verdict, Stage::Gate).is_err()
        && advance(Stage::Probe, Stage::Verdict).is_err()
        && stage_path(true) == Ok(Stage::Verdict)
        && stage_path(false) == Ok(Stage::Gate);
    s.add("状态机：单向逐档，回退/跳档均拒", stage_ok,
          "拒绝格停 Gate、放行格进 Verdict，档位不可逆");

    // 17. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：语料指纹相等", a == b, "同输入两次必须逐格相等，隐藏状态即失败");

    // 18. 文案闭集
    s.add("文案闭集：12 个诊断码均有非空中文建议",
          IoCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到可操作说明，空串即判据不可读");

    // 19. 诊断码唯一
    let mut uniq = true;
    let mut x = 0usize;
    while x < IoCode::ALL.len() {
        let mut y = x + 1;
        while y < IoCode::ALL.len() {
            if let (Some(p), Some(q)) = (IoCode::ALL.get(x), IoCode::ALL.get(y)) {
                if p.val() == q.val() { uniq = false; }
            }
            y += 1;
        }
        x += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq, "闭集码重复会让判据无法定位到具体缺陷");

    // 动态明细（≤8）
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}", ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);
    let note_text = format!("B16 {}/{} · 矩阵 {}/{} · 重叠码 {}", hits, B16_NOTES, m_hits, MATRIX_CELLS, duplicate_codes());
    s.add_dyn("对账快照", hits == B16_NOTES as u32 && m_hits == MATRIX_CELLS as u32, &note_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_paths_pass() {
        let root = Profile::Root.raw();
        assert_eq!(judge_ioperm(&root, 0x3F8, 8), 0);
        assert_eq!(judge_iopl(&root, 3), 0);
        assert_eq!(judge_ioctl(&Profile::User.raw(), TCGETS), 0);
    }

    #[test]
    fn unprivileged_ioperm_eperm() {
        let user = Profile::User.raw();
        assert_eq!(judge_ioperm(&user, 0x3F8, 8), EPERM);
        assert_eq!(judge_ioperm(&user, 0, 1), EPERM);
    }

    #[test]
    fn gate_order_differs() {
        let user = Profile::User.raw();
        let root = Profile::Root.raw();
        assert_eq!(judge_ioperm(&user, 0xFFFF, 0x10), EPERM);
        assert_eq!(judge_ioperm(&root, 0xFFFF, 0x10), EINVAL);
        assert_eq!(judge_iopl(&user, 4), EINVAL);
        assert_eq!(judge_iopl(&user, 3), EPERM);
    }

    #[test]
    fn downgrade_and_empty_range_pass() {
        assert_eq!(judge_iopl(&Profile::User.raw(), 0), 0);
        assert_eq!(judge_iopl(&Profile::Locked.raw(), 0), 0);
        assert_eq!(judge_ioperm(&Profile::Root.raw(), 0, 0), 0);
    }

    #[test]
    fn port_bound_and_overflow() {
        let root = Profile::Root.raw();
        assert_eq!(judge_ioperm(&root, 0xFFF8, 8), 0);
        assert_eq!(judge_ioperm(&root, 0xFFF8, 9), EINVAL);
        assert_eq!(judge_ioperm(&root, u32::MAX, 2), EINVAL);
    }

    #[test]
    fn redline_beats_privilege() {
        assert_eq!(judge_ioctl(&Profile::Root.raw(), TIOCSTI), EPERM);
        assert_eq!(judge_ioctl(&Profile::Locked.raw(), TIOCSTI), EPERM);
        assert_eq!(judge_ioctl(&Profile::Root.raw(), KDSETMODE), EPERM);
    }

    #[test]
    fn unknown_is_enotty_not_eperm() {
        assert_eq!(judge_ioctl(&Profile::User.raw(), UNKNOWN_REQ), ENOTTY);
        assert_eq!(judge_ioctl(&Profile::Root.raw(), UNKNOWN_REQ), ENOTTY);
        assert_eq!(class_of(UNKNOWN_REQ), ReqClass::Unknown);
    }

    #[test]
    fn locked_down_denies_io_ports() {
        let locked = Profile::Locked.raw();
        assert!(!locked.privileged());
        assert_eq!(judge_ioperm(&locked, 0x3F8, 8), EPERM);
        assert_eq!(judge_iopl(&locked, 3), EPERM);
    }

    #[test]
    fn matrix_all_cells_hit() {
        assert_eq!(matrix_hits(), MATRIX_CELLS as u32);
        for c in MATRIX.iter() {
            assert_eq!(run_cell(*c), c.expect, "{} {}", c.profile.name(), c.op.name());
        }
    }

    #[test]
    fn b16_on_note_ok() {
        assert_eq!(b16_hits(), B16_NOTES as u32);
        for n in B16_FROZEN.iter() {
            assert!(note_ok(*n), "note {}", n.name);
        }
        assert_eq!(duplicate_codes(), 0);
    }

    #[test]
    fn injection_ledger_conserved() {
        let (l, per) = run_permission_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, EPERM_CELLS as u32 * INJECT_ROUNDS);
        for (i, c) in MATRIX.iter().enumerate() {
            let n = per.get(i).copied().unwrap_or(0);
            if c.expect == EPERM { assert_eq!(n, INJECT_ROUNDS); }
            else { assert_eq!(n, 0); }
        }
    }

    #[test]
    fn stage_and_fingerprint() {
        assert_eq!(stage_path(true), Ok(Stage::Verdict));
        assert_eq!(stage_path(false), Ok(Stage::Gate));
        assert!(advance(Stage::Verdict, Stage::Gate).is_err());
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9272_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
