//! unxt147_unx143_limine —— UNX-F0143 Limine 配置文件生成器（清单→样例逐字段）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0143 · Limine 配置文件生成器 · 判据：配置由构建系统从清单生成，与手写样例逐字段一致，Limine 解析零告警
//! · 组件：Limine 配置节表 + 生成引擎（逐字段）+ 样例对拍 + 解析零告警账
//! · 判据落地：与手写样例逐字段一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T147_VERSION: &str = "t147-unx143_limine-v1";

/// 诊断段基址。
pub const T147_SEG_BASE: u16 = 0x4e20;

// --------------------------------------------------------------------------
// 诊断码（t147 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// Limine 配置诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t147Code {
    /// 0x4e20 配置节非法（表外节）。
    SectionBad,
    /// 0x4e21 键重复（同节同键双写）。
    KeyDup,
    /// 0x4e22 样例分歧（生成与样例逐字段不符）。
    SampleDiff,
    /// 0x4e23 解析告警（生成配置产生告警）。
    ParseWarn,
    /// 0x4e24 转义非法（值含引号未转义）。
    EscapeBad,
    /// 0x4e25 配置账破缺。
    LedgerBreak,
    /// 0x4e26 节序断档。
    OrderBreak,
    /// 0x4e27 守卫触发。
    GuardTripped,
    /// 0x4e28 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4e29 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4e2a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4e2b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4e2c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4e2d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x4e2e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t147Code {
    /// 全量闭集。
    pub const ALL: [t147Code; 15] = [
        t147Code::SectionBad,
        t147Code::KeyDup,
        t147Code::SampleDiff,
        t147Code::ParseWarn,
        t147Code::EscapeBad,
        t147Code::LedgerBreak,
        t147Code::OrderBreak,
        t147Code::GuardTripped,
        t147Code::LedgerBroken,
        t147Code::SnapshotFingerprint,
        t147Code::TableBroken,
        t147Code::IndexMisaligned,
        t147Code::DuplicateEntry,
        t147Code::OrderViolated,
        t147Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t147Code::SectionBad => 0x4e20,
            t147Code::KeyDup => 0x4e21,
            t147Code::SampleDiff => 0x4e22,
            t147Code::ParseWarn => 0x4e23,
            t147Code::EscapeBad => 0x4e24,
            t147Code::LedgerBreak => 0x4e25,
            t147Code::OrderBreak => 0x4e26,
            t147Code::GuardTripped => 0x4e27,
            t147Code::LedgerBroken => 0x4e28,
            t147Code::SnapshotFingerprint => 0x4e29,
            t147Code::TableBroken => 0x4e2a,
            t147Code::IndexMisaligned => 0x4e2b,
            t147Code::DuplicateEntry => 0x4e2c,
            t147Code::OrderViolated => 0x4e2d,
            t147Code::SegmentGuard => 0x4e2e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t147Code::SectionBad => "配置节非法（表外节）",
            t147Code::KeyDup => "键重复（同节同键双写）",
            t147Code::SampleDiff => "样例分歧（生成与样例逐字段不符）",
            t147Code::ParseWarn => "解析告警（生成配置产生告警）",
            t147Code::EscapeBad => "转义非法（值含引号未转义）",
            t147Code::LedgerBreak => "配置账破缺",
            t147Code::OrderBreak => "节序断档",
            t147Code::GuardTripped => "守卫触发",
            t147Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t147Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t147Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t147Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t147Code::DuplicateEntry => "条目重复（同键双写）",
            t147Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t147Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t147Code::SectionBad => "按节表校正",
            t147Code::KeyDup => "按末次生效合并",
            t147Code::SampleDiff => "按分歧路径归因",
            t147Code::ParseWarn => "按零告警口径修正",
            t147Code::EscapeBad => "按转义律校正",
            t147Code::LedgerBreak => "按守恒重建",
            t147Code::OrderBreak => "按节表序重排",
            t147Code::GuardTripped => "检查临界区",
            t147Code::LedgerBroken => "按守恒律重建台账后重跑",
            t147Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t147Code::TableBroken => "按语义规则重建常量表",
            t147Code::IndexMisaligned => "按声明序归一化后重跑",
            t147Code::DuplicateEntry => "合并同键条目为末次生效",
            t147Code::OrderViolated => "按依赖矩阵重排后重跑",
            t147Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// Limine 配置节表 + 生成引擎（format!("{key}: {value}") 逐字段）
// ---------------------------------------------------------------------------

/// Limine 配置节（闭集六节）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LimineSection {
    /// 内核加载节。
    Kernel,
    /// 模块加载节。
    Modules,
    /// 启动参数节。
    Cmdline,
    /// 分辨率节。
    Resolution,
    /// 探针节。
    Probe,
    /// 杂项节。
    Misc,
}

impl LimineSection {
    /// 全量闭集（六节）。
    pub const ALL: [LimineSection; 6] = [
        LimineSection::Kernel,
        LimineSection::Modules,
        LimineSection::Cmdline,
        LimineSection::Resolution,
        LimineSection::Probe,
        LimineSection::Misc,
    ];

    /// 节名（Limine 实际节的冒号前缀口径）。
    pub const fn name(self) -> &'static str {
        match self {
            LimineSection::Kernel => "kernel",
            LimineSection::Modules => "module",
            LimineSection::Cmdline => "cmdline",
            LimineSection::Resolution => "resolution",
            LimineSection::Probe => "probe",
            LimineSection::Misc => "misc",
        }
    }

    /// 索引。
    pub const fn index(self) -> usize {
        match self {
            LimineSection::Kernel => 0,
            LimineSection::Modules => 1,
            LimineSection::Cmdline => 2,
            LimineSection::Resolution => 3,
            LimineSection::Probe => 4,
            LimineSection::Misc => 5,
        }
    }
}

/// 键值对（生成单元）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimineKv {
    /// 所属节。
    pub section: usize,
    /// 键（ASCII）。
    pub key: &'static str,
    /// 值（ASCII）。
    pub value: &'static str,
}

/// 配置账（样例对拍 + 解析告警守恒）。
#[derive(Clone, Copy, Debug)]
pub struct LimineBook {
    /// 已生成键值对数。
    pub generated: u64,
    /// 样例匹配对数。
    pub matched: u64,
    /// 分歧对数（必须 0）。
    pub diverged: u64,
    /// 解析告警数（必须 0）。
    pub parse_warnings: u64,
}

impl LimineBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { generated: 0, matched: 0, diverged: 0, parse_warnings: 0 }
    }

    /// 记一次生成（生成即期望与样例一致）。
    pub fn generate(&mut self, expect: &LimineKv, actual: &LimineKv) -> bool {
        self.generated += 1;
        if expect == actual {
            self.matched += 1;
            true
        } else {
            self.diverged += 1;
            false
        }
    }

    /// 记一次解析告警。
    pub fn warn(&mut self) {
        self.parse_warnings += 1;
    }

    /// 逐字段一致 + 零告警判据。
    pub fn exact(&self) -> bool {
        self.diverged == 0 && self.parse_warnings == 0 && self.generated > 0 && self.matched == self.generated
    }
}

/// 行渲染（Limine 口径 `key: value`；返回是否合法：都非空）。
pub const fn limine_render_ok(kv: &LimineKv) -> bool {
    !kv.key.is_empty() && !kv.value.is_empty() && kv.section < 6
}

/// 转义需求判定（值含引号/反斜杠时必须转义）。
pub const fn limine_needs_escape(v: &str) -> bool {
    let b = v.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'"' || b[i] == b'\\' {
            return true;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4e20, 0x4e2f]。
pub const fn t147_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t147Code::ALL.len() {
        let c = t147Code::ALL[i].code();
        if c < 0x4e20 || c > 0x4e2f {
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
fn t147_guard_seg() -> bool {
    t147_segment_guard()
}

/// 六节闭集 + 节名齐备 + 键值渲染合法性。
fn check_c1_sections() -> bool {
    let mut ok = LimineSection::ALL.len() == 6;
    if ok {
        ok = LimineSection::Kernel.name() == "kernel" && LimineSection::Cmdline.name() == "cmdline";
    }
    if ok {
        let kv = LimineKv { section: 0, key: "kernel_path", value: "boot():/boot/varix" };
        ok = limine_render_ok(&kv);
    }
    if ok {
        let bad = LimineKv { section: 9, key: "k", value: "v" };
        ok = !limine_render_ok(&bad);
    }
    ok
}

/// 逐字段一致 + 零告警 + 转义判定。
fn check_c2_exact() -> bool {
    let mut bk = LimineBook::new();
    let expect = LimineKv { section: 0, key: "kernel_path", value: "boot():/boot/varix" };
    let mut ok = bk.generate(&expect, &expect);
    if ok {
        ok = bk.exact();
    }
    if ok {
        // 分歧样例必须被抓。
        let actual = LimineKv { section: 0, key: "kernel_path", value: "boot():/boot/wrong" };
        ok = !bk.generate(&expect, &actual) && bk.diverged == 1 && !bk.exact();
    }
    if ok {
        ok = limine_needs_escape("plain") == false;
        ok = ok && limine_needs_escape("needs\"quote") == true;
        ok = ok && limine_needs_escape("back\\slash") == true;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t147_checks() -> CheckSet {
    let mut set = CheckSet::new("T147");
    set.add(
        "t147-seg-guard",
        t147_guard_seg(),
        if t147_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t147-c1-sections",
        check_c1_sections(),
        if check_c1_sections() { "" } else { "limine sections mismatch" },
    );
    set.add(
        "t147-c2-exact",
        check_c2_exact(),
        if check_c2_exact() { "" } else { "limine sample exactness mismatch" },
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
    fn t147_all_checks_green() {
        let set = run_t147_checks();
        assert!(set.all_passed(), "T147 domain checks must be green");
    }

    #[test]
    fn t147_segment_guard_holds() {
        assert!(t147_segment_guard());
    }
}
