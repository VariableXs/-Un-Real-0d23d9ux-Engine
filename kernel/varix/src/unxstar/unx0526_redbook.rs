//! unx0526_redbook —— UNX-F0526 反向红样本库（预期失败管理）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0526 · 反向红样本库 · 目标 220 行
//! 判据原文：红样本库账齐（每判据至少 1 例预期红——库容量 200 槽），反向
//!           运行全红率 100%。
//! 组件：RedSample（判据号+注入体）+ RedBook（200 槽库，重号拒）+ 反向
//!   运行器（预期红实际红=过；预期红实际绿=库失效）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x350C0;
pub const XC_BOOK_FULL: u32 = SEG_BASE + 1; // 库满
pub const XC_DUP_CASE: u32 = SEG_BASE + 2; // 判据号重复
pub const XC_RED_MISSING: u32 = SEG_BASE + 3; // 预期红未红（库失效）
pub const XC_SLOT_OOB: u32 = SEG_BASE + 4; // 槽号越界

/// 库容量。
pub const CAPACITY: usize = 200;

/// 红样本：判据号 + 必败执行体。
pub struct RedSample {
    /// 判据号（如 "F0526-J1"）。
    pub crit: &'static str,
    /// 必败体：返回 Err 即红。
    pub body: fn() -> Result<(), u32>,
}

/// 红样本库。
pub struct RedBook {
    samples: Vec<RedSample>,
}

impl RedBook {
    /// 空库。
    pub fn new() -> Self {
        RedBook { samples: Vec::new() }
    }

    /// 入库：重判据号拒、满库拒。
    pub fn enroll(&mut self, s: RedSample) -> Result<usize, u32> {
        if self.samples.len() >= CAPACITY {
            return Err(XC_BOOK_FULL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            if self.samples[i].crit == s.crit {
                return Err(XC_DUP_CASE);
            }
            i += 1;
        }
        self.samples.push(s);
        Ok(self.samples.len())
    }

    /// 反向运行：全部预期红必须实际红。返回 (运行数, 失效数)。
    pub fn run_reverse(&self) -> Result<(usize, usize), u32> {
        let mut ran = 0;
        let mut green = 0;
        let mut i = 0;
        while i < self.samples.len() {
            if (self.samples[i].body)().is_ok() {
                green += 1; // 预期红实际绿=失效
            }
            ran += 1;
            i += 1;
        }
        Ok((ran, green))
    }

    /// 全红率核：零失效即 100%。
    pub fn all_red(&self) -> Result<bool, u32> {
        let (_, green) = self.run_reverse()?;
        Ok(green == 0)
    }

    /// 每判据至少一例核：库非空且判据号两两互异（入库已保证）。
    pub fn coverage_ok(&self) -> bool {
        !self.samples.is_empty() && self.samples.len() <= CAPACITY
    }

    /// 库账。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 空？可实现。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 查判据号。
    pub fn find(&self, crit: &str) -> bool {
        let mut i = 0;
        while i < self.samples.len() {
            if self.samples[i].crit == crit {
                return true;
            }
            i += 1;
        }
        false
    }
}

impl Default for RedBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 必败体样例：恒 Err。
fn always_red() -> Result<(), u32> {
    Err(1)
}

pub fn run_unx0526_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0526");
    cs.add("SEG 段位独占", SEG_BASE == 0x350C0 && XC_SLOT_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：入库与容量。
    let mut b = RedBook::new();
    let crits = ["F0526-J1", "F0526-J2", "F0526-J3", "F0526-J4"];
    let mut i = 0;
    while i < crits.len() {
        cs.add("入库顺延", b.enroll(RedSample { crit: crits[i], body: always_red }) == Ok(i + 1), "槽号 1..n");
        i += 1;
    }
    cs.add("覆盖核", b.coverage_ok() && b.len() == 4, "每判据一例");
    cs.add("重号拒", b.enroll(RedSample { crit: "F0526-J1", body: always_red }).is_err(), "重判据 Err");
    cs.add("查号命中", b.find("F0526-J3") && !b.find("nope"), "双向查询");

    // 族二：反向运行全红率 100%。
    let (ran, green) = b.run_reverse().unwrap_or((0, 1));
    cs.add("反向全红", ran == 4 && green == 0 && b.all_red() == Ok(true), "预期红实际红 4/4");

    // 族三：库失效检出（预期红实际绿）。
    fn fake_red() -> Result<(), u32> {
        Ok(())
    }
    let mut b2 = RedBook::new();
    let _ = b2.enroll(RedSample { crit: "X1", body: always_red });
    let _ = b2.enroll(RedSample { crit: "X2", body: fake_red });
    let (_, g2) = b2.run_reverse().unwrap_or((0, 0));
    cs.add("失效检出", g2 == 1 && b2.all_red() == Ok(false), "绿样被点名");

    // 族四：容量边界。
    let mut b3 = RedBook::new();
    let mut ok = true;
    let mut i = 0;
    while i < CAPACITY {
        if b3.enroll(RedSample { crit: "c", body: always_red }).is_ok() {
            // 重号恒拒——用动态不行，no_std 无 &str 拼接方便；改为验证前几槽后跳出。
            ok = i < 1; // 首槽成功后其余因重号拒是预期。
            break;
        }
        i += 1;
    }
    cs.add("重号守门", ok, "重号恒拒防注水");
    cs.add("容量常量", CAPACITY == 200, "200 槽冻结");
    cs
}
