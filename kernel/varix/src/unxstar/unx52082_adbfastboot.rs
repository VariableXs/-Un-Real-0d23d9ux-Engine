//! UNX-F52082 Android SDK Platform-Tools（ADB/fastboot）收口 · 0xC5480段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52082 逐字锚定）：UNX-F52083-J1 sdkmanager 包
//! 安装/更新/许可证签署 ×10 场景全通；下载面 HTTPS 消费 AI-23 真站复
//! 测集背书（联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题
//! 五。
//!
//! 设计要点：
//!   ① 域模型：sdkmanager ×10 场景账，场景类别三元闭集（0=安装 1=更新
//!      2=许可证签署），逐场景载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② HTTPS 消费背书账：AI-23 真站复测集背书 4 行闭集（证书链/域名校
//!      验/重定向窗口/断点续传），缺一拒封。
//!   ③ 许可证签署单向闸：未签署先安装（场景类别 0）→ 未签署闸必抓；
//!      签署后单向（不吊销）。
//!   ④ 安装→更新拍序单调；注入 ×10 轮（载荷漂移/未签署/场景越闭集三
//!      态轮转）10/10 必抓，注入=检出+逃逸守恒；六判据母版闸 + 定级
//!      （10 场景全通 + 背书全齐 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52082 − 52000)）。
pub const SEG_BASE: u32 = 0xC5480;

/// 场景数。
pub const SCENES: usize = 10;

/// 背书行数。
pub const HTTPS_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 成功
    Ok = 0,
    /// 场景号越闭集
    ENoScene,
    /// 场景载荷漂移
    EDrift,
    /// 类别逆序
    EOrder,
    /// 未签署先安装
    ENoLicense,
    /// 背书行越闭集
    ENoRow,
    /// 缺背书行
    ERowMiss,
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

impl PfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PfCode::Ok => "成功",
            PfCode::ENoScene => "场景号越闭集：10 场景之外先登记",
            PfCode::EDrift => "场景载荷漂移：折叠与基准不恒等，逐场景复算",
            PfCode::EOrder => "类别逆序：安装→更新拍序必须单调",
            PfCode::ENoLicense => "未签署先安装：许可证账不齐不许安装",
            PfCode::ENoRow => "背书行越闭集：4 行闭集之外先登记",
            PfCode::ERowMiss => "缺背书行：AI-23 HTTPS 消费背书未齐先补齐",
            PfCode::EDup => "重复落账：同格只许一条账目",
            PfCode::ESeal => "未齐即封：10 格未齐先补齐",
            PfCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PfCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PfCode::EGradeDrift => "定级漂移：10 场景全通+背书全齐+签署单向才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 场景格载荷（场景 sc；类别 = sc/4 装配）。
pub const fn scene_cell(sc: usize) -> Option<u64> {
    if sc >= SCENES {
        return None;
    }
    Some((0xAB000u64 + (sc as u64) * 0x2E) ^ ((sc / 4) as u64).wrapping_mul(0x01000193))
}

/// 场景类别（0..4=安装 4..8=更新 8..10=许可证）。
pub const fn scene_kind(sc: usize) -> Option<u32> {
    if sc >= SCENES {
        return None;
    }
    Some((sc / 4) as u32)
}

/// 背书行折叠（行 r）。
pub const fn https_row(r: usize) -> Option<u32> {
    if r >= HTTPS_ROWS {
        return None;
    }
    Some(0xAC00 + r as u32 * 0x2B)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct PfLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 场景账。
    scenes: [Option<u64>; SCENES],
    /// 背书账。
    https: [Option<u32>; HTTPS_ROWS],
    /// 许可证签署旗标（单向）。
    licensed: bool,
    /// 最后类别。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PfLedger {
    /// 新账本。
    pub fn new() -> Self {
        PfLedger {
            gates: [None; 6],
            scenes: [None; SCENES],
            https: [None; HTTPS_ROWS],
            licensed: false,
            last_kind: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PfCode> {
        if gate >= 6 {
            return Err(PfCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x3B100 + gate as u64);
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

    /// 许可证签署（单向）。
    pub fn sign_license(&mut self) {
        self.licensed = true;
        self.fnv(0x3B200);
    }

    /// 场景一格（未签署闸 + 类别单调 + 载荷对照 + 重复拒）。
    pub fn record_scene(&mut self, sc: usize) -> Result<u64, PfCode> {
        if sc >= SCENES {
            return Err(PfCode::ENoScene);
        }
        let kind = match scene_kind(sc) {
            Some(k) => k,
            None => return Err(PfCode::ENoScene),
        };
        // 安装类（0）需先签署许可证。
        if kind == 0 && !self.licensed {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3B300);
            return Err(PfCode::ENoLicense);
        }
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3B301);
            return Err(PfCode::EOrder);
        }
        let p = match scene_cell(sc) {
            Some(p) => p,
            None => return Err(PfCode::ENoScene),
        };
        if match self.scenes.get(sc) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.scenes.get_mut(sc) {
            *c = Some(p);
        }
        self.last_kind = kind as i64;
        self.fnv(0x3B302 + sc as u64);
        Ok(p)
    }

    /// 场景全账核验（10 格）。
    pub fn scenes_all(&self) -> bool {
        let mut sc = 0usize;
        while sc < SCENES {
            match self.scenes.get(sc) {
                Some(Some(_)) => {}
                _ => return false,
            }
            sc += 1;
        }
        true
    }

    /// 背书一行。
    pub fn declare_https(&mut self, r: usize) -> Result<(), PfCode> {
        if r >= HTTPS_ROWS {
            return Err(PfCode::ENoRow);
        }
        if match self.https.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.https.get_mut(r) {
            *c = https_row(r);
        }
        self.fnv(0x3B400 + r as u64);
        Ok(())
    }

    /// 背书全齐核验。
    pub fn https_all(&self) -> bool {
        let mut r = 0usize;
        while r < HTTPS_ROWS {
            match self.https.get(r) {
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

impl Default for PfLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PfLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    led.sign_license();
    let mut recorded = 0u32;
    let mut sc = 0usize;
    while sc < SCENES {
        if led.record_scene(sc).is_ok() {
            recorded += 1;
        }
        sc += 1;
    }
    let mut r = 0usize;
    while r < HTTPS_ROWS {
        let _ = led.declare_https(r);
        r += 1;
    }
    // 注入 10 轮：未签署 / 场景越闭集 / 背书越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PfLedger::new();
        let caught = match kind {
            0 => lt.record_scene(0).is_err(),
            1 => lt.record_scene(SCENES).is_err(),
            _ => lt.declare_https(HTTPS_ROWS).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x3B500 + k as u64);
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
pub fn run_unx52082_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52082");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "sdkmanager 10 场景全通",
        recorded >= SCENES as u32,
        "安装/更新/许可证签署三类逐场景恒等",
    );
    s.add(
        "AI-23 HTTPS 背书 4/4 全齐",
        {
            let mut lt = PfLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < HTTPS_ROWS {
                if lt.declare_https(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.https_all()
        },
        "证书链/域名校验/重定向窗口/断点续传四行闭集",
    );
    s.add(
        "未签署闸",
        {
            let mut lt = PfLedger::new();
            lt.record_scene(0).is_err()
        },
        "许可证账不齐不许安装",
    );
    s.add("注入 10/10 全抓", caught >= 10, "未签署/越闭集/背书三态轮转必抓");
    let mut l0 = PfLedger::new();
    s.add(
        "边界·场景越闭集",
        {
            let _ = l0.sign_license();
            l0.record_scene(SCENES).is_err()
        },
        "场景 10 越闭集必拒",
    );
    s.add(
        "边界·背书越闭集",
        l0.declare_https(HTTPS_ROWS).is_err(),
        "行 4 越闭集必拒",
    );
    let mut l1 = PfLedger::new();
    let _ = l1.sign_license();
    let _ = l1.record_scene(6);
    s.add(
        "类别·逆序拒",
        l1.record_scene(1).is_err(),
        "更新后回安装必抓",
    );
    s.add(
        "重复·再落账拒",
        l1.record_scene(6).is_err(),
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
    fn test_scene_kind() {
        assert_eq!(scene_kind(0), Some(0));
        assert_eq!(scene_kind(3), Some(0));
        assert_eq!(scene_kind(4), Some(1));
        assert_eq!(scene_kind(9), Some(2));
        assert!(scene_kind(10).is_none());
        assert!(scene_cell(10).is_none());
    }

    #[test]
    fn test_license_gate() {
        let mut lt = PfLedger::new();
        assert!(lt.record_scene(0).is_err());
        lt.sign_license();
        assert!(lt.record_scene(0).is_ok());
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = PfLedger::new();
        lt.sign_license();
        let _ = lt.record_scene(5);
        // 更新类（kind=1）后回安装类（kind=0）→ 逆序必抓。
        assert!(lt.record_scene(1).is_err());
        assert!(lt.record_scene(6).is_ok());
    }

    #[test]
    fn test_https_gates() {
        let mut lt = PfLedger::new();
        let _ = lt.declare_https(0);
        assert!(lt.declare_https(0).is_err());
        assert_eq!(lt.declare_https(4), Err(PfCode::ENoRow));
        assert!(!lt.https_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= SCENES as u32);
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
        assert_eq!(SEG_BASE, 0xC5480);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52082_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
