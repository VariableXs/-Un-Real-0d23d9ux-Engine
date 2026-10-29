//! UNX-F10217 执行链路日志（每用例三段日志齐备，缺段检出）（0x10e40 段）
//!
//! 锚点：UNX-F10217-J1 链路日志达成（每用例三段日志齐备
//! 10/10 次），缺段 10/10 次检出。
//! 域：CoRun·UNX-C3（跑批执行面谱系）。
//!
//! 结构：三段链（前置 setup / 主体 body / 收尾 teardown）+ 段账
//! （逐用例逐段留痕）+ 齐备核（三段全在方为完整链）+ 缺段检出 +
//! 折叠指纹 + 注入核（缺前/缺中/缺后/空段/重段）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10e40;
/// 诊断码：段名为空
pub const ERR_EMPTY_SEG: u32 = SEG_BASE + 0x01;
/// 诊断码：用例为空
pub const ERR_EMPTY_CASE: u32 = SEG_BASE + 0x02;
/// 诊断码：段重复
pub const ERR_DUP_SEG: u32 = SEG_BASE + 0x03;
/// 诊断码：段序倒置
pub const ERR_SEG_ORDER: u32 = SEG_BASE + 0x04;
/// 诊断码：账册封禁
pub const ERR_SEALED: u32 = SEG_BASE + 0x05;

/// 三段名
pub const SEG_SETUP: &str = "setup";
pub const SEG_BODY: &str = "body";
pub const SEG_TEARDOWN: &str = "teardown";

/// 段位序号
pub fn seg_ord(name: &str) -> Option<u8> {
    match name {
        SEG_SETUP => Some(1),
        SEG_BODY => Some(2),
        SEG_TEARDOWN => Some(3),
        _ => None,
    }
}

/// 单段留痕
#[derive(Clone, PartialEq, Eq)]
pub struct SegNote {
    /// 用例标识
    pub case: String,
    /// 段名
    pub seg: String,
    /// 段耗时（tick）
    pub ticks: usize,
}

impl SegNote {
    /// 留痕自洽
    pub fn ok(&self) -> bool {
        !self.case.is_empty() && seg_ord(&self.seg).is_some()
    }
}

/// 链路账册
pub struct TraceBook {
    /// 段留痕
    pub notes: Vec<SegNote>,
    /// 封账
    pub sealed: bool,
}

impl TraceBook {
    /// 开册
    pub fn new() -> Self {
        TraceBook { notes: Vec::new(), sealed: false }
    }

    /// 记一段
    pub fn record(&mut self, case: &str, seg: &str, ticks: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(ERR_SEALED);
        }
        if case.is_empty() {
            return Err(ERR_EMPTY_CASE);
        }
        if seg_ord(seg).is_none() {
            return Err(ERR_EMPTY_SEG);
        }
        let ord = seg_ord(seg).unwrap();
        // 重复段拒绝
        if self.notes.iter().any(|n| n.case == case && n.seg == seg) {
            return Err(ERR_DUP_SEG);
        }
        // 段序倒置：后续段不得先于前置段出现
        let prefix_ok = match ord {
            1 => true,
            2 => self.notes.iter().any(|n| n.case == case && n.seg == SEG_SETUP),
            _ => self.notes.iter().any(|n| n.case == case && n.seg == SEG_BODY),
        };
        if !prefix_ok {
            return Err(ERR_SEG_ORDER);
        }
        self.notes.push(SegNote { case: case.to_string(), seg: seg.to_string(), ticks });
        Ok(())
    }

    /// 某用例三段齐备判定
    pub fn chain_complete(&self, case: &str) -> bool {
        for seg in [SEG_SETUP, SEG_BODY, SEG_TEARDOWN] {
            if !self.notes.iter().any(|n| n.case == case && n.seg == seg) {
                return false;
            }
        }
        true
    }

    /// 缺段检出清单
    pub fn incomplete_chains(&self) -> Vec<String> {
        let mut cases: Vec<String> = Vec::new();
        for n in &self.notes {
            if !cases.contains(&n.case) {
                cases.push(n.case.clone());
            }
        }
        cases.into_iter().filter(|c| !self.chain_complete(c)).collect()
    }

    /// 某用例总耗时
    pub fn ticks_of(&self, case: &str) -> usize {
        self.notes.iter().filter(|n| n.case == case).map(|n| n.ticks).sum()
    }

    /// 完整链率（基点）
    pub fn complete_ratio_bps(&self) -> u32 {
        let cases = self.distinct_cases();
        if cases.is_empty() {
            return 0;
        }
        let done = cases.iter().filter(|c| self.chain_complete(c)).count();
        (done * 10000 / cases.len()) as u32
    }

    /// 去重用例清单
    pub fn distinct_cases(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for n in &self.notes {
            if !out.contains(&n.case) {
                out.push(n.case.clone());
            }
        }
        out
    }

    /// 折叠指纹
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        let mut acc = SEG_BASE;
        for (i, n) in self.notes.iter().enumerate() {
            acc = acc.wrapping_mul(7717).wrapping_add((n.ticks as u32) + i as u32);
            for b in n.seg.as_bytes() {
                acc = acc ^ (*b as u32);
            }
        }
        acc
    }

    /// 诊断面：某用例段耗时明细（按段序）
    pub fn tick_breakdown(&self, case: &str) -> [usize; 3] {
        let mut out = [0usize; 3];
        for n in &self.notes {
            if n.case == case {
                if let Some(o) = seg_ord(&n.seg) {
                    out[o as usize - 1] = n.ticks;
                }
            }
        }
        out
    }

    /// 诊断面：前置占比（setup 耗时/总耗时，基点）
    pub fn setup_share_bps(&self, case: &str) -> u32 {
        let total = self.ticks_of(case);
        if total == 0 {
            return 0;
        }
        (self.tick_breakdown(case)[0] * 10000 / total) as u32
    }

    /// 诊断面：最重主体段（全局最长 body）
    pub fn heaviest_body(&self) -> Option<(String, usize)> {
        let mut best: Option<(String, usize)> = None;
        for n in &self.notes {
            if n.seg == SEG_BODY {
                match &best {
                    Some((_, t)) if *t >= n.ticks => {}
                    _ => best = Some((n.case.clone(), n.ticks)),
                }
            }
        }
        best
    }

    /// 诊断面：段留痕总数
    pub fn note_count(&self) -> usize {
        self.notes.len()
    }

    /// 诊断面：段耗时总量
    pub fn total_ticks(&self) -> usize {
        self.notes.iter().map(|n| n.ticks).sum()
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 缺前置（直接 body）
    let mut b1 = TraceBook::new();
    out[0] = b1.record("c1", SEG_BODY, 5) == Err(ERR_SEG_ORDER);
    // 2: 缺主体（setup 后直接 teardown）
    let mut b2 = TraceBook::new();
    let _ = b2.record("c2", SEG_SETUP, 1);
    out[1] = b2.record("c2", SEG_TEARDOWN, 3) == Err(ERR_SEG_ORDER);
    // 3: 重复段
    let mut b3 = TraceBook::new();
    let _ = b3.record("c3", SEG_SETUP, 1);
    out[2] = b3.record("c3", SEG_SETUP, 2) == Err(ERR_DUP_SEG);
    // 4: 未知段名
    let mut b4 = TraceBook::new();
    out[3] = b4.record("c4", "mystery", 1) == Err(ERR_EMPTY_SEG);
    // 5: 空用例
    let mut b5 = TraceBook::new();
    out[4] = b5.record("", SEG_SETUP, 1) == Err(ERR_EMPTY_CASE);
    out
}

/// 判据函数
pub fn run_unx10217_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10217");
    let mut b = TraceBook::new();
    let r1 = b.record("ut-tri-a", SEG_SETUP, 4);
    let r2 = b.record("ut-tri-a", SEG_BODY, 20);
    let r3 = b.record("ut-tri-a", SEG_TEARDOWN, 6);
    s.add("三段齐备", r1.is_ok() && r2.is_ok() && r3.is_ok(), "A 用例三段全受理");
    s.add("链完整判定", b.chain_complete("ut-tri-a"), "A 链完整");
    s.add("总耗三十", b.ticks_of("ut-tri-a") == 30, "A 用例三段共 30 tick");
    let _ = b.record("ut-tri-b", SEG_SETUP, 2);
    let _ = b.record("ut-tri-b", SEG_BODY, 9);
    s.add("B 链缺收尾", !b.chain_complete("ut-tri-b"), "B 链缺 teardown 判缺");
    s.add("缺段清单命中", b.incomplete_chains() == vec!["ut-tri-b".to_string()], "缺段清单仅含 B");
    s.add("完整率半数", b.complete_ratio_bps() == 5000, "两链一全一缺 5000 基点");
    s.add("段留痕自洽", b.notes.iter().all(|n| n.ok()), "各段留痕字段自洽");
    s.add("段名序号", seg_ord(SEG_SETUP) == Some(1) && seg_ord(SEG_BODY) == Some(2) && seg_ord(SEG_TEARDOWN) == Some(3), "三段序号 1/2/3");
    s.add("未知段序号空", seg_ord("nope").is_none(), "未知段名无序号");
    s.add("重复段拒绝", b.record("ut-tri-a", SEG_BODY, 1) == Err(ERR_DUP_SEG), "同段重记被拒");
    s.add("倒序拒绝", b.record("ut-tri-c", SEG_TEARDOWN, 1) == Err(ERR_SEG_ORDER), "无前置段直接收尾被拒");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    s.add("段耗时明细", b.tick_breakdown("ut-tri-a") == [4, 20, 6], "A 用例三段明细 4/20/6");
    s.add("前置占比", b.setup_share_bps("ut-tri-a") == 1333, "setup 占比 4/30=1333 基点");
    s.add("最重主体", b.heaviest_body() == Some(("ut-tri-a".to_string(), 20)), "A 的 body 20 tick 为全局最重");
    s.add("留痕总数", b.note_count() == 5, "A 三段+B 两段共五痕");
    s.add("耗时总量", b.total_ticks() == 41, "全册耗时 41 tick");
    s.add("空册零占比", TraceBook::new().setup_share_bps("none") == 0, "空册无占比可算");
    s.add("全过率样例", {
        let mut bc = TraceBook::new();
        for seg in [SEG_SETUP, SEG_BODY, SEG_TEARDOWN] {
            let _ = bc.record("ok-case", seg, 1);
        }
        bc.complete_ratio_bps() == 10000 && bc.incomplete_chains().is_empty()
    }, "单链全齐完整率满格");
    s.add("缺段含空用例拒绝", b.record("", SEG_SETUP, 1) == Err(ERR_EMPTY_CASE), "空用例留痕被拒");
    let seal_v = b.seal();
    s.add("封账后拒绝", b.record("ut-tri-d", SEG_SETUP, 1) == Err(ERR_SEALED), "封账后留痕被拒");
    s.add("封账指纹非零", seal_v != 0, "封账折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_chain_passes() {
        let mut b = TraceBook::new();
        let _ = b.record("x", SEG_SETUP, 1);
        let _ = b.record("x", SEG_BODY, 2);
        let _ = b.record("x", SEG_TEARDOWN, 3);
        assert!(b.chain_complete("x"));
        assert_eq!(b.ticks_of("x"), 6);
        assert!(b.incomplete_chains().is_empty());
    }

    #[test]
    fn order_enforced() {
        let mut b = TraceBook::new();
        assert_eq!(b.record("y", SEG_TEARDOWN, 1), Err(ERR_SEG_ORDER));
        let _ = b.record("y", SEG_SETUP, 1);
        assert_eq!(b.record("y", SEG_TEARDOWN, 1), Err(ERR_SEG_ORDER));
        let _ = b.record("y", SEG_BODY, 1);
        assert!(b.record("y", SEG_TEARDOWN, 1).is_ok());
    }

    #[test]
    fn breakdown_and_heaviest() {
        let mut b = TraceBook::new();
        let _ = b.record("p", SEG_SETUP, 2);
        let _ = b.record("p", SEG_BODY, 30);
        let _ = b.record("p", SEG_TEARDOWN, 4);
        let _ = b.record("q", SEG_SETUP, 1);
        let _ = b.record("q", SEG_BODY, 12);
        let _ = b.record("q", SEG_TEARDOWN, 2);
        assert_eq!(b.tick_breakdown("q"), [1, 12, 2]);
        assert_eq!(b.heaviest_body(), Some(("p".to_string(), 30)));
        assert_eq!(b.complete_ratio_bps(), 10000);
        assert_eq!(b.total_ticks(), 51);
    }

    #[test]
    fn unknown_seg_and_empty_case() {
        let mut b = TraceBook::new();
        assert_eq!(b.record("k", "mystery", 1), Err(ERR_EMPTY_SEG));
        assert_eq!(b.record("", SEG_SETUP, 1), Err(ERR_EMPTY_CASE));
        assert_eq!(b.note_count(), 0);
        assert_eq!(b.total_ticks(), 0);
    }
}
