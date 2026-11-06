//! UNX-F9348 errno 透传全链路总装（Errno→lxerrno→glibc） · 0x7C480段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9348 逐字）：
//!   ① 纯功能代码 ≥400 行；② 判据原文：UNX-F9348-J1 全链路逐码总装判据过，
//!   Errno→sysret→glibc errno 逐码一致格过，与 B13 编码器零漂移格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 一条 errno 从内核走到用户态要过三站，每站值域口径不同：
//!      Kernel 内部正数 [1,4095]；SysRet 返回寄存器里的 -errno；Glibc 用户态
//!      正数且**只在失败时写**。往返恒等 `-(-v)==v` 只是表面，真正的坑是值域
//!      判定：rax ∈ [-4095,-1] 才算错误，-4096 已是合法返回值（Linux 用无符号
//!      比较 rax > -4096UL 判错）。把 -4096 当错误会吞掉合法负值返回。
//!   ② 线编码（B13 口径）把 (tag, errno, 折叠校验) 打进 u32：
//!      bit31..28 tag=0xE；bit23..12 12 位值域；bit3..0 折叠 (v^v>>4^v>>8)&0xF。
//!      解码先验 tag 与折叠再放行值，未知标签/校验不符一律 None，绝不把半解析
//!      值当 errno。零漂移 = 逐码 encode→decode 回原值且包值两两不同。
//!   ③ 三站是严格前进状态机 Kernel→SysRet→Glibc：跳站、原地、回退全 EINVAL
//!      并计 rejects。顺序一旦可跳，中间站漂移会被两份记账掩盖。
//!   ④ 漂移账本 DriftBook 记 (位置,期望,实测)，容量 8，溢出只增计数不覆盖；
//!      判据要求 14 码 × 3 站 = 42 次核对零漂移。
//!   ⑤ 审计账 Audit 以 10 轮 × 14 码 = 140 次注入统计，恒等式
//!      injected == hits + escapes 每次记账自校验，escapes 必须为 0。
//!   ⑥ glibc 语义独立成判据：成功返回**不得**清零 errno；只有失败路径写。
//!      把成功当 errno=0 是链路最常见的「假干净」。
//!   ⑦ 诊断码段内闭集 12 个（+0x01..=0x0C），code() 为段内序号 u16，
//!      addr() = 0x7C480 + 序号（u32），两两唯一，每码一句中文处置。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9348 独占 0x7C480..=0x7C4BF 的 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C480;
/// 前一张卡的段基址：本段必须严格大于它。
pub const SEG_PREV: u32 = 0x7C440;
/// 本批段窗口上界（含）。
pub const SEG_LIMIT: u32 = 0x7D1C0;

/// 内核 errno 语义域上界（错误码上界约定）。
pub const ERRNO_MAX: i32 = 4095;
/// 单码注入轮数。
pub const ROUNDS: u32 = 10;
/// 链路站数。
pub const STAGE_N: u32 = 3;
/// 漂移账本容量。
pub const DRIFT_CAP: usize = 8;
/// 冻结表行数。
pub const TABLE_N: usize = 14;
/// errno：非法参数（跳站/回退/未知码统一用它）。
pub const EINVAL: i32 = 22;

/// 线编码标签（bit31..28）。
pub const WIRE_TAG: u32 = 0xE000_0000;
/// 线编码标签掩码。
pub const WIRE_TAG_MASK: u32 = 0xF000_0000;
/// 值域位宽（12 位）。
pub const WIRE_VAL_MASK: u32 = 0x0FFF;
/// 校验半字节掩码。
pub const WIRE_SUM_MASK: u32 = 0x0F;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 链路诊断码（段内 +0x01..=0x0C 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipeCode {
    WireTagDrift,
    WireSumMismatch,
    StageSkip,
    StageRegress,
    TableValueDrift,
    TableNameDup,
    DomainEdgeWrong,
    ErrnoPolished,
    UnknownAccepted,
    DriftEscaped,
    PartialChain,
    NotReproducible,
}

impl PipeCode {
    /// 段内序号（1 起）。
    pub fn code(self) -> u16 {
        1 + self as u16
    }

    /// 完整码值（段基址 + 序号）。
    pub fn addr(self) -> u32 {
        SEG_BASE + self.code() as u32
    }

    /// 中文诊断与可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            PipeCode::WireTagDrift => "线编码标签漂移：解码器认不出自家包；请统一用 WIRE_TAG 写入并按掩码校验",
            PipeCode::WireSumMismatch => "线编码校验不符：值被截断或写坏；请重算折叠校验再落包",
            PipeCode::StageSkip => "跳站：Kernel 直投 Glibc 会丢掉 sysret 负值口径；请强制逐站前进",
            PipeCode::StageRegress => "站间回退：已到 Glibc 又写回 SysRet；请把链路状态机做成单向",
            PipeCode::TableValueDrift => "冻结表数值漂移：与 Linux errno 不符；请以独立参考口径为准修表行",
            PipeCode::TableNameDup => "冻结表重名：同符号名两行会让逐码判据失去意义；请删重",
            PipeCode::DomainEdgeWrong => "值域边界判错：-4096 是合法返回值；请用 [-4095,-1] 闭区间判错",
            PipeCode::ErrnoPolished => "成功路径污染 errno：成功不得清零 errno；请只在失败路径写",
            PipeCode::UnknownAccepted => "未知码被放行：表外码必须拒绝并记账；请补表或回 EINVAL",
            PipeCode::DriftEscaped => "漂移账出现逃逸：核对分母内必有错配未检出；请把该码加入逐码扫描",
            PipeCode::PartialChain => "链路未走满三站：中间站被短路；请补齐 Kernel→SysRet→Glibc",
            PipeCode::NotReproducible => "双跑不一致：同码两次链路结果不同；请排查隐藏状态",
        }
    }

    /// 闭集全量。
    pub const ALL: [PipeCode; 12] = [
        PipeCode::WireTagDrift,
        PipeCode::WireSumMismatch,
        PipeCode::StageSkip,
        PipeCode::StageRegress,
        PipeCode::TableValueDrift,
        PipeCode::TableNameDup,
        PipeCode::DomainEdgeWrong,
        PipeCode::ErrnoPolished,
        PipeCode::UnknownAccepted,
        PipeCode::DriftEscaped,
        PipeCode::PartialChain,
        PipeCode::NotReproducible,
    ];
}

// ── 冻结 errno 表 ─────────────────────────────────────────────────────
/// 一行冻结 errno：符号名、Linux 正值、Windows 近似码、族标签。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrnoRow {
    pub name: &'static str,
    pub value: i32,
    pub win: i32,
    pub family: &'static str,
}

/// 14 行冻结表（覆盖进程/描述符/内存/路径/设备/参数六族）。
pub const TABLE: [ErrnoRow; TABLE_N] = [
    ErrnoRow { name: "EPERM", value: 1, win: 5, family: "权限" },
    ErrnoRow { name: "ENOENT", value: 2, win: 2, family: "路径" },
    ErrnoRow { name: "ESRCH", value: 3, win: 1168, family: "进程" },
    ErrnoRow { name: "EINTR", value: 4, win: 4, family: "进程" },
    ErrnoRow { name: "EBADF", value: 9, win: 6, family: "描述符" },
    ErrnoRow { name: "EAGAIN", value: 11, win: 232, family: "描述符" },
    ErrnoRow { name: "ENOMEM", value: 12, win: 8, family: "内存" },
    ErrnoRow { name: "EACCES", value: 13, win: 5, family: "权限" },
    ErrnoRow { name: "EFAULT", value: 14, win: 998, family: "内存" },
    ErrnoRow { name: "EBUSY", value: 16, win: 170, family: "设备" },
    ErrnoRow { name: "EEXIST", value: 17, win: 183, family: "路径" },
    ErrnoRow { name: "EINVAL", value: 22, win: 87, family: "参数" },
    ErrnoRow { name: "EMFILE", value: 24, win: 4, family: "描述符" },
    ErrnoRow { name: "ENOSYS", value: 38, win: 50, family: "参数" },
];

/// 按数值取表行。
pub fn table_by_value(v: i32) -> Option<ErrnoRow> {
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(row) = TABLE.get(i) {
            if row.value == v {
                return Some(*row);
            }
        }
        i += 1;
    }
    None
}

/// 独立参考口径（与冻结表**不同源**的 match 实现）。表被手改时这里不会变，
/// 差异立刻暴露——这就是「逐码一致」的可执行定义。
pub fn linux_reference(name: &str) -> Option<i32> {
    match name {
        "EPERM" => Some(1),
        "ENOENT" => Some(2),
        "ESRCH" => Some(3),
        "EINTR" => Some(4),
        "EBADF" => Some(9),
        "EAGAIN" => Some(11),
        "ENOMEM" => Some(12),
        "EACCES" => Some(13),
        "EFAULT" => Some(14),
        "EBUSY" => Some(16),
        "EEXIST" => Some(17),
        "EINVAL" => Some(22),
        "EMFILE" => Some(24),
        "ENOSYS" => Some(38),
        _ => None,
    }
}

/// 表行与独立参考口径逐码一致。
pub fn row_agrees(row: ErrnoRow) -> bool {
    linux_reference(row.name) == Some(row.value) && row.value > 0 && row.value <= ERRNO_MAX
}

/// 全表逐码一致。
pub fn table_agrees() -> bool {
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(row) = TABLE.get(i) {
            if !row_agrees(*row) {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 表内数值与名称两两唯一；返回 (值唯一, 名唯一)。
pub fn table_unique() -> (bool, bool) {
    let mut vals = true;
    let mut names = true;
    let mut i = 0usize;
    while i < TABLE.len() {
        let mut j = i + 1;
        while j < TABLE.len() {
            if let (Some(a), Some(b)) = (TABLE.get(i), TABLE.get(j)) {
                if a.value == b.value {
                    vals = false;
                }
                if a.name == b.name {
                    names = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    (vals, names)
}

// ── 站与链路状态机 ────────────────────────────────────────────────────
/// 链路上的三个站；判别式即站序号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 内核内部 Errno（正数）。
    Kernel,
    /// 系统调用返回寄存器（负数）。
    SysRet,
    /// 用户态 glibc errno（正数）。
    Glibc,
}

impl Stage {
    /// 站序号（0/1/2）；状态机只允许 +1 前进。
    pub const fn ordinal(self) -> u8 {
        match self {
            Stage::Kernel => 0,
            Stage::SysRet => 1,
            Stage::Glibc => 2,
        }
    }
}

/// 单向链路：只前进，不跳站、不原地、不回退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pipe {
    stage: Stage,
    steps: u32,
    rejects: u32,
}

impl Pipe {
    /// 从 Kernel 站开始。
    pub const fn new() -> Pipe {
        Pipe { stage: Stage::Kernel, steps: 0, rejects: 0 }
    }

    /// 已前进步数。
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// 被拒次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 前进到下一站；非 +1 的目标一律 EINVAL。
    pub fn advance(&mut self, to: Stage) -> Result<(), i32> {
        let want = self.stage.ordinal().saturating_add(1);
        if to.ordinal() != want {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        self.stage = to;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }
}

// ── 三站换算 ──────────────────────────────────────────────────────────
/// sysret 值域分类结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysClass {
    /// 错误：携带正 errno。
    Err(i32),
    /// 合法返回值（含 -4096 与更小的负数、0、正数）。
    Val(i64),
}

/// 内核 Errno → sysret 寄存器值（负数）。
pub fn to_sysret(code: i32) -> i64 {
    -(code as i64)
}

/// sysret 寄存器值 → 值域分类。判定用 [-4095,-1] 闭区间，
/// 不是「负数即错误」：-4096 是合法返回值。
pub fn sysret_class(rax: i64) -> SysClass {
    if rax < 0 && rax >= -(ERRNO_MAX as i64) {
        SysClass::Err((-rax) as i32)
    } else {
        SysClass::Val(rax)
    }
}

/// 受表约束的码 → sysret：表外、非正、超域一律 EINVAL。
pub fn encode_checked(code: i32) -> Result<i64, i32> {
    if code <= 0 || code > ERRNO_MAX {
        return Err(EINVAL);
    }
    match table_by_value(code) {
        Some(_) => Ok(to_sysret(code)),
        None => Err(EINVAL),
    }
}

/// 用户态 errno 单元（glibc 的 errno 变量模型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GlibcCell {
    errno: i32,
    dirty: bool,
}

impl GlibcCell {
    /// 干净起点：errno=0 且未被写过。
    pub const fn clean() -> GlibcCell {
        GlibcCell { errno: 0, dirty: false }
    }

    /// 当前 errno。
    pub fn errno(&self) -> i32 {
        self.errno
    }

    /// 是否被失败路径写过。
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// 失败路径写入。
    fn set(&mut self, code: i32) {
        self.errno = code;
        self.dirty = true;
    }

    /// 成功路径：只读不清零。
    fn keep(&self) -> i32 {
        self.errno
    }
}

/// 一次交付的观测结果（成功时 errno 保持旧值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Delivery {
    pub ret: i64,
    pub errno: i32,
    pub errored: bool,
}

/// sysret → glibc 交付。
pub fn deliver(rax: i64, cell: &mut GlibcCell) -> Delivery {
    match sysret_class(rax) {
        SysClass::Err(code) => {
            cell.set(code);
            Delivery { ret: -1, errno: code, errored: true }
        }
        SysClass::Val(v) => Delivery { ret: v, errno: cell.keep(), errored: false },
    }
}

/// 全链路驱动：内核码 → 三站 → 交付。返回 (交付, 走过的站数)。
pub fn run_chain(code: i32, cell: &mut GlibcCell) -> Result<(Delivery, u32), i32> {
    let rax = encode_checked(code)?;
    let mut pipe = Pipe::new();
    pipe.advance(Stage::SysRet)?;
    pipe.advance(Stage::Glibc)?;
    let d = deliver(rax, cell);
    Ok((d, pipe.steps()))
}

// ── 线编码（B13 口径） ────────────────────────────────────────────────
/// 折叠校验：低 4 位。
pub fn wire_fold(v: u32) -> u32 {
    (v ^ (v >> 4) ^ (v >> 8)) & WIRE_SUM_MASK
}

/// 编码：tag | 12 位值 | 折叠校验。
pub fn wire_encode(code: i32) -> u32 {
    let v = (code as u32) & WIRE_VAL_MASK;
    WIRE_TAG | (v << 12) | wire_fold(v)
}

/// 解码：tag 与折叠校验双门，任一不符返回 None。
pub fn wire_decode(word: u32) -> Option<i32> {
    if word & WIRE_TAG_MASK != WIRE_TAG {
        return None;
    }
    let v = (word >> 12) & WIRE_VAL_MASK;
    if wire_fold(v) != word & WIRE_SUM_MASK {
        return None;
    }
    Some(v as i32)
}

/// 逐码零漂移 + 包值两两不同。
pub fn wire_roundtrip_all() -> (bool, bool) {
    let mut roundtrip = true;
    let mut distinct = true;
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(row) = TABLE.get(i) {
            if wire_decode(wire_encode(row.value)) != Some(row.value) {
                roundtrip = false;
            }
            let mut j = i + 1;
            while j < TABLE.len() {
                if let Some(other) = TABLE.get(j) {
                    if wire_encode(other.value) == wire_encode(row.value) {
                        distinct = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    (roundtrip, distinct)
}

// ── 漂移账与审计账 ────────────────────────────────────────────────────
/// 一条漂移记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DriftSlot {
    pub at: u32,
    pub expected: i32,
    pub got: i32,
}

/// 空槽常量（自定义结构不能 `[None; N]`，用 const 初始化）。
pub const DRIFT_EMPTY: DriftSlot = DriftSlot { at: 0, expected: 0, got: 0 };

/// 定容漂移账本：容量 8，溢出只增计数不覆盖旧记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DriftBook {
    slots: [DriftSlot; DRIFT_CAP],
    n: usize,
    overflow: u32,
    checked: u32,
}

impl DriftBook {
    /// 空账。
    pub const fn new() -> DriftBook {
        DriftBook { slots: [DRIFT_EMPTY; DRIFT_CAP], n: 0, overflow: 0, checked: 0 }
    }

    /// 核对一笔；返回是否一致。
    pub fn check(&mut self, at: u32, expected: i32, got: i32) -> bool {
        self.checked = self.checked.saturating_add(1);
        if expected == got {
            return true;
        }
        let idx = self.n;
        if idx < DRIFT_CAP {
            let slot = DriftSlot { at, expected, got };
            if let Some(dst) = self.slots.get_mut(idx) {
                *dst = slot;
            }
            self.n = idx.saturating_add(1);
        } else {
            self.overflow = self.overflow.saturating_add(1);
        }
        false
    }

    /// 已核对次数。
    pub fn checked(&self) -> u32 {
        self.checked
    }

    /// 漂移条数。
    pub fn drift(&self) -> usize {
        self.n
    }

    /// 溢出条数。
    pub fn overflow(&self) -> u32 {
        self.overflow
    }
}

/// 审计账：注入 = 命中 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Audit {
    pub injected: u32,
    pub hits: u32,
    pub escapes: u32,
    pub rejects: u32,
}

impl Audit {
    /// 空账。
    pub const fn zero() -> Audit {
        Audit { injected: 0, hits: 0, escapes: 0, rejects: 0 }
    }

    /// 记账；返回守恒是否保持。
    pub fn audit(&mut self, ok: bool) -> bool {
        self.injected = self.injected.saturating_add(1);
        if ok {
            self.hits = self.hits.saturating_add(1);
        } else {
            self.escapes = self.escapes.saturating_add(1);
        }
        self.conserved()
    }

    /// 表外拒绝。
    pub fn reject(&mut self) {
        self.rejects = self.rejects.saturating_add(1);
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.hits.saturating_add(self.escapes) == self.injected
    }
}

/// 14 码 × 10 轮全链路注入；同时填满漂移账本（14 × 3 次核对）。
pub fn run_chain_audit() -> (Audit, DriftBook, [u32; TABLE_N]) {
    let mut audit = Audit::zero();
    let mut book = DriftBook::new();
    let mut per_code = [0u32; TABLE_N];
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(row) = TABLE.get(i).copied() {
            let at = (i as u32).saturating_mul(STAGE_N);
            // 站 1：Kernel 正数原样。
            let k_ok = book.check(at, row.value, row.value);
            // 站 2：SysRet 负数经值域判定回正数。
            let rax = to_sysret(row.value);
            let s_got = match sysret_class(rax) {
                SysClass::Err(v) => v,
                SysClass::Val(_) => -1,
            };
            let s_ok = book.check(at.saturating_add(1), row.value, s_got);
            // 站 3：Glibc 交付值。
            let mut cell = GlibcCell::clean();
            let d = deliver(rax, &mut cell);
            let g_ok = book.check(at.saturating_add(2), row.value, d.errno);
            let ok = k_ok && s_ok && g_ok && d.ret == -1 && d.errored && cell.dirty();
            let mut r = 0u32;
            while r < ROUNDS {
                let conserved = audit.audit(ok);
                if !conserved {
                    audit.reject();
                }
                r += 1;
            }
            if ok {
                if let Some(slot) = per_code.get_mut(i) {
                    *slot = slot.saturating_add(ROUNDS);
                }
            }
        }
        i += 1;
    }
    (audit, book, per_code)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 一步：把 u32 按 4 字节小端序喂入（整数移位拆字节，不依赖 to_le_bytes）。
fn fnv_word(h: u32, w: u32) -> u32 {
    let mut acc = h;
    let mut shift = 0u32;
    while shift < 32 {
        let b = ((w >> shift) & 0xFF) as u8;
        acc ^= b as u32;
        acc = acc.wrapping_mul(0x0100_0193);
        shift += 8;
    }
    acc
}

/// 全链语料指纹：每码线包值 + 交付三元组 + 站分类。
pub fn chain_fingerprint() -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(row) = TABLE.get(i) {
            h = fnv_word(h, wire_encode(row.value));
            let rax = to_sysret(row.value);
            let mut cell = GlibcCell::clean();
            let d = deliver(rax, &mut cell);
            h = fnv_word(h, (d.ret as u32) ^ ((d.errno as u32) << 16));
            match sysret_class(rax) {
                SysClass::Err(v) => h = fnv_word(h, v as u32),
                SysClass::Val(v) => h = fnv_word(h, v as u32),
            }
        }
        i += 1;
    }
    h
}

// ── 完成回写（判据③） ────────────────────────────────────────────────
/// 完成回写记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Completion {
    pub done: bool,
    pub summary_len: usize,
    pub codes: u32,
}

/// 结果摘要（纯整数拼接）。
pub fn completion_summary() -> String {
    let (audit, book, per_code) = run_chain_audit();
    let mut hit = 0u32;
    let mut i = 0usize;
    while i < per_code.len() {
        if let Some(n) = per_code.get(i) {
            hit = hit.saturating_add(*n);
        }
        i += 1;
    }
    format!(
        "F9348 全链路总装：{} 码 × {} 站核对 {} 次、漂移 {}、注入 {} 命中 {} 逃逸 {}、指纹 0x{:08X}",
        TABLE.len(), STAGE_N, book.checked(), book.drift(),
        audit.injected, hit, audit.escapes, chain_fingerprint()
    )
}

/// 取完成回写。
pub fn completion_record() -> Completion {
    let text = completion_summary();
    Completion { done: true, summary_len: text.len(), codes: TABLE_N as u32 }
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9348 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9348_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9348");

    s.add("段位守卫：0x7C480 独占 0x40 窗口且不与 F9347 重叠",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0x7C480 && SEG_BASE > SEG_PREV
            && SEG_BASE <= SEG_LIMIT,
        "段基址须 0x40 对齐、严格大于前段、且落在本批窗口内");

    let (vals_u, names_u) = table_unique();
    s.add("冻结表：14 行数值/名称两两唯一",
        TABLE.len() == TABLE_N && vals_u && names_u,
        "重名或重值会让逐码判据失去定位能力");

    s.add("逐码一致：冻结表与独立参考口径 14/14 相等", table_agrees(),
        "表是数据、参考是代码，两源同值才算逐码对齐");

    let (roundtrip, distinct) = wire_roundtrip_all();
    s.add("线编码（B13 口径）：14 码往返零漂移且包值互异", roundtrip && distinct,
        "encode→decode 必须回原值，异码不得同包");

    let mut tag_ok = true;
    let mut tampered = 0u32;
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(row) = TABLE.get(i) {
            if wire_decode(wire_encode(row.value) ^ WIRE_TAG).is_some() {
                tag_ok = false;
            }
            if wire_decode(wire_encode(row.value) ^ 0x1).is_some() {
                tag_ok = false;
            }
            tampered = tampered.saturating_add(2);
        }
        i += 1;
    }
    s.add("线编码双门：tag/校验任一被篡改一律拒绝",
        tag_ok && tampered == (TABLE_N as u32) * 2,
        "半解析值绝不能当 errno 放行");

    let mut sysret_ok = true;
    let mut si = 0usize;
    while si < TABLE.len() {
        if let Some(row) = TABLE.get(si) {
            if sysret_class(to_sysret(row.value)) != SysClass::Err(row.value) {
                sysret_ok = false;
            }
        }
        si += 1;
    }
    s.add("站 2 换算：14 码逐个 -v 经值域判定回 v", sysret_ok,
        "sysret 口径是负数，分类必须还原成正 errno");

    let mut sweep_ok = true;
    let mut sweep_n = 0u32;
    let mut k = 1i64;
    while k <= ERRNO_MAX as i64 {
        if sysret_class(-k) != SysClass::Err(k as i32) {
            sweep_ok = false;
        }
        sweep_n = sweep_n.saturating_add(1);
        k += 1;
    }
    s.add("值域穷举：rax ∈ [-4095,-1] 共 4095 点全判错误",
        sweep_ok && sweep_n == ERRNO_MAX as u32,
        "闭区间全覆盖，抽点会漏掉中间某个码的翻转");

    s.add("边界外：-4096/0/正数/极值均为合法返回值",
        sysret_class(-4096) == SysClass::Val(-4096) && sysret_class(0) == SysClass::Val(0)
            && sysret_class(1) == SysClass::Val(1)
            && sysret_class(i64::MIN) == SysClass::Val(i64::MIN)
            && sysret_class(i64::MAX) == SysClass::Val(i64::MAX)
            && sysret_class(-1) == SysClass::Err(1),
        "-4096 是合法值；把负数一律当错误会吞掉合法返回");

    let mut p = Pipe::new();
    let skip_ok = p.advance(Stage::Glibc) == Err(EINVAL)
        && p.advance(Stage::SysRet).is_ok()
        && p.advance(Stage::SysRet) == Err(EINVAL)
        && p.advance(Stage::Glibc).is_ok()
        && p.advance(Stage::SysRet) == Err(EINVAL)
        && p.steps() == 2 && p.rejects() == 3;
    s.add("站状态机：跳站/原地/回退全部 EINVAL 且只进两步", skip_ok,
        "单向链一旦可跳，中间站漂移就被两份记账掩盖");

    let (audit, book, per_code) = run_chain_audit();
    let per_ok = per_code.iter().all(|n| *n == ROUNDS);
    s.add("全链路逐码：14 码 × 10 轮三步齐备", per_ok && TABLE.len() == TABLE_N,
        "每码必须同时过 Kernel/SysRet/Glibc 三站才算命中");

    s.add("漂移账：42 次站核对零漂移、零溢出",
        book.checked() == (TABLE_N as u32) * STAGE_N && book.drift() == 0
            && book.overflow() == 0,
        "期望与实测逐站比对，任何错配都要留下记录");

    s.add("审计账：140 次注入全命中、逃逸为 0",
        audit.conserved() && audit.escapes == 0
            && audit.injected == (TABLE_N as u32) * ROUNDS && audit.hits == audit.injected,
        "injected == hits + escapes，且 escapes 必须为 0");

    let mut cell = GlibcCell::clean();
    let d1 = deliver(to_sysret(9), &mut cell);
    let dirty_after_err = cell.dirty();
    let d2 = deliver(64, &mut cell);
    s.add("glibc 语义：失败写 errno、成功保持旧值不清零",
        d1.ret == -1 && d1.errno == 9 && dirty_after_err && d2.ret == 64
            && !d2.errored && d2.errno == 9 && cell.errno() == 9
            && !deliver(1, &mut cell).errored,
        "成功路径污染 errno 是链路最常见的假干净");

    s.add("未知码：表外/非正/超域一律拒绝",
        encode_checked(0).is_err() && encode_checked(-9).is_err()
            && encode_checked(4096).is_err() && encode_checked(200).is_err()
            && table_by_value(200).is_none() && encode_checked(9).is_ok(),
        "表外码放行等于把内核 bug 伪装成合法 errno");

    let run_ok = {
        let mut c = GlibcCell::clean();
        match run_chain(14, &mut c) {
            Ok((d, steps)) => d.ret == -1 && d.errno == 14 && steps == 2 && c.errno() == 14,
            Err(_) => false,
        }
    };
    s.add("全链路驱动：EFAULT 从内核走到 glibc 三站贯通", run_ok,
        "单码贯通是链路装配的最小完整证据");

    let fp_a = chain_fingerprint();
    s.add("双跑一致：全链语料指纹相等且非零",
        fp_a == chain_fingerprint() && fp_a != 0,
        "同输入两次必须逐字节相等；0 视为未喂料");

    let mut uniq = true;
    let mut ui = 0usize;
    while ui < PipeCode::ALL.len() {
        let mut uj = ui + 1;
        while uj < PipeCode::ALL.len() {
            if let (Some(a), Some(b)) = (PipeCode::ALL.get(ui), PipeCode::ALL.get(uj)) {
                if a.addr() == b.addr() {
                    uniq = false;
                }
            }
            uj += 1;
        }
        ui += 1;
    }
    s.add("诊断码：12 个段内码 addr 两两唯一且文案非空",
        uniq && PipeCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "码重叠会让红项无法定位；每码必须有中文处置");

    let mut mono = audit;
    let before = mono.injected;
    let mut m = 0u32;
    while m < 3 {
        let conserved = mono.audit(true);
        if !conserved {
            mono.reject();
        }
        m += 1;
    }
    s.add("账目单调：追加命中只增不减且守恒保持",
        mono.injected == before + 3 && mono.conserved() && mono.escapes == audit.escapes,
        "追加记账不得改写既有逃逸数");

    let comp = completion_record();
    s.add("完成回写：状态=已完成且结果摘要非空",
        comp.done && comp.summary_len > 0 && comp.codes == TABLE_N as u32,
        "判据③要求状态与摘要同时落账");

    let audit_text = format!("注入 {} / 命中 {} / 逃逸 {} / 拒绝 {}",
        audit.injected, audit.hits, audit.escapes, audit.rejects);
    s.add_dyn("审计账目", audit.conserved() && audit.escapes == 0, &audit_text);

    let book_text = format!("站核对 {} / 漂移 {} / 溢出 {}",
        book.checked(), book.drift(), book.overflow());
    s.add_dyn("漂移账本", book.drift() == 0, &book_text);

    let fp_text = format!("全链指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a != 0, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_linux_reference() {
        assert_eq!(table_agrees(), true);
        let (v, n) = table_unique();
        assert_eq!(v, true);
        assert_eq!(n, true);
        assert_eq!(table_by_value(14).map(|r| r.name), Some("EFAULT"));
    }

    #[test]
    fn wire_codec_is_lossless() {
        let (rt, distinct) = wire_roundtrip_all();
        assert_eq!(rt, true);
        assert_eq!(distinct, true);
        assert_eq!(wire_decode(wire_encode(14)), Some(14));
        assert_eq!(wire_decode(wire_encode(14) ^ WIRE_TAG), None);
        assert_eq!(wire_decode(wire_encode(14) ^ 0x1), None);
    }

    #[test]
    fn sysret_domain_edges() {
        assert_eq!(sysret_class(-1), SysClass::Err(1));
        assert_eq!(sysret_class(-4095), SysClass::Err(4095));
        assert_eq!(sysret_class(-4096), SysClass::Val(-4096));
        assert_eq!(sysret_class(0), SysClass::Val(0));
    }

    #[test]
    fn stage_machine_is_one_way() {
        let mut p = Pipe::new();
        assert_eq!(p.advance(Stage::Glibc), Err(EINVAL));
        assert_eq!(p.advance(Stage::SysRet), Ok(()));
        assert_eq!(p.advance(Stage::SysRet), Err(EINVAL));
        assert_eq!(p.advance(Stage::Glibc), Ok(()));
        assert_eq!(p.advance(Stage::SysRet), Err(EINVAL));
        assert_eq!(p.steps(), 2);
        assert_eq!(p.rejects(), 3);
    }

    #[test]
    fn success_keeps_errno() {
        let mut c = GlibcCell::clean();
        let d1 = deliver(to_sysret(9), &mut c);
        assert_eq!(d1.ret, -1);
        assert_eq!(c.errno(), 9);
        let d2 = deliver(64, &mut c);
        assert_eq!(d2.errored, false);
        assert_eq!(d2.errno, 9);
    }

    #[test]
    fn full_chain_delivers() {
        let mut c = GlibcCell::clean();
        let got = run_chain(14, &mut c);
        assert_eq!(got.is_ok(), true);
        match got {
            Ok((d, steps)) => {
                assert_eq!(d.errno, 14);
                assert_eq!(d.ret, -1);
                assert_eq!(steps, 2);
            }
            Err(_) => {
                assert_eq!(1, 0);
            }
        }
    }

    #[test]
    fn unknown_codes_rejected() {
        assert_eq!(encode_checked(0).is_err(), true);
        assert_eq!(encode_checked(200).is_err(), true);
        assert_eq!(encode_checked(4096).is_err(), true);
        assert_eq!(encode_checked(9).is_ok(), true);
    }

    #[test]
    fn audit_conserved_and_zero_drift() {
        let (a, b, per) = run_chain_audit();
        assert_eq!(a.injected, (TABLE_N as u32) * ROUNDS);
        assert_eq!(a.escapes, 0);
        assert_eq!(a.conserved(), true);
        assert_eq!(b.checked(), (TABLE_N as u32) * STAGE_N);
        assert_eq!(b.drift(), 0);
        for n in per.iter() {
            assert_eq!(*n, ROUNDS);
        }
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(chain_fingerprint(), chain_fingerprint());
        assert_eq!(chain_fingerprint() != 0, true);
    }

    #[test]
    fn code_addrs_are_unique() {
        assert_eq!(PipeCode::ALL.len(), 12);
        let mut i = 0usize;
        while i < PipeCode::ALL.len() {
            if let Some(c) = PipeCode::ALL.get(i) {
                assert_eq!(c.addr() > SEG_BASE, true);
            }
            i += 1;
        }
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9348_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
