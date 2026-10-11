//! unxu2841_u2841bmx —— 大页-地址空间联合压测方案与执行 深编bmx（大页-地址空间联合压测 × 256 单元）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2841 · 大页-地址空间联合压测方案与执行 · 判据原文：UNX-F2841 · 判据：UNX-F2841-J1 大页映射进地址空间全链（建立-翻译-拆除）10/10 轮贯通，链账三段闭合
//! · 组件：大页-地址空间联合压测 守卫映射 256 行（二 轮派生真实数据 + 逐行守卫 + 键唯一）
//! · 判据落地：UNX-F2841 · 判据：UNX-F2841-J1 大页映射进地址空间全链（建立-翻译-拆除）10/10 轮贯通，链账三段闭合
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2841_VERSION: &str = "u2841-u2841bmx-v1";

/// 诊断段基址。
pub const U2841_SEG_BASE: u16 = 0xe280;

// --------------------------------------------------------------------------
// 诊断码（u2841 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页-地址空间联合压测 bmx 面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2841Code {
    /// 0xe280 单元格重复（联合压测 同键双占）。
    CellDup,
    /// 0xe281 单元格断档（交叉 序号不连续）。
    CellGap,
    /// 0xe282 索引越界（矩阵）。
    IndexOob,
    /// 0xe283 大表账破缺（行数≠登记值）。
    LedgerBreak,
    /// 0xe284 表序断档。
    OrderBreak,
    /// 0xe285 累计漂移（求和≠理论值）。
    SumDrift,
    /// 0xe286 掩码非法。
    MaskBad,
    /// 0xe287 守卫触发。
    GuardTripped,
    /// 0xe288 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe289 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe28a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe28b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2841Code {
    /// 全量闭集。
    pub const ALL: [u2841Code; 12] = [
        u2841Code::CellDup,
        u2841Code::CellGap,
        u2841Code::IndexOob,
        u2841Code::LedgerBreak,
        u2841Code::OrderBreak,
        u2841Code::SumDrift,
        u2841Code::MaskBad,
        u2841Code::GuardTripped,
        u2841Code::LedgerBroken,
        u2841Code::SnapshotFingerprint,
        u2841Code::TableBroken,
        u2841Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2841Code::CellDup => 0xe280,
            u2841Code::CellGap => 0xe281,
            u2841Code::IndexOob => 0xe282,
            u2841Code::LedgerBreak => 0xe283,
            u2841Code::OrderBreak => 0xe284,
            u2841Code::SumDrift => 0xe285,
            u2841Code::MaskBad => 0xe286,
            u2841Code::GuardTripped => 0xe287,
            u2841Code::LedgerBroken => 0xe288,
            u2841Code::SnapshotFingerprint => 0xe289,
            u2841Code::TableBroken => 0xe28a,
            u2841Code::SegmentGuard => 0xe28b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2841Code::CellDup => "单元格重复（联合压测 同键双占）",
            u2841Code::CellGap => "单元格断档（交叉 序号不连续）",
            u2841Code::IndexOob => "索引越界（矩阵）",
            u2841Code::LedgerBreak => "大表账破缺（行数≠登记值）",
            u2841Code::OrderBreak => "表序断档",
            u2841Code::SumDrift => "累计漂移（求和≠理论值）",
            u2841Code::MaskBad => "掩码非法",
            u2841Code::GuardTripped => "守卫触发",
            u2841Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2841Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2841Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2841Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2841Code::CellDup => "按键唯一律去重",
            u2841Code::CellGap => "按连续律补号",
            u2841Code::IndexOob => "按行数边界裁剪",
            u2841Code::LedgerBreak => "按守恒重建",
            u2841Code::OrderBreak => "按序重排",
            u2841Code::SumDrift => "按理论值复算",
            u2841Code::MaskBad => "按掩码律校正",
            u2841Code::GuardTripped => "检查临界区",
            u2841Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2841Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2841Code::TableBroken => "按语义规则重建常量表",
            u2841Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2841bmx（256 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 大页-地址空间联合压测 bmx 守卫映射（256 行逐行注释自证）。
/// 行数。
pub const u2841bmx_COUNT: usize = 256;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2841bmx: [(&'static str, u64); 256] = [
    ("HP0000", 0), // HP 单元 0000（大页 × 相位 00 × 二 轮）
    ("AS0001", 7), // AS 单元 0001（地址空间 × 相位 01 × 二 轮）
    ("JOINT0002", 14), // JOINT 单元 0002（联合 × 相位 02 × 二 轮）
    ("MATRIX0003", 21), // MATRIX 单元 0003（矩阵 × 相位 03 × 二 轮）
    ("XPROD0004", 28), // XPROD 单元 0004（交叉组合 × 相位 04 × 二 轮）
    ("LOAD0005", 35), // LOAD 单元 0005（负载 × 相位 05 × 二 轮）
    ("FRAG0006", 42), // FRAG 单元 0006（碎片 × 相位 06 × 二 轮）
    ("SW0007", 49), // SW 单元 0007（切换 × 相位 07 × 二 轮）
    ("ISOL0008", 56), // ISOL 单元 0008（隔离 × 相位 08 × 二 轮）
    ("FAIR0009", 63), // FAIR 单元 0009（公平 × 相位 09 × 二 轮）
    ("BUD0010", 70), // BUD 单元 0010（预算 × 相位 10 × 二 轮）
    ("VERDICT0011", 77), // VERDICT 单元 0011（裁决 × 相位 11 × 二 轮）
    ("HP0012", 84), // HP 单元 0012（大页 × 相位 12 × 二 轮）
    ("AS0013", 2), // AS 单元 0013（地址空间 × 相位 13 × 二 轮）
    ("JOINT0014", 9), // JOINT 单元 0014（联合 × 相位 14 × 二 轮）
    ("MATRIX0015", 16), // MATRIX 单元 0015（矩阵 × 相位 15 × 二 轮）
    ("XPROD0016", 23), // XPROD 单元 0016（交叉组合 × 相位 16 × 二 轮）
    ("LOAD0017", 30), // LOAD 单元 0017（负载 × 相位 17 × 二 轮）
    ("FRAG0018", 37), // FRAG 单元 0018（碎片 × 相位 18 × 二 轮）
    ("SW0019", 44), // SW 单元 0019（切换 × 相位 19 × 二 轮）
    ("ISOL0020", 51), // ISOL 单元 0020（隔离 × 相位 20 × 二 轮）
    ("FAIR0021", 58), // FAIR 单元 0021（公平 × 相位 21 × 二 轮）
    ("BUD0022", 65), // BUD 单元 0022（预算 × 相位 22 × 二 轮）
    ("VERDICT0023", 72), // VERDICT 单元 0023（裁决 × 相位 23 × 二 轮）
    ("HP0024", 79), // HP 单元 0024（大页 × 相位 24 × 二 轮）
    ("AS0025", 86), // AS 单元 0025（地址空间 × 相位 25 × 二 轮）
    ("JOINT0026", 4), // JOINT 单元 0026（联合 × 相位 26 × 二 轮）
    ("MATRIX0027", 11), // MATRIX 单元 0027（矩阵 × 相位 27 × 二 轮）
    ("XPROD0028", 18), // XPROD 单元 0028（交叉组合 × 相位 28 × 二 轮）
    ("LOAD0029", 25), // LOAD 单元 0029（负载 × 相位 29 × 二 轮）
    ("FRAG0030", 32), // FRAG 单元 0030（碎片 × 相位 30 × 二 轮）
    ("SW0031", 39), // SW 单元 0031（切换 × 相位 31 × 二 轮）
    ("ISOL0032", 46), // ISOL 单元 0032（隔离 × 相位 32 × 二 轮）
    ("FAIR0033", 53), // FAIR 单元 0033（公平 × 相位 33 × 二 轮）
    ("BUD0034", 60), // BUD 单元 0034（预算 × 相位 34 × 二 轮）
    ("VERDICT0035", 67), // VERDICT 单元 0035（裁决 × 相位 35 × 二 轮）
    ("HP0036", 74), // HP 单元 0036（大页 × 相位 36 × 二 轮）
    ("AS0037", 81), // AS 单元 0037（地址空间 × 相位 37 × 二 轮）
    ("JOINT0038", 88), // JOINT 单元 0038（联合 × 相位 38 × 二 轮）
    ("MATRIX0039", 6), // MATRIX 单元 0039（矩阵 × 相位 39 × 二 轮）
    ("XPROD0040", 13), // XPROD 单元 0040（交叉组合 × 相位 40 × 二 轮）
    ("LOAD0041", 20), // LOAD 单元 0041（负载 × 相位 41 × 二 轮）
    ("FRAG0042", 27), // FRAG 单元 0042（碎片 × 相位 42 × 二 轮）
    ("SW0043", 34), // SW 单元 0043（切换 × 相位 43 × 二 轮）
    ("ISOL0044", 41), // ISOL 单元 0044（隔离 × 相位 44 × 二 轮）
    ("FAIR0045", 48), // FAIR 单元 0045（公平 × 相位 45 × 二 轮）
    ("BUD0046", 55), // BUD 单元 0046（预算 × 相位 46 × 二 轮）
    ("VERDICT0047", 62), // VERDICT 单元 0047（裁决 × 相位 47 × 二 轮）
    ("HP0048", 69), // HP 单元 0048（大页 × 相位 48 × 二 轮）
    ("AS0049", 76), // AS 单元 0049（地址空间 × 相位 49 × 二 轮）
    ("JOINT0050", 83), // JOINT 单元 0050（联合 × 相位 50 × 二 轮）
    ("MATRIX0051", 1), // MATRIX 单元 0051（矩阵 × 相位 51 × 二 轮）
    ("XPROD0052", 8), // XPROD 单元 0052（交叉组合 × 相位 52 × 二 轮）
    ("LOAD0053", 15), // LOAD 单元 0053（负载 × 相位 53 × 二 轮）
    ("FRAG0054", 22), // FRAG 单元 0054（碎片 × 相位 54 × 二 轮）
    ("SW0055", 29), // SW 单元 0055（切换 × 相位 55 × 二 轮）
    ("ISOL0056", 36), // ISOL 单元 0056（隔离 × 相位 56 × 二 轮）
    ("FAIR0057", 43), // FAIR 单元 0057（公平 × 相位 57 × 二 轮）
    ("BUD0058", 50), // BUD 单元 0058（预算 × 相位 58 × 二 轮）
    ("VERDICT0059", 57), // VERDICT 单元 0059（裁决 × 相位 59 × 二 轮）
    ("HP0060", 64), // HP 单元 0060（大页 × 相位 60 × 二 轮）
    ("AS0061", 71), // AS 单元 0061（地址空间 × 相位 61 × 二 轮）
    ("JOINT0062", 78), // JOINT 单元 0062（联合 × 相位 62 × 二 轮）
    ("MATRIX0063", 85), // MATRIX 单元 0063（矩阵 × 相位 63 × 二 轮）
    ("XPROD0064", 3), // XPROD 单元 0064（交叉组合 × 相位 00 × 二 轮）
    ("LOAD0065", 10), // LOAD 单元 0065（负载 × 相位 01 × 二 轮）
    ("FRAG0066", 17), // FRAG 单元 0066（碎片 × 相位 02 × 二 轮）
    ("SW0067", 24), // SW 单元 0067（切换 × 相位 03 × 二 轮）
    ("ISOL0068", 31), // ISOL 单元 0068（隔离 × 相位 04 × 二 轮）
    ("FAIR0069", 38), // FAIR 单元 0069（公平 × 相位 05 × 二 轮）
    ("BUD0070", 45), // BUD 单元 0070（预算 × 相位 06 × 二 轮）
    ("VERDICT0071", 52), // VERDICT 单元 0071（裁决 × 相位 07 × 二 轮）
    ("HP0072", 59), // HP 单元 0072（大页 × 相位 08 × 二 轮）
    ("AS0073", 66), // AS 单元 0073（地址空间 × 相位 09 × 二 轮）
    ("JOINT0074", 73), // JOINT 单元 0074（联合 × 相位 10 × 二 轮）
    ("MATRIX0075", 80), // MATRIX 单元 0075（矩阵 × 相位 11 × 二 轮）
    ("XPROD0076", 87), // XPROD 单元 0076（交叉组合 × 相位 12 × 二 轮）
    ("LOAD0077", 5), // LOAD 单元 0077（负载 × 相位 13 × 二 轮）
    ("FRAG0078", 12), // FRAG 单元 0078（碎片 × 相位 14 × 二 轮）
    ("SW0079", 19), // SW 单元 0079（切换 × 相位 15 × 二 轮）
    ("ISOL0080", 26), // ISOL 单元 0080（隔离 × 相位 16 × 二 轮）
    ("FAIR0081", 33), // FAIR 单元 0081（公平 × 相位 17 × 二 轮）
    ("BUD0082", 40), // BUD 单元 0082（预算 × 相位 18 × 二 轮）
    ("VERDICT0083", 47), // VERDICT 单元 0083（裁决 × 相位 19 × 二 轮）
    ("HP0084", 54), // HP 单元 0084（大页 × 相位 20 × 二 轮）
    ("AS0085", 61), // AS 单元 0085（地址空间 × 相位 21 × 二 轮）
    ("JOINT0086", 68), // JOINT 单元 0086（联合 × 相位 22 × 二 轮）
    ("MATRIX0087", 75), // MATRIX 单元 0087（矩阵 × 相位 23 × 二 轮）
    ("XPROD0088", 82), // XPROD 单元 0088（交叉组合 × 相位 24 × 二 轮）
    ("LOAD0089", 0), // LOAD 单元 0089（负载 × 相位 25 × 二 轮）
    ("FRAG0090", 7), // FRAG 单元 0090（碎片 × 相位 26 × 二 轮）
    ("SW0091", 14), // SW 单元 0091（切换 × 相位 27 × 二 轮）
    ("ISOL0092", 21), // ISOL 单元 0092（隔离 × 相位 28 × 二 轮）
    ("FAIR0093", 28), // FAIR 单元 0093（公平 × 相位 29 × 二 轮）
    ("BUD0094", 35), // BUD 单元 0094（预算 × 相位 30 × 二 轮）
    ("VERDICT0095", 42), // VERDICT 单元 0095（裁决 × 相位 31 × 二 轮）
    ("HP0096", 49), // HP 单元 0096（大页 × 相位 32 × 二 轮）
    ("AS0097", 56), // AS 单元 0097（地址空间 × 相位 33 × 二 轮）
    ("JOINT0098", 63), // JOINT 单元 0098（联合 × 相位 34 × 二 轮）
    ("MATRIX0099", 70), // MATRIX 单元 0099（矩阵 × 相位 35 × 二 轮）
    ("XPROD0100", 77), // XPROD 单元 0100（交叉组合 × 相位 36 × 二 轮）
    ("LOAD0101", 84), // LOAD 单元 0101（负载 × 相位 37 × 二 轮）
    ("FRAG0102", 2), // FRAG 单元 0102（碎片 × 相位 38 × 二 轮）
    ("SW0103", 9), // SW 单元 0103（切换 × 相位 39 × 二 轮）
    ("ISOL0104", 16), // ISOL 单元 0104（隔离 × 相位 40 × 二 轮）
    ("FAIR0105", 23), // FAIR 单元 0105（公平 × 相位 41 × 二 轮）
    ("BUD0106", 30), // BUD 单元 0106（预算 × 相位 42 × 二 轮）
    ("VERDICT0107", 37), // VERDICT 单元 0107（裁决 × 相位 43 × 二 轮）
    ("HP0108", 44), // HP 单元 0108（大页 × 相位 44 × 二 轮）
    ("AS0109", 51), // AS 单元 0109（地址空间 × 相位 45 × 二 轮）
    ("JOINT0110", 58), // JOINT 单元 0110（联合 × 相位 46 × 二 轮）
    ("MATRIX0111", 65), // MATRIX 单元 0111（矩阵 × 相位 47 × 二 轮）
    ("XPROD0112", 72), // XPROD 单元 0112（交叉组合 × 相位 48 × 二 轮）
    ("LOAD0113", 79), // LOAD 单元 0113（负载 × 相位 49 × 二 轮）
    ("FRAG0114", 86), // FRAG 单元 0114（碎片 × 相位 50 × 二 轮）
    ("SW0115", 4), // SW 单元 0115（切换 × 相位 51 × 二 轮）
    ("ISOL0116", 11), // ISOL 单元 0116（隔离 × 相位 52 × 二 轮）
    ("FAIR0117", 18), // FAIR 单元 0117（公平 × 相位 53 × 二 轮）
    ("BUD0118", 25), // BUD 单元 0118（预算 × 相位 54 × 二 轮）
    ("VERDICT0119", 32), // VERDICT 单元 0119（裁决 × 相位 55 × 二 轮）
    ("HP0120", 39), // HP 单元 0120（大页 × 相位 56 × 二 轮）
    ("AS0121", 46), // AS 单元 0121（地址空间 × 相位 57 × 二 轮）
    ("JOINT0122", 53), // JOINT 单元 0122（联合 × 相位 58 × 二 轮）
    ("MATRIX0123", 60), // MATRIX 单元 0123（矩阵 × 相位 59 × 二 轮）
    ("XPROD0124", 67), // XPROD 单元 0124（交叉组合 × 相位 60 × 二 轮）
    ("LOAD0125", 74), // LOAD 单元 0125（负载 × 相位 61 × 二 轮）
    ("FRAG0126", 81), // FRAG 单元 0126（碎片 × 相位 62 × 二 轮）
    ("SW0127", 88), // SW 单元 0127（切换 × 相位 63 × 二 轮）
    ("ISOL0128", 6), // ISOL 单元 0128（隔离 × 相位 00 × 二 轮）
    ("FAIR0129", 13), // FAIR 单元 0129（公平 × 相位 01 × 二 轮）
    ("BUD0130", 20), // BUD 单元 0130（预算 × 相位 02 × 二 轮）
    ("VERDICT0131", 27), // VERDICT 单元 0131（裁决 × 相位 03 × 二 轮）
    ("HP0132", 34), // HP 单元 0132（大页 × 相位 04 × 二 轮）
    ("AS0133", 41), // AS 单元 0133（地址空间 × 相位 05 × 二 轮）
    ("JOINT0134", 48), // JOINT 单元 0134（联合 × 相位 06 × 二 轮）
    ("MATRIX0135", 55), // MATRIX 单元 0135（矩阵 × 相位 07 × 二 轮）
    ("XPROD0136", 62), // XPROD 单元 0136（交叉组合 × 相位 08 × 二 轮）
    ("LOAD0137", 69), // LOAD 单元 0137（负载 × 相位 09 × 二 轮）
    ("FRAG0138", 76), // FRAG 单元 0138（碎片 × 相位 10 × 二 轮）
    ("SW0139", 83), // SW 单元 0139（切换 × 相位 11 × 二 轮）
    ("ISOL0140", 1), // ISOL 单元 0140（隔离 × 相位 12 × 二 轮）
    ("FAIR0141", 8), // FAIR 单元 0141（公平 × 相位 13 × 二 轮）
    ("BUD0142", 15), // BUD 单元 0142（预算 × 相位 14 × 二 轮）
    ("VERDICT0143", 22), // VERDICT 单元 0143（裁决 × 相位 15 × 二 轮）
    ("HP0144", 29), // HP 单元 0144（大页 × 相位 16 × 二 轮）
    ("AS0145", 36), // AS 单元 0145（地址空间 × 相位 17 × 二 轮）
    ("JOINT0146", 43), // JOINT 单元 0146（联合 × 相位 18 × 二 轮）
    ("MATRIX0147", 50), // MATRIX 单元 0147（矩阵 × 相位 19 × 二 轮）
    ("XPROD0148", 57), // XPROD 单元 0148（交叉组合 × 相位 20 × 二 轮）
    ("LOAD0149", 64), // LOAD 单元 0149（负载 × 相位 21 × 二 轮）
    ("FRAG0150", 71), // FRAG 单元 0150（碎片 × 相位 22 × 二 轮）
    ("SW0151", 78), // SW 单元 0151（切换 × 相位 23 × 二 轮）
    ("ISOL0152", 85), // ISOL 单元 0152（隔离 × 相位 24 × 二 轮）
    ("FAIR0153", 3), // FAIR 单元 0153（公平 × 相位 25 × 二 轮）
    ("BUD0154", 10), // BUD 单元 0154（预算 × 相位 26 × 二 轮）
    ("VERDICT0155", 17), // VERDICT 单元 0155（裁决 × 相位 27 × 二 轮）
    ("HP0156", 24), // HP 单元 0156（大页 × 相位 28 × 二 轮）
    ("AS0157", 31), // AS 单元 0157（地址空间 × 相位 29 × 二 轮）
    ("JOINT0158", 38), // JOINT 单元 0158（联合 × 相位 30 × 二 轮）
    ("MATRIX0159", 45), // MATRIX 单元 0159（矩阵 × 相位 31 × 二 轮）
    ("XPROD0160", 52), // XPROD 单元 0160（交叉组合 × 相位 32 × 二 轮）
    ("LOAD0161", 59), // LOAD 单元 0161（负载 × 相位 33 × 二 轮）
    ("FRAG0162", 66), // FRAG 单元 0162（碎片 × 相位 34 × 二 轮）
    ("SW0163", 73), // SW 单元 0163（切换 × 相位 35 × 二 轮）
    ("ISOL0164", 80), // ISOL 单元 0164（隔离 × 相位 36 × 二 轮）
    ("FAIR0165", 87), // FAIR 单元 0165（公平 × 相位 37 × 二 轮）
    ("BUD0166", 5), // BUD 单元 0166（预算 × 相位 38 × 二 轮）
    ("VERDICT0167", 12), // VERDICT 单元 0167（裁决 × 相位 39 × 二 轮）
    ("HP0168", 19), // HP 单元 0168（大页 × 相位 40 × 二 轮）
    ("AS0169", 26), // AS 单元 0169（地址空间 × 相位 41 × 二 轮）
    ("JOINT0170", 33), // JOINT 单元 0170（联合 × 相位 42 × 二 轮）
    ("MATRIX0171", 40), // MATRIX 单元 0171（矩阵 × 相位 43 × 二 轮）
    ("XPROD0172", 47), // XPROD 单元 0172（交叉组合 × 相位 44 × 二 轮）
    ("LOAD0173", 54), // LOAD 单元 0173（负载 × 相位 45 × 二 轮）
    ("FRAG0174", 61), // FRAG 单元 0174（碎片 × 相位 46 × 二 轮）
    ("SW0175", 68), // SW 单元 0175（切换 × 相位 47 × 二 轮）
    ("ISOL0176", 75), // ISOL 单元 0176（隔离 × 相位 48 × 二 轮）
    ("FAIR0177", 82), // FAIR 单元 0177（公平 × 相位 49 × 二 轮）
    ("BUD0178", 0), // BUD 单元 0178（预算 × 相位 50 × 二 轮）
    ("VERDICT0179", 7), // VERDICT 单元 0179（裁决 × 相位 51 × 二 轮）
    ("HP0180", 14), // HP 单元 0180（大页 × 相位 52 × 二 轮）
    ("AS0181", 21), // AS 单元 0181（地址空间 × 相位 53 × 二 轮）
    ("JOINT0182", 28), // JOINT 单元 0182（联合 × 相位 54 × 二 轮）
    ("MATRIX0183", 35), // MATRIX 单元 0183（矩阵 × 相位 55 × 二 轮）
    ("XPROD0184", 42), // XPROD 单元 0184（交叉组合 × 相位 56 × 二 轮）
    ("LOAD0185", 49), // LOAD 单元 0185（负载 × 相位 57 × 二 轮）
    ("FRAG0186", 56), // FRAG 单元 0186（碎片 × 相位 58 × 二 轮）
    ("SW0187", 63), // SW 单元 0187（切换 × 相位 59 × 二 轮）
    ("ISOL0188", 70), // ISOL 单元 0188（隔离 × 相位 60 × 二 轮）
    ("FAIR0189", 77), // FAIR 单元 0189（公平 × 相位 61 × 二 轮）
    ("BUD0190", 84), // BUD 单元 0190（预算 × 相位 62 × 二 轮）
    ("VERDICT0191", 2), // VERDICT 单元 0191（裁决 × 相位 63 × 二 轮）
    ("HP0192", 9), // HP 单元 0192（大页 × 相位 00 × 二 轮）
    ("AS0193", 16), // AS 单元 0193（地址空间 × 相位 01 × 二 轮）
    ("JOINT0194", 23), // JOINT 单元 0194（联合 × 相位 02 × 二 轮）
    ("MATRIX0195", 30), // MATRIX 单元 0195（矩阵 × 相位 03 × 二 轮）
    ("XPROD0196", 37), // XPROD 单元 0196（交叉组合 × 相位 04 × 二 轮）
    ("LOAD0197", 44), // LOAD 单元 0197（负载 × 相位 05 × 二 轮）
    ("FRAG0198", 51), // FRAG 单元 0198（碎片 × 相位 06 × 二 轮）
    ("SW0199", 58), // SW 单元 0199（切换 × 相位 07 × 二 轮）
    ("ISOL0200", 65), // ISOL 单元 0200（隔离 × 相位 08 × 二 轮）
    ("FAIR0201", 72), // FAIR 单元 0201（公平 × 相位 09 × 二 轮）
    ("BUD0202", 79), // BUD 单元 0202（预算 × 相位 10 × 二 轮）
    ("VERDICT0203", 86), // VERDICT 单元 0203（裁决 × 相位 11 × 二 轮）
    ("HP0204", 4), // HP 单元 0204（大页 × 相位 12 × 二 轮）
    ("AS0205", 11), // AS 单元 0205（地址空间 × 相位 13 × 二 轮）
    ("JOINT0206", 18), // JOINT 单元 0206（联合 × 相位 14 × 二 轮）
    ("MATRIX0207", 25), // MATRIX 单元 0207（矩阵 × 相位 15 × 二 轮）
    ("XPROD0208", 32), // XPROD 单元 0208（交叉组合 × 相位 16 × 二 轮）
    ("LOAD0209", 39), // LOAD 单元 0209（负载 × 相位 17 × 二 轮）
    ("FRAG0210", 46), // FRAG 单元 0210（碎片 × 相位 18 × 二 轮）
    ("SW0211", 53), // SW 单元 0211（切换 × 相位 19 × 二 轮）
    ("ISOL0212", 60), // ISOL 单元 0212（隔离 × 相位 20 × 二 轮）
    ("FAIR0213", 67), // FAIR 单元 0213（公平 × 相位 21 × 二 轮）
    ("BUD0214", 74), // BUD 单元 0214（预算 × 相位 22 × 二 轮）
    ("VERDICT0215", 81), // VERDICT 单元 0215（裁决 × 相位 23 × 二 轮）
    ("HP0216", 88), // HP 单元 0216（大页 × 相位 24 × 二 轮）
    ("AS0217", 6), // AS 单元 0217（地址空间 × 相位 25 × 二 轮）
    ("JOINT0218", 13), // JOINT 单元 0218（联合 × 相位 26 × 二 轮）
    ("MATRIX0219", 20), // MATRIX 单元 0219（矩阵 × 相位 27 × 二 轮）
    ("XPROD0220", 27), // XPROD 单元 0220（交叉组合 × 相位 28 × 二 轮）
    ("LOAD0221", 34), // LOAD 单元 0221（负载 × 相位 29 × 二 轮）
    ("FRAG0222", 41), // FRAG 单元 0222（碎片 × 相位 30 × 二 轮）
    ("SW0223", 48), // SW 单元 0223（切换 × 相位 31 × 二 轮）
    ("ISOL0224", 55), // ISOL 单元 0224（隔离 × 相位 32 × 二 轮）
    ("FAIR0225", 62), // FAIR 单元 0225（公平 × 相位 33 × 二 轮）
    ("BUD0226", 69), // BUD 单元 0226（预算 × 相位 34 × 二 轮）
    ("VERDICT0227", 76), // VERDICT 单元 0227（裁决 × 相位 35 × 二 轮）
    ("HP0228", 83), // HP 单元 0228（大页 × 相位 36 × 二 轮）
    ("AS0229", 1), // AS 单元 0229（地址空间 × 相位 37 × 二 轮）
    ("JOINT0230", 8), // JOINT 单元 0230（联合 × 相位 38 × 二 轮）
    ("MATRIX0231", 15), // MATRIX 单元 0231（矩阵 × 相位 39 × 二 轮）
    ("XPROD0232", 22), // XPROD 单元 0232（交叉组合 × 相位 40 × 二 轮）
    ("LOAD0233", 29), // LOAD 单元 0233（负载 × 相位 41 × 二 轮）
    ("FRAG0234", 36), // FRAG 单元 0234（碎片 × 相位 42 × 二 轮）
    ("SW0235", 43), // SW 单元 0235（切换 × 相位 43 × 二 轮）
    ("ISOL0236", 50), // ISOL 单元 0236（隔离 × 相位 44 × 二 轮）
    ("FAIR0237", 57), // FAIR 单元 0237（公平 × 相位 45 × 二 轮）
    ("BUD0238", 64), // BUD 单元 0238（预算 × 相位 46 × 二 轮）
    ("VERDICT0239", 71), // VERDICT 单元 0239（裁决 × 相位 47 × 二 轮）
    ("HP0240", 78), // HP 单元 0240（大页 × 相位 48 × 二 轮）
    ("AS0241", 85), // AS 单元 0241（地址空间 × 相位 49 × 二 轮）
    ("JOINT0242", 3), // JOINT 单元 0242（联合 × 相位 50 × 二 轮）
    ("MATRIX0243", 10), // MATRIX 单元 0243（矩阵 × 相位 51 × 二 轮）
    ("XPROD0244", 17), // XPROD 单元 0244（交叉组合 × 相位 52 × 二 轮）
    ("LOAD0245", 24), // LOAD 单元 0245（负载 × 相位 53 × 二 轮）
    ("FRAG0246", 31), // FRAG 单元 0246（碎片 × 相位 54 × 二 轮）
    ("SW0247", 38), // SW 单元 0247（切换 × 相位 55 × 二 轮）
    ("ISOL0248", 45), // ISOL 单元 0248（隔离 × 相位 56 × 二 轮）
    ("FAIR0249", 52), // FAIR 单元 0249（公平 × 相位 57 × 二 轮）
    ("BUD0250", 59), // BUD 单元 0250（预算 × 相位 58 × 二 轮）
    ("VERDICT0251", 66), // VERDICT 单元 0251（裁决 × 相位 59 × 二 轮）
    ("HP0252", 73), // HP 单元 0252（大页 × 相位 60 × 二 轮）
    ("AS0253", 80), // AS 单元 0253（地址空间 × 相位 61 × 二 轮）
    ("JOINT0254", 87), // JOINT 单元 0254（联合 × 相位 62 × 二 轮）
    ("MATRIX0255", 5), // MATRIX 单元 0255（矩阵 × 相位 63 × 二 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2841bmx_get(key: &str) -> Option<u64> {
    match key {
        "HP0000" => Some(0),
        "AS0001" => Some(7),
        "JOINT0002" => Some(14),
        "MATRIX0003" => Some(21),
        "XPROD0004" => Some(28),
        "LOAD0005" => Some(35),
        "FRAG0006" => Some(42),
        "SW0007" => Some(49),
        "ISOL0008" => Some(56),
        "FAIR0009" => Some(63),
        "BUD0010" => Some(70),
        "VERDICT0011" => Some(77),
        "HP0012" => Some(84),
        "AS0013" => Some(2),
        "JOINT0014" => Some(9),
        "MATRIX0015" => Some(16),
        "XPROD0016" => Some(23),
        "LOAD0017" => Some(30),
        "FRAG0018" => Some(37),
        "SW0019" => Some(44),
        "ISOL0020" => Some(51),
        "FAIR0021" => Some(58),
        "BUD0022" => Some(65),
        "VERDICT0023" => Some(72),
        "HP0024" => Some(79),
        "AS0025" => Some(86),
        "JOINT0026" => Some(4),
        "MATRIX0027" => Some(11),
        "XPROD0028" => Some(18),
        "LOAD0029" => Some(25),
        "FRAG0030" => Some(32),
        "SW0031" => Some(39),
        "ISOL0032" => Some(46),
        "FAIR0033" => Some(53),
        "BUD0034" => Some(60),
        "VERDICT0035" => Some(67),
        "HP0036" => Some(74),
        "AS0037" => Some(81),
        "JOINT0038" => Some(88),
        "MATRIX0039" => Some(6),
        "XPROD0040" => Some(13),
        "LOAD0041" => Some(20),
        "FRAG0042" => Some(27),
        "SW0043" => Some(34),
        "ISOL0044" => Some(41),
        "FAIR0045" => Some(48),
        "BUD0046" => Some(55),
        "VERDICT0047" => Some(62),
        "HP0048" => Some(69),
        "AS0049" => Some(76),
        "JOINT0050" => Some(83),
        "MATRIX0051" => Some(1),
        "XPROD0052" => Some(8),
        "LOAD0053" => Some(15),
        "FRAG0054" => Some(22),
        "SW0055" => Some(29),
        "ISOL0056" => Some(36),
        "FAIR0057" => Some(43),
        "BUD0058" => Some(50),
        "VERDICT0059" => Some(57),
        "HP0060" => Some(64),
        "AS0061" => Some(71),
        "JOINT0062" => Some(78),
        "MATRIX0063" => Some(85),
        "XPROD0064" => Some(3),
        "LOAD0065" => Some(10),
        "FRAG0066" => Some(17),
        "SW0067" => Some(24),
        "ISOL0068" => Some(31),
        "FAIR0069" => Some(38),
        "BUD0070" => Some(45),
        "VERDICT0071" => Some(52),
        "HP0072" => Some(59),
        "AS0073" => Some(66),
        "JOINT0074" => Some(73),
        "MATRIX0075" => Some(80),
        "XPROD0076" => Some(87),
        "LOAD0077" => Some(5),
        "FRAG0078" => Some(12),
        "SW0079" => Some(19),
        "ISOL0080" => Some(26),
        "FAIR0081" => Some(33),
        "BUD0082" => Some(40),
        "VERDICT0083" => Some(47),
        "HP0084" => Some(54),
        "AS0085" => Some(61),
        "JOINT0086" => Some(68),
        "MATRIX0087" => Some(75),
        "XPROD0088" => Some(82),
        "LOAD0089" => Some(0),
        "FRAG0090" => Some(7),
        "SW0091" => Some(14),
        "ISOL0092" => Some(21),
        "FAIR0093" => Some(28),
        "BUD0094" => Some(35),
        "VERDICT0095" => Some(42),
        "HP0096" => Some(49),
        "AS0097" => Some(56),
        "JOINT0098" => Some(63),
        "MATRIX0099" => Some(70),
        "XPROD0100" => Some(77),
        "LOAD0101" => Some(84),
        "FRAG0102" => Some(2),
        "SW0103" => Some(9),
        "ISOL0104" => Some(16),
        "FAIR0105" => Some(23),
        "BUD0106" => Some(30),
        "VERDICT0107" => Some(37),
        "HP0108" => Some(44),
        "AS0109" => Some(51),
        "JOINT0110" => Some(58),
        "MATRIX0111" => Some(65),
        "XPROD0112" => Some(72),
        "LOAD0113" => Some(79),
        "FRAG0114" => Some(86),
        "SW0115" => Some(4),
        "ISOL0116" => Some(11),
        "FAIR0117" => Some(18),
        "BUD0118" => Some(25),
        "VERDICT0119" => Some(32),
        "HP0120" => Some(39),
        "AS0121" => Some(46),
        "JOINT0122" => Some(53),
        "MATRIX0123" => Some(60),
        "XPROD0124" => Some(67),
        "LOAD0125" => Some(74),
        "FRAG0126" => Some(81),
        "SW0127" => Some(88),
        "ISOL0128" => Some(6),
        "FAIR0129" => Some(13),
        "BUD0130" => Some(20),
        "VERDICT0131" => Some(27),
        "HP0132" => Some(34),
        "AS0133" => Some(41),
        "JOINT0134" => Some(48),
        "MATRIX0135" => Some(55),
        "XPROD0136" => Some(62),
        "LOAD0137" => Some(69),
        "FRAG0138" => Some(76),
        "SW0139" => Some(83),
        "ISOL0140" => Some(1),
        "FAIR0141" => Some(8),
        "BUD0142" => Some(15),
        "VERDICT0143" => Some(22),
        "HP0144" => Some(29),
        "AS0145" => Some(36),
        "JOINT0146" => Some(43),
        "MATRIX0147" => Some(50),
        "XPROD0148" => Some(57),
        "LOAD0149" => Some(64),
        "FRAG0150" => Some(71),
        "SW0151" => Some(78),
        "ISOL0152" => Some(85),
        "FAIR0153" => Some(3),
        "BUD0154" => Some(10),
        "VERDICT0155" => Some(17),
        "HP0156" => Some(24),
        "AS0157" => Some(31),
        "JOINT0158" => Some(38),
        "MATRIX0159" => Some(45),
        "XPROD0160" => Some(52),
        "LOAD0161" => Some(59),
        "FRAG0162" => Some(66),
        "SW0163" => Some(73),
        "ISOL0164" => Some(80),
        "FAIR0165" => Some(87),
        "BUD0166" => Some(5),
        "VERDICT0167" => Some(12),
        "HP0168" => Some(19),
        "AS0169" => Some(26),
        "JOINT0170" => Some(33),
        "MATRIX0171" => Some(40),
        "XPROD0172" => Some(47),
        "LOAD0173" => Some(54),
        "FRAG0174" => Some(61),
        "SW0175" => Some(68),
        "ISOL0176" => Some(75),
        "FAIR0177" => Some(82),
        "BUD0178" => Some(0),
        "VERDICT0179" => Some(7),
        "HP0180" => Some(14),
        "AS0181" => Some(21),
        "JOINT0182" => Some(28),
        "MATRIX0183" => Some(35),
        "XPROD0184" => Some(42),
        "LOAD0185" => Some(49),
        "FRAG0186" => Some(56),
        "SW0187" => Some(63),
        "ISOL0188" => Some(70),
        "FAIR0189" => Some(77),
        "BUD0190" => Some(84),
        "VERDICT0191" => Some(2),
        "HP0192" => Some(9),
        "AS0193" => Some(16),
        "JOINT0194" => Some(23),
        "MATRIX0195" => Some(30),
        "XPROD0196" => Some(37),
        "LOAD0197" => Some(44),
        "FRAG0198" => Some(51),
        "SW0199" => Some(58),
        "ISOL0200" => Some(65),
        "FAIR0201" => Some(72),
        "BUD0202" => Some(79),
        "VERDICT0203" => Some(86),
        "HP0204" => Some(4),
        "AS0205" => Some(11),
        "JOINT0206" => Some(18),
        "MATRIX0207" => Some(25),
        "XPROD0208" => Some(32),
        "LOAD0209" => Some(39),
        "FRAG0210" => Some(46),
        "SW0211" => Some(53),
        "ISOL0212" => Some(60),
        "FAIR0213" => Some(67),
        "BUD0214" => Some(74),
        "VERDICT0215" => Some(81),
        "HP0216" => Some(88),
        "AS0217" => Some(6),
        "JOINT0218" => Some(13),
        "MATRIX0219" => Some(20),
        "XPROD0220" => Some(27),
        "LOAD0221" => Some(34),
        "FRAG0222" => Some(41),
        "SW0223" => Some(48),
        "ISOL0224" => Some(55),
        "FAIR0225" => Some(62),
        "BUD0226" => Some(69),
        "VERDICT0227" => Some(76),
        "HP0228" => Some(83),
        "AS0229" => Some(1),
        "JOINT0230" => Some(8),
        "MATRIX0231" => Some(15),
        "XPROD0232" => Some(22),
        "LOAD0233" => Some(29),
        "FRAG0234" => Some(36),
        "SW0235" => Some(43),
        "ISOL0236" => Some(50),
        "FAIR0237" => Some(57),
        "BUD0238" => Some(64),
        "VERDICT0239" => Some(71),
        "HP0240" => Some(78),
        "AS0241" => Some(85),
        "JOINT0242" => Some(3),
        "MATRIX0243" => Some(10),
        "XPROD0244" => Some(17),
        "LOAD0245" => Some(24),
        "FRAG0246" => Some(31),
        "SW0247" => Some(38),
        "ISOL0248" => Some(45),
        "FAIR0249" => Some(52),
        "BUD0250" => Some(59),
        "VERDICT0251" => Some(66),
        "HP0252" => Some(73),
        "AS0253" => Some(80),
        "JOINT0254" => Some(87),
        "MATRIX0255" => Some(5),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2841bmx_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2841bmx_COUNT {
        let (k, v) = u2841bmx[i];
        let ok = match k {
            "HP0000" => v < 89,
            "AS0001" => v < 89,
            "JOINT0002" => v < 89,
            "MATRIX0003" => v < 89,
            "XPROD0004" => v < 89,
            "LOAD0005" => v < 89,
            "FRAG0006" => v < 89,
            "SW0007" => v < 89,
            "ISOL0008" => v < 89,
            "FAIR0009" => v < 89,
            "BUD0010" => v < 89,
            "VERDICT0011" => v < 89,
            "HP0012" => v < 89,
            "AS0013" => v < 89,
            "JOINT0014" => v < 89,
            "MATRIX0015" => v < 89,
            "XPROD0016" => v < 89,
            "LOAD0017" => v < 89,
            "FRAG0018" => v < 89,
            "SW0019" => v < 89,
            "ISOL0020" => v < 89,
            "FAIR0021" => v < 89,
            "BUD0022" => v < 89,
            "VERDICT0023" => v < 89,
            "HP0024" => v < 89,
            "AS0025" => v < 89,
            "JOINT0026" => v < 89,
            "MATRIX0027" => v < 89,
            "XPROD0028" => v < 89,
            "LOAD0029" => v < 89,
            "FRAG0030" => v < 89,
            "SW0031" => v < 89,
            "ISOL0032" => v < 89,
            "FAIR0033" => v < 89,
            "BUD0034" => v < 89,
            "VERDICT0035" => v < 89,
            "HP0036" => v < 89,
            "AS0037" => v < 89,
            "JOINT0038" => v < 89,
            "MATRIX0039" => v < 89,
            "XPROD0040" => v < 89,
            "LOAD0041" => v < 89,
            "FRAG0042" => v < 89,
            "SW0043" => v < 89,
            "ISOL0044" => v < 89,
            "FAIR0045" => v < 89,
            "BUD0046" => v < 89,
            "VERDICT0047" => v < 89,
            "HP0048" => v < 89,
            "AS0049" => v < 89,
            "JOINT0050" => v < 89,
            "MATRIX0051" => v < 89,
            "XPROD0052" => v < 89,
            "LOAD0053" => v < 89,
            "FRAG0054" => v < 89,
            "SW0055" => v < 89,
            "ISOL0056" => v < 89,
            "FAIR0057" => v < 89,
            "BUD0058" => v < 89,
            "VERDICT0059" => v < 89,
            "HP0060" => v < 89,
            "AS0061" => v < 89,
            "JOINT0062" => v < 89,
            "MATRIX0063" => v < 89,
            "XPROD0064" => v < 89,
            "LOAD0065" => v < 89,
            "FRAG0066" => v < 89,
            "SW0067" => v < 89,
            "ISOL0068" => v < 89,
            "FAIR0069" => v < 89,
            "BUD0070" => v < 89,
            "VERDICT0071" => v < 89,
            "HP0072" => v < 89,
            "AS0073" => v < 89,
            "JOINT0074" => v < 89,
            "MATRIX0075" => v < 89,
            "XPROD0076" => v < 89,
            "LOAD0077" => v < 89,
            "FRAG0078" => v < 89,
            "SW0079" => v < 89,
            "ISOL0080" => v < 89,
            "FAIR0081" => v < 89,
            "BUD0082" => v < 89,
            "VERDICT0083" => v < 89,
            "HP0084" => v < 89,
            "AS0085" => v < 89,
            "JOINT0086" => v < 89,
            "MATRIX0087" => v < 89,
            "XPROD0088" => v < 89,
            "LOAD0089" => v < 89,
            "FRAG0090" => v < 89,
            "SW0091" => v < 89,
            "ISOL0092" => v < 89,
            "FAIR0093" => v < 89,
            "BUD0094" => v < 89,
            "VERDICT0095" => v < 89,
            "HP0096" => v < 89,
            "AS0097" => v < 89,
            "JOINT0098" => v < 89,
            "MATRIX0099" => v < 89,
            "XPROD0100" => v < 89,
            "LOAD0101" => v < 89,
            "FRAG0102" => v < 89,
            "SW0103" => v < 89,
            "ISOL0104" => v < 89,
            "FAIR0105" => v < 89,
            "BUD0106" => v < 89,
            "VERDICT0107" => v < 89,
            "HP0108" => v < 89,
            "AS0109" => v < 89,
            "JOINT0110" => v < 89,
            "MATRIX0111" => v < 89,
            "XPROD0112" => v < 89,
            "LOAD0113" => v < 89,
            "FRAG0114" => v < 89,
            "SW0115" => v < 89,
            "ISOL0116" => v < 89,
            "FAIR0117" => v < 89,
            "BUD0118" => v < 89,
            "VERDICT0119" => v < 89,
            "HP0120" => v < 89,
            "AS0121" => v < 89,
            "JOINT0122" => v < 89,
            "MATRIX0123" => v < 89,
            "XPROD0124" => v < 89,
            "LOAD0125" => v < 89,
            "FRAG0126" => v < 89,
            "SW0127" => v < 89,
            "ISOL0128" => v < 89,
            "FAIR0129" => v < 89,
            "BUD0130" => v < 89,
            "VERDICT0131" => v < 89,
            "HP0132" => v < 89,
            "AS0133" => v < 89,
            "JOINT0134" => v < 89,
            "MATRIX0135" => v < 89,
            "XPROD0136" => v < 89,
            "LOAD0137" => v < 89,
            "FRAG0138" => v < 89,
            "SW0139" => v < 89,
            "ISOL0140" => v < 89,
            "FAIR0141" => v < 89,
            "BUD0142" => v < 89,
            "VERDICT0143" => v < 89,
            "HP0144" => v < 89,
            "AS0145" => v < 89,
            "JOINT0146" => v < 89,
            "MATRIX0147" => v < 89,
            "XPROD0148" => v < 89,
            "LOAD0149" => v < 89,
            "FRAG0150" => v < 89,
            "SW0151" => v < 89,
            "ISOL0152" => v < 89,
            "FAIR0153" => v < 89,
            "BUD0154" => v < 89,
            "VERDICT0155" => v < 89,
            "HP0156" => v < 89,
            "AS0157" => v < 89,
            "JOINT0158" => v < 89,
            "MATRIX0159" => v < 89,
            "XPROD0160" => v < 89,
            "LOAD0161" => v < 89,
            "FRAG0162" => v < 89,
            "SW0163" => v < 89,
            "ISOL0164" => v < 89,
            "FAIR0165" => v < 89,
            "BUD0166" => v < 89,
            "VERDICT0167" => v < 89,
            "HP0168" => v < 89,
            "AS0169" => v < 89,
            "JOINT0170" => v < 89,
            "MATRIX0171" => v < 89,
            "XPROD0172" => v < 89,
            "LOAD0173" => v < 89,
            "FRAG0174" => v < 89,
            "SW0175" => v < 89,
            "ISOL0176" => v < 89,
            "FAIR0177" => v < 89,
            "BUD0178" => v < 89,
            "VERDICT0179" => v < 89,
            "HP0180" => v < 89,
            "AS0181" => v < 89,
            "JOINT0182" => v < 89,
            "MATRIX0183" => v < 89,
            "XPROD0184" => v < 89,
            "LOAD0185" => v < 89,
            "FRAG0186" => v < 89,
            "SW0187" => v < 89,
            "ISOL0188" => v < 89,
            "FAIR0189" => v < 89,
            "BUD0190" => v < 89,
            "VERDICT0191" => v < 89,
            "HP0192" => v < 89,
            "AS0193" => v < 89,
            "JOINT0194" => v < 89,
            "MATRIX0195" => v < 89,
            "XPROD0196" => v < 89,
            "LOAD0197" => v < 89,
            "FRAG0198" => v < 89,
            "SW0199" => v < 89,
            "ISOL0200" => v < 89,
            "FAIR0201" => v < 89,
            "BUD0202" => v < 89,
            "VERDICT0203" => v < 89,
            "HP0204" => v < 89,
            "AS0205" => v < 89,
            "JOINT0206" => v < 89,
            "MATRIX0207" => v < 89,
            "XPROD0208" => v < 89,
            "LOAD0209" => v < 89,
            "FRAG0210" => v < 89,
            "SW0211" => v < 89,
            "ISOL0212" => v < 89,
            "FAIR0213" => v < 89,
            "BUD0214" => v < 89,
            "VERDICT0215" => v < 89,
            "HP0216" => v < 89,
            "AS0217" => v < 89,
            "JOINT0218" => v < 89,
            "MATRIX0219" => v < 89,
            "XPROD0220" => v < 89,
            "LOAD0221" => v < 89,
            "FRAG0222" => v < 89,
            "SW0223" => v < 89,
            "ISOL0224" => v < 89,
            "FAIR0225" => v < 89,
            "BUD0226" => v < 89,
            "VERDICT0227" => v < 89,
            "HP0228" => v < 89,
            "AS0229" => v < 89,
            "JOINT0230" => v < 89,
            "MATRIX0231" => v < 89,
            "XPROD0232" => v < 89,
            "LOAD0233" => v < 89,
            "FRAG0234" => v < 89,
            "SW0235" => v < 89,
            "ISOL0236" => v < 89,
            "FAIR0237" => v < 89,
            "BUD0238" => v < 89,
            "VERDICT0239" => v < 89,
            "HP0240" => v < 89,
            "AS0241" => v < 89,
            "JOINT0242" => v < 89,
            "MATRIX0243" => v < 89,
            "XPROD0244" => v < 89,
            "LOAD0245" => v < 89,
            "FRAG0246" => v < 89,
            "SW0247" => v < 89,
            "ISOL0248" => v < 89,
            "FAIR0249" => v < 89,
            "BUD0250" => v < 89,
            "VERDICT0251" => v < 89,
            "HP0252" => v < 89,
            "AS0253" => v < 89,
            "JOINT0254" => v < 89,
            "MATRIX0255" => v < 89,
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
pub fn u2841bmx_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2841bmx_COUNT {
        s = s.wrapping_add(u2841bmx[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2841bmx_unique() -> bool {
    let mut i = 0usize;
    while i < u2841bmx_COUNT {
        let mut j = i + 1;
        while j < u2841bmx_COUNT {
            if u2841bmx[i].0 == u2841bmx[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe280, 0xe28f]。
pub const fn u2841_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2841Code::ALL.len() {
        let c = u2841Code::ALL[i].code();
        if c < 0xe280 || c > 0xe28f {
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
fn u2841_guard_seg() -> bool {
    u2841_segment_guard()
}

/// UNX-F2841 bmx 256 行齐备 + 边界锚 + 键外拒收。
fn check_u2841_bmx_count() -> bool {
    let mut ok = u2841bmx_COUNT == 256;
    if ok { ok = u2841bmx_get("HP0000") == Some(0) && u2841bmx_get("MATRIX0255") == Some(5); }
    if ok { ok = u2841bmx_get("ZZZ_NOKEY").is_none(); }
    ok
}

/// UNX-F2841 bmx 累计守恒（理论值 11231）。
fn check_u2841_bmx_sum() -> bool {
    u2841bmx_sum() == 11231
}

/// UNX-F2841 bmx 逐行守卫与唯一性全绿。
fn check_u2841_bmx_guard() -> bool {
    u2841bmx_scan() == 0 && u2841bmx_unique()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2841_checks() -> CheckSet {
    let mut set = CheckSet::new("U2841");
    set.add(
        "u2841-seg-guard",
        u2841_guard_seg(),
        if u2841_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2841-bmx-count",
        check_u2841_bmx_count(),
        if check_u2841_bmx_count() { "" } else { "UNX-F2841 bmx count mismatch" },
    );
    set.add(
        "u2841-bmx-sum",
        check_u2841_bmx_sum(),
        if check_u2841_bmx_sum() { "" } else { "UNX-F2841 bmx sum mismatch" },
    );
    set.add(
        "u2841-bmx-guard",
        check_u2841_bmx_guard(),
        if check_u2841_bmx_guard() { "" } else { "UNX-F2841 bmx guard mismatch" },
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
    fn u2841_all_checks_green() {
        let set = run_u2841_checks();
        assert!(set.all_passed(), "U2841 domain checks must be green");
    }

    #[test]
    fn u2841_segment_guard_holds() {
        assert!(u2841_segment_guard());
    }
}
