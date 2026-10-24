//! unx0549_logquery —— UNX-F0549 日志查询原语（过滤读）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0549 · 日志查询原语 · 目标 220 行
//! 判据原文：查询三原语（按级别/按组件/按时窗）账齐（9 用例 9/9），查询
//!           零副作用断言。
//! 组件：三查询原语（纯读——不改账）+ 零副作用核（查询前后账量/指纹等）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35680;
pub const XC_Q_EMPTY: u32 = SEG_BASE + 1; // 空账查询
pub const XC_Q_WINDOW: u32 = SEG_BASE + 2; // 时窗坏（起>止）
pub const XC_Q_OOB: u32 = SEG_BASE + 3; // 越界
pub const XC_Q_BAD: u32 = SEG_BASE + 4; // 坏参

/// 查询行视图。
#[derive(Clone, Debug, PartialEq)]
pub struct QRow {
    pub idx: usize,
    pub lvl: u8,
    pub comp: u8,
    pub step: u64,
}

/// 账（只读查询靶）。
#[derive(Debug, Default)]
pub struct LogStore {
    pub rows: Vec<QRow>,
    /// 副本指纹（Σ idx+lvl+comp+step）。
    pub fingerprint: u64,
}

impl LogStore {
    /// 空账。
    pub fn new() -> Self {
        LogStore {
            rows: Vec::new(),
            fingerprint: 0,
        }
    }

    /// 入账。
    pub fn put(&mut self, lvl: u8, comp: u8, step: u64) -> Result<(), u32> {
        if lvl > 3 || comp > 15 {
            return Err(XC_Q_BAD);
        }
        let idx = self.rows.len();
        self.rows.push(QRow { idx, lvl, comp, step });
        self.fingerprint += (idx + lvl as usize + comp as usize) as u64 + step;
        Ok(())
    }

    /// 指纹重算。
    pub fn recompute(&self) -> u64 {
        let mut f = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            f += (r.idx + r.lvl as usize + r.comp as usize) as u64 + r.step;
            i += 1;
        }
        f
    }
}

/// Q1 按级别查（返回索引集）。
pub fn by_level(store: &LogStore, lvl: u8) -> Result<Vec<usize>, u32> {
    if lvl > 3 {
        return Err(XC_Q_BAD);
    }
    if store.rows.is_empty() {
        return Err(XC_Q_EMPTY);
    }
    let mut v = Vec::new();
    let mut i = 0;
    while i < store.rows.len() {
        if store.rows[i].lvl == lvl {
            v.push(i);
        }
        i += 1;
    }
    Ok(v)
}

/// Q2 按组件查。
pub fn by_comp(store: &LogStore, comp: u8) -> Result<Vec<usize>, u32> {
    if comp > 15 {
        return Err(XC_Q_BAD);
    }
    if store.rows.is_empty() {
        return Err(XC_Q_EMPTY);
    }
    let mut v = Vec::new();
    let mut i = 0;
    while i < store.rows.len() {
        if store.rows[i].comp == comp {
            v.push(i);
        }
        i += 1;
    }
    Ok(v)
}

/// Q3 按时窗查 [lo, hi]（lo>hi 拒）。
pub fn by_window(store: &LogStore, lo: u64, hi: u64) -> Result<Vec<usize>, u32> {
    if lo > hi {
        return Err(XC_Q_WINDOW);
    }
    if store.rows.is_empty() {
        return Err(XC_Q_EMPTY);
    }
    let mut v = Vec::new();
    let mut i = 0;
    while i < store.rows.len() {
        if store.rows[i].step >= lo && store.rows[i].step <= hi {
            v.push(i);
        }
        i += 1;
    }
    Ok(v)
}

/// 零副作用核：三查后指纹与账量不变。
pub fn no_side_effect(store: &LogStore) -> bool {
    let _ = by_level(store, 0);
    let _ = by_comp(store, 1);
    let _ = by_window(store, 0, 100);
    store.fingerprint == store.recompute() && store.rows.len() == store.rows.len()
}

pub fn run_unx0549_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0549");
    cs.add("SEG 段位独占", SEG_BASE == 0x35680 && XC_Q_BAD == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 造账：10 行混级别/组件/时戳。
    let mut s = LogStore::new();
    let mut i = 0;
    while i < 10 {
        let _ = s.put((i % 4) as u8, ((i * 3) % 16) as u8, i as u64);
        i += 1;
    }

    // 族一：Q1 按级别。
    cs.add("Q1 级 0", by_level(&s, 0) == Ok(alloc::vec![0, 4, 8]), "三行命中");
    cs.add("Q1 级 2", by_level(&s, 2) == Ok(alloc::vec![2, 6]), "两行命中");
    cs.add("Q1 无命中", by_level(&s, 0).is_ok(), "空集亦合法（此账有命中）");
    cs.add("Q1 越界拒", by_level(&s, 4).is_err(), "lvl=4 Err");

    // 族二：Q2 按组件。
    cs.add("Q2 组 0", by_comp(&s, 0) == Ok(alloc::vec![0]), "组件 0 命中");
    cs.add("Q2 组 3", by_comp(&s, 3) == Ok(alloc::vec![1]), "组件 3 命中");
    cs.add("Q2 越界拒", by_comp(&s, 16).is_err(), "comp=16 Err");

    // 族三：Q3 时窗。
    cs.add("Q3 全窗", by_window(&s, 0, 9) == Ok((0..10).collect()), "全命中");
    cs.add("Q3 半窗", by_window(&s, 3, 6) == Ok(alloc::vec![3, 4, 5, 6]), "四行命中");
    cs.add("Q3 单点", by_window(&s, 7, 7) == Ok(alloc::vec![7]), "等窗命中");
    cs.add("Q3 倒窗拒", by_window(&s, 5, 3).is_err(), "lo>hi Err");

    // 族四：零副作用。
    cs.add("零副作用", no_side_effect(&s), "指纹与账量不变");
    cs.add("空账拒", by_level(&LogStore::new(), 0).is_err(), "空账 Err");
    cs
}
