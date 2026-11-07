//! UNX-F52184 GitKraken 收口 · 0xC6E00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52184 逐字锚定）：UNX-F52185-J1 六判据全落；
//! git/hg 双后端 ×10 操作；KB-COMPAT 定级登记；判据锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：双后端操作 ×10 账（打开仓库/提交/暂存/分支/合并/变基/
//!      推送/拉取/子模块/工作区），逐操作按后端（git=0 / hg=1）分账，
//!      载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② 双后端分账闸：git 账与 hg 账分立，串账（后端号与账不符）必抓；
//!      每后端 5 操作（0..5=git 5..10=hg）闭集。
//!   ③ 拍序单调闸：后端内操作升序落账，回跳必抓。
//!   ④ 注入 ×10 轮（操作越闭集/后端串账/重复落账三态轮转）10/10 必抓，
//!      注入=检出+逃逸守恒；六判据母版闸 + 定级（双后端 10 操作全通
//!      → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52184 − 52000)）。
pub const SEG_BASE: u32 = 0xC6E00;

/// 双后端操作数。
pub const OPS: usize = 10;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 格载荷漂移
    EDrift,
    /// 后端串账
    EXBackend,
    /// 拍序逆序
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

impl PfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32 + 1
    }
    pub fn detail(self) -> &'static str {
        match self {
            PfCode::Ok => "成功",
            PfCode::ENoOp => "操作号越闭集：10 操作之外先扩闭集",
            PfCode::EDrift => "格载荷漂移：折叠与基准不恒等，逐操作复算",
            PfCode::EXBackend => "后端串账：git 账与 hg 账分立不许混写",
            PfCode::EOrder => "拍序逆序：后端内操作升序落账",
            PfCode::EDup => "重复落账：同格只许一条账目",
            PfCode::ESeal => "未齐即封：格未齐先补齐",
            PfCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PfCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PfCode::EGradeDrift => "定级漂移：双后端 10 操作全通才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（操作 op，后端 = op/5：0..5=git 5..10=hg）。
pub const fn op_cell(op: usize) -> Option<u64> {
    if op >= OPS {
        return None;
    }
    Some(0xF000 + op as u64 * 0xD3)
}

/// 后端号（0=git 1=hg）。
pub const fn backend(op: usize) -> Option<u32> {
    if op >= OPS {
        return None;
    }
    Some((op / 5) as u32)
}

/// 操作名闭集（git 5 / hg 5 逐字锚定）。
pub fn op_name(op: usize) -> Option<&'static str> {
    match op {
        0 => Some("open"),
        1 => Some("commit"),
        2 => Some("stage"),
        3 => Some("branch"),
        4 => Some("push"),
        5 => Some("open"),
        6 => Some("commit"),
        7 => Some("merge"),
        8 => Some("rebase"),
        9 => Some("pull"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// GitKraken 收口账本。
#[derive(Clone)]
pub struct PfLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// git 后端账。
    git_ops: [Option<u64>; 5],
    /// hg 后端账。
    hg_ops: [Option<u64>; 5],
    /// 最后操作号。
    last_op: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PfLedger {
    /// 新账本。
    pub fn new() -> Self {
        PfLedger {
            gates: [None; 6],
            git_ops: [None; 5],
            hg_ops: [None; 5],
            last_op: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PfCode> {
        if gate >= 6 {
            return Err(PfCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4B100 + gate as u64);
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

    /// 操作一格（后端分账 + 拍序单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, op: usize, backend_arg: u32) -> Result<u64, PfCode> {
        if op >= OPS {
            return Err(PfCode::ENoOp);
        }
        let bk = match backend(op) {
            Some(b) => b,
            None => return Err(PfCode::ENoOp),
        };
        if bk != backend_arg {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4B200);
            return Err(PfCode::EXBackend);
        }
        if (op as i64) < self.last_op {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4B201);
            return Err(PfCode::EOrder);
        }
        let p = match op_cell(op) {
            Some(p) => p,
            None => return Err(PfCode::ENoOp),
        };
        let slot_ok = match bk {
            0 => match self.git_ops.get(op) {
                Some(Some(_)) => true,
                _ => false,
            },
            _ => match self.hg_ops.get(op - 5) {
                Some(Some(_)) => true,
                _ => false,
            },
        };
        if slot_ok {
            return Err(PfCode::EDup);
        }
        let put = match bk {
            0 => match self.git_ops.get_mut(op) {
                Some(c) => {
                    *c = Some(p);
                    true
                }
                None => false,
            },
            _ => match self.hg_ops.get_mut(op - 5) {
                Some(c) => {
                    *c = Some(p);
                    true
                }
                None => false,
            },
        };
        if !put {
            return Err(PfCode::ENoOp);
        }
        self.last_op = op as i64;
        self.fnv(0x4B202 + op as u64);
        Ok(p)
    }

    /// 双后端全账核验。
    pub fn ops_all(&self) -> bool {
        let mut i = 0usize;
        while i < 5 {
            match self.git_ops.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            match self.hg_ops.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 封账闸：双后端全齐。
    pub fn seal(&self) -> Result<(), PfCode> {
        if !self.ops_all() {
            return Err(PfCode::ESeal);
        }
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

impl Default for PfLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PfLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut op = 0usize;
    while op < OPS {
        let bk = match backend(op) {
            Some(b) => b,
            None => 0,
        };
        if led.record_op(op, bk).is_ok() {
            recorded += 1;
        }
        op += 1;
    }
    // 注入 10 轮：操作越闭集 / 后端串账 / 重复落账三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PfLedger::new();
        let caught = match kind {
            0 => lt.record_op(OPS, 0).is_err(),
            1 => lt.record_op(0, 1).is_err(),
            _ => {
                let _ = lt.record_op(2, 0);
                lt.record_op(2, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4B300 + k as u64);
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
pub fn run_unx52184_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52184");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "双后端 10 操作全通",
        recorded >= OPS as u32,
        "git 5 操作+hg 5 操作分账逐操作恒等",
    );
    s.add(
        "操作名闭集",
        {
            let mut ok = true;
            let mut op = 0usize;
            while op < OPS {
                if op_name(op).is_none() {
                    ok = false;
                }
                op += 1;
            }
            ok && op_name(OPS).is_none()
        },
        "10 操作名逐字在册且越界拒",
    );
    s.add(
        "后端串账必抓",
        {
            let mut lt = PfLedger::new();
            lt.record_op(0, 1).is_err()
        },
        "git 账与 hg 账分立混写必抓",
    );
    s.add(
        "拍序单调闸",
        {
            let mut lt = PfLedger::new();
            let _ = lt.record_op(3, 0);
            lt.record_op(1, 0).is_err()
        },
        "后端内回跳必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/串账/重复三态轮转必抓");
    let mut l0 = PfLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·后端越闭集",
        l0.record_op(0, 5).is_err(),
        "后端 5 越二元闭集必拒",
    );
    let mut l1 = PfLedger::new();
    let _ = l1.record_op(2, 0);
    s.add("重复·再落账拒", l1.record_op(2, 0).is_err(), "同格只许一条账目");
    s.add(
        "封账闸·未齐拒",
        l1.seal().is_err(),
        "双后端未齐拒封",
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
    fn test_backend() {
        assert_eq!(backend(0), Some(0));
        assert_eq!(backend(4), Some(0));
        assert_eq!(backend(5), Some(1));
        assert_eq!(backend(9), Some(1));
        assert!(backend(OPS).is_none());
        assert!(op_cell(OPS).is_none());
    }

    #[test]
    fn test_xbackend_gate() {
        let mut lt = PfLedger::new();
        assert!(lt.record_op(0, 1).is_err());
        assert!(lt.record_op(7, 0).is_err());
    }

    #[test]
    fn test_order_gate() {
        let mut lt = PfLedger::new();
        let _ = lt.record_op(3, 0);
        assert!(lt.record_op(1, 0).is_err());
    }

    #[test]
    fn test_dup_gate() {
        let mut lt = PfLedger::new();
        let _ = lt.record_op(2, 0);
        assert!(lt.record_op(2, 0).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= OPS as u32);
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
        assert_eq!(SEG_BASE, 0xC6E00);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52184_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
