//! VE-F4285 · 一致性×设计工具（U05 工具集成组）
//! 职责：设计稿→令牌校验（间距/色彩/字号对令牌表逐层核验）+ 设计走查
//! 清单按组件树自动生成 + 设计-代码令牌漂移双向对拍（超阈出双向整改单）
//! + 解析失败人工上传兜底 + 设计稿无障碍标注纳入核验（域本色）。
//! 段位：0x2A20（续表 idx519）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2A20;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DsCode {
    /// 解析失败（人工上传兜底）。
    ParseFail = SEG_BASE,
    /// 令牌版本失配。
    TokVerMismatch = SEG_BASE + 0x01,
    /// 漂移超阈（双向整改单）。
    DriftOver = SEG_BASE + 0x02,
}

/// 设计轴三类。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DsAxis {
    Space = 0,
    Color = 1,
    Font = 2,
}
pub const AXIS_N: usize = 3;

/// 设计图层。
#[derive(Clone, Copy, Debug)]
pub struct DesignLayer {
    pub axis: DsAxis,
    pub tok_id: u16,
    pub design_val: u32,
    pub a11y_annotated: bool, // 设计稿无障碍标注
}

/// 组件树节点。
#[derive(Clone, Copy, Debug)]
pub struct CompNode {
    pub id: u16,
    pub parent: Option<u16>,
}

/// 设计工具集成器。
pub struct DesignBridge {
    /// 令牌表：(id, 轴, 值, 版本)。
    pub tokens: Vec<(u16, DsAxis, u32, u32)>,
    pub layers: Vec<DesignLayer>,
    pub tree: Vec<CompNode>,
    pub cur_ver: u32,
    /// 双向整改单：(设计侧需改, 代码侧需改)。
    pub fix_design: Vec<u16>,
    pub fix_code: Vec<u16>,
    /// 漂移阈值 bp。
    pub drift_cap_bp: u32,
}

impl DesignBridge {
    pub fn new(cur_ver: u32, drift_cap_bp: u32) -> Self {
        DesignBridge { tokens: Vec::new(), layers: Vec::new(), tree: Vec::new(), cur_ver, fix_design: Vec::new(), fix_code: Vec::new(), drift_cap_bp }
    }

    pub fn add_tok(&mut self, id: u16, axis: DsAxis, val: u32) {
        if !self.tokens.iter().any(|(i, _, _, _)| *i == id) {
            self.tokens.push((id, axis, val, self.cur_ver));
        }
    }

    pub fn add_layer(&mut self, l: DesignLayer) {
        self.layers.push(l);
    }

    /// 解析（返回 Err=解析失败→人工上传兜底）。
    pub fn parse(&mut self, ok: bool) -> Result<usize, DsCode> {
        if !ok {
            return Err(DsCode::ParseFail);
        }
        Ok(self.layers.len())
    }

    /// 逐层令牌校验：轴匹配+值比对+版本对拍。
    pub fn verify(&mut self) -> Result<(usize, usize), DsCode> {
        if self.tokens.iter().any(|(_, _, _, v)| *v != self.cur_ver) {
            return Err(DsCode::TokVerMismatch);
        }
        let mut pass = 0;
        let mut fail = 0;
        for l in &self.layers {
            match self.tokens.iter().find(|(i, _, _, _)| *i == l.tok_id) {
                Some((_, a, tv, _)) if *a == l.axis && *tv == l.design_val => pass += 1,
                Some(_) => fail += 1,
                None => fail += 1,
            }
        }
        Ok((pass, fail))
    }

    /// 漂移双向对拍：设计值 vs 代码值，超阈出双向整改单。
    pub fn drift_check(&mut self, tok: u16, design_val: u32, code_val: u32) -> bool {
        let base = design_val.max(code_val).max(1);
        let diff_bp = design_val.abs_diff(code_val) * 10_000 / base;
        if diff_bp > self.drift_cap_bp {
            if !self.fix_design.contains(&tok) {
                self.fix_design.push(tok);
            }
            if !self.fix_code.contains(&tok) {
                self.fix_code.push(tok);
            }
            return true;
        }
        false
    }

    /// 走查清单生成：按组件树展开（父先子后）。
    pub fn walk_list(&self) -> Vec<u16> {
        let mut out = Vec::new();
        // 先根后子（按 parent 链序）
        for n in self.tree.iter() {
            if n.parent.is_none() {
                out.push(n.id);
            }
        }
        for n in self.tree.iter() {
            if n.parent.is_some() && !out.contains(&n.id) {
                out.push(n.id);
            }
        }
        out
    }

    /// 无障碍标注覆盖（域本色）。
    pub fn a11y_covered(&self) -> bool {
        !self.layers.is_empty() && self.layers.iter().all(|l| l.a11y_annotated)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C519-01..12。
pub fn run_ves519_checks() -> CheckSet {
    let mut set = CheckSet::new("ves519");

    let mut db = DesignBridge::new(3, 500);
    db.add_tok(1, DsAxis::Space, 8);
    db.add_tok(2, DsAxis::Color, 0xFF);
    db.add_tok(3, DsAxis::Font, 14);

    chk(&mut set, "C519-01", db.parse(false) == Err(DsCode::ParseFail), "解析失败→人工上传兜底");
    db.add_layer(DesignLayer { axis: DsAxis::Space, tok_id: 1, design_val: 8, a11y_annotated: true });
    db.add_layer(DesignLayer { axis: DsAxis::Color, tok_id: 2, design_val: 0xEE, a11y_annotated: true });
    db.add_layer(DesignLayer { axis: DsAxis::Font, tok_id: 9, design_val: 14, a11y_annotated: false });
    chk(&mut set, "C519-02", db.parse(true) == Ok(3), "解析成功三图层");

    let (p, f) = db.verify().unwrap_or((0, 0));
    chk(&mut set, "C519-03", p == 1 && f == 2, "逐层核验：1 过（间距）2 失（色值漂移+未知令牌）");
    chk(&mut set, "C519-04", !db.a11y_covered(), "无障碍标注未覆盖检出（域本色）");

    // 令牌版本失配
    db.cur_ver = 4;
    chk(&mut set, "C519-05", db.verify() == Err(DsCode::TokVerMismatch), "令牌版本失配拒验");
    db.cur_ver = 3;

    // 漂移双向
    chk(&mut set, "C519-06", db.drift_check(2, 0xFF, 0x00), "色值超阈漂移检出");
    chk(&mut set, "C519-07", !db.drift_check(1, 8, 8), "零漂移放行");
    chk(&mut set, "C519-08", db.fix_design == alloc::vec![2] && db.fix_code == alloc::vec![2], "双向整改单出两份");
    chk(&mut set, "C519-09", !db.drift_check(3, 100, 104), "4% 漂移在阈内放行");

    // 走查清单
    db.tree.push(CompNode { id: 100, parent: None });
    db.tree.push(CompNode { id: 101, parent: Some(100) });
    db.tree.push(CompNode { id: 102, parent: Some(100) });
    let wl = db.walk_list();
    chk(&mut set, "C519-10", wl == alloc::vec![100, 101, 102], "清单父先子后展开");

    // 修复后
    let d2 = db.layers.iter_mut().find(|l| l.tok_id == 9).unwrap();
    d2.a11y_annotated = true;
    drop(d2);
    let l2 = db.layers.iter_mut().find(|l| l.tok_id == 2).unwrap();
    l2.design_val = 0xFF;
    drop(l2);
    let (p2, f2) = db.verify().unwrap_or((0, 0));
    chk(&mut set, "C519-11", p2 == 2 && f2 == 1, "整改后色值过、未知令牌仍失");
    chk(&mut set, "C519-12", db.a11y_covered(), "标注补齐后覆盖（域本色）");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn design_bridge() {
        let mut db = DesignBridge::new(1, 500);
        db.add_tok(1, DsAxis::Space, 8);
        db.add_layer(DesignLayer { axis: DsAxis::Space, tok_id: 1, design_val: 8, a11y_annotated: true });
        assert_eq!(db.parse(true), Ok(1));
        assert_eq!(db.verify(), Ok((1, 0)));
        assert!(!db.drift_check(1, 8, 20));
        assert_eq!(db.fix_design.len(), 1);
    }
}
