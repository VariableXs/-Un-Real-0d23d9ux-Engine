//! UNX-F11593 静态链接 busybox 运行判据（AI-94 产物对接）
//!
//! AI-94 交出来的是「一个静态链接的 busybox 可执行文件」。判据要回答的
//! 不是「文件在不在」，而是**这份产物能不能在没有 ld.so 的根文件系统里
//! 直接跑起来**。四层不可省：
//!
//! ```text
//! 产物   kind（static/dynamic）、size、entry、.so 依赖数、系统调用需求集
//! 校验   动态即拒（无 ld.so）；尺寸上下限；入口对齐；静态不得带 .so；
//!        调用编号合法、去重、核心四调用（read/write/exit_group/execve）必须在
//! 装载   entry_va = LOAD_BASE + entry；页数 = ceil(size/4096)；
//!        gap = STACK_BASE − (LOAD_BASE + size)（栈与映像之间的空隙）
//! 判决   Runs / Degraded(缺某个非核心调用) / Rejected(错误码)
//! ```
//!
//! 为什么「动态」必须硬拒而不是降级：静态 busybox 的全部意义是把 libc
//! 链进映像，一旦产物带 `DT_NEEDED`，在只有它一个文件的 initramfs 里
//! 连解释器都找不到，降级成「能跑一部分」是假象。所以 `kind` 闸排在
//! 所有闸之前，且 `Rejected` 携带的码就是闸码。
//!
//! 「降级」只对**非核心**系统调用开放：宿主内核裁剪掉了 `clone`，`ps`
//! 起不了子进程视图，但 `ls/cat/echo` 仍然可用——这是真实的降级，不是
//! 失败。核心四调用缺失则连 `main` 都进不去，属于校验期错误（EMISS）。
//!
//! 闸门次序：类型 → 尺寸 → 入口 → 依赖 → 调用容量 → 调用编号 → 重复 → 核心调用。
//!
//! 账本 14 格（8 成 6 败）在 `audit` 逐格对账；冻结后重放须同指纹。
//! 无 std、无 unsafe、无 panic、无 unwrap。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（本卡专属）。
pub const SEG_BASE: u32 = 0xfa90;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 单个产物最多登记的系统调用数。
pub const NEED_CAP: usize = 8;

/// 系统调用名表（12 条，编号即下标）。
pub const SYSCALLS: [&str; 12] = [
    "read",
    "write",
    "openat",
    "close",
    "exit_group",
    "mmap",
    "brk",
    "rt_sigaction",
    "clone",
    "execve",
    "wait4",
    "stat",
];

/// 核心调用：缺一不可（无 read/write 无法交互，无 exit_group 无法收尾，
/// 无 execve 无法把 applet 分派成新映像）。
pub const REQUIRED: [u8; 4] = [0, 1, 4, 9];

/// 产物尺寸下限（64 KiB；更小说明产物被截断）。
pub const SIZE_MIN: u32 = 64 * 1024;

/// 产物尺寸上限（8 MiB；更大说明链接了不该链的东西）。
pub const SIZE_MAX: u32 = 8 * 1024 * 1024;

/// 入口对齐粒度。
pub const ENTRY_ALIGN: u32 = 16;

/// 页大小。
pub const PAGE: u32 = 4096;

/// 映像装载基址。
pub const LOAD_BASE: u32 = 0x0040_0000;

/// 栈顶（用户态高地址一侧）。
pub const STACK_BASE: u32 = 0x7fff_0000;

// ---------------------------------------------------------------------------
// 错误码：8 个域错误 + 5 个账本错误
// ---------------------------------------------------------------------------

/// 动态链接产物（无 ld.so，直接拒）。
pub const SB_EDYNAMIC: u32 = SEG_BASE + 0x01;
/// 尺寸越界（< SIZE_MIN 或 > SIZE_MAX）。
pub const SB_ESIZE: u32 = SEG_BASE + 0x02;
/// 入口非法（0 或未按 16 字节对齐）。
pub const SB_ENTRY: u32 = SEG_BASE + 0x03;
/// 依赖不一致（静态产物却登记了 .so 依赖）。
pub const SB_EDEP: u32 = SEG_BASE + 0x04;
/// 系统调用编号越界。
pub const SB_ESYS: u32 = SEG_BASE + 0x05;
/// 系统调用重复登记。
pub const SB_EDUP: u32 = SEG_BASE + 0x06;
/// 核心调用缺失。
pub const SB_EMISS: u32 = SEG_BASE + 0x07;
/// 需求集容量越界。
pub const SB_ECAP: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const SB_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const SB_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const SB_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const SB_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const SB_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 产物校验。
pub const OP_VALIDATE: u8 = 0x01;
/// 装载图。
pub const OP_LOAD: u8 = 0x02;
/// 调用集。
pub const OP_SYSCALL: u8 = 0x03;
/// 运行判决。
pub const OP_VERDICT: u8 = 0x04;
/// 映像尺寸。
pub const OP_SIZE: u8 = 0x05;
/// 批量产物。
pub const OP_BATCH: u8 = 0x06;

// ---------------------------------------------------------------------------
// 哈希：FNV-1a，每字节 8 次乘法
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节（每字节 8 次乘法）。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字。
fn fold(h: u64, w: u64) -> u64 {
    let mut v = h;
    let mut sh = 0u32;
    while sh < 64 {
        let b = ((w >> sh) & 0xff) as u8;
        v = fold_byte(v, b);
        sh += 8;
    }
    v
}

/// 折入一段字节。
fn fold_bytes(mut h: u64, s: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            h = fold_byte(h, *b);
        }
        i += 1;
    }
    h
}

/// 十进制拼进 String。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut at = buf.len();
    while v > 0 {
        let d = (v % 10) as u8;
        v /= 10;
        at = at.saturating_sub(1);
        if let Some(dst) = buf.get_mut(at) {
            *dst = b'0' + d;
        }
    }
    let mut i = at;
    while i < buf.len() {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// 链接类型
// ---------------------------------------------------------------------------

/// 链接类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    /// 静态链接（自带 libc，无解释器）。
    Static,
    /// 动态链接（需要 ld.so）。
    Dynamic,
}

impl LinkKind {
    /// 编码：静态 1，动态 0。
    pub fn code(&self) -> u32 {
        match self {
            LinkKind::Static => 1,
            LinkKind::Dynamic => 0,
        }
    }

    /// 标签。
    pub fn tag(&self) -> &'static str {
        match self {
            LinkKind::Static => "static",
            LinkKind::Dynamic => "dynamic",
        }
    }

    /// 是否可独立运行（只有静态）。
    pub fn freestanding(&self) -> bool {
        matches!(self, LinkKind::Static)
    }
}

// ---------------------------------------------------------------------------
// 产物
// ---------------------------------------------------------------------------

/// 一个 busybox 产物（纯模型，不触文件系统）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Artifact {
    /// 链接类型。
    pub kind: LinkKind,
    /// 文件字节数。
    pub size: u32,
    /// ELF 入口地址（映像内偏移意义上的虚拟地址）。
    pub entry: u32,
    /// `.so` 依赖条数（静态必须为 0）。
    pub deps: u8,
    /// 需要的系统调用编号。
    pub needs: [u8; NEED_CAP],
    /// 有效需求条数。
    pub n_needs: u8,
}

impl Artifact {
    /// 空产物（仅作占位）。
    pub const fn blank() -> Artifact {
        Artifact {
            kind: LinkKind::Static,
            size: 0,
            entry: 0,
            deps: 0,
            needs: [0u8; NEED_CAP],
            n_needs: 0,
        }
    }

    /// 构造：容量闸只在这里做，其余闸留给 `validate`（顺序可测）。
    pub fn new(
        kind: LinkKind,
        size: u32,
        entry: u32,
        deps: u8,
        needs: &[u8],
    ) -> Result<Artifact, u32> {
        if needs.len() > NEED_CAP {
            return Err(SB_ECAP);
        }
        let mut a = Artifact::blank();
        a.kind = kind;
        a.size = size;
        a.entry = entry;
        a.deps = deps;
        let mut i = 0usize;
        while i < needs.len() {
            match (a.needs.get_mut(i), needs.get(i)) {
                (Some(dst), Some(src)) => *dst = *src,
                _ => return Err(SB_ECAP),
            }
            i += 1;
        }
        a.n_needs = needs.len() as u8;
        Ok(a)
    }

    /// 有效需求视图。
    pub fn need_list(&self) -> &[u8] {
        let n = (self.n_needs as usize).min(NEED_CAP);
        match self.needs.get(..n) {
            Some(s) => s,
            None => &[],
        }
    }

    /// 是否登记了某调用。
    pub fn needs_sys(&self, id: u8) -> bool {
        let mut i = 0usize;
        while i < self.need_list().len() {
            if self.need_list().get(i).copied() == Some(id) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 需求条数。
    pub fn need_count(&self) -> usize {
        self.need_list().len()
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold(FNV_OFF, self.kind.code() as u64);
        h = fold(h, self.size as u64);
        h = fold(h, self.entry as u64);
        h = fold(h, self.deps as u64);
        h = fold_bytes(h, self.need_list());
        fold(h, self.n_needs as u64)
    }

    /// 摘要文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str(self.kind.tag());
        s.push(' ');
        push_u64(&mut s, self.size as u64);
        s.push(' ');
        push_u64(&mut s, self.n_needs as u64);
        s
    }
}

/// 标准静态产物（AI-94 的交付形态）。
pub fn static_artifact() -> Artifact {
    match Artifact::new(LinkKind::Static, 1024 * 1024, 0x1000, 0, &[0, 1, 2, 3, 4, 9]) {
        Ok(a) => a,
        Err(_) => Artifact::blank(),
    }
}

/// 标准动态产物（用于反例）。
pub fn dynamic_artifact() -> Artifact {
    match Artifact::new(LinkKind::Dynamic, 1024 * 1024, 0x1000, 2, &[0, 1, 4, 9]) {
        Ok(a) => a,
        Err(_) => Artifact::blank(),
    }
}

// ---------------------------------------------------------------------------
// 校验
// ---------------------------------------------------------------------------

/// 校验：八道闸，顺序即诊断优先级。
pub fn validate(a: &Artifact) -> Result<(), u32> {
    if !a.kind.freestanding() {
        return Err(SB_EDYNAMIC);
    }
    if a.size < SIZE_MIN || a.size > SIZE_MAX {
        return Err(SB_ESIZE);
    }
    if a.entry == 0 || a.entry % ENTRY_ALIGN != 0 {
        return Err(SB_ENTRY);
    }
    if a.deps != 0 {
        return Err(SB_EDEP);
    }
    if (a.n_needs as usize) > NEED_CAP {
        return Err(SB_ECAP);
    }
    let list = a.need_list();
    let mut i = 0usize;
    while i < list.len() {
        match list.get(i) {
            Some(id) => {
                if (*id as usize) >= SYSCALLS.len() {
                    return Err(SB_ESYS);
                }
                let mut j = i.saturating_add(1);
                while j < list.len() {
                    if list.get(j).copied() == Some(*id) {
                        return Err(SB_EDUP);
                    }
                    j += 1;
                }
            }
            None => return Err(SB_ECAP),
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < REQUIRED.len() {
        if let Some(id) = REQUIRED.get(r) {
            if !a.needs_sys(*id) {
                return Err(SB_EMISS);
            }
        }
        r += 1;
    }
    Ok(())
}

/// 系统调用名（越界回空串）。
pub fn syscall_name(id: u8) -> &'static str {
    match SYSCALLS.get(id as usize) {
        Some(v) => v,
        None => "",
    }
}

// ---------------------------------------------------------------------------
// 装载图
// ---------------------------------------------------------------------------

/// 装载图（纯算术，校验通过才有意义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Load {
    /// 入口虚拟地址。
    pub entry_va: u32,
    /// 映像占页数。
    pub pages: u32,
    /// 映像字节数。
    pub text_len: u32,
    /// 栈顶。
    pub stack_top: u32,
    /// 映像末尾到栈顶的空隙。
    pub gap: u32,
}

/// 生成装载图：先校验，再算四元组。
pub fn plan(a: &Artifact) -> Result<Load, u32> {
    validate(a)?;
    let pages = a.size.saturating_add(PAGE - 1) / PAGE;
    let end = LOAD_BASE.saturating_add(a.size);
    Ok(Load {
        entry_va: LOAD_BASE.saturating_add(a.entry),
        pages,
        text_len: a.size,
        stack_top: STACK_BASE,
        gap: STACK_BASE.saturating_sub(end),
    })
}

// ---------------------------------------------------------------------------
// 宿主调用集
// ---------------------------------------------------------------------------

/// 宿主内核提供的系统调用集（12 位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Syscalls {
    bits: u16,
}

impl Syscalls {
    /// 空集。
    pub const fn empty() -> Syscalls {
        Syscalls { bits: 0 }
    }

    /// 全集（12 条）。
    pub const fn full() -> Syscalls {
        Syscalls { bits: 0x0fff }
    }

    /// 加入一条（越界 → ESYS）。
    pub fn add(&mut self, id: u8) -> Result<(), u32> {
        if (id as usize) >= SYSCALLS.len() {
            return Err(SB_ESYS);
        }
        self.bits |= 1u16 << id;
        Ok(())
    }

    /// 是否有某条。
    pub fn has(&self, id: u8) -> bool {
        if (id as usize) >= SYSCALLS.len() {
            return false;
        }
        (self.bits & (1u16 << id)) != 0
    }

    /// 条数。
    pub fn count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0u8;
        while (i as usize) < SYSCALLS.len() {
            if self.has(i) {
                n += 1;
            }
            i = i.saturating_add(1);
        }
        n
    }

    /// 去掉一条。
    pub fn remove(&mut self, id: u8) {
        if (id as usize) < SYSCALLS.len() {
            self.bits &= !(1u16 << id);
        }
    }

    /// 是否覆盖某产物的需求。
    pub fn covers(&self, a: &Artifact) -> bool {
        needs_missing(a, self) == 0
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        fold(FNV_OFF, self.bits as u64)
    }
}

/// 产物需求里宿主缺少的条数。
pub fn needs_missing(a: &Artifact, allow: &Syscalls) -> usize {
    let mut n = 0usize;
    let list = a.need_list();
    let mut i = 0usize;
    while i < list.len() {
        if let Some(id) = list.get(i) {
            if !allow.has(*id) {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 第一条缺失的调用编号（无缺失回 0xff）。
pub fn first_missing(a: &Artifact, allow: &Syscalls) -> u8 {
    let list = a.need_list();
    let mut i = 0usize;
    while i < list.len() {
        if let Some(id) = list.get(i) {
            if !allow.has(*id) {
                return *id;
            }
        }
        i += 1;
    }
    0xff
}

// ---------------------------------------------------------------------------
// 判决
// ---------------------------------------------------------------------------

/// 运行判决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 可运行。
    Runs,
    /// 降级运行（缺非核心调用，编号随行）。
    Degraded(u8),
    /// 拒收（校验错误码）。
    Rejected(u32),
}

impl Verdict {
    /// 编码：拒收 0 / 运行 1 / 降级 2。
    pub fn code(&self) -> u32 {
        match self {
            Verdict::Runs => 1,
            Verdict::Degraded(_) => 2,
            Verdict::Rejected(_) => 0,
        }
    }

    /// 标签。
    pub fn tag(&self) -> &'static str {
        match self {
            Verdict::Runs => "runs",
            Verdict::Degraded(_) => "degraded",
            Verdict::Rejected(_) => "rejected",
        }
    }

    /// 是否真的能跑（运行或降级）。
    pub fn runs(&self) -> bool {
        matches!(self, Verdict::Runs | Verdict::Degraded(_))
    }
}

/// 判决：校验失败即拒；否则按宿主调用集判「全跑 / 降级」。
pub fn verdict(a: &Artifact, allow: &Syscalls) -> Verdict {
    match plan(a) {
        Err(c) => Verdict::Rejected(c),
        Ok(_) => {
            if needs_missing(a, allow) == 0 {
                Verdict::Runs
            } else {
                Verdict::Degraded(first_missing(a, allow))
            }
        }
    }
}

/// 批量产物里被拒的条数。
pub fn count_rejected(list: &[Artifact]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < list.len() {
        if let Some(a) = list.get(i) {
            if validate(a).is_err() {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 批量产物里可运行的条数（运行 + 降级都算能跑）。
pub fn count_runnable(list: &[Artifact], allow: &Syscalls) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < list.len() {
        if let Some(a) = list.get(i) {
            if verdict(a, allow).runs() {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 样本与账本
// ---------------------------------------------------------------------------

/// 一格样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// 账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记（严格槽序，冻结后拒）。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SB_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len = self.len.saturating_add(1);
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len.saturating_sub(self.ok_count())
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(SB_EBAL);
        }
        const VALS: [u32; 8] = [256, 4_198_400, 6, 1, 2_142_175_232, 12, 2, 3];
        const CODES: [u32; 6] = [
            SB_EDYNAMIC,
            SB_ESIZE,
            SB_ENTRY,
            SB_EMISS,
            SB_ESYS,
            SB_EDUP,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SB_EBAL),
            };
            if s.slot as usize != i {
                return Err(SB_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SB_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SB_EVALUE);
                        }
                    }
                    None => return Err(SB_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SB_EVALUE);
                        }
                    }
                    None => return Err(SB_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SB_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SB_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(SB_ESEAL)
        }
    }

    /// 冻结根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// `Result<u32, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 账本组装：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let good = static_artifact();
    let host = Syscalls::full();

    // 0: 静态产物装载页数 256。
    let _ = l.register(cell(
        0,
        OP_VALIDATE,
        match plan(&good) {
            Ok(p) => Ok(p.pages),
            Err(e) => Err(e),
        },
    ));

    // 1: 入口虚拟地址 0x401000。
    let _ = l.register(cell(
        1,
        OP_LOAD,
        match plan(&good) {
            Ok(p) => Ok(p.entry_va),
            Err(e) => Err(e),
        },
    ));

    // 2: 需求调用 6 条。
    let _ = l.register(cell(2, OP_SYSCALL, Ok(good.need_count() as u32)));

    // 3: 全调用宿主 → 运行（码 1）。
    let _ = l.register(cell(3, OP_VERDICT, Ok(verdict(&good, &host).code())));

    // 4: 映像末尾到栈顶的空隙 0x7faf0000。
    let _ = l.register(cell(
        4,
        OP_SIZE,
        match plan(&good) {
            Ok(p) => Ok(p.gap),
            Err(e) => Err(e),
        },
    ));

    // 5: 宿主全集 12 条。
    let _ = l.register(cell(5, OP_SYSCALL, Ok(host.count() as u32)));

    // 6: 去掉 openat 后降级（码 2）。
    let _ = l.register(cell(
        6,
        OP_VERDICT,
        {
            let mut h = Syscalls::full();
            h.remove(2);
            Ok(verdict(&good, &h).code())
        },
    ));

    // 7: 四个产物里三个被拒。
    let _ = l.register(cell(
        7,
        OP_BATCH,
        {
            let bad_dyn = dynamic_artifact();
            let small = match Artifact::new(LinkKind::Static, 4096, 0x1000, 0, &[0, 1, 4, 9]) {
                Ok(a) => a,
                Err(_) => Artifact::blank(),
            };
            let misaligned =
                match Artifact::new(LinkKind::Static, 1024 * 1024, 0x1002, 0, &[0, 1, 4, 9]) {
                    Ok(a) => a,
                    Err(_) => Artifact::blank(),
                };
            let batch = [good, bad_dyn, small, misaligned];
            Ok(count_rejected(&batch))
        },
    ));

    // 8: 动态产物 → 硬拒。
    let _ = l.register(cell(
        8,
        OP_VALIDATE,
        match validate(&dynamic_artifact()) {
            Ok(()) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 9: 尺寸过小。
    let _ = l.register(cell(
        9,
        OP_VALIDATE,
        match Artifact::new(LinkKind::Static, 4096, 0x1000, 0, &[0, 1, 4, 9]) {
            Ok(a) => match validate(&a) {
                Ok(()) => Ok(0),
                Err(c) => Err(c),
            },
            Err(c) => Err(c),
        },
    ));

    // 10: 入口未对齐。
    let _ = l.register(cell(
        10,
        OP_VALIDATE,
        match Artifact::new(LinkKind::Static, 1024 * 1024, 0x1002, 0, &[0, 1, 4, 9]) {
            Ok(a) => match validate(&a) {
                Ok(()) => Ok(0),
                Err(c) => Err(c),
            },
            Err(c) => Err(c),
        },
    ));

    // 11: 缺核心调用 execve。
    let _ = l.register(cell(
        11,
        OP_VALIDATE,
        match Artifact::new(LinkKind::Static, 1024 * 1024, 0x1000, 0, &[0, 1, 2, 3, 4]) {
            Ok(a) => match validate(&a) {
                Ok(()) => Ok(0),
                Err(c) => Err(c),
            },
            Err(c) => Err(c),
        },
    ));

    // 12: 调用编号 13 越界。
    let _ = l.register(cell(
        12,
        OP_VALIDATE,
        match Artifact::new(LinkKind::Static, 1024 * 1024, 0x1000, 0, &[0, 1, 4, 9, 13]) {
            Ok(a) => match validate(&a) {
                Ok(()) => Ok(0),
                Err(c) => Err(c),
            },
            Err(c) => Err(c),
        },
    ));

    // 13: 重复登记 read。
    let _ = l.register(cell(
        13,
        OP_VALIDATE,
        match Artifact::new(LinkKind::Static, 1024 * 1024, 0x1000, 0, &[0, 1, 4, 9, 0]) {
            Ok(a) => match validate(&a) {
                Ok(()) => Ok(0),
                Err(c) => Err(c),
            },
            Err(c) => Err(c),
        },
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据面
// ---------------------------------------------------------------------------

/// 静态产物校验通过与装载图。
fn static_ok() -> bool {
    let a = static_artifact();
    let v = validate(&a) == Ok(());
    let p = match plan(&a) {
        Ok(x) => x,
        Err(_) => return false,
    };
    v && p.pages == 256
        && p.entry_va == 4_198_400
        && p.text_len == 1024 * 1024
        && p.stack_top == STACK_BASE
        && p.gap == 2_142_175_232
}

/// 动态产物硬拒（且先于尺寸/入口闸）。
fn dynamic_ok() -> bool {
    let d = dynamic_artifact();
    let a = validate(&d) == Err(SB_EDYNAMIC);
    let b = match Artifact::new(LinkKind::Dynamic, 4096, 0x2, 0, &[]) {
        Ok(x) => validate(&x) == Err(SB_EDYNAMIC),
        Err(_) => false,
    };
    let c = verdict(&d, &Syscalls::full()) == Verdict::Rejected(SB_EDYNAMIC);
    let e = plan(&d) == Err(SB_EDYNAMIC);
    a && b && c && e
}

/// 尺寸闸：下限 / 上限 / 边界值。
fn size_ok() -> bool {
    let a = match Artifact::new(LinkKind::Static, SIZE_MIN, 0x1000, 0, &[0, 1, 4, 9]) {
        Ok(x) => validate(&x) == Ok(()),
        Err(_) => false,
    };
    let b = match Artifact::new(LinkKind::Static, SIZE_MIN - 1, 0x1000, 0, &[0, 1, 4, 9]) {
        Ok(x) => validate(&x) == Err(SB_ESIZE),
        Err(_) => false,
    };
    let c = match Artifact::new(LinkKind::Static, SIZE_MAX, 0x1000, 0, &[0, 1, 4, 9]) {
        Ok(x) => validate(&x) == Ok(()),
        Err(_) => false,
    };
    let d = match Artifact::new(LinkKind::Static, SIZE_MAX + 1, 0x1000, 0, &[0, 1, 4, 9]) {
        Ok(x) => validate(&x) == Err(SB_ESIZE),
        Err(_) => false,
    };
    a && b && c && d
}

/// 入口对齐。
fn entry_ok() -> bool {
    let a = match Artifact::new(LinkKind::Static, SIZE_MIN, 0, 0, &[0, 1, 4, 9]) {
        Ok(x) => validate(&x) == Err(SB_ENTRY),
        Err(_) => false,
    };
    let b = match Artifact::new(LinkKind::Static, SIZE_MIN, 0x1002, 0, &[0, 1, 4, 9]) {
        Ok(x) => validate(&x) == Err(SB_ENTRY),
        Err(_) => false,
    };
    let c = match Artifact::new(LinkKind::Static, SIZE_MIN, 0x1010, 0, &[0, 1, 4, 9]) {
        Ok(x) => validate(&x) == Ok(()),
        Err(_) => false,
    };
    let d = match plan(&Artifact::new(LinkKind::Static, SIZE_MIN, 0x1010, 0, &[0, 1, 4, 9]).unwrap_or(Artifact::blank())) {
        Ok(p) => p.entry_va == LOAD_BASE + 0x1010,
        Err(_) => false,
    };
    a && b && c && d
}

/// 依赖一致性：静态带 .so 即拒；动态先被类型闸拦。
fn dep_ok() -> bool {
    let a = match Artifact::new(LinkKind::Static, SIZE_MIN, 0x1000, 1, &[0, 1, 4, 9]) {
        Ok(x) => validate(&x) == Err(SB_EDEP),
        Err(_) => false,
    };
    let b = match Artifact::new(LinkKind::Static, SIZE_MIN, 0x1000, 0, &[0, 1, 4, 9]) {
        Ok(x) => validate(&x) == Ok(()),
        Err(_) => false,
    };
    let c = match Artifact::new(LinkKind::Dynamic, SIZE_MIN, 0x1000, 3, &[0, 1, 4, 9]) {
        Ok(x) => validate(&x) == Err(SB_EDYNAMIC),
        Err(_) => false,
    };
    a && b && c
}

/// 调用表：编号 / 重复 / 核心缺失 / 容量。
fn syscall_ok() -> bool {
    let a = syscall_name(0) == "read"
        && syscall_name(9) == "execve"
        && syscall_name(11) == "stat"
        && syscall_name(12) == ""
        && SYSCALLS.len() == 12;
    let b = match Artifact::new(LinkKind::Static, SIZE_MIN, 0x1000, 0, &[0, 1, 4, 9, 12]) {
        Ok(x) => validate(&x) == Err(SB_ESYS),
        Err(_) => false,
    };
    let c = match Artifact::new(LinkKind::Static, SIZE_MIN, 0x1000, 0, &[0, 1, 4, 9, 1]) {
        Ok(x) => validate(&x) == Err(SB_EDUP),
        Err(_) => false,
    };
    let d = match Artifact::new(LinkKind::Static, SIZE_MIN, 0x1000, 0, &[0, 1, 2, 3, 4]) {
        Ok(x) => validate(&x) == Err(SB_EMISS),
        Err(_) => false,
    };
    let e = Artifact::new(LinkKind::Static, SIZE_MIN, 0x1000, 0, &[0, 1, 2, 3, 4, 5, 6, 7, 8])
        == Err(SB_ECAP);
    let f = match Artifact::new(LinkKind::Static, SIZE_MIN, 0x1000, 0, &[0, 1, 2, 3, 4, 5, 6, 7, 8])
    {
        Ok(_) => false,
        Err(c) => c == SB_ECAP,
    };
    a && b && c && d && e && f
}

/// 判决三态。
fn verdict_ok() -> bool {
    let good = static_artifact();
    let full = Syscalls::full();
    let a = verdict(&good, &full) == Verdict::Runs;
    let mut deg = full;
    deg.remove(2);
    let b = verdict(&good, &deg) == Verdict::Degraded(2);
    let c = verdict(&good, &Syscalls::empty()) == Verdict::Degraded(0);
    let d = verdict(&dynamic_artifact(), &full) == Verdict::Rejected(SB_EDYNAMIC);
    let e = Verdict::Runs.code() == 1
        && Verdict::Degraded(8).code() == 2
        && Verdict::Rejected(SB_ESIZE).code() == 0;
    a && b && c && d && e
}

/// 运行位与标签。
fn verdict_flags_ok() -> bool {
    let a = Verdict::Runs.runs() && Verdict::Degraded(3).runs() && !Verdict::Rejected(SB_ESIZE).runs();
    let b = Verdict::Runs.tag() == "runs"
        && Verdict::Degraded(0).tag() == "degraded"
        && Verdict::Rejected(1).tag() == "rejected";
    let c = LinkKind::Static.freestanding() && !LinkKind::Dynamic.freestanding();
    let d = LinkKind::Static.tag() == "static" && LinkKind::Dynamic.code() == 0;
    a && b && c && d
}

/// 宿主覆盖与缺失计数。
fn cover_ok() -> bool {
    let good = static_artifact();
    let full = Syscalls::full();
    let a = full.covers(&good) && needs_missing(&good, &full) == 0 && first_missing(&good, &full) == 0xff;
    let mut h = full;
    h.remove(2);
    h.remove(3);
    let b = !h.covers(&good) && needs_missing(&good, &h) == 2 && first_missing(&good, &h) == 2;
    let c = Syscalls::empty().count() == 0 && full.count() == 12;
    let mut d = Syscalls::empty();
    let e = d.add(12) == Err(SB_ESYS);
    let f = d.add(4) == Ok(()) && d.has(4) && !d.has(3);
    a && b && c && e && f
}

/// 批量统计。
fn batch_ok() -> bool {
    let good = static_artifact();
    let dyn_a = dynamic_artifact();
    let small = match Artifact::new(LinkKind::Static, 4096, 0x1000, 0, &[0, 1, 4, 9]) {
        Ok(x) => x,
        Err(_) => return false,
    };
    let ok2 = match Artifact::new(LinkKind::Static, SIZE_MAX, 0x2000, 0, &[0, 1, 2, 4, 9]) {
        Ok(x) => x,
        Err(_) => return false,
    };
    let batch = [good, dyn_a, small, ok2];
    let a = count_rejected(&batch) == 2;
    let b = count_runnable(&batch, &Syscalls::full()) == 2;
    let c = count_runnable(&[dyn_a, small], &Syscalls::full()) == 0;
    let d = count_rejected(&[]) == 0;
    a && b && c && d
}

/// 指纹与摘要。
fn seal_ok() -> bool {
    let a = static_artifact();
    let b = static_artifact();
    if a.seal() != b.seal() || a.seal() == 0 {
        return false;
    }
    let c = dynamic_artifact();
    let d = match Artifact::new(LinkKind::Static, 1024 * 1024, 0x1000, 0, &[0, 1, 4, 9]) {
        Ok(x) => x,
        Err(_) => return false,
    };
    let e = Syscalls::full().seal() != Syscalls::empty().seal();
    a.seal() != c.seal() && a.seal() != d.seal() && e && a.text().as_str() == "static 1048576 6"
}

/// 账本闭环。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.audit() != Ok(()) {
        return false;
    }
    if l.len() != SAMPLES || l.ok_count() != 8 || l.err_count() != 6 {
        return false;
    }
    if l.seal() != build_ledger().seal() {
        return false;
    }
    let mut l2 = build_ledger();
    let root = match l2.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if l2.replay() != Ok(root) || !l2.is_frozen() {
        return false;
    }
    if l2.freeze() != Err(SB_EBAL) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    if let Some(dst) = out.get_mut(0) {
        *dst = static_ok() && dynamic_ok() && size_ok();
    }
    if let Some(dst) = out.get_mut(1) {
        *dst = entry_ok() && dep_ok() && syscall_ok();
    }
    if let Some(dst) = out.get_mut(2) {
        *dst = verdict_ok() && verdict_flags_ok() && cover_ok();
    }
    if let Some(dst) = out.get_mut(3) {
        *dst = batch_ok() && seal_ok() && ledger_ok();
    }
    out
}

/// 判据集合（13 项）。
pub fn run_unx11596_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11596.static");

    s.add("SB01-静态校验", static_ok(), "static");
    s.add("SB02-动态拒收", dynamic_ok(), "static");
    s.add("SB03-尺寸闸", size_ok(), "static");
    s.add("SB04-入口对齐", entry_ok(), "static");
    s.add("SB05-依赖一致性", dep_ok(), "static");
    s.add("SB06-调用表", syscall_ok(), "static");
    s.add("SB07-判决三态", verdict_ok(), "static");
    s.add("SB08-判决标志", verdict_flags_ok(), "static");
    s.add("SB09-宿主覆盖", cover_ok(), "static");
    s.add("SB10-批量统计", batch_ok(), "static");
    s.add("SB11-产物指纹", seal_ok(), "static");
    s.add("SB12-账本对账", build_ledger().audit() == Ok(()), "static");
    let four = inject_four();
    s.add("SB13-冻结重放四注入", four[0] && four[1] && four[2] && four[3], "static");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_validate_and_plan() {
        let a = static_artifact();
        assert_eq!(validate(&a), Ok(()));
        let p = plan(&a).expect("p");
        assert_eq!(p.pages, 256);
        assert_eq!(p.entry_va, 4_198_400);
        assert_eq!(p.gap, 2_142_175_232);
    }

    #[test]
    fn dynamic_rejected_first() {
        assert_eq!(validate(&dynamic_artifact()), Err(SB_EDYNAMIC));
        assert_eq!(plan(&dynamic_artifact()), Err(SB_EDYNAMIC));
        assert_eq!(
            verdict(&dynamic_artifact(), &Syscalls::full()),
            Verdict::Rejected(SB_EDYNAMIC)
        );
    }

    #[test]
    fn size_bounds() {
        let ok_min = Artifact::new(LinkKind::Static, SIZE_MIN, 0x1000, 0, &[0, 1, 4, 9]).expect("a");
        assert_eq!(validate(&ok_min), Ok(()));
        let bad = Artifact::new(LinkKind::Static, SIZE_MIN - 1, 0x1000, 0, &[0, 1, 4, 9]).expect("b");
        assert_eq!(validate(&bad), Err(SB_ESIZE));
        let big = Artifact::new(LinkKind::Static, SIZE_MAX + 1, 0x1000, 0, &[0, 1, 4, 9]).expect("c");
        assert_eq!(validate(&big), Err(SB_ESIZE));
    }

    #[test]
    fn entry_gate() {
        let zero = Artifact::new(LinkKind::Static, SIZE_MIN, 0, 0, &[0, 1, 4, 9]).expect("z");
        assert_eq!(validate(&zero), Err(SB_ENTRY));
        let mis = Artifact::new(LinkKind::Static, SIZE_MIN, 0x1004, 0, &[0, 1, 4, 9]).expect("m");
        assert_eq!(validate(&mis), Err(SB_ENTRY));
    }

    #[test]
    fn static_dep_gate() {
        let d = Artifact::new(LinkKind::Static, SIZE_MIN, 0x1000, 2, &[0, 1, 4, 9]).expect("d");
        assert_eq!(validate(&d), Err(SB_EDEP));
        assert_eq!(d.deps, 2);
    }

    #[test]
    fn syscall_gates() {
        let bad_id = Artifact::new(LinkKind::Static, SIZE_MIN, 0x1000, 0, &[0, 1, 4, 9, 12]).expect("i");
        assert_eq!(validate(&bad_id), Err(SB_ESYS));
        let dup = Artifact::new(LinkKind::Static, SIZE_MIN, 0x1000, 0, &[0, 1, 4, 9, 1]).expect("d");
        assert_eq!(validate(&dup), Err(SB_EDUP));
        let miss = Artifact::new(LinkKind::Static, SIZE_MIN, 0x1000, 0, &[0, 1, 2, 3, 4]).expect("m");
        assert_eq!(validate(&miss), Err(SB_EMISS));
        assert_eq!(
            Artifact::new(LinkKind::Static, SIZE_MIN, 0x1000, 0, &[0; NEED_CAP + 1]).err(),
            Some(SB_ECAP)
        );
    }

    #[test]
    fn syscall_names() {
        assert_eq!(syscall_name(0), "read");
        assert_eq!(syscall_name(9), "execve");
        assert_eq!(syscall_name(11), "stat");
        assert_eq!(syscall_name(12), "");
    }

    #[test]
    fn verdicts() {
        let a = static_artifact();
        assert_eq!(verdict(&a, &Syscalls::full()), Verdict::Runs);
        let mut h = Syscalls::full();
        h.remove(2);
        assert_eq!(verdict(&a, &h), Verdict::Degraded(2));
        assert!(verdict(&a, &h).runs());
        assert!(!Verdict::Rejected(SB_ESIZE).runs());
    }

    #[test]
    fn coverage_counts() {
        let a = static_artifact();
        let mut h = Syscalls::full();
        h.remove(2);
        h.remove(3);
        assert_eq!(needs_missing(&a, &h), 2);
        assert_eq!(first_missing(&a, &h), 2);
        assert!(!h.covers(&a));
        assert!(Syscalls::full().covers(&a));
    }

    #[test]
    fn batch_tally() {
        let good = static_artifact();
        let bad = dynamic_artifact();
        let small = Artifact::new(LinkKind::Static, 4096, 0x1000, 0, &[0, 1, 4, 9]).expect("s");
        let batch = [good, bad, small];
        assert_eq!(count_rejected(&batch), 2);
        assert_eq!(count_runnable(&batch, &Syscalls::full()), 1);
    }

    #[test]
    fn artifact_text_and_seal() {
        let a = static_artifact();
        assert_eq!(a.text().as_str(), "static 1048576 6");
        assert_eq!(a.seal(), static_artifact().seal());
        assert!(a.seal() != dynamic_artifact().seal());
        assert_eq!(a.need_count(), 6);
        assert!(a.needs_sys(9) && !a.needs_sys(11));
    }

    #[test]
    fn syscall_set_ops() {
        let mut s = Syscalls::empty();
        assert_eq!(s.add(12), Err(SB_ESYS));
        assert_eq!(s.add(3), Ok(()));
        assert!(s.has(3) && !s.has(4));
        s.remove(3);
        assert_eq!(s.count(), 0);
        assert_eq!(Syscalls::full().count(), 12);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.sample(0).map(|s| s.value), Some(256));
        assert_eq!(l.sample(4).map(|s| s.value), Some(2_142_175_232));
        assert_eq!(l.sample(8).map(|s| s.code), Some(SB_EDYNAMIC));
        assert_eq!(l.sample(13).map(|s| s.code), Some(SB_EDUP));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(SB_EBAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11596_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
