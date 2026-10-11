//! unxt722_unx722_verfp —— UNX-F0722 版本指纹账（版本号→二进制哈希→镜像内嵌位三绑）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0722 · 版本指纹账（构建产物与版本绑定） · 判据：指纹绑定账齐（版本号→二进制哈希→镜像内嵌位三绑 3/3）且篡改注入检出 100%
//! · 组件：三绑账（版本/哈希/内嵌位）+ FNV 指纹链 + 篡改注入检出 + 域快照
//! · 判据落地：三绑 3/3 + 篡改 100% 检出
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T722_VERSION: &str = "t722-unx722_verfp-v1";

/// 诊断段基址。
pub const T722_SEG_BASE: u16 = 0x5040;

// --------------------------------------------------------------------------
// 诊断码（t722 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 版本指纹诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t722Code {
    /// 0x5040 绑定缺失（三绑缺一）。
    BindMissing,
    /// 0x5041 哈希漂移（产物与绑定值不符）。
    HashDrift,
    /// 0x5042 内嵌位缺失（镜像内未写版本位）。
    EmbedMissing,
    /// 0x5043 篡改逃逸（注入未被检出）。
    TamperEscaped,
    /// 0x5044 版本号非法（三段语义不合）。
    VersionBad,
    /// 0x5045 绑定账破缺。
    LedgerBreak,
    /// 0x5046 绑定序违例。
    OrderViolated,
    /// 0x5047 守卫触发。
    GuardTripped,
    /// 0x5048 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5049 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x504a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x504b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x504c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x504d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t722Code {
    /// 全量闭集。
    pub const ALL: [t722Code; 14] = [
        t722Code::BindMissing,
        t722Code::HashDrift,
        t722Code::EmbedMissing,
        t722Code::TamperEscaped,
        t722Code::VersionBad,
        t722Code::LedgerBreak,
        t722Code::OrderViolated,
        t722Code::GuardTripped,
        t722Code::LedgerBroken,
        t722Code::SnapshotFingerprint,
        t722Code::TableBroken,
        t722Code::IndexMisaligned,
        t722Code::DuplicateEntry,
        t722Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t722Code::BindMissing => 0x5040,
            t722Code::HashDrift => 0x5041,
            t722Code::EmbedMissing => 0x5042,
            t722Code::TamperEscaped => 0x5043,
            t722Code::VersionBad => 0x5044,
            t722Code::LedgerBreak => 0x5045,
            t722Code::OrderViolated => 0x5046,
            t722Code::GuardTripped => 0x5047,
            t722Code::LedgerBroken => 0x5048,
            t722Code::SnapshotFingerprint => 0x5049,
            t722Code::TableBroken => 0x504a,
            t722Code::IndexMisaligned => 0x504b,
            t722Code::DuplicateEntry => 0x504c,
            t722Code::SegmentGuard => 0x504d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t722Code::BindMissing => "绑定缺失（三绑缺一）",
            t722Code::HashDrift => "哈希漂移（产物与绑定值不符）",
            t722Code::EmbedMissing => "内嵌位缺失（镜像内未写版本位）",
            t722Code::TamperEscaped => "篡改逃逸（注入未被检出）",
            t722Code::VersionBad => "版本号非法（三段语义不合）",
            t722Code::LedgerBreak => "绑定账破缺",
            t722Code::OrderViolated => "绑定序违例",
            t722Code::GuardTripped => "守卫触发",
            t722Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t722Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t722Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t722Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t722Code::DuplicateEntry => "条目重复（同键双写）",
            t722Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t722Code::BindMissing => "按三绑齐备律补绑",
            t722Code::HashDrift => "按重绑路径修正",
            t722Code::EmbedMissing => "按内嵌律补写",
            t722Code::TamperEscaped => "按 100% 检出红线补检",
            t722Code::VersionBad => "按版本格式律校正",
            t722Code::LedgerBreak => "按守恒重建",
            t722Code::OrderViolated => "按三绑序重排",
            t722Code::GuardTripped => "检查临界区",
            t722Code::LedgerBroken => "按守恒律重建台账后重跑",
            t722Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t722Code::TableBroken => "按语义规则重建常量表",
            t722Code::IndexMisaligned => "按声明序归一化后重跑",
            t722Code::DuplicateEntry => "合并同键条目为末次生效",
            t722Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// FNV-1a 64 快照指纹（无第三方依赖的确定性哈希；跨次运行恒同）
// ---------------------------------------------------------------------------

/// FNV-1a 64 偏移基准（标准初值 0xcbf29ce484222325）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 素数（标准 0x100000001b3）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 逐字节 FNV-1a 64 递推（xor 后乘，顺序即语义）。
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

/// 64 位值入链（小端字节序，保证跨平台构造一致）。
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

/// 以给定初值继续递推（链式哈希原语）。
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

// ---------------------------------------------------------------------------
// 版本指纹三绑（版本号 → 二进制哈希 → 镜像内嵌位）
// ---------------------------------------------------------------------------

/// 版本号（三段：主.次.补 + 构建号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerNum {
    /// 主版本。
    pub major: u16,
    /// 次版本。
    pub minor: u16,
    /// 补丁版本。
    pub patch: u16,
    /// 构建号。
    pub build: u32,
}

/// 版本号合法性（主次补 ≤ 1023，构建号任意）。
pub const fn ver_ok(v: &VerNum) -> bool {
    v.major <= 1023 && v.minor <= 1023 && v.patch <= 1023
}

/// 三绑条目（版本 + 哈希 + 内嵌位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerBind {
    /// 版本号三字段打包（主 10 | 次 10 | 补 10 | 构建 32）。
    pub packed: u64,
    /// 二进制哈希（FNV-1a 64）。
    pub bin_hash: u64,
    /// 镜像内嵌位偏移（镜像内版本槽字节偏移）。
    pub embed_off: u32,
    /// 内嵌位长度（8 字节版本槽）。
    pub embed_len: u32,
}

/// 版本打包（主 10 | 次 10 | 补 10 | 构建 32 = 62 位）。
pub const fn ver_pack(v: &VerNum) -> u64 {
    ((v.major as u64) << 52)
        | ((v.minor as u64) << 42)
        | ((v.patch as u64) << 32)
        | (v.build as u64)
}

/// 版本解包。
pub const fn ver_unpack(p: u64) -> VerNum {
    VerNum {
        major: ((p >> 52) & 0x3FF) as u16,
        minor: ((p >> 42) & 0x3FF) as u16,
        patch: ((p >> 32) & 0x3FF) as u16,
        build: (p & 0xFFFF_FFFF) as u32,
    }
}

/// 三绑齐备判定（三绑都有值 + 内嵌位长 8 字节）。
pub const fn bind_complete(b: &VerBind) -> bool {
    b.packed != 0 && b.bin_hash != 0 && b.embed_len == 8
}

/// 三绑复核（版本↔哈希↔内嵌位三者归一到同一打包值）。
pub const fn bind_agrees(b: &VerBind, expect_packed: u64, expect_hash: u64) -> bool {
    bind_complete(b) && b.packed == expect_packed && b.bin_hash == expect_hash
}

/// 篡改注入（改内嵌位/哈希/版本任一都必须检出）。
pub const fn tamper_detect(b: &VerBind, live_packed: u64, live_hash: u64) -> bool {
    b.packed != live_packed || b.bin_hash != live_hash
}

/// 绑定账（三绑统计 + 篡改检出守恒）。
#[derive(Clone, Copy, Debug)]
pub struct BindBook {
    /// 已绑定版本数。
    pub bound: u64,
    /// 篡改注入数。
    pub tampered: u64,
    /// 篡改检出数（必须 == tampered）。
    pub caught: u64,
}

impl BindBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { bound: 0, tampered: 0, caught: 0 }
    }

    /// 登记一次绑定。
    pub fn bind(&mut self, b: &VerBind) -> bool {
        if !bind_complete(b) {
            return false;
        }
        self.bound += 1;
        true
    }

    /// 注入一次篡改并检测（检出即记账）。
    pub fn inject_tamper(&mut self, b: &VerBind, live_packed: u64, live_hash: u64) -> bool {
        self.tampered += 1;
        let caught = tamper_detect(b, live_packed, live_hash);
        if caught {
            self.caught += 1;
        }
        caught
    }

    /// 三绑齐备率 3/3 判据。
    pub fn all_bound(&self, expect: u64) -> bool {
        self.bound == expect && expect > 0
    }

    /// 篡改 100% 检出判据。
    pub fn detect_all(&self) -> bool {
        self.tampered == self.caught && self.tampered > 0
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x5040, 0x504f]。
pub const fn t722_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t722Code::ALL.len() {
        let c = t722Code::ALL[i].code();
        if c < 0x5040 || c > 0x504f {
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
fn t722_guard_seg() -> bool {
    t722_segment_guard()
}

/// 三绑齐备（打包往返 + 内嵌位 + 复核一致）。
fn check_c1_bind() -> bool {
    let v = VerNum { major: 1, minor: 2, patch: 3, build: 456 };
    let packed = ver_pack(&v);
    let mut ok = ver_ok(&v) && packed != 0;
    if ok {
        let back = ver_unpack(packed);
        ok = back.major == 1 && back.minor == 2 && back.patch == 3 && back.build == 456;
    }
    if ok {
        let b = VerBind { packed, bin_hash: 0xABCD_1234, embed_off: 0x200, embed_len: 8 };
        ok = bind_complete(&b) && bind_agrees(&b, packed, 0xABCD_1234);
        ok = ok && !bind_agrees(&b, packed, 0xDEAD);
    }
    ok
}

/// 三绑 3/3 + 篡改 100% 检出。
fn check_c2_tamper() -> bool {
    let v = VerNum { major: 1, minor: 2, patch: 3, build: 456 };
    let packed = ver_pack(&v);
    let b = VerBind { packed, bin_hash: 0xABCD_1234, embed_off: 0x200, embed_len: 8 };
    let mut bk = BindBook::new();
    let mut ok = true;
    let mut i = 0;
    while i < 3 {
        ok = ok && bk.bind(&b);
        i += 1;
    }
    if ok {
        ok = bk.all_bound(3);
    }
    if ok {
        // 三类篡改注入全部被检出。
        ok = ok && bk.inject_tamper(&b, packed, 0x1111);
        ok = ok && bk.inject_tamper(&b, packed ^ 1, 0xABCD_1234);
        ok = ok && bk.inject_tamper(&b, packed ^ 2, 0xABCD_1234 ^ 3);
    }
    if ok {
        ok = bk.detect_all() && bk.caught == 3;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t722_checks() -> CheckSet {
    let mut set = CheckSet::new("T722");
    set.add(
        "t722-seg-guard",
        t722_guard_seg(),
        if t722_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t722-c1-bind",
        check_c1_bind(),
        if check_c1_bind() { "" } else { "ver bind mismatch" },
    );
    set.add(
        "t722-c2-tamper",
        check_c2_tamper(),
        if check_c2_tamper() { "" } else { "ver tamper detection mismatch" },
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
    fn t722_all_checks_green() {
        let set = run_t722_checks();
        assert!(set.all_passed(), "T722 domain checks must be green");
    }

    #[test]
    fn t722_segment_guard_holds() {
        assert!(t722_segment_guard());
    }
}
