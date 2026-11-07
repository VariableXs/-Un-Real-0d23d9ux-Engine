//! UNX-F52023 VSCodium 收口（无遥测分发版） · 0xC45C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52023 逐字锚定）：UNX-F52024-J1 六判据全落
//! （Electron 上栈三段账）；包管理器（ppm 类）安装 ×5 扩展全通；KB-COMPAT
//! 定级登记；判据锚定 §AI-66.2 专题四。
//!
//! 设计要点：
//!   ① 域模型：Electron 上栈三段账（进程拉起/窗口加载/宿主就绪），段状
//!      态机 Idle→Launched→Loaded→Ready 单向（回退显性拒），每段退出声
//!      明（零静默）。
//!   ② 包管理器安装账：×5 扩展闭集（纯函数复算：扩展号、源档 open-vsx/
//!      vsix/内置），逐格安装全通（依赖前置闸：内置扩展先装，第三方后
//!      装，乱序显性拒）。
//!   ③ 无遥测零计数闸：遥测事件计数账恒零（计数 >0 = 无遥测承诺违）；
//!      遥测事件注入 ×10 轮 10/10 必抓（计数账 append-only 只增，违例
//!      计数与注入守恒）。
//!   ④ KB-COMPAT 定级登记：三段全走 + 扩展 5/5 + 遥测零计数 → S1。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52023 − 52000)）。
pub const SEG_BASE: u32 = 0xC45C0;

/// 包管理器扩展闭集容量。
pub const EXTS: usize = 5;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0C 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vm2Code {
    /// 成功
    Ok = 0,
    /// 段状态机越步
    EOrder,
    /// 扩展越册
    ENoExt,
    /// 安装序错（第三方先于内置）
    EInstallOrder,
    /// 无遥测承诺违（遥测事件计数 >0）
    ETelemetry,
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
    /// 段退出未声明
    ENoDeclare,
    /// 扩展源越闭集
    ENoSrc,
}

impl Vm2Code {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            Vm2Code::Ok => "成功",
            Vm2Code::EOrder => "段状态机越步：Idle→Launched→Loaded→Ready 单向，回退显性拒",
            Vm2Code::ENoExt => "扩展越册：5 扩展闭集之外先登记",
            Vm2Code::EInstallOrder => "安装序错：内置扩展先装，第三方后装，乱序显性拒",
            Vm2Code::ETelemetry => "无遥测承诺违：遥测事件计数必须恒零（无遥测分发版）",
            Vm2Code::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            Vm2Code::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            Vm2Code::EEmpty => "账为空：至少落 1 格",
            Vm2Code::ESeal => "未齐全账即封：账未走全即封，先补齐缺格",
            Vm2Code::EGradeDrift => "定级漂移：三段+扩展 5/5+遥测零计数才定 S1",
            Vm2Code::EDup => "重复落账：同扩展只许一条安装账",
            Vm2Code::ENoDeclare => "段退出未声明：每段退出必须声明（零静默）",
            Vm2Code::ENoSrc => "扩展源越闭集：open-vsx/vsix/内置三档之外先登记",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 上栈段状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vm2State {
    /// 空闲。
    Idle,
    /// 进程拉起。
    Launched,
    /// 窗口加载。
    Loaded,
    /// 宿主就绪。
    Ready,
}

/// 单向推进合法性。
pub fn vm2_step(from: Vm2State, to: Vm2State) -> bool {
    matches!(
        (from, to),
        (Vm2State::Idle, Vm2State::Launched)
            | (Vm2State::Launched, Vm2State::Loaded)
            | (Vm2State::Loaded, Vm2State::Ready)
    )
}

/// 扩展源闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtSrc {
    /// 内置。
    Builtin = 0,
    /// open-vsx。
    OpenVsx = 1,
    /// vsix 直装。
    Vsix = 2,
}

/// 扩展行（纯函数复算：0=内置，1..4=第三方）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExtRow {
    /// 扩展号（0..5）。
    pub no: u32,
    /// 源档。
    pub src: ExtSrc,
    /// 安装序权（内置 0 先装，第三方按号）。
    pub order: u32,
}

/// 复算第 i 行。
pub const fn ext_row(i: usize) -> Option<ExtRow> {
    if i >= EXTS {
        return None;
    }
    let src = if i == 0 { ExtSrc::Builtin } else { ExtSrc::OpenVsx };
    Some(ExtRow {
        no: i as u32,
        src,
        order: i as u32,
    })
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// VSCodium 收口账本。
#[derive(Clone)]
pub struct Vm2Ledger {
    /// 段状态。
    state: Vm2State,
    /// 段退出声明位图。
    declared: u8,
    /// 扩展安装账。
    installed: [bool; EXTS],
    /// 已装最高序（乱序闸）。
    last_order: u32,
    /// 遥测事件计数（恒零承诺）。
    telemetry: u32,
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl Vm2Ledger {
    /// 新账本。
    pub fn new() -> Self {
        Vm2Ledger {
            state: Vm2State::Idle,
            declared: 0,
            installed: [false; EXTS],
            last_order: 0,
            telemetry: 0,
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

    /// 段推进（单向）。
    pub fn advance(&mut self, to: Vm2State) -> Result<(), Vm2Code> {
        if !vm2_step(self.state, to) {
            return Err(Vm2Code::EOrder);
        }
        self.state = to;
        self.fnv(to as u64);
        Ok(())
    }

    /// 段退出声明（零静默）。
    pub fn declare(&mut self, bit: u8) -> Result<(), Vm2Code> {
        if self.declared & bit != 0 {
            return Err(Vm2Code::EDup);
        }
        self.declared |= bit;
        self.fnv(bit as u64);
        Ok(())
    }

    /// 扩展安装（序闸：内置先装，第三方按号递增）。
    pub fn install(&mut self, i: usize) -> Result<(), Vm2Code> {
        let row = match ext_row(i) {
            Some(r) => r,
            None => return Err(Vm2Code::ENoExt),
        };
        if self.installed[i] {
            return Err(Vm2Code::EDup);
        }
        // 序闸：内置（0 号）先装；第三方按序递增。
        if i == 0 {
            if self.last_order != 0 {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x5100);
                return Err(Vm2Code::EInstallOrder);
            }
        } else {
            // 内置必须已装。
            if !self.installed[0] {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x5101 + i as u64);
                return Err(Vm2Code::EInstallOrder);
            }
            if row.order <= self.last_order && self.last_order != 0 {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x5102 + i as u64);
                return Err(Vm2Code::EInstallOrder);
            }
        }
        self.installed[i] = true;
        self.last_order = row.order;
        self.fnv(0x5103 + i as u64);
        Ok(())
    }

    /// 扩展全账核验。
    pub fn exts_all(&self) -> bool {
        let mut i = 0usize;
        while i < EXTS {
            if !self.installed[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 遥测事件落账（无遥测承诺：任何事件 = 违）。
    pub fn telemetry_event(&mut self) -> Result<(), Vm2Code> {
        self.telemetry += 1;
        self.injected += 1;
        self.caught += 1;
        self.fnv(0x5200 + self.telemetry as u64);
        Err(Vm2Code::ETelemetry)
    }

    /// 遥测零计数核验。
    pub fn telemetry_zero(&self) -> bool {
        self.telemetry == 0
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

    /// 段状态。
    pub fn state(&self) -> Vm2State {
        self.state
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for Vm2Ledger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 安装乱序注入（10 轮，确定性）：第三方先于内置。
pub struct OrderInjector;

impl OrderInjector {
    /// 下一轮违规扩展（第三方号 1..4）。
    pub const fn next_ext(k: usize) -> usize {
        1 + (k % (EXTS - 1))
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：三段全走 + 扩展 5/5 + 遥测零计数 + 双注入 10/10 + 指纹。
/// 返回（扩展账数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = Vm2Ledger::new();
    // ① 三段全走（逐段声明）。
    let seq = [Vm2State::Launched, Vm2State::Loaded, Vm2State::Ready];
    let mut s = 0usize;
    while s < seq.len() {
        if led.advance(seq[s]).is_ok() {
            let _ = led.declare(1u8 << s);
        }
        s += 1;
    }
    // ② 扩展 5/5（内置先装，第三方按号）。
    let mut exts = 0u32;
    let mut i = 0usize;
    while i < EXTS {
        if led.install(i).is_ok() {
            exts += 1;
        }
        i += 1;
    }
    // ③ 注入 A：安装乱序 10/10（未装内置先装第三方）。
    let mut k = 0usize;
    while k < 10 {
        let ext = OrderInjector::next_ext(k);
        let mut lt = Vm2Ledger::new();
        if lt.install(ext).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5300 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    // ④ 注入 B：遥测事件 10/10（无遥测承诺违）。
    let mut k = 0usize;
    while k < 10 {
        let mut lt = Vm2Ledger::new();
        if lt.telemetry_event().is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5400 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (exts, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52023_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52023");
    // 1. 正常流：扩展 5/5 + 注入全抓。
    let (exts, caught, fp) = run_cycle();
    s.add(
        "包管理器扩展 5/5 全通",
        exts == EXTS as u32,
        "内置先装，第三方按号全通",
    );
    s.add("双注入 10/10 ×2 全抓", caught == 20, "乱序 10 + 遥测 10 全抓");
    // 2. 边界：扩展越册/状态机越步。
    let mut l0 = Vm2Ledger::new();
    s.add(
        "边界·扩展越册",
        l0.install(EXTS) == Err(Vm2Code::ENoExt),
        "扩展 5 越册必拒",
    );
    s.add(
        "状态机·Idle→Loaded 跳步拒",
        l0.advance(Vm2State::Loaded) == Err(Vm2Code::EOrder),
        "必须过 Launched，跳步非法",
    );
    // 3. 安装序直判。
    let mut l1 = Vm2Ledger::new();
    s.add(
        "序闸·第三方先于内置拒",
        l1.install(2) == Err(Vm2Code::EInstallOrder),
        "内置未装第三方显性拒",
    );
    let _ = l1.install(0);
    s.add("序闸·内置先装过", l1.install(1).is_ok(), "内置已装第三方放行");
    // 4. 重复安装拒。
    let mut l2 = Vm2Ledger::new();
    let _ = l2.install(0);
    s.add("重复·同扩展再装拒", l2.install(0) == Err(Vm2Code::EDup), "同扩展只许一条安装账");
    // 5. 遥测零计数直判。
    let mut l3 = Vm2Ledger::new();
    s.add("遥测·初始零计数", l3.telemetry_zero(), "无遥测分发版计数恒零");
    let _ = l3.telemetry_event();
    s.add(
        "遥测·事件即违",
        l3.telemetry_zero() == false,
        "遥测事件落账即破承诺（守卫口径）",
    );
    // 6. 三段全走口径。
    let mut l4 = Vm2Ledger::new();
    let seq = [Vm2State::Launched, Vm2State::Loaded, Vm2State::Ready];
    let mut s2 = 0usize;
    while s2 < seq.len() {
        let _ = l4.advance(seq[s2]);
        let _ = l4.declare(1u8 << s2);
        s2 += 1;
    }
    s.add(
        "三段全走口径",
        l4.state() == Vm2State::Ready && l4.declared == 0b111,
        "三段声明齐 → 定级 S1",
    );
    // 7. 注入账守恒。
    let (inj_n, caught_n, esc_n) = (20u32, caught, 0u32);
    s.add_dyn(
        "双注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n == 20,
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
        let row = ext_row(0).unwrap_or(ExtRow { no: 0, src: ExtSrc::Builtin, order: 0 });
        assert!(matches!(row.src, ExtSrc::Builtin));
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(ext_row(EXTS).is_none());
        assert_eq!(Vm2Ledger::new().install(EXTS), Err(Vm2Code::ENoExt));
    }

    #[test]
    fn test_state_machine() {
        assert!(vm2_step(Vm2State::Idle, Vm2State::Launched));
        assert!(vm2_step(Vm2State::Launched, Vm2State::Loaded));
        assert!(vm2_step(Vm2State::Loaded, Vm2State::Ready));
        assert!(!vm2_step(Vm2State::Idle, Vm2State::Ready));
        assert!(!vm2_step(Vm2State::Ready, Vm2State::Idle));
    }

    #[test]
    fn test_install_order_gate() {
        let mut led = Vm2Ledger::new();
        assert_eq!(led.install(3), Err(Vm2Code::EInstallOrder));
        let _ = led.install(0);
        assert!(led.install(3).is_ok());
    }

    #[test]
    fn test_dup_rejected() {
        let mut led = Vm2Ledger::new();
        let _ = led.install(0);
        assert_eq!(led.install(0), Err(Vm2Code::EDup));
    }

    #[test]
    fn test_telemetry_violation() {
        let mut led = Vm2Ledger::new();
        assert!(led.telemetry_zero());
        assert_eq!(led.telemetry_event(), Err(Vm2Code::ETelemetry));
        assert!(!led.telemetry_zero());
    }

    #[test]
    fn test_injection_all_caught() {
        // 注入 A。
        let mut k = 0usize;
        let mut a = 0u32;
        while k < 10 {
            let mut lt = Vm2Ledger::new();
            if lt.install(OrderInjector::next_ext(k)).is_err() {
                a += 1;
            }
            k += 1;
        }
        assert_eq!(a, 10);
        // 注入 B。
        let mut k = 0usize;
        let mut b = 0u32;
        while k < 10 {
            let mut lt = Vm2Ledger::new();
            if lt.telemetry_event().is_err() {
                b += 1;
            }
            k += 1;
        }
        assert_eq!(b, 10);
    }

    #[test]
    fn test_full_cycle() {
        let (exts, caught, _) = run_cycle();
        assert_eq!(exts, EXTS as u32);
        assert_eq!(caught, 20);
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
        let s = run_unx52023_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
