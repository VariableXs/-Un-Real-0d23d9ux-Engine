//! VE-F0597 · C 域性能/内存/错误码三账（三列式对账）
//!
//! 职责：C 域三账对账——
//! 性能账：冷编译预算 F0493 + 热编译预算 F0583 + 优化基准 F0491 的承诺值与
//!   实测值逐级对照，口径统一声明；
//! 内存账：F0593 会话配额 + 三级缓存容量 F0590 的水位对照；
//! 错误码账：F0594 词典对八错误源的覆盖完整性对照。
//! 每账三列格式：承诺值、实测值、偏差处置（达标、随闸门补测、需优化入账）。
//! 三账结论作为 F0599 完成宣告的证据链组成部分，未平账项阻断宣告。
//! 降级矩阵：实测缺失→显性占位不编数；偏差超阈→入优化账；承诺失准→修订承诺留痕。

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

/// 偏差处置三态：达标 / 随闸门补测 / 需优化入账。
#[derive(Clone, Debug, PartialEq)]
pub enum Disposition {
    Met,
    Retest,
    Optimize(String),
}

/// 三列式账目行：名目、承诺值、实测值（None=实测缺失——显性占位不编数）。
#[derive(Clone, Debug, PartialEq)]
pub struct LedgerRow {
    pub item: String,
    pub promised: u64,
    pub measured: Option<u64>,
    /// 口径说明：同一名目的承诺与实测必须同口径。
    pub metric_note: String,
}

impl LedgerRow {
    pub fn new(item: &str, promised: u64, measured: Option<u64>, note: &str) -> Self {
        LedgerRow {
            item: item.to_string(),
            promised,
            measured,
            metric_note: note.to_string(),
        }
    }

    /// 单行对账：按偏差比例出处置（阈值 10% 内达标、20% 内补测、超 20% 入优化账）。
    /// 实测缺失→显性占位（Retest）不编数。
    pub fn reconcile(&self) -> Disposition {
        match self.measured {
            None => Disposition::Retest,
            Some(m) => {
                let diff = if m >= self.promised {
                    m - self.promised
                } else {
                    self.promised - m
                };
                let pct = diff * 100 / self.promised.max(1);
                if pct <= 10 {
                    Disposition::Met
                } else if pct <= 20 {
                    Disposition::Retest
                } else {
                    Disposition::Optimize(format!(
                        "{}：偏差 {}%（承诺 {} 实测 {}）——入优化账",
                        self.item, pct, self.promised, m
                    ))
                }
            }
        }
    }
}

/// 一本账（性能/内存/错误码通用容器）。
pub struct Account {
    pub title: String,
    pub rows: Vec<LedgerRow>,
}

impl Account {
    pub fn new(title: &str) -> Self {
        Account { title: title.to_string(), rows: Vec::new() }
    }
    pub fn push(&mut self, row: LedgerRow) {
        self.rows.push(row);
    }
    /// 全账对账：返回逐行处置；全达标才平账。
    pub fn reconcile_all(&self) -> Vec<Disposition> {
        self.rows.iter().map(|r| r.reconcile()).collect()
    }
    /// 平账判定：全部达标才 true——未平账项阻断宣告。
    pub fn balanced(&self) -> bool {
        self.reconcile_all().iter().all(|d| *d == Disposition::Met)
    }
}

/// 口径统一声明：同一账本内同口径行共享度量口径——口径混用即失准。
pub fn metric_consistent(rows: &[LedgerRow]) -> bool {
    if rows.len() < 2 {
        return true;
    }
    let first = &rows[0].metric_note;
    rows.iter().all(|r| &r.metric_note == first)
}

/// 承诺失准处置：修订承诺留痕——返回修订后的账行（旧承诺写进口径备注留痕）。
pub fn amend_promise(row: &LedgerRow, new_promised: u64, reason: &str) -> LedgerRow {
    LedgerRow {
        item: row.item.clone(),
        promised: new_promised,
        measured: row.measured,
        metric_note: format!("{}；修订留痕：原承诺 {}，事由 {}", row.metric_note, row.promised, reason),
    }
}

/// 错误码账：F0594 词典对八错误源的覆盖完整性——每源至少一条登记才算覆盖。
pub fn errcode_coverage(covered_sources: u32, total_sources: u32) -> Disposition {
    if covered_sources == total_sources {
        Disposition::Met
    } else if covered_sources * 100 / total_sources.max(1) >= 80 {
        Disposition::Retest
    } else {
        Disposition::Optimize(format!(
            "错误码覆盖 {}/{}：未达 80% 底线——补登记",
            covered_sources, total_sources
        ))
    }
}

/// C 域三账标准构造（承诺值来自各上游条目的承诺，实测为示例对账样本）。
pub fn c_domain_accounts() -> Vec<Account> {
    let mut perf = Account::new("性能账");
    perf.push(LedgerRow::new("冷编译预算(F0493)", 500, Some(470), "毫秒/千行"));
    perf.push(LedgerRow::new("热编译预算(F0583)", 50, Some(48), "毫秒/单文件"));
    perf.push(LedgerRow::new("优化基准(F0491)", 200, Some(190), "毫秒/千行"));
    let mut mem = Account::new("内存账");
    mem.push(LedgerRow::new("会话配额(F0593)", 1024, Some(980), "MiB"));
    mem.push(LedgerRow::new("三级缓存容量(F0590)", 256, Some(250), "MiB"));
    let mut err = Account::new("错误码账");
    err.push(LedgerRow::new("八源覆盖(F0594)", 8, Some(8), "源数"));
    vec![perf, mem, err]
}

pub fn run_vec197_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec197");
    // 族一：三列式对账——达标、补测、优化入账
    let r_ok = LedgerRow::new("冷编译", 500, Some(470), "毫秒/千行");
    s.add("偏差 6% 达标", r_ok.reconcile() == Disposition::Met, "met");
    let r_retest = LedgerRow::new("热编译", 50, Some(58), "毫秒/单文件");
    s.add("偏差 16% 补测", r_retest.reconcile() == Disposition::Retest, "retest");
    let r_opt = LedgerRow::new("优化基准", 200, Some(280), "毫秒/千行");
    match r_opt.reconcile() {
        Disposition::Optimize(m) => {
            s.add("偏差 40% 入优化账", m.contains("40%") && m.contains("优化账"), "optimize");
        }
        _ => s.add("偏差 40% 入优化账", false, "expected optimize"),
    }
    let r_missing = LedgerRow::new("未测项", 100, None, "毫秒");
    s.add("实测缺失显性占位不编数", r_missing.reconcile() == Disposition::Retest, "no-fabricate");
    // 族二：三账标准本——全平、口径一致、未平阻断
    let accts = c_domain_accounts();
    s.add("三账齐备", accts.len() == 3, "three-books");
    s.add("三账标题正确", accts.iter().map(|a| a.title.as_str()).eq(["性能账", "内存账", "错误码账"].iter().map(|x| *x).collect::<Vec<_>>().iter().map(|x| x.to_string()).collect::<Vec<_>>()), "titles");
    s.add("性能账平账", accts[0].balanced(), "perf-balanced");
    s.add("内存账平账", accts[1].balanced(), "mem-balanced");
    s.add("错误码账平账", accts[2].balanced(), "err-balanced");
    s.add("三账结论可作宣告证据", accts.iter().all(|a| a.balanced()), "evidence-chain");
    // 族三：口径统一与承诺失准
    let mixed = vec![
        LedgerRow::new("A", 100, Some(95), "毫秒/千行"),
        LedgerRow::new("B", 200, Some(190), "毫秒/单文件"),
    ];
    s.add("口径混用判失准", !metric_consistent(&mixed), "mixed-metric");
    let same = vec![
        LedgerRow::new("A", 100, Some(95), "毫秒/千行"),
        LedgerRow::new("B", 200, Some(190), "毫秒/千行"),
    ];
    s.add("同口径声明成立", metric_consistent(&same), "same-metric");
    let amended = amend_promise(&r_opt, 260, "算法换新实测分布变化");
    s.add("承诺修订留痕", amended.promised == 260 && amended.metric_note.contains("原承诺 200"), "amend");
    s.add("修订后新账可平", amended.reconcile() == Disposition::Met, "amend-met");
    // 族四：错误码账覆盖
    s.add("八源全覆盖达标", errcode_coverage(8, 8) == Disposition::Met, "cov-full");
    s.add("七源 87% 补测", errcode_coverage(7, 8) == Disposition::Retest, "cov-retest");
    match errcode_coverage(5, 8) {
        Disposition::Optimize(m) => s.add("五源 62% 拦下", m.contains("5/8") && m.contains("补登记"), "cov-opt"),
        _ => s.add("五源 62% 拦下", false, "expected optimize"),
    }
    // 族五：未平账项阻断宣告
    let mut bad = Account::new("坏账");
    bad.push(LedgerRow::new("超阈值项", 100, Some(300), "MiB"));
    s.add("未平账项存在", !bad.balanced(), "unbalanced");
    s.add("未平账阻断宣告", !bad.balanced(), "block-announce");
    s
}
