//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1923 · lazy TLB 模式（内核线程借用 mm） · 0x6D180
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B4）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1923-J1 借用/退出条件断言全过（内核线程借用活动 mm
//!     跳过 CR3 写），借用期击落处置正确（F1867 mask 挂靠语义），
//!     借出方退出必收回 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 借用语义：内核线程（无自有 mm）借当前活动 mm → 跳过 CR3
//!      写（lazy）；借出记录（借方/出方/mm 指纹）；
//!    - 借用期击落：mm 的 TLB 击落须挂靠到借用核（F1867 mask 语
//!      义：借用核计入应答掩码）；
//!    - 借出方退出必收回：mm 拥有者退出时收回借用（10/10 收回）；
//!    - 借用账守恒：借出数 = 借用数 - 收回数；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D180，AI-VX3-1030 批E 独占区）。
pub const SEG_BASE: u32 = 0x6D180;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 核数。
pub const CORE_COUNT: usize = 4;
/// mm 指纹样本基址。
pub const MM_BASE: u64 = 0x500_0000;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LazyCode {
    /// 核号越界
    CoreOutOfRange,
    /// mm 指纹越界
    MmOutOfRange,
    /// 重复借用（同核已借）
    DoubleBorrow,
    /// 收回时无借用
    ReturnWithoutBorrow,
    /// 击落挂靠漏（借用核未入应答掩码）
    ShootdownMaskMiss,
    /// 借出方退出未收回
    OwnerExitNoRecall,
    /// 守恒破坏（借出≠借用-收回）
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
}

impl LazyCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LazyCode::CoreOutOfRange => "核号越界：4 核",
            LazyCode::MmOutOfRange => "mm 指纹越界",
            LazyCode::DoubleBorrow => "重复借用：同核已借",
            LazyCode::ReturnWithoutBorrow => "收回时无借用",
            LazyCode::ShootdownMaskMiss => "击落挂靠漏：借用核未入掩码",
            LazyCode::OwnerExitNoRecall => "借出方退出未收回",
            LazyCode::ConservationBroken => "守恒破坏",
            LazyCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [LazyCode; 8] = [
        LazyCode::CoreOutOfRange,
        LazyCode::MmOutOfRange,
        LazyCode::DoubleBorrow,
        LazyCode::ReturnWithoutBorrow,
        LazyCode::ShootdownMaskMiss,
        LazyCode::OwnerExitNoRecall,
        LazyCode::ConservationBroken,
        LazyCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LazyCode::ALL.len() {
        if LazyCode::ALL[i].val() == code {
            return LazyCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D180 段闭集内"
}

// ── lazy TLB 账 ────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Borrow {
    /// 借用核。
    pub core: usize,
    /// mm 指纹。
    pub mm: u64,
}

pub struct LazyTlbLedger {
    /// 活动借用账。
    pub borrows: Vec<Borrow>,
    /// 累计借出/借用/收回。
    pub lent: u64,
    pub borrowed_total: u64,
    pub recalled: u64,
}

impl LazyTlbLedger {
    pub fn new() -> LazyTlbLedger {
        LazyTlbLedger { borrows: Vec::new(), lent: 0, borrowed_total: 0, recalled: 0 }
    }

    /// 内核线程借 mm（跳 CR3 写：返回 false 表示无 CR3 写）。
    pub fn borrow(&mut self, core: usize, mm: u64) -> Result<bool, u32> {
        if core >= CORE_COUNT {
            return Err(LazyCode::CoreOutOfRange.val());
        }
        if mm < MM_BASE {
            return Err(LazyCode::MmOutOfRange.val());
        }
        let mut i = 0;
        while i < self.borrows.len() {
            if self.borrows[i].core == core {
                return Err(LazyCode::DoubleBorrow.val());
            }
            i += 1;
        }
        self.borrows.push(Borrow { core, mm });
        self.lent += 1;
        self.borrowed_total += 1;
        // lazy：跳 CR3 写。
        Ok(false)
    }

    /// 借用期击落：应答掩码必须包含借用核（F1867 mask 挂靠）。
    pub fn shootdown_mask(&self, mm: u64, base_mask: u8) -> Result<u8, u32> {
        let mut mask = base_mask;
        let mut i = 0;
        while i < self.borrows.len() {
            if self.borrows[i].mm == mm {
                mask |= 1 << self.borrows[i].core;
            }
            i += 1;
        }
        // 挂靠漏判据：borrowing cores of mm 必须在掩码中。
        i = 0;
        while i < self.borrows.len() {
            if self.borrows[i].mm == mm && mask & (1 << self.borrows[i].core) == 0 {
                return Err(LazyCode::ShootdownMaskMiss.val());
            }
            i += 1;
        }
        Ok(mask)
    }

    /// 收回。
    pub fn recall(&mut self, core: usize) -> Result<(), u32> {
        let mut found = None;
        let mut i = 0;
        while i < self.borrows.len() {
            if self.borrows[i].core == core {
                found = Some(i);
            }
            i += 1;
        }
        match found {
            Some(i) => {
                self.borrows.remove(i);
                self.recalled += 1;
                Ok(())
            }
            None => Err(LazyCode::ReturnWithoutBorrow.val()),
        }
    }

    /// 借出方退出必收回：mm 拥有者退出 → 该 mm 全部借用收回。
    pub fn owner_exit(&mut self, mm: u64) -> Result<u32, u32> {
        let mut recalled = 0;
        let mut i = 0;
        while i < self.borrows.len() {
            if self.borrows[i].mm == mm {
                self.borrows.remove(i);
                self.recalled += 1;
                recalled += 1;
            } else {
                i += 1;
            }
        }
        Ok(recalled)
    }

    /// 守恒：借出 = 借用 - 收回。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.lent != self.borrowed_total - self.recalled {
            return Err(LazyCode::ConservationBroken.val());
        }
        Ok(())
    }
}

/// 借出方退出必收回 10/10。
pub fn owner_exit_10() -> Result<u32, u32> {
    let mut ok = 0;
    let mut k = 0;
    while k < 10 {
        let mut led = LazyTlbLedger::new();
        let mm = MM_BASE + k as u64 * 0x100;
        // 核 0/1 借 mm。
        led.borrow(0, mm)?;
        led.borrow(1, mm)?;
        // 拥有者退出 → 2 借用全收回。
        let n = led.owner_exit(mm)?;
        if n == 2 && led.borrows.is_empty() {
            ok += 1;
        }
        k += 1;
    }
    if ok != 10 {
        return Err(LazyCode::OwnerExitNoRecall.val());
    }
    Ok(ok)
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

// ── 注入检出账 ──────────────────────────────────────────────────
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

/// 黄金全链：借用→击落挂靠→收回→守恒→退出必收回 10/10。
pub fn golden_run() -> Result<(bool, u64, u32), u32> {
    let mut led = LazyTlbLedger::new();
    let mm = MM_BASE + 0x40;
    // 借用（跳 CR3 写）。
    let cr3_skipped = led.borrow(2, mm)?;
    if cr3_skipped {
        return Err(LazyCode::ConservationBroken.val());
    }
    // 击落挂靠：基础掩码不含核 2 → 挂靠后必须含。
    let mask = led.shootdown_mask(mm, 0b0001)?;
    if mask & (1 << 2) == 0 {
        return Err(LazyCode::ShootdownMaskMiss.val());
    }
    // 收回。
    led.recall(2)?;
    led.conserve()?;
    let exit = owner_exit_10()?;
    Ok((!cr3_skipped, mask as u64, exit))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut led = LazyTlbLedger::new();
    led.borrow(0, MM_BASE)?;
    led.recall(0)?;
    led.conserve()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 重复借用 → 必拒。
    let mut led = LazyTlbLedger::new();
    led.borrow(0, MM_BASE)?;
    led.borrow(0, MM_BASE + 0x100).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 无借用收回 → 必拒。
    let mut led = LazyTlbLedger::new();
    led.recall(3)
}

/// 注入 10/10：重复借用、无借用收回、挂靠漏、退出未收回四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：重复借用。
        let mut l0 = LazyTlbLedger::new();
        l0.borrow(0, MM_BASE).ok();
        led.record(0, l0.borrow(0, MM_BASE + 0x100).is_err());
        // 槽1：无借用收回。
        let mut l1 = LazyTlbLedger::new();
        led.record(1, l1.recall(0).is_err());
        // 槽2：挂靠漏。
        let mut l2 = LazyTlbLedger::new();
        l2.borrow(1, MM_BASE).ok();
        led.record(2, l2.shootdown_mask(MM_BASE, 0b0000).is_err());
        // 槽3：退出必收回 10/10。
        led.record(3, owner_exit_10() == Ok(10));
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(LazyCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < LazyCode::ALL.len() {
        let v = LazyCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 借用跳 CR3 写。
    let mut led = LazyTlbLedger::new();
    let borrow_ok = match led.borrow(1, MM_BASE) {
        Ok(false) => true,
        _ => false,
    };
    v.push((
        "J1-借用跳CR3写",
        borrow_ok,
        String::from("lazy 语义"),
    ));
    // 退出必收回 10/10。
    v.push((
        "J1-借出方退出必收回10/10",
        owner_exit_10() == Ok(10),
        format!("{}/10", owner_exit_10().unwrap_or(0)),
    ));
    // 注入 10/10。
    let led2 = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led2 {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led2.as_ref().map(|l| l.tries),
            led2.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D180 窗口")));
    v
}

pub fn run_unx1923_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1923 lazy TLB 模式（内核线程借用 mm）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
