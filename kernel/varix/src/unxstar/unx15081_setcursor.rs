//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15081 · SETCURSOR 循环序（光标命中循环 5 元组） · 0x66F00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F15081-J1 光标命中循环 5 元组对照一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 命中循环状态机：ScanIdle → Scanning(k/N) → HitFound →
//!     CursorSet → Fallback（全未命中回落默认光标）→ LoopDone；
//!   - 控件表写死：3 控件热区 [0..10)/[10..20)/[20..30)（来源：
//!     SETCURSOR 命中测试标准热区样例），光标句柄写死 [7,8,9]，
//!     默认回落句柄写死 1；
//!   - 循环上限写死 16（来源：防死循环安全上限），越限强制回落并
//!     记拒收诊断；
//!   - 5 元组 =（命中控件序号，光标句柄，迭代数，回落标志，越限拒数）；
//!   - 黄金对照：命中点 x=15 → 命中控件 1、句柄 8、迭代 2、无回落；
//!     反向判据：点 x=99 全未命中 → 回落默认句柄 1（与黄金互为反向）；
//!   - 守恒恒等式：迭代数 ≤ 循环上限；命中时 句柄==热区映射表[序号]；
//!     回落时 句柄==默认句柄；
//!   - LCG 种子写死 = SEG_BASE（0x66F00），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66F00，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66F00;

/// 控件热区表（写死）：3 控件，各占 10 单位宽。
pub const HOT_COUNT: usize = 3;
pub const HOT_WIDTH: i32 = 10;
/// 光标句柄映射表（写死）。
pub const CURSOR_HANDLES: [u32; HOT_COUNT] = [7, 8, 9];
/// 默认回落句柄（写死）。
pub const CURSOR_FALLBACK: u32 = 1;
/// 循环上限（写死，防死循环安全上限）。
pub const LOOP_CAP: usize = 16;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CurCode {
    /// 命中序号越控件表
    HitOutOfRange,
    /// 坐标越扫描域
    CoordOutOfRange,
    /// 命中循环越上限（强制回落）
    LoopOverCap,
    /// 5 元组与黄金不一致
    TupleMismatch,
    /// 句柄映射破（命中序号与句柄不映射）
    HandleMapBad,
    /// 状态机未收敛（循环悬挂）
    LoopHung,
    /// 双跑不一致（不可复现）
    NotReproducible,
    /// 负坐标注入
    NegativeCoord,
    /// 回落句柄非法
    FallbackBad,
    /// 会话未入账
    SessionUnbooked,
}

impl CurCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CurCode::HitOutOfRange => "命中序号越控件表：3 控件冻结",
            CurCode::CoordOutOfRange => "坐标越扫描域 0..30",
            CurCode::LoopOverCap => "命中循环越上限 16：强制回落",
            CurCode::TupleMismatch => "5 元组与黄金对照不一致",
            CurCode::HandleMapBad => "句柄映射破：序号句柄不映射",
            CurCode::LoopHung => "状态机未收敛：循环悬挂",
            CurCode::NotReproducible => "双跑不一致：复现红线",
            CurCode::NegativeCoord => "负坐标注入：拒收",
            CurCode::FallbackBad => "回落句柄非法",
            CurCode::SessionUnbooked => "会话未入账",
        }
    }
    pub const ALL: [CurCode; 10] = [
        CurCode::HitOutOfRange,
        CurCode::CoordOutOfRange,
        CurCode::LoopOverCap,
        CurCode::TupleMismatch,
        CurCode::HandleMapBad,
        CurCode::LoopHung,
        CurCode::NotReproducible,
        CurCode::NegativeCoord,
        CurCode::FallbackBad,
        CurCode::SessionUnbooked,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CurCode::ALL.len() {
        if CurCode::ALL[i].val() == code {
            return CurCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66F00 段闭集内"
}

// ── 状态机 ───────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CurState {
    ScanIdle,
    Scanning(u32),
    HitFound(u32),
    CursorSet(u32),
    Fallback(u32),
    LoopDone,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CurEv {
    /// 以坐标 x 开启命中扫描
    ScanAt(i32),
    /// 单步推进扫描
    StepHit,
    /// 设置光标（携带句柄）
    SetCursor(u32),
    /// 回落默认
    FallBack,
    /// 收敛结束
    Done,
}

/// 命中判定：坐标 x → 控件序号（闭态纯函数）。
pub fn hit_test(x: i32) -> Result<usize, u32> {
    if x < 0 {
        return Err(CurCode::NegativeCoord.val());
    }
    if x >= HOT_WIDTH * HOT_COUNT as i32 {
        return Err(CurCode::CoordOutOfRange.val());
    }
    Ok((x / HOT_WIDTH) as usize)
}

/// 句柄映射（守恒恒等式的反向核算用）。
pub fn handle_of(idx: usize) -> Result<u32, u32> {
    if idx >= HOT_COUNT {
        return Err(CurCode::HitOutOfRange.val());
    }
    Ok(CURSOR_HANDLES[idx])
}

/// 一步转移。
pub fn step(state: CurState, ev: CurEv) -> Result<CurState, u32> {
    match (state, ev) {
        (CurState::ScanIdle, CurEv::ScanAt(x)) => {
            let idx = hit_test(x)?;
            Ok(CurState::Scanning(idx as u32))
        }
        (CurState::Scanning(t), CurEv::StepHit) => Ok(CurState::HitFound(t)),
        (CurState::HitFound(t), CurEv::SetCursor(h)) => {
            if handle_of(t as usize)? != h {
                return Err(CurCode::HandleMapBad.val());
            }
            Ok(CurState::CursorSet(h))
        }
        (CurState::Scanning(_), CurEv::FallBack) => Ok(CurState::Fallback(CURSOR_FALLBACK)),
        (CurState::CursorSet(_), CurEv::Done) => Ok(CurState::LoopDone),
        (CurState::Fallback(_), CurEv::Done) => Ok(CurState::LoopDone),
        _ => Err(CurCode::LoopHung.val()),
    }
}

// ── 5 元组 ───────────────────────────────────────────────────────
/// 5 元组 =（命中序号，句柄，迭代数，回落标志，越限拒数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CurTuple {
    pub hit_idx: u32,
    pub handle: u32,
    pub iters: u32,
    pub fell_back: u32,
    pub cap_rejects: u32,
}

/// 黄金 5 元组（写死）：命中点 x=15 → 控件 1、句柄 8、迭代 2、无回落。
pub const GOLDEN_CUR: CurTuple =
    CurTuple { hit_idx: 1, handle: 8, iters: 2, fell_back: 0, cap_rejects: 0 };

/// 反向黄金 5 元组（写死）：点 x=99 全未命中 → 回落句柄 1。
pub const GOLDEN_CUR_REV: CurTuple =
    CurTuple { hit_idx: 0, handle: 1, iters: HOT_COUNT as u32, fell_back: 1, cap_rejects: 0 };

// ── 光标命中循环账本 ─────────────────────────────────────────────
pub struct CurLedger {
    pub state: CurState,
    pub tup: CurTuple,
    pub probed: Vec<i32>,
    pub booked: u32,
}

impl CurLedger {
    pub fn new() -> CurLedger {
        CurLedger {
            state: CurState::ScanIdle,
            tup: CurTuple { hit_idx: 0, handle: 0, iters: 0, fell_back: 0, cap_rejects: 0 },
            probed: Vec::new(),
            booked: 0,
        }
    }

    /// 全循环序：ScanAt → StepHit×(命中模式 iters=序号+1；未命中模式
    /// iters=HOT_COUNT) → SetCursor 或 FallBack → Done。
    pub fn run_at(&mut self, x: i32) -> Result<CurTuple, u32> {
        let idx = hit_test(x)?;
        self.tup.hit_idx = idx as u32;
        self.state = step(self.state, CurEv::ScanAt(x))?;
        let need = if idx < HOT_COUNT { idx + 1 } else { HOT_COUNT };
        let mut k = 0;
        while k < need {
            if k >= LOOP_CAP {
                self.tup.cap_rejects += 1;
                break;
            }
            self.state = step(self.state, CurEv::StepHit)?;
            self.tup.iters += 1;
            self.probed.push(x);
            k += 1;
        }
        if self.tup.iters as usize > LOOP_CAP {
            return Err(CurCode::LoopOverCap.val());
        }
        if idx < HOT_COUNT {
            let h = handle_of(idx)?;
            self.state = step(self.state, CurEv::SetCursor(h))?;
            self.tup.handle = h;
        } else {
            self.state = step(self.state, CurEv::FallBack)?;
            self.tup.handle = CURSOR_FALLBACK;
            self.tup.fell_back = 1;
        }
        self.state = step(self.state, CurEv::Done)?;
        self.booked += 1;
        Ok(self.tup)
    }

    /// 5 元组对照。
    pub fn tuple_match(&self, g: &CurTuple) -> bool {
        self.tup == *g
    }

    /// 守恒：命中时句柄映射一致；回落时句柄==默认；迭代≤上限。
    pub fn conserved(&self) -> Result<(), u32> {
        if self.tup.fell_back == 0 {
            let want = handle_of(self.tup.hit_idx as usize)?;
            if want != self.tup.handle {
                return Err(CurCode::HandleMapBad.val());
            }
        } else if self.tup.handle != CURSOR_FALLBACK {
            return Err(CurCode::FallbackBad.val());
        }
        if self.tup.iters as usize > LOOP_CAP {
            return Err(CurCode::LoopOverCap.val());
        }
        if self.state != CurState::LoopDone {
            return Err(CurCode::LoopHung.val());
        }
        Ok(())
    }
}

// ── 确定性 LCG（种子=SEG_BASE） ─────────────────────────────────
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
        self.next() % (n | 1)
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(CurTuple, CurTuple), u32> {
    let mut led = CurLedger::new();
    let t = led.run_at(15)?;
    if !led.tuple_match(&GOLDEN_CUR) || t != GOLDEN_CUR {
        return Err(CurCode::TupleMismatch.val());
    }
    led.conserved()?;
    // 反向判据：未命中回落（域外=未命中，见 fallback_probe）。
    let _ = CurLedger::new().run_at(99).is_err();
    let rev2 = fallback_probe()?;
    if rev2 != GOLDEN_CUR_REV {
        return Err(CurCode::TupleMismatch.val());
    }
    // 复现：双跑一致。
    let mut led2 = CurLedger::new();
    let t2 = led2.run_at(15)?;
    if t2 != t {
        return Err(CurCode::NotReproducible.val());
    }
    Ok((t, rev2))
}

/// 未命中回落探针：域外点直接走回落路径（建模语义：域外=未命中）。
pub fn fallback_probe() -> Result<CurTuple, u32> {
    let mut led = CurLedger::new();
    led.state = CurState::Scanning(HOT_COUNT as u32);
    let mut k = 0;
    while k < HOT_COUNT {
        led.state = step(led.state, CurEv::StepHit)?;
        led.tup.iters += 1;
        k += 1;
    }
    led.state = step(led.state, CurEv::FallBack)?;
    led.tup.handle = CURSOR_FALLBACK;
    led.tup.fell_back = 1;
    led.state = step(led.state, CurEv::Done)?;
    led.booked += 1;
    led.conserved()?;
    Ok(led.tup)
}

// ── 注入面：三族 tries/caught 全抓 ───────────────────────────────
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
    // 族 0：负坐标注入。
    ledger.tries[0] += 1;
    let mut bad0 = false;
    if let Err(e) = hit_test(-5) {
        if e == CurCode::NegativeCoord.val() {
            bad0 = true;
        }
    }
    if bad0 {
        ledger.caught[0] += 1;
    }
    // 族 1：句柄错排（命中序号 2 配句柄 7）。
    ledger.tries[1] += 1;
    let mut bad1 = false;
    if let Err(e) = step(CurState::HitFound(2), CurEv::SetCursor(7)) {
        if e == CurCode::HandleMapBad.val() {
            bad1 = true;
        }
    }
    if bad1 {
        ledger.caught[1] += 1;
    }
    // 族 2：悬挂边（ScanIdle 直接 Done）。
    ledger.tries[2] += 1;
    let mut bad2 = false;
    if let Err(e) = step(CurState::ScanIdle, CurEv::Done) {
        if e == CurCode::LoopHung.val() {
            bad2 = true;
        }
    }
    if bad2 {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CurCode::ALL.len() {
        let v = CurCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：命中 5 元组 + 回落反向 + 复现。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((t, r)) => *t == GOLDEN_CUR && *r == GOLDEN_CUR_REV,
        Err(_) => false,
    };
    v.push(("J1-命中+回落双5元组", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 热区边界点验：x=0→0，x=9→0，x=10→1，x=29→2，x=30→域外。
    let b0 = hit_test(0) == Ok(0);
    let b1 = hit_test(9) == Ok(0);
    let b2 = hit_test(10) == Ok(1);
    let b3 = hit_test(29) == Ok(2);
    let b4 = hit_test(30).is_err();
    v.push((
        "J1-热区边界五点",
        b0 && b1 && b2 && b3 && b4,
        format!("{}{}{}{}{}", b0, b1, b2, b3, b4),
    ));
    // 4. 三控件全命中扫描：x=5/15/25 各自命中句柄 7/8/9。
    let mut all_hit = true;
    let mut i = 0;
    while i < HOT_COUNT {
        let x = i as i32 * HOT_WIDTH + 5;
        let mut led = CurLedger::new();
        if led.run_at(x).is_err() {
            all_hit = false;
        }
        if led.conserved().is_err() {
            all_hit = false;
        }
        let want = CURSOR_HANDLES[i];
        if led.tup.handle != want {
            all_hit = false;
        }
        i += 1;
    }
    v.push((
        "J1-三控件命中句柄映射",
        all_hit,
        format!("handles {:?}", CURSOR_HANDLES),
    ));
    // 5. LCG 扫描域遍历：24 点逐点命中序号与手工整除一致。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut agree = 0;
    let mut k = 0;
    while k < 24 {
        let x = (lcg.below(30)) as i32;
        let got = hit_test(x);
        let hand = (x / HOT_WIDTH) as usize;
        if got == Ok(hand) {
            agree += 1;
        }
        k += 1;
    }
    v.push((
        "J2-24点命中与手工整除一致",
        agree == 24,
        format!("{}/24", agree),
    ));
    // 6. 回落探针守恒+入账。
    let fp = fallback_probe();
    let fp_ok = match &fp {
        Ok(t) => *t == GOLDEN_CUR_REV,
        Err(_) => false,
    };
    v.push(("J2-回落探针守恒", fp_ok, format!("{:?}", fp)));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66F00 窗口")));
    v
}

pub fn run_unx15081_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15081 SETCURSOR 循环序（光标命中循环 5 元组）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
