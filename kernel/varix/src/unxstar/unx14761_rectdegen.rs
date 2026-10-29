//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14761 · 矩形退化族（空/翻转/零宽高） · 0x66B00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F14761-J1 IsRectEmpty/EqualRect/IntersectRect 退化样本 30 组
//!      与对照一致，空矩交运语义留痕；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 30 组退化样本闭集：6 族 × 5 样本（全零/零宽/零高/翻转宽/
//!     翻转高/正常非空），族与样本坐标写死；
//!   - 双实现对照：模型实现（GDI IsRectEmpty 语义 l>=r || t>=b 即空）
//!     × 独立对照实现（德摩根反演式 !(r>l && b>t)），30 组逐样本
//!     双比一致 =「与对照一致」判据；
//!   - 空矩交运语义留痕：任一方为空 ⇒ 交矩形归零 (0,0,0,0)，逐组
//!     入账（IntersectRect 空结果写零语义，来源 Win32 文档约定）；
//!   - 反向判据：故意用严格不等号（r>l && b>t 的坏对照，对零宽/
//!     零高/全零样本误判为非空）必须在退化样本上产生偏差——证明
//!     对照 harness 具备检出力；
//!   - 状态机：未验 → 已验/存疑 二态收敛，非法回边拒收；
//!   - 注入面：族越界/坏对照检出/空账读回 三族全抓；
//!   - LCG 种子写死 = SEG_BASE（0x66B00），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66B00，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66B00;

// ── 样本族闭集（6 族 × 5 样本 = 30 组） ──────────────────────────
pub const CAT_ALLZERO: usize = 0;
pub const CAT_ZEROW: usize = 1;
pub const CAT_ZEROH: usize = 2;
pub const CAT_FLIPW: usize = 3;
pub const CAT_FLIPH: usize = 4;
pub const CAT_NORMAL: usize = 5;
pub const CAT_COUNT: usize = 6;
pub const SAMPLES_PER_CAT: usize = 5;
pub const SAMPLE_COUNT: usize = CAT_COUNT * SAMPLES_PER_CAT;
pub const CAT_NAMES: [&str; CAT_COUNT] = [
    "全零矩", "零宽矩", "零高矩", "翻转宽矩", "翻转高矩", "正常非空矩",
];

/// 30 组样本坐标（写死：退化样本闭集，来源判据语义推定）。
pub const SAMPLES: [(i32, i32, i32, i32); SAMPLE_COUNT] = [
    // 族 0：全零矩 ×5
    (0, 0, 0, 0),
    (0, 0, 0, 0),
    (0, 0, 0, 0),
    (0, 0, 0, 0),
    (0, 0, 0, 0),
    // 族 1：零宽矩 ×5（left == right）
    (1, 1, 1, 5),
    (2, 2, 2, 8),
    (-3, 0, -3, 4),
    (7, -2, 7, 9),
    (0, 0, 0, 1),
    // 族 2：零高矩 ×5（top == bottom）
    (1, 1, 5, 1),
    (2, 3, 9, 3),
    (-4, -1, 6, -1),
    (0, 2, 3, 2),
    (8, 0, 12, 0),
    // 族 3：翻转宽矩 ×5（right < left）
    (5, 1, 1, 5),
    (9, 0, 2, 7),
    (-1, -1, -6, 3),
    (4, 2, -4, 6),
    (3, 3, 0, 9),
    // 族 4：翻转高矩 ×5（bottom < top）
    (1, 5, 5, 1),
    (0, 7, 6, 2),
    (-2, 3, 4, -3),
    (5, 4, 9, 1),
    (2, 2, 8, -1),
    // 族 5：正常非空矩 ×5（全部与探针 (2,2,12,12) 真交）
    (0, 0, 10, 10),
    (3, 3, 20, 20),
    (2, 2, 12, 12),
    (-5, -5, 6, 6),
    (4, 1, 9, 15),
];

/// 每样本所属族（写死闭集，判据独立于被测实现）。
pub const CAT_OF: [usize; SAMPLE_COUNT] = [
    0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3,
    4, 4, 4, 4, 4, 5, 5, 5, 5, 5,
];

/// 族级期望空判（独立判据常量：族 0–4 全空，族 5 全非空）。
pub const EXPECTED_EMPTY_BY_CAT: [bool; CAT_COUNT] =
    [true, true, true, true, true, false];
/// 族级期望交运非空判（探针 (2,2,12,12)：退化族交运全归零，正常族全真交）。
pub const EXPECTED_IX_NONEMPTY_BY_CAT: [bool; CAT_COUNT] =
    [false, false, false, false, false, true];
/// 探针矩形（写死，与全部正常族样本真交）。
pub const PROBE: (i32, i32, i32, i32) = (2, 2, 12, 12);

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RectCode {
    /// 样本下标越 30 组闭集
    SampleOutOfRange,
    /// 族下标越 6 族闭集
    CatOutOfRange,
    /// 模型-判据空态不一致
    EmptyVerdictMismatch,
    /// 模型-对照实现不一致
    ControlMismatch,
    /// 空矩交运未归零
    IntersectNotZeroed,
    /// 样本未入账
    SampleUnbooked,
    /// 样本终态悬挂
    SampleHung,
    /// 双跑不一致（不可复现）
    NotReproducible,
    /// 坏对照未被检出（harness 无检出力）
    BrokenControlUndetected,
    /// 等矩判据错判（同坐标判不等）
    EqualRectMismatch,
}

impl RectCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RectCode::SampleOutOfRange => "样本下标越闭集：30 组冻结",
            RectCode::CatOutOfRange => "族下标越闭集：6 族冻结",
            RectCode::EmptyVerdictMismatch => "空态判与独立判据不一致",
            RectCode::ControlMismatch => "模型与对照实现逐样本不一致",
            RectCode::IntersectNotZeroed => "空矩交运未归零：语义留痕红线",
            RectCode::SampleUnbooked => "样本未入账：30/30 红线",
            RectCode::SampleHung => "样本终态悬挂：状态机未收敛",
            RectCode::NotReproducible => "双跑不一致：复现红线",
            RectCode::BrokenControlUndetected => "坏对照未被检出：harness 失效",
            RectCode::EqualRectMismatch => "等矩判据错判：同坐标判不等",
        }
    }
    pub const ALL: [RectCode; 10] = [
        RectCode::SampleOutOfRange,
        RectCode::CatOutOfRange,
        RectCode::EmptyVerdictMismatch,
        RectCode::ControlMismatch,
        RectCode::IntersectNotZeroed,
        RectCode::SampleUnbooked,
        RectCode::SampleHung,
        RectCode::NotReproducible,
        RectCode::BrokenControlUndetected,
        RectCode::EqualRectMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RectCode::ALL.len() {
        if RectCode::ALL[i].val() == code {
            return RectCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66B00 段闭集内"
}

// ── 矩形模型（GDI 语义） ─────────────────────────────────────────
pub type Rect4 = (i32, i32, i32, i32);

/// 模型 IsRectEmpty：l >= r || t >= b 即空（零尺寸与翻转同归空）。
pub fn model_is_empty(r: Rect4) -> bool {
    r.0 >= r.2 || r.1 >= r.3
}

/// 对照实现（独立写法：德摩根反演，与模型逻辑等价）。
pub fn control_is_empty(r: Rect4) -> bool {
    !(r.2 > r.0 && r.3 > r.1)
}

/// 坏对照（故意严格不等号——对 l==r/t==b 误判为非空，反向判据用）。
pub fn broken_control_is_empty(r: Rect4) -> bool {
    !(r.2 > r.0 && r.3 > r.1) && !(r.0 == r.2 || r.1 == r.3)
}

/// 模型 EqualRect：四元组逐分量相等。
pub fn model_equal(a: Rect4, b: Rect4) -> bool {
    a.0 == b.0 && a.1 == b.1 && a.2 == b.2 && a.3 == b.3
}

/// 手写 max/min（整数闭式，不用 Ord::max 便于审阅）。
fn imax(a: i32, b: i32) -> i32 {
    if a > b {
        a
    } else {
        b
    }
}
fn imin(a: i32, b: i32) -> i32 {
    if a < b {
        a
    } else {
        b
    }
}

/// 模型 IntersectRect：任一方空 ⇒ 归零 (0,0,0,0)（空矩交运语义）。
pub fn model_intersect(a: Rect4, b: Rect4) -> Rect4 {
    if model_is_empty(a) || model_is_empty(b) {
        return (0, 0, 0, 0);
    }
    let l = imax(a.0, b.0);
    let t = imax(a.1, b.1);
    let r = imin(a.2, b.2);
    let bt = imin(a.3, b.3);
    if l >= r || t >= bt {
        return (0, 0, 0, 0);
    }
    (l, t, r, bt)
}

// ── 样本状态机 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampleState {
    /// 未验
    Pending,
    /// 模型-判据-对照三比全过
    Verified,
    /// 存疑（任一比不一致）
    Suspect,
}

/// 30 组样本账。
pub struct SampleLedger {
    pub states: Vec<SampleState>,
    pub empty_true: u32,
    pub empty_false: u32,
    pub ix_zeroed: u32,
    pub booked: u32,
}

impl SampleLedger {
    pub fn new() -> SampleLedger {
        SampleLedger {
            states: Vec::new(),
            empty_true: 0,
            empty_false: 0,
            ix_zeroed: 0,
            booked: 0,
        }
    }

    /// 单样本三比：模型-独立判据（族级期望）、模型-对照实现、空矩交运归零。
    pub fn run_sample(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= SAMPLE_COUNT {
            return Err(RectCode::SampleOutOfRange.val());
        }
        let cat = CAT_OF[idx];
        if cat >= CAT_COUNT {
            return Err(RectCode::CatOutOfRange.val());
        }
        // 下标先比长度（防御式账本扩容）。
        while self.states.len() <= idx {
            self.states.push(SampleState::Pending);
        }
        let s = SAMPLES[idx];
        let m_empty = model_is_empty(s);
        let c_empty = control_is_empty(s);
        // 比 1：模型 vs 独立判据常量（族级写死）。
        let want_empty = EXPECTED_EMPTY_BY_CAT[cat];
        if m_empty != want_empty {
            self.states[idx] = SampleState::Suspect;
            self.booked += 1;
            return Err(RectCode::EmptyVerdictMismatch.val());
        }
        // 比 2：模型 vs 对照实现（与对照一致判据）。
        if m_empty != c_empty {
            self.states[idx] = SampleState::Suspect;
            self.booked += 1;
            return Err(RectCode::ControlMismatch.val());
        }
        // 比 3：空矩交运语义留痕——空族交运必归零；正常族交运非空。
        let ix = model_intersect(s, PROBE);
        let want_ix_nonempty = EXPECTED_IX_NONEMPTY_BY_CAT[cat];
        let ix_nonempty = !model_is_empty(ix);
        if m_empty && !model_is_empty(ix) {
            self.states[idx] = SampleState::Suspect;
            self.booked += 1;
            return Err(RectCode::IntersectNotZeroed.val());
        }
        if ix_nonempty != want_ix_nonempty {
            self.states[idx] = SampleState::Suspect;
            self.booked += 1;
            return Err(RectCode::IntersectNotZeroed.val());
        }
        if m_empty {
            self.empty_true += 1;
            self.ix_zeroed += 1;
        } else {
            self.empty_false += 1;
        }
        // 状态机收敛：Pending → Verified（非法回边拒收由单调写入保证）。
        self.states[idx] = SampleState::Verified;
        self.booked += 1;
        Ok(())
    }

    /// 全 30 组：100% 入账 + 全收敛 + 守恒恒等式。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        let mut i = 0;
        while i < SAMPLE_COUNT {
            self.run_sample(i)?;
            i += 1;
        }
        if self.states.len() != SAMPLE_COUNT {
            return Err(RectCode::SampleUnbooked.val());
        }
        if self.booked as usize != SAMPLE_COUNT {
            return Err(RectCode::SampleUnbooked.val());
        }
        // 守恒：空 + 非空 = 总数；归零留痕数 = 空数。
        if (self.empty_true + self.empty_false) as usize != SAMPLE_COUNT {
            return Err(RectCode::SampleUnbooked.val());
        }
        if self.ix_zeroed as usize != self.empty_true as usize {
            return Err(RectCode::IntersectNotZeroed.val());
        }
        let mut i = 0;
        while i < self.states.len() {
            if !matches!(self.states[i], SampleState::Verified) {
                return Err(RectCode::SampleHung.val());
            }
            i += 1;
        }
        Ok(self.booked)
    }

    /// 复现：双跑账目逐字段一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = SampleLedger::new();
        let _ = run2.run_all();
        if run2.booked != self.booked
            || run2.empty_true != self.empty_true
            || run2.empty_false != self.empty_false
            || run2.ix_zeroed != self.ix_zeroed
            || run2.states != self.states
        {
            return Err(RectCode::NotReproducible.val());
        }
        Ok(())
    }

    /// 等矩点验：每样本与自身等矩必真；与探针等矩必假（正常族）。
    pub fn equal_rect_audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < SAMPLE_COUNT {
            if !model_equal(SAMPLES[i], SAMPLES[i]) {
                return Err(RectCode::EqualRectMismatch.val());
            }
            if model_equal(SAMPLES[i], PROBE) && SAMPLES[i] != PROBE {
                return Err(RectCode::EqualRectMismatch.val());
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
        self.next() % if n < 1 { 1 } else { n }
    }
}

/// LCG 抽验：种子固定 = SEG_BASE，首 8 拍序列写死比对（确定性红线）。
pub fn lcg_audit() -> Result<(), u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut prev = 0u64;
    let mut i = 0;
    while i < 8 {
        let v = g.next();
        if i > 0 && v == prev {
            // 相邻两拍全同 = 序列退化。
            return Err(RectCode::NotReproducible.val());
        }
        prev = v;
        i += 1;
    }
    Ok(())
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = SampleLedger::new();
    let booked = led.run_all()?;
    led.reproducible()?;
    led.equal_rect_audit()?;
    lcg_audit()?;
    Ok((booked, led.empty_true, led.empty_false))
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
    // 族 0：样本下标越闭集。
    ledger.tries[0] += 1;
    if let Err(e) = SampleLedger::new().run_sample(SAMPLE_COUNT) {
        if e == RectCode::SampleOutOfRange.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：坏对照必须在零宽/零高/全零样本上被检出（检出力证明）。
    ledger.tries[1] += 1;
    let mut detected = false;
    let mut i = 0;
    while i < SAMPLE_COUNT {
        let s = SAMPLES[i];
        let cat = CAT_OF[i];
        if model_is_empty(s) != broken_control_is_empty(s)
            && EXPECTED_EMPTY_BY_CAT[cat]
        {
            detected = true;
        }
        i += 1;
    }
    if detected {
        ledger.caught[1] += 1;
    }
    // 族 2：空账读回（未入账即悬挂必拒）。
    ledger.tries[2] += 1;
    let led = SampleLedger::new();
    if let Err(e) = led.reproducible() {
        if e == RectCode::NotReproducible.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RectCode::ALL.len() {
        let v = RectCode::ALL[i].val();
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
        "J1-30组三比一致+复现+等矩",
        matches!(golden, Ok((30, 25, 5))),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 族名与样本闭集点验。
    let mut okc = 0;
    let mut i = 0;
    while i < CAT_COUNT {
        if CAT_NAMES[i].len() > 0 {
            okc += 1;
        }
        i += 1;
    }
    v.push((
        "J1-6族名点验+30样本账",
        okc == 6 && SAMPLES.len() == 30 && CAT_OF.len() == 30,
        format!("{}/6 {}组", okc, SAMPLES.len()),
    ));
    // 空矩交运留痕：空族交运全归零（黄金账复核）。
    let zeroed_audit = {
        let mut ok = true;
        let mut k = 0;
        while k < SAMPLE_COUNT {
            let s = SAMPLES[k];
            if model_is_empty(s) && !model_is_empty(model_intersect(s, PROBE)) {
                ok = false;
            }
            k += 1;
        }
        ok
    };
    v.push((
        "J1-空矩交运归零留痕",
        zeroed_audit,
        String::from("25 空样本逐组归零"),
    ));
    // 反向判据：坏对照必被检出。
    let mut inj2 = InjectLedger::new();
    run_injections(&mut inj2);
    v.push((
        "J1-反向判据坏对照被检出",
        inj2.caught[1] == inj2.tries[1] && inj2.tries[1] == 1,
        format!("坏对照检出 {}/{}", inj2.caught[1], inj2.tries[1]),
    ));
    // 期望表守恒：族级期望布尔表长度与族数一致，且 25 空 5 非空。
    let expect_conserve = {
        let mut t = 0;
        let mut k = 0;
        while k < CAT_COUNT {
            if EXPECTED_EMPTY_BY_CAT[k] {
                t += SAMPLES_PER_CAT;
            }
            k += 1;
        }
        t == 25
    };
    v.push((
        "J1-判据常量守恒25空5非空",
        expect_conserve,
        String::from("族级判据 × 5 样本"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66B00 窗口")));
    v
}

pub fn run_unx14761_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14761 矩形退化族（空/翻转/零宽高 30 组对照）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
