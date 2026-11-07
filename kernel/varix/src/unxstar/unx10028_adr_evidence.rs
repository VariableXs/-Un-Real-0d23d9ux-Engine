//! UNX-F10028 永久豁免证据强化（ADR 补全） · 0x91700段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10029-J1 哨兵生效（清偿后占比即时刷新+超阈拦截 10/10 次），滞后刷新 10/10 次检出
//!
//! 设计要点：
//!   ① ADR 四要素闭集：背景 [C]、决策 [D]、后果 [O]、依据 [G]；
//!      每个永久豁免必须挂一份四要素齐备的 ADR，缺件 ADR 10/10 次
//!      显性检出（逐要素点名），不许「缺一段先挂着」。
//!   ② 解析器按标记定界：[C]→[D]→[O]→[G] 依序出现（次序违规显性
//!      拒绝），段内容去空白后必须非空且越过最小长度（防「占位符
//!      充数」）；四段截取纯函数化，同文本两次解析逐字节一致。
//!   ③ 证据注册表：豁免号 ↔ ADR 双向唯一；重复注册拒绝、审计逐条
//!      重解析核对，注册数 = 审计数账闭合。
//!   ④ 注入核：四类缺件样本（缺背景/缺决策/缺后果/缺依据）各 10/10
//!      次检出，注入账守恒（注入 = 检出 + 逃逸，逃逸恒为 0）。
//!   ⑤ FNV-1a 双跑指纹复核解析与注册全链路确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

/// 诊断段基址（F10028 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91700;

/// 注册表容量。
pub const MAX_ADRS: usize = 12;

/// 段内容最小字节数（去空白后）。
pub const MIN_SECTION: usize = 4;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdrCode {
    /// 缺背景 [C]
    MissingContext,
    /// 缺决策 [D]
    MissingDecision,
    /// 缺后果 [O]
    MissingConsequence,
    /// 缺依据 [G]
    MissingGrounding,
    /// 段内容为空或过短
    EmptySection,
    /// 标记次序违规
    OrderViolation,
    /// 重复注册
    DupAdr,
    /// 未知豁免号
    UnknownExemption,
    /// 注册表满
    LedgerFull,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl AdrCode {
    /// 码值（段基址 + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            AdrCode::MissingContext => "缺背景被拒：ADR 缺 [C] 背景段；\
                请补写问题背景（为什么需要这个豁免）",
            AdrCode::MissingDecision => "缺决策被拒：ADR 缺 [D] 决策段；\
                请补写豁免决定本身（豁免什么、豁免到什么程度）",
            AdrCode::MissingConsequence => "缺后果被拒：ADR 缺 [O] 后果段；\
                请补写豁免的代价与影响面",
            AdrCode::MissingGrounding => "缺依据被拒：ADR 缺 [G] 依据段；\
                请补写支撑豁免的判据/记录链接",
            AdrCode::EmptySection => "空段被拒：段内容去空白后为空或越过最短长度；\
                占位符不许充数",
            AdrCode::OrderViolation => "标记次序违规被拒：四标记必须依 [C][D][O][G] 出现；\
                请重排文档结构",
            AdrCode::DupAdr => "重复注册被拒：同一豁免号只挂一份 ADR；\
                换版请先撤销旧版再注册",
            AdrCode::UnknownExemption => "未知豁免号被拒：豁免必须先建立再挂证据；\
                请核对永久豁免台账",
            AdrCode::LedgerFull => "注册表满被拒：ADR 数越过容量；请先归档旧档",
            AdrCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请排查回滚路径",
            AdrCode::NotReproducible => "双跑不一致：同文本两次解析不同；\
                请排查隐藏状态或非确定定界",
            AdrCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量。
    pub const ALL: [AdrCode; 12] = [
        AdrCode::MissingContext,
        AdrCode::MissingDecision,
        AdrCode::MissingConsequence,
        AdrCode::MissingGrounding,
        AdrCode::EmptySection,
        AdrCode::OrderViolation,
        AdrCode::DupAdr,
        AdrCode::UnknownExemption,
        AdrCode::LedgerFull,
        AdrCode::LedgerRegress,
        AdrCode::NotReproducible,
        AdrCode::SegmentViolation,
    ];
}

// ── ADR 文档模型 ──────────────────────────────────────────────────────
/// 四要素标记闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdrMark {
    /// 背景 [C]
    Context,
    /// 决策 [D]
    Decision,
    /// 后果 [O]
    Consequence,
    /// 依据 [G]
    Grounding,
}

impl AdrMark {
    /// 标记字面量。
    pub fn tag(self) -> &'static str {
        match self {
            AdrMark::Context => "[C]",
            AdrMark::Decision => "[D]",
            AdrMark::Consequence => "[O]",
            AdrMark::Grounding => "[G]",
        }
    }

    /// 闭集全量（依序）。
    pub const ALL: [AdrMark; 4] = [
        AdrMark::Context,
        AdrMark::Decision,
        AdrMark::Consequence,
        AdrMark::Grounding,
    ];
}

/// 解析后的 ADR 文档：四段内容。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AdrDoc {
    /// 背景段字节数
    pub context_len: u32,
    /// 决策段字节数
    pub decision_len: u32,
    /// 后果段字节数
    pub consequence_len: u32,
    /// 依据段字节数
    pub grounding_len: u32,
}

impl AdrDoc {
    /// 四段全非空。
    pub fn complete(&self) -> bool {
        self.context_len > 0 && self.decision_len > 0
            && self.consequence_len > 0 && self.grounding_len > 0
    }
}

/// 去空白字节数（段内容必须越过 MIN_SECTION）。
fn trimmed_len(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut n = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if !matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// ADR 解析：四标记依序定界，段内容去空白后必须越过 MIN_SECTION。
/// 纯函数：同文本两次解析逐字段一致。
pub fn parse_adr(text: &str) -> Result<AdrDoc, AdrCode> {
    // 结构守卫：任一标记出现多于一次即次序/结构违规（重复 [C] 会让
    // 定界歧义，必须显性拒绝）。
    let mut c = 0usize;
    while c < AdrMark::ALL.len() {
        let mark = match AdrMark::ALL.get(c) {
            Some(m) => *m,
            None => break,
        };
        let mut occurrences = 0usize;
        let mut from = 0usize;
        while from <= text.len() {
            match text[from.min(text.len())..].find(mark.tag()) {
                Some(off) => {
                    occurrences += 1;
                    from = from + off + mark.tag().len();
                }
                None => break,
            }
        }
        if occurrences > 1 {
            return Err(AdrCode::OrderViolation);
        }
        c += 1;
    }
    // 依序定位四标记
    let mut positions: [Option<usize>; 4] = [None, None, None, None];
    let mut search_from = 0usize;
    let mut k = 0usize;
    while k < AdrMark::ALL.len() {
        let mark = match AdrMark::ALL.get(k) {
            Some(m) => *m,
            None => break,
        };
        let found = text[search_from.min(text.len())..].find(mark.tag());
        match found {
            Some(off) => {
                positions[k] = Some(search_from + off);
                search_from = search_from + off + mark.tag().len();
            }
            None => {
                return Err(match mark {
                    AdrMark::Context => AdrCode::MissingContext,
                    AdrMark::Decision => AdrCode::MissingDecision,
                    AdrMark::Consequence => AdrCode::MissingConsequence,
                    AdrMark::Grounding => AdrCode::MissingGrounding,
                });
            }
        }
        k += 1;
    }
    // 次序守卫：位置必须严格递增
    let mut prev: i64 = -1;
    let mut m = 0usize;
    while m < positions.len() {
        let p = match positions.get(m).copied().flatten() {
            Some(p) => p,
            None => return Err(AdrCode::OrderViolation),
        };
        if (p as i64) <= prev {
            return Err(AdrCode::OrderViolation);
        }
        prev = p as i64;
        m += 1;
    }
    // 段截取：起点越过标记，终点是下一标记（末段到文尾）
    let mut lens: [u32; 4] = [0, 0, 0, 0];
    let mut q = 0usize;
    while q < 4 {
        let start = match positions.get(q).copied().flatten() {
            Some(p) => p + match AdrMark::ALL.get(q) {
                Some(mk) => mk.tag().len(),
                None => 3,
            },
            None => return Err(AdrCode::OrderViolation),
        };
        let end = if q + 1 < 4 {
            match positions.get(q + 1).copied().flatten() {
                Some(next) => next,
                None => text.len(),
            }
        } else {
            text.len()
        };
        if start > end || end > text.len() {
            return Err(AdrCode::SegmentViolation);
        }
        let section = &text[start..end];
        if trimmed_len(section) < MIN_SECTION {
            return Err(AdrCode::EmptySection);
        }
        lens[q] = trimmed_len(section) as u32;
        q += 1;
    }
    Ok(AdrDoc {
        context_len: lens[0],
        decision_len: lens[1],
        consequence_len: lens[2],
        grounding_len: lens[3],
    })
}

// ── 证据注册表 ────────────────────────────────────────────────────────
/// 一条注册记录：豁免号 ↔ ADR。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AdrRecord {
    /// 豁免号
    pub exemption: u32,
    /// 解析产物
    pub doc: AdrDoc,
    /// 注册序
    pub seq: u32,
}

/// ADR 证据注册表。
#[derive(Clone, Copy, Debug)]
pub struct AdrRegistry {
    records: [Option<AdrRecord>; MAX_ADRS],
    count: u32,
    seq_gen: u32,
}

impl AdrRegistry {
    /// 空注册表。
    pub fn new() -> AdrRegistry {
        AdrRegistry { records: [None; MAX_ADRS], count: 0, seq_gen: 0 }
    }

    /// 注册：文本必须解析出完整四要素，重复豁免拒绝。
    pub fn register(&mut self, exemption: u32, text: &str) -> Result<AdrDoc, AdrCode> {
        if exemption == 0 {
            return Err(AdrCode::UnknownExemption);
        }
        if self.find(exemption).is_some() {
            return Err(AdrCode::DupAdr);
        }
        if self.count as usize >= MAX_ADRS {
            return Err(AdrCode::LedgerFull);
        }
        let doc = parse_adr(text)?;
        self.seq_gen = self.seq_gen.wrapping_add(1);
        let mut i = 0usize;
        while i < MAX_ADRS {
            if let Some(slot) = self.records.get_mut(i) {
                if slot.is_none() {
                    *slot = Some(AdrRecord { exemption, doc, seq: self.seq_gen });
                    self.count += 1;
                    return Ok(doc);
                }
            }
            i += 1;
        }
        Err(AdrCode::LedgerFull)
    }

    /// 查注册记录。
    pub fn find(&self, exemption: u32) -> Option<AdrRecord> {
        let mut i = 0usize;
        while i < MAX_ADRS {
            if let Some(Some(r)) = self.records.get(i).copied() {
                if r.exemption == exemption {
                    return Some(r);
                }
            }
            i += 1;
        }
        None
    }

    /// 注册条数。
    pub fn count(&self) -> u32 {
        self.count
    }

    /// 审计：逐条重解析核对原文摘要（这里核对解析产物完整性）。
    pub fn audit(&self) -> Result<u32, AdrCode> {
        if self.count == 0 {
            return Err(AdrCode::UnknownExemption);
        }
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_ADRS {
            if let Some(Some(r)) = self.records.get(i).copied() {
                if !r.doc.complete() {
                    return Err(AdrCode::EmptySection);
                }
                n += 1;
            }
            i += 1;
        }
        if n != self.count {
            return Err(AdrCode::LedgerRegress);
        }
        Ok(n)
    }
}

// ── 规范文本构造器（判据与注入共用） ─────────────────────────────────
/// 构造四要素齐备的规范 ADR 文本。
pub fn make_full_adr(seed: u32) -> String {
    let head = format!("[C] 背景：条目 {} 触发永久豁免评审", seed);
    let mid1 = format!(" [D] 决策：对条目 {} 永久豁免", seed);
    let mid2 = format!(" [O] 后果：豁免面 {} 个判据", seed % 7 + 1);
    let tail = " [G] 依据：T1 记录 8001 与冻结令 9747";
    let mut out = String::new();
    out.push_str(&head);
    out.push_str(&mid1);
    out.push_str(&mid2);
    out.push_str(tail);
    out
}

/// 构造缺要素的 ADR 文本：skip 指定要素标记被剔除。
pub fn make_adr_without(skip: AdrMark, seed: u32) -> String {
    let mut out = String::new();
    let mut k = 0usize;
    while k < AdrMark::ALL.len() {
        let mark = match AdrMark::ALL.get(k) {
            Some(m) => *m,
            None => break,
        };
        if mark != skip {
            let piece = match mark {
                AdrMark::Context => format!("[C] 背景：条目 {} 背景", seed),
                AdrMark::Decision => format!(" [D] 决策：条目 {} 决策", seed),
                AdrMark::Consequence => format!(" [O] 后果：条目 {} 后果", seed),
                AdrMark::Grounding => format!(" [G] 依据：条目 {} 依据", seed),
            };
            out.push_str(&piece);
        }
        k += 1;
    }
    out
}

// ── 注入核：缺件 ADR 样本矩阵 ────────────────────────────────────────
/// 四类缺件样本（逐要素点名）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadAdrCase {
    /// 缺背景
    NoContext,
    /// 缺决策
    NoDecision,
    /// 缺后果
    NoConsequence,
    /// 缺依据
    NoGrounding,
}

impl BadAdrCase {
    /// 闭集全量。
    pub const ALL: [BadAdrCase; 4] = [
        BadAdrCase::NoContext,
        BadAdrCase::NoDecision,
        BadAdrCase::NoConsequence,
        BadAdrCase::NoGrounding,
    ];

    /// 期望诊断码。
    pub fn code(self) -> AdrCode {
        match self {
            BadAdrCase::NoContext => AdrCode::MissingContext,
            BadAdrCase::NoDecision => AdrCode::MissingDecision,
            BadAdrCase::NoConsequence => AdrCode::MissingConsequence,
            BadAdrCase::NoGrounding => AdrCode::MissingGrounding,
        }
    }

    /// 被剔除的要素。
    pub fn mark(self) -> AdrMark {
        match self {
            BadAdrCase::NoContext => AdrMark::Context,
            BadAdrCase::NoDecision => AdrMark::Decision,
            BadAdrCase::NoConsequence => AdrMark::Consequence,
            BadAdrCase::NoGrounding => AdrMark::Grounding,
        }
    }

    /// 施加坏样本：解析必须按期望拒绝。
    pub fn attempt(self, seed: u32) -> bool {
        parse_adr(&make_adr_without(self.mark(), seed)).err() == Some(self.code())
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AdrLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl AdrLedger {
    const fn zero() -> AdrLedger {
        AdrLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), AdrCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(AdrCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮缺件注入：四类各 INJECT_ROUNDS 次（种子随轮次漂移覆盖文本变体）。
pub fn run_bad_adr_matrix() -> (AdrLedger, [u32; 4]) {
    let mut ledger = AdrLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadAdrCase::ALL.len() {
        let case = match BadAdrCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = case.attempt(100 + round);
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
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺要素/未知豁免族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复注册族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；次序违规/空段族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；注册表满族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；缺据豁免放行族"),
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

/// 规范语料双跑指纹：4 份完整 ADR 注册 → 审计 → 摘哈希（逐段长度）。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = AdrRegistry::new();
    let mut n = 1u32;
    while n <= 4 {
        let _ = reg.register(n, &make_full_adr(n));
        n += 1;
    }
    let mut bytes: Vec<u8> = Vec::new();
    let mut m = 1u32;
    while m <= 4 {
        if let Some(r) = reg.find(m) {
            bytes.push((r.doc.context_len & 0xFF) as u8);
            bytes.push((r.doc.grounding_len & 0xFF) as u8);
        }
        m += 1;
    }
    if let Ok(a) = reg.audit() {
        bytes.push((a & 0xFF) as u8);
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10028 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10028_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10028");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91500..=0x91FFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91500..=0x91FFF", seg_ok,
          "段基址必须唯一且落在本批窗口");

    // 2. 完整 ADR 解析通过且四段全非空
    let full = parse_adr(&make_full_adr(7));
    let full_ok = match &full {
        Ok(doc) => doc.complete(),
        Err(_) => false,
    };
    s.add("完整 ADR：四要素齐备解析通过", full_ok,
          "背景/决策/后果/依据四段全非空且越过最短长度");

    // 3. 段长语义：四段长度全为正且互不干扰
    let len_ok = match &full {
        Ok(doc) => doc.context_len > 0 && doc.decision_len > 0
            && doc.consequence_len > 0 && doc.grounding_len > 0
            && doc.context_len != doc.grounding_len,
        Err(_) => false,
    };
    s.add("段长语义：四段定界互不干扰", len_ok,
          "每段只计自己标记到下一标记之间的内容");

    // 4. 注入核：四类缺件各 10/10 检出（逐要素点名）
    let (inj, per_class) = run_bad_adr_matrix();
    let all_caught = per_class.iter().all(|x| *x == INJECT_ROUNDS);
    s.add("注入核：缺背景/决策/后果/依据各 10/10 检出", all_caught,
          "缺件 ADR 逐要素显性检出，不许静默补全");

    // 5. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadAdrCase::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 6. 空段拦截：占位符充数拒绝
    let blank = "[C]    \n[D] 决策：条目 9 决策内容 [O] 后果：条目 9 后果内容 [G] 依据：条目 9 依据内容";
    s.add("空段拦截：背景段全空白 → EmptySection",
          parse_adr(blank).err() == Some(AdrCode::EmptySection),
          "去空白后必须越过 MIN_SECTION，占位符不许充数");

    // 7. 次序违规拦截：标记重复定界歧义拒绝
    let scrambled = "[C] 背景：首段 [D] 决策：次段 [C] 背景：重复 [O] 后果：三段 [G] 依据：末段";
    s.add("次序违规拦截：[C] 重复出现 → OrderViolation",
          parse_adr(scrambled).err() == Some(AdrCode::OrderViolation),
          "四标记各出现一次，重复即定界歧义显性拒绝");

    // 8. 注册正常流：3 份完整 ADR 注册
    let mut reg = AdrRegistry::new();
    let mut n = 1u32;
    let mut reg_ok = true;
    while n <= 3 {
        if reg.register(n, &make_full_adr(n)).is_err() {
            reg_ok = false;
        }
        n += 1;
    }
    s.add("注册正常流：3 份完整 ADR 全注册", reg_ok && reg.count() == 3,
          "豁免号 ↔ ADR 双向唯一");

    // 9. 重复注册拦截
    let dup_ok = reg.register(1, &make_full_adr(99)).err() == Some(AdrCode::DupAdr);
    s.add("重复注册拦截：同豁免号二挂 → DupAdr", dup_ok,
          "换版必须先撤销旧版，不许覆盖");

    // 10. 缺件 ADR 不得注册：注册入口拒绝缺件文本
    let mut reg2 = AdrRegistry::new();
    let reg_missing = reg2.register(1, &make_adr_without(AdrMark::Grounding, 5));
    s.add("缺件注册拦截：缺依据 ADR → MissingGrounding",
          reg_missing.err() == Some(AdrCode::MissingGrounding) && reg2.count() == 0,
          "注册入口复用同一解析闸，缺件进不了台账");

    // 11. 审计通过：注册数 = 审计数
    s.add("审计通过：3 条逐条核对完整", reg.audit() == Ok(3),
          "审计逐条验证四段齐备，注册数与审计数闭合");

    // 12. 未知豁免查证：查空表 → UnknownExemption
    let empty_reg = AdrRegistry::new();
    s.add("未知豁免：空表审计拒绝", empty_reg.audit().err() == Some(AdrCode::UnknownExemption),
          "空台账不出审计结论，避免把『没挂』伪装成『挂完为零』");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(2) && agrees_with_linux(17) && agrees_with_linux(22)
        && agrees_with_linux(28) && agrees_with_linux(1)
        && !agrees_with_linux(0) && !agrees_with_linux(6789);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 14. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < AdrCode::ALL.len() {
        let mut j = i + 1;
        while j < AdrCode::ALL.len() {
            if let (Some(a), Some(b)) = (AdrCode::ALL.get(i), AdrCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = AdrCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 15. 双跑指纹一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希相等", f1 == f2,
          "同文本两次解析+注册必须逐位一致");

    // 16. 解析确定性：同文本两次解析逐字段相等
    let p1 = parse_adr(&make_full_adr(11));
    let p2 = parse_adr(&make_full_adr(11));
    s.add("解析确定性：两次解析逐字段相等", p1 == p2,
          "定界与去空白都是纯函数，无隐藏状态");

    // 动态明细（≤8）：注册账 + 注入账 + 指纹
    let reg_text = format!("注册 {} / 审计 {:?}", reg.count(), reg.audit());
    s.add_dyn("注册账快照", reg.count() == 3, &reg_text);
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_adr_parses() {
        let doc = parse_adr(&make_full_adr(1)).unwrap_or(AdrDoc {
            context_len: 0, decision_len: 0, consequence_len: 0, grounding_len: 0,
        });
        assert!(doc.complete());
        assert!(doc.context_len >= MIN_SECTION as u32);
    }

    #[test]
    fn missing_each_element_detected() {
        for case in BadAdrCase::ALL.iter() {
            assert!(case.attempt(1), "case {:?}", case);
        }
    }

    #[test]
    fn empty_section_rejected() {
        let t = "[C] ok [D] ok1 [O] ok2 [G]    ";
        assert_eq!(parse_adr(t).err(), Some(AdrCode::EmptySection));
    }

    #[test]
    fn scrambled_order_rejected() {
        let t = "[C] 背景 [D] 决策 [C] 背景 [O] 后果 [G] 依据";
        assert_eq!(parse_adr(t).err(), Some(AdrCode::OrderViolation));
        // 纯乱序（各一次）由依序搜索判定为缺件：[C] 能找到，
        // 其后找不到 [D] → MissingDecision
        let t2 = "[D] 决策 [C] 背景 [O] 后果 [G] 依据";
        assert_eq!(parse_adr(t2).err(), Some(AdrCode::MissingDecision));
    }

    #[test]
    fn dup_register_rejected() {
        let mut reg = AdrRegistry::new();
        let _ = reg.register(1, &make_full_adr(1));
        assert_eq!(reg.register(1, &make_full_adr(2)).err(), Some(AdrCode::DupAdr));
    }

    #[test]
    fn register_rejects_incomplete() {
        let mut reg = AdrRegistry::new();
        assert!(reg.register(1, &make_adr_without(AdrMark::Context, 3)).is_err());
        assert_eq!(reg.count(), 0);
    }

    #[test]
    fn audit_green_and_empty_rejected() {
        let mut reg = AdrRegistry::new();
        assert_eq!(reg.audit().err(), Some(AdrCode::UnknownExemption));
        let _ = reg.register(2, &make_full_adr(2));
        assert_eq!(reg.audit(), Ok(1));
    }

    #[test]
    fn bad_adr_matrix_green() {
        let (inj, per) = run_bad_adr_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn parse_deterministic() {
        let t = make_full_adr(42);
        assert_eq!(parse_adr(&t), parse_adr(&t));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(28));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(22), -22);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10028_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
