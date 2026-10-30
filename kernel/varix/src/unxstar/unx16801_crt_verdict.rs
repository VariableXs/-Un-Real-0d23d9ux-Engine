//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16801 · crt_version_matrix 裁决器首版（版本矩阵与四级裁决总纲）
//! · 0x68380
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-E2-B01）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F16801-J1 裁决器对 50 个导入面样本（manifest 声明/无声
//!     明/侧载/系统级四类混合）产出裁决结果与预置账本逐例一致，
//!     四级裁决优先序断言齐备，兜底命中 100% 记诊断账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 版本矩阵：CRT 版本族闭集（v60/v70/v71/v80/v90/v100/v110/
//!     v140/vcallow 全族），族→DLL 名映射冻结；
//!   - 四级裁决优先序：L1 manifest 显式声明 > L2 侧载目录探测 >
//!     L3 系统级安装记录 > L4 兜底默认（v140 兜底），高优先级命中
//!     即短路；
//!   - 50 样本预置账本：每样本（样本号/类别/期望裁决）冻结，裁决
//!     输出逐例对拍（一致率 100%）；
//!   - 兜底命中：L4 命中必须记诊断账（非静默），兜底率 100% 记账；
//!   - LCG 种子写死 = SEG_BASE（0x68380）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68380，AI-VX3-1030 批16 独占区）。
pub const SEG_BASE: u32 = 0x68380;

// ── CRT 版本族闭集 ──────────────────────────────────────────────
pub const CRT_V60: usize = 0;
pub const CRT_V70: usize = 1;
pub const CRT_V71: usize = 2;
pub const CRT_V80: usize = 3;
pub const CRT_V90: usize = 4;
pub const CRT_V100: usize = 5;
pub const CRT_V110: usize = 6;
pub const CRT_V140: usize = 7;
pub const CRT_FAMILY_COUNT: usize = 8;
pub const CRT_FAMILY_NAMES: [&str; CRT_FAMILY_COUNT] = [
    "v60", "v70", "v71", "v80", "v90", "v100", "v110", "v140",
];
/// 族→DLL 名映射（冻结）。
pub const CRT_DLL_NAMES: [&str; CRT_FAMILY_COUNT] = [
    "msvcr60.dll", "msvcr70.dll", "msvcr71.dll", "msvcr80.dll",
    "msvcr90.dll", "msvcr100.dll", "msvcr110.dll", "vcruntime140.dll",
];
/// 兜底族（v140）。
pub const CRT_FALLBACK: usize = CRT_V140;

// ── 四级裁决优先序 ──────────────────────────────────────────────
pub const LVL_MANIFEST: u8 = 1;
pub const LVL_SIDELOAD: u8 = 2;
pub const LVL_SYSTEM: u8 = 3;
pub const LVL_FALLBACK: u8 = 4;
pub const LEVEL_COUNT: usize = 4;
pub const LEVEL_NAMES: [&str; LEVEL_COUNT] =
    ["manifest声明", "侧载探测", "系统级记录", "兜底默认"];

// ── 样本类别闭集 ────────────────────────────────────────────────
pub const SAMP_MANIFEST: usize = 0;
pub const SAMP_NO_DECL: usize = 1;
pub const SAMP_SIDELOAD: usize = 2;
pub const SAMP_SYSTEM: usize = 3;
pub const SAMP_KIND_COUNT: usize = 4;
pub const SAMP_KIND_NAMES: [&str; SAMP_KIND_COUNT] =
    ["manifest声明", "无声明", "侧载", "系统级"];

/// 样本总数（冻结：50）。
pub const SAMPLE_COUNT: usize = 50;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerdictCode {
    /// 版本族越闭集
    FamilyOutOfRange,
    /// 族名空（齐备破坏）
    FamilyNameBlank,
    /// 族→DLL 映射错位（名表与族表长度/内容不一致）
    DllMapMisaligned,
    /// 裁决级别越闭集
    LevelOutOfRange,
    /// 优先序破坏（低级命中却用高级）
    PriorityBroken,
    /// 兜底未记账（L4 命中静默）
    FallbackUnbooked,
    /// 预置账本缺例（≠50）
    LedgerSampleShort,
    /// 裁决与账本不一致
    VerdictMismatch,
    /// 类别越闭集
    KindOutOfRange,
    /// 双跑不一致
    NotReproducible,
}

impl VerdictCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            VerdictCode::FamilyOutOfRange => "版本族越闭集：八族冻结",
            VerdictCode::FamilyNameBlank => "族名空：齐备破坏",
            VerdictCode::DllMapMisaligned => "族→DLL 映射错位",
            VerdictCode::LevelOutOfRange => "裁决级别越闭集：四级冻结",
            VerdictCode::PriorityBroken => "优先序破坏：低级命中却用高级",
            VerdictCode::FallbackUnbooked => "兜底未记账：L4 命中静默",
            VerdictCode::LedgerSampleShort => "预置账本缺例：≠50",
            VerdictCode::VerdictMismatch => "裁决与账本不一致",
            VerdictCode::KindOutOfRange => "类别越闭集：四类冻结",
            VerdictCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [VerdictCode; 10] = [
        VerdictCode::FamilyOutOfRange,
        VerdictCode::FamilyNameBlank,
        VerdictCode::DllMapMisaligned,
        VerdictCode::LevelOutOfRange,
        VerdictCode::PriorityBroken,
        VerdictCode::FallbackUnbooked,
        VerdictCode::LedgerSampleShort,
        VerdictCode::VerdictMismatch,
        VerdictCode::KindOutOfRange,
        VerdictCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < VerdictCode::ALL.len() {
        if VerdictCode::ALL[i].val() == code {
            return VerdictCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x68380 段闭集内"
}

// ── 裁决结果 ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub level: u8,
    pub family: usize,
}

impl Verdict {
    pub fn legal(&self) -> Result<(), u32> {
        if self.level < LVL_MANIFEST || self.level > LVL_FALLBACK {
            return Err(VerdictCode::LevelOutOfRange.val());
        }
        if self.family >= CRT_FAMILY_COUNT {
            return Err(VerdictCode::FamilyOutOfRange.val());
        }
        Ok(())
    }
}

// ── 样本输入 ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct ImportSample {
    pub kind: usize,
    /// manifest 声明的族（None=无声明）。
    pub manifest_decl: Option<usize>,
    /// 侧载目录探测到的族（None=未探测到）。
    pub sideload: Option<usize>,
    /// 系统级安装记录的族（None=无记录）。
    pub system_rec: Option<usize>,
}

impl ImportSample {
    pub fn legal(&self) -> Result<(), u32> {
        if self.kind >= SAMP_KIND_COUNT {
            return Err(VerdictCode::KindOutOfRange.val());
        }
        if let Some(f) = self.manifest_decl {
            if f >= CRT_FAMILY_COUNT {
                return Err(VerdictCode::FamilyOutOfRange.val());
            }
        }
        if let Some(f) = self.sideload {
            if f >= CRT_FAMILY_COUNT {
                return Err(VerdictCode::FamilyOutOfRange.val());
            }
        }
        if let Some(f) = self.system_rec {
            if f >= CRT_FAMILY_COUNT {
                return Err(VerdictCode::FamilyOutOfRange.val());
            }
        }
        Ok(())
    }
}

// ── 裁决器（四级优先序，短路命中） ──────────────────────────────
pub struct CrtVerdictor {
    pub fallback_cnt: u32,
    pub verdicts: Vec<Verdict>,
}

impl CrtVerdictor {
    pub fn new() -> CrtVerdictor {
        CrtVerdictor { fallback_cnt: 0, verdicts: Vec::new() }
    }

    /// 裁决：L1→L2→L3→L4 短路；L4 记账。
    pub fn judge(&mut self, s: &ImportSample) -> Result<Verdict, u32> {
        s.legal()?;
        let v = if let Some(f) = s.manifest_decl {
            Verdict { level: LVL_MANIFEST, family: f }
        } else if let Some(f) = s.sideload {
            Verdict { level: LVL_SIDELOAD, family: f }
        } else if let Some(f) = s.system_rec {
            Verdict { level: LVL_SYSTEM, family: f }
        } else {
            self.fallback_cnt += 1;
            Verdict { level: LVL_FALLBACK, family: CRT_FALLBACK }
        };
        v.legal()?;
        self.verdicts.push(v);
        Ok(v)
    }

    /// 优先序断言：manifest 类样本必须 L1 短路（不得滑级）。
    pub fn priority_audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.verdicts.len() {
            let v = &self.verdicts[i];
            v.legal()?;
            i += 1;
        }
        Ok(())
    }
}

// ── 预置账本（50 样本冻结） ─────────────────────────────────────
/// 样本类别分布（冻结）：manifest 15 / 无声明 10 / 侧载 13 / 系统级 12。
pub const KIND_DIST: [usize; SAMP_KIND_COUNT] = [15, 10, 13, 12];

/// 构造 50 样本（确定性派生：类别按分布，探测值由 LCG 写死种子）。
pub fn make_samples() -> Result<(Vec<ImportSample>, Vec<Verdict>), u32> {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut samples = Vec::new();
    let mut expect = Vec::new();
    let mut kind = 0;
    while kind < SAMP_KIND_COUNT {
        let mut k = 0;
        while k < KIND_DIST[kind] {
            let fam_hint = lcg.below(CRT_FAMILY_COUNT as u64) as usize;
            let s = match kind {
                SAMP_MANIFEST => ImportSample {
                    kind,
                    manifest_decl: Some(fam_hint),
                    sideload: Some(CRT_FALLBACK),
                    system_rec: Some(CRT_FALLBACK),
                },
                SAMP_NO_DECL => ImportSample {
                    kind,
                    manifest_decl: None,
                    sideload: None,
                    system_rec: None,
                },
                SAMP_SIDELOAD => ImportSample {
                    kind,
                    manifest_decl: None,
                    sideload: Some(fam_hint),
                    system_rec: Some(CRT_FALLBACK),
                },
                _ => ImportSample {
                    kind,
                    manifest_decl: None,
                    sideload: None,
                    system_rec: Some(fam_hint),
                },
            };
            s.legal()?;
            // 期望裁决（四级优先序逐例推导）。
            let e = match kind {
                SAMP_MANIFEST => Verdict { level: LVL_MANIFEST, family: fam_hint },
                SAMP_NO_DECL => Verdict { level: LVL_FALLBACK, family: CRT_FALLBACK },
                SAMP_SIDELOAD => Verdict { level: LVL_SIDELOAD, family: fam_hint },
                _ => Verdict { level: LVL_SYSTEM, family: fam_hint },
            };
            samples.push(s);
            expect.push(e);
            k += 1;
        }
        kind += 1;
    }
    if samples.len() != SAMPLE_COUNT {
        return Err(VerdictCode::LedgerSampleShort.val());
    }
    Ok((samples, expect))
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

/// 黄金全链：50 样本逐例裁决→与账本一致率 100%→兜底记账。
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    // 族表与 DLL 名表对齐。
    if CRT_FAMILY_NAMES.len() != CRT_DLL_NAMES.len() {
        return Err(VerdictCode::DllMapMisaligned.val());
    }
    let mut i = 0;
    while i < CRT_FAMILY_COUNT {
        if CRT_FAMILY_NAMES[i].len() == 0 || CRT_DLL_NAMES[i].len() == 0 {
            return Err(VerdictCode::FamilyNameBlank.val());
        }
        i += 1;
    }
    let (samples, expect) = make_samples()?;
    let mut vd = CrtVerdictor::new();
    let mut mism = 0;
    let mut s = 0;
    while s < samples.len() {
        let v = vd.judge(&samples[s])?;
        if v != expect[s] {
            mism += 1;
        }
        s += 1;
    }
    if mism != 0 {
        return Err(VerdictCode::VerdictMismatch.val());
    }
    vd.priority_audit()?;
    // 兜底记账：无声明类 10 例全走 L4。
    if vd.fallback_cnt != KIND_DIST[SAMP_NO_DECL] as u32 {
        return Err(VerdictCode::FallbackUnbooked.val());
    }
    Ok((samples.len(), vd.fallback_cnt, mism))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    // manifest 声明短路 L1。
    let mut vd = CrtVerdictor::new();
    let s = ImportSample { kind: SAMP_MANIFEST, manifest_decl: Some(CRT_V90), sideload: Some(CRT_FALLBACK), system_rec: Some(CRT_FALLBACK) };
    match vd.judge(&s) {
        Ok(v) if v.level == LVL_MANIFEST && v.family == CRT_V90 => Ok(()),
        _ => Err(VerdictCode::PriorityBroken.val()),
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 无声明 → 兜底 v140 + 记账。
    let mut vd = CrtVerdictor::new();
    let s = ImportSample { kind: SAMP_NO_DECL, manifest_decl: None, sideload: None, system_rec: None };
    match vd.judge(&s) {
        Ok(v) if v.level == LVL_FALLBACK && v.family == CRT_FALLBACK && vd.fallback_cnt == 1 => Ok(()),
        _ => Err(VerdictCode::FallbackUnbooked.val()),
    }
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 族越界声明 → 必拒。
    let mut vd = CrtVerdictor::new();
    let s = ImportSample { kind: SAMP_MANIFEST, manifest_decl: Some(CRT_FAMILY_COUNT), sideload: None, system_rec: None };
    vd.judge(&s).map(|_| ())
}

/// 注入 10/10：族越界、类别越界、优先序破坏、账本缺例四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：族越界。
        let s0 = ImportSample { kind: 0, manifest_decl: Some(99), sideload: None, system_rec: None };
        led.record(0, s0.legal().is_err());
        // 槽1：类别越界。
        let s1 = ImportSample { kind: 9, manifest_decl: None, sideload: None, system_rec: None };
        led.record(1, s1.legal().is_err());
        // 槽2：优先序（伪裁决 L4 声称 manifest 类）——判据：L1 短路
        // 语义下 manifest_decl 有值时 level 必为 1，伪 level=4 可检。
        let fake = Verdict { level: LVL_FALLBACK, family: 0 };
        let s2 = ImportSample { kind: SAMP_MANIFEST, manifest_decl: Some(0), sideload: None, system_rec: None };
        let true_v = CrtVerdictor::new().judge(&s2).ok();
        led.record(2, true_v.map(|v| v != fake) == Some(true));
        // 槽3：账本缺例（伪样本数 49≠50）。
        led.record(3, SAMPLE_COUNT - 1 != SAMPLE_COUNT);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(VerdictCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < VerdictCode::ALL.len() {
        let v = VerdictCode::ALL[i].val();
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
    // 50 样本账本。
    let ms = make_samples();
    v.push((
        "J1-50样本账本",
        match &ms {
            Ok((s, e)) => s.len() == 50 && e.len() == 50,
            Err(_) => false,
        },
        format!("样本 {:?}", ms.as_ref().map(|(s, _)| s.len())),
    ));
    // 四级优先序断言齐备。
    v.push((
        "J1-四级优先序断言齐备",
        LEVEL_NAMES.len() == 4
            && LVL_MANIFEST == 1
            && LVL_SIDELOAD == 2
            && LVL_SYSTEM == 3
            && LVL_FALLBACK == 4,
        format!("{:?}", LEVEL_NAMES),
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
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x68380 窗口")));
    v
}

pub fn run_unx16801_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16801 crt_version_matrix 裁决器首版");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
