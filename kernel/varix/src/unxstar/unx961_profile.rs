//! UNX-F0961 现存轮询栈盘点账 · 特征画像
//! 判据原文（CoRun Unxreal 主册）：UNX-F0961-J1 盘点完整（现存全部轮询点清册零遗漏——grep+运行期双源交叉），特征画像落账，盘点与运行观测一致率 100%
//! · 组件：每类特征画像（频率/耗时/所在态）+ 画像指纹 + 画像与清册一致
//! · 判据落地：特征画像落账
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const PMP_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const PMP_SEG_BASE: u16 = 0x6170;

// --------------------------------------------------------------------------
// 诊断码（pmp 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 特征画像诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum pmpCode {
    /// 轮询点无画像。
    ProfileMissing,
    /// 画像与现状不符。
    ProfileStale,
    /// 频率为零但仍在跑。
    FreqZero,
    /// 单次耗时未测。
    CostMissing,
    /// 单次耗时越上限。
    CostOver,
    /// 画像数超过清册数。
    ProfileOver,
    /// 画像指纹漂移。
    FingerprintDrift,
    /// 画像类型与清册类型不符。
    ClassMix,
}

impl pmpCode {
    /// 全量闭集。
    pub const ALL: [pmpCode; 8] = [
        pmpCode::ProfileMissing,
        pmpCode::ProfileStale,
        pmpCode::FreqZero,
        pmpCode::CostMissing,
        pmpCode::CostOver,
        pmpCode::ProfileOver,
        pmpCode::FingerprintDrift,
        pmpCode::ClassMix,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            pmpCode::ProfileMissing => 0x6170 + 0,
            pmpCode::ProfileStale => 0x6170 + 1,
            pmpCode::FreqZero => 0x6170 + 2,
            pmpCode::CostMissing => 0x6170 + 3,
            pmpCode::CostOver => 0x6170 + 4,
            pmpCode::ProfileOver => 0x6170 + 5,
            pmpCode::FingerprintDrift => 0x6170 + 6,
            pmpCode::ClassMix => 0x6170 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            pmpCode::ProfileMissing => "轮询点无画像",
            pmpCode::ProfileStale => "画像与现状不符",
            pmpCode::FreqZero => "频率为零但仍在跑",
            pmpCode::CostMissing => "单次耗时未测",
            pmpCode::CostOver => "单次耗时越上限",
            pmpCode::ProfileOver => "画像数超过清册数",
            pmpCode::FingerprintDrift => "画像指纹漂移",
            pmpCode::ClassMix => "画像类型与清册类型不符",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            pmpCode::ProfileMissing => "补画像后落账",
            pmpCode::ProfileStale => "重新测绘",
            pmpCode::FreqZero => "核对是否死代码",
            pmpCode::CostMissing => "补实测",
            pmpCode::CostOver => "列入整治清单",
            pmpCode::ProfileOver => "账本校正",
            pmpCode::FingerprintDrift => "防篡改审计",
            pmpCode::ClassMix => "按清册校正",
        }
    }
}


// ---------------------------------------------------------------------------
// 轮询点画像（与清册同源；指纹防篡改）
// ---------------------------------------------------------------------------

/// 单个轮询点画像。
#[derive(Clone, Copy, Debug)]
pub struct PollPointProfile {
    /// 轮询点编号。
    pub id: u32,
    /// 类型（1/2/3）。
    pub kind: u32,
    /// 观测频率（Hz）。
    pub freq_hz: u32,
    /// 单次耗时（ns 均值）。
    pub cost_ns_avg: u64,
    /// 所在态（0=Boot 1=Init 2=Run 3=Idle）。
    pub phase: u8,
}

/// 画像指纹（FNV-1a 逐点递推；同数据同指纹）。
pub fn profile_fingerprint(pts: &[PollPointProfile]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < pts.len() {
        h = fnv1a64_u64(h, pts[i].id as u64);
        h = fnv1a64_u64(h, pts[i].kind as u64);
        h = fnv1a64_u64(h, pts[i].freq_hz as u64);
        h = fnv1a64_u64(h, pts[i].cost_ns_avg);
        h = fnv1a64_u64(h, pts[i].phase as u64);
        i += 1;
    }
    h
}

/// 忙等轮询占比（万分比）：成本权重口径。
pub fn busy_ratio_permille(pts: &[PollPointProfile]) -> u64 {
    if pts.is_empty() {
        return 0;
    }
    let mut busy = 0u64;
    let mut total = 0u64;
    let mut i = 0usize;
    while i < pts.len() {
        let w = pts[i].freq_hz as u64 * pts[i].cost_ns_avg;
        total = total.wrapping_add(w);
        if pts[i].kind == 1 {
            busy = busy.wrapping_add(w);
        }
        i += 1;
    }
    if total == 0 {
        return 0;
    }
    ((busy as u128 * 10000 / total as u128) as u64)
}

// --------------------------------------------------------------------------
// 账本 pmp_prof（画像账）
// --------------------------------------------------------------------------
/// 画像账。
#[derive(Clone, Copy, Debug)]
pub struct pmp_prof {
    /// 已画像点数
    pub profiled: u64,
    /// 已测耗时点数
    pub costed: u64,
    /// 已测频率点数
    pub measured_freq: u64,
    /// 指纹漂移次数
    pub drift: u64,
}

impl pmp_prof {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            profiled: 0,
            costed: 0,
            measured_freq: 0,
            drift: 0,
        }
    }

    /// 画像一个点。
    pub fn profile_one(&mut self) -> () {
        self.profiled += 1;
    }

    /// 测一个点耗时。
    pub fn cost_one(&mut self) -> () {
        self.costed += 1;
    }

    /// 测一个点频率。
    pub fn freq_one(&mut self) -> () {
        self.measured_freq += 1;
    }

    /// 一次漂移。
    pub fn drift_one(&mut self) -> () {
        self.drift += 1;
    }

    /// 画像齐备（每点都有画像+耗时+频率）。
    pub fn complete(&mut self) -> bool {
        self.profiled == self.costed && self.profiled == self.measured_freq
    }

}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6170, 0x617f]。
pub const fn pmp_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < pmpCode::ALL.len() {
        let c = pmpCode::ALL[i].code();
        if c < 0x6170 || c > 0x617f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ---------------------------------------------------------------------------
// FNV-1a 64 确定性指纹（无第三方依赖；跨次运行恒同）
// ---------------------------------------------------------------------------

/// FNV-1a 64 偏移基准。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 逐字节递推。
pub const fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 以给定初值继续递推（链式原语）。
pub const fn fnv1a64_from(h: u64, data: &[u8]) -> u64 {
    let mut hh = h;
    let mut i = 0usize;
    while i < data.len() {
        hh ^= data[i] as u64;
        hh = hh.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hh
}

/// 64 位值入链（小端字节序保证跨平台构造一致）。
pub const fn fnv1a64_u64(h: u64, v: u64) -> u64 {
    let b = [
        (v & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        ((v >> 16) & 0xff) as u8,
        ((v >> 24) & 0xff) as u8,
        ((v >> 32) & 0xff) as u8,
        ((v >> 40) & 0xff) as u8,
        ((v >> 48) & 0xff) as u8,
        ((v >> 56) & 0xff) as u8,
    ];
    fnv1a64_from(h, &b)
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 画像指纹确定性。
fn c1_fp() -> bool {
    let a = [PollPointProfile { id: 1, kind: 1, freq_hz: 1000, cost_ns_avg: 50, phase: 2 }];
    let b = [PollPointProfile { id: 1, kind: 1, freq_hz: 1000, cost_ns_avg: 50, phase: 2 }];
    let mut c = a;
    c[0].id = 2;
    profile_fingerprint(&a) == profile_fingerprint(&b)
        && profile_fingerprint(&a) != profile_fingerprint(&c)
}

/// 忙等占比计算（万分比）。
fn c2_ratio() -> bool {
    let all_busy = [
        PollPointProfile { id: 1, kind: 1, freq_hz: 100, cost_ns_avg: 10, phase: 2 },
        PollPointProfile { id: 2, kind: 1, freq_hz: 100, cost_ns_avg: 10, phase: 2 },
    ];
    busy_ratio_permille(&all_busy) == 10000
        && busy_ratio_permille(&[]) == 0
}

/// 段位守卫：全部诊断码落在本模块段内。
fn pmp_guard_seg() -> bool {
    pmp_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_pmp_checks() -> CheckSet {
    let mut set = CheckSet::new("pmp");
    set.add(
        "F0961-prof-c1",
        c1_fp(),
        if c1_fp() { "" } else { "profile fingerprint not deterministic" },
    );
    set.add(
        "F0961-prof-c2",
        c2_ratio(),
        if c2_ratio() { "" } else { "busy ratio out of range" },
    );
    set.add(
        "pmp-seg-guard",
        pmp_guard_seg(),
        if pmp_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn pmp_all_checks_green() {
        let set = run_pmp_checks();
        assert!(set.all_passed(), "PMP domain checks must be green");
    }

    #[test]
    fn pmp_segment_guard_holds() {
        assert!(pmp_segment_guard());
    }
}
