//! UNX-F12768 语义分界声明核验（D1–D3 分界/D4·E1 预演） · 0x82A00段
//!
//! 判据原文（派工卡 12768 逐字锚定）：
//!   UNX-F12769-J1 术语一致性核验绿（核心术语 ≥40 条全册统一），漂移演练 10/10 检出。
//!
//! 设计要点：
//! 1. D1/D2/D3 三分界声明闭集：每界为（名称/管辖号段/豁免清单）三元组，
//!    号段闭集冻结——D1 与 D2 之间留受控重叠带 [0x82A40,0x82A60) 专供
//!    EOVERLAP 语义码演练；D1/D2 各留一条界内豁免段专供落空演练；
//! 2. 号段归属唯一裁决 judge_boundary(kind, seg)：判定次序冻结为
//!    先重叠（多界命中 → EOVERLAP）→ 再落空（含豁免段 → ESRCH）→
//!    后归属（唯一命中且申请界一致 → Ok(归属界)；申请界不符 → 越权码）；
//! 3. D4/E1 预演面：分界预演须先激活（未激活 → EPERM，封存后再演 →
//!    ESEALED），预演行（界别/起段/段数/指纹）可复算——重放裁决与
//!    术语面折叠指纹逐位吻合，不符即 ERECOMPUTE；
//! 4. 漂移注入器三类各 10 轮全抓：术语漂移（重名/空名/号漂移）、
//!    分界越权（相邻界申请同段）、预演未激活——注入必须被对应判定器
//!    抓住，逃脱即判据失效；
//! 5. 双跑一致指纹：全流程裁决/预演/注入结果按 FNV-1a 折叠，两次折叠
//!    逐位一致（LCG 种子写死，零随机红线合规）；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟；取值一律
//!    match/if let/.get()/.get_mut()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12768 − 12600) = 0x82A00）。
pub const SEG_BASE: u32 = 0x82A00;

// ── 语义分界诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0D 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SbvDiag {
    /// 号段落空：申请段不在任何分界管辖范围（含豁免段）。
    ESrch,
    /// 多界重叠：申请段同时落在两个及以上分界管辖号段内。
    EOverlap,
    /// 分界越权：号段唯一归属某界，但申请界与之不符。
    EOverreach,
    /// 预演未激活：分界未走激活口就请求预演。
    EPerm,
    /// 预演已封存：封存后的分界不再受理预演。
    ESealed,
    /// 状态机倒退：激活/封存只进不退，倒退即预演面可疑。
    ERewind,
    /// 预演账本容量耗尽：行槽已满，再录被拒。
    ECap,
    /// 术语漂移：术语正名重复或条数不足全册闭集。
    EDrift,
    /// 术语号失序：术语号非严格递增或首号越界。
    EDupTerm,
    /// 术语空名：术语正名为空串，不构成词条。
    ETermEmpty,
    /// 号段声明非法：三元组起止倒挂、未 0x20 对齐或豁免越界。
    ERange,
    /// 预演行复算不符：重放指纹与行内指纹不吻合。
    ERecompute,
    /// 双跑指纹不一致：同一流程两次折叠结果有差。
    EFingerprint,
}

impl SbvDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0D 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                SbvDiag::ESrch => 0x01,
                SbvDiag::EOverlap => 0x02,
                SbvDiag::EOverreach => 0x03,
                SbvDiag::EPerm => 0x04,
                SbvDiag::ESealed => 0x05,
                SbvDiag::ERewind => 0x06,
                SbvDiag::ECap => 0x07,
                SbvDiag::EDrift => 0x08,
                SbvDiag::EDupTerm => 0x09,
                SbvDiag::ETermEmpty => 0x0a,
                SbvDiag::ERange => 0x0b,
                SbvDiag::ERecompute => 0x0c,
                SbvDiag::EFingerprint => 0x0d,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            SbvDiag::ESrch => "号段落空：段不在 D1/D2/D3 管辖内或属豁免段，先对三元组表再申请",
            SbvDiag::EOverlap => "多界重叠：段落在重叠带，先收敛分界边界消除双管后再归属",
            SbvDiag::EOverreach => "分界越权：申请界与唯一归属界不符，改按归属界提交",
            SbvDiag::EPerm => "预演未激活：先走激活口把分界置 Active 再请求预演",
            SbvDiag::ESealed => "预演已封存：封存后不受理预演，改动走下一轮滚动",
            SbvDiag::ERewind => "状态机倒退：未激活→激活→封存只进不退，倒退即面可疑",
            SbvDiag::ECap => "预演账本容量耗尽：行槽已满，先结算旧行再录新行",
            SbvDiag::EDrift => "术语漂移：正名重复或条数不足，按全册闭集回补统一",
            SbvDiag::EDupTerm => "术语号失序：术语号须从 1 起严格递增，先重排再核",
            SbvDiag::ETermEmpty => "术语空名：正名为空不构成词条，先补正名再入表",
            SbvDiag::ERange => "号段声明非法：起止倒挂/未 0x20 对齐/豁免越界，先修三元组",
            SbvDiag::ERecompute => "预演行复算不符：重放指纹与行内不符，行已失真禁止采信",
            SbvDiag::EFingerprint => "双跑指纹不一致：确定性被破坏，全流程回放排查",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const SBV_ALL: &[SbvDiag] = &[
    SbvDiag::ESrch,
    SbvDiag::EOverlap,
    SbvDiag::EOverreach,
    SbvDiag::EPerm,
    SbvDiag::ESealed,
    SbvDiag::ERewind,
    SbvDiag::ECap,
    SbvDiag::EDrift,
    SbvDiag::EDupTerm,
    SbvDiag::ETermEmpty,
    SbvDiag::ERange,
    SbvDiag::ERecompute,
    SbvDiag::EFingerprint,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in SBV_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82A01..0x82A0D 闭集内"
}

// ── 分界声明闭集（D1/D2/D3 三元组） ──────────────────────────────
/// 分界别（D1–D3 闭集，预演对象 D4/E1 另列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    /// D1：内核核心分界。
    D1,
    /// D2：驱动加载分界。
    D2,
    /// D3：用户态接口分界。
    D3,
}

impl Boundary {
    /// 界号（闭集序，0..3）。
    pub fn idx(self) -> u32 {
        match self {
            Boundary::D1 => 0,
            Boundary::D2 => 1,
            Boundary::D3 => 2,
        }
    }

    /// 界短名（诊断文本用）。
    pub fn name(self) -> &'static str {
        match self {
            Boundary::D1 => "内核界",
            Boundary::D2 => "驱动界",
            Boundary::D3 => "用户界",
        }
    }

    /// 相邻界（越权注入用：申请错界即取相邻界）。
    pub fn neighbor(self) -> Boundary {
        match self {
            Boundary::D1 => Boundary::D2,
            Boundary::D2 => Boundary::D3,
            Boundary::D3 => Boundary::D1,
        }
    }
}

/// 分界声明三元组：（名称/管辖号段/豁免清单）。
///
/// 管辖号段为 [lo, hi) 左闭右开；豁免清单内的段虽在 [lo, hi) 内但不归
/// 本界管辖（判落空）。D1 与 D2 刻意留受控重叠带 [0x82A40, 0x82A60)，
/// 供 EOVERLAP 语义码演练——重叠是声明闭集的受控样本，不是缺陷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundaryDecl {
    /// 界别。
    pub kind: Boundary,
    /// 分界名称（全册统一正名）。
    pub name: &'static str,
    /// 管辖号段起（含，0x20 半步对齐）。
    pub lo: u32,
    /// 管辖号段止（不含，0x20 半步对齐）。
    pub hi: u32,
    /// 豁免清单（界内不管辖段）。
    pub exemptions: &'static [u32],
}

impl BoundaryDecl {
    /// 段是否归本界实际管辖（在界内且不在豁免清单）。
    pub fn covers(&self, seg: u32) -> bool {
        if seg < self.lo || seg >= self.hi {
            return false;
        }
        !self.exemptions.contains(&seg)
    }
}

/// 三分界声明闭集（判据冻结，不许重排/增删）。
pub const DECL_ALL: &[BoundaryDecl] = &[
    BoundaryDecl {
        kind: Boundary::D1,
        name: "内核核心分界",
        lo: 0x82A00,
        hi: 0x82A60,
        exemptions: &[0x82A20],
    },
    BoundaryDecl {
        kind: Boundary::D2,
        name: "驱动加载分界",
        lo: 0x82A40,
        hi: 0x82AC0,
        exemptions: &[0x82A80],
    },
    BoundaryDecl {
        kind: Boundary::D3,
        name: "用户态接口分界",
        lo: 0x82AC0,
        hi: 0x82B40,
        exemptions: &[],
    },
];

/// 唯一归属探测：段恰被一界管辖时返回该界，重叠/落空返回 None。
fn sole_owner(seg: u32) -> Option<Boundary> {
    let mut hits = 0u32;
    let mut owner: Option<Boundary> = None;
    for d in DECL_ALL.iter() {
        if d.covers(seg) {
            hits += 1;
            owner = Some(d.kind);
        }
    }
    if hits == 1 {
        owner
    } else {
        None
    }
}

/// 号段归属唯一裁决（判定次序冻结：重叠 → 落空 → 归属/越权）。
///
/// - 同时命中多界 → EOVERLAP（先判，即使申请界在命中集合内）；
/// - 落空（无界管辖，含豁免段）→ ESRCH（次判）；
/// - 唯一命中且申请界一致 → Ok(归属界)；
/// - 唯一命中但申请界不符 → 越权码（最后判）。
pub fn judge_boundary(kind: Boundary, seg: u32) -> Result<Boundary, u32> {
    // 第一序：多界命中即重叠，申请界不影响此判。
    let mut hits = 0u32;
    let mut owner: Option<Boundary> = None;
    for d in DECL_ALL.iter() {
        if d.covers(seg) {
            hits += 1;
            owner = Some(d.kind);
        }
    }
    if hits > 1 {
        return Err(SbvDiag::EOverlap.val());
    }
    // 第二序：落空（含豁免段——豁免优先于归属）。
    let own = match owner {
        Some(b) => b,
        None => return Err(SbvDiag::ESrch.val()),
    };
    // 第三序：归属或越权。
    if own != kind {
        return Err(SbvDiag::EOverreach.val());
    }
    Ok(own)
}

/// 分界三元组合法性（名称不空/起止有序/0x20 半步对齐/豁免不越界）。
fn validate_decl(d: &BoundaryDecl) -> Result<(), u32> {
    if d.name.is_empty() {
        return Err(SbvDiag::ERange.val());
    }
    if d.lo >= d.hi {
        return Err(SbvDiag::ERange.val());
    }
    if d.lo % 0x20 != 0 || d.hi % 0x20 != 0 {
        return Err(SbvDiag::ERange.val());
    }
    for x in d.exemptions.iter() {
        if *x < d.lo || *x >= d.hi {
            return Err(SbvDiag::ERange.val());
        }
    }
    Ok(())
}

// ── 核心术语闭集（≥40 条全册统一） ───────────────────────────────
/// 术语类闭集（全册六类，词条按类归档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TermClass {
    /// 分界类：分界声明与号段归属术语。
    Decl,
    /// 裁决类：判定器与判定次序术语。
    Ruling,
    /// 预演类：激活/封存/复算术语。
    Rehearsal,
    /// 词书类：术语表自身术语。
    Lexicon,
    /// 诊断类：诊断码与注入术语。
    Diag,
    /// 状态机类：单调状态与指纹术语。
    Machine,
}

impl TermClass {
    /// 类中文名（诊断文本用）。
    pub fn name(self) -> &'static str {
        match self {
            TermClass::Decl => "分界",
            TermClass::Ruling => "裁决",
            TermClass::Rehearsal => "预演",
            TermClass::Lexicon => "词书",
            TermClass::Diag => "诊断",
            TermClass::Machine => "状态机",
        }
    }
}

/// 单条术语词条：（术语号/正名/术语类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermEntry {
    /// 术语号（全册统一，从 1 起严格递增）。
    pub id: u32,
    /// 术语正名（全册统一，不空不重）。
    pub name: &'static str,
    /// 术语类（闭集归档）。
    pub class: TermClass,
}

/// 核心术语下限（判据原文：核心术语 ≥40 条）。
pub const TERM_MIN: usize = 40;

/// 核心术语闭集（42 条，全册统一正名，冻结不许重排）。
pub const TERM_ALL: &[TermEntry] = &[
    TermEntry { id: 1, name: "语义分界", class: TermClass::Decl },
    TermEntry { id: 2, name: "分界声明", class: TermClass::Decl },
    TermEntry { id: 3, name: "管辖号段", class: TermClass::Decl },
    TermEntry { id: 4, name: "豁免清单", class: TermClass::Decl },
    TermEntry { id: 5, name: "号段归属", class: TermClass::Ruling },
    TermEntry { id: 6, name: "唯一裁决", class: TermClass::Ruling },
    TermEntry { id: 7, name: "重叠裁决", class: TermClass::Ruling },
    TermEntry { id: 8, name: "落空裁决", class: TermClass::Ruling },
    TermEntry { id: 9, name: "越权申请", class: TermClass::Ruling },
    TermEntry { id: 10, name: "申请界", class: TermClass::Ruling },
    TermEntry { id: 11, name: "归属界", class: TermClass::Ruling },
    TermEntry { id: 12, name: "判定次序", class: TermClass::Ruling },
    TermEntry { id: 13, name: "分界三元组", class: TermClass::Decl },
    TermEntry { id: 14, name: "预演行", class: TermClass::Rehearsal },
    TermEntry { id: 15, name: "预演激活", class: TermClass::Rehearsal },
    TermEntry { id: 16, name: "预演封存", class: TermClass::Rehearsal },
    TermEntry { id: 17, name: "复算指纹", class: TermClass::Rehearsal },
    TermEntry { id: 18, name: "预演账本", class: TermClass::Rehearsal },
    TermEntry { id: 19, name: "行容量", class: TermClass::Rehearsal },
    TermEntry { id: 20, name: "术语条目", class: TermClass::Lexicon },
    TermEntry { id: 21, name: "术语一致性", class: TermClass::Lexicon },
    TermEntry { id: 22, name: "核心术语", class: TermClass::Lexicon },
    TermEntry { id: 23, name: "术语漂移", class: TermClass::Lexicon },
    TermEntry { id: 24, name: "别名漂移", class: TermClass::Lexicon },
    TermEntry { id: 25, name: "术语号", class: TermClass::Lexicon },
    TermEntry { id: 26, name: "术语类", class: TermClass::Lexicon },
    TermEntry { id: 27, name: "诊断码", class: TermClass::Diag },
    TermEntry { id: 28, name: "闭集数组", class: TermClass::Diag },
    TermEntry { id: 29, name: "查表回落", class: TermClass::Diag },
    TermEntry { id: 30, name: "语义码", class: TermClass::Diag },
    TermEntry { id: 31, name: "中文建议", class: TermClass::Diag },
    TermEntry { id: 32, name: "注入必抓", class: TermClass::Diag },
    TermEntry { id: 33, name: "状态机", class: TermClass::Machine },
    TermEntry { id: 34, name: "单调推进", class: TermClass::Machine },
    TermEntry { id: 35, name: "倒退拒绝", class: TermClass::Machine },
    TermEntry { id: 36, name: "容量耗尽", class: TermClass::Machine },
    TermEntry { id: 37, name: "段位守卫", class: TermClass::Machine },
    TermEntry { id: 38, name: "双跑一致", class: TermClass::Machine },
    TermEntry { id: 39, name: "确定性发生器", class: TermClass::Machine },
    TermEntry { id: 40, name: "漂移注入", class: TermClass::Lexicon },
    TermEntry { id: 41, name: "指纹回放", class: TermClass::Machine },
    TermEntry { id: 42, name: "收口断言", class: TermClass::Diag },
];

/// 冻结术语表的堆副本（注入器在其上做漂移，冻结表只读）。
fn canon_terms() -> Vec<TermEntry> {
    let mut v: Vec<TermEntry> = Vec::new();
    for t in TERM_ALL.iter() {
        v.push(*t);
    }
    v
}

/// 术语一致性核验：条数足额 + 号严格递增 + 正名不空不重。
/// 任一违约即返回对应语义码（判据 J1 的"全册统一"直读口）。
pub fn terms_consistent(t: &[TermEntry]) -> Result<(), u32> {
    if t.len() < TERM_MIN {
        return Err(SbvDiag::EDrift.val());
    }
    let mut prev_id = 0u32;
    for (i, e) in t.iter().enumerate() {
        // 正名非空。
        if e.name.is_empty() {
            return Err(SbvDiag::ETermEmpty.val());
        }
        // 术语号从 1 起严格递增。
        if i == 0 {
            if e.id < 1 {
                return Err(SbvDiag::EDupTerm.val());
            }
        } else if e.id <= prev_id {
            return Err(SbvDiag::EDupTerm.val());
        }
        prev_id = e.id;
        // 正名不重（回看全部前序词条）。
        for other in t.iter().take(i) {
            if other.name == e.name {
                return Err(SbvDiag::EDrift.val());
            }
        }
    }
    Ok(())
}

// ── D4/E1 预演面（激活 → 预演 → 复算 → 封存） ────────────────────
/// 预演对象别（D4·E1 闭集，均为分界闭集的预演口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RehearsalKind {
    /// D4 预演：分界闭集重裁决演练（折叠全部裁决口供）。
    D4,
    /// E1 预演：术语面重核演练（折叠全册术语号）。
    E1,
}

impl RehearsalKind {
    /// 预演别序号（指纹折叠用）。
    pub fn idx(self) -> u32 {
        match self {
            RehearsalKind::D4 => 0,
            RehearsalKind::E1 => 1,
        }
    }

    /// 预演别中文名（诊断文本用）。
    pub fn name(self) -> &'static str {
        match self {
            RehearsalKind::D4 => "D4分界预演",
            RehearsalKind::E1 => "E1术语预演",
        }
    }
}

/// 预演状态机：未激活 → 激活 → 封存，单调只进不退。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RehearsalState {
    /// 未激活：预演拒绝（EPERM）。
    Inactive,
    /// 激活：可预演、可录行、可封存。
    Active,
    /// 封存：预演拒绝（ESEALED）、激活倒退拒（EREWIND）。
    Sealed,
}

impl RehearsalState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            RehearsalState::Inactive => 0,
            RehearsalState::Active => 1,
            RehearsalState::Sealed => 2,
        }
    }
}

/// 预演行：一次预演的可复算凭据（界别/起段/段数/指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RehearsalRow {
    /// 预演别。
    pub kind: RehearsalKind,
    /// 起段（分界面取首界管辖起；术语面取首条术语号）。
    pub seg_lo: u32,
    /// 段数（分界面取界数；术语面取词条数）。
    pub seg_n: u32,
    /// 指纹（重放口供 FNV-1a 折叠，复算必须逐位吻合）。
    pub fp: u32,
}

/// 预演账本行容量（录满即容量耗尽）。
pub const REHEARSE_CAP: usize = 4;

/// FNV-1a 折叠基值与步进（确定性指纹，零随机合规）。
const FNV_BASIS: u32 = 0x811C_9DC5;

/// FNV-1a 单步折叠。
fn fnv_step(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(0x0100_0193)
}

/// D4 预演起段（首界管辖起）。
fn d4_lo() -> u32 {
    DECL_ALL.first().map(|d| d.lo).unwrap_or(0)
}

/// D4 预演段数（界数）。
fn d4_n() -> u32 {
    DECL_ALL.len() as u32
}

/// E1 预演起段（首条术语号）。
fn e1_lo() -> u32 {
    TERM_ALL.first().map(|t| t.id).unwrap_or(0)
}

/// E1 预演段数（词条数）。
fn e1_n() -> u32 {
    TERM_ALL.len() as u32
}

/// D4 指纹：按声明闭集序，逐界以自身申请界重放其管辖起段的裁决口供并折叠。
/// 重叠带的口供（EOVERLAP）照折——预演行记录的是确定性事实，不是绿账。
fn d4_fp() -> u32 {
    let mut h = FNV_BASIS;
    for d in DECL_ALL.iter() {
        let v = match judge_boundary(d.kind, d.lo) {
            Ok(b) => b.idx().wrapping_add(1),
            Err(e) => e,
        };
        h = fnv_step(h, v);
    }
    h
}

/// E1 指纹：按术语闭集序逐条折叠术语号（号序即口供）。
fn e1_fp() -> u32 {
    let mut h = FNV_BASIS;
    for t in TERM_ALL.iter() {
        h = fnv_step(h, t.id);
    }
    h
}

/// 预演行复算：重放对应口径的折叠指纹并与行内逐位比对。
pub fn recompute_row(row: &RehearsalRow) -> Result<(), u32> {
    let (lo, n, fp) = match row.kind {
        RehearsalKind::D4 => (d4_lo(), d4_n(), d4_fp()),
        RehearsalKind::E1 => (e1_lo(), e1_n(), e1_fp()),
    };
    if row.seg_lo != lo || row.seg_n != n || row.fp != fp {
        return Err(SbvDiag::ERecompute.val());
    }
    Ok(())
}

/// 预演账本：各预演别独立状态机 + 定容行槽。
#[derive(Clone, Debug)]
pub struct RehearsalLedger {
    /// D4 预演状态。
    d4: RehearsalState,
    /// E1 预演状态。
    e1: RehearsalState,
    /// 已录预演行（按录入序）。
    rows: Vec<RehearsalRow>,
}

impl RehearsalLedger {
    /// 开一本新账（全部未激活、行槽空）。
    pub fn new() -> RehearsalLedger {
        RehearsalLedger {
            d4: RehearsalState::Inactive,
            e1: RehearsalState::Inactive,
            rows: Vec::new(),
        }
    }

    /// 读某预演别状态。
    pub fn state_of(&self, kind: RehearsalKind) -> RehearsalState {
        match kind {
            RehearsalKind::D4 => self.d4,
            RehearsalKind::E1 => self.e1,
        }
    }

    /// 激活：未激活 → 激活（已激活再激活属停滞倒退，封存后激活属倒退）。
    pub fn activate(&mut self, kind: RehearsalKind) -> Result<(), u32> {
        let st = self.state_of(kind);
        if st.rank() >= RehearsalState::Active.rank() {
            return Err(SbvDiag::ERewind.val());
        }
        match kind {
            RehearsalKind::D4 => self.d4 = RehearsalState::Active,
            RehearsalKind::E1 => self.e1 = RehearsalState::Active,
        }
        Ok(())
    }

    /// 封存：激活 → 封存（未激活禁封、已封存重封属倒退）。
    pub fn seal_rehearsal(&mut self, kind: RehearsalKind) -> Result<(), u32> {
        let st = self.state_of(kind);
        match st {
            RehearsalState::Inactive => return Err(SbvDiag::EPerm.val()),
            RehearsalState::Sealed => return Err(SbvDiag::ERewind.val()),
            RehearsalState::Active => {}
        }
        match kind {
            RehearsalKind::D4 => self.d4 = RehearsalState::Sealed,
            RehearsalKind::E1 => self.e1 = RehearsalState::Sealed,
        }
        Ok(())
    }

    /// 预演判定器：未激活 → EPERM，已封存 → ESEALED，激活则产出可复算预演行。
    pub fn rehearse(&self, kind: RehearsalKind) -> Result<RehearsalRow, u32> {
        match self.state_of(kind) {
            RehearsalState::Inactive => return Err(SbvDiag::EPerm.val()),
            RehearsalState::Sealed => return Err(SbvDiag::ESealed.val()),
            RehearsalState::Active => {}
        }
        let row = match kind {
            RehearsalKind::D4 => RehearsalRow {
                kind,
                seg_lo: d4_lo(),
                seg_n: d4_n(),
                fp: d4_fp(),
            },
            RehearsalKind::E1 => RehearsalRow {
                kind,
                seg_lo: e1_lo(),
                seg_n: e1_n(),
                fp: e1_fp(),
            },
        };
        // 出账前先自复算：预演行必须当场可复算，不符即拒。
        recompute_row(&row)?;
        Ok(row)
    }

    /// 录行：定容行槽，满即容量耗尽拒。
    pub fn record(&mut self, row: RehearsalRow) -> Result<(), u32> {
        if self.rows.len() >= REHEARSE_CAP {
            return Err(SbvDiag::ECap.val());
        }
        self.rows.push(row);
        Ok(())
    }

    /// 已录行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否无行。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<RehearsalRow> {
        self.rows.get(idx).copied()
    }
}

// ── 确定性 LCG（种子 = SEG_BASE 派生，零随机红线合规） ───────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取自 Knuth MMIX 31 位推荐值（整数手册常用闭集）。
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 注入面一：术语漂移（重名/空名/号漂移 × 10 轮全抓） ───────────
/// 构造一条漂移术语表：mode 0 重名漂移、mode 1 空名漂移、mode 2 号漂移。
/// 三种漂移均必然违反 terms_consistent 的闭集约束（重名 → EDRIFT、
/// 空名 → ETERMEMPTY、号清零 → EDUPTERM），不存在逃脱构造。
fn build_drifted_terms(mode: u32, idx: usize) -> Vec<TermEntry> {
    let mut v = canon_terms();
    let n = v.len();
    if n < 2 || idx >= n {
        return v;
    }
    match mode {
        0 => {
            // 重名漂移：把 idx 条正名改成相邻条正名（正名闭集唯一 → 必重名）。
            let nb = idx.wrapping_add(1) % n;
            let nb_name = match v.get(nb) {
                Some(t) => t.name,
                None => "",
            };
            if let Some(e) = v.get_mut(idx) {
                e.name = nb_name;
            }
        }
        1 => {
            // 空名漂移：正名清空（非空约束必抓）。
            if let Some(e) = v.get_mut(idx) {
                e.name = "";
            }
        }
        _ => {
            // 号漂移：术语号清零（从 1 起严格递增约束必抓）。
            if let Some(e) = v.get_mut(idx) {
                e.id = 0;
            }
        }
    }
    v
}

/// 术语漂移注入器：10 轮（LCG 定种），每轮注入一条漂移表，
/// terms_consistent 必须抓住——返回 (抓到, 总轮, 首个逃脱描述)。
pub fn term_drift_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x11) as u64);
    let mut k = 0usize;
    while k < 10 {
        let mode = rng.below(3);
        let idx = rng.below(TERM_ALL.len() as u32) as usize;
        let drifted = build_drifted_terms(mode, idx);
        total += 1;
        match terms_consistent(&drifted) {
            Err(_) => caught += 1,
            Ok(()) => {
                if escape.is_empty() {
                    escape = format!("术语漂移路{}(mode{})逃脱", k, mode);
                }
            }
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 注入面二：分界越权（相邻界申请 × 10 轮全抓） ─────────────────
/// 唯一归属探针段闭集（10 段，每段恰被一界管辖）。
const OWNED_PROBES: &[u32] = &[
    0x82A00, 0x82A30, 0x82A60, 0x82A70, 0x82A90, 0x82AA0, 0x82AB0, 0x82AC0, 0x82B00, 0x82B30,
];

/// 分界越权注入器：10 轮，每轮对唯一归属段以相邻界申请，
/// judge_boundary 必须回越权码——返回 (抓到, 总轮, 首个逃脱描述)。
pub fn overreach_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x17) as u64);
    let mut k = 0usize;
    while k < 10 {
        let pick = OWNED_PROBES
            .get(rng.below(OWNED_PROBES.len() as u32) as usize)
            .copied()
            .unwrap_or(0);
        total += 1;
        let hit = match sole_owner(pick) {
            Some(owner) => {
                // 越权构造：相邻界申请同一号段。
                let wrong = owner.neighbor();
                judge_boundary(wrong, pick) == Err(SbvDiag::EOverreach.val())
            }
            None => false,
        };
        if hit {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("越权路{}(seg=0x{:05x})逃脱", k, pick);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 注入面三：预演未激活（冷账本直演 × 10 轮全抓） ───────────────
/// 预演未激活注入器：10 轮，每轮开冷账本（全未激活）直演 D4/E1 交替，
/// rehearse 必须回 EPERM——返回 (抓到, 总轮, 首个逃脱描述)。
pub fn inactive_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut k = 0usize;
    while k < 10 {
        let kind = if k % 2 == 0 {
            RehearsalKind::D4
        } else {
            RehearsalKind::E1
        };
        let led = RehearsalLedger::new();
        total += 1;
        match led.rehearse(kind) {
            Err(e) => {
                if e == SbvDiag::EPerm.val() {
                    caught += 1;
                } else if escape.is_empty() {
                    escape = format!("{}回错码0x{:05x}", kind.name(), e);
                }
            }
            Ok(_) => {
                if escape.is_empty() {
                    escape = format!("{}未激活却预演得手", kind.name());
                }
            }
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 双跑一致指纹（全流程 FNV-1a 折叠） ───────────────────────────
/// 裁决回放探针段闭集（覆盖唯一归属/豁免/重叠/界外四类路径）。
const REPLAY_PROBES: &[u32] = &[
    0x82A00, 0x82A20, 0x82A30, 0x82A40, 0x82A50, 0x82A60, 0x82A80, 0x82A90, 0x82AC0, 0x82B40,
    0x82B80,
];

/// 会话指纹：术语面 + 裁决面 + 预演面 + 三类注入捕获数，
/// 全部按 FNV-1a 折叠为单值（LCG 种子写死，双跑必然逐位一致）。
pub fn session_fingerprint() -> u32 {
    let mut h = FNV_BASIS;
    // 术语面：一致性核验口供。
    let tv = match terms_consistent(&canon_terms()) {
        Ok(()) => 0x5EED_0001,
        Err(e) => e,
    };
    h = fnv_step(h, tv);
    // 裁决面：以 D1 口径回放全部探针段（含重叠/落空/越权码口供）。
    for seg in REPLAY_PROBES.iter() {
        let v = match judge_boundary(Boundary::D1, *seg) {
            Ok(b) => b.idx().wrapping_add(1),
            Err(e) => e,
        };
        h = fnv_step(h, v);
    }
    // 预演面：新账本激活两别，折叠预演行字段。
    let mut led = RehearsalLedger::new();
    let _ = led.activate(RehearsalKind::D4);
    let _ = led.activate(RehearsalKind::E1);
    for kind in [RehearsalKind::D4, RehearsalKind::E1] {
        h = fnv_step(h, kind.idx());
        if let Ok(row) = led.rehearse(kind) {
            h = fnv_step(h, row.seg_lo);
            h = fnv_step(h, row.seg_n);
            h = fnv_step(h, row.fp);
        }
    }
    // 注入面：三类捕获数入折。
    let (c1, t1, _) = term_drift_sweep();
    h = fnv_step(h, c1 as u32);
    h = fnv_step(h, t1 as u32);
    let (c2, t2, _) = overreach_sweep();
    h = fnv_step(h, c2 as u32);
    h = fnv_step(h, t2 as u32);
    let (c3, t3, _) = inactive_sweep();
    h = fnv_step(h, c3 as u32);
    h = fnv_step(h, t3 as u32);
    h
}

/// 双跑一致报告：同流程折叠两次，逐位一致且非零才算绿。
pub fn fingerprint_report() -> Result<u32, u32> {
    let a = session_fingerprint();
    let b = session_fingerprint();
    if a != b || a == 0 {
        return Err(SbvDiag::EFingerprint.val());
    }
    Ok(a)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 段窗 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落窗、互异、查表命中、未知码回落。
fn diag_closed_ok() -> bool {
    for d in SBV_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if detail(v) != d.detail() {
            return false;
        }
    }
    let mut i = 0usize;
    while i < SBV_ALL.len() {
        let vi = match SBV_ALL.get(i) {
            Some(d) => d.val(),
            None => 0,
        };
        let mut j = i + 1;
        while j < SBV_ALL.len() {
            let vj = match SBV_ALL.get(j) {
                Some(d) => d.val(),
                None => 0,
            };
            if vi == vj {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    // 未知码必须回落"未知"文案，不得冒充已知码。
    if detail(SEG_BASE + 0x3F) != "未知诊断码：不在 0x82A01..0x82A0D 闭集内" {
        return false;
    }
    true
}

/// 分界三元组闭集守卫：三界齐、界序冻结、逐界三元组合法。
fn decl_closure_ok() -> bool {
    if DECL_ALL.len() != 3 {
        return false;
    }
    for (i, d) in DECL_ALL.iter().enumerate() {
        if d.kind.idx() != i as u32 {
            return false;
        }
        if validate_decl(d).is_err() {
            return false;
        }
    }
    true
}

/// 术语一致性核验绿：冻结表全过 + 条数足额（判据 J1 直读口）。
fn terms_green_ok() -> bool {
    match terms_consistent(&canon_terms()) {
        Ok(()) => {}
        Err(_) => return false,
    }
    TERM_ALL.len() >= TERM_MIN
}

/// 归属正常流：D1/D2/D3 各取一段唯一归属段，判归本界。
fn judge_happy_ok() -> bool {
    judge_boundary(Boundary::D1, 0x82A00) == Ok(Boundary::D1)
        && judge_boundary(Boundary::D2, 0x82A70) == Ok(Boundary::D2)
        && judge_boundary(Boundary::D3, 0x82B00) == Ok(Boundary::D3)
}

/// 落空语义：豁免段与界外段全部 ESRCH。
fn judge_miss_ok() -> bool {
    judge_boundary(Boundary::D1, 0x82A20) == Err(SbvDiag::ESrch.val())
        && judge_boundary(Boundary::D2, 0x82A80) == Err(SbvDiag::ESrch.val())
        && judge_boundary(Boundary::D3, 0x82B40) == Err(SbvDiag::ESrch.val())
        && judge_boundary(Boundary::D1, 0x82B80) == Err(SbvDiag::ESrch.val())
}

/// 重叠语义：重叠带任意口径申请全部 EOVERLAP。
fn judge_overlap_ok() -> bool {
    judge_boundary(Boundary::D1, 0x82A50) == Err(SbvDiag::EOverlap.val())
        && judge_boundary(Boundary::D2, 0x82A50) == Err(SbvDiag::EOverlap.val())
        && judge_boundary(Boundary::D3, 0x82A50) == Err(SbvDiag::EOverlap.val())
        && judge_boundary(Boundary::D1, 0x82A40) == Err(SbvDiag::EOverlap.val())
        && judge_boundary(Boundary::D2, 0x82A40) == Err(SbvDiag::EOverlap.val())
}

/// 判定次序冻结：重叠 > 落空 > 归属/越权，越权只在唯一命中后判。
fn judge_order_ok() -> bool {
    // 重叠优先于归属：即使申请界在命中集合内也先判重叠。
    if judge_boundary(Boundary::D1, 0x82A40) != Err(SbvDiag::EOverlap.val()) {
        return false;
    }
    // 豁免优先于归属：申请界自己的豁免段判落空而非归属。
    if judge_boundary(Boundary::D1, 0x82A20) != Err(SbvDiag::ESrch.val()) {
        return false;
    }
    // 越权居末：落空段不回越权码。
    if judge_boundary(Boundary::D2, 0x82B80) != Err(SbvDiag::ESrch.val()) {
        return false;
    }
    // 唯一命中但申请界不符：越权。
    judge_boundary(Boundary::D2, 0x82A00) == Err(SbvDiag::EOverreach.val())
}

/// 预演正常流：激活两别 → 各演一行 → 复算绿 → 录行入账。
fn rehearsal_ok() -> bool {
    let mut led = RehearsalLedger::new();
    if led.activate(RehearsalKind::D4).is_err() {
        return false;
    }
    if led.activate(RehearsalKind::E1).is_err() {
        return false;
    }
    let r4 = match led.rehearse(RehearsalKind::D4) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let r1 = match led.rehearse(RehearsalKind::E1) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if recompute_row(&r4).is_err() || recompute_row(&r1).is_err() {
        return false;
    }
    if led.record(r4).is_err() || led.record(r1).is_err() {
        return false;
    }
    led.len() == 2 && !led.is_empty()
}

/// 预演守卫：未激活 EPERM、未激活禁封、封存后再演 ESEALED。
fn rehearsal_perm_ok() -> bool {
    let mut led = RehearsalLedger::new();
    if led.rehearse(RehearsalKind::D4) != Err(SbvDiag::EPerm.val()) {
        return false;
    }
    if led.rehearse(RehearsalKind::E1) != Err(SbvDiag::EPerm.val()) {
        return false;
    }
    if led.seal_rehearsal(RehearsalKind::D4) != Err(SbvDiag::EPerm.val()) {
        return false;
    }
    let _ = led.activate(RehearsalKind::D4);
    let _ = led.seal_rehearsal(RehearsalKind::D4);
    led.rehearse(RehearsalKind::D4) == Err(SbvDiag::ESealed.val())
}

/// 容量耗尽：行槽录满后再录必拒且账面不再增长。
fn capacity_ok() -> bool {
    let mut led = RehearsalLedger::new();
    if led.activate(RehearsalKind::E1).is_err() {
        return false;
    }
    let row = match led.rehearse(RehearsalKind::E1) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let mut k = 0usize;
    while k < REHEARSE_CAP {
        if led.record(row).is_err() {
            return false;
        }
        k += 1;
    }
    if led.len() != REHEARSE_CAP {
        return false;
    }
    if led.record(row) != Err(SbvDiag::ECap.val()) {
        return false;
    }
    led.len() == REHEARSE_CAP
}

/// 状态机单调：未激活 < 激活 < 封存，停滞与倒退全拒。
fn state_mono_ok() -> bool {
    if RehearsalState::Inactive.rank() >= RehearsalState::Active.rank() {
        return false;
    }
    if RehearsalState::Active.rank() >= RehearsalState::Sealed.rank() {
        return false;
    }
    let mut led = RehearsalLedger::new();
    if led.activate(RehearsalKind::D4).is_err() {
        return false;
    }
    // 已激活再激活：停滞倒退拒。
    if led.activate(RehearsalKind::D4) != Err(SbvDiag::ERewind.val()) {
        return false;
    }
    if led.state_of(RehearsalKind::D4) != RehearsalState::Active {
        return false;
    }
    let _ = led.seal_rehearsal(RehearsalKind::D4);
    // 封存后再激活：倒退拒。
    led.activate(RehearsalKind::D4) == Err(SbvDiag::ERewind.val())
}

/// 三类注入全抓：术语漂移/分界越权/预演未激活各 10/10 无逃脱。
fn sweeps_all_caught_ok() -> bool {
    let (c1, t1, e1s) = term_drift_sweep();
    if t1 != 10 || c1 != 10 || !e1s.is_empty() {
        return false;
    }
    let (c2, t2, e2s) = overreach_sweep();
    if t2 != 10 || c2 != 10 || !e2s.is_empty() {
        return false;
    }
    let (c3, t3, e3s) = inactive_sweep();
    if t3 != 10 || c3 != 10 || !e3s.is_empty() {
        return false;
    }
    true
}

/// 双跑一致：指纹报告绿 + 三类注入结果两次逐项相同。
fn dual_run_ok() -> bool {
    if fingerprint_report().is_err() {
        return false;
    }
    let a = session_fingerprint();
    let b = session_fingerprint();
    if a != b || a == 0 {
        return false;
    }
    term_drift_sweep() == term_drift_sweep()
        && overreach_sweep() == overreach_sweep()
        && inactive_sweep() == inactive_sweep()
}

// ── 演示账本（检查入口明细用，激活两别各演一行并录账） ───────────
fn demo_ledger() -> RehearsalLedger {
    let mut led = RehearsalLedger::new();
    let _ = led.activate(RehearsalKind::D4);
    let _ = led.activate(RehearsalKind::E1);
    if let Ok(r) = led.rehearse(RehearsalKind::D4) {
        let _ = led.record(r);
    }
    if let Ok(r) = led.rehearse(RehearsalKind::E1) {
        let _ = led.record(r);
    }
    led
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// 检查节总条数声明（收口断言锚点）。
const CHECK_TOTAL: usize = 17;

/// UNX-F12768 自检（判据 12768 卡：术语一致性核验绿 + 漂移演练 10/10 检出）。
pub fn run_unx12768_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12768");
    let led = demo_ledger();

    s.add_dyn(
        "F12768-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} 对齐={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12768-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} 首=0x{:05x}",
            SBV_ALL.len(),
            SBV_ALL.first().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12768-03 分界三元组闭集",
        decl_closure_ok(),
        &format!(
            "界数={} 首名={}",
            DECL_ALL.len(),
            DECL_ALL.first().map(|d| d.name).unwrap_or("?")
        ),
    );

    s.add_dyn(
        "F12768-04 术语一致性核验绿",
        terms_green_ok(),
        &format!(
            "条数={} 首类={}",
            TERM_ALL.len(),
            TERM_ALL.first().map(|t| t.class.name()).unwrap_or("?")
        ),
    );

    s.add_dyn(
        "F12768-05 归属正常流",
        judge_happy_ok(),
        &format!(
            "{}{}{}各一段唯一归属",
            Boundary::D1.name(),
            Boundary::D2.name(),
            Boundary::D3.name()
        ),
    );

    s.add_dyn(
        "F12768-06 落空与豁免",
        judge_miss_ok(),
        "豁免两段界外两段全 ESRCH",
    );

    s.add_dyn(
        "F12768-07 多界重叠裁决",
        judge_overlap_ok(),
        "重叠带两段三口径全 EOVERLAP",
    );

    s.add_dyn(
        "F12768-08 判定次序冻结",
        judge_order_ok(),
        "重叠>落空>归属/越权 全序锁死",
    );

    s.add_dyn(
        "F12768-09 预演激活复算",
        rehearsal_ok(),
        &format!(
            "rows={} 首行fp=0x{:08x}",
            led.len(),
            led.row_at(0).map(|r| r.fp).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12768-10 未激活与封存守卫",
        rehearsal_perm_ok(),
        "未激活EPERM 封存后ESEALED",
    );

    s.add_dyn(
        "F12768-11 预演容量耗尽",
        capacity_ok(),
        &format!("cap={} 满后录行必拒", REHEARSE_CAP),
    );

    s.add_dyn(
        "F12768-12 激活状态机单调",
        state_mono_ok(),
        "停滞激活与封存倒退全拒",
    );

    let (c1, t1, e1s) = term_drift_sweep();
    s.add_dyn(
        "F12768-13 术语漂移注入 10/10",
        c1 == 10 && t1 == 10 && e1s.is_empty(),
        &format!("{}/{} {}", c1, t1, e1s),
    );

    let (c2, t2, e2s) = overreach_sweep();
    s.add_dyn(
        "F12768-14 分界越权注入 10/10",
        c2 == 10 && t2 == 10 && e2s.is_empty(),
        &format!("{}/{} {}", c2, t2, e2s),
    );

    let (c3, t3, e3s) = inactive_sweep();
    s.add_dyn(
        "F12768-15 预演未激活注入 10/10",
        c3 == 10 && t3 == 10 && e3s.is_empty(),
        &format!("{}/{} {}", c3, t3, e3s),
    );

    s.add_dyn(
        "F12768-16 双跑一致指纹",
        dual_run_ok(),
        &format!("fp=0x{:08x}", session_fingerprint()),
    );

    // 收口：判据条数声明（本节共 CHECK_TOTAL 条，含本条）。
    let n_before = s.len();
    s.add_dyn(
        "F12768-17 收口判据条数",
        n_before + 1 == CHECK_TOTAL,
        &format!("判据条数={}/{}", n_before + 1, CHECK_TOTAL),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 段窗。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x82A00);
    }

    #[test]
    fn diag_codes_closed_set() {
        // 闭集：13 码互异、全落窗、查表命中、未知码回落。
        assert_eq!(SBV_ALL.len(), 13);
        let mut i = 0usize;
        while i < SBV_ALL.len() {
            let vi = SBV_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < SBV_ALL.len() {
                let vj = SBV_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }

    #[test]
    fn decl_triples_and_membership() {
        // 三元组闭集：三界齐、逐界合法、唯一归属抽样正确。
        assert_eq!(DECL_ALL.len(), 3);
        for d in DECL_ALL.iter() {
            assert!(validate_decl(d).is_ok());
            assert!(!d.name.is_empty());
            assert!(d.lo < d.hi);
        }
        assert_eq!(sole_owner(0x82A00), Some(Boundary::D1));
        assert_eq!(sole_owner(0x82A70), Some(Boundary::D2));
        assert_eq!(sole_owner(0x82B00), Some(Boundary::D3));
        // 重叠带与豁免段无唯一归属。
        assert_eq!(sole_owner(0x82A50), None);
        assert_eq!(sole_owner(0x82A20), None);
        assert_eq!(sole_owner(0x82B80), None);
    }

    #[test]
    fn judge_ruling_paths_and_order() {
        // 正常归属三界。
        assert_eq!(judge_boundary(Boundary::D1, 0x82A00), Ok(Boundary::D1));
        assert_eq!(judge_boundary(Boundary::D2, 0x82A70), Ok(Boundary::D2));
        assert_eq!(judge_boundary(Boundary::D3, 0x82B00), Ok(Boundary::D3));
        // 重叠带：任意申请界都回重叠。
        assert_eq!(
            judge_boundary(Boundary::D1, 0x82A50),
            Err(SbvDiag::EOverlap.val())
        );
        assert_eq!(
            judge_boundary(Boundary::D3, 0x82A50),
            Err(SbvDiag::EOverlap.val())
        );
        // 落空：豁免段与界外段。
        assert_eq!(
            judge_boundary(Boundary::D1, 0x82A20),
            Err(SbvDiag::ESrch.val())
        );
        assert_eq!(
            judge_boundary(Boundary::D1, 0x82B80),
            Err(SbvDiag::ESrch.val())
        );
        // 越权：唯一归属段以相邻界申请。
        assert_eq!(
            judge_boundary(Boundary::D2, 0x82A00),
            Err(SbvDiag::EOverreach.val())
        );
        // 判定次序冻结谓词。
        assert!(judge_order_ok());
    }

    #[test]
    fn rehearse_flow_and_recompute() {
        // 未激活直演必拒。
        let mut led = RehearsalLedger::new();
        assert_eq!(
            led.rehearse(RehearsalKind::D4),
            Err(SbvDiag::EPerm.val())
        );
        assert!(led.activate(RehearsalKind::D4).is_ok());
        assert!(led.activate(RehearsalKind::E1).is_ok());
        let made = led.rehearse(RehearsalKind::D4);
        assert!(made.is_ok());
        let row = match made {
            Ok(r) => r,
            Err(_) => return,
        };
        assert_eq!(row.seg_n, DECL_ALL.len() as u32);
        assert_eq!(row.seg_lo, 0x82A00);
        assert!(recompute_row(&row).is_ok());
        // 篡改指纹后复算必不符。
        let mut bad = row;
        bad.fp = bad.fp.wrapping_add(1);
        assert_eq!(recompute_row(&bad), Err(SbvDiag::ERecompute.val()));
        // 录行入账。
        assert!(led.record(row).is_ok());
        assert_eq!(led.len(), 1);
        // 正常流谓词。
        assert!(rehearsal_ok());
    }

    #[test]
    fn capacity_and_rewind() {
        // 容量耗尽：满后再录必拒。
        assert!(capacity_ok());
        // 状态机单调：停滞与倒退全拒。
        assert!(state_mono_ok());
        let mut led = RehearsalLedger::new();
        // 未激活禁封。
        assert_eq!(
            led.seal_rehearsal(RehearsalKind::E1),
            Err(SbvDiag::EPerm.val())
        );
        let _ = led.activate(RehearsalKind::E1);
        // 已激活再激活：停滞拒。
        assert_eq!(
            led.activate(RehearsalKind::E1),
            Err(SbvDiag::ERewind.val())
        );
        let _ = led.seal_rehearsal(RehearsalKind::E1);
        // 重封与封后再激活：倒退拒。
        assert_eq!(
            led.seal_rehearsal(RehearsalKind::E1),
            Err(SbvDiag::ERewind.val())
        );
        assert_eq!(
            led.activate(RehearsalKind::E1),
            Err(SbvDiag::ERewind.val())
        );
        assert_eq!(led.state_of(RehearsalKind::E1), RehearsalState::Sealed);
    }

    #[test]
    fn three_sweeps_all_caught() {
        // 注入必抓：三类各 10 轮全数命中，无逃脱。
        let (c1, t1, e1s) = term_drift_sweep();
        assert_eq!(t1, 10);
        assert_eq!(c1, 10, "术语漂移逃脱: {}", e1s);
        assert!(e1s.is_empty());
        let (c2, t2, e2s) = overreach_sweep();
        assert_eq!(t2, 10);
        assert_eq!(c2, 10, "越权逃脱: {}", e2s);
        assert!(e2s.is_empty());
        let (c3, t3, e3s) = inactive_sweep();
        assert_eq!(t3, 10);
        assert_eq!(c3, 10, "未激活预演逃脱: {}", e3s);
        assert!(e3s.is_empty());
        assert!(sweeps_all_caught_ok());
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        // 双跑一致：指纹两次折叠逐位相同且非零。
        assert!(fingerprint_report().is_ok());
        let a = session_fingerprint();
        let b = session_fingerprint();
        assert_eq!(a, b);
        assert_ne!(a, 0);
        // 三类注入两次逐项相同。
        assert_eq!(term_drift_sweep(), term_drift_sweep());
        assert_eq!(overreach_sweep(), overreach_sweep());
        assert_eq!(inactive_sweep(), inactive_sweep());
        assert!(dual_run_ok());
    }

    #[test]
    fn terms_consistency_and_drift() {
        // 冻结表绿 + 条数足额。
        assert!(terms_green_ok());
        assert_eq!(terms_consistent(&canon_terms()), Ok(()));
        assert!(TERM_ALL.len() >= TERM_MIN);
        // 三类漂移逐类必抓。
        let dup = build_drifted_terms(0, 5);
        assert!(terms_consistent(&dup).is_err());
        let empty_name = build_drifted_terms(1, 5);
        assert_eq!(
            terms_consistent(&empty_name),
            Err(SbvDiag::ETermEmpty.val())
        );
        let zero_id = build_drifted_terms(2, 5);
        assert!(terms_consistent(&zero_id).is_err());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条、收口条数声明吻合。
        let s = run_unx12768_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.len(), CHECK_TOTAL);
        assert_eq!(s.tally().1, 0);
        assert_eq!(s.tally().0, CHECK_TOTAL);
    }
}
