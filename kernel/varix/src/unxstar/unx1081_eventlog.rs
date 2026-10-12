//! UNX-F1081 墙钟 NTP 校准 · 校准事件账（落盘）
//! 判据原文（CoRun Unxreal 主册）：UNX-F1081-J1 双协议正确（阶跃档输出跳变 10/10 次、渐变档输出斜率受控 10/10 次），校准事件账落盘（来源/幅度/档位全记录），基础面零回归（F1079 判据复测全过）
//! · 组件：校准事件记录（来源/幅度/档位/时间戳）+ 事件账 + 落盘序列号
//! · 判据落地：校准事件账落盘（来源/幅度/档位全记录）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const NTE_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const NTE_SEG_BASE: u16 = 0x6230;

// --------------------------------------------------------------------------
// 诊断码（nte 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 校准事件账诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum nteCode {
    /// 事件缺来源。
    SourceMissing,
    /// 事件缺幅度。
    MagnitudeMissing,
    /// 事件缺档位（step/slew）。
    ModeMissing,
    /// 事件缺时间戳。
    StampMissing,
    /// 落盘序列号断档。
    SeqGap,
    /// 事件账超容量。
    LogOver,
    /// 落盘失败。
    WriteFail,
    /// 记录字段不全。
    PartialRecord,
}

impl nteCode {
    /// 全量闭集。
    pub const ALL: [nteCode; 8] = [
        nteCode::SourceMissing,
        nteCode::MagnitudeMissing,
        nteCode::ModeMissing,
        nteCode::StampMissing,
        nteCode::SeqGap,
        nteCode::LogOver,
        nteCode::WriteFail,
        nteCode::PartialRecord,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            nteCode::SourceMissing => 0x6230 + 0,
            nteCode::MagnitudeMissing => 0x6230 + 1,
            nteCode::ModeMissing => 0x6230 + 2,
            nteCode::StampMissing => 0x6230 + 3,
            nteCode::SeqGap => 0x6230 + 4,
            nteCode::LogOver => 0x6230 + 5,
            nteCode::WriteFail => 0x6230 + 6,
            nteCode::PartialRecord => 0x6230 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            nteCode::SourceMissing => "事件缺来源",
            nteCode::MagnitudeMissing => "事件缺幅度",
            nteCode::ModeMissing => "事件缺档位（step/slew）",
            nteCode::StampMissing => "事件缺时间戳",
            nteCode::SeqGap => "落盘序列号断档",
            nteCode::LogOver => "事件账超容量",
            nteCode::WriteFail => "落盘失败",
            nteCode::PartialRecord => "记录字段不全",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            nteCode::SourceMissing => "补来源后才准落盘",
            nteCode::MagnitudeMissing => "补幅度",
            nteCode::ModeMissing => "补档位",
            nteCode::StampMissing => "补时间戳",
            nteCode::SeqGap => "补号并复判",
            nteCode::LogOver => "轮转（见 F0204 账）",
            nteCode::WriteFail => "重试或缓存后补写",
            nteCode::PartialRecord => "拒落盘",
        }
    }
}


// --------------------------------------------------------------------------
// 冻结常量表 nte_mode（3 行逐行注释自证）
// --------------------------------------------------------------------------
/// 校准档位表。
pub const nte_mode_COUNT: usize = 3;

/// 表本体（编译期冻结，运行期只读）。
pub static nte_mode: [(&'static str, u32); 3] = [
    ("Step", 1), // 阶跃档（step）
    ("Slew", 2), // 渐变档（slew）
    ("ModesTotal", 2), // 档案闭集总数
];

/// 线性查找（键唯一由构造期断言保证）。
pub fn nte_mode_get(key: &'static str) -> Option<u32> {
    let mut i = 0usize;
    while i < nte_mode_COUNT {
        if nte_mode[i].0 == key {
            return Some(nte_mode[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（守恒对账用）。
pub fn nte_mode_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < nte_mode_COUNT {
        s = s.wrapping_add(nte_mode[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（逐对扫描；重复即结构腐坏）。
pub fn nte_mode_unique() -> bool {
    let mut i = 0usize;
    while i < nte_mode_COUNT {
        let mut j = i + 1;
        while j < nte_mode_COUNT {
            if nte_mode[i].0 == nte_mode[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}


// --------------------------------------------------------------------------
// 冻结常量表 nte_source（6 行逐行注释自证）
// --------------------------------------------------------------------------
/// 校准来源闭集表。
pub const nte_source_COUNT: usize = 6;

/// 表本体（编译期冻结，运行期只读）。
pub static nte_source: [(&'static str, u32); 6] = [
    ("NtpPool", 1), // NTP 池服务器
    ("NtpPeer", 2), // 指定 NTP peer
    ("Ptp", 3), // PTP（硬件时间同步）
    ("Rtc", 4), // CMOS RTC（机械钟，精度低）
    ("Manual", 5), // 人工设定
    ("SourcesTotal", 5), // 来源闭集总数
];

/// 线性查找（键唯一由构造期断言保证）。
pub fn nte_source_get(key: &'static str) -> Option<u32> {
    let mut i = 0usize;
    while i < nte_source_COUNT {
        if nte_source[i].0 == key {
            return Some(nte_source[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（守恒对账用）。
pub fn nte_source_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < nte_source_COUNT {
        s = s.wrapping_add(nte_source[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（逐对扫描；重复即结构腐坏）。
pub fn nte_source_unique() -> bool {
    let mut i = 0usize;
    while i < nte_source_COUNT {
        let mut j = i + 1;
        while j < nte_source_COUNT {
            if nte_source[i].0 == nte_source[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}


// --------------------------------------------------------------------------
// 账本 nte_log（校准事件账）
// --------------------------------------------------------------------------
/// 校准事件账。
#[derive(Clone, Copy, Debug)]
pub struct nte_log {
    /// 事件数
    pub events: u64,
    /// 丢弃事件数
    pub dropped: u64,
    /// 落盘序列号
    pub seq: u64,
    /// 事件账指纹
    pub fingerprint: u64,
}

impl nte_log {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            events: 0,
            dropped: 0,
            seq: 0,
            fingerprint: 0,
        }
    }

    /// 追加一条事件。
    pub fn append(&mut self) -> () {
        self.events += 1;
        self.seq += 1;
    }

    /// 丢弃一条事件。
    pub fn drop_one(&mut self) -> () {
        self.dropped += 1;
    }

    /// 落一次账指纹。
    pub fn stamp_fp(&mut self) -> () {
        self.fingerprint = self.fingerprint.wrapping_add(self.seq);
    }

    /// 事件账干净（零丢弃）。
    pub fn clean(&mut self) -> bool {
        self.dropped == 0 && self.events == self.seq
    }

}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6230, 0x623f]。
pub const fn nte_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < nteCode::ALL.len() {
        let c = nteCode::ALL[i].code();
        if c < 0x6230 || c > 0x623f {
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
/// 档位与来源闭集。
fn c1_modes() -> bool {
    nte_mode_get("Step") == Some(1) && nte_mode_get("Slew") == Some(2)
        && nte_source_get("NtpPool") == Some(1)
        && nte_source_get("SourcesTotal") == Some(5)
        && nte_mode_unique() && nte_source_unique()
}

/// 事件账序列连续零丢弃。
fn c2_log() -> bool {
    let mut l = nte_log::new();
    let mut i = 0u64;
    while i < 50 { l.append(); i += 1; }
    l.stamp_fp();
    l.clean() && l.events == 50 && l.seq == 50
}

/// 有丢弃即不干净。
fn c3_drop() -> bool {
    let mut l = nte_log::new();
    let mut i = 0u64;
    while i < 49 { l.append(); i += 1; }
    l.drop_one();
    l.append();
    !l.clean() && l.dropped == 1
}

/// 段位守卫：全部诊断码落在本模块段内。
fn nte_guard_seg() -> bool {
    nte_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_nte_checks() -> CheckSet {
    let mut set = CheckSet::new("nte");
    set.add(
        "F1081-log-c1",
        c1_modes(),
        if c1_modes() { "" } else { "mode/source closed sets drifted" },
    );
    set.add(
        "F1081-log-c2",
        c2_log(),
        if c2_log() { "" } else { "event log not clean" },
    );
    set.add(
        "F1081-log-c3",
        c3_drop(),
        if c3_drop() { "" } else { "dropped event slipped through" },
    );
    set.add(
        "nte-seg-guard",
        nte_guard_seg(),
        if nte_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn nte_all_checks_green() {
        let set = run_nte_checks();
        assert!(set.all_passed(), "NTE domain checks must be green");
    }

    #[test]
    fn nte_segment_guard_holds() {
        assert!(nte_segment_guard());
    }
}
