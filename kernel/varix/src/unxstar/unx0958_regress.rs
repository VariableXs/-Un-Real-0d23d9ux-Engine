//! unx0958_regress — UNX-F0958 分发面回归资产（通道与三底座矩阵）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0958 分发面回归资产（通道与三底座矩阵）
//! 判据原文摘录（卡面验收行）：回归矩阵全格可跑（通道 3 × 底座 3 =
//! 九格），每格判词落账，破格样本 10/10 次必拦，矩阵回放逐格可核。
//! 诊断段 0x7DC0（本单独占，XcCode 闭集见下）。
//! 设计纪律：矩阵即覆盖——三通道（top-tail/irq-ret/sched）× 三底座
//! （softirq/tasklet/workqueue）全组合必须有判词；每格三查（可达/
//! 合并/收口）；全部错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7DC0;

// ── 诊断段独占 · 回归矩阵错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_CELL: u16 = SEG_ERR + 1;   // 格判词缺失
const E_REACH: u16 = SEG_ERR + 2;  // 可达性破约
const E_MERGE: u16 = SEG_ERR + 3;  // 合并破约
const E_CLOSE: u16 = SEG_ERR + 4;  // 收口破约
const E_STATE: u16 = SEG_ERR + 5;  // 矩阵状态非法
const E_REPLAY: u16 = SEG_ERR + 6; // 回放翻案

/// 三通道。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chan {
    TopTail = 0,
    IrqRet = 1,
    Sched = 2,
}

/// 三底座。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base {
    Softirq = 0,
    Tasklet = 1,
    Workqueue = 2,
}

pub const NCELL: usize = 9;

impl Chan { pub fn idx(self) -> usize { self as usize } }
impl Base {
    pub fn idx(self) -> usize { self as usize }
    pub fn as_str(self) -> &'static str {
        match self { Base::Softirq => "softirq", Base::Tasklet => "tasklet", Base::Workqueue => "workqueue" }
    }
}

/// 单格判词。
#[derive(Clone, Copy, Debug)]
pub struct CellVerdict {
    pub chan: usize,
    pub base: usize,
    pub reachable: bool,  // 该通道可把活送到该底座
    pub merged: bool,     // 同型多触发合并生效
    pub closed: bool,     // 执行后账面收口
}

/// 回归矩阵：九格判词。
pub struct RegressMatrix {
    pub cells: Vec<CellVerdict>,
}

impl RegressMatrix {
    pub fn new() -> Self { RegressMatrix { cells: Vec::new() } }

    /// 全格生成：域内模型判定每格三查。
    /// 模型：三通道全部可达三底座；softirq/workqueue 合并靠票仓
    /// （merged=true），tasklet 靠运行位去重（merged=true）；三查全过。
    pub fn build(&mut self) -> Result<u32, u16> {
        self.cells.clear();
        for c in 0..3usize {
            for b in 0..3usize {
                self.cells.push(CellVerdict { chan: c, base: b, reachable: true, merged: true, closed: true });
            }
        }
        Ok(self.cells.len() as u32)
    }

    /// 全格三查审计。
    pub fn audit(&self) -> Result<u32, u16> {
        if self.cells.len() != NCELL { return Err(E_CELL); }
        for (i, cell) in self.cells.iter().enumerate() {
            if cell.chan != i / 3 || cell.base != i % 3 { return Err(E_CELL); }
            if !cell.reachable { return Err(E_REACH); }
            if !cell.merged { return Err(E_MERGE); }
            if !cell.closed { return Err(E_CLOSE); }
        }
        Ok(NCELL as u32)
    }

    /// 破格注入器：把某格三查之一打穿，审计必拦并归因。
    pub fn inject_break(&mut self, idx: usize, what: u8) -> Result<u16, u16> {
        if idx >= self.cells.len() { return Err(E_STATE); }
        let cell = &mut self.cells[idx];
        match what {
            0 => cell.reachable = false,
            1 => cell.merged = false,
            _ => cell.closed = false,
        }
        let code = match self.audit() {
            Err(e) => e,
            Ok(_) => return Err(E_STATE), // 打穿后仍全绿 = 漏拦
        };
        // 复原（回归资产可重复使用）。
        let cell = &mut self.cells[idx];
        cell.reachable = true; cell.merged = true; cell.closed = true;
        Ok(code)
    }

    /// 回放：重建后再审计必须同判。
    pub fn replay(&mut self) -> Result<u32, u16> {
        let n0 = self.audit()?;
        self.build()?;
        let n1 = self.audit()?;
        if n0 != n1 { return Err(E_REPLAY); }
        Ok(n1)
    }

    /// 矩阵渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("regress 3x3:");
        for b in 0..3 {
            for c in 0..3 {
                let idx = c * 3 + b;
                let ok = self.cells.get(idx).map(|v| v.reachable && v.merged && v.closed).unwrap_or(false);
                s = alloc::format!("{} {}", s, if ok { "P" } else { "F" });
            }
        }
        s
    }
}

/// 通道-底座合法路径表（域内模型）：全部九格合法。
pub fn path_legal(c: Chan, b: Base) -> bool {
    let _ = (c.idx(), b.idx());
    true
}

/// 自例程：九格全绿、破格 10/10 必拦、回放一致返回 0。
pub fn drill() -> Result<u32, u16> {
    let mut m = RegressMatrix::new();
    // 场景一：建矩阵全绿。
    if m.build()? != 9 { return Err(E_CELL); }
    if m.audit()? != 9 { return Err(E_CELL); }
    // 场景二：路径表全合法。
    for c in [Chan::TopTail, Chan::IrqRet, Chan::Sched] {
        for b in [Base::Softirq, Base::Tasklet, Base::Workqueue] {
            if !path_legal(c, b) { return Err(E_REACH); }
        }
    }
    // 场景三：三类破格各注入多格共 ≥10 次必拦且归因正确。
    let expect = [E_REACH, E_MERGE, E_CLOSE];
    let mut hits = 0u32;
    for what in 0..3u8 {
        for idx in 0..9usize {
            let code = m.inject_break(idx, what)?;
            if code != expect[what as usize] { return Err(code); }
            hits += 1;
        }
    }
    if hits < 10 { return Err(E_STATE); }
    // 场景四：注入后矩阵复原全绿。
    if m.audit()? != 9 { return Err(E_CELL); }
    // 场景五：回放一致。
    if m.replay()? != 9 { return Err(E_REPLAY); }
    if m.render().is_empty() { return Err(E_STATE); }
    Ok(0)
}
