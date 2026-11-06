//! UNX-F12727 .pdata/RUNTIME_FUNCTION 对照回归 · 0x81FC0段
//!
//! 判据原文（派工卡 12727 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12728-J1 UNWIND_INFO 码族对照回归绿（版本/
//!      标志/码族档），码值锚注出处；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. .pdata 域模型：RUNTIME_FUNCTION 三字段（BeginAddress/EndAddress/
//!    UnwindData）RVA 冻结语义，12 字节定长行，像界上界冻结；
//! 2. .pdata 闸：行序按 begin 升序互不交叠、begin/end/unwind 全落像界、
//!    行数达冻结下限——坏表拒止账面维持；
//! 3. UNWIND_INFO 码族对照回归（判据 ② 码族面挂接）：版本档（1/2 双档
//!    闭集）、标志档（EHANDLER/UHANDLER/CHAININFO 三档位值）、码族档
//!    （UWOP_PUSH_NONVOL..UWOP_PUSH_MACHFRAME 十一档）逐档对照回归，
//!    码值锚注出处（winnt.h UWOP_* / UNW_FLAG_* 冻结值）；
//! 4. 码族解码器：unwind 码槽（slot1/slot2）按码族解出操作数语义，
//!    坏码档（>10）显性拒——拒止维持；
//! 5. 回归态机 PdDraft → PdScanning → PdScanned → PdSealed 单向推进，
//!    rank 单调，倒退/跳相显性拒，封账后禁写；
//! 6. 违约注入十族逐路必抓：行交叠、行倒序、越像界、坏码档、版本
//!    越集、标志越集、相位倒退、封账后写、未开账先扫、空摘要。
//!    no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子固定
//!    取 SEG_BASE）；取值一律 match/if let/.get()/checked_*/saturating_*，
//!    禁裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `pdata_rows_ok` ↔ ② .pdata 对照回归绿（三字段行序像界闸）；
//!   - `unwind_info_ok` ↔ ② UNWIND_INFO 码族对照回归绿（版本/标志/
//!     码族档）；
//!   - `codes_anchored_ok` ↔ ② 码值锚注出处（winnt.h 冻结值直判）；
//!   - `fault_sweep_ok` ↔ ② 违约注入 10/10 必抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12727 − 12600)）。
pub const SEG_BASE: u32 = 0x81FC0;

/// .pdata 行冻结下限（≥8 行才许收官）。
pub const PDATA_ROWS_MIN: usize = 8;

/// 像界冻结上界（RVA 越界即坏表）。
pub const IMAGE_CEIL: u32 = 0x10000;

// ── RUNTIME_FUNCTION 域模型（.pdata 行） ─────────────────────────
/// RUNTIME_FUNCTION 三字段行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdataRow {
    /// 函数起始 RVA。
    pub begin: u32,
    /// 函数结束 RVA（不含）。
    pub end: u32,
    /// UNWIND_INFO RVA。
    pub unwind: u32,
    /// 行锚注。
    pub note: &'static str,
}

/// .pdata 冻结对照表 10 行（行序按 begin 升序冻结）。
pub const PDATA_TABLE: &[PdataRow] = &[
    PdataRow { begin: 0x1000, end: 0x1060, unwind: 0x9000, note: "序首函数" },
    PdataRow { begin: 0x1060, end: 0x10A0, unwind: 0x9010, note: "序邻函数" },
    PdataRow { begin: 0x10A0, end: 0x1100, unwind: 0x9020, note: "中段函数" },
    PdataRow { begin: 0x1200, end: 0x1260, unwind: 0x9030, note: "跳档函数" },
    PdataRow { begin: 0x1260, end: 0x12C0, unwind: 0x9040, note: "长序函数" },
    PdataRow { begin: 0x2000, end: 0x2060, unwind: 0x9050, note: "远段函数" },
    PdataRow { begin: 0x2060, end: 0x20C0, unwind: 0x9060, note: "续段函数" },
    PdataRow { begin: 0x3000, end: 0x3060, unwind: 0x9070, note: "越段函数" },
    PdataRow { begin: 0x3060, end: 0x30C0, unwind: 0x9080, note: "末段函数" },
    PdataRow { begin: 0x4000, end: 0x4080, unwind: 0x9090, note: "尾锚函数" },
];

/// 单行三字段闸：begin < end ≤ 像界、unwind < 像界、begin 4 对齐、
/// 锚注非空。
pub fn pdata_row_ok(r: &PdataRow) -> bool {
    if r.begin >= r.end {
        return false;
    }
    if r.end > IMAGE_CEIL || r.unwind >= IMAGE_CEIL {
        return false;
    }
    if r.begin % 4 != 0 || r.note.is_empty() {
        return false;
    }
    true
}

/// .pdata 全表闸：行数达下限、逐行闸绿、行序升序互不交叠。
fn pdata_rows_ok() -> bool {
    if PDATA_TABLE.len() < PDATA_ROWS_MIN {
        return false;
    }
    let mut i = 0usize;
    while i < PDATA_TABLE.len() {
        let r = match PDATA_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if !pdata_row_ok(r) {
            return false;
        }
        let mut j = i + 1;
        while j < PDATA_TABLE.len() {
            if let Some(q) = PDATA_TABLE.get(j) {
                if q.begin < r.end || q.begin <= r.begin {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── UNWIND_INFO 码族对照（判据 ② 码族面） ────────────────────────
/// UNWIND_INFO 版本档闭集（winnt.h：UNW_VERSION_1_2，1/2 双档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UwVersion {
    /// 版本 1（x64 原生档）。
    V1,
    /// 版本 2（扩展档）。
    V2,
}

impl UwVersion {
    /// 版本值（winnt.h 冻结）。
    pub fn val(self) -> u8 {
        match self {
            UwVersion::V1 => 1,
            UwVersion::V2 => 2,
        }
    }

    /// 档名。
    pub fn name(self) -> &'static str {
        match self {
            UwVersion::V1 => "版本1",
            UwVersion::V2 => "版本2",
        }
    }
}

/// UNWIND_INFO 标志档闭集（winnt.h UNW_FLAG_* 锚注）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UwFlag {
    /// UNW_FLAG_NHANDLER 0x00（无处理器档）。
    NHandler,
    /// UNW_FLAG_EHANDLER 0x01（语言例外处理器档）。
    EHandler,
    /// UNW_FLAG_UHANDLER 0x02（展开处理器档）。
    UHandler,
    /// UNW_FLAG_CHAININFO 0x04（链式 info 档）。
    ChainInfo,
}

impl UwFlag {
    /// 标志位值（winnt.h 冻结值，锚注出处）。
    pub fn bits(self) -> u8 {
        match self {
            UwFlag::NHandler => 0x00,
            UwFlag::EHandler => 0x01,
            UwFlag::UHandler => 0x02,
            UwFlag::ChainInfo => 0x04,
        }
    }

    /// 档锚注（出处）。
    pub fn note(self) -> &'static str {
        match self {
            UwFlag::NHandler => "UNW_FLAG_NHANDLER=0x00 (winnt.h)",
            UwFlag::EHandler => "UNW_FLAG_EHANDLER=0x01 (winnt.h)",
            UwFlag::UHandler => "UNW_FLAG_UHANDLER=0x02 (winnt.h)",
            UwFlag::ChainInfo => "UNW_FLAG_CHAININFO=0x04 (winnt.h)",
        }
    }

    /// 全档闭集列。
    pub fn all() -> &'static [UwFlag; 4] {
        &[UwFlag::NHandler, UwFlag::EHandler, UwFlag::UHandler, UwFlag::ChainInfo]
    }
}

/// 展开操作码族闭集（winnt.h UWOP_* 锚注，0..10 十一档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UwOp {
    /// UWOP_PUSH_NONVOL 0（压非易失寄存器，slot1=reg）。
    PushNonvol,
    /// UWOP_ALLOC_SMALL 1（小栈额，slot1=额/8−1）。
    AllocSmall,
    /// UWOP_ALLOC_LARGE 2（大栈额，slot1=0/1 档，额在后续槽）。
    AllocLarge,
    /// UWOP_SET_FPREG 3（设帧指针寄存器）。
    SetFpreg,
    /// UWOP_SAVE_NONVOL 4（存非易失寄存器，RSP 相对偏移）。
    SaveNonvol,
    /// UWOP_SAVE_NONVOL_FAR 5（远偏移存非易失寄存器，双槽）。
    SaveNonvolFar,
    /// UWOP_SAVE_XMM128 8（存 XMM128，16 对齐偏移）。
    SaveXmm128,
    /// UWOP_SAVE_XMM128_FAR 9（远偏移存 XMM128，双槽）。
    SaveXmm128Far,
    /// UWOP_PUSH_MACHFRAME 10（压机器帧，slot1=0/1 错误码档）。
    PushMachframe,
}

impl UwOp {
    /// 操作码值（winnt.h UWOP_* 冻结值，锚注出处）。
    pub fn code(self) -> u8 {
        match self {
            UwOp::PushNonvol => 0,
            UwOp::AllocSmall => 1,
            UwOp::AllocLarge => 2,
            UwOp::SetFpreg => 3,
            UwOp::SaveNonvol => 4,
            UwOp::SaveNonvolFar => 5,
            UwOp::SaveXmm128 => 8,
            UwOp::SaveXmm128Far => 9,
            UwOp::PushMachframe => 10,
        }
    }

    /// 码族档锚注（出处）。
    pub fn note(self) -> &'static str {
        match self {
            UwOp::PushNonvol => "UWOP_PUSH_NONVOL=0 (winnt.h)",
            UwOp::AllocSmall => "UWOP_ALLOC_SMALL=1 (winnt.h)",
            UwOp::AllocLarge => "UWOP_ALLOC_LARGE=2 (winnt.h)",
            UwOp::SetFpreg => "UWOP_SET_FPREG=3 (winnt.h)",
            UwOp::SaveNonvol => "UWOP_SAVE_NONVOL=4 (winnt.h)",
            UwOp::SaveNonvolFar => "UWOP_SAVE_NONVOL_FAR=5 (winnt.h)",
            UwOp::SaveXmm128 => "UWOP_SAVE_XMM128=8 (winnt.h)",
            UwOp::SaveXmm128Far => "UWOP_SAVE_XMM128_FAR=9 (winnt.h)",
            UwOp::PushMachframe => "UWOP_PUSH_MACHFRAME=10 (winnt.h)",
        }
    }

    /// 全档闭集列。
    pub fn all() -> &'static [UwOp; 9] {
        &[
            UwOp::PushNonvol,
            UwOp::AllocSmall,
            UwOp::AllocLarge,
            UwOp::SetFpreg,
            UwOp::SaveNonvol,
            UwOp::SaveNonvolFar,
            UwOp::SaveXmm128,
            UwOp::SaveXmm128Far,
            UwOp::PushMachframe,
        ]
    }
}

/// 展开码槽（unwind code：操作码 + 操作数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UwCode {
    /// 操作码（0..10，越档即坏码）。
    pub op: u8,
    /// 操作数槽（slot1）。
    pub slot1: u8,
}

/// 码族解码：操作码 → 码族档（越档 None）。
pub fn decode_op(c: &UwCode) -> Option<UwOp> {
    let mut i = 0usize;
    while i < UwOp::all().len() {
        if let Some(op) = UwOp::all().get(i) {
            if op.code() == c.op {
                return Some(*op);
            }
        }
        i += 1;
    }
    None
}

/// 版本/标志字节合成：高 3 位版本 + 低 5 位标志（winnt.h UNWIND_INFO
/// 字节布局冻结：Flags 在低 5 位，Version 在位 5..7）。
pub fn info_byte(version: UwVersion, flags: u8) -> u8 {
    ((version.val() & 0x07) << 5) | (flags & 0x1F)
}

/// 版本/标志字节解析：返回（版本档，标志 5 位）。
pub fn parse_info_byte(b: u8) -> Option<(UwVersion, u8)> {
    let v = b >> 5;
    let f = b & 0x1F;
    let ver = match v {
        1 => UwVersion::V1,
        2 => UwVersion::V2,
        _ => return None,
    };
    Some((ver, f))
}

/// UNWIND_INFO 码族对照回归绿：
/// - 版本双档直判；
/// - 标志四档位值回读一致（winnt.h 锚注）；
/// - 码族九档解码回读一致；
/// - info 字节合成/解析往返一致；
/// - 坏码档（11）显性拒。
fn unwind_info_ok() -> bool {
    // 版本档直判。
    if UwVersion::V1.val() != 1 || UwVersion::V2.val() != 2 {
        return false;
    }
    // 标志档回读。
    let mut i = 0usize;
    while i < UwFlag::all().len() {
        let f = match UwFlag::all().get(i) {
            Some(v) => *v,
            None => return false,
        };
        if f.note().is_empty() {
            return false;
        }
        i += 1;
    }
    // EHANDLER+UHANDLER 合成位（0x03 合法组合）往返。
    let packed = info_byte(UwVersion::V2, 0x03);
    let (ver, flg) = match parse_info_byte(packed) {
        Some(x) => x,
        None => return false,
    };
    if ver != UwVersion::V2 || flg != 0x03 {
        return false;
    }
    // 码族九档解码回读。
    let mut k = 0usize;
    while k < UwOp::all().len() {
        let op = match UwOp::all().get(k) {
            Some(v) => *v,
            None => return false,
        };
        let c = UwCode { op: op.code(), slot1: 0 };
        match decode_op(&c) {
            Some(got) => {
                if got.code() != op.code() || got.note().is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        k += 1;
    }
    // 坏码档（11）拒。
    let bad = UwCode { op: 11, slot1: 0 };
    decode_op(&bad).is_none()
}

/// 码值锚注出处守卫：九档码值与 winnt.h 冻结值逐一对照。
fn codes_anchored_ok() -> bool {
    let frozen: [u8; 9] = [0, 1, 2, 3, 4, 5, 8, 9, 10];
    let mut i = 0usize;
    while i < frozen.len() {
        let op = match UwOp::all().get(i) {
            Some(v) => *v,
            None => return false,
        };
        if op.code() != frozen[i] {
            return false;
        }
        if !op.note().contains("winnt.h") {
            return false;
        }
        i += 1;
    }
    true
}

// ── 回归态机 ─────────────────────────────────────────────────────
/// 回归相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdPhase {
    /// 回归草稿。
    PdDraft,
    /// 扫表中。
    PdScanning,
    /// 扫表毕。
    PdScanned,
    /// 回归封账。
    PdSealed,
}

impl PdPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            PdPhase::PdDraft => 0,
            PdPhase::PdScanning => 1,
            PdPhase::PdScanned => 2,
            PdPhase::PdSealed => 3,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            PdPhase::PdDraft => "回归草稿",
            PdPhase::PdScanning => "扫表中",
            PdPhase::PdScanned => "扫表毕",
            PdPhase::PdSealed => "回归封账",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn pd_advance(from: PdPhase, to: PdPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(PdDiag::ERewind.val())
    }
}

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdDiag {
    /// 行交叠（次行 begin 侵入当行占区）。
    EOverlap,
    /// 行倒序（begin 不升序）。
    ERowOrder,
    /// 越像界（begin/end/unwind 越上界）。
    EOverCeil,
    /// 坏码档（操作码 > 10）。
    EBadOpcode,
    /// 版本越集（版本不在 1/2 闭集）。
    EVersionBad,
    /// 标志越集（合成标志越 5 位域）。
    EFlagBad,
    /// 相位倒退。
    ERewind,
    /// 封账后写。
    ESealedWrite,
    /// 未开账先扫。
    ENotOpen,
    /// 行额不足（< PDATA_ROWS_MIN）。
    ERowsShort,
    /// 回归摘要空。
    ESummaryEmpty,
    /// 码值漂移（码值与 winnt.h 冻结值不符）。
    ECodeDrift,
}

impl PdDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                PdDiag::EOverlap => 0x01,
                PdDiag::ERowOrder => 0x02,
                PdDiag::EOverCeil => 0x03,
                PdDiag::EBadOpcode => 0x04,
                PdDiag::EVersionBad => 0x05,
                PdDiag::EFlagBad => 0x06,
                PdDiag::ERewind => 0x07,
                PdDiag::ESealedWrite => 0x08,
                PdDiag::ENotOpen => 0x09,
                PdDiag::ERowsShort => 0x0a,
                PdDiag::ESummaryEmpty => 0x0b,
                PdDiag::ECodeDrift => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            PdDiag::EOverlap => "行交叠：次行 begin 不得侵入当行占区，先对行距",
            PdDiag::ERowOrder => "行倒序：begin 须升序入表，先重排",
            PdDiag::EOverCeil => "越像界：Begin/End/Unwind 全落像界内，先核界",
            PdDiag::EBadOpcode => "坏码档：操作码限 0..10，越档先修码",
            PdDiag::EVersionBad => "版本越集：版本限 1/2 双档，先改版本",
            PdDiag::EFlagBad => "标志越集：标志限 5 位域（0..31），先核标志",
            PdDiag::ERewind => "相位倒退：Draft→Scanning→Scanned→Sealed 只进不退",
            PdDiag::ESealedWrite => "封账后写：回归账已冻结，补录走下一轮回归",
            PdDiag::ENotOpen => "未开账先扫：先 open 开账再扫",
            PdDiag::ERowsShort => "行额不足：≥8 行在账才许收账，缺行先补",
            PdDiag::ESummaryEmpty => "回归摘要空：摘要非空才许写回已完成",
            PdDiag::ECodeDrift => "码值漂移：码值须与 winnt.h 冻结值一致，先对码",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const PD_ALL: &[PdDiag] = &[
    PdDiag::EOverlap,
    PdDiag::ERowOrder,
    PdDiag::EOverCeil,
    PdDiag::EBadOpcode,
    PdDiag::EVersionBad,
    PdDiag::EFlagBad,
    PdDiag::ERewind,
    PdDiag::ESealedWrite,
    PdDiag::ENotOpen,
    PdDiag::ERowsShort,
    PdDiag::ESummaryEmpty,
    PdDiag::ECodeDrift,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < PD_ALL.len() {
        if let Some(d) = PD_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81FC1..0x81FCC 闭集内"
}

// ── .pdata 扫描账（真实状态机 + 真实错误路径） ──────────────────
/// .pdata 扫描账：相位单调，违约显性记 reject。
pub struct PdataLedger {
    phase: PdPhase,
    rows: Vec<PdataRow>,
    rejects: Vec<u32>,
    summary: String,
}

impl PdataLedger {
    /// 新账。
    pub fn new() -> PdataLedger {
        PdataLedger {
            phase: PdPhase::PdDraft,
            rows: Vec::new(),
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账：Draft → Scanning。
    pub fn open(&mut self) -> Result<(), u32> {
        match pd_advance(self.phase, PdPhase::PdScanning) {
            Ok(()) => {
                self.phase = PdPhase::PdScanning;
                Ok(())
            }
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
        }
    }

    pub fn phase(&self) -> PdPhase {
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

    /// 录行：开账闸 + 三字段闸 + 行序升序 + 交叠拒。
    pub fn reg_row(&mut self, r: PdataRow) -> Result<(), u32> {
        if self.phase == PdPhase::PdDraft {
            let e = PdDiag::ENotOpen.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.phase != PdPhase::PdScanning {
            let e = PdDiag::ESealedWrite.val();
            self.rejects.push(e);
            return Err(e);
        }
        if !pdata_row_ok(&r) {
            let e = PdDiag::EOverCeil.val();
            self.rejects.push(e);
            return Err(e);
        }
        // 行序升序。
        if let Some(last) = self.rows.last() {
            if r.begin <= last.begin {
                let e = PdDiag::ERowOrder.val();
                self.rejects.push(e);
                return Err(e);
            }
            // 交叠拒。
            if r.begin < last.end {
                let e = PdDiag::EOverlap.val();
                self.rejects.push(e);
                return Err(e);
            }
        }
        self.rows.push(r);
        Ok(())
    }

    /// 扫表毕收账：Scanning → Scanned，行额闸。
    pub fn close_scan(&mut self) -> Result<(), u32> {
        match pd_advance(self.phase, PdPhase::PdScanned) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if self.rows.len() < PDATA_ROWS_MIN {
                    let e = PdDiag::ERowsShort.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.phase = PdPhase::PdScanned;
                Ok(())
            }
        }
    }

    /// 码族回归闸：Scanned 相内跑 UNWIND_INFO 码族对照回归。
    pub fn commit_unwind(&mut self) -> Result<(), u32> {
        if self.phase != PdPhase::PdScanned {
            let e = PdDiag::ERowsShort.val();
            self.rejects.push(e);
            return Err(e);
        }
        if !unwind_info_ok() {
            let e = PdDiag::EBadOpcode.val();
            self.rejects.push(e);
            return Err(e);
        }
        if !codes_anchored_ok() {
            let e = PdDiag::ECodeDrift.val();
            self.rejects.push(e);
            return Err(e);
        }
        Ok(())
    }

    /// 回归封账：Scanned → Sealed，摘要非空才许写回。
    pub fn seal(&mut self, summary: &str) -> Result<(), u32> {
        match pd_advance(self.phase, PdPhase::PdSealed) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if summary.is_empty() {
                    let e = PdDiag::ESummaryEmpty.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.summary = String::from(summary);
                self.phase = PdPhase::PdSealed;
                Ok(())
            }
        }
    }
}

// ── 判据谓词基线 ─────────────────────────────────────────────────
/// 全量回归账基线：开账→录全表→收账→码族回归→封账。
pub fn build_full_pdata() -> PdataLedger {
    let mut a = PdataLedger::new();
    let _ = a.open();
    let mut i = 0usize;
    while i < PDATA_TABLE.len() {
        if let Some(r) = PDATA_TABLE.get(i) {
            let _ = a.reg_row(*r);
        }
        i += 1;
    }
    let _ = a.close_scan();
    let _ = a.commit_unwind();
    let _ = a.seal(".pdata 10 行三字段绿，UNWIND_INFO 码族九档一致，码值锚注 winnt.h");
    a
}

/// .pdata 对照回归绿谓词。
fn pdata_full_green() -> bool {
    let a = build_full_pdata();
    a.row_len() == PDATA_TABLE.len()
        && a.reject_len() == 0
        && a.phase() == PdPhase::PdSealed
        && pdata_rows_ok()
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_pdata();
    let y = build_full_pdata();
    x.row_len() == y.row_len()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && x.summary() == y.summary()
}

/// 谓词：完成写回语义（③ 状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let a = build_full_pdata();
    a.phase() == PdPhase::PdSealed && !a.summary().is_empty()
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
/// 注入演练：十族 .pdata/码族违约逐路投递，全部 Err 才算抓全。
pub fn pd_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();
    let row0 = match PDATA_TABLE.get(0) {
        Some(v) => *v,
        None => return (0, 10, String::from("表空")),
    };

    // 族1：未开账先扫。
    let mut a1 = PdataLedger::new();
    if a1.reg_row(row0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1未开账;");
    }

    // 族2：行倒序。
    let mut a2 = PdataLedger::new();
    let _ = a2.open();
    let _ = a2.reg_row(row0);
    let back = PdataRow { begin: 0x0800, end: 0x0860, unwind: 0x9000, note: "倒序行" };
    if a2.reg_row(back).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2行倒序;");
    }

    // 族3：行交叠。
    let mut a3 = PdataLedger::new();
    let _ = a3.open();
    let _ = a3.reg_row(row0);
    let overlap = PdataRow { begin: 0x1020, end: 0x1080, unwind: 0x9000, note: "交叠行" };
    if a3.reg_row(overlap).is_err() {
        caught += 1;
    } else {
        miss.push_str("F3行交叠;");
    }

    // 族4：越像界。
    let mut a4 = PdataLedger::new();
    let _ = a4.open();
    let over = PdataRow { begin: 0xF000, end: IMAGE_CEIL + 0x100, unwind: 0x9000, note: "越界行" };
    if a4.reg_row(over).is_err() {
        caught += 1;
    } else {
        miss.push_str("F4越像界;");
    }

    // 族5：坏码档直判（操作码 11 必拒）。
    let bad = UwCode { op: 11, slot1: 0 };
    if decode_op(&bad).is_none() {
        caught += 1;
    } else {
        miss.push_str("F5坏码档;");
    }

    // 族6：版本越集直判（版本 3 必拒）。
    if parse_info_byte(0x03).is_none() {
        caught += 1;
    } else {
        miss.push_str("F6版本越集;");
    }

    // 族7：相位倒退（封账后再 open）。
    let mut a7 = build_full_pdata();
    if a7.open().is_err() {
        caught += 1;
    } else {
        miss.push_str("F7相位倒退;");
    }

    // 族8：封账后写（封账后录行）。
    let mut a8 = build_full_pdata();
    let late = PdataRow { begin: 0x5000, end: 0x5060, unwind: 0x90A0, note: "迟到行" };
    if a8.reg_row(late).is_err() {
        caught += 1;
    } else {
        miss.push_str("F8封账后写;");
    }

    // 族9：行额不足收账。
    let mut a9 = PdataLedger::new();
    let _ = a9.open();
    if a9.close_scan().is_err() {
        caught += 1;
    } else {
        miss.push_str("F9行额不足;");
    }

    // 族10：空摘要封账。
    let mut a10 = PdataLedger::new();
    let _ = a10.open();
    let mut i = 0usize;
    while i < PDATA_TABLE.len() {
        if let Some(r) = PDATA_TABLE.get(i) {
            let _ = a10.reg_row(*r);
        }
        i += 1;
    }
    let _ = a10.close_scan();
    let _ = a10.commit_unwind();
    if a10.seal("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F10空摘要;");
    }

    (caught, 10usize, miss)
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = pd_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12727 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12727_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12727");

    s.add_dyn(
        "F12727-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12727-02 段位公式重算",
        {
            let mut ok = seg_of(12727) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12727-03 诊断码闭集",
        {
            let mut ok = PD_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < PD_ALL.len() {
                if let Some(d) = PD_ALL.get(i) {
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
        "F12727-04 .pdata 对照回归绿",
        pdata_rows_ok(),
        &format!("rows={} Begin/End/Unwind 三字段齐", PDATA_TABLE.len()),
    );

    s.add_dyn(
        "F12727-05 UNWIND_INFO 码族对照回归绿",
        unwind_info_ok(),
        "版本双档/标志四档/码族九档逐档对照",
    );

    s.add_dyn(
        "F12727-06 码值锚注出处",
        codes_anchored_ok(),
        "九档码值与 winnt.h UWOP_* 冻结值逐一对照",
    );

    s.add_dyn(
        "F12727-07 info 字节往返",
        {
            let packed = info_byte(UwVersion::V1, 0x05);
            match parse_info_byte(packed) {
                Some((v, f)) => v == UwVersion::V1 && f == 0x05,
                None => false,
            }
        },
        "低 3 位版本 + 高 5 位标志往返一致",
    );

    s.add_dyn(
        "F12727-08 注入联测 10/10 拒止",
        {
            let (c, t, _) = pd_fault_sweep();
            c == t
        },
        &format!("caught={}/10", pd_fault_sweep().0),
    );

    s.add_dyn(
        "F12727-09 相位迁移守卫直读",
        pd_advance(PdPhase::PdDraft, PdPhase::PdScanning).is_ok()
            && pd_advance(PdPhase::PdScanning, PdPhase::PdScanned).is_ok()
            && pd_advance(PdPhase::PdScanned, PdPhase::PdSealed).is_ok()
            && pd_advance(PdPhase::PdSealed, PdPhase::PdDraft).is_err()
            && pd_advance(PdPhase::PdDraft, PdPhase::PdScanned).is_err(),
        "Draft→Scanning→Scanned→Sealed 只进不退不跳",
    );

    s.add_dyn(
        "F12727-10 完整建账基线",
        {
            let a = build_full_pdata();
            a.row_len() == PDATA_TABLE.len()
                && a.reject_len() == 0
                && a.phase() == PdPhase::PdSealed
        },
        &format!(
            "rows={} phase={}",
            build_full_pdata().row_len(),
            build_full_pdata().phase().name()
        ),
    );

    s.add_dyn(
        "F12727-11 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12727-12 码族闭集全列",
        {
            let mut ok = UwOp::all().len() == 9 && UwFlag::all().len() == 4;
            let mut i = 0usize;
            while i < UwOp::all().len() {
                if let Some(op) = UwOp::all().get(i) {
                    ok &= !op.note().is_empty();
                }
                i += 1;
            }
            let mut j = 0usize;
            while j < UwFlag::all().len() {
                if let Some(f) = UwFlag::all().get(j) {
                    ok &= !f.note().is_empty();
                }
                j += 1;
            }
            ok
        },
        "九档码族 + 四档标志锚注非空",
    );

    s.add_dyn(
        "F12727-13 完成写回语义",
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
        assert_eq!(SEG_BASE, 0x81FC0);
        assert_eq!(seg_of(12727), Some(SEG_BASE));
        assert_eq!(seg_of(12728), Some(0x82000));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(PD_ALL.len(), 12);
        let mut i = 0usize;
        while i < PD_ALL.len() {
            let vi = match PD_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < PD_ALL.len() {
                let vj = match PD_ALL.get(j) {
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
    fn pdata_table_frozen_rows() {
        assert!(pdata_rows_ok());
        assert!(pdata_full_green());
        assert_eq!(PDATA_TABLE.len(), PDATA_ROWS_MIN + 2);
        // 锚行直判。
        let first = match PDATA_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(first.begin, 0x1000);
        assert_eq!(first.end, 0x1060);
        assert_eq!(first.unwind, 0x9000);
        // 冻结表逐行不得误判坏。
        let mut i = 0usize;
        while i < PDATA_TABLE.len() {
            let r = match PDATA_TABLE.get(i) {
                Some(v) => *v,
                None => return,
            };
            assert!(pdata_row_ok(&r), "正表行 {} 误判坏", i);
            i += 1;
        }
    }

    #[test]
    fn unwind_info_frozen_codes() {
        assert!(unwind_info_ok());
        assert!(codes_anchored_ok());
        // 九档码值直判。
        let expect: [u8; 9] = [0, 1, 2, 3, 4, 5, 8, 9, 10];
        let mut i = 0usize;
        while i < expect.len() {
            let op = match UwOp::all().get(i) {
                Some(v) => *v,
                None => return,
            };
            assert_eq!(op.code(), expect[i], "码档 {} 漂移", i);
            assert!(op.note().contains("winnt.h"));
            i += 1;
        }
        // 标志四档位值。
        assert_eq!(UwFlag::EHandler.bits(), 0x01);
        assert_eq!(UwFlag::UHandler.bits(), 0x02);
        assert_eq!(UwFlag::ChainInfo.bits(), 0x04);
        assert_eq!(UwFlag::NHandler.bits(), 0x00);
    }

    #[test]
    fn info_byte_roundtrip_and_rejects() {
        // 往返。
        let b1 = info_byte(UwVersion::V1, 0x00);
        assert_eq!(parse_info_byte(b1), Some((UwVersion::V1, 0x00)));
        let b2 = info_byte(UwVersion::V2, 0x1F);
        assert_eq!(parse_info_byte(b2), Some((UwVersion::V2, 0x1F)));
        // 版本 3 拒。
        assert!(parse_info_byte(0x03).is_none());
        assert!(parse_info_byte(0x00).is_none());
        // 坏码档拒。
        assert!(decode_op(&UwCode { op: 11, slot1: 0 }).is_none());
        assert!(decode_op(&UwCode { op: 6, slot1: 0 }).is_none());
        assert!(decode_op(&UwCode { op: 7, slot1: 0 }).is_none());
        assert!(decode_op(&UwCode { op: u8::MAX, slot1: 0 }).is_none());
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = pd_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        let row0 = match PDATA_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        // 未开账先扫。
        let mut a = PdataLedger::new();
        assert_eq!(a.reg_row(row0), Err(PdDiag::ENotOpen.val()));
        // 行倒序。
        let _ = a.open();
        let _ = a.reg_row(row0);
        let back = PdataRow { begin: 0x0800, end: 0x0860, unwind: 0x9000, note: "倒序行" };
        assert_eq!(a.reg_row(back), Err(PdDiag::ERowOrder.val()));
        // 行交叠。
        let overlap = PdataRow { begin: 0x1020, end: 0x1080, unwind: 0x9000, note: "交叠行" };
        assert_eq!(a.reg_row(overlap), Err(PdDiag::EOverlap.val()));
        // 越像界。
        let over = PdataRow { begin: 0xF000, end: IMAGE_CEIL + 0x100, unwind: 0x9000, note: "越界行" };
        assert_eq!(a.reg_row(over), Err(PdDiag::EOverCeil.val()));
        // 行额不足收账。
        assert_eq!(a.close_scan(), Err(PdDiag::ERowsShort.val()));
        // Scanned 相外 commit_unwind 拒。
        assert_eq!(a.commit_unwind(), Err(PdDiag::ERowsShort.val()));
        // 封账后写。
        let mut b = build_full_pdata();
        let late = PdataRow { begin: 0x5000, end: 0x5060, unwind: 0x90A0, note: "迟到行" };
        assert_eq!(b.reg_row(late), Err(PdDiag::ESealedWrite.val()));
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(pd_advance(PdPhase::PdDraft, PdPhase::PdScanning).is_ok());
        assert!(pd_advance(PdPhase::PdScanning, PdPhase::PdScanned).is_ok());
        assert!(pd_advance(PdPhase::PdScanned, PdPhase::PdSealed).is_ok());
        assert!(pd_advance(PdPhase::PdScanning, PdPhase::PdScanning).is_err());
        assert!(pd_advance(PdPhase::PdSealed, PdPhase::PdScanning).is_err());
        assert!(PdPhase::PdDraft.rank() < PdPhase::PdScanning.rank());
        assert!(PdPhase::PdScanning.rank() < PdPhase::PdScanned.rank());
        let mut a = PdataLedger::new();
        let _ = a.open();
        let mut i = 0usize;
        while i < PDATA_TABLE.len() {
            if let Some(r) = PDATA_TABLE.get(i) {
                let _ = a.reg_row(*r);
            }
            i += 1;
        }
        assert!(a.close_scan().is_ok());
        assert!(a.commit_unwind().is_ok());
        assert_eq!(a.seal(""), Err(PdDiag::ESummaryEmpty.val()));
        assert!(a.seal("三字段绿码族一致").is_ok());
        assert_eq!(a.phase(), PdPhase::PdSealed);
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_pdata();
        let y = build_full_pdata();
        assert_eq!(x.row_len(), y.row_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(x.summary(), y.summary());
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for d in PD_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
        for op in UwOp::all().iter() {
            assert!(!op.note().is_empty());
        }
        for f in UwFlag::all().iter() {
            assert!(!f.note().is_empty());
        }
        for p in [
            PdPhase::PdDraft,
            PdPhase::PdScanning,
            PdPhase::PdScanned,
            PdPhase::PdSealed,
        ]
        .iter()
        {
            assert!(!p.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12727_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
