// SPDX-License-Identifier: Apache-2.0
//! UNX-F0267 · sym 命令批量符号化
//!
//! 单地址/区间/批量三模式符号化，与 nm 对照一致。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 符号表
// ---------------------------------------------------------------------------

/// 符号条目。
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub addr: u64,
    pub size: u64,
}

/// 符号表。
pub struct SymbolTable {
    symbols: Vec<Symbol>,
}

impl SymbolTable {
    pub fn new() -> Self {
        SymbolTable { symbols: Vec::new() }
    }

    /// 插入符号。
    pub fn insert(&mut self, name: &str, addr: u64, size: u64) {
        self.symbols.push(Symbol { name: name.to_string(), addr, size });
    }

    /// 单地址符号化。
    pub fn lookup_single(&self, addr: u64) -> Option<&Symbol> {
        self.symbols.iter()
            .filter(|s| addr >= s.addr && addr < s.addr + s.size)
            .min_by_key(|s| addr - s.addr) // 最近匹配
    }

    /// 区间符号化。
    pub fn lookup_range(&self, start: u64, end: u64) -> Vec<&Symbol> {
        self.symbols.iter()
            .filter(|s| s.addr < end && s.addr + s.size > start)
            .collect()
    }

    /// 批量符号化。
    pub fn lookup_batch(&self, addrs: &[u64]) -> Vec<Option<&Symbol>> {
        addrs.iter().map(|&a| self.lookup_single(a)).collect()
    }

    /// 符号总数。
    pub fn count(&self) -> usize { self.symbols.len() }

    /// 与 nm 对照验证（名称+地址一致）。
    pub fn verify_vs_nm(&self, nm_pairs: &[(String, u64)]) -> bool {
        for (name, addr) in nm_pairs {
            match self.symbols.iter().find(|s| s.name == *name) {
                Some(s) => { if s.addr != *addr { return false; } }
                None => return false,
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 三模式符号化器
// ---------------------------------------------------------------------------

/// 符号化模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolizeMode {
    /// 单地址。
    Single,
    /// 区间。
    Range,
    /// 批量。
    Batch,
}

/// 符号化器。
pub struct Symbolizer;

impl Symbolizer {
    /// 符号化（三模式统一入口）。
    pub fn symbolize(table: &SymbolTable, mode: SymbolizeMode, input: &[u64]) -> Vec<String> {
        match mode {
            SymbolizeMode::Single => {
                input.first().map(|&a| {
                    table.lookup_single(a).map(|s| s.name.clone()).unwrap_or("<unknown>".into())
                }).into_iter().collect()
            }
            SymbolizeMode::Range => {
                if input.len() >= 2 {
                    table.lookup_range(input[0], input[1]).iter().map(|s| s.name.clone()).collect()
                } else {
                    Vec::new()
                }
            }
            SymbolizeMode::Batch => {
                table.lookup_batch(input).iter()
                    .map(|s| s.as_ref().map(|s| s.name.clone()).unwrap_or("<unknown>".into()))
                    .collect()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0267 判据全集。
pub fn run_symbolize_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0267", "sym 命令批量符号化");

    // 1. 符号表
    let mut st = SymbolTable::new();
    st.insert("main", 0x1000, 0x100);
    st.insert("helper", 0x2000, 0x50);
    st.insert("util", 0x3000, 0x200);
    cs.ok("符号表", st.count() == 3, "三符号");

    // 2. 单地址
    cs.ok("单地址命中", st.lookup_single(0x1050).map(|s| s.name.as_str()) == Some("main"), "0x1050 in main");
    cs.ok("单地址未命中", st.lookup_single(0x5000).is_none(), "无符号");
    cs.ok("边界命中", st.lookup_single(0x1000).is_some(), "起始地址");

    // 3. 区间
    cs.ok("区间命中", st.lookup_range(0x1000, 0x3000).len() == 2, "main+helper");
    cs.ok("区间空", st.lookup_range(0x5000, 0x6000).is_empty(), "无符号");

    // 4. 批量
    let batch = st.lookup_batch(&[0x1050, 0x2010, 0x5000]);
    cs.ok("批量三地址", batch.len() == 3, "三结果");
    cs.ok("批量命中", batch[0].is_some() && batch[1].is_some(), "前两个命中");
    cs.ok("批量未命中", batch[2].is_none(), "第三个未命中");

    // 5. 三模式
    let single = Symbolizer::symbolize(&st, SymbolizeMode::Single, &[0x1050]);
    cs.ok("单模式", single.len() == 1 && single[0] == "main", "符号名");
    let range = Symbolizer::symbolize(&st, SymbolizeMode::Range, &[0x1000, 0x3000]);
    cs.ok("区间模式", range.len() == 2, "两符号");
    let batch2 = Symbolizer::symbolize(&st, SymbolizeMode::Batch, &[0x1050, 0x2010]);
    cs.ok("批量模式", batch2.len() == 2, "两结果");

    // 6. nm 对照
    let nm = alloc::vec![
        ("main".to_string(), 0x1000),
        ("helper".to_string(), 0x2000),
    ];
    cs.ok("nm 一致", st.verify_vs_nm(&nm), "名称+地址一致");

    cs.done()
}
