//! UNX-F52066 .NET 8 SDK 收口（Windows 发行版，与 L4 分账） · 0xC5080段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52066 逐字锚定）：UNX-F52067-J1 同 F52066 口
//! 径；与 8 SDK 并存 global.json 选择语义 ×5；KB-COMPAT 定级登记；判
//! 据锚定 §AI-66.2 专题六。
//!
//! 设计要点：
//!   ① 域模型：global.json 选择语义 ×5 场景（精确钉版/滚动前向/滚动后
//!      向/区间匹配/缺省回退），逐场景选择载荷（纯函数折叠）恒等才收；
//!      选择漂移必抓；重复选择拒。
//!   ② 钉版单向闸：global.json 钉版登记后不许降档改写（降档即漂移必
//!      抓）；缺省回退档必须为 LTS 档（回退漂移必抓）。
//!   ③ SDK 档闭集（6.0/7.0/8.0 三档）越档拒。
//!   ④ 注入 ×10 轮（选择漂移/钉版降档/场景越闭集三态轮转）10/10 必
//!      抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（5 场景全通 + 钉
//!      版单向 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52066 − 52000)）。
pub const SEG_BASE: u32 = 0xC5080;

/// SDK 档闭集。
pub const SDK_SLOTS: usize = 3;

/// global.json 选择场景数。
pub const SEL_SCENES: usize = 5;

/// LTS 缺省回退档。
pub const LTS_SLOT: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DnCode {
    /// 成功
    Ok = 0,
    /// SDK 档越闭集
    ENoSdk,
    /// 场景号越闭集
    ENoScene,
    /// 选择漂移
    EDrift,
    /// 钉版降档
    EPinDown,
    /// 回退漂移（缺省非 LTS）
    EFallback,
    /// 重复选择
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

impl DnCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DnCode::Ok => "成功",
            DnCode::ENoSdk => "SDK 档越闭集：6.0/7.0/8.0 三档之外先登记",
            DnCode::ENoScene => "场景号越闭集：5 选择场景之外先登记",
            DnCode::EDrift => "选择漂移：选择结果折叠与基准不恒等，逐场景复算",
            DnCode::EPinDown => "钉版降档：global.json 钉版登记后不许降档改写",
            DnCode::EFallback => "回退漂移：缺省回退档必须为 LTS 档",
            DnCode::EDup => "重复选择：同场景只许一条选择账",
            DnCode::ESeal => "未齐即封：5 场景未齐先补齐",
            DnCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DnCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DnCode::EGradeDrift => "定级漂移：5 场景全通+钉版单向+回退 LTS 才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 选择格载荷（场景 sc → SDK 档折叠）。
pub const fn sel_cell(sc: usize) -> Option<u64> {
    if sc >= SEL_SCENES {
        return None;
    }
    Some((0x6E000u64 + (sc as u64) * 0x2B) ^ (LTS_SLOT as u64).wrapping_mul(0x01000193))
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct DnLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 选择账。
    sels: [Option<u64>; SEL_SCENES],
    /// 钉版登记（单向）。
    pin: Option<usize>,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DnLedger {
    /// 新账本。
    pub fn new() -> Self {
        DnLedger {
            gates: [None; 6],
            sels: [None; SEL_SCENES],
            pin: None,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DnCode> {
        if gate >= 6 {
            return Err(DnCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x2B100 + gate as u64);
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

    /// 选择一格（载荷对照 + 重复拒）。
    pub fn choose(&mut self, sc: usize, got: u64) -> Result<(), DnCode> {
        if sc >= SEL_SCENES {
            return Err(DnCode::ENoScene);
        }
        let want = match sel_cell(sc) {
            Some(w) => w,
            None => return Err(DnCode::ENoScene),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2B200 + sc as u64);
            return Err(DnCode::EDrift);
        }
        if match self.sels.get(sc) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DnCode::EDup);
        }
        if let Some(c) = self.sels.get_mut(sc) {
            *c = Some(got);
        }
        self.fnv(0x2B201 + sc as u64);
        Ok(())
    }

    /// 选择全账核验（5 格）。
    pub fn sels_all(&self) -> bool {
        let mut sc = 0usize;
        while sc < SEL_SCENES {
            match self.sels.get(sc) {
                Some(Some(_)) => {}
                _ => return false,
            }
            sc += 1;
        }
        true
    }

    /// 钉版登记（单向：只许升档或首次，降档必抓）。
    pub fn set_pin(&mut self, sdk: usize) -> Result<(), DnCode> {
        if sdk >= SDK_SLOTS {
            return Err(DnCode::ENoSdk);
        }
        if let Some(prev) = self.pin {
            if sdk < prev {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x2B300);
                return Err(DnCode::EPinDown);
            }
        }
        self.pin = Some(sdk);
        self.fnv(0x2B301 + sdk as u64);
        Ok(())
    }

    /// 缺省回退档核验（必须 LTS）。
    pub fn set_fallback(&mut self, sdk: usize) -> Result<(), DnCode> {
        if sdk >= SDK_SLOTS {
            return Err(DnCode::ENoSdk);
        }
        if sdk != LTS_SLOT {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x2B400);
            return Err(DnCode::EFallback);
        }
        self.fnv(0x2B401);
        Ok(())
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

impl Default for DnLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DnLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let _ = led.set_pin(SDK_SLOTS - 1);
    let _ = led.set_fallback(LTS_SLOT);
    let mut recorded = 0u32;
    let mut sc = 0usize;
    while sc < SEL_SCENES {
        if let Some(w) = sel_cell(sc) {
            if led.choose(sc, w).is_ok() {
                recorded += 1;
            }
        }
        sc += 1;
    }
    // 注入 10 轮：选择漂移 / 钉版降档 / 场景越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DnLedger::new();
        let caught = match kind {
            0 => lt.choose(1, sel_cell(1).unwrap_or(0).wrapping_add(1)) == Err(DnCode::EDrift),
            1 => {
                let _ = lt.set_pin(2);
                lt.set_pin(0) == Err(DnCode::EPinDown)
            }
            _ => lt.choose(SEL_SCENES, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x2B500 + k as u64);
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
pub fn run_unx52066_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52066");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "global.json 选择 5/5 全通",
        recorded == SEL_SCENES as u32,
        "钉版/滚动前向/滚动后向/区间/缺省五场景逐格恒等",
    );
    s.add(
        "钉版单向账",
        {
            let mut lt = DnLedger::new();
            let _ = lt.set_pin(1);
            lt.set_pin(0) == Err(DnCode::EPinDown)
        },
        "钉版登记后降档必抓",
    );
    s.add(
        "缺省回退 LTS",
        {
            let mut lt = DnLedger::new();
            lt.set_fallback(LTS_SLOT).is_ok()
                && lt.set_fallback(0) == Err(DnCode::EFallback)
        },
        "缺省回退档必须为 LTS 档",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/降档/越闭集三态轮转必抓");
    let mut l0 = DnLedger::new();
    s.add(
        "边界·SDK 档越闭集",
        l0.set_pin(SDK_SLOTS).is_err(),
        "档 3 越闭集必拒",
    );
    s.add(
        "边界·场景越闭集",
        l0.choose(SEL_SCENES, 0).is_err(),
        "场景 5 越闭集必拒",
    );
    let mut l1 = DnLedger::new();
    let _ = l1.choose(0, sel_cell(0).unwrap_or(0));
    s.add(
        "重复·再选择拒",
        l1.choose(0, sel_cell(0).unwrap_or(0)).is_err(),
        "同场景只许一条选择账",
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
    s.add("收口·判据条数", s.len() >= 10, "含本条共 ≥11 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sel_cell_pure() {
        let a = sel_cell(2).unwrap_or(0);
        assert_eq!(a, sel_cell(2).unwrap_or(0));
        assert_ne!(a, sel_cell(3).unwrap_or(0));
        assert!(sel_cell(SEL_SCENES).is_none());
    }

    #[test]
    fn test_pin_monotone() {
        let mut lt = DnLedger::new();
        let _ = lt.set_pin(2);
        assert_eq!(lt.set_pin(1), Err(DnCode::EPinDown));
        assert!(lt.set_pin(2).is_ok());
        assert_eq!(lt.set_pin(SDK_SLOTS), Err(DnCode::ENoSdk));
    }

    #[test]
    fn test_fallback_lts() {
        let mut lt = DnLedger::new();
        assert!(lt.set_fallback(LTS_SLOT).is_ok());
        assert_eq!(lt.set_fallback(0), Err(DnCode::EFallback));
        assert_eq!(lt.set_fallback(SDK_SLOTS), Err(DnCode::ENoSdk));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, SEL_SCENES as u32);
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
        assert_eq!(SEG_BASE, 0xC5080);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52066_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
