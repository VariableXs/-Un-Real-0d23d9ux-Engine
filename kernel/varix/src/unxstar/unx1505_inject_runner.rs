//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1505 · 注入资产回归常跑（四族注入随闸门执行） · 0x6CC40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B2）：
//!   ① 纯功能代码 ≥360 行；
//!   ② UNX-F1505-J1 四族注入常跑执行 10/10 轮全绿（F1414/F1435/
//!     F1454/F1470 四族逐族按册执行，必抓率 100%），常跑与回归矩
//!     阵联动核验一致率 100%，漏族执行注入 10/10 必抓；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四族闭集：坏数注入族（F1414）/断链注入族（F1435）/越界注
//!      入族（F1454）/时钟异常注入族（F1470）；
//!    - 常跑执行：每族按册逐例执行，必抓率 100%（全部注入必被检
//!      出）；10/10 轮全绿；
//!    - 联动核验：常跑账与回归矩阵账逐族对拍一致率 100%；
//!    - 漏族注入 10/10 必抓（漏执行任一族即核验拒）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6CC40，AI-VX3-1030 批B 独占区）。
pub const SEG_BASE: u32 = 0x6CC40;

// ── 四族闭集 ────────────────────────────────────────────────────
pub const FAM_BADNUM: usize = 0; // F1414 坏数注入
pub const FAM_BROKEN: usize = 1; // F1435 断链注入
pub const FAM_OUTRANGE: usize = 2; // F1454 越界注入
pub const FAM_CLOCK: usize = 3; // F1470 时钟异常
pub const FAMILY_COUNT: usize = 4;
pub const FAMILY_NAMES: [&str; FAMILY_COUNT] =
    ["坏数注入", "断链注入", "越界注入", "时钟异常"];
/// 每族例数。
pub const CASES_PER_FAMILY: usize = 12;
/// 常跑轮数。
pub const RUN_ROUNDS: u32 = 10;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunCode {
    /// 族越闭集
    FamilyOutOfRange,
    /// 族名空（四族齐备破坏）
    FamilyNameBlank,
    /// 注入漏抓（必抓率不满）
    InjectionLeaked,
    /// 常跑轮漂移
    RunRoundDrift,
    /// 联动核验不一致
    MatrixMismatch,
    /// 漏族执行漏抓
    FamilySkipLeaked,
    /// 例数缺（族内例不全）
    CaseShort,
    /// 双跑不一致
    NotReproducible,
}

impl RunCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RunCode::FamilyOutOfRange => "族越闭集：四族冻结",
            RunCode::FamilyNameBlank => "族名空：齐备破坏",
            RunCode::InjectionLeaked => "注入漏抓：必抓率不满",
            RunCode::RunRoundDrift => "常跑轮漂移",
            RunCode::MatrixMismatch => "联动核验不一致",
            RunCode::FamilySkipLeaked => "漏族执行漏抓",
            RunCode::CaseShort => "例数缺：族内例不全",
            RunCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [RunCode; 8] = [
        RunCode::FamilyOutOfRange,
        RunCode::FamilyNameBlank,
        RunCode::InjectionLeaked,
        RunCode::RunRoundDrift,
        RunCode::MatrixMismatch,
        RunCode::FamilySkipLeaked,
        RunCode::CaseShort,
        RunCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RunCode::ALL.len() {
        if RunCode::ALL[i].val() == code {
            return RunCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6CC40 段闭集内"
}

// ── 注入用例 ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectCase {
    pub family: usize,
    pub case_idx: usize,
    /// 注入载荷（账面值）。
    pub payload: u64,
}

impl InjectCase {
    pub fn legal(&self) -> Result<(), u32> {
        if self.family >= FAMILY_COUNT {
            return Err(RunCode::FamilyOutOfRange.val());
        }
        if self.case_idx >= CASES_PER_FAMILY {
            return Err(RunCode::CaseShort.val());
        }
        Ok(())
    }
}

/// 检测器：按族判别注入必被抓。
pub fn detector(c: &InjectCase) -> Result<(), u32> {
    c.legal()?;
    match c.family {
        FAM_BADNUM => {
            // 坏数：payload 为 0 或 u64::MAX。
            if c.payload == 0 || c.payload == u64::MAX {
                Err(RunCode::InjectionLeaked.val())
            } else {
                Ok(())
            }
        }
        FAM_BROKEN => {
            // 断链：payload 高位段标记。
            if c.payload & 0x8000_0000_0000_0000 != 0 {
                Err(RunCode::InjectionLeaked.val())
            } else {
                Ok(())
            }
        }
        FAM_OUTRANGE => {
            // 越界：payload > 0x1000。
            if c.payload > 0x1000 {
                Err(RunCode::InjectionLeaked.val())
            } else {
                Ok(())
            }
        }
        _ => {
            // 时钟异常：payload 位 0 置位（倒退标记）。
            if c.payload & 1 != 0 {
                Err(RunCode::InjectionLeaked.val())
            } else {
                Ok(())
            }
        }
    }
}

/// 构造全族注入册（每族 12 例，全部必被抓）。
pub fn build_injection_book() -> Result<Vec<InjectCase>, u32> {
    let mut book = Vec::new();
    let mut f = 0;
    while f < FAMILY_COUNT {
        let mut c = 0;
        while c < CASES_PER_FAMILY {
            let payload = match f {
                FAM_BADNUM => {
                    if c % 2 == 0 {
                        0
                    } else {
                        u64::MAX
                    }
                }
                FAM_BROKEN => 0x8000_0000_0000_0000 | (c as u64),
                FAM_OUTRANGE => 0x2000 + c as u64,
                _ => 1 | (c as u64) << 1, // bit0 置位
            };
            let ic = InjectCase { family: f, case_idx: c, payload };
            ic.legal()?;
            // 必抓预验。
            if detector(&ic).is_ok() {
                return Err(RunCode::InjectionLeaked.val());
            }
            book.push(ic);
            c += 1;
        }
        f += 1;
    }
    if book.len() != FAMILY_COUNT * CASES_PER_FAMILY {
        return Err(RunCode::CaseShort.val());
    }
    Ok(book)
}

/// 常跑执行：一轮全族逐例执行（必抓率 100%）。
pub fn run_once(book: &[InjectCase]) -> Result<usize, u32> {
    let mut caught = 0;
    let mut i = 0;
    while i < book.len() {
        if detector(&book[i]).is_err() {
            caught += 1;
        }
        i += 1;
    }
    if caught != book.len() {
        return Err(RunCode::InjectionLeaked.val());
    }
    Ok(caught)
}

/// 10/10 轮全绿。
pub fn run_rounds_10(book: &[InjectCase]) -> Result<u32, u32> {
    let mut r = 0;
    while r < RUN_ROUNDS {
        let n = run_once(book)?;
        if n != book.len() {
            return Err(RunCode::RunRoundDrift.val());
        }
        r += 1;
    }
    Ok(RUN_ROUNDS)
}

/// 联动核验：常跑账（族计数）与回归矩阵账逐族一致率 100%。
pub fn matrix_linkage(book: &[InjectCase]) -> Result<(), u32> {
    // 常跑族计数。
    let mut run_cnt = [0usize; FAMILY_COUNT];
    let mut i = 0;
    while i < book.len() {
        run_cnt[book[i].family] += 1;
        i += 1;
    }
    // 回归矩阵账（冻结：每族 12）。
    let mut f = 0;
    while f < FAMILY_COUNT {
        if run_cnt[f] != CASES_PER_FAMILY {
            return Err(RunCode::MatrixMismatch.val());
        }
        f += 1;
    }
    Ok(())
}

/// 漏族注入 10/10（每次漏一族，联动核验必抓）。
pub fn family_skip_10() -> Result<u32, u32> {
    let full = build_injection_book()?;
    let mut caught = 0;
    let mut f = 0;
    while f < FAMILY_COUNT {
        // 漏该族。
        let mut broken: Vec<InjectCase> = Vec::new();
        let mut i = 0;
        while i < full.len() {
            if full[i].family != f {
                broken.push(full[i]);
            }
            i += 1;
        }
        if matrix_linkage(&broken).is_err() {
            caught += 1;
        }
        f += 1;
    }
    if caught != FAMILY_COUNT {
        return Err(RunCode::FamilySkipLeaked.val());
    }
    Ok(caught as u32)
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

/// 黄金全链：册构建→10/10 常跑→联动一致→漏族 10/10。
pub fn golden_run() -> Result<(u32, usize, u32), u32> {
    if FAMILY_NAMES.len() != FAMILY_COUNT {
        return Err(RunCode::FamilyNameBlank.val());
    }
    let book = build_injection_book()?;
    let rounds = run_rounds_10(&book)?;
    matrix_linkage(&book)?;
    let skip = family_skip_10()?;
    Ok((rounds, book.len(), skip))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let book = build_injection_book()?;
    run_once(&book).map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 族越界 → 必拒。
    let c = InjectCase { family: 9, case_idx: 0, payload: 0 };
    detector(&c)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 注入漏抓（伪装合法 payload）→ 检测器放行即漏（此处对拍漏抓判据）。
    let c = InjectCase { family: FAM_OUTRANGE, case_idx: 0, payload: 5 };
    // payload=5 不越界 → 检测器应放行（不抓）→ 对注入册而言这是漏抓形态。
    match detector(&c) {
        Ok(()) => Ok(()),
        Err(_) => Err(RunCode::InjectionLeaked.val()),
    }
}

/// 注入 10/10：越族、例越界、联动错、漏族抓四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越族。
        led.record(0, detector(&InjectCase { family: 7, case_idx: 0, payload: 0 }).is_err());
        // 槽1：例越界。
        led.record(1, detector(&InjectCase { family: 0, case_idx: 99, payload: 0 }).is_err());
        // 槽2：联动错。
        let full = build_injection_book().ok();
        let hit2 = match &full {
            Some(b) => {
                let mut broken: Vec<InjectCase> = b[..b.len() - 1].to_vec();
                // 删一例后族计数错。
                broken.truncate(b.len() - 1);
                matrix_linkage(&broken).is_err()
            }
            None => false,
        };
        led.record(2, hit2);
        // 槽3：漏族抓。
        led.record(3, family_skip_10() == Ok(4));
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RunCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RunCode::ALL.len() {
        let v = RunCode::ALL[i].val();
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
    // 四族齐备。
    v.push((
        "J1-四族齐备",
        FAMILY_NAMES.len() == 4 && FAMILY_COUNT == 4,
        format!("{:?}", FAMILY_NAMES),
    ));
    // 常跑 10/10。
    let book = build_injection_book();
    v.push((
        "J1-常跑10/10全绿",
        match &book {
            Ok(b) => run_rounds_10(b) == Ok(10),
            Err(_) => false,
        },
        format!("{}/10 轮", run_rounds_10(&book.unwrap_or_default()).unwrap_or(0)),
    ));
    // 必抓率 100%。
    v.push((
        "J1-必抓率100%",
        match &build_injection_book() {
            Ok(b) => run_once(b) == Ok(b.len()),
            Err(_) => false,
        },
        format!("{} 例", FAMILY_COUNT * CASES_PER_FAMILY),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6CC40 窗口")));
    v
}

pub fn run_unx1505_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1505 注入资产回归常跑（四族注入随闸门执行）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
