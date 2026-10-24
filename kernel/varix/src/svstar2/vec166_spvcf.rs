//! VE-F0566 · SPIR-V 结构化控制流
//!
//! SPIR-V 的控制流要求结构化（merge 指令强制）：OpSelectionMerge/OpLoopMerge
//! 与 OpBranchConditional/OpBranch 的配对规则、continue target 语义、
//! SPIR-V 结构化规则（每个 header 必须配 merge，循环必须配 continue target）
//! ——与 GLSL 结构化重建（F0503）不同：SPIR-V 是在块图上加 merge 声明而非转文本。
//! 生成算法：块图分析→区域识别→merge/continue 位置确定→合法性自检预跑。
//!
//! 跨批对接：上游 F0565 指令族、F0503 重建前置；下游 F0567 验证对接。

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;

pub const OP_BRANCH: u32 = 249;
pub const OP_BRANCH_CONDITIONAL: u32 = 250;
pub const OP_SELECTION_MERGE: u32 = 247;
pub const OP_LOOP_MERGE: u32 = 246;
pub const OP_RETURN: u32 = 253;

/// 块：ID + 终结指令（块图节点）
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Terminator {
    Branch(u32),
    BranchConditional { cond: u32, true_blk: u32, false_blk: u32 },
    Return,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub id: u32,
    pub term: Terminator,
}

impl Block {
    pub fn successors(&self) -> Vec<u32> {
        match &self.term {
            Terminator::Branch(t) => vec![*t],
            Terminator::BranchConditional { true_blk, false_blk, .. } => vec![*true_blk, *false_blk],
            Terminator::Return => vec![],
        }
    }
}

/// 结构化区域
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Region {
    Selection { header: u32, merge: u32 },
    Loop { header: u32, merge: u32, continue_target: u32 },
}

/// 结构化规划器：块图 → merge/continue 位置确定
#[derive(Clone, Debug, Default)]
pub struct CfPlanner {
    pub blocks: Vec<Block>,
    pub regions: Vec<Region>,
}

impl CfPlanner {
    pub fn new() -> Self {
        CfPlanner::default()
    }

    pub fn block(&mut self, id: u32, term: Terminator) {
        self.blocks.push(Block { id, term });
    }

    pub fn find(&self, id: u32) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }

    /// 区域识别：条件分支块登记 Selection 区域（merge 块由规划器指定）
    pub fn plan_selection(&mut self, header: u32, merge: u32) -> bool {
        match self.find(header).map(|b| &b.term) {
            Some(Terminator::BranchConditional { .. }) => {
                self.regions.push(Region::Selection { header, merge });
                true
            }
            _ => false,
        }
    }

    /// 循环区域：header 必须是 Branch 且配 continue target
    pub fn plan_loop(&mut self, header: u32, merge: u32, continue_target: u32) -> bool {
        match self.find(header).map(|b| &b.term) {
            Some(Terminator::Branch(_)) => {
                self.regions.push(Region::Loop { header, merge, continue_target });
                true
            }
            _ => false,
        }
    }

    /// 合法性自检器：SPIR-V 结构化规则预跑
    /// 规则一：每个 Selection header 的后续指令必须配 OpSelectionMerge
    /// 规则二：每个 Loop header 必须配 OpLoopMerge + continue target 存在
    /// 规则三：merge/continue 目标块必须存在
    pub fn validate(&self) -> Vec<String> {
        let mut errs = Vec::new();
        for r in &self.regions {
            match r {
                Region::Selection { header, merge } => {
                    if self.find(*merge).is_none() {
                        errs.push(alloc::format!("sel-merge-missing header={}", header));
                    }
                }
                Region::Loop { header, merge, continue_target } => {
                    if self.find(*merge).is_none() {
                        errs.push(alloc::format!("loop-merge-missing header={}", header));
                    }
                    if self.find(*continue_target).is_none() {
                        errs.push(alloc::format!("loop-continue-missing header={}", header));
                    }
                }
            }
        }
        // 后继块存在性
        for b in &self.blocks {
            for succ in b.successors() {
                if self.find(succ).is_none() {
                    errs.push(alloc::format!("succ-missing from={} to={}", b.id, succ));
                }
            }
        }
        errs
    }

    /// 配对发射：header 块的指令序 = merge 指令 + 分支指令
    pub fn emit_header_seq(&self, header: u32) -> Vec<u32> {
        let mut out = Vec::new();
        for r in &self.regions {
            match r {
                Region::Selection { header: h, merge } if *h == header => {
                    out.push(OP_SELECTION_MERGE);
                    out.push(*merge);
                }
                Region::Loop { header: h, merge, continue_target } if *h == header => {
                    out.push(OP_LOOP_MERGE);
                    out.push(*merge);
                    out.push(*continue_target);
                }
                _ => {}
            }
        }
        match self.find(header).map(|b| &b.term) {
            Some(Terminator::Branch(_)) => out.push(OP_BRANCH),
            Some(Terminator::BranchConditional { .. }) => out.push(OP_BRANCH_CONDITIONAL),
            _ => {}
        }
        out
    }

    /// 块图全覆盖：所有块可达性检查（从入口做图遍历）
    pub fn unreachable_from(&self, entry: u32) -> Vec<u32> {
        let mut seen = vec![entry];
        let mut queue = vec![entry];
        while let Some(id) = queue.pop() {
            if let Some(b) = self.find(id) {
                for succ in b.successors() {
                    if !seen.contains(&succ) {
                        seen.push(succ);
                        queue.push(succ);
                    }
                }
            }
        }
        self.blocks.iter().filter(|b| !seen.contains(&b.id)).map(|b| b.id).collect()
    }
}

pub fn run_vec166_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec166");
    // 族一：块与后继
    let mut p = CfPlanner::new();
    p.block(1, Terminator::BranchConditional { cond: 9, true_blk: 2, false_blk: 3 });
    p.block(2, Terminator::Branch(4));
    p.block(3, Terminator::Branch(4));
    p.block(4, Terminator::Return);
    s.add("块-条件后继二元", p.find(1).unwrap().successors() == vec![2, 3], "succ2");
    s.add("块-分支后继一元", p.find(2).unwrap().successors() == vec![4], "succ1");
    s.add("块-Return无后继", p.find(4).unwrap().successors().is_empty(), "ret");
    // 族二：区域识别
    s.add("区域-Selection登记", p.plan_selection(1, 4), "sel");
    s.add("区域-非条件头拒绝", !p.plan_selection(2, 4), "nosel");
    s.add("区域-Loop登记", p.plan_loop(2, 4, 3), "loop");
    s.add("区域-Return头拒绝", !p.plan_loop(4, 4, 4), "ret-rej");
    // 族三：合法性自检
    s.add("自检-合法图零错", p.validate().is_empty(), "ok");
    // 缺失 merge 块
    let mut bad = CfPlanner::new();
    bad.block(1, Terminator::BranchConditional { cond: 9, true_blk: 2, false_blk: 3 });
    bad.block(2, Terminator::Return);
    bad.block(3, Terminator::Return);
    bad.plan_selection(1, 9);
    s.add("自检-merge缺失检出", !bad.validate().is_empty(), "nomerge");
    // 缺失 continue
    let mut bad2 = CfPlanner::new();
    bad2.block(1, Terminator::Branch(2));
    bad2.block(2, Terminator::Return);
    bad2.plan_loop(1, 2, 9);
    s.add("自检-continue缺失检出", bad2.validate().len() == 1, "nocont");
    // 后继缺失
    let mut bad3 = CfPlanner::new();
    bad3.block(1, Terminator::Branch(7));
    s.add("自检-后继缺失检出", bad3.validate().len() == 1, "nosucc");
    // 族四：配对发射
    let seq = p.emit_header_seq(1);
    s.add("发射-Selection配对", seq == vec![OP_SELECTION_MERGE, 4, OP_BRANCH_CONDITIONAL], "sel-seq");
    let seq2 = p.emit_header_seq(2);
    s.add("发射-Loop配对含continue", seq2 == vec![OP_LOOP_MERGE, 4, 3, OP_BRANCH], "loop-seq");
    s.add("发射-非头块空序", p.emit_header_seq(4).is_empty(), "none");
    // 族五：块图全覆盖
    let unr = p.unreachable_from(1);
    s.add("覆盖-全可达", unr.is_empty(), "reach");
    let mut disc = CfPlanner::new();
    disc.block(1, Terminator::Branch(2));
    disc.block(2, Terminator::Return);
    disc.block(5, Terminator::Return);
    s.add("覆盖-孤块检出", disc.unreachable_from(1) == vec![5], "orphan");
    // 族六：merge 指令码
    s.add("码-SelectionMerge", OP_SELECTION_MERGE == 247, "247");
    s.add("码-LoopMerge", OP_LOOP_MERGE == 246, "246");
    s.add("码-BranchCond", OP_BRANCH_CONDITIONAL == 250, "250");
    s
}
