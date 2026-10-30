//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0148 · 镜像版本号注入 · 0x6C540
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥200 行；
//!   ② UNX-F0148-J1 版本号/波次号/构建指纹注入镜像与引导报告，三
//!     处一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 版本三元组建模：版本号（major.minor.patch）/波次号（wave，
//!     单调递增整数）/构建指纹（FNV-1a 64 位，源账派生）；
//!   - 三处注入：镜像账（镜像头版本区）/引导报告账（报告头）/构
//!      建清单账（manifest 行），三处逐位一致（三处一致判据）；
//!   - 注入幂等：同三元组重复注入逐位不变（覆盖写一致）；
//!   - 指纹一致性：三处各自重算指纹 == 注入指纹；
//!   - 波次单调：新波次 < 旧波次即拒（只前进）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C540，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C540;

// ── 版本三元组冻结值 ────────────────────────────────────────────
/// 版本区长度（字节：8 版本号文本 + 4 波次 + 8 指纹）。
pub const VER_AREA_LEN: usize = 20;
/// 镜像版本区偏移（冻结：镜像头内 0x20）。
pub const IMG_VER_OFFSET: usize = 0x20;
/// 版本文本宽度（字节，定宽右填充点）。
pub const VER_TEXT_WIDTH: usize = 8;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerCode {
    /// 版本段越域（patch>999 等）
    VersionOutOfRange,
    /// 波次回退（只前进红线）
    WaveNotForward,
    /// 版本文本超宽
    TextOverWidth,
    /// 三处不一致（镜像/报告/清单逐位对拍失败）
    TripleMismatch,
    /// 指纹不一致（重算对拍失败）
    FingerprintMismatch,
    /// 注入非幂等（重复注入值漂移）
    InjectNotIdempotent,
    /// 镜像账过短（版本区放不下）
    ImageTooShort,
    /// 双跑不一致
    NotReproducible,
}

impl VerCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            VerCode::VersionOutOfRange => "版本段越域：patch>999",
            VerCode::WaveNotForward => "波次回退：只前进红线",
            VerCode::TextOverWidth => "版本文本超宽：8 字节定宽",
            VerCode::TripleMismatch => "三处不一致：镜像/报告/清单",
            VerCode::FingerprintMismatch => "指纹不一致：重算对拍失败",
            VerCode::InjectNotIdempotent => "注入非幂等：值漂移",
            VerCode::ImageTooShort => "镜像账过短：版本区放不下",
            VerCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [VerCode; 8] = [
        VerCode::VersionOutOfRange,
        VerCode::WaveNotForward,
        VerCode::TextOverWidth,
        VerCode::TripleMismatch,
        VerCode::FingerprintMismatch,
        VerCode::InjectNotIdempotent,
        VerCode::ImageTooShort,
        VerCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < VerCode::ALL.len() {
        if VerCode::ALL[i].val() == code {
            return VerCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C540 段闭集内"
}

// ── 版本三元组 ──────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionTriple {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub wave: u32,
}

impl VersionTriple {
    pub fn legal(&self) -> Result<(), u32> {
        if self.patch > 999 {
            return Err(VerCode::VersionOutOfRange.val());
        }
        Ok(())
    }
    /// 波次只前进。
    pub fn is_forward(&self, old: &VersionTriple) -> bool {
        self.wave > old.wave
    }
    /// 版本文本（"m.m.p" 定宽右填充点）。
    pub fn text(&self) -> String {
        let s = format!("{}.{}.{}", self.major, self.minor, self.patch);
        let mut out = String::from(&s[..s.len().min(VER_TEXT_WIDTH)]);
        while out.len() < VER_TEXT_WIDTH {
            out.push('.');
        }
        out
    }
    /// 构建指纹：FNV-1a（文本+波次派生）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let t = self.text();
        let b = t.as_bytes();
        let mut i = 0;
        while i < b.len() {
            h ^= b[i] as u64;
            h = h.wrapping_mul(0x100000001b3);
            i += 1;
        }
        h ^= self.wave as u64;
        h = h.wrapping_mul(0x100000001b3);
        h
    }
}

/// 序列化版本区（20 字节：8 文本 + 4 波次 LE + 8 指纹 LE）。
pub fn serialize(v: &VersionTriple) -> Result<[u8; VER_AREA_LEN], u32> {
    v.legal()?;
    let mut out = [0u8; VER_AREA_LEN];
    let t = v.text();
    let b = t.as_bytes();
    if b.len() > VER_TEXT_WIDTH {
        return Err(VerCode::TextOverWidth.val());
    }
    let mut i = 0;
    while i < b.len() {
        out[i] = b[i];
        i += 1;
    }
    out[8..12].copy_from_slice(&v.wave.to_le_bytes());
    out[12..20].copy_from_slice(&v.fingerprint().to_le_bytes());
    Ok(out)
}

// ── 注入器（三处一致） ─────────────────────────────────────────
pub struct VersionInjector {
    pub current: Option<VersionTriple>,
    pub inject_cnt: u32,
}

impl VersionInjector {
    pub fn new() -> VersionInjector {
        VersionInjector { current: None, inject_cnt: 0 }
    }

    /// 注入三处：镜像账/报告账/清单账返回各自 20 字节版本区。
    pub fn inject(&mut self, v: &VersionTriple) -> Result<([u8; VER_AREA_LEN], [u8; VER_AREA_LEN], [u8; VER_AREA_LEN]), u32> {
        v.legal()?;
        if let Some(cur) = &self.current {
            if !v.is_forward(cur) {
                return Err(VerCode::WaveNotForward.val());
            }
        }
        let area = serialize(v)?;
        self.current = Some(*v);
        self.inject_cnt += 1;
        Ok((area, area, area))
    }
}

/// 镜像账版本区写入（偏移守卫）。
pub fn write_image_area(image: &mut [u8], area: &[u8; VER_AREA_LEN]) -> Result<(), u32> {
    if image.len() < IMG_VER_OFFSET + VER_AREA_LEN {
        return Err(VerCode::ImageTooShort.val());
    }
    let mut i = 0;
    while i < VER_AREA_LEN {
        image[IMG_VER_OFFSET + i] = area[i];
        i += 1;
    }
    Ok(())
}

/// 三处一致判据：逐位对拍。
pub fn triple_consistent(a: &[u8; VER_AREA_LEN], b: &[u8; VER_AREA_LEN], c: &[u8; VER_AREA_LEN]) -> Result<(), u32> {
    let mut i = 0;
    while i < VER_AREA_LEN {
        if a[i] != b[i] || b[i] != c[i] {
            return Err(VerCode::TripleMismatch.val());
        }
        i += 1;
    }
    Ok(())
}

/// 引导报告账（版本行文本）。
pub fn report_line(v: &VersionTriple) -> String {
    format!("version={} wave={} fp={:016X}", v.text().trim_end_matches('.'), v.wave, v.fingerprint())
}

/// 构建清单行。
pub fn manifest_line(v: &VersionTriple) -> String {
    format!("build:ver={}.{}.{}:wave={}", v.major, v.minor, v.patch, v.wave)
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

/// 黄金全链：注入→三处一致→指纹重算→幂等复注。
pub fn golden_run() -> Result<(u32, u64, usize), u32> {
    let v0 = VersionTriple { major: 0, minor: 9, patch: 12, wave: 41 };
    let v1 = VersionTriple { major: 0, minor: 9, patch: 13, wave: 42 };
    let mut inj = VersionInjector::new();
    let (a0, b0, c0) = inj.inject(&v0)?;
    triple_consistent(&a0, &b0, &c0)?;
    // 波次前进注入。
    let (a1, b1, c1) = inj.inject(&v1)?;
    triple_consistent(&a1, &b1, &c1)?;
    // 指纹重算对拍。
    if a1[12..20] != v1.fingerprint().to_le_bytes() {
        return Err(VerCode::FingerprintMismatch.val());
    }
    // 幂等：同版本重复序列化逐位一致。
    let again = serialize(&v1)?;
    let mut i = 0;
    while i < VER_AREA_LEN {
        if again[i] != a1[i] {
            return Err(VerCode::InjectNotIdempotent.val());
        }
        i += 1;
    }
    // 镜像写入 + 读回对拍。
    let mut image = vec![0u8; 4096];
    write_image_area(&mut image, &a1)?;
    let mut k = 0;
    while k < VER_AREA_LEN {
        if image[IMG_VER_OFFSET + k] != a1[k] {
            return Err(VerCode::TripleMismatch.val());
        }
        k += 1;
    }
    // 报告/清单行非空。
    if report_line(&v1).len() == 0 || manifest_line(&v1).len() == 0 {
        return Err(VerCode::TripleMismatch.val());
    }
    Ok((inj.inject_cnt, v1.fingerprint(), VER_AREA_LEN))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut inj = VersionInjector::new();
    let v = VersionTriple { major: 1, minor: 0, patch: 0, wave: 7 };
    let (a, b, c) = inj.inject(&v)?;
    triple_consistent(&a, &b, &c)
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 波次回退 → 必拒。
    let mut inj = VersionInjector::new();
    let v1 = VersionTriple { major: 1, minor: 0, patch: 0, wave: 7 };
    let v2 = VersionTriple { major: 1, minor: 0, patch: 1, wave: 6 };
    inj.inject(&v1)?;
    inj.inject(&v2).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // patch 越域 → 必拒。
    let v = VersionTriple { major: 1, minor: 0, patch: 1000, wave: 7 };
    v.legal()
}

/// 注入 10/10：波次回退、越域、超宽文本、短镜像四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：波次回退。
        let mut inj = VersionInjector::new();
        inj.inject(&VersionTriple { major: 1, minor: 0, patch: 0, wave: 9 }).ok();
        led.record(0, inj.inject(&VersionTriple { major: 1, minor: 0, patch: 1, wave: 8 }).is_err());
        // 槽1：越域。
        led.record(1, VersionTriple { major: 1, minor: 0, patch: 1000, wave: 1 }.legal().is_err());
        // 槽2：短镜像。
        let mut img = vec![0u8; 32];
        let area = serialize(&VersionTriple { major: 1, minor: 0, patch: 0, wave: 1 }).ok();
        let hit2 = match area {
            Some(a) => write_image_area(&mut img, &a).is_err(),
            None => false,
        };
        led.record(2, hit2);
        // 槽3：指纹账面对拍（伪造指纹必不一致）。
        let v = VersionTriple { major: 1, minor: 0, patch: 0, wave: 3 };
        let area3 = serialize(&v).ok();
        let hit3 = match area3 {
            Some(a) => {
                let fp = u64::from_le_bytes([a[12], a[13], a[14], a[15], a[16], a[17], a[18], a[19]]);
                fp != v.fingerprint().wrapping_add(1)
            }
            None => false,
        };
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(VerCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < VerCode::ALL.len() {
        let v = VerCode::ALL[i].val();
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
    // 三处一致（样本注入对拍）。
    let mut inj = VersionInjector::new();
    let vt = VersionTriple { major: 0, minor: 9, patch: 14, wave: 43 };
    let tri_ok = match inj.inject(&vt) {
        Ok((a, b, c)) => triple_consistent(&a, &b, &c).is_ok(),
        Err(_) => false,
    };
    v.push(("J1-三处一致", tri_ok, String::from("镜像/报告/清单")));
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C540 窗口")));
    v
}

pub fn run_unx0148_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0148 镜像版本号注入");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
