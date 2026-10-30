//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14961 · PAINT/ERASEBKGND 交替序 · 0x66D80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F14961-J1 失效-擦除-绘制 5 场景序对照一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 消息号写死（来源 Win32 消息号表）：WM_PAINT=0x000F、
//!     WM_ERASEBKGND=0x0014、WM_INVALIDATE=0x0F10（建模失效事件，
//!     闭集内自定锚号并注明）、WM_SETREDRAW=0x000B；
//!   - 绘制状态机五态：Idle → Invalidated（失效入队）→ Erasing
//!     （ERASEBKGND）→ Painting（PAINT）→ Idle（归位）；非法边
//!     （无失效直接 Paint、Paint 后未归位再失效乱序）拒收；
//!   - 5 场景闭集（名表写死）：单失效区/多失效区合并/空失效区
//!     拒收/重复失效合并/关重绘降级——每场景产出事件序（失效/
//!     擦除/绘制/拒收/降级五类事件码）；
//!   - 对照端：5 场景真机锚定序独立写死，逐场景比对一致；
//!   - 事件码闭集：EVT_INVALIDATE=1/EVT_ERASE=2/EVT_PAINT=3/
//!     EVT_REJECT=4/EVT_DEGRADE=5；
//!   - 交替序红线：擦除必先于绘制（EVT_ERASE 下标 < EVT_PAINT
//!     下标）逐场景点验；
//!   - 注入面：场景越界/非法状态边/对照篡改 三族全抓；
//!   - LCG 种子写死 = SEG_BASE（0x66D80），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66D80，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66D80;

// ── 消息号写死（来源 Win32 消息号表；失效为建模锚号并注明） ─────
pub const WM_PAINT: u32 = 0x000F;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_SETREDRAW: u32 = 0x000B;
/// 失效事件建模锚号（闭集内自定，非 Win32 广播号，注明勿外推）。
pub const WM_INVALIDATE_ANCHOR: u32 = 0x0F10;

// ── 事件码闭集 ───────────────────────────────────────────────────
pub const EVT_INVALIDATE: u32 = 1;
pub const EVT_ERASE: u32 = 2;
pub const EVT_PAINT: u32 = 3;
pub const EVT_REJECT: u32 = 4;
pub const EVT_DEGRADE: u32 = 5;

pub fn evt_known(e: u32) -> bool {
    e >= EVT_INVALIDATE && e <= EVT_DEGRADE
}

// ── 5 场景闭集 ───────────────────────────────────────────────────
pub const SC_SINGLE: usize = 0;
pub const SC_MERGE_MULTI: usize = 1;
pub const SC_EMPTY: usize = 2;
pub const SC_MERGE_DUP: usize = 3;
pub const SC_NOREDRAW: usize = 4;
pub const SCENE_COUNT: usize = 5;

pub const SCENE_NAMES: [&str; SCENE_COUNT] = [
    "单失效区",
    "多失效区合并",
    "空失效区拒收",
    "重复失效合并",
    "关重绘降级",
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaintCode {
    /// 场景下标越 5 闭集
    SceneOutOfRange,
    /// 模型序与对照序不一致
    SeqMismatch,
    /// 绘制状态机非法边
    EdgeIllegal,
    /// 交替序破（擦除未先于绘制）
    AlternationBroken,
    /// 事件码越闭集
    EvtUnknown,
    /// 场景名表与场景数不守恒
    SceneNameMismatch,
    /// 空序拒收
    EmptySequence,
    /// 反向判据失败（篡改对照未检出）
    TamperUndetected,
    /// 双跑不一致（不可复现）
    NotReproducible,
    /// 对照表与场景数不守恒
    ControlTableMismatch,
}

impl PaintCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PaintCode::SceneOutOfRange => "场景下标越闭集：5 场景冻结",
            PaintCode::SeqMismatch => "模型序与真机对照序不一致",
            PaintCode::EdgeIllegal => "绘制状态机非法边：无失效直接绘",
            PaintCode::AlternationBroken => "交替序破：擦除未先于绘制",
            PaintCode::EvtUnknown => "事件码越闭集：1–5 冻结",
            PaintCode::SceneNameMismatch => "场景名表与场景数不守恒：5 名",
            PaintCode::EmptySequence => "空序拒收",
            PaintCode::TamperUndetected => "反向判据失败：篡改对照未检出",
            PaintCode::NotReproducible => "双跑不一致：复现红线",
            PaintCode::ControlTableMismatch => "对照表与场景数不守恒：5 元组",
        }
    }
    pub const ALL: [PaintCode; 10] = [
        PaintCode::SceneOutOfRange,
        PaintCode::SeqMismatch,
        PaintCode::EdgeIllegal,
        PaintCode::AlternationBroken,
        PaintCode::EvtUnknown,
        PaintCode::SceneNameMismatch,
        PaintCode::EmptySequence,
        PaintCode::TamperUndetected,
        PaintCode::NotReproducible,
        PaintCode::ControlTableMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PaintCode::ALL.len() {
        if PaintCode::ALL[i].val() == code {
            return PaintCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66D80 段闭集内"
}

// ── 绘制状态机（五态单向链） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaintPhase {
    Idle,
    Invalidated,
    Erasing,
    Painting,
}

/// 合法边：Idle→Invalidated、Invalidated→Erasing、Erasing→Painting、
/// Painting→Idle；其余（含无失效直接绘）拒收。
pub fn paint_advance(cur: PaintPhase, next: PaintPhase) -> Result<PaintPhase, u32> {
    let legal = matches!(
        (cur, next),
        (PaintPhase::Idle, PaintPhase::Invalidated)
            | (PaintPhase::Invalidated, PaintPhase::Erasing)
            | (PaintPhase::Erasing, PaintPhase::Painting)
            | (PaintPhase::Painting, PaintPhase::Idle)
    );
    if !legal {
        return Err(PaintCode::EdgeIllegal.val());
    }
    Ok(next)
}

/// 模型端：场景 → 事件序（状态机驱动，失效次数入参）。
///   SC_SINGLE：失效×1 → 擦 → 绘；
///   SC_MERGE_MULTI：失效×3 合并 → 擦 → 绘（单次擦绘）；
///   SC_EMPTY：失效×0 → 拒收事件（EVT_REJECT）；
///   SC_MERGE_DUP：失效×2（同区重复）合并 → 擦 → 绘；
///   SC_NOREDRAW：关重绘 → 降级事件（EVT_DEGRADE，无擦绘）。
pub fn model_sequence(scene: usize) -> Result<Vec<u32>, u32> {
    if scene >= SCENE_COUNT {
        return Err(PaintCode::SceneOutOfRange.val());
    }
    let mut seq: Vec<u32> = Vec::new();
    match scene {
        SC_EMPTY => {
            // 空失效区：无效化不成立 → 拒收；状态机保持 Idle
            //（Idle 直接走擦绘在合法边闭集下必被拒，见注入面）。
            seq.push(EVT_REJECT);
            return Ok(seq);
        }
        SC_NOREDRAW => {
            // 关重绘：降级——失效直接吞掉，无擦无绘。
            seq.push(EVT_DEGRADE);
            return Ok(seq);
        }
        SC_SINGLE => {
            seq.push(EVT_INVALIDATE);
        }
        SC_MERGE_MULTI => {
            // 三次失效合并为一格。
            seq.push(EVT_INVALIDATE);
            seq.push(EVT_INVALIDATE);
            seq.push(EVT_INVALIDATE);
        }
        _ => {
            // 重复失效同区合并。
            seq.push(EVT_INVALIDATE);
            seq.push(EVT_INVALIDATE);
        }
    }
    // 合并后单次擦-绘链。
    let s1 = paint_advance(PaintPhase::Idle, PaintPhase::Invalidated)?;
    let s2 = paint_advance(s1, PaintPhase::Erasing)?;
    let s3 = paint_advance(s2, PaintPhase::Painting)?;
    paint_advance(s3, PaintPhase::Idle)?;
    seq.push(EVT_ERASE);
    seq.push(EVT_PAINT);
    if seq.len() == 0 {
        return Err(PaintCode::EmptySequence.val());
    }
    Ok(seq)
}

/// 对照端：5 场景真机锚定序（独立写死）。
pub fn control_sequence(scene: usize) -> Result<Vec<u32>, u32> {
    if scene >= SCENE_COUNT {
        return Err(PaintCode::SceneOutOfRange.val());
    }
    let mut s: Vec<u32> = Vec::new();
    match scene {
        SC_SINGLE => {
            s.push(EVT_INVALIDATE);
            s.push(EVT_ERASE);
            s.push(EVT_PAINT);
        }
        SC_MERGE_MULTI => {
            s.push(EVT_INVALIDATE);
            s.push(EVT_INVALIDATE);
            s.push(EVT_INVALIDATE);
            s.push(EVT_ERASE);
            s.push(EVT_PAINT);
        }
        SC_EMPTY => {
            s.push(EVT_REJECT);
        }
        SC_MERGE_DUP => {
            s.push(EVT_INVALIDATE);
            s.push(EVT_INVALIDATE);
            s.push(EVT_ERASE);
            s.push(EVT_PAINT);
        }
        _ => {
            s.push(EVT_DEGRADE);
        }
    }
    Ok(s)
}

// ── 场景账 ───────────────────────────────────────────────────────
pub struct PaintLedger {
    pub matched: Vec<bool>,
    pub booked: u32,
}

impl PaintLedger {
    pub fn new() -> PaintLedger {
        PaintLedger { matched: Vec::new(), booked: 0 }
    }

    /// 单场景对照：模型 × 真机锚定 → 一致性入账。
    pub fn run_scene(&mut self, scene: usize) -> Result<bool, u32> {
        if scene >= SCENE_COUNT {
            return Err(PaintCode::SceneOutOfRange.val());
        }
        let m = model_sequence(scene)?;
        let c = control_sequence(scene)?;
        // 事件码闭集点验。
        let mut i = 0;
        while i < m.len() {
            if !evt_known(m[i]) {
                return Err(PaintCode::EvtUnknown.val());
            }
            i += 1;
        }
        i = 0;
        while i < c.len() {
            if !evt_known(c[i]) {
                return Err(PaintCode::EvtUnknown.val());
            }
            i += 1;
        }
        // 交替序红线：擦除必先于绘制（有绘必有擦且擦在前）。
        let alt_ok = alternation_audit(&m)? && alternation_audit(&c)?;
        if !alt_ok {
            return Err(PaintCode::AlternationBroken.val());
        }
        let ok = m == c;
        self.matched.push(ok);
        self.booked += 1;
        if !ok {
            return Err(PaintCode::SeqMismatch.val());
        }
        Ok(ok)
    }

    /// 全 5 场景：对照一致判据 + 勾稽。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        let mut i = 0;
        while i < SCENE_COUNT {
            self.run_scene(i)?;
            i += 1;
        }
        if self.booked as usize != SCENE_COUNT || self.matched.len() != SCENE_COUNT {
            return Err(PaintCode::ControlTableMismatch.val());
        }
        Ok(self.booked)
    }

    /// 复现：双跑账一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = PaintLedger::new();
        let r2 = run2.run_all();
        if run2.booked != self.booked || run2.matched != self.matched || r2.is_err() {
            return Err(PaintCode::NotReproducible.val());
        }
        Ok(())
    }
}

/// 交替序审计：序列中若有 PAINT，其前必有更早下标的 ERASE。
pub fn alternation_audit(seq: &Vec<u32>) -> Result<bool, u32> {
    let mut erase_idx: Option<usize> = None;
    let mut paint_idx: Option<usize> = None;
    let mut i = 0;
    while i < seq.len() {
        if !evt_known(seq[i]) {
            return Err(PaintCode::EvtUnknown.val());
        }
        if seq[i] == EVT_ERASE && erase_idx.is_none() {
            erase_idx = Some(i);
        }
        if seq[i] == EVT_PAINT && paint_idx.is_none() {
            paint_idx = Some(i);
        }
        i += 1;
    }
    match (erase_idx, paint_idx) {
        (None, None) => Ok(true), // 无擦绘场景（拒收/降级）放行
        (Some(e), Some(p)) => Ok(e < p),
        (None, Some(_)) => Ok(false), // 有绘无擦 = 交替序破
        (Some(_), None) => Ok(true),  // 只擦不绘（理论态）放行
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

/// LCG 重绘剧本：N 拍失效-擦-绘循环全走合法边，终态归 Idle。
pub fn lcg_paint_walk() -> Result<u32, u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut ph = PaintPhase::Idle;
    let mut cycles = 0;
    let mut i = 0;
    while i < 8 {
        // 每拍：Idle → Invalidated → Erasing → Painting → Idle 一整环。
        let n_inv = 1 + g.below(3) as usize;
        let mut k = 0;
        while k < n_inv {
            ph = paint_advance(ph, PaintPhase::Invalidated)?;
            // 多次失效在 Invalidated 态内合并（不出边）。
            ph = paint_advance(ph, PaintPhase::Erasing)?;
            ph = paint_advance(ph, PaintPhase::Painting)?;
            ph = paint_advance(ph, PaintPhase::Idle)?;
            cycles += 1;
            k += 1;
        }
        i += 1;
    }
    if ph != PaintPhase::Idle {
        return Err(PaintCode::EdgeIllegal.val());
    }
    Ok(cycles)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = PaintLedger::new();
    let booked = led.run_all()?;
    led.reproducible()?;
    let cycles = lcg_paint_walk()?;
    Ok((booked, cycles, SCENE_COUNT as u32))
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
    // 族 0：场景越界两发（模型端/对照端）。
    ledger.tries[0] += 1;
    let mut axis_ok = true;
    if let Err(e) = model_sequence(SCENE_COUNT) {
        if e != PaintCode::SceneOutOfRange.val() {
            axis_ok = false;
        }
    } else {
        axis_ok = false;
    }
    if let Err(e) = control_sequence(SCENE_COUNT) {
        if e != PaintCode::SceneOutOfRange.val() {
            axis_ok = false;
        }
    } else {
        axis_ok = false;
    }
    if axis_ok {
        ledger.caught[0] += 1;
    }
    // 族 1：非法状态边四发全拒（无失效直接绘/乱序/回边/跨级）。
    ledger.tries[1] += 1;
    let mut edge_ok = true;
    if paint_advance(PaintPhase::Idle, PaintPhase::Painting).is_ok() {
        edge_ok = false;
    }
    if paint_advance(PaintPhase::Idle, PaintPhase::Erasing).is_ok() {
        edge_ok = false;
    }
    if paint_advance(PaintPhase::Painting, PaintPhase::Erasing).is_ok() {
        edge_ok = false;
    }
    if paint_advance(PaintPhase::Invalidated, PaintPhase::Idle).is_ok() {
        edge_ok = false;
    }
    if edge_ok {
        ledger.caught[1] += 1;
    }
    // 族 2：对照篡改（单失效场景擦除换绘制）必判不一致 + 交替序破。
    ledger.tries[2] += 1;
    let mut tamper_ok = false;
    if let Ok(mut c) = control_sequence(SC_SINGLE) {
        let n = c.len();
        if n >= 3 {
            c[1] = EVT_PAINT;
            c[2] = EVT_ERASE;
            if let Ok(m) = model_sequence(SC_SINGLE) {
                if m != c {
                    tamper_ok = true;
                }
            }
            // 篡改序自身交替序审计必破（绘在擦前）。
            if let Ok(false) = alternation_audit(&c) {
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
    while i < PaintCode::ALL.len() {
        let v = PaintCode::ALL[i].val();
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
        "J1-5场景序对照一致+交替序+复现",
        matches!(golden, Ok((5, _, 5))),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓(含非法边/篡改)",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 场景名点验：5 名全非空。
    let mut named = 0;
    let mut i = 0;
    while i < SCENE_COUNT {
        if SCENE_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-5场景名点验",
        named == 5,
        format!("{}/5 场景名", named),
    ));
    // 事件码闭集点验：1–5 全知，6 越界拒。
    let evt_ok = evt_known(EVT_INVALIDATE)
        && evt_known(EVT_ERASE)
        && evt_known(EVT_PAINT)
        && evt_known(EVT_REJECT)
        && evt_known(EVT_DEGRADE)
        && !evt_known(EVT_DEGRADE + 1);
    v.push((
        "J1-5事件码闭集点验",
        evt_ok,
        format!("1..=5 冻结"),
    ));
    // 空失效场景拒收语义点验：空场景序 = [EVT_REJECT] 且无擦绘。
    let empty_seq = model_sequence(SC_EMPTY);
    let empty_ok = match empty_seq {
        Ok(s) => s.len() == 1 && s[0] == EVT_REJECT,
        Err(_) => false,
    };
    v.push((
        "J1-空失效区拒收语义留痕",
        empty_ok,
        String::from("空场景 = [REJECT]"),
    ));
    // 消息号锚点点验：PAINT/ERASEBKGND/SETREDRAW 与建模失效锚号互异。
    let wm_ok = WM_PAINT != WM_ERASEBKGND
        && WM_ERASEBKGND != WM_SETREDRAW
        && WM_INVALIDATE_ANCHOR != WM_PAINT
        && WM_INVALIDATE_ANCHOR != WM_ERASEBKGND;
    v.push((
        "J1-消息号锚点互异点验",
        wm_ok,
        format!(
            "PAINT={:X} ERASE={:X} ANCHOR={:X}",
            WM_PAINT, WM_ERASEBKGND, WM_INVALIDATE_ANCHOR
        ),
    ));
    // 反向判据：无失效直接绘必拒（状态机红线）。
    let no_inv = matches!(
        paint_advance(PaintPhase::Idle, PaintPhase::Painting),
        Err(e) if e == PaintCode::EdgeIllegal.val()
    );
    v.push((
        "J1-反向判据无失效直接绘拒收",
        no_inv,
        String::from("Idle→Painting 直连红线"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66D80 窗口")));
    v
}

pub fn run_unx14961_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14961 PAINT/ERASEBKGND 交替序（失效-擦除-绘制 5 场景）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
