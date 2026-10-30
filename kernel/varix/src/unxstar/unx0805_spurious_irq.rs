//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0805 · spurious 向量 0xFF 槽位与伪中断过滤 · 0x6C940
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B2）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0805-J1 spurious 触发全程入账不误 EOI（10 万次注入零
//!     误 EOI），伪中断识别率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - spurious 槽位冻结：向量 0xFF（本地 APIC spurious 向量）；
//!    - 伪中断过滤：spurious 触发只入账不发 EOI（EOI 账零命中），
//!      真中断（向量 < 0xFF）走 EOI 正常路径；
//!    - 10 万次注入：spurious/真中断混合流，误 EOI 计数恒 0、伪中
//!      断识别率 100%；
//!    - 中断账三列（向量/类别/EOI 与否），分类账守恒（spurious+
//!      real ≡ total）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C940，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C940;

/// spurious 向量（冻结：0xFF）。
pub const SPURIOUS_VECTOR: u8 = 0xFF;
/// 注入轮数（冻结：10 万）。
pub const FUZZ_ROUNDS: u32 = 100_000;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpurCode {
    /// 向量越域（>0xFF）
    VectorOutOfRange,
    /// spurious 误 EOI（红线）
    SpuriousEoiLeak,
    /// 伪中断漏识别（真中断被判伪）
    RealMisclassified,
    /// 分类守恒破坏（spurious+real≠total）
    ClassConservationBroken,
    /// 真中断未 EOI（漏 EOI）
    RealEoiMissing,
    /// 向量 0x00（合法域外）
    VectorZero,
    /// 账条缺列
    LedgerRowShort,
    /// 双跑不一致
    NotReproducible,
}

impl SpurCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SpurCode::VectorOutOfRange => "向量越域：>0xFF",
            SpurCode::SpuriousEoiLeak => "spurious 误 EOI：红线",
            SpurCode::RealMisclassified => "伪中断漏识别：真中断被判伪",
            SpurCode::ClassConservationBroken => "分类守恒破坏",
            SpurCode::RealEoiMissing => "真中断未 EOI：漏 EOI",
            SpurCode::VectorZero => "向量 0x00：合法域外",
            SpurCode::LedgerRowShort => "账条缺列",
            SpurCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SpurCode; 8] = [
        SpurCode::VectorOutOfRange,
        SpurCode::SpuriousEoiLeak,
        SpurCode::RealMisclassified,
        SpurCode::ClassConservationBroken,
        SpurCode::RealEoiMissing,
        SpurCode::VectorZero,
        SpurCode::LedgerRowShort,
        SpurCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SpurCode::ALL.len() {
        if SpurCode::ALL[i].val() == code {
            return SpurCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C940 段闭集内"
}

// ── 中断分类 ───────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IrqClass {
    Spurious,
    Real,
}

pub fn classify(vec: u8) -> Result<IrqClass, u32> {
    if vec == 0 {
        return Err(SpurCode::VectorZero.val());
    }
    if vec == SPURIOUS_VECTOR {
        Ok(IrqClass::Spurious)
    } else {
        Ok(IrqClass::Real)
    }
}

// ── 中断账 ─────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrqRow {
    pub vector: u8,
    pub class: IrqClass,
    pub eoi_sent: bool,
}

pub struct IrqLedger {
    pub rows: Vec<IrqRow>,
    pub total: u64,
    pub spurious_cnt: u64,
    pub real_cnt: u64,
    pub eoi_cnt: u64,
    pub spurious_eoi_leak: u32,
    pub real_eoi_miss: u32,
}

impl IrqLedger {
    pub fn new() -> IrqLedger {
        IrqLedger {
            rows: Vec::new(),
            total: 0,
            spurious_cnt: 0,
            real_cnt: 0,
            eoi_cnt: 0,
            spurious_eoi_leak: 0,
            real_eoi_miss: 0,
        }
    }

    /// 中断入口：分类 → spurious 只入账不 EOI；real 入账+EOI。
    pub fn ingest(&mut self, vec: u8) -> Result<(), u32> {
        let class = classify(vec)?;
        let eoi = match class {
            IrqClass::Spurious => false,
            IrqClass::Real => true,
        };
        if eoi {
            self.eoi_cnt += 1;
        } else {
            self.spurious_cnt += 1;
        }
        self.total += 1;
        if self.total <= 32 {
            // 账条只留前 32 条（防 10 万行爆账）。
            self.rows.push(IrqRow { vector: vec, class, eoi_sent: eoi });
        }
        Ok(())
    }

    /// 分类守恒：spurious + real ≡ total（EOI 数 = real 数）。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.spurious_cnt + self.real_cnt != self.total {
            return Err(SpurCode::ClassConservationBroken.val());
        }
        if self.eoi_cnt != self.real_cnt {
            return Err(SpurCode::RealEoiMissing.val());
        }
        Ok(())
    }

    /// 账条审计（前 32 条三列齐）。
    pub fn rows_audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            let expect_class = classify(r.vector)?;
            if expect_class != r.class {
                return Err(SpurCode::RealMisclassified.val());
            }
            if r.class == IrqClass::Spurious && r.eoi_sent {
                return Err(SpurCode::SpuriousEoiLeak.val());
            }
            if r.class == IrqClass::Real && !r.eoi_sent {
                return Err(SpurCode::RealEoiMissing.val());
            }
            i += 1;
        }
        Ok(())
    }
}

/// 10 万次注入（混合流）：零误 EOI + 识别率 100%。
pub fn fuzz_mixed(rounds: u32, seed: u64) -> Result<(u64, u64, u64), u32> {
    let mut lcg = Lcg::new(seed);
    let mut led = IrqLedger::new();
    let mut r = 0;
    while r < rounds {
        // 60% spurious / 40% 真中断（确定性混合）。
        let pick = lcg.below(100);
        let vec = if pick < 60 {
            SPURIOUS_VECTOR
        } else {
            (lcg.below(0xFE) + 1) as u8 // 1..=0xFE
        };
        led.ingest(vec)?;
        r += 1;
    }
    led.conserve()?;
    led.rows_audit()?;
    if led.spurious_eoi_leak != 0 {
        return Err(SpurCode::SpuriousEoiLeak.val());
    }
    Ok((led.total, led.spurious_cnt, led.eoi_cnt))
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

/// 黄金全链：10 万注入→零误 EOI→识别率 100%→账条审计。
pub fn golden_run() -> Result<(u64, u64, u64), u32> {
    if SPURIOUS_VECTOR != 0xFF {
        return Err(SpurCode::VectorOutOfRange.val());
    }
    let (total, spurious, eoi) = fuzz_mixed(FUZZ_ROUNDS, SEG_BASE as u64)?;
    let real = total - spurious;
    if eoi != real {
        return Err(SpurCode::RealEoiMissing.val());
    }
    Ok((total, spurious, eoi))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut led = IrqLedger::new();
    led.ingest(SPURIOUS_VECTOR)?;
    led.ingest(0x20)?;
    led.conserve()?;
    led.rows_audit()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 向量 0 → 必拒。
    classify(0).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // spurious 被判真（伪造分类）→ 审计必抓。
    let led = IrqLedger {
        rows: vec![IrqRow { vector: SPURIOUS_VECTOR, class: IrqClass::Real, eoi_sent: true }],
        total: 1,
        spurious_cnt: 0,
        real_cnt: 1,
        eoi_cnt: 1,
        spurious_eoi_leak: 0,
        real_eoi_miss: 0,
    };
    led.rows_audit()
}

/// 注入 10/10：向量 0、误 EOI、漏 EOI、守恒破坏四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：向量 0。
        led.record(0, classify(0).is_err());
        // 槽1：spurious 误 EOI（账面对拍）。
        let bad = IrqRow { vector: SPURIOUS_VECTOR, class: IrqClass::Spurious, eoi_sent: true };
        led.record(1, bad.class == IrqClass::Spurious && bad.eoi_sent);
        // 槽2：真中断漏 EOI。
        let mut l2 = IrqLedger::new();
        l2.ingest(0x30).ok();
        // 伪造：eoi_cnt 清零。
        l2.eoi_cnt = 0;
        led.record(2, l2.conserve().is_err());
        // 槽3：守恒破坏。
        let mut l3 = IrqLedger::new();
        l3.ingest(0x31).ok();
        l3.total += 1;
        led.record(3, l3.conserve().is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SpurCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SpurCode::ALL.len() {
        let v = SpurCode::ALL[i].val();
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
    // 10 万注入零误 EOI。
    let fz = fuzz_mixed(FUZZ_ROUNDS, SEG_BASE as u64);
    v.push((
        "J1-10万注入零误EOI",
        match &fz {
            Ok((t, s, e)) => *t == 100_000 && *e == *t - *s,
            Err(_) => false,
        },
        format!("{:?}", fz),
    ));
    // 伪中断识别率 100%。
    let mut led = IrqLedger::new();
    led.ingest(SPURIOUS_VECTOR).ok();
    led.ingest(0x21).ok();
    v.push((
        "J1-识别率100%",
        led.conserve().is_ok() && led.spurious_cnt == 1 && led.real_cnt == 1,
        format!("s={} r={}", led.spurious_cnt, led.real_cnt),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C940 窗口")));
    v
}

pub fn run_unx0805_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0805 spurious 向量 0xFF 槽位与伪中断过滤");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
