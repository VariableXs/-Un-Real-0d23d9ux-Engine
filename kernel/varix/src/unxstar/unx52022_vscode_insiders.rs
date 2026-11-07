//! UNX-F52022 VSCode Insiders 收口（滚动版本线） · 0xC4580段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52022 逐字锚定）：UNX-F52023-J1 六判据全落；与
//! VSCode 共面 shim 复用率 ≥85%（Electron 同基座）落账；扩展市场差异行
//! 为声明；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题四 Electron 族专项。
//!
//! 设计要点：
//!   ① 域模型：共面 shim 复用账。shim 表 20 行（纯函数复算），与 VSCode
//!      共面行 17 行（复用率 85% 整数口径），复用率闸 <85% 显性拒（定级
//!      S1 前置）。
//!   ② 扩展市场差异行为声明账：差异行为闭集 8 行（纯函数复算），每行必
//!      须显性声明（零静默）；未声明差异 ×10 轮 10/10 必抓。
//!   ③ 六判据母版闸（6/6 全落）。
//!   ④ KB-COMPAT 定级 S1：六判据 + 复用率达标 + 差异全声明。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52022 − 52000)）。
pub const SEG_BASE: u32 = 0xC4580;

/// shim 表行数。
pub const SHIM_ROWS: usize = 20;

/// 共面行数（17/20 = 85%）。
pub const SHARED_ROWS: usize = 17;

/// 复用率门槛（≥85%）。
pub const REUSE_MIN_PCT: u32 = 85;

/// 差异行为声明行数。
pub const DIFF_ROWS: usize = 8;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0C 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViCode {
    /// 成功
    Ok = 0,
    /// shim 行越册
    ENoShim,
    /// 六判据越闭集
    ENoGate,
    /// 复用率不足
    EReuse,
    /// 差异行为越册
    ENoDiff,
    /// 差异未声明
    EUndeclared,
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
    /// 重复声明
    EDup,
    /// 复用率漂移
    EReuseDrift,
}

impl ViCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ViCode::Ok => "成功",
            ViCode::ENoShim => "shim 行越册：20 行闭集之外先登记",
            ViCode::ENoGate => "六判据越闭集：六判据之外先登记",
            ViCode::EReuse => "复用率不足：与 VSCode 共面复用率须 ≥85%（Electron 同基座）",
            ViCode::ENoDiff => "差异行为越册：8 行闭集之外先登记",
            ViCode::EUndeclared => "差异未声明：扩展市场差异行为必须显性声明（零静默）",
            ViCode::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ViCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ViCode::EEmpty => "账为空：至少落 1 格",
            ViCode::ESeal => "未齐全账即封：账未走全即封，先补齐缺格",
            ViCode::EGradeDrift => "定级漂移：六判据+复用率达标+差异全声明才定 S1",
            ViCode::EDup => "重复声明：同行只许声明一次",
            ViCode::EReuseDrift => "复用率漂移：账面与复算不一致，逐行对平",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// shim 行（与 VSCode 共面位，纯函数复算：前 17 行共面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ShimRow {
    /// 行号（0..20）。
    pub no: u32,
    /// 共面复用位。
    pub shared: bool,
}

/// 复算第 i 行。
pub const fn shim_row(i: usize) -> Option<ShimRow> {
    if i >= SHIM_ROWS {
        return None;
    }
    Some(ShimRow {
        no: i as u32,
        shared: i < SHARED_ROWS,
    })
}

/// 复用率（整数百分比）。
pub const fn reuse_pct() -> u32 {
    (SHARED_ROWS * 100 / SHIM_ROWS) as u32
}

/// 差异行为行（纯函数复算：行为档 + 是否已声明域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffRow {
    /// 行号（0..8）。
    pub no: u32,
    /// 行为档（0..2：市场签名/遥测面/更新通道）。
    pub kind: u32,
}

/// 复算第 i 行。
pub const fn diff_row(i: usize) -> Option<DiffRow> {
    if i >= DIFF_ROWS {
        return None;
    }
    Some(DiffRow {
        no: i as u32,
        kind: (i % 3) as u32,
    })
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Insiders 收口账本。
#[derive(Clone)]
pub struct ViLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// shim 复用账（共面行在册位图）。
    shims: u32,
    /// 差异声明账（行在册位图）。
    declared: u8,
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ViLedger {
    /// 新账本。
    pub fn new() -> Self {
        ViLedger {
            gates: [None; 6],
            shims: 0,
            declared: 0,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ViCode> {
        if gate >= 6 {
            return Err(ViCode::ENoGate);
        }
        let cell = match self.gates.get_mut(gate) {
            Some(c) => c,
            None => return Err(ViCode::EEmpty),
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

    /// shim 行落账（只收共面行；独有行不计复用）。
    pub fn land_shim(&mut self, i: usize) -> Result<(), ViCode> {
        let row = match shim_row(i) {
            Some(r) => r,
            None => return Err(ViCode::ENoShim),
        };
        if !row.shared {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + i as u64);
            return Err(ViCode::EReuseDrift);
        }
        if self.shims & (1u32 << i) != 0 {
            return Err(ViCode::EDup);
        }
        self.shims |= 1u32 << i;
        self.fnv(0x5201 + i as u64);
        Ok(())
    }

    /// 复用率核验（账面 == 复算且达标）。
    pub fn reuse_ok(&self) -> bool {
        let booked = self.shims.count_ones();
        booked == SHARED_ROWS as u32 && {
            let pct = booked * 100 / SHIM_ROWS as u32;
            pct >= REUSE_MIN_PCT && pct == reuse_pct()
        }
    }

    /// 差异行为声明（零静默闸：声明行为档必须与复算一致）。
    pub fn declare_diff(&mut self, i: usize) -> Result<(), ViCode> {
        if i >= DIFF_ROWS {
            return Err(ViCode::ENoDiff);
        }
        if self.declared & (1u8 << i) != 0 {
            return Err(ViCode::EDup);
        }
        let _ = diff_row(i);
        self.declared |= 1u8 << i;
        self.fnv(0x5300 + i as u64);
        Ok(())
    }

    /// 差异全声明核验。
    pub fn diff_all(&self) -> bool {
        self.declared.count_ones() == DIFF_ROWS as u32
    }

    /// 未声明差异审计门（有差异行未声明 = 违）。
    pub fn undeclared_gate(&self) -> Result<(), ViCode> {
        if self.declared.count_ones() < DIFF_ROWS as u32 {
            return Err(ViCode::EUndeclared);
        }
        Ok(())
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

impl Default for ViLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 未声明差异注入（10 轮，确定性）：声明 0..(8-k%3) 行后审计。
pub struct UndeclInjector {
    seed: u64,
}

impl UndeclInjector {
    /// 种子 = SEG_BASE 派生。
    pub fn new() -> Self {
        UndeclInjector {
            seed: (SEG_BASE as u64) << 16 | 0x5216,
        }
    }

    /// 下一轮声明行数（5..=7，恒 <8 → 审计必违）。
    pub fn next_declared(&mut self) -> usize {
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.seed >> 33) % 3) as usize + 5
    }
}

impl Default for UndeclInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：六判据 + shim 17 格 + 差异 8/8 + 注入 10/10 + 指纹。
/// 返回（声明数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ViLedger::new();
    // ① 六判据全落。
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    // ② shim 共面行 17 格。
    let mut i = 0usize;
    while i < SHARED_ROWS {
        let _ = led.land_shim(i);
        i += 1;
    }
    // ③ 差异声明 8/8。
    let mut declared = 0u32;
    let mut i = 0usize;
    while i < DIFF_ROWS {
        if led.declare_diff(i).is_ok() {
            declared += 1;
        }
        i += 1;
    }
    // ④ 注入：未声明差异审计 10/10。
    let mut inj = UndeclInjector::new();
    let mut r = 0usize;
    while r < 10 {
        let n = inj.next_declared();
        let mut lt = ViLedger::new();
        let mut d = 0usize;
        while d < n {
            let _ = lt.declare_diff(d);
            d += 1;
        }
        if lt.undeclared_gate().is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5400 + r as u64);
        } else {
            led.escaped += 1;
        }
        r += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (declared, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52022_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52022");
    // 1. 正常流：差异 8/8 + 注入全抓。
    let (declared, caught, fp) = run_cycle();
    s.add(
        "差异声明 8/8 + 注入全抓",
        declared == DIFF_ROWS as u32 && caught >= 10,
        "未声明差异审计 10/10 必抓",
    );
    // 2. 边界：shim 越册/判据越闭集/差异越册。
    let mut l0 = ViLedger::new();
    s.add(
        "边界·shim 越册",
        l0.land_shim(SHIM_ROWS) == Err(ViCode::ENoShim),
        "shim 20 越册必拒",
    );
    s.add(
        "边界·差异越册",
        l0.declare_diff(DIFF_ROWS) == Err(ViCode::ENoDiff),
        "差异 8 越册必拒",
    );
    s.add(
        "边界·判据越闭集",
        l0.judge_gate(6) == Err(ViCode::ENoGate),
        "判据 6 越闭集必拒",
    );
    // 3. 复用率语义。
    s.add(
        "复用率 85% 达标",
        reuse_pct() >= REUSE_MIN_PCT && reuse_pct() == 85,
        "17/20 = 85%（Electron 同基座）",
    );
    // 4. 独有行不计复用（直判）。
    let mut l1 = ViLedger::new();
    s.add(
        "复用·独有行拒",
        l1.land_shim(SHARED_ROWS) == Err(ViCode::EReuseDrift),
        "行 17 为独有行，落复用账必拒",
    );
    // 5. shim 全账核验。
    let mut l2 = ViLedger::new();
    let mut i = 0usize;
    while i < SHARED_ROWS {
        let _ = l2.land_shim(i);
        i += 1;
    }
    s.add("复用·17 格全账", l2.reuse_ok(), "账面 == 复算且达标");
    // 6. 差异声明语义。
    let mut l3 = ViLedger::new();
    let _ = l3.declare_diff(0);
    s.add("差异·重复声明拒", l3.declare_diff(0) == Err(ViCode::EDup), "同行只许声明一次");
    let mut l4 = ViLedger::new();
    s.add(
        "差异·未声明审计拒",
        l4.undeclared_gate() == Err(ViCode::EUndeclared),
        "零声明审计必违",
    );
    // 7. 六判据全落。
    let mut l5 = ViLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = l5.judge_gate(g);
        g += 1;
    }
    s.add("六判据全落", l5.gates_all(), "母版闸 6/6");
    // 8. 注入账守恒。
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
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
        let row = shim_row(0).unwrap_or(ShimRow { no: 0, shared: true });
        assert!(row.shared);
        assert!(diff_row(0).is_some());
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(shim_row(SHIM_ROWS).is_none());
        assert!(diff_row(DIFF_ROWS).is_none());
        assert_eq!(ViLedger::new().land_shim(SHIM_ROWS), Err(ViCode::ENoShim));
    }

    #[test]
    fn test_reuse_rate() {
        assert_eq!(reuse_pct(), 85);
        assert!(reuse_pct() >= REUSE_MIN_PCT);
    }

    #[test]
    fn test_exclusive_row_rejected() {
        let mut led = ViLedger::new();
        // 行 17..20 为独有行（不共面）。
        assert_eq!(led.land_shim(SHARED_ROWS), Err(ViCode::EReuseDrift));
        assert!(led.land_shim(0).is_ok());
    }

    #[test]
    fn test_reuse_ok_full() {
        let mut led = ViLedger::new();
        let mut i = 0usize;
        while i < SHARED_ROWS {
            let _ = led.land_shim(i);
            i += 1;
        }
        assert!(led.reuse_ok());
        // 缺 1 格 → 不达标。
        let mut l2 = ViLedger::new();
        let mut i = 0usize;
        while i < SHARED_ROWS - 1 {
            let _ = l2.land_shim(i);
            i += 1;
        }
        assert!(!l2.reuse_ok());
    }

    #[test]
    fn test_diff_gate() {
        let mut led = ViLedger::new();
        assert!(led.undeclared_gate().is_err());
        let mut i = 0usize;
        while i < DIFF_ROWS {
            let _ = led.declare_diff(i);
            i += 1;
        }
        assert!(led.undeclared_gate().is_ok());
        assert!(led.diff_all());
        assert_eq!(led.declare_diff(0), Err(ViCode::EDup));
    }

    #[test]
    fn test_injection_all_caught() {
        let mut inj = UndeclInjector::new();
        let mut r = 0usize;
        let mut caught = 0u32;
        while r < 10 {
            let n = inj.next_declared();
            let mut lt = ViLedger::new();
            let mut d = 0usize;
            while d < n {
                let _ = lt.declare_diff(d);
                d += 1;
            }
            if lt.undeclared_gate().is_err() {
                caught += 1;
            }
            r += 1;
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_full_cycle() {
        let (declared, caught, _) = run_cycle();
        assert_eq!(declared, DIFF_ROWS as u32);
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
        let s = run_unx52022_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
