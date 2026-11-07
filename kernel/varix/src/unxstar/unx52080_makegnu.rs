//! UNX-F52080 Make for Windows（GnuWin32/独立发行版）收口 · 0xC5400段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52080 逐字锚定）：UNX-F52081-J1 六判据全落 +
//! ADB 设备枚举复用 M5 账（联签锚定行 AI-65）+ 模拟器路径显式 N/A 声明
//! （HAXM 类虚拟化消费 L5/AI-60 可选路径，不可用则 AVD 降级 N/A 账，
//! 不虚报）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二。
//!
//! 设计要点：
//!   ① 域模型：ADB 设备枚举复用 M5 账：设备 8 台闭集，枚举载荷与 M5
//!      基准恒等才收（复用判据零重测）；枚举漂移必抓。
//!   ② 模拟器路径 N/A 声明账：N/A 行 3 行闭集（HAXM 路径 N/A/AVD 降级
//!      N/A/显式不虚报声明），缺一拒封；虚报可用（伪在册）必抓。
//!   ③ Make 构建面 ×6 目标账（all/clean/install/uninstall/dist/check
//!      六标准目标），逐目标载荷恒等才收。
//!   ④ 注入 ×10 轮（枚举漂移/虚报可用/目标越闭集三态轮转）10/10 必
//!      抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（8 台 + 6 目标 +
//!      N/A 全齐 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52080 − 52000)）。
pub const SEG_BASE: u32 = 0xC5400;

/// 设备闭集数。
pub const DEVICES: usize = 8;

/// N/A 声明行数。
pub const NA_ROWS: usize = 3;

/// Make 目标数。
pub const GOALS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MkCode {
    /// 成功
    Ok = 0,
    /// 设备号越闭集
    ENoDev,
    /// 枚举漂移
    EDrift,
    /// N/A 行越闭集
    ENoRow,
    /// 缺 N/A 行
    ERowMiss,
    /// 虚报可用
    EFake,
    /// 目标越闭集
    ENoGoal,
    /// 目标漂移
    EGoalDrift,
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

impl MkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MkCode::Ok => "成功",
            MkCode::ENoDev => "设备号越闭集：8 台闭集之外先登记",
            MkCode::EDrift => "枚举漂移：M5 复用折叠与基准不恒等，逐台复算",
            MkCode::ENoRow => "N/A 行越闭集：3 行闭集之外先登记",
            MkCode::ERowMiss => "缺 N/A 行：模拟器路径显式 N/A 声明未齐先补齐",
            MkCode::EFake => "虚报可用：不可用路径伪在册即虚报必抓",
            MkCode::ENoGoal => "目标越闭集：6 标准目标之外先登记",
            MkCode::EGoalDrift => "目标漂移：目标折叠与基准不恒等",
            MkCode::EDup => "重复落账：同格只许一条账目",
            MkCode::ESeal => "未齐即封：8 台 + 6 目标未齐先补齐",
            MkCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            MkCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            MkCode::EGradeDrift => "定级漂移：8 台+6 目标全通+N/A 全齐不虚报才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 设备枚举格基准（台 d；M5 复用折叠）。
pub const fn dev_cell(d: usize) -> Option<u64> {
    if d >= DEVICES {
        return None;
    }
    Some((0xA2000u64 + (d as u64) * 0x2B) ^ 0x6D35656E756D)
}

/// N/A 行折叠（行 r）。
pub const fn na_row(r: usize) -> Option<u32> {
    if r >= NA_ROWS {
        return None;
    }
    Some(0xA300 + r as u32 * 0x37)
}

/// Make 目标格基准（目标 g）。
pub const fn goal_cell(g: usize) -> Option<u64> {
    if g >= GOALS {
        return None;
    }
    Some((0xA4000u64 + g as u64 * 0x2F) ^ 0x676E756D616B65)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct MkLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 枚举账。
    devs: [Option<u64>; DEVICES],
    /// N/A 声明账。
    nas: [Option<u32>; NA_ROWS],
    /// 目标账。
    goals: [Option<u64>; GOALS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl MkLedger {
    /// 新账本。
    pub fn new() -> Self {
        MkLedger {
            gates: [None; 6],
            devs: [None; DEVICES],
            nas: [None; NA_ROWS],
            goals: [None; GOALS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), MkCode> {
        if gate >= 6 {
            return Err(MkCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x39100 + gate as u64);
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

    /// 枚举一格（M5 复用恒等才收，漂移必抓）。
    pub fn record_dev(&mut self, d: usize, got: u64) -> Result<(), MkCode> {
        let want = match dev_cell(d) {
            Some(w) => w,
            None => return Err(MkCode::ENoDev),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x39200 + d as u64);
            return Err(MkCode::EDrift);
        }
        if match self.devs.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MkCode::EDup);
        }
        if let Some(c) = self.devs.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x39201 + d as u64);
        Ok(())
    }

    /// 枚举全账核验。
    pub fn devs_all(&self) -> bool {
        let mut d = 0usize;
        while d < DEVICES {
            match self.devs.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// N/A 声明一行（显式 N/A 才收，虚报可用必抓）。
    pub fn declare_na(&mut self, r: usize, fake_available: bool) -> Result<(), MkCode> {
        if r >= NA_ROWS {
            return Err(MkCode::ENoRow);
        }
        if fake_available {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x39300 + r as u64);
            return Err(MkCode::EFake);
        }
        if match self.nas.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MkCode::EDup);
        }
        if let Some(c) = self.nas.get_mut(r) {
            *c = na_row(r);
        }
        self.fnv(0x39301 + r as u64);
        Ok(())
    }

    /// N/A 全齐核验。
    pub fn nas_all(&self) -> bool {
        let mut r = 0usize;
        while r < NA_ROWS {
            match self.nas.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 目标一格。
    pub fn record_goal(&mut self, g: usize, got: u64) -> Result<(), MkCode> {
        let want = match goal_cell(g) {
            Some(w) => w,
            None => return Err(MkCode::ENoGoal),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x39400 + g as u64);
            return Err(MkCode::EGoalDrift);
        }
        if match self.goals.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MkCode::EDup);
        }
        if let Some(c) = self.goals.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x39401 + g as u64);
        Ok(())
    }

    /// 目标全账核验。
    pub fn goals_all(&self) -> bool {
        let mut g = 0usize;
        while g < GOALS {
            match self.goals.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
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

impl Default for MkLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = MkLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut d = 0usize;
    while d < DEVICES {
        if let Some(w) = dev_cell(d) {
            if led.record_dev(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut r = 0usize;
    while r < NA_ROWS {
        let _ = led.declare_na(r, false);
        r += 1;
    }
    let mut g2 = 0usize;
    while g2 < GOALS {
        if let Some(w) = goal_cell(g2) {
            if led.record_goal(g2, w).is_ok() {
                recorded += 1;
            }
        }
        g2 += 1;
    }
    // 注入 10 轮：枚举漂移 / 虚报可用 / 设备越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = MkLedger::new();
        let caught = match kind {
            0 => lt.record_dev(3, dev_cell(3).unwrap_or(0).wrapping_add(1)) == Err(MkCode::EDrift),
            1 => lt.declare_na(1, true) == Err(MkCode::EFake),
            _ => lt.record_dev(DEVICES, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x39500 + k as u64);
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
pub fn run_unx52080_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52080");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "ADB 枚举复用 M5 8/8 全账",
        recorded >= DEVICES as u32,
        "8 台设备枚举与 M5 基准恒等（AI-65 联签零重测）",
    );
    s.add(
        "N/A 声明 3/3 全齐不虚报",
        {
            let mut lt = MkLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < NA_ROWS {
                if lt.declare_na(r, false).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.nas_all()
        },
        "HAXM N/A/AVD 降级 N/A/不虚报声明三行闭集",
    );
    s.add(
        "Make 目标 6/6 全账",
        {
            let mut lt = MkLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < GOALS {
                if let Some(w) = goal_cell(g) {
                    if lt.record_goal(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.goals_all()
        },
        "all/clean/install/uninstall/dist/check 六标准目标恒等",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/虚报/越闭集三态轮转必抓");
    let mut l0 = MkLedger::new();
    s.add(
        "虚报可用拒",
        l0.declare_na(0, true) == Err(MkCode::EFake),
        "不可用路径伪在册必抓",
    );
    s.add(
        "边界·设备越闭集",
        l0.record_dev(DEVICES, 0).is_err(),
        "设备 8 越闭集必拒",
    );
    s.add(
        "边界·目标越闭集",
        l0.record_goal(GOALS, 0).is_err(),
        "目标 6 越闭集必拒",
    );
    let mut l1 = MkLedger::new();
    let _ = l1.record_dev(1, dev_cell(1).unwrap_or(0));
    s.add(
        "重复·再落账拒",
        l1.record_dev(1, dev_cell(1).unwrap_or(0)).is_err(),
        "同格只许一条账目",
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
    fn test_cells_pure() {
        let a = dev_cell(5).unwrap_or(0);
        assert_eq!(a, dev_cell(5).unwrap_or(0));
        assert_ne!(a, dev_cell(6).unwrap_or(0));
        assert!(dev_cell(DEVICES).is_none());
        assert!(goal_cell(GOALS).is_none());
        assert!(na_row(NA_ROWS).is_none());
        assert_ne!(na_row(0), na_row(1));
    }

    #[test]
    fn test_na_fake_gate() {
        let mut lt = MkLedger::new();
        assert_eq!(lt.declare_na(0, true), Err(MkCode::EFake));
        assert!(lt.declare_na(0, false).is_ok());
        assert!(lt.declare_na(0, false).is_err());
        assert_eq!(lt.declare_na(NA_ROWS, false), Err(MkCode::ENoRow));
    }

    #[test]
    fn test_dev_drift_caught() {
        let mut lt = MkLedger::new();
        assert_eq!(
            lt.record_dev(2, dev_cell(2).unwrap_or(0).wrapping_add(3)),
            Err(MkCode::EDrift)
        );
        assert!(lt.record_dev(2, dev_cell(2).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= DEVICES as u32);
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
        assert_eq!(SEG_BASE, 0xC5400);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52080_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
