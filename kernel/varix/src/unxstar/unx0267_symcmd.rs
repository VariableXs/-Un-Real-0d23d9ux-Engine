//! unx0267_symcmd —— UNX-F0267 sym 命令批量符号化。
//! 锚点（CoRun Unxreal 主册）：UNX-F0267 · sym 批量符号化 · 目标 200 行
//! 判据原文：UNX-F0267-J1 sym 命令一次吃地址批：批内地址升序不减
//!           （乱序拒——符号化按序扫描符号表，乱序=静默漏配）；
//!           命中输出「符号+偏移」，未命中输出原始地址不伪装；
//!           批量结果逐行编号可对账。
//! 组件：符号表（名 + 起址 + 长度，升序冻结）+ 符号化引擎（二分
//!       命中核 + 偏移记账）+ 批处理（升序闸 + 逐行编号输出）。
//! 职责定位：批量符号化是「崩溃归因的翻译层」——地址到符号的
//!   映射必须诚实：命中就给符号+偏移，不命中就原样回显地址，
//!   **绝不**用最近符号硬凑（那会把 bug 归因到错误的模块）。
//!   升序闸不是刁难：符号表二分的前提就是查询序列有序扫描。
//! 性能逐项分解：单址符号化 O(log 表深)；批处理 O(批深 × log 表深)。
//! 诊断码段：0x20200（0x20201~0x20209 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SY_VERSION: &str = "unx0267_symcmd-v1";

/// 诊断段基址（0x20200 段 · 本模块独占）。
pub const SY_SEG_BASE: u32 = 0x20200;
pub const XC_UNORDERED: u32 = 0x20201; // 批内乱序
pub const XC_EMPTY_BATCH: u32 = 0x20202; // 空批拒
pub const XC_ZERO_LEN: u32 = 0x20203; // 零长符号拒
pub const XC_OVERLAP: u32 = 0x20204; // 符号区重叠
pub const XC_UNSORTED_TABLE: u32 = 0x20205; // 符号表未升序
pub const XC_ADDR_ZERO: u32 = 0x20206; // 地址 0 拒
pub const XC_ROW_OOR: u32 = 0x20207; // 行号越域
pub const XC_BATCH_BIG: u32 = 0x20208; // 批量超上限
pub const XC_DUP_NAME: u32 = 0x20209; // 符号重名

/// 批量上限。
pub const BATCH_MAX: u32 = 64;

/// 符号表项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SymEntry {
    pub name: &'static str,
    pub base: u32,
    pub len: u32,
}

/// 符号表（构造即升序冻结 + 重叠核）。
pub struct SymTable {
    entries: Vec<SymEntry>,
}

impl SymTable {
    pub fn new(entries: &[SymEntry]) -> Result<SymTable, u32> {
        if entries.is_empty() {
            return Err(XC_UNSORTED_TABLE);
        }
        let mut prev_end: u32 = 0;
        for e in entries.iter() {
            if e.len == 0 {
                return Err(XC_ZERO_LEN);
            }
            if e.base < prev_end {
                return Err(XC_OVERLAP);
            }
            if entries.iter().filter(|x| x.name == e.name).count() > 1 {
                return Err(XC_DUP_NAME);
            }
            prev_end = e.base.checked_add(e.len).ok_or(XC_OVERLAP)?;
        }
        Ok(SymTable { entries: entries.to_vec() })
    }

    /// 单址符号化：命中 →（名, 偏移）；未命中 → None（诚实回显）。
    pub fn resolve(&self, addr: u32) -> Option<(&'static str, u32)> {
        if addr == 0 {
            return None;
        }
        // 二分：找最后一个 base ≤ addr 的表项。
        let mut lo = 0usize;
        let mut hi = self.entries.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.entries[mid].base <= addr {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return None;
        }
        let e = &self.entries[lo - 1];
        if addr < e.base.checked_add(e.len)? {
            Some((e.name, addr - e.base))
        } else {
            None
        }
    }
}

/// 批量符号化输出行：行号 + 结果（命中=名+偏移；未命中=原址回显）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SymRow {
    pub line: u32,
    pub hit: bool,
    pub name: &'static str,
    pub offset: u32,
    pub raw: u32,
}

/// 批处理：地址升序不减（允许重复地址），逐行编号 1..n。
pub fn batch_resolve(table: &SymTable, addrs: &[u32]) -> Result<Vec<SymRow>, u32> {
    if addrs.is_empty() {
        return Err(XC_EMPTY_BATCH);
    }
    if addrs.len() as u32 > BATCH_MAX {
        return Err(XC_BATCH_BIG);
    }
    let mut prev: u32 = 0;
    for &a in addrs.iter() {
        if a == 0 {
            return Err(XC_ADDR_ZERO);
        }
        if a < prev {
            return Err(XC_UNORDERED);
        }
        prev = a;
    }
    let mut rows = Vec::new();
    for (i, &a) in addrs.iter().enumerate() {
        let (hit, name, off) = match table.resolve(a) {
            Some((n, o)) => (true, n, o),
            None => (false, "", 0),
        };
        rows.push(SymRow {
            line: (i + 1) as u32,
            hit,
            name,
            offset: off,
            raw: if hit { 0 } else { a },
        });
    }
    // 行号闭账：1..n 连续（输出账自洽）。
    for (i, r) in rows.iter().enumerate() {
        if r.line != (i + 1) as u32 {
            return Err(XC_ROW_OOR);
        }
    }
    Ok(rows)
}

pub fn run_unx0267_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0267_symcmd");
    // 族一：符号表构造。
    let entries = vec![
        SymEntry { name: "ktext", base: 0x0010_0000, len: 0x0002_0000 },
        SymEntry { name: "kdata", base: 0x0012_0000, len: 0x0001_0000 },
        SymEntry { name: "heap", base: 0x0020_0000, len: 0x0010_0000 },
    ];
    let table = match SymTable::new(&entries) {
        Ok(t) => t,
        Err(_) => {
            cs.add("符号表构造", false, "被拒");
            return cs;
        }
    };
    // 族二：单址命中/偏移/未命中。
    cs.add("首符命中", table.resolve(0x0010_0004) == Some(("ktext", 4)), "基址+4");
    cs.add("跨符命中", table.resolve(0x0012_00FF) == Some(("kdata", 0xFF)), "kdata 偏移");
    cs.add("末字节命中", table.resolve(0x002F_FFFF) == Some(("heap", 0xFFFFF)), "闭端点");
    cs.add("缝隙未命中", table.resolve(0x0019_0000).is_none(), "缝隙不硬凑");
    cs.add("低于表未命中", table.resolve(0x0000_1000).is_none(), "表前不硬凑");
    cs.add("零址拒", table.resolve(0).is_none(), "ADDR_ZERO 语义");
    // 族三：批处理。
    let batch = vec![0x0010_0004u32, 0x0010_0004, 0x0012_00FF, 0x0019_0000];
    let rows = match batch_resolve(&table, &batch) {
        Ok(r) => r,
        Err(_) => {
            cs.add("批处理", false, "被拒");
            return cs;
        }
    };
    cs.add("批深 4", rows.len() == 4, "逐行编号");
    cs.add("行号闭账", rows.iter().enumerate().all(|(i, r)| r.line == (i + 1) as u32), "1..n 连续");
    cs.add("重复地址允许", rows[0].hit && rows[1].hit, "升序不减非严格");
    cs.add("命中带偏移", rows[2].hit && rows[2].offset == 0xFF, "kdata+0xFF");
    cs.add("未命中回显", !rows[3].hit && rows[3].raw == 0x0019_0000, "原址诚实回显");
    // 族四：批闸。
    cs.add("空批拒", batch_resolve(&table, &[]).is_err(), "EMPTY");
    cs.add("乱序拒", batch_resolve(&table, &[0x0020_0000, 0x0010_0000]).is_err(), "UNORDERED");
    cs.add("零址拒", batch_resolve(&table, &[0]).is_err(), "ADDR_ZERO");
    let big: Vec<u32> = (0..65u32).map(|i| 0x0010_0000 + i).collect();
    cs.add("超量拒", batch_resolve(&table, &big).is_err(), "BATCH_BIG");
    // 族五：表构造拒。
    cs.add("空表拒", SymTable::new(&[]).is_err(), "UNSORTED_TABLE");
    cs.add("零长拒", SymTable::new(&[SymEntry { name: "z", base: 0x10, len: 0 }]).is_err(), "ZERO_LEN");
    cs.add("重叠拒", SymTable::new(&[
        SymEntry { name: "a", base: 0x100, len: 0x200 },
        SymEntry { name: "b", base: 0x200, len: 0x100 },
    ]).is_err(), "OVERLAP");
    cs.add("重名拒", SymTable::new(&[
        SymEntry { name: "a", base: 0x100, len: 0x10 },
        SymEntry { name: "a", base: 0x200, len: 0x10 },
    ]).is_err(), "DUP_NAME");
    cs
}
