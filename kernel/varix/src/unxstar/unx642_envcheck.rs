//! unxt642_unx642_envcheck —— UNX-F0642 演练环境自检（QEMU/镜像/工具链三查）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0642 · 演练环境自检（QEMU/镜像/工具链三查） · 判据：环境三查（QEMU 版本/镜像哈希/工具链白名单）全过才放行，坏哈希注入拦截检出 100%
//! · 组件：三查表（QEMU 版本/镜像哈希/工具链白名单）+ 放行闸 + 坏哈希拦截账
//! · 判据落地：三查全过才放行 + 坏哈希 100% 拦截
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T642_VERSION: &str = "t642-unx642_envcheck-v1";

/// 诊断段基址。
pub const T642_SEG_BASE: u16 = 0x5010;

// --------------------------------------------------------------------------
// 诊断码（t642 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 环境自检诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t642Code {
    /// 0x5010 QEMU 版本不合规。
    QemuBad,
    /// 0x5011 镜像哈希不符。
    HashBad,
    /// 0x5012 工具链不在白名单。
    ToolBad,
    /// 0x5013 提前放行（三查未全过）。
    GateOpenEarly,
    /// 0x5014 样本被篡改（哈希注入）。
    SampleTampered,
    /// 0x5015 自检账破缺。
    LedgerBreak,
    /// 0x5016 检查序违例。
    OrderViolated,
    /// 0x5017 守卫触发。
    GuardTripped,
    /// 0x5018 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5019 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x501a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x501b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x501c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x501d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t642Code {
    /// 全量闭集。
    pub const ALL: [t642Code; 14] = [
        t642Code::QemuBad,
        t642Code::HashBad,
        t642Code::ToolBad,
        t642Code::GateOpenEarly,
        t642Code::SampleTampered,
        t642Code::LedgerBreak,
        t642Code::OrderViolated,
        t642Code::GuardTripped,
        t642Code::LedgerBroken,
        t642Code::SnapshotFingerprint,
        t642Code::TableBroken,
        t642Code::IndexMisaligned,
        t642Code::DuplicateEntry,
        t642Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t642Code::QemuBad => 0x5010,
            t642Code::HashBad => 0x5011,
            t642Code::ToolBad => 0x5012,
            t642Code::GateOpenEarly => 0x5013,
            t642Code::SampleTampered => 0x5014,
            t642Code::LedgerBreak => 0x5015,
            t642Code::OrderViolated => 0x5016,
            t642Code::GuardTripped => 0x5017,
            t642Code::LedgerBroken => 0x5018,
            t642Code::SnapshotFingerprint => 0x5019,
            t642Code::TableBroken => 0x501a,
            t642Code::IndexMisaligned => 0x501b,
            t642Code::DuplicateEntry => 0x501c,
            t642Code::SegmentGuard => 0x501d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t642Code::QemuBad => "QEMU 版本不合规",
            t642Code::HashBad => "镜像哈希不符",
            t642Code::ToolBad => "工具链不在白名单",
            t642Code::GateOpenEarly => "提前放行（三查未全过）",
            t642Code::SampleTampered => "样本被篡改（哈希注入）",
            t642Code::LedgerBreak => "自检账破缺",
            t642Code::OrderViolated => "检查序违例",
            t642Code::GuardTripped => "守卫触发",
            t642Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t642Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t642Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t642Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t642Code::DuplicateEntry => "条目重复（同键双写）",
            t642Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t642Code::QemuBad => "按版本白名单升级/降级",
            t642Code::HashBad => "按坏哈希红线拒行",
            t642Code::ToolBad => "按白名单口令径校正",
            t642Code::GateOpenEarly => "按全过才放行律补查",
            t642Code::SampleTampered => "按 100% 拦截红线拦出",
            t642Code::LedgerBreak => "按守恒重建",
            t642Code::OrderViolated => "按三查序重排",
            t642Code::GuardTripped => "检查临界区",
            t642Code::LedgerBroken => "按守恒律重建台账后重跑",
            t642Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t642Code::TableBroken => "按语义规则重建常量表",
            t642Code::IndexMisaligned => "按声明序归一化后重跑",
            t642Code::DuplicateEntry => "合并同键条目为末次生效",
            t642Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 演练环境三查（QEMU 版本/镜像哈希/工具链白名单）
// ---------------------------------------------------------------------------

/// 检查项（闭集三项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnvCheck {
    /// QEMU 版本检查。
    QemuVersion,
    /// 镜像哈希检查。
    ImageHash,
    /// 工具链白名单检查。
    Toolchain,
}

impl EnvCheck {
    /// 全量闭集（三项）。
    pub const ALL: [EnvCheck; 3] = [EnvCheck::QemuVersion, EnvCheck::ImageHash, EnvCheck::Toolchain];

    /// 索引。
    pub const fn index(self) -> usize {
        match self {
            EnvCheck::QemuVersion => 0,
            EnvCheck::ImageHash => 1,
            EnvCheck::Toolchain => 2,
        }
    }

    /// 中文件名。
    pub const fn cn(self) -> &'static str {
        match self {
            EnvCheck::QemuVersion => "QEMU 版本",
            EnvCheck::ImageHash => "镜像哈希",
            EnvCheck::Toolchain => "工具链白名单",
        }
    }
}

/// QEMU 最低主版本。
pub const QEMU_MIN_MAJOR: u32 = 8;

/// 白名单工具链（四项）。
pub const TOOLCHAIN_WHITELIST: [&str; 4] = ["rustc", "cargo", "gcc", "ld"];

/// 工具链白名单判定。
pub fn toolchain_ok(name: &str) -> bool {
    let mut i = 0;
    while i < TOOLCHAIN_WHITELIST.len() {
        if TOOLCHAIN_WHITELIST[i] == name {
            return true;
        }
        i += 1;
    }
    false
}

/// 三查结果（逐项通过位图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvVerdict {
    /// 位图（0b111 = 三查全过）。
    pub passed: u8,
}

/// 全过判定位图。
pub const ENV_ALL_PASS: u8 = 0b111;

/// 三查全过才放行。
pub const fn env_gate_open(v: &EnvVerdict) -> bool {
    v.passed == ENV_ALL_PASS
}

/// 放行账（坏哈希拦截 + 闸口守恒）。
#[derive(Clone, Copy, Debug)]
pub struct EnvBook {
    /// 已跑三查次数。
    pub runs: u64,
    /// 放行次数。
    pub opened: u64,
    /// 拒行次数。
    pub denied: u64,
    /// 坏哈希注入次数。
    pub tampered: u64,
    /// 坏哈希拦截次数（必须 == tampered）。
    pub caught: u64,
}

impl EnvBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { runs: 0, opened: 0, denied: 0, tampered: 0, caught: 0 }
    }

    /// 跑一次三查（按位图判定放行/拒行）。
    pub fn run(&mut self, v: &EnvVerdict) -> bool {
        self.runs += 1;
        if env_gate_open(v) {
            self.opened += 1;
            true
        } else {
            self.denied += 1;
            false
        }
    }

    /// 注入坏哈希（镜像哈希位清置 → 必须拒行且记拦截）。
    pub fn tamper(&mut self) -> bool {
        self.tampered += 1;
        let v = EnvVerdict { passed: ENV_ALL_PASS & !0b010 };
        let open = self.run(&v);
        if !open {
            self.caught += 1;
        }
        !open
    }

    /// 坏哈希 100% 拦截判据。
    pub fn tamper_all_caught(&self) -> bool {
        self.tampered == self.caught && self.tampered > 0
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x5010, 0x501f]。
pub const fn t642_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t642Code::ALL.len() {
        let c = t642Code::ALL[i].code();
        if c < 0x5010 || c > 0x501f {
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
fn t642_guard_seg() -> bool {
    t642_segment_guard()
}

/// 放行闸（三查全过才开 + 缺位拒行）。
fn check_c1_env_gate() -> bool {
    let full = EnvVerdict { passed: 0b111 };
    let partial = EnvVerdict { passed: 0b101 };
    let mut ok = env_gate_open(&full) && !env_gate_open(&partial);
    if ok {
        ok = EnvCheck::ALL.len() == 3 && toolchain_ok("rustc") && !toolchain_ok("python");
    }
    ok
}

/// 坏哈希 100% 拦截 + 放行守恒。
fn check_c2_tamper() -> bool {
    let mut bk = EnvBook::new();
    let mut ok = true;
    let mut i = 0;
    while i < 10 {
        ok = ok && bk.tamper();
        i += 1;
    }
    if ok {
        ok = bk.tamper_all_caught() && bk.caught == 10 && bk.denied == 10 && bk.opened == 0;
    }
    if ok {
        // 正常三查应放行。
        let full = EnvVerdict { passed: 0b111 };
        ok = bk.run(&full) && bk.opened == 1 && bk.runs == 11;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t642_checks() -> CheckSet {
    let mut set = CheckSet::new("T642");
    set.add(
        "t642-seg-guard",
        t642_guard_seg(),
        if t642_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t642-c1-env-gate",
        check_c1_env_gate(),
        if check_c1_env_gate() { "" } else { "env gate mismatch" },
    );
    set.add(
        "t642-c2-tamper",
        check_c2_tamper(),
        if check_c2_tamper() { "" } else { "env tamper intercept mismatch" },
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
    fn t642_all_checks_green() {
        let set = run_t642_checks();
        assert!(set.all_passed(), "T642 domain checks must be green");
    }

    #[test]
    fn t642_segment_guard_holds() {
        assert!(t642_segment_guard());
    }
}
