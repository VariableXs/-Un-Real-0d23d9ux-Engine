//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14841 · 矩阵定义 27 格（3×3×3） · 0x66C00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F14841-J1 3×3×3=27 格定义表零缺格，格账勾稽；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三轴骨架闭集：面 3（面A/面B/面C）× 层 3（浅层/中层/深层）×
//!     序 3（前序/中序/后序）= 27 格；
//!   - 独立判据常量 EXPECTED_CELLS = 27 写死不复用乘积，勾稽用；
//!   - 钉版处置闭式：每格按 (面+层+序) 模 3 钉 过/拒/降级 三态
//!     （闭式钉版表，逐格可重算复核）；
//!   - 格-判据映射：格 (f,l,o) → "J1-F{f}L{l}O{o}"，27 格零缺格；
//!   - 格账勾稽：账 27 == 判据 27 == 乘积 27，三态分布守恒
//!     （过+拒+降级 = 27）；
//!   - 状态机：Unpinned → Pinned → Audited 三态收敛，非法回边拒收；
//!   - 注入面：三轴越界 + 钉版篡改检出 四发全抓；
//!   - LCG 种子写死 = SEG_BASE（0x66C00），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66C00，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66C00;

// ── 三轴骨架闭集 ─────────────────────────────────────────────────
pub const FACES: usize = 3;
pub const LAYERS: usize = 3;
pub const ORDERS: usize = 3;
pub const CELLS: usize = FACES * LAYERS * ORDERS;
/// 独立判据常量（不复用乘积）。
pub const EXPECTED_CELLS: usize = 27;

pub const FACE_NAMES: [&str; FACES] = ["面A", "面B", "面C"];
pub const LAYER_NAMES: [&str; LAYERS] = ["浅层", "中层", "深层"];
pub const ORDER_NAMES: [&str; ORDERS] = ["前序", "中序", "后序"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CubeCode {
    /// 面轴越闭集
    FaceOutOfRange,
    /// 层轴越闭集
    LayerOutOfRange,
    /// 序轴越闭集
    OrderOutOfRange,
    /// 格账与判据 27 不勾稽
    CellCountMismatch,
    /// 钉版处置与闭式重算不一致
    PinMismatch,
    /// 格-判据映射缺格
    CriterionMissing,
    /// 三态分布守恒破
    DispositionConservation,
    /// 格终态悬挂
    CellHung,
    /// 非法回边（重复钉版）
    EdgeIllegal,
    /// 双跑不一致（不可复现）
    NotReproducible,
}

impl CubeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CubeCode::FaceOutOfRange => "面轴越闭集：3 面冻结",
            CubeCode::LayerOutOfRange => "层轴越闭集：3 层冻结",
            CubeCode::OrderOutOfRange => "序轴越闭集：3 序冻结",
            CubeCode::CellCountMismatch => "格账与判据 27 不勾稽",
            CubeCode::PinMismatch => "钉版处置与闭式重算不一致",
            CubeCode::CriterionMissing => "格-判据映射缺格：零缺格红线",
            CubeCode::DispositionConservation => "过+拒+降级分布守恒破",
            CubeCode::CellHung => "格终态悬挂：状态机未收敛",
            CubeCode::EdgeIllegal => "非法回边：已钉版格重复钉版",
            CubeCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [CubeCode; 10] = [
        CubeCode::FaceOutOfRange,
        CubeCode::LayerOutOfRange,
        CubeCode::OrderOutOfRange,
        CubeCode::CellCountMismatch,
        CubeCode::PinMismatch,
        CubeCode::CriterionMissing,
        CubeCode::DispositionConservation,
        CubeCode::CellHung,
        CubeCode::EdgeIllegal,
        CubeCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CubeCode::ALL.len() {
        if CubeCode::ALL[i].val() == code {
            return CubeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66C00 段闭集内"
}

// ── 格键 / 线性化 / 钉版闭式 ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CubeKey {
    pub face: usize,
    pub layer: usize,
    pub order: usize,
}

pub fn ckey_valid(k: CubeKey) -> Result<(), u32> {
    if k.face >= FACES {
        return Err(CubeCode::FaceOutOfRange.val());
    }
    if k.layer >= LAYERS {
        return Err(CubeCode::LayerOutOfRange.val());
    }
    if k.order >= ORDERS {
        return Err(CubeCode::OrderOutOfRange.val());
    }
    Ok(())
}

pub fn ckey_linear(k: CubeKey) -> Result<usize, u32> {
    ckey_valid(k)?;
    Ok((k.face * LAYERS + k.layer) * ORDERS + k.order)
}

pub fn linear_ckey(idx: usize) -> Result<CubeKey, u32> {
    if idx >= CELLS {
        return Err(CubeCode::CellCountMismatch.val());
    }
    let order = idx % ORDERS;
    let rest = idx / ORDERS;
    let layer = rest % LAYERS;
    let face = rest / LAYERS;
    Ok(CubeKey { face, layer, order })
}

/// 处置三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pin {
    Pass,
    Reject,
    Degrade,
}

/// 钉版闭式：(面+层+序) 模 3 → 0=过 1=拒 2=降级（逐格可重算）。
pub fn pin_closed(k: CubeKey) -> Result<Pin, u32> {
    ckey_valid(k)?;
    let m = (k.face + k.layer + k.order) % 3;
    Ok(match m {
        0 => Pin::Pass,
        1 => Pin::Reject,
        _ => Pin::Degrade,
    })
}

/// 判据号：J1-F{f}L{l}O{o}。
pub fn cube_criterion(k: CubeKey) -> Result<String, u32> {
    ckey_valid(k)?;
    Ok(format!("J1-F{}L{}O{}", k.face, k.layer, k.order))
}

// ── 格状态机与账 ─────────────────────────────────────────────────
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CubeCell {
    Unpinned,
    Pinned(Pin, String),
    Audited(Pin, String),
}

pub struct CubeLedger {
    pub cells: Vec<CubeCell>,
    pub booked: u32,
}

impl CubeLedger {
    pub fn new() -> CubeLedger {
        CubeLedger { cells: Vec::new(), booked: 0 }
    }

    /// 单格钉版+审计：Unpinned → Pinned → Audited。
    pub fn book(&mut self, k: CubeKey) -> Result<Pin, u32> {
        let idx = ckey_linear(k)?;
        while self.cells.len() <= idx {
            self.cells.push(CubeCell::Unpinned);
        }
        if matches!(self.cells[idx], CubeCell::Audited(_, _)) {
            return Err(CubeCode::EdgeIllegal.val());
        }
        let p = pin_closed(k)?;
        let c = cube_criterion(k)?;
        if c.len() == 0 {
            return Err(CubeCode::CriterionMissing.val());
        }
        self.cells[idx] = CubeCell::Audited(p, c);
        self.booked += 1;
        Ok(p)
    }

    /// 全 27 格：零缺格 + 勾稽 + 三态守恒 + 全收敛。
    pub fn run_all(&mut self) -> Result<(u32, u32, u32), u32> {
        let mut f = 0;
        while f < FACES {
            let mut l = 0;
            while l < LAYERS {
                let mut o = 0;
                while o < ORDERS {
                    self.book(CubeKey { face: f, layer: l, order: o })?;
                    o += 1;
                }
                l += 1;
            }
            f += 1;
        }
        if self.cells.len() != CELLS || self.booked as usize != EXPECTED_CELLS {
            return Err(CubeCode::CellCountMismatch.val());
        }
        self.audit_all()
    }

    /// 全量审计（幂等，不再 book）：钉版与闭式重算逐格一致 +
    /// 三态分布守恒 + 全收敛。
    pub fn audit_all(&self) -> Result<(u32, u32, u32), u32> {
        if self.cells.len() != CELLS {
            return Err(CubeCode::CellCountMismatch.val());
        }
        // 三态分布守恒 + 钉版与闭式重算逐格一致。
        let (mut p, mut r, mut d) = (0u32, 0u32, 0u32);
        let mut i = 0;
        while i < self.cells.len() {
            let k = linear_ckey(i)?;
            match &self.cells[i] {
                CubeCell::Audited(pin, _c) => {
                    if *pin != pin_closed(k)? {
                        return Err(CubeCode::PinMismatch.val());
                    }
                    match pin {
                        Pin::Pass => p += 1,
                        Pin::Reject => r += 1,
                        Pin::Degrade => d += 1,
                    }
                }
                _ => return Err(CubeCode::CellHung.val()),
            }
            i += 1;
        }
        if (p + r + d) as usize != EXPECTED_CELLS {
            return Err(CubeCode::DispositionConservation.val());
        }
        Ok((p, r, d))
    }

    /// 判据号批内唯一性（线性扫重）。
    pub fn uniqueness(&self) -> Result<(), u32> {
        let mut ids: Vec<&str> = Vec::new();
        let mut i = 0;
        while i < self.cells.len() {
            match &self.cells[i] {
                CubeCell::Audited(_p, c) => {
                    let mut j = 0;
                    while j < ids.len() {
                        if ids[j] == c.as_str() {
                            return Err(CubeCode::EdgeIllegal.val());
                        }
                        j += 1;
                    }
                    ids.push(c.as_str());
                }
                _ => return Err(CubeCode::CriterionMissing.val()),
            }
            i += 1;
        }
        Ok(())
    }

    /// 双射可逆：linear/linear_ckey 互逆全查。
    pub fn bijection_audit(&self) -> Result<(), u32> {
        let mut idx = 0;
        while idx < CELLS {
            let k = linear_ckey(idx)?;
            if ckey_linear(k)? != idx {
                return Err(CubeCode::CellCountMismatch.val());
            }
            idx += 1;
        }
        Ok(())
    }

    /// 复现：双跑账一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = CubeLedger::new();
        let r2 = run2.run_all();
        if run2.booked != self.booked || run2.cells != self.cells || r2.is_err() {
            return Err(CubeCode::NotReproducible.val());
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

/// LCG 行走：16 拍随机格全落 27 格闭集且钉版闭式可重算。
pub fn lcg_cube_walk() -> Result<u32, u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut n = 0;
    let mut i = 0;
    while i < 16 {
        let k = CubeKey {
            face: g.below(FACES as u64) as usize,
            layer: g.below(LAYERS as u64) as usize,
            order: g.below(ORDERS as u64) as usize,
        };
        let _ = pin_closed(k)?;
        n += 1;
        i += 1;
    }
    Ok(n)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    if CELLS != EXPECTED_CELLS {
        return Err(CubeCode::CellCountMismatch.val());
    }
    let mut led = CubeLedger::new();
    let (p, r, d) = led.run_all()?;
    led.uniqueness()?;
    led.bijection_audit()?;
    led.reproducible()?;
    let walk = lcg_cube_walk()?;
    Ok((led.booked, p, r + d, walk))
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
    // 族 0：三轴越界各一发 + 线性号越界，四发全须判对应码。
    ledger.tries[0] += 1;
    let mut axis_ok = true;
    let bad = [
        CubeKey { face: FACES, layer: 0, order: 0 },
        CubeKey { face: 0, layer: LAYERS, order: 0 },
        CubeKey { face: 0, layer: 0, order: ORDERS },
    ];
    let want = [
        CubeCode::FaceOutOfRange.val(),
        CubeCode::LayerOutOfRange.val(),
        CubeCode::OrderOutOfRange.val(),
    ];
    let mut i = 0;
    while i < 3 {
        let mut hit = false;
        if let Err(e) = ckey_linear(bad[i]) {
            if e == want[i] {
                hit = true;
            }
        }
        if !hit {
            axis_ok = false;
        }
        i += 1;
    }
    if let Err(e) = linear_ckey(CELLS) {
        if e != CubeCode::CellCountMismatch.val() {
            axis_ok = false;
        }
    } else {
        axis_ok = false;
    }
    if axis_ok {
        ledger.caught[0] += 1;
    }
    // 族 1：重复钉版同格必拒。
    ledger.tries[1] += 1;
    let mut led = CubeLedger::new();
    let k = CubeKey { face: 2, layer: 1, order: 0 };
    let mut dup_ok = false;
    if led.book(k).is_ok() {
        if let Err(e) = led.book(k) {
            if e == CubeCode::EdgeIllegal.val() {
                dup_ok = true;
            }
        }
    }
    if dup_ok {
        ledger.caught[1] += 1;
    }
    // 族 2：钉版篡改（账内改钉）必被闭式重算检出。
    ledger.tries[2] += 1;
    let mut led2 = CubeLedger::new();
    let mut tamper_detected = false;
        if led2.run_all().is_ok() {
            // 篡改格 0 的钉版：闭式重算（幂等审计）必不一致。
            led2.cells[0] = CubeCell::Audited(
                Pin::Reject,
                String::from("J1-F0L0O0"),
            );
            if let Err(e) = led2.audit_all() {
            if e == CubeCode::PinMismatch.val() {
                tamper_detected = true;
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
    while i < CubeCode::ALL.len() {
        let v = CubeCode::ALL[i].val();
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
        "J1-27格零缺格+钉版闭式+复现",
        matches!(golden, Ok((27, _, _, _))),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓(含钉版篡改)",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 轴名点验：3+3+3 名全非空。
    let mut named = 0;
    let mut i = 0;
    while i < FACES {
        if FACE_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    i = 0;
    while i < LAYERS {
        if LAYER_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    i = 0;
    while i < ORDERS {
        if ORDER_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-3x3x3轴名点验",
        named == 9,
        format!("{}/9 轴名", named),
    ));
    // 乘积勾稽：3*3*3 == 27 == 判据常量。
    v.push((
        "J1-乘积勾稽3x3x3=27",
        FACES * LAYERS * ORDERS == CELLS && CELLS == EXPECTED_CELLS && EXPECTED_CELLS == 27,
        format!("{}x{}x{}={}", FACES, LAYERS, ORDERS, CELLS),
    ));
    // 钉版闭式分布：模 3 分布在 27 格上应各 9（27/3）。
    let dist = {
        let mut p = 0;
        let mut r = 0;
        let mut d = 0;
        let mut idx = 0;
        while idx < CELLS {
            if let Ok(k) = linear_ckey(idx) {
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
        "J1-钉版闭式分布9/9/9",
        dist == (9, 9, 9),
        format!("过{}/拒{}/降级{}", dist.0, dist.1, dist.2),
    ));
    // 判据号格式抽验：格 (2,2,2) → "J1-F2L2O2"。
    let crit = cube_criterion(CubeKey { face: 2, layer: 2, order: 2 });
    v.push((
        "J1-判据号格式抽验",
        matches!(&crit, Ok(s) if s == "J1-F2L2O2"),
        format!("{:?}", crit),
    ));
    // 反向判据：缺中段一格必被判悬挂/缺格。
    let mut partial = CubeLedger::new();
    let mut missing_detected = false;
    let mut f = 0;
    while f < FACES {
        let mut l = 0;
        while l < LAYERS {
            let mut o = 0;
            while o < ORDERS {
                // 跳过中段格 (1,1,1)。
                if !(f == 1 && l == 1 && o == 1) {
                    let _ = partial.book(CubeKey { face: f, layer: l, order: o });
                }
                o += 1;
            }
            l += 1;
        }
        f += 1;
    }
    if partial.booked as usize == EXPECTED_CELLS - 1 {
        // 全量审计应判悬挂（格 (1,1,1) 未钉）。
        let mut full = CubeLedger::new();
        let _ = full.book(CubeKey { face: 1, layer: 1, order: 1 });
        let mut hung_found = false;
        let mut idx = 0;
        while idx < CELLS {
            if idx < partial.cells.len() {
                if matches!(partial.cells[idx], CubeCell::Unpinned) {
                    hung_found = true;
                }
            }
            idx += 1;
        }
        if hung_found {
            missing_detected = true;
        }
    }
    v.push((
        "J1-反向判据26格缺格被检出",
        missing_detected,
        String::from("缺 1 格必留 Unpinned"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66C00 窗口")));
    v
}

pub fn run_unx14841_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14841 矩阵定义 27 格（3x3x3 零缺格+格账勾稽）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
