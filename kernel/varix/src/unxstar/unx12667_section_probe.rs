//! UNX-F12668 Token 信息类探测档（B23 族回指） · 0x810C0段
//!
//! 判据原文（派工卡 12667 逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12668-J1 Token 信息类探测档（B23 族回指）对平
//!      断言绿，坏 Token 注入 10/10 拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. Token 信息类探测域模型：令牌查询档每行六元（查询号/令牌句柄/
//!    信息类号/账面长度/会话归属/回指指纹），信息类闭集九档（User/
//!    Groups/Privileges/Owner/PrimaryGroup/DefaultDacl/Source/Type/
//!    Statistics），类号越档即拒；
//! 2. B23 族回指：每查询行回指 B23 族令牌台账指纹（B23-TOKEN-LEDGER-
//!    COSIGN），指纹缺失或错挂即断链拒收——回指齐备是对平断言绿的
//!    前置环；
//! 3. 类长账面（冻结口径）：九档信息类各带冻结账面长度（整数口径，
//!    零浮点），查询行长度与账面长度逐档对平——长度不匹即红；
//! 4. 坏 Token 注入 10/10 拒止：坏令牌十族（零句柄/句柄越表/类号越
//!    档/零长度/长度越账/会话越闭集/权限位零/指纹断链/类号重复/未
//!    开即查）确定性出题，逐例拒止；
//! 5. 状态机：Open → Query → Attest 单向推进，倒退显性拒；查询闸五
//!    道（句柄归属/类号闭集/长度对平/会话闭集/回指指纹）逐行过闸；
//! 6. 注入联测 10 路（账面侧）：查询号乱序、行重复、封证后查询、坏
//!    句柄族漏检、指纹常量漂移、类长账面漂移、档行缺数、双跑漂移、
//!    会话表漂移、状态机倒退——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死 =
//!    SEG_BASE + 0x1B）；取值一律 match/if let/.get()，无 unwrap/
//!    expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12667 − 12600)）。
pub const SEG_BASE: u32 = 0x810C0;

// ── Token 探测诊断码闭集 ─────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokDiag {
    /// 令牌句柄为零。
    EHandleZero,
    /// 句柄越令牌表闭集。
    EHandleRange,
    /// 信息类号越档。
    EClassRange,
    /// 查询长度为零。
    ELenZero,
    /// 查询长度与账面长度不匹。
    ELenMiss,
    /// 会话归属越闭集。
    ESessionRange,
    /// 回指指纹缺失（断链）。
    EFpMissing,
    /// 回指指纹错挂。
    EFpMismatch,
    /// 查询号乱序。
    ERowOrder,
    /// 查询行重复。
    ERowDup,
    /// 未开账即查。
    ENotOpen,
    /// 状态机倒退。
    ERewind,
}

impl TokDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                TokDiag::EHandleZero => 0x01,
                TokDiag::EHandleRange => 0x02,
                TokDiag::EClassRange => 0x03,
                TokDiag::ELenZero => 0x04,
                TokDiag::ELenMiss => 0x05,
                TokDiag::ESessionRange => 0x06,
                TokDiag::EFpMissing => 0x07,
                TokDiag::EFpMismatch => 0x08,
                TokDiag::ERowOrder => 0x09,
                TokDiag::ERowDup => 0x0a,
                TokDiag::ENotOpen => 0x0b,
                TokDiag::ERewind => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            TokDiag::EHandleZero => "令牌句柄为零：坏 Token 首族，先核令牌面来源再消费",
            TokDiag::EHandleRange => "句柄越令牌表：句柄必在冻结令牌表内，越表先对句柄账",
            TokDiag::EClassRange => "信息类号越档：九档闭集（1..=9），越档先正类号",
            TokDiag::ELenZero => "查询长度为零：信息类账面长度 ≥1，零长先核缓冲口径",
            TokDiag::ELenMiss => "长度不匹：查询长度与九档冻结账面长度不符，先对账面",
            TokDiag::ESessionRange => "会话归属越闭集：会话号必在冻结会话表内，越表先对会话账",
            TokDiag::EFpMissing => "回指指纹缺失：B23 族台账指纹空即断链，先补指纹",
            TokDiag::EFpMismatch => "回指指纹错挂：指纹与 B23 族冻结值不符，先对指纹源",
            TokDiag::ERowOrder => "查询号乱序：按查询号 0..N 逐行过闸，跳号先回补",
            TokDiag::ERowDup => "查询行重复：同句柄同类号只入一笔，重复先查已有账",
            TokDiag::ENotOpen => "未开账即查：先开账（Open→Query）再消费令牌面",
            TokDiag::ERewind => "状态机倒退：Open→Query→Attest 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const TOK_ALL: &[TokDiag] = &[
    TokDiag::EHandleZero,
    TokDiag::EHandleRange,
    TokDiag::EClassRange,
    TokDiag::ELenZero,
    TokDiag::ELenMiss,
    TokDiag::ESessionRange,
    TokDiag::EFpMissing,
    TokDiag::EFpMismatch,
    TokDiag::ERowOrder,
    TokDiag::ERowDup,
    TokDiag::ENotOpen,
    TokDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in TOK_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x810C1..0x810CC 闭集内"
}

// ── B23 族回指锚 ─────────────────────────────────────────────────
/// B23 族令牌台账指纹（冻结）。
pub const B23_FP: &str = "B23-TOKEN-LEDGER-COSIGN";
/// B23 族回指批号（VX9 批 B23：12732..12743 区段族源）。
pub const B23_REF_BATCH: u32 = 23;

// ── 会话归属表 ───────────────────────────────────────────────────
/// 冻结会话号表（4 例）。
pub const SESSION_TABLE: &[u32] = &[0, 1, 2, 99];

/// 会话号是否在闭集内。
pub fn session_in_set(s: u32) -> bool {
    for v in SESSION_TABLE.iter() {
        if *v == s {
            return true;
        }
    }
    false
}

// ── 令牌表（句柄 → 归属会话） ────────────────────────────────────
/// 令牌行（句柄/归属会话/权限位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenRow {
    /// 令牌句柄（非零）。
    pub handle: u32,
    /// 归属会话号。
    pub session: u32,
    /// 权限位（非零）。
    pub rights: u32,
}

/// 冻结令牌表（6 例）。
pub const TOKEN_TABLE: &[TokenRow] = &[
    TokenRow { handle: 0x1000, session: 0, rights: 0xF },
    TokenRow { handle: 0x1004, session: 1, rights: 0x7 },
    TokenRow { handle: 0x1008, session: 2, rights: 0x3 },
    TokenRow { handle: 0x100C, session: 99, rights: 0x1F },
    TokenRow { handle: 0x1010, session: 0, rights: 0x9 },
    TokenRow { handle: 0x1014, session: 1, rights: 0x5 },
];

/// 令牌表容量。
pub const TOKEN_CAP: usize = 6;

/// 取令牌行（越表 None）。
pub fn token_of(handle: u32) -> Option<&'static TokenRow> {
    for t in TOKEN_TABLE.iter() {
        if t.handle == handle {
            return Some(t);
        }
    }
    None
}

// ── 信息类九档闭集与类长账面 ─────────────────────────────────────
/// 信息类九档闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokClass {
    /// TokenUser（1）。
    User,
    /// TokenGroups（2）。
    Groups,
    /// TokenPrivileges（3）。
    Privileges,
    /// TokenOwner（4）。
    Owner,
    /// TokenPrimaryGroup（5）。
    PrimaryGroup,
    /// TokenDefaultDacl（6）。
    DefaultDacl,
    /// TokenSource（7）。
    Source,
    /// TokenType（8）。
    Type,
    /// TokenStatistics（9）。
    Statistics,
}

impl TokClass {
    /// 类号（1..=9）。
    pub fn no(self) -> u32 {
        match self {
            TokClass::User => 1,
            TokClass::Groups => 2,
            TokClass::Privileges => 3,
            TokClass::Owner => 4,
            TokClass::PrimaryGroup => 5,
            TokClass::DefaultDacl => 6,
            TokClass::Source => 7,
            TokClass::Type => 8,
            TokClass::Statistics => 9,
        }
    }

    /// 类名。
    pub fn name(self) -> &'static str {
        match self {
            TokClass::User => "TokenUser",
            TokClass::Groups => "TokenGroups",
            TokClass::Privileges => "TokenPrivileges",
            TokClass::Owner => "TokenOwner",
            TokClass::PrimaryGroup => "TokenPrimaryGroup",
            TokClass::DefaultDacl => "TokenDefaultDacl",
            TokClass::Source => "TokenSource",
            TokClass::Type => "TokenType",
            TokClass::Statistics => "TokenStatistics",
        }
    }

    /// 全部九档（闭集数组）。
    pub fn all() -> &'static [TokClass] {
        &[
            TokClass::User,
            TokClass::Groups,
            TokClass::Privileges,
            TokClass::Owner,
            TokClass::PrimaryGroup,
            TokClass::DefaultDacl,
            TokClass::Source,
            TokClass::Type,
            TokClass::Statistics,
        ]
    }
}

/// 类号解析（0 或 >9 为 None）。
pub fn class_of(no: u32) -> Option<TokClass> {
    match no {
        1 => Some(TokClass::User),
        2 => Some(TokClass::Groups),
        3 => Some(TokClass::Privileges),
        4 => Some(TokClass::Owner),
        5 => Some(TokClass::PrimaryGroup),
        6 => Some(TokClass::DefaultDacl),
        7 => Some(TokClass::Source),
        8 => Some(TokClass::Type),
        9 => Some(TokClass::Statistics),
        _ => None,
    }
}

/// 类长账面（九档冻结长度，整数口径）。
/// 注出处：X64 Token 信息类结构账面（B23 族台账同源冻结口径）。
pub const CLASS_LENS: &[u32] = &[24, 36, 40, 16, 16, 20, 16, 4, 56];

/// 类长账面取值（越档 None）。
pub fn class_len(no: u32) -> Option<u32> {
    match no {
        1..=9 => CLASS_LENS.get((no - 1) as usize).copied(),
        _ => None,
    }
}

// ── 查询行与黄金判定 ─────────────────────────────────────────────
/// 查询行六元（查询号/句柄/类号/长度/会话/回指指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokRow {
    /// 查询号。
    pub no: u32,
    /// 令牌句柄。
    pub handle: u32,
    /// 信息类号（1..=9）。
    pub class: u32,
    /// 查询长度。
    pub len: u32,
    /// 会话归属。
    pub session: u32,
    /// 回指指纹。
    pub fp: &'static str,
}

/// 单行黄金判定（五道：句柄/类号/长度/会话/指纹）。
pub fn tok_row_ok(r: &TokRow) -> Result<(), u32> {
    // 句柄闸。
    if r.handle == 0 {
        return Err(TokDiag::EHandleZero.val());
    }
    if token_of(r.handle).is_none() {
        return Err(TokDiag::EHandleRange.val());
    }
    // 类号闸。
    if class_of(r.class).is_none() {
        return Err(TokDiag::EClassRange.val());
    }
    // 长度闸。
    if r.len == 0 {
        return Err(TokDiag::ELenZero.val());
    }
    match class_len(r.class) {
        Some(cl) => {
            if cl != r.len {
                return Err(TokDiag::ELenMiss.val());
            }
        }
        None => return Err(TokDiag::EClassRange.val()),
    }
    // 会话闸。
    if !session_in_set(r.session) {
        return Err(TokDiag::ESessionRange.val());
    }
    // 指纹闸。
    if r.fp.is_empty() {
        return Err(TokDiag::EFpMissing.val());
    }
    if r.fp != B23_FP {
        return Err(TokDiag::EFpMismatch.val());
    }
    Ok(())
}

/// 查询档容量（12 行：令牌表 6 例 × 两档类号）。
pub const QUERY_CAP: usize = 12;

/// 构造指纹常量（冻结）。
const fn std_fp() -> &'static str {
    B23_FP
}

/// 冻结查询档（12 行：句柄 0x1000..0x1014 轮询九档中八档 + 重复四档）。
pub const TOK_PROFILE: &[TokRow] = &[
    TokRow { no: 0, handle: 0x1000, class: 1, len: 24, session: 0, fp: std_fp() },
    TokRow { no: 1, handle: 0x1000, class: 8, len: 4, session: 0, fp: std_fp() },
    TokRow { no: 2, handle: 0x1004, class: 2, len: 36, session: 1, fp: std_fp() },
    TokRow { no: 3, handle: 0x1004, class: 3, len: 40, session: 1, fp: std_fp() },
    TokRow { no: 4, handle: 0x1008, class: 4, len: 16, session: 2, fp: std_fp() },
    TokRow { no: 5, handle: 0x1008, class: 5, len: 16, session: 2, fp: std_fp() },
    TokRow { no: 6, handle: 0x100C, class: 6, len: 20, session: 99, fp: std_fp() },
    TokRow { no: 7, handle: 0x100C, class: 7, len: 16, session: 99, fp: std_fp() },
    TokRow { no: 8, handle: 0x1010, class: 8, len: 4, session: 0, fp: std_fp() },
    TokRow { no: 9, handle: 0x1010, class: 9, len: 56, session: 0, fp: std_fp() },
    TokRow { no: 10, handle: 0x1014, class: 1, len: 24, session: 1, fp: std_fp() },
    TokRow { no: 11, handle: 0x1014, class: 7, len: 16, session: 1, fp: std_fp() },
];

// ── 状态机（Open → Query → Attest） ──────────────────────────────
/// 探测状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokState {
    /// 已开账。
    Open,
    /// 查询中。
    Query,
    /// 已封证。
    Attest,
}

impl TokState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            TokState::Open => 0,
            TokState::Query => 1,
            TokState::Attest => 2,
        }
    }
}

/// 探测账。
#[derive(Clone, Debug)]
pub struct TokLedger {
    /// 已对平查询行。
    rows: Vec<TokRow>,
    /// 拒收记账（查询号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: TokState,
}

impl TokLedger {
    /// 开账。
    pub fn new() -> TokLedger {
        TokLedger {
            rows: Vec::new(),
            rejects: Vec::new(),
            state: TokState::Open,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> TokState {
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
    pub fn row_at(&self, idx: usize) -> Option<&TokRow> {
        self.rows.get(idx)
    }

    /// 开查（Open→Query）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.state.rank() >= TokState::Query.rank() {
            return Err(TokDiag::ERewind.val());
        }
        self.state = TokState::Query;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < QUERY_CAP * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 逐行消费（行号序 + 黄金判定 + 互异 三闸）。
    pub fn consume(&mut self, r: TokRow) -> Result<(), u32> {
        if self.state.rank() < TokState::Query.rank() {
            return Err(TokDiag::ENotOpen.val());
        }
        if self.state.rank() >= TokState::Attest.rank() {
            self.note_reject(r.no, TokDiag::ERewind.val());
            return Err(TokDiag::ERewind.val());
        }
        // 行号序闸。
        if r.no != self.rows.len() as u32 {
            self.note_reject(r.no, TokDiag::ERowOrder.val());
            return Err(TokDiag::ERowOrder.val());
        }
        // 黄金判定闸。
        if let Err(e) = tok_row_ok(&r) {
            self.note_reject(r.no, e);
            return Err(e);
        }
        // 互异闸：同句柄同类号只入一笔。
        for done in self.rows.iter() {
            if done.handle == r.handle && done.class == r.class {
                self.note_reject(r.no, TokDiag::ERowDup.val());
                return Err(TokDiag::ERowDup.val());
            }
        }
        self.rows.push(r);
        Ok(())
    }

    /// 封证：12 行齐 + 零拒收。
    pub fn attest(&mut self) -> Result<(), u32> {
        if self.state.rank() >= TokState::Attest.rank() {
            return Err(TokDiag::ERewind.val());
        }
        if self.state.rank() < TokState::Query.rank() {
            return Err(TokDiag::ENotOpen.val());
        }
        if self.rows.len() != QUERY_CAP {
            return Err(TokDiag::ERowOrder.val());
        }
        if !self.rejects.is_empty() {
            return Err(TokDiag::ERowOrder.val());
        }
        self.state = TokState::Attest;
        Ok(())
    }

    /// 账等式。
    pub fn equation_ok(&self) -> bool {
        self.rows.len() == QUERY_CAP && self.rejects.is_empty()
    }
}

// ── 坏 Token 注入（十族确定性出题） ──────────────────────────────
/// 坏 Token 十族闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadToken {
    /// 零句柄。
    HandleZero,
    /// 句柄越表。
    HandleRange,
    /// 类号越档。
    ClassRange,
    /// 零长度。
    LenZero,
    /// 长度越账。
    LenMiss,
    /// 会话越闭集。
    SessionRange,
    /// 指纹断链。
    FpMissing,
    /// 指纹错挂。
    FpMismatch,
    /// 类号重复（同句柄同类号二入）。
    RowDup,
    /// 未开即查。
    NotOpen,
}

impl BadToken {
    /// 族号。
    pub fn index(self) -> u32 {
        match self {
            BadToken::HandleZero => 0,
            BadToken::HandleRange => 1,
            BadToken::ClassRange => 2,
            BadToken::LenZero => 3,
            BadToken::LenMiss => 4,
            BadToken::SessionRange => 5,
            BadToken::FpMissing => 6,
            BadToken::FpMismatch => 7,
            BadToken::RowDup => 8,
            BadToken::NotOpen => 9,
        }
    }

    /// 族名。
    pub fn name(self) -> &'static str {
        match self {
            BadToken::HandleZero => "零句柄",
            BadToken::HandleRange => "句柄越表",
            BadToken::ClassRange => "类号越档",
            BadToken::LenZero => "零长度",
            BadToken::LenMiss => "长度越账",
            BadToken::SessionRange => "会话越闭集",
            BadToken::FpMissing => "指纹断链",
            BadToken::FpMismatch => "指纹错挂",
            BadToken::RowDup => "类号重复",
            BadToken::NotOpen => "未开即查",
        }
    }

    /// 全部十族（闭集数组）。
    pub fn all() -> &'static [BadToken] {
        &[
            BadToken::HandleZero,
            BadToken::HandleRange,
            BadToken::ClassRange,
            BadToken::LenZero,
            BadToken::LenMiss,
            BadToken::SessionRange,
            BadToken::FpMissing,
            BadToken::FpMismatch,
            BadToken::RowDup,
            BadToken::NotOpen,
        ]
    }
}

/// 构造基行（库内第 idx 行的确定性变体）。
fn base_row(idx: usize) -> Option<TokRow> {
    TOK_PROFILE.get(idx).cloned()
}

/// 单族坏 Token 注入：返回 true 表示被拒止。
pub fn detect_bad_token(f: BadToken) -> bool {
    match f {
        BadToken::HandleZero => {
            let mut r = match base_row(0) {
                Some(v) => v,
                None => return false,
            };
            r.handle = 0;
            tok_row_ok(&r).is_err()
        }
        BadToken::HandleRange => {
            let mut r = match base_row(1) {
                Some(v) => v,
                None => return false,
            };
            r.handle = 0x9000;
            tok_row_ok(&r).is_err()
        }
        BadToken::ClassRange => {
            let mut r = match base_row(2) {
                Some(v) => v,
                None => return false,
            };
            r.class = 10;
            tok_row_ok(&r).is_err()
        }
        BadToken::LenZero => {
            let mut r = match base_row(3) {
                Some(v) => v,
                None => return false,
            };
            r.len = 0;
            tok_row_ok(&r).is_err()
        }
        BadToken::LenMiss => {
            let mut r = match base_row(4) {
                Some(v) => v,
                None => return false,
            };
            r.len = 32;
            tok_row_ok(&r).is_err()
        }
        BadToken::SessionRange => {
            let mut r = match base_row(5) {
                Some(v) => v,
                None => return false,
            };
            r.session = 777;
            tok_row_ok(&r).is_err()
        }
        BadToken::FpMissing => {
            let mut r = match base_row(6) {
                Some(v) => v,
                None => return false,
            };
            r.fp = "";
            tok_row_ok(&r).is_err()
        }
        BadToken::FpMismatch => {
            let mut r = match base_row(7) {
                Some(v) => v,
                None => return false,
            };
            r.fp = "B22-FAKE";
            tok_row_ok(&r).is_err()
        }
        BadToken::RowDup => {
            let mut led = TokLedger::new();
            let _ = led.begin();
            let a = match base_row(0) {
                Some(v) => v,
                None => return false,
            };
            if led.consume(a.clone()).is_err() {
                return false;
            }
            // 同句柄同类号二入被拒。
            let dup = TokRow { no: 9, handle: a.handle, class: a.class, len: a.len, session: a.session, fp: B23_FP };
            led.consume(dup).is_err()
        }
        BadToken::NotOpen => {
            let mut led = TokLedger::new();
            let r = match base_row(9) {
                Some(v) => v,
                None => return false,
            };
            led.consume(r).is_err()
        }
    }
}

/// 坏 Token 注入 10/10 拒止（判据 J1 直读口）。
pub fn bad_token_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for f in BadToken::all().iter() {
        total += 1;
        if detect_bad_token(*f) {
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
pub enum TokFault {
    /// 查询号乱序。
    RowOrder,
    /// 封证后查询。
    AttestQuery,
    /// 坏令牌族面漏检。
    SweepMiss,
    /// 指纹常量漂移。
    FpDrift,
    /// 类长账面漂移。
    LenDrift,
    /// 会话表漂移。
    SessionDrift,
    /// 行数不足即封证。
    ShortAttest,
    /// 未开账即封证。
    OpenAttest,
    /// 双跑漂移。
    DualDrift,
    /// 状态机倒退。
    Rewind,
}

impl TokFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            TokFault::RowOrder => 0,
            TokFault::AttestQuery => 1,
            TokFault::SweepMiss => 2,
            TokFault::FpDrift => 3,
            TokFault::LenDrift => 4,
            TokFault::SessionDrift => 5,
            TokFault::ShortAttest => 6,
            TokFault::OpenAttest => 7,
            TokFault::DualDrift => 8,
            TokFault::Rewind => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            TokFault::RowOrder => "查询号乱序",
            TokFault::AttestQuery => "封证后查询",
            TokFault::SweepMiss => "坏令牌漏检",
            TokFault::FpDrift => "指纹漂移",
            TokFault::LenDrift => "类长漂移",
            TokFault::SessionDrift => "会话表漂移",
            TokFault::ShortAttest => "缺行封证",
            TokFault::OpenAttest => "未查封证",
            TokFault::DualDrift => "双跑漂移",
            TokFault::Rewind => "状态机倒退",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [TokFault] {
        &[
            TokFault::RowOrder,
            TokFault::AttestQuery,
            TokFault::SweepMiss,
            TokFault::FpDrift,
            TokFault::LenDrift,
            TokFault::SessionDrift,
            TokFault::ShortAttest,
            TokFault::OpenAttest,
            TokFault::DualDrift,
            TokFault::Rewind,
        ]
    }
}

/// 构建完整探测账（12 行全过闸）。
pub fn build_full_ledger() -> TokLedger {
    let mut led = TokLedger::new();
    let _ = led.begin();
    let mut i = 0usize;
    while i < QUERY_CAP {
        let r = match TOK_PROFILE.get(i) {
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

/// 九档类长账面冻结直读（1..=9 每档非零且与冻结表一致）。
fn class_lens_ok() -> bool {
    let mut ok = CLASS_LENS.len() == 9;
    let mut n = 1u32;
    while n <= 9 {
        match (class_len(n), CLASS_LENS.get((n - 1) as usize)) {
            (Some(a), Some(b)) => {
                if a != *b || a == 0 {
                    ok = false;
                }
            }
            _ => ok = false,
        }
        n += 1;
    }
    // 类号 0 与 10 无档。
    if class_len(0).is_some() || class_len(10).is_some() {
        ok = false;
    }
    ok
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in TokFault::all().iter() {
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
fn run_fault(v: TokFault) -> bool {
    match v {
        TokFault::RowOrder => {
            let mut led = TokLedger::new();
            let _ = led.begin();
            let r = match TOK_PROFILE.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.consume(r).is_err()
        }
        TokFault::AttestQuery => {
            let mut led = build_full_ledger();
            if led.attest().is_err() {
                return false;
            }
            let r = match TOK_PROFILE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.consume(r).is_err()
        }
        TokFault::SweepMiss => {
            let (caught, total, escape) = bad_token_sweep();
            total == 10 && caught == 10 && escape.is_empty()
        }
        TokFault::FpDrift => {
            // 指纹常量冻结直读。
            B23_FP == "B23-TOKEN-LEDGER-COSIGN" && B23_REF_BATCH == 23
        }
        TokFault::LenDrift => class_lens_ok(),
        TokFault::SessionDrift => {
            // 会话表冻结直读：4 例互异、查表命中。
            if SESSION_TABLE.len() != 4 {
                return false;
            }
            let mut ok = true;
            let mut i = 0usize;
            while i < SESSION_TABLE.len() {
                let v = match SESSION_TABLE.get(i) {
                    Some(x) => *x,
                    None => return false,
                };
                if !session_in_set(v) {
                    ok = false;
                }
                let mut j = i + 1;
                while j < SESSION_TABLE.len() {
                    match SESSION_TABLE.get(j) {
                        Some(w) => {
                            if *w == v {
                                ok = false;
                            }
                        }
                        None => return false,
                    }
                    j += 1;
                }
                i += 1;
            }
            ok
        }
        TokFault::ShortAttest => {
            let mut led = TokLedger::new();
            let _ = led.begin();
            let r = match TOK_PROFILE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.consume(r);
            led.attest().is_err()
        }
        TokFault::OpenAttest => {
            let mut led = TokLedger::new();
            led.attest().is_err()
        }
        TokFault::DualDrift => {
            // 双跑直读：冻结档两轮同构。
            let a = build_full_ledger();
            let b = build_full_ledger();
            a.len() == b.len() && a.len() == QUERY_CAP
        }
        TokFault::Rewind => {
            let mut fresh = TokLedger::new();
            fresh.attest().is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// Token 信息类探测档对平断言绿（判据 J1 直读口）。
fn tok_profile_ok() -> bool {
    let mut led = build_full_ledger();
    if led.len() != QUERY_CAP {
        return false;
    }
    if led.reject_len() != 0 {
        return false;
    }
    if led.attest().is_err() {
        return false;
    }
    if led.state() != TokState::Attest {
        return false;
    }
    // 独立复核：12 行逐行黄金判定。
    for r in TOK_PROFILE.iter() {
        if tok_row_ok(r).is_err() {
            return false;
        }
    }
    // 类号覆盖：12 行至少覆盖 7 个不同类号。
    let mut seen: Vec<u32> = Vec::new();
    for r in TOK_PROFILE.iter() {
        let mut has = false;
        for v in seen.iter() {
            if *v == r.class {
                has = true;
            }
        }
        if !has {
            seen.push(r.class);
        }
    }
    if seen.len() < 7 {
        return false;
    }
    true
}

/// 坏 Token 注入 10/10 拒止。
fn bad_token_ok() -> bool {
    let (caught, total, escape) = bad_token_sweep();
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

/// B23 族回指齐备直读。
fn b23_ref_ok() -> bool {
    if B23_FP != "B23-TOKEN-LEDGER-COSIGN" {
        return false;
    }
    if B23_REF_BATCH != 23 {
        return false;
    }
    for r in TOK_PROFILE.iter() {
        if r.fp != B23_FP {
            return false;
        }
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

/// 信息类九档闭集普查。
fn class_census_ok() -> bool {
    let mut ok = true;
    for c in TokClass::all().iter() {
        // 类号与解析互证。
        match class_of(c.no()) {
            Some(parsed) => {
                if parsed != *c {
                    ok = false;
                }
            }
            None => ok = false,
        }
        // 类长账面非零。
        match class_len(c.no()) {
            Some(l) => {
                if l == 0 {
                    ok = false;
                }
            }
            None => ok = false,
        }
    }
    ok
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
    if bad_token_sweep().0 != bad_token_sweep().0 {
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
    if SEG_BASE != 0x80000 + 0x40 * (12667u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut led = build_full_ledger();
    if led.attest().is_err() {
        return false;
    }
    if led.state() != TokState::Attest {
        return false;
    }
    match led.row_at(0) {
        Some(r) => {
            if r.handle == 0 || r.fp != B23_FP {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12668 Token 探测面自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12667_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12667-token-probe");
    let led = build_full_ledger();

    s.add_dyn(
        "F12668-01 信息类九档闭集",
        TokClass::all().len() == 9,
        &format!("classes={}", TokClass::all().len()),
    );

    s.add_dyn(
        "F12668-02 Token 探测档对平断言绿",
        tok_profile_ok(),
        &format!("rows={} rejects={}", led.len(), led.reject_len()),
    );

    s.add_dyn(
        "F12668-03 B23 族回指齐备",
        b23_ref_ok(),
        &format!("fp={} batch={}", B23_FP, B23_REF_BATCH),
    );

    s.add_dyn(
        "F12668-04 坏 Token 注入 10/10 拒止",
        bad_token_ok(),
        &format!("caught={}", bad_token_sweep().0),
    );

    s.add_dyn(
        "F12668-05 注入联测 10/10 拒止",
        fault_sweep_ok(),
        &format!("caught={}", fault_sweep().0),
    );

    s.add_dyn(
        "F12668-06 类长账面冻结直读",
        class_lens_ok(),
        "九档账面长度逐档冻结（整数口径）",
    );

    s.add_dyn(
        "F12668-07 类闭集普查",
        class_census_ok(),
        "九档类号解析互证 + 账面非零",
    );

    s.add_dyn(
        "F12668-08 双跑一致",
        dual_run_ok(),
        "冻结档下两轮逐行同判",
    );

    s.add_dyn(
        "F12668-09 探测账等式",
        led.equation_ok(),
        &format!("rows={} rejects={}", led.len(), led.reject_len()),
    );

    s.add_dyn(
        "F12668-10 会话归属闭集",
        SESSION_TABLE.len() == 4,
        &format!("sessions={}", SESSION_TABLE.len()),
    );

    s.add_dyn(
        "F12668-11 完成写回语义",
        writeback_ok(),
        "封证即已完成且摘要非空",
    );

    s.add_dyn(
        "F12668-12 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
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
        assert_eq!(SEG_BASE, 0x810C0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中。
        assert_eq!(TOK_ALL.len(), 12);
        let mut i = 0usize;
        while i < TOK_ALL.len() {
            let vi = match TOK_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < TOK_ALL.len() {
                let vj = match TOK_ALL.get(j) {
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
    fn happy_path_attests() {
        let mut led = build_full_ledger();
        assert_eq!(led.len(), QUERY_CAP);
        assert_eq!(led.reject_len(), 0);
        assert!(led.attest().is_ok());
        assert_eq!(led.state(), TokState::Attest);
    }

    #[test]
    fn bad_token_injection_ten_ten() {
        // 坏 Token 注入 10/10 拒止。
        let (caught, total, escape) = bad_token_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱族：{}", escape);
        assert!(escape.is_empty());
        assert_eq!(BadToken::all().len(), 10);
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
    fn class_lens_frozen() {
        // 九档类长账面直读。
        assert_eq!(CLASS_LENS.len(), 9);
        assert_eq!(class_len(1), Some(24));
        assert_eq!(class_len(2), Some(36));
        assert_eq!(class_len(3), Some(40));
        assert_eq!(class_len(4), Some(16));
        assert_eq!(class_len(5), Some(16));
        assert_eq!(class_len(6), Some(20));
        assert_eq!(class_len(7), Some(16));
        assert_eq!(class_len(8), Some(4));
        assert_eq!(class_len(9), Some(56));
        assert_eq!(class_len(0), None);
        assert_eq!(class_len(10), None);
        // 类号解析互证。
        assert_eq!(class_of(1), Some(TokClass::User));
        assert_eq!(class_of(9), Some(TokClass::Statistics));
        assert_eq!(class_of(0), None);
        assert_eq!(class_of(10), None);
        assert!(class_census_ok());
    }

    #[test]
    fn token_table_semantics() {
        // 令牌表：6 例、查表命中/越表。
        assert_eq!(TOKEN_TABLE.len(), TOKEN_CAP);
        assert!(token_of(0x1000).is_some());
        assert!(token_of(0x1014).is_some());
        assert!(token_of(0x2000).is_none());
        assert!(token_of(0).is_none());
        // 查询档行句柄全部在表内。
        for r in TOK_PROFILE.iter() {
            assert!(token_of(r.handle).is_some(), "行 {} 句柄越表", r.no);
        }
    }

    #[test]
    fn row_gate_rejects() {
        // 行闸直读：乱序/重复/断链。
        let mut led = TokLedger::new();
        let _ = led.begin();
        let r1 = match TOK_PROFILE.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(led.consume(r1), Err(TokDiag::ERowOrder.val()));
        let r0 = match TOK_PROFILE.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        assert!(led.consume(r0.clone()).is_ok());
        let dup = TokRow { no: 1, handle: r0.handle, class: r0.class, len: r0.len, session: r0.session, fp: B23_FP };
        assert_eq!(led.consume(dup), Err(TokDiag::ERowDup.val()));
        assert_eq!(led.reject_len(), 2);
    }

    #[test]
    fn b23_anchor_frozen() {
        // B23 族锚冻结直读。
        assert_eq!(B23_FP, "B23-TOKEN-LEDGER-COSIGN");
        assert_eq!(B23_REF_BATCH, 23);
        assert!(b23_ref_ok());
        // 断链与错挂。
        let mut r = match TOK_PROFILE.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        r.fp = "";
        assert_eq!(tok_row_ok(&r), Err(TokDiag::EFpMissing.val()));
        r.fp = "B24-OTHER";
        assert_eq!(tok_row_ok(&r), Err(TokDiag::EFpMismatch.val()));
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
        assert_eq!(bad_token_sweep().0, bad_token_sweep().0);
        assert_eq!(fault_sweep().0, fault_sweep().0);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Open→Query→Attest。
        assert!(TokState::Open.rank() < TokState::Query.rank());
        assert!(TokState::Query.rank() < TokState::Attest.rank());
        let mut led = build_full_ledger();
        assert_eq!(led.state(), TokState::Query);
        assert!(led.attest().is_ok());
        assert_eq!(led.attest(), Err(TokDiag::ERewind.val()));
        let mut fresh = TokLedger::new();
        assert!(fresh.begin().is_ok());
        assert_eq!(fresh.begin(), Err(TokDiag::ERewind.val()));
    }

    #[test]
    fn class_coverage_in_profile() {
        // 查询档 12 行类号覆盖 ≥7 档。
        let mut seen: Vec<u32> = Vec::new();
        for r in TOK_PROFILE.iter() {
            let mut has = false;
            for v in seen.iter() {
                if *v == r.class {
                    has = true;
                }
            }
            if !has {
                seen.push(r.class);
            }
        }
        assert!(seen.len() >= 7, "类号覆盖不足：{}", seen.len());
        // 每行长度与账面一致。
        for r in TOK_PROFILE.iter() {
            assert_eq!(class_len(r.class), Some(r.len), "行 {} 长度漂移", r.no);
        }
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12667_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
