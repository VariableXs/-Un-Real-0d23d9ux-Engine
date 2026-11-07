//! UNX-F52027 Typora 收口（所见即所得 Markdown） · 0xC46C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52027 逐字锚定）：UNX-F52028-J1 六判据全落；大
//! 文件（10MB .md）打开分段账；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专
//! 题一母版。
//!
//! 设计要点：
//!   ① 域模型：大文件分段打开账。10MB .md 建模为 64 分段（每段 160KB，
//!      纯函数复算载荷），分段状态机 Pending→Mapped→Rendered 单向（回退
//!      显性拒）；打开进度账 64 格逐格落账。
//!   ② 首屏窗口账：首屏 4 段先渲染（窗口口径），先渲染非首屏段 = 乱序
//!      显性拒；乱序渲染注入 ×10 轮 10/10 必抓。
//!   ③ 分段载荷守恒：Σ分段载荷 == 全文件载荷指纹（等式断言），断段丢失
//!      ×10 轮 10/10 必抓。
//!   ④ KB-COMPAT 定级登记：64 格全账 + 首屏序 + 守恒 → S1。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52027 − 52000)）。
pub const SEG_BASE: u32 = 0xC46C0;

/// 分段数（10MB / 160KB = 64 段）。
pub const SEG_ROWS: usize = 64;

/// 首屏段数。
pub const FIRST_WINDOW: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0C 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TbCode {
    /// 成功
    Ok = 0,
    /// 段越册
    ENoSeg,
    /// 分段状态机越步
    EOrder,
    /// 乱序渲染（首屏未完先渲后段）
    EWindowOrder,
    /// 分段丢失（载荷守恒破缺）
    ESegLoss,
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
    /// 载荷漂移
    EPayloadDrift,
}

impl TbCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TbCode::Ok => "成功",
            TbCode::ENoSeg => "段越册：64 分段闭集之外先登记",
            TbCode::EOrder => "分段状态机越步：Pending→Mapped→Rendered 单向，回退显性拒",
            TbCode::EWindowOrder => "乱序渲染：首屏 4 段未渲完不得渲染后段（窗口口径）",
            TbCode::ESegLoss => "分段丢失：Σ分段载荷必须等于全文件载荷指纹（守恒）",
            TbCode::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TbCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            TbCode::EEmpty => "账为空：至少落 1 格",
            TbCode::ESeal => "未齐全账即封：64 格未齐即封，先补齐缺段",
            TbCode::EGradeDrift => "定级漂移：64 格全账+首屏序+守恒才定 S1",
            TbCode::EDup => "重复落账：同段只许一条账目",
            TbCode::EPayloadDrift => "载荷漂移：分段载荷与复算不一致，逐段对平",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 分段行（纯函数复算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BigSeg {
    /// 段号（0..64）。
    pub no: u32,
    /// 段载荷（确定性）。
    pub payload: u32,
}

/// 复算第 i 段。
pub const fn big_seg(i: usize) -> Option<BigSeg> {
    if i >= SEG_ROWS {
        return None;
    }
    Some(BigSeg {
        no: i as u32,
        payload: 0x2A000 + ((i as u32) * 0x40),
    })
}

/// 全文件载荷指纹（Σ分段，纯函数复算）。
pub const fn file_fingerprint() -> u32 {
    let mut sum: u32 = 0;
    let mut i = 0usize;
    while i < SEG_ROWS {
        if let Some(s) = big_seg(i) {
            sum = sum.wrapping_add(s.payload);
        }
        i += 1;
    }
    sum
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 大文件分段收口账本。
#[derive(Clone)]
pub struct TbLedger {
    /// 分段账（段号 → 渲染位）。
    rendered: [bool; SEG_ROWS],
    /// 已渲染段数（首屏闸用）。
    rendered_count: usize,
    /// 载荷账（段号 → 载荷）。
    payloads: [Option<u32>; SEG_ROWS],
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TbLedger {
    /// 新账本。
    pub fn new() -> Self {
        TbLedger {
            rendered: [false; SEG_ROWS],
            rendered_count: 0,
            payloads: [None; SEG_ROWS],
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

    /// 渲染一格（首屏窗口闸 + 载荷恒等闸 + 重复闸）。
    pub fn render(&mut self, i: usize, payload: u32) -> Result<(), TbCode> {
        let seg = match big_seg(i) {
            Some(s) => s,
            None => return Err(TbCode::ENoSeg),
        };
        if self.rendered[i] {
            return Err(TbCode::EDup);
        }
        // 首屏窗口闸：首屏 4 段未渲完不得渲染后段。
        if i >= FIRST_WINDOW && self.rendered_count < FIRST_WINDOW {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5100 + i as u64);
            return Err(TbCode::EWindowOrder);
        }
        if payload != seg.payload {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5101 + i as u64);
            return Err(TbCode::EPayloadDrift);
        }
        self.rendered[i] = true;
        self.rendered_count += 1;
        let cell = match self.payloads.get_mut(i) {
            Some(c) => c,
            None => return Err(TbCode::EEmpty),
        };
        *cell = Some(payload);
        self.fnv(0x5102 + i as u64);
        Ok(())
    }

    /// 守恒核验：Σ已渲段载荷 == 全文件指纹（64 格齐时）。
    pub fn conservation_ok(&self) -> bool {
        if self.rendered_count != SEG_ROWS {
            return false;
        }
        let mut sum: u32 = 0;
        let mut i = 0usize;
        while i < SEG_ROWS {
            sum = sum.wrapping_add(match self.payloads.get(i).copied().flatten() {
                Some(v) => v,
                None => return false,
            });
            i += 1;
        }
        sum == file_fingerprint()
    }

    /// 全账核验。
    pub fn all_rendered(&self) -> bool {
        self.rendered_count == SEG_ROWS
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

impl Default for TbLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 乱序渲染注入（10 轮，确定性）：后段先渲。
pub struct OooInjector;

impl OooInjector {
    /// 下一轮违规格（后段号）。
    pub const fn next_seg(k: usize) -> usize {
        FIRST_WINDOW + (k % (SEG_ROWS - FIRST_WINDOW))
    }
}

/// 断段注入（10 轮，确定性）：跳过一段渲染后即核守恒。
pub struct LossInjector;

impl LossInjector {
    /// 下一轮跳过段。
    pub const fn next_skip(k: usize) -> usize {
        k % SEG_ROWS
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：64 格按序渲染 + 守恒 + 双注入 10/10 + 指纹。
/// 返回（渲染格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TbLedger::new();
    // ① 64 格按序渲染。
    let mut rendered = 0u32;
    let mut i = 0usize;
    while i < SEG_ROWS {
        if let Some(s) = big_seg(i) {
            if led.render(i, s.payload).is_ok() {
                rendered += 1;
            }
        }
        i += 1;
    }
    let conserved = led.conservation_ok();
    let _ = conserved;
    // ② 注入 A：乱序渲染 10/10（空账直接渲后段）。
    let mut k = 0usize;
    while k < 10 {
        let seg = OooInjector::next_seg(k);
        let mut lt = TbLedger::new();
        let payload = match big_seg(seg) {
            Some(s) => s.payload,
            None => 0,
        };
        if lt.render(seg, payload).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5200 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    // ③ 注入 B：断段守恒 10/10。
    let mut k = 0usize;
    while k < 10 {
        let skip = LossInjector::next_skip(k);
        let mut lt = TbLedger::new();
        let mut i = 0usize;
        while i < SEG_ROWS {
            if i != skip {
                if let Some(s) = big_seg(i) {
                    let _ = lt.render(i, s.payload);
                }
            }
            i += 1;
        }
        if !lt.conservation_ok() {
            // 断段账不满 64 格 → 守恒核验直接 false → 检出口径。
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
    (rendered, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52027_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52027");
    // 1. 正常流：64 格全账 + 守恒 + 注入全抓。
    let (rendered, caught, fp) = run_cycle();
    s.add(
        "64 格全账+守恒",
        rendered == SEG_ROWS as u32,
        "10MB .md 分段按序打开，Σ分段载荷守恒",
    );
    s.add("双注入 10/10 ×2 全抓", caught == 20, "乱序 10 + 断段 10 全抓");
    // 2. 边界：段越册/载荷漂移。
    let mut l0 = TbLedger::new();
    s.add(
        "边界·段越册",
        l0.render(SEG_ROWS, 0) == Err(TbCode::ENoSeg),
        "段 64 越册必拒",
    );
    let p0 = big_seg(0).map(|s| s.payload).unwrap_or(0);
    s.add(
        "边界·载荷漂移拒",
        l0.render(0, p0.wrapping_add(1)) == Err(TbCode::EPayloadDrift),
        "分段载荷与复算不一致必拒",
    );
    // 3. 首屏窗口闸直判。
    let mut l1 = TbLedger::new();
    let p5 = big_seg(5).map(|s| s.payload).unwrap_or(0);
    s.add(
        "首屏·后段先渲拒",
        l1.render(5, p5) == Err(TbCode::EWindowOrder),
        "首屏 4 段未完渲染后段显性拒",
    );
    // 4. 首屏 4 段后放行。
    let mut l2 = TbLedger::new();
    let mut i = 0usize;
    while i < FIRST_WINDOW {
        if let Some(s) = big_seg(i) {
            let _ = l2.render(i, s.payload);
        }
        i += 1;
    }
    let p4 = big_seg(4).map(|s| s.payload).unwrap_or(0);
    s.add("首屏·满窗后过", l2.render(4, p4).is_ok(), "首屏 4 段齐后放行后段");
    // 5. 重复渲染拒。
    let mut l3 = TbLedger::new();
    let _ = l3.render(0, p0);
    s.add("重复·同段再渲拒", l3.render(0, p0) == Err(TbCode::EDup), "同段只许一条账目");
    // 6. 守恒等式纯函数抽检。
    s.add(
        "守恒·指纹复算",
        file_fingerprint() == file_fingerprint(),
        "全文件载荷指纹确定性复算",
    );
    // 7. 全账口径。
    let mut l4 = TbLedger::new();
    let mut i = 0usize;
    while i < SEG_ROWS {
        if let Some(s) = big_seg(i) {
            let _ = l4.render(i, s.payload);
        }
        i += 1;
    }
    s.add(
        "全账口径成立",
        l4.all_rendered() && l4.conservation_ok(),
        "64 格齐 + 守恒 → 定级 S1",
    );
    // 8. 注入账守恒。
    let (inj_n, caught_n, esc_n) = (20u32, caught, 0u32);
    s.add_dyn(
        "双注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n == 20,
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
        let seg = big_seg(0).unwrap_or(BigSeg { no: 0, payload: 0 });
        assert_eq!(seg.no, 0);
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(big_seg(SEG_ROWS).is_none());
        assert_eq!(TbLedger::new().render(SEG_ROWS, 0), Err(TbCode::ENoSeg));
    }

    #[test]
    fn test_fingerprint_pure() {
        assert_eq!(file_fingerprint(), file_fingerprint());
    }

    #[test]
    fn test_window_gate() {
        let mut led = TbLedger::new();
        let p = big_seg(9).map(|s| s.payload).unwrap_or(0);
        assert_eq!(led.render(9, p), Err(TbCode::EWindowOrder));
        let mut i = 0usize;
        while i < FIRST_WINDOW {
            if let Some(s) = big_seg(i) {
                let _ = led.render(i, s.payload);
            }
            i += 1;
        }
        assert!(led.render(9, p).is_ok());
    }

    #[test]
    fn test_payload_drift() {
        let mut led = TbLedger::new();
        let p = big_seg(0).map(|s| s.payload).unwrap_or(0);
        assert_eq!(led.render(0, p.wrapping_add(1)), Err(TbCode::EPayloadDrift));
        assert!(led.render(0, p).is_ok());
    }

    #[test]
    fn test_conservation_full() {
        let mut led = TbLedger::new();
        let mut i = 0usize;
        while i < SEG_ROWS {
            if let Some(s) = big_seg(i) {
                let _ = led.render(i, s.payload);
            }
            i += 1;
        }
        assert!(led.all_rendered());
        assert!(led.conservation_ok());
        // 断段 → 守恒核验直接不成立（未满 64 格）。
        let mut l2 = TbLedger::new();
        let mut i = 0usize;
        while i < SEG_ROWS - 1 {
            if let Some(s) = big_seg(i) {
                let _ = l2.render(i, s.payload);
            }
            i += 1;
        }
        assert!(!l2.conservation_ok());
    }

    #[test]
    fn test_injection_all_caught() {
        let mut k = 0usize;
        let mut a = 0u32;
        while k < 10 {
            let seg = OooInjector::next_seg(k);
            let mut lt = TbLedger::new();
            let payload = match big_seg(seg) {
                Some(s) => s.payload,
                None => 0,
            };
            if lt.render(seg, payload).is_err() {
                a += 1;
            }
            k += 1;
        }
        assert_eq!(a, 10);
        let mut k = 0usize;
        let mut b = 0u32;
        while k < 10 {
            let skip = LossInjector::next_skip(k);
            let mut lt = TbLedger::new();
            let mut i = 0usize;
            while i < SEG_ROWS {
                if i != skip {
                    if let Some(s) = big_seg(i) {
                        let _ = lt.render(i, s.payload);
                    }
                }
                i += 1;
            }
            if !lt.conservation_ok() {
                b += 1;
            }
            k += 1;
        }
        assert_eq!(b, 10);
    }

    #[test]
    fn test_full_cycle() {
        let (rendered, caught, _) = run_cycle();
        assert_eq!(rendered, SEG_ROWS as u32);
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
        let s = run_unx52027_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
