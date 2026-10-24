//! veb274_nvwhitelist —— VE-F0274 NVIDIA 白名单规避表。
//! 锚点（CoRun Unxreal 主册）：VE-F0274 · NVIDIA 白名单规避表 · 目标 300 行
//! 判据原文：VE-F0274-J1 白名单规避表：应用白名单（已知要在专
//!           有路径规避的应用清单）；条目（应用签名 + 规避动作
//!           + 生效代际窗）；查询命中（签名精确匹配 + 代际落窗）；
//!           窗外不命中（代际不在生效窗）；条目去重（同签名重
//!           登记 → 更新动作）。
//! 组件：白名单账 + 命中器 + 代际窗。
//! 职责定位：白名单规避表是「NVIDIA 通路的 Known-Issues 实务版
//!           」——某些应用在闭源驱动下有已确认的不兼容（反作
//!           弊冲突、崩溃组合），规避动作（关特性/换路径）按签
//!           名精确打击；代际窗让规避随驱动代际失效——新版修
//!           了就不必再绕。
//! 性能逐项分解：命中 O(条目)；登记 O(条目)。
//! 诊断码段：0x30D80（0x30D81~0x30D89 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const WL_VERSION: &str = "veb274_nvwhitelist-v1";

/// 诊断段基址（0x30D80 段 · 本模块独占）。
pub const WL_SEG_BASE: u32 = 0x30D80;
pub const XC_SIG_EMPTY: u32 = 0x30D81; // 签名空
pub const XC_MAGIC: u32 = 0x30D82; // 账魔数破
pub const XC_WIN: u32 = 0x30D83; // 窗参不合法
pub const XC_ACT_OOR: u32 = 0x30D84; // 动作越域
pub const XC_NOENT: u32 = 0x30D85; // 无此条目

/// 规避动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// 强制走开源路径。
    ForceOpen = 0,
    /// 关闭特性 X。
    DisableFeat = 1,
    /// 降级呈现路径。
    DowngradePresent = 2,
}

impl Act {
    pub fn from_u32(v: u32) -> Option<Act> {
        match v {
            0 => Some(Act::ForceOpen),
            1 => Some(Act::DisableFeat),
            2 => Some(Act::DowngradePresent),
            _ => None,
        }
    }
}

/// 条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WlItem {
    pub sig: &'static str,
    pub act: Act,
    /// 生效代际窗 [lo, hi]（闭）。
    pub lo: u32,
    pub hi: u32,
}

/// 白名单账。
pub struct WlBook {
    items: Vec<WlItem>,
    magic: u32,
}

impl WlBook {
    /// 建。
    pub fn new() -> WlBook {
        WlBook { items: Vec::new(), magic: 0x0D80_417E }
    }

    /// 登记（同签名 → 更新动作与窗）。
    pub fn put(&mut self, sig: &'static str, act: Act, lo: u32, hi: u32) -> Result<(), u32> {
        if self.magic != 0x0D80_417E {
            return Err(XC_MAGIC);
        }
        if sig.is_empty() {
            return Err(XC_SIG_EMPTY);
        }
        if lo == 0 || lo > hi {
            return Err(XC_WIN);
        }
        for it in self.items.iter_mut() {
            if it.sig == sig {
                it.act = act;
                it.lo = lo;
                it.hi = hi;
                return Ok(());
            }
        }
        self.items.push(WlItem { sig, act, lo, hi });
        Ok(())
    }

    /// 命中查询。
    pub fn hit(&self, sig: &str, gen: u32) -> Result<Act, u32> {
        if self.magic != 0x0D80_417E {
            return Err(XC_MAGIC);
        }
        for it in self.items.iter() {
            if it.sig == sig && gen >= it.lo && gen <= it.hi {
                return Ok(it.act);
            }
        }
        Err(XC_NOENT)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 动作统计。
    pub fn count_act(&self, a: Act) -> u32 {
        let mut n = 0u32;
        for it in self.items.iter() {
            if it.act == a {
                n += 1;
            }
        }
        n
    }
}

pub fn run_veb274_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0274");
    // 族一：登记。
    let mut b = WlBook::new();
    let _ = b.put("anticheat-x", Act::ForceOpen, 3, 5);
    let _ = b.put("vr-title-y", Act::DisableFeat, 4, 6);
    let _ = b.put("legacy-z", Act::DowngradePresent, 1, 3);
    s.add("条目 3", b.len() == 3, "三规避");
    s.add("签名空拒", b.put("", Act::ForceOpen, 1, 2) == Err(XC_SIG_EMPTY), "白签");
    s.add("倒窗拒", b.put("w", Act::ForceOpen, 5, 3) == Err(XC_WIN), "lo>hi");
    s.add("零窗拒", b.put("w", Act::ForceOpen, 0, 3) == Err(XC_WIN), "lo=0");
    // 族二：命中。
    s.add("命中关特性", b.hit("vr-title-y", 5) == Ok(Act::DisableFeat), "Ampere 落窗");
    s.add("窗内贴边", b.hit("anticheat-x", 3) == Ok(Act::ForceOpen), "下沿");
    s.add("窗内贴上沿", b.hit("anticheat-x", 5) == Ok(Act::ForceOpen), "上沿");
    // 族三：窗外不命中。
    s.add("窗外老拒", b.hit("anticheat-x", 2) == Err(XC_NOENT), "Pascal 无此雷");
    s.add("窗外新拒", b.hit("anticheat-x", 6) == Err(XC_NOENT), "Ada 已修");
    s.add("未登记拒", b.hit("ghost", 4) == Err(XC_NOENT), "无签名");
    // legacy-z 窗 1~3：gen 4 不命中。
    s.add("legacy 窗外", b.hit("legacy-z", 4) == Err(XC_NOENT), "过期规避");
    // 族四：同签名更新。
    let _ = b.put("anticheat-x", Act::DisableFeat, 3, 4);
    s.add("更新动作", b.hit("anticheat-x", 3) == Ok(Act::DisableFeat), "动作变");
    s.add("条目不涨", b.len() == 3, "更新非重注");
    s.add("旧窗作废", b.hit("anticheat-x", 5) == Err(XC_NOENT), "窗缩了");
    // 族五：统计。
    s.add("ForceOpen 0", b.count_act(Act::ForceOpen) == 0, "全转其它");
    s.add("DisableFeat 2", b.count_act(Act::DisableFeat) == 2, "两关特性");
    s.add("动作闭集", Act::from_u32(2) == Some(Act::DowngradePresent) && Act::from_u32(3).is_none(), "三动作");
    s.add("魔数核", WL_SEG_BASE == 0x30D80, "段基址");
    s
}
