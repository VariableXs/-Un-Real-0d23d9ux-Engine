//! UNX-F10093 双判据与 LTP 预期差对接 · 0x92700 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10094-J1 从严豁免核（J2 豁免必挂 ADR+复核 10/10 次），轻豁免 10/10 次拦截
//!
//! SEG_BASE = 0x92700（本卡独占 0x40 窗口，下一卡 0x92740）；
//! 诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 注册表：LTP 面注册表（case token ↔ 预期 errno 逐条绑定，闭集外
//!      面一律拒绝），预期差登记必须指向表内面。
//!   ② 解析器：登记规格解析器（形制 `E<n>:<case>=<errno>`，面/码双验），
//!      是预期差入账的唯一入口。
//!   ③ 账本：红馈账本 RedFeedLedger（J2 红逐条入账）+ 差异登记账
//!      ExpDiffRegistry（登记 = 红馈的唯一出账通道）；对账审计要求
//!      每条 J2 红恰有一条同号登记，绕登记逐次显性拒绝。
//!   ④ 注入核：绕登记 / 重复登记 / errno 不符 / 未知 LTP 面 四类坏样本
//!      逐类 10/10 次拒绝，注入账 注入=检出+逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10093 独占 0x40 窗口，下一卡 0x92740）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x92700;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 登记账容量上限。
pub const REGISTRY_CAP: usize = 64;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExpDiffCode {
    /// 未知 LTP 面（注册表外）
    UnknownLtpCase,
    /// errno 与注册表预期不符
    ErrnoMismatch,
    /// 同条目重复登记
    DupDiff,
    /// J2 红未走登记即出账
    BypassRegistration,
    /// 登记号不在红馈账
    NotARed,
    /// 登记事由为空
    EmptyReason,
    /// 非法规格串
    SpecMalformed,
    /// 生命周期越态
    StateViolation,
    /// 红馈与登记数失守恒
    LedgerMismatch,
    /// 面标签往返不一致
    CaseRoundTrip,
    /// 拒绝后账本被改动
    RejectedTrace,
    /// 段位越界
    SegmentViolation,
}

impl ExpDiffCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ExpDiffCode::UnknownLtpCase => "未知 LTP 面被拒：登记必须指向注册表内 case；\
                请先扩注册表再登记",
            ExpDiffCode::ErrnoMismatch => "errno 不符被拒：登记码必须等于注册表预期码；\
                请核对 LTP 面的预期错误码",
            ExpDiffCode::DupDiff => "重复登记被拒：同条目只能登记一条预期差；\
                复发请走再确认路径",
            ExpDiffCode::BypassRegistration => "绕登记被拒：J2 红必须先登记预期差再出账；\
                未登记红即出账即脱账",
            ExpDiffCode::NotARed => "登记号不在红馈账：预期差只能挂在已入账的 J2 红上；\
                请先入红再登记",
            ExpDiffCode::EmptyReason => "空事由被拒：登记必须携带非空事由；\
                空事由即无据登记",
            ExpDiffCode::SpecMalformed => "非法规格串：形制必须 E<n>:<case>=<errno>；\
                请按闭集写法重写",
            ExpDiffCode::StateViolation => "生命周期越态：差异态必须 Open→Confirmed→Fed 单向推进；\
                越态操作显性拒绝",
            ExpDiffCode::LedgerMismatch => "红馈与登记失守恒：每条已出账 J2 红必须恰有一条登记；\
                请对账补登记",
            ExpDiffCode::CaseRoundTrip => "面标签往返不一致：label 再解析必须回到原面；\
                请检查注册表文案与解析器是否同步",
            ExpDiffCode::RejectedTrace => "拒绝留痕被拒：被拒登记不得改动任何已有条目；\
                请排查拒绝路径是否误提交",
            ExpDiffCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ExpDiffCode; 12] = [
        ExpDiffCode::UnknownLtpCase,
        ExpDiffCode::ErrnoMismatch,
        ExpDiffCode::DupDiff,
        ExpDiffCode::BypassRegistration,
        ExpDiffCode::NotARed,
        ExpDiffCode::EmptyReason,
        ExpDiffCode::SpecMalformed,
        ExpDiffCode::StateViolation,
        ExpDiffCode::LedgerMismatch,
        ExpDiffCode::CaseRoundTrip,
        ExpDiffCode::RejectedTrace,
        ExpDiffCode::SegmentViolation,
    ];
}

// ── LTP 面注册表 ──────────────────────────────────────────────────────
/// LTP 面注册表：case token ↔ 预期 errno（正数）。闭集逐条绑定。
pub const LTP_CASES: &[(&str, i32)] = &[
    ("read-badfd", 9),
    ("write-badfd", 9),
    ("open-enoent", 2),
    ("lseek-einval", 22),
    ("pipe-epipe", 32),
    ("fstat-efault", 14),
];

/// 按面 token 查预期 errno；表外返回 None。
pub fn expected_errno(case: &str) -> Option<i32> {
    let mut i = 0usize;
    while i < LTP_CASES.len() {
        if let Some(row) = LTP_CASES.get(i) {
            if row.0 == case {
                return Some(row.1);
            }
        }
        i += 1;
    }
    None
}

/// 登记规格：条目号 + LTP 面 + 登记码。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiffSpec {
    /// 条目号
    pub id: u32,
    /// LTP 面 token
    pub case: String,
    /// 登记码（正数 errno）
    pub errno: i32,
    /// 事由（非空）
    pub reason: String,
}

/// 登记规格解析：形制 `E<n>:<case>=<errno>:<事由>`。
/// 面/码双验：面必须在注册表内且码等于预期码；事由非空。
pub fn parse_diff_spec(text: &str) -> Result<DiffSpec, ExpDiffCode> {
    // 三段：E<n> | case=errno | 事由
    let mut segs: [&str; 3] = ["", "", ""];
    let mut si = 0usize;
    let mut cur = text;
    loop {
        match cur.find(':') {
            Some(p) => {
                if si >= 3 {
                    return Err(ExpDiffCode::SpecMalformed);
                }
                segs[si] = match cur.get(..p) {
                    Some(h) => h,
                    None => return Err(ExpDiffCode::SpecMalformed),
                };
                si += 1;
                cur = match cur.get(p + 1..) {
                    Some(r) => r,
                    None => return Err(ExpDiffCode::SpecMalformed),
                };
            }
            None => {
                if si != 2 {
                    return Err(ExpDiffCode::SpecMalformed);
                }
                segs[2] = cur;
                break;
            }
        }
    }
    let head = segs[0];
    let id_txt = match head.strip_prefix('E') {
        Some(t) if !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()) => t,
        _ => return Err(ExpDiffCode::SpecMalformed),
    };
    let mut id = 0u32;
    let bytes = id_txt.as_bytes();
    let mut bi = 0usize;
    while bi < bytes.len() {
        if let Some(b) = bytes.get(bi) {
            id = id.wrapping_mul(10).wrapping_add((*b - b'0') as u32);
        }
        bi += 1;
    }
    let mid = segs[1];
    let eq = match mid.find('=') {
        Some(p) => p,
        None => return Err(ExpDiffCode::SpecMalformed),
    };
    let case = match mid.get(..eq) {
        Some(c) => c,
        None => return Err(ExpDiffCode::SpecMalformed),
    };
    let num_txt = match mid.get(eq + 1..) {
        Some(n) => n,
        None => return Err(ExpDiffCode::SpecMalformed),
    };
    if num_txt.is_empty() || !num_txt.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ExpDiffCode::SpecMalformed);
    }
    let mut errno = 0i64;
    let nb = num_txt.as_bytes();
    let mut ni = 0usize;
    while ni < nb.len() {
        if let Some(b) = nb.get(ni) {
            errno = errno * 10 + (*b - b'0') as i64;
            if errno > 1_000_000 {
                return Err(ExpDiffCode::SpecMalformed);
            }
        }
        ni += 1;
    }
    let errno = errno as i32;
    if segs[2].is_empty() {
        return Err(ExpDiffCode::EmptyReason);
    }
    let expect = match expected_errno(case) {
        Some(e) => e,
        None => return Err(ExpDiffCode::UnknownLtpCase),
    };
    if errno != expect {
        return Err(ExpDiffCode::ErrnoMismatch);
    }
    Ok(DiffSpec {
        id,
        case: String::from(case),
        errno,
        reason: String::from(segs[2]),
    })
}

// ── 差异生命周期状态机 ────────────────────────────────────────────────
/// 差异态：Open → Confirmed → Fed（单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffStage {
    /// 开立：红已入账、登记已成
    Open,
    /// 再确认：复跑红同现
    Confirmed,
    /// 已馈出：差异入 F9900 聚合
    Fed,
}

impl DiffStage {
    /// 态序（0..=2）。
    pub fn rank(self) -> u8 {
        match self {
            DiffStage::Open => 0,
            DiffStage::Confirmed => 1,
            DiffStage::Fed => 2,
        }
    }
}

// ── 双账本 ────────────────────────────────────────────────────────────
/// 登记条目。
#[derive(Clone, Debug)]
pub struct DiffRecord {
    /// 条目号（对应 J2 红）
    pub id: u32,
    /// LTP 面
    pub case: String,
    /// 登记码
    pub errno: i32,
    /// 事由
    pub reason: String,
    /// 生命周期态
    pub stage: DiffStage,
}

/// 红馈 + 登记双账：J2 红入红馈账，登记作为唯一出账通道。
#[derive(Clone, Debug)]
pub struct ExpDiffLedger {
    reds: Vec<u32>,
    diffs: Vec<DiffRecord>,
    cap: usize,
}

impl ExpDiffLedger {
    /// 空账构造（容量 REGISTRY_CAP）。
    pub fn new() -> ExpDiffLedger {
        ExpDiffLedger { reds: Vec::new(), diffs: Vec::new(), cap: REGISTRY_CAP }
    }

    /// J2 红入账：同号重复入红拒绝。
    pub fn feed_red(&mut self, id: u32) -> Result<(), ExpDiffCode> {
        if id == 0 {
            return Err(ExpDiffCode::NotARed);
        }
        let mut i = 0usize;
        while i < self.reds.len() {
            if let Some(r) = self.reds.get(i) {
                if *r == id {
                    return Err(ExpDiffCode::DupDiff);
                }
            }
            i += 1;
        }
        if self.reds.len() >= self.cap {
            return Err(ExpDiffCode::LedgerMismatch);
        }
        self.reds.push(id);
        Ok(())
    }

    /// 登记预期差：规格必须解析合法、条目必须在红馈账、同号未登记。
    pub fn register(&mut self, text: &str) -> Result<DiffStage, ExpDiffCode> {
        let spec = parse_diff_spec(text)?;
        let mut is_red = false;
        let mut i = 0usize;
        while i < self.reds.len() {
            if let Some(r) = self.reds.get(i) {
                if *r == spec.id {
                    is_red = true;
                }
            }
            i += 1;
        }
        if !is_red {
            return Err(ExpDiffCode::NotARed);
        }
        let mut j = 0usize;
        while j < self.diffs.len() {
            if let Some(d) = self.diffs.get(j) {
                if d.id == spec.id {
                    return Err(ExpDiffCode::DupDiff);
                }
            }
            j += 1;
        }
        if self.diffs.len() >= self.cap {
            return Err(ExpDiffCode::LedgerMismatch);
        }
        self.diffs.push(DiffRecord {
            id: spec.id,
            case: spec.case,
            errno: spec.errno,
            reason: spec.reason,
            stage: DiffStage::Open,
        });
        Ok(DiffStage::Open)
    }

    /// 生命周期推进：单向一格，越态拒绝。
    pub fn promote(&mut self, id: u32) -> Result<DiffStage, ExpDiffCode> {
        let mut i = 0usize;
        while i < self.diffs.len() {
            if let Some(d) = self.diffs.get_mut(i) {
                if d.id == id {
                    let next = match d.stage.rank() {
                        0 => DiffStage::Confirmed,
                        1 => DiffStage::Fed,
                        _ => return Err(ExpDiffCode::StateViolation),
                    };
                    d.stage = next;
                    return Ok(next);
                }
            }
            i += 1;
        }
        Err(ExpDiffCode::NotARed)
    }

    /// 红馈条数。
    pub fn red_count(&self) -> usize {
        self.reds.len()
    }

    /// 登记条数。
    pub fn diff_count(&self) -> usize {
        self.diffs.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.reds.is_empty() && self.diffs.is_empty()
    }

    /// 对账审计：每条已出账 J2 红必须恰有一条登记（绕登记 = 差额）。
    pub fn no_bypass(&self) -> bool {
        self.diff_count() == self.red_count()
    }

    /// 红是否已登记。
    pub fn is_registered(&self, id: u32) -> bool {
        let mut i = 0usize;
        while i < self.diffs.len() {
            if let Some(d) = self.diffs.get(i) {
                if d.id == id {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
}

// ── 注入核：坏登记样本矩阵 ────────────────────────────────────────────
/// 四类坏登记样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadDiff {
    /// 绕登记：红未登记即出账
    BypassRegistration,
    /// 重复登记
    DupDiff,
    /// errno 不符
    ErrnoMismatch,
    /// 未知 LTP 面
    UnknownLtpCase,
}

impl BadDiff {
    /// 闭集全量。
    pub const ALL: [BadDiff; 4] = [
        BadDiff::BypassRegistration,
        BadDiff::DupDiff,
        BadDiff::ErrnoMismatch,
        BadDiff::UnknownLtpCase,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadDiff::BypassRegistration => "绕登记出账",
            BadDiff::DupDiff => "重复登记",
            BadDiff::ErrnoMismatch => "errno 不符",
            BadDiff::UnknownLtpCase => "未知 LTP 面",
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
    pub fn record(&mut self, caught: bool) -> Result<(), ExpDiffCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(ExpDiffCode::RejectedTrace);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏登记注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_diff_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadDiff::ALL.len() {
        let case = match BadDiff::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadDiff::BypassRegistration => {
                    let mut l = ExpDiffLedger::new();
                    let _ = l.feed_red(501);
                    // 红已入账但未登记：对账审计必须暴露绕登记。
                    !l.is_registered(501) && !l.no_bypass()
                }
                BadDiff::DupDiff => {
                    let mut l = ExpDiffLedger::new();
                    let _ = l.feed_red(502);
                    let ok1 = l.register("E502:open-enoent=2:复现缺路径").is_ok();
                    ok1 && l.register("E502:open-enoent=2:重复事由")
                        .err() == Some(ExpDiffCode::DupDiff)
                }
                BadDiff::ErrnoMismatch => {
                    let mut l = ExpDiffLedger::new();
                    let _ = l.feed_red(503);
                    l.register("E503:open-enoent=9:码写错")
                        .err() == Some(ExpDiffCode::ErrnoMismatch)
                }
                BadDiff::UnknownLtpCase => {
                    let mut l = ExpDiffLedger::new();
                    let _ = l.feed_red(504);
                    l.register("E504:read-weird=9:面写错")
                        .err() == Some(ExpDiffCode::UnknownLtpCase)
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

// ── 登记成文（纯函数） ────────────────────────────────────────────────
/// 登记成文：「E<n>@<case>=<errno>;态=<Open|Confirmed|Fed>」；账外先拒。
pub fn compose_diff_summary(l: &ExpDiffLedger, id: u32) -> Result<String, ExpDiffCode> {
    if l.is_empty() {
        return Err(ExpDiffCode::LedgerMismatch);
    }
    let mut i = 0usize;
    while i < l.diffs.len() {
        if let Some(d) = l.diffs.get(i) {
            if d.id == id {
                let st = match d.stage {
                    DiffStage::Open => "Open",
                    DiffStage::Confirmed => "Confirmed",
                    DiffStage::Fed => "Fed",
                };
                let out = format!("E{}@{}={};态={}", d.id, d.case, d.errno, st);
                if out.is_empty() {
                    return Err(ExpDiffCode::LedgerMismatch);
                }
                return Ok(out);
            }
        }
        i += 1;
    }
    Err(ExpDiffCode::BypassRegistration)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 9, "Windows ERROR_INVALID_HANDLE；坏 fd 误用面族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺失路径误用面族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；非法偏移/参数面族"),
    ("EPIPE", -32, 232, "Windows ERROR_NO_DATA（管道断开）；断端写误用面族"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏缓冲误用面族"),
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

/// 规范语料双跑指纹：三红入账 → 逐条登记 → 推进两态 → 逐条成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = ExpDiffLedger::new();
    let _ = l.feed_red(601);
    let _ = l.feed_red(602);
    let _ = l.feed_red(603);
    let _ = l.register("E601:read-badfd=9:坏 fd 面红");
    let _ = l.register("E602:open-enoent=2:缺路径面红");
    let _ = l.register("E603:pipe-epipe=32:断端写面红");
    let _ = l.promote(601);
    let _ = l.promote(601);
    let _ = l.promote(602);
    let mut bytes: Vec<u8> = Vec::new();
    let mut e = 0u32;
    while e < 3 {
        match compose_diff_summary(&l, 601 + e) {
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
/// F10093 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10093_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10093");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92700..=0x9273F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. LTP 面注册表：面与预期码逐条绑定
    let reg_ok = LTP_CASES.len() >= 5
        && LTP_CASES.iter().all(|row| !row.0.is_empty() && row.1 > 0)
        && expected_errno("read-badfd") == Some(9)
        && expected_errno("open-enoent") == Some(2);
    s.add("LTP 面注册表：case↔errno 绑定齐全且表内可查", reg_ok,
          "预期差登记必须指向注册表内面，表外一律拒绝");

    // 3. 登记规格解析器：合法规格全通过
    let parse_ok = match parse_diff_spec("E77:lseek-einval=22:越界偏移面") {
        Ok(sp) => sp.id == 77 && sp.case == "lseek-einval" && sp.errno == 22
            && !sp.reason.is_empty(),
        Err(_) => false,
    };
    s.add("规格解析器：E<n>:<case>=<errno>:<事由> 全解析", parse_ok,
          "登记规格是入账唯一入口，面/码/事由三验");

    // 4. 非法规格注入：错码/未知面/空事由/缺段 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_diff_spec("E1:open-enoent=9:错码").is_ok()
            || parse_diff_spec("E1:no-such=2:野面").is_ok()
            || parse_diff_spec("E1:open-enoent=2:").is_ok()
            || parse_diff_spec("E1:open-enoent").is_ok()
            || parse_diff_spec("X1:open-enoent=2:事由").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("非法规格注入：错码/野面/空事由/缺段 10/10 拒绝", amb_ok,
          "规格歧义必须逐次显性拒绝，不许静默放行");

    // 5. 双账构建：三红入账、逐条登记、对账零绕
    let mut l = ExpDiffLedger::new();
    let f1 = l.feed_red(11);
    let f2 = l.feed_red(12);
    let f3 = l.feed_red(13);
    let r1 = l.register("E11:read-badfd=9:坏 fd");
    let r2 = l.register("E12:open-enoent=2:缺路径");
    let r3 = l.register("E13:pipe-epipe=32:断端写");
    let build_ok = f1.is_ok() && f2.is_ok() && f3.is_ok()
        && r1.is_ok() && r2.is_ok() && r3.is_ok()
        && l.red_count() == 3 && l.diff_count() == 3 && l.no_bypass();
    s.add("双账构建：三红三登记且对账零绕行", build_ok,
          "每条 J2 红恰有一条同号登记，绕登记即失守恒");

    // 6. 生命周期：Open→Confirmed→Fed 单向推进
    let mut life_ok = r1 == Ok(DiffStage::Open);
    life_ok = life_ok && l.promote(11) == Ok(DiffStage::Confirmed)
        && l.promote(11) == Ok(DiffStage::Fed)
        && l.promote(11).is_err();
    s.add("生命周期：差异态单向推进，到底后越态拒绝", life_ok,
          "差异态必须 Open→Confirmed→Fed 单向，越态显性拒绝");

    // 7. 注入核：四类坏登记样本各 10/10 拒绝
    let (inj, per_class) = run_bad_diff_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏登记样本各 10/10 拒绝", all_caught,
          "绕登记/重复登记/errno 不符/未知面必须逐类逐次拒绝");

    // 8. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadDiff::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 9. 账外登记拒绝：未入红先登记
    let mut l2 = ExpDiffLedger::new();
    s.add("账外登记拒绝：登记号必须在红馈账",
          l2.register("E99:read-badfd=9:无红登记").err() == Some(ExpDiffCode::NotARed),
          "预期差只能挂在已入账 J2 红上，账外登记即拒");

    // 10. 重复红入账拒绝
    let mut l3 = ExpDiffLedger::new();
    let _ = l3.feed_red(21);
    s.add("重复红拒绝：同号红不重复入账", l3.feed_red(21).err() == Some(ExpDiffCode::DupDiff),
          "同号红重复入账会破坏一红一登记守恒，必须拒绝");

    // 11. 绕登记对账暴露：红多登记少审计翻红
    let mut l4 = ExpDiffLedger::new();
    let _ = l4.feed_red(31);
    let _ = l4.feed_red(32);
    let _ = l4.register("E31:write-badfd=9:只读 fd 写");
    let audit_ok = !l4.no_bypass() && {
        let _ = l4.register("E32:fstat-efault=14:坏缓冲");
        l4.no_bypass()
    };
    s.add("绕登记对账：差额暴露、补登记后翻绿", audit_ok,
          "绕登记即红多登记少，对账审计必须即时暴露");

    // 12. 成文非空且含面与码 + 双跑一致
    let sum = compose_diff_summary(&l, 12);
    let text_ok = match &sum {
        Ok(t) => t.contains("@open-enoent=2") && t.contains("态=Open"),
        Err(_) => false,
    };
    s.add("登记成文：面/码/态齐备且两次成文逐字节一致",
          text_ok && sum == compose_diff_summary(&l, 12),
          "成文必须携带 面/码/态 标记且为纯函数");

    // 13. 未登记成文拒绝
    s.add("未登记成文拒绝：账外红不得产出小结",
          compose_diff_summary(&l, 999).err() == Some(ExpDiffCode::BypassRegistration),
          "未登记红产出小结即绕登记虚报");

    // 14. Linux 逐码一致：LTP 面预期码全在表
    let codes_ok = agrees_with_linux(9) && agrees_with_linux(2) && agrees_with_linux(22)
        && agrees_with_linux(32) && agrees_with_linux(14)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：五面预期码全表内精确", codes_ok,
          "预期差登记码必须落在 errno 对照表闭集内");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < ExpDiffCode::ALL.len() {
        let mut j = i + 1;
        while j < ExpDiffCode::ALL.len() {
            if let (Some(a), Some(b)) = (ExpDiffCode::ALL.get(i), ExpDiffCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = ExpDiffCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 注册表往返 + 双跑指纹
    let rt_ok = LTP_CASES.iter().all(|row| expected_errno(row.0) == Some(row.1));
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：面标签可回查且双跑哈希相等", rt_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum { Ok(t) => t.len(), Err(_) => 0 };
    let sum_text = format!("登记小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_binds_case_errno() {
        assert_eq!(expected_errno("write-badfd"), Some(9));
        assert_eq!(expected_errno("lseek-einval"), Some(22));
        assert_eq!(expected_errno("nope"), None);
    }

    #[test]
    fn spec_parser_roundtrip() {
        let sp = parse_diff_spec("E9:pipe-epipe=32:断端写").unwrap_or(DiffSpec {
            id: 0,
            case: String::new(),
            errno: 0,
            reason: String::new(),
        });
        assert_eq!(sp.id, 9);
        assert_eq!(sp.case, "pipe-epipe");
        assert_eq!(sp.errno, 32);
    }

    #[test]
    fn spec_parser_rejects() {
        assert_eq!(parse_diff_spec("").err(), Some(ExpDiffCode::SpecMalformed));
        assert_eq!(parse_diff_spec("E1:open-enoent=7:x").err(), Some(ExpDiffCode::ErrnoMismatch));
        assert_eq!(parse_diff_spec("E1:ghost=2:x").err(), Some(ExpDiffCode::UnknownLtpCase));
        assert_eq!(parse_diff_spec("E1:open-enoent=2:").err(), Some(ExpDiffCode::EmptyReason));
    }

    #[test]
    fn register_requires_red() {
        let mut l = ExpDiffLedger::new();
        assert_eq!(l.register("E5:read-badfd=9:x").err(), Some(ExpDiffCode::NotARed));
        let _ = l.feed_red(5);
        assert!(l.register("E5:read-badfd=9:x").is_ok());
        assert_eq!(l.register("E5:read-badfd=9:y").err(), Some(ExpDiffCode::DupDiff));
    }

    #[test]
    fn stage_monotonic() {
        let mut l = ExpDiffLedger::new();
        let _ = l.feed_red(7);
        let _ = l.register("E7:write-badfd=9:x");
        assert_eq!(l.promote(7), Ok(DiffStage::Confirmed));
        assert_eq!(l.promote(7), Ok(DiffStage::Fed));
        assert!(l.promote(7).is_err());
    }

    #[test]
    fn bypass_exposed_by_audit() {
        let mut l = ExpDiffLedger::new();
        let _ = l.feed_red(8);
        let _ = l.feed_red(9);
        let _ = l.register("E8:open-enoent=2:x");
        assert!(!l.no_bypass());
        assert!(compose_diff_summary(&l, 9).is_err());
    }

    #[test]
    fn dup_red_rejected() {
        let mut l = ExpDiffLedger::new();
        assert!(l.feed_red(10).is_ok());
        assert_eq!(l.feed_red(10).err(), Some(ExpDiffCode::DupDiff));
        assert_eq!(l.feed_red(0).err(), Some(ExpDiffCode::NotARed));
    }

    #[test]
    fn bad_diff_matrix_green() {
        let (inj, per) = run_bad_diff_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(14));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(32), -32);
    }

    #[test]
    fn unx10093_checks_green() {
        let set = run_unx10093_checks();
        assert!(set.all_passed(), "UNX-F10093 red: {}", set.red_items_text());
    }
}
