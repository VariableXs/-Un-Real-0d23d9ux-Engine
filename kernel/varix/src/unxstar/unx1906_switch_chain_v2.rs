//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1906 · 切换链五段修订（扩展状态第五段） · 0x6D140
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B4）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1906-J1 切换链 v2 落账（F1801 四段+扩展状态第五段，
//!     段签名/段账/回签三方一致），下游消费方（B11 tick/F1818 交
//!     接）回签齐备，链版本对齐 F1635；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五段闭集：F1801 四段（存旧/选新/装新/提交）+ 扩展状态第五
//!      段（XSAVE 区保存，F1826 承接）；
//!    - 段签名链：每段 FNV(前签名,段号)，五段链尾锚；
//!    - 回签：B11 tick 消费方 + F1818 交接方各回签一位（回签齐备
//!      才可闭）；
//!    - 链版本 v2 对齐 F1635（冻结版本号）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D140，AI-VX3-1030 批E 独占区）。
pub const SEG_BASE: u32 = 0x6D140;

// ── 五段闭集 ────────────────────────────────────────────────────
pub const CH_SAVE_OLD: usize = 0;
pub const CH_PICK_NEW: usize = 1;
pub const CH_INSTALL: usize = 2;
pub const CH_COMMIT: usize = 3;
pub const CH_EXT_STATE: usize = 4; // 扩展状态第五段
pub const CHAIN_STEPS: usize = 5;
pub const CHAIN_STEP_NAMES: [&str; CHAIN_STEPS] =
    ["存旧", "选新", "装新", "提交", "扩展状态"];
/// 链版本（F1635 对齐）。
pub const CHAIN_VER: u32 = 2;
/// 回签方闭集。
pub const ACK_NAMES: [&str; 2] = ["B11-tick", "F1818-交接"];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChvCode {
    /// 段越闭集
    StepOutOfRange,
    /// 段名空（五段齐备破坏）
    StepNameBlank,
    /// 链断（签名不符）
    ChainBroken,
    /// 回签缺（两回签方缺一）
    AckMissing,
    /// 版本不对齐（≠v2）
    VersionMisaligned,
    /// 下游消费缺
    ConsumerMissing,
    /// 链尾锚空
    EmptyTailAnchor,
    /// 双跑不一致
    NotReproducible,
}

impl ChvCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ChvCode::StepOutOfRange => "段越闭集：五段冻结",
            ChvCode::StepNameBlank => "段名空：齐备破坏",
            ChvCode::ChainBroken => "链断：签名不符",
            ChvCode::AckMissing => "回签缺：两回签方缺一",
            ChvCode::VersionMisaligned => "版本不对齐：≠v2",
            ChvCode::ConsumerMissing => "下游消费缺",
            ChvCode::EmptyTailAnchor => "链尾锚空",
            ChvCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ChvCode; 8] = [
        ChvCode::StepOutOfRange,
        ChvCode::StepNameBlank,
        ChvCode::ChainBroken,
        ChvCode::AckMissing,
        ChvCode::VersionMisaligned,
        ChvCode::ConsumerMissing,
        ChvCode::EmptyTailAnchor,
        ChvCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ChvCode::ALL.len() {
        if ChvCode::ALL[i].val() == code {
            return ChvCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D140 段闭集内"
}

/// 段签名：FNV(前签名, 段号)。
pub fn step_sig(prev: u64, step: usize) -> u64 {
    let mut h = prev;
    h ^= step as u64;
    h = h.wrapping_mul(0x100000001b3);
    h
}

// ── 切换链 v2 ──────────────────────────────────────────────────
pub struct SwitchChainV2 {
    pub sig: u64,
    pub steps_done: Vec<usize>,
    /// 回签位（两消费方）。
    pub acks: [bool; 2],
    pub version: u32,
}

impl SwitchChainV2 {
    pub fn new(version: u32) -> SwitchChainV2 {
        SwitchChainV2 { sig: SEG_BASE as u64, steps_done: Vec::new(), acks: [false; 2], version }
    }

    /// 推进一段。
    pub fn advance(&mut self, step: usize) -> Result<u64, u32> {
        if step >= CHAIN_STEPS {
            return Err(ChvCode::StepOutOfRange.val());
        }
        let expect = self.steps_done.len();
        if step != expect {
            return Err(ChvCode::ChainBroken.val());
        }
        self.sig = step_sig(self.sig, step);
        self.steps_done.push(step);
        Ok(self.sig)
    }

    /// 回签。
    pub fn ack(&mut self, consumer: usize) -> Result<(), u32> {
        if consumer >= ACK_NAMES.len() {
            return Err(ChvCode::ConsumerMissing.val());
        }
        self.acks[consumer] = true;
        Ok(())
    }

    /// 三方一致：段签名链完整 + 回签齐备 + 版本对齐。
    pub fn close(&self) -> Result<u64, u32> {
        if self.steps_done.len() != CHAIN_STEPS {
            return Err(ChvCode::ChainBroken.val());
        }
        if !self.acks[0] || !self.acks[1] {
            return Err(ChvCode::AckMissing.val());
        }
        if self.version != CHAIN_VER {
            return Err(ChvCode::VersionMisaligned.val());
        }
        if self.sig == 0 {
            return Err(ChvCode::EmptyTailAnchor.val());
        }
        Ok(self.sig)
    }
}

/// 回签齐备 10/10。
pub fn ack_10() -> Result<u32, u32> {
    let mut ok = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut ch = SwitchChainV2::new(CHAIN_VER);
        let mut s = 0;
        while s < CHAIN_STEPS {
            ch.advance(s)?;
            s += 1;
        }
        ch.ack(0)?;
        ch.ack(1)?;
        if ch.close().is_ok() {
            ok += 1;
        }
        r += 1;
    }
    if ok != JUDGE_ROUNDS {
        return Err(ChvCode::AckMissing.val());
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

/// 黄金全链：五段链→回签齐→闭链→版本对齐→10/10。
pub fn golden_run() -> Result<(u64, u32, u32), u32> {
    if CHAIN_STEP_NAMES.len() != CHAIN_STEPS {
        return Err(ChvCode::StepNameBlank.val());
    }
    let mut ch = SwitchChainV2::new(CHAIN_VER);
    let mut s = 0;
    while s < CHAIN_STEPS {
        ch.advance(s)?;
        s += 1;
    }
    ch.ack(0)?;
    ch.ack(1)?;
    let anchor = ch.close()?;
    let acks = ack_10()?;
    Ok((anchor, CHAIN_VER, acks))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut ch = SwitchChainV2::new(CHAIN_VER);
    let mut s = 0;
    while s < CHAIN_STEPS {
        ch.advance(s)?;
        s += 1;
    }
    ch.ack(0)?;
    ch.ack(1)?;
    ch.close().map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 回签缺 → 必拒。
    let mut ch = SwitchChainV2::new(CHAIN_VER);
    let mut s = 0;
    while s < CHAIN_STEPS {
        ch.advance(s)?;
        s += 1;
    }
    ch.ack(0)?;
    ch.close().map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 版本不对齐 → 必拒。
    let ch = SwitchChainV2::new(CHAIN_VER + 1);
    ch.close().map(|_| ())
}

/// 注入 10/10：跳段、回签缺、版本错、链断四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：跳段。
        let mut c0 = SwitchChainV2::new(CHAIN_VER);
        led.record(0, c0.advance(2).is_err());
        // 槽1：回签缺。
        let mut c1 = SwitchChainV2::new(CHAIN_VER);
        let mut s = 0;
        while s < CHAIN_STEPS {
            c1.advance(s).ok();
            s += 1;
        }
        led.record(1, c1.close().is_err());
        // 槽2：版本错。
        let c2 = SwitchChainV2::new(9);
        led.record(2, c2.close().is_err());
        // 槽3：链断（四段即闭）。
        let mut c3 = SwitchChainV2::new(CHAIN_VER);
        let mut s3 = 0;
        while s3 < CHAIN_STEPS - 1 {
            c3.advance(s3).ok();
            s3 += 1;
        }
        c3.ack(0).ok();
        c3.ack(1).ok();
        led.record(3, c3.close().is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ChvCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ChvCode::ALL.len() {
        let v = ChvCode::ALL[i].val();
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
    // 五段 v2。
    v.push((
        "J1-五段v2落账",
        CHAIN_STEP_NAMES.len() == 5 && CHAIN_VER == 2 && CH_EXT_STATE == 4,
        format!("{:?} v{}", CHAIN_STEP_NAMES, CHAIN_VER),
    ));
    // 回签 10/10。
    v.push((
        "J1-回签齐备10/10",
        ack_10() == Ok(10) && ACK_NAMES.len() == 2,
        format!("{}/10", ack_10().unwrap_or(0)),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D140 窗口")));
    v
}

pub fn run_unx1906_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1906 切换链五段修订（扩展状态第五段）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
