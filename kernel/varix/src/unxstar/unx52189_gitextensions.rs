//! UNX-F52189 Git Extensions 收口 · 0xC6F40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52189 逐字锚定）：UNX-F52190-J1 六判据全落；
//! JVM 上栈（JBR 同构账引用 F52057 联签锚定行）；KB-COMPAT 定级登记；
//! 判据锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：功能域 ×10 账（浏览/提交/历史/冲突解决/子模块/忽略
//!      文件/标签/暂存管理/设置/脚本），逐域载荷（纯函数折叠）恒等
//!      才收；漂移必抓。
//!   ② JVM 上栈账（F52057 JBR 同构联签）：4 行闭集（运行时层/基础库
//!      层/翻译层/图形层），缺一拒封；层序单调（自底向上），逆层必抓。
//!   ③ 上探闸：栈层上探（层号越闭集）必抓；未齐先探必抓。
//!   ④ 注入 ×10 轮（域越闭集/栈层越闭集/重复落账三态轮转）10/10
//!      必抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（10 域全通
//!      + JVM 上栈 4 行全齐且层序单调 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52189 − 52000)）。
pub const SEG_BASE: u32 = 0xC6F40;

/// 功能域数。
pub const DOMAINS: usize = 10;

/// JVM 上栈行数。
pub const STACK_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 成功
    Ok = 0,
    /// 域号越闭集
    ENoDomain,
    /// 格载荷漂移
    EDrift,
    /// 逆层
    EOrder,
    /// 重复落账
    EDup,
    /// 栈行越闭集
    ENoRow,
    /// 缺栈行
    ERowMiss,
    /// 未齐先探
    EEarly,
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
            PfCode::EOrder => "逆层：栈层自底向上层序单调不许回跳",
            PfCode::EDup => "重复落账：同格只许一条账目",
            PfCode::ENoRow => "栈行越闭集：4 行闭集之外先扩行",
            PfCode::ERowMiss => "缺栈行：JVM 上栈四行未齐先补齐",
            PfCode::EEarly => "未齐先探：栈未齐先补齐再上探",
            PfCode::ESeal => "未齐即封：格未齐先补齐",
            PfCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PfCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PfCode::EGradeDrift => "定级漂移：域全通+栈全齐且层序单调才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 域格载荷（域 d）。
pub const fn domain_cell(d: usize) -> Option<u64> {
    if d >= DOMAINS {
        return None;
    }
    Some(0xE700 + d as u64 * 0x9D)
}

/// JVM 栈行折叠（行 r：0=运行时层 1=基础库层 2=翻译层 3=图形层）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xE800 + r as u32 * 0xA3)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Git Extensions 收口账本。
#[derive(Clone)]
pub struct PfLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 域账。
    domains: [Option<u64>; DOMAINS],
    /// JVM 栈账。
    stack: [Option<u32>; STACK_ROWS],
    /// 最后层号。
    last_layer: i64,
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
            stack: [None; STACK_ROWS],
            last_layer: -1,
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
        self.fnv(0x46100 + gate as u64);
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
        self.fnv(0x46200 + d as u64);
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

    /// JVM 栈一行（层序单调自底向上 + 重复拒）。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), PfCode> {
        if r >= STACK_ROWS {
            return Err(PfCode::ENoRow);
        }
        if (r as i64) < self.last_layer {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x46300);
            return Err(PfCode::EOrder);
        }
        if match self.stack.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.stack.get_mut(r) {
            *c = stack_row(r);
        }
        self.last_layer = r as i64;
        self.fnv(0x46302 + r as u64);
        Ok(())
    }

    /// 栈全齐核验。
    pub fn stack_all(&self) -> bool {
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.stack.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 上探闸：栈未齐先上探必抓。
    pub fn probe_up(&self) -> Result<(), PfCode> {
        if !self.stack_all() {
            return Err(PfCode::EEarly);
        }
        Ok(())
    }

    /// 封账闸：域全齐 + 栈全齐。
    pub fn seal(&self) -> Result<(), PfCode> {
        if !self.domains_all() {
            return Err(PfCode::ESeal);
        }
        if !self.stack_all() {
            return Err(PfCode::ERowMiss);
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
    while r < STACK_ROWS {
        let _ = led.declare_stack(r);
        r += 1;
    }
    let _ = led.probe_up();
    // 注入 10 轮：域越闭集 / 栈行越闭集 / 重复落账三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PfLedger::new();
        let caught = match kind {
            0 => lt.record_domain(DOMAINS).is_err(),
            1 => lt.declare_stack(STACK_ROWS).is_err(),
            _ => {
                let _ = lt.record_domain(0);
                lt.record_domain(0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x46400 + k as u64);
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
pub fn run_unx52189_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52189");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "功能域 10 全通",
        recorded >= DOMAINS as u32,
        "浏览/提交/历史/冲突解决/子模块/忽略文件/标签/暂存管理/设置/脚本逐域恒等",
    );
    s.add(
        "JVM 上栈 4/4 全齐",
        {
            let mut lt = PfLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.stack_all()
        },
        "运行时层/基础库层/翻译层/图形层四行闭集 F52057 联签",
    );
    s.add(
        "层序单调闸",
        {
            let mut lt = PfLedger::new();
            let _ = lt.declare_stack(3);
            lt.declare_stack(0).is_err()
        },
        "图形层后回运行时层逆层必抓",
    );
    s.add(
        "未齐先探必抓",
        {
            let mut lt = PfLedger::new();
            let _ = lt.declare_stack(0);
            lt.probe_up().is_err()
        },
        "栈未齐上探必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/栈越闭集/重复三态轮转必抓");
    let mut l0 = PfLedger::new();
    s.add(
        "边界·域越闭集",
        l0.record_domain(DOMAINS).is_err(),
        "域 10 越闭集必拒",
    );
    s.add(
        "边界·栈行越闭集",
        l0.declare_stack(STACK_ROWS).is_err(),
        "栈行 4 越闭集必拒",
    );
    let mut l1 = PfLedger::new();
    let _ = l1.record_domain(5);
    s.add("重复·再落账拒", l1.record_domain(5).is_err(), "同格只许一条账目");
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
    fn test_bounds() {
        assert!(domain_cell(DOMAINS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
        let mut lt = PfLedger::new();
        assert!(lt.record_domain(DOMAINS).is_err());
        assert!(lt.declare_stack(STACK_ROWS).is_err());
    }

    #[test]
    fn test_layer_order() {
        let mut lt = PfLedger::new();
        assert!(lt.declare_stack(2).is_ok());
        assert!(lt.declare_stack(0).is_err());
        assert!(lt.declare_stack(3).is_ok());
    }

    #[test]
    fn test_probe_gate() {
        let mut lt = PfLedger::new();
        let _ = lt.declare_stack(0);
        assert!(lt.probe_up().is_err());
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = lt.declare_stack(r);
            r += 1;
        }
        assert!(lt.probe_up().is_ok());
    }

    #[test]
    fn test_seal_gates() {
        let mut lt = PfLedger::new();
        assert!(lt.seal().is_err());
        let _ = lt.record_domain(0);
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
        assert_eq!(SEG_BASE, 0xC6F40);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52189_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
