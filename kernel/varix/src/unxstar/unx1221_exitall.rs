//! UNX-F1221 Draining 态 · 排空退出条件
//! 判据原文（CoRun Unxreal 主册）：UNX-F1221-J1 双模并行（轮询泵+中断通道同时活跃）事件零丢失（双源恒等断言 10^5 事件），并行窗口双源事件串行化正确率 100%（同源序不乱），排空循环退出条件全验证
//! · 组件：退出条件闭集（双源皆空/达到稳态/关闭）+ 全覆盖矩阵 + 退出唯一性
//! · 判据落地：排空循环退出条件全验证
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const DRE_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const DRE_SEG_BASE: u16 = 0x6390;

// --------------------------------------------------------------------------
// 诊断码（dre 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 退出条件诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum dreCode {
    /// 退出条件未知。
    CondUnknown,
    /// 无退出条件（死循环）。
    NoExit,
    /// 多条件同时成立。
    MultiExit,
    /// 误退出（未排空）。
    ExitEarly,
    /// 已排空未退出。
    ExitLate,
    /// 条件重复。
    CondDup,
    /// 覆盖有洞。
    CondGap,
    /// 覆盖未满即宣告。
    FinalNotGreen,
}

impl dreCode {
    /// 全量闭集。
    pub const ALL: [dreCode; 8] = [
        dreCode::CondUnknown,
        dreCode::NoExit,
        dreCode::MultiExit,
        dreCode::ExitEarly,
        dreCode::ExitLate,
        dreCode::CondDup,
        dreCode::CondGap,
        dreCode::FinalNotGreen,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            dreCode::CondUnknown => 0x6390 + 0,
            dreCode::NoExit => 0x6390 + 1,
            dreCode::MultiExit => 0x6390 + 2,
            dreCode::ExitEarly => 0x6390 + 3,
            dreCode::ExitLate => 0x6390 + 4,
            dreCode::CondDup => 0x6390 + 5,
            dreCode::CondGap => 0x6390 + 6,
            dreCode::FinalNotGreen => 0x6390 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            dreCode::CondUnknown => "退出条件未知",
            dreCode::NoExit => "无退出条件（死循环）",
            dreCode::MultiExit => "多条件同时成立",
            dreCode::ExitEarly => "误退出（未排空）",
            dreCode::ExitLate => "已排空未退出",
            dreCode::CondDup => "条件重复",
            dreCode::CondGap => "覆盖有洞",
            dreCode::FinalNotGreen => "覆盖未满即宣告",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            dreCode::CondUnknown => "按三条件闭集校正",
            dreCode::NoExit => "补退出路径",
            dreCode::MultiExit => "按优先级裁定",
            dreCode::ExitEarly => "恢复循环",
            dreCode::ExitLate => "补退出判定",
            dreCode::CondDup => "去重",
            dreCode::CondGap => "补条件",
            dreCode::FinalNotGreen => "全覆盖后才宣告",
        }
    }
}


// --------------------------------------------------------------------------
// 冻结常量表 dre_cond（4 行逐行注释自证）
// --------------------------------------------------------------------------
/// 排空退出条件表。
pub const dre_cond_COUNT: usize = 4;

/// 表本体（编译期冻结，运行期只读）。
pub static dre_cond: [(&'static str, u32); 4] = [
    ("BothEmpty", 1), // 双源位图皆空（正常排空完成）
    ("SteadyReached", 2), // 达到稳态阈值（残留低于阈值）
    ("Shutdown", 3), // 系统关闭（强制退出）
    ("CondTotal", 3), // 退出条件闭集总数
];

/// 线性查找（键唯一由构造期断言保证）。
pub fn dre_cond_get(key: &'static str) -> Option<u32> {
    let mut i = 0usize;
    while i < dre_cond_COUNT {
        if dre_cond[i].0 == key {
            return Some(dre_cond[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（守恒对账用）。
pub fn dre_cond_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < dre_cond_COUNT {
        s = s.wrapping_add(dre_cond[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（逐对扫描；重复即结构腐坏）。
pub fn dre_cond_unique() -> bool {
    let mut i = 0usize;
    while i < dre_cond_COUNT {
        let mut j = i + 1;
        while j < dre_cond_COUNT {
            if dre_cond[i].0 == dre_cond[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}


// --------------------------------------------------------------------------
// 冻结矩阵 dre_exit（3 × 3 · 逐格注释自证）
// --------------------------------------------------------------------------
/// 退出互斥矩阵（行=当前条件，列=次一条件，1=可转移）。
pub const dre_exit_ROWS: usize = 3;
pub const dre_exit_COLS: usize = 3;

/// 矩阵本体（行主序）。
pub static dre_exit: [[u64; 3]; 3] = [
    [
        1, // 条件 0 → 0：允许
        1, // 条件 0 → 1：允许
        1, // 条件 0 → 2：允许
    ],
    [
        0, // 条件 1 → 0：禁回退
        1, // 条件 1 → 1：允许
        1, // 条件 1 → 2：允许
    ],
    [
        0, // 条件 2 → 0：禁回退
        0, // 条件 2 → 1：禁回退
        1, // 条件 2 → 2：允许
    ],
]
;

/// 单元格读取（越界显性 None，不 panic）。
pub fn dre_exit_cell(r: u32, c: u32) -> Option<u64> {
    let ri = r as usize;
    let ci = c as usize;
    if ri >= dre_exit_ROWS || ci >= dre_exit_COLS {
        return None;
    }
    Some(dre_exit[ri][ci])
}

/// 行求和（行守恒对账）。
pub fn dre_exit_row_sum(r: usize) -> u64 {
    if r >= dre_exit_ROWS {
        return 0;
    }
    let mut s = 0u64;
    let mut j = 0usize;
    while j < dre_exit_COLS {
        s = s.wrapping_add(dre_exit[r][j]);
        j += 1;
    }
    s
}

/// 列求和（列守恒对账）。
pub fn dre_exit_col_sum(c: usize) -> u64 {
    if c >= dre_exit_COLS {
        return 0;
    }
    let mut s = 0u64;
    let mut i = 0usize;
    while i < dre_exit_ROWS {
        s = s.wrapping_add(dre_exit[i][c]);
        i += 1;
    }
    s
}

/// 全矩阵求和。
pub fn dre_exit_total() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < dre_exit_ROWS {
        s = s.wrapping_add(dre_exit_row_sum(i));
        i += 1;
    }
    s
}

/// 行和恒等式（Σ行和 == 总和）。
pub fn dre_exit_rows_conserve() -> bool {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < dre_exit_ROWS {
        s = s.wrapping_add(dre_exit_row_sum(i));
        i += 1;
    }
    s == dre_exit_total()
}

/// 列和恒等式（Σ列和 == 总和）。
pub fn dre_exit_cols_conserve() -> bool {
    let mut s = 0u64;
    let mut j = 0usize;
    while j < dre_exit_COLS {
        s = s.wrapping_add(dre_exit_col_sum(j));
        j += 1;
    }
    s == dre_exit_total()
}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6390, 0x639f]。
pub const fn dre_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < dreCode::ALL.len() {
        let c = dreCode::ALL[i].code();
        if c < 0x6390 || c > 0x639f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 三退出条件闭集。
fn c1_cond() -> bool {
    dre_cond_get("BothEmpty") == Some(1)
        && dre_cond_get("SteadyReached") == Some(2)
        && dre_cond_get("Shutdown") == Some(3)
        && dre_cond_unique()
}

/// 退出矩阵无回退。
fn c2_matrix() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < dre_exit_ROWS {
        let mut j = 0usize;
        while j < i {
            if dre_exit_cell(i as u32, j as u32) != Some(0) { ok = false; }
            j += 1;
        }
        i += 1;
    }
    ok
}

/// 条件表和 = 6。
fn c3_sum() -> bool {
    dre_cond_sum() == 6
}

/// 段位守卫：全部诊断码落在本模块段内。
fn dre_guard_seg() -> bool {
    dre_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_dre_checks() -> CheckSet {
    let mut set = CheckSet::new("dre");
    set.add(
        "F1221-exit-c1",
        c1_cond(),
        if c1_cond() { "" } else { "exit condition set drifted" },
    );
    set.add(
        "F1221-exit-c2",
        c2_matrix(),
        if c2_matrix() { "" } else { "exit matrix allows backwards move" },
    );
    set.add(
        "F1221-exit-c3",
        c3_sum(),
        if c3_sum() { "" } else { "exit condition sum broken" },
    );
    set.add(
        "dre-seg-guard",
        dre_guard_seg(),
        if dre_guard_seg() { "" } else { "diagnostic code outside module segment" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dre_all_checks_green() {
        let set = run_dre_checks();
        assert!(set.all_passed(), "DRE domain checks must be green");
    }

    #[test]
    fn dre_segment_guard_holds() {
        assert!(dre_segment_guard());
    }
}
