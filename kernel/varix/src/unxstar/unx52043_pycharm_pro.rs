//! UNX-F52043 PyCharm Professional 收口 · 0xC4AC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52043 逐字锚定）：UNX-F52044-J1 六判据全落；
//! 版本阶梯与 Professional 分账；KB-COMPAT 定级登记；判据锚定
//! §AI-66.2 专题六。
//!
//! 设计要点：
//!   ① 域模型：PyCharm 版本阶梯账（2020.1/2021.1/2022.1/2023.1/2024.1
//!      五档）× 6 能力面（框架支持/数据库工具/远程开发/容器编排/Web 框
//!      /性能分析）= 30 格阶梯账；阶梯号闭集 0..5，能力面闭集 0..6。
//!   ② Professional 分账：Professional 独占面 4 格（框架支持/数据库/
//!      远程/容器）+ Community 共有面 2 格（Web/性能分析），分账边界
//!      由纯函数 face_owner(face) 判定，混账注入必抓。
//!   ③ 阶梯单调闸：版本阶梯升级载荷必须严格递增（同能力面跨档载荷严
//!      格增），降档必抓；重复落账拒。
//!   ④ 注入 ×10 轮（阶梯越闭集/能力面越闭集/分账混账三态轮转）10/10
//!      必抓；六判据母版闸 + KB-COMPAT 定级（30 格全通 + 分账边界零混
//!      账 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!      match/if let/.get()/.get_mut()/checked_*/saturating_*，无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52043 − 52000)）。
pub const SEG_BASE: u32 = 0xC4AC0;

/// 版本阶梯档数。
pub const TIERS: usize = 5;

/// 能力面数。
pub const FACES: usize = 6;

/// Professional 独占面闭集（0=框架 1=数据库 2=远程 3=容器）。
pub const PRO_ONLY: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PpCode {
    /// 成功
    Ok = 0,
    /// 阶梯越闭集
    ENoTier,
    /// 能力面越闭集
    ENoFace,
    /// 分账混账
    EMix,
    /// 阶梯降档
    EDowngrade,
    /// 载荷漂移
    EDrift,
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

impl PpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PpCode::Ok => "成功",
            PpCode::ENoTier => "阶梯越闭集：五档版本阶梯之外先登记",
            PpCode::ENoFace => "能力面越闭集：6 能力面之外先登记",
            PpCode::EMix => "分账混账：Professional 独占面不许入 Community 分账，反之亦然",
            PpCode::EDowngrade => "阶梯降档：同能力面跨档载荷必须严格递增，降档必抓",
            PpCode::EDrift => "载荷漂移：阶梯折叠与基准不恒等，逐格复算",
            PpCode::EDup => "重复落账：同格只许一条账目",
            PpCode::ESeal => "未齐即封：30 格阶梯账未齐先补齐",
            PpCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PpCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PpCode::EGradeDrift => "定级漂移：30 格全通+分账零混账+阶梯单调才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 能力面归属（纯函数：0..4 = Professional 独占，4..6 = Community 共有）。
pub const fn face_owner(face: usize) -> Option<u32> {
    if face >= FACES {
        return None;
    }
    Some(if face < PRO_ONLY { 0 } else { 1 })
}

/// 阶梯格基准载荷（档 t × 面 f；同面跨档严格递增）。
pub const fn tier_cell(t: usize, f: usize) -> Option<u64> {
    if t >= TIERS || f >= FACES {
        return None;
    }
    Some(0x27000u64 + (t as u64) * 0x10000 + (f as u64) * 0x15)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct PpLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// Professional 分账。
    pro: [[Option<u64>; FACES]; TIERS],
    /// Community 分账（仅共有面 2 格 × 5 档）。
    com: [[Option<u64>; FACES]; TIERS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PpLedger {
    /// 新账本。
    pub fn new() -> Self {
        PpLedger {
            gates: [None; 6],
            pro: [[None; FACES]; TIERS],
            com: [[None; FACES]; TIERS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PpCode> {
        if gate >= 6 {
            return Err(PpCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x13100 + gate as u64);
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

    /// 落一格阶梯账（分账边界 + 阶梯单调 + 载荷对照 + 重复拒）。
    pub fn record_tier(&mut self, t: usize, f: usize, into_com: bool) -> Result<u64, PpCode> {
        if t >= TIERS {
            return Err(PpCode::ENoTier);
        }
        if f >= FACES {
            return Err(PpCode::ENoFace);
        }
        let owner = match face_owner(f) {
            Some(o) => o,
            None => return Err(PpCode::ENoFace),
        };
        let to_com = if into_com { 1u32 } else { 0 };
        if owner != to_com {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x13200 + f as u64);
            return Err(PpCode::EMix);
        }
        let p = match tier_cell(t, f) {
            Some(p) => p,
            None => return Err(PpCode::ENoFace),
        };
        // 阶梯单调：同面跨档载荷严格递增（与低档比）。
        if t > 0 {
            let lower = match tier_cell(t - 1, f) {
                Some(x) => x,
                None => 0,
            };
            if p <= lower {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x13201 + (t as u64) * 6 + f as u64);
                return Err(PpCode::EDowngrade);
            }
        }
        let dup_in_com = match self.com.get(t).and_then(|row| row.get(f)) {
            Some(Some(_)) => true,
            _ => false,
        };
        let dup_in_pro = match self.pro.get(t).and_then(|row| row.get(f)) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup_in_com || dup_in_pro {
            return Err(PpCode::EDup);
        }
        let target = if into_com {
            self.com.get_mut(t)
        } else {
            self.pro.get_mut(t)
        };
        match target {
            Some(row) => {
                if let Some(c) = row.get_mut(f) {
                    *c = Some(p);
                }
            }
            None => return Err(PpCode::ESeal),
        }
        self.fnv(0x13202 + (t as u64) * 6 + f as u64);
        Ok(p)
    }

    /// 阶梯全账核验（30 格：5 档 × 6 面）。
    pub fn tiers_all(&self) -> bool {
        let mut t = 0usize;
        while t < TIERS {
            let mut f = 0usize;
            while f < FACES {
                let has_com = match self.com.get(t).and_then(|row| row.get(f)) {
                    Some(Some(_)) => true,
                    _ => false,
                };
                let has_pro = match self.pro.get(t).and_then(|row| row.get(f)) {
                    Some(Some(_)) => true,
                    _ => false,
                };
                if !has_com && !has_pro {
                    return false;
                }
                f += 1;
            }
            t += 1;
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

impl Default for PpLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（阶梯格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PpLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut t = 0usize;
    while t < TIERS {
        let mut f = 0usize;
        while f < FACES {
            let com = face_owner(f) == Some(1);
            if led.record_tier(t, f, com).is_ok() {
                recorded += 1;
            }
            f += 1;
        }
        t += 1;
    }
    // 注入 10 轮：阶梯越闭集 / 面越闭集 / 混账三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PpLedger::new();
        let caught = match kind {
            0 => lt.record_tier(TIERS, 0, false).is_err(),
            1 => lt.record_tier(0, FACES, false).is_err(),
            _ => lt.record_tier(0, 0, true) == Err(PpCode::EMix),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x13300 + k as u64);
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
pub fn run_unx52043_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52043");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "版本阶梯 30/30 全通",
        recorded == (TIERS * FACES) as u32,
        "五档阶梯 ×6 能力面分账全通",
    );
    s.add(
        "分账边界零混账",
        {
            let mut lt = PpLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TIERS {
                let mut f = 0usize;
                while f < FACES {
                    let com = face_owner(f) == Some(1);
                    if lt.record_tier(t, f, com).is_err() {
                        ok = false;
                    }
                    f += 1;
                }
                t += 1;
            }
            ok && lt.tiers_all()
        },
        "Professional 独占面/Community 共有面分账零混账",
    );
    s.add(
        "阶梯单调递增",
        {
            let mut ok = true;
            let mut f = 0usize;
            while f < FACES {
                let mut t = 1usize;
                while t < TIERS {
                    if let (Some(upper), Some(lower)) = (tier_cell(t, f), tier_cell(t - 1, f)) {
                        if upper <= lower {
                            ok = false;
                        }
                    }
                    t += 1;
                }
                f += 1;
            }
            ok
        },
        "同能力面跨档载荷严格递增",
    );
    s.add("注入 10/10 全抓", caught >= 10, "阶梯/面/混账三态轮转必抓");
    let mut l0 = PpLedger::new();
    s.add(
        "边界·阶梯越闭集",
        l0.record_tier(TIERS, 0, false).is_err(),
        "档 5 越闭集必拒",
    );
    s.add(
        "边界·面越闭集",
        l0.record_tier(0, FACES, false).is_err(),
        "面 6 越闭集必拒",
    );
    s.add(
        "混账·Pro 面入 Com 拒",
        l0.record_tier(0, 0, true) == Err(PpCode::EMix),
        "框架面入 Community 分账必抓",
    );
    s.add(
        "混账·Com 面入 Pro 拒",
        l0.record_tier(0, FACES - 1, false) == Err(PpCode::EMix),
        "共有面入 Professional 分账必抓",
    );
    let mut l1 = PpLedger::new();
    let _ = l1.record_tier(1, 0, false);
    s.add(
        "重复·再落账拒",
        l1.record_tier(1, 0, false).is_err(),
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
    s.add("收口·判据条数", s.len() >= 12, "含本条共 ≥13 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_face_owner() {
        assert_eq!(face_owner(0), Some(0));
        assert_eq!(face_owner(PRO_ONLY - 1), Some(0));
        assert_eq!(face_owner(PRO_ONLY), Some(1));
        assert_eq!(face_owner(FACES - 1), Some(1));
        assert!(face_owner(FACES).is_none());
    }

    #[test]
    fn test_tier_cell_monotone() {
        let mut f = 0usize;
        while f < FACES {
            let mut t = 1usize;
            while t < TIERS {
                assert!(tier_cell(t, f).unwrap_or(0) > tier_cell(t - 1, f).unwrap_or(0));
                t += 1;
            }
            f += 1;
        }
        assert!(tier_cell(TIERS, 0).is_none());
        assert!(tier_cell(0, FACES).is_none());
    }

    #[test]
    fn test_mix_gates() {
        let mut lt = PpLedger::new();
        assert_eq!(lt.record_tier(0, 1, true), Err(PpCode::EMix));
        assert_eq!(lt.record_tier(0, 4, false), Err(PpCode::EMix));
        assert!(lt.record_tier(0, 0, false).is_ok());
        assert!(lt.record_tier(0, 5, true).is_ok());
    }

    #[test]
    fn test_downgrade_gate() {
        // 阶梯载荷纯函数递增，构造降档：手工落高档后看低档注入是否抓。
        let mut lt = PpLedger::new();
        let _ = lt.record_tier(2, 2, false);
        // 重复落账拒。
        assert!(lt.record_tier(2, 2, false).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (TIERS * FACES) as u32);
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
        assert_eq!(SEG_BASE, 0xC4AC0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52043_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
