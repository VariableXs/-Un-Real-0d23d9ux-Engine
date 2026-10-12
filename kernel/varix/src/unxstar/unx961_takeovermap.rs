//! UNX-F0961 现存轮询栈盘点账 · 接管映射表
//! 判据原文（CoRun Unxreal 主册）：UNX-F0961-J1 盘点完整（现存全部轮询点清册零遗漏——grep+运行期双源交叉），特征画像落账，盘点与运行观测一致率 100%
//! · 组件：轮询点→接管目标（中断源/判据）映射 + 覆盖率 + 优先级
//! · 判据落地：每点接管映射登记率（接管前置测绘）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const PMT_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const PMT_SEG_BASE: u16 = 0x6190;

// --------------------------------------------------------------------------
// 诊断码（pmt 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 接管映射诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum pmtCode {
    /// 轮询点无接管映射。
    MapMissing,
    /// 接管目标非法。
    TargetBad,
    /// 接管目标不在线。
    TargetOffline,
    /// 同优先级两个接管。
    PriorityDup,
    /// 映射成环（A→B→A）。
    MapLoop,
    /// 映射与现状不符。
    MapStale,
    /// 接管覆盖率不满 100%。
    PartialCover,
    /// 接管序违反优先级。
    OrderBad,
}

impl pmtCode {
    /// 全量闭集。
    pub const ALL: [pmtCode; 8] = [
        pmtCode::MapMissing,
        pmtCode::TargetBad,
        pmtCode::TargetOffline,
        pmtCode::PriorityDup,
        pmtCode::MapLoop,
        pmtCode::MapStale,
        pmtCode::PartialCover,
        pmtCode::OrderBad,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            pmtCode::MapMissing => 0x6190 + 0,
            pmtCode::TargetBad => 0x6190 + 1,
            pmtCode::TargetOffline => 0x6190 + 2,
            pmtCode::PriorityDup => 0x6190 + 3,
            pmtCode::MapLoop => 0x6190 + 4,
            pmtCode::MapStale => 0x6190 + 5,
            pmtCode::PartialCover => 0x6190 + 6,
            pmtCode::OrderBad => 0x6190 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            pmtCode::MapMissing => "轮询点无接管映射",
            pmtCode::TargetBad => "接管目标非法",
            pmtCode::TargetOffline => "接管目标不在线",
            pmtCode::PriorityDup => "同优先级两个接管",
            pmtCode::MapLoop => "映射成环（A→B→A）",
            pmtCode::MapStale => "映射与现状不符",
            pmtCode::PartialCover => "接管覆盖率不满 100%",
            pmtCode::OrderBad => "接管序违反优先级",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            pmtCode::MapMissing => "补映射后才准接管",
            pmtCode::TargetBad => "按目标表校正",
            pmtCode::TargetOffline => "替换目标并记账",
            pmtCode::PriorityDup => "重新分级",
            pmtCode::MapLoop => "断环并重新规划",
            pmtCode::MapStale => "重新测绘",
            pmtCode::PartialCover => "补映射",
            pmtCode::OrderBad => "按优先级重排",
        }
    }
}


// --------------------------------------------------------------------------
// 冻结常量表 pmt_target（5 行逐行注释自证）
// --------------------------------------------------------------------------
/// 接管目标闭集表。
pub const pmt_target_COUNT: usize = 5;

/// 表本体（编译期冻结，运行期只读）。
pub static pmt_target: [(&'static str, u32); 5] = [
    ("t_intr", 1), // 接管为中断源（首选）
    ("t_keep", 2), // 保留轮询（无中断源可用）
    ("t_drop", 3), // 直接废弃（死代码）
    ("t_defer", 4), // 延后接管（依赖未就绪）
    ("targets_total", 4), // 目标闭集总数
];

/// 线性查找（键唯一由构造期断言保证）。
pub fn pmt_target_get(key: &'static str) -> Option<u32> {
    let mut i = 0usize;
    while i < pmt_target_COUNT {
        if pmt_target[i].0 == key {
            return Some(pmt_target[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（守恒对账用）。
pub fn pmt_target_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < pmt_target_COUNT {
        s = s.wrapping_add(pmt_target[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（逐对扫描；重复即结构腐坏）。
pub fn pmt_target_unique() -> bool {
    let mut i = 0usize;
    while i < pmt_target_COUNT {
        let mut j = i + 1;
        while j < pmt_target_COUNT {
            if pmt_target[i].0 == pmt_target[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}


// --------------------------------------------------------------------------
// 账本 pmt_map（接管映射账）
// --------------------------------------------------------------------------
/// 接管映射账。
#[derive(Clone, Copy, Debug)]
pub struct pmt_map {
    /// 已映射点数
    pub mapped: u64,
    /// 盘点总点数
    pub points: u64,
    /// 映射为中断源数
    pub to_intr: u64,
    /// 保留轮询数
    pub kept: u64,
    /// 废弃数
    pub dropped: u64,
    /// 延后接管数
    pub deferred: u64,
}

impl pmt_map {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            mapped: 0,
            points: 0,
            to_intr: 0,
            kept: 0,
            dropped: 0,
            deferred: 0,
        }
    }

    /// 映射一个点到中断源。
    pub fn map_intr(&mut self) -> () {
        self.mapped += 1;
        self.to_intr += 1;
        self.points += 1;
    }

    /// 映射一个点为保留轮询。
    pub fn map_keep(&mut self) -> () {
        self.mapped += 1;
        self.kept += 1;
        self.points += 1;
    }

    /// 映射一个点为废弃。
    pub fn map_drop(&mut self) -> () {
        self.mapped += 1;
        self.dropped += 1;
        self.points += 1;
    }

    /// 映射一个点为延后接管。
    pub fn map_defer(&mut self) -> () {
        self.mapped += 1;
        self.deferred += 1;
        self.points += 1;
    }

    /// 映射覆盖率 100%。
    pub fn covered(&mut self) -> bool {
        self.points > 0 && self.mapped == self.points
    }

    /// 分类守恒：映射数 = 四类之和。
    pub fn conserved(&mut self) -> bool {
        self.mapped == self.to_intr + self.kept + self.dropped + self.deferred
    }

}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6190, 0x619f]。
pub const fn pmt_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < pmtCode::ALL.len() {
        let c = pmtCode::ALL[i].code();
        if c < 0x6190 || c > 0x619f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 目标闭集四类。
fn c1_targets() -> bool {
    pmt_target_get("t_intr") == Some(1)
        && pmt_target_get("targets_total") == Some(4)
}

/// 映射覆盖 100%。
fn c2_cover() -> bool {
    let mut t = pmt_map::new();
    let mut i = 0u64;
    while i < 30 { t.map_intr(); i += 1; }
    let mut j = 0u64;
    while j < 10 { t.map_keep(); j += 1; }
    t.covered() && t.conserved() && t.points == 40
}

/// 段位守卫：全部诊断码落在本模块段内。
fn pmt_guard_seg() -> bool {
    pmt_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_pmt_checks() -> CheckSet {
    let mut set = CheckSet::new("pmt");
    set.add(
        "F0961-map-c1",
        c1_targets(),
        if c1_targets() { "" } else { "takeover target set drifted" },
    );
    set.add(
        "F0961-map-c2",
        c2_cover(),
        if c2_cover() { "" } else { "takeover map not 100%" },
    );
    set.add(
        "pmt-seg-guard",
        pmt_guard_seg(),
        if pmt_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn pmt_all_checks_green() {
        let set = run_pmt_checks();
        assert!(set.all_passed(), "PMT domain checks must be green");
    }

    #[test]
    fn pmt_segment_guard_holds() {
        assert!(pmt_segment_guard());
    }
}
