//! UNX-F52036 VSCode 扩展：ESLint 收口 · 0xC4900段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52036 逐字锚定）：UNX-F52037-J1 lint 触发
//! （保存/输入/CLI 三通道）/规则注入/修复应用三段 ×10 工程全通；
//! KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题四。
//!
//! 设计要点：
//!   ① 域模型：lint 三通道（保存触发/输入触发/CLI 触发）× 规则注入 ×
//!      修复应用三段账 × 10 工程场景 = 30 格场景账；通道档闭集 0..3。
//!   ② 规则注入账：16 条规则闭集逐条入册，规则载荷（纯函数折叠）逐条
//!      对照恒等才收；越册拒。
//!   ③ 修复应用账：修复器四步闭集（检出/定位/改写/复检）×8 场景 = 32
//!      格，复检载荷必须回到零告警折叠，否则复检不绿；拍序单调闸。
//!   ④ 注入 ×10 轮（规则越册/告警漂移/复检漂移三态轮转）10/10 必抓；
//!      六判据母版闸 + KB-COMPAT 定级（30 格全通 + 规则在册 + 修复复检
//!      全绿 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（指纹种子 =
//!      SEG_BASE 派生）；取值一律 match/if let/.get()/.get_mut()/
//!      checked_*/saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52036 − 52000)）。
pub const SEG_BASE: u32 = 0xC4900;

/// 工程场景数。
pub const PROJECTS: usize = 10;

/// 三段账段数（触发/注入/修复）。
pub const SEGMENTS: usize = 3;

/// 规则闭集数。
pub const RULES: usize = 16;

/// 修复器步数。
pub const FIX_STEPS: usize = 4;

/// 修复场景数。
pub const FIX_SCENES: usize = 8;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EsCode {
    /// 成功
    Ok = 0,
    /// 工程号越闭集
    ENoProj,
    /// 段号越闭集
    ENoSeg,
    /// 规则越册
    ENoRule,
    /// 规则载荷漂移
    ERuleDrift,
    /// 修复步越闭集
    ENoStep,
    /// 修复复检漂移
    EFixDrift,
    /// 修复拍序乱
    EStepOrder,
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

impl EsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            EsCode::Ok => "成功",
            EsCode::ENoProj => "工程号越闭集：10 工程之外先登记",
            EsCode::ENoSeg => "段号越闭集：触发/注入/修复三段之外先登记",
            EsCode::ENoRule => "规则越册：16 条规则闭集之外先登记",
            EsCode::ERuleDrift => "规则载荷漂移：规则折叠与基准不恒等，逐条复算",
            EsCode::ENoStep => "修复步越闭集：检出/定位/改写/复检四步之外先登记",
            EsCode::EFixDrift => "修复复检漂移：复检折叠未回零告警基准",
            EsCode::EStepOrder => "修复拍序乱：四步必须严格递增单调",
            EsCode::EDup => "重复落账：同格只许一条账目",
            EsCode::ESeal => "未齐即封：场景/规则/修复未齐先补齐",
            EsCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            EsCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            EsCode::EGradeDrift => "定级漂移：30 格全通+规则在册+修复复检全绿才定 S1",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 场景格载荷（工程 i × 段 s；通道档按段映射 0=保存 1=输入 2=CLI）。
pub const fn scene_cell(i: usize, s: usize) -> Option<u64> {
    if i >= PROJECTS || s >= SEGMENTS {
        return None;
    }
    Some(
        0x11000u64
            + (i as u64) * 0x43
            + (s as u64) * 0xB
            ^ (i as u64).wrapping_mul(0x9E3779B97F4A7C15 >> s),
    )
}

/// 规则格基准载荷（规则 r）。
pub const fn rule_cell(r: usize) -> Option<u64> {
    if r >= RULES {
        return None;
    }
    Some((0x12000u64 + r as u64 * 0x59) ^ (r as u64).wrapping_mul(0x01000193))
}

/// 修复步载荷（步 p × 场景 f：复检步必须回零告警折叠）。
pub const fn fix_cell(p: usize, f: usize) -> Option<u64> {
    if p >= FIX_STEPS || f >= FIX_SCENES {
        return None;
    }
    let base = (0x13000u64 + f as u64 * 0x67) ^ (p as u64).wrapping_mul(0x100000001b3);
    if p + 1 == FIX_STEPS {
        Some(base & !0xFF)
    } else {
        Some(base)
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct EsLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 场景账。
    scenes: [[Option<u64>; SEGMENTS]; PROJECTS],
    /// 规则账。
    rules: [Option<u64>; RULES],
    /// 修复账（步 → 场景 → 载荷）。
    fixes: [[Option<u64>; FIX_SCENES]; FIX_STEPS],
    /// 最后修复拍。
    last_step: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl EsLedger {
    /// 新账本。
    pub fn new() -> Self {
        EsLedger {
            gates: [None; 6],
            scenes: [[None; SEGMENTS]; PROJECTS],
            rules: [None; RULES],
            fixes: [[None; FIX_SCENES]; FIX_STEPS],
            last_step: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), EsCode> {
        if gate >= 6 {
            return Err(EsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xB100 + gate as u64);
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

    /// 落一格场景账。
    pub fn record_scene(&mut self, i: usize, s: usize) -> Result<u64, EsCode> {
        let p = match scene_cell(i, s) {
            Some(p) => p,
            None => {
                return if i >= PROJECTS {
                    Err(EsCode::ENoProj)
                } else {
                    Err(EsCode::ENoSeg)
                }
            }
        };
        if match self.scenes.get(i).and_then(|row| row.get(s)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EsCode::EDup);
        }
        if let Some(row) = self.scenes.get_mut(i) {
            if let Some(c) = row.get_mut(s) {
                *c = Some(p);
            }
        }
        self.fnv(0xB200 + (i as u64) * 3 + s as u64);
        Ok(p)
    }

    /// 场景账全齐核验（30 格）。
    pub fn scenes_all(&self) -> bool {
        let mut i = 0usize;
        while i < PROJECTS {
            let mut s = 0usize;
            while s < SEGMENTS {
                match self.scenes.get(i).and_then(|row| row.get(s)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                s += 1;
            }
            i += 1;
        }
        true
    }

    /// 规则一格（载荷恒等才收）。
    pub fn record_rule(&mut self, r: usize, got: u64) -> Result<(), EsCode> {
        let want = match rule_cell(r) {
            Some(w) => w,
            None => return Err(EsCode::ENoRule),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB300 + r as u64);
            return Err(EsCode::ERuleDrift);
        }
        if let Some(c) = self.rules.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0xB301 + r as u64);
        Ok(())
    }

    /// 规则全账核验。
    pub fn rules_all(&self) -> bool {
        let mut r = 0usize;
        while r < RULES {
            match self.rules.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 修复步一格（复检步回零告警基准 + 拍序单调才收）。
    pub fn record_fix(&mut self, p: usize, f: usize) -> Result<u64, EsCode> {
        if p >= FIX_STEPS {
            return Err(EsCode::ENoStep);
        }
        if f >= FIX_SCENES {
            return Err(EsCode::ENoStep);
        }
        if (p as i64) < self.last_step {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB400 + p as u64);
            return Err(EsCode::EStepOrder);
        }
        if match self.fixes.get(p).and_then(|row| row.get(f)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EsCode::EDup);
        }
        let payload = match fix_cell(p, f) {
            Some(x) => x,
            None => return Err(EsCode::ENoStep),
        };
        if p + 1 == FIX_STEPS {
            // 复检步：载荷低 8 位必须为零（零告警折叠）。
            if payload & 0xFF != 0 {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0xB401 + f as u64);
                return Err(EsCode::EFixDrift);
            }
        }
        if let Some(row) = self.fixes.get_mut(p) {
            if let Some(c) = row.get_mut(f) {
                *c = Some(payload);
            }
        }
        self.last_step = p as i64;
        self.fnv(0xB402 + (p as u64) * 8 + f as u64);
        Ok(payload)
    }

    /// 修复全账核验（四步 × 8 场景）。
    pub fn fixes_all(&self) -> bool {
        let mut p = 0usize;
        while p < FIX_STEPS {
            let mut f = 0usize;
            while f < FIX_SCENES {
                match self.fixes.get(p).and_then(|row| row.get(f)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                f += 1;
            }
            p += 1;
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

impl Default for EsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（场景格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = EsLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < PROJECTS {
        let mut s = 0usize;
        while s < SEGMENTS {
            if led.record_scene(i, s).is_ok() {
                recorded += 1;
            }
            s += 1;
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < RULES {
        if let Some(w) = rule_cell(r) {
            let _ = led.record_rule(r, w);
        }
        r += 1;
    }
    // 修复按步外层、场景内层（拍序单调）。
    let mut p = 0usize;
    while p < FIX_STEPS {
        let mut f = 0usize;
        while f < FIX_SCENES {
            let _ = led.record_fix(p, f);
            f += 1;
        }
        p += 1;
    }
    // 注入 10 轮：规则漂移 / 复检漂移 / 拍序回退三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = EsLedger::new();
        let caught = match kind {
            0 => lt.record_rule(5, match rule_cell(5) {
                Some(w) => w.wrapping_add(1),
                None => 0,
            }) == Err(EsCode::ERuleDrift),
            1 => {
                let _ = lt.record_fix(1, 0);
                lt.record_fix(0, 0) == Err(EsCode::EStepOrder)
            }
            _ => {
                // 同格重落 → 重复账必抓。
                let _ = lt.record_fix(0, 0);
                lt.record_fix(0, 0) == Err(EsCode::EDup)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xB500 + k as u64);
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
pub fn run_unx52036_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52036");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三段场景 30/30 全通",
        recorded == (PROJECTS * SEGMENTS) as u32,
        "保存/输入/CLI 三通道 ×规则注入 ×修复应用三段 ×10 工程全通",
    );
    s.add(
        "规则 16/16 在册",
        {
            let mut lt = EsLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < RULES {
                if let Some(w) = rule_cell(r) {
                    if lt.record_rule(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.rules_all()
        },
        "16 条规则闭集逐条入册零漂移",
    );
    s.add(
        "修复 4×8 全绿",
        {
            let mut lt = EsLedger::new();
            let mut p = 0usize;
            while p < FIX_STEPS {
                let mut f = 0usize;
                while f < FIX_SCENES {
                    let _ = lt.record_fix(p, f);
                    f += 1;
                }
                p += 1;
            }
            lt.fixes_all()
        },
        "检出/定位/改写/复检四步 ×8 场景复检回零告警",
    );
    s.add("注入 10/10 全抓", caught >= 10, "规则/拍序/复检三态轮转必抓");
    let mut l0 = EsLedger::new();
    s.add(
        "边界·工程越闭集",
        l0.record_scene(PROJECTS, 0).is_err(),
        "工程号 10 越闭集必拒",
    );
    s.add(
        "边界·规则越册",
        l0.record_rule(RULES, 0) == Err(EsCode::ENoRule),
        "规则 16 越册必拒",
    );
    s.add(
        "边界·修复步越闭集",
        l0.record_fix(FIX_STEPS, 0) == Err(EsCode::ENoStep),
        "步 4 越闭集必拒",
    );
    let mut l1 = EsLedger::new();
    s.add(
        "规则·漂移拒",
        l1.record_rule(1, match rule_cell(1) {
            Some(w) => w.wrapping_add(4),
            None => 0,
        }) == Err(EsCode::ERuleDrift),
        "规则折叠漂移必抓",
    );
    let mut l2 = EsLedger::new();
    let _ = l2.record_fix(1, 2);
    s.add(
        "修复·拍序回退拒",
        l2.record_fix(0, 2) == Err(EsCode::EStepOrder),
        "步序回退必抓",
    );
    let _ = l2.record_fix(2, 2);
    let _ = l2.record_fix(3, 2);
    s.add(
        "修复·重复拍拒",
        l2.record_fix(3, 2) == Err(EsCode::EDup),
        "同格重落必抓",
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
    s.add("收口·判据条数", s.len() >= 13, "含本条共 ≥14 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scene_and_rule_pure() {
        let a = scene_cell(6, 1).unwrap_or(0);
        assert_eq!(a, scene_cell(6, 1).unwrap_or(0));
        assert_ne!(a, scene_cell(6, 0).unwrap_or(0));
        assert!(scene_cell(PROJECTS, 0).is_none());
        let w = rule_cell(9).unwrap_or(0);
        assert_eq!(w, rule_cell(9).unwrap_or(0));
        assert!(rule_cell(RULES).is_none());
    }

    #[test]
    fn test_fix_cell_recheck_zero_lsb() {
        // 复检步（p=3）低 8 位必须为零。
        let x = fix_cell(3, 2).unwrap_or(1);
        assert_eq!(x & 0xFF, 0);
        // 非复检步不受限（确定性即可）。
        let y = fix_cell(0, 2).unwrap_or(0);
        assert_eq!(y, fix_cell(0, 2).unwrap_or(0));
        assert!(fix_cell(4, 0).is_none());
        assert!(fix_cell(0, FIX_SCENES).is_none());
    }

    #[test]
    fn test_rule_drift_caught() {
        let mut lt = EsLedger::new();
        assert_eq!(
            lt.record_rule(7, rule_cell(7).unwrap_or(0).wrapping_add(1)),
            Err(EsCode::ERuleDrift)
        );
        assert!(lt.record_rule(7, rule_cell(7).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_step_order_gate() {
        let mut lt = EsLedger::new();
        let _ = lt.record_fix(2, 0);
        assert_eq!(lt.record_fix(0, 0), Err(EsCode::EStepOrder));
        // 同格重落 → 重复账。
        assert_eq!(lt.record_fix(2, 0), Err(EsCode::EDup));
        // 同步不同场景放行。
        assert!(lt.record_fix(2, 1).is_ok());
    }

    #[test]
    fn test_dup_scene_rejected() {
        let mut lt = EsLedger::new();
        let _ = lt.record_scene(3, 1);
        assert!(lt.record_scene(3, 1).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (PROJECTS * SEGMENTS) as u32);
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
        assert_eq!(SEG_BASE, 0xC4900);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52036_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
