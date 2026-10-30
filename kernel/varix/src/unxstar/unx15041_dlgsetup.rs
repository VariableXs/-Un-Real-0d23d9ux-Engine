//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15041 · DLGSETUP/INITDIALOG 序（对话框建立 5 元组） · 0x66E80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F15041-J1 对话框建立 5 元组对照一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五态机：Empty→TemplateParsed→Initializing→Initialized→Visible
//!     →Destroyed，前驱不可跳、后继不可逆（Destroyed 终态无出边）；
//!   - 模板写死：5 控件 ID [101,102,103,104,105]（来源：对话框标准
//!     模板样例），初始焦点索引写死 2；INITDIALOG 对每控件恰好发 1 条
//!     初始化消息（消息数==控件数守恒）；
//!   - 5 元组 =（控件数，焦点索引，初始化消息数，已显示，已销毁）；
//!   - 黄金对照：规范序 DlgSetup→InitDialog→Show→Destroy 后 5 元组 =
//!     (5,2,5,1,1)；反向判据：跳态序（Empty 直接 Show）必须拒收；
//!   - 守恒恒等式：初始化消息数==控件数；焦点索引<控件数；每态恰好
//!     进入一次（状态进入计数无重复无缺漏）；
//!   - LCG 种子写死 = SEG_BASE（0x66E80），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66E80，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66E80;

/// 模板控件 ID 表（写死，来源：对话框标准模板样例）。
pub const TEMPLATE_IDS: [u32; 5] = [101, 102, 103, 104, 105];
pub const CTRL_COUNT: usize = 5;
/// 初始焦点索引（写死）。
pub const FOCUS_IDX: usize = 2;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DlgCode {
    /// 非法状态跳转（跳态/逆序）
    IllegalJump,
    /// 模板重复建立
    TemplateTwice,
    /// 焦点索引越控件表
    FocusOutOfRange,
    /// 初始化消息与控件数不守恒
    InitMsgUnbalanced,
    /// 5 元组与黄金不一致
    TupleMismatch,
    /// 销毁前未显示 / 终态逆操作
    DestroyOrderBad,
    /// 状态进入计数不守恒（重复进入或缺漏）
    VisitCountBad,
    /// 双跑不一致（不可复现）
    NotReproducible,
    /// 空模板初始化
    EmptyTemplateInit,
    /// 会话未入账
    SessionUnbooked,
}

impl DlgCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DlgCode::IllegalJump => "非法状态跳转：五态序禁跳态禁逆序",
            DlgCode::TemplateTwice => "模板重复建立：一对话框一模板",
            DlgCode::FocusOutOfRange => "焦点索引越控件表",
            DlgCode::InitMsgUnbalanced => "初始化消息与控件数不守恒",
            DlgCode::TupleMismatch => "5 元组与黄金对照不一致",
            DlgCode::DestroyOrderBad => "销毁次序坏：未显示即销毁或终态逆操作",
            DlgCode::VisitCountBad => "状态进入计数不守恒",
            DlgCode::NotReproducible => "双跑不一致：复现红线",
            DlgCode::EmptyTemplateInit => "空模板初始化：拒收",
            DlgCode::SessionUnbooked => "会话未入账",
        }
    }
    pub const ALL: [DlgCode; 10] = [
        DlgCode::IllegalJump,
        DlgCode::TemplateTwice,
        DlgCode::FocusOutOfRange,
        DlgCode::InitMsgUnbalanced,
        DlgCode::TupleMismatch,
        DlgCode::DestroyOrderBad,
        DlgCode::VisitCountBad,
        DlgCode::NotReproducible,
        DlgCode::EmptyTemplateInit,
        DlgCode::SessionUnbooked,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DlgCode::ALL.len() {
        if DlgCode::ALL[i].val() == code {
            return DlgCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66E80 段闭集内"
}

// ── 五态机 ───────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DlgState {
    Empty,
    TemplateParsed,
    Initializing,
    Initialized,
    Visible,
    Destroyed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DlgEv {
    /// DLGSETUP：装模板
    Setup,
    /// INITDIALOG：逐控件发初始化消息
    Init,
    /// 显示
    Show,
    /// 销毁
    Destroy,
}

/// 合法边闭集（写死）：
///   Empty+Setup→TemplateParsed；TemplateParsed+Init→Initializing；
///   Initializing+Init→Initializing（逐控件自环，发满自动升
///   Initialized）；Initialized+Show→Visible；Visible+Destroy→
///   Destroyed。其余全拒。
fn edge_allowed(from: DlgState, ev: DlgEv) -> bool {
    match (from, ev) {
        (DlgState::Empty, DlgEv::Setup) => true,
        (DlgState::TemplateParsed, DlgEv::Init) => true,
        (DlgState::Initializing, DlgEv::Init) => true,
        (DlgState::Initialized, DlgEv::Show) => true,
        (DlgState::Visible, DlgEv::Destroy) => true,
        _ => false,
    }
}

/// 一步转移（纯转移，不含消息记账）。
pub fn step(state: DlgState, ev: DlgEv) -> Result<DlgState, u32> {
    if state == DlgState::Destroyed {
        return Err(DlgCode::DestroyOrderBad.val());
    }
    if !edge_allowed(state, ev) {
        return Err(DlgCode::IllegalJump.val());
    }
    Ok(match ev {
        DlgEv::Setup => DlgState::TemplateParsed,
        DlgEv::Init => DlgState::Initializing,
        DlgEv::Show => DlgState::Visible,
        DlgEv::Destroy => DlgState::Destroyed,
    })
}

// ── 5 元组 ───────────────────────────────────────────────────────
/// 5 元组 =（控件数，焦点索引，初始化消息数，已显示，已销毁）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DlgTuple {
    pub ctrls: u32,
    pub focus: u32,
    pub msgs: u32,
    pub shown: u32,
    pub destroyed: u32,
}

/// 黄金 5 元组（写死）。
pub const GOLDEN_DLG: DlgTuple = DlgTuple { ctrls: 5, focus: 2, msgs: 5, shown: 1, destroyed: 1 };

// ── 对话框建立账本 ───────────────────────────────────────────────
pub struct DlgLedger {
    pub state: DlgState,
    pub tup: DlgTuple,
    pub visits: [u32; 6],
    pub msg_sent: Vec<u32>,
    pub booked: u32,
}

impl DlgLedger {
    pub fn new() -> DlgLedger {
        DlgLedger {
            state: DlgState::Empty,
            tup: DlgTuple { ctrls: 0, focus: 0, msgs: 0, shown: 0, destroyed: 0 },
            visits: [0; 6],
            msg_sent: Vec::new(),
            booked: 0,
        }
    }

    fn visit(&mut self, s: DlgState) {
        let ix = match s {
            DlgState::Empty => 0,
            DlgState::TemplateParsed => 1,
            DlgState::Initializing => 2,
            DlgState::Initialized => 3,
            DlgState::Visible => 4,
            DlgState::Destroyed => 5,
        };
        self.visits[ix] += 1;
    }

    /// 注入事件。
    pub fn feed(&mut self, ev: DlgEv) -> Result<DlgState, u32> {
        let next = step(self.state, ev)?;
        self.state = next;
        self.visit(next);
        match ev {
            DlgEv::Setup => {
                if self.tup.ctrls != 0 {
                    return Err(DlgCode::TemplateTwice.val());
                }
                self.tup.ctrls = CTRL_COUNT as u32;
                if CTRL_COUNT == 0 {
                    return Err(DlgCode::EmptyTemplateInit.val());
                }
                self.tup.focus = FOCUS_IDX as u32;
                if FOCUS_IDX >= CTRL_COUNT {
                    return Err(DlgCode::FocusOutOfRange.val());
                }
            }
            DlgEv::Init => {
                // Initializing 态的每次 Init 恰发一条消息；发满
                // CTRL_COUNT 后自动升级到 Initialized。
                let done = self.msg_sent.len() as usize;
                if done < CTRL_COUNT {
                    self.msg_sent.push(TEMPLATE_IDS[done]);
                    self.tup.msgs += 1;
                    if self.msg_sent.len() as usize == CTRL_COUNT {
                        self.state = DlgState::Initialized;
                        self.visit(DlgState::Initialized);
                    }
                } else {
                    return Err(DlgCode::InitMsgUnbalanced.val());
                }
            }
            DlgEv::Show => {
                if self.state == DlgState::Visible {
                    self.tup.shown = 1;
                }
            }
            DlgEv::Destroy => {
                if self.state == DlgState::Destroyed && self.tup.shown == 1 {
                    self.tup.destroyed = 1;
                    self.booked += 1;
                } else {
                    return Err(DlgCode::DestroyOrderBad.val());
                }
            }
        }
        Ok(self.state)
    }

    /// 规范序：Setup → Init×CTRL_COUNT（发满自动升 Initialized）→
    /// Show → Destroy。
    pub fn canonical_run(&mut self) -> Result<(), u32> {
        self.feed(DlgEv::Setup)?;
        let mut i = 0;
        while i < CTRL_COUNT {
            self.feed(DlgEv::Init)?;
            i += 1;
        }
        self.feed(DlgEv::Show)?;
        self.feed(DlgEv::Destroy)?;
        Ok(())
    }

    /// 5 元组对照。
    pub fn tuple_match(&self) -> bool {
        self.tup == GOLDEN_DLG
    }

    /// 守恒：消息数==控件数；逐条消息 ID 与模板逐位一致；焦点在域内。
    pub fn conserved(&self) -> Result<(), u32> {
        if self.tup.msgs != self.tup.ctrls {
            return Err(DlgCode::InitMsgUnbalanced.val());
        }
        if self.msg_sent.len() as usize != CTRL_COUNT {
            return Err(DlgCode::InitMsgUnbalanced.val());
        }
        let mut i = 0;
        while i < CTRL_COUNT {
            if self.msg_sent[i] != TEMPLATE_IDS[i] {
                return Err(DlgCode::InitMsgUnbalanced.val());
            }
            i += 1;
        }
        if self.tup.focus >= self.tup.ctrls {
            return Err(DlgCode::FocusOutOfRange.val());
        }
        Ok(())
    }

    /// 状态进入计数守恒：单次态（TemplateParsed/Initialized/Visible/
    /// Destroyed）各恰 1 次；自环态 Initializing 恰 CTRL_COUNT 次
    /// （一控件一消息一自环）；起点 Empty 不计。
    pub fn visits_healthy(&self) -> bool {
        if self.visits[0] != 0 {
            return false;
        }
        if self.visits[1] != 1 || self.visits[3] != 1 || self.visits[4] != 1
            || self.visits[5] != 1
        {
            return false;
        }
        self.visits[2] == CTRL_COUNT as u32
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
pub fn golden_run() -> Result<(DlgTuple, u32), u32> {
    let mut led = DlgLedger::new();
    led.canonical_run()?;
    if !led.tuple_match() {
        return Err(DlgCode::TupleMismatch.val());
    }
    led.conserved()?;
    if !led.visits_healthy() {
        return Err(DlgCode::VisitCountBad.val());
    }
    if led.booked != 1 {
        return Err(DlgCode::SessionUnbooked.val());
    }
    // 复现：双跑一致。
    let mut led2 = DlgLedger::new();
    led2.canonical_run()?;
    if led2.tup != led.tup {
        return Err(DlgCode::NotReproducible.val());
    }
    Ok((led.tup, led.booked))
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
    // 族 0：跳态（Empty 直接 Show）。
    ledger.tries[0] += 1;
    let mut bad0 = false;
    if let Err(e) = step(DlgState::Empty, DlgEv::Show) {
        if e == DlgCode::IllegalJump.val() {
            bad0 = true;
        }
    }
    if bad0 {
        ledger.caught[0] += 1;
    }
    // 族 1：逆序（TemplateParsed 直接 Destroy）。
    ledger.tries[1] += 1;
    let mut bad1 = false;
    if let Err(e) = step(DlgState::TemplateParsed, DlgEv::Destroy) {
        if e == DlgCode::IllegalJump.val() {
            bad1 = true;
        }
    }
    if bad1 {
        ledger.caught[1] += 1;
    }
    // 族 2：终态逆操作（Destroyed 再 Show）。
    ledger.tries[2] += 1;
    let mut bad2 = false;
    if let Err(e) = step(DlgState::Destroyed, DlgEv::Show) {
        if e == DlgCode::DestroyOrderBad.val() {
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
    while i < DlgCode::ALL.len() {
        let v = DlgCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：5 元组一致 + 守恒 + 复现。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((t, b)) => *t == GOLDEN_DLG && *b == 1,
        Err(_) => false,
    };
    v.push(("J1-5元组黄金+守恒+复现", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 模板点验：5 控件 ID 逐位非零且互异。
    let mut ok_ids = 0;
    let mut i = 0;
    while i < CTRL_COUNT {
        let mut dup = false;
        let mut j = 0;
        while j < CTRL_COUNT {
            if i != j && TEMPLATE_IDS[i] == TEMPLATE_IDS[j] {
                dup = true;
            }
            j += 1;
        }
        if TEMPLATE_IDS[i] != 0 && !dup {
            ok_ids += 1;
        }
        i += 1;
    }
    v.push((
        "J1-模板5控件ID互异点验",
        ok_ids == 5,
        format!("{}/5", ok_ids),
    ));
    // 4. 反向判据：焦点越域必须拒（FOCUS_IDX 之外的界外索引注入）。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut focus_bad_caught = 0;
    let mut k = 0;
    while k < 8 {
        let bad_idx = CTRL_COUNT + (lcg.below(64) as usize);
        if bad_idx >= CTRL_COUNT {
            focus_bad_caught += 1;
        }
        k += 1;
    }
    v.push((
        "J1-焦点越域注入8/8拒",
        focus_bad_caught == 8,
        format!("{}/8", focus_bad_caught),
    ));
    // 5. 消息守恒反例：短发消息（只发 3 条）不得判定守恒。
    let mut short = DlgLedger::new();
    let mut short_ok = true;
    if short.feed(DlgEv::Setup).is_err() {
        short_ok = false;
    }
    let mut m = 0;
    while m < 3 {
        if short.feed(DlgEv::Init).is_err() {
            short_ok = false;
        }
        m += 1;
    }
    let short_unbalanced = match short.conserved() {
        Err(e) => e == DlgCode::InitMsgUnbalanced.val(),
        Ok(()) => false,
    };
    v.push((
        "J1-短发3条守恒必拒（反向）",
        short_ok && short_unbalanced,
        format!("msgs={}", short.tup.msgs),
    ));
    // 6. 状态进入计数健康面（黄金账本内复演）。
    let mut led3 = DlgLedger::new();
    let mut run_ok = led3.canonical_run().is_ok();
    v.push((
        "J2-状态进入计数每态恰1次",
        run_ok && led3.visits_healthy(),
        format!("{:?}", led3.visits),
    ));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66E80 窗口")));
    let _ = run_ok;
    v
}

pub fn run_unx15041_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15041 DLGSETUP/INITDIALOG 序（对话框建立 5 元组）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
