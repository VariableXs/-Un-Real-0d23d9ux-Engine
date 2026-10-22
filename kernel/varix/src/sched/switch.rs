//! F077 · 硬件上下文切换（执行层）。
//!
//! # 这一层为什么必须存在
//!
//! [`super::engine::SwitchFrame`] 与 [`super::engine::SwitchDecision`] 只回答
//! 「该切谁」。它们是纯数据与纯策略，能在宿主上跑单元测试——但**没有一条
//! 指令真正保存过寄存器**。策略再完整，机器也不会因此换一个执行流。
//!
//! 本模块把决策变成机器动作，共四步：
//!
//! 1. 保存被切出线程的 callee-saved 现场（`rbx/rbp/r12-r15`）与续跑点
//!    （`rsp/rip`）；
//! 2. 目标线程声明了地址空间（`cr3 != 0`）且与当前不同，才写 CR3；
//! 3. 目标线程声明了 `fs/gs` 基址，才写对应 MSR；
//! 4. 恢复目标现场，从它的 `rip` 续跑。
//!
//! # 三条不变量
//!
//! - **帧是唯一真相**：切出去时写进 `from`，切回来时从 `to` 读。不存在
//!   「寄存器里还有一份」的隐式状态。
//! - **零值即「沿用」**：`cr3 == 0` / `fs_base == 0` / `gs_base == 0` 表示
//!   「不要动」。这条约定让尚未建立地址空间的线程（当前全部线程）可以安全
//!   地共用引导页表，而不会把 CR3 写成 0 而三重故障。
//! - **先体检后落刀**：[`vet_switch`] 在触碰任何寄存器前拒绝五类现场
//!   （自切、不可恢复、栈未对齐、CR3 未对齐、rsp 出栈）。拒绝是显性记账，
//!   不是静默跳过。
//!
//! # 中断上下文里的切换
//!
//! 从 tick 处理里切换是合法的：被切出的现场落在中断栈上，切回时从
//! `arch_switch` 的返回点继续，中断处理照常走完 `iretq`。前提是**所有**
//! 线程都在内核态运行——当前成立。

use core::sync::atomic::{AtomicU64, Ordering};

use super::engine::{SwitchDecision, SwitchFrame, MAX_THREADS};
use super::kstack::KSTACK_BYTES;

// ---------------------------------------------------------------------------
// §1 布局契约
// ---------------------------------------------------------------------------

/// 帧里必须被搬运的字数。与 `SwitchFrame::WORDS` 同源。
pub const FRAME_WORDS: usize = SwitchFrame::WORDS;
/// 帧字节数。汇编里的偏移量全部据此推导。
pub const FRAME_BYTES: usize = FRAME_WORDS * 8;

// 汇编硬编码了字段偏移；这里把「硬编码」与「结构体定义」焊死。任何一次
// 字段增删或重排都会在编译期炸掉，而不是在真机上表现为随机跳飞。
const _: () = {
    assert!(core::mem::offset_of!(SwitchFrame, rsp) == 0);
    assert!(core::mem::offset_of!(SwitchFrame, rip) == 8);
    assert!(core::mem::offset_of!(SwitchFrame, rbx) == 16);
    assert!(core::mem::offset_of!(SwitchFrame, rbp) == 24);
    assert!(core::mem::offset_of!(SwitchFrame, r12) == 32);
    assert!(core::mem::offset_of!(SwitchFrame, r13) == 40);
    assert!(core::mem::offset_of!(SwitchFrame, r14) == 48);
    assert!(core::mem::offset_of!(SwitchFrame, r15) == 56);
    assert!(core::mem::offset_of!(SwitchFrame, cr3) == 64);
    assert!(core::mem::offset_of!(SwitchFrame, fs_base) == 72);
    assert!(core::mem::offset_of!(SwitchFrame, gs_base) == 80);
    assert!(core::mem::size_of::<SwitchFrame>() == 88);
};

/// CR3 必须页对齐；低于第一页的值不可能是页表根。
pub const MIN_CR3: u64 = 0x1000;

/// 栈指针只需 8 字节对齐：已运行线程的 `rsp` 落在 16 字节边界上，而新建
/// 线程为了让入口函数看到 SysV 的「call 之后」形态，故意停在 `16n + 8`。
/// 两种形态都合法，所以门控只查 8 字节。
pub const RSP_ALIGN: u64 = 8;

// ---------------------------------------------------------------------------
// §2 拒绝原因
// ---------------------------------------------------------------------------

/// 为什么不能切。每一类都能在日志里区分，便于归因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// 目标就是当前线程。
    SelfSwitch,
    /// 目标帧 `rsp` 或 `rip` 为 0：从未保存过现场。
    NotResumable,
    /// `rsp` 不是 8 字节对齐。
    RspMisaligned,
    /// `cr3` 非零但不是页对齐。
    Cr3Misaligned,
    /// `rsp` 落在目标栈区间之外。
    OutsideStack,
}

pub const REFUSAL_COUNT: usize = 5;

impl Refusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Refusal::SelfSwitch => "self-switch",
            Refusal::NotResumable => "not-resumable",
            Refusal::RspMisaligned => "rsp-misaligned",
            Refusal::Cr3Misaligned => "cr3-misaligned",
            Refusal::OutsideStack => "outside-stack",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Refusal::SelfSwitch => 0,
            Refusal::NotResumable => 1,
            Refusal::RspMisaligned => 2,
            Refusal::Cr3Misaligned => 3,
            Refusal::OutsideStack => 4,
        }
    }
}

// ---------------------------------------------------------------------------
// §3 现场体检
// ---------------------------------------------------------------------------

/// 切换前体检。`stack_top == 0` 表示该线程尚未登记栈，此时跳过区间检查
/// ——「不知道」不等于「越界」，不能因为账本没建好就拒绝调度。
pub fn vet_switch(
    from_tid: u32,
    to_tid: u32,
    to: &SwitchFrame,
    stack_top: u64,
) -> Result<(), Refusal> {
    if from_tid == to_tid {
        return Err(Refusal::SelfSwitch);
    }
    if !to.resumable() {
        return Err(Refusal::NotResumable);
    }
    if to.rsp & (RSP_ALIGN - 1) != 0 {
        return Err(Refusal::RspMisaligned);
    }
    if to.cr3 != 0 && (to.cr3 & 0xFFF != 0 || to.cr3 < MIN_CR3) {
        return Err(Refusal::Cr3Misaligned);
    }
    if stack_top != 0 {
        let low = stack_top.saturating_sub(KSTACK_BYTES);
        if to.rsp >= stack_top || to.rsp < low {
            return Err(Refusal::OutsideStack);
        }
    }
    Ok(())
}

/// 新建线程的现场规格。
#[derive(Clone, Copy, Debug)]
pub struct NewThreadSpec {
    /// 栈顶（最高地址），必须 16 字节对齐。
    pub stack_top: u64,
    /// 入口地址。
    pub entry: u64,
    /// 入口返回后跳去的收尾函数。
    pub exit: u64,
    /// 地址空间；0 表示沿用当前。
    pub cr3: u64,
    /// 是否把 `exit` 写进目标栈。宿主测试传 `false`（栈地址不可写）。
    pub paint: bool,
}

/// 把一条新线程的「第一次被切进来」布置成一次普通函数调用。
///
/// 入口函数看到的栈必须符合 SysV：`rsp % 16 == 8`，且栈顶是返回地址。
/// 于是 `rsp = stack_top - 8`、`[rsp] = exit`、`rip = entry`——入口一旦
/// `ret`，就落到 `exit` 上，不会把控制流掉进未映射区。
pub fn prepare_new_thread(spec: &NewThreadSpec) -> Option<SwitchFrame> {
    if spec.entry == 0 || spec.exit == 0 {
        return None;
    }
    if spec.stack_top < KSTACK_BYTES || spec.stack_top & 0xF != 0 {
        return None;
    }
    if spec.cr3 != 0 && (spec.cr3 & 0xFFF != 0 || spec.cr3 < MIN_CR3) {
        return None;
    }
    let sp = spec.stack_top - 8;
    if spec.paint {
        // SAFETY: 契约要求 stack_top 是一条已分配内核栈的栈顶，sp 落在栈内。
        unsafe {
            core::ptr::write_volatile(sp as *mut u64, spec.exit);
        }
    }
    Some(SwitchFrame {
        rsp: sp,
        rip: spec.entry,
        cr3: spec.cr3,
        ..SwitchFrame::default()
    })
}

// ---------------------------------------------------------------------------
// §4 硬件切换
// ---------------------------------------------------------------------------

/// 本平台是否具备真实切换能力。宿主侧恒为 `false`——不伪装。
pub const fn hardware_available() -> bool {
    cfg!(all(target_arch = "x86_64", target_os = "none"))
}

/// `arch_switch(from, to)`：保存 `from`、恢复 `to`。
///
/// 汇编约定与 [`SwitchFrame`] 的 `repr(C)` 布局一一对应（§1 已编译期焊死）。
/// 恢复用 `jmp` 而非 `ret`：帧里保存的 `rsp` 是**返回地址已被消费之后**的
/// 值，`rip` 是返回地址本身。这样续跑点由帧唯一决定，不依赖栈顶内容。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
#[unsafe(naked)]
pub unsafe extern "C" fn arch_switch(from: *mut SwitchFrame, to: *const SwitchFrame) {
    core::arch::naked_asm!(
        // ---- 保存现场：rdi = from, rsi = to ----
        "mov rax, [rsp]",
        "mov [rdi + 8], rax",   // from.rip = 返回地址
        "lea rax, [rsp + 8]",
        "mov [rdi + 0], rax",   // from.rsp = 返回地址之后
        "mov [rdi + 16], rbx",
        "mov [rdi + 24], rbp",
        "mov [rdi + 32], r12",
        "mov [rdi + 40], r13",
        "mov [rdi + 48], r14",
        "mov [rdi + 56], r15",
        // ---- 地址空间：cr3 为零即沿用 ----
        "mov rax, [rsi + 64]",
        "test rax, rax",
        "jz 2f",
        "mov rcx, cr3",
        "cmp rax, rcx",
        "je 2f",
        "mov cr3, rax",
        "2:",
        // ---- fs 基址：IA32_FS_BASE = 0xC0000100 ----
        "mov rax, [rsi + 72]",
        "test rax, rax",
        "jz 3f",
        "mov rdx, rax",
        "shr rdx, 32",
        "mov ecx, 0xC0000100",
        "wrmsr",
        "3:",
        // ---- gs 基址：IA32_GS_BASE = 0xC0000101 ----
        "mov rax, [rsi + 80]",
        "test rax, rax",
        "jz 4f",
        "mov rdx, rax",
        "shr rdx, 32",
        "mov ecx, 0xC0000101",
        "wrmsr",
        "4:",
        // ---- 恢复现场 ----
        "mov rbx, [rsi + 16]",
        "mov rbp, [rsi + 24]",
        "mov r12, [rsi + 32]",
        "mov r13, [rsi + 40]",
        "mov r14, [rsi + 48]",
        "mov r15, [rsi + 56]",
        "mov rsp, [rsi + 0]",
        "jmp qword ptr [rsi + 8]",
    )
}

// ---------------------------------------------------------------------------
// §4.5 现场表
// ---------------------------------------------------------------------------

/// 每线程一份保存现场。
///
/// **只有 `ready` 为真的槽才允许参与真实切换**：新线程必须先经
/// [`prepare_new_thread`] 布置现场；已在跑的线程由它第一次被切出时写入。
/// 未就绪的槽不参与统计——「还没布置」不是「切换失败」，混在一起会让
/// 守恒式失去诊断价值。
pub struct FrameTable {
    frames: [SwitchFrame; MAX_THREADS],
    ready: [bool; MAX_THREADS],
    /// 该线程的栈顶，供门控判区间；0 表示未登记。
    tops: [u64; MAX_THREADS],
}

impl FrameTable {
    pub const fn new() -> FrameTable {
        FrameTable {
            frames: [SwitchFrame {
                rsp: 0,
                rip: 0,
                rbx: 0,
                rbp: 0,
                r12: 0,
                r13: 0,
                r14: 0,
                r15: 0,
                cr3: 0,
                fs_base: 0,
                gs_base: 0,
            }; MAX_THREADS],
            ready: [false; MAX_THREADS],
            tops: [0; MAX_THREADS],
        }
    }

    /// 登记一条线程的初始现场。重复登记覆盖旧值（线程重建）。
    pub fn install(&mut self, tid: u32, frame: SwitchFrame, stack_top: u64) -> bool {
        if tid as usize >= MAX_THREADS {
            return false;
        }
        self.frames[tid as usize] = frame;
        self.tops[tid as usize] = stack_top;
        self.ready[tid as usize] = frame.resumable();
        self.ready[tid as usize]
    }

    /// 撤销登记（线程退出后调用），防止野帧被切进来。
    pub fn retire(&mut self, tid: u32) -> bool {
        if tid as usize >= MAX_THREADS {
            return false;
        }
        let had = self.ready[tid as usize];
        self.frames[tid as usize] = SwitchFrame::default();
        self.tops[tid as usize] = 0;
        self.ready[tid as usize] = false;
        had
    }

    pub fn is_ready(&self, tid: u32) -> bool {
        (tid as usize) < MAX_THREADS && self.ready[tid as usize]
    }

    pub fn frame(&self, tid: u32) -> Option<&SwitchFrame> {
        if self.is_ready(tid) {
            Some(&self.frames[tid as usize])
        } else {
            None
        }
    }

    pub fn stack_top(&self, tid: u32) -> u64 {
        if (tid as usize) < MAX_THREADS {
            self.tops[tid as usize]
        } else {
            0
        }
    }

    /// 供切换路径取指针。调用者必须保证在指针使用期间该槽不被 `install`
    /// 或 `retire` 改动——切换路径的用法是「取指针 → 放锁 → 立刻切」。
    pub fn frame_ptr(&self, tid: u32) -> *const SwitchFrame {
        if !self.is_ready(tid) {
            return core::ptr::null();
        }
        core::ptr::addr_of!(self.frames[tid as usize])
    }

    pub fn frame_ptr_mut(&mut self, tid: u32) -> *mut SwitchFrame {
        if !self.is_ready(tid) {
            return core::ptr::null_mut();
        }
        core::ptr::addr_of_mut!(self.frames[tid as usize])
    }

    pub fn ready_count(&self) -> usize {
        self.ready.iter().filter(|r| **r).count()
    }
}

impl Default for FrameTable {
    fn default() -> FrameTable {
        FrameTable::new()
    }
}

/// 一次派发的结局。四值封闭，不让「没切」与「切了」混为一谈。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispatchOutcome {
    /// 目标或来源尚未登记现场——不是失败，只是还轮不到硬件层。
    NotReady,
    /// 体检拒绝。
    Refused(Refusal),
    /// 真机完成了切换。
    Switched,
    /// 宿主：体检通过但无硬件可切。
    HostSoft,
}

impl DispatchOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            DispatchOutcome::NotReady => "not-ready",
            DispatchOutcome::Refused(_) => "refused",
            DispatchOutcome::Switched => "switched",
            DispatchOutcome::HostSoft => "host-soft",
        }
    }
}

// ---------------------------------------------------------------------------
// §5 计数
// ---------------------------------------------------------------------------

/// 切换计数。`attempted == committed + soft + Σrefused` 是模块的守恒式，
/// 判据逐项核对。
pub struct SwitchCounters {
    pub attempted: AtomicU64,
    pub committed: AtomicU64,
    /// 宿主上通过了体检但无法真实切换的次数。
    pub soft: AtomicU64,
    pub refused: [AtomicU64; REFUSAL_COUNT],
}

impl SwitchCounters {
    pub const fn new() -> SwitchCounters {
        SwitchCounters {
            attempted: AtomicU64::new(0),
            committed: AtomicU64::new(0),
            soft: AtomicU64::new(0),
            refused: [const { AtomicU64::new(0) }; REFUSAL_COUNT],
        }
    }

    pub fn refused_at(&self, r: Refusal) -> u64 {
        self.refused[r.index()].load(Ordering::Relaxed)
    }

    pub fn refused_total(&self) -> u64 {
        let mut n = 0;
        for slot in self.refused.iter() {
            n += slot.load(Ordering::Relaxed);
        }
        n
    }

    /// 守恒式：每一次尝试都有一个归宿。
    pub fn balanced(&self) -> bool {
        self.attempted.load(Ordering::Relaxed)
            == self.committed.load(Ordering::Relaxed)
                + self.soft.load(Ordering::Relaxed)
                + self.refused_total()
    }
}

impl Default for SwitchCounters {
    fn default() -> SwitchCounters {
        SwitchCounters::new()
    }
}

static COUNTERS: SwitchCounters = SwitchCounters::new();

pub fn counters() -> &'static SwitchCounters {
    &COUNTERS
}

// ---------------------------------------------------------------------------
// §6 提交
// ---------------------------------------------------------------------------

/// 体检后执行切换。
///
/// # Safety
///
/// - `from` 必须指向当前线程**独占**的帧槽，且在本线程下一次被调度前不会
///   被他人写入；
/// - `to` 必须指向目标线程的帧槽，且目标帧在切换期间保持不变；
/// - 调用者必须保证目标栈已映射、可写，且目标线程不持有当前 CPU 的锁。
///
/// 宿主上不会执行任何寄存器操作：体检通过后记一次 `soft` 并返回。这不是
/// 「假装成功」——返回值与计数器都如实标注了「未真实切换」。
pub unsafe fn try_switch(
    from_tid: u32,
    to_tid: u32,
    from: *mut SwitchFrame,
    to: *const SwitchFrame,
    stack_top: u64,
) -> Result<(), Refusal> {
    let c = counters();
    c.attempted.fetch_add(1, Ordering::Relaxed);
    if from.is_null() || to.is_null() {
        c.refused[Refusal::NotResumable.index()].fetch_add(1, Ordering::Relaxed);
        return Err(Refusal::NotResumable);
    }
    let target = &*to;
    if let Err(r) = vet_switch(from_tid, to_tid, target, stack_top) {
        c.refused[r.index()].fetch_add(1, Ordering::Relaxed);
        return Err(r);
    }
    if !hardware_available() {
        c.soft.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }
    // 先记账再落刀：`arch_switch` 在目标线程里返回，本行之后的代码要等到
    // 本线程再次被切回来才执行。
    c.committed.fetch_add(1, Ordering::Relaxed);
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        arch_switch(from, to);
    }
    Ok(())
}

/// 当前 CR3（仅真机可读；宿主返回 0）。
pub fn current_cr3() -> u64 {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let v: u64;
        // SAFETY: 读控制寄存器无副作用。
        unsafe {
            core::arch::asm!("mov {}, cr3", out(reg) v, options(nomem, nostack, preserves_flags));
        }
        v & !0xFFF
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        0
    }
}

/// 装载地址空间。仅供未来的真实页表构造器调用；`cr3` 非法时拒绝。
pub fn install_address_space(cr3: u64) -> bool {
    if cr3 == 0 || cr3 & 0xFFF != 0 || cr3 < MIN_CR3 {
        return false;
    }
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        // SAFETY: cr3 已校验页对齐；调用者保证新页表映射了当前指令与栈。
        unsafe {
            core::arch::asm!("mov cr3, {}", in(reg) cr3, options(nostack, preserves_flags));
        }
        true
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        false
    }
}

/// 全局现场表。调度器在 `pick` 出决策后、**释放调度器锁**再调 [`dispatch`]。
static FRAMES: crate::cpu::sync::SpinProtected<FrameTable> =
    crate::cpu::sync::SpinProtected::new(FrameTable::new());

pub fn frames() -> &'static crate::cpu::sync::SpinProtected<FrameTable> {
    &FRAMES
}

/// 把一次调度决策交给硬件层。
///
/// # 为什么必须「取指针 → 放锁 → 切」
///
/// 若在持有现场表锁（更不用说调度器锁）时切换，被切出的线程会把锁一起带
/// 走；目标线程一旦尝试取同一把锁就是永久自旋。因此这里只做三件事：在锁
/// 内取两个裸指针与栈顶、立刻放锁、再落刀。单核上两个指针在「放锁到落刀」
/// 之间不会被改写；SMP 下需要外层保证 from/to 不迁移，这一点由调用方
/// （持有 CPU 本地调度上下文）承担。
///
/// 注意 `from` 未就绪时直接返回 [`DispatchOutcome::NotReady`]：第一次进入
/// 新线程必须由一个已有现场的线程发起，否则没有可保存的续跑点。
pub fn dispatch(d: SwitchDecision, _cpu: u32) -> DispatchOutcome {
    let (from, to, top) = {
        let mut t = FRAMES.lock();
        if !t.is_ready(d.from) || !t.is_ready(d.to) {
            return DispatchOutcome::NotReady;
        }
        let fp = t.frame_ptr_mut(d.from);
        let tp = t.frame_ptr(d.to);
        let top = t.stack_top(d.to);
        (fp, tp, top)
    };
    if from.is_null() || to.is_null() {
        return DispatchOutcome::NotReady;
    }
    // SAFETY: 两个指针都指向现场表内已就绪的槽；表锁已释放，单核上不存在
    // 并发改写。目标栈已由 install 的调用方保证映射。
    match unsafe { try_switch(d.from, d.to, from, to, top) } {
        Ok(()) => {
            if hardware_available() {
                DispatchOutcome::Switched
            } else {
                DispatchOutcome::HostSoft
            }
        }
        Err(r) => DispatchOutcome::Refused(r),
    }
}

// ---------------------------------------------------------------------------
// §7 判据
// ---------------------------------------------------------------------------

/// 切换层的十八条不变量。
pub fn run_switch_checks() -> (usize, usize) {
    let mut c = crate::checks::CheckSet::new("switch");

    // 1. 帧字数与字节数自洽。
    if FRAME_WORDS == 11 && FRAME_BYTES == 88 {
        c.ok("SW-帧-十一字八十八字节");
    } else {
        c.fail("SW-帧-十一字八十八字节", "frame size drifted");
    }

    // 2. 布局断言虽在编译期，运行时再核一次首尾偏移（防有人绕开断言改布局）。
    if core::mem::offset_of!(SwitchFrame, rsp) == 0
        && core::mem::offset_of!(SwitchFrame, gs_base) == 80
    {
        c.ok("SW-布局-首尾偏移固定");
    } else {
        c.fail("SW-布局-首尾偏移固定", "offset drifted");
    }

    // 3. 空帧不可恢复。
    let empty = SwitchFrame::default();
    if !empty.resumable() {
        c.ok("SW-空帧-不可恢复");
    } else {
        c.fail("SW-空帧-不可恢复", "default frame claims resumable");
    }

    // 4. 新线程帧：rsp 停在 16n+8，rip 为入口。
    let spec = NewThreadSpec {
        stack_top: 0x20_0000,
        entry: 0x1000,
        exit: 0x2000,
        cr3: 0,
        paint: false,
    };
    match prepare_new_thread(&spec) {
        Some(f) if f.rsp % 16 == 8 && f.rip == 0x1000 => c.ok("SW-新线程-入口形态合规"),
        _ => c.fail("SW-新线程-入口形态合规", "entry frame wrong"),
    }

    // 5. 栈顶未对齐即拒。
    let bad_top = NewThreadSpec {
        stack_top: 0x20_0008,
        ..spec
    };
    if prepare_new_thread(&bad_top).is_none() {
        c.ok("SW-新线程-栈顶未对齐即拒");
    } else {
        c.fail("SW-新线程-栈顶未对齐即拒", "misaligned top accepted");
    }

    // 6. 入口为零即拒。
    let no_entry = NewThreadSpec { entry: 0, ..spec };
    if prepare_new_thread(&no_entry).is_none() {
        c.ok("SW-新线程-入口为零即拒");
    } else {
        c.fail("SW-新线程-入口为零即拒", "zero entry accepted");
    }

    // 7. 自切拒绝。
    let ok_frame = prepare_new_thread(&spec).unwrap();
    if vet_switch(3, 3, &ok_frame, 0) == Err(Refusal::SelfSwitch) {
        c.ok("SW-门控-自切即拒");
    } else {
        c.fail("SW-门控-自切即拒", "self switch allowed");
    }

    // 8. 不可恢复帧拒绝。
    if vet_switch(1, 2, &empty, 0) == Err(Refusal::NotResumable) {
        c.ok("SW-门控-空帧即拒");
    } else {
        c.fail("SW-门控-空帧即拒", "empty frame allowed");
    }

    // 9. rsp 未按 8 字节对齐即拒。
    let mut mis_rsp = ok_frame;
    mis_rsp.rsp |= 0x1;
    if vet_switch(1, 2, &mis_rsp, 0) == Err(Refusal::RspMisaligned) {
        c.ok("SW-门控-栈指未对齐即拒");
    } else {
        c.fail("SW-门控-栈指未对齐即拒", "misaligned rsp allowed");
    }

    // 10. CR3 非零但未页对齐即拒。
    let mut bad_cr3 = ok_frame;
    bad_cr3.cr3 = 0x1234;
    if vet_switch(1, 2, &bad_cr3, 0) == Err(Refusal::Cr3Misaligned) {
        c.ok("SW-门控-页表未对齐即拒");
    } else {
        c.fail("SW-门控-页表未对齐即拒", "misaligned cr3 allowed");
    }

    // 11. CR3 为零表示沿用，必须放行。
    let mut zero_cr3 = ok_frame;
    zero_cr3.cr3 = 0;
    if vet_switch(1, 2, &zero_cr3, 0).is_ok() {
        c.ok("SW-门控-零页表表示沿用");
    } else {
        c.fail("SW-门控-零页表表示沿用", "zero cr3 rejected");
    }

    // 12. rsp 出栈即拒。
    if vet_switch(1, 2, &ok_frame, 0x20_0000) == Err(Refusal::OutsideStack) {
        c.ok("SW-门控-栈指越界即拒");
    } else {
        c.fail("SW-门控-栈指越界即拒", "outside rsp allowed");
    }

    // 13. 栈已登记且 rsp 在区间内必须放行。`ok_frame` 的 rsp 是 0x1F_FFF8，
    //     栈是 [0x1F_C000, 0x20_0000)——落在区间内，若被拒说明判反了。
    if vet_switch(1, 2, &ok_frame, 0x20_0000).is_ok() {
        c.ok("SW-门控-区间内放行");
    } else {
        c.fail("SW-门控-区间内放行", "in-range rsp rejected");
    }

    // 14. 平台能力与目标一致：宿主不可切、内核可切。
    let expected = cfg!(all(target_arch = "x86_64", target_os = "none"));
    if hardware_available() == expected {
        c.ok("SW-平台-能力如实上报");
    } else {
        c.fail("SW-平台-能力如实上报", "capability lied");
    }

    // 15. 非法地址空间装载被拒（宿主与真机同判据）。
    if !install_address_space(0) && !install_address_space(0x1234) {
        c.ok("SW-页表-非法根即拒");
    } else {
        c.fail("SW-页表-非法根即拒", "bad cr3 installed");
    }

    // 16. 拒绝原因文本互异且非空。
    let all = [
        Refusal::SelfSwitch,
        Refusal::NotResumable,
        Refusal::RspMisaligned,
        Refusal::Cr3Misaligned,
        Refusal::OutsideStack,
    ];
    let mut distinct = true;
    for i in 0..all.len() {
        if all[i].as_str().is_empty() || all[i].index() != i {
            distinct = false;
        }
        for j in (i + 1)..all.len() {
            if all[i].as_str() == all[j].as_str() {
                distinct = false;
            }
        }
    }
    if distinct {
        c.ok("SW-归因-五类拒绝互异");
    } else {
        c.fail("SW-归因-五类拒绝互异", "refusal taxonomy broken");
    }

    // 17. 计数器守恒式在空载时成立。
    if counters().balanced() {
        c.ok("SW-计数-守恒式成立");
    } else {
        c.fail("SW-计数-守恒式成立", "counter drift");
    }

    // 18. 一次被拒的尝试必须落进守恒式（自切样本，不触碰寄存器）。
    let before = counters().refused_at(Refusal::SelfSwitch);
    let mut scratch_from = ok_frame;
    let scratch_to = ok_frame;
    // SAFETY: 两个指针都指向本地栈上的帧；自切在体检阶段即被拒，不会执行
    // 任何寄存器操作。
    let r = unsafe { try_switch(7, 7, &mut scratch_from, &scratch_to, 0) };
    if r == Err(Refusal::SelfSwitch)
        && counters().refused_at(Refusal::SelfSwitch) == before + 1
        && counters().balanced()
    {
        c.ok("SW-计数-拒绝落账且守恒");
    } else {
        c.fail("SW-计数-拒绝落账且守恒", "refusal not accounted");
    }

    // 19. 现场表：未登记即不可用；登记后可查；撤销后不可查。
    let mut ft = FrameTable::new();
    let f = SwitchFrame {
        rsp: 0x8FF8,
        rip: 0x4000,
        ..SwitchFrame::default()
    };
    let installed = ft.install(2, f, 0x9000);
    let after_install = ft.is_ready(2) && ft.frame(2).is_some() && ft.ready_count() == 1;
    let retired = ft.retire(2);
    let after_retire = !ft.is_ready(2) && ft.frame(2).is_none() && ft.ready_count() == 0;
    if installed && after_install && retired && after_retire {
        c.ok("SW-现场表-登记与撤销对称");
    } else {
        c.fail("SW-现场表-登记与撤销对称", "frame table asymmetric");
    }

    // 20. 未就绪的决策必须落 NotReady，且不污染守恒式。
    let before = counters().attempted.load(Ordering::Relaxed);
    let outcome = dispatch(
        SwitchDecision {
            from: 60,
            to: 61,
            preempted: false,
            cpu: 0,
        },
        0,
    );
    let clean = outcome == DispatchOutcome::NotReady
        && counters().attempted.load(Ordering::Relaxed) == before;
    if clean {
        c.ok("SW-派发-未就绪不入账");
    } else {
        c.fail("SW-派发-未就绪不入账", "not-ready polluted counters");
    }

    c.tally()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sched::engine::SwitchFrame;

    fn frame_at(sp: u64) -> SwitchFrame {
        SwitchFrame {
            rsp: sp,
            rip: 0x4000,
            ..SwitchFrame::default()
        }
    }

    #[test]
    fn frame_layout_is_pinned() {
        assert_eq!(core::mem::size_of::<SwitchFrame>(), 88);
        assert_eq!(FRAME_WORDS, 11);
    }

    #[test]
    fn default_frame_is_not_resumable() {
        assert!(!SwitchFrame::default().resumable());
    }

    #[test]
    fn new_thread_frame_sits_at_16n_plus_8() {
        let f = prepare_new_thread(&NewThreadSpec {
            stack_top: 0x9000,
            entry: 0x1000,
            exit: 0x2000,
            cr3: 0,
            paint: false,
        })
        .unwrap();
        assert_eq!(f.rsp, 0x8FF8);
        assert_eq!(f.rsp % 16, 8);
        assert_eq!(f.rip, 0x1000);
    }

    #[test]
    fn new_thread_rejects_small_stack() {
        assert!(prepare_new_thread(&NewThreadSpec {
            stack_top: 0x100,
            entry: 0x1000,
            exit: 0x2000,
            cr3: 0,
            paint: false,
        })
        .is_none());
    }

    #[test]
    fn vet_rejects_self_switch() {
        let f = frame_at(0x8000);
        assert_eq!(vet_switch(1, 1, &f, 0), Err(Refusal::SelfSwitch));
    }

    #[test]
    fn vet_accepts_zero_cr3() {
        let f = frame_at(0x8000);
        assert!(vet_switch(1, 2, &f, 0).is_ok());
    }

    #[test]
    fn vet_rejects_unaligned_cr3() {
        let mut f = frame_at(0x8000);
        f.cr3 = 0x2FFF;
        assert_eq!(vet_switch(1, 2, &f, 0), Err(Refusal::Cr3Misaligned));
    }

    #[test]
    fn vet_accepts_aligned_cr3() {
        let mut f = frame_at(0x8000);
        f.cr3 = 0x2000;
        assert!(vet_switch(1, 2, &f, 0).is_ok());
    }

    #[test]
    fn vet_rejects_rsp_below_stack() {
        let f = frame_at(0x8000);
        // 栈 [0x7000, 0x8000)，rsp 在 0x8000 之上。
        assert_eq!(vet_switch(1, 2, &f, 0x7000), Err(Refusal::OutsideStack));
    }

    #[test]
    fn vet_accepts_rsp_inside_stack() {
        let f = frame_at(0x8FF8);
        assert!(vet_switch(1, 2, &f, 0x9000).is_ok());
    }

    #[test]
    fn vet_rejects_misaligned_rsp() {
        let mut f = frame_at(0x8000);
        f.rsp = 0x8001;
        assert_eq!(vet_switch(1, 2, &f, 0), Err(Refusal::RspMisaligned));
    }

    #[test]
    fn refusal_texts_are_unique() {
        let all = [
            Refusal::SelfSwitch,
            Refusal::NotResumable,
            Refusal::RspMisaligned,
            Refusal::Cr3Misaligned,
            Refusal::OutsideStack,
        ];
        for i in 0..all.len() {
            assert_eq!(all[i].index(), i);
            for j in (i + 1)..all.len() {
                assert_ne!(all[i].as_str(), all[j].as_str());
            }
        }
    }

    #[test]
    fn soft_switch_on_host_is_accounted() {
        // 宿主上不允许真的切，但一次合法尝试必须记进 soft 且保持守恒。
        let mut from = frame_at(0x8000);
        let to = frame_at(0x9000);
        let before = counters().soft.load(Ordering::Relaxed);
        // SAFETY: 宿主不会执行寄存器操作；两个帧都在本地栈上。
        let r = unsafe { try_switch(1, 2, &mut from, &to, 0) };
        assert!(r.is_ok());
        if !hardware_available() {
            assert_eq!(counters().soft.load(Ordering::Relaxed), before + 1);
        }
    }

    #[test]
    fn null_pointer_is_refused() {
        let to = frame_at(0x9000);
        // SAFETY: from 为空指针时在体检阶段即被拒。
        let r = unsafe { try_switch(1, 2, core::ptr::null_mut(), &to, 0) };
        assert_eq!(r, Err(Refusal::NotResumable));
    }

    #[test]
    fn install_rejects_bad_cr3() {
        assert!(!install_address_space(0));
        assert!(!install_address_space(0xFFF));
        assert!(!install_address_space(0x800));
    }

    #[test]
    fn host_has_no_hardware_switch() {
        assert!(!hardware_available());
        assert_eq!(current_cr3(), 0);
    }

    #[test]
    fn checks_are_green() {
        let (p, f) = run_switch_checks();
        assert_eq!(f, 0, "switch checks must be green, passed={}", p);
        assert!(p >= 18);
    }
}
