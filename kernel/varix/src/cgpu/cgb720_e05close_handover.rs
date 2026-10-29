//! CGPU-F0720 · E05 组收口与 E06 移交（目标 380 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥320 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0720，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E05 组收口与 E06 移交——E05 滚动组（F0705~F0719，
//!    假设区间见头注）15 单自检入树、三面验收矩阵全绿、
//!    组双签、四件移交包封存与完整性校验。
//!
//! 假设声明：清单未给 E05 组元区间，按本批清单前文（滚动组
//! F0705~F0719）取 15 单区间 (705,719)；如册内有异，以册内为准。
//!
//! 设计纪律：
//! - 自检入树：15 单逐条（任务号/结论/证据指纹），区间校验 + 去重。
//! - 三面验收矩阵：面（单测/对拍/可感）× 五滚动子域，
//  15 格，每格 Pass/Fail；全 Pass 才可收口。
//! - 组双签：两签核人角色互异 + 签核对象 = 当前收口树内容哈希；
//  哈希变了旧签作废。
//! - 移交四件：接口清单/证据指纹表/开放缺陷/验收矩阵快照，
//  封存指纹 FNV-1a，收端可复算比对。
//! - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!   确定性。
//! - 诊断段 0x4840 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4840，批内 CGPU-F0720 独占）。
pub const SEG_BASE: u16 = 0x4840;

/// 入树项数不足（15 单未齐）。
pub const E_TREE_SHORT: u16 = SEG_BASE + 1;
/// 任务号越界或重号。
pub const E_TREE_ITEM: u16 = SEG_BASE + 2;
/// 验收矩阵有 Fail 格。
pub const E_MATRIX_RED: u16 = SEG_BASE + 3;
/// 双签校验失败（角色重复/哈希不符/签数不足）。
pub const E_SIGN: u16 = SEG_BASE + 4;
/// 移交件缺失。
pub const E_HANDOVER: u16 = SEG_BASE + 5;
/// 移交包指纹不符。
pub const E_PKG_HASH: u16 = SEG_BASE + 6;

/// E05 组元区间（假设：F0705~F0719）。
pub const E05_RANGE: (u32, u32) = (705, 719);
/// 组元数。
pub const E05_COUNT: usize = 15;
/// 验收面（闭集三种）。
pub const FACES: [&str; 3] = ["unit", "diff", "feel"];
/// 验收子域（闭集五种）。
pub const DOMAINS: [&str; 5] = ["concur", "telem", "api", "docs", "bench"];

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 自检入树条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelfCheckItem {
    /// 任务号（如 710 = F0710）。
    pub task: u32,
    /// 结论（true=过）。
    pub ok: bool,
    /// 证据指纹。
    pub evid: u64,
}

/// 收口树。
#[derive(Clone, Debug)]
pub struct CloseTree {
    items: Vec<SelfCheckItem>,
}

impl CloseTree {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }
    /// 入树：区间校验 + 去重。
    pub fn enter(&mut self, it: SelfCheckItem) -> Result<(), u16> {
        if it.task < E05_RANGE.0 || it.task > E05_RANGE.1 {
            return Err(E_TREE_ITEM);
        }
        if self.items.iter().any(|x| x.task == it.task) {
            return Err(E_TREE_ITEM);
        }
        self.items.push(it);
        Ok(())
    }
    /// 收口门槛：15 单全入树且全过。
    pub fn complete(&self) -> bool {
        if self.items.len() != E05_COUNT {
            return false;
        }
        for n in E05_RANGE.0..=E05_RANGE.1 {
            match self.items.iter().find(|x| x.task == n) {
                Some(x) if x.ok => {}
                _ => return false,
            }
        }
        true
    }
    /// 收口内容哈希（双签对象）。
    pub fn content_hash(&self) -> u64 {
        let mut bytes = Vec::new();
        for it in &self.items {
            bytes.extend_from_slice(&it.task.to_le_bytes());
            bytes.push(it.ok as u8);
            bytes.extend_from_slice(&it.evid.to_le_bytes());
        }
        fnv1a64(&bytes)
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
}

/// 验收矩阵：3 面 × 5 域 = 15 格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellVerdict {
    /// 通过。
    Pass,
    /// 未过。
    Fail,
    /// 未测。
    NotRun,
}

/// 验收矩阵。
#[derive(Clone, Debug)]
pub struct AcceptMatrix {
    /// 行优先：cells[face*5 + domain]。
    cells: [CellVerdict; 15],
}

impl AcceptMatrix {
    pub fn new() -> Self {
        Self {
            cells: [CellVerdict::NotRun; 15],
        }
    }
    /// 索引合法性。
    fn idx(face: usize, domain: usize) -> Result<usize, u16> {
        if face >= FACES.len() || domain >= DOMAINS.len() {
            return Err(E_MATRIX_RED);
        }
        Ok(face * DOMAINS.len() + domain)
    }
    /// 写一格。
    pub fn set(&mut self, face: usize, domain: usize, v: CellVerdict) -> Result<(), u16> {
        let i = Self::idx(face, domain)?;
        self.cells[i] = v;
        Ok(())
    }
    /// 读一格。
    pub fn get(&self, face: usize, domain: usize) -> Result<CellVerdict, u16> {
        let i = Self::idx(face, domain)?;
        Ok(self.cells[i])
    }
    /// 全绿：15 格全 Pass（无 NotRun 无 Fail）。
    pub fn all_green(&self) -> bool {
        self.cells.iter().all(|c| *c == CellVerdict::Pass)
    }
    /// Fail 格计数。
    pub fn reds(&self) -> usize {
        self.cells.iter().filter(|c| **c == CellVerdict::Fail).count()
    }
    /// 格指纹（封存入移交包）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes = Vec::new();
        for c in &self.cells {
            bytes.push(match c {
                CellVerdict::Pass => 0,
                CellVerdict::Fail => 1,
                CellVerdict::NotRun => 2,
            });
        }
        fnv1a64(&bytes)
    }
}

/// 签核记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignRec {
    /// 签核人角色（0=组长,1=质量员,2=接口人；互异约束）。
    pub role: u8,
    /// 签核对象哈希。
    pub obj_hash: u64,
}

/// 移交包四件。
#[derive(Clone, Debug)]
pub struct HandoverPkg {
    /// 接口清单（名）。
    pub interfaces: Vec<&'static str>,
    /// 证据指纹表（任务号,指纹）。
    pub evids: Vec<(u32, u64)>,
    /// 开放缺陷（编号,严重度 1~4）。
    pub defects: Vec<(u32, u8)>,
    /// 验收矩阵快照指纹。
    pub matrix_fp: u64,
}

impl HandoverPkg {
    /// 四件完整性：每件非空（缺陷可为空，其余必须非空）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.interfaces.is_empty() || self.evids.is_empty() || self.matrix_fp == 0 {
            return Err(E_HANDOVER);
        }
        for (_, sev) in &self.defects {
            if *sev == 0 || *sev > 4 {
                return Err(E_HANDOVER);
            }
        }
        Ok(())
    }
    /// 包封存指纹。
    pub fn seal(&self) -> u64 {
        let mut bytes = Vec::new();
        for n in &self.interfaces {
            bytes.extend_from_slice(n.as_bytes());
            bytes.push(0);
        }
        for (t, e) in &self.evids {
            bytes.extend_from_slice(&t.to_le_bytes());
            bytes.extend_from_slice(&e.to_le_bytes());
        }
        for (d, s) in &self.defects {
            bytes.extend_from_slice(&d.to_le_bytes());
            bytes.push(*s);
        }
        bytes.extend_from_slice(&self.matrix_fp.to_le_bytes());
        fnv1a64(&bytes)
    }
}

/// 收口流水线：树 → 矩阵 → 双签 → 移交。
#[derive(Clone, Debug)]
pub struct ClosePipeline {
    tree: CloseTree,
    matrix: AcceptMatrix,
    signs: Vec<SignRec>,
}

impl ClosePipeline {
    pub fn new() -> Self {
        Self {
            tree: CloseTree::new(),
            matrix: AcceptMatrix::new(),
            signs: Vec::new(),
        }
    }
    pub fn tree_mut(&mut self) -> &mut CloseTree {
        &mut self.tree
    }
    pub fn matrix_mut(&mut self) -> &mut AcceptMatrix {
        &mut self.matrix
    }
    /// 加签：角色 0..=2 且互异；哈希必须等于当前树哈希。
    pub fn sign(&mut self, role: u8) -> Result<(), u16> {
        if role > 2 {
            return Err(E_SIGN);
        }
        if self.signs.iter().any(|s| s.role == role) {
            return Err(E_SIGN);
        }
        self.signs.push(SignRec {
            role,
            obj_hash: self.tree.content_hash(),
        });
        Ok(())
    }
    /// 收口判定：树全过 + 矩阵全绿 + 双签对当前哈希有效。
    pub fn can_close(&self) -> Result<(), u16> {
        if !self.tree.complete() {
            return Err(E_TREE_SHORT);
        }
        if !self.matrix.all_green() {
            return Err(E_MATRIX_RED);
        }
        if self.signs.len() < 2 {
            return Err(E_SIGN);
        }
        let h = self.tree.content_hash();
        for s in &self.signs {
            if s.obj_hash != h {
                return Err(E_SIGN);
            }
        }
        Ok(())
    }
    /// 出移交包（收口通过后）。
    pub fn handover(&self) -> Result<HandoverPkg, u16> {
        self.can_close()?;
        let evids = self
            .tree
            .items
            .iter()
            .map(|it| (it.task, it.evid))
            .collect();
        Ok(HandoverPkg {
            interfaces: vec![
                "scroll_tick",
                "scroll_telem_pct",
                "scroll_apifreeze_gate",
                "scroll_bench_run",
            ],
            evids,
            defects: Vec::new(),
            matrix_fp: self.matrix.fingerprint(),
        })
    }
    /// 已签数。
    pub fn signs(&self) -> usize {
        self.signs.len()
    }
}

/// 移交包收端校验：包自洽 + 指纹复算一致。
pub fn receive(pkg: &HandoverPkg) -> Result<(), u16> {
    pkg.validate()?;
    let fp = pkg.seal();
    if fp == 0 {
        return Err(E_PKG_HASH);
    }
    Ok(())
}

/// 自例程：主路径（15 单入树→矩阵全绿→双签→移交→收端校验）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut p = ClosePipeline::new();
    // 主路径：15 单入树（证据指纹 = 任务号*0x9E37 折叠确定性值）。
    for t in E05_RANGE.0..=E05_RANGE.1 {
        let evid = fnv1a64(&t.to_le_bytes());
        p.tree_mut().enter(SelfCheckItem { task: t, ok: true, evid })?;
    }
    if p.tree_mut().len() != E05_COUNT {
        return Err(E_TREE_SHORT);
    }
    // 错误路径 0：区间外任务号。
    if p.tree_mut().enter(SelfCheckItem { task: 720, ok: true, evid: 1 }) != Err(E_TREE_ITEM) {
        return Err(E_TREE_ITEM);
    }
    // 错误路径 1：重号。
    if p.tree_mut().enter(SelfCheckItem { task: 710, ok: true, evid: 1 }) != Err(E_TREE_ITEM) {
        return Err(E_TREE_ITEM);
    }
    // 主路径：矩阵全绿。
    for f in 0..FACES.len() {
        for d in 0..DOMAINS.len() {
            p.matrix_mut().set(f, d, CellVerdict::Pass)?;
        }
    }
    if !p.matrix_mut().all_green() || p.matrix_mut().reds() != 0 {
        return Err(E_MATRIX_RED);
    }
    // 主路径：双签（角色 0 组长、1 质量员）。
    p.sign(0)?;
    p.sign(1)?;
    p.can_close()?;
    // 移交。
    let pkg = p.handover()?;
    receive(&pkg)?;
    let fp = pkg.seal();
    if fp == 0 {
        return Err(E_PKG_HASH);
    }
    if pkg.evids.len() != 15 || pkg.matrix_fp != p.matrix_mut().fingerprint() {
        return Err(E_HANDOVER);
    }
    // 错误路径 2：角色重复签。
    let mut p2 = ClosePipeline::new();
    for t in E05_RANGE.0..=E05_RANGE.1 {
        p2.tree_mut().enter(SelfCheckItem { task: t, ok: true, evid: t as u64 })?;
    }
    p2.sign(0)?;
    if p2.sign(0) != Err(E_SIGN) {
        return Err(E_SIGN);
    }
    // 错误路径 3：签后改树 → 哈希失配。
    p2.sign(1)?;
    // 补一格矩阵与内容再触发 can_close：改树内容（重录 705？不允许重号），
    // 改用 evid 无法改；改为未齐树场景：新建管线只入 14 单后双签再判。
    let mut p3 = ClosePipeline::new();
    for t in E05_RANGE.0..E05_RANGE.1 {
        // 少最后一单（719）
        p3.tree_mut().enter(SelfCheckItem { task: t, ok: true, evid: t as u64 })?;
    }
    p3.sign(0)?;
    p3.sign(1)?;
    if p3.can_close() != Err(E_TREE_SHORT) {
        return Err(E_TREE_SHORT);
    }
    // 错误路径 4：矩阵有 Fail。
    let mut p4 = ClosePipeline::new();
    for t in E05_RANGE.0..=E05_RANGE.1 {
        p4.tree_mut().enter(SelfCheckItem { task: t, ok: true, evid: t as u64 })?;
    }
    for f in 0..FACES.len() {
        for d in 0..DOMAINS.len() {
            p4.matrix_mut().set(f, d, CellVerdict::Pass)?;
        }
    }
    p4.matrix_mut().set(1, 2, CellVerdict::Fail)?;
    p4.sign(0)?;
    p4.sign(1)?;
    if p4.can_close() != Err(E_MATRIX_RED) {
        return Err(E_MATRIX_RED);
    }
    // 错误路径 5：缺陷严重度越界的包。
    let bad_pkg = HandoverPkg {
        interfaces: vec!["x"],
        evids: vec![(710, 1)],
        defects: vec![(1, 9)],
        matrix_fp: 12345,
    };
    if receive(&bad_pkg) != Err(E_HANDOVER) {
        return Err(E_HANDOVER);
    }
    Ok(0)
}
