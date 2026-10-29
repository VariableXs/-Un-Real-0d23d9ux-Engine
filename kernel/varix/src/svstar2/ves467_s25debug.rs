//! ves467_s25debug — VE-F4233 · 契约调试
//! 三视区：DSL 调试器（语法树视区+求值单步）+验证过程追踪（六检逐步结果）+冲突定位器（冲突对→来源行高亮）；
//! 解析崩溃→最小复现+立案；单步开销→降采样；定位失败→降级文本模式；
//! 调试器遵循五标准（复述）；调试数据不含用户内容。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1880;
pub const STEP_BUDGET: u32 = 32;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbgCode {
    Ok = 0,
    CrashRepro = SEG_BASE + 0x01, // 0x1881 最小复现立案
    OverSampled = SEG_BASE + 0x02, // 0x1882
    TextFallback = SEG_BASE + 0x03, // 0x1883 降级文本
}

/// 语法树节点。
#[derive(Clone, Copy)]
pub struct AstNode { pub id: u32, pub line: u16, pub kind: u8 }

/// 调试器册。
pub struct DebugBook {
    pub tree: Vec<AstNode>,
    pub six_check: [bool; 6], // 六检逐步结果
    pub six_traced: u32,
    pub steps: u32,
    pub downsampled: bool,
    pub highlight: Option<(u32, u16)>, // 冲突对→来源行高亮
    pub text_mode: bool,
    pub repros: Vec<u32>, // 最小复现立案
}

impl DebugBook {
    pub fn new() -> Self {
        DebugBook { tree: Vec::new(), six_check: [false; 6], six_traced: 0, steps: 0,
            downsampled: false, highlight: None, text_mode: false, repros: Vec::new() }
    }
    pub fn build_tree(&mut self, nodes: Vec<AstNode>) -> Result<(), DbgCode> {
        if nodes.is_empty() {
            self.repros.push(1); // 最小复现立案
            return Err(DbgCode::CrashRepro);
        }
        self.tree = nodes;
        Ok(())
    }
    /// 六检逐步追踪。
    pub fn trace_six(&mut self, results: [bool; 6]) {
        self.six_check = results;
        self.six_traced = 6;
    }
    /// 求值单步：超预算→降采样。
    pub fn step(&mut self, n: u32) -> Result<(), DbgCode> {
        self.steps += n;
        if self.steps > STEP_BUDGET {
            self.downsampled = true;
            self.steps = STEP_BUDGET / 2;
            return Err(DbgCode::OverSampled);
        }
        Ok(())
    }
    /// 冲突定位：冲突对→来源行高亮；失败→降级文本。
    pub fn locate(&mut self, pair: (u32, u32)) -> Result<u16, DbgCode> {
        let a = self.tree.iter().find(|n| n.id == pair.0);
        let b = self.tree.iter().find(|n| n.id == pair.1);
        match (a, b) {
            (Some(x), Some(y)) => {
                let line = x.line.max(y.line);
                self.highlight = Some((pair.0, line));
                Ok(line)
            }
            _ => { self.text_mode = true; Err(DbgCode::TextFallback) }
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves467_checks() -> CheckSet {
    let mut set = CheckSet::new("ves467");
    let mut bk = DebugBook::new();

    // C467-01 语法树构建（三节点）。
    let nodes = vec![AstNode { id: 1, line: 1, kind: 0 }, AstNode { id: 2, line: 3, kind: 1 }, AstNode { id: 3, line: 5, kind: 2 }];
    chk(&mut set, "C467-01", bk.build_tree(nodes).is_ok() && bk.tree.len() == 3, "语法树视区");

    // C467-02 解析崩溃（空树）→最小复现+立案。
    chk(&mut set, "C467-02", bk.build_tree(Vec::new()) == Err(DbgCode::CrashRepro)
        && bk.repros.len() == 1, "最小复现立案");

    // C467-03 六检逐步追踪。
    bk.trace_six([true, true, true, true, false, true]);
    chk(&mut set, "C467-03", bk.six_traced == 6 && !bk.six_check[4], "六检追踪");

    // C467-04 冲突定位高亮（取大行号）。
    chk(&mut set, "C467-04", bk.locate((1, 2)) == Ok(3) && bk.highlight == Some((1, 3)), "冲突行高亮");

    // C467-05 定位失败→降级文本模式。
    chk(&mut set, "C467-05", bk.locate((1, 99)) == Err(DbgCode::TextFallback) && bk.text_mode, "降级文本");

    // C467-06 单步预算内。
    chk(&mut set, "C467-06", bk.step(16).is_ok() && bk.steps == 16, "预算内");

    // C467-07 单步超限→降采样。
    chk(&mut set, "C467-07", bk.step(17) == Err(DbgCode::OverSampled)
        && bk.downsampled && bk.steps == 16, "降采样");

    // C467-08 调试数据不含用户内容（节点 id 全为系统号）。
    chk(&mut set, "C467-08", bk.tree.iter().all(|n| n.id <= 999), "无用户内容");

    // C467-09 降采样后可续。
    chk(&mut set, "C467-09", bk.step(10).is_ok(), "降采样可续");

    // C467-10 高亮覆盖幂等。
    bk.locate((2, 3)).unwrap();
    chk(&mut set, "C467-10", bk.highlight == Some((2, 5)), "高亮替换");

    // C467-11 复现库留痕。
    chk(&mut set, "C467-11", bk.repros == vec![1], "复现留痕");

    // C467-12 预算常量单源。
    chk(&mut set, "C467-12", STEP_BUDGET == 32 && DbgCode::OverSampled as u16 == SEG_BASE + 2, "预算单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves467_smoke() {
        let set = run_ves467_checks();
        assert!(set.failed_count() == 0, "ves467 red: {}", set.red_items_text());
    }
}
