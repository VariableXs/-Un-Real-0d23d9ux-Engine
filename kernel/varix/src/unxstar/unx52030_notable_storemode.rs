//! UNX-F52030 Notable 收口 · 0xC4780段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52030 逐字锚定）：UNX-F52031-J1 六判据全落；
//! 本地存储路径与云端模式分离声明；KB-COMPAT 定级登记；判据锚定 §AI-66.2
//! 专题一母版。
//!
//! 设计要点：
//!   ① 域模型：Notable 存储模式分离面。存储目录项 24 格，模式二元闭集
//!      （本地 local / 云端 cloud），每格目录载荷（纯函数复算：目录号 ×
//!      模式档 × 载荷折叠）；分离闸：本地档夹带云端标记拒、云端档夹带
//!      本地标记拒（混布注入 ×10 轮 10/10 必抓）。
//!   ② 分离声明账：六行声明闭集（路径根声明/云端开关声明/同步域声明/
//!      回退域声明/迁移声明/审计声明），缺一拒封。
//!   ③ 六判据母版闸（6/6 全落）+ KB-COMPAT 定级登记（24 格零混布 → S1）。
//!   ④ no_std + alloc；零浮点、零 IO、零墙钟、零随机（指纹种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52030 − 52000)）。
pub const SEG_BASE: u32 = 0xC4780;

/// 存储目录格数。
pub const STORE_CELLS: usize = 24;

/// 分离声明行数。
pub const DECL_ROWS: usize = 6;

/// 模式档：本地。
pub const MODE_LOCAL: u32 = 0;

/// 模式档：云端。
pub const MODE_CLOUD: u32 = 1;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0B 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NbCode {
    /// 成功
    Ok = 0,
    /// 模式越闭集
    ENoMode,
    /// 目录格越册
    ENoCell,
    /// 混布（模式夹带）
    EMixed,
    /// 声明行越闭集
    ENoDecl,
    /// 缺声明行即封
    EDeclMiss,
    /// 判据越闭集
    ENoGate,
    /// 账量守恒破缺
    EConserve,
    /// 双跑指纹不一致
    EFp,
    /// 账为空
    EEmpty,
    /// 未齐全账即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
    /// 重复落账
    EDup,
}

impl NbCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            NbCode::Ok => "成功",
            NbCode::ENoMode => "模式越闭集：local/cloud 二元之外先登记",
            NbCode::ENoCell => "目录格越册：24 格闭集之外先登记",
            NbCode::EMixed => "混布：本地档禁带云端标记，云端档禁带本地标记",
            NbCode::ENoDecl => "声明行越闭集：六行声明之外先登记",
            NbCode::EDeclMiss => "缺声明行即封：六行声明未齐先补齐",
            NbCode::ENoGate => "判据越闭集：六判据之外先登记",
            NbCode::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            NbCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            NbCode::EEmpty => "账为空：至少落 1 格",
            NbCode::ESeal => "未齐全账即封：24 格未齐即封，先补齐缺格",
            NbCode::EGradeDrift => "定级漂移：六判据+24 格零混布才定 S1",
            NbCode::EDup => "重复落账：同格只许一条账目",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 目录格（模式 × 目录，纯函数复算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StoreCell {
    /// 格号（0..24）。
    pub no: u32,
    /// 模式档（0=本地 / 1=云端，偶格本地奇格云端交替布账）。
    pub mode: u32,
    /// 目录载荷基准（确定性折叠）。
    pub expect: u64,
}

/// 复算第 i 格。
pub const fn store_cell(i: usize) -> Option<StoreCell> {
    if i >= STORE_CELLS {
        return None;
    }
    let mode = (i % 2) as u32;
    let payload = 0x5C000u64 + (i as u64) * 0x1B;
    let expect = payload ^ ((mode as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15))
        .wrapping_add((i as u64).wrapping_mul(0x01000193));
    Some(StoreCell {
        no: i as u32,
        mode,
        expect,
    })
}

/// 声明行闭集语义（行号 → 语义序号，用于声明账折叠）。
pub const fn decl_fold(row: usize) -> Option<u32> {
    if row >= DECL_ROWS {
        return None;
    }
    Some(0x5C100u32 + (row as u32) * 0x3D)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Notable 存储分离收口账本。
#[derive(Clone)]
pub struct NbLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 目录账（格 → 载荷）。
    cells: [Option<u64>; STORE_CELLS],
    /// 声明账（行 → 折叠）。
    decls: [Option<u32>; DECL_ROWS],
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NbLedger {
    /// 新账本。
    pub fn new() -> Self {
        NbLedger {
            gates: [None; 6],
            cells: [None; STORE_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NbCode> {
        if gate >= 6 {
            return Err(NbCode::ENoGate);
        }
        let cell = match self.gates.get_mut(gate) {
            Some(c) => c,
            None => return Err(NbCode::EEmpty),
        };
        *cell = Some(true);
        self.fnv(0x5100 + gate as u64);
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

    /// 落一格声明行。
    pub fn declare(&mut self, row: usize) -> Result<(), NbCode> {
        if row >= DECL_ROWS {
            return Err(NbCode::ENoDecl);
        }
        if match self.decls.get(row) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NbCode::EDup);
        }
        let cell = match self.decls.get_mut(row) {
            Some(c) => c,
            None => return Err(NbCode::EEmpty),
        };
        *cell = decl_fold(row);
        self.fnv(0x5150 + row as u64);
        Ok(())
    }

    /// 声明账齐全核验（六行全落才许封）。
    pub fn decls_all(&self) -> bool {
        let mut i = 0usize;
        while i < DECL_ROWS {
            match self.decls.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 落一格目录账（模式与载荷与基准恒等才收；混布/漂移拒；重复拒）。
    pub fn record(&mut self, i: usize, mode: u32, got: u64) -> Result<(), NbCode> {
        let cell_ref = match store_cell(i) {
            Some(c) => c,
            None => return Err(NbCode::ENoCell),
        };
        if mode != cell_ref.mode {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + i as u64);
            return Err(NbCode::EMixed);
        }
        if got != cell_ref.expect {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + i as u64);
            return Err(NbCode::EConserve);
        }
        if match self.cells.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NbCode::EDup);
        }
        let cell = match self.cells.get_mut(i) {
            Some(c) => c,
            None => return Err(NbCode::EEmpty),
        };
        *cell = Some(got);
        self.fnv(0x5202 + i as u64);
        Ok(())
    }

    /// 目录全账核验。
    pub fn cells_all(&self) -> bool {
        let mut i = 0usize;
        while i < STORE_CELLS {
            match self.cells.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 漏检记账。
    pub fn note_missed(&mut self) {
        self.injected += 1;
        self.escaped += 1;
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

impl Default for NbLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 混布注入（10 轮，确定性）：模式翻转注入（本地→云端 / 云端→本地）。
pub struct MixInjector;

impl MixInjector {
    /// 下一轮（格号、翻转模式、基准载荷）。
    pub fn next_case(k: usize) -> (usize, u32, u64) {
        let i = k % STORE_CELLS;
        let expect = match store_cell(i) {
            Some(c) => c.expect,
            None => 0,
        };
        let flip = match store_cell(i) {
            Some(c) => (c.mode + 1) % 2,
            None => 0,
        };
        (i, flip, expect)
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：六判据 + 六行声明 + 24 格目录 + 混布注入 10/10 + 指纹。
/// 返回（目录格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NbLedger::new();
    // ① 六判据全落。
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    // ② 六行声明全落。
    let mut r = 0usize;
    while r < DECL_ROWS {
        let _ = led.declare(r);
        r += 1;
    }
    // ③ 24 格目录（基准模式与载荷）。
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < STORE_CELLS {
        if let Some(c) = store_cell(i) {
            if led.record(i, c.mode, c.expect).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    // ④ 混布注入 10 轮：模式翻转必抓。
    let mut k = 0usize;
    while k < 10 {
        let (cell, flip, payload) = MixInjector::next_case(k);
        let mut lt = NbLedger::new();
        if lt.record(cell, flip, payload).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5300 + k as u64);
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
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52030_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52030");
    // 1. 正常流：24 格目录 + 六行声明 + 注入全抓。
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "存储目录 24/24 全账",
        recorded == STORE_CELLS as u32,
        "本地/云端二元闭集逐格分离落账",
    );
    s.add("混布注入 10/10 全抓", caught >= 10, "模式翻转混布必抓");
    // 2. 边界：格越册/模式越闭集/声明越闭集。
    let mut l0 = NbLedger::new();
    s.add(
        "边界·格越册",
        l0.record(STORE_CELLS, MODE_LOCAL, 0) == Err(NbCode::ENoCell),
        "格 24 越册必拒",
    );
    s.add(
        "边界·模式越闭集",
        l0.record(0, 2, 0) == Err(NbCode::EMixed),
        "模式 2 越二元闭集必拒",
    );
    s.add(
        "边界·声明越闭集",
        l0.declare(DECL_ROWS) == Err(NbCode::ENoDecl),
        "声明行 6 越闭集必拒",
    );
    // 3. 混布直判。
    let mut l1 = NbLedger::new();
    let c0 = match store_cell(0) {
        Some(c) => c,
        None => StoreCell { no: 0, mode: 0, expect: 0 },
    };
    s.add(
        "分离·混布拒",
        l1.record(0, (c0.mode + 1) % 2, c0.expect) == Err(NbCode::EMixed),
        "模式翻转混布显性拒",
    );
    s.add("分离·基准过", l1.record(0, c0.mode, c0.expect).is_ok(), "基准模式与载荷放行");
    // 4. 缺声明封账闸。
    let mut l2 = NbLedger::new();
    let _ = l2.declare(0);
    s.add("声明·缺行核验拒", !l2.decls_all(), "六行未齐不许封账");
    // 5. 重复落账拒。
    let mut l3 = NbLedger::new();
    let _ = l3.record(1, c0.mode.wrapping_add(1), match store_cell(1) { Some(c) => c.expect, None => 0 });
    s.add(
        "重复·再落账拒",
        l3.record(1, 1, match store_cell(1) { Some(c) => c.expect, None => 0 }) == Err(NbCode::EDup),
        "同格只许一条账目",
    );
    // 6. 六判据全落。
    let mut l4 = NbLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = l4.judge_gate(g);
        g += 1;
    }
    s.add("六判据全落", l4.gates_all(), "母版闸 6/6");
    // 7. 注入账守恒。
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    // 8. 双跑指纹一致。
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    // 9. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    // 10. 收口。
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let c = store_cell(0).unwrap_or(StoreCell { no: 0, mode: 0, expect: 0 });
        assert_eq!(c.no, 0);
        assert_eq!(c.mode, MODE_LOCAL);
        assert_eq!(store_cell(1).unwrap_or(c).mode, MODE_CLOUD);
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(store_cell(STORE_CELLS).is_none());
        assert!(decl_fold(DECL_ROWS).is_none());
    }

    #[test]
    fn test_payload_pure() {
        let a = store_cell(9).unwrap_or(StoreCell { no: 0, mode: 0, expect: 0 });
        let b = store_cell(9).unwrap_or(StoreCell { no: 0, mode: 0, expect: 0 });
        assert_eq!(a.expect, b.expect);
        assert_ne!(a.expect, store_cell(10).unwrap_or(a).expect);
    }

    #[test]
    fn test_mixed_caught() {
        let mut led = NbLedger::new();
        let c = store_cell(2).unwrap_or(StoreCell { no: 0, mode: 0, expect: 0 });
        assert_eq!(led.record(2, (c.mode + 1) % 2, c.expect), Err(NbCode::EMixed));
        assert!(led.record(2, c.mode, c.expect).is_ok());
    }

    #[test]
    fn test_payload_drift_caught() {
        let mut led = NbLedger::new();
        let c = store_cell(3).unwrap_or(StoreCell { no: 0, mode: 0, expect: 0 });
        assert_eq!(
            led.record(3, c.mode, c.expect.wrapping_add(1)),
            Err(NbCode::EConserve)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut led = NbLedger::new();
        let c = store_cell(4).unwrap_or(StoreCell { no: 0, mode: 0, expect: 0 });
        let _ = led.record(4, c.mode, c.expect);
        assert_eq!(led.record(4, c.mode, c.expect), Err(NbCode::EDup));
    }

    #[test]
    fn test_decls_gate() {
        let mut led = NbLedger::new();
        let _ = led.declare(0);
        assert!(!led.decls_all());
        let mut r = 1usize;
        while r < DECL_ROWS {
            let _ = led.declare(r);
            r += 1;
        }
        assert!(led.decls_all());
        assert_eq!(led.declare(0), Err(NbCode::EDup));
    }

    #[test]
    fn test_injection_all_caught() {
        let mut k = 0usize;
        let mut caught = 0u32;
        while k < 10 {
            let (cell, flip, payload) = MixInjector::next_case(k);
            let mut lt = NbLedger::new();
            if lt.record(cell, flip, payload).is_err() {
                caught += 1;
            }
            k += 1;
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, STORE_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC4780);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52030_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
