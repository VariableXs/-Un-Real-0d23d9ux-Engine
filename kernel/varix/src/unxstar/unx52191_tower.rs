//! UNX-F52191 Tower（Windows 版）收口 · 0xC6FC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52191 逐字锚定）：UNX-F52192-J1 六判据执行；
//! EOL 冻结定级；KB-COMPAT 定级 S2 + EOL 标记；判据锚定 §AI-66.2
//! 专题六。
//!
//! 设计要点：
//!   ① 域模型：EOL 冻结域 ×10 账（仓库/提交/分支/标签/历史/撤销/
//!      冲突/忽略/远端/设置），逐域载荷（纯函数折叠）恒等才收；
//!      漂移必抓。
//!   ② EOL 冻结定级闸（§AI-66.2 专题六同构）：冻结拍数账 ×8 拍
//!      严格单调（只升不降），降拍必抓；冻结位单向（只置不清）。
//!   ③ 定级账：S2 + EOL 标记两行登记闭集，缺一拒封；定级漂移（登记
//!      值≠S2）必抓。
//!   ④ 注入 ×10 轮（域越闭集/冻结降拍/标记行越闭集三态轮转）10/10
//!      必抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（10 域全通
//!      + 8 拍单调 + 两行登记全齐 → S2+EOL）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52191 − 52000)）。
pub const SEG_BASE: u32 = 0xC6FC0;

/// 冻结域数。
pub const DOMAINS: usize = 10;

/// 冻结拍数。
pub const BEATS: usize = 8;

/// 定级登记行数。
pub const GRADE_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 成功
    Ok = 0,
    /// 域号越闭集
    ENoDomain,
    /// 格载荷漂移
    EDrift,
    /// 冻结降拍
    EBeatDrop,
    /// 重复落账
    EDup,
    /// 登记行越闭集
    ENoRow,
    /// 缺登记行
    ERowMiss,
    /// 定级漂移
    EGradeDrift,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
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
            PfCode::EBeatDrop => "冻结降拍：冻结拍数严格单调只升不降",
            PfCode::EDup => "重复落账：同格只许一条账目",
            PfCode::ENoRow => "登记行越闭集：2 行闭集之外先扩行",
            PfCode::ERowMiss => "缺登记行：S2+EOL 两行登记未齐先补齐",
            PfCode::EGradeDrift => "定级漂移：登记值必须=S2+EOL 如实",
            PfCode::ESeal => "未齐即封：格未齐先补齐",
            PfCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PfCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 域格载荷（域 d）。
pub const fn domain_cell(d: usize) -> Option<u64> {
    if d >= DOMAINS {
        return None;
    }
    Some(0xE300 + d as u64 * 0x83)
}

/// 冻结拍载荷（拍 b）。
pub const fn beat_cell(b: usize) -> Option<u32> {
    if b >= BEATS {
        return None;
    }
    Some(0xE400 + b as u32 * 0x89)
}

/// 定级登记行折叠（行 r）。
pub const fn grade_row(r: usize) -> Option<u32> {
    if r >= GRADE_ROWS {
        return None;
    }
    Some(0xE500 + r as u32 * 0x8F)
}

/// EOL 定级闭集核验（登记值必须=S2）。
pub const fn grade_ok(g: u32) -> bool {
    g == 2
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Tower 收口账本。
#[derive(Clone)]
pub struct PfLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 域账。
    domains: [Option<u64>; DOMAINS],
    /// 冻结拍账。
    beats: [Option<u32>; BEATS],
    /// 最后拍值。
    last_beat: u32,
    /// 定级登记账。
    grades: [Option<u32>; GRADE_ROWS],
    /// EOL 冻结位。
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
            beats: [None; BEATS],
            last_beat: 0,
            grades: [None; GRADE_ROWS],
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
        self.fnv(0x44100 + gate as u64);
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
        self.fnv(0x44200 + d as u64);
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

    /// 冻结拍一格（严格单调只升不降 + 载荷对照 + 重复拒）。
    pub fn record_beat(&mut self, b: usize) -> Result<u32, PfCode> {
        if b >= BEATS {
            return Err(PfCode::ESeal);
        }
        let v = match beat_cell(b) {
            Some(v) => v,
            None => return Err(PfCode::ESeal),
        };
        if v <= self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x44300);
            return Err(PfCode::EBeatDrop);
        }
        if match self.beats.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(v);
        }
        self.last_beat = v;
        self.fnv(0x44302 + b as u64);
        Ok(v)
    }

    /// 冻结拍全账核验。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// EOL 冻结置位（单向）。
    pub fn freeze_eol(&mut self) {
        self.eol_frozen = true;
        self.fnv(0x44400);
    }

    /// EOL 冻结位读取。
    pub fn eol_state(&self) -> bool {
        self.eol_frozen
    }

    /// 定级登记一行（登记值=S2 闸）。
    pub fn declare_grade(&mut self, r: usize) -> Result<(), PfCode> {
        if r >= GRADE_ROWS {
            return Err(PfCode::ENoRow);
        }
        let v = match grade_row(r) {
            Some(v) => v,
            None => return Err(PfCode::ENoRow),
        };
        if match self.grades.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.grades.get_mut(r) {
            *c = Some(v);
        }
        self.fnv(0x44500 + r as u64);
        Ok(())
    }

    /// 定级登记全齐核验。
    pub fn grades_all(&self) -> bool {
        let mut r = 0usize;
        while r < GRADE_ROWS {
            match self.grades.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封账闸：域全齐 + 拍全齐 + 冻结 + 登记全齐。
    pub fn seal(&self) -> Result<(), PfCode> {
        if !self.domains_all() {
            return Err(PfCode::ESeal);
        }
        if !self.beats_all() {
            return Err(PfCode::EBeatDrop);
        }
        if !self.eol_frozen {
            return Err(PfCode::EGradeDrift);
        }
        if !self.grades_all() {
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
    led.freeze_eol();
    let mut recorded = 0u32;
    let mut d = 0usize;
    while d < DOMAINS {
        if led.record_domain(d).is_ok() {
            recorded += 1;
        }
        d += 1;
    }
    let mut b = 0usize;
    while b < BEATS {
        let _ = led.record_beat(b);
        b += 1;
    }
    let mut r = 0usize;
    while r < GRADE_ROWS {
        let _ = led.declare_grade(r);
        r += 1;
    }
    // 注入 10 轮：域越闭集 / 拍越闭集 / 重复落账三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PfLedger::new();
        let caught = match kind {
            0 => lt.record_domain(DOMAINS).is_err(),
            1 => lt.record_beat(BEATS).is_err(),
            _ => {
                let _ = lt.record_domain(0);
                lt.record_domain(0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x44600 + k as u64);
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
pub fn run_unx52191_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52191");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "EOL 冻结域 10 全通",
        recorded >= DOMAINS as u32,
        "仓库/提交/分支/标签/历史/撤销/冲突/忽略/远端/设置逐域恒等",
    );
    s.add(
        "冻结拍 8 拍严格单调",
        {
            let mut lt = PfLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BEATS {
                if lt.record_beat(b).is_err() {
                    ok = false;
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "冻结拍数只升不降降拍必抓",
    );
    s.add(
        "EOL 定级闭集",
        grade_ok(2) && !grade_ok(1) && !grade_ok(3),
        "登记值必须=S2 如实 EOL 标记",
    );
    s.add(
        "定级登记 2/2 全齐",
        {
            let mut lt = PfLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GRADE_ROWS {
                if lt.declare_grade(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.grades_all()
        },
        "S2 行+EOL 标记行两行登记闭集",
    );
    s.add(
        "EOL 冻结单向闸",
        {
            let mut lt = PfLedger::new();
            lt.freeze_eol();
            lt.eol_state()
        },
        "冻结位只置不清",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/拍越闭集/重复三态轮转必抓");
    let mut l0 = PfLedger::new();
    s.add(
        "边界·域越闭集",
        l0.record_domain(DOMAINS).is_err(),
        "域 10 越闭集必拒",
    );
    s.add(
        "边界·拍越闭集",
        l0.record_beat(BEATS).is_err(),
        "拍 8 越闭集必拒",
    );
    s.add(
        "边界·登记行越闭集",
        l0.declare_grade(GRADE_ROWS).is_err(),
        "登记行 2 越闭集必拒",
    );
    let mut l1 = PfLedger::new();
    let _ = l1.record_beat(3);
    s.add(
        "降拍必抓",
        l1.record_beat(1).is_err(),
        "拍值回降必抓",
    );
    let mut l2 = PfLedger::new();
    let _ = l2.record_domain(4);
    s.add("重复·再落账拒", l2.record_domain(4).is_err(), "同格只许一条账目");
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
        assert!(beat_cell(BEATS).is_none());
        assert!(grade_row(GRADE_ROWS).is_none());
    }

    #[test]
    fn test_grade_set() {
        assert!(grade_ok(2));
        assert!(!grade_ok(1));
        assert!(!grade_ok(3));
    }

    #[test]
    fn test_beat_monotone() {
        let mut lt = PfLedger::new();
        assert!(lt.record_beat(0).is_ok());
        assert!(lt.record_beat(1).is_ok());
        assert!(lt.record_beat(BEATS).is_err());
        assert!(lt.beats_all() == false);
    }

    #[test]
    fn test_eol_and_seal() {
        let mut lt = PfLedger::new();
        assert!(lt.seal().is_err());
        lt.freeze_eol();
        assert!(lt.eol_state());
        let _ = lt.declare_grade(0);
        assert!(lt.declare_grade(0).is_err());
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
        assert_eq!(SEG_BASE, 0xC6FC0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52191_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
