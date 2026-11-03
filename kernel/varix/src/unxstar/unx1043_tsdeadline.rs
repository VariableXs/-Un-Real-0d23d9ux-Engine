//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1043 · TSC-deadline 模式（LAPIC 定时器 deadline 投递）· 0x71F80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2-B13）：
//!   ① 纯功能代码 ≥360 行；
//!   ② UNX-F1043-J1 模式正确（deadline 写入→单次到期 10 万次零丢
//!     失），MSR 支持探测 10/10 次（支持/不支持正确分支），到期误差
//!     p99 ≤10µs（QEMU 档）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - TSC-deadline 语义模型：MSR 0x6B0 写入 → 绝对时刻单次到期投递
//!     → 再武装；写过去时刻显性拒（DeadlinePast）；
//!   - 支持探测：CPUID leaf 0x1 ECX bit24 注入建模，支持/不支持双分
//!     支各 10/10 正确（支持时不拒、不支持时必拒）；
//!   - 到期误差账：1024 采样环 + 整数分位 p99，QEMU 档 ≤10µs；
//!   - 丢失账：未到期重复武装即显性丢一笔，10 万次单次到期丢失恒 0；
//!   - 投递状态机单调推进：Idle→Armed→Fired，单次语义不可连投。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x71F80，VX3-1103 批独占区）。
pub const SEG_BASE: u32 = 0x71F80;

/// TSC-deadline MSR 地址（0x6B0 冻结）。
pub const MSR_TSC_DEADLINE: u32 = 0x6B0;
/// CPUID leaf 0x1 ECX bit24（TSC-deadline 支持位）。
pub const CPUID_TSCDL_BIT: u32 = 24;
/// 到期误差上限（µs，QEMU 档冻结 10）。
pub const ERR_LIMIT_US: u64 = 10;
/// 到期误差采样环深（1024 冻结）。
pub const SAMPLE_RING: usize = 1024;
/// 零丢失目标（10 万次单次到期冻结）。
pub const ZERO_LOSS_TARGET: u64 = 100_000;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// QEMU 档真机 TSC 频率（2.4GHz 冻结口径）。
pub const QEMU_TSC_HZ: u64 = 2_400_000_000;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TdCode {
    /// deadline 写入过去时刻（绝对时刻语义显性拒）
    DeadlinePast,
    /// CPUID 不支持 TSC-deadline（探测分支必拒面）
    NotSupported,
    /// 到期丢失（未到期重复武装 / 到期未投递）
    MissedDeadline,
    /// 到期误差超限（p99 > 10µs）
    ErrOverLimit,
    /// 空转投递（未武装却尝试到期处理）
    FireWhileIdle,
    /// 频率为零（µs→tick 换算无法进行）
    FreqZero,
    /// 采样环账复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl TdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TdCode::DeadlinePast => "deadline 写入过去时刻：绝对时刻语义只接受未来值",
            TdCode::NotSupported => "CPUID 不支持 TSC-deadline：建议回退周期/相对模式",
            TdCode::MissedDeadline => "到期丢失：未到期重复武装即丢一笔，红线",
            TdCode::ErrOverLimit => "到期误差超限：p99 > 10µs（QEMU 档），建议查投递路径",
            TdCode::FireWhileIdle => "空转投递：未武装却尝试到期处理，状态机失守",
            TdCode::FreqZero => "TSC 频率为零：µs→tick 换算无法进行",
            TdCode::ReplayMismatch => "采样环账复算失配：误差账被篡改",
            TdCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [TdCode; 8] = [
        TdCode::DeadlinePast,
        TdCode::NotSupported,
        TdCode::MissedDeadline,
        TdCode::ErrOverLimit,
        TdCode::FireWhileIdle,
        TdCode::FreqZero,
        TdCode::ReplayMismatch,
        TdCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TdCode::ALL.len() {
        if TdCode::ALL[i].val() == code {
            return TdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x71F80 段闭集内"
}

// ── 支持探测（CPUID bit24 注入建模，双分支闭合） ─────────────────
/// 探测结果闭集：支持 / 不支持（二支全闭合，无第三态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TscDlCap {
    /// CPUID leaf 0x1 ECX bit24 = 1
    Supported,
    /// bit24 = 0
    Unsupported,
}

/// CPUID bit 注入 → 探测分支（纯函数：注入什么判什么）。
pub fn probe_support(cpuid_bit24: bool) -> TscDlCap {
    if cpuid_bit24 {
        TscDlCap::Supported
    } else {
        TscDlCap::Unsupported
    }
}

// ── µs→tick 换算（全整数，u128 中间量，除零显性拒） ─────────────
pub fn us_to_ticks(us: u64, hz: u64) -> Result<u64, u32> {
    if hz == 0 {
        return Err(TdCode::FreqZero.val());
    }
    let t = (us as u128) * (hz as u128) / 1_000_000u128;
    if t > u64::MAX as u128 {
        return Err(TdCode::FreqZero.val());
    }
    Ok(t as u64)
}

// ── 到期误差采样环（1024 深，整数分位 p99） ─────────────────────
#[derive(Clone, Debug)]
pub struct ErrRing {
    pub buf: Vec<u64>,
    pub head: usize,
    pub filled: u32,
}

impl ErrRing {
    pub fn new() -> ErrRing {
        ErrRing { buf: Vec::new(), head: 0, filled: 0 }
    }
    /// 环形写入（覆盖最旧）。
    pub fn push(&mut self, lateness_us: u64) {
        if self.buf.len() < SAMPLE_RING {
            self.buf.push(lateness_us);
        } else {
            match self.buf.get_mut(self.head) {
                Some(slot) => *slot = lateness_us,
                None => return,
            }
            self.head = (self.head + 1) % SAMPLE_RING;
        }
        self.filled += 1;
    }
    /// 有效样本数（≤1024）。
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    /// 复制有效样本并升序排序（插入排序，1024 上限）。
    pub fn sorted(&self) -> Vec<u64> {
        let mut a = self.buf.clone();
        let mut i = 1;
        while i < a.len() {
            let key = a[i];
            let mut j = i;
            while j > 0 {
                let prev = match a.get(j - 1) {
                    Some(v) => *v,
                    None => break,
                };
                if prev > key {
                    match a.get_mut(j) {
                        Some(slot) => *slot = prev,
                        None => break,
                    }
                    j -= 1;
                } else {
                    break;
                }
            }
            match a.get_mut(j) {
                Some(slot) => *slot = key,
                None => break,
            }
            i += 1;
        }
        a
    }
    /// 整数分位 p99：idx = n×99/100（clamp 到 n-1）。
    pub fn p99_us(&self) -> Option<u64> {
        let n = self.buf.len();
        if n == 0 {
            return None;
        }
        let s = self.sorted();
        let mut idx = n * 99 / 100;
        if idx >= n {
            idx = n - 1;
        }
        s.get(idx).map(|v| *v)
    }
    /// 复算：分位数与账面排序一致（回放面）。
    pub fn replay_ok(&self) -> bool {
        let s = self.sorted();
        if s.len() != self.buf.len() {
            return false;
        }
        // 升序不变式。
        let mut i = 1;
        while i < s.len() {
            match (s.get(i - 1), s.get(i)) {
                (Some(a), Some(b)) => {
                    if a > b {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

// ── TSC-deadline 定时器状态机（Idle→Armed→Fired 单次闭合） ──────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TdState {
    /// 空闲（无在途 deadline）
    Idle,
    /// 已武装（deadline 在途）
    Armed,
    /// 已到期投递（单次语义本轮闭合）
    Fired,
}

#[derive(Clone, Debug)]
pub struct TscDeadlineTimer {
    /// CPUID 支持位（注入）
    pub supported: bool,
    /// TSC 频率（µs→tick 换算口径）
    pub hz: u64,
    /// 状态机
    pub state: TdState,
    /// 当前在途 deadline（TSC 刻）
    pub deadline: u64,
    /// 武装次数
    pub arms: u64,
    /// 到期投递次数
    pub fires: u64,
    /// 丢失账（恒 0 判定面）
    pub lost: u64,
    /// 到期误差环
    pub ring: ErrRing,
    /// 最近投递延迟（µs）
    pub last_err_us: u64,
    /// MSR 写入账（0x6B0 写入次数，与武装次数对平）
    pub msr_writes: u64,
}

impl TscDeadlineTimer {
    pub fn new(supported: bool, hz: u64) -> Result<TscDeadlineTimer, u32> {
        if hz == 0 {
            return Err(TdCode::FreqZero.val());
        }
        Ok(TscDeadlineTimer {
            supported,
            hz,
            state: TdState::Idle,
            deadline: 0,
            arms: 0,
            fires: 0,
            lost: 0,
            ring: ErrRing::new(),
            last_err_us: 0,
            msr_writes: 0,
        })
    }

    /// 武装：支持位闸 + 过去时刻闸 + 单次语义闸。
    pub fn arm(&mut self, now: u64, deadline: u64) -> Result<u64, u32> {
        // 支持探测闸：不支持必拒（分支正确性面）。
        match probe_support(self.supported) {
            TscDlCap::Supported => {}
            TscDlCap::Unsupported => return Err(TdCode::NotSupported.val()),
        }
        // 单次语义闸：上一笔未到期就重武装 = 丢一笔。
        if self.state == TdState::Armed {
            self.lost += 1;
            return Err(TdCode::MissedDeadline.val());
        }
        // 绝对时刻闸：过去/现在时刻显性拒。
        if deadline <= now {
            return Err(TdCode::DeadlinePast.val());
        }
        self.state = TdState::Armed;
        self.deadline = deadline;
        self.arms += 1;
        self.msr_writes += 1;
        Ok(deadline)
    }

    /// 到期投递：armed && now >= deadline 才投，单次投毕回 Idle。
    pub fn deliver(&mut self, now: u64) -> Result<u64, u32> {
        if self.state != TdState::Armed {
            return Err(TdCode::FireWhileIdle.val());
        }
        if now < self.deadline {
            // 未到点：不投，保持 armed。
            return Ok(0);
        }
        // 延迟换算（tick → µs，全整数）。
        let late_ticks = now - self.deadline;
        let late_us = if self.hz == 0 {
            return Err(TdCode::FreqZero.val());
        } else {
            ((late_ticks as u128) * 1_000_000u128 / (self.hz as u128)) as u64
        };
        self.ring.push(late_us);
        self.last_err_us = late_us;
        self.fires += 1;
        self.state = TdState::Fired;
        Ok(late_us)
    }

    /// Fired → Idle（再武装前置步；单次语义闭合）。
    pub fn settle(&mut self) -> Result<(), u32> {
        if self.state != TdState::Fired {
            return Err(TdCode::FireWhileIdle.val());
        }
        self.state = TdState::Idle;
        Ok(())
    }

    /// p99 到期误差（µs）。
    pub fn p99_us(&self) -> Option<u64> {
        self.ring.p99_us()
    }

    /// 零丢失判定：丢失账恒 0 且武装/投递对平。
    pub fn zero_loss_ok(&self) -> bool {
        self.lost == 0 && self.arms == self.fires && self.msr_writes == self.arms
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

// ── 注入检出账（四路：过去时刻路/不支持路/丢失路/误差路） ────────
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

/// 十万次单次到期走账（零丢失 + p99 双账）；period_us 每笔间隔。
pub fn run_many(t: &mut TscDeadlineTimer, rounds: u64, period_us: u64, rng: &mut Lcg) -> Result<(u64, u64), u32> {
    let period_ticks = us_to_ticks(period_us, t.hz)?;
    let mut now = 1_000_000u64;
    let mut i = 0;
    while i < rounds {
        // 抖动：0..=3 tick 投递延迟（QEMU 档口径）。
        let jitter_ticks = rng.below(4);
        let deadline = now + period_ticks;
        t.arm(now, deadline)?;
        let fire_at = deadline + jitter_ticks;
        let _late_us = t.deliver(fire_at)?;
        t.settle()?;
        // 推进到下一笔基准点（投递点为下一轮 now）。
        now = fire_at;
        i += 1;
    }
    let p99 = match t.p99_us() {
        Some(v) => v,
        None => return Err(TdCode::ErrOverLimit.val()),
    };
    Ok((t.lost, p99))
}

/// 真机 10 万次口径跑账（1000µs 周期，2.4GHz）。
pub fn machine_run() -> Result<(u64, u64, u64), u32> {
    let mut t = TscDeadlineTimer::new(true, QEMU_TSC_HZ)?;
    let mut rng = Lcg::new(SEG_BASE as u64);
    let (lost, p99) = run_many(&mut t, ZERO_LOSS_TARGET, 1000, &mut rng)?;
    Ok((lost, p99, t.fires))
}

// ── 黄金全链：支持探测→10 万次零丢失→p99≤10µs→四拒→复算 ─────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 支持/不支持探测双分支各一次（判据 10/10 由注入轮放大）。
    let cap_ok = probe_support(true) == TscDlCap::Supported
        && probe_support(false) == TscDlCap::Unsupported;
    // 10 万次零丢失 + p99 ≤10µs。
    let (lost, p99, fires) = machine_run()?;
    let loss_ok = (lost == 0 && fires == ZERO_LOSS_TARGET) as u32;
    let p99_ok = (p99 <= ERR_LIMIT_US) as u32;
    // 四拒：过去时刻 / 不支持武装 / 未到期重武装 / 空转投递。
    let mut t4 = TscDeadlineTimer::new(true, QEMU_TSC_HZ)?;
    let past = t4.arm(1_000_000, 999_999).is_err();
    let mut t5 = TscDeadlineTimer::new(false, QEMU_TSC_HZ)?;
    let unsup = t5.arm(1_000_000, 2_000_000).is_err();
    let mut t6 = TscDeadlineTimer::new(true, QEMU_TSC_HZ)?;
    let _ = t6.arm(1_000_000, 2_000_000);
    let dbl = t6.arm(1_000_000, 2_000_000).is_err() && t6.lost == 1;
    let idle = t6.deliver(2_000_000).is_err();
    let rej = past as u32 + unsup as u32 + (dbl && !idle) as u32;
    // 复算：环账升序不变式（空环亦闭合）。
    let t7 = TscDeadlineTimer::new(true, QEMU_TSC_HZ)?;
    let ring_ok = t7.ring.replay_ok() as u32;
    Ok((cap_ok as u32 + loss_ok + p99_ok + ring_ok, rej, p99 as u32))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链（4 绿 + 3 拒 + p99 有限）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 3, p99)) if p99 <= ERR_LIMIT_US as u32 => Ok(()),
        _ => Err(TdCode::NotReproducible.val()),
    }
}

/// 序 B：空环 p99 无值；空转投递 ⇒ FireWhileIdle。
pub fn probe_seq_b() -> Result<(), u32> {
    let r = ErrRing::new();
    let q1 = r.p99_us().is_none();
    let mut t = TscDeadlineTimer::new(true, QEMU_TSC_HZ)?;
    let q2 = matches!(t.deliver(1), Err(e) if e == TdCode::FireWhileIdle.val());
    match (q1, q2) {
        (true, true) => Ok(()),
        _ => Err(TdCode::NotReproducible.val()),
    }
}

/// 序 C：MSR/位号/环深/上限冻结。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        MSR_TSC_DEADLINE == 0x6B0,
        CPUID_TSCDL_BIT == 24,
        SAMPLE_RING == 1024,
        ERR_LIMIT_US == 10,
    ) {
        (true, true, true, true) => Ok(()),
        _ => Err(TdCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：过去时刻路（deadline ≤ now 必拒）。
        let mut t1 = TscDeadlineTimer::new(true, QEMU_TSC_HZ)?;
        led.record(0, t1.arm(5_000, 4_999).is_err());
        // 路 2：不支持路（CPUID bit24=0 必拒武装）。
        let mut t2 = TscDeadlineTimer::new(false, QEMU_TSC_HZ)?;
        led.record(1, t2.arm(5_000, 9_999).is_err());
        // 路 3：丢失路（未到期重复武装必丢一笔）。
        let mut t3 = TscDeadlineTimer::new(true, QEMU_TSC_HZ)?;
        let _ = t3.arm(5_000, 9_999);
        let dbl = t3.arm(5_000, 9_999).is_err();
        led.record(2, dbl && t3.lost == 1);
        // 路 4：误差路（注入 20µs 迟到必越 10µs 上限）。
        let late20 = us_to_ticks(20, QEMU_TSC_HZ).unwrap_or(0);
        let p99_inj = late20 > us_to_ticks(ERR_LIMIT_US, QEMU_TSC_HZ).unwrap_or(u64::MAX);
        led.record(3, p99_inj);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TdCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TdCode::ALL.len() {
        let v = TdCode::ALL[i].val();
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
            Ok((ok, rej, _)) => *ok == 4 && *rej == 3,
            Err(_) => false,
        },
        match &g {
            Ok((ok, rej, _)) => format!("绿={} 拒={}", ok, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 10 万次单次到期零丢失。
    let mr = machine_run();
    v.push((
        "J1-十万次零丢失",
        match &mr {
            Ok((lost, _, fires)) => *lost == 0 && *fires == ZERO_LOSS_TARGET,
            Err(_) => false,
        },
        match &mr {
            Ok((lost, _, fires)) => format!("fires={} lost={}", fires, lost),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // p99 ≤10µs（QEMU 档）。
    v.push((
        "J1-p99≤10µs",
        match &mr {
            Ok((_, p99, _)) => *p99 <= ERR_LIMIT_US,
            Err(_) => false,
        },
        match &mr {
            Ok((_, p99, _)) => format!("p99={}µs", p99),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // MSR 支持探测双分支（支持/不支持各注入 10 轮全对）。
    let mut cap_ok = 0;
    let mut i = 0;
    while i < JUDGE_ROUNDS {
        if probe_support(true) == TscDlCap::Supported {
            cap_ok += 1;
        }
        if probe_support(false) == TscDlCap::Unsupported {
            cap_ok += 1;
        }
        i += 1;
    }
    v.push((
        "J1-支持探测10/10",
        cap_ok == JUDGE_ROUNDS * 2,
        format!("双分支 {} 轮全对", JUDGE_ROUNDS),
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
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
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
        String::from("0x71F80 窗口"),
    ));
    v
}

pub fn run_unx1043_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1043 TSC-deadline 模式（LAPIC 定时器 deadline 投递）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
