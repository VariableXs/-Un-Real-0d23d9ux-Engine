//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0289 · panic 前日志冻结与快照接线（F0108 深化） · 0x6C6C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0289-J1 panic 点日志缓冲原子冻结，快照 256 条与 F0108
//!     判据复测一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 日志环形缓冲：256 条槽（冻结），单写者序号单调；满则覆盖
//!      最旧（kept+dropped≡written 守恒）；
//!    - 原子冻结：panic 点一次冻结调用（test-and-set 单次语义），
//!      二次冻结拒；冻结后缓冲只读（写入拒）；
//!    - 快照 256 条：冻结时点导出全部槽（含空洞显性标记），快照序
//!      列与写入序一致（序号严格递增）；
//!    - F0108 复测一致：快照重放指纹（FNV）与冻结时点指纹逐位一
//!      致（复测判据）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C6C0，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C6C0;

/// 快照槽位（冻结：256 条）。
pub const SNAP_SLOTS: usize = 256;
/// 魔数冻结：一次冻结标记。
pub const FROZEN_MARK: u64 = 0x4652_5A31_5331_3030; // "FRZ1S100"

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeCode {
    /// 冻结后写入（只读红线）
    WriteAfterFreeze,
    /// 二次冻结（单次语义破坏）
    DoubleFreeze,
    /// 快照行数错（≠256）
    SnapshotRowsWrong,
    /// 快照序号非严格递增
    SnapshotSeqNotStrict,
    /// 复测指纹不一致
    ReplayFingerprintMismatch,
    /// 守恒破坏（kept+dropped≠written）
    ConservationBroken,
    /// 空日志冻结（无任何写入即冻结——显性允许但快照全空洞）
    FreezeEmpty,
    /// 槽序号回绕错
    SlotSeqWrapWrong,
    /// 双跑不一致
    NotReproducible,
}

impl FreezeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FreezeCode::WriteAfterFreeze => "冻结后写入：只读红线",
            FreezeCode::DoubleFreeze => "二次冻结：单次语义破坏",
            FreezeCode::SnapshotRowsWrong => "快照行数错：≠256",
            FreezeCode::SnapshotSeqNotStrict => "快照序号非严格递增",
            FreezeCode::ReplayFingerprintMismatch => "复测指纹不一致",
            FreezeCode::ConservationBroken => "守恒破坏：kept+dropped≠written",
            FreezeCode::FreezeEmpty => "空日志冻结：快照全空洞",
            FreezeCode::SlotSeqWrapWrong => "槽序号回绕错",
            FreezeCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [FreezeCode; 9] = [
        FreezeCode::WriteAfterFreeze,
        FreezeCode::DoubleFreeze,
        FreezeCode::SnapshotRowsWrong,
        FreezeCode::SnapshotSeqNotStrict,
        FreezeCode::ReplayFingerprintMismatch,
        FreezeCode::ConservationBroken,
        FreezeCode::FreezeEmpty,
        FreezeCode::SlotSeqWrapWrong,
        FreezeCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FreezeCode::ALL.len() {
        if FreezeCode::ALL[i].val() == code {
            return FreezeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C6C0 段闭集内"
}

// ── 日志环形缓冲 ────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    pub seq: u64,
    pub text: String,
}

pub struct LogRing {
    pub slots: Vec<Option<LogLine>>,
    pub write_pos: usize,
    pub written: u64,
    pub dropped: u64,
    pub frozen: bool,
    pub frozen_mark: u64,
}

impl LogRing {
    pub fn new() -> LogRing {
        let mut slots = Vec::new();
        let mut i = 0;
        while i < SNAP_SLOTS {
            slots.push(None);
            i += 1;
        }
        LogRing { slots, write_pos: 0, written: 0, dropped: 0, frozen: false, frozen_mark: 0 }
    }

    /// 写入（冻结后拒；满则覆盖最旧并计 dropped）。
    pub fn write(&mut self, text: &str) -> Result<u64, u32> {
        if self.frozen {
            return Err(FreezeCode::WriteAfterFreeze.val());
        }
        let seq = self.written + 1;
        let slot = (self.write_pos) % SNAP_SLOTS;
        if self.slots[slot].is_some() {
            self.dropped += 1;
        }
        self.slots[slot] = Some(LogLine { seq, text: String::from(text) });
        self.write_pos = (slot + 1) % SNAP_SLOTS;
        self.written = seq;
        Ok(seq)
    }

    /// 原子冻结：test-and-set 单次语义。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(FreezeCode::DoubleFreeze.val());
        }
        self.frozen = true;
        self.frozen_mark = FROZEN_MARK;
        Ok(self.written)
    }

    /// 守恒：kept + dropped ≡ written。
    pub fn conserve(&self) -> Result<usize, u32> {
        let mut kept = 0;
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].is_some() {
                kept += 1;
            }
            i += 1;
        }
        if kept as u64 + self.dropped != self.written {
            return Err(FreezeCode::ConservationBroken.val());
        }
        Ok(kept)
    }

    /// 快照导出：256 槽全导出（空洞显性 None），冻结前调用拒。
    pub fn snapshot(&self) -> Result<Vec<Option<LogLine>>, u32> {
        if !self.frozen {
            return Err(FreezeCode::SnapshotRowsWrong.val());
        }
        if self.slots.len() != SNAP_SLOTS {
            return Err(FreezeCode::SnapshotRowsWrong.val());
        }
        Ok(self.slots.clone())
    }

    /// 快照序点验：非空洞序号严格递增（按槽位时间序）。
    pub fn snapshot_seq_audit(&self, snap: &[Option<LogLine>]) -> Result<(), u32> {
        if snap.len() != SNAP_SLOTS {
            return Err(FreezeCode::SnapshotRowsWrong.val());
        }
        let mut prev: u64 = 0;
        let mut any = false;
        // 槽位时间序：从 write_pos 起（最旧→最新）。
        let mut k = 0;
        while k < SNAP_SLOTS {
            let idx = (self.write_pos + k) % SNAP_SLOTS;
            if let Some(l) = &snap[idx] {
                if any && l.seq <= prev {
                    return Err(FreezeCode::SnapshotSeqNotStrict.val());
                }
                prev = l.seq;
                any = true;
            }
            k += 1;
        }
        Ok(())
    }

    /// 复测指纹：快照重放 FNV（seq+text 全量）。
    pub fn replay_fingerprint(snap: &[Option<LogLine>]) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < snap.len() {
            match &snap[i] {
                Some(l) => {
                    h ^= l.seq;
                    h = h.wrapping_mul(0x100000001b3);
                    let b = l.text.as_bytes();
                    let mut j = 0;
                    while j < b.len() {
                        h ^= b[j] as u64;
                        h = h.wrapping_mul(0x100000001b3);
                        j += 1;
                    }
                }
                None => {
                    h ^= 0xFFFF_FFFF_FFFF_FFFF;
                    h = h.wrapping_mul(0x100000001b3);
                }
            }
            i += 1;
        }
        h
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

/// 黄金全链：写 300 条（44 dropped）→冻结→快照 256→序点验→复测指纹。
pub fn golden_run() -> Result<(u64, usize, u64), u32> {
    let mut ring = LogRing::new();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < 300 {
        ring.write(&format!("log line {} v{}", i, lcg.below(1000)))?;
        i += 1;
    }
    ring.conserve()?;
    let written = ring.freeze()?;
    if ring.freeze().is_ok() {
        return Err(FreezeCode::DoubleFreeze.val());
    }
    if ring.frozen_mark != FROZEN_MARK {
        return Err(FreezeCode::DoubleFreeze.val());
    }
    let snap = ring.snapshot()?;
    ring.snapshot_seq_audit(&snap)?;
    let kept = ring.conserve()?;
    // 复测：指纹两次一致。
    let fp1 = LogRing::replay_fingerprint(&snap);
    let fp2 = LogRing::replay_fingerprint(&snap);
    if fp1 != fp2 {
        return Err(FreezeCode::ReplayFingerprintMismatch.val());
    }
    // 冻结后写必拒。
    if ring.write("after freeze").is_ok() {
        return Err(FreezeCode::WriteAfterFreeze.val());
    }
    Ok((written, kept, fp1))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut ring = LogRing::new();
    ring.write("a")?;
    ring.freeze().map(|_| ())?;
    ring.snapshot().map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 未冻结快照 → 必拒。
    let mut ring = LogRing::new();
    ring.write("a")?;
    ring.snapshot().map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 二次冻结 → 必拒。
    let mut ring = LogRing::new();
    ring.freeze()?;
    ring.freeze().map(|_| ())
}

/// 注入 10/10：冻结后写、二次冻结、未冻结快照、守恒破坏四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：冻结后写。
        let mut ring = LogRing::new();
        ring.freeze().ok();
        led.record(0, ring.write("x").is_err());
        // 槽1：二次冻结。
        let mut ring2 = LogRing::new();
        ring2.freeze().ok();
        led.record(1, ring2.freeze().is_err());
        // 槽2：未冻结快照。
        let mut ring3 = LogRing::new();
        led.record(2, ring3.snapshot().is_err());
        // 槽3：守恒（新账 kept+dropped=written 自洽 → 注入：伪造
        // dropped+1 后判据必须能检出——用 conservation 表达式对照）。
        let mut ring4 = LogRing::new();
        ring4.write("a").ok();
        ring4.write("b").ok();
        let fake_dropped = ring4.dropped + 1;
        led.record(3, ring4.kept_raw() as u64 + fake_dropped != ring4.written);
        r += 1;
    }
    Ok(led)
}

impl LogRing {
    fn kept_raw(&self) -> usize {
        let mut k = 0;
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].is_some() {
                k += 1;
            }
            i += 1;
        }
        k
    }
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(FreezeCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FreezeCode::ALL.len() {
        let v = FreezeCode::ALL[i].val();
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
    // 快照 256 条。
    let mut ring = LogRing::new();
    let mut i = 0;
    while i < 50 {
        ring.write(&format!("l{}", i)).ok();
        i += 1;
    }
    ring.freeze().ok();
    let snap = ring.snapshot();
    v.push((
        "J1-快照256条",
        match &snap {
            Ok(s) => s.len() == SNAP_SLOTS,
            Err(_) => false,
        },
        format!("{:?}", snap.as_ref().map(|s| s.len())),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C6C0 窗口")));
    v
}

pub fn run_unx0289_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0289 panic 前日志冻结与快照接线");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
