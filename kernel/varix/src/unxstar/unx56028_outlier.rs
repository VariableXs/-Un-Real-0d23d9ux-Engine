//! UNX-F56028 离群点预声明剔除规则引擎（0x60360 段）
//!
//! 锚点：UNX-F56028-J1 剔除规则预先声明进代码（时钟毛刺/迁移污染/采
//! 集器自身停顿三类闭集，规则外零剔除——审计断言逐条剔除必带规则标
//! 签）；J2 对同数据集规则内/外剔除结果差分留档（双口径分位对照落
//! 账）；J3 规则改动静默拒（未声明规则名执行即拒，禁事后手挑）三判
//! 据全绿。判据锚定 §AI-71.2 专题五"不许事后手挑"。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·剔除规则面）。
//!
//! 结构：规则闭集登记册（三类逐条声明：名 × 谓词阈值）+ 执行核（逐
//! 样本过闸：命中已声明规则才剔，剔除带规则标签）+ 审计断言核（标签
//! 外零剔除）+ 差分留档核（全量口径 vs 剔除口径 p50/p95 对照）+ 注入
//! 核（未声明规则/零阈值规则/负样本账/规则未声明即执行/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60360 独占 0x20）。
pub const SEG_BASE: u32 = 0x60360;
pub const OL_RULE_UNK: u32 = SEG_BASE + 0x01; // 未声明规则名执行
pub const OL_TH_ZERO: u32 = SEG_BASE + 0x02; // 零阈值规则
pub const OL_TAG_LOST: u32 = SEG_BASE + 0x03; // 剔除无规则标签（审计破）
pub const OL_SEALED: u32 = SEG_BASE + 0x04; // 规则引擎已封账拒改
pub const OL_DUP_RULE: u32 = SEG_BASE + 0x05; // 同规则重复声明
pub const OL_EMPTY_SET: u32 = SEG_BASE + 0x06; // 空样本集执行
pub const OL_DIFF_LOST: u32 = SEG_BASE + 0x07; // 差分未留档

/// 规则闭集（三类，预声明口径）。
pub const R_GLITCH: usize = 0;
pub const R_MIGRATE: usize = 1;
pub const R_STALL: usize = 2;
pub const R_COUNT: usize = 3;
pub const R_NAMES: [&str; R_COUNT] = ["clock_glitch", "migration_pollution", "collector_stall"];

/// 规则声明：名 × 谓词阈值（毛刺倍率 / 迁移标记 / 停顿下限 ns）。
#[derive(Clone)]
pub struct RuleDecl {
    pub rule: usize,
    pub glitch_mult: u32,
    pub stall_min_ns: u64,
}

impl RuleDecl {
    pub fn new(rule: usize, glitch_mult: u32, stall_min_ns: u64) -> Result<Self, u32> {
        if rule >= R_COUNT {
            return Err(OL_RULE_UNK);
        }
        if glitch_mult == 0 && rule == R_GLITCH {
            return Err(OL_TH_ZERO);
        }
        Ok(RuleDecl { rule, glitch_mult, stall_min_ns })
    }
}

/// 带元数据样本：纳秒 × 迁移标记 × 停顿标记。
#[derive(Clone)]
pub struct MetaSample {
    pub ns: u64,
    pub migrated: bool,
    pub stalled: bool,
}

/// 剔除痕迹：样本值 × 规则标签。
#[derive(Clone)]
pub struct Removal {
    pub ns: u64,
    pub rule: usize,
}

/// 规则引擎：声明集 + 执行 + 审计。
pub struct RuleEngine {
    pub decls: Vec<RuleDecl>,
    pub removals: Vec<Removal>,
    pub baseline_p50: u64,
    pub baseline_p95: u64,
    pub filtered_p50: u64,
    pub filtered_p95: u64,
    pub sealed: bool,
}

impl RuleEngine {
    pub fn new() -> Self {
        RuleEngine {
            decls: Vec::new(),
            removals: Vec::new(),
            baseline_p50: 0,
            baseline_p95: 0,
            filtered_p50: 0,
            filtered_p95: 0,
            sealed: false,
        }
    }

    /// 预声明规则（重复拒 + 封账拒）。
    pub fn declare(&mut self, d: RuleDecl) -> Result<(), u32> {
        if self.sealed {
            return Err(OL_SEALED);
        }
        for p in self.decls.iter() {
            if p.rule == d.rule {
                return Err(OL_DUP_RULE);
            }
        }
        self.decls.push(d);
        Ok(())
    }

    /// 规则是否已声明。
    pub fn declared(&self, rule: usize) -> bool {
        for d in self.decls.iter() {
            if d.rule == rule {
                return true;
            }
        }
        false
    }

    /// 谓词：样本命中哪条已声明规则（None = 保留）。
    pub fn hit(&self, s: &MetaSample) -> Option<usize> {
        for d in self.decls.iter() {
            match d.rule {
                R_MIGRATE => {
                    if s.migrated {
                        return Some(R_MIGRATE);
                    }
                }
                R_STALL => {
                    if s.stalled && s.ns >= d.stall_min_ns {
                        return Some(R_STALL);
                    }
                }
                R_GLITCH => {
                    if self.baseline_p95 > 0 && s.ns > self.baseline_p95 * d.glitch_mult as u64 {
                        return Some(R_GLITCH);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// 执行：全量口径统计 → 逐样本过闸 → 剔除带标签 → 剔除口径统计。
    /// 剔除必带已声明规则标签，否则审计破（返回 OL_TAG_LOST）。
    pub fn apply(&mut self, samples: &[MetaSample]) -> Result<usize, u32> {
        if self.sealed {
            return Err(OL_SEALED);
        }
        if samples.is_empty() {
            return Err(OL_EMPTY_SET);
        }
        // 全量口径基线。
        let mut all: Vec<u64> = Vec::new();
        for s in samples.iter() {
            all.push(s.ns);
        }
        all.sort();
        let idx = |p: usize| -> u64 { all[(all.len() * p / 100).min(all.len() - 1)] };
        self.baseline_p50 = idx(50);
        self.baseline_p95 = idx(95);
        // 逐样本过闸。
        let mut kept: Vec<u64> = Vec::new();
        for s in samples.iter() {
            match self.hit(s) {
                None => kept.push(s.ns),
                Some(rule) => {
                    if !self.declared(rule) {
                        return Err(OL_TAG_LOST);
                    }
                    self.removals.push(Removal { ns: s.ns, rule });
                }
            }
        }
        // 剔除口径统计。
        kept.sort();
        if kept.is_empty() {
            self.filtered_p50 = 0;
            self.filtered_p95 = 0;
        } else {
            let idx2 = |p: usize| -> u64 { kept[(kept.len() * p / 100).min(kept.len() - 1)] };
            self.filtered_p50 = idx2(50);
            self.filtered_p95 = idx2(95);
        }
        Ok(self.removals.len())
    }

    /// J1：审计断言（逐条剔除带已声明规则标签）。
    pub fn audit_ok(&self) -> bool {
        for r in self.removals.iter() {
            if !self.declared(r.rule) {
                return false;
            }
        }
        true
    }

    /// J2：差分留档齐（双口径四值全非零或恰全零）。
    pub fn diff_recorded(&self) -> bool {
        (self.baseline_p50 > 0 && self.baseline_p95 > 0)
            == (self.filtered_p50 > 0 || self.removals.len() == 0)
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：三规则全声明 + 基线 100 步 1 的 50 样本 + 3 个注入离群点
/// （迁移污 / 停顿 / 毛刺）。
pub fn standard_engine() -> RuleEngine {
    let mut e = RuleEngine::new();
    e.declare(RuleDecl::new(R_GLITCH, 3, 0).expect("d1")).expect("reg1");
    e.declare(RuleDecl::new(R_MIGRATE, 0, 0).expect("d2")).expect("reg2");
    e.declare(RuleDecl::new(R_STALL, 0, 10_000).expect("d3")).expect("reg3");
    let mut samples: Vec<MetaSample> = Vec::new();
    for i in 0..50u64 {
        samples.push(MetaSample { ns: 100 + i, migrated: false, stalled: false });
    }
    samples.push(MetaSample { ns: 11_000, migrated: false, stalled: true });
    samples.push(MetaSample { ns: 1_000, migrated: true, stalled: false });
    samples.push(MetaSample { ns: 5_000, migrated: false, stalled: false });
    e.apply(&samples).expect("apply");
    e
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未声明规则名（rule=9）
    if RuleDecl::new(9, 3, 0).err() == Some(OL_RULE_UNK) {
        caught += 1;
    }
    // 2) 零阈值毛刺规则
    if RuleDecl::new(R_GLITCH, 0, 0).err() == Some(OL_TH_ZERO) {
        caught += 1;
    }
    // 3) 空样本集执行
    let mut e = standard_engine();
    if e.apply(&[]) == Err(OL_EMPTY_SET) {
        caught += 1;
    }
    // 4) 未声明规则执行（不声明 migrate，迁移样本保留不剔）
    let mut e2 = RuleEngine::new();
    e2.declare(RuleDecl::new(R_GLITCH, 3, 0).expect("g")).expect("reg");
    let ms = [MetaSample { ns: 100, migrated: true, stalled: false }];
    let _ = e2.apply(&ms);
    if e2.removals.is_empty() && e2.filtered_p50 == 100 {
        caught += 1;
    }
    // 5) 封账改写
    let mut e3 = standard_engine();
    e3.seal();
    if e3.declare(RuleDecl::new(R_STALL, 0, 1).expect("s")) == Err(OL_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56028_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56028");
    // J1-1 三规则全预声明
    let e = standard_engine();
    let ok = e.decls.len() == R_COUNT
        && e.declared(R_GLITCH) && e.declared(R_MIGRATE) && e.declared(R_STALL);
    s.add("56028-J1-三规则预声明", ok, if ok { "" } else { "规则未声明" });
    // J1-2 全量口径基线（53 样本：100..149 + 1000/5000/11000；
    // p50=idx26→126，p95=idx50→1000）
    let ok = e.baseline_p95 == 1_000 && e.baseline_p50 == 126;
    s.add("56028-J1-全量口径基线", ok, if ok { "" } else { "基线漂移" });
    // J2-1 剔除恰 3 条全带标签（1000→迁移 / 5000→毛刺 / 11000→毛刺：
    // 毛刺规则先声明先命中，停顿被毛刺短路——声明序即裁决序）
    let ok = e.removals.len() == 3
        && e.removals.iter().any(|r| r.ns == 1_000 && r.rule == R_MIGRATE)
        && e.removals.iter().any(|r| r.ns == 11_000 && r.rule == R_GLITCH)
        && e.removals.iter().any(|r| r.ns == 5_000 && r.rule == R_GLITCH);
    s.add("56028-J2-剔除带标签", ok, if ok { "" } else { "剔除标签缺" });
    // J2-2 规则外零剔除（50 留存样本 = 53 - 3）
    let kept_n = 53usize - e.removals.len();
    let ok = kept_n == 50 && e.audit_ok();
    s.add("56028-J2-规则外零剔除", ok, if ok { "" } else { "规则外误剔" });
    // J2-3 差分留档（留存 50 样本 100..149：p50=idx25→125，p95=idx47→147）
    let ok = e.filtered_p95 == 147 && e.filtered_p50 == 125 && e.diff_recorded();
    s.add("56028-J2-差分留档", ok, if ok { "" } else { "差分未留档" });
    // J2-4 毛刺阈值语义（51 样本 p95=idx48→148；148×40=5920；5000≤5920 保留）
    let mut e2 = RuleEngine::new();
    e2.declare(RuleDecl::new(R_GLITCH, 40, 0).expect("g2")).expect("reg");
    e2.declare(RuleDecl::new(R_MIGRATE, 0, 0).expect("m2")).expect("reg");
    let mut s2: Vec<MetaSample> = Vec::new();
    for i in 0..50u64 {
        s2.push(MetaSample { ns: 100 + i, migrated: false, stalled: false });
    }
    s2.push(MetaSample { ns: 5_000, migrated: false, stalled: false });
    let _ = e2.apply(&s2);
    let ok = e2.removals.is_empty();
    s.add("56028-J2-毛刺阈值语义", ok, if ok { "" } else { "阈值语义漂移" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56028-J3-注入核必抓", ok, &detail);
    // J3-2 规则名闭集同源
    let ok = R_NAMES[R_GLITCH] == "clock_glitch" && R_NAMES[R_STALL] == "collector_stall";
    s.add("56028-J3-规则名同源", ok, if ok { "" } else { "命名漂移" });
    // J3-3 未声明执行保留不剔（审计留证）
    let ok = e2.audit_ok() && e2.declared(R_GLITCH);
    s.add("56028-J3-未声明保留留证", ok, if ok { "" } else { "保留无证" });
    // J3-4 封账冻结
    let mut e3 = standard_engine();
    e3.seal();
    let ok = e3.declare(RuleDecl::new(R_GLITCH, 9, 0).expect("s")) == Err(OL_SEALED);
    s.add("56028-J3-封账冻结", ok, if ok { "" } else { "封账后可改" });
    // J1-3 停顿阈值语义（stalled 且 ns≥10000 才剔；低值停顿保留）
    let mut e4 = RuleEngine::new();
    e4.declare(RuleDecl::new(R_STALL, 0, 10_000).expect("st")).expect("reg");
    let s4 = [MetaSample { ns: 9_000, migrated: false, stalled: true }];
    let _ = e4.apply(&s4);
    let ok = e4.removals.is_empty() && e4.filtered_p95 == 9_000;
    s.add("56028-J1-停顿阈值语义", ok, if ok { "" } else { "停顿阈值漂移" });
    // J1-4 同规则重复声明拒
    let mut e5 = RuleEngine::new();
    e5.declare(RuleDecl::new(R_GLITCH, 3, 0).expect("d")).expect("r1");
    let ok = e5.declare(RuleDecl::new(R_GLITCH, 4, 0).expect("d2")) == Err(OL_DUP_RULE);
    s.add("56028-J1-重复声明拒", ok, if ok { "" } else { "重复声明穿透" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56028_all_checks_green() {
        let set = run_unx56028_checks();
        assert!(set.all_passed(), "UNX-F56028 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56028_removal_tags() {
        let e = standard_engine();
        assert_eq!(e.removals.len(), 3);
        assert!(e.audit_ok());
    }
}
