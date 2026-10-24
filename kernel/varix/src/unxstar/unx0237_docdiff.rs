//! unx0237_docdiff —— UNX-F0237 A1 域文档对照（任务书 vs 实态）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0237 · 文档对照 · 目标 200 行
//! 判据原文：UNX-F0237-J1 任务书承诺项与域实态逐条对照：一致/漂移/
//!           缺失/多余四态分类，漂移与缺失逐条立案，对照一致率
//!           出账（诚实汇报不美化）。
//! 组件：承诺项（任务号 + 承诺行数）+ 实态项（任务号 + 实际行数）
//!       + 对照引擎（四态分类 + 行数容差核）+ 一致率账。
//! 职责定位：文档对照是「说与做的对账」——四态必须**完备且互斥**：
//!   两侧都有且行数在容差内=一致；两侧都有但行数漂出=漂移；只在
//!   任务书=缺失（承诺了没做）；只在实态=多余（做了没承诺，同样
//!   要立案——静默加码也是漂移的一种）。
//! 性能逐项分解：对照 O(N×M)（N=承诺数，M=实态数，域级 ≤40 批量级）。
//! 诊断码段：0x200C0（0x200C1~0x200C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DD_VERSION: &str = "unx0237_docdiff-v1";

/// 诊断段基址（0x200C0 段 · 本模块独占）。
pub const DD_SEG_BASE: u32 = 0x200C0;
pub const XC_EMPTY_PROMISE: u32 = 0x200C1; // 空承诺侧拒
pub const XC_EMPTY_ACTUAL: u32 = 0x200C2; // 空实态侧拒
pub const XC_DUP_PROMISE: u32 = 0x200C3; // 承诺重号
pub const XC_DUP_ACTUAL: u32 = 0x200C4; // 实态重号
pub const XC_TOLERANCE_OOR: u32 = 0x200C5; // 容差越域
pub const XC_NEG_LINE: u32 = 0x200C6; // 行数负值
pub const XC_DRIFT: u32 = 0x200C7; // 漂移立案存在
pub const XC_RATE_OOR: u32 = 0x200C8; // 一致率越域

/// 对照四态。
pub const K_MATCH: u8 = 0; // 两侧有且行数容差内
pub const K_DRIFT: u8 = 1; // 两侧有但行数漂出
pub const K_MISSING: u8 = 2; // 承诺有实态无
pub const K_EXTRA: u8 = 3; // 实态有承诺无

/// 行数容差（千分制：|差|/承诺 ≤ 容差即一致）。
pub const TOL_MILLI_DEFAULT: u32 = 100;

/// 对照结论行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffRow {
    pub task: u32,
    pub kind: u8,
    /// 承诺行数（缺失态为 0）。
    pub promised: u32,
    /// 实态行数（多余态的 promised 侧为 0）。
    pub actual: u32,
}

/// 对照引擎：任务书承诺 vs 域实态。
pub fn doc_diff(
    promises: &[(u32, u32)],
    actuals: &[(u32, u32)],
    tol_milli: u32,
) -> Result<Vec<DiffRow>, u32> {
    if promises.is_empty() {
        return Err(XC_EMPTY_PROMISE);
    }
    if actuals.is_empty() {
        return Err(XC_EMPTY_ACTUAL);
    }
    if tol_milli > 1000 {
        return Err(XC_TOLERANCE_OOR);
    }
    for (id, l) in promises.iter() {
        if promises.iter().filter(|(i, _)| i == id).count() > 1 {
            return Err(XC_DUP_PROMISE);
        }
        if *l == 0 {
            return Err(XC_NEG_LINE);
        }
    }
    for (id, l) in actuals.iter() {
        if actuals.iter().filter(|(i, _)| i == id).count() > 1 {
            return Err(XC_DUP_ACTUAL);
        }
        if *l == 0 {
            return Err(XC_NEG_LINE);
        }
    }
    let mut rows = Vec::new();
    // 承诺侧：命中 → 容差核；未命中 → 缺失。
    for (id, pl) in promises.iter() {
        match actuals.iter().find(|(i, _)| i == id) {
            Some((_, al)) => {
                let diff = if pl > al { pl - al } else { al - pl };
                let drift = (diff as u64) * 1000 > (*pl as u64) * (tol_milli as u64);
                rows.push(DiffRow {
                    task: *id,
                    kind: if drift { K_DRIFT } else { K_MATCH },
                    promised: *pl,
                    actual: *al,
                });
            }
            None => rows.push(DiffRow { task: *id, kind: K_MISSING, promised: *pl, actual: 0 }),
        }
    }
    // 实态侧多余：承诺没有的。
    for (id, al) in actuals.iter() {
        if !promises.iter().any(|(i, _)| i == id) {
            rows.push(DiffRow { task: *id, kind: K_EXTRA, promised: 0, actual: *al });
        }
    }
    Ok(rows)
}

/// 一致率（千分）：MATCH 数 / 总对照行数。
pub fn match_rate_milli(rows: &[DiffRow]) -> Result<u32, u32> {
    if rows.is_empty() {
        return Err(XC_RATE_OOR);
    }
    let m = rows.iter().filter(|r| r.kind == K_MATCH).count();
    Ok(((m as u64) * 1000 / rows.len() as u64) as u32)
}

/// 立案清单：漂移 + 缺失 + 多余全要立案（任务号升序）。
pub fn tickets(rows: &[DiffRow]) -> Vec<u32> {
    let mut t: Vec<u32> = rows
        .iter()
        .filter(|r| r.kind != K_MATCH)
        .map(|r| r.task)
        .collect();
    t.sort_unstable();
    t.dedup();
    t
}

pub fn run_unx0237_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0237_docdiff");
    // 族一：全一致。
    let prom: Vec<(u32, u32)> = vec![(101, 200), (102, 300), (103, 240)];
    let act: Vec<(u32, u32)> = vec![(101, 205), (102, 300), (103, 230)];
    let rows = match doc_diff(&prom, &act, TOL_MILLI_DEFAULT) {
        Ok(r) => r,
        Err(_) => {
            cs.add("对照构造", false, "被拒");
            return cs;
        }
    };
    cs.add("全一致", rows.iter().all(|r| r.kind == K_MATCH), "±5% 容差内");
    cs.add("一致率满", match_rate_milli(&rows) == Ok(1000), "3/3");
    cs.add("零立案", tickets(&rows).is_empty(), "无漂移");
    // 族二：四态各现。
    let prom2: Vec<(u32, u32)> = vec![(1, 200), (2, 200), (3, 200)];
    let act2: Vec<(u32, u32)> = vec![(1, 260), (2, 210), (4, 150)];
    let rows2 = doc_diff(&prom2, &act2, TOL_MILLI_DEFAULT).unwrap_or(rows);
    cs.add("漂移态", rows2.iter().any(|r| r.task == 1 && r.kind == K_DRIFT), "260 超 ±10%");
    cs.add("一致态", rows2.iter().any(|r| r.task == 2 && r.kind == K_MATCH), "210 在容差");
    cs.add("缺失态", rows2.iter().any(|r| r.task == 3 && r.kind == K_MISSING), "承诺无实态");
    cs.add("多余态", rows2.iter().any(|r| r.task == 4 && r.kind == K_EXTRA), "实态无承诺");
    cs.add("立案三条", tickets(&rows2).as_slice() == &[1u32, 3, 4], "升序去重");
    cs.add("一致率半", match_rate_milli(&rows2) == Ok(250), "1/4");
    // 族三：容差边界（恰好 10% = 220 → 200×1.10，diff*1000 == pl*tol → 不漂）。
    let edge = doc_diff(&[(9u32, 200u32)][..], &[(9u32, 220u32)][..], 100);
    cs.add("边界不漂", matches!(edge, Ok(ref r) if r[0].kind == K_MATCH), "恰在容差线");
    let over = doc_diff(&[(9u32, 200u32)][..], &[(9u32, 221u32)][..], 100);
    cs.add("出界即漂", matches!(over, Ok(ref r) if r[0].kind == K_DRIFT), "221 超线");
    // 族四：构造拒。
    cs.add("空承诺拒", doc_diff(&[], &[(1, 1)], 100).is_err(), "EMPTY_PROMISE");
    cs.add("空实态拒", doc_diff(&[(1, 1)], &[], 100).is_err(), "EMPTY_ACTUAL");
    cs.add("承诺重号拒", doc_diff(&[(1, 10), (1, 20)], &[(1, 10)], 100).is_err(), "DUP");
    cs.add("实态重号拒", doc_diff(&[(1, 10)], &[(1, 10), (1, 10)], 100).is_err(), "DUP");
    cs.add("零行拒", doc_diff(&[(1, 0)], &[(1, 1)], 100).is_err(), "NEG");
    cs.add("容差越域拒", doc_diff(&[(1, 1)], &[(1, 1)], 1001).is_err(), "TOL");
    // 族五：空对照行一致率拒。
    cs.add("空行率拒", match_rate_milli(&[]).is_err(), "RATE");
    cs
}
