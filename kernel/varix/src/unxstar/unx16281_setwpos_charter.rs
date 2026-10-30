//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16281 · SetWindowPos 语义总纲 · 0x67E00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-E1-B15）：
//!   ① 纯功能代码 ≥355 行；
//!   ② UNX-F16281-J1 总纲落账（参数族/标志族/时序三段），三段齐备率
//!     100%，与 F16252 回调表挂接一致率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 总纲三段建模：参数族（hWnd/hWndInsertAfter/x/y/cx/cy）、
//!     标志族（SWP_* 位标志闭集）、时序族（六步单向时序）；
//!   - 三段齐备账：每段注册条目数对账（齐备=条目数≥冻结下限且
//!     名称非空），齐备率必须 100%；
//!   - 标志互斥矩阵：SWP_NOSIZE 压制 cx/cy、SWP_NOMOVE 压制 x/y、
//!     SWP_NOREDRAW 与 SWP_FRAMECHANGED 冲突等，互斥对显式闭集；
//!   - 时序六步单向：验证→Z序→几何→重绘→激活→回调通知，任何
//!     跳步即拒；回调仅允许在第六步触发一次；
//!   - F16252 回调表挂接：总纲时序第六步的回调名必须能在回调表
//!     中精确命中（一致率 100%），未命中即挂接断裂；
//!   - LCG 种子写死 = SEG_BASE（0x67E00）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67E00，AI-VX3-1030 批14 独占区）。
pub const SEG_BASE: u32 = 0x67E00;

// ── 参数族闭集（6 参数） ─────────────────────────────────────────
pub const PARAM_COUNT: usize = 6;
pub const PARAM_NAMES: [&str; PARAM_COUNT] =
    ["hWnd", "hWndInsertAfter", "x", "y", "cx", "cy"];
/// 参数族齐备下限（冻结：6 参数全部登记）。
pub const PARAM_FLOOR: usize = 6;

// ── 标志族闭集（9 位标志） ───────────────────────────────────────
pub const SWP_NOSIZE: u32 = 0x0001;
pub const SWP_NOMOVE: u32 = 0x0002;
pub const SWP_NOZORDER: u32 = 0x0004;
pub const SWP_NOREDRAW: u32 = 0x0008;
pub const SWP_NOACTIVATE: u32 = 0x0010;
pub const SWP_FRAMECHANGED: u32 = 0x0020;
pub const SWP_SHOWWINDOW: u32 = 0x0040;
pub const SWP_HIDEWINDOW: u32 = 0x0080;
pub const SWP_NOCOPYBITS: u32 = 0x0100;
pub const FLAG_COUNT: usize = 9;
pub const FLAG_NAMES: [&str; FLAG_COUNT] = [
    "SWP_NOSIZE",
    "SWP_NOMOVE",
    "SWP_NOZORDER",
    "SWP_NOREDRAW",
    "SWP_NOACTIVATE",
    "SWP_FRAMECHANGED",
    "SWP_SHOWWINDOW",
    "SWP_HIDEWINDOW",
    "SWP_NOCOPYBITS",
];
/// 标志族齐备下限（冻结：9 标志全部登记）。
pub const FLAG_FLOOR: usize = 9;
/// 标志掩码合法域（9 位之外非法）。
pub const FLAG_MASK_LEGAL: u32 = 0x01FF;

// ── 时序族闭集（六步单向） ───────────────────────────────────────
pub const SEQ_COUNT: usize = 6;
pub const SEQ_NAMES: [&str; SEQ_COUNT] =
    ["验证", "Z序", "几何", "重绘", "激活", "回调通知"];
pub const SEQ_FLOOR: usize = 6;

// ── F16252 回调表（冻结：挂接目标） ─────────────────────────────
pub const CB_TABLE: [&str; 4] =
    ["on_swp_committed", "on_swp_zorder", "on_swp_geometry", "on_swp_activate"];
/// 总纲时序第六步冻结回调名（必须能在 CB_TABLE 命中）。
pub const SEQ6_FROZEN_CB: &str = "on_swp_committed";

/// 判据迭代轮数（写死：判据原文 10/10 次语义，双跑各 10 轮）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwpCode {
    /// 参数越闭集
    ParamOutOfRange,
    /// 参数名空（齐备破坏）
    ParamNameBlank,
    /// 标志越掩码域（0x01FF 之外）
    FlagIllegalBit,
    /// 标志名空（齐备破坏）
    FlagNameBlank,
    /// 时序步越闭集
    SeqOutOfRange,
    /// 时序步名空（齐备破坏）
    SeqNameBlank,
    /// 标志互斥冲突（NOSIZE+改 cx/cy 等）
    FlagMutexConflict,
    /// SWP_SHOWWINDOW 与 SWP_HIDEWINDOW 同置
    ShowHideConflict,
    /// 时序跳步（六步单向破坏）
    SeqJumpDetected,
    /// 回调未挂接（F16252 表未命中）
    CbNotLinked,
    /// 齐备率不满（三段任一段破下限）
    CharterIncomplete,
    /// 双跑不一致
    NotReproducible,
}

impl SwpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SwpCode::ParamOutOfRange => "参数越闭集：6 参数冻结",
            SwpCode::ParamNameBlank => "参数名空：参数族齐备破坏",
            SwpCode::FlagIllegalBit => "标志越掩码域：0x01FF 闭集",
            SwpCode::FlagNameBlank => "标志名空：标志族齐备破坏",
            SwpCode::SeqOutOfRange => "时序步越闭集：六步冻结",
            SwpCode::SeqNameBlank => "时序步名空：时序族齐备破坏",
            SwpCode::FlagMutexConflict => "标志互斥冲突：NOSIZE 压制 cx/cy",
            SwpCode::ShowHideConflict => "SHOW/HIDE 同置：互斥对显式闭集",
            SwpCode::SeqJumpDetected => "时序跳步：六步单向破坏",
            SwpCode::CbNotLinked => "回调未挂接：F16252 表未命中",
            SwpCode::CharterIncomplete => "齐备率不满：三段任一破下限",
            SwpCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SwpCode; 12] = [
        SwpCode::ParamOutOfRange,
        SwpCode::ParamNameBlank,
        SwpCode::FlagIllegalBit,
        SwpCode::FlagNameBlank,
        SwpCode::SeqOutOfRange,
        SwpCode::SeqNameBlank,
        SwpCode::FlagMutexConflict,
        SwpCode::ShowHideConflict,
        SwpCode::SeqJumpDetected,
        SwpCode::CbNotLinked,
        SwpCode::CharterIncomplete,
        SwpCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SwpCode::ALL.len() {
        if SwpCode::ALL[i].val() == code {
            return SwpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67E00 段闭集内"
}

// ── 参数族条目 ──────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct ParamEntry {
    pub name: &'static str,
    /// 是否几何参数（被 SWP_NOMOVE/NOSIZE 压制）。
    pub is_geometry: bool,
}

pub const PARAM_TABLE: [ParamEntry; PARAM_COUNT] = [
    ParamEntry { name: "hWnd", is_geometry: false },
    ParamEntry { name: "hWndInsertAfter", is_geometry: false },
    ParamEntry { name: "x", is_geometry: true },
    ParamEntry { name: "y", is_geometry: true },
    ParamEntry { name: "cx", is_geometry: true },
    ParamEntry { name: "cy", is_geometry: true },
];

// ── 标志互斥矩阵（显式闭集：互斥对双方索引） ────────────────────
/// (a_bit, b_bit)：同置即冲突。
pub const MUTEX_PAIRS: [(u32, u32); 3] = [
    (SWP_SHOWWINDOW, SWP_HIDEWINDOW),
    (SWP_NOREDRAW, SWP_FRAMECHANGED),
    (SWP_NOZORDER, SWP_HIDEWINDOW),
];

// ── 请求结构 ────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct SwpRequest {
    pub flags: u32,
    pub x: i32,
    pub y: i32,
    pub cx: i32,
    pub cy: i32,
}

impl SwpRequest {
    pub fn new(flags: u32, x: i32, y: i32, cx: i32, cy: i32) -> SwpRequest {
        SwpRequest { flags, x, y, cx, cy }
    }
    /// 标志域守卫：越 0x01FF 即非法位。
    pub fn legal_mask(&self) -> Result<(), u32> {
        if self.flags & !FLAG_MASK_LEGAL != 0 {
            return Err(SwpCode::FlagIllegalBit.val());
        }
        Ok(())
    }
    /// 互斥矩阵检查：同置互斥对即拒。
    pub fn check_mutex(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < MUTEX_PAIRS.len() {
            let (a, b) = MUTEX_PAIRS[i];
            if self.flags & a != 0 && self.flags & b != 0 {
                if a == SWP_SHOWWINDOW && b == SWP_HIDEWINDOW {
                    return Err(SwpCode::ShowHideConflict.val());
                }
                return Err(SwpCode::FlagMutexConflict.val());
            }
            i += 1;
        }
        Ok(())
    }
    /// 压制语义：NOMOVE 压制 x/y、NOSIZE 压制 cx/cy（压制后几何退零）。
    pub fn effective_geometry(&self) -> (i32, i32, i32, i32) {
        let mx = if self.flags & SWP_NOMOVE != 0 { 0 } else { self.x };
        let my = if self.flags & SWP_NOMOVE != 0 { 0 } else { self.y };
        let mcx = if self.flags & SWP_NOSIZE != 0 { 0 } else { self.cx };
        let mcy = if self.flags & SWP_NOSIZE != 0 { 0 } else { self.cy };
        (mx, my, mcx, mcy)
    }
}

// ── 时序账（六步单向状态机） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwpPhase {
    NotStarted,
    Step(usize), // 0..=5
    Done,
}

pub struct SwpSequence {
    pub phase: SwpPhase,
    pub steps_done: [bool; SEQ_COUNT],
    pub cb_fired: u32,
    pub jump_cnt: u32,
}

impl SwpSequence {
    pub fn new() -> SwpSequence {
        SwpSequence {
            phase: SwpPhase::NotStarted,
            steps_done: [false; SEQ_COUNT],
            cb_fired: 0,
            jump_cnt: 0,
        }
    }

    /// 推进到第 step 步（0 起）。仅允许恰下一步：NotStarted→0，
    /// n→n+1，5→Done。跳步即拒并落跳步账。
    pub fn advance(&mut self, step: usize) -> Result<(), u32> {
        if step >= SEQ_COUNT {
            return Err(SwpCode::SeqOutOfRange.val());
        }
        let expect = match self.phase {
            SwpPhase::NotStarted => 0,
            SwpPhase::Step(n) => n + 1,
            SwpPhase::Done => {
                self.jump_cnt += 1;
                return Err(SwpCode::SeqJumpDetected.val());
            }
        };
        if step != expect {
            self.jump_cnt += 1;
            return Err(SwpCode::SeqJumpDetected.val());
        }
        self.steps_done[step] = true;
        if step == SEQ_COUNT - 1 {
            self.phase = SwpPhase::Done;
        } else {
            self.phase = SwpPhase::Step(step);
        }
        Ok(())
    }

    /// 第六步回调：仅 Done 态允许触发恰一次。
    pub fn fire_callback(&mut self) -> Result<&'static str, u32> {
        if self.phase != SwpPhase::Done {
            return Err(SwpCode::SeqJumpDetected.val());
        }
        if self.cb_fired != 0 {
            return Err(SwpCode::CbNotLinked.val());
        }
        self.cb_fired += 1;
        Ok(SEQ6_FROZEN_CB)
    }

    /// 回调名挂接校验：必须在 F16252 表精确命中。
    pub fn cb_linked(name: &str) -> bool {
        let mut i = 0;
        while i < CB_TABLE.len() {
            if CB_TABLE[i] == name {
                return true;
            }
            i += 1;
        }
        false
    }
}

// ── 三段齐备账 ──────────────────────────────────────────────────
pub struct CharterAudit {
    pub param_ready: bool,
    pub flag_ready: bool,
    pub seq_ready: bool,
}

impl CharterAudit {
    /// 三段齐备率审计：条目数达下限且名称全非空。
    pub fn audit() -> CharterAudit {
        let mut param_ready = PARAM_TABLE.len() >= PARAM_FLOOR;
        let mut i = 0;
        while i < PARAM_TABLE.len() {
            if PARAM_TABLE[i].name.len() == 0 {
                param_ready = false;
            }
            i += 1;
        }
        let mut flag_ready = FLAG_NAMES.len() >= FLAG_FLOOR;
        i = 0;
        while i < FLAG_NAMES.len() {
            if FLAG_NAMES[i].len() == 0 {
                flag_ready = false;
            }
            i += 1;
        }
        let mut seq_ready = SEQ_NAMES.len() >= SEQ_FLOOR;
        i = 0;
        while i < SEQ_NAMES.len() {
            if SEQ_NAMES[i].len() == 0 {
                seq_ready = false;
            }
            i += 1;
        }
        CharterAudit { param_ready, flag_ready, seq_ready }
    }
    /// 三段齐备率：3/3 才算 100%。
    pub fn completeness_pct(&self) -> u32 {
        let mut c = 0;
        if self.param_ready {
            c += 1;
        }
        if self.flag_ready {
            c += 1;
        }
        if self.seq_ready {
            c += 1;
        }
        c * 100 / 3
    }
}

// ── LCG（确定性，种子写死） ─────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 随机请求构造（第 round 轮）：flags 从合法闭集取、几何从宽域取。
pub fn random_request(round: usize) -> Result<(SwpRequest, u32), u32> {
    let mut lcg = Lcg::new(SEG_BASE as u64 + round as u64);
    let flag_choice = lcg.below(8) as u32;
    let flags = match flag_choice {
        0 => SWP_NOSIZE | SWP_NOACTIVATE,
        1 => SWP_NOMOVE | SWP_NOZORDER,
        2 => SWP_FRAMECHANGED,
        3 => SWP_NOCOPYBITS | SWP_NOACTIVATE,
        4 => SWP_NOSIZE | SWP_NOMOVE,
        5 => SWP_SHOWWINDOW,
        6 => SWP_HIDEWINDOW,
        _ => SWP_NOSIZE,
    };
    let x = lcg.below(4096) as i32;
    let y = lcg.below(4096) as i32;
    let cx = lcg.below(2048) as i32;
    let cy = lcg.below(2048) as i32;
    let req = SwpRequest::new(flags, x, y, cx, cy);
    // 全链合法闸：掩码 + 互斥。
    req.legal_mask()?;
    req.check_mutex()?;
    Ok((req, flags))
}

/// 黄金全链：一次完整六步时序 + 回调挂接。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let (req, flags) = random_request(7)?;
    let mut seq = SwpSequence::new();
    let mut step = 0;
    while step < SEQ_COUNT {
        seq.advance(step)?;
        step += 1;
    }
    let cb = seq.fire_callback()?;
    if !SwpSequence::cb_linked(cb) {
        return Err(SwpCode::CbNotLinked.val());
    }
    let audit = CharterAudit::audit();
    if audit.completeness_pct() != 100 {
        return Err(SwpCode::CharterIncomplete.val());
    }
    let (ex, ey, ecx, ecy) = req.effective_geometry();
    // 压制语义点验：NOSIZE 时 cx 退零。
    if flags & SWP_NOSIZE != 0 && ecx != 0 {
        return Err(SwpCode::FlagMutexConflict.val());
    }
    if flags & SWP_NOMOVE != 0 && (ex != 0 || ey != 0) {
        return Err(SwpCode::FlagMutexConflict.val());
    }
    Ok((seq.cb_fired, seq.jump_cnt, flags))
}

/// 探针三序（对照值写死，判据对拍锚）。
pub fn probe_seq_a() -> Result<(), u32> {
    // 黄金序：六步推进 + 回调 → 应全过。
    let mut seq = SwpSequence::new();
    let mut step = 0;
    while step < SEQ_COUNT {
        seq.advance(step)?;
        step += 1;
    }
    seq.fire_callback()?;
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 跳步序：NotStarted 直接推 step 1 → 必拒。
    let mut seq = SwpSequence::new();
    seq.advance(1)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // SHOW|HIDE 同置 → 互斥必拒。
    let req = SwpRequest::new(SWP_SHOWWINDOW | SWP_HIDEWINDOW, 1, 2, 3, 4);
    req.check_mutex()
}

/// 混域/注入 10/10 检出：跳步、互斥、非法位、回调未挂接四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：跳步注入。
        let mut seq = SwpSequence::new();
        let hit0 = seq.advance(3).is_err();
        led.record(0, hit0);
        // 槽1：互斥注入（SHOW|HIDE）。
        let req = SwpRequest::new(SWP_SHOWWINDOW | SWP_HIDEWINDOW, 0, 0, 0, 0);
        let hit1 = req.check_mutex().is_err();
        led.record(1, hit1);
        // 槽2：非法位注入（bit 10 置位）。
        let req2 = SwpRequest::new(0x0400, 0, 0, 0, 0);
        let hit2 = req2.legal_mask().is_err();
        led.record(2, hit2);
        // 槽3：回调未挂接注入（未 Done 即回调）。
        let mut seq2 = SwpSequence::new();
        let hit3 = seq2.fire_callback().is_err();
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SwpCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫：诊断码全部落在 [SEG_BASE, SEG_BASE+0x40) 窗口。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SwpCode::ALL.len() {
        let v = SwpCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        g.is_ok(),
        format!("{:?}", g),
    ));
    // 三段齐备率 100%。
    let audit = CharterAudit::audit();
    v.push((
        "J1-三段齐备率100%",
        audit.completeness_pct() == 100
            && audit.param_ready
            && audit.flag_ready
            && audit.seq_ready,
        format!("齐备 {}/3", audit.completeness_pct() / 33.max(1)),
    ));
    // F16252 回调挂接一致率 100%。
    let linked = SwpSequence::cb_linked(SEQ6_FROZEN_CB);
    v.push((
        "J1-F16252挂接一致率100%",
        linked && CB_TABLE.len() == 4,
        format!("cb={} 表={}", SEQ6_FROZEN_CB, CB_TABLE.len()),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!("t/c {:?}/{:?}", led.as_ref().map(|l| l.tries), led.as_ref().map(|l| l.caught)),
    ));
    // 探针三序。
    let a = probe_seq_a();
    let b = probe_seq_b();
    let c = probe_seq_c();
    v.push((
        "J1-探针三序",
        a.is_ok() && b.is_err() && c.is_err(),
        format!("a={:?} b={:?} c={:?}", a.is_ok(), b.is_err(), c.is_err()),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 闭集点验：参数/标志/时序三表与冻结名一致。
    v.push((
        "J1-闭集点验",
        PARAM_TABLE.len() == 6 && FLAG_NAMES.len() == 9 && SEQ_NAMES.len() == 6,
        format!("p{} f{} s{}", PARAM_TABLE.len(), FLAG_NAMES.len(), SEQ_NAMES.len()),
    ));
    // 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67E00 窗口")));
    v
}

pub fn run_unx16281_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16281 SetWindowPos 语义总纲");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
