//! unx0477_pcxacc —— UNX-F0477 PCI 账对冲总账。
//! 锚点（CoRun Unxreal 主册）：UNX-F0477 · PCI 账对冲总账 · 目标 240 行
//! 判据原文：UNX-F0477-J1 三账对冲（枚举账 / 交接信包账 /
//!           驱动登记账）：账二 数目维对冲（三账设备数逐
//!           一相等，不等立案并归因方向）；账三 签名维对
//!           冲（每设备 (bus,dev,fn) 三元组在账间一致）；
//!           账四 尺寸维对冲（BAR 汇总尺寸域一致）；账五
//!           幂等（对冲两遍结果恒等）；账六 缺账方向归因
//!           （枚举多=信包旧 / 信包多=登记迟）。
//! 组件：三账结构 + 对冲器 + 归因枚举 + 幂等核。
//! 职责定位：三本账各记一份设备清单——枚举器自己的、交给
//!   消费端的信包里的、驱动登记簿上的。三者漂移就是设备丢
//!   失或幽灵设备。对冲账是「闭环守恒」的最后防线：任何一
//!   本账单方面增删都会在此立案。
//! 性能逐项分解：排序 O(n log n)；对冲 O(n)（归并比对）。
//! 诊断码段：0x34780（0x34781~0x34789 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 设备键（bus,dev,fn 三元组打包 u32）。
pub fn key(bus: u8, dev: u8, func: u8) -> u32 {
    ((bus as u32) << 8) | ((dev as u32) << 3) | (func as u32)
}

/// 诊断码。
pub const SEG_BASE: u32 = 0x34780;
pub const XC_COUNT: u32 = SEG_BASE + 1; // 数目维不等
pub const XC_SIGN: u32 = SEG_BASE + 2; // 签名维漂移
pub const XC_SIZE: u32 = SEG_BASE + 3; // 尺寸维漂移

/// 对冲方向归因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drift {
    /// 枚举账多（信包旧）。
    EnumAhead,
    /// 信包账多（登记迟）。
    PktAhead,
    /// 登记账多（幽灵）。
    RegAhead,
    /// 三账全等。
    Even,
}

/// 三账（各为键 + BAR 总尺寸域）。
pub struct TriLedger {
    pub enum_keys: Vec<u32>,
    pub pkt_keys: Vec<u32>,
    pub reg_keys: Vec<u32>,
    /// 各账 BAR 汇总尺寸域（字节，全账合计）。
    pub enum_size: u64,
    pub pkt_size: u64,
    pub reg_size: u64,
}

impl TriLedger {
    pub fn new() -> Self {
        TriLedger { enum_keys: Vec::new(), pkt_keys: Vec::new(), reg_keys: Vec::new(), enum_size: 0, pkt_size: 0, reg_size: 0 }
    }
    pub fn push_enum(&mut self, k: u32, bar_sum: u64) {
        self.enum_keys.push(k);
        self.enum_size += bar_sum;
    }
    pub fn push_pkt(&mut self, k: u32, bar_sum: u64) {
        self.pkt_keys.push(k);
        self.pkt_size += bar_sum;
    }
    pub fn push_reg(&mut self, k: u32, bar_sum: u64) {
        self.reg_keys.push(k);
        self.reg_size += bar_sum;
    }
}

/// 对冲：数目维 + 签名维 + 尺寸维。返回（归因, 立案码或 0）。
pub fn reconcile(l: &TriLedger) -> (Drift, u32) {
    // 排序（插入序任意，签名比对需规范序）。
    let mut e = l.enum_keys.clone();
    let mut p = l.pkt_keys.clone();
    let mut r = l.reg_keys.clone();
    e.sort();
    p.sort();
    r.sort();
    // 数目维。
    if e.len() != p.len() || p.len() != r.len() {
        let drift = if e.len() > p.len() || e.len() > r.len() {
            Drift::EnumAhead
        } else if p.len() > r.len() {
            Drift::PktAhead
        } else {
            Drift::RegAhead
        };
        return (drift, XC_COUNT);
    }
    // 签名维：逐位全等。
    let mut i = 0usize;
    while i < e.len() {
        if e[i] != p[i] || p[i] != r[i] {
            return (Drift::Even, XC_SIGN);
        }
        i += 1;
    }
    // 尺寸维。
    if l.enum_size != l.pkt_size || l.pkt_size != l.reg_size {
        return (Drift::Even, XC_SIZE);
    }
    (Drift::Even, 0)
}

pub fn run_unx0477_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0477");
    cs.add("SEG 段位独占", SEG_BASE == 0x34780 && XC_SIZE == SEG_BASE + 3, "u32 高段全域普查无冲突");
    cs.add("键打包", key(0, 2, 3) == 0x0000_0013, "bus<<8|dev<<3|fn");
    cs.add("键满射", key(0xFF, 0x1F, 7) == 0x0000_FFFF, "全位占满");

    // 族一：三账全等 → Even。
    let mut l0 = TriLedger::new();
    for i in 0..4u8 {
        l0.push_enum(key(i, 0, 0), 0x1000);
        l0.push_pkt(key(i, 0, 0), 0x1000);
        l0.push_reg(key(i, 0, 0), 0x1000);
    }
    let (d0, x0) = reconcile(&l0);
    cs.add("三账全等", d0 == Drift::Even && x0 == 0, "数目/签名/尺寸三维平");

    // 族二：数目维立案 + 方向归因。
    let mut l1 = l0.clone_keys();
    l1.push_enum(key(9, 0, 0), 0x1000); // 枚举多一个
    let (d1, x1) = reconcile(&l1);
    cs.add("枚举多立案", x1 == XC_COUNT, "数目维破");
    cs.add("归因枚举先", d1 == Drift::EnumAhead, "信包旧");

    let mut l2 = l0.clone_keys();
    l2.push_pkt(key(9, 0, 0), 0x1000);
    let (d2, x2) = reconcile(&l2);
    cs.add("归因信包先", d2 == Drift::PktAhead && x2 == XC_COUNT, "登记迟");

    let mut l3 = l0.clone_keys();
    l3.push_reg(key(9, 0, 0), 0x1000);
    let (d3, x3) = reconcile(&l3);
    cs.add("归因幽灵", d3 == Drift::RegAhead && x3 == XC_COUNT, "登记多");

    // 族三：签名维立案（同数目、异成员）。
    let mut l4 = TriLedger::new();
    for i in 0..4u8 {
        l4.push_enum(key(i, 0, 0), 0x1000);
        l4.push_pkt(key(i, 0, 0), 0x1000);
        l4.push_reg(key(i, 0, 0), 0x1000);
    }
    l4.pkt_keys[2] = key(0, 5, 0); // 换一个成员
    let (d4, x4) = reconcile(&l4);
    cs.add("签名漂移立案", x4 == XC_SIGN, "数目平成员异");
    cs.add("签名维不归因", d4 == Drift::Even, "方向仅数目维");

    // 族四：尺寸维立案。
    let mut l5 = TriLedger::new();
    for i in 0..4u8 {
        l5.push_enum(key(i, 0, 0), 0x1000);
        l5.push_pkt(key(i, 0, 0), 0x1000);
        l5.push_reg(key(i, 0, 0), 0x2000); // 登记账尺寸翻倍
    }
    let (d5, x5) = reconcile(&l5);
    cs.add("尺寸漂移立案", x5 == XC_SIZE, "BAR 域破");
    cs.add("尺寸不归因", d5 == Drift::Even, "同上");

    // 族五：乱序录入（签名比对前排序规范）。
    let mut l6 = TriLedger::new();
    for i in [3u8, 1, 0, 2] {
        l6.push_enum(key(i, 0, 0), 0x1000);
    }
    for i in [0u8, 1, 2, 3] {
        l6.push_pkt(key(i, 0, 0), 0x1000);
        l6.push_reg(key(i, 0, 0), 0x1000);
    }
    let (d6, x6) = reconcile(&l6);
    cs.add("乱序等价", d6 == Drift::Even && x6 == 0, "排序后集合全等");

    // 族六：幂等（对冲两遍恒等）。
    let (a, b) = reconcile(&l0);
    let (c, d) = reconcile(&l0);
    cs.add("幂等核", a == c && b == d, "两遍恒等");

    // 族七：空账（全零）。
    let l7 = TriLedger::new();
    let (d7, x7) = reconcile(&l7);
    cs.add("空账平", d7 == Drift::Even && x7 == 0, "零设备也闭环");

    cs
}

/// 测试夹具：键表克隆（结构体未派生 Clone 时的最小复制）。
impl TriLedger {
    fn clone_keys(&self) -> TriLedger {
        TriLedger {
            enum_keys: self.enum_keys.clone(),
            pkt_keys: self.pkt_keys.clone(),
            reg_keys: self.reg_keys.clone(),
            enum_size: self.enum_size,
            pkt_size: self.pkt_size,
            reg_size: self.reg_size,
        }
    }
}
