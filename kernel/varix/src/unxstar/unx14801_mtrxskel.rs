//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14801 · 矩阵骨架与维度定义 · 0x66B80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F14801-J1 3 线程 × 4 发送式 × 3 目标态 × 2 目标代际 =
//!      72 格矩阵定义表，格-判据映射零缺格；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四维骨架闭集：线程 3（主线程/工作线程/哨兵线程）× 发送式 4
//!     （直接发送/队列投递/广播/回放）× 目标态 3（待激活/激活中/
//!     已驻留）× 目标代际 2（代际 G0/G1）= 72 格；
//!   - 独立判据常量 EXPECTED_CELLS = 72 写死（不复用四维乘积），
//!     与乘积勾稽；
//!   - 格-判据映射：每格映射判据号 J1-T{t}M{m}S{s}G{g}，72 格映射
//!     零缺格 + 全批唯一性点验（O(n²) 逐对比对）；
//!   - 状态机：Unmapped → Mapped → Audited 三态收敛，非法回边拒收；
//!   - 注入面：四轴各一越界注入 + 唯一性破坏检出 = 全抓；
//!   - LCG 种子写死 = SEG_BASE（0x66B80），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66B80，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66B80;

// ── 四维骨架闭集 ─────────────────────────────────────────────────
pub const THREADS: usize = 3;
pub const SEND_MODES: usize = 4;
pub const TARGET_STATES: usize = 3;
pub const GENERATIONS: usize = 2;
pub const CELLS: usize = THREADS * SEND_MODES * TARGET_STATES * GENERATIONS;
/// 独立判据常量（不复用乘积，勾稽用）。
pub const EXPECTED_CELLS: usize = 72;

pub const THREAD_NAMES: [&str; THREADS] = ["主线程", "工作线程", "哨兵线程"];
pub const SEND_MODE_NAMES: [&str; SEND_MODES] = ["直接发送", "队列投递", "广播", "回放"];
pub const TARGET_STATE_NAMES: [&str; TARGET_STATES] = ["待激活", "激活中", "已驻留"];
pub const GENERATION_NAMES: [&str; GENERATIONS] = ["代际G0", "代际G1"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MtrxCode {
    /// 线程轴越闭集
    ThreadOutOfRange,
    /// 发送式轴越闭集
    ModeOutOfRange,
    /// 目标态轴越闭集
    StateOutOfRange,
    /// 代际轴越闭集
    GenOutOfRange,
    /// 格账与独立判据 72 不守恒
    CellCountMismatch,
    /// 格-判据映射缺格
    CriterionMissing,
    /// 判据号批内不唯一
    CriterionNotUnique,
    /// 格终态悬挂
    CellHung,
    /// 骨架乘积与判据常量不勾稽
    SkeletonReconFail,
    /// 双跑不一致（不可复现）
    NotReproducible,
}

impl MtrxCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MtrxCode::ThreadOutOfRange => "线程轴越闭集：3 线程冻结",
            MtrxCode::ModeOutOfRange => "发送式轴越闭集：4 发送式冻结",
            MtrxCode::StateOutOfRange => "目标态轴越闭集：3 态冻结",
            MtrxCode::GenOutOfRange => "代际轴越闭集：2 代际冻结",
            MtrxCode::CellCountMismatch => "格账与独立判据 72 不守恒",
            MtrxCode::CriterionMissing => "格-判据映射缺格：零缺格红线",
            MtrxCode::CriterionNotUnique => "判据号批内不唯一",
            MtrxCode::CellHung => "格终态悬挂：状态机未收敛",
            MtrxCode::SkeletonReconFail => "骨架乘积与判据常量 72 不勾稽",
            MtrxCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [MtrxCode; 10] = [
        MtrxCode::ThreadOutOfRange,
        MtrxCode::ModeOutOfRange,
        MtrxCode::StateOutOfRange,
        MtrxCode::GenOutOfRange,
        MtrxCode::CellCountMismatch,
        MtrxCode::CriterionMissing,
        MtrxCode::CriterionNotUnique,
        MtrxCode::CellHung,
        MtrxCode::SkeletonReconFail,
        MtrxCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MtrxCode::ALL.len() {
        if MtrxCode::ALL[i].val() == code {
            return MtrxCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66B80 段闭集内"
}

// ── 格键与线性化 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellKey {
    pub thread: usize,
    pub mode: usize,
    pub state: usize,
    pub gen: usize,
}

/// 四轴合法性全检：任一轴越界即判对应码。
pub fn key_valid(k: CellKey) -> Result<(), u32> {
    if k.thread >= THREADS {
        return Err(MtrxCode::ThreadOutOfRange.val());
    }
    if k.mode >= SEND_MODES {
        return Err(MtrxCode::ModeOutOfRange.val());
    }
    if k.state >= TARGET_STATES {
        return Err(MtrxCode::StateOutOfRange.val());
    }
    if k.gen >= GENERATIONS {
        return Err(MtrxCode::GenOutOfRange.val());
    }
    Ok(())
}

/// 行主序线性化（合法性前置）。
pub fn key_linear(k: CellKey) -> Result<usize, u32> {
    key_valid(k)?;
    Ok((((k.thread * SEND_MODES) + k.mode) * TARGET_STATES + k.state) * GENERATIONS + k.gen)
}

/// 线性化逆变换（骨架可逆性）。
pub fn linear_key(idx: usize) -> Result<CellKey, u32> {
    if idx >= CELLS {
        return Err(MtrxCode::CellCountMismatch.val());
    }
    let gen = idx % GENERATIONS;
    let rest = idx / GENERATIONS;
    let state = rest % TARGET_STATES;
    let rest2 = rest / TARGET_STATES;
    let mode = rest2 % SEND_MODES;
    let thread = rest2 / SEND_MODES;
    Ok(CellKey { thread, mode, state, gen })
}

/// 格-判据号映射（判据原文骨架号制：J1-T{t}M{m}S{s}G{g}）。
pub fn criterion_of(k: CellKey) -> Result<String, u32> {
    key_valid(k)?;
    Ok(format!(
        "J1-T{}M{}S{}G{}",
        k.thread, k.mode, k.state, k.gen
    ))
}

// ── 格状态机 ─────────────────────────────────────────────────────
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CellState {
    Unmapped,
    Mapped(String),
    Audited(String),
}

/// 72 格账。
pub struct SkeletonLedger {
    pub cells: Vec<CellState>,
    pub booked: u32,
}

impl SkeletonLedger {
    pub fn new() -> SkeletonLedger {
        SkeletonLedger { cells: Vec::new(), booked: 0 }
    }

    /// 单格映射+审计：Unmapped → Mapped → Audited。
    pub fn book_cell(&mut self, k: CellKey) -> Result<(), u32> {
        let idx = key_linear(k)?;
        while self.cells.len() <= idx {
            self.cells.push(CellState::Unmapped);
        }
        // 非法回边拒收：已 Audited 的格不得重映射覆盖。
        if matches!(self.cells[idx], CellState::Audited(_)) {
            return Err(MtrxCode::CriterionNotUnique.val());
        }
        let c = criterion_of(k)?;
        if c.len() == 0 {
            return Err(MtrxCode::CriterionMissing.val());
        }
        self.cells[idx] = CellState::Audited(c);
        self.booked += 1;
        Ok(())
    }

    /// 全 72 格：零缺格 + 全收敛 + 守恒。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        let mut t = 0;
        while t < THREADS {
            let mut m = 0;
            while m < SEND_MODES {
                let mut s = 0;
                while s < TARGET_STATES {
                    let mut g = 0;
                    while g < GENERATIONS {
                        self.book_cell(CellKey { thread: t, mode: m, state: s, gen: g })?;
                        g += 1;
                    }
                    s += 1;
                }
                m += 1;
            }
            t += 1;
        }
        if self.cells.len() != CELLS {
            return Err(MtrxCode::CellCountMismatch.val());
        }
        if self.booked as usize != EXPECTED_CELLS {
            return Err(MtrxCode::CellCountMismatch.val());
        }
        // 零缺格：逐格非 Unmapped。
        let mut i = 0;
        while i < self.cells.len() {
            if matches!(self.cells[i], CellState::Unmapped) {
                return Err(MtrxCode::CriterionMissing.val());
            }
            i += 1;
        }
        Ok(self.booked)
    }

    /// 判据号批内唯一性：O(n²) 逐对。
    pub fn uniqueness(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.cells.len() {
            let ci = match &self.cells[i] {
                CellState::Audited(c) => c.clone(),
                _ => return Err(MtrxCode::CriterionMissing.val()),
            };
            let mut j = i + 1;
            while j < self.cells.len() {
                if let CellState::Audited(cj) = &self.cells[j] {
                    if *cj == ci {
                        return Err(MtrxCode::CriterionNotUnique.val());
                    }
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 骨架可逆性：linear(key) 与 linear_key(idx) 互逆全查。
    pub fn bijection_audit(&self) -> Result<(), u32> {
        let mut idx = 0;
        while idx < CELLS {
            let k = linear_key(idx)?;
            if key_linear(k)? != idx {
                return Err(MtrxCode::SkeletonReconFail.val());
            }
            idx += 1;
        }
        Ok(())
    }

    /// 复现：双跑账一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = SkeletonLedger::new();
        let _ = run2.run_all();
        if run2.booked != self.booked || run2.cells != self.cells {
            return Err(MtrxCode::NotReproducible.val());
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

/// LCG 抽验：种子固定 = SEG_BASE，采样格坐标落闭集内且逐拍递进。
pub fn lcg_cell_walk() -> Result<u32, u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut distinct = 0;
    let mut prev_key: Option<CellKey> = None;
    let mut i = 0;
    while i < 16 {
        let k = CellKey {
            thread: g.below(THREADS as u64) as usize,
            mode: g.below(SEND_MODES as u64) as usize,
            state: g.below(TARGET_STATES as u64) as usize,
            gen: g.below(GENERATIONS as u64) as usize,
        };
        key_valid(k)?;
        if let Some(p) = prev_key {
            if p != k {
                distinct += 1;
            }
        }
        prev_key = Some(k);
        i += 1;
    }
    Ok(distinct)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    if CELLS != EXPECTED_CELLS {
        return Err(MtrxCode::SkeletonReconFail.val());
    }
    let mut led = SkeletonLedger::new();
    let booked = led.run_all()?;
    led.uniqueness()?;
    led.bijection_audit()?;
    led.reproducible()?;
    let walk = lcg_cell_walk()?;
    Ok((booked, walk, CELLS as u32))
}

// ── 注入面：三族 4/4 必抓（四轴越界 + 重复映射） ─────────────────
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
    // 族 0：四轴越界各一发，全须判对应轴码。
    ledger.tries[0] += 1;
    let mut four_ok = true;
    let bad_keys = [
        CellKey { thread: THREADS, mode: 0, state: 0, gen: 0 },
        CellKey { thread: 0, mode: SEND_MODES, state: 0, gen: 0 },
        CellKey { thread: 0, mode: 0, state: TARGET_STATES, gen: 0 },
        CellKey { thread: 0, mode: 0, state: 0, gen: GENERATIONS },
    ];
    let want_codes = [
        MtrxCode::ThreadOutOfRange.val(),
        MtrxCode::ModeOutOfRange.val(),
        MtrxCode::StateOutOfRange.val(),
        MtrxCode::GenOutOfRange.val(),
    ];
    let mut i = 0;
    while i < 4 {
        let mut hit = false;
        if let Err(e) = key_linear(bad_keys[i]) {
            if e == want_codes[i] {
                hit = true;
            }
        }
        if !hit {
            four_ok = false;
        }
        i += 1;
    }
    if four_ok {
        ledger.caught[0] += 1;
    }
    // 族 1：重复映射同格必拒（Audited 回边拒收）。
    ledger.tries[1] += 1;
    let mut led = SkeletonLedger::new();
    let k = CellKey { thread: 1, mode: 1, state: 1, gen: 1 };
    let mut dup_ok = false;
    if led.book_cell(k).is_ok() {
        if let Err(e) = led.book_cell(k) {
            if e == MtrxCode::CriterionNotUnique.val() {
                dup_ok = true;
            }
        }
    }
    if dup_ok {
        ledger.caught[1] += 1;
    }
    // 族 2：越界线性号逆变换必拒。
    ledger.tries[2] += 1;
    if let Err(e) = linear_key(CELLS) {
        if e == MtrxCode::CellCountMismatch.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MtrxCode::ALL.len() {
        let v = MtrxCode::ALL[i].val();
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
        "J1-72格零缺格+唯一+双射+复现",
        matches!(golden, Ok((72, _, 72))),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓(含四轴)",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 四维名点验：3+4+3+2 名全非空。
    let mut named = 0;
    let mut i = 0;
    while i < THREADS {
        if THREAD_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    i = 0;
    while i < SEND_MODES {
        if SEND_MODE_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    i = 0;
    while i < TARGET_STATES {
        if TARGET_STATE_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    i = 0;
    while i < GENERATIONS {
        if GENERATION_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-3x4x3x2轴名点验",
        named == 12,
        format!("{}/12 轴名", named),
    ));
    // 骨架乘积勾稽：3*4*3*2 == 72（独立判据常量）。
    v.push((
        "J1-骨架乘积勾稽判据常量",
        THREADS * SEND_MODES * TARGET_STATES * GENERATIONS == EXPECTED_CELLS
            && EXPECTED_CELLS == 72,
        format!("{}x{}x{}x{}={}", THREADS, SEND_MODES, TARGET_STATES, GENERATIONS, CELLS),
    ));
    // 判据号格式抽验：格 (1,2,1,0) → "J1-T1M2S1G0"。
    let crit = criterion_of(CellKey { thread: 1, mode: 2, state: 1, gen: 0 });
    v.push((
        "J1-判据号格式抽验",
        matches!(&crit, Ok(s) if s == "J1-T1M2S1G0"),
        format!("{:?}", crit),
    ));
    // 反向判据：缺格构造（只映 71 格）必被判缺。
    let mut partial = SkeletonLedger::new();
    let mut t = 0;
    let mut missing_detected = false;
    while t < THREADS {
        let mut m = 0;
        while m < SEND_MODES {
            let mut s = 0;
            while s < TARGET_STATES {
                let mut g = 0;
                while g < GENERATIONS {
                    // 跳过中段格 (1,1,1,0) 一个不映（尾格跳过会短账）。
                    if !(t == 1 && m == 1 && s == 1 && g == 0) {
                        let _ = partial.book_cell(CellKey { thread: t, mode: m, state: s, gen: g });
                    }
                    g += 1;
                }
                s += 1;
            }
            m += 1;
        }
        t += 1;
    }
    if partial.booked as usize == EXPECTED_CELLS - 1 {
        if let Err(e) = partial.run_all_booked_audit() {
            if e == MtrxCode::CriterionMissing.val() {
                missing_detected = true;
            }
        }
    }
    v.push((
        "J1-反向判据71格缺格被检出",
        missing_detected,
        String::from("缺 1 格必判 CriterionMissing"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66B80 窗口")));
    v
}

// 反向判据辅助：对已入账账本做零缺格审计（不重映射，只查缺）。
impl SkeletonLedger {
    pub fn run_all_booked_audit(&self) -> Result<(), u32> {
        if self.cells.len() != CELLS {
            return Err(MtrxCode::CellCountMismatch.val());
        }
        let mut i = 0;
        while i < self.cells.len() {
            if matches!(self.cells[i], CellState::Unmapped) {
                return Err(MtrxCode::CriterionMissing.val());
            }
            i += 1;
        }
        Ok(())
    }
}

pub fn run_unx14801_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14801 矩阵骨架与维度定义（3x4x3x2=72 格零缺格）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
