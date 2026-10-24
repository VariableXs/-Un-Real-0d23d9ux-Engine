//! veb359_sfdoc —— VE-F0359 软渲文档。
//! 锚点：VE-F0359 · 目标 320 行。
//! 判据原文：三清单（支持 / 不支持 / 实验三段式）、预期不虚
//!           标（性能预期按伸缩结论与分档，冲突走实测修正）、
//!           同源保证（清单与实现一致性由声明背书机制同构保
//!           证）、版本化（清单过期版本化更新提示）、判据。
//! 组件：三清单 + 预期专节 + 背书引用账。
//! 职责定位：软渲文档是「柜台菜单」——三段分区（能做 / 不做
//!           / 试做），价目（性能预期）按实测写不虚标；菜单
//!           与后厨同源（声明背书），菜变了菜单跟着换版。
//! 性能项逐项分解：维护 O(清单项) 增量；一致性 O(1) 同源；
//!           更新按需。
//! 诊断码段：0x322C0（0x322C1~0x322C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SD_VERSION: &str = "veb359_sfdoc-v1";

/// 诊断段基址（0x322C0 段 · 本模块独占）。
pub const SD_SEG_BASE: u32 = 0x322C0;
pub const XC_NOBK: u32 = 0x322C1; // 无背书引用
pub const XC_OVER: u32 = 0x322C2; // 预期虚标（超实测上限）
pub const XC_STALE: u32 = 0x322C3; // 清单过期
pub const XC_DUP: u32 = 0x322C4; // 条目重复

/// 清单段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sec {
    /// 支持。
    Sup,
    /// 不支持。
    No,
    /// 实验。
    Exp,
}

/// 条目。
pub struct Doc {
    pub sec: Sec,
    /// 条目指纹。
    pub fp: u32,
    /// 背书引用号（0 = 无）。
    pub bk: u32,
}

/// 性能预期（分档表：vCPU 档 → 预期百分比）。
pub struct Expect {
    /// 实测上限（百分比），预期不得超。
    pub cap: u32,
    pub table: Vec<(u32, u32)>, // (档, 预期)
}

/// 文档簿。
pub struct SfDoc {
    items: Vec<Doc>,
    pub exp: Expect,
    /// 版本号。
    pub ver: u32,
    /// 最近更新周期。
    pub touched: u32,
}

/// 过期窗。
pub const STALE_WIN: u32 = 24;

impl SfDoc {
    /// 建：实测上限为锚。
    pub fn new(cap: u32, ver: u32, epoch: u32) -> SfDoc {
        SfDoc {
            items: Vec::new(),
            exp: Expect {
                cap,
                table: Vec::new(),
            },
            ver,
            touched: epoch,
        }
    }

    /// 记条目：支持/实验段必须带背书（同源保证）；重复拒。
    pub fn put(&mut self, sec: Sec, fp: u32, bk: u32) -> Result<(), u32> {
        if sec != Sec::No && bk == 0 {
            return Err(XC_NOBK);
        }
        if self.items.iter().any(|d| d.fp == fp) {
            return Err(XC_DUP);
        }
        self.items.push(Doc { sec, fp, bk });
        Ok(())
    }

    /// 记预期分档：预期超实测上限拒（不虚标）。
    pub fn expect(&mut self, tier: u32, pct: u32) -> Result<(), u32> {
        if pct > self.exp.cap {
            return Err(XC_OVER);
        }
        self.exp.table.push((tier, pct));
        Ok(())
    }

    /// 预期查询。
    pub fn expect_of(&self, tier: u32) -> Option<u32> {
        self.exp.table.iter().find(|x| x.0 == tier).map(|x| x.1)
    }

    /// 段内计数。
    pub fn count(&self, sec: Sec) -> u32 {
        self.items.iter().filter(|d| d.sec == sec).count() as u32
    }

    /// 三段齐核：三段都非空才是完整三段式。
    pub fn full(&self) -> bool {
        self.count(Sec::Sup) > 0 && self.count(Sec::No) > 0 && self.count(Sec::Exp) > 0
    }

    /// 同源核：所有支持段条目都有背书。
    pub fn sourced(&self) -> bool {
        self.items
            .iter()
            .all(|d| d.sec == Sec::No || d.bk != 0)
    }

    /// 版本化更新。
    pub fn bump(&mut self, ver: u32, epoch: u32) -> Result<(), u32> {
        if ver == 0 || ver <= self.ver {
            return Err(XC_STALE);
        }
        self.ver = ver;
        self.touched = epoch;
        Ok(())
    }

    /// 过期核。
    pub fn stale(&self, now: u32) -> bool {
        now.saturating_sub(self.touched) > STALE_WIN
    }

    /// 实测修正：新实测上限压低时，超上限的旧预期标虚标（返回违规数）。
    pub fn recalc(&mut self, new_cap: u32) -> u32 {
        let old = self.exp.cap;
        self.exp.cap = new_cap;
        if new_cap < old {
            self.exp
                .table
                .iter()
                .filter(|x| x.1 > new_cap)
                .count() as u32
        } else {
            0
        }
    }
}

pub fn run_veb359_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0359");
    // 族零：背书门。
    let mut d = SfDoc::new(100, 1, 1);
    s.add("支持无背书拒", d.put(Sec::Sup, 11, 0) == Err(XC_NOBK), "同源保证");
    s.add("实验无背书拒", d.put(Sec::Exp, 31, 0) == Err(XC_NOBK), "实验也要背书");
    s.add("不支持免背书过", d.put(Sec::No, 21, 0).is_ok(), "拒绝无需证据");
    s.add("支持背书过", d.put(Sec::Sup, 11, 91).is_ok(), "引用 91");
    s.add("重复拒", d.put(Sec::Exp, 11, 92) == Err(XC_DUP), "一指纹一条");
    // 族一：三段式。
    let _ = d.put(Sec::Exp, 31, 93);
    s.add("三段齐", d.full(), "支持一不一实一");
    s.add("段计数", d.count(Sec::Sup) == 1 && d.count(Sec::No) == 1 && d.count(Sec::Exp) == 1, "各一");
    s.add("同源核", d.sourced(), "全有背书");
    // 无背书实验条目破坏同源？——put 已拒，构造防御核对。
    let d2 = SfDoc::new(100, 1, 1);
    s.add("空簿同源", d2.sourced(), "空簿恒真");
    s.add("空簿不全", !d2.full(), "三段缺");
    // 族二：预期不虚标。
    s.add("预期 60 过", d.expect(4, 60).is_ok(), "中档");
    s.add("预期 100 过", d.expect(8, 100).is_ok(), "恰上限");
    s.add("预期 110 虚标拒", d.expect(16, 110) == Err(XC_OVER), ">cap");
    s.add("预期查 60", d.expect_of(4) == Some(60), "在表");
    s.add("预期查无", d.expect_of(2).is_none(), "没登记");
    // 族三：实测修正。
    s.add("上限压低", d.recalc(90) == 1, "100>90 一条违规");
    s.add("预期 90 过", d.expect(16, 90).is_ok(), "修正后可表");
    s.add("上限升高无违规", d.recalc(120) == 0, "放宽");
    // 族四：版本化。
    s.add("窗内不旧", !d.stale(1 + STALE_WIN), "恰窗");
    s.add("超窗旧", d.stale(1 + STALE_WIN + 1), "提示更新");
    s.add("升版过", d.bump(2, 40).is_ok(), "v2");
    s.add("回版拒", d.bump(1, 41) == Err(XC_STALE), "版本倒退");
    s.add("同版拒", d.bump(2, 42) == Err(XC_STALE), "非新");
    s.add("刷新期", !d.stale(42), "v2 触 42");
    // 族五：大清单。
    let mut d3 = SfDoc::new(100, 1, 1);
    for i in 0..20u32 {
        let _ = d3.put(Sec::Sup, 100 + i, 500 + i);
    }
    s.add("支持二十", d3.count(Sec::Sup) == 20, "全背书");
    s.add("魔数核", SD_SEG_BASE == 0x322C0, "段基址");
    s
}
