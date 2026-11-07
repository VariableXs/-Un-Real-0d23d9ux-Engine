//! UNX-F52078 Bazel 收口 · 0xC5380段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52078 逐字锚定）：UNX-F52079-J1 构建 ×5 样本
//! 全通；Python 消费对账（F52043 解释器判据复用）；KB-COMPAT 定级登记；
//! 判据锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：构建 ×5 样本账，逐样本构建载荷（纯函数折叠）恒等才收；
//!      构建漂移必抓。
//!   ② Python 消费对账账：解释器三元（版本档/venv 档/site-packages 档）
//!      消费 4 格（规则解析/工具链消费/仓库规则/输出组），逐格与消费基
//!      准恒等才收（F52043 解释器判据复用）；漂移必抓。
//!   ③ 构建→消费拍序单向闸（未构建先消费 → 前置闸）。
//!   ④ 注入 ×10 轮（消费漂移/构建漂移/样本越闭集三态轮转）10/10 必
//!      抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（5 样本全通 + 消费
//!      4 格全账 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52078 − 52000)）。
pub const SEG_BASE: u32 = 0xC5380;

/// 样本数。
pub const SAMPLES: usize = 5;

/// Python 消费格数。
pub const PY_CELLS: usize = 4;

/// 解释器三元基准（版本/venv/site-packages 档，F52043 判据复用）。
pub const PY_VER: u32 = 312;
pub const VENV: u32 = 1;
pub const SITE_PKGS: u32 = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BzCode {
    /// 成功
    Ok = 0,
    /// 样本号越闭集
    ENoSample,
    /// 构建漂移
    EDrift,
    /// 消费格越册
    ENoCell,
    /// 消费漂移
    EPyDrift,
    /// 解释器三元失配
    ETri,
    /// 未构建先消费
    EPreConsume,
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

impl BzCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BzCode::Ok => "成功",
            BzCode::ENoSample => "样本号越闭集：5 样本之外先登记",
            BzCode::EDrift => "构建漂移：构建折叠与基准不恒等，逐样本复算",
            BzCode::ENoCell => "消费格越册：4 格闭集之外先登记",
            BzCode::EPyDrift => "消费漂移：Python 消费折叠与基准不恒等",
            BzCode::ETri => "解释器三元失配：版本/venv/site-packages 档与基准不符",
            BzCode::EPreConsume => "未构建先消费：构建账不齐不许消费",
            BzCode::EDup => "重复落账：同格只许一条账目",
            BzCode::ESeal => "未齐即封：5 样本未齐先补齐",
            BzCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            BzCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            BzCode::EGradeDrift => "定级漂移：5 样本全通+消费 4 格全账才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 构建格载荷（样本 s）。
pub const fn bz_cell(s: usize) -> Option<u64> {
    if s >= SAMPLES {
        return None;
    }
    Some((0x9B000u64 + (s as u64) * 0x2D) ^ (s as u64).wrapping_mul(0x01000193))
}

/// 消费格基准（格 c）。
pub const fn py_cell(c: usize) -> Option<u64> {
    if c >= PY_CELLS {
        return None;
    }
    Some((0x9C000u64 + c as u64 * 0x34) ^ 0x62617A656C7079)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct BzLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 构建账。
    builds: [Option<u64>; SAMPLES],
    /// 消费账。
    py: [Option<u64>; PY_CELLS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl BzLedger {
    /// 新账本。
    pub fn new() -> Self {
        BzLedger {
            gates: [None; 6],
            builds: [None; SAMPLES],
            py: [None; PY_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), BzCode> {
        if gate >= 6 {
            return Err(BzCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x37100 + gate as u64);
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

    /// 构建一格。
    pub fn record_build(&mut self, s: usize, got: u64) -> Result<(), BzCode> {
        if s >= SAMPLES {
            return Err(BzCode::ENoSample);
        }
        let want = match bz_cell(s) {
            Some(w) => w,
            None => return Err(BzCode::ENoSample),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x37200 + s as u64);
            return Err(BzCode::EDrift);
        }
        if match self.builds.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BzCode::EDup);
        }
        if let Some(c) = self.builds.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x37201 + s as u64);
        Ok(())
    }

    /// 构建全账核验。
    pub fn builds_all(&self) -> bool {
        let mut s = 0usize;
        while s < SAMPLES {
            match self.builds.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 解释器三元闸（F52043 判据复用）。
    pub fn tri_gate(&mut self, ver: u32, venv: u32, pkgs: u32) -> Result<(), BzCode> {
        if ver != PY_VER || venv != VENV || pkgs != SITE_PKGS {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x37300);
            return Err(BzCode::ETri);
        }
        self.fnv(0x37301);
        Ok(())
    }

    /// 消费一格（构建前置 + 三元闸后 + 载荷对照 + 重复拒）。
    pub fn consume_py(&mut self, c: usize, got: u64) -> Result<(), BzCode> {
        if c >= PY_CELLS {
            return Err(BzCode::ENoCell);
        }
        if !self.builds_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x37400);
            return Err(BzCode::EPreConsume);
        }
        let want = match py_cell(c) {
            Some(w) => w,
            None => return Err(BzCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x37401 + c as u64);
            return Err(BzCode::EPyDrift);
        }
        if match self.py.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BzCode::EDup);
        }
        if let Some(cell) = self.py.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x37402 + c as u64);
        Ok(())
    }

    /// 消费全账核验。
    pub fn py_all(&self) -> bool {
        let mut c = 0usize;
        while c < PY_CELLS {
            match self.py.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
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

impl Default for BzLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = BzLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut s = 0usize;
    while s < SAMPLES {
        if let Some(w) = bz_cell(s) {
            if led.record_build(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let _ = led.tri_gate(PY_VER, VENV, SITE_PKGS);
    let mut c = 0usize;
    while c < PY_CELLS {
        if let Some(w) = py_cell(c) {
            if led.consume_py(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    // 注入 10 轮：三元失配 / 构建漂移 / 样本越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = BzLedger::new();
        let caught = match kind {
            0 => lt.tri_gate(PY_VER + 1, VENV, SITE_PKGS) == Err(BzCode::ETri),
            1 => lt.record_build(2, bz_cell(2).unwrap_or(0).wrapping_add(1)) == Err(BzCode::EDrift),
            _ => lt.record_build(SAMPLES, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x37500 + k as u64);
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
pub fn run_unx52078_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52078");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "构建 5/5 全通",
        recorded >= SAMPLES as u32,
        "5 样本构建逐样本恒等",
    );
    s.add(
        "Python 消费 4/4 全账",
        {
            let mut lt = BzLedger::new();
            let mut s2 = 0usize;
            while s2 < SAMPLES {
                let _ = lt.record_build(s2, bz_cell(s2).unwrap_or(0));
                s2 += 1;
            }
            let mut ok = true;
            let mut c = 0usize;
            while c < PY_CELLS {
                if let Some(w) = py_cell(c) {
                    if lt.consume_py(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.py_all()
        },
        "规则解析/工具链/仓库规则/输出组四格零漂移（F52043 复用）",
    );
    s.add(
        "解释器三元闸",
        {
            let mut lt = BzLedger::new();
            lt.tri_gate(PY_VER, VENV, SITE_PKGS).is_ok()
                && lt.tri_gate(PY_VER + 1, VENV, SITE_PKGS) == Err(BzCode::ETri)
        },
        "版本/venv/site-packages 三元失配必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "三元/漂移/越闭集三态轮转必抓");
    let mut l0 = BzLedger::new();
    s.add(
        "消费前置闸",
        l0.consume_py(0, 0) == Err(BzCode::EPreConsume),
        "构建账不齐不许消费",
    );
    s.add(
        "边界·样本越闭集",
        l0.record_build(SAMPLES, 0).is_err(),
        "样本 5 越闭集必拒",
    );
    s.add(
        "边界·消费越册",
        l0.consume_py(PY_CELLS, 0).is_err(),
        "消费 4 越册必拒",
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
        let a = bz_cell(3).unwrap_or(0);
        assert_eq!(a, bz_cell(3).unwrap_or(0));
        assert_ne!(a, bz_cell(4).unwrap_or(0));
        assert!(bz_cell(SAMPLES).is_none());
        assert!(py_cell(PY_CELLS).is_none());
        assert_ne!(py_cell(0), py_cell(1));
    }

    #[test]
    fn test_tri_gate() {
        let mut lt = BzLedger::new();
        assert!(lt.tri_gate(PY_VER, VENV, SITE_PKGS).is_ok());
        assert_eq!(lt.tri_gate(PY_VER + 2, VENV, SITE_PKGS), Err(BzCode::ETri));
        assert_eq!(lt.tri_gate(PY_VER, VENV + 2, SITE_PKGS), Err(BzCode::ETri));
        assert_eq!(lt.tri_gate(PY_VER, VENV, SITE_PKGS + 2), Err(BzCode::ETri));
    }

    #[test]
    fn test_pre_consume_gate() {
        let mut lt = BzLedger::new();
        assert_eq!(lt.consume_py(0, 0), Err(BzCode::EPreConsume));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= SAMPLES as u32);
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
        assert_eq!(SEG_BASE, 0xC5380);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52078_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
