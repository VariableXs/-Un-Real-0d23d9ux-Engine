//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12741 · SEH 压测核账总纲（压测矩阵登记） · 0x651C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F12741-J1 压测矩阵登记齐（深度/展开/码族/坏表四轴），
//!      矩阵可点验 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 压测矩阵四轴闭集：深度（展开层数档）/展开（逐步展开态）/
//!     码族（异常码分族）/坏表（故障表注入档）——四轴取值组合构成
//!     压测用例格，登记齐 = 格内每格有主且非空；
//!   - 核账三律：每格执行数登记 / 每格结果态登记 / 格账守恒
//!     （Σ格执行数 ≡ 总执行数）；
//!   - 压测只压模型不压真机：异常码族闭集建模展开/拒绝路径；
//!   - 注入三族：格重复登记/格账不守恒/轴越闭集 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x651C0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x651C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x651C0;

// ── 四轴闭集 ─────────────────────────────────────────────────────
/// 深度轴：3 档（浅=1层/中=4层/深=8层）。
pub const DEPTH_TIERS: [u32; 3] = [1, 4, 8];
/// 展开轴：3 态（不展开/逐步展开/全展开）。
pub const UNWIND_STATES: usize = 3;
/// 码族轴：4 族（访问违例/栈溢出/非法指令/除零）。
pub const CODE_FAMILIES: usize = 4;
/// 坏表轴：3 档（好表/半坏表/全坏表）。
pub const BADTABLE_TIERS: usize = 3;

/// 矩阵格数 = 3×3×4×3。
pub const CELL_COUNT: usize = 3 * 3 * 4 * 3;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StressCode {
    /// 深度轴越档
    DepthAxisInvalid,
    /// 展开轴越态
    UnwindAxisInvalid,
    /// 码族轴越族
    FamilyAxisInvalid,
    /// 坏表轴越档
    BadTableAxisInvalid,
    /// 格重复登记
    CellDoubleBooked,
    /// 格账不守恒（Σ格≠总）
    CellLedgerBreak,
    /// 结果态枚举外值
    ResultStateInvalid,
    /// 矩阵未登记齐即跑核账
    MatrixIncomplete,
}

impl StressCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            StressCode::DepthAxisInvalid => "深度轴越档：3 档闭集外",
            StressCode::UnwindAxisInvalid => "展开轴越态：3 态闭集外",
            StressCode::FamilyAxisInvalid => "码族轴越族：4 族闭集外",
            StressCode::BadTableAxisInvalid => "坏表轴越档：3 档闭集外",
            StressCode::CellDoubleBooked => "格重复登记：一格一主",
            StressCode::CellLedgerBreak => "格账不守恒：Σ格执行数≠总执行数",
            StressCode::ResultStateInvalid => "结果态枚举外值",
            StressCode::MatrixIncomplete => "矩阵未登记齐即核账：先登记后核账",
        }
    }
    pub const ALL: [StressCode; 8] = [
        StressCode::DepthAxisInvalid,
        StressCode::UnwindAxisInvalid,
        StressCode::FamilyAxisInvalid,
        StressCode::BadTableAxisInvalid,
        StressCode::CellDoubleBooked,
        StressCode::CellLedgerBreak,
        StressCode::ResultStateInvalid,
        StressCode::MatrixIncomplete,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < StressCode::ALL.len() {
        if StressCode::ALL[i].val() == code {
            return StressCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x651C0 段闭集内"
}

// ── 结果态三闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellResult {
    NotRun,
    Passed,
    Rejected,
}

/// 压测格：四轴坐标 + 执行账。
#[derive(Clone, Copy)]
pub struct StressCell {
    pub depth: usize,
    pub unwind: usize,
    pub family: usize,
    pub badtable: usize,
    pub runs: u32,
    pub result: CellResult,
}

// ── 压测矩阵 ─────────────────────────────────────────────────────
pub struct StressMatrix {
    pub cells: [Option<StressCell>; CELL_COUNT],
    pub booked: usize,
    pub total_runs: u32,
}

impl StressMatrix {
    pub fn new() -> StressMatrix {
        StressMatrix {
            cells: [None; CELL_COUNT],
            booked: 0,
            total_runs: 0,
        }
    }

    /// 格下标 = 线性化四轴（越轴即拒）。
    pub fn cell_index(depth: usize, unwind: usize, family: usize, badtable: usize) -> Result<usize, u32> {
        if depth >= DEPTH_TIERS.len() {
            return Err(StressCode::DepthAxisInvalid.val());
        }
        if unwind >= UNWIND_STATES {
            return Err(StressCode::UnwindAxisInvalid.val());
        }
        if family >= CODE_FAMILIES {
            return Err(StressCode::FamilyAxisInvalid.val());
        }
        if badtable >= BADTABLE_TIERS {
            return Err(StressCode::BadTableAxisInvalid.val());
        }
        Ok(depth * (UNWIND_STATES * CODE_FAMILIES * BADTABLE_TIERS)
            + unwind * (CODE_FAMILIES * BADTABLE_TIERS)
            + family * BADTABLE_TIERS
            + badtable)
    }

    /// 登记格（重复登记即拒）。
    pub fn book_cell(&mut self, depth: usize, unwind: usize, family: usize, badtable: usize) -> Result<usize, u32> {
        let idx = Self::cell_index(depth, unwind, family, badtable)?;
        if self.cells[idx].is_some() {
            return Err(StressCode::CellDoubleBooked.val());
        }
        self.cells[idx] = Some(StressCell {
            depth,
            unwind,
            family,
            badtable,
            runs: 0,
            result: CellResult::NotRun,
        });
        self.booked += 1;
        Ok(idx)
    }

    /// 执行一格：坏表轴决定结果态（好表=Passed，半坏/全坏=Rejected）。
    pub fn run_cell(&mut self, idx: usize) -> Result<CellResult, u32> {
        let cell = match self.cells[idx] {
            Some(ref mut c) => c,
            None => return Err(StressCode::MatrixIncomplete.val()),
        };
        let r = match cell.badtable {
            0 => CellResult::Passed,
            _ => CellResult::Rejected,
        };
        cell.runs += 1;
        cell.result = r;
        self.total_runs += 1;
        Ok(r)
    }

    /// 格账守恒：Σ格 runs ≡ total_runs。
    pub fn conservation(&self) -> Result<(), u32> {
        let mut sum = 0u32;
        let mut i = 0;
        while i < CELL_COUNT {
            if let Some(ref c) = self.cells[i] {
                sum += c.runs;
            }
            i += 1;
        }
        if sum != self.total_runs {
            return Err(StressCode::CellLedgerBreak.val());
        }
        Ok(())
    }

    /// 矩阵登记齐判定：booked == CELL_COUNT。
    pub fn complete(&self) -> bool {
        self.booked == CELL_COUNT
    }

    /// 矩阵可点验 10/10。
    pub fn run_asserts(&mut self) -> u32 {
        let mut pass = 0u32;
        // A1：登记齐。
        if self.complete() {
            pass += 1;
        }
        // A2：格账守恒。
        if self.conservation().is_ok() {
            pass += 1;
        }
        // A3~A5：三轴下标越界必抓（深度/展开/码族）。
        if Self::cell_index(3, 0, 0, 0).is_err() {
            pass += 1;
        }
        if Self::cell_index(0, 3, 0, 0).is_err() {
            pass += 1;
        }
        if Self::cell_index(0, 0, 4, 0).is_err() {
            pass += 1;
        }
        // A6：坏表轴越界必抓。
        if Self::cell_index(0, 0, 0, 3).is_err() {
            pass += 1;
        }
        // A7：格下标线性化双射（0~CELL_COUNT-1 全域可达不重）。
        let mut probe = StressMatrix::new();
        let mut all_booked = true;
        let mut d = 0;
        while d < 3 {
            let mut u = 0;
            while u < 3 {
                let mut f = 0;
                while f < 4 {
                    let mut b = 0;
                    while b < 3 {
                        if probe.book_cell(d, u, f, b).is_err() {
                            all_booked = false;
                        }
                        b += 1;
                    }
                    f += 1;
                }
                u += 1;
            }
            d += 1;
        }
        if all_booked && probe.booked == CELL_COUNT {
            pass += 1;
        }
        // A8：重复登记必抓（同格双登）。
        let mut probe2 = StressMatrix::new();
        let _ = probe2.book_cell(0, 0, 0, 0);
        if probe2.book_cell(0, 0, 0, 0).is_err() {
            pass += 1;
        }
        // A9：好表通过/坏表拒绝分档恰按轴。
        let mut probe3 = StressMatrix::new();
        let i0 = probe3.book_cell(0, 0, 0, 0).unwrap_or(0);
        let i1 = probe3.book_cell(0, 0, 0, 1).unwrap_or(0);
        let r0 = probe3.run_cell(i0);
        let r1 = probe3.run_cell(i1);
        if r0 == Ok(CellResult::Passed) && r1 == Ok(CellResult::Rejected) {
            pass += 1;
        }
        // A10：结果态反向——NotRun 格必无执行数。
        let mut probe4 = StressMatrix::new();
        let i4 = probe4.book_cell(1, 1, 1, 1).unwrap_or(0);
        if let Some(ref c) = probe4.cells[i4] {
            if c.result == CellResult::NotRun && c.runs == 0 {
                pass += 1;
            }
        }
        pass
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
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
        self.next() % n.max(1)
    }
}

// ── 黄金正演：登记全矩阵→确定性执行→守恒→点验 10/10 ────────────
pub fn golden_run() -> Result<u32, u32> {
    let mut m = StressMatrix::new();
    let mut d = 0;
    while d < 3 {
        let mut u = 0;
        while u < 3 {
            let mut f = 0;
            while f < 4 {
                let mut b = 0;
                while b < 3 {
                    m.book_cell(d, u, f, b)?;
                    b += 1;
                }
                f += 1;
            }
            u += 1;
        }
        d += 1;
    }
    // 确定性执行：LCG 抽 64 格次。
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < 64 {
        let idx = rng.below(CELL_COUNT as u64) as usize;
        m.run_cell(idx)?;
        i += 1;
    }
    m.conservation()?;
    Ok(m.run_asserts())
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
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
    // 族 0：轴越闭集。
    ledger.tries[0] += 1;
    if StressMatrix::cell_index(0, 0, 9, 0).is_err() {
        ledger.caught[0] += 1;
    }
    // 族 1：格重复登记。
    ledger.tries[1] += 1;
    let mut m = StressMatrix::new();
    let _ = m.book_cell(0, 0, 0, 0);
    if m.book_cell(0, 0, 0, 0).is_err() {
        ledger.caught[1] += 1;
    }
    // 族 2：格账不守恒（破坏 total_runs）。
    ledger.tries[2] += 1;
    let mut m2 = StressMatrix::new();
    let i2 = m2.book_cell(0, 0, 0, 0).unwrap_or(0);
    let _ = m2.run_cell(i2);
    m2.total_runs += 5;
    if m2.conservation() == Err(StressCode::CellLedgerBreak.val()) {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < StressCode::ALL.len() {
        let v = StressCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "J1-矩阵可点验10/10",
        golden == Ok(10),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    v.push((
        "J1-格数冻结",
        CELL_COUNT == 108,
        format!("{}=3×3×4×3", CELL_COUNT),
    ));
    v.push((
        "J1-深度档写死",
        DEPTH_TIERS == [1, 4, 8],
        format!("{:?}", DEPTH_TIERS),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x651C0 窗口")));
    v
}

pub fn run_unx12741_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12741 SEH 压测核账总纲（压测矩阵登记）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
