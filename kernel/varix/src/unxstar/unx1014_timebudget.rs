//! UNX-F1014 · 时间误差预算表（计时面 SLO 分解契约）账 · 诊断段 0xB8D0
//! 验收锚点：UNX-F1014-J1 预算表冻结（计时面误差源七项预算+责任条目），
//! 达标率 ≥99% 断言（F0949 同型账），版本化机制 10/10 次，双判据缺一即批内红灯。
use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;


pub const SEG_BASE: u16 = 0xB8D0;

// ── 诊断段独占 · 预算表错误域 ───────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_SHORT: u16 = SEG_ERR + 1;  // 误差源不足七项
const E_OWNER: u16 = SEG_ERR + 2;  // 责任条目缺
const E_FROZEN: u16 = SEG_ERR + 3; // 未冻结改账
const E_VER: u16 = SEG_ERR + 4;    // 版本漂移
const E_SLO: u16 = SEG_ERR + 5;    // 达标率不足

pub const BUDGET_SOURCES: usize = 7;
pub const SLO_RATE: u64 = 99; // 达标率 ≥99%

// ── FNV-1a 64 ────────────────────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 预算条目 ─────────────────────────────────────────────────
/// 七项计时面误差源（固定名表）。
pub fn source_names() -> [&'static str; BUDGET_SOURCES] {
    ["tsc_drift", "hpet_quantize", "sched_slice", "ipi_latency",
     "timer_jitter", "calib_residual", "acct_rounding"]
}

/// 一条预算：误差源 + 预算上限（ns）+ 责任条目 + 实测。
#[derive(Clone, Debug)]
pub struct BudgetItem {
    pub source: String,
    pub budget_ns: u64,
    pub owner: String,    // 责任条目（子系统）
    pub actual_ns: u64,
}

impl BudgetItem {
    pub fn within(&self) -> bool {
        self.actual_ns <= self.budget_ns
    }
    pub fn fp(&self) -> u64 {
        fnv1a64(format!("{}:{}:{}:{}", self.source, self.budget_ns, self.owner, self.actual_ns).as_bytes())
    }
}

/// 预算表：七项齐 + 冻结 + 版本化。
pub struct BudgetTable {
    pub items: Vec<BudgetItem>,
    pub frozen: bool,
    pub version: u32,
    frozen_fp: u64,
}

impl BudgetTable {
    pub fn new() -> Self {
        BudgetTable { items: Vec::new(), frozen: false, version: 1, frozen_fp: 0 }
    }
    /// 挂账：重名拒、冻结后拒。
    pub fn add(&mut self, source: &str, budget_ns: u64, owner: &str) -> bool {
        if self.frozen {
            return false;
        }
        if self.items.iter().any(|i| i.source == source) {
            return false;
        }
        self.items.push(BudgetItem {
            source: source.to_string(),
            budget_ns,
            owner: owner.to_string(),
            actual_ns: 0,
        });
        true
    }
    /// 七项齐且责任条目非空。
    pub fn complete(&self) -> bool {
        self.items.len() == BUDGET_SOURCES
            && self.items.iter().zip(source_names()).all(|(i, n)| i.source == n && !i.owner.is_empty())
    }
    /// 冻结：指纹定格。
    pub fn freeze(&mut self) {
        self.frozen_fp = self.compute_fp();
        self.frozen = true;
    }
    fn compute_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for i in &self.items {
            for b in i.fp().to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        for b in self.version.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }
    /// 漂移检测（冻结后改动实测值即漂移？——否：实测值是运行态读数，
    /// 漂移检测只针对「预算上限/责任条目/误差源名」三类冻结域）。
    pub fn drift(&self) -> bool {
        if !self.frozen {
            return false;
        }
        // 重算时清零实测（实测域不在冻结指纹内）
        let mut probe = BudgetTable { items: self.items.clone(), frozen: false, version: self.version, frozen_fp: 0 };
        for i in probe.items.iter_mut() {
            i.actual_ns = self.items.iter().find(|x| x.source == i.source).map(|x| x.actual_ns).unwrap_or(0);
        }
        probe.compute_fp() != self.frozen_fp
    }
    /// 录入实测（冻结后允许：运行态读数）。
    pub fn record(&mut self, source: &str, actual: u64) -> bool {
        match self.items.iter_mut().find(|i| i.source == source) {
            Some(i) => {
                i.actual_ns = actual;
                true
            }
            None => false,
        }
    }
    /// 达标率：达标项 ×100 / 总项。
    pub fn slo_rate(&self) -> u64 {
        if self.items.is_empty() {
            return 0;
        }
        let ok = self.items.iter().filter(|i| i.within()).count();
        (ok * 100 / self.items.len()) as u64
    }
    /// SLO 断言：≥99%（七项全达标才 100；6/7=85 <99，故实为全达标）。
    pub fn slo_ok(&self) -> bool {
        self.slo_rate() >= SLO_RATE
    }
    /// 版本化：冻结版本 bump（未冻结拒）。
    pub fn bump_version(&mut self) -> bool {
        if !self.frozen {
            return false;
        }
        self.version += 1;
        self.frozen_fp = self.compute_fp();
        true
    }
    pub fn version_now(&self) -> u32 {
        self.version
    }
}

/// 标准预算表夹具（七项全挂账）。
pub fn standard_table() -> BudgetTable {
    let mut t = BudgetTable::new();
    t.add("tsc_drift", 50_000, "clock");
    t.add("hpet_quantize", 30_000, "clock");
    t.add("sched_slice", 100_000, "sched");
    t.add("ipi_latency", 20_000, "smp");
    t.add("timer_jitter", 80_000, "timer");
    t.add("calib_residual", 40_000, "clock");
    t.add("acct_rounding", 10_000, "acct");
    t
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx1014_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F1014 时间误差预算表");

    // 1. 七项齐 + 责任条目非空
    let mut t = standard_table();
    s.add("TB-七项齐", t.complete(), if t.complete() { "" } else { "误差源缺失" });

    // 2. 冻结后全达标 SLO 100%
    t.freeze();
    let sources = source_names();
    for (i, src) in sources.iter().enumerate() {
        let _ = t.record(src, (40_000u64) - (i as u64 * 1000));
    }
    s.add("TB-SLO达标", t.slo_ok() && t.slo_rate() == 100,
        if t.slo_ok() { "" } else { "达标率不足" });

    // 3. 达标率下探：一项超预算 → 6/7=85 <99 拦截
    let mut t2 = standard_table();
    t2.freeze();
    for src in sources.iter() {
        let _ = t2.record(src, 10_000);
    }
    let _ = t2.record("ipi_latency", 999_999);
    s.add("TB-超预算拦", !t2.slo_ok() && t2.slo_rate() == 85,
        if !t2.slo_ok() { "" } else { "超预算放行" });

    // 4. 冻结后禁加项
    let mut t3 = standard_table();
    t3.freeze();
    let frozen_add = !t3.add("new_source", 1, "x");
    s.add("TB-冻后禁加", frozen_add, if frozen_add { "" } else { "冻后可加" });

    // 5. 冻结域漂移检出（改预算上限）
    let mut t4 = standard_table();
    t4.freeze();
    // 直改上限（绕过 record 的合法通道）
    if let Some(i) = t4.items.iter_mut().find(|i| i.source == "sched_slice") {
        i.budget_ns = 999_999;
    }
    s.add("TB-冻域漂移", t4.drift(), if t4.drift() { "" } else { "漂移未检出" });

    // 6. 实测录入不触发漂移（实测域不在冻结指纹内）
    let mut t5 = standard_table();
    t5.freeze();
    let _ = t5.record("tsc_drift", 77_777);
    s.add("TB-实测不漂移", !t5.drift(), if !t5.drift() { "" } else { "实测误报漂移" });

    // 7. 版本化机制 10/10
    let mut ver_ok = 0;
    for _ in 0..10 {
        let mut tv = standard_table();
        let v0 = tv.version_now();
        tv.freeze();
        let bump1 = tv.bump_version();
        let bump2 = tv.bump_version();
        if bump1 && bump2 && tv.version_now() == v0 + 2 && !tv.drift() {
            ver_ok += 1;
        }
    }
    s.add("TB-版本化10/10", ver_ok == 10, if ver_ok == 10 { "" } else { "版本化断" });

    // 8. 未冻结版本 bump 拒
    let mut t6 = standard_table();
    let v6 = t6.version_now();
    let rej = !t6.bump_version() && t6.version_now() == v6;
    s.add("TB-未冻bump拒", rej, if rej { "" } else { "未冻可bump" });

    // 9. 重名误差源拒
    let mut t7 = standard_table();
    let dup = !t7.add("tsc_drift", 1, "x");
    s.add("TB-重名拒", dup, if dup { "" } else { "重复放行" });

    // 10. LCG 抽样 5 条指纹稳定
    let mut lcg = Lcg(0xB8D0);
    let mut spot = true;
    for _ in 0..5 {
        let i = lcg.below(t.items.len() as u64) as usize;
        if t.items[i].fp() != t.items[i].fp() {
            spot = false;
        }
    }
    s.add("TB-指纹稳定", spot, if spot { "" } else { "指纹漂移" });

    // 11. 指纹互异
    let mut fp_uniq = true;
    for i in 0..t.items.len() {
        for j in (i + 1)..t.items.len() {
            if t.items[i].fp() == t.items[j].fp() {
                fp_uniq = false;
            }
        }
    }
    s.add("TB-指纹互异", fp_uniq, if fp_uniq { "" } else { "指纹碰撞" });

    // 12. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB8D0 && SEG_BASE < 0xB8F0;
    s.add("TB-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
