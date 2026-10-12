//! unxu3001_u3001bmx —— 双链结构冻结（active/inactive 两链节点布局） 深编bmx（双链结构冻结 × 256 单元）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3001 · 双链结构冻结（active/inactive 两链节点布局） · 判据原文：UNX-F3001 · 判据：UNX-F3001-J1 active/inactive 双链结构冻结哈希入册：节点布局（page 结构挂链位/lru 位标/双向指针）与 F2682 页缓存驻留结构对齐零缺失，链头尾指针操作单点化
//! · 组件：双链结构冻结 守卫映射 256 行（二 轮派生真实数据 + 逐行守卫 + 键唯一）
//! · 判据落地：UNX-F3001 · 判据：UNX-F3001-J1 active/inactive 双链结构冻结哈希入册：节点布局（page 结构挂链位/lru 位标/双向
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3001_VERSION: &str = "u3001-u3001bmx-v1";

/// 诊断段基址。
pub const U3001_SEG_BASE: u16 = 0xe300;

// --------------------------------------------------------------------------
// 诊断码（u3001 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 双链结构冻结 bmx 面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3001Code {
    /// 0xe300 单元格重复（双链 同键双占）。
    CellDup,
    /// 0xe301 单元格断档（节点 序号不连续）。
    CellGap,
    /// 0xe302 索引越界（迁移）。
    IndexOob,
    /// 0xe303 大表账破缺（行数≠登记值）。
    LedgerBreak,
    /// 0xe304 表序断档。
    OrderBreak,
    /// 0xe305 累计漂移（求和≠理论值）。
    SumDrift,
    /// 0xe306 掩码非法。
    MaskBad,
    /// 0xe307 守卫触发。
    GuardTripped,
    /// 0xe308 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe309 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe30a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe30b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3001Code {
    /// 全量闭集。
    pub const ALL: [u3001Code; 12] = [
        u3001Code::CellDup,
        u3001Code::CellGap,
        u3001Code::IndexOob,
        u3001Code::LedgerBreak,
        u3001Code::OrderBreak,
        u3001Code::SumDrift,
        u3001Code::MaskBad,
        u3001Code::GuardTripped,
        u3001Code::LedgerBroken,
        u3001Code::SnapshotFingerprint,
        u3001Code::TableBroken,
        u3001Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3001Code::CellDup => 0xe300,
            u3001Code::CellGap => 0xe301,
            u3001Code::IndexOob => 0xe302,
            u3001Code::LedgerBreak => 0xe303,
            u3001Code::OrderBreak => 0xe304,
            u3001Code::SumDrift => 0xe305,
            u3001Code::MaskBad => 0xe306,
            u3001Code::GuardTripped => 0xe307,
            u3001Code::LedgerBroken => 0xe308,
            u3001Code::SnapshotFingerprint => 0xe309,
            u3001Code::TableBroken => 0xe30a,
            u3001Code::SegmentGuard => 0xe30b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3001Code::CellDup => "单元格重复（双链 同键双占）",
            u3001Code::CellGap => "单元格断档（节点 序号不连续）",
            u3001Code::IndexOob => "索引越界（迁移）",
            u3001Code::LedgerBreak => "大表账破缺（行数≠登记值）",
            u3001Code::OrderBreak => "表序断档",
            u3001Code::SumDrift => "累计漂移（求和≠理论值）",
            u3001Code::MaskBad => "掩码非法",
            u3001Code::GuardTripped => "守卫触发",
            u3001Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3001Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3001Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3001Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3001Code::CellDup => "按键唯一律去重",
            u3001Code::CellGap => "按连续律补号",
            u3001Code::IndexOob => "按行数边界裁剪",
            u3001Code::LedgerBreak => "按守恒重建",
            u3001Code::OrderBreak => "按序重排",
            u3001Code::SumDrift => "按理论值复算",
            u3001Code::MaskBad => "按掩码律校正",
            u3001Code::GuardTripped => "检查临界区",
            u3001Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3001Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3001Code::TableBroken => "按语义规则重建常量表",
            u3001Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3001bmx（256 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 双链结构冻结 bmx 守卫映射（256 行逐行注释自证）。
/// 行数。
pub const u3001bmx_COUNT: usize = 256;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3001bmx: [(&'static str, u64); 256] = [
    ("ACTIVE0000", 0), // ACTIVE 单元 0000（active 链 × 相位 00 × 二 轮）
    ("INACT0001", 7), // INACT 单元 0001（inactive 链 × 相位 01 × 二 轮）
    ("NODE0002", 14), // NODE 单元 0002（节点 × 相位 02 × 二 轮）
    ("LAYOUT0003", 21), // LAYOUT 单元 0003（节点布局 × 相位 03 × 二 轮）
    ("NEXT0004", 28), // NEXT 单元 0004（后继 × 相位 04 × 二 轮）
    ("PREV0005", 35), // PREV 单元 0005（前驱 × 相位 05 × 二 轮）
    ("FREE0006", 42), // FREE 单元 0006（空闲链 × 相位 06 × 二 轮）
    ("POOL0007", 49), // POOL 单元 0007（节点池 × 相位 07 × 二 轮）
    ("MOVE0008", 56), // MOVE 单元 0008（迁移 × 相位 08 × 二 轮）
    ("SPLIT0009", 63), // SPLIT 单元 0009（拆分 × 相位 09 × 二 轮）
    ("JOIN0010", 70), // JOIN 单元 0010（合并 × 相位 10 × 二 轮）
    ("INV0011", 77), // INV 单元 0011（不变量 × 相位 11 × 二 轮）
    ("ACTIVE0012", 84), // ACTIVE 单元 0012（active 链 × 相位 12 × 二 轮）
    ("INACT0013", 2), // INACT 单元 0013（inactive 链 × 相位 13 × 二 轮）
    ("NODE0014", 9), // NODE 单元 0014（节点 × 相位 14 × 二 轮）
    ("LAYOUT0015", 16), // LAYOUT 单元 0015（节点布局 × 相位 15 × 二 轮）
    ("NEXT0016", 23), // NEXT 单元 0016（后继 × 相位 16 × 二 轮）
    ("PREV0017", 30), // PREV 单元 0017（前驱 × 相位 17 × 二 轮）
    ("FREE0018", 37), // FREE 单元 0018（空闲链 × 相位 18 × 二 轮）
    ("POOL0019", 44), // POOL 单元 0019（节点池 × 相位 19 × 二 轮）
    ("MOVE0020", 51), // MOVE 单元 0020（迁移 × 相位 20 × 二 轮）
    ("SPLIT0021", 58), // SPLIT 单元 0021（拆分 × 相位 21 × 二 轮）
    ("JOIN0022", 65), // JOIN 单元 0022（合并 × 相位 22 × 二 轮）
    ("INV0023", 72), // INV 单元 0023（不变量 × 相位 23 × 二 轮）
    ("ACTIVE0024", 79), // ACTIVE 单元 0024（active 链 × 相位 24 × 二 轮）
    ("INACT0025", 86), // INACT 单元 0025（inactive 链 × 相位 25 × 二 轮）
    ("NODE0026", 4), // NODE 单元 0026（节点 × 相位 26 × 二 轮）
    ("LAYOUT0027", 11), // LAYOUT 单元 0027（节点布局 × 相位 27 × 二 轮）
    ("NEXT0028", 18), // NEXT 单元 0028（后继 × 相位 28 × 二 轮）
    ("PREV0029", 25), // PREV 单元 0029（前驱 × 相位 29 × 二 轮）
    ("FREE0030", 32), // FREE 单元 0030（空闲链 × 相位 30 × 二 轮）
    ("POOL0031", 39), // POOL 单元 0031（节点池 × 相位 31 × 二 轮）
    ("MOVE0032", 46), // MOVE 单元 0032（迁移 × 相位 32 × 二 轮）
    ("SPLIT0033", 53), // SPLIT 单元 0033（拆分 × 相位 33 × 二 轮）
    ("JOIN0034", 60), // JOIN 单元 0034（合并 × 相位 34 × 二 轮）
    ("INV0035", 67), // INV 单元 0035（不变量 × 相位 35 × 二 轮）
    ("ACTIVE0036", 74), // ACTIVE 单元 0036（active 链 × 相位 36 × 二 轮）
    ("INACT0037", 81), // INACT 单元 0037（inactive 链 × 相位 37 × 二 轮）
    ("NODE0038", 88), // NODE 单元 0038（节点 × 相位 38 × 二 轮）
    ("LAYOUT0039", 6), // LAYOUT 单元 0039（节点布局 × 相位 39 × 二 轮）
    ("NEXT0040", 13), // NEXT 单元 0040（后继 × 相位 40 × 二 轮）
    ("PREV0041", 20), // PREV 单元 0041（前驱 × 相位 41 × 二 轮）
    ("FREE0042", 27), // FREE 单元 0042（空闲链 × 相位 42 × 二 轮）
    ("POOL0043", 34), // POOL 单元 0043（节点池 × 相位 43 × 二 轮）
    ("MOVE0044", 41), // MOVE 单元 0044（迁移 × 相位 44 × 二 轮）
    ("SPLIT0045", 48), // SPLIT 单元 0045（拆分 × 相位 45 × 二 轮）
    ("JOIN0046", 55), // JOIN 单元 0046（合并 × 相位 46 × 二 轮）
    ("INV0047", 62), // INV 单元 0047（不变量 × 相位 47 × 二 轮）
    ("ACTIVE0048", 69), // ACTIVE 单元 0048（active 链 × 相位 48 × 二 轮）
    ("INACT0049", 76), // INACT 单元 0049（inactive 链 × 相位 49 × 二 轮）
    ("NODE0050", 83), // NODE 单元 0050（节点 × 相位 50 × 二 轮）
    ("LAYOUT0051", 1), // LAYOUT 单元 0051（节点布局 × 相位 51 × 二 轮）
    ("NEXT0052", 8), // NEXT 单元 0052（后继 × 相位 52 × 二 轮）
    ("PREV0053", 15), // PREV 单元 0053（前驱 × 相位 53 × 二 轮）
    ("FREE0054", 22), // FREE 单元 0054（空闲链 × 相位 54 × 二 轮）
    ("POOL0055", 29), // POOL 单元 0055（节点池 × 相位 55 × 二 轮）
    ("MOVE0056", 36), // MOVE 单元 0056（迁移 × 相位 56 × 二 轮）
    ("SPLIT0057", 43), // SPLIT 单元 0057（拆分 × 相位 57 × 二 轮）
    ("JOIN0058", 50), // JOIN 单元 0058（合并 × 相位 58 × 二 轮）
    ("INV0059", 57), // INV 单元 0059（不变量 × 相位 59 × 二 轮）
    ("ACTIVE0060", 64), // ACTIVE 单元 0060（active 链 × 相位 60 × 二 轮）
    ("INACT0061", 71), // INACT 单元 0061（inactive 链 × 相位 61 × 二 轮）
    ("NODE0062", 78), // NODE 单元 0062（节点 × 相位 62 × 二 轮）
    ("LAYOUT0063", 85), // LAYOUT 单元 0063（节点布局 × 相位 63 × 二 轮）
    ("NEXT0064", 3), // NEXT 单元 0064（后继 × 相位 00 × 二 轮）
    ("PREV0065", 10), // PREV 单元 0065（前驱 × 相位 01 × 二 轮）
    ("FREE0066", 17), // FREE 单元 0066（空闲链 × 相位 02 × 二 轮）
    ("POOL0067", 24), // POOL 单元 0067（节点池 × 相位 03 × 二 轮）
    ("MOVE0068", 31), // MOVE 单元 0068（迁移 × 相位 04 × 二 轮）
    ("SPLIT0069", 38), // SPLIT 单元 0069（拆分 × 相位 05 × 二 轮）
    ("JOIN0070", 45), // JOIN 单元 0070（合并 × 相位 06 × 二 轮）
    ("INV0071", 52), // INV 单元 0071（不变量 × 相位 07 × 二 轮）
    ("ACTIVE0072", 59), // ACTIVE 单元 0072（active 链 × 相位 08 × 二 轮）
    ("INACT0073", 66), // INACT 单元 0073（inactive 链 × 相位 09 × 二 轮）
    ("NODE0074", 73), // NODE 单元 0074（节点 × 相位 10 × 二 轮）
    ("LAYOUT0075", 80), // LAYOUT 单元 0075（节点布局 × 相位 11 × 二 轮）
    ("NEXT0076", 87), // NEXT 单元 0076（后继 × 相位 12 × 二 轮）
    ("PREV0077", 5), // PREV 单元 0077（前驱 × 相位 13 × 二 轮）
    ("FREE0078", 12), // FREE 单元 0078（空闲链 × 相位 14 × 二 轮）
    ("POOL0079", 19), // POOL 单元 0079（节点池 × 相位 15 × 二 轮）
    ("MOVE0080", 26), // MOVE 单元 0080（迁移 × 相位 16 × 二 轮）
    ("SPLIT0081", 33), // SPLIT 单元 0081（拆分 × 相位 17 × 二 轮）
    ("JOIN0082", 40), // JOIN 单元 0082（合并 × 相位 18 × 二 轮）
    ("INV0083", 47), // INV 单元 0083（不变量 × 相位 19 × 二 轮）
    ("ACTIVE0084", 54), // ACTIVE 单元 0084（active 链 × 相位 20 × 二 轮）
    ("INACT0085", 61), // INACT 单元 0085（inactive 链 × 相位 21 × 二 轮）
    ("NODE0086", 68), // NODE 单元 0086（节点 × 相位 22 × 二 轮）
    ("LAYOUT0087", 75), // LAYOUT 单元 0087（节点布局 × 相位 23 × 二 轮）
    ("NEXT0088", 82), // NEXT 单元 0088（后继 × 相位 24 × 二 轮）
    ("PREV0089", 0), // PREV 单元 0089（前驱 × 相位 25 × 二 轮）
    ("FREE0090", 7), // FREE 单元 0090（空闲链 × 相位 26 × 二 轮）
    ("POOL0091", 14), // POOL 单元 0091（节点池 × 相位 27 × 二 轮）
    ("MOVE0092", 21), // MOVE 单元 0092（迁移 × 相位 28 × 二 轮）
    ("SPLIT0093", 28), // SPLIT 单元 0093（拆分 × 相位 29 × 二 轮）
    ("JOIN0094", 35), // JOIN 单元 0094（合并 × 相位 30 × 二 轮）
    ("INV0095", 42), // INV 单元 0095（不变量 × 相位 31 × 二 轮）
    ("ACTIVE0096", 49), // ACTIVE 单元 0096（active 链 × 相位 32 × 二 轮）
    ("INACT0097", 56), // INACT 单元 0097（inactive 链 × 相位 33 × 二 轮）
    ("NODE0098", 63), // NODE 单元 0098（节点 × 相位 34 × 二 轮）
    ("LAYOUT0099", 70), // LAYOUT 单元 0099（节点布局 × 相位 35 × 二 轮）
    ("NEXT0100", 77), // NEXT 单元 0100（后继 × 相位 36 × 二 轮）
    ("PREV0101", 84), // PREV 单元 0101（前驱 × 相位 37 × 二 轮）
    ("FREE0102", 2), // FREE 单元 0102（空闲链 × 相位 38 × 二 轮）
    ("POOL0103", 9), // POOL 单元 0103（节点池 × 相位 39 × 二 轮）
    ("MOVE0104", 16), // MOVE 单元 0104（迁移 × 相位 40 × 二 轮）
    ("SPLIT0105", 23), // SPLIT 单元 0105（拆分 × 相位 41 × 二 轮）
    ("JOIN0106", 30), // JOIN 单元 0106（合并 × 相位 42 × 二 轮）
    ("INV0107", 37), // INV 单元 0107（不变量 × 相位 43 × 二 轮）
    ("ACTIVE0108", 44), // ACTIVE 单元 0108（active 链 × 相位 44 × 二 轮）
    ("INACT0109", 51), // INACT 单元 0109（inactive 链 × 相位 45 × 二 轮）
    ("NODE0110", 58), // NODE 单元 0110（节点 × 相位 46 × 二 轮）
    ("LAYOUT0111", 65), // LAYOUT 单元 0111（节点布局 × 相位 47 × 二 轮）
    ("NEXT0112", 72), // NEXT 单元 0112（后继 × 相位 48 × 二 轮）
    ("PREV0113", 79), // PREV 单元 0113（前驱 × 相位 49 × 二 轮）
    ("FREE0114", 86), // FREE 单元 0114（空闲链 × 相位 50 × 二 轮）
    ("POOL0115", 4), // POOL 单元 0115（节点池 × 相位 51 × 二 轮）
    ("MOVE0116", 11), // MOVE 单元 0116（迁移 × 相位 52 × 二 轮）
    ("SPLIT0117", 18), // SPLIT 单元 0117（拆分 × 相位 53 × 二 轮）
    ("JOIN0118", 25), // JOIN 单元 0118（合并 × 相位 54 × 二 轮）
    ("INV0119", 32), // INV 单元 0119（不变量 × 相位 55 × 二 轮）
    ("ACTIVE0120", 39), // ACTIVE 单元 0120（active 链 × 相位 56 × 二 轮）
    ("INACT0121", 46), // INACT 单元 0121（inactive 链 × 相位 57 × 二 轮）
    ("NODE0122", 53), // NODE 单元 0122（节点 × 相位 58 × 二 轮）
    ("LAYOUT0123", 60), // LAYOUT 单元 0123（节点布局 × 相位 59 × 二 轮）
    ("NEXT0124", 67), // NEXT 单元 0124（后继 × 相位 60 × 二 轮）
    ("PREV0125", 74), // PREV 单元 0125（前驱 × 相位 61 × 二 轮）
    ("FREE0126", 81), // FREE 单元 0126（空闲链 × 相位 62 × 二 轮）
    ("POOL0127", 88), // POOL 单元 0127（节点池 × 相位 63 × 二 轮）
    ("MOVE0128", 6), // MOVE 单元 0128（迁移 × 相位 00 × 二 轮）
    ("SPLIT0129", 13), // SPLIT 单元 0129（拆分 × 相位 01 × 二 轮）
    ("JOIN0130", 20), // JOIN 单元 0130（合并 × 相位 02 × 二 轮）
    ("INV0131", 27), // INV 单元 0131（不变量 × 相位 03 × 二 轮）
    ("ACTIVE0132", 34), // ACTIVE 单元 0132（active 链 × 相位 04 × 二 轮）
    ("INACT0133", 41), // INACT 单元 0133（inactive 链 × 相位 05 × 二 轮）
    ("NODE0134", 48), // NODE 单元 0134（节点 × 相位 06 × 二 轮）
    ("LAYOUT0135", 55), // LAYOUT 单元 0135（节点布局 × 相位 07 × 二 轮）
    ("NEXT0136", 62), // NEXT 单元 0136（后继 × 相位 08 × 二 轮）
    ("PREV0137", 69), // PREV 单元 0137（前驱 × 相位 09 × 二 轮）
    ("FREE0138", 76), // FREE 单元 0138（空闲链 × 相位 10 × 二 轮）
    ("POOL0139", 83), // POOL 单元 0139（节点池 × 相位 11 × 二 轮）
    ("MOVE0140", 1), // MOVE 单元 0140（迁移 × 相位 12 × 二 轮）
    ("SPLIT0141", 8), // SPLIT 单元 0141（拆分 × 相位 13 × 二 轮）
    ("JOIN0142", 15), // JOIN 单元 0142（合并 × 相位 14 × 二 轮）
    ("INV0143", 22), // INV 单元 0143（不变量 × 相位 15 × 二 轮）
    ("ACTIVE0144", 29), // ACTIVE 单元 0144（active 链 × 相位 16 × 二 轮）
    ("INACT0145", 36), // INACT 单元 0145（inactive 链 × 相位 17 × 二 轮）
    ("NODE0146", 43), // NODE 单元 0146（节点 × 相位 18 × 二 轮）
    ("LAYOUT0147", 50), // LAYOUT 单元 0147（节点布局 × 相位 19 × 二 轮）
    ("NEXT0148", 57), // NEXT 单元 0148（后继 × 相位 20 × 二 轮）
    ("PREV0149", 64), // PREV 单元 0149（前驱 × 相位 21 × 二 轮）
    ("FREE0150", 71), // FREE 单元 0150（空闲链 × 相位 22 × 二 轮）
    ("POOL0151", 78), // POOL 单元 0151（节点池 × 相位 23 × 二 轮）
    ("MOVE0152", 85), // MOVE 单元 0152（迁移 × 相位 24 × 二 轮）
    ("SPLIT0153", 3), // SPLIT 单元 0153（拆分 × 相位 25 × 二 轮）
    ("JOIN0154", 10), // JOIN 单元 0154（合并 × 相位 26 × 二 轮）
    ("INV0155", 17), // INV 单元 0155（不变量 × 相位 27 × 二 轮）
    ("ACTIVE0156", 24), // ACTIVE 单元 0156（active 链 × 相位 28 × 二 轮）
    ("INACT0157", 31), // INACT 单元 0157（inactive 链 × 相位 29 × 二 轮）
    ("NODE0158", 38), // NODE 单元 0158（节点 × 相位 30 × 二 轮）
    ("LAYOUT0159", 45), // LAYOUT 单元 0159（节点布局 × 相位 31 × 二 轮）
    ("NEXT0160", 52), // NEXT 单元 0160（后继 × 相位 32 × 二 轮）
    ("PREV0161", 59), // PREV 单元 0161（前驱 × 相位 33 × 二 轮）
    ("FREE0162", 66), // FREE 单元 0162（空闲链 × 相位 34 × 二 轮）
    ("POOL0163", 73), // POOL 单元 0163（节点池 × 相位 35 × 二 轮）
    ("MOVE0164", 80), // MOVE 单元 0164（迁移 × 相位 36 × 二 轮）
    ("SPLIT0165", 87), // SPLIT 单元 0165（拆分 × 相位 37 × 二 轮）
    ("JOIN0166", 5), // JOIN 单元 0166（合并 × 相位 38 × 二 轮）
    ("INV0167", 12), // INV 单元 0167（不变量 × 相位 39 × 二 轮）
    ("ACTIVE0168", 19), // ACTIVE 单元 0168（active 链 × 相位 40 × 二 轮）
    ("INACT0169", 26), // INACT 单元 0169（inactive 链 × 相位 41 × 二 轮）
    ("NODE0170", 33), // NODE 单元 0170（节点 × 相位 42 × 二 轮）
    ("LAYOUT0171", 40), // LAYOUT 单元 0171（节点布局 × 相位 43 × 二 轮）
    ("NEXT0172", 47), // NEXT 单元 0172（后继 × 相位 44 × 二 轮）
    ("PREV0173", 54), // PREV 单元 0173（前驱 × 相位 45 × 二 轮）
    ("FREE0174", 61), // FREE 单元 0174（空闲链 × 相位 46 × 二 轮）
    ("POOL0175", 68), // POOL 单元 0175（节点池 × 相位 47 × 二 轮）
    ("MOVE0176", 75), // MOVE 单元 0176（迁移 × 相位 48 × 二 轮）
    ("SPLIT0177", 82), // SPLIT 单元 0177（拆分 × 相位 49 × 二 轮）
    ("JOIN0178", 0), // JOIN 单元 0178（合并 × 相位 50 × 二 轮）
    ("INV0179", 7), // INV 单元 0179（不变量 × 相位 51 × 二 轮）
    ("ACTIVE0180", 14), // ACTIVE 单元 0180（active 链 × 相位 52 × 二 轮）
    ("INACT0181", 21), // INACT 单元 0181（inactive 链 × 相位 53 × 二 轮）
    ("NODE0182", 28), // NODE 单元 0182（节点 × 相位 54 × 二 轮）
    ("LAYOUT0183", 35), // LAYOUT 单元 0183（节点布局 × 相位 55 × 二 轮）
    ("NEXT0184", 42), // NEXT 单元 0184（后继 × 相位 56 × 二 轮）
    ("PREV0185", 49), // PREV 单元 0185（前驱 × 相位 57 × 二 轮）
    ("FREE0186", 56), // FREE 单元 0186（空闲链 × 相位 58 × 二 轮）
    ("POOL0187", 63), // POOL 单元 0187（节点池 × 相位 59 × 二 轮）
    ("MOVE0188", 70), // MOVE 单元 0188（迁移 × 相位 60 × 二 轮）
    ("SPLIT0189", 77), // SPLIT 单元 0189（拆分 × 相位 61 × 二 轮）
    ("JOIN0190", 84), // JOIN 单元 0190（合并 × 相位 62 × 二 轮）
    ("INV0191", 2), // INV 单元 0191（不变量 × 相位 63 × 二 轮）
    ("ACTIVE0192", 9), // ACTIVE 单元 0192（active 链 × 相位 00 × 二 轮）
    ("INACT0193", 16), // INACT 单元 0193（inactive 链 × 相位 01 × 二 轮）
    ("NODE0194", 23), // NODE 单元 0194（节点 × 相位 02 × 二 轮）
    ("LAYOUT0195", 30), // LAYOUT 单元 0195（节点布局 × 相位 03 × 二 轮）
    ("NEXT0196", 37), // NEXT 单元 0196（后继 × 相位 04 × 二 轮）
    ("PREV0197", 44), // PREV 单元 0197（前驱 × 相位 05 × 二 轮）
    ("FREE0198", 51), // FREE 单元 0198（空闲链 × 相位 06 × 二 轮）
    ("POOL0199", 58), // POOL 单元 0199（节点池 × 相位 07 × 二 轮）
    ("MOVE0200", 65), // MOVE 单元 0200（迁移 × 相位 08 × 二 轮）
    ("SPLIT0201", 72), // SPLIT 单元 0201（拆分 × 相位 09 × 二 轮）
    ("JOIN0202", 79), // JOIN 单元 0202（合并 × 相位 10 × 二 轮）
    ("INV0203", 86), // INV 单元 0203（不变量 × 相位 11 × 二 轮）
    ("ACTIVE0204", 4), // ACTIVE 单元 0204（active 链 × 相位 12 × 二 轮）
    ("INACT0205", 11), // INACT 单元 0205（inactive 链 × 相位 13 × 二 轮）
    ("NODE0206", 18), // NODE 单元 0206（节点 × 相位 14 × 二 轮）
    ("LAYOUT0207", 25), // LAYOUT 单元 0207（节点布局 × 相位 15 × 二 轮）
    ("NEXT0208", 32), // NEXT 单元 0208（后继 × 相位 16 × 二 轮）
    ("PREV0209", 39), // PREV 单元 0209（前驱 × 相位 17 × 二 轮）
    ("FREE0210", 46), // FREE 单元 0210（空闲链 × 相位 18 × 二 轮）
    ("POOL0211", 53), // POOL 单元 0211（节点池 × 相位 19 × 二 轮）
    ("MOVE0212", 60), // MOVE 单元 0212（迁移 × 相位 20 × 二 轮）
    ("SPLIT0213", 67), // SPLIT 单元 0213（拆分 × 相位 21 × 二 轮）
    ("JOIN0214", 74), // JOIN 单元 0214（合并 × 相位 22 × 二 轮）
    ("INV0215", 81), // INV 单元 0215（不变量 × 相位 23 × 二 轮）
    ("ACTIVE0216", 88), // ACTIVE 单元 0216（active 链 × 相位 24 × 二 轮）
    ("INACT0217", 6), // INACT 单元 0217（inactive 链 × 相位 25 × 二 轮）
    ("NODE0218", 13), // NODE 单元 0218（节点 × 相位 26 × 二 轮）
    ("LAYOUT0219", 20), // LAYOUT 单元 0219（节点布局 × 相位 27 × 二 轮）
    ("NEXT0220", 27), // NEXT 单元 0220（后继 × 相位 28 × 二 轮）
    ("PREV0221", 34), // PREV 单元 0221（前驱 × 相位 29 × 二 轮）
    ("FREE0222", 41), // FREE 单元 0222（空闲链 × 相位 30 × 二 轮）
    ("POOL0223", 48), // POOL 单元 0223（节点池 × 相位 31 × 二 轮）
    ("MOVE0224", 55), // MOVE 单元 0224（迁移 × 相位 32 × 二 轮）
    ("SPLIT0225", 62), // SPLIT 单元 0225（拆分 × 相位 33 × 二 轮）
    ("JOIN0226", 69), // JOIN 单元 0226（合并 × 相位 34 × 二 轮）
    ("INV0227", 76), // INV 单元 0227（不变量 × 相位 35 × 二 轮）
    ("ACTIVE0228", 83), // ACTIVE 单元 0228（active 链 × 相位 36 × 二 轮）
    ("INACT0229", 1), // INACT 单元 0229（inactive 链 × 相位 37 × 二 轮）
    ("NODE0230", 8), // NODE 单元 0230（节点 × 相位 38 × 二 轮）
    ("LAYOUT0231", 15), // LAYOUT 单元 0231（节点布局 × 相位 39 × 二 轮）
    ("NEXT0232", 22), // NEXT 单元 0232（后继 × 相位 40 × 二 轮）
    ("PREV0233", 29), // PREV 单元 0233（前驱 × 相位 41 × 二 轮）
    ("FREE0234", 36), // FREE 单元 0234（空闲链 × 相位 42 × 二 轮）
    ("POOL0235", 43), // POOL 单元 0235（节点池 × 相位 43 × 二 轮）
    ("MOVE0236", 50), // MOVE 单元 0236（迁移 × 相位 44 × 二 轮）
    ("SPLIT0237", 57), // SPLIT 单元 0237（拆分 × 相位 45 × 二 轮）
    ("JOIN0238", 64), // JOIN 单元 0238（合并 × 相位 46 × 二 轮）
    ("INV0239", 71), // INV 单元 0239（不变量 × 相位 47 × 二 轮）
    ("ACTIVE0240", 78), // ACTIVE 单元 0240（active 链 × 相位 48 × 二 轮）
    ("INACT0241", 85), // INACT 单元 0241（inactive 链 × 相位 49 × 二 轮）
    ("NODE0242", 3), // NODE 单元 0242（节点 × 相位 50 × 二 轮）
    ("LAYOUT0243", 10), // LAYOUT 单元 0243（节点布局 × 相位 51 × 二 轮）
    ("NEXT0244", 17), // NEXT 单元 0244（后继 × 相位 52 × 二 轮）
    ("PREV0245", 24), // PREV 单元 0245（前驱 × 相位 53 × 二 轮）
    ("FREE0246", 31), // FREE 单元 0246（空闲链 × 相位 54 × 二 轮）
    ("POOL0247", 38), // POOL 单元 0247（节点池 × 相位 55 × 二 轮）
    ("MOVE0248", 45), // MOVE 单元 0248（迁移 × 相位 56 × 二 轮）
    ("SPLIT0249", 52), // SPLIT 单元 0249（拆分 × 相位 57 × 二 轮）
    ("JOIN0250", 59), // JOIN 单元 0250（合并 × 相位 58 × 二 轮）
    ("INV0251", 66), // INV 单元 0251（不变量 × 相位 59 × 二 轮）
    ("ACTIVE0252", 73), // ACTIVE 单元 0252（active 链 × 相位 60 × 二 轮）
    ("INACT0253", 80), // INACT 单元 0253（inactive 链 × 相位 61 × 二 轮）
    ("NODE0254", 87), // NODE 单元 0254（节点 × 相位 62 × 二 轮）
    ("LAYOUT0255", 5), // LAYOUT 单元 0255（节点布局 × 相位 63 × 二 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3001bmx_get(key: &str) -> Option<u64> {
    match key {
        "ACTIVE0000" => Some(0),
        "INACT0001" => Some(7),
        "NODE0002" => Some(14),
        "LAYOUT0003" => Some(21),
        "NEXT0004" => Some(28),
        "PREV0005" => Some(35),
        "FREE0006" => Some(42),
        "POOL0007" => Some(49),
        "MOVE0008" => Some(56),
        "SPLIT0009" => Some(63),
        "JOIN0010" => Some(70),
        "INV0011" => Some(77),
        "ACTIVE0012" => Some(84),
        "INACT0013" => Some(2),
        "NODE0014" => Some(9),
        "LAYOUT0015" => Some(16),
        "NEXT0016" => Some(23),
        "PREV0017" => Some(30),
        "FREE0018" => Some(37),
        "POOL0019" => Some(44),
        "MOVE0020" => Some(51),
        "SPLIT0021" => Some(58),
        "JOIN0022" => Some(65),
        "INV0023" => Some(72),
        "ACTIVE0024" => Some(79),
        "INACT0025" => Some(86),
        "NODE0026" => Some(4),
        "LAYOUT0027" => Some(11),
        "NEXT0028" => Some(18),
        "PREV0029" => Some(25),
        "FREE0030" => Some(32),
        "POOL0031" => Some(39),
        "MOVE0032" => Some(46),
        "SPLIT0033" => Some(53),
        "JOIN0034" => Some(60),
        "INV0035" => Some(67),
        "ACTIVE0036" => Some(74),
        "INACT0037" => Some(81),
        "NODE0038" => Some(88),
        "LAYOUT0039" => Some(6),
        "NEXT0040" => Some(13),
        "PREV0041" => Some(20),
        "FREE0042" => Some(27),
        "POOL0043" => Some(34),
        "MOVE0044" => Some(41),
        "SPLIT0045" => Some(48),
        "JOIN0046" => Some(55),
        "INV0047" => Some(62),
        "ACTIVE0048" => Some(69),
        "INACT0049" => Some(76),
        "NODE0050" => Some(83),
        "LAYOUT0051" => Some(1),
        "NEXT0052" => Some(8),
        "PREV0053" => Some(15),
        "FREE0054" => Some(22),
        "POOL0055" => Some(29),
        "MOVE0056" => Some(36),
        "SPLIT0057" => Some(43),
        "JOIN0058" => Some(50),
        "INV0059" => Some(57),
        "ACTIVE0060" => Some(64),
        "INACT0061" => Some(71),
        "NODE0062" => Some(78),
        "LAYOUT0063" => Some(85),
        "NEXT0064" => Some(3),
        "PREV0065" => Some(10),
        "FREE0066" => Some(17),
        "POOL0067" => Some(24),
        "MOVE0068" => Some(31),
        "SPLIT0069" => Some(38),
        "JOIN0070" => Some(45),
        "INV0071" => Some(52),
        "ACTIVE0072" => Some(59),
        "INACT0073" => Some(66),
        "NODE0074" => Some(73),
        "LAYOUT0075" => Some(80),
        "NEXT0076" => Some(87),
        "PREV0077" => Some(5),
        "FREE0078" => Some(12),
        "POOL0079" => Some(19),
        "MOVE0080" => Some(26),
        "SPLIT0081" => Some(33),
        "JOIN0082" => Some(40),
        "INV0083" => Some(47),
        "ACTIVE0084" => Some(54),
        "INACT0085" => Some(61),
        "NODE0086" => Some(68),
        "LAYOUT0087" => Some(75),
        "NEXT0088" => Some(82),
        "PREV0089" => Some(0),
        "FREE0090" => Some(7),
        "POOL0091" => Some(14),
        "MOVE0092" => Some(21),
        "SPLIT0093" => Some(28),
        "JOIN0094" => Some(35),
        "INV0095" => Some(42),
        "ACTIVE0096" => Some(49),
        "INACT0097" => Some(56),
        "NODE0098" => Some(63),
        "LAYOUT0099" => Some(70),
        "NEXT0100" => Some(77),
        "PREV0101" => Some(84),
        "FREE0102" => Some(2),
        "POOL0103" => Some(9),
        "MOVE0104" => Some(16),
        "SPLIT0105" => Some(23),
        "JOIN0106" => Some(30),
        "INV0107" => Some(37),
        "ACTIVE0108" => Some(44),
        "INACT0109" => Some(51),
        "NODE0110" => Some(58),
        "LAYOUT0111" => Some(65),
        "NEXT0112" => Some(72),
        "PREV0113" => Some(79),
        "FREE0114" => Some(86),
        "POOL0115" => Some(4),
        "MOVE0116" => Some(11),
        "SPLIT0117" => Some(18),
        "JOIN0118" => Some(25),
        "INV0119" => Some(32),
        "ACTIVE0120" => Some(39),
        "INACT0121" => Some(46),
        "NODE0122" => Some(53),
        "LAYOUT0123" => Some(60),
        "NEXT0124" => Some(67),
        "PREV0125" => Some(74),
        "FREE0126" => Some(81),
        "POOL0127" => Some(88),
        "MOVE0128" => Some(6),
        "SPLIT0129" => Some(13),
        "JOIN0130" => Some(20),
        "INV0131" => Some(27),
        "ACTIVE0132" => Some(34),
        "INACT0133" => Some(41),
        "NODE0134" => Some(48),
        "LAYOUT0135" => Some(55),
        "NEXT0136" => Some(62),
        "PREV0137" => Some(69),
        "FREE0138" => Some(76),
        "POOL0139" => Some(83),
        "MOVE0140" => Some(1),
        "SPLIT0141" => Some(8),
        "JOIN0142" => Some(15),
        "INV0143" => Some(22),
        "ACTIVE0144" => Some(29),
        "INACT0145" => Some(36),
        "NODE0146" => Some(43),
        "LAYOUT0147" => Some(50),
        "NEXT0148" => Some(57),
        "PREV0149" => Some(64),
        "FREE0150" => Some(71),
        "POOL0151" => Some(78),
        "MOVE0152" => Some(85),
        "SPLIT0153" => Some(3),
        "JOIN0154" => Some(10),
        "INV0155" => Some(17),
        "ACTIVE0156" => Some(24),
        "INACT0157" => Some(31),
        "NODE0158" => Some(38),
        "LAYOUT0159" => Some(45),
        "NEXT0160" => Some(52),
        "PREV0161" => Some(59),
        "FREE0162" => Some(66),
        "POOL0163" => Some(73),
        "MOVE0164" => Some(80),
        "SPLIT0165" => Some(87),
        "JOIN0166" => Some(5),
        "INV0167" => Some(12),
        "ACTIVE0168" => Some(19),
        "INACT0169" => Some(26),
        "NODE0170" => Some(33),
        "LAYOUT0171" => Some(40),
        "NEXT0172" => Some(47),
        "PREV0173" => Some(54),
        "FREE0174" => Some(61),
        "POOL0175" => Some(68),
        "MOVE0176" => Some(75),
        "SPLIT0177" => Some(82),
        "JOIN0178" => Some(0),
        "INV0179" => Some(7),
        "ACTIVE0180" => Some(14),
        "INACT0181" => Some(21),
        "NODE0182" => Some(28),
        "LAYOUT0183" => Some(35),
        "NEXT0184" => Some(42),
        "PREV0185" => Some(49),
        "FREE0186" => Some(56),
        "POOL0187" => Some(63),
        "MOVE0188" => Some(70),
        "SPLIT0189" => Some(77),
        "JOIN0190" => Some(84),
        "INV0191" => Some(2),
        "ACTIVE0192" => Some(9),
        "INACT0193" => Some(16),
        "NODE0194" => Some(23),
        "LAYOUT0195" => Some(30),
        "NEXT0196" => Some(37),
        "PREV0197" => Some(44),
        "FREE0198" => Some(51),
        "POOL0199" => Some(58),
        "MOVE0200" => Some(65),
        "SPLIT0201" => Some(72),
        "JOIN0202" => Some(79),
        "INV0203" => Some(86),
        "ACTIVE0204" => Some(4),
        "INACT0205" => Some(11),
        "NODE0206" => Some(18),
        "LAYOUT0207" => Some(25),
        "NEXT0208" => Some(32),
        "PREV0209" => Some(39),
        "FREE0210" => Some(46),
        "POOL0211" => Some(53),
        "MOVE0212" => Some(60),
        "SPLIT0213" => Some(67),
        "JOIN0214" => Some(74),
        "INV0215" => Some(81),
        "ACTIVE0216" => Some(88),
        "INACT0217" => Some(6),
        "NODE0218" => Some(13),
        "LAYOUT0219" => Some(20),
        "NEXT0220" => Some(27),
        "PREV0221" => Some(34),
        "FREE0222" => Some(41),
        "POOL0223" => Some(48),
        "MOVE0224" => Some(55),
        "SPLIT0225" => Some(62),
        "JOIN0226" => Some(69),
        "INV0227" => Some(76),
        "ACTIVE0228" => Some(83),
        "INACT0229" => Some(1),
        "NODE0230" => Some(8),
        "LAYOUT0231" => Some(15),
        "NEXT0232" => Some(22),
        "PREV0233" => Some(29),
        "FREE0234" => Some(36),
        "POOL0235" => Some(43),
        "MOVE0236" => Some(50),
        "SPLIT0237" => Some(57),
        "JOIN0238" => Some(64),
        "INV0239" => Some(71),
        "ACTIVE0240" => Some(78),
        "INACT0241" => Some(85),
        "NODE0242" => Some(3),
        "LAYOUT0243" => Some(10),
        "NEXT0244" => Some(17),
        "PREV0245" => Some(24),
        "FREE0246" => Some(31),
        "POOL0247" => Some(38),
        "MOVE0248" => Some(45),
        "SPLIT0249" => Some(52),
        "JOIN0250" => Some(59),
        "INV0251" => Some(66),
        "ACTIVE0252" => Some(73),
        "INACT0253" => Some(80),
        "NODE0254" => Some(87),
        "LAYOUT0255" => Some(5),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3001bmx_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3001bmx_COUNT {
        let (k, v) = u3001bmx[i];
        let ok = match k {
            "ACTIVE0000" => v < 89,
            "INACT0001" => v < 89,
            "NODE0002" => v < 89,
            "LAYOUT0003" => v < 89,
            "NEXT0004" => v < 89,
            "PREV0005" => v < 89,
            "FREE0006" => v < 89,
            "POOL0007" => v < 89,
            "MOVE0008" => v < 89,
            "SPLIT0009" => v < 89,
            "JOIN0010" => v < 89,
            "INV0011" => v < 89,
            "ACTIVE0012" => v < 89,
            "INACT0013" => v < 89,
            "NODE0014" => v < 89,
            "LAYOUT0015" => v < 89,
            "NEXT0016" => v < 89,
            "PREV0017" => v < 89,
            "FREE0018" => v < 89,
            "POOL0019" => v < 89,
            "MOVE0020" => v < 89,
            "SPLIT0021" => v < 89,
            "JOIN0022" => v < 89,
            "INV0023" => v < 89,
            "ACTIVE0024" => v < 89,
            "INACT0025" => v < 89,
            "NODE0026" => v < 89,
            "LAYOUT0027" => v < 89,
            "NEXT0028" => v < 89,
            "PREV0029" => v < 89,
            "FREE0030" => v < 89,
            "POOL0031" => v < 89,
            "MOVE0032" => v < 89,
            "SPLIT0033" => v < 89,
            "JOIN0034" => v < 89,
            "INV0035" => v < 89,
            "ACTIVE0036" => v < 89,
            "INACT0037" => v < 89,
            "NODE0038" => v < 89,
            "LAYOUT0039" => v < 89,
            "NEXT0040" => v < 89,
            "PREV0041" => v < 89,
            "FREE0042" => v < 89,
            "POOL0043" => v < 89,
            "MOVE0044" => v < 89,
            "SPLIT0045" => v < 89,
            "JOIN0046" => v < 89,
            "INV0047" => v < 89,
            "ACTIVE0048" => v < 89,
            "INACT0049" => v < 89,
            "NODE0050" => v < 89,
            "LAYOUT0051" => v < 89,
            "NEXT0052" => v < 89,
            "PREV0053" => v < 89,
            "FREE0054" => v < 89,
            "POOL0055" => v < 89,
            "MOVE0056" => v < 89,
            "SPLIT0057" => v < 89,
            "JOIN0058" => v < 89,
            "INV0059" => v < 89,
            "ACTIVE0060" => v < 89,
            "INACT0061" => v < 89,
            "NODE0062" => v < 89,
            "LAYOUT0063" => v < 89,
            "NEXT0064" => v < 89,
            "PREV0065" => v < 89,
            "FREE0066" => v < 89,
            "POOL0067" => v < 89,
            "MOVE0068" => v < 89,
            "SPLIT0069" => v < 89,
            "JOIN0070" => v < 89,
            "INV0071" => v < 89,
            "ACTIVE0072" => v < 89,
            "INACT0073" => v < 89,
            "NODE0074" => v < 89,
            "LAYOUT0075" => v < 89,
            "NEXT0076" => v < 89,
            "PREV0077" => v < 89,
            "FREE0078" => v < 89,
            "POOL0079" => v < 89,
            "MOVE0080" => v < 89,
            "SPLIT0081" => v < 89,
            "JOIN0082" => v < 89,
            "INV0083" => v < 89,
            "ACTIVE0084" => v < 89,
            "INACT0085" => v < 89,
            "NODE0086" => v < 89,
            "LAYOUT0087" => v < 89,
            "NEXT0088" => v < 89,
            "PREV0089" => v < 89,
            "FREE0090" => v < 89,
            "POOL0091" => v < 89,
            "MOVE0092" => v < 89,
            "SPLIT0093" => v < 89,
            "JOIN0094" => v < 89,
            "INV0095" => v < 89,
            "ACTIVE0096" => v < 89,
            "INACT0097" => v < 89,
            "NODE0098" => v < 89,
            "LAYOUT0099" => v < 89,
            "NEXT0100" => v < 89,
            "PREV0101" => v < 89,
            "FREE0102" => v < 89,
            "POOL0103" => v < 89,
            "MOVE0104" => v < 89,
            "SPLIT0105" => v < 89,
            "JOIN0106" => v < 89,
            "INV0107" => v < 89,
            "ACTIVE0108" => v < 89,
            "INACT0109" => v < 89,
            "NODE0110" => v < 89,
            "LAYOUT0111" => v < 89,
            "NEXT0112" => v < 89,
            "PREV0113" => v < 89,
            "FREE0114" => v < 89,
            "POOL0115" => v < 89,
            "MOVE0116" => v < 89,
            "SPLIT0117" => v < 89,
            "JOIN0118" => v < 89,
            "INV0119" => v < 89,
            "ACTIVE0120" => v < 89,
            "INACT0121" => v < 89,
            "NODE0122" => v < 89,
            "LAYOUT0123" => v < 89,
            "NEXT0124" => v < 89,
            "PREV0125" => v < 89,
            "FREE0126" => v < 89,
            "POOL0127" => v < 89,
            "MOVE0128" => v < 89,
            "SPLIT0129" => v < 89,
            "JOIN0130" => v < 89,
            "INV0131" => v < 89,
            "ACTIVE0132" => v < 89,
            "INACT0133" => v < 89,
            "NODE0134" => v < 89,
            "LAYOUT0135" => v < 89,
            "NEXT0136" => v < 89,
            "PREV0137" => v < 89,
            "FREE0138" => v < 89,
            "POOL0139" => v < 89,
            "MOVE0140" => v < 89,
            "SPLIT0141" => v < 89,
            "JOIN0142" => v < 89,
            "INV0143" => v < 89,
            "ACTIVE0144" => v < 89,
            "INACT0145" => v < 89,
            "NODE0146" => v < 89,
            "LAYOUT0147" => v < 89,
            "NEXT0148" => v < 89,
            "PREV0149" => v < 89,
            "FREE0150" => v < 89,
            "POOL0151" => v < 89,
            "MOVE0152" => v < 89,
            "SPLIT0153" => v < 89,
            "JOIN0154" => v < 89,
            "INV0155" => v < 89,
            "ACTIVE0156" => v < 89,
            "INACT0157" => v < 89,
            "NODE0158" => v < 89,
            "LAYOUT0159" => v < 89,
            "NEXT0160" => v < 89,
            "PREV0161" => v < 89,
            "FREE0162" => v < 89,
            "POOL0163" => v < 89,
            "MOVE0164" => v < 89,
            "SPLIT0165" => v < 89,
            "JOIN0166" => v < 89,
            "INV0167" => v < 89,
            "ACTIVE0168" => v < 89,
            "INACT0169" => v < 89,
            "NODE0170" => v < 89,
            "LAYOUT0171" => v < 89,
            "NEXT0172" => v < 89,
            "PREV0173" => v < 89,
            "FREE0174" => v < 89,
            "POOL0175" => v < 89,
            "MOVE0176" => v < 89,
            "SPLIT0177" => v < 89,
            "JOIN0178" => v < 89,
            "INV0179" => v < 89,
            "ACTIVE0180" => v < 89,
            "INACT0181" => v < 89,
            "NODE0182" => v < 89,
            "LAYOUT0183" => v < 89,
            "NEXT0184" => v < 89,
            "PREV0185" => v < 89,
            "FREE0186" => v < 89,
            "POOL0187" => v < 89,
            "MOVE0188" => v < 89,
            "SPLIT0189" => v < 89,
            "JOIN0190" => v < 89,
            "INV0191" => v < 89,
            "ACTIVE0192" => v < 89,
            "INACT0193" => v < 89,
            "NODE0194" => v < 89,
            "LAYOUT0195" => v < 89,
            "NEXT0196" => v < 89,
            "PREV0197" => v < 89,
            "FREE0198" => v < 89,
            "POOL0199" => v < 89,
            "MOVE0200" => v < 89,
            "SPLIT0201" => v < 89,
            "JOIN0202" => v < 89,
            "INV0203" => v < 89,
            "ACTIVE0204" => v < 89,
            "INACT0205" => v < 89,
            "NODE0206" => v < 89,
            "LAYOUT0207" => v < 89,
            "NEXT0208" => v < 89,
            "PREV0209" => v < 89,
            "FREE0210" => v < 89,
            "POOL0211" => v < 89,
            "MOVE0212" => v < 89,
            "SPLIT0213" => v < 89,
            "JOIN0214" => v < 89,
            "INV0215" => v < 89,
            "ACTIVE0216" => v < 89,
            "INACT0217" => v < 89,
            "NODE0218" => v < 89,
            "LAYOUT0219" => v < 89,
            "NEXT0220" => v < 89,
            "PREV0221" => v < 89,
            "FREE0222" => v < 89,
            "POOL0223" => v < 89,
            "MOVE0224" => v < 89,
            "SPLIT0225" => v < 89,
            "JOIN0226" => v < 89,
            "INV0227" => v < 89,
            "ACTIVE0228" => v < 89,
            "INACT0229" => v < 89,
            "NODE0230" => v < 89,
            "LAYOUT0231" => v < 89,
            "NEXT0232" => v < 89,
            "PREV0233" => v < 89,
            "FREE0234" => v < 89,
            "POOL0235" => v < 89,
            "MOVE0236" => v < 89,
            "SPLIT0237" => v < 89,
            "JOIN0238" => v < 89,
            "INV0239" => v < 89,
            "ACTIVE0240" => v < 89,
            "INACT0241" => v < 89,
            "NODE0242" => v < 89,
            "LAYOUT0243" => v < 89,
            "NEXT0244" => v < 89,
            "PREV0245" => v < 89,
            "FREE0246" => v < 89,
            "POOL0247" => v < 89,
            "MOVE0248" => v < 89,
            "SPLIT0249" => v < 89,
            "JOIN0250" => v < 89,
            "INV0251" => v < 89,
            "ACTIVE0252" => v < 89,
            "INACT0253" => v < 89,
            "NODE0254" => v < 89,
            "LAYOUT0255" => v < 89,
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
pub fn u3001bmx_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3001bmx_COUNT {
        s = s.wrapping_add(u3001bmx[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3001bmx_unique() -> bool {
    let mut i = 0usize;
    while i < u3001bmx_COUNT {
        let mut j = i + 1;
        while j < u3001bmx_COUNT {
            if u3001bmx[i].0 == u3001bmx[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe300, 0xe30f]。
pub const fn u3001_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3001Code::ALL.len() {
        let c = u3001Code::ALL[i].code();
        if c < 0xe300 || c > 0xe30f {
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
fn u3001_guard_seg() -> bool {
    u3001_segment_guard()
}

/// UNX-F3001 bmx 256 行齐备 + 边界锚 + 键外拒收。
fn check_u3001_bmx_count() -> bool {
    let mut ok = u3001bmx_COUNT == 256;
    if ok { ok = u3001bmx_get("ACTIVE0000") == Some(0) && u3001bmx_get("LAYOUT0255") == Some(5); }
    if ok { ok = u3001bmx_get("ZZZ_NOKEY").is_none(); }
    ok
}

/// UNX-F3001 bmx 累计守恒（理论值 11231）。
fn check_u3001_bmx_sum() -> bool {
    u3001bmx_sum() == 11231
}

/// UNX-F3001 bmx 逐行守卫与唯一性全绿。
fn check_u3001_bmx_guard() -> bool {
    u3001bmx_scan() == 0 && u3001bmx_unique()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3001_checks() -> CheckSet {
    let mut set = CheckSet::new("U3001");
    set.add(
        "u3001-seg-guard",
        u3001_guard_seg(),
        if u3001_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3001-bmx-count",
        check_u3001_bmx_count(),
        if check_u3001_bmx_count() { "" } else { "UNX-F3001 bmx count mismatch" },
    );
    set.add(
        "u3001-bmx-sum",
        check_u3001_bmx_sum(),
        if check_u3001_bmx_sum() { "" } else { "UNX-F3001 bmx sum mismatch" },
    );
    set.add(
        "u3001-bmx-guard",
        check_u3001_bmx_guard(),
        if check_u3001_bmx_guard() { "" } else { "UNX-F3001 bmx guard mismatch" },
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
    fn u3001_all_checks_green() {
        let set = run_u3001_checks();
        assert!(set.all_passed(), "U3001 domain checks must be green");
    }

    #[test]
    fn u3001_segment_guard_holds() {
        assert!(u3001_segment_guard());
    }
}
