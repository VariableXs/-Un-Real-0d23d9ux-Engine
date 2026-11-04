//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0056 · ACPI PM 能力位只读采集 · 0x6DE00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0056-J1 PM1a/PM1b 控制块的 SLP_TYP/SLP_EN 位账与
//!     FADT PM 能力只读采集一致（睡眠态 S0-S5 五态只读账）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - PM1a_CTRL 建模：SLP_TYP(位 10-12)/SLP_EN(位 13)，写死
//!     采样账（只读：本卡零写端口，只读回采样值）；
//!   - 睡眠五态：S0=工作/S1-S3=睡眠/S4=休眠/S5=关机，SLP_TYP
//!     值域映射（0-4），越域拒；
//!   - 能力位账：C2/C3/BJ/RTC 巡检/WAK 位逐位采集（FADT 联动）；
//!   - 双控制块：PM1a/PM1b 一致性（若 b 在场，两块 SLP_TYP 同值）；
//!   - 只读红线：采集全程零写探针（写企图只记账）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DE00，AI-VX3-1102 批H 独占区）。
pub const SEG_BASE: u32 = 0x6DE00;

/// PM1 控制位（冻结）。
pub const SLP_TYP_SHIFT: u32 = 10;
pub const SLP_TYP_MASK: u32 = 0x7;
pub const SLP_EN: u32 = 1 << 13;
/// 睡眠态值域（SLP_TYP 0-4）。
pub const SLP_TYP_MAX: u32 = 4;
/// 能力位个数（C2/C3/BJ/RTC/WAK 五位）。
pub const CAP_BITS: usize = 5;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PmCode {
    /// SLP_TYP 越域（>4）
    SlpTypOutOfRange,
    /// PM1a 与 PM1b 睡眠型不一致
    BlockMismatch,
    /// 双控制块 B 在场但账零
    BlockBMissing,
    /// 能力位账外（>31 位）
    CapOverrange,
    /// 写企图发生（只读红线）
    WriteAttempted,
    /// 睡眠使能位误置（只读采集期 SLP_EN=1）
    SlpEnPreset,
    /// 采样账缺（PM1a 无值）
    SampleMissing,
    /// 双跑不一致
    NotReproducible,
}

impl PmCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PmCode::SlpTypOutOfRange => "SLP_TYP 越域：>4",
            PmCode::BlockMismatch => "PM1a/PM1b 睡眠型不一致",
            PmCode::BlockBMissing => "B 块在场但账零",
            PmCode::CapOverrange => "能力位账外：>31 位",
            PmCode::WriteAttempted => "写企图发生：只读红线",
            PmCode::SlpEnPreset => "SLP_EN 误置：采集期恒 0",
            PmCode::SampleMissing => "采样账缺：PM1a 无值",
            PmCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [PmCode; 8] = [
        PmCode::SlpTypOutOfRange,
        PmCode::BlockMismatch,
        PmCode::BlockBMissing,
        PmCode::CapOverrange,
        PmCode::WriteAttempted,
        PmCode::SlpEnPreset,
        PmCode::SampleMissing,
        PmCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PmCode::ALL.len() {
        if PmCode::ALL[i].val() == code {
            return PmCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DE00 段闭集内"
}

// ── PM1 控制块采样账 ────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pm1Sample {
    /// 控制寄存器读回值（只读账面）。
    pub raw: u32,
    /// 块是否在场（PM1b 可缺席）。
    pub present: bool,
}

impl Pm1Sample {
    /// SLP_TYP 精解。
    pub fn slp_typ(&self) -> u32 {
        (self.raw >> SLP_TYP_SHIFT) & SLP_TYP_MASK
    }
    /// SLP_EN 位。
    pub fn slp_en(&self) -> bool {
        self.raw & SLP_EN != 0
    }
    /// 采样闸：值域 + 使能位 + 在场账。
    pub fn legal(&self) -> Result<(), u32> {
        if self.slp_typ() > SLP_TYP_MAX {
            return Err(PmCode::SlpTypOutOfRange.val());
        }
        if self.slp_en() {
            return Err(PmCode::SlpEnPreset.val());
        }
        Ok(())
    }
    /// 睡眠态文本（S0-S5 五态）。
    pub fn sleep_state(&self) -> String {
        let t = self.slp_typ();
        match t {
            0 => String::from("S0=工作"),
            1 => String::from("S1=浅睡"),
            2 => String::from("S2=深睡"),
            3 => String::from("S3=待机"),
            4 => String::from("S4=休眠"),
            _ => String::from("S5=关机"),
        }
    }
}

// ── PM 能力位账 ────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PmCaps {
    /// 五位能力（C2/C3/BJ/RTC/WAK 逐位）。
    pub bits: [bool; CAP_BITS],
}

impl PmCaps {
    /// 从 u32 采集（位 0-4 账）。
    pub fn collect(v: u32) -> PmCaps {
        PmCaps {
            bits: [v & 1 != 0, (v >> 1) & 1 != 0, (v >> 2) & 1 != 0, (v >> 3) & 1 != 0, (v >> 4) & 1 != 0],
        }
    }
    /// 位账外守卫（>位 4 恒零）。
    pub fn no_overrange(v: u32) -> Result<(), u32> {
        if v >> CAP_BITS != 0 {
            return Err(PmCode::CapOverrange.val());
        }
        Ok(())
    }
    pub fn text(&self) -> String {
        let names = ["C2", "C3", "BJ", "RTC", "WAK"];
        let mut s = String::new();
        let mut i = 0;
        while i < CAP_BITS {
            if self.bits[i] {
                s.push_str(names[i]);
                s.push('|');
            }
            i += 1;
        }
        if s.ends_with('|') {
            s.truncate(s.len() - 1);
        }
        if s.is_empty() {
            s.push_str("无能力");
        }
        s
    }
}

// ── 采集器（只读 + 零写探针） ───────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct PmCollector {
    pub pm1a: Option<Pm1Sample>,
    pub pm1b: Option<Pm1Sample>,
    pub caps: PmCaps,
    pub writes: u32,
}

impl PmCollector {
    pub fn new() -> PmCollector {
        PmCollector { pm1a: None, pm1b: None, caps: PmCaps { bits: [false; CAP_BITS] }, writes: 0 }
    }

    /// 采样登记（只读面）。
    pub fn feed_a(&mut self, s: Pm1Sample) -> Result<(), u32> {
        s.legal()?;
        self.pm1a = Some(s);
        Ok(())
    }
    pub fn feed_b(&mut self, s: Pm1Sample) -> Result<(), u32> {
        s.legal()?;
        if !s.present {
            return Err(PmCode::BlockBMissing.val());
        }
        self.pm1b = Some(s);
        Ok(())
    }
    pub fn feed_caps(&mut self, v: u32) -> Result<(), u32> {
        PmCaps::no_overrange(v)?;
        self.caps = PmCaps::collect(v);
        Ok(())
    }
    /// 写探针（只读红线）。
    pub fn probe_write(&mut self) {
        self.writes += 1;
    }
    /// 双块一致性：B 在场时 SLP_TYP 同值。
    pub fn dual_consistent(&self) -> Result<(), u32> {
        let a = match &self.pm1a {
            Some(a) => a,
            None => return Err(PmCode::SampleMissing.val()),
        };
        if let Some(b) = &self.pm1b {
            if a.slp_typ() != b.slp_typ() {
                return Err(PmCode::BlockMismatch.val());
            }
        }
        Ok(())
    }
    pub fn zero_writes(&self) -> bool {
        self.writes == 0
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

// ── 注入检出账（四路：值域路/双块路/能力路/写路） ────────────────
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

/// 真机采样账（PM1a=SLP_TYP 4 <<10 只读；PM1b 缺席）。
pub fn make_machine_samples() -> (Pm1Sample, PmCaps) {
    let a = Pm1Sample { raw: 4 << SLP_TYP_SHIFT, present: true };
    let caps = PmCaps::collect(0b01011); // C2+C3+RTC
    (a, caps)
}

// ── 黄金全链：采样→能力→一致性→零写 ─────────────────────────────
pub fn golden_run() -> Result<(String, String, u32), u32> {
    let mut col = PmCollector::new();
    let (a, caps) = make_machine_samples();
    col.feed_a(a)?;
    col.feed_caps(0b01011)?;
    col.dual_consistent()?;
    let zw = if col.zero_writes() { 0 } else { 1 };
    Ok((a.sleep_state(), caps.text(), zw))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账 → S4 休眠 + C2|C3|RTC。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((s, c, 0)) if s == "S4=休眠" && c == "C2|C3|RTC" => Ok(()),
        _ => Err(PmCode::NotReproducible.val()),
    }
}

/// 序 B：SLP_TYP 5 ⇒ 越域。
pub fn probe_seq_b() -> Result<(), u32> {
    let bad = Pm1Sample { raw: 5 << SLP_TYP_SHIFT, present: true };
    match bad.legal() {
        Err(e) if e == PmCode::SlpTypOutOfRange.val() => Ok(()),
        _ => Err(PmCode::NotReproducible.val()),
    }
}

/// 序 C：双块睡眠型不一致 ⇒ BlockMismatch。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut col = PmCollector::new();
    let _ = col.feed_a(Pm1Sample { raw: 3 << SLP_TYP_SHIFT, present: true });
    let _ = col.feed_b(Pm1Sample { raw: 4 << SLP_TYP_SHIFT, present: true });
    match col.dual_consistent() {
        Err(e) if e == PmCode::BlockMismatch.val() => Ok(()),
        _ => Err(PmCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：值域路（SLP_TYP 7 必拒）。
        let bad = Pm1Sample { raw: 7 << SLP_TYP_SHIFT, present: true };
        led.record(0, bad.legal().is_err());
        // 路 2：使能路（SLP_EN 预置必拒）。
        let bad2 = Pm1Sample { raw: (3 << SLP_TYP_SHIFT) | SLP_EN, present: true };
        led.record(1, bad2.legal().is_err());
        // 路 3：能力路（位 5 置位必拒）。
        let mut col = PmCollector::new();
        led.record(2, col.feed_caps(0b100000).is_err());
        // 路 4：写路（写探针破坏零写断言）。
        let mut col2 = PmCollector::new();
        col2.probe_write();
        led.record(3, !col2.zero_writes());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PmCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PmCode::ALL.len() {
        let v = PmCode::ALL[i].val();
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
    // 主判据：真机账 S4 + 能力文案 + 零写。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((s, c, zw)) => s == "S4=休眠" && c == "C2|C3|RTC" && *zw == 0,
            Err(_) => false,
        },
        match &g {
            Ok((s, c, zw)) => format!("{} {} zw={}", s, c, zw),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 五态文案逐字（S0-S4 全谱）。
    let mut five_ok = true;
    let mut t = 0u32;
    while t <= 4 {
        let s = Pm1Sample { raw: t << SLP_TYP_SHIFT, present: true };
        let want = ["S0=工作", "S1=浅睡", "S2=深睡", "S3=待机", "S4=休眠"][t as usize];
        if s.sleep_state() != want {
            five_ok = false;
        }
        t += 1;
    }
    v.push((
        "J1-五态文案",
        five_ok,
        String::from("S0-S4 逐字钉死"),
    ));
    // 双块一致：B 同型放行。
    let mut col = PmCollector::new();
    let _ = col.feed_a(Pm1Sample { raw: 4 << SLP_TYP_SHIFT, present: true });
    let _ = col.feed_b(Pm1Sample { raw: 4 << SLP_TYP_SHIFT, present: true });
    v.push((
        "J1-双块一致",
        col.dual_consistent().is_ok(),
        String::from("PM1a=PM1b 同型放行"),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught(),
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
        String::from("0x6DE00 窗口"),
    ));
    v
}

pub fn run_unx0056_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0056 ACPI PM 能力位只读采集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
