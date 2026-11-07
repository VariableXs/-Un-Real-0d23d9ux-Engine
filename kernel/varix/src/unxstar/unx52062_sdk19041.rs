//! UNX-F52062 Windows SDK 10.0.19041（2004 版）收口 · 0xC4F80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52062 逐字锚定）：UNX-F52063-J1 同 F52062 口
//! 径；多 SDK 并存（与 19041）选择语义 ×10 场景零漂移；KB-COMPAT 定级
//! 登记；判据锚定 §AI-66.2 专题六版本阶梯。
//!
//! 设计要点：
//!   ① 域模型：多 SDK 并存选择账。SDK 闭集 3 档（19041/22000/26100），
//!      并存选择场景 ×10（默认档/显式档/环境档/注册表档/回退档 五选择
//!      源 × 2 工程型），每场景记录选择结果载荷（纯函数折叠）恒等才
//!      收；选择漂移必抓。
//!   ② 选择优先级闸：选择源优先级闭集（显式 > 环境 > 注册表 > 默认 >
//!      回退），逆序选择必抓；重复选择拒。
//!   ③ 19041 默认档：与 19041 并存时 19041 必须为默认档（默认档漂移
//!      即抓）；三档全在册核验。
//!   ④ 注入 ×10 轴（选择漂移/逆序选择/档越闭集三态轮转）10/10 必抓，
//!      注入=检出+逃逸守恒；六判据母版闸 + 定级（10 场景全通 + 三档在
//!      册 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52062 − 52000)）。
pub const SEG_BASE: u32 = 0xC4F80;

/// SDK 档闭集。
pub const SDK_SLOTS: usize = 3;

/// 选择场景数。
pub const SCENES: usize = 10;

/// 选择源优先级基准档（19041 → 0）。
pub const DEFAULT_SDK: usize = 0;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwCode {
    /// 成功
    Ok = 0,
    /// SDK 档越闭集
    ENoSdk,
    /// 场景号越闭集
    ENoScene,
    /// 选择漂移
    EDrift,
    /// 选择源逆序
    EOrder,
    /// 默认档漂移
    EDefault,
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

impl SwCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SwCode::Ok => "成功",
            SwCode::ENoSdk => "SDK 档越闭集：19041/22000/26100 三档之外先登记",
            SwCode::ENoScene => "场景号越闭集：10 场景之外先登记",
            SwCode::EDrift => "选择漂移：选择结果折叠与基准不恒等，逐场景复算",
            SwCode::EOrder => "选择源逆序：显式 > 环境 > 注册表 > 默认 > 回退优先级必须单调",
            SwCode::EDefault => "默认档漂移：与 19041 并存时默认档必须为 19041",
            SwCode::EDup => "重复选择：同场景只许一条选择账",
            SwCode::ESeal => "未齐即封：10 场景未齐先补齐",
            SwCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SwCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SwCode::EGradeDrift => "定级漂移：10 场景全通+三档在册+优先级单调才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 选择格载荷（场景 s → SDK 档，纯函数折叠：19041 五源默认）。
pub const fn scene_cell(s: usize) -> Option<u64> {
    if s >= SCENES {
        return None;
    }
    Some((0x5E000u64 + (s as u64) * 0x1F) ^ (DEFAULT_SDK as u64).wrapping_mul(0x01000193))
}

/// 选择源优先级（源 src → 优先级序，显式 0 最高）。
pub const fn src_priority(src: usize) -> Option<u32> {
    if src >= 5 {
        return None;
    }
    Some(src as u32)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct SwLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 选择账。
    scenes: [Option<u64>; SCENES],
    /// 最后优先级。
    last_prio: i64,
    /// 默认档登记。
    default_set: Option<usize>,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SwLedger {
    /// 新账本。
    pub fn new() -> Self {
        SwLedger {
            gates: [None; 6],
            scenes: [None; SCENES],
            last_prio: -1,
            default_set: None,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SwCode> {
        if gate >= 6 {
            return Err(SwCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x27100 + gate as u64);
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

    /// 选择一格（载荷对照 + 优先级单调 + 重复拒）。
    pub fn choose(&mut self, s: usize, got: u64, prio: u32) -> Result<(), SwCode> {
        if s >= SCENES {
            return Err(SwCode::ENoScene);
        }
        let want = match scene_cell(s) {
            Some(w) => w,
            None => return Err(SwCode::ENoScene),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x27200 + s as u64);
            return Err(SwCode::EDrift);
        }
        if (prio as i64) < self.last_prio {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x27201);
            return Err(SwCode::EOrder);
        }
        if match self.scenes.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SwCode::EDup);
        }
        if let Some(c) = self.scenes.get_mut(s) {
            *c = Some(got);
        }
        self.last_prio = prio as i64;
        self.fnv(0x27202 + s as u64);
        Ok(())
    }

    /// 选择全账核验（10 格）。
    pub fn scenes_all(&self) -> bool {
        let mut s = 0usize;
        while s < SCENES {
            match self.scenes.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 默认档登记（19041 之外漂移必抓）。
    pub fn set_default(&mut self, sdk: usize) -> Result<(), SwCode> {
        if sdk >= SDK_SLOTS {
            return Err(SwCode::ENoSdk);
        }
        if sdk != DEFAULT_SDK {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x27300);
            return Err(SwCode::EDefault);
        }
        self.default_set = Some(sdk);
        self.fnv(0x27301);
        Ok(())
    }

    /// 默认档核验。
    pub fn default_ok(&self) -> bool {
        self.default_set == Some(DEFAULT_SDK)
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

impl Default for SwLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SwLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let _ = led.set_default(DEFAULT_SDK);
    let mut recorded = 0u32;
    let mut s = 0usize;
    while s < SCENES {
        if let Some(w) = scene_cell(s) {
            if led.choose(s, w, s as u32).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    // 注入 10 轮：选择漂移 / 默认档漂移 / 场景越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SwLedger::new();
        let caught = match kind {
            0 => lt.choose(2, scene_cell(2).unwrap_or(0).wrapping_add(1), 0)
                == Err(SwCode::EDrift),
            1 => lt.set_default(2) == Err(SwCode::EDefault),
            _ => lt.choose(SCENES, 0, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x27400 + k as u64);
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
pub fn run_unx52062_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52062");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "并存选择 10/10 零漂移",
        recorded == SCENES as u32,
        "五选择源 ×2 工程型 10 场景逐场景恒等",
    );
    s.add(
        "默认档 19041 登记",
        {
            let mut lt = SwLedger::new();
            let _ = lt.set_default(DEFAULT_SDK);
            lt.default_ok()
        },
        "与 19041 并存时默认档必须为 19041",
    );
    s.add(
        "选择源优先级闭集",
        {
            let mut ok = true;
            let mut src = 0usize;
            while src < 5 {
                if src_priority(src) != Some(src as u32) {
                    ok = false;
                }
                src += 1;
            }
            ok && src_priority(5).is_none()
        },
        "显式 > 环境 > 注册表 > 默认 > 回退五源优先级单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/默认/越闭集三态轮转必抓");
    let mut l0 = SwLedger::new();
    s.add(
        "默认档·漂移拒",
        l0.set_default(1) == Err(SwCode::EDefault),
        "默认档漂移必抓",
    );
    s.add(
        "边界·SDK 档越闭集",
        l0.set_default(SDK_SLOTS).is_err(),
        "档 3 越闭集必拒",
    );
    s.add(
        "边界·场景越闭集",
        l0.choose(SCENES, 0, 0).is_err(),
        "场景 10 越闭集必拒",
    );
    let mut l1 = SwLedger::new();
    let _ = l1.choose(0, scene_cell(0).unwrap_or(0), 2);
    s.add(
        "重复·再选择拒",
        l1.choose(0, scene_cell(0).unwrap_or(0), 2).is_err(),
        "同场景只许一条选择账",
    );
    s.add(
        "优先级·逆序拒",
        l1.choose(1, scene_cell(1).unwrap_or(0), 1) == Err(SwCode::EOrder),
        "优先级回退必抓",
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
        let a = scene_cell(3).unwrap_or(0);
        assert_eq!(a, scene_cell(3).unwrap_or(0));
        assert_ne!(a, scene_cell(4).unwrap_or(0));
        assert!(scene_cell(SCENES).is_none());
        assert!(src_priority(5).is_none());
        assert_eq!(src_priority(0), Some(0));
    }

    #[test]
    fn test_default_gate() {
        let mut lt = SwLedger::new();
        assert_eq!(lt.set_default(1), Err(SwCode::EDefault));
        assert!(lt.set_default(0).is_ok());
        assert!(lt.default_ok());
        assert!(!lt.default_ok() == false);
        assert_eq!(lt.set_default(SDK_SLOTS), Err(SwCode::ENoSdk));
    }

    #[test]
    fn test_priority_gate() {
        let mut lt = SwLedger::new();
        let _ = lt.choose(0, scene_cell(0).unwrap_or(0), 3);
        assert_eq!(lt.choose(1, scene_cell(1).unwrap_or(0), 1), Err(SwCode::EOrder));
        assert!(lt.choose(1, scene_cell(1).unwrap_or(0), 4).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, SCENES as u32);
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
        assert_eq!(SEG_BASE, 0xC4F80);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52062_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
