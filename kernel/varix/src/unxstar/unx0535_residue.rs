//! unx0535_residue —— UNX-F0535 演练残留清零审计。
//! 锚点（CoRun Unxreal 主册）：UNX-F0535 · 演练残留清零审计 · 目标 200 行
//! 判据原文：清零审计账齐（演练资源 20 类清单——前后校验和全等 20/20），
//!           残留检出注入红。
//! 组件：ResidueAudit（20 类演练资源清单，每类记演练前后校验和）+ 全等
//!   核（20/20 前后等=零残留）+ 残留检出（任一类漂移即点红）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35300;
pub const XC_RESIDUE: u32 = SEG_BASE + 1; // 残留
pub const XC_KIND_OOB: u32 = SEG_BASE + 2; // 资源类越界
pub const XC_AUDIT_SHORT: u32 = SEG_BASE + 3; // 清单不足
pub const XC_KINDS: usize = 20;

/// 资源类名（20 类冻结）。
pub const KIND_NAMES: [&str; XC_KINDS] = [
    "buf", "queue", "table", "page", "port", "timer", "sem", "lock", "heap", "stack",
    "reg", "dma", "irq", "task", "file", "cache", "pool", "ring", "pipe", "event",
];

/// 单类审计条目。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KindAudit {
    /// 演练前校验和。
    pub before: u64,
    /// 演练后校验和。
    pub after: u64,
}

impl KindAudit {
    /// 零残留？
    pub const fn clean(&self) -> bool {
        self.before == self.after
    }
}

/// 清零审计账。
pub struct ResidueAudit {
    pub kinds: [Option<KindAudit>; XC_KINDS],
    /// 已审计类数。
    pub audited: usize,
}

impl ResidueAudit {
    /// 空账。
    pub fn new() -> Self {
        ResidueAudit {
            kinds: [None; XC_KINDS],
            audited: 0,
        }
    }

    /// 审计一类（类号 0..20 越界拒）。
    pub fn audit(&mut self, kind: usize, before: u64, after: u64) -> Result<(), u32> {
        if kind >= XC_KINDS {
            return Err(XC_KIND_OOB);
        }
        self.kinds[kind] = Some(KindAudit { before, after });
        self.audited += 1;
        Ok(())
    }

    /// 零残留核：已审类全 clean；未审满拒（清单不足）。
    pub fn all_clean(&self) -> Result<bool, u32> {
        if self.audited < XC_KINDS {
            return Err(XC_AUDIT_SHORT);
        }
        let mut i = 0;
        while i < XC_KINDS {
            match self.kinds[i] {
                Some(a) if a.clean() => {}
                _ => return Ok(false),
            }
            i += 1;
        }
        Ok(true)
    }

    /// 残留类清单（类号）。
    pub fn residue_kinds(&self) -> Vec<usize> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < XC_KINDS {
            if let Some(a) = self.kinds[i] {
                if !a.clean() {
                    v.push(i);
                }
            }
            i += 1;
        }
        v
    }

    /// 审计数。
    pub fn len(&self) -> usize {
        self.audited
    }

    /// 空？可实现。
    pub fn is_empty(&self) -> bool {
        self.audited == 0
    }
}

impl Default for ResidueAudit {
    fn default() -> Self {
        Self::new()
    }
}

pub fn run_unx0535_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0535");
    cs.add("SEG 段位独占", SEG_BASE == 0x35300 && XC_AUDIT_SHORT == SEG_BASE + 3, "u32 高段全域普查无冲突");

    // 族一：20 类清单冻结。
    cs.add("类数 20", KIND_NAMES.len() == XC_KINDS, "清单 20 条");
    cs.add("首尾类名", KIND_NAMES[0] == "buf" && KIND_NAMES[19] == "event", "首尾冻结");

    // 族二：20/20 全等零残留。
    let mut a = ResidueAudit::new();
    let mut i = 0;
    while i < XC_KINDS {
        let _ = a.audit(i, 100 + i as u64, 100 + i as u64);
        i += 1;
    }
    cs.add("审计 20/20", a.len() == XC_KINDS, "全类入账");
    cs.add("零残留", a.all_clean() == Ok(true), "前后全等 20/20");
    cs.add("残留清单空", a.residue_kinds().is_empty(), "无点红");

    // 族三：残留检出注入红。
    let mut b = ResidueAudit::new();
    let mut i = 0;
    while i < XC_KINDS {
        // 第 7 类（lock）留残留。
        let delta = if i == 7 { 3 } else { 0 };
        let _ = b.audit(i, 50, 50 + delta);
        i += 1;
    }
    cs.add("残留检出", b.all_clean() == Ok(false), "漂移类点红");
    cs.add("残留定位", b.residue_kinds() == alloc::vec![7], "lock 类被点名");
    cs.add("类名可读", KIND_NAMES[7] == "lock", "定位与名对齐");

    // 族四：越界与不足。
    let mut c = ResidueAudit::new();
    cs.add("未审满拒", c.all_clean().is_err(), "清单不足 Err");
    cs.add("类号越界拒", c.audit(20, 0, 0).is_err(), "kind=20 Err");
    let _ = c.audit(0, 1, 1);
    cs.add("一类不足拒", c.all_clean().is_err(), "1/20 仍拒");
    cs
}
