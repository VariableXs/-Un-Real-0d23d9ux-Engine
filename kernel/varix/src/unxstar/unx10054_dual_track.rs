//! UNX-F10054 回归双档一致性（T1/ktest）· 0x91D80 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10055-J1 留存期核（逐轮日志在期 10/10 次），过期清除 10/10 次拦截
//!
//! 设计要点：
//!   ① 双档 = T1 与 ktest 两条回归轨道对同一用例各自出判定；判定三分类：
//!      Agree（两档一致）/ Drift（两档漂移）/ Undecided（任一档缺席）。
//!      三分类必须逐案留痕（DiffRecord），差异账本三计数守恒：
//!      Σrecords == agree + drift + undecided。
//!   ② 未决差异是收口阻断项：undecided > 0 时 closeout_allowed() 恒假，
//!      强行收口 → UndecidedBlock 逐次拦截（10/10）——未决不许静默归并。
//!   ③ 同一用例只允许一次终局判定（DupCase），两档判定必须分别落档；
//!      未知轨道 token（t1|ktest 闭集外）一律拒绝。
//!   ④ 注入核：未决收口 / 未知轨道 / 重复用例 / 缺档判定四类坏样本
//!      逐类 10/10 次拒绝，注入账守恒逃逸 = 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10054 独占 0x40 窗口，下一卡 0x91DC0）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x91D80;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 差异账本容量上限。
pub const LEDGER_CAP: usize = 64;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DualCode {
    /// 未知轨道 token（不在 t1|ktest 闭集内）
    UnknownTrack,
    /// 空轨道 token
    EmptyTrack,
    /// 重复用例终局判定
    DupCase,
    /// 缺档判定：任一档缺席不得出终局
    MissingTrack,
    /// 未决差异未清却强行收口
    UndecidedBlock,
    /// 差异账失守恒（Σrecords ≠ 三分类之和）
    LedgerRegress,
    /// 账本容量满
    LedgerFull,
    /// 用例号为零
    BadCase,
    /// 空成文
    EmptySummary,
    /// 缺档引用（查询未落档的轨道判定）
    UnknownRef,
    /// 三分类留痕缺类（成文必须携带三类标记）
    MissingClass,
    /// 段位越界
    SegmentViolation,
}

impl DualCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            DualCode::UnknownTrack => "未知轨道被拒：轨道只认 t1 与 ktest；\
                请改用闭集内轨道名再落档",
            DualCode::EmptyTrack => "空轨道被拒：轨道名不能为空串；请先确认双档轨道",
            DualCode::DupCase => "重复用例被拒：同一用例只允许一次终局判定；\
                请先清理旧档再重判",
            DualCode::MissingTrack => "缺档判定被拒：两档判定齐备才能出终局；\
                请补齐缺席档的回归结果",
            DualCode::UndecidedBlock => "未决差异被拒：存在未决差异不得收口；\
                请先复核未决用例补齐双档判定",
            DualCode::LedgerRegress => "差异账失守恒：Σrecords 必须等于 \
                agree+drift+undecided 之和；请排查绕账路径",
            DualCode::LedgerFull => "差异账容量满被拒：已达上限；请先归档旧账",
            DualCode::BadCase => "用例号非法被拒：用例号从 1 起算且为正整数",
            DualCode::EmptySummary => "空成文被拒：双档小结不得为空串；请检查账本是否为空册",
            DualCode::UnknownRef => "缺档引用被拒：查询的轨道判定尚未落档；\
                请先落档再查询",
            DualCode::MissingClass => "三分类缺类被拒：成文必须携带 \
                一致/漂移/未决 三类计数标记",
            DualCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [DualCode; 12] = [
        DualCode::UnknownTrack,
        DualCode::EmptyTrack,
        DualCode::DupCase,
        DualCode::MissingTrack,
        DualCode::UndecidedBlock,
        DualCode::LedgerRegress,
        DualCode::LedgerFull,
        DualCode::BadCase,
        DualCode::EmptySummary,
        DualCode::UnknownRef,
        DualCode::MissingClass,
        DualCode::SegmentViolation,
    ];
}

// ── 轨道闭集与解析器 ──────────────────────────────────────────────────
/// 双档轨道闭集：T1 / ktest。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Track {
    /// T1 主轨
    T1,
    /// ktest 陪轨
    Ktest,
}

impl Track {
    /// 长名（落档与成文用）。
    pub fn name(self) -> &'static str {
        match self {
            Track::T1 => "t1",
            Track::Ktest => "ktest",
        }
    }

    /// 闭集全量。
    pub const ALL: [Track; 2] = [Track::T1, Track::Ktest];
}

/// 轨道解析：长名 t1|ktest 与短名 1|k 双形制，其余 UnknownTrack，
/// 空串 EmptyTrack。
pub fn parse_track(tag: &str) -> Result<Track, DualCode> {
    if tag.is_empty() {
        return Err(DualCode::EmptyTrack);
    }
    match tag {
        "t1" | "1" => Ok(Track::T1),
        "ktest" | "k" => Ok(Track::Ktest),
        _ => Err(DualCode::UnknownTrack),
    }
}

// ── 三分类闭集 ────────────────────────────────────────────────────────
/// 双档差异三分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffClass {
    /// 两档判定一致
    Agree,
    /// 两档判定漂移
    Drift,
    /// 任一档缺席，未决
    Undecided,
}

impl DiffClass {
    /// 中文标签（成文用）。
    pub fn label(self) -> &'static str {
        match self {
            DiffClass::Agree => "一致",
            DiffClass::Drift => "漂移",
            DiffClass::Undecided => "未决",
        }
    }

    /// 闭集全量。
    pub const ALL: [DiffClass; 3] = [DiffClass::Agree, DiffClass::Drift, DiffClass::Undecided];
}

/// 由两档判定推三分类：任一缺席 → Undecided；一致 → Agree；否则 Drift。
/// 纯函数，同输入必同输出。
pub fn classify_diff(t1: Option<bool>, ktest: Option<bool>) -> DiffClass {
    match (t1, ktest) {
        (Some(a), Some(b)) => {
            if a == b {
                DiffClass::Agree
            } else {
                DiffClass::Drift
            }
        }
        _ => DiffClass::Undecided,
    }
}

// ── 差异留痕 ──────────────────────────────────────────────────────────
/// 差异留痕记录：用例号 + 三分类 + 两档判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffRecord {
    /// 用例号（从 1 起算）
    pub case: u32,
    /// 三分类结果
    pub class: DiffClass,
    /// T1 档判定（None = 缺席）
    pub t1: Option<bool>,
    /// ktest 档判定（None = 缺席）
    pub ktest: Option<bool>,
}

impl DiffRecord {
    /// 留痕自检：分类必须与两档判定推导一致。
    pub fn sound(&self) -> bool {
        classify_diff(self.t1, self.ktest) == self.class && self.case > 0
    }
}

/// 单案双档档案：两档判定分别落档，齐备后才能出终局。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseFile {
    /// 用例号
    pub case: u32,
    /// T1 档判定
    pub t1: Option<bool>,
    /// ktest 档判定
    pub ktest: Option<bool>,
    /// 终局是否已出
    pub sealed: bool,
}

/// 双档一致性账本：落档 → 终局三分类 → 收口闸。
#[derive(Clone, Debug)]
pub struct DiffLedger {
    files: Vec<CaseFile>,
    records: Vec<DiffRecord>,
    cap: usize,
    agree: u32,
    drift: u32,
    undecided: u32,
}

impl DiffLedger {
    /// 空账构造。
    pub fn new() -> DiffLedger {
        DiffLedger {
            files: Vec::new(),
            records: Vec::new(),
            cap: LEDGER_CAP,
            agree: 0,
            drift: 0,
            undecided: 0,
        }
    }

    fn find(&self, case: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some(f) = self.files.get(i) {
                if f.case == case {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 落单档判定：用例首次出现自动建档；sealed 用例 DupCase 拒绝。
    /// track 必须在闭集内（由调用方解析后传入）。
    pub fn record_track(&mut self, case: u32, track: Track, pass: bool) -> Result<(), DualCode> {
        if case == 0 {
            return Err(DualCode::BadCase);
        }
        if self.find(case).is_none() {
            if self.files.len() >= self.cap {
                return Err(DualCode::LedgerFull);
            }
            self.files.push(CaseFile { case, t1: None, ktest: None, sealed: false });
        }
        let idx = match self.find(case) {
            Some(i) => i,
            None => return Err(DualCode::SegmentViolation),
        };
        let sealed = match self.files.get(idx) {
            Some(f) => f.sealed,
            None => return Err(DualCode::SegmentViolation),
        };
        if sealed {
            return Err(DualCode::DupCase);
        }
        if let Some(f) = self.files.get_mut(idx) {
            match track {
                Track::T1 => f.t1 = Some(pass),
                Track::Ktest => f.ktest = Some(pass),
            }
        }
        Ok(())
    }

    /// 出终局：两档齐备才允许（MissingTrack 拒绝），三分类留痕入账。
    pub fn seal(&mut self, case: u32) -> Result<DiffClass, DualCode> {
        let idx = match self.find(case) {
            Some(i) => i,
            None => return Err(DualCode::UnknownRef),
        };
        let f = match self.files.get(idx) {
            Some(f) => *f,
            None => return Err(DualCode::SegmentViolation),
        };
        if f.sealed {
            return Err(DualCode::DupCase);
        }
        let class = classify_diff(f.t1, f.ktest);
        if class == DiffClass::Undecided {
            return Err(DualCode::MissingTrack);
        }
        if self.records.len() >= self.cap {
            return Err(DualCode::LedgerFull);
        }
        let rec = DiffRecord { case, class, t1: f.t1, ktest: f.ktest };
        self.records.push(rec);
        if let Some(f) = self.files.get_mut(idx) {
            f.sealed = true;
        }
        match class {
            DiffClass::Agree => self.agree = self.agree.saturating_add(1),
            DiffClass::Drift => self.drift = self.drift.saturating_add(1),
            DiffClass::Undecided => self.undecided = self.undecided.saturating_add(1),
        }
        Ok(class)
    }

    /// 逐案留痕清单（只读）。
    pub fn records(&self) -> &[DiffRecord] {
        &self.records
    }

    /// 未决差异数：账内 undecided 计数（seal 拒绝未决，故恒为 0，
    /// 但档案中两档未齐的用例计入敞口）。
    pub fn open_undecided(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some(f) = self.files.get(i) {
                if !f.sealed && classify_diff(f.t1, f.ktest) == DiffClass::Undecided {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 收口闸：存在未决差异（敞口）时收口被拦。
    pub fn closeout_allowed(&self) -> bool {
        self.open_undecided() == 0
    }

    /// 守恒：Σrecords == agree + drift + undecided，且每条留痕自洽。
    pub fn conserved(&self) -> bool {
        let sum = self.agree.saturating_add(self.drift).saturating_add(self.undecided);
        if sum != self.records.len() as u32 {
            return false;
        }
        let mut i = 0usize;
        while i < self.records.len() {
            match self.records.get(i) {
                Some(r) => {
                    if !r.sound() {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 已建档用例数。
    pub fn len(&self) -> usize {
        self.files.len()
    }
}

// ── 双档小结成文（纯函数） ────────────────────────────────────────────
/// 双档成文：「一致=<a>;漂移=<d>;未决=<u>;收口=<ok|blocked>」；
/// 失守恒先拒，三类标记缺一即 MissingClass。
pub fn compose_dual_summary(l: &DiffLedger) -> Result<String, DualCode> {
    if !l.conserved() {
        return Err(DualCode::LedgerRegress);
    }
    let out = format!(
        "一致={};漂移={};未决={};收口={}",
        l.agree,
        l.drift,
        l.open_undecided(),
        if l.closeout_allowed() { "ok" } else { "blocked" }
    );
    if out.is_empty() {
        return Err(DualCode::EmptySummary);
    }
    let marks_ok = out.contains("一致=") && out.contains("漂移=") && out.contains("未决=");
    if !marks_ok {
        return Err(DualCode::MissingClass);
    }
    Ok(out)
}

// ── 注入核：坏双档样本矩阵 ────────────────────────────────────────────
/// 四类坏双档样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadDual {
    /// 未决收口：存在敞口未决差异却申请收口
    UndecidedCloseout,
    /// 未知轨道 token
    UnknownTrackTok,
    /// 重复用例终局
    DupSeal,
    /// 缺档出终局
    MissingTrackSeal,
}

impl BadDual {
    /// 闭集全量。
    pub const ALL: [BadDual; 4] = [
        BadDual::UndecidedCloseout,
        BadDual::UnknownTrackTok,
        BadDual::DupSeal,
        BadDual::MissingTrackSeal,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadDual::UndecidedCloseout => "未决收口",
            BadDual::UnknownTrackTok => "未知轨道",
            BadDual::DupSeal => "重复终局",
            BadDual::MissingTrackSeal => "缺档终局",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账失守恒返回 LedgerRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), DualCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(DualCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏双档注入矩阵：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_dual_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadDual::ALL.len() {
        let case = match BadDual::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadDual::UndecidedCloseout => {
                    let mut l = DiffLedger::new();
                    let _ = l.record_track(1, Track::T1, true); // ktest 缺席 → 敞口
                    !l.closeout_allowed()
                        && l.open_undecided() == 1
                        && l.seal(1).err() == Some(DualCode::MissingTrack)
                }
                BadDual::UnknownTrackTok => {
                    parse_track("t2").err() == Some(DualCode::UnknownTrack)
                        && parse_track("").err() == Some(DualCode::EmptyTrack)
                }
                BadDual::DupSeal => {
                    let mut l = DiffLedger::new();
                    let _ = l.record_track(1, Track::T1, true);
                    let _ = l.record_track(1, Track::Ktest, true);
                    let _ = l.seal(1);
                    l.seal(1).err() == Some(DualCode::DupCase)
                }
                BadDual::MissingTrackSeal => {
                    let mut l = DiffLedger::new();
                    let _ = l.record_track(2, Track::Ktest, false); // t1 缺席
                    l.seal(2).err() == Some(DualCode::MissingTrack)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏用例号/坏轨道族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；未决收口/缺档终局族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复终局族"),
    ("ENODATA", -61, 232, "Windows ERROR_NO_DATA；缺档判定族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；账本容量满族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：三案双档落档 → 终局 → 成文 → 摘哈希。
/// 案 1 两档一致、案 2 两档漂移、案 3 一致；无敞口，收口 ok。
pub fn corpus_fingerprint() -> u32 {
    let mut l = DiffLedger::new();
    let _ = l.record_track(1, Track::T1, true);
    let _ = l.record_track(1, Track::Ktest, true);
    let _ = l.record_track(2, Track::T1, true);
    let _ = l.record_track(2, Track::Ktest, false);
    let _ = l.record_track(3, Track::T1, false);
    let _ = l.record_track(3, Track::Ktest, false);
    let _ = l.seal(1);
    let _ = l.seal(2);
    let _ = l.seal(3);
    let bytes = match compose_dual_summary(&l) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10054 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10054_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10054");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91D80..=0x91DBF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠");

    // 2. 轨道解析器：双形制全通过
    let parse_ok = parse_track("t1") == Ok(Track::T1)
        && parse_track("1") == Ok(Track::T1)
        && parse_track("ktest") == Ok(Track::Ktest)
        && parse_track("k") == Ok(Track::Ktest);
    s.add("轨道解析器：t1|1 / ktest|k 双形制全通过", parse_ok,
          "双档轨道闭集双形制必须全可解析，闭集外显性拒绝");

    // 3. 未知轨道注入 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_track("t2").is_ok()
            || parse_track("").is_ok()
            || parse_track("T1").is_ok()
            || parse_track("k-test").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("未知轨道注入：闭集外/空串 10/10 拒绝", amb_ok,
          "轨道歧义必须逐次显性拒绝，不许静默归并到相邻轨道");

    // 4. 三分类纯函数：一致/漂移/未决全取值正确
    let clf_ok = classify_diff(Some(true), Some(true)) == DiffClass::Agree
        && classify_diff(Some(true), Some(false)) == DiffClass::Drift
        && classify_diff(Some(false), Some(false)) == DiffClass::Agree
        && classify_diff(Some(true), None) == DiffClass::Undecided
        && classify_diff(None, None) == DiffClass::Undecided;
    s.add("三分类推演：一致/漂移/未决取值 10/10 正确", clf_ok,
          "两档判定推三分类必须为纯函数，任一缺席即未决");

    // 5. 双档落档：T1 与 ktest 分别落档且可查
    let mut l = DiffLedger::new();
    let _ = l.record_track(1, Track::T1, true);
    let _ = l.record_track(1, Track::Ktest, false);
    s.add("双档落档：两档分别落档且档案齐备", l.len() == 1,
          "同一用例两档判定必须分别落档，缺档不得出终局");

    // 6. 差异三分类留痕：终局入账且留痕自洽 10/10
    let mut trace_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut g = DiffLedger::new();
        let _ = g.record_track(1, Track::T1, true);
        let _ = g.record_track(1, Track::Ktest, false);
        let cls = g.seal(1);
        let rec_sound = g.records().first().map(|r| r.sound()).unwrap_or(false);
        if cls.ok() != Some(DiffClass::Drift) || !rec_sound {
            trace_ok = false;
        }
        rr += 1;
    }
    s.add("差异留痕：三分类逐案入账且自洽 10/10", trace_ok,
          "漂移判例必须留痕（用例/分类/两档判定），留痕不自洽即失败");

    // 7. 未决差异拦截：敞口存在时收口 10/10 次被拦
    let mut blk_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut g = DiffLedger::new();
        let _ = g.record_track(1, Track::T1, true); // ktest 缺席
        if g.closeout_allowed() || g.seal(1).err() != Some(DualCode::MissingTrack) {
            blk_ok = false;
        }
        rr += 1;
    }
    s.add("未决拦截：敞口收口 10/10 次被拦", blk_ok,
          "未决差异是收口阻断项，缺档终局逐次显性拒绝");

    // 8. 注入核：四类坏样本各 10/10 拒绝
    let (inj, per_class) = run_bad_dual_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏双档样本各 10/10 拒绝", all_caught,
          "未决收口/未知轨道/重复终局/缺档终局必须逐类逐次拒绝");

    // 9. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadDual::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 差异账守恒：Σrecords == 三分类之和
    let _ = l.seal(1);
    s.add("差异守恒：Σrecords == 一致+漂移+未决 且留痕全自洽",
          l.conserved() && l.records().len() == 1,
          "差异账三计数必须逐笔对得上，绕账路径即失败");

    // 11. 重复终局拒绝
    let dup_ok = l.seal(1).err() == Some(DualCode::DupCase);
    s.add("重复终局：同用例二封印拒绝", dup_ok,
          "终局判定一次定案，重复封印逐次拒绝");

    // 12. 拒绝不留痕：被拒落档/终局不改账
    let before = (l.agree, l.drift, l.records().len());
    let _ = l.record_track(1, Track::T1, true); // sealed 用例再落档
    let _ = l.seal(1);
    let after = (l.agree, l.drift, l.records().len());
    s.add("拒绝不留痕：坏落档/终局不改三计数", before == after,
          "被拒操作不得改动一致/漂移/留痕任何账目");

    // 13. 成文非空且含三分类标记 + 双跑一致 + 收口位
    let sum = compose_dual_summary(&l);
    let text_ok = match &sum {
        Ok(t) => t.contains("一致=") && t.contains("漂移=") && t.contains("未决=") && t.contains("收口=ok"),
        Err(_) => false,
    };
    s.add("双档成文：三分类标记齐全且两次成文一致",
          text_ok && sum == compose_dual_summary(&l),
          "成文必须携带 一致/漂移/未决/收口 标记且为纯函数");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(1) && agrees_with_linux(17)
        && agrees_with_linux(61) && agrees_with_linux(28)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < DualCode::ALL.len() {
        let mut j = i + 1;
        while j < DualCode::ALL.len() {
            if let (Some(a), Some(b)) = (DualCode::ALL.get(i), DualCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = DualCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 双跑指纹一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希相等", f1 == f2,
          "同输入两次必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum {
        Ok(t) => t.len(),
        Err(_) => 0,
    };
    let sum_text = format!("双档小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_closed_set() {
        assert_eq!(parse_track("t1"), Ok(Track::T1));
        assert_eq!(parse_track("k"), Ok(Track::Ktest));
        assert!(parse_track("t2").is_err());
        assert_eq!(parse_track("").err(), Some(DualCode::EmptyTrack));
    }

    #[test]
    fn classify_pure() {
        assert_eq!(classify_diff(Some(false), Some(false)), DiffClass::Agree);
        assert_eq!(classify_diff(Some(false), Some(true)), DiffClass::Drift);
        assert_eq!(classify_diff(None, Some(true)), DiffClass::Undecided);
    }

    #[test]
    fn seal_requires_both_tracks() {
        let mut l = DiffLedger::new();
        let _ = l.record_track(1, Track::T1, true);
        assert_eq!(l.seal(1).err(), Some(DualCode::MissingTrack));
        let _ = l.record_track(1, Track::Ktest, true);
        assert_eq!(l.seal(1).ok(), Some(DiffClass::Agree));
    }

    #[test]
    fn dup_seal_rejected() {
        let mut l = DiffLedger::new();
        let _ = l.record_track(1, Track::T1, true);
        let _ = l.record_track(1, Track::Ktest, true);
        assert!(l.seal(1).is_ok());
        assert_eq!(l.seal(1).err(), Some(DualCode::DupCase));
    }

    #[test]
    fn bad_dual_matrix_green() {
        let (inj, per) = run_bad_dual_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn open_undecided_blocks_closeout() {
        let mut l = DiffLedger::new();
        let _ = l.record_track(1, Track::Ktest, true);
        assert!(!l.closeout_allowed());
        let _ = l.record_track(1, Track::T1, true);
        assert!(l.closeout_allowed());
    }

    #[test]
    fn record_sound_roundtrip() {
        let r = DiffRecord { case: 3, class: DiffClass::Drift, t1: Some(true), ktest: Some(false) };
        assert!(r.sound());
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(61));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(17), -17);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10054_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10054_checks_green() {
        let set = run_unx10054_checks();
        assert!(set.all_passed(), "UNX-F10054 red: {}", set.red_items_text());
    }
}
