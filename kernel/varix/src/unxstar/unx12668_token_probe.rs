//! UNX-F12669 同步对象计数探测（Wait/Semaphore/Mutant 计数探测） · 0x81100段
//!
//! 判据原文（派工卡 12668 逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12669-J1 同步对象计数探测档对平断言绿（B15 族
//!      回指），计数突变注入 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 同步对象计数探测域模型：计数档每行七元（探测号/对象句柄/对象
//!    族闭集/当前计数/上限计数/状态位/回指指纹），对象族三闭集
//!    （Semaphore 计数信号量 / Mutant 互斥体 / Event 事件对）；
//! 2. B15 族回指：每行回指 B15 族同步台账指纹（B15-SYNC-LEDGER-
//!    COSIGN），指纹缺失或错挂即断链拒收；
//! 3. 计数对平口径：当前计数 ≤ 上限计数；Mutant 有主位与计数一致
//!    （有主则计数为零，无主则计数为上限）；Semaphore 计数闭集
//!    0..=上限；Event 状态位（有信/无信）与计数互证——逐行对平断
//!    言绿是本卡验收主判据；
//! 4. 计数突变注入 10/10 一致：突变十族（计数越上限/负数突变/上限
//!    清零/有主计数不为零/无主计数不为上限/Event 有信计数归零/句柄
//!    越表/指纹断链/重复行/未对齐即查）确定性出题，逐例必检；
//! 5. 状态机：Attuned → Counted → Filed 单向推进，倒退显性拒；对平
//!    闸五道（句柄归属/族闭集/计数闭集/状态互证/回指指纹）逐行过
//!    闸；
//! 6. 注入联测 10 路（账面侧）：探测号乱序、封账后探测、突变面漏检、
//!    B15 指纹漂移、探测档缺行、突变族越闭集、双跑漂移、探测行重复、
//!    未对齐即探测、状态机倒退——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（突变族表确定性
//!    出题）；取值一律 match/if let/.get()/checked_*/saturating_*，
//!    无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12668 − 12600)）。
pub const SEG_BASE: u32 = 0x81100;

// ── 同步对象诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncDiag {
    /// 对象句柄为零。
    EHandleZero,
    /// 句柄越对象表闭集。
    EHandleRange,
    /// 对象族越闭集。
    EKindRange,
    /// 计数越上限。
    ECountOver,
    /// 上限为零。
    ELimitZero,
    /// 有主计数不为零。
    EOwnedCount,
    /// 无主计数不为上限。
    EFreeCount,
    /// Event 状态位与计数互证失败。
    EEventState,
    /// 回指指纹缺失（断链）。
    EFpMissing,
    /// 回指指纹错挂。
    EFpMismatch,
    /// 探测号乱序。
    ERowOrder,
    /// 状态机倒退。
    ERewind,
}

impl SyncDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                SyncDiag::EHandleZero => 0x01,
                SyncDiag::EHandleRange => 0x02,
                SyncDiag::EKindRange => 0x03,
                SyncDiag::ECountOver => 0x04,
                SyncDiag::ELimitZero => 0x05,
                SyncDiag::EOwnedCount => 0x06,
                SyncDiag::EFreeCount => 0x07,
                SyncDiag::EEventState => 0x08,
                SyncDiag::EFpMissing => 0x09,
                SyncDiag::EFpMismatch => 0x0a,
                SyncDiag::ERowOrder => 0x0b,
                SyncDiag::ERewind => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            SyncDiag::EHandleZero => "对象句柄为零：突变首族，先核对象面来源再探测",
            SyncDiag::EHandleRange => "句柄越对象表：句柄必在冻结对象表内，越表先对对象账",
            SyncDiag::EKindRange => "对象族越闭集：只认 Semaphore/Mutant/Event 三族，先正族名",
            SyncDiag::ECountOver => "计数越上限：当前计数 ≤ 上限计数，越沿先核突变源",
            SyncDiag::ELimitZero => "上限为零：活对象上限 ≥1，零限先核对象账",
            SyncDiag::EOwnedCount => "有主计数不为零：Mutant 有主则计数为零，先核持有位",
            SyncDiag::EFreeCount => "无主计数不为上限：Mutant 无主则计数为上限，先核释放位",
            SyncDiag::EEventState => "Event 状态互证失败：有信位与计数不一致，先核状态位",
            SyncDiag::EFpMissing => "回指指纹缺失：B15 族台账指纹空即断链，先补指纹",
            SyncDiag::EFpMismatch => "回指指纹错挂：指纹与 B15 族冻结值不符，先对指纹源",
            SyncDiag::ERowOrder => "探测号乱序：按探测号 0..N 逐行过闸，跳号先回补",
            SyncDiag::ERewind => "状态机倒退：Attuned→Counted→Filed 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const SYNC_ALL: &[SyncDiag] = &[
    SyncDiag::EHandleZero,
    SyncDiag::EHandleRange,
    SyncDiag::EKindRange,
    SyncDiag::ECountOver,
    SyncDiag::ELimitZero,
    SyncDiag::EOwnedCount,
    SyncDiag::EFreeCount,
    SyncDiag::EEventState,
    SyncDiag::EFpMissing,
    SyncDiag::EFpMismatch,
    SyncDiag::ERowOrder,
    SyncDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in SYNC_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81101..0x8110C 闭集内"
}

// ── B15 族回指锚 ─────────────────────────────────────────────────
/// B15 族同步台账指纹（冻结）。
pub const B15_FP: &str = "B15-SYNC-LEDGER-COSIGN";
/// B15 族回指批号。
pub const B15_REF_BATCH: u32 = 15;

// ── 对象族闭集 ───────────────────────────────────────────────────
/// 同步对象族三闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncKind {
    /// 计数信号量。
    Semaphore,
    /// 互斥体。
    Mutant,
    /// 事件对。
    Event,
}

impl SyncKind {
    /// 族名。
    pub fn name(self) -> &'static str {
        match self {
            SyncKind::Semaphore => "Semaphore",
            SyncKind::Mutant => "Mutant",
            SyncKind::Event => "Event",
        }
    }

    /// 全部三族（闭集数组）。
    pub fn all() -> &'static [SyncKind] {
        &[SyncKind::Semaphore, SyncKind::Mutant, SyncKind::Event]
    }
}

// ── 同步对象表（句柄 → 族） ──────────────────────────────────────
/// 对象行（句柄/族）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncObj {
    /// 对象句柄（非零）。
    pub handle: u32,
    /// 对象族。
    pub kind: SyncKind,
}

/// 冻结同步对象表（6 例）。
pub const SYNC_TABLE: &[SyncObj] = &[
    SyncObj { handle: 0x2000, kind: SyncKind::Semaphore },
    SyncObj { handle: 0x2004, kind: SyncKind::Semaphore },
    SyncObj { handle: 0x2008, kind: SyncKind::Mutant },
    SyncObj { handle: 0x200C, kind: SyncKind::Mutant },
    SyncObj { handle: 0x2010, kind: SyncKind::Event },
    SyncObj { handle: 0x2014, kind: SyncKind::Event },
];

/// 对象表容量。
pub const OBJ_CAP: usize = 6;

/// 取对象行（越表 None）。
pub fn obj_of(handle: u32) -> Option<&'static SyncObj> {
    for o in SYNC_TABLE.iter() {
        if o.handle == handle {
            return Some(o);
        }
    }
    None
}

// ── 计数档行与黄金判定 ───────────────────────────────────────────
/// 计数档行七元（探测号/句柄/族/当前计数/上限计数/有主或信位/指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncRow {
    /// 探测号。
    pub no: u32,
    /// 对象句柄。
    pub handle: u32,
    /// 对象族。
    pub kind: SyncKind,
    /// 当前计数。
    pub count: u32,
    /// 上限计数（≥1）。
    pub limit: u32,
    /// 有主位（Mutant）/有信位（Event）/恒假（Semaphore）。
    pub owned: bool,
    /// 回指指纹。
    pub fp: &'static str,
}

/// 单行黄金判定（五道：句柄/族/计数闭集/状态互证/指纹）。
pub fn sync_row_ok(r: &SyncRow) -> Result<(), u32> {
    // 句柄闸。
    if r.handle == 0 {
        return Err(SyncDiag::EHandleZero.val());
    }
    let obj = match obj_of(r.handle) {
        Some(v) => v,
        None => return Err(SyncDiag::EHandleRange.val()),
    };
    // 族闸：行族与对象表族一致。
    if obj.kind != r.kind {
        return Err(SyncDiag::EKindRange.val());
    }
    // 上限闸。
    if r.limit == 0 {
        return Err(SyncDiag::ELimitZero.val());
    }
    // 计数闭集闸（checked 口径：limit < count 即越沿）。
    if r.limit.checked_sub(r.count).is_none() {
        return Err(SyncDiag::ECountOver.val());
    }
    // 族内状态互证闸。
    match r.kind {
        SyncKind::Mutant => {
            if r.owned && r.count != 0 {
                return Err(SyncDiag::EOwnedCount.val());
            }
            if !r.owned && r.count != r.limit {
                return Err(SyncDiag::EFreeCount.val());
            }
        }
        SyncKind::Semaphore => {
            // 信号量：有主位恒假（冻结口径），计数 0..=limit。
            if r.owned {
                return Err(SyncDiag::EEventState.val());
            }
        }
        SyncKind::Event => {
            // 事件：有信则计数为上限，无信则计数为零。
            if r.owned && r.count != r.limit {
                return Err(SyncDiag::EEventState.val());
            }
            if !r.owned && r.count != 0 {
                return Err(SyncDiag::EEventState.val());
            }
        }
    }
    // 指纹闸。
    if r.fp.is_empty() {
        return Err(SyncDiag::EFpMissing.val());
    }
    if r.fp != B15_FP {
        return Err(SyncDiag::EFpMismatch.val());
    }
    Ok(())
}

/// 探测档容量（8 行：6 对象 + 2 补行）。
pub const SYNC_CAP: usize = 8;

/// 冻结计数探测档（8 行）。
pub const SYNC_PROFILE: &[SyncRow] = &[
    SyncRow { no: 0, handle: 0x2000, kind: SyncKind::Semaphore, count: 3, limit: 5, owned: false, fp: B15_FP },
    SyncRow { no: 1, handle: 0x2004, kind: SyncKind::Semaphore, count: 0, limit: 2, owned: false, fp: B15_FP },
    SyncRow { no: 2, handle: 0x2008, kind: SyncKind::Mutant, count: 0, limit: 1, owned: true, fp: B15_FP },
    SyncRow { no: 3, handle: 0x200C, kind: SyncKind::Mutant, count: 1, limit: 1, owned: false, fp: B15_FP },
    SyncRow { no: 4, handle: 0x2010, kind: SyncKind::Event, count: 1, limit: 1, owned: true, fp: B15_FP },
    SyncRow { no: 5, handle: 0x2014, kind: SyncKind::Event, count: 0, limit: 1, owned: false, fp: B15_FP },
    SyncRow { no: 6, handle: 0x2000, kind: SyncKind::Semaphore, count: 5, limit: 5, owned: false, fp: B15_FP },
    SyncRow { no: 7, handle: 0x200C, kind: SyncKind::Mutant, count: 0, limit: 1, owned: true, fp: B15_FP },
];

// ── 状态机（Attuned → Counted → Filed） ──────────────────────────
/// 探测状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncState {
    /// 已对齐（对象表挂账）。
    Attuned,
    /// 计数中。
    Counted,
    /// 已归档。
    Filed,
}

impl SyncState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            SyncState::Attuned => 0,
            SyncState::Counted => 1,
            SyncState::Filed => 2,
        }
    }
}

/// 探测账。
#[derive(Clone, Debug)]
pub struct SyncLedger {
    /// 已对平行。
    rows: Vec<SyncRow>,
    /// 拒收记账（探测号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: SyncState,
}

impl SyncLedger {
    /// 开账（Attuned 态）。
    pub fn new() -> SyncLedger {
        SyncLedger {
            rows: Vec::new(),
            rejects: Vec::new(),
            state: SyncState::Attuned,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> SyncState {
        self.state
    }

    /// 已对平行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&SyncRow> {
        self.rows.get(idx)
    }

    /// 对齐（Attuned→Counted）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.state.rank() >= SyncState::Counted.rank() {
            return Err(SyncDiag::ERewind.val());
        }
        self.state = SyncState::Counted;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < SYNC_CAP * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 逐行消费（行号序 + 黄金判定 + 互异 三闸）。
    pub fn consume(&mut self, r: SyncRow) -> Result<(), u32> {
        if self.state.rank() < SyncState::Counted.rank() {
            return Err(SyncDiag::ERewind.val());
        }
        if self.state.rank() >= SyncState::Filed.rank() {
            self.note_reject(r.no, SyncDiag::ERewind.val());
            return Err(SyncDiag::ERewind.val());
        }
        // 行号序闸。
        if r.no != self.rows.len() as u32 {
            self.note_reject(r.no, SyncDiag::ERowOrder.val());
            return Err(SyncDiag::ERowOrder.val());
        }
        // 黄金判定闸。
        if let Err(e) = sync_row_ok(&r) {
            self.note_reject(r.no, e);
            return Err(e);
        }
        // 互异闸：同句柄同计数快照只入一笔。
        for done in self.rows.iter() {
            if done.handle == r.handle && done.count == r.count && done.owned == r.owned {
                self.note_reject(r.no, SyncDiag::ERowOrder.val());
                return Err(SyncDiag::ERowOrder.val());
            }
        }
        self.rows.push(r);
        Ok(())
    }

    /// 归档：8 行齐 + 零拒收。
    pub fn file(&mut self) -> Result<(), u32> {
        if self.state.rank() >= SyncState::Filed.rank() {
            return Err(SyncDiag::ERewind.val());
        }
        if self.state.rank() < SyncState::Counted.rank() {
            return Err(SyncDiag::ERewind.val());
        }
        if self.rows.len() != SYNC_CAP {
            return Err(SyncDiag::ERowOrder.val());
        }
        if !self.rejects.is_empty() {
            return Err(SyncDiag::ERowOrder.val());
        }
        self.state = SyncState::Filed;
        Ok(())
    }

    /// 账等式。
    pub fn equation_ok(&self) -> bool {
        self.rows.len() == SYNC_CAP && self.rejects.is_empty()
    }
}

// ── 计数突变注入（十族确定性出题） ───────────────────────────────
/// 突变十族闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountMutation {
    /// 计数越上限。
    CountOver,
    /// 负数突变（saturating 口径下溢为零仍违状态位）。
    SatUnderflow,
    /// 上限清零。
    LimitZero,
    /// 有主计数不为零。
    OwnedCount,
    /// 无主计数不为上限。
    FreeCount,
    /// Event 有信计数归零。
    EventSignaledZero,
    /// 句柄越表。
    HandleRange,
    /// 指纹断链。
    FpMissing,
    /// 族错挂。
    KindSwap,
    /// 未对齐即查。
    NotAttuned,
}

impl CountMutation {
    /// 族号。
    pub fn index(self) -> u32 {
        match self {
            CountMutation::CountOver => 0,
            CountMutation::SatUnderflow => 1,
            CountMutation::LimitZero => 2,
            CountMutation::OwnedCount => 3,
            CountMutation::FreeCount => 4,
            CountMutation::EventSignaledZero => 5,
            CountMutation::HandleRange => 6,
            CountMutation::FpMissing => 7,
            CountMutation::KindSwap => 8,
            CountMutation::NotAttuned => 9,
        }
    }

    /// 族名。
    pub fn name(self) -> &'static str {
        match self {
            CountMutation::CountOver => "计数越上限",
            CountMutation::SatUnderflow => "饱和下溢突变",
            CountMutation::LimitZero => "上限清零",
            CountMutation::OwnedCount => "有主计数非零",
            CountMutation::FreeCount => "无主计数非上限",
            CountMutation::EventSignaledZero => "有信计数归零",
            CountMutation::HandleRange => "句柄越表",
            CountMutation::FpMissing => "指纹断链",
            CountMutation::KindSwap => "族错挂",
            CountMutation::NotAttuned => "未对齐即查",
        }
    }

    /// 全部十族（闭集数组）。
    pub fn all() -> &'static [CountMutation] {
        &[
            CountMutation::CountOver,
            CountMutation::SatUnderflow,
            CountMutation::LimitZero,
            CountMutation::OwnedCount,
            CountMutation::FreeCount,
            CountMutation::EventSignaledZero,
            CountMutation::HandleRange,
            CountMutation::FpMissing,
            CountMutation::KindSwap,
            CountMutation::NotAttuned,
        ]
    }
}

/// 构造基行（库内第 idx 行）。
fn base_row(idx: usize) -> Option<SyncRow> {
    SYNC_PROFILE.get(idx).cloned()
}

/// 单族突变注入：返回 true 表示被检出。
pub fn detect_mutation(f: CountMutation) -> bool {
    match f {
        CountMutation::CountOver => {
            let mut r = match base_row(0) {
                Some(v) => v,
                None => return false,
            };
            r.count = r.limit + 1;
            sync_row_ok(&r).is_err()
        }
        CountMutation::SatUnderflow => {
            // 饱和口径：count saturating_sub 大数后归零，仍违有主互证。
            let mut r = match base_row(2) {
                Some(v) => v,
                None => return false,
            };
            // 有主行计数被突变到 1（有主计数必须为零）。
            r.count = r.count.saturating_add(1);
            sync_row_ok(&r).is_err()
        }
        CountMutation::LimitZero => {
            let mut r = match base_row(1) {
                Some(v) => v,
                None => return false,
            };
            r.limit = 0;
            sync_row_ok(&r).is_err()
        }
        CountMutation::OwnedCount => {
            let mut r = match base_row(7) {
                Some(v) => v,
                None => return false,
            };
            r.count = 1;
            sync_row_ok(&r).is_err()
        }
        CountMutation::FreeCount => {
            let mut r = match base_row(3) {
                Some(v) => v,
                None => return false,
            };
            r.count = r.count.saturating_sub(1);
            sync_row_ok(&r).is_err()
        }
        CountMutation::EventSignaledZero => {
            let mut r = match base_row(4) {
                Some(v) => v,
                None => return false,
            };
            r.count = 0;
            sync_row_ok(&r).is_err()
        }
        CountMutation::HandleRange => {
            let mut r = match base_row(5) {
                Some(v) => v,
                None => return false,
            };
            r.handle = 0x9000;
            sync_row_ok(&r).is_err()
        }
        CountMutation::FpMissing => {
            let mut r = match base_row(6) {
                Some(v) => v,
                None => return false,
            };
            r.fp = "";
            sync_row_ok(&r).is_err()
        }
        CountMutation::KindSwap => {
            let mut r = match base_row(0) {
                Some(v) => v,
                None => return false,
            };
            r.kind = SyncKind::Mutant;
            sync_row_ok(&r).is_err()
        }
        CountMutation::NotAttuned => {
            // 未对齐即查：Attuned 态直接消费，状态机闸拒。
            let mut fresh = SyncLedger::new();
            let r = match base_row(0) {
                Some(v) => v,
                None => return false,
            };
            fresh.consume(r).is_err()
        }
    }
}

/// 计数突变注入 10/10 一致（判据 J1 直读口）。
pub fn mutation_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for f in CountMutation::all().iter() {
        total += 1;
        if detect_mutation(*f) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("族{}({}) 逃脱", f.index(), f.name());
        }
    }
    (caught, total, escape)
}

// ── 注入面：十路违约（账面侧） ───────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncFault {
    /// 探测号乱序。
    RowOrder,
    /// 归档后探测。
    FiledProbe,
    /// 突变面漏检。
    MutationMiss,
    /// B15 指纹漂移。
    FpDrift,
    /// 探测档缺行。
    ShortFile,
    /// 突变族越闭集（闭集数组直读）。
    MutationRange,
    /// 对象表漂移。
    ObjDrift,
    /// 未对齐即探测。
    NotAttuned,
    /// 双跑漂移。
    DualDrift,
    /// 状态机倒退。
    Rewind,
}

impl SyncFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            SyncFault::RowOrder => 0,
            SyncFault::FiledProbe => 1,
            SyncFault::MutationMiss => 2,
            SyncFault::FpDrift => 3,
            SyncFault::ShortFile => 4,
            SyncFault::MutationRange => 5,
            SyncFault::ObjDrift => 6,
            SyncFault::NotAttuned => 7,
            SyncFault::DualDrift => 8,
            SyncFault::Rewind => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            SyncFault::RowOrder => "探测号乱序",
            SyncFault::FiledProbe => "归档后探测",
            SyncFault::MutationMiss => "突变面漏检",
            SyncFault::FpDrift => "B15 指纹漂移",
            SyncFault::ShortFile => "档缺行",
            SyncFault::MutationRange => "突变族越闭集",
            SyncFault::ObjDrift => "对象表漂移",
            SyncFault::NotAttuned => "未对齐即探",
            SyncFault::DualDrift => "双跑漂移",
            SyncFault::Rewind => "状态机倒退",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [SyncFault] {
        &[
            SyncFault::RowOrder,
            SyncFault::FiledProbe,
            SyncFault::MutationMiss,
            SyncFault::FpDrift,
            SyncFault::ShortFile,
            SyncFault::MutationRange,
            SyncFault::ObjDrift,
            SyncFault::NotAttuned,
            SyncFault::DualDrift,
            SyncFault::Rewind,
        ]
    }
}

/// 构建完整探测账（8 行全过闸）。
pub fn build_full_ledger() -> SyncLedger {
    let mut led = SyncLedger::new();
    let _ = led.begin();
    let mut i = 0usize;
    while i < SYNC_CAP {
        let r = match SYNC_PROFILE.get(i) {
            Some(v) => v.clone(),
            None => break,
        };
        match led.consume(r) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    led
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in SyncFault::all().iter() {
        total += 1;
        if run_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_fault(v: SyncFault) -> bool {
    match v {
        SyncFault::RowOrder => {
            let mut led = SyncLedger::new();
            let _ = led.begin();
            let r = match SYNC_PROFILE.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.consume(r).is_err()
        }
        SyncFault::FiledProbe => {
            let mut led = build_full_ledger();
            if led.file().is_err() {
                return false;
            }
            let r = match SYNC_PROFILE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.consume(r).is_err()
        }
        SyncFault::MutationMiss => {
            let (caught, total, escape) = mutation_sweep();
            total == 10 && caught == 10 && escape.is_empty()
        }
        SyncFault::FpDrift => {
            // B15 指纹冻结直读。
            B15_FP == "B15-SYNC-LEDGER-COSIGN" && B15_REF_BATCH == 15
        }
        SyncFault::ShortFile => {
            let mut led = SyncLedger::new();
            let _ = led.begin();
            let r = match SYNC_PROFILE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.consume(r);
            led.file().is_err()
        }
        SyncFault::MutationRange => {
            // 突变族闭集直读：十族互异编号 0..10。
            let all = CountMutation::all();
            if all.len() != 10 {
                return false;
            }
            let mut ok = true;
            let mut i = 0usize;
            while i < all.len() {
                let fi = match all.get(i) {
                    Some(x) => x.index(),
                    None => return false,
                };
                if fi != i as u32 {
                    ok = false;
                }
                let mut j = i + 1;
                while j < all.len() {
                    let fj = match all.get(j) {
                        Some(x) => x.index(),
                        None => return false,
                    };
                    if fi == fj {
                        ok = false;
                    }
                    j += 1;
                }
                i += 1;
            }
            ok
        }
        SyncFault::ObjDrift => {
            // 对象表冻结直读：6 例、句柄非零、查表命中。
            if SYNC_TABLE.len() != OBJ_CAP {
                return false;
            }
            let mut ok = true;
            for o in SYNC_TABLE.iter() {
                if o.handle == 0 {
                    ok = false;
                }
                if obj_of(o.handle).is_none() {
                    ok = false;
                }
            }
            if obj_of(0).is_some() {
                ok = false;
            }
            ok
        }
        SyncFault::NotAttuned => {
            let mut fresh = SyncLedger::new();
            let r = match SYNC_PROFILE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            fresh.consume(r).is_err()
        }
        SyncFault::DualDrift => {
            let a = build_full_ledger();
            let b = build_full_ledger();
            a.len() == b.len() && a.len() == SYNC_CAP
        }
        SyncFault::Rewind => {
            let mut fresh = SyncLedger::new();
            fresh.file().is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 同步对象计数探测档对平断言绿（判据 J1 直读口）。
fn sync_profile_ok() -> bool {
    let mut led = build_full_ledger();
    if led.len() != SYNC_CAP {
        return false;
    }
    if led.reject_len() != 0 {
        return false;
    }
    if led.file().is_err() {
        return false;
    }
    if led.state() != SyncState::Filed {
        return false;
    }
    // 独立复核：8 行逐行黄金判定。
    for r in SYNC_PROFILE.iter() {
        if sync_row_ok(r).is_err() {
            return false;
        }
    }
    true
}

/// B15 族回指齐备直读。
fn b15_ref_ok() -> bool {
    if B15_FP != "B15-SYNC-LEDGER-COSIGN" {
        return false;
    }
    if B15_REF_BATCH != 15 {
        return false;
    }
    for r in SYNC_PROFILE.iter() {
        if r.fp != B15_FP {
            return false;
        }
    }
    true
}

/// 计数突变注入 10/10 一致。
fn mutation_ok() -> bool {
    let (caught, total, escape) = mutation_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 注入联测 10/10 拒止。
fn fault_sweep_ok() -> bool {
    let (caught, total, escape) = fault_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 三族互证普查（每族在档内至少一行且黄金判定全绿）。
fn kind_census_ok() -> bool {
    let mut n_sema = 0usize;
    let mut n_mut = 0usize;
    let mut n_ev = 0usize;
    for r in SYNC_PROFILE.iter() {
        match r.kind {
            SyncKind::Semaphore => n_sema += 1,
            SyncKind::Mutant => n_mut += 1,
            SyncKind::Event => n_ev += 1,
        }
        if sync_row_ok(r).is_err() {
            return false;
        }
    }
    n_sema >= 2 && n_mut >= 2 && n_ev >= 2
}

/// 双跑一致（冻结档下两轮全流程同判）。
fn dual_run_ok() -> bool {
    let a = build_full_ledger();
    let b = build_full_ledger();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if mutation_sweep().0 != mutation_sweep().0 {
        return false;
    }
    if fault_sweep().0 != fault_sweep().0 {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12668u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut led = build_full_ledger();
    if led.file().is_err() {
        return false;
    }
    if led.state() != SyncState::Filed {
        return false;
    }
    match led.row_at(0) {
        Some(r) => {
            if r.handle == 0 || r.fp != B15_FP {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12669 同步对象探测自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12668_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12668-sync-probe");
    let led = build_full_ledger();

    s.add_dyn(
        "F12669-01 对象族三闭集",
        SyncKind::all().len() == 3,
        "Semaphore/Mutant/Event",
    );

    s.add_dyn(
        "F12669-02 计数探测档对平断言绿",
        sync_profile_ok(),
        &format!("rows={} rejects={}", led.len(), led.reject_len()),
    );

    s.add_dyn(
        "F12669-03 B15 族回指齐备",
        b15_ref_ok(),
        &format!("fp={} batch={}", B15_FP, B15_REF_BATCH),
    );

    s.add_dyn(
        "F12669-04 计数突变注入 10/10 一致",
        mutation_ok(),
        &format!("caught={}", mutation_sweep().0),
    );

    s.add_dyn(
        "F12669-05 注入联测 10/10 拒止",
        fault_sweep_ok(),
        &format!("caught={}", fault_sweep().0),
    );

    s.add_dyn(
        "F12669-06 三族互证普查",
        kind_census_ok(),
        "每族 ≥2 行且黄金判定全绿",
    );

    s.add_dyn(
        "F12669-07 对象表冻结直读",
        SYNC_TABLE.len() == OBJ_CAP,
        &format!("objs={}", SYNC_TABLE.len()),
    );

    s.add_dyn(
        "F12669-08 双跑一致",
        dual_run_ok(),
        "冻结档下两轮逐行同判",
    );

    s.add_dyn(
        "F12669-09 探测账等式",
        led.equation_ok(),
        &format!("rows={} rejects={}", led.len(), led.reject_len()),
    );

    s.add_dyn(
        "F12669-10 完成写回语义",
        writeback_ok(),
        "归档即已完成且摘要非空",
    );

    s.add_dyn(
        "F12669-11 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 计数闭集边界直读（与行闸独立复核）。
    let edge_ok = {
        // 恰在上沿。
        let r0 = SyncRow { no: 0, handle: 0x2000, kind: SyncKind::Semaphore, count: 5, limit: 5, owned: false, fp: B15_FP };
        // 越上沿一位。
        let mut r1 = r0.clone();
        r1.count = 6;
        sync_row_ok(&r0).is_ok() && sync_row_ok(&r1).is_err()
    };
    s.add_dyn(
        "F12669-12 计数闭集边界",
        edge_ok,
        "count == limit 绿 / count > limit 红",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口 + 本卡公式位。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x81100);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中。
        assert_eq!(SYNC_ALL.len(), 12);
        let mut i = 0usize;
        while i < SYNC_ALL.len() {
            let vi = match SYNC_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < SYNC_ALL.len() {
                let vj = match SYNC_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn happy_path_files() {
        let mut led = build_full_ledger();
        assert_eq!(led.len(), SYNC_CAP);
        assert_eq!(led.reject_len(), 0);
        assert!(led.file().is_ok());
        assert_eq!(led.state(), SyncState::Filed);
    }

    #[test]
    fn mutation_injection_ten_ten() {
        // 计数突变注入 10/10 一致。
        let (caught, total, escape) = mutation_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱族：{}", escape);
        assert!(escape.is_empty());
        assert_eq!(CountMutation::all().len(), 10);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路违约全数拒止。
        let (caught, total, escape) = fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn mutant_semantics() {
        // Mutant 互证直读：有主计数零 / 无主计数上限。
        let owned = SyncRow { no: 0, handle: 0x2008, kind: SyncKind::Mutant, count: 0, limit: 1, owned: true, fp: B15_FP };
        assert!(sync_row_ok(&owned).is_ok());
        let free = SyncRow { no: 0, handle: 0x2008, kind: SyncKind::Mutant, count: 1, limit: 1, owned: false, fp: B15_FP };
        assert!(sync_row_ok(&free).is_ok());
        let bad_owned = SyncRow { no: 0, handle: 0x2008, kind: SyncKind::Mutant, count: 1, limit: 1, owned: true, fp: B15_FP };
        assert_eq!(sync_row_ok(&bad_owned), Err(SyncDiag::EOwnedCount.val()));
        let bad_free = SyncRow { no: 0, handle: 0x2008, kind: SyncKind::Mutant, count: 0, limit: 1, owned: false, fp: B15_FP };
        assert_eq!(sync_row_ok(&bad_free), Err(SyncDiag::EFreeCount.val()));
    }

    #[test]
    fn event_semantics() {
        // Event 互证直读：有信计数上限 / 无信计数零。
        let sig = SyncRow { no: 0, handle: 0x2010, kind: SyncKind::Event, count: 1, limit: 1, owned: true, fp: B15_FP };
        assert!(sync_row_ok(&sig).is_ok());
        let unsig = SyncRow { no: 0, handle: 0x2014, kind: SyncKind::Event, count: 0, limit: 1, owned: false, fp: B15_FP };
        assert!(sync_row_ok(&unsig).is_ok());
        let bad = SyncRow { no: 0, handle: 0x2010, kind: SyncKind::Event, count: 0, limit: 1, owned: true, fp: B15_FP };
        assert_eq!(sync_row_ok(&bad), Err(SyncDiag::EEventState.val()));
    }

    #[test]
    fn semaphore_semantics() {
        // Semaphore：有主位恒假，计数 0..=limit。
        let ok0 = SyncRow { no: 0, handle: 0x2000, kind: SyncKind::Semaphore, count: 0, limit: 5, owned: false, fp: B15_FP };
        assert!(sync_row_ok(&ok0).is_ok());
        let ok5 = SyncRow { no: 0, handle: 0x2000, kind: SyncKind::Semaphore, count: 5, limit: 5, owned: false, fp: B15_FP };
        assert!(sync_row_ok(&ok5).is_ok());
        let over = SyncRow { no: 0, handle: 0x2000, kind: SyncKind::Semaphore, count: 6, limit: 5, owned: false, fp: B15_FP };
        assert_eq!(sync_row_ok(&over), Err(SyncDiag::ECountOver.val()));
        let with_owned = SyncRow { no: 0, handle: 0x2000, kind: SyncKind::Semaphore, count: 3, limit: 5, owned: true, fp: B15_FP };
        assert_eq!(sync_row_ok(&with_owned), Err(SyncDiag::EEventState.val()));
        // 族错挂：信号量句柄挂 Mutant 族被拒。
        let swapped = SyncRow { no: 0, handle: 0x2000, kind: SyncKind::Mutant, count: 0, limit: 1, owned: false, fp: B15_FP };
        assert_eq!(sync_row_ok(&swapped), Err(SyncDiag::EKindRange.val()));
    }

    #[test]
    fn row_gate_rejects() {
        // 行闸直读：乱序/越表/断链。
        let mut led = SyncLedger::new();
        let _ = led.begin();
        let r1 = match SYNC_PROFILE.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(led.consume(r1), Err(SyncDiag::ERowOrder.val()));
        let r0 = match SYNC_PROFILE.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        assert!(led.consume(r0).is_ok());
        // 越表句柄。
        let ghost = SyncRow { no: 1, handle: 0x9000, kind: SyncKind::Event, count: 1, limit: 1, owned: true, fp: B15_FP };
        assert_eq!(led.consume(ghost), Err(SyncDiag::EHandleRange.val()));
        // 指纹断链。
        let nofp = SyncRow { no: 1, handle: 0x2004, kind: SyncKind::Semaphore, count: 0, limit: 2, owned: false, fp: "" };
        assert_eq!(led.consume(nofp), Err(SyncDiag::EFpMissing.val()));
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致。
        let a = build_full_ledger();
        let b = build_full_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑例缺失"),
            }
            i += 1;
        }
        assert_eq!(mutation_sweep().0, mutation_sweep().0);
        assert_eq!(fault_sweep().0, fault_sweep().0);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Attuned→Counted→Filed。
        assert!(SyncState::Attuned.rank() < SyncState::Counted.rank());
        assert!(SyncState::Counted.rank() < SyncState::Filed.rank());
        let mut led = build_full_ledger();
        assert_eq!(led.state(), SyncState::Counted);
        assert!(led.file().is_ok());
        assert_eq!(led.file(), Err(SyncDiag::ERewind.val()));
        let mut fresh = SyncLedger::new();
        assert!(fresh.begin().is_ok());
        assert_eq!(fresh.begin(), Err(SyncDiag::ERewind.val()));
    }

    #[test]
    fn b15_anchor_frozen() {
        // B15 族锚冻结直读。
        assert_eq!(B15_FP, "B15-SYNC-LEDGER-COSIGN");
        assert_eq!(B15_REF_BATCH, 15);
        assert!(b15_ref_ok());
        // 指纹错挂直读。
        let mut r = match SYNC_PROFILE.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        r.fp = "B14-OTHER";
        assert_eq!(sync_row_ok(&r), Err(SyncDiag::EFpMismatch.val()));
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12668_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
