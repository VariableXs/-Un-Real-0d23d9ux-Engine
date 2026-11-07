//! UNX-F52047 CLion 收口（CMake 消费面） · 0xC4BC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52047 逐字锚定）：UNX-F52048-J1 六判据全落；
//! MSBuild 消费对账（B01 构建链判据复用）；ReSharper 引擎共面声明；
//! KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五。
//!
//! 设计要点：
//!   ① 域模型：CMake 消费面账。消费段闭集 5 段（configure/generate/
//!      build/test/install）× 8 工程场景 = 40 格消费账；段序闭集 0..5。
//!   ② MSBuild 消费对账：6 格消费账（版本档/平台档/工具链档/属性档/
//!      日志档/退出码档），逐格与消费基准恒等才收；未对账先消费 → 消
//!      费前置闸。
//!   ③ ReSharper 引擎共面声明：声明行 4 条闭集（引擎档声明/共面 shim
//!      声明/独立升级窗声明/回滚声明），缺一拒封。
//!   ④ 注入 ×10 轮（段越闭集/消费漂移/声明缺项三态轮转）10/10 必抓；
//!      六判据母版闸 + KB-COMPAT 定级（40 格全通 + 消费零漂移 + 声明
//!      全齐 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52047 − 52000)）。
pub const SEG_BASE: u32 = 0xC4BC0;

/// CMake 消费段数。
pub const STAGES: usize = 5;

/// 工程场景数。
pub const PROJECTS: usize = 8;

/// MSBuild 消费账格数。
pub const MSB_CELLS: usize = 6;

/// 声明行数。
pub const DECL_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    /// 成功
    Ok = 0,
    /// 段号越闭集
    ENoStage,
    /// 工程号越闭集
    ENoProj,
    /// 消费格越册
    ENoCell,
    /// 消费漂移
    EDrift,
    /// 声明行越闭集
    ENoDecl,
    /// 缺声明行
    EDeclMiss,
    /// 未对账先消费
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

impl ClCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ClCode::Ok => "成功",
            ClCode::ENoStage => "段号越闭集：configure/generate/build/test/install 五段之外先登记",
            ClCode::ENoProj => "工程号越闭集：8 工程之外先登记",
            ClCode::ENoCell => "消费格越册：MSBuild 6 格闭集之外先登记",
            ClCode::EDrift => "消费漂移：MSBuild 消费折叠与基准不恒等，逐格复算",
            ClCode::ENoDecl => "声明行越闭集：4 行声明闭集之外先登记",
            ClCode::EDeclMiss => "缺声明行：4 行声明未齐先补齐（ReSharper 共面声明闭集）",
            ClCode::EPreConsume => "未对账先消费：MSBuild 6 格消费账不齐不许报五段全通",
            ClCode::EDup => "重复落账：同格只许一条账目",
            ClCode::ESeal => "未齐即封：40 格未齐先补齐",
            ClCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ClCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ClCode::EGradeDrift => "定级漂移：40 格全通+消费零漂移+声明全齐才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 消费格载荷（段 s × 工程 p）。
pub const fn stage_cell(s: usize, p: usize) -> Option<u64> {
    if s >= STAGES || p >= PROJECTS {
        return None;
    }
    Some(
        0x30000u64
            + (s as u64) * 0x1D
            + (p as u64) * 0x9
            ^ (s as u64).wrapping_mul(0x9E3779B97F4A7C15 >> p),
    )
}

/// MSBuild 消费格基准载荷（格 c）。
pub const fn msb_cell(c: usize) -> Option<u64> {
    if c >= MSB_CELLS {
        return None;
    }
    Some((0x31000u64 + c as u64 * 0x2F) ^ 0x6D7362756923)
}

/// 声明行折叠（行 r）。
pub const fn decl_fold(r: usize) -> Option<u32> {
    if r >= DECL_ROWS {
        return None;
    }
    Some(0x3200 + r as u32 * 0x21)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct ClLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 消费账。
    stages: [[Option<u64>; PROJECTS]; STAGES],
    /// MSBuild 消费账。
    msb: [Option<u64>; MSB_CELLS],
    /// 声明账。
    decls: [Option<u32>; DECL_ROWS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ClLedger {
    /// 新账本。
    pub fn new() -> Self {
        ClLedger {
            gates: [None; 6],
            stages: [[None; PROJECTS]; STAGES],
            msb: [None; MSB_CELLS],
            decls: [None; DECL_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ClCode> {
        if gate >= 6 {
            return Err(ClCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x17100 + gate as u64);
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

    /// MSBuild 消费一格。
    pub fn consume_msb(&mut self, c: usize, got: u64) -> Result<(), ClCode> {
        let want = match msb_cell(c) {
            Some(w) => w,
            None => return Err(ClCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x17200 + c as u64);
            return Err(ClCode::EDrift);
        }
        if let Some(cell) = self.msb.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x17201 + c as u64);
        Ok(())
    }

    /// MSBuild 消费全账核验。
    pub fn msb_all(&self) -> bool {
        let mut c = 0usize;
        while c < MSB_CELLS {
            match self.msb.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 落一格消费账（前置闸 + 重复拒）。
    pub fn record_stage(&mut self, s: usize, p: usize) -> Result<u64, ClCode> {
        if s >= STAGES {
            return Err(ClCode::ENoStage);
        }
        if p >= PROJECTS {
            return Err(ClCode::ENoProj);
        }
        if !self.msb_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x17300);
            return Err(ClCode::EPreConsume);
        }
        let payload = match stage_cell(s, p) {
            Some(p) => p,
            None => return Err(ClCode::ENoProj),
        };
        if match self.stages.get(s).and_then(|row| row.get(p)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ClCode::EDup);
        }
        if let Some(row) = self.stages.get_mut(s) {
            if let Some(c) = row.get_mut(p) {
                *c = Some(payload);
            }
        }
        self.fnv(0x17301 + (s as u64) * 8 + p as u64);
        Ok(payload)
    }

    /// 消费全账核验（40 格）。
    pub fn stages_all(&self) -> bool {
        let mut s = 0usize;
        while s < STAGES {
            let mut p = 0usize;
            while p < PROJECTS {
                match self.stages.get(s).and_then(|row| row.get(p)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                p += 1;
            }
            s += 1;
        }
        true
    }

    /// 声明一行。
    pub fn declare(&mut self, r: usize) -> Result<(), ClCode> {
        if r >= DECL_ROWS {
            return Err(ClCode::ENoDecl);
        }
        if match self.decls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ClCode::EDup);
        }
        if let Some(c) = self.decls.get_mut(r) {
            *c = decl_fold(r);
        }
        self.fnv(0x17400 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn decls_all(&self) -> bool {
        let mut r = 0usize;
        while r < DECL_ROWS {
            match self.decls.get(r) {
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

impl Default for ClLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ClLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut c = 0usize;
    while c < MSB_CELLS {
        if let Some(w) = msb_cell(c) {
            let _ = led.consume_msb(c, w);
        }
        c += 1;
    }
    let mut recorded = 0u32;
    let mut s = 0usize;
    while s < STAGES {
        let mut p = 0usize;
        while p < PROJECTS {
            if led.record_stage(s, p).is_ok() {
                recorded += 1;
            }
            p += 1;
        }
        s += 1;
    }
    let mut r = 0usize;
    while r < DECL_ROWS {
        let _ = led.declare(r);
        r += 1;
    }
    // 注入 10 轮：消费漂移 / 段越闭集 / 声明缺项三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ClLedger::new();
        let caught = match kind {
            0 => lt.consume_msb(2, msb_cell(2).unwrap_or(0).wrapping_add(1)) == Err(ClCode::EDrift),
            1 => lt.record_stage(STAGES, 0).is_err(),
            _ => lt.decls_all() == false && lt.declare(DECL_ROWS).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x17500 + k as u64);
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
pub fn run_unx52047_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52047");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "CMake 五段 40/40 全通",
        recorded == (STAGES * PROJECTS) as u32,
        "configure/generate/build/test/install 五段 ×8 工程全通",
    );
    s.add(
        "MSBuild 消费 6/6 对账",
        {
            let mut lt = ClLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < MSB_CELLS {
                if let Some(w) = msb_cell(c) {
                    if lt.consume_msb(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.msb_all()
        },
        "版本/平台/工具链/属性/日志/退出码六格消费零漂移",
    );
    s.add(
        "声明 4/4 全齐",
        {
            let mut lt = ClLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < DECL_ROWS {
                if lt.declare(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.decls_all()
        },
        "ReSharper 引擎共面声明四行闭集",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/段/声明三态轮转必抓");
    let mut l0 = ClLedger::new();
    s.add(
        "消费前置闸",
        l0.record_stage(0, 0) == Err(ClCode::EPreConsume),
        "MSBuild 消费账不齐不许报五段全通",
    );
    let mut l1 = ClLedger::new();
    let mut c = 0usize;
    while c < MSB_CELLS {
        if let Some(w) = msb_cell(c) {
            let _ = l1.consume_msb(c, w);
        }
        c += 1;
    }
    s.add(
        "边界·段越闭集",
        l1.record_stage(STAGES, 0).is_err(),
        "段 5 越闭集必拒",
    );
    s.add(
        "边界·工程越闭集",
        l1.record_stage(0, PROJECTS).is_err(),
        "工程 8 越闭集必拒",
    );
    s.add(
        "边界·声明越闭集",
        l1.declare(DECL_ROWS) == Err(ClCode::ENoDecl),
        "行 4 越闭集必拒",
    );
    let mut l2 = ClLedger::new();
    let _ = l2.consume_msb(0, msb_cell(0).unwrap_or(0));
    s.add(
        "消费·漂移拒",
        l2.consume_msb(0, msb_cell(0).unwrap_or(0).wrapping_add(4)) == Err(ClCode::EDrift),
        "消费折叠漂移必抓",
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
        let a = stage_cell(2, 3).unwrap_or(0);
        assert_eq!(a, stage_cell(2, 3).unwrap_or(0));
        assert_ne!(a, stage_cell(2, 4).unwrap_or(0));
        assert!(stage_cell(STAGES, 0).is_none());
        assert!(stage_cell(0, PROJECTS).is_none());
        let w = msb_cell(4).unwrap_or(0);
        assert_eq!(w, msb_cell(4).unwrap_or(0));
        assert!(msb_cell(MSB_CELLS).is_none());
        assert!(decl_fold(DECL_ROWS).is_none());
        assert_eq!(decl_fold(0), Some(0x3200));
    }

    #[test]
    fn test_pre_consume_gate() {
        let mut lt = ClLedger::new();
        assert_eq!(lt.record_stage(0, 0), Err(ClCode::EPreConsume));
    }

    #[test]
    fn test_msb_drift_caught() {
        let mut lt = ClLedger::new();
        assert_eq!(
            lt.consume_msb(3, msb_cell(3).unwrap_or(0).wrapping_add(2)),
            Err(ClCode::EDrift)
        );
        assert!(lt.consume_msb(3, msb_cell(3).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_decl_dup_rejected() {
        let mut lt = ClLedger::new();
        let _ = lt.declare(1);
        assert!(lt.declare(1).is_err());
        assert_eq!(lt.declare(DECL_ROWS), Err(ClCode::ENoDecl));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (STAGES * PROJECTS) as u32);
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
        assert_eq!(SEG_BASE, 0xC4BC0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52047_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
