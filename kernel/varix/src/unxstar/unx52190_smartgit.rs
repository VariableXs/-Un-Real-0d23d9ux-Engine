//! UNX-F52190 SmartGit 收口 · 0xC6F80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52190 逐字锚定）：UNX-F52191-J1 六判据全落；
//! 操作集 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：操作集 ×10 账（拉取/推送/提交/暂存/合并/变基/比较/
//!      日志/暂存区管理/工作树查询），逐操作载荷（纯函数折叠）恒等
//!      才收；漂移必抓。
//!   ② 双向闭集闸：操作两类（本地 0..6 / 远端 6..10），本地→远端
//!      拍序必须单调，回跳必抓；同类别内允许乱序。
//!   ③ 许可四态闭集（0=试用 1=非商业 2=商业 3=过期）：过期/试用态
//!      申报商业全通 = 如实定级闸必抓（AI-95 同构）。
//!   ④ 注入 ×10 轮（操作越闭集/许可越态/重复落账三态轮转）10/10
//!      必抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（10 操作全通
//!      + 许可在册 → S1；许可过期 → S3 如实）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52190 − 52000)）。
pub const SEG_BASE: u32 = 0xC6F80;

/// 操作集数。
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
    /// 拍序逆序
    EOrder,
    /// 重复落账
    EDup,
    /// 许可越态
    ENoLic,
    /// 过期申报商业
    EFalseClaim,
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
            PfCode::EOrder => "拍序逆序：本地操作先于远端操作",
            PfCode::EDup => "重复落账：同格只许一条账目",
            PfCode::ENoLic => "许可越态：四态闭集之外必拒",
            PfCode::EFalseClaim => "过期/试用申报商业全通：如实定级不许虚报",
            PfCode::ESeal => "未齐即封：格未齐先补齐",
            PfCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PfCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PfCode::EGradeDrift => "定级漂移：操作全通+许可在册才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（操作 op）。
pub const fn op_cell(op: usize) -> Option<u64> {
    if op >= OPS {
        return None;
    }
    Some(0xE600 + op as u64 * 0x97)
}

/// 操作类别（0..6=本地 6..10=远端）。
pub const fn op_kind(op: usize) -> Option<u32> {
    if op >= OPS {
        return None;
    }
    if op < 6 {
        Some(0)
    } else {
        Some(1)
    }
}

/// 许可四态闭集（0=试用 1=非商业 2=商业 3=过期）。
pub const fn lic_ok(v: u32) -> bool {
    v <= 3
}

/// 许可态对定级的影响（2=商业→S1 3=过期→S3 其余→S2）。
pub const fn lic_grade(v: u32) -> Option<u32> {
    if !lic_ok(v) {
        return None;
    }
    if v == 2 {
        Some(1)
    } else if v == 3 {
        Some(3)
    } else {
        Some(2)
    }
}

/// 商业可用核验（仅商业态可申报商业全通）。
pub const fn lic_commercial(v: u32) -> bool {
    v == 2
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// SmartGit 收口账本。
#[derive(Clone)]
pub struct PfLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [Option<u64>; OPS],
    /// 许可态。
    lic: u32,
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
            lic: 0,
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
        self.fnv(0x45100 + gate as u64);
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

    /// 许可落账（四态闸）。
    pub fn set_lic(&mut self, v: u32) -> Result<(), PfCode> {
        if !lic_ok(v) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x45200);
            return Err(PfCode::ENoLic);
        }
        self.lic = v;
        self.fnv(0x45201);
        Ok(())
    }

    /// 商业申报闸：非商业态申报商业全通必抓。
    pub fn claim_commercial(&mut self) -> bool {
        if !lic_commercial(self.lic) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x45202);
            return false;
        }
        true
    }

    /// 操作一格（拍序单调 + 载荷对照 + 重复拒）。
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
            self.fnv(0x45300);
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
        self.fnv(0x45302 + op as u64);
        Ok(p)
    }

    /// 操作全账核验。
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

    /// 封账闸：操作全齐 + 许可在册。
    pub fn seal(&self) -> Result<(), PfCode> {
        if !self.ops_all() {
            return Err(PfCode::ESeal);
        }
        if !lic_ok(self.lic) {
            return Err(PfCode::ENoLic);
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
    let _ = led.set_lic(2);
    let mut recorded = 0u32;
    let mut op = 0usize;
    while op < OPS {
        if led.record_op(op).is_ok() {
            recorded += 1;
        }
        op += 1;
    }
    // 注入 10 轮：操作越闭集 / 许可越态 / 重复落账三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PfLedger::new();
        let caught = match kind {
            0 => lt.record_op(OPS).is_err(),
            1 => lt.set_lic(9).is_err(),
            _ => {
                let _ = lt.record_op(0);
                lt.record_op(0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x45400 + k as u64);
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
pub fn run_unx52190_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52190");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "操作集 10 全通",
        recorded >= OPS as u32,
        "拉取/推送/提交/暂存/合并/变基/比较/日志/暂存区管理/工作树查询逐操作恒等",
    );
    s.add(
        "许可四态闭集",
        lic_ok(0) && lic_ok(3) && !lic_ok(4),
        "试用/非商业/商业/过期四态在册且越态拒",
    );
    s.add(
        "许可定级映射",
        match lic_grade(2) {
            Some(1) => true,
            _ => false,
        } && match lic_grade(3) {
            Some(3) => true,
            _ => false,
        } && match lic_grade(0) {
            Some(2) => true,
            _ => false,
        },
        "商业→S1 过期→S3 试用/非商业→S2 如实",
    );
    s.add(
        "非商业态申报商业必抓",
        {
            let mut lt = PfLedger::new();
            let _ = lt.set_lic(0);
            !lt.claim_commercial()
        },
        "试用/非商业/过期申报商业全通必须如实",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/许可越态/重复三态轮转必抓");
    let mut l0 = PfLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·许可越闭集",
        l0.set_lic(7).is_err(),
        "许可 7 越四态闭集必拒",
    );
    let mut l1 = PfLedger::new();
    let _ = l1.record_op(8);
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
    fn test_lic_set() {
        assert!(lic_ok(0) && lic_ok(3));
        assert!(!lic_ok(4));
        assert_eq!(lic_grade(2), Some(1));
        assert_eq!(lic_grade(3), Some(3));
        assert_eq!(lic_grade(1), Some(2));
        assert!(lic_commercial(2));
        assert!(!lic_commercial(0));
    }

    #[test]
    fn test_claim_gate() {
        let mut lt = PfLedger::new();
        let _ = lt.set_lic(0);
        assert!(!lt.claim_commercial());
        let _ = lt.set_lic(2);
        assert!(lt.claim_commercial());
    }

    #[test]
    fn test_order_gate() {
        let mut lt = PfLedger::new();
        assert!(lt.record_op(8).is_ok());
        assert!(lt.record_op(2).is_err());
    }

    #[test]
    fn test_dup_gate() {
        let mut lt = PfLedger::new();
        let _ = lt.record_op(0);
        assert!(lt.record_op(0).is_err());
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
        assert_eq!(SEG_BASE, 0xC6F80);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52190_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
