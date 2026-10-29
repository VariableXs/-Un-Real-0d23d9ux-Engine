//! CGPU-F0794 · E 域文档总纲（目标 340 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥340 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0794，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E 域文档总纲——E 域全部交付文档的章节树（SectionTree）：
//!   章节登记、层级约束、字数预算（整数口径）、完成度矩阵与
//!   总纲齐备性判定；文档内容不入账，只入结构与预算。
//!
//! 设计纪律：
//! - 章节层级闭集三级：L0（卷）/ L1（章）/ L2（节）；L2 必挂 L1，
//!   L1 必挂 L0，否则登记被拒；L0 只能是根。
//! - 字数预算整数口径：每节 words_budget 与 words_actual，
//!   完成判据：actual ≥ budget 且 budget ≥ 1；超预算不罚，
//!   欠额按差额计入缺欠总量。
//! - 完成度矩阵：三维计数（L0/L1/L2 各自 完成/总数），
//!   总纲齐备判据：三级完成率均为 100%（千分比口径）。
//! - 章节账本定容：SECT_CAP=96，超容逐出最旧叶子（L2）并计数，
//!   守恒恒等式：total_sections == held + evicted。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
//!   诊断段 0x6E80 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x6E80，批内 CGPU-F0794 独占）。
pub const SEG_BASE: u16 = 0x6E80;

/// 章节编号重复。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 章节账本已满且无可逐出叶子。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 层级约束非法（L1 无父卷 / L2 无父章 / L0 带父 / 父层级不匹配）。
pub const E_LEVEL: u16 = SEG_BASE + 3;
/// 父章节不存在。
pub const E_NOPARENT: u16 = SEG_BASE + 4;
/// 字数预算口径非法（0）。
pub const E_WORDS: u16 = SEG_BASE + 5;
/// 章节树为空或标题为空。
pub const E_EMPTY: u16 = SEG_BASE + 6;
/// 守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 7;

/// 章节账本容量。
pub const SECT_CAP: usize = 96;

/// 章节层级（闭集三级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// 卷。
    L0,
    /// 章。
    L1,
    /// 节。
    L2,
}

impl Level {
    pub fn ord(self) -> u8 {
        match self {
            Level::L0 => 0,
            Level::L1 => 1,
            Level::L2 => 2,
        }
    }
    /// 合法父层级：L1→L0，L2→L1，L0→无。
    pub fn parent_level(self) -> Option<Level> {
        match self {
            Level::L0 => None,
            Level::L1 => Some(Level::L0),
            Level::L2 => Some(Level::L1),
        }
    }
}

/// 章节。
#[derive(Clone, Debug)]
pub struct Section {
    /// 编号（树内唯一）。
    pub id: u32,
    /// 标题。
    pub title: String,
    /// 层级。
    pub level: Level,
    /// 父编号（L0 为 None）。
    pub parent: Option<u32>,
    /// 字数预算。
    pub words_budget: u32,
    /// 实际字数（0 = 未写）。
    pub words_actual: u32,
}

impl Section {
    /// 是否完成。
    pub fn done(&self) -> bool {
        self.words_budget >= 1 && self.words_actual >= self.words_budget
    }
    /// 欠额字数。
    pub fn shortfall(&self) -> u32 {
        self.words_budget.saturating_sub(self.words_actual)
    }
}

/// 章节树。
#[derive(Clone, Debug)]
pub struct SectionTree {
    nodes: Vec<Section>,
    evicted: u64,
    total_sections: u64,
}

impl SectionTree {
    pub fn new() -> Self {
        SectionTree {
            nodes: Vec::new(),
            evicted: 0,
            total_sections: 0,
        }
    }
    /// 层级校验：父存在且层级匹配；L0 不带父。
    pub fn add(
        &mut self,
        id: u32,
        title: &str,
        level: Level,
        parent: Option<u32>,
        words_budget: u32,
    ) -> Result<(), u16> {
        if title.is_empty() {
            return Err(E_EMPTY);
        }
        if words_budget == 0 {
            return Err(E_WORDS);
        }
        if self.nodes.iter().any(|s| s.id == id) {
            return Err(E_DUP);
        }
        match level.parent_level() {
            None => {
                if parent.is_some() {
                    return Err(E_LEVEL);
                }
            }
            Some(pl) => {
                let p = parent.ok_or(E_LEVEL)?;
                let ps = self.nodes.iter().find(|s| s.id == p).ok_or(E_NOPARENT)?;
                if ps.level != pl {
                    return Err(E_LEVEL);
                }
            }
        }
        if self.nodes.len() >= SECT_CAP {
            // 逐出最旧叶子（L2）；无叶子可逐则拒绝。
            let p = self.nodes.iter().position(|s| s.level == Level::L2);
            match p {
                Some(idx) => {
                    self.nodes.remove(idx);
                    self.evicted += 1;
                }
                None => return Err(E_CAP),
            }
        }
        self.nodes.push(Section {
            id,
            title: title.to_string(),
            level,
            parent,
            words_budget,
            words_actual: 0,
        });
        self.total_sections += 1;
        Ok(())
    }
    /// 写作进度落账。
    pub fn write(&mut self, id: u32, words: u32) -> Result<(), u16> {
        let s = self
            .nodes
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(E_NOPARENT)?;
        s.words_actual = words;
        Ok(())
    }
    /// 按层统计（闭集三级遍历）。
    pub fn count_level(&self, lv: Level) -> u32 {
        self.nodes.iter().filter(|s| s.level == lv).count() as u32
    }
    /// 完成度矩阵：[ [完成,总数] × 3 层 ]。
    pub fn progress(&self) -> Result<[[u32; 2]; 3], u16> {
        if self.nodes.is_empty() {
            return Err(E_EMPTY);
        }
        let mut m = [[0u32; 2]; 3];
        for s in &self.nodes {
            let o = s.level.ord() as usize;
            m[o][1] += 1;
            if s.done() {
                m[o][0] += 1;
            }
        }
        Ok(m)
    }
    /// 总纲齐备判据：三级全部存在且全部完成。
    pub fn master_ready(&self) -> Result<bool, u16> {
        let m = self.progress()?;
        for lvl in m.iter() {
            if lvl[1] == 0 || lvl[0] != lvl[1] {
                return Ok(false);
            }
        }
        Ok(true)
    }
    /// 欠额总量。
    pub fn shortfall_total(&self) -> u32 {
        self.nodes.iter().map(|s| s.shortfall()).sum()
    }
    /// 守恒审计：total_sections == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total_sections != self.nodes.len() as u64 + self.evicted {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    pub fn held(&self) -> usize {
        self.nodes.len()
    }
    pub fn evicted(&self) -> u64 {
        self.evicted
    }
}

/// 完成率（千分比）：done*1000/total；total==0 时返回 Err。
pub fn per_mille(done: u32, total: u32) -> Result<u32, u16> {
    if total == 0 {
        return Err(E_EMPTY);
    }
    Ok(done * 1000 / total)
}

/// 自例程：主路径（建树→写作→齐备）+ 层级错误路径 + 定容逐出守恒。
pub fn drill() -> Result<u32, u16> {
    let mut t = SectionTree::new();
    // 错误路径 0：空标题 / 零预算。
    if t.add(1, "", Level::L0, None, 100) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    if t.add(1, "卷", Level::L0, None, 0) != Err(E_WORDS) {
        return Err(E_WORDS);
    }
    // 主路径：建树。
    t.add(1, "架构卷", Level::L0, None, 200)?;
    t.add(2, "接口章", Level::L1, Some(1), 300)?;
    t.add(3, "内存节", Level::L2, Some(2), 150)?;
    t.add(4, "调度节", Level::L2, Some(2), 150)?;
    t.add(5, "迁移卷", Level::L0, None, 200)?;
    // 层级错误路径。
    if t.add(9, "孤儿章", Level::L1, Some(99), 100) != Err(E_NOPARENT) {
        return Err(E_NOPARENT);
    }
    // L2 挂 L0：层级不匹配。
    if t.add(10, "错挂节", Level::L2, Some(1), 100) != Err(E_LEVEL) {
        return Err(E_LEVEL);
    }
    // L0 带父：非法。
    if t.add(11, "带父卷", Level::L0, Some(1), 100) != Err(E_LEVEL) {
        return Err(E_LEVEL);
    }
    // L1 无父：非法。
    if t.add(12, "无父章", Level::L1, None, 100) != Err(E_LEVEL) {
        return Err(E_LEVEL);
    }
    // 重复编号。
    if t.add(1, "重号", Level::L0, None, 100) != Err(E_DUP) {
        return Err(E_DUP);
    }
    // 未写 → 不齐备。
    t.audit()?;
    if t.master_ready()? {
        return Err(E_EMPTY);
    }
    // 欠额 = 200+300+150+150+200 = 1000。
    if t.shortfall_total() != 1000 {
        return Err(E_EMPTY);
    }
    // 写作：先写满部分节。
    t.write(3, 150)?;
    t.write(4, 160)?;
    if t.shortfall_total() != 700 {
        return Err(E_EMPTY);
    }
    // 写满全部 → 齐备。
    t.write(1, 220)?;
    t.write(2, 300)?;
    t.write(5, 200)?;
    if !t.master_ready()? {
        return Err(E_EMPTY);
    }
    // 完成度矩阵：每层 done==total。
    let m = t.progress()?;
    for lvl in m.iter() {
        if per_mille(lvl[0], lvl[1])? != 1000 {
            return Err(E_EMPTY);
        }
    }
    // per_mille 边界：0/5 → 0；total=0 → Err。
    if per_mille(0, 5)? != 0 || per_mille(1, 0) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 定容逐出：建满 96 节点（1 卷 + 若干章节 + 90 叶），再入 2 叶触发逐出。
    let mut t2 = SectionTree::new();
    t2.add(100, "大卷", Level::L0, None, 100)?;
    t2.add(101, "大章", Level::L1, Some(100), 100)?;
    for i in 0..94u32 {
        t2.add(200 + i, "叶", Level::L2, Some(101), 100)?;
    }
    t2.audit()?;
    if t2.held() != SECT_CAP {
        return Err(E_CAP);
    }
    // 再入 2 叶：逐出最旧两叶（id=200、201）。
    t2.add(300, "新叶A", Level::L2, Some(101), 100)?;
    t2.add(301, "新叶B", Level::L2, Some(101), 100)?;
    t2.audit()?;
    if t2.held() != SECT_CAP || t2.evicted() != 2 || t2.count_level(Level::L2) != 94 {
        return Err(E_LEDGER);
    }
    // 无叶可逐时：96 个非叶节点 → 拒绝。
    let mut t3 = SectionTree::new();
    t3.add(1, "卷A", Level::L0, None, 10)?;
    for i in 0..95u32 {
        t3.add(10 + i, "章", Level::L1, Some(1), 10)?;
    }
    if t3.add(999, "爆", Level::L1, Some(1), 10) != Err(E_CAP) {
        return Err(E_CAP);
    }
    // 空树进度。
    if SectionTree::new().progress() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    Ok(0)
}
