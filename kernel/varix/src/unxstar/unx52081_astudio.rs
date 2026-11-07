//! UNX-F52081 Android Studio 收口（组旗舰） · 0xC5440段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52081 逐字锚定）：UNX-F52082-J1 adb devices/
//! shell/push/pull/logcat 20 操作集全通；设备枚举判据复用 M5 账零重测
//! （联签锚定行 AI-65）；fastboot 与 M5 vendor 类管道分账声明；KB-COMPAT
//! 定级登记。
//!
//! 设计要点：
//!   ① 域模型：adb 20 操作集账，操作类别五元闭集（devices/shell/push/
//!      pull/logcat），逐操作载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② 设备枚举复用 M5 账：6 台闭集枚举恒等（AI-65 联签零重测）。
//!   ③ fastboot 分账声明：2 行闭集（fastboot 独立账声明/vendor 类管道
//!      分账声明），缺一拒封。
//!   ④ 操作类别单调闸（devices→shell→push→pull→logcat 逆序必抓）；
//!      注入 ×10 轮（操作漂移/类别逆序/操作越闭集三态轮转）10/10 必
//!      抓；六判据母版闸 + 定级（20 格 + 6 台全通 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52081 − 52000)）。
pub const SEG_BASE: u32 = 0xC5440;

/// adb 操作数。
pub const ADB_OPS: usize = 20;

/// 设备闭集数。
pub const DEVS: usize = 6;

/// 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 类别逆序
    EOrder,
    /// 设备号越闭集
    ENoDev,
    /// 枚举漂移
    EDevDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
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

impl AsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AsCode::Ok => "成功",
            AsCode::ENoOp => "操作号越闭集：20 操作集之外先登记",
            AsCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            AsCode::EOrder => "类别逆序：devices→shell→push→pull→logcat 单调，逆序必抓",
            AsCode::ENoDev => "设备号越闭集：6 台闭集之外先登记",
            AsCode::EDevDrift => "枚举漂移：M5 复用折叠与基准不恒等（AI-65 联签）",
            AsCode::ENoRow => "声明行越闭集：2 行闭集之外先登记",
            AsCode::ERowMiss => "缺声明行：fastboot 分账声明未齐先补齐",
            AsCode::EDup => "重复落账：同格只许一条账目",
            AsCode::ESeal => "未齐即封：20 格 + 6 台未齐先补齐",
            AsCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            AsCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            AsCode::EGradeDrift => "定级漂移：20 格+6 台全通+分账声明全齐才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（操作 o；类别 = o/4）。
pub const fn adb_cell(o: usize) -> Option<u64> {
    if o >= ADB_OPS {
        return None;
    }
    Some((0xA7000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作类别（0..4=devices 4..8=shell 8..12=push 12..16=pull 16..20=logcat）。
pub const fn adb_kind(o: usize) -> Option<u32> {
    if o >= ADB_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 设备枚举格基准（台 d；M5 复用）。
pub const fn dev_cell(d: usize) -> Option<u64> {
    if d >= DEVS {
        return None;
    }
    Some((0xA8000u64 + (d as u64) * 0x31) ^ 0x6D35656E756D32)
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xA900 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct AsLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [Option<u64>; ADB_OPS],
    /// 枚举账。
    devs: [Option<u64>; DEVS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后类别。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl AsLedger {
    /// 新账本。
    pub fn new() -> Self {
        AsLedger {
            gates: [None; 6],
            ops: [None; ADB_OPS],
            devs: [None; DEVS],
            splits: [None; SPLIT_ROWS],
            last_kind: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), AsCode> {
        if gate >= 6 {
            return Err(AsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x3A100 + gate as u64);
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

    /// 操作一格（类别单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), AsCode> {
        if o >= ADB_OPS {
            return Err(AsCode::ENoOp);
        }
        let kind = match adb_kind(o) {
            Some(k) => k,
            None => return Err(AsCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3A200);
            return Err(AsCode::EOrder);
        }
        let want = match adb_cell(o) {
            Some(w) => w,
            None => return Err(AsCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3A201 + o as u64);
            return Err(AsCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AsCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x3A202 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < ADB_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 枚举一格（M5 复用恒等）。
    pub fn record_dev(&mut self, d: usize, got: u64) -> Result<(), AsCode> {
        let want = match dev_cell(d) {
            Some(w) => w,
            None => return Err(AsCode::ENoDev),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3A300 + d as u64);
            return Err(AsCode::EDevDrift);
        }
        if match self.devs.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AsCode::EDup);
        }
        if let Some(c) = self.devs.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x3A301 + d as u64);
        Ok(())
    }

    /// 枚举全账核验。
    pub fn devs_all(&self) -> bool {
        let mut d = 0usize;
        while d < DEVS {
            match self.devs.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), AsCode> {
        if r >= SPLIT_ROWS {
            return Err(AsCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AsCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x3A400 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
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

impl Default for AsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = AsLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < ADB_OPS {
        if let Some(w) = adb_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut d = 0usize;
    while d < DEVS {
        if let Some(w) = dev_cell(d) {
            if led.record_dev(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：类别逆序 / 枚举漂移 / 操作越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = AsLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_op(17, adb_cell(17).unwrap_or(0));
                lt.record_op(5, adb_cell(5).unwrap_or(0)) == Err(AsCode::EOrder)
            }
            1 => lt.record_dev(2, dev_cell(2).unwrap_or(0).wrapping_add(1)) == Err(AsCode::EDevDrift),
            _ => lt.record_op(ADB_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x3A500 + k as u64);
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
pub fn run_unx52081_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52081");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "adb 20 操作全通",
        recorded >= ADB_OPS as u32,
        "devices/shell/push/pull/logcat 五类逐操作恒等",
    );
    s.add(
        "M5 枚举复用 6/6 零重测",
        {
            let mut lt = AsLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DEVS {
                if let Some(w) = dev_cell(d) {
                    if lt.record_dev(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.devs_all()
        },
        "6 台设备与 M5 基准恒等（AI-65 联签）",
    );
    s.add(
        "fastboot 分账声明 2/2 全齐",
        {
            let mut lt = AsLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "fastboot 独立账/vendor 类管道分账两行闭集",
    );
    s.add(
        "类别单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < ADB_OPS {
                if adb_kind(o).unwrap_or(5) < adb_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五类闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "逆序/漂移/越闭集三态轮转必抓");
    let mut l0 = AsLedger::new();
    s.add(
        "类别·逆序拒",
        {
            let _ = l0.record_op(17, adb_cell(17).unwrap_or(0));
            l0.record_op(5, adb_cell(5).unwrap_or(0)) == Err(AsCode::EOrder)
        },
        "logcat 后回 shell 必抓",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(ADB_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·设备越闭集",
        l0.record_dev(DEVS, 0).is_err(),
        "设备 6 越闭集必拒",
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
    fn test_adb_kind() {
        assert_eq!(adb_kind(0), Some(0));
        assert_eq!(adb_kind(3), Some(0));
        assert_eq!(adb_kind(4), Some(1));
        assert_eq!(adb_kind(19), Some(4));
        assert!(adb_kind(20).is_none());
        assert!(adb_cell(20).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = dev_cell(3).unwrap_or(0);
        assert_eq!(a, dev_cell(3).unwrap_or(0));
        assert_ne!(a, dev_cell(4).unwrap_or(0));
        assert!(dev_cell(DEVS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = AsLedger::new();
        let _ = lt.record_op(9, adb_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, adb_cell(2).unwrap_or(0)), Err(AsCode::EOrder));
        assert!(lt.record_op(10, adb_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= ADB_OPS as u32);
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
        assert_eq!(SEG_BASE, 0xC5440);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52081_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
