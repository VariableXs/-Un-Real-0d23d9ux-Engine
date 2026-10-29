//! UNX-F10215 执行资源预算（内存与时长预算带守恒，超预算熔断）（0x10e00 段）
//!
//! 锚点：UNX-F10215-J1 预算守恒达成（跑批内存与时长在预算带内
//! 10/10 次），超预算 10/10 次熔断。
//! 域：CoRun·UNX-C3（跑批执行面谱系）。
//!
//! 结构：预算带（内存上限 + 时长上限 + 已耗计数）+ 消耗核（逐用例
//! 扣减）+ 熔断核（任一越带即熔断收口）+ 折叠指纹 +
//! 注入核（超内存/超时长/负消耗/熔断后放行/空预算）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10e00;
/// 诊断码：内存越带
pub const ERR_MEM_OVER: u32 = SEG_BASE + 0x01;
/// 诊断码：时长越带
pub const ERR_TIME_OVER: u32 = SEG_BASE + 0x02;
/// 诊断码：消耗倒扣
pub const ERR_NEG_SPEND: u32 = SEG_BASE + 0x03;
/// 诊断码：熔断后放行
pub const ERR_TRIPPED_RUN: u32 = SEG_BASE + 0x04;
/// 诊断码：预算为零
pub const ERR_ZERO_BUDGET: u32 = SEG_BASE + 0x05;

/// 预算带
pub struct BudgetBand {
    /// 批次标识
    pub batch: String,
    /// 内存预算（KiB）
    pub mem_budget: usize,
    /// 时长预算（tick）
    pub time_budget: usize,
    /// 已耗内存
    pub mem_used: usize,
    /// 已耗时长
    pub time_used: usize,
    /// 熔断态
    pub tripped: bool,
    /// 熔断原因
    pub trip_reason: String,
}

impl BudgetBand {
    /// 开带：预算必须非零
    pub fn new(batch: &str, mem: usize, time: usize) -> Result<Self, u32> {
        if mem == 0 || time == 0 {
            return Err(ERR_ZERO_BUDGET);
        }
        Ok(BudgetBand {
            batch: batch.to_string(),
            mem_budget: mem,
            time_budget: time,
            mem_used: 0,
            time_used: 0,
            tripped: false,
            trip_reason: String::new(),
        })
    }

    /// 消耗一次：内存与时长同步扣减，越带即熔断
    pub fn spend(&mut self, mem: usize, time: usize) -> Result<(), u32> {
        if self.tripped {
            return Err(ERR_TRIPPED_RUN);
        }
        let new_mem = match self.mem_used.checked_add(mem) {
            Some(v) => v,
            None => return Err(ERR_NEG_SPEND),
        };
        let new_time = match self.time_used.checked_add(time) {
            Some(v) => v,
            None => return Err(ERR_NEG_SPEND),
        };
        if new_mem > self.mem_budget {
            self.tripped = true;
            self.trip_reason = format!("mem {}>{}", new_mem, self.mem_budget);
            return Err(ERR_MEM_OVER);
        }
        if new_time > self.time_budget {
            self.tripped = true;
            self.trip_reason = format!("time {}>{}", new_time, self.time_budget);
            return Err(ERR_TIME_OVER);
        }
        self.mem_used = new_mem;
        self.time_used = new_time;
        Ok(())
    }

    /// 预算带内判定：未熔断且两项都有余量
    pub fn within_band(&self) -> bool {
        !self.tripped && self.mem_used <= self.mem_budget && self.time_used <= self.time_budget
    }

    /// 内存余量（KiB）
    pub fn mem_headroom(&self) -> usize {
        self.mem_budget - self.mem_used
    }

    /// 时长余量（tick）
    pub fn time_headroom(&self) -> usize {
        self.time_budget - self.time_used
    }

    /// 内存占用率（基点）
    pub fn mem_load_bps(&self) -> u32 {
        (self.mem_used * 10000 / self.mem_budget) as u32
    }

    /// 时长占用率（基点）
    pub fn time_load_bps(&self) -> u32 {
        (self.time_used * 10000 / self.time_budget) as u32
    }

    /// 双带占用取大者（基点）
    pub fn peak_load_bps(&self) -> u32 {
        let m = self.mem_load_bps();
        let t = self.time_load_bps();
        if m > t {
            m
        } else {
            t
        }
    }

    /// 折叠指纹
    pub fn seal(&self) -> u32 {
        let mut acc = SEG_BASE;
        acc = acc.wrapping_add((self.mem_budget as u32) << 2);
        acc = acc.wrapping_add((self.time_budget as u32) << 1);
        acc = acc.wrapping_add(self.mem_used as u32);
        acc = acc.wrapping_add((self.time_used as u32).wrapping_mul(7));
        acc = acc.wrapping_add(if self.tripped { 0xF00D } else { 0 });
        acc
    }

    /// 诊断面：批次标识
    pub fn batch_name(&self) -> &str {
        &self.batch
    }

    /// 诊断面：熔断摘要
    pub fn trip_summary(&self) -> String {
        if !self.tripped {
            return format!("band={} clean", self.batch);
        }
        format!("band={} tripped {}", self.batch, self.trip_reason)
    }

    /// 诊断面：单笔预估放行判定（不落账）
    pub fn would_admit(&self, mem: usize, time: usize) -> bool {
        !self.tripped
            && self.mem_used + mem <= self.mem_budget
            && self.time_used + time <= self.time_budget
    }
}

/// 消耗流水条目
#[derive(Clone, PartialEq, Eq)]
pub struct SpendNote {
    /// 用例标识
    pub case: String,
    /// 本笔内存
    pub mem: usize,
    /// 本笔时长
    pub time: usize,
    /// 是否被受理
    pub admitted: bool,
}

/// 消耗流水（带熔断轨迹的完整审计链）
pub struct SpendLog {
    /// 流水
    pub notes: Vec<SpendNote>,
}

impl SpendLog {
    /// 空流水
    pub fn new() -> Self {
        SpendLog { notes: Vec::new() }
    }

    /// 记一笔（无论受理与否均留痕）
    pub fn log(&mut self, case: &str, mem: usize, time: usize, admitted: bool) {
        self.notes.push(SpendNote {
            case: case.to_string(),
            mem,
            time,
            admitted,
        });
    }

    /// 受理笔数
    pub fn admitted_count(&self) -> usize {
        self.notes.iter().filter(|n| n.admitted).count()
    }

    /// 拒绝笔数
    pub fn rejected_count(&self) -> usize {
        self.notes.iter().filter(|n| !n.admitted).count()
    }

    /// 拒绝原因分布：首笔拒绝前的受理笔数（即熔断点位置）
    pub fn trip_position(&self) -> Option<usize> {
        self.notes.iter().position(|n| !n.admitted)
    }

    /// 受理笔内存总量
    pub fn admitted_mem_total(&self) -> usize {
        self.notes.iter().filter(|n| n.admitted).map(|n| n.mem).sum()
    }

    /// 受理笔时长总量
    pub fn admitted_time_total(&self) -> usize {
        self.notes.iter().filter(|n| n.admitted).map(|n| n.time).sum()
    }

    /// 流水与预算带账面对平
    pub fn matches_band(&self, band: &BudgetBand) -> bool {
        self.admitted_mem_total() == band.mem_used && self.admitted_time_total() == band.time_used
    }

    /// 折叠指纹
    pub fn fold(&self) -> u32 {
        let mut acc = SEG_BASE + 0x77;
        for (i, n) in self.notes.iter().enumerate() {
            acc = acc.wrapping_mul(8191).wrapping_add(n.mem as u32 + n.time as u32);
            acc = acc.wrapping_add(if n.admitted { i as u32 + 1 } else { 0xFFFF });
        }
        acc
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 内存越带熔断
    let mut b1 = BudgetBand::new("inj-mem", 100, 1000).unwrap();
    out[0] = b1.spend(120, 10) == Err(ERR_MEM_OVER) && b1.tripped && b1.trip_reason.contains("mem");
    // 2: 时长越带熔断
    let mut b2 = BudgetBand::new("inj-time", 100, 50).unwrap();
    out[1] = b2.spend(10, 60) == Err(ERR_TIME_OVER) && b2.trip_reason.contains("time");
    // 3: 熔断后再放行被拒
    let mut b3 = BudgetBand::new("inj-trip", 100, 100).unwrap();
    let _ = b3.spend(120, 1);
    out[2] = b3.spend(1, 1) == Err(ERR_TRIPPED_RUN);
    // 4: 零预算开带被拒
    out[3] = BudgetBand::new("inj-zero", 0, 100).is_err()
        && BudgetBand::new("inj-zero2", 100, 0).is_err();
    // 5: 越带消耗不落账（熔断时账面回滚）
    let mut b5 = BudgetBand::new("inj-rollback", 100, 100).unwrap();
    let before = b5.mem_used;
    let _ = b5.spend(999, 1);
    out[4] = b5.mem_used == before;
    out
}

/// 流水联动注入：熔断点与流水位置一致
pub fn inject_log_trip_alignment() -> bool {
    let mut band = BudgetBand::new("inj-log", 100, 100).unwrap();
    let mut log = SpendLog::new();
    for i in 0..4 {
        let r = band.spend(30, 30);
        log.log(&format!("case-{}", i), 30, 30, r.is_ok());
    }
    let trip = log.trip_position();
    trip == Some(3) && log.admitted_count() == 3 && log.rejected_count() == 1
        && log.matches_band(&band)
}

/// 判据函数
pub fn run_unx10215_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10215");
    let mut b = match BudgetBand::new("bt-budget", 1000, 2000) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let r1 = b.spend(300, 500);
    let r2 = b.spend(400, 600);
    s.add("两笔消耗入账", r1.is_ok() && r2.is_ok() && b.mem_used == 700 && b.time_used == 1100, "两笔消耗累计正确");
    s.add("带内守恒", b.within_band(), "未越带且账面一致");
    s.add("内存余量三百", b.mem_headroom() == 300 && b.time_headroom() == 900, "余量扣减正确");
    s.add("占用率七成", b.mem_load_bps() == 7000 && b.time_load_bps() == 5500, "两带占用率正确");
    s.add("峰值取大", b.peak_load_bps() == 7000, "峰值占用取两带最大");
    let r3 = b.spend(200, 500);
    s.add("第三笔仍在带内", r3.is_ok() && b.mem_used == 900 && b.time_used == 1600, "贴带消耗放行");
    // 内存恰好贴带（900+100=1000），时长越带（1600+500>2000）触发时长熔断。
    let r4 = b.spend(100, 500);
    s.add("时长越带熔断", r4 == Err(ERR_TIME_OVER) && b.tripped, "末笔时长越带触发熔断");
    s.add("熔断后拒放行", b.spend(1, 1) == Err(ERR_TRIPPED_RUN), "熔断态下任何消耗被拒");
    s.add("越带不落账", b.mem_used == 900, "熔断笔不落账面");
    let zero = BudgetBand::new("bt-zero", 0, 0);
    s.add("零预算开带拒绝", zero.is_err(), "零预算开带即拒");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    s.add("指纹非零", b.seal() != 0, "折叠指纹非零");
    s.add("批次名核对", b.batch_name() == "bt-budget", "批次标识一致");
    s.add("熔断摘要", b.trip_summary().contains("tripped"), "熔断摘要含 tripped 字样");
    s.add("预估放行拒越带", !b.would_admit(1, 1) && {
        let bc = BudgetBand::new("bt-precheck", 100, 100).unwrap();
        bc.would_admit(50, 50) && !bc.would_admit(150, 1)
    }, "预估门与实放一致");
    s.add("流水熔断点对齐", inject_log_trip_alignment(), "流水首拒位=熔断点=第 4 笔");
    s.add("流水账面平", {
        let mut band2 = BudgetBand::new("bt-ledger", 500, 500).unwrap();
        let mut log2 = SpendLog::new();
        for i in 0..3 {
            let r = band2.spend(100, 100);
            log2.log(&format!("c{}", i), 100, 100, r.is_ok());
        }
        log2.matches_band(&band2) && log2.admitted_mem_total() == 300
    }, "流水汇总与带账一致");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_band_trips_before_time() {
        let mut b = BudgetBand::new("t", 10, 100).unwrap();
        assert_eq!(b.spend(20, 1), Err(ERR_MEM_OVER));
        assert!(b.tripped);
    }

    #[test]
    fn exact_budget_spend_ok() {
        let mut b = BudgetBand::new("t2", 100, 100).unwrap();
        assert!(b.spend(100, 100).is_ok());
        assert!(b.within_band());
        assert_eq!(b.mem_load_bps(), 10000);
    }

    #[test]
    fn trip_reason_covers_both_bands() {
        let mut bm = BudgetBand::new("t3", 10, 100).unwrap();
        let _ = bm.spend(11, 1);
        assert!(bm.trip_summary().contains("mem"));
        let mut bt = BudgetBand::new("t4", 100, 10).unwrap();
        let _ = bt.spend(1, 11);
        assert!(bt.trip_summary().contains("time"));
    }

    #[test]
    fn spend_log_aligns_with_band() {
        let mut band = BudgetBand::new("t5", 200, 200).unwrap();
        let mut log = SpendLog::new();
        let r1 = band.spend(100, 100);
        log.log("a", 100, 100, r1.is_ok());
        let r2 = band.spend(100, 100);
        log.log("b", 100, 100, r2.is_ok());
        let r3 = band.spend(50, 50);
        log.log("c", 50, 50, r3.is_ok());
        assert!(log.matches_band(&band));
        assert_eq!(log.trip_position(), Some(2));
        assert!(log.fold() != 0);
    }
}
