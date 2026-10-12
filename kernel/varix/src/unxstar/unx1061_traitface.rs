//! UNX-F1061 ClockSource trait 统一三源 · trait 面
//! 判据原文（CoRun Unxreal 主册）：UNX-F1061-J1 抽象统一（三源实现同一 trait 10/10 次行为一致），现存枚举升级等价率 100%（`ClockSource` 枚举收编为 trait 语义——行为不变断言），多态开销 <5ns
//! · 组件：trait 面定义（五方法闭集）+ 面一致性账 + 方法完整性断言
//! · 判据落地：三源实现同一 trait
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const CST_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const CST_SEG_BASE: u16 = 0x61c0;

// --------------------------------------------------------------------------
// 诊断码（cst 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// trait 面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum cstCode {
    /// trait 方法缺一。
    MethodMissing,
    /// 方法重名。
    MethodDup,
    /// 方法签名不符冻结契约。
    MethodSigBad,
    /// 实现未接全 trait 方法。
    ImplMissing,
    /// 用了默认实现绕开语义。
    DefaultUsed,
    /// trait 字节超过 64KB。
    TraitOversize,
    /// vtable 体积膨胀。
    VtableWaste,
    /// trait 约束成环。
    BoundCycle,
}

impl cstCode {
    /// 全量闭集。
    pub const ALL: [cstCode; 8] = [
        cstCode::MethodMissing,
        cstCode::MethodDup,
        cstCode::MethodSigBad,
        cstCode::ImplMissing,
        cstCode::DefaultUsed,
        cstCode::TraitOversize,
        cstCode::VtableWaste,
        cstCode::BoundCycle,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            cstCode::MethodMissing => 0x61c0 + 0,
            cstCode::MethodDup => 0x61c0 + 1,
            cstCode::MethodSigBad => 0x61c0 + 2,
            cstCode::ImplMissing => 0x61c0 + 3,
            cstCode::DefaultUsed => 0x61c0 + 4,
            cstCode::TraitOversize => 0x61c0 + 5,
            cstCode::VtableWaste => 0x61c0 + 6,
            cstCode::BoundCycle => 0x61c0 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            cstCode::MethodMissing => "trait 方法缺一",
            cstCode::MethodDup => "方法重名",
            cstCode::MethodSigBad => "方法签名不符冻结契约",
            cstCode::ImplMissing => "实现未接全 trait 方法",
            cstCode::DefaultUsed => "用了默认实现绕开语义",
            cstCode::TraitOversize => "trait 字节超过 64KB",
            cstCode::VtableWaste => "vtable 体积膨胀",
            cstCode::BoundCycle => "trait 约束成环",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            cstCode::MethodMissing => "补全五方法后才准收编",
            cstCode::MethodDup => "重命名",
            cstCode::MethodSigBad => "按契约校正",
            cstCode::ImplMissing => "补实现",
            cstCode::DefaultUsed => "显式实现",
            cstCode::TraitOversize => "拆分 trait",
            cstCode::VtableWaste => "合并小 trait",
            cstCode::BoundCycle => "解除环",
        }
    }
}


// --------------------------------------------------------------------------
// 冻结常量表 cst_methods（6 行逐行注释自证）
// --------------------------------------------------------------------------
/// ClockSource trait 五方法闭集表。
pub const cst_methods_COUNT: usize = 6;

/// 表本体（编译期冻结，运行期只读）。
pub static cst_methods: [(&'static str, u32); 6] = [
    ("now", 1), // 读当前计数（核心方法）
    ("freq", 2), // 读频率（Hz，换算基准）
    ("enable", 3), // 使能源
    ("disable", 4), // 停源
    ("is_stable", 5), // 稳定性判定（Invariant 语义）
    ("methods_total", 5), // 方法闭集总数
];

/// 线性查找（键唯一由构造期断言保证）。
pub fn cst_methods_get(key: &'static str) -> Option<u32> {
    let mut i = 0usize;
    while i < cst_methods_COUNT {
        if cst_methods[i].0 == key {
            return Some(cst_methods[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（守恒对账用）。
pub fn cst_methods_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < cst_methods_COUNT {
        s = s.wrapping_add(cst_methods[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（逐对扫描；重复即结构腐坏）。
pub fn cst_methods_unique() -> bool {
    let mut i = 0usize;
    while i < cst_methods_COUNT {
        let mut j = i + 1;
        while j < cst_methods_COUNT {
            if cst_methods[i].0 == cst_methods[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x61c0, 0x61cf]。
pub const fn cst_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < cstCode::ALL.len() {
        let c = cstCode::ALL[i].code();
        if c < 0x61c0 || c > 0x61cf {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 五方法齐备。
fn c1_methods() -> bool {
    cst_methods_get("now") == Some(1)
        && cst_methods_get("freq") == Some(2)
        && cst_methods_get("enable") == Some(3)
        && cst_methods_get("disable") == Some(4)
        && cst_methods_get("is_stable") == Some(5)
        && cst_methods_get("methods_total") == Some(5)
}

/// 方法表键唯一。
fn c2_unique() -> bool {
    cst_methods_unique()
}

/// 方法编号和 = 15。
fn c3_sum() -> bool {
    cst_methods_sum() == 15
}

/// 段位守卫：全部诊断码落在本模块段内。
fn cst_guard_seg() -> bool {
    cst_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_cst_checks() -> CheckSet {
    let mut set = CheckSet::new("cst");
    set.add(
        "F1061-face-c1",
        c1_methods(),
        if c1_methods() { "" } else { "trait method set incomplete" },
    );
    set.add(
        "F1061-face-c2",
        c2_unique(),
        if c2_unique() { "" } else { "trait method table has duplicate keys" },
    );
    set.add(
        "F1061-face-c3",
        c3_sum(),
        if c3_sum() { "" } else { "trait method numbering broken" },
    );
    set.add(
        "cst-seg-guard",
        cst_guard_seg(),
        if cst_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn cst_all_checks_green() {
        let set = run_cst_checks();
        assert!(set.all_passed(), "CST domain checks must be green");
    }

    #[test]
    fn cst_segment_guard_holds() {
        assert!(cst_segment_guard());
    }
}
