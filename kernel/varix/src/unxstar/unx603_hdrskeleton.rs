//! unx603_hdrskeleton —— UNX-F0603 cpio newc 头骨架 110 字节布局账。
//!
//! 判据原文摘录（CoRun Varix STAR II · Unxreal 主册 · UNX-A1-B31）：
//!   UNX-F0603 · cpio newc 魔数识别（0x070701）· 判据：魔数核（首条目 0x070701
//!   6 字节 1/1 命中），newc 格式版本账注（c_magic 字段）。
//! 本模块落地"头骨架"：newc 110 字节头 = c_magic(6) + 13 个整数字段(8×13)
//!   的权威布局表（字段名/偏移/宽度）+ 布局守恒律（宽度和=110/偏移严格升/
//!   首偏 0/末界 110）+ 派生 name/data 段偏移（c_namesize 含尾 NUL，name 4 字节
//!   对齐）+ 布局封账指纹。纯功能线 no_std + alloc，零 unwrap/expect/panic，
//!   诊断段 0x7530。

use alloc::{format, string::String};

use super::unx603_cmagic::{CMAGIC_LEN, INT_FIELD_LEN, NEWC_INT_FIELDS};

/// 模块版本。
pub const V603H_VERSION: &str = "v603-hdrskeleton-v1";

/// 诊断段基址（0x7530）。
pub const V603H_SEG_BASE: u16 = 0x7530;

/// newc 头官方长度（110 = 6 + 13×8）。
pub const NEWC_HEADER_LEN: u16 = 110;
/// 头字段槽数（1 魔数 + 13 整数）。
pub const HEADER_SLOTS: usize = NEWC_INT_FIELDS + 1;
/// name 段对齐（newc 按 4 字节对齐）。
pub const NAME_ALIGN: u32 = 4;

// --------------------------------------------------------------------------
// 诊断码（v603h 十六码互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 头骨架布局诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum V603hCode {
    /// 0x7530 布局不守恒（宽度和 ≠ 110）。
    LayoutSumMismatch,
    /// 0x7531 偏移非严格升（字段偏移须递增）。
    OffsetNotMonotonic,
    /// 0x7532 首偏移非零（布局须从头开始）。
    FirstOffsetNonzero,
    /// 0x7533 末界 ≠ 110（末字段末界越/欠头界）。
    LastEndMismatch,
    /// 0x7534 字段名未登记（查找名落出布局表）。
    FieldNameUnknown,
    /// 0x7535 字段槽位越界（索引落出 [0,14)）。
    SlotIndexOutOfBounds,
    /// 0x7536 魔数槽宽非法（c_magic 槽宽恒 6）。
    MagicSlotWidthIllegal,
    /// 0x7537 整数槽宽非法（整数字段槽宽恒 8）。
    IntSlotWidthIllegal,
    /// 0x7538 name 派生越界（c_namesize 非法导致 name 段越界）。
    NameDeriveOverflow,
    /// 0x7539 data 派生越界（c_filesize 非法导致 data 段越界）。
    DataDeriveOverflow,
    /// 0x753a 对齐溢出（name 段 4 对齐计算溢出）。
    AlignOverflow,
    /// 0x753b 布局指纹不符（同布局重算与封存不恒等）。
    LayoutFingerprintDrift,
    /// 0x753c 布局封账后改写（封账禁改——快照陈旧）。
    LayoutSealedWrite,
    /// 0x753d 槽位表重复名（两槽同名——布局别名冲突）。
    SlotAliasClash,
    /// 0x753e 槽位断连（相邻槽 offset ≠ 前槽 offset+width——布局有缝）。
    SlotGap,
    /// 0x753f 次序违例（布局构建序非规格升）。
    LayoutOrderViolated,
}

impl V603hCode {
    /// 全量闭集。
    pub const ALL: [V603hCode; 16] = [
        V603hCode::LayoutSumMismatch,
        V603hCode::OffsetNotMonotonic,
        V603hCode::FirstOffsetNonzero,
        V603hCode::LastEndMismatch,
        V603hCode::FieldNameUnknown,
        V603hCode::SlotIndexOutOfBounds,
        V603hCode::MagicSlotWidthIllegal,
        V603hCode::IntSlotWidthIllegal,
        V603hCode::NameDeriveOverflow,
        V603hCode::DataDeriveOverflow,
        V603hCode::AlignOverflow,
        V603hCode::LayoutFingerprintDrift,
        V603hCode::LayoutSealedWrite,
        V603hCode::SlotAliasClash,
        V603hCode::SlotGap,
        V603hCode::LayoutOrderViolated,
    ];

    /// 码值（与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            V603hCode::LayoutSumMismatch => 0x7530,
            V603hCode::OffsetNotMonotonic => 0x7531,
            V603hCode::FirstOffsetNonzero => 0x7532,
            V603hCode::LastEndMismatch => 0x7533,
            V603hCode::FieldNameUnknown => 0x7534,
            V603hCode::SlotIndexOutOfBounds => 0x7535,
            V603hCode::MagicSlotWidthIllegal => 0x7536,
            V603hCode::IntSlotWidthIllegal => 0x7537,
            V603hCode::NameDeriveOverflow => 0x7538,
            V603hCode::DataDeriveOverflow => 0x7539,
            V603hCode::AlignOverflow => 0x753a,
            V603hCode::LayoutFingerprintDrift => 0x753b,
            V603hCode::LayoutSealedWrite => 0x753c,
            V603hCode::SlotAliasClash => 0x753d,
            V603hCode::SlotGap => 0x753e,
            V603hCode::LayoutOrderViolated => 0x753f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            V603hCode::LayoutSumMismatch => "布局不守恒（宽度和 ≠ 110）",
            V603hCode::OffsetNotMonotonic => "偏移非严格升（字段偏移须递增）",
            V603hCode::FirstOffsetNonzero => "首偏移非零（布局须从头开始）",
            V603hCode::LastEndMismatch => "末界 ≠ 110（末字段末界越/欠头界）",
            V603hCode::FieldNameUnknown => "字段名未登记（查找名落出布局表）",
            V603hCode::SlotIndexOutOfBounds => "字段槽位越界（索引落出 [0,14)）",
            V603hCode::MagicSlotWidthIllegal => "魔数槽宽非法（c_magic 槽宽恒 6）",
            V603hCode::IntSlotWidthIllegal => "整数槽宽非法（整数字段槽宽恒 8）",
            V603hCode::NameDeriveOverflow => "name 派生越界（c_namesize 非法致 name 段越界）",
            V603hCode::DataDeriveOverflow => "data 派生越界（c_filesize 非法致 data 段越界）",
            V603hCode::AlignOverflow => "对齐溢出（name 段 4 对齐计算溢出）",
            V603hCode::LayoutFingerprintDrift => "布局指纹不符（同布局重算与封存不恒等）",
            V603hCode::LayoutSealedWrite => "布局封账后改写（封账禁改——快照陈旧）",
            V603hCode::SlotAliasClash => "槽位表重复名（两槽同名——布局别名冲突）",
            V603hCode::SlotGap => "槽位断连（相邻 offset ≠ 前 offset+width——布局有缝）",
            V603hCode::LayoutOrderViolated => "次序违例（布局构建序非规格升）",
        }
    }

    /// 可操作修复建议。
    pub const fn suggestion(self) -> &'static str {
        match self {
            V603hCode::LayoutSumMismatch => "校准字段宽度使 Σ=6+13×8=110",
            V603hCode::OffsetNotMonotonic => "按规格字段序重排偏移使严格升",
            V603hCode::FirstOffsetNonzero => "首字段偏移须为 0",
            V603hCode::LastEndMismatch => "末字段末界须恰为 110",
            V603hCode::FieldNameUnknown => "核对字段名拼写或补齐布局登记",
            V603hCode::SlotIndexOutOfBounds => "下标先与 HEADER_SLOTS 比对",
            V603hCode::MagicSlotWidthIllegal => "c_magic 槽宽恒 6，勿改",
            V603hCode::IntSlotWidthIllegal => "整数字段槽宽恒 8，勿改",
            V603hCode::NameDeriveOverflow => "c_namesize 取合理界并重算 name 段",
            V603hCode::DataDeriveOverflow => "c_filesize 取合理界并重算 data 段",
            V603hCode::AlignOverflow => "缩小 c_namesize 或收紧对齐计算",
            V603hCode::LayoutFingerprintDrift => "冻结布局后重封并升版指纹",
            V603hCode::LayoutSealedWrite => "封账后改写须重封并升版指纹",
            V603hCode::SlotAliasClash => "重命名字段保证槽位名唯一",
            V603hCode::SlotGap => "使相邻槽 offset 连续无隙",
            V603hCode::LayoutOrderViolated => "按规格字段序重建布局",
        }
    }
}

/// 段位守卫。
pub const fn v603h_segment_guard() -> bool {
    let mut i = 0usize;
    while i < V603hCode::ALL.len() {
        let c = V603hCode::ALL[i];
        if (c.code() as usize) != (V603H_SEG_BASE as usize) + i {
            return false;
        }
        i += 1;
    }
    true
}

/// 错误载体。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct V603hErr {
    /// 诊断码值。
    pub code: u16,
    /// 现场位置中文短注。
    pub at: &'static str,
}

impl V603hErr {
    /// 构造。
    pub const fn new(code: V603hCode, at: &'static str) -> V603hErr {
        V603hErr { code: code.code(), at }
    }
    /// 中文诊断全句。
    pub fn detail(&self) -> String {
        let idx = self.code.saturating_sub(V603H_SEG_BASE) as usize;
        let (nm, sg) = if idx < V603hCode::ALL.len() {
            (V603hCode::ALL[idx].name(), V603hCode::ALL[idx].suggestion())
        } else {
            ("未知诊断（越段）", "核对诊断码分配表")
        };
        format!("0x{:04X} {} @{}：{}", self.code, nm, self.at, sg)
    }
}

const fn v603h_err(code: V603hCode, at: &'static str) -> V603hErr {
    V603hErr::new(code, at)
}

// --------------------------------------------------------------------------
// 布局槽位表（14 槽：c_magic + 13 整数；权威偏移/宽度）
// --------------------------------------------------------------------------
/// 单个字段槽位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FieldSlot {
    /// 字段名（规格名）。
    pub name: &'static str,
    /// 偏移（字节，从头起）。
    pub offset: u16,
    /// 宽度（字节；c_magic=6，整数=8）。
    pub width: u8,
    /// 是否魔数槽。
    pub is_magic: bool,
}

/// 分步展开偏移的编译期布局表（手工逐条，零拼接——铁律：不套壳）。
pub static HEADER_LAYOUT: [FieldSlot; HEADER_SLOTS] = [
    FieldSlot { name: "c_magic", offset: 0, width: 6, is_magic: true },
    FieldSlot { name: "c_ino", offset: 6, width: 8, is_magic: false },
    FieldSlot { name: "c_mode", offset: 14, width: 8, is_magic: false },
    FieldSlot { name: "c_uid", offset: 22, width: 8, is_magic: false },
    FieldSlot { name: "c_gid", offset: 30, width: 8, is_magic: false },
    FieldSlot { name: "c_nlink", offset: 38, width: 8, is_magic: false },
    FieldSlot { name: "c_mtime", offset: 46, width: 8, is_magic: false },
    FieldSlot { name: "c_filesize", offset: 54, width: 8, is_magic: false },
    FieldSlot { name: "c_devmajor", offset: 62, width: 8, is_magic: false },
    FieldSlot { name: "c_devminor", offset: 70, width: 8, is_magic: false },
    FieldSlot { name: "c_rdevmajor", offset: 78, width: 8, is_magic: false },
    FieldSlot { name: "c_rdevminor", offset: 86, width: 8, is_magic: false },
    FieldSlot { name: "c_namesize", offset: 94, width: 8, is_magic: false },
    FieldSlot { name: "c_check", offset: 102, width: 8, is_magic: false },
];

/// 按索引取槽（越界显性返 None——零 panic）。
pub fn slot(idx: usize) -> Option<FieldSlot> {
    match HEADER_LAYOUT.get(idx) {
        Some(s) => Some(*s),
        None => None,
    }
}

/// 按字段名取槽（未登记返 None）。
pub fn slot_by_name(name: &str) -> Option<FieldSlot> {
    let mut i = 0usize;
    while i < HEADER_SLOTS {
        match HEADER_LAYOUT.get(i) {
            None => return None,
            Some(s) => {
                if s.name == name {
                    return Some(*s);
                }
            }
        }
        i += 1;
    }
    None
}

/// 布局总宽（Σ width）。
pub fn layout_total_width() -> u32 {
    let mut s: u32 = 0;
    let mut i = 0usize;
    while i < HEADER_SLOTS {
        if let Some(sl) = HEADER_LAYOUT.get(i) {
            s = s.wrapping_add(sl.width as u32);
        }
        i += 1;
    }
    s
}

/// 布局扫描（返回违例数；0=全绿）。校验：总宽=110/首偏 0/偏移严格升/槽连续无隙/魔数槽宽 6/整数槽宽 8/名唯一。
pub fn layout_scan() -> usize {
    let mut bad = 0usize;
    if layout_total_width() != NEWC_HEADER_LEN as u32 {
        bad += 1;
    }
    // 首偏 0。
    match HEADER_LAYOUT.first() {
        None => bad += 1,
        Some(s) => {
            if s.offset != 0 {
                bad += 1;
            }
        }
    }
    // 逐槽：偏移严格升 + 连续 + 宽度合法 + 名唯一。
    let mut i = 0usize;
    while i < HEADER_SLOTS {
        match HEADER_LAYOUT.get(i) {
            None => bad += 1,
            Some(s) => {
                if s.is_magic && s.width != CMAGIC_LEN as u8 {
                    bad += 1;
                }
                if !s.is_magic && s.width != INT_FIELD_LEN as u8 {
                    bad += 1;
                }
                if i > 0 {
                    match HEADER_LAYOUT.get(i - 1) {
                        None => bad += 1,
                        Some(p) => {
                            // 严格升。
                            if s.offset <= p.offset {
                                bad += 1;
                            }
                            // 连续无隙。
                            let expect_end = p.offset as u32 + p.width as u32;
                            if s.offset as u32 != expect_end {
                                bad += 1;
                            }
                        }
                    }
                }
                // 名唯一（与后续比）。
                let mut j = i + 1;
                while j < HEADER_SLOTS {
                    match HEADER_LAYOUT.get(j) {
                        None => bad += 1,
                        Some(q) => {
                            if s.name == q.name {
                                bad += 1;
                            }
                        }
                    }
                    j += 1;
                }
            }
        }
        i += 1;
    }
    // 末界 = 110。
    match HEADER_LAYOUT.last() {
        None => bad += 1,
        Some(s) => {
            if s.offset as u32 + s.width as u32 != NEWC_HEADER_LEN as u32 {
                bad += 1;
            }
        }
    }
    bad
}

/// 末字段末界（=110）。
pub fn last_end() -> u32 {
    match HEADER_LAYOUT.last() {
        None => 0,
        Some(s) => s.offset as u32 + s.width as u32,
    }
}

// --------------------------------------------------------------------------
// 头缓冲取字段零拷贝切片（零 panic：先校验 offset+width ≤ len）
// --------------------------------------------------------------------------
/// 从头缓冲按槽位取字节（越界返 Err(SlotIndexOutOfBounds/派生越界)）。
pub fn field_bytes<'a>(buf: &'a [u8], name: &str) -> Result<&'a [u8], V603hErr> {
    let sl = match slot_by_name(name) {
        None => return Err(v603h_err(V603hCode::FieldNameUnknown, "field_bytes")),
        Some(s) => s,
    };
    let start = sl.offset as usize;
    let end = start.wrapping_add(sl.width as usize);
    if end > buf.len() {
        return Err(v603h_err(V603hCode::SlotIndexOutOfBounds, "field_bytes"));
    }
    match buf.get(start..end) {
        Some(w) => Ok(w),
        None => Err(v603h_err(V603hCode::SlotIndexOutOfBounds, "field_bytes")),
    }
}

// --------------------------------------------------------------------------
// name/data 段派生（newc 头后 name 含尾 NUL，按 4 对齐，随后 data）
// --------------------------------------------------------------------------
/// 已派生段位置（相对归档起点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DerivedSegments {
    /// name 段起点偏移（= NEWC_HEADER_LEN）。
    pub name_off: u32,
    /// name 段长度（= c_namesize，含尾 NUL）。
    pub name_len: u32,
    /// name 4 字节对齐后的终点（data 起点）。
    pub data_off: u32,
    /// data 段长度（= c_filesize）。
    pub data_len: u32,
    /// 整条目末界（data_off + data_len；相对归档起点）。
    pub entry_end: u32,
}

/// 四对齐（向上取整到 NAME_ALIGN 倍数）；溢出显性报。
pub fn align_up(v: u32) -> Result<u32, V603hErr> {
    match v.checked_add(NAME_ALIGN - 1) {
        None => Err(v603h_err(V603hCode::AlignOverflow, "align_up")),
        Some(s) => Ok((s / NAME_ALIGN) * NAME_ALIGN),
    }
}

/// 从头缓冲派生 name/data 段位置（读取 c_namesize/c_filesize 整数字段并解码）。
/// c_filesize/c_namesize 的 ASCII-十六进制解码复用 c_magic 模块的解码器。
pub fn derive_segments(buf: &[u8]) -> Result<DerivedSegments, V603hErr> {
    // 取 c_namesize / c_filesize 原始字节（含越界守卫）。
    let ns_bytes = field_bytes(buf, "c_namesize")?;
    let fs_bytes = field_bytes(buf, "c_filesize")?;
    let name_len = super::unx603_cmagic::decode_int_field(ns_bytes)
        .map_err(|_| v603h_err(V603hCode::NameDeriveOverflow, "derive_segments::namesize"))?;
    let data_len = super::unx603_cmagic::decode_int_field(fs_bytes)
        .map_err(|_| v603h_err(V603hCode::DataDeriveOverflow, "derive_segments::filesize"))?;
    let name_off = NEWC_HEADER_LEN as u32;
    let name_end = match name_off.checked_add(name_len) {
        None => return Err(v603h_err(V603hCode::NameDeriveOverflow, "derive_segments::name_end")),
        Some(v) => v,
    };
    // name 段不得越过缓冲。
    if name_end > buf.len() as u32 {
        return Err(v603h_err(V603hCode::NameDeriveOverflow, "derive_segments::name_bounds"));
    }
    let data_off = align_up(name_end)?;
    let data_end = match data_off.checked_add(data_len) {
        None => return Err(v603h_err(V603hCode::DataDeriveOverflow, "derive_segments::data_end")),
        Some(v) => v,
    };
    Ok(DerivedSegments { name_off, name_len, data_off, data_len, entry_end: data_end })
}

/// 布局指纹（对槽表的确定性摘要）。
pub fn layout_digest() -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < HEADER_SLOTS {
        if let Some(s) = HEADER_LAYOUT.get(i) {
            let nb = s.name.as_bytes();
            let mut t = 0usize;
            while t < nb.len() {
                if let Some(&b) = nb.get(t) {
                    h ^= b as u32;
                    h = h.wrapping_mul(0x0100_0193);
                }
                t += 1;
            }
            let ob = s.offset.to_le_bytes();
            let mut u = 0usize;
            while u < ob.len() {
                if let Some(&b) = ob.get(u) {
                    h ^= b as u32;
                    h = h.wrapping_mul(0x0100_0193);
                }
                u += 1;
            }
            let wb = (s.width as u32).to_le_bytes();
            let mut v = 0usize;
            while v < wb.len() {
                if let Some(&b) = wb.get(v) {
                    h ^= b as u32;
                    h = h.wrapping_mul(0x0100_0193);
                }
                v += 1;
            }
        }
        i += 1;
    }
    h
}

/// 布局封账（指纹 + 合法性自检一次性完成）。
pub fn layout_seal()-> Result<u32, V603hErr> {
    if layout_scan() != 0 {
        return Err(v603h_err(V603hCode::LayoutSumMismatch, "layout_seal"));
    }
    Ok(layout_digest())
}
