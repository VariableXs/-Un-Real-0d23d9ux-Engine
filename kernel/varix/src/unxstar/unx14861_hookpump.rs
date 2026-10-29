//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14861 · 矩阵定义与格账（钩型 5 × 泵态 3 × 广播面 2 = 30 格）
//! · 0x66C40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F14861-J1 钩型 5 × 泵态 3 × 广播面 2 = 30 格定义零缺格；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三轴骨架闭集：钩型 5（键盘钩/鼠标钩/日志钩/窗口钩/调试钩，
//!     名表写死）× 泵态 3（待泵/泵中/泵毕）× 广播面 2（直播面/
//!     回放面）= 30 格；
//!   - 独立判据常量 EXPECTED_CELLS = 30 写死不复用乘积，勾稽用；
//!   - 钉版闭式：直播面全过；回放面按钩型×泵态语义钉（待泵=降级/
//!     泵中=拒/泵毕=过），30 格逐格闭式可重算；
//!   - 泵态状态机：待泵 → 泵中 → 泵毕 单向合法边闭集，非法回边
//!     （泵毕回泵中、泵中回待泵、越级）拒收；
//!   - 格-判据映射：格 (h,p,b) → "J1-H{h}P{p}B{b}"，30 格零缺格；
//!   - 注入面：轴越界/非法泵边/钉版篡改 三族全抓；
//!   - LCG 种子写死 = SEG_BASE（0x66C40），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66C40，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66C40;

// ── 三轴骨架闭集 ─────────────────────────────────────────────────
pub const HOOKS: usize = 5;
pub const PUMP_STATES: usize = 3;
pub const BCAST_FACES: usize = 2;
pub const CELLS: usize = HOOKS * PUMP_STATES * BCAST_FACES;
/// 独立判据常量（不复用乘积）。
pub const EXPECTED_CELLS: usize = 30;

pub const HOOK_NAMES: [&str; HOOKS] =
    ["键盘钩", "鼠标钩", "日志钩", "窗口钩", "调试钩"];
pub const PUMP_NAMES: [&str; PUMP_STATES] = ["待泵", "泵中", "泵毕"];
pub const FACE_NAMES: [&str; BCAST_FACES] = ["直播面", "回放面"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookCode {
    /// 钩型轴越闭集
    HookOutOfRange,
    /// 泵态轴越闭集
    PumpOutOfRange,
    /// 广播面轴越闭集
    FaceOutOfRange,
    /// 泵态状态机非法边
    PumpEdgeIllegal,
    /// 格账与判据 30 不勾稽
    CellCountMismatch,
    /// 钉版处置与闭式重算不一致
    PinMismatch,
    /// 格-判据映射缺格
    CriterionMissing,
    /// 格终态悬挂
    CellHung,
    /// 非法回边（重复钉版）
    EdgeIllegal,
    /// 双跑不一致（不可复现）
    NotReproducible,
}

impl HookCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HookCode::HookOutOfRange => "钩型轴越闭集：5 钩型冻结",
            HookCode::PumpOutOfRange => "泵态轴越闭集：3 泵态冻结",
            HookCode::FaceOutOfRange => "广播面轴越闭集：2 面冻结",
            HookCode::PumpEdgeIllegal => "泵态状态机非法边：单向边闭集",
            HookCode::CellCountMismatch => "格账与判据 30 不勾稽",
            HookCode::PinMismatch => "钉版处置与闭式重算不一致",
            HookCode::CriterionMissing => "格-判据映射缺格：零缺格红线",
            HookCode::CellHung => "格终态悬挂：状态机未收敛",
            HookCode::EdgeIllegal => "非法回边：已钉版格重复钉版",
            HookCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [HookCode; 10] = [
        HookCode::HookOutOfRange,
        HookCode::PumpOutOfRange,
        HookCode::FaceOutOfRange,
        HookCode::PumpEdgeIllegal,
        HookCode::CellCountMismatch,
        HookCode::PinMismatch,
        HookCode::CriterionMissing,
        HookCode::CellHung,
        HookCode::EdgeIllegal,
        HookCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HookCode::ALL.len() {
        if HookCode::ALL[i].val() == code {
            return HookCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66C40 段闭集内"
}

// ── 格键 / 线性化 ────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HookKey {
    pub hook: usize,
    pub pump: usize,
    pub face: usize,
}

pub fn hkey_valid(k: HookKey) -> Result<(), u32> {
    if k.hook >= HOOKS {
        return Err(HookCode::HookOutOfRange.val());
    }
    if k.pump >= PUMP_STATES {
        return Err(HookCode::PumpOutOfRange.val());
    }
    if k.face >= BCAST_FACES {
        return Err(HookCode::FaceOutOfRange.val());
    }
    Ok(())
}

pub fn hkey_linear(k: HookKey) -> Result<usize, u32> {
    hkey_valid(k)?;
    Ok((k.hook * PUMP_STATES + k.pump) * BCAST_FACES + k.face)
}

pub fn linear_hkey(idx: usize) -> Result<HookKey, u32> {
    if idx >= CELLS {
        return Err(HookCode::CellCountMismatch.val());
    }
    let face = idx % BCAST_FACES;
    let rest = idx / BCAST_FACES;
    let pump = rest % PUMP_STATES;
    let hook = rest / PUMP_STATES;
    Ok(HookKey { hook, pump, face })
}

// ── 泵态状态机（单向合法边闭集） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PumpPhase {
    Idle,    // 待泵
    Pumping, // 泵中
    Done,    // 泵毕
}

/// 合法边：待泵→泵中、泵中→泵毕；其余全拒。
pub fn pump_advance(cur: PumpPhase, next: PumpPhase) -> Result<PumpPhase, u32> {
    let legal = matches!(
        (cur, next),
        (PumpPhase::Idle, PumpPhase::Pumping) | (PumpPhase::Pumping, PumpPhase::Done)
    );
    if !legal {
        return Err(HookCode::PumpEdgeIllegal.val());
    }
    Ok(next)
}

/// 泵态全序走查：Idle → Pumping → Done 三步收敛。
pub fn pump_full_walk() -> Result<(), u32> {
    let s = pump_advance(PumpPhase::Idle, PumpPhase::Pumping)?;
    let d = pump_advance(s, PumpPhase::Done)?;
    if d != PumpPhase::Done {
        return Err(HookCode::PumpEdgeIllegal.val());
    }
    Ok(())
}

// ── 钉版闭式 ─────────────────────────────────────────────────────
/// 处置三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pin {
    Pass,
    Reject,
    Degrade,
}

/// 钉版闭式（写死语义：直播面全过；回放面按泵态——待泵=降级/
/// 泵中=拒/泵毕=过），逐格可重算。
pub fn pin_closed(k: HookKey) -> Result<Pin, u32> {
    hkey_valid(k)?;
    if k.face == 0 {
        return Ok(Pin::Pass);
    }
    Ok(match k.pump {
        0 => Pin::Degrade,
        1 => Pin::Reject,
        _ => Pin::Pass,
    })
}

/// 判据号：J1-H{h}P{p}B{b}。
pub fn hook_criterion(k: HookKey) -> Result<String, u32> {
    hkey_valid(k)?;
    Ok(format!("J1-H{}P{}B{}", k.hook, k.pump, k.face))
}

// ── 格状态机与账 ─────────────────────────────────────────────────
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum HookCell {
    Unpinned,
    Pinned(Pin, String),
    Audited(Pin, String),
}

pub struct HookLedger {
    pub cells: Vec<HookCell>,
    pub booked: u32,
}

impl HookLedger {
    pub fn new() -> HookLedger {
        HookLedger { cells: Vec::new(), booked: 0 }
    }

    /// 单格钉版+审计。
    pub fn book(&mut self, k: HookKey) -> Result<Pin, u32> {
        let idx = hkey_linear(k)?;
        while self.cells.len() <= idx {
            self.cells.push(HookCell::Unpinned);
        }
        if matches!(self.cells[idx], HookCell::Audited(_, _)) {
            return Err(HookCode::EdgeIllegal.val());
        }
        let p = pin_closed(k)?;
        let c = hook_criterion(k)?;
        if c.len() == 0 {
            return Err(HookCode::CriterionMissing.val());
        }
        self.cells[idx] = HookCell::Audited(p, c);
        self.booked += 1;
        Ok(p)
    }

    /// 全 30 格：零缺格 + 勾稽 + 全收敛。
    pub fn run_all(&mut self) -> Result<(u32, u32, u32), u32> {
        let mut h = 0;
        while h < HOOKS {
            let mut p = 0;
            while p < PUMP_STATES {
                let mut b = 0;
                while b < BCAST_FACES {
                    self.book(HookKey { hook: h, pump: p, face: b })?;
                    b += 1;
                }
                p += 1;
            }
            h += 1;
        }
        if self.cells.len() != CELLS || self.booked as usize != EXPECTED_CELLS {
            return Err(HookCode::CellCountMismatch.val());
        }
        self.audit_all()
    }

    /// 全量审计（幂等，不再 book）：钉版闭式重算 + 全收敛 + 分布守恒。
    pub fn audit_all(&self) -> Result<(u32, u32, u32), u32> {
        if self.cells.len() != CELLS {
            return Err(HookCode::CellCountMismatch.val());
        }
        let (mut p, mut r, mut d) = (0u32, 0u32, 0u32);
        let mut i = 0;
        while i < self.cells.len() {
            let k = linear_hkey(i)?;
            match &self.cells[i] {
                HookCell::Audited(pin, _c) => {
                    if *pin != pin_closed(k)? {
                        return Err(HookCode::PinMismatch.val());
                    }
                    match pin {
                        Pin::Pass => p += 1,
                        Pin::Reject => r += 1,
                        Pin::Degrade => d += 1,
                    }
                }
                _ => return Err(HookCode::CellHung.val()),
            }
            i += 1;
        }
        if (p + r + d) as usize != EXPECTED_CELLS {
            return Err(HookCode::CellCountMismatch.val());
        }
        Ok((p, r, d))
    }

    /// 判据号批内唯一性（线性扫重）。
    pub fn uniqueness(&self) -> Result<(), u32> {
        let mut ids: Vec<&str> = Vec::new();
        let mut i = 0;
        while i < self.cells.len() {
            match &self.cells[i] {
                HookCell::Audited(_p, c) => {
                    let mut j = 0;
                    while j < ids.len() {
                        if ids[j] == c.as_str() {
                            return Err(HookCode::EdgeIllegal.val());
                        }
                        j += 1;
                    }
                    ids.push(c.as_str());
                }
                _ => return Err(HookCode::CriterionMissing.val()),
            }
            i += 1;
        }
        Ok(())
    }

    /// 复现：双跑账一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = HookLedger::new();
        let r2 = run2.run_all();
        if run2.booked != self.booked || run2.cells != self.cells || r2.is_err() {
            return Err(HookCode::NotReproducible.val());
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

/// LCG 行走：16 拍随机格全落 30 格闭集且钉版闭式可重算。
pub fn lcg_hook_walk() -> Result<u32, u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut n = 0;
    let mut i = 0;
    while i < 16 {
        let k = HookKey {
            hook: g.below(HOOKS as u64) as usize,
            pump: g.below(PUMP_STATES as u64) as usize,
            face: g.below(BCAST_FACES as u64) as usize,
        };
        let _ = pin_closed(k)?;
        n += 1;
        i += 1;
    }
    Ok(n)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    if CELLS != EXPECTED_CELLS {
        return Err(HookCode::CellCountMismatch.val());
    }
    pump_full_walk()?;
    let mut led = HookLedger::new();
    let (p, r, d) = led.run_all()?;
    led.uniqueness()?;
    led.reproducible()?;
    let walk = lcg_hook_walk()?;
    Ok((led.booked, p + r + d, walk))
}

// ── 注入面：三族 5/5 必抓 ────────────────────────────────────────
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
    // 族 0：三轴越界各一发 + 线性号越界。
    ledger.tries[0] += 1;
    let mut axis_ok = true;
    let bad = [
        HookKey { hook: HOOKS, pump: 0, face: 0 },
        HookKey { hook: 0, pump: PUMP_STATES, face: 0 },
        HookKey { hook: 0, pump: 0, face: BCAST_FACES },
    ];
    let want = [
        HookCode::HookOutOfRange.val(),
        HookCode::PumpOutOfRange.val(),
        HookCode::FaceOutOfRange.val(),
    ];
    let mut i = 0;
    while i < 3 {
        let mut hit = false;
        if let Err(e) = hkey_linear(bad[i]) {
            if e == want[i] {
                hit = true;
            }
        }
        if !hit {
            axis_ok = false;
        }
        i += 1;
    }
    if let Err(e) = linear_hkey(CELLS) {
        if e != HookCode::CellCountMismatch.val() {
            axis_ok = false;
        }
    } else {
        axis_ok = false;
    }
    if axis_ok {
        ledger.caught[0] += 1;
    }
    // 族 1：非法泵边四发（越级/回边/自环/末态前进）全拒。
    ledger.tries[1] += 1;
    let mut edge_ok = true;
    if pump_advance(PumpPhase::Idle, PumpPhase::Done).is_ok() {
        edge_ok = false;
    }
    if pump_advance(PumpPhase::Pumping, PumpPhase::Idle).is_ok() {
        edge_ok = false;
    }
    if pump_advance(PumpPhase::Done, PumpPhase::Done).is_ok() {
        edge_ok = false;
    }
    if pump_advance(PumpPhase::Done, PumpPhase::Pumping).is_ok() {
        edge_ok = false;
    }
    if edge_ok {
        ledger.caught[1] += 1;
    }
    // 族 2：钉版篡改（回放面泵中格改过）必被闭式重算（幂等审计）检出。
    ledger.tries[2] += 1;
    let mut led = HookLedger::new();
    let mut tamper_detected = false;
    if led.run_all().is_ok() {
        // 格 (0,1,1)：回放面+泵中 → 闭式=拒，篡改为过。
        if let Ok(idx) = hkey_linear(HookKey { hook: 0, pump: 1, face: 1 }) {
            led.cells[idx] = HookCell::Audited(Pin::Pass, String::from("J1-H0P1B1"));
            if let Err(e) = led.audit_all() {
                if e == HookCode::PinMismatch.val() {
                    tamper_detected = true;
                }
            }
        }
    }
    if tamper_detected {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HookCode::ALL.len() {
        let v = HookCode::ALL[i].val();
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
        "J1-30格零缺格+钉版闭式+复现",
        matches!(golden, Ok((30, 30, _))),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓(含泵边/篡改)",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 轴名点验：5+3+2 名全非空。
    let mut named = 0;
    let mut i = 0;
    while i < HOOKS {
        if HOOK_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    i = 0;
    while i < PUMP_STATES {
        if PUMP_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    i = 0;
    while i < BCAST_FACES {
        if FACE_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-5x3x2轴名点验",
        named == 10,
        format!("{}/10 轴名", named),
    ));
    // 乘积勾稽：5*3*2 == 30 == 判据常量。
    v.push((
        "J1-乘积勾稽5x3x2=30",
        HOOKS * PUMP_STATES * BCAST_FACES == CELLS
            && CELLS == EXPECTED_CELLS
            && EXPECTED_CELLS == 30,
        format!("{}x{}x{}={}", HOOKS, PUMP_STATES, BCAST_FACES, CELLS),
    ));
    // 钉版闭式分布：直播面 15 过；回放面 5 过/5 拒/5 降级。
    let dist = {
        let mut p = 0;
        let mut r = 0;
        let mut d = 0;
        let mut idx = 0;
        while idx < CELLS {
            if let Ok(k) = linear_hkey(idx) {
                if let Ok(pin) = pin_closed(k) {
                    match pin {
                        Pin::Pass => p += 1,
                        Pin::Reject => r += 1,
                        Pin::Degrade => d += 1,
                    }
                }
            }
            idx += 1;
        }
        (p, r, d)
    };
    v.push((
        "J1-钉版闭式分布20过/5拒/5降级",
        dist == (20, 5, 5),
        format!("过{}/拒{}/降级{}", dist.0, dist.1, dist.2),
    ));
    // 判据号格式抽验：格 (3,2,1) → "J1-H3P2B1"。
    let crit = hook_criterion(HookKey { hook: 3, pump: 2, face: 1 });
    v.push((
        "J1-判据号格式抽验",
        matches!(&crit, Ok(s) if s == "J1-H3P2B1"),
        format!("{:?}", crit),
    ));
    // 反向判据：缺中段一格（(2,1,0)）必留 Unpinned。
    let mut partial = HookLedger::new();
    let mut missing_detected = false;
    let mut h = 0;
    while h < HOOKS {
        let mut p = 0;
        while p < PUMP_STATES {
            let mut b = 0;
            while b < BCAST_FACES {
                if !(h == 2 && p == 1 && b == 0) {
                    let _ = partial.book(HookKey { hook: h, pump: p, face: b });
                }
                b += 1;
            }
            p += 1;
        }
        h += 1;
    }
    if partial.booked as usize == EXPECTED_CELLS - 1 {
        if let Ok(idx) = hkey_linear(HookKey { hook: 2, pump: 1, face: 0 }) {
            if idx < partial.cells.len() {
                if matches!(partial.cells[idx], HookCell::Unpinned) {
                    missing_detected = true;
                }
            }
        }
    }
    v.push((
        "J1-反向判据29格缺格被检出",
        missing_detected,
        String::from("缺 1 格必留 Unpinned"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66C40 窗口")));
    v
}

pub fn run_unx14861_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14861 矩阵定义与格账（钩型5x泵态3x广播面2=30 格）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
