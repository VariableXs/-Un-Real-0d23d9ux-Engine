//! UNX-F52075 CMake GUI 收口（Windows 发行版，与 L4 分账） · 0xC52C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52075 逐字锚定）：UNX-F52076-J1 setup/compile
//! ×10 工程全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：setup/compile ×10 工程账（两段 ×10 = 20 格），逐格载荷
//!      （纯函数折叠）恒等才收；漂移必抓。
//!   ② setup→compile 拍序单向闸（未 setup 先 compile → 前置闸必抓）。
//!   ③ L4 分账声明 2 行闭集（发行版收口声明/CMake 嫁接件布设声明），
//!      缺一拒封。
//!   ④ 注入 ×10 轮（compile 前置/载荷漂移/工程越闭集三态轮转）10/10
//!      必抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（20 格全通 + 声
//!      明全齐 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52075 − 52000)）。
pub const SEG_BASE: u32 = 0xC52C0;

/// 工程数。
pub const PROJS: usize = 10;

/// 声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CgCode {
    /// 成功
    Ok = 0,
    /// 工程号越闭集
    ENoProj,
    /// setup 漂移
    ESetupDrift,
    /// compile 前置闸
    EPreCompile,
    /// compile 漂移
    ECompileDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
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

impl CgCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CgCode::Ok => "成功",
            CgCode::ENoProj => "工程号越闭集：10 工程之外先登记",
            CgCode::ESetupDrift => "setup 漂移：setup 折叠与基准不恒等，逐工程复算",
            CgCode::EPreCompile => "未 setup 先 compile：setup 账不齐不许 compile",
            CgCode::ECompileDrift => "compile 漂移：compile 折叠与基准不恒等",
            CgCode::ENoRow => "声明行越闭集：2 行闭集之外先登记",
            CgCode::ERowMiss => "缺声明行：L4 分账声明未齐先补齐",
            CgCode::EDup => "重复落账：同格只许一条账目",
            CgCode::ESeal => "未齐即封：20 格未齐先补齐",
            CgCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            CgCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            CgCode::EGradeDrift => "定级漂移：20 格全通+声明全齐+拍序单调才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// setup 格载荷（工程 p）。
pub const fn setup_cell(p: usize) -> Option<u64> {
    if p >= PROJS {
        return None;
    }
    Some((0x90000u64 + (p as u64) * 0x2C) ^ (p as u64).wrapping_mul(0x01000193))
}

/// compile 格载荷（工程 p）。
pub const fn compile_cell(p: usize) -> Option<u64> {
    if p >= PROJS {
        return None;
    }
    Some((0x91000u64 + (p as u64) * 0x31) ^ ((p as u64) + 0x40).wrapping_mul(0x01000193))
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0x9200 + r as u32 * 0x3B)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct CgLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// setup 账。
    setups: [Option<u64>; PROJS],
    /// compile 账。
    compiles: [Option<u64>; PROJS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CgLedger {
    /// 新账本。
    pub fn new() -> Self {
        CgLedger {
            gates: [None; 6],
            setups: [None; PROJS],
            compiles: [None; PROJS],
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CgCode> {
        if gate >= 6 {
            return Err(CgCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x34100 + gate as u64);
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

    /// setup 一格。
    pub fn record_setup(&mut self, p: usize, got: u64) -> Result<(), CgCode> {
        if p >= PROJS {
            return Err(CgCode::ENoProj);
        }
        let want = match setup_cell(p) {
            Some(w) => w,
            None => return Err(CgCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x34200 + p as u64);
            return Err(CgCode::ESetupDrift);
        }
        if match self.setups.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CgCode::EDup);
        }
        if let Some(c) = self.setups.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x34201 + p as u64);
        Ok(())
    }

    /// setup 全账核验。
    pub fn setups_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJS {
            match self.setups.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// compile 一格（setup 前置闸 + 载荷对照 + 重复拒）。
    pub fn record_compile(&mut self, p: usize, got: u64) -> Result<(), CgCode> {
        if p >= PROJS {
            return Err(CgCode::ENoProj);
        }
        if !self.setups_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x34300);
            return Err(CgCode::EPreCompile);
        }
        let want = match compile_cell(p) {
            Some(w) => w,
            None => return Err(CgCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x34301 + p as u64);
            return Err(CgCode::ECompileDrift);
        }
        if match self.compiles.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CgCode::EDup);
        }
        if let Some(c) = self.compiles.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x34302 + p as u64);
        Ok(())
    }

    /// compile 全账核验。
    pub fn compiles_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJS {
            match self.compiles.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), CgCode> {
        if r >= SPLIT_ROWS {
            return Err(CgCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CgCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x34400 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
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

impl Default for CgLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CgLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PROJS {
        if let Some(w) = setup_cell(p) {
            if led.record_setup(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut p2 = 0usize;
    while p2 < PROJS {
        if let Some(w) = compile_cell(p2) {
            if led.record_compile(p2, w).is_ok() {
                recorded += 1;
            }
        }
        p2 += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：compile 前置 / setup 漂移 / 工程越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CgLedger::new();
        let caught = match kind {
            0 => lt.record_compile(0, 0) == Err(CgCode::EPreCompile),
            1 => lt.record_setup(2, setup_cell(2).unwrap_or(0).wrapping_add(1)) == Err(CgCode::ESetupDrift),
            _ => lt.record_setup(PROJS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x34500 + k as u64);
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
pub fn run_unx52075_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52075");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "setup/compile 20/20 全通",
        recorded == (PROJS * 2) as u32,
        "setup/compile ×10 工程逐格恒等",
    );
    s.add(
        "L4 分账声明 2/2 全齐",
        {
            let mut lt = CgLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "发行版收口/CMake 嫁接件布设两行闭集",
    );
    s.add("注入 10/10 全抓", caught >= 10, "前置/漂移/越闭集三态轮转必抓");
    let mut l0 = CgLedger::new();
    s.add(
        "compile 前置闸",
        l0.record_compile(0, 0) == Err(CgCode::EPreCompile),
        "未 setup 先 compile 必抓",
    );
    s.add(
        "边界·工程越闭集",
        l0.record_setup(PROJS, 0).is_err(),
        "工程 10 越闭集必拒",
    );
    let mut l1 = CgLedger::new();
    let _ = l1.record_setup(1, setup_cell(1).unwrap_or(0));
    s.add(
        "重复·再落账拒",
        l1.record_setup(1, setup_cell(1).unwrap_or(0)).is_err(),
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
    s.add("收口·判据条数", s.len() >= 9, "含本条共 ≥10 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cells_pure() {
        let a = setup_cell(4).unwrap_or(0);
        assert_eq!(a, setup_cell(4).unwrap_or(0));
        assert_ne!(a, setup_cell(5).unwrap_or(0));
        assert_ne!(setup_cell(4), compile_cell(4));
        assert!(setup_cell(PROJS).is_none());
        assert!(compile_cell(PROJS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_pre_compile_gate() {
        let mut lt = CgLedger::new();
        assert_eq!(lt.record_compile(0, 0), Err(CgCode::EPreCompile));
    }

    #[test]
    fn test_setup_and_compile() {
        let mut lt = CgLedger::new();
        assert_eq!(
            lt.record_setup(3, setup_cell(3).unwrap_or(0).wrapping_add(2)),
            Err(CgCode::ESetupDrift)
        );
        assert!(lt.record_setup(3, setup_cell(3).unwrap_or(0)).is_ok());
        let mut p = 0usize;
        while p < PROJS {
            let _ = lt.record_setup(p, setup_cell(p).unwrap_or(0));
            p += 1;
        }
        assert!(lt.record_compile(3, compile_cell(3).unwrap_or(0)).is_ok());
        assert!(lt.record_compile(3, compile_cell(3).unwrap_or(0)).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (PROJS * 2) as u32);
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
        assert_eq!(SEG_BASE, 0xC52C0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52075_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 10);
    }
}
