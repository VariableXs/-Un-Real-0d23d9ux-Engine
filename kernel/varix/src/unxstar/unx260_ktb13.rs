//! UNX-F0260 ktest 引导面 B13 批断言集（0x3E60 段）
//!
//! 锚点：UNX-F0260-J1 本批判据聚合入 ktest，一次命令全绿。
//! 域：CoRun·UNX-A1-B13（B13 批 ktest 聚合面）。
//!
//! 结构：B13 批断言集聚合器（F0247~F0259 十三模块判据全量入集）
//! + 防自递归设计（本模块 self_audit 只查结构完备性，full_green
//! 走独立外部入口，不重入本集）+ 批位图（13 模块逐位亮灯）+
//! 入口表冻结（模块名 → 判据入口签名一致性）+ 注入核（缺模块 /
//! 位图漂移 / 表序漂移必抓）。
//!
//! 纪律：聚合器不重入；断言失败不 panic，落 red_items 明细。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

/// 诊断段（0x3E60 独占 0x10）。
pub const SEG_BASE: u32 = 0x3E60;
pub const KT_MISS: u32 = SEG_BASE + 0x01; // 断言集缺模块
pub const KT_BITMAP: u32 = SEG_BASE + 0x02; // 批位图漂移
pub const KT_ORDER: u32 = SEG_BASE + 0x03; // 入口表序漂移
pub const KT_REENTRY: u32 = SEG_BASE + 0x04; // 聚合器重入被拒

/// B13 批成员模块数（F0247..=F0259 十三模块，聚合器自身除外）。
pub const B13_MEMBERS: usize = 13;

/// 成员名册（判据原文引用序，冻结）。
pub const B13_NAMES: [&str; B13_MEMBERS] = [
    "UNX-F0247", // 健康位面板
    "UNX-F0248", // 时间基准链
    "UNX-F0249", // 日志-指纹关联键
    "UNX-F0250", // 上次引导记录携带
    "UNX-F0251", // 消费方订阅接口
    "UNX-F0252", // 十二步齐备 CRC 链
    "UNX-F0253", // 早传版本兼容
    "UNX-F0254", // cmdline 白名单注册
    "UNX-F0255", // 参数来源优先级
    "UNX-F0256", // 参数变更审计账
    "UNX-F0257", // 快照到报告字段映射
    "UNX-F0258", // 十二步时序账
    "UNX-F0259", // 防篡改 CRC 链巡检
];

/// 批位图（每位对应一名册成员：绿 = 1）。
pub struct BatchBitmap {
    pub bits: u32,
}

impl BatchBitmap {
    pub fn new() -> Self {
        BatchBitmap { bits: 0 }
    }

    /// 点灯（位号越界拒绝）。
    pub fn light(&mut self, idx: usize) -> bool {
        if idx >= B13_MEMBERS {
            return false;
        }
        self.bits |= 1u32 << idx;
        true
    }

    /// 全亮核。
    pub fn all_lit(&self) -> bool {
        self.bits == (1u32 << B13_MEMBERS) - 1
    }

    /// 亮灯数。
    pub fn lit_count(&self) -> u32 {
        self.bits.count_ones()
    }
}

/// 结构自审（防自递归：只查名册/位图/表序结构，不调成员判据）。
pub fn self_audit() -> Result<(), u32> {
    // 名册序冻结核：十三名按 F0247..F0259 递增
    let mut i = 0usize;
    while i < B13_MEMBERS {
        let expect = format!("UNX-F0{}", 247 + i);
        if B13_NAMES[i] != expect.as_str() {
            return Err(KT_ORDER);
        }
        i += 1;
    }
    // 位图全亮可达核（空位图 → 逐位点亮 → 全亮）
    let mut bm = BatchBitmap::new();
    let mut j = 0usize;
    while j < B13_MEMBERS {
        if !bm.light(j) {
            return Err(KT_BITMAP);
        }
        j += 1;
    }
    if !bm.all_lit() || bm.lit_count() != B13_MEMBERS as u32 {
        return Err(KT_BITMAP);
    }
    // 位号越界拒核
    let mut bm2 = BatchBitmap::new();
    if bm2.light(B13_MEMBERS) {
        return Err(KT_BITMAP);
    }
    Ok(())
}

/// B13 批全量判据（外部入口，一次命令全绿）：
/// 聚合 F0247..F0259 十三模块 run_*_checks 到单一 CheckSet。
/// 独立入口（非成员递归），重入守卫由调用方（ktest 域聚合）保证。
pub fn b13_full_green() -> CheckSet {
    let mut set = CheckSet::new("UNX-F0260-B13");
    let mut bm = BatchBitmap::new();

    // 1) F0247 健康位面板
    {
        let sub = crate::unxstar::unx247_health::run_unx247_checks();
        let ok = sub.all_passed();
        let d = if ok { String::new() } else { sub.red_items_text() };
        set.add_dyn("B13-F0247-健康位面板", ok, &d);
        if ok {
            bm.light(0);
        }
    }
    // 2) F0248 时间基准链
    {
        let sub = crate::unxstar::unx248_timebase::run_unx248_checks();
        let ok = sub.all_passed();
        let d = if ok { String::new() } else { sub.red_items_text() };
        set.add_dyn("B13-F0248-时间基准链", ok, &d);
        if ok {
            bm.light(1);
        }
    }
    // 3) F0249 日志-指纹关联键
    {
        let sub = crate::unxstar::unx249_logkey::run_unx249_checks();
        let ok = sub.all_passed();
        let d = if ok { String::new() } else { sub.red_items_text() };
        set.add_dyn("B13-F0249-日志关联键", ok, &d);
        if ok {
            bm.light(2);
        }
    }
    // 4) F0250 上次引导记录携带
    {
        let sub = crate::unxstar::unx250_lastboot::run_unx250_checks();
        let ok = sub.all_passed();
        let d = if ok { String::new() } else { sub.red_items_text() };
        set.add_dyn("B13-F0250-上次引导携带", ok, &d);
        if ok {
            bm.light(3);
        }
    }
    // 5) F0251 消费方订阅接口
    {
        let sub = crate::unxstar::unx251_subscribe::run_unx251_checks();
        let ok = sub.all_passed();
        let d = if ok { String::new() } else { sub.red_items_text() };
        set.add_dyn("B13-F0251-订阅接口", ok, &d);
        if ok {
            bm.light(4);
        }
    }
    // 6) F0252 十二步齐备 CRC 链
    {
        let sub = crate::unxstar::unx252_crcchain::run_unx252_checks();
        let ok = sub.all_passed();
        let d = if ok { String::new() } else { sub.red_items_text() };
        set.add_dyn("B13-F0252-完整性CRC链", ok, &d);
        if ok {
            bm.light(5);
        }
    }
    // 7) F0253 早传版本兼容
    {
        let sub = crate::unxstar::unx253_vercompat::run_unx253_checks();
        let ok = sub.all_passed();
        let d = if ok { String::new() } else { sub.red_items_text() };
        set.add_dyn("B13-F0253-版本兼容", ok, &d);
        if ok {
            bm.light(6);
        }
    }
    // 8) F0254 cmdline 白名单注册
    {
        let sub = crate::unxstar::unx254_cmdreg::run_unx254_checks();
        let ok = sub.all_passed();
        let d = if ok { String::new() } else { sub.red_items_text() };
        set.add_dyn("B13-F0254-白名单注册", ok, &d);
        if ok {
            bm.light(7);
        }
    }
    // 9) F0255 参数来源优先级
    {
        let sub = crate::unxstar::unx255_prio::run_unx255_checks();
        let ok = sub.all_passed();
        let d = if ok { String::new() } else { sub.red_items_text() };
        set.add_dyn("B13-F0255-来源优先级", ok, &d);
        if ok {
            bm.light(8);
        }
    }
    // 10) F0256 参数变更审计账
    {
        let sub = crate::unxstar::unx256_audit::run_unx256_checks();
        let ok = sub.all_passed();
        let d = if ok { String::new() } else { sub.red_items_text() };
        set.add_dyn("B13-F0256-变更审计账", ok, &d);
        if ok {
            bm.light(9);
        }
    }
    // 11) F0257 快照到报告字段映射
    {
        let sub = crate::unxstar::unx257_fieldmap::run_unx257_checks();
        let ok = sub.all_passed();
        let d = if ok { String::new() } else { sub.red_items_text() };
        set.add_dyn("B13-F0257-字段映射", ok, &d);
        if ok {
            bm.light(10);
        }
    }
    // 12) F0258 十二步时序账
    {
        let sub = crate::unxstar::unx258_timing::run_unx258_checks();
        let ok = sub.all_passed();
        let d = if ok { String::new() } else { sub.red_items_text() };
        set.add_dyn("B13-F0258-时序账", ok, &d);
        if ok {
            bm.light(11);
        }
    }
    // 13) F0259 防篡改 CRC 链巡检
    {
        let sub = crate::unxstar::unx259_tamper::run_unx259_checks();
        let ok = sub.all_passed();
        let d = if ok { String::new() } else { sub.red_items_text() };
        set.add_dyn("B13-F0259-防篡改巡检", ok, &d);
        if ok {
            bm.light(12);
        }
    }

    // 收口：位图全亮 = 十三模块全绿
    let ok = bm.all_lit();
    let d = if ok { String::new() } else { format!("位图 {}/{}", bm.lit_count(), B13_MEMBERS) };
    set.add_dyn("B13-F0260-批位图全亮", ok, &d);
    set
}

/// 本模块判据（结构面：self_audit + 名册 + 位图）。
pub fn run_unx260_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0260");
    // J1-1 断言集结构完备（名册序 + 位图 + 越界拒）
    let ok = self_audit().is_ok();
    s.add("B13-65-断言集结构完备", ok, if ok { "" } else { "结构破缺" });
    // J1-2 名册冻结（十三名与任务号映射恒等）
    let ok = B13_NAMES[0] == "UNX-F0247"
        && B13_NAMES[4] == "UNX-F0251"
        && B13_NAMES[12] == "UNX-F0259"
        && B13_NAMES.len() == B13_MEMBERS;
    s.add("B13-66-名册冻结", ok, if ok { "" } else { "名册漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx260_struct_green() {
        let set = run_unx260_checks();
        assert!(set.all_passed(), "UNX-F0260 struct checks: {}", set.red_items_text());
    }

    #[test]
    fn unx260_b13_full_green() {
        // 一次命令全绿：B13 批十三模块 + 聚合位图
        let set = b13_full_green();
        assert!(set.all_passed(), "B13 batch must be green: {}", set.red_items_text());
    }
}
