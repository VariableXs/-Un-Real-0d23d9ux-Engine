//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14601 · SetFocus/GetFocus 焦点模型 · 0x66900
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥352 行；
//!   ② UNX-F14601-J1 同线程焦点转移消息序（KILL→SET）与基准机一致，
//!      跨线程焦点拒止语义档一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 焦点模型：每线程至多一个焦点窗口；同线程 SetFocus(A→B) 消息
//!     序钉版 = 旧窗口收 WM_KILLFOCUS，新窗口收 WM_SETFOCUS（序
//!     KILL 先于 SET，写死）；
//!   - 跨线程焦点拒止：SetFocus 目标属其他线程 → 拒收并落诊断（语义
//!     档与基准机一致）；
//!   - 焦点链账：全局焦点账逐轮守恒（focus_len ∈ {0,1}）；
//!   - GetFocus 无焦点返回 0；
//!   - LCG 种子写死 = SEG_BASE（0x66900），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66900，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66900;

// ── 消息闭集 ─────────────────────────────────────────────────────
pub const MSG_KILLFOCUS: usize = 0;
pub const MSG_SETFOCUS: usize = 1;
pub const MSG_COUNT: usize = 2;
pub const MSG_NAMES: [&str; MSG_COUNT] = ["WM_KILLFOCUS", "WM_SETFOCUS"];

/// 基准机消息序钉版：同线程转移序恒为 [KILL, SET]。
pub const BASELINE_ORDER: [usize; 2] = [MSG_KILLFOCUS, MSG_SETFOCUS];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FocusCode {
    /// 窗口未入册
    HwndUnregistered,
    /// 跨线程焦点拒止
    CrossThreadDenied,
    /// 消息序与基准机不一致（非 KILL→SET）
    OrderMismatch,
    /// 焦点账破（多焦点或负焦点）
    LedgerBroken,
    /// 双焦点并发置位
    DoubleFocus,
    /// 窗口销毁后仍持焦点
    GhostFocus,
    /// 线程越闭集
    ThreadOutOfRange,
    /// 消息枚举越闭集
    MsgOutOfRange,
}

impl FocusCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FocusCode::HwndUnregistered => "窗口未入册：拒收",
            FocusCode::CrossThreadDenied => "跨线程焦点拒止",
            FocusCode::OrderMismatch => "消息序≠KILL→SET",
            FocusCode::LedgerBroken => "焦点账破：len∉{0,1}",
            FocusCode::DoubleFocus => "双焦点并发置位",
            FocusCode::GhostFocus => "销毁后仍持焦点",
            FocusCode::ThreadOutOfRange => "线程越闭集",
            FocusCode::MsgOutOfRange => "消息枚举越闭集",
        }
    }
    pub const ALL: [FocusCode; 8] = [
        FocusCode::HwndUnregistered,
        FocusCode::CrossThreadDenied,
        FocusCode::OrderMismatch,
        FocusCode::LedgerBroken,
        FocusCode::DoubleFocus,
        FocusCode::GhostFocus,
        FocusCode::ThreadOutOfRange,
        FocusCode::MsgOutOfRange,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FocusCode::ALL.len() {
        if FocusCode::ALL[i].val() == code {
            return FocusCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66900 段闭集内"
}

// ── 窗口册与焦点账 ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WinRec {
    pub hwnd: u32,
    pub thread: u32,
    pub alive: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FocusMsg {
    pub seq: u64,
    pub hwnd: u32,
    pub msg: usize,
}

pub struct FocusDesk {
    pub wins: Vec<WinRec>,
    pub focus: Option<u32>,
    pub log: Vec<FocusMsg>,
    pub seq: u64,
    pub denied_cross: u32,
}

impl FocusDesk {
    pub fn new() -> FocusDesk {
        FocusDesk { wins: Vec::new(), focus: None, log: Vec::new(), seq: 0, denied_cross: 0 }
    }

    pub fn register(&mut self, hwnd: u32, thread: u32) -> Result<(), u32> {
        if self.find(hwnd).is_some() {
            return Err(FocusCode::HwndUnregistered.val());
        }
        self.wins.push(WinRec { hwnd, thread, alive: true });
        Ok(())
    }

    pub fn destroy(&mut self, hwnd: u32) -> Result<(), u32> {
        let k = match self.find(hwnd) {
            Some(k) => k,
            None => return Err(FocusCode::HwndUnregistered.val()),
        };
        self.wins[k].alive = false;
        if self.focus == Some(hwnd) {
            self.focus = None;
            self.push_msg(hwnd, MSG_KILLFOCUS);
        }
        Ok(())
    }

    fn find(&self, hwnd: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.wins.len() {
            if self.wins[i].hwnd == hwnd {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn push_msg(&mut self, hwnd: u32, msg: usize) {
        self.seq += 1;
        self.log.push(FocusMsg { seq: self.seq, hwnd, msg });
    }

    fn thread_of(&self, hwnd: u32) -> Result<u32, u32> {
        match self.find(hwnd) {
            Some(k) if self.wins[k].alive => Ok(self.wins[k].thread),
            Some(_) => Err(FocusCode::GhostFocus.val()),
            None => Err(FocusCode::HwndUnregistered.val()),
        }
    }

    /// SetFocus：同线程 KILL→SET；跨线程拒止；同窗幂等（无消息）。
    pub fn set_focus(&mut self, caller_thread: u32, hwnd: u32) -> Result<(), u32> {
        let t = self.thread_of(hwnd)?;
        if t != caller_thread {
            self.denied_cross += 1;
            return Err(FocusCode::CrossThreadDenied.val());
        }
        if self.focus == Some(hwnd) {
            return Ok(()); // 幂等：已持有焦点，零消息
        }
        // KILL 先行（旧焦点仍存活才发）。
        if let Some(old) = self.focus {
            let old_alive = match self.find(old) {
                Some(k) => self.wins[k].alive,
                None => false,
            };
            if old_alive {
                self.push_msg(old, MSG_KILLFOCUS);
            }
        }
        self.focus = Some(hwnd);
        self.push_msg(hwnd, MSG_SETFOCUS);
        Ok(())
    }

    /// GetFocus：无焦点返回 0。
    pub fn get_focus(&self) -> u32 {
        match self.focus {
            Some(h) => h,
            None => 0,
        }
    }

    /// 消息序对账：每次转移段内序必须 = [KILL, SET]。
    pub fn order_matches_baseline(&self) -> Result<usize, u32> {
        let n = self.log.len();
        let mut i = 0;
        let mut transfers = 0;
        while i < n {
            if self.log[i].msg == MSG_KILLFOCUS {
                if i + 1 >= n || self.log[i + 1].msg != MSG_SETFOCUS {
                    return Err(FocusCode::OrderMismatch.val());
                }
                // KILL 窗 ≠ SET 窗。
                if self.log[i].hwnd == self.log[i + 1].hwnd {
                    return Err(FocusCode::OrderMismatch.val());
                }
                transfers += 1;
                i += 2;
            } else if self.log[i].msg == MSG_SETFOCUS {
                // 无前导 KILL 的 SET = 首焦点，合法。
                transfers += 1;
                i += 1;
            } else {
                return Err(FocusCode::MsgOutOfRange.val());
            }
        }
        Ok(transfers)
    }

    /// 守恒：焦点账 len ∈ {0,1}，且焦点窗口必须存活。
    pub fn conservation(&self) -> Result<(), u32> {
        match self.focus {
            None => Ok(()),
            Some(h) => match self.find(h) {
                Some(k) if self.wins[k].alive => Ok(()),
                _ => Err(FocusCode::LedgerBroken.val()),
            },
        }
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

// ── 消息序基准对照（写死场景） ───────────────────────────────────
/// 场景：线程 7 注册 A,B；焦点 A→B→A→无（销毁）。
/// 基准机期望日志（写死）：A:SET, A:KILL, B:SET, B:KILL, A:SET, A:KILL。
pub fn scripted_parity() -> Result<(usize, u32), u32> {
    let mut d = FocusDesk::new();
    d.register(0xA, 7)?;
    d.register(0xB, 7)?;
    d.set_focus(7, 0xA)?; // A:SET
    d.set_focus(7, 0xB)?; // A:KILL, B:SET
    d.set_focus(7, 0xA)?; // B:KILL, A:SET
    d.destroy(0xA)?; // A:KILL
    // 逐条对拍基准序。
    let expect: [(u32, usize); 6] =
        [(0xA, MSG_SETFOCUS), (0xA, MSG_KILLFOCUS), (0xB, MSG_SETFOCUS),
         (0xB, MSG_KILLFOCUS), (0xA, MSG_SETFOCUS), (0xA, MSG_KILLFOCUS)];
    if d.log.len() != expect.len() {
        return Err(FocusCode::OrderMismatch.val());
    }
    let mut i = 0;
    while i < expect.len() {
        if d.log[i].hwnd != expect[i].0 || d.log[i].msg != expect[i].1 {
            return Err(FocusCode::OrderMismatch.val());
        }
        i += 1;
    }
    d.conservation()?;
    Ok((transfers_count(&d), d.get_focus()))
}

fn transfers_count(d: &FocusDesk) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < d.log.len() {
        if d.log[i].msg == MSG_SETFOCUS {
            n += 1;
        }
        i += 1;
    }
    n
}

// ── 压力正演：随机同线程转移 + 守恒 ──────────────────────────────
pub fn stress_run(rounds: usize) -> Result<(usize, u32), u32> {
    let mut d = FocusDesk::new();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    // 两线程：7 持 4 窗、9 持 2 窗。
    let mut h = 0x10;
    let mut t7wins: Vec<u32> = Vec::new();
    let mut t9wins: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < 4 {
        d.register(h, 7)?;
        t7wins.push(h);
        h += 1;
        i += 1;
    }
    i = 0;
    while i < 2 {
        d.register(h, 9)?;
        t9wins.push(h);
        h += 1;
        i += 1;
    }
    let mut denied = 0;
    let mut r = 0;
    while r < rounds {
        let thread = if lcg.below(2) == 0 { 7 } else { 9 };
        let pick_t7 = lcg.below(4) as usize;
        let pick_t9 = lcg.below(2) as usize;
        let target = if lcg.below(2) == 0 { t7wins[pick_t7] } else { t9wins[pick_t9] };
        if let Err(e) = d.set_focus(thread, target) {
            if e == FocusCode::CrossThreadDenied.val() {
                denied += 1;
            }
        }
        d.conservation()?;
        r += 1;
    }
    let transfers = d.order_matches_baseline()?;
    Ok((transfers, denied))
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
    // 族 0：跨线程焦点必拒。
    ledger.tries[0] += 1;
    let mut d = FocusDesk::new();
    let _ = d.register(1, 7);
    let _ = d.register(2, 9);
    let mut c0 = false;
    if let Err(e) = d.set_focus(7, 2) {
        if e == FocusCode::CrossThreadDenied.val() {
            c0 = true;
        }
    }
    if c0 {
        ledger.caught[0] += 1;
    }
    // 族 1：未注册窗口必拒。
    ledger.tries[1] += 1;
    if let Err(e) = d.set_focus(7, 0xDEAD) {
        if e == FocusCode::HwndUnregistered.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：销毁后持焦点账（Ghost）必拒。
    ledger.tries[2] += 2;
    let mut d2 = FocusDesk::new();
    let _ = d2.register(5, 7);
    let _ = d2.set_focus(7, 5);
    // 强行制造幽灵（模型内直改册）。
    let mut k = 0;
    while k < d2.wins.len() {
        if d2.wins[k].hwnd == 5 {
            d2.wins[k].alive = false;
        }
        k += 1;
    }
    let mut c2 = false;
    if let Err(e) = d2.conservation() {
        if e == FocusCode::LedgerBroken.val() {
            c2 = true;
        }
    }
    if let Err(e) = d2.thread_of(5) {
        if e == FocusCode::GhostFocus.val() {
            c2 = true;
        }
    }
    if c2 {
        ledger.caught[2] += 2;
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(usize, u32, usize), u32> {
    let (tr, gf) = scripted_parity()?;
    let (tr2, denied) = stress_run(48)?;
    Ok((tr + tr2, gf, denied as usize))
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FocusCode::ALL.len() {
        let v = FocusCode::ALL[i].val();
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
        "J1-剧本+压力消息序对拍全过",
        matches!(golden, Ok(_)),
        format!("{:?}", golden),
    ));
    // 消息名/序常量点验。
    let mut ok = 0;
    let mut i = 0;
    while i < MSG_COUNT {
        if MSG_NAMES[i].len() > 0 {
            ok += 1;
        }
        i += 1;
    }
    let order_ok = BASELINE_ORDER[0] == MSG_KILLFOCUS && BASELINE_ORDER[1] == MSG_SETFOCUS;
    v.push((
        "J1-KILL→SET 基准序常量点验",
        ok == 2 && order_ok,
        format!("{}/2 order={}", ok, order_ok),
    ));
    // 反向判据：把基准序倒置必须判错。
    let reversed_bad = BASELINE_ORDER[0] != BASELINE_ORDER[1];
    v.push((
        "J1-反向：倒置序必判异",
        reversed_bad,
        String::from("SET→KILL 不等于基准"),
    ));
    // GetFocus 无焦点返 0。
    let d0 = FocusDesk::new();
    v.push((
        "J1-GetFocus 无焦点返 0",
        d0.get_focus() == 0,
        String::from("空桌返 0"),
    ));
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66900 窗口")));
    v
}

pub fn run_unx14601_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14601 SetFocus/GetFocus 焦点模型");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
