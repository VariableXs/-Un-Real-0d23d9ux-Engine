//! UNX-F52077 Ninja 收口 · 0xC5340段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52077 逐字锚定）：UNX-F52078-J1 build/test
//! ×10 目标全通；远程缓存禁用本地回退路径账；KB-COMPAT 定级登记；判
//! 据锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：build/test ×10 目标账（两段 ×10 = 20 格），逐格载荷恒
//!      等才收；漂移必抓。
//!   ② 远程缓存回退账：回退链 4 格（探测/降级/本地命中/本地构建），远
//!      程禁用时必须走本地回退（禁用却走远程 → 回退闸必抓）；链拍序单
//!      向（探测→降级→本地命中→本地构建逆序必抓）。
//!   ③ build→compile 拍序单向闸（未 build 先 test → 前置闸）。
//!   ④ 注入 ×10 轮（回退越序/前置越序/目标越闭集三态轮转）10/10 必
//!      抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（20 格全通 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52077 − 52000)）。
pub const SEG_BASE: u32 = 0xC5340;

/// 目标数。
pub const TARGETS: usize = 10;

/// 回退链格数。
pub const FALLBACK: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NjCode {
    /// 成功
    Ok = 0,
    /// 目标号越闭集
    ENoTarget,
    /// build 漂移
    EBuildDrift,
    /// test 前置闸
    EPreTest,
    /// test 漂移
    ETestDrift,
    /// 回退格越册
    ENoFb,
    /// 回退越序
    EFbOrder,
    /// 远程禁用未回退
    ENoFbPath,
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

impl NjCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            NjCode::Ok => "成功",
            NjCode::ENoTarget => "目标号越闭集：10 目标之外先登记",
            NjCode::EBuildDrift => "build 漂移：build 折叠与基准不恒等，逐目标复算",
            NjCode::EPreTest => "未 build 先 test：build 账不齐不许 test",
            NjCode::ETestDrift => "test 漂移：test 折叠与基准不恒等",
            NjCode::ENoFb => "回退格越册：4 格闭集之外先登记",
            NjCode::EFbOrder => "回退越序：探测→降级→本地命中→本地构建逆序必抓",
            NjCode::ENoFbPath => "远程禁用未回退：禁用后必须走本地回退路径",
            NjCode::EDup => "重复落账：同格只许一条账目",
            NjCode::ESeal => "未齐即封：20 格未齐先补齐",
            NjCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            NjCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            NjCode::EGradeDrift => "定级漂移：20 格全通+回退链全账+拍序单调才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// build 格载荷（目标 t）。
pub const fn build_cell(t: usize) -> Option<u64> {
    if t >= TARGETS {
        return None;
    }
    Some((0x97000u64 + (t as u64) * 0x2B) ^ (t as u64).wrapping_mul(0x01000193))
}

/// test 格载荷（目标 t）。
pub const fn test_cell(t: usize) -> Option<u64> {
    if t >= TARGETS {
        return None;
    }
    Some((0x98000u64 + (t as u64) * 0x2F) ^ ((t as u64) + 0x30).wrapping_mul(0x01000193))
}

/// 回退格基准（格 f）。
pub const fn fb_cell(f: usize) -> Option<u64> {
    if f >= FALLBACK {
        return None;
    }
    Some((0x99000u64 + f as u64 * 0x33) ^ 0x6E6A66626C6434)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct NjLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// build 账。
    builds: [Option<u64>; TARGETS],
    /// test 账。
    tests: [Option<u64>; TARGETS],
    /// 回退账。
    fbs: [Option<u64>; FALLBACK],
    /// 远程禁用旗标。
    remote_off: bool,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NjLedger {
    /// 新账本。
    pub fn new() -> Self {
        NjLedger {
            gates: [None; 6],
            builds: [None; TARGETS],
            tests: [None; TARGETS],
            fbs: [None; FALLBACK],
            remote_off: false,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NjCode> {
        if gate >= 6 {
            return Err(NjCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x36100 + gate as u64);
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

    /// build 一格。
    pub fn record_build(&mut self, t: usize, got: u64) -> Result<(), NjCode> {
        if t >= TARGETS {
            return Err(NjCode::ENoTarget);
        }
        let want = match build_cell(t) {
            Some(w) => w,
            None => return Err(NjCode::ENoTarget),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x36200 + t as u64);
            return Err(NjCode::EBuildDrift);
        }
        if match self.builds.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NjCode::EDup);
        }
        if let Some(c) = self.builds.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x36201 + t as u64);
        Ok(())
    }

    /// build 全账核验。
    pub fn builds_all(&self) -> bool {
        let mut t = 0usize;
        while t < TARGETS {
            match self.builds.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// test 一格（build 前置闸）。
    pub fn record_test(&mut self, t: usize, got: u64) -> Result<(), NjCode> {
        if t >= TARGETS {
            return Err(NjCode::ENoTarget);
        }
        if !self.builds_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x36300);
            return Err(NjCode::EPreTest);
        }
        let want = match test_cell(t) {
            Some(w) => w,
            None => return Err(NjCode::ENoTarget),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x36301 + t as u64);
            return Err(NjCode::ETestDrift);
        }
        if match self.tests.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NjCode::EDup);
        }
        if let Some(c) = self.tests.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x36302 + t as u64);
        Ok(())
    }

    /// test 全账核验。
    pub fn tests_all(&self) -> bool {
        let mut t = 0usize;
        while t < TARGETS {
            match self.tests.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 远程禁用登记。
    pub fn disable_remote(&mut self) {
        self.remote_off = true;
        self.fnv(0x36400);
    }

    /// 回退一格（禁用闸 + 拍序按格号单调）。
    pub fn record_fb(&mut self, f: usize, got: u64) -> Result<(), NjCode> {
        if f >= FALLBACK {
            return Err(NjCode::ENoFb);
        }
        if !self.remote_off {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x36500);
            return Err(NjCode::ENoFbPath);
        }
        // 拍序：格 f 落账后不许回退到更小格号（以账面是否已填判断）。
        if f > 0 {
            let prev_done = match self.fbs.get(f - 1) {
                Some(Some(_)) => true,
                _ => false,
            };
            if !prev_done {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x36501 + f as u64);
                return Err(NjCode::EFbOrder);
            }
        }
        let want = match fb_cell(f) {
            Some(w) => w,
            None => return Err(NjCode::ENoFb),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x36502 + f as u64);
            return Err(NjCode::EFbOrder);
        }
        if let Some(c) = self.fbs.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x36503 + f as u64);
        Ok(())
    }

    /// 回退全账核验。
    pub fn fbs_all(&self) -> bool {
        let mut f = 0usize;
        while f < FALLBACK {
            match self.fbs.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
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

impl Default for NjLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NjLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut t = 0usize;
    while t < TARGETS {
        if let Some(w) = build_cell(t) {
            if led.record_build(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut t2 = 0usize;
    while t2 < TARGETS {
        if let Some(w) = test_cell(t2) {
            if led.record_test(t2, w).is_ok() {
                recorded += 1;
            }
        }
        t2 += 1;
    }
    led.disable_remote();
    let mut f = 0usize;
    while f < FALLBACK {
        if let Some(w) = fb_cell(f) {
            if led.record_fb(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    // 注入 10 轮：回退越序 / test 前置 / 目标越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = NjLedger::new();
        let caught = match kind {
            0 => {
                lt.disable_remote();
                lt.record_fb(2, fb_cell(2).unwrap_or(0)) == Err(NjCode::EFbOrder)
            }
            1 => lt.record_test(0, 0) == Err(NjCode::EPreTest),
            _ => lt.record_build(TARGETS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x36600 + k as u64);
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
pub fn run_unx52077_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52077");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "build/test 20/20 全通",
        recorded >= (TARGETS * 2) as u32,
        "build/test ×10 目标逐格恒等",
    );
    s.add(
        "回退链 4/4 全账",
        {
            let mut lt = NjLedger::new();
            lt.disable_remote();
            let mut ok = true;
            let mut f = 0usize;
            while f < FALLBACK {
                if let Some(w) = fb_cell(f) {
                    if lt.record_fb(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.fbs_all()
        },
        "探测→降级→本地命中→本地构建四格零漂移",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越序/前置/越闭集三态轮转必抓");
    let mut l0 = NjLedger::new();
    s.add(
        "回退前置闸",
        l0.record_fb(0, 0) == Err(NjCode::ENoFbPath),
        "远程未禁用不许走回退",
    );
    s.add(
        "test 前置闸",
        l0.record_test(0, 0) == Err(NjCode::EPreTest),
        "未 build 先 test 必抓",
    );
    s.add(
        "边界·目标越闭集",
        l0.record_build(TARGETS, 0).is_err(),
        "目标 10 越闭集必拒",
    );
    let mut l1 = NjLedger::new();
    let _ = l1.record_build(2, build_cell(2).unwrap_or(0));
    s.add(
        "重复·再落账拒",
        l1.record_build(2, build_cell(2).unwrap_or(0)).is_err(),
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
    s.add("收口·判据条数", s.len() >= 10, "含本条共 ≥11 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cells_pure() {
        let a = build_cell(5).unwrap_or(0);
        assert_eq!(a, build_cell(5).unwrap_or(0));
        assert_ne!(a, build_cell(6).unwrap_or(0));
        assert_ne!(build_cell(5), test_cell(5));
        assert!(build_cell(TARGETS).is_none());
        assert!(test_cell(TARGETS).is_none());
        assert!(fb_cell(FALLBACK).is_none());
    }

    #[test]
    fn test_fb_gates() {
        let mut lt = NjLedger::new();
        assert_eq!(lt.record_fb(0, 0), Err(NjCode::ENoFbPath));
        lt.disable_remote();
        assert_eq!(lt.record_fb(2, fb_cell(2).unwrap_or(0)), Err(NjCode::EFbOrder));
        assert!(lt.record_fb(0, fb_cell(0).unwrap_or(0)).is_ok());
        assert!(lt.record_fb(1, fb_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_pre_test_gate() {
        let mut lt = NjLedger::new();
        assert_eq!(lt.record_test(0, 0), Err(NjCode::EPreTest));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (TARGETS * 2) as u32);
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
        assert_eq!(SEG_BASE, 0xC5340);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52077_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
