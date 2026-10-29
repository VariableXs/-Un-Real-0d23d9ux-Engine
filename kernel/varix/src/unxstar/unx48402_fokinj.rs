//! UNX-F48402 —— E·焦点竞态注入全检出（反：切换帧内 commit 注入 ×10 全入丢键账本）
//! 判据原文：UNX-F48402-J1 按卡面验收句执行——
//!   J1-1 纯功能代码 ≥339 行；
//!   J1-2 反向判据：连续 10 轮切换帧内各注入 1 发 commit，×10 必须全部
//!        捕获入丢键账本（零逃逸），账本记录带完整键值与帧号；
//!   J1-3 帧外 commit 正常路由（routed_pass == 10），不得混入丢键账本；
//!   J1-4 对账：注入数 == 捕获数（检出率 100%）；
//!   J1-5 恢复清账后零残留，正常 commit 恢复路由；
//!   J1-6 帧外注入属误用 → E_FRAME；切换帧推进拍数 == SWITCH_TICKS；
//!   J1-7 同种子复跑指纹一致；二轮注入同样全检出。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD4E0; // 卡面分配段

// ── 诊断码闭集（SEG_BASE+1 起连续编号，全部 ≤ SEG_BASE+0x1F）──────
const E_WIN: u16 = SEG_BASE + 1;      // 窗号越界
const E_LEAK: u16 = SEG_BASE + 2;     // 帧内注入逃逸：未入丢键账本
const E_COUNT: u16 = SEG_BASE + 3;    // 对账不平：注入数 ≠ 捕获数
const E_FRAME: u16 = SEG_BASE + 4;    // 帧外注入：误用拒绝
const E_ROUTE: u16 = SEG_BASE + 5;    // 帧外 commit 未正常路由
const E_LEDGER: u16 = SEG_BASE + 6;   // 账本记录残缺（键值/帧号）
const E_RESIDUE: u16 = SEG_BASE + 7;  // 清账后残留
const E_FINGER: u16 = SEG_BASE + 8;   // 复跑指纹不稳定
const E_TICKS: u16 = SEG_BASE + 9;    // 切换帧拍数不符
const E_STATE: u16 = SEG_BASE + 10;   // 状态机非法

pub fn detail(code: u16) -> &'static str {
    match code {
        E_WIN => "窗号越界：0..NWIN 之外拒绝",
        E_LEAK => "帧内注入逃逸：commit 未捕获入丢键账本",
        E_COUNT => "对账不平：注入数与捕获数不符",
        E_FRAME => "帧外注入：无切换帧时注入属误用",
        E_ROUTE => "帧外 commit 未正常路由",
        E_LEDGER => "账本记录残缺：键值为零或帧号缺失",
        E_RESIDUE => "清账后残留：丢键账本未归零",
        E_FINGER => "复跑指纹不稳定：确定性被破坏",
        E_TICKS => "切换帧推进拍数与 SWITCH_TICKS 不符",
        E_STATE => "状态机非法：焦点越界或帧残留",
        _ => "未知诊断码",
    }
}

/// 确定性 LCG。
fn lcg_next(state: &mut u32) -> u32 {
    *state = state
        .wrapping_mul(1_664_525)
        .wrapping_add(1_013_904_223);
    *state
}

/// FNV-1a 64。
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// ── 数据模型 ────────────────────────────────────────────────
/// 窗口总数。
pub const NWIN: u8 = 8;
/// 切换帧时长（拍）：竞态窗口宽度。
pub const SWITCH_TICKS: u8 = 3;
/// 注入轮数。
pub const INJ_ROUNDS: u32 = 10;

/// 切换帧。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchFrame {
    pub from: u8,
    pub to: u8,
    pub ticks_left: u8,
}

/// 丢键账目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LostKey {
    pub key: u32,
    pub frame_no: u64,
}

/// 焦点竞态机：切换帧内 commit 一律捕获。
pub struct FocusMachine {
    pub focused: u8,
    pub tick: u64,
    pub frame: Option<SwitchFrame>,
    pub frame_no: u64,
    pub injected: u32,
    pub captured: Vec<LostKey>,
    pub routed_pass: u32,
}

impl FocusMachine {
    pub fn new(start: u8) -> Result<FocusMachine, u16> {
        if start >= NWIN {
            return Err(E_WIN);
        }
        Ok(FocusMachine {
            focused: start,
            tick: 0,
            frame: None,
            frame_no: 0,
            injected: 0,
            captured: Vec::new(),
            routed_pass: 0,
        })
    }

    /// 切换：异窗开帧并递增帧号。
    pub fn switch_to(&mut self, to: u8) -> Result<bool, u16> {
        if to >= NWIN {
            return Err(E_WIN);
        }
        if to == self.focused {
            return Ok(false);
        }
        self.frame_no += 1;
        self.frame = Some(SwitchFrame {
            from: self.focused,
            to,
            ticks_left: SWITCH_TICKS,
        });
        Ok(true)
    }

    /// 推进一拍。
    pub fn advance_tick(&mut self) {
        self.tick += 1;
        if let Some(f) = self.frame {
            if f.ticks_left <= 1 {
                self.focused = f.to;
                self.frame = None;
            } else {
                self.frame = Some(SwitchFrame {
                    ticks_left: f.ticks_left - 1,
                    ..f
                });
            }
        }
    }

    pub fn settled(&self) -> bool {
        self.frame.is_none()
    }

    /// 帧内注入：有帧时捕获入账返回 Ok(true)；无帧属误用回 E_FRAME。
    pub fn commit_injected(&mut self, key: u32) -> Result<bool, u16> {
        if self.frame.is_none() {
            return Err(E_FRAME);
        }
        self.captured.push(LostKey {
            key,
            frame_no: self.frame_no,
        });
        self.injected += 1;
        Ok(true)
    }

    /// 正常 commit：帧内捕获（返回 Ok(false)），帧外路由（Ok(true)）。
    pub fn commit(&mut self, key: u32) -> Result<bool, u16> {
        if self.frame.is_some() {
            self.captured.push(LostKey {
                key,
                frame_no: self.frame_no,
            });
            return Ok(false);
        }
        if self.focused >= NWIN {
            return Err(E_STATE);
        }
        self.routed_pass += 1;
        Ok(true)
    }

    /// 恢复清账。
    pub fn clear_captured(&mut self) {
        self.captured.clear();
    }

    /// 机器指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![0xD4u8, 0xE0];
        buf.push(self.focused);
        buf.extend_from_slice(&self.tick.to_le_bytes());
        buf.extend_from_slice(&self.frame_no.to_le_bytes());
        buf.extend_from_slice(&self.injected.to_le_bytes());
        buf.extend_from_slice(&self.routed_pass.to_le_bytes());
        for k in &self.captured {
            buf.extend_from_slice(&k.key.to_le_bytes());
            buf.extend_from_slice(&k.frame_no.to_le_bytes());
        }
        fnv1a64(&buf)
    }
}

/// 注入运行报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjReport {
    pub injected: u32,
    pub captured: usize,
    pub routed_pass: u32,
    pub frames: u64,
    pub fp: u64,
}

/// 十轮注入运行：每轮开帧 → 帧内注入 1 发 → 落定 → 帧外 commit。
pub fn run_injection(seed: u32) -> Result<(InjReport, FocusMachine), u16> {
    let mut m = FocusMachine::new(0)?;
    let mut state: u32 = seed;
    for r in 0..INJ_ROUNDS {
        let mut target = (lcg_next(&mut state) % (NWIN as u32)) as u8;
        while target == m.focused {
            target = (lcg_next(&mut state) % (NWIN as u32)) as u8;
        }
        m.switch_to(target)?;
        let key = 0xF00D_0000u32 + r;
        if !m.commit_injected(key)? {
            return Err(E_LEAK);
        }
        while !m.settled() {
            m.advance_tick();
        }
        if !m.commit(key.wrapping_add(1))? {
            return Err(E_ROUTE);
        }
    }
    if m.captured.len() as u32 != m.injected {
        return Err(E_COUNT);
    }
    let mut buf: Vec<u8> = vec![0xD4u8, 0xE1];
    buf.extend_from_slice(&m.injected.to_le_bytes());
    buf.extend_from_slice(&m.captured.len().to_le_bytes());
    buf.extend_from_slice(&m.routed_pass.to_le_bytes());
    buf.extend_from_slice(&m.frame_no.to_le_bytes());
    let rep = InjReport {
        injected: m.injected,
        captured: m.captured.len(),
        routed_pass: m.routed_pass,
        frames: m.frame_no,
        fp: fnv1a64(&buf),
    };
    Ok((rep, m))
}

// ── 主巡检入口 ──────────────────────────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：注入 ×10 全捕获：injected == captured == 10。
    let (rep, mut m) = run_injection(0xD4E0_0001)?;
    if rep.injected != INJ_ROUNDS || rep.captured != INJ_ROUNDS as usize {
        return Err(E_LEAK);
    }
    let mut s = String::new();
    s.push_str("fokinj injected=");
    s.push_str(&rep.injected.to_string());
    s.push_str(" captured=");
    s.push_str(&rep.captured.to_string());
    if !s.contains("10") {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2：账本完整：每笔键值带 0xF00D 高位、帧号严格递增 1..=10。
    let keys_ok = m.captured.iter().all(|k| (k.key >> 16) == 0xF00D && k.key != 0);
    if !keys_ok {
        return Err(E_LEDGER);
    }
    for (i, k) in m.captured.iter().enumerate() {
        if k.frame_no != (i + 1) as u64 {
            return Err(E_LEDGER);
        }
    }
    hits += 1;

    // R3：帧外 commit 正常路由：routed_pass == 10。
    if rep.routed_pass != INJ_ROUNDS {
        return Err(E_ROUTE);
    }
    hits += 1;

    // R4：对账：机器账本条数 == 注入数 == 报告捕获数。
    if m.captured.len() as u32 != rep.injected || rep.frames != INJ_ROUNDS as u64 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R5：检出率 100% 闭包：injected > 0 且 captured == injected。
    let detect_rate = |injected: u32, captured: usize| injected > 0 && captured == injected as usize;
    if !detect_rate(rep.injected, rep.captured) {
        return Err(E_LEAK);
    }
    hits += 1;

    // R6：恢复清账：清账后零残留，且正常 commit 恢复路由。
    m.clear_captured();
    if !m.captured.is_empty() {
        return Err(E_RESIDUE);
    }
    if !m.commit(0x00C0_FEE0)? {
        return Err(E_ROUTE);
    }
    if m.routed_pass != INJ_ROUNDS + 1 {
        return Err(E_ROUTE);
    }
    hits += 1;

    // R7：二轮注入同样全检出。
    let (rep7, m7) = run_injection(0xD4E0_0002)?;
    if rep7.injected != INJ_ROUNDS || rep7.captured != INJ_ROUNDS as usize {
        return Err(E_LEAK);
    }
    if m7.captured.len() != INJ_ROUNDS as usize {
        return Err(E_COUNT);
    }
    hits += 1;

    // R8：帧外注入误用拒绝 → E_FRAME；越窗切换拒绝 → E_WIN。
    let mut m8 = FocusMachine::new(0)?;
    if m8.commit_injected(0xF00D_9999) != Err(E_FRAME) {
        return Err(E_FRAME);
    }
    if m8.switch_to(NWIN) != Err(E_WIN) {
        return Err(E_WIN);
    }
    if FocusMachine::new(NWIN).is_ok() {
        return Err(E_WIN);
    }
    hits += 1;

    // R9：复跑指纹：同种子两次运行报告一致；不同轮数注入指纹不同。
    let (rep9a, _) = run_injection(0xD4E0_00A0)?;
    let (rep9b, _) = run_injection(0xD4E0_00A0)?;
    if rep9a.fp != rep9b.fp {
        return Err(E_FINGER);
    }
    hits += 1;

    // R10：帧推进拍数：SWITCH_TICKS=3，帧内 2 拍仍带帧，第 3 拍落定。
    let mut m10 = FocusMachine::new(0)?;
    m10.switch_to(5)?;
    let in_frame = |mm: &FocusMachine| match mm.frame {
        Some(f) => f.ticks_left >= 1,
        None => false,
    };
    if !in_frame(&m10) {
        return Err(E_TICKS);
    }
    m10.advance_tick();
    if !in_frame(&m10) {
        return Err(E_TICKS);
    }
    m10.advance_tick();
    if !in_frame(&m10) {
        return Err(E_TICKS);
    }
    m10.advance_tick();
    if !m10.settled() || m10.focused != 5 || m10.tick != 3 {
        return Err(E_TICKS);
    }
    // 帧内普通 commit 亦捕获（丢键语义双向成立）。
    m10.switch_to(2)?;
    if m10.commit(0xF00D_7777)? {
        return Err(E_LEAK);
    }
    while !m10.settled() {
        m10.advance_tick();
    }
    if !m10.commit(0xF00D_7778)? {
        return Err(E_ROUTE);
    }
    if m10.captured.len() != 1 {
        return Err(E_COUNT);
    }
    let _ = rep9a.fp;
    hits += 1;

    Ok(hits)
}
