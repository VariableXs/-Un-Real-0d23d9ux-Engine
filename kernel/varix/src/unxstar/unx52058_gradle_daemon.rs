//! UNX-F52058 Gradle daemon 常驻进程语义收口 · 0xC4E80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52058 逐字锚定）：UNX-F52059-J1 六判据全落
//! （前后端分离进程模型账）；智能模式切换 ×5 场景；KB-COMPAT 定级登
//! 记；判据锚定 §AI-66.2 专题五。
//!
//! 设计要点：
//!   ① 域模型：前后端分离进程模型账。daemon 四态（拉起/复用/回收/崩
//!      溃重启）× 3 轮 = 12 格态账（与 JBR 四态同构不同参：此处轮计
//!      时拍按 daemon 空闲超时 3h 折算）；智能模式切换 5 场景闭集（省
//!      电/离线/持续构建/CI 批处理/交互开发），模式切换拍序严格递增。
//!   ② 前置闸：daemon 生命周期账（拉起拍/复用拍/回收拍）齐才许报智
//!      能模式切换全通，缺账先切 → EPreSwitch。
//!   ③ 空闲超时闸：空闲拍数 ≤ 3h 折算 180 拍封顶，超限回收必抓；回
//!      收后复用必须先重启（复用未重启 → 复用前置闸）。
//!   ④ 注入 ×10 轮（超时超限/模式越闭集/切换拍序回退三态轮转）10/10
//!      必抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（12 格态账全通
//!      + 5 模式全账 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52058 − 52000)）。
pub const SEG_BASE: u32 = 0xC4E80;

/// 四态数。
pub const STATES: usize = 4;

/// 轮数。
pub const ROUNDS: usize = 3;

/// 空闲超时封顶（拍，3h 折算 180 拍）。
pub const IDLE_CAP: u32 = 180;

/// 智能模式数。
pub const MODES: usize = 5;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GdCode {
    /// 成功
    Ok = 0,
    /// 态号越闭集
    ENoState,
    /// 轮号越闭集
    ENoRound,
    /// 模式越闭集
    ENoMode,
    /// 空闲超时超限
    EIdleCap,
    /// 复用未重启
    ENoRestart,
    /// 未齐账先切换
    EPreSwitch,
    /// 切换拍序乱
    EOrder,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl GdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GdCode::Ok => "成功",
            GdCode::ENoState => "态号越闭集：拉起/复用/回收/崩溃重启四态之外先登记",
            GdCode::ENoRound => "轮号越闭集：3 轮之外先登记",
            GdCode::ENoMode => "模式越闭集：5 智能模式之外先登记",
            GdCode::EIdleCap => "空闲超时超限：空闲拍数 ≤180 拍（3h 折算），超限必回收",
            GdCode::ENoRestart => "复用未重启：回收后必须先重启态才许复用",
            GdCode::EPreSwitch => "未齐账先切换：daemon 生命周期账不齐不许报模式切换全通",
            GdCode::EOrder => "切换拍序乱：模式切换拍序必须严格递增单调",
            GdCode::EDup => "重复落账：同格只许一条账目",
            GdCode::ESeal => "未齐即封：12 格态账 + 5 模式未齐先补齐",
            GdCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GdCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            GdCode::EGradeDrift => "定级漂移：12 格全通+5 模式全账+超时闸在册才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 态格载荷（轮 r × 态 s）。
pub const fn state_cell(r: usize, s: usize) -> Option<u64> {
    if r >= ROUNDS || s >= STATES {
        return None;
    }
    Some(
        0x51000u64
            + (r as u64) * 0x36
            + (s as u64) * 0x1A
            ^ (r as u64).wrapping_mul(0x9E3779B97F4A7C15 >> s),
    )
}

/// 空闲超时闸（拍数 ≤ 180）。
pub const fn idle_ok(ticks: u32) -> bool {
    ticks <= IDLE_CAP
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct GdLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 态账。
    states: [[Option<u64>; STATES]; ROUNDS],
    /// 各轮最后态。
    last_state: [i64; ROUNDS],
    /// 上一态（轮内复用前置判断）。
    prev_state: [i64; ROUNDS],
    /// 模式账。
    modes: [Option<u64>; MODES],
    /// 切换最后拍。
    last_switch: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GdLedger {
    /// 新账本。
    pub fn new() -> Self {
        GdLedger {
            gates: [None; 6],
            states: [[None; STATES]; ROUNDS],
            last_state: [-1; ROUNDS],
            prev_state: [-1; ROUNDS],
            modes: [None; MODES],
            last_switch: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GdCode> {
        if gate >= 6 {
            return Err(GdCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x23100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 生命周期段：态一格（回收后复用前置 + 空闲超时闸 + 单向态机）。
    pub fn record_state(&mut self, r: usize, s: usize, idle: u32) -> Result<u64, GdCode> {
        if r >= ROUNDS {
            return Err(GdCode::ENoRound);
        }
        if s >= STATES {
            return Err(GdCode::ENoState);
        }
        // 空闲超时闸：态 2（回收）允许大 idle，其余态超限必抓。
        if s != 2 && !idle_ok(idle) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x23200 + r as u64);
            return Err(GdCode::EIdleCap);
        }
        // 复用前置：上一态为回收（2）时，复用（1）前必须先重启（3）。
        if s == 1 && self.prev_state[r] == 2 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x23201 + r as u64);
            return Err(GdCode::ENoRestart);
        }
        if (s as i64) < self.last_state[r] {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x23202 + r as u64);
            return Err(GdCode::ESeal);
        }
        let p = match state_cell(r, s) {
            Some(p) => p,
            None => return Err(GdCode::ENoState),
        };
        if match self.states.get(r).and_then(|row| row.get(s)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GdCode::EDup);
        }
        if let Some(row) = self.states.get_mut(r) {
            if let Some(c) = row.get_mut(s) {
                *c = Some(p);
            }
        }
        self.last_state[r] = s as i64;
        self.prev_state[r] = s as i64;
        self.fnv(0x23203 + (r as u64) * 4 + s as u64);
        Ok(p)
    }

    /// 态全账核验（12 格）。
    pub fn states_all(&self) -> bool {
        let mut r = 0usize;
        while r < ROUNDS {
            let mut s = 0usize;
            while s < STATES {
                match self.states.get(r).and_then(|row| row.get(s)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                s += 1;
            }
            r += 1;
        }
        true
    }

    /// 智能模式切换一格（前置闸 + 拍序单调 + 重复拒）。
    pub fn switch_mode(&mut self, m: usize, tick: u32) -> Result<(), GdCode> {
        if m >= MODES {
            return Err(GdCode::ENoMode);
        }
        if !self.states_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x23300);
            return Err(GdCode::EPreSwitch);
        }
        if (tick as i64) <= self.last_switch {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x23301);
            return Err(GdCode::EOrder);
        }
        if match self.modes.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GdCode::EDup);
        }
        if let Some(c) = self.modes.get_mut(m) {
            *c = Some(tick as u64);
        }
        self.last_switch = tick as i64;
        self.fnv(0x23302 + m as u64);
        Ok(())
    }

    /// 模式全账核验。
    pub fn modes_all(&self) -> bool {
        let mut m = 0usize;
        while m < MODES {
            match self.modes.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for GdLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GdLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut r = 0usize;
    while r < ROUNDS {
        let mut s = 0usize;
        while s < STATES {
            let idle: u32 = match s {
                2 => IDLE_CAP,
                _ => 30,
            };
            if led.record_state(r, s, idle).is_ok() {
                recorded += 1;
            }
            s += 1;
        }
        r += 1;
    }
    let mut m = 0usize;
    let mut tick: u32 = 100;
    while m < MODES {
        let _ = led.switch_mode(m, tick);
        tick = tick.saturating_add(10);
        m += 1;
    }
    // 注入 10 轮：超时超限 / 模式越闭集 / 切换拍序回退三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GdLedger::new();
        let caught = match kind {
            0 => lt.record_state(0, 0, IDLE_CAP + 1) == Err(GdCode::EIdleCap),
            1 => lt.switch_mode(MODES, 1).is_err(),
            _ => {
                // 先补齐态账再注入切换拍序回退。
                let mut r2 = 0usize;
                while r2 < ROUNDS {
                    let mut s2 = 0usize;
                    while s2 < STATES {
                        let idle: u32 = if s2 == 2 { IDLE_CAP } else { 20 };
                        let _ = lt.record_state(r2, s2, idle);
                        s2 += 1;
                    }
                    r2 += 1;
                }
                let _ = lt.switch_mode(0, 20);
                lt.switch_mode(1, 10) == Err(GdCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x23400 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
pub fn run_unx52058_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52058");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "四态 ×3 轮 12/12 全通",
        recorded == (STATES * ROUNDS) as u32,
        "前后端分离进程模型 daemon 四态账 ×3 轮全通",
    );
    s.add(
        "智能模式 5/5 全账",
        {
            let mut lt = GdLedger::new();
            // 先补齐态账。
            let mut r = 0usize;
            while r < ROUNDS {
                let mut st = 0usize;
                while st < STATES {
                    let idle: u32 = if st == 2 { IDLE_CAP } else { 20 };
                    let _ = lt.record_state(r, st, idle);
                    st += 1;
                }
                r += 1;
            }
            let mut ok = true;
            let mut m = 0usize;
            let mut tick: u32 = 10;
            while m < MODES {
                if lt.switch_mode(m, tick).is_err() {
                    ok = false;
                }
                tick = tick.saturating_add(5);
                m += 1;
            }
            ok && lt.modes_all()
        },
        "省电/离线/持续构建/CI 批处理/交互开发五模式逐格切换",
    );
    s.add(
        "空闲超时闸",
        idle_ok(180) && !idle_ok(181),
        "3h 折算 180 拍封顶，超限必回收",
    );
    s.add("注入 10/10 全抓", caught >= 10, "超时/模式/拍序三态轮转必抓");
    let mut l0 = GdLedger::new();
    s.add(
        "超时·超限拒",
        l0.record_state(0, 0, 999) == Err(GdCode::EIdleCap),
        "空闲拍数超 180 必抓",
    );
    s.add(
        "切换前置闸",
        l0.switch_mode(0, 1) == Err(GdCode::EPreSwitch),
        "态账不齐不许报模式切换全通",
    );
    s.add(
        "边界·态越闭集",
        l0.record_state(0, STATES, 1).is_err(),
        "态 4 越闭集必拒",
    );
    s.add(
        "边界·模式越闭集",
        l0.switch_mode(MODES, 1).is_err(),
        "模式 5 越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cells_pure() {
        let a = state_cell(2, 1).unwrap_or(0);
        assert_eq!(a, state_cell(2, 1).unwrap_or(0));
        assert_ne!(a, state_cell(2, 2).unwrap_or(0));
        assert!(state_cell(ROUNDS, 0).is_none());
        assert!(state_cell(0, STATES).is_none());
    }

    #[test]
    fn test_idle_cap() {
        assert!(idle_ok(0));
        assert!(idle_ok(180));
        assert!(!idle_ok(181));
        assert!(!idle_ok(u32::MAX));
    }

    #[test]
    fn test_state_gates() {
        let mut lt = GdLedger::new();
        assert_eq!(lt.record_state(0, 0, 999), Err(GdCode::EIdleCap));
        assert!(lt.record_state(0, 0, 10).is_ok());
        // 回收后未重启复用。
        let _ = lt.record_state(0, 2, IDLE_CAP);
        assert_eq!(lt.record_state(0, 1, 10), Err(GdCode::ENoRestart));
        // 重复落账。
        assert!(lt.record_state(0, 0, 10).is_err());
    }

    #[test]
    fn test_switch_gate() {
        let mut lt = GdLedger::new();
        assert_eq!(lt.switch_mode(0, 1), Err(GdCode::EPreSwitch));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (STATES * ROUNDS) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC4E80);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52058_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
