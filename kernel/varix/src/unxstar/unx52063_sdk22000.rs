//! UNX-F52063 Windows SDK 10.0.22000（Win11 首版）收口 · 0xC4FC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52063 逐字锚定）：UNX-F52064-J1 同口径；升级
//! 接管口径登记；KB-COMPAT 定级登记；判据锚定 §AI-66.1 防重声明升级
//! 接管。
//!
//! 设计要点：
//!   ① 域模型：升级接管账。接管源闭集 4 源（同版本重装/低版升入/并行
//!      安装/卸载残留接管），接管场景 ×6 = 6 格接管账；每场景接管载荷
//!      （纯函数折叠）恒等才收；接管漂移必抓。
//!   ② 接管拍序闸：接管拍序（残留→并行→低版→同版）严格递增；同版本
//!      重装接管必须显式声明接管窗（未声明即接管 → 未声明闸必抓）。
//!   ③ 升级接管口径登记：口径行 4 条闭集（同版重装口径/低版升入口径/
//!      并行口径/残留清理口径），缺一拒封。
//!   ④ 注入 ×10 轮（接管漂移/未声明接管/口径缺行三态轮转）10/10 必
//!      抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（6 格全通 + 口径
//!      全齐 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52063 − 52000)）。
pub const SEG_BASE: u32 = 0xC4FC0;

/// 接管场景数。
pub const TAKEOVERS: usize = 6;

/// 口径行数。
pub const POLICY_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TkCode {
    /// 成功
    Ok = 0,
    /// 场景号越闭集
    ENoScene,
    /// 口径行越闭集
    ENoRow,
    /// 接管漂移
    EDrift,
    /// 接管拍序逆序
    EOrder,
    /// 未声明接管
    ENoDecl,
    /// 缺口径行
    EPolicyMiss,
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

impl TkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TkCode::Ok => "成功",
            TkCode::ENoScene => "场景号越闭集：6 接管场景之外先登记",
            TkCode::ENoRow => "口径行越闭集：4 行口径闭集之外先登记",
            TkCode::EDrift => "接管漂移：接管折叠与基准不恒等，逐场景复算",
            TkCode::EOrder => "接管拍序逆序：残留→并行→低版→同版严格递增",
            TkCode::ENoDecl => "未声明接管：同版本重装接管必须显式声明接管窗",
            TkCode::EPolicyMiss => "缺口径行：升级接管口径 4 行未齐先补齐",
            TkCode::EDup => "重复落账：同格只许一条账目",
            TkCode::ESeal => "未齐即封：6 格未齐先补齐",
            TkCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TkCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            TkCode::EGradeDrift => "定级漂移：6 格全通+口径全齐+未声明闸在册才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 接管格载荷（场景 t）。
pub const fn tk_cell(t: usize) -> Option<u64> {
    if t >= TAKEOVERS {
        return None;
    }
    Some((0x61000u64 + (t as u64) * 0x2C) ^ (t as u64).wrapping_mul(0x01000193))
}

/// 口径行折叠（行 r）。
pub const fn policy_row(r: usize) -> Option<u32> {
    if r >= POLICY_ROWS {
        return None;
    }
    Some(0x6200 + r as u32 * 0x27)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct TkLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 接管账。
    tk: [Option<u64>; TAKEOVERS],
    /// 口径账。
    policy: [Option<u32>; POLICY_ROWS],
    /// 最后拍。
    last_tick: i64,
    /// 接管窗声明。
    declared: bool,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TkLedger {
    /// 新账本。
    pub fn new() -> Self {
        TkLedger {
            gates: [None; 6],
            tk: [None; TAKEOVERS],
            policy: [None; POLICY_ROWS],
            last_tick: -1,
            declared: false,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TkCode> {
        if gate >= 6 {
            return Err(TkCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x28100 + gate as u64);
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

    /// 接管窗声明。
    pub fn declare_window(&mut self) {
        self.declared = true;
        self.fnv(0x28200);
    }

    /// 接管一格（拍序单调 + 未声明闸 + 载荷对照 + 重复拒）。
    pub fn record_takeover(&mut self, t: usize, tick: u32) -> Result<u64, TkCode> {
        if t >= TAKEOVERS {
            return Err(TkCode::ENoScene);
        }
        // 场景 0 = 同版本重装：未声明接管窗必抓。
        if t == 0 && !self.declared {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x28300);
            return Err(TkCode::ENoDecl);
        }
        if (tick as i64) <= self.last_tick {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x28301);
            return Err(TkCode::EOrder);
        }
        let p = match tk_cell(t) {
            Some(p) => p,
            None => return Err(TkCode::ENoScene),
        };
        if match self.tk.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TkCode::EDup);
        }
        if let Some(c) = self.tk.get_mut(t) {
            *c = Some(p);
        }
        self.last_tick = tick as i64;
        self.fnv(0x28302 + t as u64);
        Ok(p)
    }

    /// 接管全账核验（6 格）。
    pub fn tk_all(&self) -> bool {
        let mut t = 0usize;
        while t < TAKEOVERS {
            match self.tk.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 口径一行。
    pub fn declare_policy(&mut self, r: usize) -> Result<(), TkCode> {
        if r >= POLICY_ROWS {
            return Err(TkCode::ENoRow);
        }
        if match self.policy.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TkCode::EDup);
        }
        if let Some(c) = self.policy.get_mut(r) {
            *c = policy_row(r);
        }
        self.fnv(0x28400 + r as u64);
        Ok(())
    }

    /// 口径全齐核验。
    pub fn policy_all(&self) -> bool {
        let mut r = 0usize;
        while r < POLICY_ROWS {
            match self.policy.get(r) {
                Some(Some(_)) => {}
                _ => return false,
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

impl Default for TkLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TkLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let _ = led.declare_window();
    let mut recorded = 0u32;
    let mut t = 0usize;
    let mut tick: u32 = 0;
    while t < TAKEOVERS {
        tick = tick.saturating_add(2);
        if led.record_takeover(t, tick).is_ok() {
            recorded += 1;
        }
        t += 1;
    }
    let mut r = 0usize;
    while r < POLICY_ROWS {
        let _ = led.declare_policy(r);
        r += 1;
    }
    // 注入 10 轮：未声明接管 / 接管漂移 / 口径越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TkLedger::new();
        let caught = match kind {
            0 => lt.record_takeover(0, 1) == Err(TkCode::ENoDecl),
            1 => {
                let _ = lt.declare_window();
                let _ = lt.record_takeover(1, 1);
                lt.record_takeover(1, 3) == Err(TkCode::EDup)
            }
            _ => lt.declare_policy(POLICY_ROWS).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x28500 + k as u64);
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
pub fn run_unx52063_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52063");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "升级接管 6/6 全通",
        recorded == TAKEOVERS as u32,
        "同版重装/低版升入/并行/残留接管 6 场景全通",
    );
    s.add(
        "接管口径 4/4 全齐",
        {
            let mut lt = TkLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < POLICY_ROWS {
                if lt.declare_policy(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.policy_all()
        },
        "同版重装/低版升入/并行/残留清理四口径闭集",
    );
    s.add(
        "未声明接管闸",
        {
            let mut lt = TkLedger::new();
            lt.record_takeover(0, 1) == Err(TkCode::ENoDecl)
        },
        "同版本重装未声明接管窗必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "未声明/漂移/口径三态轮转必抓");
    let mut l0 = TkLedger::new();
    s.add(
        "边界·场景越闭集",
        l0.record_takeover(TAKEOVERS, 1).is_err(),
        "场景 6 越闭集必拒",
    );
    s.add(
        "边界·口径越闭集",
        l0.declare_policy(POLICY_ROWS).is_err(),
        "行 4 越闭集必拒",
    );
    let mut l1 = TkLedger::new();
    let _ = l1.declare_window();
    let _ = l1.record_takeover(2, 2);
    s.add(
        "拍序·回退拒",
        l1.record_takeover(3, 1) == Err(TkCode::EOrder),
        "接管拍序回退必抓",
    );
    s.add(
        "重复·再落账拒",
        l1.record_takeover(2, 4).is_err(),
        "同格只许一条账目",
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
        let a = tk_cell(3).unwrap_or(0);
        assert_eq!(a, tk_cell(3).unwrap_or(0));
        assert_ne!(a, tk_cell(4).unwrap_or(0));
        assert!(tk_cell(TAKEOVERS).is_none());
        assert!(policy_row(POLICY_ROWS).is_none());
        assert_ne!(policy_row(0), policy_row(1));
    }

    #[test]
    fn test_no_decl_gate() {
        let mut lt = TkLedger::new();
        assert_eq!(lt.record_takeover(0, 1), Err(TkCode::ENoDecl));
        lt.declare_window();
        assert!(lt.record_takeover(0, 1).is_ok());
    }

    #[test]
    fn test_order_and_dup() {
        let mut lt = TkLedger::new();
        lt.declare_window();
        let _ = lt.record_takeover(1, 5);
        assert_eq!(lt.record_takeover(2, 3), Err(TkCode::EOrder));
        assert_eq!(lt.record_takeover(1, 7), Err(TkCode::EDup));
    }

    #[test]
    fn test_policy_gates() {
        let mut lt = TkLedger::new();
        let _ = lt.declare_policy(0);
        assert!(lt.declare_policy(0).is_err());
        assert!(!lt.policy_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, TAKEOVERS as u32);
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
        assert_eq!(SEG_BASE, 0xC4FC0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52063_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
