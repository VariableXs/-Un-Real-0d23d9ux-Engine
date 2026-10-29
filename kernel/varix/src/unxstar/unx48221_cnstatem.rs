//! unx48221_cnstatem — UNX-F48221 中英三态状态机总成（中文/英文临时/英文锁定）（诊断段 0xBC80）。
//!
//! 判据原文：
//!   J1 三态转移矩阵 ×10^3 全对；判据锚定 §AI-61 §2 专题五状态机。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三态封闭：Zh（中文）/ EnTemp（英文临时）/ EnLock（英文锁定）；
//!      状态编码 0/1/2 闭集，越界编码拒收（E_ST_OOB）。
//!   2. 五事件驱动：ShiftTap（单击切换临时）、Commit（字符提交）、
//!      Escape（回中文）、LockToggle（锁定开关）、CnSelect（回中文选字）。
//!   3. 双编码对照：转移逻辑用 match 逐态实现，规格表用 const 矩阵
//!      [次态][动作] 双套编码；×10^3 全对 = 每步双套逐位相等——两套
//!      独立编码互为哨兵，杜绝单套自证。
//!   4. 转移伴随动作账：每次转移携带动作码（0 无动作 / 1 临时开 /
//!      2 回中文 / 3 锁定开 / 4 锁定解除），动作账逐条留痕可溯。
//!   5. 格命中账：3 态 × 5 事件 = 15 格转移格，每格命中次数逐格记账，
//!      ×10^3 行走后逐格账实相符、列覆盖（每事件至少一命中）。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBC80;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_ST_OOB: u16 = SEG_BASE + 1; // 状态编码越界
const E_EV_OOB: u16 = SEG_BASE + 2; // 事件编码越界
const E_MATRIX_OFF: u16 = SEG_BASE + 3; // 双编码对照失守
const E_ACT_OFF: u16 = SEG_BASE + 4; // 动作账不符
const E_CELL_OFF: u16 = SEG_BASE + 5; // 格命中账不平
const E_STATE_BAD: u16 = SEG_BASE + 6; // 机器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 状态编码闭集。
pub const ST_ZH: u8 = 0; // 中文
pub const ST_EN_TEMP: u8 = 1; // 英文临时
pub const ST_EN_LOCK: u8 = 2; // 英文锁定
pub const ST_N: usize = 3;

/// 事件编码闭集。
pub const EV_SHIFT_TAP: usize = 0;
pub const EV_COMMIT: usize = 1;
pub const EV_ESCAPE: usize = 2;
pub const EV_LOCK_TOGGLE: usize = 3;
pub const EV_CN_SELECT: usize = 4;
pub const EV_N: usize = 5;

/// 规格矩阵（哨兵套）：行=现态，列=事件，值=次态编码。
pub const NEXT: [[u8; EV_N]; ST_N] = [
    //  ShiftTap Commit Escape LockTg CnSel
    [1, 0, 0, 2, 0], // Zh
    [0, 0, 0, 2, 0], // EnTemp
    [2, 2, 0, 0, 0], // EnLock
];

/// 动作矩阵（哨兵套）：0 无 / 1 临时开 / 2 回中文 / 3 锁定开 / 4 锁定解除。
pub const ACT: [[u8; EV_N]; ST_N] = [
    [1, 0, 0, 3, 0], // Zh
    [2, 2, 2, 3, 2], // EnTemp
    [0, 0, 2, 4, 2], // EnLock
];

/// 状态编码校验（闭集）。
pub fn st_valid(st: u8) -> Result<(), u16> {
    if st >= ST_N as u8 {
        return Err(E_ST_OOB);
    }
    Ok(())
}

/// match 独立实现：单格转移（次态、动作）——与规格矩阵互为哨兵。
pub fn cell(st: u8, ev: usize) -> Result<(u8, u8), u16> {
    st_valid(st)?;
    if ev >= EV_N {
        return Err(E_EV_OOB);
    }
    let next = match (st, ev) {
        (ST_ZH, EV_SHIFT_TAP) => ST_EN_TEMP,
        (ST_ZH, EV_COMMIT) => ST_ZH,
        (ST_ZH, EV_ESCAPE) => ST_ZH,
        (ST_ZH, EV_LOCK_TOGGLE) => ST_EN_LOCK,
        (ST_ZH, EV_CN_SELECT) => ST_ZH,
        (ST_EN_TEMP, EV_SHIFT_TAP) => ST_ZH,
        (ST_EN_TEMP, EV_COMMIT) => ST_ZH,
        (ST_EN_TEMP, EV_ESCAPE) => ST_ZH,
        (ST_EN_TEMP, EV_LOCK_TOGGLE) => ST_EN_LOCK,
        (ST_EN_TEMP, EV_CN_SELECT) => ST_ZH,
        (ST_EN_LOCK, EV_SHIFT_TAP) => ST_EN_LOCK,
        (ST_EN_LOCK, EV_COMMIT) => ST_EN_LOCK,
        (ST_EN_LOCK, EV_ESCAPE) => ST_ZH,
        (ST_EN_LOCK, EV_LOCK_TOGGLE) => ST_ZH,
        (ST_EN_LOCK, EV_CN_SELECT) => ST_ZH,
        _ => return Err(E_MATRIX_OFF),
    };
    let act = match (st, ev) {
        (ST_ZH, EV_SHIFT_TAP) => 1,
        (ST_ZH, EV_LOCK_TOGGLE) => 3,
        (ST_EN_TEMP, EV_SHIFT_TAP) => 2,
        (ST_EN_TEMP, EV_COMMIT) => 2,
        (ST_EN_TEMP, EV_ESCAPE) => 2,
        (ST_EN_TEMP, EV_LOCK_TOGGLE) => 3,
        (ST_EN_TEMP, EV_CN_SELECT) => 2,
        (ST_EN_LOCK, EV_ESCAPE) => 2,
        (ST_EN_LOCK, EV_LOCK_TOGGLE) => 4,
        (ST_EN_LOCK, EV_CN_SELECT) => 2,
        _ => 0,
    };
    Ok((next, act))
}

/// 轨迹账一条：(前态, 事件, 次态, 动作) 四元组留痕。
#[derive(Clone, Copy, Debug)]
pub struct TraceStep {
    pub from: u8,
    pub ev: u8,
    pub to: u8,
    pub act: u8,
}

/// 状态机总成。
pub struct CnStatem {
    pub st: u8,
    pub steps: u64,
    pub cell_hits: [[u32; EV_N]; ST_N],
    pub act_log: Vec<u8>,
    pub trace: Vec<TraceStep>,
    pub matrix_faults: u32,
}

impl CnStatem {
    pub fn new() -> CnStatem {
        CnStatem {
            st: ST_ZH,
            steps: 0,
            cell_hits: [[0; EV_N]; ST_N],
            act_log: Vec::new(),
            trace: Vec::new(),
            matrix_faults: 0,
        }
    }

    /// 单步：match 逻辑驱动，规格矩阵同步比对（双哨兵），失守即记故障并拒。
    pub fn step(&mut self, ev: usize) -> Result<u8, u16> {
        let (next, act) = cell(self.st, ev)?;
        // 双编码对照（纪律 3）：规格矩阵逐位核对本步结果。
        if NEXT[self.st as usize][ev] != next {
            self.matrix_faults += 1;
            return Err(E_MATRIX_OFF);
        }
        if ACT[self.st as usize][ev] != act {
            self.matrix_faults += 1;
            return Err(E_ACT_OFF);
        }
        self.cell_hits[self.st as usize][ev] += 1;
        self.act_log.push(act);
        self.trace.push(TraceStep { from: self.st, ev: ev as u8, to: next, act });
        self.st = next;
        self.steps += 1;
        Ok(act)
    }

    /// ×10^3 行走（J1 主体）：确定性事件流逐步双哨兵比对。
    pub fn walk(&mut self, n: u32) -> Result<(), u16> {
        let mut i = 0u32;
        while i < n {
            let ev = (i as usize * 3) % EV_N;
            self.step(ev)?;
            i += 1;
        }
        Ok(())
    }

    /// 语义锚点复核：四组关键转移的 (次态, 动作) 必须与语义逐位相符。
    pub fn assert_anchors(&self) -> Result<(), u16> {
        let a = cell(ST_ZH, EV_SHIFT_TAP)?;
        if a != (ST_EN_TEMP, 1) {
            return Err(E_ACT_OFF); // 中文单击 Shift → 临时英文开
        }
        let b = cell(ST_EN_TEMP, EV_SHIFT_TAP)?;
        if b != (ST_ZH, 2) {
            return Err(E_ACT_OFF); // 临时英文单击 Shift → 回中文
        }
        let c = cell(ST_ZH, EV_LOCK_TOGGLE)?;
        if c != (ST_EN_LOCK, 3) {
            return Err(E_ACT_OFF); // 中文锁定开
        }
        let d = cell(ST_EN_LOCK, EV_LOCK_TOGGLE)?;
        if d != (ST_ZH, 4) {
            return Err(E_ACT_OFF); // 锁定态再切 → 锁定解除回中文
        }
        Ok(())
    }

    /// 格命中账审计：总命中 = 步数、逐格非负闭合、列覆盖（每事件至少一命中）。
    pub fn audit_cells(&self, want_steps: u64) -> Result<(), u16> {
        let mut total: u64 = 0;
        let mut col = [0u32; EV_N];
        let mut s = 0usize;
        while s < ST_N {
            let mut e = 0usize;
            while e < EV_N {
                total += self.cell_hits[s][e] as u64;
                col[e] += self.cell_hits[s][e];
                s_pow_check(s, e)?;
                e += 1;
            }
            s += 1;
        }
        if total != want_steps {
            return Err(E_CELL_OFF);
        }
        let mut e = 0usize;
        while e < EV_N {
            if col[e] == 0 {
                return Err(E_CELL_OFF); // 事件列零命中 = 流覆盖缺口
            }
            e += 1;
        }
        if self.st >= ST_N as u8 {
            return Err(E_ST_OOB);
        }
        if self.act_log.len() as u64 != self.steps {
            return Err(E_ACT_OFF);
        }
        Ok(())
    }

    /// 轨迹重放：从 ST_ZH 起按轨迹账逐步重放（独立走 cell 逻辑），
    /// 逐条四元组核对、终态与现态相等、轨迹长度与步数闭合——账实可溯。
    pub fn replay(&self) -> Result<u8, u16> {
        let mut st: u8 = ST_ZH;
        let mut i = 0usize;
        while i < self.trace.len() {
            let t = self.trace[i];
            let got = cell(st, t.ev as usize)?;
            if got.0 != t.to || got.1 != t.act || st != t.from {
                return Err(E_MATRIX_OFF);
            }
            st = got.0;
            i += 1;
        }
        if self.trace.len() as u64 != self.steps {
            return Err(E_CELL_OFF);
        }
        if st != self.st {
            return Err(E_MATRIX_OFF);
        }
        Ok(st)
    }

    /// 动作账闭集审计：动作码只在 0..=4。
    pub fn audit_act_closure(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.act_log.len() {
            if self.act_log[i] > 4 {
                return Err(E_ACT_OFF);
            }
            i += 1;
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：行走量达标 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.steps == 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

// 格位自检（行列界内恒真，越界编码在 cell 入口拦截）。
fn s_pow_check(s: usize, e: usize) -> Result<(), u16> {
    if s >= ST_N || e >= EV_N {
        return Err(E_CELL_OFF);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（三态转移矩阵 ×10^3 全对），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 全矩阵扫格：15 格 (次态, 动作) 双编码逐格对照全对——
    //    match 套与规格矩阵独立编码互为哨兵。
    let mut s = 0u8;
    while s < ST_N as u8 {
        let mut e = 0usize;
        while e < EV_N {
            let got = cell(s, e)?;
            if got.0 != NEXT[s as usize][e] {
                return Err(E_MATRIX_OFF);
            }
            if got.1 != ACT[s as usize][e] {
                return Err(E_ACT_OFF);
            }
            e += 1;
        }
        s += 1;
    }
    hits += 1;

    // R2 主对齐 ×10^3：确定性事件流 (i*3)%5 行走 1000 步，逐步双哨兵
    //    比对零失守（matrix_faults 恒 0）。
    let mut sm = CnStatem::new();
    sm.walk(1000)?;
    if sm.steps != 1000 || sm.matrix_faults != 0 {
        return Err(E_MATRIX_OFF);
    }
    sm.audit_cells(1000)?;
    hits += 1;

    // R3 格命中账终判：总命中=1000、五事件列全有命中（流覆盖无缺口）。
    let mut col = [0u32; EV_N];
    let mut s = 0usize;
    while s < ST_N {
        let mut e = 0usize;
        while e < EV_N {
            col[e] += sm.cell_hits[s][e];
            e += 1;
        }
        s += 1;
    }
    let mut e = 0usize;
    while e < EV_N {
        if col[e] == 0 {
            return Err(E_CELL_OFF);
        }
        e += 1;
    }
    hits += 1;

    // R4 语义锚点（专题五口径）：Zh→EnTemp 临时开、EnTemp→Zh 回中文、
    //    Zh→EnLock 锁定开、EnLock→Zh 锁定解除——四锚逐位核对。
    let mut sm2 = CnStatem::new();
    sm2.assert_anchors()?;
    let act1 = sm2.step(EV_SHIFT_TAP)?;
    if act1 != 1 || sm2.st != ST_EN_TEMP {
        return Err(E_ACT_OFF);
    }
    let act2 = sm2.step(EV_SHIFT_TAP)?;
    if act2 != 2 || sm2.st != ST_ZH {
        return Err(E_ACT_OFF);
    }
    hits += 1;

    // R5 锁定支线语义：锁定态下 ShiftTap/Commit 不解锁（自环），
    //    Escape/LockToggle/CnSelect 三路回中文——锁定语义逐路核对。
    if cell(ST_EN_LOCK, EV_SHIFT_TAP)? != (ST_EN_LOCK, 0) {
        return Err(E_ACT_OFF);
    }
    if cell(ST_EN_LOCK, EV_COMMIT)? != (ST_EN_LOCK, 0) {
        return Err(E_ACT_OFF);
    }
    if cell(ST_EN_LOCK, EV_ESCAPE)? != (ST_ZH, 2) {
        return Err(E_ACT_OFF);
    }
    if cell(ST_EN_LOCK, EV_CN_SELECT)? != (ST_ZH, 2) {
        return Err(E_ACT_OFF);
    }
    hits += 1;

    // R6 临时态语义：EnTemp 下 Commit/Escape/CnSelect 均回中文（临时
    //    英文一次一收），LockToggle 可升级锁定——临时态五路核对。
    if cell(ST_EN_TEMP, EV_COMMIT)? != (ST_ZH, 2) {
        return Err(E_ACT_OFF);
    }
    if cell(ST_EN_TEMP, EV_ESCAPE)? != (ST_ZH, 2) {
        return Err(E_ACT_OFF);
    }
    if cell(ST_EN_TEMP, EV_CN_SELECT)? != (ST_ZH, 2) {
        return Err(E_ACT_OFF);
    }
    if cell(ST_EN_TEMP, EV_LOCK_TOGGLE)? != (ST_EN_LOCK, 3) {
        return Err(E_ACT_OFF);
    }
    hits += 1;

    // R7 编码闭集：越界状态/事件编码双拒（E_ST_OOB / E_EV_OOB），
    //    状态机故障计数与动作账闭合。
    if cell(3, EV_COMMIT).is_ok() {
        return Err(E_ST_OOB);
    }
    if cell(ST_ZH, 9).is_ok() {
        return Err(E_EV_OOB);
    }
    let mut sm3 = CnStatem::new();
    let mut i = 0u32;
    while i < 64 {
        sm3.step((i as usize * 2) % EV_N)?;
        i += 1;
    }
    if sm3.act_log.len() as u64 != sm3.steps || sm3.matrix_faults != 0 {
        return Err(E_ACT_OFF);
    }
    hits += 1;

    // R8 写回（判据 J2）：零行走不写回、空摘要拒、×10^3 达标+非空摘要放行。
    let mut sm4 = CnStatem::new();
    if sm4.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    sm4.walk(1000)?;
    if sm4.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    sm4.writeback(true, 0x64)?;
    hits += 1;

    // R9 轨迹重放账：×10^3 行走的四元组轨迹从 ST_ZH 独立重放，逐条
    //    (前态,事件,次态,动作) 核对、终态与现态相等——账实可溯不悬空。
    let mut sm5 = CnStatem::new();
    sm5.walk(1000)?;
    let end = sm5.replay()?;
    if end != sm5.st || sm5.trace.len() as u64 != 1000 {
        return Err(E_MATRIX_OFF);
    }
    // 轨迹首条必为 (Zh, ShiftTap, EnTemp, 临时开)——事件流 i=0 即 ShiftTap。
    let t0 = sm5.trace[0];
    if t0.from != ST_ZH || t0.ev != EV_SHIFT_TAP as u8 || t0.to != ST_EN_TEMP || t0.act != 1 {
        return Err(E_CELL_OFF);
    }
    hits += 1;

    // R10 动作账闭集 + 账间闭合：动作码只在 0..=4；动作账、轨迹账、
    //    格命中账三方对步数闭合（同一行走的三本账不串号）。
    sm5.audit_act_closure()?;
    if sm5.act_log.len() != sm5.trace.len() {
        return Err(E_ACT_OFF);
    }
    let mut total: u64 = 0;
    let mut s = 0usize;
    while s < ST_N {
        let mut e = 0usize;
        while e < EV_N {
            total += sm5.cell_hits[s][e] as u64;
            e += 1;
        }
        s += 1;
    }
    if total != sm5.steps {
        return Err(E_CELL_OFF);
    }
    sm5.audit_cells(1000)?;
    hits += 1;

    Ok(hits)
}
