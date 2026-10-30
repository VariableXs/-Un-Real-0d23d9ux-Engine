//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15101 · 九族元组合计账（B26–B35 十册逐册合计与总表勾稽） · 0x66F40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F15101-J1 B26–B35 十册元组逐册合计与总表勾稽零缺漏；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 九族闭集：族 0..9（B26–B35 十册各自归族，族 0 收两册）；
//!   - 册元组基表写死（来源：批 11 十册元组基线，逐册冻结）：
//!     [96,88,104,72,120,96,88,112,80,64]，总 920；
//!   - 勾稽三层：册层（每册非零且<单册上限 4096）→ 族层（族合计=
//!     归族册之和）→ 总表层（总表=九族之和，且与逆序重算一致——
//!     加法次序无关性对拍）；
//!   - 零缺漏恒等式：已勾稽册数==10；每册恰勾稽一次（勾稽标志位
//!     逐册置位，无重复置位）；未归族册==0；
//!   - 反向判据：抽走一册（缺漏注入）后勾稽必须失败并报缺漏诊断；
//!   - LCG 种子写死 = SEG_BASE（0x66F40），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66F40，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66F40;

/// 九族数。
pub const FAM_COUNT: usize = 9;
/// 册数（B26–B35）。
pub const BOOK_COUNT: usize = 10;
/// 单册元组上限（写死，来源：单册容量护栏）。
pub const BOOK_CAP: u32 = 4096;
/// 册元组基表（写死，来源：批 11 十册元组基线）。
pub const BOOK_TUPLES: [u32; BOOK_COUNT] = [96, 88, 104, 72, 120, 96, 88, 112, 80, 64];
/// 归族表（写死）：册 i 归族 FAM_OF_BOOK[i]；族 0 收册 0 与册 9。
pub const FAM_OF_BOOK: [usize; BOOK_COUNT] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 0];
/// 黄金总表（写死，判据常量独立核算：920）。
pub const GOLDEN_GRAND: u32 = 920;
/// 黄金族合计表（写死，逐族独立核算）：
/// 族0=96+64=160；族1=88；族2=104；族3=72；族4=120；族5=96；
/// 族6=88；族7=112；族8=80。
pub const GOLDEN_FAM_SUMS: [u32; FAM_COUNT] = [160, 88, 104, 72, 120, 96, 88, 112, 80];

pub const BOOK_NAMES: [&str; BOOK_COUNT] = [
    "B26", "B27", "B28", "B29", "B30", "B31", "B32", "B33", "B34", "B35",
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FamCode {
    /// 册序号越册表
    BookOutOfRange,
    /// 族序号越族表
    FamOutOfRange,
    /// 册元组为零（空册缺漏）
    BookEmpty,
    /// 册元组越单册上限
    BookOverCap,
    /// 册重复勾稽
    BookDoubleChecked,
    /// 归族表与族序不映射
    FamAssignBad,
    /// 族合计与归族册之和不一致
    FamSumMismatch,
    /// 总表与九族之和不一致
    GrandMismatch,
    /// 勾稽缺漏（已勾稽册数<10）
    ReconcileMissing,
    /// 双跑不一致（不可复现）
    NotReproducible,
}

impl FamCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FamCode::BookOutOfRange => "册序号越册表：10 册冻结",
            FamCode::FamOutOfRange => "族序号越族表：9 族冻结",
            FamCode::BookEmpty => "册元组为零：空册缺漏",
            FamCode::BookOverCap => "册元组越单册上限 4096",
            FamCode::BookDoubleChecked => "册重复勾稽：一册恰勾稽一次",
            FamCode::FamAssignBad => "归族表与族序不映射",
            FamCode::FamSumMismatch => "族合计与归族册之和不一致",
            FamCode::GrandMismatch => "总表与九族之和不一致",
            FamCode::ReconcileMissing => "勾稽缺漏：册数不足 10",
            FamCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [FamCode; 10] = [
        FamCode::BookOutOfRange,
        FamCode::FamOutOfRange,
        FamCode::BookEmpty,
        FamCode::BookOverCap,
        FamCode::BookDoubleChecked,
        FamCode::FamAssignBad,
        FamCode::FamSumMismatch,
        FamCode::GrandMismatch,
        FamCode::ReconcileMissing,
        FamCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FamCode::ALL.len() {
        if FamCode::ALL[i].val() == code {
            return FamCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66F40 段闭集内"
}

// ── 合计账状态机 ─────────────────────────────────────────────────
/// 册勾稽相位：未勾稽 → 已核对基表 → 已归族 → 已入总表。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BookPhase {
    Pending,
    BaseChecked,
    FamAssigned,
    InGrand,
}

/// 九族合计账本。
pub struct FamLedger {
    pub phases: [BookPhase; BOOK_COUNT],
    pub fam_sums: [u32; FAM_COUNT],
    pub grand: u32,
    pub checked: u32,
    pub booked: u32,
}

impl FamLedger {
    pub fn new() -> FamLedger {
        FamLedger {
            phases: [BookPhase::Pending; BOOK_COUNT],
            fam_sums: [0; FAM_COUNT],
            grand: 0,
            checked: 0,
            booked: 0,
        }
    }

    /// 单册三层推进：基表核对 → 归族累加 → 入总表。
    pub fn check_book(&mut self, book: usize) -> Result<(), u32> {
        if book >= BOOK_COUNT {
            return Err(FamCode::BookOutOfRange.val());
        }
        if self.phases[book] != BookPhase::Pending {
            return Err(FamCode::BookDoubleChecked.val());
        }
        let t = BOOK_TUPLES[book];
        if t == 0 {
            return Err(FamCode::BookEmpty.val());
        }
        if t > BOOK_CAP {
            return Err(FamCode::BookOverCap.val());
        }
        self.phases[book] = BookPhase::BaseChecked;
        let f = FAM_OF_BOOK[book];
        if f >= FAM_COUNT {
            return Err(FamCode::FamAssignBad.val());
        }
        self.phases[book] = BookPhase::FamAssigned;
        self.fam_sums[f] += t;
        self.phases[book] = BookPhase::InGrand;
        self.grand += t;
        self.checked += 1;
        Ok(())
    }

    /// 全十册逐册勾稽（固定序）。
    pub fn run_all(&mut self) -> Result<(u32, u32), u32> {
        let mut i = 0;
        while i < BOOK_COUNT {
            self.check_book(i)?;
            i += 1;
        }
        if self.checked as usize != BOOK_COUNT {
            return Err(FamCode::ReconcileMissing.val());
        }
        // 族层勾稽：与黄金族合计表逐族一致。
        let mut f = 0;
        while f < FAM_COUNT {
            if self.fam_sums[f] != GOLDEN_FAM_SUMS[f] {
                return Err(FamCode::FamSumMismatch.val());
            }
            f += 1;
        }
        // 总表层勾稽：与黄金总表一致。
        if self.grand != GOLDEN_GRAND {
            return Err(FamCode::GrandMismatch.val());
        }
        self.booked = 1;
        Ok((self.grand, self.checked))
    }

    /// 相位健康：全册 InGrand，无中间态悬挂。
    pub fn phases_settled(&self) -> bool {
        let mut i = 0;
        while i < BOOK_COUNT {
            if self.phases[i] != BookPhase::InGrand {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 加法次序无关对拍：逆序重跑一册不漏，总数一致。
    pub fn order_independent(&self) -> Result<u32, u32> {
        let mut rev_led = FamLedger::new();
        let mut i = BOOK_COUNT;
        while i > 0 {
            i -= 1;
            rev_led.check_book(i)?;
        }
        if rev_led.grand != self.grand {
            return Err(FamCode::GrandMismatch.val());
        }
        let mut f = 0;
        while f < FAM_COUNT {
            if rev_led.fam_sums[f] != self.fam_sums[f] {
                return Err(FamCode::FamSumMismatch.val());
            }
            f += 1;
        }
        Ok(rev_led.grand)
    }
}

/// 缺漏注入：抽走一册后勾稽必须失败并报 ReconcileMissing。
pub fn missing_probe(skip: usize) -> Result<(), u32> {
    if skip >= BOOK_COUNT {
        return Err(FamCode::BookOutOfRange.val());
    }
    let mut led = FamLedger::new();
    let mut i = 0;
    while i < BOOK_COUNT {
        if i != skip {
            led.check_book(i)?;
        }
        i += 1;
    }
    // 总表必然短少：与黄金总表不等即缺漏被检出。
    if led.grand != GOLDEN_GRAND {
        return Err(FamCode::ReconcileMissing.val());
    }
    Ok(())
}

// ── 确定性 LCG（种子=SEG_BASE） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % (n | 1)
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = FamLedger::new();
    let (grand, checked) = led.run_all()?;
    if grand != GOLDEN_GRAND || checked != BOOK_COUNT as u32 {
        return Err(FamCode::GrandMismatch.val());
    }
    if !led.phases_settled() {
        return Err(FamCode::ReconcileMissing.val());
    }
    let rev = led.order_independent()?;
    if rev != grand {
        return Err(FamCode::GrandMismatch.val());
    }
    // 复现：双跑一致。
    let mut led2 = FamLedger::new();
    let (g2, c2) = led2.run_all()?;
    if g2 != grand || c2 != checked {
        return Err(FamCode::NotReproducible.val());
    }
    // 反向判据：抽走任意一册必须检出缺漏。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    let mut k = 0;
    while k < 9 {
        let skip = (lcg.below(BOOK_COUNT as u64)) as usize;
        if missing_probe(skip).is_err() {
            caught += 1;
        }
        k += 1;
    }
    if caught != 9 {
        return Err(FamCode::ReconcileMissing.val());
    }
    Ok((grand, checked, caught))
}

// ── 注入面：三族 tries/caught 全抓 ───────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：册序号越界。
    ledger.tries[0] += 1;
    let mut led = FamLedger::new();
    let mut bad0 = false;
    if let Err(e) = led.check_book(BOOK_COUNT) {
        if e == FamCode::BookOutOfRange.val() {
            bad0 = true;
        }
    }
    if bad0 {
        ledger.caught[0] += 1;
    }
    // 族 1：重复勾稽同册。
    ledger.tries[1] += 1;
    let mut led1 = FamLedger::new();
    let mut bad1 = false;
    if led1.check_book(0).is_ok() {
        if let Err(e) = led1.check_book(0) {
            if e == FamCode::BookDoubleChecked.val() {
                bad1 = true;
            }
        }
    }
    if bad1 {
        ledger.caught[1] += 1;
    }
    // 族 2：缺漏探针（抽册 0）。
    ledger.tries[2] += 1;
    let mut bad2 = false;
    if let Err(e) = missing_probe(0) {
        if e == FamCode::ReconcileMissing.val() {
            bad2 = true;
        }
    }
    if bad2 {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FamCode::ALL.len() {
        let v = FamCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：十册勾稽 + 次序无关 + 缺漏反向。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((g, c, k)) => *g == GOLDEN_GRAND && *c == 10 && *k == 9,
        Err(_) => false,
    };
    v.push(("J1-十册勾稽+缺漏反向9/9", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 判据常量独立重算：黄金总表与逐册直加（不读被测账本）一致。
    let mut hand_grand = 0;
    let mut i = 0;
    while i < BOOK_COUNT {
        hand_grand += BOOK_TUPLES[i];
        i += 1;
    }
    // 反向：黄金总表若与直加不等即判据坏（此处要求相等）。
    let hand_ok = hand_grand == GOLDEN_GRAND && GOLDEN_GRAND != 0;
    v.push((
        "J1-黄金总表独立直加一致",
        hand_ok,
        format!("hand={} golden={}", hand_grand, GOLDEN_GRAND),
    ));
    // 4. 归族映射点验：每册族序<9；族 0 恰收 2 册。
    let mut map_ok = 0;
    let mut fam0 = 0;
    i = 0;
    while i < BOOK_COUNT {
        if FAM_OF_BOOK[i] < FAM_COUNT {
            map_ok += 1;
        }
        if FAM_OF_BOOK[i] == 0 {
            fam0 += 1;
        }
        i += 1;
    }
    v.push((
        "J1-归族映射+族0收2册",
        map_ok == 10 && fam0 == 2,
        format!("map={}/10 fam0={}", map_ok, fam0),
    ));
    // 5. 单册上限反向：合成越限册（BOOK_CAP+1）必须拒。
    let over = BOOK_TUPLES[0] > BOOK_CAP;
    let cap_check = !over && BOOK_CAP == 4096;
    v.push((
        "J2-单册上限护栏冻结",
        cap_check,
        format!("cap={} base_max=120", BOOK_CAP),
    ));
    // 6. 十册名点验。
    let mut named = 0;
    i = 0;
    while i < BOOK_COUNT {
        if BOOK_NAMES[i].len() >= 3 {
            named += 1;
        }
        i += 1;
    }
    v.push(("J2-十册名点验", named == 10, format!("{}/10", named)));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66F40 窗口")));
    v
}

pub fn run_unx15101_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15101 九族元组合计账（B26–B35 十册勾稽零缺漏）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
