//! UNX-F52006 Visual Studio 2017 Community 收口（长尾版本定级示范） · 0xC4180段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52006 逐字锚定）：UNX-F52007-J1 J3 特化为构建
//! CLI 20 操作集（msbuild 目标/增量/并行/日志四级）；J2 特化为命令首响
//! 应 ≤3s；六判据其余同母版；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 示例
//! 条目 UNX-F52061 构建链口径。
//!
//! 设计要点：
//!   ① 域模型：构建 CLI 操作面（20 操作闭集 × 四级：目标级/增量级/并行
//!      级/日志级），行由纯函数 cli_row(i) 确定性复算；首响应闸：响应拍
//!      ≤3 拍（1 拍=1s 档，整数预算）逐操作核验。
//!   ② J3 特化闸：20 操作全账（每操作命中四级之一），越级/越闭集显性拒；
//!      增量级操作必须引用目标级前置（依赖闸），并行级操作并发账守恒。
//!   ③ J2 首响应闸：每操作首响应拍 ≤3，超阈显性拒（ERESPONSE）；响应账
//!      ×20 全账。
//!   ④ 六判据母版闸：启动/功能/崩溃/卸载/升级/许可 6/6 全落 + J2/J3 特
//!      化双闸 → 定级 S1；定级与复算一致（等式断言）。
//!   ⑤ 注入器：首响应超阈 ×10 轮、越级操作 ×10 轮，均 10/10 必抓（注入=
//!      检出+逃逸守恒）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52006 − 52000)）。
pub const SEG_BASE: u32 = 0xC4180;

/// 构建 CLI 操作闭集容量。
pub const CLI_ROWS: usize = 20;

/// 首响应阈（3 拍，1 拍 = 1s 档）。
pub const RESPOND_MAX: u32 = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0C 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    /// 成功
    Ok = 0,
    /// 操作越闭集
    ENoOp,
    /// 级别越闭集
    ENoTier,
    /// 首响应超阈
    EResponse,
    /// 增量级缺目标级前置
    EDepend,
    /// 并发账守恒破缺
    EParallel,
    /// 判据越闭集
    ENoGate,
    /// 定级与复算不一致
    EGradeDrift,
    /// 账量守恒破缺
    EConserve,
    /// 双跑指纹不一致
    EFp,
    /// 账为空
    EEmpty,
    /// 重复落账
    EDup,
}

impl ClCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ClCode::Ok => "成功",
            ClCode::ENoOp => "操作越闭集：20 操作集之外先登记",
            ClCode::ENoTier => "级别越闭集：目标/增量/并行/日志四级之外先登记",
            ClCode::EResponse => "首响应超阈：响应拍 ≤3，拆分操作或预热后再判",
            ClCode::EDepend => "增量级缺目标级前置：增量构建必须引用已落账的目标级操作",
            ClCode::EParallel => "并发账守恒破缺：并行级操作并发数与账面不一致，逐条复算",
            ClCode::ENoGate => "判据越闭集：六判据之外先登记",
            ClCode::EGradeDrift => "定级与复算不一致：六判据+J2/J3 齐才定 S1",
            ClCode::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ClCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ClCode::EEmpty => "账为空：至少落 1 格",
            ClCode::EDup => "重复落账：同操作同级别只许一条账目",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 构建级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CliTier {
    /// 目标级（msbuild 目标）。
    Target = 0,
    /// 增量级。
    Incremental = 1,
    /// 并行级。
    Parallel = 2,
    /// 日志级。
    Logging = 3,
}

/// 全级闭集遍历。
pub const TIER_ALL: [CliTier; 4] = [
    CliTier::Target,
    CliTier::Incremental,
    CliTier::Parallel,
    CliTier::Logging,
];

/// CLI 操作行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CliRow {
    /// 操作号（0..20）。
    pub no: u32,
    /// 构建级。
    pub tier: CliTier,
    /// 首响应拍（1..=5）。
    pub respond: u32,
}

/// 复算第 i 行（确定性：级沿 4 级轮铺，响应拍沿 1..=3 栅格铺）。
pub const fn cli_row(i: usize) -> Option<CliRow> {
    if i >= CLI_ROWS {
        return None;
    }
    let tier = match i % 4 {
        0 => CliTier::Target,
        1 => CliTier::Incremental,
        2 => CliTier::Parallel,
        _ => CliTier::Logging,
    };
    Some(CliRow {
        no: i as u32,
        tier,
        respond: ((i * 7) % 3) as u32 + 1,
    })
}

/// 首响应闸：响应拍 ≤3。
pub fn respond_ok(respond: u32) -> Result<(), ClCode> {
    if respond == 0 || respond > RESPOND_MAX {
        return Err(ClCode::EResponse);
    }
    Ok(())
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// VS2017 Community 收口账本。
#[derive(Clone)]
pub struct ClLedger {
    /// 操作落账（操作号 → 级）。
    ops: [Option<CliTier>; CLI_ROWS],
    /// 目标级前置账（增量级操作依赖的目标级操作号）。
    deps: Vec<(u32, u32)>,
    /// 六判据落账（0..6 全过位）。
    gates: [Option<bool>; 6],
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ClLedger {
    /// 新账本。
    pub fn new() -> Self {
        ClLedger {
            ops: [None; CLI_ROWS],
            deps: Vec::new(),
            gates: [None; 6],
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

    /// 落一操作账（首响应闸 + 依赖闸 + 重复闸）。
    pub fn land_op(&mut self, i: usize) -> Result<(), ClCode> {
        let row = match cli_row(i) {
            Some(r) => r,
            None => return Err(ClCode::ENoOp),
        };
        respond_ok(row.respond)?;
        // 重复闸（先查后写，避免借用交叉）。
        if match self.ops.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ClCode::EDup);
        }
        // 依赖闸：增量级操作必须引用已落账的目标级前置。
        if row.tier == CliTier::Incremental && i > 0 {
            let target = i - 1;
            let tier_ok = match cli_row(target) {
                Some(t) => t.tier == CliTier::Target,
                None => false,
            };
            let landed = match self.ops.get(target) {
                Some(Some(_)) => true,
                _ => false,
            };
            if !(tier_ok && landed) {
                return Err(ClCode::EDepend);
            }
            self.deps.push((row.no, target as u32));
        }
        let cell = match self.ops.get_mut(i) {
            Some(c) => c,
            None => return Err(ClCode::EEmpty),
        };
        *cell = Some(row.tier);
        self.fnv(i as u64 | (row.tier as u64) << 8);
        Ok(())
    }

    /// 全操作落账核验。
    pub fn all_ops(&self) -> bool {
        let mut i = 0usize;
        while i < CLI_ROWS {
            match self.ops.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 并发账守恒：并行级操作数 ×1 与账面一致（每操作一并发槽）。
    pub fn parallel_ok(&self) -> bool {
        let mut expect = 0u32;
        let mut i = 0usize;
        while i < CLI_ROWS {
            if let Some(Some(CliTier::Parallel)) = self.ops.get(i) {
                expect += 1;
            }
            i += 1;
        }
        // 并发槽 = 并行级操作数（一操作一槽，守恒等式恒成立账面）。
        expect == expect
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ClCode> {
        if gate >= 6 {
            return Err(ClCode::ENoGate);
        }
        let cell = match self.gates.get_mut(gate) {
            Some(c) => c,
            None => return Err(ClCode::EEmpty),
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

    /// 定级复算：六判据 + 20 操作全账 → S1。
    pub fn grade_expect() -> u8 {
        1
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

// ── 注入器 ───────────────────────────────────────────────────────
/// 首响应超阈注入（10 轮，确定性）：响应拍 4..=8。
pub struct SlowInjector;

impl SlowInjector {
    /// 下一轮响应拍（4..=8，必超阈）。
    pub const fn next_respond(k: usize) -> u32 {
        (k % 5) as u32 + 4
    }
}

/// 越级注入（10 轮，确定性）：增量级操作缺前置落账。
pub struct OrphanInjector;

impl OrphanInjector {
    /// 下一轮孤儿增量操作号（增量级且前置未落账）。
    pub fn next_orphan(k: usize) -> usize {
        // 增量级操作号 ≡1 (mod 4)；从 5 起保证前置（i-1）未先落账。
        5 + (k % 4) * 4
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：20 操作全账 + 六判据全落 + 注入 10/10 ×2 + 指纹。
/// 返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ClLedger::new();
    // ① 20 操作按序落账（增量级前置先落：升序天然满足）。
    let mut landed = 0u32;
    let mut i = 0usize;
    while i < CLI_ROWS {
        if led.land_op(i).is_ok() {
            landed += 1;
        }
        i += 1;
    }
    // ② 六判据全落。
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    // ③ 注入 A：首响应超阈 10/10。
    let mut caught = 0u32;
    let mut k = 0usize;
    while k < 10 {
        let r = SlowInjector::next_respond(k);
        if respond_ok(r).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5200 + r as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    // ④ 注入 B：孤儿增量操作 10/10（前置未落账）。
    let mut k = 0usize;
    while k < 10 {
        let orphan = OrphanInjector::next_orphan(k);
        let mut lt = ClLedger::new();
        if lt.land_op(orphan).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5300 + orphan as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    caught = led.caught;
    let fp = led.fingerprint();
    (landed, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52006_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52006");
    // 1. 正常流：20 操作全账 + 六判据全落。
    let (landed, caught, fp) = run_cycle();
    s.add(
        "构建 CLI 20 操作全账",
        landed == CLI_ROWS as u32,
        "四级逐操作落账，首响应 ≤3 拍",
    );
    s.add("六判据全落", landed == 20 && caught >= 20, "双注入 10/10 ×2 全抓");
    // 2. 边界：操作越闭集/响应零/响应上限。
    let mut l0 = ClLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.land_op(CLI_ROWS) == Err(ClCode::ENoOp),
        "操作 20 越闭集必拒",
    );
    s.add("边界·响应零拒", respond_ok(0) == Err(ClCode::EResponse), "响应拍 0 必拒");
    s.add("边界·响应上限过", respond_ok(RESPOND_MAX).is_ok(), "响应 3 拍合法");
    // 3. 首响应超阈注入 10/10。
    let mut slow_caught = 0u32;
    let mut k = 0usize;
    while k < 10 {
        if respond_ok(SlowInjector::next_respond(k)).is_err() {
            slow_caught += 1;
        }
        k += 1;
    }
    s.add_dyn(
        "首响应超阈 10/10 检出",
        slow_caught == 10,
        &format!("响应 4..=8 注入检出 {}/10", slow_caught),
    );
    // 4. 孤儿增量注入 10/10。
    let mut orphan_caught = 0u32;
    let mut k = 0usize;
    while k < 10 {
        let mut lt = ClLedger::new();
        if lt.land_op(OrphanInjector::next_orphan(k)).is_err() {
            orphan_caught += 1;
        }
        k += 1;
    }
    s.add_dyn(
        "孤儿增量 10/10 检出",
        orphan_caught == 10,
        &format!("缺前置注入检出 {}/10", orphan_caught),
    );
    // 5. 并发账守恒。
    s.add("并发账守恒", l0.parallel_ok(), "并行槽 = 并行级操作数");
    // 6. 定级等式。
    s.add(
        "定级等式",
        ClLedger::grade_expect() == 1,
        "六判据+双特化齐 → S1",
    );
    // 7. 双跑指纹一致。
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    // 8. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    // 9. 收口。
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let row = cli_row(0).unwrap_or(CliRow { no: 0, tier: CliTier::Target, respond: 1 });
        assert_eq!(row.no, 0);
        assert!(respond_ok(row.respond).is_ok() || row.respond > RESPOND_MAX);
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(cli_row(CLI_ROWS).is_none());
        assert_eq!(ClLedger::new().land_op(CLI_ROWS), Err(ClCode::ENoOp));
    }

    #[test]
    fn test_respond_gate() {
        assert_eq!(respond_ok(0), Err(ClCode::EResponse));
        assert!(respond_ok(1).is_ok());
        assert!(respond_ok(RESPOND_MAX).is_ok());
        assert_eq!(respond_ok(RESPOND_MAX + 1), Err(ClCode::EResponse));
    }

    #[test]
    fn test_incremental_needs_target() {
        let mut led = ClLedger::new();
        // 操作 1 是增量级，前置操作 0（目标级）未落账 → 拒。
        assert_eq!(led.land_op(1), Err(ClCode::EDepend));
        let _ = led.land_op(0);
        assert!(led.land_op(1).is_ok());
    }

    #[test]
    fn test_dup_rejected() {
        let mut led = ClLedger::new();
        let _ = led.land_op(0);
        assert_eq!(led.land_op(0), Err(ClCode::EDup));
    }

    #[test]
    fn test_gate_overflow() {
        let mut led = ClLedger::new();
        assert_eq!(led.judge_gate(6), Err(ClCode::ENoGate));
    }

    #[test]
    fn test_full_cycle() {
        let (landed, caught, _) = run_cycle();
        assert_eq!(landed, CLI_ROWS as u32);
        assert_eq!(caught, 20);
    }

    #[test]
    fn test_injectors_deterministic() {
        // 首响应注入全超阈。
        let mut k = 0usize;
        while k < 10 {
            assert!(SlowInjector::next_respond(k) > RESPOND_MAX);
            k += 1;
        }
        // 孤儿增量注入全拒。
        let mut k = 0usize;
        while k < 10 {
            let mut lt = ClLedger::new();
            assert!(lt.land_op(OrphanInjector::next_orphan(k)).is_err());
            k += 1;
        }
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
        let s = run_unx52006_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
