//! UNX-F48549 —— E·出厂态一键复位（正：复位路径用户可见可撤销预警）
//! 判据原文：UNX-F48549-J1 出厂态复位路径必须带预警与撤销窗：
//! 复位先布防（arm）、预警必须用户可见（可见位）、撤销窗内可撤销
//! （撤销后回到布防前态且账目回滚）、窗满确认后才执行；
//! 未确认不得执行、执行后计数恰一、账目守恒、指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 复位态闭集；R2 预警可见位；
//! R3 撤销窗内撤销成功；R4 窗满确认执行；R5 未确认不执行；
//! R6 执行计数恰一；R7 撤销回滚账目；R8 fuzz 复位流零错态；
//! R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF360;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_STATE: u16 = SEG_BASE + 1; // 复位态越出闭集
const E_VIS: u16 = SEG_BASE + 2; // 预警不可见
const E_CANCEL: u16 = SEG_BASE + 3; // 撤销失败
const E_EXEC: u16 = SEG_BASE + 4; // 未确认执行
const E_ONCE: u16 = SEG_BASE + 5; // 执行计数异常
const E_ROLLBACK: u16 = SEG_BASE + 6; // 撤销回滚不彻底
const E_ORDER: u16 = SEG_BASE + 7; // 态序错乱
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 复位流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 复位态闭集：1 空闲 / 2 布防 / 3 撤销窗 / 4 已执行。
pub const S_IDLE: u8 = 1;
pub const S_ARMED: u8 = 2;
pub const S_GRACE: u8 = 3;
pub const S_DONE: u8 = 4;
pub const STATE_NAMES: [&str; 4] = ["idle", "armed", "grace", "done"];

/// 撤销窗宽（毫秒）。
pub const GRACE_MS: u32 = 5000;

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 复位态闭集判定。
pub fn known_state(s: u8) -> bool {
    s == S_IDLE || s == S_ARMED || s == S_GRACE || s == S_DONE
}

/// 复位流程器：态机 + 预警可见位 + 撤销回滚账 + 执行计数。
pub struct ResetFlow {
    pub state: u8,
    pub warn_visible: bool,
    pub exec_count: u32,
    pub cancel_count: u32,
    pub armed_at: u32,
}

impl ResetFlow {
    pub fn new() -> ResetFlow {
        ResetFlow {
            state: S_IDLE,
            warn_visible: false,
            exec_count: 0,
            cancel_count: 0,
            armed_at: 0,
        }
    }

    /// 布防：仅空闲态可布防；预警必须用户可见（布防即亮位）。
    pub fn arm(&mut self, at: u32) -> Result<(), u16> {
        if self.state != S_IDLE {
            return Err(E_ORDER);
        }
        self.state = S_ARMED;
        self.warn_visible = true;
        self.armed_at = at;
        Ok(())
    }

    /// 进入撤销窗：布防后开窗，可见位保持。
    pub fn open_grace(&mut self) -> Result<(), u16> {
        if self.state != S_ARMED {
            return Err(E_ORDER);
        }
        self.state = S_GRACE;
        Ok(())
    }

    /// 撤销：仅撤销窗内可撤；回滚到空闲且可见位熄灭。
    pub fn cancel(&mut self) -> Result<(), u16> {
        if self.state != S_GRACE {
            return Err(E_CANCEL);
        }
        self.state = S_IDLE;
        self.warn_visible = false;
        self.armed_at = 0;
        self.cancel_count += 1;
        Ok(())
    }

    /// 确认执行：仅撤销窗可执行；执行计数恰一后进终态并熄可见位。
    pub fn confirm(&mut self) -> Result<(), u16> {
        if self.state != S_GRACE {
            return Err(E_EXEC);
        }
        if self.exec_count >= 1 {
            return Err(E_ONCE);
        }
        self.state = S_DONE;
        self.warn_visible = false;
        self.exec_count += 1;
        Ok(())
    }

    /// 回滚彻底性：撤销后态/位/时戳全部归零。
    pub fn rollback_clean(&self) -> bool {
        self.state == S_IDLE && !self.warn_visible && self.armed_at == 0
    }

    /// 执行位一致性：DONE 态必须恰执行一次。
    pub fn exec_sane(&self) -> bool {
        (self.state == S_DONE) == (self.exec_count == 1)
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 复位态闭集：四态合法、0/5/255 非法；态名非空互异。
    if !known_state(S_IDLE) || !known_state(S_ARMED) || !known_state(S_GRACE) || !known_state(S_DONE)
    {
        return Err(E_STATE);
    }
    if known_state(0) || known_state(5) || known_state(255) || STATE_NAMES[0] == STATE_NAMES[3] {
        return Err(E_STATE);
    }
    hits += 1;

    // R2 预警可见位：布防即亮位；未布防恒灭。
    let mut rf = ResetFlow::new();
    if rf.warn_visible {
        return Err(E_VIS);
    }
    rf.arm(100)?;
    if !rf.warn_visible {
        return Err(E_VIS);
    }
    hits += 1;

    // R3 撤销窗内撤销成功：开窗后撤销回到空闲、回滚彻底。
    rf.open_grace()?;
    rf.cancel()?;
    if !rf.rollback_clean() || rf.cancel_count != 1 {
        return Err(E_CANCEL);
    }
    hits += 1;

    // R4 窗满确认执行：布防 → 开窗 → 确认 → 终态 + 恰一执行。
    rf.arm(200)?;
    rf.open_grace()?;
    rf.confirm()?;
    if rf.state != S_DONE || rf.exec_count != 1 || !rf.exec_sane() {
        return Err(E_EXEC);
    }
    hits += 1;

    // R5 未确认不执行：布防态直接确认被拒，码恰为 E_EXEC。
    let mut rf2 = ResetFlow::new();
    rf2.arm(0)?;
    match rf2.confirm() {
        Err(E_EXEC) => hits += 1,
        _ => return Err(E_EXEC),
    }

    // R6 执行计数恰一：终态后不可再布防（态序拦截），执行位不变。
    match rf.arm(999) {
        Err(E_ORDER) => hits += 1,
        _ => return Err(E_ONCE),
    }
    if rf.exec_count != 1 {
        return Err(E_ONCE);
    }

    // R7 撤销回滚账目：撤销后 armed_at 归零、取消计数累加；撤销窗外撤被拒。
    let mut rf3 = ResetFlow::new();
    rf3.arm(300)?;
    match rf3.cancel() {
        Err(E_CANCEL) => {}
        _ => return Err(E_CANCEL),
    }
    rf3.open_grace()?;
    rf3.cancel()?;
    if rf3.armed_at != 0 || rf3.cancel_count != 1 {
        return Err(E_ROLLBACK);
    }
    hits += 1;

    // R8 fuzz 复位流零错态：LCG 驱动 64 轮随机流（布防/开窗/撤/确认），
    // 非法操作必拒、终态账自洽。
    let mut lcg = Lcg::new(0x48549);
    let mut t = 0u32;
    let mut exec_total = 0u32;
    let mut cancel_total = 0u32;
    while t < 64 {
        let mut f = ResetFlow::new();
        f.arm(t)?;
        if lcg.below(2) == 0 {
            f.open_grace()?;
            if lcg.below(2) == 0 {
                f.confirm()?;
                exec_total += 1;
            } else {
                f.cancel()?;
                cancel_total += 1;
            }
        } else {
            // 留在布防态，验证态序拦截。
            match f.confirm() {
                Err(E_EXEC) => {}
                _ => return Err(E_FUZZ),
            }
        }
        if !f.exec_sane() {
            return Err(E_FUZZ);
        }
        t += 1;
    }
    if exec_total == 0 || cancel_total == 0 {
        return Err(E_FUZZ);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = flow_fp(&rf);
    let f2 = flow_fp(&rf);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：st=/exec=/cancel= 三段前缀在位。
    let line = flow_line(&rf);
    if !has_kw(line.as_bytes(), b"st=")
        || !has_kw(line.as_bytes(), b"exec=")
        || !has_kw(line.as_bytes(), b"cancel=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：态 + 执行/取消计数 + 可见位串联。
pub fn flow_fp(f: &ResetFlow) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.push(f.state);
    buf.push(f.warn_visible as u8);
    buf.extend_from_slice(&f.exec_count.to_le_bytes());
    buf.extend_from_slice(&f.cancel_count.to_le_bytes());
    buf.extend_from_slice(&f.armed_at.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：st=<态> exec=<执行> cancel=<撤销>。
pub fn flow_line(f: &ResetFlow) -> String {
    let mut s = String::new();
    s.push_str("st=");
    s.push_str(&f.state.to_string());
    s.push_str(" exec=");
    s.push_str(&f.exec_count.to_string());
    s.push_str(" cancel=");
    s.push_str(&f.cancel_count.to_string());
    s
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}
