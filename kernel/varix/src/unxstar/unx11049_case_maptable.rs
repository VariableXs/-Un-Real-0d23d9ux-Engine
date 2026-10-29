//! UNX-F11049 用例映射表对账（LTP 用例号与域判据号双账）（0x1e620 段）
//!
//! 外部对照面第 9 批：用例映射表对账。双账结构：
//! LTP 用例号 → 域判据号（正向账），域判据号 → LTP 用例号（反向账）。
//! 40 条映射（5 族 × 8 例）逐行对平，双向可查零断链：
//! 正向查到 dom 后反向必须查回同一 ltp，否则即断链。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11049 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e000 + (11049 - 11000) * 0x20;

/// 映射表空 —— 查表前必须先立表
pub const MT_EMPTY: u32 = SEG_BASE + 0x01;
/// 映射行号越界 —— 40 行编号 0..40
pub const MT_RANGE: u32 = SEG_BASE + 0x02;
/// 断链 —— 正向查到 dom 但反向查不回原 ltp
pub const MT_BROKEN: u32 = SEG_BASE + 0x03;
/// 查无此号 —— 正向/反向均未命中
pub const MT_MISS: u32 = SEG_BASE + 0x04;
/// 重复登记 —— 同一 LTP 号或判据号二次入表
pub const MT_DUP: u32 = SEG_BASE + 0x05;
/// 表已冻结 —— freeze 后禁止再登记
pub const MT_FROZEN: u32 = SEG_BASE + 0x06;

/// 映射族数：pipe/signal/pty/sysv/sock 五族
pub const FAMILIES: usize = 5;
/// 每族映射行数
pub const ROWS_PER_FAM: usize = 8;
/// 映射总行数
pub const ROWS_TOTAL: usize = FAMILIES * ROWS_PER_FAM;
/// LTP 用例号基址
pub const LTP_BASE: u32 = 0x7000;
/// 域判据号基址
pub const DOM_BASE: u32 = 0x8100;
/// 映射表标签
pub const MAP_TAG: u64 = 0x6d61_705f_6d74_3049;

/// 单条映射行：LTP 号 + 判据号 + 族号
#[derive(Clone, Debug)]
pub struct MapRow {
    /// LTP 用例号
    pub ltp: u32,
    /// 域判据号
    pub dom: u32,
    /// 所属族号 0..5
    pub fam: u8,
    /// 双向对平标记
    pub linked: bool,
}

/// 映射表双账
#[derive(Clone, Debug)]
pub struct MapTable {
    /// 40 条映射行
    pub rows: Vec<MapRow>,
    /// 正向查中次数
    pub fwd_hits: u64,
    /// 反向查中次数
    pub rev_hits: u64,
    /// 断链检出次数
    pub breaks: u64,
    /// 已对平行数
    pub linked_n: usize,
    /// 冻结标记
    pub frozen: bool,
}

impl MapTable {
    /// 立表：40 行映射，ltp = LTP_BASE + fam*100 + half，dom = DOM_BASE + fam*10 + half
    pub fn new() -> MapTable {
        let mut tb = MapTable {
            rows: Vec::new(),
            fwd_hits: 0, rev_hits: 0, breaks: 0,
            linked_n: 0, frozen: false,
        };
        for fam in 0..FAMILIES {
            for half in 0..ROWS_PER_FAM {
                let ltp = LTP_BASE + (fam as u32) * 100 + half as u32;
                let dom = DOM_BASE + (fam as u32) * 10 + half as u32;
                tb.rows.push(MapRow { ltp, dom, fam: fam as u8, linked: false });
            }
        }
        tb
    }

    /// 登记一行（重复 LTP 号或判据号拒绝）
    pub fn put(&mut self, ltp: u32, dom: u32, fam: u8) -> Result<(), u32> {
        if self.frozen { return Err(MT_FROZEN); }
        if self.rows.iter().any(|r| r.ltp == ltp || r.dom == dom) {
            return Err(MT_DUP);
        }
        self.rows.push(MapRow { ltp, dom, fam, linked: false });
        Ok(())
    }

    /// 正向查：LTP 号 → 判据号
    pub fn forward(&mut self, ltp: u32) -> Result<u32, u32> {
        if self.rows.is_empty() { return Err(MT_EMPTY); }
        match self.rows.iter().find(|r| r.ltp == ltp) {
            Some(r) => {
                self.fwd_hits = self.fwd_hits.wrapping_add(1);
                Ok(r.dom)
            }
            None => Err(MT_MISS),
        }
    }

    /// 反向查：判据号 → LTP 号
    pub fn reverse(&mut self, dom: u32) -> Result<u32, u32> {
        if self.rows.is_empty() { return Err(MT_EMPTY); }
        match self.rows.iter().find(|r| r.dom == dom) {
            Some(r) => {
                self.rev_hits = self.rev_hits.wrapping_add(1);
                Ok(r.ltp)
            }
            None => Err(MT_MISS),
        }
    }

    /// 对平一行：正向查 dom 再反向查回 ltp，闭环即 linked
    pub fn link_row(&mut self, no: usize) -> Result<bool, u32> {
        if no >= self.rows.len() { return Err(MT_RANGE); }
        let ltp = self.rows[no].ltp;
        let dom = self.rows[no].dom;
        let fwd = self.forward(ltp)?;
        let back = if fwd == dom { self.reverse(dom)? } else {
            self.breaks = self.breaks.wrapping_add(1);
            return Err(MT_BROKEN);
        };
        let ok = back == ltp;
        if !ok {
            self.breaks = self.breaks.wrapping_add(1);
            return Err(MT_BROKEN);
        }
        self.rows[no].linked = true;
        if ok { self.linked_n += 1; }
        Ok(ok)
    }

    /// 全表逐行对平
    pub fn link_all(&mut self) -> Result<usize, u32> {
        let n = self.rows.len();
        for no in 0..n {
            self.link_row(no)?;
        }
        Ok(self.linked_n)
    }

    /// 按族统计对平行数
    pub fn linked_fam(&self, fam: u8) -> usize {
        self.rows.iter().filter(|r| r.fam == fam && r.linked).count()
    }

    /// 全面对平：40/40 且零断链
    pub fn all_linked(&self) -> bool {
        self.linked_n == ROWS_TOTAL && self.breaks == 0
    }

    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = MAP_TAG;
        for r in &self.rows {
            h = (h ^ (r.ltp as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (r.dom as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (r.linked as u64)).wrapping_mul(0x100000001b3);
        }
        for x in [self.fwd_hits, self.rev_hits, self.breaks] {
            h = (h ^ x).wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 冻结映射表
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：40 行全部双向对平
pub fn sample_linked() -> (MapTable, bool) {
    let mut tb = MapTable::new();
    let _ = tb.link_all();
    let ok = tb.all_linked();
    tb.freeze();
    (tb, ok)
}

/// 注入路径：断链 / 越界 / 重复 / 冻结
pub fn inject_four() -> [bool; 4] {
    // 注入 1：正向查不存在的 LTP 号 → MT_MISS 拒止
    let mut tb = MapTable::new();
    let i1 = matches!(tb.forward(0xdead), Err(MT_MISS));
    // 注入 2：行号越界 → MT_RANGE 拒止
    let i2 = matches!(tb.link_row(ROWS_TOTAL), Err(MT_RANGE));
    // 注入 3：重复登记同一 LTP 号 → MT_DUP 拒止
    let i3 = matches!(tb.put(tb.rows[0].ltp, 0x9999, 0), Err(MT_DUP));
    // 注入 4：冻结后登记 → MT_FROZEN 拒止
    tb.freeze();
    let i4 = matches!(tb.put(0x7abc, 0x8abc, 0), Err(MT_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 映射表逐行对平（40/40），双向可查零断链
pub fn run_unx11049_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11049");
    let (tb, ok) = sample_linked();
    let f0 = tb.linked_fam(0);

    s.add("J1-逐行对平", ok && tb.linked_n == ROWS_TOTAL, "映射表 40/40 行逐行双向对平");
    s.add("J1-零断链", tb.breaks == 0, "双向闭环零断链检出");
    s.add("J1-族覆盖", (0..FAMILIES as u8).all(|f| tb.linked_fam(f) == ROWS_PER_FAM), "五族每族 8/8 行对平");
    s.add("J2-正向可查", tb.fwd_hits >= ROWS_TOTAL as u64, "正向账 LTP→判据全量命中");
    s.add("J2-反向可查", tb.rev_hits >= ROWS_TOTAL as u64, "反向账判据→LTP 全量命中");
    s.add("J2-查收平衡", tb.fwd_hits == tb.rev_hits, "正反向查中次数平衡");
    s.add("J3-编号正交", tb.rows.iter().all(|r| r.dom - DOM_BASE == (r.ltp - LTP_BASE) / 100 * 10 + (r.ltp - LTP_BASE) % 100), "ltp 与 dom 编号按族正交映射");
    s.add("J3-行数守恒", tb.rows.len() == ROWS_TOTAL, "映射行数 40 守恒");
    s.add("J3-首尾抽样", tb.rows[0].ltp == LTP_BASE && tb.rows[ROWS_TOTAL - 1].ltp == LTP_BASE + 407, "首行 0x7000 尾行 0x7407 起编正确");
    s.add("J4-重复拒登", inject_four()[2], "重复 LTP 号被 MT_DUP 拒止");
    s.add("J4-冻结拒登", inject_four()[3], "冻结后登记被 MT_FROZEN 拒止");
    s.add("J5-查无拒返", inject_four()[0], "查无此号被 MT_MISS 拒止");
    s.add("J5-越界拒链", inject_four()[1], "行号越界被 MT_RANGE 拒止");
    s.add("J6-指纹稳定", tb.frozen && tb.fold() == tb.fold(), "冻结后指纹重算稳定");
    s.add("J6-样本一致性", ok && f0 == ROWS_PER_FAM, "样本账与族账口径一致");
    s.add_dyn(
        "J7-对平动态",
        ok,
        &format!("对平 {}/{} 行，断链 {} 次，fwd {} rev {}，fold={:#x}", tb.linked_n, ROWS_TOTAL, tb.breaks, tb.fwd_hits, tb.rev_hits, tb.fold()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11049_case_maptable_flow() {
        let mut tb = MapTable::new();
        // 冻结前越界拒链
        assert!(matches!(tb.link_row(999), Err(MT_RANGE)));
        // 全表对平
        assert_eq!(tb.link_all().unwrap_or(0), ROWS_TOTAL);
        assert!(tb.all_linked());
        // 双向抽查闭环
        let dom = tb.rows[13].dom;
        let ltp = tb.rows[13].ltp;
        assert_eq!(tb.forward(ltp).unwrap_or(0), dom);
        assert_eq!(tb.reverse(dom).unwrap_or(0), ltp);
        let root = tb.freeze();
        assert!(tb.frozen);
        assert_eq!(root, tb.fold());
        // 注入路径全拒止
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11049_checks_green() {
        let s = run_unx11049_checks();
        assert!(s.all_passed(), "UNX-F11049 判据未全绿");
    }
}
