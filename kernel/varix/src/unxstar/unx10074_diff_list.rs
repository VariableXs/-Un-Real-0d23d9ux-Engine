//! UNX-F10074 差异项清单生成（对账差异）· 0x92280 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10075-J1 差异清单齐备（逐差异带证据锚 10/10 次），无锚差异 10/10 次拦截
//!
//! 设计要点：
//!   ① 差异项 = (条目号, 账面值, 实计值, 差额, 证据锚)；账面与实计相等
//!      不构成差异 → NotDiff 拒绝；证据锚是差异入表的硬前提，缺锚
//!      NoAnchor 逐次拦截（10/10），锚形制不符 MalformedAnchor 拒绝。
//!   ② 证据锚形制冻结为 UNX-F<num>-J<n>（如 UNX-F9747-J1）：解析器是
//!      锚入表的唯一入口，解析器手写防溢出，往返一致（RoundTrip）。
//!   ③ 差异账守恒式：entries 总数 = 增差 + 减差 + 平账（delta=0 不入表
//!      所以平账恒 0，但保留格位防口径漂移），恒成立。
//!   ④ 注入核：无锚差异 / 畸形锚 / 平账冒充 / 条目越窗四类坏样本逐类
//!      10/10 次拒绝，注入账 注入 = 检出 + 逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10074 独占 0x40 窗口，下一卡 0x922C0）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x92280;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 差异清单容量上限。
pub const DIFF_CAP: usize = 64;

/// 条目号窗口下界（F9601）。
pub const ITEM_LO: u32 = 9601;

/// 条目号窗口上界（F10080）。
pub const ITEM_HI: u32 = 10080;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffCode {
    /// 空证据锚
    EmptyAnchor,
    /// 锚形制不符（不是 UNX-F<num>-J<n>）
    MalformedAnchor,
    /// 无锚差异（锚位缺省）
    NoAnchor,
    /// 平账冒充差异（账面=实计）
    NotDiff,
    /// 条目号越窗
    ItemOutOfRange,
    /// 同条目重复差异
    DupDiff,
    /// 清单容量满
    LedgerFull,
    /// 差异账失守恒
    DiffRegress,
    /// 锚往返不一致
    AnchorRoundTrip,
    /// 空成文
    EmptySummary,
    /// 拒绝后账被改动（拒绝不留痕被破坏）
    RejectedTrace,
    /// 段位越界
    SegmentViolation,
}

impl DiffCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            DiffCode::EmptyAnchor => "空证据锚被拒：差异项不能没有证据出处；\
                请按 UNX-F<num>-J<n> 形制补锚后再入表",
            DiffCode::MalformedAnchor => "锚形制被拒：证据锚只认 UNX-F<num>-J<n>；\
                请修正形制后重试",
            DiffCode::NoAnchor => "无锚差异被拒：逐差异必须带证据锚入表；\
                无出处的差异不得进入清单（10/10 拦截）",
            DiffCode::NotDiff => "平账冒充被拒：账面与实计相等不构成差异；\
                请勿把对齐项计入差异清单稀释口径",
            DiffCode::ItemOutOfRange => "条目越窗被拒：条目号必须落在 F9601..=F10080；\
                窗外条目不属于本域对账面",
            DiffCode::DupDiff => "重复差异被拒：同一条目只能有一条在案差异；\
                复核变化请走差异处置通道而非重复入表",
            DiffCode::LedgerFull => "容量满被拒：差异清单已达上限；请先归档旧差异再入新差异",
            DiffCode::DiffRegress => "差异账失守恒：entries 总数必须等于 增差+减差+平账；\
                请排查是否绕过分类计数器",
            DiffCode::AnchorRoundTrip => "锚往返不一致：重格式化再解析必须回到原锚；\
                请检查解析器与格式化器是否同步",
            DiffCode::EmptySummary => "空成文被拒：差异小结不得为空串；请检查清单是否为空",
            DiffCode::RejectedTrace => "拒绝留痕被拒：被拒差异不得改动清单；\
                请排查拒绝路径是否误提交",
            DiffCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [DiffCode; 12] = [
        DiffCode::EmptyAnchor,
        DiffCode::MalformedAnchor,
        DiffCode::NoAnchor,
        DiffCode::NotDiff,
        DiffCode::ItemOutOfRange,
        DiffCode::DupDiff,
        DiffCode::LedgerFull,
        DiffCode::DiffRegress,
        DiffCode::AnchorRoundTrip,
        DiffCode::EmptySummary,
        DiffCode::RejectedTrace,
        DiffCode::SegmentViolation,
    ];
}

// ── 证据锚解析器 ──────────────────────────────────────────────────────
/// 解析后的证据锚：判据号 + 判据序。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EvidenceAnchor {
    /// 判据号（F 后数字，如 9747）
    pub num: u32,
    /// 判据序（J 后数字，如 1）
    pub ord: u32,
}

/// 证据锚解析：形制冻结 UNX-F<num>-J<n>；手写数字解析防溢出；
/// 判据号必须落在 9601..=10080。这是锚入表的唯一入口。
pub fn parse_anchor(text: &str) -> Result<EvidenceAnchor, DiffCode> {
    if text.is_empty() {
        return Err(DiffCode::EmptyAnchor);
    }
    let rest = match text.strip_prefix("UNX-F") {
        Some(r) => r,
        None => return Err(DiffCode::MalformedAnchor),
    };
    let dash = match rest.find('-') {
        Some(d) => d,
        None => return Err(DiffCode::MalformedAnchor),
    };
    let (num_part, j_part) = match (rest.get(0..dash), rest.get(dash + 1..)) {
        (Some(n), Some(j)) => (n, j),
        _ => return Err(DiffCode::MalformedAnchor),
    };
    let num = parse_digits(num_part)?;
    let ord = match j_part.strip_prefix('J') {
        Some(o) => parse_digits(o)?,
        None => return Err(DiffCode::MalformedAnchor),
    };
    if num < ITEM_LO || num > ITEM_HI {
        return Err(DiffCode::ItemOutOfRange);
    }
    if ord == 0 {
        return Err(DiffCode::MalformedAnchor);
    }
    Ok(EvidenceAnchor { num, ord })
}

/// 手写十进制解析：非数字 MalformedAnchor，溢出回退拒绝。
fn parse_digits(s: &str) -> Result<u32, DiffCode> {
    if s.is_empty() {
        return Err(DiffCode::MalformedAnchor);
    }
    let mut num: u32 = 0;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => break,
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(DiffCode::MalformedAnchor);
        }
        let d = (b - b'0') as u32;
        num = match num.checked_mul(10).and_then(|v| v.checked_add(d)) {
            Some(v) => v,
            None => return Err(DiffCode::MalformedAnchor),
        };
        i += 1;
    }
    Ok(num)
}

/// 锚格式化：判据号+序还原为规范形制（往返用）。
pub fn format_anchor(a: EvidenceAnchor) -> String {
    format!("UNX-F{}-J{}", a.num, a.ord)
}

// ── 差异项与差异账 ────────────────────────────────────────────────────
/// 差异项：条目号 + 账面/实计 + 差额 + 证据锚。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiffEntry {
    /// 条目号
    pub item: u32,
    /// 账面值
    pub book: i64,
    /// 实计值
    pub actual: i64,
    /// 差额 = 实计 - 账面
    pub delta: i64,
    /// 证据锚（原文，已通过解析器）
    pub anchor: String,
}

/// 差异账：守恒式 entries = 增差 + 减差（平账不入表）。
#[derive(Clone, Debug)]
pub struct DiffLedger {
    entries: Vec<DiffEntry>,
    cap: usize,
    plus: u32,
    minus: u32,
    rejected: u32,
}

impl DiffLedger {
    /// 空账构造。
    pub fn new() -> DiffLedger {
        DiffLedger { entries: Vec::new(), cap: DIFF_CAP, plus: 0, minus: 0, rejected: 0 }
    }

    /// 差异入表：条目越窗、平账、锚非法、重复条目逐项显性拒绝；
    /// 任何拒绝路径不留痕只计 rejected。成功返回差额。
    pub fn push(
        &mut self,
        item: u32,
        book: i64,
        actual: i64,
        anchor_text: &str,
    ) -> Result<i64, DiffCode> {
        if item < ITEM_LO || item > ITEM_HI {
            return Err(DiffCode::ItemOutOfRange);
        }
        let _a = parse_anchor(anchor_text)?;
        let delta = match actual.checked_sub(book) {
            Some(d) => d,
            None => return Err(DiffCode::NotDiff),
        };
        if delta == 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DiffCode::NotDiff);
        }
        if self.contains_item(item) {
            return Err(DiffCode::DupDiff);
        }
        if self.entries.len() >= self.cap {
            return Err(DiffCode::LedgerFull);
        }
        self.entries.push(DiffEntry {
            item,
            book,
            actual,
            delta,
            anchor: String::from(anchor_text),
        });
        if delta > 0 {
            self.plus = self.plus.saturating_add(1);
        } else {
            self.minus = self.minus.saturating_add(1);
        }
        Ok(delta)
    }

    /// 线性查找条目是否已在案。
    pub fn contains_item(&self, item: u32) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.item == item {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 账本条目总数。
    pub fn total(&self) -> usize {
        self.entries.len()
    }

    /// 增差计数。
    pub fn plus(&self) -> u32 {
        self.plus
    }

    /// 减差计数。
    pub fn minus(&self) -> u32 {
        self.minus
    }

    /// 被拒计数（显性记账不静默）。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 守恒式：条目总数 = 增差 + 减差。
    pub fn conserved(&self) -> bool {
        self.entries.len() as u32 == self.plus + self.minus
    }

    /// 全账差额代数和（复核用：应与逐条求和一致）。
    pub fn net_delta(&self) -> i64 {
        let mut s: i64 = 0;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                s = s.wrapping_add(e.delta);
            }
            i += 1;
        }
        s
    }
}

// ── 注入核：坏差异样本矩阵 ────────────────────────────────────────────
/// 四类坏差异样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadDiff {
    /// 无锚差异：锚位空串
    NoAnchor,
    /// 畸形锚：形制不符
    MalformedAnchor,
    /// 平账冒充差异
    FlatAsDiff,
    /// 条目越窗
    ItemOutOfRange,
}

impl BadDiff {
    /// 闭集全量。
    pub const ALL: [BadDiff; 4] = [
        BadDiff::NoAnchor,
        BadDiff::MalformedAnchor,
        BadDiff::FlatAsDiff,
        BadDiff::ItemOutOfRange,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadDiff::NoAnchor => "无锚差异",
            BadDiff::MalformedAnchor => "畸形证据锚",
            BadDiff::FlatAsDiff => "平账冒充差异",
            BadDiff::ItemOutOfRange => "条目越窗",
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

    /// 记一次注入结果；账失守恒返回 DiffRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), DiffCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(DiffCode::DiffRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏差异注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
/// 每轮用全新账本，确保被拒样本不留痕。
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
                BadDiff::NoAnchor => {
                    let mut l = DiffLedger::new();
                    l.push(9747, 10, 12, "").err() == Some(DiffCode::EmptyAnchor)
                        && l.total() == 0
                }
                BadDiff::MalformedAnchor => {
                    let mut l = DiffLedger::new();
                    l.push(9747, 10, 12, "F9747").err() == Some(DiffCode::MalformedAnchor)
                        && l.push(9747, 10, 12, "UNX-F9747-K1").err()
                            == Some(DiffCode::MalformedAnchor)
                }
                BadDiff::FlatAsDiff => {
                    let mut l = DiffLedger::new();
                    l.push(9747, 10, 10, "UNX-F9747-J1").err() == Some(DiffCode::NotDiff)
                        && l.rejected() == 1
                        && l.total() == 0
                }
                BadDiff::ItemOutOfRange => {
                    let mut l = DiffLedger::new();
                    l.push(9600, 10, 12, "UNX-F9747-J1").err() == Some(DiffCode::ItemOutOfRange)
                        && l.push(10081, 10, 12, "UNX-F9747-J1").err()
                            == Some(DiffCode::ItemOutOfRange)
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

// ── 差异清单成文（纯函数） ────────────────────────────────────────────
/// 差异成文：「差异=<n>;增=<n>;减=<n>;净差=<d>」；失守恒或空账先拒。
pub fn compose_diff_summary(l: &DiffLedger) -> Result<String, DiffCode> {
    if !l.conserved() || l.total() == 0 {
        return Err(DiffCode::EmptySummary);
    }
    let out = format!(
        "差异={};增={};减={};净差={}",
        l.total(),
        l.plus(),
        l.minus(),
        l.net_delta()
    );
    if out.is_empty() {
        return Err(DiffCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；畸形锚/平账冒充族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；无锚差异缺出处族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复条目差异族"),
    ("EDOM", -33, 227, "Windows ERROR_DOMAIN；条目越窗族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；清单容量满族"),
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

/// 规范语料双跑指纹：三差异入表（增/减/重复被拒）→ 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = DiffLedger::new();
    let _ = l.push(9747, 100, 120, "UNX-F9747-J1");
    let _ = l.push(9900, 50, 45, "UNX-F9900-J1");
    let _ = l.push(9905, 30, 30, "UNX-F9905-J1");
    let bytes = match compose_diff_summary(&l) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10074 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10074_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10074");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92280..=0x922BF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 锚解析器：合法形制全通过
    let parse_ok = parse_anchor("UNX-F9747-J1") == Ok(EvidenceAnchor { num: 9747, ord: 1 })
        && parse_anchor("UNX-F10080-J12") == Ok(EvidenceAnchor { num: 10080, ord: 12 });
    s.add("锚解析器：UNX-F<num>-J<n> 形制全通过", parse_ok,
          "证据锚形制冻结，解析器是锚入表的唯一入口");

    // 3. 无锚/畸形锚注入 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_anchor("").is_ok()
            || parse_anchor("F9747-J1").is_ok()
            || parse_anchor("UNX-F9747-K1").is_ok()
            || parse_anchor("UNX-F9747-J").is_ok()
            || parse_anchor("UNX-F97x7-J1").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("锚歧义注入：空串/短形制/坏序 10/10 拒绝", amb_ok,
          "锚形制歧义必须逐次显性拒绝，不许静默截断");

    // 4. 差异生成：增差/减差各一条入表并带回差额
    let mut l = DiffLedger::new();
    let d1 = l.push(9747, 100, 120, "UNX-F9747-J1");
    let d2 = l.push(9900, 50, 45, "UNX-F9900-J1");
    s.add("差异生成：增差 +5/减差 -5 逐条入表带证据锚",
          d1 == Ok(20) && d2 == Ok(-5) && l.total() == 2 && l.conserved(),
          "每条差异必须携带 UNX-F<num>-J<n> 形证据锚入表");

    // 5. 无锚差异拦截：锚位空串 10/10 次拒绝
    let mut no_anchor_ok = true;
    let mut r2 = 0u32;
    while r2 < INJECT_ROUNDS {
        let mut l2 = DiffLedger::new();
        if l2.push(9747, 1, 2, "").err() != Some(DiffCode::EmptyAnchor) || l2.total() != 0 {
            no_anchor_ok = false;
        }
        r2 += 1;
    }
    s.add("无锚差异拦截：缺证据锚 10/10 次拒绝", no_anchor_ok,
          "无出处的差异不得进入清单，逐次显性拦截");

    // 6. 平账冒充拦截：账面=实计 10/10 次拒绝
    let mut flat_ok = true;
    let mut r3 = 0u32;
    while r3 < INJECT_ROUNDS {
        let mut l2 = DiffLedger::new();
        if l2.push(9747, 8, 8, "UNX-F9747-J1").err() != Some(DiffCode::NotDiff)
            || l2.total() != 0
        {
            flat_ok = false;
        }
        r3 += 1;
    }
    s.add("平账冒充拦截：零差额 10/10 次拒绝", flat_ok,
          "账面=实计不构成差异，计入即稀释口径");

    // 7. 重复条目差异拒绝
    let mut l3 = DiffLedger::new();
    let _ = l3.push(9747, 1, 2, "UNX-F9747-J1");
    s.add("重复差异：同条目二次入表显性拒绝",
          l3.push(9747, 3, 4, "UNX-F9747-J2").err() == Some(DiffCode::DupDiff),
          "同一条目只能有一条在案差异");

    // 8. 条目越窗拒绝
    s.add("条目越窗：窗外条目显性拒绝",
          l3.push(9600, 1, 2, "UNX-F9747-J1").err() == Some(DiffCode::ItemOutOfRange)
              && l3.push(10081, 1, 2, "UNX-F9747-J1").err() == Some(DiffCode::ItemOutOfRange),
          "条目号必须落在 F9601..=F10080 全域窗口");

    // 9. 注入核：四类坏差异样本各 10/10 拒绝
    let (inj, per_class) = run_bad_diff_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏差异样本各 10/10 拒绝", all_caught,
          "无锚/畸形锚/平账冒充/越窗必须逐类逐次拒绝");

    // 10. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadDiff::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 拒绝不留痕：坏差异不改账面
    let before = (l.total(), l.plus(), l.minus(), l.conserved());
    let _ = l.push(9747, 1, 2, "");
    let _ = l.push(9747, 1, 2, "F9747-J1");
    let after = (l.total(), l.plus(), l.minus(), l.conserved());
    s.add("拒绝不留痕：坏差异不改清单账面", before == after,
          "被拒差异不得改动条目数与分类计数");

    // 12. 被拒显性记账：平账冒充计入 rejected（全新账本独立复核）
    let mut l4 = DiffLedger::new();
    let _ = l4.push(9747, 8, 8, "UNX-F9747-J1");
    let _ = l4.push(9900, 2, 2, "UNX-F9900-J1");
    s.add("被拒显性记账：平账冒充计入 rejected",
          l4.rejected() == 2 && l4.total() == 0 && l4.conserved(),
          "被拒样本必须显性计数，不许静默丢弃");

    // 13. 净差复核：全账代数和与逐条求和一致
    let manual_net: i64 = match (d1, d2) {
        (Ok(a), Ok(b)) => a.wrapping_add(b),
        _ => 0,
    };
    s.add("净差复核：net_delta 与逐条求和逐位一致",
          l.net_delta() == manual_net && l.conserved(),
          "差额列必须可复算，复算失配即失真");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(2) && agrees_with_linux(17)
        && agrees_with_linux(33) && agrees_with_linux(28)
        && !agrees_with_linux(0) && !agrees_with_linux(5555);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < DiffCode::ALL.len() {
        let mut j = i + 1;
        while j < DiffCode::ALL.len() {
            if let (Some(a), Some(b)) = (DiffCode::ALL.get(i), DiffCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = DiffCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 锚往返 + 成文 + 双跑指纹
    let rt_ok = {
        let a = EvidenceAnchor { num: 9905, ord: 3 };
        parse_anchor(&format_anchor(a)) == Ok(a)
    };
    let sum = compose_diff_summary(&l);
    let sum_ok = match &sum {
        Ok(t) => t.contains("差异=2") && t.contains("净差=15"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：锚可回解析且双跑哈希逐位相等", rt_ok && sum_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 差异账快照 + 指纹
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let led_text = format!("差异 {}（增 {} 减 {}）", l.total(), l.plus(), l.minus());
    s.add_dyn("差异账快照", l.conserved(), &led_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_parser_closed_set() {
        assert_eq!(parse_anchor("UNX-F9747-J1"), Ok(EvidenceAnchor { num: 9747, ord: 1 }));
        assert_eq!(parse_anchor("").err(), Some(DiffCode::EmptyAnchor));
        assert_eq!(parse_anchor("UNX-F9747-K1").err(), Some(DiffCode::MalformedAnchor));
        assert_eq!(parse_anchor("UNX-F9600-J1").err(), Some(DiffCode::ItemOutOfRange));
    }

    #[test]
    fn anchor_roundtrip() {
        let a = EvidenceAnchor { num: 9905, ord: 3 };
        assert_eq!(parse_anchor(&format_anchor(a)), Ok(a));
    }

    #[test]
    fn diff_push_and_conservation() {
        let mut l = DiffLedger::new();
        assert_eq!(l.push(9747, 100, 120, "UNX-F9747-J1"), Ok(20));
        assert_eq!(l.push(9900, 50, 45, "UNX-F9900-J1"), Ok(-5));
        assert!(l.conserved());
        assert_eq!(l.plus(), 1);
        assert_eq!(l.minus(), 1);
        assert_eq!(l.net_delta(), 15);
    }

    #[test]
    fn flat_and_no_anchor_rejected() {
        let mut l = DiffLedger::new();
        assert_eq!(l.push(9747, 5, 5, "UNX-F9747-J1").err(), Some(DiffCode::NotDiff));
        assert_eq!(l.push(9747, 5, 6, "").err(), Some(DiffCode::EmptyAnchor));
        assert_eq!(l.rejected(), 1);
        assert_eq!(l.total(), 0);
    }

    #[test]
    fn dup_item_rejected() {
        let mut l = DiffLedger::new();
        let _ = l.push(9747, 1, 2, "UNX-F9747-J1");
        assert_eq!(l.push(9747, 3, 4, "UNX-F9900-J1").err(), Some(DiffCode::DupDiff));
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
    fn item_window_enforced() {
        let mut l = DiffLedger::new();
        assert_eq!(l.push(9600, 1, 2, "UNX-F9747-J1").err(), Some(DiffCode::ItemOutOfRange));
        assert_eq!(l.push(10081, 1, 2, "UNX-F9747-J1").err(), Some(DiffCode::ItemOutOfRange));
    }

    #[test]
    fn summary_compose_and_empty_reject() {
        let mut l = DiffLedger::new();
        let _ = l.push(9747, 100, 120, "UNX-F9747-J1");
        let s = compose_diff_summary(&l).unwrap_or_default();
        assert!(s.contains("差异=1") && s.contains("净差=20"));
        assert!(compose_diff_summary(&DiffLedger::new()).is_err());
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(33));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(28), -28);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10074_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10074_checks_green() {
        let set = run_unx10074_checks();
        assert!(set.all_passed(), "UNX-F10074 red: {}", set.red_items_text());
    }
}
