//! VE-F0495 · 优化器资源上限（VE-C 域 · 优化组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0495`
//!
//! **锚点原文**：「优化器资源上限防护：编译内存上限（超限中止并建议拆
//! 分）、指令数上限（病态输入防护——超限显性拒绝不挂死）、迭代上限（每
//! pass 收敛轮数封顶）；防护触发全部显性三要素（不是静默截断）；上限值
//! 版本化可配。数据结构：上限配置（内存×指令×迭代）；触发记录。错误路
//! 径与降级矩阵：内存超限→中止建议拆分（F0493 联动）；指令超限→拒绝带
//! 规模说明；迭代超限→保守输出加标注。判据：三重上限、显性触发、可配版
//! 本化、拆分建议、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三重上限**（判据一）。内存 × 指令数 × 迭代三重封顶，每个检查点
//!    O(1) 判定；缺任一上限配置即入账待补。
//!
//! 2. **显性触发**（判据二）。触发全部带三要素（上限值 / 实际值 / 处置）
//!    显性记录——不静默截断。
//!
//! 3. **可配版本化**（判据三）。上限值版本化可配（版本不匹配拒绝生效）。
//!
//! 4. **拆分建议**（判据四）。内存超限中止时附拆分建议（F0493 联动）；
//!    迭代超限 → 保守输出加标注（不硬收敛）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0495";

/// 上限配置（内存 × 指令 × 迭代）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// 编译内存上限（KiB）。
    pub mem_kib: u32,
    /// 指令数上限。
    pub ins_cap: u32,
    /// 每 pass 迭代收敛上限。
    pub iter_cap: u32,
    /// 配置版本号。
    pub ver: u32,
}

/// 处置动作。
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// 内存超限 → 中止 + 拆分建议。
    AbortSplit(String),
    /// 指令超限 → 显性拒绝带规模说明。
    RejectScale(String),
    /// 迭代超限 → 保守输出 + 标注。
    ConservativeMark(String),
}

/// 上限防护账。
#[derive(Debug, Default)]
pub struct LimitBook {
    pub limits: Option<Limits>,
    /// 触发记录（三要素：上限/实际/处置）。
    pub triggers: Vec<String>,
    pub audit: u32,
}

impl LimitBook {
    /// 配置装载：版本化校验。
    pub fn load(&mut self, l: Limits, want_ver: u32) -> Result<(), &'static str> {
        if l.mem_kib == 0 || l.ins_cap == 0 || l.iter_cap == 0 {
            return Err("上限配置缺项（三重上限缺一即拒）");
        }
        if l.ver != want_ver {
            return Err("上限配置版本不匹配（可配版本化）");
        }
        self.limits = Some(l);
        Ok(())
    }

    /// 内存检查点：超限 → 中止 + 拆分建议（三要素显性）。
    pub fn check_mem(&mut self, used_kib: u32) -> Option<Action> {
        let l = self.limits?;
        if used_kib > l.mem_kib {
            let msg = format!(
                "内存超限：上限 {}KiB 实际 {}KiB → 中止并建议拆分（F0493 联动）",
                l.mem_kib, used_kib
            );
            self.triggers.push(msg.clone());
            Some(Action::AbortSplit(msg))
        } else {
            self.audit += 1;
            None
        }
    }

    /// 指令数检查点：超限 → 显性拒绝带规模说明（不挂死）。
    pub fn check_ins(&mut self, count: u32) -> Option<Action> {
        let l = self.limits?;
        if count > l.ins_cap {
            let msg = format!(
                "指令超限：上限 {} 实际 {} → 显性拒绝（病态输入防护，规模 {} 指令）",
                l.ins_cap, count, count
            );
            self.triggers.push(msg.clone());
            Some(Action::RejectScale(msg))
        } else {
            self.audit += 1;
            None
        }
    }

    /// 迭代检查点：超上限轮数 → 保守输出加标注（不硬收敛）。
    pub fn check_iter(&mut self, pass: &str, rounds: u32) -> Option<Action> {
        let l = self.limits?;
        if rounds > l.iter_cap {
            let msg = format!(
                "迭代超限：pass {} 上限 {} 轮实际 {} 轮 → 保守输出加标注（不硬收敛）",
                pass, l.iter_cap, rounds
            );
            self.triggers.push(msg.clone());
            Some(Action::ConservativeMark(msg))
        } else {
            self.audit += 1;
            None
        }
    }

    /// 全部触发均显性断言：记录非空即显性面成立。
    pub fn triggers_explicit(&self) -> bool {
        self.triggers.iter().all(|t| t.contains("上限") && t.contains("实际") && t.contains("→"))
    }
}

// ---------------------------------------------------------------------------
// 判据（F0495-xxx）
// ---------------------------------------------------------------------------

/// 原始判据。
pub fn vec95_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、配置装载与版本化。
    {
        let mut book = LimitBook::default();
        let bad = book.load(Limits { mem_kib: 0, ins_cap: 100, iter_cap: 8, ver: 2 }, 2);
        v.push(("F0495-配置-缺项拦截", bad.is_err()));
        let vbad = book.load(Limits { mem_kib: 64, ins_cap: 100, iter_cap: 8, ver: 1 }, 2);
        v.push(("F0495-配置-版本拦截", vbad.is_err()));
        let ok = book.load(Limits { mem_kib: 64, ins_cap: 1000, iter_cap: 8, ver: 2 }, 2);
        v.push(("F0495-配置-装载成功", ok.is_ok()));
    }

    // 二、内存上限：中止 + 拆分建议。
    {
        let mut book = LimitBook::default();
        let _ = book.load(Limits { mem_kib: 64, ins_cap: 1000, iter_cap: 8, ver: 2 }, 2);
        let r1 = book.check_mem(60);
        v.push(("F0495-内存-带内放行", r1.is_none() && book.audit == 1));
        let r2 = book.check_mem(100);
        v.push((
            "F0495-内存-超限中止拆分建议",
            matches!(r2, Some(Action::AbortSplit(ref m)) if m.contains("拆分") && m.contains("F0493")),
        ));
    }

    // 三、指令上限：显性拒绝带规模。
    {
        let mut book = LimitBook::default();
        let _ = book.load(Limits { mem_kib: 64, ins_cap: 1000, iter_cap: 8, ver: 2 }, 2);
        let r1 = book.check_ins(999);
        v.push(("F0495-指令-带内放行", r1.is_none()));
        let r2 = book.check_ins(1200);
        v.push((
            "F0495-指令-超限拒绝带规模",
            matches!(r2, Some(Action::RejectScale(ref m)) if m.contains("1200")),
        ));
    }

    // 四、迭代上限：保守输出加标注。
    {
        let mut book = LimitBook::default();
        let _ = book.load(Limits { mem_kib: 64, ins_cap: 1000, iter_cap: 8, ver: 2 }, 2);
        let r1 = book.check_iter("inline", 8);
        v.push(("F0495-迭代-带内放行", r1.is_none()));
        let r2 = book.check_iter("inline", 12);
        v.push((
            "F0495-迭代-超限保守标注",
            matches!(r2, Some(Action::ConservativeMark(ref m)) if m.contains("不硬收敛")),
        ));
        // 显性三要素全成立。
        v.push(("F0495-显性-三要素齐全", book.triggers_explicit()));
        v.push(("F0495-判据-锚点入册", ANCHOR == "VE-F0495"));
    }

    v
}

/// 注册表入口。
pub fn run_vec95_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec95");
    for (name, ok) in vec95_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec95_raw_checks as run_vec95_checks;
    #[test]
    fn vec95_limits_checks() {
        for (name, ok) in run_vec95_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
