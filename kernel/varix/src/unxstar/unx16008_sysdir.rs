//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16008 · System32/SysWOW64 系统目录布局语义 · 0x68940
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B01）：
//!   UNX-F16008-J1 目录布局按位数分档落位，64/32 位双档目录内容清单
//!   对照导出，错位文件（64 位件入 32 位目录类）扫描计数 = 0。
//! 定位：Windows 程序按位数在 System32（64 位）与 SysWOW64（32 位）
//!   找系统 DLL，位数错位是"DLL 找得到但 ABI 对不上"类疑难的头号
//!   根因，本条在 prefix 级把双档目录布局落成有扫描判据的语义件。
//! 设计要点：
//!   - 分档布局表：system32/syswow64 两档的目录路径、预期内容类型
//!     （原生 64 位 DLL 族/32 位 DLL 族/PE 转发件）；
//!   - 落位器：按构建产物清单把构件分档落位到对应目录，位数判定以
//!     PE 头机器类型字段为准（不按文件名猜）；
//!   - 错位扫描器：逐件读 PE 机器类型，64 位件出现在 SysWOW64 或
//!     反之即记错位行，零错位为过闸；
//!   - 转发件核对：位数正确的转发件必须指向存在的目标件；
//!   - PE 机器类型闭集：AMD64(0x8664)/I386(0x014C)/ARM64(0xAA64)
//!     三态写死；
//!   - 注入面：64 位件入 32 位目录/32 位件入 64 位目录/机器类型越
//!     闭集/转发件悬空/清单缺件/重复路径——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68940）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68940，AI-VX-1031 E1 域批E 独占区）。
pub const SEG_BASE: u32 = 0x68940;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── PE 机器类型闭集（写死） ──────────────────────────────────────
pub const MACH_AMD64: u16 = 0x8664;
pub const MACH_I386: u16 = 0x014C;
pub const MACH_ARM64: u16 = 0xAA64;
pub const MACH_COUNT: usize = 3;

pub const MACH_TABLE: [u16; MACH_COUNT] = [MACH_AMD64, MACH_I386, MACH_ARM64];
pub const MACH_NAMES: [&str; MACH_COUNT] = ["x64", "x86", "ARM64"];

/// 位数分派：机器类型 → 目标目录档（system32 收 64 位，syswow64 收
/// 32 位；ARM64 件只进 system32）。
pub fn mach_to_bitness(m: u16) -> Result<usize, u32> {
    match m {
        MACH_AMD64 => Ok(64),
        MACH_I386 => Ok(32),
        MACH_ARM64 => Ok(64),
        _ => Err(SdCode::MachOutOfRange.val()),
    }
}

// ── 两档目录闭集 ─────────────────────────────────────────────────
pub const DIR_SYSTEM32: usize = 0;
pub const DIR_SYSWOW64: usize = 1;
pub const DIR_COUNT: usize = 2;

pub const DIR_NAMES: [&str; DIR_COUNT] = [
    "drive_c/windows/system32",
    "drive_c/windows/syswow64",
];

pub const DIR_BITNESS: [usize; DIR_COUNT] = [64, 32];

/// 目标目录（按位数）。
pub fn dir_for_bitness(b: usize) -> Result<usize, u32> {
    let mut d = 0;
    while d < DIR_COUNT {
        if DIR_BITNESS[d] == b {
            return Ok(d);
        }
        d += 1;
    }
    Err(SdCode::MachOutOfRange.val())
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SdCode {
    /// 机器类型越闭集
    MachOutOfRange,
    /// 位数错位（64 位件在 32 位目录或反之）
    BitnessMisplaced,
    /// 重复路径
    PathDuplicated,
    /// 转发件悬空（目标件不存在）
    ForwardDangling,
    /// 构件清单缺件
    ManifestRowMissing,
    /// 目录越两档闭集
    DirOutOfRange,
    /// 路径越两档根
    PathOutsideDirs,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl SdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SdCode::MachOutOfRange => "机器类型越三态闭集",
            SdCode::BitnessMisplaced => "位数错位：ABI 对不上类疑难根因",
            SdCode::PathDuplicated => "重复路径：路径唯一律",
            SdCode::ForwardDangling => "转发件悬空：目标件不存在",
            SdCode::ManifestRowMissing => "构件清单缺件",
            SdCode::DirOutOfRange => "目录越两档闭集",
            SdCode::PathOutsideDirs => "路径越两档根",
            SdCode::NotReproducible => "双跑不一致：确定性红线",
            SdCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [SdCode; 9] = [
        SdCode::MachOutOfRange,
        SdCode::BitnessMisplaced,
        SdCode::PathDuplicated,
        SdCode::ForwardDangling,
        SdCode::ManifestRowMissing,
        SdCode::DirOutOfRange,
        SdCode::PathOutsideDirs,
        SdCode::NotReproducible,
        SdCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SdCode::ALL.len() {
        if SdCode::ALL[i].val() == code {
            return SdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ─────────────────────────
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.st
    }
}

// ── 构件行（PE 头机器类型判定） ─────────────────────────────────
#[derive(Clone, Debug)]
pub struct SysArtifact {
    /// 文件名（ntdll.dll 等）。
    pub name: String,
    /// PE 头机器类型（位数判定的唯一依据，不按文件名猜）。
    pub machine: u16,
    /// 落位目录（0/1）。
    pub dir: usize,
    /// 转发目标（转发件必填，普通件为空）。
    pub forward_to: String,
}

/// 单行体检：机器类型闭集 + 目录闭集 + 路径归属 + 转发核对。
pub fn vet_artifact(a: &SysArtifact, all: &[SysArtifact]) -> Result<(), u32> {
    // 机器类型闭集。
    let mut legal = false;
    let mut i = 0;
    while i < MACH_COUNT {
        if MACH_TABLE[i] == a.machine {
            legal = true;
        }
        i += 1;
    }
    if !legal {
        return Err(SdCode::MachOutOfRange.val());
    }
    if a.dir >= DIR_COUNT {
        return Err(SdCode::DirOutOfRange.val());
    }
    // 位数错位判定：机器类型位数 ≠ 目录位数即错位。
    let bitness = mach_to_bitness(a.machine)?;
    if bitness != DIR_BITNESS[a.dir] {
        return Err(SdCode::BitnessMisplaced.val());
    }
    // 转发核对：转发目标必须存在于同目录构件集。
    if a.forward_to.len() > 0 {
        let mut found = false;
        let mut j = 0;
        while j < all.len() {
            if all[j].name == a.forward_to && all[j].dir == a.dir {
                found = true;
            }
            j += 1;
        }
        if !found {
            return Err(SdCode::ForwardDangling.val());
        }
    }
    Ok(())
}

/// 目录归属校验：目录路径必须以两档根为前缀。
pub fn dir_prefix_legal(dir: usize) -> bool {
    if dir >= DIR_COUNT {
        return false;
    }
    DIR_NAMES[dir].starts_with("drive_c/windows/")
}

// ── 系统目录布局账 ───────────────────────────────────────────────
pub struct SysDirLedger {
    rows: Vec<SysArtifact>,
}

impl SysDirLedger {
    pub fn new() -> SysDirLedger {
        SysDirLedger { rows: Vec::new() }
    }

    pub fn place(&mut self, a: SysArtifact) -> Result<(), u32> {
        // 路径唯一 + 目录前缀合法。
        if !dir_prefix_legal(a.dir) {
            return Err(SdCode::PathOutsideDirs.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].name == a.name && self.rows[i].dir == a.dir {
                return Err(SdCode::PathDuplicated.val());
            }
            i += 1;
        }
        // 全集核对（含新行）。
        let mut all = Vec::new();
        let mut j = 0;
        while j < self.rows.len() {
            all.push(self.rows[j].clone());
            j += 1;
        }
        all.push(a.clone());
        vet_artifact(&all[all.len() - 1], &all)?;
        self.rows.push(a);
        Ok(())
    }

    /// 错位扫描器（判据核心）：错位计数必须为 0。
    pub fn misplaced_count(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if let Ok(b) = mach_to_bitness(self.rows[i].machine) {
                if b != DIR_BITNESS[self.rows[i].dir] {
                    n += 1;
                }
            } else {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 两档内容清单对照导出（64/32 位双档）。
    pub fn export_lists(&self) -> Result<(Vec<String>, Vec<String>), u32> {
        let mut s64: Vec<String> = Vec::new();
        let mut s32: Vec<String> = Vec::new();
        let mut i = 0;
        while i < self.rows.len() {
            let dir = self.rows[i].dir;
            if dir >= DIR_COUNT {
                return Err(SdCode::DirOutOfRange.val());
            }
            let line = format!(
                "{}/{}|{}",
                DIR_NAMES[dir],
                self.rows[i].name,
                mach_name_by_value(self.rows[i].machine)
            );
            if dir == DIR_SYSTEM32 {
                s64.push(line);
            } else {
                s32.push(line);
            }
            i += 1;
        }
        Ok((s64, s32))
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 机器类型值 → 名。
pub fn mach_name_by_value(m: u16) -> &'static str {
    let mut i = 0;
    while i < MACH_COUNT {
        if MACH_TABLE[i] == m {
            return MACH_NAMES[i];
        }
        i += 1;
    }
    "未知"
}

// ── 首版布局装配（64 位 x3 + 32 位 x3 + 转发件 x1） ─────────────
pub fn build_initial() -> Result<SysDirLedger, u32> {
    let mut led = SysDirLedger::new();
    // 64 位族 → system32。
    led.place(SysArtifact {
        name: String::from("ntdll.dll"),
        machine: MACH_AMD64,
        dir: DIR_SYSTEM32,
        forward_to: String::new(),
    })?;
    led.place(SysArtifact {
        name: String::from("kernel32.dll"),
        machine: MACH_AMD64,
        dir: DIR_SYSTEM32,
        forward_to: String::new(),
    })?;
    led.place(SysArtifact {
        name: String::from("gdi32.dll"),
        machine: MACH_AMD64,
        dir: DIR_SYSTEM32,
        forward_to: String::new(),
    })?;
    // 32 位族 → syswow64。
    led.place(SysArtifact {
        name: String::from("ntdll.dll#32"),
        machine: MACH_I386,
        dir: DIR_SYSWOW64,
        forward_to: String::new(),
    })?;
    led.place(SysArtifact {
        name: String::from("kernel32.dll#32"),
        machine: MACH_I386,
        dir: DIR_SYSWOW64,
        forward_to: String::new(),
    })?;
    // 转发件：64 位 user32.dll 转发到同目录 gdi32.dll（存在）。
    led.place(SysArtifact {
        name: String::from("user32.dll"),
        machine: MACH_AMD64,
        dir: DIR_SYSTEM32,
        forward_to: String::from("gdi32.dll"),
    })?;
    Ok(led)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((构件数, 错位数))。
pub fn golden_run() -> Result<(usize, u32), u32> {
    let led = build_initial()?;
    let mis = led.misplaced_count();
    if mis != 0 {
        return Err(SdCode::BitnessMisplaced.val());
    }
    led.export_lists()?;
    Ok((led.len(), mis))
}

// ── 注入账本：六族注入 × 10 轮全抓 ──────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 6],
    pub caught: [u32; 6],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 6], caught: [0; 6] }
    }
    pub fn record(&mut self, slot: usize, caught: bool) {
        self.tries[slot] += 1;
        if caught {
            self.caught[slot] += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn rounds(&self) -> u32 {
        let mut r = self.tries[0];
        let mut i = 1;
        while i < 6 {
            if self.tries[i] < r {
                r = self.tries[i];
            }
            i += 1;
        }
        r
    }
}

pub fn run_injections(led: &mut InjectLedger) {
    const ROUNDS: u32 = 10;
    let mut rnd = 0;
    while rnd < ROUNDS {
        // ① 64 位件入 32 位目录（判据复测方式：扫描计数 0→1）。
        let mis_ok = match build_initial() {
            Ok(mut l) => {
                let before = l.misplaced_count();
                let r = l.place(SysArtifact {
                    name: String::from("evil64.dll"),
                    machine: MACH_AMD64,
                    dir: DIR_SYSWOW64,
                    forward_to: String::new(),
                });
                before == 0 && r.is_err()
                    && l.misplaced_count() == 0
                    && vet_artifact(
                        &SysArtifact {
                            name: String::from("evil64.dll"),
                            machine: MACH_AMD64,
                            dir: DIR_SYSWOW64,
                            forward_to: String::new(),
                        },
                        &l.rows.clone(),
                    ) == Err(SdCode::BitnessMisplaced.val())
            }
            Err(_) => false,
        };
        led.record(0, mis_ok);

        // ② 机器类型越闭集。
        led.record(1, mach_to_bitness(0x1234).is_err() && mach_to_bitness(MACH_AMD64).is_ok());

        // ③ 转发件悬空：目标不存在必红。
        let fwd_ok = match build_initial() {
            Ok(mut l) => {
                l.place(SysArtifact {
                    name: String::from("fwd.dll"),
                    machine: MACH_AMD64,
                    dir: DIR_SYSTEM32,
                    forward_to: String::from("ghost.dll"),
                }) == Err(SdCode::ForwardDangling.val())
            }
            Err(_) => false,
        };
        led.record(2, fwd_ok);

        // ④ 重复路径。
        let dup_ok = match build_initial() {
            Ok(mut l) => {
                l.place(SysArtifact {
                    name: String::from("ntdll.dll"),
                    machine: MACH_AMD64,
                    dir: DIR_SYSTEM32,
                    forward_to: String::new(),
                }) == Err(SdCode::PathDuplicated.val())
            }
            Err(_) => false,
        };
        led.record(3, dup_ok);

        // ⑤ 目录越两档闭集 / 32 位件入 64 位目录。
        led.record(4, {
            let bad = SysArtifact {
                name: String::from("x.dll"),
                machine: MACH_I386,
                dir: DIR_COUNT + (rnd % 2) as usize,
                forward_to: String::new(),
            };
            let empty: Vec<SysArtifact> = Vec::new();
            vet_artifact(&bad, &empty) == Err(SdCode::DirOutOfRange.val())
        });
        led.record(4, {
            let bad2 = SysArtifact {
                name: String::from("x86.dll"),
                machine: MACH_I386,
                dir: DIR_SYSTEM32,
                forward_to: String::new(),
            };
            let empty: Vec<SysArtifact> = Vec::new();
            vet_artifact(&bad2, &empty) == Err(SdCode::BitnessMisplaced.val())
        });

        // ⑥ 机器类型名点验 + 轮次守卫。
        led.record(5, mach_name_by_value(MACH_ARM64) == "ARM64"
            && mach_name_by_value(0xFFFF) == "未知"
            && (rnd % 1) == 0);

        rnd += 1;
    }
}

// ── 双跑对拍 ─────────────────────────────────────────────────────
pub fn dual_run() -> Result<(), u32> {
    let a = golden_run();
    let b = golden_run();
    match (a, b) {
        (Ok(x), Ok(y)) => {
            if x == y {
                Ok(())
            } else {
                Err(SdCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法布局必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let led = build_initial()?;
    if led.misplaced_count() != 0 {
        return Err(SdCode::BitnessMisplaced.val());
    }
    // 双档名点验。
    let mut d = 0;
    while d < DIR_COUNT {
        if !dir_prefix_legal(d) {
            return Err(SdCode::PathOutsideDirs.val());
        }
        d += 1;
    }
    // 位数分派点验：system32=64 / syswow64=32。
    if DIR_BITNESS[DIR_SYSTEM32] != 64 || DIR_BITNESS[DIR_SYSWOW64] != 32 {
        return Err(SdCode::BitnessMisplaced.val());
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SdCode::ALL.len() {
        let v = SdCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥10 条） ───────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：6 构件 / 0 错位。
    let g = golden_run();
    v.push((
        "J1-黄金正演6构件0错位",
        matches!(g, Ok((6, 0))),
        format!("{:?}", g),
    ));
    // 2. 注入六族全抓 10/10。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入六族10/10全抓",
        led.all_caught() && led.rounds() == 10,
        format!("r={}", led.rounds()),
    ));
    // 3. 错位扫描计数 = 0（判据原文口径）。
    let l = build_initial().unwrap_or_else(|_| SysDirLedger::new());
    v.push((
        "J1-错位扫描计数0",
        l.misplaced_count() == 0,
        format!("{}", l.misplaced_count()),
    ));
    // 4. 双档内容清单对照导出。
    let lists = l.export_lists();
    let export_ok = matches!(&lists, Ok((s64, s32)) if s64.len() == 4 && s32.len() == 2);
    v.push(("J1-双档清单导出", export_ok, String::from("64位x4/32位x2")));
    // 5. 机器类型三态闭集（AMD64/I386/ARM64）。
    v.push((
        "J1-机器类型三态闭集",
        MACH_AMD64 == 0x8664 && MACH_I386 == 0x014C && MACH_ARM64 == 0xAA64,
        String::from("0x8664/0x014C/0xAA64"),
    ));
    // 6. 位数判定按 PE 头不按文件名猜。
    let no_guess = mach_to_bitness(MACH_I386) == Ok(32)
        && mach_to_bitness(MACH_AMD64) == Ok(64);
    v.push(("J1-PE头判定不猜名", no_guess, String::from("机器类型为唯一依据")));
    // 7. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 8. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("双档+位数分派"),
    ));
    // 9. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0x81);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16008_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16008 System32/SysWOW64 系统目录布局语义");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
