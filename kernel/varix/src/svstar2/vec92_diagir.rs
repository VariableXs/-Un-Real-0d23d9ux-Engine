//! VE-F0492 · 编译错误诊断增强（IR 级）（VE-C 域 · 优化组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0492`
//!
//! **锚点原文**：「IR 级诊断增强：优化与后端期错误的源码定位（经 F0476
//! 调试装饰回溯到源行列）、诊断质量（三要素加优化上下文说明——哪个 pass
//! 在什么变换后发现）、诊断限频合并（同类错误合并呈现）。数据结构：回溯
//! 链（IR 位置到源位置）；上下文说明模板；合并器。错误路径与降级矩阵：
//! 装饰缺失→降级 IR 位置呈现并标注；上下文模板缺失→最小告知；合并误并
//! →阈值调保守。判据：装饰回溯、pass 上下文、限频合并、降级标注、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **装饰回溯**（判据一）。IR 位置 → 源行列（经调试装饰表回溯）；
//!    装饰缺失 → 降级 IR 位置呈现并显性标注「无源映射」。
//!
//! 2. **pass 上下文**（判据二）。三要素（位置 + 代码 + 说明）加优化上下
//!    文（哪个 pass 在什么变换后发现）；模板缺失 → 最小告知降级。
//!
//! 3. **限频合并**（判据三）。同类错误合并呈现（同类键 = pass + 错误码）；
//!    合并阈值保守（计数达标才合并，不误并）。
//!
//! 4. **降级标注**（判据四）。一切降级路径显性标注（不冒充完整诊断）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0492";

/// 同类合并阈值（保守：同键达到 3 条才合并）。
pub const MERGE_THRESHOLD: usize = 3;

/// 源位置（行列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SrcPos {
    pub line: u32,
    pub col: u32,
}

/// IR 位置（块序 + 指令序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IrPos {
    pub block: u32,
    pub insn: u32,
}

/// 一条 IR 级诊断。
#[derive(Clone, Debug, PartialEq)]
pub struct Diag {
    pub ir: IrPos,
    /// 源位置（装饰回溯结果；None = 装饰缺失降级）。
    pub src: Option<SrcPos>,
    /// 三要素之代码。
    pub code: u16,
    /// 发现它的 pass。
    pub pass: &'static str,
    /// 上下文说明（什么变换后发现）。
    pub context: Option<&'static str>,
    /// 降级标注（非空 = 处于降级呈现）。
    pub degraded: bool,
}

/// 调试装饰表：IR 位置 → 源位置（F0476 装饰）。
#[derive(Debug, Default)]
pub struct DecorTable {
    pub map: Vec<(IrPos, SrcPos)>,
}

impl DecorTable {
    /// 回溯：O(1) 面语义（小表线性即可），缺装饰返回 None（降级触发面）。
    pub fn lookup(&self, pos: IrPos) -> Option<SrcPos> {
        self.map.iter().find(|(p, _)| *p == pos).map(|(_, s)| *s)
    }
}

/// 诊断发射器（上下文 + 限频合并）。
#[derive(Debug, Default)]
pub struct DiagSink {
    pub out: Vec<Diag>,
    /// 合并账：同类键 → 合并计数。
    pub merged: Vec<(String, usize)>,
    /// 模板缺失降级计数。
    pub min_notice: usize,
}

impl DiagSink {
    /// 发射：装饰回溯 + pass 上下文 + 限频合并。
    pub fn emit(
        &mut self,
        decor: &DecorTable,
        ir: IrPos,
        code: u16,
        pass: &'static str,
        context: Option<&'static str>,
    ) -> Diag {
        let (src, degraded) = match decor.lookup(ir) {
            Some(s) => (Some(s), false),
            // 装饰缺失 → 降级 IR 位置呈现并标注。
            None => (None, true),
        };
        // 上下文模板缺失 → 最小告知降级。
        let degraded = degraded || context.is_none();
        if context.is_none() {
            self.min_notice += 1;
        }
        let d = Diag { ir, src, code, pass, context, degraded };
        // 限频合并：同键（pass + code）达到阈值才合并呈现。
        let key = format!("{}#{}", pass, code);
        let same = self.out.iter().filter(|x| x.pass == pass && x.code == code).count();
        if same + 1 >= MERGE_THRESHOLD {
            match self.merged.iter_mut().find(|(k, _)| *k == key) {
                Some((_, c)) => *c += 1,
                None => self.merged.push((key, 1)),
            }
        }
        let ret = d.clone();
        self.out.push(d);
        ret
    }

    /// 合并呈现文本（同类键合并计数说明）。
    pub fn merge_summary(&self) -> Vec<String> {
        self.merged.iter().map(|(k, c)| format!("{} ×{}", k, c)).collect()
    }
}

// ---------------------------------------------------------------------------
// 判据（F0492-xxx）
// ---------------------------------------------------------------------------

/// 原始判据。
pub fn vec92_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、装饰回溯。
    {
        let mut decor = DecorTable::default();
        decor.map.push((IrPos { block: 1, insn: 2 }, SrcPos { line: 10, col: 4 }));
        let hit = decor.lookup(IrPos { block: 1, insn: 2 });
        v.push((
            "F0492-回溯-命中源行列",
            hit == Some(SrcPos { line: 10, col: 4 }),
        ));
        let miss = decor.lookup(IrPos { block: 9, insn: 9 });
        v.push(("F0492-回溯-缺装饰None", miss.is_none()));
    }

    // 二、pass 上下文。
    {
        let mut sink = DiagSink::default();
        let d = sink.emit(&DecorTable::default(), IrPos { block: 0, insn: 0 }, 0x21, "dce", Some("常量折叠后死代码"));
        v.push((
            "F0492-上下文-带变换说明",
            d.context == Some("常量折叠后死代码") && !d.degraded && d.src.is_none(),
        ));
        // 模板缺失 → 最小告知降级。
        let d2 = sink.emit(&DecorTable::default(), IrPos { block: 0, insn: 1 }, 0x21, "dce", None);
        v.push((
            "F0492-上下文-缺失最小告知",
            d2.context.is_none() && d2.degraded && sink.min_notice == 1,
        ));
    }

    // 三、限频合并。
    {
        let mut sink = DiagSink::default();
        let decor = DecorTable::default();
        for i in 0..3u32 {
            let _ = sink.emit(&decor, IrPos { block: i, insn: 0 }, 0x31, "cse", Some("公共子表达式"));
        }
        // 3 条同键 → 合并账记录 1 组（第 3 条达标时入账）。
        v.push((
            "F0492-合并-达标入账",
            sink.merged.len() == 1 && sink.merge_summary()[0].contains("cse#49 ×1"),
        ));
        // 第 4 条同键 → 计数递增。
        let _ = sink.emit(&decor, IrPos { block: 3, insn: 0 }, 0x31, "cse", Some("公共子表达式"));
        v.push(("F0492-合并-计数递增", sink.merged[0].1 == 2));
        // 不同键不合并不误并。
        let _ = sink.emit(&decor, IrPos { block: 4, insn: 0 }, 0x32, "cse", Some("另码"));
        v.push(("F0492-合并-异键不误并", sink.merged.len() == 1));
    }

    // 四、降级标注。
    {
        let mut sink = DiagSink::default();
        let d = sink.emit(&DecorTable::default(), IrPos { block: 7, insn: 1 }, 0x41, "fold", Some("折叠"));
        v.push((
            "F0492-降级-缺装饰显性",
            d.degraded && d.src.is_none() && d.ir == IrPos { block: 7, insn: 1 },
        ));
        v.push(("F0492-判据-锚点入册", ANCHOR == "VE-F0492"));
    }

    v
}

/// 注册表入口。
pub fn run_vec92_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec92");
    for (name, ok) in vec92_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec92_raw_checks as run_vec92_checks;
    #[test]
    fn vec92_diagir_checks() {
        for (name, ok) in run_vec92_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
