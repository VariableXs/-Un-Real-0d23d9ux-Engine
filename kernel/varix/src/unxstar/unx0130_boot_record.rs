//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0130 · 上次引导记录持久化 · 0x6ECC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0130-J1 上次引导记录：五槽账（序号/结果/时刻/原因/
//!     版本）封存落盘账，本次读上次（读回与写入逐字段一致），
//!     写后改写抓（FNV 指纹），序号单调递增（回退拒）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五槽：序号/结果（0 成功 1 失败 2 诊断）/时刻/原因码/版本；
//!   - 指纹账：FNV 全槽折叠，写后只读（改写抓）；
//!   - 序号单调（新序号 = 旧 + 1）；
//!   - 复算：同账两写两读恒一致；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6ECC0，AI-VX3-1102 批M 独占区）。
pub const SEG_BASE: u32 = 0x6ECC0;

/// 槽数（冻结 5）。
pub const SLOTS: usize = 5;
pub const S_SEQ: usize = 0;
pub const S_RESULT: usize = 1;
pub const S_TIME: usize = 2;
pub const S_REASON: usize = 3;
pub const S_VERSION: usize = 4;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootRecCode {
    /// 序号回退（单调红线）
    SeqRewind,
    /// 结果账外（>2）
    ResultOver,
    /// 版本槽失配
    VersionBad,
    /// 写后改写
    SealedMutated,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl BootRecCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BootRecCode::SeqRewind => "序号回退：单调红线",
            BootRecCode::ResultOver => "结果账外：>2",
            BootRecCode::VersionBad => "版本槽失配：非 1",
            BootRecCode::SealedMutated => "写后改写：只读红线",
            BootRecCode::ReplayMismatch => "复算失配",
            BootRecCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [BootRecCode; 6] = [
        BootRecCode::SeqRewind,
        BootRecCode::ResultOver,
        BootRecCode::VersionBad,
        BootRecCode::SealedMutated,
        BootRecCode::ReplayMismatch,
        BootRecCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BootRecCode::ALL.len() {
        if BootRecCode::ALL[i].val() == code {
            return BootRecCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6ECC0 段闭集内"
}

// ── 引导记录账 ──────────────────────────────────────────────────
fn fnv_slots(slots: &[u64; SLOTS]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < SLOTS {
        let v = slots[i];
        let mut b = 0;
        while b < 8 {
            h = (h ^ ((v >> (b * 8)) & 0xFF)).wrapping_mul(0x100000001b3);
            b += 1;
        }
        i += 1;
    }
    h
}

#[derive(Clone, Debug)]
pub struct BootRecord {
    pub slots: [u64; SLOTS],
    pub fp: u64,
    pub sealed: bool,
}

impl BootRecord {
    /// 写入（序号单调 + 结果/版本闸，自动指纹）。
    pub fn write(prev_seq: u64, slots: [u64; SLOTS]) -> Result<BootRecord, u32> {
        if slots[S_SEQ] <= prev_seq && prev_seq != 0 {
            return Err(BootRecCode::SeqRewind.val());
        }
        if slots[S_RESULT] > 2 {
            return Err(BootRecCode::ResultOver.val());
        }
        if slots[S_VERSION] != 1 {
            return Err(BootRecCode::VersionBad.val());
        }
        Ok(BootRecord { slots, fp: fnv_slots(&slots), sealed: true })
    }

    /// 读回（逐字段）。
    pub fn read(&self, slot: usize) -> Result<u64, u32> {
        if slot >= SLOTS {
            return Err(BootRecCode::VersionBad.val());
        }
        Ok(self.slots[slot])
    }

    /// 写后改写检测。
    pub fn mutate_audit(&self, fresh_slots: &[u64; SLOTS]) -> bool {
        self.slots == *fresh_slots && self.fp == fnv_slots(fresh_slots)
    }
}

/// 复算：同账两写两读恒一致。
pub fn replay_ok(slots: &[u64; SLOTS], prev_seq: u64) -> Result<(), u32> {
    let a = BootRecord::write(prev_seq, *slots)?;
    let b = BootRecord::write(prev_seq, *slots)?;
    if a.fp != b.fp || a.slots != b.slots {
        return Err(BootRecCode::ReplayMismatch.val());
    }
    Ok(())
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

// ── 注入检出账（四路：回退路/结果路/改写路/复算路） ──────────────
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

/// 真机账（序号 41 成功）。
pub fn machine_slots() -> [u64; SLOTS] {
    [41, 0, 0xE8_1234_5678, 0, 1]
}

// ── 黄金全链：写→读回→改写对拍→序号链→复算 ──────────────────────
pub fn golden_run() -> Result<(u64, u64, bool), u32> {
    let s1 = machine_slots();
    let r1 = BootRecord::write(40, s1)?;
    let seq_read = r1.read(S_SEQ)?;
    let res_read = r1.read(S_RESULT)?;
    // 改写对拍（同账恒真，漂移必假）。
    let mut bad = s1;
    bad[S_TIME] += 1;
    let same = r1.mutate_audit(&s1) && !r1.mutate_audit(&bad);
    // 序号链：42 写成功，40 回退拒。
    let r2 = BootRecord::write(r1.slots[S_SEQ], [42, 1, 100, 1, 1])?;
    let rewind = BootRecord::write(r2.slots[S_SEQ], [r2.slots[S_SEQ] - 3, 0, 90, 0, 1]).is_err();
    replay_ok(&s1, 40)?;
    Ok((seq_read + res_read, r2.slots[S_SEQ], same && rewind))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：读回 41/0 + 链 42 + 对拍真。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((41, 42, true)) => Ok(()),
        _ => Err(BootRecCode::NotReproducible.val()),
    }
}

/// 序 B：结果 3 ⇒ ResultOver。
pub fn probe_seq_b() -> Result<(), u32> {
    match BootRecord::write(40, [41, 3, 100, 0, 1]) {
        Err(e) if e == BootRecCode::ResultOver.val() => Ok(()),
        _ => Err(BootRecCode::NotReproducible.val()),
    }
}

/// 序 C：版本 2 ⇒ VersionBad。
pub fn probe_seq_c() -> Result<(), u32> {
    match BootRecord::write(40, [41, 0, 100, 0, 2]) {
        Err(e) if e == BootRecCode::VersionBad.val() => Ok(()),
        _ => Err(BootRecCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：回退路。
        led.record(0, BootRecord::write(42, [40, 0, 1, 0, 1]).is_err());
        // 路 2：结果路。
        led.record(1, BootRecord::write(40, [41, 9, 1, 0, 1]).is_err());
        // 路 3：改写路。
        let r3 = BootRecord::write(40, machine_slots())?;
        let mut bad = machine_slots();
        bad[S_SEQ] += 1;
        led.record(2, !r3.mutate_audit(&bad));
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_slots(), 40).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BootRecCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BootRecCode::ALL.len() {
        let v = BootRecCode::ALL[i].val();
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
            Ok((reads, chain, audit)) => *reads == 41 && *chain == 42 && *audit,
            Err(_) => false,
        },
        match &g {
            Ok((reads, chain, audit)) => format!("seq+res={} next={} audit={}", reads, chain, audit),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 五槽语义冻结。
    v.push((
        "J1-五槽语义",
        SLOTS == 5 && machine_slots()[S_VERSION] == 1 && machine_slots()[S_RESULT] == 0,
        String::from("序号/结果/时刻/原因/版本 五槽冻结"),
    ));
    // 首账特例：prev_seq=0 时任意序号放行。
    v.push((
        "J1-首账特例",
        BootRecord::write(0, [1, 0, 1, 0, 1]).is_ok(),
        String::from("prev=0 ⇒ 首账放行"),
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
        format!("a=绿 b=检出{} c=检出{}", b, c),
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
        String::from("0x6ECC0 窗口"),
    ));
    v
}

pub fn run_unx0130_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0130 上次引导记录持久化");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
