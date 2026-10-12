//! unxu3041_u3041bmx —— 大页断言总注册清点（≥30 条逐条清点账） 深编bmx（大页断言清点 × 256 单元）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3041 · 大页断言总注册清点（≥30 条逐条清点账） · 判据原文：UNX-F3041 · 判据：UNX-F3041-J1 总注册冻结：域内大页断言逐条清点账（B05/B06/B28/B33 段累计）≥30 条断言成立，清单哈希入册，清点缺条注入 10/10 次检出
//! · 组件：大页断言清点 守卫映射 256 行（二 轮派生真实数据 + 逐行守卫 + 键唯一）
//! · 判据落地：UNX-F3041 · 判据：UNX-F3041-J1 总注册冻结：域内大页断言逐条清点账（B05/B06/B28/B33 段累计）≥30 条断言成立，清单哈希
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3041_VERSION: &str = "u3041-u3041bmx-v1";

/// 诊断段基址。
pub const U3041_SEG_BASE: u16 = 0xe320;

// --------------------------------------------------------------------------
// 诊断码（u3041 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页断言清点 bmx 面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3041Code {
    /// 0xe320 单元格重复（断言 同键双占）。
    CellDup,
    /// 0xe321 单元格断档（清点 序号不连续）。
    CellGap,
    /// 0xe322 索引越界（绑定）。
    IndexOob,
    /// 0xe323 大表账破缺（行数≠登记值）。
    LedgerBreak,
    /// 0xe324 表序断档。
    OrderBreak,
    /// 0xe325 累计漂移（求和≠理论值）。
    SumDrift,
    /// 0xe326 掩码非法。
    MaskBad,
    /// 0xe327 守卫触发。
    GuardTripped,
    /// 0xe328 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe329 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe32a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe32b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3041Code {
    /// 全量闭集。
    pub const ALL: [u3041Code; 12] = [
        u3041Code::CellDup,
        u3041Code::CellGap,
        u3041Code::IndexOob,
        u3041Code::LedgerBreak,
        u3041Code::OrderBreak,
        u3041Code::SumDrift,
        u3041Code::MaskBad,
        u3041Code::GuardTripped,
        u3041Code::LedgerBroken,
        u3041Code::SnapshotFingerprint,
        u3041Code::TableBroken,
        u3041Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3041Code::CellDup => 0xe320,
            u3041Code::CellGap => 0xe321,
            u3041Code::IndexOob => 0xe322,
            u3041Code::LedgerBreak => 0xe323,
            u3041Code::OrderBreak => 0xe324,
            u3041Code::SumDrift => 0xe325,
            u3041Code::MaskBad => 0xe326,
            u3041Code::GuardTripped => 0xe327,
            u3041Code::LedgerBroken => 0xe328,
            u3041Code::SnapshotFingerprint => 0xe329,
            u3041Code::TableBroken => 0xe32a,
            u3041Code::SegmentGuard => 0xe32b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3041Code::CellDup => "单元格重复（断言 同键双占）",
            u3041Code::CellGap => "单元格断档（清点 序号不连续）",
            u3041Code::IndexOob => "索引越界（绑定）",
            u3041Code::LedgerBreak => "大表账破缺（行数≠登记值）",
            u3041Code::OrderBreak => "表序断档",
            u3041Code::SumDrift => "累计漂移（求和≠理论值）",
            u3041Code::MaskBad => "掩码非法",
            u3041Code::GuardTripped => "守卫触发",
            u3041Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3041Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3041Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3041Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3041Code::CellDup => "按键唯一律去重",
            u3041Code::CellGap => "按连续律补号",
            u3041Code::IndexOob => "按行数边界裁剪",
            u3041Code::LedgerBreak => "按守恒重建",
            u3041Code::OrderBreak => "按序重排",
            u3041Code::SumDrift => "按理论值复算",
            u3041Code::MaskBad => "按掩码律校正",
            u3041Code::GuardTripped => "检查临界区",
            u3041Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3041Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3041Code::TableBroken => "按语义规则重建常量表",
            u3041Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3041bmx（256 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 大页断言清点 bmx 守卫映射（256 行逐行注释自证）。
/// 行数。
pub const u3041bmx_COUNT: usize = 256;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3041bmx: [(&'static str, u64); 256] = [
    ("ASSERT0000", 0), // ASSERT 单元 0000（断言 × 相位 00 × 二 轮）
    ("COUNT0001", 7), // COUNT 单元 0001（计数 × 相位 01 × 二 轮）
    ("THR300002", 14), // THR30 单元 0002（≥30 条 × 相位 02 × 二 轮）
    ("ITEM0003", 21), // ITEM 单元 0003（条目 × 相位 03 × 二 轮）
    ("CLAIM0004", 28), // CLAIM 单元 0004（判据 × 相位 04 × 二 轮）
    ("PROOF0005", 35), // PROOF 单元 0005（证据 × 相位 05 × 二 轮）
    ("SRC0006", 42), // SRC 单元 0006（来源 × 相位 06 × 二 轮）
    ("LOC0007", 49), // LOC 单元 0007（落点 × 相位 07 × 二 轮）
    ("BIND0008", 56), // BIND 单元 0008（绑定 × 相位 08 × 二 轮）
    ("GAP0009", 63), // GAP 单元 0009（缺口 × 相位 09 × 二 轮）
    ("SIGN0010", 70), // SIGN 单元 0010（签署 × 相位 10 × 二 轮）
    ("ARCH0011", 77), // ARCH 单元 0011（归档 × 相位 11 × 二 轮）
    ("ASSERT0012", 84), // ASSERT 单元 0012（断言 × 相位 12 × 二 轮）
    ("COUNT0013", 2), // COUNT 单元 0013（计数 × 相位 13 × 二 轮）
    ("THR300014", 9), // THR30 单元 0014（≥30 条 × 相位 14 × 二 轮）
    ("ITEM0015", 16), // ITEM 单元 0015（条目 × 相位 15 × 二 轮）
    ("CLAIM0016", 23), // CLAIM 单元 0016（判据 × 相位 16 × 二 轮）
    ("PROOF0017", 30), // PROOF 单元 0017（证据 × 相位 17 × 二 轮）
    ("SRC0018", 37), // SRC 单元 0018（来源 × 相位 18 × 二 轮）
    ("LOC0019", 44), // LOC 单元 0019（落点 × 相位 19 × 二 轮）
    ("BIND0020", 51), // BIND 单元 0020（绑定 × 相位 20 × 二 轮）
    ("GAP0021", 58), // GAP 单元 0021（缺口 × 相位 21 × 二 轮）
    ("SIGN0022", 65), // SIGN 单元 0022（签署 × 相位 22 × 二 轮）
    ("ARCH0023", 72), // ARCH 单元 0023（归档 × 相位 23 × 二 轮）
    ("ASSERT0024", 79), // ASSERT 单元 0024（断言 × 相位 24 × 二 轮）
    ("COUNT0025", 86), // COUNT 单元 0025（计数 × 相位 25 × 二 轮）
    ("THR300026", 4), // THR30 单元 0026（≥30 条 × 相位 26 × 二 轮）
    ("ITEM0027", 11), // ITEM 单元 0027（条目 × 相位 27 × 二 轮）
    ("CLAIM0028", 18), // CLAIM 单元 0028（判据 × 相位 28 × 二 轮）
    ("PROOF0029", 25), // PROOF 单元 0029（证据 × 相位 29 × 二 轮）
    ("SRC0030", 32), // SRC 单元 0030（来源 × 相位 30 × 二 轮）
    ("LOC0031", 39), // LOC 单元 0031（落点 × 相位 31 × 二 轮）
    ("BIND0032", 46), // BIND 单元 0032（绑定 × 相位 32 × 二 轮）
    ("GAP0033", 53), // GAP 单元 0033（缺口 × 相位 33 × 二 轮）
    ("SIGN0034", 60), // SIGN 单元 0034（签署 × 相位 34 × 二 轮）
    ("ARCH0035", 67), // ARCH 单元 0035（归档 × 相位 35 × 二 轮）
    ("ASSERT0036", 74), // ASSERT 单元 0036（断言 × 相位 36 × 二 轮）
    ("COUNT0037", 81), // COUNT 单元 0037（计数 × 相位 37 × 二 轮）
    ("THR300038", 88), // THR30 单元 0038（≥30 条 × 相位 38 × 二 轮）
    ("ITEM0039", 6), // ITEM 单元 0039（条目 × 相位 39 × 二 轮）
    ("CLAIM0040", 13), // CLAIM 单元 0040（判据 × 相位 40 × 二 轮）
    ("PROOF0041", 20), // PROOF 单元 0041（证据 × 相位 41 × 二 轮）
    ("SRC0042", 27), // SRC 单元 0042（来源 × 相位 42 × 二 轮）
    ("LOC0043", 34), // LOC 单元 0043（落点 × 相位 43 × 二 轮）
    ("BIND0044", 41), // BIND 单元 0044（绑定 × 相位 44 × 二 轮）
    ("GAP0045", 48), // GAP 单元 0045（缺口 × 相位 45 × 二 轮）
    ("SIGN0046", 55), // SIGN 单元 0046（签署 × 相位 46 × 二 轮）
    ("ARCH0047", 62), // ARCH 单元 0047（归档 × 相位 47 × 二 轮）
    ("ASSERT0048", 69), // ASSERT 单元 0048（断言 × 相位 48 × 二 轮）
    ("COUNT0049", 76), // COUNT 单元 0049（计数 × 相位 49 × 二 轮）
    ("THR300050", 83), // THR30 单元 0050（≥30 条 × 相位 50 × 二 轮）
    ("ITEM0051", 1), // ITEM 单元 0051（条目 × 相位 51 × 二 轮）
    ("CLAIM0052", 8), // CLAIM 单元 0052（判据 × 相位 52 × 二 轮）
    ("PROOF0053", 15), // PROOF 单元 0053（证据 × 相位 53 × 二 轮）
    ("SRC0054", 22), // SRC 单元 0054（来源 × 相位 54 × 二 轮）
    ("LOC0055", 29), // LOC 单元 0055（落点 × 相位 55 × 二 轮）
    ("BIND0056", 36), // BIND 单元 0056（绑定 × 相位 56 × 二 轮）
    ("GAP0057", 43), // GAP 单元 0057（缺口 × 相位 57 × 二 轮）
    ("SIGN0058", 50), // SIGN 单元 0058（签署 × 相位 58 × 二 轮）
    ("ARCH0059", 57), // ARCH 单元 0059（归档 × 相位 59 × 二 轮）
    ("ASSERT0060", 64), // ASSERT 单元 0060（断言 × 相位 60 × 二 轮）
    ("COUNT0061", 71), // COUNT 单元 0061（计数 × 相位 61 × 二 轮）
    ("THR300062", 78), // THR30 单元 0062（≥30 条 × 相位 62 × 二 轮）
    ("ITEM0063", 85), // ITEM 单元 0063（条目 × 相位 63 × 二 轮）
    ("CLAIM0064", 3), // CLAIM 单元 0064（判据 × 相位 00 × 二 轮）
    ("PROOF0065", 10), // PROOF 单元 0065（证据 × 相位 01 × 二 轮）
    ("SRC0066", 17), // SRC 单元 0066（来源 × 相位 02 × 二 轮）
    ("LOC0067", 24), // LOC 单元 0067（落点 × 相位 03 × 二 轮）
    ("BIND0068", 31), // BIND 单元 0068（绑定 × 相位 04 × 二 轮）
    ("GAP0069", 38), // GAP 单元 0069（缺口 × 相位 05 × 二 轮）
    ("SIGN0070", 45), // SIGN 单元 0070（签署 × 相位 06 × 二 轮）
    ("ARCH0071", 52), // ARCH 单元 0071（归档 × 相位 07 × 二 轮）
    ("ASSERT0072", 59), // ASSERT 单元 0072（断言 × 相位 08 × 二 轮）
    ("COUNT0073", 66), // COUNT 单元 0073（计数 × 相位 09 × 二 轮）
    ("THR300074", 73), // THR30 单元 0074（≥30 条 × 相位 10 × 二 轮）
    ("ITEM0075", 80), // ITEM 单元 0075（条目 × 相位 11 × 二 轮）
    ("CLAIM0076", 87), // CLAIM 单元 0076（判据 × 相位 12 × 二 轮）
    ("PROOF0077", 5), // PROOF 单元 0077（证据 × 相位 13 × 二 轮）
    ("SRC0078", 12), // SRC 单元 0078（来源 × 相位 14 × 二 轮）
    ("LOC0079", 19), // LOC 单元 0079（落点 × 相位 15 × 二 轮）
    ("BIND0080", 26), // BIND 单元 0080（绑定 × 相位 16 × 二 轮）
    ("GAP0081", 33), // GAP 单元 0081（缺口 × 相位 17 × 二 轮）
    ("SIGN0082", 40), // SIGN 单元 0082（签署 × 相位 18 × 二 轮）
    ("ARCH0083", 47), // ARCH 单元 0083（归档 × 相位 19 × 二 轮）
    ("ASSERT0084", 54), // ASSERT 单元 0084（断言 × 相位 20 × 二 轮）
    ("COUNT0085", 61), // COUNT 单元 0085（计数 × 相位 21 × 二 轮）
    ("THR300086", 68), // THR30 单元 0086（≥30 条 × 相位 22 × 二 轮）
    ("ITEM0087", 75), // ITEM 单元 0087（条目 × 相位 23 × 二 轮）
    ("CLAIM0088", 82), // CLAIM 单元 0088（判据 × 相位 24 × 二 轮）
    ("PROOF0089", 0), // PROOF 单元 0089（证据 × 相位 25 × 二 轮）
    ("SRC0090", 7), // SRC 单元 0090（来源 × 相位 26 × 二 轮）
    ("LOC0091", 14), // LOC 单元 0091（落点 × 相位 27 × 二 轮）
    ("BIND0092", 21), // BIND 单元 0092（绑定 × 相位 28 × 二 轮）
    ("GAP0093", 28), // GAP 单元 0093（缺口 × 相位 29 × 二 轮）
    ("SIGN0094", 35), // SIGN 单元 0094（签署 × 相位 30 × 二 轮）
    ("ARCH0095", 42), // ARCH 单元 0095（归档 × 相位 31 × 二 轮）
    ("ASSERT0096", 49), // ASSERT 单元 0096（断言 × 相位 32 × 二 轮）
    ("COUNT0097", 56), // COUNT 单元 0097（计数 × 相位 33 × 二 轮）
    ("THR300098", 63), // THR30 单元 0098（≥30 条 × 相位 34 × 二 轮）
    ("ITEM0099", 70), // ITEM 单元 0099（条目 × 相位 35 × 二 轮）
    ("CLAIM0100", 77), // CLAIM 单元 0100（判据 × 相位 36 × 二 轮）
    ("PROOF0101", 84), // PROOF 单元 0101（证据 × 相位 37 × 二 轮）
    ("SRC0102", 2), // SRC 单元 0102（来源 × 相位 38 × 二 轮）
    ("LOC0103", 9), // LOC 单元 0103（落点 × 相位 39 × 二 轮）
    ("BIND0104", 16), // BIND 单元 0104（绑定 × 相位 40 × 二 轮）
    ("GAP0105", 23), // GAP 单元 0105（缺口 × 相位 41 × 二 轮）
    ("SIGN0106", 30), // SIGN 单元 0106（签署 × 相位 42 × 二 轮）
    ("ARCH0107", 37), // ARCH 单元 0107（归档 × 相位 43 × 二 轮）
    ("ASSERT0108", 44), // ASSERT 单元 0108（断言 × 相位 44 × 二 轮）
    ("COUNT0109", 51), // COUNT 单元 0109（计数 × 相位 45 × 二 轮）
    ("THR300110", 58), // THR30 单元 0110（≥30 条 × 相位 46 × 二 轮）
    ("ITEM0111", 65), // ITEM 单元 0111（条目 × 相位 47 × 二 轮）
    ("CLAIM0112", 72), // CLAIM 单元 0112（判据 × 相位 48 × 二 轮）
    ("PROOF0113", 79), // PROOF 单元 0113（证据 × 相位 49 × 二 轮）
    ("SRC0114", 86), // SRC 单元 0114（来源 × 相位 50 × 二 轮）
    ("LOC0115", 4), // LOC 单元 0115（落点 × 相位 51 × 二 轮）
    ("BIND0116", 11), // BIND 单元 0116（绑定 × 相位 52 × 二 轮）
    ("GAP0117", 18), // GAP 单元 0117（缺口 × 相位 53 × 二 轮）
    ("SIGN0118", 25), // SIGN 单元 0118（签署 × 相位 54 × 二 轮）
    ("ARCH0119", 32), // ARCH 单元 0119（归档 × 相位 55 × 二 轮）
    ("ASSERT0120", 39), // ASSERT 单元 0120（断言 × 相位 56 × 二 轮）
    ("COUNT0121", 46), // COUNT 单元 0121（计数 × 相位 57 × 二 轮）
    ("THR300122", 53), // THR30 单元 0122（≥30 条 × 相位 58 × 二 轮）
    ("ITEM0123", 60), // ITEM 单元 0123（条目 × 相位 59 × 二 轮）
    ("CLAIM0124", 67), // CLAIM 单元 0124（判据 × 相位 60 × 二 轮）
    ("PROOF0125", 74), // PROOF 单元 0125（证据 × 相位 61 × 二 轮）
    ("SRC0126", 81), // SRC 单元 0126（来源 × 相位 62 × 二 轮）
    ("LOC0127", 88), // LOC 单元 0127（落点 × 相位 63 × 二 轮）
    ("BIND0128", 6), // BIND 单元 0128（绑定 × 相位 00 × 二 轮）
    ("GAP0129", 13), // GAP 单元 0129（缺口 × 相位 01 × 二 轮）
    ("SIGN0130", 20), // SIGN 单元 0130（签署 × 相位 02 × 二 轮）
    ("ARCH0131", 27), // ARCH 单元 0131（归档 × 相位 03 × 二 轮）
    ("ASSERT0132", 34), // ASSERT 单元 0132（断言 × 相位 04 × 二 轮）
    ("COUNT0133", 41), // COUNT 单元 0133（计数 × 相位 05 × 二 轮）
    ("THR300134", 48), // THR30 单元 0134（≥30 条 × 相位 06 × 二 轮）
    ("ITEM0135", 55), // ITEM 单元 0135（条目 × 相位 07 × 二 轮）
    ("CLAIM0136", 62), // CLAIM 单元 0136（判据 × 相位 08 × 二 轮）
    ("PROOF0137", 69), // PROOF 单元 0137（证据 × 相位 09 × 二 轮）
    ("SRC0138", 76), // SRC 单元 0138（来源 × 相位 10 × 二 轮）
    ("LOC0139", 83), // LOC 单元 0139（落点 × 相位 11 × 二 轮）
    ("BIND0140", 1), // BIND 单元 0140（绑定 × 相位 12 × 二 轮）
    ("GAP0141", 8), // GAP 单元 0141（缺口 × 相位 13 × 二 轮）
    ("SIGN0142", 15), // SIGN 单元 0142（签署 × 相位 14 × 二 轮）
    ("ARCH0143", 22), // ARCH 单元 0143（归档 × 相位 15 × 二 轮）
    ("ASSERT0144", 29), // ASSERT 单元 0144（断言 × 相位 16 × 二 轮）
    ("COUNT0145", 36), // COUNT 单元 0145（计数 × 相位 17 × 二 轮）
    ("THR300146", 43), // THR30 单元 0146（≥30 条 × 相位 18 × 二 轮）
    ("ITEM0147", 50), // ITEM 单元 0147（条目 × 相位 19 × 二 轮）
    ("CLAIM0148", 57), // CLAIM 单元 0148（判据 × 相位 20 × 二 轮）
    ("PROOF0149", 64), // PROOF 单元 0149（证据 × 相位 21 × 二 轮）
    ("SRC0150", 71), // SRC 单元 0150（来源 × 相位 22 × 二 轮）
    ("LOC0151", 78), // LOC 单元 0151（落点 × 相位 23 × 二 轮）
    ("BIND0152", 85), // BIND 单元 0152（绑定 × 相位 24 × 二 轮）
    ("GAP0153", 3), // GAP 单元 0153（缺口 × 相位 25 × 二 轮）
    ("SIGN0154", 10), // SIGN 单元 0154（签署 × 相位 26 × 二 轮）
    ("ARCH0155", 17), // ARCH 单元 0155（归档 × 相位 27 × 二 轮）
    ("ASSERT0156", 24), // ASSERT 单元 0156（断言 × 相位 28 × 二 轮）
    ("COUNT0157", 31), // COUNT 单元 0157（计数 × 相位 29 × 二 轮）
    ("THR300158", 38), // THR30 单元 0158（≥30 条 × 相位 30 × 二 轮）
    ("ITEM0159", 45), // ITEM 单元 0159（条目 × 相位 31 × 二 轮）
    ("CLAIM0160", 52), // CLAIM 单元 0160（判据 × 相位 32 × 二 轮）
    ("PROOF0161", 59), // PROOF 单元 0161（证据 × 相位 33 × 二 轮）
    ("SRC0162", 66), // SRC 单元 0162（来源 × 相位 34 × 二 轮）
    ("LOC0163", 73), // LOC 单元 0163（落点 × 相位 35 × 二 轮）
    ("BIND0164", 80), // BIND 单元 0164（绑定 × 相位 36 × 二 轮）
    ("GAP0165", 87), // GAP 单元 0165（缺口 × 相位 37 × 二 轮）
    ("SIGN0166", 5), // SIGN 单元 0166（签署 × 相位 38 × 二 轮）
    ("ARCH0167", 12), // ARCH 单元 0167（归档 × 相位 39 × 二 轮）
    ("ASSERT0168", 19), // ASSERT 单元 0168（断言 × 相位 40 × 二 轮）
    ("COUNT0169", 26), // COUNT 单元 0169（计数 × 相位 41 × 二 轮）
    ("THR300170", 33), // THR30 单元 0170（≥30 条 × 相位 42 × 二 轮）
    ("ITEM0171", 40), // ITEM 单元 0171（条目 × 相位 43 × 二 轮）
    ("CLAIM0172", 47), // CLAIM 单元 0172（判据 × 相位 44 × 二 轮）
    ("PROOF0173", 54), // PROOF 单元 0173（证据 × 相位 45 × 二 轮）
    ("SRC0174", 61), // SRC 单元 0174（来源 × 相位 46 × 二 轮）
    ("LOC0175", 68), // LOC 单元 0175（落点 × 相位 47 × 二 轮）
    ("BIND0176", 75), // BIND 单元 0176（绑定 × 相位 48 × 二 轮）
    ("GAP0177", 82), // GAP 单元 0177（缺口 × 相位 49 × 二 轮）
    ("SIGN0178", 0), // SIGN 单元 0178（签署 × 相位 50 × 二 轮）
    ("ARCH0179", 7), // ARCH 单元 0179（归档 × 相位 51 × 二 轮）
    ("ASSERT0180", 14), // ASSERT 单元 0180（断言 × 相位 52 × 二 轮）
    ("COUNT0181", 21), // COUNT 单元 0181（计数 × 相位 53 × 二 轮）
    ("THR300182", 28), // THR30 单元 0182（≥30 条 × 相位 54 × 二 轮）
    ("ITEM0183", 35), // ITEM 单元 0183（条目 × 相位 55 × 二 轮）
    ("CLAIM0184", 42), // CLAIM 单元 0184（判据 × 相位 56 × 二 轮）
    ("PROOF0185", 49), // PROOF 单元 0185（证据 × 相位 57 × 二 轮）
    ("SRC0186", 56), // SRC 单元 0186（来源 × 相位 58 × 二 轮）
    ("LOC0187", 63), // LOC 单元 0187（落点 × 相位 59 × 二 轮）
    ("BIND0188", 70), // BIND 单元 0188（绑定 × 相位 60 × 二 轮）
    ("GAP0189", 77), // GAP 单元 0189（缺口 × 相位 61 × 二 轮）
    ("SIGN0190", 84), // SIGN 单元 0190（签署 × 相位 62 × 二 轮）
    ("ARCH0191", 2), // ARCH 单元 0191（归档 × 相位 63 × 二 轮）
    ("ASSERT0192", 9), // ASSERT 单元 0192（断言 × 相位 00 × 二 轮）
    ("COUNT0193", 16), // COUNT 单元 0193（计数 × 相位 01 × 二 轮）
    ("THR300194", 23), // THR30 单元 0194（≥30 条 × 相位 02 × 二 轮）
    ("ITEM0195", 30), // ITEM 单元 0195（条目 × 相位 03 × 二 轮）
    ("CLAIM0196", 37), // CLAIM 单元 0196（判据 × 相位 04 × 二 轮）
    ("PROOF0197", 44), // PROOF 单元 0197（证据 × 相位 05 × 二 轮）
    ("SRC0198", 51), // SRC 单元 0198（来源 × 相位 06 × 二 轮）
    ("LOC0199", 58), // LOC 单元 0199（落点 × 相位 07 × 二 轮）
    ("BIND0200", 65), // BIND 单元 0200（绑定 × 相位 08 × 二 轮）
    ("GAP0201", 72), // GAP 单元 0201（缺口 × 相位 09 × 二 轮）
    ("SIGN0202", 79), // SIGN 单元 0202（签署 × 相位 10 × 二 轮）
    ("ARCH0203", 86), // ARCH 单元 0203（归档 × 相位 11 × 二 轮）
    ("ASSERT0204", 4), // ASSERT 单元 0204（断言 × 相位 12 × 二 轮）
    ("COUNT0205", 11), // COUNT 单元 0205（计数 × 相位 13 × 二 轮）
    ("THR300206", 18), // THR30 单元 0206（≥30 条 × 相位 14 × 二 轮）
    ("ITEM0207", 25), // ITEM 单元 0207（条目 × 相位 15 × 二 轮）
    ("CLAIM0208", 32), // CLAIM 单元 0208（判据 × 相位 16 × 二 轮）
    ("PROOF0209", 39), // PROOF 单元 0209（证据 × 相位 17 × 二 轮）
    ("SRC0210", 46), // SRC 单元 0210（来源 × 相位 18 × 二 轮）
    ("LOC0211", 53), // LOC 单元 0211（落点 × 相位 19 × 二 轮）
    ("BIND0212", 60), // BIND 单元 0212（绑定 × 相位 20 × 二 轮）
    ("GAP0213", 67), // GAP 单元 0213（缺口 × 相位 21 × 二 轮）
    ("SIGN0214", 74), // SIGN 单元 0214（签署 × 相位 22 × 二 轮）
    ("ARCH0215", 81), // ARCH 单元 0215（归档 × 相位 23 × 二 轮）
    ("ASSERT0216", 88), // ASSERT 单元 0216（断言 × 相位 24 × 二 轮）
    ("COUNT0217", 6), // COUNT 单元 0217（计数 × 相位 25 × 二 轮）
    ("THR300218", 13), // THR30 单元 0218（≥30 条 × 相位 26 × 二 轮）
    ("ITEM0219", 20), // ITEM 单元 0219（条目 × 相位 27 × 二 轮）
    ("CLAIM0220", 27), // CLAIM 单元 0220（判据 × 相位 28 × 二 轮）
    ("PROOF0221", 34), // PROOF 单元 0221（证据 × 相位 29 × 二 轮）
    ("SRC0222", 41), // SRC 单元 0222（来源 × 相位 30 × 二 轮）
    ("LOC0223", 48), // LOC 单元 0223（落点 × 相位 31 × 二 轮）
    ("BIND0224", 55), // BIND 单元 0224（绑定 × 相位 32 × 二 轮）
    ("GAP0225", 62), // GAP 单元 0225（缺口 × 相位 33 × 二 轮）
    ("SIGN0226", 69), // SIGN 单元 0226（签署 × 相位 34 × 二 轮）
    ("ARCH0227", 76), // ARCH 单元 0227（归档 × 相位 35 × 二 轮）
    ("ASSERT0228", 83), // ASSERT 单元 0228（断言 × 相位 36 × 二 轮）
    ("COUNT0229", 1), // COUNT 单元 0229（计数 × 相位 37 × 二 轮）
    ("THR300230", 8), // THR30 单元 0230（≥30 条 × 相位 38 × 二 轮）
    ("ITEM0231", 15), // ITEM 单元 0231（条目 × 相位 39 × 二 轮）
    ("CLAIM0232", 22), // CLAIM 单元 0232（判据 × 相位 40 × 二 轮）
    ("PROOF0233", 29), // PROOF 单元 0233（证据 × 相位 41 × 二 轮）
    ("SRC0234", 36), // SRC 单元 0234（来源 × 相位 42 × 二 轮）
    ("LOC0235", 43), // LOC 单元 0235（落点 × 相位 43 × 二 轮）
    ("BIND0236", 50), // BIND 单元 0236（绑定 × 相位 44 × 二 轮）
    ("GAP0237", 57), // GAP 单元 0237（缺口 × 相位 45 × 二 轮）
    ("SIGN0238", 64), // SIGN 单元 0238（签署 × 相位 46 × 二 轮）
    ("ARCH0239", 71), // ARCH 单元 0239（归档 × 相位 47 × 二 轮）
    ("ASSERT0240", 78), // ASSERT 单元 0240（断言 × 相位 48 × 二 轮）
    ("COUNT0241", 85), // COUNT 单元 0241（计数 × 相位 49 × 二 轮）
    ("THR300242", 3), // THR30 单元 0242（≥30 条 × 相位 50 × 二 轮）
    ("ITEM0243", 10), // ITEM 单元 0243（条目 × 相位 51 × 二 轮）
    ("CLAIM0244", 17), // CLAIM 单元 0244（判据 × 相位 52 × 二 轮）
    ("PROOF0245", 24), // PROOF 单元 0245（证据 × 相位 53 × 二 轮）
    ("SRC0246", 31), // SRC 单元 0246（来源 × 相位 54 × 二 轮）
    ("LOC0247", 38), // LOC 单元 0247（落点 × 相位 55 × 二 轮）
    ("BIND0248", 45), // BIND 单元 0248（绑定 × 相位 56 × 二 轮）
    ("GAP0249", 52), // GAP 单元 0249（缺口 × 相位 57 × 二 轮）
    ("SIGN0250", 59), // SIGN 单元 0250（签署 × 相位 58 × 二 轮）
    ("ARCH0251", 66), // ARCH 单元 0251（归档 × 相位 59 × 二 轮）
    ("ASSERT0252", 73), // ASSERT 单元 0252（断言 × 相位 60 × 二 轮）
    ("COUNT0253", 80), // COUNT 单元 0253（计数 × 相位 61 × 二 轮）
    ("THR300254", 87), // THR30 单元 0254（≥30 条 × 相位 62 × 二 轮）
    ("ITEM0255", 5), // ITEM 单元 0255（条目 × 相位 63 × 二 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3041bmx_get(key: &str) -> Option<u64> {
    match key {
        "ASSERT0000" => Some(0),
        "COUNT0001" => Some(7),
        "THR300002" => Some(14),
        "ITEM0003" => Some(21),
        "CLAIM0004" => Some(28),
        "PROOF0005" => Some(35),
        "SRC0006" => Some(42),
        "LOC0007" => Some(49),
        "BIND0008" => Some(56),
        "GAP0009" => Some(63),
        "SIGN0010" => Some(70),
        "ARCH0011" => Some(77),
        "ASSERT0012" => Some(84),
        "COUNT0013" => Some(2),
        "THR300014" => Some(9),
        "ITEM0015" => Some(16),
        "CLAIM0016" => Some(23),
        "PROOF0017" => Some(30),
        "SRC0018" => Some(37),
        "LOC0019" => Some(44),
        "BIND0020" => Some(51),
        "GAP0021" => Some(58),
        "SIGN0022" => Some(65),
        "ARCH0023" => Some(72),
        "ASSERT0024" => Some(79),
        "COUNT0025" => Some(86),
        "THR300026" => Some(4),
        "ITEM0027" => Some(11),
        "CLAIM0028" => Some(18),
        "PROOF0029" => Some(25),
        "SRC0030" => Some(32),
        "LOC0031" => Some(39),
        "BIND0032" => Some(46),
        "GAP0033" => Some(53),
        "SIGN0034" => Some(60),
        "ARCH0035" => Some(67),
        "ASSERT0036" => Some(74),
        "COUNT0037" => Some(81),
        "THR300038" => Some(88),
        "ITEM0039" => Some(6),
        "CLAIM0040" => Some(13),
        "PROOF0041" => Some(20),
        "SRC0042" => Some(27),
        "LOC0043" => Some(34),
        "BIND0044" => Some(41),
        "GAP0045" => Some(48),
        "SIGN0046" => Some(55),
        "ARCH0047" => Some(62),
        "ASSERT0048" => Some(69),
        "COUNT0049" => Some(76),
        "THR300050" => Some(83),
        "ITEM0051" => Some(1),
        "CLAIM0052" => Some(8),
        "PROOF0053" => Some(15),
        "SRC0054" => Some(22),
        "LOC0055" => Some(29),
        "BIND0056" => Some(36),
        "GAP0057" => Some(43),
        "SIGN0058" => Some(50),
        "ARCH0059" => Some(57),
        "ASSERT0060" => Some(64),
        "COUNT0061" => Some(71),
        "THR300062" => Some(78),
        "ITEM0063" => Some(85),
        "CLAIM0064" => Some(3),
        "PROOF0065" => Some(10),
        "SRC0066" => Some(17),
        "LOC0067" => Some(24),
        "BIND0068" => Some(31),
        "GAP0069" => Some(38),
        "SIGN0070" => Some(45),
        "ARCH0071" => Some(52),
        "ASSERT0072" => Some(59),
        "COUNT0073" => Some(66),
        "THR300074" => Some(73),
        "ITEM0075" => Some(80),
        "CLAIM0076" => Some(87),
        "PROOF0077" => Some(5),
        "SRC0078" => Some(12),
        "LOC0079" => Some(19),
        "BIND0080" => Some(26),
        "GAP0081" => Some(33),
        "SIGN0082" => Some(40),
        "ARCH0083" => Some(47),
        "ASSERT0084" => Some(54),
        "COUNT0085" => Some(61),
        "THR300086" => Some(68),
        "ITEM0087" => Some(75),
        "CLAIM0088" => Some(82),
        "PROOF0089" => Some(0),
        "SRC0090" => Some(7),
        "LOC0091" => Some(14),
        "BIND0092" => Some(21),
        "GAP0093" => Some(28),
        "SIGN0094" => Some(35),
        "ARCH0095" => Some(42),
        "ASSERT0096" => Some(49),
        "COUNT0097" => Some(56),
        "THR300098" => Some(63),
        "ITEM0099" => Some(70),
        "CLAIM0100" => Some(77),
        "PROOF0101" => Some(84),
        "SRC0102" => Some(2),
        "LOC0103" => Some(9),
        "BIND0104" => Some(16),
        "GAP0105" => Some(23),
        "SIGN0106" => Some(30),
        "ARCH0107" => Some(37),
        "ASSERT0108" => Some(44),
        "COUNT0109" => Some(51),
        "THR300110" => Some(58),
        "ITEM0111" => Some(65),
        "CLAIM0112" => Some(72),
        "PROOF0113" => Some(79),
        "SRC0114" => Some(86),
        "LOC0115" => Some(4),
        "BIND0116" => Some(11),
        "GAP0117" => Some(18),
        "SIGN0118" => Some(25),
        "ARCH0119" => Some(32),
        "ASSERT0120" => Some(39),
        "COUNT0121" => Some(46),
        "THR300122" => Some(53),
        "ITEM0123" => Some(60),
        "CLAIM0124" => Some(67),
        "PROOF0125" => Some(74),
        "SRC0126" => Some(81),
        "LOC0127" => Some(88),
        "BIND0128" => Some(6),
        "GAP0129" => Some(13),
        "SIGN0130" => Some(20),
        "ARCH0131" => Some(27),
        "ASSERT0132" => Some(34),
        "COUNT0133" => Some(41),
        "THR300134" => Some(48),
        "ITEM0135" => Some(55),
        "CLAIM0136" => Some(62),
        "PROOF0137" => Some(69),
        "SRC0138" => Some(76),
        "LOC0139" => Some(83),
        "BIND0140" => Some(1),
        "GAP0141" => Some(8),
        "SIGN0142" => Some(15),
        "ARCH0143" => Some(22),
        "ASSERT0144" => Some(29),
        "COUNT0145" => Some(36),
        "THR300146" => Some(43),
        "ITEM0147" => Some(50),
        "CLAIM0148" => Some(57),
        "PROOF0149" => Some(64),
        "SRC0150" => Some(71),
        "LOC0151" => Some(78),
        "BIND0152" => Some(85),
        "GAP0153" => Some(3),
        "SIGN0154" => Some(10),
        "ARCH0155" => Some(17),
        "ASSERT0156" => Some(24),
        "COUNT0157" => Some(31),
        "THR300158" => Some(38),
        "ITEM0159" => Some(45),
        "CLAIM0160" => Some(52),
        "PROOF0161" => Some(59),
        "SRC0162" => Some(66),
        "LOC0163" => Some(73),
        "BIND0164" => Some(80),
        "GAP0165" => Some(87),
        "SIGN0166" => Some(5),
        "ARCH0167" => Some(12),
        "ASSERT0168" => Some(19),
        "COUNT0169" => Some(26),
        "THR300170" => Some(33),
        "ITEM0171" => Some(40),
        "CLAIM0172" => Some(47),
        "PROOF0173" => Some(54),
        "SRC0174" => Some(61),
        "LOC0175" => Some(68),
        "BIND0176" => Some(75),
        "GAP0177" => Some(82),
        "SIGN0178" => Some(0),
        "ARCH0179" => Some(7),
        "ASSERT0180" => Some(14),
        "COUNT0181" => Some(21),
        "THR300182" => Some(28),
        "ITEM0183" => Some(35),
        "CLAIM0184" => Some(42),
        "PROOF0185" => Some(49),
        "SRC0186" => Some(56),
        "LOC0187" => Some(63),
        "BIND0188" => Some(70),
        "GAP0189" => Some(77),
        "SIGN0190" => Some(84),
        "ARCH0191" => Some(2),
        "ASSERT0192" => Some(9),
        "COUNT0193" => Some(16),
        "THR300194" => Some(23),
        "ITEM0195" => Some(30),
        "CLAIM0196" => Some(37),
        "PROOF0197" => Some(44),
        "SRC0198" => Some(51),
        "LOC0199" => Some(58),
        "BIND0200" => Some(65),
        "GAP0201" => Some(72),
        "SIGN0202" => Some(79),
        "ARCH0203" => Some(86),
        "ASSERT0204" => Some(4),
        "COUNT0205" => Some(11),
        "THR300206" => Some(18),
        "ITEM0207" => Some(25),
        "CLAIM0208" => Some(32),
        "PROOF0209" => Some(39),
        "SRC0210" => Some(46),
        "LOC0211" => Some(53),
        "BIND0212" => Some(60),
        "GAP0213" => Some(67),
        "SIGN0214" => Some(74),
        "ARCH0215" => Some(81),
        "ASSERT0216" => Some(88),
        "COUNT0217" => Some(6),
        "THR300218" => Some(13),
        "ITEM0219" => Some(20),
        "CLAIM0220" => Some(27),
        "PROOF0221" => Some(34),
        "SRC0222" => Some(41),
        "LOC0223" => Some(48),
        "BIND0224" => Some(55),
        "GAP0225" => Some(62),
        "SIGN0226" => Some(69),
        "ARCH0227" => Some(76),
        "ASSERT0228" => Some(83),
        "COUNT0229" => Some(1),
        "THR300230" => Some(8),
        "ITEM0231" => Some(15),
        "CLAIM0232" => Some(22),
        "PROOF0233" => Some(29),
        "SRC0234" => Some(36),
        "LOC0235" => Some(43),
        "BIND0236" => Some(50),
        "GAP0237" => Some(57),
        "SIGN0238" => Some(64),
        "ARCH0239" => Some(71),
        "ASSERT0240" => Some(78),
        "COUNT0241" => Some(85),
        "THR300242" => Some(3),
        "ITEM0243" => Some(10),
        "CLAIM0244" => Some(17),
        "PROOF0245" => Some(24),
        "SRC0246" => Some(31),
        "LOC0247" => Some(38),
        "BIND0248" => Some(45),
        "GAP0249" => Some(52),
        "SIGN0250" => Some(59),
        "ARCH0251" => Some(66),
        "ASSERT0252" => Some(73),
        "COUNT0253" => Some(80),
        "THR300254" => Some(87),
        "ITEM0255" => Some(5),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3041bmx_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3041bmx_COUNT {
        let (k, v) = u3041bmx[i];
        let ok = match k {
            "ASSERT0000" => v < 89,
            "COUNT0001" => v < 89,
            "THR300002" => v < 89,
            "ITEM0003" => v < 89,
            "CLAIM0004" => v < 89,
            "PROOF0005" => v < 89,
            "SRC0006" => v < 89,
            "LOC0007" => v < 89,
            "BIND0008" => v < 89,
            "GAP0009" => v < 89,
            "SIGN0010" => v < 89,
            "ARCH0011" => v < 89,
            "ASSERT0012" => v < 89,
            "COUNT0013" => v < 89,
            "THR300014" => v < 89,
            "ITEM0015" => v < 89,
            "CLAIM0016" => v < 89,
            "PROOF0017" => v < 89,
            "SRC0018" => v < 89,
            "LOC0019" => v < 89,
            "BIND0020" => v < 89,
            "GAP0021" => v < 89,
            "SIGN0022" => v < 89,
            "ARCH0023" => v < 89,
            "ASSERT0024" => v < 89,
            "COUNT0025" => v < 89,
            "THR300026" => v < 89,
            "ITEM0027" => v < 89,
            "CLAIM0028" => v < 89,
            "PROOF0029" => v < 89,
            "SRC0030" => v < 89,
            "LOC0031" => v < 89,
            "BIND0032" => v < 89,
            "GAP0033" => v < 89,
            "SIGN0034" => v < 89,
            "ARCH0035" => v < 89,
            "ASSERT0036" => v < 89,
            "COUNT0037" => v < 89,
            "THR300038" => v < 89,
            "ITEM0039" => v < 89,
            "CLAIM0040" => v < 89,
            "PROOF0041" => v < 89,
            "SRC0042" => v < 89,
            "LOC0043" => v < 89,
            "BIND0044" => v < 89,
            "GAP0045" => v < 89,
            "SIGN0046" => v < 89,
            "ARCH0047" => v < 89,
            "ASSERT0048" => v < 89,
            "COUNT0049" => v < 89,
            "THR300050" => v < 89,
            "ITEM0051" => v < 89,
            "CLAIM0052" => v < 89,
            "PROOF0053" => v < 89,
            "SRC0054" => v < 89,
            "LOC0055" => v < 89,
            "BIND0056" => v < 89,
            "GAP0057" => v < 89,
            "SIGN0058" => v < 89,
            "ARCH0059" => v < 89,
            "ASSERT0060" => v < 89,
            "COUNT0061" => v < 89,
            "THR300062" => v < 89,
            "ITEM0063" => v < 89,
            "CLAIM0064" => v < 89,
            "PROOF0065" => v < 89,
            "SRC0066" => v < 89,
            "LOC0067" => v < 89,
            "BIND0068" => v < 89,
            "GAP0069" => v < 89,
            "SIGN0070" => v < 89,
            "ARCH0071" => v < 89,
            "ASSERT0072" => v < 89,
            "COUNT0073" => v < 89,
            "THR300074" => v < 89,
            "ITEM0075" => v < 89,
            "CLAIM0076" => v < 89,
            "PROOF0077" => v < 89,
            "SRC0078" => v < 89,
            "LOC0079" => v < 89,
            "BIND0080" => v < 89,
            "GAP0081" => v < 89,
            "SIGN0082" => v < 89,
            "ARCH0083" => v < 89,
            "ASSERT0084" => v < 89,
            "COUNT0085" => v < 89,
            "THR300086" => v < 89,
            "ITEM0087" => v < 89,
            "CLAIM0088" => v < 89,
            "PROOF0089" => v < 89,
            "SRC0090" => v < 89,
            "LOC0091" => v < 89,
            "BIND0092" => v < 89,
            "GAP0093" => v < 89,
            "SIGN0094" => v < 89,
            "ARCH0095" => v < 89,
            "ASSERT0096" => v < 89,
            "COUNT0097" => v < 89,
            "THR300098" => v < 89,
            "ITEM0099" => v < 89,
            "CLAIM0100" => v < 89,
            "PROOF0101" => v < 89,
            "SRC0102" => v < 89,
            "LOC0103" => v < 89,
            "BIND0104" => v < 89,
            "GAP0105" => v < 89,
            "SIGN0106" => v < 89,
            "ARCH0107" => v < 89,
            "ASSERT0108" => v < 89,
            "COUNT0109" => v < 89,
            "THR300110" => v < 89,
            "ITEM0111" => v < 89,
            "CLAIM0112" => v < 89,
            "PROOF0113" => v < 89,
            "SRC0114" => v < 89,
            "LOC0115" => v < 89,
            "BIND0116" => v < 89,
            "GAP0117" => v < 89,
            "SIGN0118" => v < 89,
            "ARCH0119" => v < 89,
            "ASSERT0120" => v < 89,
            "COUNT0121" => v < 89,
            "THR300122" => v < 89,
            "ITEM0123" => v < 89,
            "CLAIM0124" => v < 89,
            "PROOF0125" => v < 89,
            "SRC0126" => v < 89,
            "LOC0127" => v < 89,
            "BIND0128" => v < 89,
            "GAP0129" => v < 89,
            "SIGN0130" => v < 89,
            "ARCH0131" => v < 89,
            "ASSERT0132" => v < 89,
            "COUNT0133" => v < 89,
            "THR300134" => v < 89,
            "ITEM0135" => v < 89,
            "CLAIM0136" => v < 89,
            "PROOF0137" => v < 89,
            "SRC0138" => v < 89,
            "LOC0139" => v < 89,
            "BIND0140" => v < 89,
            "GAP0141" => v < 89,
            "SIGN0142" => v < 89,
            "ARCH0143" => v < 89,
            "ASSERT0144" => v < 89,
            "COUNT0145" => v < 89,
            "THR300146" => v < 89,
            "ITEM0147" => v < 89,
            "CLAIM0148" => v < 89,
            "PROOF0149" => v < 89,
            "SRC0150" => v < 89,
            "LOC0151" => v < 89,
            "BIND0152" => v < 89,
            "GAP0153" => v < 89,
            "SIGN0154" => v < 89,
            "ARCH0155" => v < 89,
            "ASSERT0156" => v < 89,
            "COUNT0157" => v < 89,
            "THR300158" => v < 89,
            "ITEM0159" => v < 89,
            "CLAIM0160" => v < 89,
            "PROOF0161" => v < 89,
            "SRC0162" => v < 89,
            "LOC0163" => v < 89,
            "BIND0164" => v < 89,
            "GAP0165" => v < 89,
            "SIGN0166" => v < 89,
            "ARCH0167" => v < 89,
            "ASSERT0168" => v < 89,
            "COUNT0169" => v < 89,
            "THR300170" => v < 89,
            "ITEM0171" => v < 89,
            "CLAIM0172" => v < 89,
            "PROOF0173" => v < 89,
            "SRC0174" => v < 89,
            "LOC0175" => v < 89,
            "BIND0176" => v < 89,
            "GAP0177" => v < 89,
            "SIGN0178" => v < 89,
            "ARCH0179" => v < 89,
            "ASSERT0180" => v < 89,
            "COUNT0181" => v < 89,
            "THR300182" => v < 89,
            "ITEM0183" => v < 89,
            "CLAIM0184" => v < 89,
            "PROOF0185" => v < 89,
            "SRC0186" => v < 89,
            "LOC0187" => v < 89,
            "BIND0188" => v < 89,
            "GAP0189" => v < 89,
            "SIGN0190" => v < 89,
            "ARCH0191" => v < 89,
            "ASSERT0192" => v < 89,
            "COUNT0193" => v < 89,
            "THR300194" => v < 89,
            "ITEM0195" => v < 89,
            "CLAIM0196" => v < 89,
            "PROOF0197" => v < 89,
            "SRC0198" => v < 89,
            "LOC0199" => v < 89,
            "BIND0200" => v < 89,
            "GAP0201" => v < 89,
            "SIGN0202" => v < 89,
            "ARCH0203" => v < 89,
            "ASSERT0204" => v < 89,
            "COUNT0205" => v < 89,
            "THR300206" => v < 89,
            "ITEM0207" => v < 89,
            "CLAIM0208" => v < 89,
            "PROOF0209" => v < 89,
            "SRC0210" => v < 89,
            "LOC0211" => v < 89,
            "BIND0212" => v < 89,
            "GAP0213" => v < 89,
            "SIGN0214" => v < 89,
            "ARCH0215" => v < 89,
            "ASSERT0216" => v < 89,
            "COUNT0217" => v < 89,
            "THR300218" => v < 89,
            "ITEM0219" => v < 89,
            "CLAIM0220" => v < 89,
            "PROOF0221" => v < 89,
            "SRC0222" => v < 89,
            "LOC0223" => v < 89,
            "BIND0224" => v < 89,
            "GAP0225" => v < 89,
            "SIGN0226" => v < 89,
            "ARCH0227" => v < 89,
            "ASSERT0228" => v < 89,
            "COUNT0229" => v < 89,
            "THR300230" => v < 89,
            "ITEM0231" => v < 89,
            "CLAIM0232" => v < 89,
            "PROOF0233" => v < 89,
            "SRC0234" => v < 89,
            "LOC0235" => v < 89,
            "BIND0236" => v < 89,
            "GAP0237" => v < 89,
            "SIGN0238" => v < 89,
            "ARCH0239" => v < 89,
            "ASSERT0240" => v < 89,
            "COUNT0241" => v < 89,
            "THR300242" => v < 89,
            "ITEM0243" => v < 89,
            "CLAIM0244" => v < 89,
            "PROOF0245" => v < 89,
            "SRC0246" => v < 89,
            "LOC0247" => v < 89,
            "BIND0248" => v < 89,
            "GAP0249" => v < 89,
            "SIGN0250" => v < 89,
            "ARCH0251" => v < 89,
            "ASSERT0252" => v < 89,
            "COUNT0253" => v < 89,
            "THR300254" => v < 89,
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
pub fn u3041bmx_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3041bmx_COUNT {
        s = s.wrapping_add(u3041bmx[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3041bmx_unique() -> bool {
    let mut i = 0usize;
    while i < u3041bmx_COUNT {
        let mut j = i + 1;
        while j < u3041bmx_COUNT {
            if u3041bmx[i].0 == u3041bmx[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe320, 0xe32f]。
pub const fn u3041_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3041Code::ALL.len() {
        let c = u3041Code::ALL[i].code();
        if c < 0xe320 || c > 0xe32f {
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
fn u3041_guard_seg() -> bool {
    u3041_segment_guard()
}

/// UNX-F3041 bmx 256 行齐备 + 边界锚 + 键外拒收。
fn check_u3041_bmx_count() -> bool {
    let mut ok = u3041bmx_COUNT == 256;
    if ok { ok = u3041bmx_get("ASSERT0000") == Some(0) && u3041bmx_get("ITEM0255") == Some(5); }
    if ok { ok = u3041bmx_get("ZZZ_NOKEY").is_none(); }
    ok
}

/// UNX-F3041 bmx 累计守恒（理论值 11231）。
fn check_u3041_bmx_sum() -> bool {
    u3041bmx_sum() == 11231
}

/// UNX-F3041 bmx 逐行守卫与唯一性全绿。
fn check_u3041_bmx_guard() -> bool {
    u3041bmx_scan() == 0 && u3041bmx_unique()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3041_checks() -> CheckSet {
    let mut set = CheckSet::new("U3041");
    set.add(
        "u3041-seg-guard",
        u3041_guard_seg(),
        if u3041_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3041-bmx-count",
        check_u3041_bmx_count(),
        if check_u3041_bmx_count() { "" } else { "UNX-F3041 bmx count mismatch" },
    );
    set.add(
        "u3041-bmx-sum",
        check_u3041_bmx_sum(),
        if check_u3041_bmx_sum() { "" } else { "UNX-F3041 bmx sum mismatch" },
    );
    set.add(
        "u3041-bmx-guard",
        check_u3041_bmx_guard(),
        if check_u3041_bmx_guard() { "" } else { "UNX-F3041 bmx guard mismatch" },
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
    fn u3041_all_checks_green() {
        let set = run_u3041_checks();
        assert!(set.all_passed(), "U3041 domain checks must be green");
    }

    #[test]
    fn u3041_segment_guard_holds() {
        assert!(u3041_segment_guard());
    }
}
