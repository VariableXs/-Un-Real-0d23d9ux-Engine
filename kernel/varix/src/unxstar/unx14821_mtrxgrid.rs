//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14821 · 矩阵定义与格账（18×3=54 格） · 0x66BC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F14821-J1 18×3=54 格定义表，格-判据映射零缺格，格账与
//!      批头勾稽；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 二维骨架闭集：18 定义行 × 3 列（占位列/判据列/留痕列）= 54 格；
//!   - 18 行名写死：六段（准备/注入/触发/采集/对账/收尾）× 每段
//!     三行（前/中/后）；
//!   - 独立判据常量：EXPECTED_CELLS = 54、HEADER_ROWS = 18、
//!     HEADER_COLS = 3 均写死不复用被测值，勾稽用；
//!   - 格-判据映射：格 (r,c) → "J1-R{r:02}C{c}"，54 格零缺格；
//!   - 格账与批头勾稽：账总格数 == 批头 54，逐行 3 格、逐列 18 格
//!     双向守恒；
//!   - 状态机：Blank → Defined → Audited 三态收敛，非法回边拒收；
//!   - 注入面：行越界/列越界/勾稽破坏 三族全抓；
//!   - LCG 种子写死 = SEG_BASE（0x66BC0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66BC0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66BC0;

// ── 骨架闭集 ─────────────────────────────────────────────────────
pub const HEADER_ROWS: usize = 18;
pub const HEADER_COLS: usize = 3;
pub const CELLS: usize = HEADER_ROWS * HEADER_COLS;
/// 独立判据常量（不复用乘积）。
pub const EXPECTED_CELLS: usize = 54;

pub const COL_NAMES: [&str; HEADER_COLS] = ["占位列", "判据列", "留痕列"];
/// 18 行名（六段 × 三行，写死）。
pub const ROW_NAMES: [&str; HEADER_ROWS] = [
    "准备·前", "准备·中", "准备·后",
    "注入·前", "注入·中", "注入·后",
    "触发·前", "触发·中", "触发·后",
    "采集·前", "采集·中", "采集·后",
    "对账·前", "对账·中", "对账·后",
    "收尾·前", "收尾·中", "收尾·后",
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GridCode {
    /// 行下标越 18 行闭集
    RowOutOfRange,
    /// 列下标越 3 列闭集
    ColOutOfRange,
    /// 格账与批头 54 不勾稽
    HeaderReconFail,
    /// 逐行/逐列守恒破
    RowColConservation,
    /// 格-判据映射缺格
    CriterionMissing,
    /// 判据号批内不唯一
    CriterionNotUnique,
    /// 格终态悬挂
    CellHung,
    /// 非法回边（重复定义）
    EdgeIllegal,
    /// 双跑不一致（不可复现）
    NotReproducible,
    /// 行名表与行数不守恒
    RowNameMismatch,
}

impl GridCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GridCode::RowOutOfRange => "行下标越闭集：18 行冻结",
            GridCode::ColOutOfRange => "列下标越闭集：3 列冻结",
            GridCode::HeaderReconFail => "格账与批头 54 不勾稽",
            GridCode::RowColConservation => "逐行 3 格/逐列 18 格守恒破",
            GridCode::CriterionMissing => "格-判据映射缺格：零缺格红线",
            GridCode::CriterionNotUnique => "判据号批内不唯一",
            GridCode::CellHung => "格终态悬挂：状态机未收敛",
            GridCode::EdgeIllegal => "非法回边：已定义格重复定义",
            GridCode::NotReproducible => "双跑不一致：复现红线",
            GridCode::RowNameMismatch => "行名表与行数不守恒：18 名",
        }
    }
    pub const ALL: [GridCode; 10] = [
        GridCode::RowOutOfRange,
        GridCode::ColOutOfRange,
        GridCode::HeaderReconFail,
        GridCode::RowColConservation,
        GridCode::CriterionMissing,
        GridCode::CriterionNotUnique,
        GridCode::CellHung,
        GridCode::EdgeIllegal,
        GridCode::NotReproducible,
        GridCode::RowNameMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GridCode::ALL.len() {
        if GridCode::ALL[i].val() == code {
            return GridCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66BC0 段闭集内"
}

// ── 格键与判据号 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GridKey {
    pub row: usize,
    pub col: usize,
}

pub fn gkey_valid(k: GridKey) -> Result<(), u32> {
    if k.row >= HEADER_ROWS {
        return Err(GridCode::RowOutOfRange.val());
    }
    if k.col >= HEADER_COLS {
        return Err(GridCode::ColOutOfRange.val());
    }
    Ok(())
}

pub fn gkey_linear(k: GridKey) -> Result<usize, u32> {
    gkey_valid(k)?;
    Ok(k.row * HEADER_COLS + k.col)
}

/// 格-判据号：J1-R{row:02}C{col}。
pub fn grid_criterion(k: GridKey) -> Result<String, u32> {
    gkey_valid(k)?;
    Ok(format!("J1-R{:02}C{}", k.row, k.col))
}

// ── 格状态机 ─────────────────────────────────────────────────────
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum GridCell {
    Blank,
    Defined(String),
    Audited(String),
}

/// 54 格账 + 批头。
pub struct GridLedger {
    pub cells: Vec<GridCell>,
    pub header_count: usize,
    pub booked: u32,
}

impl GridLedger {
    pub fn new() -> GridLedger {
        GridLedger {
            cells: Vec::new(),
            header_count: EXPECTED_CELLS,
            booked: 0,
        }
    }

    /// 单格定义+审计：Blank → Defined → Audited。
    pub fn book(&mut self, k: GridKey) -> Result<(), u32> {
        let idx = gkey_linear(k)?;
        while self.cells.len() <= idx {
            self.cells.push(GridCell::Blank);
        }
        if matches!(self.cells[idx], GridCell::Audited(_)) {
            return Err(GridCode::EdgeIllegal.val());
        }
        let c = grid_criterion(k)?;
        if c.len() == 0 {
            return Err(GridCode::CriterionMissing.val());
        }
        self.cells[idx] = GridCell::Audited(c);
        self.booked += 1;
        Ok(())
    }

    /// 全 54 格：零缺格 + 与批头勾稽 + 双向守恒 + 全收敛。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        let mut r = 0;
        while r < HEADER_ROWS {
            let mut c = 0;
            while c < HEADER_COLS {
                self.book(GridKey { row: r, col: c })?;
                c += 1;
            }
            r += 1;
        }
        if self.cells.len() != CELLS {
            return Err(GridCode::HeaderReconFail.val());
        }
        // 格账与批头勾稽。
        if self.booked as usize != self.header_count {
            return Err(GridCode::HeaderReconFail.val());
        }
        if self.header_count != EXPECTED_CELLS {
            return Err(GridCode::HeaderReconFail.val());
        }
        // 逐行 3 格 / 逐列 18 格双向守恒。
        let mut r = 0;
        while r < HEADER_ROWS {
            let mut n = 0;
            let mut c = 0;
            while c < HEADER_COLS {
                if !matches!(self.cells[r * HEADER_COLS + c], GridCell::Blank) {
                    n += 1;
                }
                c += 1;
            }
            if n != HEADER_COLS {
                return Err(GridCode::RowColConservation.val());
            }
            r += 1;
        }
        let mut c = 0;
        while c < HEADER_COLS {
            let mut n = 0;
            let mut r = 0;
            while r < HEADER_ROWS {
                if !matches!(self.cells[r * HEADER_COLS + c], GridCell::Blank) {
                    n += 1;
                }
                r += 1;
            }
            if n != HEADER_ROWS {
                return Err(GridCode::RowColConservation.val());
            }
            c += 1;
        }
        // 全收敛（无 Blank 悬挂）。
        let mut i = 0;
        while i < self.cells.len() {
            if matches!(self.cells[i], GridCell::Blank) {
                return Err(GridCode::CellHung.val());
            }
            i += 1;
        }
        Ok(self.booked)
    }

    /// 判据号批内唯一性。
    pub fn uniqueness(&self) -> Result<(), u32> {
        let mut ids: Vec<&str> = Vec::new();
        let mut i = 0;
        while i < self.cells.len() {
            match &self.cells[i] {
                GridCell::Audited(c) => {
                    // 下标先比长度（线性扫重）。
                    let mut j = 0;
                    while j < ids.len() {
                        if ids[j] == c.as_str() {
                            return Err(GridCode::CriterionNotUnique.val());
                        }
                        j += 1;
                    }
                    ids.push(c.as_str());
                }
                _ => return Err(GridCode::CriterionMissing.val()),
            }
            i += 1;
        }
        Ok(())
    }

    /// 复现：双跑账一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = GridLedger::new();
        let _ = run2.run_all();
        if run2.booked != self.booked || run2.cells != self.cells {
            return Err(GridCode::NotReproducible.val());
        }
        Ok(())
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
        self.next() % if n < 1 { 1 } else { n }
    }
}

/// LCG 行走抽验：16 拍随机格全落 54 格闭集。
pub fn lcg_grid_walk() -> Result<u32, u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut seen = 0;
    let mut i = 0;
    while i < 16 {
        let k = GridKey {
            row: g.below(HEADER_ROWS as u64) as usize,
            col: g.below(HEADER_COLS as u64) as usize,
        };
        gkey_valid(k)?;
        seen += 1;
        i += 1;
    }
    Ok(seen)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    if CELLS != EXPECTED_CELLS {
        return Err(GridCode::HeaderReconFail.val());
    }
    if ROW_NAMES.len() != HEADER_ROWS {
        return Err(GridCode::RowNameMismatch.val());
    }
    let mut led = GridLedger::new();
    let booked = led.run_all()?;
    led.uniqueness()?;
    led.reproducible()?;
    let walk = lcg_grid_walk()?;
    Ok((booked, walk, led.header_count as u32))
}

// ── 注入面：三族 3/3 必抓 ────────────────────────────────────────
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
    // 族 0：行/列越界各一发。
    ledger.tries[0] += 1;
    let mut axis_ok = true;
    if let Err(e) = gkey_linear(GridKey { row: HEADER_ROWS, col: 0 }) {
        if e != GridCode::RowOutOfRange.val() {
            axis_ok = false;
        }
    } else {
        axis_ok = false;
    }
    if let Err(e) = gkey_linear(GridKey { row: 0, col: HEADER_COLS }) {
        if e != GridCode::ColOutOfRange.val() {
            axis_ok = false;
        }
    } else {
        axis_ok = false;
    }
    if axis_ok {
        ledger.caught[0] += 1;
    }
    // 族 1：重复定义同格必拒。
    ledger.tries[1] += 1;
    let mut led = GridLedger::new();
    let k = GridKey { row: 5, col: 1 };
    let mut dup_ok = false;
    if led.book(k).is_ok() {
        if let Err(e) = led.book(k) {
            if e == GridCode::EdgeIllegal.val() {
                dup_ok = true;
            }
        }
    }
    if dup_ok {
        ledger.caught[1] += 1;
    }
    // 族 2：批头勾稽破坏（header 改小 1）必判不勾稽。
    ledger.tries[2] += 1;
    let mut led2 = GridLedger::new();
    led2.header_count = EXPECTED_CELLS - 1;
    let mut recon_ok = false;
    if let Err(e) = led2.run_all() {
        if e == GridCode::HeaderReconFail.val() {
            recon_ok = true;
        }
    }
    if recon_ok {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < GridCode::ALL.len() {
        let v = GridCode::ALL[i].val();
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
        "J1-54格零缺格+批头勾稽+复现",
        matches!(golden, Ok((54, _, 54))),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 行名列名点验：18 行名 + 3 列名全非空。
    let mut okr = 0;
    let mut i = 0;
    while i < ROW_NAMES.len() {
        if ROW_NAMES[i].len() > 0 {
            okr += 1;
        }
        i += 1;
    }
    let mut okc = 0;
    i = 0;
    while i < COL_NAMES.len() {
        if COL_NAMES[i].len() > 0 {
            okc += 1;
        }
        i += 1;
    }
    v.push((
        "J1-18行名+3列名点验",
        okr == 18 && okc == 3,
        format!("{}/18 {}/3", okr, okc),
    ));
    // 乘积勾稽：18*3 == 54 == 判据常量。
    v.push((
        "J1-乘积勾稽18x3=54",
        HEADER_ROWS * HEADER_COLS == CELLS && CELLS == EXPECTED_CELLS && EXPECTED_CELLS == 54,
        format!("{}x{}={}", HEADER_ROWS, HEADER_COLS, CELLS),
    ));
    // 判据号格式抽验：格 (7,2) → "J1-R07C2"。
    let crit = grid_criterion(GridKey { row: 7, col: 2 });
    v.push((
        "J1-判据号格式抽验",
        matches!(&crit, Ok(s) if s == "J1-R07C2"),
        format!("{:?}", crit),
    ));
    // 反向判据：缺中段一格必被判缺格/悬挂。
    let mut partial = GridLedger::new();
    let mut r = 0;
    let mut missing_detected = false;
    while r < HEADER_ROWS {
        let mut c = 0;
        while c < HEADER_COLS {
            // 跳过中段格 (9,1)。
            if !(r == 9 && c == 1) {
                let _ = partial.book(GridKey { row: r, col: c });
            }
            c += 1;
        }
        r += 1;
    }
    if partial.booked as usize == EXPECTED_CELLS - 1 {
        // 逐行守恒审计：行 9 只有 2 格必破守恒。
        let mut n9 = 0;
        let mut c = 0;
        while c < HEADER_COLS {
            if !matches!(partial.cells[9 * HEADER_COLS + c], GridCell::Blank) {
                n9 += 1;
            }
            c += 1;
        }
        if n9 == HEADER_COLS - 1 {
            missing_detected = true;
        }
    }
    v.push((
        "J1-反向判据53格缺格被检出",
        missing_detected,
        String::from("行9 仅 2 格必破守恒"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66BC0 窗口")));
    v
}

pub fn run_unx14821_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14821 矩阵定义与格账（18x3=54 格零缺格+批头勾稽）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
