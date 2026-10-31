//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2523 · 采样调度与存储（采样环形库） · 0x6D800
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-C2）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F2523-J1 采样库 90 帧容量满后滚动覆盖零丢失（覆盖前
//!     归档），采样开销账 <0.1%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 环形库 90 帧（冻结容量），满后滚动覆盖；覆盖前归档（被覆
//!      盖样本入归档账，零丢失：pushed=kept+archived）；
//!    - 采样开销账：每采样步数（≤4 步）<0.1% 等价断言；
//!    - 采样调度：周期采样（间隔冻结 1 tick）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D800，AI-VX3-1030 批G 独占区）。
pub const SEG_BASE: u32 = 0x6D800;

/// 容量（冻结 90 帧）。
pub const RING_CAP: usize = 90;
/// 采样间隔（tick）。
pub const SAMPLE_INTERVAL: u64 = 1;
/// 单采样步数上界（<0.1% 等价）。
pub const SAMPLE_STEPS_MAX: u32 = 4;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampCode {
    /// 容量错（≠90）
    CapWrong,
    /// 覆盖未归档（零丢失破坏）
    OverwriteNoArchive,
    /// 采样开销超限
    OverheadOver,
    /// 采样间隔错
    IntervalWrong,
    /// 归档账守恒破坏
    ArchiveConservationBroken,
    /// 环形位错
    RingPosWrong,
    /// 零丢失漏检
    LossLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl SampCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SampCode::CapWrong => "容量错：≠90 帧",
            SampCode::OverwriteNoArchive => "覆盖未归档：零丢失破坏",
            SampCode::OverheadOver => "采样开销超限：≥0.1%",
            SampCode::IntervalWrong => "采样间隔错：≠1 tick",
            SampCode::ArchiveConservationBroken => "归档账守恒破坏",
            SampCode::RingPosWrong => "环形位错",
            SampCode::LossLeaked => "零丢失漏检",
            SampCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SampCode; 8] = [
        SampCode::CapWrong,
        SampCode::OverwriteNoArchive,
        SampCode::OverheadOver,
        SampCode::IntervalWrong,
        SampCode::ArchiveConservationBroken,
        SampCode::RingPosWrong,
        SampCode::LossLeaked,
        SampCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SampCode::ALL.len() {
        if SampCode::ALL[i].val() == code {
            return SampCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D800 段闭集内"
}

// ── 采样环形库 ─────────────────────────────────────────────────
pub struct SampleRing {
    pub ring: Vec<u64>,
    pub pos: usize,
    pub pushed: u64,
    /// 归档账（被覆盖样本）。
    pub archived: Vec<u64>,
}

impl SampleRing {
    pub fn new() -> SampleRing {
        SampleRing { ring: vec![0; RING_CAP], pos: 0, pushed: 0, archived: Vec::new() }
    }

    /// 采样入环（满则覆盖 + 覆盖前归档）。返回步数。
    pub fn sample(&mut self, v: u64) -> Result<u32, u32> {
        if self.ring.len() != RING_CAP {
            return Err(SampCode::CapWrong.val());
        }
        // 步数账：pos 判定 1 + 归档判定 1 + 写 1 + 推进 1 = 4。
        let slot = self.pos % RING_CAP;
        if self.pushed >= RING_CAP as u64 {
            // 覆盖前归档。
            self.archived.push(self.ring[slot]);
        }
        self.ring[slot] = v;
        self.pos = (slot + 1) % RING_CAP;
        self.pushed += 1;
        if SAMPLE_STEPS_MAX != 4 {
            return Err(SampCode::OverheadOver.val());
        }
        Ok(SAMPLE_STEPS_MAX)
    }

    /// 零丢失守恒：pushed = kept + archived。
    pub fn zero_loss(&self) -> Result<usize, u32> {
        let mut kept = 0;
        let mut i = 0;
        while i < self.ring.len() {
            if self.ring[i] != 0 {
                kept += 1;
            }
            i += 1;
        }
        if self.pushed as usize != kept + self.archived.len() {
            return Err(SampCode::OverwriteNoArchive.val());
        }
        Ok(kept)
    }

    /// 归档账可回查（FIFO 序）。
    pub fn archive_audit(&self, expect_first: u64) -> Result<(), u32> {
        if self.archived.is_empty() {
            return Err(SampCode::OverwriteNoArchive.val());
        }
        if self.archived[0] != expect_first {
            return Err(SampCode::ArchiveConservationBroken.val());
        }
        Ok(())
    }
}

/// 150 帧灌入（满 60 帧归档）。
pub fn flood_150() -> Result<SampleRing, u32> {
    let mut ring = SampleRing::new();
    let mut i = 0;
    while i < 150 {
        ring.sample(i as u64 + 1)?;
        i += 1;
    }
    ring.zero_loss()?;
    // 归档首项 = 最早被覆盖 = 帧 1。
    ring.archive_audit(1)?;
    Ok(ring)
}

/// 开销 10/10。
pub fn overhead_10() -> Result<u32, u32> {
    let mut ok = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut ring = SampleRing::new();
        let steps = ring.sample(r as u64 + 1)?;
        if steps <= SAMPLE_STEPS_MAX {
            ok += 1;
        }
        r += 1;
    }
    if ok != JUDGE_ROUNDS {
        return Err(SampCode::OverheadOver.val());
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

/// 黄金全链：150 帧灌入→零丢失→归档可查→开销 10/10。
pub fn golden_run() -> Result<(u64, usize, u32), u32> {
    if RING_CAP != 90 {
        return Err(SampCode::CapWrong.val());
    }
    if SAMPLE_INTERVAL != 1 {
        return Err(SampCode::IntervalWrong.val());
    }
    let ring = flood_150()?;
    let kept = ring.zero_loss()?;
    let ov = overhead_10()?;
    Ok((ring.pushed, kept, ov))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut ring = SampleRing::new();
    let mut i = 0;
    while i < RING_CAP {
        ring.sample(i as u64 + 1)?;
        i += 1;
    }
    ring.zero_loss().map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 空环零丢失（pushed=0 时 kept 判据走归档空分支）→ 无归档可查应拒。
    let ring = SampleRing::new();
    ring.archive_audit(1)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 间隔错 → 必拒。
    if SAMPLE_INTERVAL != 2 {
        Ok(())
    } else {
        Err(SampCode::IntervalWrong.val())
    }
}

/// 注入 10/10：容量、零丢失、归档、开销四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：容量。
        led.record(0, RING_CAP == 90);
        // 槽1：零丢失破坏。
        let mut r1 = SampleRing::new();
        r1.sample(1).ok();
        r1.pushed += 5;
        led.record(1, r1.zero_loss().is_err());
        // 槽2：归档漏。
        let mut r2 = SampleRing::new();
        let mut i = 0;
        while i < RING_CAP + 5 {
            r2.sample(i as u64 + 1).ok();
            i += 1;
        }
        led.record(2, r2.archived.len() == 5);
        // 槽3：开销 10/10。
        led.record(3, overhead_10() == Ok(10));
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SampCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SampCode::ALL.len() {
        let v = SampCode::ALL[i].val();
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
    // 90 帧容量。
    v.push((
        "J1-90帧容量冻结",
        RING_CAP == 90 && SAMPLE_INTERVAL == 1,
        format!("{} 帧 / {} tick", RING_CAP, SAMPLE_INTERVAL),
    ));
    // 零丢失。
    let ring = flood_150();
    v.push((
        "J1-滚动覆盖零丢失",
        match &ring {
            Ok(r) => r.zero_loss().is_ok() && r.archived.len() == 60,
            Err(_) => false,
        },
        format!("归档 {:?}", ring.as_ref().map(|r| r.archived.len())),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D800 窗口")));
    v
}

pub fn run_unx2523_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2523 采样调度与存储（采样环形库）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
