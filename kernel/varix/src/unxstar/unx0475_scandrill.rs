//! unx0475_scandrill —— UNX-F0475 扫描负载演练（重入/重复扫描）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0475 · 扫描负载演练 · 目标 220 行
//! 判据原文：UNX-F0475-J1 扫描器压力演练账：账二 重入防护：
//!           扫描进行中再次请求扫描 → 拒（重入旗，BUSY 立
//!           案——两轮并发扫描会撕裂快照）；账三 重复扫描计
//!           数：连续 N 轮扫描完成计数（轮次账），每轮终态
//!           归档（发现数、耗时步数）；账四 轮次守恒：完成
//!           轮数 ≡ 归档条目数；账五 疲劳面：连续扫描预算
//!           上限（max_rounds），超限休眠立案（不静默丢轮）；
//!           账六 终态一致性：每轮发现数在无热插拔面下恒等
//!           （与 0473 稳定核联动）。
//! 组件：重入闸 + 轮次账 + 归档核 + 疲劳闸 + 终态一致性核。
//! 职责定位：扫描负载演练是枚举器的「耐力测试」——真实世
//!   界里热插拔守护、诊断工具都会反复触发扫描；重入撕裂是
//!   最隐蔽的并发 bug（半新半旧的快照），疲劳上限防的是诊
//!   断脚本把总线扫穿（每轮扫描都是真实的配置空间读流量）。
//! 性能逐项分解：闸 O(1)；轮次账 O(n)。
//! 诊断码段：0x34700（0x34701~0x34709 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x34700 起）。
pub const SEG_BASE: u32 = 0x34700;
pub const XC_BUSY: u32 = SEG_BASE + 1; // 重入拒
pub const XC_FATIGUE: u32 = SEG_BASE + 2; // 疲劳超限

/// 轮归档条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundRec {
    pub round: u32,
    pub found: u32,
    pub steps: u32,
}

/// 扫描器负载账。
#[derive(Clone, Debug)]
pub struct ScanDrill {
    pub busy: bool,
    pub rounds: u32,
    pub max_rounds: u32,
    pub archive: Vec<RoundRec>,
    /// 上轮发现数（终态一致性基准）。
    pub last_found: Option<u32>,
}

impl ScanDrill {
    pub fn new(max_rounds: u32) -> ScanDrill {
        ScanDrill { busy: false, rounds: 0, max_rounds, archive: Vec::new(), last_found: None }
    }

    /// 开始一轮：疲劳闸 + 重入闸。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.busy {
            return Err(XC_BUSY);
        }
        if self.rounds >= self.max_rounds {
            return Err(XC_FATIGUE);
        }
        self.busy = true;
        Ok(())
    }

    /// 完成一轮：归档 + 释放重入旗。
    pub fn commit(&mut self, found: u32, steps: u32) -> Result<(), u32> {
        if !self.busy {
            return Err(XC_BUSY);
        }
        self.busy = false;
        self.rounds += 1;
        self.last_found = Some(found);
        self.archive.push(RoundRec { round: self.rounds, found, steps });
        Ok(())
    }

    /// 轮次守恒：完成轮数 ≡ 归档条目数。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.rounds as usize != self.archive.len() {
            return Err(XC_FATIGUE);
        }
        Ok(())
    }

    /// 终态一致性：全部归档条目 found 恒等（无热插拔面）。
    pub fn consistent(&self) -> Result<(), u32> {
        match (self.archive.first(), self.last_found) {
            (Some(f), Some(l)) => {
                if self.archive.iter().any(|r| r.found != f.found) || f.found != l {
                    return Err(XC_FATIGUE);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// 判据入口：UNX-F0475 全族核。
pub fn run_unx0475_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0475");

    // 族一：正常单轮。
    let mut d1 = ScanDrill::new(4);
    cs.add("开轮成", d1.begin().is_ok(), "空闲态");
    cs.add("重入拒", d1.begin() == Err(XC_BUSY), "进行中");
    cs.add("交轮成", d1.commit(12, 340).is_ok(), "归档");
    cs.add("交后闲", d1.begin().is_ok(), "旗释放");

    // 族二：轮次守恒。
    let mut d2 = ScanDrill::new(8);
    for i in 0..3 {
        d2.begin().ok();
        d2.commit(10 + i * 0, 300 + i).ok();
    }
    cs.add("三轮三档", d2.conserve().is_ok(), "守恒");
    cs.add("轮号连续", d2.archive.iter().enumerate().all(|(i, r)| r.round == i as u32 + 1), "1,2,3");
    cs.add("步数各记", d2.archive[0].steps == 300 && d2.archive[2].steps == 302, "逐轮");

    // 族三：疲劳闸。
    let mut d3 = ScanDrill::new(2);
    d3.begin().ok(); d3.commit(5, 100).ok();
    d3.begin().ok(); d3.commit(5, 100).ok();
    cs.add("超限立案", d3.begin() == Err(XC_FATIGUE), "max_rounds=2");
    cs.add("未入轮不交", d3.commit(5, 100) == Err(XC_BUSY), "无开轮");

    // 族四：终态一致性。
    let mut d4 = ScanDrill::new(4);
    for _ in 0..3 {
        d4.begin().ok();
        d4.commit(9, 250).ok();
    }
    cs.add("终态恒等", d4.consistent().is_ok(), "九设备×3 轮");
    let mut d5 = ScanDrill::new(4);
    d5.begin().ok(); d5.commit(9, 250).ok();
    d5.begin().ok(); d5.commit(10, 250).ok(); // 热插拔 +1
    cs.add("波动立案", d5.consistent() == Err(XC_FATIGUE), "9→10");

    // 族五：空账一致性（零轮合法）。
    let d6 = ScanDrill::new(4);
    cs.add("空账一致", d6.consistent().is_ok(), "零轮");
    cs.add("空账守恒", d6.conserve().is_ok(), "0=0");

    // 族六：守恒破（人工错账）。
    let mut d7 = ScanDrill::new(4);
    d7.begin().ok(); d7.commit(3, 50).ok();
    d7.rounds += 1; // 错账注入
    cs.add("错账立案", d7.conserve() == Err(XC_FATIGUE), "轮数≠档数");

    // 族七：归档保真。
    let mut d8 = ScanDrill::new(4);
    d8.begin().ok(); d8.commit(7, 111).ok();
    let rec = d8.archive[0];
    cs.add("归档字段", rec.found == 7 && rec.steps == 111, "逐字段");
    cs.add("last_found 同步", d8.last_found == Some(7), "基准更新");

    cs
}
