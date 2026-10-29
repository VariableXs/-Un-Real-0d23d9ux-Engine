//! UNX-F10208 双档执行标记（模拟/真机两档分离可统计，混档检出）（0x10d20 段）
//!
//! 锚点：UNX-F10208-J1 双档分账达成（模拟/真机两档标记分离可统计
//! 10/10 次），混档 10/10 次检出。
//! 域：CoRun·UNX-C3（跑批执行面谱系）。
//!
//! 结构：档位枚举（Sim/Real 双档 raw 编解码）+ 双档账本（登记/
//! 重复/换档拒绝）+ 混档检出核（外部清单比对）+ 分账统计面 +
//! 折叠指纹 + 注入核（换档/未知档/空壳/重复/封后登记）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10d20;
/// 诊断码：用例标识为空
pub const ERR_EMPTY_CASE: u32 = SEG_BASE + 0x01;
/// 诊断码：重复登记
pub const ERR_DUP_CASE: u32 = SEG_BASE + 0x02;
/// 诊断码：混档检出
pub const ERR_TIER_MIX: u32 = SEG_BASE + 0x03;
/// 诊断码：档位未知
pub const ERR_BAD_TIER: u32 = SEG_BASE + 0x04;
/// 诊断码：账册封禁
pub const ERR_SEALED: u32 = SEG_BASE + 0x05;

/// 双档上限（合法 raw 值）
const TIER_SIM: u8 = 1;
const TIER_REAL: u8 = 2;

/// 执行档位
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 仿真档
    Sim,
    /// 实机档
    Real,
}

impl Tier {
    /// 从 raw 解码；越界返回 None
    pub fn from_raw(v: u8) -> Option<Tier> {
        match v {
            TIER_SIM => Some(Tier::Sim),
            TIER_REAL => Some(Tier::Real),
            _ => None,
        }
    }
    /// 编码为 raw
    pub fn raw(&self) -> u8 {
        match self {
            Tier::Sim => TIER_SIM,
            Tier::Real => TIER_REAL,
        }
    }
    /// 档位名称
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Sim => "SIM",
            Tier::Real => "REAL",
        }
    }
}

/// 双档登记条目
#[derive(Clone, PartialEq, Eq)]
pub struct TierEntry {
    /// 用例标识
    pub case: String,
    /// 执行档位
    pub tier: Tier,
    /// 是否通过
    pub passed: bool,
}

impl TierEntry {
    /// 条目自洽
    pub fn ok(&self) -> bool {
        !self.case.is_empty()
    }
}

/// 双档账本
pub struct TierBook {
    /// 登记条目
    pub entries: Vec<TierEntry>,
    /// 封账标记
    pub sealed: bool,
}

impl TierBook {
    /// 开册
    pub fn new() -> Self {
        TierBook { entries: Vec::new(), sealed: false }
    }

    /// 登记一条执行档位；混档立即拒绝
    pub fn enroll(&mut self, case: &str, raw_tier: u8, passed: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(ERR_SEALED);
        }
        if case.is_empty() {
            return Err(ERR_EMPTY_CASE);
        }
        let tier = match Tier::from_raw(raw_tier) {
            Some(t) => t,
            None => return Err(ERR_BAD_TIER),
        };
        if let Some(prev) = self.entries.iter().find(|e| e.case == case) {
            if prev.tier != tier {
                return Err(ERR_TIER_MIX);
            }
            return Err(ERR_DUP_CASE);
        }
        self.entries.push(TierEntry { case: case.to_string(), tier, passed });
        Ok(())
    }

    /// 混档检出：同一用例两档并存视为违规（扫描外部清单）
    pub fn detect_mix_in(&self, raw_pairs: &[(&str, u8)]) -> bool {
        for (case, raw) in raw_pairs {
            let t = match Tier::from_raw(*raw) {
                Some(t) => t,
                None => continue,
            };
            if let Some(e) = self.entries.iter().find(|e| &e.case == case) {
                if e.tier != t {
                    return true;
                }
            }
        }
        false
    }

    /// 某档通过率（基点）
    pub fn pass_ratio_bps(&self, tier: Tier) -> u32 {
        let matched: Vec<&TierEntry> = self.entries.iter().filter(|e| e.tier == tier).collect();
        if matched.is_empty() {
            return 0;
        }
        let pass = matched.iter().filter(|e| e.passed).count();
        (pass * 10000 / matched.len()) as u32
    }

    /// 双档分账是否完整（两档均有记录）
    pub fn both_tiers_present(&self) -> bool {
        self.entries.iter().any(|e| e.tier == Tier::Sim)
            && self.entries.iter().any(|e| e.tier == Tier::Real)
    }

    /// 封账
    pub fn seal(&mut self) -> u32 {
        let mut acc = 0u32;
        for (i, e) in self.entries.iter().enumerate() {
            acc = acc.wrapping_mul(6271).wrapping_add(e.tier.raw() as u32);
            acc = acc.wrapping_add((e.case.len() as u32) << (i % 5));
            acc = acc.wrapping_add(if e.passed { 3 } else { 7 });
        }
        acc
    }

    /// 诊断面：档位分布（Sim 计数, Real 计数）
    pub fn tier_split(&self) -> (usize, usize) {
        let sim = self.entries.iter().filter(|e| e.tier == Tier::Sim).count();
        let real = self.entries.iter().filter(|e| e.tier == Tier::Real).count();
        (sim, real)
    }

    /// 诊断面：某档条目名清单
    pub fn cases_of_tier(&self, tier: Tier) -> Vec<String> {
        self.entries.iter().filter(|e| e.tier == tier).map(|e| e.case.clone()).collect()
    }

    /// 诊断面：某档通过计数
    pub fn passed_count_of(&self, tier: Tier) -> usize {
        self.entries.iter().filter(|e| e.tier == tier && e.passed).count()
    }

    /// 诊断面：最长用例名
    pub fn longest_case_name(&self) -> String {
        let mut best = String::new();
        for e in &self.entries {
            if e.case.len() > best.len() {
                best = e.case.clone();
            }
        }
        best
    }

    /// 诊断面：全册失败清单
    pub fn failed_cases(&self) -> Vec<String> {
        self.entries.iter().filter(|e| !e.passed).map(|e| e.case.clone()).collect()
    }

    /// 诊断面：某用例档位查询
    pub fn tier_of(&self, case: &str) -> Option<Tier> {
        self.entries.iter().find(|e| e.case == case).map(|e| e.tier)
    }

    /// 诊断面：登记总数
    pub fn total_enrolled(&self) -> usize {
        self.entries.len()
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 同用例先 Sim 后 Real → 混档拒绝
    let mut b1 = TierBook::new();
    let _ = b1.enroll("mix-case", TIER_SIM, true);
    out[0] = b1.enroll("mix-case", TIER_REAL, true) == Err(ERR_TIER_MIX);
    // 2: 未知档位 raw=7 → 拒绝
    let mut b2 = TierBook::new();
    out[1] = b2.enroll("ghost", 7, true) == Err(ERR_BAD_TIER);
    // 3: 空用例 → 拒绝
    let mut b3 = TierBook::new();
    out[2] = b3.enroll("", TIER_SIM, true) == Err(ERR_EMPTY_CASE);
    // 4: 重复登记（同档）→ 拒绝
    let mut b4 = TierBook::new();
    let _ = b4.enroll("dup", TIER_REAL, false);
    out[3] = b4.enroll("dup", TIER_REAL, true) == Err(ERR_DUP_CASE);
    // 5: 封账后登记 → 拒绝
    let mut b5 = TierBook::new();
    b5.sealed = true;
    out[4] = b5.enroll("late", TIER_SIM, true) == Err(ERR_SEALED);
    out
}

/// 判据函数
pub fn run_unx10208_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10208");
    let mut b = TierBook::new();
    let e1 = b.enroll("ut-sim-a", TIER_SIM, true);
    let e2 = b.enroll("ut-sim-b", TIER_SIM, false);
    let e3 = b.enroll("ut-real-a", TIER_REAL, true);
    s.add("三登记成功", e1.is_ok() && e2.is_ok() && e3.is_ok(), "三条档位登记全部受理");
    s.add("双档齐备", b.both_tiers_present(), "Sim 与 Real 两档均有条目");
    s.add("档位解码往返", Tier::from_raw(TIER_REAL) == Some(Tier::Real) && Tier::Real.raw() == TIER_REAL, "raw 编解码往返一致");
    s.add("档名标注", Tier::Sim.name() == "SIM" && Tier::Real.name() == "REAL", "档位名称正确");
    s.add("混档外部检出", b.detect_mix_in(&[("ut-sim-a", TIER_REAL), ("ut-real-a", TIER_REAL)]), "外部清单中混档被检出");
    s.add("无混档不误报", !b.detect_mix_in(&[("ut-sim-a", TIER_SIM), ("ghost-x", TIER_REAL)]), "同档与未知用例不触发");
    s.add("混档登记拒绝", b.enroll("ut-sim-a", TIER_REAL, false) == Err(ERR_TIER_MIX), "同用例换档登记被拒");
    let (sim_n, real_n) = b.tier_split();
    s.add("分账计数", sim_n == 2 && real_n == 1, "两档计数正确");
    s.add("Sim 通过率半数", b.pass_ratio_bps(Tier::Sim) == 5000, "Sim 档通过率 50%");
    s.add("Real 全过", b.pass_ratio_bps(Tier::Real) == 10000 && b.passed_count_of(Tier::Real) == 1, "Real 档通过率 100%");
    s.add("Sim 清单核对", b.cases_of_tier(Tier::Sim).len() == 2 && b.cases_of_tier(Tier::Real).len() == 1, "分档清单计数正确");
    s.add("最长名命中", b.longest_case_name() == "ut-real-a", "最长用例名为 ut-real-a");
    s.add("失败清单含 Sim-B", b.failed_cases() == vec!["ut-sim-b".to_string()], "失败清单仅 ut-sim-b");
    s.add("条目自洽", b.entries.iter().all(|e| e.ok()), "各条目字段自洽");
    s.add("档位查询", b.tier_of("ut-sim-a") == Some(Tier::Sim) && b.tier_of("nope").is_none(), "在册可查档位、缺席为 None");
    s.add("登记总数三", b.total_enrolled() == 3, "登记总数 3");
    s.add("Real 清单唯一", b.cases_of_tier(Tier::Real) == vec!["ut-real-a".to_string()], "Real 分档清单仅一条");
    s.add("未知档不误检", !b.detect_mix_in(&[("new-case", 7)]), "未知档 raw 不触发混档判定");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let seal_v = b.seal();
    s.add("封账指纹非零", seal_v != 0, "封账折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_roundtrip_and_mix() {
        assert_eq!(Tier::from_raw(1), Some(Tier::Sim));
        assert_eq!(Tier::from_raw(2), Some(Tier::Real));
        assert_eq!(Tier::from_raw(0), None);
        let mut b = TierBook::new();
        let _ = b.enroll("c", TIER_SIM, true);
        assert_eq!(b.enroll("c", TIER_REAL, true), Err(ERR_TIER_MIX));
    }

    #[test]
    fn both_tiers_split_count() {
        let mut b = TierBook::new();
        let _ = b.enroll("a", TIER_SIM, true);
        let _ = b.enroll("b", TIER_REAL, false);
        assert!(b.both_tiers_present());
        assert_eq!(b.tier_split(), (1, 1));
    }

    #[test]
    fn tier_name_and_failed_list() {
        let mut b = TierBook::new();
        let _ = b.enroll("x1", TIER_SIM, false);
        let _ = b.enroll("x2", TIER_REAL, false);
        let fails = b.failed_cases();
        assert_eq!(fails.len(), 2);
        assert!(fails.contains(&"x1".to_string()));
        assert_eq!(Tier::Real.name(), "REAL");
    }

    #[test]
    fn empty_tier_ratio_zero() {
        let b = TierBook::new();
        assert_eq!(b.pass_ratio_bps(Tier::Sim), 0);
        assert!(!b.both_tiers_present());
    }

    #[test]
    fn unknown_tier_never_mixes() {
        let mut b = TierBook::new();
        let _ = b.enroll("k", TIER_SIM, true);
        assert!(!b.detect_mix_in(&[("k", 0), ("k", 9)]));
        assert_eq!(b.tier_of("k"), Some(Tier::Sim));
        assert_eq!(b.total_enrolled(), 1);
    }

    #[test]
    fn seal_fingerprint_stable() {
        let mut g = TierBook::new();
        let _ = g.enroll("t1", TIER_SIM, true);
        let _ = g.enroll("t2", TIER_REAL, true);
        let _ = g.enroll("t3", TIER_SIM, false);
        let a = g.seal();
        let b = g.seal();
        assert_eq!(a, b);
    }

    #[test]
    fn sealed_book_rejects_enroll() {
        let mut b = TierBook::new();
        let _ = b.enroll("a", TIER_SIM, true);
        let _ = b.seal();
        assert_eq!(b.enroll("b", TIER_REAL, true), Err(ERR_SEALED));
    }

}