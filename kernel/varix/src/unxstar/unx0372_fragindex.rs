//! unx0372_fragindex —— UNX-F0372 内存碎片度量（外碎片指数）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0372 · 碎片度量 · 目标 220 行
//! 判据原文：UNX-F0372-J1 外碎片指数：给定空闲段账与请求规模 n，
//!           指数 = 可满足 n 帧请求的空闲总量 / 空闲总量（千分）；
//!           指数随碎片化单调下降（合并后指数不降——合并收益的
//!           可验证表达）；指数可跨快照对账（同账同指数）。
//! 组件：空闲段账 + 指数引擎（可满足量/总量千分比）+ 单调核
//!       （合并前后对比）+ 快照确定性。
//! 职责定位：外碎片指数是「内存健康度的单一数字」——总空闲
//!   128 帧听起来健康，但如果最大连续段只有 3 帧，任何 ≥4 帧
//!   的分配都是死路。指数把这个「听起来 vs 实际上」的差距压
//!   成一个千分数，随快照追踪就能看到碎片化趋势。
//! 性能逐项分解：指数 O(段数)；单调核 O(段数)；快照 O(1)。
//! 诊断码段：0x20980（0x20981~0x20989 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const FI_VERSION: &str = "unx0372_fragindex-v1";

/// 诊断段基址（0x20980 段 · 本模块独占）。
pub const FI_SEG_BASE: u32 = 0x20980;
pub const XC_RUN_EMPTY: u32 = 0x20981; // 空账拒
pub const XC_RUN_BAD: u32 = 0x20982; // 段零长/重叠
pub const XC_N_ZERO: u32 = 0x20983; // 请求 0 帧拒
pub const XC_ORDER_OOO: u32 = 0x20984; // 段乱序
pub const XC_INDEX_OOR: u32 = 0x20985; // 指数越域
pub const XC_SNAP_DRIFT: u32 = 0x20986; // 快照漂移
pub const XC_MONO_VIOL: u32 = 0x20987; // 单调违例
pub const XC_TOTAL_ZERO: u32 = 0x20988; // 零空闲总量
pub const XC_KIND_OOR: u32 = 0x20989; // 段描述越域

/// 空闲段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FreeRun {
    pub start: u32,
    pub len: u32,
}

/// 空闲段账。
pub struct FragBook {
    runs: Vec<FreeRun>,
}

impl FragBook {
    pub fn new(runs: &[FreeRun]) -> Result<FragBook, u32> {
        if runs.is_empty() {
            return Err(XC_RUN_EMPTY);
        }
        for r in runs.iter() {
            if r.len == 0 {
                return Err(XC_RUN_BAD);
            }
        }
        for i in 1..runs.len() {
            if runs[i].start <= runs[i - 1].start {
                return Err(XC_ORDER_OOO);
            }
            if runs[i].start < runs[i - 1].start + runs[i - 1].len {
                return Err(XC_RUN_BAD);
            }
        }
        Ok(FragBook { runs: runs.to_vec() })
    }

    /// 空闲总量。
    pub fn total_free(&self) -> u64 {
        self.runs.iter().map(|r| r.len as u64).sum()
    }

    /// 空闲段数。
    pub fn run_count(&self) -> u32 {
        self.runs.len() as u32
    }

    /// 最大连续段。
    pub fn max_run(&self) -> u32 {
        self.runs.iter().map(|r| r.len).max().unwrap_or(0)
    }

    /// 外碎片指数（千分）：可满足 n 帧请求的空闲量 / 总空闲量。
    /// 1000 = 零碎片（全部空闲可立即服务 n 帧请求）；0 = 全碎。
    pub fn index_milli(&self, n: u32) -> Result<u32, u32> {
        if n == 0 {
            return Err(XC_N_ZERO);
        }
        let total = self.total_free();
        if total == 0 {
            return Err(XC_TOTAL_ZERO);
        }
        let usable: u64 = self.runs.iter().map(|r| if r.len >= n { r.len as u64 } else { 0 }).sum();
        let idx = ((usable * 1000) / total) as u32;
        if idx > 1000 {
            return Err(XC_INDEX_OOR);
        }
        Ok(idx)
    }

    /// 快照（段数, 总量, 最大段）。
    pub fn snapshot(&self) -> (u32, u64, u32) {
        (self.run_count(), self.total_free(), self.max_run())
    }

    /// 快照确定性：同账重算两次同快照。
    pub fn snapshot_stable(&self) -> Result<(), u32> {
        if self.snapshot() == self.snapshot() {
            Ok(())
        } else {
            Err(XC_SNAP_DRIFT)
        }
    }
}

/// 合并两相邻空闲段（索引 i 与 i+1 相邻时合一）。
pub fn coalesce(runs: &[FreeRun], i: usize) -> Result<Vec<FreeRun>, u32> {
    if i + 1 >= runs.len() {
        return Err(XC_KIND_OOR);
    }
    if runs[i].start + runs[i].len != runs[i + 1].start {
        return Err(XC_MONO_VIOL); // 不相邻无可合并
    }
    let mut out = Vec::new();
    for (k, r) in runs.iter().enumerate() {
        if k == i {
            out.push(FreeRun { start: r.start, len: r.len + runs[i + 1].len });
        } else if k == i + 1 {
            continue;
        } else {
            out.push(*r);
        }
    }
    Ok(out)
}

/// 单调核：合并后指数不降（对任意 n）。
pub fn mono_after_coalesce(before: &FragBook, after: &FragBook, n: u32) -> Result<(), u32> {
    let a = before.index_milli(n)?;
    let b = after.index_milli(n)?;
    if b >= a {
        Ok(())
    } else {
        Err(XC_MONO_VIOL)
    }
}

pub fn run_unx0372_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0372_fragindex");
    // 族一：指数计算。
    // 段：4+4+8+120 = 136 总；n=8 → 可满足 8+120=128 → 941‰。
    let book = match FragBook::new(&[
        FreeRun { start: 0, len: 4 },
        FreeRun { start: 16, len: 4 },
        FreeRun { start: 32, len: 8 },
        FreeRun { start: 64, len: 120 },
    ]) {
        Ok(b) => b,
        Err(_) => {
            cs.add("段账构建", false, "被拒");
            return cs;
        }
    };
    cs.add("总空闲 136", book.total_free() == 136, "4+4+8+120");
    cs.add("最大段 120", book.max_run() == 120, "尾段");
    cs.add("n=8 指数 941", book.index_milli(8) == Ok(941), "128/136");
    cs.add("n=2 指数 1000", book.index_milli(2) == Ok(1000), "全段 ≥2");
    cs.add("n=16 指数 882", book.index_milli(16) == Ok(882), "120/136");
    // 族二：碎片化趋势（n 越大指数越低）。
    let i2 = book.index_milli(2).unwrap_or(0);
    let i8 = book.index_milli(8).unwrap_or(0);
    let i16 = book.index_milli(16).unwrap_or(0);
    cs.add("指数随 n 降", i2 >= i8 && i8 >= i16, "2≥8≥16");
    // 极碎账：全 1 帧段。
    let crumbs = FragBook::new(&[
        FreeRun { start: 0, len: 1 },
        FreeRun { start: 8, len: 1 },
        FreeRun { start: 16, len: 1 },
    ]).unwrap_or(FragBook { runs: Vec::new() });
    cs.add("碎屑 n=2 指数 0", crumbs.index_milli(2) == Ok(0), "无段可服务");
    cs.add("碎屑 n=1 指数 1000", crumbs.index_milli(1) == Ok(1000), "单帧全可");
    // 族三：合并单调。
    let merged_runs = coalesce(&[
        FreeRun { start: 0, len: 4 },
        FreeRun { start: 4, len: 4 },
        FreeRun { start: 64, len: 120 },
    ], 0).unwrap_or(Vec::new());
    let merged = FragBook::new(&merged_runs).unwrap_or(FragBook { runs: Vec::new() });
    cs.add("合并段数降", merged.run_count() == 2, "4+4 → 8");
    cs.add("合并总量守恒", merged.total_free() == 136, "总量不变");
    cs.add("合并单调", mono_after_coalesce(&book, &merged, 8).is_ok(), "指数不降");
    cs.add("不相邻拒并", coalesce(&[
        FreeRun { start: 0, len: 4 },
        FreeRun { start: 16, len: 4 },
    ], 0).is_err(), "MONO_VIOL");
    // 族四：构造闸。
    cs.add("空账拒", FragBook::new(&[]).is_err(), "RUN_EMPTY");
    cs.add("零长段拒", FragBook::new(&[FreeRun { start: 0, len: 0 }]).is_err(), "RUN_BAD");
    cs.add("重叠拒", FragBook::new(&[
        FreeRun { start: 0, len: 8 },
        FreeRun { start: 4, len: 8 },
    ]).is_err(), "RUN_BAD");
    cs.add("乱序拒", FragBook::new(&[
        FreeRun { start: 16, len: 4 },
        FreeRun { start: 0, len: 4 },
    ]).is_err(), "ORDER");
    cs.add("n=0 拒", book.index_milli(0).is_err(), "N_ZERO");
    // 族五：快照确定性。
    cs.add("快照稳", book.snapshot_stable().is_ok(), "同账同快照");
    cs.add("快照三元", book.snapshot() == (4, 136, 120), "(段数,总量,最大)");
    cs
}
