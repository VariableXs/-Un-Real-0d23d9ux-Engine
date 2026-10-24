//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0318 · trampoline 低 1MB 映射预置（B17 前置）· 诊断段 0x3df0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0318-J1 低 1MB 恒等映射在主表与临时表双在位，B17 消费
//!   断言过。
//! 难点分析：
//!   AP 唤醒用的 trampoline 代码住在物理低 1MB。AP 开分页前访问它
//!   靠恒等映射；开分页后内核访问它靠主表映射。两张表任何一张缺
//!   这段映射，启动在「恰好那一步」炸——且 AP 已起飞，崩溃现场带
//!   不回来。双在位断言必须在 BSP 单核阶段就做完（B17 消费前）。
//! 纪律约束：
//!   - 双在位：主表与临时表各自完整覆盖 [0, 1MiB)；
//!   - 恒等语义：虚拟==物理，页内偏移逐项核对；
//!   - 权限正确：这段是可执行代码（NX=0），且只有它 NX=0 的例外
//!     必须显性登记；
//!   - 消费断言：B17 消费接口按 (页数, 权限) 取映射，取不到即拒。
//! 成本记账：预置 O(页数)；双在位断言 O(页数)；消费 O(1)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x3df0;

/// 低区恒等映射页数：1MiB / 4KiB = 256 页。
pub const LOW_PAGES: u32 = 256;

/// 一页映射行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapRow {
    /// 物理帧基址。
    pub phys: u32,
    /// 恒等：虚拟==物理。
    pub identity: bool,
    /// NX 位（false = 可执行）。
    pub nx: bool,
}

/// 一张表的低 1MB 视图。
#[derive(Debug)]
pub struct LowMapTable {
    /// 表名（Main / Temp）。
    pub name: &'static str,
    /// 每页映射行；None = 该页无映射。
    pub rows: Vec<Option<MapRow>>,
}

impl LowMapTable {
    /// 空表。
    pub fn empty(name: &'static str) -> Self {
        LowMapTable { name, rows: vec![None; LOW_PAGES as usize] }
    }

    /// 恒等预置整段（黄金路径）。
    pub fn preset_identity(&mut self, exec: bool) {
        for i in 0..LOW_PAGES as usize {
            self.rows[i] = Some(MapRow { phys: (i as u32) * 4096, identity: true, nx: !exec });
        }
    }

    /// 缺页统计。
    pub fn holes(&self) -> Vec<u32> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.is_none())
            .map(|(i, _)| i as u32)
            .collect()
    }

    /// 恒等完整性：每页有映射且 identity==true 且 phys==i*4096。
    pub fn identity_complete(&self) -> bool {
        self.rows.iter().enumerate().all(|(i, r)| match r {
            Some(m) => m.identity && m.phys == i as u32 * 4096,
            None => false,
        })
    }

    /// NX 一致性：整表要么全可执行要么全 NX（例外登记制——混布即违例）。
    pub fn nx_uniform(&self) -> Option<bool> {
        let mut seen: Option<bool> = None;
        for r in self.rows.iter().flatten() {
            match seen {
                None => seen = Some(r.nx),
                Some(s) if s != r.nx => return None, // 混布
                _ => {}
            }
        }
        seen
    }

    /// B17 消费接口：取第 i 页映射，越界/缺页即拒。
    pub fn consume(&self, page: u32) -> Result<MapRow, &'static str> {
        if page >= LOW_PAGES {
            return Err("页号超出低 1MB");
        }
        match self.rows[page as usize] {
            Some(m) => Ok(m),
            None => Err("低区页无映射（B17 消费失败）"),
        }
    }
}

/// 双表视图（主表 + 临时表）。
#[derive(Debug)]
pub struct DualLowMap {
    pub main: LowMapTable,
    pub temp: LowMapTable,
}

impl DualLowMap {
    /// 黄金预置：双表全恒等、可执行。
    pub fn golden() -> Self {
        let mut main = LowMapTable::empty("Main");
        main.preset_identity(true);
        let mut temp = LowMapTable::empty("Temp");
        temp.preset_identity(true);
        DualLowMap { main, temp }
    }

    /// 双在位断言：两表恒等完整且互相一致。
    pub fn both_present(&self) -> bool {
        self.main.identity_complete() && self.temp.identity_complete()
    }

    /// 逐页一致性：双表对同一物理页的权限必须相同。
    pub fn page_wise_consistent(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for i in 0..LOW_PAGES as usize {
            match (&self.main.rows[i], &self.temp.rows[i]) {
                (Some(a), Some(b)) if a.nx != b.nx => out.push(i as u32),
                (Some(_), None) | (None, Some(_)) => out.push(i as u32),
                _ => {}
            }
        }
        out
    }
}

/// UNX-F0318 判据（J1 双在位 + B17 消费断言）。
pub fn run_unx318_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0318 trampoline 低 1MB 映射预置");

    // J1 主线：黄金双表
    let d = DualLowMap::golden();
    cs.add("J1 主表双在位之主表", d.main.identity_complete(), "256 页恒等");
    cs.add("J1 双在位之临时表", d.temp.identity_complete(), "256 页恒等");
    cs.add("J1 both_present 总断言", d.both_present(), "B17 前置");
    cs.add("J1 逐页权限一致", d.page_wise_consistent().is_empty(), "双表同权限");
    cs.add("J1 NX 均匀（全可执行）", d.main.nx_uniform() == Some(false), "trampoline 代码段");

    // B17 消费断言
    let c0 = d.main.consume(0);
    let c255 = d.main.consume(255);
    cs.add("B17 消费首页", matches!(c0, Ok(m) if m.phys == 0 && m.identity), "phys 0");
    cs.add("B17 消费末页", matches!(c255, Ok(m) if m.phys == 255 * 4096), "phys 0xFF000");
    cs.add("B17 消费越界拒收", d.main.consume(256).is_err(), "1MiB 之外");
    cs.add("B17 消费恒等语义", c0.as_ref().map(|m| m.phys == 0 && m.identity).unwrap_or(false), "virt==phys");

    // 注入 1：临时表缺中段 → 双在位破 + 逐页不一致定位
    let mut bad = DualLowMap::golden();
    for i in 100..110 {
        bad.temp.rows[i] = None;
    }
    cs.add("注入1 双在位破", !bad.both_present(), "temp 10 缺页");
    cs.add("注入1 缺页数精确", bad.temp.holes().len() == 10, "100..109");
    cs.add("注入1 逐页定位", bad.page_wise_consistent() == (100..110).collect::<Vec<u32>>(), "页号清单");

    // 注入 2：恒等破坏（phys 偏移）→ 恒等完整性抓
    let mut bad2 = DualLowMap::golden();
    bad2.main.rows[42] = Some(MapRow { phys: 42 * 4096 + 0x123, identity: false, nx: false });
    cs.add("注入2 恒等破坏检出", !bad2.main.identity_complete(), "phys 偏移");
    cs.add("注入2 双在位随之破", !bad2.both_present(), "完整性含恒等");

    // 注入 3：NX 混布 → 均匀性返回 None
    let mut bad3 = DualLowMap::golden();
    if let Some(m) = bad3.main.rows[7].as_mut() {
        m.nx = true;
    }
    cs.add("注入3 NX 混布检出", bad3.main.nx_uniform().is_none(), "例外须登记");

    // 注入 4：B17 消费缺页页 → 拒收带人话
    cs.add("注入4 消费缺页拒收", matches!(bad.temp.consume(105), Err("低区页无映射（B17 消费失败）")), "显性告知");

    // 常量口径：256 页 == 1MiB
    cs.add("口径 1MiB=256 页", LOW_PAGES == 1048576 / 4096, "4KiB 页");

    cs
}
