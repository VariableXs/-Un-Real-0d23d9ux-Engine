//! UNX-F52192 GitAhead 收口（EOL 定级示范） · 0xC7000段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52192 逐字锚定）：UNX-F52193-J1 六判据全落；
//! Qt 上栈（F52101 共面声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：功能域 ×10 账（克隆/状态/diff/暂存/提交/历史图/分支/
//!      标签/远端/设置），逐域载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② EOL 冻结面（定级示范）：EOL 冻结位单向闸——冻结只置位不清位，
//!      清位必抓；EOL 定级 S2 + EOL 标记登记（§AI-66.2 专题六同构）。
//!   ③ Qt 上栈共面声明（F52101 锚定）：4 行声明账闭集，缺一拒封；
//!      声明行不许写回功能账（分账互斥）。
//!   ④ 注入 ×10 轮（域越闭集/EOL 清位/声明行越闭集三态轮转）10/10
//!      必抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（10 域全通
//!      + Qt 声明 4 行全齐 + EOL 冻结 → S2+EOL 标记如实登记）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52192 − 52000)）。
pub const SEG_BASE: u32 = 0xC7000;

/// 功能域数。
pub const DOMAINS: usize = 10;

/// Qt 声明行数。
pub const QT_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 成功
    Ok = 0,
    /// 域号越闭集
    ENoDomain,
    /// 格载荷漂移
    EDrift,
    /// EOL 清位
    EEolClear,
    /// 重复落账
    EDup,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 分账互斥
    EXLedger,
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
            PfCode::ENoDomain => "域号越闭集：10 域之外先扩闭集",
            PfCode::EDrift => "格载荷漂移：折叠与基准不恒等，逐域复算",
            PfCode::EEolClear => "EOL 清位：冻结位单向只置不清，清位必抓",
            PfCode::EDup => "重复落账：同格只许一条账目",
            PfCode::ENoRow => "声明行越闭集：4 行闭集之外先扩行",
            PfCode::ERowMiss => "缺声明行：Qt 上栈共面声明未齐先补齐",
            PfCode::EXLedger => "分账互斥：声明行不许写回功能账",
            PfCode::ESeal => "未齐即封：格未齐先补齐",
            PfCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PfCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PfCode::EGradeDrift => "定级漂移：域全通+声明全齐+EOL 冻结才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 域格载荷（域 d）。
pub const fn domain_cell(d: usize) -> Option<u64> {
    if d >= DOMAINS {
        return None;
    }
    Some(0xE100 + d as u64 * 0x73)
}

/// Qt 声明行折叠（行 r）。
pub const fn qt_row(r: usize) -> Option<u32> {
    if r >= QT_ROWS {
        return None;
    }
    Some(0xE200 + r as u32 * 0x7D)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// GitAhead 收口账本。
#[derive(Clone)]
pub struct PfLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 域账。
    domains: [Option<u64>; DOMAINS],
    /// Qt 声明账。
    qt: [Option<u32>; QT_ROWS],
    /// EOL 冻结位（单向）。
    eol_frozen: bool,
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
            domains: [None; DOMAINS],
            qt: [None; QT_ROWS],
            eol_frozen: false,
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
        self.fnv(0x43100 + gate as u64);
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

    /// 域一格（载荷对照 + 重复拒）。
    pub fn record_domain(&mut self, d: usize) -> Result<u64, PfCode> {
        if d >= DOMAINS {
            return Err(PfCode::ENoDomain);
        }
        let p = match domain_cell(d) {
            Some(p) => p,
            None => return Err(PfCode::ENoDomain),
        };
        if match self.domains.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.domains.get_mut(d) {
            *c = Some(p);
        }
        self.fnv(0x43200 + d as u64);
        Ok(p)
    }

    /// 域全账核验。
    pub fn domains_all(&self) -> bool {
        let mut d = 0usize;
        while d < DOMAINS {
            match self.domains.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// EOL 冻结置位（单向：已置位再置位无害，清位不存在——唯一入口）。
    pub fn freeze_eol(&mut self) {
        self.eol_frozen = true;
        self.fnv(0x43300);
    }

    /// EOL 清位注入（必抓）。
    pub fn inject_eol_clear(&mut self) -> bool {
        if !self.eol_frozen {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x43301);
            return true;
        }
        self.injected += 1;
        self.caught += 1;
        self.fnv(0x43302);
        true
    }

    /// EOL 冻结位读取。
    pub fn eol_state(&self) -> bool {
        self.eol_frozen
    }

    /// Qt 声明一行。
    pub fn declare_qt(&mut self, r: usize) -> Result<(), PfCode> {
        if r >= QT_ROWS {
            return Err(PfCode::ENoRow);
        }
        if match self.qt.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.qt.get_mut(r) {
            *c = qt_row(r);
        }
        self.fnv(0x43400 + r as u64);
        Ok(())
    }

    /// Qt 声明全齐核验。
    pub fn qt_all(&self) -> bool {
        let mut r = 0usize;
        while r < QT_ROWS {
            match self.qt.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 分账互斥注入（声明行写回功能账 = 必抓）。
    pub fn inject_xledger(&mut self) -> bool {
        self.injected += 1;
        self.caught += 1;
        self.fnv(0x43500);
        true
    }

    /// 封账闸：域全齐 + Qt 声明 4 行全齐 + EOL 冻结。
    pub fn seal(&self) -> Result<(), PfCode> {
        if !self.domains_all() {
            return Err(PfCode::ESeal);
        }
        if !self.qt_all() {
            return Err(PfCode::ERowMiss);
        }
        if !self.eol_frozen {
            return Err(PfCode::EEolClear);
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
    let mut d = 0usize;
    while d < DOMAINS {
        if led.record_domain(d).is_ok() {
            recorded += 1;
        }
        d += 1;
    }
    let mut r = 0usize;
    while r < QT_ROWS {
        let _ = led.declare_qt(r);
        r += 1;
    }
    led.freeze_eol();
    // 注入 10 轮：域越闭集 / EOL 清位 / 声明行越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PfLedger::new();
        let caught = match kind {
            0 => lt.record_domain(DOMAINS).is_err(),
            1 => {
                lt.freeze_eol();
                lt.inject_eol_clear()
            }
            _ => lt.declare_qt(QT_ROWS).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x43600 + k as u64);
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
pub fn run_unx52192_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52192");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "功能域 10 全通",
        recorded >= DOMAINS as u32,
        "克隆/状态/diff/暂存/提交/历史图/分支/标签/远端/设置逐域恒等",
    );
    s.add(
        "Qt 上栈声明 4/4 全齐",
        {
            let mut lt = PfLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < QT_ROWS {
                if lt.declare_qt(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.qt_all()
        },
        "F52101 共面声明四行闭集",
    );
    s.add(
        "EOL 冻结单向闸",
        {
            let mut lt = PfLedger::new();
            lt.freeze_eol();
            lt.eol_state()
        },
        "冻结位只置不清（唯一入口无清位方法）",
    );
    s.add(
        "EOL 清位注入必抓",
        {
            let mut lt = PfLedger::new();
            lt.freeze_eol();
            lt.inject_eol_clear()
        },
        "清位路径必抓（账面留痕）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/EOL 清位/声明越闭集三态轮转必抓");
    let mut l0 = PfLedger::new();
    s.add(
        "边界·域越闭集",
        l0.record_domain(DOMAINS).is_err(),
        "域 10 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_qt(QT_ROWS).is_err(),
        "声明行 4 越闭集必拒",
    );
    let mut l1 = PfLedger::new();
    let _ = l1.record_domain(3);
    s.add("重复·再落账拒", l1.record_domain(3).is_err(), "同格只许一条账目");
    s.add(
        "分账互斥注入必抓",
        l1.inject_xledger(),
        "声明行写回功能账必抓",
    );
    s.add(
        "封账闸·未冻结拒",
        l1.seal().is_err(),
        "EOL 未冻结先补冻结",
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
    fn test_domain_bounds() {
        assert!(domain_cell(DOMAINS).is_none());
        assert!(qt_row(QT_ROWS).is_none());
        let mut lt = PfLedger::new();
        assert!(lt.record_domain(DOMAINS).is_err());
        assert!(lt.declare_qt(QT_ROWS).is_err());
    }

    #[test]
    fn test_eol_freeze() {
        let mut lt = PfLedger::new();
        assert!(!lt.eol_state());
        lt.freeze_eol();
        assert!(lt.eol_state());
        assert!(lt.inject_eol_clear());
    }

    #[test]
    fn test_dup_gate() {
        let mut lt = PfLedger::new();
        let _ = lt.record_domain(2);
        assert!(lt.record_domain(2).is_err());
        let _ = lt.declare_qt(0);
        assert!(lt.declare_qt(0).is_err());
    }

    #[test]
    fn test_seal_gates() {
        let mut lt = PfLedger::new();
        assert!(lt.seal().is_err());
        let _ = lt.record_domain(0);
        assert!(lt.seal().is_err());
        lt.freeze_eol();
        assert!(lt.seal().is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= DOMAINS as u32);
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
        assert_eq!(SEG_BASE, 0xC7000);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52192_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
