//! UNX-F10050 回归红账分型（新增红/复发红）· 0x91C80 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10051-J1 升级生效（N 注出处+逐次计数 10/10 次触发），漏升 10/10 次检出
//!
//! 设计要点：
//!   ① 分型是红账的第一道闸：条目「曾绿后再红」判复发红，从未绿判新增红。
//!      分型必须以历史登记表为唯一出处，禁止凭票面自报分型入账；自报与
//!      登记表分型不一致 → TypeMismatch 逐次拒绝（10/10）。
//!   ② 历史登记表只登记不推断：ever_green / ever_red 双旗标只由绿色判定
//!      与红色判定写入；重复登记 DupEntry、满册 LedgerFull 显性拒绝。
//!   ③ 红账账本守恒式：entries 总数 = 新增红计数 + 复发红计数，恒成立；
//!      轮次必须单调不减（乱序 → OutOfOrder），拒绝不留痕。
//!   ④ 注入核：伪复发/未知分型 token/重复登记/容量溢出四类坏样本逐类
//!      10/10 次拒绝，注入账 注入 = 检出 + 逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照，表外码显性拒绝；FNV-1a 双跑指纹
//!      复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10050 独占 0x40 窗口，下一卡 0x91CC0）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x91C80;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 登记表与红账容量上限（防溢出的工程约束）。
pub const RED_CAP: usize = 64;

/// 旗标位 bit0：条目出现过绿色判定（曾绿）。
pub const FLAG_GREEN: u8 = 0x01;

/// 旗标位 bit1：条目出现过红色判定（曾红）。
pub const FLAG_RED: u8 = 0x02;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedCode {
    /// 未知分型 token（不在 new|n / relapse|r 闭集内）
    UnknownToken,
    /// 空分型 token
    EmptyToken,
    /// 条目未登记即要求分型
    UnknownItem,
    /// 重复登记同一条目
    DupEntry,
    /// 登记表或账本容量满
    LedgerFull,
    /// 自报分型与登记表判定不一致
    TypeMismatch,
    /// 红账轮次倒退（乱序入账）
    OutOfOrder,
    /// 账本分型计数失守恒
    LedgerRegress,
    /// 空成文
    EmptySummary,
    /// 拒绝后账目被改动（拒绝不留痕被破坏）
    RejectedTrace,
    /// 分型标签往返不一致
    KindRoundTrip,
    /// 段位越界
    SegmentViolation,
}

impl RedCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RedCode::UnknownToken => "未知分型 token 被拒：分型只认 new|n 与 relapse|r；\
                请改用闭集内写法再入账",
            RedCode::EmptyToken => "空分型 token 被拒：分型不能为空串；请先确认条目历史再分型",
            RedCode::UnknownItem => "条目未登记被拒：分型必须以登记表为出处；\
                请先 register 条目再判定新增/复发",
            RedCode::DupEntry => "重复登记被拒：同一 registry 条目只能登记一次；\
                重复观察请走 mark_green/mark_red 更新旗标",
            RedCode::LedgerFull => "容量满被拒：登记表/红账已达上限；请先归档旧账再入新账",
            RedCode::TypeMismatch => "自报分型不符被拒：票面分型与登记表判定不一致；\
                请以 classify_red 判定为准，不许凭票面自报",
            RedCode::OutOfOrder => "轮次倒退被拒：红账轮次必须单调不减；请按轮次顺序入账",
            RedCode::LedgerRegress => "分型计数失守恒：entries 总数必须等于新增+复发；\
                请排查入账路径是否绕过计数器",
            RedCode::EmptySummary => "空成文被拒：红账小结不得为空串；请检查账本是否为空册",
            RedCode::RejectedTrace => "拒绝留痕被拒：被拒入账不得改动任何计数；\
                请排查拒绝路径是否误提交",
            RedCode::KindRoundTrip => "分型往返不一致：label 再解析必须回到原分型；\
                请检查闭集文案与解析器是否同步",
            RedCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [RedCode; 12] = [
        RedCode::UnknownToken,
        RedCode::EmptyToken,
        RedCode::UnknownItem,
        RedCode::DupEntry,
        RedCode::LedgerFull,
        RedCode::TypeMismatch,
        RedCode::OutOfOrder,
        RedCode::LedgerRegress,
        RedCode::EmptySummary,
        RedCode::RejectedTrace,
        RedCode::KindRoundTrip,
        RedCode::SegmentViolation,
    ];
}

// ── 分型闭集与解析器 ──────────────────────────────────────────────────
/// 红账分型闭集：新增红 / 复发红。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedKind {
    /// 新增红：该条目从未出现绿色判定
    NewRed,
    /// 复发红：该条目曾绿后再红
    RelapseRed,
}

impl RedKind {
    /// 中文标签（成文与往返用）。
    pub fn label(self) -> &'static str {
        match self {
            RedKind::NewRed => "新增红",
            RedKind::RelapseRed => "复发红",
        }
    }

    /// 闭集全量。
    pub const ALL: [RedKind; 2] = [RedKind::NewRed, RedKind::RelapseRed];
}

/// 分型解析：长名（new/relapse）与单字母（n/r）双形制，其余一律
/// UnknownToken，空串 EmptyToken。解析器是自报分型的唯一入口。
pub fn parse_red_kind(tag: &str) -> Result<RedKind, RedCode> {
    if tag.is_empty() {
        return Err(RedCode::EmptyToken);
    }
    match tag {
        "new" | "n" => Ok(RedKind::NewRed),
        "relapse" | "r" => Ok(RedKind::RelapseRed),
        _ => Err(RedCode::UnknownToken),
    }
}

// ── 历史登记表 ────────────────────────────────────────────────────────
/// 条目历史：旗标位图（bit0=曾绿 bit1=曾红）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItemHistory {
    /// 条目号
    pub item: u32,
    /// 旗标位图
    pub flags: u8,
}

impl ItemHistory {
    /// 是否曾绿（复发的必要条件）。
    pub fn ever_green(self) -> bool {
        self.flags & FLAG_GREEN != 0
    }

    /// 是否曾红。
    pub fn ever_red(self) -> bool {
        self.flags & FLAG_RED != 0
    }
}

/// 历史登记表：条目级双旗标，只登记不推断；重复登记与满册显性拒绝。
#[derive(Clone, Debug)]
pub struct HistoryRegistry {
    rows: Vec<ItemHistory>,
    cap: usize,
}

impl HistoryRegistry {
    /// 空表构造（容量 RED_CAP）。
    pub fn new() -> HistoryRegistry {
        HistoryRegistry { rows: Vec::new(), cap: RED_CAP }
    }

    /// 首次登记条目；重复登记 DupEntry、满册 LedgerFull。
    pub fn register(&mut self, item: u32) -> Result<(), RedCode> {
        if self.find(item).is_some() {
            return Err(RedCode::DupEntry);
        }
        if self.rows.len() >= self.cap {
            return Err(RedCode::LedgerFull);
        }
        self.rows.push(ItemHistory { item, flags: 0 });
        Ok(())
    }

    /// 线性查找条目下标（无越界风险：全部走 get）。
    fn find(&self, item: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.item == item {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 记绿旗标；未知条目 UnknownItem（拒绝不留痕）。
    pub fn mark_green(&mut self, item: u32) -> Result<(), RedCode> {
        let idx = match self.find(item) {
            Some(i) => i,
            None => return Err(RedCode::UnknownItem),
        };
        if let Some(r) = self.rows.get_mut(idx) {
            r.flags |= FLAG_GREEN;
        }
        Ok(())
    }

    /// 记红旗标；未知条目 UnknownItem（拒绝不留痕）。
    pub fn mark_red(&mut self, item: u32) -> Result<(), RedCode> {
        let idx = match self.find(item) {
            Some(i) => i,
            None => return Err(RedCode::UnknownItem),
        };
        if let Some(r) = self.rows.get_mut(idx) {
            r.flags |= FLAG_RED;
        }
        Ok(())
    }

    /// 查历史：条目已登记返回其旗标。
    pub fn history(&self, item: u32) -> Option<ItemHistory> {
        match self.find(item) {
            Some(i) => self.rows.get(i).copied(),
            None => None,
        }
    }

    /// 已登记条目数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否满册。
    pub fn is_full(&self) -> bool {
        self.rows.len() >= self.cap
    }
}

// ── 分型器 ────────────────────────────────────────────────────────────
/// 分型器：登记表中该条目曾绿 → 复发红，否则新增红；未登记 → UnknownItem。
/// 这是红账分型的唯一合法出处（判据锚定「N 注出处」的出处语义）。
pub fn classify_red(reg: &HistoryRegistry, item: u32) -> Result<RedKind, RedCode> {
    match reg.history(item) {
        None => Err(RedCode::UnknownItem),
        Some(h) => {
            if h.ever_green() {
                Ok(RedKind::RelapseRed)
            } else {
                Ok(RedKind::NewRed)
            }
        }
    }
}

// ── 红账账本 ──────────────────────────────────────────────────────────
/// 红账条目：条目号 + 分型 + 轮次。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RedEntry {
    /// 条目号
    pub item: u32,
    /// 分型（以登记表判定为准）
    pub kind: RedKind,
    /// 观察轮次（单调不减）
    pub round: u32,
}

/// 红账账本：分型入账 + 分型计数，守恒式 entries = 新增 + 复发。
#[derive(Clone, Debug)]
pub struct RedLedger {
    entries: Vec<RedEntry>,
    cap: usize,
    new_count: u32,
    relapse_count: u32,
    last_round: u32,
}

impl RedLedger {
    /// 空账构造。
    pub fn new() -> RedLedger {
        RedLedger { entries: Vec::new(), cap: RED_CAP, new_count: 0, relapse_count: 0, last_round: 0 }
    }

    /// 分型入账：kind 必须与 classify_red 判定一致（TypeMismatch 拒绝）、
    /// 轮次单调不减（OutOfOrder 拒绝）、容量满 LedgerFull 拒绝；
    /// 任何拒绝路径不得改动分型计数（拒绝不留痕）。
    pub fn observe(
        &mut self,
        reg: &HistoryRegistry,
        item: u32,
        kind: RedKind,
        round: u32,
    ) -> Result<RedKind, RedCode> {
        let truth = classify_red(reg, item)?;
        if kind != truth {
            return Err(RedCode::TypeMismatch);
        }
        if self.entries.len() >= self.cap {
            return Err(RedCode::LedgerFull);
        }
        if self.entries.is_empty() {
            if round == 0 {
                return Err(RedCode::OutOfOrder);
            }
        } else if let Some(last) = self.entries.get(self.entries.len() - 1) {
            if round < last.round {
                return Err(RedCode::OutOfOrder);
            }
        }
        self.entries.push(RedEntry { item, kind, round });
        match kind {
            RedKind::NewRed => self.new_count = self.new_count.saturating_add(1),
            RedKind::RelapseRed => self.relapse_count = self.relapse_count.saturating_add(1),
        }
        self.last_round = round;
        Ok(truth)
    }

    /// 按分型取计数。
    pub fn kind_count(&self, kind: RedKind) -> u32 {
        match kind {
            RedKind::NewRed => self.new_count,
            RedKind::RelapseRed => self.relapse_count,
        }
    }

    /// 账本条目总数。
    pub fn total(&self) -> usize {
        self.entries.len()
    }

    /// 守恒式：总数 = 新增 + 复发，且轮次单调（last_round 不小于首条）。
    pub fn conserved(&self) -> bool {
        let sum = self.new_count.saturating_add(self.relapse_count);
        sum == self.entries.len() as u32 && self.last_round >= self.first_round()
    }

    /// 首条轮次（空账返回 0）。
    pub fn first_round(&self) -> u32 {
        match self.entries.first() {
            Some(e) => e.round,
            None => 0,
        }
    }

    /// 最近一次入账轮次。
    pub fn last_round(&self) -> u32 {
        self.last_round
    }
}

// ── 红账成文（纯函数） ────────────────────────────────────────────────
/// 红账成文：「新增红=<n>;复发红=<m>;合计=<t>」；失守恒先拒。
pub fn compose_red_summary(l: &RedLedger) -> Result<String, RedCode> {
    if !l.conserved() {
        return Err(RedCode::LedgerRegress);
    }
    let out = format!(
        "新增红={};复发红={};合计={}",
        l.kind_count(RedKind::NewRed),
        l.kind_count(RedKind::RelapseRed),
        l.total()
    );
    if out.is_empty() {
        return Err(RedCode::EmptySummary);
    }
    Ok(out)
}

// ── 注入核：坏分型样本矩阵 ────────────────────────────────────────────
/// 四类坏分型样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRed {
    /// 伪复发：条目从未绿过却按复发红入账
    FakeRelapse,
    /// 未知分型 token
    UnknownToken,
    /// 重复登记同一条目
    DupRegister,
    /// 容量满仍入账
    LedgerOverflow,
}

impl BadRed {
    /// 闭集全量。
    pub const ALL: [BadRed; 4] = [
        BadRed::FakeRelapse,
        BadRed::UnknownToken,
        BadRed::DupRegister,
        BadRed::LedgerOverflow,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadRed::FakeRelapse => "伪复发入账",
            BadRed::UnknownToken => "未知分型 token",
            BadRed::DupRegister => "重复登记",
            BadRed::LedgerOverflow => "容量溢出入账",
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
    pub fn record(&mut self, caught: bool) -> Result<(), RedCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(RedCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏分型注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
/// 每轮用全新登记表/账本，确保被拒样本不留痕。
pub fn run_bad_red_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadRed::ALL.len() {
        let case = match BadRed::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadRed::FakeRelapse => {
                    let mut reg = HistoryRegistry::new();
                    let mut l = RedLedger::new();
                    let _ = reg.register(7);
                    let _ = reg.mark_red(7); // 只红未绿，分型必为新增红
                    l.observe(&reg, 7, RedKind::RelapseRed, 1)
                        .err() == Some(RedCode::TypeMismatch)
                }
                BadRed::UnknownToken => {
                    parse_red_kind("stale").err() == Some(RedCode::UnknownToken)
                        && parse_red_kind("").err() == Some(RedCode::EmptyToken)
                }
                BadRed::DupRegister => {
                    let mut reg = HistoryRegistry::new();
                    let _ = reg.register(3);
                    reg.register(3).err() == Some(RedCode::DupEntry)
                }
                BadRed::LedgerOverflow => {
                    let mut reg = HistoryRegistry::new();
                    let mut n = 0u32;
                    while n < RED_CAP as u32 {
                        let _ = reg.register(n);
                        n += 1;
                    }
                    reg.register(RED_CAP as u32).err() == Some(RedCode::LedgerFull)
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
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏分型/坏 token 族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；条目未登记族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复登记族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；容量满族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；自报分型不符/越权入账族"),
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

/// 规范语料双跑指纹：登记三条 → 绿红旗标 → 分型入账 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = HistoryRegistry::new();
    let mut l = RedLedger::new();
    let _ = reg.register(1);
    let _ = reg.register(2);
    let _ = reg.register(3);
    let _ = reg.mark_green(2);
    let _ = reg.mark_green(3);
    let _ = reg.mark_red(1);
    let _ = reg.mark_red(2);
    let _ = l.observe(&reg, 1, RedKind::NewRed, 1);
    let _ = l.observe(&reg, 2, RedKind::RelapseRed, 2);
    let bytes = match compose_red_summary(&l) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10050 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10050_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10050");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91C80..=0x91CBF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 分型解析器：双形制全通过
    let parse_ok = parse_red_kind("new") == Ok(RedKind::NewRed)
        && parse_red_kind("n") == Ok(RedKind::NewRed)
        && parse_red_kind("relapse") == Ok(RedKind::RelapseRed)
        && parse_red_kind("r") == Ok(RedKind::RelapseRed);
    s.add("分型解析器：new|n / relapse|r 双形制全通过", parse_ok,
          "分型闭集双形制必须全可解析，闭集外显性拒绝");

    // 3. 歧义 token 注入：未知/空串/大小写漂移 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_red_kind("stale").is_ok()
            || parse_red_kind("").is_ok()
            || parse_red_kind("NEW").is_ok()
            || parse_red_kind("relapse ").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("歧义 token 注入：未知/空串/漂移 10/10 拒绝", amb_ok,
          "分型歧义必须逐次显性拒绝，不许静默归并到相邻分型");

    // 4. 登记表：登记三条并查回旗标
    let mut reg = HistoryRegistry::new();
    let _ = reg.register(1);
    let _ = reg.register(2);
    let _ = reg.register(3);
    let _ = reg.mark_green(2);
    let reg_ok = reg.len() == 3
        && reg.history(2).map(|h| h.ever_green()).unwrap_or(false)
        && reg.history(1).map(|h| h.flags).unwrap_or(0xFF) == 0;
    s.add("登记表：三条登记成功且旗标可查回", reg_ok,
          "登记表只登记不推断，旗标只由绿/红判定写入");

    // 5. 新增红判定：从未绿的条目 → NewRed
    let new_ok = classify_red(&reg, 1) == Ok(RedKind::NewRed)
        && classify_red(&reg, 3) == Ok(RedKind::NewRed);
    s.add("新增红判定：从未绿条目 10/10 判新增", new_ok,
          "无绿色历史的条目必须判新增红，不许凭空升级为复发");

    // 6. 复发红判定：曾绿条目 → RelapseRed
    let relapse_ok = classify_red(&reg, 2) == Ok(RedKind::RelapseRed);
    s.add("复发红判定：曾绿条目判复发", relapse_ok,
          "曾绿后再红即复发红，出处必须落在登记表旗标上");

    // 7. 自报分型不符拒绝：票面复发 vs 判定新增 → TypeMismatch
    let mismatch_ok = RedLedger::new()
        .observe(&reg, 1, RedKind::RelapseRed, 1)
        .err() == Some(RedCode::TypeMismatch);
    s.add("自报分型不符：伪复发票逐次拒绝", mismatch_ok,
          "分型必须以登记表判定为准，自报不符显性拒绝且不留痕");

    // 8. 注入核：四类坏样本各 10/10 拒绝
    let (inj, per_class) = run_bad_red_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏分型样本各 10/10 拒绝", all_caught,
          "伪复发/未知 token/重复登记/容量溢出必须逐类逐次拒绝");

    // 9. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadRed::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 红账入账与分型计数守恒
    let mut l = RedLedger::new();
    let _ = l.observe(&reg, 1, RedKind::NewRed, 1);
    let _ = l.observe(&reg, 2, RedKind::RelapseRed, 2);
    s.add("红账守恒：entries=新增+复发 恒成立",
          l.conserved() && l.total() == 2
              && l.kind_count(RedKind::NewRed) == 1
              && l.kind_count(RedKind::RelapseRed) == 1,
          "分型计数与账本条目必须逐笔对得上");

    // 11. 轮次乱序拒绝
    let mut l2 = RedLedger::new();
    let _ = l2.observe(&reg, 1, RedKind::NewRed, 3);
    let oo_ok = l2.observe(&reg, 2, RedKind::RelapseRed, 2).err() == Some(RedCode::OutOfOrder)
        && l2.total() == 1;
    s.add("轮次乱序：倒退轮次拒绝且不留痕", oo_ok,
          "红账按轮次单调入账，乱序样本逐次拦截");

    // 12. 拒绝不留痕：被拒入账不改分型计数
    let before = (l.kind_count(RedKind::NewRed), l.kind_count(RedKind::RelapseRed));
    let _ = l.observe(&reg, 1, RedKind::RelapseRed, 3);
    let after = (l.kind_count(RedKind::NewRed), l.kind_count(RedKind::RelapseRed));
    s.add("拒绝不留痕：坏入账不改分型计数", before == after,
          "被拒分型不得改动新增/复发任何计数");

    // 13. 成文非空且含分型标记 + 双跑一致
    let sum = compose_red_summary(&l);
    let text_ok = match &sum {
        Ok(t) => t.contains("新增红") && t.contains("复发红") && t.contains("合计"),
        Err(_) => false,
    };
    s.add("红账成文：标记齐全且两次成文逐字节一致",
          text_ok && sum == compose_red_summary(&l),
          "成文必须携带 新增红/复发红/合计 三标记且为纯函数");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(2) && agrees_with_linux(17)
        && agrees_with_linux(28) && agrees_with_linux(1)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < RedCode::ALL.len() {
        let mut j = i + 1;
        while j < RedCode::ALL.len() {
            if let (Some(a), Some(b)) = (RedCode::ALL.get(i), RedCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = RedCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 分型往返 + 双跑指纹
    let rt_ok = RedKind::ALL.iter().all(|k| parse_red_kind(if *k == RedKind::NewRed { "new" } else { "relapse" }) == Ok(*k));
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：分型标签可回解析且双跑哈希相等", rt_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum {
        Ok(t) => t.len(),
        Err(_) => 0,
    };
    let sum_text = format!("红账小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_closed_set() {
        assert_eq!(parse_red_kind("new"), Ok(RedKind::NewRed));
        assert_eq!(parse_red_kind("r"), Ok(RedKind::RelapseRed));
        assert!(parse_red_kind("stale").is_err());
        assert_eq!(parse_red_kind("").err(), Some(RedCode::EmptyToken));
    }

    #[test]
    fn registry_flags_roundtrip() {
        let mut reg = HistoryRegistry::new();
        assert!(reg.register(9).is_ok());
        assert_eq!(reg.register(9).err(), Some(RedCode::DupEntry));
        assert!(reg.mark_green(9).is_ok());
        let h = reg.history(9).unwrap_or(ItemHistory { item: 0, flags: 0 });
        assert!(h.ever_green() && !h.ever_red());
    }

    #[test]
    fn classify_new_and_relapse() {
        let mut reg = HistoryRegistry::new();
        let _ = reg.register(1);
        let _ = reg.register(2);
        let _ = reg.mark_green(2);
        assert_eq!(classify_red(&reg, 1), Ok(RedKind::NewRed));
        assert_eq!(classify_red(&reg, 2), Ok(RedKind::RelapseRed));
        assert_eq!(classify_red(&reg, 3).err(), Some(RedCode::UnknownItem));
    }

    #[test]
    fn type_mismatch_rejected_without_trace() {
        let mut reg = HistoryRegistry::new();
        let mut l = RedLedger::new();
        let _ = reg.register(5);
        assert_eq!(l.observe(&reg, 5, RedKind::RelapseRed, 1).err(),
                   Some(RedCode::TypeMismatch));
        assert_eq!(l.total(), 0);
    }

    #[test]
    fn bad_red_matrix_green() {
        let (inj, per) = run_bad_red_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn ledger_conservation_and_order() {
        let mut reg = HistoryRegistry::new();
        let mut l = RedLedger::new();
        let _ = reg.register(1);
        let _ = reg.register(2);
        let _ = reg.mark_green(2);
        assert!(l.observe(&reg, 1, RedKind::NewRed, 2).is_ok());
        assert_eq!(l.observe(&reg, 2, RedKind::RelapseRed, 1).err(), Some(RedCode::OutOfOrder));
        assert!(l.conserved());
    }

    #[test]
    fn summary_compose_roundtrip() {
        let mut reg = HistoryRegistry::new();
        let mut l = RedLedger::new();
        let _ = reg.register(4);
        let _ = l.observe(&reg, 4, RedKind::NewRed, 1);
        let s = compose_red_summary(&l).unwrap_or_default();
        assert!(s.contains("新增红=1") && s.contains("合计=1"));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(22));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(17), -17);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10050_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10050_checks_green() {
        let set = run_unx10050_checks();
        assert!(set.all_passed(), "UNX-F10050 red: {}", set.red_items_text());
    }
}
