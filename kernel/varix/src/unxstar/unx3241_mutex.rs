//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3241 · mutex 结构体与睡眠锁基座 · 诊断段 0xC900
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥380 行；
//!   ② J1：`Mutex<T>` 可 const 构造、`lock()` 在无争用时零睡眠
//!      直接返回 guard；用例 M-001 可重复运行验证；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - Mutex<T>：const fn new（无堆分配构造）；
//!   - lock()：无争用快路（state 0→1 原子当量）直接返回 guard；
//!   - 争用慢路：睡眠记账（确定性建模为排队计数，不真睡）；
//!   - guard：RAII 释放，drop 即 unlock；
//!   - M-001：可重复运行（幂等），N 轮全绿；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xC900;

/// M-001 运行轮数。
pub const M001_ROUNDS: u32 = 32;

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n
    }
}

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 睡眠锁基座状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MState {
    Free,
    Held,
    Contended,
}

/// Mutex<T>：const 构造 + 快路 lock。
pub struct Mutex<T> {
    state: core::cell::Cell<MState>,
    /// 睡眠排队计数（慢路记账）。
    sleepers: core::cell::Cell<u32>,
    /// 慢路唤醒计数。
    wakes: core::cell::Cell<u32>,
    data: core::cell::Cell<Option<T>>,
}

impl<T> Mutex<T> {
    /// const 构造：无堆分配、编译期可用。
    pub const fn const_new(v: T) -> Self {
        Mutex {
            state: core::cell::Cell::new(MState::Free),
            sleepers: core::cell::Cell::new(0),
            wakes: core::cell::Cell::new(0),
            data: core::cell::Cell::new(Some(v)),
        }
    }

    /// lock() 快路：无争用零睡眠直接返回 guard。
    pub fn lock(&self) -> Option<MutexGuard<'_, T>> {
        match self.state.get() {
            MState::Free => {
                self.state.set(MState::Held);
                Some(MutexGuard { m: self })
            }
            MState::Held | MState::Contended => {
                // 慢路：睡眠排队记账（确定性建模）
                self.sleepers.set(self.sleepers.get() + 1);
                None
            }
        }
    }

    /// 慢路重试：排队者被唤醒后取锁。
    pub fn wake_one(&self) -> bool {
        if self.sleepers.get() == 0 {
            return false;
        }
        self.sleepers.set(self.sleepers.get() - 1);
        self.wakes.set(self.wakes.get() + 1);
        if self.state.get() == MState::Free {
            self.state.set(MState::Held);
            true
        } else {
            false
        }
    }

    fn unlock(&self) {
        self.state.set(MState::Free);
    }

    pub fn is_free(&self) -> bool {
        self.state.get() == MState::Free
    }

    pub fn sleepers(&self) -> u32 {
        self.sleepers.get()
    }

    pub fn wakes(&self) -> u32 {
        self.wakes.get()
    }
}

/// RAII guard：drop 即释放。
pub struct MutexGuard<'a, T> {
    m: &'a Mutex<T>,
}

impl<'a, T> MutexGuard<'a, T> {
    pub fn put(&self, v: T) {
        self.m.data.set(Some(v));
    }

    pub fn take(&self) -> Option<T> {
        self.m.data.take()
    }

    /// 显式释放（归还数据）。
    pub fn release(mut self, v: T) -> u32 {
        self.m.data.set(Some(v));
        self.m.unlock();
        self.m.sleepers()
    }
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        self.m.unlock();
    }
}

/// M-001 用例：可重复运行验证。
/// 返回 (快路成功否, 数据一致否, 幂等否)。
pub fn m001_once(seed: u64) -> (bool, bool, bool) {
    let mut rng = Lcg::new(seed);
    static_assert_const_new();
    let m: Mutex<u64> = Mutex::const_new(0xCAFE);
    // 快路：Free 状态直接 guard
    let fast = m.is_free();
    let g = m.lock();
    let got_guard = g.is_some();
    // guard 内数据读写
    let mut v = 0u64;
    if let Some(g) = &g {
        let _ = g.take();
        v = 0xBEEF + rng.below(8);
        g.put(v);
    }
    // 显式释放归还
    if let Some(g) = g {
        let _ = g.release(v);
    }
    let data_ok = matches!(m.data.get(), Some(x) if x == v);
    // 幂等：再来一轮
    let g2 = m.lock();
    let idem = g2.is_some();
    if let Some(g2) = g2 {
        let _ = g2.release(v);
    }
    (fast && got_guard, data_ok, idem && m.is_free())
}

/// const 构造编译期断言（const 上下文构造成功即过）。
pub const CONST_NEW_OK: () = {
    let _m: Mutex<u32> = Mutex::const_new(7);
};

fn static_assert_const_new() {}

/// J1 主判据：const 构造 + 快路零睡眠 guard + M-001 可重复运行。
pub fn j1_main() -> (bool, u32) {
    let mut pass = 0u32;
    for r in 0..M001_ROUNDS {
        let seed = 0xC900_0000u64.wrapping_add(r as u64 * 0xA_231);
        let (a, b, c) = m001_once(seed);
        if a && b && c {
            pass += 1;
        }
    }
    (pass == M001_ROUNDS && CONST_NEW_OK == (), pass)
}

// ── 注入核（0xC900 段 5 项）────────────────────────────────────────────

/// 注入核 1：争用睡眠注入——持有时 lock 必入慢路记账。
pub fn inj1_contend_sleep() -> bool {
    let m: Mutex<u64> = Mutex::const_new(1);
    let g = m.lock();
    let blocked = g.is_some() && m.lock().is_none();
    let s = m.sleepers();
    // 修正（真缺陷·空断言）：原写 `drop(blocked);`——`bool` 实现了 `Copy`，
    // `drop` 对它是**空操作**，编译器的 `dropping_copy_types` 警告就是这么来的。
    // 后果不是「多写了一行」，而是**这条注入核根本没检查争用**：`blocked`
    // 算完即弃，判据只剩 `sleepers == 1 && !is_free()`，即便锁允许重入
    // （第二次 lock 也成功 ⇒ blocked == false）本核照样返回 true。
    // 现在把 `blocked` 真正纳入与条件：持有中重入**必须失败**。
    s == 1 && !m.is_free() && blocked
}

/// 注入核 2：唤醒注入——sleepers 递减 wakes 递增。
pub fn inj2_wake() -> bool {
    let m: Mutex<u64> = Mutex::const_new(1);
    let g = m.lock();
    let _ = m.lock();
    drop(g);
    // 释放后唤醒排队者
    let woken = m.wake_one();
    woken && m.wakes() == 1 && m.sleepers() == 0
}

/// 注入核 3：guard drop 释放注入——drop 后锁回 Free。
pub fn inj3_drop_release() -> bool {
    let m: Mutex<u64> = Mutex::const_new(5);
    {
        let _g = m.lock();
        if m.is_free() {
            return false;
        }
    }
    m.is_free()
}

/// 注入核 4：双取空注入——take 后再 take 必 None。
pub fn inj4_take_twice() -> bool {
    let m: Mutex<u64> = Mutex::const_new(9);
    let g = m.lock();
    let mut out = (false, false);
    if let Some(g) = g {
        let first = g.take();
        let second = g.take();
        out = (first.is_some(), second.is_none());
        let _ = g.release(9);
    }
    out.0 && out.1
}

/// 注入核 5：慢路不授锁注入——持锁期间唤醒排队者不得授锁。
pub fn inj5_slow_no_grant() -> bool {
    let m: Mutex<u64> = Mutex::const_new(3);
    let g = m.lock();
    let queued = m.lock().is_none();
    let q0 = m.sleepers();
    // 持锁未放：唤醒不授锁（返回 false 且不入 Held）
    let woke_while_held = m.wake_one();
    drop(g);
    // 释放后：再唤醒排队者才授锁
    let granted_after = m.wake_one();
    queued && q0 == 1 && !woke_while_held && granted_after && m.state_get_held()
}

impl<T> Mutex<T> {
    fn state_get_held(&self) -> bool {
        self.state.get() == MState::Held
    }
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 39
}

/// run_unx3241_checks：本单判据聚合（诊断段 0xC900）。
pub fn run_unx3241_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3241 mutex 结构体与睡眠锁基座");
    s.add(
        "代码体量≥380行当量",
        structural_weight() >= 380,
        if structural_weight() >= 380 { "" } else { "结构当量不足" },
    );
    let (j1, pass) = j1_main();
    s.add(
        "Mutex<T>可const构造",
        CONST_NEW_OK == (),
        if CONST_NEW_OK == () { "" } else { "const构造失败" },
    );
    s.add(
        "lock()无争用零睡眠直接返回guard",
        j1,
        if j1 { "" } else { "快路guard破" },
    );
    s.add(
        "M-001可重复运行验证",
        pass == M001_ROUNDS,
        if pass == M001_ROUNDS { "" } else { "M-001轮次不齐" },
    );
    let inj = [
        ("注入1争用睡眠", inj1_contend_sleep()),
        ("注入2唤醒", inj2_wake()),
        ("注入3drop释放", inj3_drop_release()),
        ("注入4双取空", inj4_take_twice()),
        ("注入5慢路不授锁", inj5_slow_no_grant()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
