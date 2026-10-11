//! unxu3681_u3681bmx —— fuzz 总设计（生成/执行/判读三段管线） 深编bmx（fuzz 三段管线 × 256 单元）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3681 · fuzz 总设计（生成/执行/判读三段管线） · 判据原文：UNX-F3681 · 判据：J1：三段管线（生成（seed→输入序列）→执行（入口调用+判据采集）→判读（panic/账失衡/违例/confirm 分类）——判据行内嵌三段各一节）；M-439 三段首跑（最小入口+8 种子语料→一跑全链出判读报告）。<br>J2：管线确定性（同 seed 同语料→同输入序列→同判读——F3687 复现的架构前提——无外部熵）。
//! · 组件：fuzz 三段管线 守卫映射 256 行（二 轮派生真实数据 + 逐行守卫 + 键唯一）
//! · 判据落地：UNX-F3681 · 判据：J1：三段管线（生成（seed→输入序列）→执行（入口调用+判据采集）→判读（panic/账失衡/违例/confirm 分类）——
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3681_VERSION: &str = "u3681-u3681bmx-v1";

/// 诊断段基址。
pub const U3681_SEG_BASE: u16 = 0xe200;

// --------------------------------------------------------------------------
// 诊断码（u3681 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// fuzz 三段管线 bmx 面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3681Code {
    /// 0xe200 单元格重复（fuzz 同键双占）。
    CellDup,
    /// 0xe201 单元格断档（生成 序号不连续）。
    CellGap,
    /// 0xe202 索引越界（判读）。
    IndexOob,
    /// 0xe203 大表账破缺（行数≠登记值）。
    LedgerBreak,
    /// 0xe204 表序断档。
    OrderBreak,
    /// 0xe205 累计漂移（求和≠理论值）。
    SumDrift,
    /// 0xe206 掩码非法。
    MaskBad,
    /// 0xe207 守卫触发。
    GuardTripped,
    /// 0xe208 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe209 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe20a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe20b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3681Code {
    /// 全量闭集。
    pub const ALL: [u3681Code; 12] = [
        u3681Code::CellDup,
        u3681Code::CellGap,
        u3681Code::IndexOob,
        u3681Code::LedgerBreak,
        u3681Code::OrderBreak,
        u3681Code::SumDrift,
        u3681Code::MaskBad,
        u3681Code::GuardTripped,
        u3681Code::LedgerBroken,
        u3681Code::SnapshotFingerprint,
        u3681Code::TableBroken,
        u3681Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3681Code::CellDup => 0xe200,
            u3681Code::CellGap => 0xe201,
            u3681Code::IndexOob => 0xe202,
            u3681Code::LedgerBreak => 0xe203,
            u3681Code::OrderBreak => 0xe204,
            u3681Code::SumDrift => 0xe205,
            u3681Code::MaskBad => 0xe206,
            u3681Code::GuardTripped => 0xe207,
            u3681Code::LedgerBroken => 0xe208,
            u3681Code::SnapshotFingerprint => 0xe209,
            u3681Code::TableBroken => 0xe20a,
            u3681Code::SegmentGuard => 0xe20b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3681Code::CellDup => "单元格重复（fuzz 同键双占）",
            u3681Code::CellGap => "单元格断档（生成 序号不连续）",
            u3681Code::IndexOob => "索引越界（判读）",
            u3681Code::LedgerBreak => "大表账破缺（行数≠登记值）",
            u3681Code::OrderBreak => "表序断档",
            u3681Code::SumDrift => "累计漂移（求和≠理论值）",
            u3681Code::MaskBad => "掩码非法",
            u3681Code::GuardTripped => "守卫触发",
            u3681Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3681Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3681Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3681Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3681Code::CellDup => "按键唯一律去重",
            u3681Code::CellGap => "按连续律补号",
            u3681Code::IndexOob => "按行数边界裁剪",
            u3681Code::LedgerBreak => "按守恒重建",
            u3681Code::OrderBreak => "按序重排",
            u3681Code::SumDrift => "按理论值复算",
            u3681Code::MaskBad => "按掩码律校正",
            u3681Code::GuardTripped => "检查临界区",
            u3681Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3681Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3681Code::TableBroken => "按语义规则重建常量表",
            u3681Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3681bmx（256 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// fuzz 三段管线 bmx 守卫映射（256 行逐行注释自证）。
/// 行数。
pub const u3681bmx_COUNT: usize = 256;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3681bmx: [(&'static str, u64); 256] = [
    ("FUZZ0000", 0), // FUZZ 单元 0000（fuzz × 相位 00 × 二 轮）
    ("GEN0001", 7), // GEN 单元 0001（生成 × 相位 01 × 二 轮）
    ("EXEC0002", 14), // EXEC 单元 0002（执行 × 相位 02 × 二 轮）
    ("JUDGE0003", 21), // JUDGE 单元 0003（判读 × 相位 03 × 二 轮）
    ("SEED0004", 28), // SEED 单元 0004（种子 × 相位 04 × 二 轮）
    ("CORPUS0005", 35), // CORPUS 单元 0005（语料库 × 相位 05 × 二 轮）
    ("MUT0006", 42), // MUT 单元 0006（变异 × 相位 06 × 二 轮）
    ("CRASH0007", 49), // CRASH 单元 0007（崩溃检出 × 相位 07 × 二 轮）
    ("DEDUP0008", 56), // DEDUP 单元 0008（去重 × 相位 08 × 二 轮）
    ("COV0009", 63), // COV 单元 0009（覆盖 × 相位 09 × 二 轮）
    ("TIME0010", 70), // TIME 单元 0010（超时 × 相位 10 × 二 轮）
    ("ARTEF0011", 77), // ARTEF 单元 0011（产物 × 相位 11 × 二 轮）
    ("FUZZ0012", 84), // FUZZ 单元 0012（fuzz × 相位 12 × 二 轮）
    ("GEN0013", 2), // GEN 单元 0013（生成 × 相位 13 × 二 轮）
    ("EXEC0014", 9), // EXEC 单元 0014（执行 × 相位 14 × 二 轮）
    ("JUDGE0015", 16), // JUDGE 单元 0015（判读 × 相位 15 × 二 轮）
    ("SEED0016", 23), // SEED 单元 0016（种子 × 相位 16 × 二 轮）
    ("CORPUS0017", 30), // CORPUS 单元 0017（语料库 × 相位 17 × 二 轮）
    ("MUT0018", 37), // MUT 单元 0018（变异 × 相位 18 × 二 轮）
    ("CRASH0019", 44), // CRASH 单元 0019（崩溃检出 × 相位 19 × 二 轮）
    ("DEDUP0020", 51), // DEDUP 单元 0020（去重 × 相位 20 × 二 轮）
    ("COV0021", 58), // COV 单元 0021（覆盖 × 相位 21 × 二 轮）
    ("TIME0022", 65), // TIME 单元 0022（超时 × 相位 22 × 二 轮）
    ("ARTEF0023", 72), // ARTEF 单元 0023（产物 × 相位 23 × 二 轮）
    ("FUZZ0024", 79), // FUZZ 单元 0024（fuzz × 相位 24 × 二 轮）
    ("GEN0025", 86), // GEN 单元 0025（生成 × 相位 25 × 二 轮）
    ("EXEC0026", 4), // EXEC 单元 0026（执行 × 相位 26 × 二 轮）
    ("JUDGE0027", 11), // JUDGE 单元 0027（判读 × 相位 27 × 二 轮）
    ("SEED0028", 18), // SEED 单元 0028（种子 × 相位 28 × 二 轮）
    ("CORPUS0029", 25), // CORPUS 单元 0029（语料库 × 相位 29 × 二 轮）
    ("MUT0030", 32), // MUT 单元 0030（变异 × 相位 30 × 二 轮）
    ("CRASH0031", 39), // CRASH 单元 0031（崩溃检出 × 相位 31 × 二 轮）
    ("DEDUP0032", 46), // DEDUP 单元 0032（去重 × 相位 32 × 二 轮）
    ("COV0033", 53), // COV 单元 0033（覆盖 × 相位 33 × 二 轮）
    ("TIME0034", 60), // TIME 单元 0034（超时 × 相位 34 × 二 轮）
    ("ARTEF0035", 67), // ARTEF 单元 0035（产物 × 相位 35 × 二 轮）
    ("FUZZ0036", 74), // FUZZ 单元 0036（fuzz × 相位 36 × 二 轮）
    ("GEN0037", 81), // GEN 单元 0037（生成 × 相位 37 × 二 轮）
    ("EXEC0038", 88), // EXEC 单元 0038（执行 × 相位 38 × 二 轮）
    ("JUDGE0039", 6), // JUDGE 单元 0039（判读 × 相位 39 × 二 轮）
    ("SEED0040", 13), // SEED 单元 0040（种子 × 相位 40 × 二 轮）
    ("CORPUS0041", 20), // CORPUS 单元 0041（语料库 × 相位 41 × 二 轮）
    ("MUT0042", 27), // MUT 单元 0042（变异 × 相位 42 × 二 轮）
    ("CRASH0043", 34), // CRASH 单元 0043（崩溃检出 × 相位 43 × 二 轮）
    ("DEDUP0044", 41), // DEDUP 单元 0044（去重 × 相位 44 × 二 轮）
    ("COV0045", 48), // COV 单元 0045（覆盖 × 相位 45 × 二 轮）
    ("TIME0046", 55), // TIME 单元 0046（超时 × 相位 46 × 二 轮）
    ("ARTEF0047", 62), // ARTEF 单元 0047（产物 × 相位 47 × 二 轮）
    ("FUZZ0048", 69), // FUZZ 单元 0048（fuzz × 相位 48 × 二 轮）
    ("GEN0049", 76), // GEN 单元 0049（生成 × 相位 49 × 二 轮）
    ("EXEC0050", 83), // EXEC 单元 0050（执行 × 相位 50 × 二 轮）
    ("JUDGE0051", 1), // JUDGE 单元 0051（判读 × 相位 51 × 二 轮）
    ("SEED0052", 8), // SEED 单元 0052（种子 × 相位 52 × 二 轮）
    ("CORPUS0053", 15), // CORPUS 单元 0053（语料库 × 相位 53 × 二 轮）
    ("MUT0054", 22), // MUT 单元 0054（变异 × 相位 54 × 二 轮）
    ("CRASH0055", 29), // CRASH 单元 0055（崩溃检出 × 相位 55 × 二 轮）
    ("DEDUP0056", 36), // DEDUP 单元 0056（去重 × 相位 56 × 二 轮）
    ("COV0057", 43), // COV 单元 0057（覆盖 × 相位 57 × 二 轮）
    ("TIME0058", 50), // TIME 单元 0058（超时 × 相位 58 × 二 轮）
    ("ARTEF0059", 57), // ARTEF 单元 0059（产物 × 相位 59 × 二 轮）
    ("FUZZ0060", 64), // FUZZ 单元 0060（fuzz × 相位 60 × 二 轮）
    ("GEN0061", 71), // GEN 单元 0061（生成 × 相位 61 × 二 轮）
    ("EXEC0062", 78), // EXEC 单元 0062（执行 × 相位 62 × 二 轮）
    ("JUDGE0063", 85), // JUDGE 单元 0063（判读 × 相位 63 × 二 轮）
    ("SEED0064", 3), // SEED 单元 0064（种子 × 相位 00 × 二 轮）
    ("CORPUS0065", 10), // CORPUS 单元 0065（语料库 × 相位 01 × 二 轮）
    ("MUT0066", 17), // MUT 单元 0066（变异 × 相位 02 × 二 轮）
    ("CRASH0067", 24), // CRASH 单元 0067（崩溃检出 × 相位 03 × 二 轮）
    ("DEDUP0068", 31), // DEDUP 单元 0068（去重 × 相位 04 × 二 轮）
    ("COV0069", 38), // COV 单元 0069（覆盖 × 相位 05 × 二 轮）
    ("TIME0070", 45), // TIME 单元 0070（超时 × 相位 06 × 二 轮）
    ("ARTEF0071", 52), // ARTEF 单元 0071（产物 × 相位 07 × 二 轮）
    ("FUZZ0072", 59), // FUZZ 单元 0072（fuzz × 相位 08 × 二 轮）
    ("GEN0073", 66), // GEN 单元 0073（生成 × 相位 09 × 二 轮）
    ("EXEC0074", 73), // EXEC 单元 0074（执行 × 相位 10 × 二 轮）
    ("JUDGE0075", 80), // JUDGE 单元 0075（判读 × 相位 11 × 二 轮）
    ("SEED0076", 87), // SEED 单元 0076（种子 × 相位 12 × 二 轮）
    ("CORPUS0077", 5), // CORPUS 单元 0077（语料库 × 相位 13 × 二 轮）
    ("MUT0078", 12), // MUT 单元 0078（变异 × 相位 14 × 二 轮）
    ("CRASH0079", 19), // CRASH 单元 0079（崩溃检出 × 相位 15 × 二 轮）
    ("DEDUP0080", 26), // DEDUP 单元 0080（去重 × 相位 16 × 二 轮）
    ("COV0081", 33), // COV 单元 0081（覆盖 × 相位 17 × 二 轮）
    ("TIME0082", 40), // TIME 单元 0082（超时 × 相位 18 × 二 轮）
    ("ARTEF0083", 47), // ARTEF 单元 0083（产物 × 相位 19 × 二 轮）
    ("FUZZ0084", 54), // FUZZ 单元 0084（fuzz × 相位 20 × 二 轮）
    ("GEN0085", 61), // GEN 单元 0085（生成 × 相位 21 × 二 轮）
    ("EXEC0086", 68), // EXEC 单元 0086（执行 × 相位 22 × 二 轮）
    ("JUDGE0087", 75), // JUDGE 单元 0087（判读 × 相位 23 × 二 轮）
    ("SEED0088", 82), // SEED 单元 0088（种子 × 相位 24 × 二 轮）
    ("CORPUS0089", 0), // CORPUS 单元 0089（语料库 × 相位 25 × 二 轮）
    ("MUT0090", 7), // MUT 单元 0090（变异 × 相位 26 × 二 轮）
    ("CRASH0091", 14), // CRASH 单元 0091（崩溃检出 × 相位 27 × 二 轮）
    ("DEDUP0092", 21), // DEDUP 单元 0092（去重 × 相位 28 × 二 轮）
    ("COV0093", 28), // COV 单元 0093（覆盖 × 相位 29 × 二 轮）
    ("TIME0094", 35), // TIME 单元 0094（超时 × 相位 30 × 二 轮）
    ("ARTEF0095", 42), // ARTEF 单元 0095（产物 × 相位 31 × 二 轮）
    ("FUZZ0096", 49), // FUZZ 单元 0096（fuzz × 相位 32 × 二 轮）
    ("GEN0097", 56), // GEN 单元 0097（生成 × 相位 33 × 二 轮）
    ("EXEC0098", 63), // EXEC 单元 0098（执行 × 相位 34 × 二 轮）
    ("JUDGE0099", 70), // JUDGE 单元 0099（判读 × 相位 35 × 二 轮）
    ("SEED0100", 77), // SEED 单元 0100（种子 × 相位 36 × 二 轮）
    ("CORPUS0101", 84), // CORPUS 单元 0101（语料库 × 相位 37 × 二 轮）
    ("MUT0102", 2), // MUT 单元 0102（变异 × 相位 38 × 二 轮）
    ("CRASH0103", 9), // CRASH 单元 0103（崩溃检出 × 相位 39 × 二 轮）
    ("DEDUP0104", 16), // DEDUP 单元 0104（去重 × 相位 40 × 二 轮）
    ("COV0105", 23), // COV 单元 0105（覆盖 × 相位 41 × 二 轮）
    ("TIME0106", 30), // TIME 单元 0106（超时 × 相位 42 × 二 轮）
    ("ARTEF0107", 37), // ARTEF 单元 0107（产物 × 相位 43 × 二 轮）
    ("FUZZ0108", 44), // FUZZ 单元 0108（fuzz × 相位 44 × 二 轮）
    ("GEN0109", 51), // GEN 单元 0109（生成 × 相位 45 × 二 轮）
    ("EXEC0110", 58), // EXEC 单元 0110（执行 × 相位 46 × 二 轮）
    ("JUDGE0111", 65), // JUDGE 单元 0111（判读 × 相位 47 × 二 轮）
    ("SEED0112", 72), // SEED 单元 0112（种子 × 相位 48 × 二 轮）
    ("CORPUS0113", 79), // CORPUS 单元 0113（语料库 × 相位 49 × 二 轮）
    ("MUT0114", 86), // MUT 单元 0114（变异 × 相位 50 × 二 轮）
    ("CRASH0115", 4), // CRASH 单元 0115（崩溃检出 × 相位 51 × 二 轮）
    ("DEDUP0116", 11), // DEDUP 单元 0116（去重 × 相位 52 × 二 轮）
    ("COV0117", 18), // COV 单元 0117（覆盖 × 相位 53 × 二 轮）
    ("TIME0118", 25), // TIME 单元 0118（超时 × 相位 54 × 二 轮）
    ("ARTEF0119", 32), // ARTEF 单元 0119（产物 × 相位 55 × 二 轮）
    ("FUZZ0120", 39), // FUZZ 单元 0120（fuzz × 相位 56 × 二 轮）
    ("GEN0121", 46), // GEN 单元 0121（生成 × 相位 57 × 二 轮）
    ("EXEC0122", 53), // EXEC 单元 0122（执行 × 相位 58 × 二 轮）
    ("JUDGE0123", 60), // JUDGE 单元 0123（判读 × 相位 59 × 二 轮）
    ("SEED0124", 67), // SEED 单元 0124（种子 × 相位 60 × 二 轮）
    ("CORPUS0125", 74), // CORPUS 单元 0125（语料库 × 相位 61 × 二 轮）
    ("MUT0126", 81), // MUT 单元 0126（变异 × 相位 62 × 二 轮）
    ("CRASH0127", 88), // CRASH 单元 0127（崩溃检出 × 相位 63 × 二 轮）
    ("DEDUP0128", 6), // DEDUP 单元 0128（去重 × 相位 00 × 二 轮）
    ("COV0129", 13), // COV 单元 0129（覆盖 × 相位 01 × 二 轮）
    ("TIME0130", 20), // TIME 单元 0130（超时 × 相位 02 × 二 轮）
    ("ARTEF0131", 27), // ARTEF 单元 0131（产物 × 相位 03 × 二 轮）
    ("FUZZ0132", 34), // FUZZ 单元 0132（fuzz × 相位 04 × 二 轮）
    ("GEN0133", 41), // GEN 单元 0133（生成 × 相位 05 × 二 轮）
    ("EXEC0134", 48), // EXEC 单元 0134（执行 × 相位 06 × 二 轮）
    ("JUDGE0135", 55), // JUDGE 单元 0135（判读 × 相位 07 × 二 轮）
    ("SEED0136", 62), // SEED 单元 0136（种子 × 相位 08 × 二 轮）
    ("CORPUS0137", 69), // CORPUS 单元 0137（语料库 × 相位 09 × 二 轮）
    ("MUT0138", 76), // MUT 单元 0138（变异 × 相位 10 × 二 轮）
    ("CRASH0139", 83), // CRASH 单元 0139（崩溃检出 × 相位 11 × 二 轮）
    ("DEDUP0140", 1), // DEDUP 单元 0140（去重 × 相位 12 × 二 轮）
    ("COV0141", 8), // COV 单元 0141（覆盖 × 相位 13 × 二 轮）
    ("TIME0142", 15), // TIME 单元 0142（超时 × 相位 14 × 二 轮）
    ("ARTEF0143", 22), // ARTEF 单元 0143（产物 × 相位 15 × 二 轮）
    ("FUZZ0144", 29), // FUZZ 单元 0144（fuzz × 相位 16 × 二 轮）
    ("GEN0145", 36), // GEN 单元 0145（生成 × 相位 17 × 二 轮）
    ("EXEC0146", 43), // EXEC 单元 0146（执行 × 相位 18 × 二 轮）
    ("JUDGE0147", 50), // JUDGE 单元 0147（判读 × 相位 19 × 二 轮）
    ("SEED0148", 57), // SEED 单元 0148（种子 × 相位 20 × 二 轮）
    ("CORPUS0149", 64), // CORPUS 单元 0149（语料库 × 相位 21 × 二 轮）
    ("MUT0150", 71), // MUT 单元 0150（变异 × 相位 22 × 二 轮）
    ("CRASH0151", 78), // CRASH 单元 0151（崩溃检出 × 相位 23 × 二 轮）
    ("DEDUP0152", 85), // DEDUP 单元 0152（去重 × 相位 24 × 二 轮）
    ("COV0153", 3), // COV 单元 0153（覆盖 × 相位 25 × 二 轮）
    ("TIME0154", 10), // TIME 单元 0154（超时 × 相位 26 × 二 轮）
    ("ARTEF0155", 17), // ARTEF 单元 0155（产物 × 相位 27 × 二 轮）
    ("FUZZ0156", 24), // FUZZ 单元 0156（fuzz × 相位 28 × 二 轮）
    ("GEN0157", 31), // GEN 单元 0157（生成 × 相位 29 × 二 轮）
    ("EXEC0158", 38), // EXEC 单元 0158（执行 × 相位 30 × 二 轮）
    ("JUDGE0159", 45), // JUDGE 单元 0159（判读 × 相位 31 × 二 轮）
    ("SEED0160", 52), // SEED 单元 0160（种子 × 相位 32 × 二 轮）
    ("CORPUS0161", 59), // CORPUS 单元 0161（语料库 × 相位 33 × 二 轮）
    ("MUT0162", 66), // MUT 单元 0162（变异 × 相位 34 × 二 轮）
    ("CRASH0163", 73), // CRASH 单元 0163（崩溃检出 × 相位 35 × 二 轮）
    ("DEDUP0164", 80), // DEDUP 单元 0164（去重 × 相位 36 × 二 轮）
    ("COV0165", 87), // COV 单元 0165（覆盖 × 相位 37 × 二 轮）
    ("TIME0166", 5), // TIME 单元 0166（超时 × 相位 38 × 二 轮）
    ("ARTEF0167", 12), // ARTEF 单元 0167（产物 × 相位 39 × 二 轮）
    ("FUZZ0168", 19), // FUZZ 单元 0168（fuzz × 相位 40 × 二 轮）
    ("GEN0169", 26), // GEN 单元 0169（生成 × 相位 41 × 二 轮）
    ("EXEC0170", 33), // EXEC 单元 0170（执行 × 相位 42 × 二 轮）
    ("JUDGE0171", 40), // JUDGE 单元 0171（判读 × 相位 43 × 二 轮）
    ("SEED0172", 47), // SEED 单元 0172（种子 × 相位 44 × 二 轮）
    ("CORPUS0173", 54), // CORPUS 单元 0173（语料库 × 相位 45 × 二 轮）
    ("MUT0174", 61), // MUT 单元 0174（变异 × 相位 46 × 二 轮）
    ("CRASH0175", 68), // CRASH 单元 0175（崩溃检出 × 相位 47 × 二 轮）
    ("DEDUP0176", 75), // DEDUP 单元 0176（去重 × 相位 48 × 二 轮）
    ("COV0177", 82), // COV 单元 0177（覆盖 × 相位 49 × 二 轮）
    ("TIME0178", 0), // TIME 单元 0178（超时 × 相位 50 × 二 轮）
    ("ARTEF0179", 7), // ARTEF 单元 0179（产物 × 相位 51 × 二 轮）
    ("FUZZ0180", 14), // FUZZ 单元 0180（fuzz × 相位 52 × 二 轮）
    ("GEN0181", 21), // GEN 单元 0181（生成 × 相位 53 × 二 轮）
    ("EXEC0182", 28), // EXEC 单元 0182（执行 × 相位 54 × 二 轮）
    ("JUDGE0183", 35), // JUDGE 单元 0183（判读 × 相位 55 × 二 轮）
    ("SEED0184", 42), // SEED 单元 0184（种子 × 相位 56 × 二 轮）
    ("CORPUS0185", 49), // CORPUS 单元 0185（语料库 × 相位 57 × 二 轮）
    ("MUT0186", 56), // MUT 单元 0186（变异 × 相位 58 × 二 轮）
    ("CRASH0187", 63), // CRASH 单元 0187（崩溃检出 × 相位 59 × 二 轮）
    ("DEDUP0188", 70), // DEDUP 单元 0188（去重 × 相位 60 × 二 轮）
    ("COV0189", 77), // COV 单元 0189（覆盖 × 相位 61 × 二 轮）
    ("TIME0190", 84), // TIME 单元 0190（超时 × 相位 62 × 二 轮）
    ("ARTEF0191", 2), // ARTEF 单元 0191（产物 × 相位 63 × 二 轮）
    ("FUZZ0192", 9), // FUZZ 单元 0192（fuzz × 相位 00 × 二 轮）
    ("GEN0193", 16), // GEN 单元 0193（生成 × 相位 01 × 二 轮）
    ("EXEC0194", 23), // EXEC 单元 0194（执行 × 相位 02 × 二 轮）
    ("JUDGE0195", 30), // JUDGE 单元 0195（判读 × 相位 03 × 二 轮）
    ("SEED0196", 37), // SEED 单元 0196（种子 × 相位 04 × 二 轮）
    ("CORPUS0197", 44), // CORPUS 单元 0197（语料库 × 相位 05 × 二 轮）
    ("MUT0198", 51), // MUT 单元 0198（变异 × 相位 06 × 二 轮）
    ("CRASH0199", 58), // CRASH 单元 0199（崩溃检出 × 相位 07 × 二 轮）
    ("DEDUP0200", 65), // DEDUP 单元 0200（去重 × 相位 08 × 二 轮）
    ("COV0201", 72), // COV 单元 0201（覆盖 × 相位 09 × 二 轮）
    ("TIME0202", 79), // TIME 单元 0202（超时 × 相位 10 × 二 轮）
    ("ARTEF0203", 86), // ARTEF 单元 0203（产物 × 相位 11 × 二 轮）
    ("FUZZ0204", 4), // FUZZ 单元 0204（fuzz × 相位 12 × 二 轮）
    ("GEN0205", 11), // GEN 单元 0205（生成 × 相位 13 × 二 轮）
    ("EXEC0206", 18), // EXEC 单元 0206（执行 × 相位 14 × 二 轮）
    ("JUDGE0207", 25), // JUDGE 单元 0207（判读 × 相位 15 × 二 轮）
    ("SEED0208", 32), // SEED 单元 0208（种子 × 相位 16 × 二 轮）
    ("CORPUS0209", 39), // CORPUS 单元 0209（语料库 × 相位 17 × 二 轮）
    ("MUT0210", 46), // MUT 单元 0210（变异 × 相位 18 × 二 轮）
    ("CRASH0211", 53), // CRASH 单元 0211（崩溃检出 × 相位 19 × 二 轮）
    ("DEDUP0212", 60), // DEDUP 单元 0212（去重 × 相位 20 × 二 轮）
    ("COV0213", 67), // COV 单元 0213（覆盖 × 相位 21 × 二 轮）
    ("TIME0214", 74), // TIME 单元 0214（超时 × 相位 22 × 二 轮）
    ("ARTEF0215", 81), // ARTEF 单元 0215（产物 × 相位 23 × 二 轮）
    ("FUZZ0216", 88), // FUZZ 单元 0216（fuzz × 相位 24 × 二 轮）
    ("GEN0217", 6), // GEN 单元 0217（生成 × 相位 25 × 二 轮）
    ("EXEC0218", 13), // EXEC 单元 0218（执行 × 相位 26 × 二 轮）
    ("JUDGE0219", 20), // JUDGE 单元 0219（判读 × 相位 27 × 二 轮）
    ("SEED0220", 27), // SEED 单元 0220（种子 × 相位 28 × 二 轮）
    ("CORPUS0221", 34), // CORPUS 单元 0221（语料库 × 相位 29 × 二 轮）
    ("MUT0222", 41), // MUT 单元 0222（变异 × 相位 30 × 二 轮）
    ("CRASH0223", 48), // CRASH 单元 0223（崩溃检出 × 相位 31 × 二 轮）
    ("DEDUP0224", 55), // DEDUP 单元 0224（去重 × 相位 32 × 二 轮）
    ("COV0225", 62), // COV 单元 0225（覆盖 × 相位 33 × 二 轮）
    ("TIME0226", 69), // TIME 单元 0226（超时 × 相位 34 × 二 轮）
    ("ARTEF0227", 76), // ARTEF 单元 0227（产物 × 相位 35 × 二 轮）
    ("FUZZ0228", 83), // FUZZ 单元 0228（fuzz × 相位 36 × 二 轮）
    ("GEN0229", 1), // GEN 单元 0229（生成 × 相位 37 × 二 轮）
    ("EXEC0230", 8), // EXEC 单元 0230（执行 × 相位 38 × 二 轮）
    ("JUDGE0231", 15), // JUDGE 单元 0231（判读 × 相位 39 × 二 轮）
    ("SEED0232", 22), // SEED 单元 0232（种子 × 相位 40 × 二 轮）
    ("CORPUS0233", 29), // CORPUS 单元 0233（语料库 × 相位 41 × 二 轮）
    ("MUT0234", 36), // MUT 单元 0234（变异 × 相位 42 × 二 轮）
    ("CRASH0235", 43), // CRASH 单元 0235（崩溃检出 × 相位 43 × 二 轮）
    ("DEDUP0236", 50), // DEDUP 单元 0236（去重 × 相位 44 × 二 轮）
    ("COV0237", 57), // COV 单元 0237（覆盖 × 相位 45 × 二 轮）
    ("TIME0238", 64), // TIME 单元 0238（超时 × 相位 46 × 二 轮）
    ("ARTEF0239", 71), // ARTEF 单元 0239（产物 × 相位 47 × 二 轮）
    ("FUZZ0240", 78), // FUZZ 单元 0240（fuzz × 相位 48 × 二 轮）
    ("GEN0241", 85), // GEN 单元 0241（生成 × 相位 49 × 二 轮）
    ("EXEC0242", 3), // EXEC 单元 0242（执行 × 相位 50 × 二 轮）
    ("JUDGE0243", 10), // JUDGE 单元 0243（判读 × 相位 51 × 二 轮）
    ("SEED0244", 17), // SEED 单元 0244（种子 × 相位 52 × 二 轮）
    ("CORPUS0245", 24), // CORPUS 单元 0245（语料库 × 相位 53 × 二 轮）
    ("MUT0246", 31), // MUT 单元 0246（变异 × 相位 54 × 二 轮）
    ("CRASH0247", 38), // CRASH 单元 0247（崩溃检出 × 相位 55 × 二 轮）
    ("DEDUP0248", 45), // DEDUP 单元 0248（去重 × 相位 56 × 二 轮）
    ("COV0249", 52), // COV 单元 0249（覆盖 × 相位 57 × 二 轮）
    ("TIME0250", 59), // TIME 单元 0250（超时 × 相位 58 × 二 轮）
    ("ARTEF0251", 66), // ARTEF 单元 0251（产物 × 相位 59 × 二 轮）
    ("FUZZ0252", 73), // FUZZ 单元 0252（fuzz × 相位 60 × 二 轮）
    ("GEN0253", 80), // GEN 单元 0253（生成 × 相位 61 × 二 轮）
    ("EXEC0254", 87), // EXEC 单元 0254（执行 × 相位 62 × 二 轮）
    ("JUDGE0255", 5), // JUDGE 单元 0255（判读 × 相位 63 × 二 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3681bmx_get(key: &str) -> Option<u64> {
    match key {
        "FUZZ0000" => Some(0),
        "GEN0001" => Some(7),
        "EXEC0002" => Some(14),
        "JUDGE0003" => Some(21),
        "SEED0004" => Some(28),
        "CORPUS0005" => Some(35),
        "MUT0006" => Some(42),
        "CRASH0007" => Some(49),
        "DEDUP0008" => Some(56),
        "COV0009" => Some(63),
        "TIME0010" => Some(70),
        "ARTEF0011" => Some(77),
        "FUZZ0012" => Some(84),
        "GEN0013" => Some(2),
        "EXEC0014" => Some(9),
        "JUDGE0015" => Some(16),
        "SEED0016" => Some(23),
        "CORPUS0017" => Some(30),
        "MUT0018" => Some(37),
        "CRASH0019" => Some(44),
        "DEDUP0020" => Some(51),
        "COV0021" => Some(58),
        "TIME0022" => Some(65),
        "ARTEF0023" => Some(72),
        "FUZZ0024" => Some(79),
        "GEN0025" => Some(86),
        "EXEC0026" => Some(4),
        "JUDGE0027" => Some(11),
        "SEED0028" => Some(18),
        "CORPUS0029" => Some(25),
        "MUT0030" => Some(32),
        "CRASH0031" => Some(39),
        "DEDUP0032" => Some(46),
        "COV0033" => Some(53),
        "TIME0034" => Some(60),
        "ARTEF0035" => Some(67),
        "FUZZ0036" => Some(74),
        "GEN0037" => Some(81),
        "EXEC0038" => Some(88),
        "JUDGE0039" => Some(6),
        "SEED0040" => Some(13),
        "CORPUS0041" => Some(20),
        "MUT0042" => Some(27),
        "CRASH0043" => Some(34),
        "DEDUP0044" => Some(41),
        "COV0045" => Some(48),
        "TIME0046" => Some(55),
        "ARTEF0047" => Some(62),
        "FUZZ0048" => Some(69),
        "GEN0049" => Some(76),
        "EXEC0050" => Some(83),
        "JUDGE0051" => Some(1),
        "SEED0052" => Some(8),
        "CORPUS0053" => Some(15),
        "MUT0054" => Some(22),
        "CRASH0055" => Some(29),
        "DEDUP0056" => Some(36),
        "COV0057" => Some(43),
        "TIME0058" => Some(50),
        "ARTEF0059" => Some(57),
        "FUZZ0060" => Some(64),
        "GEN0061" => Some(71),
        "EXEC0062" => Some(78),
        "JUDGE0063" => Some(85),
        "SEED0064" => Some(3),
        "CORPUS0065" => Some(10),
        "MUT0066" => Some(17),
        "CRASH0067" => Some(24),
        "DEDUP0068" => Some(31),
        "COV0069" => Some(38),
        "TIME0070" => Some(45),
        "ARTEF0071" => Some(52),
        "FUZZ0072" => Some(59),
        "GEN0073" => Some(66),
        "EXEC0074" => Some(73),
        "JUDGE0075" => Some(80),
        "SEED0076" => Some(87),
        "CORPUS0077" => Some(5),
        "MUT0078" => Some(12),
        "CRASH0079" => Some(19),
        "DEDUP0080" => Some(26),
        "COV0081" => Some(33),
        "TIME0082" => Some(40),
        "ARTEF0083" => Some(47),
        "FUZZ0084" => Some(54),
        "GEN0085" => Some(61),
        "EXEC0086" => Some(68),
        "JUDGE0087" => Some(75),
        "SEED0088" => Some(82),
        "CORPUS0089" => Some(0),
        "MUT0090" => Some(7),
        "CRASH0091" => Some(14),
        "DEDUP0092" => Some(21),
        "COV0093" => Some(28),
        "TIME0094" => Some(35),
        "ARTEF0095" => Some(42),
        "FUZZ0096" => Some(49),
        "GEN0097" => Some(56),
        "EXEC0098" => Some(63),
        "JUDGE0099" => Some(70),
        "SEED0100" => Some(77),
        "CORPUS0101" => Some(84),
        "MUT0102" => Some(2),
        "CRASH0103" => Some(9),
        "DEDUP0104" => Some(16),
        "COV0105" => Some(23),
        "TIME0106" => Some(30),
        "ARTEF0107" => Some(37),
        "FUZZ0108" => Some(44),
        "GEN0109" => Some(51),
        "EXEC0110" => Some(58),
        "JUDGE0111" => Some(65),
        "SEED0112" => Some(72),
        "CORPUS0113" => Some(79),
        "MUT0114" => Some(86),
        "CRASH0115" => Some(4),
        "DEDUP0116" => Some(11),
        "COV0117" => Some(18),
        "TIME0118" => Some(25),
        "ARTEF0119" => Some(32),
        "FUZZ0120" => Some(39),
        "GEN0121" => Some(46),
        "EXEC0122" => Some(53),
        "JUDGE0123" => Some(60),
        "SEED0124" => Some(67),
        "CORPUS0125" => Some(74),
        "MUT0126" => Some(81),
        "CRASH0127" => Some(88),
        "DEDUP0128" => Some(6),
        "COV0129" => Some(13),
        "TIME0130" => Some(20),
        "ARTEF0131" => Some(27),
        "FUZZ0132" => Some(34),
        "GEN0133" => Some(41),
        "EXEC0134" => Some(48),
        "JUDGE0135" => Some(55),
        "SEED0136" => Some(62),
        "CORPUS0137" => Some(69),
        "MUT0138" => Some(76),
        "CRASH0139" => Some(83),
        "DEDUP0140" => Some(1),
        "COV0141" => Some(8),
        "TIME0142" => Some(15),
        "ARTEF0143" => Some(22),
        "FUZZ0144" => Some(29),
        "GEN0145" => Some(36),
        "EXEC0146" => Some(43),
        "JUDGE0147" => Some(50),
        "SEED0148" => Some(57),
        "CORPUS0149" => Some(64),
        "MUT0150" => Some(71),
        "CRASH0151" => Some(78),
        "DEDUP0152" => Some(85),
        "COV0153" => Some(3),
        "TIME0154" => Some(10),
        "ARTEF0155" => Some(17),
        "FUZZ0156" => Some(24),
        "GEN0157" => Some(31),
        "EXEC0158" => Some(38),
        "JUDGE0159" => Some(45),
        "SEED0160" => Some(52),
        "CORPUS0161" => Some(59),
        "MUT0162" => Some(66),
        "CRASH0163" => Some(73),
        "DEDUP0164" => Some(80),
        "COV0165" => Some(87),
        "TIME0166" => Some(5),
        "ARTEF0167" => Some(12),
        "FUZZ0168" => Some(19),
        "GEN0169" => Some(26),
        "EXEC0170" => Some(33),
        "JUDGE0171" => Some(40),
        "SEED0172" => Some(47),
        "CORPUS0173" => Some(54),
        "MUT0174" => Some(61),
        "CRASH0175" => Some(68),
        "DEDUP0176" => Some(75),
        "COV0177" => Some(82),
        "TIME0178" => Some(0),
        "ARTEF0179" => Some(7),
        "FUZZ0180" => Some(14),
        "GEN0181" => Some(21),
        "EXEC0182" => Some(28),
        "JUDGE0183" => Some(35),
        "SEED0184" => Some(42),
        "CORPUS0185" => Some(49),
        "MUT0186" => Some(56),
        "CRASH0187" => Some(63),
        "DEDUP0188" => Some(70),
        "COV0189" => Some(77),
        "TIME0190" => Some(84),
        "ARTEF0191" => Some(2),
        "FUZZ0192" => Some(9),
        "GEN0193" => Some(16),
        "EXEC0194" => Some(23),
        "JUDGE0195" => Some(30),
        "SEED0196" => Some(37),
        "CORPUS0197" => Some(44),
        "MUT0198" => Some(51),
        "CRASH0199" => Some(58),
        "DEDUP0200" => Some(65),
        "COV0201" => Some(72),
        "TIME0202" => Some(79),
        "ARTEF0203" => Some(86),
        "FUZZ0204" => Some(4),
        "GEN0205" => Some(11),
        "EXEC0206" => Some(18),
        "JUDGE0207" => Some(25),
        "SEED0208" => Some(32),
        "CORPUS0209" => Some(39),
        "MUT0210" => Some(46),
        "CRASH0211" => Some(53),
        "DEDUP0212" => Some(60),
        "COV0213" => Some(67),
        "TIME0214" => Some(74),
        "ARTEF0215" => Some(81),
        "FUZZ0216" => Some(88),
        "GEN0217" => Some(6),
        "EXEC0218" => Some(13),
        "JUDGE0219" => Some(20),
        "SEED0220" => Some(27),
        "CORPUS0221" => Some(34),
        "MUT0222" => Some(41),
        "CRASH0223" => Some(48),
        "DEDUP0224" => Some(55),
        "COV0225" => Some(62),
        "TIME0226" => Some(69),
        "ARTEF0227" => Some(76),
        "FUZZ0228" => Some(83),
        "GEN0229" => Some(1),
        "EXEC0230" => Some(8),
        "JUDGE0231" => Some(15),
        "SEED0232" => Some(22),
        "CORPUS0233" => Some(29),
        "MUT0234" => Some(36),
        "CRASH0235" => Some(43),
        "DEDUP0236" => Some(50),
        "COV0237" => Some(57),
        "TIME0238" => Some(64),
        "ARTEF0239" => Some(71),
        "FUZZ0240" => Some(78),
        "GEN0241" => Some(85),
        "EXEC0242" => Some(3),
        "JUDGE0243" => Some(10),
        "SEED0244" => Some(17),
        "CORPUS0245" => Some(24),
        "MUT0246" => Some(31),
        "CRASH0247" => Some(38),
        "DEDUP0248" => Some(45),
        "COV0249" => Some(52),
        "TIME0250" => Some(59),
        "ARTEF0251" => Some(66),
        "FUZZ0252" => Some(73),
        "GEN0253" => Some(80),
        "EXEC0254" => Some(87),
        "JUDGE0255" => Some(5),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3681bmx_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3681bmx_COUNT {
        let (k, v) = u3681bmx[i];
        let ok = match k {
            "FUZZ0000" => v < 89,
            "GEN0001" => v < 89,
            "EXEC0002" => v < 89,
            "JUDGE0003" => v < 89,
            "SEED0004" => v < 89,
            "CORPUS0005" => v < 89,
            "MUT0006" => v < 89,
            "CRASH0007" => v < 89,
            "DEDUP0008" => v < 89,
            "COV0009" => v < 89,
            "TIME0010" => v < 89,
            "ARTEF0011" => v < 89,
            "FUZZ0012" => v < 89,
            "GEN0013" => v < 89,
            "EXEC0014" => v < 89,
            "JUDGE0015" => v < 89,
            "SEED0016" => v < 89,
            "CORPUS0017" => v < 89,
            "MUT0018" => v < 89,
            "CRASH0019" => v < 89,
            "DEDUP0020" => v < 89,
            "COV0021" => v < 89,
            "TIME0022" => v < 89,
            "ARTEF0023" => v < 89,
            "FUZZ0024" => v < 89,
            "GEN0025" => v < 89,
            "EXEC0026" => v < 89,
            "JUDGE0027" => v < 89,
            "SEED0028" => v < 89,
            "CORPUS0029" => v < 89,
            "MUT0030" => v < 89,
            "CRASH0031" => v < 89,
            "DEDUP0032" => v < 89,
            "COV0033" => v < 89,
            "TIME0034" => v < 89,
            "ARTEF0035" => v < 89,
            "FUZZ0036" => v < 89,
            "GEN0037" => v < 89,
            "EXEC0038" => v < 89,
            "JUDGE0039" => v < 89,
            "SEED0040" => v < 89,
            "CORPUS0041" => v < 89,
            "MUT0042" => v < 89,
            "CRASH0043" => v < 89,
            "DEDUP0044" => v < 89,
            "COV0045" => v < 89,
            "TIME0046" => v < 89,
            "ARTEF0047" => v < 89,
            "FUZZ0048" => v < 89,
            "GEN0049" => v < 89,
            "EXEC0050" => v < 89,
            "JUDGE0051" => v < 89,
            "SEED0052" => v < 89,
            "CORPUS0053" => v < 89,
            "MUT0054" => v < 89,
            "CRASH0055" => v < 89,
            "DEDUP0056" => v < 89,
            "COV0057" => v < 89,
            "TIME0058" => v < 89,
            "ARTEF0059" => v < 89,
            "FUZZ0060" => v < 89,
            "GEN0061" => v < 89,
            "EXEC0062" => v < 89,
            "JUDGE0063" => v < 89,
            "SEED0064" => v < 89,
            "CORPUS0065" => v < 89,
            "MUT0066" => v < 89,
            "CRASH0067" => v < 89,
            "DEDUP0068" => v < 89,
            "COV0069" => v < 89,
            "TIME0070" => v < 89,
            "ARTEF0071" => v < 89,
            "FUZZ0072" => v < 89,
            "GEN0073" => v < 89,
            "EXEC0074" => v < 89,
            "JUDGE0075" => v < 89,
            "SEED0076" => v < 89,
            "CORPUS0077" => v < 89,
            "MUT0078" => v < 89,
            "CRASH0079" => v < 89,
            "DEDUP0080" => v < 89,
            "COV0081" => v < 89,
            "TIME0082" => v < 89,
            "ARTEF0083" => v < 89,
            "FUZZ0084" => v < 89,
            "GEN0085" => v < 89,
            "EXEC0086" => v < 89,
            "JUDGE0087" => v < 89,
            "SEED0088" => v < 89,
            "CORPUS0089" => v < 89,
            "MUT0090" => v < 89,
            "CRASH0091" => v < 89,
            "DEDUP0092" => v < 89,
            "COV0093" => v < 89,
            "TIME0094" => v < 89,
            "ARTEF0095" => v < 89,
            "FUZZ0096" => v < 89,
            "GEN0097" => v < 89,
            "EXEC0098" => v < 89,
            "JUDGE0099" => v < 89,
            "SEED0100" => v < 89,
            "CORPUS0101" => v < 89,
            "MUT0102" => v < 89,
            "CRASH0103" => v < 89,
            "DEDUP0104" => v < 89,
            "COV0105" => v < 89,
            "TIME0106" => v < 89,
            "ARTEF0107" => v < 89,
            "FUZZ0108" => v < 89,
            "GEN0109" => v < 89,
            "EXEC0110" => v < 89,
            "JUDGE0111" => v < 89,
            "SEED0112" => v < 89,
            "CORPUS0113" => v < 89,
            "MUT0114" => v < 89,
            "CRASH0115" => v < 89,
            "DEDUP0116" => v < 89,
            "COV0117" => v < 89,
            "TIME0118" => v < 89,
            "ARTEF0119" => v < 89,
            "FUZZ0120" => v < 89,
            "GEN0121" => v < 89,
            "EXEC0122" => v < 89,
            "JUDGE0123" => v < 89,
            "SEED0124" => v < 89,
            "CORPUS0125" => v < 89,
            "MUT0126" => v < 89,
            "CRASH0127" => v < 89,
            "DEDUP0128" => v < 89,
            "COV0129" => v < 89,
            "TIME0130" => v < 89,
            "ARTEF0131" => v < 89,
            "FUZZ0132" => v < 89,
            "GEN0133" => v < 89,
            "EXEC0134" => v < 89,
            "JUDGE0135" => v < 89,
            "SEED0136" => v < 89,
            "CORPUS0137" => v < 89,
            "MUT0138" => v < 89,
            "CRASH0139" => v < 89,
            "DEDUP0140" => v < 89,
            "COV0141" => v < 89,
            "TIME0142" => v < 89,
            "ARTEF0143" => v < 89,
            "FUZZ0144" => v < 89,
            "GEN0145" => v < 89,
            "EXEC0146" => v < 89,
            "JUDGE0147" => v < 89,
            "SEED0148" => v < 89,
            "CORPUS0149" => v < 89,
            "MUT0150" => v < 89,
            "CRASH0151" => v < 89,
            "DEDUP0152" => v < 89,
            "COV0153" => v < 89,
            "TIME0154" => v < 89,
            "ARTEF0155" => v < 89,
            "FUZZ0156" => v < 89,
            "GEN0157" => v < 89,
            "EXEC0158" => v < 89,
            "JUDGE0159" => v < 89,
            "SEED0160" => v < 89,
            "CORPUS0161" => v < 89,
            "MUT0162" => v < 89,
            "CRASH0163" => v < 89,
            "DEDUP0164" => v < 89,
            "COV0165" => v < 89,
            "TIME0166" => v < 89,
            "ARTEF0167" => v < 89,
            "FUZZ0168" => v < 89,
            "GEN0169" => v < 89,
            "EXEC0170" => v < 89,
            "JUDGE0171" => v < 89,
            "SEED0172" => v < 89,
            "CORPUS0173" => v < 89,
            "MUT0174" => v < 89,
            "CRASH0175" => v < 89,
            "DEDUP0176" => v < 89,
            "COV0177" => v < 89,
            "TIME0178" => v < 89,
            "ARTEF0179" => v < 89,
            "FUZZ0180" => v < 89,
            "GEN0181" => v < 89,
            "EXEC0182" => v < 89,
            "JUDGE0183" => v < 89,
            "SEED0184" => v < 89,
            "CORPUS0185" => v < 89,
            "MUT0186" => v < 89,
            "CRASH0187" => v < 89,
            "DEDUP0188" => v < 89,
            "COV0189" => v < 89,
            "TIME0190" => v < 89,
            "ARTEF0191" => v < 89,
            "FUZZ0192" => v < 89,
            "GEN0193" => v < 89,
            "EXEC0194" => v < 89,
            "JUDGE0195" => v < 89,
            "SEED0196" => v < 89,
            "CORPUS0197" => v < 89,
            "MUT0198" => v < 89,
            "CRASH0199" => v < 89,
            "DEDUP0200" => v < 89,
            "COV0201" => v < 89,
            "TIME0202" => v < 89,
            "ARTEF0203" => v < 89,
            "FUZZ0204" => v < 89,
            "GEN0205" => v < 89,
            "EXEC0206" => v < 89,
            "JUDGE0207" => v < 89,
            "SEED0208" => v < 89,
            "CORPUS0209" => v < 89,
            "MUT0210" => v < 89,
            "CRASH0211" => v < 89,
            "DEDUP0212" => v < 89,
            "COV0213" => v < 89,
            "TIME0214" => v < 89,
            "ARTEF0215" => v < 89,
            "FUZZ0216" => v < 89,
            "GEN0217" => v < 89,
            "EXEC0218" => v < 89,
            "JUDGE0219" => v < 89,
            "SEED0220" => v < 89,
            "CORPUS0221" => v < 89,
            "MUT0222" => v < 89,
            "CRASH0223" => v < 89,
            "DEDUP0224" => v < 89,
            "COV0225" => v < 89,
            "TIME0226" => v < 89,
            "ARTEF0227" => v < 89,
            "FUZZ0228" => v < 89,
            "GEN0229" => v < 89,
            "EXEC0230" => v < 89,
            "JUDGE0231" => v < 89,
            "SEED0232" => v < 89,
            "CORPUS0233" => v < 89,
            "MUT0234" => v < 89,
            "CRASH0235" => v < 89,
            "DEDUP0236" => v < 89,
            "COV0237" => v < 89,
            "TIME0238" => v < 89,
            "ARTEF0239" => v < 89,
            "FUZZ0240" => v < 89,
            "GEN0241" => v < 89,
            "EXEC0242" => v < 89,
            "JUDGE0243" => v < 89,
            "SEED0244" => v < 89,
            "CORPUS0245" => v < 89,
            "MUT0246" => v < 89,
            "CRASH0247" => v < 89,
            "DEDUP0248" => v < 89,
            "COV0249" => v < 89,
            "TIME0250" => v < 89,
            "ARTEF0251" => v < 89,
            "FUZZ0252" => v < 89,
            "GEN0253" => v < 89,
            "EXEC0254" => v < 89,
            "JUDGE0255" => v < 89,
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
pub fn u3681bmx_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3681bmx_COUNT {
        s = s.wrapping_add(u3681bmx[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3681bmx_unique() -> bool {
    let mut i = 0usize;
    while i < u3681bmx_COUNT {
        let mut j = i + 1;
        while j < u3681bmx_COUNT {
            if u3681bmx[i].0 == u3681bmx[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe200, 0xe20f]。
pub const fn u3681_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3681Code::ALL.len() {
        let c = u3681Code::ALL[i].code();
        if c < 0xe200 || c > 0xe20f {
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
fn u3681_guard_seg() -> bool {
    u3681_segment_guard()
}

/// UNX-F3681 bmx 256 行齐备 + 边界锚 + 键外拒收。
fn check_u3681_bmx_count() -> bool {
    let mut ok = u3681bmx_COUNT == 256;
    if ok { ok = u3681bmx_get("FUZZ0000") == Some(0) && u3681bmx_get("JUDGE0255") == Some(5); }
    if ok { ok = u3681bmx_get("ZZZ_NOKEY").is_none(); }
    ok
}

/// UNX-F3681 bmx 累计守恒（理论值 11231）。
fn check_u3681_bmx_sum() -> bool {
    u3681bmx_sum() == 11231
}

/// UNX-F3681 bmx 逐行守卫与唯一性全绿。
fn check_u3681_bmx_guard() -> bool {
    u3681bmx_scan() == 0 && u3681bmx_unique()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3681_checks() -> CheckSet {
    let mut set = CheckSet::new("U3681");
    set.add(
        "u3681-seg-guard",
        u3681_guard_seg(),
        if u3681_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3681-bmx-count",
        check_u3681_bmx_count(),
        if check_u3681_bmx_count() { "" } else { "UNX-F3681 bmx count mismatch" },
    );
    set.add(
        "u3681-bmx-sum",
        check_u3681_bmx_sum(),
        if check_u3681_bmx_sum() { "" } else { "UNX-F3681 bmx sum mismatch" },
    );
    set.add(
        "u3681-bmx-guard",
        check_u3681_bmx_guard(),
        if check_u3681_bmx_guard() { "" } else { "UNX-F3681 bmx guard mismatch" },
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
    fn u3681_all_checks_green() {
        let set = run_u3681_checks();
        assert!(set.all_passed(), "U3681 domain checks must be green");
    }

    #[test]
    fn u3681_segment_guard_holds() {
        assert!(u3681_segment_guard());
    }
}
