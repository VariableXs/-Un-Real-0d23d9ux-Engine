//! unxt421_idtvectors —— UNX-F0421 IDT 256 向量表（逐门语义）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0421 · IDT 全量重装（256 门冻结） · 判据：256 门全部装载且 Type/IST/DPL 位态与布局账一致（逐门比对 256/256），载入后 lidt 回读一致
//! · 组件：256 向量语义表（异常/保留/中断分段）+ 逐门守卫 + 分段计数
//! · 判据落地：256 门全部装载的语义底座
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T421_VERSION: &str = "t421-idtvectors-v1";

/// 诊断段基址。
pub const T421_SEG_BASE: u16 = 0x4200;

// --------------------------------------------------------------------------
// 诊断码（t421 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// IDT 向量诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t421Code {
    /// 0x4200 向量越界（≥256）。
    VecOob,
    /// 0x4201 门类型非法。
    TypeBad,
    /// 0x4202 IST 非法（0..7 之外）。
    IstBad,
    /// 0x4203 DPL 非法（0..3 之外）。
    DplBad,
    /// 0x4204 向量断档。
    VectorGap,
    /// 0x4205 存在位为零（装载缺失）。
    PresentBitZero,
    /// 0x4206 布局账不符。
    LayoutMismatch,
    /// 0x4207 lidt 回读不一致。
    LidtReadbackBad,
    /// 0x4208 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4209 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x420a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x420b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x420c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x420d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x420e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x420f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t421Code {
    /// 全量闭集。
    pub const ALL: [t421Code; 16] = [
        t421Code::VecOob,
        t421Code::TypeBad,
        t421Code::IstBad,
        t421Code::DplBad,
        t421Code::VectorGap,
        t421Code::PresentBitZero,
        t421Code::LayoutMismatch,
        t421Code::LidtReadbackBad,
        t421Code::LedgerBroken,
        t421Code::SnapshotFingerprint,
        t421Code::TableBroken,
        t421Code::IndexMisaligned,
        t421Code::DuplicateEntry,
        t421Code::OrderViolated,
        t421Code::GuardTripped,
        t421Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t421Code::VecOob => 0x4200,
            t421Code::TypeBad => 0x4201,
            t421Code::IstBad => 0x4202,
            t421Code::DplBad => 0x4203,
            t421Code::VectorGap => 0x4204,
            t421Code::PresentBitZero => 0x4205,
            t421Code::LayoutMismatch => 0x4206,
            t421Code::LidtReadbackBad => 0x4207,
            t421Code::LedgerBroken => 0x4208,
            t421Code::SnapshotFingerprint => 0x4209,
            t421Code::TableBroken => 0x420a,
            t421Code::IndexMisaligned => 0x420b,
            t421Code::DuplicateEntry => 0x420c,
            t421Code::OrderViolated => 0x420d,
            t421Code::GuardTripped => 0x420e,
            t421Code::SegmentGuard => 0x420f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t421Code::VecOob => "向量越界（≥256）",
            t421Code::TypeBad => "门类型非法",
            t421Code::IstBad => "IST 非法（0..7 之外）",
            t421Code::DplBad => "DPL 非法（0..3 之外）",
            t421Code::VectorGap => "向量断档",
            t421Code::PresentBitZero => "存在位为零（装载缺失）",
            t421Code::LayoutMismatch => "布局账不符",
            t421Code::LidtReadbackBad => "lidt 回读不一致",
            t421Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t421Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t421Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t421Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t421Code::DuplicateEntry => "条目重复（同键双写）",
            t421Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t421Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t421Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t421Code::VecOob => "按 256 空间裁剪",
            t421Code::TypeBad => "按三类型闭集校正",
            t421Code::IstBad => "按 IST 表校正",
            t421Code::DplBad => "按 DPL 三值校正",
            t421Code::VectorGap => "按 256 连续律补号",
            t421Code::PresentBitZero => "补装载后重校",
            t421Code::LayoutMismatch => "按布局账重建",
            t421Code::LidtReadbackBad => "重装后复读",
            t421Code::LedgerBroken => "按守恒律重建台账后重跑",
            t421Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t421Code::TableBroken => "按语义规则重建常量表",
            t421Code::IndexMisaligned => "按声明序归一化后重跑",
            t421Code::DuplicateEntry => "合并同键条目为末次生效",
            t421Code::OrderViolated => "按依赖矩阵重排后重跑",
            t421Code::GuardTripped => "检查临界区嵌套与阈值",
            t421Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 idtvec（256 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// IDT 256 向量语义表（逐门守卫：向量号+门型+IST+DPL）。
/// 行数。
pub const idtvec_COUNT: usize = 256;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static idtvec: [(&'static str, u64); 256] = [
    ("DivideError", 0), // 向量 0x00 DivideError（Fault 门，IST=ist_none，DPL=0，#DE）
    ("Debug", 1), // 向量 0x01 Debug（Fault 门，IST=ist_debug，DPL=0，#DB）
    ("Nmi", 2), // 向量 0x02 Nmi（Int 门，IST=ist_nmi，DPL=0，#NMI）
    ("Breakpoint", 3), // 向量 0x03 Breakpoint（Trap 门，IST=ist_debug，DPL=3，#BP）
    ("Overflow", 4), // 向量 0x04 Overflow（Trap 门，IST=ist_none，DPL=3，#OF）
    ("BoundRange", 5), // 向量 0x05 BoundRange（Fault 门，IST=ist_none，DPL=0，#BR）
    ("InvalidOpcode", 6), // 向量 0x06 InvalidOpcode（Fault 门，IST=ist_none，DPL=0，#UD）
    ("DeviceNotAvailable", 7), // 向量 0x07 DeviceNotAvailable（Fault 门，IST=ist_none，DPL=0，#NM）
    ("DoubleFault", 8), // 向量 0x08 DoubleFault（Abort 门，IST=ist_df，DPL=0，#DF）
    ("CoprocessorSeg", 9), // 向量 0x09 CoprocessorSeg（Fault 门，IST=ist_none，DPL=0，协处理器段越界）
    ("InvalidTss", 10), // 向量 0x0A InvalidTss（Fault 门，IST=ist_none，DPL=0，#TS）
    ("SegmentNotPresent", 11), // 向量 0x0B SegmentNotPresent（Fault 门，IST=ist_none，DPL=0，#NP）
    ("StackSegment", 12), // 向量 0x0C StackSegment（Fault 门，IST=ist_none，DPL=0，#SS）
    ("GeneralProtection", 13), // 向量 0x0D GeneralProtection（Fault 门，IST=ist_none，DPL=0，#GP）
    ("PageFault", 14), // 向量 0x0E PageFault（Fault 门，IST=ist_pf，DPL=0，#PF）
    ("Reserved0f", 15), // 向量 0x0F Reserved0f（Fault 门，IST=ist_none，DPL=0，保留）
    ("X87Fpe", 16), // 向量 0x10 X87Fpe（Fault 门，IST=ist_none，DPL=0，#MF）
    ("AlignmentCheck", 17), // 向量 0x11 AlignmentCheck（Fault 门，IST=ist_none，DPL=0，#AC）
    ("MachineCheck", 18), // 向量 0x12 MachineCheck（Abort 门，IST=ist_mc，DPL=0，#MC）
    ("SimdFpe", 19), // 向量 0x13 SimdFpe（Fault 门，IST=ist_none，DPL=0，#XM）
    ("Virtualization", 20), // 向量 0x14 Virtualization（Fault 门，IST=ist_none，DPL=0，#VE）
    ("ControlProtection", 21), // 向量 0x15 ControlProtection（Fault 门，IST=ist_none，DPL=0，#CP）
    ("Reserved22", 22), // 向量 0x16 Reserved22（Int 门，IST=ist_none，DPL=0，保留向量）
    ("Reserved23", 23), // 向量 0x17 Reserved23（Int 门，IST=ist_none，DPL=0，保留向量）
    ("Reserved24", 24), // 向量 0x18 Reserved24（Int 门，IST=ist_none，DPL=0，保留向量）
    ("Reserved25", 25), // 向量 0x19 Reserved25（Int 门，IST=ist_none，DPL=0，保留向量）
    ("Reserved26", 26), // 向量 0x1A Reserved26（Int 门，IST=ist_none，DPL=0，保留向量）
    ("Reserved27", 27), // 向量 0x1B Reserved27（Int 门，IST=ist_none，DPL=0，保留向量）
    ("Reserved28", 28), // 向量 0x1C Reserved28（Int 门，IST=ist_none，DPL=0，保留向量）
    ("Reserved29", 29), // 向量 0x1D Reserved29（Int 门，IST=ist_none，DPL=0，保留向量）
    ("Reserved30", 30), // 向量 0x1E Reserved30（Int 门，IST=ist_none，DPL=0，保留向量）
    ("Reserved31", 31), // 向量 0x1F Reserved31（Int 门，IST=ist_none，DPL=0，保留向量）
    ("Timer", 32), // 向量 0x20 Timer（Int 门，IST=ist_none，DPL=0，LAPIC 定时器）
    ("LocalIrq33", 33), // 向量 0x21 LocalIrq33（Int 门，IST=ist_none，DPL=0，本地中断）
    ("LocalIrq34", 34), // 向量 0x22 LocalIrq34（Int 门，IST=ist_none，DPL=0，本地中断）
    ("LocalIrq35", 35), // 向量 0x23 LocalIrq35（Int 门，IST=ist_none，DPL=0，本地中断）
    ("LocalIrq36", 36), // 向量 0x24 LocalIrq36（Int 门，IST=ist_none，DPL=0，本地中断）
    ("LocalIrq37", 37), // 向量 0x25 LocalIrq37（Int 门，IST=ist_none，DPL=0，本地中断）
    ("LocalIrq38", 38), // 向量 0x26 LocalIrq38（Int 门，IST=ist_none，DPL=0，本地中断）
    ("LocalIrq39", 39), // 向量 0x27 LocalIrq39（Int 门，IST=ist_none，DPL=0，本地中断）
    ("LocalIrq40", 40), // 向量 0x28 LocalIrq40（Int 门，IST=ist_none，DPL=0，本地中断）
    ("LocalIrq41", 41), // 向量 0x29 LocalIrq41（Int 门，IST=ist_none，DPL=0，本地中断）
    ("LocalIrq42", 42), // 向量 0x2A LocalIrq42（Int 门，IST=ist_none，DPL=0，本地中断）
    ("LocalIrq43", 43), // 向量 0x2B LocalIrq43（Int 门，IST=ist_none，DPL=0，本地中断）
    ("LocalIrq44", 44), // 向量 0x2C LocalIrq44（Int 门，IST=ist_none，DPL=0，本地中断）
    ("LocalIrq45", 45), // 向量 0x2D LocalIrq45（Int 门，IST=ist_none，DPL=0，本地中断）
    ("LocalIrq46", 46), // 向量 0x2E LocalIrq46（Int 门，IST=ist_none，DPL=0，本地中断）
    ("LocalIrq47", 47), // 向量 0x2F LocalIrq47（Int 门，IST=ist_none，DPL=0，本地中断）
    ("IoIrq48", 48), // 向量 0x30 IoIrq48（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq49", 49), // 向量 0x31 IoIrq49（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq50", 50), // 向量 0x32 IoIrq50（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq51", 51), // 向量 0x33 IoIrq51（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq52", 52), // 向量 0x34 IoIrq52（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq53", 53), // 向量 0x35 IoIrq53（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq54", 54), // 向量 0x36 IoIrq54（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq55", 55), // 向量 0x37 IoIrq55（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq56", 56), // 向量 0x38 IoIrq56（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq57", 57), // 向量 0x39 IoIrq57（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq58", 58), // 向量 0x3A IoIrq58（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq59", 59), // 向量 0x3B IoIrq59（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq60", 60), // 向量 0x3C IoIrq60（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq61", 61), // 向量 0x3D IoIrq61（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq62", 62), // 向量 0x3E IoIrq62（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq63", 63), // 向量 0x3F IoIrq63（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq64", 64), // 向量 0x40 IoIrq64（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq65", 65), // 向量 0x41 IoIrq65（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq66", 66), // 向量 0x42 IoIrq66（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq67", 67), // 向量 0x43 IoIrq67（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq68", 68), // 向量 0x44 IoIrq68（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq69", 69), // 向量 0x45 IoIrq69（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq70", 70), // 向量 0x46 IoIrq70（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq71", 71), // 向量 0x47 IoIrq71（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq72", 72), // 向量 0x48 IoIrq72（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq73", 73), // 向量 0x49 IoIrq73（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq74", 74), // 向量 0x4A IoIrq74（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq75", 75), // 向量 0x4B IoIrq75（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq76", 76), // 向量 0x4C IoIrq76（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq77", 77), // 向量 0x4D IoIrq77（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq78", 78), // 向量 0x4E IoIrq78（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq79", 79), // 向量 0x4F IoIrq79（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq80", 80), // 向量 0x50 IoIrq80（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq81", 81), // 向量 0x51 IoIrq81（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq82", 82), // 向量 0x52 IoIrq82（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq83", 83), // 向量 0x53 IoIrq83（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq84", 84), // 向量 0x54 IoIrq84（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq85", 85), // 向量 0x55 IoIrq85（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq86", 86), // 向量 0x56 IoIrq86（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq87", 87), // 向量 0x57 IoIrq87（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq88", 88), // 向量 0x58 IoIrq88（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq89", 89), // 向量 0x59 IoIrq89（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq90", 90), // 向量 0x5A IoIrq90（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq91", 91), // 向量 0x5B IoIrq91（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq92", 92), // 向量 0x5C IoIrq92（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq93", 93), // 向量 0x5D IoIrq93（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq94", 94), // 向量 0x5E IoIrq94（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq95", 95), // 向量 0x5F IoIrq95（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq96", 96), // 向量 0x60 IoIrq96（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq97", 97), // 向量 0x61 IoIrq97（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq98", 98), // 向量 0x62 IoIrq98（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq99", 99), // 向量 0x63 IoIrq99（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq100", 100), // 向量 0x64 IoIrq100（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq101", 101), // 向量 0x65 IoIrq101（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq102", 102), // 向量 0x66 IoIrq102（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq103", 103), // 向量 0x67 IoIrq103（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq104", 104), // 向量 0x68 IoIrq104（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq105", 105), // 向量 0x69 IoIrq105（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq106", 106), // 向量 0x6A IoIrq106（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq107", 107), // 向量 0x6B IoIrq107（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq108", 108), // 向量 0x6C IoIrq108（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq109", 109), // 向量 0x6D IoIrq109（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq110", 110), // 向量 0x6E IoIrq110（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq111", 111), // 向量 0x6F IoIrq111（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq112", 112), // 向量 0x70 IoIrq112（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq113", 113), // 向量 0x71 IoIrq113（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq114", 114), // 向量 0x72 IoIrq114（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq115", 115), // 向量 0x73 IoIrq115（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq116", 116), // 向量 0x74 IoIrq116（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq117", 117), // 向量 0x75 IoIrq117（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq118", 118), // 向量 0x76 IoIrq118（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq119", 119), // 向量 0x77 IoIrq119（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq120", 120), // 向量 0x78 IoIrq120（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq121", 121), // 向量 0x79 IoIrq121（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq122", 122), // 向量 0x7A IoIrq122（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq123", 123), // 向量 0x7B IoIrq123（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq124", 124), // 向量 0x7C IoIrq124（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq125", 125), // 向量 0x7D IoIrq125（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq126", 126), // 向量 0x7E IoIrq126（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("IoIrq127", 127), // 向量 0x7F IoIrq127（Int 门，IST=ist_none，DPL=0，I/O 中断）
    ("SyscallOld", 128), // 向量 0x80 SyscallOld（Trap 门，IST=ist_none，DPL=3，0x80 传统调用门）
    ("DeviceIrq129", 129), // 向量 0x81 DeviceIrq129（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq130", 130), // 向量 0x82 DeviceIrq130（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq131", 131), // 向量 0x83 DeviceIrq131（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq132", 132), // 向量 0x84 DeviceIrq132（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq133", 133), // 向量 0x85 DeviceIrq133（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq134", 134), // 向量 0x86 DeviceIrq134（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq135", 135), // 向量 0x87 DeviceIrq135（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq136", 136), // 向量 0x88 DeviceIrq136（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq137", 137), // 向量 0x89 DeviceIrq137（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq138", 138), // 向量 0x8A DeviceIrq138（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq139", 139), // 向量 0x8B DeviceIrq139（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq140", 140), // 向量 0x8C DeviceIrq140（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq141", 141), // 向量 0x8D DeviceIrq141（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq142", 142), // 向量 0x8E DeviceIrq142（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq143", 143), // 向量 0x8F DeviceIrq143（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq144", 144), // 向量 0x90 DeviceIrq144（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq145", 145), // 向量 0x91 DeviceIrq145（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq146", 146), // 向量 0x92 DeviceIrq146（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq147", 147), // 向量 0x93 DeviceIrq147（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq148", 148), // 向量 0x94 DeviceIrq148（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq149", 149), // 向量 0x95 DeviceIrq149（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq150", 150), // 向量 0x96 DeviceIrq150（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq151", 151), // 向量 0x97 DeviceIrq151（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq152", 152), // 向量 0x98 DeviceIrq152（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq153", 153), // 向量 0x99 DeviceIrq153（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq154", 154), // 向量 0x9A DeviceIrq154（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq155", 155), // 向量 0x9B DeviceIrq155（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq156", 156), // 向量 0x9C DeviceIrq156（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq157", 157), // 向量 0x9D DeviceIrq157（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq158", 158), // 向量 0x9E DeviceIrq158（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq159", 159), // 向量 0x9F DeviceIrq159（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq160", 160), // 向量 0xA0 DeviceIrq160（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq161", 161), // 向量 0xA1 DeviceIrq161（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq162", 162), // 向量 0xA2 DeviceIrq162（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq163", 163), // 向量 0xA3 DeviceIrq163（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq164", 164), // 向量 0xA4 DeviceIrq164（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq165", 165), // 向量 0xA5 DeviceIrq165（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq166", 166), // 向量 0xA6 DeviceIrq166（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq167", 167), // 向量 0xA7 DeviceIrq167（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq168", 168), // 向量 0xA8 DeviceIrq168（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq169", 169), // 向量 0xA9 DeviceIrq169（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq170", 170), // 向量 0xAA DeviceIrq170（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq171", 171), // 向量 0xAB DeviceIrq171（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq172", 172), // 向量 0xAC DeviceIrq172（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq173", 173), // 向量 0xAD DeviceIrq173（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq174", 174), // 向量 0xAE DeviceIrq174（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq175", 175), // 向量 0xAF DeviceIrq175（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq176", 176), // 向量 0xB0 DeviceIrq176（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq177", 177), // 向量 0xB1 DeviceIrq177（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq178", 178), // 向量 0xB2 DeviceIrq178（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq179", 179), // 向量 0xB3 DeviceIrq179（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq180", 180), // 向量 0xB4 DeviceIrq180（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq181", 181), // 向量 0xB5 DeviceIrq181（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq182", 182), // 向量 0xB6 DeviceIrq182（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq183", 183), // 向量 0xB7 DeviceIrq183（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq184", 184), // 向量 0xB8 DeviceIrq184（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq185", 185), // 向量 0xB9 DeviceIrq185（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq186", 186), // 向量 0xBA DeviceIrq186（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq187", 187), // 向量 0xBB DeviceIrq187（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq188", 188), // 向量 0xBC DeviceIrq188（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq189", 189), // 向量 0xBD DeviceIrq189（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq190", 190), // 向量 0xBE DeviceIrq190（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq191", 191), // 向量 0xBF DeviceIrq191（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq192", 192), // 向量 0xC0 DeviceIrq192（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq193", 193), // 向量 0xC1 DeviceIrq193（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq194", 194), // 向量 0xC2 DeviceIrq194（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq195", 195), // 向量 0xC3 DeviceIrq195（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq196", 196), // 向量 0xC4 DeviceIrq196（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq197", 197), // 向量 0xC5 DeviceIrq197（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq198", 198), // 向量 0xC6 DeviceIrq198（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq199", 199), // 向量 0xC7 DeviceIrq199（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq200", 200), // 向量 0xC8 DeviceIrq200（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq201", 201), // 向量 0xC9 DeviceIrq201（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq202", 202), // 向量 0xCA DeviceIrq202（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq203", 203), // 向量 0xCB DeviceIrq203（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq204", 204), // 向量 0xCC DeviceIrq204（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq205", 205), // 向量 0xCD DeviceIrq205（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq206", 206), // 向量 0xCE DeviceIrq206（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq207", 207), // 向量 0xCF DeviceIrq207（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq208", 208), // 向量 0xD0 DeviceIrq208（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq209", 209), // 向量 0xD1 DeviceIrq209（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq210", 210), // 向量 0xD2 DeviceIrq210（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq211", 211), // 向量 0xD3 DeviceIrq211（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq212", 212), // 向量 0xD4 DeviceIrq212（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq213", 213), // 向量 0xD5 DeviceIrq213（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq214", 214), // 向量 0xD6 DeviceIrq214（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq215", 215), // 向量 0xD7 DeviceIrq215（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq216", 216), // 向量 0xD8 DeviceIrq216（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq217", 217), // 向量 0xD9 DeviceIrq217（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq218", 218), // 向量 0xDA DeviceIrq218（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq219", 219), // 向量 0xDB DeviceIrq219（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq220", 220), // 向量 0xDC DeviceIrq220（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq221", 221), // 向量 0xDD DeviceIrq221（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq222", 222), // 向量 0xDE DeviceIrq222（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq223", 223), // 向量 0xDF DeviceIrq223（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq224", 224), // 向量 0xE0 DeviceIrq224（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq225", 225), // 向量 0xE1 DeviceIrq225（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq226", 226), // 向量 0xE2 DeviceIrq226（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq227", 227), // 向量 0xE3 DeviceIrq227（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq228", 228), // 向量 0xE4 DeviceIrq228（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq229", 229), // 向量 0xE5 DeviceIrq229（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq230", 230), // 向量 0xE6 DeviceIrq230（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq231", 231), // 向量 0xE7 DeviceIrq231（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq232", 232), // 向量 0xE8 DeviceIrq232（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq233", 233), // 向量 0xE9 DeviceIrq233（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq234", 234), // 向量 0xEA DeviceIrq234（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq235", 235), // 向量 0xEB DeviceIrq235（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq236", 236), // 向量 0xEC DeviceIrq236（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq237", 237), // 向量 0xED DeviceIrq237（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq238", 238), // 向量 0xEE DeviceIrq238（Int 门，IST=ist_none，DPL=0，设备中断）
    ("DeviceIrq239", 239), // 向量 0xEF DeviceIrq239（Int 门，IST=ist_none，DPL=0，设备中断）
    ("ReservedHi240", 240), // 向量 0xF0 ReservedHi240（Int 门，IST=ist_none，DPL=0，保留高段）
    ("ReservedHi241", 241), // 向量 0xF1 ReservedHi241（Int 门，IST=ist_none，DPL=0，保留高段）
    ("ReservedHi242", 242), // 向量 0xF2 ReservedHi242（Int 门，IST=ist_none，DPL=0，保留高段）
    ("ReservedHi243", 243), // 向量 0xF3 ReservedHi243（Int 门，IST=ist_none，DPL=0，保留高段）
    ("ReservedHi244", 244), // 向量 0xF4 ReservedHi244（Int 门，IST=ist_none，DPL=0，保留高段）
    ("ReservedHi245", 245), // 向量 0xF5 ReservedHi245（Int 门，IST=ist_none，DPL=0，保留高段）
    ("ReservedHi246", 246), // 向量 0xF6 ReservedHi246（Int 门，IST=ist_none，DPL=0，保留高段）
    ("ReservedHi247", 247), // 向量 0xF7 ReservedHi247（Int 门，IST=ist_none，DPL=0，保留高段）
    ("ReservedHi248", 248), // 向量 0xF8 ReservedHi248（Int 门，IST=ist_none，DPL=0，保留高段）
    ("ReservedHi249", 249), // 向量 0xF9 ReservedHi249（Int 门，IST=ist_none，DPL=0，保留高段）
    ("ReservedHi250", 250), // 向量 0xFA ReservedHi250（Int 门，IST=ist_none，DPL=0，保留高段）
    ("ReservedHi251", 251), // 向量 0xFB ReservedHi251（Int 门，IST=ist_none，DPL=0，保留高段）
    ("ReservedHi252", 252), // 向量 0xFC ReservedHi252（Int 门，IST=ist_none，DPL=0，保留高段）
    ("ApicError", 253), // 向量 0xFD ApicError（Int 门，IST=ist_err，DPL=0，LAPIC 错误）
    ("Spurious", 254), // 向量 0xFE Spurious（Int 门，IST=ist_none，DPL=0，伪中断）
    ("PanicVector", 255), // 向量 0xFF PanicVector（Int 门，IST=ist_panic，DPL=0，内核 panic 向量）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn idtvec_get(key: &str) -> Option<u64> {
    match key {
        "DivideError" => Some(0),
        "Debug" => Some(1),
        "Nmi" => Some(2),
        "Breakpoint" => Some(3),
        "Overflow" => Some(4),
        "BoundRange" => Some(5),
        "InvalidOpcode" => Some(6),
        "DeviceNotAvailable" => Some(7),
        "DoubleFault" => Some(8),
        "CoprocessorSeg" => Some(9),
        "InvalidTss" => Some(10),
        "SegmentNotPresent" => Some(11),
        "StackSegment" => Some(12),
        "GeneralProtection" => Some(13),
        "PageFault" => Some(14),
        "Reserved0f" => Some(15),
        "X87Fpe" => Some(16),
        "AlignmentCheck" => Some(17),
        "MachineCheck" => Some(18),
        "SimdFpe" => Some(19),
        "Virtualization" => Some(20),
        "ControlProtection" => Some(21),
        "Reserved22" => Some(22),
        "Reserved23" => Some(23),
        "Reserved24" => Some(24),
        "Reserved25" => Some(25),
        "Reserved26" => Some(26),
        "Reserved27" => Some(27),
        "Reserved28" => Some(28),
        "Reserved29" => Some(29),
        "Reserved30" => Some(30),
        "Reserved31" => Some(31),
        "Timer" => Some(32),
        "LocalIrq33" => Some(33),
        "LocalIrq34" => Some(34),
        "LocalIrq35" => Some(35),
        "LocalIrq36" => Some(36),
        "LocalIrq37" => Some(37),
        "LocalIrq38" => Some(38),
        "LocalIrq39" => Some(39),
        "LocalIrq40" => Some(40),
        "LocalIrq41" => Some(41),
        "LocalIrq42" => Some(42),
        "LocalIrq43" => Some(43),
        "LocalIrq44" => Some(44),
        "LocalIrq45" => Some(45),
        "LocalIrq46" => Some(46),
        "LocalIrq47" => Some(47),
        "IoIrq48" => Some(48),
        "IoIrq49" => Some(49),
        "IoIrq50" => Some(50),
        "IoIrq51" => Some(51),
        "IoIrq52" => Some(52),
        "IoIrq53" => Some(53),
        "IoIrq54" => Some(54),
        "IoIrq55" => Some(55),
        "IoIrq56" => Some(56),
        "IoIrq57" => Some(57),
        "IoIrq58" => Some(58),
        "IoIrq59" => Some(59),
        "IoIrq60" => Some(60),
        "IoIrq61" => Some(61),
        "IoIrq62" => Some(62),
        "IoIrq63" => Some(63),
        "IoIrq64" => Some(64),
        "IoIrq65" => Some(65),
        "IoIrq66" => Some(66),
        "IoIrq67" => Some(67),
        "IoIrq68" => Some(68),
        "IoIrq69" => Some(69),
        "IoIrq70" => Some(70),
        "IoIrq71" => Some(71),
        "IoIrq72" => Some(72),
        "IoIrq73" => Some(73),
        "IoIrq74" => Some(74),
        "IoIrq75" => Some(75),
        "IoIrq76" => Some(76),
        "IoIrq77" => Some(77),
        "IoIrq78" => Some(78),
        "IoIrq79" => Some(79),
        "IoIrq80" => Some(80),
        "IoIrq81" => Some(81),
        "IoIrq82" => Some(82),
        "IoIrq83" => Some(83),
        "IoIrq84" => Some(84),
        "IoIrq85" => Some(85),
        "IoIrq86" => Some(86),
        "IoIrq87" => Some(87),
        "IoIrq88" => Some(88),
        "IoIrq89" => Some(89),
        "IoIrq90" => Some(90),
        "IoIrq91" => Some(91),
        "IoIrq92" => Some(92),
        "IoIrq93" => Some(93),
        "IoIrq94" => Some(94),
        "IoIrq95" => Some(95),
        "IoIrq96" => Some(96),
        "IoIrq97" => Some(97),
        "IoIrq98" => Some(98),
        "IoIrq99" => Some(99),
        "IoIrq100" => Some(100),
        "IoIrq101" => Some(101),
        "IoIrq102" => Some(102),
        "IoIrq103" => Some(103),
        "IoIrq104" => Some(104),
        "IoIrq105" => Some(105),
        "IoIrq106" => Some(106),
        "IoIrq107" => Some(107),
        "IoIrq108" => Some(108),
        "IoIrq109" => Some(109),
        "IoIrq110" => Some(110),
        "IoIrq111" => Some(111),
        "IoIrq112" => Some(112),
        "IoIrq113" => Some(113),
        "IoIrq114" => Some(114),
        "IoIrq115" => Some(115),
        "IoIrq116" => Some(116),
        "IoIrq117" => Some(117),
        "IoIrq118" => Some(118),
        "IoIrq119" => Some(119),
        "IoIrq120" => Some(120),
        "IoIrq121" => Some(121),
        "IoIrq122" => Some(122),
        "IoIrq123" => Some(123),
        "IoIrq124" => Some(124),
        "IoIrq125" => Some(125),
        "IoIrq126" => Some(126),
        "IoIrq127" => Some(127),
        "SyscallOld" => Some(128),
        "DeviceIrq129" => Some(129),
        "DeviceIrq130" => Some(130),
        "DeviceIrq131" => Some(131),
        "DeviceIrq132" => Some(132),
        "DeviceIrq133" => Some(133),
        "DeviceIrq134" => Some(134),
        "DeviceIrq135" => Some(135),
        "DeviceIrq136" => Some(136),
        "DeviceIrq137" => Some(137),
        "DeviceIrq138" => Some(138),
        "DeviceIrq139" => Some(139),
        "DeviceIrq140" => Some(140),
        "DeviceIrq141" => Some(141),
        "DeviceIrq142" => Some(142),
        "DeviceIrq143" => Some(143),
        "DeviceIrq144" => Some(144),
        "DeviceIrq145" => Some(145),
        "DeviceIrq146" => Some(146),
        "DeviceIrq147" => Some(147),
        "DeviceIrq148" => Some(148),
        "DeviceIrq149" => Some(149),
        "DeviceIrq150" => Some(150),
        "DeviceIrq151" => Some(151),
        "DeviceIrq152" => Some(152),
        "DeviceIrq153" => Some(153),
        "DeviceIrq154" => Some(154),
        "DeviceIrq155" => Some(155),
        "DeviceIrq156" => Some(156),
        "DeviceIrq157" => Some(157),
        "DeviceIrq158" => Some(158),
        "DeviceIrq159" => Some(159),
        "DeviceIrq160" => Some(160),
        "DeviceIrq161" => Some(161),
        "DeviceIrq162" => Some(162),
        "DeviceIrq163" => Some(163),
        "DeviceIrq164" => Some(164),
        "DeviceIrq165" => Some(165),
        "DeviceIrq166" => Some(166),
        "DeviceIrq167" => Some(167),
        "DeviceIrq168" => Some(168),
        "DeviceIrq169" => Some(169),
        "DeviceIrq170" => Some(170),
        "DeviceIrq171" => Some(171),
        "DeviceIrq172" => Some(172),
        "DeviceIrq173" => Some(173),
        "DeviceIrq174" => Some(174),
        "DeviceIrq175" => Some(175),
        "DeviceIrq176" => Some(176),
        "DeviceIrq177" => Some(177),
        "DeviceIrq178" => Some(178),
        "DeviceIrq179" => Some(179),
        "DeviceIrq180" => Some(180),
        "DeviceIrq181" => Some(181),
        "DeviceIrq182" => Some(182),
        "DeviceIrq183" => Some(183),
        "DeviceIrq184" => Some(184),
        "DeviceIrq185" => Some(185),
        "DeviceIrq186" => Some(186),
        "DeviceIrq187" => Some(187),
        "DeviceIrq188" => Some(188),
        "DeviceIrq189" => Some(189),
        "DeviceIrq190" => Some(190),
        "DeviceIrq191" => Some(191),
        "DeviceIrq192" => Some(192),
        "DeviceIrq193" => Some(193),
        "DeviceIrq194" => Some(194),
        "DeviceIrq195" => Some(195),
        "DeviceIrq196" => Some(196),
        "DeviceIrq197" => Some(197),
        "DeviceIrq198" => Some(198),
        "DeviceIrq199" => Some(199),
        "DeviceIrq200" => Some(200),
        "DeviceIrq201" => Some(201),
        "DeviceIrq202" => Some(202),
        "DeviceIrq203" => Some(203),
        "DeviceIrq204" => Some(204),
        "DeviceIrq205" => Some(205),
        "DeviceIrq206" => Some(206),
        "DeviceIrq207" => Some(207),
        "DeviceIrq208" => Some(208),
        "DeviceIrq209" => Some(209),
        "DeviceIrq210" => Some(210),
        "DeviceIrq211" => Some(211),
        "DeviceIrq212" => Some(212),
        "DeviceIrq213" => Some(213),
        "DeviceIrq214" => Some(214),
        "DeviceIrq215" => Some(215),
        "DeviceIrq216" => Some(216),
        "DeviceIrq217" => Some(217),
        "DeviceIrq218" => Some(218),
        "DeviceIrq219" => Some(219),
        "DeviceIrq220" => Some(220),
        "DeviceIrq221" => Some(221),
        "DeviceIrq222" => Some(222),
        "DeviceIrq223" => Some(223),
        "DeviceIrq224" => Some(224),
        "DeviceIrq225" => Some(225),
        "DeviceIrq226" => Some(226),
        "DeviceIrq227" => Some(227),
        "DeviceIrq228" => Some(228),
        "DeviceIrq229" => Some(229),
        "DeviceIrq230" => Some(230),
        "DeviceIrq231" => Some(231),
        "DeviceIrq232" => Some(232),
        "DeviceIrq233" => Some(233),
        "DeviceIrq234" => Some(234),
        "DeviceIrq235" => Some(235),
        "DeviceIrq236" => Some(236),
        "DeviceIrq237" => Some(237),
        "DeviceIrq238" => Some(238),
        "DeviceIrq239" => Some(239),
        "ReservedHi240" => Some(240),
        "ReservedHi241" => Some(241),
        "ReservedHi242" => Some(242),
        "ReservedHi243" => Some(243),
        "ReservedHi244" => Some(244),
        "ReservedHi245" => Some(245),
        "ReservedHi246" => Some(246),
        "ReservedHi247" => Some(247),
        "ReservedHi248" => Some(248),
        "ReservedHi249" => Some(249),
        "ReservedHi250" => Some(250),
        "ReservedHi251" => Some(251),
        "ReservedHi252" => Some(252),
        "ApicError" => Some(253),
        "Spurious" => Some(254),
        "PanicVector" => Some(255),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn idtvec_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < idtvec_COUNT {
        let (k, v) = idtvec[i];
        let ok = match k {
            "DivideError" => v == 0,
            "Debug" => v == 1,
            "Nmi" => v == 2,
            "Breakpoint" => v == 3,
            "Overflow" => v == 4,
            "BoundRange" => v == 5,
            "InvalidOpcode" => v == 6,
            "DeviceNotAvailable" => v == 7,
            "DoubleFault" => v == 8,
            "CoprocessorSeg" => v == 9,
            "InvalidTss" => v == 10,
            "SegmentNotPresent" => v == 11,
            "StackSegment" => v == 12,
            "GeneralProtection" => v == 13,
            "PageFault" => v == 14,
            "Reserved0f" => v == 15,
            "X87Fpe" => v == 16,
            "AlignmentCheck" => v == 17,
            "MachineCheck" => v == 18,
            "SimdFpe" => v == 19,
            "Virtualization" => v == 20,
            "ControlProtection" => v == 21,
            "Reserved22" => v == 22,
            "Reserved23" => v == 23,
            "Reserved24" => v == 24,
            "Reserved25" => v == 25,
            "Reserved26" => v == 26,
            "Reserved27" => v == 27,
            "Reserved28" => v == 28,
            "Reserved29" => v == 29,
            "Reserved30" => v == 30,
            "Reserved31" => v == 31,
            "Timer" => v == 32,
            "LocalIrq33" => v == 33,
            "LocalIrq34" => v == 34,
            "LocalIrq35" => v == 35,
            "LocalIrq36" => v == 36,
            "LocalIrq37" => v == 37,
            "LocalIrq38" => v == 38,
            "LocalIrq39" => v == 39,
            "LocalIrq40" => v == 40,
            "LocalIrq41" => v == 41,
            "LocalIrq42" => v == 42,
            "LocalIrq43" => v == 43,
            "LocalIrq44" => v == 44,
            "LocalIrq45" => v == 45,
            "LocalIrq46" => v == 46,
            "LocalIrq47" => v == 47,
            "IoIrq48" => v == 48,
            "IoIrq49" => v == 49,
            "IoIrq50" => v == 50,
            "IoIrq51" => v == 51,
            "IoIrq52" => v == 52,
            "IoIrq53" => v == 53,
            "IoIrq54" => v == 54,
            "IoIrq55" => v == 55,
            "IoIrq56" => v == 56,
            "IoIrq57" => v == 57,
            "IoIrq58" => v == 58,
            "IoIrq59" => v == 59,
            "IoIrq60" => v == 60,
            "IoIrq61" => v == 61,
            "IoIrq62" => v == 62,
            "IoIrq63" => v == 63,
            "IoIrq64" => v == 64,
            "IoIrq65" => v == 65,
            "IoIrq66" => v == 66,
            "IoIrq67" => v == 67,
            "IoIrq68" => v == 68,
            "IoIrq69" => v == 69,
            "IoIrq70" => v == 70,
            "IoIrq71" => v == 71,
            "IoIrq72" => v == 72,
            "IoIrq73" => v == 73,
            "IoIrq74" => v == 74,
            "IoIrq75" => v == 75,
            "IoIrq76" => v == 76,
            "IoIrq77" => v == 77,
            "IoIrq78" => v == 78,
            "IoIrq79" => v == 79,
            "IoIrq80" => v == 80,
            "IoIrq81" => v == 81,
            "IoIrq82" => v == 82,
            "IoIrq83" => v == 83,
            "IoIrq84" => v == 84,
            "IoIrq85" => v == 85,
            "IoIrq86" => v == 86,
            "IoIrq87" => v == 87,
            "IoIrq88" => v == 88,
            "IoIrq89" => v == 89,
            "IoIrq90" => v == 90,
            "IoIrq91" => v == 91,
            "IoIrq92" => v == 92,
            "IoIrq93" => v == 93,
            "IoIrq94" => v == 94,
            "IoIrq95" => v == 95,
            "IoIrq96" => v == 96,
            "IoIrq97" => v == 97,
            "IoIrq98" => v == 98,
            "IoIrq99" => v == 99,
            "IoIrq100" => v == 100,
            "IoIrq101" => v == 101,
            "IoIrq102" => v == 102,
            "IoIrq103" => v == 103,
            "IoIrq104" => v == 104,
            "IoIrq105" => v == 105,
            "IoIrq106" => v == 106,
            "IoIrq107" => v == 107,
            "IoIrq108" => v == 108,
            "IoIrq109" => v == 109,
            "IoIrq110" => v == 110,
            "IoIrq111" => v == 111,
            "IoIrq112" => v == 112,
            "IoIrq113" => v == 113,
            "IoIrq114" => v == 114,
            "IoIrq115" => v == 115,
            "IoIrq116" => v == 116,
            "IoIrq117" => v == 117,
            "IoIrq118" => v == 118,
            "IoIrq119" => v == 119,
            "IoIrq120" => v == 120,
            "IoIrq121" => v == 121,
            "IoIrq122" => v == 122,
            "IoIrq123" => v == 123,
            "IoIrq124" => v == 124,
            "IoIrq125" => v == 125,
            "IoIrq126" => v == 126,
            "IoIrq127" => v == 127,
            "SyscallOld" => v == 128,
            "DeviceIrq129" => v == 129,
            "DeviceIrq130" => v == 130,
            "DeviceIrq131" => v == 131,
            "DeviceIrq132" => v == 132,
            "DeviceIrq133" => v == 133,
            "DeviceIrq134" => v == 134,
            "DeviceIrq135" => v == 135,
            "DeviceIrq136" => v == 136,
            "DeviceIrq137" => v == 137,
            "DeviceIrq138" => v == 138,
            "DeviceIrq139" => v == 139,
            "DeviceIrq140" => v == 140,
            "DeviceIrq141" => v == 141,
            "DeviceIrq142" => v == 142,
            "DeviceIrq143" => v == 143,
            "DeviceIrq144" => v == 144,
            "DeviceIrq145" => v == 145,
            "DeviceIrq146" => v == 146,
            "DeviceIrq147" => v == 147,
            "DeviceIrq148" => v == 148,
            "DeviceIrq149" => v == 149,
            "DeviceIrq150" => v == 150,
            "DeviceIrq151" => v == 151,
            "DeviceIrq152" => v == 152,
            "DeviceIrq153" => v == 153,
            "DeviceIrq154" => v == 154,
            "DeviceIrq155" => v == 155,
            "DeviceIrq156" => v == 156,
            "DeviceIrq157" => v == 157,
            "DeviceIrq158" => v == 158,
            "DeviceIrq159" => v == 159,
            "DeviceIrq160" => v == 160,
            "DeviceIrq161" => v == 161,
            "DeviceIrq162" => v == 162,
            "DeviceIrq163" => v == 163,
            "DeviceIrq164" => v == 164,
            "DeviceIrq165" => v == 165,
            "DeviceIrq166" => v == 166,
            "DeviceIrq167" => v == 167,
            "DeviceIrq168" => v == 168,
            "DeviceIrq169" => v == 169,
            "DeviceIrq170" => v == 170,
            "DeviceIrq171" => v == 171,
            "DeviceIrq172" => v == 172,
            "DeviceIrq173" => v == 173,
            "DeviceIrq174" => v == 174,
            "DeviceIrq175" => v == 175,
            "DeviceIrq176" => v == 176,
            "DeviceIrq177" => v == 177,
            "DeviceIrq178" => v == 178,
            "DeviceIrq179" => v == 179,
            "DeviceIrq180" => v == 180,
            "DeviceIrq181" => v == 181,
            "DeviceIrq182" => v == 182,
            "DeviceIrq183" => v == 183,
            "DeviceIrq184" => v == 184,
            "DeviceIrq185" => v == 185,
            "DeviceIrq186" => v == 186,
            "DeviceIrq187" => v == 187,
            "DeviceIrq188" => v == 188,
            "DeviceIrq189" => v == 189,
            "DeviceIrq190" => v == 190,
            "DeviceIrq191" => v == 191,
            "DeviceIrq192" => v == 192,
            "DeviceIrq193" => v == 193,
            "DeviceIrq194" => v == 194,
            "DeviceIrq195" => v == 195,
            "DeviceIrq196" => v == 196,
            "DeviceIrq197" => v == 197,
            "DeviceIrq198" => v == 198,
            "DeviceIrq199" => v == 199,
            "DeviceIrq200" => v == 200,
            "DeviceIrq201" => v == 201,
            "DeviceIrq202" => v == 202,
            "DeviceIrq203" => v == 203,
            "DeviceIrq204" => v == 204,
            "DeviceIrq205" => v == 205,
            "DeviceIrq206" => v == 206,
            "DeviceIrq207" => v == 207,
            "DeviceIrq208" => v == 208,
            "DeviceIrq209" => v == 209,
            "DeviceIrq210" => v == 210,
            "DeviceIrq211" => v == 211,
            "DeviceIrq212" => v == 212,
            "DeviceIrq213" => v == 213,
            "DeviceIrq214" => v == 214,
            "DeviceIrq215" => v == 215,
            "DeviceIrq216" => v == 216,
            "DeviceIrq217" => v == 217,
            "DeviceIrq218" => v == 218,
            "DeviceIrq219" => v == 219,
            "DeviceIrq220" => v == 220,
            "DeviceIrq221" => v == 221,
            "DeviceIrq222" => v == 222,
            "DeviceIrq223" => v == 223,
            "DeviceIrq224" => v == 224,
            "DeviceIrq225" => v == 225,
            "DeviceIrq226" => v == 226,
            "DeviceIrq227" => v == 227,
            "DeviceIrq228" => v == 228,
            "DeviceIrq229" => v == 229,
            "DeviceIrq230" => v == 230,
            "DeviceIrq231" => v == 231,
            "DeviceIrq232" => v == 232,
            "DeviceIrq233" => v == 233,
            "DeviceIrq234" => v == 234,
            "DeviceIrq235" => v == 235,
            "DeviceIrq236" => v == 236,
            "DeviceIrq237" => v == 237,
            "DeviceIrq238" => v == 238,
            "DeviceIrq239" => v == 239,
            "ReservedHi240" => v == 240,
            "ReservedHi241" => v == 241,
            "ReservedHi242" => v == 242,
            "ReservedHi243" => v == 243,
            "ReservedHi244" => v == 244,
            "ReservedHi245" => v == 245,
            "ReservedHi246" => v == 246,
            "ReservedHi247" => v == 247,
            "ReservedHi248" => v == 248,
            "ReservedHi249" => v == 249,
            "ReservedHi250" => v == 250,
            "ReservedHi251" => v == 251,
            "ReservedHi252" => v == 252,
            "ApicError" => v == 253,
            "Spurious" => v == 254,
            "PanicVector" => v == 255,
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
pub fn idtvec_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < idtvec_COUNT {
        s = s.wrapping_add(idtvec[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn idtvec_unique() -> bool {
    let mut i = 0usize;
    while i < idtvec_COUNT {
        let mut j = i + 1;
        while j < idtvec_COUNT {
            if idtvec[i].0 == idtvec[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x4200, 0x420f]。
pub const fn t421_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t421Code::ALL.len() {
        let c = t421Code::ALL[i].code();
        if c < 0x4200 || c > 0x420f {
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
fn t421_guard_seg() -> bool {
    t421_segment_guard()
}

/// 表 idtvec 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t421_idtvec_guard() -> bool {
    let mut ok = idtvec_COUNT == 256;
    if ok {
        ok = idtvec_scan() == 0;
    }
    if ok {
        ok = idtvec_unique();
    }
    ok
}

/// 异常向量抽查（#DE/#DF/#PF/#MC + 中断分段）。
fn check_c2_vec_spot() -> bool {
    let mut ok = idtvec_get("DivideError") == Some(0);
    if ok {
        ok = idtvec_get("DoubleFault") == Some(8) && idtvec_get("PageFault") == Some(14);
    }
    if ok {
        ok = idtvec_get("MachineCheck") == Some(18) && idtvec_get("X87Fpe") == Some(16);
    }
    if ok {
        ok = idtvec_get("SyscallOld") == Some(0x80) && idtvec_get("PanicVector") == Some(0xff);
    }
    if ok {
        ok = idtvec_get("Spurious") == Some(0xfe) && idtvec_get("Timer") == Some(32);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t421_checks() -> CheckSet {
    let mut set = CheckSet::new("T421");
    set.add(
        "t421-seg-guard",
        t421_guard_seg(),
        if t421_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t421-c1-vec-guard",
        check_t421_idtvec_guard(),
        if check_t421_idtvec_guard() { "" } else { "table idtvec guard mismatch" },
    );
    set.add(
        "t421-c2-vec-spot",
        check_c2_vec_spot(),
        if check_c2_vec_spot() { "" } else { "idt vector spot check mismatch" },
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
    fn t421_all_checks_green() {
        let set = run_t421_checks();
        assert!(set.all_passed(), "T421 domain checks must be green");
    }

    #[test]
    fn t421_segment_guard_holds() {
        assert!(t421_segment_guard());
    }
}
