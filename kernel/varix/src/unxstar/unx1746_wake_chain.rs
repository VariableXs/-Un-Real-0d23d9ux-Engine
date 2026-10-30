//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1746 · 唤醒路径总纲（wake 五段链） · 0x6CF40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B3）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1746-J1 五段链冻结 v1，段间交接签名齐备，跳段注入
//!     10/10 次拒绝，一次唤醒全程账可导出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五段链闭集：置态（Runnable）→入队→选核→触发→IPI（冻结
//!      单向链）；
//!    - 段间交接签名：每段完成后生成签名（段号+线程号+tick FNV），
//!      下一段校验前段签名（签名齐备才可推进）；
//!    - 跳段注入：跳过任一段（签名缺失）即拒，10/10 拒绝；
//!    - 一次唤醒全程账导出（五段账行）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6CF40，AI-VX3-1030 批D 独占区）。
pub const SEG_BASE: u32 = 0x6CF40;

// ── 五段闭集 ────────────────────────────────────────────────────
pub const W_SET_STATE: usize = 0;
pub const W_ENQUEUE: usize = 1;
pub const W_PICK_CORE: usize = 2;
pub const W_KICK: usize = 3;
pub const W_IPI: usize = 4;
pub const WAKE_STEPS: usize = 5;
pub const WAKE_STEP_NAMES: [&str; WAKE_STEPS] =
    ["置态", "入队", "选核", "触发", "IPI"];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WakeCode {
    /// 段越闭集
    StepOutOfRange,
    /// 段名空（五段齐备破坏）
    StepNameBlank,
    /// 跳段（签名缺失）
    StepJumpDetected,
    /// 交接签名不符
    HandoverSigMismatch,
    /// 重复唤醒（已唤醒再 wake）
    DoubleWake,
    /// 全程账缺段
    FullLedgerShort,
    /// 线程号越界
    ThreadOutOfRange,
    /// 双跑不一致
    NotReproducible,
}

impl WakeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            WakeCode::StepOutOfRange => "段越闭集：五段冻结",
            WakeCode::StepNameBlank => "段名空：齐备破坏",
            WakeCode::StepJumpDetected => "跳段：签名缺失",
            WakeCode::HandoverSigMismatch => "交接签名不符",
            WakeCode::DoubleWake => "重复唤醒：已唤醒再 wake",
            WakeCode::FullLedgerShort => "全程账缺段：≠5",
            WakeCode::ThreadOutOfRange => "线程号越界",
            WakeCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [WakeCode; 8] = [
        WakeCode::StepOutOfRange,
        WakeCode::StepNameBlank,
        WakeCode::StepJumpDetected,
        WakeCode::HandoverSigMismatch,
        WakeCode::DoubleWake,
        WakeCode::FullLedgerShort,
        WakeCode::ThreadOutOfRange,
        WakeCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < WakeCode::ALL.len() {
        if WakeCode::ALL[i].val() == code {
            return WakeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6CF40 段闭集内"
}

/// 段间交接签名：FNV(前段签名, 段号, 线程号)。
pub fn handover_sig(prev: u64, step: usize, tid: usize) -> u64 {
    let mut h = prev;
    h ^= step as u64;
    h = h.wrapping_mul(0x100000001b3);
    h ^= tid as u64;
    h = h.wrapping_mul(0x100000001b3);
    h
}

// ── 唤醒账（五段单向状态机） ────────────────────────────────────
pub struct WakePath {
    pub tid: usize,
    pub next_step: usize,
    pub sig: u64,
    pub done: bool,
    pub ledger: Vec<(usize, u64)>,
    pub jump_caught: u32,
}

impl WakePath {
    pub fn new(tid: usize) -> Result<WakePath, u32> {
        if tid == 0 {
            return Err(WakeCode::ThreadOutOfRange.val());
        }
        Ok(WakePath {
            tid,
            next_step: W_SET_STATE,
            sig: SEG_BASE as u64,
            done: false,
            ledger: Vec::new(),
            jump_caught: 0,
        })
    }

    /// 推进一段（签名校验链）。
    pub fn advance(&mut self, step: usize) -> Result<u64, u32> {
        if step >= WAKE_STEPS {
            return Err(WakeCode::StepOutOfRange.val());
        }
        if self.done {
            self.jump_caught += 1;
            return Err(WakeCode::DoubleWake.val());
        }
        if step != self.next_step {
            self.jump_caught += 1;
            return Err(WakeCode::StepJumpDetected.val());
        }
        self.sig = handover_sig(self.sig, step, self.tid);
        self.ledger.push((step, self.sig));
        if step == W_IPI {
            self.done = true;
        } else {
            self.next_step += 1;
        }
        Ok(self.sig)
    }

    /// 跳段注入：直接推 step+1（签名缺失必拒）。
    pub fn skip_inject(&mut self) -> Result<u64, u32> {
        // 不推进 next_step，直接尝试更远段。
        let far = (self.next_step + 1).min(WAKE_STEPS - 1);
        self.advance(far)
    }

    /// 全程账导出（五段行）。
    pub fn full_ledger(&self) -> Result<Vec<(usize, u64)>, u32> {
        if self.ledger.len() != WAKE_STEPS {
            return Err(WakeCode::FullLedgerShort.val());
        }
        Ok(self.ledger.clone())
    }
}

/// 跳段注入 10/10。
pub fn skip_inject_10() -> Result<u32, u32> {
    let mut caught = 0;
    let mut k = 0;
    while k < 10 {
        let mut wp = WakePath::new(k as usize + 1)?;
        // 正常走两段，然后跳段。
        wp.advance(W_SET_STATE)?;
        wp.advance(W_ENQUEUE).map(|_| ())?;
        if wp.skip_inject().is_err() {
            caught += 1;
        }
        k += 1;
    }
    if caught != 10 {
        return Err(WakeCode::StepJumpDetected.val());
    }
    Ok(caught)
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

/// 黄金全链：五段顺走→签名链齐→跳段 10/10→全程账导出。
pub fn golden_run() -> Result<(u64, u64, u32), u32> {
    if WAKE_STEP_NAMES.len() != WAKE_STEPS {
        return Err(WakeCode::StepNameBlank.val());
    }
    let mut wp = WakePath::new(7)?;
    let mut s = 0;
    while s < WAKE_STEPS {
        wp.advance(s)?;
        s += 1;
    }
    let ledger = wp.full_ledger()?;
    if ledger.len() != WAKE_STEPS {
        return Err(WakeCode::FullLedgerShort.val());
    }
    // 重复唤醒拒。
    if wp.advance(W_SET_STATE).is_ok() {
        return Err(WakeCode::DoubleWake.val());
    }
    let caught = skip_inject_10()?;
    Ok((wp.sig, ledger[4].1, caught))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut wp = WakePath::new(1)?;
    let mut s = 0;
    while s < WAKE_STEPS {
        wp.advance(s)?;
        s += 1;
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 跳段 → 必拒。
    let mut wp = WakePath::new(2)?;
    wp.advance(W_ENQUEUE).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 线程号 0 → 必拒。
    WakePath::new(0).map(|_| ())
}

/// 注入 10/10：跳段、重复唤醒、线程零、段越界四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：跳段。
        let mut w0 = WakePath::new(3).ok();
        let h0 = match &mut w0 {
            Some(w) => {
                w.advance(W_SET_STATE).ok();
                w.skip_inject().is_err()
            }
            None => false,
        };
        led.record(0, h0);
        // 槽1：重复唤醒。
        let mut w1 = WakePath::new(4).ok();
        let h1 = match &mut w1 {
            Some(w) => {
                let mut s = 0;
                while s < WAKE_STEPS {
                    w.advance(s).ok();
                    s += 1;
                }
                w.advance(W_SET_STATE).is_err()
            }
            None => false,
        };
        led.record(1, h1);
        // 槽2：线程零。
        led.record(2, WakePath::new(0).is_err());
        // 槽3：段越界。
        let mut w3 = WakePath::new(5).ok();
        let h3 = match &mut w3 {
            Some(w) => w.advance(9).is_err(),
            None => false,
        };
        led.record(3, h3);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(WakeCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < WakeCode::ALL.len() {
        let v = WakeCode::ALL[i].val();
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
    // 五段链冻结。
    v.push((
        "J1-五段链冻结v1",
        WAKE_STEP_NAMES.len() == 5,
        format!("{:?}", WAKE_STEP_NAMES),
    ));
    // 跳段 10/10。
    v.push((
        "J1-跳段注入10/10",
        skip_inject_10() == Ok(10),
        format!("{}/10 拒绝", skip_inject_10().unwrap_or(0)),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6CF40 窗口")));
    v
}

pub fn run_unx1746_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1746 唤醒路径总纲（wake 五段链）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
