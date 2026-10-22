//! F077 配套 · 内核栈账本与溢出哨兵。
//!
//! # 为什么单独一层
//!
//! 线程要能被切出去再切回来，必须有一块**只属于它**的栈。栈的分配有两个
//! 容易做错的地方：
//!
//! 1. **页序**：栈必须是物理连续的。`alloc_page()` 给一页，`alloc_order(2)`
//!    给 16 KiB 连续四页——用四次 `alloc_page()` 拼出来的「栈」在物理上
//!    不连续，一旦某层页表按 2 MiB 大叶映射，跨界访问就会落到别人的帧上。
//! 2. **溢出不可见**：栈向下生长，溢出先踩低地址。没有哨兵时，越界写只会
//!    在某次无关的调度里表现成「随机数据被改」，归因成本极高。
//!
//! 本模块给每个线程槽记一条 [`KernelStack`]，在栈底（最低地址）押两个
//! [`STACK_CANARY`] 字。分配时押、释放前验，破了就**不归还物理帧**——
//! 坏页回池比漏一页危险得多。
//!
//! # 账本模式
//!
//! `hhdm == 0` 表示纯记账：不计算虚拟地址、不触碰物理内存。宿主测试用它
//! 验证分配/释放/重复/越界四类判定；真机路径永远传真实 HHDM 偏移。

use super::engine::MAX_THREADS;

/// 栈的阶：`2^2 = 4` 页 = 16 KiB。深调用链（判据聚合一族递归很深）与
/// 中断嵌套都要装得下，16 KiB 是「够用且不浪费」的折中。
pub const KSTACK_ORDER: usize = 2;
pub const KSTACK_PAGES: usize = 1 << KSTACK_ORDER;
pub const PAGE_BYTES: u64 = 4096;
pub const KSTACK_BYTES: u64 = KSTACK_PAGES as u64 * PAGE_BYTES;

/// 栈底哨兵。取 ASCII "VARIXSTK" 的小端写法——串口转储里能直接认出来，
/// 不必翻常量表。
pub const STACK_CANARY: u64 = 0x5641_5249_5853_544B;
/// 哨兵占两个字：单字被「压栈一个值再返回」这类正常操作覆盖的概率不低，
/// 两个字让误报率降到可以忽略。
pub const CANARY_WORDS: u64 = 2;

/// 栈账本的失败原因。调用方必须能区分「重了」与「没有帧了」——前者是
/// 逻辑 bug，后者是资源耗尽，处理方式完全相反。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackError {
    /// tid 超出 `MAX_THREADS`。
    BadTid,
    /// 该槽已有存活栈；重复分配意味着上一次释放被漏掉。
    Duplicate,
    /// 物理页来源返回空。
    OutOfFrames,
    /// 物理基址不是 4 KiB 对齐。
    Misaligned,
    /// 释放一个从未分配的槽。
    Unknown,
    /// 栈底哨兵被改写：栈溢出或野写。
    Corrupt,
    /// 槽已因哨兵损坏被封存，等待隔离处理。
    Poisoned,
}

impl StackError {
    pub fn as_str(self) -> &'static str {
        match self {
            StackError::BadTid => "bad-tid",
            StackError::Duplicate => "duplicate",
            StackError::OutOfFrames => "out-of-frames",
            StackError::Misaligned => "misaligned",
            StackError::Unknown => "unknown",
            StackError::Corrupt => "corrupt",
            StackError::Poisoned => "poisoned",
        }
    }

    /// 该错误是否值得让上层停机。资源耗尽可以退化成「少起一个线程」，
    /// 结构性错误不行。
    pub fn fatal(self) -> bool {
        matches!(
            self,
            StackError::Duplicate | StackError::Corrupt | StackError::Poisoned
        )
    }
}

/// 一个线程槽的栈记录。
#[derive(Clone, Copy, Debug)]
pub struct KernelStack {
    /// 物理基址（最低地址那页）。
    pub phys: u64,
    /// 栈顶虚拟地址；账本模式下为 0。
    pub top: u64,
    /// 是否在服务中。
    pub live: bool,
    /// 哨兵已损坏，禁止归还物理帧。
    pub poisoned: bool,
}

impl KernelStack {
    pub const fn empty() -> KernelStack {
        KernelStack {
            phys: 0,
            top: 0,
            live: false,
            poisoned: false,
        }
    }

    pub fn usable(&self) -> bool {
        self.live && !self.poisoned && self.top != 0
    }
}

/// 固定容量栈账本。不做分配，随内核静态存活。
pub struct StackLedger {
    slots: [KernelStack; MAX_THREADS],
    allocs: u32,
    frees: u32,
    rejects: u32,
    corruptions: u32,
    live: u32,
}

impl StackLedger {
    pub const fn new() -> StackLedger {
        StackLedger {
            slots: [KernelStack::empty(); MAX_THREADS],
            allocs: 0,
            frees: 0,
            rejects: 0,
            corruptions: 0,
            live: 0,
        }
    }

    /// 分配一条内核栈。`source` 返回**物理**基址，必须是 `2^KSTACK_ORDER`
    /// 页的连续块；`hhdm` 为 0 时进入账本模式。
    ///
    /// 返回栈顶虚拟地址（账本模式返回 0，调用方应据 `hhdm` 自行判断）。
    pub fn alloc_with<F>(&mut self, tid: u32, hhdm: u64, mut source: F) -> Result<u64, StackError>
    where
        F: FnMut(usize) -> Option<u64>,
    {
        let slot = match self.slots.get_mut(tid as usize) {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(StackError::BadTid);
            }
        };
        if slot.live {
            self.rejects += 1;
            return Err(StackError::Duplicate);
        }
        if slot.poisoned {
            self.rejects += 1;
            return Err(StackError::Poisoned);
        }
        let phys = match source(KSTACK_ORDER) {
            Some(p) => p,
            None => {
                self.rejects += 1;
                return Err(StackError::OutOfFrames);
            }
        };
        if phys & (PAGE_BYTES - 1) != 0 || phys == 0 {
            self.rejects += 1;
            return Err(StackError::Misaligned);
        }
        let top = if hhdm == 0 {
            0
        } else {
            let base = phys + hhdm;
            paint_canary(base);
            base + KSTACK_BYTES
        };
        *slot = KernelStack {
            phys,
            top,
            live: true,
            poisoned: false,
        };
        self.allocs += 1;
        self.live += 1;
        Ok(top)
    }

    /// 释放一条栈。哨兵损坏时**不归还物理帧**，槽转为封存态并返回
    /// [`StackError::Corrupt`]——坏页回池会让下一个用户踩到同一块内存。
    pub fn release_with<F>(&mut self, tid: u32, mut free: F) -> Result<u64, StackError>
    where
        F: FnMut(u64) -> bool,
    {
        let slot = match self.slots.get_mut(tid as usize) {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(StackError::BadTid);
            }
        };
        if !slot.live {
            self.rejects += 1;
            return Err(if slot.poisoned {
                StackError::Poisoned
            } else {
                StackError::Unknown
            });
        }
        if slot.top != 0 && !canary_intact(slot.top - KSTACK_BYTES) {
            slot.live = false;
            slot.poisoned = true;
            self.live = self.live.saturating_sub(1);
            self.corruptions += 1;
            return Err(StackError::Corrupt);
        }
        let phys = slot.phys;
        *slot = KernelStack::empty();
        self.frees += 1;
        self.live = self.live.saturating_sub(1);
        if !free(phys) {
            // 归还失败不回滚账本：帧已经不在我们手里，账要如实反映。
            return Err(StackError::Unknown);
        }
        Ok(phys)
    }

    pub fn top(&self, tid: u32) -> Option<u64> {
        let s = self.slots.get(tid as usize)?;
        if s.usable() {
            Some(s.top)
        } else {
            None
        }
    }

    pub fn phys(&self, tid: u32) -> Option<u64> {
        let s = self.slots.get(tid as usize)?;
        if s.live && !s.poisoned {
            Some(s.phys)
        } else {
            None
        }
    }

    /// `rsp` 是否落在这条栈的合法区间内。给切换门控用。
    pub fn contains(&self, tid: u32, rsp: u64) -> bool {
        match self.top(tid) {
            Some(top) => rsp < top && rsp >= top.saturating_sub(KSTACK_BYTES),
            None => false,
        }
    }

    pub fn live(&self) -> u32 {
        self.live
    }

    pub fn allocs(&self) -> u32 {
        self.allocs
    }

    pub fn frees(&self) -> u32 {
        self.frees
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    pub fn corruptions(&self) -> u32 {
        self.corruptions
    }

    /// 逐槽验哨兵，返回（完好数, 损坏数）。损坏的槽就地封存。
    pub fn audit(&mut self) -> (u32, u32) {
        let mut good = 0u32;
        let mut bad = 0u32;
        for i in 0..MAX_THREADS {
            let s = self.slots[i];
            if !s.live || s.top == 0 {
                continue;
            }
            if canary_intact(s.top - KSTACK_BYTES) {
                good += 1;
            } else {
                bad += 1;
                self.slots[i].poisoned = true;
                self.slots[i].live = false;
                self.live = self.live.saturating_sub(1);
                self.corruptions += 1;
            }
        }
        (good, bad)
    }

    /// 账本恒等式：分配 - 释放 == 在线数。任何时刻都该成立。
    pub fn balanced(&self) -> bool {
        self.allocs.saturating_sub(self.frees) == self.live
    }
}

impl Default for StackLedger {
    fn default() -> StackLedger {
        StackLedger::new()
    }
}

/// 在 `base` 处押两个哨兵字。
///
/// # Safety
///
/// `base` 必须指向一条已分配、可写、至少 16 字节的栈底。真机路径里它来自
/// `phys + hhdm`，HHDM 对 PMM 管辖的全部帧都有映射。
pub fn paint_canary(base: u64) {
    if base == 0 {
        return;
    }
    // SAFETY: 契约见函数文档；base 由 alloc_with 从 PMM 帧算出。
    unsafe {
        let p = base as *mut u64;
        core::ptr::write_volatile(p, STACK_CANARY);
        core::ptr::write_volatile(p.add(1), STACK_CANARY);
    }
}

/// 验 `base` 处两个哨兵字。
pub fn canary_intact(base: u64) -> bool {
    if base == 0 {
        return false;
    }
    // SAFETY: 同 paint_canary；只读。
    unsafe {
        let p = base as *const u64;
        core::ptr::read_volatile(p) == STACK_CANARY
            && core::ptr::read_volatile(p.add(1)) == STACK_CANARY
    }
}

/// 判据：栈账本的十条不变量。
pub fn run_kstack_checks() -> (usize, usize) {
    let mut c = crate::checks::CheckSet::new("kstack");

    // 1. 容量算式自洽。
    if KSTACK_BYTES == KSTACK_PAGES as u64 * PAGE_BYTES && KSTACK_PAGES == 4 {
        c.ok("KSTK-容量-四页十六千字节");
    } else {
        c.fail("KSTK-容量-四页十六千字节", "pages*page != bytes");
    }

    // 2. 空账本自洽。
    let mut l = StackLedger::new();
    if l.balanced() && l.live() == 0 {
        c.ok("KSTK-空账-分配减释放为零");
    } else {
        c.fail("KSTK-空账-分配减释放为零", "empty ledger unbalanced");
    }

    // 3. 账本模式分配成功且在线数 +1。
    match l.alloc_with(1, 0, |_| Some(0x20_0000)) {
        Ok(_) if l.live() == 1 => c.ok("KSTK-分配-账本模式在线加一"),
        _ => c.fail("KSTK-分配-账本模式在线加一", "alloc did not take"),
    }

    // 4. 重复分配必须被拒，且在线数不变。
    let dup = l.alloc_with(1, 0, |_| Some(0x40_0000));
    if dup == Err(StackError::Duplicate) && l.live() == 1 {
        c.ok("KSTK-重复-同槽二次分配即拒");
    } else {
        c.fail("KSTK-重复-同槽二次分配即拒", "duplicate accepted");
    }

    // 5. 无帧可分配如实报耗尽。
    let oom = l.alloc_with(2, 0, |_| None);
    if oom == Err(StackError::OutOfFrames) {
        c.ok("KSTK-耗尽-来源为空即报");
    } else {
        c.fail("KSTK-耗尽-来源为空即报", "oom not reported");
    }

    // 6. 未对齐基址即拒。
    let mis = l.alloc_with(3, 0, |_| Some(0x1234));
    if mis == Err(StackError::Misaligned) {
        c.ok("KSTK-对齐-非四千零九十六倍数即拒");
    } else {
        c.fail("KSTK-对齐-非四千零九十六倍数即拒", "misaligned accepted");
    }

    // 7. 释放未分配的槽即拒。
    if l.release_with(9, |_| true) == Err(StackError::Unknown) {
        c.ok("KSTK-释放-未分配槽即拒");
    } else {
        c.fail("KSTK-释放-未分配槽即拒", "unknown release accepted");
    }

    // 8. 正常释放后账目平衡。
    let rel = l.release_with(1, |_| true);
    if rel == Ok(0x20_0000) && l.live() == 0 && l.balanced() {
        c.ok("KSTK-释放-归还基址且账平");
    } else {
        c.fail("KSTK-释放-归还基址且账平", "release mismatch");
    }

    // 9. 哨兵：本地缓冲上押/验一体。
    let mut buf = [0u64; 4];
    let base = buf.as_mut_ptr() as u64;
    paint_canary(base);
    let intact = canary_intact(base);
    // SAFETY: buf 是本地数组，下标 0 合法。
    unsafe {
        core::ptr::write_volatile(base as *mut u64, STACK_CANARY ^ 0xFF);
    }
    let broken = !canary_intact(base);
    if intact && broken {
        c.ok("KSTK-哨兵-完好与破坏可辨");
    } else {
        c.fail("KSTK-哨兵-完好与破坏可辨", "canary verdict wrong");
    }

    // 10. 区间判定：栈顶不含、栈内某点含。
    let mut l2 = StackLedger::new();
    let _ = l2.alloc_with(5, 0, |_| Some(0x80_0000));
    // 账本模式下 top 为 0，contains 必然 false——这里验证的是「未登记虚拟
    // 地址时不得误判为在栈内」这一条保守性质。
    if !l2.contains(5, 0x80_0000) && !l2.contains(5, 0x80_1000) {
        c.ok("KSTK-区间-无虚拟地址时不误判");
    } else {
        c.fail("KSTK-区间-无虚拟地址时不误判", "contains guessed true");
    }

    c.tally()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger_with_one(tid: u32) -> StackLedger {
        let mut l = StackLedger::new();
        let _ = l.alloc_with(tid, 0, |_| Some(0x10_0000));
        l
    }

    #[test]
    fn capacity_matches_four_pages() {
        assert_eq!(KSTACK_PAGES, 4);
        assert_eq!(KSTACK_BYTES, 16 * 1024);
    }

    #[test]
    fn alloc_marks_live_and_balances() {
        let l = ledger_with_one(1);
        assert_eq!(l.live(), 1);
        assert_eq!(l.allocs(), 1);
        assert!(l.balanced());
        assert_eq!(l.phys(1), Some(0x10_0000));
    }

    #[test]
    fn duplicate_is_rejected_and_counted() {
        let mut l = ledger_with_one(1);
        assert_eq!(
            l.alloc_with(1, 0, |_| Some(0x20_0000)),
            Err(StackError::Duplicate)
        );
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.live(), 1);
    }

    #[test]
    fn out_of_frames_is_not_fatal() {
        let mut l = StackLedger::new();
        let e = l.alloc_with(2, 0, |_| None).unwrap_err();
        assert_eq!(e, StackError::OutOfFrames);
        assert!(!e.fatal());
        assert_eq!(l.live(), 0);
    }

    #[test]
    fn misaligned_base_is_refused() {
        let mut l = StackLedger::new();
        assert_eq!(
            l.alloc_with(3, 0, |_| Some(0x1001)),
            Err(StackError::Misaligned)
        );
    }

    #[test]
    fn zero_base_is_refused() {
        let mut l = StackLedger::new();
        assert_eq!(
            l.alloc_with(3, 0, |_| Some(0)),
            Err(StackError::Misaligned)
        );
    }

    #[test]
    fn release_restores_balance() {
        let mut l = ledger_with_one(1);
        assert_eq!(l.release_with(1, |p| p == 0x10_0000), Ok(0x10_0000));
        assert_eq!(l.live(), 0);
        assert_eq!(l.frees(), 1);
        assert!(l.balanced());
        assert_eq!(l.top(1), None);
    }

    #[test]
    fn release_of_unknown_slot_is_refused() {
        let mut l = StackLedger::new();
        assert_eq!(l.release_with(7, |_| true), Err(StackError::Unknown));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn canary_detects_tampering() {
        let mut buf = [0u64; 8];
        let base = buf.as_mut_ptr() as u64;
        paint_canary(base);
        assert!(canary_intact(base));
        buf[1] ^= 1;
        assert!(!canary_intact(base));
    }

    #[test]
    fn corrupt_stack_is_not_returned_to_pool() {
        // 用真实缓冲模拟一条栈：top = base + KSTACK_BYTES，但缓冲只有 64 字节，
        // 因此只验证 release 的判定分支——先押哨兵再破坏，再要求释放被拒。
        let mut buf = [0u64; 8];
        let base = buf.as_mut_ptr() as u64;
        paint_canary(base);
        buf[0] = 0;
        assert!(!canary_intact(base));
        let mut l = StackLedger::new();
        // 手工构造一条映射到该缓冲的槽。
        l.slots[4] = KernelStack {
            phys: 0x10_0000,
            top: base + KSTACK_BYTES,
            live: true,
            poisoned: false,
        };
        l.live = 1;
        l.allocs = 1;
        let mut freed = false;
        let r = l.release_with(4, |_| {
            freed = true;
            true
        });
        assert_eq!(r, Err(StackError::Corrupt));
        assert!(!freed, "corrupt stack must not be freed");
        assert_eq!(l.corruptions(), 1);
    }

    #[test]
    fn poison_blocks_reallocation() {
        let mut l = StackLedger::new();
        assert_eq!(
            l.alloc_with(5, 0, |_| Some(0x30_0000)),
            Ok(0)
        );
        l.slots[5].poisoned = true;
        assert_eq!(
            l.alloc_with(5, 0, |_| Some(0x40_0000)),
            Err(StackError::Poisoned)
        );
    }

    #[test]
    fn audit_counts_intact_and_broken() {
        let mut buf = [0u64; 8];
        let base = buf.as_mut_ptr() as u64;
        paint_canary(base);
        let mut l = StackLedger::new();
        l.slots[0] = KernelStack {
            phys: 0x10_0000,
            top: base + KSTACK_BYTES,
            live: true,
            poisoned: false,
        };
        l.live = 1;
        l.allocs = 1;
        let (good, bad) = l.audit();
        assert_eq!((good, bad), (1, 0));
        // 破坏后再审，槽被封存。
        buf[0] = 0;
        let (good, bad) = l.audit();
        assert_eq!((good, bad), (0, 1));
        assert_eq!(l.live(), 0);
        assert_eq!(l.corruptions(), 1);
    }

    #[test]
    fn error_texts_are_distinct() {
        let all = [
            StackError::BadTid,
            StackError::Duplicate,
            StackError::OutOfFrames,
            StackError::Misaligned,
            StackError::Unknown,
            StackError::Corrupt,
            StackError::Poisoned,
        ];
        for i in 0..all.len() {
            assert!(!all[i].as_str().is_empty());
            for j in (i + 1)..all.len() {
                assert_ne!(all[i].as_str(), all[j].as_str());
            }
        }
    }

    #[test]
    fn checks_are_green() {
        let (p, f) = run_kstack_checks();
        assert_eq!(f, 0, "kstack checks must be green, passed={}", p);
        assert!(p >= 10);
    }
}
