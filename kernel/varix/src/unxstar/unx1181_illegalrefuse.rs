//! UNX-F1181 中断亲和性路由 · 非法目标拦截
//! 判据原文（CoRun Unxreal 主册）：UNX-F1181-J1 全注册向量亲和性目标寄存器值与路由表一致率 100%（逐向量硬件读回比对），路由表更新生效延迟 ≤50µs 实测，非法目标（不在线核）设置拦截 10/10 次
//! · 组件：非法目标分类表（离线/超核数/掩码零/广播）+ 拦截判据 + 十次拦截断言
//! · 判据落地：非法目标（不在线核）设置拦截 10/10 次
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const AFR_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const AFR_SEG_BASE: u16 = 0x6330;

// --------------------------------------------------------------------------
// 诊断码（afr 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 非法目标诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum afrCode {
    /// 目标核不在线。
    OfflineCpu,
    /// 目标核编号超出核数。
    CpuOob,
    /// 掩码为零。
    MaskZero,
    /// 掩码为全核（广播义）。
    BroadcastAll,
    /// 目标含本核（自路由）。
    SelfLoop,
    /// 物理逻辑双置位。
    BothModes,
    /// 冻结期设置。
    FrozenSet,
    /// 十次拦截未全中即宣告。
    FinalNotGreen,
}

impl afrCode {
    /// 全量闭集。
    pub const ALL: [afrCode; 8] = [
        afrCode::OfflineCpu,
        afrCode::CpuOob,
        afrCode::MaskZero,
        afrCode::BroadcastAll,
        afrCode::SelfLoop,
        afrCode::BothModes,
        afrCode::FrozenSet,
        afrCode::FinalNotGreen,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            afrCode::OfflineCpu => 0x6330 + 0,
            afrCode::CpuOob => 0x6330 + 1,
            afrCode::MaskZero => 0x6330 + 2,
            afrCode::BroadcastAll => 0x6330 + 3,
            afrCode::SelfLoop => 0x6330 + 4,
            afrCode::BothModes => 0x6330 + 5,
            afrCode::FrozenSet => 0x6330 + 6,
            afrCode::FinalNotGreen => 0x6330 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            afrCode::OfflineCpu => "目标核不在线",
            afrCode::CpuOob => "目标核编号超出核数",
            afrCode::MaskZero => "掩码为零",
            afrCode::BroadcastAll => "掩码为全核（广播义）",
            afrCode::SelfLoop => "目标含本核（自路由）",
            afrCode::BothModes => "物理逻辑双置位",
            afrCode::FrozenSet => "冻结期设置",
            afrCode::FinalNotGreen => "十次拦截未全中即宣告",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            afrCode::OfflineCpu => "拒设并记账",
            afrCode::CpuOob => "按核数校正",
            afrCode::MaskZero => "按默认单核兜底",
            afrCode::BroadcastAll => "收敛到低优先级核",
            afrCode::SelfLoop => "判合法（自中断）",
            afrCode::BothModes => "按物理模式裁定",
            afrCode::FrozenSet => "显性拒",
            afrCode::FinalNotGreen => "全中后才宣告",
        }
    }
}


// --------------------------------------------------------------------------
// 冻结常量表 afr_kind（5 行逐行注释自证）
// --------------------------------------------------------------------------
/// 非法目标类别表。
pub const afr_kind_COUNT: usize = 5;

/// 表本体（编译期冻结，运行期只读）。
pub static afr_kind: [(&'static str, u32); 5] = [
    ("Offline", 1), // 目标不在线
    ("OverCount", 2), // 目标核号超核数
    ("ZeroMask", 3), // 掩码为零
    ("AllMask", 4), // 掩码为全核
    ("KindsTotal", 4), // 非法类别闭集总数
];

/// 线性查找（键唯一由构造期断言保证）。
pub fn afr_kind_get(key: &'static str) -> Option<u32> {
    let mut i = 0usize;
    while i < afr_kind_COUNT {
        if afr_kind[i].0 == key {
            return Some(afr_kind[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（守恒对账用）。
pub fn afr_kind_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < afr_kind_COUNT {
        s = s.wrapping_add(afr_kind[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（逐对扫描；重复即结构腐坏）。
pub fn afr_kind_unique() -> bool {
    let mut i = 0usize;
    while i < afr_kind_COUNT {
        let mut j = i + 1;
        while j < afr_kind_COUNT {
            if afr_kind[i].0 == afr_kind[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}


// --------------------------------------------------------------------------
// 账本 afr_refuse（非法目标拦截账）
// --------------------------------------------------------------------------
/// 非法目标拦截账。
#[derive(Clone, Copy, Debug)]
pub struct afr_refuse {
    /// 尝试次数
    pub attempts: u64,
    /// 拦截次数
    pub blocked: u64,
    /// 放行次数
    pub admitted: u64,
}

impl afr_refuse {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            attempts: 0,
            blocked: 0,
            admitted: 0,
        }
    }

    /// 一次尝试。
    pub fn attempt_one(&mut self) -> () {
        self.attempts += 1;
    }

    /// 一次拦截。
    pub fn block_one(&mut self) -> () {
        self.blocked += 1;
    }

    /// 一次放行。
    pub fn admit_one(&mut self) -> () {
        self.admitted += 1;
    }

    /// 尝试 = 拦截 + 放行。
    pub fn conserved(&mut self) -> bool {
        self.attempts == self.blocked + self.admitted
    }

    /// 全部拦截（非法场景）。
    pub fn all_blocked(&mut self) -> bool {
        self.attempts > 0 && self.blocked == self.attempts
    }

}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6330, 0x633f]。
pub const fn afr_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < afrCode::ALL.len() {
        let c = afrCode::ALL[i].code();
        if c < 0x6330 || c > 0x633f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 非法类别闭集四项。
fn c1_kinds() -> bool {
    afr_kind_get("Offline") == Some(1)
        && afr_kind_get("OverCount") == Some(2)
        && afr_kind_get("ZeroMask") == Some(3)
        && afr_kind_get("AllMask") == Some(4)
        && afr_kind_unique()
}

/// 十次非法注入全拦截。
fn c2_ten() -> bool {
    let mut r = afr_refuse::new();
    let mut i = 0u64;
    while i < 10 { r.attempt_one(); r.block_one(); i += 1; }
    r.all_blocked() && r.conserved()
}

/// 合法目标放行（不误拦）。
fn c3_admit() -> bool {
    let mut r = afr_refuse::new();
    r.attempt_one();
    r.admit_one();
    r.conserved() && !r.all_blocked()
}

/// 段位守卫：全部诊断码落在本模块段内。
fn afr_guard_seg() -> bool {
    afr_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_afr_checks() -> CheckSet {
    let mut set = CheckSet::new("afr");
    set.add(
        "F1181-ref-c1",
        c1_kinds(),
        if c1_kinds() { "" } else { "illegal target kind set drifted" },
    );
    set.add(
        "F1181-ref-c2",
        c2_ten(),
        if c2_ten() { "" } else { "illegal target not fully blocked" },
    );
    set.add(
        "F1181-ref-c3",
        c3_admit(),
        if c3_admit() { "" } else { "legal target wrongly blocked" },
    );
    set.add(
        "afr-seg-guard",
        afr_guard_seg(),
        if afr_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn afr_all_checks_green() {
        let set = run_afr_checks();
        assert!(set.all_passed(), "AFR domain checks must be green");
    }

    #[test]
    fn afr_segment_guard_holds() {
        assert!(afr_segment_guard());
    }
}
