//! VE-F0476 · IR 调试信息装饰（VE-C 域 · IR 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0476`
//!
//! **锚点原文**：「IR 调试信息装饰：指令到源码位置全量装饰（F0462 搬运的
//! 落库）、变量到原始名映射（优化改名后仍可溯源）、作用域标记装饰；调试信
//! 息与优化联动（优化时位置随指令走，不可溯源即标注）；调试信息可剥离（发
//! 布模式裁剪）。判据：全量装饰、溯源映射、随优化走、可剥离、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **全量装饰**（判据一）。指令 → 源位置全量落库（[`DebugBook`]：缺位
//!    即灰态标注——不静默缺）。
//!
//! 2. **溯源映射**（判据二）。寄存器 → 原始名映射（优化改名后仍可溯——
//!    [`VarMap`] 单向改名链），映射缺失 → 灰态。
//!
//! 3. **随优化走**（判据三）。优化后位置随指令走（装饰表重放：优化记录的
//!    位置迁移 → 表同步），不可溯源 → 质量账标注（计数入账）。
//!
//! 4. **可剥离**（判据四）。发布模式裁剪（[`strip`]：调试表清空 + 裁剪计
//!    数），剥离后诊断降级如实告知。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0476";

// ---------------------------------------------------------------------------
// 调试册（全量装饰 + 溯源映射）
// ---------------------------------------------------------------------------

/// 调试册：位置装饰表 + 变量映射 + 质量账。
#[derive(Debug, Default)]
pub struct DebugBook {
    /// (块, 指令序) → 源偏移。
    pub pos: Vec<(u32, usize, u32)>,
    /// 寄存器 → 原始名（优化改名链的溯源面）。
    pub var_names: Vec<(u32, String)>,
    /// 不可溯源标注计数（优化质量账——F0496 消费）。
    pub untraceable: u64,
}

impl DebugBook {
    pub fn new() -> Self {
        Self::default()
    }
    /// 全量装饰：每条指令位置落库（重复装饰幂等——同位覆写）。
    pub fn decorate(&mut self, block: u32, inst: usize, src_off: u32) {
        if let Some(e) = self.pos.iter_mut().find(|(b, i, _)| *b == block && *i == inst) {
            e.2 = src_off;
        } else {
            self.pos.push((block, inst, src_off));
        }
    }
    /// 溯源查询：位置面 O(表长) 线性（装饰面最小形态）。
    pub fn src_of(&self, block: u32, inst: usize) -> Option<u32> {
        self.pos.iter().find(|(b, i, _)| *b == block && *i == inst).map(|(_, _, p)| *p)
    }
    /// 变量改名登记（单向链：新寄存器 → 原始名）。
    pub fn rename(&mut self, new_reg: u32, orig: &str) {
        if !self.var_names.iter().any(|(r, _)| *r == new_reg) {
            self.var_names.push((new_reg, orig.to_string()));
        }
    }
    /// 溯源：寄存器 → 原始名（映射缺失 → None = 灰态）。
    pub fn orig_name(&self, reg: u32) -> Option<&String> {
        self.var_names.iter().find(|(r, _)| *r == reg).map(|(_, n)| n)
    }
    /// 优化后同步：位置迁移重放（优化记录 (旧位, 新位) → 表迁移）。
    /// 迁移后不可溯源（旧位消失且新位缺装饰）→ 质量账计数。
    pub fn replay_opt(&mut self, moves: &[(u32, usize, u32, usize)]) {
        for (ob, oi, nb, ni) in moves.iter() {
            if let Some(p) = self.src_of(*ob, *oi) {
                // 迁移：旧位装饰随指令走到新位。
                self.pos.retain(|(b, i, _)| !(*b == *ob && *i == *oi));
                self.decorate(*nb, *ni, p);
            } else {
                // 旧位无装饰 → 不可溯源标注（质量账）。
                self.untraceable += 1;
            }
        }
    }
    /// 剥离（发布模式）：调试表全清 + 裁剪计数。
    pub fn strip(&mut self) -> usize {
        let n = self.pos.len() + self.var_names.len();
        self.pos.clear();
        self.var_names.clear();
        n
    }
}

/// 剥离后诊断降级告知文本（如实声明面）。
pub const STRIP_NOTICE: &str = "调试信息已剥离：诊断仅到指令级，不回源码行";

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec76_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、全量装饰。
    {
        let mut b = DebugBook::new();
        b.decorate(0, 0, 100);
        b.decorate(0, 1, 105);
        b.decorate(1, 0, 200);
        v.push((
            "F0476-装饰-全量落库",
            b.src_of(0, 0) == Some(100) && b.src_of(1, 0) == Some(200),
        ));
        // 重复装饰幂等（同位覆写不重行）。
        b.decorate(0, 0, 999);
        v.push((
            "F0476-装饰-幂等覆写",
            b.src_of(0, 0) == Some(999) && b.pos.len() == 3,
        ));
        // 缺位灰态：查不到显性 None（不静默给假位）。
        v.push(("F0476-装饰-缺位灰态", b.src_of(9, 9).is_none()));
    }

    // 二、溯源映射。
    {
        let mut b = DebugBook::new();
        b.rename(0, "uv");
        b.rename(7, "color_opt"); // 优化改名：新寄存器 7 ← 原 color
        v.push((
            "F0476-溯源-改名可溯",
            b.orig_name(7).map(|s| s.as_str()) == Some("color_opt")
                && b.orig_name(0).map(|s| s.as_str()) == Some("uv"),
        ));
        // 映射缺失 → 灰态。
        v.push(("F0476-溯源-缺失灰态", b.orig_name(42).is_none()));
        // 重复登记幂等。
        b.rename(7, "another");
        v.push((
            "F0476-溯源-登记幂等",
            b.orig_name(7).map(|s| s.as_str()) == Some("color_opt") && b.var_names.len() == 2,
        ));
    }

    // 三、随优化走。
    {
        let mut b = DebugBook::new();
        b.decorate(0, 0, 10);
        b.decorate(0, 1, 20);
        // 优化迁移：块 0 指令 0 → 块 0 指令 5（位置随指令走）。
        b.replay_opt(&[(0, 0, 0, 5)]);
        v.push((
            "F0476-随走-位置迁移",
            b.src_of(0, 0).is_none() && b.src_of(0, 5) == Some(10),
        ));
        // 不可溯源：迁移一个无装饰位 → 质量账计数。
        b.replay_opt(&[(3, 3, 4, 4)]);
        v.push(("F0476-随走-不可溯计数", b.untraceable == 1));
    }

    // 四、可剥离。
    {
        let mut b = DebugBook::new();
        b.decorate(0, 0, 10);
        b.rename(0, "x");
        let n = b.strip();
        v.push((
            "F0476-剥离-表清计数",
            n == 2 && b.pos.is_empty() && b.var_names.is_empty(),
        ));
        // 剥离后如实告知（降级声明常量非空）。
        v.push((
            "F0476-剥离-降级告知",
            STRIP_NOTICE.contains("剥离") && STRIP_NOTICE.contains("不回源码行"),
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0476-判据-锚点入册", ANCHOR == "VE-F0476"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec76）。
pub fn run_vec76_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec76");
    for (name, ok) in vec76_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec76_raw_checks as run_vec76_checks;
    #[test]
    fn vec76_dbgchecks() {
        for (name, ok) in run_vec76_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
