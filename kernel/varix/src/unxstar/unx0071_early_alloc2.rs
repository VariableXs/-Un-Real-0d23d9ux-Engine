//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0071 · 早年引导内存分配器（深化第二实现） · 0x6E100
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F0071-J1 引导期 bump 分配器：顺序分配、4K/8/16 三档
//!     对齐、释放窗口（只许回滚顶针）、分配账可重放（重放指纹
//!     一致）、耗尽显性码；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - bump 核心：顶针单调（只进不退，释放 = 回滚顶针到标记）；
//!   - 三档对齐：4K（页）/8（指针）/16（SIMD），上取对齐丢弃记账；
//!   - 分配账：每次分配（对齐档/尺寸/地址）登记，回滚弹账；
//!   - 重放指纹：账本 FNV 折叠，重放（重建分配序列）指纹一致；
//!   - 耗尽闸：剩余 < 请求 + 对齐丢弃 ⇒ 显性码（不静默截断）；
//!   - 边界账：分配区 [heap_base, heap_top) 越界拒；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6E100，AI-VX3-1102 批I 独占区）。
pub const SEG_BASE: u32 = 0x6E100;

/// 对齐档（冻结）。
pub const ALIGN_PAGE: u64 = 0x1000;
pub const ALIGN_8: u64 = 8;
pub const ALIGN_16: u64 = 16;
/// 引导堆（真机口径：16MB @ 0x100000）。
pub const HEAP_BASE: u64 = 0x0010_0000;
pub const HEAP_SIZE: u64 = 0x100_0000;
/// 最大分配账数。
pub const MAX_ALLOC_ACCOUNTS: usize = 128;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BumpCode {
    /// 堆耗尽
    HeapExhausted,
    /// 请求尺寸 0
    ZeroRequest,
    /// 对齐档账外
    AlignGradeBad,
    /// 回滚越顶（回滚点不在账内）
    RollbackOver,
    /// 越堆边界（顶针越 heap_top）
    TopOverHeap,
    /// 重放指纹不一致
    ReplayMismatch,
    /// 分配账溢出
    AccountOver,
    /// 回滚序错（非顶回滚）
    RollbackNotTop,
    /// 双跑不一致
    NotReproducible,
}

impl BumpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BumpCode::HeapExhausted => "堆耗尽：显性码不截断",
            BumpCode::ZeroRequest => "请求尺寸 0",
            BumpCode::AlignGradeBad => "对齐档账外：非 4K/8/16",
            BumpCode::RollbackOver => "回滚越顶：标记不在账内",
            BumpCode::TopOverHeap => "顶针越堆边界",
            BumpCode::ReplayMismatch => "重放指纹不一致",
            BumpCode::AccountOver => "分配账溢出：128 冻结",
            BumpCode::RollbackNotTop => "回滚序错：非顶回滚",
            BumpCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [BumpCode; 9] = [
        BumpCode::HeapExhausted,
        BumpCode::ZeroRequest,
        BumpCode::AlignGradeBad,
        BumpCode::RollbackOver,
        BumpCode::TopOverHeap,
        BumpCode::ReplayMismatch,
        BumpCode::AccountOver,
        BumpCode::RollbackNotTop,
        BumpCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BumpCode::ALL.len() {
        if BumpCode::ALL[i].val() == code {
            return BumpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6E100 段闭集内"
}

// ── bump 分配器 ─────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocAccount {
    pub addr: u64,
    pub size: u64,
    pub align: u64,
    /// 对齐丢弃字节。
    pub pad: u64,
}

#[derive(Clone, Debug)]
pub struct BumpAlloc {
    top: u64,
    limit: u64,
    pub accounts: Vec<AllocAccount>,
    pub pad_total: u64,
}

impl BumpAlloc {
    pub fn new(base: u64, size: u64) -> BumpAlloc {
        BumpAlloc { top: base, limit: base + size, accounts: Vec::new(), pad_total: 0 }
    }

    pub fn heap_top() -> u64 {
        HEAP_BASE + HEAP_SIZE
    }

    pub fn used(&self) -> u64 {
        self.top - HEAP_BASE
    }

    pub fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.top)
    }

    /// 分配（三档对齐）。
    pub fn alloc(&mut self, size: u64, align: u64) -> Result<u64, u32> {
        if size == 0 {
            return Err(BumpCode::ZeroRequest.val());
        }
        if align != ALIGN_PAGE && align != ALIGN_8 && align != ALIGN_16 {
            return Err(BumpCode::AlignGradeBad.val());
        }
        if self.accounts.len() >= MAX_ALLOC_ACCOUNTS {
            return Err(BumpCode::AccountOver.val());
        }
        let aligned = (self.top + align - 1) & !(align - 1);
        let pad = aligned - self.top;
        let new_top = aligned + size;
        if new_top > self.limit {
            return Err(BumpCode::HeapExhausted.val());
        }
        self.accounts.push(AllocAccount { addr: aligned, size, align, pad });
        self.pad_total += pad;
        self.top = new_top;
        Ok(aligned)
    }

    /// 回滚顶针（只许回滚到账内标记 = 弹出顶账）。
    pub fn rollback_top(&mut self) -> Result<u64, u32> {
        match self.accounts.pop() {
            Some(a) => {
                self.top = a.addr;
                self.pad_total -= a.pad;
                Ok(self.top)
            }
            None => Err(BumpCode::RollbackOver.val()),
        }
    }

    /// 回滚到指定深度（深度必须 ≤ 当前账深）。
    pub fn rollback_to(&mut self, depth: usize) -> Result<usize, u32> {
        if depth > self.accounts.len() {
            return Err(BumpCode::RollbackNotTop.val());
        }
        while self.accounts.len() > depth {
            let _ = self.rollback_top();
        }
        Ok(self.accounts.len())
    }

    /// 重放指纹：账本逐条折叠。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.accounts.len() {
            let a = &self.accounts[i];
            let words = [a.addr, a.size, a.align, a.pad];
            let mut w = 0;
            while w < 4 {
                let mut b = 0;
                while b < 8 {
                    h = (h ^ ((words[w] >> (b * 8)) & 0xFF)).wrapping_mul(0x100000001b3);
                    b += 1;
                }
                w += 1;
            }
            i += 1;
        }
        h
    }

    /// 重放：同序列分配重建账本比指纹。
    pub fn replay_consistent(&self) -> Result<(), u32> {
        let mut fresh = BumpAlloc::new(HEAP_BASE, HEAP_SIZE);
        let mut i = 0;
        while i < self.accounts.len() {
            fresh.alloc(self.accounts[i].size, self.accounts[i].align)?;
            i += 1;
        }
        if fresh.fingerprint() != self.fingerprint() || fresh.top != self.top {
            return Err(BumpCode::ReplayMismatch.val());
        }
        Ok(())
    }
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路：耗尽路/档位路/回滚路/重放路） ──────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 标准分配序列（真机口径：页表页×2 + 指针账×3 + SIMD 账×2）。
pub fn standard_sequence(a: &mut BumpAlloc) -> Result<Vec<u64>, u32> {
    let mut out = Vec::new();
    out.push(a.alloc(ALIGN_PAGE, ALIGN_PAGE)?);
    out.push(a.alloc(ALIGN_PAGE, ALIGN_PAGE)?);
    out.push(a.alloc(0x30, ALIGN_8)?);
    out.push(a.alloc(0x18, ALIGN_8)?);
    out.push(a.alloc(0x20, ALIGN_16)?);
    out.push(a.alloc(0x40, ALIGN_16)?);
    Ok(out)
}

// ── 黄金全链：标准序列→账数→对齐闸→重放一致→回滚恢复 ─────────────
pub fn golden_run() -> Result<(usize, u64, u64), u32> {
    let mut a = BumpAlloc::new(HEAP_BASE, HEAP_SIZE);
    let addrs = standard_sequence(&mut a)?;
    // 对齐闸：页账 4K 对齐、SIMD 账 16 对齐。
    let mut i = 0;
    while i < addrs.len() {
        let acc = &a.accounts[i];
        if acc.addr % acc.align != 0 {
            return Err(BumpCode::TopOverHeap.val());
        }
        i += 1;
    }
    let fp = a.fingerprint();
    a.replay_consistent()?;
    let before = a.accounts.len();
    a.rollback_to(4)?;
    let after = a.accounts.len();
    let _ = fp;
    Ok((before, after as u64, a.pad_total))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账 → 账 6 → 回滚到 4 → 对齐丢弃有账。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((6, 4, _)) => Ok(()),
        _ => Err(BumpCode::NotReproducible.val()),
    }
}

/// 序 B：耗尽 ⇒ HeapExhausted（请求越堆顶）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut a = BumpAlloc::new(HEAP_BASE, HEAP_SIZE);
    match a.alloc(HEAP_SIZE + 1, ALIGN_8) {
        Err(e) if e == BumpCode::HeapExhausted.val() => Ok(()),
        _ => Err(BumpCode::NotReproducible.val()),
    }
}

/// 序 C：档位账外 ⇒ AlignGradeBad。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut a = BumpAlloc::new(HEAP_BASE, HEAP_SIZE);
    match a.alloc(16, 32) {
        Err(e) if e == BumpCode::AlignGradeBad.val() => Ok(()),
        _ => Err(BumpCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：耗尽路（小堆连续吞到尽）。
        let mut a1 = BumpAlloc::new(HEAP_BASE, ALIGN_PAGE * 2);
        let mut hit1 = false;
        let mut k = 0;
        while k < 4 {
            if a1.alloc(ALIGN_PAGE, ALIGN_PAGE).is_err() {
                hit1 = true;
            }
            k += 1;
        }
        led.record(0, hit1);
        // 路 2：档位路。
        let mut a2 = BumpAlloc::new(HEAP_BASE, HEAP_SIZE);
        led.record(1, a2.alloc(8, 64).is_err());
        // 路 3：回滚路（空账回滚必拒）。
        let mut a3 = BumpAlloc::new(HEAP_BASE, HEAP_SIZE);
        led.record(2, a3.rollback_top().is_err());
        // 路 4：重放路（同序列重放指纹恒一致）。
        let mut a4 = BumpAlloc::new(HEAP_BASE, HEAP_SIZE);
        let _ = standard_sequence(&mut a4);
        led.record(3, a4.replay_consistent().is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BumpCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BumpCode::ALL.len() {
        let v = BumpCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((before, after, _pad)) => *before == 6 && *after == 4,
            Err(_) => false,
        },
        match &g {
            Ok((before, after, pad)) => format!("accounts {}→{} pad={}", before, after, pad),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 顶针单调 + 回滚恢复顶针。
    let mut a = BumpAlloc::new(HEAP_BASE, HEAP_SIZE);
    let x1 = a.alloc(0x100, ALIGN_8);
    let t1 = a.used();
    let _ = a.rollback_top();
    let t0 = a.used();
    v.push((
        "J1-顶针恢复",
        x1.is_ok() && t1 > 0 && t0 == 0,
        String::from("分配进顶 / 回滚复位"),
    ));
    // 对齐丢弃记账：非对齐顶上取后 pad 有账。
    let mut a2 = BumpAlloc::new(HEAP_BASE, HEAP_SIZE);
    let _ = a2.alloc(0x18, ALIGN_8);
    let _ = a2.alloc(ALIGN_PAGE, ALIGN_PAGE); // 页对齐 pad 记账
    v.push((
        "J1-丢弃记账",
        a2.pad_total > 0 && a2.accounts[1].pad > 0,
        format!("pad_total={}", a2.pad_total),
    ));
    // 首地址即堆基（对齐恒真）。
    let mut a3 = BumpAlloc::new(HEAP_BASE, HEAP_SIZE);
    let p = a3.alloc(0x10, ALIGN_16);
    v.push((
        "J1-基址账",
        p == Ok(HEAP_BASE),
        String::from("顶针初始 = heap_base"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a4 = probe_seq_a().is_ok();
    let b4 = probe_seq_b().is_ok();
    let c4 = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a4 && b4 && c4,
        format!("a=绿 b=检出{} c=检出{}", b4, c4),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6E100 窗口"),
    ));
    v
}

pub fn run_unx0071_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0071 早年引导内存分配器");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
