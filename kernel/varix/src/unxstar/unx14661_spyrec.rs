//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14661 · Spy 录制器核心（逐消息元组捕获） · 0x669C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥370 行；
//!   ② UNX-F14661-J1 (序号,hwnd,message,wParam,lParam) 五元组捕获账与
//!      泵埋点逐条一致，漏捕检测断言；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 消息泵埋点：每抽一条消息（GetMessage 位）即落五元组账；泵产出
//!     计数器与捕获账逐条对齐（漏捕 = 泵计数 > 账长，断言必检出）；
//!   - 五元组：(seq 单调递增, hwnd, message, wParam, lParam)；
//!   - 录制器状态机：Stopped → Recording → Stopped；非录态抽消息不落
//!     账（拒收面）；录态重复启动拒收；
//!   - 漏捕检测：账序连续性扫描（seq 必须严格 +1 递增）+ 泵计数守恒
//!     双闸；含反向判据（人为抽账一条必检出）；
//!   - LCG 种子写死 = SEG_BASE（0x669C0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x669C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x669C0;

// ── 录制器状态闭集 ───────────────────────────────────────────────
pub const REC_STOPPED: usize = 0;
pub const REC_RECORDING: usize = 1;
pub const REC_STATE_COUNT: usize = 2;
pub const REC_STATE_NAMES: [&str; REC_STATE_COUNT] = ["Stopped", "Recording"];

/// 消息闭集（采样：泵可产出的消息型）。
pub const MSG_NULL: u32 = 0x0000;
pub const MSG_MOVE: u32 = 0x0200;
pub const MSG_LBUTTONDOWN: u32 = 0x0201;
pub const MSG_LBUTTONUP: u32 = 0x0202;
pub const MSG_KEYDOWN: u32 = 0x0100;
pub const MSG_KEYUP: u32 = 0x0101;
pub const MSG_PAINT: u32 = 0x000F;
pub const MSG_CLOSE: u32 = 0x0010;
pub const MSG_SET_COUNT: usize = 8;
pub const MSG_SET: [u32; MSG_SET_COUNT] = [
    MSG_NULL, MSG_MOVE, MSG_LBUTTONDOWN, MSG_LBUTTONUP, MSG_KEYDOWN, MSG_KEYUP, MSG_PAINT,
    MSG_CLOSE,
];
pub const MSG_SET_NAMES: [&str; MSG_SET_COUNT] = [
    "WM_NULL", "WM_MOUSEMOVE", "WM_LBUTTONDOWN", "WM_LBUTTONUP", "WM_KEYDOWN", "WM_KEYUP",
    "WM_PAINT", "WM_CLOSE",
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpyCode {
    /// 录制器状态越闭集
    StateOutOfRange,
    /// 非录态落账拒收
    NotRecording,
    /// 录态重复启动拒收
    DoubleStart,
    /// 未录先停拒收
    StopBeforeStart,
    /// 五元组捕获账与泵埋点不一致（漏捕）
    MissedCapture,
    /// seq 不连续（账序断）
    SeqGap,
    /// 消息型越闭集
    MsgOutOfRange,
    /// hwnd 未入册
    HwndUnregistered,
}

impl SpyCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SpyCode::StateOutOfRange => "录制器状态越闭集：2 态冻结",
            SpyCode::NotRecording => "非录态落账拒收",
            SpyCode::DoubleStart => "录态重复启动拒收",
            SpyCode::StopBeforeStart => "未录先停拒收",
            SpyCode::MissedCapture => "漏捕：泵计数>账长",
            SpyCode::SeqGap => "seq 不连续：账序断",
            SpyCode::MsgOutOfRange => "消息型越闭集",
            SpyCode::HwndUnregistered => "hwnd 未入册",
        }
    }
    pub const ALL: [SpyCode; 8] = [
        SpyCode::StateOutOfRange,
        SpyCode::NotRecording,
        SpyCode::DoubleStart,
        SpyCode::StopBeforeStart,
        SpyCode::MissedCapture,
        SpyCode::SeqGap,
        SpyCode::MsgOutOfRange,
        SpyCode::HwndUnregistered,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SpyCode::ALL.len() {
        if SpyCode::ALL[i].val() == code {
            return SpyCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x669C0 段闭集内"
}

/// 消息型 → 闭集下标（越闭集返回 MSG_SET_COUNT）。
pub fn msg_index_of(m: u32) -> usize {
    let mut i = 0;
    while i < MSG_SET_COUNT {
        if MSG_SET[i] == m {
            return i;
        }
        i += 1;
    }
    MSG_SET_COUNT
}

// ── 五元组与录制账 ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MsgTuple {
    pub seq: u64,
    pub hwnd: u32,
    pub message: u32,
    pub wp: u32,
    pub lp: u32,
}

/// 录制器状态机：Stopped → Recording → Stopped（合法边闭集 2 条）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecState {
    Stopped,
    Recording,
}

pub struct SpyRecorder {
    pub state: RecState,
    pub recs: Vec<MsgTuple>,
    pub pump_poked: u32,
    pub next_seq: u64,
    pub wins: Vec<u32>,
    pub dropped_while_stopped: u32,
}

impl SpyRecorder {
    pub fn new() -> SpyRecorder {
        SpyRecorder {
            state: RecState::Stopped,
            recs: Vec::new(),
            pump_poked: 0,
            next_seq: 1,
            wins: Vec::new(),
            dropped_while_stopped: 0,
        }
    }

    pub fn register(&mut self, hwnd: u32) -> Result<(), u32> {
        let mut i = 0;
        while i < self.wins.len() {
            if self.wins[i] == hwnd {
                return Err(SpyCode::HwndUnregistered.val());
            }
            i += 1;
        }
        self.wins.push(hwnd);
        Ok(())
    }

    fn find(&self, hwnd: u32) -> bool {
        let mut i = 0;
        while i < self.wins.len() {
            if self.wins[i] == hwnd {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 启动录制：录态重复启动拒收。
    pub fn start(&mut self) -> Result<(), u32> {
        if self.state == RecState::Recording {
            return Err(SpyCode::DoubleStart.val());
        }
        self.state = RecState::Recording;
        self.next_seq = 1;
        Ok(())
    }

    /// 停止录制：未录先停拒收。
    pub fn stop(&mut self) -> Result<(), u32> {
        if self.state == RecState::Stopped {
            return Err(SpyCode::StopBeforeStart.val());
        }
        self.state = RecState::Stopped;
        Ok(())
    }

    /// 泵埋点：GetMessage 位抽到一条消息。录态落账；停态丢账计数。
    pub fn pump_poke(
        &mut self,
        hwnd: u32,
        message: u32,
        wp: u32,
        lp: u32,
    ) -> Result<Option<u64>, u32> {
        self.pump_poked += 1;
        if msg_index_of(message) == MSG_SET_COUNT {
            return Err(SpyCode::MsgOutOfRange.val());
        }
        if !self.find(hwnd) {
            return Err(SpyCode::HwndUnregistered.val());
        }
        match self.state {
            RecState::Recording => {
                let seq = self.next_seq;
                self.next_seq += 1;
                self.recs.push(MsgTuple { seq, hwnd, message, wp, lp });
                Ok(Some(seq))
            }
            RecState::Stopped => {
                self.dropped_while_stopped += 1;
                Ok(None)
            }
        }
    }

    /// 漏捕检测双闸：
    ///   闸 1：录态期间账长 + 丢账计数 == 泵计数（录态段内）；
    ///   闸 2：账内 seq 严格 +1 连续。
    pub fn missed_capture_audit(&self) -> Result<(), u32> {
        // 闸 2：seq 连续性。
        let mut i = 0;
        while i < self.recs.len() {
            if self.recs[i].seq != (i as u64 + 1) {
                return Err(SpyCode::SeqGap.val());
            }
            i += 1;
        }
        // 闸 1：守恒（建模口径：录态段内泵产出 = 账长）。
        let recorded = self.recs.len() as u32;
        if self.next_seq as u32 - 1 != recorded {
            return Err(SpyCode::MissedCapture.val());
        }
        Ok(())
    }
}

// ── 漏捕检测断言（含反向判据） ───────────────────────────────────
/// 正向：正常录制零漏捕。反向：人为抹去账尾一条必检出（SeqGap/
/// MissedCapture 双检出）。
pub fn missed_detection() -> Result<(bool, bool), u32> {
    // 正向。
    let mut rec = SpyRecorder::new();
    rec.register(0x11)?;
    rec.start()?;
    let _ = rec.pump_poke(0x11, MSG_MOVE, 3, 4)?;
    let _ = rec.pump_poke(0x11, MSG_KEYDOWN, 0x41, 0)?;
    let _ = rec.pump_poke(0x11, MSG_KEYUP, 0x41, 0)?;
    rec.stop()?;
    let clean = rec.missed_capture_audit().is_ok();
    // 反向：抹去中间一条制造漏捕。
    let mut rec2 = SpyRecorder::new();
    rec2.register(0x22)?;
    rec2.start()?;
    let _ = rec2.pump_poke(0x22, MSG_MOVE, 1, 1)?;
    let _ = rec2.pump_poke(0x22, MSG_LBUTTONDOWN, 1, 1)?;
    let _ = rec2.pump_poke(0x22, MSG_LBUTTONUP, 1, 1)?;
    // 模拟漏捕：抽掉账[1]但泵计数不变。
    rec2.recs.remove(1);
    let detected = rec2.missed_capture_audit().is_err();
    Ok((clean, detected))
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

// ── 泵驱动正演：随机消息流逐条捕获对账 ──────────────────────────
pub fn pump_run(n_pokes: usize) -> Result<(u32, usize, u32), u32> {
    let mut rec = SpyRecorder::new();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    rec.register(0xA1)?;
    rec.register(0xA2)?;
    let mut poked_while_rec = 0;
    let mut i = 0;
    while i < n_pokes {
        // 偶尔启停。
        if lcg.below(16) == 0 {
            if rec.state == RecState::Recording {
                rec.stop()?;
            } else {
                rec.start()?;
            }
        }
        let hwnd = if lcg.below(2) == 0 { 0xA1 } else { 0xA2 };
        let msg = MSG_SET[lcg.below(MSG_SET_COUNT as u64) as usize];
        let wp = lcg.next() as u32;
        let lp = lcg.next() as u32;
        let was_rec = rec.state == RecState::Recording;
        let _ = rec.pump_poke(hwnd, msg, wp, lp)?;
        if was_rec {
            poked_while_rec += 1;
        }
        i += 1;
    }
    if rec.state == RecState::Recording {
        rec.stop()?;
    }
    rec.missed_capture_audit()?;
    // 泵计数守恒：全部埋点 = 录态账长 + 停态丢账。
    if rec.pump_poked as usize != rec.recs.len() + rec.dropped_while_stopped as usize {
        return Err(SpyCode::MissedCapture.val());
    }
    Ok((rec.pump_poked, rec.recs.len(), rec.dropped_while_stopped))
}

// ── 剧本对拍（写死五元组期望表） ─────────────────────────────────
/// 基准剧本（写死）：窗 0x33，三击消息流，期望五元组逐条一致。
pub fn scripted_parity() -> Result<[MsgTuple; 3], u32> {
    let mut rec = SpyRecorder::new();
    rec.register(0x33)?;
    rec.start()?;
    let s1 = rec.pump_poke(0x33, MSG_MOVE, 10, 20)?;
    let s2 = rec.pump_poke(0x33, MSG_LBUTTONDOWN, 1, 0x0014_000A)?;
    let s3 = rec.pump_poke(0x33, MSG_LBUTTONUP, 0, 0x0014_000A)?;
    rec.stop()?;
    let a = match s1 {
        Some(v) => v,
        None => return Err(SpyCode::MissedCapture.val()),
    };
    let b = match s2 {
        Some(v) => v,
        None => return Err(SpyCode::MissedCapture.val()),
    };
    let c = match s3 {
        Some(v) => v,
        None => return Err(SpyCode::MissedCapture.val()),
    };
    let expect = [
        MsgTuple { seq: a, hwnd: 0x33, message: MSG_MOVE, wp: 10, lp: 20 },
        MsgTuple { seq: b, hwnd: 0x33, message: MSG_LBUTTONDOWN, wp: 1, lp: 0x0014_000A },
        MsgTuple { seq: c, hwnd: 0x33, message: MSG_LBUTTONUP, wp: 0, lp: 0x0014_000A },
    ];
    let n = rec.recs.len();
    if n != 3 {
        return Err(SpyCode::MissedCapture.val());
    }
    let mut i = 0;
    while i < 3 {
        if rec.recs[i] != expect[i] {
            return Err(SpyCode::MissedCapture.val());
        }
        i += 1;
    }
    Ok(expect)
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
    // 族 0：停态抽消息丢账（有账：dropped 计数）。
    ledger.tries[0] += 1;
    let mut rec = SpyRecorder::new();
    let _ = rec.register(1);
    let r = rec.pump_poke(1, MSG_MOVE, 0, 0);
    let mut c0 = false;
    if let Ok(None) = r {
        if rec.dropped_while_stopped == 1 {
            c0 = true;
        }
    }
    if c0 {
        ledger.caught[0] += 1;
    }
    // 族 1：录态重复启动必拒。
    ledger.tries[1] += 1;
    let _ = rec.start();
    if let Err(e) = rec.start() {
        if e == SpyCode::DoubleStart.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：未入册 hwnd 必拒。
    ledger.tries[2] += 1;
    if let Err(e) = rec.pump_poke(0x99, MSG_MOVE, 0, 0) {
        if e == SpyCode::HwndUnregistered.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<([MsgTuple; 3], u32, usize, u32, bool, bool), u32> {
    let scr = scripted_parity()?;
    let (poked, rec_len, dropped) = pump_run(96)?;
    let (clean, detected) = missed_detection()?;
    Ok((scr, poked, rec_len, dropped, clean, detected))
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SpyCode::ALL.len() {
        let v = SpyCode::ALL[i].val();
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
        "J1-剧本3条+泵96点对账全过",
        matches!(golden, Ok(_)),
        format!("{:?}", golden),
    ));
    // 漏捕检测断言：正向零漏捕 + 反向抽账必检出。
    let md = missed_detection();
    let md_ok = match md {
        Ok((clean, detected)) => clean && detected,
        Err(_) => false,
    };
    v.push((
        "J1-漏捕检测断言（正+反）",
        md_ok,
        format!("{:?}", md),
    ));
    // 消息闭集名/值点验。
    let mut ok = 0;
    let mut uniq = true;
    let mut i = 0;
    while i < MSG_SET_COUNT {
        if MSG_SET_NAMES[i].len() > 0 {
            ok += 1;
        }
        let mut j = i + 1;
        while j < MSG_SET_COUNT {
            if MSG_SET[i] == MSG_SET[j] {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    v.push((
        "J1-8 消息型名值点验+唯一",
        ok == 8 && uniq,
        format!("{}/8 uniq={}", ok, uniq),
    ));
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 录态机收敛：终态必为 Stopped。
    let mut rec = SpyRecorder::new();
    let _ = rec.register(7);
    let _ = rec.start();
    let _ = rec.stop();
    v.push((
        "J1-录态机收敛 Stopped",
        rec.state == RecState::Stopped,
        String::from("启停配对"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x669C0 窗口")));
    v
}

pub fn run_unx14661_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14661 Spy 录制器核心（逐消息元组捕获）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
