//! veb269_nvlimit —— VE-F0269 NVIDIA 闭源限制清单。
//! 锚点（CoRun Unxreal 主册）：VE-F0269 · NVIDIA 闭源限制清单 · 目标 340 行
//! 判据原文：VE-F0269-J1 闭源驱动限制条目账：条目（限制项 +
//!           影响面 + 规避等级：不可/可绕/已绕）；路径决策联
//!           动（存在「不可」级限制的路径段 → 该段标灰）；规
//!           避核（可绕条目须带规避手段代号）；清单完整性核
//!           （灰段数 = 不可条目数）；条目去重。
//! 组件：限制条目账 + 灰段映射 + 规避账。
//! 职责定位：闭源限制清单是「NVIDIA 通路的雷区图」——闭源驱
//!           动不给文档的地雷（重映射 BAR、私有一键超频、固件
//!           签名链）要一条条记下来；「不可」级限制直接把路径
//!           段标灰：绕不过去的雷不许人走；规避要留代号：怎么
//!           绕的必须可追溯。
//! 性质逐项分解：登记 O(n)（去重）；灰段 O(条目)；核 O(n)。
//! 诊断码段：0x30C40（0x30C41~0x30C49 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const LX_VERSION: &str = "veb269_nvlimit-v1";

/// 诊断段基址（0x30C40 段 · 本模块独占）。
pub const LX_SEG_BASE: u32 = 0x30C40;
pub const XC_DUP: u32 = 0x30C41; // 条目重
pub const XC_NO_WAY: u32 = 0x30C42; // 可绕无手段
pub const XC_MAGIC: u32 = 0x30C43; // 账魔数破
pub const XC_LV_OOR: u32 = 0x30C44; // 等级越域
pub const XC_NOENT: u32 = 0x30C45; // 无此条目
pub const XC_GRAY: u32 = 0x30C46; // 灰段拦路
pub const XC_LEDGER: u32 = 0x30C47; // 清单不平

/// 规避等级。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Lv {
    /// 不可绕（雷）。
    Hard = 0,
    /// 可绕。
    Bypass = 1,
    /// 已绕（带手段代号）。
    Bypassed = 2,
}

/// 限制条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LimitItem {
    pub name: &'static str,
    /// 影响路径段号。
    pub seg: u32,
    pub lv: Lv,
    /// 规避手段代号（可绕/已绕须非空）。
    pub way: Option<&'static str>,
}

/// 限制账。
pub struct LimitBook {
    items: Vec<LimitItem>,
    magic: u32,
}

impl LimitBook {
    /// 建。
    pub fn new() -> LimitBook {
        LimitBook { items: Vec::new(), magic: 0x0C40_1A07 }
    }

    /// 登记。
    pub fn add(&mut self, name: &'static str, seg: u32, lv: Lv, way: Option<&'static str>) -> Result<(), u32> {
        if self.magic != 0x0C40_1A07 {
            return Err(XC_MAGIC);
        }
        for it in self.items.iter() {
            if it.name == name {
                return Err(XC_DUP);
            }
        }
        match lv {
            Lv::Hard => {
                if way.is_some() {
                    return Err(XC_LV_OOR); // 不可绕不带手段
                }
            }
            _ => {
                match way {
                    Some(w) if !w.is_empty() => {}
                    _ => return Err(XC_NO_WAY),
                }
            }
        }
        self.items.push(LimitItem { name, seg, lv, way });
        Ok(())
    }

    /// 灰段核：段内存在 Hard 条目 → 段灰（路径禁行）。
    pub fn segment_open(&self, seg: u32) -> Result<bool, u32> {
        for it in self.items.iter() {
            if it.seg == seg && it.lv == Lv::Hard {
                return Err(XC_GRAY);
            }
        }
        Ok(true)
    }

    /// 清单完整性核：灰段数 = Hard 条目覆盖段数。
    pub fn audit(&self) -> Result<u32, u32> {
        if self.magic != 0x0C40_1A07 {
            return Err(XC_MAGIC);
        }
        // 收 Hard 段集合。
        let mut hard_segs: Vec<u32> = Vec::new();
        for it in self.items.iter() {
            if it.lv == Lv::Hard && !hard_segs.contains(&it.seg) {
                hard_segs.push(it.seg);
            }
        }
        // 规避账：Bypassed 必有手段。
        for it in self.items.iter() {
            if it.lv == Lv::Bypassed && it.way.is_none() {
                return Err(XC_NO_WAY);
            }
        }
        Ok(hard_segs.len() as u32)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 按段计。
    pub fn count_seg(&self, seg: u32) -> u32 {
        let mut n = 0u32;
        for it in self.items.iter() {
            if it.seg == seg {
                n += 1;
            }
        }
        n
    }

    /// 升级为已绕。
    pub fn bypass(&mut self, name: &str, way: &'static str) -> Result<(), u32> {
        if way.is_empty() {
            return Err(XC_NO_WAY);
        }
        for it in self.items.iter_mut() {
            if it.name == name {
                if it.lv == Lv::Hard {
                    return Err(XC_GRAY); // 不可绕不许转
                }
                it.lv = Lv::Bypassed;
                it.way = Some(way);
                return Ok(());
            }
        }
        Err(XC_NOENT)
    }
}

pub fn run_veb269_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0269");
    // 族一：登记。
    let mut b = LimitBook::new();
    let _ = b.add("rebar-hidden", 3, Lv::Hard, None);
    let _ = b.add("oc-priv", 5, Lv::Bypass, Some("NVML-PROXY"));
    let _ = b.add("fw-chain", 3, Lv::Hard, None);
    let _ = b.add("fan-ioctl", 7, Lv::Bypassed, Some("SYSFS-EMUL"));
    s.add("条目 4", b.len() == 4, "四雷");
    s.add("重名拒", b.add("oc-priv", 5, Lv::Bypass, Some("X")) == Err(XC_DUP), "主键");
    s.add("可绕无手段拒", b.add("x", 9, Lv::Bypass, None) == Err(XC_NO_WAY), "空手");
    s.add("不可绕带手段拒", b.add("y", 9, Lv::Hard, Some("X")) == Err(XC_LV_OOR), "矛盾");
    // 族二：灰段。
    s.add("段 3 灰", b.segment_open(3) == Err(XC_GRAY), "双雷段");
    s.add("段 5 通", b.segment_open(5) == Ok(true), "可绕段放行");
    s.add("段 9 通", b.segment_open(9) == Ok(true), "无雷段");
    s.add("段 3 计 2", b.count_seg(3) == 2, "两雷");
    // 族三：完整性核。
    s.add("灰段 1", b.audit() == Ok(1), "仅段 3");
    // 族四：升级为已绕。
    s.add("升级过", b.bypass("oc-priv", "NVML-DIRECT").is_ok(), "已绕");
    s.add("不可绕升级拒", b.bypass("rebar-hidden", "X") == Err(XC_GRAY), "雷不转");
    s.add("野名拒", b.bypass("ghost", "X") == Err(XC_NOENT), "无条目");
    s.add("空手段拒", b.bypass("fan-ioctl", "") == Err(XC_NO_WAY), "空代号");
    // 族五：全可绕清单 → 灰段 0。
    let mut c = LimitBook::new();
    let _ = c.add("a", 1, Lv::Bypass, Some("W1"));
    let _ = c.add("d", 2, Lv::Bypassed, Some("W2"));
    s.add("无灰段", c.audit() == Ok(0), "全通");
    s.add("全段通", c.segment_open(1) == Ok(true) && c.segment_open(2) == Ok(true), "畅行");
    s.add("魔数核", LX_SEG_BASE == 0x30C40, "段基址");
    s
}
