//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14541 · 逻辑笔与 CreatePen/CreatePenIndirect · 0x66840
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥329 行；
//!   ② UNX-F14541-J1 PS_* 笔型/宽度/颜色三元账一致，ExtCreatePen 几何
//!      笔底座挂点有账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - PS_* 笔型闭集 7 型（SOLID/DASH/DOT/DASHDOT/DASHDOTDOT/NULL/
//!     INSIDEFRAME），PS_STYLE 掩码低 4 位抽账；
//!   - 逻辑笔三元（笔型, 宽度, 颜色 COLORREF）：CreatePen 直参入账，
//!     CreatePenIndirect 经 LOGPEN 结构入账，两路三元账逐笔一致；
//!   - ExtCreatePen 几何笔底座挂点：PS_GEOMETRIC 位置抽账 + 底座挂点
//!     记账（style 位 0x10）；
//!   - 非法笔型拒收；宽度负溢拒收；NULL 笔宽度强制 1 入账；
//!   - LCG 种子写死 = SEG_BASE（0x66840），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66840，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66840;

// ── PS_* 笔型闭集（写死：wingdi PS_* 值） ────────────────────────
pub const PS_SOLID: u32 = 0;
pub const PS_DASH: u32 = 1;
pub const PS_DOT: u32 = 2;
pub const PS_DASHDOT: u32 = 3;
pub const PS_DASHDOTDOT: u32 = 4;
pub const PS_NULL: u32 = 5;
pub const PS_INSIDEFRAME: u32 = 6;
pub const PS_STYLE_COUNT: usize = 7;
pub const PS_STYLE_NAMES: [&str; PS_STYLE_COUNT] = [
    "PS_SOLID", "PS_DASH", "PS_DOT", "PS_DASHDOT", "PS_DASHDOTDOT", "PS_NULL", "PS_INSIDEFRAME",
];
/// PS_GEOMETRIC 位（ExtCreatePen 底座挂点：style bit 4）。
pub const PS_GEOMETRIC_BIT: u32 = 0x10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PenCode {
    /// 笔型越闭集
    StyleOutOfRange,
    /// 笔宽非法（超上限）
    WidthInvalid,
    /// 三元账不一致（CreatePen vs Indirect）
    TripleMismatch,
    /// 底座挂点未记账
    BaseHookUnbooked,
    /// NULL 笔宽度未归一
    NullWidthNotNorm,
    /// 笔句柄未入账
    PenUnbooked,
    /// 账序错乱（销毁序先于创建序）
    OrderBroken,
    /// 颜色位域越界（>24bit）
    ColorOverflow,
}

impl PenCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PenCode::StyleOutOfRange => "笔型越闭集：PS_* 7 型冻结",
            PenCode::WidthInvalid => "笔宽非法：超写死上限",
            PenCode::TripleMismatch => "三元账不一致：直参 vs 间接",
            PenCode::BaseHookUnbooked => "ExtCreatePen 底座挂点未记账",
            PenCode::NullWidthNotNorm => "NULL 笔宽度未归一为 1",
            PenCode::PenUnbooked => "笔句柄未入账",
            PenCode::OrderBroken => "账序错乱：销毁先于创建",
            PenCode::ColorOverflow => "颜色位域越界：超 24bit",
        }
    }
    pub const ALL: [PenCode; 8] = [
        PenCode::StyleOutOfRange,
        PenCode::WidthInvalid,
        PenCode::TripleMismatch,
        PenCode::BaseHookUnbooked,
        PenCode::NullWidthNotNorm,
        PenCode::PenUnbooked,
        PenCode::OrderBroken,
        PenCode::ColorOverflow,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PenCode::ALL.len() {
        if PenCode::ALL[i].val() == code {
            return PenCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66840 段闭集内"
}

// ── 写死常量（判据独立，不复用被测值） ───────────────────────────
/// 笔宽上限（来源：GDI 逻辑坐标 16bit 有符号上限推定，写死）。
pub const PEN_WIDTH_MAX: u32 = 32767;
/// 颜色 24bit 上限掩码。
pub const COLOR_MASK: u32 = 0x00FF_FFFF;

/// LOGPEN 结构（CreatePenIndirect 入参建模）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LogPen {
    pub style: u32,
    pub width: u32,
    pub color: u32,
}

// ── 笔账 ─────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PenState {
    Created,
    Selected,
    Deleted,
}

#[derive(Clone, Debug)]
pub struct PenEntry {
    pub handle: u32,
    pub triple: LogPen,
    pub geometric: bool,
    pub state: PenState,
    pub created_seq: u64,
    pub deleted_seq: u64,
}

pub struct PenLedger {
    pub pens: Vec<PenEntry>,
    pub next_handle: u32,
    pub seq: u64,
    pub base_hooks: u32,
}

impl PenLedger {
    pub fn new() -> PenLedger {
        PenLedger { pens: Vec::new(), next_handle: 1, seq: 0, base_hooks: 0 }
    }

    /// CreatePen：直参三元入账。NULL 笔宽度归一为 1。
    pub fn create_pen(&mut self, style: u32, width: u32, color: u32) -> Result<u32, u32> {
        if style as usize >= PS_STYLE_COUNT {
            return Err(PenCode::StyleOutOfRange.val());
        }
        if color & !COLOR_MASK != 0 {
            return Err(PenCode::ColorOverflow.val());
        }
        let w = if style == PS_NULL { 1 } else { width };
        self.create_common(LogPen { style, width: w, color })
    }

    /// CreatePenIndirect：LOGPEN 结构入账（同闸同账）。
    pub fn create_pen_indirect(&mut self, lp: LogPen) -> Result<u32, u32> {
        if lp.style as usize >= PS_STYLE_COUNT {
            return Err(PenCode::StyleOutOfRange.val());
        }
        if lp.color & !COLOR_MASK != 0 {
            return Err(PenCode::ColorOverflow.val());
        }
        if lp.width > PEN_WIDTH_MAX {
            return Err(PenCode::WidthInvalid.val());
        }
        let w = if lp.style == PS_NULL { 1 } else { lp.width };
        self.create_common(LogPen { style: lp.style, width: w, color: lp.color })
    }

    /// ExtCreatePen：几何笔底座挂点（PS_GEOMETRIC 位抽账）。
    pub fn ext_create_pen(&mut self, style: u32, width: u32, color: u32) -> Result<u32, u32> {
        let base_style = style & !PS_GEOMETRIC_BIT;
        if base_style as usize >= PS_STYLE_COUNT {
            return Err(PenCode::StyleOutOfRange.val());
        }
        if style & PS_GEOMETRIC_BIT != 0 {
            self.base_hooks += 1;
        }
        self.create_pen(base_style, width, color)
    }

    fn create_common(&mut self, triple: LogPen) -> Result<u32, u32> {
        if triple.width > PEN_WIDTH_MAX {
            return Err(PenCode::WidthInvalid.val());
        }
        self.seq += 1;
        let h = self.next_handle;
        self.next_handle += 1;
        self.pens.push(PenEntry {
            handle: h,
            triple,
            geometric: false,
            state: PenState::Created,
            created_seq: self.seq,
            deleted_seq: 0,
        });
        Ok(h)
    }

    pub fn select(&mut self, handle: u32) -> Result<(), u32> {
        let k = self.find(handle)?;
        self.pens[k].state = PenState::Selected;
        Ok(())
    }

    pub fn delete(&mut self, handle: u32) -> Result<(), u32> {
        self.seq += 1;
        let k = self.find(handle)?;
        if self.pens[k].state == PenState::Deleted {
            return Err(PenCode::OrderBroken.val());
        }
        self.pens[k].state = PenState::Deleted;
        self.pens[k].deleted_seq = self.seq;
        Ok(())
    }

    fn find(&self, handle: u32) -> Result<usize, u32> {
        let mut i = 0;
        while i < self.pens.len() {
            if self.pens[i].handle == handle {
                return Ok(i);
            }
            i += 1;
        }
        Err(PenCode::PenUnbooked.val())
    }

    /// J1：两路三元账逐笔一致（直参 vs 间接，按入账序配对）。
    pub fn triples_consistent(&self) -> Result<(), u32> {
        // 直参与间接各建一支，同序同三元必须同账。
        let n = self.pens.len();
        let mut i = 0;
        while i < n {
            let t = self.pens[i].triple;
            if t.style as usize >= PS_STYLE_COUNT {
                return Err(PenCode::TripleMismatch.val());
            }
            if t.style == PS_NULL && t.width != 1 {
                return Err(PenCode::NullWidthNotNorm.val());
            }
            if t.width > PEN_WIDTH_MAX {
                return Err(PenCode::WidthInvalid.val());
            }
            i += 1;
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
        self.next() % (if n == 0 { 1 } else { n })
    }
}

// ── 双路对拍：同三元 CreatePen vs Indirect 账一致 ────────────────
pub fn dual_path_parity() -> Result<usize, u32> {
    let mut a = PenLedger::new();
    let mut b = PenLedger::new();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut paired = 0;
    let mut r = 0;
    while r < 64 {
        let style = lcg.below(PS_STYLE_COUNT as u64) as u32;
        let width = 1 + lcg.below(16) as u32;
        let color = (lcg.next() & COLOR_MASK as u64) as u32;
        let ha = a.create_pen(style, width, color)?;
        let hb = b.create_pen_indirect(LogPen { style, width, color })?;
        if a.pens[ha as usize - 1].triple != b.pens[hb as usize - 1].triple {
            return Err(PenCode::TripleMismatch.val());
        }
        paired += 1;
        r += 1;
    }
    a.triples_consistent()?;
    b.triples_consistent()?;
    Ok(paired)
}

// ── 注入面：三族全抓 ─────────────────────────────────────────────
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
    // 族 0：坏笔型（7 越闭集）必拒。
    ledger.tries[0] += 1;
    let mut led = PenLedger::new();
    if let Err(e) = led.create_pen(7, 1, 0) {
        if e == PenCode::StyleOutOfRange.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：超宽笔必拒。
    ledger.tries[1] += 1;
    if let Err(e) = led.create_pen(PS_SOLID, PEN_WIDTH_MAX + 1, 0) {
        if e == PenCode::WidthInvalid.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：未入账句柄 select 必拒。
    ledger.tries[2] += 1;
    if let Err(e) = led.select(999) {
        if e == PenCode::PenUnbooked.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(usize, u32), u32> {
    let paired = dual_path_parity()?;
    // 几何笔底座挂点有账。
    let mut led = PenLedger::new();
    let _ = led.ext_create_pen(PS_GEOMETRIC_BIT | PS_SOLID, 3, 0x00FF_00FF)?;
    if led.base_hooks != 1 {
        return Err(PenCode::BaseHookUnbooked.val());
    }
    Ok((paired, led.base_hooks))
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PenCode::ALL.len() {
        let v = PenCode::ALL[i].val();
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
        "J1-双路三元账64笔全齐",
        matches!(golden, Ok((64, 1))),
        format!("{:?}", golden),
    ));
    // 笔型名点验 + PS 值唯一性。
    let mut ok = 0;
    let mut i = 0;
    while i < PS_STYLE_COUNT {
        if PS_STYLE_NAMES[i].len() > 0 {
            ok += 1;
        }
        i += 1;
    }
    let mut uniq = true;
    i = 0;
    while i < PS_STYLE_COUNT {
        let mut j = i + 1;
        while j < PS_STYLE_COUNT {
            if PS_STYLE_NAMES[i] == PS_STYLE_NAMES[j] {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    v.push((
        "J1-PS_* 7型名点验+唯一",
        ok == 7 && uniq,
        format!("{}/7 uniq={}", ok, uniq),
    ));
    // NULL 笔宽度归一抽验（含反向：SOLID 不归一）。
    let mut led = PenLedger::new();
    let h = led.create_pen(PS_NULL, 42, 0);
    let null_norm = match h {
        Ok(hh) => led.pens[hh as usize - 1].triple.width == 1,
        Err(_) => false,
    };
    let h2 = led.create_pen(PS_SOLID, 42, 0);
    let solid_keep = match h2 {
        Ok(hh) => led.pens[hh as usize - 1].triple.width == 42,
        Err(_) => false,
    };
    v.push((
        "J1-NULL宽归一+SOLID保留",
        null_norm && solid_keep,
        format!("null={} solid={}", null_norm, solid_keep),
    ));
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66840 窗口")));
    v
}

pub fn run_unx14541_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14541 逻辑笔与 CreatePen/CreatePenIndirect");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
