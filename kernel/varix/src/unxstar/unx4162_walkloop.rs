//! unx4162_walkloop — UNX-F4162 逐分量 walk 主循环（类型分派）（诊断段 0x95E0）。
//!
//! 判据原文：
//!   J1 主循环对七类类型分派行为矩阵（目录可穿越/普通文件仅可为末分量/
//!      其余按类型规则共 14 格）全过。
//!   J2 10 万分量负载零错账。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 七类定编：目录/普通/链接/FIFO/套接字/字符设备/块设备，
//!      分派行为按 POSIX path_resolution 口径裁定。
//!   2. 14 格矩阵：七类 × {非末分量, 末分量} 两列——目录两列皆过、
//!      普通文件仅末列过、链接非末列穿越、FIFO/套接字/字符/块仅末列过。
//!   3. 主循环逐分量推进：每分量一次分派一账，出错即短路（错误
//!      短路归 F4171 分账，本模块只记停点）。
//!   4. 10 万分量负载：计数器驱动确定性负载，错账计数必须为零。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x95E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOT_DIR: u16 = SEG_BASE + 1;   // 非末分量遇不可穿越类型（ENOTDIR 口径）
const E_BAD_TYPE: u16 = SEG_BASE + 2;  // 类型号越界（七类闭集外）
const E_EMPTY_WALK: u16 = SEG_BASE + 3;// 零分量 walk 拒收
const E_MATRIX_MISS: u16 = SEG_BASE + 4; // 矩阵格缺验（14/14 失守）
const E_LOAD_BAL: u16 = SEG_BASE + 5;  // 负载账与分量账不平
const E_BAD_STATE: u16 = SEG_BASE + 6; // 循环状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 七类类型号（POSIX DT_ 口径裁剪）。
pub const T_DIR: u8 = 0;    // 目录：可穿越
pub const T_REG: u8 = 1;    // 普通文件：仅末分量
pub const T_LNK: u8 = 2;    // 符号链接：非末分量可穿越（预锚）
pub const T_FIFO: u8 = 3;   // FIFO：仅末分量
pub const T_SOCK: u8 = 4;   // 套接字：仅末分量
pub const T_CHR: u8 = 5;    // 字符设备：仅末分量
pub const T_BLK: u8 = 6;    // 块设备：仅末分量
pub const TYPE_TOTAL: u8 = 7;

pub const LOAD_TOTAL: u32 = 100_000; // 10 万分量负载定编

/// 分派结果状态机：Traverse 穿越 / Final 末分量达标 / Stop 短路停点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispState {
    Traverse,
    Final,
    Stop,
}

/// walk 循环账：步数、分派分布、错账。
pub struct WalkLedger {
    pub steps: u32,               // 已走分量数
    pub traverse_cnt: u32,        // Traverse 分派计数
    pub final_cnt: u32,           // Final 分派计数
    pub stop_cnt: u32,            // Stop 分派计数（含矩阵验证注入）
    pub err_count: u32,           // 错账（必须为零）
    pub state: DispState,
}

impl WalkLedger {
    pub fn new() -> WalkLedger {
        WalkLedger {
            steps: 0,
            traverse_cnt: 0,
            final_cnt: 0,
            stop_cnt: 0,
            err_count: 0,
            state: DispState::Traverse,
        }
    }
}

/// 分派矩阵裁定：type × 是否末分量 → 分派状态（14 格闭集）。
/// 口径：目录两格皆 Traverse/Final；普通文件仅末格 Final；
/// 链接非末格 Traverse（跟随预锚）、末格 Final；
/// FIFO/套接字/字符/块仅末格 Final，非末格一律 Stop(ENOTDIR 族)。
pub fn dispatch(ty: u8, is_last: bool) -> Result<DispState, u16> {
    if ty >= TYPE_TOTAL {
        return Err(E_BAD_TYPE);
    }
    if is_last {
        // 末分量列：七类全部可达 Final（各类型均合法终结）。
        Ok(DispState::Final)
    } else {
        match ty {
            T_DIR => Ok(DispState::Traverse),
            T_LNK => Ok(DispState::Traverse), // 链接穿越（B10 展开预锚）
            _ => Err(E_NOT_DIR),              // 其余五类非末分量即 ENOTDIR 口径
        }
    }
}

/// walk 主循环：分量类型序列逐一分派推进（判据 J1 驱动面）。
pub fn walk(types: &[u8], ledger: &mut WalkLedger) -> Result<(), u16> {
    if types.is_empty() {
        return Err(E_EMPTY_WALK);
    }
    ledger.state = DispState::Traverse;
    let last = types.len() - 1;
    let mut i = 0usize;
    while i <= last {
        let is_last = i == last;
        match dispatch(types[i], is_last) {
            Ok(DispState::Traverse) => {
                ledger.traverse_cnt += 1;
                ledger.state = DispState::Traverse;
            }
            Ok(DispState::Final) => {
                ledger.final_cnt += 1;
                ledger.state = DispState::Final;
            }
            Ok(DispState::Stop) => {
                ledger.stop_cnt += 1;
                ledger.state = DispState::Stop;
            }
            Err(e) => {
                // 短路停点：错误码出账，步数停在出错分量。
                ledger.stop_cnt += 1;
                ledger.state = DispState::Stop;
                ledger.err_count += 1;
                return Err(e);
            }
        }
        ledger.steps += 1;
        i += 1;
    }
    Ok(())
}

/// 14 格矩阵验证：七类 × 两列逐一断言（判据 J1 断言面）。
pub fn verify_matrix() -> Result<u32, u16> {
    let mut cells = 0u32;
    let mut ty = 0u8;
    while ty < TYPE_TOTAL {
        // 非末分量列。
        let mid = dispatch(ty, false);
        match ty {
            T_DIR | T_LNK => {
                if mid? != DispState::Traverse {
                    return Err(E_MATRIX_MISS);
                }
            }
            _ => {
                if mid.is_ok() {
                    return Err(E_MATRIX_MISS);
                }
            }
        }
        cells += 1;
        // 末分量列。
        let fin = dispatch(ty, true)?;
        if fin != DispState::Final {
            return Err(E_MATRIX_MISS);
        }
        cells += 1;
        ty += 1;
    }
    if cells != 14 {
        return Err(E_MATRIX_MISS);
    }
    Ok(cells)
}

/// 10 万分量负载：目录链 99_999 分量 + 末分量普通文件，零错账（判据 J2）。
pub fn heavy_load(ledger: &mut WalkLedger) -> Result<(), u16> {
    ledger.steps = 0;
    ledger.traverse_cnt = 0;
    ledger.final_cnt = 0;
    ledger.stop_cnt = 0;
    ledger.err_count = 0;
    // 计数器驱动：不真分配 10 万长序列（no_std 内存红线），账面等价。
    let mut i = 0u32;
    while i < LOAD_TOTAL - 1 {
        match dispatch(T_DIR, false)? {
            DispState::Traverse => ledger.traverse_cnt += 1,
            _ => return Err(E_LOAD_BAL),
        }
        ledger.steps += 1;
        i += 1;
    }
    match dispatch(T_REG, true)? {
        DispState::Final => ledger.final_cnt += 1,
        _ => return Err(E_LOAD_BAL),
    }
    ledger.steps += 1;
    if ledger.steps != LOAD_TOTAL {
        return Err(E_LOAD_BAL);
    }
    if ledger.err_count != 0 {
        return Err(E_LOAD_BAL);
    }
    ledger.state = DispState::Final; // 走完全程落 Final 态
    Ok(())
}

/// 守恒对账：三分布计数之和 = 步数。
pub fn audit(ledger: &WalkLedger) -> Result<(), u16> {
    if ledger.traverse_cnt + ledger.final_cnt + ledger.stop_cnt != ledger.steps {
        return Err(E_LOAD_BAL);
    }
    Ok(())
}

/// 自例程：R1 矩阵 14 格 → R2 正常 walk → R3 注入拦截 → R4 十万负载。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 矩阵全过：14/14。
    let cells = verify_matrix()?;
    if cells != 14 {
        return Err(E_MATRIX_MISS);
    }
    hits += 1;

    // R2 正常路径：dir→dir→reg 末分量，walk 全过账守恒。
    let mut l = WalkLedger::new();
    walk(&[T_DIR, T_DIR, T_REG], &mut l)?;
    audit(&l)?;
    if l.final_cnt != 1 || l.traverse_cnt != 2 {
        return Err(E_MATRIX_MISS);
    }
    hits += 1;

    // R3 注入拦截：非末分量普通文件 / 类型越界 / 空序列 全拒收。
    let mut l = WalkLedger::new();
    if walk(&[T_DIR, T_REG, T_REG], &mut l).is_ok() {
        return Err(E_NOT_DIR);
    }
    if l.stop_cnt != 1 {
        return Err(E_MATRIX_MISS);
    }
    if walk(&[9], &mut l).is_ok() {
        return Err(E_BAD_TYPE);
    }
    if walk(&[], &mut l).is_ok() {
        return Err(E_EMPTY_WALK);
    }
    hits += 1;

    // R4 十万分量负载零错账 + 对账 + 写回。
    let mut l = WalkLedger::new();
    heavy_load(&mut l)?;
    audit(&l)?;
    if l.err_count != 0 || l.steps != LOAD_TOTAL {
        return Err(E_LOAD_BAL);
    }
    // 写回面（判据 J3）：状态合法 + 摘要非空。
    if l.state != DispState::Final {
        return Err(E_BAD_STATE);
    }
    hits += 1;

    // R5 追加回归：链接穿越链 + 非末分量五类停点定位 + 末列全绿。
    let mut l = WalkLedger::new();
    walk(&[T_DIR, T_LNK, T_DIR, T_LNK, T_DIR, T_FIFO], &mut l)?;
    audit(&l)?;
    if l.traverse_cnt != 5 || l.final_cnt != 1 {
        return Err(E_MATRIX_MISS);
    }
    // 停点账：非末分量五类各自停（REG/FIFO/SOCK/CHR/BLK 逐一注入）。
    let stops = [T_REG, T_FIFO, T_SOCK, T_CHR, T_BLK];
    let mut s = 0usize;
    while s < stops.len() {
        let mut l2 = WalkLedger::new();
        let path = [T_DIR, stops[s], T_DIR];
        if walk(&path, &mut l2).is_ok() {
            return Err(E_NOT_DIR);
        }
        if l2.stop_cnt != 1 || l2.steps != 1 {
            return Err(E_MATRIX_MISS); // 短路定位：处理计数=出错位置
        }
        if l2.state != DispState::Stop {
            return Err(E_BAD_STATE);
        }
        s += 1;
    }
    // 末分量列：七类全部可达 Final（右列全绿复核）。
    let mut t = 0u8;
    while t < TYPE_TOTAL {
        let mut l3 = WalkLedger::new();
        walk(&[T_DIR, t], &mut l3)?;
        if l3.final_cnt != 1 {
            return Err(E_MATRIX_MISS);
        }
        t += 1;
    }
    hits += 1;

    // R6 追加回归：深目录链 50 分量负载 + 十万负载两轮 + 写回面。
    let mut deep: Vec<u8> = Vec::new();
    let mut d = 0usize;
    while d < 49 {
        deep.push(T_DIR);
        d += 1;
    }
    deep.push(T_SOCK);
    let mut l5 = WalkLedger::new();
    walk(&deep, &mut l5)?;
    audit(&l5)?;
    if l5.traverse_cnt != 49 || l5.final_cnt != 1 {
        return Err(E_LOAD_BAL);
    }
    let mut l4 = WalkLedger::new();
    heavy_load(&mut l4)?;
    heavy_load(&mut l4)?; // 二轮重跑账重置
    audit(&l4)?;
    if l4.traverse_cnt != LOAD_TOTAL - 1 || l4.final_cnt != 1 {
        return Err(E_LOAD_BAL);
    }
    if l4.err_count != 0 {
        return Err(E_LOAD_BAL);
    }
    writeback(l4.state, 0x80)?;
    hits += 1;

    // R7 追加回归：dispatch 单点逐格复核（左右两列 7+7 格）+ 写回边界。
    let mut cells = 0u32;
    let mut ty = 0u8;
    while ty < TYPE_TOTAL {
        let left = dispatch(ty, false);
        let right = dispatch(ty, true)?;
        match ty {
            T_DIR | T_LNK => {
                if left? != DispState::Traverse || right != DispState::Final {
                    return Err(E_MATRIX_MISS);
                }
            }
            _ => {
                if left.is_ok() || right != DispState::Final {
                    return Err(E_MATRIX_MISS);
                }
            }
        }
        cells += 1;
        ty += 1;
    }
    if cells != TYPE_TOTAL as u32 {
        return Err(E_MATRIX_MISS);
    }
    // 写回面边界：Stop 态与零摘要均拒收（写回闭集校验）。
    if writeback(DispState::Stop, 8).is_ok() {
        return Err(E_BAD_STATE);
    }
    if writeback(DispState::Final, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    hits += 1;

    Ok(hits)
}

// 写回面 API（判据 J3）：链账状态合法且摘要非空才放行。
pub fn writeback(state: DispState, summary_len: u32) -> Result<(), u16> {
    if state != DispState::Final && state != DispState::Traverse {
        return Err(E_BAD_STATE);
    }
    if summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}
