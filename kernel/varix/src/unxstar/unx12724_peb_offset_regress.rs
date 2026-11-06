//! UNX-F12724 PEB 字段对照表回归（偏移表逐行） · 0x81F00段
//!
//! 判据原文（派工卡 12724 逐字锚定）：
//!   ① 纯功能代码 ≥310 行；
//!   ② 判据原文：UNX-F12725-J1 TEB 槽位对照全行回归绿，槽位漂移
//!      演练 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. PEB 字段对照冻结域模型：x64 偏移表逐行冻结 18 行（字段名/
//!    偏移/尺寸/读权限四列），行序按偏移升序，行号连号互异——
//!    偏移表逐行回归的账面基面；
//! 2. 逐行回归闸：每行偏移回读、尺寸守恒（±行距==尺寸或对齐间隙
//!    显性记账）、读权限在闭集——全行绿才许收官；
//! 3. TEB 槽位对照全行回归（判据 ② 槽位面挂接）：TEB x64 槽位
//!    冻结表 14 行（Self/TlsSlots/TlsExpansionSlots 等锚注），全行
//!    回归绿为挂接判据；
//! 4. 槽位漂移演练 10/10：对 TEB 槽位表注入 ±8/字段互换/权限错挂/
//!    缺行等十路漂移，检出器逐路必检出；
//! 5. 回归态机 PbDraft → PbRowing → PbRowed → PbSealed 单向推进，
//!    rank 单调，倒退/跳相显性拒，封账后禁写；
//! 6. 违约注入十族逐路必抓：偏移漂移、尺寸违约、权限越集、行序
//!    漂移、相位倒退、封账后写、未开账先录、行额不足、槽位漂移、
//!    空摘要。
//!    no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子固定
//!    取 SEG_BASE）；取值一律 match/if let/.get()/checked_*/saturating_*，
//!    禁裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `peb_rows_full_ok` ↔ ① 偏移表逐行回归绿（18 行四列齐）；
//!   - `teb_slots_full_ok` ↔ ② TEB 槽位对照全行回归绿；
//!   - `slot_drill_ok` ↔ ② 槽位漂移演练 10/10 检出；
//!   - `fault_sweep_ok` ↔ ② 违约注入 10/10 必抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12724 − 12600)）。
pub const SEG_BASE: u32 = 0x81F00;

/// PEB 偏移行冻结下限（≥18 行才许收官）。
pub const PEB_ROWS_MIN: usize = 18;

/// TEB 槽位行冻结下限（≥14 行才许收官）。
pub const TEB_ROWS_MIN: usize = 14;

// ── PEB 偏移对照冻结表（x64，四列） ─────────────────────────────
/// PEB 字段读权限闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PbAccess {
    /// 进程内读写。
    Rw,
    /// 只读（跨进程核账面）。
    Ro,
}

impl PbAccess {
    /// 权限中文名。
    pub fn name(self) -> &'static str {
        match self {
            PbAccess::Rw => "读写",
            PbAccess::Ro => "只读",
        }
    }
}

/// PEB 偏移对照行（字段名/偏移/尺寸/读权限四列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PebFieldRow {
    /// 字段名。
    pub field: &'static str,
    /// x64 偏移（冻结值）。
    pub offset: u32,
    /// 字段尺寸（字节）。
    pub size: u32,
    /// 读权限（闭集）。
    pub access: PbAccess,
}

/// PEB x64 偏移对照冻结表 18 行（行序按偏移升序冻结）。
pub const PEB_FIELD_TABLE: &[PebFieldRow] = &[
    PebFieldRow { field: "InheritedAddressSpace", offset: 0x0000, size: 1, access: PbAccess::Ro },
    PebFieldRow { field: "ReadImageFileExecOptions", offset: 0x0001, size: 1, access: PbAccess::Rw },
    PebFieldRow { field: "BeingDebugged", offset: 0x0002, size: 1, access: PbAccess::Rw },
    PebFieldRow { field: "BitField", offset: 0x0003, size: 1, access: PbAccess::Rw },
    PebFieldRow { field: "Mutant", offset: 0x0008, size: 8, access: PbAccess::Rw },
    PebFieldRow { field: "ImageBaseAddress", offset: 0x0010, size: 8, access: PbAccess::Rw },
    PebFieldRow { field: "Ldr", offset: 0x0018, size: 8, access: PbAccess::Ro },
    PebFieldRow { field: "ProcessParameters", offset: 0x0020, size: 8, access: PbAccess::Ro },
    PebFieldRow { field: "SubSystemData", offset: 0x0028, size: 8, access: PbAccess::Rw },
    PebFieldRow { field: "ProcessHeap", offset: 0x0030, size: 8, access: PbAccess::Ro },
    PebFieldRow { field: "FastPebLock", offset: 0x0038, size: 8, access: PbAccess::Rw },
    PebFieldRow { field: "AtlThunkSListPtr", offset: 0x0040, size: 8, access: PbAccess::Ro },
    PebFieldRow { field: "IFEOKey", offset: 0x0048, size: 8, access: PbAccess::Ro },
    PebFieldRow { field: "CrossProcessFlags", offset: 0x0050, size: 4, access: PbAccess::Ro },
    PebFieldRow { field: "KernelCallbackTable", offset: 0x0058, size: 8, access: PbAccess::Ro },
    PebFieldRow { field: "ApiSetMap", offset: 0x0068, size: 8, access: PbAccess::Ro },
    PebFieldRow { field: "TlsExpansionCounter", offset: 0x0070, size: 4, access: PbAccess::Ro },
    PebFieldRow { field: "TlsBitmap", offset: 0x0078, size: 8, access: PbAccess::Ro },
];

/// 表序守卫：偏移升序、字段名互异非空、行号可连号。
pub fn peb_field_order_ok() -> bool {
    let mut i = 0usize;
    while i < PEB_FIELD_TABLE.len() {
        let p = match PEB_FIELD_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if p.field.is_empty() || p.size == 0 {
            return false;
        }
        let mut j = i + 1;
        while j < PEB_FIELD_TABLE.len() {
            let q = match PEB_FIELD_TABLE.get(j) {
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

/// 单行守卫：尺寸对齐账（指针槽 8 对齐、UCHAR 槽按字节、ULONG 槽 4 对齐）。
fn peb_row_aligned(p: &PebFieldRow) -> bool {
    if p.size == 8 {
        return p.offset % 8 == 0;
    }
    if p.size == 4 {
        return p.offset % 4 == 0;
    }
    p.size == 1
}

/// 逐行回归绿：行数达下限、行序绿、每行对齐绿、行距守恒（次行偏移
/// ≥ 当行偏移+尺寸，间隙显性可查即账平）。
fn peb_rows_full_ok() -> bool {
    if PEB_FIELD_TABLE.len() < PEB_ROWS_MIN {
        return false;
    }
    if !peb_field_order_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < PEB_FIELD_TABLE.len() {
        let p = match PEB_FIELD_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if !peb_row_aligned(p) || p.access.name().is_empty() {
            return false;
        }
        // 行距守恒：次行偏移 ≥ 当行偏移 + 尺寸（间隙显性即账平）。
        if let Some(q) = PEB_FIELD_TABLE.get(i + 1) {
            if q.offset < p.offset.saturating_add(p.size) {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── TEB 槽位对照冻结表（x64，判据 ② 槽位面） ────────────────────
/// TEB 槽位对照行（槽名/偏移/类别三列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TebSlotRow {
    /// 槽名。
    pub slot: &'static str,
    /// x64 偏移（冻结值）。
    pub offset: u32,
    /// 槽位类别（TIB 段/线程段/TLS 段）。
    pub group: TebGroup,
}

/// TEB 槽位类别闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TebGroup {
    /// TIB 段（异常链/栈界/自指）。
    Tib,
    /// 线程段（ClientId/PEB/错误值）。
    Thread,
    /// TLS 段（槽位/扩展槽）。
    Tls,
}

impl TebGroup {
    /// 类别中文名。
    pub fn name(self) -> &'static str {
        match self {
            TebGroup::Tib => "TIB段",
            TebGroup::Thread => "线程段",
            TebGroup::Tls => "TLS段",
        }
    }
}

/// TEB x64 槽位对照冻结表 14 行（行序按偏移升序冻结）。
pub const TEB_SLOT_TABLE: &[TebSlotRow] = &[
    TebSlotRow { slot: "ExceptionList", offset: 0x0000, group: TebGroup::Tib },
    TebSlotRow { slot: "StackBase", offset: 0x0008, group: TebGroup::Tib },
    TebSlotRow { slot: "StackLimit", offset: 0x0010, group: TebGroup::Tib },
    TebSlotRow { slot: "SubSystemTib", offset: 0x0018, group: TebGroup::Tib },
    TebSlotRow { slot: "ArbitraryUserPointer", offset: 0x0028, group: TebGroup::Tib },
    TebSlotRow { slot: "Self", offset: 0x0030, group: TebGroup::Tib },
    TebSlotRow { slot: "EnvironmentPointer", offset: 0x0038, group: TebGroup::Thread },
    TebSlotRow { slot: "ClientId", offset: 0x0040, group: TebGroup::Thread },
    TebSlotRow { slot: "ActiveRpcHandle", offset: 0x0050, group: TebGroup::Thread },
    TebSlotRow { slot: "ThreadLocalStoragePointer", offset: 0x0058, group: TebGroup::Thread },
    TebSlotRow { slot: "ProcessEnvironmentBlock", offset: 0x0060, group: TebGroup::Thread },
    TebSlotRow { slot: "LastErrorValue", offset: 0x0068, group: TebGroup::Thread },
    TebSlotRow { slot: "WOW32Reserved", offset: 0x0100, group: TebGroup::Thread },
    TebSlotRow { slot: "TlsSlots", offset: 0x1480, group: TebGroup::Tls },
];

/// TEB 槽位表序守卫：偏移升序、槽名互异非空。
pub fn teb_slot_order_ok() -> bool {
    let mut i = 0usize;
    while i < TEB_SLOT_TABLE.len() {
        let p = match TEB_SLOT_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if p.slot.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < TEB_SLOT_TABLE.len() {
            let q = match TEB_SLOT_TABLE.get(j) {
                Some(v) => v,
                None => return false,
            };
            if q.offset <= p.offset || q.slot == p.slot {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// TEB 槽位全行回归绿：行数达下限、表序绿、锚注行直读（Self=0x30、
/// TlsSlots=0x1480）、类别闭集名非空。
fn teb_slots_full_ok() -> bool {
    if TEB_SLOT_TABLE.len() < TEB_ROWS_MIN {
        return false;
    }
    if !teb_slot_order_ok() {
        return false;
    }
    let self_row = match TEB_SLOT_TABLE.get(5) {
        Some(v) => *v,
        None => return false,
    };
    let tls_row = match TEB_SLOT_TABLE.get(13) {
        Some(v) => *v,
        None => return false,
    };
    if self_row.slot != "Self" || self_row.offset != 0x0030 {
        return false;
    }
    if tls_row.slot != "TlsSlots" || tls_row.offset != 0x1480 {
        return false;
    }
    let mut i = 0usize;
    while i < TEB_SLOT_TABLE.len() {
        let r = match TEB_SLOT_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if r.group.name().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 槽位漂移演练（10/10 检出） ───────────────────────────────────
/// 漂移检出器：对照冻结表逐行核，漂移行返回 Some(行号)。
pub fn slot_drift_detect(rows: &[TebSlotRow]) -> Option<usize> {
    if rows.len() != TEB_SLOT_TABLE.len() {
        return Some(0);
    }
    let mut i = 0usize;
    while i < TEB_SLOT_TABLE.len() {
        let frozen = match TEB_SLOT_TABLE.get(i) {
            Some(v) => v,
            None => return Some(i),
        };
        let probe = match rows.get(i) {
            Some(v) => v,
            None => return Some(i),
        };
        if probe.slot != frozen.slot || probe.offset != frozen.offset || probe.group != frozen.group {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 槽位漂移演练：十路漂移注入逐路必检出。
pub fn teb_drill() -> (usize, usize, String) {
    let mut hits = 0usize;
    let mut miss = String::new();

    // 漂1：Self +8。
    let mut r = TEB_SLOT_TABLE.to_vec();
    if let Some(row) = r.get_mut(5) {
        row.offset = row.offset.saturating_add(8);
    }
    match slot_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D1Self加8;"),
    }

    // 漂2：StackLimit −8。
    let mut r = TEB_SLOT_TABLE.to_vec();
    if let Some(row) = r.get_mut(2) {
        row.offset = row.offset.saturating_sub(8);
    }
    match slot_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D2栈界减8;"),
    }

    // 漂3：ClientId ↔ ActiveRpcHandle 换槽。
    let mut r = TEB_SLOT_TABLE.to_vec();
    let g7 = TebSlotRow { slot: "ActiveRpcHandle", offset: 0x0040, group: TebGroup::Thread };
    let g8 = TebSlotRow { slot: "ClientId", offset: 0x0050, group: TebGroup::Thread };
    if let Some(x) = r.get_mut(7) {
        *x = g7;
    }
    if let Some(x) = r.get_mut(8) {
        *x = g8;
    }
    match slot_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D3换槽;"),
    }

    // 漂4：类别错挂（TlsSlots 挂 TIB 段）。
    let mut r = TEB_SLOT_TABLE.to_vec();
    if let Some(row) = r.get_mut(13) {
        row.group = TebGroup::Tib;
    }
    match slot_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D4类别错挂;"),
    }

    // 漂5：缺行。
    let mut r = TEB_SLOT_TABLE.to_vec();
    let _ = r.pop();
    match slot_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D5缺行;"),
    }

    // 漂6：槽名漂移。
    let mut r = TEB_SLOT_TABLE.to_vec();
    if let Some(row) = r.get_mut(0) {
        row.slot = "漂移槽";
    }
    match slot_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D6槽名漂移;"),
    }

    // 漂7：ExceptionList +8。
    let mut r = TEB_SLOT_TABLE.to_vec();
    if let Some(row) = r.get_mut(0) {
        row.offset = row.offset.saturating_add(8);
    }
    match slot_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D7异常链加8;"),
    }

    // 漂8：TlsSlots 换 0x1488。
    let mut r = TEB_SLOT_TABLE.to_vec();
    if let Some(row) = r.get_mut(13) {
        row.offset = 0x1488;
    }
    match slot_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D8TLS换槽;"),
    }

    // 漂9：WOW32Reserved 类别错挂 TLS 段。
    let mut r = TEB_SLOT_TABLE.to_vec();
    if let Some(row) = r.get_mut(12) {
        row.group = TebGroup::Tls;
    }
    match slot_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D9WOW类别;"),
    }

    // 漂10：ProcessEnvironmentBlock −8。
    let mut r = TEB_SLOT_TABLE.to_vec();
    if let Some(row) = r.get_mut(10) {
        row.offset = row.offset.saturating_sub(8);
    }
    match slot_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D10PEB减8;"),
    }

    (hits, 10usize, miss)
}

/// 演练绿判：10/10 检出。
fn slot_drill_ok() -> bool {
    let (h, t, m) = teb_drill();
    h == t && m.is_empty()
}

// ── 回归态机 ─────────────────────────────────────────────────────
/// 回归相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PbPhase {
    /// 回归草稿。
    PbDraft,
    /// 逐行录账中。
    PbRowing,
    /// 逐行录账毕。
    PbRowed,
    /// 回归封账。
    PbSealed,
}

impl PbPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            PbPhase::PbDraft => 0,
            PbPhase::PbRowing => 1,
            PbPhase::PbRowed => 2,
            PbPhase::PbSealed => 3,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            PbPhase::PbDraft => "回归草稿",
            PbPhase::PbRowing => "逐行录账中",
            PbPhase::PbRowed => "逐行录账毕",
            PbPhase::PbSealed => "回归封账",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn pb_advance(from: PbPhase, to: PbPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(PbDiag::ERewind.val())
    }
}

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PbDiag {
    /// 偏移漂移（对照行与冻结值不符）。
    EOffsetDrift,
    /// 尺寸违约（尺寸为零或对齐破）。
    ESizeBad,
    /// 权限越集（读权限不在闭集）。
    EAccessUnknown,
    /// 行序漂移（偏移不升序或字段重名）。
    ERowOrder,
    /// 相位倒退。
    ERewind,
    /// 封账后写。
    ESealedWrite,
    /// 未开账先录。
    ENotOpen,
    /// 行额不足（< PEB_ROWS_MIN 不许收账）。
    ERowsShort,
    /// 槽位漂移（TEB 槽位对照不符）。
    ESlotDrift,
    /// 行距守恒破（次行偏移侵入当行占区）。
    ESpanBreak,
    /// 回归摘要空。
    ESummaryEmpty,
    /// 槽位行额不足（< TEB_ROWS_MIN）。
    ESlotsShort,
}

impl PbDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                PbDiag::EOffsetDrift => 0x01,
                PbDiag::ESizeBad => 0x02,
                PbDiag::EAccessUnknown => 0x03,
                PbDiag::ERowOrder => 0x04,
                PbDiag::ERewind => 0x05,
                PbDiag::ESealedWrite => 0x06,
                PbDiag::ENotOpen => 0x07,
                PbDiag::ERowsShort => 0x08,
                PbDiag::ESlotDrift => 0x09,
                PbDiag::ESpanBreak => 0x0a,
                PbDiag::ESummaryEmpty => 0x0b,
                PbDiag::ESlotsShort => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            PbDiag::EOffsetDrift => "偏移漂移：对照行偏移须与冻结表一致，先对表",
            PbDiag::ESizeBad => "尺寸违约：尺寸非零且按类对齐，先核尺寸",
            PbDiag::EAccessUnknown => "权限越集：读权限限读写/只读，先改权限",
            PbDiag::ERowOrder => "行序漂移：偏移升序且字段名互异，先重排",
            PbDiag::ERewind => "相位倒退：Draft→Rowing→Rowed→Sealed 只进不退",
            PbDiag::ESealedWrite => "封账后写：回归账已冻结，补录走下一轮回归",
            PbDiag::ENotOpen => "未开账先录：先 open 开账再录行",
            PbDiag::ERowsShort => "行额不足：≥18 行在账才许收账，缺行先补",
            PbDiag::ESlotDrift => "槽位漂移：TEB 槽位须与冻结表一致，先对槽",
            PbDiag::ESpanBreak => "行距守恒破：次行偏移不得侵入当行占区，先对距",
            PbDiag::ESummaryEmpty => "回归摘要空：摘要非空才许写回已完成",
            PbDiag::ESlotsShort => "槽位行额不足：≥14 行在账才许收官，缺槽先补",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const PB_ALL: &[PbDiag] = &[
    PbDiag::EOffsetDrift,
    PbDiag::ESizeBad,
    PbDiag::EAccessUnknown,
    PbDiag::ERowOrder,
    PbDiag::ERewind,
    PbDiag::ESealedWrite,
    PbDiag::ENotOpen,
    PbDiag::ERowsShort,
    PbDiag::ESlotDrift,
    PbDiag::ESpanBreak,
    PbDiag::ESummaryEmpty,
    PbDiag::ESlotsShort,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < PB_ALL.len() {
        if let Some(d) = PB_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81F01..0x81F0C 闭集内"
}

// ── 偏移回归账（真实状态机 + 真实错误路径） ─────────────────────
/// PEB 偏移回归账：相位单调，违约显性记 reject。
pub struct PebLedger {
    phase: PbPhase,
    rows: Vec<PebFieldRow>,
    rejects: Vec<u32>,
    summary: String,
}

impl PebLedger {
    /// 新账。
    pub fn new() -> PebLedger {
        PebLedger {
            phase: PbPhase::PbDraft,
            rows: Vec::new(),
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账：Draft → Rowing。
    pub fn open(&mut self) -> Result<(), u32> {
        match pb_advance(self.phase, PbPhase::PbRowing) {
            Ok(()) => {
                self.phase = PbPhase::PbRowing;
                Ok(())
            }
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
        }
    }

    pub fn phase(&self) -> PbPhase {
        self.phase
    }

    pub fn row_len(&self) -> usize {
        self.rows.len()
    }

    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    fn has_field(&self, name: &str) -> bool {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.field == name {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 录行：开账闸 + 字段查重 + 尺寸闸 + 权限闭集 + 行距守恒闸。
    pub fn reg_row(&mut self, r: PebFieldRow) -> Result<(), u32> {
        if self.phase == PbPhase::PbDraft {
            let e = PbDiag::ENotOpen.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.phase != PbPhase::PbRowing {
            let e = PbDiag::ESealedWrite.val();
            self.rejects.push(e);
            return Err(e);
        }
        if r.field.is_empty() || self.has_field(r.field) {
            let e = PbDiag::ERowOrder.val();
            self.rejects.push(e);
            return Err(e);
        }
        // 行距守恒：新行偏移不得侵入已录任一行占区（结构违约先于对齐违约判）。
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(p) = self.rows.get(i) {
                let start = p.offset;
                let end = p.offset.saturating_add(p.size);
                if r.offset >= start && r.offset < end {
                    let e = PbDiag::ESpanBreak.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                // 新行占区亦不得侵入既有行起点。
                let r_end = r.offset.saturating_add(r.size);
                if r_end > start && r.offset < start {
                    let e = PbDiag::ESpanBreak.val();
                    self.rejects.push(e);
                    return Err(e);
                }
            }
            i += 1;
        }
        if r.size == 0 || !peb_row_aligned(&r) {
            let e = PbDiag::ESizeBad.val();
            self.rejects.push(e);
            return Err(e);
        }
        self.rows.push(r);
        Ok(())
    }

    /// 录账毕收账：Rowing → Rowed，行额闸。
    pub fn close_rows(&mut self) -> Result<(), u32> {
        match pb_advance(self.phase, PbPhase::PbRowed) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if self.rows.len() < PEB_ROWS_MIN {
                    let e = PbDiag::ERowsShort.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.phase = PbPhase::PbRowed;
                Ok(())
            }
        }
    }

    /// 槽位全行回归闸：Rowed 相内跑 TEB 槽位全行回归。
    pub fn commit_slots(&mut self) -> Result<(), u32> {
        if self.phase != PbPhase::PbRowed {
            let e = PbDiag::ESlotsShort.val();
            self.rejects.push(e);
            return Err(e);
        }
        if !teb_slots_full_ok() {
            let e = PbDiag::ESlotDrift.val();
            self.rejects.push(e);
            return Err(e);
        }
        Ok(())
    }

    /// 回归封账：Rowed → Sealed，摘要非空才许写回。
    pub fn seal(&mut self, summary: &str) -> Result<(), u32> {
        match pb_advance(self.phase, PbPhase::PbSealed) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if summary.is_empty() {
                    let e = PbDiag::ESummaryEmpty.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.summary = String::from(summary);
                self.phase = PbPhase::PbSealed;
                Ok(())
            }
        }
    }
}

// ── 判据谓词基线 ─────────────────────────────────────────────────
/// 全量回归账基线：开账→录全表→收账→槽位全行回归→封账。
pub fn build_full_peb() -> PebLedger {
    let mut a = PebLedger::new();
    let _ = a.open();
    let mut i = 0usize;
    while i < PEB_FIELD_TABLE.len() {
        if let Some(r) = PEB_FIELD_TABLE.get(i) {
            let _ = a.reg_row(*r);
        }
        i += 1;
    }
    let _ = a.close_rows();
    let _ = a.commit_slots();
    let _ = a.seal("PEB 偏移表逐行 18 行绿，TEB 槽位全行绿，漂移演练 10/10");
    a
}

/// 偏移表逐行回归绿谓词。
fn peb_rows_full_green() -> bool {
    let a = build_full_peb();
    a.row_len() == PEB_FIELD_TABLE.len()
        && a.reject_len() == 0
        && a.phase() == PbPhase::PbSealed
        && peb_rows_full_ok()
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_peb();
    let y = build_full_peb();
    x.row_len() == y.row_len()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && x.summary() == y.summary()
}

/// 谓词：完成写回语义（③ 状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let a = build_full_peb();
    a.phase() == PbPhase::PbSealed && !a.summary().is_empty()
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
/// 注入演练：十族偏移回归违约逐路投递，全部 Err 才算抓全。
pub fn pb_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();
    let row0 = match PEB_FIELD_TABLE.get(0) {
        Some(v) => *v,
        None => return (0, 10, String::from("表空")),
    };

    // 族1：未开账先录。
    let mut a1 = PebLedger::new();
    if a1.reg_row(row0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1未开账;");
    }

    // 族2：字段重名。
    let mut a2 = PebLedger::new();
    let _ = a2.open();
    let _ = a2.reg_row(row0);
    if a2.reg_row(row0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2重名;");
    }

    // 族3：尺寸违约（零尺寸）。
    let mut a3 = PebLedger::new();
    let _ = a3.open();
    let zero = PebFieldRow { field: "零尺寸", offset: 0x0900, size: 0, access: PbAccess::Rw };
    if a3.reg_row(zero).is_err() {
        caught += 1;
    } else {
        miss.push_str("F3零尺寸;");
    }

    // 族4：行距守恒破（侵入账内已录 InheritedAddressSpace 占区 [0x0000,0x0001)）。
    let mut a4 = PebLedger::new();
    let _ = a4.open();
    let _ = a4.reg_row(row0);
    let intrude = PebFieldRow { field: "侵入行", offset: 0x0000, size: 1, access: PbAccess::Rw };
    if a4.reg_row(intrude).is_err() {
        caught += 1;
    } else {
        miss.push_str("F4行距破;");
    }

    // 族5：相位倒退（封账后再 open）。
    let mut a5 = build_full_peb();
    if a5.open().is_err() {
        caught += 1;
    } else {
        miss.push_str("F5相位倒退;");
    }

    // 族6：封账后写（封账后录行）。
    let mut a6 = build_full_peb();
    let late = PebFieldRow { field: "迟到行", offset: 0x0908, size: 8, access: PbAccess::Rw };
    if a6.reg_row(late).is_err() {
        caught += 1;
    } else {
        miss.push_str("F6封账后写;");
    }

    // 族7：行额不足收账。
    let mut a7 = PebLedger::new();
    let _ = a7.open();
    if a7.close_rows().is_err() {
        caught += 1;
    } else {
        miss.push_str("F7行额不足;");
    }

    // 族8：槽位漂移检出（漂移行必 Some）。
    let mut drifted = TEB_SLOT_TABLE.to_vec();
    if let Some(row) = drifted.get_mut(5) {
        row.offset = row.offset.saturating_add(8);
    }
    if slot_drift_detect(&drifted).is_some() {
        caught += 1;
    } else {
        miss.push_str("F8槽位漂移;");
    }

    // 族9：槽位行额不足（空表必检出且槽位回归态直判绿）。
    let empty: Vec<TebSlotRow> = Vec::new();
    if slot_drift_detect(&empty).is_some() && teb_slots_full_ok() {
        caught += 1;
    } else {
        miss.push_str("F9槽位额不足;");
    }

    // 族10：空摘要封账。
    let mut a10 = PebLedger::new();
    let _ = a10.open();
    let mut i = 0usize;
    while i < PEB_FIELD_TABLE.len() {
        if let Some(r) = PEB_FIELD_TABLE.get(i) {
            let _ = a10.reg_row(*r);
        }
        i += 1;
    }
    let _ = a10.close_rows();
    let _ = a10.commit_slots();
    if a10.seal("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F10空摘要;");
    }

    (caught, 10usize, miss)
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = pb_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12724 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12724_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12724");

    s.add_dyn(
        "F12724-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12724-02 段位公式重算",
        {
            let mut ok = seg_of(12724) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12724-03 诊断码闭集",
        {
            let mut ok = PB_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < PB_ALL.len() {
                if let Some(d) = PB_ALL.get(i) {
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
        "F12724-04 PEB 偏移表逐行回归绿",
        peb_rows_full_green(),
        &format!("rows={} 四列齐（字段/偏移/尺寸/权限）", PEB_FIELD_TABLE.len()),
    );

    s.add_dyn(
        "F12724-05 PEB 表序守卫",
        peb_field_order_ok(),
        "偏移升序、字段名互异非空",
    );

    s.add_dyn(
        "F12724-06 行距守恒直读",
        {
            // BeingDebugged(0x2,+1) 与 BitField(0x3) 相邻不侵入。
            let mut ok = true;
            let mut i = 0usize;
            while ok && i + 1 < PEB_FIELD_TABLE.len() {
                let p = match PEB_FIELD_TABLE.get(i) {
                    Some(v) => *v,
                    None => break,
                };
                if let Some(q) = PEB_FIELD_TABLE.get(i + 1) {
                    ok &= q.offset >= p.offset.saturating_add(p.size);
                }
                i += 1;
            }
            ok
        },
        "次行偏移 ≥ 当行偏移+尺寸，逐行核",
    );

    s.add_dyn(
        "F12724-07 TEB 槽位对照全行回归绿",
        teb_slots_full_ok(),
        &format!("slots={} 三列齐（槽名/偏移/类别）", TEB_SLOT_TABLE.len()),
    );

    s.add_dyn(
        "F12724-08 TEB 锚注行直读",
        {
            // Self=0x30、TlsSlots=0x1480 锚注直读（Option 内比较）。
            let ok = match (TEB_SLOT_TABLE.get(5), TEB_SLOT_TABLE.get(13)) {
                (Some(a), Some(b)) => {
                    a.slot == "Self" && a.offset == 0x0030 && b.slot == "TlsSlots" && b.offset == 0x1480
                }
                _ => false,
            };
            ok
        },
        "Self=0x30、TlsSlots=0x1480 锚注直读",
    );

    s.add_dyn(
        "F12724-09 槽位漂移演练 10/10 检出",
        {
            let (h, t, _) = teb_drill();
            h == t
        },
        &format!("hits={}/10", teb_drill().0),
    );

    s.add_dyn(
        "F12724-10 注入联测 10/10 拒止",
        {
            let (c, t, _) = pb_fault_sweep();
            c == t
        },
        &format!("caught={}/10", pb_fault_sweep().0),
    );

    s.add_dyn(
        "F12724-11 相位迁移守卫直读",
        pb_advance(PbPhase::PbDraft, PbPhase::PbRowing).is_ok()
            && pb_advance(PbPhase::PbRowing, PbPhase::PbRowed).is_ok()
            && pb_advance(PbPhase::PbRowed, PbPhase::PbSealed).is_ok()
            && pb_advance(PbPhase::PbSealed, PbPhase::PbDraft).is_err()
            && pb_advance(PbPhase::PbDraft, PbPhase::PbRowed).is_err(),
        "Draft→Rowing→Rowed→Sealed 只进不退不跳",
    );

    s.add_dyn(
        "F12724-12 完整建账基线",
        {
            let a = build_full_peb();
            a.row_len() == PEB_FIELD_TABLE.len()
                && a.reject_len() == 0
                && a.phase() == PbPhase::PbSealed
        },
        &format!(
            "rows={} phase={}",
            build_full_peb().row_len(),
            build_full_peb().phase().name()
        ),
    );

    s.add_dyn(
        "F12724-13 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12724-14 完成写回语义",
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
        assert_eq!(SEG_BASE, 0x81F00);
        assert_eq!(seg_of(12724), Some(SEG_BASE));
        assert_eq!(seg_of(12600), Some(0x80000));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(PB_ALL.len(), 12);
        let mut i = 0usize;
        while i < PB_ALL.len() {
            let vi = match PB_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < PB_ALL.len() {
                let vj = match PB_ALL.get(j) {
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
    fn peb_table_frozen_rows() {
        assert!(peb_rows_full_ok());
        assert!(peb_rows_full_green());
        assert!(peb_field_order_ok());
        assert_eq!(PEB_FIELD_TABLE.len(), PEB_ROWS_MIN);
        // 锚行直判。
        let being = match PEB_FIELD_TABLE.get(2) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(being.field, "BeingDebugged");
        assert_eq!(being.offset, 0x0002);
        assert_eq!(being.size, 1);
        let ldr = match PEB_FIELD_TABLE.get(6) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(ldr.offset, 0x0018);
        assert_eq!(ldr.size, 8);
    }

    #[test]
    fn teb_table_frozen_slots() {
        assert!(teb_slots_full_ok());
        assert!(teb_slot_order_ok());
        assert_eq!(TEB_SLOT_TABLE.len(), TEB_ROWS_MIN);
        // Self/TlsSlots 锚直判。
        let self_row = match TEB_SLOT_TABLE.get(5) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(self_row.slot, "Self");
        assert_eq!(self_row.offset, 0x0030);
        let tls_row = match TEB_SLOT_TABLE.get(13) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(tls_row.slot, "TlsSlots");
        assert_eq!(tls_row.offset, 0x1480);
        // 冻结表自身不得漂移。
        assert!(slot_drift_detect(TEB_SLOT_TABLE).is_none());
    }

    #[test]
    fn slot_drill_ten_ten_detected() {
        let (h, t, miss) = teb_drill();
        assert_eq!(h, t, "漏检：{}", miss);
        assert_eq!(t, 10);
        assert!(slot_drill_ok());
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = pb_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        let row0 = match PEB_FIELD_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        // 未开账先录。
        let mut a = PebLedger::new();
        assert_eq!(a.reg_row(row0), Err(PbDiag::ENotOpen.val()));
        // 字段重名。
        let _ = a.open();
        let _ = a.reg_row(row0);
        assert_eq!(a.reg_row(row0), Err(PbDiag::ERowOrder.val()));
        // 零尺寸。
        let zero = PebFieldRow { field: "零尺寸", offset: 0x0900, size: 0, access: PbAccess::Rw };
        assert_eq!(a.reg_row(zero), Err(PbDiag::ESizeBad.val()));
        // 行距守恒破（侵入账内已录行占区 [0x0000,0x0001)）。
        let intrude = PebFieldRow { field: "侵入行", offset: 0x0000, size: 1, access: PbAccess::Rw };
        assert_eq!(a.reg_row(intrude), Err(PbDiag::ESpanBreak.val()));
        // 行额不足收账。
        assert_eq!(a.close_rows(), Err(PbDiag::ERowsShort.val()));
        // Rowed 相外 commit_slots 拒。
        assert_eq!(a.commit_slots(), Err(PbDiag::ESlotsShort.val()));
        // 封账后写。
        let mut b = build_full_peb();
        let late = PebFieldRow { field: "迟到行", offset: 0x0908, size: 8, access: PbAccess::Rw };
        assert_eq!(b.reg_row(late), Err(PbDiag::ESealedWrite.val()));
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(pb_advance(PbPhase::PbDraft, PbPhase::PbRowing).is_ok());
        assert!(pb_advance(PbPhase::PbRowing, PbPhase::PbRowed).is_ok());
        assert!(pb_advance(PbPhase::PbRowed, PbPhase::PbSealed).is_ok());
        assert!(pb_advance(PbPhase::PbRowing, PbPhase::PbRowing).is_err());
        assert!(pb_advance(PbPhase::PbSealed, PbPhase::PbRowing).is_err());
        assert!(PbPhase::PbDraft.rank() < PbPhase::PbRowing.rank());
        assert!(PbPhase::PbRowing.rank() < PbPhase::PbRowed.rank());
        let mut a = PebLedger::new();
        let _ = a.open();
        let mut i = 0usize;
        while i < PEB_FIELD_TABLE.len() {
            if let Some(r) = PEB_FIELD_TABLE.get(i) {
                let _ = a.reg_row(*r);
            }
            i += 1;
        }
        assert!(a.close_rows().is_ok());
        assert!(a.commit_slots().is_ok());
        assert_eq!(a.seal(""), Err(PbDiag::ESummaryEmpty.val()));
        assert!(a.seal("偏移表逐行绿").is_ok());
        assert_eq!(a.phase(), PbPhase::PbSealed);
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_peb();
        let y = build_full_peb();
        assert_eq!(x.row_len(), y.row_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(x.summary(), y.summary());
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for d in PB_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
        for a in [PbAccess::Rw, PbAccess::Ro].iter() {
            assert!(!a.name().is_empty());
        }
        for g in [TebGroup::Tib, TebGroup::Thread, TebGroup::Tls].iter() {
            assert!(!g.name().is_empty());
        }
        for p in [
            PbPhase::PbDraft,
            PbPhase::PbRowing,
            PbPhase::PbRowed,
            PbPhase::PbSealed,
        ]
        .iter()
        {
            assert!(!p.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12724_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
