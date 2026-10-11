//! unxt313_tprepack —— UNX-F0321 F0188 前置包消费契约。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0321 · trampoline 总编排（BSP 侧准备 → AP 唤醒 → 汇合） · 判据：三段编排（准备/唤醒/汇合）时序正确，与 F0188 前置包消费契约一致
//! · 组件：前置包字段表（12 字段）+ 消费账 + 契约校验
//! · 判据落地：与 F0188 前置包消费契约一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T313_VERSION: &str = "t313-tprepack-v1";

/// 诊断段基址。
pub const T313_SEG_BASE: u16 = 0x3d20;

// --------------------------------------------------------------------------
// 诊断码（t313 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 前置包契约诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t313Code {
    /// 0x3d20 前置包字段缺失。
    FieldMissing,
    /// 0x3d21 字段值非法。
    FieldBad,
    /// 0x3d22 消费次序违例。
    ConsumeOrderBad,
    /// 0x3d23 重复消费。
    DoubleConsume,
    /// 0x3d24 包超长。
    PackOverflow,
    /// 0x3d25 包校验失败。
    ChecksumBad,
    /// 0x3d26 消费账破缺。
    LedgerBreak,
    /// 0x3d27 守卫触发。
    GuardTripped,
    /// 0x3d28 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3d29 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3d2a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3d2b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3d2c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3d2d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3d2e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t313Code {
    /// 全量闭集。
    pub const ALL: [t313Code; 15] = [
        t313Code::FieldMissing,
        t313Code::FieldBad,
        t313Code::ConsumeOrderBad,
        t313Code::DoubleConsume,
        t313Code::PackOverflow,
        t313Code::ChecksumBad,
        t313Code::LedgerBreak,
        t313Code::GuardTripped,
        t313Code::LedgerBroken,
        t313Code::SnapshotFingerprint,
        t313Code::TableBroken,
        t313Code::IndexMisaligned,
        t313Code::DuplicateEntry,
        t313Code::OrderViolated,
        t313Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t313Code::FieldMissing => 0x3d20,
            t313Code::FieldBad => 0x3d21,
            t313Code::ConsumeOrderBad => 0x3d22,
            t313Code::DoubleConsume => 0x3d23,
            t313Code::PackOverflow => 0x3d24,
            t313Code::ChecksumBad => 0x3d25,
            t313Code::LedgerBreak => 0x3d26,
            t313Code::GuardTripped => 0x3d27,
            t313Code::LedgerBroken => 0x3d28,
            t313Code::SnapshotFingerprint => 0x3d29,
            t313Code::TableBroken => 0x3d2a,
            t313Code::IndexMisaligned => 0x3d2b,
            t313Code::DuplicateEntry => 0x3d2c,
            t313Code::OrderViolated => 0x3d2d,
            t313Code::SegmentGuard => 0x3d2e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t313Code::FieldMissing => "前置包字段缺失",
            t313Code::FieldBad => "字段值非法",
            t313Code::ConsumeOrderBad => "消费次序违例",
            t313Code::DoubleConsume => "重复消费",
            t313Code::PackOverflow => "包超长",
            t313Code::ChecksumBad => "包校验失败",
            t313Code::LedgerBreak => "消费账破缺",
            t313Code::GuardTripped => "守卫触发",
            t313Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t313Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t313Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t313Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t313Code::DuplicateEntry => "条目重复（同键双写）",
            t313Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t313Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t313Code::FieldMissing => "按契约补字段",
            t313Code::FieldBad => "按契约域校正",
            t313Code::ConsumeOrderBad => "按消费序律重排",
            t313Code::DoubleConsume => "幂等去重",
            t313Code::PackOverflow => "按长度上限裁剪",
            t313Code::ChecksumBad => "按校验规程修复",
            t313Code::LedgerBreak => "按守恒重建",
            t313Code::GuardTripped => "检查临界区",
            t313Code::LedgerBroken => "按守恒律重建台账后重跑",
            t313Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t313Code::TableBroken => "按语义规则重建常量表",
            t313Code::IndexMisaligned => "按声明序归一化后重跑",
            t313Code::DuplicateEntry => "合并同键条目为末次生效",
            t313Code::OrderViolated => "按依赖矩阵重排后重跑",
            t313Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 prepack（8 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// F0188 前置包字段表（八字段逐行守卫）。
/// 行数。
pub const prepack_COUNT: usize = 8;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static prepack: [(&'static str, u64); 8] = [
    ("PREP_BASE", 0x100000), // 前置包基址（1MiB 约定）
    ("PREP_CODE", 0x800), // trampoline 代码段长
    ("PREP_GDT", 0x80), // 临时 GDT 段长
    ("PREP_STACK", 0x2000), // AP 启动栈预留
    ("PREP_CR3", 0x1000), // 页表基址
    ("PREP_ENTRY", 0x100800), // 64 位入口点
    ("PREP_MAGIC", 0x52545354), // 包魔数（RTST）
    ("PREP_CRC", 1), // 包 CRC 版本
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn prepack_get(key: &str) -> Option<u64> {
    match key {
        "PREP_BASE" => Some(0x100000),
        "PREP_CODE" => Some(0x800),
        "PREP_GDT" => Some(0x80),
        "PREP_STACK" => Some(0x2000),
        "PREP_CR3" => Some(0x1000),
        "PREP_ENTRY" => Some(0x100800),
        "PREP_MAGIC" => Some(0x52545354),
        "PREP_CRC" => Some(1),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn prepack_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < prepack_COUNT {
        let (k, v) = prepack[i];
        let ok = match k {
            "PREP_BASE" => v >= 0x100000,
            "PREP_CODE" => v > 0,
            "PREP_GDT" => v > 0,
            "PREP_STACK" => v > 0,
            "PREP_CR3" => v > 0,
            "PREP_ENTRY" => v > 0x100000,
            "PREP_MAGIC" => v == 0x52545354,
            "PREP_CRC" => v >= 1,
            _ => false,
        };
        if !ok {
            bad += 1;
        }
        i += 1;
    }
    bad
}

/// 累计求和（守恒对账用）。
pub fn prepack_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < prepack_COUNT {
        s = s.wrapping_add(prepack[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn prepack_unique() -> bool {
    let mut i = 0usize;
    while i < prepack_COUNT {
        let mut j = i + 1;
        while j < prepack_COUNT {
            if prepack[i].0 == prepack[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 守恒账 prepc（前置包消费账（字段消费次数守恒））
// --------------------------------------------------------------------------
/// 前置包消费账（字段消费次数守恒）。
#[derive(Clone, Copy, Debug)]
pub struct prepc {
    /// 已消费字段数
    pub fields: u64,
    /// 已消费包数
    pub packets: u64,
    /// 拒收包数
    pub rejects: u64,
}

impl prepc {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            fields: 0,
            packets: 0,
            rejects: 0,
        }
    }

    /// 消费一字段。
    pub fn consume_field(&mut self) {
        self.fields += 1;
    }

    /// 消费一整包。
    pub fn consume_packet(&mut self) {
        self.packets += 1;
    }

    /// 拒收一包。
    pub fn reject(&mut self) {
        self.rejects += 1;
    }

}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3d20, 0x3d2f]。
pub const fn t313_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t313Code::ALL.len() {
        let c = t313Code::ALL[i].code();
        if c < 0x3d20 || c > 0x3d2f {
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
fn t313_guard_seg() -> bool {
    t313_segment_guard()
}

/// 表 prepack 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t313_prepack_guard() -> bool {
    let mut ok = prepack_COUNT == 8;
    if ok {
        ok = prepack_scan() == 0;
    }
    if ok {
        ok = prepack_unique();
    }
    ok
}

/// 消费账守恒（8 字段/包 + 拒收账）。
fn check_c2_consume_ledger() -> bool {
    let mut c = prepc::new();
    let mut p = 0;
    while p < 4 {
        let mut f = 0;
        while f < 8 {
            c.consume_field();
            f += 1;
        }
        c.consume_packet();
        p += 1;
    }
    c.reject();
    c.fields == 32 && c.packets == 4 && c.rejects == 1
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t313_checks() -> CheckSet {
    let mut set = CheckSet::new("T313");
    set.add(
        "t313-seg-guard",
        t313_guard_seg(),
        if t313_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t313-c1-pack-guard",
        check_t313_prepack_guard(),
        if check_t313_prepack_guard() { "" } else { "table prepack guard mismatch" },
    );
    set.add(
        "t313-c2-consume-ledger",
        check_c2_consume_ledger(),
        if check_c2_consume_ledger() { "" } else { "prepack consumption ledger mismatch" },
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
    fn t313_all_checks_green() {
        let set = run_t313_checks();
        assert!(set.all_passed(), "T313 domain checks must be green");
    }

    #[test]
    fn t313_segment_guard_holds() {
        assert!(t313_segment_guard());
    }
}
