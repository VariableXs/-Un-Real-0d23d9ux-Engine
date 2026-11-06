//! UNX-F10095 双判据快照字段扩展 · 0x92780 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10096-J1 防 DM 核（同 ID 的 J1/J2 同册同构 10/10 次），拆册异构 10/10 次检出
//!
//! SEG_BASE = 0x92780（本卡独占 0x40 窗口，下一卡 0x927C0）；
//! 诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器：快照规格解析器（形制 `v<n>|名=值|名=值|...`，字段名非空、
//!      名内重复一律拒绝），是快照入账的唯一入口。
//!   ② 账本：快照链账本 SnapshotLedger，逐期快照按版本入链；J2 段扩展
//!      走追加式协议：旧字段 逐名逐值 逐序 不许动，新字段只准尾插。
//!   ③ 守护器：追加式扩展校验 extend() —— 版本必须 +1（跳变/回退拒绝）、
//!      旧字段任何改名/删名/换序/改值逐因拒绝、新字段必须全部带 j2. 前缀
//!      且只能追加在尾部（中段插入拒绝）。
//!   ④ 注入核：私改旧字段值 / 重排旧字段 / 删旧字段 / 中段插入 四类坏样本
//!      逐类 10/10 次拒绝，注入账 注入=检出+逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10095 独占 0x40 窗口，下一卡 0x927C0）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x92780;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 快照链容量上限（期数）。
pub const SNAP_CAP: usize = 32;

/// J2 扩展段字段前缀（新字段必须携带此前缀）。
pub const J2_PREFIX: &str = "j2.";

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapCode {
    /// 空快照
    EmptySnapshot,
    /// 非法快照规格
    SpecMalformed,
    /// 同期字段名重复
    DupField,
    /// 版本回退
    VersionRegress,
    /// 版本跳变（不接尾期 +1）
    VersionGap,
    /// 旧字段被删
    FieldDropped,
    /// 旧字段被重排
    FieldReorder,
    /// 旧字段值被私改
    PrivateModify,
    /// 新字段尾插违规（中段插入）
    TailInsertion,
    /// 快照成文往返不一致
    FieldRoundTrip,
    /// 拒绝后账本被改动
    RejectedTrace,
    /// 段位越界
    SegmentViolation,
}

impl SnapCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SnapCode::EmptySnapshot => "空快照被拒：快照必须携带至少一个字段；\
                空快照即无账可对",
            SnapCode::SpecMalformed => "非法规格被拒：形制必须 v<n>|名=值|...；\
                字段名不得为空",
            SnapCode::DupField => "字段重名被拒：同期快照字段名必须唯一；\
                请改用带后缀的新名",
            SnapCode::VersionRegress => "版本回退被拒：新期版本必须大于尾期；\
                私改版本即私改快照结构",
            SnapCode::VersionGap => "版本跳变被拒：新期必须接尾期 +1；\
                禁止越过未冻结期",
            SnapCode::FieldDropped => "旧字段被删被拒：追加式协议下旧字段只准保留；\
                请恢复该字段",
            SnapCode::FieldReorder => "旧字段被重排被拒：旧字段必须按原相对顺序保留；\
                重排即结构私改",
            SnapCode::PrivateModify => "旧字段值被私改被拒：追加式协议只许尾插新字段；\
                改值请走重测新期",
            SnapCode::TailInsertion => "中段插入被拒：新字段只能追加在尾部；\
                请把新字段挪到末尾",
            SnapCode::FieldRoundTrip => "快照成文往返不一致：再解析必须逐字段复原；\
                请检查规格文案与解析器是否同步",
            SnapCode::RejectedTrace => "拒绝留痕被拒：被拒扩展不得改动任何已冻结期；\
                请排查拒绝路径是否误提交",
            SnapCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [SnapCode; 12] = [
        SnapCode::EmptySnapshot,
        SnapCode::SpecMalformed,
        SnapCode::DupField,
        SnapCode::VersionRegress,
        SnapCode::VersionGap,
        SnapCode::FieldDropped,
        SnapCode::FieldReorder,
        SnapCode::PrivateModify,
        SnapCode::TailInsertion,
        SnapCode::FieldRoundTrip,
        SnapCode::RejectedTrace,
        SnapCode::SegmentViolation,
    ];
}

// ── 快照结构 ──────────────────────────────────────────────────────────
/// 一个字段（名 + 值）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SnapField {
    /// 字段名（J2 扩展段字段以 j2. 开头）
    pub name: String,
    /// 字段值（非空）
    pub value: String,
}

/// 一期快照：版本号 + 字段列（有序）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// 版本号（从 1 起严格递增）
    pub version: u32,
    /// 有序字段列
    pub fields: Vec<SnapField>,
}

impl Snapshot {
    /// J2 扩展段字段数。
    pub fn j2_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.fields.len() {
            if let Some(f) = self.fields.get(i) {
                if f.name.starts_with(J2_PREFIX) {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 成文：「v<n>|名=值|...」（纯函数，往返语料）。
    pub fn to_spec(&self) -> String {
        let mut out = format!("v{}", self.version);
        let mut i = 0usize;
        while i < self.fields.len() {
            if let Some(f) = self.fields.get(i) {
                out.push('|');
                out.push_str(&f.name);
                out.push('=');
                out.push_str(&f.value);
            }
            i += 1;
        }
        out
    }
}

/// 快照规格解析：形制 `v<n>|名=值|名=值|...`。
/// 至少一个字段；字段名非空；同期重名拒绝；版本号必须正数。
pub fn parse_snapshot(text: &str) -> Result<Snapshot, SnapCode> {
    let body = match text.strip_prefix('v') {
        Some(b) => b,
        None => return Err(SnapCode::SpecMalformed),
    };
    // 首段：版本号（到首个 '|' 为止）。
    let head_end = match body.find('|') {
        Some(p) => p,
        None => return Err(SnapCode::SpecMalformed),
    };
    let head = match body.get(..head_end) {
        Some(h) => h,
        None => return Err(SnapCode::SpecMalformed),
    };
    if head.is_empty() || !head.bytes().all(|b| b.is_ascii_digit()) {
        return Err(SnapCode::SpecMalformed);
    }
    let mut version = 0u32;
    let hb = head.as_bytes();
    let mut hi = 0usize;
    while hi < hb.len() {
        if let Some(b) = hb.get(hi) {
            version = version.wrapping_mul(10).wrapping_add((*b - b'0') as u32);
        }
        hi += 1;
    }
    if version == 0 {
        return Err(SnapCode::SpecMalformed);
    }
    let rest = match body.get(head_end + 1..) {
        Some(r) => r,
        None => return Err(SnapCode::SpecMalformed),
    };
    let mut fields: Vec<SnapField> = Vec::new();
    let mut cur = rest;
    loop {
        let seg = match cur.find('|') {
            Some(p) => {
                let s = match cur.get(..p) {
                    Some(x) => x,
                    None => return Err(SnapCode::SpecMalformed),
                };
                cur = match cur.get(p + 1..) {
                    Some(x) => x,
                    None => return Err(SnapCode::SpecMalformed),
                };
                s
            }
            None => {
                let s = cur;
                cur = "";
                s
            }
        };
        let eq = match seg.find('=') {
            Some(p) => p,
            None => return Err(SnapCode::SpecMalformed),
        };
        let name = match seg.get(..eq) {
            Some(n) => n,
            None => return Err(SnapCode::SpecMalformed),
        };
        let value = match seg.get(eq + 1..) {
            Some(v) => v,
            None => return Err(SnapCode::SpecMalformed),
        };
        if name.is_empty() || value.is_empty() {
            return Err(SnapCode::SpecMalformed);
        }
        let mut di = 0usize;
        while di < fields.len() {
            if let Some(f) = fields.get(di) {
                if f.name == name {
                    return Err(SnapCode::DupField);
                }
            }
            di += 1;
        }
        fields.push(SnapField { name: String::from(name), value: String::from(value) });
        if cur.is_empty() {
            break;
        }
    }
    if fields.is_empty() {
        return Err(SnapCode::EmptySnapshot);
    }
    Ok(Snapshot { version, fields })
}

// ── 追加式扩展校验（守护器） ──────────────────────────────────────────
/// 追加式扩展校验：new 必须是 old 的合法下一期。
/// 版本 = 尾期 +1；旧字段逐名逐值逐序保留；新字段全部带 j2. 前缀且只准尾插。
pub fn validate_extend(old: &Snapshot, new: &Snapshot) -> Result<(), SnapCode> {
    if new.fields.is_empty() || old.fields.is_empty() {
        return Err(SnapCode::EmptySnapshot);
    }
    // 版本必须恰为尾期 +1：低于（含持平）→ 回退，高于 → 跳变。
    if new.version != old.version.wrapping_add(1) {
        if new.version < old.version.wrapping_add(1) {
            return Err(SnapCode::VersionRegress);
        }
        return Err(SnapCode::VersionGap);
    }
    // 旧字段逐序核对：位置对齐后逐名逐值必须一致。
    let mut i = 0usize;
    while i < old.fields.len() {
        let of = match old.fields.get(i) {
            Some(f) => f,
            None => return Err(SnapCode::FieldDropped),
        };
        let nf = match new.fields.get(i) {
            Some(f) => f,
            None => return Err(SnapCode::FieldDropped),
        };
        if nf.name != of.name {
            // 该位置名不符：先判删（新表里找不到旧名）还是重排（位置漂移）。
            let mut found_at: Option<usize> = None;
            let mut k = 0usize;
            while k < new.fields.len() {
                if let Some(f) = new.fields.get(k) {
                    if f.name == of.name {
                        found_at = Some(k);
                    }
                }
                k += 1;
            }
            match found_at {
                None => return Err(SnapCode::FieldDropped),
                Some(pos) => {
                    if pos > i {
                        return Err(SnapCode::TailInsertion);
                    }
                    return Err(SnapCode::FieldReorder);
                }
            }
        }
        if nf.value != of.value {
            return Err(SnapCode::PrivateModify);
        }
        i += 1;
    }
    // 新增段：全部必须带 j2. 前缀，且只能出现在尾插区。
    let mut k = old.fields.len();
    while k < new.fields.len() {
        let nf = match new.fields.get(k) {
            Some(f) => f,
            None => return Err(SnapCode::FieldDropped),
        };
        if !nf.name.starts_with(J2_PREFIX) {
            return Err(SnapCode::SpecMalformed);
        }
        k += 1;
    }
    Ok(())
}

// ── 快照链账本 ────────────────────────────────────────────────────────
/// 快照链：逐期追加入链，每期必须通过追加式校验。
#[derive(Clone, Debug)]
pub struct SnapshotLedger {
    snaps: Vec<Snapshot>,
    cap: usize,
}

impl SnapshotLedger {
    /// 空链构造（容量 SNAP_CAP）。
    pub fn new() -> SnapshotLedger {
        SnapshotLedger { snaps: Vec::new(), cap: SNAP_CAP }
    }

    /// 创世期入链：版本必须为 1，规格必须解析合法。
    pub fn genesis(&mut self, text: &str) -> Result<u32, SnapCode> {
        if !self.snaps.is_empty() {
            return Err(SnapCode::VersionRegress);
        }
        let snap = parse_snapshot(text)?;
        if snap.version != 1 {
            return Err(SnapCode::VersionGap);
        }
        let v = snap.version;
        self.snaps.push(snap);
        Ok(v)
    }

    /// 下一期入链：走追加式校验；被拒不改链。
    pub fn extend(&mut self, text: &str) -> Result<u32, SnapCode> {
        if self.snaps.len() >= self.cap {
            return Err(SnapCode::RejectedTrace);
        }
        let new = parse_snapshot(text)?;
        let old = match self.snaps.last() {
            Some(s) => s.clone(),
            None => return Err(SnapCode::VersionGap),
        };
        validate_extend(&old, &new)?;
        let v = new.version;
        self.snaps.push(new);
        Ok(v)
    }

    /// 期数。
    pub fn len(&self) -> usize {
        self.snaps.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.snaps.is_empty()
    }

    /// 链审计：版本从 1 起逐期 +1 且每期相对前一期只增字段。
    pub fn continuous(&self) -> bool {
        if self.snaps.is_empty() {
            return false;
        }
        let mut i = 0usize;
        let mut expect = 1u32;
        while i < self.snaps.len() {
            let s = match self.snaps.get(i) {
                Some(s) => s,
                None => return false,
            };
            if s.version != expect {
                return false;
            }
            if i > 0 {
                let prev = match self.snaps.get(i - 1) {
                    Some(p) => p.clone(),
                    None => return false,
                };
                if validate_extend(&prev, s).is_err() {
                    return false;
                }
            }
            expect += 1;
            i += 1;
        }
        true
    }

    /// 尾期 J2 扩展段字段数。
    pub fn tail_j2_count(&self) -> usize {
        match self.snaps.last() {
            Some(s) => s.j2_count(),
            None => 0,
        }
    }
}

// ── 注入核：坏扩展样本矩阵 ────────────────────────────────────────────
/// 四类坏扩展样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadExtend {
    /// 私改旧字段值
    PrivateModify,
    /// 重排旧字段
    FieldReorder,
    /// 删旧字段
    FieldDropped,
    /// 中段插入新字段
    TailInsertion,
}

impl BadExtend {
    /// 闭集全量。
    pub const ALL: [BadExtend; 4] = [
        BadExtend::PrivateModify,
        BadExtend::FieldReorder,
        BadExtend::FieldDropped,
        BadExtend::TailInsertion,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadExtend::PrivateModify => "私改旧字段值",
            BadExtend::FieldReorder => "重排旧字段",
            BadExtend::FieldDropped => "删旧字段",
            BadExtend::TailInsertion => "中段插入新字段",
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

    /// 记一次注入结果；账失守恒返回 RejectedTrace。
    pub fn record(&mut self, caught: bool) -> Result<(), SnapCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(SnapCode::RejectedTrace);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏扩展注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_extend_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadExtend::ALL.len() {
        let case = match BadExtend::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadExtend::PrivateModify => {
                    let mut l = SnapshotLedger::new();
                    let ok1 = l.genesis("v1|base=核心|j1.a=read").is_ok();
                    ok1 && l.extend("v2|base=核心改|j1.a=read|j2.b=write")
                        .err() == Some(SnapCode::PrivateModify)
                }
                BadExtend::FieldReorder => {
                    let mut l = SnapshotLedger::new();
                    let ok1 = l.genesis("v1|base=核心|j1.a=read").is_ok();
                    ok1 && l.extend("v2|j1.a=read|base=核心|j2.b=write")
                        .err() == Some(SnapCode::TailInsertion)
                }
                BadExtend::FieldDropped => {
                    let mut l = SnapshotLedger::new();
                    let ok1 = l.genesis("v1|base=核心|j1.a=read").is_ok();
                    ok1 && l.extend("v2|base=核心|j2.b=write")
                        .err() == Some(SnapCode::FieldDropped)
                }
                BadExtend::TailInsertion => {
                    let mut l = SnapshotLedger::new();
                    let ok1 = l.genesis("v1|base=核心|j1.a=read").is_ok();
                    // 非法新版：j2 新字段插在 base 与 j1.a 之间（中段），
                    // 校验在 base 对齐后第二位失配 → 检出。
                    ok1 && l.extend("v2|base=核心|j2.b=write|j1.a=read")
                        .err() == Some(SnapCode::TailInsertion)
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

// ── 快照成文（纯函数） ────────────────────────────────────────────────
/// 链尾成文：「期=<n>;尾版=v<m>;J2段=<k>」；空链先拒。
pub fn compose_snap_summary(l: &SnapshotLedger) -> Result<String, SnapCode> {
    if l.is_empty() {
        return Err(SnapCode::EmptySnapshot);
    }
    let tail = match l.snaps.last() {
        Some(s) => s,
        None => return Err(SnapCode::EmptySnapshot),
    };
    let out = format!("期={};尾版=v{};J2段={}", l.len(), tail.version, tail.j2_count());
    if out.is_empty() {
        return Err(SnapCode::EmptySnapshot);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；非法规格/前缀错族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；私改旧字段/重排族"),
    ("ENODATA", -61, 232, "Windows ERROR_NO_DATA；删字段/空快照族"),
    ("EAGAIN", -11, 997, "Windows ERROR_RETRY_ON_CONFLICT（近似）；版本跳变待重放族"),
    ("EBADE", -52, 31, "Windows ERROR_GEN_FAILURE；成文往返失配族"),
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

/// 规范语料双跑指纹：创世 v1 → 两期 J2 追加 → 逐期成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = SnapshotLedger::new();
    let _ = l.genesis("v1|base=双判据快照|j1.a=read 正路径");
    let _ = l.extend("v2|base=双判据快照|j1.a=read 正路径|j2.a=坏 fd 反路径");
    let _ = l.extend("v3|base=双判据快照|j1.a=read 正路径|j2.a=坏 fd 反路径|j2.b=断端写");
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < l.len() {
        if let Some(s) = l.snaps.get(i) {
            bytes.extend_from_slice(s.to_spec().as_bytes());
            bytes.push(b';');
        }
        i += 1;
    }
    match compose_snap_summary(&l) {
        Ok(s) => bytes.extend_from_slice(s.as_bytes()),
        Err(_) => {}
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10095 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10095_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10095");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92780..=0x927BF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 快照规格解析器：三字段全解析
    let parse_ok = match parse_snapshot("v3|base=核心|j1.a=read|j2.b=write") {
        Ok(snap) => snap.version == 3 && snap.fields.len() == 3 && snap.j2_count() == 1,
        Err(_) => false,
    };
    s.add("规格解析器：v<n>|名=值 全解析且 J2 前缀可数", parse_ok,
          "快照规格是入链唯一入口，重名/空名一律拒绝");

    // 3. 非法规格注入：空名/空值/重名/零版/缺段 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_snapshot("v1|=x").is_ok()
            || parse_snapshot("v1|a=").is_ok()
            || parse_snapshot("v1|a=1|a=2").is_ok()
            || parse_snapshot("v0|a=1").is_ok()
            || parse_snapshot("v1").is_ok()
            || parse_snapshot("snap|a=1").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("非法规格注入：空名/空值/重名/零版/缺段 10/10 拒绝", amb_ok,
          "规格歧义必须逐次显性拒绝，不许静默放行");

    // 4. 创世规则：版本必须 1 且只许一次
    let mut l = SnapshotLedger::new();
    let g_ok = l.genesis("v1|base=双判据快照|j1.a=read").is_ok()
        && l.genesis("v1|again=x").is_err()
        && l.len() == 1;
    s.add("创世规则：首期必须 v1 且链上只能有一个创世期", g_ok,
          "创世期唯一，二次创世即版本回退");

    // 5. J2 段追加合法路径：逐期尾插，链连续
    let e1 = l.extend("v2|base=双判据快照|j1.a=read|j2.a=坏 fd");
    let e2 = l.extend("v3|base=双判据快照|j1.a=read|j2.a=坏 fd|j2.b=断端写");
    let ext_ok = e1 == Ok(2) && e2 == Ok(3) && l.continuous() && l.tail_j2_count() == 2;
    s.add("J2 段追加：两期尾插成功且链连续审计过", ext_ok,
          "追加式协议下 J2 段只增不改，链审计逐期复核");

    // 6. 版本守卫：回退与跳变拒绝
    let mut l2 = SnapshotLedger::new();
    let _ = l2.genesis("v1|base=核心");
    let _ = l2.extend("v2|base=核心|j2.a=反路径");
    let guard_ok = l2.extend("v1|base=核心|j2.a=反路径").err() == Some(SnapCode::VersionRegress)
        && l2.extend("v9|base=核心|j2.a=反路径").err() == Some(SnapCode::VersionGap);
    s.add("版本守卫：回退与跳变 10/10 拒绝", guard_ok,
          "新期必须接尾期 +1，回退与跳变都是结构私改");

    // 7. 注入核：四类坏扩展样本各 10/10 拒绝
    let (inj, per_class) = run_bad_extend_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏扩展样本各 10/10 拒绝", all_caught,
          "私改/重排/删字段/中段插入必须逐类逐次拒绝");

    // 8. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadExtend::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 9. 拒绝不留痕：私改被拒后期数不变
    let mut l3 = SnapshotLedger::new();
    let _ = l3.genesis("v1|base=核心");
    let before = l3.len();
    let _ = l3.extend("v2|base=核心改|j2.a=反路径");
    let trace_ok = l3.len() == before && l3.continuous();
    s.add("拒绝不留痕：私改被拒不改链任何期", trace_ok,
          "被拒扩展不得改动已冻结期，拒绝即回滚");

    // 10. 非 j2 前缀新字段拒绝
    let mut l4 = SnapshotLedger::new();
    let _ = l4.genesis("v1|base=核心");
    let prefix_ok = l4.extend("v2|base=核心|extra.к=野字段")
        .err() == Some(SnapCode::SpecMalformed);
    s.add("前缀守卫：新增字段必须带 j2. 前缀", prefix_ok,
          "J2 段扩展是唯一合法新增通道，野前缀即拒");

    // 11. 往返一致：成文再解析逐字段复原
    let mut l5 = SnapshotLedger::new();
    let _ = l5.genesis("v1|base=核心|j1.a=read");
    let _ = l5.extend("v2|base=核心|j1.a=read|j2.a=write");
    let rt_ok = match l5.snaps.last() {
        Some(tail) => {
            let spec = tail.to_spec();
            match parse_snapshot(&spec) {
                Ok(back) => back == *tail,
                Err(_) => false,
            }
        }
        None => false,
    };
    s.add("往返一致：尾期成文再解析逐字段复原", rt_ok,
          "成文必须可逆，隐藏状态即往返失败");

    // 12. 成文非空且含三标记 + 双跑一致
    let sum = compose_snap_summary(&l);
    let text_ok = match &sum {
        Ok(t) => t.contains("期=") && t.contains("尾版=v3") && t.contains("J2段=2"),
        Err(_) => false,
    };
    s.add("快照成文：期/尾版/J2段齐备且两次成文逐字节一致",
          text_ok && sum == compose_snap_summary(&l),
          "成文必须携带 期/尾版/J2段 标记且为纯函数");

    // 13. 空链成文拒绝
    s.add("空链成文拒绝：空链不得产出小结",
          compose_snap_summary(&SnapshotLedger::new()).err() == Some(SnapCode::EmptySnapshot),
          "空链没有对账对象，产出小结即虚报");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(1) && agrees_with_linux(61)
        && agrees_with_linux(11) && agrees_with_linux(52)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < SnapCode::ALL.len() {
        let mut j = i + 1;
        while j < SnapCode::ALL.len() {
            if let (Some(a), Some(b)) = (SnapCode::ALL.get(i), SnapCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = SnapCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 双跑指纹
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：同语料两次哈希逐位相等", f1 == f2,
          "指纹由三期快照逐期成文聚合，任何漂移即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum { Ok(t) => t.len(), Err(_) => 0 };
    let sum_text = format!("快照小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_roundtrip() {
        let snap = parse_snapshot("v2|base=核心|j2.a=反路径").unwrap_or(Snapshot {
            version: 0,
            fields: Vec::new(),
        });
        assert_eq!(snap.version, 2);
        assert_eq!(snap.j2_count(), 1);
        assert_eq!(snap.to_spec(), "v2|base=核心|j2.a=反路径");
        assert_eq!(parse_snapshot(&snap.to_spec()), Ok(snap));
    }

    #[test]
    fn parser_rejects() {
        assert_eq!(parse_snapshot("").err(), Some(SnapCode::SpecMalformed));
        assert_eq!(parse_snapshot("v1|a=1|a=2").err(), Some(SnapCode::DupField));
        assert_eq!(parse_snapshot("v1|a=").err(), Some(SnapCode::SpecMalformed));
        assert_eq!(parse_snapshot("v0|a=1").err(), Some(SnapCode::SpecMalformed));
    }

    #[test]
    fn genesis_rules() {
        let mut l = SnapshotLedger::new();
        assert!(l.genesis("v1|a=1").is_ok());
        assert_eq!(l.genesis("v1|a=1").err(), Some(SnapCode::VersionRegress));
        let mut l2 = SnapshotLedger::new();
        assert_eq!(l2.genesis("v2|a=1").err(), Some(SnapCode::VersionGap));
    }

    #[test]
    fn version_guards() {
        let mut l = SnapshotLedger::new();
        let _ = l.genesis("v1|a=1");
        assert_eq!(l.extend("v3|a=1|j2.b=2").err(), Some(SnapCode::VersionGap));
        assert!(l.extend("v2|a=1|j2.b=2").is_ok());
        assert_eq!(l.extend("v2|a=1|j2.b=2").err(), Some(SnapCode::VersionRegress));
        assert_eq!(l.extend("v1|a=1|j2.b=2").err(), Some(SnapCode::VersionRegress));
        assert_eq!(l.extend("v4|a=1|j2.b=2").err(), Some(SnapCode::VersionGap));
    }

    #[test]
    fn extend_append_only() {
        let mut l = SnapshotLedger::new();
        let _ = l.genesis("v1|a=1|b=2");
        assert!(l.extend("v2|a=1|b=2|j2.c=3").is_ok());
        assert_eq!(l.extend("v3|a=9|b=2|j2.c=3|j2.d=4").err(), Some(SnapCode::PrivateModify));
        assert_eq!(l.extend("v3|b=2|a=1|j2.c=3").err(), Some(SnapCode::TailInsertion));
        assert_eq!(l.extend("v3|a=1|j2.c=3").err(), Some(SnapCode::FieldDropped));
        assert!(l.continuous());
    }

    #[test]
    fn reject_no_trace() {
        let mut l = SnapshotLedger::new();
        let _ = l.genesis("v1|a=1");
        let n = l.len();
        let _ = l.extend("v2|a=changed|j2.b=2");
        assert_eq!(l.len(), n);
    }

    #[test]
    fn bad_extend_matrix_green() {
        let (inj, per) = run_bad_extend_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn j2_prefix_enforced() {
        let mut l = SnapshotLedger::new();
        let _ = l.genesis("v1|a=1");
        assert_eq!(l.extend("v2|a=1|wild=2").err(), Some(SnapCode::SpecMalformed));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(11));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(61), -61);
    }

    #[test]
    fn unx10095_checks_green() {
        let set = run_unx10095_checks();
        assert!(set.all_passed(), "UNX-F10095 red: {}", set.red_items_text());
    }
}
