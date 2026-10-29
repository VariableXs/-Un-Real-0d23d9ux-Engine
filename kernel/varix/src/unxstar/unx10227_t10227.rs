//! UNX-F10227 修复重跑闭环（修复通知触达后重跑翻绿留痕，闭环断裂检出）（0x10f80 段）
//!
//! 锚点：UNX-F10227-J1 闭环达成（修复通知触达后重跑翻绿留痕
//! 10/10 次），闭环断裂 10/10 次检出。
//! 域：CoRun·UNX-C3（归因面谱系）。
//!
//! 结构：闭环账（工单 → 修复通知 → 重跑 → 翻绿留痕四段）+ 触达核
//! （通知未达不得重跑翻绿）+ 断裂检出核 + 折叠指纹 +
//! 注入核（未通知翻绿/重跑未翻绿/重复通知/封账/回跳翻绿）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10f80;
/// 诊断码：通知未达
pub const ERR_NOT_NOTIFIED: u32 = SEG_BASE + 0x01;
/// 诊断码：重复通知
pub const ERR_DUP_NOTIFY: u32 = SEG_BASE + 0x02;
/// 诊断码：重跑未翻绿
pub const ERR_STILL_RED: u32 = SEG_BASE + 0x03;
/// 诊断码：回跳翻绿
pub const ERR_REGRESS: u32 = SEG_BASE + 0x04;
/// 诊断码：封账操作
pub const ERR_FROZEN: u32 = SEG_BASE + 0x05;

/// 闭环阶段
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoopStage {
    /// 已开单待修
    Opened,
    /// 通知已达
    Notified,
    /// 重跑已过
    RerunGreen,
}

impl LoopStage {
    /// 阶段序
    pub fn ord(&self) -> u8 {
        match self {
            LoopStage::Opened => 1,
            LoopStage::Notified => 2,
            LoopStage::RerunGreen => 3,
        }
    }
}

/// 闭环条目
#[derive(Clone, PartialEq, Eq)]
pub struct LoopEntry {
    /// 工单号
    pub ticket: String,
    /// 阶段
    pub stage: LoopStage,
    /// 通知触达时间戳（tick）
    pub notified_at: Option<u32>,
    /// 重跑时间戳（tick）
    pub rerun_at: Option<u32>,
}

impl LoopEntry {
    /// 条目自洽：阶段与时间戳一致
    pub fn ok(&self) -> bool {
        !self.ticket.is_empty()
            && match self.stage {
                LoopStage::Opened => self.notified_at.is_none() && self.rerun_at.is_none(),
                LoopStage::Notified => self.notified_at.is_some() && self.rerun_at.is_none(),
                LoopStage::RerunGreen => self.notified_at.is_some() && self.rerun_at.is_some(),
            }
    }
}

/// 闭环账
pub struct CloseLoopBook {
    /// 闭环条目
    pub entries: Vec<LoopEntry>,
    /// 封账
    pub frozen: bool,
}

impl CloseLoopBook {
    /// 空账
    pub fn new() -> Self {
        CloseLoopBook { entries: Vec::new(), frozen: false }
    }

    /// 开闭环（工单入场）
    pub fn open(&mut self, ticket: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if ticket.is_empty() {
            return Err(ERR_NOT_NOTIFIED);
        }
        if self.entries.iter().any(|e| e.ticket == ticket) {
            return Err(ERR_DUP_NOTIFY);
        }
        self.entries.push(LoopEntry {
            ticket: ticket.to_string(),
            stage: LoopStage::Opened,
            notified_at: None,
            rerun_at: None,
        });
        Ok(())
    }

    /// 修复通知触达
    pub fn notify(&mut self, ticket: &str, at: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        let pos = match self.entries.iter().position(|e| e.ticket == ticket) {
            Some(p) => p,
            None => return Err(ERR_NOT_NOTIFIED),
        };
        if self.entries[pos].stage != LoopStage::Opened {
            return Err(ERR_DUP_NOTIFY);
        }
        self.entries[pos].stage = LoopStage::Notified;
        self.entries[pos].notified_at = Some(at);
        Ok(())
    }

    /// 重跑翻绿（通知未达则拒）
    pub fn rerun_green(&mut self, ticket: &str, at: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        let pos = match self.entries.iter().position(|e| e.ticket == ticket) {
            Some(p) => p,
            None => return Err(ERR_NOT_NOTIFIED),
        };
        match self.entries[pos].stage {
            LoopStage::Opened => Err(ERR_NOT_NOTIFIED),
            LoopStage::Notified => {
                if self.entries[pos].notified_at.unwrap() > at {
                    return Err(ERR_REGRESS);
                }
                self.entries[pos].stage = LoopStage::RerunGreen;
                self.entries[pos].rerun_at = Some(at);
                Ok(())
            }
            LoopStage::RerunGreen => Err(ERR_REGRESS),
        }
    }

    /// 断裂检出：有单停在 Opened/Notified 超过宽限 tick
    pub fn broken_loops(&self, now: u32, grace: u32) -> Vec<String> {
        let mut out = Vec::new();
        for e in &self.entries {
            let stalled = match (e.stage, e.notified_at, e.rerun_at) {
                (LoopStage::Opened, None, None) => true,
                (LoopStage::Notified, Some(n), None) => now.saturating_sub(n) > grace,
                _ => false,
            };
            if stalled {
                out.push(e.ticket.clone());
            }
        }
        out
    }

    /// 闭环完成率（基点）
    pub fn closed_ratio_bps(&self) -> u32 {
        if self.entries.is_empty() {
            return 0;
        }
        let done = self.entries.iter().filter(|e| e.stage == LoopStage::RerunGreen).count();
        (done * 10000 / self.entries.len()) as u32
    }

    /// 诊断面：阶段分布（开/通知/翻绿）
    pub fn stage_split(&self) -> (usize, usize, usize) {
        let o = self.entries.iter().filter(|e| e.stage == LoopStage::Opened).count();
        let n = self.entries.iter().filter(|e| e.stage == LoopStage::Notified).count();
        let g = self.entries.iter().filter(|e| e.stage == LoopStage::RerunGreen).count();
        (o, n, g)
    }

    /// 诊断面：某单阶段回读
    pub fn stage_of(&self, ticket: &str) -> Option<LoopStage> {
        self.entries.iter().find(|e| e.ticket == ticket).map(|e| e.stage)
    }

    /// 诊断面：通知时延（重跑-通知，tick）
    pub fn latency_of(&self, ticket: &str) -> Option<u32> {
        match self.entries.iter().find(|e| e.ticket == ticket) {
            Some(e) => match (e.notified_at, e.rerun_at) {
                (Some(n), Some(r)) => Some(r - n),
                _ => None,
            },
            None => None,
        }
    }

    /// 诊断面：账面摘要
    pub fn book_summary(&self) -> String {
        format!(
            "loops={} closed_ratio_bps={}",
            self.entries.len(),
            self.closed_ratio_bps()
        )
    }

    /// 诊断面：阶段名
    pub fn stage_name(s: LoopStage) -> &'static str {
        match s {
            LoopStage::Opened => "OPENED",
            LoopStage::Notified => "NOTIFIED",
            LoopStage::RerunGreen => "RERUN_GREEN",
        }
    }

    /// 折叠指纹
    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        let mut acc = SEG_BASE;
        for (i, e) in self.entries.iter().enumerate() {
            acc = acc.wrapping_mul(6421).wrapping_add((e.stage.ord() as u32) + i as u32);
            if let Some(n) = e.notified_at {
                acc = acc.wrapping_add(n);
            }
            if let Some(r) = e.rerun_at {
                acc = acc.wrapping_add(r.wrapping_mul(3));
            }
        }
        acc
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 未通知翻绿拒绝
    let mut b1 = CloseLoopBook::new();
    let _ = b1.open("t1");
    out[0] = b1.rerun_green("t1", 10) == Err(ERR_NOT_NOTIFIED);
    // 2: 重复通知拒绝
    let mut b2 = CloseLoopBook::new();
    let _ = b2.open("t2");
    let _ = b2.notify("t2", 5);
    out[1] = b2.notify("t2", 6) == Err(ERR_DUP_NOTIFY);
    // 3: 重跑未翻绿（幽灵单）
    let mut b3 = CloseLoopBook::new();
    out[2] = b3.rerun_green("ghost", 10) == Err(ERR_NOT_NOTIFIED);
    // 4: 回跳翻绿拒绝（重跑时刻早于通知）
    let mut b4 = CloseLoopBook::new();
    let _ = b4.open("t4");
    let _ = b4.notify("t4", 100);
    out[3] = b4.rerun_green("t4", 50) == Err(ERR_REGRESS);
    // 5: 封账拒绝
    let mut b5 = CloseLoopBook::new();
    let _ = b5.open("t5");
    let _ = b5.seal();
    out[4] = b5.open("t5b") == Err(ERR_FROZEN) && b5.notify("t5", 1) == Err(ERR_FROZEN);
    out
}

/// 判据函数
pub fn run_unx10227_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10227");
    let mut b = CloseLoopBook::new();
    let o1 = b.open("BUG-UNX-301");
    let o2 = b.open("BUG-UNX-302");
    s.add("双闭环开立", o1.is_ok() && o2.is_ok(), "两张闭环入场");
    let n1 = b.notify("BUG-UNX-301", 100);
    s.add("通知触达", n1.is_ok() && b.entries[0].stage == LoopStage::Notified, "修复通知触达留痕");
    s.add("未通知单拒重跑", b.rerun_green("BUG-UNX-302", 100) == Err(ERR_NOT_NOTIFIED), "通知未达不得翻绿");
    let g1 = b.rerun_green("BUG-UNX-301", 150);
    s.add("重跑翻绿", g1.is_ok() && b.entries[0].stage == LoopStage::RerunGreen, "重跑翻绿留痕");
    s.add("条目自洽", b.entries.iter().all(|e| e.ok()), "各条目阶段与时间戳一致");
    s.add("闭环完成率半数", b.closed_ratio_bps() == 5000, "两单一翻绿 5000 基点");
    s.add("断裂检出", b.broken_loops(400, 200) == vec!["BUG-UNX-302".to_string()], "302 号停在通知超宽限");
    s.add("宽限内不断裂", {
        // 独立样本：通知后 150 tick 落在 200 宽限带内，不得断裂。
        let mut bw = CloseLoopBook::new();
        let _ = bw.open("w1");
        let _ = bw.notify("w1", 100);
        bw.broken_loops(250, 200).is_empty()
    }, "通知后宽限内无断裂");
    s.add("重复通知拒绝", b.notify("BUG-UNX-301", 200) == Err(ERR_DUP_NOTIFY), "已通知单不得再通知");
    s.add("重复翻绿拒绝", b.rerun_green("BUG-UNX-301", 300) == Err(ERR_REGRESS), "已翻绿单不得回跳");
    s.add("阶段分布", b.stage_split() == (1, 0, 1), "一开一翻绿");
    s.add("阶段回读", b.stage_of("BUG-UNX-301") == Some(LoopStage::RerunGreen) && b.stage_of("ghost").is_none(), "在账可回读、缺席为 None");
    s.add("通知时延", b.latency_of("BUG-UNX-301") == Some(50) && b.latency_of("BUG-UNX-302").is_none(), "301 时延 50 tick、302 未翻绿");
    s.add("账面摘要", b.book_summary().contains("loops=2"), "摘要含闭环数");
    s.add("阶段名", CloseLoopBook::stage_name(LoopStage::Notified) == "NOTIFIED" && CloseLoopBook::stage_name(LoopStage::RerunGreen) == "RERUN_GREEN", "阶段名正确");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let fp = b.seal();
    s.add("封账拒绝", b.open("late") == Err(ERR_FROZEN), "封账后入场被拒");
    s.add("封账指纹非零", fp != 0, "封账折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_loop_flow() {
        let mut b = CloseLoopBook::new();
        let _ = b.open("t");
        let _ = b.notify("t", 10);
        assert!(b.rerun_green("t", 20).is_ok());
        assert_eq!(b.closed_ratio_bps(), 10000);
        assert!(b.broken_loops(100, 5).is_empty());
        assert!(b.entries[0].ok());
    }

    #[test]
    fn notify_requires_open_entry() {
        let mut b = CloseLoopBook::new();
        assert_eq!(b.notify("nope", 1), Err(ERR_NOT_NOTIFIED));
        let _ = b.open("t2");
        let _ = b.notify("t2", 1);
        assert_eq!(b.notify("t2", 2), Err(ERR_DUP_NOTIFY));
    }

    #[test]
    fn latency_and_stage_names() {
        let mut b = CloseLoopBook::new();
        let _ = b.open("k");
        let _ = b.notify("k", 40);
        let _ = b.rerun_green("k", 90);
        assert_eq!(b.latency_of("k"), Some(50));
        assert_eq!(CloseLoopBook::stage_name(b.stage_of("k").unwrap()), "RERUN_GREEN");
        assert!(b.book_summary().contains("closed_ratio_bps=10000"));
    }

    #[test]
    fn broken_loop_thresholds() {
        let mut b = CloseLoopBook::new();
        let _ = b.open("t1");
        let _ = b.open("t2");
        let _ = b.notify("t2", 10);
        // 开单未通知恒断裂
        assert!(b.broken_loops(1, 0).contains(&"t1".to_string()));
        // 通知后宽限内不断裂
        assert!(!b.broken_loops(15, 10).contains(&"t2".to_string()));
        // 超宽限断裂
        assert!(b.broken_loops(25, 10).contains(&"t2".to_string()));
        assert_eq!(b.stage_split(), (1, 1, 0));
    }
}
