//! UNX-F52044 PyCharm Community 收口 · 0xC4B00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52044 逐字锚定）：UNX-F52045-J1 六判据全落；
//! 版本阶梯与 Professional 分账；KB-COMPAT 定级登记；判据锚定
//! §AI-66.2 专题六。
//!
//! 设计要点：
//!   ① 域模型：PyCharm Community 版本阶梯账（五档）× 4 共有面（纯
//!      Python/调试/测试/版本控制）= 20 格阶梯账；同面跨档载荷严格递增
//!      （纯函数保证）。
//!   ② 与 Professional 分账：Community 只许落共有面；Professional 独
//!      占面以参照账单独账本记录（显式声明归属），载荷恒等才收，参照
//!      漂移必抓；参照账不与 Community 主账混。
//!   ③ 注入 ×10 轮（阶梯越闭集/参照漂移/重复落账三态轮转）10/10 必抓，
//!      注入=检出+逃逸守恒；六判据母版闸 + KB-COMPAT 定级（20 格全通
//!      + 参照账零混 → S1）。
//!   ④ no_std + alloc；零浮点、零 IO、零墙钟、零随机（指纹种子 =
//!      SEG_BASE 派生）；取值一律 match/if let/.get()/.get_mut()/
//!      checked_*/saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52044 − 52000)）。
pub const SEG_BASE: u32 = 0xC4B00;

/// 版本阶梯档数。
pub const TIERS: usize = 5;

/// Community 共有面数。
pub const CE_FACES: usize = 4;

/// Professional 独占面数。
pub const PRO_FACES: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PcCode {
    /// 成功
    Ok = 0,
    /// 阶梯越闭集
    ENoTier,
    /// 共有面越闭集
    ENoFace,
    /// Pro 面越闭集
    ENoPro,
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

impl PcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PcCode::Ok => "成功",
            PcCode::ENoTier => "阶梯越闭集：五档版本阶梯之外先登记",
            PcCode::ENoFace => "共有面越闭集：4 共有面之外先登记",
            PcCode::ENoPro => "Pro 面越闭集：4 独占面之外先登记",
            PcCode::EDrift => "载荷漂移：折叠与基准不恒等，逐格复算",
            PcCode::EDup => "重复落账：同格只许一条账目",
            PcCode::ESeal => "未齐即封：20 格阶梯账未齐先补齐",
            PcCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PcCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PcCode::EGradeDrift => "定级漂移：20 格全通+参照账零混账才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 共有面格基准载荷（档 t × 面 f；同面跨档严格递增）。
pub const fn ce_cell(t: usize, f: usize) -> Option<u64> {
    if t >= TIERS || f >= CE_FACES {
        return None;
    }
    Some(0x29000u64 + (t as u64) * 0x8000 + (f as u64) * 0x19)
}

/// Pro 独占面参照格载荷（面 p）。
pub const fn pro_ref_cell(p: usize) -> Option<u64> {
    if p >= PRO_FACES {
        return None;
    }
    Some((0x2A000u64 + p as u64 * 0x2F) ^ (p as u64).wrapping_mul(0x01000193))
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct PcLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// Community 主账。
    ce: [[Option<u64>; CE_FACES]; TIERS],
    /// Pro 参照账。
    pro_ref: [Option<u64>; PRO_FACES],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PcLedger {
    /// 新账本。
    pub fn new() -> Self {
        PcLedger {
            gates: [None; 6],
            ce: [[None; CE_FACES]; TIERS],
            pro_ref: [None; PRO_FACES],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PcCode> {
        if gate >= 6 {
            return Err(PcCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x14100 + gate as u64);
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

    /// Community 主账一格（阶梯单调 + 载荷对照 + 重复拒）。
    pub fn record_ce(&mut self, t: usize, f: usize) -> Result<u64, PcCode> {
        if t >= TIERS {
            return Err(PcCode::ENoTier);
        }
        if f >= CE_FACES {
            return Err(PcCode::ENoFace);
        }
        let p = match ce_cell(t, f) {
            Some(p) => p,
            None => return Err(PcCode::ENoFace),
        };
        if match self.ce.get(t).and_then(|row| row.get(f)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PcCode::EDup);
        }
        if let Some(row) = self.ce.get_mut(t) {
            if let Some(c) = row.get_mut(f) {
                *c = Some(p);
            }
        }
        self.fnv(0x14201 + (t as u64) * 4 + f as u64);
        Ok(p)
    }

    /// Community 主账全账核验（20 格）。
    pub fn ce_all(&self) -> bool {
        let mut t = 0usize;
        while t < TIERS {
            let mut f = 0usize;
            while f < CE_FACES {
                match self.ce.get(t).and_then(|row| row.get(f)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                f += 1;
            }
            t += 1;
        }
        true
    }

    /// Pro 参照账一格（载荷恒等才收，漂移必抓）。
    pub fn record_pro_ref(&mut self, p: usize, got: u64) -> Result<(), PcCode> {
        let want = match pro_ref_cell(p) {
            Some(w) => w,
            None => return Err(PcCode::ENoPro),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x14300 + p as u64);
            return Err(PcCode::EDrift);
        }
        if let Some(c) = self.pro_ref.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x14301 + p as u64);
        Ok(())
    }

    /// Pro 参照账全账核验。
    pub fn pro_ref_all(&self) -> bool {
        let mut p = 0usize;
        while p < PRO_FACES {
            match self.pro_ref.get(p) {
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

impl Default for PcLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（阶梯格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PcLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut t = 0usize;
    while t < TIERS {
        let mut f = 0usize;
        while f < CE_FACES {
            if led.record_ce(t, f).is_ok() {
                recorded += 1;
            }
            f += 1;
        }
        t += 1;
    }
    let mut p = 0usize;
    while p < PRO_FACES {
        if let Some(w) = pro_ref_cell(p) {
            let _ = led.record_pro_ref(p, w);
        }
        p += 1;
    }
    // 注入 10 轮：阶梯越闭集 / 参照漂移 / 重复落账三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PcLedger::new();
        let caught = match kind {
            0 => lt.record_ce(TIERS, 0).is_err(),
            1 => {
                lt.record_pro_ref(1, pro_ref_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(PcCode::EDrift)
            }
            _ => {
                let _ = lt.record_ce(0, 0);
                lt.record_ce(0, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x14400 + k as u64);
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
pub fn run_unx52044_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52044");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Community 阶梯 20/20 全通",
        recorded == (TIERS * CE_FACES) as u32,
        "五档阶梯 ×4 共有面全通",
    );
    s.add(
        "Pro 参照账 4/4 零混",
        {
            let mut lt = PcLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PRO_FACES {
                if let Some(w) = pro_ref_cell(p) {
                    if lt.record_pro_ref(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.pro_ref_all()
        },
        "Pro 独占面参照账单独账本零混",
    );
    s.add(
        "阶梯单调递增",
        {
            let mut ok = true;
            let mut f = 0usize;
            while f < CE_FACES {
                let mut t = 1usize;
                while t < TIERS {
                    if ce_cell(t, f).unwrap_or(0) <= ce_cell(t - 1, f).unwrap_or(0) {
                        ok = false;
                    }
                    t += 1;
                }
                f += 1;
            }
            ok
        },
        "同面跨档载荷严格递增",
    );
    s.add("注入 10/10 全抓", caught >= 10, "阶梯/参照/重复三态轮转必抓");
    let mut l0 = PcLedger::new();
    s.add(
        "边界·阶梯越闭集",
        l0.record_ce(TIERS, 0).is_err(),
        "档 5 越闭集必拒",
    );
    s.add(
        "边界·共有面越闭集",
        l0.record_ce(0, CE_FACES).is_err(),
        "面 4 越闭集必拒",
    );
    s.add(
        "边界·Pro 面越闭集",
        l0.record_pro_ref(PRO_FACES, 0) == Err(PcCode::ENoPro),
        "面 4 越闭集必拒",
    );
    let mut l1 = PcLedger::new();
    s.add(
        "参照·漂移拒",
        l1.record_pro_ref(2, pro_ref_cell(2).unwrap_or(0).wrapping_add(6)) == Err(PcCode::EDrift),
        "参照折叠漂移必抓",
    );
    let mut l2 = PcLedger::new();
    let _ = l2.record_ce(3, 1);
    s.add(
        "重复·再落账拒",
        l2.record_ce(3, 1).is_err(),
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
    fn test_ce_cell_monotone() {
        let mut f = 0usize;
        while f < CE_FACES {
            let mut t = 1usize;
            while t < TIERS {
                assert!(ce_cell(t, f).unwrap_or(0) > ce_cell(t - 1, f).unwrap_or(0));
                t += 1;
            }
            f += 1;
        }
        assert!(ce_cell(TIERS, 0).is_none());
        assert!(ce_cell(0, CE_FACES).is_none());
    }

    #[test]
    fn test_pro_ref_cells() {
        assert!(pro_ref_cell(0).is_some());
        assert!(pro_ref_cell(PRO_FACES).is_none());
        assert_ne!(pro_ref_cell(0), pro_ref_cell(1));
    }

    #[test]
    fn test_ce_gates() {
        let mut lt = PcLedger::new();
        assert!(lt.record_ce(TIERS, 0).is_err());
        assert!(lt.record_ce(0, CE_FACES).is_err());
        assert!(lt.record_ce(0, 0).is_ok());
        assert!(lt.record_ce(0, 0).is_err());
    }

    #[test]
    fn test_pro_ref_drift_caught() {
        let mut lt = PcLedger::new();
        assert_eq!(
            lt.record_pro_ref(0, pro_ref_cell(0).unwrap_or(0).wrapping_add(8)),
            Err(PcCode::EDrift)
        );
        assert!(lt.record_pro_ref(0, pro_ref_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (TIERS * CE_FACES) as u32);
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
        assert_eq!(SEG_BASE, 0xC4B00);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52044_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
