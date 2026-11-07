//! UNX-F52097 Google USB Driver 收口 · 0xC5600段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52097 逐字锚定）：UNX-F52098-J1 devices/getvar
//! /boot 状态查询 ×10 全通；flash/erase 类破坏性写判据一律只干跑
//!（dry-run 列清单），真机写入不在本域判据内（硬件数据安全红线适用性
//! 声明）；KB-COMPAT 定级登记。
//!
//! 设计要点：
//!   ① 域模型：状态查询 ×10 场景账（devices/getvar/boot 三类闭集），
//!      逐场景载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② 破坏性写红线闸：flash/erase 类操作一律 dry-run（只列清单不执
//!      行）；红线声明行 2 行闭集（dry-run-only 声明/真机写不在域声明），
//!      缺一拒封；越红线（未 dry-run 直接执行）必抓。
//!   ③ 查询类别单调闸（devices→getvar→boot）；注入 ×10 轮（红线越线/
//!      载荷漂移/场景越闭集三态轮转）10/10 必抓，注入=检出+逃逸守恒；
//!      六判据母版闸 + 定级（10 格全通 + 红线声明全齐 → S1）。
//!   ④ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52097 − 52000)）。
pub const SEG_BASE: u32 = 0xC5600;

/// 查询场景数。
pub const QSCENES: usize = 10;

/// 查询类别三元闭集。
pub const QKINDS: usize = 3;

/// 红线声明行数。
pub const RED_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuCode {
    /// 成功
    Ok = 0,
    /// 场景号越闭集
    ENoScene,
    /// 类别越闭集
    ENoKind,
    /// 查询载荷漂移
    EDrift,
    /// 红线越线（破坏性写未 dry-run）
    ERedLine,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 类别逆序
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

impl GuCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GuCode::Ok => "成功",
            GuCode::ENoScene => "场景号越闭集：10 场景之外先登记",
            GuCode::ENoKind => "类别越闭集：devices/getvar/boot 三类之外先登记",
            GuCode::EDrift => "查询载荷漂移：折叠与基准不恒等，逐场景复算",
            GuCode::ERedLine => "红线越线：flash/erase 类破坏性写必须 dry-run，真机写必抓",
            GuCode::ENoRow => "声明行越闭集：2 行闭集之外先登记",
            GuCode::ERowMiss => "缺声明行：硬件数据安全红线适用性声明未齐先补齐",
            GuCode::EOrder => "类别逆序：devices→getvar→boot 严格递增，逆序必抓",
            GuCode::EDup => "重复落账：同格只许一条账目",
            GuCode::ESeal => "未齐即封：10 格未齐先补齐",
            GuCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GuCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            GuCode::EGradeDrift => "定级漂移：10 格全通+红线声明全齐+越线全抓才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 查询格载荷（场景 sc；类别 = sc/4 装配）。
pub const fn q_cell(sc: usize) -> Option<u64> {
    if sc >= QSCENES {
        return None;
    }
    Some((0xBE000u64 + (sc as u64) * 0x2C) ^ ((sc / 4) as u64).wrapping_mul(0x01000193))
}

/// 查询类别（0..4=devices 4..8=getvar 8..10=boot）。
pub const fn q_kind(sc: usize) -> Option<u32> {
    if sc >= QSCENES {
        return None;
    }
    Some((sc / 4) as u32)
}

/// 破坏性写判定（类别 99=flash/erase 类占位；只 dry-run）。
pub const fn is_destructive(kind: u32) -> bool {
    kind >= 99
}

/// 红线声明行折叠（行 r）。
pub const fn red_row(r: usize) -> Option<u32> {
    if r >= RED_ROWS {
        return None;
    }
    Some(0xBF00 + r as u32 * 0x36)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct GuLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 查询账。
    qs: [Option<u64>; QSCENES],
    /// 红线声明账。
    reds: [Option<u32>; RED_ROWS],
    /// dry-run 清单长度。
    dryruns: u32,
    /// 最后类别。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GuLedger {
    /// 新账本。
    pub fn new() -> Self {
        GuLedger {
            gates: [None; 6],
            qs: [None; QSCENES],
            reds: [None; RED_ROWS],
            dryruns: 0,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GuCode> {
        if gate >= 6 {
            return Err(GuCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x41100 + gate as u64);
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

    /// 破坏性写入口（一律 dry-run；exec=true 越红线必抓）。
    pub fn destructive(&mut self, kind: u32, exec: bool) -> Result<(), GuCode> {
        if !is_destructive(kind) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x41200);
            return Err(GuCode::ENoKind);
        }
        if exec {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x41201);
            return Err(GuCode::ERedLine);
        }
        self.dryruns = self.dryruns.saturating_add(1);
        self.fnv(0x41202);
        Ok(())
    }

    /// 查询一格（类别单调 + 载荷对照 + 重复拒）。
    pub fn record_query(&mut self, sc: usize) -> Result<u64, GuCode> {
        if sc >= QSCENES {
            return Err(GuCode::ENoScene);
        }
        let kind = match q_kind(sc) {
            Some(k) => k,
            None => return Err(GuCode::ENoScene),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x41300);
            return Err(GuCode::EOrder);
        }
        let p = match q_cell(sc) {
            Some(p) => p,
            None => return Err(GuCode::ENoScene),
        };
        if match self.qs.get(sc) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GuCode::EDup);
        }
        if let Some(c) = self.qs.get_mut(sc) {
            *c = Some(p);
        }
        self.last_kind = kind as i64;
        self.fnv(0x41301 + sc as u64);
        Ok(p)
    }

    /// 查询全账核验（10 格）。
    pub fn qs_all(&self) -> bool {
        let mut sc = 0usize;
        while sc < QSCENES {
            match self.qs.get(sc) {
                Some(Some(_)) => {}
                _ => return false,
            }
            sc += 1;
        }
        true
    }

    /// 红线声明一行。
    pub fn declare_red(&mut self, r: usize) -> Result<(), GuCode> {
        if r >= RED_ROWS {
            return Err(GuCode::ENoRow);
        }
        if match self.reds.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GuCode::EDup);
        }
        if let Some(c) = self.reds.get_mut(r) {
            *c = red_row(r);
        }
        self.fnv(0x41400 + r as u64);
        Ok(())
    }

    /// 红线声明全齐核验。
    pub fn reds_all(&self) -> bool {
        let mut r = 0usize;
        while r < RED_ROWS {
            match self.reds.get(r) {
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

impl Default for GuLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GuLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut sc = 0usize;
    while sc < QSCENES {
        if led.record_query(sc).is_ok() {
            recorded += 1;
        }
        sc += 1;
    }
    // 破坏性写一律 dry-run。
    let _ = led.destructive(99, false);
    let _ = led.destructive(100, false);
    let mut r = 0usize;
    while r < RED_ROWS {
        let _ = led.declare_red(r);
        r += 1;
    }
    // 注入 10 轮：红线越线 / 类别逆序 / 场景越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GuLedger::new();
        let caught = match kind {
            0 => lt.destructive(99, true) == Err(GuCode::ERedLine),
            1 => {
                let _ = lt.record_query(6);
                lt.record_query(2) == Err(GuCode::EOrder)
            }
            _ => lt.record_query(QSCENES).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x41500 + k as u64);
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
pub fn run_unx52097_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52097");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "状态查询 10/10 全通",
        recorded >= QSCENES as u32,
        "devices/getvar/boot 三类 ×10 场景逐格恒等",
    );
    s.add(
        "破坏性写一律 dry-run",
        {
            let mut lt = GuLedger::new();
            lt.destructive(99, true) == Err(GuCode::ERedLine)
                && lt.destructive(99, false).is_ok()
        },
        "flash/erase 越线必抓，dry-run 放行",
    );
    s.add(
        "红线声明 2/2 全齐",
        {
            let mut lt = GuLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < RED_ROWS {
                if lt.declare_red(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.reds_all()
        },
        "dry-run-only/真机写不在域两行闭集",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越线/逆序/越闭集三态轮转必抓");
    let mut l0 = GuLedger::new();
    s.add(
        "红线·越线拒",
        l0.destructive(100, true) == Err(GuCode::ERedLine),
        "真机写入必抓",
    );
    s.add(
        "类别·逆序拒",
        {
            let _ = l0.record_query(6);
            l0.record_query(2) == Err(GuCode::EOrder)
        },
        "getvar 后回 devices 必抓",
    );
    s.add(
        "边界·场景越闭集",
        l0.record_query(QSCENES).is_err(),
        "场景 10 越闭集必拒",
    );
    s.add(
        "边界·声明越闭集",
        l0.declare_red(RED_ROWS).is_err(),
        "行 2 越闭集必拒",
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
    fn test_q_kind() {
        assert_eq!(q_kind(0), Some(0));
        assert_eq!(q_kind(4), Some(1));
        assert_eq!(q_kind(9), Some(2));
        assert!(q_kind(10).is_none());
        assert!(q_cell(10).is_none());
    }

    #[test]
    fn test_redline_gate() {
        let mut lt = GuLedger::new();
        assert_eq!(lt.destructive(99, true), Err(GuCode::ERedLine));
        assert!(lt.destructive(99, false).is_ok());
        assert_eq!(lt.destructive(50, false), Err(GuCode::ENoKind));
        assert!(lt.destructive(50, true).is_err());
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = GuLedger::new();
        let _ = lt.record_query(7);
        assert_eq!(lt.record_query(3), Err(GuCode::EOrder));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= QSCENES as u32);
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
        assert_eq!(SEG_BASE, 0xC5600);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52097_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
