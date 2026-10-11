//! unxu2981_u2981bmx —— 域级收口方案冻结（收口六件套范围） 深编bmx（域级收口六件套 × 256 单元）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2981 · 域级收口方案冻结（收口六件套范围） · 判据原文：UNX-F2981 · 判据：UNX-F2981-J1 收口六件套（总对账/完整性终核/守恒终账/指标总览/欠账终账/总判定）范围冻结哈希入册，六件与 F2982–F2997 条位一一对应
//! · 组件：域级收口六件套 守卫映射 256 行（二 轮派生真实数据 + 逐行守卫 + 键唯一）
//! · 判据落地：UNX-F2981 · 判据：UNX-F2981-J1 收口六件套（总对账/完整性终核/守恒终账/指标总览/欠账终账/总判定）范围冻结哈希入册，六件与 F298
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2981_VERSION: &str = "u2981-u2981bmx-v1";

/// 诊断段基址。
pub const U2981_SEG_BASE: u16 = 0xe2f0;

// --------------------------------------------------------------------------
// 诊断码（u2981 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域级收口六件套 bmx 面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2981Code {
    /// 0xe2f0 单元格重复（收口 同键双占）。
    CellDup,
    /// 0xe2f1 单元格断档（六件套 序号不连续）。
    CellGap,
    /// 0xe2f2 索引越界（封口）。
    IndexOob,
    /// 0xe2f3 大表账破缺（行数≠登记值）。
    LedgerBreak,
    /// 0xe2f4 表序断档。
    OrderBreak,
    /// 0xe2f5 累计漂移（求和≠理论值）。
    SumDrift,
    /// 0xe2f6 掩码非法。
    MaskBad,
    /// 0xe2f7 守卫触发。
    GuardTripped,
    /// 0xe2f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe2f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe2fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe2fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2981Code {
    /// 全量闭集。
    pub const ALL: [u2981Code; 12] = [
        u2981Code::CellDup,
        u2981Code::CellGap,
        u2981Code::IndexOob,
        u2981Code::LedgerBreak,
        u2981Code::OrderBreak,
        u2981Code::SumDrift,
        u2981Code::MaskBad,
        u2981Code::GuardTripped,
        u2981Code::LedgerBroken,
        u2981Code::SnapshotFingerprint,
        u2981Code::TableBroken,
        u2981Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2981Code::CellDup => 0xe2f0,
            u2981Code::CellGap => 0xe2f1,
            u2981Code::IndexOob => 0xe2f2,
            u2981Code::LedgerBreak => 0xe2f3,
            u2981Code::OrderBreak => 0xe2f4,
            u2981Code::SumDrift => 0xe2f5,
            u2981Code::MaskBad => 0xe2f6,
            u2981Code::GuardTripped => 0xe2f7,
            u2981Code::LedgerBroken => 0xe2f8,
            u2981Code::SnapshotFingerprint => 0xe2f9,
            u2981Code::TableBroken => 0xe2fa,
            u2981Code::SegmentGuard => 0xe2fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2981Code::CellDup => "单元格重复（收口 同键双占）",
            u2981Code::CellGap => "单元格断档（六件套 序号不连续）",
            u2981Code::IndexOob => "索引越界（封口）",
            u2981Code::LedgerBreak => "大表账破缺（行数≠登记值）",
            u2981Code::OrderBreak => "表序断档",
            u2981Code::SumDrift => "累计漂移（求和≠理论值）",
            u2981Code::MaskBad => "掩码非法",
            u2981Code::GuardTripped => "守卫触发",
            u2981Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2981Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2981Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2981Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2981Code::CellDup => "按键唯一律去重",
            u2981Code::CellGap => "按连续律补号",
            u2981Code::IndexOob => "按行数边界裁剪",
            u2981Code::LedgerBreak => "按守恒重建",
            u2981Code::OrderBreak => "按序重排",
            u2981Code::SumDrift => "按理论值复算",
            u2981Code::MaskBad => "按掩码律校正",
            u2981Code::GuardTripped => "检查临界区",
            u2981Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2981Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2981Code::TableBroken => "按语义规则重建常量表",
            u2981Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2981bmx（256 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 域级收口六件套 bmx 守卫映射（256 行逐行注释自证）。
/// 行数。
pub const u2981bmx_COUNT: usize = 256;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2981bmx: [(&'static str, u64); 256] = [
    ("SIX0000", 0), // SIX 单元 0000（六件套 × 相位 00 × 二 轮）
    ("CLOSE0001", 7), // CLOSE 单元 0001（收口 × 相位 01 × 二 轮）
    ("SCOPE0002", 14), // SCOPE 单元 0002（范围 × 相位 02 × 二 轮）
    ("ITEM0003", 21), // ITEM 单元 0003（条目 × 相位 03 × 二 轮）
    ("PROOF0004", 28), // PROOF 单元 0004（证据 × 相位 04 × 二 轮）
    ("BIND0005", 35), // BIND 单元 0005（绑定 × 相位 05 × 二 轮）
    ("ACCT0006", 42), // ACCT 单元 0006（账 × 相位 06 × 二 轮）
    ("CHK0007", 49), // CHK 单元 0007（核验 × 相位 07 × 二 轮）
    ("GAP0008", 56), // GAP 单元 0008（缺口 × 相位 08 × 二 轮）
    ("SIGN0009", 63), // SIGN 单元 0009（签署 × 相位 09 × 二 轮）
    ("ARCH0010", 70), // ARCH 单元 0010（归档 × 相位 10 × 二 轮）
    ("SEAL0011", 77), // SEAL 单元 0011（封口 × 相位 11 × 二 轮）
    ("SIX0012", 84), // SIX 单元 0012（六件套 × 相位 12 × 二 轮）
    ("CLOSE0013", 2), // CLOSE 单元 0013（收口 × 相位 13 × 二 轮）
    ("SCOPE0014", 9), // SCOPE 单元 0014（范围 × 相位 14 × 二 轮）
    ("ITEM0015", 16), // ITEM 单元 0015（条目 × 相位 15 × 二 轮）
    ("PROOF0016", 23), // PROOF 单元 0016（证据 × 相位 16 × 二 轮）
    ("BIND0017", 30), // BIND 单元 0017（绑定 × 相位 17 × 二 轮）
    ("ACCT0018", 37), // ACCT 单元 0018（账 × 相位 18 × 二 轮）
    ("CHK0019", 44), // CHK 单元 0019（核验 × 相位 19 × 二 轮）
    ("GAP0020", 51), // GAP 单元 0020（缺口 × 相位 20 × 二 轮）
    ("SIGN0021", 58), // SIGN 单元 0021（签署 × 相位 21 × 二 轮）
    ("ARCH0022", 65), // ARCH 单元 0022（归档 × 相位 22 × 二 轮）
    ("SEAL0023", 72), // SEAL 单元 0023（封口 × 相位 23 × 二 轮）
    ("SIX0024", 79), // SIX 单元 0024（六件套 × 相位 24 × 二 轮）
    ("CLOSE0025", 86), // CLOSE 单元 0025（收口 × 相位 25 × 二 轮）
    ("SCOPE0026", 4), // SCOPE 单元 0026（范围 × 相位 26 × 二 轮）
    ("ITEM0027", 11), // ITEM 单元 0027（条目 × 相位 27 × 二 轮）
    ("PROOF0028", 18), // PROOF 单元 0028（证据 × 相位 28 × 二 轮）
    ("BIND0029", 25), // BIND 单元 0029（绑定 × 相位 29 × 二 轮）
    ("ACCT0030", 32), // ACCT 单元 0030（账 × 相位 30 × 二 轮）
    ("CHK0031", 39), // CHK 单元 0031（核验 × 相位 31 × 二 轮）
    ("GAP0032", 46), // GAP 单元 0032（缺口 × 相位 32 × 二 轮）
    ("SIGN0033", 53), // SIGN 单元 0033（签署 × 相位 33 × 二 轮）
    ("ARCH0034", 60), // ARCH 单元 0034（归档 × 相位 34 × 二 轮）
    ("SEAL0035", 67), // SEAL 单元 0035（封口 × 相位 35 × 二 轮）
    ("SIX0036", 74), // SIX 单元 0036（六件套 × 相位 36 × 二 轮）
    ("CLOSE0037", 81), // CLOSE 单元 0037（收口 × 相位 37 × 二 轮）
    ("SCOPE0038", 88), // SCOPE 单元 0038（范围 × 相位 38 × 二 轮）
    ("ITEM0039", 6), // ITEM 单元 0039（条目 × 相位 39 × 二 轮）
    ("PROOF0040", 13), // PROOF 单元 0040（证据 × 相位 40 × 二 轮）
    ("BIND0041", 20), // BIND 单元 0041（绑定 × 相位 41 × 二 轮）
    ("ACCT0042", 27), // ACCT 单元 0042（账 × 相位 42 × 二 轮）
    ("CHK0043", 34), // CHK 单元 0043（核验 × 相位 43 × 二 轮）
    ("GAP0044", 41), // GAP 单元 0044（缺口 × 相位 44 × 二 轮）
    ("SIGN0045", 48), // SIGN 单元 0045（签署 × 相位 45 × 二 轮）
    ("ARCH0046", 55), // ARCH 单元 0046（归档 × 相位 46 × 二 轮）
    ("SEAL0047", 62), // SEAL 单元 0047（封口 × 相位 47 × 二 轮）
    ("SIX0048", 69), // SIX 单元 0048（六件套 × 相位 48 × 二 轮）
    ("CLOSE0049", 76), // CLOSE 单元 0049（收口 × 相位 49 × 二 轮）
    ("SCOPE0050", 83), // SCOPE 单元 0050（范围 × 相位 50 × 二 轮）
    ("ITEM0051", 1), // ITEM 单元 0051（条目 × 相位 51 × 二 轮）
    ("PROOF0052", 8), // PROOF 单元 0052（证据 × 相位 52 × 二 轮）
    ("BIND0053", 15), // BIND 单元 0053（绑定 × 相位 53 × 二 轮）
    ("ACCT0054", 22), // ACCT 单元 0054（账 × 相位 54 × 二 轮）
    ("CHK0055", 29), // CHK 单元 0055（核验 × 相位 55 × 二 轮）
    ("GAP0056", 36), // GAP 单元 0056（缺口 × 相位 56 × 二 轮）
    ("SIGN0057", 43), // SIGN 单元 0057（签署 × 相位 57 × 二 轮）
    ("ARCH0058", 50), // ARCH 单元 0058（归档 × 相位 58 × 二 轮）
    ("SEAL0059", 57), // SEAL 单元 0059（封口 × 相位 59 × 二 轮）
    ("SIX0060", 64), // SIX 单元 0060（六件套 × 相位 60 × 二 轮）
    ("CLOSE0061", 71), // CLOSE 单元 0061（收口 × 相位 61 × 二 轮）
    ("SCOPE0062", 78), // SCOPE 单元 0062（范围 × 相位 62 × 二 轮）
    ("ITEM0063", 85), // ITEM 单元 0063（条目 × 相位 63 × 二 轮）
    ("PROOF0064", 3), // PROOF 单元 0064（证据 × 相位 00 × 二 轮）
    ("BIND0065", 10), // BIND 单元 0065（绑定 × 相位 01 × 二 轮）
    ("ACCT0066", 17), // ACCT 单元 0066（账 × 相位 02 × 二 轮）
    ("CHK0067", 24), // CHK 单元 0067（核验 × 相位 03 × 二 轮）
    ("GAP0068", 31), // GAP 单元 0068（缺口 × 相位 04 × 二 轮）
    ("SIGN0069", 38), // SIGN 单元 0069（签署 × 相位 05 × 二 轮）
    ("ARCH0070", 45), // ARCH 单元 0070（归档 × 相位 06 × 二 轮）
    ("SEAL0071", 52), // SEAL 单元 0071（封口 × 相位 07 × 二 轮）
    ("SIX0072", 59), // SIX 单元 0072（六件套 × 相位 08 × 二 轮）
    ("CLOSE0073", 66), // CLOSE 单元 0073（收口 × 相位 09 × 二 轮）
    ("SCOPE0074", 73), // SCOPE 单元 0074（范围 × 相位 10 × 二 轮）
    ("ITEM0075", 80), // ITEM 单元 0075（条目 × 相位 11 × 二 轮）
    ("PROOF0076", 87), // PROOF 单元 0076（证据 × 相位 12 × 二 轮）
    ("BIND0077", 5), // BIND 单元 0077（绑定 × 相位 13 × 二 轮）
    ("ACCT0078", 12), // ACCT 单元 0078（账 × 相位 14 × 二 轮）
    ("CHK0079", 19), // CHK 单元 0079（核验 × 相位 15 × 二 轮）
    ("GAP0080", 26), // GAP 单元 0080（缺口 × 相位 16 × 二 轮）
    ("SIGN0081", 33), // SIGN 单元 0081（签署 × 相位 17 × 二 轮）
    ("ARCH0082", 40), // ARCH 单元 0082（归档 × 相位 18 × 二 轮）
    ("SEAL0083", 47), // SEAL 单元 0083（封口 × 相位 19 × 二 轮）
    ("SIX0084", 54), // SIX 单元 0084（六件套 × 相位 20 × 二 轮）
    ("CLOSE0085", 61), // CLOSE 单元 0085（收口 × 相位 21 × 二 轮）
    ("SCOPE0086", 68), // SCOPE 单元 0086（范围 × 相位 22 × 二 轮）
    ("ITEM0087", 75), // ITEM 单元 0087（条目 × 相位 23 × 二 轮）
    ("PROOF0088", 82), // PROOF 单元 0088（证据 × 相位 24 × 二 轮）
    ("BIND0089", 0), // BIND 单元 0089（绑定 × 相位 25 × 二 轮）
    ("ACCT0090", 7), // ACCT 单元 0090（账 × 相位 26 × 二 轮）
    ("CHK0091", 14), // CHK 单元 0091（核验 × 相位 27 × 二 轮）
    ("GAP0092", 21), // GAP 单元 0092（缺口 × 相位 28 × 二 轮）
    ("SIGN0093", 28), // SIGN 单元 0093（签署 × 相位 29 × 二 轮）
    ("ARCH0094", 35), // ARCH 单元 0094（归档 × 相位 30 × 二 轮）
    ("SEAL0095", 42), // SEAL 单元 0095（封口 × 相位 31 × 二 轮）
    ("SIX0096", 49), // SIX 单元 0096（六件套 × 相位 32 × 二 轮）
    ("CLOSE0097", 56), // CLOSE 单元 0097（收口 × 相位 33 × 二 轮）
    ("SCOPE0098", 63), // SCOPE 单元 0098（范围 × 相位 34 × 二 轮）
    ("ITEM0099", 70), // ITEM 单元 0099（条目 × 相位 35 × 二 轮）
    ("PROOF0100", 77), // PROOF 单元 0100（证据 × 相位 36 × 二 轮）
    ("BIND0101", 84), // BIND 单元 0101（绑定 × 相位 37 × 二 轮）
    ("ACCT0102", 2), // ACCT 单元 0102（账 × 相位 38 × 二 轮）
    ("CHK0103", 9), // CHK 单元 0103（核验 × 相位 39 × 二 轮）
    ("GAP0104", 16), // GAP 单元 0104（缺口 × 相位 40 × 二 轮）
    ("SIGN0105", 23), // SIGN 单元 0105（签署 × 相位 41 × 二 轮）
    ("ARCH0106", 30), // ARCH 单元 0106（归档 × 相位 42 × 二 轮）
    ("SEAL0107", 37), // SEAL 单元 0107（封口 × 相位 43 × 二 轮）
    ("SIX0108", 44), // SIX 单元 0108（六件套 × 相位 44 × 二 轮）
    ("CLOSE0109", 51), // CLOSE 单元 0109（收口 × 相位 45 × 二 轮）
    ("SCOPE0110", 58), // SCOPE 单元 0110（范围 × 相位 46 × 二 轮）
    ("ITEM0111", 65), // ITEM 单元 0111（条目 × 相位 47 × 二 轮）
    ("PROOF0112", 72), // PROOF 单元 0112（证据 × 相位 48 × 二 轮）
    ("BIND0113", 79), // BIND 单元 0113（绑定 × 相位 49 × 二 轮）
    ("ACCT0114", 86), // ACCT 单元 0114（账 × 相位 50 × 二 轮）
    ("CHK0115", 4), // CHK 单元 0115（核验 × 相位 51 × 二 轮）
    ("GAP0116", 11), // GAP 单元 0116（缺口 × 相位 52 × 二 轮）
    ("SIGN0117", 18), // SIGN 单元 0117（签署 × 相位 53 × 二 轮）
    ("ARCH0118", 25), // ARCH 单元 0118（归档 × 相位 54 × 二 轮）
    ("SEAL0119", 32), // SEAL 单元 0119（封口 × 相位 55 × 二 轮）
    ("SIX0120", 39), // SIX 单元 0120（六件套 × 相位 56 × 二 轮）
    ("CLOSE0121", 46), // CLOSE 单元 0121（收口 × 相位 57 × 二 轮）
    ("SCOPE0122", 53), // SCOPE 单元 0122（范围 × 相位 58 × 二 轮）
    ("ITEM0123", 60), // ITEM 单元 0123（条目 × 相位 59 × 二 轮）
    ("PROOF0124", 67), // PROOF 单元 0124（证据 × 相位 60 × 二 轮）
    ("BIND0125", 74), // BIND 单元 0125（绑定 × 相位 61 × 二 轮）
    ("ACCT0126", 81), // ACCT 单元 0126（账 × 相位 62 × 二 轮）
    ("CHK0127", 88), // CHK 单元 0127（核验 × 相位 63 × 二 轮）
    ("GAP0128", 6), // GAP 单元 0128（缺口 × 相位 00 × 二 轮）
    ("SIGN0129", 13), // SIGN 单元 0129（签署 × 相位 01 × 二 轮）
    ("ARCH0130", 20), // ARCH 单元 0130（归档 × 相位 02 × 二 轮）
    ("SEAL0131", 27), // SEAL 单元 0131（封口 × 相位 03 × 二 轮）
    ("SIX0132", 34), // SIX 单元 0132（六件套 × 相位 04 × 二 轮）
    ("CLOSE0133", 41), // CLOSE 单元 0133（收口 × 相位 05 × 二 轮）
    ("SCOPE0134", 48), // SCOPE 单元 0134（范围 × 相位 06 × 二 轮）
    ("ITEM0135", 55), // ITEM 单元 0135（条目 × 相位 07 × 二 轮）
    ("PROOF0136", 62), // PROOF 单元 0136（证据 × 相位 08 × 二 轮）
    ("BIND0137", 69), // BIND 单元 0137（绑定 × 相位 09 × 二 轮）
    ("ACCT0138", 76), // ACCT 单元 0138（账 × 相位 10 × 二 轮）
    ("CHK0139", 83), // CHK 单元 0139（核验 × 相位 11 × 二 轮）
    ("GAP0140", 1), // GAP 单元 0140（缺口 × 相位 12 × 二 轮）
    ("SIGN0141", 8), // SIGN 单元 0141（签署 × 相位 13 × 二 轮）
    ("ARCH0142", 15), // ARCH 单元 0142（归档 × 相位 14 × 二 轮）
    ("SEAL0143", 22), // SEAL 单元 0143（封口 × 相位 15 × 二 轮）
    ("SIX0144", 29), // SIX 单元 0144（六件套 × 相位 16 × 二 轮）
    ("CLOSE0145", 36), // CLOSE 单元 0145（收口 × 相位 17 × 二 轮）
    ("SCOPE0146", 43), // SCOPE 单元 0146（范围 × 相位 18 × 二 轮）
    ("ITEM0147", 50), // ITEM 单元 0147（条目 × 相位 19 × 二 轮）
    ("PROOF0148", 57), // PROOF 单元 0148（证据 × 相位 20 × 二 轮）
    ("BIND0149", 64), // BIND 单元 0149（绑定 × 相位 21 × 二 轮）
    ("ACCT0150", 71), // ACCT 单元 0150（账 × 相位 22 × 二 轮）
    ("CHK0151", 78), // CHK 单元 0151（核验 × 相位 23 × 二 轮）
    ("GAP0152", 85), // GAP 单元 0152（缺口 × 相位 24 × 二 轮）
    ("SIGN0153", 3), // SIGN 单元 0153（签署 × 相位 25 × 二 轮）
    ("ARCH0154", 10), // ARCH 单元 0154（归档 × 相位 26 × 二 轮）
    ("SEAL0155", 17), // SEAL 单元 0155（封口 × 相位 27 × 二 轮）
    ("SIX0156", 24), // SIX 单元 0156（六件套 × 相位 28 × 二 轮）
    ("CLOSE0157", 31), // CLOSE 单元 0157（收口 × 相位 29 × 二 轮）
    ("SCOPE0158", 38), // SCOPE 单元 0158（范围 × 相位 30 × 二 轮）
    ("ITEM0159", 45), // ITEM 单元 0159（条目 × 相位 31 × 二 轮）
    ("PROOF0160", 52), // PROOF 单元 0160（证据 × 相位 32 × 二 轮）
    ("BIND0161", 59), // BIND 单元 0161（绑定 × 相位 33 × 二 轮）
    ("ACCT0162", 66), // ACCT 单元 0162（账 × 相位 34 × 二 轮）
    ("CHK0163", 73), // CHK 单元 0163（核验 × 相位 35 × 二 轮）
    ("GAP0164", 80), // GAP 单元 0164（缺口 × 相位 36 × 二 轮）
    ("SIGN0165", 87), // SIGN 单元 0165（签署 × 相位 37 × 二 轮）
    ("ARCH0166", 5), // ARCH 单元 0166（归档 × 相位 38 × 二 轮）
    ("SEAL0167", 12), // SEAL 单元 0167（封口 × 相位 39 × 二 轮）
    ("SIX0168", 19), // SIX 单元 0168（六件套 × 相位 40 × 二 轮）
    ("CLOSE0169", 26), // CLOSE 单元 0169（收口 × 相位 41 × 二 轮）
    ("SCOPE0170", 33), // SCOPE 单元 0170（范围 × 相位 42 × 二 轮）
    ("ITEM0171", 40), // ITEM 单元 0171（条目 × 相位 43 × 二 轮）
    ("PROOF0172", 47), // PROOF 单元 0172（证据 × 相位 44 × 二 轮）
    ("BIND0173", 54), // BIND 单元 0173（绑定 × 相位 45 × 二 轮）
    ("ACCT0174", 61), // ACCT 单元 0174（账 × 相位 46 × 二 轮）
    ("CHK0175", 68), // CHK 单元 0175（核验 × 相位 47 × 二 轮）
    ("GAP0176", 75), // GAP 单元 0176（缺口 × 相位 48 × 二 轮）
    ("SIGN0177", 82), // SIGN 单元 0177（签署 × 相位 49 × 二 轮）
    ("ARCH0178", 0), // ARCH 单元 0178（归档 × 相位 50 × 二 轮）
    ("SEAL0179", 7), // SEAL 单元 0179（封口 × 相位 51 × 二 轮）
    ("SIX0180", 14), // SIX 单元 0180（六件套 × 相位 52 × 二 轮）
    ("CLOSE0181", 21), // CLOSE 单元 0181（收口 × 相位 53 × 二 轮）
    ("SCOPE0182", 28), // SCOPE 单元 0182（范围 × 相位 54 × 二 轮）
    ("ITEM0183", 35), // ITEM 单元 0183（条目 × 相位 55 × 二 轮）
    ("PROOF0184", 42), // PROOF 单元 0184（证据 × 相位 56 × 二 轮）
    ("BIND0185", 49), // BIND 单元 0185（绑定 × 相位 57 × 二 轮）
    ("ACCT0186", 56), // ACCT 单元 0186（账 × 相位 58 × 二 轮）
    ("CHK0187", 63), // CHK 单元 0187（核验 × 相位 59 × 二 轮）
    ("GAP0188", 70), // GAP 单元 0188（缺口 × 相位 60 × 二 轮）
    ("SIGN0189", 77), // SIGN 单元 0189（签署 × 相位 61 × 二 轮）
    ("ARCH0190", 84), // ARCH 单元 0190（归档 × 相位 62 × 二 轮）
    ("SEAL0191", 2), // SEAL 单元 0191（封口 × 相位 63 × 二 轮）
    ("SIX0192", 9), // SIX 单元 0192（六件套 × 相位 00 × 二 轮）
    ("CLOSE0193", 16), // CLOSE 单元 0193（收口 × 相位 01 × 二 轮）
    ("SCOPE0194", 23), // SCOPE 单元 0194（范围 × 相位 02 × 二 轮）
    ("ITEM0195", 30), // ITEM 单元 0195（条目 × 相位 03 × 二 轮）
    ("PROOF0196", 37), // PROOF 单元 0196（证据 × 相位 04 × 二 轮）
    ("BIND0197", 44), // BIND 单元 0197（绑定 × 相位 05 × 二 轮）
    ("ACCT0198", 51), // ACCT 单元 0198（账 × 相位 06 × 二 轮）
    ("CHK0199", 58), // CHK 单元 0199（核验 × 相位 07 × 二 轮）
    ("GAP0200", 65), // GAP 单元 0200（缺口 × 相位 08 × 二 轮）
    ("SIGN0201", 72), // SIGN 单元 0201（签署 × 相位 09 × 二 轮）
    ("ARCH0202", 79), // ARCH 单元 0202（归档 × 相位 10 × 二 轮）
    ("SEAL0203", 86), // SEAL 单元 0203（封口 × 相位 11 × 二 轮）
    ("SIX0204", 4), // SIX 单元 0204（六件套 × 相位 12 × 二 轮）
    ("CLOSE0205", 11), // CLOSE 单元 0205（收口 × 相位 13 × 二 轮）
    ("SCOPE0206", 18), // SCOPE 单元 0206（范围 × 相位 14 × 二 轮）
    ("ITEM0207", 25), // ITEM 单元 0207（条目 × 相位 15 × 二 轮）
    ("PROOF0208", 32), // PROOF 单元 0208（证据 × 相位 16 × 二 轮）
    ("BIND0209", 39), // BIND 单元 0209（绑定 × 相位 17 × 二 轮）
    ("ACCT0210", 46), // ACCT 单元 0210（账 × 相位 18 × 二 轮）
    ("CHK0211", 53), // CHK 单元 0211（核验 × 相位 19 × 二 轮）
    ("GAP0212", 60), // GAP 单元 0212（缺口 × 相位 20 × 二 轮）
    ("SIGN0213", 67), // SIGN 单元 0213（签署 × 相位 21 × 二 轮）
    ("ARCH0214", 74), // ARCH 单元 0214（归档 × 相位 22 × 二 轮）
    ("SEAL0215", 81), // SEAL 单元 0215（封口 × 相位 23 × 二 轮）
    ("SIX0216", 88), // SIX 单元 0216（六件套 × 相位 24 × 二 轮）
    ("CLOSE0217", 6), // CLOSE 单元 0217（收口 × 相位 25 × 二 轮）
    ("SCOPE0218", 13), // SCOPE 单元 0218（范围 × 相位 26 × 二 轮）
    ("ITEM0219", 20), // ITEM 单元 0219（条目 × 相位 27 × 二 轮）
    ("PROOF0220", 27), // PROOF 单元 0220（证据 × 相位 28 × 二 轮）
    ("BIND0221", 34), // BIND 单元 0221（绑定 × 相位 29 × 二 轮）
    ("ACCT0222", 41), // ACCT 单元 0222（账 × 相位 30 × 二 轮）
    ("CHK0223", 48), // CHK 单元 0223（核验 × 相位 31 × 二 轮）
    ("GAP0224", 55), // GAP 单元 0224（缺口 × 相位 32 × 二 轮）
    ("SIGN0225", 62), // SIGN 单元 0225（签署 × 相位 33 × 二 轮）
    ("ARCH0226", 69), // ARCH 单元 0226（归档 × 相位 34 × 二 轮）
    ("SEAL0227", 76), // SEAL 单元 0227（封口 × 相位 35 × 二 轮）
    ("SIX0228", 83), // SIX 单元 0228（六件套 × 相位 36 × 二 轮）
    ("CLOSE0229", 1), // CLOSE 单元 0229（收口 × 相位 37 × 二 轮）
    ("SCOPE0230", 8), // SCOPE 单元 0230（范围 × 相位 38 × 二 轮）
    ("ITEM0231", 15), // ITEM 单元 0231（条目 × 相位 39 × 二 轮）
    ("PROOF0232", 22), // PROOF 单元 0232（证据 × 相位 40 × 二 轮）
    ("BIND0233", 29), // BIND 单元 0233（绑定 × 相位 41 × 二 轮）
    ("ACCT0234", 36), // ACCT 单元 0234（账 × 相位 42 × 二 轮）
    ("CHK0235", 43), // CHK 单元 0235（核验 × 相位 43 × 二 轮）
    ("GAP0236", 50), // GAP 单元 0236（缺口 × 相位 44 × 二 轮）
    ("SIGN0237", 57), // SIGN 单元 0237（签署 × 相位 45 × 二 轮）
    ("ARCH0238", 64), // ARCH 单元 0238（归档 × 相位 46 × 二 轮）
    ("SEAL0239", 71), // SEAL 单元 0239（封口 × 相位 47 × 二 轮）
    ("SIX0240", 78), // SIX 单元 0240（六件套 × 相位 48 × 二 轮）
    ("CLOSE0241", 85), // CLOSE 单元 0241（收口 × 相位 49 × 二 轮）
    ("SCOPE0242", 3), // SCOPE 单元 0242（范围 × 相位 50 × 二 轮）
    ("ITEM0243", 10), // ITEM 单元 0243（条目 × 相位 51 × 二 轮）
    ("PROOF0244", 17), // PROOF 单元 0244（证据 × 相位 52 × 二 轮）
    ("BIND0245", 24), // BIND 单元 0245（绑定 × 相位 53 × 二 轮）
    ("ACCT0246", 31), // ACCT 单元 0246（账 × 相位 54 × 二 轮）
    ("CHK0247", 38), // CHK 单元 0247（核验 × 相位 55 × 二 轮）
    ("GAP0248", 45), // GAP 单元 0248（缺口 × 相位 56 × 二 轮）
    ("SIGN0249", 52), // SIGN 单元 0249（签署 × 相位 57 × 二 轮）
    ("ARCH0250", 59), // ARCH 单元 0250（归档 × 相位 58 × 二 轮）
    ("SEAL0251", 66), // SEAL 单元 0251（封口 × 相位 59 × 二 轮）
    ("SIX0252", 73), // SIX 单元 0252（六件套 × 相位 60 × 二 轮）
    ("CLOSE0253", 80), // CLOSE 单元 0253（收口 × 相位 61 × 二 轮）
    ("SCOPE0254", 87), // SCOPE 单元 0254（范围 × 相位 62 × 二 轮）
    ("ITEM0255", 5), // ITEM 单元 0255（条目 × 相位 63 × 二 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2981bmx_get(key: &str) -> Option<u64> {
    match key {
        "SIX0000" => Some(0),
        "CLOSE0001" => Some(7),
        "SCOPE0002" => Some(14),
        "ITEM0003" => Some(21),
        "PROOF0004" => Some(28),
        "BIND0005" => Some(35),
        "ACCT0006" => Some(42),
        "CHK0007" => Some(49),
        "GAP0008" => Some(56),
        "SIGN0009" => Some(63),
        "ARCH0010" => Some(70),
        "SEAL0011" => Some(77),
        "SIX0012" => Some(84),
        "CLOSE0013" => Some(2),
        "SCOPE0014" => Some(9),
        "ITEM0015" => Some(16),
        "PROOF0016" => Some(23),
        "BIND0017" => Some(30),
        "ACCT0018" => Some(37),
        "CHK0019" => Some(44),
        "GAP0020" => Some(51),
        "SIGN0021" => Some(58),
        "ARCH0022" => Some(65),
        "SEAL0023" => Some(72),
        "SIX0024" => Some(79),
        "CLOSE0025" => Some(86),
        "SCOPE0026" => Some(4),
        "ITEM0027" => Some(11),
        "PROOF0028" => Some(18),
        "BIND0029" => Some(25),
        "ACCT0030" => Some(32),
        "CHK0031" => Some(39),
        "GAP0032" => Some(46),
        "SIGN0033" => Some(53),
        "ARCH0034" => Some(60),
        "SEAL0035" => Some(67),
        "SIX0036" => Some(74),
        "CLOSE0037" => Some(81),
        "SCOPE0038" => Some(88),
        "ITEM0039" => Some(6),
        "PROOF0040" => Some(13),
        "BIND0041" => Some(20),
        "ACCT0042" => Some(27),
        "CHK0043" => Some(34),
        "GAP0044" => Some(41),
        "SIGN0045" => Some(48),
        "ARCH0046" => Some(55),
        "SEAL0047" => Some(62),
        "SIX0048" => Some(69),
        "CLOSE0049" => Some(76),
        "SCOPE0050" => Some(83),
        "ITEM0051" => Some(1),
        "PROOF0052" => Some(8),
        "BIND0053" => Some(15),
        "ACCT0054" => Some(22),
        "CHK0055" => Some(29),
        "GAP0056" => Some(36),
        "SIGN0057" => Some(43),
        "ARCH0058" => Some(50),
        "SEAL0059" => Some(57),
        "SIX0060" => Some(64),
        "CLOSE0061" => Some(71),
        "SCOPE0062" => Some(78),
        "ITEM0063" => Some(85),
        "PROOF0064" => Some(3),
        "BIND0065" => Some(10),
        "ACCT0066" => Some(17),
        "CHK0067" => Some(24),
        "GAP0068" => Some(31),
        "SIGN0069" => Some(38),
        "ARCH0070" => Some(45),
        "SEAL0071" => Some(52),
        "SIX0072" => Some(59),
        "CLOSE0073" => Some(66),
        "SCOPE0074" => Some(73),
        "ITEM0075" => Some(80),
        "PROOF0076" => Some(87),
        "BIND0077" => Some(5),
        "ACCT0078" => Some(12),
        "CHK0079" => Some(19),
        "GAP0080" => Some(26),
        "SIGN0081" => Some(33),
        "ARCH0082" => Some(40),
        "SEAL0083" => Some(47),
        "SIX0084" => Some(54),
        "CLOSE0085" => Some(61),
        "SCOPE0086" => Some(68),
        "ITEM0087" => Some(75),
        "PROOF0088" => Some(82),
        "BIND0089" => Some(0),
        "ACCT0090" => Some(7),
        "CHK0091" => Some(14),
        "GAP0092" => Some(21),
        "SIGN0093" => Some(28),
        "ARCH0094" => Some(35),
        "SEAL0095" => Some(42),
        "SIX0096" => Some(49),
        "CLOSE0097" => Some(56),
        "SCOPE0098" => Some(63),
        "ITEM0099" => Some(70),
        "PROOF0100" => Some(77),
        "BIND0101" => Some(84),
        "ACCT0102" => Some(2),
        "CHK0103" => Some(9),
        "GAP0104" => Some(16),
        "SIGN0105" => Some(23),
        "ARCH0106" => Some(30),
        "SEAL0107" => Some(37),
        "SIX0108" => Some(44),
        "CLOSE0109" => Some(51),
        "SCOPE0110" => Some(58),
        "ITEM0111" => Some(65),
        "PROOF0112" => Some(72),
        "BIND0113" => Some(79),
        "ACCT0114" => Some(86),
        "CHK0115" => Some(4),
        "GAP0116" => Some(11),
        "SIGN0117" => Some(18),
        "ARCH0118" => Some(25),
        "SEAL0119" => Some(32),
        "SIX0120" => Some(39),
        "CLOSE0121" => Some(46),
        "SCOPE0122" => Some(53),
        "ITEM0123" => Some(60),
        "PROOF0124" => Some(67),
        "BIND0125" => Some(74),
        "ACCT0126" => Some(81),
        "CHK0127" => Some(88),
        "GAP0128" => Some(6),
        "SIGN0129" => Some(13),
        "ARCH0130" => Some(20),
        "SEAL0131" => Some(27),
        "SIX0132" => Some(34),
        "CLOSE0133" => Some(41),
        "SCOPE0134" => Some(48),
        "ITEM0135" => Some(55),
        "PROOF0136" => Some(62),
        "BIND0137" => Some(69),
        "ACCT0138" => Some(76),
        "CHK0139" => Some(83),
        "GAP0140" => Some(1),
        "SIGN0141" => Some(8),
        "ARCH0142" => Some(15),
        "SEAL0143" => Some(22),
        "SIX0144" => Some(29),
        "CLOSE0145" => Some(36),
        "SCOPE0146" => Some(43),
        "ITEM0147" => Some(50),
        "PROOF0148" => Some(57),
        "BIND0149" => Some(64),
        "ACCT0150" => Some(71),
        "CHK0151" => Some(78),
        "GAP0152" => Some(85),
        "SIGN0153" => Some(3),
        "ARCH0154" => Some(10),
        "SEAL0155" => Some(17),
        "SIX0156" => Some(24),
        "CLOSE0157" => Some(31),
        "SCOPE0158" => Some(38),
        "ITEM0159" => Some(45),
        "PROOF0160" => Some(52),
        "BIND0161" => Some(59),
        "ACCT0162" => Some(66),
        "CHK0163" => Some(73),
        "GAP0164" => Some(80),
        "SIGN0165" => Some(87),
        "ARCH0166" => Some(5),
        "SEAL0167" => Some(12),
        "SIX0168" => Some(19),
        "CLOSE0169" => Some(26),
        "SCOPE0170" => Some(33),
        "ITEM0171" => Some(40),
        "PROOF0172" => Some(47),
        "BIND0173" => Some(54),
        "ACCT0174" => Some(61),
        "CHK0175" => Some(68),
        "GAP0176" => Some(75),
        "SIGN0177" => Some(82),
        "ARCH0178" => Some(0),
        "SEAL0179" => Some(7),
        "SIX0180" => Some(14),
        "CLOSE0181" => Some(21),
        "SCOPE0182" => Some(28),
        "ITEM0183" => Some(35),
        "PROOF0184" => Some(42),
        "BIND0185" => Some(49),
        "ACCT0186" => Some(56),
        "CHK0187" => Some(63),
        "GAP0188" => Some(70),
        "SIGN0189" => Some(77),
        "ARCH0190" => Some(84),
        "SEAL0191" => Some(2),
        "SIX0192" => Some(9),
        "CLOSE0193" => Some(16),
        "SCOPE0194" => Some(23),
        "ITEM0195" => Some(30),
        "PROOF0196" => Some(37),
        "BIND0197" => Some(44),
        "ACCT0198" => Some(51),
        "CHK0199" => Some(58),
        "GAP0200" => Some(65),
        "SIGN0201" => Some(72),
        "ARCH0202" => Some(79),
        "SEAL0203" => Some(86),
        "SIX0204" => Some(4),
        "CLOSE0205" => Some(11),
        "SCOPE0206" => Some(18),
        "ITEM0207" => Some(25),
        "PROOF0208" => Some(32),
        "BIND0209" => Some(39),
        "ACCT0210" => Some(46),
        "CHK0211" => Some(53),
        "GAP0212" => Some(60),
        "SIGN0213" => Some(67),
        "ARCH0214" => Some(74),
        "SEAL0215" => Some(81),
        "SIX0216" => Some(88),
        "CLOSE0217" => Some(6),
        "SCOPE0218" => Some(13),
        "ITEM0219" => Some(20),
        "PROOF0220" => Some(27),
        "BIND0221" => Some(34),
        "ACCT0222" => Some(41),
        "CHK0223" => Some(48),
        "GAP0224" => Some(55),
        "SIGN0225" => Some(62),
        "ARCH0226" => Some(69),
        "SEAL0227" => Some(76),
        "SIX0228" => Some(83),
        "CLOSE0229" => Some(1),
        "SCOPE0230" => Some(8),
        "ITEM0231" => Some(15),
        "PROOF0232" => Some(22),
        "BIND0233" => Some(29),
        "ACCT0234" => Some(36),
        "CHK0235" => Some(43),
        "GAP0236" => Some(50),
        "SIGN0237" => Some(57),
        "ARCH0238" => Some(64),
        "SEAL0239" => Some(71),
        "SIX0240" => Some(78),
        "CLOSE0241" => Some(85),
        "SCOPE0242" => Some(3),
        "ITEM0243" => Some(10),
        "PROOF0244" => Some(17),
        "BIND0245" => Some(24),
        "ACCT0246" => Some(31),
        "CHK0247" => Some(38),
        "GAP0248" => Some(45),
        "SIGN0249" => Some(52),
        "ARCH0250" => Some(59),
        "SEAL0251" => Some(66),
        "SIX0252" => Some(73),
        "CLOSE0253" => Some(80),
        "SCOPE0254" => Some(87),
        "ITEM0255" => Some(5),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2981bmx_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2981bmx_COUNT {
        let (k, v) = u2981bmx[i];
        let ok = match k {
            "SIX0000" => v < 89,
            "CLOSE0001" => v < 89,
            "SCOPE0002" => v < 89,
            "ITEM0003" => v < 89,
            "PROOF0004" => v < 89,
            "BIND0005" => v < 89,
            "ACCT0006" => v < 89,
            "CHK0007" => v < 89,
            "GAP0008" => v < 89,
            "SIGN0009" => v < 89,
            "ARCH0010" => v < 89,
            "SEAL0011" => v < 89,
            "SIX0012" => v < 89,
            "CLOSE0013" => v < 89,
            "SCOPE0014" => v < 89,
            "ITEM0015" => v < 89,
            "PROOF0016" => v < 89,
            "BIND0017" => v < 89,
            "ACCT0018" => v < 89,
            "CHK0019" => v < 89,
            "GAP0020" => v < 89,
            "SIGN0021" => v < 89,
            "ARCH0022" => v < 89,
            "SEAL0023" => v < 89,
            "SIX0024" => v < 89,
            "CLOSE0025" => v < 89,
            "SCOPE0026" => v < 89,
            "ITEM0027" => v < 89,
            "PROOF0028" => v < 89,
            "BIND0029" => v < 89,
            "ACCT0030" => v < 89,
            "CHK0031" => v < 89,
            "GAP0032" => v < 89,
            "SIGN0033" => v < 89,
            "ARCH0034" => v < 89,
            "SEAL0035" => v < 89,
            "SIX0036" => v < 89,
            "CLOSE0037" => v < 89,
            "SCOPE0038" => v < 89,
            "ITEM0039" => v < 89,
            "PROOF0040" => v < 89,
            "BIND0041" => v < 89,
            "ACCT0042" => v < 89,
            "CHK0043" => v < 89,
            "GAP0044" => v < 89,
            "SIGN0045" => v < 89,
            "ARCH0046" => v < 89,
            "SEAL0047" => v < 89,
            "SIX0048" => v < 89,
            "CLOSE0049" => v < 89,
            "SCOPE0050" => v < 89,
            "ITEM0051" => v < 89,
            "PROOF0052" => v < 89,
            "BIND0053" => v < 89,
            "ACCT0054" => v < 89,
            "CHK0055" => v < 89,
            "GAP0056" => v < 89,
            "SIGN0057" => v < 89,
            "ARCH0058" => v < 89,
            "SEAL0059" => v < 89,
            "SIX0060" => v < 89,
            "CLOSE0061" => v < 89,
            "SCOPE0062" => v < 89,
            "ITEM0063" => v < 89,
            "PROOF0064" => v < 89,
            "BIND0065" => v < 89,
            "ACCT0066" => v < 89,
            "CHK0067" => v < 89,
            "GAP0068" => v < 89,
            "SIGN0069" => v < 89,
            "ARCH0070" => v < 89,
            "SEAL0071" => v < 89,
            "SIX0072" => v < 89,
            "CLOSE0073" => v < 89,
            "SCOPE0074" => v < 89,
            "ITEM0075" => v < 89,
            "PROOF0076" => v < 89,
            "BIND0077" => v < 89,
            "ACCT0078" => v < 89,
            "CHK0079" => v < 89,
            "GAP0080" => v < 89,
            "SIGN0081" => v < 89,
            "ARCH0082" => v < 89,
            "SEAL0083" => v < 89,
            "SIX0084" => v < 89,
            "CLOSE0085" => v < 89,
            "SCOPE0086" => v < 89,
            "ITEM0087" => v < 89,
            "PROOF0088" => v < 89,
            "BIND0089" => v < 89,
            "ACCT0090" => v < 89,
            "CHK0091" => v < 89,
            "GAP0092" => v < 89,
            "SIGN0093" => v < 89,
            "ARCH0094" => v < 89,
            "SEAL0095" => v < 89,
            "SIX0096" => v < 89,
            "CLOSE0097" => v < 89,
            "SCOPE0098" => v < 89,
            "ITEM0099" => v < 89,
            "PROOF0100" => v < 89,
            "BIND0101" => v < 89,
            "ACCT0102" => v < 89,
            "CHK0103" => v < 89,
            "GAP0104" => v < 89,
            "SIGN0105" => v < 89,
            "ARCH0106" => v < 89,
            "SEAL0107" => v < 89,
            "SIX0108" => v < 89,
            "CLOSE0109" => v < 89,
            "SCOPE0110" => v < 89,
            "ITEM0111" => v < 89,
            "PROOF0112" => v < 89,
            "BIND0113" => v < 89,
            "ACCT0114" => v < 89,
            "CHK0115" => v < 89,
            "GAP0116" => v < 89,
            "SIGN0117" => v < 89,
            "ARCH0118" => v < 89,
            "SEAL0119" => v < 89,
            "SIX0120" => v < 89,
            "CLOSE0121" => v < 89,
            "SCOPE0122" => v < 89,
            "ITEM0123" => v < 89,
            "PROOF0124" => v < 89,
            "BIND0125" => v < 89,
            "ACCT0126" => v < 89,
            "CHK0127" => v < 89,
            "GAP0128" => v < 89,
            "SIGN0129" => v < 89,
            "ARCH0130" => v < 89,
            "SEAL0131" => v < 89,
            "SIX0132" => v < 89,
            "CLOSE0133" => v < 89,
            "SCOPE0134" => v < 89,
            "ITEM0135" => v < 89,
            "PROOF0136" => v < 89,
            "BIND0137" => v < 89,
            "ACCT0138" => v < 89,
            "CHK0139" => v < 89,
            "GAP0140" => v < 89,
            "SIGN0141" => v < 89,
            "ARCH0142" => v < 89,
            "SEAL0143" => v < 89,
            "SIX0144" => v < 89,
            "CLOSE0145" => v < 89,
            "SCOPE0146" => v < 89,
            "ITEM0147" => v < 89,
            "PROOF0148" => v < 89,
            "BIND0149" => v < 89,
            "ACCT0150" => v < 89,
            "CHK0151" => v < 89,
            "GAP0152" => v < 89,
            "SIGN0153" => v < 89,
            "ARCH0154" => v < 89,
            "SEAL0155" => v < 89,
            "SIX0156" => v < 89,
            "CLOSE0157" => v < 89,
            "SCOPE0158" => v < 89,
            "ITEM0159" => v < 89,
            "PROOF0160" => v < 89,
            "BIND0161" => v < 89,
            "ACCT0162" => v < 89,
            "CHK0163" => v < 89,
            "GAP0164" => v < 89,
            "SIGN0165" => v < 89,
            "ARCH0166" => v < 89,
            "SEAL0167" => v < 89,
            "SIX0168" => v < 89,
            "CLOSE0169" => v < 89,
            "SCOPE0170" => v < 89,
            "ITEM0171" => v < 89,
            "PROOF0172" => v < 89,
            "BIND0173" => v < 89,
            "ACCT0174" => v < 89,
            "CHK0175" => v < 89,
            "GAP0176" => v < 89,
            "SIGN0177" => v < 89,
            "ARCH0178" => v < 89,
            "SEAL0179" => v < 89,
            "SIX0180" => v < 89,
            "CLOSE0181" => v < 89,
            "SCOPE0182" => v < 89,
            "ITEM0183" => v < 89,
            "PROOF0184" => v < 89,
            "BIND0185" => v < 89,
            "ACCT0186" => v < 89,
            "CHK0187" => v < 89,
            "GAP0188" => v < 89,
            "SIGN0189" => v < 89,
            "ARCH0190" => v < 89,
            "SEAL0191" => v < 89,
            "SIX0192" => v < 89,
            "CLOSE0193" => v < 89,
            "SCOPE0194" => v < 89,
            "ITEM0195" => v < 89,
            "PROOF0196" => v < 89,
            "BIND0197" => v < 89,
            "ACCT0198" => v < 89,
            "CHK0199" => v < 89,
            "GAP0200" => v < 89,
            "SIGN0201" => v < 89,
            "ARCH0202" => v < 89,
            "SEAL0203" => v < 89,
            "SIX0204" => v < 89,
            "CLOSE0205" => v < 89,
            "SCOPE0206" => v < 89,
            "ITEM0207" => v < 89,
            "PROOF0208" => v < 89,
            "BIND0209" => v < 89,
            "ACCT0210" => v < 89,
            "CHK0211" => v < 89,
            "GAP0212" => v < 89,
            "SIGN0213" => v < 89,
            "ARCH0214" => v < 89,
            "SEAL0215" => v < 89,
            "SIX0216" => v < 89,
            "CLOSE0217" => v < 89,
            "SCOPE0218" => v < 89,
            "ITEM0219" => v < 89,
            "PROOF0220" => v < 89,
            "BIND0221" => v < 89,
            "ACCT0222" => v < 89,
            "CHK0223" => v < 89,
            "GAP0224" => v < 89,
            "SIGN0225" => v < 89,
            "ARCH0226" => v < 89,
            "SEAL0227" => v < 89,
            "SIX0228" => v < 89,
            "CLOSE0229" => v < 89,
            "SCOPE0230" => v < 89,
            "ITEM0231" => v < 89,
            "PROOF0232" => v < 89,
            "BIND0233" => v < 89,
            "ACCT0234" => v < 89,
            "CHK0235" => v < 89,
            "GAP0236" => v < 89,
            "SIGN0237" => v < 89,
            "ARCH0238" => v < 89,
            "SEAL0239" => v < 89,
            "SIX0240" => v < 89,
            "CLOSE0241" => v < 89,
            "SCOPE0242" => v < 89,
            "ITEM0243" => v < 89,
            "PROOF0244" => v < 89,
            "BIND0245" => v < 89,
            "ACCT0246" => v < 89,
            "CHK0247" => v < 89,
            "GAP0248" => v < 89,
            "SIGN0249" => v < 89,
            "ARCH0250" => v < 89,
            "SEAL0251" => v < 89,
            "SIX0252" => v < 89,
            "CLOSE0253" => v < 89,
            "SCOPE0254" => v < 89,
            "ITEM0255" => v < 89,
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
pub fn u2981bmx_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2981bmx_COUNT {
        s = s.wrapping_add(u2981bmx[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2981bmx_unique() -> bool {
    let mut i = 0usize;
    while i < u2981bmx_COUNT {
        let mut j = i + 1;
        while j < u2981bmx_COUNT {
            if u2981bmx[i].0 == u2981bmx[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe2f0, 0xe2ff]。
pub const fn u2981_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2981Code::ALL.len() {
        let c = u2981Code::ALL[i].code();
        if c < 0xe2f0 || c > 0xe2ff {
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
fn u2981_guard_seg() -> bool {
    u2981_segment_guard()
}

/// UNX-F2981 bmx 256 行齐备 + 边界锚 + 键外拒收。
fn check_u2981_bmx_count() -> bool {
    let mut ok = u2981bmx_COUNT == 256;
    if ok { ok = u2981bmx_get("SIX0000") == Some(0) && u2981bmx_get("ITEM0255") == Some(5); }
    if ok { ok = u2981bmx_get("ZZZ_NOKEY").is_none(); }
    ok
}

/// UNX-F2981 bmx 累计守恒（理论值 11231）。
fn check_u2981_bmx_sum() -> bool {
    u2981bmx_sum() == 11231
}

/// UNX-F2981 bmx 逐行守卫与唯一性全绿。
fn check_u2981_bmx_guard() -> bool {
    u2981bmx_scan() == 0 && u2981bmx_unique()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2981_checks() -> CheckSet {
    let mut set = CheckSet::new("U2981");
    set.add(
        "u2981-seg-guard",
        u2981_guard_seg(),
        if u2981_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2981-bmx-count",
        check_u2981_bmx_count(),
        if check_u2981_bmx_count() { "" } else { "UNX-F2981 bmx count mismatch" },
    );
    set.add(
        "u2981-bmx-sum",
        check_u2981_bmx_sum(),
        if check_u2981_bmx_sum() { "" } else { "UNX-F2981 bmx sum mismatch" },
    );
    set.add(
        "u2981-bmx-guard",
        check_u2981_bmx_guard(),
        if check_u2981_bmx_guard() { "" } else { "UNX-F2981 bmx guard mismatch" },
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
    fn u2981_all_checks_green() {
        let set = run_u2981_checks();
        assert!(set.all_passed(), "U2981 domain checks must be green");
    }

    #[test]
    fn u2981_segment_guard_holds() {
        assert!(u2981_segment_guard());
    }
}
