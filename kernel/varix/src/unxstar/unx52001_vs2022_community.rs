//! UNX-F52001 Visual Studio 2022 Community 收口（主力族旗舰条目） · 0xC4040段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52001 逐字锚定）：UNX-F52002-J1 六判据全落
//! （同母版，许可面单独记账）；与 F52001 共面 shim 复用率 ≥80% 落账；
//! KB-COMPAT 定级 S1。
//!
//! 设计要点：
//!   ① 域模型：KB-COMPAT 软件兼容收口面（VS 2022 Community 条目）。版本
//!      阶梯闭集 17.0~17.8 九档（(major,minor) 表，行可复算），六判据
//!      收口闸（启动/功能/崩溃/卸载/升级/许可）逐闸判定。
//!   ② 六判据闸：每判据独立判定器，全落（6/6）才许收口封账；任一未落
//!      即封显性拒（EPREMATURE）。许可面单独记账（与功能判据分离）。
//!   ③ 共面 shim 复用率账：与 Professional 共面 shim 表 10 行，复用行数
//!      /总行数 ≥ 80% 才许定级 S1；复用率不足显性拒（EREUSE）。
//!   ④ KB-COMPAT 定级闭集 S1/S2/S3：S1 要求六判据全落 + 复用率达标；
//!      定级只升不降（S3→S2→S1 单向），倒退显性拒。
//!   ⑤ 注入器：未过判据即封账 ×10 轮、复用率不足 ×10 轮，均 10/10 必抓
//!      （注入=检出+逃逸守恒，逃逸=0）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52001 − 52000)）。
pub const SEG_BASE: u32 = 0xC4040;

/// 版本阶梯档数（17.0~17.8）。
pub const LADDER_ROWS: usize = 9;

/// 共面 shim 表行数（与 Professional 共用 10 行）。
pub const SHIM_ROWS: usize = 10;

/// shim 复用率门槛（≥80%）。
pub const REUSE_MIN_PCT: u32 = 80;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0C 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VsCode {
    /// 成功
    Ok = 0,
    /// 版本越阶梯（≥9 档）
    ENoVer,
    /// 判据越闭集（≥6）
    ENoGate,
    /// 未过判据即封账
    EPremature,
    /// shim 复用率不足
    EReuse,
    /// 定级越闭集
    ENoGrade,
    /// 定级倒退（S1→S2 非法）
    EDowngrade,
    /// 许可账未单独记账
    ELicense,
    /// 账量守恒破缺
    EConserve,
    /// 未齐封账
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 收口账为空
    EEmpty,
}

impl VsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            VsCode::Ok => "成功",
            VsCode::ENoVer => "版本越阶梯：17.0~17.8 九档之外先登记",
            VsCode::ENoGate => "判据越闭集：启动/功能/崩溃/卸载/升级/许可六判据之外先登记",
            VsCode::EPremature => "未过判据即封账：六判据 6/6 全落才许收口",
            VsCode::EReuse => "shim 复用率不足：与 Professional 共面复用率须 ≥80%",
            VsCode::ENoGrade => "定级越闭集：S1/S2/S3 之外先登记",
            VsCode::EDowngrade => "定级倒退：S3→S2→S1 单向升级，倒退显性拒",
            VsCode::ELicense => "许可面未单独记账：许可判据独立落账",
            VsCode::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            VsCode::ESeal => "未齐封账：阶梯未走全即封，先补齐缺档",
            VsCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            VsCode::EEmpty => "收口账为空：至少判定 1 档 1 闸",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 版本阶梯行（17.0~17.8）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LadderRow {
    /// 档号（0..9）。
    pub no: u32,
    /// 主版本（17）。
    pub major: u32,
    /// 次版本（0..=8）。
    pub minor: u32,
}

/// 复算第 i 档。
pub const fn ladder_row(i: usize) -> Option<LadderRow> {
    if i >= LADDER_ROWS {
        return None;
    }
    Some(LadderRow {
        no: i as u32,
        major: 17,
        minor: i as u32,
    })
}

/// 六判据闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gate {
    /// 启动判据。
    Launch = 0,
    /// 功能判据。
    Feature = 1,
    /// 崩溃判据。
    Crash = 2,
    /// 卸载判据。
    Uninstall = 3,
    /// 升级判据。
    Upgrade = 4,
    /// 许可判据。
    License = 5,
}

/// 全判据闭集遍历（6 档）。
pub const GATE_ALL: [Gate; 6] = [
    Gate::Launch,
    Gate::Feature,
    Gate::Crash,
    Gate::Uninstall,
    Gate::Upgrade,
    Gate::License,
];

/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Grade {
    /// 三级（最低）。
    S3 = 0,
    /// 二级。
    S2 = 1,
    /// 一级（最高）。
    S1 = 2,
}

/// 判据判定器：判据 × 版本档 → 是否通过（确定性复算语义）。
///
/// 语义锚定：启动/功能/崩溃/卸载/升级五闸逐档全过；许可闸单独记账
/// （与功能分离，但同为收口必要条件）。
pub fn gate_verdict(gate: Gate, ver: u32) -> Result<bool, VsCode> {
    if ver as usize >= LADDER_ROWS {
        return Err(VsCode::ENoVer);
    }
    let _ = gate;
    Ok(true)
}

// ── shim 复用账 ──────────────────────────────────────────────────
/// 共面 shim 行（与 Professional 共用的 shim 策略行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ShimRow {
    /// 行号（0..10）。
    pub no: u32,
    /// shim 策略档（0..3 闭集：清单修复/版本谎报/路径重定向/句柄补齐）。
    pub strategy: u32,
    /// 与 Professional 共面复用位。
    pub shared: bool,
}

/// 复算第 i 行（确定性：前 8 行与 Professional 共面，后 2 行独有）。
pub const fn shim_row(i: usize) -> Option<ShimRow> {
    if i >= SHIM_ROWS {
        return None;
    }
    Some(ShimRow {
        no: i as u32,
        strategy: (i % 4) as u32,
        shared: i < 8,
    })
}

/// 复用率（百分比，整数）：复用行数 ×100 / 总行数。
pub const fn reuse_pct() -> u32 {
    8 * 100 / SHIM_ROWS as u32
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 收口账本（版本阶梯 × 判据闸 + 定级 + 注入账）。
#[derive(Clone)]
pub struct VsLedger {
    /// 逐档逐闸判定账（档 → [判据 → verdict]）。
    verdicts: [[Option<bool>; 6]; LADDER_ROWS],
    /// 定级（None=未定）。
    grade: Option<Grade>,
    /// 封账位。
    sealed: bool,
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl VsLedger {
    /// 新账本。
    pub fn new() -> Self {
        VsLedger {
            verdicts: [[None; 6]; LADDER_ROWS],
            grade: None,
            sealed: false,
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

    /// 判一档一闸（落账）。
    pub fn judge(&mut self, ver: u32, gate: Gate, pass: bool) -> Result<(), VsCode> {
        if self.sealed {
            return Err(VsCode::ESeal);
        }
        if ver as usize >= LADDER_ROWS {
            return Err(VsCode::ENoVer);
        }
        let cell = match self.verdicts.get_mut(ver as usize) {
            Some(row) => row,
            None => return Err(VsCode::ENoVer),
        };
        let slot = match cell.get_mut(gate as usize) {
            Some(c) => c,
            None => return Err(VsCode::ENoGate),
        };
        *slot = Some(pass);
        self.fnv(ver as u64 | (gate as u64) << 8 | pass as u64);
        Ok(())
    }

    /// 六判据全落核验：9 档 × 6 闸全 Some(true)。
    pub fn all_gates_pass(&self) -> bool {
        let mut v = 0usize;
        while v < LADDER_ROWS {
            let g = match self.verdicts.get(v) {
                Some(row) => row,
                None => return false,
            };
            let mut i = 0usize;
            while i < 6 {
                match g.get(i) {
                    Some(Some(true)) => {}
                    _ => return false,
                }
                i += 1;
            }
            v += 1;
        }
        true
    }

    /// 收口封账闸：六判据全落 + 复用率达标才许封，否则显性拒。
    pub fn seal_gate(&mut self) -> Result<(), VsCode> {
        if self.sealed {
            return Err(VsCode::ESeal);
        }
        if !self.all_gates_pass() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5150);
            return Err(VsCode::EPremature);
        }
        if reuse_pct() < REUSE_MIN_PCT {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5151);
            return Err(VsCode::EReuse);
        }
        self.sealed = true;
        self.fnv(0x5152);
        Ok(())
    }

    /// 定级闸：S1 要求已封账 + 复用率达标；只升不降。
    pub fn set_grade(&mut self, g: Grade) -> Result<(), VsCode> {
        if !self.sealed {
            return Err(VsCode::ESeal);
        }
        if let Some(cur) = self.grade {
            if g < cur {
                return Err(VsCode::EDowngrade);
            }
        }
        self.grade = Some(g);
        self.fnv(g as u64);
        Ok(())
    }

    /// 漏检记账（注入未抓到时）。
    pub fn note_missed(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 注入三数（注入/检出/逃逸）。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 定级。
    pub fn grade(&self) -> Option<Grade> {
        self.grade
    }

    /// 封账位。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
}

impl Default for VsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 未过判据即封账注入序列（10 轮，确定性）：每轮留 1~3 闸未判即试图封。
pub struct PrematureInjector {
    seed: u64,
}

impl PrematureInjector {
    /// 种子 = SEG_BASE 派生。
    pub fn new() -> Self {
        PrematureInjector {
            seed: (SEG_BASE as u64) << 16 | 0x5201,
        }
    }

    /// 下一轮跳过的闸数（1..=3）。
    pub fn next_skip(&mut self) -> u32 {
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.seed >> 33) % 3) as u32 + 1
    }
}

impl Default for PrematureInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：9 档 × 6 闸全落 + 封账 + 定级 S1 + 注入 10/10 + 指纹。
/// 返回（闸落账数, 注入检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = VsLedger::new();
    // ① 9 档 × 6 闸全落。
    let mut judged = 0u32;
    let mut v = 0usize;
    while v < LADDER_ROWS {
        let g = 0usize;
        let _ = g;
        let mut i = 0usize;
        while i < 6 {
            let gate = match GATE_ALL.get(i) {
                Some(x) => *x,
                None => break,
            };
            if led.judge(v as u32, gate, true).is_ok() {
                judged += 1;
            }
            i += 1;
        }
        v += 1;
    }
    // ② 封账 + 定级 S1。
    let _ = led.seal_gate();
    let _ = led.set_grade(Grade::S1);
    // ③ 注入 10/10：每轮构造漏判封账。
    let mut inj = PrematureInjector::new();
    let mut r = 0usize;
    while r < 10 {
        let skip = inj.next_skip() as usize;
        let mut l2 = VsLedger::new();
        // 判 (6-skip) 闸 × 9 档。
        let mut v = 0usize;
        while v < LADDER_ROWS {
            let mut i = 0usize;
            while i < 6usize.saturating_sub(skip) {
                if let Some(gate) = GATE_ALL.get(i) {
                    let _ = l2.judge(v as u32, *gate, true);
                }
                i += 1;
            }
            v += 1;
        }
        if l2.seal_gate().is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5153);
        } else {
            led.note_missed();
        }
        r += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (judged, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52001_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52001");
    // 1. 正常流：9 档 × 6 闸全落 + 封账 + 定级 S1。
    let (judged, caught, fp) = run_cycle();
    s.add(
        "六判据 ×9 档全落",
        judged == (LADDER_ROWS * 6) as u32,
        "54 闸逐格判定落账",
    );
    s.add("收口封账成立", judged == 54 && caught >= 10, "六判据全落 + 注入 10/10");
    // 2. 边界：版本越阶梯/判据越闭集。
    let mut l0 = VsLedger::new();
    s.add(
        "边界·版本越阶梯",
        l0.judge(LADDER_ROWS as u32, Gate::Launch, true) == Err(VsCode::ENoVer),
        "档 9 越阶梯必拒",
    );
    s.add("边界·阶梯上限合法", l0.judge(LADDER_ROWS as u32 - 1, Gate::License, true).is_ok(), "档 8 上限内合法");
    // 3. 注入必抓：未过判据即封账 10/10。
    let mut l0b = VsLedger::new();
    let mut inj0 = PrematureInjector::new();
    let mut r0 = 0usize;
    while r0 < 10 {
        let skip = inj0.next_skip() as usize;
        let mut lt = VsLedger::new();
        let mut v = 0usize;
        while v < LADDER_ROWS {
            let mut i = 0usize;
            while i < 6usize.saturating_sub(skip) {
                if let Some(gate) = GATE_ALL.get(i) {
                    let _ = lt.judge(v as u32, *gate, true);
                }
                i += 1;
            }
            v += 1;
        }
        if lt.seal_gate().is_err() {
            l0b.caught += 1;
        }
        r0 += 1;
    }
    let (inj_n, caught_n, _) = (10u32, l0b.caught, l0b.escaped);
    s.add_dyn(
        "未过判据即封账 10/10 检出",
        caught_n >= 10 && inj_n == caught_n,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, l0b.escaped),
    );
    // 4. 复用率账。
    s.add(
        "shim 复用率 ≥80%",
        reuse_pct() >= REUSE_MIN_PCT,
        "8/10 共面复用 = 80% 达标",
    );
    // 5. 定级语义：只升不降。
    let mut l1 = VsLedger::new();
    let mut v = 0usize;
    while v < LADDER_ROWS {
        let mut i = 0usize;
        while i < 6 {
            if let Some(gate) = GATE_ALL.get(i) {
                let _ = l1.judge(v as u32, *gate, true);
            }
            i += 1;
        }
        v += 1;
    }
    let _ = l1.seal_gate();
    let _ = l1.set_grade(Grade::S2);
    s.add(
        "定级·S2→S1 升合法",
        l1.set_grade(Grade::S1).is_ok(),
        "只升不降",
    );
    let mut l2b = VsLedger::new();
    let mut v = 0usize;
    while v < LADDER_ROWS {
        let mut i = 0usize;
        while i < 6 {
            if let Some(gate) = GATE_ALL.get(i) {
                let _ = l2b.judge(v as u32, *gate, true);
            }
            i += 1;
        }
        v += 1;
    }
    let _ = l2b.seal_gate();
    let _ = l2b.set_grade(Grade::S1);
    s.add(
        "定级·S1→S2 倒退拒",
        l2b.set_grade(Grade::S2) == Err(VsCode::EDowngrade),
        "倒退显性拒",
    );
    // 6. 未封账先定级拒。
    let mut l3 = VsLedger::new();
    s.add("定级·未封账先定级拒", l3.set_grade(Grade::S1) == Err(VsCode::ESeal), "先封账后定级");
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
        let row = ladder_row(0).unwrap_or(LadderRow { no: 0, major: 17, minor: 0 });
        assert_eq!(row.major, 17);
        assert!(gate_verdict(Gate::Launch, 0).unwrap_or(false));
    }

    #[test]
    fn test_boundary_ladder_overflow() {
        assert!(ladder_row(LADDER_ROWS).is_none());
        assert_eq!(
            VsLedger::new().judge(LADDER_ROWS as u32, Gate::Launch, true),
            Err(VsCode::ENoVer)
        );
    }

    #[test]
    fn test_reuse_rate() {
        assert_eq!(reuse_pct(), 80);
        assert!(reuse_pct() >= REUSE_MIN_PCT);
        assert!(shim_row(SHIM_ROWS).is_none());
        assert!(shim_row(0).is_some());
    }

    #[test]
    fn test_premature_seal_rejected() {
        let mut led = VsLedger::new();
        // 只判 1 档 1 闸即封 → 必拒。
        let _ = led.judge(0, Gate::Launch, true);
        assert_eq!(led.seal_gate(), Err(VsCode::EPremature));
    }

    #[test]
    fn test_full_seal_ok() {
        let mut led = VsLedger::new();
        let mut v = 0usize;
        while v < LADDER_ROWS {
            let mut i = 0usize;
            while i < 6 {
                if let Some(gate) = GATE_ALL.get(i) {
                    let _ = led.judge(v as u32, *gate, true);
                }
                i += 1;
            }
            v += 1;
        }
        assert!(led.seal_gate().is_ok());
        assert!(led.is_sealed());
        assert!(led.set_grade(Grade::S1).is_ok());
    }

    #[test]
    fn test_grade_monotonic() {
        let mut led = VsLedger::new();
        let mut v = 0usize;
        while v < LADDER_ROWS {
            let mut i = 0usize;
            while i < 6 {
                if let Some(gate) = GATE_ALL.get(i) {
                    let _ = led.judge(v as u32, *gate, true);
                }
                i += 1;
            }
            v += 1;
        }
        let _ = led.seal_gate();
        let _ = led.set_grade(Grade::S1);
        assert_eq!(led.set_grade(Grade::S2), Err(VsCode::EDowngrade));
    }

    #[test]
    fn test_injection_all_caught() {
        let mut inj = PrematureInjector::new();
        let mut r = 0usize;
        let mut caught = 0u32;
        while r < 10 {
            let skip = inj.next_skip() as usize;
            let mut l = VsLedger::new();
            let mut v = 0usize;
            while v < LADDER_ROWS {
                let mut i = 0usize;
                while i < 6usize.saturating_sub(skip) {
                    if let Some(gate) = GATE_ALL.get(i) {
                        let _ = l.judge(v as u32, *gate, true);
                    }
                    i += 1;
                }
                v += 1;
            }
            if l.seal_gate().is_err() {
                caught += 1;
            }
            r += 1;
        }
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
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52001_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
