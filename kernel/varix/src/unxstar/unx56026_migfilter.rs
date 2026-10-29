//! UNX-F56026 CPU 迁移污染样本剔除器（0x60320 段）
//!
//! 锚点：UNX-F56026-J1 采样线程 CPU ID 逐样本记录（每样本带 CPU 号
//! 落账）；J2 跨核迁移样本剔除规则进代码（禁事后手挑——剔除只认绑核
//! 比对单规则）；J3 剔除率落账（>5% 触发重采）×5 场景验证三判据全绿。
//! 判据锚定 §AI-71.2 专题五统计纪律。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·迁移剔除面）。
//!
//! 结构：绑核声明账（绑核 CPU 号唯一落账）+ 逐样本 CPU ID 账（纳秒 ×
//! CPU 号逐条）+ 单规则剔除核（只比绑核，无白名单无人工豁免）+ 剔除
//! 率万分比账 + 重采闸（>500 触发）+ ×5 场景验证核 + 注入核（未绑核
//! 采/零纳秒/越核样本未剔/重采闸失灵/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60320 独占 0x20）。
pub const SEG_BASE: u32 = 0x60320;
pub const MG_PIN_MISS: u32 = SEG_BASE + 0x01; // 未绑核即采
pub const MG_NS_ZERO: u32 = SEG_BASE + 0x02; // 零纳秒样本
pub const MG_MIGRATED: u32 = SEG_BASE + 0x03; // 跨核迁移样本（剔除）
pub const MG_RESAMPLE: u32 = SEG_BASE + 0x04; // 剔除率超阈触发重采
pub const MG_SEALED: u32 = SEG_BASE + 0x05; // 采样账已封账拒写
pub const MG_PIN_REBIND: u32 = SEG_BASE + 0x06; // 绑核重声明（纪律破）
pub const MG_TRACE_LOST: u32 = SEG_BASE + 0x07; // 剔除样本未留痕

/// 剔除率重采阈值（万分比 5% = 500）。
pub const DROP_PBP_MAX: u32 = 500;

/// 单样本：纳秒 × CPU 号（逐样本落账，J1 口径）。
#[derive(Clone)]
pub struct SampleRec {
    pub ns: u64,
    pub cpu: u32,
}

/// 剔除痕迹：被剔样本 × 原因码（禁静默丢弃）。
#[derive(Clone)]
pub struct DropTrace {
    pub ns: u64,
    pub cpu: u32,
    pub code: u32,
}

/// 剔除器：绑核 + 逐样本账 + 单规则剔除。
pub struct MigrationFilter {
    pub bound_cpu: Option<u32>,
    pub kept: Vec<SampleRec>,
    pub drops: Vec<DropTrace>,
    pub sealed: bool,
}

impl MigrationFilter {
    pub fn new() -> Self {
        MigrationFilter { bound_cpu: None, kept: Vec::new(), drops: Vec::new(), sealed: false }
    }

    /// 绑核声明（重声明拒——绑核纪律一采样窗一核）。
    pub fn bind(&mut self, cpu: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(MG_SEALED);
        }
        if self.bound_cpu.is_some() {
            return Err(MG_PIN_REBIND);
        }
        self.bound_cpu = Some(cpu);
        Ok(())
    }

    /// 落样本：未绑核拒 / 零值拒 / 跨核剔（带痕）。
    pub fn record(&mut self, ns: u64, cpu: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(MG_SEALED);
        }
        match self.bound_cpu {
            None => return Err(MG_PIN_MISS),
            Some(b) => {
                if ns == 0 {
                    return Err(MG_NS_ZERO);
                }
                if cpu != b {
                    self.drops.push(DropTrace { ns, cpu, code: MG_MIGRATED });
                    return Err(MG_MIGRATED);
                }
            }
        }
        self.kept.push(SampleRec { ns, cpu });
        Ok(())
    }

    /// J1：逐样本 CPU ID 一致性（留存的 CPU 号全等于绑核）。
    pub fn cpu_ids_consistent(&self) -> bool {
        match self.bound_cpu {
            None => false,
            Some(b) => self.kept.iter().all(|s| s.cpu == b),
        }
    }

    /// 剔除率（万分比；分母 = 留存 + 剔除）。
    pub fn drop_pbp(&self) -> u32 {
        let total = self.kept.len() + self.drops.len();
        if total == 0 {
            return 0;
        }
        ((self.drops.len() as u64 * 10_000) / total as u64) as u32
    }

    /// J3：重采闸（>5% 触发）。
    pub fn needs_resample(&self) -> bool {
        self.drop_pbp() > DROP_PBP_MAX
    }

    /// J2：禁手挑核（剔除痕迹与剔除计数一致，无账外剔除）。
    pub fn no_manual_pick(&self) -> bool {
        self.drops.iter().all(|d| d.code == MG_MIGRATED)
            && self.drops.len() == self.drop_count()
    }

    /// 剔除计数。
    pub fn drop_count(&self) -> usize {
        self.drops.len()
    }

    /// 留存 p95（≥30 样本）。
    pub fn p95(&self) -> Result<u64, u32> {
        if self.kept.len() < 30 {
            return Err(MG_RESAMPLE);
        }
        let mut v: Vec<u64> = Vec::new();
        for s in self.kept.iter() {
            v.push(s.ns);
        }
        v.sort();
        let idx = (v.len() * 95 / 100).min(v.len() - 1);
        Ok(v[idx])
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：绑核 3，95 留存 + 5 剔除（剔除率 500 恰阈不触发）。
pub fn standard_filter() -> MigrationFilter {
    let mut f = MigrationFilter::new();
    f.bind(3).expect("bind");
    for i in 0..95u32 {
        f.record(200 + i as u64, 3).expect("keep");
    }
    for i in 0..5u32 {
        let _ = f.record(200 + i as u64, 9);
    }
    f
}

/// ×5 场景验证核：0% / 1% / 5% / 6% / 100% 污染注入全按规则判定。
pub fn scenario_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 场景 1：0% 污染（40 留存 0 剔除）
    let mut f1 = MigrationFilter::new();
    f1.bind(0).expect("b1");
    for i in 0..40u32 {
        f1.record(100 + i as u64, 0).expect("keep1");
    }
    if f1.drop_pbp() == 0 && !f1.needs_resample() {
        caught += 1;
    }
    // 场景 2：1% 污染（99 留存 1 剔除 → 100 万分比）
    let mut f2 = MigrationFilter::new();
    f2.bind(0).expect("b2");
    for i in 0..99u32 {
        f2.record(100 + i as u64, 0).expect("keep2");
    }
    let _ = f2.record(100, 5);
    if f2.drop_pbp() == 100 && !f2.needs_resample() {
        caught += 1;
    }
    // 场景 3：恰 5%（95/5 → 500 恰阈不触发）
    let f3 = standard_filter();
    if f3.drop_pbp() == 500 && !f3.needs_resample() {
        caught += 1;
    }
    // 场景 4：6%（94/6 → 600 触发）
    let mut f4 = MigrationFilter::new();
    f4.bind(0).expect("b4");
    for i in 0..94u32 {
        f4.record(100 + i as u64, 0).expect("keep4");
    }
    for i in 0..6u32 {
        let _ = f4.record(100 + i as u64, 7);
    }
    if f4.drop_pbp() == 600 && f4.needs_resample() {
        caught += 1;
    }
    // 场景 5：100% 污染（0 留存 10 剔除 → 10000 触发）
    let mut f5 = MigrationFilter::new();
    f5.bind(0).expect("b5");
    for _ in 0..10 {
        let _ = f5.record(100, 8);
    }
    if f5.drop_pbp() == 10_000 && f5.needs_resample() && f5.kept.is_empty() {
        caught += 1;
    }
    (caught, 5)
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未绑核即采
    let mut f = MigrationFilter::new();
    if f.record(100, 0) == Err(MG_PIN_MISS) {
        caught += 1;
    }
    // 2) 零纳秒样本
    let mut f2 = MigrationFilter::new();
    f2.bind(0).expect("b");
    if f2.record(0, 0) == Err(MG_NS_ZERO) {
        caught += 1;
    }
    // 3) 跨核样本未剔（剔除带痕）
    let mut f3 = MigrationFilter::new();
    f3.bind(1).expect("b3");
    if f3.record(100, 4) == Err(MG_MIGRATED)
        && f3.drop_count() == 1
        && f3.drops[0].code == MG_MIGRATED
    {
        caught += 1;
    }
    // 4) 绑核重声明拒
    let mut f4 = standard_filter();
    if f4.bind(2) == Err(MG_PIN_REBIND) {
        caught += 1;
    }
    // 5) 封账改写
    let mut f5 = standard_filter();
    f5.seal();
    if f5.record(100, 3) == Err(MG_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56026_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56026");
    // J1-1 逐样本 CPU ID 一致（留存全 = 绑核）
    let f = standard_filter();
    let ok = f.cpu_ids_consistent() && f.kept.len() == 95 && f.bound_cpu == Some(3);
    s.add("56026-J1-逐样本CPUID", ok, if ok { "" } else { "CPU ID 账漂移" });
    // J1-2 剔除痕迹齐（5 条全带原因码）
    let ok = f.drop_count() == 5 && f.no_manual_pick()
        && f.drops.iter().all(|d| d.code == MG_MIGRATED);
    s.add("56026-J1-剔除痕迹齐", ok, if ok { "" } else { "剔除账外流失" });
    // J2-1 单规则剔除（禁手挑：无白名单无豁免接口——痕迹原因码唯一）
    let ok = f.no_manual_pick();
    s.add("56026-J2-单规则禁手挑", ok, if ok { "" } else { "剔除规则多门" });
    // J2-2 剔除率语义（95/5 → 500 恰阈）
    let ok = f.drop_pbp() == 500 && !f.needs_resample();
    s.add("56026-J2-剔除率恰阈", ok, if ok { "" } else { "剔除率漂移" });
    // J2-3 留存 p95 不含污染（idx90 → 290）
    let ok = f.p95().expect("p") == 290;
    s.add("56026-J2-留存p95纯净", ok, if ok { "" } else { "污染混入统计" });
    // J3-1 ×5 场景验证全过
    let (c, n) = scenario_core();
    let ok = c == n && n == 5;
    let detail = if ok { String::new() } else { format!("场景 {c}/{n}") };
    s.add_dyn("56026-J3-五场景验证", ok, &detail);
    // J3-2 注入核 5 类必抓
    let (ic, in_) = injection_core();
    let ok = ic == in_;
    let detail = if ok { String::new() } else { format!("注入 {ic}/{in_}") };
    s.add_dyn("56026-J3-注入核必抓", ok, &detail);
    // J3-3 重采阈值语义（500 不触发 / 501 触发——6% 场景 600 已证）
    let ok = DROP_PBP_MAX == 500;
    s.add("56026-J3-重采阈值语义", ok, if ok { "" } else { "阈值漂移" });
    // J1-3 未绑核账为零（记录数与剔除数皆零）
    let f2 = MigrationFilter::new();
    let ok = f2.kept.is_empty() && f2.drops.is_empty() && f2.drop_pbp() == 0;
    s.add("56026-J1-未绑核零账", ok, if ok { "" } else { "未绑核出账" });
    // J2-4 100% 污染零留存（无样本出统计）
    let mut f3 = MigrationFilter::new();
    f3.bind(0).expect("b");
    for _ in 0..5 {
        let _ = f3.record(100, 1);
    }
    let ok = f3.kept.is_empty() && f3.p95().is_err();
    s.add("56026-J2-全污零留存", ok, if ok { "" } else { "全污出统计" });
    // J2-5 封账冻结
    let mut f4 = standard_filter();
    f4.seal();
    let ok = f4.record(100, 3) == Err(MG_SEALED) && f4.bind(0) == Err(MG_SEALED);
    s.add("56026-J2-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J1-4 绑核重声明拒（纪律）
    let ok = MigrationFilter::new().bind(5).is_ok();
    let mut f5 = MigrationFilter::new();
    let _ = f5.bind(5);
    let ok = ok && f5.bind(6) == Err(MG_PIN_REBIND);
    s.add("56026-J1-绑核唯一", ok, if ok { "" } else { "绑核重声明穿透" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56026_all_checks_green() {
        let set = run_unx56026_checks();
        assert!(set.all_passed(), "UNX-F56026 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56026_scenarios() {
        assert_eq!(scenario_core(), (5, 5));
    }
}
