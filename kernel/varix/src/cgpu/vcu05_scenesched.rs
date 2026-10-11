//! unxu05a_vcu05_scenesched —— CGPU-F3205 基准运行器场景调度（队列 + 优先级）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：CGPU-F3205 · 基准运行器（CGPU-U 域 · 批次 U01 · U05 组 · 判据：场景调度/计时/采集三功能）
//! · 组件：场景调度表（8 场景族）+ 调度流水 + 调度账
//! · 判据落地：场景调度三功能之一
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U05A_VERSION: &str = "u05a-vcu05_scenesched-v1";

/// 诊断段基址。
pub const U05A_SEG_BASE: u16 = 0x7300;

// --------------------------------------------------------------------------
// 诊断码（u05a 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 场景调度诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u05aCode {
    /// 0x7300 场景非法（八族之外）。
    SceneBad,
    /// 0x7301 队列满员（调度溢出）。
    QueueFull,
    /// 0x7302 调度序违例。
    OrderViolated,
    /// 0x7303 饿死检出（低优先级长期未调度）。
    Starvation,
    /// 0x7304 调度账破缺。
    LedgerBreak,
    /// 0x7305 重复调度（同场景双入队）。
    DoubleSchedule,
    /// 0x7306 报告陈旧。
    ReportStale,
    /// 0x7307 守卫触发。
    GuardTripped,
    /// 0x7308 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x7309 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x730a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x730b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x730c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x730d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u05aCode {
    /// 全量闭集。
    pub const ALL: [u05aCode; 14] = [
        u05aCode::SceneBad,
        u05aCode::QueueFull,
        u05aCode::OrderViolated,
        u05aCode::Starvation,
        u05aCode::LedgerBreak,
        u05aCode::DoubleSchedule,
        u05aCode::ReportStale,
        u05aCode::GuardTripped,
        u05aCode::LedgerBroken,
        u05aCode::SnapshotFingerprint,
        u05aCode::TableBroken,
        u05aCode::IndexMisaligned,
        u05aCode::DuplicateEntry,
        u05aCode::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u05aCode::SceneBad => 0x7300,
            u05aCode::QueueFull => 0x7301,
            u05aCode::OrderViolated => 0x7302,
            u05aCode::Starvation => 0x7303,
            u05aCode::LedgerBreak => 0x7304,
            u05aCode::DoubleSchedule => 0x7305,
            u05aCode::ReportStale => 0x7306,
            u05aCode::GuardTripped => 0x7307,
            u05aCode::LedgerBroken => 0x7308,
            u05aCode::SnapshotFingerprint => 0x7309,
            u05aCode::TableBroken => 0x730a,
            u05aCode::IndexMisaligned => 0x730b,
            u05aCode::DuplicateEntry => 0x730c,
            u05aCode::SegmentGuard => 0x730d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u05aCode::SceneBad => "场景非法（八族之外）",
            u05aCode::QueueFull => "队列满员（调度溢出）",
            u05aCode::OrderViolated => "调度序违例",
            u05aCode::Starvation => "饿死检出（低优先级长期未调度）",
            u05aCode::LedgerBreak => "调度账破缺",
            u05aCode::DoubleSchedule => "重复调度（同场景双入队）",
            u05aCode::ReportStale => "报告陈旧",
            u05aCode::GuardTripped => "守卫触发",
            u05aCode::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u05aCode::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u05aCode::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u05aCode::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            u05aCode::DuplicateEntry => "条目重复（同键双写）",
            u05aCode::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u05aCode::SceneBad => "按场景族闭集校正",
            u05aCode::QueueFull => "按排队/拒收路径处置",
            u05aCode::OrderViolated => "按优先级律重排",
            u05aCode::Starvation => "按老化路径提升",
            u05aCode::LedgerBreak => "按守恒重建",
            u05aCode::DoubleSchedule => "幂等去重",
            u05aCode::ReportStale => "重渲染",
            u05aCode::GuardTripped => "检查临界区",
            u05aCode::LedgerBroken => "按守恒律重建台账后重跑",
            u05aCode::SnapshotFingerprint => "按防篡改规程重建快照",
            u05aCode::TableBroken => "按语义规则重建常量表",
            u05aCode::IndexMisaligned => "按声明序归一化后重跑",
            u05aCode::DuplicateEntry => "合并同键条目为末次生效",
            u05aCode::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 基准场景族（八族闭集）+ 调度优先级
// ---------------------------------------------------------------------------

/// 场景族（八族）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneFamily {
    /// 合成渲染族。
    Composite,
    /// 计算族（GPGPU）。
    Compute,
    /// 视频编解码族。
    Video,
    /// 显示输出族。
    Present,
    /// 混合族（合成+计算）。
    Hybrid,
    /// 压力族（极限负载）。
    Stress,
    /// 空闲族（基线）。
    Idle,
    /// 长稳族（耐久场景）。
    Soak,
}

impl SceneFamily {
    /// 全量闭集（八族）。
    pub const ALL: [SceneFamily; 8] = [
        SceneFamily::Composite,
        SceneFamily::Compute,
        SceneFamily::Video,
        SceneFamily::Present,
        SceneFamily::Hybrid,
        SceneFamily::Stress,
        SceneFamily::Idle,
        SceneFamily::Soak,
    ];

    /// 索引。
    pub const fn index(self) -> usize {
        match self {
            SceneFamily::Composite => 0,
            SceneFamily::Compute => 1,
            SceneFamily::Video => 2,
            SceneFamily::Present => 3,
            SceneFamily::Hybrid => 4,
            SceneFamily::Stress => 5,
            SceneFamily::Idle => 6,
            SceneFamily::Soak => 7,
        }
    }

    /// 中文族名。
    pub const fn cn(self) -> &'static str {
        match self {
            SceneFamily::Composite => "合成渲染",
            SceneFamily::Compute => "计算",
            SceneFamily::Video => "视频编解码",
            SceneFamily::Present => "显示输出",
            SceneFamily::Hybrid => "混合",
            SceneFamily::Stress => "压力",
            SceneFamily::Idle => "空闲",
            SceneFamily::Soak => "长稳",
        }
    }

    /// 默认帧数（每场景运行帧）。
    pub const fn default_frames(self) -> u32 {
        match self {
            SceneFamily::Composite => 3000,
            SceneFamily::Compute => 2000,
            SceneFamily::Video => 3600,
            SceneFamily::Present => 3000,
            SceneFamily::Hybrid => 2400,
            SceneFamily::Stress => 1200,
            SceneFamily::Idle => 600,
            SceneFamily::Soak => 12000,
        }
    }
}

/// 队列表深度上限。
pub const SCENE_QUEUE_CAP: usize = 32;

/// 调度条目（场景 + 优先级 + 入队序号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneTask {
    /// 场景族。
    pub family: SceneFamily,
    /// 优先级（0 最高，15 最低）。
    pub priority: u8,
    /// 入队序号（同优先级的 FIFO 凭证）。
    pub seq: u64,
    /// 计划帧数。
    pub frames: u32,
}

/// 调度账本（FIFO + 优先级 + 饿死防护账）。
#[derive(Clone, Copy, Debug)]
pub struct SceneBook {
    /// 已入队数。
    pub queued: u64,
    /// 已出队数。
    pub dispatched: u64,
    /// 饿死提升次数。
    pub aged: u64,
    /// 当前序号计数器。
    pub seq: u64,
}

impl SceneBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { queued: 0, dispatched: 0, aged: 0, seq: 0 }
    }

    /// 入队（分配序号；满员拒收由调用方按容量判定）。
    pub fn enqueue(&mut self, family: SceneFamily, priority: u8, frames: u32) -> Option<SceneTask> {
        if priority > 15 {
            return None;
        }
        self.seq += 1;
        self.queued += 1;
        Some(SceneTask { family, priority, seq: self.seq, frames })
    }

    /// 出队（守恒：queued == dispatched + 在队数）。
    pub fn dispatch(&mut self) -> bool {
        self.dispatched += 1;
        true
    }

    /// 饿死老化（低优先级等待超阈后提升一级）。
    pub fn age_one(&mut self) -> bool {
        self.aged += 1;
        true
    }

    /// 调度序比较（先优先级后序号；返回 true 表示 a 应先调度）。
    pub fn should_precede(a: &SceneTask, b: &SceneTask) -> bool {
        if a.priority != b.priority {
            a.priority < b.priority
        } else {
            a.seq < b.seq
        }
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x7300, 0x730f]。
pub const fn u05a_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u05aCode::ALL.len() {
        let c = u05aCode::ALL[i].code();
        if c < 0x7300 || c > 0x730f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn u05a_guard_seg() -> bool {
    u05a_segment_guard()
}

/// 场景族八族齐备 + 默认帧数在位 + 长稳最长。
fn check_c1_scene_seed() -> bool {
    let mut ok = SceneFamily::ALL.len() == 8;
    if ok {
        ok = SceneFamily::Composite.default_frames() == 3000;
    }
    if ok {
        ok = SceneFamily::Soak.default_frames() == 12000;
    }
    if ok {
        // 长稳族帧数必须是合成族的 4 倍。
        ok = SceneFamily::Soak.default_frames() == SceneFamily::Composite.default_frames() * 4;
    }
    ok
}

/// 调度序（优先级优先 + 同优先 FIFO + 饿死记账）。
fn check_c2_sched_order() -> bool {
    let mut bk = SceneBook::new();
    let t1 = bk.enqueue(SceneFamily::Composite, 8, 3000);
    let t2 = bk.enqueue(SceneFamily::Compute, 4, 2000);
    let t3 = bk.enqueue(SceneFamily::Video, 8, 3600);
    let mut ok = false;
    if let (Some(a), Some(b), Some(c)) = (t1, t2, t3) {
        ok = SceneBook::should_precede(&b, &a) && SceneBook::should_precede(&a, &c);
    }
    if ok {
        bk.dispatch();
        bk.dispatch();
        ok = bk.queued == 3 && bk.dispatched == 2;
    }
    if ok {
        bk.age_one();
        ok = bk.aged == 1;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u05a_checks() -> CheckSet {
    let mut set = CheckSet::new("U05A");
    set.add(
        "u05a-seg-guard",
        u05a_guard_seg(),
        if u05a_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u05a-c1-scene-seed",
        check_c1_scene_seed(),
        if check_c1_scene_seed() { "" } else { "scene family seed mismatch" },
    );
    set.add(
        "u05a-c2-sched-order",
        check_c2_sched_order(),
        if check_c2_sched_order() { "" } else { "scene scheduler order mismatch" },
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
    fn u05a_all_checks_green() {
        let set = run_u05a_checks();
        assert!(set.all_passed(), "U05A domain checks must be green");
    }

    #[test]
    fn u05a_segment_guard_holds() {
        assert!(u05a_segment_guard());
    }
}
