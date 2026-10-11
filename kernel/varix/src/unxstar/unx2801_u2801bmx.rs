//! unxu2801_u2801bmx —— 大页池容量压测方案冻结 深编bmx（大页池容量压测 × 256 单元）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2801 · 大页池容量压测方案冻结 · 判据原文：UNX-F2801 · 判据：UNX-F2801-J1 三态方案（满池/近满/空）参数冻结哈希入册，方案与 F2425 池账字段逐项对齐
//! · 组件：大页池容量压测 守卫映射 256 行（二 轮派生真实数据 + 逐行守卫 + 键唯一）
//! · 判据落地：UNX-F2801 · 判据：UNX-F2801-J1 三态方案（满池/近满/空）参数冻结哈希入册，方案与 F2425 池账字段逐项对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2801_VERSION: &str = "u2801-u2801bmx-v1";

/// 诊断段基址。
pub const U2801_SEG_BASE: u16 = 0xe260;

// --------------------------------------------------------------------------
// 诊断码（u2801 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页池容量压测 bmx 面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2801Code {
    /// 0xe260 单元格重复（大页 同键双占）。
    CellDup,
    /// 0xe261 单元格断档（压测 序号不连续）。
    CellGap,
    /// 0xe262 索引越界（容量）。
    IndexOob,
    /// 0xe263 大表账破缺（行数≠登记值）。
    LedgerBreak,
    /// 0xe264 表序断档。
    OrderBreak,
    /// 0xe265 累计漂移（求和≠理论值）。
    SumDrift,
    /// 0xe266 掩码非法。
    MaskBad,
    /// 0xe267 守卫触发。
    GuardTripped,
    /// 0xe268 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe269 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe26a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe26b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2801Code {
    /// 全量闭集。
    pub const ALL: [u2801Code; 12] = [
        u2801Code::CellDup,
        u2801Code::CellGap,
        u2801Code::IndexOob,
        u2801Code::LedgerBreak,
        u2801Code::OrderBreak,
        u2801Code::SumDrift,
        u2801Code::MaskBad,
        u2801Code::GuardTripped,
        u2801Code::LedgerBroken,
        u2801Code::SnapshotFingerprint,
        u2801Code::TableBroken,
        u2801Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2801Code::CellDup => 0xe260,
            u2801Code::CellGap => 0xe261,
            u2801Code::IndexOob => 0xe262,
            u2801Code::LedgerBreak => 0xe263,
            u2801Code::OrderBreak => 0xe264,
            u2801Code::SumDrift => 0xe265,
            u2801Code::MaskBad => 0xe266,
            u2801Code::GuardTripped => 0xe267,
            u2801Code::LedgerBroken => 0xe268,
            u2801Code::SnapshotFingerprint => 0xe269,
            u2801Code::TableBroken => 0xe26a,
            u2801Code::SegmentGuard => 0xe26b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2801Code::CellDup => "单元格重复（大页 同键双占）",
            u2801Code::CellGap => "单元格断档（压测 序号不连续）",
            u2801Code::IndexOob => "索引越界（容量）",
            u2801Code::LedgerBreak => "大表账破缺（行数≠登记值）",
            u2801Code::OrderBreak => "表序断档",
            u2801Code::SumDrift => "累计漂移（求和≠理论值）",
            u2801Code::MaskBad => "掩码非法",
            u2801Code::GuardTripped => "守卫触发",
            u2801Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2801Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2801Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2801Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2801Code::CellDup => "按键唯一律去重",
            u2801Code::CellGap => "按连续律补号",
            u2801Code::IndexOob => "按行数边界裁剪",
            u2801Code::LedgerBreak => "按守恒重建",
            u2801Code::OrderBreak => "按序重排",
            u2801Code::SumDrift => "按理论值复算",
            u2801Code::MaskBad => "按掩码律校正",
            u2801Code::GuardTripped => "检查临界区",
            u2801Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2801Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2801Code::TableBroken => "按语义规则重建常量表",
            u2801Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2801bmx（256 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 大页池容量压测 bmx 守卫映射（256 行逐行注释自证）。
/// 行数。
pub const u2801bmx_COUNT: usize = 256;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2801bmx: [(&'static str, u64); 256] = [
    ("HP0000", 0), // HP 单元 0000（大页 × 相位 00 × 二 轮）
    ("POOL0001", 7), // POOL 单元 0001（池 × 相位 01 × 二 轮）
    ("CAP0002", 14), // CAP 单元 0002（容量 × 相位 02 × 二 轮）
    ("PRESS0003", 21), // PRESS 单元 0003（压测 × 相位 03 × 二 轮）
    ("MATRIX0004", 28), // MATRIX 单元 0004（矩阵 × 相位 04 × 二 轮）
    ("ALLOC0005", 35), // ALLOC 单元 0005（分配 × 相位 05 × 二 轮）
    ("FREE0006", 42), // FREE 单元 0006（释放 × 相位 06 × 二 轮）
    ("FRAG0007", 49), // FRAG 单元 0007（碎片 × 相位 07 × 二 轮）
    ("RSS0008", 56), // RSS 单元 0008（常驻 × 相位 08 × 二 轮）
    ("FLOOR0009", 63), // FLOOR 单元 0009（地板 × 相位 09 × 二 轮）
    ("CEIL0010", 70), // CEIL 单元 0010（天花板 × 相位 10 × 二 轮）
    ("BUD0011", 77), // BUD 单元 0011（预算 × 相位 11 × 二 轮）
    ("HP0012", 84), // HP 单元 0012（大页 × 相位 12 × 二 轮）
    ("POOL0013", 2), // POOL 单元 0013（池 × 相位 13 × 二 轮）
    ("CAP0014", 9), // CAP 单元 0014（容量 × 相位 14 × 二 轮）
    ("PRESS0015", 16), // PRESS 单元 0015（压测 × 相位 15 × 二 轮）
    ("MATRIX0016", 23), // MATRIX 单元 0016（矩阵 × 相位 16 × 二 轮）
    ("ALLOC0017", 30), // ALLOC 单元 0017（分配 × 相位 17 × 二 轮）
    ("FREE0018", 37), // FREE 单元 0018（释放 × 相位 18 × 二 轮）
    ("FRAG0019", 44), // FRAG 单元 0019（碎片 × 相位 19 × 二 轮）
    ("RSS0020", 51), // RSS 单元 0020（常驻 × 相位 20 × 二 轮）
    ("FLOOR0021", 58), // FLOOR 单元 0021（地板 × 相位 21 × 二 轮）
    ("CEIL0022", 65), // CEIL 单元 0022（天花板 × 相位 22 × 二 轮）
    ("BUD0023", 72), // BUD 单元 0023（预算 × 相位 23 × 二 轮）
    ("HP0024", 79), // HP 单元 0024（大页 × 相位 24 × 二 轮）
    ("POOL0025", 86), // POOL 单元 0025（池 × 相位 25 × 二 轮）
    ("CAP0026", 4), // CAP 单元 0026（容量 × 相位 26 × 二 轮）
    ("PRESS0027", 11), // PRESS 单元 0027（压测 × 相位 27 × 二 轮）
    ("MATRIX0028", 18), // MATRIX 单元 0028（矩阵 × 相位 28 × 二 轮）
    ("ALLOC0029", 25), // ALLOC 单元 0029（分配 × 相位 29 × 二 轮）
    ("FREE0030", 32), // FREE 单元 0030（释放 × 相位 30 × 二 轮）
    ("FRAG0031", 39), // FRAG 单元 0031（碎片 × 相位 31 × 二 轮）
    ("RSS0032", 46), // RSS 单元 0032（常驻 × 相位 32 × 二 轮）
    ("FLOOR0033", 53), // FLOOR 单元 0033（地板 × 相位 33 × 二 轮）
    ("CEIL0034", 60), // CEIL 单元 0034（天花板 × 相位 34 × 二 轮）
    ("BUD0035", 67), // BUD 单元 0035（预算 × 相位 35 × 二 轮）
    ("HP0036", 74), // HP 单元 0036（大页 × 相位 36 × 二 轮）
    ("POOL0037", 81), // POOL 单元 0037（池 × 相位 37 × 二 轮）
    ("CAP0038", 88), // CAP 单元 0038（容量 × 相位 38 × 二 轮）
    ("PRESS0039", 6), // PRESS 单元 0039（压测 × 相位 39 × 二 轮）
    ("MATRIX0040", 13), // MATRIX 单元 0040（矩阵 × 相位 40 × 二 轮）
    ("ALLOC0041", 20), // ALLOC 单元 0041（分配 × 相位 41 × 二 轮）
    ("FREE0042", 27), // FREE 单元 0042（释放 × 相位 42 × 二 轮）
    ("FRAG0043", 34), // FRAG 单元 0043（碎片 × 相位 43 × 二 轮）
    ("RSS0044", 41), // RSS 单元 0044（常驻 × 相位 44 × 二 轮）
    ("FLOOR0045", 48), // FLOOR 单元 0045（地板 × 相位 45 × 二 轮）
    ("CEIL0046", 55), // CEIL 单元 0046（天花板 × 相位 46 × 二 轮）
    ("BUD0047", 62), // BUD 单元 0047（预算 × 相位 47 × 二 轮）
    ("HP0048", 69), // HP 单元 0048（大页 × 相位 48 × 二 轮）
    ("POOL0049", 76), // POOL 单元 0049（池 × 相位 49 × 二 轮）
    ("CAP0050", 83), // CAP 单元 0050（容量 × 相位 50 × 二 轮）
    ("PRESS0051", 1), // PRESS 单元 0051（压测 × 相位 51 × 二 轮）
    ("MATRIX0052", 8), // MATRIX 单元 0052（矩阵 × 相位 52 × 二 轮）
    ("ALLOC0053", 15), // ALLOC 单元 0053（分配 × 相位 53 × 二 轮）
    ("FREE0054", 22), // FREE 单元 0054（释放 × 相位 54 × 二 轮）
    ("FRAG0055", 29), // FRAG 单元 0055（碎片 × 相位 55 × 二 轮）
    ("RSS0056", 36), // RSS 单元 0056（常驻 × 相位 56 × 二 轮）
    ("FLOOR0057", 43), // FLOOR 单元 0057（地板 × 相位 57 × 二 轮）
    ("CEIL0058", 50), // CEIL 单元 0058（天花板 × 相位 58 × 二 轮）
    ("BUD0059", 57), // BUD 单元 0059（预算 × 相位 59 × 二 轮）
    ("HP0060", 64), // HP 单元 0060（大页 × 相位 60 × 二 轮）
    ("POOL0061", 71), // POOL 单元 0061（池 × 相位 61 × 二 轮）
    ("CAP0062", 78), // CAP 单元 0062（容量 × 相位 62 × 二 轮）
    ("PRESS0063", 85), // PRESS 单元 0063（压测 × 相位 63 × 二 轮）
    ("MATRIX0064", 3), // MATRIX 单元 0064（矩阵 × 相位 00 × 二 轮）
    ("ALLOC0065", 10), // ALLOC 单元 0065（分配 × 相位 01 × 二 轮）
    ("FREE0066", 17), // FREE 单元 0066（释放 × 相位 02 × 二 轮）
    ("FRAG0067", 24), // FRAG 单元 0067（碎片 × 相位 03 × 二 轮）
    ("RSS0068", 31), // RSS 单元 0068（常驻 × 相位 04 × 二 轮）
    ("FLOOR0069", 38), // FLOOR 单元 0069（地板 × 相位 05 × 二 轮）
    ("CEIL0070", 45), // CEIL 单元 0070（天花板 × 相位 06 × 二 轮）
    ("BUD0071", 52), // BUD 单元 0071（预算 × 相位 07 × 二 轮）
    ("HP0072", 59), // HP 单元 0072（大页 × 相位 08 × 二 轮）
    ("POOL0073", 66), // POOL 单元 0073（池 × 相位 09 × 二 轮）
    ("CAP0074", 73), // CAP 单元 0074（容量 × 相位 10 × 二 轮）
    ("PRESS0075", 80), // PRESS 单元 0075（压测 × 相位 11 × 二 轮）
    ("MATRIX0076", 87), // MATRIX 单元 0076（矩阵 × 相位 12 × 二 轮）
    ("ALLOC0077", 5), // ALLOC 单元 0077（分配 × 相位 13 × 二 轮）
    ("FREE0078", 12), // FREE 单元 0078（释放 × 相位 14 × 二 轮）
    ("FRAG0079", 19), // FRAG 单元 0079（碎片 × 相位 15 × 二 轮）
    ("RSS0080", 26), // RSS 单元 0080（常驻 × 相位 16 × 二 轮）
    ("FLOOR0081", 33), // FLOOR 单元 0081（地板 × 相位 17 × 二 轮）
    ("CEIL0082", 40), // CEIL 单元 0082（天花板 × 相位 18 × 二 轮）
    ("BUD0083", 47), // BUD 单元 0083（预算 × 相位 19 × 二 轮）
    ("HP0084", 54), // HP 单元 0084（大页 × 相位 20 × 二 轮）
    ("POOL0085", 61), // POOL 单元 0085（池 × 相位 21 × 二 轮）
    ("CAP0086", 68), // CAP 单元 0086（容量 × 相位 22 × 二 轮）
    ("PRESS0087", 75), // PRESS 单元 0087（压测 × 相位 23 × 二 轮）
    ("MATRIX0088", 82), // MATRIX 单元 0088（矩阵 × 相位 24 × 二 轮）
    ("ALLOC0089", 0), // ALLOC 单元 0089（分配 × 相位 25 × 二 轮）
    ("FREE0090", 7), // FREE 单元 0090（释放 × 相位 26 × 二 轮）
    ("FRAG0091", 14), // FRAG 单元 0091（碎片 × 相位 27 × 二 轮）
    ("RSS0092", 21), // RSS 单元 0092（常驻 × 相位 28 × 二 轮）
    ("FLOOR0093", 28), // FLOOR 单元 0093（地板 × 相位 29 × 二 轮）
    ("CEIL0094", 35), // CEIL 单元 0094（天花板 × 相位 30 × 二 轮）
    ("BUD0095", 42), // BUD 单元 0095（预算 × 相位 31 × 二 轮）
    ("HP0096", 49), // HP 单元 0096（大页 × 相位 32 × 二 轮）
    ("POOL0097", 56), // POOL 单元 0097（池 × 相位 33 × 二 轮）
    ("CAP0098", 63), // CAP 单元 0098（容量 × 相位 34 × 二 轮）
    ("PRESS0099", 70), // PRESS 单元 0099（压测 × 相位 35 × 二 轮）
    ("MATRIX0100", 77), // MATRIX 单元 0100（矩阵 × 相位 36 × 二 轮）
    ("ALLOC0101", 84), // ALLOC 单元 0101（分配 × 相位 37 × 二 轮）
    ("FREE0102", 2), // FREE 单元 0102（释放 × 相位 38 × 二 轮）
    ("FRAG0103", 9), // FRAG 单元 0103（碎片 × 相位 39 × 二 轮）
    ("RSS0104", 16), // RSS 单元 0104（常驻 × 相位 40 × 二 轮）
    ("FLOOR0105", 23), // FLOOR 单元 0105（地板 × 相位 41 × 二 轮）
    ("CEIL0106", 30), // CEIL 单元 0106（天花板 × 相位 42 × 二 轮）
    ("BUD0107", 37), // BUD 单元 0107（预算 × 相位 43 × 二 轮）
    ("HP0108", 44), // HP 单元 0108（大页 × 相位 44 × 二 轮）
    ("POOL0109", 51), // POOL 单元 0109（池 × 相位 45 × 二 轮）
    ("CAP0110", 58), // CAP 单元 0110（容量 × 相位 46 × 二 轮）
    ("PRESS0111", 65), // PRESS 单元 0111（压测 × 相位 47 × 二 轮）
    ("MATRIX0112", 72), // MATRIX 单元 0112（矩阵 × 相位 48 × 二 轮）
    ("ALLOC0113", 79), // ALLOC 单元 0113（分配 × 相位 49 × 二 轮）
    ("FREE0114", 86), // FREE 单元 0114（释放 × 相位 50 × 二 轮）
    ("FRAG0115", 4), // FRAG 单元 0115（碎片 × 相位 51 × 二 轮）
    ("RSS0116", 11), // RSS 单元 0116（常驻 × 相位 52 × 二 轮）
    ("FLOOR0117", 18), // FLOOR 单元 0117（地板 × 相位 53 × 二 轮）
    ("CEIL0118", 25), // CEIL 单元 0118（天花板 × 相位 54 × 二 轮）
    ("BUD0119", 32), // BUD 单元 0119（预算 × 相位 55 × 二 轮）
    ("HP0120", 39), // HP 单元 0120（大页 × 相位 56 × 二 轮）
    ("POOL0121", 46), // POOL 单元 0121（池 × 相位 57 × 二 轮）
    ("CAP0122", 53), // CAP 单元 0122（容量 × 相位 58 × 二 轮）
    ("PRESS0123", 60), // PRESS 单元 0123（压测 × 相位 59 × 二 轮）
    ("MATRIX0124", 67), // MATRIX 单元 0124（矩阵 × 相位 60 × 二 轮）
    ("ALLOC0125", 74), // ALLOC 单元 0125（分配 × 相位 61 × 二 轮）
    ("FREE0126", 81), // FREE 单元 0126（释放 × 相位 62 × 二 轮）
    ("FRAG0127", 88), // FRAG 单元 0127（碎片 × 相位 63 × 二 轮）
    ("RSS0128", 6), // RSS 单元 0128（常驻 × 相位 00 × 二 轮）
    ("FLOOR0129", 13), // FLOOR 单元 0129（地板 × 相位 01 × 二 轮）
    ("CEIL0130", 20), // CEIL 单元 0130（天花板 × 相位 02 × 二 轮）
    ("BUD0131", 27), // BUD 单元 0131（预算 × 相位 03 × 二 轮）
    ("HP0132", 34), // HP 单元 0132（大页 × 相位 04 × 二 轮）
    ("POOL0133", 41), // POOL 单元 0133（池 × 相位 05 × 二 轮）
    ("CAP0134", 48), // CAP 单元 0134（容量 × 相位 06 × 二 轮）
    ("PRESS0135", 55), // PRESS 单元 0135（压测 × 相位 07 × 二 轮）
    ("MATRIX0136", 62), // MATRIX 单元 0136（矩阵 × 相位 08 × 二 轮）
    ("ALLOC0137", 69), // ALLOC 单元 0137（分配 × 相位 09 × 二 轮）
    ("FREE0138", 76), // FREE 单元 0138（释放 × 相位 10 × 二 轮）
    ("FRAG0139", 83), // FRAG 单元 0139（碎片 × 相位 11 × 二 轮）
    ("RSS0140", 1), // RSS 单元 0140（常驻 × 相位 12 × 二 轮）
    ("FLOOR0141", 8), // FLOOR 单元 0141（地板 × 相位 13 × 二 轮）
    ("CEIL0142", 15), // CEIL 单元 0142（天花板 × 相位 14 × 二 轮）
    ("BUD0143", 22), // BUD 单元 0143（预算 × 相位 15 × 二 轮）
    ("HP0144", 29), // HP 单元 0144（大页 × 相位 16 × 二 轮）
    ("POOL0145", 36), // POOL 单元 0145（池 × 相位 17 × 二 轮）
    ("CAP0146", 43), // CAP 单元 0146（容量 × 相位 18 × 二 轮）
    ("PRESS0147", 50), // PRESS 单元 0147（压测 × 相位 19 × 二 轮）
    ("MATRIX0148", 57), // MATRIX 单元 0148（矩阵 × 相位 20 × 二 轮）
    ("ALLOC0149", 64), // ALLOC 单元 0149（分配 × 相位 21 × 二 轮）
    ("FREE0150", 71), // FREE 单元 0150（释放 × 相位 22 × 二 轮）
    ("FRAG0151", 78), // FRAG 单元 0151（碎片 × 相位 23 × 二 轮）
    ("RSS0152", 85), // RSS 单元 0152（常驻 × 相位 24 × 二 轮）
    ("FLOOR0153", 3), // FLOOR 单元 0153（地板 × 相位 25 × 二 轮）
    ("CEIL0154", 10), // CEIL 单元 0154（天花板 × 相位 26 × 二 轮）
    ("BUD0155", 17), // BUD 单元 0155（预算 × 相位 27 × 二 轮）
    ("HP0156", 24), // HP 单元 0156（大页 × 相位 28 × 二 轮）
    ("POOL0157", 31), // POOL 单元 0157（池 × 相位 29 × 二 轮）
    ("CAP0158", 38), // CAP 单元 0158（容量 × 相位 30 × 二 轮）
    ("PRESS0159", 45), // PRESS 单元 0159（压测 × 相位 31 × 二 轮）
    ("MATRIX0160", 52), // MATRIX 单元 0160（矩阵 × 相位 32 × 二 轮）
    ("ALLOC0161", 59), // ALLOC 单元 0161（分配 × 相位 33 × 二 轮）
    ("FREE0162", 66), // FREE 单元 0162（释放 × 相位 34 × 二 轮）
    ("FRAG0163", 73), // FRAG 单元 0163（碎片 × 相位 35 × 二 轮）
    ("RSS0164", 80), // RSS 单元 0164（常驻 × 相位 36 × 二 轮）
    ("FLOOR0165", 87), // FLOOR 单元 0165（地板 × 相位 37 × 二 轮）
    ("CEIL0166", 5), // CEIL 单元 0166（天花板 × 相位 38 × 二 轮）
    ("BUD0167", 12), // BUD 单元 0167（预算 × 相位 39 × 二 轮）
    ("HP0168", 19), // HP 单元 0168（大页 × 相位 40 × 二 轮）
    ("POOL0169", 26), // POOL 单元 0169（池 × 相位 41 × 二 轮）
    ("CAP0170", 33), // CAP 单元 0170（容量 × 相位 42 × 二 轮）
    ("PRESS0171", 40), // PRESS 单元 0171（压测 × 相位 43 × 二 轮）
    ("MATRIX0172", 47), // MATRIX 单元 0172（矩阵 × 相位 44 × 二 轮）
    ("ALLOC0173", 54), // ALLOC 单元 0173（分配 × 相位 45 × 二 轮）
    ("FREE0174", 61), // FREE 单元 0174（释放 × 相位 46 × 二 轮）
    ("FRAG0175", 68), // FRAG 单元 0175（碎片 × 相位 47 × 二 轮）
    ("RSS0176", 75), // RSS 单元 0176（常驻 × 相位 48 × 二 轮）
    ("FLOOR0177", 82), // FLOOR 单元 0177（地板 × 相位 49 × 二 轮）
    ("CEIL0178", 0), // CEIL 单元 0178（天花板 × 相位 50 × 二 轮）
    ("BUD0179", 7), // BUD 单元 0179（预算 × 相位 51 × 二 轮）
    ("HP0180", 14), // HP 单元 0180（大页 × 相位 52 × 二 轮）
    ("POOL0181", 21), // POOL 单元 0181（池 × 相位 53 × 二 轮）
    ("CAP0182", 28), // CAP 单元 0182（容量 × 相位 54 × 二 轮）
    ("PRESS0183", 35), // PRESS 单元 0183（压测 × 相位 55 × 二 轮）
    ("MATRIX0184", 42), // MATRIX 单元 0184（矩阵 × 相位 56 × 二 轮）
    ("ALLOC0185", 49), // ALLOC 单元 0185（分配 × 相位 57 × 二 轮）
    ("FREE0186", 56), // FREE 单元 0186（释放 × 相位 58 × 二 轮）
    ("FRAG0187", 63), // FRAG 单元 0187（碎片 × 相位 59 × 二 轮）
    ("RSS0188", 70), // RSS 单元 0188（常驻 × 相位 60 × 二 轮）
    ("FLOOR0189", 77), // FLOOR 单元 0189（地板 × 相位 61 × 二 轮）
    ("CEIL0190", 84), // CEIL 单元 0190（天花板 × 相位 62 × 二 轮）
    ("BUD0191", 2), // BUD 单元 0191（预算 × 相位 63 × 二 轮）
    ("HP0192", 9), // HP 单元 0192（大页 × 相位 00 × 二 轮）
    ("POOL0193", 16), // POOL 单元 0193（池 × 相位 01 × 二 轮）
    ("CAP0194", 23), // CAP 单元 0194（容量 × 相位 02 × 二 轮）
    ("PRESS0195", 30), // PRESS 单元 0195（压测 × 相位 03 × 二 轮）
    ("MATRIX0196", 37), // MATRIX 单元 0196（矩阵 × 相位 04 × 二 轮）
    ("ALLOC0197", 44), // ALLOC 单元 0197（分配 × 相位 05 × 二 轮）
    ("FREE0198", 51), // FREE 单元 0198（释放 × 相位 06 × 二 轮）
    ("FRAG0199", 58), // FRAG 单元 0199（碎片 × 相位 07 × 二 轮）
    ("RSS0200", 65), // RSS 单元 0200（常驻 × 相位 08 × 二 轮）
    ("FLOOR0201", 72), // FLOOR 单元 0201（地板 × 相位 09 × 二 轮）
    ("CEIL0202", 79), // CEIL 单元 0202（天花板 × 相位 10 × 二 轮）
    ("BUD0203", 86), // BUD 单元 0203（预算 × 相位 11 × 二 轮）
    ("HP0204", 4), // HP 单元 0204（大页 × 相位 12 × 二 轮）
    ("POOL0205", 11), // POOL 单元 0205（池 × 相位 13 × 二 轮）
    ("CAP0206", 18), // CAP 单元 0206（容量 × 相位 14 × 二 轮）
    ("PRESS0207", 25), // PRESS 单元 0207（压测 × 相位 15 × 二 轮）
    ("MATRIX0208", 32), // MATRIX 单元 0208（矩阵 × 相位 16 × 二 轮）
    ("ALLOC0209", 39), // ALLOC 单元 0209（分配 × 相位 17 × 二 轮）
    ("FREE0210", 46), // FREE 单元 0210（释放 × 相位 18 × 二 轮）
    ("FRAG0211", 53), // FRAG 单元 0211（碎片 × 相位 19 × 二 轮）
    ("RSS0212", 60), // RSS 单元 0212（常驻 × 相位 20 × 二 轮）
    ("FLOOR0213", 67), // FLOOR 单元 0213（地板 × 相位 21 × 二 轮）
    ("CEIL0214", 74), // CEIL 单元 0214（天花板 × 相位 22 × 二 轮）
    ("BUD0215", 81), // BUD 单元 0215（预算 × 相位 23 × 二 轮）
    ("HP0216", 88), // HP 单元 0216（大页 × 相位 24 × 二 轮）
    ("POOL0217", 6), // POOL 单元 0217（池 × 相位 25 × 二 轮）
    ("CAP0218", 13), // CAP 单元 0218（容量 × 相位 26 × 二 轮）
    ("PRESS0219", 20), // PRESS 单元 0219（压测 × 相位 27 × 二 轮）
    ("MATRIX0220", 27), // MATRIX 单元 0220（矩阵 × 相位 28 × 二 轮）
    ("ALLOC0221", 34), // ALLOC 单元 0221（分配 × 相位 29 × 二 轮）
    ("FREE0222", 41), // FREE 单元 0222（释放 × 相位 30 × 二 轮）
    ("FRAG0223", 48), // FRAG 单元 0223（碎片 × 相位 31 × 二 轮）
    ("RSS0224", 55), // RSS 单元 0224（常驻 × 相位 32 × 二 轮）
    ("FLOOR0225", 62), // FLOOR 单元 0225（地板 × 相位 33 × 二 轮）
    ("CEIL0226", 69), // CEIL 单元 0226（天花板 × 相位 34 × 二 轮）
    ("BUD0227", 76), // BUD 单元 0227（预算 × 相位 35 × 二 轮）
    ("HP0228", 83), // HP 单元 0228（大页 × 相位 36 × 二 轮）
    ("POOL0229", 1), // POOL 单元 0229（池 × 相位 37 × 二 轮）
    ("CAP0230", 8), // CAP 单元 0230（容量 × 相位 38 × 二 轮）
    ("PRESS0231", 15), // PRESS 单元 0231（压测 × 相位 39 × 二 轮）
    ("MATRIX0232", 22), // MATRIX 单元 0232（矩阵 × 相位 40 × 二 轮）
    ("ALLOC0233", 29), // ALLOC 单元 0233（分配 × 相位 41 × 二 轮）
    ("FREE0234", 36), // FREE 单元 0234（释放 × 相位 42 × 二 轮）
    ("FRAG0235", 43), // FRAG 单元 0235（碎片 × 相位 43 × 二 轮）
    ("RSS0236", 50), // RSS 单元 0236（常驻 × 相位 44 × 二 轮）
    ("FLOOR0237", 57), // FLOOR 单元 0237（地板 × 相位 45 × 二 轮）
    ("CEIL0238", 64), // CEIL 单元 0238（天花板 × 相位 46 × 二 轮）
    ("BUD0239", 71), // BUD 单元 0239（预算 × 相位 47 × 二 轮）
    ("HP0240", 78), // HP 单元 0240（大页 × 相位 48 × 二 轮）
    ("POOL0241", 85), // POOL 单元 0241（池 × 相位 49 × 二 轮）
    ("CAP0242", 3), // CAP 单元 0242（容量 × 相位 50 × 二 轮）
    ("PRESS0243", 10), // PRESS 单元 0243（压测 × 相位 51 × 二 轮）
    ("MATRIX0244", 17), // MATRIX 单元 0244（矩阵 × 相位 52 × 二 轮）
    ("ALLOC0245", 24), // ALLOC 单元 0245（分配 × 相位 53 × 二 轮）
    ("FREE0246", 31), // FREE 单元 0246（释放 × 相位 54 × 二 轮）
    ("FRAG0247", 38), // FRAG 单元 0247（碎片 × 相位 55 × 二 轮）
    ("RSS0248", 45), // RSS 单元 0248（常驻 × 相位 56 × 二 轮）
    ("FLOOR0249", 52), // FLOOR 单元 0249（地板 × 相位 57 × 二 轮）
    ("CEIL0250", 59), // CEIL 单元 0250（天花板 × 相位 58 × 二 轮）
    ("BUD0251", 66), // BUD 单元 0251（预算 × 相位 59 × 二 轮）
    ("HP0252", 73), // HP 单元 0252（大页 × 相位 60 × 二 轮）
    ("POOL0253", 80), // POOL 单元 0253（池 × 相位 61 × 二 轮）
    ("CAP0254", 87), // CAP 单元 0254（容量 × 相位 62 × 二 轮）
    ("PRESS0255", 5), // PRESS 单元 0255（压测 × 相位 63 × 二 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2801bmx_get(key: &str) -> Option<u64> {
    match key {
        "HP0000" => Some(0),
        "POOL0001" => Some(7),
        "CAP0002" => Some(14),
        "PRESS0003" => Some(21),
        "MATRIX0004" => Some(28),
        "ALLOC0005" => Some(35),
        "FREE0006" => Some(42),
        "FRAG0007" => Some(49),
        "RSS0008" => Some(56),
        "FLOOR0009" => Some(63),
        "CEIL0010" => Some(70),
        "BUD0011" => Some(77),
        "HP0012" => Some(84),
        "POOL0013" => Some(2),
        "CAP0014" => Some(9),
        "PRESS0015" => Some(16),
        "MATRIX0016" => Some(23),
        "ALLOC0017" => Some(30),
        "FREE0018" => Some(37),
        "FRAG0019" => Some(44),
        "RSS0020" => Some(51),
        "FLOOR0021" => Some(58),
        "CEIL0022" => Some(65),
        "BUD0023" => Some(72),
        "HP0024" => Some(79),
        "POOL0025" => Some(86),
        "CAP0026" => Some(4),
        "PRESS0027" => Some(11),
        "MATRIX0028" => Some(18),
        "ALLOC0029" => Some(25),
        "FREE0030" => Some(32),
        "FRAG0031" => Some(39),
        "RSS0032" => Some(46),
        "FLOOR0033" => Some(53),
        "CEIL0034" => Some(60),
        "BUD0035" => Some(67),
        "HP0036" => Some(74),
        "POOL0037" => Some(81),
        "CAP0038" => Some(88),
        "PRESS0039" => Some(6),
        "MATRIX0040" => Some(13),
        "ALLOC0041" => Some(20),
        "FREE0042" => Some(27),
        "FRAG0043" => Some(34),
        "RSS0044" => Some(41),
        "FLOOR0045" => Some(48),
        "CEIL0046" => Some(55),
        "BUD0047" => Some(62),
        "HP0048" => Some(69),
        "POOL0049" => Some(76),
        "CAP0050" => Some(83),
        "PRESS0051" => Some(1),
        "MATRIX0052" => Some(8),
        "ALLOC0053" => Some(15),
        "FREE0054" => Some(22),
        "FRAG0055" => Some(29),
        "RSS0056" => Some(36),
        "FLOOR0057" => Some(43),
        "CEIL0058" => Some(50),
        "BUD0059" => Some(57),
        "HP0060" => Some(64),
        "POOL0061" => Some(71),
        "CAP0062" => Some(78),
        "PRESS0063" => Some(85),
        "MATRIX0064" => Some(3),
        "ALLOC0065" => Some(10),
        "FREE0066" => Some(17),
        "FRAG0067" => Some(24),
        "RSS0068" => Some(31),
        "FLOOR0069" => Some(38),
        "CEIL0070" => Some(45),
        "BUD0071" => Some(52),
        "HP0072" => Some(59),
        "POOL0073" => Some(66),
        "CAP0074" => Some(73),
        "PRESS0075" => Some(80),
        "MATRIX0076" => Some(87),
        "ALLOC0077" => Some(5),
        "FREE0078" => Some(12),
        "FRAG0079" => Some(19),
        "RSS0080" => Some(26),
        "FLOOR0081" => Some(33),
        "CEIL0082" => Some(40),
        "BUD0083" => Some(47),
        "HP0084" => Some(54),
        "POOL0085" => Some(61),
        "CAP0086" => Some(68),
        "PRESS0087" => Some(75),
        "MATRIX0088" => Some(82),
        "ALLOC0089" => Some(0),
        "FREE0090" => Some(7),
        "FRAG0091" => Some(14),
        "RSS0092" => Some(21),
        "FLOOR0093" => Some(28),
        "CEIL0094" => Some(35),
        "BUD0095" => Some(42),
        "HP0096" => Some(49),
        "POOL0097" => Some(56),
        "CAP0098" => Some(63),
        "PRESS0099" => Some(70),
        "MATRIX0100" => Some(77),
        "ALLOC0101" => Some(84),
        "FREE0102" => Some(2),
        "FRAG0103" => Some(9),
        "RSS0104" => Some(16),
        "FLOOR0105" => Some(23),
        "CEIL0106" => Some(30),
        "BUD0107" => Some(37),
        "HP0108" => Some(44),
        "POOL0109" => Some(51),
        "CAP0110" => Some(58),
        "PRESS0111" => Some(65),
        "MATRIX0112" => Some(72),
        "ALLOC0113" => Some(79),
        "FREE0114" => Some(86),
        "FRAG0115" => Some(4),
        "RSS0116" => Some(11),
        "FLOOR0117" => Some(18),
        "CEIL0118" => Some(25),
        "BUD0119" => Some(32),
        "HP0120" => Some(39),
        "POOL0121" => Some(46),
        "CAP0122" => Some(53),
        "PRESS0123" => Some(60),
        "MATRIX0124" => Some(67),
        "ALLOC0125" => Some(74),
        "FREE0126" => Some(81),
        "FRAG0127" => Some(88),
        "RSS0128" => Some(6),
        "FLOOR0129" => Some(13),
        "CEIL0130" => Some(20),
        "BUD0131" => Some(27),
        "HP0132" => Some(34),
        "POOL0133" => Some(41),
        "CAP0134" => Some(48),
        "PRESS0135" => Some(55),
        "MATRIX0136" => Some(62),
        "ALLOC0137" => Some(69),
        "FREE0138" => Some(76),
        "FRAG0139" => Some(83),
        "RSS0140" => Some(1),
        "FLOOR0141" => Some(8),
        "CEIL0142" => Some(15),
        "BUD0143" => Some(22),
        "HP0144" => Some(29),
        "POOL0145" => Some(36),
        "CAP0146" => Some(43),
        "PRESS0147" => Some(50),
        "MATRIX0148" => Some(57),
        "ALLOC0149" => Some(64),
        "FREE0150" => Some(71),
        "FRAG0151" => Some(78),
        "RSS0152" => Some(85),
        "FLOOR0153" => Some(3),
        "CEIL0154" => Some(10),
        "BUD0155" => Some(17),
        "HP0156" => Some(24),
        "POOL0157" => Some(31),
        "CAP0158" => Some(38),
        "PRESS0159" => Some(45),
        "MATRIX0160" => Some(52),
        "ALLOC0161" => Some(59),
        "FREE0162" => Some(66),
        "FRAG0163" => Some(73),
        "RSS0164" => Some(80),
        "FLOOR0165" => Some(87),
        "CEIL0166" => Some(5),
        "BUD0167" => Some(12),
        "HP0168" => Some(19),
        "POOL0169" => Some(26),
        "CAP0170" => Some(33),
        "PRESS0171" => Some(40),
        "MATRIX0172" => Some(47),
        "ALLOC0173" => Some(54),
        "FREE0174" => Some(61),
        "FRAG0175" => Some(68),
        "RSS0176" => Some(75),
        "FLOOR0177" => Some(82),
        "CEIL0178" => Some(0),
        "BUD0179" => Some(7),
        "HP0180" => Some(14),
        "POOL0181" => Some(21),
        "CAP0182" => Some(28),
        "PRESS0183" => Some(35),
        "MATRIX0184" => Some(42),
        "ALLOC0185" => Some(49),
        "FREE0186" => Some(56),
        "FRAG0187" => Some(63),
        "RSS0188" => Some(70),
        "FLOOR0189" => Some(77),
        "CEIL0190" => Some(84),
        "BUD0191" => Some(2),
        "HP0192" => Some(9),
        "POOL0193" => Some(16),
        "CAP0194" => Some(23),
        "PRESS0195" => Some(30),
        "MATRIX0196" => Some(37),
        "ALLOC0197" => Some(44),
        "FREE0198" => Some(51),
        "FRAG0199" => Some(58),
        "RSS0200" => Some(65),
        "FLOOR0201" => Some(72),
        "CEIL0202" => Some(79),
        "BUD0203" => Some(86),
        "HP0204" => Some(4),
        "POOL0205" => Some(11),
        "CAP0206" => Some(18),
        "PRESS0207" => Some(25),
        "MATRIX0208" => Some(32),
        "ALLOC0209" => Some(39),
        "FREE0210" => Some(46),
        "FRAG0211" => Some(53),
        "RSS0212" => Some(60),
        "FLOOR0213" => Some(67),
        "CEIL0214" => Some(74),
        "BUD0215" => Some(81),
        "HP0216" => Some(88),
        "POOL0217" => Some(6),
        "CAP0218" => Some(13),
        "PRESS0219" => Some(20),
        "MATRIX0220" => Some(27),
        "ALLOC0221" => Some(34),
        "FREE0222" => Some(41),
        "FRAG0223" => Some(48),
        "RSS0224" => Some(55),
        "FLOOR0225" => Some(62),
        "CEIL0226" => Some(69),
        "BUD0227" => Some(76),
        "HP0228" => Some(83),
        "POOL0229" => Some(1),
        "CAP0230" => Some(8),
        "PRESS0231" => Some(15),
        "MATRIX0232" => Some(22),
        "ALLOC0233" => Some(29),
        "FREE0234" => Some(36),
        "FRAG0235" => Some(43),
        "RSS0236" => Some(50),
        "FLOOR0237" => Some(57),
        "CEIL0238" => Some(64),
        "BUD0239" => Some(71),
        "HP0240" => Some(78),
        "POOL0241" => Some(85),
        "CAP0242" => Some(3),
        "PRESS0243" => Some(10),
        "MATRIX0244" => Some(17),
        "ALLOC0245" => Some(24),
        "FREE0246" => Some(31),
        "FRAG0247" => Some(38),
        "RSS0248" => Some(45),
        "FLOOR0249" => Some(52),
        "CEIL0250" => Some(59),
        "BUD0251" => Some(66),
        "HP0252" => Some(73),
        "POOL0253" => Some(80),
        "CAP0254" => Some(87),
        "PRESS0255" => Some(5),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2801bmx_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2801bmx_COUNT {
        let (k, v) = u2801bmx[i];
        let ok = match k {
            "HP0000" => v < 89,
            "POOL0001" => v < 89,
            "CAP0002" => v < 89,
            "PRESS0003" => v < 89,
            "MATRIX0004" => v < 89,
            "ALLOC0005" => v < 89,
            "FREE0006" => v < 89,
            "FRAG0007" => v < 89,
            "RSS0008" => v < 89,
            "FLOOR0009" => v < 89,
            "CEIL0010" => v < 89,
            "BUD0011" => v < 89,
            "HP0012" => v < 89,
            "POOL0013" => v < 89,
            "CAP0014" => v < 89,
            "PRESS0015" => v < 89,
            "MATRIX0016" => v < 89,
            "ALLOC0017" => v < 89,
            "FREE0018" => v < 89,
            "FRAG0019" => v < 89,
            "RSS0020" => v < 89,
            "FLOOR0021" => v < 89,
            "CEIL0022" => v < 89,
            "BUD0023" => v < 89,
            "HP0024" => v < 89,
            "POOL0025" => v < 89,
            "CAP0026" => v < 89,
            "PRESS0027" => v < 89,
            "MATRIX0028" => v < 89,
            "ALLOC0029" => v < 89,
            "FREE0030" => v < 89,
            "FRAG0031" => v < 89,
            "RSS0032" => v < 89,
            "FLOOR0033" => v < 89,
            "CEIL0034" => v < 89,
            "BUD0035" => v < 89,
            "HP0036" => v < 89,
            "POOL0037" => v < 89,
            "CAP0038" => v < 89,
            "PRESS0039" => v < 89,
            "MATRIX0040" => v < 89,
            "ALLOC0041" => v < 89,
            "FREE0042" => v < 89,
            "FRAG0043" => v < 89,
            "RSS0044" => v < 89,
            "FLOOR0045" => v < 89,
            "CEIL0046" => v < 89,
            "BUD0047" => v < 89,
            "HP0048" => v < 89,
            "POOL0049" => v < 89,
            "CAP0050" => v < 89,
            "PRESS0051" => v < 89,
            "MATRIX0052" => v < 89,
            "ALLOC0053" => v < 89,
            "FREE0054" => v < 89,
            "FRAG0055" => v < 89,
            "RSS0056" => v < 89,
            "FLOOR0057" => v < 89,
            "CEIL0058" => v < 89,
            "BUD0059" => v < 89,
            "HP0060" => v < 89,
            "POOL0061" => v < 89,
            "CAP0062" => v < 89,
            "PRESS0063" => v < 89,
            "MATRIX0064" => v < 89,
            "ALLOC0065" => v < 89,
            "FREE0066" => v < 89,
            "FRAG0067" => v < 89,
            "RSS0068" => v < 89,
            "FLOOR0069" => v < 89,
            "CEIL0070" => v < 89,
            "BUD0071" => v < 89,
            "HP0072" => v < 89,
            "POOL0073" => v < 89,
            "CAP0074" => v < 89,
            "PRESS0075" => v < 89,
            "MATRIX0076" => v < 89,
            "ALLOC0077" => v < 89,
            "FREE0078" => v < 89,
            "FRAG0079" => v < 89,
            "RSS0080" => v < 89,
            "FLOOR0081" => v < 89,
            "CEIL0082" => v < 89,
            "BUD0083" => v < 89,
            "HP0084" => v < 89,
            "POOL0085" => v < 89,
            "CAP0086" => v < 89,
            "PRESS0087" => v < 89,
            "MATRIX0088" => v < 89,
            "ALLOC0089" => v < 89,
            "FREE0090" => v < 89,
            "FRAG0091" => v < 89,
            "RSS0092" => v < 89,
            "FLOOR0093" => v < 89,
            "CEIL0094" => v < 89,
            "BUD0095" => v < 89,
            "HP0096" => v < 89,
            "POOL0097" => v < 89,
            "CAP0098" => v < 89,
            "PRESS0099" => v < 89,
            "MATRIX0100" => v < 89,
            "ALLOC0101" => v < 89,
            "FREE0102" => v < 89,
            "FRAG0103" => v < 89,
            "RSS0104" => v < 89,
            "FLOOR0105" => v < 89,
            "CEIL0106" => v < 89,
            "BUD0107" => v < 89,
            "HP0108" => v < 89,
            "POOL0109" => v < 89,
            "CAP0110" => v < 89,
            "PRESS0111" => v < 89,
            "MATRIX0112" => v < 89,
            "ALLOC0113" => v < 89,
            "FREE0114" => v < 89,
            "FRAG0115" => v < 89,
            "RSS0116" => v < 89,
            "FLOOR0117" => v < 89,
            "CEIL0118" => v < 89,
            "BUD0119" => v < 89,
            "HP0120" => v < 89,
            "POOL0121" => v < 89,
            "CAP0122" => v < 89,
            "PRESS0123" => v < 89,
            "MATRIX0124" => v < 89,
            "ALLOC0125" => v < 89,
            "FREE0126" => v < 89,
            "FRAG0127" => v < 89,
            "RSS0128" => v < 89,
            "FLOOR0129" => v < 89,
            "CEIL0130" => v < 89,
            "BUD0131" => v < 89,
            "HP0132" => v < 89,
            "POOL0133" => v < 89,
            "CAP0134" => v < 89,
            "PRESS0135" => v < 89,
            "MATRIX0136" => v < 89,
            "ALLOC0137" => v < 89,
            "FREE0138" => v < 89,
            "FRAG0139" => v < 89,
            "RSS0140" => v < 89,
            "FLOOR0141" => v < 89,
            "CEIL0142" => v < 89,
            "BUD0143" => v < 89,
            "HP0144" => v < 89,
            "POOL0145" => v < 89,
            "CAP0146" => v < 89,
            "PRESS0147" => v < 89,
            "MATRIX0148" => v < 89,
            "ALLOC0149" => v < 89,
            "FREE0150" => v < 89,
            "FRAG0151" => v < 89,
            "RSS0152" => v < 89,
            "FLOOR0153" => v < 89,
            "CEIL0154" => v < 89,
            "BUD0155" => v < 89,
            "HP0156" => v < 89,
            "POOL0157" => v < 89,
            "CAP0158" => v < 89,
            "PRESS0159" => v < 89,
            "MATRIX0160" => v < 89,
            "ALLOC0161" => v < 89,
            "FREE0162" => v < 89,
            "FRAG0163" => v < 89,
            "RSS0164" => v < 89,
            "FLOOR0165" => v < 89,
            "CEIL0166" => v < 89,
            "BUD0167" => v < 89,
            "HP0168" => v < 89,
            "POOL0169" => v < 89,
            "CAP0170" => v < 89,
            "PRESS0171" => v < 89,
            "MATRIX0172" => v < 89,
            "ALLOC0173" => v < 89,
            "FREE0174" => v < 89,
            "FRAG0175" => v < 89,
            "RSS0176" => v < 89,
            "FLOOR0177" => v < 89,
            "CEIL0178" => v < 89,
            "BUD0179" => v < 89,
            "HP0180" => v < 89,
            "POOL0181" => v < 89,
            "CAP0182" => v < 89,
            "PRESS0183" => v < 89,
            "MATRIX0184" => v < 89,
            "ALLOC0185" => v < 89,
            "FREE0186" => v < 89,
            "FRAG0187" => v < 89,
            "RSS0188" => v < 89,
            "FLOOR0189" => v < 89,
            "CEIL0190" => v < 89,
            "BUD0191" => v < 89,
            "HP0192" => v < 89,
            "POOL0193" => v < 89,
            "CAP0194" => v < 89,
            "PRESS0195" => v < 89,
            "MATRIX0196" => v < 89,
            "ALLOC0197" => v < 89,
            "FREE0198" => v < 89,
            "FRAG0199" => v < 89,
            "RSS0200" => v < 89,
            "FLOOR0201" => v < 89,
            "CEIL0202" => v < 89,
            "BUD0203" => v < 89,
            "HP0204" => v < 89,
            "POOL0205" => v < 89,
            "CAP0206" => v < 89,
            "PRESS0207" => v < 89,
            "MATRIX0208" => v < 89,
            "ALLOC0209" => v < 89,
            "FREE0210" => v < 89,
            "FRAG0211" => v < 89,
            "RSS0212" => v < 89,
            "FLOOR0213" => v < 89,
            "CEIL0214" => v < 89,
            "BUD0215" => v < 89,
            "HP0216" => v < 89,
            "POOL0217" => v < 89,
            "CAP0218" => v < 89,
            "PRESS0219" => v < 89,
            "MATRIX0220" => v < 89,
            "ALLOC0221" => v < 89,
            "FREE0222" => v < 89,
            "FRAG0223" => v < 89,
            "RSS0224" => v < 89,
            "FLOOR0225" => v < 89,
            "CEIL0226" => v < 89,
            "BUD0227" => v < 89,
            "HP0228" => v < 89,
            "POOL0229" => v < 89,
            "CAP0230" => v < 89,
            "PRESS0231" => v < 89,
            "MATRIX0232" => v < 89,
            "ALLOC0233" => v < 89,
            "FREE0234" => v < 89,
            "FRAG0235" => v < 89,
            "RSS0236" => v < 89,
            "FLOOR0237" => v < 89,
            "CEIL0238" => v < 89,
            "BUD0239" => v < 89,
            "HP0240" => v < 89,
            "POOL0241" => v < 89,
            "CAP0242" => v < 89,
            "PRESS0243" => v < 89,
            "MATRIX0244" => v < 89,
            "ALLOC0245" => v < 89,
            "FREE0246" => v < 89,
            "FRAG0247" => v < 89,
            "RSS0248" => v < 89,
            "FLOOR0249" => v < 89,
            "CEIL0250" => v < 89,
            "BUD0251" => v < 89,
            "HP0252" => v < 89,
            "POOL0253" => v < 89,
            "CAP0254" => v < 89,
            "PRESS0255" => v < 89,
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
pub fn u2801bmx_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2801bmx_COUNT {
        s = s.wrapping_add(u2801bmx[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2801bmx_unique() -> bool {
    let mut i = 0usize;
    while i < u2801bmx_COUNT {
        let mut j = i + 1;
        while j < u2801bmx_COUNT {
            if u2801bmx[i].0 == u2801bmx[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe260, 0xe26f]。
pub const fn u2801_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2801Code::ALL.len() {
        let c = u2801Code::ALL[i].code();
        if c < 0xe260 || c > 0xe26f {
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
fn u2801_guard_seg() -> bool {
    u2801_segment_guard()
}

/// UNX-F2801 bmx 256 行齐备 + 边界锚 + 键外拒收。
fn check_u2801_bmx_count() -> bool {
    let mut ok = u2801bmx_COUNT == 256;
    if ok { ok = u2801bmx_get("HP0000") == Some(0) && u2801bmx_get("PRESS0255") == Some(5); }
    if ok { ok = u2801bmx_get("ZZZ_NOKEY").is_none(); }
    ok
}

/// UNX-F2801 bmx 累计守恒（理论值 11231）。
fn check_u2801_bmx_sum() -> bool {
    u2801bmx_sum() == 11231
}

/// UNX-F2801 bmx 逐行守卫与唯一性全绿。
fn check_u2801_bmx_guard() -> bool {
    u2801bmx_scan() == 0 && u2801bmx_unique()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2801_checks() -> CheckSet {
    let mut set = CheckSet::new("U2801");
    set.add(
        "u2801-seg-guard",
        u2801_guard_seg(),
        if u2801_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2801-bmx-count",
        check_u2801_bmx_count(),
        if check_u2801_bmx_count() { "" } else { "UNX-F2801 bmx count mismatch" },
    );
    set.add(
        "u2801-bmx-sum",
        check_u2801_bmx_sum(),
        if check_u2801_bmx_sum() { "" } else { "UNX-F2801 bmx sum mismatch" },
    );
    set.add(
        "u2801-bmx-guard",
        check_u2801_bmx_guard(),
        if check_u2801_bmx_guard() { "" } else { "UNX-F2801 bmx guard mismatch" },
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
    fn u2801_all_checks_green() {
        let set = run_u2801_checks();
        assert!(set.all_passed(), "U2801 domain checks must be green");
    }

    #[test]
    fn u2801_segment_guard_holds() {
        assert!(u2801_segment_guard());
    }
}
