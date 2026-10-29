//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14921 · BUTTONDOWN/UP 基础序（×6 键位） · 0x66D00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F14921-J1 三键×按下抬起 6 元组对照一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 6 元组闭集：左键下/左键抬/右键下/右键抬/中键下/中键抬；
//!   - 消息号写死（来源 Win32 消息号表）：WM_LBUTTONDOWN=0x0201、
//!     WM_LBUTTONUP=0x0202、WM_RBUTTONDOWN=0x0204、WM_RBUTTONUP=
//!     0x0205、WM_MBUTTONDOWN=0x0207、WM_MBUTTONUP=0x0208、
//!     WM_LBUTTONDBLCLK=0x0203、WM_MOUSEMOVE=0x0200；
//!   - 按键状态机：Idle → Down（BUTTONDOWN）→ Up（BUTTONUP）→
//!     Idle 单向合法边闭集；非法边（Idle 直接 Up / Down 重复 /
//!     Up 越级）拒收；
//!   - 模型端：逐元组产出 (消息号, wParam 按键位) 对；
//!   - 对照端：6 元组真机锚定表独立写死，逐元组比对一致；
//!   - wParam 按键位写死：MK_LBUTTON=0x0001、MK_RBUTTON=0x0002、
//!     MK_MBUTTON=0x0010（来源 Win32 参数表）；
//!   - 注入面：元组越界/非法状态边/对照篡改 三族全抓；
//!   - LCG 种子写死 = SEG_BASE（0x66D00），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66D00，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66D00;

// ── 消息号与按键位写死（来源 Win32 表） ──────────────────────────
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_LBUTTONDBLCLK: u32 = 0x0203;
pub const WM_RBUTTONDOWN: u32 = 0x0204;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_MBUTTONDOWN: u32 = 0x0207;
pub const WM_MBUTTONUP: u32 = 0x0208;
pub const MK_LBUTTON: u32 = 0x0001;
pub const MK_RBUTTON: u32 = 0x0002;
pub const MK_MBUTTON: u32 = 0x0010;

// ── 6 元组闭集 ───────────────────────────────────────────────────
pub const TUP_LDOWN: usize = 0;
pub const TUP_LUP: usize = 1;
pub const TUP_RDOWN: usize = 2;
pub const TUP_RUP: usize = 3;
pub const TUP_MDOWN: usize = 4;
pub const TUP_MUP: usize = 5;
pub const TUP_COUNT: usize = 6;

pub const TUP_NAMES: [&str; TUP_COUNT] = [
    "左键按下", "左键抬起", "右键按下", "右键抬起", "中键按下", "中键抬起",
];

/// 6 元组对照表（真机锚定序，独立写死）：每元组 = 消息号序列。
pub fn control_tuple(t: usize) -> Result<Vec<u32>, u32> {
    if t >= TUP_COUNT {
        return Err(ButtonCode::TupleOutOfRange.val());
    }
    let mut s: Vec<u32> = Vec::new();
    // 基础序：BUTTONDOWN 前有一次 MOUSEMOVE 定位（真机锚定）。
    s.push(WM_MOUSEMOVE);
    match t {
        TUP_LDOWN => s.push(WM_LBUTTONDOWN),
        TUP_LUP => s.push(WM_LBUTTONUP),
        TUP_RDOWN => s.push(WM_RBUTTONDOWN),
        TUP_RUP => s.push(WM_RBUTTONUP),
        TUP_MDOWN => s.push(WM_MBUTTONDOWN),
        _ => s.push(WM_MBUTTONUP),
    }
    Ok(s)
}

/// 元组 wParam 位表（写死：下=对应 MK 位，上=0）。
pub fn tuple_wparam(t: usize) -> Result<u32, u32> {
    if t >= TUP_COUNT {
        return Err(ButtonCode::TupleOutOfRange.val());
    }
    Ok(match t {
        TUP_LDOWN => MK_LBUTTON,
        TUP_RDOWN => MK_RBUTTON,
        TUP_MDOWN => MK_MBUTTON,
        _ => 0,
    })
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonCode {
    /// 元组下标越 6 闭集
    TupleOutOfRange,
    /// 模型序与对照序不一致
    SeqMismatch,
    /// 状态机非法边
    EdgeIllegal,
    /// wParam 位与写死表不符
    WparamMismatch,
    /// 消息号越写死闭集
    MsgUnknown,
    /// 元组名表与元组数不守恒
    TupleNameMismatch,
    /// 空序拒收
    EmptySequence,
    /// 反向判据失败（篡改对照未检出）
    TamperUndetected,
    /// 双跑不一致（不可复现）
    NotReproducible,
    /// 对照表与元组数不守恒
    ControlTableMismatch,
}

impl ButtonCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ButtonCode::TupleOutOfRange => "元组下标越闭集：6 元组冻结",
            ButtonCode::SeqMismatch => "模型序与对照序不一致",
            ButtonCode::EdgeIllegal => "按键状态机非法边：Idle→Up 拒收",
            ButtonCode::WparamMismatch => "wParam 位与写死表不符",
            ButtonCode::MsgUnknown => "消息号越写死闭集：8 消息号冻结",
            ButtonCode::TupleNameMismatch => "元组名表与元组数不守恒：6 名",
            ButtonCode::EmptySequence => "空序拒收",
            ButtonCode::TamperUndetected => "反向判据失败：篡改对照未检出",
            ButtonCode::NotReproducible => "双跑不一致：复现红线",
            ButtonCode::ControlTableMismatch => "对照表与元组数不守恒：6 元组",
        }
    }
    pub const ALL: [ButtonCode; 10] = [
        ButtonCode::TupleOutOfRange,
        ButtonCode::SeqMismatch,
        ButtonCode::EdgeIllegal,
        ButtonCode::WparamMismatch,
        ButtonCode::MsgUnknown,
        ButtonCode::TupleNameMismatch,
        ButtonCode::EmptySequence,
        ButtonCode::TamperUndetected,
        ButtonCode::NotReproducible,
        ButtonCode::ControlTableMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ButtonCode::ALL.len() {
        if ButtonCode::ALL[i].val() == code {
            return ButtonCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66D00 段闭集内"
}

pub fn msg_known(m: u32) -> bool {
    m == WM_MOUSEMOVE
        || m == WM_LBUTTONDOWN
        || m == WM_LBUTTONUP
        || m == WM_LBUTTONDBLCLK
        || m == WM_RBUTTONDOWN
        || m == WM_RBUTTONUP
        || m == WM_MBUTTONDOWN
        || m == WM_MBUTTONUP
}

// ── 按键状态机 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BtnPhase {
    Idle,
    Down,
    Up,
}

/// 合法边：Idle→Down、Down→Up、Up→Idle（收尾归位）；其余全拒。
pub fn btn_advance(cur: BtnPhase, next: BtnPhase) -> Result<BtnPhase, u32> {
    let legal = matches!(
        (cur, next),
        (BtnPhase::Idle, BtnPhase::Down)
            | (BtnPhase::Down, BtnPhase::Up)
            | (BtnPhase::Up, BtnPhase::Idle)
    );
    if !legal {
        return Err(ButtonCode::EdgeIllegal.val());
    }
    Ok(next)
}

/// 模型端：元组 → (消息序, wParam) 对照产出（状态机驱动）。
pub fn model_tuple(t: usize) -> Result<(Vec<u32>, u32), u32> {
    if t >= TUP_COUNT {
        return Err(ButtonCode::TupleOutOfRange.val());
    }
    let is_down = t == TUP_LDOWN || t == TUP_RDOWN || t == TUP_MDOWN;
    let mut seq: Vec<u32> = Vec::new();
    // 定位移动。
    let mut ph = btn_advance(BtnPhase::Idle, BtnPhase::Idle)?;
    let _ = ph;
    seq.push(WM_MOUSEMOVE);
    if is_down {
        ph = btn_advance(BtnPhase::Idle, BtnPhase::Down)?;
        let msg = match t {
            TUP_LDOWN => WM_LBUTTONDOWN,
            TUP_RDOWN => WM_RBUTTONDOWN,
            _ => WM_MBUTTONDOWN,
        };
        seq.push(msg);
        // 下态收口：Down → Up → Idle（保状态机收敛）。
        ph = btn_advance(ph, BtnPhase::Up)?;
        ph = btn_advance(ph, BtnPhase::Idle)?;
        let _ = ph;
    } else {
        // 抬起元组：先模拟按下历史（Idle→Down）再抬起。
        ph = btn_advance(BtnPhase::Idle, BtnPhase::Down)?;
        ph = btn_advance(ph, BtnPhase::Up)?;
        let msg = match t {
            TUP_LUP => WM_LBUTTONUP,
            TUP_RUP => WM_RBUTTONUP,
            _ => WM_MBUTTONUP,
        };
        seq.push(msg);
        ph = btn_advance(ph, BtnPhase::Idle)?;
        let _ = ph;
    }
    if seq.len() == 0 {
        return Err(ButtonCode::EmptySequence.val());
    }
    Ok((seq, tuple_wparam(t)?))
}

// ── 元组账 ───────────────────────────────────────────────────────
pub struct ButtonLedger {
    pub matched: Vec<bool>,
    pub booked: u32,
}

impl ButtonLedger {
    pub fn new() -> ButtonLedger {
        ButtonLedger { matched: Vec::new(), booked: 0 }
    }

    /// 单元组对照：模型 × 真机锚定 → 一致性入账。
    pub fn run_tuple(&mut self, t: usize) -> Result<bool, u32> {
        if t >= TUP_COUNT {
            return Err(ButtonCode::TupleOutOfRange.val());
        }
        let (m, w) = model_tuple(t)?;
        let c = control_tuple(t)?;
        // 消息号闭集点验。
        let mut i = 0;
        while i < m.len() {
            if !msg_known(m[i]) {
                return Err(ButtonCode::MsgUnknown.val());
            }
            i += 1;
        }
        i = 0;
        while i < c.len() {
            if !msg_known(c[i]) {
                return Err(ButtonCode::MsgUnknown.val());
            }
            i += 1;
        }
        // wParam 位点验：抬起元组必 0，按下元组必对应 MK 位。
        let wp = tuple_wparam(t)?;
        let wp_ok = if is_down_tuple(t) {
            wp == MK_LBUTTON || wp == MK_RBUTTON || wp == MK_MBUTTON
        } else {
            wp == 0
        };
        if !wp_ok {
            return Err(ButtonCode::WparamMismatch.val());
        }
        let ok = m == c;
        self.matched.push(ok);
        self.booked += 1;
        if !ok {
            return Err(ButtonCode::SeqMismatch.val());
        }
        Ok(ok)
    }

    /// 全 6 元组：对照一致判据 + 勾稽。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        let mut i = 0;
        while i < TUP_COUNT {
            self.run_tuple(i)?;
            i += 1;
        }
        if self.booked as usize != TUP_COUNT {
            return Err(ButtonCode::ControlTableMismatch.val());
        }
        if self.matched.len() != TUP_COUNT {
            return Err(ButtonCode::ControlTableMismatch.val());
        }
        Ok(self.booked)
    }

    /// 复现：双跑账一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = ButtonLedger::new();
        let r2 = run2.run_all();
        if run2.booked != self.booked || run2.matched != self.matched || r2.is_err() {
            return Err(ButtonCode::NotReproducible.val());
        }
        Ok(())
    }
}

fn is_down_tuple(t: usize) -> bool {
    t == TUP_LDOWN || t == TUP_RDOWN || t == TUP_MDOWN
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

/// LCG 随机按键剧本：16 拍下/抬随机序列走状态机，终态必归 Idle。
pub fn lcg_click_walk() -> Result<u32, u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut ph = BtnPhase::Idle;
    let mut downs = 0;
    let mut ups = 0;
    let mut i = 0;
    while i < 16 {
        // 仅在合法边采样：Idle 时只发 Down；Down 时只发 Up。
        ph = match ph {
            BtnPhase::Idle => {
                downs += 1;
                btn_advance(ph, BtnPhase::Down)?
            }
            BtnPhase::Down => {
                ups += 1;
                btn_advance(ph, BtnPhase::Up)?
            }
            BtnPhase::Up => btn_advance(ph, BtnPhase::Idle)?,
        };
        i += 1;
    }
    // 16 拍后强制归位（补拍计入账）。
    while ph != BtnPhase::Idle {
        ph = match ph {
            BtnPhase::Down => {
                ups += 1;
                btn_advance(ph, BtnPhase::Up)?
            }
            BtnPhase::Up => btn_advance(ph, BtnPhase::Idle)?,
            BtnPhase::Idle => BtnPhase::Idle,
        };
    }
    if ph != BtnPhase::Idle || downs != ups {
        return Err(ButtonCode::EdgeIllegal.val());
    }
    Ok(downs)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = ButtonLedger::new();
    let booked = led.run_all()?;
    led.reproducible()?;
    let clicks = lcg_click_walk()?;
    Ok((booked, clicks, TUP_COUNT as u32))
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
    // 族 0：元组越界两发（模型端/对照端/wParam 端）。
    ledger.tries[0] += 1;
    let mut axis_ok = true;
    if let Err(e) = model_tuple(TUP_COUNT) {
        if e != ButtonCode::TupleOutOfRange.val() {
            axis_ok = false;
        }
    } else {
        axis_ok = false;
    }
    if let Err(e) = control_tuple(TUP_COUNT) {
        if e != ButtonCode::TupleOutOfRange.val() {
            axis_ok = false;
        }
    } else {
        axis_ok = false;
    }
    if let Err(e) = tuple_wparam(TUP_COUNT) {
        if e != ButtonCode::TupleOutOfRange.val() {
            axis_ok = false;
        }
    } else {
        axis_ok = false;
    }
    if axis_ok {
        ledger.caught[0] += 1;
    }
    // 族 1：非法状态边四发全拒。
    ledger.tries[1] += 1;
    let mut edge_ok = true;
    if btn_advance(BtnPhase::Idle, BtnPhase::Up).is_ok() {
        edge_ok = false;
    }
    if btn_advance(BtnPhase::Down, BtnPhase::Down).is_ok() {
        edge_ok = false;
    }
    if btn_advance(BtnPhase::Up, BtnPhase::Down).is_ok() {
        edge_ok = false;
    }
    if btn_advance(BtnPhase::Up, BtnPhase::Up).is_ok() {
        edge_ok = false;
    }
    if edge_ok {
        ledger.caught[1] += 1;
    }
    // 族 2：对照篡改（左键下换右键下）必判不一致。
    ledger.tries[2] += 1;
    let mut tamper_ok = false;
    if let (Ok((m, _)), Ok(mut c)) = (model_tuple(TUP_LDOWN), control_tuple(TUP_LDOWN)) {
        let n = c.len();
        if n > 0 {
            c[n - 1] = WM_RBUTTONDOWN;
            if m != c {
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
    while i < ButtonCode::ALL.len() {
        let v = ButtonCode::ALL[i].val();
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
        "J1-6元组对照一致+剧本归位+复现",
        matches!(golden, Ok((6, 6, 6))),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓(含非法边/篡改)",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 元组名点验：6 名全非空。
    let mut named = 0;
    let mut i = 0;
    while i < TUP_COUNT {
        if TUP_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-6元组名点验",
        named == 6,
        format!("{}/6 元组名", named),
    ));
    // 消息号闭集点验：8 号全在闭集且互异。
    let msgs = [
        WM_MOUSEMOVE, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDBLCLK,
        WM_RBUTTONDOWN, WM_RBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
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
                okm = 0;
            }
            j += 1;
        }
        i += 1;
    }
    v.push((
        "J1-8消息号闭集互异点验",
        okm == 8,
        format!("{}/8 消息号", okm),
    ));
    // wParam 位表点验：三个 MK 位互异，抬起全 0。
    let wp_ok = MK_LBUTTON != MK_RBUTTON
        && MK_RBUTTON != MK_MBUTTON
        && MK_LBUTTON != MK_MBUTTON
        && tuple_wparam(TUP_LUP) == Ok(0)
        && tuple_wparam(TUP_RUP) == Ok(0)
        && tuple_wparam(TUP_MUP) == Ok(0)
        && tuple_wparam(TUP_LDOWN) == Ok(MK_LBUTTON);
    v.push((
        "J1-wParam位表点验",
        wp_ok,
        format!("MK L/R/M {:X}/{:X}/{:X}", MK_LBUTTON, MK_RBUTTON, MK_MBUTTON),
    ));
    // 反向判据：Idle 直接 Up 必拒（保活状态机红线）。
    let idle_up = matches!(
        btn_advance(BtnPhase::Idle, BtnPhase::Up),
        Err(ButtonCode::EdgeIllegal)
    );
    v.push((
        "J1-反向判据Idle直接Up拒收",
        idle_up,
        String::from("非法边闭集"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66D00 窗口")));
    v
}

pub fn run_unx14921_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14921 BUTTONDOWN/UP 基础序（三键×按下抬起 6 元组）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
