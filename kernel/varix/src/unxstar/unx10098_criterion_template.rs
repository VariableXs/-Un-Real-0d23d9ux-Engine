//! UNX-F10098 E 型验收判据模板（统一句式）· 0x92840 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10099-J1 衔接核（E 型判据接入 F9900 聚合 10/10 次），脱聚合 10/10 次检出
//!
//! SEG_BASE = 0x92840（本卡独占 0x40 窗口，下一卡 0x928C0）；
//! 诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器：J1/J2 统一句式模板解析器 —— J1 形制 `<主题>达成（<依据>
//!      10/10 次）`、J2 形制 `<误用>受控（预期 <ERRNO> 精确复现 10/10 次）`；
//!      脱句式 / 主题空 / 轮数漂移 逐槽拒绝，ERRNO 必须落在对照闭集。
//!   ② 注册表：判据登记表 CriterionLedger，逐 ID 双侧（J1/J2）成对登记；
//!      对账审计要求每 ID 双侧齐备且同号。
//!   ③ 账本：登记账（ID → 双侧槽位），J2 侧必须携带表内 ERRNO。
//!   ④ 注入核：脱句式 J1 / 脱句式 J2 / 轮数漂移 / 野错误码 四类坏样本
//!      逐类 10/10 次拒绝，注入账 注入=检出+逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10098 独占 0x40 窗口，下一卡 0x928C0）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x92840;

/// 注入复测轮数（判据要求 10/10；句式轮数槽固定同源）。
pub const INJECT_ROUNDS: u32 = 10;

/// 判据登记表容量上限（ID 数）。
pub const LEDGER_CAP: usize = 48;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TmplCode {
    /// 不符合 J1 句式
    NotJ1Form,
    /// 不符合 J2 句式
    NotJ2Form,
    /// 主题/误用槽为空
    EmptySlot,
    /// 轮数槽漂移（非 10/10）
    BadRounds,
    /// ERRNO 不在对照闭集
    UnknownErrno,
    /// 句式种别未知（非 j1/j2 前缀）
    UnknownKind,
    /// 缺对侧判据
    MissingCounterpart,
    /// 同侧重登
    DupSide,
    /// 零号 ID
    ZeroId,
    /// 登记表满
    LedgerFull,
    /// ERRNO 标签往返不一致
    ErrnoRoundTrip,
    /// 段位越界
    SegmentViolation,
}

impl TmplCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            TmplCode::NotJ1Form => "脱 J1 句式被拒：正判据必须形如 \
                <主题>达成（<依据> 10/10 次）；请按统一句式重写",
            TmplCode::NotJ2Form => "脱 J2 句式被拒：反判据必须形如 \
                <误用>受控（预期 <ERRNO> 精确复现 10/10 次）；请按统一句式重写",
            TmplCode::EmptySlot => "槽位为空被拒：主题/误用/依据槽不得为空；\
                空槽即无据判据",
            TmplCode::BadRounds => "轮数漂移被拒：轮数槽必须恰为 10/10；\
                漂移轮数即口径漂移",
            TmplCode::UnknownErrno => "野错误码被拒：ERRNO 必须落在对照闭集；\
                表外码不得入句式",
            TmplCode::UnknownKind => "种别未知被拒：判据必须以 j1:/j2: 前缀声明种别；\
                其余前缀一律拒绝",
            TmplCode::MissingCounterpart => "缺对侧被拒：每 ID 必须 J1/J2 成对；\
                单侧条目审计必须检出",
            TmplCode::DupSide => "同侧重登被拒：同 ID 同侧只能登记一次；\
                修订请走重测",
            TmplCode::ZeroId => "零号 ID：零是保留键，禁止入表；请换用正数 ID",
            TmplCode::LedgerFull => "登记表满：已达容量上限；请先归档旧表再续",
            TmplCode::ErrnoRoundTrip => "ERRNO 往返不一致：label 再解析必须回到原码；\
                请检查闭集文案与解析器是否同步",
            TmplCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [TmplCode; 12] = [
        TmplCode::NotJ1Form,
        TmplCode::NotJ2Form,
        TmplCode::EmptySlot,
        TmplCode::BadRounds,
        TmplCode::UnknownErrno,
        TmplCode::UnknownKind,
        TmplCode::MissingCounterpart,
        TmplCode::DupSide,
        TmplCode::ZeroId,
        TmplCode::LedgerFull,
        TmplCode::ErrnoRoundTrip,
        TmplCode::SegmentViolation,
    ];
}

// ── ERRNO 对照闭集 ────────────────────────────────────────────────────
/// J2 句式允许的 ERRNO 闭集：(名, 正数值)。
pub const TMPL_ERRNOS: &[(&str, i32)] = &[
    ("EBADF", 9),
    ("ENOENT", 2),
    ("EINVAL", 22),
    ("EPIPE", 32),
    ("EFAULT", 14),
    ("EPERM", 1),
];

/// 按名查码；表外 None。
pub fn tmpl_errno(name: &str) -> Option<i32> {
    let mut i = 0usize;
    while i < TMPL_ERRNOS.len() {
        if let Some(row) = TMPL_ERRNOS.get(i) {
            if row.0 == name {
                return Some(row.1);
            }
        }
        i += 1;
    }
    None
}

/// 按码查名；表外 None。
pub fn tmpl_errno_name(v: i32) -> Option<&'static str> {
    let mut i = 0usize;
    while i < TMPL_ERRNOS.len() {
        if let Some(row) = TMPL_ERRNOS.get(i) {
            if row.1 == v {
                return Some(row.0);
            }
        }
        i += 1;
    }
    None
}

// ── J1 句式解析器 ────────────────────────────────────────────────────
/// J1 槽位：主题 + 依据。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct J1Slot {
    /// 主题（达成面）
    pub subject: String,
    /// 依据（轮数前的口径文）
    pub evidence: String,
}

/// J1 句式解析：形制 `<主题>达成（<依据> 10/10 次）`。
/// 主题非空、依据非空、轮数槽恰为 `10/10 次`，任一违反逐因拒绝。
pub fn parse_j1(text: &str) -> Result<J1Slot, TmplCode> {
    let tail = "10/10 次）";
    if !text.ends_with(tail) {
        // 尾部含“次）”但轮数漂移 → BadRounds；否则脱句式。
        if text.ends_with("次）") {
            return Err(TmplCode::BadRounds);
        }
        return Err(TmplCode::NotJ1Form);
    }
    let head = match text.get(..text.len() - tail.len()) {
        Some(h) => h,
        None => return Err(TmplCode::NotJ1Form),
    };
    let mid = match head.find("达成（") {
        Some(p) => p,
        None => return Err(TmplCode::NotJ1Form),
    };
    let subject = match head.get(..mid) {
        Some(s) => s,
        None => return Err(TmplCode::EmptySlot),
    };
    let evidence = match head.get(mid + "达成（".len()..) {
        Some(e) => e,
        None => return Err(TmplCode::EmptySlot),
    };
    if subject.is_empty() || evidence.is_empty() {
        return Err(TmplCode::EmptySlot);
    }
    // 依据槽允许句式自带的尾随空格（“…面 10/10 次”），解析时剥掉。
    Ok(J1Slot { subject: String::from(subject), evidence: String::from(evidence.trim_end()) })
}

// ── J2 句式解析器 ────────────────────────────────────────────────────
/// J2 槽位：误用 + 预期 ERRNO。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct J2Slot {
    /// 误用路径主题
    pub misuse: String,
    /// 预期 ERRNO（正数）
    pub errno: i32,
}

/// J2 句式解析：形制 `<误用>受控（预期 <ERRNO> 精确复现 10/10 次）`。
/// 误用非空；ERRNO 必须落在对照闭集；轮数槽恰为 10/10。
pub fn parse_j2(text: &str) -> Result<J2Slot, TmplCode> {
    let tail = "10/10 次）";
    if !text.ends_with(tail) {
        if text.ends_with("次）") {
            return Err(TmplCode::BadRounds);
        }
        return Err(TmplCode::NotJ2Form);
    }
    let mid = match text.find("受控（预期 ") {
        Some(p) => p,
        None => return Err(TmplCode::NotJ2Form),
    };
    let misuse = match text.get(..mid) {
        Some(m) => m,
        None => return Err(TmplCode::EmptySlot),
    };
    if misuse.is_empty() {
        return Err(TmplCode::EmptySlot);
    }
    let after = match text.get(mid + "受控（预期 ".len()..) {
        Some(a) => a,
        None => return Err(TmplCode::NotJ2Form),
    };
    // ERRNO 段：到 " 精确复现" 为止。
    let end = match after.find(" 精确复现") {
        Some(p) => p,
        None => return Err(TmplCode::NotJ2Form),
    };
    let name = match after.get(..end) {
        Some(n) => n,
        None => return Err(TmplCode::NotJ2Form),
    };
    if name.is_empty() || name.bytes().any(|b| b == b' ') {
        return Err(TmplCode::NotJ2Form);
    }
    match tmpl_errno(name) {
        Some(e) => Ok(J2Slot { misuse: String::from(misuse), errno: e }),
        None => Err(TmplCode::UnknownErrno),
    }
}

// ── 判据登记表 ────────────────────────────────────────────────────────
/// 判据种别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SideKind {
    /// 正判据
    J1,
    /// 反判据
    J2,
}

impl SideKind {
    /// 前缀。
    pub fn prefix(self) -> &'static str {
        match self {
            SideKind::J1 => "j1:",
            SideKind::J2 => "j2:",
        }
    }
}

/// 登记条目：ID + 双侧句式槽。
#[derive(Clone, Debug)]
pub struct TmplRecord {
    /// 条目号
    pub id: u32,
    /// 正判据槽
    pub j1: Option<J1Slot>,
    /// 反判据槽
    pub j2: Option<J2Slot>,
}

/// 判据登记表：逐 ID 成对登记，双侧齐备方过审计。
#[derive(Clone, Debug)]
pub struct TmplLedger {
    recs: Vec<TmplRecord>,
    cap: usize,
}

impl TmplLedger {
    /// 空表构造。
    pub fn new() -> TmplLedger {
        TmplLedger { recs: Vec::new(), cap: LEDGER_CAP }
    }

    /// 登记：text 必须带 j1:/j2: 前缀并按对应模板解析通过。
    /// 同 ID 双侧成对；同侧重登 / 缺前缀 / 野码 逐因拒绝。
    pub fn enroll(&mut self, id: u32, text: &str) -> Result<SideKind, TmplCode> {
        if id == 0 {
            return Err(TmplCode::ZeroId);
        }
        if text.starts_with("j1:") {
            let body = match text.get(3..) {
                Some(b) => b,
                None => return Err(TmplCode::NotJ1Form),
            };
            let slot = parse_j1(body)?;
            self.put_j1(id, slot)?;
            Ok(SideKind::J1)
        } else if text.starts_with("j2:") {
            let body = match text.get(3..) {
                Some(b) => b,
                None => return Err(TmplCode::NotJ2Form),
            };
            let slot = parse_j2(body)?;
            self.put_j2(id, slot)?;
            Ok(SideKind::J2)
        } else {
            Err(TmplCode::UnknownKind)
        }
    }

    fn put_j1(&mut self, id: u32, slot: J1Slot) -> Result<(), TmplCode> {
        let mut idx: Option<usize> = None;
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.id == id {
                    idx = Some(i);
                }
            }
            i += 1;
        }
        if let Some(ix) = idx {
            let rec = match self.recs.get_mut(ix) {
                Some(r) => r,
                None => return Err(TmplCode::ZeroId),
            };
            if rec.j1.is_some() {
                return Err(TmplCode::DupSide);
            }
            rec.j1 = Some(slot);
            return Ok(());
        }
        if self.recs.len() >= self.cap {
            return Err(TmplCode::LedgerFull);
        }
        let mut rec = TmplRecord { id, j1: None, j2: None };
        rec.j1 = Some(slot);
        self.recs.push(rec);
        Ok(())
    }

    fn put_j2(&mut self, id: u32, slot: J2Slot) -> Result<(), TmplCode> {
        let mut idx: Option<usize> = None;
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.id == id {
                    idx = Some(i);
                }
            }
            i += 1;
        }
        if let Some(ix) = idx {
            let rec = match self.recs.get_mut(ix) {
                Some(r) => r,
                None => return Err(TmplCode::ZeroId),
            };
            if rec.j2.is_some() {
                return Err(TmplCode::DupSide);
            }
            rec.j2 = Some(slot);
            return Ok(());
        }
        if self.recs.len() >= self.cap {
            return Err(TmplCode::LedgerFull);
        }
        let mut rec = TmplRecord { id, j1: None, j2: None };
        rec.j2 = Some(slot);
        self.recs.push(rec);
        Ok(())
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    /// 全表审计：每 ID 双侧齐备。
    pub fn all_paired(&self) -> bool {
        let mut i = 0usize;
        while i < self.recs.len() {
            match self.recs.get(i) {
                None => return false,
                Some(r) => {
                    if r.j1.is_none() || r.j2.is_none() {
                        return false;
                    }
                }
            }
            i += 1;
        }
        true
    }

    /// 全表 J2 ERRNO 都在闭集（野码 = 0 条；登记时已把关，此处复核）。
    pub fn all_errno_in_table(&self) -> bool {
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if let Some(j2) = &r.j2 {
                    if tmpl_errno_name(j2.errno).is_none() {
                        return false;
                    }
                }
            }
            i += 1;
        }
        true
    }
}

// ── 注入核：坏句式样本矩阵 ────────────────────────────────────────────
/// 四类坏句式样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadTmpl {
    /// 脱句式 J1
    OffJ1,
    /// 脱句式 J2
    OffJ2,
    /// 轮数漂移
    BadRounds,
    /// 野错误码
    WildErrno,
}

impl BadTmpl {
    /// 闭集全量。
    pub const ALL: [BadTmpl; 4] = [
        BadTmpl::OffJ1,
        BadTmpl::OffJ2,
        BadTmpl::BadRounds,
        BadTmpl::WildErrno,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadTmpl::OffJ1 => "脱句式 J1",
            BadTmpl::OffJ2 => "脱句式 J2",
            BadTmpl::BadRounds => "轮数漂移",
            BadTmpl::WildErrno => "野错误码",
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

    /// 记一次注入结果；账失守恒返回 SegmentViolation 桥接码（账守恒检查）。
    pub fn record(&mut self, caught: bool) -> Result<(), TmplCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(TmplCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏句式注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_tmpl_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadTmpl::ALL.len() {
        let case = match BadTmpl::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadTmpl::OffJ1 => {
                    parse_j1("常规读 OK（10/10）").is_err()
                        && parse_j1("常规读已通过（10/10 次）").err() == Some(TmplCode::NotJ1Form)
                }
                BadTmpl::OffJ2 => {
                    parse_j2("坏 fd 拦下了（预期 EBADF 复现 10/10 次）")
                        .err() == Some(TmplCode::NotJ2Form)
                }
                BadTmpl::BadRounds => {
                    parse_j1("常规读达成（LTP 面 9/10 次）").err() == Some(TmplCode::BadRounds)
                        && parse_j2("坏 fd 受控（预期 EBADF 精确复现 8/10 次）")
                            .err() == Some(TmplCode::BadRounds)
                }
                BadTmpl::WildErrno => {
                    parse_j2("坏 fd 受控（预期 EHOPE 精确复现 10/10 次）")
                        .err() == Some(TmplCode::UnknownErrno)
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

// ── 判据成文（纯函数） ────────────────────────────────────────────────
/// 条目成文：「E<n>:J1=<有|无>;J2=<有|无>;码=<名|->」；账外先拒。
pub fn compose_tmpl_summary(l: &TmplLedger, id: u32) -> Result<String, TmplCode> {
    if l.is_empty() {
        return Err(TmplCode::ZeroId);
    }
    let mut i = 0usize;
    while i < l.recs.len() {
        if let Some(r) = l.recs.get(i) {
            if r.id == id {
                let errno_txt = match &r.j2 {
                    Some(j2) => match tmpl_errno_name(j2.errno) {
                        Some(n) => n,
                        None => "-",
                    },
                    None => "-",
                };
                let out = format!("E{}:J1={};J2={};码={}",
                    r.id,
                    if r.j1.is_some() { "有" } else { "无" },
                    if r.j2.is_some() { "有" } else { "无" },
                    errno_txt);
                if out.is_empty() {
                    return Err(TmplCode::ZeroId);
                }
                return Ok(out);
            }
        }
        i += 1;
    }
    Err(TmplCode::MissingCounterpart)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 9, "Windows ERROR_INVALID_HANDLE；坏 fd 句式面族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺路径句式面族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；非法句式/野码族"),
    ("EPIPE", -32, 232, "Windows ERROR_NO_DATA（管道断开）；断端写面族"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏缓冲面族"),
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

/// 规范语料双跑指纹：三 ID 双侧按统一句式登记 → 逐条成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = TmplLedger::new();
    let spec: [(u32, &str, &str); 3] = [
        (21, "j1:常规读达成（LTP read 面 10/10 次）",
             "j2:负 fd 误用受控（预期 EBADF 精确复现 10/10 次）"),
        (22, "j1:常规写达成（LTP write 面 10/10 次）",
             "j2:只读 fd 写误用受控（预期 EBADF 精确复现 10/10 次）"),
        (23, "j1:打开达成（标志组合面 10/10 次）",
             "j2:缺失路径误用受控（预期 ENOENT 精确复现 10/10 次）"),
    ];
    let mut si = 0usize;
    while si < spec.len() {
        if let Some(row) = spec.get(si) {
            let _ = l.enroll(row.0, row.1);
            let _ = l.enroll(row.0, row.2);
        }
        si += 1;
    }
    let mut bytes: Vec<u8> = Vec::new();
    let mut e = 0u32;
    while e < 3 {
        match compose_tmpl_summary(&l, 21 + e) {
            Ok(s) => {
                bytes.extend_from_slice(s.as_bytes());
                bytes.push(b';');
            }
            Err(_) => {}
        }
        e += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10098 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10098_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10098");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92840..=0x9287F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. J1 句式解析：合法全通过
    let j1_ok = match parse_j1("常规读达成（LTP read 面 10/10 次）") {
        Ok(sl) => sl.subject == "常规读" && sl.evidence == "LTP read 面",
        Err(_) => false,
    };
    s.add("J1 句式：<主题>达成（<依据> 10/10 次）全解析", j1_ok,
          "统一句式是 E 型判据的验收基座，逐槽命中");

    // 3. J2 句式解析：合法全通过且码落表
    let j2_ok = match parse_j2("负 fd 误用受控（预期 EBADF 精确复现 10/10 次）") {
        Ok(sl) => sl.misuse == "负 fd 误用" && sl.errno == 9,
        Err(_) => false,
    };
    s.add("J2 句式：<误用>受控（预期 <ERRNO> 精确复现 10/10 次）全解析", j2_ok,
          "反判据句式必须携带表内 ERRNO，脱句式退回");

    // 4. 注入核：四类坏句式样本各 10/10 拒绝
    let (inj, per_class) = run_bad_tmpl_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏句式样本各 10/10 拒绝", all_caught,
          "脱句式 J1/J2、轮数漂移、野错误码必须逐类逐次拒绝");

    // 5. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadTmpl::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 6. 脱句式退回：变体句式逐个拒绝 10/10
    let mut off_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_j1("常规读 达成 10/10").is_ok()
            || parse_j1("").is_ok()
            || parse_j2("坏 fd 已受控").is_ok()
            || parse_j1("达成（10/10 次）").is_ok()
        {
            off_ok = false;
        }
        r += 1;
    }
    s.add("脱句式退回：变体/空串/缺槽 10/10 拒绝", off_ok,
          "统一句式逐字匹配，变体一律退回重写");

    // 7. ERRNO 闭集：名码双向可查且野码拒绝
    let errno_ok = tmpl_errno("EBADF") == Some(9)
        && tmpl_errno_name(22) == Some("EINVAL")
        && tmpl_errno("EHOPE").is_none()
        && tmpl_errno_name(999).is_none();
    s.add("ERRNO 闭集：六码双向可查且表外拒绝", errno_ok,
          "句式错误码必须落在对照闭集，野码不得入句式");

    // 8. 种别前缀守卫：无前缀/错前缀拒绝
    let mut l = TmplLedger::new();
    let kind_ok = l.enroll(1, "常规读达成（LTP 面 10/10 次）").err() == Some(TmplCode::UnknownKind)
        && l.enroll(1, "j3:常规读达成（LTP 面 10/10 次）").err() == Some(TmplCode::UnknownKind)
        && l.enroll(0, "j1:常规读达成（LTP 面 10/10 次）").err() == Some(TmplCode::ZeroId);
    s.add("种别守卫：j1:/j2: 前缀强制，无/错前缀与零号拒绝", kind_ok,
          "句式种别由前缀声明，缺前缀即不可归类");

    // 9. 双侧成对登记 + 全表审计
    let e1 = l.enroll(11, "j1:常规读达成（LTP read 面 10/10 次）");
    let e2 = l.enroll(11, "j2:负 fd 误用受控（预期 EBADF 精确复现 10/10 次）");
    let pair_ok = e1 == Ok(SideKind::J1) && e2 == Ok(SideKind::J2)
        && l.len() == 1 && l.all_paired() && l.all_errno_in_table();
    s.add("双侧成对：J1/J2 同号登记且全表审计过", pair_ok,
          "统一句式成对落表，单侧条目审计必须检出");

    // 10. 同侧重登拒绝
    let dup_ok = l.enroll(11, "j1:常规读达成（另一面 10/10 次）").err() == Some(TmplCode::DupSide)
        && l.len() == 1;
    s.add("重登拦截：同 ID 同侧重登被拒不改表", dup_ok,
          "同侧唯一是登记表契约，重登即重复口径");

    // 11. 轮数漂移逐字拦截：9/10 与 10/11 都不入表
    let mut l2 = TmplLedger::new();
    let drift_ok = l2.enroll(21, "j1:常规读达成（LTP 面 9/10 次）").err() == Some(TmplCode::BadRounds)
        && l2.enroll(21, "j2:坏 fd 受控（预期 EBADF 精确复现 10/11 次）")
            .err() == Some(TmplCode::BadRounds)
        && l2.is_empty();
    s.add("轮数守卫：漂移轮数 10/10 拒绝且不入表", drift_ok,
          "轮数槽恰为 10/10，漂移即口径漂移，直接退回");

    // 12. 成文非空且含双侧与码标记 + 双跑一致
    let sum = compose_tmpl_summary(&l, 11);
    let text_ok = match &sum {
        Ok(t) => t.contains("J1=有") && t.contains("J2=有") && t.contains("码=EBADF"),
        Err(_) => false,
    };
    s.add("判据成文：双侧与码齐备且两次成文逐字节一致",
          text_ok && sum == compose_tmpl_summary(&l, 11),
          "成文必须携带 双侧/码 标记且为纯函数");

    // 13. 账外成文拒绝
    s.add("账外成文拒绝：未知 ID 不得产出小结",
          compose_tmpl_summary(&l, 999).err() == Some(TmplCode::MissingCounterpart)
              && compose_tmpl_summary(&TmplLedger::new(), 1).is_err(),
          "空表与账外 ID 都无对账对象，产出小结即虚报");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(9) && agrees_with_linux(2) && agrees_with_linux(22)
        && agrees_with_linux(32) && agrees_with_linux(14)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：句式闭集码全表内精确", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < TmplCode::ALL.len() {
        let mut j = i + 1;
        while j < TmplCode::ALL.len() {
            if let (Some(a), Some(b)) = (TmplCode::ALL.get(i), TmplCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = TmplCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. ERRNO 往返 + 双跑指纹
    let rt_ok = TMPL_ERRNOS.iter().all(|row| tmpl_errno_name(row.1) == Some(row.0));
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：ERRNO 名码可回查且双跑哈希相等", rt_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum { Ok(t) => t.len(), Err(_) => 0 };
    let sum_text = format!("句式小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn j1_template() {
        let sl = parse_j1("写盘达成（落盘计数面 10/10 次）").unwrap_or(J1Slot {
            subject: String::new(),
            evidence: String::new(),
        });
        assert_eq!(sl.subject, "写盘");
        assert_eq!(sl.evidence, "落盘计数面");
    }

    #[test]
    fn j1_rejects() {
        assert_eq!(parse_j1("").err(), Some(TmplCode::NotJ1Form));
        assert_eq!(parse_j1("常规读达成（9/10 次）").err(), Some(TmplCode::BadRounds));
        assert_eq!(parse_j1("常规读达成（10/10 次）").err(), Some(TmplCode::EmptySlot));
        assert_eq!(parse_j1("达成（面 10/10 次）").err(), Some(TmplCode::EmptySlot));
    }

    #[test]
    fn j2_template() {
        let sl = parse_j2("断端写误用受控（预期 EPIPE 精确复现 10/10 次）")
            .unwrap_or(J2Slot { misuse: String::new(), errno: 0 });
        assert_eq!(sl.errno, 32);
        assert!(parse_j2("断端写（预期 EPIPE 精确复现 10/10 次）").is_err());
    }

    #[test]
    fn errno_table_bidirectional() {
        assert_eq!(tmpl_errno("EINVAL"), Some(22));
        assert_eq!(tmpl_errno_name(1), Some("EPERM"));
        assert_eq!(tmpl_errno("EXXX"), None);
    }

    #[test]
    fn kind_prefix_enforced() {
        let mut l = TmplLedger::new();
        assert_eq!(l.enroll(1, "no prefix").err(), Some(TmplCode::UnknownKind));
        assert_eq!(l.enroll(0, "j1:x达成（y 10/10 次）").err(), Some(TmplCode::ZeroId));
    }

    #[test]
    fn pairing_audit() {
        let mut l = TmplLedger::new();
        let _ = l.enroll(2, "j1:读达成（面 10/10 次）");
        assert!(!l.all_paired());
        let _ = l.enroll(2, "j2:坏 fd 受控（预期 EBADF 精确复现 10/10 次）");
        assert!(l.all_paired() && l.all_errno_in_table());
    }

    #[test]
    fn dup_side_rejected() {
        let mut l = TmplLedger::new();
        let _ = l.enroll(3, "j1:读达成（面 10/10 次）");
        assert_eq!(l.enroll(3, "j1:读达成（面2 10/10 次）").err(), Some(TmplCode::DupSide));
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn bad_tmpl_matrix_green() {
        let (inj, per) = run_bad_tmpl_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn summary_marks() {
        let mut l = TmplLedger::new();
        let _ = l.enroll(4, "j1:读达成（面 10/10 次）");
        let s = compose_tmpl_summary(&l, 4).unwrap_or_default();
        assert!(s.contains("J2=无"));
        assert!(s.contains("码=-"));
        assert!(compose_tmpl_summary(&l, 9).is_err());
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(32));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(9), -9);
    }

    #[test]
    fn unx10098_checks_green() {
        let set = run_unx10098_checks();
        assert!(set.all_passed(), "UNX-F10098 red: {}", set.red_items_text());
    }
}
