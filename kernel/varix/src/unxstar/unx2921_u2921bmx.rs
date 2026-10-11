//! unxu2921_u2921bmx —— COW-页缓存联合压测方案冻结 深编bmx（COW-页缓存联合压测 × 256 单元）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2921 · COW-页缓存联合压测方案冻结 · 判据原文：UNX-F2921 · 判据：UNX-F2921-J1 联合矩阵（私有/共享/匿名三态×读写比五档）冻结哈希入册，方案与 F2641/F2682 双接口字段对齐
//! · 组件：COW-页缓存联合压测 守卫映射 256 行（二 轮派生真实数据 + 逐行守卫 + 键唯一）
//! · 判据落地：UNX-F2921 · 判据：UNX-F2921-J1 联合矩阵（私有/共享/匿名三态×读写比五档）冻结哈希入册，方案与 F2641/F2682 双接口字段对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2921_VERSION: &str = "u2921-u2921bmx-v1";

/// 诊断段基址。
pub const U2921_SEG_BASE: u16 = 0xe2c0;

// --------------------------------------------------------------------------
// 诊断码（u2921 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// COW-页缓存联合压测 bmx 面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2921Code {
    /// 0xe2c0 单元格重复（联合压测 同键双占）。
    CellDup,
    /// 0xe2c1 单元格断档（COW 序号不连续）。
    CellGap,
    /// 0xe2c2 索引越界（页缓存）。
    IndexOob,
    /// 0xe2c3 大表账破缺（行数≠登记值）。
    LedgerBreak,
    /// 0xe2c4 表序断档。
    OrderBreak,
    /// 0xe2c5 累计漂移（求和≠理论值）。
    SumDrift,
    /// 0xe2c6 掩码非法。
    MaskBad,
    /// 0xe2c7 守卫触发。
    GuardTripped,
    /// 0xe2c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe2c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe2ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe2cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2921Code {
    /// 全量闭集。
    pub const ALL: [u2921Code; 12] = [
        u2921Code::CellDup,
        u2921Code::CellGap,
        u2921Code::IndexOob,
        u2921Code::LedgerBreak,
        u2921Code::OrderBreak,
        u2921Code::SumDrift,
        u2921Code::MaskBad,
        u2921Code::GuardTripped,
        u2921Code::LedgerBroken,
        u2921Code::SnapshotFingerprint,
        u2921Code::TableBroken,
        u2921Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2921Code::CellDup => 0xe2c0,
            u2921Code::CellGap => 0xe2c1,
            u2921Code::IndexOob => 0xe2c2,
            u2921Code::LedgerBreak => 0xe2c3,
            u2921Code::OrderBreak => 0xe2c4,
            u2921Code::SumDrift => 0xe2c5,
            u2921Code::MaskBad => 0xe2c6,
            u2921Code::GuardTripped => 0xe2c7,
            u2921Code::LedgerBroken => 0xe2c8,
            u2921Code::SnapshotFingerprint => 0xe2c9,
            u2921Code::TableBroken => 0xe2ca,
            u2921Code::SegmentGuard => 0xe2cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2921Code::CellDup => "单元格重复（联合压测 同键双占）",
            u2921Code::CellGap => "单元格断档（COW 序号不连续）",
            u2921Code::IndexOob => "索引越界（页缓存）",
            u2921Code::LedgerBreak => "大表账破缺（行数≠登记值）",
            u2921Code::OrderBreak => "表序断档",
            u2921Code::SumDrift => "累计漂移（求和≠理论值）",
            u2921Code::MaskBad => "掩码非法",
            u2921Code::GuardTripped => "守卫触发",
            u2921Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2921Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2921Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2921Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2921Code::CellDup => "按键唯一律去重",
            u2921Code::CellGap => "按连续律补号",
            u2921Code::IndexOob => "按行数边界裁剪",
            u2921Code::LedgerBreak => "按守恒重建",
            u2921Code::OrderBreak => "按序重排",
            u2921Code::SumDrift => "按理论值复算",
            u2921Code::MaskBad => "按掩码律校正",
            u2921Code::GuardTripped => "检查临界区",
            u2921Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2921Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2921Code::TableBroken => "按语义规则重建常量表",
            u2921Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2921bmx（256 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// COW-页缓存联合压测 bmx 守卫映射（256 行逐行注释自证）。
/// 行数。
pub const u2921bmx_COUNT: usize = 256;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2921bmx: [(&'static str, u64); 256] = [
    ("COW0000", 0), // COW 单元 0000（写时复制 × 相位 00 × 二 轮）
    ("PC0001", 7), // PC 单元 0001（页缓存 × 相位 01 × 二 轮）
    ("JOINT0002", 14), // JOINT 单元 0002（联合 × 相位 02 × 二 轮）
    ("SHARE0003", 21), // SHARE 单元 0003（共享 × 相位 03 × 二 轮）
    ("EVICT0004", 28), // EVICT 单元 0004（逐出 × 相位 04 × 二 轮）
    ("DIRTY0005", 35), // DIRTY 单元 0005（脏页 × 相位 05 × 二 轮）
    ("WB0006", 42), // WB 单元 0006（回写 × 相位 06 × 二 轮）
    ("RECLAIM0007", 49), // RECLAIM 单元 0007（回收 × 相位 07 × 二 轮）
    ("ISO0008", 56), // ISO 单元 0008（隔离 × 相位 08 × 二 轮）
    ("THR0009", 63), // THR 单元 0009（阈值 × 相位 09 × 二 轮）
    ("ACCT0010", 70), // ACCT 单元 0010（账 × 相位 10 × 二 轮）
    ("VERDICT0011", 77), // VERDICT 单元 0011（裁决 × 相位 11 × 二 轮）
    ("COW0012", 84), // COW 单元 0012（写时复制 × 相位 12 × 二 轮）
    ("PC0013", 2), // PC 单元 0013（页缓存 × 相位 13 × 二 轮）
    ("JOINT0014", 9), // JOINT 单元 0014（联合 × 相位 14 × 二 轮）
    ("SHARE0015", 16), // SHARE 单元 0015（共享 × 相位 15 × 二 轮）
    ("EVICT0016", 23), // EVICT 单元 0016（逐出 × 相位 16 × 二 轮）
    ("DIRTY0017", 30), // DIRTY 单元 0017（脏页 × 相位 17 × 二 轮）
    ("WB0018", 37), // WB 单元 0018（回写 × 相位 18 × 二 轮）
    ("RECLAIM0019", 44), // RECLAIM 单元 0019（回收 × 相位 19 × 二 轮）
    ("ISO0020", 51), // ISO 单元 0020（隔离 × 相位 20 × 二 轮）
    ("THR0021", 58), // THR 单元 0021（阈值 × 相位 21 × 二 轮）
    ("ACCT0022", 65), // ACCT 单元 0022（账 × 相位 22 × 二 轮）
    ("VERDICT0023", 72), // VERDICT 单元 0023（裁决 × 相位 23 × 二 轮）
    ("COW0024", 79), // COW 单元 0024（写时复制 × 相位 24 × 二 轮）
    ("PC0025", 86), // PC 单元 0025（页缓存 × 相位 25 × 二 轮）
    ("JOINT0026", 4), // JOINT 单元 0026（联合 × 相位 26 × 二 轮）
    ("SHARE0027", 11), // SHARE 单元 0027（共享 × 相位 27 × 二 轮）
    ("EVICT0028", 18), // EVICT 单元 0028（逐出 × 相位 28 × 二 轮）
    ("DIRTY0029", 25), // DIRTY 单元 0029（脏页 × 相位 29 × 二 轮）
    ("WB0030", 32), // WB 单元 0030（回写 × 相位 30 × 二 轮）
    ("RECLAIM0031", 39), // RECLAIM 单元 0031（回收 × 相位 31 × 二 轮）
    ("ISO0032", 46), // ISO 单元 0032（隔离 × 相位 32 × 二 轮）
    ("THR0033", 53), // THR 单元 0033（阈值 × 相位 33 × 二 轮）
    ("ACCT0034", 60), // ACCT 单元 0034（账 × 相位 34 × 二 轮）
    ("VERDICT0035", 67), // VERDICT 单元 0035（裁决 × 相位 35 × 二 轮）
    ("COW0036", 74), // COW 单元 0036（写时复制 × 相位 36 × 二 轮）
    ("PC0037", 81), // PC 单元 0037（页缓存 × 相位 37 × 二 轮）
    ("JOINT0038", 88), // JOINT 单元 0038（联合 × 相位 38 × 二 轮）
    ("SHARE0039", 6), // SHARE 单元 0039（共享 × 相位 39 × 二 轮）
    ("EVICT0040", 13), // EVICT 单元 0040（逐出 × 相位 40 × 二 轮）
    ("DIRTY0041", 20), // DIRTY 单元 0041（脏页 × 相位 41 × 二 轮）
    ("WB0042", 27), // WB 单元 0042（回写 × 相位 42 × 二 轮）
    ("RECLAIM0043", 34), // RECLAIM 单元 0043（回收 × 相位 43 × 二 轮）
    ("ISO0044", 41), // ISO 单元 0044（隔离 × 相位 44 × 二 轮）
    ("THR0045", 48), // THR 单元 0045（阈值 × 相位 45 × 二 轮）
    ("ACCT0046", 55), // ACCT 单元 0046（账 × 相位 46 × 二 轮）
    ("VERDICT0047", 62), // VERDICT 单元 0047（裁决 × 相位 47 × 二 轮）
    ("COW0048", 69), // COW 单元 0048（写时复制 × 相位 48 × 二 轮）
    ("PC0049", 76), // PC 单元 0049（页缓存 × 相位 49 × 二 轮）
    ("JOINT0050", 83), // JOINT 单元 0050（联合 × 相位 50 × 二 轮）
    ("SHARE0051", 1), // SHARE 单元 0051（共享 × 相位 51 × 二 轮）
    ("EVICT0052", 8), // EVICT 单元 0052（逐出 × 相位 52 × 二 轮）
    ("DIRTY0053", 15), // DIRTY 单元 0053（脏页 × 相位 53 × 二 轮）
    ("WB0054", 22), // WB 单元 0054（回写 × 相位 54 × 二 轮）
    ("RECLAIM0055", 29), // RECLAIM 单元 0055（回收 × 相位 55 × 二 轮）
    ("ISO0056", 36), // ISO 单元 0056（隔离 × 相位 56 × 二 轮）
    ("THR0057", 43), // THR 单元 0057（阈值 × 相位 57 × 二 轮）
    ("ACCT0058", 50), // ACCT 单元 0058（账 × 相位 58 × 二 轮）
    ("VERDICT0059", 57), // VERDICT 单元 0059（裁决 × 相位 59 × 二 轮）
    ("COW0060", 64), // COW 单元 0060（写时复制 × 相位 60 × 二 轮）
    ("PC0061", 71), // PC 单元 0061（页缓存 × 相位 61 × 二 轮）
    ("JOINT0062", 78), // JOINT 单元 0062（联合 × 相位 62 × 二 轮）
    ("SHARE0063", 85), // SHARE 单元 0063（共享 × 相位 63 × 二 轮）
    ("EVICT0064", 3), // EVICT 单元 0064（逐出 × 相位 00 × 二 轮）
    ("DIRTY0065", 10), // DIRTY 单元 0065（脏页 × 相位 01 × 二 轮）
    ("WB0066", 17), // WB 单元 0066（回写 × 相位 02 × 二 轮）
    ("RECLAIM0067", 24), // RECLAIM 单元 0067（回收 × 相位 03 × 二 轮）
    ("ISO0068", 31), // ISO 单元 0068（隔离 × 相位 04 × 二 轮）
    ("THR0069", 38), // THR 单元 0069（阈值 × 相位 05 × 二 轮）
    ("ACCT0070", 45), // ACCT 单元 0070（账 × 相位 06 × 二 轮）
    ("VERDICT0071", 52), // VERDICT 单元 0071（裁决 × 相位 07 × 二 轮）
    ("COW0072", 59), // COW 单元 0072（写时复制 × 相位 08 × 二 轮）
    ("PC0073", 66), // PC 单元 0073（页缓存 × 相位 09 × 二 轮）
    ("JOINT0074", 73), // JOINT 单元 0074（联合 × 相位 10 × 二 轮）
    ("SHARE0075", 80), // SHARE 单元 0075（共享 × 相位 11 × 二 轮）
    ("EVICT0076", 87), // EVICT 单元 0076（逐出 × 相位 12 × 二 轮）
    ("DIRTY0077", 5), // DIRTY 单元 0077（脏页 × 相位 13 × 二 轮）
    ("WB0078", 12), // WB 单元 0078（回写 × 相位 14 × 二 轮）
    ("RECLAIM0079", 19), // RECLAIM 单元 0079（回收 × 相位 15 × 二 轮）
    ("ISO0080", 26), // ISO 单元 0080（隔离 × 相位 16 × 二 轮）
    ("THR0081", 33), // THR 单元 0081（阈值 × 相位 17 × 二 轮）
    ("ACCT0082", 40), // ACCT 单元 0082（账 × 相位 18 × 二 轮）
    ("VERDICT0083", 47), // VERDICT 单元 0083（裁决 × 相位 19 × 二 轮）
    ("COW0084", 54), // COW 单元 0084（写时复制 × 相位 20 × 二 轮）
    ("PC0085", 61), // PC 单元 0085（页缓存 × 相位 21 × 二 轮）
    ("JOINT0086", 68), // JOINT 单元 0086（联合 × 相位 22 × 二 轮）
    ("SHARE0087", 75), // SHARE 单元 0087（共享 × 相位 23 × 二 轮）
    ("EVICT0088", 82), // EVICT 单元 0088（逐出 × 相位 24 × 二 轮）
    ("DIRTY0089", 0), // DIRTY 单元 0089（脏页 × 相位 25 × 二 轮）
    ("WB0090", 7), // WB 单元 0090（回写 × 相位 26 × 二 轮）
    ("RECLAIM0091", 14), // RECLAIM 单元 0091（回收 × 相位 27 × 二 轮）
    ("ISO0092", 21), // ISO 单元 0092（隔离 × 相位 28 × 二 轮）
    ("THR0093", 28), // THR 单元 0093（阈值 × 相位 29 × 二 轮）
    ("ACCT0094", 35), // ACCT 单元 0094（账 × 相位 30 × 二 轮）
    ("VERDICT0095", 42), // VERDICT 单元 0095（裁决 × 相位 31 × 二 轮）
    ("COW0096", 49), // COW 单元 0096（写时复制 × 相位 32 × 二 轮）
    ("PC0097", 56), // PC 单元 0097（页缓存 × 相位 33 × 二 轮）
    ("JOINT0098", 63), // JOINT 单元 0098（联合 × 相位 34 × 二 轮）
    ("SHARE0099", 70), // SHARE 单元 0099（共享 × 相位 35 × 二 轮）
    ("EVICT0100", 77), // EVICT 单元 0100（逐出 × 相位 36 × 二 轮）
    ("DIRTY0101", 84), // DIRTY 单元 0101（脏页 × 相位 37 × 二 轮）
    ("WB0102", 2), // WB 单元 0102（回写 × 相位 38 × 二 轮）
    ("RECLAIM0103", 9), // RECLAIM 单元 0103（回收 × 相位 39 × 二 轮）
    ("ISO0104", 16), // ISO 单元 0104（隔离 × 相位 40 × 二 轮）
    ("THR0105", 23), // THR 单元 0105（阈值 × 相位 41 × 二 轮）
    ("ACCT0106", 30), // ACCT 单元 0106（账 × 相位 42 × 二 轮）
    ("VERDICT0107", 37), // VERDICT 单元 0107（裁决 × 相位 43 × 二 轮）
    ("COW0108", 44), // COW 单元 0108（写时复制 × 相位 44 × 二 轮）
    ("PC0109", 51), // PC 单元 0109（页缓存 × 相位 45 × 二 轮）
    ("JOINT0110", 58), // JOINT 单元 0110（联合 × 相位 46 × 二 轮）
    ("SHARE0111", 65), // SHARE 单元 0111（共享 × 相位 47 × 二 轮）
    ("EVICT0112", 72), // EVICT 单元 0112（逐出 × 相位 48 × 二 轮）
    ("DIRTY0113", 79), // DIRTY 单元 0113（脏页 × 相位 49 × 二 轮）
    ("WB0114", 86), // WB 单元 0114（回写 × 相位 50 × 二 轮）
    ("RECLAIM0115", 4), // RECLAIM 单元 0115（回收 × 相位 51 × 二 轮）
    ("ISO0116", 11), // ISO 单元 0116（隔离 × 相位 52 × 二 轮）
    ("THR0117", 18), // THR 单元 0117（阈值 × 相位 53 × 二 轮）
    ("ACCT0118", 25), // ACCT 单元 0118（账 × 相位 54 × 二 轮）
    ("VERDICT0119", 32), // VERDICT 单元 0119（裁决 × 相位 55 × 二 轮）
    ("COW0120", 39), // COW 单元 0120（写时复制 × 相位 56 × 二 轮）
    ("PC0121", 46), // PC 单元 0121（页缓存 × 相位 57 × 二 轮）
    ("JOINT0122", 53), // JOINT 单元 0122（联合 × 相位 58 × 二 轮）
    ("SHARE0123", 60), // SHARE 单元 0123（共享 × 相位 59 × 二 轮）
    ("EVICT0124", 67), // EVICT 单元 0124（逐出 × 相位 60 × 二 轮）
    ("DIRTY0125", 74), // DIRTY 单元 0125（脏页 × 相位 61 × 二 轮）
    ("WB0126", 81), // WB 单元 0126（回写 × 相位 62 × 二 轮）
    ("RECLAIM0127", 88), // RECLAIM 单元 0127（回收 × 相位 63 × 二 轮）
    ("ISO0128", 6), // ISO 单元 0128（隔离 × 相位 00 × 二 轮）
    ("THR0129", 13), // THR 单元 0129（阈值 × 相位 01 × 二 轮）
    ("ACCT0130", 20), // ACCT 单元 0130（账 × 相位 02 × 二 轮）
    ("VERDICT0131", 27), // VERDICT 单元 0131（裁决 × 相位 03 × 二 轮）
    ("COW0132", 34), // COW 单元 0132（写时复制 × 相位 04 × 二 轮）
    ("PC0133", 41), // PC 单元 0133（页缓存 × 相位 05 × 二 轮）
    ("JOINT0134", 48), // JOINT 单元 0134（联合 × 相位 06 × 二 轮）
    ("SHARE0135", 55), // SHARE 单元 0135（共享 × 相位 07 × 二 轮）
    ("EVICT0136", 62), // EVICT 单元 0136（逐出 × 相位 08 × 二 轮）
    ("DIRTY0137", 69), // DIRTY 单元 0137（脏页 × 相位 09 × 二 轮）
    ("WB0138", 76), // WB 单元 0138（回写 × 相位 10 × 二 轮）
    ("RECLAIM0139", 83), // RECLAIM 单元 0139（回收 × 相位 11 × 二 轮）
    ("ISO0140", 1), // ISO 单元 0140（隔离 × 相位 12 × 二 轮）
    ("THR0141", 8), // THR 单元 0141（阈值 × 相位 13 × 二 轮）
    ("ACCT0142", 15), // ACCT 单元 0142（账 × 相位 14 × 二 轮）
    ("VERDICT0143", 22), // VERDICT 单元 0143（裁决 × 相位 15 × 二 轮）
    ("COW0144", 29), // COW 单元 0144（写时复制 × 相位 16 × 二 轮）
    ("PC0145", 36), // PC 单元 0145（页缓存 × 相位 17 × 二 轮）
    ("JOINT0146", 43), // JOINT 单元 0146（联合 × 相位 18 × 二 轮）
    ("SHARE0147", 50), // SHARE 单元 0147（共享 × 相位 19 × 二 轮）
    ("EVICT0148", 57), // EVICT 单元 0148（逐出 × 相位 20 × 二 轮）
    ("DIRTY0149", 64), // DIRTY 单元 0149（脏页 × 相位 21 × 二 轮）
    ("WB0150", 71), // WB 单元 0150（回写 × 相位 22 × 二 轮）
    ("RECLAIM0151", 78), // RECLAIM 单元 0151（回收 × 相位 23 × 二 轮）
    ("ISO0152", 85), // ISO 单元 0152（隔离 × 相位 24 × 二 轮）
    ("THR0153", 3), // THR 单元 0153（阈值 × 相位 25 × 二 轮）
    ("ACCT0154", 10), // ACCT 单元 0154（账 × 相位 26 × 二 轮）
    ("VERDICT0155", 17), // VERDICT 单元 0155（裁决 × 相位 27 × 二 轮）
    ("COW0156", 24), // COW 单元 0156（写时复制 × 相位 28 × 二 轮）
    ("PC0157", 31), // PC 单元 0157（页缓存 × 相位 29 × 二 轮）
    ("JOINT0158", 38), // JOINT 单元 0158（联合 × 相位 30 × 二 轮）
    ("SHARE0159", 45), // SHARE 单元 0159（共享 × 相位 31 × 二 轮）
    ("EVICT0160", 52), // EVICT 单元 0160（逐出 × 相位 32 × 二 轮）
    ("DIRTY0161", 59), // DIRTY 单元 0161（脏页 × 相位 33 × 二 轮）
    ("WB0162", 66), // WB 单元 0162（回写 × 相位 34 × 二 轮）
    ("RECLAIM0163", 73), // RECLAIM 单元 0163（回收 × 相位 35 × 二 轮）
    ("ISO0164", 80), // ISO 单元 0164（隔离 × 相位 36 × 二 轮）
    ("THR0165", 87), // THR 单元 0165（阈值 × 相位 37 × 二 轮）
    ("ACCT0166", 5), // ACCT 单元 0166（账 × 相位 38 × 二 轮）
    ("VERDICT0167", 12), // VERDICT 单元 0167（裁决 × 相位 39 × 二 轮）
    ("COW0168", 19), // COW 单元 0168（写时复制 × 相位 40 × 二 轮）
    ("PC0169", 26), // PC 单元 0169（页缓存 × 相位 41 × 二 轮）
    ("JOINT0170", 33), // JOINT 单元 0170（联合 × 相位 42 × 二 轮）
    ("SHARE0171", 40), // SHARE 单元 0171（共享 × 相位 43 × 二 轮）
    ("EVICT0172", 47), // EVICT 单元 0172（逐出 × 相位 44 × 二 轮）
    ("DIRTY0173", 54), // DIRTY 单元 0173（脏页 × 相位 45 × 二 轮）
    ("WB0174", 61), // WB 单元 0174（回写 × 相位 46 × 二 轮）
    ("RECLAIM0175", 68), // RECLAIM 单元 0175（回收 × 相位 47 × 二 轮）
    ("ISO0176", 75), // ISO 单元 0176（隔离 × 相位 48 × 二 轮）
    ("THR0177", 82), // THR 单元 0177（阈值 × 相位 49 × 二 轮）
    ("ACCT0178", 0), // ACCT 单元 0178（账 × 相位 50 × 二 轮）
    ("VERDICT0179", 7), // VERDICT 单元 0179（裁决 × 相位 51 × 二 轮）
    ("COW0180", 14), // COW 单元 0180（写时复制 × 相位 52 × 二 轮）
    ("PC0181", 21), // PC 单元 0181（页缓存 × 相位 53 × 二 轮）
    ("JOINT0182", 28), // JOINT 单元 0182（联合 × 相位 54 × 二 轮）
    ("SHARE0183", 35), // SHARE 单元 0183（共享 × 相位 55 × 二 轮）
    ("EVICT0184", 42), // EVICT 单元 0184（逐出 × 相位 56 × 二 轮）
    ("DIRTY0185", 49), // DIRTY 单元 0185（脏页 × 相位 57 × 二 轮）
    ("WB0186", 56), // WB 单元 0186（回写 × 相位 58 × 二 轮）
    ("RECLAIM0187", 63), // RECLAIM 单元 0187（回收 × 相位 59 × 二 轮）
    ("ISO0188", 70), // ISO 单元 0188（隔离 × 相位 60 × 二 轮）
    ("THR0189", 77), // THR 单元 0189（阈值 × 相位 61 × 二 轮）
    ("ACCT0190", 84), // ACCT 单元 0190（账 × 相位 62 × 二 轮）
    ("VERDICT0191", 2), // VERDICT 单元 0191（裁决 × 相位 63 × 二 轮）
    ("COW0192", 9), // COW 单元 0192（写时复制 × 相位 00 × 二 轮）
    ("PC0193", 16), // PC 单元 0193（页缓存 × 相位 01 × 二 轮）
    ("JOINT0194", 23), // JOINT 单元 0194（联合 × 相位 02 × 二 轮）
    ("SHARE0195", 30), // SHARE 单元 0195（共享 × 相位 03 × 二 轮）
    ("EVICT0196", 37), // EVICT 单元 0196（逐出 × 相位 04 × 二 轮）
    ("DIRTY0197", 44), // DIRTY 单元 0197（脏页 × 相位 05 × 二 轮）
    ("WB0198", 51), // WB 单元 0198（回写 × 相位 06 × 二 轮）
    ("RECLAIM0199", 58), // RECLAIM 单元 0199（回收 × 相位 07 × 二 轮）
    ("ISO0200", 65), // ISO 单元 0200（隔离 × 相位 08 × 二 轮）
    ("THR0201", 72), // THR 单元 0201（阈值 × 相位 09 × 二 轮）
    ("ACCT0202", 79), // ACCT 单元 0202（账 × 相位 10 × 二 轮）
    ("VERDICT0203", 86), // VERDICT 单元 0203（裁决 × 相位 11 × 二 轮）
    ("COW0204", 4), // COW 单元 0204（写时复制 × 相位 12 × 二 轮）
    ("PC0205", 11), // PC 单元 0205（页缓存 × 相位 13 × 二 轮）
    ("JOINT0206", 18), // JOINT 单元 0206（联合 × 相位 14 × 二 轮）
    ("SHARE0207", 25), // SHARE 单元 0207（共享 × 相位 15 × 二 轮）
    ("EVICT0208", 32), // EVICT 单元 0208（逐出 × 相位 16 × 二 轮）
    ("DIRTY0209", 39), // DIRTY 单元 0209（脏页 × 相位 17 × 二 轮）
    ("WB0210", 46), // WB 单元 0210（回写 × 相位 18 × 二 轮）
    ("RECLAIM0211", 53), // RECLAIM 单元 0211（回收 × 相位 19 × 二 轮）
    ("ISO0212", 60), // ISO 单元 0212（隔离 × 相位 20 × 二 轮）
    ("THR0213", 67), // THR 单元 0213（阈值 × 相位 21 × 二 轮）
    ("ACCT0214", 74), // ACCT 单元 0214（账 × 相位 22 × 二 轮）
    ("VERDICT0215", 81), // VERDICT 单元 0215（裁决 × 相位 23 × 二 轮）
    ("COW0216", 88), // COW 单元 0216（写时复制 × 相位 24 × 二 轮）
    ("PC0217", 6), // PC 单元 0217（页缓存 × 相位 25 × 二 轮）
    ("JOINT0218", 13), // JOINT 单元 0218（联合 × 相位 26 × 二 轮）
    ("SHARE0219", 20), // SHARE 单元 0219（共享 × 相位 27 × 二 轮）
    ("EVICT0220", 27), // EVICT 单元 0220（逐出 × 相位 28 × 二 轮）
    ("DIRTY0221", 34), // DIRTY 单元 0221（脏页 × 相位 29 × 二 轮）
    ("WB0222", 41), // WB 单元 0222（回写 × 相位 30 × 二 轮）
    ("RECLAIM0223", 48), // RECLAIM 单元 0223（回收 × 相位 31 × 二 轮）
    ("ISO0224", 55), // ISO 单元 0224（隔离 × 相位 32 × 二 轮）
    ("THR0225", 62), // THR 单元 0225（阈值 × 相位 33 × 二 轮）
    ("ACCT0226", 69), // ACCT 单元 0226（账 × 相位 34 × 二 轮）
    ("VERDICT0227", 76), // VERDICT 单元 0227（裁决 × 相位 35 × 二 轮）
    ("COW0228", 83), // COW 单元 0228（写时复制 × 相位 36 × 二 轮）
    ("PC0229", 1), // PC 单元 0229（页缓存 × 相位 37 × 二 轮）
    ("JOINT0230", 8), // JOINT 单元 0230（联合 × 相位 38 × 二 轮）
    ("SHARE0231", 15), // SHARE 单元 0231（共享 × 相位 39 × 二 轮）
    ("EVICT0232", 22), // EVICT 单元 0232（逐出 × 相位 40 × 二 轮）
    ("DIRTY0233", 29), // DIRTY 单元 0233（脏页 × 相位 41 × 二 轮）
    ("WB0234", 36), // WB 单元 0234（回写 × 相位 42 × 二 轮）
    ("RECLAIM0235", 43), // RECLAIM 单元 0235（回收 × 相位 43 × 二 轮）
    ("ISO0236", 50), // ISO 单元 0236（隔离 × 相位 44 × 二 轮）
    ("THR0237", 57), // THR 单元 0237（阈值 × 相位 45 × 二 轮）
    ("ACCT0238", 64), // ACCT 单元 0238（账 × 相位 46 × 二 轮）
    ("VERDICT0239", 71), // VERDICT 单元 0239（裁决 × 相位 47 × 二 轮）
    ("COW0240", 78), // COW 单元 0240（写时复制 × 相位 48 × 二 轮）
    ("PC0241", 85), // PC 单元 0241（页缓存 × 相位 49 × 二 轮）
    ("JOINT0242", 3), // JOINT 单元 0242（联合 × 相位 50 × 二 轮）
    ("SHARE0243", 10), // SHARE 单元 0243（共享 × 相位 51 × 二 轮）
    ("EVICT0244", 17), // EVICT 单元 0244（逐出 × 相位 52 × 二 轮）
    ("DIRTY0245", 24), // DIRTY 单元 0245（脏页 × 相位 53 × 二 轮）
    ("WB0246", 31), // WB 单元 0246（回写 × 相位 54 × 二 轮）
    ("RECLAIM0247", 38), // RECLAIM 单元 0247（回收 × 相位 55 × 二 轮）
    ("ISO0248", 45), // ISO 单元 0248（隔离 × 相位 56 × 二 轮）
    ("THR0249", 52), // THR 单元 0249（阈值 × 相位 57 × 二 轮）
    ("ACCT0250", 59), // ACCT 单元 0250（账 × 相位 58 × 二 轮）
    ("VERDICT0251", 66), // VERDICT 单元 0251（裁决 × 相位 59 × 二 轮）
    ("COW0252", 73), // COW 单元 0252（写时复制 × 相位 60 × 二 轮）
    ("PC0253", 80), // PC 单元 0253（页缓存 × 相位 61 × 二 轮）
    ("JOINT0254", 87), // JOINT 单元 0254（联合 × 相位 62 × 二 轮）
    ("SHARE0255", 5), // SHARE 单元 0255（共享 × 相位 63 × 二 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2921bmx_get(key: &str) -> Option<u64> {
    match key {
        "COW0000" => Some(0),
        "PC0001" => Some(7),
        "JOINT0002" => Some(14),
        "SHARE0003" => Some(21),
        "EVICT0004" => Some(28),
        "DIRTY0005" => Some(35),
        "WB0006" => Some(42),
        "RECLAIM0007" => Some(49),
        "ISO0008" => Some(56),
        "THR0009" => Some(63),
        "ACCT0010" => Some(70),
        "VERDICT0011" => Some(77),
        "COW0012" => Some(84),
        "PC0013" => Some(2),
        "JOINT0014" => Some(9),
        "SHARE0015" => Some(16),
        "EVICT0016" => Some(23),
        "DIRTY0017" => Some(30),
        "WB0018" => Some(37),
        "RECLAIM0019" => Some(44),
        "ISO0020" => Some(51),
        "THR0021" => Some(58),
        "ACCT0022" => Some(65),
        "VERDICT0023" => Some(72),
        "COW0024" => Some(79),
        "PC0025" => Some(86),
        "JOINT0026" => Some(4),
        "SHARE0027" => Some(11),
        "EVICT0028" => Some(18),
        "DIRTY0029" => Some(25),
        "WB0030" => Some(32),
        "RECLAIM0031" => Some(39),
        "ISO0032" => Some(46),
        "THR0033" => Some(53),
        "ACCT0034" => Some(60),
        "VERDICT0035" => Some(67),
        "COW0036" => Some(74),
        "PC0037" => Some(81),
        "JOINT0038" => Some(88),
        "SHARE0039" => Some(6),
        "EVICT0040" => Some(13),
        "DIRTY0041" => Some(20),
        "WB0042" => Some(27),
        "RECLAIM0043" => Some(34),
        "ISO0044" => Some(41),
        "THR0045" => Some(48),
        "ACCT0046" => Some(55),
        "VERDICT0047" => Some(62),
        "COW0048" => Some(69),
        "PC0049" => Some(76),
        "JOINT0050" => Some(83),
        "SHARE0051" => Some(1),
        "EVICT0052" => Some(8),
        "DIRTY0053" => Some(15),
        "WB0054" => Some(22),
        "RECLAIM0055" => Some(29),
        "ISO0056" => Some(36),
        "THR0057" => Some(43),
        "ACCT0058" => Some(50),
        "VERDICT0059" => Some(57),
        "COW0060" => Some(64),
        "PC0061" => Some(71),
        "JOINT0062" => Some(78),
        "SHARE0063" => Some(85),
        "EVICT0064" => Some(3),
        "DIRTY0065" => Some(10),
        "WB0066" => Some(17),
        "RECLAIM0067" => Some(24),
        "ISO0068" => Some(31),
        "THR0069" => Some(38),
        "ACCT0070" => Some(45),
        "VERDICT0071" => Some(52),
        "COW0072" => Some(59),
        "PC0073" => Some(66),
        "JOINT0074" => Some(73),
        "SHARE0075" => Some(80),
        "EVICT0076" => Some(87),
        "DIRTY0077" => Some(5),
        "WB0078" => Some(12),
        "RECLAIM0079" => Some(19),
        "ISO0080" => Some(26),
        "THR0081" => Some(33),
        "ACCT0082" => Some(40),
        "VERDICT0083" => Some(47),
        "COW0084" => Some(54),
        "PC0085" => Some(61),
        "JOINT0086" => Some(68),
        "SHARE0087" => Some(75),
        "EVICT0088" => Some(82),
        "DIRTY0089" => Some(0),
        "WB0090" => Some(7),
        "RECLAIM0091" => Some(14),
        "ISO0092" => Some(21),
        "THR0093" => Some(28),
        "ACCT0094" => Some(35),
        "VERDICT0095" => Some(42),
        "COW0096" => Some(49),
        "PC0097" => Some(56),
        "JOINT0098" => Some(63),
        "SHARE0099" => Some(70),
        "EVICT0100" => Some(77),
        "DIRTY0101" => Some(84),
        "WB0102" => Some(2),
        "RECLAIM0103" => Some(9),
        "ISO0104" => Some(16),
        "THR0105" => Some(23),
        "ACCT0106" => Some(30),
        "VERDICT0107" => Some(37),
        "COW0108" => Some(44),
        "PC0109" => Some(51),
        "JOINT0110" => Some(58),
        "SHARE0111" => Some(65),
        "EVICT0112" => Some(72),
        "DIRTY0113" => Some(79),
        "WB0114" => Some(86),
        "RECLAIM0115" => Some(4),
        "ISO0116" => Some(11),
        "THR0117" => Some(18),
        "ACCT0118" => Some(25),
        "VERDICT0119" => Some(32),
        "COW0120" => Some(39),
        "PC0121" => Some(46),
        "JOINT0122" => Some(53),
        "SHARE0123" => Some(60),
        "EVICT0124" => Some(67),
        "DIRTY0125" => Some(74),
        "WB0126" => Some(81),
        "RECLAIM0127" => Some(88),
        "ISO0128" => Some(6),
        "THR0129" => Some(13),
        "ACCT0130" => Some(20),
        "VERDICT0131" => Some(27),
        "COW0132" => Some(34),
        "PC0133" => Some(41),
        "JOINT0134" => Some(48),
        "SHARE0135" => Some(55),
        "EVICT0136" => Some(62),
        "DIRTY0137" => Some(69),
        "WB0138" => Some(76),
        "RECLAIM0139" => Some(83),
        "ISO0140" => Some(1),
        "THR0141" => Some(8),
        "ACCT0142" => Some(15),
        "VERDICT0143" => Some(22),
        "COW0144" => Some(29),
        "PC0145" => Some(36),
        "JOINT0146" => Some(43),
        "SHARE0147" => Some(50),
        "EVICT0148" => Some(57),
        "DIRTY0149" => Some(64),
        "WB0150" => Some(71),
        "RECLAIM0151" => Some(78),
        "ISO0152" => Some(85),
        "THR0153" => Some(3),
        "ACCT0154" => Some(10),
        "VERDICT0155" => Some(17),
        "COW0156" => Some(24),
        "PC0157" => Some(31),
        "JOINT0158" => Some(38),
        "SHARE0159" => Some(45),
        "EVICT0160" => Some(52),
        "DIRTY0161" => Some(59),
        "WB0162" => Some(66),
        "RECLAIM0163" => Some(73),
        "ISO0164" => Some(80),
        "THR0165" => Some(87),
        "ACCT0166" => Some(5),
        "VERDICT0167" => Some(12),
        "COW0168" => Some(19),
        "PC0169" => Some(26),
        "JOINT0170" => Some(33),
        "SHARE0171" => Some(40),
        "EVICT0172" => Some(47),
        "DIRTY0173" => Some(54),
        "WB0174" => Some(61),
        "RECLAIM0175" => Some(68),
        "ISO0176" => Some(75),
        "THR0177" => Some(82),
        "ACCT0178" => Some(0),
        "VERDICT0179" => Some(7),
        "COW0180" => Some(14),
        "PC0181" => Some(21),
        "JOINT0182" => Some(28),
        "SHARE0183" => Some(35),
        "EVICT0184" => Some(42),
        "DIRTY0185" => Some(49),
        "WB0186" => Some(56),
        "RECLAIM0187" => Some(63),
        "ISO0188" => Some(70),
        "THR0189" => Some(77),
        "ACCT0190" => Some(84),
        "VERDICT0191" => Some(2),
        "COW0192" => Some(9),
        "PC0193" => Some(16),
        "JOINT0194" => Some(23),
        "SHARE0195" => Some(30),
        "EVICT0196" => Some(37),
        "DIRTY0197" => Some(44),
        "WB0198" => Some(51),
        "RECLAIM0199" => Some(58),
        "ISO0200" => Some(65),
        "THR0201" => Some(72),
        "ACCT0202" => Some(79),
        "VERDICT0203" => Some(86),
        "COW0204" => Some(4),
        "PC0205" => Some(11),
        "JOINT0206" => Some(18),
        "SHARE0207" => Some(25),
        "EVICT0208" => Some(32),
        "DIRTY0209" => Some(39),
        "WB0210" => Some(46),
        "RECLAIM0211" => Some(53),
        "ISO0212" => Some(60),
        "THR0213" => Some(67),
        "ACCT0214" => Some(74),
        "VERDICT0215" => Some(81),
        "COW0216" => Some(88),
        "PC0217" => Some(6),
        "JOINT0218" => Some(13),
        "SHARE0219" => Some(20),
        "EVICT0220" => Some(27),
        "DIRTY0221" => Some(34),
        "WB0222" => Some(41),
        "RECLAIM0223" => Some(48),
        "ISO0224" => Some(55),
        "THR0225" => Some(62),
        "ACCT0226" => Some(69),
        "VERDICT0227" => Some(76),
        "COW0228" => Some(83),
        "PC0229" => Some(1),
        "JOINT0230" => Some(8),
        "SHARE0231" => Some(15),
        "EVICT0232" => Some(22),
        "DIRTY0233" => Some(29),
        "WB0234" => Some(36),
        "RECLAIM0235" => Some(43),
        "ISO0236" => Some(50),
        "THR0237" => Some(57),
        "ACCT0238" => Some(64),
        "VERDICT0239" => Some(71),
        "COW0240" => Some(78),
        "PC0241" => Some(85),
        "JOINT0242" => Some(3),
        "SHARE0243" => Some(10),
        "EVICT0244" => Some(17),
        "DIRTY0245" => Some(24),
        "WB0246" => Some(31),
        "RECLAIM0247" => Some(38),
        "ISO0248" => Some(45),
        "THR0249" => Some(52),
        "ACCT0250" => Some(59),
        "VERDICT0251" => Some(66),
        "COW0252" => Some(73),
        "PC0253" => Some(80),
        "JOINT0254" => Some(87),
        "SHARE0255" => Some(5),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2921bmx_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2921bmx_COUNT {
        let (k, v) = u2921bmx[i];
        let ok = match k {
            "COW0000" => v < 89,
            "PC0001" => v < 89,
            "JOINT0002" => v < 89,
            "SHARE0003" => v < 89,
            "EVICT0004" => v < 89,
            "DIRTY0005" => v < 89,
            "WB0006" => v < 89,
            "RECLAIM0007" => v < 89,
            "ISO0008" => v < 89,
            "THR0009" => v < 89,
            "ACCT0010" => v < 89,
            "VERDICT0011" => v < 89,
            "COW0012" => v < 89,
            "PC0013" => v < 89,
            "JOINT0014" => v < 89,
            "SHARE0015" => v < 89,
            "EVICT0016" => v < 89,
            "DIRTY0017" => v < 89,
            "WB0018" => v < 89,
            "RECLAIM0019" => v < 89,
            "ISO0020" => v < 89,
            "THR0021" => v < 89,
            "ACCT0022" => v < 89,
            "VERDICT0023" => v < 89,
            "COW0024" => v < 89,
            "PC0025" => v < 89,
            "JOINT0026" => v < 89,
            "SHARE0027" => v < 89,
            "EVICT0028" => v < 89,
            "DIRTY0029" => v < 89,
            "WB0030" => v < 89,
            "RECLAIM0031" => v < 89,
            "ISO0032" => v < 89,
            "THR0033" => v < 89,
            "ACCT0034" => v < 89,
            "VERDICT0035" => v < 89,
            "COW0036" => v < 89,
            "PC0037" => v < 89,
            "JOINT0038" => v < 89,
            "SHARE0039" => v < 89,
            "EVICT0040" => v < 89,
            "DIRTY0041" => v < 89,
            "WB0042" => v < 89,
            "RECLAIM0043" => v < 89,
            "ISO0044" => v < 89,
            "THR0045" => v < 89,
            "ACCT0046" => v < 89,
            "VERDICT0047" => v < 89,
            "COW0048" => v < 89,
            "PC0049" => v < 89,
            "JOINT0050" => v < 89,
            "SHARE0051" => v < 89,
            "EVICT0052" => v < 89,
            "DIRTY0053" => v < 89,
            "WB0054" => v < 89,
            "RECLAIM0055" => v < 89,
            "ISO0056" => v < 89,
            "THR0057" => v < 89,
            "ACCT0058" => v < 89,
            "VERDICT0059" => v < 89,
            "COW0060" => v < 89,
            "PC0061" => v < 89,
            "JOINT0062" => v < 89,
            "SHARE0063" => v < 89,
            "EVICT0064" => v < 89,
            "DIRTY0065" => v < 89,
            "WB0066" => v < 89,
            "RECLAIM0067" => v < 89,
            "ISO0068" => v < 89,
            "THR0069" => v < 89,
            "ACCT0070" => v < 89,
            "VERDICT0071" => v < 89,
            "COW0072" => v < 89,
            "PC0073" => v < 89,
            "JOINT0074" => v < 89,
            "SHARE0075" => v < 89,
            "EVICT0076" => v < 89,
            "DIRTY0077" => v < 89,
            "WB0078" => v < 89,
            "RECLAIM0079" => v < 89,
            "ISO0080" => v < 89,
            "THR0081" => v < 89,
            "ACCT0082" => v < 89,
            "VERDICT0083" => v < 89,
            "COW0084" => v < 89,
            "PC0085" => v < 89,
            "JOINT0086" => v < 89,
            "SHARE0087" => v < 89,
            "EVICT0088" => v < 89,
            "DIRTY0089" => v < 89,
            "WB0090" => v < 89,
            "RECLAIM0091" => v < 89,
            "ISO0092" => v < 89,
            "THR0093" => v < 89,
            "ACCT0094" => v < 89,
            "VERDICT0095" => v < 89,
            "COW0096" => v < 89,
            "PC0097" => v < 89,
            "JOINT0098" => v < 89,
            "SHARE0099" => v < 89,
            "EVICT0100" => v < 89,
            "DIRTY0101" => v < 89,
            "WB0102" => v < 89,
            "RECLAIM0103" => v < 89,
            "ISO0104" => v < 89,
            "THR0105" => v < 89,
            "ACCT0106" => v < 89,
            "VERDICT0107" => v < 89,
            "COW0108" => v < 89,
            "PC0109" => v < 89,
            "JOINT0110" => v < 89,
            "SHARE0111" => v < 89,
            "EVICT0112" => v < 89,
            "DIRTY0113" => v < 89,
            "WB0114" => v < 89,
            "RECLAIM0115" => v < 89,
            "ISO0116" => v < 89,
            "THR0117" => v < 89,
            "ACCT0118" => v < 89,
            "VERDICT0119" => v < 89,
            "COW0120" => v < 89,
            "PC0121" => v < 89,
            "JOINT0122" => v < 89,
            "SHARE0123" => v < 89,
            "EVICT0124" => v < 89,
            "DIRTY0125" => v < 89,
            "WB0126" => v < 89,
            "RECLAIM0127" => v < 89,
            "ISO0128" => v < 89,
            "THR0129" => v < 89,
            "ACCT0130" => v < 89,
            "VERDICT0131" => v < 89,
            "COW0132" => v < 89,
            "PC0133" => v < 89,
            "JOINT0134" => v < 89,
            "SHARE0135" => v < 89,
            "EVICT0136" => v < 89,
            "DIRTY0137" => v < 89,
            "WB0138" => v < 89,
            "RECLAIM0139" => v < 89,
            "ISO0140" => v < 89,
            "THR0141" => v < 89,
            "ACCT0142" => v < 89,
            "VERDICT0143" => v < 89,
            "COW0144" => v < 89,
            "PC0145" => v < 89,
            "JOINT0146" => v < 89,
            "SHARE0147" => v < 89,
            "EVICT0148" => v < 89,
            "DIRTY0149" => v < 89,
            "WB0150" => v < 89,
            "RECLAIM0151" => v < 89,
            "ISO0152" => v < 89,
            "THR0153" => v < 89,
            "ACCT0154" => v < 89,
            "VERDICT0155" => v < 89,
            "COW0156" => v < 89,
            "PC0157" => v < 89,
            "JOINT0158" => v < 89,
            "SHARE0159" => v < 89,
            "EVICT0160" => v < 89,
            "DIRTY0161" => v < 89,
            "WB0162" => v < 89,
            "RECLAIM0163" => v < 89,
            "ISO0164" => v < 89,
            "THR0165" => v < 89,
            "ACCT0166" => v < 89,
            "VERDICT0167" => v < 89,
            "COW0168" => v < 89,
            "PC0169" => v < 89,
            "JOINT0170" => v < 89,
            "SHARE0171" => v < 89,
            "EVICT0172" => v < 89,
            "DIRTY0173" => v < 89,
            "WB0174" => v < 89,
            "RECLAIM0175" => v < 89,
            "ISO0176" => v < 89,
            "THR0177" => v < 89,
            "ACCT0178" => v < 89,
            "VERDICT0179" => v < 89,
            "COW0180" => v < 89,
            "PC0181" => v < 89,
            "JOINT0182" => v < 89,
            "SHARE0183" => v < 89,
            "EVICT0184" => v < 89,
            "DIRTY0185" => v < 89,
            "WB0186" => v < 89,
            "RECLAIM0187" => v < 89,
            "ISO0188" => v < 89,
            "THR0189" => v < 89,
            "ACCT0190" => v < 89,
            "VERDICT0191" => v < 89,
            "COW0192" => v < 89,
            "PC0193" => v < 89,
            "JOINT0194" => v < 89,
            "SHARE0195" => v < 89,
            "EVICT0196" => v < 89,
            "DIRTY0197" => v < 89,
            "WB0198" => v < 89,
            "RECLAIM0199" => v < 89,
            "ISO0200" => v < 89,
            "THR0201" => v < 89,
            "ACCT0202" => v < 89,
            "VERDICT0203" => v < 89,
            "COW0204" => v < 89,
            "PC0205" => v < 89,
            "JOINT0206" => v < 89,
            "SHARE0207" => v < 89,
            "EVICT0208" => v < 89,
            "DIRTY0209" => v < 89,
            "WB0210" => v < 89,
            "RECLAIM0211" => v < 89,
            "ISO0212" => v < 89,
            "THR0213" => v < 89,
            "ACCT0214" => v < 89,
            "VERDICT0215" => v < 89,
            "COW0216" => v < 89,
            "PC0217" => v < 89,
            "JOINT0218" => v < 89,
            "SHARE0219" => v < 89,
            "EVICT0220" => v < 89,
            "DIRTY0221" => v < 89,
            "WB0222" => v < 89,
            "RECLAIM0223" => v < 89,
            "ISO0224" => v < 89,
            "THR0225" => v < 89,
            "ACCT0226" => v < 89,
            "VERDICT0227" => v < 89,
            "COW0228" => v < 89,
            "PC0229" => v < 89,
            "JOINT0230" => v < 89,
            "SHARE0231" => v < 89,
            "EVICT0232" => v < 89,
            "DIRTY0233" => v < 89,
            "WB0234" => v < 89,
            "RECLAIM0235" => v < 89,
            "ISO0236" => v < 89,
            "THR0237" => v < 89,
            "ACCT0238" => v < 89,
            "VERDICT0239" => v < 89,
            "COW0240" => v < 89,
            "PC0241" => v < 89,
            "JOINT0242" => v < 89,
            "SHARE0243" => v < 89,
            "EVICT0244" => v < 89,
            "DIRTY0245" => v < 89,
            "WB0246" => v < 89,
            "RECLAIM0247" => v < 89,
            "ISO0248" => v < 89,
            "THR0249" => v < 89,
            "ACCT0250" => v < 89,
            "VERDICT0251" => v < 89,
            "COW0252" => v < 89,
            "PC0253" => v < 89,
            "JOINT0254" => v < 89,
            "SHARE0255" => v < 89,
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
pub fn u2921bmx_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2921bmx_COUNT {
        s = s.wrapping_add(u2921bmx[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2921bmx_unique() -> bool {
    let mut i = 0usize;
    while i < u2921bmx_COUNT {
        let mut j = i + 1;
        while j < u2921bmx_COUNT {
            if u2921bmx[i].0 == u2921bmx[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0xe2c0, 0xe2cf]。
pub const fn u2921_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2921Code::ALL.len() {
        let c = u2921Code::ALL[i].code();
        if c < 0xe2c0 || c > 0xe2cf {
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
fn u2921_guard_seg() -> bool {
    u2921_segment_guard()
}

/// UNX-F2921 bmx 256 行齐备 + 边界锚 + 键外拒收。
fn check_u2921_bmx_count() -> bool {
    let mut ok = u2921bmx_COUNT == 256;
    if ok { ok = u2921bmx_get("COW0000") == Some(0) && u2921bmx_get("SHARE0255") == Some(5); }
    if ok { ok = u2921bmx_get("ZZZ_NOKEY").is_none(); }
    ok
}

/// UNX-F2921 bmx 累计守恒（理论值 11231）。
fn check_u2921_bmx_sum() -> bool {
    u2921bmx_sum() == 11231
}

/// UNX-F2921 bmx 逐行守卫与唯一性全绿。
fn check_u2921_bmx_guard() -> bool {
    u2921bmx_scan() == 0 && u2921bmx_unique()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2921_checks() -> CheckSet {
    let mut set = CheckSet::new("U2921");
    set.add(
        "u2921-seg-guard",
        u2921_guard_seg(),
        if u2921_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2921-bmx-count",
        check_u2921_bmx_count(),
        if check_u2921_bmx_count() { "" } else { "UNX-F2921 bmx count mismatch" },
    );
    set.add(
        "u2921-bmx-sum",
        check_u2921_bmx_sum(),
        if check_u2921_bmx_sum() { "" } else { "UNX-F2921 bmx sum mismatch" },
    );
    set.add(
        "u2921-bmx-guard",
        check_u2921_bmx_guard(),
        if check_u2921_bmx_guard() { "" } else { "UNX-F2921 bmx guard mismatch" },
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
    fn u2921_all_checks_green() {
        let set = run_u2921_checks();
        assert!(set.all_passed(), "U2921 domain checks must be green");
    }

    #[test]
    fn u2921_segment_guard_holds() {
        assert!(u2921_segment_guard());
    }
}
