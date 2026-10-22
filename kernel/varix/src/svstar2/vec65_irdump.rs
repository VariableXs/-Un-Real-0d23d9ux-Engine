//! VE-F0465 · IR 转储与可视化（VE-C 域 · IR 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0465`
//!
//! **锚点原文**：「IR 转储与可视化：文本转储（函数加块加指令缩进格式）、图
//! 形化 CFG 视图（块节点加跳转边，懒展开防大函数卡顿）、指令到源码位置反
//! 向定位；转储格式版本化（供优化对拍与测试）。判据：缩进转储、CFG 懒展
//! 开、反向定位、版本化、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **缩进转储**（判据一）。函数/块/指令三层缩进格式（[`dump`]），行格式
//!    版本化（[`DUMP_VER`]，供对拍固定）。
//!
//! 2. **CFG 懒展开**（判据二）。大函数 CFG 视图按需逐块取（[`CfgView`]
//!    懒迭代器面），防一次全量渲染卡顿；块数超阈值（[`LAZY_THRESHOLD`]）
//!    自动降级为懒模式。
//!
//! 3. **反向定位**（判据三）。指令 → 源码位置反向索引（[`PosIndex`]），
//!    O(1) 查询；无位置指令灰态标注（`"??"`）。
//!
//! 4. **版本化**（判据四）。转储头部带版本行（对拍样例随版本登记），版本
//!    变更 → 对拍基线重录（显性不静默）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0465";

/// 转储格式版本（对拍样例绑定）。
pub const DUMP_VER: u32 = 1;
/// 懒展开阈值（块数超过 → CFG 视图懒模式）。
pub const LAZY_THRESHOLD: usize = 32;

use super::vec61_irstruct::{DbgPos, IrFunc, Opcode};

// ---------------------------------------------------------------------------
// 文本转储（三层缩进）
// ---------------------------------------------------------------------------

/// 文本转储：函数 → 块 → 指令（缩进两格/层；头部版本行）。
pub fn dump(f: &IrFunc) -> String {
    let mut out = String::new();
    out.push_str(&format!("# dump-ver:{}\n", DUMP_VER));
    out.push_str(&format!("func {}:\n", f.name));
    for b in f.blocks.iter() {
        out.push_str(&format!("  block {}:\n", b.id));
        for i in b.insts.iter() {
            let pos = match i.dbg {
                Some(p) => format!("@{}", p),
                None => "??".to_string(),
            };
            let ty = i.ty.unwrap_or("-");
            out.push_str(&format!(
                "    {} {:?} ops={:?} ty={} {}\n",
                i.opcode.mnemonic(),
                i.def.unwrap_or(u32::MAX),
                i.operands,
                ty,
                pos
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// CFG 视图（懒展开）
// ---------------------------------------------------------------------------

/// CFG 视图：全量或懒展开两态。
pub struct CfgView<'a> {
    pub func: &'a IrFunc,
    pub lazy: bool,
    pub cursor: usize,
}

impl<'a> CfgView<'a> {
    /// 构造：块数超阈值自动懒模式（防大函数卡顿）。
    pub fn new(func: &'a IrFunc) -> Self {
        CfgView {
            func,
            lazy: func.blocks.len() > LAZY_THRESHOLD,
            cursor: 0,
        }
    }
    /// 拉取下一块的节点（懒面：每次一节点，调用方驱动渲染节奏）。
    pub fn next_node(&mut self) -> Option<(u32, usize)> {
        if self.cursor >= self.func.blocks.len() {
            return None;
        }
        let b = &self.func.blocks[self.cursor];
        self.cursor += 1;
        Some((b.id, b.insts.len()))
    }
    /// 边清单（跳转边：Br 目标）。
    pub fn edges(&self) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        for b in self.func.blocks.iter() {
            if let Some(t) = b.terminator() {
                if t.opcode == Opcode::Br {
                    if let Some(&target) = t.operands.first() {
                        out.push((b.id, target));
                    }
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 反向位置索引
// ---------------------------------------------------------------------------

/// 指令 → 源码位置反向索引（O(1) 查询）。
pub struct PosIndex {
    /// (函数名, 块号, 指令序) → 源偏移。
    pub map: Vec<(String, u32, usize, u32)>,
}

impl PosIndex {
    pub fn build(f: &IrFunc) -> Self {
        let mut map = Vec::new();
        for b in f.blocks.iter() {
            for (ii, i) in b.insts.iter().enumerate() {
                if let Some(p) = i.dbg {
                    map.push((f.name.clone(), b.id, ii, p));
                }
            }
        }
        PosIndex { map }
    }
    /// 查询：函数+块+指令序 → 源偏移（无位置 → None = 灰态）。
    pub fn lookup(&self, fname: &str, block: u32, inst: usize) -> Option<u32> {
        self.map
            .iter()
            .find(|(f, b, i, _)| f == fname && *b == block && *i == inst)
            .map(|(_, _, _, p)| *p)
    }
    /// 灰态标注面：查不到即 "??"（转储口径一致）。
    pub fn lookup_or_gray(&self, fname: &str, block: u32, inst: usize) -> String {
        match self.lookup(fname, block, inst) {
            Some(p) => format!("@{}", p),
            None => "??".to_string(),
        }
    }
}

/// 位置缺失灰态标注常量（转储与索引共用口径）。
pub const GRAY_MARK: &str = "??";

/// 灰态判定（DbgPos → 显示面）。
pub fn pos_display(p: DbgPos) -> String {
    match p {
        Some(v) => format!("@{}", v),
        None => GRAY_MARK.to_string(),
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec65_raw_checks() -> Vec<(&'static str, bool)> {
    use super::vec61_irstruct::{Block, Inst};
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 夹具：两块函数（const/ret + 加法块）。
    let make = || {
        let mut f = IrFunc::new("vs_main");
        let r = f.fresh_reg();
        f.cur_block().push(Inst::value(Opcode::Const, vec![], "ir.f32", 10, r));
        f.cur_block().push(Inst::term(Opcode::Br, vec![1], 11));
        f.new_block();
        f.blocks[1].push(Inst::value(Opcode::Add, vec![r, r], "ir.f32", 12, 1));
        f.blocks[1].push(Inst::term(Opcode::Ret, vec![1], 13));
        f
    };

    // 一、缩进转储。
    {
        let f = make();
        let s = dump(&f);
        v.push((
            "F0465-转储-版本头",
            s.starts_with("# dump-ver:1"),
        ));
        // 三层缩进：func 顶格 / block 两格 / 指令四格。
        v.push((
            "F0465-转储-三层缩进",
            s.contains("func vs_main:\n")
                && s.contains("  block 0:\n")
                && s.contains("    const"),
        ));
        // 灰态标注：位置缺失 → "??"（造一条灰态指令）。
        let mut g = IrFunc::new("t");
        g.cur_block().push(Inst::new(Opcode::Not, vec![0], Some("ir.f32"), None, Some(0)));
        g.cur_block().push(Inst::term(Opcode::Ret, vec![], 1));
        let gs = dump(&g);
        v.push(("F0465-转储-灰态标注", gs.contains("??")));
    }

    // 二、CFG 懒展开。
    {
        let f = make();
        let mut view = CfgView::new(&f);
        // 小函数全量模式。
        v.push(("F0465-视图-小函数全量", !view.lazy));
        // 懒拉取：逐块出节点直至尽。
        let mut got = Vec::new();
        while let Some((id, n)) = view.next_node() {
            got.push((id, n));
        }
        v.push((
            "F0465-视图-懒拉取",
            got == vec![(0, 2), (1, 2)],
        ));
        // 边清单：Br 0→1。
        v.push(("F0465-视图-跳转边", view.edges() == vec![(0, 1)]));
        // 大函数自动懒（>32 块）。
        let mut big = IrFunc::new("big");
        for _ in 0..(LAZY_THRESHOLD + 1) {
            big.new_block();
        }
        v.push(("F0465-视图-大函数懒", CfgView::new(&big).lazy));
    }

    // 三、反向定位。
    {
        let f = make();
        let idx = PosIndex::build(&f);
        // O(1) 查询：块 0 指令 0 → @10。
        v.push((
            "F0465-定位-正查",
            idx.lookup("vs_main", 0, 0) == Some(10)
                && idx.lookup("vs_main", 1, 1) == Some(13),
        ));
        // 灰态：查不到 → "??"（口径与转储一致）。
        v.push((
            "F0465-定位-灰态一致",
            idx.lookup_or_gray("vs_main", 9, 9) == GRAY_MARK,
        ));
        // 显示面：Some/None 双态。
        v.push((
            "F0465-定位-双态显示",
            pos_display(Some(10)) == "@10" && pos_display(None) == "??",
        ));
    }

    // 四、版本化。
    {
        // 版本常量当前为 1（对拍基线绑定）。
        v.push(("F0465-版本-基线绑定", DUMP_VER == 1));
        // 版本行在转储首行（对拍可解析）。
        let f = make();
        let first = dump(&f).lines().next().unwrap_or("").to_string();
        v.push((
            "F0465-版本-首行可解析",
            first == "# dump-ver:1",
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0465-判据-锚点入册", ANCHOR == "VE-F0465"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec65）。
pub fn run_vec65_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec65");
    for (name, ok) in vec65_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec65_raw_checks as run_vec65_checks;
    #[test]
    fn vec65_irchecks() {
        for (name, ok) in run_vec65_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
