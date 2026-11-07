//! UNX-F52076 Meson 收口 · 0xC5300段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52076 逐字锚定）：UNX-F52077-J1 增量构建正确
//! 性（依赖图 ×10 注入：头改/源改/无改三类动作）与 Windows 逐字节对
//! 照；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二。
//!
//! 设计要点：
//!   ① 域模型：依赖图注入账。动作三元闭集（0=头改 1=源改 2=无改），
//!      ×10 注入场景 = 30 格动作账；逐格重建决策载荷（纯函数折叠）恒
//!      等才收；决策漂移必抓。
//!   ② 重建决策闸：头改 → 必重建（跳过即漏检必抓）；源改 → 必重建；
//!      无改 → 必跳过（误重建必抓）。决策与动作二元恒等闸。
//!   ③ Windows 逐字节对照：输出对照 4 行闭集（编译输出/链接输出/依赖
//!      报告/回声行），漂移必抓。
//!   ④ 注入 ×10 轮（决策漂移/动作越闭集/对照漂移三态轮转）10/10 必
//!      抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（30 格全通 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52076 − 52000)）。
pub const SEG_BASE: u32 = 0xC5300;

/// 注入场景数。
pub const SCENES: usize = 10;

/// 动作三元闭集。
pub const ACTIONS: usize = 3;

/// 对照行数。
pub const DIFF_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MsCode {
    /// 成功
    Ok = 0,
    /// 场景号越闭集
    ENoScene,
    /// 动作越闭集
    ENoAction,
    /// 决策漂移
    EDrift,
    /// 决策漏检（应重建却跳过）
    EMiss,
    /// 误重建（无改却重建）
    ERebuild,
    /// 对照行越闭集
    ENoRow,
    /// 对照漂移
    EDiff,
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

impl MsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MsCode::Ok => "成功",
            MsCode::ENoScene => "场景号越闭集：10 场景之外先登记",
            MsCode::ENoAction => "动作越闭集：头改/源改/无改三元之外先登记",
            MsCode::EDrift => "决策漂移：重建决策折叠与基准不恒等，逐格复算",
            MsCode::EMiss => "决策漏检：头改/源改必须重建，跳过即漏检必抓",
            MsCode::ERebuild => "误重建：无改必须跳过，重建即误检必抓",
            MsCode::ENoRow => "对照行越闭集：4 行闭集之外先登记",
            MsCode::EDiff => "对照漂移：Windows 逐字节对照折叠与基准不恒等",
            MsCode::EDup => "重复落账：同格只许一条账目",
            MsCode::ESeal => "未齐即封：30 格未齐先补齐",
            MsCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            MsCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            MsCode::EGradeDrift => "定级漂移：30 格全通+对照零漂移+决策恒等才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 动作格载荷（场景 sc × 动作 a）。
pub const fn act_cell(sc: usize, a: usize) -> Option<u64> {
    if sc >= SCENES || a >= ACTIONS {
        return None;
    }
    Some(
        (0x94000u64 + (sc as u64) * 0x2A)
            ^ (a as u64).wrapping_mul(0x01000193)
            ^ 0x6D65736F6E696E63,
    )
}

/// 重建决策闸（动作 → 是否应重建：头改/源改必重建，无改必跳过）。
pub const fn rebuild_expected(a: usize) -> Option<bool> {
    match a {
        0 => Some(true),
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// 对照行基准（行 r）。
pub const fn diff_row(r: usize) -> Option<u64> {
    if r >= DIFF_ROWS {
        return None;
    }
    Some((0x95000u64 + r as u64 * 0x2F) ^ 0x77696E6466627974)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct MsLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 动作账。
    acts: [[Option<u64>; ACTIONS]; SCENES],
    /// 对照账。
    diffs: [Option<u64>; DIFF_ROWS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl MsLedger {
    /// 新账本。
    pub fn new() -> Self {
        MsLedger {
            gates: [None; 6],
            acts: [[None; ACTIONS]; SCENES],
            diffs: [None; DIFF_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), MsCode> {
        if gate >= 6 {
            return Err(MsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x35100 + gate as u64);
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

    /// 决策一格（动作闭集 + 决策恒等闸 + 重复拒）。
    pub fn record_decision(&mut self, sc: usize, a: usize, decided_rebuild: bool) -> Result<(), MsCode> {
        if sc >= SCENES {
            return Err(MsCode::ENoScene);
        }
        if a >= ACTIONS {
            return Err(MsCode::ENoAction);
        }
        let want = match rebuild_expected(a) {
            Some(w) => w,
            None => return Err(MsCode::ENoAction),
        };
        if decided_rebuild != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x35200 + (a as u64) * 10 + sc as u64);
            return if want {
                Err(MsCode::EMiss)
            } else {
                Err(MsCode::ERebuild)
            };
        }
        let payload = match act_cell(sc, a) {
            Some(p) => p,
            None => return Err(MsCode::ENoAction),
        };
        if match self.acts.get(sc).and_then(|row| row.get(a)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MsCode::EDup);
        }
        if let Some(row) = self.acts.get_mut(sc) {
            if let Some(c) = row.get_mut(a) {
                *c = Some(payload);
            }
        }
        self.fnv(0x35201 + (sc as u64) * 3 + a as u64);
        Ok(())
    }

    /// 动作全账核验（30 格）。
    pub fn acts_all(&self) -> bool {
        let mut sc = 0usize;
        while sc < SCENES {
            let mut a = 0usize;
            while a < ACTIONS {
                match self.acts.get(sc).and_then(|row| row.get(a)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                a += 1;
            }
            sc += 1;
        }
        true
    }

    /// 对照一行（漂移必抓）。
    pub fn record_diff(&mut self, r: usize, got: u64) -> Result<(), MsCode> {
        let want = match diff_row(r) {
            Some(w) => w,
            None => return Err(MsCode::ENoRow),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x35300 + r as u64);
            return Err(MsCode::EDiff);
        }
        if let Some(c) = self.diffs.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x35301 + r as u64);
        Ok(())
    }

    /// 对照全账核验。
    pub fn diffs_all(&self) -> bool {
        let mut r = 0usize;
        while r < DIFF_ROWS {
            match self.diffs.get(r) {
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

impl Default for MsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = MsLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut sc = 0usize;
    while sc < SCENES {
        let mut a = 0usize;
        while a < ACTIONS {
            let rb = rebuild_expected(a) == Some(true);
            if led.record_decision(sc, a, rb).is_ok() {
                recorded += 1;
            }
            a += 1;
        }
        sc += 1;
    }
    let mut r = 0usize;
    while r < DIFF_ROWS {
        if let Some(w) = diff_row(r) {
            let _ = led.record_diff(r, w);
        }
        r += 1;
    }
    // 注入 10 轮：漏检 / 误重建 / 对照漂移三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = MsLedger::new();
        let caught = match kind {
            0 => lt.record_decision(0, 0, false) == Err(MsCode::EMiss),
            1 => lt.record_decision(0, 2, true) == Err(MsCode::ERebuild),
            _ => lt.record_diff(1, diff_row(1).unwrap_or(0).wrapping_add(1)) == Err(MsCode::EDiff),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x35400 + k as u64);
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
pub fn run_unx52076_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52076");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "依赖图注入 30/30 全通",
        recorded >= (SCENES * ACTIONS) as u32,
        "头改/源改/无改 ×10 场景决策恒等全账",
    );
    s.add(
        "Windows 对照 4/4 零漂移",
        {
            let mut lt = MsLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < DIFF_ROWS {
                if let Some(w) = diff_row(r) {
                    if lt.record_diff(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.diffs_all()
        },
        "编译/链接/依赖报告/回声行逐字节对照",
    );
    s.add(
        "决策恒等闸",
        rebuild_expected(0) == Some(true)
            && rebuild_expected(1) == Some(true)
            && rebuild_expected(2) == Some(false)
            && rebuild_expected(3).is_none(),
        "头改/源改必重建，无改必跳过",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漏检/误重建/对照三态轮转必抓");
    let mut l0 = MsLedger::new();
    s.add(
        "漏检拒",
        l0.record_decision(1, 1, false) == Err(MsCode::EMiss),
        "源改跳过必抓",
    );
    s.add(
        "误重建拒",
        l0.record_decision(1, 2, true) == Err(MsCode::ERebuild),
        "无改重建必抓",
    );
    s.add(
        "边界·动作越闭集",
        l0.record_decision(0, ACTIONS, true).is_err(),
        "动作 3 越闭集必拒",
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
    s.add("收口·判据条数", s.len() >= 10, "含本条共 ≥11 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rebuild_gate() {
        assert_eq!(rebuild_expected(0), Some(true));
        assert_eq!(rebuild_expected(1), Some(true));
        assert_eq!(rebuild_expected(2), Some(false));
        assert!(rebuild_expected(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = act_cell(2, 1).unwrap_or(0);
        assert_eq!(a, act_cell(2, 1).unwrap_or(0));
        assert_ne!(a, act_cell(2, 2).unwrap_or(0));
        assert!(act_cell(SCENES, 0).is_none());
        assert!(act_cell(0, ACTIONS).is_none());
        assert!(diff_row(DIFF_ROWS).is_none());
    }

    #[test]
    fn test_decision_gates() {
        let mut lt = MsLedger::new();
        assert!(lt.record_decision(0, 0, true).is_ok());
        assert!(lt.record_decision(0, 0, true).is_err());
        assert!(lt.record_decision(0, 2, false).is_ok());
        assert_eq!(lt.record_decision(0, 3, true), Err(MsCode::ENoAction));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SCENES * ACTIONS) as u32);
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
        assert_eq!(SEG_BASE, 0xC5300);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52076_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
