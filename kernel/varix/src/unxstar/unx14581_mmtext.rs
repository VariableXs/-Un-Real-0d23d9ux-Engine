//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14581 · MM_TEXT 像素模式语义 · 0x668C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥312 行；
//!   ② UNX-F14581-J1 Y 轴向下恒等换算账与基准机一致，默认原点/范围账
//!      一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - MM_TEXT 语义钉版：单位=像素、+X 向右、+Y 向下、窗口与视口
//!     默认原点 (0,0)、默认窗口/视口范围均为 (1,1)；
//!   - 逻辑→设备换算账：device = logical×extV/extW + orgV - orgW×extV/extW，
//!     默认参数下退化为恒等（逐点对拍）；
//!   - Y 轴向下账：同一逻辑 Y 增大 → 设备 Y 增大（与 MM_LOMETRIC 等
//!     上向模式反向），反向判据：向上模式账必检出差异；
//!   - 非 (1,1) 范围 / 越界原点换算拒收面；
//!   - LCG 种子写死 = SEG_BASE（0x668C0），禁 rand/浮点/IO/墙钟；
//!     整数换算（全整型，无浮点）；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x668C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x668C0;

// ── 映射模式闭集（对照引用，仅 MM_TEXT 受测） ────────────────────
pub const MM_TEXT: usize = 1;
pub const MM_LOMETRIC: usize = 2;
pub const MM_MODE_COUNT: usize = 2;
pub const MM_MODE_NAMES: [&str; MM_MODE_COUNT] = ["MM_TEXT", "MM_LOMETRIC"];

// ── 写死默认账（判据独立，不复用被测值） ─────────────────────────
/// 默认视口原点 (0,0)；默认窗口原点 (0,0)；默认视口范围 (1,1)；
/// 默认窗口范围 (1,1)。来源：GDI 映射模式默认值钉版。
pub const DEF_VORG: (i32, i32) = (0, 0);
pub const DEF_WORG: (i32, i32) = (0, 0);
pub const DEF_VEXT: (i32, i32) = (1, 1);
pub const DEF_WEXT: (i32, i32) = (1, 1);
/// 坐标域上限（16bit 有符号域，写死）。
pub const COORD_MAX: i32 = 32767;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MmCode {
    /// 模式越闭集
    ModeOutOfRange,
    /// 范围为零或负
    ExtentInvalid,
    /// 坐标越 16bit 域
    CoordOverflow,
    /// 恒等换算账与基准机不一致
    IdentityMismatch,
    /// Y 轴向下账与基准机不一致
    YDownMismatch,
    /// 默认原点/范围账不符
    DefaultMismatch,
    /// 原点平移账破（平移后恒等性未保持相对量）
    ShiftBroken,
    /// 换算账自反性破（往返不等）
    RoundTripBroken,
}

impl MmCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MmCode::ModeOutOfRange => "映射模式越闭集",
            MmCode::ExtentInvalid => "范围为零或负：拒收",
            MmCode::CoordOverflow => "坐标越 16bit 域",
            MmCode::IdentityMismatch => "恒等换算账与基准机不一致",
            MmCode::YDownMismatch => "Y 轴向下账与基准机不一致",
            MmCode::DefaultMismatch => "默认原点/范围账不符",
            MmCode::ShiftBroken => "原点平移账破",
            MmCode::RoundTripBroken => "换算往返自反性破",
        }
    }
    pub const ALL: [MmCode; 8] = [
        MmCode::ModeOutOfRange,
        MmCode::ExtentInvalid,
        MmCode::CoordOverflow,
        MmCode::IdentityMismatch,
        MmCode::YDownMismatch,
        MmCode::DefaultMismatch,
        MmCode::ShiftBroken,
        MmCode::RoundTripBroken,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MmCode::ALL.len() {
        if MmCode::ALL[i].val() == code {
            return MmCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x668C0 段闭集内"
}

// ── 视口/窗口映射账 ──────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mapping {
    pub mode: usize,
    pub vorg: (i32, i32),
    pub worg: (i32, i32),
    pub vext: (i32, i32),
    pub wext: (i32, i32),
}

impl Mapping {
    /// 默认 MM_TEXT 账。
    pub fn mm_text_default() -> Mapping {
        Mapping { mode: MM_TEXT, vorg: DEF_VORG, worg: DEF_WORG, vext: DEF_VEXT, wext: DEF_WEXT }
    }

    pub fn validate(&self) -> Result<(), u32> {
        if self.mode != MM_TEXT && self.mode != MM_LOMETRIC {
            return Err(MmCode::ModeOutOfRange.val());
        }
        if self.vext.0 == 0 || self.vext.1 == 0 || self.wext.0 == 0 || self.wext.1 == 0 {
            return Err(MmCode::ExtentInvalid.val());
        }
        Ok(())
    }

    fn in_domain(p: i32) -> Result<i32, u32> {
        if p < -COORD_MAX || p > COORD_MAX {
            return Err(MmCode::CoordOverflow.val());
        }
        Ok(p)
    }

    /// 逻辑→设备换算：device = (logical - worg) * vext / wext + vorg。
    /// 全整型有理化（MM_TEXT 默认 ext 全 1 → 恒等）。
    pub fn l2d(&self, lx: i32, ly: i32) -> Result<(i32, i32), u32> {
        self.validate()?;
        let wx = Self::in_domain(lx)?;
        let wy = Self::in_domain(ly)?;
        let dx = (wx - self.worg.0) * self.vext.0 / self.wext.0 + self.vorg.0;
        let dy = (wy - self.worg.1) * self.vext.1 / self.wext.1 + self.vorg.1;
        Ok((Self::in_domain(dx)?, Self::in_domain(dy)?))
    }

    /// 设备→逻辑换算（逆算，供往返自反性对账）。
    pub fn d2l(&self, dx: i32, dy: i32) -> Result<(i32, i32), u32> {
        self.validate()?;
        let lx = (dx - self.vorg.0) * self.wext.0 / self.vext.0 + self.worg.0;
        let ly = (dy - self.vorg.1) * self.wext.1 / self.vext.1 + self.worg.1;
        Ok((Self::in_domain(lx)?, Self::in_domain(ly)?))
    }

    /// Y 轴向下账：同 X 下，Y 增 → 设备 Y 不减（MM_TEXT）。
    pub fn y_down_invariant(&self) -> Result<(), u32> {
        if self.mode != MM_TEXT {
            return Err(MmCode::YDownMismatch.val());
        }
        let a = self.l2d(100, 10)?;
        let b = self.l2d(100, 50)?;
        if b.1 < a.1 {
            return Err(MmCode::YDownMismatch.val());
        }
        // 严格增大（默认恒等账下）。
        if b.1 <= a.1 {
            return Err(MmCode::YDownMismatch.val());
        }
        Ok(())
    }
}

// ── 恒等换算账（默认参数逐点对拍） ───────────────────────────────
/// 基准机恒等换算表（写死：默认参数下 (lx,ly)→(lx,ly)）。
pub const BASELINE_IDENTITY: [(i32, i32); 8] = [
    (0, 0), (1, 1), (-1, -1), (100, 50), (-200, 320), (7, -9), (32767, 0), (-32768 / 2, 12345),
];

pub fn identity_parity() -> Result<usize, u32> {
    let m = Mapping::mm_text_default();
    let mut i = 0;
    while i < BASELINE_IDENTITY.len() {
        let (x, y) = BASELINE_IDENTITY[i];
        let d = m.l2d(x, y)?;
        if d != (x, y) {
            return Err(MmCode::IdentityMismatch.val());
        }
        i += 1;
    }
    Ok(BASELINE_IDENTITY.len())
}

// ── Y 轴向下对拍（含反向：LOMETRIC 上向账必检出） ────────────────
/// 基准机 Y 账（写死）：MM_TEXT 下 (0,10)→(0,10)、(0,50)→(0,50)，
/// 50 在 10 之下（更大）；MM_LOMETRIC 上向（0.1mm 单位），此处仅
/// 取反向语义对照：同点对 Y 方向必须相反。
pub fn y_axis_parity() -> Result<(), u32> {
    let m = Mapping::mm_text_default();
    let a = m.l2d(0, 10)?;
    let b = m.l2d(0, 50)?;
    if a != (0, 10) || b != (0, 50) {
        return Err(MmCode::YDownMismatch.val());
    }
    m.y_down_invariant()?;
    Ok(())
}

/// 反向判据：把映射账仿造成"上向"（vext.y 为负）必须被 ExtentInvalid/
/// YDown 检出——负范围即上向模式，MM_TEXT 账下拒收。
pub fn y_up_detected() -> bool {
    let mut m = Mapping::mm_text_default();
    m.vext = (1, -1);
    match m.validate() {
        Err(_) => true,
        Ok(()) => m.y_down_invariant().is_err(),
    }
}

// ── 原点平移账 + 往返自反 ────────────────────────────────────────
/// 平移对拍表（写死：域内安全点，平移 +10/+20 不越 16bit 域）。
pub const SHIFT_TABLE: [(i32, i32); 6] = [(0, 0), (1, 1), (-5, -9), (100, 50), (-200, 320), (7, -9)];

pub fn shift_and_roundtrip() -> Result<(usize, usize), u32> {
    let mut m = Mapping::mm_text_default();
    // 平移原点：vorg=(10,20), worg=(0,0) → device = logical + (10,20)。
    m.vorg = (10, 20);
    let mut ok_shift = 0;
    let mut i = 0;
    while i < SHIFT_TABLE.len() {
        let (x, y) = SHIFT_TABLE[i];
        let d = m.l2d(x, y)?;
        if d != (x + 10, y + 20) {
            return Err(MmCode::ShiftBroken.val());
        }
        ok_shift += 1;
        i += 1;
    }
    // 往返自反：默认账 l2d→d2l 恒等。
    let m0 = Mapping::mm_text_default();
    let mut ok_rt = 0;
    i = 0;
    while i < BASELINE_IDENTITY.len() {
        let (x, y) = BASELINE_IDENTITY[i];
        let d = m0.l2d(x, y)?;
        let back = m0.d2l(d.0, d.1)?;
        if back != (x, y) {
            return Err(MmCode::RoundTripBroken.val());
        }
        ok_rt += 1;
        i += 1;
    }
    Ok((ok_shift, ok_rt))
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

// ── 随机域内恒等抽查 ─────────────────────────────────────────────
pub fn random_identity(spots: usize) -> Result<usize, u32> {
    let m0 = Mapping::mm_text_default();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut ok = 0;
    let mut i = 0;
    while i < spots {
        let x = lcg.below(2 * COORD_MAX as u64) as i32 - COORD_MAX;
        let y = lcg.below(2 * COORD_MAX as u64) as i32 - COORD_MAX;
        let d = m0.l2d(x, y)?;
        if d != (x, y) {
            return Err(MmCode::IdentityMismatch.val());
        }
        ok += 1;
        i += 1;
    }
    Ok(ok)
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
    // 族 0：范围为零必拒。
    ledger.tries[0] += 1;
    let mut m = Mapping::mm_text_default();
    m.wext = (1, 0);
    if let Err(e) = m.validate() {
        if e == MmCode::ExtentInvalid.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：坐标越 16bit 域必拒。
    ledger.tries[1] += 1;
    let m0 = Mapping::mm_text_default();
    if let Err(e) = m0.l2d(COORD_MAX + 1, 0) {
        if e == MmCode::CoordOverflow.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：模式越闭集必拒。
    ledger.tries[2] += 1;
    let bad = Mapping { mode: 9, vorg: (0, 0), worg: (0, 0), vext: (1, 1), wext: (1, 1) };
    if let Err(e) = bad.validate() {
        if e == MmCode::ModeOutOfRange.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let id = identity_parity()?;
    y_axis_parity()?;
    let (sh, rt) = shift_and_roundtrip()?;
    let rnd = random_identity(64)?;
    Ok((id, sh + rt, rnd))
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MmCode::ALL.len() {
        let v = MmCode::ALL[i].val();
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
        "J1-恒等8点+平移/往返16点+随机64点",
        matches!(golden, Ok((8, 16, 64))),
        format!("{:?}", golden),
    ));
    // 默认原点/范围账（逐字段比对写死常量）。
    let m = Mapping::mm_text_default();
    let def_ok = m.vorg == (0, 0) && m.worg == (0, 0) && m.vext == (1, 1) && m.wext == (1, 1);
    v.push((
        "J1-默认原点(0,0)/范围(1,1)账",
        def_ok,
        format!("{:?}", (m.vorg, m.worg, m.vext, m.wext)),
    ));
    v.push((
        "J1-反向：上向负范围必检出",
        y_up_detected(),
        String::from("vext.y=-1 检出"),
    ));
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    let mut ok = 0;
    let mut i = 0;
    while i < MM_MODE_COUNT {
        if MM_MODE_NAMES[i].len() > 0 {
            ok += 1;
        }
        i += 1;
    }
    v.push(("J1-模式名点验", ok == 2, format!("{}/2", ok)));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x668C0 窗口")));
    v
}

pub fn run_unx14581_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14581 MM_TEXT 像素模式语义");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
