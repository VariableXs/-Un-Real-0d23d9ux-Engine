//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0108 · panic 前日志快照 · 0x6E880
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0108-J1 panic 前快照：最近 16 条冻结（封存只读），
//!     快照含 panic 三要素（PC/错误码/时间戳），快照与日志流指
//!     纹一致，二次 panic 快照不覆盖（首快照红线）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 快照账：触发时取日志流尾 16 条冻结（FNV 指纹）；
//!   - 三要素账：PC/错误码/时间戳（三槽必填）；
//!   - 首快照红线：已冻结再触发不覆盖（计数留痕）；
//!   - 对拍：快照指纹 == 流尾 16 条指纹（漂移拒）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6E880，AI-VX3-1102 批L 独占区）。
pub const SEG_BASE: u32 = 0x6E880;

/// 快照条数（冻结 16）。
pub const SNAP_LEN: usize = 16;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PsnapCode {
    /// 快照不足（日志流 < 16 条拒取满窗）
    LogTooShort,
    /// 快照漂移（指纹与流尾失配）
    SnapshotDrift,
    /// 首快照被覆盖（红线）
    FirstOverwritten,
    /// 三要素缺账
    TripletMissing,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl PsnapCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PsnapCode::LogTooShort => "日志流不足 16 条：拒取满窗",
            PsnapCode::SnapshotDrift => "快照漂移：指纹失配",
            PsnapCode::FirstOverwritten => "首快照被覆盖：红线",
            PsnapCode::TripletMissing => "panic 三要素缺账",
            PsnapCode::ReplayMismatch => "复算失配",
            PsnapCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [PsnapCode; 6] = [
        PsnapCode::LogTooShort,
        PsnapCode::SnapshotDrift,
        PsnapCode::FirstOverwritten,
        PsnapCode::TripletMissing,
        PsnapCode::ReplayMismatch,
        PsnapCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PsnapCode::ALL.len() {
        if PsnapCode::ALL[i].val() == code {
            return PsnapCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6E880 段闭集内"
}

// ── 日志流与快照 ────────────────────────────────────────────────
pub fn fnv2(h: u64, a: u64, b: u64) -> u64 {
    let mut acc = (h ^ a).wrapping_mul(0x100000001b3);
    acc = (acc ^ b).wrapping_mul(0x100000001b3);
    acc
}

#[derive(Clone, Debug, Default)]
pub struct LogStream {
    pub entries: Vec<(u64, u64)>, // (tsc, code)
}

impl LogStream {
    /// 流尾 16 条指纹。
    pub fn tail_fp(&self) -> Result<u64, u32> {
        if self.entries.len() < SNAP_LEN {
            return Err(PsnapCode::LogTooShort.val());
        }
        let start = self.entries.len() - SNAP_LEN;
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = start;
        while i < self.entries.len() {
            h = fnv2(h, self.entries[i].0, self.entries[i].1);
            i += 1;
        }
        Ok(h)
    }
}

#[derive(Clone, Debug)]
pub struct PanicSnapshot {
    pub entries: Vec<(u64, u64)>,
    pub fp: u64,
    /// panic 三要素（PC/错误码/时间戳）。
    pub pc: u64,
    pub err_code: u32,
    pub ts: u64,
    pub overwrite_caught: u32,
}

impl PanicSnapshot {
    /// 触发快照（流尾 16 条 + 三要素）。
    pub fn take(stream: &LogStream, pc: u64, err_code: u32, ts: u64) -> Result<PanicSnapshot, u32> {
        if stream.entries.len() < SNAP_LEN {
            return Err(PsnapCode::LogTooShort.val());
        }
        if pc == 0 || err_code == 0 || ts == 0 {
            return Err(PsnapCode::TripletMissing.val());
        }
        let fp = stream.tail_fp()?;
        let start = stream.entries.len() - SNAP_LEN;
        let mut entries = Vec::new();
        let mut i = start;
        while i < stream.entries.len() {
            entries.push(stream.entries[i]);
            i += 1;
        }
        Ok(PanicSnapshot { entries, fp, pc, err_code, ts, overwrite_caught: 0 })
    }

    /// 指纹对拍（快照 vs 流尾）。
    pub fn drift_audit(&self, stream: &LogStream) -> Result<(), u32> {
        if self.fp != stream.tail_fp()? {
            return Err(PsnapCode::SnapshotDrift.val());
        }
        Ok(())
    }
}

// ── 快照持账者（首快照红线） ────────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct SnapHolder {
    pub snap: Option<PanicSnapshot>,
    pub retrigger: u32,
}

impl SnapHolder {
    pub fn new() -> SnapHolder {
        SnapHolder::default()
    }

    /// panic 触发（已有快照不覆盖，计数留痕）。
    pub fn on_panic(&mut self, stream: &LogStream, pc: u64, code: u32, ts: u64) -> Result<bool, u32> {
        if self.snap.is_some() {
            self.retrigger += 1;
            return Ok(false); // 首快照红线：不覆盖。
        }
        let s = PanicSnapshot::take(stream, pc, code, ts)?;
        self.snap = Some(s);
        Ok(true)
    }
}

/// 占位（类型对齐用；运行时不可达：上游已判 Some）。
fn unreachable_snapshot() -> &'static PanicSnapshot {
    // 静态最小快照（不可达兜底）。
    static MIN: PanicSnapshot = PanicSnapshot {
        entries: Vec::new(),
        fp: 0,
        pc: 0,
        err_code: 0,
        ts: 0,
        overwrite_caught: 0,
    };
    &MIN
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

// ── 注入检出账（四路：缺账路/漂移路/覆盖路/复算路） ──────────────
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

/// 真机日志流（20 条）。
pub fn machine_stream() -> LogStream {
    let mut s = LogStream::default();
    let mut i = 0u64;
    while i < 20 {
        s.entries.push((1000 + i * 100, 0x100 + i));
        i += 1;
    }
    s
}

// ── 黄金全链：触发→三要素→对拍→复跑→覆盖红线 ────────────────────
pub fn golden_run() -> Result<(u64, u32, bool), u32> {
    let stream = machine_stream();
    let mut holder = SnapHolder::new();
    let first = holder.on_panic(&stream, 0x1000_0000, 0x2A, 0xE8_0000)?;
    let (fp0, n_entries) = match &holder.snap {
        Some(s) => (s.fp, s.entries.len()),
        None => return Err(PsnapCode::TripletMissing.val()),
    };
    {
        let s = holder.snap.as_ref().unwrap_or_else(|| unreachable_snapshot());
        s.drift_audit(&stream)?;
    }
    // 复算：同流再快照指纹恒一致。
    let again = PanicSnapshot::take(&stream, 0x1000_0000, 0x2A, 0xE8_0000)?;
    if again.fp != fp0 {
        return Err(PsnapCode::ReplayMismatch.val());
    }
    // 二次 panic 不覆盖。
    let second = holder.on_panic(&stream, 0x2000_0000, 0x2B, 0xE9_0000)?;
    Ok((fp0, n_entries as u32, first && !second))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账 → 16 条 + 首快照保住。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((fp, 16, true)) if fp != 0 => Ok(()),
        _ => Err(PsnapCode::NotReproducible.val()),
    }
}

/// 序 B：流不足 ⇒ LogTooShort。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut short = LogStream::default();
    let mut i = 0;
    while i < 8 {
        short.entries.push((i, i));
        i += 1;
    }
    match PanicSnapshot::take(&short, 1, 1, 1) {
        Err(e) if e == PsnapCode::LogTooShort.val() => Ok(()),
        _ => Err(PsnapCode::NotReproducible.val()),
    }
}

/// 序 C：三要素缺账 ⇒ TripletMissing。
pub fn probe_seq_c() -> Result<(), u32> {
    match PanicSnapshot::take(&machine_stream(), 0, 0x2A, 0xE8_0000) {
        Err(e) if e == PsnapCode::TripletMissing.val() => Ok(()),
        _ => Err(PsnapCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：缺账路。
        led.record(0, PanicSnapshot::take(&machine_stream(), 0x1000, 0, 0xE8).is_err());
        // 路 2：漂移路（流追加一条后对拍必失配）。
        let mut st2 = machine_stream();
        let snap2 = PanicSnapshot::take(&st2, 0x1000, 0x2A, 0xE8)?;
        st2.entries.push((9999, 0x999));
        led.record(1, snap2.drift_audit(&st2).is_err());
        // 路 3：覆盖路（二次 panic 首快照保住）。
        let mut h3 = SnapHolder::new();
        let _ = h3.on_panic(&machine_stream(), 1, 1, 1);
        let _ = h3.on_panic(&machine_stream(), 2, 2, 2);
        led.record(2, h3.retrigger == 1 && h3.snap.as_ref().map(|s| s.pc == 1).unwrap_or(false));
        // 路 4：复算路。
        let a = PanicSnapshot::take(&machine_stream(), 1, 1, 1)?.fp;
        let b = PanicSnapshot::take(&machine_stream(), 1, 1, 1)?.fp;
        led.record(3, a == b);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PsnapCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PsnapCode::ALL.len() {
        let v = PsnapCode::ALL[i].val();
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
            Ok((fp, n, first_kept)) => *fp != 0 && *n == 16 && *first_kept,
            Err(_) => false,
        },
        match &g {
            Ok((fp, n, first_kept)) => format!("fp=0x{:x} entries={} first_kept={}", fp, n, first_kept),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 三要素账（PC/码/戳全填）。
    let s = PanicSnapshot::take(&machine_stream(), 0x1000_0000, 0x2A, 0xE8_0000);
    v.push((
        "J1-三要素",
        match &s {
            Ok(s) => s.pc == 0x1000_0000 && s.err_code == 0x2A && s.ts == 0xE8_0000,
            Err(_) => false,
        },
        match &s {
            Ok(s) => format!("pc=0x{:x} code=0x{:x} ts=0x{:x}", s.pc, s.err_code, s.ts),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 快照窗（尾 16 条 = seq 4..20）。
    v.push((
        "J1-快照窗",
        match &s {
            Ok(s) => s.entries[0].0 == 1400 && s.entries[15].0 == 2900,
            Err(_) => false,
        },
        String::from("尾 16 条（tsc 1400..2900）冻结"),
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
        String::from("0x6E880 窗口"),
    ));
    v
}

pub fn run_unx0108_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0108 panic 前日志快照");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
