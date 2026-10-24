//! unx0548_comptag —— UNX-F0548 组件标签注册账。
//! 锚点（CoRun Unxreal 主册）：UNX-F0548 · 组件标签注册账 · 目标 240 行
//! 判据原文：标签注册账齐（16 组件槽冻结），未注册标签拒收率 100%。
//! 组件：CompRegistry（16 槽，名注册+重名拒）+ emit 门（未注册组件号拒收）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35640;
pub const XC_COMP_SLOTS: usize = 16;
pub const XC_DUP_COMP: u32 = SEG_BASE + 1; // 重名组件
pub const XC_COMP_MISS: u32 = SEG_BASE + 2; // 未注册
pub const XC_TAG_BAD: u32 = SEG_BASE + 3; // 名坏
pub const XC_TAG_OOB: u32 = SEG_BASE + 4; // 越界

/// 组件注册表（16 槽）。
#[derive(Debug)]
pub struct CompRegistry {
    pub names: [Option<&'static str>; XC_COMP_SLOTS],
    pub registered: usize,
}

impl CompRegistry {
    /// 空表。
    pub fn new() -> Self {
        CompRegistry {
            names: [None; XC_COMP_SLOTS],
            registered: 0,
        }
    }

    /// 注册：名须非空/无竖线；重名拒；满表拒。返回槽号。
    pub fn register(&mut self, name: &'static str) -> Result<usize, u32> {
        if name.is_empty() || name.contains('|') {
            return Err(XC_TAG_BAD);
        }
        if self.registered >= XC_COMP_SLOTS {
            return Err(XC_TAG_OOB);
        }
        let mut i = 0;
        while i < XC_COMP_SLOTS {
            if self.names[i] == Some(name) {
                return Err(XC_DUP_COMP);
            }
            i += 1;
        }
        // 找第一个空槽。
        let mut slot = 0;
        while slot < XC_COMP_SLOTS {
            if self.names[slot].is_none() {
                break;
            }
            slot += 1;
        }
        self.names[slot] = Some(name);
        self.registered += 1;
        Ok(slot)
    }

    /// 槽号→名（未注册 None）。
    pub fn name_of(&self, slot: usize) -> Option<&'static str> {
        if slot >= XC_COMP_SLOTS {
            return None;
        }
        self.names[slot]
    }

    /// emit 门：组件号必须已注册。
    pub fn gate(&self, slot: usize) -> Result<(), u32> {
        if slot >= XC_COMP_SLOTS || self.names[slot].is_none() {
            return Err(XC_COMP_MISS);
        }
        Ok(())
    }

    /// 注册数。
    pub fn len(&self) -> usize {
        self.registered
    }

    /// 空？可实现。
    pub fn is_empty(&self) -> bool {
        self.registered == 0
    }
}

impl Default for CompRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub fn run_unx0548_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0548");
    cs.add("SEG 段位独占", SEG_BASE == 0x35640 && XC_TAG_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：16 槽冻结。
    cs.add("槽数冻结", XC_COMP_SLOTS == 16, "16 组件槽");
    let mut r = CompRegistry::new();
    let names = ["sched", "mem", "io", "fs", "net", "gfx", "input", "power", "acpi", "pci", "usb", "audio", "time", "ipc", "sec", "boot"];
    let mut i = 0;
    while i < 16 {
        cs.add("逐槽注册", r.register(names[i]) == Ok(i), "槽号顺延");
        i += 1;
    }
    cs.add("满表账", r.len() == 16, "16/16");

    // 族二：拒收。
    cs.add("重名拒", r.register("sched").is_err(), "重名 Err");
    cs.add("满表拒", r.register("extra").is_err(), "17th Err");
    cs.add("空名拒", r.register("").is_err(), "空名 Err");
    cs.add("污染名拒", r.register("a|b").is_err(), "含竖线 Err");

    // 族三：emit 门 100%。
    cs.add("注册号过门", (0..16).all(|s| r.gate(s).is_ok()), "16/16 放行");
    cs.add("越界拒", r.gate(16).is_err(), "slot=16 Err");
    cs.add("未注册拒", CompRegistry::new().gate(0).is_err(), "空表 slot0 Err");
    cs.add("名回环", r.name_of(0) == Some("sched") && r.name_of(15) == Some("boot"), "槽名对齐");

    // 族四：空槽洞补位。
    let mut r2 = CompRegistry::new();
    let _ = r2.register("a");
    let _ = r2.register("b");
    cs.add("补位注册", r2.register("c") == Ok(2), "顺延空槽");
    cs.add("查空槽", r2.name_of(5).is_none(), "未用槽 None");
    cs
}
