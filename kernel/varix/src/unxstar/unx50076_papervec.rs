//! UNX-F50076 纸张向量账（选择用例集 + 校验和）（0x435a0 段）
//!
//! 锚点：UNX-F50076-J1 用例可复跑；判据锚定 §AI-63 §5。
//!
//! 数据段：`SEG_BASE = 0x435a0`，诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 11`，
//! 逐项见 [`PaperVecError`]。
//!
//! 为什么纸盒选择需要「向量用例」而不是随手一测：选择规则是三维约束
//! （幅面能否装下 / 克重是否超限 / 介质类型是否支持）下的**最小可用盒**，
//! 三个维度的边界（正好装下、旋转后装下、克重打满）恰好是最容易写错的点。
//! 把 12 条「请求 → 期望盒号」固化成用例集，每次改选择器只需重跑并比对
//! 校验和：校验和一致 = 逐条决策未变；不一致 = 某条边界被动了，直接定位。
//!
//! 结构清单：
//! * [`MediaKind`] + [`Tray`] + [`TrayTable`]：纸盒注册表（能力位图 + 面积序）；
//! * [`MediaRequest`] + [`select_tray`]：选择器（最小面积优先，装不下即拒）；
//! * [`VectorCase`] + [`VectorSet`]：用例集（去重 / 定容 / 封账）；
//! * [`CaseResult`] + [`VectorLedger`]：向量账（逐条判定 + 校验和 + 复跑比对）；
//! * [`injection_core`]：6 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x435a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x435a0;

const E_EMPTY: u16 = 1;
const E_BAD_DIMS: u16 = 2;
const E_BAD_WEIGHT: u16 = 3;
const E_BAD_TRAY: u16 = 4;
const E_DUPLICATE: u16 = 5;
const E_SET_FULL: u16 = 6;
const E_NO_FIT: u16 = 7;
const E_CASE_FAILED: u16 = 8;
const E_LEDGER_FULL: u16 = 9;
const E_SEALED: u16 = 10;
const E_CHECKSUM: u16 = 11;

/// 向量账错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaperVecError {
    /// 空用例集 / 空盒表。
    Empty,
    /// 幅面越出 20..=1200 mm。
    BadDims,
    /// 克重越出 40..=400 gsm。
    BadWeight,
    /// 盒号不在 1..=8。
    BadTray,
    /// 盒号或用例号重复。
    Duplicate,
    /// 用例集容量已满。
    SetFull,
    /// 没有任何纸盒能装下该请求。
    NoFit,
    /// 严格复跑出现用例失败。
    CaseFailed,
    /// 账本容量已满。
    LedgerFull,
    /// 表面已封账。
    Sealed,
    /// 复跑校验和与账不一致。
    ChecksumDrift,
}

impl PaperVecError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            PaperVecError::Empty => E_EMPTY,
            PaperVecError::BadDims => E_BAD_DIMS,
            PaperVecError::BadWeight => E_BAD_WEIGHT,
            PaperVecError::BadTray => E_BAD_TRAY,
            PaperVecError::Duplicate => E_DUPLICATE,
            PaperVecError::SetFull => E_SET_FULL,
            PaperVecError::NoFit => E_NO_FIT,
            PaperVecError::CaseFailed => E_CASE_FAILED,
            PaperVecError::LedgerFull => E_LEDGER_FULL,
            PaperVecError::Sealed => E_SEALED,
            PaperVecError::ChecksumDrift => E_CHECKSUM,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            PaperVecError::Empty => "empty vector set or tray table",
            PaperVecError::BadDims => "request size outside 20..=1200 mm",
            PaperVecError::BadWeight => "request weight outside 40..=400 gsm",
            PaperVecError::BadTray => "tray id outside 1..=8",
            PaperVecError::Duplicate => "duplicate tray or case id",
            PaperVecError::SetFull => "vector set full",
            PaperVecError::NoFit => "no tray can hold the request",
            PaperVecError::CaseFailed => "vector case failed on replay",
            PaperVecError::LedgerFull => "vector ledger full",
            PaperVecError::Sealed => "surface sealed read-only",
            PaperVecError::ChecksumDrift => "replay checksum drifted from ledger",
        }
    }
}

/// 介质类型闭集。位图化让「一盒支持多介质」用一次按位与判定，
/// 避免为每种类型维护一个 bool 数组（结构体随类型数膨胀）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    /// 普通纸。
    Plain,
    /// 证券纸。
    Bond,
    /// 透明胶片。
    Transparency,
    /// 标签纸。
    Label,
    /// 信封。
    Envelope,
}

impl MediaKind {
    /// 类型码 0..=4。
    pub fn code(self) -> u8 {
        match self {
            MediaKind::Plain => 0,
            MediaKind::Bond => 1,
            MediaKind::Transparency => 2,
            MediaKind::Label => 3,
            MediaKind::Envelope => 4,
        }
    }

    /// 类型码反解；闭集外返回 [`PaperVecError::BadTray`]。
    pub fn from_code(c: u8) -> Result<MediaKind, PaperVecError> {
        match c {
            0 => Ok(MediaKind::Plain),
            1 => Ok(MediaKind::Bond),
            2 => Ok(MediaKind::Transparency),
            3 => Ok(MediaKind::Label),
            4 => Ok(MediaKind::Envelope),
            _ => Err(PaperVecError::BadTray),
        }
    }

    /// 能力位（第 code 位）。
    pub fn bit(self) -> u8 {
        1u8 << self.code()
    }

    /// 类型名。
    pub fn label(self) -> &'static str {
        match self {
            MediaKind::Plain => "plain",
            MediaKind::Bond => "bond",
            MediaKind::Transparency => "transparency",
            MediaKind::Label => "label",
            MediaKind::Envelope => "envelope",
        }
    }
}

/// 纸盒。尺寸单位 mm；`kinds` 是介质能力位图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tray {
    pub id: u8,
    pub w_mm: u16,
    pub h_mm: u16,
    pub max_gsm: u16,
    pub kinds: u8,
}

impl Tray {
    /// 盒号闭集 1..=8 且尺寸非零。
    pub fn validate(&self) -> Result<(), PaperVecError> {
        if self.id == 0 || self.id > 8 {
            return Err(PaperVecError::BadTray);
        }
        if self.w_mm == 0 || self.h_mm == 0 || self.w_mm > 1200 || self.h_mm > 1200 {
            return Err(PaperVecError::BadDims);
        }
        if self.max_gsm < 40 || self.max_gsm > 400 {
            return Err(PaperVecError::BadWeight);
        }
        Ok(())
    }

    /// 面积（mm²，用于「最小可用盒」比较）。
    pub fn area(&self) -> u32 {
        self.w_mm as u32 * self.h_mm as u32
    }

    /// 能否承载该请求：克重上限 / 介质位 / 幅面（允许旋转 90°）。
    pub fn fits(&self, req: &MediaRequest) -> bool {
        if req.gsm > self.max_gsm {
            return false;
        }
        if (self.kinds & req.kind.bit()) == 0 {
            return false;
        }
        let upright = req.w_mm <= self.w_mm && req.h_mm <= self.h_mm;
        let turned = req.w_mm <= self.h_mm && req.h_mm <= self.w_mm;
        upright || turned
    }

    /// 一行可读能力。
    pub fn line(&self) -> String {
        format!(
            "tray{} {}x{} <= {}gsm kinds={:#04x}",
            self.id, self.w_mm, self.h_mm, self.max_gsm, self.kinds
        )
    }
}

/// 作业介质请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaRequest {
    pub w_mm: u16,
    pub h_mm: u16,
    pub gsm: u16,
    pub kind: MediaKind,
}

impl MediaRequest {
    /// 入参自检：幅面 20..=1200、克重 40..=400。
    pub fn validate(&self) -> Result<(), PaperVecError> {
        if self.w_mm < 20 || self.w_mm > 1200 || self.h_mm < 20 || self.h_mm > 1200 {
            return Err(PaperVecError::BadDims);
        }
        if self.gsm < 40 || self.gsm > 400 {
            return Err(PaperVecError::BadWeight);
        }
        Ok(())
    }

    /// 一行可读请求。
    pub fn line(&self) -> String {
        format!(
            "{}x{} {}gsm {}",
            self.w_mm,
            self.h_mm,
            self.gsm,
            self.kind.label()
        )
    }
}

/// 纸盒注册表。
#[derive(Clone, Debug)]
pub struct TrayTable {
    trays: Vec<Tray>,
    cap: usize,
    sealed: bool,
}

impl TrayTable {
    /// 建空表。
    pub fn new(cap: usize) -> TrayTable {
        TrayTable {
            trays: Vec::with_capacity(cap),
            cap,
            sealed: false,
        }
    }

    /// 登记纸盒：封账 → 定容 → 自检 → 号唯一。
    pub fn insert(&mut self, tray: Tray) -> Result<(), PaperVecError> {
        if self.sealed {
            return Err(PaperVecError::Sealed);
        }
        // 先验内容再验容量：坏盒号/重复号是**更具体**的故障，
        // 表满只会掩盖它（现场看到 SetFull 会去加盒，而真因是盒号写错）。
        tray.validate()?;
        if self.by_id(tray.id).is_some() {
            return Err(PaperVecError::Duplicate);
        }
        if self.trays.len() >= self.cap {
            return Err(PaperVecError::SetFull);
        }
        self.trays.push(tray);
        Ok(())
    }

    /// 按号查纸盒。
    pub fn by_id(&self, id: u8) -> Option<&Tray> {
        let mut i = 0usize;
        while i < self.trays.len() {
            if let Some(t) = self.trays.get(i) {
                if t.id == id {
                    return Some(t);
                }
            }
            i += 1;
        }
        None
    }

    /// 选择：在全部可承载的盒里取面积最小者，同面积取小盒号。
    ///
    /// 为什么是面积最小而不是盒号最小：盒号是安装顺序，面积才是耗材与
    /// 纸路代价的代理；按盒号选会让「A5 作业打到 A4 盒」成为默认行为。
    pub fn select(&self, req: &MediaRequest) -> Result<u8, PaperVecError> {
        req.validate()?;
        let mut best: Option<(u32, u8)> = None;
        let mut i = 0usize;
        while i < self.trays.len() {
            if let Some(t) = self.trays.get(i) {
                if t.fits(req) {
                    let a = t.area();
                    let better = match best {
                        None => true,
                        Some((ba, bi)) => a < ba || (a == ba && t.id < bi),
                    };
                    if better {
                        best = Some((a, t.id));
                    }
                }
            }
            i += 1;
        }
        match best {
            Some((_, id)) => Ok(id),
            None => Err(PaperVecError::NoFit),
        }
    }

    /// 该请求的可承载盒数量（性质判据用）。
    pub fn eligible(&self, req: &MediaRequest) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.trays.len() {
            if let Some(t) = self.trays.get(i) {
                if t.fits(req) {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.trays.len()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
}

/// 独立选择函数（表无关的薄封装，方便外部按表复用同一判据）。
pub fn select_tray(req: &MediaRequest, table: &TrayTable) -> Result<u8, PaperVecError> {
    table.select(req)
}

/// 标准四盒表：A4 主盒 / A5 副盒 / Letter 盒 / 信封盒。
///
/// 参数不是随手填的：tray2 的 250 gsm 与 tray3 的 220 gsm 刻意不同，
/// 好让「克重打满」这条边界在用例里真的能被区分出来。
pub fn default_trays() -> TrayTable {
    let mut t = TrayTable::new(4);
    let plain_bond = MediaKind::Plain.bit() | MediaKind::Bond.bit();
    let _ = t.insert(Tray {
        id: 1,
        w_mm: 210,
        h_mm: 297,
        max_gsm: 300,
        kinds: plain_bond | MediaKind::Label.bit(),
    });
    let _ = t.insert(Tray {
        id: 2,
        w_mm: 148,
        h_mm: 210,
        max_gsm: 250,
        kinds: plain_bond,
    });
    let _ = t.insert(Tray {
        id: 3,
        w_mm: 216,
        h_mm: 279,
        max_gsm: 220,
        kinds: plain_bond | MediaKind::Label.bit(),
    });
    let _ = t.insert(Tray {
        id: 4,
        w_mm: 220,
        h_mm: 110,
        max_gsm: 120,
        kinds: MediaKind::Envelope.bit(),
    });
    t
}

/// 向量用例：请求 → 期望盒号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VectorCase {
    pub id: u16,
    pub req: MediaRequest,
    pub expected: u8,
}

impl VectorCase {
    /// 构造用例。
    pub fn new(id: u16, w_mm: u16, h_mm: u16, gsm: u16, kind: MediaKind, expected: u8) -> VectorCase {
        VectorCase {
            id,
            req: MediaRequest {
                w_mm,
                h_mm,
                gsm,
                kind,
            },
            expected,
        }
    }

    /// 一行可读用例。
    pub fn line(&self) -> String {
        format!("#{} {} -> tray{}", self.id, self.req.line(), self.expected)
    }
}

/// 用例集（定容 + 去重 + 封账）。
#[derive(Clone, Debug)]
pub struct VectorSet {
    cases: Vec<VectorCase>,
    cap: usize,
    sealed: bool,
}

impl VectorSet {
    /// 建空集。
    pub fn new(cap: usize) -> VectorSet {
        VectorSet {
            cases: Vec::with_capacity(cap),
            cap,
            sealed: false,
        }
    }

    /// 由用例表建集；空表返回 [`PaperVecError::Empty`]。
    pub fn from_cases(cases: &[VectorCase]) -> Result<VectorSet, PaperVecError> {
        if cases.is_empty() {
            return Err(PaperVecError::Empty);
        }
        let mut v = VectorSet::new(cases.len());
        let mut i = 0usize;
        while i < cases.len() {
            if let Some(c) = cases.get(i) {
                v.add(*c)?;
            }
            i += 1;
        }
        Ok(v)
    }

    /// 加一条用例：封账 → 定容 → 入参自检 → 号唯一。
    pub fn add(&mut self, case: VectorCase) -> Result<(), PaperVecError> {
        if self.sealed {
            return Err(PaperVecError::Sealed);
        }
        // 同盒表：先内容后容量，重复号不得被 SetFull 掩盖。
        case.req.validate()?;
        if case.expected == 0 || case.expected > 8 {
            return Err(PaperVecError::BadTray);
        }
        if self.by_id(case.id).is_some() {
            return Err(PaperVecError::Duplicate);
        }
        if self.cases.len() >= self.cap {
            return Err(PaperVecError::SetFull);
        }
        self.cases.push(case);
        Ok(())
    }

    /// 按号查用例。
    pub fn by_id(&self, id: u16) -> Option<&VectorCase> {
        let mut i = 0usize;
        while i < self.cases.len() {
            if let Some(c) = self.cases.get(i) {
                if c.id == id {
                    return Some(c);
                }
            }
            i += 1;
        }
        None
    }

    /// 取第 i 条。
    pub fn case(&self, i: usize) -> Option<&VectorCase> {
        self.cases.get(i)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
}

/// 标准 12 条用例（覆盖：正好装下 / 旋转 90° / 克重打满 / 介质独占 / 小盒优先）。
pub fn default_cases() -> Vec<VectorCase> {
    let mut v = Vec::with_capacity(12);
    v.push(VectorCase::new(1, 210, 297, 80, MediaKind::Plain, 1));
    v.push(VectorCase::new(2, 148, 210, 80, MediaKind::Plain, 2));
    v.push(VectorCase::new(3, 216, 279, 80, MediaKind::Plain, 3));
    v.push(VectorCase::new(4, 210, 297, 200, MediaKind::Label, 1));
    v.push(VectorCase::new(5, 210, 297, 120, MediaKind::Bond, 1));
    v.push(VectorCase::new(6, 110, 220, 100, MediaKind::Envelope, 4));
    v.push(VectorCase::new(7, 148, 210, 200, MediaKind::Label, 3));
    v.push(VectorCase::new(8, 210, 297, 240, MediaKind::Plain, 1));
    v.push(VectorCase::new(9, 148, 210, 240, MediaKind::Bond, 2));
    v.push(VectorCase::new(10, 216, 279, 150, MediaKind::Label, 3));
    v.push(VectorCase::new(11, 210, 297, 60, MediaKind::Plain, 1));
    v.push(VectorCase::new(12, 148, 210, 90, MediaKind::Plain, 2));
    v
}

/// 标准用例集。
pub fn default_set() -> Result<VectorSet, PaperVecError> {
    VectorSet::from_cases(&default_cases())
}

const FNV_OFFSET: u32 = 0x811c_9dc5;
const FNV_PRIME: u32 = 0x0100_0193;

fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(FNV_PRIME)
}

fn fnv_u16(h: u32, v: u16) -> u32 {
    fnv_byte(fnv_byte(h, (v >> 8) as u8), v as u8)
}

/// 单条用例结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaseResult {
    pub id: u16,
    pub chosen: u8,
    pub expected: u8,
    pub pass: bool,
}

/// 向量账：逐条判定 + 校验和。
#[derive(Clone, Debug)]
pub struct VectorLedger {
    rows: Vec<CaseResult>,
    cap: usize,
    sealed: bool,
    checksum: u32,
}

impl VectorLedger {
    /// 建空账。
    pub fn new(cap: usize) -> VectorLedger {
        VectorLedger {
            rows: Vec::with_capacity(cap),
            cap,
            sealed: false,
            checksum: FNV_OFFSET,
        }
    }

    /// 记一条结果：封账 → 定容 → 折叠校验和。
    pub fn push(&mut self, r: CaseResult) -> Result<(), PaperVecError> {
        if self.sealed {
            return Err(PaperVecError::Sealed);
        }
        if self.rows.len() >= self.cap {
            return Err(PaperVecError::LedgerFull);
        }
        let mut h = self.checksum;
        h = fnv_u16(h, r.id);
        h = fnv_byte(h, r.chosen);
        h = fnv_byte(h, r.expected);
        h = fnv_byte(h, if r.pass { 1 } else { 0 });
        self.checksum = h;
        self.rows.push(r);
        Ok(())
    }

    /// 逐条跑用例集。选择器报错（如 NoFit）按「选不出盒」计一条失败，
    /// 不中断整批——一批里一条坏用例不该把其余 11 条的证据一起吞掉。
    pub fn run(set: &VectorSet, table: &TrayTable) -> Result<VectorLedger, PaperVecError> {
        let mut led = VectorLedger::new(set.len());
        let mut i = 0usize;
        while i < set.len() {
            if let Some(c) = set.case(i) {
                let chosen = match table.select(&c.req) {
                    Ok(t) => t,
                    Err(_) => 0u8,
                };
                led.push(CaseResult {
                    id: c.id,
                    chosen,
                    expected: c.expected,
                    pass: chosen == c.expected,
                })?;
            }
            i += 1;
        }
        Ok(led)
    }

    /// 严格版：有任何一条失败即 `CaseFailed`（给 CI 门禁用）。
    pub fn run_strict(set: &VectorSet, table: &TrayTable) -> Result<VectorLedger, PaperVecError> {
        let led = VectorLedger::run(set, table)?;
        if led.failed() > 0 {
            Err(PaperVecError::CaseFailed)
        } else {
            Ok(led)
        }
    }

    /// 通过条数。
    pub fn passed(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.pass {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败条数。
    pub fn failed(&self) -> usize {
        self.rows.len() - self.passed()
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// 取第 i 条结果。
    pub fn row(&self, i: usize) -> Option<&CaseResult> {
        self.rows.get(i)
    }

    /// 校验和。
    pub fn checksum(&self) -> u32 {
        self.checksum
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 复跑：用同一用例集与盒表重算校验和并与账比对。
    ///
    /// 这是 J1「可复跑」的实现：不是「再跑一次看着一样」，而是**逐位比对
    /// 校验和**；盒表被换掉（少一个盒、克重上限改了）同样会漂。
    pub fn replay(&self, set: &VectorSet, table: &TrayTable) -> Result<u32, PaperVecError> {
        let fresh = VectorLedger::run(set, table)?;
        if fresh.len() != self.rows.len() || fresh.checksum != self.checksum {
            return Err(PaperVecError::ChecksumDrift);
        }
        Ok(fresh.checksum)
    }

    /// 一行可读汇总。
    pub fn line(&self) -> String {
        format!(
            "n={} pass={} fail={} cs={:08x}",
            self.rows.len(),
            self.passed(),
            self.failed(),
            self.checksum
        )
    }
}

// ---------------------------------------------------------------------------
// 注入核：6 类坏样本必须全部被抓
// ---------------------------------------------------------------------------

/// 6 类坏样本注入核，返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;

    // 1) 空用例集。
    if VectorSet::from_cases(&[]).err() == Some(PaperVecError::Empty) {
        caught += 1;
    }

    // 2) 坏尺寸（宽度 0）。
    let zero_w = VectorCase::new(1, 0, 297, 80, MediaKind::Plain, 1);
    if zero_w.req.validate().err() == Some(PaperVecError::BadDims) {
        caught += 1;
    }

    // 3) 坏克重（10 gsm 低于下限）。
    let light = VectorCase::new(1, 148, 210, 10, MediaKind::Plain, 2);
    if light.req.validate().err() == Some(PaperVecError::BadWeight) {
        caught += 1;
    }

    // 4) 坏盒号（9 不在 1..=8）。
    let mut bad_tray = default_trays();
    let rogue = Tray {
        id: 9,
        w_mm: 210,
        h_mm: 297,
        max_gsm: 200,
        kinds: MediaKind::Plain.bit(),
    };
    if bad_tray.insert(rogue).err() == Some(PaperVecError::BadTray) {
        caught += 1;
    }

    // 5) 重复用例号。
    let mut dup = VectorSet::new(4);
    let c = VectorCase::new(1, 148, 210, 80, MediaKind::Plain, 2);
    if dup.add(c).is_ok() && dup.add(c).err() == Some(PaperVecError::Duplicate) {
        caught += 1;
    }

    // 6) 无盒可装（400×400 超出全部四盒）。
    let huge = MediaRequest {
        w_mm: 400,
        h_mm: 400,
        gsm: 100,
        kind: MediaKind::Plain,
    };
    if default_trays().select(&huge).err() == Some(PaperVecError::NoFit) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50076 自检：15 条判据。
pub fn run_unx50076_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50076");

    let table = default_trays();
    let set = default_set();

    // J1-1 用例规模：12 条、号 1..=12 互异且连续。
    let mut shape_ok = false;
    let mut n = 0usize;
    if let Ok(v) = &set {
        n = v.len();
        shape_ok = n == 12;
        let mut id: u16 = 1;
        while id <= 12 {
            match v.by_id(id) {
                Some(c) => {
                    if c.id != id {
                        shape_ok = false;
                    }
                }
                None => shape_ok = false,
            }
            id += 1;
        }
    }
    let det = format!("cases={}/12", n);
    s.add_dyn("50076-J1-用例12条", shape_ok, &det);

    // J1-2 全绿：12 条逐条通过（宽松跑 + 严格跑都必须成功）。
    let led = match &set {
        Ok(v) => VectorLedger::run(v, &table),
        Err(_) => Err(PaperVecError::Empty),
    };
    let strict_ok = match &set {
        Ok(v) => VectorLedger::run_strict(v, &table).is_ok(),
        Err(_) => false,
    };
    let ok = match &led {
        Ok(l) => l.len() == 12 && l.passed() == 12 && l.failed() == 0 && strict_ok,
        Err(_) => false,
    };
    let det = match &led {
        Ok(l) => l.line(),
        Err(_) => String::from("no ledger"),
    };
    s.add_dyn("50076-J1-用例全绿", ok, &det);

    // J1-3 可复跑：两次独立运行校验和逐位一致。
    let cs1 = match (&led, &set) {
        (Ok(l), Ok(v)) => l.replay(v, &table).ok(),
        _ => None,
    };
    let cs2 = match &set {
        Ok(v) => VectorLedger::run(v, &table).ok().map(|l| l.checksum()),
        Err(_) => None,
    };
    let ok = match (cs1, cs2) {
        (Some(a), Some(b)) => a == b && a != FNV_OFFSET,
        _ => false,
    };
    let det = format!("{:?}/{:?}", cs1, cs2);
    s.add_dyn("50076-J1-校验和复跑", ok, &det);

    // J1-4 盒表变则账必漂：去掉 tray2 后 replay 必须 ChecksumDrift。
    let mut shrunk = TrayTable::new(4);
    let mut i4 = 1u8;
    while i4 <= 4 {
        if i4 != 2 {
            if let Some(t) = table.by_id(i4) {
                let _ = shrunk.insert(*t);
            }
        }
        i4 += 1;
    }
    let ok = shrunk.len() == 3
        && match (&led, &set) {
            (Ok(l), Ok(v)) => l.replay(v, &shrunk).err() == Some(PaperVecError::ChecksumDrift),
            _ => false,
        };
    s.add("50076-J1-表变账漂", ok, if ok { "" } else { "换表后校验和未变" });

    // J1-5 校验和灵敏度：追加一条期望盒号被写错的用例，账必须漂且只挂一条。
    let mut tampered = VectorSet::new(16);
    let base_cases = default_cases();
    let mut bi = 0usize;
    while bi < base_cases.len() {
        if let Some(c) = base_cases.get(bi) {
            let _ = tampered.add(*c);
        }
        bi += 1;
    }
    let mut tweaked = VectorCase::new(99, 210, 297, 80, MediaKind::Plain, 2);
    tweaked.id = 99;
    let _ = tampered.add(tweaked);
    let ok = match (&led, &tampered) {
        (Ok(l), v) if v.len() == 13 => match VectorLedger::run(v, &table) {
            Ok(l2) => l2.checksum != l.checksum && l2.failed() == 1,
            Err(_) => false,
        },
        _ => false,
    };
    s.add("50076-J1-改例账漂", ok, if ok { "" } else { "失败用例未进校验和" });

    // J1-6 选择必可装：所有选中的盒都真的 fits（12 条用例逐条验）。
    let mut fit_ok = match &led {
        Ok(_) => true,
        Err(_) => false,
    };
    if let Ok(v) = &set {
        let mut i6 = 0usize;
        while i6 < v.len() {
            if let Some(c) = v.case(i6) {
                match table.select(&c.req) {
                    Ok(id) => match table.by_id(id) {
                        Some(t) => {
                            if !t.fits(&c.req) {
                                fit_ok = false;
                            }
                        }
                        None => fit_ok = false,
                    },
                    Err(_) => fit_ok = false,
                }
            } else {
                fit_ok = false;
            }
            i6 += 1;
        }
    }
    s.add("50076-J1-选中必可装", fit_ok, if fit_ok { "" } else { "选出了装不下的盒" });

    // J1-7 最小性：网格请求逐个验证「不存在更小面积的可用盒」，同时验证
    // 「无可用盒 ⇔ 返回 NoFit」（选择器完备性）。
    let widths = [100u16, 148, 210, 216, 220];
    let heights = [110u16, 210, 279, 297, 300];
    let weights = [60u16, 120, 200, 240, 300];
    let kinds = [
        MediaKind::Plain,
        MediaKind::Bond,
        MediaKind::Label,
        MediaKind::Envelope,
    ];
    let mut grid_ok = true;
    let mut tested = 0usize;
    let mut wi = 0usize;
    while wi < widths.len() {
        if let Some(w) = widths.get(wi) {
            let mut hi = 0usize;
            while hi < heights.len() {
                if let Some(h) = heights.get(hi) {
                    let mut gi = 0usize;
                    while gi < weights.len() {
                        if let Some(g) = weights.get(gi) {
                            let mut ki = 0usize;
                            while ki < kinds.len() {
                                if let Some(k) = kinds.get(ki) {
                                    let req = MediaRequest {
                                        w_mm: *w,
                                        h_mm: *h,
                                        gsm: *g,
                                        kind: *k,
                                    };
                                    tested += 1;
                                    match table.select(&req) {
                                        Ok(id) => {
                                            let chosen_area = match table.by_id(id) {
                                                Some(t) => t.area(),
                                                None => {
                                                    grid_ok = false;
                                                    0
                                                }
                                            };
                                            let mut j = 0usize;
                                            while j < table.len() {
                                                if let Some(t) = table.by_id((j + 1) as u8) {
                                                    if t.fits(&req) && t.area() < chosen_area {
                                                        grid_ok = false;
                                                    }
                                                }
                                                j += 1;
                                            }
                                        }
                                        Err(_) => {
                                            if table.eligible(&req) != 0 {
                                                grid_ok = false;
                                            }
                                        }
                                    }
                                }
                                ki += 1;
                            }
                        }
                        gi += 1;
                    }
                }
                hi += 1;
            }
        }
        wi += 1;
    }
    let det = format!("grid={} ok={}", tested, grid_ok);
    s.add_dyn("50076-J1-最小性完备", grid_ok, &det);

    // J1-8 旋转适配：110×220 信封只能靠旋转进 220×110 的盒 4。
    let env = MediaRequest {
        w_mm: 110,
        h_mm: 220,
        gsm: 100,
        kind: MediaKind::Envelope,
    };
    let ok = table.select(&env).ok() == Some(4)
        && match table.by_id(4) {
            Some(t) => t.fits(&env),
            None => false,
        };
    s.add("50076-J1-旋转适配", ok, if ok { "" } else { "旋转适配失效" });

    // J1-9 小盒优先：A5 作业必须落 2 号盒而不是 1 号（面积序）。
    let a5 = MediaRequest {
        w_mm: 148,
        h_mm: 210,
        gsm: 80,
        kind: MediaKind::Plain,
    };
    let big = MediaRequest {
        w_mm: 210,
        h_mm: 297,
        gsm: 80,
        kind: MediaKind::Plain,
    };
    let ok = table.select(&a5).ok() == Some(2) && table.select(&big).ok() == Some(1);
    s.add("50076-J1-小盒优先", ok, if ok { "" } else { "未按面积选盒" });

    // J1-10 无盒必拒：400×400 / 超大克重 / 独占介质三条都必须 NoFit 或相应拒。
    let huge = MediaRequest {
        w_mm: 400,
        h_mm: 400,
        gsm: 100,
        kind: MediaKind::Plain,
    };
    let heavy = MediaRequest {
        w_mm: 210,
        h_mm: 297,
        gsm: 400,
        kind: MediaKind::Plain,
    };
    let trans = MediaRequest {
        w_mm: 210,
        h_mm: 297,
        gsm: 100,
        kind: MediaKind::Transparency,
    };
    let ok = table.select(&huge).err() == Some(PaperVecError::NoFit)
        && table.select(&heavy).err() == Some(PaperVecError::NoFit)
        && table.select(&trans).err() == Some(PaperVecError::NoFit);
    s.add("50076-J1-无盒必拒", ok, if ok { "" } else { "不可装请求被接受" });

    // J1-11 入参闭集：坏尺寸 / 坏克重 / 坏介质码各自命中。
    let bad_w = MediaRequest {
        w_mm: 0,
        h_mm: 210,
        gsm: 80,
        kind: MediaKind::Plain,
    };
    let bad_g = MediaRequest {
        w_mm: 210,
        h_mm: 297,
        gsm: 500,
        kind: MediaKind::Plain,
    };
    let ok = table.select(&bad_w).err() == Some(PaperVecError::BadDims)
        && table.select(&bad_g).err() == Some(PaperVecError::BadWeight)
        && MediaKind::from_code(7).err() == Some(PaperVecError::BadTray);
    s.add("50076-J1-入参闭集", ok, if ok { "" } else { "越界入参被接受" });

    // J1-12 用例集定容 / 去重 / 封账。
    let mut tiny = VectorSet::new(1);
    let one = VectorCase::new(1, 148, 210, 80, MediaKind::Plain, 2);
    let dup_case = VectorCase::new(1, 210, 297, 80, MediaKind::Plain, 1);
    let two = VectorCase::new(2, 210, 297, 80, MediaKind::Plain, 1);
    let ok = tiny.add(one).is_ok()
        && tiny.add(dup_case).err() == Some(PaperVecError::Duplicate)
        && tiny.add(two).err() == Some(PaperVecError::SetFull)
        && tiny.len() == 1;
    tiny.seal();
    let ok = ok
        && tiny.add(VectorCase::new(3, 148, 210, 80, MediaKind::Plain, 2)).err()
            == Some(PaperVecError::Sealed)
        && tiny.is_sealed();
    s.add("50076-J1-用例集闸", ok, if ok { "" } else { "定容/去重/封账失效" });

    // J1-13 盒表闸：重复号 / 定容 / 封账 / 坏尺寸。
    let mut tt = TrayTable::new(1);
    let t1 = Tray {
        id: 1,
        w_mm: 210,
        h_mm: 297,
        max_gsm: 300,
        kinds: MediaKind::Plain.bit(),
    };
    let dup_tray = Tray { id: 1, ..t1 };
    let t2 = Tray { id: 2, ..t1 };
    let bad_dims = Tray {
        id: 3,
        w_mm: 0,
        h_mm: 297,
        max_gsm: 300,
        kinds: MediaKind::Plain.bit(),
    };
    let ok = tt.insert(t1).is_ok()
        && tt.insert(dup_tray).err() == Some(PaperVecError::Duplicate)
        && tt.insert(t2).err() == Some(PaperVecError::SetFull)
        && bad_dims.validate().err() == Some(PaperVecError::BadDims)
        && tt.len() == 1;
    tt.seal();
    let ok = ok && tt.insert(t2).err() == Some(PaperVecError::Sealed) && tt.is_sealed();
    s.add("50076-J1-盒表闸", ok, if ok { "" } else { "盒表闸门失效" });

    // J1-14 账本定容 + 封账：第 3 条 LedgerFull，seal 后 Sealed。
    let mut small = VectorLedger::new(2);
    let r = CaseResult {
        id: 1,
        chosen: 1,
        expected: 1,
        pass: true,
    };
    let ok = small.push(r).is_ok()
        && small.push(CaseResult { id: 2, ..r }).is_ok()
        && small.push(CaseResult { id: 3, ..r }).err() == Some(PaperVecError::LedgerFull)
        && small.len() == 2;
    small.seal();
    let ok = ok
        && small.push(CaseResult { id: 4, ..r }).err() == Some(PaperVecError::Sealed)
        && small.len() == 2;
    s.add("50076-J1-账本闸", ok, if ok { "" } else { "账本闸门失效" });

    // J1-15 六类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50076-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50076_checks_green() {
        let set = run_unx50076_checks();
        assert!(set.all_passed(), "UNX-F50076 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50076_twelve_cases_replay() {
        let table = default_trays();
        let set = default_set();
        assert!(set.is_ok());
        if let Ok(v) = set {
            let a = VectorLedger::run(&v, &table);
            let b = VectorLedger::run(&v, &table);
            assert!(a.is_ok() && b.is_ok());
            if let (Ok(x), Ok(y)) = (a, b) {
                assert_eq!(x.passed(), 12);
                assert_eq!(x.checksum(), y.checksum());
                assert_eq!(x.replay(&v, &table).ok(), Some(x.checksum()));
            }
        }
    }

    #[test]
    fn unx50076_selection_is_minimal() {
        let table = default_trays();
        let req = MediaRequest {
            w_mm: 148,
            h_mm: 210,
            gsm: 80,
            kind: MediaKind::Plain,
        };
        let id = table.select(&req);
        assert_eq!(id.ok(), Some(2));
        if let Ok(i) = id {
            if let Some(t) = table.by_id(i) {
                assert!(t.fits(&req));
            }
        }
    }

    #[test]
    fn unx50076_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 6);
    }
}
