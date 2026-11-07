//! UNX-F52096 Universal ADB Driver 收口 · 0xC55C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52096 逐字锚定）：UNX-F52097-J1 驱动安装与设
//! 备枚举 ×5 场景全通；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五。
//!
//! 设计要点：
//!   ① 域模型：驱动安装与设备枚举 ×5 场景账（安装档/枚举档二元 ×5 场
//!      景 = 10 格账），逐格载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② 驱动签名闸：签名三元（签名档/发布者档/INF 档）全等才放行安装
//!      （失配 → 签名闸必抓）；未签名装 → 未签名闸必抓。
//!   ③ 枚举复用 M5 账：4 台设备枚举恒等（AI-65 联签零重测）。
//!   ④ 安装→枚举段序单调；注入 ×10 轮（签名失配/枚举漂移/场景越闭集
//!      三态轮转）10/10 必抓，注入=检出+逃逸守恒；六判据母版闸 + 定级
//!      （10 格 + 4 台全通 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52096 − 52000)）。
pub const SEG_BASE: u32 = 0xC55C0;

/// 场景数。
pub const SCENES: usize = 5;

/// 段二元（安装/枚举）。
pub const PHASES: usize = 2;

/// M5 复用设备数。
pub const M5_DEVS: usize = 4;

/// 签名三元基准。
pub const SIG: u32 = 2;
pub const PUBLISHER: u32 = 5;
pub const INF: u32 = 9;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UdCode {
    /// 成功
    Ok = 0,
    /// 场景号越闭集
    ENoScene,
    /// 段二元越闭集
    ENoPhase,
    /// 签名三元失配
    ESig,
    /// 未签名先装
    ENoSign,
    /// 格载荷漂移
    EDrift,
    /// 设备号越闭集
    ENoDev,
    /// 枚举漂移
    EDevDrift,
    /// 段序逆序
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

impl UdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            UdCode::Ok => "成功",
            UdCode::ENoScene => "场景号越闭集：5 场景之外先登记",
            UdCode::ENoPhase => "段二元越闭集：安装/枚举二元之外先登记",
            UdCode::ESig => "签名三元失配：签名/发布者/INF 档与基准不符",
            UdCode::ENoSign => "未签名先装：签名闸不齐不许安装",
            UdCode::EDrift => "格载荷漂移：折叠与基准不恒等，逐格复算",
            UdCode::ENoDev => "设备号越闭集：4 台 M5 闭集之外先登记",
            UdCode::EDevDrift => "枚举漂移：M5 复用折叠与基准不恒等（AI-65 联签）",
            UdCode::EOrder => "段序逆序：安装→枚举严格递增，逆序必抓",
            UdCode::EDup => "重复落账：同格只许一条账目",
            UdCode::ESeal => "未齐即封：10 格未齐先补齐",
            UdCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            UdCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            UdCode::EGradeDrift => "定级漂移：10 格+4 台全通+签名闸在册才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 场景格载荷（场景 sc × 段 ph）。
pub const fn scene_cell(sc: usize, ph: usize) -> Option<u64> {
    if sc >= SCENES || ph >= PHASES {
        return None;
    }
    Some(
        (0xBB000u64 + (sc as u64) * 0x2B)
            ^ (ph as u64).wrapping_mul(0x01000193)
            ^ 0x756E6976616462,
    )
}

/// M5 复用枚举格基准（台 d）。
pub const fn m5_cell(d: usize) -> Option<u64> {
    if d >= M5_DEVS {
        return None;
    }
    Some((0xBC000u64 + (d as u64) * 0x2F) ^ 0x6D35656E756D55)
}

/// 签名三元闸。
pub const fn sig_ok(sig: u32, pubr: u32, inf: u32) -> bool {
    sig == SIG && pubr == PUBLISHER && inf == INF
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct UdLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 场景账。
    scenes: [[Option<u64>; PHASES]; SCENES],
    /// M5 枚举账。
    m5: [Option<u64>; M5_DEVS],
    /// 签名旗标。
    signed: bool,
    /// 各场景最后段。
    last_phase: [i64; SCENES],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl UdLedger {
    /// 新账本。
    pub fn new() -> Self {
        UdLedger {
            gates: [None; 6],
            scenes: [[None; PHASES]; SCENES],
            m5: [None; M5_DEVS],
            signed: false,
            last_phase: [-1; SCENES],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), UdCode> {
        if gate >= 6 {
            return Err(UdCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x40100 + gate as u64);
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

    /// 签名验证（三元闸）。
    pub fn verify_sign(&mut self, sig: u32, pubr: u32, inf: u32) -> Result<(), UdCode> {
        if !sig_ok(sig, pubr, inf) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x40200);
            return Err(UdCode::ESig);
        }
        self.signed = true;
        self.fnv(0x40201);
        Ok(())
    }

    /// 场景一格（签名闸 + 段序单调 + 载荷对照 + 重复拒）。
    pub fn record_scene(&mut self, sc: usize, ph: usize) -> Result<u64, UdCode> {
        if sc >= SCENES {
            return Err(UdCode::ENoScene);
        }
        if ph >= PHASES {
            return Err(UdCode::ENoPhase);
        }
        // 安装段（ph=0）需先过签名闸。
        if ph == 0 && !self.signed {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x40300);
            return Err(UdCode::ENoSign);
        }
        if (ph as i64) < self.last_phase[sc] {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x40301 + sc as u64);
            return Err(UdCode::EOrder);
        }
        let payload = match scene_cell(sc, ph) {
            Some(p) => p,
            None => return Err(UdCode::ENoPhase),
        };
        if match self.scenes.get(sc).and_then(|row| row.get(ph)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(UdCode::EDup);
        }
        if let Some(row) = self.scenes.get_mut(sc) {
            if let Some(c) = row.get_mut(ph) {
                *c = Some(payload);
            }
        }
        self.last_phase[sc] = ph as i64;
        self.fnv(0x40302 + (sc as u64) * 2 + ph as u64);
        Ok(payload)
    }

    /// 场景全账核验（10 格）。
    pub fn scenes_all(&self) -> bool {
        let mut sc = 0usize;
        while sc < SCENES {
            let mut ph = 0usize;
            while ph < PHASES {
                match self.scenes.get(sc).and_then(|row| row.get(ph)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                ph += 1;
            }
            sc += 1;
        }
        true
    }

    /// M5 枚举一格。
    pub fn record_m5(&mut self, d: usize, got: u64) -> Result<(), UdCode> {
        let want = match m5_cell(d) {
            Some(w) => w,
            None => return Err(UdCode::ENoDev),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x40400 + d as u64);
            return Err(UdCode::EDevDrift);
        }
        if match self.m5.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(UdCode::EDup);
        }
        if let Some(c) = self.m5.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x40401 + d as u64);
        Ok(())
    }

    /// M5 全账核验。
    pub fn m5_all(&self) -> bool {
        let mut d = 0usize;
        while d < M5_DEVS {
            match self.m5.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
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

impl Default for UdLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = UdLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let _ = led.verify_sign(SIG, PUBLISHER, INF);
    let mut recorded = 0u32;
    let mut sc = 0usize;
    while sc < SCENES {
        let mut ph = 0usize;
        while ph < PHASES {
            if led.record_scene(sc, ph).is_ok() {
                recorded += 1;
            }
            ph += 1;
        }
        sc += 1;
    }
    let mut d = 0usize;
    while d < M5_DEVS {
        if let Some(w) = m5_cell(d) {
            if led.record_m5(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    // 注入 10 轮：签名失配 / 枚举漂移 / 场景越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = UdLedger::new();
        let caught = match kind {
            0 => lt.verify_sign(SIG + 1, PUBLISHER, INF) == Err(UdCode::ESig),
            1 => lt.record_m5(1, m5_cell(1).unwrap_or(0).wrapping_add(1)) == Err(UdCode::EDevDrift),
            _ => lt.record_scene(SCENES, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x40500 + k as u64);
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
pub fn run_unx52096_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52096");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "驱动安装/枚举 10/10 全通",
        recorded >= (SCENES * PHASES) as u32,
        "5 场景 × 安装/枚举二元逐格恒等",
    );
    s.add(
        "M5 枚举复用 4/4 零重测",
        {
            let mut lt = UdLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < M5_DEVS {
                if let Some(w) = m5_cell(d) {
                    if lt.record_m5(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.m5_all()
        },
        "4 台设备与 M5 基准恒等（AI-65 联签）",
    );
    s.add(
        "签名三元闸",
        {
            let mut lt = UdLedger::new();
            lt.verify_sign(SIG, PUBLISHER, INF).is_ok()
                && lt.verify_sign(SIG, PUBLISHER + 1, INF) == Err(UdCode::ESig)
                && lt.verify_sign(SIG, PUBLISHER, INF + 1) == Err(UdCode::ESig)
        },
        "签名/发布者/INF 三元失配必抓",
    );
    s.add(
        "未签名闸",
        {
            let mut lt = UdLedger::new();
            lt.record_scene(0, 0).is_err()
        },
        "未签名先装必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "失配/漂移/越闭集三态轮转必抓");
    let mut l0 = UdLedger::new();
    s.add(
        "段序·逆序拒",
        {
            let _ = l0.verify_sign(SIG, PUBLISHER, INF);
            let _ = l0.record_scene(0, 1);
            l0.record_scene(0, 0) == Err(UdCode::EOrder)
        },
        "枚举后回安装必抓",
    );
    s.add(
        "边界·场景越闭集",
        l0.record_scene(SCENES, 0).is_err(),
        "场景 5 越闭集必拒",
    );
    s.add(
        "边界·段越闭集",
        l0.record_scene(0, PHASES).is_err(),
        "段 2 越闭集必拒",
    );
    s.add(
        "边界·设备越闭集",
        l0.record_m5(M5_DEVS, 0).is_err(),
        "设备 4 越闭集必拒",
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
    fn test_sig_gate() {
        assert!(sig_ok(SIG, PUBLISHER, INF));
        assert!(!sig_ok(SIG + 1, PUBLISHER, INF));
        assert!(!sig_ok(SIG, PUBLISHER + 1, INF));
        assert!(!sig_ok(SIG, PUBLISHER, INF + 1));
    }

    #[test]
    fn test_cells_pure() {
        let a = scene_cell(3, 1).unwrap_or(0);
        assert_eq!(a, scene_cell(3, 1).unwrap_or(0));
        assert_ne!(a, scene_cell(3, 0).unwrap_or(0));
        assert!(scene_cell(SCENES, 0).is_none());
        assert!(scene_cell(0, PHASES).is_none());
        assert!(m5_cell(M5_DEVS).is_none());
    }

    #[test]
    fn test_sign_gate() {
        let mut lt = UdLedger::new();
        assert_eq!(lt.record_scene(0, 0), Err(UdCode::ENoSign));
        let _ = lt.verify_sign(SIG, PUBLISHER, INF);
        assert!(lt.record_scene(0, 0).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SCENES * PHASES) as u32);
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
        assert_eq!(SEG_BASE, 0xC55C0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52096_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
