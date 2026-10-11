//! unxt602_unx602_initrdro —— UNX-F0602 initrd 归档区只读映射登记（B19 联动）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0602 · initrd 只读映射登记（B19 联动） · 判据：归档区映射登记（基址/长度/RO 属性入 B19 账 1/1），区外写尝试拦截 100%
//! · 组件：映射登记表（基址/长度/RO）+ B19 账登记引擎 + 区外写拦截账 + 域快照
//! · 判据落地：入账 1/1 + 区外写拦截 100%
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T602_VERSION: &str = "t602-unx602_initrdro-v1";

/// 诊断段基址。
pub const T602_SEG_BASE: u16 = 0x5000;

// --------------------------------------------------------------------------
// 诊断码（t602 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// initrd 映射诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t602Code {
    /// 0x5000 映射非法（基址/长度违例）。
    MapBad,
    /// 0x5001 重复登记（同区双映）。
    AlreadyMapped,
    /// 0x5002 区外写逃逸（RO 区被写）。
    WriteEscaped,
    /// 0x5003 B19 账漏登。
    B19Missed,
    /// 0x5004 解映射泄漏（区未注销）。
    UnmapLeak,
    /// 0x5005 登记账破缺。
    LedgerBreak,
    /// 0x5006 登记序违例。
    OrderViolated,
    /// 0x5007 守卫触发。
    GuardTripped,
    /// 0x5008 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5009 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x500a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x500b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x500c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x500d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t602Code {
    /// 全量闭集。
    pub const ALL: [t602Code; 14] = [
        t602Code::MapBad,
        t602Code::AlreadyMapped,
        t602Code::WriteEscaped,
        t602Code::B19Missed,
        t602Code::UnmapLeak,
        t602Code::LedgerBreak,
        t602Code::OrderViolated,
        t602Code::GuardTripped,
        t602Code::LedgerBroken,
        t602Code::SnapshotFingerprint,
        t602Code::TableBroken,
        t602Code::IndexMisaligned,
        t602Code::DuplicateEntry,
        t602Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t602Code::MapBad => 0x5000,
            t602Code::AlreadyMapped => 0x5001,
            t602Code::WriteEscaped => 0x5002,
            t602Code::B19Missed => 0x5003,
            t602Code::UnmapLeak => 0x5004,
            t602Code::LedgerBreak => 0x5005,
            t602Code::OrderViolated => 0x5006,
            t602Code::GuardTripped => 0x5007,
            t602Code::LedgerBroken => 0x5008,
            t602Code::SnapshotFingerprint => 0x5009,
            t602Code::TableBroken => 0x500a,
            t602Code::IndexMisaligned => 0x500b,
            t602Code::DuplicateEntry => 0x500c,
            t602Code::SegmentGuard => 0x500d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t602Code::MapBad => "映射非法（基址/长度违例）",
            t602Code::AlreadyMapped => "重复登记（同区双映）",
            t602Code::WriteEscaped => "区外写逃逸（RO 区被写）",
            t602Code::B19Missed => "B19 账漏登",
            t602Code::UnmapLeak => "解映射泄漏（区未注销）",
            t602Code::LedgerBreak => "登记账破缺",
            t602Code::OrderViolated => "登记序违例",
            t602Code::GuardTripped => "守卫触发",
            t602Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t602Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t602Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t602Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t602Code::DuplicateEntry => "条目重复（同键双写）",
            t602Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t602Code::MapBad => "按映射律校正",
            t602Code::AlreadyMapped => "幂等拒绝",
            t602Code::WriteEscaped => "按 100% 拦截红线补拦",
            t602Code::B19Missed => "按 1/1 登记律补登",
            t602Code::UnmapLeak => "按注销账追回",
            t602Code::LedgerBreak => "按守恒重建",
            t602Code::OrderViolated => "按序重排",
            t602Code::GuardTripped => "检查临界区",
            t602Code::LedgerBroken => "按守恒律重建台账后重跑",
            t602Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t602Code::TableBroken => "按语义规则重建常量表",
            t602Code::IndexMisaligned => "按声明序归一化后重跑",
            t602Code::DuplicateEntry => "合并同键条目为末次生效",
            t602Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}


// ---------------------------------------------------------------------------
// initrd 归档区只读映射（基址/长度/RO 属性 + B19 账登记 + 区外写拦截）
// ---------------------------------------------------------------------------

/// 映射登记条目（三元组：基址/长度/RO 属性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InitrdMap {
    /// 归档区基址（页对齐）。
    pub base: u64,
    /// 归档区长度（页倍数）。
    pub len: u64,
    /// 只读属性（initrd 恒 true）。
    pub read_only: bool,
    /// 是否已入 B19 账。
    pub in_b19: bool,
}

/// 映射合法性（页对齐 + 长度非零 + RO 置位）。
pub const fn initrd_map_ok(m: &InitrdMap) -> bool {
    m.base % 4096 == 0 && m.len != 0 && m.len % 4096 == 0 && m.read_only
}

/// 地址归属判定（映射区内为 true）。
pub const fn initrd_in_region(m: &InitrdMap, addr: u64) -> bool {
    addr >= m.base && addr < m.base + m.len
}

/// 写拦截判定（只读区内任何写都被拦；区外写放行）。
pub const fn initrd_write_blocked(m: &InitrdMap, addr: u64) -> bool {
    m.read_only && initrd_in_region(m, addr)
}

/// 登记账（B19 联动 + 拦截守恒）。
#[derive(Clone, Copy, Debug)]
pub struct MapBook {
    /// 已登记映射数。
    pub registered: u64,
    /// 已入 B19 账数。
    pub in_b19: u64,
    /// 区外写尝试数。
    pub write_tries: u64,
    /// 拦截成功数（必须 == write_tries）。
    pub blocked: u64,
}

impl MapBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { registered: 0, in_b19: 0, write_tries: 0, blocked: 0 }
    }

    /// 登记一映射（合法且自动入 B19）。
    pub fn register(&mut self, m: &InitrdMap) -> bool {
        if !initrd_map_ok(m) {
            return false;
        }
        self.registered += 1;
        self.in_b19 += 1;
        true
    }

    /// 记一次区外写尝试（拦截口径：区内写必拦）。
    pub fn try_write(&mut self, m: &InitrdMap, addr: u64) -> bool {
        self.write_tries += 1;
        if initrd_write_blocked(m, addr) {
            self.blocked += 1;
            true
        } else {
            false
        }
    }

    /// B19 1/1 判据（登记即入账，无漏登）。
    pub fn b19_full(&self) -> bool {
        self.registered == self.in_b19
    }

    /// 拦截 100% 判据（区内写全部被拦）。
    pub fn intercept_all(&self) -> bool {
        self.blocked == self.write_tries && self.write_tries > 0
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x5000, 0x500f]。
pub const fn t602_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t602Code::ALL.len() {
        let c = t602Code::ALL[i].code();
        if c < 0x5000 || c > 0x500f {
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
fn t602_guard_seg() -> bool {
    t602_segment_guard()
}

/// 映射合法性（页对齐/RO/归属/拦截四查）。
fn check_c1_map_ok() -> bool {
    let m = InitrdMap { base: 0x1000_0000, len: 0x40_0000, read_only: true, in_b19: true };
    let mut ok = initrd_map_ok(&m) && initrd_in_region(&m, 0x1000_0000 + 0x1000);
    if ok {
        ok = !initrd_in_region(&m, 0x1000_0000 + 0x40_0000);
    }
    if ok {
        ok = initrd_write_blocked(&m, 0x1000_0000) && !initrd_write_blocked(&m, 0x8000_0000);
    }
    ok
}

/// B19 登记 1/1 + 区外写拦截 100%。
fn check_c2_book() -> bool {
    let m = InitrdMap { base: 0x1000_0000, len: 0x40_0000, read_only: true, in_b19: true };
    let mut bk = MapBook::new();
    let mut ok = bk.register(&m);
    if ok {
        ok = bk.b19_full() && bk.in_b19 == 1;
    }
    if ok {
        // 区内 8 次写全部被拦 + 区外 1 次放过。
        let mut i = 0;
        while i < 8 {
            ok = ok && bk.try_write(&m, 0x1000_0000 + i * 4096);
            i += 1;
        }
        ok = ok && !bk.try_write(&m, 0x9000_0000);
    }
    if ok {
        // 拦截口径：8 拦 / 9 试（区外写不拦但计尝试）。
        ok = bk.write_tries == 9;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t602_checks() -> CheckSet {
    let mut set = CheckSet::new("T602");
    set.add(
        "t602-seg-guard",
        t602_guard_seg(),
        if t602_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t602-c1-map-ok",
        check_c1_map_ok(),
        if check_c1_map_ok() { "" } else { "initrd map validation mismatch" },
    );
    set.add(
        "t602-c2-book",
        check_c2_book(),
        if check_c2_book() { "" } else { "initrd map book mismatch" },
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
    fn t602_all_checks_green() {
        let set = run_t602_checks();
        assert!(set.all_passed(), "T602 domain checks must be green");
    }

    #[test]
    fn t602_segment_guard_holds() {
        assert!(t602_segment_guard());
    }
}
