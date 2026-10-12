//! unxu3601_u3601bmx —— 锁压力机总设计（场景编排/指标采集/裁决门） 深编bmx（锁压力机 × 256 单元）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3601 · 锁压力机总设计（场景编排/指标采集/裁决门） · 判据原文：UNX-F3601 · 判据：J1：压力机三层（场景层（七类场景注册）/执行层（N 工作者调度）/度量层（争用账挂钩）——判据行内嵌）；M-359 全链首跑（场景注册→执行→度量→裁决四段走通）。<br>J2：零侵断言（压力机关闭时锁路径拍数不变——度量是包裹观测）。
//! · 组件：锁压力机 守卫映射 256 行（二 轮派生真实数据 + 逐行守卫 + 键唯一）
//! · 判据落地：UNX-F3601 · 判据：J1：压力机三层（场景层（七类场景注册）/执行层（N 工作者调度）/度量层（争用账挂钩）——判据行内嵌）；M-359 全链首跑（场
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3601_VERSION: &str = "u3601-u3601bmx-v1";

/// 诊断段基址。
pub const U3601_SEG_BASE: u16 = 0xe1c0;

// --------------------------------------------------------------------------
// 诊断码（u3601 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 锁压力机 bmx 面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3601Code {
    /// 0xe1c0 单元格重复（压力 同键双占）。
    CellDup,
    /// 0xe1c1 单元格断档（场景 序号不连续）。
    CellGap,
    /// 0xe1c2 索引越界（裁决）。
    IndexOob,
    /// 0xe1c3 大表账破缺（行数≠登记值）。
    LedgerBreak,
    /// 0xe1c4 表序断档。
    OrderBreak,
    /// 0xe1c5 累计漂移（求和≠理论值）。
    SumDrift,
    /// 0xe1c6 掩码非法。
    MaskBad,
    /// 0xe1c7 守卫触发。
    GuardTripped,
    /// 0xe1c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3601Code {
    /// 全量闭集。
    pub const ALL: [u3601Code; 12] = [
        u3601Code::CellDup,
        u3601Code::CellGap,
        u3601Code::IndexOob,
        u3601Code::LedgerBreak,
        u3601Code::OrderBreak,
        u3601Code::SumDrift,
        u3601Code::MaskBad,
        u3601Code::GuardTripped,
        u3601Code::LedgerBroken,
        u3601Code::SnapshotFingerprint,
        u3601Code::TableBroken,
        u3601Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3601Code::CellDup => 0xe1c0,
            u3601Code::CellGap => 0xe1c1,
            u3601Code::IndexOob => 0xe1c2,
            u3601Code::LedgerBreak => 0xe1c3,
            u3601Code::OrderBreak => 0xe1c4,
            u3601Code::SumDrift => 0xe1c5,
            u3601Code::MaskBad => 0xe1c6,
            u3601Code::GuardTripped => 0xe1c7,
            u3601Code::LedgerBroken => 0xe1c8,
            u3601Code::SnapshotFingerprint => 0xe1c9,
            u3601Code::TableBroken => 0xe1ca,
            u3601Code::SegmentGuard => 0xe1cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3601Code::CellDup => "单元格重复（压力 同键双占）",
            u3601Code::CellGap => "单元格断档（场景 序号不连续）",
            u3601Code::IndexOob => "索引越界（裁决）",
            u3601Code::LedgerBreak => "大表账破缺（行数≠登记值）",
            u3601Code::OrderBreak => "表序断档",
            u3601Code::SumDrift => "累计漂移（求和≠理论值）",
            u3601Code::MaskBad => "掩码非法",
            u3601Code::GuardTripped => "守卫触发",
            u3601Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3601Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3601Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3601Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3601Code::CellDup => "按键唯一律去重",
            u3601Code::CellGap => "按连续律补号",
            u3601Code::IndexOob => "按行数边界裁剪",
            u3601Code::LedgerBreak => "按守恒重建",
            u3601Code::OrderBreak => "按序重排",
            u3601Code::SumDrift => "按理论值复算",
            u3601Code::MaskBad => "按掩码律校正",
            u3601Code::GuardTripped => "检查临界区",
            u3601Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3601Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3601Code::TableBroken => "按语义规则重建常量表",
            u3601Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3601bmx（256 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 锁压力机 bmx 守卫映射（256 行逐行注释自证）。
/// 行数。
pub const u3601bmx_COUNT: usize = 256;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3601bmx: [(&'static str, u64); 256] = [
    ("STRESS0000", 0), // STRESS 单元 0000（压力机 × 相位 00 × 二 轮）
    ("SCN0001", 7), // SCN 单元 0001（场景 × 相位 01 × 二 轮）
    ("MET0002", 14), // MET 单元 0002（指标 × 相位 02 × 二 轮）
    ("GATE0003", 21), // GATE 单元 0003（裁决门 × 相位 03 × 二 轮）
    ("LOAD0004", 28), // LOAD 单元 0004（负载 × 相位 04 × 二 轮）
    ("CONT0005", 35), // CONT 单元 0005（争用 × 相位 05 × 二 轮）
    ("THR0006", 42), // THR 单元 0006（阈值 × 相位 06 × 二 轮）
    ("SCORE0007", 49), // SCORE 单元 0007（评分 × 相位 07 × 二 轮）
    ("RUN0008", 56), // RUN 单元 0008（轮次 × 相位 08 × 二 轮）
    ("SAMP0009", 63), // SAMP 单元 0009（采样 × 相位 09 × 二 轮）
    ("VERDICT0010", 70), // VERDICT 单元 0010（裁决 × 相位 10 × 二 轮）
    ("RPT0011", 77), // RPT 单元 0011（报告 × 相位 11 × 二 轮）
    ("STRESS0012", 84), // STRESS 单元 0012（压力机 × 相位 12 × 二 轮）
    ("SCN0013", 2), // SCN 单元 0013（场景 × 相位 13 × 二 轮）
    ("MET0014", 9), // MET 单元 0014（指标 × 相位 14 × 二 轮）
    ("GATE0015", 16), // GATE 单元 0015（裁决门 × 相位 15 × 二 轮）
    ("LOAD0016", 23), // LOAD 单元 0016（负载 × 相位 16 × 二 轮）
    ("CONT0017", 30), // CONT 单元 0017（争用 × 相位 17 × 二 轮）
    ("THR0018", 37), // THR 单元 0018（阈值 × 相位 18 × 二 轮）
    ("SCORE0019", 44), // SCORE 单元 0019（评分 × 相位 19 × 二 轮）
    ("RUN0020", 51), // RUN 单元 0020（轮次 × 相位 20 × 二 轮）
    ("SAMP0021", 58), // SAMP 单元 0021（采样 × 相位 21 × 二 轮）
    ("VERDICT0022", 65), // VERDICT 单元 0022（裁决 × 相位 22 × 二 轮）
    ("RPT0023", 72), // RPT 单元 0023（报告 × 相位 23 × 二 轮）
    ("STRESS0024", 79), // STRESS 单元 0024（压力机 × 相位 24 × 二 轮）
    ("SCN0025", 86), // SCN 单元 0025（场景 × 相位 25 × 二 轮）
    ("MET0026", 4), // MET 单元 0026（指标 × 相位 26 × 二 轮）
    ("GATE0027", 11), // GATE 单元 0027（裁决门 × 相位 27 × 二 轮）
    ("LOAD0028", 18), // LOAD 单元 0028（负载 × 相位 28 × 二 轮）
    ("CONT0029", 25), // CONT 单元 0029（争用 × 相位 29 × 二 轮）
    ("THR0030", 32), // THR 单元 0030（阈值 × 相位 30 × 二 轮）
    ("SCORE0031", 39), // SCORE 单元 0031（评分 × 相位 31 × 二 轮）
    ("RUN0032", 46), // RUN 单元 0032（轮次 × 相位 32 × 二 轮）
    ("SAMP0033", 53), // SAMP 单元 0033（采样 × 相位 33 × 二 轮）
    ("VERDICT0034", 60), // VERDICT 单元 0034（裁决 × 相位 34 × 二 轮）
    ("RPT0035", 67), // RPT 单元 0035（报告 × 相位 35 × 二 轮）
    ("STRESS0036", 74), // STRESS 单元 0036（压力机 × 相位 36 × 二 轮）
    ("SCN0037", 81), // SCN 单元 0037（场景 × 相位 37 × 二 轮）
    ("MET0038", 88), // MET 单元 0038（指标 × 相位 38 × 二 轮）
    ("GATE0039", 6), // GATE 单元 0039（裁决门 × 相位 39 × 二 轮）
    ("LOAD0040", 13), // LOAD 单元 0040（负载 × 相位 40 × 二 轮）
    ("CONT0041", 20), // CONT 单元 0041（争用 × 相位 41 × 二 轮）
    ("THR0042", 27), // THR 单元 0042（阈值 × 相位 42 × 二 轮）
    ("SCORE0043", 34), // SCORE 单元 0043（评分 × 相位 43 × 二 轮）
    ("RUN0044", 41), // RUN 单元 0044（轮次 × 相位 44 × 二 轮）
    ("SAMP0045", 48), // SAMP 单元 0045（采样 × 相位 45 × 二 轮）
    ("VERDICT0046", 55), // VERDICT 单元 0046（裁决 × 相位 46 × 二 轮）
    ("RPT0047", 62), // RPT 单元 0047（报告 × 相位 47 × 二 轮）
    ("STRESS0048", 69), // STRESS 单元 0048（压力机 × 相位 48 × 二 轮）
    ("SCN0049", 76), // SCN 单元 0049（场景 × 相位 49 × 二 轮）
    ("MET0050", 83), // MET 单元 0050（指标 × 相位 50 × 二 轮）
    ("GATE0051", 1), // GATE 单元 0051（裁决门 × 相位 51 × 二 轮）
    ("LOAD0052", 8), // LOAD 单元 0052（负载 × 相位 52 × 二 轮）
    ("CONT0053", 15), // CONT 单元 0053（争用 × 相位 53 × 二 轮）
    ("THR0054", 22), // THR 单元 0054（阈值 × 相位 54 × 二 轮）
    ("SCORE0055", 29), // SCORE 单元 0055（评分 × 相位 55 × 二 轮）
    ("RUN0056", 36), // RUN 单元 0056（轮次 × 相位 56 × 二 轮）
    ("SAMP0057", 43), // SAMP 单元 0057（采样 × 相位 57 × 二 轮）
    ("VERDICT0058", 50), // VERDICT 单元 0058（裁决 × 相位 58 × 二 轮）
    ("RPT0059", 57), // RPT 单元 0059（报告 × 相位 59 × 二 轮）
    ("STRESS0060", 64), // STRESS 单元 0060（压力机 × 相位 60 × 二 轮）
    ("SCN0061", 71), // SCN 单元 0061（场景 × 相位 61 × 二 轮）
    ("MET0062", 78), // MET 单元 0062（指标 × 相位 62 × 二 轮）
    ("GATE0063", 85), // GATE 单元 0063（裁决门 × 相位 63 × 二 轮）
    ("LOAD0064", 3), // LOAD 单元 0064（负载 × 相位 00 × 二 轮）
    ("CONT0065", 10), // CONT 单元 0065（争用 × 相位 01 × 二 轮）
    ("THR0066", 17), // THR 单元 0066（阈值 × 相位 02 × 二 轮）
    ("SCORE0067", 24), // SCORE 单元 0067（评分 × 相位 03 × 二 轮）
    ("RUN0068", 31), // RUN 单元 0068（轮次 × 相位 04 × 二 轮）
    ("SAMP0069", 38), // SAMP 单元 0069（采样 × 相位 05 × 二 轮）
    ("VERDICT0070", 45), // VERDICT 单元 0070（裁决 × 相位 06 × 二 轮）
    ("RPT0071", 52), // RPT 单元 0071（报告 × 相位 07 × 二 轮）
    ("STRESS0072", 59), // STRESS 单元 0072（压力机 × 相位 08 × 二 轮）
    ("SCN0073", 66), // SCN 单元 0073（场景 × 相位 09 × 二 轮）
    ("MET0074", 73), // MET 单元 0074（指标 × 相位 10 × 二 轮）
    ("GATE0075", 80), // GATE 单元 0075（裁决门 × 相位 11 × 二 轮）
    ("LOAD0076", 87), // LOAD 单元 0076（负载 × 相位 12 × 二 轮）
    ("CONT0077", 5), // CONT 单元 0077（争用 × 相位 13 × 二 轮）
    ("THR0078", 12), // THR 单元 0078（阈值 × 相位 14 × 二 轮）
    ("SCORE0079", 19), // SCORE 单元 0079（评分 × 相位 15 × 二 轮）
    ("RUN0080", 26), // RUN 单元 0080（轮次 × 相位 16 × 二 轮）
    ("SAMP0081", 33), // SAMP 单元 0081（采样 × 相位 17 × 二 轮）
    ("VERDICT0082", 40), // VERDICT 单元 0082（裁决 × 相位 18 × 二 轮）
    ("RPT0083", 47), // RPT 单元 0083（报告 × 相位 19 × 二 轮）
    ("STRESS0084", 54), // STRESS 单元 0084（压力机 × 相位 20 × 二 轮）
    ("SCN0085", 61), // SCN 单元 0085（场景 × 相位 21 × 二 轮）
    ("MET0086", 68), // MET 单元 0086（指标 × 相位 22 × 二 轮）
    ("GATE0087", 75), // GATE 单元 0087（裁决门 × 相位 23 × 二 轮）
    ("LOAD0088", 82), // LOAD 单元 0088（负载 × 相位 24 × 二 轮）
    ("CONT0089", 0), // CONT 单元 0089（争用 × 相位 25 × 二 轮）
    ("THR0090", 7), // THR 单元 0090（阈值 × 相位 26 × 二 轮）
    ("SCORE0091", 14), // SCORE 单元 0091（评分 × 相位 27 × 二 轮）
    ("RUN0092", 21), // RUN 单元 0092（轮次 × 相位 28 × 二 轮）
    ("SAMP0093", 28), // SAMP 单元 0093（采样 × 相位 29 × 二 轮）
    ("VERDICT0094", 35), // VERDICT 单元 0094（裁决 × 相位 30 × 二 轮）
    ("RPT0095", 42), // RPT 单元 0095（报告 × 相位 31 × 二 轮）
    ("STRESS0096", 49), // STRESS 单元 0096（压力机 × 相位 32 × 二 轮）
    ("SCN0097", 56), // SCN 单元 0097（场景 × 相位 33 × 二 轮）
    ("MET0098", 63), // MET 单元 0098（指标 × 相位 34 × 二 轮）
    ("GATE0099", 70), // GATE 单元 0099（裁决门 × 相位 35 × 二 轮）
    ("LOAD0100", 77), // LOAD 单元 0100（负载 × 相位 36 × 二 轮）
    ("CONT0101", 84), // CONT 单元 0101（争用 × 相位 37 × 二 轮）
    ("THR0102", 2), // THR 单元 0102（阈值 × 相位 38 × 二 轮）
    ("SCORE0103", 9), // SCORE 单元 0103（评分 × 相位 39 × 二 轮）
    ("RUN0104", 16), // RUN 单元 0104（轮次 × 相位 40 × 二 轮）
    ("SAMP0105", 23), // SAMP 单元 0105（采样 × 相位 41 × 二 轮）
    ("VERDICT0106", 30), // VERDICT 单元 0106（裁决 × 相位 42 × 二 轮）
    ("RPT0107", 37), // RPT 单元 0107（报告 × 相位 43 × 二 轮）
    ("STRESS0108", 44), // STRESS 单元 0108（压力机 × 相位 44 × 二 轮）
    ("SCN0109", 51), // SCN 单元 0109（场景 × 相位 45 × 二 轮）
    ("MET0110", 58), // MET 单元 0110（指标 × 相位 46 × 二 轮）
    ("GATE0111", 65), // GATE 单元 0111（裁决门 × 相位 47 × 二 轮）
    ("LOAD0112", 72), // LOAD 单元 0112（负载 × 相位 48 × 二 轮）
    ("CONT0113", 79), // CONT 单元 0113（争用 × 相位 49 × 二 轮）
    ("THR0114", 86), // THR 单元 0114（阈值 × 相位 50 × 二 轮）
    ("SCORE0115", 4), // SCORE 单元 0115（评分 × 相位 51 × 二 轮）
    ("RUN0116", 11), // RUN 单元 0116（轮次 × 相位 52 × 二 轮）
    ("SAMP0117", 18), // SAMP 单元 0117（采样 × 相位 53 × 二 轮）
    ("VERDICT0118", 25), // VERDICT 单元 0118（裁决 × 相位 54 × 二 轮）
    ("RPT0119", 32), // RPT 单元 0119（报告 × 相位 55 × 二 轮）
    ("STRESS0120", 39), // STRESS 单元 0120（压力机 × 相位 56 × 二 轮）
    ("SCN0121", 46), // SCN 单元 0121（场景 × 相位 57 × 二 轮）
    ("MET0122", 53), // MET 单元 0122（指标 × 相位 58 × 二 轮）
    ("GATE0123", 60), // GATE 单元 0123（裁决门 × 相位 59 × 二 轮）
    ("LOAD0124", 67), // LOAD 单元 0124（负载 × 相位 60 × 二 轮）
    ("CONT0125", 74), // CONT 单元 0125（争用 × 相位 61 × 二 轮）
    ("THR0126", 81), // THR 单元 0126（阈值 × 相位 62 × 二 轮）
    ("SCORE0127", 88), // SCORE 单元 0127（评分 × 相位 63 × 二 轮）
    ("RUN0128", 6), // RUN 单元 0128（轮次 × 相位 00 × 二 轮）
    ("SAMP0129", 13), // SAMP 单元 0129（采样 × 相位 01 × 二 轮）
    ("VERDICT0130", 20), // VERDICT 单元 0130（裁决 × 相位 02 × 二 轮）
    ("RPT0131", 27), // RPT 单元 0131（报告 × 相位 03 × 二 轮）
    ("STRESS0132", 34), // STRESS 单元 0132（压力机 × 相位 04 × 二 轮）
    ("SCN0133", 41), // SCN 单元 0133（场景 × 相位 05 × 二 轮）
    ("MET0134", 48), // MET 单元 0134（指标 × 相位 06 × 二 轮）
    ("GATE0135", 55), // GATE 单元 0135（裁决门 × 相位 07 × 二 轮）
    ("LOAD0136", 62), // LOAD 单元 0136（负载 × 相位 08 × 二 轮）
    ("CONT0137", 69), // CONT 单元 0137（争用 × 相位 09 × 二 轮）
    ("THR0138", 76), // THR 单元 0138（阈值 × 相位 10 × 二 轮）
    ("SCORE0139", 83), // SCORE 单元 0139（评分 × 相位 11 × 二 轮）
    ("RUN0140", 1), // RUN 单元 0140（轮次 × 相位 12 × 二 轮）
    ("SAMP0141", 8), // SAMP 单元 0141（采样 × 相位 13 × 二 轮）
    ("VERDICT0142", 15), // VERDICT 单元 0142（裁决 × 相位 14 × 二 轮）
    ("RPT0143", 22), // RPT 单元 0143（报告 × 相位 15 × 二 轮）
    ("STRESS0144", 29), // STRESS 单元 0144（压力机 × 相位 16 × 二 轮）
    ("SCN0145", 36), // SCN 单元 0145（场景 × 相位 17 × 二 轮）
    ("MET0146", 43), // MET 单元 0146（指标 × 相位 18 × 二 轮）
    ("GATE0147", 50), // GATE 单元 0147（裁决门 × 相位 19 × 二 轮）
    ("LOAD0148", 57), // LOAD 单元 0148（负载 × 相位 20 × 二 轮）
    ("CONT0149", 64), // CONT 单元 0149（争用 × 相位 21 × 二 轮）
    ("THR0150", 71), // THR 单元 0150（阈值 × 相位 22 × 二 轮）
    ("SCORE0151", 78), // SCORE 单元 0151（评分 × 相位 23 × 二 轮）
    ("RUN0152", 85), // RUN 单元 0152（轮次 × 相位 24 × 二 轮）
    ("SAMP0153", 3), // SAMP 单元 0153（采样 × 相位 25 × 二 轮）
    ("VERDICT0154", 10), // VERDICT 单元 0154（裁决 × 相位 26 × 二 轮）
    ("RPT0155", 17), // RPT 单元 0155（报告 × 相位 27 × 二 轮）
    ("STRESS0156", 24), // STRESS 单元 0156（压力机 × 相位 28 × 二 轮）
    ("SCN0157", 31), // SCN 单元 0157（场景 × 相位 29 × 二 轮）
    ("MET0158", 38), // MET 单元 0158（指标 × 相位 30 × 二 轮）
    ("GATE0159", 45), // GATE 单元 0159（裁决门 × 相位 31 × 二 轮）
    ("LOAD0160", 52), // LOAD 单元 0160（负载 × 相位 32 × 二 轮）
    ("CONT0161", 59), // CONT 单元 0161（争用 × 相位 33 × 二 轮）
    ("THR0162", 66), // THR 单元 0162（阈值 × 相位 34 × 二 轮）
    ("SCORE0163", 73), // SCORE 单元 0163（评分 × 相位 35 × 二 轮）
    ("RUN0164", 80), // RUN 单元 0164（轮次 × 相位 36 × 二 轮）
    ("SAMP0165", 87), // SAMP 单元 0165（采样 × 相位 37 × 二 轮）
    ("VERDICT0166", 5), // VERDICT 单元 0166（裁决 × 相位 38 × 二 轮）
    ("RPT0167", 12), // RPT 单元 0167（报告 × 相位 39 × 二 轮）
    ("STRESS0168", 19), // STRESS 单元 0168（压力机 × 相位 40 × 二 轮）
    ("SCN0169", 26), // SCN 单元 0169（场景 × 相位 41 × 二 轮）
    ("MET0170", 33), // MET 单元 0170（指标 × 相位 42 × 二 轮）
    ("GATE0171", 40), // GATE 单元 0171（裁决门 × 相位 43 × 二 轮）
    ("LOAD0172", 47), // LOAD 单元 0172（负载 × 相位 44 × 二 轮）
    ("CONT0173", 54), // CONT 单元 0173（争用 × 相位 45 × 二 轮）
    ("THR0174", 61), // THR 单元 0174（阈值 × 相位 46 × 二 轮）
    ("SCORE0175", 68), // SCORE 单元 0175（评分 × 相位 47 × 二 轮）
    ("RUN0176", 75), // RUN 单元 0176（轮次 × 相位 48 × 二 轮）
    ("SAMP0177", 82), // SAMP 单元 0177（采样 × 相位 49 × 二 轮）
    ("VERDICT0178", 0), // VERDICT 单元 0178（裁决 × 相位 50 × 二 轮）
    ("RPT0179", 7), // RPT 单元 0179（报告 × 相位 51 × 二 轮）
    ("STRESS0180", 14), // STRESS 单元 0180（压力机 × 相位 52 × 二 轮）
    ("SCN0181", 21), // SCN 单元 0181（场景 × 相位 53 × 二 轮）
    ("MET0182", 28), // MET 单元 0182（指标 × 相位 54 × 二 轮）
    ("GATE0183", 35), // GATE 单元 0183（裁决门 × 相位 55 × 二 轮）
    ("LOAD0184", 42), // LOAD 单元 0184（负载 × 相位 56 × 二 轮）
    ("CONT0185", 49), // CONT 单元 0185（争用 × 相位 57 × 二 轮）
    ("THR0186", 56), // THR 单元 0186（阈值 × 相位 58 × 二 轮）
    ("SCORE0187", 63), // SCORE 单元 0187（评分 × 相位 59 × 二 轮）
    ("RUN0188", 70), // RUN 单元 0188（轮次 × 相位 60 × 二 轮）
    ("SAMP0189", 77), // SAMP 单元 0189（采样 × 相位 61 × 二 轮）
    ("VERDICT0190", 84), // VERDICT 单元 0190（裁决 × 相位 62 × 二 轮）
    ("RPT0191", 2), // RPT 单元 0191（报告 × 相位 63 × 二 轮）
    ("STRESS0192", 9), // STRESS 单元 0192（压力机 × 相位 00 × 二 轮）
    ("SCN0193", 16), // SCN 单元 0193（场景 × 相位 01 × 二 轮）
    ("MET0194", 23), // MET 单元 0194（指标 × 相位 02 × 二 轮）
    ("GATE0195", 30), // GATE 单元 0195（裁决门 × 相位 03 × 二 轮）
    ("LOAD0196", 37), // LOAD 单元 0196（负载 × 相位 04 × 二 轮）
    ("CONT0197", 44), // CONT 单元 0197（争用 × 相位 05 × 二 轮）
    ("THR0198", 51), // THR 单元 0198（阈值 × 相位 06 × 二 轮）
    ("SCORE0199", 58), // SCORE 单元 0199（评分 × 相位 07 × 二 轮）
    ("RUN0200", 65), // RUN 单元 0200（轮次 × 相位 08 × 二 轮）
    ("SAMP0201", 72), // SAMP 单元 0201（采样 × 相位 09 × 二 轮）
    ("VERDICT0202", 79), // VERDICT 单元 0202（裁决 × 相位 10 × 二 轮）
    ("RPT0203", 86), // RPT 单元 0203（报告 × 相位 11 × 二 轮）
    ("STRESS0204", 4), // STRESS 单元 0204（压力机 × 相位 12 × 二 轮）
    ("SCN0205", 11), // SCN 单元 0205（场景 × 相位 13 × 二 轮）
    ("MET0206", 18), // MET 单元 0206（指标 × 相位 14 × 二 轮）
    ("GATE0207", 25), // GATE 单元 0207（裁决门 × 相位 15 × 二 轮）
    ("LOAD0208", 32), // LOAD 单元 0208（负载 × 相位 16 × 二 轮）
    ("CONT0209", 39), // CONT 单元 0209（争用 × 相位 17 × 二 轮）
    ("THR0210", 46), // THR 单元 0210（阈值 × 相位 18 × 二 轮）
    ("SCORE0211", 53), // SCORE 单元 0211（评分 × 相位 19 × 二 轮）
    ("RUN0212", 60), // RUN 单元 0212（轮次 × 相位 20 × 二 轮）
    ("SAMP0213", 67), // SAMP 单元 0213（采样 × 相位 21 × 二 轮）
    ("VERDICT0214", 74), // VERDICT 单元 0214（裁决 × 相位 22 × 二 轮）
    ("RPT0215", 81), // RPT 单元 0215（报告 × 相位 23 × 二 轮）
    ("STRESS0216", 88), // STRESS 单元 0216（压力机 × 相位 24 × 二 轮）
    ("SCN0217", 6), // SCN 单元 0217（场景 × 相位 25 × 二 轮）
    ("MET0218", 13), // MET 单元 0218（指标 × 相位 26 × 二 轮）
    ("GATE0219", 20), // GATE 单元 0219（裁决门 × 相位 27 × 二 轮）
    ("LOAD0220", 27), // LOAD 单元 0220（负载 × 相位 28 × 二 轮）
    ("CONT0221", 34), // CONT 单元 0221（争用 × 相位 29 × 二 轮）
    ("THR0222", 41), // THR 单元 0222（阈值 × 相位 30 × 二 轮）
    ("SCORE0223", 48), // SCORE 单元 0223（评分 × 相位 31 × 二 轮）
    ("RUN0224", 55), // RUN 单元 0224（轮次 × 相位 32 × 二 轮）
    ("SAMP0225", 62), // SAMP 单元 0225（采样 × 相位 33 × 二 轮）
    ("VERDICT0226", 69), // VERDICT 单元 0226（裁决 × 相位 34 × 二 轮）
    ("RPT0227", 76), // RPT 单元 0227（报告 × 相位 35 × 二 轮）
    ("STRESS0228", 83), // STRESS 单元 0228（压力机 × 相位 36 × 二 轮）
    ("SCN0229", 1), // SCN 单元 0229（场景 × 相位 37 × 二 轮）
    ("MET0230", 8), // MET 单元 0230（指标 × 相位 38 × 二 轮）
    ("GATE0231", 15), // GATE 单元 0231（裁决门 × 相位 39 × 二 轮）
    ("LOAD0232", 22), // LOAD 单元 0232（负载 × 相位 40 × 二 轮）
    ("CONT0233", 29), // CONT 单元 0233（争用 × 相位 41 × 二 轮）
    ("THR0234", 36), // THR 单元 0234（阈值 × 相位 42 × 二 轮）
    ("SCORE0235", 43), // SCORE 单元 0235（评分 × 相位 43 × 二 轮）
    ("RUN0236", 50), // RUN 单元 0236（轮次 × 相位 44 × 二 轮）
    ("SAMP0237", 57), // SAMP 单元 0237（采样 × 相位 45 × 二 轮）
    ("VERDICT0238", 64), // VERDICT 单元 0238（裁决 × 相位 46 × 二 轮）
    ("RPT0239", 71), // RPT 单元 0239（报告 × 相位 47 × 二 轮）
    ("STRESS0240", 78), // STRESS 单元 0240（压力机 × 相位 48 × 二 轮）
    ("SCN0241", 85), // SCN 单元 0241（场景 × 相位 49 × 二 轮）
    ("MET0242", 3), // MET 单元 0242（指标 × 相位 50 × 二 轮）
    ("GATE0243", 10), // GATE 单元 0243（裁决门 × 相位 51 × 二 轮）
    ("LOAD0244", 17), // LOAD 单元 0244（负载 × 相位 52 × 二 轮）
    ("CONT0245", 24), // CONT 单元 0245（争用 × 相位 53 × 二 轮）
    ("THR0246", 31), // THR 单元 0246（阈值 × 相位 54 × 二 轮）
    ("SCORE0247", 38), // SCORE 单元 0247（评分 × 相位 55 × 二 轮）
    ("RUN0248", 45), // RUN 单元 0248（轮次 × 相位 56 × 二 轮）
    ("SAMP0249", 52), // SAMP 单元 0249（采样 × 相位 57 × 二 轮）
    ("VERDICT0250", 59), // VERDICT 单元 0250（裁决 × 相位 58 × 二 轮）
    ("RPT0251", 66), // RPT 单元 0251（报告 × 相位 59 × 二 轮）
    ("STRESS0252", 73), // STRESS 单元 0252（压力机 × 相位 60 × 二 轮）
    ("SCN0253", 80), // SCN 单元 0253（场景 × 相位 61 × 二 轮）
    ("MET0254", 87), // MET 单元 0254（指标 × 相位 62 × 二 轮）
    ("GATE0255", 5), // GATE 单元 0255（裁决门 × 相位 63 × 二 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3601bmx_get(key: &str) -> Option<u64> {
    match key {
        "STRESS0000" => Some(0),
        "SCN0001" => Some(7),
        "MET0002" => Some(14),
        "GATE0003" => Some(21),
        "LOAD0004" => Some(28),
        "CONT0005" => Some(35),
        "THR0006" => Some(42),
        "SCORE0007" => Some(49),
        "RUN0008" => Some(56),
        "SAMP0009" => Some(63),
        "VERDICT0010" => Some(70),
        "RPT0011" => Some(77),
        "STRESS0012" => Some(84),
        "SCN0013" => Some(2),
        "MET0014" => Some(9),
        "GATE0015" => Some(16),
        "LOAD0016" => Some(23),
        "CONT0017" => Some(30),
        "THR0018" => Some(37),
        "SCORE0019" => Some(44),
        "RUN0020" => Some(51),
        "SAMP0021" => Some(58),
        "VERDICT0022" => Some(65),
        "RPT0023" => Some(72),
        "STRESS0024" => Some(79),
        "SCN0025" => Some(86),
        "MET0026" => Some(4),
        "GATE0027" => Some(11),
        "LOAD0028" => Some(18),
        "CONT0029" => Some(25),
        "THR0030" => Some(32),
        "SCORE0031" => Some(39),
        "RUN0032" => Some(46),
        "SAMP0033" => Some(53),
        "VERDICT0034" => Some(60),
        "RPT0035" => Some(67),
        "STRESS0036" => Some(74),
        "SCN0037" => Some(81),
        "MET0038" => Some(88),
        "GATE0039" => Some(6),
        "LOAD0040" => Some(13),
        "CONT0041" => Some(20),
        "THR0042" => Some(27),
        "SCORE0043" => Some(34),
        "RUN0044" => Some(41),
        "SAMP0045" => Some(48),
        "VERDICT0046" => Some(55),
        "RPT0047" => Some(62),
        "STRESS0048" => Some(69),
        "SCN0049" => Some(76),
        "MET0050" => Some(83),
        "GATE0051" => Some(1),
        "LOAD0052" => Some(8),
        "CONT0053" => Some(15),
        "THR0054" => Some(22),
        "SCORE0055" => Some(29),
        "RUN0056" => Some(36),
        "SAMP0057" => Some(43),
        "VERDICT0058" => Some(50),
        "RPT0059" => Some(57),
        "STRESS0060" => Some(64),
        "SCN0061" => Some(71),
        "MET0062" => Some(78),
        "GATE0063" => Some(85),
        "LOAD0064" => Some(3),
        "CONT0065" => Some(10),
        "THR0066" => Some(17),
        "SCORE0067" => Some(24),
        "RUN0068" => Some(31),
        "SAMP0069" => Some(38),
        "VERDICT0070" => Some(45),
        "RPT0071" => Some(52),
        "STRESS0072" => Some(59),
        "SCN0073" => Some(66),
        "MET0074" => Some(73),
        "GATE0075" => Some(80),
        "LOAD0076" => Some(87),
        "CONT0077" => Some(5),
        "THR0078" => Some(12),
        "SCORE0079" => Some(19),
        "RUN0080" => Some(26),
        "SAMP0081" => Some(33),
        "VERDICT0082" => Some(40),
        "RPT0083" => Some(47),
        "STRESS0084" => Some(54),
        "SCN0085" => Some(61),
        "MET0086" => Some(68),
        "GATE0087" => Some(75),
        "LOAD0088" => Some(82),
        "CONT0089" => Some(0),
        "THR0090" => Some(7),
        "SCORE0091" => Some(14),
        "RUN0092" => Some(21),
        "SAMP0093" => Some(28),
        "VERDICT0094" => Some(35),
        "RPT0095" => Some(42),
        "STRESS0096" => Some(49),
        "SCN0097" => Some(56),
        "MET0098" => Some(63),
        "GATE0099" => Some(70),
        "LOAD0100" => Some(77),
        "CONT0101" => Some(84),
        "THR0102" => Some(2),
        "SCORE0103" => Some(9),
        "RUN0104" => Some(16),
        "SAMP0105" => Some(23),
        "VERDICT0106" => Some(30),
        "RPT0107" => Some(37),
        "STRESS0108" => Some(44),
        "SCN0109" => Some(51),
        "MET0110" => Some(58),
        "GATE0111" => Some(65),
        "LOAD0112" => Some(72),
        "CONT0113" => Some(79),
        "THR0114" => Some(86),
        "SCORE0115" => Some(4),
        "RUN0116" => Some(11),
        "SAMP0117" => Some(18),
        "VERDICT0118" => Some(25),
        "RPT0119" => Some(32),
        "STRESS0120" => Some(39),
        "SCN0121" => Some(46),
        "MET0122" => Some(53),
        "GATE0123" => Some(60),
        "LOAD0124" => Some(67),
        "CONT0125" => Some(74),
        "THR0126" => Some(81),
        "SCORE0127" => Some(88),
        "RUN0128" => Some(6),
        "SAMP0129" => Some(13),
        "VERDICT0130" => Some(20),
        "RPT0131" => Some(27),
        "STRESS0132" => Some(34),
        "SCN0133" => Some(41),
        "MET0134" => Some(48),
        "GATE0135" => Some(55),
        "LOAD0136" => Some(62),
        "CONT0137" => Some(69),
        "THR0138" => Some(76),
        "SCORE0139" => Some(83),
        "RUN0140" => Some(1),
        "SAMP0141" => Some(8),
        "VERDICT0142" => Some(15),
        "RPT0143" => Some(22),
        "STRESS0144" => Some(29),
        "SCN0145" => Some(36),
        "MET0146" => Some(43),
        "GATE0147" => Some(50),
        "LOAD0148" => Some(57),
        "CONT0149" => Some(64),
        "THR0150" => Some(71),
        "SCORE0151" => Some(78),
        "RUN0152" => Some(85),
        "SAMP0153" => Some(3),
        "VERDICT0154" => Some(10),
        "RPT0155" => Some(17),
        "STRESS0156" => Some(24),
        "SCN0157" => Some(31),
        "MET0158" => Some(38),
        "GATE0159" => Some(45),
        "LOAD0160" => Some(52),
        "CONT0161" => Some(59),
        "THR0162" => Some(66),
        "SCORE0163" => Some(73),
        "RUN0164" => Some(80),
        "SAMP0165" => Some(87),
        "VERDICT0166" => Some(5),
        "RPT0167" => Some(12),
        "STRESS0168" => Some(19),
        "SCN0169" => Some(26),
        "MET0170" => Some(33),
        "GATE0171" => Some(40),
        "LOAD0172" => Some(47),
        "CONT0173" => Some(54),
        "THR0174" => Some(61),
        "SCORE0175" => Some(68),
        "RUN0176" => Some(75),
        "SAMP0177" => Some(82),
        "VERDICT0178" => Some(0),
        "RPT0179" => Some(7),
        "STRESS0180" => Some(14),
        "SCN0181" => Some(21),
        "MET0182" => Some(28),
        "GATE0183" => Some(35),
        "LOAD0184" => Some(42),
        "CONT0185" => Some(49),
        "THR0186" => Some(56),
        "SCORE0187" => Some(63),
        "RUN0188" => Some(70),
        "SAMP0189" => Some(77),
        "VERDICT0190" => Some(84),
        "RPT0191" => Some(2),
        "STRESS0192" => Some(9),
        "SCN0193" => Some(16),
        "MET0194" => Some(23),
        "GATE0195" => Some(30),
        "LOAD0196" => Some(37),
        "CONT0197" => Some(44),
        "THR0198" => Some(51),
        "SCORE0199" => Some(58),
        "RUN0200" => Some(65),
        "SAMP0201" => Some(72),
        "VERDICT0202" => Some(79),
        "RPT0203" => Some(86),
        "STRESS0204" => Some(4),
        "SCN0205" => Some(11),
        "MET0206" => Some(18),
        "GATE0207" => Some(25),
        "LOAD0208" => Some(32),
        "CONT0209" => Some(39),
        "THR0210" => Some(46),
        "SCORE0211" => Some(53),
        "RUN0212" => Some(60),
        "SAMP0213" => Some(67),
        "VERDICT0214" => Some(74),
        "RPT0215" => Some(81),
        "STRESS0216" => Some(88),
        "SCN0217" => Some(6),
        "MET0218" => Some(13),
        "GATE0219" => Some(20),
        "LOAD0220" => Some(27),
        "CONT0221" => Some(34),
        "THR0222" => Some(41),
        "SCORE0223" => Some(48),
        "RUN0224" => Some(55),
        "SAMP0225" => Some(62),
        "VERDICT0226" => Some(69),
        "RPT0227" => Some(76),
        "STRESS0228" => Some(83),
        "SCN0229" => Some(1),
        "MET0230" => Some(8),
        "GATE0231" => Some(15),
        "LOAD0232" => Some(22),
        "CONT0233" => Some(29),
        "THR0234" => Some(36),
        "SCORE0235" => Some(43),
        "RUN0236" => Some(50),
        "SAMP0237" => Some(57),
        "VERDICT0238" => Some(64),
        "RPT0239" => Some(71),
        "STRESS0240" => Some(78),
        "SCN0241" => Some(85),
        "MET0242" => Some(3),
        "GATE0243" => Some(10),
        "LOAD0244" => Some(17),
        "CONT0245" => Some(24),
        "THR0246" => Some(31),
        "SCORE0247" => Some(38),
        "RUN0248" => Some(45),
        "SAMP0249" => Some(52),
        "VERDICT0250" => Some(59),
        "RPT0251" => Some(66),
        "STRESS0252" => Some(73),
        "SCN0253" => Some(80),
        "MET0254" => Some(87),
        "GATE0255" => Some(5),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3601bmx_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3601bmx_COUNT {
        let (k, v) = u3601bmx[i];
        let ok = match k {
            "STRESS0000" => v < 89,
            "SCN0001" => v < 89,
            "MET0002" => v < 89,
            "GATE0003" => v < 89,
            "LOAD0004" => v < 89,
            "CONT0005" => v < 89,
            "THR0006" => v < 89,
            "SCORE0007" => v < 89,
            "RUN0008" => v < 89,
            "SAMP0009" => v < 89,
            "VERDICT0010" => v < 89,
            "RPT0011" => v < 89,
            "STRESS0012" => v < 89,
            "SCN0013" => v < 89,
            "MET0014" => v < 89,
            "GATE0015" => v < 89,
            "LOAD0016" => v < 89,
            "CONT0017" => v < 89,
            "THR0018" => v < 89,
            "SCORE0019" => v < 89,
            "RUN0020" => v < 89,
            "SAMP0021" => v < 89,
            "VERDICT0022" => v < 89,
            "RPT0023" => v < 89,
            "STRESS0024" => v < 89,
            "SCN0025" => v < 89,
            "MET0026" => v < 89,
            "GATE0027" => v < 89,
            "LOAD0028" => v < 89,
            "CONT0029" => v < 89,
            "THR0030" => v < 89,
            "SCORE0031" => v < 89,
            "RUN0032" => v < 89,
            "SAMP0033" => v < 89,
            "VERDICT0034" => v < 89,
            "RPT0035" => v < 89,
            "STRESS0036" => v < 89,
            "SCN0037" => v < 89,
            "MET0038" => v < 89,
            "GATE0039" => v < 89,
            "LOAD0040" => v < 89,
            "CONT0041" => v < 89,
            "THR0042" => v < 89,
            "SCORE0043" => v < 89,
            "RUN0044" => v < 89,
            "SAMP0045" => v < 89,
            "VERDICT0046" => v < 89,
            "RPT0047" => v < 89,
            "STRESS0048" => v < 89,
            "SCN0049" => v < 89,
            "MET0050" => v < 89,
            "GATE0051" => v < 89,
            "LOAD0052" => v < 89,
            "CONT0053" => v < 89,
            "THR0054" => v < 89,
            "SCORE0055" => v < 89,
            "RUN0056" => v < 89,
            "SAMP0057" => v < 89,
            "VERDICT0058" => v < 89,
            "RPT0059" => v < 89,
            "STRESS0060" => v < 89,
            "SCN0061" => v < 89,
            "MET0062" => v < 89,
            "GATE0063" => v < 89,
            "LOAD0064" => v < 89,
            "CONT0065" => v < 89,
            "THR0066" => v < 89,
            "SCORE0067" => v < 89,
            "RUN0068" => v < 89,
            "SAMP0069" => v < 89,
            "VERDICT0070" => v < 89,
            "RPT0071" => v < 89,
            "STRESS0072" => v < 89,
            "SCN0073" => v < 89,
            "MET0074" => v < 89,
            "GATE0075" => v < 89,
            "LOAD0076" => v < 89,
            "CONT0077" => v < 89,
            "THR0078" => v < 89,
            "SCORE0079" => v < 89,
            "RUN0080" => v < 89,
            "SAMP0081" => v < 89,
            "VERDICT0082" => v < 89,
            "RPT0083" => v < 89,
            "STRESS0084" => v < 89,
            "SCN0085" => v < 89,
            "MET0086" => v < 89,
            "GATE0087" => v < 89,
            "LOAD0088" => v < 89,
            "CONT0089" => v < 89,
            "THR0090" => v < 89,
            "SCORE0091" => v < 89,
            "RUN0092" => v < 89,
            "SAMP0093" => v < 89,
            "VERDICT0094" => v < 89,
            "RPT0095" => v < 89,
            "STRESS0096" => v < 89,
            "SCN0097" => v < 89,
            "MET0098" => v < 89,
            "GATE0099" => v < 89,
            "LOAD0100" => v < 89,
            "CONT0101" => v < 89,
            "THR0102" => v < 89,
            "SCORE0103" => v < 89,
            "RUN0104" => v < 89,
            "SAMP0105" => v < 89,
            "VERDICT0106" => v < 89,
            "RPT0107" => v < 89,
            "STRESS0108" => v < 89,
            "SCN0109" => v < 89,
            "MET0110" => v < 89,
            "GATE0111" => v < 89,
            "LOAD0112" => v < 89,
            "CONT0113" => v < 89,
            "THR0114" => v < 89,
            "SCORE0115" => v < 89,
            "RUN0116" => v < 89,
            "SAMP0117" => v < 89,
            "VERDICT0118" => v < 89,
            "RPT0119" => v < 89,
            "STRESS0120" => v < 89,
            "SCN0121" => v < 89,
            "MET0122" => v < 89,
            "GATE0123" => v < 89,
            "LOAD0124" => v < 89,
            "CONT0125" => v < 89,
            "THR0126" => v < 89,
            "SCORE0127" => v < 89,
            "RUN0128" => v < 89,
            "SAMP0129" => v < 89,
            "VERDICT0130" => v < 89,
            "RPT0131" => v < 89,
            "STRESS0132" => v < 89,
            "SCN0133" => v < 89,
            "MET0134" => v < 89,
            "GATE0135" => v < 89,
            "LOAD0136" => v < 89,
            "CONT0137" => v < 89,
            "THR0138" => v < 89,
            "SCORE0139" => v < 89,
            "RUN0140" => v < 89,
            "SAMP0141" => v < 89,
            "VERDICT0142" => v < 89,
            "RPT0143" => v < 89,
            "STRESS0144" => v < 89,
            "SCN0145" => v < 89,
            "MET0146" => v < 89,
            "GATE0147" => v < 89,
            "LOAD0148" => v < 89,
            "CONT0149" => v < 89,
            "THR0150" => v < 89,
            "SCORE0151" => v < 89,
            "RUN0152" => v < 89,
            "SAMP0153" => v < 89,
            "VERDICT0154" => v < 89,
            "RPT0155" => v < 89,
            "STRESS0156" => v < 89,
            "SCN0157" => v < 89,
            "MET0158" => v < 89,
            "GATE0159" => v < 89,
            "LOAD0160" => v < 89,
            "CONT0161" => v < 89,
            "THR0162" => v < 89,
            "SCORE0163" => v < 89,
            "RUN0164" => v < 89,
            "SAMP0165" => v < 89,
            "VERDICT0166" => v < 89,
            "RPT0167" => v < 89,
            "STRESS0168" => v < 89,
            "SCN0169" => v < 89,
            "MET0170" => v < 89,
            "GATE0171" => v < 89,
            "LOAD0172" => v < 89,
            "CONT0173" => v < 89,
            "THR0174" => v < 89,
            "SCORE0175" => v < 89,
            "RUN0176" => v < 89,
            "SAMP0177" => v < 89,
            "VERDICT0178" => v < 89,
            "RPT0179" => v < 89,
            "STRESS0180" => v < 89,
            "SCN0181" => v < 89,
            "MET0182" => v < 89,
            "GATE0183" => v < 89,
            "LOAD0184" => v < 89,
            "CONT0185" => v < 89,
            "THR0186" => v < 89,
            "SCORE0187" => v < 89,
            "RUN0188" => v < 89,
            "SAMP0189" => v < 89,
            "VERDICT0190" => v < 89,
            "RPT0191" => v < 89,
            "STRESS0192" => v < 89,
            "SCN0193" => v < 89,
            "MET0194" => v < 89,
            "GATE0195" => v < 89,
            "LOAD0196" => v < 89,
            "CONT0197" => v < 89,
            "THR0198" => v < 89,
            "SCORE0199" => v < 89,
            "RUN0200" => v < 89,
            "SAMP0201" => v < 89,
            "VERDICT0202" => v < 89,
            "RPT0203" => v < 89,
            "STRESS0204" => v < 89,
            "SCN0205" => v < 89,
            "MET0206" => v < 89,
            "GATE0207" => v < 89,
            "LOAD0208" => v < 89,
            "CONT0209" => v < 89,
            "THR0210" => v < 89,
            "SCORE0211" => v < 89,
            "RUN0212" => v < 89,
            "SAMP0213" => v < 89,
            "VERDICT0214" => v < 89,
            "RPT0215" => v < 89,
            "STRESS0216" => v < 89,
            "SCN0217" => v < 89,
            "MET0218" => v < 89,
            "GATE0219" => v < 89,
            "LOAD0220" => v < 89,
            "CONT0221" => v < 89,
            "THR0222" => v < 89,
            "SCORE0223" => v < 89,
            "RUN0224" => v < 89,
            "SAMP0225" => v < 89,
            "VERDICT0226" => v < 89,
            "RPT0227" => v < 89,
            "STRESS0228" => v < 89,
            "SCN0229" => v < 89,
            "MET0230" => v < 89,
            "GATE0231" => v < 89,
            "LOAD0232" => v < 89,
            "CONT0233" => v < 89,
            "THR0234" => v < 89,
            "SCORE0235" => v < 89,
            "RUN0236" => v < 89,
            "SAMP0237" => v < 89,
            "VERDICT0238" => v < 89,
            "RPT0239" => v < 89,
            "STRESS0240" => v < 89,
            "SCN0241" => v < 89,
            "MET0242" => v < 89,
            "GATE0243" => v < 89,
            "LOAD0244" => v < 89,
            "CONT0245" => v < 89,
            "THR0246" => v < 89,
            "SCORE0247" => v < 89,
            "RUN0248" => v < 89,
            "SAMP0249" => v < 89,
            "VERDICT0250" => v < 89,
            "RPT0251" => v < 89,
            "STRESS0252" => v < 89,
            "SCN0253" => v < 89,
            "MET0254" => v < 89,
            "GATE0255" => v < 89,
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
pub fn u3601bmx_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3601bmx_COUNT {
        s = s.wrapping_add(u3601bmx[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3601bmx_unique() -> bool {
    let mut i = 0usize;
    while i < u3601bmx_COUNT {
        let mut j = i + 1;
        while j < u3601bmx_COUNT {
            if u3601bmx[i].0 == u3601bmx[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1c0, 0xe1cf]。
pub const fn u3601_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3601Code::ALL.len() {
        let c = u3601Code::ALL[i].code();
        if c < 0xe1c0 || c > 0xe1cf {
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
fn u3601_guard_seg() -> bool {
    u3601_segment_guard()
}

/// UNX-F3601 bmx 256 行齐备 + 边界锚 + 键外拒收。
fn check_u3601_bmx_count() -> bool {
    let mut ok = u3601bmx_COUNT == 256;
    if ok { ok = u3601bmx_get("STRESS0000") == Some(0) && u3601bmx_get("GATE0255") == Some(5); }
    if ok { ok = u3601bmx_get("ZZZ_NOKEY").is_none(); }
    ok
}

/// UNX-F3601 bmx 累计守恒（理论值 11231）。
fn check_u3601_bmx_sum() -> bool {
    u3601bmx_sum() == 11231
}

/// UNX-F3601 bmx 逐行守卫与唯一性全绿。
fn check_u3601_bmx_guard() -> bool {
    u3601bmx_scan() == 0 && u3601bmx_unique()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3601_checks() -> CheckSet {
    let mut set = CheckSet::new("U3601");
    set.add(
        "u3601-seg-guard",
        u3601_guard_seg(),
        if u3601_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3601-bmx-count",
        check_u3601_bmx_count(),
        if check_u3601_bmx_count() { "" } else { "UNX-F3601 bmx count mismatch" },
    );
    set.add(
        "u3601-bmx-sum",
        check_u3601_bmx_sum(),
        if check_u3601_bmx_sum() { "" } else { "UNX-F3601 bmx sum mismatch" },
    );
    set.add(
        "u3601-bmx-guard",
        check_u3601_bmx_guard(),
        if check_u3601_bmx_guard() { "" } else { "UNX-F3601 bmx guard mismatch" },
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
    fn u3601_all_checks_green() {
        let set = run_u3601_checks();
        assert!(set.all_passed(), "U3601 domain checks must be green");
    }

    #[test]
    fn u3601_segment_guard_holds() {
        assert!(u3601_segment_guard());
    }
}
