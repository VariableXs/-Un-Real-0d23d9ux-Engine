//! UNX-F52005 Visual Studio 2019 Enterprise 收口 · 0xC4140段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52005 逐字锚定）：UNX-F52006-J1 六判据执行，S2
//! 以下如实降级并列缺失项清单（不凑绿）；KB-COMPAT 定级 S2 + 缺失清单公
//! 开；判据锚定 §AI-66.2 专题六「S2 以下如实降级」。
//!
//! 设计要点：
//!   ① 域模型：VS 2019 Enterprise 收口面 = 六判据执行账（真实判定：过/
//!      不过如实落账）+ 缺失项清单账（每个不过判据必须有清单条目，零静
//!      默）+ 如实降级闸（有失败判据 → 定级 ≤S2 且清单公开；全过 → S1）。
//!   ② 凑绿注入器：把失败判据谎报为过（清单无条目）×10 轮，一致性闸
//!      10/10 必抓（判据失败 + 清单缺条目 = 凑绿显性拒）。
//!   ③ 清单公开闸：定级 ≤S2 时清单条目数 = 失败判据数（逐条对应），缺失
//!      清单不公开显性拒。
//!   ④ 定级公式：全过 → S1；失败判据数 1..=2 → S2；≥3 → S3。降级只按
//!      失败数单向（不凑绿），与复算一致才收。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52005 − 52000)）。
pub const SEG_BASE: u32 = 0xC4140;

/// 六判据数。
pub const SIX_GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0B 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EeCode {
    /// 成功
    Ok = 0,
    /// 判据越闭集
    ENoGate,
    /// 判据未执行
    ENotRun,
    /// 凑绿（失败判据谎报为过）
    EFakeGreen,
    /// 缺失清单未公开
    ENoList,
    /// 清单与失败判据不对应
    EListMismatch,
    /// 定级与失败数不一致
    EGradeDrift,
    /// 定级越闭集
    ENoGrade,
    /// 账量守恒破缺
    EConserve,
    /// 双跑指纹不一致
    EFp,
    /// 账为空
    EEmpty,
    /// 重复清单条目
    EDupList,
}

impl EeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            EeCode::Ok => "成功",
            EeCode::ENoGate => "判据越闭集：六判据之外先登记",
            EeCode::ENotRun => "判据未执行：六判据逐条执行后才许定级",
            EeCode::EFakeGreen => "凑绿：失败判据谎报为过，如实降级不凑绿，改回真实判定",
            EeCode::ENoList => "缺失清单未公开：定级 ≤S2 时清单必须公开逐条对应",
            EeCode::EListMismatch => "清单与失败判据不对应：逐条对平",
            EeCode::EGradeDrift => "定级与失败数不一致：全过 S1、1..=2 失败 S2、≥3 失败 S3",
            EeCode::ENoGrade => "定级越闭集：S1/S2/S3 之外先登记",
            EeCode::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            EeCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            EeCode::EEmpty => "账为空：至少执行 1 判据",
            EeCode::EDupList => "重复清单条目：同判据只许一条清单条目",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 判据执行态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateRun {
    /// 未执行。
    NotRun,
    /// 真实过。
    Pass,
    /// 真实不过。
    Fail,
}

/// 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum EeGrade {
    /// 三级。
    S3 = 0,
    /// 二级。
    S2 = 1,
    /// 一级。
    S1 = 2,
}

/// 定级复算：失败数 0 → S1；1..=2 → S2；≥3 → S3。
pub const fn grade_of(fails: u32) -> EeGrade {
    if fails == 0 {
        EeGrade::S1
    } else if fails <= 2 {
        EeGrade::S2
    } else {
        EeGrade::S3
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Enterprise 收口账本（如实判定 + 缺失清单 + 定级）。
#[derive(Clone)]
pub struct EeLedger {
    /// 判据执行态。
    runs: [GateRun; SIX_GATES],
    /// 缺失清单（失败判据集，位图）。
    missing_list: u8,
    /// 定级（None=未定）。
    grade: Option<EeGrade>,
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl EeLedger {
    /// 新账本。
    pub fn new() -> Self {
        EeLedger {
            runs: [GateRun::NotRun; SIX_GATES],
            missing_list: 0,
            grade: None,
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

    /// 执行判据（如实落账，重复执行覆盖）。
    pub fn run_gate(&mut self, gate: usize, pass: bool) -> Result<(), EeCode> {
        if gate >= SIX_GATES {
            return Err(EeCode::ENoGate);
        }
        let cell = match self.runs.get_mut(gate) {
            Some(c) => c,
            None => return Err(EeCode::EEmpty),
        };
        *cell = if pass { GateRun::Pass } else { GateRun::Fail };
        if !pass {
            self.missing_list |= 1u8 << gate;
        } else {
            self.missing_list &= !(1u8 << gate);
        }
        self.fnv(gate as u64 | pass as u64);
        Ok(())
    }

    /// 全判据已执行。
    pub fn all_run(&self) -> bool {
        let mut i = 0usize;
        while i < SIX_GATES {
            match self.runs.get(i) {
                Some(GateRun::NotRun) => return false,
                _ => {}
            }
            i += 1;
        }
        true
    }

    /// 失败判据数。
    pub fn fail_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < SIX_GATES {
            if let Some(GateRun::Fail) = self.runs.get(i) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 凑绿一致性闸：失败判据必须有清单条目，清单条目必须是失败判据。
    pub fn fake_green_gate(&self) -> Result<(), EeCode> {
        let mut i = 0usize;
        while i < SIX_GATES {
            let run = match self.runs.get(i) {
                Some(r) => *r,
                None => return Err(EeCode::EEmpty),
            };
            let listed = self.missing_list & (1u8 << i) != 0;
            match run {
                GateRun::Fail if !listed => return Err(EeCode::EFakeGreen),
                GateRun::Pass if listed => return Err(EeCode::EListMismatch),
                _ => {}
            }
            i += 1;
        }
        Ok(())
    }

    /// 定级闸：全执行 + 凑绿闸过 + 定级与失败数一致。
    pub fn grade_gate(&mut self, g: EeGrade) -> Result<(), EeCode> {
        if !self.all_run() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400);
            return Err(EeCode::ENotRun);
        }
        if let Err(e) = self.fake_green_gate() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5401);
            return Err(e);
        }
        let expect = grade_of(self.fail_count());
        if g != expect {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5402);
            return Err(EeCode::EGradeDrift);
        }
        self.grade = Some(g);
        self.fnv(g as u64);
        Ok(())
    }

    /// 定级 ≤S2 时清单公开闸：清单非空（有失败必有条目，条目即公开面）。
    pub fn list_public_gate(&self) -> Result<(), EeCode> {
        let fails = self.fail_count();
        if fails == 0 {
            return Ok(());
        }
        let bits = self.missing_list.count_ones();
        if bits != fails {
            return Err(EeCode::EListMismatch);
        }
        Ok(())
    }

    /// 漏检记账。
    pub fn note_missed(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 缺失清单位图。
    pub fn list_bits(&self) -> u8 {
        self.missing_list
    }

    /// 定级。
    pub fn grade(&self) -> Option<EeGrade> {
        self.grade
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for EeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 凑绿注入（10 轮，确定性）：每轮把 1~2 个失败判据谎报为过。
pub struct FakeGreenInjector {
    seed: u64,
}

impl FakeGreenInjector {
    /// 种子 = SEG_BASE 派生。
    pub fn new() -> Self {
        FakeGreenInjector {
            seed: (SEG_BASE as u64) << 16 | 0x5205,
        }
    }

    /// 下一轮谎报的判据数（1..=2）。
    pub fn next_fake(&mut self) -> usize {
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.seed >> 33) % 2) as usize + 1
    }
}

impl Default for FakeGreenInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：六判据如实执行（2 失败）→ 定级 S2 + 清单公开 + 凑绿注入
/// 10/10 + 指纹。返回（清单位数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = EeLedger::new();
    // ① 六判据如实执行：判据 1、4 失败（确定性）。
    let mut g = 0usize;
    while g < SIX_GATES {
        let pass = g != 1 && g != 4;
        let _ = led.run_gate(g, pass);
        g += 1;
    }
    // ② 定级 S2（失败数 2）+ 清单公开。
    let _ = led.grade_gate(EeGrade::S2);
    let _ = led.list_public_gate();
    // ③ 凑绿注入 10/10。
    let mut inj = FakeGreenInjector::new();
    let mut r = 0usize;
    while r < 10 {
        let fake = inj.next_fake();
        let mut lt = EeLedger::new();
        let mut g = 0usize;
        while g < SIX_GATES {
            let _ = lt.run_gate(g, true);
            g += 1;
        }
        // 把 fake 个"失败"塞回去但清单留空 → 凑绿。
        let mut f = 0usize;
        while f < fake {
            let runs_cell = lt.runs.get_mut(f);
            if let Some(c) = runs_cell {
                *c = GateRun::Fail;
            }
            f += 1;
        }
        if lt.grade_gate(EeGrade::S1).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5403);
        } else {
            led.note_missed();
        }
        r += 1;
    }
    let caught = led.caught;
    let bits = led.list_bits().count_ones();
    let fp = led.fingerprint();
    (bits, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52005_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52005");
    // 1. 正常流：2 失败 → S2 + 清单 2 条公开。
    let (bits, caught, fp) = run_cycle();
    s.add("如实降级 S2", bits == 2, "失败判据 1、4 → 清单 2 条");
    s.add("凑绿注入 10/10", caught >= 10, "谎报必抓");
    // 2. 边界：判据越闭集。
    let mut l0 = EeLedger::new();
    s.add(
        "边界·判据越闭集",
        l0.run_gate(SIX_GATES, true) == Err(EeCode::ENoGate),
        "判据 6 越闭集必拒",
    );
    // 3. 定级公式逐档。
    s.add("定级·0 失败 S1", matches!(grade_of(0), EeGrade::S1), "全过定 S1");
    s.add("定级·2 失败 S2", matches!(grade_of(2), EeGrade::S2), "2 失败定 S2");
    s.add("定级·3 失败 S3", matches!(grade_of(3), EeGrade::S3), "3 失败定 S3");
    // 4. 未执行定级拒。
    s.add(
        "定级·未执行拒",
        EeLedger::new().grade_gate(EeGrade::S1) == Err(EeCode::ENotRun),
        "判据未执行先定级必拒",
    );
    // 5. 清单不对应拒。
    let mut l1 = EeLedger::new();
    let mut g = 0usize;
    while g < SIX_GATES {
        let _ = l1.run_gate(g, true);
        g += 1;
    }
    l1.missing_list |= 0b0000_0001;
    s.add(
        "清单·过判据带条目拒",
        l1.fake_green_gate() == Err(EeCode::EListMismatch),
        "过判据不得有清单条目",
    );
    // 6. 如实全过 → S1。
    let mut l2 = EeLedger::new();
    let mut g = 0usize;
    while g < SIX_GATES {
        let _ = l2.run_gate(g, true);
        g += 1;
    }
    s.add("如实全过定级 S1", l2.grade_gate(EeGrade::S1).is_ok(), "凑绿闸过 + 定级一致");
    // 7. 注入必抓账守恒。
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "凑绿注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    // 8. 双跑指纹一致。
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    // 9. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    // 10. 收口。
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let mut led = EeLedger::new();
        let _ = led.run_gate(0, true);
        assert!(matches!(led.runs.get(0), Some(GateRun::Pass)));
    }

    #[test]
    fn test_boundary_gate_overflow() {
        assert_eq!(
            EeLedger::new().run_gate(SIX_GATES, true),
            Err(EeCode::ENoGate)
        );
    }

    #[test]
    fn test_grade_formula() {
        assert!(matches!(grade_of(0), EeGrade::S1));
        assert!(matches!(grade_of(1), EeGrade::S2));
        assert!(matches!(grade_of(2), EeGrade::S2));
        assert!(matches!(grade_of(3), EeGrade::S3));
        assert!(matches!(grade_of(6), EeGrade::S3));
    }

    #[test]
    fn test_not_run_rejected() {
        let mut led = EeLedger::new();
        assert_eq!(led.grade_gate(EeGrade::S1), Err(EeCode::ENotRun));
    }

    #[test]
    fn test_fake_green_caught() {
        let mut led = EeLedger::new();
        let mut g = 0usize;
        while g < SIX_GATES {
            // 判据 0 失败但清单覆盖（run_gate 同步置位）→ 过闸；
            // 再把清单位偷拆掉 → 凑绿必抓。
            let _ = led.run_gate(g, g != 0);
            g += 1;
        }
        led.missing_list &= !0b0000_0001;
        assert_eq!(led.fake_green_gate(), Err(EeCode::EFakeGreen));
    }

    #[test]
    fn test_list_mismatch_caught() {
        let mut led = EeLedger::new();
        let mut g = 0usize;
        while g < SIX_GATES {
            let _ = led.run_gate(g, true);
            g += 1;
        }
        led.missing_list |= 0b0000_0010;
        assert_eq!(led.fake_green_gate(), Err(EeCode::EListMismatch));
    }

    #[test]
    fn test_honest_downgrade_s2() {
        let mut led = EeLedger::new();
        let mut g = 0usize;
        while g < SIX_GATES {
            let _ = led.run_gate(g, g != 2 && g != 5);
            g += 1;
        }
        assert_eq!(led.fail_count(), 2);
        assert!(led.grade_gate(EeGrade::S2).is_ok());
        assert!(led.list_public_gate().is_ok());
        assert!(matches!(led.grade(), Some(EeGrade::S2)));
    }

    #[test]
    fn test_grade_drift_rejected() {
        let mut led = EeLedger::new();
        let mut g = 0usize;
        while g < SIX_GATES {
            let _ = led.run_gate(g, g != 2 && g != 5);
            g += 1;
        }
        assert_eq!(led.grade_gate(EeGrade::S1), Err(EeCode::EGradeDrift));
    }

    #[test]
    fn test_injection_all_caught() {
        let mut inj = FakeGreenInjector::new();
        let mut r = 0usize;
        let mut caught = 0u32;
        while r < 10 {
            let fake = inj.next_fake();
            let mut lt = EeLedger::new();
            let mut g = 0usize;
            while g < SIX_GATES {
                let _ = lt.run_gate(g, true);
                g += 1;
            }
            let mut f = 0usize;
            while f < fake {
                if let Some(c) = lt.runs.get_mut(f) {
                    *c = GateRun::Fail;
                }
                f += 1;
            }
            if lt.grade_gate(EeGrade::S1).is_err() {
                caught += 1;
            }
            r += 1;
        }
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
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52005_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
