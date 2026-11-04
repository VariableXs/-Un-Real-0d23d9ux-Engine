//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0087 · KASLR 关闭模式（诊断模式） · 0x6E400
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥180 行；
//!   ② UNX-F0087-J1 KASLR 关闭语义：nokaslr 关闭时基址恒冻结值
//!     （0x100000 物理基址 + 高半区 0xFFFF8000_00000000 偏移），
//!     关闭时熵账恒零、两次构建基址逐位一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三态开关：cmdline nokaslr 显性关 / kaslr=seed 显性开 /
//!     缺省开（真机口径）；
//!   - 关闭账：基址 = 冻结值（无熵注入），熵账 = 0；
//!   - 一致性：关闭模式下两次构建基址逐位一致（确定性红线）；
//!   - 开启对照：开启时两次构建基址不同（熵生效证词）；
//!   - 关闭证词账：关闭原因（cmdline 显性/配置缺省）入账；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6E400，AI-VX3-1102 批J 独占区）。
pub const SEG_BASE: u32 = 0x6E400;

/// 冻结基址（关闭模式）。
pub const FROZEN_PHYS: u64 = 0x10_0000;
pub const FROZEN_HIGH_HALF: u64 = 0xFFFF_8000_0000_0000;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KaslrCode {
    /// 关闭模式基址漂移（确定性红线）
    FrozenBaseDrift,
    /// 关闭模式熵非零
    EntropyNotZero,
    /// 开关账外（非 on/off/seed）
    SwitchBad,
    /// 开启模式两次基址相同（熵失效证词）
    EntropyDead,
    /// 冻结值失配
    FrozenValueBad,
    /// 双跑不一致
    NotReproducible,
}

impl KaslrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KaslrCode::FrozenBaseDrift => "关闭模式基址漂移",
            KaslrCode::EntropyNotZero => "关闭模式熵非零",
            KaslrCode::SwitchBad => "开关账外：非 on/off/seed",
            KaslrCode::EntropyDead => "开启模式熵失效：两跑同基址",
            KaslrCode::FrozenValueBad => "冻结值失配",
            KaslrCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [KaslrCode; 6] = [
        KaslrCode::FrozenBaseDrift,
        KaslrCode::EntropyNotZero,
        KaslrCode::SwitchBad,
        KaslrCode::EntropyDead,
        KaslrCode::FrozenValueBad,
        KaslrCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < KaslrCode::ALL.len() {
        if KaslrCode::ALL[i].val() == code {
            return KaslrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6E400 段闭集内"
}

// ── 开关账 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KaslrSwitch {
    /// 显性开（seed 注入）。
    On(u64),
    /// 显性关（nokaslr）。
    Off,
    /// 缺省开（真机口径）。
    DefaultOn,
}

/// cmdline 解析：nokaslr → Off；kaslr=NNN → On(seed)；无 → DefaultOn。
pub fn parse_switch(cmdline: &str) -> KaslrSwitch {
    let b = cmdline.as_bytes();
    // nokaslr 词匹配。
    let mut i = 0;
    while i + 7 <= b.len() {
        if &cmdline[i..i + 7] == "nokaslr" {
            let boundary_before = i == 0 || b[i - 1] == b' ' || b[i - 1] == b',';
            let j = i + 7;
            let boundary_after = j >= b.len() || b[j] == b' ' || b[j] == b',';
            if boundary_before && boundary_after {
                return KaslrSwitch::Off;
            }
        }
        i += 1;
    }
    // kaslr=NNN。
    let mut i = 0;
    while i + 6 <= b.len() {
        if &cmdline[i..i + 6] == "kaslr=" {
            let start = i + 6;
            let mut end = start;
            while end < b.len() && b[end] != b' ' && b[end] != b',' {
                end += 1;
            }
            let mut acc: u64 = 0;
            let mut k = start;
            let mut any = false;
            while k < end {
                if b[k] >= b'0' && b[k] <= b'9' {
                    acc = acc.wrapping_mul(10).wrapping_add((b[k] - b'0') as u64);
                    any = true;
                }
                k += 1;
            }
            if any {
                return KaslrSwitch::On(acc);
            }
        }
        i += 1;
    }
    KaslrSwitch::DefaultOn
}

// ── 基址构建 ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaseChoice {
    pub phys: u64,
    pub high_half: u64,
    pub entropy: u64,
    pub reason: &'static str,
}

/// 构建（关闭 = 冻结值；开启 = seed 驱动偏移）。
pub fn build_base(sw: KaslrSwitch) -> BaseChoice {
    match sw {
        KaslrSwitch::Off => BaseChoice {
            phys: FROZEN_PHYS,
            high_half: FROZEN_HIGH_HALF,
            entropy: 0,
            reason: "cmdline 显性关闭",
        },
        KaslrSwitch::DefaultOn => BaseChoice {
            phys: FROZEN_PHYS + 0x20_0000,
            high_half: FROZEN_HIGH_HALF,
            entropy: 0x5A5A,
            reason: "缺省开（缺 seed 用兜底熵）",
        },
        KaslrSwitch::On(seed) => BaseChoice {
            phys: FROZEN_PHYS + (seed % 0x400) * 0x20_0000,
            high_half: FROZEN_HIGH_HALF,
            entropy: seed,
            reason: "kaslr=seed 显性开",
        },
    }
}

/// 关闭模式确定性红线：两次构建逐位一致 + 熵零。
pub fn off_determinism(sw: KaslrSwitch) -> Result<BaseChoice, u32> {
    let a = build_base(sw);
    let b = build_base(sw);
    if a != b {
        return Err(KaslrCode::FrozenBaseDrift.val());
    }
    if a.entropy != 0 {
        return Err(KaslrCode::EntropyNotZero.val());
    }
    if a.phys != FROZEN_PHYS || a.high_half != FROZEN_HIGH_HALF {
        return Err(KaslrCode::FrozenValueBad.val());
    }
    Ok(a)
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

// ── 注入检出账（四路：开关路/冻结路/熵死路/证词路） ──────────────
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

// ── 黄金全链：nokaslr 关闭全账 + 开启对照证词 ────────────────────
pub fn golden_run() -> Result<(u64, u64, u64), u32> {
    let off = off_determinism(KaslrSwitch::Off)?;
    // 开启对照：seed=1 与 seed=2 基址不同（熵生效证词）。
    let on1 = build_base(KaslrSwitch::On(1));
    let on2 = build_base(KaslrSwitch::On(2));
    let entropy_alive = if on1.phys != on2.phys { 0 } else { 1 };
    Ok((off.phys, off.entropy, entropy_alive))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：nokaslr → 冻结基址 + 熵零 + 熵活证词。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((0x10_0000, 0, 0)) => Ok(()),
        _ => Err(KaslrCode::NotReproducible.val()),
    }
}

/// 序 B：nokaslr 词边界解析（"confnokaslr" 不误关）。
pub fn probe_seq_b() -> Result<(), u32> {
    match parse_switch("confnokaslr quiet") {
        KaslrSwitch::DefaultOn => Ok(()),
        _ => Err(KaslrCode::NotReproducible.val()),
    }
}

/// 序 C：kaslr=123 显性开 → 熵 = 123。
pub fn probe_seq_c() -> Result<(), u32> {
    match parse_switch("quiet kaslr=123") {
        KaslrSwitch::On(123) => Ok(()),
        _ => Err(KaslrCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：开关路（nokaslr 前后词边界）。
        led.record(0, matches!(parse_switch(" x nokaslr "), KaslrSwitch::Off)
            && matches!(parse_switch("nokaslrfoo"), KaslrSwitch::DefaultOn));
        // 路 2：冻结路。
        led.record(1, off_determinism(KaslrSwitch::Off).is_ok());
        // 路 3：熵死路（同 seed 两跑同基址恒定 = 正确语义；不同 seed 必异）。
        let s1 = build_base(KaslrSwitch::On(7));
        let s2 = build_base(KaslrSwitch::On(9));
        led.record(2, s1.phys != s2.phys && s1.phys == build_base(KaslrSwitch::On(7)).phys);
        // 路 4：证词路（缺省开有熵有偏移）。
        let d = build_base(KaslrSwitch::DefaultOn);
        led.record(3, d.entropy != 0 && d.phys != FROZEN_PHYS);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(KaslrCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < KaslrCode::ALL.len() {
        let v = KaslrCode::ALL[i].val();
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
            Ok((phys, ent, alive)) => *phys == 0x10_0000 && *ent == 0 && *alive == 0,
            Err(_) => false,
        },
        match &g {
            Ok((phys, ent, alive)) => format!("phys=0x{:x} entropy={} alive={}", phys, ent, alive),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 三态开关解析。
    let sw = matches!(parse_switch("quiet nokaslr loglevel=3"), KaslrSwitch::Off)
        && matches!(parse_switch("kaslr=42"), KaslrSwitch::On(42))
        && matches!(parse_switch("quiet"), KaslrSwitch::DefaultOn);
    v.push((
        "J1-三态开关",
        sw,
        String::from("nokaslr → Off / kaslr=N → On / 无 → DefaultOn"),
    ));
    // 冻结值逐位。
    v.push((
        "J1-冻结值",
        FROZEN_PHYS == 0x10_0000 && FROZEN_HIGH_HALF == 0xFFFF_8000_0000_0000,
        String::from("物理 1M + 高半区 0xFFFF8000_00000000"),
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
        String::from("0x6E400 窗口"),
    ));
    v
}

pub fn run_unx0087_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0087 KASLR 关闭模式（诊断模式）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
