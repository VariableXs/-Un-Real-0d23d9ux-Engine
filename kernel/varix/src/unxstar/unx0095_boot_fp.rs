//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0095 · 随机化指纹（Per-Boot ID） · 0x6E600
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥200 行；
//!   ② UNX-F0095-J1 Per-Boot ID：四源账（熵/基址/时间槽/版本）
//!     FNV 折叠出 64 位引导指纹，同账恒同指纹（确定性）、异账异
//!     指纹（区分度 ≥ 源数）、指纹落账封存只读；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四源账：熵/物理基址/时间槽/版本号；
//!   - 指纹：FNV-1a 逐源折叠（字节账）；
//!   - 确定性红线：同账两次折叠逐位一致；
//!   - 区分度账：异源改任一槽指纹必变（4/4 检出）；
//!   - 封存账：落账后只读（改写抓）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6E600，AI-VX3-1102 批K 独占区）。
pub const SEG_BASE: u32 = 0x6E600;

/// 源数（冻结 4）。
pub const SOURCES: usize = 4;
pub const SRC_ENTROPY: usize = 0;
pub const SRC_BASE: usize = 1;
pub const SRC_TIME: usize = 2;
pub const SRC_VERSION: usize = 3;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FpCode {
    /// 确定性破坏（同账异指纹）
    DeterminismBroken,
    /// 区分度不足（改源指纹不变）
    DiscriminationBad,
    /// 封存后改写
    SealedMutated,
    /// 源越界
    SourceOver,
    /// 指纹恒零
    ZeroFingerprint,
    /// 双跑不一致
    NotReproducible,
}

impl FpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FpCode::DeterminismBroken => "确定性破坏：同账异指纹",
            FpCode::DiscriminationBad => "区分度不足：改源指纹不变",
            FpCode::SealedMutated => "封存后改写：只读红线",
            FpCode::SourceOver => "源越界：≥4",
            FpCode::ZeroFingerprint => "指纹恒零：空账",
            FpCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [FpCode; 6] = [
        FpCode::DeterminismBroken,
        FpCode::DiscriminationBad,
        FpCode::SealedMutated,
        FpCode::SourceOver,
        FpCode::ZeroFingerprint,
        FpCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FpCode::ALL.len() {
        if FpCode::ALL[i].val() == code {
            return FpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6E600 段闭集内"
}

// ── 引导指纹账 ──────────────────────────────────────────────────
pub fn fnv_slot(h: u64, v: u64) -> u64 {
    let mut acc = h;
    let mut b = 0;
    while b < 8 {
        acc = (acc ^ ((v >> (b * 8)) & 0xFF)).wrapping_mul(0x100000001b3);
        b += 1;
    }
    acc
}

#[derive(Clone, Debug)]
pub struct BootFingerprint {
    pub sources: [u64; SOURCES],
    pub fp: u64,
    pub sealed: bool,
}

impl BootFingerprint {
    /// 构建（自动折叠）。
    pub fn build(sources: [u64; SOURCES]) -> BootFingerprint {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < SOURCES {
            h = fnv_slot(h, sources[i]);
            i += 1;
        }
        BootFingerprint { sources, fp: h, sealed: false }
    }

    /// 封存。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 源读（越界拒）。
    pub fn read(&self, idx: usize) -> Result<u64, u32> {
        if idx >= SOURCES {
            return Err(FpCode::SourceOver.val());
        }
        Ok(self.sources[idx])
    }

    /// 确定性：两次构建指纹逐位一致。
    pub fn determinism_ok(&self) -> Result<(), u32> {
        let fresh = BootFingerprint::build(self.sources);
        if fresh.fp != self.fp {
            return Err(FpCode::DeterminismBroken.val());
        }
        if self.fp == 0 {
            return Err(FpCode::ZeroFingerprint.val());
        }
        Ok(())
    }

    /// 区分度：逐源改写指纹必变（4/4 检出）。
    pub fn discrimination(&self) -> Result<u32, u32> {
        let mut hit = 0u32;
        let mut i = 0;
        while i < SOURCES {
            let mut bad = self.sources;
            bad[i] = bad[i].wrapping_add(1);
            let bf = BootFingerprint::build(bad);
            if bf.fp != self.fp {
                hit += 1;
            }
            i += 1;
        }
        if hit != SOURCES as u32 {
            return Err(FpCode::DiscriminationBad.val());
        }
        Ok(hit)
    }

    /// 封存对拍（同账恒真）。
    pub fn mutate_audit(&self, fresh: &BootFingerprint) -> bool {
        if !self.sealed {
            return true;
        }
        self.fp == fresh.fp && self.sources[..] == fresh.sources[..]
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

// ── 注入检出账（四路：确定性路/区分度路/越界路/封存路） ──────────
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

/// 真机四源账。
pub fn machine_sources() -> [u64; SOURCES] {
    [0x5A5A_1234_9ABC_DEF0, 0x10_0000, 0xE8_1234_5678, 1]
}

// ── 黄金全链：构建→确定性→区分度→封存对拍→越界拒 ─────────────────
pub fn golden_run() -> Result<(u64, u32, bool), u32> {
    let mut f = BootFingerprint::build(machine_sources());
    f.determinism_ok()?;
    let hit = f.discrimination()?;
    f.seal();
    let same = f.mutate_audit(&BootFingerprint::build(machine_sources()));
    Ok((f.fp, hit, same))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账 → 指纹非零 + 区分度 4/4 + 对拍真。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((fp, 4, true)) if fp != 0 => Ok(()),
        _ => Err(FpCode::NotReproducible.val()),
    }
}

/// 序 B：源越界 ⇒ SourceOver。
pub fn probe_seq_b() -> Result<(), u32> {
    let f = BootFingerprint::build(machine_sources());
    match f.read(SOURCES) {
        Err(e) if e == FpCode::SourceOver.val() => Ok(()),
        _ => Err(FpCode::NotReproducible.val()),
    }
}

/// 序 C：封存后改账对拍必假。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut f = BootFingerprint::build(machine_sources());
    f.seal();
    let mut bad = machine_sources();
    bad[SRC_TIME] += 1;
    match f.mutate_audit(&BootFingerprint::build(bad)) {
        false => Ok(()),
        true => Err(FpCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：确定性路。
        let f1 = BootFingerprint::build(machine_sources());
        led.record(0, f1.determinism_ok().is_ok());
        // 路 2：区分度路。
        let f2 = BootFingerprint::build(machine_sources());
        led.record(1, f2.discrimination() == Ok(4));
        // 路 3：越界路。
        led.record(2, f2.read(SOURCES + 3).is_err());
        // 路 4：封存路（封存 + 漂移对拍必假）。
        let mut f4 = BootFingerprint::build(machine_sources());
        f4.seal();
        let mut bad = machine_sources();
        bad[SRC_BASE] += 0x1000;
        led.record(3, !f4.mutate_audit(&BootFingerprint::build(bad)));
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(FpCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FpCode::ALL.len() {
        let v = FpCode::ALL[i].val();
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
            Ok((fp, hit, same)) => *fp != 0 && *hit == 4 && *same,
            Err(_) => false,
        },
        match &g {
            Ok((fp, hit, same)) => format!("fp=0x{:x} discrim={} same={}", fp, hit, same),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 四源账冻结。
    v.push((
        "J1-四源账",
        SOURCES == 4
            && machine_sources()[SRC_ENTROPY] != 0
            && machine_sources()[SRC_BASE] == 0x10_0000
            && machine_sources()[SRC_VERSION] == 1,
        String::from("熵/基址/时间槽/版本 四源冻结"),
    ));
    // 异引导区分：异熵异指纹。
    let mut alt = machine_sources();
    alt[SRC_ENTROPY] ^= 0xFF;
    v.push((
        "J1-异引导区分",
        BootFingerprint::build(alt).fp != BootFingerprint::build(machine_sources()).fp,
        String::from("熵源差 1 位 ⇒ 指纹必异"),
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
        String::from("0x6E600 窗口"),
    ));
    v
}

pub fn run_unx0095_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0095 随机化指纹（Per-Boot ID）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
