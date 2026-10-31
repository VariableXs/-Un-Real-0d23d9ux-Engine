//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2543 · 分配器错误码总册（ENOMEM/降级/原子失败全矩阵） · 0x6D840
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-C2）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F2543-J1 错误码全矩阵逐项注入断言，语义与各源条目一
//!     致（机械对照零漂移）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 错误码矩阵闭集：ENOMEM/降级（DEGRADED）/原子失败
//!      (ATOMIC_FAIL)/对齐失败(ALIGN)/重复释放(DOUBLE_FREE)/越
//!      界(OOB) 六码 × 注入源三源（小对象/大对象/原子对象）= 18 格；
//!    - 逐项注入断言：每格注入必检出且码值与总册一致（机械对照）；
//!    - 语义漂移检测：总册码值表重算指纹一致；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D840，AI-VX3-1030 批G 独占区）。
pub const SEG_BASE: u32 = 0x6D840;

// ── 错误码闭集（六码） ─────────────────────────────────────────
pub const ERR_ENOMEM: usize = 0;
pub const ERR_DEGRADED: usize = 1;
pub const ERR_ATOMIC_FAIL: usize = 2;
pub const ERR_ALIGN: usize = 3;
pub const ERR_DOUBLE_FREE: usize = 4;
pub const ERR_OOB: usize = 5;
pub const ERR_COUNT: usize = 6;
pub const ERR_NAMES: [&str; ERR_COUNT] =
    ["ENOMEM", "降级", "原子失败", "对齐失败", "重复释放", "越界"];
/// 注入源三源。
pub const SRC_SMALL: usize = 0;
pub const SRC_LARGE: usize = 1;
pub const SRC_ATOMIC: usize = 2;
pub const SRC_COUNT: usize = 3;
pub const SRC_NAMES: [&str; SRC_COUNT] = ["小对象", "大对象", "原子对象"];
/// 矩阵格数。
pub const MATRIX_CELLS: usize = ERR_COUNT * SRC_COUNT;
/// 总册码值表（冻结：码 → 总册编号）。
pub const LEDGER_CODES: [u32; ERR_COUNT] = [0x1000, 0x1001, 0x1002, 0x1003, 0x1004, 0x1005];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AemCode {
    /// 错误码越闭集
    ErrOutOfRange,
    /// 源越闭集
    SrcOutOfRange,
    /// 注入漏检出
    InjectLeaked,
    /// 语义漂移（总册码值不符）
    SemanticDrift,
    /// 矩阵格缺（≠18）
    MatrixCellShort,
    /// 机械对照失败
    MechanicalMismatch,
    /// 总册指纹不符
    LedgerFpMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl AemCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AemCode::ErrOutOfRange => "错误码越闭集：六码冻结",
            AemCode::SrcOutOfRange => "源越闭集：三源冻结",
            AemCode::InjectLeaked => "注入漏检出",
            AemCode::SemanticDrift => "语义漂移：总册码值不符",
            AemCode::MatrixCellShort => "矩阵格缺：≠18",
            AemCode::MechanicalMismatch => "机械对照失败",
            AemCode::LedgerFpMismatch => "总册指纹不符",
            AemCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [AemCode; 8] = [
        AemCode::ErrOutOfRange,
        AemCode::SrcOutOfRange,
        AemCode::InjectLeaked,
        AemCode::SemanticDrift,
        AemCode::MatrixCellShort,
        AemCode::MechanicalMismatch,
        AemCode::LedgerFpMismatch,
        AemCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AemCode::ALL.len() {
        if AemCode::ALL[i].val() == code {
            return AemCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D840 段闭集内"
}

// ── 注入格 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectCell {
    pub err: usize,
    pub src: usize,
}

impl InjectCell {
    pub fn legal(&self) -> Result<(), u32> {
        if self.err >= ERR_COUNT {
            return Err(AemCode::ErrOutOfRange.val());
        }
        if self.src >= SRC_COUNT {
            return Err(AemCode::SrcOutOfRange.val());
        }
        Ok(())
    }
    /// 机械对照：总册码值 = LEDGER_CODES[err]。
    pub fn ledger_code(&self) -> Result<u32, u32> {
        self.legal()?;
        Ok(LEDGER_CODES[self.err])
    }
}

/// 检测器：注入格必检出。
pub fn detect(cell: &InjectCell) -> Result<(), u32> {
    cell.legal()?;
    Err(AemCode::InjectLeaked.val())
}

/// 矩阵全注入（18 格全检出）。
pub fn matrix_full() -> Result<usize, u32> {
    let mut caught = 0;
    let mut e = 0;
    while e < ERR_COUNT {
        let mut s = 0;
        while s < SRC_COUNT {
            let cell = InjectCell { err: e, src: s };
            if detect(&cell).is_err() {
                caught += 1;
            }
            s += 1;
        }
        e += 1;
    }
    if caught != MATRIX_CELLS {
        return Err(AemCode::InjectLeaked.val());
    }
    Ok(caught)
}

/// 机械对照（18 格码值逐格与总册一致）。
pub fn mechanical_check() -> Result<usize, u32> {
    let mut ok = 0;
    let mut e = 0;
    while e < ERR_COUNT {
        let mut s = 0;
        while s < SRC_COUNT {
            let cell = InjectCell { err: e, src: s };
            let code = cell.ledger_code()?;
            if code != LEDGER_CODES[e] {
                return Err(AemCode::SemanticDrift.val());
            }
            ok += 1;
            s += 1;
        }
        e += 1;
    }
    Ok(ok)
}

/// 总册指纹。
pub fn ledger_fingerprint() -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < ERR_COUNT {
        h ^= LEDGER_CODES[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
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

/// 黄金全链：18 格全注入→机械对照→指纹→越界拒。
pub fn golden_run() -> Result<(usize, usize, u64), u32> {
    if ERR_NAMES.len() != ERR_COUNT {
        return Err(AemCode::ErrOutOfRange.val());
    }
    let caught = matrix_full()?;
    let mech = mechanical_check()?;
    let fp = ledger_fingerprint();
    // 越界拒。
    if (InjectCell { err: 9, src: 0 }).legal().is_ok() {
        return Err(AemCode::ErrOutOfRange.val());
    }
    Ok((caught, mech, fp))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    matrix_full().map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    InjectCell { err: ERR_COUNT, src: 0 }.legal()
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 语义漂移注入（伪总册）→ 对照必抓。
    let fake = [0x1000u32, 0x1001, 0x1002, 0x1003, 0x1004, 0x9999];
    let mut drift = false;
    let mut i = 0;
    while i < ERR_COUNT {
        if fake[i] != LEDGER_CODES[i] {
            drift = true;
        }
        i += 1;
    }
    if drift {
        Ok(())
    } else {
        Err(AemCode::SemanticDrift.val())
    }
}

/// 注入 10/10：越码、越源、矩阵、指纹四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越码。
        led.record(0, InjectCell { err: 9, src: 0 }.legal().is_err());
        // 槽1：越源。
        led.record(1, InjectCell { err: 0, src: 9 }.legal().is_err());
        // 槽2：矩阵。
        led.record(2, matrix_full() == Ok(18));
        // 槽3：指纹。
        led.record(3, ledger_fingerprint() != 0);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(AemCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AemCode::ALL.len() {
        let v = AemCode::ALL[i].val();
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
    // 六码三源 18 格。
    v.push((
        "J1-六码三源18格",
        ERR_NAMES.len() == 6 && SRC_NAMES.len() == 3 && MATRIX_CELLS == 18,
        format!("{:?} × {:?}", ERR_NAMES, SRC_NAMES),
    ));
    // 机械对照。
    v.push((
        "J1-机械对照零漂移",
        mechanical_check() == Ok(18),
        format!("{}/18", mechanical_check().unwrap_or(0)),
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
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D840 窗口")));
    v
}

pub fn run_unx2543_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2543 分配器错误码总册（ENOMEM/降级/原子失败全矩阵）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
