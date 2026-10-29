//! UNX-F10229 豁免清偿冲刺（豁免逐项裁决留痕，空转复核检出）（0x10fc0 段）
//!
//! 锚点：UNX-F10229-J1 清偿冲刺达成（豁免逐项裁决留痕
//! 10/10 次），空转复核 10/10 次检出。
//! 域：CoRun·UNX-C3（归因面谱系）。
//!
//! 结构：清偿账（豁免项 + 逐项裁决 + 清偿态）+ 裁决核（三项结论：
//! 补测清偿/永久豁免/维持挂账）+ 空转检出核（复核未变裁决计空转）
//! + 折叠指纹 + 注入核（空转复核/幽灵项/重裁决/封账/越类裁决）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10fc0;
/// 诊断码：空转复核
pub const ERR_IDLE_REVIEW: u32 = SEG_BASE + 0x01;
/// 诊断码：幽灵项
pub const ERR_GHOST_ITEM: u32 = SEG_BASE + 0x02;
/// 诊断码：重复裁决
pub const ERR_DUP_VERDICT: u32 = SEG_BASE + 0x03;
/// 诊断码：封账操作
pub const ERR_FROZEN: u32 = SEG_BASE + 0x04;
/// 诊断码：越类裁决
pub const ERR_BAD_VERDICT: u32 = SEG_BASE + 0x05;

/// 清偿裁决
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Settlement {
    /// 补测清偿（已补测翻绿）
    Compensated,
    /// 永久豁免
    Permanent,
    /// 维持挂账
    KeepPending,
}

impl Settlement {
    /// 裁决名
    pub fn name(&self) -> &'static str {
        match self {
            Settlement::Compensated => "COMPENSATED",
            Settlement::Permanent => "PERMANENT",
            Settlement::KeepPending => "KEEP_PENDING",
        }
    }
    /// 是否终态
    pub fn terminal(&self) -> bool {
        matches!(self, Settlement::Compensated | Settlement::Permanent)
    }
}

/// 豁免条目
#[derive(Clone, PartialEq, Eq)]
pub struct ExemptRow {
    /// 豁免项
    pub item: String,
    /// 最近裁决
    pub verdict: Settlement,
    /// 裁决轮次
    pub rounds: usize,
}

impl ExemptRow {
    /// 条目自洽
    pub fn ok(&self) -> bool {
        !self.item.is_empty() && self.rounds >= 1
    }
}

/// 清偿账
pub struct SettlementBook {
    /// 豁免条目
    pub rows: Vec<ExemptRow>,
    /// 空转计数
    pub idle_count: usize,
    /// 封账
    pub frozen: bool,
}

impl SettlementBook {
    /// 空账
    pub fn new() -> Self {
        SettlementBook { rows: Vec::new(), idle_count: 0, frozen: false }
    }

    /// 登记豁免项（初始维持挂账）
    pub fn register(&mut self, item: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if item.is_empty() {
            return Err(ERR_GHOST_ITEM);
        }
        if self.rows.iter().any(|r| r.item == item) {
            return Err(ERR_DUP_VERDICT);
        }
        self.rows.push(ExemptRow {
            item: item.to_string(),
            verdict: Settlement::KeepPending,
            rounds: 1,
        });
        Ok(())
    }

    /// 逐项裁决：复核后改判；未变裁决计空转
    pub fn review(&mut self, item: &str, v: Settlement) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        let pos = match self.rows.iter().position(|r| r.item == item) {
            Some(p) => p,
            None => return Err(ERR_GHOST_ITEM),
        };
        if self.rows[pos].verdict == v {
            self.idle_count += 1;
            return Err(ERR_IDLE_REVIEW);
        }
        // 终态后不得改判
        if self.rows[pos].verdict.terminal() {
            return Err(ERR_DUP_VERDICT);
        }
        self.rows[pos].verdict = v;
        self.rows[pos].rounds += 1;
        Ok(())
    }

    /// 清偿率（基点，终态项/总项）
    pub fn settled_ratio_bps(&self) -> u32 {
        if self.rows.is_empty() {
            return 0;
        }
        let done = self.rows.iter().filter(|r| r.verdict.terminal()).count();
        (done * 10000 / self.rows.len()) as u32
    }

    /// 空转率（基点）
    pub fn idle_ratio_bps(&self) -> u32 {
        let total = self.total_reviews();
        if total == 0 {
            return 0;
        }
        (self.idle_count * 10000 / total) as u32
    }

    /// 总复核轮次
    pub fn total_reviews(&self) -> usize {
        self.rows.iter().map(|r| r.rounds).sum()
    }

    /// 维持挂账清单
    pub fn pending_list(&self) -> Vec<String> {
        self.rows.iter().filter(|r| r.verdict == Settlement::KeepPending).map(|r| r.item.clone()).collect()
    }

    /// 诊断面：终态清单（已清偿+永久）
    pub fn terminal_list(&self) -> Vec<String> {
        self.rows.iter().filter(|r| r.verdict.terminal()).map(|r| r.item.clone()).collect()
    }

    /// 诊断面：某项裁决回读
    pub fn verdict_of(&self, item: &str) -> Option<Settlement> {
        self.rows.iter().find(|r| r.item == item).map(|r| r.verdict)
    }

    /// 诊断面：某项轮次回读
    pub fn rounds_of(&self, item: &str) -> Option<usize> {
        self.rows.iter().find(|r| r.item == item).map(|r| r.rounds)
    }

    /// 诊断面：按裁决计数
    pub fn count_of_verdict(&self, v: Settlement) -> usize {
        self.rows.iter().filter(|r| r.verdict == v).count()
    }

    /// 诊断面：清偿均值轮次（基点，轮次×10000/条数）
    pub fn avg_rounds_bps(&self) -> u32 {
        if self.rows.is_empty() {
            return 0;
        }
        let sum: usize = self.rows.iter().map(|r| r.rounds).sum();
        (sum * 10000 / self.rows.len()) as u32
    }

    /// 诊断面：账面摘要
    pub fn book_summary(&self) -> String {
        format!(
            "rows={} settled_bps={} idle={}",
            self.rows.len(),
            self.settled_ratio_bps(),
            self.idle_count
        )
    }

    /// 诊断面：全部条目自洽复核
    pub fn all_ok(&self) -> bool {
        self.rows.iter().all(|r| r.ok())
    }

    /// 诊断面：冲刺完成判定（无空转且全终态）
    pub fn sprint_complete(&self) -> bool {
        self.idle_count == 0 && !self.rows.is_empty() && self.settled_ratio_bps() == 10000
    }

    /// 折叠指纹
    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        let mut acc = SEG_BASE;
        for (i, r) in self.rows.iter().enumerate() {
            let v = match r.verdict {
                Settlement::Compensated => 3,
                Settlement::Permanent => 5,
                Settlement::KeepPending => 7,
            };
            acc = acc.wrapping_mul(5889).wrapping_add(v + (r.rounds as u32) + i as u32);
        }
        acc = acc.wrapping_add(self.idle_count as u32);
        acc
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 空转复核检出（同裁决复核）
    let mut b1 = SettlementBook::new();
    let _ = b1.register("x");
    let _ = b1.review("x", Settlement::KeepPending);
    out[0] = b1.idle_count == 1 && b1.review("x", Settlement::KeepPending) == Err(ERR_IDLE_REVIEW);
    // 2: 幽灵项拒绝
    let mut b2 = SettlementBook::new();
    out[1] = b2.review("ghost", Settlement::Compensated) == Err(ERR_GHOST_ITEM);
    // 3: 终态后改判拒绝
    let mut b3 = SettlementBook::new();
    let _ = b3.register("y");
    let _ = b3.review("y", Settlement::Permanent);
    out[2] = b3.review("y", Settlement::Compensated) == Err(ERR_DUP_VERDICT);
    // 4: 封账拒绝
    let mut b4 = SettlementBook::new();
    let _ = b4.register("z");
    let _ = b4.seal();
    out[3] = b4.review("z", Settlement::KeepPending) == Err(ERR_FROZEN)
        && b4.register("w") == Err(ERR_FROZEN);
    // 5: 重登记拒绝
    let mut b5 = SettlementBook::new();
    let _ = b5.register("dup");
    out[4] = b5.register("dup") == Err(ERR_DUP_VERDICT);
    out
}

/// 判据函数
pub fn run_unx10229_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10229");
    let mut b = SettlementBook::new();
    let r1 = b.register("exm-pty-wide");
    let r2 = b.register("exm-hpet-miss");
    let r3 = b.register("exm-net-flaky");
    s.add("三豁免入账", r1.is_ok() && r2.is_ok() && r3.is_ok(), "三条豁免项登记");
    s.add("条目自洽", b.rows.iter().all(|r| r.ok()), "各条目字段自洽");
    s.add("初态维持挂账", b.pending_list().len() == 3, "初态三项全挂账");
    let v1 = b.review("exm-pty-wide", Settlement::Compensated);
    let v2 = b.review("exm-hpet-miss", Settlement::Permanent);
    s.add("两项裁决落痕", v1.is_ok() && v2.is_ok(), "补测清偿与永久豁免各一");
    s.add("清偿率三分之二", b.settled_ratio_bps() == 6666, "2/3 终态 6666 基点");
    s.add("维持清单余一", b.pending_list() == vec!["exm-net-flaky".to_string()], "挂账清单余一项");
    s.add("空转检出", b.review("exm-pty-wide", Settlement::Compensated) == Err(ERR_IDLE_REVIEW), "同裁决复核计空转");
    s.add("空转计数", b.idle_count == 1 && b.idle_ratio_bps() > 0, "空转一轮留痕");
    s.add("裁决名核", Settlement::Compensated.name() == "COMPENSATED" && Settlement::Permanent.terminal(), "裁决名与终态核正确");
    s.add("幽灵项拒绝", b.review("ghost", Settlement::KeepPending) == Err(ERR_GHOST_ITEM), "未登记项裁决被拒");
    s.add("终态清单", b.terminal_list() == vec!["exm-pty-wide".to_string(), "exm-hpet-miss".to_string()], "两项终态入清单");
    s.add("裁决回读", b.verdict_of("exm-hpet-miss") == Some(Settlement::Permanent) && b.verdict_of("ghost").is_none(), "在账可回读裁决、缺席为 None");
    s.add("轮次回读", b.rounds_of("exm-pty-wide") == Some(2) && b.rounds_of("exm-net-flaky") == Some(1), "裁决轮次留痕");
    s.add("按裁决计数", b.count_of_verdict(Settlement::KeepPending) == 1 && b.count_of_verdict(Settlement::Permanent) == 1, "两类裁决各一");
    s.add("均轮基点", b.avg_rounds_bps() == 16666, "总轮 5/条 3 = 16666 基点");
    s.add("账面摘要", b.book_summary().contains("rows=3") && b.book_summary().contains("idle=1"), "摘要含条目数与空转数");
    s.add("全条目自洽", b.all_ok(), "全条目复核通过");
    s.add("冲刺未完", !b.sprint_complete(), "有空转且有挂账故冲刺未完");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let fp = b.seal();
    s.add("封账拒绝", b.review("exm-net-flaky", Settlement::Compensated) == Err(ERR_FROZEN), "封账后裁决被拒");
    s.add("封账指纹非零", fp != 0, "封账折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settle_then_terminal_lock() {
        let mut b = SettlementBook::new();
        let _ = b.register("t");
        let _ = b.review("t", Settlement::Compensated);
        assert!(Settlement::Compensated.terminal());
        assert_eq!(b.review("t", Settlement::Permanent), Err(ERR_DUP_VERDICT));
        assert_eq!(b.settled_ratio_bps(), 10000);
    }

    #[test]
    fn idle_ratio_counts() {
        let mut b = SettlementBook::new();
        let _ = b.register("a");
        let _ = b.register("bb");
        let _ = b.review("a", Settlement::KeepPending); // 空转
        let _ = b.review("bb", Settlement::Permanent); // 实裁
        // 总轮次 1+1=2，空转 1 → 5000 基点
        assert_eq!(b.idle_ratio_bps(), 5000);
        assert_eq!(b.pending_list(), vec!["a".to_string()]);
    }

    #[test]
    fn sprint_complete_when_all_settled() {
        let mut b = SettlementBook::new();
        let _ = b.register("a");
        let _ = b.register("b");
        let _ = b.review("a", Settlement::Compensated);
        let _ = b.review("b", Settlement::Permanent);
        assert!(b.sprint_complete());
        assert!(b.terminal_list().len() == 2);
        assert!(b.pending_list().is_empty());
        assert_eq!(b.idle_count, 0);
    }

    #[test]
    fn ghost_and_roundtrip() {
        let mut b = SettlementBook::new();
        assert_eq!(b.review("ghost", Settlement::KeepPending), Err(ERR_GHOST_ITEM));
        let _ = b.register("g");
        assert_eq!(b.verdict_of("g"), Some(Settlement::KeepPending));
        assert_eq!(b.rounds_of("g"), Some(1));
        assert!(b.all_ok());
    }
}
