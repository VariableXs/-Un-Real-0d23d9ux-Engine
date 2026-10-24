//! VE-F0496 · 优化诊断记录（VE-C 域 · 优化组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0496`
//!
//! **锚点原文**：「优化诊断记录（优化器体验日志）：每 pass 执行记录（pass
//! 名、输入输出指令数、时长、消除与变换计数）、优化决策事件（重要变换的
//! what 与 why 摘要）、质量事件（对拍失败、确定性异常入账）；记录供 F0491
//! 基准与总日志中心消费。数据结构：pass 记录流；决策事件；质量事件账。
//! 错误路径与降级矩阵：记录写入失败→暂存补写（不阻塞编译）；计数异常→
//! 如实记录不修数；事件风暴→合并限频。判据：pass 记录、决策摘要、质量入
//! 账、异步补写、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **pass 记录**（判据一）。每 pass 一条执行记录（名/入出指令数/时长/
//!    消除与变换计数）——流式追加。
//!
//! 2. **决策摘要**（判据二）。重要变换的 what + why 双字段摘要入账（缺
//!    why → 摘要不完整标注）。
//!
//! 3. **质量入账**（判据三）。对拍失败、确定性异常 → 质量事件账（消费方
//!    F0491/F0499）；计数异常如实记录不修数。
//!
//! 4. **异步补写**（判据四）。写入失败 → 暂存补写（不阻塞编译）；事件风
//!    暴 → 同键合并限频。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0496";
/// 事件风暴合并阈值。
pub const STORM_THRESHOLD: usize = 5;

/// 一条 pass 执行记录。
#[derive(Clone, Debug, PartialEq)]
pub struct PassRec {
    pub pass: &'static str,
    pub ins_in: u32,
    pub ins_out: u32,
    /// 时长（微秒）。
    pub dur_us: u64,
    /// 消除计数。
    pub eliminated: u32,
    /// 变换计数。
    pub transformed: u32,
}

/// 决策事件（what + why）。
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub pass: &'static str,
    /// what：做了什么变换。
    pub what: &'static str,
    /// why：为什么（None = 摘要不完整）。
    pub why: Option<&'static str>,
}

/// 诊断记录账。
#[derive(Debug, Default)]
pub struct RecBook {
    pub recs: Vec<PassRec>,
    pub decisions: Vec<Decision>,
    /// 质量事件账。
    pub quality: Vec<String>,
    /// 写失败暂存区（补写队列）。
    pub pending: Vec<PassRec>,
    /// 风暴合并账：同键 → 合并计数。
    pub storm: Vec<(String, usize)>,
    /// 异常计数原样面（如实记录不修数）。
    pub odd_counters: Vec<String>,
}

impl RecBook {
    /// pass 记录写入（直接成功路径）。
    pub fn write(&mut self, r: PassRec) -> Result<(), &'static str> {
        self.recs.push(r);
        Ok(())
    }

    /// 写入失败模拟 → 暂存补写（不阻塞编译）。
    pub fn write_fallback(&mut self, r: PassRec) {
        self.pending.push(r);
    }

    /// 补写：暂存区全部搬入主账。
    pub fn flush(&mut self) -> usize {
        let n = self.pending.len();
        self.recs.append(&mut self.pending);
        n
    }

    /// 决策摘要入账。
    pub fn decide(&mut self, d: Decision) {
        self.decisions.push(d);
    }

    /// 摘要完整性：缺 why 的决策计数。
    pub fn incomplete_why(&self) -> usize {
        self.decisions.iter().filter(|d| d.why.is_none()).count()
    }

    /// 质量事件入账（对拍失败 / 确定性异常）。
    pub fn quality_event(&mut self, kind: &'static str, detail: &str) {
        self.quality.push(format!("质量事件[{}]：{}", kind, detail));
    }

    /// 计数异常如实记录（不修数）。
    pub fn odd_counter(&mut self, pass: &str, elim: u32, ins_out: u32) {
        if elim > ins_out {
            self.odd_counters
                .push(format!("计数异常如实记录：{} 消除 {} > 出指令 {}（不修数）", pass, elim, ins_out));
        }
    }

    /// 事件风暴合并限频：同键累计计数，达阈值后可见。
    pub fn storm_event(&mut self, key: &str) {
        match self.storm.iter_mut().find(|(k, _)| k == key) {
            Some((_, n)) => *n += 1,
            None => self.storm.push((key.to_string(), 1)),
        }
    }

    /// 风暴呈现：只呈现达到阈值的键。
    pub fn storm_visible(&self) -> Vec<String> {
        self.storm
            .iter()
            .filter(|(_, n)| *n >= STORM_THRESHOLD)
            .map(|(k, n)| format!("{} ×{}", k, n))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 判据（F0496-xxx）
// ---------------------------------------------------------------------------

/// 原始判据。
pub fn vec96_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、pass 记录。
    {
        let mut book = RecBook::default();
        let _ = book.write(PassRec { pass: "dce", ins_in: 100, ins_out: 80, dur_us: 12, eliminated: 20, transformed: 0 });
        let _ = book.write(PassRec { pass: "fold", ins_in: 80, ins_out: 75, dur_us: 9, eliminated: 0, transformed: 5 });
        v.push((
            "F0496-记录-流式追加",
            book.recs.len() == 2 && book.recs[0].pass == "dce" && book.recs[1].eliminated == 0,
        ));
    }

    // 二、异步补写。
    {
        let mut book = RecBook::default();
        let _ = book.write(PassRec { pass: "a", ins_in: 1, ins_out: 1, dur_us: 1, eliminated: 0, transformed: 0 });
        book.write_fallback(PassRec { pass: "b", ins_in: 2, ins_out: 2, dur_us: 2, eliminated: 0, transformed: 0 });
        book.write_fallback(PassRec { pass: "c", ins_in: 3, ins_out: 3, dur_us: 3, eliminated: 0, transformed: 0 });
        v.push((
            "F0496-补写-失败暂存不阻塞",
            book.recs.len() == 1 && book.pending.len() == 2,
        ));
        let n = book.flush();
        v.push(("F0496-补写-搬入主账", n == 2 && book.recs.len() == 3 && book.pending.is_empty()));
    }

    // 三、决策摘要。
    {
        let mut book = RecBook::default();
        book.decide(Decision { pass: "inline", what: "内联热函数", why: Some("调用点少体积小") });
        book.decide(Decision { pass: "unroll", what: "展开 4 次", why: None });
        v.push((
            "F0496-决策-what完整",
            book.decisions[0].what == "内联热函数" && book.decisions[0].why.is_some(),
        ));
        v.push(("F0496-决策-缺why标注", book.incomplete_why() == 1));
    }

    // 四、质量入账 + 计数异常不修数。
    {
        let mut book = RecBook::default();
        book.quality_event("对拍失败", "样例 #3 首违 #2");
        book.quality_event("确定性异常", "pass dce 双跑不一致");
        v.push((
            "F0496-质量-事件入账",
            book.quality.len() == 2 && book.quality[0].contains("对拍失败"),
        ));
        book.odd_counter("weird", 50, 30);
        v.push((
            "F0496-计数-异常如实不修",
            book.odd_counters.len() == 1 && book.odd_counters[0].contains("不修数"),
        ));
        // 正常计数不入异常账。
        book.odd_counter("dce", 10, 30);
        v.push(("F0496-计数-正常不入账", book.odd_counters.len() == 1));
    }

    // 五、事件风暴合并 + 判据自检。
    {
        let mut book = RecBook::default();
        for _ in 0..6 {
            book.storm_event("fold#const");
        }
        book.storm_event("dce#dead");
        v.push((
            "F0496-风暴-达标合并限频",
            book.storm_visible() == vec!["fold#const ×6".to_string()],
        ));
        v.push(("F0496-风暴-未达标不呈现", !book.storm_visible()[0].contains("dce")));
        v.push(("F0496-判据-锚点入册", ANCHOR == "VE-F0496"));
    }

    v
}

/// 注册表入口。
pub fn run_vec96_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec96");
    for (name, ok) in vec96_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec96_raw_checks as run_vec96_checks;
    #[test]
    fn vec96_optlog_checks() {
        for (name, ok) in run_vec96_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
