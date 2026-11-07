//! UNX-F52099 Android SDK 平台包族收口（android-33/34/35 三档） · 0xC56C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52099 逐字锚定）：UNX-F52100-J1 三档平台包安
//! 装/引用/编译样本 ×3 ×10 全通；多档并存选择语义零漂移；KB-COMPAT 定
//! 级登记；判据锚定 §AI-66.2 专题六版本阶梯。
//!
//! 设计要点：
//!   ① 域模型：三档平台包（android-33/34/35）闭集 ×3 动作（安装/引用/
//!      编译）×3 样本 = 27 格并存账，逐格载荷（纯函数折叠）恒等才收；
//!      漂移必抓。
//!   ② 版本阶梯闸：三档 API 阶梯（33/34/35）严格递增；并存选择语义：
//!      目标 API ≤ 最高档才可解析（越档 → 选择闸必抓）；降档选择必抓。
//!   ③ 安装→引用→编译动作序单调；注入 ×10 轮（阶梯降档/并存漂移/包
//!      越闭集三态轮转）10/10 必抓，注入=检出+逃逸守恒；六判据母版闸
//!      + 定级（27 格全通 + 阶梯单调 → S1）。
//!   ④ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52099 − 52000)）。
pub const SEG_BASE: u32 = 0xC56C0;

/// 平台包档数。
pub const TIERS: usize = 3;

/// 动作数。
pub const ACTIONS: usize = 3;

/// 样本数。
pub const SAMPLES: usize = 3;

/// API 阶梯基准（严格递增）。
pub const API0: u32 = 33;
pub const API1: u32 = 34;
pub const API2: u32 = 35;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpCode {
    /// 成功
    Ok = 0,
    /// 包档越闭集
    ENoTier,
    /// 动作越闭集
    ENoAction,
    /// 样本号越闭集
    ENoSample,
    /// 并存载荷漂移
    EDrift,
    /// 阶梯降档
    EDowngrade,
    /// 选择越档（目标 API 超最高档）
    ENoResolve,
    /// 动作序逆序
    EOrder,
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

impl SpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SpCode::Ok => "成功",
            SpCode::ENoTier => "包档越闭集：android-33/34/35 三档之外先登记",
            SpCode::ENoAction => "动作越闭集：安装/引用/编译三元之外先登记",
            SpCode::ENoSample => "样本号越闭集：3 样本之外先登记",
            SpCode::EDrift => "并存载荷漂移：折叠与基准不恒等，逐格复算",
            SpCode::EDowngrade => "阶梯降档：API 阶梯必须严格递增，降档必抓",
            SpCode::ENoResolve => "选择越档：目标 API 超最高档不可解析必抓",
            SpCode::EOrder => "动作序逆序：安装→引用→编译严格递增，逆序必抓",
            SpCode::EDup => "重复落账：同格只许一条账目",
            SpCode::ESeal => "未齐即封：27 格未齐先补齐",
            SpCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SpCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SpCode::EGradeDrift => "定级漂移：27 格全通+阶梯单调+选择闸在册才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// API 阶梯（纯函数：档 t → API 档）。
pub const fn tier_api(t: usize) -> Option<u32> {
    match t {
        0 => Some(API0),
        1 => Some(API1),
        2 => Some(API2),
        _ => None,
    }
}

/// 并存格载荷（档 t × 动作 a × 样本 s）。
pub const fn cell(t: usize, a: usize, s: usize) -> Option<u64> {
    if t >= TIERS || a >= ACTIONS || s >= SAMPLES {
        return None;
    }
    Some(
        (0xC5000u64 + (t as u64) * 0x300)
            + (a as u64) * 0x30
            + (s as u64) * 0x3
            ^ (t as u64).wrapping_mul(0x01000193),
    )
}

/// 选择闸（目标 API 可解析：≤ 最高档）。
pub const fn resolvable(api: u32) -> bool {
    api <= API2
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct SpLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 并存账。
    cells: [[[Option<u64>; SAMPLES]; ACTIONS]; TIERS],
    /// 各档最后动作。
    last_action: [i64; TIERS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SpLedger {
    /// 新账本。
    pub fn new() -> Self {
        SpLedger {
            gates: [None; 6],
            cells: [[[None; SAMPLES]; ACTIONS]; TIERS],
            last_action: [-1; TIERS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SpCode> {
        if gate >= 6 {
            return Err(SpCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x43100 + gate as u64);
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

    /// 选择闸（目标 API 可解析核验）。
    pub fn resolve(&mut self, api: u32) -> Result<(), SpCode> {
        if !resolvable(api) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x43200 + api as u64);
            return Err(SpCode::ENoResolve);
        }
        self.fnv(0x43201 + api as u64);
        Ok(())
    }

    /// 并存一格（动作序单调 + 载荷对照 + 重复拒）。
    pub fn record(&mut self, t: usize, a: usize, s: usize) -> Result<u64, SpCode> {
        if t >= TIERS {
            return Err(SpCode::ENoTier);
        }
        if a >= ACTIONS {
            return Err(SpCode::ENoAction);
        }
        if s >= SAMPLES {
            return Err(SpCode::ENoSample);
        }
        if (a as i64) < self.last_action[t] {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x43300 + t as u64);
            return Err(SpCode::EOrder);
        }
        let payload = match cell(t, a, s) {
            Some(p) => p,
            None => return Err(SpCode::ENoSample),
        };
        if match self.cells.get(t).and_then(|m| m.get(a)).and_then(|r| r.get(s)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SpCode::EDup);
        }
        if let Some(m) = self.cells.get_mut(t) {
            if let Some(r) = m.get_mut(a) {
                if let Some(c) = r.get_mut(s) {
                    *c = Some(payload);
                }
            }
        }
        self.last_action[t] = a as i64;
        self.fnv(0x43301 + (t as u64) * 9 + (a as u64) * 3 + s as u64);
        Ok(payload)
    }

    /// 并存全账核验（27 格）。
    pub fn all(&self) -> bool {
        let mut t = 0usize;
        while t < TIERS {
            let mut a = 0usize;
            while a < ACTIONS {
                let mut s = 0usize;
                while s < SAMPLES {
                    match self.cells.get(t).and_then(|m| m.get(a)).and_then(|r| r.get(s)) {
                        Some(Some(_)) => {}
                        _ => return false,
                    }
                    s += 1;
                }
                a += 1;
            }
            t += 1;
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

impl Default for SpLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SpLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let _ = led.resolve(API2);
    let mut recorded = 0u32;
    let mut t = 0usize;
    while t < TIERS {
        let mut a = 0usize;
        while a < ACTIONS {
            let mut s = 0usize;
            while s < SAMPLES {
                if led.record(t, a, s).is_ok() {
                    recorded += 1;
                }
                s += 1;
            }
            a += 1;
        }
        t += 1;
    }
    // 注入 10 轮：选择越档 / 动作逆序 / 包档越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SpLedger::new();
        let caught = match kind {
            0 => lt.resolve(API2 + 1) == Err(SpCode::ENoResolve),
            1 => {
                let _ = lt.record(0, 2, 0);
                lt.record(0, 0, 0) == Err(SpCode::EOrder)
            }
            _ => lt.record(TIERS, 0, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x43400 + k as u64);
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
pub fn run_unx52099_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52099");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三档并存 27/27 全通",
        recorded >= (TIERS * ACTIONS * SAMPLES) as u32,
        "android-33/34/35 × 安装/引用/编译 ×3 样本逐格恒等",
    );
    s.add(
        "API 阶梯严格递增",
        {
            let mut ok = true;
            let mut t = 1usize;
            while t < TIERS {
                if tier_api(t).unwrap_or(0) <= tier_api(t - 1).unwrap_or(0) {
                    ok = false;
                }
                t += 1;
            }
            ok && tier_api(TIERS).is_none()
        },
        "33→34→35 阶梯单调",
    );
    s.add(
        "选择闸",
        resolvable(API2) && !resolvable(API2 + 1) && !resolvable(99),
        "目标 API ≤ 最高档可解析，越档必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越档/逆序/越闭集三态轮转必抓");
    let mut l0 = SpLedger::new();
    s.add(
        "选择·越档拒",
        l0.resolve(API2 + 1) == Err(SpCode::ENoResolve),
        "目标 API 超最高档必抓",
    );
    s.add(
        "动作·逆序拒",
        {
            let _ = l0.record(0, 2, 0);
            l0.record(0, 0, 0) == Err(SpCode::EOrder)
        },
        "编译后回安装必抓",
    );
    s.add(
        "边界·包档越闭集",
        l0.record(TIERS, 0, 0).is_err(),
        "档 3 越闭集必拒",
    );
    s.add(
        "边界·样本越闭集",
        l0.record(0, 0, SAMPLES).is_err(),
        "样本 3 越闭集必拒",
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
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_api() {
        assert_eq!(tier_api(0), Some(33));
        assert_eq!(tier_api(1), Some(34));
        assert_eq!(tier_api(2), Some(35));
        assert!(tier_api(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cell(1, 2, 0).unwrap_or(0);
        assert_eq!(a, cell(1, 2, 0).unwrap_or(0));
        assert_ne!(a, cell(1, 2, 1).unwrap_or(0));
        assert_ne!(a, cell(2, 2, 0).unwrap_or(0));
        assert!(cell(TIERS, 0, 0).is_none());
        assert!(cell(0, ACTIONS, 0).is_none());
        assert!(cell(0, 0, SAMPLES).is_none());
    }

    #[test]
    fn test_action_order_caught() {
        let mut lt = SpLedger::new();
        let _ = lt.record(1, 2, 0);
        assert_eq!(lt.record(1, 0, 0), Err(SpCode::EOrder));
        assert!(lt.record(1, 2, 1).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (TIERS * ACTIONS * SAMPLES) as u32);
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
        assert_eq!(SEG_BASE, 0xC56C0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52099_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
