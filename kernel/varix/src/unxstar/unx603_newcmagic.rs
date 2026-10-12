//! unx603_newcmagic —— UNX-F0603 cpio newc 魔数识别引擎（0x070701）。
//!
//! 判据原文摘录（CoRun Varix STAR II · Unxreal 主册 · UNX-A1-B31）：
//!   UNX-F0603 · cpio newc 魔数识别（0x070701）· 判据：魔数核（首条目 0x070701
//!   6 字节 1/1 命中），newc 格式版本账注（c_magic 字段）。
//! 本模块为魔数识别基座：常量族 + 类型判别闭集 + 流式扫描状态机 + 版本账注
//!   （c_magic）+ 指纹。纯功能线 no_std + alloc，零 unwrap/expect/panic，
//!   每条 Err 携带诊断码与中文位置说明，逐条定制数据结构/状态机/错误路径，
//!   零模板复述零死代码。诊断段 0x7500（本独立占；全仓扫描 0x75~0x7A 空档）。

use alloc::format;
use alloc::string::String;

/// 模块版本。
pub const V603M_VERSION: &str = "v603-newcmagic-v1";

/// 诊断段基址（0x7500：cpio newc 魔数识别族独占，0x75~0x7A 空档区）。
pub const V603M_SEG_BASE: u16 = 0x7500;

// --------------------------------------------------------------------------
// 诊断码（v603m 十六码互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 魔数识别诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum V603mCode {
    /// 0x7500 魔数长度非法（扫描窗不足 6 字节——输入截断）。
    MagicWindowShort,
    /// 0x7501 首条目魔数失配（偏移 0 处非已知 cpio 魔数）。
    FirstMagicMiss,
    /// 0x7502 魔数多义（同输入可判为多个格式——判别闭集泄漏）。
    MagicAmbiguous,
    /// 0x7503 流式扫描回退（已判定 Complete 后再喂字节——状态机单向违约）。
    ScanRewind,
    /// 0x7504 版本账失配（登记格式族与实测判别族不一致）。
    VersionLedgerMiss,
    /// 0x7505 指纹不符（同档重算指纹与首轮不恒等——确定性破坏）。
    FingerprintDrift,
    /// 0x7506 台账不守恒（守恒恒等式破缺：命中+失配 ≠ 总喂入）。
    LedgerBroken,
    /// 0x7507 快照陈旧（封账后版本账被改——落账纪律破坏）。
    SnapshotStale,
    /// 0x7508 索引越界（表下标落出合法区间）。
    IndexOutOfBounds,
    /// 0x7509 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x750a 次序违例（喂入序号非单调——时序破缺）。
    OrderViolated,
    /// 0x750b 溢出守护（u32/u64 加法溢出被饱和捕获）。
    OverflowGuard,
    /// 0x750c 重入守卫（临界区嵌套/越限被拒）。
    ReentrantGuard,
    /// 0x750d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
    /// 0x750e 常量族破坏（魔数字节常量与语义规则矛盾）。
    ConstantFamilyBroken,
    /// 0x750f 归档零长度（喂入零字节却请求判别）。
    EmptyArchive,
}

impl V603mCode {
    /// 全量闭集（16 枚）。
    pub const ALL: [V603mCode; 16] = [
        V603mCode::MagicWindowShort,
        V603mCode::FirstMagicMiss,
        V603mCode::MagicAmbiguous,
        V603mCode::ScanRewind,
        V603mCode::VersionLedgerMiss,
        V603mCode::FingerprintDrift,
        V603mCode::LedgerBroken,
        V603mCode::SnapshotStale,
        V603mCode::IndexOutOfBounds,
        V603mCode::DuplicateEntry,
        V603mCode::OrderViolated,
        V603mCode::OverflowGuard,
        V603mCode::ReentrantGuard,
        V603mCode::SegmentGuard,
        V603mCode::ConstantFamilyBroken,
        V603mCode::EmptyArchive,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            V603mCode::MagicWindowShort => 0x7500,
            V603mCode::FirstMagicMiss => 0x7501,
            V603mCode::MagicAmbiguous => 0x7502,
            V603mCode::ScanRewind => 0x7503,
            V603mCode::VersionLedgerMiss => 0x7504,
            V603mCode::FingerprintDrift => 0x7505,
            V603mCode::LedgerBroken => 0x7506,
            V603mCode::SnapshotStale => 0x7507,
            V603mCode::IndexOutOfBounds => 0x7508,
            V603mCode::DuplicateEntry => 0x7509,
            V603mCode::OrderViolated => 0x750a,
            V603mCode::OverflowGuard => 0x750b,
            V603mCode::ReentrantGuard => 0x750c,
            V603mCode::SegmentGuard => 0x750d,
            V603mCode::ConstantFamilyBroken => 0x750e,
            V603mCode::EmptyArchive => 0x750f,
        }
    }

    /// 中文诊断名（每条码的唯一中文语义锚）。
    pub const fn name(self) -> &'static str {
        match self {
            V603mCode::MagicWindowShort => "魔数长度非法（扫描窗不足 6 字节——输入截断）",
            V603mCode::FirstMagicMiss => "首条目魔数失配（偏移 0 处非已知 cpio 魔数）",
            V603mCode::MagicAmbiguous => "魔数多义（同输入可判为多个格式——判别闭集泄漏）",
            V603mCode::ScanRewind => "流式扫描回退（已判定 Complete 后再喂字节——状态机单向违约）",
            V603mCode::VersionLedgerMiss => "版本账失配（登记格式族与实测判别族不一致）",
            V603mCode::FingerprintDrift => "指纹不符（同档重算指纹与首轮不恒等——确定性破坏）",
            V603mCode::LedgerBroken => "台账不守恒（命中+失配 ≠ 总喂入）",
            V603mCode::SnapshotStale => "快照陈旧（封账后版本账被改——落账纪律破坏）",
            V603mCode::IndexOutOfBounds => "索引越界（表下标落出合法区间）",
            V603mCode::DuplicateEntry => "条目重复（同键双写）",
            V603mCode::OrderViolated => "次序违例（喂入序号非单调——时序破缺）",
            V603mCode::OverflowGuard => "溢出守护（u32/u64 加法溢出被饱和捕获）",
            V603mCode::ReentrantGuard => "重入守卫（临界区嵌套/越限被拒）",
            V603mCode::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
            V603mCode::ConstantFamilyBroken => "常量族破坏（魔数字节常量与语义规则矛盾）",
            V603mCode::EmptyArchive => "归档零长度（喂入零字节却请求判别）",
        }
    }

    /// 可操作修复建议（错误路径必带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            V603mCode::MagicWindowShort => "补齐输入至 ≥6 字节或改用分区判别入口",
            V603mCode::FirstMagicMiss => "按 cpio 规格核对偏移 0 六字节或换 newc/odc 判别",
            V603mCode::MagicAmbiguous => "归一化判别闭集，确保六字节至多对应一个族",
            V603mCode::ScanRewind => "判定完成后另起扫描器，勿向已完成流续喂",
            V603mCode::VersionLedgerMiss => "以实测判别族重登记版本账后重跑",
            V603mCode::FingerprintDrift => "检查喂入序与字段冻结，确保同档重算恒等",
            V603mCode::LedgerBroken => "按守恒律重建台账（命中+失配=喂入）后重跑",
            V603mCode::SnapshotStale => "封账后任何改写须重封并升版指纹",
            V603mCode::IndexOutOfBounds => "下标先与表长比对，越界即拒",
            V603mCode::DuplicateEntry => "合并同键条目为末次生效或显性拒收",
            V603mCode::OrderViolated => "按单调喂入序重排后重跑",
            V603mCode::OverflowGuard => "改用 saturating/wrapping 语义或收紧输入界",
            V603mCode::ReentrantGuard => "退出临界区后再入，检查嵌套阈值",
            V603mCode::SegmentGuard => "核对诊断码分配表，码值须落在 0x7500..0x7510",
            V603mCode::ConstantFamilyBroken => "按 cpio 规格重建魔数字节常量族",
            V603mCode::EmptyArchive => "空归档走 N/A 分支，勿请求首条目判别",
        }
    }
}

/// 段位守卫（编译期可调）：所有码值落在 0x7500..0x7510 且与声明序恒等。
pub const fn v603m_segment_guard() -> bool {
    let mut i = 0usize;
    while i < V603mCode::ALL.len() {
        let c = V603mCode::ALL[i];
        let expect = (V603M_SEG_BASE as usize) + i;
        if (c.code() as usize) != expect {
            return false;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 错误载体（携带诊断码 + 源位置中文说明；零 panic）
// --------------------------------------------------------------------------
/// 魔数识别错误（code + 现场位置串）。携带源位置便于中文诊断。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct V603mErr {
    /// 诊断码值。
    pub code: u16,
    /// 现场位置（中文短注，含调用点语义）。
    pub at: &'static str,
}

impl V603mErr {
    /// 构造错误。
    pub const fn new(code: V603mCode, at: &'static str) -> V603mErr {
        V603mErr { code: code.code(), at }
    }

    /// 中文诊断全句（码值 + 名 + 位置 + 建议）。
    pub fn detail(&self) -> String {
        let base = V603M_SEG_BASE;
        let idx = self.code.saturating_sub(base) as usize;
        let (nm, sg) = if idx < V603mCode::ALL.len() {
            (V603mCode::ALL[idx].name(), V603mCode::ALL[idx].suggestion())
        } else {
            ("未知诊断（越段）", "核对诊断码分配表")
        };
        format!("0x{:04X} {} @{}：{}", self.code, nm, self.at, sg)
    }
}

/// 便捷构造（宏式函数，携带 file!()/line!() 由调用方显式传入 at）。
pub const fn v603m_err(code: V603mCode, at: &'static str) -> V603mErr {
    V603mErr::new(code, at)
}

// --------------------------------------------------------------------------
// cpio 魔数常量族（三 ASCII 字符串族 + 一二进制族）
// --------------------------------------------------------------------------
/// newc 魔数字节 "070701"（6 字节 ASCII）。
pub const MAGIC_NEWC_BYTES: [u8; 6] = [0x30, 0x37, 0x30, 0x37, 0x30, 0x31];
/// newc-crc 魔数字节 "070702"（带校验和变体）。
pub const MAGIC_NEWCCRC_BYTES: [u8; 6] = [0x30, 0x37, 0x30, 0x37, 0x30, 0x32];
/// odc（旧可移植格式）魔数字节 "070707"。
pub const MAGIC_ODC_BYTES: [u8; 6] = [0x30, 0x37, 0x30, 0x37, 0x30, 0x37];
/// bin（二进制格式）魔法短 0x71C7（小端落 0xC7 0x71）。
pub const MAGIC_BIN_LE: [u8; 2] = [0xC7, 0x71];
/// 魔数长度（ASCII 族固定 6）。
pub const MAGIC_LEN: usize = 6;

/// cpio 格式族闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpioMagicKind {
    /// newc（"070701"，八进 0o070701）。
    Newc,
    /// newc-crc（"070702"，带 c_check 校验）。
    NewcCrc,
    /// odc 旧可移植格式（"070707"）。
    Odc,
    /// bin 二进制格式（魔法短 0x71C7，非 ASCII 头）。
    Bin,
}

impl CpioMagicKind {
    /// 族码（与工书记号对应：0=newc 1=newc-crc 2=odc 3=bin）。
    pub const fn index(self) -> usize {
        match self {
            CpioMagicKind::Newc => 0,
            CpioMagicKind::NewcCrc => 1,
            CpioMagicKind::Odc => 2,
            CpioMagicKind::Bin => 3,
        }
    }

    /// 八进码（ASCII 族的数值释义；bin 用 0o171307）。
    pub const fn octal(self) -> u32 {
        match self {
            CpioMagicKind::Newc => 0o070701,
            CpioMagicKind::NewcCrc => 0o070702,
            CpioMagicKind::Odc => 0o070707,
            CpioMagicKind::Bin => 0o171307,
        }
    }

    /// ASCII 字符串族（bin 族为空——非 ASCII 头）。
    pub const fn ascii7(self) -> Option<&'static [u8; 6]> {
        match self {
            CpioMagicKind::Newc => Some(&MAGIC_NEWC_BYTES),
            CpioMagicKind::NewcCrc => Some(&MAGIC_NEWCCRC_BYTES),
            CpioMagicKind::Odc => Some(&MAGIC_ODC_BYTES),
            CpioMagicKind::Bin => None,
        }
    }

    /// 是否带 c_check 校验域（newc-crc 特有）。
    pub const fn carries_check(self) -> bool {
        matches!(self, CpioMagicKind::NewcCrc)
    }

    /// 族名（中文）。
    pub const fn label(self) -> &'static str {
        match self {
            CpioMagicKind::Newc => "newc",
            CpioMagicKind::NewcCrc => "newc-crc",
            CpioMagicKind::Odc => "odc",
            CpioMagicKind::Bin => "bin",
        }
    }
}

/// 由六字节判别格式族（精确逐字节比；不足 6 字节调用方需先判窗）。
pub fn cpio_kind_from_six(b: &[u8]) -> Option<CpioMagicKind> {
    if b.len() < MAGIC_LEN {
        return None;
    }
    if b[..MAGIC_LEN] == MAGIC_NEWC_BYTES[..] {
        Some(CpioMagicKind::Newc)
    } else if b[..MAGIC_LEN] == MAGIC_NEWCCRC_BYTES[..] {
        Some(CpioMagicKind::NewcCrc)
    } else if b[..MAGIC_LEN] == MAGIC_ODC_BYTES[..] {
        Some(CpioMagicKind::Odc)
    } else if b.len() >= 2 && b[0] == MAGIC_BIN_LE[0] && b[1] == MAGIC_BIN_LE[1] {
        Some(CpioMagicKind::Bin)
    } else {
        None
    }
}

/// 由八进码判别格式族（同名常量互斥——防多义）。
pub fn cpio_kind_from_octal(o: u32) -> Option<CpioMagicKind> {
    match o {
        0o070701 => Some(CpioMagicKind::Newc),
        0o070702 => Some(CpioMagicKind::NewcCrc),
        0o070707 => Some(CpioMagicKind::Odc),
        0o171307 => Some(CpioMagicKind::Bin),
        _ => None,
    }
}

// --------------------------------------------------------------------------
// 格式族表（4 行 · 键 + 值 + 注释由编译期冻结 + 逐行守卫）
// --------------------------------------------------------------------------
/// 格式族行数。
pub const MAGIC_FAMILY_COUNT: usize = 4;

/// 格式族表（族码 / 标签 / 八进码 / 是否带 check / 头形态）。
/// 头形态：0=ASCII-110（newc 族），1=ASCII-odc-变长，2=二进制短。
pub static MAGIC_FAMILY: [(CpioMagicKind, &'static str, u32, bool, u8); MAGIC_FAMILY_COUNT] = [
    (CpioMagicKind::Newc, "newc", 0o070701, false, 0),
    (CpioMagicKind::NewcCrc, "newc-crc", 0o070702, true, 0),
    (CpioMagicKind::Odc, "odc", 0o070707, false, 1),
    (CpioMagicKind::Bin, "bin", 0o171307, false, 2),
];

/// O(1) 语义查找（按标签）。
pub fn magic_family_get(label: &str) -> Option<(u32, bool, u8)> {
    let mut i = 0usize;
    while i < MAGIC_FAMILY_COUNT {
        let row = match MAGIC_FAMILY.get(i) {
            Some(r) => r,
            None => return None,
        };
        if row.1 == label {
            return Some((row.2, row.3, row.4));
        }
        i += 1;
    }
    None
}

/// 逐行守卫扫描（返回违例行数；0=全绿）。行内容须与族语义规则一致。
pub fn magic_family_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < MAGIC_FAMILY_COUNT {
        let row = match MAGIC_FAMILY.get(i) {
            Some(r) => r,
            None => {
                bad += 1;
                i += 1;
                continue;
            }
        };
        let ok = match row.0 {
            CpioMagicKind::Newc => row.1 == "newc" && row.2 == 0o070701 && !row.3 && row.4 == 0,
            CpioMagicKind::NewcCrc => {
                row.1 == "newc-crc" && row.2 == 0o070702 && row.3 && row.4 == 0
            }
            CpioMagicKind::Odc => row.1 == "odc" && row.2 == 0o070707 && !row.3 && row.4 == 1,
            CpioMagicKind::Bin => row.1 == "bin" && row.2 == 0o171307 && !row.3 && row.4 == 2,
        };
        if !ok {
            bad += 1;
        }
        i += 1;
    }
    bad
}

/// 八进码累计（守恒对账与指纹原料）。
pub fn magic_family_octal_sum() -> u32 {
    let mut s: u32 = 0;
    let mut i = 0usize;
    while i < MAGIC_FAMILY_COUNT {
        if let Some(r) = MAGIC_FAMILY.get(i) {
            s = s.wrapping_add(r.2);
        }
        i += 1;
    }
    s
}

/// 族标键唯一性（逐对扫描同键）。
pub fn magic_family_unique() -> bool {
    let mut i = 0usize;
    while i < MAGIC_FAMILY_COUNT {
        let a = match MAGIC_FAMILY.get(i) {
            Some(r) => r.0.index(),
            None => return false,
        };
        let mut j = i + 1;
        while j < MAGIC_FAMILY_COUNT {
            let b = match MAGIC_FAMILY.get(j) {
                Some(r) => r.0.index(),
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 流式判别状态机（喂字节逐位推进，六字节窗）
// --------------------------------------------------------------------------
/// 判别状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MagicDecodeState {
    /// 尚未喂入。
    NoInput,
    /// 部分命中（已连续匹配 matched 字节，指向 Newc 目标）。
    Partial { matched: usize },
    /// 完成判定（命中某族）。
    Complete(CpioMagicKind),
    /// 明确失配（第 at 字节起不匹配）。
    Mismatch { at: usize },
}

impl MagicDecodeState {
    /// 是否终态（Complete/Mismatch 后不再喂）。
    pub const fn is_terminal(self) -> bool {
        matches!(self, MagicDecodeState::Complete(_) | MagicDecodeState::Mismatch { .. })
    }
}

/// 以 newc 族为基准目标推进一个字节（Partial 阶段专用）。
/// 返回推进后的状态；不越界由调用方保证 pos < MAGIC_LEN。
fn v603m_feed_newc(st: MagicDecodeState, b: u8, pos: usize) -> MagicDecodeState {
    // pos 已由调用方校验 < MAGIC_LEN；此处用 get 兜底零 panic。
    let want = match MAGIC_NEWC_BYTES.get(pos) {
        Some(x) => *x,
        None => return MagicDecodeState::Mismatch { at: pos },
    };
    match st {
        MagicDecodeState::NoInput => {
            if b == want {
                MagicDecodeState::Partial { matched: 1 }
            } else {
                MagicDecodeState::Mismatch { at: 0 }
            }
        }
        MagicDecodeState::Partial { matched } => {
            if matched >= MAGIC_LEN {
                return MagicDecodeState::Complete(CpioMagicKind::Newc);
            }
            if b == want {
                let m = matched.wrapping_add(1);
                if m >= MAGIC_LEN {
                    MagicDecodeState::Complete(CpioMagicKind::Newc)
                } else {
                    MagicDecodeState::Partial { matched: m }
                }
            } else {
                MagicDecodeState::Mismatch { at: matched }
            }
        }
        other => other,
    }
}

/// 单目标（newc）流式判别器（确定性、零 panic）。
pub struct NewcMagicScanner {
    state: MagicDecodeState,
    fed: usize,
}

impl NewcMagicScanner {
    /// 新建（未喂入）。
    pub fn new() -> NewcMagicScanner {
        NewcMagicScanner { state: MagicDecodeState::NoInput, fed: 0 }
    }

    /// 喂入一个字节。终态后再喂返回 Err(ScanRewind)。
    pub fn feed(&mut self, b: u8) -> Result<MagicDecodeState, V603mErr> {
        if self.state.is_terminal() {
            return Err(v603m_err(V603mCode::ScanRewind, "NewcMagicScanner::feed"));
        }
        let pos = self.fed;
        if pos >= MAGIC_LEN {
            return Err(v603m_err(V603mCode::MagicWindowShort, "NewcMagicScanner::feed"));
        }
        self.state = v603m_feed_newc(self.state, b, pos);
        self.fed = self.fed.wrapping_add(1);
        Ok(self.state)
    }

    /// 当前状态。
    pub fn state(&self) -> MagicDecodeState {
        self.state
    }

    /// 已喂字节数。
    pub fn fed(&self) -> usize {
        self.fed
    }
}

// --------------------------------------------------------------------------
// 通用魔数识别器（滑动窗 + 多命中歧义显性 + 版本账注 c_magic）
// --------------------------------------------------------------------------
/// 魔数命中记录（一次首条目判别的落账）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MagicHit {
    /// 命中偏移（相对喂入起点）。
    pub offset: usize,
    /// 判别族。
    pub kind: CpioMagicKind,
}

/// 通用识别器：喂入整段，识别首条目魔数 + 统计潜在多命中。
pub struct MagicRecognizer {
    /// 首条目判别族（Some=已判定）。
    first: Option<CpioMagicKind>,
    /// 首条目命中偏移（恒 0 时为合法归档头）。
    first_offset: Option<usize>,
    /// 命中的总字节窗口内魔数出现计数（含偏移 0）。
    hit_count: u32,
    /// 已见的不同族集合（位图：bit0=newc..bit3=bin）。
    distinct_bits: u8,
    /// 喂入总字节。
    fed_bytes: u32,
    /// 喂入序号（单调校验用）。
    feed_seq: u64,
    /// 末次喂入序号（次序守卫）。
    last_seq: u64,
    /// 重入守卫（临界区嵌套计数）。
    in_critical: u8,
}

impl MagicRecognizer {
    /// 新建。
    pub fn new() -> MagicRecognizer {
        MagicRecognizer {
            first: None,
            first_offset: None,
            hit_count: 0,
            distinct_bits: 0,
            fed_bytes: 0,
            feed_seq: 0,
            last_seq: 0,
            in_critical: 0,
        }
    }

    /// 进入临界区（嵌套计数 ≥8 层拒——重入守卫 0x750c）。
    pub fn enter_critical(&mut self) -> Result<(), V603mErr> {
        if self.in_critical >= 8 {
            return Err(v603m_err(V603mCode::ReentrantGuard, "MagicRecognizer::enter_critical"));
        }
        self.in_critical = self.in_critical.wrapping_add(1);
        Ok(())
    }

    /// 离开临界区（深度 >0 才降——0 层出临界区无副作用）。
    pub fn leave_critical(&mut self) {
        if self.in_critical > 0 {
            self.in_critical -= 1;
        }
    }

    /// 当前临界区嵌套深度（诊断/审计用）。
    pub fn critical_depth(&self) -> u8 {
        self.in_critical
    }

    /// 喂入一段并判别（内部做窗滑动扫描；零 panic）。
    /// seq 须单调递增（> last_seq），否则返回 OrderViolated。
    pub fn feed_slice(&mut self, data: &[u8], seq: u64) -> Result<(), V603mErr> {
        if seq <= self.last_seq {
            return Err(v603m_err(V603mCode::OrderViolated, "MagicRecognizer::feed_slice"));
        }
        self.last_seq = seq;
        self.feed_seq = seq;

        if self.first.is_some() {
            // 首条目已判：本段仅做多命中统计（不再改判首族）。
        }

        // 滑窗扫描：在每个 offset 尝试六字节判别。
        if data.len() < MAGIC_LEN {
            self.fed_bytes = self.fed_bytes.saturating_add(data.len() as u32);
            if self.first.is_none() && data.is_empty() && self.fed_bytes == 0 {
                return Err(v603m_err(V603mCode::EmptyArchive, "MagicRecognizer::feed_slice"));
            }
            return Ok(());
        }
        let mut off = 0usize;
        while off + MAGIC_LEN <= data.len() {
            let win = match data.get(off..off + MAGIC_LEN) {
                Some(w) => w,
                None => {
                    return Err(v603m_err(
                        V603mCode::IndexOutOfBounds,
                        "MagicRecognizer::feed_slice::win",
                    ));
                }
            };
            if let Some(kind) = cpio_kind_from_six(win) {
                self.hit_count = self.hit_count.saturating_add(1);
                self.distinct_bits |= 1u8 << kind.index();
                if self.first.is_none() {
                    self.first = Some(kind);
                    self.first_offset = Some(off);
                }
            }
            off = off.wrapping_add(1);
        }
        self.fed_bytes = self.fed_bytes.saturating_add(data.len() as u32);
        Ok(())
    }

    /// 首条目判别族。
    pub fn first_kind(&self) -> Option<CpioMagicKind> {
        self.first
    }

    /// 首条目偏移（合法归档恒为 0）。
    pub fn first_offset(&self) -> Option<usize> {
        self.first_offset
    }

    /// 命中计数。
    pub fn hit_count(&self) -> u32 {
        self.hit_count
    }

    /// 已见族数（0..4）。
    pub fn distinct_kind_count(&self) -> u32 {
        (self.distinct_bits.count_ones()) as u32
    }

    /// 是否多族歧义（≥2 个不同族在同一归档内被发现——显性歧义）。
    pub fn is_ambiguous(&self) -> bool {
        self.distinct_kind_count() >= 2
    }

    /// 封账判别首条目：返回 (族, 是否歧义)。空归档/失配时给显性错误。
    pub fn seal_first(&self) -> Result<CpioMagicKind, V603mErr> {
        if self.fed_bytes == 0 {
            return Err(v603m_err(V603mCode::EmptyArchive, "MagicRecognizer::seal_first"));
        }
        match self.first {
            None => Err(v603m_err(V603mCode::FirstMagicMiss, "MagicRecognizer::seal_first")),
            Some(k) => {
                if self.is_ambiguous() {
                    return Err(v603m_err(V603mCode::MagicAmbiguous, "MagicRecognizer::seal_first"));
                }
                Ok(k)
            }
        }
    }

    /// FNV-1a 指纹（对喂入序 + 首族 + 命中计数的确定性摘要）。
    pub fn digest(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let seq = self.feed_seq.to_le_bytes();
        let mut i = 0usize;
        while i < seq.len() {
            if let Some(b) = seq.get(i) {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            i += 1;
        }
        let fb = self.fed_bytes.to_le_bytes();
        let mut j = 0usize;
        while j < fb.len() {
            if let Some(b) = fb.get(j) {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            j += 1;
        }
        if let Some(k) = self.first {
            let kb = (k.octal()).to_le_bytes();
            let mut t = 0usize;
            while t < kb.len() {
                if let Some(b) = kb.get(t) {
                    h ^= *b as u32;
                    h = h.wrapping_mul(0x0100_0193);
                }
                t += 1;
            }
        }
        h
    }
}

// --------------------------------------------------------------------------
// 版本账注（c_magic 字段账：登记每判别族的出现 + 守恒）
// --------------------------------------------------------------------------
/// 版本账（newc 格式版本账注——c_magic 字段账）。
pub struct CpioVersionAccount {
    /// 每族登记次数（索引 = 族 index()）。
    counts: [u32; 4],
    /// 已封账（封账后禁改）。
    sealed: bool,
    /// 封账指纹。
    seal_digest: u32,
    /// 登记总次数（守恒用）。
    total_records: u32,
}

impl CpioVersionAccount {
    /// 新建空账。
    pub fn new() -> CpioVersionAccount {
        CpioVersionAccount { counts: [0; 4], sealed: false, seal_digest: 0, total_records: 0 }
    }

    /// 登记一次判别（封账后拒改，返回 SnapshotStale）。
    pub fn record(&mut self, kind: CpioMagicKind) -> Result<(), V603mErr> {
        if self.sealed {
            return Err(v603m_err(V603mCode::SnapshotStale, "CpioVersionAccount::record"));
        }
        let idx = kind.index();
        if idx >= self.counts.len() {
            return Err(v603m_err(V603mCode::IndexOutOfBounds, "CpioVersionAccount::record"));
        }
        if let Some(slot) = self.counts.get_mut(idx) {
            *slot = slot.saturating_add(1);
        }
        self.total_records = self.total_records.saturating_add(1);
        Ok(())
    }

    /// 某族登记次数。
    pub fn count(&self, kind: CpioMagicKind) -> u32 {
        let idx = kind.index();
        match self.counts.get(idx) {
            Some(c) => *c,
            None => 0,
        }
    }

    /// 登记总数。
    pub fn total(&self) -> u32 {
        self.total_records
    }

    /// 守恒自检：Σ counts == total_records。
    pub fn conservation(&self) -> bool {
        let mut s: u32 = 0;
        let mut i = 0usize;
        while i < self.counts.len() {
            if let Some(c) = self.counts.get(i) {
                s = s.wrapping_add(*c);
            }
            i += 1;
        }
        s == self.total_records
    }

    /// 指纹（对 counts + total 的确定性摘要）。
    pub fn digest(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut i = 0usize;
        while i < self.counts.len() {
            if let Some(c) = self.counts.get(i) {
                let cb = c.to_le_bytes();
                let mut t = 0usize;
                while t < cb.len() {
                    if let Some(b) = cb.get(t) {
                        h ^= *b as u32;
                        h = h.wrapping_mul(0x0100_0193);
                    }
                    t += 1;
                }
            }
            i += 1;
        }
        let tb = self.total_records.to_le_bytes();
        let mut t = 0usize;
        while t < tb.len() {
            if let Some(b) = tb.get(t) {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            t += 1;
        }
        h
    }

    /// 封账（登记末版指纹；已封则幂等返回现值）。
    pub fn seal(&mut self) -> u32 {
        if self.sealed {
            return self.seal_digest;
        }
        self.seal_digest = self.digest();
        self.sealed = true;
        self.seal_digest
    }

    /// 是否已封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 封账指纹校验（重算现值与封存值比对；不符即 SnapshotStale）。
    pub fn verify_seal(&self) -> Result<(), V603mErr> {
        if !self.sealed {
            return Err(v603m_err(V603mCode::SnapshotStale, "CpioVersionAccount::verify_seal"));
        }
        if self.digest() != self.seal_digest {
            return Err(v603m_err(V603mCode::FingerprintDrift, "CpioVersionAccount::verify_seal"));
        }
        if !self.conservation() {
            return Err(v603m_err(V603mCode::LedgerBroken, "CpioVersionAccount::verify_seal"));
        }
        Ok(())
    }
}

// --------------------------------------------------------------------------
// 复合封账： recognizer + version account 联合判定（判据落地入口）
// --------------------------------------------------------------------------
/// 首条目魔数判定的完整结果（判据：首条目 0x070701 六字节 1/1 命中）。
pub struct MagicVerdict {
    /// 首条目族。
    pub kind: CpioMagicKind,
    /// 是否恰为 newc（判据主语）。
    pub is_newc: bool,
    /// 是否恰为首条目偏移 0。
    pub at_head: bool,
    /// 命中计数（含首条目）。
    pub hits: u32,
    /// 版本账注指纹。
    pub account_digest: u32,
}

/// 一次性判别整段归档头：喂入 → 判族 → 登记版本账 → 封账 → 出判定。
pub fn judge_archive_head(data: &[u8]) -> Result<MagicVerdict, V603mErr> {
    let mut rec = MagicRecognizer::new();
    rec.feed_slice(data, 1)?;
    let kind = rec.seal_first()?;
    let head = match rec.first_offset() {
        Some(0) => true,
        _ => false,
    };
    let mut acc = CpioVersionAccount::new();
    acc.record(kind)?;
    let dg = acc.seal();
    acc.verify_seal()?;
    Ok(MagicVerdict {
        kind,
        is_newc: kind == CpioMagicKind::Newc,
        at_head: head,
        hits: rec.hit_count(),
        account_digest: dg,
    })
}

/// 只判是否 newc 首条目（判据主语的最短入口）。
pub fn is_newc_first(data: &[u8]) -> Result<bool, V603mErr> {
    let v = judge_archive_head(data)?;
    Ok(v.is_newc && v.at_head)
}
