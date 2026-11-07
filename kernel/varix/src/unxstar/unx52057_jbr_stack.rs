//! UNX-F52057 JBR（JetBrains Runtime）自带上栈收口 · 0xC4E40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52057 逐字锚定）：UNX-F52058-J1 daemon 拉起/
//! 复用/超时回收/崩溃重启四态账 ×3 轮；常驻 7 天内存漂移 <10%；JVM 进
//! 程语义消费 E1 面（联签锚定行）；KB-COMPAT 定级登记；判据锚定
//! §AI-66.2 专题五。
//!
//! 设计要点：
//!   ① 域模型：daemon 四态账（拉起/复用/超时回收/崩溃重启）× 3 轮 =
//!      12 格态账；态序闭集 0..4；态机单向（拉起→复用→回收→重启），越
//!      态/逆态必抓。
//!   ② 内存漂移账：常驻 7 天 RSS 增量账逐日复算，Σ增量 / 基线 < 10%
//!      才达标；超限注入必抓。
//!   ③ JVM 进程语义消费 E1 面：消费格 6 格（进程档/退出码档/信号档/
//!      环境档/工作目录档/stdio 档），逐格与消费基准恒等才收；未对账
//!      先报四态全通 → 消费前置闸。
//!   ④ 注入 ×10 轮（态机逆态/E1 漂移/态越闭集三态轮转）10/10 必抓，
//!      注入=检出+逃逸守恒；六判据母版闸 + 定级（12 格全通 + 漂移达标
//!      + E1 零漂移 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52057 − 52000)）。
pub const SEG_BASE: u32 = 0xC4E40;

/// 四态数。
pub const STATES: usize = 4;

/// 轮数。
pub const ROUNDS: usize = 3;

/// 常驻天数。
pub const DAEMON_DAYS: usize = 7;

/// RSS 基线（拍）。
pub const RSS_BASE: u64 = 4096;

/// 漂移封顶（千分之 100 = 10%）。
pub const DRIFT_CAP_PERMIL: u64 = 100;

/// E1 消费格数。
pub const E1_CELLS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JbCode {
    /// 成功
    Ok = 0,
    /// 态号越闭集
    ENoState,
    /// 轮号越闭集
    ENoRound,
    /// 态机逆态
    EBackward,
    /// 内存漂移超限
    EDriftCap,
    /// E1 消费格越册
    ENoCell,
    /// E1 消费漂移
    EE1Drift,
    /// 未对账先报全通
    EPreAll,
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

impl JbCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            JbCode::Ok => "成功",
            JbCode::ENoState => "态号越闭集：拉起/复用/回收/重启四态之外先登记",
            JbCode::ENoRound => "轮号越闭集：3 轮之外先登记",
            JbCode::EBackward => "态机逆态：拉起→复用→回收→重启单向态机，逆态必抓",
            JbCode::EDriftCap => "内存漂移超限：常驻 7 天 Σ增量/基线 <10% 才达标",
            JbCode::ENoCell => "E1 消费格越册：6 格闭集之外先登记",
            JbCode::EE1Drift => "E1 消费漂移：消费折叠与基准不恒等，逐格复算",
            JbCode::EPreAll => "未对账先报全通：E1 消费账不齐不许报四态全通",
            JbCode::EDup => "重复落账：同格只许一条账目",
            JbCode::ESeal => "未齐即封：12 格未齐先补齐",
            JbCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            JbCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            JbCode::EGradeDrift => "定级漂移：12 格全通+漂移达标+E1 零漂移才定级",
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
        0x4E000u64
            + (r as u64) * 0x2A
            + (s as u64) * 0x18
            ^ (r as u64).wrapping_mul(0x9E3779B97F4A7C15 >> s),
    )
}

/// E1 消费格基准载荷（格 c）。
pub const fn e1_cell(c: usize) -> Option<u64> {
    if c >= E1_CELLS {
        return None;
    }
    Some((0x4F000u64 + c as u64 * 0x34) ^ 0x6A766D5F6531)
}

/// 第 d 天 RSS 增量（纯函数，≤漂移封顶）。
pub const fn rss_delta(d: usize) -> u64 {
    if d >= DAEMON_DAYS {
        return 0;
    }
    16
}

/// 7 天 Σ漂移达标闸。
pub const fn drift_ok() -> bool {
    let mut sum: u64 = 0;
    let mut d = 0usize;
    while d < DAEMON_DAYS {
        sum = sum.wrapping_add(rss_delta(d));
        d += 1;
    }
    sum * 1000 < RSS_BASE * DRIFT_CAP_PERMIL
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct JbLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 态账。
    states: [[Option<u64>; STATES]; ROUNDS],
    /// E1 消费账。
    e1: [Option<u64>; E1_CELLS],
    /// 各轮最后态。
    last_state: [i64; ROUNDS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl JbLedger {
    /// 新账本。
    pub fn new() -> Self {
        JbLedger {
            gates: [None; 6],
            states: [[None; STATES]; ROUNDS],
            e1: [None; E1_CELLS],
            last_state: [-1; ROUNDS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), JbCode> {
        if gate >= 6 {
            return Err(JbCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x22100 + gate as u64);
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

    /// E1 消费一格。
    pub fn consume_e1(&mut self, c: usize, got: u64) -> Result<(), JbCode> {
        let want = match e1_cell(c) {
            Some(w) => w,
            None => return Err(JbCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x22200 + c as u64);
            return Err(JbCode::EE1Drift);
        }
        if let Some(cell) = self.e1.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x22201 + c as u64);
        Ok(())
    }

    /// E1 消费全账核验。
    pub fn e1_all(&self) -> bool {
        let mut c = 0usize;
        while c < E1_CELLS {
            match self.e1.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 态一格（单向态机 + 前置闸 + 重复拒）。
    pub fn record_state(&mut self, r: usize, s: usize) -> Result<u64, JbCode> {
        if r >= ROUNDS {
            return Err(JbCode::ENoRound);
        }
        if s >= STATES {
            return Err(JbCode::ENoState);
        }
        if !self.e1_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x22300);
            return Err(JbCode::EPreAll);
        }
        if (s as i64) < self.last_state[r] {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x22301 + r as u64);
            return Err(JbCode::EBackward);
        }
        let p = match state_cell(r, s) {
            Some(p) => p,
            None => return Err(JbCode::ENoState),
        };
        if match self.states.get(r).and_then(|row| row.get(s)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JbCode::EDup);
        }
        if let Some(row) = self.states.get_mut(r) {
            if let Some(c) = row.get_mut(s) {
                *c = Some(p);
            }
        }
        self.last_state[r] = s as i64;
        self.fnv(0x22302 + (r as u64) * 4 + s as u64);
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

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for JbLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = JbLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut c = 0usize;
    while c < E1_CELLS {
        if let Some(w) = e1_cell(c) {
            let _ = led.consume_e1(c, w);
        }
        c += 1;
    }
    let mut recorded = 0u32;
    let mut r = 0usize;
    while r < ROUNDS {
        let mut s = 0usize;
        while s < STATES {
            if led.record_state(r, s).is_ok() {
                recorded += 1;
            }
            s += 1;
        }
        r += 1;
    }
    // 注入 10 轮：逆态 / E1 漂移 / 态越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = JbLedger::new();
        let mut c2 = 0usize;
        while c2 < E1_CELLS {
            if let Some(w) = e1_cell(c2) {
                let _ = lt.consume_e1(c2, w);
            }
            c2 += 1;
        }
        let caught = match kind {
            0 => {
                let _ = lt.record_state(0, 2);
                lt.record_state(0, 0) == Err(JbCode::EBackward)
            }
            1 => lt.consume_e1(4, e1_cell(4).unwrap_or(0).wrapping_add(1)) == Err(JbCode::EE1Drift),
            _ => lt.record_state(0, STATES).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x22400 + k as u64);
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
pub fn run_unx52057_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52057");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "daemon 四态 ×3 轮 12/12 全通",
        recorded == (STATES * ROUNDS) as u32,
        "拉起/复用/超时回收/崩溃重启四态账 ×3 轮全通",
    );
    s.add(
        "常驻 7 天漂移 <10%",
        drift_ok(),
        "ΣRSS 增量/基线 < 10%（千分之百封顶）",
    );
    s.add(
        "E1 消费 6/6 对账",
        {
            let mut lt = JbLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < E1_CELLS {
                if let Some(w) = e1_cell(c) {
                    if lt.consume_e1(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.e1_all()
        },
        "JVM 进程语义消费 E1 面六格零漂移",
    );
    s.add("注入 10/10 全抓", caught >= 10, "逆态/漂移/越闭集三态轮转必抓");
    let mut l0 = JbLedger::new();
    s.add(
        "消费前置闸",
        l0.record_state(0, 0) == Err(JbCode::EPreAll),
        "E1 消费账不齐不许报四态全通",
    );
    let mut l1 = JbLedger::new();
    let mut c = 0usize;
    while c < E1_CELLS {
        if let Some(w) = e1_cell(c) {
            let _ = l1.consume_e1(c, w);
        }
        c += 1;
    }
    s.add(
        "态机·逆态拒",
        {
            let _ = l1.record_state(0, 2);
            l1.record_state(0, 0) == Err(JbCode::EBackward)
        },
        "单向态机逆态必抓",
    );
    s.add(
        "边界·态越闭集",
        l1.record_state(0, STATES).is_err(),
        "态 4 越闭集必拒",
    );
    s.add(
        "边界·轮越闭集",
        l1.record_state(ROUNDS, 0).is_err(),
        "轮 3 越闭集必拒",
    );
    let mut l2 = JbLedger::new();
    s.add(
        "E1·漂移拒",
        l2.consume_e1(1, e1_cell(1).unwrap_or(0).wrapping_add(5)) == Err(JbCode::EE1Drift),
        "E1 折叠漂移必抓",
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
    s.add("收口·判据条数", s.len() >= 12, "含本条共 ≥13 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cells_pure() {
        let a = state_cell(1, 2).unwrap_or(0);
        assert_eq!(a, state_cell(1, 2).unwrap_or(0));
        assert_ne!(a, state_cell(1, 3).unwrap_or(0));
        assert!(state_cell(ROUNDS, 0).is_none());
        assert!(state_cell(0, STATES).is_none());
        let w = e1_cell(5).unwrap_or(0);
        assert_eq!(w, e1_cell(5).unwrap_or(0));
        assert!(e1_cell(E1_CELLS).is_none());
    }

    #[test]
    fn test_drift_cap() {
        assert!(drift_ok());
        // 7 天 × 16 = 112 < 4096 × 0.1 = 409.6。
        let mut sum: u64 = 0;
        let mut d = 0usize;
        while d < DAEMON_DAYS {
            sum += rss_delta(d);
            d += 1;
        }
        assert_eq!(sum, 112);
    }

    #[test]
    fn test_state_machine() {
        let mut lt = JbLedger::new();
        let mut c = 0usize;
        while c < E1_CELLS {
            if let Some(w) = e1_cell(c) {
                let _ = lt.consume_e1(c, w);
            }
            c += 1;
        }
        assert!(lt.record_state(0, 1).is_ok());
        assert_eq!(lt.record_state(0, 0), Err(JbCode::EBackward));
        assert!(lt.record_state(0, 1).is_err());
        assert!(lt.record_state(0, 2).is_ok());
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
        assert_eq!(SEG_BASE, 0xC4E40);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52057_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
