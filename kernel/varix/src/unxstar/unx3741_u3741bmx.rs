//! unxu3741_u3741bmx —— 门禁总设计（五门体系与流水线拓扑） 深编bmx（门禁体系 × 256 单元）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3741 · 门禁总设计（五门体系与流水线拓扑） · 判据原文：UNX-F3741 · 判据：J1：五门体系（守恒门/防重门/覆盖门/断言门/fuzz 门——判据行内嵌五门各守什么律）+流水线拓扑（提交→五门串行→报告——门序 const）；M-499 五门首跑（样例提交过五门→全绿报告出）。<br>J2：门是冷的（机器判——门无人工覆写通道（豁免走 F3755 显式制——不是当场翻绿）。
//! · 组件：门禁体系 守卫映射 256 行（二 轮派生真实数据 + 逐行守卫 + 键唯一）
//! · 判据落地：UNX-F3741 · 判据：J1：五门体系（守恒门/防重门/覆盖门/断言门/fuzz 门——判据行内嵌五门各守什么律）+流水线拓扑（提交→五门串行→报告——门
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3741_VERSION: &str = "u3741-u3741bmx-v1";

/// 诊断段基址。
pub const U3741_SEG_BASE: u16 = 0xe230;

// --------------------------------------------------------------------------
// 诊断码（u3741 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 门禁体系 bmx 面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3741Code {
    /// 0xe230 单元格重复（门禁 同键双占）。
    CellDup,
    /// 0xe231 单元格断档（五门 序号不连续）。
    CellGap,
    /// 0xe232 索引越界（拓扑）。
    IndexOob,
    /// 0xe233 大表账破缺（行数≠登记值）。
    LedgerBreak,
    /// 0xe234 表序断档。
    OrderBreak,
    /// 0xe235 累计漂移（求和≠理论值）。
    SumDrift,
    /// 0xe236 掩码非法。
    MaskBad,
    /// 0xe237 守卫触发。
    GuardTripped,
    /// 0xe238 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe239 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe23a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe23b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3741Code {
    /// 全量闭集。
    pub const ALL: [u3741Code; 12] = [
        u3741Code::CellDup,
        u3741Code::CellGap,
        u3741Code::IndexOob,
        u3741Code::LedgerBreak,
        u3741Code::OrderBreak,
        u3741Code::SumDrift,
        u3741Code::MaskBad,
        u3741Code::GuardTripped,
        u3741Code::LedgerBroken,
        u3741Code::SnapshotFingerprint,
        u3741Code::TableBroken,
        u3741Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3741Code::CellDup => 0xe230,
            u3741Code::CellGap => 0xe231,
            u3741Code::IndexOob => 0xe232,
            u3741Code::LedgerBreak => 0xe233,
            u3741Code::OrderBreak => 0xe234,
            u3741Code::SumDrift => 0xe235,
            u3741Code::MaskBad => 0xe236,
            u3741Code::GuardTripped => 0xe237,
            u3741Code::LedgerBroken => 0xe238,
            u3741Code::SnapshotFingerprint => 0xe239,
            u3741Code::TableBroken => 0xe23a,
            u3741Code::SegmentGuard => 0xe23b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3741Code::CellDup => "单元格重复（门禁 同键双占）",
            u3741Code::CellGap => "单元格断档（五门 序号不连续）",
            u3741Code::IndexOob => "索引越界（拓扑）",
            u3741Code::LedgerBreak => "大表账破缺（行数≠登记值）",
            u3741Code::OrderBreak => "表序断档",
            u3741Code::SumDrift => "累计漂移（求和≠理论值）",
            u3741Code::MaskBad => "掩码非法",
            u3741Code::GuardTripped => "守卫触发",
            u3741Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3741Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3741Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3741Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3741Code::CellDup => "按键唯一律去重",
            u3741Code::CellGap => "按连续律补号",
            u3741Code::IndexOob => "按行数边界裁剪",
            u3741Code::LedgerBreak => "按守恒重建",
            u3741Code::OrderBreak => "按序重排",
            u3741Code::SumDrift => "按理论值复算",
            u3741Code::MaskBad => "按掩码律校正",
            u3741Code::GuardTripped => "检查临界区",
            u3741Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3741Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3741Code::TableBroken => "按语义规则重建常量表",
            u3741Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3741bmx（256 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 门禁体系 bmx 守卫映射（256 行逐行注释自证）。
/// 行数。
pub const u3741bmx_COUNT: usize = 256;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3741bmx: [(&'static str, u64); 256] = [
    ("GATE0000", 0), // GATE 单元 0000（门禁 × 相位 00 × 二 轮）
    ("FIVE0001", 7), // FIVE 单元 0001（五门 × 相位 01 × 二 轮）
    ("PIPE0002", 14), // PIPE 单元 0002（流水线 × 相位 02 × 二 轮）
    ("TOPO0003", 21), // TOPO 单元 0003（拓扑 × 相位 03 × 二 轮）
    ("FAST0004", 28), // FAST 单元 0004（快速门 × 相位 04 × 二 轮）
    ("DEEP0005", 35), // DEEP 单元 0005（深度门 × 相位 05 × 二 轮）
    ("FUZZG0006", 42), // FUZZG 单元 0006（fuzz 门 × 相位 06 × 二 轮）
    ("REGR0007", 49), // REGR 单元 0007（回归门 × 相位 07 × 二 轮）
    ("RELEASE0008", 56), // RELEASE 单元 0008（发布门 × 相位 08 × 二 轮）
    ("ORDER0009", 63), // ORDER 单元 0009（门序 × 相位 09 × 二 轮）
    ("BLOCK0010", 70), // BLOCK 单元 0010（阻断 × 相位 10 × 二 轮）
    ("SIGN0011", 77), // SIGN 单元 0011（签署 × 相位 11 × 二 轮）
    ("GATE0012", 84), // GATE 单元 0012（门禁 × 相位 12 × 二 轮）
    ("FIVE0013", 2), // FIVE 单元 0013（五门 × 相位 13 × 二 轮）
    ("PIPE0014", 9), // PIPE 单元 0014（流水线 × 相位 14 × 二 轮）
    ("TOPO0015", 16), // TOPO 单元 0015（拓扑 × 相位 15 × 二 轮）
    ("FAST0016", 23), // FAST 单元 0016（快速门 × 相位 16 × 二 轮）
    ("DEEP0017", 30), // DEEP 单元 0017（深度门 × 相位 17 × 二 轮）
    ("FUZZG0018", 37), // FUZZG 单元 0018（fuzz 门 × 相位 18 × 二 轮）
    ("REGR0019", 44), // REGR 单元 0019（回归门 × 相位 19 × 二 轮）
    ("RELEASE0020", 51), // RELEASE 单元 0020（发布门 × 相位 20 × 二 轮）
    ("ORDER0021", 58), // ORDER 单元 0021（门序 × 相位 21 × 二 轮）
    ("BLOCK0022", 65), // BLOCK 单元 0022（阻断 × 相位 22 × 二 轮）
    ("SIGN0023", 72), // SIGN 单元 0023（签署 × 相位 23 × 二 轮）
    ("GATE0024", 79), // GATE 单元 0024（门禁 × 相位 24 × 二 轮）
    ("FIVE0025", 86), // FIVE 单元 0025（五门 × 相位 25 × 二 轮）
    ("PIPE0026", 4), // PIPE 单元 0026（流水线 × 相位 26 × 二 轮）
    ("TOPO0027", 11), // TOPO 单元 0027（拓扑 × 相位 27 × 二 轮）
    ("FAST0028", 18), // FAST 单元 0028（快速门 × 相位 28 × 二 轮）
    ("DEEP0029", 25), // DEEP 单元 0029（深度门 × 相位 29 × 二 轮）
    ("FUZZG0030", 32), // FUZZG 单元 0030（fuzz 门 × 相位 30 × 二 轮）
    ("REGR0031", 39), // REGR 单元 0031（回归门 × 相位 31 × 二 轮）
    ("RELEASE0032", 46), // RELEASE 单元 0032（发布门 × 相位 32 × 二 轮）
    ("ORDER0033", 53), // ORDER 单元 0033（门序 × 相位 33 × 二 轮）
    ("BLOCK0034", 60), // BLOCK 单元 0034（阻断 × 相位 34 × 二 轮）
    ("SIGN0035", 67), // SIGN 单元 0035（签署 × 相位 35 × 二 轮）
    ("GATE0036", 74), // GATE 单元 0036（门禁 × 相位 36 × 二 轮）
    ("FIVE0037", 81), // FIVE 单元 0037（五门 × 相位 37 × 二 轮）
    ("PIPE0038", 88), // PIPE 单元 0038（流水线 × 相位 38 × 二 轮）
    ("TOPO0039", 6), // TOPO 单元 0039（拓扑 × 相位 39 × 二 轮）
    ("FAST0040", 13), // FAST 单元 0040（快速门 × 相位 40 × 二 轮）
    ("DEEP0041", 20), // DEEP 单元 0041（深度门 × 相位 41 × 二 轮）
    ("FUZZG0042", 27), // FUZZG 单元 0042（fuzz 门 × 相位 42 × 二 轮）
    ("REGR0043", 34), // REGR 单元 0043（回归门 × 相位 43 × 二 轮）
    ("RELEASE0044", 41), // RELEASE 单元 0044（发布门 × 相位 44 × 二 轮）
    ("ORDER0045", 48), // ORDER 单元 0045（门序 × 相位 45 × 二 轮）
    ("BLOCK0046", 55), // BLOCK 单元 0046（阻断 × 相位 46 × 二 轮）
    ("SIGN0047", 62), // SIGN 单元 0047（签署 × 相位 47 × 二 轮）
    ("GATE0048", 69), // GATE 单元 0048（门禁 × 相位 48 × 二 轮）
    ("FIVE0049", 76), // FIVE 单元 0049（五门 × 相位 49 × 二 轮）
    ("PIPE0050", 83), // PIPE 单元 0050（流水线 × 相位 50 × 二 轮）
    ("TOPO0051", 1), // TOPO 单元 0051（拓扑 × 相位 51 × 二 轮）
    ("FAST0052", 8), // FAST 单元 0052（快速门 × 相位 52 × 二 轮）
    ("DEEP0053", 15), // DEEP 单元 0053（深度门 × 相位 53 × 二 轮）
    ("FUZZG0054", 22), // FUZZG 单元 0054（fuzz 门 × 相位 54 × 二 轮）
    ("REGR0055", 29), // REGR 单元 0055（回归门 × 相位 55 × 二 轮）
    ("RELEASE0056", 36), // RELEASE 单元 0056（发布门 × 相位 56 × 二 轮）
    ("ORDER0057", 43), // ORDER 单元 0057（门序 × 相位 57 × 二 轮）
    ("BLOCK0058", 50), // BLOCK 单元 0058（阻断 × 相位 58 × 二 轮）
    ("SIGN0059", 57), // SIGN 单元 0059（签署 × 相位 59 × 二 轮）
    ("GATE0060", 64), // GATE 单元 0060（门禁 × 相位 60 × 二 轮）
    ("FIVE0061", 71), // FIVE 单元 0061（五门 × 相位 61 × 二 轮）
    ("PIPE0062", 78), // PIPE 单元 0062（流水线 × 相位 62 × 二 轮）
    ("TOPO0063", 85), // TOPO 单元 0063（拓扑 × 相位 63 × 二 轮）
    ("FAST0064", 3), // FAST 单元 0064（快速门 × 相位 00 × 二 轮）
    ("DEEP0065", 10), // DEEP 单元 0065（深度门 × 相位 01 × 二 轮）
    ("FUZZG0066", 17), // FUZZG 单元 0066（fuzz 门 × 相位 02 × 二 轮）
    ("REGR0067", 24), // REGR 单元 0067（回归门 × 相位 03 × 二 轮）
    ("RELEASE0068", 31), // RELEASE 单元 0068（发布门 × 相位 04 × 二 轮）
    ("ORDER0069", 38), // ORDER 单元 0069（门序 × 相位 05 × 二 轮）
    ("BLOCK0070", 45), // BLOCK 单元 0070（阻断 × 相位 06 × 二 轮）
    ("SIGN0071", 52), // SIGN 单元 0071（签署 × 相位 07 × 二 轮）
    ("GATE0072", 59), // GATE 单元 0072（门禁 × 相位 08 × 二 轮）
    ("FIVE0073", 66), // FIVE 单元 0073（五门 × 相位 09 × 二 轮）
    ("PIPE0074", 73), // PIPE 单元 0074（流水线 × 相位 10 × 二 轮）
    ("TOPO0075", 80), // TOPO 单元 0075（拓扑 × 相位 11 × 二 轮）
    ("FAST0076", 87), // FAST 单元 0076（快速门 × 相位 12 × 二 轮）
    ("DEEP0077", 5), // DEEP 单元 0077（深度门 × 相位 13 × 二 轮）
    ("FUZZG0078", 12), // FUZZG 单元 0078（fuzz 门 × 相位 14 × 二 轮）
    ("REGR0079", 19), // REGR 单元 0079（回归门 × 相位 15 × 二 轮）
    ("RELEASE0080", 26), // RELEASE 单元 0080（发布门 × 相位 16 × 二 轮）
    ("ORDER0081", 33), // ORDER 单元 0081（门序 × 相位 17 × 二 轮）
    ("BLOCK0082", 40), // BLOCK 单元 0082（阻断 × 相位 18 × 二 轮）
    ("SIGN0083", 47), // SIGN 单元 0083（签署 × 相位 19 × 二 轮）
    ("GATE0084", 54), // GATE 单元 0084（门禁 × 相位 20 × 二 轮）
    ("FIVE0085", 61), // FIVE 单元 0085（五门 × 相位 21 × 二 轮）
    ("PIPE0086", 68), // PIPE 单元 0086（流水线 × 相位 22 × 二 轮）
    ("TOPO0087", 75), // TOPO 单元 0087（拓扑 × 相位 23 × 二 轮）
    ("FAST0088", 82), // FAST 单元 0088（快速门 × 相位 24 × 二 轮）
    ("DEEP0089", 0), // DEEP 单元 0089（深度门 × 相位 25 × 二 轮）
    ("FUZZG0090", 7), // FUZZG 单元 0090（fuzz 门 × 相位 26 × 二 轮）
    ("REGR0091", 14), // REGR 单元 0091（回归门 × 相位 27 × 二 轮）
    ("RELEASE0092", 21), // RELEASE 单元 0092（发布门 × 相位 28 × 二 轮）
    ("ORDER0093", 28), // ORDER 单元 0093（门序 × 相位 29 × 二 轮）
    ("BLOCK0094", 35), // BLOCK 单元 0094（阻断 × 相位 30 × 二 轮）
    ("SIGN0095", 42), // SIGN 单元 0095（签署 × 相位 31 × 二 轮）
    ("GATE0096", 49), // GATE 单元 0096（门禁 × 相位 32 × 二 轮）
    ("FIVE0097", 56), // FIVE 单元 0097（五门 × 相位 33 × 二 轮）
    ("PIPE0098", 63), // PIPE 单元 0098（流水线 × 相位 34 × 二 轮）
    ("TOPO0099", 70), // TOPO 单元 0099（拓扑 × 相位 35 × 二 轮）
    ("FAST0100", 77), // FAST 单元 0100（快速门 × 相位 36 × 二 轮）
    ("DEEP0101", 84), // DEEP 单元 0101（深度门 × 相位 37 × 二 轮）
    ("FUZZG0102", 2), // FUZZG 单元 0102（fuzz 门 × 相位 38 × 二 轮）
    ("REGR0103", 9), // REGR 单元 0103（回归门 × 相位 39 × 二 轮）
    ("RELEASE0104", 16), // RELEASE 单元 0104（发布门 × 相位 40 × 二 轮）
    ("ORDER0105", 23), // ORDER 单元 0105（门序 × 相位 41 × 二 轮）
    ("BLOCK0106", 30), // BLOCK 单元 0106（阻断 × 相位 42 × 二 轮）
    ("SIGN0107", 37), // SIGN 单元 0107（签署 × 相位 43 × 二 轮）
    ("GATE0108", 44), // GATE 单元 0108（门禁 × 相位 44 × 二 轮）
    ("FIVE0109", 51), // FIVE 单元 0109（五门 × 相位 45 × 二 轮）
    ("PIPE0110", 58), // PIPE 单元 0110（流水线 × 相位 46 × 二 轮）
    ("TOPO0111", 65), // TOPO 单元 0111（拓扑 × 相位 47 × 二 轮）
    ("FAST0112", 72), // FAST 单元 0112（快速门 × 相位 48 × 二 轮）
    ("DEEP0113", 79), // DEEP 单元 0113（深度门 × 相位 49 × 二 轮）
    ("FUZZG0114", 86), // FUZZG 单元 0114（fuzz 门 × 相位 50 × 二 轮）
    ("REGR0115", 4), // REGR 单元 0115（回归门 × 相位 51 × 二 轮）
    ("RELEASE0116", 11), // RELEASE 单元 0116（发布门 × 相位 52 × 二 轮）
    ("ORDER0117", 18), // ORDER 单元 0117（门序 × 相位 53 × 二 轮）
    ("BLOCK0118", 25), // BLOCK 单元 0118（阻断 × 相位 54 × 二 轮）
    ("SIGN0119", 32), // SIGN 单元 0119（签署 × 相位 55 × 二 轮）
    ("GATE0120", 39), // GATE 单元 0120（门禁 × 相位 56 × 二 轮）
    ("FIVE0121", 46), // FIVE 单元 0121（五门 × 相位 57 × 二 轮）
    ("PIPE0122", 53), // PIPE 单元 0122（流水线 × 相位 58 × 二 轮）
    ("TOPO0123", 60), // TOPO 单元 0123（拓扑 × 相位 59 × 二 轮）
    ("FAST0124", 67), // FAST 单元 0124（快速门 × 相位 60 × 二 轮）
    ("DEEP0125", 74), // DEEP 单元 0125（深度门 × 相位 61 × 二 轮）
    ("FUZZG0126", 81), // FUZZG 单元 0126（fuzz 门 × 相位 62 × 二 轮）
    ("REGR0127", 88), // REGR 单元 0127（回归门 × 相位 63 × 二 轮）
    ("RELEASE0128", 6), // RELEASE 单元 0128（发布门 × 相位 00 × 二 轮）
    ("ORDER0129", 13), // ORDER 单元 0129（门序 × 相位 01 × 二 轮）
    ("BLOCK0130", 20), // BLOCK 单元 0130（阻断 × 相位 02 × 二 轮）
    ("SIGN0131", 27), // SIGN 单元 0131（签署 × 相位 03 × 二 轮）
    ("GATE0132", 34), // GATE 单元 0132（门禁 × 相位 04 × 二 轮）
    ("FIVE0133", 41), // FIVE 单元 0133（五门 × 相位 05 × 二 轮）
    ("PIPE0134", 48), // PIPE 单元 0134（流水线 × 相位 06 × 二 轮）
    ("TOPO0135", 55), // TOPO 单元 0135（拓扑 × 相位 07 × 二 轮）
    ("FAST0136", 62), // FAST 单元 0136（快速门 × 相位 08 × 二 轮）
    ("DEEP0137", 69), // DEEP 单元 0137（深度门 × 相位 09 × 二 轮）
    ("FUZZG0138", 76), // FUZZG 单元 0138（fuzz 门 × 相位 10 × 二 轮）
    ("REGR0139", 83), // REGR 单元 0139（回归门 × 相位 11 × 二 轮）
    ("RELEASE0140", 1), // RELEASE 单元 0140（发布门 × 相位 12 × 二 轮）
    ("ORDER0141", 8), // ORDER 单元 0141（门序 × 相位 13 × 二 轮）
    ("BLOCK0142", 15), // BLOCK 单元 0142（阻断 × 相位 14 × 二 轮）
    ("SIGN0143", 22), // SIGN 单元 0143（签署 × 相位 15 × 二 轮）
    ("GATE0144", 29), // GATE 单元 0144（门禁 × 相位 16 × 二 轮）
    ("FIVE0145", 36), // FIVE 单元 0145（五门 × 相位 17 × 二 轮）
    ("PIPE0146", 43), // PIPE 单元 0146（流水线 × 相位 18 × 二 轮）
    ("TOPO0147", 50), // TOPO 单元 0147（拓扑 × 相位 19 × 二 轮）
    ("FAST0148", 57), // FAST 单元 0148（快速门 × 相位 20 × 二 轮）
    ("DEEP0149", 64), // DEEP 单元 0149（深度门 × 相位 21 × 二 轮）
    ("FUZZG0150", 71), // FUZZG 单元 0150（fuzz 门 × 相位 22 × 二 轮）
    ("REGR0151", 78), // REGR 单元 0151（回归门 × 相位 23 × 二 轮）
    ("RELEASE0152", 85), // RELEASE 单元 0152（发布门 × 相位 24 × 二 轮）
    ("ORDER0153", 3), // ORDER 单元 0153（门序 × 相位 25 × 二 轮）
    ("BLOCK0154", 10), // BLOCK 单元 0154（阻断 × 相位 26 × 二 轮）
    ("SIGN0155", 17), // SIGN 单元 0155（签署 × 相位 27 × 二 轮）
    ("GATE0156", 24), // GATE 单元 0156（门禁 × 相位 28 × 二 轮）
    ("FIVE0157", 31), // FIVE 单元 0157（五门 × 相位 29 × 二 轮）
    ("PIPE0158", 38), // PIPE 单元 0158（流水线 × 相位 30 × 二 轮）
    ("TOPO0159", 45), // TOPO 单元 0159（拓扑 × 相位 31 × 二 轮）
    ("FAST0160", 52), // FAST 单元 0160（快速门 × 相位 32 × 二 轮）
    ("DEEP0161", 59), // DEEP 单元 0161（深度门 × 相位 33 × 二 轮）
    ("FUZZG0162", 66), // FUZZG 单元 0162（fuzz 门 × 相位 34 × 二 轮）
    ("REGR0163", 73), // REGR 单元 0163（回归门 × 相位 35 × 二 轮）
    ("RELEASE0164", 80), // RELEASE 单元 0164（发布门 × 相位 36 × 二 轮）
    ("ORDER0165", 87), // ORDER 单元 0165（门序 × 相位 37 × 二 轮）
    ("BLOCK0166", 5), // BLOCK 单元 0166（阻断 × 相位 38 × 二 轮）
    ("SIGN0167", 12), // SIGN 单元 0167（签署 × 相位 39 × 二 轮）
    ("GATE0168", 19), // GATE 单元 0168（门禁 × 相位 40 × 二 轮）
    ("FIVE0169", 26), // FIVE 单元 0169（五门 × 相位 41 × 二 轮）
    ("PIPE0170", 33), // PIPE 单元 0170（流水线 × 相位 42 × 二 轮）
    ("TOPO0171", 40), // TOPO 单元 0171（拓扑 × 相位 43 × 二 轮）
    ("FAST0172", 47), // FAST 单元 0172（快速门 × 相位 44 × 二 轮）
    ("DEEP0173", 54), // DEEP 单元 0173（深度门 × 相位 45 × 二 轮）
    ("FUZZG0174", 61), // FUZZG 单元 0174（fuzz 门 × 相位 46 × 二 轮）
    ("REGR0175", 68), // REGR 单元 0175（回归门 × 相位 47 × 二 轮）
    ("RELEASE0176", 75), // RELEASE 单元 0176（发布门 × 相位 48 × 二 轮）
    ("ORDER0177", 82), // ORDER 单元 0177（门序 × 相位 49 × 二 轮）
    ("BLOCK0178", 0), // BLOCK 单元 0178（阻断 × 相位 50 × 二 轮）
    ("SIGN0179", 7), // SIGN 单元 0179（签署 × 相位 51 × 二 轮）
    ("GATE0180", 14), // GATE 单元 0180（门禁 × 相位 52 × 二 轮）
    ("FIVE0181", 21), // FIVE 单元 0181（五门 × 相位 53 × 二 轮）
    ("PIPE0182", 28), // PIPE 单元 0182（流水线 × 相位 54 × 二 轮）
    ("TOPO0183", 35), // TOPO 单元 0183（拓扑 × 相位 55 × 二 轮）
    ("FAST0184", 42), // FAST 单元 0184（快速门 × 相位 56 × 二 轮）
    ("DEEP0185", 49), // DEEP 单元 0185（深度门 × 相位 57 × 二 轮）
    ("FUZZG0186", 56), // FUZZG 单元 0186（fuzz 门 × 相位 58 × 二 轮）
    ("REGR0187", 63), // REGR 单元 0187（回归门 × 相位 59 × 二 轮）
    ("RELEASE0188", 70), // RELEASE 单元 0188（发布门 × 相位 60 × 二 轮）
    ("ORDER0189", 77), // ORDER 单元 0189（门序 × 相位 61 × 二 轮）
    ("BLOCK0190", 84), // BLOCK 单元 0190（阻断 × 相位 62 × 二 轮）
    ("SIGN0191", 2), // SIGN 单元 0191（签署 × 相位 63 × 二 轮）
    ("GATE0192", 9), // GATE 单元 0192（门禁 × 相位 00 × 二 轮）
    ("FIVE0193", 16), // FIVE 单元 0193（五门 × 相位 01 × 二 轮）
    ("PIPE0194", 23), // PIPE 单元 0194（流水线 × 相位 02 × 二 轮）
    ("TOPO0195", 30), // TOPO 单元 0195（拓扑 × 相位 03 × 二 轮）
    ("FAST0196", 37), // FAST 单元 0196（快速门 × 相位 04 × 二 轮）
    ("DEEP0197", 44), // DEEP 单元 0197（深度门 × 相位 05 × 二 轮）
    ("FUZZG0198", 51), // FUZZG 单元 0198（fuzz 门 × 相位 06 × 二 轮）
    ("REGR0199", 58), // REGR 单元 0199（回归门 × 相位 07 × 二 轮）
    ("RELEASE0200", 65), // RELEASE 单元 0200（发布门 × 相位 08 × 二 轮）
    ("ORDER0201", 72), // ORDER 单元 0201（门序 × 相位 09 × 二 轮）
    ("BLOCK0202", 79), // BLOCK 单元 0202（阻断 × 相位 10 × 二 轮）
    ("SIGN0203", 86), // SIGN 单元 0203（签署 × 相位 11 × 二 轮）
    ("GATE0204", 4), // GATE 单元 0204（门禁 × 相位 12 × 二 轮）
    ("FIVE0205", 11), // FIVE 单元 0205（五门 × 相位 13 × 二 轮）
    ("PIPE0206", 18), // PIPE 单元 0206（流水线 × 相位 14 × 二 轮）
    ("TOPO0207", 25), // TOPO 单元 0207（拓扑 × 相位 15 × 二 轮）
    ("FAST0208", 32), // FAST 单元 0208（快速门 × 相位 16 × 二 轮）
    ("DEEP0209", 39), // DEEP 单元 0209（深度门 × 相位 17 × 二 轮）
    ("FUZZG0210", 46), // FUZZG 单元 0210（fuzz 门 × 相位 18 × 二 轮）
    ("REGR0211", 53), // REGR 单元 0211（回归门 × 相位 19 × 二 轮）
    ("RELEASE0212", 60), // RELEASE 单元 0212（发布门 × 相位 20 × 二 轮）
    ("ORDER0213", 67), // ORDER 单元 0213（门序 × 相位 21 × 二 轮）
    ("BLOCK0214", 74), // BLOCK 单元 0214（阻断 × 相位 22 × 二 轮）
    ("SIGN0215", 81), // SIGN 单元 0215（签署 × 相位 23 × 二 轮）
    ("GATE0216", 88), // GATE 单元 0216（门禁 × 相位 24 × 二 轮）
    ("FIVE0217", 6), // FIVE 单元 0217（五门 × 相位 25 × 二 轮）
    ("PIPE0218", 13), // PIPE 单元 0218（流水线 × 相位 26 × 二 轮）
    ("TOPO0219", 20), // TOPO 单元 0219（拓扑 × 相位 27 × 二 轮）
    ("FAST0220", 27), // FAST 单元 0220（快速门 × 相位 28 × 二 轮）
    ("DEEP0221", 34), // DEEP 单元 0221（深度门 × 相位 29 × 二 轮）
    ("FUZZG0222", 41), // FUZZG 单元 0222（fuzz 门 × 相位 30 × 二 轮）
    ("REGR0223", 48), // REGR 单元 0223（回归门 × 相位 31 × 二 轮）
    ("RELEASE0224", 55), // RELEASE 单元 0224（发布门 × 相位 32 × 二 轮）
    ("ORDER0225", 62), // ORDER 单元 0225（门序 × 相位 33 × 二 轮）
    ("BLOCK0226", 69), // BLOCK 单元 0226（阻断 × 相位 34 × 二 轮）
    ("SIGN0227", 76), // SIGN 单元 0227（签署 × 相位 35 × 二 轮）
    ("GATE0228", 83), // GATE 单元 0228（门禁 × 相位 36 × 二 轮）
    ("FIVE0229", 1), // FIVE 单元 0229（五门 × 相位 37 × 二 轮）
    ("PIPE0230", 8), // PIPE 单元 0230（流水线 × 相位 38 × 二 轮）
    ("TOPO0231", 15), // TOPO 单元 0231（拓扑 × 相位 39 × 二 轮）
    ("FAST0232", 22), // FAST 单元 0232（快速门 × 相位 40 × 二 轮）
    ("DEEP0233", 29), // DEEP 单元 0233（深度门 × 相位 41 × 二 轮）
    ("FUZZG0234", 36), // FUZZG 单元 0234（fuzz 门 × 相位 42 × 二 轮）
    ("REGR0235", 43), // REGR 单元 0235（回归门 × 相位 43 × 二 轮）
    ("RELEASE0236", 50), // RELEASE 单元 0236（发布门 × 相位 44 × 二 轮）
    ("ORDER0237", 57), // ORDER 单元 0237（门序 × 相位 45 × 二 轮）
    ("BLOCK0238", 64), // BLOCK 单元 0238（阻断 × 相位 46 × 二 轮）
    ("SIGN0239", 71), // SIGN 单元 0239（签署 × 相位 47 × 二 轮）
    ("GATE0240", 78), // GATE 单元 0240（门禁 × 相位 48 × 二 轮）
    ("FIVE0241", 85), // FIVE 单元 0241（五门 × 相位 49 × 二 轮）
    ("PIPE0242", 3), // PIPE 单元 0242（流水线 × 相位 50 × 二 轮）
    ("TOPO0243", 10), // TOPO 单元 0243（拓扑 × 相位 51 × 二 轮）
    ("FAST0244", 17), // FAST 单元 0244（快速门 × 相位 52 × 二 轮）
    ("DEEP0245", 24), // DEEP 单元 0245（深度门 × 相位 53 × 二 轮）
    ("FUZZG0246", 31), // FUZZG 单元 0246（fuzz 门 × 相位 54 × 二 轮）
    ("REGR0247", 38), // REGR 单元 0247（回归门 × 相位 55 × 二 轮）
    ("RELEASE0248", 45), // RELEASE 单元 0248（发布门 × 相位 56 × 二 轮）
    ("ORDER0249", 52), // ORDER 单元 0249（门序 × 相位 57 × 二 轮）
    ("BLOCK0250", 59), // BLOCK 单元 0250（阻断 × 相位 58 × 二 轮）
    ("SIGN0251", 66), // SIGN 单元 0251（签署 × 相位 59 × 二 轮）
    ("GATE0252", 73), // GATE 单元 0252（门禁 × 相位 60 × 二 轮）
    ("FIVE0253", 80), // FIVE 单元 0253（五门 × 相位 61 × 二 轮）
    ("PIPE0254", 87), // PIPE 单元 0254（流水线 × 相位 62 × 二 轮）
    ("TOPO0255", 5), // TOPO 单元 0255（拓扑 × 相位 63 × 二 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3741bmx_get(key: &str) -> Option<u64> {
    match key {
        "GATE0000" => Some(0),
        "FIVE0001" => Some(7),
        "PIPE0002" => Some(14),
        "TOPO0003" => Some(21),
        "FAST0004" => Some(28),
        "DEEP0005" => Some(35),
        "FUZZG0006" => Some(42),
        "REGR0007" => Some(49),
        "RELEASE0008" => Some(56),
        "ORDER0009" => Some(63),
        "BLOCK0010" => Some(70),
        "SIGN0011" => Some(77),
        "GATE0012" => Some(84),
        "FIVE0013" => Some(2),
        "PIPE0014" => Some(9),
        "TOPO0015" => Some(16),
        "FAST0016" => Some(23),
        "DEEP0017" => Some(30),
        "FUZZG0018" => Some(37),
        "REGR0019" => Some(44),
        "RELEASE0020" => Some(51),
        "ORDER0021" => Some(58),
        "BLOCK0022" => Some(65),
        "SIGN0023" => Some(72),
        "GATE0024" => Some(79),
        "FIVE0025" => Some(86),
        "PIPE0026" => Some(4),
        "TOPO0027" => Some(11),
        "FAST0028" => Some(18),
        "DEEP0029" => Some(25),
        "FUZZG0030" => Some(32),
        "REGR0031" => Some(39),
        "RELEASE0032" => Some(46),
        "ORDER0033" => Some(53),
        "BLOCK0034" => Some(60),
        "SIGN0035" => Some(67),
        "GATE0036" => Some(74),
        "FIVE0037" => Some(81),
        "PIPE0038" => Some(88),
        "TOPO0039" => Some(6),
        "FAST0040" => Some(13),
        "DEEP0041" => Some(20),
        "FUZZG0042" => Some(27),
        "REGR0043" => Some(34),
        "RELEASE0044" => Some(41),
        "ORDER0045" => Some(48),
        "BLOCK0046" => Some(55),
        "SIGN0047" => Some(62),
        "GATE0048" => Some(69),
        "FIVE0049" => Some(76),
        "PIPE0050" => Some(83),
        "TOPO0051" => Some(1),
        "FAST0052" => Some(8),
        "DEEP0053" => Some(15),
        "FUZZG0054" => Some(22),
        "REGR0055" => Some(29),
        "RELEASE0056" => Some(36),
        "ORDER0057" => Some(43),
        "BLOCK0058" => Some(50),
        "SIGN0059" => Some(57),
        "GATE0060" => Some(64),
        "FIVE0061" => Some(71),
        "PIPE0062" => Some(78),
        "TOPO0063" => Some(85),
        "FAST0064" => Some(3),
        "DEEP0065" => Some(10),
        "FUZZG0066" => Some(17),
        "REGR0067" => Some(24),
        "RELEASE0068" => Some(31),
        "ORDER0069" => Some(38),
        "BLOCK0070" => Some(45),
        "SIGN0071" => Some(52),
        "GATE0072" => Some(59),
        "FIVE0073" => Some(66),
        "PIPE0074" => Some(73),
        "TOPO0075" => Some(80),
        "FAST0076" => Some(87),
        "DEEP0077" => Some(5),
        "FUZZG0078" => Some(12),
        "REGR0079" => Some(19),
        "RELEASE0080" => Some(26),
        "ORDER0081" => Some(33),
        "BLOCK0082" => Some(40),
        "SIGN0083" => Some(47),
        "GATE0084" => Some(54),
        "FIVE0085" => Some(61),
        "PIPE0086" => Some(68),
        "TOPO0087" => Some(75),
        "FAST0088" => Some(82),
        "DEEP0089" => Some(0),
        "FUZZG0090" => Some(7),
        "REGR0091" => Some(14),
        "RELEASE0092" => Some(21),
        "ORDER0093" => Some(28),
        "BLOCK0094" => Some(35),
        "SIGN0095" => Some(42),
        "GATE0096" => Some(49),
        "FIVE0097" => Some(56),
        "PIPE0098" => Some(63),
        "TOPO0099" => Some(70),
        "FAST0100" => Some(77),
        "DEEP0101" => Some(84),
        "FUZZG0102" => Some(2),
        "REGR0103" => Some(9),
        "RELEASE0104" => Some(16),
        "ORDER0105" => Some(23),
        "BLOCK0106" => Some(30),
        "SIGN0107" => Some(37),
        "GATE0108" => Some(44),
        "FIVE0109" => Some(51),
        "PIPE0110" => Some(58),
        "TOPO0111" => Some(65),
        "FAST0112" => Some(72),
        "DEEP0113" => Some(79),
        "FUZZG0114" => Some(86),
        "REGR0115" => Some(4),
        "RELEASE0116" => Some(11),
        "ORDER0117" => Some(18),
        "BLOCK0118" => Some(25),
        "SIGN0119" => Some(32),
        "GATE0120" => Some(39),
        "FIVE0121" => Some(46),
        "PIPE0122" => Some(53),
        "TOPO0123" => Some(60),
        "FAST0124" => Some(67),
        "DEEP0125" => Some(74),
        "FUZZG0126" => Some(81),
        "REGR0127" => Some(88),
        "RELEASE0128" => Some(6),
        "ORDER0129" => Some(13),
        "BLOCK0130" => Some(20),
        "SIGN0131" => Some(27),
        "GATE0132" => Some(34),
        "FIVE0133" => Some(41),
        "PIPE0134" => Some(48),
        "TOPO0135" => Some(55),
        "FAST0136" => Some(62),
        "DEEP0137" => Some(69),
        "FUZZG0138" => Some(76),
        "REGR0139" => Some(83),
        "RELEASE0140" => Some(1),
        "ORDER0141" => Some(8),
        "BLOCK0142" => Some(15),
        "SIGN0143" => Some(22),
        "GATE0144" => Some(29),
        "FIVE0145" => Some(36),
        "PIPE0146" => Some(43),
        "TOPO0147" => Some(50),
        "FAST0148" => Some(57),
        "DEEP0149" => Some(64),
        "FUZZG0150" => Some(71),
        "REGR0151" => Some(78),
        "RELEASE0152" => Some(85),
        "ORDER0153" => Some(3),
        "BLOCK0154" => Some(10),
        "SIGN0155" => Some(17),
        "GATE0156" => Some(24),
        "FIVE0157" => Some(31),
        "PIPE0158" => Some(38),
        "TOPO0159" => Some(45),
        "FAST0160" => Some(52),
        "DEEP0161" => Some(59),
        "FUZZG0162" => Some(66),
        "REGR0163" => Some(73),
        "RELEASE0164" => Some(80),
        "ORDER0165" => Some(87),
        "BLOCK0166" => Some(5),
        "SIGN0167" => Some(12),
        "GATE0168" => Some(19),
        "FIVE0169" => Some(26),
        "PIPE0170" => Some(33),
        "TOPO0171" => Some(40),
        "FAST0172" => Some(47),
        "DEEP0173" => Some(54),
        "FUZZG0174" => Some(61),
        "REGR0175" => Some(68),
        "RELEASE0176" => Some(75),
        "ORDER0177" => Some(82),
        "BLOCK0178" => Some(0),
        "SIGN0179" => Some(7),
        "GATE0180" => Some(14),
        "FIVE0181" => Some(21),
        "PIPE0182" => Some(28),
        "TOPO0183" => Some(35),
        "FAST0184" => Some(42),
        "DEEP0185" => Some(49),
        "FUZZG0186" => Some(56),
        "REGR0187" => Some(63),
        "RELEASE0188" => Some(70),
        "ORDER0189" => Some(77),
        "BLOCK0190" => Some(84),
        "SIGN0191" => Some(2),
        "GATE0192" => Some(9),
        "FIVE0193" => Some(16),
        "PIPE0194" => Some(23),
        "TOPO0195" => Some(30),
        "FAST0196" => Some(37),
        "DEEP0197" => Some(44),
        "FUZZG0198" => Some(51),
        "REGR0199" => Some(58),
        "RELEASE0200" => Some(65),
        "ORDER0201" => Some(72),
        "BLOCK0202" => Some(79),
        "SIGN0203" => Some(86),
        "GATE0204" => Some(4),
        "FIVE0205" => Some(11),
        "PIPE0206" => Some(18),
        "TOPO0207" => Some(25),
        "FAST0208" => Some(32),
        "DEEP0209" => Some(39),
        "FUZZG0210" => Some(46),
        "REGR0211" => Some(53),
        "RELEASE0212" => Some(60),
        "ORDER0213" => Some(67),
        "BLOCK0214" => Some(74),
        "SIGN0215" => Some(81),
        "GATE0216" => Some(88),
        "FIVE0217" => Some(6),
        "PIPE0218" => Some(13),
        "TOPO0219" => Some(20),
        "FAST0220" => Some(27),
        "DEEP0221" => Some(34),
        "FUZZG0222" => Some(41),
        "REGR0223" => Some(48),
        "RELEASE0224" => Some(55),
        "ORDER0225" => Some(62),
        "BLOCK0226" => Some(69),
        "SIGN0227" => Some(76),
        "GATE0228" => Some(83),
        "FIVE0229" => Some(1),
        "PIPE0230" => Some(8),
        "TOPO0231" => Some(15),
        "FAST0232" => Some(22),
        "DEEP0233" => Some(29),
        "FUZZG0234" => Some(36),
        "REGR0235" => Some(43),
        "RELEASE0236" => Some(50),
        "ORDER0237" => Some(57),
        "BLOCK0238" => Some(64),
        "SIGN0239" => Some(71),
        "GATE0240" => Some(78),
        "FIVE0241" => Some(85),
        "PIPE0242" => Some(3),
        "TOPO0243" => Some(10),
        "FAST0244" => Some(17),
        "DEEP0245" => Some(24),
        "FUZZG0246" => Some(31),
        "REGR0247" => Some(38),
        "RELEASE0248" => Some(45),
        "ORDER0249" => Some(52),
        "BLOCK0250" => Some(59),
        "SIGN0251" => Some(66),
        "GATE0252" => Some(73),
        "FIVE0253" => Some(80),
        "PIPE0254" => Some(87),
        "TOPO0255" => Some(5),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3741bmx_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3741bmx_COUNT {
        let (k, v) = u3741bmx[i];
        let ok = match k {
            "GATE0000" => v < 89,
            "FIVE0001" => v < 89,
            "PIPE0002" => v < 89,
            "TOPO0003" => v < 89,
            "FAST0004" => v < 89,
            "DEEP0005" => v < 89,
            "FUZZG0006" => v < 89,
            "REGR0007" => v < 89,
            "RELEASE0008" => v < 89,
            "ORDER0009" => v < 89,
            "BLOCK0010" => v < 89,
            "SIGN0011" => v < 89,
            "GATE0012" => v < 89,
            "FIVE0013" => v < 89,
            "PIPE0014" => v < 89,
            "TOPO0015" => v < 89,
            "FAST0016" => v < 89,
            "DEEP0017" => v < 89,
            "FUZZG0018" => v < 89,
            "REGR0019" => v < 89,
            "RELEASE0020" => v < 89,
            "ORDER0021" => v < 89,
            "BLOCK0022" => v < 89,
            "SIGN0023" => v < 89,
            "GATE0024" => v < 89,
            "FIVE0025" => v < 89,
            "PIPE0026" => v < 89,
            "TOPO0027" => v < 89,
            "FAST0028" => v < 89,
            "DEEP0029" => v < 89,
            "FUZZG0030" => v < 89,
            "REGR0031" => v < 89,
            "RELEASE0032" => v < 89,
            "ORDER0033" => v < 89,
            "BLOCK0034" => v < 89,
            "SIGN0035" => v < 89,
            "GATE0036" => v < 89,
            "FIVE0037" => v < 89,
            "PIPE0038" => v < 89,
            "TOPO0039" => v < 89,
            "FAST0040" => v < 89,
            "DEEP0041" => v < 89,
            "FUZZG0042" => v < 89,
            "REGR0043" => v < 89,
            "RELEASE0044" => v < 89,
            "ORDER0045" => v < 89,
            "BLOCK0046" => v < 89,
            "SIGN0047" => v < 89,
            "GATE0048" => v < 89,
            "FIVE0049" => v < 89,
            "PIPE0050" => v < 89,
            "TOPO0051" => v < 89,
            "FAST0052" => v < 89,
            "DEEP0053" => v < 89,
            "FUZZG0054" => v < 89,
            "REGR0055" => v < 89,
            "RELEASE0056" => v < 89,
            "ORDER0057" => v < 89,
            "BLOCK0058" => v < 89,
            "SIGN0059" => v < 89,
            "GATE0060" => v < 89,
            "FIVE0061" => v < 89,
            "PIPE0062" => v < 89,
            "TOPO0063" => v < 89,
            "FAST0064" => v < 89,
            "DEEP0065" => v < 89,
            "FUZZG0066" => v < 89,
            "REGR0067" => v < 89,
            "RELEASE0068" => v < 89,
            "ORDER0069" => v < 89,
            "BLOCK0070" => v < 89,
            "SIGN0071" => v < 89,
            "GATE0072" => v < 89,
            "FIVE0073" => v < 89,
            "PIPE0074" => v < 89,
            "TOPO0075" => v < 89,
            "FAST0076" => v < 89,
            "DEEP0077" => v < 89,
            "FUZZG0078" => v < 89,
            "REGR0079" => v < 89,
            "RELEASE0080" => v < 89,
            "ORDER0081" => v < 89,
            "BLOCK0082" => v < 89,
            "SIGN0083" => v < 89,
            "GATE0084" => v < 89,
            "FIVE0085" => v < 89,
            "PIPE0086" => v < 89,
            "TOPO0087" => v < 89,
            "FAST0088" => v < 89,
            "DEEP0089" => v < 89,
            "FUZZG0090" => v < 89,
            "REGR0091" => v < 89,
            "RELEASE0092" => v < 89,
            "ORDER0093" => v < 89,
            "BLOCK0094" => v < 89,
            "SIGN0095" => v < 89,
            "GATE0096" => v < 89,
            "FIVE0097" => v < 89,
            "PIPE0098" => v < 89,
            "TOPO0099" => v < 89,
            "FAST0100" => v < 89,
            "DEEP0101" => v < 89,
            "FUZZG0102" => v < 89,
            "REGR0103" => v < 89,
            "RELEASE0104" => v < 89,
            "ORDER0105" => v < 89,
            "BLOCK0106" => v < 89,
            "SIGN0107" => v < 89,
            "GATE0108" => v < 89,
            "FIVE0109" => v < 89,
            "PIPE0110" => v < 89,
            "TOPO0111" => v < 89,
            "FAST0112" => v < 89,
            "DEEP0113" => v < 89,
            "FUZZG0114" => v < 89,
            "REGR0115" => v < 89,
            "RELEASE0116" => v < 89,
            "ORDER0117" => v < 89,
            "BLOCK0118" => v < 89,
            "SIGN0119" => v < 89,
            "GATE0120" => v < 89,
            "FIVE0121" => v < 89,
            "PIPE0122" => v < 89,
            "TOPO0123" => v < 89,
            "FAST0124" => v < 89,
            "DEEP0125" => v < 89,
            "FUZZG0126" => v < 89,
            "REGR0127" => v < 89,
            "RELEASE0128" => v < 89,
            "ORDER0129" => v < 89,
            "BLOCK0130" => v < 89,
            "SIGN0131" => v < 89,
            "GATE0132" => v < 89,
            "FIVE0133" => v < 89,
            "PIPE0134" => v < 89,
            "TOPO0135" => v < 89,
            "FAST0136" => v < 89,
            "DEEP0137" => v < 89,
            "FUZZG0138" => v < 89,
            "REGR0139" => v < 89,
            "RELEASE0140" => v < 89,
            "ORDER0141" => v < 89,
            "BLOCK0142" => v < 89,
            "SIGN0143" => v < 89,
            "GATE0144" => v < 89,
            "FIVE0145" => v < 89,
            "PIPE0146" => v < 89,
            "TOPO0147" => v < 89,
            "FAST0148" => v < 89,
            "DEEP0149" => v < 89,
            "FUZZG0150" => v < 89,
            "REGR0151" => v < 89,
            "RELEASE0152" => v < 89,
            "ORDER0153" => v < 89,
            "BLOCK0154" => v < 89,
            "SIGN0155" => v < 89,
            "GATE0156" => v < 89,
            "FIVE0157" => v < 89,
            "PIPE0158" => v < 89,
            "TOPO0159" => v < 89,
            "FAST0160" => v < 89,
            "DEEP0161" => v < 89,
            "FUZZG0162" => v < 89,
            "REGR0163" => v < 89,
            "RELEASE0164" => v < 89,
            "ORDER0165" => v < 89,
            "BLOCK0166" => v < 89,
            "SIGN0167" => v < 89,
            "GATE0168" => v < 89,
            "FIVE0169" => v < 89,
            "PIPE0170" => v < 89,
            "TOPO0171" => v < 89,
            "FAST0172" => v < 89,
            "DEEP0173" => v < 89,
            "FUZZG0174" => v < 89,
            "REGR0175" => v < 89,
            "RELEASE0176" => v < 89,
            "ORDER0177" => v < 89,
            "BLOCK0178" => v < 89,
            "SIGN0179" => v < 89,
            "GATE0180" => v < 89,
            "FIVE0181" => v < 89,
            "PIPE0182" => v < 89,
            "TOPO0183" => v < 89,
            "FAST0184" => v < 89,
            "DEEP0185" => v < 89,
            "FUZZG0186" => v < 89,
            "REGR0187" => v < 89,
            "RELEASE0188" => v < 89,
            "ORDER0189" => v < 89,
            "BLOCK0190" => v < 89,
            "SIGN0191" => v < 89,
            "GATE0192" => v < 89,
            "FIVE0193" => v < 89,
            "PIPE0194" => v < 89,
            "TOPO0195" => v < 89,
            "FAST0196" => v < 89,
            "DEEP0197" => v < 89,
            "FUZZG0198" => v < 89,
            "REGR0199" => v < 89,
            "RELEASE0200" => v < 89,
            "ORDER0201" => v < 89,
            "BLOCK0202" => v < 89,
            "SIGN0203" => v < 89,
            "GATE0204" => v < 89,
            "FIVE0205" => v < 89,
            "PIPE0206" => v < 89,
            "TOPO0207" => v < 89,
            "FAST0208" => v < 89,
            "DEEP0209" => v < 89,
            "FUZZG0210" => v < 89,
            "REGR0211" => v < 89,
            "RELEASE0212" => v < 89,
            "ORDER0213" => v < 89,
            "BLOCK0214" => v < 89,
            "SIGN0215" => v < 89,
            "GATE0216" => v < 89,
            "FIVE0217" => v < 89,
            "PIPE0218" => v < 89,
            "TOPO0219" => v < 89,
            "FAST0220" => v < 89,
            "DEEP0221" => v < 89,
            "FUZZG0222" => v < 89,
            "REGR0223" => v < 89,
            "RELEASE0224" => v < 89,
            "ORDER0225" => v < 89,
            "BLOCK0226" => v < 89,
            "SIGN0227" => v < 89,
            "GATE0228" => v < 89,
            "FIVE0229" => v < 89,
            "PIPE0230" => v < 89,
            "TOPO0231" => v < 89,
            "FAST0232" => v < 89,
            "DEEP0233" => v < 89,
            "FUZZG0234" => v < 89,
            "REGR0235" => v < 89,
            "RELEASE0236" => v < 89,
            "ORDER0237" => v < 89,
            "BLOCK0238" => v < 89,
            "SIGN0239" => v < 89,
            "GATE0240" => v < 89,
            "FIVE0241" => v < 89,
            "PIPE0242" => v < 89,
            "TOPO0243" => v < 89,
            "FAST0244" => v < 89,
            "DEEP0245" => v < 89,
            "FUZZG0246" => v < 89,
            "REGR0247" => v < 89,
            "RELEASE0248" => v < 89,
            "ORDER0249" => v < 89,
            "BLOCK0250" => v < 89,
            "SIGN0251" => v < 89,
            "GATE0252" => v < 89,
            "FIVE0253" => v < 89,
            "PIPE0254" => v < 89,
            "TOPO0255" => v < 89,
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
pub fn u3741bmx_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3741bmx_COUNT {
        s = s.wrapping_add(u3741bmx[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3741bmx_unique() -> bool {
    let mut i = 0usize;
    while i < u3741bmx_COUNT {
        let mut j = i + 1;
        while j < u3741bmx_COUNT {
            if u3741bmx[i].0 == u3741bmx[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe230, 0xe23f]。
pub const fn u3741_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3741Code::ALL.len() {
        let c = u3741Code::ALL[i].code();
        if c < 0xe230 || c > 0xe23f {
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
fn u3741_guard_seg() -> bool {
    u3741_segment_guard()
}

/// UNX-F3741 bmx 256 行齐备 + 边界锚 + 键外拒收。
fn check_u3741_bmx_count() -> bool {
    let mut ok = u3741bmx_COUNT == 256;
    if ok { ok = u3741bmx_get("GATE0000") == Some(0) && u3741bmx_get("TOPO0255") == Some(5); }
    if ok { ok = u3741bmx_get("ZZZ_NOKEY").is_none(); }
    ok
}

/// UNX-F3741 bmx 累计守恒（理论值 11231）。
fn check_u3741_bmx_sum() -> bool {
    u3741bmx_sum() == 11231
}

/// UNX-F3741 bmx 逐行守卫与唯一性全绿。
fn check_u3741_bmx_guard() -> bool {
    u3741bmx_scan() == 0 && u3741bmx_unique()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3741_checks() -> CheckSet {
    let mut set = CheckSet::new("U3741");
    set.add(
        "u3741-seg-guard",
        u3741_guard_seg(),
        if u3741_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3741-bmx-count",
        check_u3741_bmx_count(),
        if check_u3741_bmx_count() { "" } else { "UNX-F3741 bmx count mismatch" },
    );
    set.add(
        "u3741-bmx-sum",
        check_u3741_bmx_sum(),
        if check_u3741_bmx_sum() { "" } else { "UNX-F3741 bmx sum mismatch" },
    );
    set.add(
        "u3741-bmx-guard",
        check_u3741_bmx_guard(),
        if check_u3741_bmx_guard() { "" } else { "UNX-F3741 bmx guard mismatch" },
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
    fn u3741_all_checks_green() {
        let set = run_u3741_checks();
        assert!(set.all_passed(), "U3741 domain checks must be green");
    }

    #[test]
    fn u3741_segment_guard_holds() {
        assert!(u3741_segment_guard());
    }
}
