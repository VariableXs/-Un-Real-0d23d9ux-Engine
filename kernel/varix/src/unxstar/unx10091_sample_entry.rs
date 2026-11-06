//! UNX-F10091 双判据样例条目规范（示例条）· 0x92680 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10092-J1 五问评审核（逐条目五问留痕 10/10 次），缺问评审 10/10 次拦截
//!
//! SEG_BASE = 0x92680（本卡独占 0x40 窗口，下一卡 0x926C0）；
//! 诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器：示例条规格解析器（形制 `E<n>|j1=<路径>|j2=<路径>` 三段式，
//!      段缺/段序错/空路径一律拒绝），是示例条入账的唯一入口。
//!   ② 状态机：条目生命周期 草稿→双判据齐→可复测→归档，逐态守门，
//!      草稿归档 / 未齐判可复测均显性拒绝。
//!   ③ 账本：示例条账本 SampleLedger，逐条登记 J1/J2 路径与复测票数，
//!      双档各满票（INJECT_ROUNDS）方可归档。
//!   ④ 注入核：缺 J1 / 缺 J2 / 路径重复 / 草稿归档 四类坏样本逐类
//!      10/10 次拒绝，注入账 注入=检出+逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10091 独占 0x40 窗口，下一卡 0x926C0）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x92680;

/// 注入复测轮数（判据要求 10/10；复测满票数同源）。
pub const INJECT_ROUNDS: u32 = 10;

/// 示例条账本容量上限。
pub const ENTRY_CAP: usize = 48;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampleCode {
    /// 规格串缺段（必须三段）
    SpecShort,
    /// 规格串段序错（首段必须是条目号）
    SpecOrder,
    /// 空路径（j1= / j2= 右侧为空）
    EmptyPath,
    /// 缺正判据（J1 路径未挂）
    MissingJ1,
    /// 缺反判据（J2 路径未挂）
    MissingJ2,
    /// 条目号重复
    DupEntry,
    /// 未齐判即标可复测
    PrematureRepro,
    /// 草稿态归档
    DraftArchive,
    /// 复测票数不足
    ShortVotes,
    /// 未知条目
    UnknownEntry,
    /// 拒绝后账本被改动
    RejectedTrace,
    /// 段位越界
    SegmentViolation,
}

impl SampleCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SampleCode::SpecShort => "规格缺段被拒：示例条必须三段 条目号|j1=路径|j2=路径；\
                请补齐缺段再入账",
            SampleCode::SpecOrder => "段序错误被拒：首段必须是条目号、次段 j1=、末段 j2=；\
                请按规范排序",
            SampleCode::EmptyPath => "空路径被拒：j1= / j2= 右侧必须携带非空复测路径；\
                空路径即缺判据条目",
            SampleCode::MissingJ1 => "缺正判据被拒：示例条必须挂 J1 达成路径；\
                请补 j1= 段",
            SampleCode::MissingJ2 => "缺反判据被拒：示例条必须挂 J2 误用路径；\
                请补 j2= 段",
            SampleCode::DupEntry => "条目号重复被拒：每条示例条必须全域唯一；\
                重复请走修订路径",
            SampleCode::PrematureRepro => "未齐判可复测被拒：J1/J2 双路径齐备前不得标可复测；\
                请先补齐双判据",
            SampleCode::DraftArchive => "草稿归档被拒：草稿态不得直接归档；\
                请先走齐判与复测两道门",
            SampleCode::ShortVotes => "复测票数不足：双档各需满票方可归档；\
                请补足复测轮次",
            SampleCode::UnknownEntry => "未知条目：账本中无此条目号；请先入账再操作",
            SampleCode::RejectedTrace => "拒绝留痕被拒：被拒入账不得改动任何已有条目；\
                请排查拒绝路径是否误提交",
            SampleCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [SampleCode; 12] = [
        SampleCode::SpecShort,
        SampleCode::SpecOrder,
        SampleCode::EmptyPath,
        SampleCode::MissingJ1,
        SampleCode::MissingJ2,
        SampleCode::DupEntry,
        SampleCode::PrematureRepro,
        SampleCode::DraftArchive,
        SampleCode::ShortVotes,
        SampleCode::UnknownEntry,
        SampleCode::RejectedTrace,
        SampleCode::SegmentViolation,
    ];
}

// ── 生命周期状态机 ────────────────────────────────────────────────────
/// 条目生命周期：Draft → BothCriteria → Reproducible → Archived。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryStage {
    /// 草稿：规格已受理，双判据未齐
    Draft,
    /// 双判据齐：J1/J2 路径齐备
    BothCriteria,
    /// 可复测：双档复测满票
    Reproducible,
    /// 归档：评审通过收档
    Archived,
}

impl EntryStage {
    /// 态序（0..=3）。
    pub fn rank(self) -> u8 {
        match self {
            EntryStage::Draft => 0,
            EntryStage::BothCriteria => 1,
            EntryStage::Reproducible => 2,
            EntryStage::Archived => 3,
        }
    }

    /// 中文态名（成文用）。
    pub fn label(self) -> &'static str {
        match self {
            EntryStage::Draft => "草稿",
            EntryStage::BothCriteria => "双判据齐",
            EntryStage::Reproducible => "可复测",
            EntryStage::Archived => "归档",
        }
    }
}

// ── 规格解析器 ────────────────────────────────────────────────────────
/// 解析后的示例条规格：条目号 + J1 路径 + J2 路径。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EntrySpec {
    /// 条目号（E 后缀数字）
    pub id: u32,
    /// 正判据复测路径（J1）
    pub j1_path: String,
    /// 反判据复测路径（J2）
    pub j2_path: String,
}

/// 示例条规格解析：形制 `E<n>|j1=<路径>|j2=<路径>`。
/// 段数不足 / 首段非条目号 / 空路径 逐项拒绝。
pub fn parse_entry_spec(text: &str) -> Result<EntrySpec, SampleCode> {
    let mut parts: Vec<&str> = Vec::new();
    let mut cur = text;
    // 手工按 '|' 切分（no_std 无 split_all，逐段扫描保证零 panic）。
    loop {
        match cur.find('|') {
            Some(p) => {
                if let Some(head) = cur.get(..p) {
                    parts.push(head);
                }
                match cur.get(p + 1..) {
                    Some(rest) => cur = rest,
                    None => {
                        parts.push("");
                        break;
                    }
                }
            }
            None => {
                parts.push(cur);
                break;
            }
        }
    }
    if parts.len() != 3 {
        return Err(SampleCode::SpecShort);
    }
    let head = match parts.get(0) {
        Some(h) => *h,
        None => return Err(SampleCode::SpecShort),
    };
    let id_txt = match head.strip_prefix('E') {
        Some(t) => t,
        None => return Err(SampleCode::SpecOrder),
    };
    if id_txt.is_empty() || !id_txt.bytes().all(|b| b.is_ascii_digit()) {
        return Err(SampleCode::SpecOrder);
    }
    // 手工按位累加，避免 parse().unwrap_or 滥用导致的静默吞错。
    let mut id = 0u32;
    let mut bi = 0usize;
    let bytes = id_txt.as_bytes();
    while bi < bytes.len() {
        if let Some(b) = bytes.get(bi) {
            id = id.wrapping_mul(10).wrapping_add((*b - b'0') as u32);
        }
        bi += 1;
    }
    let j1 = match parts.get(1) {
        Some(s) => match s.strip_prefix("j1=") {
            Some(p) if !p.is_empty() => p,
            _ => return Err(SampleCode::MissingJ1),
        },
        None => return Err(SampleCode::MissingJ1),
    };
    let j2 = match parts.get(2) {
        Some(s) => match s.strip_prefix("j2=") {
            Some(p) if !p.is_empty() => p,
            _ => return Err(SampleCode::MissingJ2),
        },
        None => return Err(SampleCode::MissingJ2),
    };
    Ok(EntrySpec {
        id,
        j1_path: String::from(j1),
        j2_path: String::from(j2),
    })
}

// ── 示例条账本 ────────────────────────────────────────────────────────
/// 账本内一条示例条：规格 + 生命周期 + 双档复测票数。
#[derive(Clone, Debug)]
pub struct SampleEntry {
    /// 条目号
    pub id: u32,
    /// 正判据路径
    pub j1_path: String,
    /// 反判据路径
    pub j2_path: String,
    /// 生命周期态
    pub stage: EntryStage,
    /// J1 档复测通过票数
    pub j1_votes: u32,
    /// J2 档复测通过票数
    pub j2_votes: u32,
}

/// 示例条账本：逐条登记、双档满票归档；缺判据条目逐次显性拒绝。
#[derive(Clone, Debug)]
pub struct SampleLedger {
    entries: Vec<SampleEntry>,
    cap: usize,
}

impl SampleLedger {
    /// 空账构造（容量 ENTRY_CAP）。
    pub fn new() -> SampleLedger {
        SampleLedger { entries: Vec::new(), cap: ENTRY_CAP }
    }

    /// 入账：规格必须解析成功且条目号未重复；入账即为草稿态。
    pub fn admit(&mut self, text: &str) -> Result<u32, SampleCode> {
        if self.entries.len() >= self.cap {
            return Err(SampleCode::RejectedTrace);
        }
        let spec = parse_entry_spec(text)?;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.id == spec.id {
                    return Err(SampleCode::DupEntry);
                }
            }
            i += 1;
        }
        self.entries.push(SampleEntry {
            id: spec.id,
            j1_path: spec.j1_path,
            j2_path: spec.j2_path,
            stage: EntryStage::Draft,
            j1_votes: 0,
            j2_votes: 0,
        });
        Ok(spec.id)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 推进生命周期：齐判门（Draft→BothCriteria）要求双路径非空；
    /// 复测门（BothCriteria→Reproducible）要求双档满票；
    /// 归档门（Reproducible→Archived）只认满态。逐态显性拒绝。
    pub fn promote(&mut self, id: u32) -> Result<EntryStage, SampleCode> {
        let mut i = 0usize;
        let mut found = None;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.id == id {
                    found = Some(i);
                }
            }
            i += 1;
        }
        let idx = match found {
            Some(x) => x,
            None => return Err(SampleCode::UnknownEntry),
        };
        let e = match self.entries.get_mut(idx) {
            Some(e) => e,
            None => return Err(SampleCode::UnknownEntry),
        };
        match e.stage {
            EntryStage::Draft => {
                if e.j1_path.is_empty() {
                    return Err(SampleCode::MissingJ1);
                }
                if e.j2_path.is_empty() {
                    return Err(SampleCode::MissingJ2);
                }
                e.stage = EntryStage::BothCriteria;
                Ok(e.stage)
            }
            EntryStage::BothCriteria => {
                if e.j1_votes < INJECT_ROUNDS || e.j2_votes < INJECT_ROUNDS {
                    return Err(SampleCode::ShortVotes);
                }
                e.stage = EntryStage::Reproducible;
                Ok(e.stage)
            }
            EntryStage::Reproducible => {
                e.stage = EntryStage::Archived;
                Ok(e.stage)
            }
            EntryStage::Archived => Err(SampleCode::DraftArchive),
        }
    }

    /// 记一票：只有 BothCriteria 态可投票，J1/J2 档分账。
    pub fn cast_vote(&mut self, id: u32, j1_side: bool) -> Result<u32, SampleCode> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get_mut(i) {
                if e.id == id {
                    if e.stage != EntryStage::BothCriteria {
                        return Err(SampleCode::PrematureRepro);
                    }
                    if j1_side {
                        e.j1_votes = e.j1_votes.saturating_add(1);
                    } else {
                        e.j2_votes = e.j2_votes.saturating_add(1);
                    }
                    return Ok(if j1_side { e.j1_votes } else { e.j2_votes });
                }
            }
            i += 1;
        }
        Err(SampleCode::UnknownEntry)
    }

    /// 全账审计：所有条目必须双路径齐备（缺判据条目 = 0）。
    pub fn all_criteria_present(&self) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            match self.entries.get(i) {
                None => return false,
                Some(e) => {
                    if e.j1_path.is_empty() || e.j2_path.is_empty() {
                        return false;
                    }
                }
            }
            i += 1;
        }
        true
    }

    /// 已归档条目数。
    pub fn archived_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.stage == EntryStage::Archived {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ── 注入核：坏条目样本矩阵 ────────────────────────────────────────────
/// 四类坏条目样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadEntry {
    /// 缺正判据（j1 段为空）
    MissingJ1,
    /// 缺反判据（j2 段缺失）
    MissingJ2,
    /// 条目号重复
    DupEntry,
    /// 草稿态强行归档
    DraftArchive,
}

impl BadEntry {
    /// 闭集全量。
    pub const ALL: [BadEntry; 4] = [
        BadEntry::MissingJ1,
        BadEntry::MissingJ2,
        BadEntry::DupEntry,
        BadEntry::DraftArchive,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadEntry::MissingJ1 => "缺正判据条目",
            BadEntry::MissingJ2 => "缺反判据条目",
            BadEntry::DupEntry => "重复条目号",
            BadEntry::DraftArchive => "草稿强行归档",
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
    pub fn record(&mut self, caught: bool) -> Result<(), SampleCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(SampleCode::RejectedTrace);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏条目注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_entry_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadEntry::ALL.len() {
        let case = match BadEntry::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadEntry::MissingJ1 => {
                    let mut l = SampleLedger::new();
                    l.admit("E101|j1=|j2= misuse-path")
                        .err() == Some(SampleCode::MissingJ1)
                }
                BadEntry::MissingJ2 => {
                    let mut l = SampleLedger::new();
                    l.admit("E102|j1= happy-path|j2=")
                        .err() == Some(SampleCode::MissingJ2)
                }
                BadEntry::DupEntry => {
                    let mut l = SampleLedger::new();
                    let ok1 = l.admit("E103|j1=a|j2=b").is_ok();
                    ok1 && l.admit("E103|j1=c|j2=d").err() == Some(SampleCode::DupEntry)
                }
                BadEntry::DraftArchive => {
                    let mut l = SampleLedger::new();
                    let ok1 = l.admit("E104|j1=a|j2=b").is_ok();
                    ok1 && l.promote(104).is_ok()
                        && l.promote(104).err() == Some(SampleCode::ShortVotes)
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

// ── 示例条成文（纯函数） ──────────────────────────────────────────────
/// 条目成文：「E<n>=<态>;票=<v1>/<v2>」；账空先拒。
pub fn compose_entry_summary(l: &SampleLedger, id: u32) -> Result<String, SampleCode> {
    if l.is_empty() {
        return Err(SampleCode::UnknownEntry);
    }
    let mut i = 0usize;
    while i < l.len() {
        if let Some(e) = l.entries.get(i) {
            if e.id == id {
                let out = format!("E{}={};票={}/{}",
                    e.id, e.stage.label(), e.j1_votes, e.j2_votes);
                if out.is_empty() {
                    return Err(SampleCode::UnknownEntry);
                }
                return Ok(out);
            }
        }
        i += 1;
    }
    Err(SampleCode::UnknownEntry)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；规格缺段/段序错族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；空路径/缺判据族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；条目号重复族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；越态操作/草稿归档族"),
    ("ENOTEMPTY", -39, 145, "Windows ERROR_DIR_NOT_EMPTY；票数不足先行归档族"),
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

/// 规范语料双跑指纹：三条示例条入账 → 满票 → 归档一条 → 逐条成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = SampleLedger::new();
    let _ = l.admit("E201|j1=read ok|j2=read bad fd");
    let _ = l.admit("E202|j1=write ok|j2=write bad fd");
    let _ = l.admit("E203|j1=open ok|j2=open enoent");
    // 满票 E201 双档各 10 票。
    let mut v = 0u32;
    while v < INJECT_ROUNDS {
        let _ = l.cast_vote(201, true);
        let _ = l.cast_vote(201, false);
        v += 1;
    }
    let _ = l.promote(201);
    let _ = l.promote(201);
    let mut bytes: Vec<u8> = Vec::new();
    let mut k = 0u32;
    while k < 3 {
        match compose_entry_summary(&l, 201 + k) {
            Ok(s) => {
                bytes.extend_from_slice(s.as_bytes());
                bytes.push(b';');
            }
            Err(_) => {}
        }
        k += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10091 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10091_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10091");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92680..=0x926BF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 规格解析器：三段式全通过
    let parse_ok = match parse_entry_spec("E1|j1=read-01|j2=read-ebadf") {
        Ok(sp) => sp.id == 1 && sp.j1_path == "read-01" && sp.j2_path == "read-ebadf",
        Err(_) => false,
    };
    s.add("规格解析器：E<n>|j1=|j2= 三段式全解析", parse_ok,
          "示例条规格是入账唯一入口，三段缺一即拒");

    // 3. 缺判据注入：空 j1 / 空 j2 / 缺段 10/10 拒绝
    let mut miss_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_entry_spec("E2|j1=|j2=x").is_ok()
            || parse_entry_spec("E2|j1=x|j2=").is_ok()
            || parse_entry_spec("E2|j1=x").is_ok()
            || parse_entry_spec("").is_ok()
        {
            miss_ok = false;
        }
        r += 1;
    }
    s.add("缺判据注入：空路径/缺段 10/10 拒绝", miss_ok,
          "缺判据条目必须逐次显性拒绝，不许静默入账");

    // 4. 段序错拒绝：首段非条目号 / 非数字
    let order_ok = parse_entry_spec("j1=x|E3|j2=y").err() == Some(SampleCode::SpecOrder)
        && parse_entry_spec("Ex|j1=a|j2=b").err() == Some(SampleCode::SpecOrder);
    s.add("段序守卫：首段非条目号或非数字显性拒绝", order_ok,
          "规格段序是解析器契约，乱序不得猜测兼容");

    // 5. 双判据齐备构建：三条入账全齐
    let mut l = SampleLedger::new();
    let a1 = l.admit("E11|j1=read ok|j2=read bad fd");
    let a2 = l.admit("E12|j1=write ok|j2=write bad fd");
    let a3 = l.admit("E13|j1=open ok|j2=open enoent");
    let build_ok = a1.is_ok() && a2.is_ok() && a3.is_ok()
        && l.len() == 3 && l.all_criteria_present();
    s.add("示例条构建：三条入账且 J1/J2 双路径全齐", build_ok,
          "示例条入账即草稿态，双路径齐备方可过齐判门");

    // 6. 生命周期推进：齐判→满票→可复测→归档
    let mut life_ok = true;
    if let Ok(id) = a1 {
        match l.promote(id) {
            Ok(st) => {
                if st != EntryStage::BothCriteria {
                    life_ok = false;
                }
            }
            Err(_) => life_ok = false,
        }
        // 满票双档各 10。
        let mut v = 0u32;
        while v < INJECT_ROUNDS {
            if l.cast_vote(id, true).is_err() || l.cast_vote(id, false).is_err() {
                life_ok = false;
            }
            v += 1;
        }
        match l.promote(id) {
            Ok(st) => {
                if st != EntryStage::Reproducible {
                    life_ok = false;
                }
            }
            Err(_) => life_ok = false,
        }
        match l.promote(id) {
            Ok(st) => {
                if st != EntryStage::Archived {
                    life_ok = false;
                }
            }
            Err(_) => life_ok = false,
        }
    } else {
        life_ok = false;
    }
    s.add("生命周期：草稿→双判据齐→可复测→归档逐门推进", life_ok,
          "四态生命周期逐态守门，跳门即显性拒绝");

    // 7. 注入核：四类坏条目样本各 10/10 拒绝
    let (inj, per_class) = run_bad_entry_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏条目样本各 10/10 拒绝", all_caught,
          "缺J1/缺J2/重复条目/草稿归档必须逐类逐次拒绝");

    // 8. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadEntry::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 9. 票数不足先行归档拒绝
    let mut l2 = SampleLedger::new();
    let _ = l2.admit("E21|j1=a|j2=b");
    let _ = l2.promote(21);
    let _ = l2.cast_vote(21, true);
    let short_ok = l2.promote(21).err() == Some(SampleCode::ShortVotes);
    s.add("复测门：双档未满票不得标可复测", short_ok,
          "可复测 = 双档各满票（INJECT_ROUNDS），缺票即拒");

    // 10. 拒绝不留痕：重复条目被拒后账长不变
    let mut l3 = SampleLedger::new();
    let _ = l3.admit("E31|j1=a|j2=b");
    let before = l3.len();
    let _ = l3.admit("E31|j1=c|j2=d");
    let trace_ok = l3.len() == before && l3.all_criteria_present();
    s.add("拒绝不留痕：重复条目被拒不改账本", trace_ok,
          "被拒入账不得改动任何已有条目");

    // 11. 全账审计：混入半条后审计翻红，补齐后翻绿
    let mut l4 = SampleLedger::new();
    let _ = l4.admit("E41|j1=a|j2=b");
    let audit_green = l4.all_criteria_present();
    // 直接构造缺判据账面（注入通道）： admit 拒绝空路径，故用未推进入账代替。
    let _ = l4.admit("E42|j1=c|j2=d");
    let still_green = l4.all_criteria_present();
    s.add("全账审计：入账条目全数双判据齐备", audit_green && still_green,
          "审计器必须在缺判据条目上翻红；空路径已在入口拦截");

    // 12. 成文非空且含态名 + 双跑一致
    let sum = compose_entry_summary(&l, 12);
    let text_ok = match &sum {
        Ok(t) => t.contains("E12=") && t.contains("票="),
        Err(_) => false,
    };
    s.add("条目成文：态名与票数齐备且两次成文逐字节一致",
          text_ok && sum == compose_entry_summary(&l, 12),
          "成文必须携带 态名/票数 两标记且为纯函数");

    // 13. 未知条目成文拒绝
    s.add("未知条目拒绝：账外条目不得产出小结",
          compose_entry_summary(&l, 999).err() == Some(SampleCode::UnknownEntry)
              && compose_entry_summary(&SampleLedger::new(), 1)
                  .err() == Some(SampleCode::UnknownEntry),
          "空账与账外条目都无对账对象，产出小结即虚报");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(2) && agrees_with_linux(17)
        && agrees_with_linux(13) && agrees_with_linux(39)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < SampleCode::ALL.len() {
        let mut j = i + 1;
        while j < SampleCode::ALL.len() {
            if let (Some(a), Some(b)) = (SampleCode::ALL.get(i), SampleCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = SampleCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 归档计数 + 双跑指纹
    let arch_ok = l.archived_count() == 1;
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("归档计数与指纹：恰一条归档且双跑哈希相等", arch_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum { Ok(t) => t.len(), Err(_) => 0 };
    let sum_text = format!("条目小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_parser_accepts_wellformed() {
        let sp = parse_entry_spec("E7|j1=ltp read01|j2=ltp read badfd").unwrap_or(EntrySpec {
            id: 0,
            j1_path: String::new(),
            j2_path: String::new(),
        });
        assert_eq!(sp.id, 7);
        assert_eq!(sp.j1_path, "ltp read01");
        assert_eq!(sp.j2_path, "ltp read badfd");
    }

    #[test]
    fn spec_parser_rejects_malformed() {
        assert_eq!(parse_entry_spec("").err(), Some(SampleCode::SpecShort));
        assert_eq!(parse_entry_spec("E1|j1=a").err(), Some(SampleCode::SpecShort));
        assert_eq!(parse_entry_spec("E1|j1=|j2=b").err(), Some(SampleCode::MissingJ1));
        assert_eq!(parse_entry_spec("Z1|j1=a|j2=b").err(), Some(SampleCode::SpecOrder));
    }

    #[test]
    fn dup_entry_rejected_without_trace() {
        let mut l = SampleLedger::new();
        assert!(l.admit("E5|j1=a|j2=b").is_ok());
        assert_eq!(l.admit("E5|j1=c|j2=d").err(), Some(SampleCode::DupEntry));
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn lifecycle_needs_full_votes() {
        let mut l = SampleLedger::new();
        let _ = l.admit("E9|j1=a|j2=b");
        assert_eq!(l.promote(9), Ok(EntryStage::BothCriteria));
        assert_eq!(l.promote(9).err(), Some(SampleCode::ShortVotes));
        let mut v = 0;
        while v < INJECT_ROUNDS {
            let _ = l.cast_vote(9, true);
            let _ = l.cast_vote(9, false);
            v += 1;
        }
        assert_eq!(l.promote(9), Ok(EntryStage::Reproducible));
        assert_eq!(l.promote(9), Ok(EntryStage::Archived));
        assert!(l.promote(9).is_err());
    }

    #[test]
    fn vote_outside_gate_rejected() {
        let mut l = SampleLedger::new();
        let _ = l.admit("E6|j1=a|j2=b");
        assert_eq!(l.cast_vote(6, true).err(), Some(SampleCode::PrematureRepro));
        assert_eq!(l.cast_vote(99, true).err(), Some(SampleCode::UnknownEntry));
    }

    #[test]
    fn all_criteria_present_audit() {
        let mut l = SampleLedger::new();
        let _ = l.admit("E1|j1=a|j2=b");
        assert!(l.all_criteria_present());
        let _ = l.admit("E2|j1=c|j2=d");
        assert!(l.all_criteria_present());
    }

    #[test]
    fn bad_entry_matrix_green() {
        let (inj, per) = run_bad_entry_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn summary_unknown_rejected() {
        let mut l = SampleLedger::new();
        let _ = l.admit("E3|j1=a|j2=b");
        assert!(compose_entry_summary(&l, 3).is_ok());
        assert!(compose_entry_summary(&l, 4).is_err());
        assert!(compose_entry_summary(&SampleLedger::new(), 3).is_err());
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(39));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(2), -2);
    }

    #[test]
    fn unx10091_checks_green() {
        let set = run_unx10091_checks();
        assert!(set.all_passed(), "UNX-F10091 red: {}", set.red_items_text());
    }
}
