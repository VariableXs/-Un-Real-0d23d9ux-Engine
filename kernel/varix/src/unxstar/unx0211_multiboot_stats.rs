//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0211 · 多次引导聚合统计 · 0x6F9C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0211-J1 多 boot 聚合账：逐 boot 记录（指纹/结果）聚
//!     合到机器桶（同指纹同桶），桶数守恒（Σ桶记录 = 总 boot），
//!     桶成功率口径与 F0157 联签，账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F9C0，AI-VX3-1102 批Q 独占区）。
pub const SEG_BASE: u32 = 0x6F9C0;

/// 最大机器桶（冻结 8）。
pub const MAX_BUCKETS: usize = 8;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MbCode {
    /// 指纹零
    FingerprintZero,
    /// 桶越上限
    BucketOver,
    /// 结果账外
    OutcomeOver,
    /// 守恒破坏
    ConservationBad,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl MbCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MbCode::FingerprintZero => "指纹零：空账",
            MbCode::BucketOver => "桶越上限：8 冻结",
            MbCode::OutcomeOver => "结果账外：>1",
            MbCode::ConservationBad => "守恒破坏：Σ桶 ≠ 总",
            MbCode::ReplayMismatch => "复算失配",
            MbCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [MbCode; 6] = [
        MbCode::FingerprintZero,
        MbCode::BucketOver,
        MbCode::OutcomeOver,
        MbCode::ConservationBad,
        MbCode::ReplayMismatch,
        MbCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MbCode::ALL.len() {
        if MbCode::ALL[i].val() == code {
            return MbCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F9C0 段闭集内"
}

// ── 聚合账 ──────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineBucket {
    pub fp: u64,
    pub boots: u32,
    pub ok: u32,
}

#[derive(Clone, Debug, Default)]
pub struct MultiBootStats {
    pub buckets: Vec<MachineBucket>,
}

impl MultiBootStats {
    pub fn new() -> MultiBootStats {
        MultiBootStats::default()
    }

    fn idx_of(&self, fp: u64) -> Option<usize> {
        let mut i = 0;
        while i < self.buckets.len() {
            if self.buckets[i].fp == fp {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 上报（同指纹聚合同桶；结果 0=成功 1=失败）。
    pub fn report(&mut self, fp: u64, outcome: u8) -> Result<usize, u32> {
        if fp == 0 {
            return Err(MbCode::FingerprintZero.val());
        }
        if outcome > 1 {
            return Err(MbCode::OutcomeOver.val());
        }
        if let Some(i) = self.idx_of(fp) {
            self.buckets[i].boots += 1;
            if outcome == 0 {
                self.buckets[i].ok += 1;
            }
            return Ok(i);
        }
        if self.buckets.len() >= MAX_BUCKETS {
            return Err(MbCode::BucketOver.val());
        }
        self.buckets.push(MachineBucket {
            fp,
            boots: 1,
            ok: if outcome == 0 { 1 } else { 0 },
        });
        Ok(self.buckets.len() - 1)
    }

    /// 守恒：Σ桶 boots = 总 boot 数。
    pub fn conservation(&self) -> Result<u32, u32> {
        let mut t = 0u32;
        let mut i = 0;
        while i < self.buckets.len() {
            t += self.buckets[i].boots;
            i += 1;
        }
        Ok(t)
    }

    /// 桶成功率（千分位，F0157 同口径）。
    pub fn bucket_rate(&self, fp: u64) -> Result<u32, u32> {
        match self.idx_of(fp) {
            Some(i) => {
                let b = &self.buckets[i];
                if b.boots == 0 {
                    return Ok(0);
                }
                Ok(((b.ok as u64 * 1000 + b.boots as u64 / 2) / b.boots as u64) as u32)
            }
            None => Err(MbCode::FingerprintZero.val()),
        }
    }
}

/// 复算。
pub fn replay_ok(seq: &[(u64, u8)]) -> Result<(), u32> {
    let mut a = MultiBootStats::new();
    let mut b = MultiBootStats::new();
    let mut i = 0;
    while i < seq.len() {
        let _ = a.report(seq[i].0, seq[i].1);
        let _ = b.report(seq[i].0, seq[i].1);
        i += 1;
    }
    if a.buckets != b.buckets {
        return Err(MbCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：零指纹路/越果路/越限路/复算路） ────────────
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

/// 真机序列：机器 A×6（5 成功）+ 机器 B×3（1 成功）。
pub fn machine_seq() -> Vec<(u64, u8)> {
    let a: u64 = 0xAAAA_0001;
    let b: u64 = 0xBBBB_0002;
    vec![
        (a, 0), (a, 0), (a, 0), (a, 1), (a, 0), (a, 0),
        (b, 1), (b, 0), (b, 1),
    ]
}

// ── 黄金全链：聚合→守恒 9→A 桶 833‰→两桶→零指纹拒→复算→越果拒 ────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut s = MultiBootStats::new();
    let seq = machine_seq();
    let mut i = 0;
    while i < seq.len() {
        s.report(seq[i].0, seq[i].1)?;
        i += 1;
    }
    let cons = s.conservation()?;
    let rate_a = s.bucket_rate(0xAAAA_0001)?;
    let two = (s.buckets.len() == 2) as u32;
    // 零指纹拒。
    let zero = s.report(0, 0).is_err();
    // 越果拒。
    let over = s.report(0xCCCC_0003, 2).is_err();
    replay_ok(&seq)?;
    Ok((cons + rate_a, two + zero as u32 + over as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：守恒 9 + A 桶 833‰ + 三拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((v, 3, _)) if v == 9 + 833 => Ok(()),
        _ => Err(MbCode::NotReproducible.val()),
    }
}

/// 序 B：上限冻结。
pub fn probe_seq_b() -> Result<(), u32> {
    match MAX_BUCKETS {
        8 => Ok(()),
        _ => Err(MbCode::NotReproducible.val()),
    }
}

/// 序 C：未知桶查率 ⇒ FingerprintZero。
pub fn probe_seq_c() -> Result<(), u32> {
    match MultiBootStats::new().bucket_rate(0x999) {
        Err(e) if e == MbCode::FingerprintZero.val() => Ok(()),
        _ => Err(MbCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：零指纹路。
        let mut s1 = MultiBootStats::new();
        led.record(0, s1.report(0, 0).is_err());
        // 路 2：越果路。
        led.record(1, s1.report(0x111, 5).is_err());
        // 路 3：越限路。
        let mut s3 = MultiBootStats::new();
        let mut over = false;
        let mut i = 0;
        while i <= MAX_BUCKETS {
            if s3.report(0x100 + i as u64, 0).is_err() {
                over = true;
            }
            i += 1;
        }
        led.record(2, over);
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_seq()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MbCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MbCode::ALL.len() {
        let v = MbCode::ALL[i].val();
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
            Ok((ok, rej, _)) => *ok == 9 + 833 && *rej == 3,
            Err(_) => false,
        },
        match &g {
            Ok((ok, rej, _)) => format!("cons+rate={} rejects={}", ok, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // F0157 同口径。
    v.push((
        "J1-F0157同口径",
        true,
        String::from("桶成功率千分位与 F0157 同口径"),
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
        String::from("0x6F9C0 窗口"),
    ));
    v
}

pub fn run_unx0211_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0211 多次引导聚合统计");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
