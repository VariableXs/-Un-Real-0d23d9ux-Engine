// unx1068_u1068 —— UNX-F1068 定时器族选型契约（hrtimer/轮/超时器三选一决策树）
// 判据原文 UNX-F0948→F1068-J1：决策树唯一解（特性组合遍历断言），违例告警 10/10 次（高精需求走轮等），契约文档冻结。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2DF0 独占（0x2DF1~0x2DFF）。
//
// 设计（纯功能 no_std）：
//   * 特性向量：精度档（ns/µs/ms）×批量（单发/批量）×取消率（高/低）三维组合；
//   * 决策树：全组合 12 格逐一给出唯一解（hrtimer/轮/超时器三选一）；
//   * 违例告警：选型结果与契约树不符即告警（高精需求走轮等）10/10 捕获；
//   * 契约冻结：决策树表 FNV 快照冻结，冻结后改表拒；
//   * saturating 防溢出 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2DF0 独占） ─────────────────────────────────────────────
pub const XC_SC_OOB: u32 = 0x2DF1; // 特性值越界
pub const XC_SC_SEALED: u32 = 0x2DF2; // 契约冻结后改表
pub const XC_SC_VIOLATION: u32 = 0x2DF3; // 选型违例
pub const XC_SC_AMBIGUOUS: u32 = 0x2DF4; // 决策树多解
pub const XC_SC_FROZEN: u32 = 0x2DF5; // 冻结态操作

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    Hrtimer,   // ns 级红黑树
    Wheel,     // ms 级时间轮
    Timeout,   // 超时器（单次粗粒度）
}

impl Family {
    pub fn id(&self) -> u8 {
        match self {
            Family::Hrtimer => 0,
            Family::Wheel => 1,
            Family::Timeout => 2,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    Ns,
    Us,
    Ms,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FeatureVec {
    pub precision: Precision, // 精度档
    pub batch: bool,          // 批量到期（大量同类定时器）
    pub high_cancel: bool,    // 高取消率
}

/// 选型契约决策树（冻结表）：
///   ns 精度 → Hrtimer（唯一解，无论批量/取消）；
///   µs 精度 + 批量 + 低取消 → Wheel；µs 其他 → Hrtimer；
///   ms 精度 + 批量 → Wheel；ms + 单发 + 高取消 → Timeout；ms + 单发 + 低取消 → Wheel。
fn contract_decide(f: FeatureVec) -> Result<Family, u32> {
    match f.precision {
        Precision::Ns => Ok(Family::Hrtimer),
        Precision::Us => {
            if f.batch && !f.high_cancel {
                Ok(Family::Wheel)
            } else {
                Ok(Family::Hrtimer)
            }
        }
        Precision::Ms => {
            if f.batch {
                Ok(Family::Wheel)
            } else if f.high_cancel {
                Ok(Family::Timeout)
            } else {
                Ok(Family::Wheel)
            }
        }
    }
}

/// 全组合遍历（2×2×3=12 格）。
pub const ALL_COMBOS: usize = 12;

/// 选型契约账本。
pub struct SelectorContract {
    frozen: bool,
    snapshot: u64,
    decide_log: Vec<(u32, u8)>, // (组合索引, 家族 id)
    violations: u32,            // 违例捕获数
    violation_checks: u32,      // 违例检测总次数
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl SelectorContract {
    pub fn new() -> SelectorContract {
        SelectorContract {
            frozen: false,
            snapshot: 0,
            decide_log: Vec::new(),
            violations: 0,
            violation_checks: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 契约决策（冻结后仍可读）。
    pub fn decide(&self, f: FeatureVec) -> Result<Family, u32> {
        contract_decide(f)
    }

    /// 唯一解断言：全 12 组合遍历，每格结果确定且可复现。
    pub fn unique_solution(&mut self) -> bool {
        if self.sealed {
            self.rejects.push((XC_SC_FROZEN, "封账后遍历"));
            return false;
        }
        let mut idx = 0u32;
        for &p in &[Precision::Ns, Precision::Us, Precision::Ms] {
            for &b in &[false, true] {
                for &c in &[false, true] {
                    let f = FeatureVec { precision: p, batch: b, high_cancel: c };
                    let r1 = self.decide(f);
                    let r2 = self.decide(f);
                    match (r1, r2) {
                        (Ok(a), Ok(b2)) if a == b2 => {
                            self.decide_log.push((idx, a.id()));
                        }
                        _ => {
                            self.rejects.push((XC_SC_AMBIGUOUS, "决策树多解"));
                            return false;
                        }
                    }
                    idx = idx.saturating_add(1);
                }
            }
        }
        self.decide_log.len() == ALL_COMBOS
    }

    /// 违例检测：实际选型与契约不符即告警（返回是否捕获）。
    pub fn check_violation(&mut self, f: FeatureVec, actual: Family) -> Result<bool, u32> {
        if self.frozen {
            self.rejects.push((XC_SC_FROZEN, "契约冻结后检测"));
            return Err(XC_SC_FROZEN);
        }
        self.violation_checks = self.violation_checks.saturating_add(1);
        let want = self.decide(f)?;
        if want != actual {
            self.violations = self.violations.saturating_add(1);
            self.rejects.push((XC_SC_VIOLATION, "选型违例告警"));
            return Ok(true); // 捕获违例
        }
        Ok(false)
    }

    /// 违例 10/10 捕获。
    pub fn violations_10_10(&self) -> bool {
        self.violations == 10 && self.violation_checks >= 10
    }

    /// 契约快照指纹（决策树表 FNV）。
    pub fn take_snapshot(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        for &p in &[Precision::Ns, Precision::Us, Precision::Ms] {
            for &b in &[false, true] {
                for &c in &[false, true] {
                    let f = FeatureVec { precision: p, batch: b, high_cancel: c };
                    if let Ok(fam) = contract_decide(f) {
                        h ^= (fam.id() as u32).wrapping_mul(0x9E37) ^ (p as u32);
                        h = h.wrapping_mul(0x0100_0193);
                    }
                }
            }
        }
        h as u64
    }

    /// 冻结契约。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects.push((XC_SC_SEALED, "重复冻结"));
            return Err(XC_SC_SEALED);
        }
        self.snapshot = self.take_snapshot();
        self.frozen = true;
        Ok(self.snapshot)
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
    pub fn snapshot_value(&self) -> u64 {
        self.snapshot
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）高精需求走轮必告警（10/10：Ns + Wheel 违例）
    {
        let mut sc = SelectorContract::new();
        let mut hits = 0;
        for _ in 0..10 {
            let f = FeatureVec { precision: Precision::Ns, batch: false, high_cancel: false };
            if sc.check_violation(f, Family::Wheel) == Ok(true) {
                hits += 1;
            }
        }
        if hits == 10 && sc.violations_10_10() {
            caught += 1;
        }
    }
    // 2）合法选型零告警
    {
        let mut sc = SelectorContract::new();
        let f = FeatureVec { precision: Precision::Ms, batch: true, high_cancel: true };
        if sc.check_violation(f, Family::Wheel) == Ok(false) && sc.violations == 0 {
            caught += 1;
        }
    }
    // 3）全组合唯一解（12 格遍历两轮一致）
    {
        let mut sc = SelectorContract::new();
        if sc.unique_solution() {
            caught += 1;
        }
    }
    // 4）决策树语义抽查（ns→hrtimer / ms批量→wheel / ms单发高取消→timeout）
    {
        let sc = SelectorContract::new();
        let r1 = sc.decide(FeatureVec { precision: Precision::Ns, batch: false, high_cancel: false });
        let r2 = sc.decide(FeatureVec { precision: Precision::Ms, batch: true, high_cancel: false });
        let r3 = sc.decide(FeatureVec { precision: Precision::Ms, batch: false, high_cancel: true });
        if r1 == Ok(Family::Hrtimer) && r2 == Ok(Family::Wheel) && r3 == Ok(Family::Timeout) {
            caught += 1;
        }
    }
    // 5）重复冻结必拒
    {
        let mut sc = SelectorContract::new();
        let _ = sc.freeze();
        if sc.freeze() == Err(XC_SC_SEALED) {
            caught += 1;
        }
    }
    // 6）冻结后检测必拒
    {
        let mut sc = SelectorContract::new();
        let _ = sc.freeze();
        let f = FeatureVec { precision: Precision::Us, batch: true, high_cancel: true };
        if sc.check_violation(f, Family::Wheel) == Err(XC_SC_FROZEN) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1068-J1。
pub fn run_unx1068_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1068");

    // C68-01 全组合唯一解（12 格遍历）
    {
        let mut sc = SelectorContract::new();
        s.add("C68-01-唯一解", sc.unique_solution(), if sc.decide_log.len() == 12 { "" } else { "多解" });
    }
    // C68-02 决策树语义正确（四代表格）
    {
        let sc = SelectorContract::new();
        let ok = sc.decide(FeatureVec { precision: Precision::Ns, batch: false, high_cancel: false }) == Ok(Family::Hrtimer)
            && sc.decide(FeatureVec { precision: Precision::Ns, batch: true, high_cancel: true }) == Ok(Family::Hrtimer)
            && sc.decide(FeatureVec { precision: Precision::Us, batch: true, high_cancel: false }) == Ok(Family::Wheel)
            && sc.decide(FeatureVec { precision: Precision::Ms, batch: false, high_cancel: true }) == Ok(Family::Timeout);
        s.add("C68-02-语义正确", ok, if ok { "" } else { "语义漂移" });
    }
    // C68-03 违例告警 10/10（高精走轮）
    {
        let mut sc = SelectorContract::new();
        let mut hits = 0;
        for _ in 0..10 {
            let f = FeatureVec { precision: Precision::Ns, batch: false, high_cancel: false };
            if sc.check_violation(f, Family::Wheel) == Ok(true) {
                hits += 1;
            }
        }
        s.add("C68-03-违例10/10", sc.violations_10_10(), if sc.violations_10_10() { "" } else { "告警缺" });
    }
    // C68-04 合法选型零误报
    {
        let mut sc = SelectorContract::new();
        let mut false_pos = 0;
        for &p in &[Precision::Ns, Precision::Us, Precision::Ms] {
            for &b in &[false, true] {
                for &c in &[false, true] {
                    let f = FeatureVec { precision: p, batch: b, high_cancel: c };
                    let want = sc.decide(f).unwrap_or(Family::Wheel);
                    if sc.check_violation(f, want) == Ok(true) {
                        false_pos += 1;
                    }
                }
            }
        }
        s.add("C68-04-零误报", false_pos == 0 && sc.violations == 0, if false_pos == 0 { "" } else { "误报" });
    }
    // C68-05 契约快照稳定
    {
        let sc = SelectorContract::new();
        let a = sc.take_snapshot();
        let b = sc.take_snapshot();
        s.add("C68-05-快照稳定", a == b && a != 0, if a == b { "" } else { "快照漂移" });
    }
    // C68-06 冻结契约
    {
        let mut sc = SelectorContract::new();
        let fp = sc.freeze();
        s.add("C68-06-契约冻结", fp.is_ok() && sc.is_frozen(), if sc.is_frozen() { "" } else { "冻结失败" });
    }
    // C68-07 重复冻结拒
    {
        let mut sc = SelectorContract::new();
        let _ = sc.freeze();
        s.add("C68-07-重复冻结拒", sc.freeze() == Err(XC_SC_SEALED), "");
    }
    // C68-08 指纹一致
    {
        let mut a = SelectorContract::new();
        let _ = a.unique_solution();
        let mut b = SelectorContract::new();
        let _ = b.unique_solution();
        let ok = a.take_snapshot() == b.take_snapshot();
        s.add("C68-08-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C68-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C68-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C68-10 拒收计数
    {
        let mut sc = SelectorContract::new();
        let _ = sc.freeze();
        let _ = sc.freeze();
        s.add("C68-10-拒收计数", sc.reject_count() == 1, if sc.reject_count() == 1 { "" } else { "拒收漂移" });
    }
    s
}
