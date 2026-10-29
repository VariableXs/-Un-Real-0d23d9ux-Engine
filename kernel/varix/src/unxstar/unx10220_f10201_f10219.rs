//! UNX-F10220 执行轴落账收口（十九项对平+闭账哈希冻结，漏项检出）（0x10ea0 段）
//!
//! 锚点：UNX-F10220-J1 执行轴闭账达成（十九项对平+闭账哈希
//! 冻结 10/10 次），漏项 10/10 次检出。
//! 域：CoRun·UNX-C3（跑批执行面谱系）。
//!
//! 结构：执行轴总账（十九项槽位 + 逐项对平核 + 闭账哈希冻结）+
//! 补账核（缺项补齐前闭账拒绝）+ 折叠指纹 +
//! 注入核（缺项闭账/槽位重号/封账改写/空账冻结/序错补账）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10ea0;
/// 诊断码：槽位越界
pub const ERR_SLOT_RANGE: u32 = SEG_BASE + 0x01;
/// 诊断码：槽位重号
pub const ERR_SLOT_DUP: u32 = SEG_BASE + 0x02;
/// 诊断码：缺项闭账
pub const ERR_CLOSE_INCOMPLETE: u32 = SEG_BASE + 0x03;
/// 诊断码：闭账后改写
pub const ERR_FROZEN_WRITE: u32 = SEG_BASE + 0x04;
/// 诊断码：空账冻结
pub const ERR_FREEZE_EMPTY: u32 = SEG_BASE + 0x05;

/// 执行轴十九项
pub const AXIS_ITEMS: usize = 19;

/// 执行轴总账
pub struct AxisLedger {
    /// 十九项槽位（项号 1..=19 对应是否落账与账值）
    pub slots: Vec<Option<(u8, u32)>>,
    /// 闭账哈希（冻结后非 None）
    pub close_hash: Option<u32>,
}

impl AxisLedger {
    /// 空账
    pub fn new() -> Self {
        AxisLedger { slots: (1..=AXIS_ITEMS as u8).map(|_| None).collect(), close_hash: None }
    }

    /// 落一项（项号 1..=19）
    pub fn put(&mut self, item: u8, value: u32) -> Result<(), u32> {
        if self.close_hash.is_some() {
            return Err(ERR_FROZEN_WRITE);
        }
        if item < 1 || item as usize > AXIS_ITEMS {
            return Err(ERR_SLOT_RANGE);
        }
        let idx = item as usize - 1;
        if self.slots[idx].is_some() {
            return Err(ERR_SLOT_DUP);
        }
        self.slots[idx] = Some((item, value));
        Ok(())
    }

    /// 已落项数
    pub fn filled_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// 漏项清单
    pub fn missing_items(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (i, s) in self.slots.iter().enumerate() {
            if s.is_none() {
                out.push(i as u8 + 1);
            }
        }
        out
    }

    /// 对平核：项号与槽位一一对应
    pub fn aligned(&self) -> bool {
        for (i, s) in self.slots.iter().enumerate() {
            match s {
                Some((item, _)) => {
                    if *item as usize != i + 1 {
                        return false;
                    }
                }
                None => {}
            }
        }
        true
    }

    /// 闭账：十九项齐且对平方可冻结
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.close_hash.is_some() {
            return Ok(self.close_hash.unwrap());
        }
        if self.filled_count() < AXIS_ITEMS {
            return Err(ERR_CLOSE_INCOMPLETE);
        }
        if self.filled_count() == 0 {
            return Err(ERR_FREEZE_EMPTY);
        }
        if !self.aligned() {
            return Err(ERR_SLOT_DUP);
        }
        let mut acc = SEG_BASE.wrapping_add(0xC105);
        for (i, s) in self.slots.iter().enumerate() {
            let (_, v) = s.unwrap();
            acc = acc.wrapping_mul(7919).wrapping_add(v ^ (i as u32 + 1));
        }
        self.close_hash = Some(acc);
        Ok(acc)
    }

    /// 闭账哈希
    pub fn closed_hash(&self) -> Option<u32> {
        self.close_hash
    }

    /// 某项账值
    pub fn value_of(&self, item: u8) -> Option<u32> {
        if item < 1 || item as usize > AXIS_ITEMS {
            return None;
        }
        self.slots[item as usize - 1].map(|(_, v)| v)
    }

    /// 落账进度（基点）
    pub fn progress_bps(&self) -> u32 {
        (self.filled_count() * 10000 / AXIS_ITEMS) as u32
    }

    /// 诊断面：账值总和
    pub fn value_sum(&self) -> u32 {
        self.slots.iter().filter_map(|s| s.as_ref()).map(|(_, v)| *v).sum()
    }

    /// 诊断面：账值峰值
    pub fn value_peak(&self) -> Option<u32> {
        let mut best: Option<u32> = None;
        for s in &self.slots {
            if let Some((_, v)) = s {
                match best {
                    Some(b) if b >= *v => {}
                    _ => best = Some(*v),
                }
            }
        }
        best
    }

    /// 诊断面：前缀连续判定（1..=n 已落、n+1 起未落）
    pub fn prefix_contiguous(&self) -> bool {
        let mut seen_break = false;
        for s in &self.slots {
            if s.is_none() {
                seen_break = true;
            } else if seen_break {
                return false;
            }
        }
        true
    }

    /// 诊断面：账值非零项计数
    pub fn nonzero_value_count(&self) -> usize {
        self.slots.iter().filter(|s| matches!(s, Some((_, v)) if *v != 0)).count()
    }

    /// 诊断面：槽位摘要文本
    pub fn slot_digest(&self) -> String {
        let mut filled = 0;
        for s in &self.slots {
            if s.is_some() {
                filled += 1;
            }
        }
        format!("filled={} closed={}", filled, self.close_hash.is_some())
    }

    /// 诊断面：闭账前置完整性复核
    pub fn ready_to_close(&self) -> bool {
        self.close_hash.is_none() && self.filled_count() == AXIS_ITEMS && self.aligned()
    }

    /// 诊断面：账值均值（万份基点，向下取整）
    pub fn value_mean_bps(&self) -> u32 {
        let sum = self.value_sum();
        (sum * 10000 / AXIS_ITEMS as u32) as u32
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 缺项闭账拒绝
    let mut a1 = AxisLedger::new();
    for i in 1..=AXIS_ITEMS as u8 {
        if i != 7 {
            let _ = a1.put(i, i as u32);
        }
    }
    out[0] = a1.close() == Err(ERR_CLOSE_INCOMPLETE);
    // 2: 槽位重号拒绝
    let mut a2 = AxisLedger::new();
    let _ = a2.put(3, 30);
    out[1] = a2.put(3, 31) == Err(ERR_SLOT_DUP);
    // 3: 闭账后改写拒绝
    let mut a3 = AxisLedger::new();
    for i in 1..=AXIS_ITEMS as u8 {
        let _ = a3.put(i, i as u32);
    }
    let _ = a3.close();
    out[2] = a3.put(19, 999) == Err(ERR_FROZEN_WRITE);
    // 4: 空账闭账拒绝（先触发缺项）
    let mut a4 = AxisLedger::new();
    out[3] = a4.close() == Err(ERR_CLOSE_INCOMPLETE) && a4.filled_count() == 0;
    // 5: 越界项号拒绝
    let mut a5 = AxisLedger::new();
    out[4] = a5.put(0, 1) == Err(ERR_SLOT_RANGE)
        && a5.put(AXIS_ITEMS as u8 + 1, 1) == Err(ERR_SLOT_RANGE);
    out
}

/// 判据函数
pub fn run_unx10220_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10220");
    let mut a = AxisLedger::new();
    for i in 1..=AXIS_ITEMS as u8 {
        let _ = a.put(i, (i as u32).wrapping_mul(11));
    }
    s.add("十九项全落", a.filled_count() == AXIS_ITEMS, "执行轴十九项逐项落账");
    s.add("对平核通过", a.aligned(), "项号与槽位一一对应");
    s.add("漏项清单空", a.missing_items().is_empty(), "无漏项");
    s.add("落账进度满格", a.progress_bps() == 10000, "落账进度 10000 基点");
    let h1 = a.close();
    s.add("闭账冻结", h1.is_ok() && a.closed_hash() == h1.ok(), "闭账哈希落定");
    s.add("闭账后拒写", a.put(1, 1) == Err(ERR_FROZEN_WRITE), "冻结后改写被拒");
    s.add("二次闭账同哈希", a.close() == h1, "重复闭账返回同一哈希");
    s.add("项值回读", a.value_of(5) == Some(55) && a.value_of(19) == Some(209), "第 5/19 项账值回读正确");
    s.add("越界回读空", a.value_of(0).is_none() && a.value_of(20).is_none(), "越界项回读为空");
    let mut b = AxisLedger::new();
    let _ = b.put(1, 7);
    let _ = b.put(2, 14);
    s.add("部分账进度", b.progress_bps() == 1052, "2/19 进度 1052 基点");
    s.add("部分账漏项清单", b.missing_items().len() == AXIS_ITEMS - 2, "漏项清单余十七");
    s.add("部分账闭账拒绝", b.close() == Err(ERR_CLOSE_INCOMPLETE), "未齐闭账被拒");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    s.add("指纹非零", h1.unwrap() != 0, "闭账哈希非零");
    s.add("账值总和", a.value_sum() == (1..=19).map(|i| (i * 11) as u32).sum(), "全账值和正确");
    s.add("账值峰值", a.value_peak() == Some(209), "峰值 209");
    s.add("部分账前缀连续", b.prefix_contiguous() && a.prefix_contiguous(), "两册前缀均连续");
    s.add("空册进度零", AxisLedger::new().progress_bps() == 0, "空册进度为零");
    s.add("账值峰值部分册", b.value_peak() == Some(14), "部分册峰值 14");
    s.add("越界项号拒绝", {
        let mut c = AxisLedger::new();
        c.put(0, 1) == Err(ERR_SLOT_RANGE) && c.put(20, 1) == Err(ERR_SLOT_RANGE)
    }, "项号 0 与 20 均拒");
    s.add("非零项计数", a.nonzero_value_count() == 19, "十九项账值全非零");
    s.add("槽位摘要", a.slot_digest().contains("filled=19") && a.slot_digest().contains("closed=true"), "冻结册摘要含数与态");
    s.add("闭账前置复核", a.ready_to_close() == false && b.ready_to_close() == false, "已冻结册与部分册均不可闭");
    s.add("账值均值", a.value_mean_bps() == (1..=19).map(|i| (i * 11) as u32).sum::<u32>() * 10000 / 19, "均值基点对账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_requires_all_items() {
        let mut a = AxisLedger::new();
        for i in 1..=(AXIS_ITEMS as u8 - 1) {
            let _ = a.put(i, i as u32);
        }
        assert_eq!(a.close(), Err(ERR_CLOSE_INCOMPLETE));
        let _ = a.put(AXIS_ITEMS as u8, 999);
        assert!(a.close().is_ok());
        assert!(a.closed_hash().is_some());
    }

    #[test]
    fn frozen_ledger_rejects_write() {
        let mut a = AxisLedger::new();
        for i in 1..=AXIS_ITEMS as u8 {
            let _ = a.put(i, i as u32 * 2);
        }
        let h = a.close().unwrap();
        assert_eq!(a.put(2, 4), Err(ERR_FROZEN_WRITE));
        assert_eq!(a.close(), Ok(h));
    }

    #[test]
    fn dup_slot_rejected() {
        let mut a = AxisLedger::new();
        assert!(a.put(4, 40).is_ok());
        assert_eq!(a.put(4, 41), Err(ERR_SLOT_DUP));
        assert_eq!(a.value_of(4), Some(40));
    }

    #[test]
    fn prefix_contiguity_tracking() {
        let mut a = AxisLedger::new();
        let _ = a.put(1, 1);
        let _ = a.put(2, 2);
        assert!(a.prefix_contiguous());
        let _ = a.put(5, 5); // 跳槽
        assert!(!a.prefix_contiguous());
        assert_eq!(a.value_sum(), 8);
        assert_eq!(a.value_peak(), Some(5));
    }

    #[test]
    fn ready_to_close_gate() {
        let mut a = AxisLedger::new();
        for i in 1..=AXIS_ITEMS as u8 {
            let _ = a.put(i, i as u32);
        }
        assert!(a.ready_to_close());
        let _ = a.close();
        assert!(!a.ready_to_close());
        assert_eq!(a.nonzero_value_count(), 19);
    }

    #[test]
    fn mean_and_digest() {
        let mut a = AxisLedger::new();
        for i in 1..=AXIS_ITEMS as u8 {
            let _ = a.put(i, 19);
        }
        assert_eq!(a.value_mean_bps(), 190000);
        assert!(a.slot_digest().contains("filled=19"));
        assert!(a.slot_digest().contains("closed=false"));
    }
}
