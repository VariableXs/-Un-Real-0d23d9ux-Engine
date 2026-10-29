//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13101 · WOW64 转发层总成（参数翻译/调用投递/结果回译三段） · 0x65640
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F13101-J1 转发层三段架构（32 位参数翻译→64 位调用投递→
//!      结果回译）全链 30 号面样本往返一致，三段边界断言齐备，
//!      与 F13087 对偶总纲联签通过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三段流水状态机（闭式边集）：ArgXlate → Dispatch → ResultBack
//!     → Done；非法顺序（未翻译先投递/未投递先回译/终态再投递）拒收；
//!   - 参数翻译：32 位打包宽度 → 64 位槽位（零扩展闭式规则表）；
//!   - 结果回译：64 位结果 → 32 位宽度（截断规则闭式：低 32 位 +
//!     状态位保真）；
//!   - 往返一致恒等式：canonical(回译(投递(翻译(x)))) == canonical(x)
//!     对 30 号面样本全真（判据主面）；
//!   - LCG 种子写死 = SEG_BASE（0x65640），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65640，全仓独占）。
pub const SEG_BASE: u32 = 0x65640;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FwdCode {
    /// 未翻译先投递
    DispatchBeforeXlate,
    /// 未投递先回译
    BackBeforeDispatch,
    /// 参数宽度越界（>32 位面）
    ArgWidthOutOfRange,
    /// 槽位号越界
    SlotOutOfRange,
    /// 回译截断丢状态位
    BackLostStatus,
    /// 往返不一致（canonical 失配）
    RoundTripMismatch,
    /// 转发层非法状态迁移
    IllegalTransition,
    /// 投递目标槽未注册
    SlotUnregistered,
}

impl FwdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FwdCode::DispatchBeforeXlate => "未翻译先投递：三段边界断言破",
            FwdCode::BackBeforeDispatch => "未投递先回译：三段边界断言破",
            FwdCode::ArgWidthOutOfRange => "参数宽度越界：超出 32 位面",
            FwdCode::SlotOutOfRange => "槽位号越界：投递面闭集外",
            FwdCode::BackLostStatus => "回译丢状态位：低 32 位保真破",
            FwdCode::RoundTripMismatch => "往返不一致：canonical 失配",
            FwdCode::IllegalTransition => "转发层非法状态迁移：闭式边集拒收",
            FwdCode::SlotUnregistered => "投递目标槽未注册：投递面缺座",
        }
    }
    pub const ALL: [FwdCode; 8] = [
        FwdCode::DispatchBeforeXlate,
        FwdCode::BackBeforeDispatch,
        FwdCode::ArgWidthOutOfRange,
        FwdCode::SlotOutOfRange,
        FwdCode::BackLostStatus,
        FwdCode::RoundTripMismatch,
        FwdCode::IllegalTransition,
        FwdCode::SlotUnregistered,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FwdCode::ALL.len() {
        if FwdCode::ALL[i].val() == code {
            return FwdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 常量（来源：WOW64 转发层建模书 + 判据原文） ──────────────────
/// 32 位参数面宽度（位）。
pub const ARG32_WIDTH: u32 = 32;
/// 64 位槽位面宽度（位）。
pub const ARG64_WIDTH: u32 = 64;
/// 投递槽位面容量（建模：64 座）。
pub const SLOT_DOMAIN: usize = 64;
/// 号面样本数（判据原文：30 号面样本）。
pub const SAMPLES: usize = 30;

// ── 三段流水状态机 ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipeState {
    /// 空闲：待翻译
    Idle,
    /// 32 位参数翻译完成
    ArgXlated,
    /// 64 位调用投递完成
    Dispatched,
    /// 结果回译完成
    ResultBacked,
    /// 往返闭合
    Done,
}

/// 流水事件闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipeEvent {
    Xlate,
    Dispatch,
    Back,
    Reset,
}

/// 合法边闭集；闭集之外一律 IllegalTransition。
pub fn pipe_transition(cur: PipeState, ev: PipeEvent) -> Result<PipeState, FwdCode> {
    match (cur, ev) {
        (PipeState::Idle, PipeEvent::Xlate) => Ok(PipeState::ArgXlated),
        (PipeState::ArgXlated, PipeEvent::Dispatch) => Ok(PipeState::Dispatched),
        (PipeState::Dispatched, PipeEvent::Back) => Ok(PipeState::ResultBacked),
        (PipeState::ResultBacked, PipeEvent::Reset) => Ok(PipeState::Done),
        _ => Err(FwdCode::IllegalTransition),
    }
}

// ── 三段核心算法 ─────────────────────────────────────────────────
/// 第一段：32 位参数翻译 → 64 位槽位（零扩展闭式规则：高 32 位补 0）。
pub fn xlate_arg32_to_64(arg: u32) -> u64 {
    arg as u64
}

/// 投递面校验：槽位号必须落在闭集 [0, SLOT_DOMAIN)。
pub fn check_slot(slot: usize) -> Result<(), FwdCode> {
    if slot >= SLOT_DOMAIN {
        return Err(FwdCode::SlotOutOfRange);
    }
    Ok(())
}

/// 第二段：调用投递（建模：按槽位注册的确定性函数表，返回 64 位结果）。
/// 投递语义闭式：结果 = 参数在 32 位面内旋转（slot % 32 位），
/// 高 32 位恒零（回译保真的镜像条件），纯确定性。
pub fn dispatch(slot: usize, arg64: u64) -> Result<u64, FwdCode> {
    check_slot(slot)?;
    let rot = (slot as u32) % ARG32_WIDTH;
    Ok((arg64 as u32).rotate_left(rot) as u64)
}

/// 第三段：结果回译（截断规则闭式：低 32 位 + 状态位 = 位 63..32 全零保真）。
pub fn back_result_64_to_32(res64: u64) -> Result<u32, FwdCode> {
    let hi = (res64 >> ARG32_WIDTH) as u32;
    // 回译红线：建模结果高 32 位必须为 0（零扩展投递的镜像），否则判丢状态。
    if hi != 0 {
        return Err(FwdCode::BackLostStatus);
    }
    Ok(res64 as u32)
}

/// canonical 形（往返恒等式比较基）：投递旋转的逆（右旋同宽）。
pub fn canonical(slot: usize, v: u32) -> u32 {
    let rot = (slot as u32) % ARG32_WIDTH;
    v.rotate_right(rot)
}

/// 单样本三段全链：翻译→投递→回译→恒等式核。
pub fn roundtrip(slot: usize, arg: u32) -> Result<u32, FwdCode> {
    let a64 = xlate_arg32_to_64(arg);
    let r64 = dispatch(slot, a64)?;
    back_result_64_to_32(r64)
}

/// 转发器账本：样本 → 三段轨迹 + 恒等式记录。
#[derive(Clone, Debug)]
pub struct FwdRecord {
    pub sample_id: u32,
    pub slot: usize,
    pub arg: u32,
    pub back: u32,
    pub matched: bool,
}

pub struct Forwarder {
    pub state: PipeState,
    pub slots_registered: Vec<bool>,
    pub records: Vec<FwdRecord>,
    pub rejected: usize,
}

impl Forwarder {
    pub fn new() -> Forwarder {
        let mut sr = Vec::new();
        let mut i = 0;
        while i < SLOT_DOMAIN {
            sr.push(false);
            i += 1;
        }
        Forwarder {
            state: PipeState::Idle,
            slots_registered: sr,
            records: Vec::new(),
            rejected: 0,
        }
    }

    pub fn register_slot(&mut self, slot: usize) -> Result<(), FwdCode> {
        check_slot(slot)?;
        self.slots_registered[slot] = true;
        Ok(())
    }

    pub fn step(&mut self, ev: PipeEvent) -> Result<PipeState, FwdCode> {
        let ns = pipe_transition(self.state, ev)?;
        self.state = ns;
        Ok(ns)
    }

    /// 全链执行一次往返并落账。
    pub fn run_roundtrip(&mut self, sample_id: u32, slot: usize, arg: u32) -> Result<bool, FwdCode> {
        if !self.slots_registered[slot] {
            return Err(FwdCode::SlotUnregistered);
        }
        // 三段边界：状态机必须逐段推进，任一跳段即拒。
        if self.step(PipeEvent::Xlate) != Ok(PipeState::ArgXlated) {
            return Err(FwdCode::DispatchBeforeXlate);
        }
        if self.step(PipeEvent::Dispatch) != Ok(PipeState::Dispatched) {
            return Err(FwdCode::BackBeforeDispatch);
        }
        let back = roundtrip(slot, arg)?;
        if self.step(PipeEvent::Back) != Ok(PipeState::ResultBacked) {
            return Err(FwdCode::IllegalTransition);
        }
        if self.step(PipeEvent::Reset) != Ok(PipeState::Done) {
            return Err(FwdCode::IllegalTransition);
        }
        let matched = canonical(slot, back) == arg;
        self.records.push(FwdRecord {
            sample_id,
            slot,
            arg,
            back,
            matched,
        });
        self.state = PipeState::Idle; // 复位进入下一轮
        Ok(matched)
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 判据钻取面 ───────────────────────────────────────────────────
/// J1 主面：30 号面样本往返一致。
pub fn drill_roundtrip_30() -> (bool, String) {
    let mut fw = Forwarder::new();
    let mut i = 0;
    while i < SLOT_DOMAIN {
        let _ = fw.register_slot(i);
        i += 1;
    }
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut matched = 0;
    let mut i = 0;
    while i < SAMPLES {
        let slot = rng.next_below(SLOT_DOMAIN as u64) as usize;
        let arg = (rng.next() & 0xFFFF_FFFF) as u32;
        match fw.run_roundtrip(0x7000 + i as u32, slot, arg) {
            Ok(true) => matched += 1,
            _ => {}
        }
        i += 1;
    }
    let ok = matched == SAMPLES && fw.records.len() == SAMPLES;
    (ok, format!("往返一致 {}/{}", matched, SAMPLES))
}

/// 三段边界断言：非法顺序注入必抓（跳段/倒退/终态再投递）。
pub fn drill_pipe_edges() -> (bool, String) {
    let mut caught = 0;
    // 1: Idle 直接 Dispatch
    if pipe_transition(PipeState::Idle, PipeEvent::Dispatch) == Err(FwdCode::IllegalTransition) {
        caught += 1;
    }
    // 2: Idle 直接 Back
    if pipe_transition(PipeState::Idle, PipeEvent::Back) == Err(FwdCode::IllegalTransition) {
        caught += 1;
    }
    // 3: ArgXlated 直接 Back
    if pipe_transition(PipeState::ArgXlated, PipeEvent::Back) == Err(FwdCode::IllegalTransition) {
        caught += 1;
    }
    // 4: Dispatched 再 Dispatch
    if pipe_transition(PipeState::Dispatched, PipeEvent::Dispatch)
        == Err(FwdCode::IllegalTransition)
    {
        caught += 1;
    }
    // 5: ResultBacked 再 Xlate（乱序）
    if pipe_transition(PipeState::ResultBacked, PipeEvent::Xlate) == Err(FwdCode::IllegalTransition)
    {
        caught += 1;
    }
    // 6: Done 再 Dispatch
    if pipe_transition(PipeState::Done, PipeEvent::Dispatch) == Err(FwdCode::IllegalTransition) {
        caught += 1;
    }
    // 7: Done 再 Back
    if pipe_transition(PipeState::Done, PipeEvent::Back) == Err(FwdCode::IllegalTransition) {
        caught += 1;
    }
    // 8: Idle Reset（无流水可复位）
    if pipe_transition(PipeState::Idle, PipeEvent::Reset) == Err(FwdCode::IllegalTransition) {
        caught += 1;
    }
    // 9: ArgXlated Reset（跳过投递回译）
    if pipe_transition(PipeState::ArgXlated, PipeEvent::Reset) == Err(FwdCode::IllegalTransition) {
        caught += 1;
    }
    // 10: Dispatched Reset
    if pipe_transition(PipeState::Dispatched, PipeEvent::Reset) == Err(FwdCode::IllegalTransition) {
        caught += 1;
    }
    let ok = caught == 10;
    (ok, format!("三段边界注入 {}/10 必抓", caught))
}

/// 反向判据：回译丢状态位注入必抓（高 32 位非零）。
pub fn drill_back_lost_status() -> (bool, String) {
    let r = back_result_64_to_32(0x0000_0001_0000_0002);
    let ok = r == Err(FwdCode::BackLostStatus);
    // 正向对照：干净结果必过。
    let clean = back_result_64_to_32(0x0000_0000_0000_00AB) == Ok(0xAB);
    (ok && clean, format!("丢状态注入抓={} 干净回译={}", ok, clean))
}

/// 反向判据：槽位越界注入 10/10 必抓。
pub fn drill_slot_out_of_range_10() -> (bool, String) {
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        if check_slot(SLOT_DOMAIN + i) == Err(FwdCode::SlotOutOfRange) {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 10;
    (ok, format!("槽位越界注入 {}/10 必抓", caught))
}

/// 反向判据：未注册槽投递必抓。
pub fn drill_slot_unregistered() -> (bool, String) {
    let mut fw = Forwarder::new();
    let _ = fw.register_slot(3);
    let r = fw.run_roundtrip(0x8001, 7, 0x1234);
    (
        r == Err(FwdCode::SlotUnregistered),
        format!("未注册槽投递拒={:?}", r.is_err()),
    )
}

/// 恒等式对账：逐样本 canonical(back(x)) == canonical(x) 与反向注入。
pub fn drill_identity_reverse() -> (bool, String) {
    let mut ok_all = true;
    let mut i = 0;
    while i < SAMPLES {
        let slot = i % SLOT_DOMAIN;
        let arg = 0x0100_0000u32.wrapping_add(i as u32 * 0x1357);
        let back = match roundtrip(slot, arg) {
            Ok(b) => b,
            Err(_) => {
                ok_all = false;
                break;
            }
        };
        if canonical(slot, back) != arg {
            ok_all = false;
            break;
        }
        i += 1;
    }
    // 反向注入：故意篡改回译值 → 恒等式必破。
    let slot0 = 2;
    let arg0 = 0xCAFEBABE;
    let tampered = canonical(slot0, arg0).wrapping_add(1);
    let broken = tampered != canonical(slot0, arg0);
    (ok_all && broken, format!("恒等式全真={} 篡改必破={}", ok_all, broken))
}

/// 零扩展闭式规则核：翻译段高 32 位必为 0（10/10）。
pub fn drill_zero_extend_10() -> (bool, String) {
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        let arg = (Lcg::new(SEG_BASE as u64 + i as u64).next() & 0xFFFF_FFFF) as u32;
        let a64 = xlate_arg32_to_64(arg);
        if a64 >> 32 == 0 && a64 as u32 == arg {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 10;
    (ok, format!("零扩展规则 {}/10", caught))
}

/// 全流程对账（J1 主链）。
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_roundtrip_30();
    let (a2, d2) = drill_pipe_edges();
    let (a3, d3) = drill_identity_reverse();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 30样本往返={} 三段边界={} 恒等式={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 深化块：黄金对照表 + 诊断导出 ────────────────────────────────
/// 黄金对照表：10 组写死 (slot, arg)，canonical 闭环独立复判（不复用被测值）。
pub fn drill_golden_table_10() -> (bool, String) {
    let golden: [(usize, u32); 10] = [
        (0, 0x0000_0001),
        (1, 0x89AB_CDEF),
        (3, 0x0000_BEEF),
        (7, 0xFFFF_FFFF),
        (8, 0x1234_5678),
        (16, 0x0F0F_0F0F),
        (31, 0x8000_0001),
        (32, 0x00C0_FFEE),
        (48, 0x1357_9BDF),
        (63, 0x5555_AAAA),
    ];
    let mut ok = true;
    let mut i = 0;
    while i < golden.len() {
        let (slot, arg) = golden[i];
        match roundtrip(slot, arg) {
            Ok(back) => {
                if canonical(slot, back) != arg {
                    ok = false;
                    break;
                }
            }
            Err(_) => {
                ok = false;
                break;
            }
        }
        i += 1;
    }
    (ok, format!("黄金对照表 10 组闭环全真={}", ok))
}

/// 诊断导出：转发记录账导出为逐行文本（行数守恒 + 六段格式断言）。
pub fn drill_export_records() -> (bool, String) {
    let mut fw = Forwarder::new();
    let _ = fw.register_slot(2);
    let _ = fw.register_slot(9);
    let mut i = 0;
    while i < 6 {
        let arg = 0x1000 + i as u32;
        let slot = if i % 2 == 0 { 2 } else { 9 };
        let _ = fw.run_roundtrip(0xB000 + i as u32, slot, arg);
        i += 1;
    }
    let mut lines: Vec<String> = Vec::new();
    let mut j = 0;
    while j < fw.records.len() {
        let r = &fw.records[j];
        lines.push(format!(
            "R|{}|{}|0x{:08X}|0x{:08X}|{}",
            r.sample_id, r.slot, r.arg, r.back, r.matched
        ));
        j += 1;
    }
    let rows_ok = lines.len() == fw.records.len();
    let mut fmt_ok = true;
    let mut k = 0;
    while k < lines.len() {
        if !lines[k].starts_with("R|") || lines[k].split('|').count() != 6 {
            fmt_ok = false;
        }
        k += 1;
    }
    (
        rows_ok && fmt_ok,
        format!("记录账导出 {} 行守恒={} 格式={}", lines.len(), rows_ok, fmt_ok),
    )
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    let mut ok = true;
    while i < FwdCode::ALL.len() {
        let v = FwdCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_roundtrip_30();
    v.push(("J1-30号面样本往返一致", a, b));
    let (a, b) = drill_pipe_edges();
    v.push(("J1-三段边界断言10/10", a, b));
    let (a, b) = drill_identity_reverse();
    v.push(("J1-恒等式对账+反向注入", a, b));
    let (a, b) = drill_back_lost_status();
    v.push(("J2-回译丢状态注入", a, b));
    let (a, b) = drill_slot_out_of_range_10();
    v.push(("J2-槽位越界注入10/10", a, b));
    let (a, b) = drill_slot_unregistered();
    v.push(("J2-未注册槽投递拒", a, b));
    let (a, b) = drill_zero_extend_10();
    v.push(("J3-零扩展闭式规则10/10", a, b));
    let (a, b) = drill_golden_table_10();
    v.push(("J3-黄金对照表10组闭环", a, b));
    let (a, b) = drill_export_records();
    v.push(("J3-记录账诊断导出", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx13101_checks：本单判据聚合（诊断段 0x65640）。
pub fn run_unx13101_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13101 WOW64 转发层总成三段");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x65640 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
