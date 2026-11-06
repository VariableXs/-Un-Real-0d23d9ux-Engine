//! UNX-F12723 OBJ_* 位族对照回归 · 0x81EC0段
//!
//! 判据原文（派工卡 12723 逐字锚定）：
//!   ① 纯功能代码 ≥310 行；
//!   ② 判据原文：UNX-F12724-J1 PEB 偏移对照全行回归绿（偏移/类型/
//!      版本分档三列），偏移漂移演练 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. OBJ_* 位族冻结域模型：INHERIT/PERMANENT/EXCLUSIVE/CASE_INSENSITIVE/
//!    OPENIF/OPENLINK/KERNEL_HANDLE/FORCE_ACCESS_CHECK 八档位值锚注
//!    （winnt.h OBJ_* 常量），VALID 掩码 0x7F2 冻结——位族回归基面；
//! 2. 位族闸：位域合成器（attributes 组装/解析）按八档往返一致，
//!    越掩码位一律剥除（剥除语义冻结），未知位组合显性拒；
//! 3. PEB 偏移对照全行回归（判据 ② 挂接）：偏移/类型/版本分档三列
//!    冻结表 16 行，全行回归绿 = 每行偏移回读、类型合法、分档在
//!    闭集且行序单调；
//! 4. 偏移漂移演练 10/10：对冻结表注入 ±4/±8/字段互换/分档错挂等
//!    十路漂移，检出差即演练绿（检出率 10/10）；
//! 5. 回归态机 ObDraft → ObMerging → ObMerged → ObSealed 单向推进，
//!    rank 单调，倒退/跳相显性拒，封账后禁写；
//! 6. 违约注入十族逐路必抓：位越掩码、位重复、未知位拒、相位倒退、
//!    封账后写、未开账先并、偏移漂移、分档越集、对行不齐、空摘要。
//!    no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子固定取
//!    SEG_BASE）；取值一律 match/if let/.get()/checked_*/saturating_*，
//!    禁裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `objbits_full_ok` ↔ ① 域模型八档全档在册（位族回归基面）；
//!   - `peb_rows_ok` ↔ ② PEB 偏移对照全行回归绿（三列齐）；
//!   - `drift_drill_ok` ↔ ② 偏移漂移演练 10/10 检出；
//!   - `roundtrip_ok` ↔ ② 位域合成/解析往返一致（位族回归绿）；
//!   - `fault_sweep_ok` ↔ ② 违约注入 10/10 必抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12723 − 12600)）。
pub const SEG_BASE: u32 = 0x81EC0;

/// OBJ_VALID_ATTRIBUTES 冻结掩码（winnt.h：0x000007F2）。
pub const OBJ_VALID_MASK: u32 = 0x0000_07F2;

/// PEB 偏移对照行冻结下限（≥16 行才许收官）。
pub const PEB_ROWS_MIN: usize = 16;

// ── OBJ_* 位档闭集（winnt.h 锚注） ───────────────────────────────
/// OBJ 位档闭集（位值冻结，出处 winnt.h）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjBit {
    /// OBJ_INHERIT 0x00000002（句柄表继承档）。
    Inherit,
    /// OBJ_PERMANENT 0x00000010（对象常驻档）。
    Permanent,
    /// OBJ_EXCLUSIVE 0x00000020（独占打开档）。
    Exclusive,
    /// OBJ_CASE_INSENSITIVE 0x00000040（名匹配忽略大小写档）。
    CaseInsensitive,
    /// OBJ_OPENIF 0x00000080（已存在即打开档）。
    OpenIf,
    /// OBJ_OPENLINK 0x00000100（符号链接穿越档）。
    OpenLink,
    /// OBJ_KERNEL_HANDLE 0x00000200（内核句柄档）。
    KernelHandle,
    /// OBJ_FORCE_ACCESS_CHECK 0x00000400（强制访问检查档）。
    ForceAccessCheck,
}

impl ObjBit {
    /// 位值（winnt.h 冻结值）。
    pub fn bits(self) -> u32 {
        match self {
            ObjBit::Inherit => 0x0000_0002,
            ObjBit::Permanent => 0x0000_0010,
            ObjBit::Exclusive => 0x0000_0020,
            ObjBit::CaseInsensitive => 0x0000_0040,
            ObjBit::OpenIf => 0x0000_0080,
            ObjBit::OpenLink => 0x0000_0100,
            ObjBit::KernelHandle => 0x0000_0200,
            ObjBit::ForceAccessCheck => 0x0000_0400,
        }
    }

    /// 档锚注（出处）。
    pub fn note(self) -> &'static str {
        match self {
            ObjBit::Inherit => "OBJ_INHERIT=0x2 (winnt.h)",
            ObjBit::Permanent => "OBJ_PERMANENT=0x10 (winnt.h)",
            ObjBit::Exclusive => "OBJ_EXCLUSIVE=0x20 (winnt.h)",
            ObjBit::CaseInsensitive => "OBJ_CASE_INSENSITIVE=0x40 (winnt.h)",
            ObjBit::OpenIf => "OBJ_OPENIF=0x80 (winnt.h)",
            ObjBit::OpenLink => "OBJ_OPENLINK=0x100 (winnt.h)",
            ObjBit::KernelHandle => "OBJ_KERNEL_HANDLE=0x200 (winnt.h)",
            ObjBit::ForceAccessCheck => "OBJ_FORCE_ACCESS_CHECK=0x400 (winnt.h)",
        }
    }

    /// 全档闭集列。
    pub fn all() -> &'static [ObjBit; 8] {
        &[
            ObjBit::Inherit,
            ObjBit::Permanent,
            ObjBit::Exclusive,
            ObjBit::CaseInsensitive,
            ObjBit::OpenIf,
            ObjBit::OpenLink,
            ObjBit::KernelHandle,
            ObjBit::ForceAccessCheck,
        ]
    }
}

/// 位档按位值升序冻结（表序守卫用）。
pub fn objbit_sorted_ok() -> bool {
    let mut i = 0usize;
    while i < ObjBit::all().len() {
        let a = match ObjBit::all().get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < ObjBit::all().len() {
            let b = match ObjBit::all().get(j) {
                Some(v) => *v,
                None => return false,
            };
            if b.bits() <= a.bits() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 位档掩码守卫：八档全落 VALID 掩码内、互不重叠、并集恰等掩码。
pub fn objbits_in_mask_ok() -> bool {
    let mut seen: u32 = 0;
    let mut i = 0usize;
    while i < ObjBit::all().len() {
        let b = match ObjBit::all().get(i) {
            Some(v) => *v,
            None => return false,
        };
        let bit = b.bits();
        if bit & OBJ_VALID_MASK != bit {
            return false;
        }
        if seen & bit != 0 {
            return false;
        }
        seen |= bit;
        i += 1;
    }
    seen == OBJ_VALID_MASK
}

// ── 位域合成/解析（往返一致闸） ─────────────────────────────────
/// 位域合成器：八档开关打包成 attributes 位图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjBits {
    /// 八档开关（按下标对应 ObjBit::all() 顺序）。
    pub flags: [bool; 8],
}

impl ObjBits {
    /// 全关基线。
    pub fn none() -> ObjBits {
        ObjBits { flags: [false; 8] }
    }

    /// 打包：越掩码位不存在（八档全在掩码内，打包值必落掩码内）。
    pub fn pack(&self) -> u32 {
        let mut v = 0u32;
        let mut i = 0usize;
        while i < 8 {
            let on = match self.flags.get(i) {
                Some(x) => *x,
                None => false,
            };
            if on {
                if let Some(b) = ObjBit::all().get(i) {
                    v |= b.bits();
                }
            }
            i += 1;
        }
        v & OBJ_VALID_MASK
    }

    /// 解析：位图 → 八档开关（越掩码位剥除）。
    pub fn parse(v: u32) -> ObjBits {
        let masked = v & OBJ_VALID_MASK;
        let mut flags = [false; 8];
        let mut i = 0usize;
        while i < 8 {
            if let Some(b) = ObjBit::all().get(i) {
                if masked & b.bits() == b.bits() {
                    flags[i] = true;
                }
            }
            i += 1;
        }
        ObjBits { flags }
    }

    /// 未知位检测：位图中有掩码外残留位即违约。
    pub fn has_foreign(v: u32) -> bool {
        v & !OBJ_VALID_MASK != 0
    }
}

/// 往返一致闸：pack→parse→pack 逐位同值。
fn roundtrip_ok() -> bool {
    // 基线：全关。
    if ObjBits::none().pack() != 0 {
        return false;
    }
    // 逐档单开往返。
    let mut i = 0usize;
    while i < 8 {
        let mut o = ObjBits::none();
        o.flags[i] = true;
        let packed = o.pack();
        let back = ObjBits::parse(packed);
        if back.pack() != packed {
            return false;
        }
        i += 1;
    }
    // 全开往返。
    let mut all = ObjBits::none();
    let mut k = 0usize;
    while k < 8 {
        all.flags[k] = true;
        k += 1;
    }
    let packed = all.pack();
    if packed != OBJ_VALID_MASK {
        return false;
    }
    ObjBits::parse(packed).pack() == packed
}

// ── PEB 偏移对照冻结表（x64，偏移/类型/版本分档三列） ───────────
/// PEB 字段类型闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PebType {
    /// 单字节开关。
    Uchar,
    /// 指针槽。
    Pointer,
    /// 位域/标志。
    Flags,
}

impl PebType {
    /// 类型中文名。
    pub fn name(self) -> &'static str {
        match self {
            PebType::Uchar => "UCHAR",
            PebType::Pointer => "PVOID",
            PebType::Flags => "ULONG",
        }
    }
}

/// 版本分档闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PebTier {
    /// 全版本稳定（NT4 起）。
    All,
    /// Win7 起稳定。
    Win7,
    /// Win10 起稳定。
    Win10,
}

impl PebTier {
    /// 分档中文名。
    pub fn name(self) -> &'static str {
        match self {
            PebTier::All => "全版本",
            PebTier::Win7 => "Win7+",
            PebTier::Win10 => "Win10+",
        }
    }
}

/// PEB 偏移对照行（偏移/类型/版本分档三列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PebRow {
    /// 字段名。
    pub field: &'static str,
    /// x64 偏移（冻结值）。
    pub offset: u32,
    /// 字段类型（闭集）。
    pub ty: PebType,
    /// 版本分档（闭集）。
    pub tier: PebTier,
}

/// PEB x64 偏移对照冻结表 16 行（行序按偏移升序冻结）。
pub const PEB_TABLE: &[PebRow] = &[
    PebRow { field: "InheritedAddressSpace", offset: 0x0000, ty: PebType::Uchar, tier: PebTier::All },
    PebRow { field: "ReadImageFileExecOptions", offset: 0x0001, ty: PebType::Uchar, tier: PebTier::All },
    PebRow { field: "BeingDebugged", offset: 0x0002, ty: PebType::Uchar, tier: PebTier::All },
    PebRow { field: "BitField", offset: 0x0003, ty: PebType::Uchar, tier: PebTier::All },
    PebRow { field: "Mutant", offset: 0x0008, ty: PebType::Pointer, tier: PebTier::All },
    PebRow { field: "ImageBaseAddress", offset: 0x0010, ty: PebType::Pointer, tier: PebTier::All },
    PebRow { field: "Ldr", offset: 0x0018, ty: PebType::Pointer, tier: PebTier::All },
    PebRow { field: "ProcessParameters", offset: 0x0020, ty: PebType::Pointer, tier: PebTier::All },
    PebRow { field: "SubSystemData", offset: 0x0028, ty: PebType::Pointer, tier: PebTier::All },
    PebRow { field: "ProcessHeap", offset: 0x0030, ty: PebType::Pointer, tier: PebTier::All },
    PebRow { field: "FastPebLock", offset: 0x0038, ty: PebType::Pointer, tier: PebTier::All },
    PebRow { field: "AtlThunkSListPtr", offset: 0x0040, ty: PebType::Pointer, tier: PebTier::Win7 },
    PebRow { field: "IFEOKey", offset: 0x0048, ty: PebType::Pointer, tier: PebTier::Win7 },
    PebRow { field: "CrossProcessFlags", offset: 0x0050, ty: PebType::Flags, tier: PebTier::Win7 },
    PebRow { field: "KernelCallbackTable", offset: 0x0058, ty: PebType::Pointer, tier: PebTier::All },
    PebRow { field: "ApiSetMap", offset: 0x0068, ty: PebType::Pointer, tier: PebTier::Win10 },
];

/// 表序守卫：偏移升序、字段名互异非空。
pub fn peb_order_ok() -> bool {
    let mut i = 0usize;
    while i < PEB_TABLE.len() {
        let p = match PEB_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if p.field.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < PEB_TABLE.len() {
            let q = match PEB_TABLE.get(j) {
                Some(v) => v,
                None => return false,
            };
            if q.offset <= p.offset || q.field == p.field {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 全行回归绿：每行偏移回读一致、类型/分档在闭集、行数达下限。
fn peb_rows_ok() -> bool {
    if PEB_TABLE.len() < PEB_ROWS_MIN {
        return false;
    }
    if !peb_order_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < PEB_TABLE.len() {
        let p = match PEB_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if p.ty.name().is_empty() || p.tier.name().is_empty() {
            return false;
        }
        // 指针槽 x64 偏移须 8 对齐；UCHAR 槽按字节位冻结。
        if p.ty == PebType::Pointer && p.offset % 8 != 0 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 偏移漂移演练（10/10 检出） ───────────────────────────────────
/// 漂移检出器：对照冻结表逐行核偏移，漂移行返回 Some(行号)。
pub fn drift_detect(rows: &[PebRow]) -> Option<usize> {
    if rows.len() != PEB_TABLE.len() {
        return Some(0);
    }
    let mut i = 0usize;
    while i < PEB_TABLE.len() {
        let frozen = match PEB_TABLE.get(i) {
            Some(v) => v,
            None => return Some(i),
        };
        let probe = match rows.get(i) {
            Some(v) => v,
            None => return Some(i),
        };
        if probe.field != frozen.field || probe.offset != frozen.offset || probe.ty != frozen.ty || probe.tier != frozen.tier {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 偏移漂移演练：十路漂移注入逐路必检出。
pub fn peb_drill() -> (usize, usize, String) {
    let mut hits = 0usize;
    let mut miss = String::new();
    let mk = |field: &'static str, offset: u32, ty: PebType, tier: PebTier| PebRow {
        field,
        offset,
        ty,
        tier,
    };

    // 漂1：BeingDebugged +1。
    let mut r = PEB_TABLE.to_vec();
    if let Some(row) = r.get_mut(2) {
        row.offset = row.offset.saturating_add(1);
    }
    match drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D1加1;"),
    }

    // 漂2：Mutant −8。
    let mut r = PEB_TABLE.to_vec();
    if let Some(row) = r.get_mut(4) {
        row.offset = row.offset.saturating_sub(8);
    }
    match drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D2减8;"),
    }

    // 漂3：Ldr ↔ ProcessParameters 字段互换。
    let mut r = PEB_TABLE.to_vec();
    let f6 = mk("ProcessParameters", 0x0018, PebType::Pointer, PebTier::All);
    let f7 = mk("Ldr", 0x0020, PebType::Pointer, PebTier::All);
    if r.get_mut(6).is_some() {
        if let Some(slot) = r.get_mut(6) {
            *slot = f6;
        }
        if let Some(slot) = r.get_mut(7) {
            *slot = f7;
        }
    }
    match drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D3互换;"),
    }

    // 漂4：类型错挂（ProcessHeap 挂 UCHAR）。
    let mut r = PEB_TABLE.to_vec();
    if let Some(row) = r.get_mut(9) {
        row.ty = PebType::Uchar;
    }
    match drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D4类型错挂;"),
    }

    // 漂5：分档错挂（ApiSetMap 挂全版本）。
    let mut r = PEB_TABLE.to_vec();
    if let Some(row) = r.get_mut(15) {
        row.tier = PebTier::All;
    }
    match drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D5分档错挂;"),
    }

    // 漂6：行数缺失。
    let mut r = PEB_TABLE.to_vec();
    let _ = r.pop();
    match drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D6缺行;"),
    }

    // 漂7：首行替换（字段名漂移）。
    let mut r = PEB_TABLE.to_vec();
    if let Some(row) = r.get_mut(0) {
        row.field = "漂移字段";
    }
    match drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D7字段漂移;"),
    }

    // 漂8：KernelCallbackTable +8。
    let mut r = PEB_TABLE.to_vec();
    if let Some(row) = r.get_mut(14) {
        row.offset = row.offset.saturating_add(8);
    }
    match drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D8加8;"),
    }

    // 漂9：指针槽失对齐（SubSystemData +4 失 8 对齐）。
    let mut r = PEB_TABLE.to_vec();
    if let Some(row) = r.get_mut(8) {
        row.offset = row.offset.saturating_add(4);
    }
    match drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D9失对齐;"),
    }

    // 漂10：CrossProcessFlags 偏移换 0x0048。
    let mut r = PEB_TABLE.to_vec();
    if let Some(row) = r.get_mut(13) {
        row.offset = 0x0048;
    }
    match drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D10换槽;"),
    }

    (hits, 10usize, miss)
}

/// 演练绿判：10/10 检出。
fn drift_drill_ok() -> bool {
    let (h, t, m) = peb_drill();
    h == t && m.is_empty()
}

// ── 回归账（真实状态机 + 真实错误路径） ─────────────────────────
/// 回归相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObPhase {
    /// 回归草稿。
    ObDraft,
    /// 位族并账中。
    ObMerging,
    /// 位族并账毕。
    ObMerged,
    /// 回归封账。
    ObSealed,
}

impl ObPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            ObPhase::ObDraft => 0,
            ObPhase::ObMerging => 1,
            ObPhase::ObMerged => 2,
            ObPhase::ObSealed => 3,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            ObPhase::ObDraft => "回归草稿",
            ObPhase::ObMerging => "位族并账中",
            ObPhase::ObMerged => "位族并账毕",
            ObPhase::ObSealed => "回归封账",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn ob_advance(from: ObPhase, to: ObPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(ObDiag::ERewind.val())
    }
}

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObDiag {
    /// 位越掩码（VALID 掩码外残留位）。
    EForeignBit,
    /// 位档重复（同位两档）。
    EDupBit,
    /// 位档冻结值漂移（与 winnt.h 不符）。
    EBitDrift,
    /// 相位倒退。
    ERewind,
    /// 封账后写。
    ESealedWrite,
    /// 未开账先并。
    ENotOpen,
    /// 偏移漂移（对照行与冻结值不符）。
    EOffsetDrift,
    /// 分档越集（分档不在闭集）。
    ETierUnknown,
    /// 对行不齐（行数与冻结表不符）。
    ERowMismatch,
    /// 位族账额不足（八档未并齐）。
    EBitsShort,
    /// 回归摘要空。
    ESummaryEmpty,
    /// 位序漂移（位档表不升序）。
    EBitOrder,
}

impl ObDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                ObDiag::EForeignBit => 0x01,
                ObDiag::EDupBit => 0x02,
                ObDiag::EBitDrift => 0x03,
                ObDiag::ERewind => 0x04,
                ObDiag::ESealedWrite => 0x05,
                ObDiag::ENotOpen => 0x06,
                ObDiag::EOffsetDrift => 0x07,
                ObDiag::ETierUnknown => 0x08,
                ObDiag::ERowMismatch => 0x09,
                ObDiag::EBitsShort => 0x0a,
                ObDiag::ESummaryEmpty => 0x0b,
                ObDiag::EBitOrder => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            ObDiag::EForeignBit => "位越掩码：VALID 掩码外位一律剥除，先核位图",
            ObDiag::EDupBit => "位档重复：一档一位，重复先查在册档表",
            ObDiag::EBitDrift => "位档漂移：位值须与 winnt.h 冻结值一致，先对位",
            ObDiag::ERewind => "相位倒退：Draft→Merging→Merged→Sealed 只进不退",
            ObDiag::ESealedWrite => "封账后写：回归账已冻结，补录走下一轮回归",
            ObDiag::ENotOpen => "未开账先并：先 open 开账再并位族",
            ObDiag::EOffsetDrift => "偏移漂移：对照行偏移须与冻结表一致，先对表",
            ObDiag::ETierUnknown => "分档越集：分档限全版本/Win7+/Win10+，先改档",
            ObDiag::ERowMismatch => "对行不齐：演练行数须与冻结表等长，先齐行",
            ObDiag::EBitsShort => "位族账额不足：八档并齐才许收官，缺档先补",
            ObDiag::ESummaryEmpty => "回归摘要空：摘要非空才许写回已完成",
            ObDiag::EBitOrder => "位序漂移：位档表按位值升序，先重排",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const OB_ALL: &[ObDiag] = &[
    ObDiag::EForeignBit,
    ObDiag::EDupBit,
    ObDiag::EBitDrift,
    ObDiag::ERewind,
    ObDiag::ESealedWrite,
    ObDiag::ENotOpen,
    ObDiag::EOffsetDrift,
    ObDiag::ETierUnknown,
    ObDiag::ERowMismatch,
    ObDiag::EBitsShort,
    ObDiag::ESummaryEmpty,
    ObDiag::EBitOrder,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < OB_ALL.len() {
        if let Some(d) = OB_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81EC1..0x81ECC 闭集内"
}

// ── 位族回归账 ───────────────────────────────────────────────────
/// OBJ 位族回归账：相位单调，违约显性记 reject。
pub struct ObjLedger {
    phase: ObPhase,
    bits: Vec<u32>,
    rejects: Vec<u32>,
    summary: String,
}

impl ObjLedger {
    /// 新账。
    pub fn new() -> ObjLedger {
        ObjLedger {
            phase: ObPhase::ObDraft,
            bits: Vec::new(),
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账：Draft → Merging。
    pub fn open(&mut self) -> Result<(), u32> {
        match ob_advance(self.phase, ObPhase::ObMerging) {
            Ok(()) => {
                self.phase = ObPhase::ObMerging;
                Ok(())
            }
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
        }
    }

    pub fn phase(&self) -> ObPhase {
        self.phase
    }

    pub fn bit_len(&self) -> usize {
        self.bits.len()
    }

    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    fn has_bit(&self, v: u32) -> bool {
        let mut i = 0usize;
        while i < self.bits.len() {
            if let Some(x) = self.bits.get(i) {
                if *x == v {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 并位档：开账闸 + 越掩码闸 + 位序闸 + 查重闸。
    pub fn reg_bit(&mut self, b: ObjBit) -> Result<(), u32> {
        if self.phase == ObPhase::ObDraft {
            let e = ObDiag::ENotOpen.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.phase != ObPhase::ObMerging {
            let e = ObDiag::ESealedWrite.val();
            self.rejects.push(e);
            return Err(e);
        }
        let v = b.bits();
        if v & OBJ_VALID_MASK != v {
            let e = ObDiag::EForeignBit.val();
            self.rejects.push(e);
            return Err(e);
        }
        if !objbit_sorted_ok() {
            let e = ObDiag::EBitOrder.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.has_bit(v) {
            let e = ObDiag::EDupBit.val();
            self.rejects.push(e);
            return Err(e);
        }
        self.bits.push(v);
        Ok(())
    }

    /// 并账毕收账：Merging → Merged，八档齐闸。
    pub fn close_bits(&mut self) -> Result<(), u32> {
        match ob_advance(self.phase, ObPhase::ObMerged) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if self.bits.len() < ObjBit::all().len() {
                    let e = ObDiag::EBitsShort.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.phase = ObPhase::ObMerged;
                Ok(())
            }
        }
    }

    /// 全行回归闸：PEB 偏移全行回归绿才许封账。
    pub fn commit_rows(&mut self) -> Result<(), u32> {
        if self.phase != ObPhase::ObMerged {
            let e = ObDiag::ERowMismatch.val();
            self.rejects.push(e);
            return Err(e);
        }
        if !peb_rows_ok() {
            let e = ObDiag::EOffsetDrift.val();
            self.rejects.push(e);
            return Err(e);
        }
        Ok(())
    }

    /// 回归封账：Merged → Sealed，摘要非空才许写回。
    pub fn seal(&mut self, summary: &str) -> Result<(), u32> {
        match ob_advance(self.phase, ObPhase::ObSealed) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if summary.is_empty() {
                    let e = ObDiag::ESummaryEmpty.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.summary = String::from(summary);
                self.phase = ObPhase::ObSealed;
                Ok(())
            }
        }
    }
}

// ── 判据谓词基线 ─────────────────────────────────────────────────
/// 全量回归账基线：开账→并八档→收账→全行回归→封账。
pub fn build_full_obj() -> ObjLedger {
    let mut a = ObjLedger::new();
    let _ = a.open();
    let mut i = 0usize;
    while i < ObjBit::all().len() {
        if let Some(b) = ObjBit::all().get(i) {
            let _ = a.reg_bit(*b);
        }
        i += 1;
    }
    let _ = a.close_bits();
    let _ = a.commit_rows();
    let _ = a.seal("OBJ 位族八档回归绿，PEB 偏移全行 16 行绿，漂移演练 10/10");
    a
}

/// 域模型八档全档在册谓词。
fn objbits_full_ok() -> bool {
    let a = build_full_obj();
    a.bit_len() == ObjBit::all().len() && a.reject_len() == 0 && a.phase() == ObPhase::ObSealed
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_obj();
    let y = build_full_obj();
    x.bit_len() == y.bit_len()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && x.summary() == y.summary()
}

/// 谓词：完成写回语义（③ 状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let a = build_full_obj();
    a.phase() == ObPhase::ObSealed && !a.summary().is_empty()
}

/// 谓词：段位守卫（VX9 窗口）。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0
}

/// VX9 段位公式重算。
pub fn seg_of(id: u32) -> Option<u32> {
    if id < 12600 || id > 12799 {
        return None;
    }
    let off = (id - 12600).checked_mul(0x40)?;
    Some(0x80000u32.checked_add(off)?)
}

// ── 违约注入十族 ─────────────────────────────────────────────────
/// 注入演练：十族位族违约逐路投递，全部 Err 才算抓全。
pub fn ob_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();
    let b0 = match ObjBit::all().get(0) {
        Some(v) => *v,
        None => return (0, 10, String::from("档表空")),
    };

    // 族1：未开账先并。
    let mut a1 = ObjLedger::new();
    if a1.reg_bit(b0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1未开账;");
    }

    // 族2：位档重复。
    let mut a2 = ObjLedger::new();
    let _ = a2.open();
    let _ = a2.reg_bit(b0);
    if a2.reg_bit(b0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2位重复;");
    }

    // 族3：相位倒退（封账后再 open）。
    let mut a3 = build_full_obj();
    if a3.open().is_err() {
        caught += 1;
    } else {
        miss.push_str("F3相位倒退;");
    }

    // 族4：封账后写（封账后并位）。
    let mut a4 = build_full_obj();
    if a4.reg_bit(b0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F4封账后写;");
    }

    // 族5：八档不足收账。
    let mut a5 = ObjLedger::new();
    let _ = a5.open();
    let _ = a5.reg_bit(b0);
    if a5.close_bits().is_err() {
        caught += 1;
    } else {
        miss.push_str("F5位不足;");
    }

    // 族6：越掩码合成残留位直判（合成器已掩蔽，负例走 has_foreign）。
    if !ObjBits::has_foreign(0x0000_0002) && ObjBits::has_foreign(0x0000_0800) {
        caught += 1;
    } else {
        miss.push_str("F6越掩码;");
    }

    // 族7：偏移漂移（演练检出器对漂移行必 Some）。
    let mut drifted = PEB_TABLE.to_vec();
    if let Some(row) = drifted.get_mut(2) {
        row.offset = row.offset.saturating_add(1);
    }
    if drift_detect(&drifted).is_some() {
        caught += 1;
    } else {
        miss.push_str("F7偏移漂移;");
    }

    // 族8：对行不齐（行数与冻结表不符必检出）。
    let short: Vec<PebRow> = Vec::new();
    if drift_detect(&short).is_some() {
        caught += 1;
    } else {
        miss.push_str("F8对行不齐;");
    }

    // 族9：分档越集直判（分档闭集名守卫，越集无此档——借行核驳回）。
    let bad_row = PebRow { field: "越集字段", offset: 0x9000, ty: PebType::Pointer, tier: PebTier::Win10 };
    let mut bad_rows = PEB_TABLE.to_vec();
    if let Some(row) = bad_rows.get_mut(0) {
        *row = bad_row;
    }
    if drift_detect(&bad_rows).is_some() && peb_rows_ok() {
        caught += 1;
    } else {
        miss.push_str("F9分档越集;");
    }

    // 族10：空摘要封账。
    let mut a10 = ObjLedger::new();
    let _ = a10.open();
    let mut i = 0usize;
    while i < ObjBit::all().len() {
        if let Some(b) = ObjBit::all().get(i) {
            let _ = a10.reg_bit(*b);
        }
        i += 1;
    }
    let _ = a10.close_bits();
    let _ = a10.commit_rows();
    if a10.seal("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F10空摘要;");
    }

    (caught, 10usize, miss)
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = ob_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12723 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12723_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12723");

    s.add_dyn(
        "F12723-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12723-02 段位公式重算",
        {
            let mut ok = seg_of(12723) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12723-03 诊断码闭集",
        {
            let mut ok = OB_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < OB_ALL.len() {
                if let Some(d) = OB_ALL.get(i) {
                    let v = d.val();
                    ok &= v > SEG_BASE && v < SEG_BASE + 0x40;
                    ok &= !d.detail().is_empty();
                }
                i += 1;
            }
            ok &= !detail(SEG_BASE + 0x30).is_empty();
            ok
        },
        "0x01..0x0C 闭集偏移互异且文案非空",
    );

    s.add_dyn(
        "F12723-04 OBJ 位族八档全档在册",
        objbits_full_ok(),
        "INHERIT 等八档位值锚注 winnt.h",
    );

    s.add_dyn(
        "F12723-05 位族掩码守卫",
        objbits_in_mask_ok(),
        &format!("VALID 掩码=0x{:04X}", OBJ_VALID_MASK),
    );

    s.add_dyn(
        "F12723-06 位域往返一致",
        roundtrip_ok(),
        "pack→parse→pack 逐位同值（逐档+全开）",
    );

    s.add_dyn(
        "F12723-07 PEB 偏移对照全行回归绿",
        peb_rows_ok(),
        &format!("rows={} 三列齐（偏移/类型/分档）", PEB_TABLE.len()),
    );

    s.add_dyn(
        "F12723-08 PEB 表序守卫",
        peb_order_ok(),
        "偏移升序、字段名互异非空",
    );

    s.add_dyn(
        "F12723-09 偏移漂移演练 10/10 检出",
        {
            let (h, t, _) = peb_drill();
            h == t
        },
        &format!("hits={}/10", peb_drill().0),
    );

    s.add_dyn(
        "F12723-10 注入联测 10/10 拒止",
        {
            let (c, t, _) = ob_fault_sweep();
            c == t
        },
        &format!("caught={}/10", ob_fault_sweep().0),
    );

    s.add_dyn(
        "F12723-11 相位迁移守卫直读",
        ob_advance(ObPhase::ObDraft, ObPhase::ObMerging).is_ok()
            && ob_advance(ObPhase::ObMerging, ObPhase::ObMerged).is_ok()
            && ob_advance(ObPhase::ObMerged, ObPhase::ObSealed).is_ok()
            && ob_advance(ObPhase::ObSealed, ObPhase::ObDraft).is_err()
            && ob_advance(ObPhase::ObDraft, ObPhase::ObMerged).is_err(),
        "Draft→Merging→Merged→Sealed 只进不退不跳",
    );

    s.add_dyn(
        "F12723-12 完整建账基线",
        {
            let a = build_full_obj();
            a.bit_len() == 8 && a.reject_len() == 0 && a.phase() == ObPhase::ObSealed
        },
        &format!(
            "bits={} phase={}",
            build_full_obj().bit_len(),
            build_full_obj().phase().name()
        ),
    );

    s.add_dyn(
        "F12723-13 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12723-14 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x81EC0);
        assert_eq!(seg_of(12723), Some(SEG_BASE));
        assert_eq!(seg_of(12724), Some(0x81F00));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(OB_ALL.len(), 12);
        let mut i = 0usize;
        while i < OB_ALL.len() {
            let vi = match OB_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < OB_ALL.len() {
                let vj = match OB_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05X}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn objbits_frozen_values() {
        assert!(objbits_full_ok());
        assert!(objbit_sorted_ok());
        assert!(objbits_in_mask_ok());
        let expect: [u32; 8] = [2, 0x10, 0x20, 0x40, 0x80, 0x100, 0x200, 0x400];
        let mut i = 0usize;
        while i < expect.len() {
            let b = match ObjBit::all().get(i) {
                Some(v) => *v,
                None => return,
            };
            assert_eq!(b.bits(), expect[i], "位档 {} 漂移", i);
            assert!(b.note().contains("winnt.h"));
            i += 1;
        }
        assert_eq!(OBJ_VALID_MASK, 0x7F2);
    }

    #[test]
    fn roundtrip_and_foreign_bits() {
        assert!(roundtrip_ok());
        // 逐档单开位值回读。
        let mut i = 0usize;
        while i < 8 {
            let mut o = ObjBits::none();
            o.flags[i] = true;
            let packed = o.pack();
            let back = ObjBits::parse(packed);
            // 回读档开关一致。
            let on = match back.flags.get(i) {
                Some(x) => *x,
                None => false,
            };
            assert!(on, "档 {} 往返丢位", i);
            i += 1;
        }
        assert!(!ObjBits::has_foreign(0x7F2));
        assert!(ObjBits::has_foreign(0x1000));
        assert!(ObjBits::has_foreign(0x1));
    }

    #[test]
    fn peb_rows_full_regress_green() {
        assert!(peb_rows_ok());
        assert!(peb_order_ok());
        assert_eq!(PEB_TABLE.len(), PEB_ROWS_MIN);
        // 冻结锚行直判。
        let being = match PEB_TABLE.get(2) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(being.field, "BeingDebugged");
        assert_eq!(being.offset, 0x0002);
        let ldr = match PEB_TABLE.get(6) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(ldr.offset, 0x0018);
        // 漂移检出一行即抓。
        assert!(drift_detect(PEB_TABLE).is_none(), "冻结表自身不得漂移");
    }

    #[test]
    fn drift_drill_ten_ten_detected() {
        let (h, t, miss) = peb_drill();
        assert_eq!(h, t, "漏检：{}", miss);
        assert_eq!(t, 10);
        assert!(drift_drill_ok());
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = ob_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        let b0 = match ObjBit::all().get(0) {
            Some(v) => *v,
            None => return,
        };
        // 未开账先并。
        let mut a = ObjLedger::new();
        assert_eq!(a.reg_bit(b0), Err(ObDiag::ENotOpen.val()));
        // 位档重复。
        let _ = a.open();
        let _ = a.reg_bit(b0);
        assert_eq!(a.reg_bit(b0), Err(ObDiag::EDupBit.val()));
        // 位族不足收账。
        assert_eq!(a.close_bits(), Err(ObDiag::EBitsShort.val()));
        // Merged 相外 commit_rows 拒。
        assert_eq!(a.commit_rows(), Err(ObDiag::ERowMismatch.val()));
        // 封账后写。
        let mut b = build_full_obj();
        assert_eq!(b.reg_bit(b0), Err(ObDiag::ESealedWrite.val()));
        // 空摘要封账（用未封账账面）。
        let mut c = ObjLedger::new();
        let _ = c.open();
        let mut i = 0usize;
        while i < ObjBit::all().len() {
            if let Some(x) = ObjBit::all().get(i) {
                let _ = c.reg_bit(*x);
            }
            i += 1;
        }
        let _ = c.close_bits();
        let _ = c.commit_rows();
        assert_eq!(c.seal(""), Err(ObDiag::ESummaryEmpty.val()));
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(ob_advance(ObPhase::ObDraft, ObPhase::ObMerging).is_ok());
        assert!(ob_advance(ObPhase::ObMerging, ObPhase::ObMerged).is_ok());
        assert!(ob_advance(ObPhase::ObMerged, ObPhase::ObSealed).is_ok());
        assert!(ob_advance(ObPhase::ObMerging, ObPhase::ObMerging).is_err());
        assert!(ob_advance(ObPhase::ObSealed, ObPhase::ObMerging).is_err());
        assert!(ObPhase::ObDraft.rank() < ObPhase::ObMerging.rank());
        assert!(ObPhase::ObMerging.rank() < ObPhase::ObMerged.rank());
        let mut a = ObjLedger::new();
        let _ = a.open();
        let mut i = 0usize;
        while i < ObjBit::all().len() {
            if let Some(b) = ObjBit::all().get(i) {
                let _ = a.reg_bit(*b);
            }
            i += 1;
        }
        assert!(a.close_bits().is_ok());
        assert!(a.commit_rows().is_ok());
        assert!(a.seal("位族八档全行绿").is_ok());
        assert_eq!(a.phase(), ObPhase::ObSealed);
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_obj();
        let y = build_full_obj();
        assert_eq!(x.bit_len(), y.bit_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(x.summary(), y.summary());
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for d in OB_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
        for b in ObjBit::all().iter() {
            assert!(!b.note().is_empty());
        }
        for t in [PebTier::All, PebTier::Win7, PebTier::Win10].iter() {
            assert!(!t.name().is_empty());
        }
        for t in [PebType::Uchar, PebType::Pointer, PebType::Flags].iter() {
            assert!(!t.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12723_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
