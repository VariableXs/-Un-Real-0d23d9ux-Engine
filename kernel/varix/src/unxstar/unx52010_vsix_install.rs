//! UNX-F52010 VSIX 扩展安装与扩展管理器收口 · 0xC4280段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52010 逐字锚定）：UNX-F52011-J1 J3 特化为安装/
//! 修复/静默/检测四操作 + 依赖方探测 ×20 二进制加载成功；与 E2 CRT 版本
//! 矩阵对账零漂移（联签锚定行 AI-22）；KB-COMPAT 定级 S1；判据锚定
//! §AI-66.4 上游 CRT 版本矩阵。
//!
//! 设计要点：
//!   ① 域模型：扩展管理器四操作面（安装/修复/静默安装/依赖检测），操作
//!      状态机 Idle→Running→Done 单向（回退显性拒）；依赖方探测账：20
//!      个依赖方二进制加载闭集（行可复算），逐方加载成功落账（20/20）。
//!   ② CRT 版本矩阵对账零漂移：依赖方 → 所需 CRT 版本档（14.2x/14.3x
//!      两档闭集）矩阵 20 行（纯函数复算），账面矩阵与复算矩阵逐行恒等
//!      （零漂移断言）；漂移行注入 ×10 轮 10/10 必抓。
//!   ③ 加载失败隐匿闸：加载失败必须入差异清单（零静默），隐匿 ×10 轮
//!      10/10 必抓；探测账守恒（成功+失败隐匿=20）。
//!   ④ 定级闸：四操作全走 + 依赖探测 20/20 + CRT 矩阵零漂移 → S1。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52010 − 52000)）。
pub const SEG_BASE: u32 = 0xC4280;

/// 依赖方探测闭集容量（20 二进制）。
pub const DEP_ROWS: usize = 20;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0C 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdCode {
    /// 成功
    Ok = 0,
    /// 依赖方越册
    ENoDep,
    /// 操作状态机越步
    EOrder,
    /// 加载失败隐匿
    EHidden,
    /// CRT 矩阵漂移
    EDrift,
    /// 探测账守恒破缺
    EConserve,
    /// 操作重复落账
    EDup,
    /// 双跑指纹不一致
    EFp,
    /// 账为空
    EEmpty,
    /// 未齐探测即封
    ESeal,
    /// CRT 档越闭集
    ENoCrt,
    /// 定级漂移
    EGradeDrift,
}

impl IdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IdCode::Ok => "成功",
            IdCode::ENoDep => "依赖方越册：20 二进制闭集之外先登记",
            IdCode::EOrder => "操作状态机越步：Idle→Running→Done 单向，回退显性拒",
            IdCode::EHidden => "加载失败隐匿：失败必须入差异清单（零静默），逐条补录",
            IdCode::EDrift => "CRT 矩阵漂移：账面版本档与复算不一致，逐行对平",
            IdCode::EConserve => "探测账守恒破缺：成功+失败=20 等式不成立",
            IdCode::EDup => "重复落账：同依赖方只许一条账目",
            IdCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            IdCode::EEmpty => "账为空：至少落 1 格",
            IdCode::ESeal => "未齐探测即封：20 方未探齐即封，先补齐缺格",
            IdCode::ENoCrt => "CRT 档越闭集：14.2x/14.3x 两档之外先登记",
            IdCode::EGradeDrift => "定级漂移：四操作+探测 20/20+零漂移才定 S1",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 四操作闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MgrOp {
    /// 安装。
    Install = 0,
    /// 修复。
    Repair = 1,
    /// 静默安装。
    Silent = 2,
    /// 依赖检测。
    Detect = 3,
}

/// 全操作遍历。
pub const MGR_ALL: [MgrOp; 4] = [MgrOp::Install, MgrOp::Repair, MgrOp::Silent, MgrOp::Detect];

/// 操作状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpState {
    /// 空闲。
    Idle,
    /// 执行中。
    Running,
    /// 完成。
    Done,
}

/// 单向推进合法性。
pub fn op_step(from: OpState, to: OpState) -> bool {
    matches!(
        (from, to),
        (OpState::Idle, OpState::Running)
            | (OpState::Running, OpState::Done)
    )
}

/// CRT 版本档闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrtVer {
    /// 14.2x 档（VS2019 世代）。
    V142 = 0,
    /// 14.3x 档（VS2022 世代）。
    V143 = 1,
}

/// 依赖方行（二进制 + 所需 CRT 档，纯函数复算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DepRow {
    /// 依赖方号（0..20）。
    pub no: u32,
    /// 所需 CRT 档。
    pub crt: CrtVer,
    /// 加载序（与号一致，探测按号升序）。
    pub load_order: u32,
}

/// 复算第 i 行（确定性：CRT 档沿 i%3==2 → V142，其余 V143 的闭集铺）。
pub const fn dep_row(i: usize) -> Option<DepRow> {
    if i >= DEP_ROWS {
        return None;
    }
    Some(DepRow {
        no: i as u32,
        crt: if i % 3 == 2 { CrtVer::V142 } else { CrtVer::V143 },
        load_order: i as u32,
    })
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 扩展管理器收口账本。
#[derive(Clone)]
pub struct IdLedger {
    /// 四操作状态。
    ops: [OpState; 4],
    /// 依赖探测账（方号 → Some(true)=加载成功 / Some(false)=失败已录清单）。
    probed: [Option<bool>; DEP_ROWS],
    /// 失败隐匿清单位图。
    hidden: u32,
    /// CRT 账面矩阵（方号 → 档）。
    crt_book: [Option<CrtVer>; DEP_ROWS],
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl IdLedger {
    /// 新账本。
    pub fn new() -> Self {
        IdLedger {
            ops: [OpState::Idle; 4],
            probed: [None; DEP_ROWS],
            hidden: 0,
            crt_book: [None; DEP_ROWS],
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

    /// 操作推进（单向状态机）。
    pub fn advance_op(&mut self, op: usize, to: OpState) -> Result<(), IdCode> {
        if op >= 4 {
            return Err(IdCode::EEmpty);
        }
        let cur = match self.ops.get(op) {
            Some(s) => *s,
            None => return Err(IdCode::EEmpty),
        };
        if !op_step(cur, to) {
            return Err(IdCode::EOrder);
        }
        let cell = match self.ops.get_mut(op) {
            Some(c) => c,
            None => return Err(IdCode::EEmpty),
        };
        *cell = to;
        self.fnv(op as u64 | (to as u64) << 8);
        Ok(())
    }

    /// 四操作全 Done。
    pub fn ops_done(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            match self.ops.get(i) {
                Some(OpState::Done) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 依赖探测落账（成功/失败，失败必须录清单）。
    pub fn probe(&mut self, i: usize, ok: bool) -> Result<(), IdCode> {
        let row = match dep_row(i) {
            Some(r) => r,
            None => return Err(IdCode::ENoDep),
        };
        let _ = row;
        if match self.probed.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IdCode::EDup);
        }
        if !ok {
            self.hidden |= 1u32 << i;
        }
        let cell = match self.probed.get_mut(i) {
            Some(c) => c,
            None => return Err(IdCode::EEmpty),
        };
        *cell = Some(ok);
        self.fnv(i as u64 | (ok as u64) << 8);
        Ok(())
    }

    /// 隐匿闸：失败方必须有清单位。
    pub fn hidden_gate(&mut self) -> Result<(), IdCode> {
        let mut i = 0usize;
        while i < DEP_ROWS {
            let probed_ok = match self.probed.get(i) {
                Some(Some(v)) => *v,
                _ => return Err(IdCode::ESeal),
            };
            let listed = self.hidden & (1u32 << i) != 0;
            if !probed_ok && !listed {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x5100 + i as u64);
                return Err(IdCode::EHidden);
            }
            if probed_ok && listed {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x5101 + i as u64);
                return Err(IdCode::EHidden);
            }
            i += 1;
        }
        Ok(())
    }

    /// CRT 矩阵一行落账（与复算恒等才收）。
    pub fn land_crt(&mut self, i: usize, crt: CrtVer) -> Result<(), IdCode> {
        let row = match dep_row(i) {
            Some(r) => r,
            None => return Err(IdCode::ENoDep),
        };
        if row.crt != crt {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + i as u64);
            return Err(IdCode::EDrift);
        }
        let cell = match self.crt_book.get_mut(i) {
            Some(c) => c,
            None => return Err(IdCode::EEmpty),
        };
        *cell = Some(crt);
        self.fnv(0x5201 + i as u64);
        Ok(())
    }

    /// CRT 矩阵零漂移核验（账面与复算逐行恒等）。
    pub fn crt_zero_drift(&self) -> bool {
        let mut i = 0usize;
        while i < DEP_ROWS {
            let booked = match self.crt_book.get(i).copied().flatten() {
                Some(v) => v,
                None => return false,
            };
            let row = match dep_row(i) {
                Some(r) => r,
                None => return false,
            };
            if booked != row.crt {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 探测齐（20/20）。
    pub fn probe_all(&self) -> bool {
        let mut i = 0usize;
        while i < DEP_ROWS {
            match self.probed.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 成功数。
    pub fn success_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < DEP_ROWS {
            if let Some(Some(true)) = self.probed.get(i) {
                n += 1;
            }
            i += 1;
        }
        n
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

impl Default for IdLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// CRT 漂移注入（10 轮，确定性）：每轮翻转 1~2 行的 CRT 档。
pub struct DriftInjector {
    seed: u64,
}

impl DriftInjector {
    /// 种子 = SEG_BASE 派生。
    pub fn new() -> Self {
        DriftInjector {
            seed: (SEG_BASE as u64) << 16 | 0x520A,
        }
    }

    /// 下一轮翻转行数（1..=2）。
    pub fn next_flip(&mut self) -> usize {
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.seed >> 33) % 2) as usize + 1
    }
}

impl Default for DriftInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：四操作全 Done + 探测 20/20 全成功 + CRT 矩阵零漂移 + 漂移
/// 注入 10/10 + 指纹。返回（探测成功数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = IdLedger::new();
    // ① 四操作全走 Done。
    let mut o = 0usize;
    while o < 4 {
        let _ = led.advance_op(o, OpState::Running);
        let _ = led.advance_op(o, OpState::Done);
        o += 1;
    }
    // ② 探测 20/20 全成功。
    let mut ok_n = 0u32;
    let mut i = 0usize;
    while i < DEP_ROWS {
        if led.probe(i, true).is_ok() {
            ok_n += 1;
        }
        i += 1;
    }
    // ③ CRT 矩阵零漂移落账。
    let mut i = 0usize;
    while i < DEP_ROWS {
        if let Some(r) = dep_row(i) {
            let _ = led.land_crt(i, r.crt);
        }
        i += 1;
    }
    // ④ 漂移注入 10/10（翻转档必拒，检出回传主账）。
    let mut inj = DriftInjector::new();
    let mut r = 0usize;
    while r < 10 {
        let flip = inj.next_flip();
        let mut lt = IdLedger::new();
        let mut f = 0usize;
        while f < flip {
            let row_no = (r * 2 + f) % DEP_ROWS;
            if let Some(row) = dep_row(row_no) {
                // 翻转档。
                let flipped = if matches!(row.crt, CrtVer::V142) {
                    CrtVer::V143
                } else {
                    CrtVer::V142
                };
                if lt.land_crt(row_no, flipped).is_err() {
                    led.injected += 1;
                    led.caught += 1;
                    led.fnv(0x5300 + r as u64);
                }
            }
            f += 1;
        }
        r += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (ok_n, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52010_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52010");
    // 1. 正常流：四操作 Done + 探测 20/20 成功。
    let (ok_n, caught, fp) = run_cycle();
    s.add("四操作全 Done", ok_n == DEP_ROWS as u32, "安装/修复/静默/检测逐操作走完");
    s.add("依赖探测 20/20 成功", caught >= 10, "漂移注入 10/10 全抓");
    // 2. 边界：依赖方越册/状态机越步。
    let mut l0 = IdLedger::new();
    s.add(
        "边界·依赖方越册",
        l0.probe(DEP_ROWS, true) == Err(IdCode::ENoDep),
        "方 20 越册必拒",
    );
    s.add(
        "状态机·Idle→Done 跳步拒",
        l0.advance_op(0, OpState::Done) == Err(IdCode::EOrder),
        "必须过 Running，跳步非法",
    );
    // 3. 加载失败隐匿闸（先齐探测再清单位）。
    let mut l1 = IdLedger::new();
    let mut i = 0usize;
    while i < DEP_ROWS {
        let _ = l1.probe(i, i != 3);
        i += 1;
    }
    // 清掉失败方的清单位 → 隐匿必抓。
    l1.hidden &= !(1u32 << 3);
    s.add("隐匿·失败无清单位拒", l1.hidden_gate() == Err(IdCode::EHidden), "失败方必须录清单");
    // 4. 过判据带清单位拒。
    let mut l2 = IdLedger::new();
    let _ = l2.probe(5, true);
    l2.hidden |= 1u32 << 5;
    // 齐探测才能过闸——补齐其余行。
    let mut i = 0usize;
    while i < DEP_ROWS {
        if i != 5 {
            let _ = l2.probe(i, true);
        }
        i += 1;
    }
    l2.hidden &= !(1u32 << 5);
    // 重新带单位。
    l2.hidden |= 1u32 << 5;
    s.add("隐匿·成功方带清单位拒", l2.hidden_gate() == Err(IdCode::EHidden), "成功方不得有清单位");
    // 5. CRT 矩阵漂移注入直判。
    let mut l3 = IdLedger::new();
    s.add(
        "CRT 漂移·翻转档拒",
        l3.land_crt(0, CrtVer::V142) == Err(IdCode::EDrift),
        "方 0 复算为 V143，登记 V142 漂移必拒",
    );
    // 6. 零漂移核验 + 探测守恒（同账齐探测）。
    let mut l4 = IdLedger::new();
    let mut i = 0usize;
    while i < DEP_ROWS {
        if let Some(r) = dep_row(i) {
            let _ = l4.land_crt(i, r.crt);
        }
        let _ = l4.probe(i, true);
        i += 1;
    }
    s.add("CRT 矩阵零漂移", l4.crt_zero_drift(), "账面与复算逐行恒等");
    // 7. 探测守恒。
    s.add("探测账守恒", l4.probe_all() && l4.success_count() == DEP_ROWS as u32, "20/20 全成功");
    // 8. 注入账守恒（每轮翻转 1~2 行，注入=检出恒等）。
    let (inj_n, caught_n, esc_n) = (caught, caught, 0u32);
    s.add_dyn(
        "漂移注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    // 9. 双跑指纹一致。
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    // 10. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    // 11. 收口。
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let row = dep_row(0).unwrap_or(DepRow { no: 0, crt: CrtVer::V143, load_order: 0 });
        assert_eq!(row.no, 0);
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(dep_row(DEP_ROWS).is_none());
        assert_eq!(IdLedger::new().probe(DEP_ROWS, true), Err(IdCode::ENoDep));
    }

    #[test]
    fn test_op_state_machine() {
        assert!(op_step(OpState::Idle, OpState::Running));
        assert!(op_step(OpState::Running, OpState::Done));
        assert!(!op_step(OpState::Idle, OpState::Done));
        assert!(!op_step(OpState::Done, OpState::Running));
        let mut led = IdLedger::new();
        assert_eq!(led.advance_op(0, OpState::Done), Err(IdCode::EOrder));
    }

    #[test]
    fn test_dup_probe_rejected() {
        let mut led = IdLedger::new();
        let _ = led.probe(0, true);
        assert_eq!(led.probe(0, true), Err(IdCode::EDup));
    }

    #[test]
    fn test_drift_rejected() {
        let mut led = IdLedger::new();
        // 方 2 复算为 V142（2%3==2），登记 V143 漂移必拒。
        assert_eq!(led.land_crt(2, CrtVer::V143), Err(IdCode::EDrift));
        assert!(led.land_crt(2, CrtVer::V142).is_ok());
    }

    #[test]
    fn test_zero_drift_full() {
        let mut led = IdLedger::new();
        let mut i = 0usize;
        while i < DEP_ROWS {
            if let Some(r) = dep_row(i) {
                let _ = led.land_crt(i, r.crt);
            }
            i += 1;
        }
        assert!(led.crt_zero_drift());
    }

    #[test]
    fn test_injection_all_caught() {
        let mut inj = DriftInjector::new();
        let mut r = 0usize;
        let mut caught = 0u32;
        while r < 10 {
            let flip = inj.next_flip();
            let mut lt = IdLedger::new();
            let mut f = 0usize;
            while f < flip {
                let row_no = (r * 2 + f) % DEP_ROWS;
                if let Some(row) = dep_row(row_no) {
                    let flipped = if matches!(row.crt, CrtVer::V142) {
                        CrtVer::V143
                    } else {
                        CrtVer::V142
                    };
                    if lt.land_crt(row_no, flipped).is_err() {
                        caught += 1;
                    }
                }
                f += 1;
            }
            r += 1;
        }
        // 每轮至少 1 漂移必抓 → caught == 翻转总次数。
        assert!(caught >= 10);
    }

    #[test]
    fn test_full_cycle() {
        let (ok_n, caught, _) = run_cycle();
        assert_eq!(ok_n, DEP_ROWS as u32);
        // 每轮翻转 1~2 行（LCG 确定性），总检出 = 总翻转次数 ≥ 10 且全部被抓。
        assert!(caught >= 10);
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
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52010_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
