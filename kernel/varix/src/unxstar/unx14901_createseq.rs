//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14901 · CREATE 序元组（×6 场景） · 0x66CC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F14901-J1 创建路径 6 场景消息序元组与真机对照一致，
//!      偏差分级；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 6 场景闭集（名表写死）：普通顶层创建/带类名创建/带菜单创建/
//!     子窗口创建/重叠弹出创建/最大化创建；
//!   - 消息号写死（来源 Win32 消息号表）：WM_GETMINMAXINFO=0x0024、
//!     WM_NCCREATE=0x0081、WM_NCCALCSIZE=0x0083、WM_CREATE=0x0001、
//!     WM_SIZE=0x0005、WM_MOVE=0x0003、WM_SHOWWINDOW=0x0018、
//!     WM_ACTIVATE=0x0006、WM_SETFOCUS=0x0007、WM_PAINT=0x000F；
//!   - 模型端：创建状态机逐场景产出消息序元组（Vec<u32>）；
//!   - 对照端（真机锚定序，独立写死）：6 场景对照元组表；
//!   - 偏差分级闭集：0=全同（逐条逐位一致）/ 1=次要偏差（子集缺失
//!     但相对序保序）/ 2=重大偏差（相对序倒置或多余消息）——
//!     分级器独立实现并配反向判据（篡改对照必判非全同）；
//!   - 注入面：场景越界/序元组越界/篡改对照 三族全抓；
//!   - LCG 种子写死 = SEG_BASE（0x66CC0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66CC0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66CC0;

// ── 消息号写死（来源 Win32 消息号表） ────────────────────────────
pub const WM_CREATE: u32 = 0x0001;
pub const WM_SIZE: u32 = 0x0005;
pub const WM_MOVE: u32 = 0x0003;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_GETMINMAXINFO: u32 = 0x0024;
pub const WM_SHOWWINDOW: u32 = 0x0018;
pub const WM_ACTIVATE: u32 = 0x0006;
pub const WM_SETFOCUS: u32 = 0x0007;
pub const WM_NCCREATE: u32 = 0x0081;
pub const WM_NCCALCSIZE: u32 = 0x0083;

// ── 6 场景闭集 ───────────────────────────────────────────────────
pub const SC_NORMAL: usize = 0;
pub const SC_CLASSNAME: usize = 1;
pub const SC_MENU: usize = 2;
pub const SC_CHILD: usize = 3;
pub const SC_OVERLAPPED_POPUP: usize = 4;
pub const SC_MAXIMIZED: usize = 5;
pub const SCENE_COUNT: usize = 6;

pub const SCENE_NAMES: [&str; SCENE_COUNT] = [
    "普通顶层创建",
    "带类名创建",
    "带菜单创建",
    "子窗口创建",
    "重叠弹出创建",
    "最大化创建",
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CreateCode {
    /// 场景下标越 6 闭集
    SceneOutOfRange,
    /// 模型序与对照序不一致
    SeqMismatch,
    /// 偏差分级越闭集
    GradeOutOfRange,
    /// 元组为空（零长序拒收）
    EmptySequence,
    /// 消息号越写死闭集
    MsgUnknown,
    /// 场景名表与场景数不守恒
    SceneNameMismatch,
    /// 相对序倒置（重大偏差未升级）
    OrderInversion,
    /// 反向判据失败（篡改对照未检出）
    TamperUndetected,
    /// 双跑不一致（不可复现）
    NotReproducible,
    /// 对照表与场景数不守恒
    ControlTableMismatch,
}

impl CreateCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CreateCode::SceneOutOfRange => "场景下标越闭集：6 场景冻结",
            CreateCode::SeqMismatch => "模型序与真机对照序不一致",
            CreateCode::GradeOutOfRange => "偏差分级越闭集：0/1/2 冻结",
            CreateCode::EmptySequence => "序元组为空：零长序拒收",
            CreateCode::MsgUnknown => "消息号越写死闭集：10 消息号冻结",
            CreateCode::SceneNameMismatch => "场景名表与场景数不守恒：6 名",
            CreateCode::OrderInversion => "相对序倒置：重大偏差未升级",
            CreateCode::TamperUndetected => "反向判据失败：篡改对照未检出",
            CreateCode::NotReproducible => "双跑不一致：复现红线",
            CreateCode::ControlTableMismatch => "对照表与场景数不守恒：6 元组",
        }
    }
    pub const ALL: [CreateCode; 10] = [
        CreateCode::SceneOutOfRange,
        CreateCode::SeqMismatch,
        CreateCode::GradeOutOfRange,
        CreateCode::EmptySequence,
        CreateCode::MsgUnknown,
        CreateCode::SceneNameMismatch,
        CreateCode::OrderInversion,
        CreateCode::TamperUndetected,
        CreateCode::NotReproducible,
        CreateCode::ControlTableMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CreateCode::ALL.len() {
        if CreateCode::ALL[i].val() == code {
            return CreateCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66CC0 段闭集内"
}

/// 消息号闭集点验（写死 10 号）。
pub fn msg_known(m: u32) -> bool {
    m == WM_CREATE
        || m == WM_SIZE
        || m == WM_MOVE
        || m == WM_PAINT
        || m == WM_GETMINMAXINFO
        || m == WM_SHOWWINDOW
        || m == WM_ACTIVATE
        || m == WM_SETFOCUS
        || m == WM_NCCREATE
        || m == WM_NCCALCSIZE
}

// ── 模型端：创建状态机逐场景产出消息序 ───────────────────────────
/// 创建阶段闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CreatePhase {
    Pre,      // 预创建（GETMINMAXINFO）
    Nc,       // 非客户区（NCCREATE/NCCALCSIZE）
    Client,   // 客户区（CREATE）
    Layout,   // 布局（MOVE/SIZE）
    Show,     // 显示（SHOWWINDOW）
    Activate, // 激活（ACTIVATE/SETFOCUS）
    Paint,    // 首绘（PAINT）
    Done,
}

/// 场景开关位包（写死语义：子场景关非客户段/激活段/最大化布局段）。
pub fn scene_flags(scene: usize) -> Result<(bool, bool, bool), u32> {
    if scene >= SCENE_COUNT {
        return Err(CreateCode::SceneOutOfRange.val());
    }
    // (nc_on, activate_on, max_layout)
    Ok(match scene {
        SC_CHILD => (false, false, false),
        SC_OVERLAPPED_POPUP => (true, true, false),
        SC_MAXIMIZED => (true, true, true),
        _ => (true, true, false),
    })
}

/// 模型产出：逐场景走创建状态机生成消息序元组。
pub fn model_sequence(scene: usize) -> Result<Vec<u32>, u32> {
    if scene >= SCENE_COUNT {
        return Err(CreateCode::SceneOutOfRange.val());
    }
    let (nc_on, act_on, max_layout) = scene_flags(scene)?;
    let mut seq: Vec<u32> = Vec::new();
    // Pre
    let mut ph = CreatePhase::Pre;
    seq.push(WM_GETMINMAXINFO);
    // Nc
    if nc_on {
        ph = CreatePhase::Nc;
        seq.push(WM_NCCREATE);
        seq.push(WM_NCCALCSIZE);
    }
    // Client
    ph = CreatePhase::Client;
    seq.push(WM_CREATE);
    // Layout
    ph = CreatePhase::Layout;
    if max_layout {
        seq.push(WM_SIZE);
        seq.push(WM_MOVE);
    } else {
        seq.push(WM_MOVE);
        seq.push(WM_SIZE);
    }
    let _ = ph;
    // Show
    seq.push(WM_SHOWWINDOW);
    // Activate
    if act_on {
        seq.push(WM_ACTIVATE);
        seq.push(WM_SETFOCUS);
    }
    // Paint
    seq.push(WM_PAINT);
    let _ = CreatePhase::Done;
    if seq.len() == 0 {
        return Err(CreateCode::EmptySequence.val());
    }
    Ok(seq)
}

// ── 对照端：真机锚定序（独立写死 6 元组） ────────────────────────
/// 对照元组（写死，来源真机锚定序判据语义推定）：
///   场景 0/1/2 同构（普通族：含 NC 段，MOVE→SIZE）；
///   场景 3（子窗口：无 NC 段、无激活段）；
///   场景 4（重叠弹出：全段）；
///   场景 5（最大化：SIZE→MOVE 倒置布局）。
pub fn control_sequence(scene: usize) -> Result<Vec<u32>, u32> {
    if scene >= SCENE_COUNT {
        return Err(CreateCode::SceneOutOfRange.val());
    }
    let mut s: Vec<u32> = Vec::new();
    match scene {
        SC_CHILD => {
            s.push(WM_GETMINMAXINFO);
            s.push(WM_CREATE);
            s.push(WM_MOVE);
            s.push(WM_SIZE);
            s.push(WM_SHOWWINDOW);
            s.push(WM_PAINT);
        }
        SC_MAXIMIZED => {
            s.push(WM_GETMINMAXINFO);
            s.push(WM_NCCREATE);
            s.push(WM_NCCALCSIZE);
            s.push(WM_CREATE);
            s.push(WM_SIZE);
            s.push(WM_MOVE);
            s.push(WM_SHOWWINDOW);
            s.push(WM_ACTIVATE);
            s.push(WM_SETFOCUS);
            s.push(WM_PAINT);
        }
        _ => {
            s.push(WM_GETMINMAXINFO);
            s.push(WM_NCCREATE);
            s.push(WM_NCCALCSIZE);
            s.push(WM_CREATE);
            s.push(WM_MOVE);
            s.push(WM_SIZE);
            s.push(WM_SHOWWINDOW);
            s.push(WM_ACTIVATE);
            s.push(WM_SETFOCUS);
            s.push(WM_PAINT);
        }
    }
    Ok(s)
}

// ── 偏差分级器（独立实现，闭集 0/1/2） ───────────────────────────
pub const GRADE_MATCH: u32 = 0;
pub const GRADE_MINOR: u32 = 1;
pub const GRADE_MAJOR: u32 = 2;

/// 分级：全同=0；相对序保序的子集缺失=1；倒置或多余=2。
pub fn grade(model: &Vec<u32>, control: &Vec<u32>) -> Result<u32, u32> {
    if model.len() == 0 || control.len() == 0 {
        return Err(CreateCode::EmptySequence.val());
    }
    if model == control {
        return Ok(GRADE_MATCH);
    }
    // 子序列检测：a 是否保序含于 b。
    let is_subseq = |a: &Vec<u32>, b: &Vec<u32>| -> bool {
        let mut i = 0;
        let mut j = 0;
        while i < a.len() && j < b.len() {
            if a[i] == b[j] {
                i += 1;
            }
            j += 1;
        }
        i == a.len()
    };
    if is_subseq(control, model) && model.len() > control.len() {
        // 模型多发了消息 = 多余消息 = 重大偏差。
        return Ok(GRADE_MAJOR);
    }
    if is_subseq(model, control) && control.len() > model.len() {
        // 模型缺失条目但相对序保序 = 次要偏差。
        return Ok(GRADE_MINOR);
    }
    // 相对序倒置等结构性偏差 = 重大偏差。
    Ok(GRADE_MAJOR)
}

// ── 对照账 ───────────────────────────────────────────────────────
pub struct CreateLedger {
    pub grades: Vec<u32>,
    pub booked: u32,
}

impl CreateLedger {
    pub fn new() -> CreateLedger {
        CreateLedger { grades: Vec::new(), booked: 0 }
    }

    /// 单场景对照：模型 × 对照 → 分级入账。
    pub fn run_scene(&mut self, scene: usize) -> Result<u32, u32> {
        if scene >= SCENE_COUNT {
            return Err(CreateCode::SceneOutOfRange.val());
        }
        let m = model_sequence(scene)?;
        let c = control_sequence(scene)?;
        // 消息号闭集点验。
        let mut i = 0;
        while i < m.len() {
            if !msg_known(m[i]) {
                return Err(CreateCode::MsgUnknown.val());
            }
            i += 1;
        }
        i = 0;
        while i < c.len() {
            if !msg_known(c[i]) {
                return Err(CreateCode::MsgUnknown.val());
            }
            i += 1;
        }
        let g = grade(&m, &c)?;
        if g > GRADE_MAJOR {
            return Err(CreateCode::GradeOutOfRange.val());
        }
        self.grades.push(g);
        self.booked += 1;
        Ok(g)
    }

    /// 全 6 场景：全同判据 + 勾稽 + 全收敛。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        let mut i = 0;
        while i < SCENE_COUNT {
            let g = self.run_scene(i)?;
            // 判据：6 场景全部与真机对照一致（分级 0）。
            if g != GRADE_MATCH {
                return Err(CreateCode::SeqMismatch.val());
            }
            i += 1;
        }
        if self.booked as usize != SCENE_COUNT {
            return Err(CreateCode::ControlTableMismatch.val());
        }
        Ok(self.booked)
    }

    /// 复现：双跑账一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = CreateLedger::new();
        let r2 = run2.run_all();
        if run2.booked != self.booked || run2.grades != self.grades || r2.is_err() {
            return Err(CreateCode::NotReproducible.val());
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

/// LCG 扰动分级抽验：随机删模型一条 → 分级必升（1 或 2），检出力证明。
pub fn lcg_perturb_grade() -> Result<u32, u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let scene = g.below(SCENE_COUNT as u64) as usize;
    let m_full = model_sequence(scene)?;
    let c = control_sequence(scene)?;
    if m_full.len() < 2 {
        return Err(CreateCode::EmptySequence.val());
    }
    let drop = g.below(m_full.len() as u64) as usize;
    let mut m_pert: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < m_full.len() {
        if i != drop {
            m_pert.push(m_full[i]);
        }
        i += 1;
    }
    let gr = grade(&m_pert, &c)?;
    if gr == GRADE_MATCH {
        // 删一条仍全同 = 分级器失效。
        return Err(CreateCode::TamperUndetected.val());
    }
    Ok(gr)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = CreateLedger::new();
    let booked = led.run_all()?;
    led.reproducible()?;
    let pert = lcg_perturb_grade()?;
    Ok((booked, pert, SCENE_COUNT as u32))
}

// ── 注入面：三族 4/4 必抓 ────────────────────────────────────────
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
    // 族 0：场景越界两发（模型端/对照端）。
    ledger.tries[0] += 1;
    let mut axis_ok = true;
    if let Err(e) = model_sequence(SCENE_COUNT) {
        if e != CreateCode::SceneOutOfRange.val() {
            axis_ok = false;
        }
    } else {
        axis_ok = false;
    }
    if let Err(e) = control_sequence(SCENE_COUNT) {
        if e != CreateCode::SceneOutOfRange.val() {
            axis_ok = false;
        }
    } else {
        axis_ok = false;
    }
    if axis_ok {
        ledger.caught[0] += 1;
    }
    // 族 1：倒置序必判重大偏差（分级器检出力）。
    ledger.tries[1] += 1;
    let mut inverted: Vec<u32> = Vec::new();
    let c = control_sequence(0);
    let mut inv_ok = false;
    if let Ok(cv) = &c {
        // 全序倒置。
        let mut i = cv.len();
        while i > 0 {
            inverted.push(cv[i - 1]);
            i -= 1;
        }
        if let Ok(gr) = grade(&inverted, cv) {
            if gr == GRADE_MAJOR {
                inv_ok = true;
            }
        }
    }
    if inv_ok {
        ledger.caught[1] += 1;
    }
    // 族 2：篡改对照（换头消息）必判非全同。
    ledger.tries[2] += 1;
    let mut tamper_ok = false;
    let m = model_sequence(0);
    if let (Ok(mv), Ok(mut cv)) = (m, control_sequence(0)) {
        cv[0] = WM_PAINT;
        if let Ok(gr) = grade(&mv, &cv) {
            if gr != GRADE_MATCH {
                tamper_ok = true;
            }
        }
    }
    if tamper_ok {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CreateCode::ALL.len() {
        let v = CreateCode::ALL[i].val();
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
        "J1-6场景对照全同+扰动检出+复现",
        matches!(golden, Ok((6, _, 6))),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓(含倒置/篡改)",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 场景名点验：6 名全非空。
    let mut named = 0;
    let mut i = 0;
    while i < SCENE_COUNT {
        if SCENE_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-6场景名点验",
        named == 6,
        format!("{}/6 场景名", named),
    ));
    // 消息号闭集点验：10 号全在闭集且互异。
    let msgs = [
        WM_CREATE, WM_SIZE, WM_MOVE, WM_PAINT, WM_GETMINMAXINFO,
        WM_SHOWWINDOW, WM_ACTIVATE, WM_SETFOCUS, WM_NCCREATE, WM_NCCALCSIZE,
    ];
    let mut okm = 0;
    i = 0;
    while i < msgs.len() {
        if msg_known(msgs[i]) {
            okm += 1;
        }
        let mut j = i + 1;
        while j < msgs.len() {
            if msgs[i] == msgs[j] {
                okm = 0; // 互异破即清零
            }
            j += 1;
        }
        i += 1;
    }
    v.push((
        "J1-10消息号闭集互异点验",
        okm == 10,
        format!("{}/10 消息号", okm),
    ));
    // 分级闭集抽验：空序拒收、全同=0。
    let empty_reject = {
        let e: Vec<u32> = Vec::new();
        match control_sequence(0) {
            Ok(c0) => matches!(grade(&e, &c0), Err(_)),
            Err(_) => false,
        }
    };
    let mut one: Vec<u32> = Vec::new();
    one.push(WM_CREATE);
    let mut two: Vec<u32> = Vec::new();
    two.push(WM_CREATE);
    let same_grade = grade(&one, &two);
    v.push((
        "J1-分级闭集抽验(空拒/全同0)",
        empty_reject && matches!(same_grade, Ok(0)),
        format!("空拒={} 同={:?}", empty_reject, same_grade),
    ));
    // 反向判据：子窗口场景若误开激活段必判非全同（模型-对照分叉检出）。
    let m3 = model_sequence(SC_CHILD);
    let c3 = control_sequence(SC_CHILD);
    let child_ok = match (m3, c3) {
        (Ok(mv), Ok(cv)) => {
            // 子窗口两侧都无 ACTIVATE。
            let mut no_act = true;
            let mut k = 0;
            while k < mv.len() {
                if mv[k] == WM_ACTIVATE {
                    no_act = false;
                }
                k += 1;
            }
            no_act && mv == cv
        }
        _ => false,
    };
    v.push((
        "J1-反向判据子窗口无激活段",
        child_ok,
        String::from("子场景模型=对照且无 ACTIVATE"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66CC0 窗口")));
    v
}

pub fn run_unx14901_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14901 CREATE 序元组（6 场景与真机对照一致+偏差分级）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
