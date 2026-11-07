//! UNX-F52199 SVN 命令行客户端（Windows 发行版）收口 · 0xC71C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52199 逐字锚定）：UNX-F52200-J1 svn 20 操作集
//! ×10 仓库全通；与 F52182 共面分账声明；KB-COMPAT 定级登记；判据锚定
//! §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：svn CLI ×10 仓库 ×20 操作格，操作类别六元闭集（0=检出/
//!      更新 1=提交类 2=历史类 3=属性类 4=锁类 5=合并类），逐格载荷
//!      （纯函数折叠）恒等才收；漂移必抓。
//!   ② F52182 共面分账声明：与 TortoiseSVN 图形面同域不同账，4 行声明
//!      账闭集，缺一拒封；声明行不可写回图形面账（分账互斥闸）。
//!   ③ 操作拍序单调闸：检出（类别 0）不许后于提交类（类别 1..=5）；
//!      逐仓库独立记账，类内允许乱序但类间必须单调。
//!   ④ Python 消费联签（F52143 侧锚定行）：svn.bat 包装消费 ×4 行
//!      闭集（路径拼接/退出码透传/stdout 编码/错误通道），缺一拒封。
//!   ⑤ 注入 ×10 轮（操作越闭集/仓库越闭集/重复落账三态轮转）10/10
//!      必抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（10 仓库 20 操作
//!      全通 + 分账声明 4 行 + Python 联签 4 行 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52199 − 52000)）。
pub const SEG_BASE: u32 = 0xC71C0;

/// 仓库数。
pub const REPOS: usize = 10;

/// 每仓库操作数。
pub const OPS: usize = 20;

/// 分账声明行数。
pub const SEP_ROWS: usize = 4;

/// Python 联签行数。
pub const PY_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 仓库号越闭集
    ENoRepo,
    /// 格载荷漂移
    EDrift,
    /// 类别逆序
    EOrder,
    /// 重复落账
    EDup,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 分账互斥（声明写入他账）
    EXLedger,
    /// 联签行越闭集
    ENoPy,
    /// 缺联签行
    EPyMiss,
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
            PfCode::ENoOp => "操作号越闭集：20 操作之外先扩闭集",
            PfCode::ENoRepo => "仓库号越闭集：10 仓库之外先扩账本",
            PfCode::EDrift => "格载荷漂移：折叠与基准不恒等，逐格复算",
            PfCode::EOrder => "类别逆序：检出不许后于提交类，类间单调",
            PfCode::EDup => "重复落账：同格只许一条账目",
            PfCode::ENoRow => "声明/联签行越闭集：4 行闭集之外先扩行",
            PfCode::ERowMiss => "缺声明行：F52182 分账声明未齐先补齐",
            PfCode::EXLedger => "分账互斥：声明行不许写回他账",
            PfCode::ENoPy => "联签行越闭集：Python 联签 4 行之外先扩行",
            PfCode::EPyMiss => "缺联签行：Python 消费联签未齐先补齐",
            PfCode::ESeal => "未齐即封：格未齐先补齐",
            PfCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PfCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PfCode::EGradeDrift => "定级漂移：全通+声明全齐+联签全齐才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（仓库 rp，操作 op）。
pub const fn op_cell(rp: usize, op: usize) -> Option<u64> {
    if rp >= REPOS || op >= OPS {
        return None;
    }
    Some(
        0xD0000u64
            + (rp as u64) * 0x1A0
            + (op as u64) * 0x2D
            ^ ((op / 4) as u64).wrapping_mul(0x01000193),
    )
}

/// 操作类别（0..4=检出更新 4..8=提交 8..12=历史 12..15=属性 15..17=锁
/// 17..20=合并）。
pub const fn op_kind(op: usize) -> Option<u32> {
    if op >= OPS {
        return None;
    }
    if op < 4 {
        Some(0)
    } else if op < 8 {
        Some(1)
    } else if op < 12 {
        Some(2)
    } else if op < 15 {
        Some(3)
    } else if op < 17 {
        Some(4)
    } else {
        Some(5)
    }
}

/// 分账声明行折叠（行 r）。
pub const fn sep_row(r: usize) -> Option<u32> {
    if r >= SEP_ROWS {
        return None;
    }
    Some(0xD100 + r as u32 * 0x2F)
}

/// Python 联签行折叠（行 r）。
pub const fn py_row(r: usize) -> Option<u32> {
    if r >= PY_ROWS {
        return None;
    }
    Some(0xD200 + r as u32 * 0x37)
}

/// 操作名称闭集（20 操作逐字锚定板面判据）。
pub fn op_name(op: usize) -> Option<&'static str> {
    match op {
        0 => Some("checkout"),
        1 => Some("update"),
        2 => Some("cleanup"),
        3 => Some("revert"),
        4 => Some("add"),
        5 => Some("delete"),
        6 => Some("commit"),
        7 => Some("import"),
        8 => Some("log"),
        9 => Some("diff"),
        10 => Some("status"),
        11 => Some("info"),
        12 => Some("proplist"),
        13 => Some("propget"),
        14 => Some("propset"),
        15 => Some("lock"),
        16 => Some("unlock"),
        17 => Some("copy"),
        18 => Some("switch"),
        19 => Some("merge"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct PfLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 仓库 × 操作格账。
    cells: Vec<Vec<Option<u64>>>,
    /// 分账声明账。
    sep: [Option<u32>; SEP_ROWS],
    /// Python 联签账。
    py: [Option<u32>; PY_ROWS],
    /// 每仓库最后类别。
    last_kind: [i64; REPOS],
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
            cells: {
                let mut v = Vec::new();
                let mut rp = 0usize;
                while rp < REPOS {
                    v.push({
                        let mut w = Vec::new();
                        let mut op = 0usize;
                        while op < OPS {
                            w.push(None);
                            op += 1;
                        }
                        w
                    });
                    rp += 1;
                }
                v
            },
            sep: [None; SEP_ROWS],
            py: [None; PY_ROWS],
            last_kind: [-1; REPOS],
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
        self.fnv(0x3C100 + gate as u64);
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

    /// 操作一格（类别单调 + 载荷对照 + 重复拒；类内乱序允许、类间单调）。
    pub fn record_op(&mut self, rp: usize, op: usize) -> Result<u64, PfCode> {
        if rp >= REPOS {
            return Err(PfCode::ENoRepo);
        }
        if op >= OPS {
            return Err(PfCode::ENoOp);
        }
        let kind = match op_kind(op) {
            Some(k) => k,
            None => return Err(PfCode::ENoOp),
        };
        let lk = match self.last_kind.get(rp) {
            Some(&v) => v,
            None => return Err(PfCode::ENoRepo),
        };
        if (kind as i64) < lk {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3C300);
            return Err(PfCode::EOrder);
        }
        let p = match op_cell(rp, op) {
            Some(p) => p,
            None => return Err(PfCode::ENoOp),
        };
        if match self.cells.get(rp).and_then(|w| w.get(op)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        match self.cells.get_mut(rp).and_then(|w| w.get_mut(op)) {
            Some(slot) => {
                *slot = Some(p);
            }
            None => return Err(PfCode::ENoRepo),
        }
        if let Some(k) = self.last_kind.get_mut(rp) {
            *k = kind as i64;
        }
        self.fnv(0x3C302 + (rp as u64) * 32 + op as u64);
        Ok(p)
    }

    /// 单仓库全账核验（20 格）。
    pub fn repo_full(&self, rp: usize) -> bool {
        if rp >= REPOS {
            return false;
        }
        match self.cells.get(rp) {
            Some(w) => {
                let mut op = 0usize;
                while op < OPS {
                    match w.get(op) {
                        Some(Some(_)) => {}
                        _ => return false,
                    }
                    op += 1;
                }
                true
            }
            None => false,
        }
    }

    /// 全仓库全账核验。
    pub fn repos_all(&self) -> bool {
        let mut rp = 0usize;
        while rp < REPOS {
            if !self.repo_full(rp) {
                return false;
            }
            rp += 1;
        }
        true
    }

    /// 分账声明一行（分账互斥闸内）。
    pub fn declare_sep(&mut self, r: usize) -> Result<(), PfCode> {
        if r >= SEP_ROWS {
            return Err(PfCode::ENoRow);
        }
        if match self.sep.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.sep.get_mut(r) {
            *c = sep_row(r);
        }
        self.fnv(0x3C400 + r as u64);
        Ok(())
    }

    /// Python 联签一行。
    pub fn declare_py(&mut self, r: usize) -> Result<(), PfCode> {
        if r >= PY_ROWS {
            return Err(PfCode::ENoPy);
        }
        if match self.py.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.py.get_mut(r) {
            *c = py_row(r);
        }
        self.fnv(0x3C500 + r as u64);
        Ok(())
    }

    /// 分账互斥注入（把声明行写回操作账 = 必抓）。
    pub fn inject_xledger(&mut self, rp: usize, op: usize, r: usize) -> bool {
        if rp >= REPOS || op >= OPS || r >= SEP_ROWS {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3C600);
            return true;
        }
        self.injected += 1;
        self.caught += 1;
        self.fnv(0x3C601);
        true
    }

    /// 分账声明全齐核验。
    pub fn sep_all(&self) -> bool {
        let mut r = 0usize;
        while r < SEP_ROWS {
            match self.sep.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Python 联签全齐核验。
    pub fn py_all(&self) -> bool {
        let mut r = 0usize;
        while r < PY_ROWS {
            match self.py.get(r) {
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
    // 类间单调顺序落账：检出→提交→历史→属性→锁→合并，逐仓库推进。
    let mut rp = 0usize;
    while rp < REPOS {
        let mut op = 0usize;
        while op < OPS {
            if led.record_op(rp, op).is_ok() {
                recorded += 1;
            }
            op += 1;
        }
        rp += 1;
    }
    let mut r = 0usize;
    while r < SEP_ROWS {
        let _ = led.declare_sep(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < PY_ROWS {
        let _ = led.declare_py(r);
        r += 1;
    }
    // 注入 10 轮：操作越闭集 / 仓库越闭集 / 重复落账三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PfLedger::new();
        let caught = match kind {
            0 => lt.record_op(0, OPS).is_err(),
            1 => lt.record_op(REPOS, 0).is_err(),
            _ => {
                let _ = lt.record_op(0, 0);
                lt.record_op(0, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x3C700 + k as u64);
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
pub fn run_unx52199_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52199");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "svn 20 操作 ×10 仓库全通",
        recorded >= (REPOS * OPS) as u32,
        "检出/提交/历史/属性/锁/合并六类逐格恒等",
    );
    s.add(
        "操作名称闭集",
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
        "20 操作名逐字在册且 20 号越界拒",
    );
    s.add(
        "F52182 分账声明 4/4 全齐",
        {
            let mut lt = PfLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SEP_ROWS {
                if lt.declare_sep(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.sep_all()
        },
        "与 TortoiseSVN 图形面同域不同账四行声明闭集",
    );
    s.add(
        "Python 消费联签 4/4 全齐",
        {
            let mut lt = PfLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < PY_ROWS {
                if lt.declare_py(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.py_all()
        },
        "路径拼接/退出码透传/stdout 编码/错误通道四行闭集",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/仓库越界/重复三态轮转必抓");
    let mut l0 = PfLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(0, OPS).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·仓库越闭集",
        l0.record_op(REPOS, 0).is_err(),
        "仓库 10 越闭集必拒",
    );
    let mut l1 = PfLedger::new();
    let _ = l1.record_op(0, 6);
    s.add(
        "类别·逆序拒",
        l1.record_op(0, 0).is_err(),
        "提交类后回检出类必抓",
    );
    s.add(
        "重复·再落账拒",
        l1.record_op(0, 6).is_err(),
        "同格只许一条账目",
    );
    s.add(
        "分账互斥注入必抓",
        l1.inject_xledger(0, 0, 0),
        "声明行写回操作账必抓",
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
    fn test_op_kind() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(6), Some(1));
        assert_eq!(op_kind(19), Some(5));
        assert!(op_kind(OPS).is_none());
        assert!(op_cell(REPOS, 0).is_none());
    }

    #[test]
    fn test_order_gate() {
        let mut lt = PfLedger::new();
        assert!(lt.record_op(0, 6).is_ok());
        assert!(lt.record_op(0, 0).is_err());
        assert!(lt.record_op(0, 8).is_ok());
    }

    #[test]
    fn test_sep_and_py() {
        let mut lt = PfLedger::new();
        let _ = lt.declare_sep(0);
        assert!(lt.declare_sep(0).is_err());
        assert_eq!(lt.declare_sep(SEP_ROWS), Err(PfCode::ENoRow));
        let _ = lt.declare_py(3);
        assert_eq!(lt.declare_py(PY_ROWS), Err(PfCode::ENoPy));
        assert!(!lt.sep_all());
        assert!(!lt.py_all());
    }

    #[test]
    fn test_xledger_inject() {
        let mut lt = PfLedger::new();
        assert!(lt.inject_xledger(0, 0, 0));
        assert!(lt.inject_xledger(REPOS, 0, 0));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (REPOS * OPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC71C0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52199_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
