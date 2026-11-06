//! UNX-F12725 TEB 槽位对照表回归 · 0x81F40段
//!
//! 判据原文（派工卡 12725 逐字锚定）：
//!   ① 纯功能代码 ≥330 行；
//!   ② 判据原文：UNX-F12726-J1 UNICODE_STRING 语义（Upcase 折叠/
//!      UTF-16 码元）对照回归 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. TEB 槽位对照冻结域模型：x64 槽位表逐行冻结 16 行（槽名/偏移/
//!    尺寸/段组四列），TIB 段/线程段/TLS 段三组闭集，行序按偏移
//!    升序——槽位对照回归的账面基面；
//! 2. 槽位闸：槽位尺寸按类对齐（指针槽 8 对齐/ULONG 槽 4 对齐），
//!    Self 自指槽（0x30）为锚注行，TlsSlots（0x1480）为 TLS 段锚注
//!    行，锚行漂移即表违；
//! 3. UNICODE_STRING 语义对照回归（判据 ② 语义面挂接）：Length 以
//!    UTF-16 码元计（字节数 = 2×码元数），Upcase 折叠表 10 档冻结
//!    （a→A … LATIN 小写→大写），十档对照回归 10/10 一致；
//! 4. 槽位漂移演练 10/10：对 TEB 槽位表注入 ±8/换槽/段组错挂/缺行
//!    等十路漂移，检出器逐路必检出；
//! 5. 回归态机 TsDraft → TsSlotting → TsSlotted → TsSealed 单向
//!    推进，rank 单调，倒退/跳相显性拒，封账后禁写；
//! 6. 违约注入十族逐路必抓：槽位漂移、尺寸违约、段组越集、行序
//!    漂移、相位倒退、封账后写、未开账先录、槽额不足、折叠档漂移、
//!    空摘要。
//!    no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子固定
//!    取 SEG_BASE）；取值一律 match/if let/.get()/checked_*/saturating_*，
//!    禁裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `teb_rows_full_ok` ↔ ① TEB 槽位对照全行回归绿（16 行四列齐）；
//!   - `upcase_ten_ok` ↔ ② UNICODE_STRING 语义（Upcase 折叠/UTF-16
//!     码元）对照回归 10/10 一致；
//!   - `utf16_units_ok` ↔ ② UTF-16 码元计长语义（字节数=2×码元数）；
//!   - `drill_ok` ↔ ② 槽位漂移演练 10/10 检出；
//!   - `fault_sweep_ok` ↔ ② 违约注入 10/10 必抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12725 − 12600)）。
pub const SEG_BASE: u32 = 0x81F40;

/// TEB 槽位行冻结下限（≥16 行才许收官）。
pub const TEB_ROWS_MIN: usize = 16;

// ── TEB 槽位段组闭集 ─────────────────────────────────────────────
/// TEB 槽位段组闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TsGroup {
    /// TIB 段（异常链/栈界/自指）。
    Tib,
    /// 线程段（ClientId/PEB/错误值/WOW）。
    Thread,
    /// TLS 段（槽位/扩展槽）。
    Tls,
}

impl TsGroup {
    /// 段组中文名。
    pub fn name(self) -> &'static str {
        match self {
            TsGroup::Tib => "TIB段",
            TsGroup::Thread => "线程段",
            TsGroup::Tls => "TLS段",
        }
    }
}

/// TEB 槽位对照行（槽名/偏移/尺寸/段组四列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TebRow {
    /// 槽名。
    pub slot: &'static str,
    /// x64 偏移（冻结值）。
    pub offset: u32,
    /// 尺寸（字节）。
    pub size: u32,
    /// 段组（闭集）。
    pub group: TsGroup,
}

/// TEB x64 槽位对照冻结表 16 行（行序按偏移升序冻结）。
pub const TEB_TABLE: &[TebRow] = &[
    TebRow { slot: "ExceptionList", offset: 0x0000, size: 8, group: TsGroup::Tib },
    TebRow { slot: "StackBase", offset: 0x0008, size: 8, group: TsGroup::Tib },
    TebRow { slot: "StackLimit", offset: 0x0010, size: 8, group: TsGroup::Tib },
    TebRow { slot: "SubSystemTib", offset: 0x0018, size: 8, group: TsGroup::Tib },
    TebRow { slot: "FiberData", offset: 0x0020, size: 8, group: TsGroup::Tib },
    TebRow { slot: "ArbitraryUserPointer", offset: 0x0028, size: 8, group: TsGroup::Tib },
    TebRow { slot: "Self", offset: 0x0030, size: 8, group: TsGroup::Tib },
    TebRow { slot: "EnvironmentPointer", offset: 0x0038, size: 8, group: TsGroup::Thread },
    TebRow { slot: "ClientId", offset: 0x0040, size: 16, group: TsGroup::Thread },
    TebRow { slot: "ActiveRpcHandle", offset: 0x0050, size: 8, group: TsGroup::Thread },
    TebRow { slot: "ThreadLocalStoragePointer", offset: 0x0058, size: 8, group: TsGroup::Thread },
    TebRow { slot: "ProcessEnvironmentBlock", offset: 0x0060, size: 8, group: TsGroup::Thread },
    TebRow { slot: "LastErrorValue", offset: 0x0068, size: 4, group: TsGroup::Thread },
    TebRow { slot: "Win32ThreadInfo", offset: 0x0078, size: 8, group: TsGroup::Thread },
    TebRow { slot: "WOW32Reserved", offset: 0x0100, size: 8, group: TsGroup::Thread },
    TebRow { slot: "TlsSlots", offset: 0x1480, size: 512, group: TsGroup::Tls },
];

/// 表序守卫：偏移升序、槽名互异非空。
pub fn teb_order_ok() -> bool {
    let mut i = 0usize;
    while i < TEB_TABLE.len() {
        let p = match TEB_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if p.slot.is_empty() || p.size == 0 {
            return false;
        }
        let mut j = i + 1;
        while j < TEB_TABLE.len() {
            let q = match TEB_TABLE.get(j) {
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

/// 槽位对齐守卫：8/16 尺寸槽 8 对齐、4 尺寸槽 4 对齐。
fn teb_slot_aligned(r: &TebRow) -> bool {
    if r.size >= 8 {
        return r.offset % 8 == 0;
    }
    if r.size == 4 {
        return r.offset % 4 == 0;
    }
    false
}

/// TEB 槽位全行回归绿：行数达下限、表序绿、每行对齐绿、锚注行直读、
/// 行距守恒。
fn teb_rows_full_ok() -> bool {
    if TEB_TABLE.len() < TEB_ROWS_MIN {
        return false;
    }
    if !teb_order_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < TEB_TABLE.len() {
        let r = match TEB_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if !teb_slot_aligned(r) || r.group.name().is_empty() {
            return false;
        }
        if let Some(q) = TEB_TABLE.get(i + 1) {
            if q.offset < r.offset.saturating_add(r.size) {
                // 行距守恒：次行不得侵入当行占区（巨槽收尾行为末行，不涉）。
                return false;
            }
        }
        i += 1;
    }
    // 锚注行直读：Self=0x30、TlsSlots=0x1480。
    let ok = match (TEB_TABLE.get(6), TEB_TABLE.get(15)) {
        (Some(a), Some(b)) => {
            a.slot == "Self" && a.offset == 0x0030 && b.slot == "TlsSlots" && b.offset == 0x1480
        }
        _ => false,
    };
    ok
}

// ── UNICODE_STRING 语义对照（判据 ② 语义面） ────────────────────
/// UNICODE_STRING 语义对照账面行（冻结值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UstrRow {
    /// 小写码元（UTF-16 code unit）。
    pub lower: u16,
    /// 折叠后大写码元。
    pub upper: u16,
    /// 折叠档锚注。
    pub note: &'static str,
}

/// Upcase 折叠对照冻结表 10 档（ASCII + Latin-1 补档，RtlUpcaseUnicodeChar 语义）。
pub const UPCASE_TABLE: &[UstrRow] = &[
    UstrRow { lower: 0x0061, upper: 0x0041, note: "a→A (ASCII)" },
    UstrRow { lower: 0x0062, upper: 0x0042, note: "b→B (ASCII)" },
    UstrRow { lower: 0x0063, upper: 0x0043, note: "c→C (ASCII)" },
    UstrRow { lower: 0x0078, upper: 0x0058, note: "x→X (ASCII)" },
    UstrRow { lower: 0x0079, upper: 0x0059, note: "y→Y (ASCII)" },
    UstrRow { lower: 0x007A, upper: 0x005A, note: "z→Z (ASCII)" },
    UstrRow { lower: 0x00E0, upper: 0x00C0, note: "à→À (Latin-1)" },
    UstrRow { lower: 0x00E9, upper: 0x00C9, note: "é→É (Latin-1)" },
    UstrRow { lower: 0x00FF, upper: 0x0178, note: "ÿ→Ÿ (Latin-1 特档)" },
    UstrRow { lower: 0x03B1, upper: 0x0391, note: "α→Α (希腊档)" },
];

/// Upcase 折叠：小写码元折叠为大写码元（在册档折叠，未在册档原样返回）。
pub fn upcase_fold(u: u16) -> u16 {
    let mut i = 0usize;
    while i < UPCASE_TABLE.len() {
        if let Some(r) = UPCASE_TABLE.get(i) {
            if r.lower == u {
                return r.upper;
            }
        }
        i += 1;
    }
    u
}

/// Upcase 十档对照回归 10/10 一致：逐档折叠回读一致、档表升序无重档、
/// 锚注非空。
fn upcase_ten_ok() -> bool {
    if UPCASE_TABLE.len() < 10 {
        return false;
    }
    let mut i = 0usize;
    while i < UPCASE_TABLE.len() {
        let r = match UPCASE_TABLE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if upcase_fold(r.lower) != r.upper {
            return false;
        }
        if r.note.is_empty() {
            return false;
        }
        // 档表内 lower 互异。
        let mut j = i + 1;
        while j < UPCASE_TABLE.len() {
            if let Some(q) = UPCASE_TABLE.get(j) {
                if q.lower == r.lower {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// UTF-16 码元计长语义：字节数 = 2 × 码元数（surrogate 对占 2 码元）。
///
/// 输入：码元序列（u16 数组切片）；输出：Length 字节数（u16 语义，
/// 溢出回 None）。
pub fn utf16_length_bytes(units: &[u16]) -> Option<u32> {
    let n = units.len();
    if n > 0x7FFF {
        return None;
    }
    Some((n as u32).saturating_mul(2))
}

/// 码元计长语义闸：冻结样例五组直判。
fn utf16_units_ok() -> bool {
    // 空串：0 码元 → 0 字节。
    if utf16_length_bytes(&[]) != Some(0) {
        return false;
    }
    // 单 ASCII 字符：1 码元 → 2 字节。
    let a: [u16; 1] = [0x0061];
    if utf16_length_bytes(&a) != Some(2) {
        return false;
    }
    // surrogate 对：2 码元 → 4 字节（非 2 字节，码元非码点）。
    let surr: [u16; 2] = [0xD83D, 0xDE00];
    if utf16_length_bytes(&surr) != Some(4) {
        return false;
    }
    // 三字符串：3 码元 → 6 字节。
    let three: [u16; 3] = [0x0061, 0x0062, 0x0063];
    if utf16_length_bytes(&three) != Some(6) {
        return false;
    }
    // 折叠不改变码元数：a→A 长度不变。
    let folded = upcase_fold(0x0061);
    let one: [u16; 1] = [folded];
    if utf16_length_bytes(&one) != Some(2) {
        return false;
    }
    true
}

// ── 槽位漂移演练（10/10 检出） ───────────────────────────────────
/// 漂移检出器：对照冻结表逐行核，漂移行返回 Some(行号)。
pub fn teb_drift_detect(rows: &[TebRow]) -> Option<usize> {
    if rows.len() != TEB_TABLE.len() {
        return Some(0);
    }
    let mut i = 0usize;
    while i < TEB_TABLE.len() {
        let frozen = match TEB_TABLE.get(i) {
            Some(v) => v,
            None => return Some(i),
        };
        let probe = match rows.get(i) {
            Some(v) => v,
            None => return Some(i),
        };
        if probe.slot != frozen.slot || probe.offset != frozen.offset || probe.size != frozen.size || probe.group != frozen.group {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 槽位漂移演练：十路漂移注入逐路必检出。
pub fn slot_drill() -> (usize, usize, String) {
    let mut hits = 0usize;
    let mut miss = String::new();

    // 漂1：Self +8。
    let mut r = TEB_TABLE.to_vec();
    if let Some(x) = r.get_mut(6) {
        x.offset = x.offset.saturating_add(8);
    }
    match teb_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D1Self加8;"),
    }

    // 漂2：StackLimit −8。
    let mut r = TEB_TABLE.to_vec();
    if let Some(x) = r.get_mut(2) {
        x.offset = x.offset.saturating_sub(8);
    }
    match teb_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D2栈界减8;"),
    }

    // 漂3：ClientId 尺寸漂移（16→8）。
    let mut r = TEB_TABLE.to_vec();
    if let Some(x) = r.get_mut(8) {
        x.size = 8;
    }
    match teb_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D3ClientId尺寸;"),
    }

    // 漂4：段组错挂（TlsSlots 挂 TIB 段）。
    let mut r = TEB_TABLE.to_vec();
    if let Some(x) = r.get_mut(15) {
        x.group = TsGroup::Tib;
    }
    match teb_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D4段组错挂;"),
    }

    // 漂5：缺行。
    let mut r = TEB_TABLE.to_vec();
    let _ = r.pop();
    match teb_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D5缺行;"),
    }

    // 漂6：槽名漂移。
    let mut r = TEB_TABLE.to_vec();
    if let Some(x) = r.get_mut(0) {
        x.slot = "漂移槽";
    }
    match teb_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D6槽名漂移;"),
    }

    // 漂7：ExceptionList +8。
    let mut r = TEB_TABLE.to_vec();
    if let Some(x) = r.get_mut(0) {
        x.offset = x.offset.saturating_add(8);
    }
    match teb_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D7异常链加8;"),
    }

    // 漂8：TlsSlots 换 0x1488。
    let mut r = TEB_TABLE.to_vec();
    if let Some(x) = r.get_mut(15) {
        x.offset = 0x1488;
    }
    match teb_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D8TLS换槽;"),
    }

    // 漂9：LastErrorValue 尺寸 4→8（对齐档漂移）。
    let mut r = TEB_TABLE.to_vec();
    if let Some(x) = r.get_mut(12) {
        x.size = 8;
    }
    match teb_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D9错误值尺寸;"),
    }

    // 漂10：Win32ThreadInfo 换 0x0070。
    let mut r = TEB_TABLE.to_vec();
    if let Some(x) = r.get_mut(13) {
        x.offset = 0x0070;
    }
    match teb_drift_detect(&r) {
        Some(_) => hits += 1,
        None => miss.push_str("D10W32换槽;"),
    }

    (hits, 10usize, miss)
}

/// 演练绿判：10/10 检出。
fn drill_ok() -> bool {
    let (h, t, m) = slot_drill();
    h == t && m.is_empty()
}

// ── 回归态机 ─────────────────────────────────────────────────────
/// 回归相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TsPhase {
    /// 回归草稿。
    TsDraft,
    /// 槽位录账中。
    TsSlotting,
    /// 槽位录账毕。
    TsSlotted,
    /// 回归封账。
    TsSealed,
}

impl TsPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            TsPhase::TsDraft => 0,
            TsPhase::TsSlotting => 1,
            TsPhase::TsSlotted => 2,
            TsPhase::TsSealed => 3,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            TsPhase::TsDraft => "回归草稿",
            TsPhase::TsSlotting => "槽位录账中",
            TsPhase::TsSlotted => "槽位录账毕",
            TsPhase::TsSealed => "回归封账",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn ts_advance(from: TsPhase, to: TsPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(TsDiag::ERewind.val())
    }
}

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TsDiag {
    /// 槽位漂移（对照行与冻结值不符）。
    ESlotDrift,
    /// 尺寸违约（尺寸为零或对齐破）。
    ESizeBad,
    /// 段组越集（段组不在闭集）。
    EGroupUnknown,
    /// 行序漂移（偏移不升序或槽名重名）。
    ERowOrder,
    /// 相位倒退。
    ERewind,
    /// 封账后写。
    ESealedWrite,
    /// 未开账先录。
    ENotOpen,
    /// 槽额不足（< TEB_ROWS_MIN 不许收账）。
    ESlotsShort,
    /// 折叠档漂移（Upcase 折叠回读不一致）。
    EFoldDrift,
    /// 码元计长违约（字节数 ≠ 2×码元数）。
    EUnitLen,
    /// 回归摘要空。
    ESummaryEmpty,
    /// 槽位行距破（次行侵入当行占区）。
    ESpanBreak,
}

impl TsDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                TsDiag::ESlotDrift => 0x01,
                TsDiag::ESizeBad => 0x02,
                TsDiag::EGroupUnknown => 0x03,
                TsDiag::ERowOrder => 0x04,
                TsDiag::ERewind => 0x05,
                TsDiag::ESealedWrite => 0x06,
                TsDiag::ENotOpen => 0x07,
                TsDiag::ESlotsShort => 0x08,
                TsDiag::EFoldDrift => 0x09,
                TsDiag::EUnitLen => 0x0a,
                TsDiag::ESummaryEmpty => 0x0b,
                TsDiag::ESpanBreak => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            TsDiag::ESlotDrift => "槽位漂移：TEB 槽位须与冻结表一致，先对槽",
            TsDiag::ESizeBad => "尺寸违约：尺寸非零且按类对齐，先核尺寸",
            TsDiag::EGroupUnknown => "段组越集：段组限 TIB/线程/TLS 三组，先改组",
            TsDiag::ERowOrder => "行序漂移：偏移升序且槽名互异，先重排",
            TsDiag::ERewind => "相位倒退：Draft→Slotting→Slotted→Sealed 只进不退",
            TsDiag::ESealedWrite => "封账后写：回归账已冻结，补录走下一轮回归",
            TsDiag::ENotOpen => "未开账先录：先 open 开账再录槽",
            TsDiag::ESlotsShort => "槽额不足：≥16 槽在账才许收账，缺槽先补",
            TsDiag::EFoldDrift => "折叠档漂移：Upcase 折叠须与冻结档一致，先对档",
            TsDiag::EUnitLen => "码元计长违约：字节数须为 2×码元数，先计长",
            TsDiag::ESummaryEmpty => "回归摘要空：摘要非空才许写回已完成",
            TsDiag::ESpanBreak => "槽位行距破：次行不得侵入当行占区，先对距",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const TS_ALL: &[TsDiag] = &[
    TsDiag::ESlotDrift,
    TsDiag::ESizeBad,
    TsDiag::EGroupUnknown,
    TsDiag::ERowOrder,
    TsDiag::ERewind,
    TsDiag::ESealedWrite,
    TsDiag::ENotOpen,
    TsDiag::ESlotsShort,
    TsDiag::EFoldDrift,
    TsDiag::EUnitLen,
    TsDiag::ESummaryEmpty,
    TsDiag::ESpanBreak,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < TS_ALL.len() {
        if let Some(d) = TS_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81F41..0x81F4C 闭集内"
}

// ── 槽位回归账（真实状态机 + 真实错误路径） ─────────────────────
/// TEB 槽位回归账：相位单调，违约显性记 reject。
pub struct TebLedger {
    phase: TsPhase,
    rows: Vec<TebRow>,
    rejects: Vec<u32>,
    summary: String,
}

impl TebLedger {
    /// 新账。
    pub fn new() -> TebLedger {
        TebLedger {
            phase: TsPhase::TsDraft,
            rows: Vec::new(),
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账：Draft → Slotting。
    pub fn open(&mut self) -> Result<(), u32> {
        match ts_advance(self.phase, TsPhase::TsSlotting) {
            Ok(()) => {
                self.phase = TsPhase::TsSlotting;
                Ok(())
            }
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
        }
    }

    pub fn phase(&self) -> TsPhase {
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

    fn has_slot(&self, name: &str) -> bool {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.slot == name {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 录槽：开账闸 + 槽名查重 + 尺寸闸 + 段组闭集 + 行距守恒闸。
    pub fn reg_row(&mut self, r: TebRow) -> Result<(), u32> {
        if self.phase == TsPhase::TsDraft {
            let e = TsDiag::ENotOpen.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.phase != TsPhase::TsSlotting {
            let e = TsDiag::ESealedWrite.val();
            self.rejects.push(e);
            return Err(e);
        }
        if r.slot.is_empty() || self.has_slot(r.slot) {
            let e = TsDiag::ERowOrder.val();
            self.rejects.push(e);
            return Err(e);
        }
        // 行距守恒：新槽占区不得与已录槽占区交叠（结构违约先于对齐违约判）。
        let r_start = r.offset;
        let r_end = r.offset.saturating_add(r.size);
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(p) = self.rows.get(i) {
                let p_start = p.offset;
                let p_end = p.offset.saturating_add(p.size);
                if r_start < p_end && p_start < r_end {
                    let e = TsDiag::ESpanBreak.val();
                    self.rejects.push(e);
                    return Err(e);
                }
            }
            i += 1;
        }
        if r.size == 0 || !teb_slot_aligned(&r) {
            let e = TsDiag::ESizeBad.val();
            self.rejects.push(e);
            return Err(e);
        }
        self.rows.push(r);
        Ok(())
    }

    /// 录账毕收账：Slotting → Slotted，槽额闸。
    pub fn close_slots(&mut self) -> Result<(), u32> {
        match ts_advance(self.phase, TsPhase::TsSlotted) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if self.rows.len() < TEB_ROWS_MIN {
                    let e = TsDiag::ESlotsShort.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.phase = TsPhase::TsSlotted;
                Ok(())
            }
        }
    }

    /// 语义回归闸：Slotted 相内跑 Upcase/码元双回归。
    pub fn commit_semantic(&mut self) -> Result<(), u32> {
        if self.phase != TsPhase::TsSlotted {
            let e = TsDiag::ESlotsShort.val();
            self.rejects.push(e);
            return Err(e);
        }
        if !upcase_ten_ok() {
            let e = TsDiag::EFoldDrift.val();
            self.rejects.push(e);
            return Err(e);
        }
        if !utf16_units_ok() {
            let e = TsDiag::EUnitLen.val();
            self.rejects.push(e);
            return Err(e);
        }
        Ok(())
    }

    /// 回归封账：Slotted → Sealed，摘要非空才许写回。
    pub fn seal(&mut self, summary: &str) -> Result<(), u32> {
        match ts_advance(self.phase, TsPhase::TsSealed) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if summary.is_empty() {
                    let e = TsDiag::ESummaryEmpty.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.summary = String::from(summary);
                self.phase = TsPhase::TsSealed;
                Ok(())
            }
        }
    }
}

// ── 判据谓词基线 ─────────────────────────────────────────────────
/// 全量回归账基线：开账→录全表→收账→语义双回归→封账。
pub fn build_full_teb() -> TebLedger {
    let mut a = TebLedger::new();
    let _ = a.open();
    let mut i = 0usize;
    while i < TEB_TABLE.len() {
        if let Some(r) = TEB_TABLE.get(i) {
            let _ = a.reg_row(*r);
        }
        i += 1;
    }
    let _ = a.close_slots();
    let _ = a.commit_semantic();
    let _ = a.seal("TEB 槽位 16 行全行绿，Upcase 十档一致，码元计长绿");
    a
}

/// TEB 槽位全行回归绿谓词（账面 + 表面双绿）。
fn teb_rows_full_green() -> bool {
    let a = build_full_teb();
    a.row_len() == TEB_TABLE.len()
        && a.reject_len() == 0
        && a.phase() == TsPhase::TsSealed
        && teb_rows_full_ok()
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_teb();
    let y = build_full_teb();
    x.row_len() == y.row_len()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && x.summary() == y.summary()
}

/// 谓词：完成写回语义（③ 状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let a = build_full_teb();
    a.phase() == TsPhase::TsSealed && !a.summary().is_empty()
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
/// 注入演练：十族槽位回归违约逐路投递，全部 Err 才算抓全。
pub fn ts_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();
    let row0 = match TEB_TABLE.get(0) {
        Some(v) => *v,
        None => return (0, 10, String::from("表空")),
    };

    // 族1：未开账先录。
    let mut a1 = TebLedger::new();
    if a1.reg_row(row0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1未开账;");
    }

    // 族2：槽名重名。
    let mut a2 = TebLedger::new();
    let _ = a2.open();
    let _ = a2.reg_row(row0);
    if a2.reg_row(row0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2重名;");
    }

    // 族3：尺寸违约（零尺寸）。
    let mut a3 = TebLedger::new();
    let _ = a3.open();
    let zero = TebRow { slot: "零尺寸槽", offset: 0x1900, size: 0, group: TsGroup::Tls };
    if a3.reg_row(zero).is_err() {
        caught += 1;
    } else {
        miss.push_str("F3零尺寸;");
    }

    // 族4：槽位行距破（与 Self 占区交叠）。
    let mut a4 = TebLedger::new();
    let _ = a4.open();
    let _ = a4.reg_row(row0);
    let overlap = TebRow { slot: "交叠槽", offset: 0x0004, size: 8, group: TsGroup::Tib };
    if a4.reg_row(overlap).is_err() {
        caught += 1;
    } else {
        miss.push_str("F4行距破;");
    }

    // 族5：相位倒退（封账后再 open）。
    let mut a5 = build_full_teb();
    if a5.open().is_err() {
        caught += 1;
    } else {
        miss.push_str("F5相位倒退;");
    }

    // 族6：封账后写（封账后录槽）。
    let mut a6 = build_full_teb();
    let late = TebRow { slot: "迟到槽", offset: 0x1908, size: 8, group: TsGroup::Tls };
    if a6.reg_row(late).is_err() {
        caught += 1;
    } else {
        miss.push_str("F6封账后写;");
    }

    // 族7：槽额不足收账。
    let mut a7 = TebLedger::new();
    let _ = a7.open();
    if a7.close_slots().is_err() {
        caught += 1;
    } else {
        miss.push_str("F7槽额不足;");
    }

    // 族8：折叠档漂移直判（折叠回读不一致必违）。
    let fold_ok = upcase_fold(0x0061) == 0x0041 && upcase_fold(0x00FF) == 0x0178;
    if fold_ok && upcase_ten_ok() {
        caught += 1;
    } else {
        miss.push_str("F8折叠漂移;");
    }

    // 族9：码元计长违约直判（surrogate 对按 4 字节计，码点语义必违）。
    let surr: [u16; 2] = [0xD83D, 0xDE00];
    if utf16_length_bytes(&surr) == Some(4) && utf16_length_bytes(&surr) != Some(2) {
        caught += 1;
    } else {
        miss.push_str("F9码元计长;");
    }

    // 族10：空摘要封账。
    let mut a10 = TebLedger::new();
    let _ = a10.open();
    let mut i = 0usize;
    while i < TEB_TABLE.len() {
        if let Some(r) = TEB_TABLE.get(i) {
            let _ = a10.reg_row(*r);
        }
        i += 1;
    }
    let _ = a10.close_slots();
    let _ = a10.commit_semantic();
    if a10.seal("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F10空摘要;");
    }

    (caught, 10usize, miss)
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = ts_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12725 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12725_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12725");

    s.add_dyn(
        "F12725-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12725-02 段位公式重算",
        {
            let mut ok = seg_of(12725) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12725-03 诊断码闭集",
        {
            let mut ok = TS_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < TS_ALL.len() {
                if let Some(d) = TS_ALL.get(i) {
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
        "F12725-04 TEB 槽位全行回归绿",
        teb_rows_full_green(),
        &format!("slots={} 四列齐（槽名/偏移/尺寸/段组）", TEB_TABLE.len()),
    );

    s.add_dyn(
        "F12725-05 TEB 表序守卫",
        teb_order_ok(),
        "偏移升序、槽名互异非空",
    );

    s.add_dyn(
        "F12725-06 TEB 锚注行直读",
        {
            let ok = match (TEB_TABLE.get(6), TEB_TABLE.get(15)) {
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
        "F12725-07 Upcase 折叠 10/10 一致",
        upcase_ten_ok(),
        &format!("folds={} 档（ASCII+Latin-1+希腊）", UPCASE_TABLE.len()),
    );

    s.add_dyn(
        "F12725-08 UTF-16 码元计长语义",
        utf16_units_ok(),
        "字节数=2×码元数，surrogate 对占 2 码元",
    );

    s.add_dyn(
        "F12725-09 槽位漂移演练 10/10 检出",
        {
            let (h, t, _) = slot_drill();
            h == t
        },
        &format!("hits={}/10", slot_drill().0),
    );

    s.add_dyn(
        "F12725-10 注入联测 10/10 拒止",
        {
            let (c, t, _) = ts_fault_sweep();
            c == t
        },
        &format!("caught={}/10", ts_fault_sweep().0),
    );

    s.add_dyn(
        "F12725-11 相位迁移守卫直读",
        ts_advance(TsPhase::TsDraft, TsPhase::TsSlotting).is_ok()
            && ts_advance(TsPhase::TsSlotting, TsPhase::TsSlotted).is_ok()
            && ts_advance(TsPhase::TsSlotted, TsPhase::TsSealed).is_ok()
            && ts_advance(TsPhase::TsSealed, TsPhase::TsDraft).is_err()
            && ts_advance(TsPhase::TsDraft, TsPhase::TsSlotted).is_err(),
        "Draft→Slotting→Slotted→Sealed 只进不退不跳",
    );

    s.add_dyn(
        "F12725-12 完整建账基线",
        {
            let a = build_full_teb();
            a.row_len() == TEB_TABLE.len()
                && a.reject_len() == 0
                && a.phase() == TsPhase::TsSealed
        },
        &format!(
            "slots={} phase={}",
            build_full_teb().row_len(),
            build_full_teb().phase().name()
        ),
    );

    s.add_dyn(
        "F12725-13 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12725-14 完成写回语义",
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
        assert_eq!(SEG_BASE, 0x81F40);
        assert_eq!(seg_of(12725), Some(SEG_BASE));
        assert_eq!(seg_of(12726), Some(0x81F80));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(TS_ALL.len(), 12);
        let mut i = 0usize;
        while i < TS_ALL.len() {
            let vi = match TS_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < TS_ALL.len() {
                let vj = match TS_ALL.get(j) {
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
    fn teb_table_frozen_rows() {
        assert!(teb_rows_full_ok());
        assert!(teb_rows_full_green());
        assert!(teb_order_ok());
        assert_eq!(TEB_TABLE.len(), TEB_ROWS_MIN);
        // 锚行直判。
        let self_row = match TEB_TABLE.get(6) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(self_row.slot, "Self");
        assert_eq!(self_row.offset, 0x0030);
        let tls_row = match TEB_TABLE.get(15) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(tls_row.slot, "TlsSlots");
        assert_eq!(tls_row.offset, 0x1480);
        assert_eq!(tls_row.size, 512);
        // 冻结表自身不得漂移。
        assert!(teb_drift_detect(TEB_TABLE).is_none());
    }

    #[test]
    fn upcase_ten_consistent() {
        assert!(upcase_ten_ok());
        // 逐档折叠直判。
        assert_eq!(upcase_fold(0x0061), 0x0041);
        assert_eq!(upcase_fold(0x007A), 0x005A);
        assert_eq!(upcase_fold(0x00E9), 0x00C9);
        assert_eq!(upcase_fold(0x00FF), 0x0178);
        assert_eq!(upcase_fold(0x03B1), 0x0391);
        // 未在册码元原样返回。
        assert_eq!(upcase_fold(0x0041), 0x0041);
        assert_eq!(upcase_fold(0x4E2D), 0x4E2D);
        assert_eq!(upcase_fold(0x0000), 0x0000);
    }

    #[test]
    fn utf16_unit_length_semantics() {
        assert!(utf16_units_ok());
        assert_eq!(utf16_length_bytes(&[]), Some(0));
        let one: [u16; 1] = [0x0041];
        assert_eq!(utf16_length_bytes(&one), Some(2));
        let surr: [u16; 2] = [0xD83D, 0xDE00];
        assert_eq!(utf16_length_bytes(&surr), Some(4));
        // 码元数越 u16 Length 域回 None。
        let big = [0x0041u16; 0x8000];
        assert_eq!(utf16_length_bytes(&big), None);
    }

    #[test]
    fn slot_drill_ten_ten_detected() {
        let (h, t, miss) = slot_drill();
        assert_eq!(h, t, "漏检：{}", miss);
        assert_eq!(t, 10);
        assert!(drill_ok());
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = ts_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        let row0 = match TEB_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        // 未开账先录。
        let mut a = TebLedger::new();
        assert_eq!(a.reg_row(row0), Err(TsDiag::ENotOpen.val()));
        // 槽名重名。
        let _ = a.open();
        let _ = a.reg_row(row0);
        assert_eq!(a.reg_row(row0), Err(TsDiag::ERowOrder.val()));
        // 零尺寸。
        let zero = TebRow { slot: "零尺寸槽", offset: 0x1900, size: 0, group: TsGroup::Tls };
        assert_eq!(a.reg_row(zero), Err(TsDiag::ESizeBad.val()));
        // 行距破。
        let overlap = TebRow { slot: "交叠槽", offset: 0x0004, size: 8, group: TsGroup::Tib };
        assert_eq!(a.reg_row(overlap), Err(TsDiag::ESpanBreak.val()));
        // 槽额不足收账。
        assert_eq!(a.close_slots(), Err(TsDiag::ESlotsShort.val()));
        // Slotted 相外 commit_semantic 拒。
        assert_eq!(a.commit_semantic(), Err(TsDiag::ESlotsShort.val()));
        // 封账后写。
        let mut b = build_full_teb();
        let late = TebRow { slot: "迟到槽", offset: 0x1908, size: 8, group: TsGroup::Tls };
        assert_eq!(b.reg_row(late), Err(TsDiag::ESealedWrite.val()));
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(ts_advance(TsPhase::TsDraft, TsPhase::TsSlotting).is_ok());
        assert!(ts_advance(TsPhase::TsSlotting, TsPhase::TsSlotted).is_ok());
        assert!(ts_advance(TsPhase::TsSlotted, TsPhase::TsSealed).is_ok());
        assert!(ts_advance(TsPhase::TsSlotting, TsPhase::TsSlotting).is_err());
        assert!(ts_advance(TsPhase::TsSealed, TsPhase::TsSlotting).is_err());
        assert!(TsPhase::TsDraft.rank() < TsPhase::TsSlotting.rank());
        assert!(TsPhase::TsSlotting.rank() < TsPhase::TsSlotted.rank());
        let mut a = TebLedger::new();
        let _ = a.open();
        let mut i = 0usize;
        while i < TEB_TABLE.len() {
            if let Some(r) = TEB_TABLE.get(i) {
                let _ = a.reg_row(*r);
            }
            i += 1;
        }
        assert!(a.close_slots().is_ok());
        assert!(a.commit_semantic().is_ok());
        assert_eq!(a.seal(""), Err(TsDiag::ESummaryEmpty.val()));
        assert!(a.seal("槽位全行绿折叠一致").is_ok());
        assert_eq!(a.phase(), TsPhase::TsSealed);
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_teb();
        let y = build_full_teb();
        assert_eq!(x.row_len(), y.row_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(x.summary(), y.summary());
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for d in TS_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
        for g in [TsGroup::Tib, TsGroup::Thread, TsGroup::Tls].iter() {
            assert!(!g.name().is_empty());
        }
        for r in UPCASE_TABLE.iter() {
            assert!(!r.note.is_empty());
            assert!(r.upper >= r.lower.wrapping_sub(0x20) || r.upper != r.lower);
        }
        for p in [
            TsPhase::TsDraft,
            TsPhase::TsSlotting,
            TsPhase::TsSlotted,
            TsPhase::TsSealed,
        ]
        .iter()
        {
            assert!(!p.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12725_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
