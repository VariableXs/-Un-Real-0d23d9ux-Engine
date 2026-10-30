//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0586 · 参数值域核（域断言） · 0x6C900
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2）：
//!   ① 纯功能代码 ≥200 行；
//!   ② UNX-F0586-J1 值域核账齐（整数 8 键域表+字符串 8 键长表），
//!     越域拒收率 100%（注入 4/4）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 整数 8 键域表：键（冻结名）→ [min,max] 闭区间，越域拒；
//!    - 字符串 8 键长表：键（冻结名）→ [min_len,max_len]，越长拒；
//!    - 越域拒收率 100%：4 路注入（整数越上/下界、串越上/下长）
//!      全拒；
//!    - 值域账三列（键/域/拒收数），域表 16 键齐备；
//!    - 未知键拒（注册制：账外键不得核）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C900，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C900;

// ── 整数 8 键域表（冻结） ───────────────────────────────────────
pub const INT_KEYS: [&str; 8] = [
    "timeout_us", "max_threads", "queue_depth", "cache_lines",
    "tick_hz", "dma_burst", "retry_max", "buffer_pages",
];
pub const INT_MIN: [i64; 8] = [1, 1, 1, 1, 10, 1, 0, 1];
pub const INT_MAX: [i64; 8] = [60_000_000, 4096, 1024, 8192, 100_000, 256, 16, 4096];

// ── 字符串 8 键长表（冻结） ─────────────────────────────────────
pub const STR_KEYS: [&str; 8] = [
    "hostname", "logfile", "mount_tag", "boot_id",
    "driver_name", "bus_id", "locale", "log_level_name",
];
pub const STR_MIN: [usize; 8] = [1, 1, 1, 8, 1, 1, 2, 4];
pub const STR_MAX: [usize; 8] = [64, 128, 32, 16, 32, 32, 8, 8];

/// 注入路数（冻结：4）。
pub const INJECT_PATHS: usize = 4;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DomCode {
    /// 键未知（注册制拒）
    KeyUnknown,
    /// 整数越上界
    IntOverMax,
    /// 整数越下界
    IntUnderMin,
    /// 串越上长
    StrOverLen,
    /// 串越下长
    StrUnderLen,
    /// 域表倒置（min>max）
    DomainInverted,
    /// 域表缺键（16 键不全）
    DomainTableShort,
    /// 越域拒收漏
    RejectLeak,
    /// 双跑不一致
    NotReproducible,
}

impl DomCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DomCode::KeyUnknown => "键未知：注册制拒",
            DomCode::IntOverMax => "整数越上界",
            DomCode::IntUnderMin => "整数越下界",
            DomCode::StrOverLen => "串越上长",
            DomCode::StrUnderLen => "串越下长",
            DomCode::DomainInverted => "域表倒置：min>max",
            DomCode::DomainTableShort => "域表缺键：16 键不全",
            DomCode::RejectLeak => "越域拒收漏",
            DomCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [DomCode; 9] = [
        DomCode::KeyUnknown,
        DomCode::IntOverMax,
        DomCode::IntUnderMin,
        DomCode::StrOverLen,
        DomCode::StrUnderLen,
        DomCode::DomainInverted,
        DomCode::DomainTableShort,
        DomCode::RejectLeak,
        DomCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DomCode::ALL.len() {
        if DomCode::ALL[i].val() == code {
            return DomCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C900 段闭集内"
}

// ── 值域核 ─────────────────────────────────────────────────────
/// 整数键下标。
pub fn int_key_idx(key: &str) -> Option<usize> {
    let mut i = 0;
    while i < INT_KEYS.len() {
        if INT_KEYS[i] == key {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 串键下标。
pub fn str_key_idx(key: &str) -> Option<usize> {
    let mut i = 0;
    while i < STR_KEYS.len() {
        if STR_KEYS[i] == key {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 整数值域核：越域拒。
pub fn check_int(key: &str, v: i64) -> Result<(), u32> {
    let idx = match int_key_idx(key) {
        Some(i) => i,
        None => return Err(DomCode::KeyUnknown.val()),
    };
    if INT_MIN[idx] > INT_MAX[idx] {
        return Err(DomCode::DomainInverted.val());
    }
    if v > INT_MAX[idx] {
        return Err(DomCode::IntOverMax.val());
    }
    if v < INT_MIN[idx] {
        return Err(DomCode::IntUnderMin.val());
    }
    Ok(())
}

/// 串长域核：越长拒。
pub fn check_str(key: &str, s: &str) -> Result<(), u32> {
    let idx = match str_key_idx(key) {
        Some(i) => i,
        None => return Err(DomCode::KeyUnknown.val()),
    };
    if STR_MIN[idx] > STR_MAX[idx] {
        return Err(DomCode::DomainInverted.val());
    }
    let n = s.len();
    if n > STR_MAX[idx] {
        return Err(DomCode::StrOverLen.val());
    }
    if n < STR_MIN[idx] {
        return Err(DomCode::StrUnderLen.val());
    }
    Ok(())
}

/// 域表齐备审计：16 键全在 + 无倒置。
pub fn domain_table_audit() -> Result<usize, u32> {
    if INT_KEYS.len() != 8 || STR_KEYS.len() != 8 {
        return Err(DomCode::DomainTableShort.val());
    }
    let mut i = 0;
    while i < 8 {
        if INT_MIN[i] > INT_MAX[i] || STR_MIN[i] > STR_MAX[i] {
            return Err(DomCode::DomainInverted.val());
        }
        if INT_KEYS[i].len() == 0 || STR_KEYS[i].len() == 0 {
            return Err(DomCode::DomainTableShort.val());
        }
        i += 1;
    }
    Ok(16)
}

/// 4 路注入全拒（越域拒收率 100%）。
pub fn inject4_all_rejected() -> Result<[bool; INJECT_PATHS], u32> {
    let r0 = check_int("timeout_us", 60_000_001).is_err(); // 越上界
    let r1 = check_int("tick_hz", 9).is_err(); // 越下界
    let r2 = check_str("hostname", "").is_err(); // 越下长
    let r3 = check_str("logfile", &"x".repeat(129)).is_err(); // 越上长
    let all = [r0, r1, r2, r3];
    let mut i = 0;
    while i < INJECT_PATHS {
        if !all[i] {
            return Err(DomCode::RejectLeak.val());
        }
        i += 1;
    }
    Ok(all)
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

/// 黄金全链：域表审计→16 键合法值全过→4 路注入全拒。
pub fn golden_run() -> Result<(usize, [bool; 4], u32), u32> {
    let keys = domain_table_audit()?;
    // 16 键合法值全过。
    let mut i = 0;
    while i < 8 {
        check_int(INT_KEYS[i], INT_MIN[i])?;
        check_int(INT_KEYS[i], INT_MAX[i])?;
        check_str(STR_KEYS[i], &"a".repeat(STR_MIN[i]))?;
        check_str(STR_KEYS[i], &"b".repeat(STR_MAX[i]))?;
        i += 1;
    }
    // 4 路注入。
    let inj = inject4_all_rejected()?;
    Ok((keys, inj, INT_KEYS.len() as u32 + STR_KEYS.len() as u32))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    check_int("max_threads", 64)?;
    check_str("hostname", "varix-node")?;
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 未知键 → 必拒。
    check_int("bogus_key", 1)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 串越上长 → 必拒。
    check_str("locale", "abcdefghi")
}

/// 注入 10/10：越上界、越下界、未知键、串越上长四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        led.record(0, check_int("buffer_pages", 4097).is_err());
        led.record(1, check_int("retry_max", -1).is_err());
        led.record(2, check_str("no_such", "x").is_err());
        led.record(3, check_str("boot_id", "short").is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DomCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DomCode::ALL.len() {
        let v = DomCode::ALL[i].val();
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
    // 16 键域表齐。
    v.push((
        "J1-16键域表齐",
        domain_table_audit() == Ok(16) && INT_KEYS.len() == 8 && STR_KEYS.len() == 8,
        format!("整 {} + 串 {}", INT_KEYS.len(), STR_KEYS.len()),
    ));
    // 4/4 注入全拒。
    let inj = inject4_all_rejected();
    v.push((
        "J1-越域拒收4/4",
        match &inj {
            Ok(a) => a.iter().all(|x| *x),
            Err(_) => false,
        },
        format!("{:?}", inj),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C900 窗口")));
    v
}

pub fn run_unx0586_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0586 参数值域核（域断言）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
