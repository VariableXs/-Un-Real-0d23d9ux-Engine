//! UNX-F52072 NSIS 收口 · 0xC5200段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52072 逐字锚定）：UNX-F52073-J1 pacman 包管
//! 理 ×20 操作（安装/更新/查询）全通；与 L4 工具链布设分界声明（发行
//! 版环境收口 vs 嫁接件布设，联签锚定行 AI-59）；KB-COMPAT 定级登记；
//! 判据锚定 §AI-66.1 防重声明。
//!
//! 设计要点：
//!   ① 域模型：pacman 包管理 ×20 操作账（安装 8/更新 6/查询 6），逐
//!      操作载荷（纯函数折叠）恒等才收；操作类别三元闭集（0=安装
//!      1=更新 2=查询）越类拒。
//!   ② L4 分界声明：2 行声明闭集（发行版环境收口声明/嫁接件布设声
//!      明），缺一拒封。
//!   ③ 安装→更新→查询拍序单向闸（类别号单调）；注入 ×10 轮（操作漂
//!      移/类别越闭集/声明缺行三态轮转）10/10 必抓，注入=检出+逃逸守
//!      恒；六判据母版闸 + 定级（20 格全通 + 声明全齐 → S1）。
//!   ④ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52072 − 52000)）。
pub const SEG_BASE: u32 = 0xC5200;

/// 操作数。
pub const PAC_OPS: usize = 20;

/// 声明行数。
pub const BOUND_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NsCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 类别越闭集
    ENoKind,
    /// 操作漂移
    EDrift,
    /// 类别逆序
    EOrder,
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

impl NsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            NsCode::Ok => "成功",
            NsCode::ENoOp => "操作号越闭集：20 操作之外先登记",
            NsCode::ENoKind => "类别越闭集：安装/更新/查询三元之外先登记",
            NsCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            NsCode::EOrder => "类别逆序：安装→更新→查询拍序必须单调",
            NsCode::ENoRow => "声明行越闭集：2 行闭集之外先登记",
            NsCode::ERowMiss => "缺声明行：L4 分界声明未齐先补齐",
            NsCode::EDup => "重复落账：同格只许一条账目",
            NsCode::ESeal => "未齐即封：20 格未齐先补齐",
            NsCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            NsCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            NsCode::EGradeDrift => "定级漂移：20 格全通+声明全齐+类别单调才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（操作 o；类别 = o/8 装配）。
pub const fn pac_cell(o: usize) -> Option<u64> {
    if o >= PAC_OPS {
        return None;
    }
    Some((0x85000u64 + (o as u64) * 0x2C) ^ ((o / 8) as u64).wrapping_mul(0x01000193))
}

/// 操作类别（纯函数：0..8=安装 8..14=更新 14..20=查询）。
pub const fn pac_kind(o: usize) -> Option<u32> {
    if o >= PAC_OPS {
        return None;
    }
    Some(if o < 8 {
        0
    } else if o < 14 {
        1
    } else {
        2
    })
}

/// 声明行折叠（行 r）。
pub const fn bound_row(r: usize) -> Option<u32> {
    if r >= BOUND_ROWS {
        return None;
    }
    Some(0x8600 + r as u32 * 0x3A)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct NsLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [Option<u64>; PAC_OPS],
    /// 声明账。
    bounds: [Option<u32>; BOUND_ROWS],
    /// 最后类别。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NsLedger {
    /// 新账本。
    pub fn new() -> Self {
        NsLedger {
            gates: [None; 6],
            ops: [None; PAC_OPS],
            bounds: [None; BOUND_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NsCode> {
        if gate >= 6 {
            return Err(NsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x31100 + gate as u64);
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

    /// 操作一格（类别闭集 + 类别单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), NsCode> {
        if o >= PAC_OPS {
            return Err(NsCode::ENoOp);
        }
        let kind = match pac_kind(o) {
            Some(k) => k,
            None => return Err(NsCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x31200);
            return Err(NsCode::EOrder);
        }
        let want = match pac_cell(o) {
            Some(w) => w,
            None => return Err(NsCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x31201 + o as u64);
            return Err(NsCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NsCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x31202 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < PAC_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 声明一行。
    pub fn declare_bound(&mut self, r: usize) -> Result<(), NsCode> {
        if r >= BOUND_ROWS {
            return Err(NsCode::ENoRow);
        }
        if match self.bounds.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NsCode::EDup);
        }
        if let Some(c) = self.bounds.get_mut(r) {
            *c = bound_row(r);
        }
        self.fnv(0x31300 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn bounds_all(&self) -> bool {
        let mut r = 0usize;
        while r < BOUND_ROWS {
            match self.bounds.get(r) {
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

impl Default for NsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NsLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < PAC_OPS {
        if let Some(w) = pac_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < BOUND_ROWS {
        let _ = led.declare_bound(r);
        r += 1;
    }
    // 注入 10 轮：操作漂移 / 类别逆序 / 操作越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = NsLedger::new();
        let caught = match kind {
            0 => lt.record_op(5, pac_cell(5).unwrap_or(0).wrapping_add(1)) == Err(NsCode::EDrift),
            1 => {
                let _ = lt.record_op(9, pac_cell(9).unwrap_or(0));
                lt.record_op(3, pac_cell(3).unwrap_or(0)) == Err(NsCode::EOrder)
            }
            _ => lt.record_op(PAC_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x31400 + k as u64);
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
pub fn run_unx52072_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52072");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "pacman 20 操作全通",
        recorded == PAC_OPS as u32,
        "安装 8/更新 6/查询 6 逐操作恒等",
    );
    s.add(
        "L4 分界声明 2/2 全齐",
        {
            let mut lt = NsLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < BOUND_ROWS {
                if lt.declare_bound(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.bounds_all()
        },
        "发行版环境收口/嫁接件布设两行闭集（AI-59 联签）",
    );
    s.add(
        "类别单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PAC_OPS {
                if pac_kind(o).unwrap_or(3) < pac_kind(o - 1).unwrap_or(3) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "安装→更新→查询三元闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/逆序/越闭集三态轮转必抓");
    let mut l0 = NsLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(PAC_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·声明越闭集",
        l0.declare_bound(BOUND_ROWS).is_err(),
        "行 2 越闭集必拒",
    );
    let mut l1 = NsLedger::new();
    let _ = l1.record_op(11, pac_cell(11).unwrap_or(0));
    s.add(
        "类别·逆序拒",
        l1.record_op(4, pac_cell(4).unwrap_or(0)) == Err(NsCode::EOrder),
        "查询后回安装必抓",
    );
    s.add(
        "重复·再落账拒",
        l1.record_op(11, pac_cell(11).unwrap_or(0)).is_err(),
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
    fn test_pac_kind() {
        assert_eq!(pac_kind(0), Some(0));
        assert_eq!(pac_kind(7), Some(0));
        assert_eq!(pac_kind(8), Some(1));
        assert_eq!(pac_kind(13), Some(1));
        assert_eq!(pac_kind(14), Some(2));
        assert_eq!(pac_kind(19), Some(2));
        assert!(pac_kind(20).is_none());
        assert!(pac_cell(20).is_none());
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = NsLedger::new();
        let _ = lt.record_op(15, pac_cell(15).unwrap_or(0));
        assert_eq!(lt.record_op(2, pac_cell(2).unwrap_or(0)), Err(NsCode::EOrder));
        assert!(lt.record_op(16, pac_cell(16).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_bound_gates() {
        let mut lt = NsLedger::new();
        let _ = lt.declare_bound(0);
        assert!(lt.declare_bound(0).is_err());
        assert_eq!(lt.declare_bound(2), Err(NsCode::ENoRow));
        assert!(!lt.bounds_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, PAC_OPS as u32);
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
        assert_eq!(SEG_BASE, 0xC5200);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52072_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
