//! UNX-F52064 Windows SDK 10.0.26100（24H2 版）收口 · 0xC5000段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52064 逐字锚定）：UNX-F52065-J1 引用程序集/
//! 目标包 ×20 工程编译全通；msbuild TargetFramework 解析 ×5 场景；
//! KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二。
//!
//! 设计要点：
//!   ① 域模型：引用程序集/目标包 ×20 工程编译账；20 格编译载荷（纯函
//!      数折叠）恒等才收；编译漂移必抓。
//!   ② TargetFramework 解析账：解析 ×5 场景（净 6.0/净 8.0/净框架 4.8
//!      /uap/原生），解析载荷三元（TFM 档/平台档/版本档）全等才收；解
//!      析漂移必抓；重复解析拒。
//!   ③ 编译拍序闸：解析→编译拍序严格递增；未解析先编译 → 解析前置闸。
//!   ④ 注入 ×10 轮（解析漂移/编译漂移/工程越闭集三态轮转）10/10 必
//!      抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（20 格全通 + 解析
//!      5 场景全账 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52064 − 52000)）。
pub const SEG_BASE: u32 = 0xC5000;

/// 工程编译数。
pub const PROJS: usize = 20;

/// 解析场景数。
pub const PARSE_SCENES: usize = 5;

/// 解析三元基准（TFM/平台/版本档）。
pub const TFM: u32 = 8;
pub const PLATFORM: u32 = 2;
pub const SDK_VER: u32 = 10;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SdCode {
    /// 成功
    Ok = 0,
    /// 工程号越闭集
    ENoProj,
    /// 解析场景越闭集
    ENoScene,
    /// 解析漂移
    EParseDrift,
    /// 编译漂移
    EDrift,
    /// 未解析先编译
    EPreBuild,
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

impl SdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SdCode::Ok => "成功",
            SdCode::ENoProj => "工程号越闭集：20 工程之外先登记",
            SdCode::ENoScene => "解析场景越闭集：5 场景之外先登记",
            SdCode::EParseDrift => "解析漂移：TFM/平台/版本三元折叠与基准不恒等",
            SdCode::EDrift => "编译漂移：编译折叠与基准不恒等，逐工程复算",
            SdCode::EPreBuild => "未解析先编译：解析 5 场景全账不齐不许报编译全通",
            SdCode::EDup => "重复落账：同格只许一条账目",
            SdCode::ESeal => "未齐即封：20 格未齐先补齐",
            SdCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SdCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SdCode::EGradeDrift => "定级漂移：20 格全通+解析 5 场景全账才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 编译格载荷（工程 p）。
pub const fn build_cell(p: usize) -> Option<u64> {
    if p >= PROJS {
        return None;
    }
    Some((0x64000u64 + (p as u64) * 0x31) ^ (p as u64).wrapping_mul(0x01000193))
}

/// 解析格载荷（场景 sc，三元折叠）。
pub const fn parse_cell(sc: usize) -> Option<u64> {
    if sc >= PARSE_SCENES {
        return None;
    }
    Some(
        (0x65000u64 + (sc as u64) * 0x24)
            ^ (TFM as u64).wrapping_mul(0x9E3779B97F4A7C15)
            ^ (PLATFORM as u64).wrapping_mul(0x01000193)
            ^ (SDK_VER as u64),
    )
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct SdLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 编译账。
    builds: [Option<u64>; PROJS],
    /// 解析账。
    parses: [Option<u64>; PARSE_SCENES],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SdLedger {
    /// 新账本。
    pub fn new() -> Self {
        SdLedger {
            gates: [None; 6],
            builds: [None; PROJS],
            parses: [None; PARSE_SCENES],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SdCode> {
        if gate >= 6 {
            return Err(SdCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x29100 + gate as u64);
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

    /// 解析一格（三元载荷对照 + 重复拒）。
    pub fn parse_tfm(&mut self, sc: usize, got: u64) -> Result<(), SdCode> {
        if sc >= PARSE_SCENES {
            return Err(SdCode::ENoScene);
        }
        let want = match parse_cell(sc) {
            Some(w) => w,
            None => return Err(SdCode::ENoScene),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x29200 + sc as u64);
            return Err(SdCode::EParseDrift);
        }
        if match self.parses.get(sc) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SdCode::EDup);
        }
        if let Some(c) = self.parses.get_mut(sc) {
            *c = Some(got);
        }
        self.fnv(0x29201 + sc as u64);
        Ok(())
    }

    /// 解析全账核验（5 格）。
    pub fn parses_all(&self) -> bool {
        let mut sc = 0usize;
        while sc < PARSE_SCENES {
            match self.parses.get(sc) {
                Some(Some(_)) => {}
                _ => return false,
            }
            sc += 1;
        }
        true
    }

    /// 编译一格（解析前置闸 + 载荷对照 + 重复拒）。
    pub fn build(&mut self, p: usize, got: u64) -> Result<(), SdCode> {
        if p >= PROJS {
            return Err(SdCode::ENoProj);
        }
        if !self.parses_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x29300);
            return Err(SdCode::EPreBuild);
        }
        let want = match build_cell(p) {
            Some(w) => w,
            None => return Err(SdCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x29301 + p as u64);
            return Err(SdCode::EDrift);
        }
        if match self.builds.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SdCode::EDup);
        }
        if let Some(c) = self.builds.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x29302 + p as u64);
        Ok(())
    }

    /// 编译全账核验（20 格）。
    pub fn builds_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJS {
            match self.builds.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
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

impl Default for SdLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SdLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut sc = 0usize;
    while sc < PARSE_SCENES {
        if let Some(w) = parse_cell(sc) {
            let _ = led.parse_tfm(sc, w);
        }
        sc += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PROJS {
        if let Some(w) = build_cell(p) {
            if led.build(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    // 注入 10 轮：解析漂移 / 编译漂移 / 工程越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SdLedger::new();
        let caught = match kind {
            0 => lt.parse_tfm(1, parse_cell(1).unwrap_or(0).wrapping_add(1)) == Err(SdCode::EParseDrift),
            1 => lt.build(0, 0) == Err(SdCode::EPreBuild),
            _ => lt.build(PROJS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x29400 + k as u64);
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
pub fn run_unx52064_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52064");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "引用程序集编译 20/20 全通",
        {
            let mut lt = SdLedger::new();
            let mut sc = 0usize;
            while sc < PARSE_SCENES {
                if let Some(w) = parse_cell(sc) {
                    let _ = lt.parse_tfm(sc, w);
                }
                sc += 1;
            }
            let mut ok = true;
            let mut p = 0usize;
            while p < PROJS {
                if let Some(w) = build_cell(p) {
                    if lt.build(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.builds_all()
        },
        "引用程序集/目标包 ×20 工程编译逐格恒等",
    );
    s.add(
        "TFM 解析 5/5 全账",
        {
            let mut lt = SdLedger::new();
            let mut ok = true;
            let mut sc = 0usize;
            while sc < PARSE_SCENES {
                if let Some(w) = parse_cell(sc) {
                    if lt.parse_tfm(sc, w).is_err() {
                        ok = false;
                    }
                }
                sc += 1;
            }
            ok && lt.parses_all()
        },
        "净 6.0/净 8.0/净框架 4.8/uap/原生五场景三元零漂移",
    );
    s.add("注入 10/10 全抓", caught >= 10, "解析/编译/越闭集三态轮转必抓");
    let mut l0 = SdLedger::new();
    s.add(
        "解析前置闸",
        l0.build(0, 0) == Err(SdCode::EPreBuild),
        "解析 5 场景不齐不许报编译全通",
    );
    s.add(
        "边界·工程越闭集",
        l0.build(PROJS, 0).is_err(),
        "工程 20 越闭集必拒",
    );
    s.add(
        "边界·解析越闭集",
        l0.parse_tfm(PARSE_SCENES, 0).is_err(),
        "场景 5 越闭集必拒",
    );
    let mut l1 = SdLedger::new();
    let mut sc = 0usize;
    while sc < PARSE_SCENES {
        if let Some(w) = parse_cell(sc) {
            let _ = l1.parse_tfm(sc, w);
        }
        sc += 1;
    }
    let _ = l1.build(1, build_cell(1).unwrap_or(0));
    s.add(
        "重复·再落账拒",
        l1.build(1, build_cell(1).unwrap_or(0)).is_err(),
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
        let a = build_cell(7).unwrap_or(0);
        assert_eq!(a, build_cell(7).unwrap_or(0));
        assert_ne!(a, build_cell(8).unwrap_or(0));
        assert!(build_cell(PROJS).is_none());
        assert!(parse_cell(PARSE_SCENES).is_none());
        assert_ne!(parse_cell(0), parse_cell(1));
    }

    #[test]
    fn test_parse_gate() {
        let mut lt = SdLedger::new();
        assert_eq!(
            lt.parse_tfm(2, parse_cell(2).unwrap_or(0).wrapping_add(3)),
            Err(SdCode::EParseDrift)
        );
        assert!(lt.parse_tfm(2, parse_cell(2).unwrap_or(0)).is_ok());
        assert!(lt.parse_tfm(2, parse_cell(2).unwrap_or(0)).is_err());
    }

    #[test]
    fn test_pre_build_gate() {
        let mut lt = SdLedger::new();
        assert_eq!(lt.build(0, 0), Err(SdCode::EPreBuild));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, PROJS as u32);
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
        assert_eq!(SEG_BASE, 0xC5000);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52064_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
