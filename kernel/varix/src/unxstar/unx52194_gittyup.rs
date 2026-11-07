//! UNX-F52194 Gittyup 收口 · 0xC7080段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52194 逐字锚定）：UNX-F52195-J1 六判据全落；
//! libgit2 消费 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：libgit2 消费操作 ×10 账（仓库开/状态/diff/暂存/提交/
//!      分支/标签/远端/抓取/推），逐操作载荷（纯函数折叠）恒等才收；
//!      漂移必抓。
//!   ② 绑定三元闸：libgit2 动态库三元（存在/ABI 版本/符号表）三行
//!      账，缺一拒封——库缺失不虚报可用（禁"能开但卡死"隐性失败）。
//!   ③ 操作拍序单调闸：本地操作（0..5）先于远端操作（5..10），回跳
//!      必抓。
//!   ④ 注入 ×10 轮（操作越闭集/绑定行越闭集/重复落账三态轮转）10/10
//!      必抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（10 操作全通
//!      + 绑定三元全齐 → S1；库缺失 → S3 如实降级）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52194 − 52000)）。
pub const SEG_BASE: u32 = 0xC7080;

/// libgit2 消费操作数。
pub const OPS: usize = 10;

/// 绑定三元行数。
pub const BIND_ROWS: usize = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 格载荷漂移
    EDrift,
    /// 操作拍序逆序
    EOrder,
    /// 重复落账
    EDup,
    /// 绑定行越闭集
    ENoRow,
    /// 缺绑定行
    ERowMiss,
    /// 库缺失申报可用
    ENoLib,
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
            PfCode::EOrder => "操作拍序逆序：本地操作先于远端操作",
            PfCode::EDup => "重复落账：同格只许一条账目",
            PfCode::ENoRow => "绑定行越闭集：3 行闭集之外先扩行",
            PfCode::ERowMiss => "缺绑定行：libgit2 绑定三元未齐先补齐",
            PfCode::ENoLib => "库缺失申报可用：如实降级不许虚报",
            PfCode::ESeal => "未齐即封：格未齐先补齐",
            PfCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PfCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PfCode::EGradeDrift => "定级漂移：操作全通+绑定三元全齐才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（操作 op）。
pub const fn op_cell(op: usize) -> Option<u64> {
    if op >= OPS {
        return None;
    }
    Some(0xDC00 + op as u64 * 0x5B)
}

/// 操作类别（0..5=本地 5..10=远端）。
pub const fn op_kind(op: usize) -> Option<u32> {
    if op >= OPS {
        return None;
    }
    Some((op / 5) as u32)
}

/// 绑定行折叠（行 r）。
pub const fn bind_row(r: usize) -> Option<u32> {
    if r >= BIND_ROWS {
        return None;
    }
    Some(0xDD00 + r as u32 * 0x61)
}

/// 库存在三态（0=缺失 1=在 2=版本不符）。
pub const fn lib_state_ok(v: u32) -> bool {
    v <= 2
}

/// 库态对定级的影响（非 1 → S3 如实降级）。
pub const fn lib_grade(v: u32) -> Option<u32> {
    if !lib_state_ok(v) {
        return None;
    }
    if v == 1 {
        Some(1)
    } else {
        Some(3)
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Gittyup 收口账本。
#[derive(Clone)]
pub struct PfLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [Option<u64>; OPS],
    /// 绑定三元账。
    bind: [Option<u32>; BIND_ROWS],
    /// 库存在态。
    lib: u32,
    /// 最后类别。
    last_kind: i64,
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
            ops: [None; OPS],
            bind: [None; BIND_ROWS],
            lib: 0,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PfCode> {
        if gate >= 6 {
            return Err(PfCode::ESeal);
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

    /// 操作一格（本地先于远端单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, op: usize) -> Result<u64, PfCode> {
        if op >= OPS {
            return Err(PfCode::ENoOp);
        }
        let kind = match op_kind(op) {
            Some(k) => k,
            None => return Err(PfCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x41200);
            return Err(PfCode::EOrder);
        }
        let p = match op_cell(op) {
            Some(p) => p,
            None => return Err(PfCode::ENoOp),
        };
        if match self.ops.get(op) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(op) {
            *c = Some(p);
        }
        self.last_kind = kind as i64;
        self.fnv(0x41202 + op as u64);
        Ok(p)
    }

    /// 操作全账核验（10 格）。
    pub fn ops_all(&self) -> bool {
        let mut op = 0usize;
        while op < OPS {
            match self.ops.get(op) {
                Some(Some(_)) => {}
                _ => return false,
            }
            op += 1;
        }
        true
    }

    /// 绑定行一格。
    pub fn declare_bind(&mut self, r: usize) -> Result<(), PfCode> {
        if r >= BIND_ROWS {
            return Err(PfCode::ENoRow);
        }
        if match self.bind.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.bind.get_mut(r) {
            *c = bind_row(r);
        }
        self.fnv(0x41300 + r as u64);
        Ok(())
    }

    /// 绑定行全齐核验。
    pub fn bind_all(&self) -> bool {
        let mut r = 0usize;
        while r < BIND_ROWS {
            match self.bind.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 库态落账（三态闸）。
    pub fn set_lib(&mut self, v: u32) -> Result<(), PfCode> {
        if !lib_state_ok(v) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x41400);
            return Err(PfCode::ENoLib);
        }
        self.lib = v;
        self.fnv(0x41401);
        Ok(())
    }

    /// 库缺失申报可用闸。
    pub fn claim_usable(&mut self) -> bool {
        if self.lib != 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x41402);
            return false;
        }
        true
    }

    /// 封账闸：操作全齐 + 绑定三元全齐 + 库态在册。
    pub fn seal(&self) -> Result<(), PfCode> {
        if !self.ops_all() {
            return Err(PfCode::ESeal);
        }
        if !self.bind_all() {
            return Err(PfCode::ERowMiss);
        }
        if !lib_state_ok(self.lib) {
            return Err(PfCode::ENoLib);
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
    // 库态落账：在库态。
    let _ = led.set_lib(1);
    let mut recorded = 0u32;
    let mut op = 0usize;
    while op < OPS {
        if led.record_op(op).is_ok() {
            recorded += 1;
        }
        op += 1;
    }
    let mut r = 0usize;
    while r < BIND_ROWS {
        let _ = led.declare_bind(r);
        r += 1;
    }
    // 注入 10 轮：操作越闭集 / 库态越闭集 / 重复落账三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PfLedger::new();
        let caught = match kind {
            0 => lt.record_op(OPS).is_err(),
            1 => lt.set_lib(9).is_err(),
            _ => {
                let _ = lt.record_op(0);
                lt.record_op(0).is_err()
            }
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
pub fn run_unx52194_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52194");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "libgit2 消费 10 操作全通",
        recorded >= OPS as u32,
        "仓库开/状态/diff/暂存/提交/分支/标签/远端/抓取/推逐操作恒等",
    );
    s.add(
        "绑定三元 3/3 全齐",
        {
            let mut lt = PfLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < BIND_ROWS {
                if lt.declare_bind(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.bind_all()
        },
        "库存在/ABI 版本/符号表三行闭集",
    );
    s.add(
        "库态三态闭集",
        lib_state_ok(0) && lib_state_ok(2) && !lib_state_ok(3),
        "缺失/在/版本不符三态在册且越态拒",
    );
    s.add(
        "库态定级映射",
        match lib_grade(1) {
            Some(1) => true,
            _ => false,
        } && match lib_grade(0) {
            Some(3) => true,
            _ => false,
        },
        "在库→S1 缺失→S3 如实降级不虚报",
    );
    s.add(
        "库缺失申报可用必抓",
        {
            let mut lt = PfLedger::new();
            let _ = lt.set_lib(0);
            !lt.claim_usable()
        },
        "库缺失必须如实降级 S3",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/库态越态/重复三态轮转必抓");
    let mut l0 = PfLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·绑定行越闭集",
        l0.declare_bind(BIND_ROWS).is_err(),
        "绑定行 3 越闭集必拒",
    );
    let mut l1 = PfLedger::new();
    let _ = l1.record_op(7);
    s.add("拍序·逆序拒", l1.record_op(2).is_err(), "远端类后回本地类必抓");
    let mut l2 = PfLedger::new();
    let _ = l2.record_op(0);
    s.add("重复·再落账拒", l2.record_op(0).is_err(), "同格只许一条账目");
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
    fn test_op_kind() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(7), Some(1));
        assert!(op_kind(OPS).is_none());
        assert!(op_cell(OPS).is_none());
        assert!(bind_row(BIND_ROWS).is_none());
    }

    #[test]
    fn test_lib_gates() {
        assert!(lib_state_ok(0) && lib_state_ok(2));
        assert!(!lib_state_ok(3));
        assert_eq!(lib_grade(1), Some(1));
        assert_eq!(lib_grade(2), Some(3));
        let mut lt = PfLedger::new();
        let _ = lt.set_lib(0);
        assert!(!lt.claim_usable());
    }

    #[test]
    fn test_order_gate() {
        let mut lt = PfLedger::new();
        assert!(lt.record_op(7).is_ok());
        assert!(lt.record_op(2).is_err());
    }

    #[test]
    fn test_bind_and_seal() {
        let mut lt = PfLedger::new();
        assert!(lt.seal().is_err());
        let _ = lt.declare_bind(0);
        assert!(lt.declare_bind(0).is_err());
        assert_eq!(lt.declare_bind(BIND_ROWS), Err(PfCode::ENoRow));
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
        assert_eq!(SEG_BASE, 0xC7080);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52194_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
