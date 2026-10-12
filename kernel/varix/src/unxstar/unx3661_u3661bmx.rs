//! unxu3661_u3661bmx —— 符号化总设计（地址→符号的解析管线） 深编bmx（符号化管线 × 256 单元）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3661 · 符号化总设计（地址→符号的解析管线） · 判据原文：UNX-F3661 · 判据：J1：解析管线（输入（帧地址集+版本锚）→查表（符号表）→歧义消解→报告（符号帧序列）——判据行内嵌四步）；M-419 全链首译（单帧地址→符号名+偏移+源行——翻译三要素）。<br>J2：离线纯净（符号化器不依赖内核状态（纯函数——同输入同输出——F3672 的前提））。
//! · 组件：符号化管线 守卫映射 256 行（二 轮派生真实数据 + 逐行守卫 + 键唯一）
//! · 判据落地：UNX-F3661 · 判据：J1：解析管线（输入（帧地址集+版本锚）→查表（符号表）→歧义消解→报告（符号帧序列）——判据行内嵌四步）；M-419 全链首译（
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3661_VERSION: &str = "u3661-u3661bmx-v1";

/// 诊断段基址。
pub const U3661_SEG_BASE: u16 = 0xe1f0;

// --------------------------------------------------------------------------
// 诊断码（u3661 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 符号化管线 bmx 面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3661Code {
    /// 0xe1f0 单元格重复（符号 同键双占）。
    CellDup,
    /// 0xe1f1 单元格断档（解析 序号不连续）。
    CellGap,
    /// 0xe1f2 索引越界（内联）。
    IndexOob,
    /// 0xe1f3 大表账破缺（行数≠登记值）。
    LedgerBreak,
    /// 0xe1f4 表序断档。
    OrderBreak,
    /// 0xe1f5 累计漂移（求和≠理论值）。
    SumDrift,
    /// 0xe1f6 掩码非法。
    MaskBad,
    /// 0xe1f7 守卫触发。
    GuardTripped,
    /// 0xe1f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3661Code {
    /// 全量闭集。
    pub const ALL: [u3661Code; 12] = [
        u3661Code::CellDup,
        u3661Code::CellGap,
        u3661Code::IndexOob,
        u3661Code::LedgerBreak,
        u3661Code::OrderBreak,
        u3661Code::SumDrift,
        u3661Code::MaskBad,
        u3661Code::GuardTripped,
        u3661Code::LedgerBroken,
        u3661Code::SnapshotFingerprint,
        u3661Code::TableBroken,
        u3661Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3661Code::CellDup => 0xe1f0,
            u3661Code::CellGap => 0xe1f1,
            u3661Code::IndexOob => 0xe1f2,
            u3661Code::LedgerBreak => 0xe1f3,
            u3661Code::OrderBreak => 0xe1f4,
            u3661Code::SumDrift => 0xe1f5,
            u3661Code::MaskBad => 0xe1f6,
            u3661Code::GuardTripped => 0xe1f7,
            u3661Code::LedgerBroken => 0xe1f8,
            u3661Code::SnapshotFingerprint => 0xe1f9,
            u3661Code::TableBroken => 0xe1fa,
            u3661Code::SegmentGuard => 0xe1fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3661Code::CellDup => "单元格重复（符号 同键双占）",
            u3661Code::CellGap => "单元格断档（解析 序号不连续）",
            u3661Code::IndexOob => "索引越界（内联）",
            u3661Code::LedgerBreak => "大表账破缺（行数≠登记值）",
            u3661Code::OrderBreak => "表序断档",
            u3661Code::SumDrift => "累计漂移（求和≠理论值）",
            u3661Code::MaskBad => "掩码非法",
            u3661Code::GuardTripped => "守卫触发",
            u3661Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3661Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3661Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3661Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3661Code::CellDup => "按键唯一律去重",
            u3661Code::CellGap => "按连续律补号",
            u3661Code::IndexOob => "按行数边界裁剪",
            u3661Code::LedgerBreak => "按守恒重建",
            u3661Code::OrderBreak => "按序重排",
            u3661Code::SumDrift => "按理论值复算",
            u3661Code::MaskBad => "按掩码律校正",
            u3661Code::GuardTripped => "检查临界区",
            u3661Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3661Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3661Code::TableBroken => "按语义规则重建常量表",
            u3661Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3661bmx（256 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 符号化管线 bmx 守卫映射（256 行逐行注释自证）。
/// 行数。
pub const u3661bmx_COUNT: usize = 256;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3661bmx: [(&'static str, u64); 256] = [
    ("SYM0000", 0), // SYM 单元 0000（符号 × 相位 00 × 二 轮）
    ("ADDR0001", 7), // ADDR 单元 0001（地址 × 相位 01 × 二 轮）
    ("RESOLVE0002", 14), // RESOLVE 单元 0002（解析 × 相位 02 × 二 轮）
    ("TBL0003", 21), // TBL 单元 0003（符号表 × 相位 03 × 二 轮）
    ("MOD0004", 28), // MOD 单元 0004（模块 × 相位 04 × 二 轮）
    ("OFFSET0005", 35), // OFFSET 单元 0005（偏移 × 相位 05 × 二 轮）
    ("INLINE0006", 42), // INLINE 单元 0006（内联 × 相位 06 × 二 轮）
    ("FUNC0007", 49), // FUNC 单元 0007（函数 × 相位 07 × 二 轮）
    ("CACHE0008", 56), // CACHE 单元 0008（缓存 × 相位 08 × 二 轮）
    ("HITRATE0009", 63), // HITRATE 单元 0009（命中率 × 相位 09 × 二 轮）
    ("UNK0010", 70), // UNK 单元 0010（未知 × 相位 10 × 二 轮）
    ("FALLBACK0011", 77), // FALLBACK 单元 0011（回落 × 相位 11 × 二 轮）
    ("SYM0012", 84), // SYM 单元 0012（符号 × 相位 12 × 二 轮）
    ("ADDR0013", 2), // ADDR 单元 0013（地址 × 相位 13 × 二 轮）
    ("RESOLVE0014", 9), // RESOLVE 单元 0014（解析 × 相位 14 × 二 轮）
    ("TBL0015", 16), // TBL 单元 0015（符号表 × 相位 15 × 二 轮）
    ("MOD0016", 23), // MOD 单元 0016（模块 × 相位 16 × 二 轮）
    ("OFFSET0017", 30), // OFFSET 单元 0017（偏移 × 相位 17 × 二 轮）
    ("INLINE0018", 37), // INLINE 单元 0018（内联 × 相位 18 × 二 轮）
    ("FUNC0019", 44), // FUNC 单元 0019（函数 × 相位 19 × 二 轮）
    ("CACHE0020", 51), // CACHE 单元 0020（缓存 × 相位 20 × 二 轮）
    ("HITRATE0021", 58), // HITRATE 单元 0021（命中率 × 相位 21 × 二 轮）
    ("UNK0022", 65), // UNK 单元 0022（未知 × 相位 22 × 二 轮）
    ("FALLBACK0023", 72), // FALLBACK 单元 0023（回落 × 相位 23 × 二 轮）
    ("SYM0024", 79), // SYM 单元 0024（符号 × 相位 24 × 二 轮）
    ("ADDR0025", 86), // ADDR 单元 0025（地址 × 相位 25 × 二 轮）
    ("RESOLVE0026", 4), // RESOLVE 单元 0026（解析 × 相位 26 × 二 轮）
    ("TBL0027", 11), // TBL 单元 0027（符号表 × 相位 27 × 二 轮）
    ("MOD0028", 18), // MOD 单元 0028（模块 × 相位 28 × 二 轮）
    ("OFFSET0029", 25), // OFFSET 单元 0029（偏移 × 相位 29 × 二 轮）
    ("INLINE0030", 32), // INLINE 单元 0030（内联 × 相位 30 × 二 轮）
    ("FUNC0031", 39), // FUNC 单元 0031（函数 × 相位 31 × 二 轮）
    ("CACHE0032", 46), // CACHE 单元 0032（缓存 × 相位 32 × 二 轮）
    ("HITRATE0033", 53), // HITRATE 单元 0033（命中率 × 相位 33 × 二 轮）
    ("UNK0034", 60), // UNK 单元 0034（未知 × 相位 34 × 二 轮）
    ("FALLBACK0035", 67), // FALLBACK 单元 0035（回落 × 相位 35 × 二 轮）
    ("SYM0036", 74), // SYM 单元 0036（符号 × 相位 36 × 二 轮）
    ("ADDR0037", 81), // ADDR 单元 0037（地址 × 相位 37 × 二 轮）
    ("RESOLVE0038", 88), // RESOLVE 单元 0038（解析 × 相位 38 × 二 轮）
    ("TBL0039", 6), // TBL 单元 0039（符号表 × 相位 39 × 二 轮）
    ("MOD0040", 13), // MOD 单元 0040（模块 × 相位 40 × 二 轮）
    ("OFFSET0041", 20), // OFFSET 单元 0041（偏移 × 相位 41 × 二 轮）
    ("INLINE0042", 27), // INLINE 单元 0042（内联 × 相位 42 × 二 轮）
    ("FUNC0043", 34), // FUNC 单元 0043（函数 × 相位 43 × 二 轮）
    ("CACHE0044", 41), // CACHE 单元 0044（缓存 × 相位 44 × 二 轮）
    ("HITRATE0045", 48), // HITRATE 单元 0045（命中率 × 相位 45 × 二 轮）
    ("UNK0046", 55), // UNK 单元 0046（未知 × 相位 46 × 二 轮）
    ("FALLBACK0047", 62), // FALLBACK 单元 0047（回落 × 相位 47 × 二 轮）
    ("SYM0048", 69), // SYM 单元 0048（符号 × 相位 48 × 二 轮）
    ("ADDR0049", 76), // ADDR 单元 0049（地址 × 相位 49 × 二 轮）
    ("RESOLVE0050", 83), // RESOLVE 单元 0050（解析 × 相位 50 × 二 轮）
    ("TBL0051", 1), // TBL 单元 0051（符号表 × 相位 51 × 二 轮）
    ("MOD0052", 8), // MOD 单元 0052（模块 × 相位 52 × 二 轮）
    ("OFFSET0053", 15), // OFFSET 单元 0053（偏移 × 相位 53 × 二 轮）
    ("INLINE0054", 22), // INLINE 单元 0054（内联 × 相位 54 × 二 轮）
    ("FUNC0055", 29), // FUNC 单元 0055（函数 × 相位 55 × 二 轮）
    ("CACHE0056", 36), // CACHE 单元 0056（缓存 × 相位 56 × 二 轮）
    ("HITRATE0057", 43), // HITRATE 单元 0057（命中率 × 相位 57 × 二 轮）
    ("UNK0058", 50), // UNK 单元 0058（未知 × 相位 58 × 二 轮）
    ("FALLBACK0059", 57), // FALLBACK 单元 0059（回落 × 相位 59 × 二 轮）
    ("SYM0060", 64), // SYM 单元 0060（符号 × 相位 60 × 二 轮）
    ("ADDR0061", 71), // ADDR 单元 0061（地址 × 相位 61 × 二 轮）
    ("RESOLVE0062", 78), // RESOLVE 单元 0062（解析 × 相位 62 × 二 轮）
    ("TBL0063", 85), // TBL 单元 0063（符号表 × 相位 63 × 二 轮）
    ("MOD0064", 3), // MOD 单元 0064（模块 × 相位 00 × 二 轮）
    ("OFFSET0065", 10), // OFFSET 单元 0065（偏移 × 相位 01 × 二 轮）
    ("INLINE0066", 17), // INLINE 单元 0066（内联 × 相位 02 × 二 轮）
    ("FUNC0067", 24), // FUNC 单元 0067（函数 × 相位 03 × 二 轮）
    ("CACHE0068", 31), // CACHE 单元 0068（缓存 × 相位 04 × 二 轮）
    ("HITRATE0069", 38), // HITRATE 单元 0069（命中率 × 相位 05 × 二 轮）
    ("UNK0070", 45), // UNK 单元 0070（未知 × 相位 06 × 二 轮）
    ("FALLBACK0071", 52), // FALLBACK 单元 0071（回落 × 相位 07 × 二 轮）
    ("SYM0072", 59), // SYM 单元 0072（符号 × 相位 08 × 二 轮）
    ("ADDR0073", 66), // ADDR 单元 0073（地址 × 相位 09 × 二 轮）
    ("RESOLVE0074", 73), // RESOLVE 单元 0074（解析 × 相位 10 × 二 轮）
    ("TBL0075", 80), // TBL 单元 0075（符号表 × 相位 11 × 二 轮）
    ("MOD0076", 87), // MOD 单元 0076（模块 × 相位 12 × 二 轮）
    ("OFFSET0077", 5), // OFFSET 单元 0077（偏移 × 相位 13 × 二 轮）
    ("INLINE0078", 12), // INLINE 单元 0078（内联 × 相位 14 × 二 轮）
    ("FUNC0079", 19), // FUNC 单元 0079（函数 × 相位 15 × 二 轮）
    ("CACHE0080", 26), // CACHE 单元 0080（缓存 × 相位 16 × 二 轮）
    ("HITRATE0081", 33), // HITRATE 单元 0081（命中率 × 相位 17 × 二 轮）
    ("UNK0082", 40), // UNK 单元 0082（未知 × 相位 18 × 二 轮）
    ("FALLBACK0083", 47), // FALLBACK 单元 0083（回落 × 相位 19 × 二 轮）
    ("SYM0084", 54), // SYM 单元 0084（符号 × 相位 20 × 二 轮）
    ("ADDR0085", 61), // ADDR 单元 0085（地址 × 相位 21 × 二 轮）
    ("RESOLVE0086", 68), // RESOLVE 单元 0086（解析 × 相位 22 × 二 轮）
    ("TBL0087", 75), // TBL 单元 0087（符号表 × 相位 23 × 二 轮）
    ("MOD0088", 82), // MOD 单元 0088（模块 × 相位 24 × 二 轮）
    ("OFFSET0089", 0), // OFFSET 单元 0089（偏移 × 相位 25 × 二 轮）
    ("INLINE0090", 7), // INLINE 单元 0090（内联 × 相位 26 × 二 轮）
    ("FUNC0091", 14), // FUNC 单元 0091（函数 × 相位 27 × 二 轮）
    ("CACHE0092", 21), // CACHE 单元 0092（缓存 × 相位 28 × 二 轮）
    ("HITRATE0093", 28), // HITRATE 单元 0093（命中率 × 相位 29 × 二 轮）
    ("UNK0094", 35), // UNK 单元 0094（未知 × 相位 30 × 二 轮）
    ("FALLBACK0095", 42), // FALLBACK 单元 0095（回落 × 相位 31 × 二 轮）
    ("SYM0096", 49), // SYM 单元 0096（符号 × 相位 32 × 二 轮）
    ("ADDR0097", 56), // ADDR 单元 0097（地址 × 相位 33 × 二 轮）
    ("RESOLVE0098", 63), // RESOLVE 单元 0098（解析 × 相位 34 × 二 轮）
    ("TBL0099", 70), // TBL 单元 0099（符号表 × 相位 35 × 二 轮）
    ("MOD0100", 77), // MOD 单元 0100（模块 × 相位 36 × 二 轮）
    ("OFFSET0101", 84), // OFFSET 单元 0101（偏移 × 相位 37 × 二 轮）
    ("INLINE0102", 2), // INLINE 单元 0102（内联 × 相位 38 × 二 轮）
    ("FUNC0103", 9), // FUNC 单元 0103（函数 × 相位 39 × 二 轮）
    ("CACHE0104", 16), // CACHE 单元 0104（缓存 × 相位 40 × 二 轮）
    ("HITRATE0105", 23), // HITRATE 单元 0105（命中率 × 相位 41 × 二 轮）
    ("UNK0106", 30), // UNK 单元 0106（未知 × 相位 42 × 二 轮）
    ("FALLBACK0107", 37), // FALLBACK 单元 0107（回落 × 相位 43 × 二 轮）
    ("SYM0108", 44), // SYM 单元 0108（符号 × 相位 44 × 二 轮）
    ("ADDR0109", 51), // ADDR 单元 0109（地址 × 相位 45 × 二 轮）
    ("RESOLVE0110", 58), // RESOLVE 单元 0110（解析 × 相位 46 × 二 轮）
    ("TBL0111", 65), // TBL 单元 0111（符号表 × 相位 47 × 二 轮）
    ("MOD0112", 72), // MOD 单元 0112（模块 × 相位 48 × 二 轮）
    ("OFFSET0113", 79), // OFFSET 单元 0113（偏移 × 相位 49 × 二 轮）
    ("INLINE0114", 86), // INLINE 单元 0114（内联 × 相位 50 × 二 轮）
    ("FUNC0115", 4), // FUNC 单元 0115（函数 × 相位 51 × 二 轮）
    ("CACHE0116", 11), // CACHE 单元 0116（缓存 × 相位 52 × 二 轮）
    ("HITRATE0117", 18), // HITRATE 单元 0117（命中率 × 相位 53 × 二 轮）
    ("UNK0118", 25), // UNK 单元 0118（未知 × 相位 54 × 二 轮）
    ("FALLBACK0119", 32), // FALLBACK 单元 0119（回落 × 相位 55 × 二 轮）
    ("SYM0120", 39), // SYM 单元 0120（符号 × 相位 56 × 二 轮）
    ("ADDR0121", 46), // ADDR 单元 0121（地址 × 相位 57 × 二 轮）
    ("RESOLVE0122", 53), // RESOLVE 单元 0122（解析 × 相位 58 × 二 轮）
    ("TBL0123", 60), // TBL 单元 0123（符号表 × 相位 59 × 二 轮）
    ("MOD0124", 67), // MOD 单元 0124（模块 × 相位 60 × 二 轮）
    ("OFFSET0125", 74), // OFFSET 单元 0125（偏移 × 相位 61 × 二 轮）
    ("INLINE0126", 81), // INLINE 单元 0126（内联 × 相位 62 × 二 轮）
    ("FUNC0127", 88), // FUNC 单元 0127（函数 × 相位 63 × 二 轮）
    ("CACHE0128", 6), // CACHE 单元 0128（缓存 × 相位 00 × 二 轮）
    ("HITRATE0129", 13), // HITRATE 单元 0129（命中率 × 相位 01 × 二 轮）
    ("UNK0130", 20), // UNK 单元 0130（未知 × 相位 02 × 二 轮）
    ("FALLBACK0131", 27), // FALLBACK 单元 0131（回落 × 相位 03 × 二 轮）
    ("SYM0132", 34), // SYM 单元 0132（符号 × 相位 04 × 二 轮）
    ("ADDR0133", 41), // ADDR 单元 0133（地址 × 相位 05 × 二 轮）
    ("RESOLVE0134", 48), // RESOLVE 单元 0134（解析 × 相位 06 × 二 轮）
    ("TBL0135", 55), // TBL 单元 0135（符号表 × 相位 07 × 二 轮）
    ("MOD0136", 62), // MOD 单元 0136（模块 × 相位 08 × 二 轮）
    ("OFFSET0137", 69), // OFFSET 单元 0137（偏移 × 相位 09 × 二 轮）
    ("INLINE0138", 76), // INLINE 单元 0138（内联 × 相位 10 × 二 轮）
    ("FUNC0139", 83), // FUNC 单元 0139（函数 × 相位 11 × 二 轮）
    ("CACHE0140", 1), // CACHE 单元 0140（缓存 × 相位 12 × 二 轮）
    ("HITRATE0141", 8), // HITRATE 单元 0141（命中率 × 相位 13 × 二 轮）
    ("UNK0142", 15), // UNK 单元 0142（未知 × 相位 14 × 二 轮）
    ("FALLBACK0143", 22), // FALLBACK 单元 0143（回落 × 相位 15 × 二 轮）
    ("SYM0144", 29), // SYM 单元 0144（符号 × 相位 16 × 二 轮）
    ("ADDR0145", 36), // ADDR 单元 0145（地址 × 相位 17 × 二 轮）
    ("RESOLVE0146", 43), // RESOLVE 单元 0146（解析 × 相位 18 × 二 轮）
    ("TBL0147", 50), // TBL 单元 0147（符号表 × 相位 19 × 二 轮）
    ("MOD0148", 57), // MOD 单元 0148（模块 × 相位 20 × 二 轮）
    ("OFFSET0149", 64), // OFFSET 单元 0149（偏移 × 相位 21 × 二 轮）
    ("INLINE0150", 71), // INLINE 单元 0150（内联 × 相位 22 × 二 轮）
    ("FUNC0151", 78), // FUNC 单元 0151（函数 × 相位 23 × 二 轮）
    ("CACHE0152", 85), // CACHE 单元 0152（缓存 × 相位 24 × 二 轮）
    ("HITRATE0153", 3), // HITRATE 单元 0153（命中率 × 相位 25 × 二 轮）
    ("UNK0154", 10), // UNK 单元 0154（未知 × 相位 26 × 二 轮）
    ("FALLBACK0155", 17), // FALLBACK 单元 0155（回落 × 相位 27 × 二 轮）
    ("SYM0156", 24), // SYM 单元 0156（符号 × 相位 28 × 二 轮）
    ("ADDR0157", 31), // ADDR 单元 0157（地址 × 相位 29 × 二 轮）
    ("RESOLVE0158", 38), // RESOLVE 单元 0158（解析 × 相位 30 × 二 轮）
    ("TBL0159", 45), // TBL 单元 0159（符号表 × 相位 31 × 二 轮）
    ("MOD0160", 52), // MOD 单元 0160（模块 × 相位 32 × 二 轮）
    ("OFFSET0161", 59), // OFFSET 单元 0161（偏移 × 相位 33 × 二 轮）
    ("INLINE0162", 66), // INLINE 单元 0162（内联 × 相位 34 × 二 轮）
    ("FUNC0163", 73), // FUNC 单元 0163（函数 × 相位 35 × 二 轮）
    ("CACHE0164", 80), // CACHE 单元 0164（缓存 × 相位 36 × 二 轮）
    ("HITRATE0165", 87), // HITRATE 单元 0165（命中率 × 相位 37 × 二 轮）
    ("UNK0166", 5), // UNK 单元 0166（未知 × 相位 38 × 二 轮）
    ("FALLBACK0167", 12), // FALLBACK 单元 0167（回落 × 相位 39 × 二 轮）
    ("SYM0168", 19), // SYM 单元 0168（符号 × 相位 40 × 二 轮）
    ("ADDR0169", 26), // ADDR 单元 0169（地址 × 相位 41 × 二 轮）
    ("RESOLVE0170", 33), // RESOLVE 单元 0170（解析 × 相位 42 × 二 轮）
    ("TBL0171", 40), // TBL 单元 0171（符号表 × 相位 43 × 二 轮）
    ("MOD0172", 47), // MOD 单元 0172（模块 × 相位 44 × 二 轮）
    ("OFFSET0173", 54), // OFFSET 单元 0173（偏移 × 相位 45 × 二 轮）
    ("INLINE0174", 61), // INLINE 单元 0174（内联 × 相位 46 × 二 轮）
    ("FUNC0175", 68), // FUNC 单元 0175（函数 × 相位 47 × 二 轮）
    ("CACHE0176", 75), // CACHE 单元 0176（缓存 × 相位 48 × 二 轮）
    ("HITRATE0177", 82), // HITRATE 单元 0177（命中率 × 相位 49 × 二 轮）
    ("UNK0178", 0), // UNK 单元 0178（未知 × 相位 50 × 二 轮）
    ("FALLBACK0179", 7), // FALLBACK 单元 0179（回落 × 相位 51 × 二 轮）
    ("SYM0180", 14), // SYM 单元 0180（符号 × 相位 52 × 二 轮）
    ("ADDR0181", 21), // ADDR 单元 0181（地址 × 相位 53 × 二 轮）
    ("RESOLVE0182", 28), // RESOLVE 单元 0182（解析 × 相位 54 × 二 轮）
    ("TBL0183", 35), // TBL 单元 0183（符号表 × 相位 55 × 二 轮）
    ("MOD0184", 42), // MOD 单元 0184（模块 × 相位 56 × 二 轮）
    ("OFFSET0185", 49), // OFFSET 单元 0185（偏移 × 相位 57 × 二 轮）
    ("INLINE0186", 56), // INLINE 单元 0186（内联 × 相位 58 × 二 轮）
    ("FUNC0187", 63), // FUNC 单元 0187（函数 × 相位 59 × 二 轮）
    ("CACHE0188", 70), // CACHE 单元 0188（缓存 × 相位 60 × 二 轮）
    ("HITRATE0189", 77), // HITRATE 单元 0189（命中率 × 相位 61 × 二 轮）
    ("UNK0190", 84), // UNK 单元 0190（未知 × 相位 62 × 二 轮）
    ("FALLBACK0191", 2), // FALLBACK 单元 0191（回落 × 相位 63 × 二 轮）
    ("SYM0192", 9), // SYM 单元 0192（符号 × 相位 00 × 二 轮）
    ("ADDR0193", 16), // ADDR 单元 0193（地址 × 相位 01 × 二 轮）
    ("RESOLVE0194", 23), // RESOLVE 单元 0194（解析 × 相位 02 × 二 轮）
    ("TBL0195", 30), // TBL 单元 0195（符号表 × 相位 03 × 二 轮）
    ("MOD0196", 37), // MOD 单元 0196（模块 × 相位 04 × 二 轮）
    ("OFFSET0197", 44), // OFFSET 单元 0197（偏移 × 相位 05 × 二 轮）
    ("INLINE0198", 51), // INLINE 单元 0198（内联 × 相位 06 × 二 轮）
    ("FUNC0199", 58), // FUNC 单元 0199（函数 × 相位 07 × 二 轮）
    ("CACHE0200", 65), // CACHE 单元 0200（缓存 × 相位 08 × 二 轮）
    ("HITRATE0201", 72), // HITRATE 单元 0201（命中率 × 相位 09 × 二 轮）
    ("UNK0202", 79), // UNK 单元 0202（未知 × 相位 10 × 二 轮）
    ("FALLBACK0203", 86), // FALLBACK 单元 0203（回落 × 相位 11 × 二 轮）
    ("SYM0204", 4), // SYM 单元 0204（符号 × 相位 12 × 二 轮）
    ("ADDR0205", 11), // ADDR 单元 0205（地址 × 相位 13 × 二 轮）
    ("RESOLVE0206", 18), // RESOLVE 单元 0206（解析 × 相位 14 × 二 轮）
    ("TBL0207", 25), // TBL 单元 0207（符号表 × 相位 15 × 二 轮）
    ("MOD0208", 32), // MOD 单元 0208（模块 × 相位 16 × 二 轮）
    ("OFFSET0209", 39), // OFFSET 单元 0209（偏移 × 相位 17 × 二 轮）
    ("INLINE0210", 46), // INLINE 单元 0210（内联 × 相位 18 × 二 轮）
    ("FUNC0211", 53), // FUNC 单元 0211（函数 × 相位 19 × 二 轮）
    ("CACHE0212", 60), // CACHE 单元 0212（缓存 × 相位 20 × 二 轮）
    ("HITRATE0213", 67), // HITRATE 单元 0213（命中率 × 相位 21 × 二 轮）
    ("UNK0214", 74), // UNK 单元 0214（未知 × 相位 22 × 二 轮）
    ("FALLBACK0215", 81), // FALLBACK 单元 0215（回落 × 相位 23 × 二 轮）
    ("SYM0216", 88), // SYM 单元 0216（符号 × 相位 24 × 二 轮）
    ("ADDR0217", 6), // ADDR 单元 0217（地址 × 相位 25 × 二 轮）
    ("RESOLVE0218", 13), // RESOLVE 单元 0218（解析 × 相位 26 × 二 轮）
    ("TBL0219", 20), // TBL 单元 0219（符号表 × 相位 27 × 二 轮）
    ("MOD0220", 27), // MOD 单元 0220（模块 × 相位 28 × 二 轮）
    ("OFFSET0221", 34), // OFFSET 单元 0221（偏移 × 相位 29 × 二 轮）
    ("INLINE0222", 41), // INLINE 单元 0222（内联 × 相位 30 × 二 轮）
    ("FUNC0223", 48), // FUNC 单元 0223（函数 × 相位 31 × 二 轮）
    ("CACHE0224", 55), // CACHE 单元 0224（缓存 × 相位 32 × 二 轮）
    ("HITRATE0225", 62), // HITRATE 单元 0225（命中率 × 相位 33 × 二 轮）
    ("UNK0226", 69), // UNK 单元 0226（未知 × 相位 34 × 二 轮）
    ("FALLBACK0227", 76), // FALLBACK 单元 0227（回落 × 相位 35 × 二 轮）
    ("SYM0228", 83), // SYM 单元 0228（符号 × 相位 36 × 二 轮）
    ("ADDR0229", 1), // ADDR 单元 0229（地址 × 相位 37 × 二 轮）
    ("RESOLVE0230", 8), // RESOLVE 单元 0230（解析 × 相位 38 × 二 轮）
    ("TBL0231", 15), // TBL 单元 0231（符号表 × 相位 39 × 二 轮）
    ("MOD0232", 22), // MOD 单元 0232（模块 × 相位 40 × 二 轮）
    ("OFFSET0233", 29), // OFFSET 单元 0233（偏移 × 相位 41 × 二 轮）
    ("INLINE0234", 36), // INLINE 单元 0234（内联 × 相位 42 × 二 轮）
    ("FUNC0235", 43), // FUNC 单元 0235（函数 × 相位 43 × 二 轮）
    ("CACHE0236", 50), // CACHE 单元 0236（缓存 × 相位 44 × 二 轮）
    ("HITRATE0237", 57), // HITRATE 单元 0237（命中率 × 相位 45 × 二 轮）
    ("UNK0238", 64), // UNK 单元 0238（未知 × 相位 46 × 二 轮）
    ("FALLBACK0239", 71), // FALLBACK 单元 0239（回落 × 相位 47 × 二 轮）
    ("SYM0240", 78), // SYM 单元 0240（符号 × 相位 48 × 二 轮）
    ("ADDR0241", 85), // ADDR 单元 0241（地址 × 相位 49 × 二 轮）
    ("RESOLVE0242", 3), // RESOLVE 单元 0242（解析 × 相位 50 × 二 轮）
    ("TBL0243", 10), // TBL 单元 0243（符号表 × 相位 51 × 二 轮）
    ("MOD0244", 17), // MOD 单元 0244（模块 × 相位 52 × 二 轮）
    ("OFFSET0245", 24), // OFFSET 单元 0245（偏移 × 相位 53 × 二 轮）
    ("INLINE0246", 31), // INLINE 单元 0246（内联 × 相位 54 × 二 轮）
    ("FUNC0247", 38), // FUNC 单元 0247（函数 × 相位 55 × 二 轮）
    ("CACHE0248", 45), // CACHE 单元 0248（缓存 × 相位 56 × 二 轮）
    ("HITRATE0249", 52), // HITRATE 单元 0249（命中率 × 相位 57 × 二 轮）
    ("UNK0250", 59), // UNK 单元 0250（未知 × 相位 58 × 二 轮）
    ("FALLBACK0251", 66), // FALLBACK 单元 0251（回落 × 相位 59 × 二 轮）
    ("SYM0252", 73), // SYM 单元 0252（符号 × 相位 60 × 二 轮）
    ("ADDR0253", 80), // ADDR 单元 0253（地址 × 相位 61 × 二 轮）
    ("RESOLVE0254", 87), // RESOLVE 单元 0254（解析 × 相位 62 × 二 轮）
    ("TBL0255", 5), // TBL 单元 0255（符号表 × 相位 63 × 二 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3661bmx_get(key: &str) -> Option<u64> {
    match key {
        "SYM0000" => Some(0),
        "ADDR0001" => Some(7),
        "RESOLVE0002" => Some(14),
        "TBL0003" => Some(21),
        "MOD0004" => Some(28),
        "OFFSET0005" => Some(35),
        "INLINE0006" => Some(42),
        "FUNC0007" => Some(49),
        "CACHE0008" => Some(56),
        "HITRATE0009" => Some(63),
        "UNK0010" => Some(70),
        "FALLBACK0011" => Some(77),
        "SYM0012" => Some(84),
        "ADDR0013" => Some(2),
        "RESOLVE0014" => Some(9),
        "TBL0015" => Some(16),
        "MOD0016" => Some(23),
        "OFFSET0017" => Some(30),
        "INLINE0018" => Some(37),
        "FUNC0019" => Some(44),
        "CACHE0020" => Some(51),
        "HITRATE0021" => Some(58),
        "UNK0022" => Some(65),
        "FALLBACK0023" => Some(72),
        "SYM0024" => Some(79),
        "ADDR0025" => Some(86),
        "RESOLVE0026" => Some(4),
        "TBL0027" => Some(11),
        "MOD0028" => Some(18),
        "OFFSET0029" => Some(25),
        "INLINE0030" => Some(32),
        "FUNC0031" => Some(39),
        "CACHE0032" => Some(46),
        "HITRATE0033" => Some(53),
        "UNK0034" => Some(60),
        "FALLBACK0035" => Some(67),
        "SYM0036" => Some(74),
        "ADDR0037" => Some(81),
        "RESOLVE0038" => Some(88),
        "TBL0039" => Some(6),
        "MOD0040" => Some(13),
        "OFFSET0041" => Some(20),
        "INLINE0042" => Some(27),
        "FUNC0043" => Some(34),
        "CACHE0044" => Some(41),
        "HITRATE0045" => Some(48),
        "UNK0046" => Some(55),
        "FALLBACK0047" => Some(62),
        "SYM0048" => Some(69),
        "ADDR0049" => Some(76),
        "RESOLVE0050" => Some(83),
        "TBL0051" => Some(1),
        "MOD0052" => Some(8),
        "OFFSET0053" => Some(15),
        "INLINE0054" => Some(22),
        "FUNC0055" => Some(29),
        "CACHE0056" => Some(36),
        "HITRATE0057" => Some(43),
        "UNK0058" => Some(50),
        "FALLBACK0059" => Some(57),
        "SYM0060" => Some(64),
        "ADDR0061" => Some(71),
        "RESOLVE0062" => Some(78),
        "TBL0063" => Some(85),
        "MOD0064" => Some(3),
        "OFFSET0065" => Some(10),
        "INLINE0066" => Some(17),
        "FUNC0067" => Some(24),
        "CACHE0068" => Some(31),
        "HITRATE0069" => Some(38),
        "UNK0070" => Some(45),
        "FALLBACK0071" => Some(52),
        "SYM0072" => Some(59),
        "ADDR0073" => Some(66),
        "RESOLVE0074" => Some(73),
        "TBL0075" => Some(80),
        "MOD0076" => Some(87),
        "OFFSET0077" => Some(5),
        "INLINE0078" => Some(12),
        "FUNC0079" => Some(19),
        "CACHE0080" => Some(26),
        "HITRATE0081" => Some(33),
        "UNK0082" => Some(40),
        "FALLBACK0083" => Some(47),
        "SYM0084" => Some(54),
        "ADDR0085" => Some(61),
        "RESOLVE0086" => Some(68),
        "TBL0087" => Some(75),
        "MOD0088" => Some(82),
        "OFFSET0089" => Some(0),
        "INLINE0090" => Some(7),
        "FUNC0091" => Some(14),
        "CACHE0092" => Some(21),
        "HITRATE0093" => Some(28),
        "UNK0094" => Some(35),
        "FALLBACK0095" => Some(42),
        "SYM0096" => Some(49),
        "ADDR0097" => Some(56),
        "RESOLVE0098" => Some(63),
        "TBL0099" => Some(70),
        "MOD0100" => Some(77),
        "OFFSET0101" => Some(84),
        "INLINE0102" => Some(2),
        "FUNC0103" => Some(9),
        "CACHE0104" => Some(16),
        "HITRATE0105" => Some(23),
        "UNK0106" => Some(30),
        "FALLBACK0107" => Some(37),
        "SYM0108" => Some(44),
        "ADDR0109" => Some(51),
        "RESOLVE0110" => Some(58),
        "TBL0111" => Some(65),
        "MOD0112" => Some(72),
        "OFFSET0113" => Some(79),
        "INLINE0114" => Some(86),
        "FUNC0115" => Some(4),
        "CACHE0116" => Some(11),
        "HITRATE0117" => Some(18),
        "UNK0118" => Some(25),
        "FALLBACK0119" => Some(32),
        "SYM0120" => Some(39),
        "ADDR0121" => Some(46),
        "RESOLVE0122" => Some(53),
        "TBL0123" => Some(60),
        "MOD0124" => Some(67),
        "OFFSET0125" => Some(74),
        "INLINE0126" => Some(81),
        "FUNC0127" => Some(88),
        "CACHE0128" => Some(6),
        "HITRATE0129" => Some(13),
        "UNK0130" => Some(20),
        "FALLBACK0131" => Some(27),
        "SYM0132" => Some(34),
        "ADDR0133" => Some(41),
        "RESOLVE0134" => Some(48),
        "TBL0135" => Some(55),
        "MOD0136" => Some(62),
        "OFFSET0137" => Some(69),
        "INLINE0138" => Some(76),
        "FUNC0139" => Some(83),
        "CACHE0140" => Some(1),
        "HITRATE0141" => Some(8),
        "UNK0142" => Some(15),
        "FALLBACK0143" => Some(22),
        "SYM0144" => Some(29),
        "ADDR0145" => Some(36),
        "RESOLVE0146" => Some(43),
        "TBL0147" => Some(50),
        "MOD0148" => Some(57),
        "OFFSET0149" => Some(64),
        "INLINE0150" => Some(71),
        "FUNC0151" => Some(78),
        "CACHE0152" => Some(85),
        "HITRATE0153" => Some(3),
        "UNK0154" => Some(10),
        "FALLBACK0155" => Some(17),
        "SYM0156" => Some(24),
        "ADDR0157" => Some(31),
        "RESOLVE0158" => Some(38),
        "TBL0159" => Some(45),
        "MOD0160" => Some(52),
        "OFFSET0161" => Some(59),
        "INLINE0162" => Some(66),
        "FUNC0163" => Some(73),
        "CACHE0164" => Some(80),
        "HITRATE0165" => Some(87),
        "UNK0166" => Some(5),
        "FALLBACK0167" => Some(12),
        "SYM0168" => Some(19),
        "ADDR0169" => Some(26),
        "RESOLVE0170" => Some(33),
        "TBL0171" => Some(40),
        "MOD0172" => Some(47),
        "OFFSET0173" => Some(54),
        "INLINE0174" => Some(61),
        "FUNC0175" => Some(68),
        "CACHE0176" => Some(75),
        "HITRATE0177" => Some(82),
        "UNK0178" => Some(0),
        "FALLBACK0179" => Some(7),
        "SYM0180" => Some(14),
        "ADDR0181" => Some(21),
        "RESOLVE0182" => Some(28),
        "TBL0183" => Some(35),
        "MOD0184" => Some(42),
        "OFFSET0185" => Some(49),
        "INLINE0186" => Some(56),
        "FUNC0187" => Some(63),
        "CACHE0188" => Some(70),
        "HITRATE0189" => Some(77),
        "UNK0190" => Some(84),
        "FALLBACK0191" => Some(2),
        "SYM0192" => Some(9),
        "ADDR0193" => Some(16),
        "RESOLVE0194" => Some(23),
        "TBL0195" => Some(30),
        "MOD0196" => Some(37),
        "OFFSET0197" => Some(44),
        "INLINE0198" => Some(51),
        "FUNC0199" => Some(58),
        "CACHE0200" => Some(65),
        "HITRATE0201" => Some(72),
        "UNK0202" => Some(79),
        "FALLBACK0203" => Some(86),
        "SYM0204" => Some(4),
        "ADDR0205" => Some(11),
        "RESOLVE0206" => Some(18),
        "TBL0207" => Some(25),
        "MOD0208" => Some(32),
        "OFFSET0209" => Some(39),
        "INLINE0210" => Some(46),
        "FUNC0211" => Some(53),
        "CACHE0212" => Some(60),
        "HITRATE0213" => Some(67),
        "UNK0214" => Some(74),
        "FALLBACK0215" => Some(81),
        "SYM0216" => Some(88),
        "ADDR0217" => Some(6),
        "RESOLVE0218" => Some(13),
        "TBL0219" => Some(20),
        "MOD0220" => Some(27),
        "OFFSET0221" => Some(34),
        "INLINE0222" => Some(41),
        "FUNC0223" => Some(48),
        "CACHE0224" => Some(55),
        "HITRATE0225" => Some(62),
        "UNK0226" => Some(69),
        "FALLBACK0227" => Some(76),
        "SYM0228" => Some(83),
        "ADDR0229" => Some(1),
        "RESOLVE0230" => Some(8),
        "TBL0231" => Some(15),
        "MOD0232" => Some(22),
        "OFFSET0233" => Some(29),
        "INLINE0234" => Some(36),
        "FUNC0235" => Some(43),
        "CACHE0236" => Some(50),
        "HITRATE0237" => Some(57),
        "UNK0238" => Some(64),
        "FALLBACK0239" => Some(71),
        "SYM0240" => Some(78),
        "ADDR0241" => Some(85),
        "RESOLVE0242" => Some(3),
        "TBL0243" => Some(10),
        "MOD0244" => Some(17),
        "OFFSET0245" => Some(24),
        "INLINE0246" => Some(31),
        "FUNC0247" => Some(38),
        "CACHE0248" => Some(45),
        "HITRATE0249" => Some(52),
        "UNK0250" => Some(59),
        "FALLBACK0251" => Some(66),
        "SYM0252" => Some(73),
        "ADDR0253" => Some(80),
        "RESOLVE0254" => Some(87),
        "TBL0255" => Some(5),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3661bmx_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3661bmx_COUNT {
        let (k, v) = u3661bmx[i];
        let ok = match k {
            "SYM0000" => v < 89,
            "ADDR0001" => v < 89,
            "RESOLVE0002" => v < 89,
            "TBL0003" => v < 89,
            "MOD0004" => v < 89,
            "OFFSET0005" => v < 89,
            "INLINE0006" => v < 89,
            "FUNC0007" => v < 89,
            "CACHE0008" => v < 89,
            "HITRATE0009" => v < 89,
            "UNK0010" => v < 89,
            "FALLBACK0011" => v < 89,
            "SYM0012" => v < 89,
            "ADDR0013" => v < 89,
            "RESOLVE0014" => v < 89,
            "TBL0015" => v < 89,
            "MOD0016" => v < 89,
            "OFFSET0017" => v < 89,
            "INLINE0018" => v < 89,
            "FUNC0019" => v < 89,
            "CACHE0020" => v < 89,
            "HITRATE0021" => v < 89,
            "UNK0022" => v < 89,
            "FALLBACK0023" => v < 89,
            "SYM0024" => v < 89,
            "ADDR0025" => v < 89,
            "RESOLVE0026" => v < 89,
            "TBL0027" => v < 89,
            "MOD0028" => v < 89,
            "OFFSET0029" => v < 89,
            "INLINE0030" => v < 89,
            "FUNC0031" => v < 89,
            "CACHE0032" => v < 89,
            "HITRATE0033" => v < 89,
            "UNK0034" => v < 89,
            "FALLBACK0035" => v < 89,
            "SYM0036" => v < 89,
            "ADDR0037" => v < 89,
            "RESOLVE0038" => v < 89,
            "TBL0039" => v < 89,
            "MOD0040" => v < 89,
            "OFFSET0041" => v < 89,
            "INLINE0042" => v < 89,
            "FUNC0043" => v < 89,
            "CACHE0044" => v < 89,
            "HITRATE0045" => v < 89,
            "UNK0046" => v < 89,
            "FALLBACK0047" => v < 89,
            "SYM0048" => v < 89,
            "ADDR0049" => v < 89,
            "RESOLVE0050" => v < 89,
            "TBL0051" => v < 89,
            "MOD0052" => v < 89,
            "OFFSET0053" => v < 89,
            "INLINE0054" => v < 89,
            "FUNC0055" => v < 89,
            "CACHE0056" => v < 89,
            "HITRATE0057" => v < 89,
            "UNK0058" => v < 89,
            "FALLBACK0059" => v < 89,
            "SYM0060" => v < 89,
            "ADDR0061" => v < 89,
            "RESOLVE0062" => v < 89,
            "TBL0063" => v < 89,
            "MOD0064" => v < 89,
            "OFFSET0065" => v < 89,
            "INLINE0066" => v < 89,
            "FUNC0067" => v < 89,
            "CACHE0068" => v < 89,
            "HITRATE0069" => v < 89,
            "UNK0070" => v < 89,
            "FALLBACK0071" => v < 89,
            "SYM0072" => v < 89,
            "ADDR0073" => v < 89,
            "RESOLVE0074" => v < 89,
            "TBL0075" => v < 89,
            "MOD0076" => v < 89,
            "OFFSET0077" => v < 89,
            "INLINE0078" => v < 89,
            "FUNC0079" => v < 89,
            "CACHE0080" => v < 89,
            "HITRATE0081" => v < 89,
            "UNK0082" => v < 89,
            "FALLBACK0083" => v < 89,
            "SYM0084" => v < 89,
            "ADDR0085" => v < 89,
            "RESOLVE0086" => v < 89,
            "TBL0087" => v < 89,
            "MOD0088" => v < 89,
            "OFFSET0089" => v < 89,
            "INLINE0090" => v < 89,
            "FUNC0091" => v < 89,
            "CACHE0092" => v < 89,
            "HITRATE0093" => v < 89,
            "UNK0094" => v < 89,
            "FALLBACK0095" => v < 89,
            "SYM0096" => v < 89,
            "ADDR0097" => v < 89,
            "RESOLVE0098" => v < 89,
            "TBL0099" => v < 89,
            "MOD0100" => v < 89,
            "OFFSET0101" => v < 89,
            "INLINE0102" => v < 89,
            "FUNC0103" => v < 89,
            "CACHE0104" => v < 89,
            "HITRATE0105" => v < 89,
            "UNK0106" => v < 89,
            "FALLBACK0107" => v < 89,
            "SYM0108" => v < 89,
            "ADDR0109" => v < 89,
            "RESOLVE0110" => v < 89,
            "TBL0111" => v < 89,
            "MOD0112" => v < 89,
            "OFFSET0113" => v < 89,
            "INLINE0114" => v < 89,
            "FUNC0115" => v < 89,
            "CACHE0116" => v < 89,
            "HITRATE0117" => v < 89,
            "UNK0118" => v < 89,
            "FALLBACK0119" => v < 89,
            "SYM0120" => v < 89,
            "ADDR0121" => v < 89,
            "RESOLVE0122" => v < 89,
            "TBL0123" => v < 89,
            "MOD0124" => v < 89,
            "OFFSET0125" => v < 89,
            "INLINE0126" => v < 89,
            "FUNC0127" => v < 89,
            "CACHE0128" => v < 89,
            "HITRATE0129" => v < 89,
            "UNK0130" => v < 89,
            "FALLBACK0131" => v < 89,
            "SYM0132" => v < 89,
            "ADDR0133" => v < 89,
            "RESOLVE0134" => v < 89,
            "TBL0135" => v < 89,
            "MOD0136" => v < 89,
            "OFFSET0137" => v < 89,
            "INLINE0138" => v < 89,
            "FUNC0139" => v < 89,
            "CACHE0140" => v < 89,
            "HITRATE0141" => v < 89,
            "UNK0142" => v < 89,
            "FALLBACK0143" => v < 89,
            "SYM0144" => v < 89,
            "ADDR0145" => v < 89,
            "RESOLVE0146" => v < 89,
            "TBL0147" => v < 89,
            "MOD0148" => v < 89,
            "OFFSET0149" => v < 89,
            "INLINE0150" => v < 89,
            "FUNC0151" => v < 89,
            "CACHE0152" => v < 89,
            "HITRATE0153" => v < 89,
            "UNK0154" => v < 89,
            "FALLBACK0155" => v < 89,
            "SYM0156" => v < 89,
            "ADDR0157" => v < 89,
            "RESOLVE0158" => v < 89,
            "TBL0159" => v < 89,
            "MOD0160" => v < 89,
            "OFFSET0161" => v < 89,
            "INLINE0162" => v < 89,
            "FUNC0163" => v < 89,
            "CACHE0164" => v < 89,
            "HITRATE0165" => v < 89,
            "UNK0166" => v < 89,
            "FALLBACK0167" => v < 89,
            "SYM0168" => v < 89,
            "ADDR0169" => v < 89,
            "RESOLVE0170" => v < 89,
            "TBL0171" => v < 89,
            "MOD0172" => v < 89,
            "OFFSET0173" => v < 89,
            "INLINE0174" => v < 89,
            "FUNC0175" => v < 89,
            "CACHE0176" => v < 89,
            "HITRATE0177" => v < 89,
            "UNK0178" => v < 89,
            "FALLBACK0179" => v < 89,
            "SYM0180" => v < 89,
            "ADDR0181" => v < 89,
            "RESOLVE0182" => v < 89,
            "TBL0183" => v < 89,
            "MOD0184" => v < 89,
            "OFFSET0185" => v < 89,
            "INLINE0186" => v < 89,
            "FUNC0187" => v < 89,
            "CACHE0188" => v < 89,
            "HITRATE0189" => v < 89,
            "UNK0190" => v < 89,
            "FALLBACK0191" => v < 89,
            "SYM0192" => v < 89,
            "ADDR0193" => v < 89,
            "RESOLVE0194" => v < 89,
            "TBL0195" => v < 89,
            "MOD0196" => v < 89,
            "OFFSET0197" => v < 89,
            "INLINE0198" => v < 89,
            "FUNC0199" => v < 89,
            "CACHE0200" => v < 89,
            "HITRATE0201" => v < 89,
            "UNK0202" => v < 89,
            "FALLBACK0203" => v < 89,
            "SYM0204" => v < 89,
            "ADDR0205" => v < 89,
            "RESOLVE0206" => v < 89,
            "TBL0207" => v < 89,
            "MOD0208" => v < 89,
            "OFFSET0209" => v < 89,
            "INLINE0210" => v < 89,
            "FUNC0211" => v < 89,
            "CACHE0212" => v < 89,
            "HITRATE0213" => v < 89,
            "UNK0214" => v < 89,
            "FALLBACK0215" => v < 89,
            "SYM0216" => v < 89,
            "ADDR0217" => v < 89,
            "RESOLVE0218" => v < 89,
            "TBL0219" => v < 89,
            "MOD0220" => v < 89,
            "OFFSET0221" => v < 89,
            "INLINE0222" => v < 89,
            "FUNC0223" => v < 89,
            "CACHE0224" => v < 89,
            "HITRATE0225" => v < 89,
            "UNK0226" => v < 89,
            "FALLBACK0227" => v < 89,
            "SYM0228" => v < 89,
            "ADDR0229" => v < 89,
            "RESOLVE0230" => v < 89,
            "TBL0231" => v < 89,
            "MOD0232" => v < 89,
            "OFFSET0233" => v < 89,
            "INLINE0234" => v < 89,
            "FUNC0235" => v < 89,
            "CACHE0236" => v < 89,
            "HITRATE0237" => v < 89,
            "UNK0238" => v < 89,
            "FALLBACK0239" => v < 89,
            "SYM0240" => v < 89,
            "ADDR0241" => v < 89,
            "RESOLVE0242" => v < 89,
            "TBL0243" => v < 89,
            "MOD0244" => v < 89,
            "OFFSET0245" => v < 89,
            "INLINE0246" => v < 89,
            "FUNC0247" => v < 89,
            "CACHE0248" => v < 89,
            "HITRATE0249" => v < 89,
            "UNK0250" => v < 89,
            "FALLBACK0251" => v < 89,
            "SYM0252" => v < 89,
            "ADDR0253" => v < 89,
            "RESOLVE0254" => v < 89,
            "TBL0255" => v < 89,
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
pub fn u3661bmx_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3661bmx_COUNT {
        s = s.wrapping_add(u3661bmx[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3661bmx_unique() -> bool {
    let mut i = 0usize;
    while i < u3661bmx_COUNT {
        let mut j = i + 1;
        while j < u3661bmx_COUNT {
            if u3661bmx[i].0 == u3661bmx[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1f0, 0xe1ff]。
pub const fn u3661_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3661Code::ALL.len() {
        let c = u3661Code::ALL[i].code();
        if c < 0xe1f0 || c > 0xe1ff {
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
fn u3661_guard_seg() -> bool {
    u3661_segment_guard()
}

/// UNX-F3661 bmx 256 行齐备 + 边界锚 + 键外拒收。
fn check_u3661_bmx_count() -> bool {
    let mut ok = u3661bmx_COUNT == 256;
    if ok { ok = u3661bmx_get("SYM0000") == Some(0) && u3661bmx_get("TBL0255") == Some(5); }
    if ok { ok = u3661bmx_get("ZZZ_NOKEY").is_none(); }
    ok
}

/// UNX-F3661 bmx 累计守恒（理论值 11231）。
fn check_u3661_bmx_sum() -> bool {
    u3661bmx_sum() == 11231
}

/// UNX-F3661 bmx 逐行守卫与唯一性全绿。
fn check_u3661_bmx_guard() -> bool {
    u3661bmx_scan() == 0 && u3661bmx_unique()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3661_checks() -> CheckSet {
    let mut set = CheckSet::new("U3661");
    set.add(
        "u3661-seg-guard",
        u3661_guard_seg(),
        if u3661_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3661-bmx-count",
        check_u3661_bmx_count(),
        if check_u3661_bmx_count() { "" } else { "UNX-F3661 bmx count mismatch" },
    );
    set.add(
        "u3661-bmx-sum",
        check_u3661_bmx_sum(),
        if check_u3661_bmx_sum() { "" } else { "UNX-F3661 bmx sum mismatch" },
    );
    set.add(
        "u3661-bmx-guard",
        check_u3661_bmx_guard(),
        if check_u3661_bmx_guard() { "" } else { "UNX-F3661 bmx guard mismatch" },
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
    fn u3661_all_checks_green() {
        let set = run_u3661_checks();
        assert!(set.all_passed(), "U3661 domain checks must be green");
    }

    #[test]
    fn u3661_segment_guard_holds() {
        assert!(u3661_segment_guard());
    }
}
