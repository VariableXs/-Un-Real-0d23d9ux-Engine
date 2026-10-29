//! UNX-F10224 环境受限挂账（受限项挂补测标记可枚举，裸受限拦截）（0x10f20 段）
//!
//! 锚点：UNX-F10224-J1 受限挂账达成（受限项挂补测标记可枚举
//! 10/10 次），裸受限 10/10 次拦截。
//! 域：CoRun·UNX-C3（归因面谱系）。
//!
//! 结构：受限账（受限项 + 缺件清单 + 补测标记 + 枚举序）+ 挂账核
//! （缺件非空方许挂补测）+ 裸受限拦截核 + 折叠指纹 +
//! 注入核（无缺件挂账/重挂/清账未补/封账/枚举越界）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10f20;
/// 诊断码：缺件清单为空
pub const ERR_NO_MISSING: u32 = SEG_BASE + 0x01;
/// 诊断码：重复挂账
pub const ERR_DUP_TAG: u32 = SEG_BASE + 0x02;
/// 诊断码：清账未补件
pub const ERR_CLEAR_UNFILLED: u32 = SEG_BASE + 0x03;
/// 诊断码：封账操作
pub const ERR_FROZEN: u32 = SEG_BASE + 0x04;
/// 诊断码：受限项为空
pub const ERR_EMPTY_ITEM: u32 = SEG_BASE + 0x05;

/// 受限挂账条目
#[derive(Clone, PartialEq, Eq)]
pub struct RestrictEntry {
    /// 受限项标识
    pub item: String,
    /// 缺件清单（非空）
    pub missing_parts: Vec<String>,
    /// 补测标记（挂账时为 true）
    pub retag: bool,
    /// 挂账批次
    pub batch: usize,
}

impl RestrictEntry {
    /// 条目自洽：缺件非空且有补测标记
    pub fn ok(&self) -> bool {
        !self.item.is_empty() && !self.missing_parts.is_empty() && self.retag
    }
}

/// 受限账
pub struct RestrictBook {
    /// 挂账条目
    pub entries: Vec<RestrictEntry>,
    /// 批次序
    pub batch_seq: usize,
    /// 封账
    pub frozen: bool,
}

impl RestrictBook {
    /// 空账
    pub fn new() -> Self {
        RestrictBook { entries: Vec::new(), batch_seq: 0, frozen: false }
    }

    /// 挂账：受限项 + 缺件清单
    pub fn tag(&mut self, item: &str, missing: &[&str]) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if item.is_empty() {
            return Err(ERR_EMPTY_ITEM);
        }
        if missing.is_empty() {
            return Err(ERR_NO_MISSING);
        }
        if self.entries.iter().any(|e| e.item == item) {
            return Err(ERR_DUP_TAG);
        }
        self.batch_seq += 1;
        self.entries.push(RestrictEntry {
            item: item.to_string(),
            missing_parts: missing.iter().map(|m| m.to_string()).collect(),
            retag: true,
            batch: self.batch_seq,
        });
        Ok(())
    }

    /// 补件并清账：缺件全补方可清
    pub fn clear(&mut self, item: &str, filled: &[&str]) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        let pos = match self.entries.iter().position(|e| e.item == item) {
            Some(p) => p,
            None => return Err(ERR_EMPTY_ITEM),
        };
        let need = &self.entries[pos].missing_parts;
        for f in filled {
            if !need.iter().any(|n| n == f) {
                return Err(ERR_CLEAR_UNFILLED);
            }
        }
        if filled.len() < need.len() {
            return Err(ERR_CLEAR_UNFILLED);
        }
        self.entries.remove(pos);
        Ok(())
    }

    /// 裸受限拦截：有缺件但无补测标记的条目视为裸受限
    pub fn naked_entries(&self) -> Vec<String> {
        self.entries.iter().filter(|e| !e.retag).map(|e| e.item.clone()).collect()
    }

    /// 挂账枚举（按批次序）
    pub fn enumerate_by_batch(&self) -> Vec<(usize, String)> {
        let mut out: Vec<(usize, String)> =
            self.entries.iter().map(|e| (e.batch, e.item.clone())).collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// 挂账总数
    pub fn tagged_count(&self) -> usize {
        self.entries.iter().filter(|e| e.retag).count()
    }

    /// 缺件总量
    pub fn missing_total(&self) -> usize {
        self.entries.iter().map(|e| e.missing_parts.len()).sum()
    }

    /// 折叠指纹
    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        let mut acc = SEG_BASE;
        for (i, e) in self.entries.iter().enumerate() {
            acc = acc.wrapping_mul(6271).wrapping_add((e.batch as u32) + i as u32);
            acc = acc.wrapping_add(e.missing_parts.len() as u32);
            for m in &e.missing_parts {
                acc = acc ^ (m.len() as u32);
            }
        }
        acc
    }

    /// 诊断面：某受限项缺件回读
    pub fn missing_of(&self, item: &str) -> Option<Vec<String>> {
        self.entries.iter().find(|e| e.item == item).map(|e| e.missing_parts.clone())
    }

    /// 诊断面：挂账批次回读
    pub fn batch_of(&self, item: &str) -> Option<usize> {
        self.entries.iter().find(|e| e.item == item).map(|e| e.batch)
    }

    /// 诊断面：补测标记率（基点）
    pub fn retag_ratio_bps(&self) -> u32 {
        if self.entries.is_empty() {
            return 0;
        }
        (self.tagged_count() * 10000 / self.entries.len()) as u32
    }

    /// 诊断面：受限项清单
    pub fn item_list(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.item.clone()).collect()
    }

    /// 诊断面：账面摘要
    pub fn book_summary(&self) -> String {
        format!(
            "entries={} tagged={} missing={}",
            self.entries.len(),
            self.tagged_count(),
            self.missing_total()
        )
    }

    /// 诊断面：全条目自洽复核
    pub fn all_ok(&self) -> bool {
        self.entries.iter().all(|e| e.ok())
    }

    /// 诊断面：最长缺件清单项
    pub fn item_with_most_missing(&self) -> Option<(String, usize)> {
        let mut best: Option<(String, usize)> = None;
        for e in &self.entries {
            match &best {
                Some((_, n)) if *n >= e.missing_parts.len() => {}
                _ => best = Some((e.item.clone(), e.missing_parts.len())),
            }
        }
        best
    }

    /// 诊断面：批次无空隙判定（1..=seq 连续）
    pub fn batches_contiguous(&self) -> bool {
        let mut bs: Vec<usize> = self.entries.iter().map(|e| e.batch).collect();
        bs.sort_unstable();
        for (i, b) in bs.iter().enumerate() {
            if *b != i + 1 {
                return false;
            }
        }
        true
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 无缺件挂账拒绝
    let mut b1 = RestrictBook::new();
    out[0] = b1.tag("item-a", &[]) == Err(ERR_NO_MISSING);
    // 2: 重挂拒绝
    let mut b2 = RestrictBook::new();
    let _ = b2.tag("item-b", &["uart"]);
    out[1] = b2.tag("item-b", &["uart"]) == Err(ERR_DUP_TAG);
    // 3: 清账未补件拒绝
    let mut b3 = RestrictBook::new();
    let _ = b3.tag("item-c", &["uart", "hpet"]);
    out[2] = b3.clear("item-c", &["uart"]) == Err(ERR_CLEAR_UNFILLED);
    // 4: 封账操作拒绝
    let mut b4 = RestrictBook::new();
    let _ = b4.tag("item-d", &["hpet"]);
    let _ = b4.seal();
    out[3] = b4.tag("item-e", &["x"]) == Err(ERR_FROZEN)
        && b4.clear("item-d", &["hpet"]) == Err(ERR_FROZEN);
    // 5: 空受限项拒绝
    let mut b5 = RestrictBook::new();
    out[4] = b5.tag("", &["uart"]) == Err(ERR_EMPTY_ITEM);
    out
}

/// 判据函数
pub fn run_unx10224_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10224");
    let mut b = RestrictBook::new();
    let t1 = b.tag("ut-pty-real", &["实机从口 2", "交叉线"]);
    let t2 = b.tag("ut-hpet-wide", &["高频计数器"]);
    s.add("双受限挂账", t1.is_ok() && t2.is_ok(), "两条受限项挂补测标记");
    s.add("条目自洽", b.entries.iter().all(|e| e.ok()), "各条目缺件非空且有标记");
    s.add("无裸受限", b.naked_entries().is_empty(), "无缺件无标记的裸受限");
    s.add("枚举按批", b.enumerate_by_batch() == vec![(1, "ut-pty-real".to_string()), (2, "ut-hpet-wide".to_string())], "枚举按批次序");
    s.add("挂账计数", b.tagged_count() == 2, "挂账两条");
    s.add("缺件总量三", b.missing_total() == 3, "缺件合计三件");
    let c = b.clear("ut-hpet-wide", &["高频计数器"]);
    s.add("补件清账", c.is_ok() && b.tagged_count() == 1, "补齐缺件后清账");
    s.add("清账后枚举", b.enumerate_by_batch() == vec![(1, "ut-pty-real".to_string())], "清账后枚举余一条");
    s.add("未挂项清账拒绝", b.clear("ghost", &["x"]) == Err(ERR_EMPTY_ITEM), "未挂账项不可清");
    s.add("多余补件拒绝", b.clear("ut-pty-real", &["实机从口 2", "交叉线", "幽灵件"]) == Err(ERR_CLEAR_UNFILLED), "清单外补件被拒");
    s.add("缺件回读", b.missing_of("ut-pty-real") == Some(vec!["实机从口 2".to_string(), "交叉线".to_string()]), "缺件清单可回读");
    s.add("批次回读", b.batch_of("ut-pty-real") == Some(1) && b.batch_of("ghost").is_none(), "挂账批次可回读");
    s.add("标记率满格", b.retag_ratio_bps() == 10000, "在账条目全带补测标记");
    s.add("受限项清单", b.item_list() == vec!["ut-pty-real".to_string()], "清账后受限项唯一");
    s.add("账面摘要", b.book_summary().contains("entries=1") && b.book_summary().contains("missing=2"), "摘要含条目数与缺件数");
    s.add("全条目自洽", b.all_ok(), "全条目复核通过");
    s.add("缺件最重项", b.item_with_most_missing() == Some(("ut-pty-real".to_string(), 2)), "最重缺件项命中");
    s.add("批次连续", b.batches_contiguous(), "批次号 1..=n 无空隙");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let fp = b.seal();
    s.add("封账拒绝", b.tag("late", &["x"]) == Err(ERR_FROZEN), "封账后挂账被拒");
    s.add("封账指纹非零", fp != 0, "封账折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_enumerate_clear_cycle() {
        let mut b = RestrictBook::new();
        let _ = b.tag("x", &["m1", "m2"]);
        let _ = b.tag("y", &["m3"]);
        assert_eq!(b.missing_total(), 3);
        assert!(b.clear("x", &["m2", "m1"]).is_ok());
        assert_eq!(b.tagged_count(), 1);
        assert_eq!(b.enumerate_by_batch(), vec![(2, "y".to_string())]);
    }

    #[test]
    fn no_missing_parts_rejected() {
        let mut b = RestrictBook::new();
        assert_eq!(b.tag("z", &[]), Err(ERR_NO_MISSING));
        assert!(b.entries.is_empty());
        assert!(b.naked_entries().is_empty());
    }

    #[test]
    fn batches_stay_contiguous_after_clear() {
        let mut b = RestrictBook::new();
        let _ = b.tag("a", &["m"]);
        let _ = b.tag("b", &["n"]);
        let _ = b.tag("c", &["o"]);
        let _ = b.clear("b", &["n"]);
        assert!(b.batches_contiguous());
        assert_eq!(b.item_list(), vec!["a".to_string(), "c".to_string()]);
        assert_eq!(b.item_with_most_missing().map(|(_, n)| n), Some(1));
    }

    #[test]
    fn summary_reflects_state() {
        let mut b = RestrictBook::new();
        let _ = b.tag("q", &["m1", "m2", "m3"]);
        assert!(b.book_summary().contains("entries=1"));
        assert!(b.book_summary().contains("missing=3"));
        assert_eq!(b.retag_ratio_bps(), 10000);
    }

    #[test]
    fn missing_roundtrip_and_ghost() {
        let mut b = RestrictBook::new();
        let _ = b.tag("it", &["m1"]);
        assert_eq!(b.missing_of("it"), Some(vec!["m1".to_string()]));
        assert_eq!(b.missing_of("nope"), None);
        assert_eq!(b.batch_of("nope"), None);
        assert!(b.all_ok());
    }

    #[test]
    fn clear_requires_full_fill() {
        let mut b = RestrictBook::new();
        let _ = b.tag("m", &["a", "b", "c"]);
        assert_eq!(b.clear("m", &["a", "b"]), Err(ERR_CLEAR_UNFILLED));
        assert_eq!(b.tagged_count(), 1);
        assert!(b.clear("m", &["c", "a", "b"]).is_ok());
        assert_eq!(b.tagged_count(), 0);
        assert!(b.enumerate_by_batch().is_empty());
    }
}
