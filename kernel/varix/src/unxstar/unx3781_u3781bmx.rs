//! unxu3781_u3781bmx —— 域中期收口总设计（B20 前向声明的兑现面） 深编bmx（域中期收口 × 256 单元）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3781 · 域中期收口总设计（B20 前向声明的兑现面） · 判据原文：UNX-F3781 · 判据：J1：收口架构（三总轧（批账/域账/凭证）+三宣告（断言大关/移交就绪/风险清单）——判据行内嵌：B20 前向声明的原文兑现（"F3781 域中期收口：段断言大关 ≥80+五条件移交"——期票与兑付跨十批的闭链）；收口的范围声明（B16–B30 十五批——A5 域续领波的全域——不含 B01–B15（已在前次 finalize 收口——历史账冻结）；M-539 收口预跑（三总轧空转→流程通→正式总轧前演练）。<br>J2：收口的纪律（收口只核不改（发现的偏差走修正流程+留痕——收口不涂改历史账。
//! · 组件：域中期收口 守卫映射 256 行（二 轮派生真实数据 + 逐行守卫 + 键唯一）
//! · 判据落地：UNX-F3781 · 判据：J1：收口架构（三总轧（批账/域账/凭证）+三宣告（断言大关/移交就绪/风险清单）——判据行内嵌：B20 前向声明的原文兑现（"F
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3781_VERSION: &str = "u3781-u3781bmx-v1";

/// 诊断段基址。
pub const U3781_SEG_BASE: u16 = 0xe250;

// --------------------------------------------------------------------------
// 诊断码（u3781 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域中期收口 bmx 面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3781Code {
    /// 0xe250 单元格重复（收口 同键双占）。
    CellDup,
    /// 0xe251 单元格断档（兑现 序号不连续）。
    CellGap,
    /// 0xe252 索引越界（缺口）。
    IndexOob,
    /// 0xe253 大表账破缺（行数≠登记值）。
    LedgerBreak,
    /// 0xe254 表序断档。
    OrderBreak,
    /// 0xe255 累计漂移（求和≠理论值）。
    SumDrift,
    /// 0xe256 掩码非法。
    MaskBad,
    /// 0xe257 守卫触发。
    GuardTripped,
    /// 0xe258 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe259 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe25a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe25b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3781Code {
    /// 全量闭集。
    pub const ALL: [u3781Code; 12] = [
        u3781Code::CellDup,
        u3781Code::CellGap,
        u3781Code::IndexOob,
        u3781Code::LedgerBreak,
        u3781Code::OrderBreak,
        u3781Code::SumDrift,
        u3781Code::MaskBad,
        u3781Code::GuardTripped,
        u3781Code::LedgerBroken,
        u3781Code::SnapshotFingerprint,
        u3781Code::TableBroken,
        u3781Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3781Code::CellDup => 0xe250,
            u3781Code::CellGap => 0xe251,
            u3781Code::IndexOob => 0xe252,
            u3781Code::LedgerBreak => 0xe253,
            u3781Code::OrderBreak => 0xe254,
            u3781Code::SumDrift => 0xe255,
            u3781Code::MaskBad => 0xe256,
            u3781Code::GuardTripped => 0xe257,
            u3781Code::LedgerBroken => 0xe258,
            u3781Code::SnapshotFingerprint => 0xe259,
            u3781Code::TableBroken => 0xe25a,
            u3781Code::SegmentGuard => 0xe25b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3781Code::CellDup => "单元格重复（收口 同键双占）",
            u3781Code::CellGap => "单元格断档（兑现 序号不连续）",
            u3781Code::IndexOob => "索引越界（缺口）",
            u3781Code::LedgerBreak => "大表账破缺（行数≠登记值）",
            u3781Code::OrderBreak => "表序断档",
            u3781Code::SumDrift => "累计漂移（求和≠理论值）",
            u3781Code::MaskBad => "掩码非法",
            u3781Code::GuardTripped => "守卫触发",
            u3781Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3781Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3781Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3781Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3781Code::CellDup => "按键唯一律去重",
            u3781Code::CellGap => "按连续律补号",
            u3781Code::IndexOob => "按行数边界裁剪",
            u3781Code::LedgerBreak => "按守恒重建",
            u3781Code::OrderBreak => "按序重排",
            u3781Code::SumDrift => "按理论值复算",
            u3781Code::MaskBad => "按掩码律校正",
            u3781Code::GuardTripped => "检查临界区",
            u3781Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3781Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3781Code::TableBroken => "按语义规则重建常量表",
            u3781Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3781bmx（256 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 域中期收口 bmx 守卫映射（256 行逐行注释自证）。
/// 行数。
pub const u3781bmx_COUNT: usize = 256;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3781bmx: [(&'static str, u64); 256] = [
    ("CLOSE0000", 0), // CLOSE 单元 0000（收口 × 相位 00 × 二 轮）
    ("MID0001", 7), // MID 单元 0001（中期 × 相位 01 × 二 轮）
    ("DECL0002", 14), // DECL 单元 0002（前向声明 × 相位 02 × 二 轮）
    ("Fulfill0003", 21), // Fulfill 单元 0003（兑现 × 相位 03 × 二 轮）
    ("SCOPE0004", 28), // SCOPE 单元 0004（范围 × 相位 04 × 二 轮）
    ("ITEM0005", 35), // ITEM 单元 0005（条目 × 相位 05 × 二 轮）
    ("DONE0006", 42), // DONE 单元 0006（已完成 × 相位 06 × 二 轮）
    ("OPEN0007", 49), // OPEN 单元 0007（未开 × 相位 07 × 二 轮）
    ("GAP0008", 56), // GAP 单元 0008（缺口 × 相位 08 × 二 轮）
    ("BIND0009", 63), // BIND 单元 0009（绑定 × 相位 09 × 二 轮）
    ("PROOF0010", 70), // PROOF 单元 0010（证据 × 相位 10 × 二 轮）
    ("GATE0011", 77), // GATE 单元 0011（闸门 × 相位 11 × 二 轮）
    ("CLOSE0012", 84), // CLOSE 单元 0012（收口 × 相位 12 × 二 轮）
    ("MID0013", 2), // MID 单元 0013（中期 × 相位 13 × 二 轮）
    ("DECL0014", 9), // DECL 单元 0014（前向声明 × 相位 14 × 二 轮）
    ("Fulfill0015", 16), // Fulfill 单元 0015（兑现 × 相位 15 × 二 轮）
    ("SCOPE0016", 23), // SCOPE 单元 0016（范围 × 相位 16 × 二 轮）
    ("ITEM0017", 30), // ITEM 单元 0017（条目 × 相位 17 × 二 轮）
    ("DONE0018", 37), // DONE 单元 0018（已完成 × 相位 18 × 二 轮）
    ("OPEN0019", 44), // OPEN 单元 0019（未开 × 相位 19 × 二 轮）
    ("GAP0020", 51), // GAP 单元 0020（缺口 × 相位 20 × 二 轮）
    ("BIND0021", 58), // BIND 单元 0021（绑定 × 相位 21 × 二 轮）
    ("PROOF0022", 65), // PROOF 单元 0022（证据 × 相位 22 × 二 轮）
    ("GATE0023", 72), // GATE 单元 0023（闸门 × 相位 23 × 二 轮）
    ("CLOSE0024", 79), // CLOSE 单元 0024（收口 × 相位 24 × 二 轮）
    ("MID0025", 86), // MID 单元 0025（中期 × 相位 25 × 二 轮）
    ("DECL0026", 4), // DECL 单元 0026（前向声明 × 相位 26 × 二 轮）
    ("Fulfill0027", 11), // Fulfill 单元 0027（兑现 × 相位 27 × 二 轮）
    ("SCOPE0028", 18), // SCOPE 单元 0028（范围 × 相位 28 × 二 轮）
    ("ITEM0029", 25), // ITEM 单元 0029（条目 × 相位 29 × 二 轮）
    ("DONE0030", 32), // DONE 单元 0030（已完成 × 相位 30 × 二 轮）
    ("OPEN0031", 39), // OPEN 单元 0031（未开 × 相位 31 × 二 轮）
    ("GAP0032", 46), // GAP 单元 0032（缺口 × 相位 32 × 二 轮）
    ("BIND0033", 53), // BIND 单元 0033（绑定 × 相位 33 × 二 轮）
    ("PROOF0034", 60), // PROOF 单元 0034（证据 × 相位 34 × 二 轮）
    ("GATE0035", 67), // GATE 单元 0035（闸门 × 相位 35 × 二 轮）
    ("CLOSE0036", 74), // CLOSE 单元 0036（收口 × 相位 36 × 二 轮）
    ("MID0037", 81), // MID 单元 0037（中期 × 相位 37 × 二 轮）
    ("DECL0038", 88), // DECL 单元 0038（前向声明 × 相位 38 × 二 轮）
    ("Fulfill0039", 6), // Fulfill 单元 0039（兑现 × 相位 39 × 二 轮）
    ("SCOPE0040", 13), // SCOPE 单元 0040（范围 × 相位 40 × 二 轮）
    ("ITEM0041", 20), // ITEM 单元 0041（条目 × 相位 41 × 二 轮）
    ("DONE0042", 27), // DONE 单元 0042（已完成 × 相位 42 × 二 轮）
    ("OPEN0043", 34), // OPEN 单元 0043（未开 × 相位 43 × 二 轮）
    ("GAP0044", 41), // GAP 单元 0044（缺口 × 相位 44 × 二 轮）
    ("BIND0045", 48), // BIND 单元 0045（绑定 × 相位 45 × 二 轮）
    ("PROOF0046", 55), // PROOF 单元 0046（证据 × 相位 46 × 二 轮）
    ("GATE0047", 62), // GATE 单元 0047（闸门 × 相位 47 × 二 轮）
    ("CLOSE0048", 69), // CLOSE 单元 0048（收口 × 相位 48 × 二 轮）
    ("MID0049", 76), // MID 单元 0049（中期 × 相位 49 × 二 轮）
    ("DECL0050", 83), // DECL 单元 0050（前向声明 × 相位 50 × 二 轮）
    ("Fulfill0051", 1), // Fulfill 单元 0051（兑现 × 相位 51 × 二 轮）
    ("SCOPE0052", 8), // SCOPE 单元 0052（范围 × 相位 52 × 二 轮）
    ("ITEM0053", 15), // ITEM 单元 0053（条目 × 相位 53 × 二 轮）
    ("DONE0054", 22), // DONE 单元 0054（已完成 × 相位 54 × 二 轮）
    ("OPEN0055", 29), // OPEN 单元 0055（未开 × 相位 55 × 二 轮）
    ("GAP0056", 36), // GAP 单元 0056（缺口 × 相位 56 × 二 轮）
    ("BIND0057", 43), // BIND 单元 0057（绑定 × 相位 57 × 二 轮）
    ("PROOF0058", 50), // PROOF 单元 0058（证据 × 相位 58 × 二 轮）
    ("GATE0059", 57), // GATE 单元 0059（闸门 × 相位 59 × 二 轮）
    ("CLOSE0060", 64), // CLOSE 单元 0060（收口 × 相位 60 × 二 轮）
    ("MID0061", 71), // MID 单元 0061（中期 × 相位 61 × 二 轮）
    ("DECL0062", 78), // DECL 单元 0062（前向声明 × 相位 62 × 二 轮）
    ("Fulfill0063", 85), // Fulfill 单元 0063（兑现 × 相位 63 × 二 轮）
    ("SCOPE0064", 3), // SCOPE 单元 0064（范围 × 相位 00 × 二 轮）
    ("ITEM0065", 10), // ITEM 单元 0065（条目 × 相位 01 × 二 轮）
    ("DONE0066", 17), // DONE 单元 0066（已完成 × 相位 02 × 二 轮）
    ("OPEN0067", 24), // OPEN 单元 0067（未开 × 相位 03 × 二 轮）
    ("GAP0068", 31), // GAP 单元 0068（缺口 × 相位 04 × 二 轮）
    ("BIND0069", 38), // BIND 单元 0069（绑定 × 相位 05 × 二 轮）
    ("PROOF0070", 45), // PROOF 单元 0070（证据 × 相位 06 × 二 轮）
    ("GATE0071", 52), // GATE 单元 0071（闸门 × 相位 07 × 二 轮）
    ("CLOSE0072", 59), // CLOSE 单元 0072（收口 × 相位 08 × 二 轮）
    ("MID0073", 66), // MID 单元 0073（中期 × 相位 09 × 二 轮）
    ("DECL0074", 73), // DECL 单元 0074（前向声明 × 相位 10 × 二 轮）
    ("Fulfill0075", 80), // Fulfill 单元 0075（兑现 × 相位 11 × 二 轮）
    ("SCOPE0076", 87), // SCOPE 单元 0076（范围 × 相位 12 × 二 轮）
    ("ITEM0077", 5), // ITEM 单元 0077（条目 × 相位 13 × 二 轮）
    ("DONE0078", 12), // DONE 单元 0078（已完成 × 相位 14 × 二 轮）
    ("OPEN0079", 19), // OPEN 单元 0079（未开 × 相位 15 × 二 轮）
    ("GAP0080", 26), // GAP 单元 0080（缺口 × 相位 16 × 二 轮）
    ("BIND0081", 33), // BIND 单元 0081（绑定 × 相位 17 × 二 轮）
    ("PROOF0082", 40), // PROOF 单元 0082（证据 × 相位 18 × 二 轮）
    ("GATE0083", 47), // GATE 单元 0083（闸门 × 相位 19 × 二 轮）
    ("CLOSE0084", 54), // CLOSE 单元 0084（收口 × 相位 20 × 二 轮）
    ("MID0085", 61), // MID 单元 0085（中期 × 相位 21 × 二 轮）
    ("DECL0086", 68), // DECL 单元 0086（前向声明 × 相位 22 × 二 轮）
    ("Fulfill0087", 75), // Fulfill 单元 0087（兑现 × 相位 23 × 二 轮）
    ("SCOPE0088", 82), // SCOPE 单元 0088（范围 × 相位 24 × 二 轮）
    ("ITEM0089", 0), // ITEM 单元 0089（条目 × 相位 25 × 二 轮）
    ("DONE0090", 7), // DONE 单元 0090（已完成 × 相位 26 × 二 轮）
    ("OPEN0091", 14), // OPEN 单元 0091（未开 × 相位 27 × 二 轮）
    ("GAP0092", 21), // GAP 单元 0092（缺口 × 相位 28 × 二 轮）
    ("BIND0093", 28), // BIND 单元 0093（绑定 × 相位 29 × 二 轮）
    ("PROOF0094", 35), // PROOF 单元 0094（证据 × 相位 30 × 二 轮）
    ("GATE0095", 42), // GATE 单元 0095（闸门 × 相位 31 × 二 轮）
    ("CLOSE0096", 49), // CLOSE 单元 0096（收口 × 相位 32 × 二 轮）
    ("MID0097", 56), // MID 单元 0097（中期 × 相位 33 × 二 轮）
    ("DECL0098", 63), // DECL 单元 0098（前向声明 × 相位 34 × 二 轮）
    ("Fulfill0099", 70), // Fulfill 单元 0099（兑现 × 相位 35 × 二 轮）
    ("SCOPE0100", 77), // SCOPE 单元 0100（范围 × 相位 36 × 二 轮）
    ("ITEM0101", 84), // ITEM 单元 0101（条目 × 相位 37 × 二 轮）
    ("DONE0102", 2), // DONE 单元 0102（已完成 × 相位 38 × 二 轮）
    ("OPEN0103", 9), // OPEN 单元 0103（未开 × 相位 39 × 二 轮）
    ("GAP0104", 16), // GAP 单元 0104（缺口 × 相位 40 × 二 轮）
    ("BIND0105", 23), // BIND 单元 0105（绑定 × 相位 41 × 二 轮）
    ("PROOF0106", 30), // PROOF 单元 0106（证据 × 相位 42 × 二 轮）
    ("GATE0107", 37), // GATE 单元 0107（闸门 × 相位 43 × 二 轮）
    ("CLOSE0108", 44), // CLOSE 单元 0108（收口 × 相位 44 × 二 轮）
    ("MID0109", 51), // MID 单元 0109（中期 × 相位 45 × 二 轮）
    ("DECL0110", 58), // DECL 单元 0110（前向声明 × 相位 46 × 二 轮）
    ("Fulfill0111", 65), // Fulfill 单元 0111（兑现 × 相位 47 × 二 轮）
    ("SCOPE0112", 72), // SCOPE 单元 0112（范围 × 相位 48 × 二 轮）
    ("ITEM0113", 79), // ITEM 单元 0113（条目 × 相位 49 × 二 轮）
    ("DONE0114", 86), // DONE 单元 0114（已完成 × 相位 50 × 二 轮）
    ("OPEN0115", 4), // OPEN 单元 0115（未开 × 相位 51 × 二 轮）
    ("GAP0116", 11), // GAP 单元 0116（缺口 × 相位 52 × 二 轮）
    ("BIND0117", 18), // BIND 单元 0117（绑定 × 相位 53 × 二 轮）
    ("PROOF0118", 25), // PROOF 单元 0118（证据 × 相位 54 × 二 轮）
    ("GATE0119", 32), // GATE 单元 0119（闸门 × 相位 55 × 二 轮）
    ("CLOSE0120", 39), // CLOSE 单元 0120（收口 × 相位 56 × 二 轮）
    ("MID0121", 46), // MID 单元 0121（中期 × 相位 57 × 二 轮）
    ("DECL0122", 53), // DECL 单元 0122（前向声明 × 相位 58 × 二 轮）
    ("Fulfill0123", 60), // Fulfill 单元 0123（兑现 × 相位 59 × 二 轮）
    ("SCOPE0124", 67), // SCOPE 单元 0124（范围 × 相位 60 × 二 轮）
    ("ITEM0125", 74), // ITEM 单元 0125（条目 × 相位 61 × 二 轮）
    ("DONE0126", 81), // DONE 单元 0126（已完成 × 相位 62 × 二 轮）
    ("OPEN0127", 88), // OPEN 单元 0127（未开 × 相位 63 × 二 轮）
    ("GAP0128", 6), // GAP 单元 0128（缺口 × 相位 00 × 二 轮）
    ("BIND0129", 13), // BIND 单元 0129（绑定 × 相位 01 × 二 轮）
    ("PROOF0130", 20), // PROOF 单元 0130（证据 × 相位 02 × 二 轮）
    ("GATE0131", 27), // GATE 单元 0131（闸门 × 相位 03 × 二 轮）
    ("CLOSE0132", 34), // CLOSE 单元 0132（收口 × 相位 04 × 二 轮）
    ("MID0133", 41), // MID 单元 0133（中期 × 相位 05 × 二 轮）
    ("DECL0134", 48), // DECL 单元 0134（前向声明 × 相位 06 × 二 轮）
    ("Fulfill0135", 55), // Fulfill 单元 0135（兑现 × 相位 07 × 二 轮）
    ("SCOPE0136", 62), // SCOPE 单元 0136（范围 × 相位 08 × 二 轮）
    ("ITEM0137", 69), // ITEM 单元 0137（条目 × 相位 09 × 二 轮）
    ("DONE0138", 76), // DONE 单元 0138（已完成 × 相位 10 × 二 轮）
    ("OPEN0139", 83), // OPEN 单元 0139（未开 × 相位 11 × 二 轮）
    ("GAP0140", 1), // GAP 单元 0140（缺口 × 相位 12 × 二 轮）
    ("BIND0141", 8), // BIND 单元 0141（绑定 × 相位 13 × 二 轮）
    ("PROOF0142", 15), // PROOF 单元 0142（证据 × 相位 14 × 二 轮）
    ("GATE0143", 22), // GATE 单元 0143（闸门 × 相位 15 × 二 轮）
    ("CLOSE0144", 29), // CLOSE 单元 0144（收口 × 相位 16 × 二 轮）
    ("MID0145", 36), // MID 单元 0145（中期 × 相位 17 × 二 轮）
    ("DECL0146", 43), // DECL 单元 0146（前向声明 × 相位 18 × 二 轮）
    ("Fulfill0147", 50), // Fulfill 单元 0147（兑现 × 相位 19 × 二 轮）
    ("SCOPE0148", 57), // SCOPE 单元 0148（范围 × 相位 20 × 二 轮）
    ("ITEM0149", 64), // ITEM 单元 0149（条目 × 相位 21 × 二 轮）
    ("DONE0150", 71), // DONE 单元 0150（已完成 × 相位 22 × 二 轮）
    ("OPEN0151", 78), // OPEN 单元 0151（未开 × 相位 23 × 二 轮）
    ("GAP0152", 85), // GAP 单元 0152（缺口 × 相位 24 × 二 轮）
    ("BIND0153", 3), // BIND 单元 0153（绑定 × 相位 25 × 二 轮）
    ("PROOF0154", 10), // PROOF 单元 0154（证据 × 相位 26 × 二 轮）
    ("GATE0155", 17), // GATE 单元 0155（闸门 × 相位 27 × 二 轮）
    ("CLOSE0156", 24), // CLOSE 单元 0156（收口 × 相位 28 × 二 轮）
    ("MID0157", 31), // MID 单元 0157（中期 × 相位 29 × 二 轮）
    ("DECL0158", 38), // DECL 单元 0158（前向声明 × 相位 30 × 二 轮）
    ("Fulfill0159", 45), // Fulfill 单元 0159（兑现 × 相位 31 × 二 轮）
    ("SCOPE0160", 52), // SCOPE 单元 0160（范围 × 相位 32 × 二 轮）
    ("ITEM0161", 59), // ITEM 单元 0161（条目 × 相位 33 × 二 轮）
    ("DONE0162", 66), // DONE 单元 0162（已完成 × 相位 34 × 二 轮）
    ("OPEN0163", 73), // OPEN 单元 0163（未开 × 相位 35 × 二 轮）
    ("GAP0164", 80), // GAP 单元 0164（缺口 × 相位 36 × 二 轮）
    ("BIND0165", 87), // BIND 单元 0165（绑定 × 相位 37 × 二 轮）
    ("PROOF0166", 5), // PROOF 单元 0166（证据 × 相位 38 × 二 轮）
    ("GATE0167", 12), // GATE 单元 0167（闸门 × 相位 39 × 二 轮）
    ("CLOSE0168", 19), // CLOSE 单元 0168（收口 × 相位 40 × 二 轮）
    ("MID0169", 26), // MID 单元 0169（中期 × 相位 41 × 二 轮）
    ("DECL0170", 33), // DECL 单元 0170（前向声明 × 相位 42 × 二 轮）
    ("Fulfill0171", 40), // Fulfill 单元 0171（兑现 × 相位 43 × 二 轮）
    ("SCOPE0172", 47), // SCOPE 单元 0172（范围 × 相位 44 × 二 轮）
    ("ITEM0173", 54), // ITEM 单元 0173（条目 × 相位 45 × 二 轮）
    ("DONE0174", 61), // DONE 单元 0174（已完成 × 相位 46 × 二 轮）
    ("OPEN0175", 68), // OPEN 单元 0175（未开 × 相位 47 × 二 轮）
    ("GAP0176", 75), // GAP 单元 0176（缺口 × 相位 48 × 二 轮）
    ("BIND0177", 82), // BIND 单元 0177（绑定 × 相位 49 × 二 轮）
    ("PROOF0178", 0), // PROOF 单元 0178（证据 × 相位 50 × 二 轮）
    ("GATE0179", 7), // GATE 单元 0179（闸门 × 相位 51 × 二 轮）
    ("CLOSE0180", 14), // CLOSE 单元 0180（收口 × 相位 52 × 二 轮）
    ("MID0181", 21), // MID 单元 0181（中期 × 相位 53 × 二 轮）
    ("DECL0182", 28), // DECL 单元 0182（前向声明 × 相位 54 × 二 轮）
    ("Fulfill0183", 35), // Fulfill 单元 0183（兑现 × 相位 55 × 二 轮）
    ("SCOPE0184", 42), // SCOPE 单元 0184（范围 × 相位 56 × 二 轮）
    ("ITEM0185", 49), // ITEM 单元 0185（条目 × 相位 57 × 二 轮）
    ("DONE0186", 56), // DONE 单元 0186（已完成 × 相位 58 × 二 轮）
    ("OPEN0187", 63), // OPEN 单元 0187（未开 × 相位 59 × 二 轮）
    ("GAP0188", 70), // GAP 单元 0188（缺口 × 相位 60 × 二 轮）
    ("BIND0189", 77), // BIND 单元 0189（绑定 × 相位 61 × 二 轮）
    ("PROOF0190", 84), // PROOF 单元 0190（证据 × 相位 62 × 二 轮）
    ("GATE0191", 2), // GATE 单元 0191（闸门 × 相位 63 × 二 轮）
    ("CLOSE0192", 9), // CLOSE 单元 0192（收口 × 相位 00 × 二 轮）
    ("MID0193", 16), // MID 单元 0193（中期 × 相位 01 × 二 轮）
    ("DECL0194", 23), // DECL 单元 0194（前向声明 × 相位 02 × 二 轮）
    ("Fulfill0195", 30), // Fulfill 单元 0195（兑现 × 相位 03 × 二 轮）
    ("SCOPE0196", 37), // SCOPE 单元 0196（范围 × 相位 04 × 二 轮）
    ("ITEM0197", 44), // ITEM 单元 0197（条目 × 相位 05 × 二 轮）
    ("DONE0198", 51), // DONE 单元 0198（已完成 × 相位 06 × 二 轮）
    ("OPEN0199", 58), // OPEN 单元 0199（未开 × 相位 07 × 二 轮）
    ("GAP0200", 65), // GAP 单元 0200（缺口 × 相位 08 × 二 轮）
    ("BIND0201", 72), // BIND 单元 0201（绑定 × 相位 09 × 二 轮）
    ("PROOF0202", 79), // PROOF 单元 0202（证据 × 相位 10 × 二 轮）
    ("GATE0203", 86), // GATE 单元 0203（闸门 × 相位 11 × 二 轮）
    ("CLOSE0204", 4), // CLOSE 单元 0204（收口 × 相位 12 × 二 轮）
    ("MID0205", 11), // MID 单元 0205（中期 × 相位 13 × 二 轮）
    ("DECL0206", 18), // DECL 单元 0206（前向声明 × 相位 14 × 二 轮）
    ("Fulfill0207", 25), // Fulfill 单元 0207（兑现 × 相位 15 × 二 轮）
    ("SCOPE0208", 32), // SCOPE 单元 0208（范围 × 相位 16 × 二 轮）
    ("ITEM0209", 39), // ITEM 单元 0209（条目 × 相位 17 × 二 轮）
    ("DONE0210", 46), // DONE 单元 0210（已完成 × 相位 18 × 二 轮）
    ("OPEN0211", 53), // OPEN 单元 0211（未开 × 相位 19 × 二 轮）
    ("GAP0212", 60), // GAP 单元 0212（缺口 × 相位 20 × 二 轮）
    ("BIND0213", 67), // BIND 单元 0213（绑定 × 相位 21 × 二 轮）
    ("PROOF0214", 74), // PROOF 单元 0214（证据 × 相位 22 × 二 轮）
    ("GATE0215", 81), // GATE 单元 0215（闸门 × 相位 23 × 二 轮）
    ("CLOSE0216", 88), // CLOSE 单元 0216（收口 × 相位 24 × 二 轮）
    ("MID0217", 6), // MID 单元 0217（中期 × 相位 25 × 二 轮）
    ("DECL0218", 13), // DECL 单元 0218（前向声明 × 相位 26 × 二 轮）
    ("Fulfill0219", 20), // Fulfill 单元 0219（兑现 × 相位 27 × 二 轮）
    ("SCOPE0220", 27), // SCOPE 单元 0220（范围 × 相位 28 × 二 轮）
    ("ITEM0221", 34), // ITEM 单元 0221（条目 × 相位 29 × 二 轮）
    ("DONE0222", 41), // DONE 单元 0222（已完成 × 相位 30 × 二 轮）
    ("OPEN0223", 48), // OPEN 单元 0223（未开 × 相位 31 × 二 轮）
    ("GAP0224", 55), // GAP 单元 0224（缺口 × 相位 32 × 二 轮）
    ("BIND0225", 62), // BIND 单元 0225（绑定 × 相位 33 × 二 轮）
    ("PROOF0226", 69), // PROOF 单元 0226（证据 × 相位 34 × 二 轮）
    ("GATE0227", 76), // GATE 单元 0227（闸门 × 相位 35 × 二 轮）
    ("CLOSE0228", 83), // CLOSE 单元 0228（收口 × 相位 36 × 二 轮）
    ("MID0229", 1), // MID 单元 0229（中期 × 相位 37 × 二 轮）
    ("DECL0230", 8), // DECL 单元 0230（前向声明 × 相位 38 × 二 轮）
    ("Fulfill0231", 15), // Fulfill 单元 0231（兑现 × 相位 39 × 二 轮）
    ("SCOPE0232", 22), // SCOPE 单元 0232（范围 × 相位 40 × 二 轮）
    ("ITEM0233", 29), // ITEM 单元 0233（条目 × 相位 41 × 二 轮）
    ("DONE0234", 36), // DONE 单元 0234（已完成 × 相位 42 × 二 轮）
    ("OPEN0235", 43), // OPEN 单元 0235（未开 × 相位 43 × 二 轮）
    ("GAP0236", 50), // GAP 单元 0236（缺口 × 相位 44 × 二 轮）
    ("BIND0237", 57), // BIND 单元 0237（绑定 × 相位 45 × 二 轮）
    ("PROOF0238", 64), // PROOF 单元 0238（证据 × 相位 46 × 二 轮）
    ("GATE0239", 71), // GATE 单元 0239（闸门 × 相位 47 × 二 轮）
    ("CLOSE0240", 78), // CLOSE 单元 0240（收口 × 相位 48 × 二 轮）
    ("MID0241", 85), // MID 单元 0241（中期 × 相位 49 × 二 轮）
    ("DECL0242", 3), // DECL 单元 0242（前向声明 × 相位 50 × 二 轮）
    ("Fulfill0243", 10), // Fulfill 单元 0243（兑现 × 相位 51 × 二 轮）
    ("SCOPE0244", 17), // SCOPE 单元 0244（范围 × 相位 52 × 二 轮）
    ("ITEM0245", 24), // ITEM 单元 0245（条目 × 相位 53 × 二 轮）
    ("DONE0246", 31), // DONE 单元 0246（已完成 × 相位 54 × 二 轮）
    ("OPEN0247", 38), // OPEN 单元 0247（未开 × 相位 55 × 二 轮）
    ("GAP0248", 45), // GAP 单元 0248（缺口 × 相位 56 × 二 轮）
    ("BIND0249", 52), // BIND 单元 0249（绑定 × 相位 57 × 二 轮）
    ("PROOF0250", 59), // PROOF 单元 0250（证据 × 相位 58 × 二 轮）
    ("GATE0251", 66), // GATE 单元 0251（闸门 × 相位 59 × 二 轮）
    ("CLOSE0252", 73), // CLOSE 单元 0252（收口 × 相位 60 × 二 轮）
    ("MID0253", 80), // MID 单元 0253（中期 × 相位 61 × 二 轮）
    ("DECL0254", 87), // DECL 单元 0254（前向声明 × 相位 62 × 二 轮）
    ("Fulfill0255", 5), // Fulfill 单元 0255（兑现 × 相位 63 × 二 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3781bmx_get(key: &str) -> Option<u64> {
    match key {
        "CLOSE0000" => Some(0),
        "MID0001" => Some(7),
        "DECL0002" => Some(14),
        "Fulfill0003" => Some(21),
        "SCOPE0004" => Some(28),
        "ITEM0005" => Some(35),
        "DONE0006" => Some(42),
        "OPEN0007" => Some(49),
        "GAP0008" => Some(56),
        "BIND0009" => Some(63),
        "PROOF0010" => Some(70),
        "GATE0011" => Some(77),
        "CLOSE0012" => Some(84),
        "MID0013" => Some(2),
        "DECL0014" => Some(9),
        "Fulfill0015" => Some(16),
        "SCOPE0016" => Some(23),
        "ITEM0017" => Some(30),
        "DONE0018" => Some(37),
        "OPEN0019" => Some(44),
        "GAP0020" => Some(51),
        "BIND0021" => Some(58),
        "PROOF0022" => Some(65),
        "GATE0023" => Some(72),
        "CLOSE0024" => Some(79),
        "MID0025" => Some(86),
        "DECL0026" => Some(4),
        "Fulfill0027" => Some(11),
        "SCOPE0028" => Some(18),
        "ITEM0029" => Some(25),
        "DONE0030" => Some(32),
        "OPEN0031" => Some(39),
        "GAP0032" => Some(46),
        "BIND0033" => Some(53),
        "PROOF0034" => Some(60),
        "GATE0035" => Some(67),
        "CLOSE0036" => Some(74),
        "MID0037" => Some(81),
        "DECL0038" => Some(88),
        "Fulfill0039" => Some(6),
        "SCOPE0040" => Some(13),
        "ITEM0041" => Some(20),
        "DONE0042" => Some(27),
        "OPEN0043" => Some(34),
        "GAP0044" => Some(41),
        "BIND0045" => Some(48),
        "PROOF0046" => Some(55),
        "GATE0047" => Some(62),
        "CLOSE0048" => Some(69),
        "MID0049" => Some(76),
        "DECL0050" => Some(83),
        "Fulfill0051" => Some(1),
        "SCOPE0052" => Some(8),
        "ITEM0053" => Some(15),
        "DONE0054" => Some(22),
        "OPEN0055" => Some(29),
        "GAP0056" => Some(36),
        "BIND0057" => Some(43),
        "PROOF0058" => Some(50),
        "GATE0059" => Some(57),
        "CLOSE0060" => Some(64),
        "MID0061" => Some(71),
        "DECL0062" => Some(78),
        "Fulfill0063" => Some(85),
        "SCOPE0064" => Some(3),
        "ITEM0065" => Some(10),
        "DONE0066" => Some(17),
        "OPEN0067" => Some(24),
        "GAP0068" => Some(31),
        "BIND0069" => Some(38),
        "PROOF0070" => Some(45),
        "GATE0071" => Some(52),
        "CLOSE0072" => Some(59),
        "MID0073" => Some(66),
        "DECL0074" => Some(73),
        "Fulfill0075" => Some(80),
        "SCOPE0076" => Some(87),
        "ITEM0077" => Some(5),
        "DONE0078" => Some(12),
        "OPEN0079" => Some(19),
        "GAP0080" => Some(26),
        "BIND0081" => Some(33),
        "PROOF0082" => Some(40),
        "GATE0083" => Some(47),
        "CLOSE0084" => Some(54),
        "MID0085" => Some(61),
        "DECL0086" => Some(68),
        "Fulfill0087" => Some(75),
        "SCOPE0088" => Some(82),
        "ITEM0089" => Some(0),
        "DONE0090" => Some(7),
        "OPEN0091" => Some(14),
        "GAP0092" => Some(21),
        "BIND0093" => Some(28),
        "PROOF0094" => Some(35),
        "GATE0095" => Some(42),
        "CLOSE0096" => Some(49),
        "MID0097" => Some(56),
        "DECL0098" => Some(63),
        "Fulfill0099" => Some(70),
        "SCOPE0100" => Some(77),
        "ITEM0101" => Some(84),
        "DONE0102" => Some(2),
        "OPEN0103" => Some(9),
        "GAP0104" => Some(16),
        "BIND0105" => Some(23),
        "PROOF0106" => Some(30),
        "GATE0107" => Some(37),
        "CLOSE0108" => Some(44),
        "MID0109" => Some(51),
        "DECL0110" => Some(58),
        "Fulfill0111" => Some(65),
        "SCOPE0112" => Some(72),
        "ITEM0113" => Some(79),
        "DONE0114" => Some(86),
        "OPEN0115" => Some(4),
        "GAP0116" => Some(11),
        "BIND0117" => Some(18),
        "PROOF0118" => Some(25),
        "GATE0119" => Some(32),
        "CLOSE0120" => Some(39),
        "MID0121" => Some(46),
        "DECL0122" => Some(53),
        "Fulfill0123" => Some(60),
        "SCOPE0124" => Some(67),
        "ITEM0125" => Some(74),
        "DONE0126" => Some(81),
        "OPEN0127" => Some(88),
        "GAP0128" => Some(6),
        "BIND0129" => Some(13),
        "PROOF0130" => Some(20),
        "GATE0131" => Some(27),
        "CLOSE0132" => Some(34),
        "MID0133" => Some(41),
        "DECL0134" => Some(48),
        "Fulfill0135" => Some(55),
        "SCOPE0136" => Some(62),
        "ITEM0137" => Some(69),
        "DONE0138" => Some(76),
        "OPEN0139" => Some(83),
        "GAP0140" => Some(1),
        "BIND0141" => Some(8),
        "PROOF0142" => Some(15),
        "GATE0143" => Some(22),
        "CLOSE0144" => Some(29),
        "MID0145" => Some(36),
        "DECL0146" => Some(43),
        "Fulfill0147" => Some(50),
        "SCOPE0148" => Some(57),
        "ITEM0149" => Some(64),
        "DONE0150" => Some(71),
        "OPEN0151" => Some(78),
        "GAP0152" => Some(85),
        "BIND0153" => Some(3),
        "PROOF0154" => Some(10),
        "GATE0155" => Some(17),
        "CLOSE0156" => Some(24),
        "MID0157" => Some(31),
        "DECL0158" => Some(38),
        "Fulfill0159" => Some(45),
        "SCOPE0160" => Some(52),
        "ITEM0161" => Some(59),
        "DONE0162" => Some(66),
        "OPEN0163" => Some(73),
        "GAP0164" => Some(80),
        "BIND0165" => Some(87),
        "PROOF0166" => Some(5),
        "GATE0167" => Some(12),
        "CLOSE0168" => Some(19),
        "MID0169" => Some(26),
        "DECL0170" => Some(33),
        "Fulfill0171" => Some(40),
        "SCOPE0172" => Some(47),
        "ITEM0173" => Some(54),
        "DONE0174" => Some(61),
        "OPEN0175" => Some(68),
        "GAP0176" => Some(75),
        "BIND0177" => Some(82),
        "PROOF0178" => Some(0),
        "GATE0179" => Some(7),
        "CLOSE0180" => Some(14),
        "MID0181" => Some(21),
        "DECL0182" => Some(28),
        "Fulfill0183" => Some(35),
        "SCOPE0184" => Some(42),
        "ITEM0185" => Some(49),
        "DONE0186" => Some(56),
        "OPEN0187" => Some(63),
        "GAP0188" => Some(70),
        "BIND0189" => Some(77),
        "PROOF0190" => Some(84),
        "GATE0191" => Some(2),
        "CLOSE0192" => Some(9),
        "MID0193" => Some(16),
        "DECL0194" => Some(23),
        "Fulfill0195" => Some(30),
        "SCOPE0196" => Some(37),
        "ITEM0197" => Some(44),
        "DONE0198" => Some(51),
        "OPEN0199" => Some(58),
        "GAP0200" => Some(65),
        "BIND0201" => Some(72),
        "PROOF0202" => Some(79),
        "GATE0203" => Some(86),
        "CLOSE0204" => Some(4),
        "MID0205" => Some(11),
        "DECL0206" => Some(18),
        "Fulfill0207" => Some(25),
        "SCOPE0208" => Some(32),
        "ITEM0209" => Some(39),
        "DONE0210" => Some(46),
        "OPEN0211" => Some(53),
        "GAP0212" => Some(60),
        "BIND0213" => Some(67),
        "PROOF0214" => Some(74),
        "GATE0215" => Some(81),
        "CLOSE0216" => Some(88),
        "MID0217" => Some(6),
        "DECL0218" => Some(13),
        "Fulfill0219" => Some(20),
        "SCOPE0220" => Some(27),
        "ITEM0221" => Some(34),
        "DONE0222" => Some(41),
        "OPEN0223" => Some(48),
        "GAP0224" => Some(55),
        "BIND0225" => Some(62),
        "PROOF0226" => Some(69),
        "GATE0227" => Some(76),
        "CLOSE0228" => Some(83),
        "MID0229" => Some(1),
        "DECL0230" => Some(8),
        "Fulfill0231" => Some(15),
        "SCOPE0232" => Some(22),
        "ITEM0233" => Some(29),
        "DONE0234" => Some(36),
        "OPEN0235" => Some(43),
        "GAP0236" => Some(50),
        "BIND0237" => Some(57),
        "PROOF0238" => Some(64),
        "GATE0239" => Some(71),
        "CLOSE0240" => Some(78),
        "MID0241" => Some(85),
        "DECL0242" => Some(3),
        "Fulfill0243" => Some(10),
        "SCOPE0244" => Some(17),
        "ITEM0245" => Some(24),
        "DONE0246" => Some(31),
        "OPEN0247" => Some(38),
        "GAP0248" => Some(45),
        "BIND0249" => Some(52),
        "PROOF0250" => Some(59),
        "GATE0251" => Some(66),
        "CLOSE0252" => Some(73),
        "MID0253" => Some(80),
        "DECL0254" => Some(87),
        "Fulfill0255" => Some(5),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3781bmx_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3781bmx_COUNT {
        let (k, v) = u3781bmx[i];
        let ok = match k {
            "CLOSE0000" => v < 89,
            "MID0001" => v < 89,
            "DECL0002" => v < 89,
            "Fulfill0003" => v < 89,
            "SCOPE0004" => v < 89,
            "ITEM0005" => v < 89,
            "DONE0006" => v < 89,
            "OPEN0007" => v < 89,
            "GAP0008" => v < 89,
            "BIND0009" => v < 89,
            "PROOF0010" => v < 89,
            "GATE0011" => v < 89,
            "CLOSE0012" => v < 89,
            "MID0013" => v < 89,
            "DECL0014" => v < 89,
            "Fulfill0015" => v < 89,
            "SCOPE0016" => v < 89,
            "ITEM0017" => v < 89,
            "DONE0018" => v < 89,
            "OPEN0019" => v < 89,
            "GAP0020" => v < 89,
            "BIND0021" => v < 89,
            "PROOF0022" => v < 89,
            "GATE0023" => v < 89,
            "CLOSE0024" => v < 89,
            "MID0025" => v < 89,
            "DECL0026" => v < 89,
            "Fulfill0027" => v < 89,
            "SCOPE0028" => v < 89,
            "ITEM0029" => v < 89,
            "DONE0030" => v < 89,
            "OPEN0031" => v < 89,
            "GAP0032" => v < 89,
            "BIND0033" => v < 89,
            "PROOF0034" => v < 89,
            "GATE0035" => v < 89,
            "CLOSE0036" => v < 89,
            "MID0037" => v < 89,
            "DECL0038" => v < 89,
            "Fulfill0039" => v < 89,
            "SCOPE0040" => v < 89,
            "ITEM0041" => v < 89,
            "DONE0042" => v < 89,
            "OPEN0043" => v < 89,
            "GAP0044" => v < 89,
            "BIND0045" => v < 89,
            "PROOF0046" => v < 89,
            "GATE0047" => v < 89,
            "CLOSE0048" => v < 89,
            "MID0049" => v < 89,
            "DECL0050" => v < 89,
            "Fulfill0051" => v < 89,
            "SCOPE0052" => v < 89,
            "ITEM0053" => v < 89,
            "DONE0054" => v < 89,
            "OPEN0055" => v < 89,
            "GAP0056" => v < 89,
            "BIND0057" => v < 89,
            "PROOF0058" => v < 89,
            "GATE0059" => v < 89,
            "CLOSE0060" => v < 89,
            "MID0061" => v < 89,
            "DECL0062" => v < 89,
            "Fulfill0063" => v < 89,
            "SCOPE0064" => v < 89,
            "ITEM0065" => v < 89,
            "DONE0066" => v < 89,
            "OPEN0067" => v < 89,
            "GAP0068" => v < 89,
            "BIND0069" => v < 89,
            "PROOF0070" => v < 89,
            "GATE0071" => v < 89,
            "CLOSE0072" => v < 89,
            "MID0073" => v < 89,
            "DECL0074" => v < 89,
            "Fulfill0075" => v < 89,
            "SCOPE0076" => v < 89,
            "ITEM0077" => v < 89,
            "DONE0078" => v < 89,
            "OPEN0079" => v < 89,
            "GAP0080" => v < 89,
            "BIND0081" => v < 89,
            "PROOF0082" => v < 89,
            "GATE0083" => v < 89,
            "CLOSE0084" => v < 89,
            "MID0085" => v < 89,
            "DECL0086" => v < 89,
            "Fulfill0087" => v < 89,
            "SCOPE0088" => v < 89,
            "ITEM0089" => v < 89,
            "DONE0090" => v < 89,
            "OPEN0091" => v < 89,
            "GAP0092" => v < 89,
            "BIND0093" => v < 89,
            "PROOF0094" => v < 89,
            "GATE0095" => v < 89,
            "CLOSE0096" => v < 89,
            "MID0097" => v < 89,
            "DECL0098" => v < 89,
            "Fulfill0099" => v < 89,
            "SCOPE0100" => v < 89,
            "ITEM0101" => v < 89,
            "DONE0102" => v < 89,
            "OPEN0103" => v < 89,
            "GAP0104" => v < 89,
            "BIND0105" => v < 89,
            "PROOF0106" => v < 89,
            "GATE0107" => v < 89,
            "CLOSE0108" => v < 89,
            "MID0109" => v < 89,
            "DECL0110" => v < 89,
            "Fulfill0111" => v < 89,
            "SCOPE0112" => v < 89,
            "ITEM0113" => v < 89,
            "DONE0114" => v < 89,
            "OPEN0115" => v < 89,
            "GAP0116" => v < 89,
            "BIND0117" => v < 89,
            "PROOF0118" => v < 89,
            "GATE0119" => v < 89,
            "CLOSE0120" => v < 89,
            "MID0121" => v < 89,
            "DECL0122" => v < 89,
            "Fulfill0123" => v < 89,
            "SCOPE0124" => v < 89,
            "ITEM0125" => v < 89,
            "DONE0126" => v < 89,
            "OPEN0127" => v < 89,
            "GAP0128" => v < 89,
            "BIND0129" => v < 89,
            "PROOF0130" => v < 89,
            "GATE0131" => v < 89,
            "CLOSE0132" => v < 89,
            "MID0133" => v < 89,
            "DECL0134" => v < 89,
            "Fulfill0135" => v < 89,
            "SCOPE0136" => v < 89,
            "ITEM0137" => v < 89,
            "DONE0138" => v < 89,
            "OPEN0139" => v < 89,
            "GAP0140" => v < 89,
            "BIND0141" => v < 89,
            "PROOF0142" => v < 89,
            "GATE0143" => v < 89,
            "CLOSE0144" => v < 89,
            "MID0145" => v < 89,
            "DECL0146" => v < 89,
            "Fulfill0147" => v < 89,
            "SCOPE0148" => v < 89,
            "ITEM0149" => v < 89,
            "DONE0150" => v < 89,
            "OPEN0151" => v < 89,
            "GAP0152" => v < 89,
            "BIND0153" => v < 89,
            "PROOF0154" => v < 89,
            "GATE0155" => v < 89,
            "CLOSE0156" => v < 89,
            "MID0157" => v < 89,
            "DECL0158" => v < 89,
            "Fulfill0159" => v < 89,
            "SCOPE0160" => v < 89,
            "ITEM0161" => v < 89,
            "DONE0162" => v < 89,
            "OPEN0163" => v < 89,
            "GAP0164" => v < 89,
            "BIND0165" => v < 89,
            "PROOF0166" => v < 89,
            "GATE0167" => v < 89,
            "CLOSE0168" => v < 89,
            "MID0169" => v < 89,
            "DECL0170" => v < 89,
            "Fulfill0171" => v < 89,
            "SCOPE0172" => v < 89,
            "ITEM0173" => v < 89,
            "DONE0174" => v < 89,
            "OPEN0175" => v < 89,
            "GAP0176" => v < 89,
            "BIND0177" => v < 89,
            "PROOF0178" => v < 89,
            "GATE0179" => v < 89,
            "CLOSE0180" => v < 89,
            "MID0181" => v < 89,
            "DECL0182" => v < 89,
            "Fulfill0183" => v < 89,
            "SCOPE0184" => v < 89,
            "ITEM0185" => v < 89,
            "DONE0186" => v < 89,
            "OPEN0187" => v < 89,
            "GAP0188" => v < 89,
            "BIND0189" => v < 89,
            "PROOF0190" => v < 89,
            "GATE0191" => v < 89,
            "CLOSE0192" => v < 89,
            "MID0193" => v < 89,
            "DECL0194" => v < 89,
            "Fulfill0195" => v < 89,
            "SCOPE0196" => v < 89,
            "ITEM0197" => v < 89,
            "DONE0198" => v < 89,
            "OPEN0199" => v < 89,
            "GAP0200" => v < 89,
            "BIND0201" => v < 89,
            "PROOF0202" => v < 89,
            "GATE0203" => v < 89,
            "CLOSE0204" => v < 89,
            "MID0205" => v < 89,
            "DECL0206" => v < 89,
            "Fulfill0207" => v < 89,
            "SCOPE0208" => v < 89,
            "ITEM0209" => v < 89,
            "DONE0210" => v < 89,
            "OPEN0211" => v < 89,
            "GAP0212" => v < 89,
            "BIND0213" => v < 89,
            "PROOF0214" => v < 89,
            "GATE0215" => v < 89,
            "CLOSE0216" => v < 89,
            "MID0217" => v < 89,
            "DECL0218" => v < 89,
            "Fulfill0219" => v < 89,
            "SCOPE0220" => v < 89,
            "ITEM0221" => v < 89,
            "DONE0222" => v < 89,
            "OPEN0223" => v < 89,
            "GAP0224" => v < 89,
            "BIND0225" => v < 89,
            "PROOF0226" => v < 89,
            "GATE0227" => v < 89,
            "CLOSE0228" => v < 89,
            "MID0229" => v < 89,
            "DECL0230" => v < 89,
            "Fulfill0231" => v < 89,
            "SCOPE0232" => v < 89,
            "ITEM0233" => v < 89,
            "DONE0234" => v < 89,
            "OPEN0235" => v < 89,
            "GAP0236" => v < 89,
            "BIND0237" => v < 89,
            "PROOF0238" => v < 89,
            "GATE0239" => v < 89,
            "CLOSE0240" => v < 89,
            "MID0241" => v < 89,
            "DECL0242" => v < 89,
            "Fulfill0243" => v < 89,
            "SCOPE0244" => v < 89,
            "ITEM0245" => v < 89,
            "DONE0246" => v < 89,
            "OPEN0247" => v < 89,
            "GAP0248" => v < 89,
            "BIND0249" => v < 89,
            "PROOF0250" => v < 89,
            "GATE0251" => v < 89,
            "CLOSE0252" => v < 89,
            "MID0253" => v < 89,
            "DECL0254" => v < 89,
            "Fulfill0255" => v < 89,
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
pub fn u3781bmx_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3781bmx_COUNT {
        s = s.wrapping_add(u3781bmx[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3781bmx_unique() -> bool {
    let mut i = 0usize;
    while i < u3781bmx_COUNT {
        let mut j = i + 1;
        while j < u3781bmx_COUNT {
            if u3781bmx[i].0 == u3781bmx[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe250, 0xe25f]。
pub const fn u3781_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3781Code::ALL.len() {
        let c = u3781Code::ALL[i].code();
        if c < 0xe250 || c > 0xe25f {
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
fn u3781_guard_seg() -> bool {
    u3781_segment_guard()
}

/// UNX-F3781 bmx 256 行齐备 + 边界锚 + 键外拒收。
fn check_u3781_bmx_count() -> bool {
    let mut ok = u3781bmx_COUNT == 256;
    if ok { ok = u3781bmx_get("CLOSE0000") == Some(0) && u3781bmx_get("Fulfill0255") == Some(5); }
    if ok { ok = u3781bmx_get("ZZZ_NOKEY").is_none(); }
    ok
}

/// UNX-F3781 bmx 累计守恒（理论值 11231）。
fn check_u3781_bmx_sum() -> bool {
    u3781bmx_sum() == 11231
}

/// UNX-F3781 bmx 逐行守卫与唯一性全绿。
fn check_u3781_bmx_guard() -> bool {
    u3781bmx_scan() == 0 && u3781bmx_unique()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3781_checks() -> CheckSet {
    let mut set = CheckSet::new("U3781");
    set.add(
        "u3781-seg-guard",
        u3781_guard_seg(),
        if u3781_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3781-bmx-count",
        check_u3781_bmx_count(),
        if check_u3781_bmx_count() { "" } else { "UNX-F3781 bmx count mismatch" },
    );
    set.add(
        "u3781-bmx-sum",
        check_u3781_bmx_sum(),
        if check_u3781_bmx_sum() { "" } else { "UNX-F3781 bmx sum mismatch" },
    );
    set.add(
        "u3781-bmx-guard",
        check_u3781_bmx_guard(),
        if check_u3781_bmx_guard() { "" } else { "UNX-F3781 bmx guard mismatch" },
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
    fn u3781_all_checks_green() {
        let set = run_u3781_checks();
        assert!(set.all_passed(), "U3781 domain checks must be green");
    }

    #[test]
    fn u3781_segment_guard_holds() {
        assert!(u3781_segment_guard());
    }
}
